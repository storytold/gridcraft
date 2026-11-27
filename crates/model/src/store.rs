//! Copy-on-write sparse cell storage.
//!
//! Cells live in bands of 64 rows, each band behind an `Arc`. Cloning a store (an undo snapshot)
//! copies only the band pointers; editing a cell copies just its band.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, RangeRef};
use serde::{Deserialize, Serialize, Serializer};

use crate::cell::Cell;

const BAND: u32 = 64;
const COL_BITS: u32 = 14;
const COL_MASK: u32 = (1 << COL_BITS) - 1;
/// Non-tail mid-band insert or single-cell removal threshold at or above which a flat band
/// promotes to a `BTreeMap` so reverse, interleaved, or scattered single-cell mutations stay
/// O(n log n) instead of O(n²). Sequential appends (XLSX import) always push in O(1) and remain
/// flat at any width.
const FLAT_MAX_MUTATE: usize = 256;

#[inline]
fn pack_key(row: u32, col: u32) -> u32 {
    ((row % BAND) << COL_BITS) | col
}

#[inline]
fn unpack_local_row(k: u32) -> u32 {
    k >> COL_BITS
}

#[inline]
fn unpack_col(k: u32) -> u32 {
    k & COL_MASK
}

#[inline]
fn next_row_key(k: u32) -> u32 {
    // This boundary is allowed to be one past the final packed key of a band.
    (unpack_local_row(k) + 1) << COL_BITS
}

#[inline]
fn flat_lower_bound(keys: &[u32], target: u32) -> usize {
    if keys.first().is_some_and(|&k| k < target) { keys.partition_point(|&k| k < target) } else { 0 }
}

/// Contiguous span bounded by the inclusive packed keys `lo` and `hi`.
fn flat_key_span(keys: &[u32], lo: u32, hi: u32) -> Range<usize> {
    let start = flat_lower_bound(keys, lo);
    let end = if keys.last().is_some_and(|&k| k > hi) { start + flat_lower_bound(&keys[start..], hi + 1) } else { keys.len() };
    start..end
}

fn has_value(cell: &Cell) -> bool {
    cell.has_content()
}

/// One 64-row slice of a [`CellStore`].
///
/// Ordinary bands use a compact Structure-of-Arrays flat layout (`keys` + `cells`, 4 bytes per key,
/// zero per-row heap allocations or B-tree slack). Bands that hold off-grid columns (`>= MAX_COLS`,
/// before `rebuild`) or receive non-tail mid-band inserts or single-cell removals at or above
/// [`FLAT_MAX_MUTATE`] cells promote to a nested `BTreeMap`.
#[derive(Clone, Debug)]
enum Band {
    Flat { keys: Vec<u32>, cells: Vec<Cell> },
    Tree(BTreeMap<u32, BTreeMap<u32, Cell>>),
}

impl Default for Band {
    fn default() -> Self {
        Band::Flat { keys: Vec::new(), cells: Vec::new() }
    }
}

impl PartialEq for Band {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Band::Flat { keys: ka, cells: ca }, Band::Flat { keys: kb, cells: cb }) => ka == kb && ca == cb,
            (Band::Tree(a), Band::Tree(b)) => a == b,
            (Band::Flat { keys, cells }, Band::Tree(tree)) | (Band::Tree(tree), Band::Flat { keys, cells }) => {
                let tree_len: usize = tree.values().map(BTreeMap::len).sum();
                if keys.len() != tree_len {
                    return false;
                }
                let mut idx = 0usize;
                for (&row, cols) in tree {
                    let local = row % BAND;
                    for (&col, cell) in cols {
                        if col >= MAX_COLS {
                            return false;
                        }
                        let expected = (local << COL_BITS) | col;
                        if keys.get(idx) != Some(&expected) || cells.get(idx) != Some(cell) {
                            return false;
                        }
                        idx += 1;
                    }
                }
                idx == keys.len()
            }
        }
    }
}

impl Band {
    fn is_empty(&self) -> bool {
        match self {
            Band::Flat { keys, .. } => keys.is_empty(),
            Band::Tree(rows) => rows.is_empty(),
        }
    }

    fn get(&self, c: CellRef) -> Option<&Cell> {
        match self {
            Band::Flat { keys, cells } => {
                if c.col >= MAX_COLS {
                    return None;
                }
                let i = keys.binary_search(&pack_key(c.row, c.col)).ok()?;
                cells.get(i)
            }
            Band::Tree(rows) => rows.get(&c.row)?.get(&c.col),
        }
    }

    fn contains(&self, c: CellRef) -> bool {
        match self {
            Band::Flat { keys, .. } => c.col < MAX_COLS && keys.binary_search(&pack_key(c.row, c.col)).is_ok(),
            Band::Tree(rows) => rows.get(&c.row).is_some_and(|r| r.contains_key(&c.col)),
        }
    }

    fn get_mut(&mut self, c: CellRef) -> Option<&mut Cell> {
        match self {
            Band::Flat { keys, cells } => {
                if c.col >= MAX_COLS {
                    return None;
                }
                let i = keys.binary_search(&pack_key(c.row, c.col)).ok()?;
                cells.get_mut(i)
            }
            Band::Tree(rows) => rows.get_mut(&c.row)?.get_mut(&c.col),
        }
    }

    fn promote_to_tree(&mut self, band_key: u32) {
        if let Band::Flat { keys, cells } = self {
            let base_row = band_key.saturating_mul(BAND);
            let mut rows: BTreeMap<u32, BTreeMap<u32, Cell>> = BTreeMap::new();
            for (k, cell) in std::mem::take(keys).into_iter().zip(std::mem::take(cells)) {
                rows.entry(base_row + unpack_local_row(k)).or_default().insert(unpack_col(k), cell);
            }
            *self = Band::Tree(rows);
        }
    }

    /// Inserts `cell` at `c`; returns `true` if a new entry was inserted (`false` on overwrite).
    fn insert(&mut self, band_key: u32, c: CellRef, cell: Cell) -> bool {
        if c.col >= MAX_COLS {
            self.promote_to_tree(band_key);
        }
        match self {
            Band::Flat { keys, cells } => {
                let k = pack_key(c.row, c.col);
                if keys.last().is_none_or(|&last| k > last) {
                    keys.push(k);
                    cells.push(cell);
                    return true;
                }
                match keys.binary_search(&k) {
                    Ok(i) => {
                        if let Some(slot) = cells.get_mut(i) {
                            *slot = cell;
                        }
                        false
                    }
                    Err(i) => {
                        if keys.len() >= FLAT_MAX_MUTATE {
                            self.promote_to_tree(band_key);
                            if let Band::Tree(rows) = self { rows.entry(c.row).or_default().insert(c.col, cell).is_none() } else { false }
                        } else {
                            keys.insert(i, k);
                            cells.insert(i, cell);
                            true
                        }
                    }
                }
            }
            Band::Tree(rows) => rows.entry(c.row).or_default().insert(c.col, cell).is_none(),
        }
    }

    fn remove(&mut self, band_key: u32, c: CellRef) -> Option<Cell> {
        if let Band::Flat { keys, cells } = self {
            if c.col >= MAX_COLS {
                return None;
            }
            let i = keys.binary_search(&pack_key(c.row, c.col)).ok()?;
            if i + 1 < keys.len() && keys.len() >= FLAT_MAX_MUTATE {
                self.promote_to_tree(band_key);
            } else {
                keys.remove(i);
                return Some(cells.remove(i));
            }
        }
        match self {
            Band::Tree(rows) => {
                let row = rows.get_mut(&c.row)?;
                let old = row.remove(&c.col);
                if row.is_empty() {
                    rows.remove(&c.row);
                }
                old
            }
            Band::Flat { .. } => None,
        }
    }

    fn first_row(&self, band_key: u32) -> Option<u32> {
        match self {
            Band::Flat { keys, .. } => keys.first().map(|&k| band_key.saturating_mul(BAND) + unpack_local_row(k)),
            Band::Tree(rows) => rows.keys().next().copied(),
        }
    }

    fn last_row(&self, band_key: u32) -> Option<u32> {
        match self {
            Band::Flat { keys, .. } => keys.last().map(|&k| band_key.saturating_mul(BAND) + unpack_local_row(k)),
            Band::Tree(rows) => rows.keys().next_back().copied(),
        }
    }

    fn col_bounds(&self) -> Option<(u32, u32)> {
        match self {
            Band::Flat { keys, .. } => {
                let mut remaining = keys.as_slice();
                let last = *remaining.last()?;
                let mut lo = COL_MASK;
                let mut hi = unpack_col(last);
                // Columns are sorted within each populated row; jump to its last key.
                while let Some((&first, tail)) = remaining.split_first() {
                    lo = lo.min(unpack_col(first));
                    let next_row = next_row_key(first);
                    if last < next_row || (lo == 0 && hi == COL_MASK) {
                        break;
                    }
                    let (row_tail, later_rows) = tail.split_at(flat_lower_bound(tail, next_row));
                    hi = hi.max(unpack_col(row_tail.last().copied().unwrap_or(first)));
                    remaining = later_rows;
                }
                Some((lo, hi))
            }
            Band::Tree(rows) => {
                let mut bounds: Option<(u32, u32)> = None;
                for cols in rows.values() {
                    if let (Some(&a), Some(&b)) = (cols.keys().next(), cols.keys().next_back()) {
                        bounds = Some(match bounds {
                            Some((lo, hi)) => (lo.min(a), hi.max(b)),
                            None => (a, b),
                        });
                    }
                }
                bounds
            }
        }
    }

    fn has_any_in(&self, band_key: u32, r: RangeRef) -> bool {
        self.iter_range(band_key, r).next().is_some()
    }

    fn into_rows(self, band_key: u32) -> BTreeMap<u32, BTreeMap<u32, Cell>> {
        match self {
            Band::Tree(rows) => rows,
            Band::Flat { keys, cells } => {
                let base_row = band_key.saturating_mul(BAND);
                let mut rows: BTreeMap<u32, BTreeMap<u32, Cell>> = BTreeMap::new();
                for (k, cell) in keys.into_iter().zip(cells) {
                    rows.entry(base_row + unpack_local_row(k)).or_default().insert(unpack_col(k), cell);
                }
                rows
            }
        }
    }

