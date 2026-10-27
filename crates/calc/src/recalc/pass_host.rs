use super::graph::{HashMap, HashSet, Key};
use super::{Evaluator, Host, Workbook, stack_position};
use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;

/// Formulas a formula may calculate on demand one inside the other (each needing one another's
/// value before it is done). Each level takes stack, so a chain deeper than this is suspended and
/// calculated from the top instead (see [`PassHost::calculate`]): long dependency chains calculate
/// whatever order they come in, with bounded stack.
const MAX_NESTED: usize = 64;

/// Formula evaluations iterative calculation may spend on one recalculation (passes × formulas
/// evaluated again), however high Maximum Iterations is.
const MAX_ITERATION_WORK: usize = 10_000_000;

pub(super) struct PassHost<'a> {
    wb: &'a Workbook,
    exprs: &'a HashMap<Key, std::sync::Arc<Expr>>,
    pub(super) results: HashMap<Key, Value>,
    pending: HashSet<Key>,
    in_progress: HashSet<Key>,
    /// New spills found during the pass: anchor → array.
    pub(super) spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Used range per sheet: the workbook doesn't change during a pass, and working it out walks
    /// every row, so once per sheet rather than once per range read.
    used: HashMap<usize, Option<RangeRef>>,
    /// Optional constant-range indexes live only for this pass. None remembers an ineligible
    /// range too, so thousands of calls do not repeatedly check the same formula cells.
    text_criteria: HashMap<(Area, Area), Option<std::sync::Arc<gridcraft_functions::TextCriteriaIndex>>>,
    /// First requests do not build indexes: running COUNTIF ranges are often used only once.
    criteria_seen: HashSet<(Area, Area)>,
    criteria_recent: VecDeque<(Area, Area)>,
    criteria_cells: u64,
    criteria_bytes: usize,
    criteria_groups: usize,
    rng: &'a mut u64,
    now: f64,
    cycle: bool,
    pub(super) cycles: Vec<Key>,
    depth: usize,
    /// A formula needed at [`MAX_NESTED`] levels deep: the evaluations in progress are abandoned
    /// and it is calculated first.
    suspend: Option<Key>,
    /// Formulas whose evaluation was abandoned for another one, to be calculated again after it.
    /// They all need it, so reading one of them while it is calculated is a circular reference,
    /// as reading a formula in progress is.
    waiting: HashSet<Key>,
    /// The formulas abandoned since the last suspension.
    abandoned: Vec<Key>,
    /// Other formulas read at [`MAX_NESTED`] levels deep by the evaluations being abandoned. They
    /// are calculated along with the suspended one, rather than one per restart (a wide range
    /// read at the limit would restart the abandoned evaluations once per cell in it).
    also: Vec<Key>,
    /// Suspensions so far (tests check that a wide read at the limit needs one, not one per cell).
    #[cfg(test)]
    pub(super) suspensions: usize,
    /// The formulas in `cycles`.
    cycle_seen: HashSet<Key>,
    /// Iterative calculation: a circular reference reads the cell's previous value instead of
    /// giving `#CIRC!`.
    pub(super) iterate: bool,
    /// Values from the previous iteration; cells not in it read their stored value.
    prev: HashMap<Key, Value>,
    /// Formulas being evaluated, innermost last (kept only when iterating).
    stack: Vec<Key>,
    /// Formulas found on a circular reference (when iterating).
    pub(super) members: HashSet<Key>,
    /// Results that read a previous value, directly or through other formulas.
    pub(super) tainted: HashSet<Key>,
    /// Whether the formula being evaluated has read a previous value.
    taint: bool,
    /// `stack_position()` where the outermost formula being calculated started.
    stack_top: usize,
    /// Whether an evaluation ran past the stack budget since the innermost formula started.
    out_of_stack: bool,
}

