//! Dependency graph and recalculation.
//!
//! The graph maps every formula cell to its static precedents (resolved areas). A reverse index
//! answers "which formulas read this cell?": single-cell precedents in a hash map, ranges as row
//! intervals per column (wide ranges per sheet), each an interval index so a lookup costs
//! O(log n + matches) however many ranges share the column. After an edit, the dirty set is the
//! transitive closure of dependents; it is evaluated in topological order, with on-demand
//! evaluation as a safety net for dynamic references. Cycles produce `#CIRC!` unless iterative
//! calculation is on: then a formula read while it is being evaluated gives its previous value,
//! and the formulas that read one are evaluated again until the loop settles
//! (`PassHost::iterate_loops`).

use std::collections::{BTreeSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};

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

use crate::eval::{Area, Evaluator, Host, NameCache, precedents_with};

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

/// Formulas a formula may calculate on demand one inside the other (each needing one another's
/// value before it is done). Each level takes stack, so a chain deeper than this is suspended and
/// calculated from the top instead (see [`PassHost::calculate`]): long dependency chains calculate
/// whatever order they come in, with bounded stack.
const MAX_NESTED: usize = 64;

/// Formula evaluations iterative calculation may spend on one recalculation (passes × formulas
/// evaluated again), however high Maximum Iterations is.
const MAX_ITERATION_WORK: usize = 10_000_000;

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
    /// Whether the formula at `k` is evaluated on every recalc (its references aren't known
    /// statically: INDIRECT, OFFSET, tables, unresolvable names).
    pub fn is_dynamic(&self, k: Key) -> bool {
        self.dynamic.contains(&k)
    }
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

    /// Orders `dirty` into levels: a formula's level is one past the highest level of the dirty
    /// formulas it reads (statically), so the formulas of a level don't read one another and
    /// can be calculated in any order, or at once. Formulas on a cycle, and those behind one,
    /// never reach a level and come last (see [`Plan::rest`]). `None` when planning would cost
    /// more than it saves (a budget of index work), and the caller calculates in row-major order.
    ///
    /// Each formula waits for the dirty formulas inside its precedent areas, counted through a
    /// per-column index of the dirty formulas (a binary search per column an area covers, not a
    /// report per cell: a formula over a column of 25K formulas costs one search, not 25K
    /// reports). A formula with few precedents gets an exact release list from them; one with
    /// many (a whole column of formulas) is recounted when a level completes, through the index
    /// of the formulas not leveled yet. A formula that spills now also holds back the readers of
    /// its spill range, so they come after it and read the new array rather than the old one.
    pub(crate) fn levels(&self, wb: &Workbook, dirty: &[Key]) -> Option<Plan> {
        self.levels_within(wb, dirty, None)
    }

    /// [`Graph::levels`] with the index work capped at `cap` (tests force the row-major fallback).
    pub(crate) fn levels_within(&self, wb: &Workbook, dirty: &[Key], cap: Option<usize>) -> Option<Plan> {
        /// Precedents up to this many get an exact release list; more are recounted per level.
        const EXACT: usize = 64;
        /// Index work per dirty formula before planning is given up: columns visited (twice for
        /// a formula with an exact list), up to `EXACT` entries pushed and later released, and
        /// recounts. A few microseconds a formula, a small part of calculating it.
        const BUDGET_PER_FORMULA: usize = 4 * EXACT;
        let mut index: HashMap<Key, u32> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
        let mut keys: Vec<Key> = Vec::with_capacity(dirty.len());
        for &k in dirty {
            if let std::collections::hash_map::Entry::Vacant(e) = index.entry(k) {
                let i = u32::try_from(keys.len()).ok()?;
                e.insert(i);
                keys.push(k);
            }
        }
        let n = keys.len();
        let budget = n.saturating_mul(BUDGET_PER_FORMULA).saturating_add(1 << 20).min(cap.unwrap_or(usize::MAX));
        // The dirty formulas by (sheet, column), rows sorted, with the formula's index; and per
        // sheet the columns that hold any, for areas wider than that list.
        let mut by_col: Vec<(usize, u32, u32, u32)> = keys.iter().enumerate().map(|(i, k)| (k.0, k.1.col, k.1.row, i as u32)).collect();
        by_col.sort_unstable();
        let mut cols: HashMap<(usize, u32), ColIndex> = HashMap::default();
        let mut sheet_cols: HashMap<usize, Vec<u32>> = HashMap::default();
        let mut pos: Vec<((usize, u32), usize)> = vec![((0, 0), 0); n];
        for &(sheet, col, row, i) in &by_col {
            let c = cols.entry((sheet, col)).or_insert_with(|| {
                sheet_cols.entry(sheet).or_default().push(col);
                ColIndex::default()
            });
            if let Some(p) = pos.get_mut(i as usize) {
                *p = ((sheet, col), c.rows.len());
            }
            c.rows.push(row);
            c.keys.push(i);
        }
        for c in cols.values_mut() {
            c.pending = Fenwick::ones(c.rows.len());
        }
        let mut work = 0usize;
        // Visits each column of the index that `area` covers.
        fn visit(
            cols: &HashMap<(usize, u32), ColIndex>,
            sheet_cols: &HashMap<usize, Vec<u32>>,
            area: &Area,
            work: &mut usize,
            f: &mut dyn FnMut(&ColIndex),
        ) {
            let (c0, c1) = (area.range.start.col, area.range.end.col);
            let Some(list) = sheet_cols.get(&area.sheet) else { return };
            if ((c1 - c0) as usize) < list.len() {
                for col in c0..=c1 {
                    *work += 1;
                    if let Some(c) = cols.get(&(area.sheet, col)) {
                        f(c);
                    }
                }
            } else {
                let from = list.partition_point(|c| *c < c0);
                for col in list.get(from..).unwrap_or(&[]).iter().take_while(|c| **c <= c1) {
                    *work += 1;
                    if let Some(c) = cols.get(&(area.sheet, *col)) {
                        f(c);
                    }
                }
            }
        }
        let mut waits = vec![0usize; n];
        let mut spill_waits = vec![0usize; n];
        let mut release: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut spill_release: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut big: Vec<u32> = Vec::new();
        let mut deps = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let areas = self.nodes.get(k).map(|nd| nd.areas.as_slice()).unwrap_or(&[]);
            let mut total = 0usize;
            for a in areas {
                let (r0, r1) = (a.range.start.row, a.range.end.row);
                visit(&cols, &sheet_cols, a, &mut work, &mut |c| total += c.span(r0, r1).len());
            }
            if total <= EXACT {
                // The entries are charged as they are pushed, so a plan that can't fit the budget
                // is given up here, before the lists are built in full.
                work += total;
                for a in areas {
                    let (r0, r1) = (a.range.start.row, a.range.end.row);
                    visit(&cols, &sheet_cols, a, &mut work, &mut |c| {
                        for &j in c.keys.get(c.span(r0, r1)).unwrap_or(&[]) {
                            if let Some(r) = release.get_mut(j as usize) {
                                r.push(i as u32);
                            }
                        }
                    });
                }
            } else {
                big.push(i as u32);
            }
            if let Some(w) = waits.get_mut(i) {
                *w = total;
            }
            // Readers of the range this formula spills over now. The query walks the range's
            // cells, or the whole cell index past 4,096 of them, and the row intervals of the
            // columns: charged to the budget like the rest.
            if let Some(r) = wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1)) {
                deps.clear();
                self.dependents_of_range(k.0, *r, &mut deps);
                deps.sort_unstable();
                deps.dedup();
                let cells = usize::try_from(r.count()).unwrap_or(usize::MAX);
                work += deps.len() + if cells > 4096 { self.cell_deps.len() } else { cells } + (r.width() as usize).min(self.col_deps.len());
                for d in &deps {
                    if let Some(&j) = index.get(d)
                        && j as usize != i
                        && let Some(w) = spill_waits.get_mut(j as usize)
                    {
                        *w += 1;
                        if let Some(r) = spill_release.get_mut(i) {
                            r.push(j);
                        }
                    }
                }
            }
            if work > budget {
                return None;
            }
        }
        let is_big: HashSet<u32> = big.iter().copied().collect();
        // A wide reader with `left` precedents pending can't be ready before `left` more
        // formulas complete, anywhere: it is counted again only then, so the readers of a long
        // chain's end are recounted once per `left` waves, not every wave.
        // Kept by the completion count they are due at, so a wave only looks at the due ones.
        let mut big: std::collections::BinaryHeap<std::cmp::Reverse<(usize, u32)>> =
            big.iter().map(|&i| std::cmp::Reverse((waits.get(i as usize).copied().unwrap_or(0), i))).collect();
        let mut completed = 0usize;
        let mut order: Vec<Key> = Vec::with_capacity(n);
        let mut bounds: Vec<usize> = Vec::new();
        let mut done = vec![false; n];
        let mut wave: Vec<u32> = (0..n).filter(|&i| waits.get(i) == Some(&0) && spill_waits.get(i) == Some(&0)).map(|i| i as u32).collect();
        let mut next: Vec<u32> = Vec::new();
        while !wave.is_empty() {
            completed += wave.len();
            for &i in &wave {
                let Some(&k) = keys.get(i as usize) else { continue };
                order.push(k);
                if let Some(d) = done.get_mut(i as usize) {
                    *d = true;
                }
                if let Some((col, p)) = pos.get(i as usize)
                    && let Some(c) = cols.get_mut(col)
                {
                    c.pending.decrement(*p);
                }
            }
            for &i in &wave {
                let ready = |j: u32, waits: &[usize], spill_waits: &[usize], next: &mut Vec<u32>| {
                    let j = j as usize;
                    if waits.get(j) == Some(&0) && spill_waits.get(j) == Some(&0) && !is_big.contains(&(j as u32)) && done.get(j) == Some(&false) {
                        next.push(j as u32);
                    }
                };
                for &j in release.get(i as usize).map(Vec::as_slice).unwrap_or(&[]) {
                    work += 1;
                    if let Some(w) = waits.get_mut(j as usize)
                        && *w > 0
                    {
                        *w -= 1;
                        ready(j, &waits, &spill_waits, &mut next);
                    }
                }
                for &j in spill_release.get(i as usize).map(Vec::as_slice).unwrap_or(&[]) {
                    work += 1;
                    if let Some(w) = spill_waits.get_mut(j as usize)
                        && *w > 0
                    {
                        *w -= 1;
                        ready(j, &waits, &spill_waits, &mut next);
                    }
                }
            }
            // Wide readers: counted again through what is still pending, once enough formulas
            // have completed for the count to have reached zero.
            while let Some(&std::cmp::Reverse((due, i))) = big.peek() {
                if due > completed {
                    break;
                }
                big.pop();
                let Some(k) = keys.get(i as usize) else { continue };
                work += 1;
                let mut left = waits.get(i as usize).copied().unwrap_or(0);
                if left > 0 {
                    let areas = self.nodes.get(k).map(|nd| nd.areas.as_slice()).unwrap_or(&[]);
                    left = 0;
                    for a in areas {
                        let (r0, r1) = (a.range.start.row, a.range.end.row);
                        visit(&cols, &sheet_cols, a, &mut work, &mut |c| left += c.pending_in(c.span(r0, r1)));
                    }
                    if let Some(w) = waits.get_mut(i as usize) {
                        *w = left;
                    }
                }
                // A count of zero stays zero; the reader may still wait on a spill range, which
                // is looked at again next wave.
                if left == 0 && spill_waits.get(i as usize) == Some(&0) {
                    next.push(i);
                } else {
                    big.push(std::cmp::Reverse((completed.saturating_add(left.max(1)), i)));
                }
            }
            if work > budget {
                return None;
            }
            bounds.push(order.len());
            // Row-major within the level (`dirty` is sorted so), whatever order released them:
            // the same order on one thread and on many, and the sequence RAND draws in.
            next.sort_unstable();
            std::mem::swap(&mut wave, &mut next);
            next.clear();
        }
        let leveled = order.len();
        if leveled < n {
            order.extend(keys.iter().zip(&done).filter(|(_, d)| !**d).map(|(k, _)| *k));
        }
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            let sizes: Vec<usize> = std::iter::once(0).chain(bounds.iter().copied()).zip(bounds.iter().copied()).map(|(a, b)| b - a).collect();
            eprintln!("  plan: {} index work, {} wide readers, level sizes {:?}", work, is_big.len(), &sizes[..sizes.len().min(12)]);
        }
        Some(Plan { order, bounds, leveled, work })
    }
}

