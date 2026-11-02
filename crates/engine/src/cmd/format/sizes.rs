use super::{
    CellRef, EngineError, Json, RangeRef, Result, Session, StyleId, bad, edit, f64_param, parse_lines, split_sheet, str_param, target_sheet,
};

// ---------------------------------------------------------------- sizes

/// The sheet and the row (or column) spans a size or visibility command works on. The lines are
/// `rows`/`cols` (`"2:5"`, `"B:B,D:F"`, optionally `Sheet!`-prefixed), else the selection's; the
/// sheet is `sheet`, else the prefix, else the active one. The selection belongs to the active
/// sheet, so another sheet needs explicit lines.
fn line_targets(s: &Session, p: &Json, rows: bool) -> Result<(usize, Vec<(u32, u32)>)> {
    let key = if rows { "rows" } else { "cols" };
    let mut sheet = target_sheet(s, p)?;
    if let Some(t) = str_param(p, key) {
        let (prefix, body) = split_sheet(t);
        if let Some(name) = prefix {
            let named = s.doc()?.wb.sheet_index(&name).ok_or_else(|| bad(key, format!("no sheet named `{name}`")))?;
            if p.get("sheet").is_some_and(|v| !v.is_null()) && named != sheet {
                return Err(bad(key, format!("`{t}` names a sheet other than `sheet`")));
            }
            sheet = named;
        }
        // A comma-separated list ("B:B,D:F") targets several areas, like a multi-area selection.
        let lines = body
            .split(',')
            .map(|part| {
                let r = parse_lines(part).ok_or_else(|| bad(key, format!("not a range: {part}")))?;
                Ok(if rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) })
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((sheet, lines));
    }
    let d = s.doc()?;
    if sheet != d.wb.active_sheet() {
        return Err(bad(key, format!("missing `{key}`: the selection is on the active sheet")));
    }
    Ok((sheet, d.selection.ranges.iter().map(|r| if rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) }).collect()))
}

/// Sizes are pixels at 96 dpi, the model's unit (header drags send pixels). `points` (row height)
/// and `chars` (column width) take Excel's units, those of its Row Height and Column Width dialogs
/// and of the file (`ht` is in points).
pub(super) fn row_height(s: &mut Session, p: &Json) -> Result<Json> {
    let h = match (f64_param(p, "height"), f64_param(p, "points")) {
        (Some(h), _) => {
            if !(0.0..=546.0).contains(&h) {
                return Err(EngineError::Other("Row height must be between 0 and 546 pixels (409.5 points).".into()));
            }
            h
        }
        (None, Some(pt)) => {
            if !(0.0..=409.5).contains(&pt) {
                return Err(EngineError::Other("Row height must be between 0 and 409.5 points.".into()));
            }
            pt * 4.0 / 3.0
        }
        _ => return Err(bad("home.rowHeight", "missing `height` (pixels) or `points`")),
    };
    let (si, lines) = line_targets(s, p, true)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            for r in a..=b.min(a.saturating_add(1_048_575)) {
                let e = sh.rows.entry(r).or_default();
                e.size = Some(h as f32);
                e.hidden = h == 0.0;
                e.custom = true;
            }
        }
        cx.structural = true; // setting a height shows or hides rows: SUBTOTAL(10x) depends on it
        Ok(Json::Null)
    })
}

pub(super) fn col_width(s: &mut Session, p: &Json) -> Result<Json> {
    let w = match (f64_param(p, "width"), f64_param(p, "chars")) {
        (Some(w), _) => {
            // 255 characters of the Normal font, and never less than the 1800 px allowed so far.
            let most = f64::from(gridcraft_model::widest_col_width(s.doc()?.wb.digit_width())).max(1800.0);
            if !(0.0..=most).contains(&w) {
                return Err(EngineError::Other(format!("Column width must be between 0 and {most} pixels.")));
            }
            w
        }
        (None, Some(c)) => {
            if !(0.0..=255.0).contains(&c) {
                return Err(EngineError::Other("Column width must be between 0 and 255 characters.".into()));
            }
            chars_to_points(c, s.doc()?.wb.digit_width())
        }
        _ => return Err(bad("home.columnWidth", "missing `width` (pixels) or `chars`")),
    };
    let (si, lines) = line_targets(s, p, false)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            for c in a..=b {
                let e = sh.cols.entry(c).or_default();
                e.size = Some(w as f32);
                e.hidden = w == 0.0;
            }
        }
        Ok(Json::Null)
    })
}

pub use gridcraft_model::chars_to_points;