impl<'a> PassHost<'a> {
    pub(super) fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, rng: &'a mut u64, now: f64) -> Self {
        PassHost {
            wb,
            exprs,
            results: HashMap::with_capacity_and_hasher(exprs.len(), Default::default()),
            pending: exprs.keys().copied().collect(),
            in_progress: HashSet::default(),
            spills: HashMap::default(),
            used: HashMap::default(),
            text_criteria: HashMap::default(),
            criteria_seen: HashSet::default(),
            criteria_recent: VecDeque::new(),
            criteria_cells: 0,
            criteria_bytes: 0,
            criteria_groups: 0,
            rng,
            now,
            cycle: false,
            cycles: vec![],
            depth: 0,
            suspend: None,
            waiting: HashSet::default(),
            abandoned: Vec::new(),
            also: Vec::new(),
            #[cfg(test)]
            suspensions: 0,
            cycle_seen: HashSet::default(),
            iterate: false,
            prev: HashMap::default(),
            stack: Vec::new(),
            members: HashSet::default(),
            tainted: HashSet::default(),
            taint: false,
            stack_top: 0,
            out_of_stack: false,
        }
    }
}

/// Whether an array result at `k` can't spill: it is in a table, or its area runs off the sheet,
/// overlaps merged cells, holds other cells, or another formula's spill (unless that formula is
/// recalculated in this pass, and so may spill elsewhere now).
fn spill_blocked(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = host.wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    let own = sh.spill_ranges.get(&anchor);
    let spilled_over = |c: CellRef| {
        !own.is_some_and(|o| o.contains(c))
            && sh.spill_ranges.iter().any(|(o, r)| *o != anchor && r.contains(c) && !host.exprs.contains_key(&(sheet, *o)))
    };
    end.row >= gridcraft_core::MAX_ROWS
        || end.col >= gridcraft_core::MAX_COLS
        || sh.merges.iter().any(|m| m.intersects(&range))
        || sh.tables.iter().any(|t| t.range.contains(anchor))
        || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (!cell.value.is_empty() || cell.formula.is_some()))
        || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c) && spilled_over(*c))
}

/// Whether `arr`, anchored at `anchor`, would spill exactly what the sheet already holds over
/// `range`: the same area, nothing new in the way, and the same values.
pub(super) fn spills_as_before(sheet: &gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, arr: &Array) -> bool {
    let end =
        CellRef::new(anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)), anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)));
    RangeRef::new(anchor, end) == range
        && !sheet.merges.iter().any(|m| m.intersects(&range))
        && !sheet.tables.iter().any(|t| t.range.contains(anchor))
        && range
            .iter()
            .enumerate()
            .all(|(i, c)| c == anchor || (!sheet.cells.has(c) && sheet.spill.get(&c) == Some(arr.data.get(i).unwrap_or(&Value::Empty))))
}