/// The dirty formulas of one column, for [`Graph::levels`]: rows sorted, each with the formula's
/// index in the plan, and which of them are not leveled yet.
#[derive(Default)]
struct ColIndex {
    rows: Vec<u32>,
    keys: Vec<u32>,
    pending: Fenwick,
}

impl ColIndex {
    /// The positions of the rows in `r0..=r1`.
    fn span(&self, r0: u32, r1: u32) -> std::ops::Range<usize> {
        let a = self.rows.partition_point(|r| *r < r0);
        let b = a + self.rows.get(a..).unwrap_or(&[]).partition_point(|r| *r <= r1);
        a..b
    }
    /// How many of the positions are not leveled yet.
    fn pending_in(&self, span: std::ops::Range<usize>) -> usize {
        self.pending.prefix(span.end).saturating_sub(self.pending.prefix(span.start))
    }
}

/// Prefix counts over positions, each starting at one and decremented once.
#[derive(Default)]
struct Fenwick(Vec<usize>);

// `isolate_lowest_one` needs Rust 1.97; the workspace builds on 1.90.
#[allow(clippy::manual_isolate_lowest_one)]
impl Fenwick {
    fn ones(n: usize) -> Fenwick {
        let mut t = vec![0usize; n + 1];
        for i in 1..=n {
            if let Some(x) = t.get_mut(i) {
                *x += 1;
            }
            let j = i + (i & i.wrapping_neg());
            if j <= n {
                let add = t.get(i).copied().unwrap_or(0);
                if let Some(y) = t.get_mut(j) {
                    *y += add;
                }
            }
        }
        Fenwick(t)
    }
    /// Count of positions `< end`.
    fn prefix(&self, end: usize) -> usize {
        let mut i = end.min(self.0.len().saturating_sub(1));
        let mut s = 0;
        while i > 0 {
            s += self.0.get(i).copied().unwrap_or(0);
            i &= i - 1;
        }
        s
    }
    fn decrement(&mut self, p: usize) {
        let n = self.0.len().saturating_sub(1);
        let mut i = p + 1;
        while i <= n {
            if let Some(x) = self.0.get_mut(i) {
                *x = x.saturating_sub(1);
            }
            i += i & i.wrapping_neg();
        }
    }
}

