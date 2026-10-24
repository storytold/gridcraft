//! Data tab: sort, filter, remove duplicates, text to columns, data validation, outline.

use std::cmp::Ordering;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value, sort_compare};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;
use crate::outline;
use crate::selection::current_region;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("data.sortAscending", "Sort A to Z", ["Data", "Sort & Filter"], None, "{range?, column?: \"B\"}", has_doc, |s, p| quick_sort(
            s, p, true
        )),
        cmd!("data.sortDescending", "Sort Z to A", ["Data", "Sort & Filter"], None, "{range?, column?}", has_doc, |s, p| quick_sort(s, p, false)),
        cmd!(
            "data.sort",
            "Sort…",
            ["Data", "Sort & Filter"],
            None,
            "{range?, header?: bool, keys: [{column: \"B\", order: asc|desc, by?: values|cellColor|fontColor, customList?: [..]}], orientation?: rows|columns, matchCase?}",
            has_doc,
            sort
        ),
        cmd!(
            "data.filter",
            "Filter",
            ["Data", "Sort & Filter"],
            Some("Ctrl+Shift+L"),
            "{range?} toggles AutoFilter on the current region",
            has_doc,
            toggle_filter
        ),
        cmd!(
            "data.filterBy",
            "Filter Column",
            [],
            None,
            "{column: \"B\" (or 0-based offset), values?: [..], blanks?: bool, custom?: {op: \">\", value: \"10\", op2?, value2?, and?}, top?: {count, bottom?, percent?}, aboveAverage?: bool, clear?: bool}",
            has_doc,
            filter_by
        ),
        cmd!("data.clearFilter", "Clear", ["Data", "Sort & Filter"], None, "{}", has_doc, clear_filter),
        cmd!("data.reapply", "Reapply", ["Data", "Sort & Filter"], None, "{}", has_doc, reapply),
        cmd!(
            "data.removeDuplicates",
            "Remove Duplicates",
            ["Data", "Data Tools"],
            None,
            "{range?, columns?: [\"A\",\"C\"], header?: bool}",
            has_doc,
            remove_duplicates
        ),
        cmd!(
            "data.textToColumns",
            "Text to Columns",
            ["Data", "Data Tools"],
            None,
            "{range?, delimiters?: [\",\", \"\\t\", \" \", \";\"], other?: \"|\", fixedWidths?: [5, 10], treatConsecutive?: bool, textQualifier?: \"\\\"\", destination?: \"B1\"}",
            has_doc,
            text_to_columns
        ),
        cmd!(
            "data.validation",
            "Data Validation…",
            ["Data", "Data Tools"],
            None,
            "{range?, type: any|whole|decimal|list|date|time|textLength|custom, operator?: between|notBetween|equal|notEqual|greater|less|greaterOrEqual|lessOrEqual, formula1, formula2?, ignoreBlank?, dropdown?, inputTitle?, inputMessage?, errorStyle?: stop|warning|information, errorTitle?, errorMessage?, clear?: bool}",
            has_doc,
            validation
        ),
        cmd!(query "data.validate", "Check Value Against Validation", [], None, "{cell?, input} → {ok, message?}", has_doc, validate_cmd),
        cmd!(noundo "data.circleInvalid", "Circle Invalid Data", ["Data", "Data Tools", "Data Validation"], None, "{} → invalid cells", has_doc, circle_invalid),
        cmd!(
            "data.group",
            "Group",
            ["Data", "Outline"],
            Some("Alt+Shift+Right"),
            "{rows?: \"2:5\" | cols?: \"B:C\" | by?: rows|cols (the selection's rows or columns; a block of cells groups rows by default), sheet?}",
            has_doc,
            |s, p| group(s, p, true)
        ),
        cmd!("data.ungroup", "Ungroup", ["Data", "Outline"], Some("Alt+Shift+Left"), "{rows? | cols? | by?: rows|cols, sheet?}", has_doc, |s, p| {
            group(s, p, false)
        }),
        cmd!(
            "data.hideDetail",
            "Hide Detail",
            ["Data", "Outline"],
            None,
            "{rows? | cols? | by?, level?: 1-7} (the innermost group, or the one at `level`, holding the lines or summarised by them)",
            has_doc,
            |s, p| detail(s, p, true)
        ),
        cmd!("data.showDetail", "Show Detail", ["Data", "Outline"], None, "{rows? | cols? | by?, level?: 1-7}", has_doc, |s, p| detail(s, p, false)),
        cmd!(
            "data.showLevels",
            "Show Outline Levels",
            [],
            None,
            "{rows?: 1-8, cols?: 1-8} (1 shows only the top-level summaries, as the outline's level buttons)",
            has_doc,
            show_levels
        ),
        cmd!(
            "data.subtotal",
            "Subtotal",
            ["Data", "Outline"],
            None,
            "{range?, groupBy: \"A\", function?: sum|count|average|max|min|product, columns: [\"C\"], header?: true}",
            has_doc,
            subtotal
        ),
        cmd!("data.flashFill", "Flash Fill", ["Data", "Data Tools"], None, "{range?}", has_doc, |s, p| s.execute("edit.flashFill", p.clone())),
    ]
}

fn col_param(v: Option<&Json>, base: u32) -> Option<u32> {
    match v? {
        Json::String(s) => gridcraft_core::letters_to_col(s.trim_start_matches('$')),
        Json::Number(n) => n.as_u64().and_then(|o| u32::try_from(o).ok()).and_then(|o| base.checked_add(o)),
        _ => None,
    }
}

/// The range a data command works on: the selection when it's a block, else the current
/// region (trimmed to the used range for whole columns).
fn data_range(s: &Session, p: &Json) -> Result<RangeRef> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    if p.get("range").is_some() {
        return target_range(s, p);
    }
    if let Some(t) = sh.table_at(d.selection.active) {
        return Ok(t.data_range().map(|dr| RangeRef::new(t.range.start, dr.end)).unwrap_or(t.range));
    }
    let sel = d.selection.current();
    if !sel.is_single() {
        // An empty sheet has no data to work on: just the active cell.
        let Some(used) = sh.used_range() else { return Ok(RangeRef::cell(d.selection.active)) };
        return Ok(sel.intersection(&used).unwrap_or(sel));
    }
    Ok(current_region(sh, d.selection.active))
}

/// The sheet and range a data command works on: the sheet `sheet` or a `Sheet!` prefix on `range`
/// names, else the active one. The selection belongs to the active sheet, so another sheet needs `range`.
fn data_target(s: &Session, p: &Json, cmd: &str) -> Result<(usize, RangeRef)> {
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    if let Some(t) = str_param(p, "range")
        && let (Some(name), _) = split_sheet(t)
        && d.wb.sheet_index(&name) != Some(sheet)
    {
        return Err(bad(cmd, format!("`{t}` names a sheet other than `sheet`")));
    }
    if sheet != d.wb.active_sheet && p.get("range").is_none() {
        return Err(bad(cmd, "missing `range`: the selection is on the active sheet"));
    }
    Ok((sheet, data_range(s, p)?))
}

/// The range Custom Sort works on and whether its first row looks like a header.
pub fn sort_context(s: &Session, p: &Json) -> Result<(RangeRef, bool)> {
    let d = s.doc()?;
    let sel = d.selection.current();
    // Unlike other data commands, an explicit block wins over the table it sits in, so a totals row can be sorted.
    let r = match d.wb.active().and_then(|sh| sh.used_range()) {
        Some(used) if p.get("range").is_none() && !sel.is_single() => sel.intersection(&used).unwrap_or(sel),
        _ => data_range(s, p)?,
    };
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    Ok((r, bool_param(p, "header").unwrap_or_else(|| guess_header(&d.wb, sh, r))))
}

/// Excel's header guess: the first row is text over a column of numbers, or is bold.
fn guess_header(wb: &Workbook, sh: &Sheet, r: RangeRef) -> bool {
    if let Some(t) = sh.table_at(r.start) {
        return t.header_row && r.start.row == t.range.start.row;
    }
    if r.height() < 2 {
        return false;
    }
    let mut votes = 0i32;
    for c in r.start.col..=r.end.col {
        let a = sh.value(CellRef::new(r.start.row, c));
        let b = sh.value(CellRef::new(r.start.row + 1, c));
        if a.is_text() && !b.is_text() && !b.is_empty() {
            votes += 1;
        }
        if wb.styles.get(sh.style_id(CellRef::new(r.start.row, c))).font.bold
            != wb.styles.get(sh.style_id(CellRef::new(r.start.row + 1, c))).font.bold
        {
            votes += 1;
        }
        if a.is_text() && b.is_text() {
            votes -= 0;
        }
    }
    votes > 0
}

#[derive(Clone)]
struct SortKey {
    col: u32,
    desc: bool,
    by: String,
    list: Option<Vec<String>>,
}

/// The sheet and range a sort works on: the sheet `sheet` or a `Sheet!` prefix on `range` names,
/// else the active one. The selection belongs to the active sheet, so another sheet needs `range`.
fn sort_target(s: &Session, p: &Json) -> Result<(usize, RangeRef)> {
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    if let Some(t) = str_param(p, "range")
        && let (Some(name), _) = split_sheet(t)
        && d.wb.sheet_index(&name) != Some(sheet)
    {
        return Err(bad("data.sort", format!("`{t}` names a sheet other than `sheet`")));
    }
    if sheet != d.wb.active_sheet && p.get("range").is_none() {
        return Err(bad("data.sort", "missing `range`: the selection is on the active sheet"));
    }
    Ok((sheet, data_range(s, p)?))
}

fn quick_sort(s: &mut Session, p: &Json, asc: bool) -> Result<Json> {
    let (sheet, r) = sort_target(s, p)?;
    let col = col_param(p.get("column"), r.start.col).unwrap_or(s.doc()?.selection.active.col).clamp(r.start.col, r.end.col);
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let header = guess_header(&d.wb, sh, r);
    do_sort(s, sheet, r, header, vec![SortKey { col, desc: !asc, by: "values".into(), list: None }], false, false)
}

