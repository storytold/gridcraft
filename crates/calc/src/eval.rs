//! The formula evaluator.
//!
//! Expressions evaluate to [`Ev`]: a value, a reference (one or more areas, kept unresolved so
//! `ROW`, `OFFSET`, `INDEX`, `:` and friends can work on references) or a lambda. Cell reads go
//! through [`Host`], which lets the recalculation engine evaluate dirty precedents on demand.

use std::sync::Arc;

use gridcraft_core::{Array, CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, compare};
use gridcraft_formula::{BinOp, Expr, Reference, SheetSel, StructItem, StructRef, UnOp};
use gridcraft_functions::{Arg, Asked, Ctx, Halt};
use gridcraft_model::Workbook;

mod array;
mod call;
mod fence;
mod format;
mod lambda;
mod reference_fns;
mod special;

use array::map_unary;
pub use array::{binary_values, zip2};
pub use call::{CallGives, call_gives};
use fence::Reader;
pub use format::r1c1_to_a1;
#[cfg(test)]
pub(crate) use special::precedents_with;
#[cfg(test)]
pub(crate) use special::{AREAS_SIFTED, BOUND_NAMES_COMPARED, BOXES_LOOKED_AT, FOLD_BUDGET, FREE_NAMES_COPIED, NAME_INDEXES_BUILT, SCANNED_UP_TO};
pub(crate) use special::{NAME_LOOKUPS_SCANNED, NameCache, NameIndex, NameLookup, precedents_drawing};
pub use special::{SPECIAL_FUNCTIONS, is_known_function, precedents, resized_sum_range};

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

/// Whether reading `range` as a block ([`Evaluator::read_range`]: every function argument and
/// operator) cuts it to the sheet's used range: over `FULL_SIZE_CELLS` cells and running to the
/// sheet's edge, or over `FULL_SIZE_BOUNDED_CELLS` cells.
pub(crate) fn read_is_cut(range: &RangeRef) -> bool {
    let cells = u64::from(range.height()) * u64::from(range.width());
    // `A:A`, `1:1` and `A2:A1048576` run to the edge of the sheet: they stand for "the rest of
    // the column", not for a block of a chosen size.
    let to_edge = range.end.row == MAX_ROWS - 1 || range.end.col == MAX_COLS - 1;
    cells > FULL_SIZE_CELLS && (to_edge || cells > FULL_SIZE_BOUNDED_CELLS)
}

/// Whether ROW and COLUMN cut `range` to the sheet's used range (`Evaluator::trim_lines`): whole
/// columns or whole rows.
pub(crate) fn lines_are_cut(range: &RangeRef) -> bool {
    range.is_full_cols() || range.is_full_rows()
}

/// Whether `read_range` or `trim_lines` may cut `range` to the sheet's used range, by either rule
/// above: what they read then depends on cells outside `range`. The precedent walk asks this
/// (`eval/special.rs`), and the two evaluator functions call the rule each follows. TRIMRANGE,
/// SUBTOTAL and AGGREGATE cut by their own rules (see the walk's list of dynamic cases).
pub(crate) fn may_be_cut(range: &RangeRef) -> bool {
    read_is_cut(range) || lines_are_cut(range)
}
const MAX_DEPTH: usize = 400;
/// Lambda invocations one cell may make. A semantically bounded formula in Excel can invoke
/// its lambda at most once per cell of an array (capped here by `MAX_CELLS`), so this covers
/// every legitimate call; hostile work that grows past one dimension (a LAMBDA that recurses
/// or nest `MAP` into `MAP`) then stops at the same bound instead of squaring.
const LAMBDA_FUEL: u64 = MAX_CELLS;
/// Work one cell's formula may do: one unit per expression evaluated, per cell read from a
/// reference, per array value handed to a function, per pair of areas intersected and per
/// character of a defined name's formula parsed where the name is used. Each limit
/// above bounds one kind of step, but a compact formula could still repeat bounded steps
/// exponentially often (defined names that each use the one before twice, a computed criterion
/// whose database function reruns it, an aggregate over a large array inside every LAMBDA call).
/// Past this the formula is `#CALC!`. It is far above what a formula that reads every cell of a
/// full-size array a few times needs.
const EVAL_WORK: u64 = 16 * MAX_CELLS;

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

