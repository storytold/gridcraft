use super::graph::{Graph, HashMap, HashSet, Key};
use super::{Evaluator, Host, Workbook, prof_now, stack_position};
use crate::eval::{NameIndex, ParsedName};
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
/// Most definition text, in bytes, whose parsed formulas a pass keeps. A parsed formula takes
/// about a hundred times its text, so a workbook of many long names would otherwise hold gigabytes
/// for the pass; past this, about 100 MB of trees, a name is parsed again at each use, as before
/// the pass kept any. A library of LAMBDA names is a few kilobytes.
const MAX_PASS_NAME_TEXT: usize = 1 << 20;

/// Levels with fewer formulas than this are calculated on the main thread: starting threads
/// would cost more than they save. `GRIDCRAFT_PARALLEL_MIN` overrides it (tests thread tiny
/// levels to shake the worker path).
const PARALLEL_MIN: usize = 512;

/// Formulas a worker takes from a level at a time.
const CHUNK: usize = 32;

/// Stack of a calculation thread: at least the main thread's on every desktop platform (8 MiB
/// on macOS and Linux, 1 MiB on Windows), so a formula nests no less deep on a worker than on
/// the main thread.
const WORKER_STACK: usize = 8 << 20;

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
    pub(super) depth: usize,
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
    /// Parsed defined names, by their place in the workbook's list: the workbook doesn't change
    /// during a pass, and a column of cells calling one LAMBDA name would otherwise parse that
    /// name's formula (and every name it calls) once per cell.
    names: HashMap<usize, Option<std::sync::Arc<Expr>>>,
    /// The workbook's names by lower-cased name and scope, built at the first lookup.
    name_index: Option<NameIndex>,
    /// Bytes of the definitions kept in `names`.
    names_text: usize,
    /// The main thread's host, when this one runs on a worker thread of a multi-threaded level:
    /// what it has calculated so far is read through it (see [`PassHost::worker`]).
    base: Option<&'a PassHost<'a>>,
    /// On a worker: the formula being evaluated read a formula that isn't done, or drew a random
    /// number, so it is handed back to the main thread (see [`PassHost::compute`]).
    pub(super) blocked: bool,
    /// On a worker: large blocks read twice in this level, with the text copied, so that
    /// reading them again clones reference counts this thread alone holds. Every thread reading
    /// the same lookup column clones (and drops) each cell's shared text otherwise, and those
    /// counters bounce between the cores: with eight threads that cost more than the calculation.
    pub(super) ranges: HashMap<(usize, RangeRef), Vec<Value>>,
    /// On a worker: blocks read once in this level; the second read copies them, so a block read
    /// by one formula only (a running total's, say) never pays for a copy.
    pub(super) seen: HashSet<(usize, RangeRef)>,
    /// Bytes held in `ranges`, and the most this host may hold (its share of
    /// [`CACHE_BYTES_BUDGET`]).
    pub(super) cached_bytes: usize,
    cache_budget: usize,
    /// Levels calculated on worker threads, and on this thread because their workers could not
    /// run.
    pub(super) worker_levels: usize,
    pub(super) fallbacks: usize,
    /// Formulas the workers handed back to this host (see [`Calc::handed_back`]).
    pub(super) handed_back: usize,
    /// Levels in which formulas after an array laid out differently from what the workbook
    /// stores were calculated again on this thread (see [`Calc::relaid_levels`]).
    pub(super) relaid_levels: usize,
    /// Formulas of levels run on workers that this host calculated at their turn (handed back,
    /// random, or with an array the workers' result can't stand with), for tests.
    #[cfg(test)]
    pub(super) on_main: usize,
    /// Of those, the ones whose worker result was thrown away for an array before them.
    #[cfg(test)]
    pub(super) redone: usize,
    /// While `recording`: formulas whose array this host laid out differently from what the
    /// workbook stores (in place, shape or values, or none where one is stored); see `run_level`.
    laid_out: Vec<Key>,
    recording: bool,
    /// The dependency graph, for the readers of an array's area (see `run_level`), and its
    /// formulas whose references are only known while evaluating.
    graph: Option<&'a Graph>,
    dynamic: Option<&'a HashSet<Key>>,
    /// Formulas a worker takes from a level at a time ([`CHUNK`]; tests use smaller chunks to
    /// spread the formulas of small workbooks over the workers).
    pub(super) chunk: usize,
    #[cfg(test)]
    pub(super) fail_workers: bool,
}

