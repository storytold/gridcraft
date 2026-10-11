//! Cell and range addresses.
//!
//! Internally rows and columns are 0-based (`A1` = row 0, col 0). The sheet is 1,048,576 rows by
//! 16,384 columns (`XFD1048576`), like Excel.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_ROWS: u32 = 1_048_576;
pub const MAX_COLS: u32 = 16_384;

/// A cell position (0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct CellRef {
    pub row: u32,
    pub col: u32,
}

impl CellRef {
    pub const fn new(row: u32, col: u32) -> Self {
        CellRef { row, col }
    }
    /// `A1`-style text, e.g. `B7`.
    pub fn a1(&self) -> String {
        format!("{}{}", col_to_letters(self.col), self.row as u64 + 1)
    }
    pub fn is_valid(&self) -> bool {
        self.row < MAX_ROWS && self.col < MAX_COLS
    }
    /// Moved by `(dr, dc)`, `None` when the result leaves the sheet.
    pub fn offset(&self, dr: i64, dc: i64) -> Option<CellRef> {
        let r = (self.row as i64).checked_add(dr)?;
        let c = (self.col as i64).checked_add(dc)?;
        if (0..MAX_ROWS as i64).contains(&r) && (0..MAX_COLS as i64).contains(&c) { Some(CellRef::new(r as u32, c as u32)) } else { None }
    }
    /// Moved by `(dr, dc)` and clamped to the sheet.
    pub fn offset_clamped(&self, dr: i64, dc: i64) -> CellRef {
        let r = (self.row as i64).saturating_add(dr).clamp(0, MAX_ROWS as i64 - 1);
        let c = (self.col as i64).saturating_add(dc).clamp(0, MAX_COLS as i64 - 1);
        CellRef::new(r as u32, c as u32)
    }
    /// Parses `A1`, `$A$1`, `xfd1048576` (case-insensitive). `None` if malformed or out of range.
    pub fn parse(s: &str) -> Option<CellRef> {
        let (c, r, _, _, rest) = parse_a1_prefix(s)?;
        if !rest.is_empty() {
            return None;
        }
        Some(CellRef::new(r, c))
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", col_to_letters(self.col), self.row as u64 + 1)
    }
}

/// Parses an A1 reference at the start of `s`: `(col, row, col_abs, row_abs, rest)`.
pub fn parse_a1_prefix(s: &str) -> Option<(u32, u32, bool, bool, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    let col_abs = b.first() == Some(&b'$');
    if col_abs {
        i += 1;
    }
    let cs = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == cs || i - cs > 3 {
        return None;
    }
    let col = letters_to_col(s.get(cs..i)?)?;
    let row_abs = b.get(i) == Some(&b'$');
    if row_abs {
        i += 1;
    }
    let rs = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == rs || i - rs > 7 {
        return None;
    }
    let row: u32 = s.get(rs..i)?.parse().ok()?;
    if row == 0 || row > MAX_ROWS {
        return None;
    }
    Some((col, row - 1, col_abs, row_abs, s.get(i..)?))
}

/// 0 → `A`, 25 → `Z`, 26 → `AA`, 16383 → `XFD`.
pub fn col_to_letters(col: u32) -> String {
    let mut n = col as u64 + 1;
    let mut out = Vec::with_capacity(3);
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(b'A' + rem);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `A` → 0, `xfd` → 16383. `None` for empty, non-letters or beyond `XFD`.
pub fn letters_to_col(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for ch in s.bytes() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (ch.to_ascii_uppercase() - b'A' + 1) as u32;
    }
    if n == 0 || n > MAX_COLS {
        return None;
    }
    Some(n - 1)
}

/// A rectangular block of cells, inclusive on both ends. Always normalised (`start <= end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct RangeRef {
    pub start: CellRef,
    pub end: CellRef,
}

