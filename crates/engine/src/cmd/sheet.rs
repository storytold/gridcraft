//! Worksheets and structure: insert/delete rows, columns and cells; sheet tabs.

use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_formula::adjust::{Axis, Edit};
use gridcraft_model::{Anchor, AnchorMode, Sheet, Visibility};
use serde_json::{Value as Json, json};

use super::{
    CommandSpec, Ctx, EngineError, PROTECTED, Result, Session, bad, bool_param, cell_param_on, cmd, edit, has_clipboard, has_doc, ok, parse_lines,
    str_param, target_range, target_sheet, u32_param,
};

mod tabs;

use tabs::{activate, delete_sheet, hide_sheet, insert_sheet, move_sheet, rename_sheet, step_sheet, tab_color, unhide_sheet};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "home.insertRows",
            "Insert Sheet Rows",
            ["Home", "Cells", "Insert"],
            Some("Ctrl+Shift+="),
            "{rows?: \"3:5\", count?}",
            has_doc,
            |s, p| insert_lines(s, p, Axis::Rows)
        ),
        cmd!("home.insertColumns", "Insert Sheet Columns", ["Home", "Cells", "Insert"], None, "{cols?: \"B:C\"}", has_doc, |s, p| insert_lines(
            s,
            p,
            Axis::Cols
        )),
        cmd!("home.deleteRows", "Delete Sheet Rows", ["Home", "Cells", "Delete"], Some("Cmd+-"), "{rows?}", has_doc, |s, p| delete_lines(
            s,
            p,
            Axis::Rows
        )),
        cmd!("home.deleteColumns", "Delete Sheet Columns", ["Home", "Cells", "Delete"], None, "{cols?}", has_doc, |s, p| delete_lines(
            s,
            p,
            Axis::Cols
        )),
        cmd!("sheet.moveColumns", "Move Columns", [], None, "{cols?: \"B:C\", before: zero-based column index}", has_doc, move_columns),
        cmd!("home.insertCells", "Insert Cells…", ["Home", "Cells", "Insert"], None, "{range?, shift: right|down|row|column}", has_doc, insert_cells),
        cmd!(
            "home.insertCopiedCells",
            "Insert Copied Cells",
            ["Home", "Cells", "Insert"],
            None,
            "{at?: \"B3\", shift?: down|right}: inserts the copied cells at `at` (the active cell) and pastes them there. Whole copied rows or columns insert whole rows or columns; other blocks shift the cells at `at` down (default) or right. A cut of whole rows or columns moves them (Insert Cut Cells)",
            has_clipboard,
            insert_copied_cells
        ),
        cmd!("home.deleteCells", "Delete Cells…", ["Home", "Cells", "Delete"], None, "{range?, shift: left|up|row|column}", has_doc, delete_cells),
        cmd!("home.insertSheet", "Insert Sheet", ["Home", "Cells", "Insert"], Some("Shift+F11"), "{name?, before?: index}", has_doc, insert_sheet),
        cmd!("home.deleteSheet", "Delete Sheet", ["Home", "Cells", "Delete"], None, "{sheet?}", has_doc, delete_sheet),
        cmd!("sheet.rename", "Rename Sheet", ["Home", "Cells", "Format"], None, "{sheet?, name}", has_doc, rename_sheet),
        cmd!("sheet.move", "Move or Copy Sheet…", ["Home", "Cells", "Format"], None, "{sheet?, to: index, copy?: bool}", has_doc, move_sheet),
        cmd!("sheet.tabColor", "Tab Color", ["Home", "Cells", "Format"], None, "{sheet?, color: \"#FF0000\"|\"none\"}", has_doc, tab_color),
        cmd!("sheet.hide", "Hide Sheet", ["Home", "Cells", "Format", "Hide & Unhide"], None, "{sheet?}", has_doc, hide_sheet),
        cmd!("sheet.unhide", "Unhide Sheet…", ["Home", "Cells", "Format", "Hide & Unhide"], None, "{sheet}", has_doc, unhide_sheet),
        cmd!(noundo "sheet.activate", "Activate Sheet", [], None, "{sheet: index | name}", has_doc, activate),
        cmd!(noundo "sheet.next", "Next Sheet", [], Some("Ctrl+PageDown"), "{}", has_doc, |s, _| step_sheet(s, 1)),
        cmd!(noundo "sheet.previous", "Previous Sheet", [], Some("Ctrl+PageUp"), "{}", has_doc, |s, _| step_sheet(s, -1)),
    ]
}

fn lines(s: &Session, p: &Json, axis: Axis) -> Result<(u32, u32)> {
    let key = if axis == Axis::Rows { "rows" } else { "cols" };
    let r = match str_param(p, key).or_else(|| str_param(p, "range")) {
        Some(t) => parse_lines(t).ok_or_else(|| bad(key, format!("not a range: {t}")))?,
        None => s.doc()?.selection.current(),
    };
    let (a, b) = if axis == Axis::Rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) };
    let count = u32_param(p, "count").unwrap_or(b - a + 1).max(1);
    Ok((a, count))
}