fn sort(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = sort_target(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let header = bool_param(p, "header").unwrap_or_else(|| guess_header(&d.wb, sh, r));
    let by_cols = str_param(p, "orientation") == Some("columns");
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let mut keys: Vec<SortKey> = Vec::new();
    for k in p.get("keys").and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default() {
        let col = if by_cols {
            let row = match k.get("row").and_then(Json::as_u64) {
                // Worksheet rows are 1-based; 0 (or a value past u32) has no coordinate.
                Some(n) => {
                    let row = u32::try_from(n).ok().and_then(|n| n.checked_sub(1));
                    Some(row.ok_or_else(|| bad("data.sort", "sort key row must be 1 or greater"))?)
                }
                None => None,
            };
            row.or_else(|| col_param(k.get("column"), r.start.row))
        } else {
            col_param(k.get("column"), r.start.col)
        };
        let Some(col) = col else { continue };
        keys.push(SortKey {
            col,
            desc: k.get("order").and_then(Json::as_str) == Some("desc"),
            by: k.get("by").and_then(Json::as_str).unwrap_or("values").to_string(),
            list: k.get("customList").and_then(Json::as_array).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        });
    }
    if keys.is_empty() {
        let (r, header) = sort_context(s, p)?;
        s.ui_requests.push(crate::UiRequest::Dialog("sort".into(), json!({"range": r.a1(), "header": header})));
        return ok();
    }
    if keys.len() > 64 {
        return Err(bad("data.sort", "too many sort levels (64 max)"));
    }
    do_sort(s, sheet, r, header, keys, by_cols, case)
}

fn do_sort(s: &mut Session, sheet: usize, r: RangeRef, header: bool, keys: Vec<SortKey>, by_cols: bool, case: bool) -> Result<Json> {
    // Protect Sheet ▸ Sort. Both Sort… and the A-to-Z buttons come through here.
    super::refuse_unless_allowed(s, sheet, "Sorting", |p| p.sort)?;
    let body = if by_cols {
        if header && r.width() > 1 { RangeRef::new(CellRef::new(r.start.row, r.start.col + 1), r.end) } else { r }
    } else if header && r.height() > 1 {
        RangeRef::new(CellRef::new(r.start.row + 1, r.start.col), r.end)
    } else {
        r
    };
    if body.count() > 20_000_000 {
        return Err(bad("data.sort", "range too large"));
    }
    edit(s, |cx| {
        let Some(sh) = cx.wb.sheet(sheet) else { return Ok(Json::Null) };
        if sh.merges.iter().any(|m| m.intersects(&body) && !m.is_single()) {
            return Err(EngineError::Other("To do this, all the merged cells need to be the same size.".into()));
        }
        let n = if by_cols { body.width() } else { body.height() };
        let line = |i: u32, k: u32| if by_cols { CellRef::new(k, body.start.col + i) } else { CellRef::new(body.start.row + i, k) };
        let theme = cx.wb.theme.clone();
        let styles = &cx.wb.styles;
        let key_val = |i: u32, k: &SortKey| -> (Value, u32) {
            let c = line(i, k.col);
            let v = sh.value(c);
            let color = match k.by.as_str() {
                "cellColor" => {
                    styles.get(sh.style_id(c)).fill.fg.resolve(&theme).map(|[r, g, b]| u32::from_be_bytes([0, r, g, b])).unwrap_or(u32::MAX)
                }
                "fontColor" => {
                    styles.get(sh.style_id(c)).font.color.resolve(&theme).map(|[r, g, b]| u32::from_be_bytes([0, r, g, b])).unwrap_or(u32::MAX)
                }
                _ => 0,
            };
            (v, color)
        };
        let mut order: Vec<u32> = (0..n).collect();
        let keyed: Vec<Vec<(Value, u32)>> = (0..n).map(|i| keys.iter().map(|k| key_val(i, k)).collect()).collect();
        order.sort_by(|&a, &b| {
            for (ki, k) in keys.iter().enumerate() {
                let (va, ca) = &keyed[a as usize][ki];
                let (vb, cb) = &keyed[b as usize][ki];
                let o = if k.by != "values" {
                    ca.cmp(cb)
                } else if let Some(list) = &k.list {
                    let pos = |v: &Value| v.as_text().and_then(|t| list.iter().position(|x| x.eq_ignore_ascii_case(t))).unwrap_or(usize::MAX);
                    pos(va).cmp(&pos(vb))
                } else if case && va.is_text() && vb.is_text() {
                    sort_compare(va, vb).then_with(|| case_order(va.as_text().unwrap_or(""), vb.as_text().unwrap_or("")))
                } else {
                    sort_compare(va, vb)
                };
                // Blanks always sort last, whatever the order.
                let o = if k.desc && !(va.is_empty() || vb.is_empty()) { o.reverse() } else { o };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
        if order.iter().enumerate().all(|(i, &o)| i as u32 == o) {
            return Ok(Json::Null);
        }
        // Move whole lines (cells with formulas adjusted for relative references).
        let lanes: Vec<u32> = if by_cols { (body.start.row..=body.end.row).collect() } else { (body.start.col..=body.end.col).collect() };
        // Notes and hyperlinks hang off a cell, not a row, so they travel with the row they
        // describe — the way cell pictures and row heights do. Collect them before inserting,
        // so a swap can't resurrect one from a destination that is also a source.
        let side: Vec<(CellRef, Comment)> = order
            .iter()
            .enumerate()
            .flat_map(|(dst_i, &src_i)| lanes.iter().map(move |&k| (src_i, dst_i as u32, k)))
            .filter_map(|(src_i, dst_i, k)| sh.comments.get(&line(src_i, k)).map(|cm| (line(dst_i, k), cm.clone())))
            .collect();
        let links: Vec<(CellRef, Hyperlink)> = order
            .iter()
            .enumerate()
            .flat_map(|(dst_i, &src_i)| lanes.iter().map(move |&k| (src_i, dst_i as u32, k)))
            .filter_map(|(src_i, dst_i, k)| sh.hyperlinks.get(&line(src_i, k)).map(|h| (line(dst_i, k), h.clone())))
            .collect();
        let mut moved: Vec<(CellRef, Option<Cell>)> = Vec::with_capacity(body.count() as usize);
        let mut pictures = Vec::new();
        for (dst_i, &src_i) in order.iter().enumerate() {
            for &k in &lanes {
                let src = line(src_i, k);
                let dst = line(dst_i as u32, k);
                let mut cell = sh.cell(src).cloned();
                if let Some(c) = cell.as_mut()
                    && let Some(f) = &c.formula
                    && let Some(e) = f.expr()
                {
                    let shifted = gridcraft_formula::adjust::shift_relative(e, dst.row as i64 - src.row as i64, dst.col as i64 - src.col as i64);
                    c.formula = Some(Arc::new(Formula::from_expr(shifted)));
                }
                if let Some(picture) = sh.cell_pictures.get(&src) {
                    pictures.push((dst, picture.clone()));
                }
                moved.push((dst, cell));
            }
        }
        // Row heights travel with rows.
        let heights: Vec<Option<LineInfo>> =
            if by_cols { vec![] } else { order.iter().map(|&i| sh.rows.get(&(body.start.row + i)).copied()).collect() };
        // Floating objects anchored inside the sorted body travel with their lines, unless they
        // are pinned absolutely ("Don't move or size with cells", `editAs="absolute"`). Reordering
        // is the inverse of `order`: `dst_of[src] = dst`.
        let dst_of: Vec<u32> = {
            let mut v = vec![0u32; order.len()];
            for (dst_i, &src_i) in order.iter().enumerate() {
                if let Some(slot) = v.get_mut(src_i as usize) {
                    *slot = dst_i as u32;
                }
            }
            v
        };
        // The object's own geometry (its top-left anchor cell, offset and size) is read from the
        // pre-sort `sh`; a two-cell object's bottom-right cell is derived from its size here so it
        // can be re-fitted to the lines it spans after the sort.
        let objs: Vec<Anchor> =
            sh.images.iter().map(|o| o.anchor).chain(sh.charts.iter().map(|o| o.anchor)).chain(sh.shapes.iter().map(|o| o.anchor)).collect();
        let descs: Vec<(u32, u32, f32, f32)> = objs
            .iter()
            .map(|a| {
                let x2 = sh.col_left(a.cell.col) + a.dx as f64 + a.width.max(0.0) as f64;
                let y2 = sh.row_top(a.cell.row) + a.dy as f64 + a.height.max(0.0) as f64;
                let tc = sh.col_at(x2);
                let tr = sh.row_at(y2);
                (tc, tr, (x2 - sh.col_left(tc)) as f32, (y2 - sh.row_top(tr)) as f32)
            })
            .collect();
        let shm = cx.sheet_mut(sheet)?;
        for (c, cell) in &moved {
            match cell {
                Some(x) => shm.set_cell(*c, x.clone()),
                None => {
                    shm.remove_cell(*c);
                }
            }
        }
        // Every cell of the body is the destination of exactly one source cell, so clearing the
        // body and inserting the permuted entries is the same permutation the cells went through.
        for c in body.iter() {
            shm.comments.remove(&c);
            shm.hyperlinks.remove(&c);
        }
        for (at, cm) in side {
            shm.comments.insert(at, cm);
        }
        for (at, h) in links {
            shm.hyperlinks.insert(at, h);
        }
        shm.cell_pictures.extend(pictures);
        for (i, h) in heights.into_iter().enumerate() {
            let row = body.start.row + i as u32;
            match h {
                Some(info) => {
                    shm.rows.insert(row, LineInfo { hidden: shm.rows.get(&row).is_some_and(|x| x.hidden), ..info });
                }
                None => {
                    if let Some(e) = shm.rows.get_mut(&row) {
                        e.size = None;
                    }
                }
            }
        }
        // Re-anchor floating objects now that the row heights are final. `objs` was collected as
        // images ++ charts ++ shapes, so the same index maps straight onto each vec.
        // Only objects anchored inside the sorted block move: within its lines on the sort axis
        // and within its span on the other axis (a picture beside the block stays put).
        let (b0, b1) = if by_cols { (body.start.col, body.end.col) } else { (body.start.row, body.end.row) };
        let (o0, o1) = if by_cols { (body.start.row, body.end.row) } else { (body.start.col, body.end.col) };
        let ni = shm.images.len();
        let nch = shm.charts.len();
        for (i, (a, desc)) in objs.into_iter().zip(descs).enumerate() {
            let other = if by_cols { a.cell.row } else { a.cell.col };
            if other < o0 || other > o1 {
                continue;
            }
            let na = sorted_anchor(shm, a, desc, by_cols, b0, b1, &dst_of);
            if na != a {
                if i < ni {
                    if let Some(o) = shm.images.get_mut(i) {
                        o.anchor = na;
                    }
                } else if i < ni + nch {
                    if let Some(o) = shm.charts.get_mut(i - ni) {
                        o.anchor = na;
                    }
                } else if let Some(o) = shm.shapes.get_mut(i - ni - nch) {
                    o.anchor = na;
                }
            }
        }
        for (c, _) in moved {
            cx.changed.push((sheet, c));
        }
        cx.structural = true;
        Ok(Json::Null)
    })?;
    Ok(json!({"sorted": body.a1()}))
}

/// Re-anchor one floating object for a sort. `bottom` is the object's bottom-right line and the
/// offset of that corner inside it, measured before the sort. `map` (via `dst_of`) translates a
/// line index on the moving axis and is the identity outside `b0..=b1`. `sh` is the post-sort
/// grid (row heights already reordered), so re-fitting uses the heights the object lands on.
/// Objects pinned absolutely ("Don't move or size with cells") never move; "move but don't size"
/// keeps its size; "move and size" is re-fitted when its whole extent stays inside the body.
fn sorted_anchor(sh: &Sheet, a: Anchor, bottom: (u32, u32, f32, f32), by_cols: bool, b0: u32, b1: u32, dst_of: &[u32]) -> Anchor {
    if a.mode == AnchorMode::Absolute {
        return a;
    }
    let (tc, tr, tdx, tdy) = bottom;
    let map = |v: u32| -> u32 { if v >= b0 && v <= b1 { dst_of.get((v - b0) as usize).map_or(v, |&d| b0 + d) } else { v } };
    let (axis_start, axis_end) = if by_cols { (a.cell.col, tc) } else { (a.cell.row, tr) };
    if axis_start < b0 || axis_start > b1 {
        return a;
    }
    let (nr, nc) = if by_cols { (a.cell.row, map(a.cell.col)) } else { (map(a.cell.row), a.cell.col) };
    if a.mode == AnchorMode::MoveAndSize && axis_end >= b0 && axis_end <= b1 {
        let (nbr, nbc) = if by_cols { (tr, map(tc)) } else { (map(tr), tc) };
        let x1 = sh.col_left(nc) + a.dx as f64;
        let y1 = sh.row_top(nr) + a.dy as f64;
        let x2 = sh.col_left(nbc) + tdx as f64;
        let y2 = sh.row_top(nbr) + tdy as f64;
        Anchor { cell: CellRef::new(nr, nc), dx: a.dx, dy: a.dy, width: (x2 - x1).max(0.0) as f32, height: (y2 - y1).max(0.0) as f32, mode: a.mode }
    } else {
        Anchor { cell: CellRef::new(nr, nc), ..a }
    }
}

// ---------------------------------------------------------------- filter

fn toggle_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.filter")?;
    super::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    // The active cell belongs to the active sheet: on another sheet the table is the one at `range`.
    let at = if sheet == s.doc()?.wb.active_sheet { s.doc()?.selection.active } else { r.start };
    let has = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.autofilter.is_some());
    let in_table = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.table_at(at).map(|t| t.name.clone()));
    if let Some(name) = in_table {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                }
            }
            Ok(Json::Null)
        });
    }
    if has {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            // Reveal only rows the filter hid; rows hidden by hand or by a collapsed group stay hidden.
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            sh.autofilter = None;
            for r in filtered {
                if let Some(info) = sh.rows.get_mut(&r) {
                    info.hidden = false;
                }
            }
            cx.structural = true;
            Ok(json!({"filter": false}))
        });
    }
    install_filter(s, p)
}

