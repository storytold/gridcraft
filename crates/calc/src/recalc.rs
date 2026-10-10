//! Dependency graph and recalculation.
//!
//! The graph maps every formula cell to its static precedents (resolved areas). A reverse index
//! answers "which formulas read this cell?": single-cell precedents in a hash map, ranges as row
//! intervals per column (wide ranges per sheet), each an interval index so a lookup costs
//! O(log n + matches) however many ranges share the column. After an edit, the dirty set is the
//! transitive closure of dependents; it is evaluated in topological order, with on-demand
//! evaluation as a safety net for dynamic references. Cycles produce `#CIRC!` unless iterative
//! calculation is on.

use std::collections::{BTreeSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

/// A fast, non-cryptographic hasher for cell keys (FxHash-style multiply-rotate).
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(*b as u64);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{CalcMode, Workbook};

use crate::eval::{Area, Evaluator, Host, precedents};

pub type Key = (usize, CellRef);

#[derive(Clone, Debug, Default)]
struct Node {
    areas: Vec<Area>,
    dynamic: bool,
}

/// Ranges wider than this many columns go to the per-sheet wide index instead of per-column ones.
const WIDE: u32 = 64;

/// Insertions an [`Intervals`] keeps aside (scanned by every query) before merging them in.
const PENDING: usize = 32;

/// Row intervals `[r0, r1]`, each with a payload, answering "which intervals overlap rows
/// `a..=b`?" in O(log n + k).
///
/// The intervals are sorted by start with an implicit segment tree of their largest end, so a query
/// only descends into subtrees that hold a match. Removals mark entries dead (re-inserting one
/// revives it); insertions wait in a small sorted set. [`Intervals::settle`] merges them and drops
/// dead entries once either grows, so mutations cost O(log n) amortised.
#[derive(Clone, Debug)]
struct Intervals<T> {
    /// Sorted, without duplicates.
    items: Vec<(u32, u32, T)>,
    dead: Vec<bool>,
    dead_count: usize,
    /// Segment tree over `items`: node `i` has children `2i` and `2i + 1`, leaves start at
    /// `max_end.len() / 2`. Each node holds the largest end below it (dead entries included).
    max_end: Vec<u32>,
    /// Inserted since the last merge.
    pending: BTreeSet<(u32, u32, T)>,
}

impl<T> Default for Intervals<T> {
    fn default() -> Self {
        Intervals { items: Vec::new(), dead: Vec::new(), dead_count: 0, max_end: Vec::new(), pending: BTreeSet::new() }
    }
}

impl<T: Copy + Ord> Intervals<T> {
    fn is_empty(&self) -> bool {
        self.items.len() == self.dead_count && self.pending.is_empty()
    }

    fn insert(&mut self, r0: u32, r1: u32, t: T) {
        let it = (r0, r1, t);
        if let Ok(i) = self.items.binary_search(&it) {
            if let Some(d) = self.dead.get_mut(i)
                && *d
            {
                *d = false;
                self.dead_count -= 1;
            }
            return;
        }
        self.pending.insert(it);
    }

    fn remove(&mut self, r0: u32, r1: u32, t: T) {
        let it = (r0, r1, t);
        if let Ok(i) = self.items.binary_search(&it) {
            if let Some(d) = self.dead.get_mut(i)
                && !*d
            {
                *d = true;
                self.dead_count += 1;
            }
            return;
        }
        self.pending.remove(&it);
    }

    /// Merges pending insertions and drops dead entries when there are enough of them.
    fn settle(&mut self) {
        if self.pending.len() <= PENDING && self.dead_count <= (self.items.len() / 2).max(PENDING) {
            return;
        }
        let mut all: Vec<(u32, u32, T)> = Vec::with_capacity(self.items.len() - self.dead_count + self.pending.len());
        all.extend(self.items.iter().zip(&self.dead).filter(|(_, d)| !**d).map(|(it, _)| *it));
        all.extend(std::mem::take(&mut self.pending));
        // Two sorted runs: the stable sort merges them in linear time.
        all.sort();
        all.dedup();
        let size = all.len().next_power_of_two();
        let mut max_end = vec![0; 2 * size];
        if let Some(leaves) = max_end.get_mut(size..) {
            for (slot, it) in leaves.iter_mut().zip(&all) {
                *slot = it.1;
            }
        }
        for i in (1..size).rev() {
            let m = max_end.get(2 * i).copied().unwrap_or(0).max(max_end.get(2 * i + 1).copied().unwrap_or(0));
            if let Some(slot) = max_end.get_mut(i) {
                *slot = m;
            }
        }
        self.dead = vec![false; all.len()];
        self.dead_count = 0;
        self.items = all;
        self.max_end = max_end;
    }

    /// Calls `f` for every interval overlapping rows `a..=b` (`r0 <= b && r1 >= a`). Returns the
    /// number of tree nodes visited.
    fn query(&self, a: u32, b: u32, f: &mut dyn FnMut(&T)) -> usize {
        for it in &self.pending {
            if it.0 <= b && it.1 >= a {
                f(&it.2);
            }
        }
        // Entries before `p` start at or before row `b`.
        let p = self.items.partition_point(|it| it.0 <= b);
        if p == 0 {
            return 0;
        }
        self.walk(1, 0, self.max_end.len() / 2, p, a, f)
    }

    fn walk(&self, node: usize, lo: usize, hi: usize, p: usize, a: u32, f: &mut dyn FnMut(&T)) -> usize {
        if lo >= p || self.max_end.get(node).is_none_or(|m| *m < a) {
            return 1;
        }
        if hi - lo <= 1 {
            if let (Some(it), Some(false)) = (self.items.get(lo), self.dead.get(lo))
                && it.1 >= a
            {
                f(&it.2);
            }
            return 1;
        }
        let mid = lo + (hi - lo) / 2;
        1 + self.walk(2 * node, lo, mid, p, a, f) + self.walk(2 * node + 1, mid, hi, p, a, f)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: HashMap<Key, Node>,
    /// single cell → dependents
    cell_deps: HashMap<Key, HashSet<Key>>,
    /// (sheet, col) → row intervals of the ranges (up to `WIDE` columns) that cover the column
    col_deps: HashMap<(usize, u32), Intervals<Key>>,
    /// sheet → row intervals of wider ranges, with their columns: (first, last, dependent)
    wide_deps: HashMap<usize, Intervals<(u32, u32, Key)>>,
    dynamic: HashSet<Key>,
    /// Range indexes changed since the last [`Graph::settle`].
    unsettled_cols: HashSet<(usize, u32)>,
    unsettled_wide: HashSet<usize>,
}

impl Graph {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Index entries of a formula's precedents, without duplicates: single cells, column
    /// intervals and wide ranges.
    fn entries(areas: &[Area]) -> (Vec<Key>, Vec<(usize, u32, u32, u32)>, Vec<(usize, RangeRef)>) {
        let (mut cells, mut cols, mut wide) = (Vec::new(), Vec::new(), Vec::new());
        for a in areas {
            if a.range.is_single() {
                cells.push((a.sheet, a.range.start));
            } else if a.range.width() > WIDE {
                wide.push((a.sheet, a.range));
            } else {
                for c in a.range.start.col..=a.range.end.col {
                    cols.push((a.sheet, c, a.range.start.row, a.range.end.row));
                }
            }
        }
        cells.sort_unstable();
        cells.dedup();
        cols.sort_unstable();
        cols.dedup();
        wide.sort_unstable_by_key(|(s, r)| (*s, r.start, r.end));
        wide.dedup();
        (cells, cols, wide)
    }

    fn insert(&mut self, k: Key, node: Node) {
        self.remove(k);
        let (cells, cols, wide) = Self::entries(&node.areas);
        for c in cells {
            self.cell_deps.entry(c).or_default().insert(k);
        }
        for (s, c, r0, r1) in cols {
            self.col_deps.entry((s, c)).or_default().insert(r0, r1, k);
            self.unsettled_cols.insert((s, c));
        }
        for (s, r) in wide {
            self.wide_deps.entry(s).or_default().insert(r.start.row, r.end.row, (r.start.col, r.end.col, k));
            self.unsettled_wide.insert(s);
        }
        if node.dynamic {
            self.dynamic.insert(k);
        }
        self.nodes.insert(k, node);
    }

    fn remove(&mut self, k: Key) {
        let Some(old) = self.nodes.remove(&k) else { return };
        let (cells, cols, wide) = Self::entries(&old.areas);
        for c in cells {
            if let Some(v) = self.cell_deps.get_mut(&c) {
                v.remove(&k);
                if v.is_empty() {
                    self.cell_deps.remove(&c);
                }
            }
        }
        for (s, c, r0, r1) in cols {
            if let Some(v) = self.col_deps.get_mut(&(s, c)) {
                v.remove(r0, r1, k);
                self.unsettled_cols.insert((s, c));
            }
        }
        for (s, r) in wide {
            if let Some(v) = self.wide_deps.get_mut(&s) {
                v.remove(r.start.row, r.end.row, (r.start.col, r.end.col, k));
                self.unsettled_wide.insert(s);
            }
        }
        self.dynamic.remove(&k);
    }

    /// Tidies the range indexes changed by `insert`/`remove` (queries stay correct without it,
    /// just slower).
    fn settle(&mut self) {
        for key in std::mem::take(&mut self.unsettled_cols) {
            if let Some(v) = self.col_deps.get_mut(&key) {
                v.settle();
                if v.is_empty() {
                    self.col_deps.remove(&key);
                }
            }
        }
        for key in std::mem::take(&mut self.unsettled_wide) {
            if let Some(v) = self.wide_deps.get_mut(&key) {
                v.settle();
                if v.is_empty() {
                    self.wide_deps.remove(&key);
                }
            }
        }
    }

    /// Formulas that read cell `c` of `sheet`.
    pub fn dependents(&self, sheet: usize, c: CellRef, out: &mut Vec<Key>) {
        if let Some(v) = self.cell_deps.get(&(sheet, c)) {
            out.extend(v.iter().copied());
        }
        if let Some(v) = self.col_deps.get(&(sheet, c.col)) {
            v.query(c.row, c.row, &mut |k| out.push(*k));
        }
        if let Some(v) = self.wide_deps.get(&sheet) {
            v.query(c.row, c.row, &mut |(c0, c1, k)| {
                if *c0 <= c.col && c.col <= *c1 {
                    out.push(*k);
                }
            });
        }
    }

    /// Formulas that read any cell of `range` (a formula can be listed more than once).
    pub fn dependents_of_range(&self, sheet: usize, range: RangeRef, out: &mut Vec<Key>) {
        if range.count() <= 4096 {
            for c in range.iter() {
                if let Some(v) = self.cell_deps.get(&(sheet, c)) {
                    out.extend(v.iter().copied());
                }
            }
        } else {
            for ((s, c), v) in &self.cell_deps {
                if *s == sheet && range.contains(*c) {
                    out.extend(v.iter().copied());
                }
            }
        }
        let (r0, r1) = (range.start.row, range.end.row);
        if (range.width() as usize) <= self.col_deps.len() {
            for col in range.start.col..=range.end.col {
                if let Some(v) = self.col_deps.get(&(sheet, col)) {
                    v.query(r0, r1, &mut |k| out.push(*k));
                }
            }
        } else {
            for ((s, col), v) in &self.col_deps {
                if *s == sheet && range.start.col <= *col && *col <= range.end.col {
                    v.query(r0, r1, &mut |k| out.push(*k));
                }
            }
        }
        if let Some(v) = self.wide_deps.get(&sheet) {
            v.query(r0, r1, &mut |(c0, c1, k)| {
                if *c0 <= range.end.col && range.start.col <= *c1 {
                    out.push(*k);
                }
            });
        }
    }

    /// Precedent areas of a formula cell (for Trace Precedents).
    pub fn precedents_of(&self, k: Key) -> Vec<Area> {
        self.nodes.get(&k).map(|n| n.areas.clone()).unwrap_or_default()
    }

    /// The order to evaluate `dirty` in, as levels: each formula after the dirty formulas its
    /// static precedents cover (Kahn's algorithm), and the formulas of one level independent of
    /// each other, so a level can be evaluated on several threads. Formulas that are ready at the
    /// same time keep the order of `dirty`. Formulas on or behind a cycle come last (`rest`), in
    /// the order of `dirty`; evaluating them finds the cycle.
    ///
    /// The edges come from [`Graph::dependents`], which reports a formula once per index entry of
    /// its precedents that covers a cell, so a formula waits once for each such report and each
    /// is released once. Edges between dirty formulas are kept in one flat list while they stay
    /// within a budget of a few per formula; past it (many formulas over the same large range of
    /// formulas) they are looked up again instead of stored.
    pub fn levels(&self, dirty: &[Key]) -> Plan {
        let mut index: HashMap<Key, usize> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
        let mut keys: Vec<Key> = Vec::with_capacity(dirty.len());
        for &k in dirty {
            if let std::collections::hash_map::Entry::Vacant(e) = index.entry(k) {
                e.insert(keys.len());
                keys.push(k);
            }
        }
        let budget = keys.len().saturating_mul(8).saturating_add(1 << 20);
        let mut waiting = vec![0usize; keys.len()];
        let mut edges: Vec<usize> = Vec::new();
        let mut starts: Vec<usize> = Vec::with_capacity(keys.len() + 1);
        let mut stored = true;
        let mut deps = Vec::new();
        for &k in &keys {
            starts.push(edges.len());
            deps.clear();
            self.dependents(k.0, k.1, &mut deps);
            for d in &deps {
                if let Some(&j) = index.get(d)
                    && let Some(w) = waiting.get_mut(j)
                {
                    *w += 1;
                    if stored {
                        edges.push(j);
                    }
                }
            }
            if stored && edges.len() > budget {
                stored = false;
                edges = Vec::new();
            }
        }
        starts.push(edges.len());
        let mut order: Vec<Key> = Vec::with_capacity(keys.len());
        let mut bounds: Vec<usize> = Vec::new();
        let mut wave: Vec<usize> = (0..keys.len()).filter(|&i| waiting.get(i) == Some(&0)).collect();
        let mut next: Vec<usize> = Vec::new();
        let mut released: Vec<usize> = Vec::new();
        while !wave.is_empty() {
            for &i in &wave {
                let Some(&k) = keys.get(i) else { continue };
                order.push(k);
                released.clear();
                if stored {
                    let (a, b) = (starts.get(i).copied().unwrap_or(0), starts.get(i + 1).copied().unwrap_or(0));
                    released.extend(edges.get(a..b).unwrap_or(&[]));
                } else {
                    deps.clear();
                    self.dependents(k.0, k.1, &mut deps);
                    released.extend(deps.iter().filter_map(|d| index.get(d).copied()));
                }
                for &j in &released {
                    if let Some(w) = waiting.get_mut(j)
                        && *w > 0
                    {
                        *w -= 1;
                        if *w == 0 {
                            next.push(j);
                        }
                    }
                }
            }
            bounds.push(order.len());
            std::mem::swap(&mut wave, &mut next);
            next.clear();
        }
        let leveled = order.len();
        if leveled < keys.len() {
            order.extend(keys.iter().zip(&waiting).filter(|(_, w)| **w > 0).map(|(k, _)| *k));
        }
        Plan { order, bounds, leveled }
    }

    /// [`Graph::levels`] as one sequence.
    pub fn order(&self, dirty: &[Key]) -> Vec<Key> {
        self.levels(dirty).order
    }
}

/// An evaluation plan from [`Graph::levels`].
#[derive(Clone, Debug, Default)]
pub struct Plan {
    order: Vec<Key>,
    /// End of each level in `order`.
    bounds: Vec<usize>,
    /// Where the formulas on or behind cycles start in `order`.
    leveled: usize,
}

impl Plan {
    /// The levels, in order: no formula of a level reads another of the same level (statically).
    pub fn levels(&self) -> impl Iterator<Item = &[Key]> {
        let starts = std::iter::once(0).chain(self.bounds.iter().copied());
        starts.zip(self.bounds.iter().copied()).filter_map(|(a, b)| self.order.get(a..b))
    }
    /// Formulas on or behind a cycle.
    pub fn rest(&self) -> &[Key] {
        self.order.get(self.leveled..).unwrap_or(&[])
    }
}

/// Calculation state kept beside a workbook.
#[derive(Clone, Debug)]
pub struct Calc {
    pub graph: Graph,
    rng: u64,
    /// Fixed "now" for deterministic tests; `None` = system clock.
    pub fixed_now: Option<f64>,
    /// Cells whose formulas have errors from cycles.
    pub circular: Vec<Key>,
    pub last_recalc_cells: usize,
}

impl Default for Calc {
    fn default() -> Self {
        Calc { graph: Graph::default(), rng: 0x9E37_79B9_7F4A_7C15, fixed_now: None, circular: vec![], last_recalc_cells: 0 }
    }
}

/// What a pass has worked out so far: results, the formulas still to evaluate, new spills,
/// shared ranges, used ranges. On a multi-threaded level the workers read the main thread's
/// layer and each fills its own, merged back when the level is done.
#[derive(Default)]
struct Layer {
    results: HashMap<Key, Value>,
    pending: HashSet<Key>,
    /// New spills found during the pass: anchor → array.
    spills: HashMap<Key, Arc<Array>>,
    /// Ranges read more than once in this pass, shared as one array (see `range_array`).
    ranges: HashMap<(usize, RangeRef), SharedRange>,
    /// Cells held in `ranges`.
    shared_cells: usize,
    /// Used range per sheet: the workbook doesn't change during a pass, and working it out walks
    /// every row, so once per sheet rather than once per range read.
    used: HashMap<usize, Option<RangeRef>>,
}

struct PassHost<'a> {
    wb: &'a Workbook,
    exprs: &'a HashMap<Key, Arc<Expr>>,
    /// The main thread's layer, when this host is a worker thread on a multi-threaded level.
    base: Option<&'a Layer>,
    own: Layer,
    /// Formulas set aside until the pending cells they read are done (see [`PassHost::settle`]).
    in_stack: HashSet<Key>,
    /// Pending cells the formula being evaluated read before they were done.
    blocked: Vec<Key>,
    /// The formulas set aside by [`PassHost::settle`] (kept to reuse its allocation).
    stack: Vec<Key>,
    lookups: gridcraft_functions::LookupCache,
    /// Areas of spills a worker found: shared ranges of the main layer they land in are evicted
    /// when the level is merged.
    evict: Vec<(usize, RangeRef)>,
    rng: u64,
    now: f64,
    cycle: bool,
    cycles: Vec<Key>,
}

/// How often a range has been read in a pass, and its shared array once it is shared.
#[derive(Default)]
struct SharedRange {
    reads: u32,
    array: Option<Arc<Array>>,
}

/// Ranges smaller than this are rebuilt for every read: sharing them saves little.
const MIN_SHARED_CELLS: usize = 64;
/// Cells shared per pass at most (as many as a dozen full columns).
const SHARED_CELLS_BUDGET: usize = 1 << 24;
/// Levels with fewer formulas than this are evaluated on one thread: starting threads would cost
/// more than they save.
const PARALLEL_MIN: usize = 512;
/// Formulas a worker takes from a level at a time.
const CHUNK: usize = 32;
/// Formulas of a multi-threaded level evaluated first on the main thread, so the ranges and
/// lookup indexes they share are built once and every worker starts with them.
const WARM_UP: usize = 64;

/// What a worker thread worked out on one level.
struct WorkerOut {
    results: HashMap<Key, Value>,
    spills: HashMap<Key, Arc<Array>>,
    evict: Vec<(usize, RangeRef)>,
    /// Formulas that read a cell still pending in the level: the main thread finishes them.
    deferred: Vec<Key>,
    cycle: bool,
    cycles: Vec<Key>,
}

/// Calculation threads for `calc`: one on wasm, when multi-threaded calculation is off, or when
/// the machine has one processor; else the manual count or every processor, at most 64.
pub fn thread_count(calc: &gridcraft_model::CalcSettings) -> usize {
    if cfg!(target_arch = "wasm32") || !calc.multi_threaded {
        return 1;
    }
    let n = if calc.threads > 0 { calc.threads as usize } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) };
    n.clamp(1, 64)
}