/// Bottom-right line of an anchor (the `to` marker of a two-cell anchor) and the offset of that
/// corner inside it, in points. Measured against `sh`'s current geometry.
fn anchor_corner(sh: &Sheet, a: &Anchor) -> (u32, u32, f32, f32) {
    let x2 = sh.col_left(a.cell.col) + a.dx as f64 + a.width.max(0.0) as f64;
    let y2 = sh.row_top(a.cell.row) + a.dy as f64 + a.height.max(0.0) as f64;
    let tc = sh.col_at(x2);
    let tr = sh.row_at(y2);
    (tc, tr, (x2 - sh.col_left(tc)) as f32, (y2 - sh.row_top(tr)) as f32)
}

/// Repositions one floating object's anchor for a structural insert/delete, honouring its
/// [`AnchorMode`]: absolute objects ("Don't move or size with cells") stay put; "move but don't
/// size" follows its cell and keeps its size; "move and size" is re-fitted to the lines it lands
/// on. `corner` is the pre-edit bottom-right line/offset (from [`anchor_corner`]); a line deleted
/// out from under an edge collapses that edge onto `at`. `sh` is the post-edit grid, used to
/// measure the re-fitted size.
fn shift_anchor(sh: &Sheet, a: Anchor, corner: (u32, u32, f32, f32), axis: Axis, at: u32, map_line: &impl Fn(u32) -> Option<u32>) -> Anchor {
    if a.mode == AnchorMode::Absolute {
        return a;
    }
    let main = if axis == Axis::Rows { a.cell.row } else { a.cell.col };
    // A deleted anchor line collapses onto the line that takes its place.
    let nmain = map_line(main).unwrap_or(at);
    let (nr, nc) = if axis == Axis::Rows { (nmain, a.cell.col) } else { (a.cell.row, nmain) };
    if a.mode == AnchorMode::MoveOnly {
        return Anchor { cell: CellRef::new(nr, nc), ..a };
    }
    // Move and size: re-fit to the lines the object lands on. A far edge whose line was deleted
    // collapses onto the near cell, shrinking the object.
    let (tc, tr, tdx, tdy) = corner;
    let far = if axis == Axis::Rows { tr } else { tc };
    let nfar = map_line(far).unwrap_or(nmain);
    let (nbr, nbc) = if axis == Axis::Rows { (nfar, tc) } else { (tr, nfar) };
    let x1 = sh.col_left(nc) + a.dx as f64;
    let y1 = sh.row_top(nr) + a.dy as f64;
    let xn = sh.col_left(nbc) + tdx as f64;
    let yn = sh.row_top(nbr) + tdy as f64;
    Anchor { cell: CellRef::new(nr, nc), dx: a.dx, dy: a.dy, width: (xn - x1).max(0.0) as f32, height: (yn - y1).max(0.0) as f32, mode: a.mode }
}

/// Shifts everything that has a position on the sheet for an insert/delete.
fn shift_sheet_features(sh: &mut Sheet, axis: Axis, at: u32, count: u32, insert: bool) {
    let map_line = |v: u32| -> Option<u32> {
        if insert {
            if v >= at { v.checked_add(count) } else { Some(v) }
        } else if v < at {
            Some(v)
        } else if v >= at + count {
            Some(v - count)
        } else {
            None
        }
    };
    let e = if insert { Edit::Insert { axis, at, count } } else { Edit::Delete { axis, at, count } };
    let map_range = |r: RangeRef| map_range(r, &e);
    let map_cell = |c: CellRef| -> Option<CellRef> {
        match axis {
            Axis::Rows => map_line(c.row).map(|r| CellRef::new(r, c.col)),
            Axis::Cols => map_line(c.col).map(|x| CellRef::new(c.row, x)),
        }
    };
    // Floating objects: snapshot their bottom-right corners against the pre-shift geometry, so a
    // re-fit after the shift measures against the grid the object actually lands on.
    let obj_corners: Vec<(u32, u32, f32, f32)> = sh
        .charts
        .iter()
        .map(|o| &o.anchor)
        .chain(sh.images.iter().map(|o| &o.anchor))
        .chain(sh.shapes.iter().map(|o| &o.anchor))
        .map(|a| anchor_corner(sh, a))
        .collect();
    // Line info.
    let lines = if axis == Axis::Rows { &mut sh.rows } else { &mut sh.cols };
    let old = std::mem::take(lines);
    for (k, v) in old {
        if let Some(n) = map_line(k)
            && n < if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS }
        {
            lines.insert(n, v);
        }
    }
    sh.merges = sh.merges.iter().filter_map(|m| map_range(*m)).filter(|m| !m.is_single()).collect();
    for cf in sh.cond_formats.iter_mut() {
        cf.ranges = cf.ranges.iter().filter_map(|r| map_range(*r)).collect();
    }
    for er in sh.edit_ranges.iter_mut() {
        er.ranges = er.ranges.iter().filter_map(|r| map_range(*r)).collect();
    }
    sh.edit_ranges.retain(|er| !er.ranges.is_empty());
    sh.cond_formats.retain(|cf| !cf.ranges.is_empty());
    for dv in sh.validations.iter_mut() {
        dv.ranges = dv.ranges.iter().filter_map(|r| map_range(*r)).collect();
    }
    sh.validations.retain(|dv| !dv.ranges.is_empty());
    for t in sh.tables.iter_mut() {
        if let Some(r) = map_range(t.range) {
            if axis == Axis::Cols {
                shift_table_columns(t, at, count, insert);
            }
            t.range = r;
        } else {
            t.range = RangeRef::default();
        }
    }
    sh.tables.retain(|t| t.range != RangeRef::default() && !t.columns.is_empty());
    if let Some(af) = sh.autofilter.as_mut() {
        match map_range(af.range) {
            Some(r) => af.range = r,
            None => sh.autofilter = None,
        }
    }
    sh.cell_pictures =
        std::mem::take(&mut sh.cell_pictures).into_iter().filter_map(|(c, v)| map_cell(c).filter(CellRef::is_valid).map(|c| (c, v))).collect();
    sh.comments = std::mem::take(&mut sh.comments).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    sh.hyperlinks = std::mem::take(&mut sh.hyperlinks).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    // Floating objects follow their anchor lines per their [`AnchorMode`]. Corners were snapshotted
    // as charts ++ images ++ shapes, matching the three slices below. New anchors are computed
    // against the post-shift grid first (the re-fit needs its sizes), then written back.
    let anchored = |ui: usize, a: Anchor| -> Anchor { obj_corners.get(ui).map_or(a, |&corner| shift_anchor(sh, a, corner, axis, at, &map_line)) };
    let new_charts: Vec<Anchor> = sh.charts.iter().enumerate().map(|(i, o)| anchored(i, o.anchor)).collect();
    let new_images: Vec<Anchor> = sh.images.iter().enumerate().map(|(i, o)| anchored(new_charts.len() + i, o.anchor)).collect();
    let base = new_charts.len() + new_images.len();
    let new_shapes: Vec<Anchor> = sh.shapes.iter().enumerate().map(|(i, o)| anchored(base + i, o.anchor)).collect();
    for (o, a) in sh.charts.iter_mut().zip(new_charts) {
        o.anchor = a;
    }
    for (o, a) in sh.images.iter_mut().zip(new_images) {
        o.anchor = a;
    }
    for (o, a) in sh.shapes.iter_mut().zip(new_shapes) {
        o.anchor = a;
    }
    sh.sparklines.retain_mut(|sp| match map_cell(sp.cell) {
        Some(c) => {
            sp.cell = c;
            true
        }
        None => false,
    });
    if let Some(pa) = sh.print.print_area {
        sh.print.print_area = map_range(pa);
    }
    // PivotTables follow their report cells. A deleted anchor line collapses onto the line that
    // takes its place; a report deleted entirely has no last area until the next refresh.
    for pt in sh.pivots.iter_mut() {
        pt.anchor = map_cell(pt.anchor).unwrap_or_else(|| match axis {
            Axis::Rows => CellRef::new(at, pt.anchor.col),
            Axis::Cols => CellRef::new(pt.anchor.row, at),
        });
        pt.last_range = pt.last_range.and_then(map_range);
    }
}