/// What the tail of a LAMBDA body came to.
enum Tail {
    Done(Ev),
    /// A call to make in place of the current one, its arguments evaluated.
    Call(Arc<Lambda>, Vec<(String, Ev)>),
}

/// A tail call can pass a closure that captures the previous one, thousands deep. Releasing the
/// chain is a loop, not a recursion, so it cannot overflow the native stack.
impl Drop for Lambda {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.env);
        while let Some((_, ev)) = pending.pop() {
            if let Ev::L(l) = ev
                && let Ok(mut inner) = Arc::try_unwrap(l)
            {
                pending.append(&mut inner.env);
            }
        }
    }
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

/// LAMBDA calls that one formula may have in progress at once, counted the same way on every build.
/// The next call answers `#NUM!`. A countdown from 131, the deepest that `MAX_DEPTH` allows, has 132
/// calls in progress, so it still runs.
const MAX_LAMBDA_CALLS: usize = 140;

/// Tail calls one LAMBDA call may chain. They run in a loop on one native frame, so this only keeps
/// an endless tail recursion finite. Excel computes a tail countdown to 5,459 and answers `#NUM!`
/// for 5,460, which is 5,460 invocations in the chain.
const MAX_TAIL_CALLS: usize = 5460;

/// Native stack, in bytes, that one formula's evaluation may use below its first `eval`. Past it,
/// `eval` answers `#NUM!`. It guards the stack and leaves 128 KiB of a 1 MiB thread for the frames
/// above the formula. A cell that another cell's formula computes on demand shares the budget of the
/// outermost formula ([`Host::stack_top`]).
const STACK_BUDGET: usize = 896 * 1024;

/// The address of a local variable, which shows how far down the native stack this call is.
#[inline(always)]
pub(crate) fn stack_position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
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
    /// Where in the workbook's names the defined name visible from `scope_sheet` is, as
    /// [`Workbook::name_position`] finds it, or `None` for no such name. The evaluator charges the
    /// definition's text before it asks [`Host::name_expr`] for its parse.
    fn name_position(&mut self, name: &str, scope_sheet: usize) -> Option<usize> {
        self.workbook().name_position(name, scope_sheet)
    }
    /// The parsed definition of the name at `position` in the workbook's names, `None` when it
    /// doesn't parse (the name evaluates to `#NAME?`). The default parses on every call; a recalc
    /// pass caches per name, since a formula column calling a LAMBDA name re-parses that name once
    /// per cell otherwise.
    fn name_expr(&mut self, position: usize) -> Option<Arc<Expr>> {
        let def = self.workbook().names.get(position)?;
        gridcraft_formula::parse_name(&def.formula).ok().map(Arc::new)
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
    /// `(sum, count)` of the numbers in a block that holds nothing but constants (no formula,
    /// error or spilled value), added row-major from `0.0` exactly as `visit_range` would hand
    /// them over. `None`: the block isn't like that, or this host can't tell.
    fn range_sum(&mut self, _sheet: usize, _range: RangeRef) -> Option<(f64, u64)> {
        None
    }
    /// While a formula is calculated on demand inside another one: `stack_position()` where the
    /// outermost one started, which the stack budget is counted from.
    fn stack_top(&self) -> Option<usize> {
        None
    }
    /// Called when a formula's evaluation runs past the stack budget.
    fn stack_exhausted(&mut self) {}
    /// Whether the result of the evaluation will be thrown away whatever it comes to (a worker
    /// thread handing its formula back). Only a read of cells or a random draw makes it so. The
    /// evaluator asks only through its `fence` module, after each read and each call of a
    /// volatile function, and its loops through `Gather`; then it stops (see
    /// [`gridcraft_functions::Halt`]).
    fn halted(&self) -> bool {
        false
    }
    /// Whether `halted` can become true on this host at all (a worker thread's), where every
    /// random draw makes it so. Asked once when an [`Evaluator`] is made: where it can't (one
    /// thread, the main thread) nothing is asked after a read and a loop has no check per item.
    fn can_halt(&self) -> bool {
        false
    }
}

