//! Worksheets and structure: insert/delete rows, columns and cells; sheet tabs.

use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_formula::adjust::{Axis, Edit};
use gridcraft_model::{Sheet, Visibility};
use serde_json::{Value as Json, json};

use super::*;

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
        cmd!("home.insertCells", "Insert Cells…", ["Home", "Cells", "Insert"], None, "{range?, shift: right|down|row|column}", has_doc, insert_cells),
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
        Some(t) => RangeRef::parse(t).ok_or_else(|| bad(key, format!("not a range: {t}")))?,
        None => s.doc()?.selection.current(),
    };
    let (a, b) = if axis == Axis::Rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) };
    let count = u32_param(p, "count").unwrap_or(b - a + 1).max(1);
    Ok((a, count))
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
    let map_range = |r: RangeRef| -> Option<RangeRef> {
        let e = if insert { Edit::Insert { axis, at, count } } else { Edit::Delete { axis, at, count } };
        let expr = gridcraft_formula::Expr::Ref(gridcraft_formula::Reference {
            sheet: gridcraft_formula::SheetSel::Current,
            kind: gridcraft_formula::RefKind::Range(
                gridcraft_formula::Anchor { row: r.start.row, col: r.start.col, row_abs: false, col_abs: false },
                gridcraft_formula::Anchor { row: r.end.row, col: r.end.col, row_abs: false, col_abs: false },
            ),
        });
        match gridcraft_formula::adjust::adjust(expr, "S", "S", &e) {
            gridcraft_formula::Expr::Ref(r) => Some(r.range()),
            _ => None,
        }
    };
    let map_cell = |c: CellRef| -> Option<CellRef> {
        match axis {
            Axis::Rows => map_line(c.row).map(|r| CellRef::new(r, c.col)),
            Axis::Cols => map_line(c.col).map(|x| CellRef::new(c.row, x)),
        }
    };
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
    sh.cond_formats.retain(|cf| !cf.ranges.is_empty());
    for dv in sh.validations.iter_mut() {
        dv.ranges = dv.ranges.iter().filter_map(|r| map_range(*r)).collect();
    }
    sh.validations.retain(|dv| !dv.ranges.is_empty());
    for t in sh.tables.iter_mut() {
        if let Some(r) = map_range(t.range) {
            // Columns inserted inside a table become new table columns.
            if axis == Axis::Cols && insert && at > t.range.start.col && at <= t.range.end.col {
                let idx = (at - t.range.start.col) as usize;
                for k in 0..count {
                    let name = format!("Column{}", t.columns.len() + 1);
                    t.columns.insert(
                        (idx + k as usize).min(t.columns.len()),
                        gridcraft_model::TableColumn { name, totals: Default::default(), totals_label: None, formula: None },
                    );
                }
            }
            if axis == Axis::Cols && !insert {
                let lo = at.max(t.range.start.col);
                let hi = (at + count).min(t.range.end.col + 1);
                if hi > lo {
                    let a = (lo - t.range.start.col) as usize;
                    let b = (hi - t.range.start.col) as usize;
                    if b <= t.columns.len() {
                        t.columns.drain(a..b);
                    }
                }
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
    sh.comments = std::mem::take(&mut sh.comments).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    sh.hyperlinks = std::mem::take(&mut sh.hyperlinks).into_iter().filter_map(|(c, v)| map_cell(c).map(|c| (c, v))).collect();
    for ch in sh.charts.iter_mut() {
        if let Some(c) = map_cell(ch.anchor.cell) {
            ch.anchor.cell = c;
        }
    }
    for im in sh.images.iter_mut() {
        if let Some(c) = map_cell(im.anchor.cell) {
            im.anchor.cell = c;
        }
    }
    for sp in sh.sparklines.iter_mut() {
        if let Some(c) = map_cell(sp.cell) {
            sp.cell = c;
        }
    }
    if let Some(pa) = sh.print.print_area {
        sh.print.print_area = map_range(pa);
    }
}

/// Rewrites every formula reference (and chart series) for a structural edit on `target`.
fn rewrite(wb: &mut gridcraft_model::Workbook, target: &str, e: &Edit) {
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
            for se in ch.series.iter_mut() {
                se.values = fix(&se.values);
                se.categories = se.categories.as_deref().map(fix);
                se.name = se.name.as_deref().map(fix);
            }
        }
        for sp in sh.sparklines.iter_mut() {
            sp.source = fix(&sp.source);
        }
    }
}

