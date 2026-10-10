//! Reference adjustment: copying formulas, inserting/deleting rows and columns, moving cells,
//! renaming and deleting sheets.

use gridcraft_core::{CellError, MAX_COLS, MAX_ROWS, RangeRef};

use crate::ast::*;

/// Copy/fill: relative parts move by `(dr, dc)`. References pushed off the sheet become `#REF!`.
pub fn shift_relative(e: Expr, dr: i64, dc: i64) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) => match shift_ref(&r, dr, dc) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        other => other,
    })
}

fn shift_anchor(a: &Anchor, dr: i64, dc: i64) -> Option<Anchor> {
    let row = if a.row_abs { a.row as i64 } else { a.row as i64 + dr };
    let col = if a.col_abs { a.col as i64 } else { a.col as i64 + dc };
    let (row, col) = clamp_anchor(row, col)?;
    Some(Anchor { row, col, ..*a })
}

fn shift_line(v: u32, abs: bool, d: i64, max: u32) -> Option<u32> {
    if abs {
        return Some(v);
    }
    let n = v as i64 + d;
    if (0..max as i64).contains(&n) { Some(n as u32) } else { None }
}

fn shift_ref(r: &Reference, dr: i64, dc: i64) -> Option<Reference> {
    let kind = match &r.kind {
        RefKind::Cell(a) => RefKind::Cell(shift_anchor(a, dr, dc)?),
        RefKind::Range(a, b) => RefKind::Range(shift_anchor(a, dr, dc)?, shift_anchor(b, dr, dc)?),
        RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(shift_line(*r0, *a0, dr, MAX_ROWS)?, *a0, shift_line(*r1, *a1, dr, MAX_ROWS)?, *a1),
        RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(shift_line(*c0, *a0, dc, MAX_COLS)?, *a0, shift_line(*c1, *a1, dc, MAX_COLS)?, *a1),
    };
    Some(Reference { sheet: r.sheet.clone(), kind })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// A structural edit on one sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Insert `count` rows/cols before index `at`.
    Insert { axis: Axis, at: u32, count: u32 },
    /// Delete `count` rows/cols starting at `at`.
    Delete { axis: Axis, at: u32, count: u32 },
    /// Cut-paste of a block: references into `from` now point into the block at `to` (top-left).
    Move { from: RangeRef, to_row: u32, to_col: u32 },
    /// Insert Cells shifting down (`Rows`) or right (`Cols`): the cells of `range` and those
    /// below (right of) it move by its height (width), within its columns (rows) only.
    InsertCells { axis: Axis, range: RangeRef },
    /// Delete Cells shifting up (`Rows`) or left (`Cols`): `range` goes and the cells below
    /// (right of) it move back by its height (width), within its columns (rows) only.
    DeleteCells { axis: Axis, range: RangeRef },
}

/// Adjusts a formula living on sheet `host` after `edit` on sheet `target`.
pub fn adjust(e: Expr, host: &str, target: &str, edit: &Edit) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) => {
            let on_target = match &r.sheet {
                SheetSel::Current => host.eq_ignore_ascii_case(target),
                SheetSel::Named(n) => n.eq_ignore_ascii_case(target),
                SheetSel::Span(..) => false,
            };
            if !on_target {
                return Expr::Ref(r);
            }
            match adjust_ref(&r, edit) {
                Some(r) => Expr::Ref(r),
                None => Expr::Error(CellError::Ref),
            }
        }
        other => other,
    })
}

/// New position of an index after an insert (`None` never) or delete (`None` = deleted).
fn map_index(v: u32, at: u32, count: u32, insert: bool, max: u32) -> Option<u32> {
    if insert {
        if v >= at {
            let n = v as u64 + count as u64;
            if n >= max as u64 { None } else { Some(n as u32) }
        } else {
            Some(v)
        }
    } else if v < at {
        Some(v)
    } else if v >= at.saturating_add(count) {
        Some(v - count)
    } else {
        None
    }
}

/// Adjusts a span `[lo, hi]` (inclusive) on one axis.
fn map_span(lo: u32, hi: u32, at: u32, count: u32, insert: bool, max: u32) -> Option<(u32, u32)> {
    if insert {
        let lo2 = if lo >= at { lo as u64 + count as u64 } else { lo as u64 };
        let hi2 = if hi >= at { hi as u64 + count as u64 } else { hi as u64 };
        // Whole-sheet spans (e.g. A:A) stay whole.
        let hi2 = hi2.min(max as u64 - 1);
        if lo2 > hi2 {
            return None;
        }
        Some((lo2 as u32, hi2 as u32))
    } else {
        let end = at.saturating_add(count); // exclusive
        if lo >= at && hi < end {
            return None; // fully deleted
        }
        let lo2 = if lo < at {
            lo
        } else if lo >= end {
            lo - count
        } else {
            at
        };
        let hi2 = if hi < at {
            hi
        } else if hi >= end {
            hi - count
        } else {
            at.saturating_sub(1)
        };
        Some((lo2, hi2.max(lo2)))
    }
}