/// Rewrites every formula reference (and chart series) for a structural edit on `target`.
pub(super) fn rewrite(cx: &mut Ctx, target: &str, e: &Edit) {
    cx.edits.push((target.to_string(), e.clone()));
    let wb = &mut cx.wb;
    super::edit::rewrite_all_formulas(wb, target, e);
    for si in 0..wb.sheets.len() {
        let host = wb.sheets.get(si).map(|s| s.name.clone()).unwrap_or_default();
        let Some(sh) = wb.sheet_mut(si) else { continue };
        let fix = |text: &str| -> String {
            match gridcraft_formula::parse(text) {
                Ok(expr) => gridcraft_formula::print(&gridcraft_formula::adjust::adjust(expr, &host, target, e)),
                Err(_) => text.to_string(),
            }
        };
        for ch in sh.charts.iter_mut() {
            // The source block moves with its cells; one that was deleted is dropped.
            ch.source = ch.source.as_deref().map(fix).filter(|s| !s.contains("#REF!"));
            for se in ch.series.iter_mut() {
                se.values = fix(&se.values);
                se.categories = se.categories.as_deref().map(fix);
                se.name = se.name.as_deref().map(fix);
            }
        }
        for sp in sh.sparklines.iter_mut() {
            sp.source = fix(&sp.source);
        }
        // PivotTable sources (a range, table or defined name) are only rewritten when the edit
        // moves them, so table and name sources keep their text.
        for pt in sh.pivots.iter_mut() {
            let body = pt.source.trim().trim_start_matches('=');
            if let Ok(expr) = gridcraft_formula::parse(body) {
                let before = gridcraft_formula::print(&expr);
                let after = gridcraft_formula::print(&gridcraft_formula::adjust::adjust(expr, &host, target, e));
                if after != before {
                    pt.source = after;
                }
            }
        }
    }
}

/// Inserting or deleting cells moves the copied block away from the stored source range, so, like
/// Excel, it cancels copy/cut mode for this workbook (a later paste would take the wrong cells).
fn cancel_copy_mode(s: &mut Session) {
    let uid = s.doc().map(|d| d.uid).ok();
    if s.clipboard.as_ref().is_some_and(|c| Some(c.doc_uid) == uid) {
        s.clipboard = None;
    }
}

fn inherit_line_format(sh: &mut Sheet, axis: Axis, at: u32, count: u32) {
    if at == 0 {
        return;
    }
    let prev = if axis == Axis::Rows { sh.rows.get(&(at - 1)).copied() } else { sh.cols.get(&(at - 1)).copied() };
    if let Some(info) = prev {
        for i in at..at + count {
            let map = if axis == Axis::Rows { &mut sh.rows } else { &mut sh.cols };
            map.insert(i, gridcraft_model::LineInfo { hidden: false, outline: 0, collapsed: false, ..info });
        }
    }
}

