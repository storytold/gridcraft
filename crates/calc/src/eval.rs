//! The formula evaluator.
//!
//! Expressions evaluate to [`Ev`]: a value, a reference (one or more areas, kept unresolved so
//! `ROW`, `OFFSET`, `INDEX`, `:` and friends can work on references) or a lambda. Cell reads go
//! through [`Host`], which lets the recalculation engine evaluate dirty precedents on demand.

use std::sync::Arc;

use gridcraft_core::{Array, CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, compare};
use gridcraft_formula::{BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, UnOp};
use gridcraft_functions::{Arg, Ctx};
use gridcraft_model::Workbook;

/// Largest array a reference may expand to.
pub const MAX_CELLS: u64 = 16_000_000;
/// References up to this many cells (one whole row) are always read at their full size.
const FULL_SIZE_CELLS: u64 = 16_384;
/// Bounded references up to this many cells (one column's worth) are read at their full size too, as
/// Excel does, so their shape lines up with the arrays next to them (`D3:AZ400*B3:B400` is 398 by
/// 49 even when the data ends at column F) and their blanks count. References that run to the edge
/// of the sheet (`A:A`, `A2:A1048576`, several whole rows) and larger blocks stop at the sheet's used
/// range, so that formulas over them stay fast.
const FULL_SIZE_BOUNDED_CELLS: u64 = 1_048_576;
const MAX_DEPTH: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Area {
    pub sheet: usize,
    pub range: RangeRef,
}

#[derive(Clone, Debug)]
pub struct Lambda {
    pub params: Vec<String>,
    pub body: Expr,
    pub env: Vec<(String, Ev)>,
}

#[derive(Clone, Debug)]
pub enum Ev {
    V(Value),
    R(Vec<Area>),
    L(Arc<Lambda>),
}

impl From<Value> for Ev {
    fn from(v: Value) -> Self {
        Ev::V(v)
    }
}

fn err(e: CellError) -> Ev {
    Ev::V(Value::Error(e))
}

/// What the evaluator needs from its surroundings.
pub trait Host {
    fn workbook(&self) -> &Workbook;
    /// The current value of a cell (evaluating it first if it is dirty).
    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value;
    fn now_serial(&self) -> f64;
    fn random(&mut self) -> f64;
    /// Called when a volatile or dynamic-reference function is used.
    fn note_volatile(&mut self) {}
    /// The spill range of the dynamic array anchored at `anchor` (`None`: it doesn't spill).
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        self.workbook().sheet(sheet)?.spill_ranges.get(&anchor).copied()
    }
    /// Values of a block, row-major. Hosts can override with a sparse fast path.
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        range.iter().map(|c| self.cell_value(sheet, c)).collect()
    }
    /// The sheet's used range (`Sheet::used_range`, which walks every stored row). Hosts that
    /// keep the workbook unchanged while evaluating can remember it.
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        self.workbook().sheet(sheet).and_then(|s| s.used_range())
    }
    /// Calls `f` on each stored value of a block, row-major (formula cells evaluated first),
    /// until `f` returns false. `false` when this host can't (callers use `range_values`).
    fn visit_range(&mut self, _sheet: usize, _range: RangeRef, _f: &mut dyn FnMut(&Value) -> bool) -> bool {
        false
    }
}

pub struct Evaluator<'h> {
    pub host: &'h mut dyn Host,
    pub sheet: usize,
    pub at: CellRef,
    env: Vec<(String, Ev)>,
    depth: usize,
}

struct FnCtx<'a, 'h> {
    ev: &'a mut Evaluator<'h>,
    /// The criteria argument of a database function, when it is a reference.
    criteria: Option<Area>,
    /// The LAMBDA given as the function argument of GROUPBY/PIVOTBY.
    lambda: Option<Arc<Lambda>>,
}

impl Ctx for FnCtx<'_, '_> {
    fn date_system(&self) -> gridcraft_core::DateSystem {
        self.ev.host.workbook().date_system
    }
    fn now_serial(&self) -> f64 {
        self.ev.host.now_serial()
    }
    fn random(&mut self) -> f64 {
        self.ev.host.random()
    }
    /// Excel's "formula as criteria": the formula is written for the first record, so its relative
    /// references move down to `record`'s row, and the database's labels name its fields.
    /// Known gap: the shifted references are not dependencies of the calling cell. One that lands
    /// outside both the database and the criteria cell's own precedents (say `=G2<H2` with column H
    /// outside the database) is read here but not tracked, so editing that cell doesn't recalculate
    /// the D-function until something else does.
    fn computed_criterion(&mut self, row: usize, col: usize, db: &Array, record: usize) -> Option<bool> {
        let area = self.criteria?;
        let at = area.range.start.offset(row as i64, col as i64).filter(|c| area.range.contains(*c))?;
        let expr = self.ev.host.workbook().sheet(area.sheet)?.cell(at)?.formula.as_ref()?.expr_arc()?;
        let shifted = gridcraft_formula::adjust::shift_relative((*expr).clone(), record as i64 - 1, 0);
        let env = (0..db.cols)
            .filter_map(|c| match db.get(0, c) {
                Some(Value::Text(t)) if !t.trim().is_empty() => {
                    Some((t.trim().to_string(), Ev::V(db.get(record, c).cloned().unwrap_or(Value::Empty))))
                }
                _ => None,
            })
            .collect();
        let mut ev = Evaluator { host: &mut *self.ev.host, sheet: area.sheet, at, env, depth: self.ev.depth };
        Some(matches!(ev.value(&shifted).scalar().to_bool(), Ok(true)))
    }
    fn has_lambda(&self) -> bool {
        self.lambda.is_some()
    }
    /// A LAMBDA must give one value per group; an array result is #CALC!.
    fn call_lambda(&mut self, arg: Value) -> Option<Value> {
        let l = self.lambda.clone()?;
        let [p] = l.params.as_slice() else { return Some(Value::Error(CellError::Value)) };
        let r = self.ev.call_lambda_values(&l, vec![(p.clone(), Ev::V(arg))]);
        Some(match self.ev.deref(r) {
            Value::Array(a) if a.data.len() != 1 => Value::Error(CellError::Calc),
            v => v.scalar(),
        })
    }
}

impl<'h> Evaluator<'h> {
    pub fn new(host: &'h mut dyn Host, sheet: usize, at: CellRef) -> Self {
        Evaluator { host, sheet, at, env: Vec::new(), depth: 0 }
    }

    /// Evaluates to a value (references are dereferenced; multi-cell → array).
    pub fn value(&mut self, e: &Expr) -> Value {
        let ev = self.eval(e);
        self.deref(ev)
    }

    fn resolve_sheet(&self, s: &SheetSel) -> Result<Vec<usize>, CellError> {
        let wb = self.host.workbook();
        match s {
            SheetSel::Current => Ok(vec![self.sheet]),
            SheetSel::Named(n) => wb.sheet_index(n).map(|i| vec![i]).ok_or(CellError::Ref),
            SheetSel::Span(a, b) => {
                let (i, j) = (wb.sheet_index(a).ok_or(CellError::Ref)?, wb.sheet_index(b).ok_or(CellError::Ref)?);
                Ok((i.min(j)..=i.max(j)).collect())
            }
        }
    }

    pub fn reference_areas(&self, r: &Reference) -> Result<Vec<Area>, CellError> {
        let range = r.range();
        Ok(self.resolve_sheet(&r.sheet)?.into_iter().map(|sheet| Area { sheet, range }).collect())
    }

    /// Dereferences: one cell → its value; a block → an array; several areas → `#VALUE!`.
    ///
    /// A block keeps its full shape (blanks included) when it holds at most `FULL_SIZE_CELLS`
    /// cells, or when it holds at most `FULL_SIZE_BOUNDED_CELLS` cells (one full column's worth)
    /// and does not run to the sheet's last row or column. Every other block (larger, or a
    /// bigger one that runs to the sheet's edge, such as `A:A` or `1:1`) is trimmed to the
    /// sheet's used range before its values are read. An array over `MAX_CELLS` is `#NUM!`.
    pub fn deref(&mut self, ev: Ev) -> Value {
        match ev {
            Ev::V(v) => v,
            Ev::L(_) => Value::Error(CellError::Calc),
            Ev::R(areas) => match areas.as_slice() {
                [a] => self.area_value(a),
                _ => Value::Error(CellError::Value),
            },
        }
    }

    pub fn area_value(&mut self, a: &Area) -> Value {
        if a.range.is_single() {
            return self.host.cell_value(a.sheet, a.range.start);
        }
        // A reference keeps its size, so the array has its shape and its blanks count
        // (`=A1:A10`, `SUMPRODUCT(--(A1:A10=""))`).
        let cells = u64::from(a.range.height()) * u64::from(a.range.width());
        // `A:A`, `1:1` and `A2:A1048576` run to the edge of the sheet: they stand for "the rest of
        // the column", not for a block of a chosen size.
        let to_edge = a.range.end.row == MAX_ROWS - 1 || a.range.end.col == MAX_COLS - 1;
        let full_size = cells <= FULL_SIZE_CELLS || (!to_edge && cells <= FULL_SIZE_BOUNDED_CELLS);
        let range = if full_size { a.range } else { self.trim(a) };
        let (h, w) = (range.height() as usize, range.width() as usize);
        if (h as u64) * (w as u64) > MAX_CELLS {
            return Value::Error(CellError::Num);
        }
        let data = self.host.range_values(a.sheet, range);
        Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
    }

