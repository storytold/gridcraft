//! Copy-on-write sparse cell storage.
//!
//! Cells live in bands of 64 rows, each band behind an `Arc`. Cloning a store (an undo snapshot)
//! copies only the band pointers; editing a cell copies just its band. A row holds its cells in
//! a vector sorted by column: compact for the few cells most rows have, and contiguous to scan.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef};
use serde::{Deserialize, Serialize};

use crate::cell::Cell;

const BAND: u32 = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Band {
    /// row → cells
    rows: BTreeMap<u32, Row>,
}

/// One row's cells, sorted by column (serialized as a column → cell map).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "BTreeMap<u32, Cell>", into = "BTreeMap<u32, Cell>")]
struct Row(Vec<(u32, Cell)>);

impl From<BTreeMap<u32, Cell>> for Row {
    fn from(m: BTreeMap<u32, Cell>) -> Row {
        Row(m.into_iter().collect())
    }
}

impl From<Row> for BTreeMap<u32, Cell> {
    fn from(r: Row) -> BTreeMap<u32, Cell> {
        r.0.into_iter().collect()
    }
}

impl Row {
    fn find(&self, col: u32) -> Result<usize, usize> {
        self.0.binary_search_by_key(&col, |(c, _)| *c)
    }
    fn get(&self, col: u32) -> Option<&Cell> {
        self.find(col).ok().and_then(|i| self.0.get(i)).map(|(_, cell)| cell)
    }
    fn get_mut(&mut self, col: u32) -> Option<&mut Cell> {
        self.find(col).ok().and_then(|i| self.0.get_mut(i)).map(|(_, cell)| cell)
    }
    fn contains(&self, col: u32) -> bool {
        self.find(col).is_ok()
    }
    /// Stores `cell` at `col`, returning the cell it replaces.
    fn insert(&mut self, col: u32, cell: Cell) -> Option<Cell> {
        match self.find(col) {
            Ok(i) => self.0.get_mut(i).map(|slot| std::mem::replace(&mut slot.1, cell)),
            Err(i) => {
                // Grow by a quarter, not double: most rows hold a few cells.
                if self.0.len() == self.0.capacity() {
                    self.0.reserve_exact((self.0.len() / 4).max(1));
                }
                self.0.insert(i, (col, cell));
                None
            }
        }
    }
    fn remove(&mut self, col: u32) -> Option<Cell> {
        self.find(col).ok().map(|i| self.0.remove(i).1)
    }
    /// Cells in columns `c0..=c1`.
    fn range(&self, c0: u32, c1: u32) -> &[(u32, Cell)] {
        let a = self.0.partition_point(|(c, _)| *c < c0);
        let b = self.0.partition_point(|(c, _)| *c <= c1);
        self.0.get(a..b.max(a)).unwrap_or(&[])
    }
    fn iter(&self) -> std::slice::Iter<'_, (u32, Cell)> {
        self.0.iter()
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Removes and returns the cells in columns `from..`.
    fn split_off(&mut self, from: u32) -> Vec<(u32, Cell)> {
        let i = self.0.partition_point(|(c, _)| *c < from);
        self.0.split_off(i)
    }
    /// Stores every cell of `other`, replacing cells in the same columns.
    fn merge(&mut self, other: Row) {
        if self.0.is_empty() {
            *self = other;
            return;
        }
        for (c, cell) in other.0 {
            self.insert(c, cell);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CellStore {
    bands: BTreeMap<u32, Arc<Band>>,
    len: usize,
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
    pub fn get(&self, c: CellRef) -> Option<&Cell> {
        self.bands.get(&(c.row / BAND))?.rows.get(&c.row)?.get(c.col)
    }
    /// Mutable access (copies the band if shared).
    pub fn get_mut(&mut self, c: CellRef) -> Option<&mut Cell> {
        let band = self.bands.get_mut(&(c.row / BAND))?;
        if !band.rows.get(&c.row).is_some_and(|r| r.contains(c.col)) {
            return None;
        }
        Arc::make_mut(band).rows.get_mut(&c.row)?.get_mut(c.col)
    }
    /// Stores a cell; a default (empty, unstyled) cell is removed instead.
    pub fn set(&mut self, c: CellRef, cell: Cell) {
        if cell.is_blank() {
            self.remove(c);
            return;
        }
        let band = Arc::make_mut(self.bands.entry(c.row / BAND).or_default());
        if band.rows.entry(c.row).or_default().insert(c.col, cell).is_none() {
            self.len += 1;
        }
    }
    pub fn remove(&mut self, c: CellRef) -> Option<Cell> {
        let key = c.row / BAND;
        let band = self.bands.get_mut(&key)?;
        if !band.rows.get(&c.row).is_some_and(|r| r.contains(c.col)) {
            return None;
        }
        let b = Arc::make_mut(band);
        let row = b.rows.get_mut(&c.row)?;
        let old = row.remove(c.col);
        if row.is_empty() {
            b.rows.remove(&c.row);
        }
        if b.rows.is_empty() {
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
        self.bands.range(b0..=b1).flat_map(move |(_, band)| {
            band.rows
                .range(r.start.row..=r.end.row)
                .flat_map(move |(row, cols)| cols.range(r.start.col, r.end.col).iter().map(move |(col, cell)| (CellRef::new(*row, *col), cell)))
        })
    }
    pub fn iter(&self) -> impl Iterator<Item = (CellRef, &Cell)> + '_ {
        self.bands
            .values()
            .flat_map(|band| band.rows.iter().flat_map(|(row, cols)| cols.iter().map(move |(col, cell)| (CellRef::new(*row, *col), cell))))
    }
    /// Cells of one row in `c0..=c1`.
    pub fn row(&self, row: u32, c0: u32, c1: u32) -> impl Iterator<Item = (u32, &Cell)> + '_ {
        self.bands
            .get(&(row / BAND))
            .and_then(|b| b.rows.get(&row))
            .into_iter()
            .flat_map(move |cols| cols.range(c0, c1).iter().map(|(c, cell)| (*c, cell)))
    }
    /// Smallest range containing every stored cell, `None` when empty.
    pub fn used_range(&self) -> Option<RangeRef> {
        let first_band = self.bands.values().next()?;
        let last_band = self.bands.values().next_back()?;
        let r0 = *first_band.rows.keys().next()?;
        let r1 = *last_band.rows.keys().next_back()?;
        let mut c0 = u32::MAX;
        let mut c1 = 0;
        for band in self.bands.values() {
            for cols in band.rows.values() {
                if let (Some((a, _)), Some((b, _))) = (cols.0.first(), cols.0.last()) {
                    c0 = c0.min(*a);
                    c1 = c1.max(*b);
                }
            }
        }
        if c0 == u32::MAX {
            return None;
        }
        Some(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }
    /// Last stored row in column `col` at or above `below_or_at` (for Ctrl+arrow navigation).
    pub fn has(&self, c: CellRef) -> bool {
        self.get(c).is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some())
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
            for (row, cols) in &a.rows {
                for (col, cell) in cols.iter() {
                    if b.rows.get(row).and_then(|r| r.get(*col)) != Some(cell) {
                        out.push(CellRef::new(*row, *col));
                    }
                }
            }
            for (row, cols) in &b.rows {
                for (col, _) in cols.iter() {
                    if !a.rows.get(row).is_some_and(|r| r.contains(*col)) {
                        out.push(CellRef::new(*row, *col));
                    }
                }
            }
        }
        out
    }
    /// Removes every cell in `r` and returns them.
    pub fn take_range(&mut self, r: RangeRef) -> Vec<(CellRef, Cell)> {
        let keys: Vec<CellRef> = self.iter_range(r).map(|(c, _)| c).collect();
        keys.into_iter().filter_map(|c| self.remove(c).map(|cell| (c, cell))).collect()
    }
    /// Moves every row at or below `from` by `delta` rows (a row insert or delete). Whole rows
    /// move, not cell by cell: the bands from `from` on are taken apart and their rows put back
    /// at their new numbers. A moved cell replaces one already where it lands; rows moved past
    /// the sheet's edge are dropped.
    pub fn shift_rows(&mut self, from: u32, delta: i64) {
        if delta == 0 {
            return;
        }
        let mut tail = self.bands.split_off(&(from / BAND));
        // (moved, row number, cells): rows that stay are put back first, so a moved row landing
        // on one replaces its cells.
        let mut rows: Vec<(bool, u32, Row)> = Vec::new();
        for (_, band) in std::mem::take(&mut tail) {
            let band = Arc::try_unwrap(band).unwrap_or_else(|shared| (*shared).clone());
            for (r, row) in band.rows {
                if r < from {
                    rows.push((false, r, row));
                } else {
                    let nr = r as i64 + delta;
                    if (0..gridcraft_core::MAX_ROWS as i64).contains(&nr) {
                        rows.push((true, nr as u32, row));
                    }
                }
            }
        }
        rows.sort_by_key(|(moved, _, _)| *moved);
        for (_, r, row) in rows {
            let band = Arc::make_mut(self.bands.entry(r / BAND).or_default());
            band.rows.entry(r).or_default().merge(row);
        }
        self.recount();
    }
    /// Moves every cell in a column at or right of `from` by `delta` columns (a column insert
    /// or delete), row by row. A moved cell replaces one already where it lands; cells moved
    /// past the sheet's edge are dropped.
    pub fn shift_cols(&mut self, from: u32, delta: i64) {
        if delta == 0 {
            return;
        }
        let affected: Vec<u32> =
            self.bands.iter().filter(|(_, b)| b.rows.values().any(|row| !row.range(from, u32::MAX).is_empty())).map(|(k, _)| *k).collect();
        for k in affected {
            let Some(band) = self.bands.get_mut(&k) else { continue };
            for row in Arc::make_mut(band).rows.values_mut() {
                let moved = row.split_off(from);
                for (c, cell) in moved {
                    let nc = c as i64 + delta;
                    if (0..gridcraft_core::MAX_COLS as i64).contains(&nc) {
                        row.insert(nc as u32, cell);
                    }
                }
            }
            if let Some(band) = self.bands.get_mut(&k) {
                Arc::make_mut(band).rows.retain(|_, row| !row.is_empty());
            }
        }
        self.bands.retain(|_, b| !b.rows.is_empty());
        self.recount();
    }
    pub fn shift_cols_in_rows(&mut self, r0: u32, r1: u32, from: u32, delta: i64) {
        let range = RangeRef::new(CellRef::new(r0, from), CellRef::new(r1, gridcraft_core::MAX_COLS - 1));
        let moved = self.take_range(range);
        for (c, cell) in moved {
            let nc = c.col as i64 + delta;
            if (0..gridcraft_core::MAX_COLS as i64).contains(&nc) {
                self.set(CellRef::new(c.row, nc as u32), cell);
            }
        }
    }
    fn recount(&mut self) {
        self.len = self.bands.values().flat_map(|b| b.rows.values()).map(Row::len).sum();
    }
    pub fn shift_rows_in_cols(&mut self, c0: u32, c1: u32, from: u32, delta: i64) {
        let range = RangeRef::new(CellRef::new(from, c0), CellRef::new(gridcraft_core::MAX_ROWS - 1, c1));
        let moved = self.take_range(range);
        for (c, cell) in moved {
            let nr = c.row as i64 + delta;
            if (0..gridcraft_core::MAX_ROWS as i64).contains(&nr) {
                self.set(CellRef::new(nr as u32, c.col), cell);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::Value;

    fn c(r: u32, col: u32) -> CellRef {
        CellRef::new(r, col)
    }

    #[test]
    fn set_get_remove() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(1.0.into()));
        s.set(c(100, 5), Cell::value("x".into()));
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(c(100, 5)).map(|x| x.value.clone()), Some(Value::from("x")));
        assert_eq!(s.used_range().unwrap().a1(), "A1:F101");
        s.set(c(0, 0), Cell::default());
        assert_eq!(s.len(), 1);
        assert!(s.remove(c(100, 5)).is_some());
        assert!(s.is_empty());
        assert!(s.used_range().is_none());
    }

    #[test]
    fn cow_snapshots() {
        let mut s = CellStore::new();
        for r in 0..1000 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        let snap = s.clone();
        s.set(c(5, 0), Cell::value(99.0.into()));
        assert_eq!(snap.get(c(5, 0)).unwrap().value, Value::Number(5.0));
        assert_eq!(s.get(c(5, 0)).unwrap().value, Value::Number(99.0));
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
    fn range_iteration_order() {
        let mut s = CellStore::new();
        s.set(c(70, 2), Cell::value(3.0.into()));
        s.set(c(1, 1), Cell::value(1.0.into()));
        s.set(c(1, 3), Cell::value(2.0.into()));
        s.set(c(500, 0), Cell::value(4.0.into()));
        let v: Vec<f64> = s.iter_range(RangeRef::parse("A1:D100").unwrap()).map(|(_, x)| x.value.as_f64().unwrap()).collect();
        assert_eq!(v, [1.0, 2.0, 3.0]);
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

    #[test]
    fn shifts_move_cells_and_moved_cells_win() {
        let v = |n: f64| Cell::value(gridcraft_core::Value::Number(n));
        let mut s = CellStore::new();
        for (r, c, n) in [(0, 0, 1.0), (3, 0, 2.0), (3, 1, 3.0), (5, 0, 4.0), (70, 2, 5.0), (200, 0, 6.0)] {
            s.set(CellRef::new(r, c), v(n));
        }
        let snapshot = s.clone(); // shared bands, as an undo step holds them
        // Delete rows 4-5 (shift up by 2 from row 6): row 5 lands on row 3 and replaces A4.
        s.shift_rows(5, -2);
        let at = |s: &CellStore, r, c| s.get(CellRef::new(r, c)).map(|x| x.value.clone());
        assert_eq!(at(&s, 3, 0), Some(gridcraft_core::Value::Number(4.0)));
        assert_eq!(at(&s, 3, 1), Some(gridcraft_core::Value::Number(3.0)));
        assert_eq!(at(&s, 68, 2), Some(gridcraft_core::Value::Number(5.0)));
        assert_eq!(at(&s, 198, 0), Some(gridcraft_core::Value::Number(6.0)));
        assert_eq!(s.len(), 5);
        assert_eq!(snapshot.len(), 6);
        assert_eq!(at(&snapshot, 5, 0), Some(gridcraft_core::Value::Number(4.0)));
        // Insert two rows at 1: everything from row 1 moves down; past the edge is dropped.
        s.shift_rows(1, 2);
        assert_eq!(at(&s, 0, 0), Some(gridcraft_core::Value::Number(1.0)));
        assert_eq!(at(&s, 5, 0), Some(gridcraft_core::Value::Number(4.0)));
        s.shift_rows(0, i64::from(gridcraft_core::MAX_ROWS) - 1);
        assert_eq!(s.len(), 1);
        // Columns: delete column B, then C lands on B; insert one at A.
        let mut t = CellStore::new();
        t.set(CellRef::new(0, 1), v(1.0));
        t.set(CellRef::new(0, 2), v(2.0));
        t.set(CellRef::new(9, 3), v(3.0));
        t.shift_cols(2, -1);
        assert_eq!(at(&t, 0, 1), Some(gridcraft_core::Value::Number(2.0)));
        assert_eq!(at(&t, 9, 2), Some(gridcraft_core::Value::Number(3.0)));
        assert_eq!(t.len(), 2);
        t.shift_cols(0, 1);
        assert_eq!((at(&t, 0, 2), at(&t, 9, 3), t.len()), (Some(gridcraft_core::Value::Number(2.0)), Some(gridcraft_core::Value::Number(3.0)), 2));
    }

    #[test]
    fn rows_serialize_as_column_maps() {
        // Saved JSON workbooks hold rows as column → cell maps: the vector rows read and write
        // the same shape.
        let mut s = CellStore::new();
        s.set(c(3, 7), Cell::value(1.0.into()));
        s.set(c(3, 2), Cell::value("x".into()));
        let row = s.bands.get(&0).and_then(|b| b.rows.get(&3)).cloned().unwrap();
        let as_map: BTreeMap<u32, Cell> = row.clone().into();
        assert_eq!(serde_json::to_value(&row).unwrap(), serde_json::to_value(&as_map).unwrap());
        let json = serde_json::to_string(&s).unwrap();
        let back: CellStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.row(3, 0, 10).map(|(col, _)| col).collect::<Vec<_>>(), [2, 7]);
    }
}