fn check_push_off_sheet(s: &Session, sheet: usize, axis: Axis, at: u32, count: u32, limit: u32) -> Result<()> {
    if let Some(u) = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.used_range()) {
        let last = if axis == Axis::Rows { u.end.row } else { u.end.col };
        if last >= at && last as u64 + count as u64 >= limit as u64 {
            return Err(EngineError::Other("To prevent possible loss of data, Excel cannot shift nonblank cells off of the worksheet.".into()));
        }
    }
    Ok(())
}

fn insert_lines(s: &mut Session, p: &Json, axis: Axis) -> Result<Json> {
    let (at, count) = lines(s, p, axis)?;
    let limit = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
    if at.checked_add(count).is_none_or(|end| end > limit) {
        let cmd = if axis == Axis::Rows { "home.insertRows" } else { "home.insertColumns" };
        return Err(bad(cmd, "the inserted lines would extend past the end of the worksheet"));
    }
    check_lines_protection(s, p, axis, at, count, true)?;
    let sheet = target_sheet(s, p)?;
    check_push_off_sheet(s, sheet, axis, at, count, limit)?;
    let out = edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        cx.protection_checked = true;
        {
            let sh = cx.sheet_mut(sheet)?;
            match axis {
                Axis::Rows => sh.cells.shift_rows(at, count as i64),
                Axis::Cols => sh.cells.shift_cols(at, count as i64),
            }
            shift_sheet_features(sh, axis, at, count, true);
            // New lines take the format of the line before (Excel's "Format Same As Above").
            inherit_line_format(sh, axis, at, count);
        }
        rewrite(cx, &name, &Edit::Insert { axis, at, count });
        cx.structural = true;
        Ok(Json::Null)
    })?;
    cancel_copy_mode(s);
    Ok(out)
}

/// The areas a line delete/insert targets as `(first line, count)`, merged and sorted from the
/// last line to the first so deleting one never shifts the next. A comma-separated list
/// (`"3:3,7:8"`) or a multi-area selection yields several; `count` forces the single-area form.
fn delete_areas(s: &Session, p: &Json, axis: Axis) -> Result<Vec<(u32, u32)>> {
    let key = if axis == Axis::Rows { "rows" } else { "cols" };
    let spans: Vec<(u32, u32)> = if u32_param(p, "count").is_some() {
        let (at, count) = lines(s, p, axis)?;
        let limit = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
        // Deleting past the last line only removes up to the end of the worksheet.
        let count = count.min(limit.saturating_sub(at)).max(1);
        vec![(at, at.saturating_add(count - 1))]
    } else if let Some(t) = str_param(p, key).or_else(|| str_param(p, "range")) {
        t.split(',')
            .map(|part| {
                let r = parse_lines(part).ok_or_else(|| bad(key, format!("not a range: {part}")))?;
                Ok(if axis == Axis::Rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) })
            })
            .collect::<Result<_>>()?
    } else {
        s.doc()?.selection.ranges.iter().map(|r| if axis == Axis::Rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) }).collect()
    };
    let mut spans: Vec<(u32, u32)> = spans.into_iter().map(|(a, b)| (a.min(b), a.max(b))).collect();
    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    Ok(merged.into_iter().rev().map(|(a, b)| (a, b - a + 1)).collect())
}

fn delete_lines(s: &mut Session, p: &Json, axis: Axis) -> Result<Json> {
    let areas = delete_areas(s, p, axis)?;
    for &(at, count) in &areas {
        check_lines_protection(s, p, axis, at, count, false)?;
    }
    let sheet = target_sheet(s, p)?;
    let out = edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        cx.protection_checked = true;
        // Last area first, so the earlier ones keep their positions.
        for (at, count) in areas {
            {
                let sh = cx.sheet_mut(sheet)?;
                let block = match axis {
                    Axis::Rows => RangeRef::rows(at, (at + count - 1).min(MAX_ROWS - 1)),
                    Axis::Cols => RangeRef::cols(at, (at + count - 1).min(MAX_COLS - 1)),
                };
                sh.take_cells(block);
                match axis {
                    Axis::Rows => sh.cells.shift_rows(at + count, -(count as i64)),
                    Axis::Cols => sh.cells.shift_cols(at + count, -(count as i64)),
                }
                shift_sheet_features(sh, axis, at, count, false);
            }
            rewrite(cx, &name, &Edit::Delete { axis, at, count });
        }
        cx.structural = true;
        Ok(Json::Null)
    })?;
    cancel_copy_mode(s);
    Ok(out)
}