fn insert_lines(s: &mut Session, p: &Json, axis: Axis) -> Result<Json> {
    let (at, count) = lines(s, p, axis)?;
    let sheet = target_sheet(s, p)?;
    // Refuse to push data off the sheet.
    let limit = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
    if let Some(u) = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.used_range()) {
        let last = if axis == Axis::Rows { u.end.row } else { u.end.col };
        if last >= at && last as u64 + count as u64 >= limit as u64 {
            return Err(EngineError::Other("To prevent possible loss of data, Excel cannot shift nonblank cells off of the worksheet.".into()));
        }
    }
    edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        {
            let sh = cx.sheet_mut(sheet)?;
            match axis {
                Axis::Rows => sh.cells.shift_rows(at, count as i64),
                Axis::Cols => sh.cells.shift_cols(at, count as i64),
            }
            shift_sheet_features(sh, axis, at, count, true);
            // New lines take the format of the line before (Excel's "Format Same As Above").
            if at > 0 {
                let prev = if axis == Axis::Rows { sh.rows.get(&(at - 1)).copied() } else { sh.cols.get(&(at - 1)).copied() };
                if let Some(info) = prev {
                    for i in at..at + count {
                        let map = if axis == Axis::Rows { &mut sh.rows } else { &mut sh.cols };
                        map.insert(i, gridcraft_model::LineInfo { hidden: false, outline: 0, collapsed: false, ..info });
                    }
                }
            }
        }
        rewrite(&mut cx.wb, &name, &Edit::Insert { axis, at, count });
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn delete_lines(s: &mut Session, p: &Json, axis: Axis) -> Result<Json> {
    let (at, count) = lines(s, p, axis)?;
    let sheet = target_sheet(s, p)?;
    edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        {
            let sh = cx.sheet_mut(sheet)?;
            let block = match axis {
                Axis::Rows => RangeRef::rows(at, (at + count - 1).min(MAX_ROWS - 1)),
                Axis::Cols => RangeRef::cols(at, (at + count - 1).min(MAX_COLS - 1)),
            };
            sh.cells.take_range(block);
            match axis {
                Axis::Rows => sh.cells.shift_rows(at + count, -(count as i64)),
                Axis::Cols => sh.cells.shift_cols(at + count, -(count as i64)),
            }
            shift_sheet_features(sh, axis, at, count, false);
        }
        rewrite(&mut cx.wb, &name, &Edit::Delete { axis, at, count });
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn insert_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let shift = str_param(p, "shift").unwrap_or("down");
    let r = target_range(s, p)?;
    match shift {
        "row" | "entireRow" => return insert_lines(s, &json!({"rows": format!("{}:{}", r.start.row + 1, r.end.row + 1)}), Axis::Rows),
        "column" | "entireColumn" => return insert_lines(s, &json!({"cols": RangeRef::cols(r.start.col, r.end.col).a1()}), Axis::Cols),
        _ => {}
    }
    let sheet = target_sheet(s, p)?;
    let right = shift == "right";
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if right {
            sh.cells.shift_cols_in_rows(r.start.row, r.end.row, r.start.col, r.width() as i64);
        } else {
            sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.start.row, r.height() as i64);
        }
        // Formulas referring to cells that moved: approximate with a whole-sheet move edit.
        let moved = if right {
            RangeRef::new(r.start, CellRef::new(r.end.row, MAX_COLS - 1 - r.width()))
        } else {
            RangeRef::new(r.start, CellRef::new(MAX_ROWS - 1 - r.height(), r.end.col))
        };
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        let to = if right { (r.start.row, r.start.col + r.width()) } else { (r.start.row + r.height(), r.start.col) };
        super::edit::rewrite_all_formulas(&mut cx.wb, &name, &Edit::Move { from: moved, to_row: to.0, to_col: to.1 });
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn delete_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let shift = str_param(p, "shift").unwrap_or("up");
    let r = target_range(s, p)?;
    match shift {
        "row" | "entireRow" => return delete_lines(s, &json!({"rows": format!("{}:{}", r.start.row + 1, r.end.row + 1)}), Axis::Rows),
        "column" | "entireColumn" => return delete_lines(s, &json!({"cols": RangeRef::cols(r.start.col, r.end.col).a1()}), Axis::Cols),
        _ => {}
    }
    let sheet = target_sheet(s, p)?;
    let left = shift == "left";
    edit(s, |cx| {
        let name = cx.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default();
        {
            let sh = cx.sheet_mut(sheet)?;
            sh.cells.take_range(r);
            if left {
                sh.cells.shift_cols_in_rows(r.start.row, r.end.row, r.end.col + 1, -(r.width() as i64));
            } else {
                sh.cells.shift_rows_in_cols(r.start.col, r.end.col, r.end.row + 1, -(r.height() as i64));
            }
        }
        // References into the deleted block become #REF!; the rest move.
        let deleted = Edit::Move { from: r, to_row: r.start.row, to_col: r.start.col };
        let _ = deleted;
        let moved_from = if left {
            RangeRef::new(CellRef::new(r.start.row, r.end.col + 1), CellRef::new(r.end.row, MAX_COLS - 1))
        } else {
            RangeRef::new(CellRef::new(r.end.row + 1, r.start.col), CellRef::new(MAX_ROWS - 1, r.end.col))
        };
        super::edit::rewrite_all_formulas(&mut cx.wb, &name, &Edit::Move { from: moved_from, to_row: r.start.row, to_col: r.start.col });
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn insert_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => s.doc()?.wb.next_sheet_name_from(s.lang.sheet_base()),
    };
    s.doc()?.wb.check_sheet_name(&name, None).map_err(EngineError::Other)?;
    let d = s.doc_mut()?;
    let before = p.get("before").and_then(Json::as_u64).map(|v| v as usize);
    let idx = edit_doc(d, |cx| {
        // Excel inserts before the active sheet; the "+" button appends after it.
        let at = before.unwrap_or(cx.wb.active_sheet + 1).min(cx.wb.sheets.len());
        cx.wb.sheets.insert(at, Arc::new(Sheet::new(name.clone())));
        // Names scoped to later sheets shift.
        for n in cx.wb.names.iter_mut() {
            if let Some(sc) = n.scope.as_mut()
                && *sc >= at
            {
                *sc += 1;
            }
        }
        cx.wb.active_sheet = at;
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(at)
    })?;
    d.sheet_selections.insert(idx.min(d.sheet_selections.len()), crate::Selection::default());
    Ok(json!({"index": idx, "name": name}))
}