/// Whether `e` draws random numbers: such formulas share one generator, so they are evaluated in
/// order on the main thread (Excel too keeps some functions off its calculation threads).
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

/// Whether an array result at `k` can't spill: it is in a table, or its area runs off the sheet,
/// overlaps merged cells, holds other cells, or another formula's spill (unless that formula is
/// recalculated in this pass, and so may spill elsewhere now).
fn spill_blocked(wb: &Workbook, exprs: &HashMap<Key, Arc<Expr>>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    let own = sh.spill_ranges.get(&anchor);
    let spilled_over = |c: CellRef| {
        !own.is_some_and(|o| o.contains(c)) && sh.spill_ranges.iter().any(|(o, r)| *o != anchor && r.contains(c) && !exprs.contains_key(&(sheet, *o)))
    };
    end.row >= gridcraft_core::MAX_ROWS
        || end.col >= gridcraft_core::MAX_COLS
        || sh.merges.iter().any(|m| m.intersects(&range))
        || sh.tables.iter().any(|t| t.range.contains(anchor))
        || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (!cell.value.is_empty() || cell.formula.is_some()))
        || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c) && spilled_over(*c))
}

impl<'a> PassHost<'a> {
    fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, Arc<Expr>>, now: f64, rng: u64, base: Option<&'a Layer>) -> Self {
        PassHost {
            wb,
            exprs,
            base,
            own: Layer::default(),
            in_stack: HashSet::default(),
            blocked: Vec::new(),
            stack: Vec::new(),
            lookups: Default::default(),
            evict: Vec::new(),
            rng,
            now,
            cycle: false,
            cycles: vec![],
        }
    }

    fn result(&self, k: &Key) -> Option<&Value> {
        self.own.results.get(k).or_else(|| self.base.and_then(|b| b.results.get(k)))
    }

    fn is_pending(&self, k: &Key) -> bool {
        self.own.pending.contains(k) || self.base.is_some_and(|b| b.pending.contains(k)) && !self.own.results.contains_key(k)
    }

    fn spill(&self, k: &Key) -> Option<&Arc<Array>> {
        self.own.spills.get(k).or_else(|| self.base.and_then(|b| b.spills.get(k)))
    }

    fn all_spills(&self) -> impl Iterator<Item = (&Key, &Arc<Array>)> {
        self.own.spills.iter().chain(self.base.into_iter().flat_map(|b| b.spills.iter()))
    }

    /// The value of `root`, evaluating it first if it is pending, together with every pending
    /// formula it turns out to read, without recursing: a formula that reads a pending cell is
    /// set aside on a stack, the cells it read go on top, and it is evaluated again once they are
    /// done. A cell read while it is set aside is part of a cycle (`#CIRC!`). Chains of any length
    /// cost heap, not call stack.
    fn settle(&mut self, root: Key) -> Value {
        if let Some(v) = self.result(&root) {
            return v.clone();
        }
        if !self.is_pending(&root) {
            return self.wb.sheet(root.0).map(|s| s.value(root.1)).unwrap_or_default();
        }
        // Fast path: a formula whose precedents are done (nearly all of them, in `levels`).
        self.in_stack.insert(root);
        if self.attempt(root) {
            self.in_stack.remove(&root);
            return self.result(&root).cloned().unwrap_or_default();
        }
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push(root);
        let mut retry = false;
        while let Some(&k) = stack.last() {
            if retry && self.attempt(k) {
                stack.pop();
                self.in_stack.remove(&k);
            } else {
                for p in std::mem::take(&mut self.blocked) {
                    if self.in_stack.insert(p) {
                        stack.push(p);
                    }
                }
            }
            retry = true;
        }
        self.stack = stack;
        self.result(&root).cloned().unwrap_or_default()
    }

    /// Evaluates pending formula `k` and records its result, or returns `false` (recording
    /// nothing) when it read pending cells, listed in `blocked`.
    fn attempt(&mut self, k: Key) -> bool {
        self.blocked.clear();
        let Some(expr) = self.exprs.get(&k).map(Arc::clone) else {
            self.own.pending.remove(&k);
            let v = self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
            self.own.results.insert(k, v);
            return true;
        };
        let v = {
            let mut ev = Evaluator::new(self, k.0, k.1);
            ev.value(&expr)
        };
        if !self.blocked.is_empty() {
            return false;
        }
        // A legacy array formula fills the range it was entered in.
        let legacy = self.wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.as_ref()).and_then(|f| f.array);
        let v = match legacy {
            Some(range) => fit_to_range(v, range),
            None => v,
        };
        self.own.pending.remove(&k);
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
                    Array::new(a.rows, a.cols, data).map(Arc::new).unwrap_or(a)
                };
                if a.rows > 1 || a.cols > 1 {
                    // Shared ranges the new spill lands in no longer hold what they read.
                    let end = CellRef::new(
                        k.1.row.saturating_add((a.rows as u32).saturating_sub(1)),
                        k.1.col.saturating_add((a.cols as u32).saturating_sub(1)),
                    );
                    let area = RangeRef::new(k.1, end);
                    evict_ranges(&mut self.own, k.0, area);
                    if self.base.is_some() {
                        self.evict.push((k.0, area));
                    }
                    self.own.spills.insert(k, a.clone());
                }
                a.data.first().cloned().unwrap_or_default()
            }
            Value::Empty => Value::Number(0.0),
            v => v,
        };
        // An array that can't spill is #SPILL! for the formulas evaluated after it in this pass too.
        let v = match self.own.spills.get(&k) {
            Some(a) if spill_blocked(self.wb, self.exprs, k, a) => {
                self.own.spills.remove(&k);
                Value::Error(CellError::Spill)
            }
            _ => v,
        };
        self.own.results.insert(k, v);
        true
    }

    /// Reading pending cell `k` from the formula being evaluated: a cycle if `k` is set aside
    /// (it waits for this formula), otherwise `k` is noted as blocking and the formula will be
    /// evaluated again. Returns whether it is a cycle.
    fn wait_for(&mut self, k: Key) -> bool {
        if self.in_stack.contains(&k) {
            self.cycle = true;
            self.cycles.push(k);
            true
        } else {
            self.blocked.push(k);
            false
        }
    }

    /// Evaluates one level of the plan: formulas that don't read each other's results (as far as
    /// their static precedents tell). A large level is split across `threads` worker threads,
    /// each reading this layer and writing its own; a formula that turns out to read a cell
    /// still pending in the level (through a name, a table, INDIRECT) is handed back and
    /// finished here, as are formulas that draw random numbers. The results are the same as on
    /// one thread. If a worker can't start or fails, the level is evaluated on this thread.
    fn run_level(&mut self, level: &[Key], threads: usize) {
        if threads <= 1 || level.len() < PARALLEL_MIN {
            for &k in level {
                self.settle(k);
            }
            return;
        }
        let exprs = self.exprs;
        let (random, mut parallel): (Vec<Key>, Vec<Key>) = level.iter().partition(|k| exprs.get(k).is_some_and(|e| uses_random(e)));
        for k in parallel.drain(..WARM_UP.min(parallel.len())) {
            self.settle(k);
        }
        let base = std::mem::take(&mut self.own);
        let (wb, now, rng) = (self.wb, self.now, self.rng);
        let lookups = &self.lookups;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let work = || {
            let mut w = PassHost::new(wb, exprs, now, rng, Some(&base));
            w.lookups = lookups.fork();
            let mut deferred = Vec::new();
            loop {
                let i = next.fetch_add(CHUNK, std::sync::atomic::Ordering::Relaxed);
                let Some(chunk) = parallel.get(i..parallel.len().min(i.saturating_add(CHUNK))) else { break };
                if chunk.is_empty() {
                    break;
                }
                for &k in chunk {
                    w.in_stack.insert(k);
                    if !w.attempt(k) {
                        deferred.push(k);
                    }
                    w.in_stack.remove(&k);
                }
            }
            WorkerOut { results: w.own.results, spills: w.own.spills, evict: w.evict, deferred, cycle: w.cycle, cycles: w.cycles }
        };
        let mut outs = Vec::with_capacity(threads);
        let mut failed = false;
        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(threads);
            for _ in 0..threads.min(parallel.len().div_ceil(CHUNK)) {
                match std::thread::Builder::new().name("gridcraft-calc".into()).spawn_scoped(s, work) {
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
        self.own = base;
        if failed {
            log::warn!("multi-threaded calculation failed; calculating the level on one thread");
            for &k in level {
                self.settle(k);
            }
            return;
        }
        let mut deferred = Vec::new();
        for out in outs {
            for (k, v) in out.results {
                self.own.pending.remove(&k);
                self.own.results.insert(k, v);
            }
            self.own.spills.extend(out.spills);
            for (sheet, area) in out.evict {
                evict_ranges(&mut self.own, sheet, area);
            }
            deferred.extend(out.deferred);
            self.cycle |= out.cycle;
            self.cycles.extend(out.cycles);
        }
        deferred.sort_by_key(|(s, c)| (*s, c.row, c.col));
        for k in deferred.into_iter().chain(random) {
            self.settle(k);
        }
    }
}

/// Drops the shared ranges of `layer` on `sheet` that `area` (a new spill) overlaps.
fn evict_ranges(layer: &mut Layer, sheet: usize, area: RangeRef) {
    let before = layer.ranges.len();
    layer.ranges.retain(|(s, r), _| *s != sheet || !r.intersects(&area));
    if layer.ranges.len() != before {
        layer.shared_cells = layer.ranges.values().filter_map(|x| x.array.as_ref()).map(|a| a.data.len()).sum();
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
    fn workbook(&self) -> &Workbook {
        self.wb
    }
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        let k = (sheet, anchor);
        let done = self.result(&k).is_some();
        if !(done || self.is_pending(&k)) {
            return self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied();
        }
        // Recalculated in this pass: its new array, unless something blocks it (#SPILL!).
        if !done {
            self.wait_for(k);
            return None;
        }
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
        if let Some(v) = self.result(&k) {
            return v.clone();
        }
        if self.is_pending(&k) {
            // Not done yet: the value is a placeholder, the reading formula is evaluated again.
            return if self.wait_for(k) { Value::Error(CellError::Circ) } else { Value::Empty };
        }
        // A cell covered by a spill computed in this pass.
        for (anchor, arr) in self.all_spills() {
            if anchor.0 == sheet && c.row >= anchor.1.row && c.col >= anchor.1.col {
                let (dr, dc) = ((c.row - anchor.1.row) as usize, (c.col - anchor.1.col) as usize);
                if dr < arr.rows && dc < arr.cols {
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
        // `cell_value`, constants are copied straight from the store.
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
        let new: Vec<(usize, Value)> = self
            .all_spills()
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
        for (i, v) in new {
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        out
    }
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        if let Some(u) = self.own.used.get(&sheet).or_else(|| self.base.and_then(|b| b.used.get(&sheet))) {
            return *u;
        }
        let u = self.wb.sheet(sheet).and_then(|s| s.used_range());
        self.own.used.insert(sheet, u);
        u
    }
    /// Shares a range from its second read on: a lookup table or `A:A` read by thousands of
    /// formulas is built once, and lookups can index the one array. A range is shared only when
    /// building it read no cell that wasn't done yet (no placeholder, no cycle), so it holds the
    /// final values of the pass; a spill computed later in the pass that lands in it evicts it.
    fn range_array(&mut self, sheet: usize, range: RangeRef) -> Value {
        let (h, w) = (range.height() as usize, range.width() as usize);
        let cells = h.saturating_mul(w);
        let mut share = false;
        if cells >= MIN_SHARED_CELLS {
            if let Some(a) = self.base.and_then(|b| b.ranges.get(&(sheet, range))).and_then(|s| s.array.as_ref()) {
                return Value::Array(Arc::clone(a));
            }
            let slot = self.own.ranges.entry((sheet, range)).or_default();
            if let Some(a) = &slot.array {
                return Value::Array(Arc::clone(a));
            }
            slot.reads = slot.reads.saturating_add(1);
            share = slot.reads >= 2 && self.own.shared_cells.saturating_add(cells) <= SHARED_CELLS_BUDGET;
        }
        let (blocked, cycles) = (self.blocked.len(), self.cycles.len());
        let data = self.range_values(sheet, range);
        let Some(array) = Array::new(h, w, data) else { return Value::Error(CellError::Value) };
        let array = Arc::new(array);
        if share
            && self.blocked.len() == blocked
            && self.cycles.len() == cycles
            && let Some(slot) = self.own.ranges.get_mut(&(sheet, range))
        {
            slot.array = Some(Arc::clone(&array));
            self.own.shared_cells += cells;
        }
        Value::Array(array)
    }
    fn lookup_cache(&mut self) -> Option<&mut gridcraft_functions::LookupCache> {
        Some(&mut self.lookups)
    }
    fn now_serial(&self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Calc {
    pub fn new() -> Self {
        Self::default()
    }

    fn now(&self) -> f64 {
        if let Some(n) = self.fixed_now {
            return n;
        }
        now_serial()
    }

    /// Rebuilds the graph from scratch.
    pub fn rebuild(&mut self, wb: &Workbook) {
        self.graph = Graph::default();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (c, cell) in sheet.cells.iter() {
                if let Some(f) = &cell.formula
                    && let Some(e) = f.expr()
                {
                    let (areas, dynamic) = precedents(wb, si, &e);
                    self.graph.insert((si, c), Node { areas, dynamic });
                }
            }
        }
        self.graph.settle();
    }

    /// Recalculates every formula (F9 / Ctrl+Alt+F9, and after loading).
    pub fn recalc_all(&mut self, wb: &mut Workbook) {
        self.rebuild(wb);
        // Spills are laid out again from the formulas where they are now (a structural edit moves
        // or deletes the formulas, not the values they spilled).
        for i in 0..wb.sheets.len() {
            if wb.sheet(i).is_some_and(|s| !s.spill_ranges.is_empty() || !s.spill.is_empty())
                && let Some(sh) = wb.sheet_mut(i)
            {
                sh.spill.clear();
                sh.spill_ranges.clear();
            }
        }
        let all: Vec<Key> = self.graph.nodes.keys().copied().collect();
        self.run(wb, all, true);
    }

    /// Updates the graph for cells whose content changed and recalculates what depends on them.
    pub fn cells_changed(&mut self, wb: &mut Workbook, changed: &[Key]) {
        let mut cleared: Vec<(usize, RangeRef)> = Vec::new();
        for &k in changed {
            let formula = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.clone());
            match formula.and_then(|f| f.expr()) {
                Some(e) => {
                    let (areas, dynamic) = precedents(wb, k.0, &e);
                    self.graph.insert(k, Node { areas, dynamic });
                }
                None => {
                    self.graph.remove(k);
                    // A formula replaced by a constant (or cleared) takes its spilled values with it.
                    if let Some(sh) = wb.sheet_mut(k.0)
                        && let Some(r) = sh.spill_ranges.remove(&k.1)
                    {
                        for c in r.iter() {
                            sh.spill.remove(&c);
                        }
                        cleared.push((k.0, r));
                    }
                    // A formula that doesn't parse evaluates to #NAME?.
                    if let Some(cell) = wb.sheet_mut(k.0).and_then(|s| s.cells.get_mut(k.1))
                        && cell.formula.is_some()
                    {
                        cell.value = Value::Error(CellError::Name);
                    }
                }
            }
        }
        self.graph.settle();
        if wb.calc.mode == CalcMode::Manual {
            // Only the edited formulas themselves are evaluated.
            let own: Vec<Key> = changed.iter().copied().filter(|k| self.graph.nodes.contains_key(k)).collect();
            self.run(wb, own, false);
            return;
        }
        let mut seeds: Vec<Key> = changed.to_vec();
        let mut emptied: Vec<Key> = Vec::new();
        // Typing into (or clearing) a cell a dynamic array spills over (or would spill over)
        // re-evaluates the anchor, which may now be blocked or unblocked.
        for &k in changed {
            if let Some(sh) = wb.sheet(k.0) {
                for (anchor, r) in &sh.spill_ranges {
                    if *anchor != k.1 && r.contains(k.1) {
                        seeds.push((k.0, *anchor));
                    }
                }
                for (c, cell) in sh.cells.iter_range(RangeRef::new(CellRef::new(k.1.row.saturating_sub(64), k.1.col.saturating_sub(64)), k.1)) {
                    if c != k.1 && cell.value == Value::Error(CellError::Spill) {
                        seeds.push((k.0, c));
                    }
                }
                if !sh.cells.has(k.1) {
                    match emptied.iter_mut().find(|x| x.0 == k.0) {
                        Some(x) => x.1 = CellRef::new(x.1.row.max(k.1.row), x.1.col.max(k.1.col)),
                        None => emptied.push(k),
                    }
                }
            }
        }
        // An emptied cell can unblock an anchor any distance above or to the left of it.
        for (si, end) in emptied {
            for &(s, c) in self.graph.nodes.keys() {
                if s == si && c.row <= end.row && c.col <= end.col && wb.sheet(s).is_some_and(|sh| sh.value(c) == Value::Error(CellError::Spill)) {
                    seeds.push((s, c));
                }
            }
        }
        // Spill areas of changed anchors also change.
        for &k in changed {
            if let Some(r) = wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1)) {
                seeds.extend(r.iter().take(65536).map(|c| (k.0, c)));
            }
        }
        for (si, r) in &cleared {
            seeds.extend(r.iter().take(65536).map(|c| (*si, c)));
        }
        let t0 = prof_now();
        let dirty = self.dirty_closure(wb, &seeds);
        let t1 = prof_now();
        let n = dirty.len();
        self.run(wb, dirty, false);
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("recalc: {} seeds, closure {} cells {:.1} ms, run {:.1} ms", seeds.len(), n, t1 - t0, prof_now() - t1);
        }
    }

    fn dirty_closure(&self, _wb: &Workbook, seeds: &[Key]) -> Vec<Key> {
        let mut seen: HashSet<Key> = HashSet::default();
        let mut queue: VecDeque<Key> = VecDeque::new();
        for k in seeds {
            if self.graph.nodes.contains_key(k) && seen.insert(*k) {
                queue.push_back(*k);
            }
            let mut deps = Vec::new();
            self.graph.dependents(k.0, k.1, &mut deps);
            for d in deps {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        for k in &self.graph.dynamic {
            if seen.insert(*k) {
                queue.push_back(*k);
            }
        }
        let mut deps = Vec::new();
        while let Some(k) = queue.pop_front() {
            deps.clear();
            self.graph.dependents(k.0, k.1, &mut deps);
            for d in deps.drain(..) {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Evaluates the dirty set and writes results (and spills) back.
    fn run(&mut self, wb: &mut Workbook, mut dirty: Vec<Key>, full: bool) {
        let mut dynamic_again = !self.graph.dynamic.is_empty();
        for _round in 0..8 {
            if dirty.is_empty() {
                break;
            }
            // Row-major order is a good topological guess; on-demand evaluation fixes the rest.
            dirty.sort_by_key(|(s, c)| (*s, c.row, c.col));
            let mut exprs: HashMap<Key, std::sync::Arc<Expr>> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
            for k in &dirty {
                if let Some(e) = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.as_ref()).and_then(|f| f.expr_arc()) {
                    exprs.insert(*k, e);
                }
            }
            let now = self.now();
            let threads = thread_count(&wb.calc);
            let tp = prof_now();
            let (results, spills, cycles, rng) = {
                let mut host = PassHost::new(wb, &exprs, now, self.rng, None);
                host.own.pending = exprs.keys().copied().collect();
                host.own.results.reserve(dirty.len());
                let plan = self.graph.levels(&dirty);
                for level in plan.levels() {
                    host.run_level(level, threads);
                }
                for &k in plan.rest() {
                    host.settle(k);
                }
                (host.own.results, host.own.spills, host.cycles, host.rng)
            };
            self.rng = rng;
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  eval {} formulas {:.1} ms", results.len(), prof_now() - tp);
            }
            let tw = prof_now();
            self.last_recalc_cells = results.len();
            self.circular = cycles;
            // Write back.
            let mut spill_changes: Vec<Key> = Vec::new();
            let mut freed: Vec<(usize, RangeRef)> = Vec::new();
            for (k, v) in results {
                let Some(sheet) = wb.sheet_mut(k.0) else { continue };
                if let Some(cell) = sheet.cells.get_mut(k.1)
                    && cell.value != v
                {
                    cell.value = v;
                }
            }
            for k in &dirty {
                let new = spills.get(k);
                let Some(sheet) = wb.sheet_mut(k.0) else { continue };
                let old = sheet.spill_ranges.get(&k.1).copied();
                if old.is_none() && new.is_none() {
                    continue;
                }
                if let Some(r) = old {
                    for c in r.iter() {
                        sheet.spill.remove(&c);
                    }
                    sheet.spill_ranges.remove(&k.1);
                }
                let mut changed_range = old;
                if let Some(arr) = new {
                    let end = CellRef::new(k.1.row.saturating_add(arr.rows as u32 - 1), k.1.col.saturating_add(arr.cols as u32 - 1));
                    let range = RangeRef::new(k.1, end);
                    let blocked = end.row >= gridcraft_core::MAX_ROWS
                        || end.col >= gridcraft_core::MAX_COLS
                        || range.iter().any(|c| c != k.1 && (sheet.cells.has(c) || sheet.spill.contains_key(&c)))
                        || sheet.merges.iter().any(|m| m.intersects(&range))
                        || sheet.tables.iter().any(|t| t.range.contains(k.1));
                    if blocked {
                        if let Some(cell) = sheet.cells.get_mut(k.1) {
                            cell.value = Value::Error(CellError::Spill);
                        }
                    } else {
                        for (i, c) in range.iter().enumerate() {
                            if c != k.1 {
                                sheet.spill.insert(c, arr.data.get(i).cloned().unwrap_or_default());
                            }
                        }
                        sheet.spill_ranges.insert(k.1, range);
                        changed_range = Some(changed_range.map_or(range, |o| o.union(&range)));
                    }
                }
                if let Some(r) = changed_range {
                    let mut deps = Vec::new();
                    self.graph.dependents_of_range(k.0, r, &mut deps);
                    spill_changes.extend(deps.into_iter().filter(|d| d != k));
                }
                if let Some(o) = old
                    && !sheet.spill_ranges.get(&k.1).is_some_and(|n| n.contains(o.start) && n.contains(o.end))
                {
                    freed.push((k.0, o));
                }
            }
            // Formulas with references only known while evaluating (INDIRECT, OFFSET) may have read
            // a spill area before it was laid out: they are evaluated once more.
            if dynamic_again && !spills.is_empty() {
                dynamic_again = false;
                spill_changes.extend(self.graph.dynamic.iter().copied());
            }
            // An area a formula no longer spills over may unblock another formula's array.
            for (si, o) in freed {
                for &(s, c) in self.graph.nodes.keys() {
                    if s == si
                        && c.row <= o.end.row
                        && c.col <= o.end.col
                        && wb.sheet(s).is_some_and(|sh| sh.value(c) == Value::Error(CellError::Spill))
                    {
                        spill_changes.push((s, c));
                    }
                }
            }
            if full {
                // Everything was already evaluated once; spill dependents need another pass.
            }
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  write-back {:.1} ms", prof_now() - tw);
            }
            spill_changes.sort_by_key(|(s, c)| (*s, c.row, c.col));
            spill_changes.dedup();
            if spill_changes.is_empty() {
                break;
            }
            dirty = self.dirty_closure(wb, &spill_changes);
        }
    }
}

/// Local date-time as a serial (1900 system). Wasm without a clock returns a fixed date.
#[allow(clippy::disallowed_methods)] // the clock is read only off wasm
pub fn now_serial() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            // UTC; local offset isn't available without platform calls.
            return 25569.0 + d.as_secs_f64() / 86400.0;
        }
    }
    46302.5
}

