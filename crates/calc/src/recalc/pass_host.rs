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

/// Bounds uncharged index traversal between persistent refusal/cancellation checkpoints.
const SCAN_CHUNK: usize = 256;

/// Charges actual index entries in bounded chunks, including keys outside a rectangular
/// range that lie inside its row-major key interval. Unmetered hosts skip this counting pass.
fn charge_scan(hook: Option<&crate::WorkHook>, mut entries: impl Iterator) -> bool {
    let Some(hook) = hook else { return true };
    loop {
        if !hook.charge(0) {
            return false;
        }
        let count = entries.by_ref().take(SCAN_CHUNK).count();
        if count == 0 {
            return true;
        }
        if !hook.charge(count as u64) {
            return false;
        }
    }
}

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
    pub(super) work: Option<crate::WorkHook>,
    simulation: Option<crate::SimulationHook>,
}

impl<'a> PassHost<'a> {
    /// Copies only the intersection of newly calculated arrays and the requested rectangle.
    /// Each array lookup and intersecting copy is charged; no temporary full-spill expansion
    /// is allocated. Iterative-calculation taint follows the same intersections as before.
    fn overlay_spills(&mut self, sheet: usize, range: RangeRef, width: usize, out: &mut [Value]) -> bool {
        for (anchor, array) in &self.spills {
            if self.work.as_ref().is_some_and(|hook| !hook.charge(1)) {
                return false;
            }
            if anchor.0 != sheet {
                continue;
            }
            let end = CellRef::new(
                anchor.1.row.saturating_add((array.rows as u32).saturating_sub(1)),
                anchor.1.col.saturating_add((array.cols as u32).saturating_sub(1)),
            );
            let Some(overlap) = RangeRef::new(anchor.1, end).intersection(&range) else { continue };
            if self.work.as_ref().is_some_and(|hook| !hook.charge(overlap.count())) {
                return false;
            }
            self.taint |= self.iterate && self.tainted.contains(anchor);
            for (offset, cell) in overlap.iter().enumerate() {
                if offset.is_multiple_of(SCAN_CHUNK) && self.work.as_ref().is_some_and(|hook| !hook.charge(0)) {
                    return false;
                }
                if cell != anchor.1 {
                    let i = (cell.row - range.start.row) as usize * width + (cell.col - range.start.col) as usize;
                    if let Some(slot) = out.get_mut(i) {
                        *slot = array.get((cell.row - anchor.1.row) as usize, (cell.col - anchor.1.col) as usize).cloned().unwrap_or_default();
                    }
                }
            }
        }
        true
    }