/// Reorders a contiguous block of whole columns without replacing the destination columns.
/// `before` is a boundary in the original sheet: moving B:C before E produces A,D,B,C,E.
fn move_columns(s: &mut Session, p: &Json) -> Result<Json> {
    let (from, count) = lines(s, p, Axis::Cols)?;
    let before = u32_param(p, "before").ok_or_else(|| bad("sheet.moveColumns", "missing `before`"))?;
    let end =
        from.checked_add(count).filter(|end| *end <= MAX_COLS).ok_or_else(|| bad("sheet.moveColumns", "source columns are outside the worksheet"))?;
    if before >= MAX_COLS || before.checked_add(count).is_none_or(|end| end > MAX_COLS) {
        return Err(bad("sheet.moveColumns", "destination columns are outside the worksheet"));
    }
    if (from..=end).contains(&before) {
        return Ok(json!({"cols": RangeRef::cols(from, end.saturating_sub(1)).a1()}));
    }
    let sheet = target_sheet(s, p)?;
    if let Some(u) = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.used_range())
        && u.end.col >= before
        && u.end.col as u64 + count as u64 >= MAX_COLS as u64
    {
        return Err(EngineError::Other("To prevent possible loss of data, columns cannot be shifted off of the worksheet.".into()));
    }
    // The checks must authorize the sheet this command modifies (`target_sheet`), not whatever
    // sheet is active: both carry the command's own `sheet` parameter on.
    check_lines_protection(s, &json!({"cols": RangeRef::cols(before, before).a1(), "sheet": p.get("sheet")}), Axis::Cols, before, count, true)?;
    check_lines_protection(s, &json!({"cols": RangeRef::cols(from, end - 1).a1(), "sheet": p.get("sheet")}), Axis::Cols, from, count, false)?;

    let moved_from = if before <= from { from + count } else { from };
    if moved_from.checked_add(count).is_none_or(|end| end > MAX_COLS) {
        return Err(bad("sheet.moveColumns", "the move is too close to the worksheet boundary"));
    }
    let destination = before;
    let delete_at = moved_from;
    let final_start = if before > from { before - count } else { before };
    let active_row = s.doc()?.selection.active.row;
    let source = RangeRef::cols(moved_from, moved_from + count - 1);
    let out = edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|sh| sh.name.clone()).unwrap_or_default();
        cx.protection_checked = true;

        {
            let sh = cx.sheet_mut(sheet)?;
            sh.cells.shift_cols(before, count as i64);
            shift_sheet_features(sh, Axis::Cols, before, count, true);
        }
        rewrite(cx, &name, &Edit::Insert { axis: Axis::Cols, at: before, count });

        let mut touched = Vec::new();
        {
            let sh = cx.sheet_mut(sheet)?;
            let pictures: Vec<_> =
                sh.cell_pictures.iter().filter(|(cell, _)| source.contains(**cell)).map(|(cell, picture)| (*cell, picture.clone())).collect();
            let cells = sh.take_cells(source);
            let moved_lines: Vec<_> = (0..count).filter_map(|offset| sh.cols.remove(&(moved_from + offset)).map(|info| (offset, info))).collect();
            let movement = Edit::Move { from: source, to_row: 0, to_col: destination };
            shift_cell_features(sh, &movement);
            for object in sh
                .charts
                .iter_mut()
                .map(|o| &mut o.anchor)
                .chain(sh.images.iter_mut().map(|o| &mut o.anchor))
                .chain(sh.shapes.iter_mut().map(|o| &mut o.anchor))
            {
                if object.mode != AnchorMode::Absolute && (moved_from..moved_from + count).contains(&object.cell.col) {
                    object.cell.col = destination + (object.cell.col - moved_from);
                }
            }
            for (cell, value) in cells {
                let dest = CellRef::new(cell.row, destination + (cell.col - moved_from));
                sh.cells.set(dest, value);
                touched.extend([cell, dest]);
            }
            for (cell, picture) in pictures {
                let dest = CellRef::new(cell.row, destination + (cell.col - moved_from));
                sh.cell_pictures.insert(dest, picture);
            }
            for (offset, info) in moved_lines {
                sh.cols.insert(destination + offset, info);
            }
        }
        for cell in touched {
            cx.touch(sheet, cell);
        }
        rewrite(cx, &name, &Edit::Move { from: source, to_row: 0, to_col: destination });

        {
            let sh = cx.sheet_mut(sheet)?;
            sh.cells.shift_cols(delete_at + count, -(count as i64));
            shift_sheet_features(sh, Axis::Cols, delete_at, count, false);
        }
        rewrite(cx, &name, &Edit::Delete { axis: Axis::Cols, at: delete_at, count });
        *cx.sel = crate::Selection {
            active: CellRef::new(active_row, final_start),
            anchor: CellRef::new(active_row, final_start),
            ranges: vec![RangeRef::cols(final_start, final_start + count - 1)],
        };
        cx.structural = true;
        Ok(json!({"cols": RangeRef::cols(final_start, final_start + count - 1).a1()}))
    })?;
    cancel_copy_mode(s);
    Ok(out)
}

fn insert_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let shift = str_param(p, "shift").unwrap_or("down");
    let r = target_range(s, p)?;
    match shift {
        "row" | "entireRow" => return insert_lines(s, &json!({"rows": format!("{}:{}", r.start.row + 1, r.end.row + 1)}), Axis::Rows),
        "column" | "entireColumn" => return insert_lines(s, &json!({"cols": RangeRef::cols(r.start.col, r.end.col).a1()}), Axis::Cols),
        _ => {}
    }
    let e = Edit::InsertCells { axis: if shift == "right" { Axis::Cols } else { Axis::Rows }, range: r };
    let sheet = target_sheet(s, p)?;
    if let Some(sh) = s.doc()?.wb.sheet(sheet) {
        check_table_shift(sh, &e)?;
        // Refuse to push data off the sheet: cells in the last `width`/`height` positions would be dropped.
        let doomed = if shift == "right" {
            RangeRef::new(CellRef::new(r.start.row, r.start.col.max(MAX_COLS.saturating_sub(r.width()))), CellRef::new(r.end.row, MAX_COLS - 1))
        } else {
            RangeRef::new(CellRef::new(r.start.row.max(MAX_ROWS.saturating_sub(r.height())), r.start.col), CellRef::new(MAX_ROWS - 1, r.end.col))
        };
        if sh.cells.iter_range(doomed).any(|(_, x)| x.formula.is_some() || !x.value.is_empty()) {
            return Err(EngineError::Other("To prevent possible loss of data, Excel cannot shift nonblank cells off of the worksheet.".into()));
        }
    }
    let right = shift == "right";
    let out = edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if right {
            sh.cells.shift_cols_in_rows(r.start.row, r.end.row, r.start.col, r.width() as i64);
        } else {
            sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.start.row, r.height() as i64);
        }
        shift_cell_features(sh, &e);
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        rewrite(cx, &name, &e);
        cx.structural = true;
        Ok(Json::Null)
    })?;
    cancel_copy_mode(s);
    Ok(out)
}