fn edit_doc<R>(d: &mut crate::DocState, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
    super::commit(d, f)
}

fn delete_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let d = s.doc()?;
    if d.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    if d.wb.sheets.iter().filter(|s| s.visibility == Visibility::Visible).count() <= 1 {
        return Err(EngineError::Other("A workbook must contain at least one visible worksheet.".into()));
    }
    let name = d.wb.sheet(i).map(|s| s.name.clone()).unwrap_or_default();
    let d = s.doc_mut()?;
    edit_doc(d, |cx| {
        cx.wb.sheets.remove(i);
        for si in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, Arc<gridcraft_model::Formula>)> =
                cx.wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            let Some(sh) = cx.wb.sheet_mut(si) else { continue };
            for (c, f) in keys {
                if let Some(e) = f.expr() {
                    let ne = gridcraft_formula::adjust::delete_sheet(e, &name);
                    if let Some(cell) = sh.cells.get_mut(c) {
                        cell.formula = Some(Arc::new(gridcraft_model::Formula::from_expr(ne)));
                    }
                }
            }
        }
        cx.wb.names.retain(|n| n.scope != Some(i));
        for n in cx.wb.names.iter_mut() {
            if let Some(sc) = n.scope.as_mut()
                && *sc > i
            {
                *sc -= 1;
            }
        }
        if cx.wb.active_sheet >= cx.wb.sheets.len() || cx.wb.active_sheet > i {
            cx.wb.active_sheet = cx.wb.active_sheet.saturating_sub(1).min(cx.wb.sheets.len().saturating_sub(1));
        }
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(())
    })?;
    if i < d.sheet_selections.len() {
        d.sheet_selections.remove(i);
    }
    ok()
}

