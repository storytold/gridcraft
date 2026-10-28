use super::RangeRef;
use crate::eval::Area;
use gridcraft_core::CellRef;
use gridcraft_model::Workbook;
use std::collections::BTreeSet;
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

pub(super) type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub(super) type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

pub type Key = (usize, CellRef);

#[derive(Clone, Debug, Default)]
pub(super) struct Node {
    pub(super) areas: Vec<Area>,
    pub(super) dynamic: bool,
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
pub(super) struct Intervals<T> {
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
    pub(super) fn is_empty(&self) -> bool {
        self.items.len() == self.dead_count && self.pending.is_empty()
    }

    pub(super) fn insert(&mut self, r0: u32, r1: u32, t: T) {
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

    pub(super) fn remove(&mut self, r0: u32, r1: u32, t: T) {
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
    pub(super) fn settle(&mut self) {
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
    pub(super) fn query(&self, a: u32, b: u32, f: &mut dyn FnMut(&T)) -> usize {
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
    pub(super) nodes: HashMap<Key, Node>,
    /// single cell → dependents
    cell_deps: HashMap<Key, HashSet<Key>>,
    /// (sheet, col) → row intervals of the ranges (up to `WIDE` columns) that cover the column
    col_deps: HashMap<(usize, u32), Intervals<Key>>,
    /// sheet → row intervals of wider ranges, with their columns: (first, last, dependent)
    wide_deps: HashMap<usize, Intervals<(u32, u32, Key)>>,
    pub(super) dynamic: HashSet<Key>,
    /// Range indexes changed since the last [`Graph::settle`].
    unsettled_cols: HashSet<(usize, u32)>,
    unsettled_wide: HashSet<usize>,
}

/// A formula's precedents split by shape: single cells, column intervals and wide ranges.
type Entries = (Vec<Key>, Vec<(usize, u32, u32, u32)>, Vec<(usize, RangeRef)>);

impl Graph {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Index entries of a formula's precedents, without duplicates: single cells, column
    /// intervals and wide ranges.
    fn entries(areas: &[Area]) -> Entries {
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

    pub(super) fn insert(&mut self, k: Key, node: Node) {
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

    pub(super) fn remove(&mut self, k: Key) {
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
    pub(super) fn settle(&mut self) {
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

    /// Whether the formula at `k` is evaluated on every recalc (its references aren't known
    /// statically: INDIRECT, OFFSET, tables, unresolvable names).
    pub fn is_dynamic(&self, k: Key) -> bool {
        self.dynamic.contains(&k)
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
    /// its spill range, so they come after it and read the new array rather than the old one,
    /// and a formula reading an area waits for the nearest dirty formula above it in each of its
    /// columns, which spills into it if it returns an array taller than it was (or new), as it
    /// would come before the reader in row-major order.
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
        let DirtyIndex { index, keys, mut cols, sheet_cols, pos } = DirtyIndex::new(dirty)?;
        let n = keys.len();
        let budget = n.saturating_mul(BUDGET_PER_FORMULA).saturating_add(1 << 20).min(cap.unwrap_or(usize::MAX));
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
        // The nearest dirty formula above an area, in each of its columns, spills into it if it
        // returns an array tall enough and nothing stands between them (one further up would be
        // blocked by it): the reader waits for it, as it would come after it in row-major order,
        // so a chain of spills, each sized by the area of the one before, settles in one round.
        // Held back like a spill reader. A cell under the formula or at the top of the area that
        // holds a value or a formula would block the spill, so the reader doesn't wait then (a
        // sheet of formulas has no such waits, nor loops of them). Calls `found` with each.
        #[allow(clippy::too_many_arguments)]
        fn anchors_above(
            wb: &Workbook,
            cols: &HashMap<(usize, u32), ColIndex>,
            sheet_cols: &HashMap<usize, Vec<u32>>,
            keys: &[Key],
            areas: &[Area],
            i: usize,
            work: &mut usize,
            found: &mut dyn FnMut(u32),
        ) {
            for a in areas {
                let r0 = a.range.start.row;
                let sheet = wb.sheet(a.sheet);
                let occupied =
                    |at: CellRef| sheet.and_then(|sh| sh.cells.get(at)).is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some());
                visit(cols, sheet_cols, a, work, &mut |c| {
                    let above = c.rows.partition_point(|r| *r < r0);
                    // A dirty formula at the top of the area fills it: nothing spills into it.
                    if c.rows.get(above) == Some(&r0) {
                        return;
                    }
                    if let Some(&j) = above.checked_sub(1).and_then(|p| c.keys.get(p))
                        && j as usize != i
                        && let Some(anchor) = keys.get(j as usize).filter(|anchor| anchor.0 == a.sheet)
                        && !occupied(CellRef::new(anchor.1.row.saturating_add(1), anchor.1.col))
                        && !occupied(CellRef::new(r0, anchor.1.col))
                    {
                        found(j);
                    }
                });
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
            anchors_above(wb, &cols, &sheet_cols, &keys, areas, i, &mut work, &mut |j| {
                if let Some(w) = spill_waits.get_mut(i) {
                    *w += 1;
                    if let Some(r) = spill_release.get_mut(j as usize) {
                        r.push(i as u32);
                    }
                }
            });
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
            // have completed for the count to have reached zero. A reader of overlapping areas
            // (two lookups over nested tables) counts a formula in both, so its count can lie
            // past what will ever complete: when nothing else is ready, all of them are counted
            // again, so such a reader is leveled rather than left for after the levels.
            let stalled = next.is_empty();
            let mut later = Vec::new();
            while let Some(&std::cmp::Reverse((due, i))) = big.peek() {
                if due > completed && !stalled {
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
                    later.push(std::cmp::Reverse((completed.saturating_add(left.max(1)), i)));
                }
            }
            big.extend(later);
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

// `isolate_lowest_one` needs Rust 1.97; the workspace builds on 1.95.
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
    pub(super) order: Vec<Key>,
    /// End of each level in `order`.
    pub(super) bounds: Vec<usize>,
    /// Where the formulas on or behind cycles start in `order`.
    pub(super) leveled: usize,
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

/// The dirty formulas of a plan by their place in it, and by (sheet, column), rows sorted, with
/// each formula's place; per sheet, the columns that hold any (for areas wider than that list);
/// and where each formula sits in its column's lists.
struct DirtyIndex {
    index: HashMap<Key, u32>,
    keys: Vec<Key>,
    cols: HashMap<(usize, u32), ColIndex>,
    sheet_cols: HashMap<usize, Vec<u32>>,
    pos: Vec<((usize, u32), usize)>,
}

impl DirtyIndex {
    /// `None` when there are more dirty formulas than a `u32` counts.
    fn new(dirty: &[Key]) -> Option<DirtyIndex> {
        let mut index: HashMap<Key, u32> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
        let mut keys: Vec<Key> = Vec::with_capacity(dirty.len());
        for &k in dirty {
            if let std::collections::hash_map::Entry::Vacant(e) = index.entry(k) {
                let i = u32::try_from(keys.len()).ok()?;
                e.insert(i);
                keys.push(k);
            }
        }
        // The dirty formulas by (sheet, column), rows sorted, with the formula's index; and per
        // sheet the columns that hold any, for areas wider than that list.
        let n = keys.len();
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
        Some(DirtyIndex { index, keys, cols, sheet_cols, pos })
    }
}