/// Turn a sheet AutoFilter on over `p.range`, or over the data range of the selection.
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, mut r) = data_target(s, p, "data.filter")?;
    // A selected header row filters the data below it (#201).
    if p.get("range").is_none() && r.height() == 1 {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        let below = current_region(sh, r.start).end.row;
        if below > r.end.row {
            r = RangeRef::new(r.start, CellRef::new(below, r.end.col));
        }
    }
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.autofilter = Some(AutoFilter { range: r, criteria: vec![] });
        Ok(json!({"filter": true, "range": r.a1()}))
    })
}

fn filter_by(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet;
    super::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let af = match &sh.autofilter {
        Some(a) => a.clone(),
        None => {
            install_filter(s, &json!({}))?;
            s.doc()?.wb.sheet(sheet).and_then(|sh| sh.autofilter.clone()).ok_or_else(|| EngineError::Other("no filter".into()))?
        }
    };
    let col = match col_param(p.get("column"), af.range.start.col) {
        Some(c) => c,
        // A number too large to be a column is outside the filter range, not a missing parameter.
        None if p.get("column").is_some_and(|v| v.as_u64().is_some()) => return Err(bad("data.filterBy", "column outside the filter range")),
        None => return Err(bad("data.filterBy", "missing `column`")),
    };
    if col < af.range.start.col || col > af.range.end.col {
        return Err(bad("data.filterBy", "column outside the filter range"));
    }
    let off = col - af.range.start.col;
    let crit = if bool_param(p, "clear").unwrap_or(false) {
        None
    } else if let Some(vals) = p.get("values").and_then(Json::as_array) {
        Some(FilterCriterion::Values {
            values: vals.iter().map(super::edit::json_to_input).collect(),
            blanks: bool_param(p, "blanks").unwrap_or(false),
        })
    } else if let Some(c) = p.get("custom") {
        let a = (c.get("op").and_then(Json::as_str).unwrap_or("=").to_string(), c.get("value").map(super::edit::json_to_input).unwrap_or_default());
        let b = c.get("op2").and_then(Json::as_str).map(|o| (o.to_string(), c.get("value2").map(super::edit::json_to_input).unwrap_or_default()));
        Some(FilterCriterion::Custom { a, b, and: c.get("and").and_then(Json::as_bool).unwrap_or(true) })
    } else if let Some(t) = p.get("top") {
        Some(FilterCriterion::Top10 {
            bottom: t.get("bottom").and_then(Json::as_bool).unwrap_or(false),
            percent: t.get("percent").and_then(Json::as_bool).unwrap_or(false),
            count: t.get("count").and_then(Json::as_u64).unwrap_or(10) as u32,
        })
    } else {
        bool_param(p, "aboveAverage").map(FilterCriterion::AboveAverage)
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.retain(|(o, _)| *o != off);
            if let Some(c) = crit.clone() {
                a.criteria.push((off, c));
            }
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })?;
    let hidden = s.doc()?.wb.sheet(sheet).map(|sh| sh.rows.values().filter(|r| r.hidden).count()).unwrap_or(0);
    Ok(json!({"hiddenRows": hidden}))
}

/// Does a value pass a custom comparison (`>`, `=*abc*`, `<>x`…)?
fn custom_ok(v: &Value, text: &str, op: &str, crit: &str) -> bool {
    let num = gridcraft_core::parse::parse_number_text(crit);
    let cmp = match (v.as_f64(), num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        // A numeric ordering comparison only matches numbers; blanks and text never do.
        (None, Some(_)) if matches!(op, "<" | "<=" | ">" | ">=") => None,
        _ => Some(gridcraft_core::compare_text(text, crit)),
    };
    let wild = || super::edit::wildcard(&text.to_lowercase(), &crit.to_lowercase());
    match op {
        "=" => {
            if crit.contains(['*', '?']) {
                wild()
            } else {
                cmp == Some(Ordering::Equal)
            }
        }
        "<>" => {
            if crit.contains(['*', '?']) {
                !wild()
            } else {
                cmp != Some(Ordering::Equal)
            }
        }
        ">" => cmp == Some(Ordering::Greater),
        "<" => cmp == Some(Ordering::Less),
        ">=" => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
        "<=" => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
        "beginsWith" => text.to_lowercase().starts_with(&crit.to_lowercase()),
        "endsWith" => text.to_lowercase().ends_with(&crit.to_lowercase()),
        "contains" => text.to_lowercase().contains(&crit.to_lowercase()),
        "notContains" => !text.to_lowercase().contains(&crit.to_lowercase()),
        _ => true,
    }
}

/// The filter's "(Blanks)" group: truly empty cells and zero-length text such as a `=""` result.
fn filter_blank(v: &Value) -> bool {
    v.is_empty() || matches!(v, Value::Text(t) if t.is_empty())
}

/// Excel grows an AutoFilter range when a value is entered in the row right below it, in one of its columns (#2342).
/// A table's own filter is left to the table, and a row typed after a blank row does not count.
/// A formula counts before it is calculated: this runs inside the edit, ahead of the recalc.
pub(crate) fn grow_autofilter(sh: &mut Sheet, at: CellRef) {
    if !sh.cells.get(at).is_some_and(|c| !filter_blank(&c.value) || c.formula.is_some()) {
        return;
    }
    let Some(af) = sh.autofilter.as_ref() else { return };
    let r = af.range;
    if r.end.row.checked_add(1) != Some(at.row) || at.col < r.start.col || at.col > r.end.col || sh.tables.iter().any(|t| t.range == r) {
        return;
    }
    let grown = RangeRef::new(r.start, CellRef::new(at.row, r.end.col));
    if let Some(af) = sh.autofilter.as_mut() {
        af.range = grown;
    }
}

/// Hides rows of the autofilter range that fail any criterion.
pub(crate) fn apply_filter(wb: &mut Workbook, sheet: usize) {
    let Some(sh) = wb.sheet(sheet) else { return };
    let Some(af) = sh.autofilter.clone() else { return };
    let r = af.range;
    let mut hide = Vec::new();
    // Precompute per-criterion top10 threshold and average once, not per row.
    let mut col_stats: std::collections::HashMap<usize, (Option<f64>, f64)> = std::collections::HashMap::new();
    for (i, (off, c)) in af.criteria.iter().enumerate() {
        if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
            let col = r.start.col + off;
            let mut nums: Vec<f64> = (r.start.row + 1..=r.end.row).filter_map(|row| sh.value(CellRef::new(row, col)).as_f64()).collect();
            let avg = if nums.is_empty() { 0.0 } else { nums.iter().sum::<f64>() / nums.len() as f64 };
            let thr = match c {
                FilterCriterion::Top10 { bottom, percent, count } => {
                    nums.sort_by(|a, b| if *bottom { a.partial_cmp(b) } else { b.partial_cmp(a) }.unwrap_or(Ordering::Equal));
                    let k = if *percent { ((nums.len() as f64) * (*count as f64) / 100.0).ceil() as usize } else { *count as usize };
                    let k = k.clamp(1, nums.len().max(1));
                    nums.get(k - 1).copied()
                }
                _ => None,
            };
            col_stats.insert(i, (thr, avg));
        }
    }
    for row in r.start.row + 1..=r.end.row {
        let mut visible = true;
        for (i, (off, crit)) in af.criteria.iter().enumerate() {
            let c = CellRef::new(row, r.start.col + off);
            let v = sh.value(c);
            let text = crate::display::cell_text(wb, sh, c);
            let ok = match crit {
                FilterCriterion::Values { values, blanks } => {
                    if filter_blank(&v) {
                        *blanks
                    } else {
                        values.iter().any(|x| x.eq_ignore_ascii_case(&text) || x.eq_ignore_ascii_case(&v.display()))
                    }
                }
                FilterCriterion::Custom { a, b, and } => {
                    let x = custom_ok(&v, &text, &a.0, &a.1);
                    match b {
                        Some(b) => {
                            let y = custom_ok(&v, &text, &b.0, &b.1);
                            if *and { x && y } else { x || y }
                        }
                        None => x,
                    }
                }
                FilterCriterion::Top10 { bottom, .. } => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((thr, _))) => {
                        let thr = thr.unwrap_or(n);
                        if *bottom { n <= thr } else { n >= thr }
                    }
                    _ => false,
                },
                FilterCriterion::AboveAverage(above) => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((_, avg))) => {
                        if *above {
                            n > *avg
                        } else {
                            n < *avg
                        }
                    }
                    _ => false,
                },
                FilterCriterion::FillColor(col) => wb.styles.get(sh.style_id(c)).fill.fg == *col,
                FilterCriterion::FontColor(col) => wb.styles.get(sh.style_id(c)).font.color == *col,
                // Icons are not evaluated here: the row keeps its current visibility.
                FilterCriterion::Icon { .. } => !sh.is_row_hidden(row),
            };
            if !ok {
                visible = false;
                break;
            }
        }
        hide.push((row, !visible));
    }
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    for (row, h) in hide {
        if h {
            shm.rows.entry(row).or_default().hidden = true;
        } else if let Some(e) = shm.rows.get_mut(&row) {
            e.hidden = false;
        }
    }
}