/// An evaluation plan from [`Graph::levels`].
#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    order: Vec<Key>,
    /// End of each level in `order`.
    bounds: Vec<usize>,
    /// Where the formulas on or behind cycles start in `order`.
    leveled: usize,
    /// Index work it took (tests bound it).
    work: usize,
}

impl Plan {
    /// The index work planning took: columns visited, entries released, readers recounted.
    pub(crate) fn work(&self) -> usize {
        self.work
    }
    /// The levels, in order: no formula of a level reads another of the same level (statically).
    pub(crate) fn levels(&self) -> impl Iterator<Item = &[Key]> {
        let starts = std::iter::once(0).chain(self.bounds.iter().copied());
        starts.zip(self.bounds.iter().copied()).filter_map(|(a, b)| self.order.get(a..b))
    }
    /// Formulas on or behind a cycle.
    pub(crate) fn rest(&self) -> &[Key] {
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
    /// Levels with fewer formulas than this are calculated on the main thread (see
    /// [`PARALLEL_MIN`]); tests lower it to put every level on worker threads.
    pub(crate) parallel_min: usize,
    /// Levels calculated on worker threads in the last recalculation.
    pub worker_levels: usize,
    /// Levels whose worker threads could not be started or failed, calculated on the main
    /// thread instead, in the last recalculation.
    pub worker_fallbacks: usize,
    /// Makes every worker thread fail, to test the fallback.
    #[cfg(test)]
    pub(crate) fail_workers: bool,
    /// Calculation threads regardless of the settings, the environment and the processors
    /// present, so a test means what it says on any machine.
    #[cfg(test)]
    pub(crate) threads: Option<usize>,
    /// Caps the planner's index work, to test the row-major fallback.
    #[cfg(test)]
    pub(crate) plan_budget: Option<usize>,
}

impl Default for Calc {
    fn default() -> Self {
        Calc {
            graph: Graph::default(),
            rng: 0x9E37_79B9_7F4A_7C15,
            fixed_now: None,
            circular: vec![],
            last_recalc_cells: 0,
            parallel_min: parallel_min(),
            worker_levels: 0,
            worker_fallbacks: 0,
            #[cfg(test)]
            fail_workers: false,
            #[cfg(test)]
            threads: None,
            #[cfg(test)]
            plan_budget: None,
        }
    }
}

struct PassHost<'a> {
    wb: &'a Workbook,
    exprs: &'a HashMap<Key, std::sync::Arc<Expr>>,
    results: HashMap<Key, Value>,
    pending: HashSet<Key>,
    in_progress: HashSet<Key>,
    /// New spills found during the pass: anchor → array.
    spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Used range per sheet: the workbook doesn't change during a pass, and working it out walks
    /// every row, so once per sheet rather than once per range read.
    used: HashMap<usize, Option<RangeRef>>,
    rng: &'a mut u64,
    now: f64,
    cycle: bool,
    cycles: Vec<Key>,
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
    suspensions: usize,
    /// The formulas in `cycles`.
    cycle_seen: HashSet<Key>,
    /// Iterative calculation: a circular reference reads the cell's previous value instead of
    /// giving `#CIRC!`.
    iterate: bool,
    /// Values from the previous iteration; cells not in it read their stored value.
    prev: HashMap<Key, Value>,
    /// Formulas being evaluated, innermost last (kept only when iterating).
    stack: Vec<Key>,
    /// Formulas found on a circular reference (when iterating).
    members: HashSet<Key>,
    /// Results that read a previous value, directly or through other formulas.
    tainted: HashSet<Key>,
    /// Whether the formula being evaluated has read a previous value.
    taint: bool,
    /// Parsed defined names, by (lower-cased name, scope sheet asked from): the workbook doesn't
    /// change during a pass, and a column of cells calling one LAMBDA name would otherwise parse
    /// that name's formula (and every name it calls) once per cell.
    names: HashMap<(String, usize), Option<(Option<std::sync::Arc<Expr>>, Option<usize>)>>,
    /// The main thread's host, when this one runs on a worker thread of a multi-threaded level:
    /// what it has calculated so far is read through it (see [`PassHost::worker`]).
    base: Option<&'a PassHost<'a>>,
    /// On a worker: the formula being evaluated read a formula that isn't done, or drew a random
    /// number, so it is handed back to the main thread (see [`PassHost::compute`]).
    blocked: bool,
    /// On a worker: large blocks read twice in this level, with the text copied, so that
    /// reading them again clones reference counts this thread alone holds. Every thread reading
    /// the same lookup column clones (and drops) each cell's shared text otherwise, and those
    /// counters bounce between the cores: with eight threads that cost more than the calculation.
    ranges: HashMap<(usize, RangeRef), Vec<Value>>,
    /// On a worker: blocks read once in this level; the second read copies them, so a block read
    /// by one formula only (a running total's, say) never pays for a copy.
    seen: HashSet<(usize, RangeRef)>,
    /// Bytes held in `ranges`, and the most this host may hold (its share of
    /// [`CACHE_BYTES_BUDGET`]).
    cached_bytes: usize,
    cache_budget: usize,
    /// Levels calculated on worker threads, and on this thread because their workers could not
    /// run.
    worker_levels: usize,
    fallbacks: usize,
    #[cfg(test)]
    fail_workers: bool,
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
    fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, rng: &'a mut u64, now: f64) -> Self {
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
            names: HashMap::default(),
            base: None,
            blocked: false,
            ranges: HashMap::default(),
            seen: HashSet::default(),
            cached_bytes: 0,
            cache_budget: 0,
            worker_levels: 0,
            fallbacks: 0,
            #[cfg(test)]
            fail_workers: false,
        }
    }

    /// A host for a worker thread of a multi-threaded level: it reads what this host has
    /// calculated so far and records its own results, to be merged back by
    /// [`PassHost::run_level`]. It never calculates a formula on demand (see
    /// [`PassHost::compute`]), so it holds no pending set.
    fn worker<'w>(&'w self, rng: &'w mut u64, cache_budget: usize) -> PassHost<'w> {
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
            names: HashMap::default(),
            base: Some(self),
            blocked: false,
            ranges: HashMap::default(),
            seen: HashSet::default(),
            cached_bytes: 0,
            cache_budget,
            worker_levels: 0,
            fallbacks: 0,
            #[cfg(test)]
            fail_workers: false,
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
fn spills_as_before(sheet: &gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, arr: &Array) -> bool {
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
    fn calculate(&mut self, k: Key) {
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
        self.in_progress.insert(k);
        self.depth += 1;
        let outer_taint = std::mem::replace(&mut self.taint, false);
        if self.iterate {
            self.stack.push(k);
        }
        let expr = std::sync::Arc::clone(expr);
        let v = {
            let mut ev = Evaluator::new(self, k.0, k.1);
            ev.value(&expr)
        };
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
    fn iterate_loops(&mut self, order: &[Key], max_iterations: u32, max_change: f64) -> u32 {
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
    /// definition reads one) is handed back and calculated here once the workers' results are
    /// merged, with the usual on-demand machinery. Formulas that draw random numbers run here
    /// too, from the one generator, in the same row-major order as one thread would draw (a
    /// draw inside a name's body hands the formula back the same way). If a thread can't start
    /// or fails, the level is calculated on this thread.
    fn run_level(&mut self, level: &[Key], threads: usize, parallel_min: usize) {
        if threads <= 1 || level.len() < parallel_min {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        let exprs = self.exprs;
        let (random, parallel): (Vec<Key>, Vec<Key>) = level.iter().partition(|k| exprs.get(k).is_some_and(|e| uses_random(e)));
        if parallel.is_empty() {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        #[cfg(test)]
        let fail = self.fail_workers;
        let seed = *self.rng;
        let t0 = prof_now();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let cache_budget = CACHE_BYTES_BUDGET / threads.max(1);
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
                let i = next.fetch_add(CHUNK, std::sync::atomic::Ordering::Relaxed);
                let Some(chunk) = parallel.get(i..parallel.len().min(i.saturating_add(CHUNK))) else { break };
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
            for _ in 0..threads.min(parallel.len().div_ceil(CHUNK)) {
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
        let mut deferred: Vec<Key> = Vec::new();
        for out in outs {
            for (k, v) in out.results {
                self.pending.remove(&k);
                self.results.insert(k, v);
            }
            self.spills.extend(out.spills);
            self.names.extend(out.names);
            self.used.extend(out.used);
            deferred.extend(out.deferred);
        }
        // In row-major order, as one thread would: random draws then come in the same sequence.
        deferred.extend(random);
        deferred.sort_by_key(|(s, c)| (*s, c.row, c.col));
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("  level of {}: {} handed back, {:.1} ms on workers", level.len(), deferred.len(), prof_now() - t0);
        }
        for k in deferred {
            self.calculate(k);
        }
    }
}

/// What a worker thread worked out on one level.
struct WorkerOut {
    results: HashMap<Key, Value>,
    spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Formulas that read a formula not done yet: the main thread calculates them.
    deferred: Vec<Key>,
    names: HashMap<(String, usize), Option<(Option<std::sync::Arc<Expr>>, Option<usize>)>>,
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
fn parallel_min() -> usize {
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
    /// The parsed definition, cached for the pass: the workbook's names don't change while it runs.
    fn name_expr(&mut self, name: &str, scope_sheet: usize) -> Option<(Option<std::sync::Arc<Expr>>, Option<usize>)> {
        let key = (name.to_ascii_lowercase(), scope_sheet);
        if let Some(hit) = self.names.get(&key).or_else(|| self.base.and_then(|b| b.names.get(&key))) {
            return hit.clone();
        }
        let found = self.wb.name(name, scope_sheet).map(|def| (gridcraft_formula::parse_name(&def.formula).ok().map(std::sync::Arc::new), def.scope));
        self.names.insert(key, found.clone());
        found
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
        // A cell covered by a spill computed in this pass. (Checked for emptiness first: this
        // runs once per cell of every range read, and the chained iterator isn't free.)
        if !self.spills.is_empty() || self.base.is_some_and(|b| !b.spills.is_empty()) {
            for (anchor, arr) in self.spills.iter().chain(self.base.into_iter().flat_map(|b| b.spills.iter())) {
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
        let mut names = NameCache::default();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (c, cell) in sheet.cells.iter() {
                if let Some(f) = &cell.formula
                    && let Some(e) = f.expr()
                {
                    let (areas, dynamic) = precedents_with(wb, si, &e, &mut names);
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
        let mut names = NameCache::default();
        for &k in changed {
            let formula = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.clone());
            match formula.and_then(|f| f.expr()) {
                Some(e) => {
                    let (areas, dynamic) = precedents_with(wb, k.0, &e, &mut names);
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
                seeds.extend(r.iter().map(|c| (k.0, c)));
            }
        }
        for (si, r) in &cleared {
            seeds.extend(r.iter().map(|c| (*si, c)));
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
        self.last_recalc_cells = 0;
        self.worker_levels = 0;
        self.worker_fallbacks = 0;
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
            let mut rng = self.rng;
            let tp = prof_now();
            // Iterative calculation reads previous values as it goes: one thread keeps that simple.
            #[cfg(test)]
            let threads = if wb.calc.iterative { 1 } else { self.threads.unwrap_or_else(|| thread_count(&wb.calc)) };
            #[cfg(not(test))]
            let threads = if wb.calc.iterative { 1 } else { thread_count(&wb.calc) };
            // Planned on one thread too, so the order the formulas are calculated in (which
            // cell of a cycle shows `#CIRC!`, the sequence RAND draws in, how many rounds a
            // chain of spills takes) is the same whatever the machine and the setting.
            #[cfg(test)]
            let plan = self.graph.levels_within(wb, &dirty, self.plan_budget);
            #[cfg(not(test))]
            let plan = self.graph.levels(wb, &dirty);
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some()
                && let Some(p) = &plan
            {
                eprintln!(
                    "  plan: {} levels over {} formulas, {} behind cycles, {} index work, {} threads, {:.1} ms",
                    p.bounds.len(),
                    p.leveled,
                    p.order.len() - p.leveled,
                    p.work(),
                    threads,
                    prof_now() - tp
                );
            }
            let te = prof_now();
            let (results, spills, cycles) = {
                let mut host = PassHost::new(wb, &exprs, &mut rng, now);
                host.iterate = wb.calc.iterative;
                #[cfg(test)]
                {
                    host.fail_workers = self.fail_workers;
                }
                match &plan {
                    Some(plan) => {
                        for level in plan.levels() {
                            host.run_level(level, threads, self.parallel_min);
                        }
                        for &k in plan.rest() {
                            host.calculate(k);
                        }
                    }
                    None => {
                        for k in &dirty {
                            host.calculate(*k);
                        }
                    }
                }
                if host.iterate && !host.members.is_empty() {
                    let order: Vec<Key> = match &plan {
                        Some(p) => p.levels().flatten().copied().chain(p.rest().iter().copied()).collect(),
                        None => dirty.clone(),
                    };
                    let passes = host.iterate_loops(&order, wb.calc.max_iterations, wb.calc.max_change);
                    if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                        eprintln!("  iterative calculation: {} passes over {} formulas", passes, host.tainted.len());
                    }
                }
                self.worker_levels += host.worker_levels;
                self.worker_fallbacks += host.fallbacks;
                (host.results, host.spills, host.cycles)
            };
            self.rng = rng;
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  eval {} formulas {:.1} ms", results.len(), prof_now() - te);
            }
            let tw = prof_now();
            self.last_recalc_cells += results.len();
            self.circular = cycles;
            // Write back.
            let mut spill_changes: Vec<Key> = Vec::new();
            let mut freed: Vec<(usize, RangeRef)> = Vec::new();
            // Whether a spill was laid out differently from before in this round.
            let mut laid_out = false;
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
                // An array that spills exactly as before changes nothing its dependents read, so
                // they aren't queued again. Otherwise two arrays that read each other's spill areas
                // queue each other in every round, up to the round limit.
                if let (Some(r), Some(arr)) = (old, new)
                    && spills_as_before(sheet, k.1, r, arr)
                {
                    continue;
                }
                laid_out = true;
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
                        // Not blocked while the pass ran (it would have been #SPILL! there), so
                        // blocked by another array of this pass: the formulas that read this one,
                        // at its anchor or over the area it would have spilled into, read #SPILL!
                        // or empty cells now.
                        let mut deps = Vec::new();
                        self.graph.dependents_of_range(k.0, range, &mut deps);
                        spill_changes.extend(deps.into_iter().filter(|d| d != k));
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
                    // Not the anchor itself (that would read its own array), unless its old array
                    // is gone or partly freed: its new formula may have read the old values, still
                    // laid out while it was evaluated.
                    let freed = match (old, sheet.spill_ranges.get(&k.1)) {
                        (Some(o), Some(n)) => !(n.contains(o.start) && n.contains(o.end)),
                        (_, None) => true,
                        (None, Some(_)) => false,
                    };
                    spill_changes.extend(deps.into_iter().filter(|d| d != k || freed));
                }
                if let Some(o) = old
                    && !sheet.spill_ranges.get(&k.1).is_some_and(|n| n.contains(o.start) && n.contains(o.end))
                {
                    freed.push((k.0, o));
                }
            }
            // Formulas with references only known while evaluating (INDIRECT, OFFSET) may have read
            // a spill area before it was laid out: they are evaluated once more after every round
            // that laid one out differently (a spill laid out in a later round changes what they
            // read as much as one from the first).
            if laid_out {
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

/// Current date-time as a serial (1900 system), from the system clock (browser clock on wasm).
/// UTC; the local offset isn't available without platform calls. Falls back to a fixed date only
/// when no clock is available.
pub fn now_serial() -> f64 {
    gridcraft_core::date::unix_now_ms().map_or(46302.5, gridcraft_core::date::serial_from_unix_ms)
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
    let mut rng = 0x1234_5678_9ABC_DEF0u64;
    let mut host = PassHost::new(wb, &exprs, &mut rng, now_serial());
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
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use gridcraft_core::{CellRef, RangeRef, Value};
    use gridcraft_model::{Cell, Formula, Workbook};

    use super::{Calc, HashMap, Host, Intervals, Key, PassHost};

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

    /// A formula at the nesting limit reading thousands of formulas not calculated yet suspends the
    /// evaluations in progress once, not once per formula read.
    #[test]
    fn wide_read_at_the_nesting_limit_suspends_once() {
        const WIDTH: u32 = 3000;
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        let mut put = |wb: &mut Workbook, at: String, text: String| {
            let at = CellRef::parse(&at).unwrap();
            wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(Formula::new(&text)));
            exprs.insert((0, at), std::sync::Arc::new(gridcraft_formula::parse(&text[1..]).unwrap()));
        };
        // A1 reads A2, ..., A64 (the 64th formula deep) reads a wide range of formulas.
        for r in 1..64 {
            put(&mut wb, format!("A{r}"), format!("=A{}+1", r + 1));
        }
        put(&mut wb, "A64".to_string(), format!("=SUM(B1:B{WIDTH})"));
        for r in 1..=WIDTH {
            put(&mut wb, format!("B{r}"), format!("={r}*2"));
        }
        let mut rng = 1u64;
        let mut host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        host.calculate((0, CellRef::parse("A1").unwrap()));
        let total = (WIDTH * (WIDTH + 1)) as f64;
        assert_eq!(host.results.get(&(0, CellRef::parse("A1").unwrap())), Some(&Value::Number(total + 63.0)));
        assert!(host.cycles.is_empty());
        assert!(host.suspensions <= 3, "{} suspensions", host.suspensions);
    }

    #[test]
    fn a_workers_copied_block_is_dropped_by_its_own_spill() {
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        let a2 = CellRef::parse("A2").unwrap();
        wb.sheet_mut(0).unwrap().cells.set(a2, Cell::formula(Formula::new("=SEQUENCE(1,200)")));
        exprs.insert((0, a2), std::sync::Arc::new(gridcraft_formula::parse("SEQUENCE(1,200)").unwrap()));
        let mut rng = 1u64;
        let host = PassHost::new(&wb, &exprs, &mut rng, 0.0);
        let mut rng2 = 1u64;
        let mut w = host.worker(&mut rng2, 1 << 20);
        let block = RangeRef::parse("B2:CW2").unwrap();
        // The first read only notes the (empty) block; the second copies it, and the copy
        // serves the reads after that.
        assert!(w.range_values(0, block).iter().all(Value::is_empty));
        assert_eq!((w.ranges.len(), w.seen.len(), w.cached_bytes), (0, 1, 0));
        assert!(w.range_values(0, block).iter().all(Value::is_empty));
        assert_eq!((w.ranges.len(), w.cached_bytes), (1, 100 * std::mem::size_of::<Value>()));
        // The worker's own spill lands in it: the copy goes, the next read sees the array.
        w.blocked = false;
        w.compute((0, a2));
        assert!(!w.blocked && w.spills.contains_key(&(0, a2)));
        assert!(w.ranges.is_empty() && w.cached_bytes == 0);
        let fresh = w.range_values(0, block);
        assert_eq!((fresh.first(), fresh.last()), (Some(&Value::Number(2.0)), Some(&Value::Number(101.0))));
        // A block holding a formula not done yet: the read is handed back, nothing is copied.
        let d1 = CellRef::parse("D1").unwrap();
        let mut wb2 = Workbook::new();
        wb2.sheet_mut(0).unwrap().cells.set(d1, Cell::formula(Formula::new("=5")));
        let mut exprs2 = HashMap::default();
        exprs2.insert((0, d1), std::sync::Arc::new(gridcraft_formula::parse("5").unwrap()));
        let mut rng3 = 1u64;
        let host2 = PassHost::new(&wb2, &exprs2, &mut rng3, 0.0);
        let mut rng4 = 1u64;
        let mut w2 = host2.worker(&mut rng4, 1 << 20);
        w2.depth = 1;
        let _ = w2.range_values(0, RangeRef::parse("B1:CW1").unwrap());
        let _ = w2.range_values(0, RangeRef::parse("B1:CW1").unwrap());
        assert!(w2.blocked && w2.ranges.is_empty());
        // A block of long text past the worker's share of the byte budget is not copied.
        let mut wb3 = Workbook::new();
        for col in 0..100u32 {
            wb3.sheet_mut(0).unwrap().cells.set(CellRef::new(0, col), Cell::value(Value::Text("x".repeat(20_000).into())));
        }
        let exprs3 = HashMap::default();
        let mut rng5 = 1u64;
        let host3 = PassHost::new(&wb3, &exprs3, &mut rng5, 0.0);
        let mut rng6 = 1u64;
        let mut w3 = host3.worker(&mut rng6, 1 << 20);
        let wide = RangeRef::parse("A1:CV1").unwrap();
        let _ = w3.range_values(0, wide);
        let _ = w3.range_values(0, wide);
        assert!(w3.ranges.is_empty() && w3.cached_bytes == 0);
    }

    /// For tests of `PassHost::visit_range`, which can't be reached from outside this module: the
    /// values it hands over for `range` while the formulas in `pending` (cell, formula) have not been
    /// calculated yet, or `None` when it declines the block.
    pub(crate) fn probe_visit_range(wb: &Workbook, pending: &[(&str, &str)], range: RangeRef) -> Option<Vec<Value>> {
        let mut exprs = HashMap::default();
        for (at, formula) in pending {
            let expr = gridcraft_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).ok()?;
            exprs.insert((0, CellRef::parse(at)?), std::sync::Arc::new(expr));
        }

        // A cell in `pending` is calculated the first time something reads it, which is the state
        // of a real pass partway through; `visit_range` has to cope with the formulas it reads
        // being calculated under it.
        let mut rng = 1u64;
        let mut host = PassHost::new(wb, &exprs, &mut rng, 0.0);

        let mut seen = Vec::new();
        host.visit_range(0, range, &mut |v| {
            seen.push(v.clone());
            true
        })
        .then_some(seen)
    }
}