impl RangeRef {
    pub fn new(a: CellRef, b: CellRef) -> Self {
        RangeRef { start: CellRef::new(a.row.min(b.row), a.col.min(b.col)), end: CellRef::new(a.row.max(b.row), a.col.max(b.col)) }
    }
    pub fn cell(c: CellRef) -> Self {
        RangeRef { start: c, end: c }
    }
    /// Whole rows `r0..=r1`.
    pub fn rows(r0: u32, r1: u32) -> Self {
        RangeRef::new(CellRef::new(r0, 0), CellRef::new(r1, MAX_COLS - 1))
    }
    /// Whole columns `c0..=c1`.
    pub fn cols(c0: u32, c1: u32) -> Self {
        RangeRef::new(CellRef::new(0, c0), CellRef::new(MAX_ROWS - 1, c1))
    }
    pub fn all() -> Self {
        RangeRef::new(CellRef::new(0, 0), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1))
    }
    pub fn height(&self) -> u32 {
        self.end.row - self.start.row + 1
    }
    pub fn width(&self) -> u32 {
        self.end.col - self.start.col + 1
    }
    pub fn count(&self) -> u64 {
        self.height() as u64 * self.width() as u64
    }
    pub fn is_single(&self) -> bool {
        self.start == self.end
    }
    pub fn is_full_rows(&self) -> bool {
        self.start.col == 0 && self.end.col == MAX_COLS - 1
    }
    pub fn is_full_cols(&self) -> bool {
        self.start.row == 0 && self.end.row == MAX_ROWS - 1
    }
    pub fn contains(&self, c: CellRef) -> bool {
        c.row >= self.start.row && c.row <= self.end.row && c.col >= self.start.col && c.col <= self.end.col
    }
    pub fn contains_range(&self, o: &RangeRef) -> bool {
        self.contains(o.start) && self.contains(o.end)
    }
    pub fn intersects(&self, o: &RangeRef) -> bool {
        self.start.row <= o.end.row && o.start.row <= self.end.row && self.start.col <= o.end.col && o.start.col <= self.end.col
    }
    pub fn intersection(&self, o: &RangeRef) -> Option<RangeRef> {
        if !self.intersects(o) {
            return None;
        }
        Some(RangeRef {
            start: CellRef::new(self.start.row.max(o.start.row), self.start.col.max(o.start.col)),
            end: CellRef::new(self.end.row.min(o.end.row), self.end.col.min(o.end.col)),
        })
    }
    pub fn union(&self, o: &RangeRef) -> RangeRef {
        RangeRef {
            start: CellRef::new(self.start.row.min(o.start.row), self.start.col.min(o.start.col)),
            end: CellRef::new(self.end.row.max(o.end.row), self.end.col.max(o.end.col)),
        }
    }
    /// Row-major iterator over the cells. Callers must bound huge ranges themselves.
    pub fn iter(&self) -> impl Iterator<Item = CellRef> + '_ {
        let (r0, r1, c0, c1) = (self.start.row, self.end.row, self.start.col, self.end.col);
        (r0..=r1).flat_map(move |r| (c0..=c1).map(move |c| CellRef::new(r, c)))
    }
    /// `A1:B2`, `A1` for single cells, `1:3` for full rows, `A:C` for full columns.
    pub fn a1(&self) -> String {
        if self.is_full_rows() && !self.is_full_cols() {
            return format!("{}:{}", self.start.row + 1, self.end.row + 1);
        }
        if self.is_full_cols() && !self.is_full_rows() {
            return format!("{}:{}", col_to_letters(self.start.col), col_to_letters(self.end.col));
        }
        if self.is_single() { self.start.a1() } else { format!("{}:{}", self.start.a1(), self.end.a1()) }
    }
    /// Parses `A1`, `A1:B2`, `$A$1:$B$2`, `A:C`, `3:5` (no sheet prefix).
    pub fn parse(s: &str) -> Option<RangeRef> {
        let s = s.trim();
        if let Some((a, b)) = s.split_once(':') {
            if let (Some(x), Some(y)) = (CellRef::parse(a), CellRef::parse(b)) {
                return Some(RangeRef::new(x, y));
            }
            let strip = |t: &str| t.trim_start_matches('$').to_string();
            if let (Some(x), Some(y)) = (letters_to_col(&strip(a)), letters_to_col(&strip(b))) {
                return Some(RangeRef::cols(x.min(y), x.max(y)));
            }
            let (pa, pb) = (strip(a).parse::<u32>().ok()?, strip(b).parse::<u32>().ok()?);
            if pa == 0 || pb == 0 || pa > MAX_ROWS || pb > MAX_ROWS {
                return None;
            }
            return Some(RangeRef::rows(pa.min(pb) - 1, pa.max(pb) - 1));
        }
        CellRef::parse(s).map(RangeRef::cell)
    }
}