    /// Trims the bottom/right of an area to the sheet's used range (keeps the top-left so
    /// positions stay right).
    pub fn trim(&mut self, a: &Area) -> RangeRef {
        if self.host.workbook().sheet(a.sheet).is_none() {
            return a.range;
        }
        let used = self.host.used_range(a.sheet);
        let (mut r1, mut c1) = (a.range.end.row, a.range.end.col);
        match used {
            Some(u) => {
                r1 = r1.min(u.end.row.max(a.range.start.row));
                c1 = c1.min(u.end.col.max(a.range.start.col));
            }
            None => {
                r1 = a.range.start.row;
                c1 = a.range.start.col;
            }
        }
        RangeRef::new(a.range.start, CellRef::new(r1, c1))
    }

    pub fn eval(&mut self, e: &Expr) -> Ev {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return err(CellError::Calc);
        }
        let r = self.eval_inner(e);
        self.depth -= 1;
        r
    }

    fn eval_inner(&mut self, e: &Expr) -> Ev {
        match e {
            Expr::Number(n) => Ev::V(Value::number(*n)),
            Expr::Text(t) => Ev::V(Value::Text(t.clone())),
            Expr::Bool(b) => Ev::V(Value::Bool(*b)),
            Expr::Error(x) => err(*x),
            Expr::Missing => Ev::V(Value::Empty),
            Expr::Paren(x) => self.eval(x),
            Expr::Array(rows) => {
                let h = rows.len();
                let w = rows.first().map_or(0, Vec::len);
                let mut data = Vec::with_capacity(h * w);
                for row in rows {
                    for el in row {
                        let v = self.value(el);
                        data.push(v);
                    }
                }
                Ev::V(Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
            }
            Expr::Ref(r) => match self.reference_areas(r) {
                Ok(a) => Ev::R(a),
                Err(x) => err(x),
            },
            Expr::Name(n) => self.name(n),
            Expr::Struct(s) => self.structured(s),
            Expr::Unary(op, x) => self.unary(*op, x),
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
            Expr::Call(name, args) => self.call(name, args),
            Expr::Invoke(callee, args) => {
                let f = self.eval(callee);
                match f {
                    Ev::L(l) => self.apply_lambda(&l, args),
                    Ev::V(Value::Error(x)) => err(x),
                    _ => err(CellError::Value),
                }
            }
        }
    }

    fn name(&mut self, n: &str) -> Ev {
        if let Some((_, v)) = self.env.iter().rev().find(|(k, _)| gridcraft_formula::same_name(k, n)) {
            return v.clone();
        }
        let wb = self.host.workbook();
        // Sheet-qualified `Sheet1!Name`.
        let (scope_sheet, bare) = match n.split_once('!') {
            Some((s, b)) => match wb.sheet_index(&gridcraft_core::unquote_sheet(s)) {
                Some(i) => (i, b),
                None => return err(CellError::Name),
            },
            None => (self.sheet, n),
        };
        if let Some(def) = wb.name(bare, scope_sheet) {
            let text = def.formula.clone();
            let scope = def.scope.unwrap_or(self.sheet);
            return match gridcraft_formula::parse_name(&text) {
                Ok(expr) => {
                    // Relative references in a name are relative to A1: used here, they move by
                    // this cell's offset from it.
                    let expr = if gridcraft_formula::adjust::has_relative_refs(&expr) {
                        gridcraft_formula::adjust::shift_relative_wrapping(expr, self.at.row as i64, self.at.col as i64)
                    } else {
                        expr
                    };
                    let saved = self.sheet;
                    // Unqualified references in a name refer to the scope sheet.
                    self.sheet = scope;
                    let r = self.eval(&expr);
                    self.sheet = saved;
                    r
                }
                Err(_) => err(CellError::Name),
            };
        }
        // A table name alone means its data body.
        if let Some((si, ti)) = wb.table(bare)
            && let Some(t) = wb.sheet(si).and_then(|s| s.tables.get(ti))
            && let Some(d) = t.data_range()
        {
            return Ev::R(vec![Area { sheet: si, range: d }]);
        }
        err(CellError::Name)
    }

    fn structured(&mut self, s: &StructRef) -> Ev {
        let wb = self.host.workbook();
        let found = if s.table.is_empty() {
            wb.sheet(self.sheet).and_then(|sh| sh.tables.iter().position(|t| t.range.contains(self.at)).map(|ti| (self.sheet, ti)))
        } else {
            wb.table(&s.table)
        };
        let Some((si, ti)) = found else { return err(CellError::Ref) };
        let Some(t) = wb.sheet(si).and_then(|sh| sh.tables.get(ti)) else { return err(CellError::Ref) };
        let mut r0 = t.range.start.row;
        let mut r1 = t.range.end.row;
        let data = t.data_range();
        let items = &s.specifiers;
        if items.is_empty() || items == &[StructItem::Data] {
            match data {
                Some(d) => {
                    r0 = d.start.row;
                    r1 = d.end.row;
                }
                None => return err(CellError::Ref),
            }
        } else if items.contains(&StructItem::ThisRow) {
            if self.at.row < t.range.start.row || self.at.row > t.range.end.row {
                return err(CellError::Value);
            }
            r0 = self.at.row;
            r1 = self.at.row;
        } else if !items.contains(&StructItem::All) {
            let has = |i: StructItem| items.contains(&i);
            let hdr = t.header_row.then_some(t.range.start.row);
            let tot = t.totals_row.then_some(t.range.end.row);
            let mut lo = u32::MAX;
            let mut hi = 0;
            let mut add = |a: u32, b: u32| {
                lo = lo.min(a);
                hi = hi.max(b);
            };
            if has(StructItem::Headers) {
                match hdr {
                    Some(h) => add(h, h),
                    None => return err(CellError::Ref),
                }
            }
            if has(StructItem::Data)
                && let Some(d) = data
            {
                add(d.start.row, d.end.row);
            }
            if has(StructItem::Totals) {
                match tot {
                    Some(h) => add(h, h),
                    None => return err(CellError::Ref),
                }
            }
            if lo > hi {
                return err(CellError::Ref);
            }
            r0 = lo;
            r1 = hi;
        }
        let (mut c0, mut c1) = (t.range.start.col, t.range.end.col);
        if let Some(a) = &s.col_start {
            let Some(i) = t.column_index(a) else { return err(CellError::Ref) };
            c0 = t.range.start.col + i as u32;
            c1 = c0;
            if let Some(b) = &s.col_end {
                let Some(j) = t.column_index(b) else { return err(CellError::Ref) };
                let cj = t.range.start.col + j as u32;
                c0 = c0.min(cj);
                c1 = c1.max(cj);
            }
        }
        Ev::R(vec![Area { sheet: si, range: RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)) }])
    }

    fn unary(&mut self, op: UnOp, x: &Expr) -> Ev {
        if let UnOp::Trim(mode) = op {
            return self.trim_ref(mode, x);
        }
        if op == UnOp::Spill {
            return match self.eval(x) {
                Ev::R(areas) => match areas.as_slice() {
                    [a] if a.range.is_single() => match self.host.spill_range(a.sheet, a.range.start) {
                        Some(range) => Ev::R(vec![Area { sheet: a.sheet, range }]),
                        None => err(CellError::Ref),
                    },
                    _ => err(CellError::Ref),
                },
                Ev::V(Value::Error(e)) => err(e),
                _ => err(CellError::Ref),
            };
        }
        if op == UnOp::At {
            let ev = self.eval(x);
            return match ev {
                Ev::R(areas) => match areas.as_slice() {
                    [a] => match self.implicit_intersect(a) {
                        Some(c) => Ev::V(self.host.cell_value(a.sheet, c)),
                        None => err(CellError::Value),
                    },
                    _ => err(CellError::Value),
                },
                Ev::V(v) => Ev::V(v.scalar()),
                l => l,
            };
        }
        let v = self.value(x);
        let sys = self.host.workbook().date_system;
        Ev::V(map_unary(
            &v,
            &|n| match op {
                UnOp::Neg => Value::number(-n),
                UnOp::Plus => Value::number(n),
                UnOp::Percent => Value::number(n / 100.0),
                UnOp::At | UnOp::Spill | UnOp::Trim(_) => Value::number(n),
            },
            op == UnOp::Plus,
            sys,
        ))
    }

    fn implicit_intersect(&self, a: &Area) -> Option<CellRef> {
        let r = a.range;
        if r.is_single() {
            return Some(r.start);
        }
        if r.width() == 1 && r.start.row <= self.at.row && self.at.row <= r.end.row {
            return Some(CellRef::new(self.at.row, r.start.col));
        }
        if r.height() == 1 && r.start.col <= self.at.col && self.at.col <= r.end.col {
            return Some(CellRef::new(r.start.row, self.at.col));
        }
        None
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> Ev {
        match op {
            BinOp::Range => {
                let (x, y) = (self.eval(a), self.eval(b));
                let (Ev::R(xa), Ev::R(ya)) = (&x, &y) else {
                    if let Ev::V(Value::Error(e)) = x {
                        return err(e);
                    }
                    if let Ev::V(Value::Error(e)) = y {
                        return err(e);
                    }
                    return err(CellError::Value);
                };
                let all: Vec<&Area> = xa.iter().chain(ya.iter()).collect();
                let Some(first) = all.first() else { return err(CellError::Ref) };
                if all.iter().any(|ar| ar.sheet != first.sheet) {
                    return err(CellError::Value);
                }
                let mut r = first.range;
                for ar in &all {
                    r = r.union(&ar.range);
                }
                Ev::R(vec![Area { sheet: first.sheet, range: r }])
            }
            BinOp::Union => {
                let (x, y) = (self.eval(a), self.eval(b));
                match (x, y) {
                    (Ev::R(mut xa), Ev::R(ya)) => {
                        xa.extend(ya);
                        Ev::R(xa)
                    }
                    (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => err(e),
                    _ => err(CellError::Value),
                }
            }
            BinOp::Intersect => {
                let (x, y) = (self.eval(a), self.eval(b));
                match (x, y) {
                    (Ev::R(xa), Ev::R(ya)) => {
                        let mut out = Vec::new();
                        for p in &xa {
                            for q in &ya {
                                if p.sheet == q.sheet
                                    && let Some(i) = p.range.intersection(&q.range)
                                {
                                    out.push(Area { sheet: p.sheet, range: i });
                                }
                            }
                        }
                        if out.is_empty() { err(CellError::Null) } else { Ev::R(out) }
                    }
                    (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => err(e),
                    _ => err(CellError::Value),
                }
            }
            _ => {
                // A long chain such as A1+A2+…+A500 nests to the left: fold it in a loop instead
                // of one level of recursion per operator (past the depth limit it was #CALC!).
                let mut rest = vec![(op, b)];
                let mut left = a;
                while let Expr::Binary(o, l, r) = left
                    && !matches!(o, BinOp::Range | BinOp::Union | BinOp::Intersect)
                {
                    rest.push((*o, r.as_ref()));
                    left = l.as_ref();
                }
                let mut acc = self.value(left);
                while let Some((o, r)) = rest.pop() {
                    let y = self.value(r);
                    acc = binary_values(o, &acc, &y, self.host.workbook().date_system);
                }
                Ev::V(acc)
            }
        }
    }

    // ------------------------------------------------------------------ calls

    fn arg_ev(&mut self, args: &[Expr], i: usize) -> Option<Ev> {
        args.get(i).map(|e| self.eval(e))
    }
    fn arg_val(&mut self, args: &[Expr], i: usize) -> Option<Value> {
        args.get(i).map(|e| self.value(e))
    }
    fn arg_num(&mut self, args: &[Expr], i: usize, default: f64) -> Result<f64, CellError> {
        match args.get(i) {
            None | Some(Expr::Missing) => Ok(default),
            Some(e) => {
                let v = self.value(e);
                v.scalar().to_number()
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Ev {
        match name {
            "IF" => {
                if args.is_empty() || args.len() > 3 {
                    return err(CellError::Value);
                }
                let c = self.arg_val(args, 0).unwrap_or_default();
                if let Value::Array(arr) = &c {
                    // Array condition: element-wise.
                    let t = self.arg_val(args, 1).unwrap_or(Value::Bool(true));
                    let f = if args.len() > 2 { self.arg_val(args, 2).unwrap_or(Value::Bool(false)) } else { Value::Bool(false) };
                    return Ev::V(zip3(arr, &t, &f, |c, t, f| match c.to_bool() {
                        Ok(true) => t.clone(),
                        Ok(false) => f.clone(),
                        Err(e) => Value::Error(e),
                    }));
                }
                match c.to_bool() {
                    Ok(true) => match args.get(1) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(true)),
                    },
                    Ok(false) => match args.get(2) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(false)),
                    },
                    Err(e) => err(e),
                }
            }
            "IFS" => {
                if args.len() < 2 || !args.len().is_multiple_of(2) {
                    return err(CellError::Value);
                }
                for pair in args.chunks(2) {
                    let [c, v] = pair else { break };
                    let cv = self.value(c);
                    match cv.scalar().to_bool() {
                        Ok(true) => return self.eval(v),
                        Ok(false) => {}
                        Err(e) => return err(e),
                    }
                }
                err(CellError::NA)
            }
            "IFERROR" | "IFNA" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_ev(args, 0).unwrap_or(Ev::V(Value::Empty));
                let na_only = name == "IFNA";
                let catch = |e: CellError| if na_only { e == CellError::NA } else { true };
                match &v {
                    Ev::V(Value::Error(e)) if catch(*e) => self.eval(&args[1]),
                    Ev::V(Value::Array(a)) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                        let alt = self.arg_val(args, 1).unwrap_or_default();
                        let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                        Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                    }
                    Ev::R(_) => {
                        let val = self.deref(v.clone());
                        match val {
                            Value::Error(e) if catch(e) => self.eval(&args[1]),
                            Value::Array(ref a) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                                let alt = self.arg_val(args, 1).unwrap_or_default();
                                let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                                Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                            }
                            _ => v,
                        }
                    }
                    _ => v,
                }
            }
            "CHOOSE" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let index = self.value(&args[0]);
                if let Value::Array(_) = index {
                    // An array of indexes picks element by element: `CHOOSE({1,2},A1:A3,B1:B3)`
                    // is a two-column array. The library CHOOSE lifts over its arguments.
                    let Some(spec) = gridcraft_functions::lookup(name) else { return err(CellError::Name) };
                    let mut evaluated = vec![Arg { value: index, from_ref: false }];
                    for a in &args[1..] {
                        let value = self.value(a);
                        evaluated.push(Arg { value, from_ref: false });
                    }
                    let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                    return gridcraft_functions::call(spec, &evaluated, &mut ctx).pipe(Ev::V);
                }
                let i = match index.to_number() {
                    Ok(n) => n.trunc() as i64,
                    Err(e) => return err(e),
                };
                if i < 1 || i as usize >= args.len() {
                    return err(CellError::Value);
                }
                self.eval(&args[i as usize])
            }
            "SWITCH" => {
                if args.len() < 3 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default().scalar();
                if let Value::Error(e) = v {
                    return err(e);
                }
                let rest = &args[1..];
                let mut i = 0;
                while i + 1 < rest.len() {
                    let cand = self.value(&rest[i]).scalar();
                    if !cand.is_error() && compare(&v, &cand) == std::cmp::Ordering::Equal && same_kind(&v, &cand) {
                        return self.eval(&rest[i + 1]);
                    }
                    i += 2;
                }
                if rest.len() % 2 == 1 { rest.last().map_or(err(CellError::NA), |d| self.eval(d)) } else { err(CellError::NA) }
            }
            "ROW" | "COLUMN" => {
                let is_row = name == "ROW";
                if args.is_empty() || matches!(args.first(), Some(Expr::Missing)) {
                    return Ev::V(Value::number(if is_row { self.at.row + 1 } else { self.at.col + 1 } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        let Some(a) = a.first() else { return err(CellError::Ref) };
                        let r = self.trim_lines(a);
                        if is_row {
                            let rows: Vec<Value> = (r.start.row..=r.end.row).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if rows.len() == 1 { Ev::V(rows.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::column(rows).into()) }
                        } else {
                            let cols: Vec<Value> = (r.start.col..=r.end.col).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if cols.len() == 1 { Ev::V(cols.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::row(cols).into()) }
                        }
                    }
                    Ev::V(Value::Error(e)) => err(e),
                    _ => err(CellError::Value),
                }
            }
            "ROWS" | "COLUMNS" => {
                if args.len() != 1 {
                    return err(CellError::Value);
                }
                let is_rows = name == "ROWS";
                match self.eval(&args[0]) {
                    Ev::R(a) => match a.as_slice() {
                        [a] => Ev::V(Value::number(if is_rows { a.range.height() } else { a.range.width() } as f64)),
                        _ => err(CellError::Ref),
                    },
                    Ev::V(Value::Array(arr)) => Ev::V(Value::number(if is_rows { arr.rows } else { arr.cols } as f64)),
                    Ev::V(Value::Error(e)) => err(e),
                    Ev::V(_) => Ev::V(Value::Number(1.0)),
                    Ev::L(_) => err(CellError::Value),
                }
            }
            "AREAS" => match args.first().map(|a| self.eval(a)) {
                Some(Ev::R(a)) => Ev::V(Value::number(a.len() as f64)),
                Some(Ev::V(Value::Error(e))) => err(e),
                _ => err(CellError::Value),
            },
            "ISREF" => Ev::V(Value::Bool(matches!(args.first().map(|a| self.eval(a)), Some(Ev::R(_))))),
            "ISFORMULA" | "FORMULATEXT" => {
                let Some(Ev::R(a)) = args.first().map(|a| self.eval(a)) else { return err(CellError::NA) };
                let Some(a) = a.first() else { return err(CellError::NA) };
                let f = self.host.workbook().sheet(a.sheet).and_then(|s| s.cell(a.range.start)).and_then(|c| c.formula.clone());
                if name == "ISFORMULA" {
                    Ev::V(Value::Bool(f.is_some()))
                } else {
                    match f {
                        Some(f) => Ev::V(Value::text(format!("={}", f.text))),
                        None => err(CellError::NA),
                    }
                }
            }
            "SHEET" | "SHEETS" => {
                let wb = self.host.workbook();
                if args.is_empty() {
                    return Ev::V(Value::number(if name == "SHEET" { self.sheet + 1 } else { wb.sheets.len() } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        if name == "SHEET" {
                            a.first().map_or(err(CellError::Ref), |a| Ev::V(Value::number(a.sheet as f64 + 1.0)))
                        } else {
                            let mut s: Vec<usize> = a.iter().map(|x| x.sheet).collect();
                            s.dedup();
                            Ev::V(Value::number(s.len() as f64))
                        }
                    }
                    Ev::V(Value::Text(t)) if name == "SHEET" => {
                        let wb = self.host.workbook();
                        wb.sheet_index(&t).map_or(err(CellError::NA), |i| Ev::V(Value::number(i as f64 + 1.0)))
                    }
                    _ => err(CellError::NA),
                }
            }
            "OFFSET" => self.offset(args),
            "INDIRECT" => self.indirect(args),
            "INDEX" => self.index(args),
            "XLOOKUP" => self.xlookup(args),
            "CELL" => self.cell_info(args),
            "TRIMRANGE" => self.trimrange(args),
            "SUBTOTAL" | "AGGREGATE" => self.subtotal(name, args),
            "TEXT" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default();
                let f = self.arg_val(args, 1).unwrap_or_default();
                let sys = self.host.workbook().date_system;
                let one = |v: &Value, f: &Value| -> Value {
                    if let Value::Error(e) = v {
                        return Value::Error(*e);
                    }
                    let code = match f.to_text() {
                        Ok(c) => c,
                        Err(e) => return Value::Error(e),
                    };
                    match gridcraft_numfmt::text_function(v, &code, sys) {
                        Ok(s) => Value::text(s),
                        Err(e) => Value::Error(e),
                    }
                };
                Ev::V(zip2(&v, &f, one))
            }
            "LET" => self.let_(args),
            "LAMBDA" => {
                if args.is_empty() {
                    return err(CellError::Value);
                }
                let mut params = Vec::new();
                for p in &args[..args.len() - 1] {
                    match p {
                        Expr::Name(n) => params.push(n.trim_start_matches('[').trim_end_matches(']').to_string()),
                        _ => return err(CellError::Value),
                    }
                }
                let Some(body) = args.last() else { return err(CellError::Value) };
                Ev::L(Arc::new(Lambda { params, body: body.clone(), env: self.env.clone() }))
            }
            "ISOMITTED" => {
                let r = matches!(args.first(), Some(Expr::Name(n)) if self.env.iter().rev().find(|(k, _)| gridcraft_formula::same_name(k, n)).is_some_and(|(_, v)| matches!(v, Ev::V(Value::Empty))));
                Ev::V(Value::Bool(r))
            }
            "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" => self.higher_order(name, args),
            "GROUPBY" | "PIVOTBY" => {
                // The aggregation may be a function given by name (`SUM`): pass it as its name.
                let i = if name == "GROUPBY" { 2 } else { 3 };
                match args.get(i).and_then(|a| self.function_by_name(a)) {
                    Some(f) => {
                        let mut args = args.to_vec();
                        if let Some(a) = args.get_mut(i) {
                            *a = Expr::Text(f.into());
                        }
                        self.builtin(name, &args)
                    }
                    None => self.builtin(name, args),
                }
            }
            "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" => {
                self.host.note_volatile();
                self.builtin(name, args)
            }
            _ => self.builtin(name, args),
        }
    }

    fn builtin(&mut self, name: &str, args: &[Expr]) -> Ev {
        let Some(spec) = gridcraft_functions::lookup(name) else {
            // A defined name holding a LAMBDA, or a LET-bound lambda.
            if let Ev::L(l) = self.name(name) {
                return self.apply_lambda(&l, args);
            }
            return err(CellError::Name);
        };
        if args.len() < spec.min_args || args.len() > spec.max_args {
            return err(CellError::Value);
        }
        if let Some(kind) = Agg::of(spec.name) {
            return self.aggregate(kind, args);
        }
        let mut evaluated = Vec::with_capacity(args.len());
        // SUMIF/AVERAGEIF: the size of the criteria range (argument 1), which the sum range takes.
        let mut criteria_shape = None;
        // Database functions: the criteria range (argument 2), whose formulas are computed criteria.
        let mut criteria = None;
        // GROUPBY/PIVOTBY: the LAMBDA given as their function.
        let mut lambda = None;
        let lambda_at = match name {
            "GROUPBY" => Some(2),
            "PIVOTBY" => Some(3),
            _ => None,
        };
        for (i, a) in args.iter().enumerate() {
            let mut ev = self.eval(a);
            if lambda_at == Some(i)
                && let Ev::L(l) = &ev
            {
                lambda = Some(l.clone());
                ev = Ev::V(Value::Empty);
            }
            if spec.category == gridcraft_functions::Category::Database
                && i == 2
                && let Ev::R(areas) = &ev
                && let [area] = areas.as_slice()
            {
                criteria = Some(*area);
            }
            if resizes_sum_range(name, args.len()) {
                if i == 0 {
                    criteria_shape = ev_shape(&ev);
                } else if i == 2
                    && let (Some((h, w)), Ev::R(areas)) = (criteria_shape, &ev)
                    && let [area] = areas.as_slice()
                {
                    ev = Ev::R(vec![Area { sheet: area.sheet, range: resized_sum_range(area.range, h, w) }]);
                }
            }
            let from_ref = matches!(ev, Ev::R(_));
            let value = match ev {
                Ev::R(areas) if areas.len() > 1 => {
                    // Multi-area references (unions): flatten into one row for aggregates.
                    let mut vals = Vec::new();
                    for ar in &areas {
                        match self.area_value(ar) {
                            Value::Array(a) => vals.extend(a.data.iter().cloned()),
                            v => vals.push(v),
                        }
                    }
                    Value::from(Array::row(vals))
                }
                ev => self.deref(ev),
            };
            evaluated.push(Arg { value, from_ref });
        }
        let mut ctx = FnCtx { ev: self, criteria, lambda };
        gridcraft_functions::call(spec, &evaluated, &mut ctx).pipe(Ev::V)
    }

    /// Performs aggregation operations without copying its ranges: blocks are folded over the stored
    /// cells, so no array is built and there is no `MAX_CELLS` limit. Arguments are evaluated and read one
    /// after another, as the array path does, so volatile formulas (like `RAND`) see the same order.
    /// Only `SUM`, `COUNT` and `AVG` are supported currently.
    fn aggregate(&mut self, kind: Agg, args: &[Expr]) -> Ev {
        let mut acc = Acc::default();
        let mut first_error = None;
        for a in args {
            let ev = self.eval(a);
            let steps = match ev {
                Ev::R(areas) => areas.iter().map(|area| self.aggregate_area(kind, area, &mut acc)).collect(),
                ev => {
                    let value = self.deref(ev);
                    vec![self.aggregate_value(kind, value, false, &mut acc)]
                }
            };
            if first_error.is_none() {
                first_error = steps.into_iter().find_map(Result::err);
            }
        }
        first_error.unwrap_or_else(|| Ev::V(acc.finish(kind)))
    }

    /// Adds one area to `acc`; `Err` is the result the function ends with.
    fn aggregate_area(&mut self, kind: Agg, a: &Area, acc: &mut Acc) -> Result<(), Ev> {
        if !a.range.is_single() {
            let mut error = None;
            let walked = self.host.visit_range(a.sheet, a.range, &mut |v| match acc.add_cell(kind, v) {
                Ok(()) => true,
                Err(e) => {
                    error = Some(e);
                    false
                }
            });
            if walked {
                return error.map_or(Ok(()), |e| Err(err(e)));
            }
        }
        let value = self.area_value(a);
        self.aggregate_value(kind, value, true, acc)
    }

    /// Adds one argument's value by calling `SUM` and `COUNT` on it, so coercion and error rules
    /// stay in one place (`AVERAGE` of an argument is its `SUM` over its `COUNT`).
    fn aggregate_value(&mut self, kind: Agg, value: Value, from_ref: bool, acc: &mut Acc) -> Result<(), Ev> {
        let (Some(sum), Some(count)) = (gridcraft_functions::lookup("SUM"), gridcraft_functions::lookup("COUNT")) else {
            return Err(err(CellError::Name));
        };
        let args = [Arg { value, from_ref }];
        if kind != Agg::Count {
            match gridcraft_functions::call(sum, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.sum += n,
                other => return Err(Ev::V(other)),
            }
        }
        if kind != Agg::Sum {
            match gridcraft_functions::call(count, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.count = acc.count.saturating_add(n as u64),
                other => return Err(Ev::V(other)),
            }
        }
        Ok(())
    }

    pub fn apply_lambda(&mut self, l: &Lambda, args: &[Expr]) -> Ev {
        // Too many or too few argument positions is #VALUE!. An explicitly empty position, as in
        // `(1,)`, is `Expr::Missing` and still counts as a position (it binds as omitted). `f()`
        // parses to no arguments, which for a one-parameter lambda can also be read as one empty
        // position, so that call still binds the parameter as omitted.
        let positions_ok = args.len() == l.params.len() || (args.is_empty() && l.params.len() == 1);
        if !positions_ok {
            return err(CellError::Value);
        }
        let mut bound = Vec::with_capacity(l.params.len());
        for (i, p) in l.params.iter().enumerate() {
            let v = match args.get(i) {
                Some(Expr::Missing) | None => Ev::V(Value::Empty),
                Some(e) => self.eval(e),
            };
            bound.push((p.clone(), v));
        }
        self.call_lambda_values(l, bound)
    }

    fn call_lambda_values(&mut self, l: &Lambda, bound: Vec<(String, Ev)>) -> Ev {
        let saved = std::mem::replace(&mut self.env, l.env.clone());
        self.env.extend(bound);
        let r = self.eval(&l.body);
        self.env = saved;
        r
    }

    fn let_(&mut self, args: &[Expr]) -> Ev {
        if args.len() < 3 || args.len().is_multiple_of(2) {
            return err(CellError::Value);
        }
        let base = self.env.len();
        for pair in args[..args.len() - 1].chunks(2) {
            let [Expr::Name(n), v] = pair else {
                self.env.truncate(base);
                return err(CellError::Value);
            };
            let val = self.eval(v);
            self.env.push((n.clone(), val));
        }
        let r = match args.last() {
            Some(body) => self.eval(body),
            None => err(CellError::Value),
        };
        self.env.truncate(base);
        r
    }

    /// A worksheet function given by name where a function is expected (`BYROW(A1:C3,SUM)`,
    /// `GROUPBY(A2:A9,B2:B9,SUM)`): its name, unless a LET/LAMBDA variable or a defined name has
    /// that name.
    fn function_by_name(&self, e: &Expr) -> Option<String> {
        let Expr::Name(n) = e else { return None };
        if self.env.iter().any(|(k, _)| gridcraft_formula::same_name(k, n)) || self.host.workbook().name(n, self.sheet).is_some() {
            return None;
        }
        let u = n.to_ascii_uppercase();
        gridcraft_functions::lookup(&u).map(|_| u)
    }

    /// The function argument of a LAMBDA helper; a function given by name takes `arity`
    /// arguments.
    fn lambda_arg(&mut self, e: Option<&Expr>, arity: usize) -> Option<Arc<Lambda>> {
        if let Some(f) = e.and_then(|x| self.function_by_name(x)) {
            let params: Vec<String> = (0..arity).map(|i| format!("_xleta.{i}")).collect();
            let body = Expr::Call(f, params.iter().map(|p| Expr::Name(p.clone())).collect());
            return Some(Arc::new(Lambda { params, body, env: Vec::new() }));
        }
        match e.map(|x| self.eval(x)) {
            Some(Ev::L(l)) => Some(l),
            _ => None,
        }
    }

    fn higher_order(&mut self, name: &str, args: &[Expr]) -> Ev {
        let as_array = |v: Value| -> Arc<Array> {
            match v {
                Value::Array(a) => a,
                other => Arc::new(Array::scalar(other)),
            }
        };
        match name {
            "MAP" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.last(), args.len() - 1) else { return err(CellError::Value) };
                let arrays: Vec<Arc<Array>> = args[..args.len() - 1].iter().map(|a| as_array(self.value(a))).collect();
                let rows = arrays.iter().map(|a| a.rows).max().unwrap_or(1);
                let cols = arrays.iter().map(|a| a.cols).max().unwrap_or(1);
                let Some(n) = broadcast_len(rows, cols) else { return err(CellError::Num) };
                // The LAMBDA takes one parameter per array.
                if l.params.len() != arrays.len() {
                    return err(CellError::Value);
                }
                let mut out = Vec::with_capacity(n);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = l.params.iter().zip(arrays.iter()).map(|(p, a)| (p.clone(), Ev::V(a.get_broadcast(r, c)))).collect();
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            "REDUCE" | "SCAN" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let mut acc = self.value(&args[0]);
                let arr = as_array(self.value(&args[1]));
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let mut scan = Vec::with_capacity(arr.data.len());
                for v in arr.data.iter() {
                    let bound = vec![(l.params[0].clone(), Ev::V(acc.clone())), (l.params[1].clone(), Ev::V(v.clone()))];
                    let r = self.call_lambda_values(&l, bound);
                    acc = self.deref(r);
                    if name == "SCAN" {
                        let Some(one) = single(acc.clone()) else { return err(CellError::Calc) };
                        scan.push(one);
                    }
                }
                if name == "SCAN" {
                    Ev::V(Array::new(arr.rows, arr.cols, scan).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
                } else {
                    Ev::V(acc)
                }
            }
            "BYROW" | "BYCOL" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let arr = as_array(self.value(&args[0]));
                let Some(l) = self.lambda_arg(args.get(1), 1) else { return err(CellError::Value) };
                let Some(p) = l.params.first().cloned() else { return err(CellError::Value) };
                if l.params.len() != 1 {
                    return err(CellError::Value);
                }
                let by_row = name == "BYROW";
                let n = if by_row { arr.rows } else { arr.cols };
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let slice: Vec<Value> = if by_row {
                        (0..arr.cols).map(|c| arr.get(i, c).cloned().unwrap_or_default()).collect()
                    } else {
                        (0..arr.rows).map(|r| arr.get(r, i).cloned().unwrap_or_default()).collect()
                    };
                    let a = if by_row { Array::row(slice) } else { Array::column(slice) };
                    let r = self.call_lambda_values(&l, vec![(p.clone(), Ev::V(a.into()))]);
                    let Some(one) = single(self.deref(r)) else { return err(CellError::Calc) };
                    out.push(one);
                }
                let a = if by_row { Array::column(out) } else { Array::row(out) };
                Ev::V(a.into())
            }
            "MAKEARRAY" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let (rows, cols) = match (self.arg_num(args, 0, 0.0), self.arg_num(args, 1, 0.0)) {
                    (Ok(r), Ok(c)) => (r.trunc(), c.trunc()),
                    (Err(e), _) | (_, Err(e)) => return err(e),
                };
                if rows < 1.0 || cols < 1.0 || rows * cols > MAX_CELLS as f64 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let (rows, cols) = (rows as usize, cols as usize);
                let mut out = Vec::with_capacity(rows * cols);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = vec![
                            (l.params[0].clone(), Ev::V(Value::number(r as f64 + 1.0))),
                            (l.params[1].clone(), Ev::V(Value::number(c as f64 + 1.0))),
                        ];
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            _ => err(CellError::Name),
        }
    }

    /// Whole rows/columns trimmed for ROW()/COLUMN() arrays.
    fn trim_lines(&mut self, a: &Area) -> RangeRef {
        if a.range.is_full_cols() || a.range.is_full_rows() { self.trim(a) } else { a.range }
    }

    fn offset(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.len() < 3 || args.len() > 5 {
            return err(CellError::Value);
        }
        let base = match self.eval(&args[0]) {
            Ev::R(a) if a.len() == 1 => a[0],
            Ev::V(Value::Error(e)) => return err(e),
            _ => return err(CellError::Value),
        };
        let rows = match self.arg_num(args, 1, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let cols = match self.arg_num(args, 2, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let h = match self.arg_num(args, 3, base.range.height() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let w = match self.arg_num(args, 4, base.range.width() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        if h == 0 || w == 0 {
            return err(CellError::Ref);
        }
        let r0 = (base.range.start.row as i64).saturating_add(rows);
        let c0 = (base.range.start.col as i64).saturating_add(cols);
        let (r0, r1) = if h > 0 { (r0, r0.saturating_add(h - 1)) } else { (r0.saturating_add(h + 1), r0) };
        let (c0, c1) = if w > 0 { (c0, c0.saturating_add(w - 1)) } else { (c0.saturating_add(w + 1), c0) };
        if r0 < 0 || c0 < 0 || r1 >= MAX_ROWS as i64 || c1 >= MAX_COLS as i64 {
            return err(CellError::Ref);
        }
        Ev::R(vec![Area { sheet: base.sheet, range: RangeRef::new(CellRef::new(r0 as u32, c0 as u32), CellRef::new(r1 as u32, c1 as u32)) }])
    }

    fn indirect(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.is_empty() || args.len() > 2 {
            return err(CellError::Value);
        }
        let text = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t,
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let a1 = match args.get(1) {
            None | Some(Expr::Missing) => true,
            Some(e) => {
                let v = self.value(e);
                v.scalar().to_bool().unwrap_or(true)
            }
        };
        // With a1 = FALSE, text that isn't R1C1 can still be a name, but never an A1 reference.
        let (text, a1_ok) = if a1 {
            (text, true)
        } else {
            match r1c1_to_a1(&text, self.at) {
                Ok(Some(a1_text)) => (a1_text, true),
                Ok(None) => (text, false),
                Err(e) => return err(e),
            }
        };
        match gridcraft_formula::parse(&text) {
            Ok(Expr::Ref(_)) if !a1_ok => err(CellError::Ref),
            Ok(e @ (Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_))) => match self.eval(&e) {
                r @ Ev::R(_) => r,
                _ => err(CellError::Ref),
            },
            _ => err(CellError::Ref),
        }
    }

    fn index(&mut self, args: &[Expr]) -> Ev {
        if args.is_empty() || args.len() > 4 {
            return err(CellError::Value);
        }
        let src = self.eval(&args[0]);
        let mut arg = |i: usize| match args.get(i) {
            None | Some(Expr::Missing) => None,
            Some(e) => Some(self.value(e)),
        };
        let (rows, cols) = (arg(1), arg(2));
        let area_n = match self.arg_num(args, 3, 1.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let two = args.len() == 2;
        let num = |v: &Option<Value>| v.as_ref().map_or(Ok(0.0), |v| v.to_number()).map(|n| n.trunc() as i64);
        // Arrays of positions pick a value for each: INDEX(A1:C10,{1;2},{1,3}) is 2×2.
        if let (Some(Value::Array(_)), _) | (_, Some(Value::Array(_))) = (&rows, &cols) {
            let as_array = |v: &Option<Value>| match v {
                Some(Value::Array(a)) => Arc::clone(a),
                Some(v) => Arc::new(Array::scalar(v.clone())),
                None => Arc::new(Array::scalar(Value::Number(0.0))),
            };
            let (ra, ca) = (as_array(&rows), as_array(&cols));
            let (h, w) = (ra.rows.max(ca.rows), ra.cols.max(ca.cols));
            if (h as u64).saturating_mul(w as u64) > MAX_CELLS {
                return err(CellError::Num);
            }
            let mut out = Vec::with_capacity(h * w);
            for r in 0..h {
                for c in 0..w {
                    let pos = (num(&Some(ra.get_broadcast(r, c))), num(&Some(ca.get_broadcast(r, c))));
                    let v = match pos {
                        (Ok(row), Ok(col)) => {
                            let one = self.index_one(src.clone(), row, col, area_n, two);
                            match self.deref(one) {
                                Value::Array(a) if a.rows * a.cols > 1 => Value::Error(CellError::Calc),
                                v => v.scalar(),
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => Value::Error(e),
                    };
                    out.push(v);
                }
            }
            return Ev::V(Array::new(h, w, out).map(Value::from).unwrap_or(Value::Error(CellError::Value)));
        }
        let (row, col) = match (num(&rows), num(&cols)) {
            (Ok(r), Ok(c)) => (r, c),
            (Err(e), _) | (_, Err(e)) => return err(e),
        };
        self.index_one(src, row, col, area_n, two)
    }

    /// INDEX for one row and column position (0 = the whole column or row).
    fn index_one(&mut self, src: Ev, row: i64, col: i64, area_n: i64, two_args: bool) -> Ev {
        if row < 0 || col < 0 {
            return err(CellError::Value);
        }
        match src {
            Ev::R(areas) => {
                let area = usize::try_from((area_n as u64).saturating_sub(1)).ok();
                let Some(a) = area.and_then(|i| areas.get(i)).copied() else { return err(CellError::Ref) };
                let r = a.range;
                // A single row or column with one index picks along it.
                let (row, col) = if two_args && r.height() == 1 { (1, row) } else { (row, col) };
                if row as u64 > r.height() as u64 || col as u64 > r.width() as u64 {
                    return err(CellError::Ref);
                }
                let (r0, r1) = if row == 0 { (r.start.row, r.end.row) } else { (r.start.row + row as u32 - 1, r.start.row + row as u32 - 1) };
                let (c0, c1) = if col == 0 { (r.start.col, r.end.col) } else { (r.start.col + col as u32 - 1, r.start.col + col as u32 - 1) };
                Ev::R(vec![Area { sheet: a.sheet, range: RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)) }])
            }
            Ev::V(Value::Array(arr)) => {
                let (row, col) = if two_args && arr.rows == 1 { (1, row) } else { (row, col) };
                if row as usize > arr.rows || col as usize > arr.cols {
                    return err(CellError::Ref);
                }
                match (row, col) {
                    (0, 0) => Ev::V(Value::Array(arr)),
                    (0, c) => Ev::V(Array::column((0..arr.rows).map(|r| arr.get(r, c as usize - 1).cloned().unwrap_or_default()).collect()).into()),
                    (r, 0) => Ev::V(Array::row((0..arr.cols).map(|c| arr.get(r as usize - 1, c).cloned().unwrap_or_default()).collect()).into()),
                    (r, c) => Ev::V(arr.get(r as usize - 1, c as usize - 1).cloned().unwrap_or(Value::Error(CellError::Ref))),
                }
            }
            Ev::V(Value::Error(e)) => err(e),
            Ev::V(v) => {
                if row <= 1 && col <= 1 {
                    Ev::V(v)
                } else {
                    err(CellError::Ref)
                }
            }
            Ev::L(_) => err(CellError::Value),
        }
    }

    /// XLOOKUP whose result range is a reference returns a reference, as in Excel, so that it can
    /// end a range (`SUM(XLOOKUP(a,…):XLOOKUP(b,…))`) or feed CELL. Everything else (arrays of
    /// lookup values, array arguments, errors) goes to the library function.
    fn xlookup(&mut self, args: &[Expr]) -> Ev {
        if (3..=6).contains(&args.len())
            && let (Ev::R(look), Ev::R(ret)) = (self.eval(&args[1]), self.eval(&args[2]))
            && let ([l], [r]) = (look.as_slice(), ret.as_slice())
            && let Some(spec) = gridcraft_functions::lookup("XMATCH")
        {
            let key = self.value(&args[0]);
            let (l, r) = (*l, *r);
            let vertical = l.range.width() == 1 && l.range.height() > 1;
            let horizontal = l.range.height() == 1 && l.range.width() > 1;
            let same_len = if vertical { r.range.height() == l.range.height() } else { r.range.width() == l.range.width() };
            if !matches!(key, Value::Array(_)) && (vertical || horizontal) && same_len {
                let mut xargs = vec![Arg { value: key, from_ref: false }, Arg { value: self.area_value(&l), from_ref: true }];
                // An empty match_mode before a given search_mode is the default (0), not the end.
                let given = |i: usize| args.get(i).is_some_and(|e| !matches!(e, Expr::Missing));
                let last = if given(5) {
                    5
                } else if given(4) {
                    4
                } else {
                    3
                };
                for i in 4..=last {
                    match args.get(i) {
                        Some(Expr::Missing) | None => xargs.push(Arg { value: Value::Number(0.0), from_ref: false }),
                        Some(e) => {
                            let value = self.value(e);
                            xargs.push(Arg { value, from_ref: false });
                        }
                    }
                }
                let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                return match gridcraft_functions::call(spec, &xargs, &mut ctx) {
                    Value::Number(p) if p >= 1.0 => {
                        let k = p as u32 - 1;
                        let range = if vertical {
                            RangeRef::new(
                                CellRef::new(r.range.start.row + k, r.range.start.col),
                                CellRef::new(r.range.start.row + k, r.range.end.col),
                            )
                        } else {
                            RangeRef::new(
                                CellRef::new(r.range.start.row, r.range.start.col + k),
                                CellRef::new(r.range.end.row, r.range.start.col + k),
                            )
                        };
                        Ev::R(vec![Area { sheet: r.sheet, range }])
                    }
                    Value::Error(CellError::NA) if args.get(3).is_some_and(|e| !matches!(e, Expr::Missing)) => self.eval(&args[3]),
                    Value::Error(e) => err(e),
                    _ => err(CellError::Value),
                };
            }
        }
        self.builtin("XLOOKUP", args)
    }

    fn cell_info(&mut self, args: &[Expr]) -> Ev {
        // Lower-cased by Unicode, not ASCII: the German `PRÄFIX` has to reach `präfix`.
        let kind = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => cell_info_type(&t.to_lowercase()).to_string(),
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let area = match args.get(1).map(|a| self.eval(a)) {
            Some(Ev::R(a)) => a.first().copied(),
            None => Some(Area { sheet: self.sheet, range: RangeRef::cell(self.at) }),
            _ => return err(CellError::Value),
        };
        let Some(a) = area else { return err(CellError::Ref) };
        let c = a.range.start;
        let wb = self.host.workbook();
        let sheet = wb.sheet(a.sheet);
        match kind.as_str() {
            "address" => Ev::V(Value::text(format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1))),
            "row" => Ev::V(Value::number(c.row as f64 + 1.0)),
            "col" => Ev::V(Value::number(c.col as f64 + 1.0)),
            "contents" => Ev::V(self.host.cell_value(a.sheet, c)),
            "type" => {
                let v = self.host.cell_value(a.sheet, c);
                Ev::V(Value::text(match v {
                    Value::Empty => "b",
                    Value::Text(_) => "l",
                    _ => "v",
                }))
            }
            "width" => Ev::V(Value::number(sheet.map_or(8.43, |s| gridcraft_model::points_to_chars(s.col_width(c.col) as f64)).round())),
            // Excel's `<folder>/[<file>]<sheet>`, which the classic sheet-name formula splits on
            // the `]`. Empty until the workbook has a file of its own, as in Excel (#1489).
            "filename" => Ev::V(Value::text(match &wb.source_path {
                Some(file) => {
                    let name = file.file_name().unwrap_or_default().to_string_lossy();
                    let on = sheet.map_or("", |s| s.name.as_str());
                    file.parent().unwrap_or(std::path::Path::new("")).join(format!("[{name}]{on}")).to_string_lossy().into_owned()
                }
                None => String::new(),
            })),
            "sheetname" => Ev::V(Value::text(sheet.map(|s| s.name.clone()).unwrap_or_default())),
            "protect" => Ev::V(Value::number(if sheet.is_some_and(|s| wb.styles.get(s.style_id(c)).protection.locked) { 1.0 } else { 0.0 })),
            "format" => {
                let code = sheet.map(|s| wb.styles.get(s.style_id(c)).num_fmt.as_str().to_string()).unwrap_or_default();
                Ev::V(Value::text(cell_format_code(&code)))
            }
            "color" | "parentheses" => Ev::V(Value::Number(0.0)),
            "prefix" => Ev::V(Value::text("")),
            _ => err(CellError::Value),
        }
    }

    fn subtotal(&mut self, name: &str, args: &[Expr]) -> Ev {
        // SUBTOTAL needs function_num and ref1; AGGREGATE needs function_num, options and ref1.
        let min_args = if name == "AGGREGATE" { 3 } else { 2 };
        if args.len() < min_args {
            return err(CellError::Value);
        }
        let code = match self.arg_num(args, 0, 0.0) {
            Ok(n) => n.trunc() as u32,
            Err(e) => return err(e),
        };
        let (func, options, rest) = if name == "AGGREGATE" {
            let opt = match self.arg_num(args, 1, 0.0) {
                Ok(n) => n.trunc() as u32,
                Err(e) => return err(e),
            };
            (code, opt, &args[2..])
        } else {
            let (f, ignore_hidden) = if code > 100 { (code - 100, true) } else { (code, false) };
            (f, if ignore_hidden { 5 } else { 0 }, &args[1..])
        };
        if !(1..=19).contains(&func) || (name == "SUBTOTAL" && func > 11) {
            return err(CellError::Value);
        }
        let skip_hidden = matches!(options, 1 | 3 | 5 | 7);
        // SUBTOTAL leaves out rows a filter hides whatever the function number (Microsoft's
        // SUBTOTAL documentation); 101-111 also leave out rows hidden by hand.
        let skip_filtered = name == "SUBTOTAL";
        let skip_errors = matches!(options, 2 | 3 | 6 | 7);
        // SUBTOTAL always leaves out nested SUBTOTAL/AGGREGATE cells (101-111 too); AGGREGATE does so for options 0-3.
        let skip_nested = name == "SUBTOTAL" || options <= 3;
        let mut values = Vec::new();
        let mut k = None;
        let refs = if name == "AGGREGATE" && func >= 14 { &rest[..1.min(rest.len())] } else { rest };
        if name == "AGGREGATE" && func >= 14 {
            k = rest.get(1).map(|e| self.value(e));
        }
        // Every cell visited (even one skipped as hidden, nested or an error) and every value
        // counts against one budget for the whole call. All arguments are evaluated (references
        // stay lazy) and charged before any cell is read: a value stored in XFD1048576 makes
        // `A:XFD` trim to the whole sheet, ~17 billion cells (#2175).
        enum Input {
            Cells(usize, RangeRef),
            Values(Value),
        }
        let mut inputs = Vec::new();
        let mut budget = MAX_CELLS;
        for e in refs {
            match self.eval(e) {
                Ev::R(areas) => {
                    for a in areas {
                        let range = self.trim(&a);
                        budget = match budget.checked_sub(range.count()) {
                            Some(left) => left,
                            None => return err(CellError::Num),
                        };
                        inputs.push(Input::Cells(a.sheet, range));
                    }
                }
                Ev::V(v) => {
                    let n = match &v {
                        Value::Array(a) => u64::try_from(a.data.len()).unwrap_or(u64::MAX),
                        _ => 1,
                    };
                    budget = match budget.checked_sub(n) {
                        Some(left) => left,
                        None => return err(CellError::Num),
                    };
                    inputs.push(Input::Values(v));
                }
                Ev::L(_) => return err(CellError::Value),
            }
        }
        for input in inputs {
            match input {
                Input::Cells(sheet, range) => {
                    for c in range.iter() {
                        let (hidden, nested) = {
                            let wb = self.host.workbook();
                            let s = wb.sheet(sheet);
                            let hidden =
                                s.is_some_and(|s| if skip_hidden { s.is_row_hidden(c.row) } else { skip_filtered && s.is_row_filtered(c.row) });
                            let nested = skip_nested
                                && s.and_then(|s| s.cell(c)).and_then(|cell| cell.formula.as_ref()).is_some_and(|f| {
                                    let u = f.text.to_ascii_uppercase();
                                    u.contains("SUBTOTAL(") || u.contains("AGGREGATE(")
                                });
                            (hidden, nested)
                        };
                        if hidden || nested {
                            continue;
                        }
                        let v = self.host.cell_value(sheet, c);
                        if skip_errors && v.is_error() {
                            continue;
                        }
                        values.push(v);
                    }
                }
                Input::Values(Value::Array(a)) => values.extend(a.data.iter().filter(|v| !(skip_errors && v.is_error())).cloned()),
                Input::Values(v) => values.push(v),
            }
        }
        if let Some(k) = k {
            return Ev::V(gridcraft_functions::aggregate_values_k(func, &values, &k));
        }
        Ev::V(gridcraft_functions::aggregate_values(func, &values))
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

/// CELL("format") code for a number format.
/// The English name of an info type. Excel translates `CELL`'s function name but not its first
/// argument, which is a string: a German Excel saves `ZELLE("Dateiname";A1)` as
/// `CELL("Dateiname",A1)`, so the localized names have to be understood as they stand (#1489).
/// `format` is spelled the same in German and needs no entry.
fn cell_info_type(kind: &str) -> &str {
    match kind {
        "adresse" => "address",
        "zeile" => "row",
        "spalte" => "col",
        "inhalt" => "contents",
        "typ" => "type",
        "breite" => "width",
        "dateiname" => "filename",
        "schutz" => "protect",
        "farbe" => "color",
        "klammern" => "parentheses",
        "präfix" => "prefix",
        other => other,
    }
}

fn cell_format_code(code: &str) -> String {
    let c = code.to_ascii_lowercase();
    if c == "general" {
        return "G".into();
    }
    if c.contains('%') {
        return format!("P{}", decimals(&c));
    }
    if c.contains('e') && c.contains('0') {
        return format!("S{}", decimals(&c));
    }
    if c.contains('$') {
        return format!("C{}", decimals(&c));
    }
    if c.contains('y') || c.contains('d') {
        return "D1".into();
    }
    if c.contains('h') {
        return "D9".into();
    }
    if c.contains(',') {
        return format!(",{}", decimals(&c));
    }
    format!("F{}", decimals(&c))
}

fn decimals(c: &str) -> usize {
    c.split_once('.').map_or(0, |(_, d)| d.chars().take_while(|ch| *ch == '0').count())
}

fn same_kind(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b) || (a.is_empty() || b.is_empty())
}

/// Converts an R1C1 reference text to A1 (for INDIRECT(…, FALSE)). `Ok(None)` means the text is
/// not R1C1; `Err` means it is R1C1 but points past the sheet.
pub fn r1c1_to_a1(s: &str, at: CellRef) -> Result<Option<String>, CellError> {
    let (sheet, body) = match s.rsplit_once('!') {
        Some((sh, b)) => (format!("{sh}!"), b),
        None => (String::new(), s),
    };
    let conv = |part: &str| -> Result<Option<String>, CellError> {
        let u = part.to_ascii_uppercase();
        let Some((rpart, cpart)) = u.strip_prefix('R').and_then(|rest| rest.split_once('C')) else { return Ok(None) };
        // `None`: not R1C1. `Some(None)`: R1C1, but past the sheet.
        let num = |p: &str, base: u32, max: u32| -> Option<Option<(u32, bool)>> {
            let (v, absolute) = if p.is_empty() {
                (Some(i64::from(base)), false)
            } else if let Some(inner) = p.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                (i64::from(base).checked_add(inner.parse().ok()?), false)
            } else {
                (p.parse::<u32>().ok()?.checked_sub(1).map(i64::from), true)
            };
            Some(v.filter(|v| (0..i64::from(max)).contains(v)).map(|v| (v as u32, absolute)))
        };
        let (Some(r), Some(c)) = (num(rpart, at.row, MAX_ROWS), num(cpart, at.col, MAX_COLS)) else { return Ok(None) };
        let (Some((r, ra)), Some((c, ca))) = (r, c) else { return Err(CellError::Ref) };
        Ok(Some(format!("{}{}{}{}", if ca { "$" } else { "" }, gridcraft_core::col_to_letters(c), if ra { "$" } else { "" }, r + 1)))
    };
    let out = match body.split_once(':') {
        Some((a, b)) => match (conv(a)?, conv(b)?) {
            (Some(a), Some(b)) => format!("{a}:{b}"),
            _ => return Ok(None),
        },
        None => match conv(body)? {
            Some(x) => x,
            None => return Ok(None),
        },
    };
    Ok(Some(format!("{sheet}{out}")))
}

/// The aggregates that can fold a reference without building an array.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Agg {
    Sum,
    Count,
    Average,
}

impl Agg {
    fn of(name: &str) -> Option<Agg> {
        match name {
            "SUM" => Some(Agg::Sum),
            "COUNT" => Some(Agg::Count),
            "AVERAGE" => Some(Agg::Average),
            _ => None,
        }
    }
}

/// What a folded aggregate carries from one value to the next.
#[derive(Default)]
struct Acc {
    sum: f64,
    count: u64,
}

impl Acc {
    /// Takes a value found inside a reference: only numbers count. `Err` is an error value that
    /// ends `SUM` and `AVERAGE`; `COUNT` skips errors.
    fn add_cell(&mut self, kind: Agg, v: &Value) -> Result<(), CellError> {
        match v {
            Value::Number(n) => {
                self.sum += n;
                self.count = self.count.saturating_add(1);
                Ok(())
            }
            Value::Error(e) if kind != Agg::Count => Err(*e),
            _ => Ok(()),
        }
    }

    fn finish(&self, kind: Agg) -> Value {
        match kind {
            Agg::Sum => Value::number(self.sum),
            Agg::Count => Value::number(self.count as f64),
            Agg::Average if self.count == 0 => Value::Error(CellError::Div0),
            Agg::Average => Value::number(self.sum / self.count as f64),
        }
    }
}

// ---------------------------------------------------------------- operators

fn map_unary(v: &Value, f: &dyn Fn(f64) -> Value, plus: bool, sys: gridcraft_core::DateSystem) -> Value {
    match v {
        Value::Array(a) => Value::from(Array { rows: a.rows, cols: a.cols, data: a.data.iter().map(|x| map_unary(x, f, plus, sys)).collect() }),
        Value::Error(e) => Value::Error(*e),
        Value::Text(_) if plus => v.clone(),
        other => match other.to_number_in(sys) {
            Ok(n) => f(n),
            Err(e) => Value::Error(e),
        },
    }
}

/// Element-wise combination with Excel's broadcasting.
pub fn zip2(a: &Value, b: &Value, f: impl Fn(&Value, &Value) -> Value) -> Value {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            let rows = broadcast_dim(x.rows, y.rows);
            let cols = broadcast_dim(x.cols, y.cols);
            let Some(n) = broadcast_len(rows, cols) else { return Value::Error(CellError::Num) };
            let mut data = Vec::with_capacity(n);
            for r in 0..rows {
                for c in 0..cols {
                    data.push(f(&x.get_broadcast(r, c), &y.get_broadcast(r, c)));
                }
            }
            Value::from(Array { rows, cols, data })
        }
        (Value::Array(x), s) => Value::from(Array { rows: x.rows, cols: x.cols, data: x.data.iter().map(|v| f(v, s)).collect() }),
        (s, Value::Array(y)) => Value::from(Array { rows: y.rows, cols: y.cols, data: y.data.iter().map(|v| f(s, v)).collect() }),
        (x, y) => f(x, y),
    }
}