/// Evaluates a formula in the context of a cell without storing it (conditional formats, data
/// validation, Evaluate Formula, Name Manager previews, the CLI `eval`).
pub fn evaluate(wb: &Workbook, sheet: usize, at: CellRef, formula: &str) -> Value {
    let body = formula.strip_prefix('=').unwrap_or(formula);
    let Ok(expr) = gridcraft_formula::parse(body) else { return Value::Error(CellError::Name) };
    evaluate_expr(wb, sheet, at, &expr)
}

pub fn evaluate_expr(wb: &Workbook, sheet: usize, at: CellRef, expr: &Expr) -> Value {
    let exprs = HashMap::default();
    let mut host = PassHost::new(wb, &exprs, now_serial(), 0x1234_5678_9ABC_DEF0, None);
    let mut ev = Evaluator::new(&mut host, sheet, at);
    ev.value(expr)
}

#[allow(clippy::disallowed_methods)] // the clock is read only off wasm
fn prof_now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gridcraft_core::{CellRef, RangeRef, Value};
    use gridcraft_model::{Cell, Formula, Workbook};

    use super::{Calc, Intervals, Key};

    /// xorshift64 for repeatable pseudo-random cases.
    fn rng(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn found(iv: &Intervals<u32>, a: u32, b: u32) -> Vec<u32> {
        let mut v = Vec::new();
        iv.query(a, b, &mut |t| v.push(*t));
        v.sort_unstable();
        v
    }

    #[test]
    fn intervals_match_a_linear_scan() {
        let mut iv: Intervals<u32> = Intervals::default();
        let mut model: Vec<(u32, u32, u32)> = Vec::new();
        let mut s = 0x9E37_79B9_7F4A_7C15;
        for step in 0..20_000u32 {
            let roll = rng(&mut s) % 10;
            if roll < 6 || model.is_empty() {
                let r0 = (rng(&mut s) % 500) as u32;
                let r1 = r0 + (rng(&mut s) % 40) as u32;
                if !model.contains(&(r0, r1, step)) {
                    iv.insert(r0, r1, step);
                    model.push((r0, r1, step));
                }
            } else {
                let i = (rng(&mut s) % model.len() as u64) as usize;
                let (r0, r1, t) = model.swap_remove(i);
                iv.remove(r0, r1, t);
                if roll == 9 {
                    // Removed and put back before the next merge.
                    iv.insert(r0, r1, t);
                    model.push((r0, r1, t));
                }
            }
            if rng(&mut s).is_multiple_of(50) {
                iv.settle();
            }
            if step.is_multiple_of(97) {
                let a = (rng(&mut s) % 560) as u32;
                let b = a + (rng(&mut s) % 30) as u32;
                let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= b && *r1 >= a).map(|(_, _, t)| *t).collect();
                want.sort_unstable();
                assert_eq!(found(&iv, a, b), want, "rows {a}..={b} at step {step}");
            }
        }
        iv.settle();
        for row in 0..560 {
            let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= row && row <= *r1).map(|(_, _, t)| *t).collect();
            want.sort_unstable();
            assert_eq!(found(&iv, row, row), want, "row {row}");
        }
        for (r0, r1, t) in std::mem::take(&mut model) {
            iv.remove(r0, r1, t);
        }
        iv.settle();
        assert!(iv.is_empty());
    }

    #[test]
    fn interval_queries_cost_log_n_plus_matches() {
        // 40k one-row ranges (like =SUM(E1:J1) down a column) and 1k ranges over everything:
        // a point query visits O(log n) tree nodes per match, not every range in the column.
        let n = 40_000u32;
        let mut iv: Intervals<u32> = Intervals::default();
        for r in 0..n {
            iv.insert(r, r, r);
        }
        for t in 0..1_000 {
            iv.insert(0, n, n + t);
        }
        iv.settle();
        let mut worst = 0;
        for row in (0..n).step_by(7) {
            let mut k = 0;
            let visited = iv.query(row, row, &mut |_| k += 1);
            assert_eq!(k, 1_001);
            worst = worst.max(visited);
        }
        // 1,001 matches in a tree of depth 16: well under 4 nodes per match plus the path.
        assert!(worst < 4 * 1_001 + 64, "visited {worst} nodes");
        let mut k = 0;
        let visited = iv.query(n + 1, n + 5, &mut |_| k += 1);
        assert_eq!(k, 0);
        assert!(visited < 64, "visited {visited} nodes for no match");
    }

    /// `rows` rows of six numbers (E:J) and `=SUM(E{r}:J{r})` in K.
    fn sum_rows(rows: u32) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for r in 0..rows {
                for c in 4..10 {
                    sh.cells.set(CellRef::new(r, c), Cell::value(Value::Number(f64::from(r % 97 + c))));
                }
                sh.cells.set(CellRef::new(r, 10), Cell::formula(Formula::new(&format!("=SUM(E{0}:J{0})", r + 1))));
            }
        }
        wb
    }

    /// Opening (full recalc), changing every value of a column, and re-entering every formula.
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn open_and_bulk_edit(rows: u32) -> Duration {
        let mut wb = sum_rows(rows);
        let mut calc = Calc::new();
        let t = Instant::now();
        calc.recalc_all(&mut wb);
        let values: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 4))).collect();
        if let Some(sh) = wb.sheet_mut(0) {
            for (_, c) in &values {
                sh.cells.set(*c, Cell::value(Value::Number(1.0)));
            }
        }
        calc.cells_changed(&mut wb, &values);
        let formulas: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 10))).collect();
        calc.cells_changed(&mut wb, &formulas);
        let elapsed = t.elapsed();
        let last = wb.sheet(0).map(|s| s.value(CellRef::new(rows - 1, 10)));
        let r = rows - 1;
        assert_eq!(last, Some(Value::Number(f64::from(1 + (5..10).map(|c| r % 97 + c).sum::<u32>()))));
        elapsed
    }

    #[test]
    fn range_formulas_recalc_and_bulk_edit_correctly() {
        // The values behind the timing test below, checked on every run.
        open_and_bulk_edit(500);
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    #[allow(clippy::disallowed_methods)]
    fn recalc_with_range_formulas_scales_linearly() {
        // Each row's SUM reads a range in the same columns. Recalculating on open used to walk
        // every row per range read (the used range), and finding a cell's dependents scanned
        // every range in its column: 4x the rows took 16x the time. Linear is 4x; allow 8x.
        let best = |rows| (0..3).map(|_| open_and_bulk_edit(rows)).min().unwrap_or_default();
        let small = best(5_000);
        let large = best(20_000);
        assert!(large < small * 8 + Duration::from_millis(100), "5k rows {small:?}, 20k rows {large:?}");
    }

    #[test]
    fn range_dependents_after_edits() {
        // Formulas that read ranges, wide ranges and whole columns, found again after edits.
        let mut wb = Workbook::new();
        let mut calc = Calc::new();
        let set = |wb: &mut Workbook, calc: &mut Calc, at: &str, input: &str| {
            let c = CellRef::parse(at).unwrap();
            let cell = if input.starts_with('=') { Cell::formula(Formula::new(input)) } else { Cell::value(Value::Number(input.parse().unwrap())) };
            wb.sheet_mut(0).unwrap().cells.set(c, cell);
            calc.cells_changed(wb, &[(0, c)]);
        };
        let num = |wb: &Workbook, at: &str| {
            let v = wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap());
            v.as_f64().unwrap_or_else(|| panic!("{at} = {v:?}"))
        };
        set(&mut wb, &mut calc, "A1", "1");
        set(&mut wb, &mut calc, "B2", "2");
        set(&mut wb, &mut calc, "CZ3", "3");
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:B2)");
        set(&mut wb, &mut calc, "DA6", "=SUM(A1:CZ3)");
        set(&mut wb, &mut calc, "DA7", "=SUM(B:B)");
        set(&mut wb, &mut calc, "DA8", "=SUM(2:2)");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (3.0, 6.0, 2.0, 2.0));
        set(&mut wb, &mut calc, "B2", "20");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (21.0, 24.0, 20.0, 20.0));
        // Re-entering a formula with another range drops the old one.
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:A2)");
        set(&mut wb, &mut calc, "B2", "5");
        assert_eq!(num(&wb, "DA5"), 1.0);
        let mut deps = Vec::new();
        calc.graph.dependents(0, CellRef::parse("B2").unwrap(), &mut deps);
        deps.sort_unstable();
        let at = |s: &str| (0, CellRef::parse(s).unwrap());
        assert_eq!(deps, vec![at("DA6"), at("DA7"), at("DA8")]);
        deps.clear();
        calc.graph.dependents_of_range(0, RangeRef::new(CellRef::parse("A1").unwrap(), CellRef::parse("A9").unwrap()), &mut deps);
        deps.sort_unstable();
        deps.dedup();
        assert_eq!(deps, vec![at("DA5"), at("DA6"), at("DA8")]);
        set(&mut wb, &mut calc, "CZ3", "30");
        assert_eq!(num(&wb, "DA6"), 36.0);
        // Ranges on another sheet.
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
        set(&mut wb, &mut calc, "DA9", "=SUM(Data!A1:A3)+SUM(Data!A:CZ)");
        assert_eq!(num(&wb, "DA9"), 0.0);
        wb.sheet_mut(1).unwrap().cells.set(CellRef::parse("A2").unwrap(), Cell::value(Value::Number(4.0)));
        calc.cells_changed(&mut wb, &[(1, CellRef::parse("A2").unwrap())]);
        assert_eq!(num(&wb, "DA9"), 8.0);
        deps.clear();
        calc.graph.dependents(1, CellRef::parse("A2").unwrap(), &mut deps);
        assert_eq!(deps, vec![at("DA9"), at("DA9")]);
    }
}