fn delete_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let shift = str_param(p, "shift").unwrap_or("up");
    let r = target_range(s, p)?;
    match shift {
        "row" | "entireRow" => return delete_lines(s, &json!({"rows": format!("{}:{}", r.start.row + 1, r.end.row + 1)}), Axis::Rows),
        "column" | "entireColumn" => return delete_lines(s, &json!({"cols": RangeRef::cols(r.start.col, r.end.col).a1()}), Axis::Cols),
        _ => {}
    }
    let e = Edit::DeleteCells { axis: if shift == "left" { Axis::Cols } else { Axis::Rows }, range: r };
    let sheet = target_sheet(s, p)?;
    if let Some(sh) = s.doc()?.wb.sheet(sheet) {
        check_table_shift(sh, &e)?;
    }
    let left = shift == "left";
    let out = edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        {
            let sh = cx.sheet_mut(sheet)?;
            sh.take_cells(r);
            if left {
                sh.cells.shift_cols_in_rows(r.start.row, r.end.row, r.end.col + 1, -(r.width() as i64));
            } else {
                sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.end.row + 1, -(r.height() as i64));
            }
            shift_cell_features(sh, &e);
        }
        // References into the deleted cells become #REF!; the rest move.
        rewrite(cx, &name, &e);
        cx.structural = true;
        Ok(Json::Null)
    })?;
    cancel_copy_mode(s);
    Ok(out)
}

/// Insert Copied Cells / Insert Cut Cells: makes room at the destination, then pastes (or, for a
/// cut, moves) the clipboard there, as one undo step.
fn insert_copied_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(clip) = s.clipboard.clone() else { return Err(EngineError::Other("Nothing to paste.".into())) };
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    let at = cell_param_on(s, p, "at", sheet)?.unwrap_or(d.selection.current().start);
    let shift = str_param(p, "shift").unwrap_or("down");
    if shift != "down" && shift != "right" {
        return Err(bad("home.insertCopiedCells", format!("unknown `shift`: {shift} (down or right)")));
    }
    let sel = clip.selected;
    let rows = sel.is_full_rows();
    let cols = sel.is_full_cols();
    if rows && cols {
        return Err(EngineError::Other("The copy area is the whole sheet; there is no room to insert it.".into()));
    }
    let same_doc = d.uid == clip.doc_uid;
    let cut = clip.cut && same_doc;
    if cut && !rows && !cols {
        return Err(EngineError::Other("Insert Cut Cells works on whole rows or columns; cut whole rows or columns, or use Paste.".into()));
    }
    // A moved block can't be inserted inside itself.
    if cut && clip.sheet == sheet {
        let inside = if rows { at.row > sel.start.row && at.row <= sel.end.row } else { at.col > sel.start.col && at.col <= sel.end.col };
        if inside {
            return Err(EngineError::Other("The cut rows or columns can't be inserted inside themselves.".into()));
        }
    }
    let revision = d.revision;
    let before = (d.wb.clone(), d.selection.clone());
    // Inside an undo group the nested steps add no undo entries of their own.
    let nested_entries = !s.in_group(d.uid);
    let result = insert_copied_steps(s, &clip, sheet, at, shift, cut);
    let d = s.doc_mut()?;
    // The nested steps collapse into this command's single undo step.
    if nested_entries {
        for _ in 0..d.revision.saturating_sub(revision) {
            d.undo.pop();
        }
    }
    match result {
        Ok((final_range, out)) => {
            d.selection = crate::Selection { active: final_range.start, anchor: final_range.start, ranges: vec![final_range] };
            // Like inserting cells, this ends copy mode (the copied block may have moved).
            cancel_copy_mode(s);
            Ok(out)
        }
        Err(e) => {
            d.wb = before.0;
            d.selection = before.1;
            d.calc.rebuild(&d.wb);
            s.clipboard = Some(clip);
            Err(e)
        }
    }
}