/// Cell count of a `rows`×`cols` broadcast result, or `None` when it would exceed [`MAX_CELLS`].
/// Each operand passes its own size cap, but their broadcast product can be far larger
/// (`SEQUENCE(1048576)*SEQUENCE(1,16384)` is 17 billion cells), so it is checked before allocating.
fn broadcast_len(rows: usize, cols: usize) -> Option<usize> {
    rows.checked_mul(cols).filter(|&n| n as u64 <= MAX_CELLS)
}

fn broadcast_dim(a: usize, b: usize) -> usize {
    if a == 1 {
        b
    } else if b == 1 {
        a
    } else {
        a.max(b)
    }
}

fn zip3(c: &Array, t: &Value, f: &Value, g: impl Fn(&Value, &Value, &Value) -> Value) -> Value {
    let dims = |v: &Value| if let Value::Array(a) = v { (a.rows, a.cols) } else { (1, 1) };
    let (tr, tc) = dims(t);
    let (fr, fc) = dims(f);
    let rows = broadcast_dim(broadcast_dim(c.rows, tr), fr);
    let cols = broadcast_dim(broadcast_dim(c.cols, tc), fc);
    let Some(n) = broadcast_len(rows, cols) else { return Value::Error(CellError::Num) };
    let get = |v: &Value, r: usize, col: usize| if let Value::Array(a) = v { a.get_broadcast(r, col) } else { v.clone() };
    let mut data = Vec::with_capacity(n);
    for r in 0..rows {
        for col in 0..cols {
            data.push(g(&c.get_broadcast(r, col), &get(t, r, col), &get(f, r, col)));
        }
    }
    Value::from(Array { rows, cols, data })
}