fn clear_filter(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.clear();
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn reapply(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Distinct display values of a filter column (for the dropdown list).
pub fn filter_values(wb: &Workbook, sheet: usize, col: u32) -> Vec<(String, bool)> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = &sh.autofilter else { return vec![] };
    let off = col.saturating_sub(af.range.start.col);
    let selected: Option<&FilterCriterion> = af.criteria.iter().find(|(o, _)| *o == off).map(|(_, c)| c);
    let mut seen = std::collections::BTreeMap::new();
    for row in af.range.start.row + 1..=af.range.end.row.min(af.range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = match selected {
            Some(FilterCriterion::Values { values, blanks }) => {
                if blank {
                    *blanks
                } else {
                    values.iter().any(|x| x.eq_ignore_ascii_case(&t))
                }
            }
            _ => true,
        };
        let key = if blank { sort_key(&Value::Empty) } else { sort_key(&v) };
        seen.entry((key, t)).or_insert(on);
    }
    seen.into_iter().map(|((_, t), on)| (t, on)).collect()
}

/// Case-sensitive tie-break for text that is equal ignoring case: as in Excel, lowercase sorts
/// before uppercase at the first letter that differs in case.
fn case_order(a: &str, b: &str) -> Ordering {
    a.chars().zip(b.chars()).find(|(x, y)| x != y).map_or(Ordering::Equal, |(x, y)| y.is_lowercase().cmp(&x.is_lowercase()))
}

fn sort_key(v: &Value) -> (u8, i64, String) {
    match v {
        Value::Number(n) => (0, (n * 1e6) as i64, String::new()),
        Value::Text(t) => (1, 0, t.to_lowercase()),
        Value::Bool(b) => (2, *b as i64, String::new()),
        Value::Empty => (9, 0, String::new()),
        _ => (3, 0, v.display()),
    }
}

// ---------------------------------------------------------------- tools

fn remove_duplicates(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.removeDuplicates")?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    // Rows and columns past the data are blank: only the part with data is compared (whole
    // columns compared a million blank rows and reported them as removed duplicates).
    let r = sh.used_range().and_then(|u| r.intersection(&u)).unwrap_or(RangeRef::cell(r.start));
    let header = bool_param(p, "header").unwrap_or_else(|| guess_header(&d.wb, sh, r));
    let cols: Vec<u32> = p
        .get("columns")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|c| col_param(Some(c), r.start.col)).collect())
        .unwrap_or_else(|| (r.start.col..=r.end.col).collect());
    // With no column to compare, every row key is empty: the first row would survive and every
    // other row would be deleted as a duplicate. A column outside the range compares data the
    // user didn't select. Excel refuses both.
    if cols.is_empty() || cols.iter().any(|c| !(r.start.col..=r.end.col).contains(c)) {
        return Err(bad("data.removeDuplicates", "`columns` must name at least one column of the range"));
    }
    let start = r.start.row + header as u32;
    let mut removed = 0;
    edit(s, |cx| {
        let Some(sh) = cx.wb.sheet(sheet) else { return Ok(()) };
        let mut seen = std::collections::HashSet::new();
        let mut keep: Vec<u32> = Vec::new();
        // Cells are compared by the text they show, as in Excel: the same date in two formats
        // isn't a duplicate, while 1.01 and 1.02 shown as 1.0 are.
        let shown = |c: CellRef| crate::display::format(&sh.value(c), cx.wb.styles.get(sh.style_id(c)).num_fmt.as_str(), &cx.wb).text;
        for row in start..=r.end.row {
            let key: Vec<String> = cols.iter().map(|c| shown(CellRef::new(row, *c)).to_lowercase()).collect();
            if seen.insert(key) {
                keep.push(row);
            } else {
                removed += 1;
            }
        }
        let pictures: Vec<_> = keep
            .iter()
            .enumerate()
            .flat_map(|(i, row)| {
                (r.start.col..=r.end.col)
                    .filter_map(move |col| sh.cell_pictures.get(&CellRef::new(*row, col)).map(|p| (CellRef::new(start + i as u32, col), p.clone())))
            })
            .collect();
        // Rows a filter keeps are compacted to `start + i`, so the notes and hyperlinks that
        // describe them move with them; the ones on removed rows are dropped with the row.
        let notes: Vec<_> = keep
            .iter()
            .enumerate()
            .flat_map(|(i, row)| {
                (r.start.col..=r.end.col)
                    .filter_map(move |col| sh.comments.get(&CellRef::new(*row, col)).map(|cm| (CellRef::new(start + i as u32, col), cm.clone())))
            })
            .collect();
        let links: Vec<_> = keep
            .iter()
            .enumerate()
            .flat_map(|(i, row)| {
                (r.start.col..=r.end.col)
                    .filter_map(move |col| sh.hyperlinks.get(&CellRef::new(*row, col)).map(|h| (CellRef::new(start + i as u32, col), h.clone())))
            })
            .collect();
        let rows: Vec<Vec<Option<Cell>>> =
            keep.iter().map(|row| (r.start.col..=r.end.col).map(|c| sh.cell(CellRef::new(*row, c)).cloned()).collect()).collect();
        let shm = cx.sheet_mut(sheet)?;
        for row in start..=r.end.row {
            for c in r.start.col..=r.end.col {
                shm.remove_cell(CellRef::new(row, c));
            }
        }
        for row in start..=r.end.row {
            for c in r.start.col..=r.end.col {
                let at = CellRef::new(row, c);
                shm.comments.remove(&at);
                shm.hyperlinks.remove(&at);
            }
        }
        for (i, cells) in rows.into_iter().enumerate() {
            for (j, cell) in cells.into_iter().enumerate() {
                if let Some(cell) = cell {
                    shm.set_cell(CellRef::new(start + i as u32, r.start.col + j as u32), cell);
                }
            }
        }
        shm.cell_pictures.extend(pictures);
        for (at, cm) in notes {
            shm.comments.insert(at, cm);
        }
        for (at, h) in links {
            shm.hyperlinks.insert(at, h);
        }
        for c in RangeRef::new(CellRef::new(start, r.start.col), r.end).iter() {
            cx.changed.push((sheet, c));
        }
        Ok(())
    })?;
    let remaining = (r.end.row + 1 - start) as usize - removed;
    Ok(json!({"removed": removed, "remaining": remaining}))
}

fn text_to_columns(s: &mut Session, p: &Json) -> Result<Json> {
    let r = target_range(s, p)?;
    let sheet = target_sheet(s, p)?;
    let mut delims: Vec<char> = p
        .get("delimiters")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|d| d.as_str().and_then(|x| x.chars().next())).collect())
        .unwrap_or_else(|| vec!['\t']);
    if let Some(o) = str_param(p, "other").and_then(|o| o.chars().next()) {
        delims.push(o);
    }
    let fixed: Option<Vec<usize>> =
        p.get("fixedWidths").and_then(Json::as_array).map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect());
    let consecutive = bool_param(p, "treatConsecutive").unwrap_or(false);
    let quote = str_param(p, "textQualifier").and_then(|q| q.chars().next()).unwrap_or('"');
    let dest = cell_param_on(s, p, "destination", sheet)?.unwrap_or(r.start);
    edit(s, |cx| {
        let Some(sh) = cx.wb.sheet(sheet) else { return Ok(()) };
        let mut out: Vec<(CellRef, String)> = Vec::new();
        for (i, row) in (r.start.row..=r.end.row).enumerate() {
            let text = sh.value(CellRef::new(row, r.start.col)).display();
            let parts: Vec<String> = match &fixed {
                Some(w) => {
                    let chars: Vec<char> = text.chars().collect();
                    let mut cuts = vec![0];
                    let mut breaks: Vec<usize> = w.iter().copied().filter(|x| *x > 0 && *x < chars.len()).collect();
                    breaks.sort_unstable();
                    breaks.dedup();
                    cuts.extend(breaks);
                    cuts.push(chars.len());
                    cuts.windows(2)
                        .map(|p| chars.get(p[0]..p[1]).map(|s| s.iter().collect::<String>().trim().to_string()).unwrap_or_default())
                        .collect()
                }
                None => split_delimited(&text, &delims, consecutive, quote),
            };
            for (j, part) in parts.into_iter().enumerate() {
                out.push((CellRef::new(dest.row + i as u32, dest.col + j as u32), part));
            }
        }
        for (c, t) in out {
            let cell = super::edit::input_to_cell(&t, sheet, c, &mut cx.wb).unwrap_or(None);
            let shm = cx.sheet_mut(sheet)?;
            match cell {
                Some(x) => shm.set_cell(c, x),
                None => {
                    shm.remove_cell(c);
                }
            }
            cx.changed.push((sheet, c));
        }
        Ok(())
    })?;
    ok()
}

fn split_delimited(text: &str, delims: &[char], consecutive: bool, quote: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut last_delim = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == quote {
            if in_q && chars.peek() == Some(&quote) {
                cur.push(quote);
                chars.next();
            } else {
                in_q = !in_q;
            }
            continue;
        }
        if !in_q && delims.contains(&c) {
            if consecutive && last_delim {
                continue;
            }
            out.push(std::mem::take(&mut cur));
            last_delim = true;
            continue;
        }
        last_delim = false;
        cur.push(c);
    }
    out.push(cur);
    out
}

