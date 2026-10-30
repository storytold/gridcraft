use super::cell::input_to_cell;
use super::paste_special::paste_special;
use super::{
    Cell, Clipboard, EngineError, Json, RangeRef, Result, Session, Value, cell_param_on, edit, json, ok, str_param, target_ranges, target_sheet,
};

// ---------------------------------------------------------------- clipboard

fn copy_impl(s: &mut Session, p: &Json, cut: bool) -> Result<Json> {
    let multiple = || EngineError::Other("This action won't work on multiple selections.".into());
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if ranges.len() > 1 && cut {
        return Err(multiple());
    }
    // The selection as made (untrimmed), for Insert Copied Cells.
    let selected = ranges.first().copied().unwrap_or_default();
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let trim = |range: RangeRef| {
        if range.is_full_cols() || range.is_full_rows() {
            let used = sh.used_range().unwrap_or(RangeRef::cell(range.start));
            range.intersection(&used.union(&RangeRef::cell(range.start))).unwrap_or(range)
        } else {
            range
        }
    };
    let (wb, range, areas) = if ranges.len() > 1 {
        let areas: Vec<RangeRef> = ranges.iter().map(|r| trim(*r)).collect();
        let (wb, block) = slide_together(&d.wb, sheet, &areas).ok_or_else(multiple)?;
        (std::sync::Arc::new(wb), block, areas)
    } else {
        (d.wb.clone(), trim(ranges.first().copied().unwrap_or_default()), Vec::new())
    };
    let text = crate::display::range_text(&wb, sheet, range);
    // The HTML rendering is for the host clipboard (UI); it can reach 4 MB, so agents and
    // scripts get it only when they ask for it.
    let html = if p.get("html").and_then(Json::as_bool) == Some(true) { crate::io::range_html(&wb, sheet, range, &text) } else { None };
    let clip = Clipboard { wb, sheet, range, cut, doc_uid: d.uid, text: text.clone(), areas, selected, trust: d.trust };
    s.clipboard = Some(clip);
    let mut result = json!({"range": range.a1(), "text": text});
    if let Some(html) = html {
        result["html"] = Json::String(html);
    }
    Ok(result)
}

/// Excel copies a multi-area selection only when the areas line up: all cover the same rows (they
/// slide together side by side, left to right) or all cover the same columns (stacked, top to
/// bottom), without overlapping. Returns a copy of the workbook whose sheet holds the areas packed
/// into one block at the first area's place, and that block. Formulas travel as their values, as
/// in Excel. `None` when the areas don't line up.
fn slide_together(wb: &gridcraft_model::Workbook, sheet: usize, areas: &[RangeRef]) -> Option<(gridcraft_model::Workbook, RangeRef)> {
    let first = *areas.first()?;
    let across = areas.iter().all(|a| a.start.row == first.start.row && a.end.row == first.end.row);
    let down = areas.iter().all(|a| a.start.col == first.start.col && a.end.col == first.end.col);
    let mut sorted = areas.to_vec();
    if across {
        sorted.sort_by_key(|a| a.start.col);
    } else if down {
        sorted.sort_by_key(|a| a.start.row);
    } else {
        return None;
    }
    if sorted.windows(2).any(|w| matches!(w, [a, b] if a.intersects(b))) {
        return None;
    }
    let anchor = sorted.first()?.start;
    let (h, w) = if across {
        (first.height(), sorted.iter().map(RangeRef::width).fold(0u32, u32::saturating_add))
    } else {
        (sorted.iter().map(RangeRef::height).fold(0u32, u32::saturating_add), first.width())
    };
    let block = RangeRef::new(anchor, anchor.offset(i64::from(h) - 1, i64::from(w) - 1)?);
    let src = wb.sheet(sheet)?;
    let mut out = wb.clone();
    let dst = out.sheet_mut(sheet)?;
    dst.take_cells(block);
    dst.spill.retain(|c, _| !block.contains(*c));
    dst.spill_ranges.retain(|c, _| !block.contains(*c));
    dst.comments.retain(|c, _| !block.contains(*c));
    dst.merges.retain(|m| !m.intersects(&block));
    if across {
        dst.cols.retain(|c, _| !(block.start.col..=block.end.col).contains(c));
    } else {
        dst.rows.retain(|r, _| !(block.start.row..=block.end.row).contains(r));
    }
    let mut next = if across { anchor.col } else { anchor.row };
    for a in &sorted {
        let (dr, dc) = if across { (0, i64::from(next) - i64::from(a.start.col)) } else { (i64::from(next) - i64::from(a.start.row), 0) };
        let cells = src.cells.iter_range(*a).map(|(c, _)| c).chain(src.spill.keys().copied().filter(|c| a.contains(*c)));
        for c in cells {
            let Some(to) = c.offset(dr, dc) else { continue };
            let style = src.cell(c).map(|x| x.style).unwrap_or_default();
            dst.set_cell(to, Cell { value: src.value(c), formula: None, style });
            if let Some(picture) = src.cell_pictures.get(&c) {
                dst.cell_pictures.insert(to, picture.clone());
            }
        }
        for (c, comment) in src.comments.iter().filter(|(c, _)| a.contains(**c)) {
            if let Some(to) = c.offset(dr, dc) {
                dst.comments.insert(to, comment.clone());
            }
        }
        for m in src.merges.iter().filter(|m| a.contains_range(m)) {
            if let (Some(s), Some(e)) = (m.start.offset(dr, dc), m.end.offset(dr, dc)) {
                dst.merges.push(RangeRef::new(s, e));
            }
        }
        // Column widths (side by side) or row heights and hidden rows (stacked) follow their cells.
        if across {
            for (c, info) in src.cols.range(a.start.col..=a.end.col) {
                dst.cols.insert(next.saturating_add(c.saturating_sub(a.start.col)), *info);
            }
        } else {
            for (r, info) in src.rows.range(a.start.row..=a.end.row) {
                dst.rows.insert(next.saturating_add(r.saturating_sub(a.start.row)), *info);
            }
        }
        next = next.saturating_add(if across { a.width() } else { a.height() });
    }
    Some((out, block))
}