fn insert_copied_steps(s: &mut Session, clip: &crate::Clipboard, sheet: usize, at: CellRef, shift: &str, cut: bool) -> Result<(RangeRef, Json)> {
    let sel = clip.selected;
    let rows = sel.is_full_rows();
    let cols = sel.is_full_cols();
    // Where the copied block's top-left lands, relative to the (untrimmed) copied lines.
    let (inserted, paste_at) = if rows {
        let h = sel.height();
        let start = at.row;
        let r = format!("{}:{}", start as u64 + 1, start as u64 + h as u64);
        s.execute("home.insertRows", json!({"rows": r, "sheet": sheet}))?;
        let dr = clip.range.start.row.saturating_sub(sel.start.row);
        (RangeRef::rows(start, start.saturating_add(h - 1).min(MAX_ROWS - 1)), CellRef::new(start.saturating_add(dr), clip.range.start.col))
    } else if cols {
        let w = sel.width();
        let start = at.col;
        let end = start.saturating_add(w - 1).min(MAX_COLS - 1);
        s.execute("home.insertColumns", json!({"cols": RangeRef::cols(start, end).a1(), "sheet": sheet}))?;
        let dc = clip.range.start.col.saturating_sub(sel.start.col);
        (RangeRef::cols(start, end), CellRef::new(clip.range.start.row, start.saturating_add(dc)))
    } else {
        let end = at
            .offset(clip.range.height() as i64 - 1, clip.range.width() as i64 - 1)
            .ok_or_else(|| EngineError::Other("The copied cells don't fit on the sheet at this position.".into()))?;
        let block = RangeRef::new(at, end);
        s.execute("home.insertCells", json!({"range": block.a1(), "shift": shift, "sheet": sheet}))?;
        (block, at)
    };
    // Inserting ended copy mode; bring the clipboard back for the paste. A cut reads the live
    // workbook, so it follows the source if the insert pushed it along.
    let mut clip = clip.clone();
    if cut {
        let pushed = clip.sheet == sheet && if rows { sel.start.row >= at.row } else { sel.start.col >= at.col };
        if pushed {
            let (dr, dc) = if rows { (inserted.height() as i64, 0) } else { (0, inserted.width() as i64) };
            clip.range = RangeRef::new(clip.range.start.offset_clamped(dr, dc), clip.range.end.offset_clamped(dr, dc));
            clip.selected = RangeRef::new(sel.start.offset_clamped(dr, dc), sel.end.offset_clamped(dr, dc));
        }
        clip.wb = s.doc()?.wb.clone();
    }
    let source_lines = clip.selected;
    s.clipboard = Some(clip.clone());
    let out = s.execute("edit.pasteSpecial", json!({"what": "all", "at": paste_at.a1(), "sheet": sheet}))?;
    if !cut {
        return Ok((inserted, out));
    }
    // Close the gap the moved lines left behind.
    let (removed, before_dest) = if rows {
        s.execute(
            "home.deleteRows",
            json!({"rows": format!("{}:{}", source_lines.start.row as u64 + 1, source_lines.end.row as u64 + 1), "sheet": clip.sheet}),
        )?;
        (source_lines.height(), clip.sheet == sheet && source_lines.start.row < inserted.start.row)
    } else {
        s.execute("home.deleteColumns", json!({"cols": RangeRef::cols(source_lines.start.col, source_lines.end.col).a1(), "sheet": clip.sheet}))?;
        (source_lines.width(), clip.sheet == sheet && source_lines.start.col < inserted.start.col)
    };
    let moved = if before_dest {
        let (dr, dc) = if rows { (-(removed as i64), 0) } else { (0, -(removed as i64)) };
        RangeRef::new(inserted.start.offset_clamped(dr, dc), inserted.end.offset_clamped(dr, dc))
    } else {
        inserted
    };
    Ok((moved, out))
}

/// Sheet protection for inserting or deleting rows/columns: the protection must allow it, and
/// deleted lines must not hold locked cells.
fn check_lines_protection(s: &Session, p: &Json, axis: Axis, at: u32, count: u32, insert: bool) -> Result<()> {
    let sheet = target_sheet(s, p)?;
    let wb = &s.doc()?.wb;
    let Some(sh) = wb.sheet(sheet) else { return Ok(()) };
    let Some(pr) = &sh.protection else { return Ok(()) };
    let allowed = match (axis, insert) {
        (Axis::Rows, true) => pr.insert_rows,
        (Axis::Rows, false) => pr.delete_rows,
        (Axis::Cols, true) => pr.insert_columns,
        (Axis::Cols, false) => pr.delete_columns,
    };
    let locked = |st: gridcraft_model::StyleId| wb.styles.get(st).protection.locked;
    let refuse = || EngineError::Other(PROTECTED.into());
    if !allowed {
        return Err(refuse());
    }
    if insert {
        return Ok(());
    }
    let end = at.saturating_add(count - 1).min(if axis == Axis::Rows { MAX_ROWS - 1 } else { MAX_COLS - 1 });
    let infos = if axis == Axis::Rows { &sh.rows } else { &sh.cols };
    if (at..=end).any(|i| locked(infos.get(&i).and_then(|x| x.style).unwrap_or_default())) {
        return Err(refuse());
    }
    let block = if axis == Axis::Rows { RangeRef::rows(at, end) } else { RangeRef::cols(at, end) };
    if sh.cells.iter_range(block).any(|(_, cell)| locked(cell.style)) {
        return Err(refuse());
    }
    Ok(())
}

/// Where a range of the edited sheet goes after a structural edit (`None` = deleted).
pub(crate) fn map_range(r: RangeRef, e: &Edit) -> Option<RangeRef> {
    let expr = gridcraft_formula::Expr::Ref(gridcraft_formula::Reference {
        sheet: gridcraft_formula::SheetSel::Current,
        kind: gridcraft_formula::RefKind::Range(
            gridcraft_formula::Anchor { row: r.start.row, col: r.start.col, row_abs: false, col_abs: false },
            gridcraft_formula::Anchor { row: r.end.row, col: r.end.col, row_abs: false, col_abs: false },
        ),
    });
    match gridcraft_formula::adjust::adjust(expr, "S", "S", e) {
        gridcraft_formula::Expr::Ref(r) => Some(r.range()),
        _ => None,
    }
}

