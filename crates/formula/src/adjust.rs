//! Reference adjustment: copying formulas, inserting/deleting rows and columns, moving cells,
//! renaming and deleting sheets.

use gridcraft_core::{CellError, MAX_COLS, MAX_ROWS, RangeRef};

use crate::ast::{Anchor, Expr, RefKind, Reference, SheetSel, clamp_anchor};

/// Copy/fill: relative parts move by `(dr, dc)`. References pushed off the sheet become `#REF!`.
pub fn shift_relative(e: Expr, dr: i64, dc: i64) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) => match shift_ref(r, dr, dc) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        other => other,
    })
}

/// Defined names: relative parts move by `(dr, dc)`, wrapping around the sheet's edges. Files
/// store a name's relative references as seen from A1 (a name for "the cell to the left" defined
/// in B1 is `Sheet1!XFD1`), and a use in another cell moves them by that cell's offset from A1.
pub fn shift_relative_wrapping(e: Expr, dr: i64, dc: i64) -> Expr {
    let wrap = |v: u32, abs: bool, d: i64, max: u32| if abs { v } else { (v as i64 + d).rem_euclid(max as i64) as u32 };
    let anchor = |a: Anchor| Anchor { row: wrap(a.row, a.row_abs, dr, MAX_ROWS), col: wrap(a.col, a.col_abs, dc, MAX_COLS), ..a };
    e.map(&mut |x| match x {
        Expr::Ref(r) => {
            let kind = match r.kind {
                RefKind::Cell(a) => RefKind::Cell(anchor(a)),
                RefKind::Range(a, b) => RefKind::Range(anchor(a), anchor(b)),
                RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(wrap(r0, a0, dr, MAX_ROWS), a0, wrap(r1, a1, dr, MAX_ROWS), a1),
                RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(wrap(c0, a0, dc, MAX_COLS), a0, wrap(c1, a1, dc, MAX_COLS), a1),
            };
            Expr::Ref(Reference { sheet: r.sheet, kind })
        }
        other => other,
    })
}

/// Whether the formula has a relative (no `$`) row or column anywhere.
pub fn has_relative_refs(e: &Expr) -> bool {
    e.references().iter().any(|r| match r.kind {
        RefKind::Cell(a) => !a.row_abs || !a.col_abs,
        RefKind::Range(a, b) => !a.row_abs || !a.col_abs || !b.row_abs || !b.col_abs,
        RefKind::Rows(_, a0, _, a1) | RefKind::Cols(_, a0, _, a1) => !a0 || !a1,
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

fn shift_ref(r: Reference, dr: i64, dc: i64) -> Option<Reference> {
    let kind = match &r.kind {
        RefKind::Cell(a) => RefKind::Cell(shift_anchor(a, dr, dc)?),
        RefKind::Range(a, b) => RefKind::Range(shift_anchor(a, dr, dc)?, shift_anchor(b, dr, dc)?),
        RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(shift_line(*r0, *a0, dr, MAX_ROWS)?, *a0, shift_line(*r1, *a1, dr, MAX_ROWS)?, *a1),
        RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(shift_line(*c0, *a0, dc, MAX_COLS)?, *a0, shift_line(*c1, *a1, dc, MAX_COLS)?, *a1),
    };
    Some(Reference { sheet: r.sheet, kind })
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
    /// Reorder whole lines as one reference edit. Ranges enclosing the affected interval stay intact.
    Reorder { axis: Axis, from: u32, count: u32, before: u32 },
    /// Cut-paste of a block: references into `from` now point into the block at `to` (top-left).
    Move { from: RangeRef, to_row: u32, to_col: u32 },
    /// Insert Cells shifting down (`Rows`) or right (`Cols`): the cells of `range` and those
    /// below (right of) it move by its height (width), within its columns (rows) only.
    InsertCells { axis: Axis, range: RangeRef },
    /// Delete Cells shifting up (`Rows`) or left (`Cols`): `range` goes and the cells below
    /// (right of) it move back by its height (width), within its columns (rows) only.
    DeleteCells { axis: Axis, range: RangeRef },
    /// Cut-paste to another sheet; fully contained references follow the destination sheet too.
    MoveToSheet { from: RangeRef, to_sheet: String, to_row: u32, to_col: u32 },
}

/// Whether `r`, in a formula living on sheet `host`, points into sheet `target`.
fn on_target(r: &Reference, host: &str, target: &str) -> bool {
    match &r.sheet {
        SheetSel::Current => host.eq_ignore_ascii_case(target),
        SheetSel::Named(n) => n.eq_ignore_ascii_case(target),
        SheetSel::Span(..) => false,
    }
}

/// Whether [`adjust`] would change `r`. It runs the same [`adjust_ref`] on a copy without the
/// sheet name, so no name is copied: the adjustment reads only the position, and the sheet a cut
/// to another sheet assigns is compared afterwards.
fn ref_changes(r: &Reference, edit: &Edit) -> bool {
    let probe = Reference { sheet: SheetSel::Current, kind: r.kind.clone() };
    match adjust_ref(probe, edit) {
        None => true,
        Some(p) => p.kind != r.kind || (p.sheet != SheetSel::Current && p.sheet != r.sheet),
    }
}

/// Whether [`adjust`] would change the formula `e` at all, without copying or rebuilding it.
fn changes(e: &Expr, host: &str, target: &str, edit: &Edit) -> bool {
    let mut changed = false;
    e.walk(&mut |x| {
        if !changed && let Expr::Ref(r) = x {
            changed = on_target(r, host, target) && ref_changes(r, edit);
        }
    });
    changed
}

/// Adjusts a formula living on sheet `host` after `edit` on sheet `target`.
pub fn adjust(e: Expr, host: &str, target: &str, edit: &Edit) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) if on_target(&r, host, target) => match adjust_ref(r, edit) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        other => other,
    })
}