fn rename_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let Some(name) = str_param(p, "name").map(|n| n.trim().to_string()) else { return Err(bad("sheet.rename", "missing `name`")) };
    s.doc()?.wb.check_sheet_name(&name, Some(i)).map_err(EngineError::Other)?;
    edit(s, |cx| {
        let old = cx.wb.sheet(i).map(|s| s.name.clone()).unwrap_or_default();
        cx.sheet_mut(i)?.name = name.clone();
        for si in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, Arc<gridcraft_model::Formula>)> =
                cx.wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            let Some(sh) = cx.wb.sheet_mut(si) else { continue };
            for (c, f) in keys {
                if let Some(e) = f.expr() {
                    let ne = gridcraft_formula::adjust::rename_sheet(e, &old, &name);
                    let text = gridcraft_formula::print(&ne);
                    if text != f.text
                        && let Some(cell) = sh.cells.get_mut(c)
                    {
                        cell.formula = Some(Arc::new(gridcraft_model::Formula::from_expr(ne)));
                    }
                }
            }
            let fix = |t: &str| {
                gridcraft_formula::parse(t)
                    .map(|e| gridcraft_formula::print(&gridcraft_formula::adjust::rename_sheet(e, &old, &name)))
                    .unwrap_or_else(|_| t.to_string())
            };
            for ch in sh.charts.iter_mut() {
                for se in ch.series.iter_mut() {
                    se.values = fix(&se.values);
                    se.categories = se.categories.as_deref().map(fix);
                    se.name = se.name.as_deref().map(fix);
                }
            }
        }
        for n in cx.wb.names.iter_mut() {
            if let Ok(e) = gridcraft_formula::parse(&n.formula) {
                n.formula = gridcraft_formula::print(&gridcraft_formula::adjust::rename_sheet(e, &old, &name));
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn move_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let copy = bool_param(p, "copy").unwrap_or(false);
    let to = p.get("to").and_then(Json::as_u64).map(|v| v as usize).ok_or_else(|| bad("sheet.move", "missing `to`"))?;
    edit(s, |cx| {
        let n = cx.wb.sheets.len();
        let Some(src) = cx.wb.sheets.get(i).cloned() else { return Err(bad("sheet.move", "no such sheet")) };
        if copy {
            let mut sh = (*src).clone();
            let base = sh.name.clone();
            let name = (2..).map(|k| format!("{base} ({k})")).find(|nm| cx.wb.sheet_index(nm).is_none()).unwrap_or(base);
            sh.name = name.chars().take(31).collect();
            sh.tables.clear(); // table names must be unique
            let at = to.min(n);
            cx.wb.sheets.insert(at, Arc::new(sh));
            cx.wb.active_sheet = at;
        } else {
            let sh = cx.wb.sheets.remove(i);
            let at = to.min(n - 1);
            cx.wb.sheets.insert(at, sh);
            cx.wb.active_sheet = at;
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn tab_color(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let c = super::format::color_param(p.get("color"));
    edit(s, |cx| {
        cx.sheet_mut(i)?.tab_color = c.filter(|c| *c != gridcraft_model::Color::Auto);
        Ok(Json::Null)
    })
}

fn hide_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    if s.doc()?.wb.sheets.iter().filter(|s| s.visibility == Visibility::Visible).count() <= 1 {
        return Err(EngineError::Other("A workbook must contain at least one visible worksheet.".into()));
    }
    edit(s, |cx| {
        cx.sheet_mut(i)?.visibility = Visibility::Hidden;
        if cx.wb.active_sheet == i {
            cx.wb.active_sheet = cx.wb.sheets.iter().position(|s| s.visibility == Visibility::Visible).unwrap_or(0);
        }
        Ok(Json::Null)
    })
}

fn unhide_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("sheet").is_none() {
        let hidden: Vec<String> = s.doc()?.wb.sheets.iter().filter(|s| s.visibility != Visibility::Visible).map(|s| s.name.clone()).collect();
        if hidden.is_empty() {
            return Err(EngineError::Other("No sheets are hidden.".into()));
        }
        s.ui_requests.push(crate::UiRequest::Dialog("unhideSheet".into(), json!({"sheets": hidden})));
        return ok();
    }
    let i = target_sheet(s, p)?;
    edit(s, |cx| {
        cx.sheet_mut(i)?.visibility = Visibility::Visible;
        cx.wb.active_sheet = i;
        Ok(Json::Null)
    })
}

fn activate(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let d = s.doc_mut()?;
    d.wb_switch_sheet(i);
    Ok(json!({"sheet": i, "name": d.wb.active().map(|s| s.name.clone())}))
}

fn step_sheet(s: &mut Session, dir: i64) -> Result<Json> {
    let d = s.doc()?;
    let n = d.wb.sheets.len() as i64;
    let mut i = d.wb.active_sheet as i64;
    for _ in 0..n {
        i = (i + dir).clamp(0, n - 1);
        if d.wb.sheet(i as usize).is_some_and(|s| s.visibility == Visibility::Visible) {
            break;
        }
    }
    s.doc_mut()?.wb_switch_sheet(i as usize);
    ok()
}