impl PassHost<'_> {
    /// Calculates `k` (and the formulas it reads) with bounded stack: when [`Self::compute`] would
    /// go deeper than [`MAX_NESTED`], it suspends what is in progress, the formula it needed is
    /// calculated from here, then the suspended ones again (reading the results kept so far).
    pub(super) fn calculate(&mut self, k: Key) {
        // Formulas to calculate, innermost last, each with the ones abandoned to calculate the
        // next.
        let mut work: Vec<(Key, Vec<Key>)> = vec![(k, Vec::new())];
        while let Some(&(top, _)) = work.last() {
            self.compute(top);
            match self.suspend.take() {
                Some(next) => {
                    #[cfg(test)]
                    {
                        self.suspensions += 1;
                    }
                    let held = std::mem::take(&mut self.abandoned);
                    self.waiting.extend(held.iter().copied());
                    if let Some(entry) = work.last_mut() {
                        entry.1 = held;
                    }
                    // The other formulas read at the limit are calculated after `next`, before the
                    // held evaluations restart. Each has nothing held of its own, so the held ones
                    // are released only when the last of them is done.
                    let mut seen: HashSet<Key> = HashSet::default();
                    seen.insert(next);
                    for s in std::mem::take(&mut self.also) {
                        if seen.insert(s) {
                            work.push((s, Vec::new()));
                        }
                    }
                    work.push((next, Vec::new()));
                }
                None => {
                    work.pop();
                    if let Some(entry) = work.last_mut() {
                        for a in entry.1.drain(..) {
                            self.waiting.remove(&a);
                        }
                    }
                }
            }
        }
    }

    fn compute(&mut self, k: Key) -> Value {
        if let Some(v) = self.results.get(&k) {
            let v = v.clone();
            if self.iterate && self.tainted.contains(&k) {
                self.taint = true;
            }
            return v;
        }
        let Some(expr) = self.exprs.get(&k) else {
            return self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
        };
        if self.suspend.is_some() {
            // Unwinding to `calculate`: the value is thrown away. A formula read at the limit is
            // calculated with the suspended one, so the evaluations are not restarted for each.
            if self.depth >= MAX_NESTED && !self.in_progress.contains(&k) && !self.waiting.contains(&k) {
                self.also.push(k);
            }
            return Value::Error(CellError::Calc);
        }
        if self.in_progress.contains(&k) || self.waiting.contains(&k) {
            self.cycle = true;
            if self.iterate {
                return self.circular_read(k);
            }
            // Once each, though a suspended evaluation meets it again.
            if self.cycle_seen.insert(k) {
                self.cycles.push(k);
            }
            return Value::Error(CellError::Circ);
        }
        if self.depth >= MAX_NESTED {
            self.suspend = Some(k);
            return Value::Error(CellError::Calc);
        }
        if self.depth == 0 {
            self.stack_top = stack_position();
        }
        self.in_progress.insert(k);
        self.depth += 1;
        let outer_taint = std::mem::replace(&mut self.taint, false);
        if self.iterate {
            self.stack.push(k);
        }
        let expr = std::sync::Arc::clone(expr);
        let outer_out_of_stack = std::mem::replace(&mut self.out_of_stack, false);
        let v = {
            let mut ev = Evaluator::new(self, k.0, k.1);
            ev.value(&expr)
        };
        let out_of_stack = std::mem::replace(&mut self.out_of_stack, outer_out_of_stack);
        if out_of_stack && self.depth > 1 && self.suspend.is_none() {
            // It ran out of the stack left below the formulas that read it: it is calculated
            // from the top instead, as a formula needed at `MAX_NESTED` levels deep is.
            self.suspend = Some(k);
            self.depth -= 1;
            self.in_progress.remove(&k);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        if self.suspend.is_some() {
            // Abandoned: `calculate` evaluates this formula again once the one it needs is done.
            self.depth -= 1;
            self.in_progress.remove(&k);
            self.abandoned.push(k);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        // A legacy array formula fills the range it was entered in.
        let legacy = self.wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.as_ref()).and_then(|f| f.array);
        let v = match legacy {
            Some(range) => fit_to_range(v, range),
            None => v,
        };
        self.depth -= 1;
        self.in_progress.remove(&k);
        self.pending.remove(&k);
        if self.iterate {
            self.stack.pop();
            if self.taint {
                self.tainted.insert(k);
            } else {
                self.tainted.remove(&k);
            }
        }
        self.taint |= outer_taint;
        // A blank cell shows as 0 in a formula's result, inside an array as well (`=A1:A3`,
        // FILTER or SORT of a range with blanks). GROUPBY and PIVOTBY lay out blank cells
        // of their own, which stay blank.
        let keep_blanks = matches!(&*expr, Expr::Call(n, _) if matches!(n.as_str(), "GROUPBY" | "PIVOTBY"));
        let v = match v {
            Value::Array(a) => {
                let a = if keep_blanks || !a.data.iter().any(|x| matches!(x, Value::Empty)) {
                    a
                } else {
                    let data = a.data.iter().map(|x| if matches!(x, Value::Empty) { Value::Number(0.0) } else { x.clone() }).collect();
                    Array::new(a.rows, a.cols, data).map(std::sync::Arc::new).unwrap_or(a)
                };
                // A dynamic-array formula anchors a spill range even while its result is one
                // value -- a FILTER matching one row, a SORT or UNIQUE over one item -- so `C1#`
                // is the 1 x 1 range and `ROWS(C1#)` is 1, as in Excel (#1491). A formula that
                // returns a scalar never reaches this arm, so it does not become an anchor.
                self.spills.insert(k, a.clone());
                a.data.first().cloned().unwrap_or_default()
            }
            Value::Empty => Value::Number(0.0),
            v => v,
        };
        // An array that can't spill is #SPILL! for the formulas evaluated after it in this pass too.
        let v = match self.spills.get(&k) {
            Some(a) if spill_blocked(self, k, a) => {
                self.spills.remove(&k);
                Value::Error(CellError::Spill)
            }
            _ => v,
        };
        if let Some(a) = self.spills.get(&k) {
            let area = spill_area(k, a);
            // A spill can fill cells that were empty when an index was built. Drop only
            // intersecting indexes, so unrelated arrays do not disable criteria acceleration.
            self.text_criteria.retain(|(source, target), _| !overlaps(*source, area) && !overlaps(*target, area));
        }
        self.results.insert(k, v.clone());
        v
    }

    /// A formula's previous value for iterative calculation: from the last iteration, else the
    /// value it shows. Blank, and the `#CIRC!` of a loop calculated with iteration off, are 0.
    fn previous(&self, k: Key) -> Value {
        let v = match self.prev.get(&k) {
            Some(v) => v.clone(),
            None => self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(),
        };
        if matches!(v, Value::Empty | Value::Error(CellError::Circ)) { Value::Number(0.0) } else { v }
    }

    /// Reading `k` while it is being evaluated, with iterative calculation on: its previous value.
    /// The formulas around the loop are the ones whose changes decide when iteration stops.
    fn circular_read(&mut self, k: Key) -> Value {
        self.taint = true;
        match self.stack.iter().rposition(|s| *s == k) {
            Some(i) => self.members.extend(self.stack.get(i..).unwrap_or_default().iter().copied()),
            // Too deep to follow: the formula itself is watched.
            None => {
                self.members.insert(k);
            }
        }
        self.previous(k)
    }

    /// Iterative calculation, after a first pass over `order` that met circular references. The
    /// formulas that read a previous value (the loops and what depends on them) are evaluated
    /// again in the same order, each pass starting from the last one's values and reading what it
    /// has already produced (Gauss-Seidel), until no formula on a loop changes by `max_change` or
    /// more, or after `max_iterations` passes in all. Returns the number of passes.
    pub(super) fn iterate_loops(&mut self, order: &[Key], max_iterations: u32, max_change: f64) -> u32 {
        let again: Vec<Key> = order.iter().copied().filter(|k| self.tainted.contains(k)).collect();
        let set: HashSet<Key> = again.iter().copied().collect();
        // Bounds the work whatever Maximum Iterations says.
        let max_passes = u32::try_from(MAX_ITERATION_WORK / again.len().max(1)).unwrap_or(u32::MAX).max(1);
        let mut passes = 1u32;
        while passes < max_iterations.min(max_passes) && !self.converged(max_change) {
            self.prev = again.iter().filter_map(|k| self.results.remove(k).map(|v| (*k, v))).collect();
            self.pending = set.clone();
            self.spills.retain(|a, _| !set.contains(a));
            for k in &again {
                self.calculate(*k);
            }
            passes += 1;
        }
        passes
    }

    /// Whether every formula on a loop changed by less than `max_change` in the last pass (text,
    /// logical and error values: not at all).
    fn converged(&self, max_change: f64) -> bool {
        self.members.iter().all(|k| {
            let Some(new) = self.results.get(k) else { return true };
            match (new, &self.previous(*k)) {
                (Value::Number(a), Value::Number(b)) => {
                    let d = (a - b).abs();
                    d < max_change || d == 0.0
                }
                (new, old) => new == old,
            }
        })
    }
}