/// One cell of a LAMBDA helper's result, or `None` for an array of more than one value: arrays
/// can't nest, so the whole helper gives `#CALC!`, as in Excel.
fn single(v: Value) -> Option<Value> {
    match v {
        Value::Array(a) if a.rows * a.cols > 1 => None,
        v => Some(v.scalar()),
    }
}

pub fn binary_values(op: BinOp, x: &Value, y: &Value, sys: gridcraft_core::DateSystem) -> Value {
    zip2(x, y, |a, b| binary_scalar(op, a, b, sys))
}

fn binary_scalar(op: BinOp, a: &Value, b: &Value, sys: gridcraft_core::DateSystem) -> Value {
    if let Value::Error(e) = a {
        return Value::Error(*e);
    }
    if let Value::Error(e) = b {
        return Value::Error(*e);
    }
    let num = |f: fn(f64, f64) -> Value| match (a.to_number_in(sys), b.to_number_in(sys)) {
        (Ok(x), Ok(y)) => f(x, y),
        (Err(e), _) | (_, Err(e)) => Value::Error(e),
    };
    use std::cmp::Ordering::*;
    match op {
        BinOp::Add => num(|x, y| Value::number(x + y)),
        BinOp::Sub => num(|x, y| Value::number(x - y)),
        BinOp::Mul => num(|x, y| Value::number(x * y)),
        BinOp::Div => num(|x, y| if y == 0.0 { Value::Error(CellError::Div0) } else { Value::number(x / y) }),
        BinOp::Pow => num(|x, y| {
            if x == 0.0 && y == 0.0 {
                Value::Error(CellError::Num)
            } else if x == 0.0 && y < 0.0 {
                Value::Error(CellError::Div0)
            } else {
                Value::number(x.powf(y))
            }
        }),
        BinOp::Concat => match (a.to_text(), b.to_text()) {
            (Ok(x), Ok(y)) => {
                if x.len() + y.len() > 32767 * 4 {
                    Value::Error(CellError::Value)
                } else {
                    Value::text(x + &y)
                }
            }
            (Err(e), _) | (_, Err(e)) => Value::Error(e),
        },
        BinOp::Eq => Value::Bool(compare(a, b) == Equal),
        BinOp::Ne => Value::Bool(compare(a, b) != Equal),
        BinOp::Lt => Value::Bool(compare(a, b) == Less),
        BinOp::Gt => Value::Bool(compare(a, b) == Greater),
        BinOp::Le => Value::Bool(compare(a, b) != Greater),
        BinOp::Ge => Value::Bool(compare(a, b) != Less),
        BinOp::Range | BinOp::Union | BinOp::Intersect => Value::Error(CellError::Value),
    }
}