/// [`adjust`] for a borrowed formula: `None` when the edit leaves it exactly as it is, so a
/// caller can skip unaffected formulas without copying, rebuilding or printing them.
pub fn adjust_if_changed(e: &Expr, host: &str, target: &str, edit: &Edit) -> Option<Expr> {
    changes(e, host, target, edit).then(|| adjust(e.clone(), host, target, edit))
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

/// A reference wholly inside the moved lines follows those lines directly. This also covers a
/// source block at the sheet edge, which cannot be represented by the intermediate Insert.
fn move_reorder_source_ref(r: &Reference, axis: Axis, from: u32, end: u32, to: u32) -> Option<Reference> {
    let mut moved = r.clone();
    let line = |v: u32| to + (v - from);
    match (&mut moved.kind, axis) {
        (RefKind::Cell(a), Axis::Rows) if (from..end).contains(&a.row) => a.row = line(a.row),
        (RefKind::Cell(a), Axis::Cols) if (from..end).contains(&a.col) => a.col = line(a.col),
        (RefKind::Range(a, b), Axis::Rows) if a.row.min(b.row) >= from && a.row.max(b.row) < end => {
            a.row = line(a.row);
            b.row = line(b.row);
        }
        (RefKind::Range(a, b), Axis::Cols) if a.col.min(b.col) >= from && a.col.max(b.col) < end => {
            a.col = line(a.col);
            b.col = line(b.col);
        }
        (RefKind::Rows(a, _, b, _), Axis::Rows) if (*a).min(*b) >= from && (*a).max(*b) < end => {
            *a = line(*a);
            *b = line(*b);
        }
        (RefKind::Cols(a, _, b, _), Axis::Cols) if (*a).min(*b) >= from && (*a).max(*b) < end => {
            *a = line(*a);
            *b = line(*b);
        }
        _ => return None,
    }
    Some(moved)
}

fn adjust_ref(r: Reference, edit: &Edit) -> Option<Reference> {
    match edit {
        Edit::Reorder { axis, from, count, before } => {
            let limit = if *axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
            let end = from.checked_add(*count).filter(|end| *end <= limit)?;
            if *before >= limit || before.checked_add(*count).is_none_or(|end| end > limit) {
                return None;
            }
            if *count == 0 || (*from..=end).contains(before) {
                return Some(r);
            }
            let span = r.range();
            let (lo, hi) = if *axis == Axis::Rows { (span.start.row, span.end.row) } else { (span.start.col, span.end.col) };
            // Reordering inside a referenced interval changes no member of that interval.
            // Applying delete separately would wrongly shrink a range ending at the drop boundary.
            if lo <= (*from).min(*before) && hi >= end.max(*before) - 1 {
                return Some(r);
            }
            let final_start = if before < from { *before } else { *before - *count };
            if let Some(moved) = move_reorder_source_ref(&r, *axis, *from, end, final_start) {
                return Some(moved);
            }
            // Fully contained references followed their source above. All remaining references
            // use the existing partial-overlap Insert/Delete semantics, with enough temporary
            // capacity for the Insert. Do not clamp or relocate sheet-edge endpoints before the
            // Delete restores the real sheet extent: that could change range containment.
            let moved_from = if before <= from { from.checked_add(*count)? } else { *from };
            let temporary_limit = limit.checked_add(*count)?;
            let line = |v| {
                let inserted = map_index(v, *before, *count, true, temporary_limit)?;
                map_index(inserted, moved_from, *count, false, limit)
            };
            let span = |a: u32, b: u32| {
                let (lo, hi) = map_span(a.min(b), a.max(b), *before, *count, true, temporary_limit)?;
                let (lo, hi) = map_span(lo, hi, moved_from, *count, false, limit)?;
                Some(if a <= b { (lo, hi) } else { (hi, lo) })
            };
            let kind = match (&r.kind, axis) {
                (RefKind::Cell(a), Axis::Rows) => RefKind::Cell(Anchor { row: line(a.row)?, ..*a }),
                (RefKind::Cell(a), Axis::Cols) => RefKind::Cell(Anchor { col: line(a.col)?, ..*a }),
                (RefKind::Range(a, b), Axis::Rows) => {
                    let (first, last) = span(a.row, b.row)?;
                    RefKind::Range(Anchor { row: first, ..*a }, Anchor { row: last, ..*b })
                }
                (RefKind::Range(a, b), Axis::Cols) => {
                    let (first, last) = span(a.col, b.col)?;
                    RefKind::Range(Anchor { col: first, ..*a }, Anchor { col: last, ..*b })
                }
                (RefKind::Rows(a, a_abs, b, b_abs), Axis::Rows) => {
                    let (first, last) = span(*a, *b)?;
                    RefKind::Rows(first, *a_abs, last, *b_abs)
                }
                (RefKind::Cols(a, a_abs, b, b_abs), Axis::Cols) => {
                    let (first, last) = span(*a, *b)?;
                    RefKind::Cols(first, *a_abs, last, *b_abs)
                }
                (kind, _) => kind.clone(),
            };
            Some(Reference { sheet: r.sheet, kind })
        }
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
            Some(Reference { sheet: r.sheet, kind })
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
                return Some(r);
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
                return Some(r);
            }
            let mv = |a: &Anchor| -> Option<Anchor> {
                let (row, col) = clamp_anchor(a.row as i64 + dr, a.col as i64 + dc)?;
                Some(Anchor { row, col, ..*a })
            };
            let kind = match &r.kind {
                RefKind::Cell(a) => RefKind::Cell(mv(a)?),
                RefKind::Range(a, b) => RefKind::Range(mv(a)?, mv(b)?),
                RefKind::Rows(a, aa, b, ba) => {
                    let dr = *to_row as i64 - from.start.row as i64;
                    RefKind::Rows(shift_line(*a, false, dr, MAX_ROWS)?, *aa, shift_line(*b, false, dr, MAX_ROWS)?, *ba)
                }
                RefKind::Cols(a, aa, b, ba) => {
                    let dc = *to_col as i64 - from.start.col as i64;
                    RefKind::Cols(shift_line(*a, false, dc, MAX_COLS)?, *aa, shift_line(*b, false, dc, MAX_COLS)?, *ba)
                }
            };
            Some(Reference { sheet: r.sheet, kind })
        }
        Edit::MoveToSheet { from, to_sheet, to_row, to_col } => {
            if !from.contains_range(&r.range()) {
                return Some(r);
            }
            let mut moved = adjust_ref(r, &Edit::Move { from: *from, to_row: *to_row, to_col: *to_col })?;
            moved.sheet = SheetSel::Named(to_sheet.clone());
            Some(moved)
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
        // Sheet-qualified defined name `Old!MyName` is stored as one string, with the bare sheet name as the parser
        // stores it (the printer adds the quotes).
        Expr::Name(n) => match n.split_at_checked(old.len()) {
            Some((head, rest)) if head.eq_ignore_ascii_case(old) && rest.starts_with('!') => Expr::Name(format!("{new}{rest}")),
            _ => Expr::Name(n),
        },
        other => other,
    })
}