    fn iter_all(&self, band_key: u32) -> BandIter<'_> {
        match self {
            Band::Flat { keys, cells } => {
                BandIter::Flat { base_row: band_key.saturating_mul(BAND), keys, cells, idx: 0, c0: 0, c1: MAX_COLS - 1, seek: false }
            }
            Band::Tree(rows) => BandIter::Tree { rows: rows.range(0..=u32::MAX), cur_row: 0, cols: None, c0: 0, c1: u32::MAX },
        }
    }

    fn iter_range(&self, band_key: u32, r: RangeRef) -> BandIter<'_> {
        match self {
            Band::Flat { keys, cells } => {
                let base_row = band_key.saturating_mul(BAND);
                let r0 = r.start.row.max(base_row);
                let r1 = r.end.row.min(base_row.saturating_add(BAND - 1));
                let c0 = r.start.col;
                let c1 = r.end.col.min(MAX_COLS - 1);
                if r0 > r1 || c0 > c1 {
                    return BandIter::Flat { base_row, keys: &[], cells: &[], idx: 0, c0: 0, c1: 0, seek: false };
                }
                let span = flat_key_span(keys, pack_key(r0, c0), pack_key(r1, c1));
                // Broad selections keep a linear scan; narrow ones seek only on a miss.
                let seek = c1 - c0 < MAX_COLS / 2;
                BandIter::Flat { base_row, keys: &keys[span.clone()], cells: &cells[span], idx: 0, c0, c1, seek }
            }
            Band::Tree(rows) => BandIter::Tree { rows: rows.range(r.start.row..=r.end.row), cur_row: 0, cols: None, c0: r.start.col, c1: r.end.col },
        }
    }

    fn row_iter(&self, row: u32, c0: u32, c1: u32) -> RowIter<'_> {
        match self {
            Band::Flat { keys, cells } => {
                if c0 > c1 || c0 >= MAX_COLS || keys.is_empty() {
                    return RowIter::Flat { keys: &[], cells: &[], idx: 0 };
                }
                let span = flat_key_span(keys, pack_key(row, c0), pack_key(row, c1.min(MAX_COLS - 1)));
                RowIter::Flat { keys: &keys[span.clone()], cells: &cells[span], idx: 0 }
            }
            Band::Tree(rows) => match rows.get(&row) {
                Some(cols) => RowIter::Tree(cols.range(c0..=c1)),
                None => RowIter::Flat { keys: &[], cells: &[], idx: 0 },
            },
        }
    }

    /// Merges strictly ascending `(packed_key, cell)` entries (all `col < MAX_COLS`) into a
    /// `Band::Flat` in one O(n + m) linear pass without promoting to `Band::Tree`. Blank cells
    /// clear any existing entry at their key (matching `CellStore::set`).
    fn merge_flat_sorted(&mut self, incoming: impl IntoIterator<Item = (u32, Cell)>, len: &mut usize) {
        let Band::Flat { keys: bkeys, cells: bcells } = self else { return };
        let mut inc = incoming.into_iter().peekable();
        if inc.peek().is_none() {
            return;
        }
        if bkeys.last().is_none_or(|&last| inc.peek().is_some_and(|&(ik, _)| ik > last)) {
            for (k, cell) in inc {
                if !cell.is_blank() {
                    bkeys.push(k);
                    bcells.push(cell);
                    *len += 1;
                }
            }
            return;
        }
        let old_keys = std::mem::take(bkeys);
        let old_cells = std::mem::take(bcells);
        let cap = old_keys.len() + inc.size_hint().0;
        let mut new_keys = Vec::with_capacity(cap);
        let mut new_cells = Vec::with_capacity(cap);
        let mut old = old_keys.into_iter().zip(old_cells).peekable();
        loop {
            match (old.peek().map(|(k, _)| *k), inc.peek().map(|(k, _)| *k)) {
                (Some(ok), Some(ik)) if ok < ik => {
                    if let Some((k, c)) = old.next() {
                        new_keys.push(k);
                        new_cells.push(c);
                    }
                }
                (Some(ok), Some(ik)) if ok > ik => {
                    if let Some((k, c)) = inc.next()
                        && !c.is_blank()
                    {
                        new_keys.push(k);
                        new_cells.push(c);
                        *len += 1;
                    }
                }
                (Some(_), Some(_)) => {
                    let _ = old.next();
                    if let Some((k, c)) = inc.next() {
                        if c.is_blank() {
                            *len -= 1;
                        } else {
                            new_keys.push(k);
                            new_cells.push(c);
                        }
                    }
                }
                (Some(_), None) => {
                    for (k, c) in old {
                        new_keys.push(k);
                        new_cells.push(c);
                    }
                    break;
                }
                (None, Some(_)) => {
                    for (k, c) in inc {
                        if !c.is_blank() {
                            new_keys.push(k);
                            new_cells.push(c);
                            *len += 1;
                        }
                    }
                    break;
                }
                (None, None) => break,
            }
        }
        *bkeys = new_keys;
        *bcells = new_cells;
    }
}

enum BandIter<'a> {
    Flat {
        base_row: u32,
        keys: &'a [u32],
        cells: &'a [Cell],
        idx: usize,
        c0: u32,
        c1: u32,
        seek: bool,
    },
    Tree {
        rows: std::collections::btree_map::Range<'a, u32, BTreeMap<u32, Cell>>,
        cur_row: u32,
        cols: Option<std::collections::btree_map::Range<'a, u32, Cell>>,
        c0: u32,
        c1: u32,
    },
}

impl<'a> Iterator for BandIter<'a> {
    type Item = (CellRef, &'a Cell);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BandIter::Flat { base_row, keys, cells, idx, c0, c1, seek } => {
                while let Some(&k) = keys.get(*idx) {
                    let i = *idx;
                    *idx += 1;
                    let col = unpack_col(k);
                    if col >= *c0 && col <= *c1 {
                        return cells.get(i).map(|cell| (CellRef::new(*base_row + unpack_local_row(k), col), cell));
                    }
                    if *seek {
                        let target = if col < *c0 { k - col + *c0 } else { next_row_key(k) + *c0 };
                        *idx += flat_lower_bound(&keys[*idx..], target);
                    }
                }
                None
            }
            BandIter::Tree { rows, cur_row, cols, c0, c1 } => loop {
                if let Some(col_it) = cols
                    && let Some((&col, cell)) = col_it.next()
                {
                    return Some((CellRef::new(*cur_row, col), cell));
                }
                let (&row, map) = rows.next()?;
                *cur_row = row;
                *cols = Some(map.range(*c0..=*c1));
            },
        }
    }
}

enum RowIter<'a> {
    Flat { keys: &'a [u32], cells: &'a [Cell], idx: usize },
    Tree(std::collections::btree_map::Range<'a, u32, Cell>),
}

impl<'a> Iterator for RowIter<'a> {
    type Item = (u32, &'a Cell);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RowIter::Flat { keys, cells, idx } => {
                let i = *idx;
                let k = *keys.get(i)?;
                let cell = cells.get(i)?;
                *idx = i + 1;
                Some((unpack_col(k), cell))
            }
            RowIter::Tree(it) => it.next().map(|(&c, cell)| (c, cell)),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SavedBand {
    rows: BTreeMap<u32, BTreeMap<u32, Cell>>,
}

#[derive(Deserialize)]
struct SavedCellStore {
    bands: BTreeMap<u32, SavedBand>,
    len: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(from = "SavedCellStore")]
pub struct CellStore {
    bands: BTreeMap<u32, Arc<Band>>,
    len: usize,
}

impl From<SavedCellStore> for CellStore {
    fn from(saved: SavedCellStore) -> Self {
        let mut bands = BTreeMap::new();
        for (band_key, sb) in saved.bands {
            let can_flat = sb.rows.iter().all(|(&r, cols)| r / BAND == band_key && cols.keys().all(|&c| c < MAX_COLS));
            let band = if can_flat {
                let cap: usize = sb.rows.values().map(BTreeMap::len).sum();
                let mut keys = Vec::with_capacity(cap);
                let mut cells = Vec::with_capacity(cap);
                for (row, cols) in sb.rows {
                    for (col, cell) in cols {
                        keys.push(pack_key(row, col));
                        cells.push(cell);
                    }
                }
                Band::Flat { keys, cells }
            } else {
                Band::Tree(sb.rows)
            };
            bands.insert(band_key, Arc::new(band));
        }
        CellStore { bands, len: saved.len }
    }
}

impl Serialize for CellStore {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        struct BorrowedBand<'a>(u32, &'a Band);
        impl Serialize for BorrowedBand<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut s = serializer.serialize_struct("Band", 1)?;
                match self.1 {
                    Band::Tree(rows) => s.serialize_field("rows", rows)?,
                    Band::Flat { keys, cells } => {
                        let base_row = self.0.saturating_mul(BAND);
                        let mut rows: BTreeMap<u32, BTreeMap<u32, &Cell>> = BTreeMap::new();
                        for (&k, cell) in keys.iter().zip(cells.iter()) {
                            rows.entry(base_row + unpack_local_row(k)).or_default().insert(unpack_col(k), cell);
                        }
                        s.serialize_field("rows", &rows)?;
                    }
                }
                s.end()
            }
        }
        let borrowed: BTreeMap<u32, BorrowedBand<'_>> = self.bands.iter().map(|(&k, b)| (k, BorrowedBand(k, b))).collect();
        let mut st = serializer.serialize_struct("CellStore", 2)?;
        st.serialize_field("bands", &borrowed)?;
        st.serialize_field("len", &self.len)?;
        st.end()
    }
}

