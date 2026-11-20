use super::{
    Area, BinOp, CellError, Ev, Expr, ExternalBook, ExternalName, ExternalSheet, LINKED_BOOK_BITS, LINKED_SHEET_BITS, LINKED_SHEETS, Reference,
    SheetSel, Value, Workbook,
};
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};

/// SUMIF and AVERAGEIF with a sum range: Excel sizes the sum range like the criteria range.
pub(super) fn resizes_sum_range(name: &str, args: usize) -> bool {
    args == 3 && matches!(name, "SUMIF" | "AVERAGEIF")
}

/// Height and width of a single-area reference or an array argument.
pub(super) fn ev_shape(ev: &Ev) -> Option<(u32, u32)> {
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

/// `[2]!Rates` → linked workbook 2, no sheet, `Rates`; `[2]Prices!Rates` → sheet `Prices`.
pub(super) fn external_name(name: &str) -> Option<(usize, &str, &str)> {
    let (book, rest) = name.strip_prefix('[')?.split_once(']')?;
    let (sheet, bare) = rest.split_once('!')?;
    if book.is_empty() || !book.bytes().all(|b| b.is_ascii_digit()) || bare.is_empty() {
        return None;
    }
    Some((book.parse().ok()?, sheet, bare))
}

/// Whether argument `i` of `name` is a range parameter that takes only a reference.
pub fn takes_reference(name: &str, i: usize) -> bool {
    match name {
        "SUMIF" | "AVERAGEIF" => i == 0 || i == 2,
        "COUNTIF" | "COUNTBLANK" => i == 0,
        "COUNTIFS" => i.is_multiple_of(2),
        "SUMIFS" | "AVERAGEIFS" | "MAXIFS" | "MINIFS" => i == 0 || i % 2 == 1,
        _ => false,
    }
}

/// `[2]Prices` (the sheet part of `'[2]Prices'!B2`) → linked workbook 2, sheet `Prices`.
pub(super) fn external_sheet(name: &str) -> Option<(usize, &str)> {
    let (book, sheet) = name.strip_prefix('[')?.split_once(']')?;
    if book.is_empty() || !book.bytes().all(|b| b.is_ascii_digit()) || sheet.is_empty() {
        return None;
    }
    Some((book.parse().ok()?, sheet))
}

/// The sheet number of an [`Area`] in linked workbook `book` (index into
/// `Workbook::external_books`), sheet `sheet` (index into its `sheets`); `None` past the limits.
pub(super) fn linked_sheet_number(book: usize, sheet: usize) -> Option<usize> {
    (book < 1 << LINKED_BOOK_BITS && sheet < 1 << LINKED_SHEET_BITS).then_some(LINKED_SHEETS + (book << LINKED_SHEET_BITS) + sheet)
}

/// `(book, sheet)` of an area's sheet number in a linked workbook (see [`linked_sheet_number`]);
/// `None` for a sheet of this workbook.
pub fn linked_parts(sheet: usize) -> Option<(usize, usize)> {
    let k = sheet.checked_sub(LINKED_SHEETS)?;
    Some((k >> LINKED_SHEET_BITS, k & ((1 << LINKED_SHEET_BITS) - 1)))
}

/// The used range of an area's sheet (`Sheet::used_range`, which walks every stored row); for a
/// sheet of a linked workbook, the block its saved cells cover.
pub(crate) fn used_range_of(wb: &Workbook, sheet: usize) -> Option<RangeRef> {
    match linked_parts(sheet) {
        Some((book, s)) => wb.external_books.get(book)?.sheets.get(s)?.extent(),
        None => wb.sheet(sheet)?.used_range(),
    }
}

/// Whether a linked workbook's name `def` (parsed as `expr`) can be calculated from the values
/// saved with the link: no `#REF!` in it, and only the link's saved sheets and names. A sheet
/// range, a sheetless reference in a workbook-level name and a table can't be read either.
pub(super) fn linked_definition_readable(b: &ExternalBook, def: &ExternalName, expr: &Expr) -> bool {
    let scope = def.sheet.and_then(|i| b.sheets.get(i)).map(|s| s.name.as_str());
    let mut readable = true;
    expr.walk(&mut |e| {
        readable &= match e {
            Expr::Error(CellError::Ref) | Expr::Struct(_) | Expr::LinkedRefError(_) => false,
            Expr::Ref(r) => match &r.sheet {
                SheetSel::Named(s) => b.sheet_index(s).is_some(),
                SheetSel::Current => scope.is_some(),
                SheetSel::Span(..) => false,
            },
            Expr::Name(n) => match n.split_once('!') {
                Some((s, bare)) => b.name(bare, Some(s.trim_matches('\''))).is_some(),
                None => b.name(n, scope).is_some(),
            },
            _ => true,
        };
    });
    readable
}

/// A value read from a linked sheet's saved cells. When the last update of the link failed for
/// the sheet (`refreshError`), Excel keeps the values it had and shows `#REF!` for anything it
/// hadn't saved.
pub fn linked_value(s: &ExternalSheet, v: Value) -> Value {
    if v.is_empty() && s.refresh_error { Value::Error(CellError::Ref) } else { v }
}

/// Most expression nodes [`precedents`] visits before giving up and marking the formula dynamic.
const MAX_PRECEDENT_NODES: usize = 1_000_000;

/// Collects the static references of an expression with their sheets resolved; `dynamic` is
/// set when the formula uses INDIRECT/OFFSET/names/tables whose targets can change.
///
/// Every node is visited once (#2183: range-operator chains used to re-walk shared operands,
/// costing 2^k for k operands), with an explicit stack, so a flat chain as long as Excel allows
/// (`1+1+…`, about 4,000 levels deep) never recurses. Past [`MAX_PRECEDENT_NODES`] the result
/// is conservative: `dynamic` is set so the formula recalculates every time.
pub fn precedents(wb: &Workbook, sheet: usize, e: &Expr) -> (Vec<Area>, bool) {
    let mut c = Precedents { wb, sheet, out: Vec::new(), dynamic: false, boxes: Vec::new() };
    c.collect(e);
    (c.out, c.dynamic)
}

/// Grows the per-sheet bounding box in `boxes` to cover `area`.
fn union_box(boxes: &mut Vec<Area>, area: Area) {
    match boxes.iter_mut().find(|x| x.sheet == area.sheet) {
        Some(x) => x.range = x.range.union(&area.range),
        None => boxes.push(area),
    }
}

/// One entry of [`Precedents::collect`]'s explicit stack. `boxes` indexes
/// [`Precedents::boxes`]: the bounding boxes of the innermost enclosing range operator, if any.
enum PrecedentStep<'e> {
    /// Visit an expression and queue its children.
    Visit(&'e Expr, Option<usize>),
    /// Both operands of a range operator have been visited: record the bounding boxes they
    /// collected in `inner`, growing the `outer` operator's boxes too.
    Close { inner: usize, outer: Option<usize> },
}

struct Precedents<'w> {
    wb: &'w Workbook,
    sheet: usize,
    out: Vec<Area>,
    dynamic: bool,
    /// Per-sheet bounding boxes, one list per range operator (`A1:INDEX(…)`) met so far.
    boxes: Vec<Vec<Area>>,
}