impl fmt::Display for RangeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.a1())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_roundtrip() {
        for (c, s) in [(0, "A"), (25, "Z"), (26, "AA"), (51, "AZ"), (52, "BA"), (701, "ZZ"), (702, "AAA"), (16383, "XFD")] {
            assert_eq!(col_to_letters(c), s);
            assert_eq!(letters_to_col(s), Some(c));
        }
        assert_eq!(letters_to_col("XFE"), None);
        assert_eq!(letters_to_col(""), None);
        assert_eq!(letters_to_col("A1"), None);
    }

    #[test]
    fn parse_cells() {
        assert_eq!(CellRef::parse("B7"), Some(CellRef::new(6, 1)));
        assert_eq!(CellRef::parse("$b$7"), Some(CellRef::new(6, 1)));
        assert_eq!(CellRef::parse("XFD1048576"), Some(CellRef::new(MAX_ROWS - 1, MAX_COLS - 1)));
        assert_eq!(CellRef::parse("A0"), None);
        assert_eq!(CellRef::parse("A1048577"), None);
        assert_eq!(CellRef::parse("A1x"), None);
        assert_eq!(CellRef::parse(""), None);
        assert_eq!(CellRef::parse("$"), None);
    }

    #[test]
    fn ranges() {
        let r = RangeRef::parse("C3:A1").unwrap();
        assert_eq!(r.a1(), "A1:C3");
        assert_eq!(r.count(), 9);
        assert_eq!(RangeRef::parse("A:C").unwrap().a1(), "A:C");
        assert_eq!(RangeRef::parse("3:5").unwrap().a1(), "3:5");
        assert!(RangeRef::parse("0:5").is_none());
        let a = RangeRef::parse("A1:C3").unwrap();
        let b = RangeRef::parse("B2:D4").unwrap();
        assert_eq!(a.intersection(&b).unwrap().a1(), "B2:C3");
        assert_eq!(a.union(&b).a1(), "A1:D4");
        assert_eq!(a.iter().count(), 9);
    }

    #[test]
    fn extreme_offsets() {
        let cell = CellRef::new(1, 1);
        for delta in [i64::MIN, i64::MAX] {
            assert_eq!(cell.offset(delta, 0), None);
            assert_eq!(cell.offset(0, delta), None);
            assert_eq!(cell.offset(delta, delta), None);
        }
        assert_eq!(cell.offset_clamped(i64::MAX, i64::MAX), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1));
        assert_eq!(cell.offset_clamped(i64::MIN, i64::MIN), CellRef::new(0, 0));
        assert_eq!(cell.offset_clamped(i64::MAX, i64::MIN), CellRef::new(MAX_ROWS - 1, 0));
        assert_eq!(cell.offset_clamped(i64::MIN, i64::MAX), CellRef::new(0, MAX_COLS - 1));
        assert_eq!(cell.offset(2, -1), Some(CellRef::new(3, 0)));
    }

    #[test]
    fn offsets() {
        assert_eq!(CellRef::new(0, 0).offset(-1, 0), None);
        assert_eq!(CellRef::new(0, 0).offset_clamped(-5, 3), CellRef::new(0, 3));
    }
}