// ---------------------------------------------------------------- validation

fn op_param(s: Option<&str>) -> CfOperator {
    match s.unwrap_or("between") {
        "notBetween" => CfOperator::NotBetween,
        "equal" => CfOperator::Equal,
        "notEqual" => CfOperator::NotEqual,
        "greater" | "greaterThan" => CfOperator::Greater,
        "less" | "lessThan" => CfOperator::Less,
        "greaterOrEqual" | "greaterThanOrEqual" => CfOperator::GreaterOrEqual,
        "lessOrEqual" | "lessThanOrEqual" => CfOperator::LessOrEqual,
        _ => CfOperator::Between,
    }
}

/// `r` minus `cut`, as up to four non-overlapping rectangles.
fn subtract_range(r: RangeRef, cut: &RangeRef) -> Vec<RangeRef> {
    let Some(i) = r.intersection(cut) else { return vec![r] };
    let mut out = Vec::new();
    if i.start.row > r.start.row {
        out.push(RangeRef::new(r.start, CellRef::new(i.start.row - 1, r.end.col)));
    }
    if i.end.row < r.end.row {
        out.push(RangeRef::new(CellRef::new(i.end.row + 1, r.start.col), r.end));
    }
    if i.start.col > r.start.col {
        out.push(RangeRef::new(CellRef::new(i.start.row, r.start.col), CellRef::new(i.end.row, i.start.col - 1)));
    }
    if i.end.col < r.end.col {
        out.push(RangeRef::new(CellRef::new(i.start.row, i.end.col + 1), CellRef::new(i.end.row, r.end.col)));
    }
    out
}

/// Removes the cells of `cuts` from every validation, keeping the rest of each rule's ranges.
pub(super) fn remove_validation_cells(validations: &mut Vec<Validation>, cuts: &[RangeRef]) {
    for dv in validations.iter_mut() {
        for cut in cuts {
            dv.ranges = dv.ranges.iter().flat_map(|r| subtract_range(*r, cut)).collect();
        }
    }
    validations.retain(|d| !d.ranges.is_empty());
}

fn validation(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let sheet = target_sheet(s, p)?;
    if bool_param(p, "clear").unwrap_or(false) {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            remove_validation_cells(&mut sh.validations, &ranges);
            Ok(Json::Null)
        });
    }
    let Some(kind) = str_param(p, "type") else {
        s.ui_requests.push(crate::UiRequest::Dialog("dataValidation".into(), json!({})));
        return ok();
    };
    let kind = match kind {
        "whole" => ValidationKind::Whole,
        "decimal" => ValidationKind::Decimal,
        "list" => ValidationKind::List,
        "date" => ValidationKind::Date,
        "time" => ValidationKind::Time,
        "textLength" => ValidationKind::TextLength,
        "custom" => ValidationKind::Custom,
        "any" => ValidationKind::Any,
        // An unknown type used to give an "any value" rule, saved as `type="none"`: no validation.
        other => return Err(bad("data.validation", format!("unknown type `{other}` (any, whole, decimal, list, date, time, textLength, custom)"))),
    };
    if kind == ValidationKind::List && str_param(p, "formula1").is_none_or(|f| f.trim().trim_start_matches('=').is_empty()) {
        return Err(bad("data.validation", "a list validation needs `formula1`"));
    }
    const OPERATORS: [&str; 12] = [
        "between",
        "notBetween",
        "equal",
        "notEqual",
        "greater",
        "greaterThan",
        "less",
        "lessThan",
        "greaterOrEqual",
        "greaterThanOrEqual",
        "lessOrEqual",
        "lessThanOrEqual",
    ];
    if let Some(op) = str_param(p, "operator").filter(|op| !OPERATORS.contains(op)) {
        return Err(bad("data.validation", format!("unknown operator `{op}` ({})", OPERATORS.join(", "))));
    }
    let error_style = match str_param(p, "errorStyle") {
        None | Some("stop") => ErrorStyle::Stop,
        Some("warning") => ErrorStyle::Warning,
        Some("information") => ErrorStyle::Information,
        Some(other) => return Err(bad("data.validation", format!("unknown errorStyle `{other}` (stop, warning, information)"))),
    };
    let f1 = str_param(p, "formula1").unwrap_or("");
    let f1 = if kind == ValidationKind::List { list_source(f1) } else { f1.trim_start_matches('=').to_string() };
    let dv = Validation {
        ranges: ranges.clone(),
        kind,
        op: op_param(str_param(p, "operator")),
        f1,
        f2: str_param(p, "formula2").map(|f| f.trim_start_matches('=').to_string()),
        allow_blank: bool_param(p, "ignoreBlank").unwrap_or(true),
        in_cell_dropdown: bool_param(p, "dropdown").unwrap_or(true),
        input_title: str_param(p, "inputTitle").unwrap_or("").into(),
        input_message: str_param(p, "inputMessage").unwrap_or("").into(),
        show_input: true,
        error_title: str_param(p, "errorTitle").unwrap_or("").into(),
        error_message: str_param(p, "errorMessage").unwrap_or("").into(),
        error_style,
        show_error: true,
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        remove_validation_cells(&mut sh.validations, &ranges);
        sh.validations.push(dv.clone());
        Ok(Json::Null)
    })
}

/// Whether a list rule's source is a literal list (`"a,b"`, or `a,b` typed without quotes) rather
/// than a reference or a name to evaluate.
fn is_literal_list(f: &str) -> bool {
    f.starts_with('"')
        || (!f.contains('!')
            && gridcraft_formula::parse(f).map(|e| !matches!(e, gridcraft_formula::Expr::Ref(_) | gridcraft_formula::Expr::Name(_))).unwrap_or(true))
}

/// A list rule's source as a file stores it. A literal list typed without quotes, as in Excel's
/// Source box (`Open,In progress,Closed`), becomes the quoted string Excel saves
/// (`"Open,In progress,Closed"`, inner quotes doubled); unquoted, the file would read it as a
/// formula. A formula (`=…`), a reference, a name or an already quoted list is kept as before.
fn list_source(f: &str) -> String {
    let t = f.trim();
    if t.is_empty() || t.starts_with('=') || t.starts_with('"') || !is_literal_list(t) {
        return f.trim_start_matches('=').to_string();
    }
    format!("\"{}\"", t.replace('"', "\"\""))
}

/// List entries of a list validation (literal or from a range).
pub fn list_items(wb: &Workbook, sheet: usize, dv: &Validation) -> Vec<String> {
    let f = dv.f1.trim();
    if is_literal_list(f) {
        // A quoted list is a string literal: `""` inside stands for one `"`.
        let items = match f.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            Some(body) => body.replace("\"\"", "\""),
            None => f.trim_matches('"').to_string(),
        };
        return items.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    let at = dv.ranges.first().map(|r| r.start).unwrap_or_default();
    match gridcraft_calc::evaluate(wb, sheet, at, f) {
        Value::Array(a) => a.data.iter().filter(|v| !v.is_empty()).map(Value::display).collect(),
        v if !v.is_empty() && !v.is_error() => vec![v.display()],
        _ => vec![],
    }
}

/// Checks typed input against the validation of a cell. `Ok(None)` = valid.
pub fn check_validation(wb: &Workbook, sheet: usize, at: CellRef, input: &str) -> Option<(Validation, String)> {
    let sh = wb.sheet(sheet)?;
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(at)))?;
    if dv.kind == ValidationKind::Any || !dv.show_error {
        return None;
    }
    if input.is_empty() && dv.allow_blank {
        return None;
    }
    let v = gridcraft_core::parse::parse_input(input, wb.date_system).value;
    // The formulas are written for the first cell of the range and see the value being entered,
    // as in Excel (a custom `=A1>10` on A1:A9 checks A5's new value at A5).
    let first = dv.ranges.first().map(|r| r.start).unwrap_or(at);
    let entered;
    let wb = if sh.value(at) == v {
        wb
    } else {
        let mut w = wb.clone();
        if let Some(sh) = w.sheet_mut(sheet) {
            let style = sh.style_id(at);
            sh.cells.set(at, Cell { value: v.clone(), style, ..Default::default() });
        }
        entered = w;
        &entered
    };
    let eval = |f: &str| gridcraft_calc::evaluate(wb, sheet, at, &formula_at(f, first, at));
    let num = |f: &str| eval(f).to_number().ok();
    let ok = match dv.kind {
        ValidationKind::List => list_items(wb, sheet, dv).iter().any(|x| x.eq_ignore_ascii_case(input)),
        ValidationKind::Custom => eval(&dv.f1).to_bool().unwrap_or(false),
        ValidationKind::TextLength => {
            let n = input.chars().count() as f64;
            compare_op(dv.op, n, num(&dv.f1), dv.f2.as_deref().and_then(num))
        }
        _ => match v.as_f64() {
            Some(n) => {
                if dv.kind == ValidationKind::Whole && n.fract() != 0.0 {
                    false
                } else {
                    compare_op(dv.op, n, num(&dv.f1), dv.f2.as_deref().and_then(num))
                }
            }
            None => false,
        },
    };
    if ok {
        return None;
    }
    let msg = if dv.error_message.is_empty() {
        "This value doesn't match the data validation restrictions defined for this cell.".to_string()
    } else {
        dv.error_message.clone()
    };
    Some((dv.clone(), msg))
}

/// A formula written for `first`, as seen from `at` (relative references shifted).
fn formula_at(f: &str, first: CellRef, at: CellRef) -> String {
    if first == at {
        return f.to_string();
    }
    match gridcraft_formula::parse(f.trim_start_matches('=')) {
        Ok(e) => gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(
            e,
            at.row as i64 - first.row as i64,
            at.col as i64 - first.col as i64,
        )),
        Err(_) => f.to_string(),
    }
}

/// Moves the relative references of a rule formula written for `first` so it reads the same cells from `to`
/// (pasting a rule); text without relative references (constants, literal lists) stays as typed.
pub(super) fn move_rule_formula(f: &str, first: CellRef, to: CellRef) -> String {
    if first == to {
        return f.to_string();
    }
    let moved = formula_at(f, first, to);
    let has_relative = gridcraft_formula::parse(f.trim_start_matches('=')).is_ok_and(|e| gridcraft_formula::print(&e) != moved);
    let eq = if f.starts_with('=') { "=" } else { "" };
    if has_relative { format!("{eq}{moved}") } else { f.to_string() }
}