impl Precedents<'_> {
    /// Records `area`, also growing the bounding boxes of the enclosing range operator, if any.
    fn add(&mut self, area: Area, boxes: Option<usize>) {
        if let Some(b) = boxes.and_then(|i| self.boxes.get_mut(i)) {
            union_box(b, area);
        }
        self.out.push(area);
    }

    fn push(&mut self, r: &Reference, range: RangeRef, boxes: Option<usize>) {
        match &r.sheet {
            SheetSel::Current => self.add(Area { sheet: self.sheet, range }, boxes),
            SheetSel::Named(n) => {
                if let Some(i) = self.wb.sheet_index(n) {
                    self.add(Area { sheet: i, range }, boxes);
                }
            }
            SheetSel::Span(a, b) => {
                if let (Some(i), Some(j)) = (self.wb.sheet_index(a), self.wb.sheet_index(b)) {
                    for s in i.min(j)..=i.max(j) {
                        self.add(Area { sheet: s, range }, boxes);
                    }
                }
            }
        }
    }

    /// Visits every node of `root` once, parents before children, left to right. Areas found
    /// inside a range operator (`A1:INDEX(…)`) also grow that operator's per-sheet bounding boxes.
    fn collect(&mut self, root: &Expr) {
        let mut pending = vec![PrecedentStep::Visit(root, None)];
        let mut nodes = 0usize;
        while let Some(step) = pending.pop() {
            let (x, boxes) = match step {
                PrecedentStep::Visit(x, boxes) => (x, boxes),
                PrecedentStep::Close { inner, outer } => {
                    let areas = self.boxes.get_mut(inner).map(std::mem::take).unwrap_or_default();
                    for area in areas {
                        self.add(area, outer);
                    }
                    continue;
                }
            };
            nodes = nodes.saturating_add(1);
            if nodes > MAX_PRECEDENT_NODES {
                self.dynamic = true;
                return;
            }
            self.visit(x, boxes, &mut pending);
        }
    }

    /// Handles one node for [`Precedents::collect`], queueing the children still to visit.
    fn visit<'e>(&mut self, x: &'e Expr, boxes: Option<usize>, pending: &mut Vec<PrecedentStep<'e>>) {
        match x {
            Expr::Ref(r) => self.push(r, r.range(), boxes),
            // The cells SUMIF/AVERAGEIF really read from the sum range (see `resized_sum_range`); when
            // the criteria range's size isn't known statically, recalculate the formula every time.
            Expr::Call(n, args) if resizes_sum_range(n, args.len()) => match (args.first(), args.get(2)) {
                (Some(Expr::Ref(c)), Some(Expr::Ref(s))) => {
                    let (crit, sum) = (c.range(), s.range());
                    self.push(s, resized_sum_range(sum, crit.height(), crit.width()), boxes);
                }
                (
                    _,
                    Some(Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::LinkedRefError(_) | Expr::Array(_) | Expr::Missing),
                ) => {}
                _ => self.dynamic = true,
            },
            // GETPIVOTDATA reads the report cells, so it depends on the whole report its reference
            // points into (pivot commands recalculate everything). Any other reference is dynamic.
            Expr::Call(n, args) if n == "GETPIVOTDATA" => match args.get(1) {
                Some(Expr::Ref(r)) => {
                    let si = match &r.sheet {
                        SheetSel::Current => Some(self.sheet),
                        SheetSel::Named(name) => self.wb.sheet_index(name),
                        SheetSel::Span(..) => None,
                    };
                    let report = si
                        .and_then(|i| self.wb.sheet(i))
                        .and_then(|sh| sh.pivots.iter().find_map(|p| p.last_range.filter(|lr| lr.intersects(&r.range()))));
                    match report {
                        Some(lr) => self.push(r, lr, boxes),
                        None => self.dynamic = true,
                    }
                }
                _ => self.dynamic = true,
            },
            Expr::Call(n, _)
                if matches!(n.as_str(), "INDIRECT" | "OFFSET" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO") =>
            {
                self.dynamic = true
            }
            Expr::Name(_) | Expr::Struct(_) => self.dynamic = true,
            Expr::Call(n, _) if gridcraft_functions::lookup(n).is_none() && !is_special(n) => self.dynamic = true,
            // A range ending in a function's result (`A1:INDEX(B1:B9,n)`), or a chain of plain
            // references the parser can't fold into one (`A1:B2:C3`), covers the cells between the
            // references inside it, not only those references. Both operands are visited once,
            // collecting their bounding boxes on the way, which `Close` then records.
            Expr::Binary(BinOp::Range, a, b) => {
                let inner = self.boxes.len();
                self.boxes.push(Vec::new());
                pending.push(PrecedentStep::Close { inner, outer: boxes });
                pending.push(PrecedentStep::Visit(b, Some(inner)));
                pending.push(PrecedentStep::Visit(a, Some(inner)));
                return;
            }
            _ => {}
        }
        // Children are pushed in reverse so they pop left to right.
        let visit = |e: &'e Expr| PrecedentStep::Visit(e, boxes);
        match x {
            Expr::Array(rows) => pending.extend(rows.iter().flatten().rev().map(visit)),
            Expr::Unary(_, e) | Expr::Paren(e) => pending.push(visit(e)),
            Expr::Binary(_, a, b) => {
                pending.push(visit(b));
                pending.push(visit(a));
            }
            Expr::Call(_, args) => pending.extend(args.iter().rev().map(visit)),
            Expr::Invoke(c, args) => {
                pending.extend(args.iter().rev().map(visit));
                pending.push(visit(c));
            }
            _ => {}
        }
    }
}