fn spill_area((sheet, anchor): Key, a: &Array) -> Area {
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    Area { sheet, range: RangeRef::new(anchor, end) }
}

fn overlaps(a: Area, b: Area) -> bool {
    a.sheet == b.sheet && a.range.intersects(&b.range)
}

/// A legacy (Ctrl+Shift+Enter) array formula's result fitted to the range it was entered in, as
/// Excel does: a single row or column repeats across the range, a single value fills it, cells
/// past the result show `#N/A`, and a result larger than the range is cut off.
fn fit_to_range(v: Value, range: RangeRef) -> Value {
    if range.count() > crate::eval::MAX_CELLS {
        return Value::Error(CellError::Num);
    }
    let (h, w) = (range.height() as usize, range.width() as usize);
    let Value::Array(a) = v else {
        return if h * w == 1 { v } else { Array::new(h, w, vec![v; h * w]).map(Value::from).unwrap_or(Value::Error(CellError::Value)) };
    };
    if h * w == 1 {
        return a.data.first().cloned().unwrap_or_default();
    }
    let mut data = Vec::with_capacity(h * w);
    for r in 0..h {
        for c in 0..w {
            let (r, c) = (if a.rows == 1 { 0 } else { r }, if a.cols == 1 { 0 } else { c });
            data.push(a.get(r, c).cloned().unwrap_or(Value::Error(CellError::NA)));
        }
    }
    Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
}