/// Approximate text width in points for autofit (no font metrics in the engine: average glyph
/// widths for the default font; a front end with fonts measures instead, see `Session::text_width`).
pub fn approx_text_width(text: &str, size: f32, bold: bool) -> f32 {
    let mut w = 0.0;
    for c in text.chars() {
        w += match c {
            'i' | 'l' | 'j' | '\'' | '.' | ',' | ':' | ';' | '!' | '|' => 0.28,
            'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '-' => 0.38,
            'm' | 'w' | 'M' | 'W' => 0.85,
            c if c.is_ascii_uppercase() => 0.65,
            c if c.is_ascii_digit() => 0.56,
            c if c.is_ascii() => 0.52,
            _ => 1.0,
        };
    }
    w * size * if bold { 1.07 } else { 1.0 } * 96.0 / 72.0
}

/// Distinct texts AutoFit measures exactly; past this many, `Session::text_width` is asked for a
/// quick measure so a huge column doesn't lay out every value.
const EXACT_TEXT_WIDTHS: usize = 2_000;

pub(super) fn autofit_cols(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, lines) = line_targets(s, p, false)?;
    let measure = s.text_width.clone();
    edit(s, |cx| {
        let Some(sh) = cx.wb.sheet(si) else { return Ok(Json::Null) };
        // At most 255 characters of the Normal font, the widest a file can store.
        let widest = gridcraft_model::widest_col_width(cx.wb.digit_width());
        let mut widths = Vec::new();
        let mut measured: std::collections::HashMap<StyleId, std::collections::HashMap<String, f32>> = Default::default();
        let mut distinct = 0usize;
        let used = sh.used_range();
        for (a, b) in lines {
            for c in a..=b.min(a + 16383) {
                let Some(u) = used else { continue };
                let col = RangeRef::new(CellRef::new(u.start.row, c), CellRef::new(u.end.row, c));
                let mut max: f32 = 0.0;
                for (cell, _) in sh.cells.iter_range(col) {
                    if sh.merge_at(cell).is_some_and(|m| !m.is_single()) {
                        continue;
                    }
                    let id = sh.style_id(cell);
                    let st = cx.wb.styles.get(id);
                    let text = crate::display::cell_text(&cx.wb, sh, cell);
                    for line in text.lines() {
                        let w = match &measure {
                            Some(m) => {
                                let known = measured.entry(id).or_default();
                                match known.get(line) {
                                    Some(w) => *w,
                                    None => {
                                        let w = m(line, &st.font, distinct < EXACT_TEXT_WIDTHS);
                                        known.insert(line.to_owned(), w);
                                        distinct += 1;
                                        w
                                    }
                                }
                            }
                            None => approx_text_width(line, st.font.size, st.font.bold),
                        };
                        max = max.max(w + 6.0 + st.align.indent as f32 * 9.0 + sh.filter_button_space(cell));
                    }
                }
                if max > 0.0 {
                    widths.push((c, max.clamp(8.0, widest)));
                }
            }
        }
        let sh = cx.sheet_mut(si)?;
        for (c, w) in widths {
            let e = sh.cols.entry(c).or_default();
            e.size = Some(w.ceil());
            e.hidden = false;
        }
        Ok(Json::Null)
    })
}

pub(super) fn autofit_rows(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, lines) = line_targets(s, p, true)?;
    edit(s, |cx| {
        for (a, b) in lines {
            let b = b.min(a + 100_000);
            if let Some(sh) = cx.wb.sheet_mut(si) {
                for r in a..=b {
                    if let Some(e) = sh.rows.get_mut(&r) {
                        e.custom = false;
                        e.hidden = false;
                    }
                }
            }
            cx.fit_rows.extend((a..=b).map(|r| (si, r)));
        }
        cx.structural = true; // autofit reveals hidden rows: SUBTOTAL(10x) depends on it
        Ok(Json::Null)
    })
}

pub(super) fn default_width(s: &mut Session, p: &Json) -> Result<Json> {
    let w = f64_param(p, "width").ok_or_else(|| bad("home.defaultWidth", "missing `width`"))?;
    let si = target_sheet(s, p)?;
    edit(s, |cx| {
        cx.sheet_mut(si)?.default_col_width = w.clamp(0.0, 1800.0) as f32;
        Ok(Json::Null)
    })
}

pub(super) fn hide(s: &mut Session, p: &Json, rows: bool, on: bool) -> Result<Json> {
    let (si, lines) = line_targets(s, p, rows)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            // Unhiding a single selected line also reveals hidden neighbours (Excel needs the
            // lines on both sides selected; we're forgiving).
            let (a, b) = if !on && a == b { (a.saturating_sub(1), b + 1) } else { (a, b) };
            for i in a..=b.min(a.saturating_add(1_048_575)) {
                let map = if rows { &mut sh.rows } else { &mut sh.cols };
                if on {
                    map.entry(i).or_default().hidden = true;
                } else if let Some(e) = map.get_mut(&i) {
                    e.hidden = false;
                    if e.size == Some(0.0) {
                        e.size = None;
                    }
                }
            }
        }
        cx.structural = true; // SUBTOTAL(10x) depends on hidden rows
        Ok(Json::Null)
    })
}