/// SUMIF and AVERAGEIF with a sum range: Excel sizes the sum range like the criteria range.
fn resizes_sum_range(name: &str, args: usize) -> bool {
    args == 3 && matches!(name, "SUMIF" | "AVERAGEIF")
}

/// Height and width of a single-area reference or an array argument.
fn ev_shape(ev: &Ev) -> Option<(u32, u32)> {
    match ev {
        Ev::R(areas) => match areas.as_slice() {
            [a] => Some((a.range.height(), a.range.width())),
            _ => None,
        },
        Ev::V(Value::Array(a)) => Some((u32::try_from(a.rows).unwrap_or(u32::MAX), u32::try_from(a.cols).unwrap_or(u32::MAX))),
        Ev::V(_) => Some((1, 1)),
        Ev::L(_) => None,
    }
}

/// The range SUMIF/AVERAGEIF add up: from the sum range's top-left cell, as many rows and columns
/// as the criteria range (`height` × `width`), cut off at the sheet's edge.
pub fn resized_sum_range(sum: RangeRef, height: u32, width: u32) -> RangeRef {
    let row = sum.start.row.saturating_add(height.saturating_sub(1)).min(MAX_ROWS - 1);
    let col = sum.start.col.saturating_add(width.saturating_sub(1)).min(MAX_COLS - 1);
    RangeRef::new(sum.start, CellRef::new(row, col))
}