/// Blocks smaller than this aren't kept per worker: copying them costs more than it saves.
const MIN_CACHED_CELLS: usize = 64;
/// Bytes the workers of a level keep in copied blocks at most, all together: text is copied in
/// full, so the budget counts bytes, not cells (a cell can hold 32K characters).
const CACHE_BYTES_BUDGET: usize = 256 << 20;

/// What a copied block costs to keep: the values, and the text they hold (with its
/// reference-count header).
fn block_bytes(block: &[Value]) -> usize {
    block.iter().map(|v| std::mem::size_of::<Value>() + if let Value::Text(t) = v { t.len() + 16 } else { 0 }).sum()
}

impl<'a> PassHost<'a> {
    pub(super) fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, graph: Option<&'a Graph>, rng: &'a mut u64, now: f64) -> Self {
        PassHost {
            wb,
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
            names: HashMap::default(),
            name_index: None,
            names_text: 0,
            base: None,
            blocked: false,
            ranges: HashMap::default(),
            seen: HashSet::default(),
            cached_bytes: 0,
            cache_budget: 0,
            worker_levels: 0,
            fallbacks: 0,
            handed_back: 0,
            relaid_levels: 0,
            #[cfg(test)]
            on_main: 0,
            #[cfg(test)]
            redone: 0,
            laid_out: Vec::new(),
            recording: false,
            graph,
            dynamic: graph.map(|g| &g.dynamic),
            chunk: CHUNK,
            #[cfg(test)]
            fail_workers: false,
        }
    }

    /// A host for a worker thread of a multi-threaded level: it reads what this host has
    /// calculated so far and records its own results, to be merged back by
    /// [`PassHost::run_level`]. It never calculates a formula on demand (see
    /// [`PassHost::compute`]), so it holds no pending set.
    pub(super) fn worker<'w>(&'w self, rng: &'w mut u64, cache_budget: usize) -> PassHost<'w> {
        PassHost {
            wb: self.wb,
            exprs: self.exprs,
            results: HashMap::default(),
            pending: HashSet::default(),
            in_progress: HashSet::default(),
            spills: HashMap::default(),
            used: HashMap::default(),
            rng,
            now: self.now,
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
            names: HashMap::default(),
            name_index: None,
            names_text: self.names_text,
            base: Some(self),
            blocked: false,
            ranges: HashMap::default(),
            seen: HashSet::default(),
            cached_bytes: 0,
            cache_budget,
            worker_levels: 0,
            fallbacks: 0,
            handed_back: 0,
            relaid_levels: 0,
            #[cfg(test)]
            on_main: 0,
            #[cfg(test)]
            redone: 0,
            laid_out: Vec::new(),
            recording: false,
            graph: self.graph,
            dynamic: self.dynamic,
            chunk: CHUNK,
            #[cfg(test)]
            fail_workers: false,
        }
    }

    /// Keeps a parsed definition for the rest of the pass while the definitions kept stay within
    /// [`MAX_PASS_NAME_TEXT`] (on a worker, counted on from what the main thread keeps).
    fn keep_name(&mut self, i: usize, expr: Option<std::sync::Arc<Expr>>) {
        if self.names.contains_key(&i) {
            return;
        }
        let len = self.wb.names.get(i).map_or(0, |d| d.formula.len());
        let kept = self.names_text.saturating_add(len);
        if kept <= MAX_PASS_NAME_TEXT {
            self.names_text = kept;
            self.names.insert(i, expr);
        }
    }

    /// The result of `k` from this pass so far (a worker's own, or the main thread's).
    fn result(&self, k: &Key) -> Option<&Value> {
        self.results.get(k).or_else(|| self.base.and_then(|b| b.results.get(k)))
    }

    /// Whether `k` is a formula of this pass that isn't done yet.
    fn is_pending(&self, k: &Key) -> bool {
        self.pending.contains(k) || self.base.is_some_and(|b| b.pending.contains(k) && !self.results.contains_key(k))
    }

    /// The array `k` spilled in this pass so far.
    fn spill(&self, k: &Key) -> Option<&std::sync::Arc<Array>> {
        self.spills.get(k).or_else(|| self.base.and_then(|b| b.spills.get(k)))
    }

    /// The arrays spilled in this pass so far.
    fn all_spills(&self) -> impl Iterator<Item = (&Key, &std::sync::Arc<Array>)> {
        self.spills.iter().chain(self.base.into_iter().flat_map(|b| b.spills.iter()))
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

/// Whether what `k` spills in this pass (`arr`, or nothing) reads as what the workbook stores
/// for it: an array laid out as before, or no array and no stored spill range. An array of one
/// cell is its anchor's value, which a reader gets from the formula itself wherever it is
/// calculated, so one laid out anew, or one stored and gone, changes nothing a reader sees.
fn array_as_stored(wb: &Workbook, k: Key, arr: Option<&std::sync::Arc<Array>>) -> bool {
    match (wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1).map(|r| (s, *r))), arr) {
        (None, None) => true,
        (None, Some(a)) => a.rows == 1 && a.cols == 1,
        (Some((_, r)), None) => r.start == r.end,
        (Some((s, r)), Some(a)) => spills_as_before(s, k.1, r, a),
    }
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

    pub(super) fn compute(&mut self, k: Key) -> Value {
        if let Some(v) = self.result(&k) {
            let v = v.clone();
            if self.iterate && self.tainted.contains(&k) {
                self.taint = true;
            }
            return v;
        }
        let Some(expr) = self.exprs.get(&k) else {
            return self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
        };
        if self.base.is_some() && self.depth > 0 {
            // A worker reads a formula that isn't done: the formula reading it is handed back to
            // the main thread rather than calculating it here (another worker may be on it, and
            // a chain of them would nest). The value is thrown away.
            self.blocked = true;
            return Value::Error(CellError::Calc);
        }
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
        if self.blocked {
            // A worker: the evaluation read a formula that isn't done, so `run_level` evaluates
            // it again on the main thread once the level is merged.
            self.depth -= 1;
            self.in_progress.remove(&k);
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
                if !self.ranges.is_empty() {
                    // Copied blocks the new array lands in no longer hold what they read.
                    let end = CellRef::new(
                        k.1.row.saturating_add((a.rows as u32).saturating_sub(1)),
                        k.1.col.saturating_add((a.cols as u32).saturating_sub(1)),
                    );
                    let area = RangeRef::new(k.1, end);
                    let bytes = &mut self.cached_bytes;
                    self.ranges.retain(|(s, r), v| {
                        let keep = *s != k.0 || !r.intersects(&area);
                        if !keep {
                            *bytes = bytes.saturating_sub(block_bytes(v));
                        }
                        keep
                    });
                }
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
        if self.recording && !array_as_stored(self.wb, k, self.spills.get(&k)) {
            self.laid_out.push(k);
        }
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

    /// Calculates one level of a [`Plan`]: formulas that read no other formula of the level
    /// (statically). A small level, or one thread, calculates them on this thread in order.
    /// Otherwise worker threads take the formulas in chunks, each evaluating with its own host
    /// over this one's results so far; a formula that turns out to read a formula not done yet
    /// (through INDIRECT, OFFSET, a spill-range operator on a pending anchor, a name whose
    /// definition reads one) is handed back, and formulas that draw random numbers stay here, so
    /// the one generator draws in order. The level is then walked in row-major order, the order
    /// one thread calculates it in: a worker's result is merged at its turn, and a formula that
    /// runs here is calculated at its turn with the usual on-demand machinery.
    ///
    /// A worker read what this host held when the workers started, and its own results so far.
    /// That differs from what one thread reads at the same turn only by the formulas before it
    /// in the level, which it doesn't read (the level's formulas don't read one another), and by
    /// their arrays where those differ from what the workbook stores (in place, shape or values,
    /// or lost). So whenever the walk meets such an array (merged, laid out here, or a worker's
    /// that is thrown away), the formulas after it whose references meet its new area (the
    /// planner already holds the readers of the stored one back to a later level), and every
    /// dynamic formula after it (INDIRECT, OFFSET, a volatile function: what they read
    /// isn't known), are calculated here at their turn instead of merged. Every formula is
    /// calculated over the state one thread would have had at its turn, and a level costs one
    /// worker pass. If a thread can't start or fails, the level is calculated on this thread.
    pub(super) fn run_level(&mut self, level: &[Key], threads: usize, parallel_min: usize) {
        if threads <= 1 || level.len() < parallel_min {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        let exprs = self.exprs;
        let wb = self.wb;
        #[cfg(test)]
        let fail = self.fail_workers;
        let t0 = prof_now();
        let cache_budget = CACHE_BYTES_BUDGET / threads.max(1);
        let chunk_size = self.chunk.max(1);
        let parallel: Vec<Key> = level.iter().copied().filter(|k| !exprs.get(k).is_some_and(|e| uses_random(e))).collect();
        if parallel.is_empty() {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        // The workers share the main thread's index of names rather than each building one.
        if !wb.names.is_empty() {
            self.name_index.get_or_insert_with(|| NameIndex::new(wb));
        }
        let seed = *self.rng;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let me: &PassHost<'_> = &*self;
        let work = || {
            #[cfg(test)]
            #[allow(clippy::panic)]
            if fail {
                panic!("worker failure injected by a test");
            }
            let mut rng = seed;
            let mut w = me.worker(&mut rng, cache_budget);
            let mut deferred = Vec::new();
            loop {
                let i = next.fetch_add(chunk_size, std::sync::atomic::Ordering::Relaxed);
                let Some(chunk) = parallel.get(i..parallel.len().min(i.saturating_add(chunk_size))) else { break };
                if chunk.is_empty() {
                    break;
                }
                for &k in chunk {
                    w.blocked = false;
                    w.compute(k);
                    if w.blocked {
                        deferred.push(k);
                    }
                }
            }
            // A worker never finds a cycle: reading a formula not done hands the reader back
            // before the in-progress check, so `w.cycles` stays empty.
            WorkerOut { results: w.results, spills: w.spills, deferred, names: w.names, used: w.used }
        };
        let mut outs: Vec<WorkerOut> = Vec::with_capacity(threads);
        let mut failed = false;
        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(threads);
            for _ in 0..threads.min(parallel.len().div_ceil(chunk_size)) {
                match std::thread::Builder::new().name("gridcraft-calc".into()).stack_size(WORKER_STACK).spawn_scoped(s, work) {
                    Ok(h) => handles.push(h),
                    Err(_) => break,
                }
            }
            failed = handles.is_empty();
            for h in handles {
                match h.join() {
                    Ok(out) => outs.push(out),
                    Err(_) => failed = true,
                }
            }
        });
        if failed {
            if self.fallbacks == 0 {
                log::warn!("calculation threads could not run; calculating on one thread");
            }
            self.fallbacks += 1;
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        self.worker_levels += 1;
        // What the workers found, and the formulas they handed back.
        let mut results: HashMap<Key, Value> = HashMap::default();
        let mut spills: HashMap<Key, std::sync::Arc<Array>> = HashMap::default();
        let mut handed = 0usize;
        for out in outs {
            // A worker spills only for the formulas it has a result for (a formula it hands back
            // stops before its array is kept).
            spills.extend(out.spills);
            results.extend(out.results);
            handed += out.deferred.len();
            for (i, expr) in out.names {
                self.keep_name(i, expr);
            }
            self.used.extend(out.used);
        }
        self.handed_back += handed;
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("  level of {}: {} handed back, {:.1} ms on workers", level.len(), handed, prof_now() - t0);
        }
        // In row-major order: a formula calculated here (with on-demand reads, random draws and
        // arrays laid out as on one thread), or a worker's result merged in. `again`: the
        // formulas after an array laid out differently from what is stored that may have read its
        // cells; `every_dynamic`: such an array has been met, so every dynamic formula after it
        // runs here too.
        let mut again: HashSet<Key> = HashSet::default();
        let mut every_dynamic = false;
        let mut redone = 0usize;
        for &k in level {
            let worker_array = spills.remove(&k);
            let worker_result = results.remove(&k);
            let thrown = |host: &Self, again: &mut HashSet<Key>| {
                // A worker's array that is thrown away: the formulas after it on that worker may
                // have read it. (An array that differs from what is stored came from a state that
                // differed from one thread's, so an array laid out differently has been met before
                // it and the dynamic formulas after it are already being calculated again.)
                if let Some(a) = &worker_array
                    && !array_as_stored(wb, k, Some(a))
                {
                    host.mark_readers(k, Some(a), again);
                }
            };
            if self.results.contains_key(&k) {
                // Calculated on demand by a formula before it.
                thrown(self, &mut again);
                continue;
            }
            let dynamic = self.dynamic.is_none_or(|d| d.contains(&k));
            let read_stale = again.contains(&k) || (every_dynamic && dynamic);
            // A formula handed back has no worker result.
            if read_stale || worker_result.is_none() {
                if read_stale && worker_result.is_some() {
                    redone += 1;
                }
                thrown(self, &mut again);
                #[cfg(test)]
                {
                    self.on_main += 1;
                }
                self.laid_out.clear();
                self.recording = true;
                self.calculate(k);
                self.recording = false;
                for a in std::mem::take(&mut self.laid_out) {
                    self.mark_readers(a, self.spills.get(&a), &mut again);
                    every_dynamic = true;
                }
            } else if let Some(v) = worker_result {
                // (Every lookup looks at the results as well; the pending set is kept accurate.)
                self.pending.remove(&k);
                self.results.insert(k, v);
                if let Some(a) = worker_array {
                    self.spills.insert(k, a);
                }
                if !array_as_stored(wb, k, self.spills.get(&k)) {
                    self.mark_readers(k, self.spills.get(&k), &mut again);
                    every_dynamic = true;
                }
            }
        }
        if redone > 0 {
            self.relaid_levels += 1;
        }
        #[cfg(test)]
        {
            self.redone += redone;
        }
    }

    /// Adds to `again` the formulas whose references meet the area `k`'s array covers (`arr`).
    /// Not the area the workbook stores for it: the planner holds its readers back to a later
    /// level. Without the graph, none: then every formula counts as dynamic, and all of them
    /// after it run again anyway.
    fn mark_readers(&self, k: Key, arr: Option<&std::sync::Arc<Array>>, again: &mut HashSet<Key>) {
        let Some(graph) = self.graph else { return };
        let mut deps = Vec::new();
        if let Some(a) = arr {
            let end =
                CellRef::new(k.1.row.saturating_add((a.rows as u32).saturating_sub(1)), k.1.col.saturating_add((a.cols as u32).saturating_sub(1)));
            graph.dependents_of_range(k.0, RangeRef::new(k.1, end), &mut deps);
        }
        again.extend(deps);
    }
}

/// What a worker thread worked out on one level.
struct WorkerOut {
    results: HashMap<Key, Value>,
    spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Formulas that read a formula not done yet: the main thread calculates them.
    deferred: Vec<Key>,
    names: HashMap<usize, Option<std::sync::Arc<Expr>>>,
    used: HashMap<usize, Option<RangeRef>>,
}

/// Calculation threads for `calc`: one on wasm, when multi-threaded calculation is off, or when
/// the machine has one processor; else the manual count or every processor, at most the
/// processors present and 64. `GRIDCRAFT_CALC_THREADS` in the environment overrides the
/// workbook's setting (to time or rule out threading without editing the file); 0 means every
/// processor there too.
pub fn thread_count(calc: &gridcraft_model::CalcSettings) -> usize {
    static OVERRIDE: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    // A count never exceeds the processors present: the workbook's setting travels with the
    // file, and 64 workers on a 2-core machine would only cost memory.
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 64);
    if let Some(n) = OVERRIDE.get_or_init(|| std::env::var("GRIDCRAFT_CALC_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok())) {
        return if *n == 0 { cores } else { (*n).min(cores) };
    }
    if !calc.multi_threaded {
        return 1;
    }
    if calc.threads > 0 { (calc.threads as usize).min(cores) } else { cores }
}

/// [`PARALLEL_MIN`], or `GRIDCRAFT_PARALLEL_MIN` from the environment.
pub(super) fn parallel_min() -> usize {
    static MIN: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MIN.get_or_init(|| std::env::var("GRIDCRAFT_PARALLEL_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(PARALLEL_MIN))
}

/// Whether `e` draws random numbers itself: such formulas share one generator, so they are
/// calculated in order on the main thread (a draw inside a name's body is caught as it happens).
fn uses_random(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |x| {
        if let Expr::Call(n, _) = x
            && matches!(n.as_str(), "RAND" | "RANDBETWEEN" | "RANDARRAY")
        {
            found = true;
        }
    });
    found
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
    fn workbook(&self) -> &Workbook {
        self.wb
    }
    fn stack_top(&self) -> Option<usize> {
        (self.depth > 1).then_some(self.stack_top)
    }
    fn stack_exhausted(&mut self) {
        self.out_of_stack = true;
    }
    /// The parsed definition, cached for the pass: the workbook's names don't change while it runs.
    /// Parsed once per definition, whichever sheets ask for it, and kept for the pass while the
    /// definitions kept stay within [`MAX_PASS_NAME_TEXT`]; past that, parsed at each use. A worker
    /// reads the main thread's index and parsed names first, and keeps what it parses itself
    /// within what is left of that bound.
    fn name_expr(&mut self, name: &str, scope_sheet: usize) -> Option<ParsedName> {
        let wb = self.wb;
        let i = match self.base.and_then(|b| b.name_index.as_ref()) {
            Some(index) if self.name_index.is_none() => index.find(name, scope_sheet)?,
            _ => self.name_index.get_or_insert_with(|| NameIndex::new(wb)).find(name, scope_sheet)?,
        };
        let def = wb.names.get(i)?;
        if let Some(expr) = self.names.get(&i).or_else(|| self.base.and_then(|b| b.names.get(&i))) {
            return Some((expr.clone(), def.scope));
        }
        let expr = gridcraft_formula::parse_name(&def.formula).ok().map(std::sync::Arc::new);
        self.keep_name(i, expr.clone());
        Some((expr, def.scope))
    }
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        let k = (sheet, anchor);
        if !(self.is_pending(&k) || self.result(&k).is_some()) {
            return self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied();
        }
        // Recalculated in this pass: its new array, unless something blocks it (#SPILL!).
        self.compute(k);
        let arr = self.spill(&k)?;
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
        // Spelled out rather than through `is_pending`/`result`: this is the hottest path of a
        // pass (once per cell of every range read), and on the main thread it stays two lookups.
        if self.pending.contains(&k)
            || self.results.contains_key(&k)
            || self.base.is_some_and(|b| b.pending.contains(&k) || b.results.contains_key(&k))
        {
            return self.compute(k);
        }
        // A cell covered by a spill computed in this pass; of two, the one anchored first in
        // row-major order, which is the one the write-back keeps, whatever order the arrays are
        // kept in. (Checked for emptiness first: this runs once per cell of every range read,
        // and the chained iterator isn't free.)
        if !self.spills.is_empty() || self.base.is_some_and(|b| !b.spills.is_empty()) {
            let mut hit: Option<(CellRef, Value)> = None;
            for (anchor, arr) in self.spills.iter().chain(self.base.into_iter().flat_map(|b| b.spills.iter())) {
                if anchor.0 == sheet && c.row >= anchor.1.row && c.col >= anchor.1.col && hit.as_ref().is_none_or(|(a, _)| anchor.1 < *a) {
                    let (dr, dc) = ((c.row - anchor.1.row) as usize, (c.col - anchor.1.col) as usize);
                    if dr < arr.rows && dc < arr.cols {
                        if self.iterate && self.tainted.contains(anchor) {
                            self.taint = true;
                        }
                        hit = Some((anchor.1, arr.get(dr, dc).cloned().unwrap_or_default()));
                    }
                }
            }
            if let Some((_, v)) = hit {
                return v;
            }
        }
        self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default()
    }
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        if self.base.is_some()
            && let Some(hit) = self.ranges.get(&(sheet, range))
        {
            return hit.clone();
        }
        let blocked_before = self.blocked;
        let out = self.read_range(sheet, range);
        // Copied on a worker, on the second read of the block, when no formula in it was
        // pending either time, within the worker's share of the byte budget.
        if self.base.is_some() && out.len() >= MIN_CACHED_CELLS && !self.seen.insert((sheet, range)) && !self.blocked && !blocked_before {
            let bytes = block_bytes(&out);
            if self.cached_bytes.saturating_add(bytes) <= self.cache_budget {
                let copy: Vec<Value> = out
                    .iter()
                    .map(|v| match v {
                        Value::Text(t) => Value::Text(std::sync::Arc::from(&**t)),
                        v => v.clone(),
                    })
                    .collect();
                self.cached_bytes += bytes;
                self.ranges.insert((sheet, range), copy);
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
        if self.all_spills().any(|(a, _)| a.0 == sheet) || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c)) {
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
        if let Some(u) = self.base.and_then(|b| b.used.get(&sheet)) {
            return *u;
        }
        let wb = self.wb;
        *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(|s| s.used_range()))
    }
    fn now_serial(&self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        if self.base.is_some() {
            // A worker: the formula is handed back to the main thread, which draws from the one
            // generator (a name's body can call RAND where `uses_random` doesn't look).
            self.blocked = true;
        }
        // xorshift64*
        let mut x = *self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl PassHost<'_> {
    /// Values of a block, row-major, from the store, this pass's spills and the formulas
    /// calculated so far (see [`Host::range_values`]).
    fn read_range(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
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
        if !self.spills.is_empty() || self.base.is_some_and(|b| !b.spills.is_empty()) {
            // The arrays over the block, each with the part of it they cover. Of two arrays over
            // one cell the one anchored first in row-major order wins, as at the write-back: it is
            // laid down last here.
            let mut arrays: Vec<(CellRef, &std::sync::Arc<Array>, RangeRef)> = self
                .all_spills()
                .filter(|(a, _)| a.0 == sheet)
                .filter_map(|(a, arr)| {
                    let end = CellRef::new(
                        a.1.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                        a.1.col.saturating_add((arr.cols as u32).saturating_sub(1)),
                    );
                    RangeRef::new(a.1, end).intersection(&range).map(|part| (a.1, arr, part))
                })
                .collect();
            arrays.sort_by_key(|x| std::cmp::Reverse(x.0));
            for (anchor, arr, part) in arrays {
                for row in part.start.row..=part.end.row {
                    for col in part.start.col..=part.end.col {
                        if (row, col) == (anchor.row, anchor.col) {
                            continue;
                        }
                        let i = (row - range.start.row) as usize * w + (col - range.start.col) as usize;
                        if let Some(slot) = out.get_mut(i) {
                            *slot = arr.get((row - anchor.row) as usize, (col - anchor.col) as usize).cloned().unwrap_or_default();
                        }
                    }
                }
            }
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
        }
        out
    }
}