fn compare_op(op: CfOperator, n: f64, a: Option<f64>, b: Option<f64>) -> bool {
    let Some(a) = a else { return false };
    match op {
        CfOperator::Between => b.is_some_and(|b| n >= a.min(b) && n <= a.max(b)),
        CfOperator::NotBetween => b.is_some_and(|b| n < a.min(b) || n > a.max(b)),
        CfOperator::Equal => n == a,
        CfOperator::NotEqual => n != a,
        CfOperator::Greater => n > a,
        CfOperator::Less => n < a,
        CfOperator::GreaterOrEqual => n >= a,
        CfOperator::LessOrEqual => n <= a,
    }
}

fn validate_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let input = str_param(p, "input").unwrap_or("");
    match check_validation(&d.wb, d.wb.active_sheet, at, input) {
        None => Ok(json!({"ok": true})),
        Some((dv, msg)) => Ok(json!({"ok": false, "message": msg, "title": dv.error_title, "style": format!("{:?}", dv.error_style)})),
    }
}

fn circle_invalid(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sheet = d.wb.active_sheet;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let mut bad_cells = Vec::new();
    for dv in &sh.validations {
        for r in &dv.ranges {
            let r = sh.used_range().and_then(|u| r.intersection(&u));
            let Some(r) = r else { continue };
            for c in r.iter().take(100_000) {
                let text = sh.input_text(c);
                if !text.is_empty() && check_validation(&d.wb, sheet, c, &text).is_some() {
                    bad_cells.push(c.a1());
                }
            }
        }
    }
    Ok(json!({"invalid": bad_cells}))
}

// ---------------------------------------------------------------- outline

fn outline_lines(s: &Session, p: &Json) -> Result<(bool, u32, u32)> {
    if let Some(r) = str_param(p, "rows").and_then(parse_lines) {
        return Ok((true, r.start.row, r.end.row));
    }
    if let Some(r) = str_param(p, "cols").and_then(parse_lines) {
        return Ok((false, r.start.col, r.end.col));
    }
    let r = s.doc()?.selection.current();
    match str_param(p, "by") {
        Some("rows") => Ok((true, r.start.row, r.end.row)),
        Some("cols" | "columns") => Ok((false, r.start.col, r.end.col)),
        _ if r.is_full_cols() && !r.is_full_rows() => Ok((false, r.start.col, r.end.col)),
        _ => Ok((true, r.start.row, r.end.row)),
    }
}

fn group(s: &mut Session, p: &Json, on: bool) -> Result<Json> {
    let si = target_sheet(s, p)?;
    // The selection belongs to the active sheet, so another sheet needs its lines.
    if si != s.doc()?.wb.active_sheet && p.get("rows").is_none() && p.get("cols").is_none() {
        return Err(bad(if on { "data.group" } else { "data.ungroup" }, "missing `rows` or `cols`: the selection is on the active sheet"));
    }
    let (rows, a, b) = outline_lines(s, p)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let map = if rows { &mut sh.rows } else { &mut sh.cols };
        let b = b.min(a.saturating_add(1_048_575));
        for i in a..=b {
            let e = map.entry(i).or_default();
            e.outline = if on { (e.outline + 1).min(outline::MAX_LEVEL) } else { e.outline.saturating_sub(1) };
            if !on && e.outline == 0 {
                e.hidden = false;
            }
        }
        // A summary line whose group is gone has nothing left to collapse.
        if !on
            && !outline::groups(map).iter().any(|g| g.end == b)
            && let Some(e) = map.get_mut(&b.saturating_add(1))
        {
            e.collapsed = false;
        }
        cx.structural = true; // ungrouping reveals hidden rows: SUBTOTAL(10x) depends on it
        Ok(Json::Null)
    })
}