/// What a formula can read in this workbook besides its static references ([`precedents`]):
/// the targets of the local names it uses, through other names too, and the tables it refers
/// to. `names` lists those names. `None` when that isn't known without evaluating it:
/// INDIRECT, OFFSET, CELL, INFO, a SUMIF whose criteria range isn't a reference, or a name
/// with relative references.
pub struct DynamicReads {
    pub areas: Vec<Area>,
    pub names: Vec<String>,
}

pub fn dynamic_reads(wb: &Workbook, sheet: usize, e: &Expr) -> Option<DynamicReads> {
    let mut out = DynamicReads { areas: Vec::new(), names: Vec::new() };
    collect_dynamic_reads(wb, sheet, e, &mut out, 0).then_some(out)
}

fn collect_dynamic_reads(wb: &Workbook, sheet: usize, e: &Expr, out: &mut DynamicReads, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    let mut known = true;
    let tables = |out: &mut DynamicReads, si: usize, ti: usize| {
        if let Some(t) = wb.sheet(si).and_then(|s| s.tables.get(ti)) {
            if !out.names.iter().any(|n| n.eq_ignore_ascii_case(&t.name)) {
                out.names.push(t.name.clone());
            }
            out.areas.push(Area { sheet: si, range: t.range });
        }
    };
    let name = |out: &mut DynamicReads, n: &str| -> bool {
        let (scope_sheet, bare) = match n.split_once('!') {
            Some((s, b)) => match wb.sheet_index(s.trim_matches('\'')) {
                Some(i) => (i, b),
                None => return true,
            },
            None => (sheet, n),
        };
        if out.names.iter().any(|x| x.eq_ignore_ascii_case(bare)) {
            return true;
        }
        out.names.push(bare.to_string());
        if let Some(def) = wb.name(bare, scope_sheet) {
            let scope = def.scope.unwrap_or(sheet);
            return match gridcraft_formula::parse_name(&def.formula) {
                Ok(expr) if !gridcraft_formula::adjust::has_relative_refs(&expr) => {
                    out.areas.extend(precedents(wb, scope, &expr).0);
                    collect_dynamic_reads(wb, scope, &expr, out, depth + 1)
                }
                Ok(_) => false,
                Err(_) => false,
            };
        }
        if let Some((si, ti)) = wb.table(bare) {
            tables(out, si, ti);
        }
        // Otherwise a LET or LAMBDA variable, or a name nothing defines.
        true
    };
    e.walk(&mut |x| match x {
        Expr::Name(n) if external_name(n).is_none() => known &= name(out, n),
        Expr::Struct(s) if s.table.is_empty() => {
            for ti in 0..wb.sheet(sheet).map_or(0, |s| s.tables.len()) {
                tables(out, sheet, ti);
            }
        }
        Expr::Struct(s) => {
            if let Some((si, ti)) = wb.table(&s.table) {
                tables(out, si, ti);
            }
        }
        Expr::Call(n, _)
            if matches!(n.as_str(), "INDIRECT" | "OFFSET" | "CELL" | "INFO" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY") =>
        {
            known = false
        }
        Expr::Call(n, args)
            if resizes_sum_range(n, args.len()) && !matches!(args.first(), Some(Expr::Ref(_))) && matches!(args.get(2), Some(Expr::Ref(_))) =>
        {
            known = false;
        }
        // A function defined by a name (a LAMBDA).
        Expr::Call(n, _) if gridcraft_functions::lookup(n).is_none() && !is_special(n) => known &= name(out, n),
        _ => {}
    });
    known
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
            | "GETPIVOTDATA"
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
    "GETPIVOTDATA",
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