/// References to a deleted sheet become `#REF!`. `order` lists the sheet names (before the deletion) in tab order:
/// a 3-D span that loses an end sheet shrinks to the neighbouring surviving sheet, as in Excel.
pub fn delete_sheet(e: Expr, name: &str, order: &[String]) -> Expr {
    let pos = |n: &str| order.iter().position(|o| o.eq_ignore_ascii_case(n));
    e.map(&mut |x| match x {
        Expr::Ref(r) if r.sheet_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) => Expr::Error(CellError::Ref),
        Expr::Ref(mut r) => {
            if let SheetSel::Span(a, b) = &r.sheet
                && (a.eq_ignore_ascii_case(name) || b.eq_ignore_ascii_case(name))
            {
                let (Some(ia), Some(ib)) = (pos(a), pos(b)) else { return Expr::Ref(r) };
                if ia == ib {
                    return Expr::Error(CellError::Ref);
                }
                // The deleted end moves one sheet towards the other end; the span keeps its direction.
                let inward = |from: usize, to: usize| order.get(if from < to { from + 1 } else { from.saturating_sub(1) });
                let (na, nb) = if a.eq_ignore_ascii_case(name) { (inward(ia, ib), order.get(ib)) } else { (order.get(ia), inward(ib, ia)) };
                let (Some(na), Some(nb)) = (na, nb) else { return Expr::Error(CellError::Ref) };
                r.sheet = if na.eq_ignore_ascii_case(nb) { SheetSel::Named(na.clone()) } else { SheetSel::Span(na.clone(), nb.clone()) };
            }
            Expr::Ref(r)
        }
        other => other,
    })
}