pub struct Evaluator<'h> {
    /// Its reads go through the evaluator's `fence` methods (see [`gridcraft_functions::Halt`]).
    pub(crate) host: Reader<'h>,
    /// `Host::can_halt`, asked once.
    can_halt: bool,
    pub sheet: usize,
    pub at: CellRef,
    env: Vec<(String, Ev)>,
    depth: usize,
    /// LAMBDA calls in progress.
    calls: usize,
    /// `stack_position()` at the outermost `eval`.
    stack_top: usize,
    /// Lambda invocations this cell still has left. Every call spends one charge, so a
    /// compact hostile formula (a LAMBDA that calls itself, nested `MAP`s over big arrays)
    /// runs out of work after the same number of calls that a legitimately wide one could
    /// make, instead of an amount of work that squares with array nesting (#2382).
    fuel: u64,
    /// Work this cell still has left (see [`EVAL_WORK`]), shared by every branch of the formula
    /// and by the formulas it computes on the way (names, computed criteria).
    work: u64,
    /// A spend found too little work left. A spend that leaves exactly nothing is fine, so this
    /// can't be told from `work` alone; a database function checks it after its computed
    /// criteria ran, whose failures it can't otherwise see.
    exhausted: bool,
    /// Computed criteria being evaluated, outermost first: one that is reached again from inside
    /// itself counts as not met instead of repeating its work at every level.
    active_criteria: Vec<(usize, CellRef)>,
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
    /// The evaluation stops after a volatile function's call (`Evaluator::call_function`), not
    /// after each draw: a function drawing many draws through `Gather`.
    fn random(&mut self) -> f64 {
        self.ev.host.random()
    }
    /// Excel's "formula as criteria": the formula is written for the first record, so its relative
    /// references move down to `record`'s row, and the database's labels name its fields.
    /// The shifted references are not dependencies of the calling cell, so the precedent walk in
    /// `eval/special.rs` (`precedents_with`) makes every formula calling a D-function, or reading
    /// a name whose definition holds one, dynamic: it is evaluated at every recalculation, so a
    /// shifted reference that lands outside both the database and the criteria cell's own
    /// precedents (say `=G2<H2` with column H outside the database) is read afresh after any edit,
    /// as a full recalculation reads it. The graph still doesn't list the shifted cells, so Trace
    /// Precedents and Trace Dependents don't show them.
    fn computed_criterion(&mut self, row: usize, col: usize, db: &Array, record: usize) -> Option<bool> {
        let area = self.criteria?;
        let at = area.range.start.offset(row as i64, col as i64).filter(|c| area.range.contains(*c))?;
        if self.ev.active_criteria.contains(&(area.sheet, at)) {
            return Some(false);
        }
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
        let mut ev = Evaluator {
            host: self.ev.host.reborrow(),
            can_halt: self.ev.can_halt,
            sheet: area.sheet,
            at,
            env,
            depth: self.ev.depth,
            calls: self.ev.calls,
            stack_top: self.ev.stack_top,
            fuel: self.ev.fuel,
            work: self.ev.work,
            exhausted: false,
            active_criteria: std::mem::take(&mut self.ev.active_criteria),
        };
        ev.active_criteria.push((area.sheet, at));
        let v = matches!(ev.value(&shifted).scalar().to_bool(), Ok(true));
        // The child's spend counts against this cell's budget.
        self.ev.fuel = ev.fuel;
        self.ev.work = ev.work;
        self.ev.exhausted |= ev.exhausted;
        let mut active = std::mem::take(&mut ev.active_criteria);
        active.pop();
        self.ev.active_criteria = active;
        Some(v)
    }
    fn exhausted(&self) -> bool {
        self.ev.exhausted
    }
    fn can_halt(&self) -> bool {
        Halt::can_halt(&*self.ev)
    }
    fn halted(&mut self, asked: Asked) -> bool {
        self.ev.draws_halted(asked)
    }
    fn has_lambda(&self) -> bool {
        self.lambda.is_some()
    }
    /// A LAMBDA must give one value per group; an array result is #CALC!.
    fn call_lambda(&mut self, arg: Value) -> Option<Value> {
        let l = self.lambda.clone()?;
        if self.ev.refuses_calls() {
            return Some(Value::Error(CellError::Calc));
        }
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
        let host = Reader::new(host);
        Evaluator {
            can_halt: host.can_halt(),
            host,
            sheet,
            at,
            env: Vec::new(),
            depth: 0,
            calls: 0,
            stack_top: 0,
            fuel: LAMBDA_FUEL,
            work: EVAL_WORK,
            exhausted: false,
            active_criteria: Vec::new(),
        }
    }

    /// Gives this evaluation `work` units instead of [`EVAL_WORK`] (never more than that).
    pub fn with_work(mut self, work: u64) -> Self {
        self.work = work.min(EVAL_WORK);
        self
    }

    /// Whether a spend found too little work left (the formula stopped with `#CALC!`).
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Spends `units` of this cell's work; `false` (and nothing left) when there isn't enough.
    pub(crate) fn spend(&mut self, units: u64) -> bool {
        match self.work.checked_sub(units) {
            Some(w) => {
                self.work = w;
                true
            }
            None => {
                self.work = 0;
                self.exhausted = true;
                false
            }
        }
    }

    /// Charges the values of an array (one unit per value; one for anything else).
    pub(crate) fn spend_on(&mut self, v: &Value) -> bool {
        match v {
            Value::Array(a) => self.spend(a.data.len() as u64),
            _ => self.spend(1),
        }
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
            return self.read_cell(a.sheet, a.range.start);
        }
        // A reference keeps its size, so the array has its shape and its blanks count
        // (`=A1:A10`, `SUMPRODUCT(--(A1:A10=""))`).
        let range = self.read_range(a);
        let (h, w) = (range.height() as usize, range.width() as usize);
        if (h as u64) * (w as u64) > MAX_CELLS {
            return Value::Error(CellError::Num);
        }
        if !self.spend((h as u64) * (w as u64)) {
            return Value::Error(CellError::Calc);
        }
        let data = self.read_values(a.sheet, range);
        Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
    }

    /// The block that reading `a` gives: `a` itself, or when it is large (or runs to the sheet's
    /// edge) the part inside the sheet's used range.
    pub(crate) fn read_range(&mut self, a: &Area) -> RangeRef {
        if read_is_cut(&a.range) { self.trim(a) } else { a.range }
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
        let here = stack_position();
        if self.depth == 0 {
            self.stack_top = self.host.stack_top().unwrap_or(here);
        }
        if self.stack_top.abs_diff(here) > STACK_BUDGET {
            self.host.stack_exhausted();
            return err(CellError::Num);
        }
        if !self.spend(1) {
            return err(CellError::Calc);
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return err(CellError::Num);
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
                // A ragged literal is an error either way; one wide row and many empty ones must
                // not reserve their rows-times-width product first.
                if rows.iter().any(|row| row.len() != w) {
                    return err(CellError::Value);
                }
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
        // The precedent walk resolves names by the same rules (`Walk::name` and `Walk::collect` in
        // `eval/special.rs`): LET/LAMBDA bindings before defined names, the split at the last `!`,
        // a name scoped to the sheet before a workbook-wide one. A change here changes it there.
        if let Some((_, v)) = self.env.iter().rev().find(|(k, _)| gridcraft_formula::same_name(k, n)) {
            return v.clone();
        }
        // Sheet-qualified `Sheet1!Name`. The parser stores the sheet unquoted, so it is used as
        // is (`' Sales '` keeps its spaces) and split at the last `!` (`'a!b'!Name`).
        let (scope_sheet, bare) = match n.rsplit_once('!') {
            Some((s, b)) => match self.host.workbook().sheet_index(s) {
                Some(i) => (i, b),
                None => return err(CellError::Name),
            },
            None => (self.sheet, n),
        };
        if let Some(position) = self.host.name_position(bare, scope_sheet)
            && let Some((text_len, def_scope)) = self.host.workbook().names.get(position).map(|d| (d.formula.len(), d.scope))
        {
            let scope = def_scope.unwrap_or(self.sheet);
            // Each use evaluates the definition again: names that each use the one before twice
            // double the work per name, so it is charged, by its text as if parsed again, whether
            // or not the host keeps the parse (the result can't depend on the host). The charge
            // comes first: a cell without the work left doesn't get the definition parsed.
            if !self.spend(text_len as u64) {
                return err(CellError::Calc);
            }
            let Some(expr) = self.host.name_expr(position) else { return err(CellError::Name) };
            // Relative references in a name are relative to A1: used here, they move by this
            // cell's offset from it. Only then is the shared parse copied.
            let shifted;
            let expr: &Expr = if gridcraft_formula::adjust::has_relative_refs(&expr) {
                shifted = gridcraft_formula::adjust::shift_relative_wrapping((*expr).clone(), self.at.row as i64, self.at.col as i64);
                &shifted
            } else {
                &expr
            };
            let saved = self.sheet;
            // Unqualified references in a name refer to the scope sheet.
            self.sheet = scope;
            let r = self.eval(expr);
            self.sheet = saved;
            return r;
        }
        // A table name alone means its data body.
        let wb = self.host.workbook();
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
                    [a] if a.range.is_single() => match self.read_spill(a.sheet, a.range.start) {
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
                        Some(c) => Ev::V(self.read_cell(a.sheet, c)),
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
        // A 2-D range intersects on both the formula's row and column (observed in Excel, on
        // another sheet too); one that includes the formula's own cell is a circular reference.
        if r.start.row <= self.at.row && self.at.row <= r.end.row && r.start.col <= self.at.col && self.at.col <= r.end.col {
            return Some(self.at);
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
                        // A union reused through LET doubles at each step: its areas count.
                        if !self.spend((xa.len() as u64).saturating_add(ya.len() as u64)) {
                            return err(CellError::Calc);
                        }
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
                        // Every pair is looked at, and each may be kept: charged before the loop,
                        // so unions squared through LET can't build an enormous list.
                        if !self.spend((xa.len() as u64).saturating_mul(ya.len() as u64)) {
                            return err(CellError::Calc);
                        }
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
                // of one level of recursion per operator (past the depth limit it was an error).
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
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod fuel_tests {
    use super::*;
    use gridcraft_model::Workbook;

    struct Empty {
        wb: Workbook,
    }
    impl Host for Empty {
        fn workbook(&self) -> &Workbook {
            &self.wb
        }
        fn cell_value(&mut self, _sheet: usize, _c: CellRef) -> Value {
            Value::Empty
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
    }

    fn eval_with(fuel: u64, src: &str) -> Value {
        let mut host = Empty { wb: Workbook::default() };
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(0, 0));
        ev.fuel = fuel;
        ev.value(&gridcraft_formula::parser::parse(src).unwrap_or_else(|e| panic!("{e}")))
    }

    /// Evaluates `src` in `wb` with `work` units of work, and the work left.
    fn eval_work(wb: Workbook, work: u64, src: &str) -> (Value, u64) {
        let mut host = Empty { wb };
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(0, 0));
        ev.work = work;
        let v = ev.value(&gridcraft_formula::parser::parse(src).unwrap_or_else(|e| panic!("{e}")));
        (v, ev.work)
    }

    fn is_calc(v: &Value) -> bool {
        matches!(v, Value::Error(CellError::Calc))
    }

    /// Serves the workbook's stored values, for database functions to read.
    struct Stored {
        wb: Workbook,
    }
    impl Host for Stored {
        fn workbook(&self) -> &Workbook {
            &self.wb
        }
        fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
            self.wb.sheet(sheet).and_then(|s| s.cell(c)).map_or(Value::Empty, |cell| cell.value.clone())
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
    }

    /// A computed criterion that runs out of work counts as not met for its record, which the
    /// database function can't see: it must answer #CALC!, not a count that leaves records out.
    #[test]
    fn a_computed_criterion_out_of_work_makes_the_database_function_calc() {
        use gridcraft_model::{Cell, Formula};
        let mut wb = Workbook::new();
        let name = |name: String, formula: String| gridcraft_model::DefinedName { name, scope: None, formula, comment: String::new(), hidden: false };
        wb.names_mut().push(name("Step0".into(), "1".into()));
        for i in 1..=60 {
            wb.names_mut().push(name(format!("Step{i}"), format!("Step{}+Step{}", i - 1, i - 1)));
        }
        let at = |s: &str| CellRef::parse(s).unwrap();
        let sheet = wb.sheet_mut(0).unwrap();
        for (r, (n, age)) in [("name", Value::text("age")), ("a", Value::number(1.0)), ("b", Value::number(2.0))].into_iter().enumerate() {
            sheet.cells.set(at(&format!("A{}", r + 1)), Cell::value(Value::text(n)));
            sheet.cells.set(at(&format!("B{}", r + 1)), Cell::value(age));
        }
        sheet.cells.set(at("D1"), Cell::value(Value::text("Formula")));
        let host_wb = |criterion: &str| {
            let mut wb = wb.clone();
            // A criteria cell is read by its value first (a blank one matches everything): give it
            // the value a recalculation would have stored.
            let mut cell = Cell::formula(Formula::new(criterion));
            cell.value = Value::Bool(true);
            wb.sheet_mut(0).unwrap().cells.set(at("D2"), cell);
            wb
        };
        let run = |criterion: &str| {
            let mut host = Stored { wb: host_wb(criterion) };
            let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
            ev.work = 100_000;
            ev.value(&gridcraft_formula::parser::parse("=DCOUNT(A1:B3,\"age\",D1:D2)").unwrap())
        };
        assert_eq!(run("=B2>0"), Value::number(2.0), "a cheap criterion counts every record");
        assert!(is_calc(&run("=Step60>0")), "{:?}", run("=Step60>0"));
    }

    #[test]
    fn a_summed_union_ends_with_its_first_error() {
        // The union's areas are added in order, and the first error among the arguments, in
        // order, is the result, also when a later area or argument holds another.
        use gridcraft_model::Cell;
        let mut wb = Workbook::new();
        let sheet = wb.sheet_mut(0).unwrap();
        for (at, v) in [("A1", Value::Error(CellError::NA)), ("A2", Value::Error(CellError::Div0)), ("A3", Value::number(1.0))] {
            sheet.cells.set(CellRef::parse(at).unwrap(), Cell::value(v));
        }
        let run = |src: &str| {
            let mut host = Stored { wb: wb.clone() };
            let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
            ev.value(&gridcraft_formula::parser::parse(src).unwrap())
        };
        assert_eq!(run("=SUM((A3,A1,A2))"), Value::Error(CellError::NA));
        assert_eq!(run("=SUM((A3,A2,A1))"), Value::Error(CellError::Div0));
        assert_eq!(run("=AVERAGE((A2,A3),(A1,A3))"), Value::Error(CellError::Div0));
        assert_eq!(run("=SUM(A1,(A2,A3))"), Value::Error(CellError::NA));
        assert_eq!(run("=SUM((A3,A3))"), Value::number(2.0));
    }

    /// Serves the workbook's stored values, and on a read of A9 notes how many copies of `text`
    /// there are (the cells holding it, `text` and this host's copy, and any row holding it).
    struct Watching {
        wb: Workbook,
        text: Arc<str>,
        seen: Option<usize>,
    }
    impl Host for Watching {
        fn workbook(&self) -> &Workbook {
            &self.wb
        }
        fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
            if c == CellRef::new(8, 0) {
                self.seen = Some(Arc::strong_count(&self.text));
                return Value::number(1.0);
            }
            self.wb.sheet(sheet).and_then(|s| s.cell(c)).map_or(Value::Empty, |cell| cell.value.clone())
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
    }

    #[test]
    fn a_union_row_past_the_work_left_is_read_but_not_kept() {
        // A9 reports how many copies of the text C1:C5 share while it is read: the row must not
        // hold C5's copy, read after the row already held more values than the work left.
        use gridcraft_model::Cell;
        let text: Arc<str> = Arc::from("shared");
        let mut wb = Workbook::new();
        for r in 0..5 {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 2), Cell::value(Value::Text(text.clone())));
        }
        let run = |work: u64| {
            let mut host = Watching { wb: wb.clone(), text: text.clone(), seen: None };
            let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
            ev.work = work;
            let v = ev.value(&gridcraft_formula::parser::parse("=MAX((C1:C4,C5,A9))").unwrap());
            let left = ev.work;
            (v, host.seen, left)
        };
        // Stored: the five cells, `text` and the host's copy.
        let stored = 7;
        let runs: Vec<_> = (0..40).map(run).collect();
        for (v, seen, left) in &runs {
            if is_calc(v) {
                assert_eq!(*left, 0);
                // A row that keeps C5 must also keep C1:C4: one that holds C5 alone is a row
                // whose C1:C4 read ran out of work, and then the row has no work left either.
                assert!(seen.is_none_or(|s| s == stored || s == stored + 4 || s == stored + 5), "{seen:?}");
            }
        }
        // C1:C4 copied, more values than the work left: C5 is read (A9 comes after it) but let go.
        assert!(runs.iter().any(|(v, seen, _)| is_calc(v) && *seen == Some(stored + 4)), "{runs:?}");
        // With enough work the row keeps every value, and the function sees them.
        let (v, seen, _) = run(40);
        assert_eq!(v, Value::number(1.0));
        assert_eq!(seen, Some(stored + 5));
    }

    #[test]
    fn union_rows_together_past_the_work_left_are_read_but_not_kept() {
        // The charge counts the values of every argument, so the second union's row stops
        // copying once it and the first union's row together hold more values than the work left,
        // not only once it alone does. A9, read last, reports how many copies of the text in
        // C1:C6 the rows hold.
        use gridcraft_model::Cell;
        let text: Arc<str> = Arc::from("shared");
        let mut wb = Workbook::new();
        for r in 0..6 {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 2), Cell::value(Value::Text(text.clone())));
        }
        // Stored: the six cells, `text` and the host's copy.
        let stored = 8;
        let run = |work: u64| {
            let mut host = Watching { wb: wb.clone(), text: text.clone(), seen: None };
            let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
            ev.work = work;
            let v = ev.value(&gridcraft_formula::parser::parse("=MAX((C1:C2,C3),(C4,C5,C6,A9))").unwrap());
            (v, host.seen.map(|s| s - stored))
        };
        let runs: Vec<_> = (0..40).map(run).collect();
        // With the least work that reaches A9, the first row is whole and the second keeps C4
        // only: no work is left when C4 is kept.
        let least = runs.iter().position(|(_, kept)| kept.is_some()).unwrap();
        assert_eq!(runs[least], (Value::Error(CellError::Calc), Some(4)), "{runs:?}");
        // C5 is kept only once the work left when C4 is kept covers C4 and the first row's three
        // values: four units more. Counted alone, the second row would keep it with one more.
        let first_keeping = |copies| runs.iter().position(|(_, kept)| *kept == Some(copies)).unwrap();
        assert_eq!(first_keeping(5) - least, 4, "{runs:?}");
        // With enough work every value is kept, and the function sees them.
        assert_eq!(run(40), (Value::number(1.0), Some(6)));
    }

    #[test]
    fn names_that_double_at_each_step_run_out_of_work() {
        // Step1 = Step0+Step0, Step2 = Step1+Step1, ...: Step60 would evaluate Step0 2^60 times.
        let mut wb = Workbook::default();
        let name = |name: String, formula: String| gridcraft_model::DefinedName { name, scope: None, formula, comment: String::new(), hidden: false };
        wb.names_mut().push(name("Step0".into(), "1".into()));
        for i in 1..=60 {
            wb.names_mut().push(name(format!("Step{i}"), format!("Step{}+Step{}", i - 1, i - 1)));
        }
        let (v, _) = eval_work(wb.clone(), EVAL_WORK, "=Step5");
        assert_eq!(v, Value::number(32.0));
        let (v, left) = eval_work(wb, 100_000, "=Step60");
        assert!(is_calc(&v), "{v:?}");
        assert_eq!(left, 0);
    }

    #[test]
    fn intersections_of_unions_are_charged_per_pair() {
        let wb = Workbook::default();
        // 4 x 4 pairs, 4 of which meet.
        let src = "=AREAS((A1,B1,C1,D1) (A1,B1,C1,D1))";
        let (v, _) = eval_work(wb.clone(), EVAL_WORK, src);
        assert_eq!(v, Value::number(4.0));
        let (v, _) = eval_work(wb, 20, src);
        assert!(is_calc(&v), "{v:?}");
    }

    #[test]
    fn an_aggregate_inside_every_lambda_call_spends_work() {
        let wb = Workbook::default();
        // 100 calls, each summing the 10,000 values of a captured array: about a million units.
        let src = "=LET(big,SEQUENCE(10000),SUM(MAP(SEQUENCE(100),LAMBDA(x,SUM(big)))))";
        let (v, _) = eval_work(wb.clone(), 2_000_000, src);
        assert_eq!(v, Value::number(100.0 * 50_005_000.0));
        let (v, _) = eval_work(wb, 200_000, src);
        assert!(is_calc(&v), "{v:?}");
    }

    #[test]
    fn lambda_calls_spend_fuel_and_run_out_quietly() {
        // Three applications, one charge each: 3 inside the budget. (A nested MAP wouldn't do
        // here: an inner MAP result is an array, and MAP of array elements is #CALC! on shape,
        // which happens well before the budget does.)
        let r = eval_with(3, "=LAMBDA(x,LAMBDA(y,LAMBDA(z,z+1)(3))(2))(1)");
        assert_eq!(r, Value::number(4.0), "{r:?}");
        // One charge short: the nest bottoms out in #CALC! instead of running unbounded.
        let r = eval_with(2, "=LAMBDA(x,LAMBDA(y,LAMBDA(z,z+1)(3))(2))(1)");
        assert!(matches!(r, Value::Error(CellError::Calc)), "{r:?}");
        // One charge short of a single application: #CALC! with a zero budget.
        let r = eval_with(0, "=LAMBDA(x,x+1)(2)");
        assert!(matches!(r, Value::Error(CellError::Calc)), "{r:?}");
        let r = eval_with(1, "=LAMBDA(x,x+1)(2)");
        assert_eq!(r, Value::number(3.0));
        // A flat helper map keeps working (3 of 3 invokes inside the budget).
        let r = eval_with(3, "=MAP({1,2,3},LAMBDA(x,x*2))");
        assert!(matches!(r, Value::Array(_)) && !matches!(&r, Value::Array(a) if a.data.iter().any(|v| matches!(v, Value::Error(_)))), "{r:?}");
    }
}