/// Collects the static references of an expression with their sheets resolved; `dynamic` is
/// set when the formula uses INDIRECT/OFFSET/names/tables whose targets can change.
pub fn precedents(wb: &Workbook, sheet: usize, e: &Expr) -> (Vec<Area>, bool) {
    let mut out = Vec::new();
    let mut dynamic = false;
    let push = |out: &mut Vec<Area>, r: &Reference, range: RangeRef| match &r.sheet {
        SheetSel::Current => out.push(Area { sheet, range }),
        SheetSel::Named(n) => {
            if let Some(i) = wb.sheet_index(n) {
                out.push(Area { sheet: i, range });
            }
        }
        SheetSel::Span(a, b) => {
            if let (Some(i), Some(j)) = (wb.sheet_index(a), wb.sheet_index(b)) {
                for s in i.min(j)..=i.max(j) {
                    out.push(Area { sheet: s, range });
                }
            }
        }
    };
    e.walk(&mut |x| match x {
        Expr::Ref(r) => push(&mut out, r, r.range()),
        // The cells SUMIF/AVERAGEIF really read from the sum range (see `resized_sum_range`); when
        // the criteria range's size isn't known statically, recalculate the formula every time.
        Expr::Call(n, args) if resizes_sum_range(n, args.len()) => match (args.first(), args.get(2)) {
            (Some(Expr::Ref(c)), Some(Expr::Ref(s))) => {
                let (crit, sum) = (c.range(), s.range());
                push(&mut out, s, resized_sum_range(sum, crit.height(), crit.width()));
            }
            (_, Some(Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Array(_) | Expr::Missing)) => {}
            _ => dynamic = true,
        },
        Expr::Call(n, _)
            if matches!(n.as_str(), "INDIRECT" | "OFFSET" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO") =>
        {
            dynamic = true
        }
        Expr::Name(_) | Expr::Struct(_) => dynamic = true,
        Expr::Call(n, _) if gridcraft_functions::lookup(n).is_none() && !is_special(n) => dynamic = true,
        // A range ending in a function's result (`A1:INDEX(B1:B9,n)`) covers the cells between
        // the references inside it, not only those references.
        Expr::Binary(BinOp::Range, a, b) if !matches!((&**a, &**b), (Expr::Ref(_), Expr::Ref(_))) => {
            let mut inner = precedents(wb, sheet, a).0;
            inner.extend(precedents(wb, sheet, b).0);
            let mut boxes: Vec<Area> = Vec::new();
            for area in inner {
                match boxes.iter_mut().find(|x| x.sheet == area.sheet) {
                    Some(x) => x.range = x.range.union(&area.range),
                    None => boxes.push(area),
                }
            }
            out.extend(boxes);
        }
        _ => {}
    });
    (out, dynamic)
}

fn is_special(n: &str) -> bool {
    matches!(
        n,
        "IF" | "IFS"
            | "IFERROR"
            | "IFNA"
            | "CHOOSE"
            | "SWITCH"
            | "ROW"
            | "COLUMN"
            | "ROWS"
            | "COLUMNS"
            | "AREAS"
            | "ISREF"
            | "ISFORMULA"
            | "FORMULATEXT"
            | "SHEET"
            | "SHEETS"
            | "OFFSET"
            | "INDIRECT"
            | "INDEX"
            | "CELL"
            | "TRIMRANGE"
            | "SUBTOTAL"
            | "AGGREGATE"
            | "TEXT"
            | "LET"
            | "LAMBDA"
            | "ISOMITTED"
            | "MAP"
            | "REDUCE"
            | "SCAN"
            | "BYROW"
            | "BYCOL"
            | "MAKEARRAY"
    )
}

/// Names of functions the evaluator implements itself (merged into the function list for UI).
pub const SPECIAL_FUNCTIONS: &[&str] = &[
    "IF",
    "IFS",
    "IFERROR",
    "IFNA",
    "CHOOSE",
    "SWITCH",
    "ROW",
    "COLUMN",
    "ROWS",
    "COLUMNS",
    "AREAS",
    "ISREF",
    "ISFORMULA",
    "FORMULATEXT",
    "SHEET",
    "SHEETS",
    "OFFSET",
    "INDIRECT",
    "INDEX",
    "CELL",
    "TRIMRANGE",
    "SUBTOTAL",
    "AGGREGATE",
    "TEXT",
    "LET",
    "LAMBDA",
    "ISOMITTED",
    "MAP",
    "REDUCE",
    "SCAN",
    "BYROW",
    "BYCOL",
    "MAKEARRAY",
];

pub fn is_known_function(n: &str) -> bool {
    is_special(&n.to_ascii_uppercase()) || gridcraft_functions::lookup(n).is_some()
}

#[allow(dead_code)]
fn unused(_: RefKind) {}