fn adjust_ref(r: &Reference, edit: &Edit) -> Option<Reference> {
    match edit {
        Edit::Insert { axis, at, count } | Edit::Delete { axis, at, count } => {
            let insert = matches!(edit, Edit::Insert { .. });
            let kind = match (&r.kind, axis) {
                (RefKind::Cell(a), Axis::Rows) => RefKind::Cell(Anchor { row: map_index(a.row, *at, *count, insert, MAX_ROWS)?, ..*a }),
                (RefKind::Cell(a), Axis::Cols) => RefKind::Cell(Anchor { col: map_index(a.col, *at, *count, insert, MAX_COLS)?, ..*a }),
                (RefKind::Range(a, b), Axis::Rows) => {
                    let (lo, hi) = map_span(a.row.min(b.row), a.row.max(b.row), *at, *count, insert, MAX_ROWS)?;
                    RefKind::Range(Anchor { row: lo, ..*a }, Anchor { row: hi, ..*b })
                }
                (RefKind::Range(a, b), Axis::Cols) => {
                    let (lo, hi) = map_span(a.col.min(b.col), a.col.max(b.col), *at, *count, insert, MAX_COLS)?;
                    RefKind::Range(Anchor { col: lo, ..*a }, Anchor { col: hi, ..*b })
                }
                (RefKind::Rows(r0, a0, r1, a1), Axis::Rows) => {
                    let (lo, hi) = map_span(*r0.min(r1), *r0.max(r1), *at, *count, insert, MAX_ROWS)?;
                    RefKind::Rows(lo, *a0, hi, *a1)
                }
                (RefKind::Cols(c0, a0, c1, a1), Axis::Cols) => {
                    let (lo, hi) = map_span(*c0.min(c1), *c0.max(c1), *at, *count, insert, MAX_COLS)?;
                    RefKind::Cols(lo, *a0, hi, *a1)
                }
                (k, _) => k.clone(),
            };
            Some(Reference { sheet: r.sheet.clone(), kind })
        }
        Edit::InsertCells { axis, range } | Edit::DeleteCells { axis, range } => {
            // Like inserting/deleting lines, for references that lie within the shifted
            // columns (rows); Excel leaves references reaching outside them as they are.
            let span = r.range();
            let (inside, at, count) = match axis {
                Axis::Rows => (range.start.col <= span.start.col && span.end.col <= range.end.col, range.start.row, range.height()),
                Axis::Cols => (range.start.row <= span.start.row && span.end.row <= range.end.row, range.start.col, range.width()),
            };
            if !inside {
                return Some(r.clone());
            }
            let axis = *axis;
            let lines = if matches!(edit, Edit::InsertCells { .. }) { Edit::Insert { axis, at, count } } else { Edit::Delete { axis, at, count } };
            adjust_ref(r, &lines)
        }
        Edit::Move { from, to_row, to_col } => {
            let dr = *to_row as i64 - from.start.row as i64;
            let dc = *to_col as i64 - from.start.col as i64;
            let range = r.range();
            if !from.contains_range(&range) {
                return Some(r.clone());
            }
            let mv = |a: &Anchor| -> Option<Anchor> {
                let (row, col) = clamp_anchor(a.row as i64 + dr, a.col as i64 + dc)?;
                Some(Anchor { row, col, ..*a })
            };
            let kind = match &r.kind {
                RefKind::Cell(a) => RefKind::Cell(mv(a)?),
                RefKind::Range(a, b) => RefKind::Range(mv(a)?, mv(b)?),
                k => k.clone(),
            };
            Some(Reference { sheet: r.sheet.clone(), kind })
        }
    }
}

/// Renames sheet references `old` → `new`.
pub fn rename_sheet(e: Expr, old: &str, new: &str) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(mut r) => {
            match &mut r.sheet {
                SheetSel::Named(n) if n.eq_ignore_ascii_case(old) => *n = new.to_string(),
                SheetSel::Span(a, b) => {
                    if a.eq_ignore_ascii_case(old) {
                        *a = new.to_string();
                    }
                    if b.eq_ignore_ascii_case(old) {
                        *b = new.to_string();
                    }
                }
                _ => {}
            }
            Expr::Ref(r)
        }
        other => other,
    })
}

/// References to a deleted sheet become `#REF!`.
pub fn delete_sheet(e: Expr, name: &str) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) if r.sheet_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) => Expr::Error(CellError::Ref),
        other => other,
    })
}