    /// Captures the current host policy once for a recalculation pass and its native kernels.
    pub(super) fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, rng: &'a mut u64, now: f64) -> Self {
        PassHost {
            wb,
            work: crate::WorkHook::current(),
            simulation: crate::SimulationHook::current(),
            exprs,
            results: HashMap::with_capacity_and_hasher(exprs.len(), Default::default()),
            pending: exprs.keys().copied().collect(),
            in_progress: HashSet::default(),
            spills: HashMap::default(),
            used: HashMap::default(),
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
    /// Propagates the same work policy through native calls, broadcasts and formula evaluation.
    fn charge_work(&mut self, units: u64) -> bool {
        self.work.as_ref().is_none_or(|hook| hook.charge(units))
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
    /// Reserves rectangular materialization work before allocating values, including blanks.
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        if !self.charge_work(range.count()) {
            return vec![Value::Error(CellError::Num)];
        }
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
        if !charge_scan(self.work.as_ref(), sh.spill.range(range.start..=range.end)) {
            return vec![Value::Error(CellError::Num)];
        }
        for (offset, (c, v)) in sh.spill.range(range.start..=range.end).enumerate() {
            if offset.is_multiple_of(SCAN_CHUNK) && self.work.as_ref().is_some_and(|hook| !hook.charge(0)) {
                return vec![Value::Error(CellError::Num)];
            }
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
        if !self.overlay_spills(sheet, range, w, &mut out) {
            return vec![Value::Error(CellError::Num)];
        }
        out
    }
    /// Charges stored cells and spill traversal without billing virtual whole-column blanks.
    fn visit_range(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        // Sparse aggregations must charge the work they visit, not millions of virtual blanks.
        if !self.charge_work(1) {
            f(&Value::Error(CellError::Num));
            return true;
        }
        let wb = self.wb;
        let Some(sh) = wb.sheet(sheet) else { return false };
        if !charge_scan(self.work.as_ref(), sh.cells.iter_range(range)) || !charge_scan(self.work.as_ref(), sh.spill.range(range.start..=range.end)) {
            f(&Value::Error(CellError::Num));
            return true;
        }
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
        if self.spills.keys().any(|a| a.0 == sheet) {
            return false;
        }
        for (offset, (cell, _)) in sh.spill.range(range.start..=range.end).enumerate() {
            if offset.is_multiple_of(SCAN_CHUNK) && self.work.as_ref().is_some_and(|hook| !hook.charge(0)) {
                f(&Value::Error(CellError::Num));
                return true;
            }
            if range.contains(*cell) {
                return false;
            }
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

    /// Uses the pass-captured simulation host rather than calc's unrelated volatile RNG.
    fn simulation(&mut self, sheet: usize, cell: CellRef, request: crate::SimulationRequest<'_>) -> Option<Value> {
        self.simulation.as_ref().map(|hook| hook.sample(crate::SimulationSite { sheet, cell }, request))
    }
}

#[cfg(test)]
mod spill_work_tests {
    use std::{cell::Cell, sync::Arc};

    use super::{Array, CellError, CellRef, HashMap, Host, PassHost, RangeRef, SCAN_CHUNK, Value, Workbook, charge_scan};
    use crate::WorkHook;

    /// A refused chunk cannot scan the rest of a large row-major interval.
    #[test]
    fn index_scan_stops_within_one_chunk_on_refusal() {
        let seen = Cell::new(0usize);
        let hook = WorkHook::new(|units| units < SCAN_CHUNK as u64);
        let entries = (0..1_000_000).inspect(|_| seen.set(seen.get() + 1));
        assert!(!charge_scan(Some(&hook), entries));
        assert_eq!(seen.get(), SCAN_CHUNK);
        assert!(!charge_scan(Some(&hook), (0..1_000_000).inspect(|_| seen.set(seen.get() + 1))));
        assert_eq!(seen.get(), SCAN_CHUNK);
    }

    /// A tiny read of a new 10k-row spill copies and charges only its intersection.
    #[test]
    fn same_pass_spill_intersection_is_bounded_and_preserves_taint() {
        let hook = WorkHook::new(|units| units <= 2);
        let _scope = hook.enter().unwrap();
        let wb = Workbook::new();
        let exprs = HashMap::default();
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        let anchor = (0, CellRef::new(0, 25));
        host.spills.insert(anchor, Arc::new(Array::column((0..10_000).map(|i| Value::Number(i as f64)).collect())));
        host.iterate = true;
        host.tainted.insert(anchor);
        let range = RangeRef::new(CellRef::new(9998, 25), CellRef::new(9999, 25));
        assert_eq!(host.range_values(0, range), vec![Value::Number(9998.0), Value::Number(9999.0)]);
        assert!(host.taint);
        assert!(hook.charge(0));
    }

    /// Refusal at the same-pass array lookup is terminal even for an unrelated small range.
    #[test]
    fn same_pass_spill_lookup_observes_work_refusal() {
        let hook = WorkHook::new(|units| units != 1);
        let _scope = hook.enter().unwrap();
        let wb = Workbook::new();
        let exprs = HashMap::default();
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        host.spills.insert((0, CellRef::new(1000, 25)), Arc::new(Array::column(vec![Value::Number(7.0); 10_000])));
        assert_eq!(host.range_values(0, RangeRef::new(CellRef::new(0, 0), CellRef::new(1, 0))), vec![Value::Error(CellError::Num)]);
        assert!(!hook.charge(0));
    }
}