/// Hide Detail / Show Detail: the innermost group (or the one at `level`) that holds the lines, or
/// failing that the one they are the summary of. Hiding marks the summary line collapsed (the
/// + button, saved to XLSX); showing keeps inner groups that were collapsed on their own collapsed.
fn detail(s: &mut Session, p: &Json, hide: bool) -> Result<Json> {
    let (rows, a, b) = outline_lines(s, p)?;
    let want = p.get("level").and_then(Json::as_u64).map(|l| l.min(outline::MAX_LEVEL as u64) as u8);
    edit(s, |cx| {
        let si = cx.sheet_index();
        let sh = cx.sheet_mut(si)?;
        let map = if rows { &mut sh.rows } else { &mut sh.cols };
        let all = outline::groups(map);
        let level_ok = |g: &outline::OutlineGroup| want.is_none_or(|l| g.level == l);
        let pick = |hit: &dyn Fn(&outline::OutlineGroup) -> bool| -> Vec<outline::OutlineGroup> {
            let hits: Vec<_> = all.iter().filter(|g| level_ok(g) && hit(g)).copied().collect();
            let deepest = hits.iter().map(|g| g.level).max().unwrap_or(0);
            hits.into_iter().filter(|g| g.level == deepest).collect()
        };
        let mut chosen = pick(&|g| g.start <= b && g.end >= a);
        if chosen.is_empty() {
            chosen = pick(&|g| g.summary() >= a && g.summary() <= b);
        }
        for g in chosen {
            // Inner groups collapsed on their own (their summary marked), before this one changes.
            let inner: Vec<outline::OutlineGroup> = all
                .iter()
                .filter(|o| o.level > g.level && o.start >= g.start && o.end <= g.end && o.summary() != g.summary())
                .filter(|o| map.get(&o.summary()).is_some_and(|e| e.collapsed))
                .copied()
                .collect();
            for i in g.start..=g.end {
                if let Some(e) = map.get_mut(&i) {
                    set_hidden(e, hide || inner.iter().any(|o| o.contains(i)));
                }
            }
            if hide {
                map.entry(g.summary()).or_default().collapsed = true;
            } else if let Some(e) = map.get_mut(&g.summary()) {
                e.collapsed = false;
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Hides or shows an outline line. A line shown with a zero size (files that hide columns as
/// `width="0" hidden="1"`) gets the default size back, as Unhide does; otherwise + would mark the
/// group expanded and leave it invisible (#1714).
fn set_hidden(e: &mut LineInfo, hidden: bool) {
    e.hidden = hidden;
    if !hidden && e.size == Some(0.0) {
        e.size = None;
    }
}

/// The outline's level buttons: show rows / columns down to outline level `n` (1 = only the
/// top-level summaries); deeper detail is hidden and its summary lines marked collapsed.
fn show_levels(s: &mut Session, p: &Json) -> Result<Json> {
    let level = |k: &str| p.get(k).and_then(Json::as_u64).map(|n| n.clamp(1, outline::MAX_LEVEL as u64 + 1) as u8);
    let (rows, cols) = (level("rows"), level("cols"));
    if rows.is_none() && cols.is_none() {
        return Err(EngineError::BadParams {
            cmd: "data.showLevels".into(),
            msg: "give `rows` or `cols`: the outline level to show (1 = top-level summaries only)".into(),
        });
    }
    edit(s, |cx| {
        let si = cx.sheet_index();
        let sh = cx.sheet_mut(si)?;
        for (map, n) in [(&mut sh.rows, rows), (&mut sh.cols, cols)] {
            let Some(n) = n else { continue };
            let groups = outline::groups(map);
            for e in map.values_mut().filter(|e| e.outline > 0) {
                set_hidden(e, e.outline >= n);
            }
            // Inner groups come first, so a summary line shared with the group around it ends up
            // as that outer group says.
            for g in groups {
                map.entry(g.summary()).or_default().collapsed = g.level >= n;
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn subtotal(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.subtotal")?;
    let header = bool_param(p, "header").unwrap_or(true);
    let group_col = col_param(p.get("groupBy"), r.start.col).unwrap_or(r.start.col);
    let func = str_param(p, "function").unwrap_or("sum");
    let code = match func {
        "count" => 3,
        "average" => 1,
        "max" => 4,
        "min" => 5,
        "product" => 6,
        _ => 9,
    };
    let cols: Vec<u32> = p
        .get("columns")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|c| col_param(Some(c), r.start.col)).collect())
        .unwrap_or_else(|| vec![r.end.col]);
    let label = match func {
        "count" => "Count",
        "average" => "Average",
        "max" => "Max",
        "min" => "Min",
        "product" => "Product",
        _ => "Total",
    };
    // Group boundaries (header in the first row).
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let mut groups: Vec<(u32, u32, String)> = Vec::new();
    let first = r.start.row + u32::from(header);
    let mut start = first;
    for row in first..=r.end.row + 1 {
        let cur = sh.value(CellRef::new(row, group_col)).display();
        let prev = sh.value(CellRef::new(start, group_col)).display();
        if row > r.end.row || cur != prev {
            groups.push((start, row - 1, prev));
            start = row;
        }
    }
    // Insert a total row after each group, bottom-up so indices stay valid.
    let mut inserted = 0;
    for (g0, g1, name) in groups.iter().rev() {
        s.execute("home.insertRows", json!({"sheet": sheet, "rows": format!("{}:{}", g1 + 2, g1 + 2)}))?;
        let row = g1 + 1;
        let lbl = CellRef::new(row, group_col).a1();
        s.execute("cell.set", json!({"sheet": sheet, "cell": lbl, "input": format!("{name} {label}")}))?;
        for c in &cols {
            let range = RangeRef::new(CellRef::new(*g0, *c), CellRef::new(*g1, *c)).a1();
            s.execute("cell.set", json!({"sheet": sheet, "cell": CellRef::new(row, *c).a1(), "input": format!("=SUBTOTAL({code},{range})")}))?;
        }
        s.execute(
            "home.bold",
            json!({"sheet": sheet, "range": RangeRef::new(CellRef::new(row, r.start.col), CellRef::new(row, r.end.col)).a1(), "on": true}),
        )?;
        s.execute("data.group", json!({"sheet": sheet, "rows": format!("{}:{}", g0 + 1, g1 + 1)}))?;
        inserted += 1;
    }
    let last = r.end.row + inserted + 1;
    let lbl = CellRef::new(last, group_col).a1();
    s.execute("home.insertRows", json!({"sheet": sheet, "rows": format!("{}:{}", last + 1, last + 1)}))?;
    s.execute("cell.set", json!({"sheet": sheet, "cell": lbl, "input": format!("Grand {label}")}))?;
    for c in &cols {
        let range = RangeRef::new(CellRef::new(first, *c), CellRef::new(last - 1, *c)).a1();
        s.execute("cell.set", json!({"sheet": sheet, "cell": CellRef::new(last, *c).a1(), "input": format!("=SUBTOTAL({code},{range})")}))?;
    }
    Ok(json!({"groups": inserted}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_blanks_include_empty_text_and_formula_results() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["id", "f"], ["e", null], ["ef", "=\"\""], ["sp", " "], ["z", 0]]})).unwrap();
        s.execute("data.filter", json!({"range": "A1:B5"})).unwrap();
        let r = s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
        assert_eq!(r["hiddenRows"], json!(2));
        let vals = filter_values(&s.doc().unwrap().wb, 0, 1);
        // The `=""` result joins (Blanks) instead of showing as its own empty entry.
        assert!(!vals.iter().any(|(t, _)| t.is_empty()), "{vals:?}");
        let blanks: Vec<_> = vals.into_iter().filter(|(t, _)| t == "(Blanks)").collect();
        assert_eq!(blanks, vec![("(Blanks)".to_string(), true)]);
    }

    #[test]
    fn subtotal_honours_header_false() {
        // #131: with header:false the first data row was treated as a header and left out of its
        // group's subtotal and the grand total.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["East", 10], ["East", 20], ["West", 30], ["West", 40]]})).unwrap();
        s.execute("data.subtotal", json!({"range": "A1:B4", "groupBy": "A", "function": "sum", "columns": ["B"], "header": false})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], json!("East"));
        assert_eq!(get(&mut s, "A3")["text"], json!("East Total"));
        assert_eq!(get(&mut s, "B3")["value"], json!(30.0), "East's subtotal counts both East rows");
        assert_eq!(get(&mut s, "A7")["text"], json!("Grand Total"));
        assert_eq!(get(&mut s, "B7")["value"], json!(100.0), "the grand total counts every data row");
    }

    #[test]
    fn custom_sort_context_sorts_a_selected_block_even_inside_a_table() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["H"], [2], [1]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A3", "header": true, "name": "T"})).unwrap();
        s.execute("table.totalRow", json!({"table": "T", "on": true})).unwrap();
        // One cell in the table: the table's data, without its totals row.
        s.execute("selection.set", json!({"cell": "A2"})).unwrap();
        assert_eq!(sort_context(&s, &json!({})).unwrap().0.a1(), "A1:A3");
        // A selected block that includes the totals row is sorted as selected.
        s.execute("selection.set", json!({"range": "A1:A4"})).unwrap();
        assert_eq!(sort_context(&s, &json!({})).unwrap().0.a1(), "A1:A4");
        // Other data commands keep the table's range.
        assert_eq!(data_range(&s, &json!({})).unwrap().a1(), "A1:A3");
        // An explicit range wins over the selection.
        assert_eq!(sort_context(&s, &json!({"range": "A1:A2"})).unwrap().0.a1(), "A1:A2");
        // A block starting below the header row has no header.
        s.execute("selection.set", json!({"range": "A2:A4"})).unwrap();
        assert!(!sort_context(&s, &json!({})).unwrap().1);
        // The data.sort dialog request uses the selected block.
        s.execute("selection.set", json!({"range": "A1:A4"})).unwrap();
        s.ui_requests.clear();
        s.execute("data.sort", json!({})).unwrap();
        assert!(matches!(s.ui_requests.last(), Some(crate::UiRequest::Dialog(n, v)) if n == "sort" && v["range"] == "A1:A4"));
    }

    #[test]
    fn filter_by_huge_numeric_column_is_rejected_not_overflowed() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "B1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
        s.execute("data.filter", json!({"range": "B1:C3"})).unwrap();
        for col in [json!(2), json!(u64::from(u32::MAX)), json!(u64::MAX)] {
            let err = s.execute("data.filterBy", json!({"column": col, "values": ["a"]})).unwrap_err();
            assert!(err.to_string().contains("column outside the filter range"), "{col}: {err}");
        }
        let ok = s.execute("data.filterBy", json!({"column": 0, "values": ["a"]})).unwrap();
        assert_eq!(ok["hiddenRows"], json!(1));
    }

    #[test]
    fn filter_by_inside_a_table_installs_the_autofilter_and_keeps_the_button() {
        for sel in [json!({"cell": "A1"}), json!({"cell": "A2"}), json!({"range": "A1:A3"})] {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"]]})).unwrap();
            s.execute("insert.table", json!({"range": "A1:A3", "header": true})).unwrap();
            s.execute("selection.set", sel.clone()).unwrap();
            s.execute("data.filterBy", json!({"column": "A", "values": ["a"]})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            assert_eq!(sh.autofilter.as_ref().map(|a| a.range.a1()), Some("A1:A3".to_string()), "{sel}");
            assert!(sh.tables[0].filter_button, "{sel}");
            assert!(!sh.is_row_hidden(1), "{sel}");
            assert!(sh.is_row_hidden(2), "{sel}");
            assert_eq!(sh.value(CellRef::new(2, 0)).display(), "b", "{sel}");
        }
    }

    #[test]
    fn autofilter_range_grows_when_a_value_is_entered_in_the_row_below() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["d", 4], ["b", 2]]})).unwrap();
        s.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
        let range = |s: &Session| s.doc().unwrap().wb.active().unwrap().autofilter.as_ref().map(|a| a.range.a1());
        s.execute("cell.set", json!({"cell": "A4", "input": "e"})).unwrap();
        assert_eq!(range(&s), Some("A1:B4".to_string()));
        s.execute("range.setValues", json!({"range": "A5", "values": [["f", 6]]})).unwrap();
        assert_eq!(range(&s), Some("A1:B5".to_string()));
        // A row typed after a blank row does not grow it.
        s.execute("cell.set", json!({"cell": "A7", "input": "x"})).unwrap();
        assert_eq!(range(&s), Some("A1:B5".to_string()));
        // A cell outside the filter's columns does not either.
        s.execute("cell.set", json!({"cell": "C6", "input": "x"})).unwrap();
        assert_eq!(range(&s), Some("A1:B5".to_string()));
        // Empty text is a blank entry, not a value.
        s.execute("range.setValues", json!({"range": "A6", "parse": false, "values": [[""]]})).unwrap();
        assert_eq!(range(&s), Some("A1:B5".to_string()));
        // A formula and Ctrl+Enter (range.fill) are entries too.
        s.execute("cell.set", json!({"cell": "B6", "input": "=1+1"})).unwrap();
        assert_eq!(range(&s), Some("A1:B6".to_string()));
        s.execute("range.fill", json!({"range": "A7:B7", "input": "g"})).unwrap();
        assert_eq!(range(&s), Some("A1:B7".to_string()));
    }

    #[test]
    fn list_validation_without_formula1_is_an_error() {
        let mut s = Session::new();
        s.new_workbook();
        assert!(s.execute("data.validation", json!({"range": "A1:A3", "type": "list", "source": "\"a,b,c\""})).is_err());
        assert!(s.execute("data.validation", json!({"range": "A1:A3", "type": "list"})).is_err());
        assert!(s.doc().unwrap().wb.active().unwrap().validations.is_empty());
        s.execute("data.validation", json!({"range": "A1:A3", "type": "list", "formula1": "\"a,b,c\""})).unwrap();
        assert_eq!(s.execute("data.validate", json!({"cell": "A1", "input": "a"})).unwrap()["ok"], json!(true));
    }

    #[test]
    fn partial_validation_edits_keep_the_rest_of_the_old_rule() {
        let mut s = Session::new();
        s.new_workbook();
        let rule = |range: &str, lo: &str, hi: &str| json!({"range": range, "type": "whole", "operator": "between", "formula1": lo, "formula2": hi});
        let ok = |s: &mut Session, cell: &str, input: &str| {
            s.execute("data.validate", json!({"cell": cell, "input": input})).unwrap()["ok"] == json!(true)
        };
        s.execute("data.validation", rule("B1:B3", "1", "10")).unwrap();
        s.execute("data.validation", json!({"range": "B2", "clear": true})).unwrap();
        assert!(ok(&mut s, "B2", "999"));
        assert!(!ok(&mut s, "B1", "999"));
        assert!(!ok(&mut s, "B3", "999"));
        s.execute("data.validation", rule("B2:B4", "100", "200")).unwrap();
        for cell in ["B2", "B3", "B4"] {
            assert!(ok(&mut s, cell, "150"), "{cell}");
            assert!(!ok(&mut s, cell, "5"), "{cell}");
        }
        assert!(ok(&mut s, "B1", "5"));
        assert!(!ok(&mut s, "B1", "150"));
    }

    #[test]
    fn showing_detail_gives_zero_width_columns_their_width_back() {
        // #1714: templates hide grouped columns as `width="0" hidden="1"`; + must show them.
        let mut s = Session::new();
        s.new_workbook();
        let hidden = LineInfo { size: Some(0.0), hidden: true, outline: 1, ..LineInfo::default() };
        let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        for c in 2..=4 {
            sh.cols.insert(c, hidden);
        }
        sh.cols.insert(5, LineInfo { collapsed: true, ..LineInfo::default() });
        s.execute("data.showDetail", json!({"cols": "C:E", "level": 1})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        for c in 2..=4 {
            assert_eq!(sh.col_width(c), sh.default_col_width, "column {c}");
        }
        // Hiding again hides them; the level buttons show zero-width columns too.
        s.execute("data.hideDetail", json!({"cols": "C:E", "level": 1})).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().col_width(2), 0.0);
        Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().cols.insert(3, hidden);
        s.execute("data.showLevels", json!({"cols": 2})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!((sh.col_width(2), sh.col_width(3)), (sh.default_col_width, sh.default_col_width));
    }

    #[test]
    fn removing_an_autofilter_keeps_manually_hidden_and_collapsed_rows() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"], ["c"]]})).unwrap();
        s.execute("range.setValues", json!({"range": "A10", "values": [["z"]]})).unwrap();
        s.execute("data.group", json!({"rows": "2:4"})).unwrap();
        s.execute("data.hideDetail", json!({"rows": "2:4"})).unwrap();
        s.execute("home.hideRows", json!({"rows": "10:10"})).unwrap();
        let hidden = |s: &Session| -> Vec<u32> {
            let sh = s.doc().unwrap().wb.active().unwrap();
            sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect()
        };
        let before = hidden(&s);
        assert_eq!(before, [1, 2, 3, 9]);
        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
        assert!(s.doc().unwrap().wb.active().unwrap().autofilter.is_none());
        assert_eq!(hidden(&s), before);
        assert_eq!(row_state(&s, 4..=4), [(false, true)]);
    }

    #[test]
    fn table_dropdown_setup_outside_the_table_keeps_manually_hidden_rows() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"], ["c"]]})).unwrap();
        s.execute("range.setValues", json!({"range": "A10", "values": [["z"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A4", "header": true})).unwrap();
        s.execute("home.hideRows", json!({"rows": "3:3"})).unwrap();
        s.execute("home.hideRows", json!({"rows": "10:10"})).unwrap();
        s.execute("selection.set", json!({"range": "E1"})).unwrap();
        // The table filter dropdown installs an AutoFilter on demand by toggling `data.filter` twice.
        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect::<Vec<_>>(), [2, 9]);
    }

    /// Rows 5:8 grouped, with 6:7 grouped again inside (0-based 4..=7 and 5..=6).
    fn nested_rows() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("data.group", json!({"rows": "5:8"})).unwrap();
        s.execute("data.group", json!({"rows": "6:7"})).unwrap();
        s
    }

    /// (hidden, collapsed) of 0-based rows.
    fn row_state(s: &Session, rows: std::ops::RangeInclusive<u32>) -> Vec<(bool, bool)> {
        let sh = s.doc().unwrap().wb.active().unwrap();
        rows.map(|r| sh.rows.get(&r).map(|l| (l.hidden, l.collapsed)).unwrap_or_default()).collect()
    }

    #[test]
    fn hide_detail_marks_the_summary_collapsed_and_show_detail_keeps_inner_groups_collapsed() {
        let mut s = nested_rows();
        s.execute("data.hideDetail", json!({"rows": "6:7", "level": 2})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
        s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, true), (false, true)]);
        s.execute("data.showDetail", json!({"rows": "5:8", "level": 1})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
        s.execute("data.showDetail", json!({"rows": "6:7", "level": 2})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(false, false); 5]);
        // Each step is one undoable command.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(row_state(&s, 5..=7), [(true, false), (true, false), (false, true)]);
    }

    #[test]
    fn hide_detail_on_the_summary_row_collapses_the_group_above_it() {
        let mut s = nested_rows();
        s.execute("data.hideDetail", json!({"rows": "9:9"})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, false), (false, true)]);
    }

    #[test]
    fn show_levels_collapses_and_expands_to_a_level() {
        let mut s = nested_rows();
        s.execute("data.showLevels", json!({"rows": 1})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, true), (false, true)]);
        s.execute("data.showLevels", json!({"rows": 2})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
        s.execute("data.showLevels", json!({"rows": 3})).unwrap();
        assert_eq!(row_state(&s, 4..=8), [(false, false); 5]);
        // Columns are separate; no level at all is an error rather than a guess.
        s.execute("data.group", json!({"cols": "C:E"})).unwrap();
        s.execute("data.showLevels", json!({"cols": 1})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert!((2..=4).all(|c| sh.cols.get(&c).is_some_and(|l| l.hidden)));
        assert!(sh.cols.get(&5).is_some_and(|l| l.collapsed));
        assert_eq!(row_state(&s, 4..=4), [(false, false)]);
        assert!(s.execute("data.showLevels", json!({})).is_err());
    }

    #[test]
    fn group_by_columns_for_a_block_of_cells() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
        s.execute("data.group", json!({"by": "cols"})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert!((1..=2).all(|c| sh.cols.get(&c).is_some_and(|l| l.outline == 1)));
        assert!(sh.rows.values().all(|l| l.outline == 0));
    }

    #[test]
    fn group_shortcuts_are_the_windows_ones() {
        let sc = |id: &str| specs().into_iter().find(|c| c.id == id).and_then(|c| c.shortcut);
        assert_eq!(sc("data.group"), Some("Alt+Shift+Right"));
        assert_eq!(sc("data.ungroup"), Some("Alt+Shift+Left"));
    }

    #[test]
    fn column_sort_rejects_row_key_zero_without_panicking() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
        for row in [0u64, 1 << 32] {
            let params = json!({"range": "A1:B2", "header": false, "orientation": "columns", "keys": [{"row": row}]});
            let err = s.execute("data.sort", params).unwrap_err();
            assert!(matches!(err, EngineError::BadParams { .. }), "row {row}: {err:?}");
        }
    }

    #[test]
    fn single_row_and_column_parameters() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
        s.execute("home.insertRows", json!({"rows": "2"})).unwrap();
        s.execute("home.insertColumns", json!({"cols": "B"})).unwrap();
        let v = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(v(&mut s, "C3"), json!(4.0));
        s.execute("home.hideRows", json!({"rows": "1"})).unwrap();
        s.execute("data.group", json!({"cols": "C"})).unwrap();
        assert!(s.execute("home.deleteRows", json!({"rows": "0"})).is_err());
        s.execute("home.deleteRows", json!({"rows": "2"})).unwrap();
        s.execute("home.deleteColumns", json!({"cols": "B"})).unwrap();
        assert_eq!(v(&mut s, "B2"), json!(4.0));
    }

    #[test]
    fn t2c_fixed_widths_are_positions_in_any_order_without_duplicates() {
        let cases: [(Json, [&str; 4]); 5] = [
            (json!([6, 3]), ["abc", "def", "ghij", ""]),
            (json!([3, 3]), ["abc", "defghij", "", ""]),
            (json!([0]), ["abcdefghij", "", "", ""]),
            (json!([20]), ["abcdefghij", "", "", ""]),
            (json!([7, 0, 3, 7, 10]), ["abc", "defg", "hij", ""]),
        ];
        for (widths, want) in cases {
            let mut s = Session::new();
            s.new_workbook();
            s.execute("cell.set", json!({"cell": "A1", "input": "abcdefghij"})).unwrap();
            s.execute("data.textToColumns", json!({"range": "A1:A1", "fixedWidths": widths})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            let got: Vec<String> = (0..4).map(|c| sh.value(CellRef::new(0, c)).display()).collect();
            assert_eq!(got, want, "{widths}");
        }
    }

    #[test]
    fn split() {
        assert_eq!(split_delimited("a,b,,c", &[','], false, '"'), ["a", "b", "", "c"]);
        assert_eq!(split_delimited("a,b,,c", &[','], true, '"'), ["a", "b", "c"]);
        assert_eq!(split_delimited("\"x,y\",z", &[','], false, '"'), ["x,y", "z"]);
    }

    #[test]
    fn remove_duplicates_without_a_valid_column_is_refused() {
        // `columns: []` (or only unusable names) used to compare nothing and delete every row
        // but the first.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["a"], ["b"], ["c"]]})).unwrap();
        for columns in [json!([]), json!(["ZZZ"])] {
            let err = s.execute("data.removeDuplicates", json!({"range": "A1:A3", "columns": columns})).unwrap_err();
            assert!(err.to_string().contains("columns"), "{err}");
        }
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("a"));
        assert_eq!(sh.value(CellRef::new(2, 0)), Value::text("c"));
    }

    #[test]
    fn a_list_typed_without_quotes_is_saved_as_a_quoted_list() {
        // In a file an inline list is a string literal (`"a,b"`); unquoted it reads as a formula.
        // References, names and formulas stay unquoted, and every rule survives a save and reopen.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "K1:K3", "values": [["Red"], ["Green"], ["Blue"]]})).unwrap();
        s.execute("formulas.defineName", json!({"name": "Colors", "refersTo": "=Sheet1!$K$1:$K$3"})).unwrap();
        let rules = [
            ("A1", "Open,In progress,Closed", r#""Open,In progress,Closed""#),
            ("B1", r#""Open,In progress,Closed""#, r#""Open,In progress,Closed""#),
            ("C1", " 1,2,3 ", r#""1,2,3""#),
            ("D1", r#"Say "hi",Bye"#, r#""Say ""hi"",Bye""#),
            ("E1", "=$K$1:$K$3", "$K$1:$K$3"),
            ("F1", "$K$1:$K$3", "$K$1:$K$3"),
            ("G1", "=Colors", "Colors"),
            ("H1", "Sheet1!$K$1:$K$3", "Sheet1!$K$1:$K$3"),
            ("I1", "=OFFSET($K$1,0,0,3,1)", "OFFSET($K$1,0,0,3,1)"),
        ];
        for (cell, source, _) in rules {
            s.execute("data.validation", json!({"range": cell, "type": "list", "formula1": source})).unwrap();
        }
        let accepted = [
            ("A1", "In progress"),
            ("B1", "Closed"),
            ("C1", "2"),
            ("D1", r#"Say "hi""#),
            ("E1", "Green"),
            ("F1", "Blue"),
            ("G1", "Red"),
            ("H1", "Green"),
        ];
        let check = |wb: &Workbook| {
            for (cell, input) in accepted {
                let at = CellRef::parse(cell).unwrap();
                assert!(check_validation(wb, 0, at, input).is_none(), "{cell} refuses {input}");
                assert!(check_validation(wb, 0, at, "Pending").is_some(), "{cell} accepts Pending");
            }
        };
        let wb = s.doc().unwrap().wb.clone();
        check(&wb);

        let sheet_xml = |wb: &Workbook| {
            let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut xml).unwrap();
            xml
        };
        let xml = sheet_xml(&wb);
        for (cell, _, saved) in rules {
            assert!(xml.contains(&format!(r#"sqref="{cell}"><formula1>{saved}</formula1>"#)), "{cell}: {saved} missing from {xml}");
        }
        let (reopened, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
        check(&reopened);
        assert_eq!(sheet_xml(&reopened), xml, "a second save writes the same rules");
    }

    #[test]
    fn a_rule_with_an_unknown_type_operator_or_error_style_is_refused() {
        // A typo used to give a rule that validates nothing (`type="none"`) or the wrong check,
        // with no error.
        let mut s = Session::new();
        s.new_workbook();
        for params in [
            json!({"range": "A1", "type": "lst", "formula1": "\"a,b\""}),
            json!({"range": "A1", "type": "whole", "operator": "greaterthan", "formula1": "1"}),
            json!({"range": "A1", "type": "whole", "operator": ">", "formula1": "1"}),
            json!({"range": "A1", "type": "list", "formula1": "\"a,b\"", "errorStyle": "warn"}),
        ] {
            let e = s.execute("data.validation", params.clone()).unwrap_err().to_string();
            assert!(e.contains("unknown"), "{params}: {e}");
        }
        assert!(s.doc().unwrap().wb.active().unwrap().validations.is_empty());
        for params in [
            json!({"range": "A1", "type": "any"}),
            json!({"range": "A2", "type": "whole", "operator": "greaterThan", "formula1": "1"}),
            json!({"range": "A3", "type": "list", "formula1": "\"a,b\"", "errorStyle": "warning"}),
        ] {
            s.execute("data.validation", params).unwrap();
        }
        let sh = s.doc().unwrap().wb.active().unwrap().clone();
        assert_eq!(sh.validations.len(), 3);
        assert!(check_validation(&s.doc().unwrap().wb, 0, CellRef::parse("A2").unwrap(), "0").is_some());
    }
}