/// Shifts what Insert/Delete Cells moves along with the cells: merged areas, conditional
/// format, validation, table, filter and print-area ranges, notes, links and sparklines.
pub(super) fn shift_cell_features(sh: &mut Sheet, e: &Edit) {
    let map_cell = |c: CellRef| map_range(RangeRef::cell(c), e).map(|r| r.start);
    sh.merges = sh.merges.iter().filter_map(|m| map_range(*m, e)).filter(|m| !m.is_single()).collect();
    for cf in sh.cond_formats.iter_mut() {
        cf.ranges = cf.ranges.iter().filter_map(|r| map_range(*r, e)).collect();
    }
    for er in sh.edit_ranges.iter_mut() {
        er.ranges = er.ranges.iter().filter_map(|r| map_range(*r, e)).collect();
    }
    sh.edit_ranges.retain(|er| !er.ranges.is_empty());
    sh.cond_formats.retain(|cf| !cf.ranges.is_empty());
    for dv in sh.validations.iter_mut() {
        dv.ranges = dv.ranges.iter().filter_map(|r| map_range(*r, e)).collect();
    }
    sh.validations.retain(|dv| !dv.ranges.is_empty());
    for t in sh.tables.iter_mut() {
        // Cells inserted or deleted across a table's full height add or remove table columns.
        if let Edit::InsertCells { axis: Axis::Cols, range } | Edit::DeleteCells { axis: Axis::Cols, range } = *e
            && range.start.row <= t.range.start.row
            && range.end.row >= t.range.end.row
        {
            shift_table_columns(t, range.start.col, range.width(), matches!(e, Edit::InsertCells { .. }));
        }
        t.range = map_range(t.range, e).unwrap_or_default();
    }
    sh.tables.retain(|t| t.range != RangeRef::default() && !t.columns.is_empty());
    if let Some(af) = sh.autofilter.as_mut() {
        match map_range(af.range, e) {
            Some(r) => af.range = r,
            None => sh.autofilter = None,
        }
    }
    sh.cell_pictures =
        std::mem::take(&mut sh.cell_pictures).into_iter().filter_map(|(c, v)| map_cell(c).filter(CellRef::is_valid).map(|c| (c, v))).collect();
    sh.comments = std::mem::take(&mut sh.comments).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    sh.hyperlinks = std::mem::take(&mut sh.hyperlinks).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    for sp in sh.sparklines.iter_mut() {
        if let Some(c) = map_cell(sp.cell) {
            sp.cell = c;
        }
    }
    if let Some(pa) = sh.print.print_area {
        sh.print.print_area = map_range(pa, e);
    }
}

/// Columns inserted inside a table become new table columns (with names no other column has);
/// deleted ones leave it.
fn shift_table_columns(t: &mut gridcraft_model::Table, at: u32, count: u32, insert: bool) {
    if insert {
        if at > t.range.start.col && at <= t.range.end.col {
            let idx = (at - t.range.start.col) as usize;
            let mut n = t.columns.len();
            for k in 0..count as usize {
                let name = loop {
                    n += 1;
                    let name = format!("Column{n}");
                    if !t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
                        break name;
                    }
                };
                let col = gridcraft_model::TableColumn {
                    name,
                    totals: Default::default(),
                    totals_label: None,
                    formula: None,
                    unique_name: None,
                    xml: None,
                };
                t.columns.insert((idx + k).min(t.columns.len()), col);
            }
        }
        return;
    }
    let lo = at.max(t.range.start.col);
    let hi = at.saturating_add(count).min(t.range.end.col + 1);
    if hi > lo {
        let a = (lo - t.range.start.col) as usize;
        let b = (hi - t.range.start.col) as usize;
        if b <= t.columns.len() {
            t.columns.drain(a..b);
        }
    }
}

/// Like Excel, Insert/Delete Cells never shifts part of a table: cells shifted up or down must
/// hold whole table columns, cells shifted left or right whole table rows, and deleting cells
/// never takes a table's header row without the whole table.
pub(crate) fn check_table_shift(sh: &Sheet, e: &Edit) -> Result<()> {
    let (axis, r, delete) = match *e {
        Edit::InsertCells { axis, range } => (axis, range, false),
        Edit::DeleteCells { axis, range } => (axis, range, true),
        _ => return Ok(()),
    };
    for t in &sh.tables {
        let tr = t.range;
        let ok = match axis {
            Axis::Rows => {
                let moves = tr.end.row >= r.start.row && tr.start.col <= r.end.col && tr.end.col >= r.start.col;
                let header_alone = delete && t.header_row && r.start.row <= tr.start.row && r.end.row >= tr.start.row && r.end.row < tr.end.row;
                !moves || (r.start.col <= tr.start.col && r.end.col >= tr.end.col && !header_alone)
            }
            Axis::Cols => {
                let moves = tr.end.col >= r.start.col && tr.start.row <= r.end.row && tr.end.row >= r.start.row;
                !moves || (r.start.row <= tr.start.row && r.end.row >= tr.end.row)
            }
        };
        if !ok {
            return Err(EngineError::Other("This operation isn't allowed: it would shift cells in a table on your worksheet.".into()));
        }
    }
    Ok(())
}