impl Host for PassHost<'_> {
    fn constant_value(&self, area: Area) -> Option<Value> {
        let sh = self.wb.sheet(area.sheet)?;
        if self.iterate || !area.range.is_single() {
            return None;
        }
        // A spill cannot overwrite a nonempty stored constant (spill_blocked). Empty cells
        // may be covered by a new spill, so decline them without reading/evaluating anything.
        match sh.cell(area.range.start) {
            Some(cell) if cell.formula.is_some() || matches!(cell.value, Value::Array(_) | Value::Empty) => None,
            Some(cell) => Some(cell.value.clone()),
            None => None,
        }
    }
    fn text_criteria_index(&mut self, range: Area, target: Area) -> Option<std::sync::Arc<gridcraft_functions::TextCriteriaIndex>> {
        if self.iterate {
            return None;
        }
        let source = self.wb.sheet(range.sheet)?;
        let dest = self.wb.sheet(target.sheet)?;
        let key = (range, target);
        if let Some(index) = self.text_criteria.get(&key) {
            return index.clone();
        }
        if !self.criteria_seen.contains(&key) {
            if self.criteria_recent.len() >= 1_024
                && let Some(old) = self.criteria_recent.pop_front()
            {
                self.criteria_seen.remove(&old);
            }
            self.criteria_seen.insert(key);
            self.criteria_recent.push_back(key);
            return None;
        }
        // Bound the cells inspected for optional builds, including ineligible ranges. Text
        // and group allowances also bound retained keys and per-group/map overhead. Reaching
        // any allowance changes no results: the ordinary evaluator remains the fallback.
        let cells = u64::from(range.range.height()) * u64::from(range.range.width());
        if self.text_criteria.len() >= 1_024 || self.criteria_cells.saturating_add(cells) > 1_000_000 {
            return None;
        }
        self.criteria_cells += cells;
        let constant = |area: Area| {
            self.wb
                .sheet(area.sheet)
                .is_some_and(|sh| sh.cells.iter_range(area.range).all(|(_, cell)| cell.formula.is_none() && !matches!(cell.value, Value::Array(_))))
        };
        let spill_overlaps = |area: Area| {
            self.wb.sheet(area.sheet).is_some_and(|sh| {
                sh.spill_ranges.values().any(|r| r.intersects(&area.range))
                    || sh.spill.range(area.range.start..=area.range.end).any(|(c, _)| area.range.contains(*c))
            }) || self.spills.iter().any(|(k, a)| overlaps(area, spill_area(*k, a)))
        };
        let index = if constant(range) && constant(target) && !spill_overlaps(range) && !spill_overlaps(target) {
            let cells = source.cells.iter_range(range.range).map(|(at, cell)| {
                let target_cell =
                    CellRef::new(target.range.start.row + at.row - range.range.start.row, target.range.start.col + at.col - range.range.start.col);
                (&cell.value, dest.value_ref(target_cell).unwrap_or(&Value::Empty))
            });
            gridcraft_functions::TextCriteriaIndex::new(
                cells,
                8_000_000usize.saturating_sub(self.criteria_bytes),
                100_000usize.saturating_sub(self.criteria_groups),
            )
            .map(std::sync::Arc::new)
        } else {
            None
        };
        if let Some(index) = &index {
            self.criteria_bytes += index.text_bytes();
            self.criteria_groups += index.groups();
        }
        self.text_criteria.insert(key, index.clone());
        index
    }
    fn workbook(&self) -> &Workbook {
        self.wb
    }
    fn stack_top(&self) -> Option<usize> {
        (self.depth > 1).then_some(self.stack_top)
    }
    fn stack_exhausted(&mut self) {
        self.out_of_stack = true;
    }
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        let k = (sheet, anchor);
        if !(self.pending.contains(&k) || self.results.contains_key(&k)) {
            return self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied();
        }
        // Recalculated in this pass: its new array, unless something blocks it (#SPILL!).
        self.compute(k);
        let arr = self.spills.get(&k)?;
        let end = CellRef::new(
            anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)),
            anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)),
        );
        let range = RangeRef::new(anchor, end);
        let sh = self.wb.sheet(sheet)?;
        let blocked = end.row >= gridcraft_core::MAX_ROWS
            || end.col >= gridcraft_core::MAX_COLS
            || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (cell.formula.is_some() || !cell.value.is_empty()))
            || sh.merges.iter().any(|m| m.intersects(&range));
        (!blocked).then_some(range)
    }
    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
        let k = (sheet, c);
        if self.pending.contains(&k) || self.results.contains_key(&k) {
            return self.compute(k);
        }
        // A cell covered by a spill computed in this pass.
        for (anchor, arr) in &self.spills {
            if anchor.0 == sheet && c.row >= anchor.1.row && c.col >= anchor.1.col {
                let (dr, dc) = ((c.row - anchor.1.row) as usize, (c.col - anchor.1.col) as usize);
                if dr < arr.rows && dc < arr.cols {
                    if self.iterate && self.tainted.contains(anchor) {
                        self.taint = true;
                    }
                    return arr.get(dr, dc).cloned().unwrap_or_default();
                }
            }
        }
        self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default()
    }
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        let (h, w) = (range.height() as usize, range.width() as usize);
        let mut out = vec![Value::Empty; h * w];
        let Some(sh) = self.wb.sheet(sheet) else { return out };
        // Sparse walk: only stored cells; formula cells that are (or were) dirty go through
        // `compute`, constants are copied straight from the store.
        let mut formulas: Vec<(usize, CellRef)> = Vec::new();
        for (c, cell) in sh.cells.iter_range(range) {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            if cell.formula.is_some() {
                formulas.push((i, c));
            } else if let Some(slot) = out.get_mut(i) {
                *slot = cell.value.clone();
            }
        }
        for (c, v) in sh.spill.range(range.start..=range.end) {
            if range.contains(*c) {
                let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
                if let Some(slot) = out.get_mut(i)
                    && slot.is_empty()
                {
                    *slot = v.clone();
                }
            }
        }
        for (i, c) in formulas {
            let v = self.cell_value(sheet, c);
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        if !self.spills.is_empty() {
            let new: Vec<(usize, Value)> = self
                .spills
                .iter()
                .filter(|(a, _)| a.0 == sheet)
                .flat_map(|(a, arr)| {
                    let mut v = Vec::new();
                    for r in 0..arr.rows {
                        for cc in 0..arr.cols {
                            let c = CellRef::new(a.1.row.saturating_add(r as u32), a.1.col.saturating_add(cc as u32));
                            if c != a.1 && range.contains(c) {
                                v.push((
                                    (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize,
                                    arr.get(r, cc).cloned().unwrap_or_default(),
                                ));
                            }
                        }
                    }
                    v
                })
                .collect();
            // Iterating: an array spilled by a formula that read a previous value.
            if self.iterate
                && self.spills.iter().any(|(a, arr)| {
                    let end = CellRef::new(
                        a.1.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                        a.1.col.saturating_add((arr.cols as u32).saturating_sub(1)),
                    );
                    a.0 == sheet && self.tainted.contains(a) && RangeRef::new(a.1, end).intersects(&range)
                })
            {
                self.taint = true;
            }
            for (i, v) in new {
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
        }
        out
    }
    fn visit_range(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        let wb = self.wb;
        let Some(sh) = wb.sheet(sheet) else { return false };
        // Formula cells first, in row-major order like `range_values`: reading one calculates it
        // and the pass remembers the value, so the walk below gets the same value again without
        // this method holding any.
        for (c, cell) in sh.cells.iter_range(range) {
            if cell.formula.is_some() {
                let _ = self.cell_value(sheet, c);
            }
        }
        // Spilled values sit between the stored cells (and the formulas just calculated may have
        // produced some): leave such blocks to `range_values`.
        if self.spills.keys().any(|a| a.0 == sheet) || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c)) {
            return false;
        }
        for (c, cell) in sh.cells.iter_range(range) {
            let go = if cell.formula.is_some() { f(&self.cell_value(sheet, c)) } else { f(&cell.value) };
            if !go {
                break;
            }
        }
        true
    }
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        let wb = self.wb;
        *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(|s| s.used_range()))
    }
    fn now_serial(&self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        // xorshift64*
        let mut x = *self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod criteria_tests {
    use super::*;
    use gridcraft_model::{Cell, Formula};

    #[test]
    fn one_off_running_counts_do_not_exhaust_the_repeated_range_cache() {
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        for r in 0..1_500 {
            let sh = wb.sheet_mut(0).unwrap();
            sh.cells.set(CellRef::new(r, 0), Cell::value(Value::from(format!("key{r}"))));
            sh.cells.set(CellRef::new(r, 1), Cell::value(Value::Number(1.0)));
            for (col, text) in [(2, format!("=COUNTIF(A$1:A{0},A{0})", r + 1)), (3, format!("=SUMIF(A$1:A$1500,A{},B$1:B$1500)", r + 1))] {
                let at = CellRef::new(r, col);
                let formula = Formula::new(&text);
                exprs.insert((0, at), formula.expr_arc().unwrap());
                sh.cells.set(at, Cell::formula(formula));
            }
        }
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        for col in [2, 3] {
            for r in 0..1_500 {
                let key = (0, CellRef::new(r, col));
                host.calculate(key);
                assert_eq!(host.results.get(&key), Some(&Value::Number(1.0)));
            }
            if col == 2 {
                assert_eq!(host.criteria_cells, 0, "one-off ranges spent the build allowance");
            }
        }
        assert_eq!(host.criteria_cells, 1_500);
        assert_eq!(host.text_criteria.len(), 1);
        assert!(host.criteria_seen.len() <= 1_024);
    }

    #[test]
    fn saved_spill_values_are_not_indexed_as_empty_stored_cells() {
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::formula(Formula::new("={\"header\";\"key\";\"key\"}")));
        for r in 0..3 {
            let at = CellRef::new(r, 2);
            let formula = Formula::new("=COUNTIF(A2:A3,\"key\")");
            exprs.insert((0, at), formula.expr_arc().unwrap());
            wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(formula));
        }
        Calc::new().recalc_all(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(2, 0)), Value::from("key"));
        // Only the criteria formulas are dirty in this pass. Their array precedent is saved.
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        for r in 0..3 {
            let key = (0, CellRef::new(r, 2));
            host.calculate(key);
            assert_eq!(host.results.get(&key), Some(&Value::Number(2.0)));
        }
        assert!(host.spills.is_empty());
        assert_eq!(host.criteria_cells, 2);
        assert!(host.text_criteria.values().all(Option::is_none));
    }

    #[test]
    fn text_criteria_formulas_index_their_ranges_once_per_pass() {
        use crate::eval::Area;
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        for r in 0..500 {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 0), Cell::value(Value::from(format!("key{r}"))));
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 1), Cell::value(Value::Number(f64::from(r))));
            for col in 2..5 {
                let at = CellRef::new(r, col);
                let formula = Formula::new(&format!("=SUMIF(A$1:A$500,A{},B$1:B$500)", r + 1));
                exprs.insert((0, at), formula.expr_arc().unwrap());
                wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(formula));
            }
        }
        let spill_anchor = CellRef::new(0, 25);
        let formula = Formula::new("=SEQUENCE(2)");
        exprs.insert((0, spill_anchor), formula.expr_arc().unwrap());
        let sh = wb.sheet_mut(0).unwrap();
        sh.cells.set(spill_anchor, Cell::formula(formula));
        sh.spill.insert(CellRef::new(1, 25), Value::Number(2.0));
        sh.spill_ranges.insert(spill_anchor, RangeRef::parse("Z1:Z2").unwrap());
        let range = Area { sheet: 0, range: RangeRef::parse("A1:A500").unwrap() };
        let target = Area { sheet: 0, range: RangeRef::parse("B1:B500").unwrap() };
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        host.calculate((0, spill_anchor));
        for col in 2..5 {
            for r in 0..500 {
                let key = (0, CellRef::new(r, col));
                host.calculate(key);
                assert_eq!(host.results.get(&key), Some(&Value::Number(f64::from(r))));
            }
        }
        // Exercise the real evaluator, so bypassing the fast path fails this active guard too.
        assert_eq!(host.criteria_cells, 500);
        assert_eq!(host.text_criteria.len(), 1);
        host.iterate = true;
        assert!(host.text_criteria_index(range, target).is_none());
    }

    #[test]
    fn a_new_overlapping_spill_invalidates_a_text_criteria_index() {
        for source_spills in [false, true] {
            let mut wb = Workbook::new();
            let mut exprs = HashMap::default();
            let (anchor, array) = if source_spills { ("A1", "={\"header\";\"key\";\"key\"}") } else { ("B1", "={0;1;2}") };
            for (at, text) in
                [("C1", "=SUMIF(A2:A3,\"key\",B2:B3)"), (anchor, array), ("C2", "=SUMIF(A2:A3,\"key\",B2:B3)"), ("C3", "=SUMIF(A2:A3,\"key\",B2:B3)")]
            {
                let cell = CellRef::parse(at).unwrap();
                let formula = Formula::new(text);
                exprs.insert((0, cell), formula.expr_arc().unwrap());
                wb.sheet_mut(0).unwrap().cells.set(cell, Cell::formula(formula));
            }
            for r in 1..=2 {
                let (col, value) = if source_spills { (1, Value::Number(f64::from(r))) } else { (0, Value::from("key")) };
                wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, col), Cell::value(value));
            }
            let mut rng = 1;
            let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
            host.calculate((0, CellRef::new(0, 2)));
            assert_eq!(host.results.get(&(0, CellRef::new(0, 2))), Some(&Value::Number(0.0)));
            host.calculate((0, CellRef::new(1, 2)));
            assert_eq!(host.results.get(&(0, CellRef::new(1, 2))), Some(&Value::Number(0.0)));
            assert_eq!(host.criteria_cells, 2);
            host.calculate((0, CellRef::parse(anchor).unwrap()));
            host.calculate((0, CellRef::new(2, 2)));
            assert_eq!(host.results.get(&(0, CellRef::new(2, 2))), Some(&Value::Number(3.0)));
        }
    }
}