impl CellStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Trims spare Vec capacity on unshared flat bands (useful right after bulk file import).
    pub fn shrink_to_fit(&mut self) {
        for band in self.bands.values_mut() {
            if let Some(Band::Flat { keys, cells }) = Arc::get_mut(band) {
                keys.shrink_to_fit();
                cells.shrink_to_fit();
            }
        }
    }
    /// Unique populated row indices in strictly ascending order, without collecting all cells or sorting.
    pub fn row_indices(&self) -> Vec<u32> {
        let mut rows = Vec::new();
        for (&band_key, band) in &self.bands {
            match band.as_ref() {
                Band::Flat { keys, .. } => {
                    let base = band_key.saturating_mul(BAND);
                    let mut last = u32::MAX;
                    for &k in keys {
                        let lr = unpack_local_row(k);
                        if lr != last {
                            last = lr;
                            rows.push(base + lr);
                        }
                    }
                }
                Band::Tree(map) => rows.extend(map.keys().copied()),
            }
        }
        rows
    }
    pub fn get(&self, c: CellRef) -> Option<&Cell> {
        self.bands.get(&(c.row / BAND))?.get(c)
    }
    /// Mutable access (copies the band if shared).
    pub fn get_mut(&mut self, c: CellRef) -> Option<&mut Cell> {
        let band = self.bands.get_mut(&(c.row / BAND))?;
        if !band.contains(c) {
            return None;
        }
        Arc::make_mut(band).get_mut(c)
    }
    /// Stores a cell; a default (empty, unstyled) cell is removed instead.
    pub fn set(&mut self, c: CellRef, cell: Cell) {
        if cell.is_blank() {
            self.remove(c);
            return;
        }
        let band_key = c.row / BAND;
        let band = Arc::make_mut(self.bands.entry(band_key).or_default());
        if band.insert(band_key, c, cell) {
            self.len += 1;
        }
    }
    /// Puts every cell in the band its row says and recounts the length — serialized band keys
    /// and the cached count are derived state, and the ones a file carries are not trusted. Cells
    /// outside the sheet grid are dropped; returns how many (#1008).
    pub fn rebuild(&mut self) -> usize {
        let mut out = CellStore::new();
        let mut dropped = 0;
        for (band_key, band) in std::mem::take(&mut self.bands) {
            for (at, cell) in band.iter_all(band_key) {
                if at.is_valid() {
                    out.set(at, cell.clone());
                } else {
                    dropped += 1;
                }
            }
        }
        *self = out;
        dropped
    }

    pub fn remove(&mut self, c: CellRef) -> Option<Cell> {
        let key = c.row / BAND;
        let band = self.bands.get_mut(&key)?;
        if !band.contains(c) {
            return None;
        }
        let b = Arc::make_mut(band);
        let old = b.remove(key, c);
        if b.is_empty() {
            self.bands.remove(&key);
        }
        if old.is_some() {
            self.len -= 1;
        }
        old
    }
    /// Cells in `r`, row-major. Efficient for sparse data: skips empty bands and rows.
    pub fn iter_range(&self, r: RangeRef) -> impl Iterator<Item = (CellRef, &Cell)> + '_ {
        let (b0, b1) = (r.start.row / BAND, r.end.row / BAND);
        self.bands.range(b0..=b1).flat_map(move |(&band_key, band)| band.iter_range(band_key, r))
    }
    pub fn iter(&self) -> impl Iterator<Item = (CellRef, &Cell)> + '_ {
        self.bands.iter().flat_map(|(&band_key, band)| band.iter_all(band_key))
    }
    /// Cells of one row in `c0..=c1`.
    pub fn row(&self, row: u32, c0: u32, c1: u32) -> impl Iterator<Item = (u32, &Cell)> + '_ {
        self.bands.get(&(row / BAND)).into_iter().flat_map(move |b| b.row_iter(row, c0, c1))
    }
    /// Smallest range containing every stored cell, `None` when empty.
    pub fn used_range(&self) -> Option<RangeRef> {
        let (&first_k, first_band) = self.bands.iter().next()?;
        let (&last_k, last_band) = self.bands.iter().next_back()?;
        let r0 = first_band.first_row(first_k)?;
        let r1 = last_band.last_row(last_k)?;
        let mut cols: Option<(u32, u32)> = None;
        for band in self.bands.values() {
            if let Some((a, b)) = band.col_bounds() {
                cols = Some(match cols {
                    Some((c0, c1)) => (c0.min(a), c1.max(b)),
                    None => (a, b),
                });
            }
        }
        let (c0, c1) = cols?;
        Some(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }
    /// Last stored row in column `col` at or above `below_or_at` (for Ctrl+arrow navigation).
    pub fn has(&self, c: CellRef) -> bool {
        self.get(c).is_some_and(has_value)
    }
    /// Last row (inclusive, capped at `cap`) of the maximal run of contiguous rows at or after
    /// `from.row` that store a cell in `from.col` with a non-empty *value* in `from.col`. `None`
    /// when `from` itself doesn't qualify.
    pub fn contiguous_value_run(&self, from: CellRef, cap: u32) -> Option<u32> {
        let mut last = None;
        let mut expect = from.row;
        while expect <= cap {
            let band_key = expect / BAND;
            let Some(band) = self.bands.get(&band_key) else { break };
            let band_hi = band_key.saturating_mul(BAND).saturating_add(BAND - 1).min(cap);
            let mut advanced = false;
            match band.as_ref() {
                Band::Flat { keys, cells } => {
                    if from.col >= MAX_COLS {
                        return last;
                    }
                    let mut idx = keys.partition_point(|&k| k < pack_key(expect, from.col));
                    while expect <= band_hi {
                        let want = pack_key(expect, from.col);
                        if idx < keys.len() && keys[idx] < want {
                            idx += keys[idx..].partition_point(|&k| k < want);
                        }
                        match keys.get(idx) {
                            Some(&k) if k == want && !cells.get(idx).is_none_or(|c| c.value.is_empty()) => {
                                last = Some(expect);
                                expect += 1;
                                idx += 1;
                                advanced = true;
                            }
                            _ => return last,
                        }
                    }
                }
                Band::Tree(rows) => {
                    for (r, cols) in rows.range(expect..=band_hi) {
                        if *r != expect || cols.get(&from.col).is_none_or(|c| c.value.is_empty()) {
                            return last;
                        }
                        last = Some(expect);
                        expect += 1;
                        advanced = true;
                    }
                }
            }
            if !advanced {
                break;
            }
        }
        last
    }
    /// Positions whose cells differ between `self` and `other` (stored on one side only, or not
    /// equal). Bands both stores share are skipped, so comparing an edited copy with the store
    /// it was cloned from only visits the bands the edit touched.
    pub fn diff(&self, other: &CellStore) -> Vec<CellRef> {
        let empty = Band::default();
        let mut keys: Vec<u32> = self.bands.keys().chain(other.bands.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        let mut out = Vec::new();
        for k in keys {
            let (a, b) = (self.bands.get(&k), other.bands.get(&k));
            if let (Some(a), Some(b)) = (a, b)
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            let (a, b) = (a.map_or(&empty, |x| x), b.map_or(&empty, |x| x));
            let mut ia = a.iter_all(k).peekable();
            let mut ib = b.iter_all(k).peekable();
            loop {
                match (ia.peek(), ib.peek()) {
                    (Some(&(ca, cell_a)), Some(&(cb, cell_b))) => match ca.cmp(&cb) {
                        std::cmp::Ordering::Less => {
                            out.push(ca);
                            ia.next();
                        }
                        std::cmp::Ordering::Greater => {
                            out.push(cb);
                            ib.next();
                        }
                        std::cmp::Ordering::Equal => {
                            if cell_a != cell_b {
                                out.push(ca);
                            }
                            ia.next();
                            ib.next();
                        }
                    },
                    (Some(&(ca, _)), None) => {
                        out.push(ca);
                        ia.next();
                    }
                    (None, Some(&(cb, _))) => {
                        out.push(cb);
                        ib.next();
                    }
                    (None, None) => break,
                }
            }
        }
        out
    }
    /// Approximate heap storage kept alive by this snapshot's bands that `newer` no longer
    /// shares. This is for bounding Undo history, not for reporting process memory.
    pub fn retained_bytes_against(&self, newer: &CellStore) -> usize {
        use std::mem::size_of;

        let mut bytes = 0usize;
        for (&key, band) in &self.bands {
            let next = newer.bands.get(&key);
            if next.is_some_and(|b| Arc::ptr_eq(b, band)) {
                continue;
            }
            bytes = bytes.saturating_add(size_of::<Band>());
            let per_cell_overhead = match band.as_ref() {
                Band::Flat { keys, cells } => {
                    bytes = bytes.saturating_add(keys.capacity().saturating_mul(size_of::<u32>()));
                    bytes = bytes.saturating_add(cells.capacity().saturating_mul(size_of::<Cell>()));
                    0
                }
                Band::Tree(_) => size_of::<u32>() + size_of::<Cell>() + 3 * size_of::<usize>(),
            };
            for (at, cell) in band.iter_all(key) {
                bytes = bytes.saturating_add(per_cell_overhead);
                let next_cell = next.and_then(|b| b.get(at));
                if let Some(formula) = &cell.formula
                    && !next_cell.and_then(|c| c.formula.as_ref()).is_some_and(|f| Arc::ptr_eq(f, formula))
                {
                    bytes = bytes.saturating_add(size_of::<crate::cell::Formula>() + formula.owned_text_len());
                    if !formula.is_shared()
                        && let Some((expr, _, _)) = formula.expr_shifted()
                    {
                        expr.walk(&mut |_| bytes = bytes.saturating_add(size_of::<gridcraft_formula::Expr>()));
                    }
                }
                match &cell.value {
                    gridcraft_core::Value::Text(text)
                        if !next_cell.and_then(|c| c.value.as_text()).is_some_and(|t| std::ptr::eq(t, text.as_ref())) =>
                    {
                        bytes = bytes.saturating_add(text.len());
                    }
                    gridcraft_core::Value::Array(array)
                        if !next_cell.is_some_and(|c| matches!(&c.value, gridcraft_core::Value::Array(a) if Arc::ptr_eq(a, array))) =>
                    {
                        bytes = bytes
                            .saturating_add(size_of::<gridcraft_core::Array>() + array.data.len().saturating_mul(size_of::<gridcraft_core::Value>()));
                    }
                    _ => {}
                }
            }
        }
        bytes
    }
    /// Removes every cell in `r` and returns them, row-major.
    pub fn take_range(&mut self, r: RangeRef) -> Vec<(CellRef, Cell)> {
        let (r0, r1, c0, c1) = (r.start.row, r.end.row, r.start.col, r.end.col);
        let mut out = Vec::new();
        let keys: Vec<u32> =
            self.bands.range(r0 / BAND..=r1 / BAND).filter(|&(&band_key, band)| band.has_any_in(band_key, r)).map(|(&k, _)| k).collect();
        for key in keys {
            let Some(band) = self.bands.get_mut(&key) else { continue };
            let inside_rows = |row: Option<u32>| row.is_some_and(|row| (r0..=r1).contains(&row));
            let whole_band = match band.as_ref() {
                Band::Flat { .. } => c0 == 0 && c1 >= MAX_COLS - 1 && inside_rows(band.first_row(key)) && inside_rows(band.last_row(key)),
                Band::Tree(_) => c0 == 0 && c1 == u32::MAX && inside_rows(band.first_row(key)) && inside_rows(band.last_row(key)),
            };
            if whole_band {
                let Some(band) = self.bands.remove(&key) else { continue };
                match Arc::unwrap_or_clone(band) {
                    Band::Flat { keys: bkeys, cells: bcells } => {
                        let base = key.saturating_mul(BAND);
                        self.len -= bkeys.len();
                        out.reserve(bkeys.len());
                        for (k, cell) in bkeys.into_iter().zip(bcells) {
                            out.push((CellRef::new(base + unpack_local_row(k), unpack_col(k)), cell));
                        }
                    }
                    Band::Tree(brows) => {
                        for (row, cols) in brows {
                            self.len -= cols.len();
                            for (col, cell) in cols {
                                out.push((CellRef::new(row, col), cell));
                            }
                        }
                    }
                }
                continue;
            }
            let band = Arc::make_mut(band);
            match band {
                Band::Flat { keys: bkeys, cells: bcells } if r0 == r1 => {
                    let span = flat_key_span(bkeys, pack_key(r0, c0), pack_key(r0, c1.min(MAX_COLS - 1)));
                    self.len -= span.len();
                    out.extend(bkeys.drain(span.clone()).zip(bcells.drain(span)).map(|(k, cell)| (CellRef::new(r0, unpack_col(k)), cell)));
                }
                Band::Flat { keys: bkeys, cells: bcells } => {
                    let base = key.saturating_mul(BAND);
                    let old_keys = std::mem::take(bkeys);
                    let old_cells = std::mem::take(bcells);
                    let mut kept_keys = Vec::with_capacity(old_keys.len());
                    let mut kept_cells = Vec::with_capacity(old_cells.len());
                    for (k, cell) in old_keys.into_iter().zip(old_cells) {
                        let row = base + unpack_local_row(k);
                        let col = unpack_col(k);
                        if (r0..=r1).contains(&row) && (c0..=c1).contains(&col) {
                            self.len -= 1;
                            out.push((CellRef::new(row, col), cell));
                        } else {
                            kept_keys.push(k);
                            kept_cells.push(cell);
                        }
                    }
                    *bkeys = kept_keys;
                    *bcells = kept_cells;
                }
                Band::Tree(brows) => {
                    let mut block = brows.split_off(&r0);
                    let mut tail = match r1.checked_add(1) {
                        Some(next) => block.split_off(&next),
                        None => BTreeMap::new(),
                    };
                    for (row, mut kept) in block {
                        if c0 == 0 && c1 == u32::MAX {
                            self.len -= kept.len();
                            for (col, cell) in kept {
                                out.push((CellRef::new(row, col), cell));
                            }
                        } else if c1.checked_add(1).is_none_or(|next| kept.range(next..).next().is_none()) {
                            let taken = kept.split_off(&c0);
                            self.len -= taken.len();
                            for (col, cell) in taken {
                                out.push((CellRef::new(row, col), cell));
                            }
                            if !kept.is_empty() {
                                brows.insert(row, kept);
                            }
                        } else {
                            for (col, cell) in kept.extract_if(c0..=c1, |_, _| true) {
                                self.len -= 1;
                                out.push((CellRef::new(row, col), cell));
                            }
                            if !kept.is_empty() {
                                brows.insert(row, kept);
                            }
                        }
                    }
                    brows.append(&mut tail);
                }
            }
            if band.is_empty() {
                self.bands.remove(&key);
            }
        }
        out
    }
    /// Rows strictly greater than or equal to `from` moved by `delta` (positive = down). Rows that
    /// would land before `from`... callers delete the vacated block first.
    pub fn shift_rows(&mut self, from: u32, delta: i64) {
        let moved = self.take_rows(from..=u32::MAX, 0..=u32::MAX);
        self.put_rows(moved.into_iter().filter_map(|(row, cols)| Some((shifted(row, delta, gridcraft_core::MAX_ROWS)?, cols))));
    }
    pub fn shift_cols(&mut self, from: u32, delta: i64) {
        let moved = self.take_rows(0..=u32::MAX, from..=u32::MAX);
        self.put_rows(moved.into_iter().map(|(row, cols)| (row, shift_keys(cols, delta))));
    }
    /// Deletes the rows in `spans` (`(first, count)`, ascending and disjoint) and closes the gaps
    /// in one pass: each later cell moves up once by the number of deleted rows above it.
    pub fn delete_rows(&mut self, spans: &[(u32, u32)]) {
        self.delete_lines(spans, true);
    }
    /// Column form of [`CellStore::delete_rows`].
    pub fn delete_cols(&mut self, spans: &[(u32, u32)]) {
        self.delete_lines(spans, false);
    }
    fn delete_lines(&mut self, spans: &[(u32, u32)], rows: bool) {
        let Some(&(first, _)) = spans.first() else { return };
        // Lines deleted before the end of each span, for mapping a kept line to its new place.
        let mut before = Vec::with_capacity(spans.len());
        let mut total = 0u32;
        for &(_, count) in spans {
            total = total.saturating_add(count);
            before.push(total);
        }
        let map = |v: u32| -> Option<u32> {
            let i = spans.partition_point(|&(at, _)| at <= v);
            let Some(i) = i.checked_sub(1) else { return Some(v) };
            let (at, count) = spans.get(i).copied().unwrap_or_default();
            if v - at < count { None } else { Some(v - before.get(i).copied().unwrap_or_default()) }
        };
        if !rows {
            let band_keys: Vec<u32> =
                self.bands.iter().filter(|(_, band)| band.col_bounds().is_some_and(|(_, hi)| hi >= first)).map(|(&k, _)| k).collect();
            for key in band_keys {
                let Some(band) = self.bands.get_mut(&key) else { continue };
                let band = Arc::make_mut(band);
                match band {
                    Band::Flat { keys, cells } => {
                        let mut write = 0usize;
                        for read in 0..keys.len() {
                            let k = keys[read];
                            let col = unpack_col(k);
                            if col < first {
                                if write != read {
                                    keys[write] = k;
                                    cells.swap(write, read);
                                }
                                write += 1;
                            } else if let Some(new_col) = map(col)
                                && !cells[read].is_blank()
                            {
                                keys[write] = (k & !COL_MASK) | new_col;
                                if write != read {
                                    cells.swap(write, read);
                                }
                                write += 1;
                            } else {
                                self.len -= 1;
                            }
                        }
                        keys.truncate(write);
                        cells.truncate(write);
                    }
                    Band::Tree(brows) => {
                        for cols in brows.values_mut() {
                            let tail = cols.split_off(&first);
                            for (col, cell) in tail {
                                if let Some(new_col) = map(col)
                                    && !cell.is_blank()
                                {
                                    cols.insert(new_col, cell);
                                } else {
                                    self.len -= 1;
                                }
                            }
                        }
                        brows.retain(|_, cols| !cols.is_empty());
                    }
                }
                if band.is_empty() {
                    self.bands.remove(&key);
                }
            }
            return;
        }
        if first >= gridcraft_core::MAX_ROWS {
            return;
        }
        let first_band = first / BAND;
        let last_band = (gridcraft_core::MAX_ROWS - 1) / BAND;
        let range = RangeRef::rows(first, gridcraft_core::MAX_ROWS - 1);
        let src_keys: Vec<u32> =
            self.bands.range(first_band..=last_band).filter(|&(&bk, band)| band.has_any_in(bk, range)).map(|(&k, _)| k).collect();
        if src_keys.is_empty() {
            return;
        }
        let mut extracted: Vec<(u32, Band)> = Vec::with_capacity(src_keys.len());
        for bk in src_keys {
            let whole_flat = bk > first_band || first.is_multiple_of(BAND);
            let is_flat = matches!(self.bands.get(&bk).map(Arc::as_ref), Some(Band::Flat { .. }));
            if is_flat && whole_flat {
                if let Some(band) = self.bands.remove(&bk) {
                    extracted.push((bk, Arc::unwrap_or_clone(band)));
                }
                continue;
            }
            let Some(band_arc) = self.bands.get_mut(&bk) else { continue };
            let band = Arc::make_mut(band_arc);
            match band {
                Band::Flat { keys: bkeys, cells: bcells } => {
                    let split_key = pack_key(first, 0);
                    let split_at = bkeys.partition_point(|&k| k < split_key);
                    let taken_keys = bkeys.split_off(split_at);
                    let taken_cells = bcells.split_off(split_at);
                    extracted.push((bk, Band::Flat { keys: taken_keys, cells: taken_cells }));
                }
                Band::Tree(brows) => {
                    let mut taken: BTreeMap<u32, BTreeMap<u32, Cell>> = BTreeMap::new();
                    let mut block = brows.split_off(&first);
                    let mut after = block.split_off(&gridcraft_core::MAX_ROWS);
                    for (row, mut cols) in block {
                        let off_grid = cols.split_off(&MAX_COLS);
                        if !cols.is_empty() {
                            taken.insert(row, cols);
                        }
                        if !off_grid.is_empty() {
                            brows.insert(row, off_grid);
                        }
                    }
                    brows.append(&mut after);
                    if !taken.is_empty() {
                        extracted.push((bk, Band::Tree(taken)));
                    }
                }
            }
            if band.is_empty() {
                self.bands.remove(&bk);
            }
        }
        let mut cur_bk: Option<u32> = None;
        let mut dest_keys: Vec<u32> = Vec::new();
        let mut dest_cells: Vec<Cell> = Vec::new();
        let flush = |bands: &mut BTreeMap<u32, Arc<Band>>, bk: u32, dkeys: &mut Vec<u32>, dcells: &mut Vec<Cell>| {
            if dkeys.is_empty() {
                return;
            }
            let keys = std::mem::take(dkeys);
            let cells = std::mem::take(dcells);
            match bands.get_mut(&bk) {
                None => {
                    bands.insert(bk, Arc::new(Band::Flat { keys, cells }));
                }
                Some(existing) => match Arc::make_mut(existing) {
                    Band::Flat { keys: bkeys, cells: bcells } => {
                        // All retained keys in `first_band` have `row < first`, while every
                        // surviving moved key has `new_row >= first`.
                        bkeys.extend(keys);
                        bcells.extend(cells);
                    }
                    Band::Tree(brows) => {
                        let base = bk.saturating_mul(BAND);
                        for (k, cell) in keys.into_iter().zip(cells) {
                            brows.entry(base + unpack_local_row(k)).or_default().insert(unpack_col(k), cell);
                        }
                    }
                },
            }
        };
        let mut push_cell = |bands: &mut BTreeMap<u32, Arc<Band>>, len: &mut usize, row: u32, col: u32, cell: Cell| {
            if let Some(new_row) = map(row)
                && !cell.is_blank()
            {
                let dest_bk = new_row / BAND;
                if cur_bk != Some(dest_bk) {
                    if let Some(prev_bk) = cur_bk {
                        flush(bands, prev_bk, &mut dest_keys, &mut dest_cells);
                    }
                    cur_bk = Some(dest_bk);
                }
                dest_keys.push(pack_key(new_row, col));
                dest_cells.push(cell);
            } else {
                *len -= 1;
            }
        };
        for (bk, band) in extracted {
            match band {
                Band::Flat { keys: bkeys, cells: bcells } => {
                    let base = bk.saturating_mul(BAND);
                    for (k, cell) in bkeys.into_iter().zip(bcells) {
                        push_cell(&mut self.bands, &mut self.len, base + unpack_local_row(k), unpack_col(k), cell);
                    }
                }
                Band::Tree(brows) => {
                    for (row, cols) in brows {
                        for (col, cell) in cols {
                            push_cell(&mut self.bands, &mut self.len, row, col, cell);
                        }
                    }
                }
            }
        }
        if let Some(prev_bk) = cur_bk {
            flush(&mut self.bands, prev_bk, &mut dest_keys, &mut dest_cells);
        }
    }
    /// Shifts cells within rows `r0..=r1` and columns `>= from` horizontally (Insert/Delete Cells
    /// with shift right/left).
    pub fn shift_cols_in_rows(&mut self, r0: u32, r1: u32, from: u32, delta: i64) {
        let r = RangeRef::new(CellRef::new(r0, from), CellRef::new(r1, gridcraft_core::MAX_COLS - 1));
        let moved = self.take_rows(r.start.row..=r.end.row, r.start.col..=r.end.col);
        self.put_rows(moved.into_iter().map(|(row, cols)| (row, shift_keys(cols, delta))));
    }
    /// Shifts cells within columns `c0..=c1` and rows `>= from` vertically.
    pub fn shift_rows_in_cols(&mut self, c0: u32, c1: u32, from: u32, delta: i64) {
        let range = RangeRef::new(CellRef::new(from, c0), CellRef::new(gridcraft_core::MAX_ROWS - 1, c1));
        let taken = self.take_range(range);
        if taken.is_empty() {
            return;
        }
        let mut iter =
            taken.into_iter().filter_map(|(c, cell)| shifted(c.row, delta, gridcraft_core::MAX_ROWS).map(|nr| (nr, c.col, cell))).peekable();
        let mut group: Vec<(u32, u32, Cell)> = Vec::new();
        while let Some((first_r, first_c, first_cell)) = iter.next() {
            let key = first_r / BAND;
            group.clear();
            group.push((first_r, first_c, first_cell));
            while let Some(next) = iter.next_if(|(r, _, _)| r / BAND == key) {
                group.push(next);
            }
            let existing = self.bands.get(&key);
            let writes = group.iter().any(|(r, c, cell)| !cell.is_blank() || existing.is_some_and(|b| b.contains(CellRef::new(*r, *c))));
            if !writes {
                continue;
            }
            let has_off_grid_col = group.iter().any(|(_, c, cell)| *c >= MAX_COLS && !cell.is_blank());
            let band = Arc::make_mut(self.bands.entry(key).or_default());
            if has_off_grid_col {
                band.promote_to_tree(key);
            }
            match band {
                Band::Flat { .. } => {
                    band.merge_flat_sorted(
                        group.drain(..).filter(|(_, c, _)| *c < MAX_COLS).map(|(r, c, cell)| (pack_key(r, c), cell)),
                        &mut self.len,
                    );
                }
                Band::Tree(brows) => {
                    for (r, c, cell) in group.drain(..) {
                        if cell.is_blank() {
                            if let Some(cols) = brows.get_mut(&r) {
                                if cols.remove(&c).is_some() {
                                    self.len -= 1;
                                }
                                if cols.is_empty() {
                                    brows.remove(&r);
                                }
                            }
                        } else if brows.entry(r).or_default().insert(c, cell).is_none() {
                            self.len += 1;
                        }
                    }
                }
            }
            if band.is_empty() {
                self.bands.remove(&key);
            }
        }
    }
    /// Removes the cells in `rows` x `cols` and returns them grouped by row, in row order. Only
    /// copies bands that hold a cell in the block, so snapshots keep sharing the others.
    fn take_rows(&mut self, rows: RangeInclusive<u32>, cols: RangeInclusive<u32>) -> Vec<(u32, BTreeMap<u32, Cell>)> {
        let (r0, r1, c0, c1) = (*rows.start(), *rows.end(), *cols.start(), *cols.end());
        let mut out = Vec::new();
        if r0 > r1 || c0 > c1 {
            return out;
        }
        let whole_rows = c0 == 0 && c1 == u32::MAX;
        let range = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
        let keys: Vec<u32> =
            self.bands.range(r0 / BAND..=r1 / BAND).filter(|&(&band_key, band)| band.has_any_in(band_key, range)).map(|(&k, _)| k).collect();
        for key in keys {
            let Some(band) = self.bands.get_mut(&key) else { continue };
            let inside = |row: Option<u32>| row.is_some_and(|r| (r0..=r1).contains(&r));
            if whole_rows && inside(band.first_row(key)) && inside(band.last_row(key)) {
                let Some(band) = self.bands.remove(&key) else { continue };
                for (row, taken) in Arc::unwrap_or_clone(band).into_rows(key) {
                    self.len -= taken.len();
                    out.push((row, taken));
                }
                continue;
            }
            let band = Arc::make_mut(band);
            match band {
                Band::Flat { keys: bkeys, cells: bcells } => {
                    let base = key.saturating_mul(BAND);
                    let mut kept_keys = Vec::with_capacity(bkeys.len());
                    let mut kept_cells = Vec::with_capacity(bcells.len());
                    let mut taken_rows: BTreeMap<u32, BTreeMap<u32, Cell>> = BTreeMap::new();
                    for (k, cell) in std::mem::take(bkeys).into_iter().zip(std::mem::take(bcells)) {
                        let r = base + unpack_local_row(k);
                        let c = unpack_col(k);
                        if (r0..=r1).contains(&r) && (c0..=c1).contains(&c) {
                            taken_rows.entry(r).or_default().insert(c, cell);
                            self.len -= 1;
                        } else {
                            kept_keys.push(k);
                            kept_cells.push(cell);
                        }
                    }
                    *bkeys = kept_keys;
                    *bcells = kept_cells;
                    out.extend(taken_rows);
                }
                Band::Tree(brows) => {
                    let mut block = brows.split_off(&r0);
                    let mut tail = match r1.checked_add(1) {
                        Some(next) => block.split_off(&next),
                        None => BTreeMap::new(),
                    };
                    for (row, mut kept) in block {
                        let taken = if whole_rows {
                            std::mem::take(&mut kept)
                        } else if c1.checked_add(1).is_none_or(|next| kept.range(next..).next().is_none()) {
                            kept.split_off(&c0)
                        } else {
                            let mut taken = BTreeMap::new();
                            for (col, cell) in kept.extract_if(c0..=c1, |_, _| true) {
                                taken.insert(col, cell);
                            }
                            taken
                        };
                        if !kept.is_empty() {
                            brows.insert(row, kept);
                        }
                        if !taken.is_empty() {
                            self.len -= taken.len();
                            out.push((row, taken));
                        }
                    }
                    brows.append(&mut tail);
                }
            }
            if band.is_empty() {
                self.bands.remove(&key);
            }
        }
        out
    }
    /// Stores rows of cells (each row at most once) the way `set` stores each cell: a cell
    /// replaces what is there, and a blank cell clears its destination instead of being stored.
    fn put_rows(&mut self, rows: impl Iterator<Item = (u32, BTreeMap<u32, Cell>)>) {
        let mut rows = rows.peekable();
        let mut group = Vec::new();
        while let Some((first, cols)) = rows.next() {
            let key = first / BAND;
            group.clear();
            group.push((first, cols));
            while let Some(next) = rows.next_if(|(row, _)| row / BAND == key) {
                group.push(next);
            }
            // Leave the band shared when only blank cells land on empty positions.
            let existing = self.bands.get(&key);
            let writes = group
                .iter()
                .any(|(row, cols)| cols.iter().any(|(&col, cell)| !cell.is_blank() || existing.is_some_and(|b| b.contains(CellRef::new(*row, col)))));
            if !writes {
                continue;
            }
            let has_off_grid_col = group.iter().any(|(_, cols)| cols.iter().any(|(&c, cell)| c >= MAX_COLS && !cell.is_blank()));
            let band = Arc::make_mut(self.bands.entry(key).or_default());
            if has_off_grid_col {
                band.promote_to_tree(key);
            }
            match band {
                Band::Tree(brows) => {
                    for (row, mut cols) in group.drain(..) {
                        match brows.entry(row) {
                            Entry::Vacant(slot) => {
                                if cols.values().any(Cell::is_blank) {
                                    cols.retain(|_, cell| !cell.is_blank());
                                }
                                if !cols.is_empty() {
                                    self.len += cols.len();
                                    slot.insert(cols);
                                }
                            }
                            Entry::Occupied(mut slot) => {
                                let dest = slot.get_mut();
                                for (col, cell) in cols {
                                    if cell.is_blank() {
                                        if dest.remove(&col).is_some() {
                                            self.len -= 1;
                                        }
                                    } else if dest.insert(col, cell).is_none() {
                                        self.len += 1;
                                    }
                                }
                                if dest.is_empty() {
                                    slot.remove();
                                }
                            }
                        }
                    }
                }
                Band::Flat { .. } => {
                    band.merge_flat_sorted(
                        group.drain(..).flat_map(|(row, cols)| {
                            cols.into_iter().filter(|(col, _)| *col < MAX_COLS).map(move |(col, cell)| (pack_key(row, col), cell))
                        }),
                        &mut self.len,
                    );
                }
            }
            if band.is_empty() {
                self.bands.remove(&key);
            }
        }
    }
}
/// `at + delta` when it lies in `0..limit`.
fn shifted(at: u32, delta: i64, limit: u32) -> Option<u32> {
    let to = (at as i64).checked_add(delta)?;
    if (0..limit as i64).contains(&to) { Some(to as u32) } else { None }
}

/// The cells of one row with their columns moved by `delta`; cells that would leave the sheet are
/// dropped.
fn shift_keys(cols: BTreeMap<u32, Cell>, delta: i64) -> BTreeMap<u32, Cell> {
    let mut out = BTreeMap::new();
    for (col, cell) in cols {
        if let Some(c) = shifted(col, delta, gridcraft_core::MAX_COLS) {
            out.insert(c, cell);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::{MAX_COLS, MAX_ROWS, Value};

    fn c(r: u32, col: u32) -> CellRef {
        CellRef::new(r, col)
    }

    #[test]
    fn set_get_remove() -> Result<(), Box<dyn std::error::Error>> {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(1.0.into()));
        s.set(c(100, 5), Cell::value("x".into()));
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(c(100, 5)).map(|x| x.value.clone()), Some(Value::from("x")));
        assert_eq!(s.used_range().ok_or("none")?.a1(), "A1:F101");
        s.set(c(0, 0), Cell::default());
        assert_eq!(s.len(), 1);
        assert!(s.remove(c(100, 5)).is_some());
        assert!(s.is_empty());
        assert!(s.used_range().is_none());
        Ok(())
    }

    #[test]
    fn cow_snapshots() -> Result<(), Box<dyn std::error::Error>> {
        let mut s = CellStore::new();
        for r in 0..1000 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        let snap = s.clone();
        s.set(c(5, 0), Cell::value(99.0.into()));
        assert_eq!(snap.get(c(5, 0)).ok_or("none")?.value, Value::Number(5.0));
        assert_eq!(s.get(c(5, 0)).ok_or("none")?.value, Value::Number(99.0));
        Ok(())
    }

    #[test]
    fn diff_visits_changed_bands() {
        let mut s = CellStore::new();
        for r in 0..1000 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        let snap = s.clone();
        assert!(s.diff(&snap).is_empty());
        s.set(c(5, 0), Cell::value(99.0.into()));
        s.set(c(700, 3), Cell::value("new".into()));
        s.remove(c(900, 0));
        assert_eq!(snap.diff(&s), [c(5, 0), c(700, 3), c(900, 0)]);
    }

    #[test]
    fn range_iteration_order() -> Result<(), Box<dyn std::error::Error>> {
        let mut s = CellStore::new();
        s.set(c(70, 2), Cell::value(3.0.into()));
        s.set(c(1, 1), Cell::value(1.0.into()));
        s.set(c(1, 3), Cell::value(2.0.into()));
        s.set(c(500, 0), Cell::value(4.0.into()));
        let v: Vec<f64> =
            s.iter_range(RangeRef::parse("A1:D100").ok_or("none")?).map(|(_, x)| x.value.as_f64()).collect::<Option<_>>().ok_or("none")?;
        assert_eq!(v, [1.0, 2.0, 3.0]);
        Ok(())
    }

    #[test]
    fn shifting() {
        let mut s = CellStore::new();
        s.set(c(2, 0), Cell::value(1.0.into()));
        s.set(c(5, 0), Cell::value(2.0.into()));
        s.shift_rows(3, 2);
        assert!(s.get(c(7, 0)).is_some());
        assert!(s.get(c(2, 0)).is_some());
        s.shift_cols(0, 1);
        assert!(s.get(c(7, 1)).is_some());
    }

    /// The per-cell algorithms the shifts used before they moved whole rows (#3575), kept as the
    /// reference the structural versions must match cell for cell.
    mod per_cell {
        use super::*;

        fn reset(s: &mut CellStore, moved: Vec<(CellRef, Cell)>, to: impl Fn(CellRef) -> Option<CellRef>) {
            for (at, cell) in moved {
                if let Some(at) = to(at) {
                    s.set(at, cell);
                }
            }
        }
        fn row_by(delta: i64) -> impl Fn(CellRef) -> Option<CellRef> {
            move |at| {
                let r = at.row as i64 + delta;
                (0..MAX_ROWS as i64).contains(&r).then(|| CellRef::new(r as u32, at.col))
            }
        }
        fn col_by(delta: i64) -> impl Fn(CellRef) -> Option<CellRef> {
            move |at| {
                let c = at.col as i64 + delta;
                (0..MAX_COLS as i64).contains(&c).then(|| CellRef::new(at.row, c as u32))
            }
        }
        pub fn take_range(s: &mut CellStore, r: RangeRef) -> Vec<(CellRef, Cell)> {
            let keys: Vec<CellRef> = s.iter_range(r).map(|(c, _)| c).collect();
            keys.into_iter().filter_map(|c| s.remove(c).map(|cell| (c, cell))).collect()
        }
        pub fn shift_rows(s: &mut CellStore, from: u32, delta: i64) {
            let keys: Vec<CellRef> = s.iter().filter(|(c, _)| c.row >= from).map(|(c, _)| c).collect();
            let moved = keys.into_iter().filter_map(|c| s.remove(c).map(|cell| (c, cell))).collect();
            reset(s, moved, row_by(delta));
        }
        pub fn shift_cols(s: &mut CellStore, from: u32, delta: i64) {
            let keys: Vec<CellRef> = s.iter().filter(|(c, _)| c.col >= from).map(|(c, _)| c).collect();
            let moved = keys.into_iter().filter_map(|c| s.remove(c).map(|cell| (c, cell))).collect();
            reset(s, moved, col_by(delta));
        }
        pub fn shift_cols_in_rows(s: &mut CellStore, r0: u32, r1: u32, from: u32, delta: i64) {
            let moved = take_range(s, RangeRef::new(CellRef::new(r0, from), CellRef::new(r1, MAX_COLS - 1)));
            reset(s, moved, col_by(delta));
        }
        pub fn shift_rows_in_cols(s: &mut CellStore, c0: u32, c1: u32, from: u32, delta: i64) {
            let moved = take_range(s, RangeRef::new(CellRef::new(from, c0), CellRef::new(MAX_ROWS - 1, c1)));
            reset(s, moved, row_by(delta));
        }
    }

    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % n.max(1)
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[self.below(xs.len() as u64) as usize]
        }
    }

    /// A position near a band edge, the sheet edge, or anywhere below `max`.
    fn coord(rng: &mut Lcg, max: u32, edges: &[u32]) -> u32 {
        match rng.below(3) {
            0 => rng.pick(edges).min(max - 1),
            1 => max - 1 - rng.below(70.min(max as u64)) as u32,
            _ => rng.below(max as u64) as u32,
        }
    }

    /// Random store with values, styled-only cells, a few cells off the grid, and blank cells left
    /// behind by `get_mut` (which `set` never stores, but the old shifts dropped and let clear
    /// their destination).
    fn random_store(rng: &mut Lcg, max_r: u32, max_c: u32) -> CellStore {
        let row_edges = [0, 1, 62, 63, 64, 65, 127, 128, 129, 191, 192, MAX_ROWS - 64, MAX_ROWS - 1];
        let col_edges = [0, 1, 2, 63, 64, 65, MAX_COLS - 2, MAX_COLS - 1];
        let mut s = CellStore::new();
        for _ in 0..rng.below(300) {
            let mut at = CellRef::new(coord(rng, max_r, &row_edges), coord(rng, max_c, &col_edges));
            // Now and then a cell outside the grid (`set` does not check): the shifts must treat
            // it as the old code did.
            match rng.below(60) {
                0 => at.row = MAX_ROWS + rng.below(3) as u32,
                1 => at.col = MAX_COLS + rng.below(3) as u32,
                _ => {}
            }
            let cell = match rng.below(5) {
                0 => Cell { style: crate::style::StyleId(1 + rng.below(3) as u32), ..Cell::default() },
                1 => Cell::value(Value::from("t")),
                _ => Cell::value(Value::Number(rng.below(1000) as f64)),
            };
            s.set(at, cell);
            if rng.below(8) == 0
                && let Some(cell) = s.get_mut(at)
            {
                *cell = Cell::default();
            }
        }
        s
    }

    fn assert_same(new: &CellStore, old: &CellStore, base: &CellStore, base_copy: &CellStore, what: &str) {
        assert_eq!(new, old, "{what}");
        assert_eq!(new.len(), new.iter().count(), "{what}: len");
        assert!(new.iter().map(|(c, x)| (c, x.clone())).eq(old.iter().map(|(c, x)| (c, x.clone()))), "{what}: iteration");
        assert!(
            new.bands.values().all(|b| !b.is_empty()
                && match b.as_ref() {
                    Band::Flat { keys, cells } => keys.len() == cells.len() && keys.windows(2).all(|w| w[0] < w[1]),
                    Band::Tree(rows) => rows.values().all(|r| !r.is_empty()),
                }),
            "{what}: empty band or row"
        );
        // Copy-on-write: every band the old code left shared with the snapshot stays shared.
        for (k, b) in &base.bands {
            if old.bands.get(k).is_some_and(|o| Arc::ptr_eq(o, b)) {
                assert!(new.bands.get(k).is_some_and(|n| Arc::ptr_eq(n, b)), "{what}: band {k} copied");
            }
        }
        assert_eq!(base, base_copy, "{what}: snapshot changed");
    }

    #[test]
    fn structural_shifts_match_per_cell_shifts() {
        let mut rng = Lcg(3575);
        let deltas = [0i64, 1, -1, 2, -2, 63, -63, 64, -64, 65, -65, 128, -128, 1000, -1000, MAX_ROWS as i64, -(MAX_ROWS as i64)];
        let row_edges = [0, 1, 63, 64, 65, 127, 128, MAX_ROWS - 1, MAX_ROWS];
        let col_edges = [0, 1, 63, 64, 65, MAX_COLS - 1, MAX_COLS];
        for trial in 0..3000 {
            let max_r = [200, 5000, MAX_ROWS][trial % 3];
            let max_c = [30, MAX_COLS][trial / 3 % 2];
            let mut base = random_store(&mut rng, max_r, max_c);
            for step in 0..4 {
                let snapshot = base.clone();
                let (mut new, mut old) = (base.clone(), base.clone());
                let delta = if rng.below(2) == 0 { rng.pick(&deltas) } else { rng.below(301) as i64 - 150 };
                let row = if rng.below(2) == 0 { rng.pick(&row_edges) } else { rng.below(max_r as u64 + 1) as u32 };
                let col = if rng.below(2) == 0 { rng.pick(&col_edges) } else { rng.below(max_c as u64 + 1) as u32 };
                let (row2, col2) = (rng.below(max_r as u64 + 1) as u32, rng.below(max_c as u64 + 1) as u32);
                let op = rng.below(5);
                match op {
                    0 => {
                        new.shift_rows(row, delta);
                        per_cell::shift_rows(&mut old, row, delta);
                    }
                    1 => {
                        new.shift_cols(col, delta);
                        per_cell::shift_cols(&mut old, col, delta);
                    }
                    2 => {
                        new.shift_cols_in_rows(row, row2, col, delta);
                        per_cell::shift_cols_in_rows(&mut old, row, row2, col, delta);
                    }
                    3 => {
                        new.shift_rows_in_cols(col, col2, row, delta);
                        per_cell::shift_rows_in_cols(&mut old, col, col2, row, delta);
                    }
                    _ => {
                        let r = RangeRef::new(CellRef::new(row, col), CellRef::new(row2, col2));
                        assert_eq!(new.take_range(r), per_cell::take_range(&mut old, r), "trial {trial} step {step}: taken cells");
                    }
                }
                let what = format!("trial {trial} step {step} op {op} row {row}..{row2} col {col}..{col2} delta {delta}");
                assert_same(&new, &old, &base, &snapshot, &what);
                base = new;
            }
        }
    }

    #[test]
    fn off_grid_cells_right_of_a_block_stay_put() {
        let mut s = CellStore::new();
        for col in [0, 5, MAX_COLS - 1, MAX_COLS + 1] {
            s.set(c(3, col), Cell::value((col as f64).into()));
        }
        let mut old = s.clone();
        s.shift_cols_in_rows(3, 3, 0, 1);
        per_cell::shift_cols_in_rows(&mut old, 3, 3, 0, 1);
        assert_eq!(s, old);
        assert_eq!(s.get(c(3, MAX_COLS + 1)).map(|x| x.value.clone()), Some(Value::Number((MAX_COLS + 1) as f64)));
        let r = RangeRef::rows(3, 3);
        assert_eq!(s.take_range(r), per_cell::take_range(&mut old, r));
        assert_eq!(s, old);
    }

    #[test]
    fn moved_blank_cell_clears_its_destination() {
        let mut s = CellStore::new();
        s.set(c(3, 0), Cell::value(1.0.into()));
        s.set(c(5, 0), Cell::value(2.0.into()));
        s.set(c(5, 1), Cell::value(3.0.into()));
        if let Some(cell) = s.get_mut(c(5, 0)) {
            *cell = Cell::default();
        }
        s.shift_rows(5, -2);
        assert!(s.get(c(3, 0)).is_none());
        assert_eq!(s.get(c(3, 1)).map(|x| x.value.clone()), Some(Value::Number(3.0)));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn shifting_keeps_untouched_bands_shared() {
        let mut s = CellStore::new();
        for r in 0..256 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        s.set(c(200, 5), Cell::value("x".into()));
        let snap = s.clone();
        s.shift_rows(130, 1);
        assert!(Arc::ptr_eq(&s.bands[&0], &snap.bands[&0]));
        assert!(Arc::ptr_eq(&s.bands[&1], &snap.bands[&1]));
        s.shift_cols(5, 1);
        assert!(Arc::ptr_eq(&s.bands[&0], &snap.bands[&0]));
        assert_eq!(s.get(c(201, 6)).map(|x| x.value.clone()), Some(Value::from("x")));
    }

    #[test]
    fn flat_band_row_indices_shrink_and_promotion_preserve_semantics() {
        let mut s = CellStore::new();
        for r in [0, 1, 63, 64, 128, 128, 1000] {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        s.set(c(128, 5), Cell::value(99.0.into()));
        s.shrink_to_fit();
        assert_eq!(s.row_indices(), vec![0, 1, 63, 64, 128, 1000]);
        let mut promoted = CellStore::new();
        let mut flat = CellStore::new();
        for r in 0..64u32 {
            for col in 0..70u32 {
                let v = Cell::value(((r * 100 + col) as f64).into());
                flat.set(c(r, col), v.clone());
                if r > 0 {
                    promoted.set(c(r, col), v);
                }
            }
        }
        for col in (0..70u32).rev() {
            promoted.set(c(0, col), Cell::value((col as f64).into()));
        }
        assert_eq!(promoted, flat);
        assert_eq!(promoted.row_indices(), flat.row_indices());
    }

    #[test]
    fn dense_flat_bands_stay_flat_across_delete_take_and_shift_ops() {
        let mut base = CellStore::new();
        // 192 rows (3 bands) x 80 cols = 5120 cells per band (> FLAT_MAX_MUTATE = 256).
        for r in 0..192u32 {
            for col in 0..80u32 {
                base.set(c(r, col), Cell::value(((r * 100 + col) as f64).into()));
            }
        }
        let all_flat = |s: &CellStore| s.bands.values().all(|b| matches!(b.as_ref(), Band::Flat { .. }));
        assert!(all_flat(&base));

        // delete_rows: verify exact semantics, CoW on untouched bands, and Flat retention.
        let mut s = base.clone();
        let snap = s.clone();
        s.delete_rows(&[(65, 2), (130, 3)]);
        assert!(all_flat(&s), "delete_rows should keep bands Flat");
        assert!(Arc::ptr_eq(&s.bands[&0], &snap.bands[&0]), "band 0 before first deleted row stays shared");
        assert_eq!(s.len(), (192 - 5) * 80);
        assert_eq!(s.get(c(64, 7)).unwrap().value, Value::Number((64 * 100 + 7) as f64));
        assert_eq!(s.get(c(65, 7)).unwrap().value, Value::Number((67 * 100 + 7) as f64));
        assert_eq!(s.get(c(128, 7)).unwrap().value, Value::Number((133 * 100 + 7) as f64));

        // delete_cols: verify exact semantics and Flat retention.
        let mut s = base.clone();
        s.delete_cols(&[(0, 1), (10, 2)]);
        assert!(all_flat(&s), "delete_cols should keep bands Flat");
        assert_eq!(s.len(), 192 * (80 - 3));
        assert_eq!(s.get(c(5, 0)).unwrap().value, Value::Number((5 * 100 + 1) as f64));
        assert_eq!(s.get(c(5, 9)).unwrap().value, Value::Number((5 * 100 + 12) as f64));

        // take_range: interior block stays Flat and returns exact cells in row-major order.
        let mut s = base.clone();
        let mut ref_store = base.clone();
        let r = RangeRef::new(c(10, 5), c(150, 40));
        let taken = s.take_range(r);
        let expected = per_cell::take_range(&mut ref_store, r);
        assert!(all_flat(&s), "take_range should keep bands Flat");
        assert_eq!(taken, expected);
        assert_eq!(s, ref_store);

        // shift_rows_in_cols: interior columns shift down/up while staying Flat.
        let mut s = base.clone();
        let mut ref_store = base.clone();
        s.shift_rows_in_cols(5, 30, 10, 3);
        per_cell::shift_rows_in_cols(&mut ref_store, 5, 30, 10, 3);
        assert!(all_flat(&s), "shift_rows_in_cols(+3) should keep bands Flat");
        assert_eq!(s, ref_store);
        s.shift_rows_in_cols(5, 30, 15, -4);
        per_cell::shift_rows_in_cols(&mut ref_store, 5, 30, 15, -4);
        assert!(all_flat(&s), "shift_rows_in_cols(-4) should keep bands Flat");
        assert_eq!(s, ref_store);
    }

    #[test]
    fn delete_lines_matches_per_cell_with_edge_cases() {
        let mut rng = Lcg(9001);
        for trial in 0..200 {
            let mut base = random_store(&mut rng, 200, 30);
            for &rows in &[true, false] {
                let spans = [(1u32, 2u32), (63, 3), (128, 1)];
                let mut new = base.clone();
                let mut old = base.clone();
                let snap = base.clone();
                if rows {
                    new.delete_rows(&spans);
                } else {
                    new.delete_cols(&spans);
                }
                // Reference per-cell delete_lines
                let mut before = Vec::with_capacity(spans.len());
                let mut total = 0u32;
                for &(_, count) in &spans {
                    total = total.saturating_add(count);
                    before.push(total);
                }
                let map = |v: u32| -> Option<u32> {
                    let i = spans.partition_point(|&(at, _)| at <= v);
                    let i = i.checked_sub(1)?;
                    let (at, count) = spans[i];
                    if v - at < count { None } else { Some(v - before[i]) }
                };
                let keys: Vec<CellRef> = if rows {
                    old.iter_range(RangeRef::new(c(1, 0), c(MAX_ROWS - 1, MAX_COLS - 1))).map(|(p, _)| p).collect()
                } else {
                    old.iter().filter(|(p, _)| p.col >= 1).map(|(p, _)| p).collect()
                };
                let moved: Vec<(CellRef, Cell)> = keys.into_iter().filter_map(|p| old.remove(p).map(|cell| (p, cell))).collect();
                for (p, cell) in moved {
                    let to = if rows { map(p.row).map(|r| c(r, p.col)) } else { map(p.col).map(|x| c(p.row, x)) };
                    if let Some(to) = to {
                        old.set(to, cell);
                    }
                }
                assert_same(&new, &old, &base, &snap, &format!("trial {trial} rows={rows}"));
            }
            base.delete_rows(&[(0, 1)]);
        }
    }

    #[test]
    fn col_bounds_and_used_range_handle_u32_max_column() {
        let mut s = CellStore::new();
        s.set(c(0, u32::MAX), Cell::value(1.0.into()));
        assert_eq!(s.used_range(), Some(RangeRef::cell(c(0, u32::MAX))));
        assert!(s.bands[&0].has_any_in(0, RangeRef::new(c(0, u32::MAX), c(10, u32::MAX))));
    }

    #[test]
    fn used_range_finds_flat_column_extrema_on_interior_rows() {
        let mut s = CellStore::new();
        for (row, cols) in [(2, 9..520), (7, 0..320), (21, MAX_COLS - 300..MAX_COLS), (63, 7..800)] {
            for col in cols {
                s.set(c(row, col), Cell::value(1.0.into()));
            }
        }
        assert_eq!(s.used_range(), Some(RangeRef::new(c(2, 0), c(63, MAX_COLS - 1))));
        s.take_range(RangeRef::rows(7, 7));
        s.take_range(RangeRef::cell(c(21, MAX_COLS - 1)));
        assert_eq!(s.used_range(), Some(RangeRef::new(c(2, 7), c(63, MAX_COLS - 2))));
    }

    #[test]
    fn flat_range_iteration_matches_linear_filter_across_rows_and_bands() {
        let mut s = CellStore::new();
        let last_band = u32::MAX - (BAND - 1);
        for row in [0, 1, 3, 7, 63, 64, 65, last_band, last_band + 1, u32::MAX] {
            for col in (0..256).chain([MAX_COLS - 1]) {
                if col == 128 && (row == 3 || row == 64) {
                    continue;
                }
                s.set(c(row, col), Cell::value((f64::from(row) + f64::from(col)).into()));
            }
        }
        for r in [
            RangeRef::new(c(1, 128), c(65, 128)),
            RangeRef::new(c(1, 0), c(64, 2)),
            RangeRef::new(c(63, 100), c(65, MAX_COLS - 2)),
            RangeRef::new(c(0, 0), c(u32::MAX, u32::MAX)),
            RangeRef::new(c(last_band, MAX_COLS - 1), c(u32::MAX, u32::MAX)),
            RangeRef::new(c(last_band, MAX_COLS), c(u32::MAX, u32::MAX)),
        ] {
            assert!(s.iter_range(r).eq(s.iter().filter(|&(at, _)| r.contains(at))), "{r:?}");
        }

        let snap = s.clone();
        assert!(s.take_range(RangeRef::new(c(1, 1024), c(63, 1024))).is_empty());
        assert!(Arc::ptr_eq(&s.bands[&0], &snap.bands[&0]));
    }

    #[test]
    fn flat_single_row_take_reuses_unique_storage_and_preserves_snapshots() {
        let mut s = CellStore::new();
        let value = |row, col| Cell::value((f64::from(row * 128 + col)).into());
        for row in 0..4u32 {
            for col in 0..128 {
                s.set(c(row, col), value(row, col));
            }
        }
        let allocations = |s: &CellStore| {
            let Band::Flat { keys, cells } = s.bands[&0].as_ref() else { panic!("expected a flat band") };
            (keys.as_ptr(), cells.as_ptr())
        };
        let before = allocations(&s);
        let at = c(1, 64);
        assert_eq!(s.take_range(RangeRef::cell(at)), [(at, value(1, 64))]);
        assert_eq!(allocations(&s), before);
        assert_eq!(s.len(), 4 * 128 - 1);
        assert!(s.get(at).is_none());

        let snap = s.clone();
        let r = RangeRef::new(c(2, 125), c(2, u32::MAX));
        let expected: Vec<_> = (125..128).map(|col| (c(2, col), value(2, col))).collect();
        assert_eq!(s.take_range(r), expected);
        assert_eq!(s.len(), snap.len() - expected.len());
        assert_eq!(snap.diff(&s), [c(2, 125), c(2, 126), c(2, 127)]);
        assert!(snap.iter_range(r).eq(expected.iter().map(|(at, cell)| (*at, cell))));
    }

    #[test]
    fn flat_single_row_take_drops_empty_band_at_coordinate_limits() {
        let mut s = CellStore::new();
        let at = c(u32::MAX, MAX_COLS - 1);
        let cell = Cell::value(1.0.into());
        s.set(at, cell.clone());
        let r = RangeRef::new(at, c(u32::MAX, u32::MAX));
        assert_eq!(s.take_range(r), [(at, cell)]);
        assert!(s.is_empty());
        assert!(s.bands.is_empty());
    }

    #[test]
    fn undo_retention_charges_both_flat_capacities_only_when_unshared() {
        use std::mem::size_of;

        let mut snap = CellStore::new();
        let text = "retained text";
        snap.set(c(0, 0), Cell::value(text.into()));
        let band = Arc::get_mut(snap.bands.get_mut(&0).unwrap()).unwrap();
        let Band::Flat { keys, cells } = band else { panic!("expected a flat band") };
        keys.reserve_exact(16_000);
        cells.reserve_exact(7_000);
        let backing_bytes = size_of::<Band>() + keys.capacity() * size_of::<u32>() + cells.capacity() * size_of::<Cell>();

        let mut newer = snap.clone();
        assert_eq!(snap.retained_bytes_against(&newer), 0);
        newer.set(c(0, 1), Cell::value(1.0.into()));
        assert_eq!(snap.retained_bytes_against(&newer), backing_bytes);
        newer.set(c(0, 0), Cell::value(2.0.into()));
        assert_eq!(snap.retained_bytes_against(&newer), backing_bytes + text.len());
    }

    #[test]
    fn flat_band_front_to_back_remove_promotes_and_empties_band() {
        let mut s = CellStore::new();
        // Single 64x64 band = 4,096 cells, built sequentially so it stays Flat.
        for r in 0..64u32 {
            for col in 0..64u32 {
                s.set(c(r, col), Cell::value(((r * 100 + col) as f64).into()));
            }
        }
        assert_eq!(s.len(), 4096);
        assert!(matches!(s.bands[&0].as_ref(), Band::Flat { .. }));

        // Tail removal stays Flat.
        let tail = s.remove(c(63, 63)).unwrap();
        assert_eq!(tail.value, Value::Number(6363.0));
        assert!(matches!(s.bands[&0].as_ref(), Band::Flat { .. }));
        s.set(c(63, 63), tail);
        assert!(matches!(s.bands[&0].as_ref(), Band::Flat { .. }));

        // Bulk take_range on a clone clears an interior block while keeping the band Flat.
        let mut bulk = s.clone();
        let taken = bulk.take_range(RangeRef::new(c(8, 8), c(55, 55)));
        assert_eq!(taken.len(), 48 * 48);
        assert_eq!(bulk.len(), 4096 - 48 * 48);
        assert!(matches!(bulk.bands[&0].as_ref(), Band::Flat { .. }));

        // Front-to-back single-cell removal promotes on the first non-tail remove (>= 256)
        // and empties the entire band in O(n log n).
        for r in 0..64u32 {
            for col in 0..64u32 {
                let removed = s.remove(c(r, col)).expect("cell should exist");
                assert_eq!(removed.value, Value::Number((r * 100 + col) as f64));
                if r == 0 && col == 0 {
                    assert!(matches!(s.bands[&0].as_ref(), Band::Tree(_)), "non-tail remove on dense Flat band must promote to Tree");
                }
            }
        }
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.bands.is_empty());
    }

    #[test]
    fn flat_band_reverse_and_interleaved_inserts_promote_at_threshold() {
        // Reverse insertion of 1,000 cells into a single band (20 rows x 50 cols).
        let mut rev = CellStore::new();
        for (step, idx) in (0..1000u32).rev().enumerate() {
            let r = idx / 50;
            let col = idx % 50;
            rev.set(c(r, col), Cell::value((idx as f64).into()));
            if step + 1 == FLAT_MAX_MUTATE {
                assert!(matches!(rev.bands[&0].as_ref(), Band::Flat { .. }), "band should remain Flat up to FLAT_MAX_MUTATE cells");
            } else if step == FLAT_MAX_MUTATE {
                assert!(matches!(rev.bands[&0].as_ref(), Band::Tree(_)), "non-tail insert at >= FLAT_MAX_MUTATE must promote to Tree");
            }
        }
        assert_eq!(rev.len(), 1000);
        assert!(matches!(rev.bands[&0].as_ref(), Band::Tree(_)));
        for idx in 0..1000u32 {
            let r = idx / 50;
            let col = idx % 50;
            assert_eq!(rev.get(c(r, col)).map(|cell| &cell.value), Some(&Value::Number(idx as f64)));
        }

        // Interleaved insertion of 1,000 cells (even columns then odd columns) into one band.
        let mut interleaved = CellStore::new();
        let mut sequential = CellStore::new();
        for r in 0..20u32 {
            for col in 0..50u32 {
                sequential.set(c(r, col), Cell::value(((r * 50 + col) as f64).into()));
            }
        }
        for parity in 0..2u32 {
            for r in 0..20u32 {
                for col in (parity..50u32).step_by(2) {
                    interleaved.set(c(r, col), Cell::value(((r * 50 + col) as f64).into()));
                }
            }
        }
        assert!(matches!(sequential.bands[&0].as_ref(), Band::Flat { .. }));
        assert!(matches!(interleaved.bands[&0].as_ref(), Band::Tree(_)));
        assert_eq!(interleaved, sequential);
    }
}