pub(super) fn copy(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, false)
}

pub(super) fn cut(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, true)
}

pub(super) fn clear_clipboard(s: &mut Session, _: &Json) -> Result<Json> {
    s.clipboard = None;
    s.format_painter = None;
    ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum What {
    All,
    Formulas,
    Values,
    Formats,
    Comments,
    Validation,
    AllExceptBorders,
    ColumnWidths,
    FormulasAndNumberFormats,
    ValuesAndNumberFormats,
}

pub(super) fn paste(s: &mut Session, p: &Json) -> Result<Json> {
    // External text (from the system clipboard) when it differs from ours.
    if let Some(text) = str_param(p, "text")
        && s.clipboard.as_ref().is_none_or(|c| c.text.trim_end_matches(['\r', '\n']) != text.trim_end_matches(['\r', '\n']))
    {
        return paste_text(s, p, text);
    }
    if s.clipboard.is_none() {
        return Err(EngineError::Other("Nothing to paste.".into()));
    }
    paste_special(s, &json!({"what": "all", "at": p.get("at").cloned().unwrap_or(Json::Null)}))
}

fn paste_text(s: &mut Session, p: &Json, text: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "at", sheet)?.unwrap_or(s.doc()?.selection.current().start);
    let rows: Vec<Vec<String>> = parse_tsv(text);
    let h = rows.len();
    let w = rows.iter().map(Vec::len).max().unwrap_or(0);
    // The HTML flavour, when the host passed it: the number behind each cell's shown text. Read
    // only as far as the text's own shape, so spans in the markup can't expand without bound.
    let html = str_param(p, "html").map(|html| crate::clip_html::table_cells(html, h, w)).unwrap_or_default();
    edit(s, |cx| {
        for (ri, row) in rows.iter().enumerate() {
            for (ci, v) in row.iter().enumerate() {
                let Some(c) = at.offset(ri as i64, ci as i64) else { continue };
                let mut cell = input_to_cell(v, sheet, c, &mut cx.wb).unwrap_or_else(|_| Some(Cell::value(Value::text(v.as_str()))));
                if let Some(cl) = cell.as_mut() {
                    numeric_from_html(cl, v, html.get(ri).and_then(|r| r.get(ci)), &mut cx.wb);
                }
                let sh = cx.sheet_mut(sheet)?;
                match cell {
                    Some(cell) => sh.set_cell(c, cell),
                    None => {
                        sh.remove_cell(c);
                    }
                }
                cx.touch(sheet, c);
            }
        }
        if h > 0 && w > 0 {
            let end = at.offset_clamped(h as i64 - 1, w as i64 - 1);
            *cx.sel = crate::Selection { active: at, anchor: at, ranges: vec![RangeRef::new(at, end)] };
        }
        Ok(())
    })?;
    Ok(json!({"rows": h, "cols": w}))
}

/// A pasted cell that came out as text ("Rp10,000.00" isn't en-US input) becomes the number the
/// source recorded for it in its HTML, with a format that shows the same text when one can be
/// inferred and the cell has none. The HTML cell must show the same text as the tab-separated
/// field, so HTML that belongs to another copy (or a table laid out differently) changes nothing.
fn numeric_from_html(cell: &mut Cell, field: &str, html: Option<&crate::clip_html::HtmlCell>, wb: &mut gridcraft_model::Workbook) {
    let Some(hc) = html else { return };
    let Some(n) = hc.num else { return };
    if cell.formula.is_some() || !matches!(cell.value, Value::Text(_)) || wb.styles.get(cell.style).num_fmt.as_str() == "@" {
        return;
    }
    if crate::clip_html::normalized(&hc.text) != crate::clip_html::normalized(field) {
        return;
    }
    cell.value = Value::number(n);
    if wb.styles.get(cell.style).num_fmt.as_str() == "General"
        && let Some(code) = crate::clip_html::format_for(field, n)
    {
        cell.style = wb.styles.derive(cell.style, |s| s.num_fmt = gridcraft_model::NumFmt::new(&code));
    }
}

/// Tab-separated text with Excel-style quoting.
pub(crate) fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut at_start = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if at_start => {
                quoted = true;
                at_start = false;
            }
            '\t' => {
                row.push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            _ => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}
