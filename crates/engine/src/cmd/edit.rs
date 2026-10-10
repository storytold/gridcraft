//! Cell entry, selection, undo/redo, clipboard, clear, fill, find & replace.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, Formula, StyleId};
use serde_json::{Value as Json, json};

use super::*;
use crate::selection::{current_region, jump};
use crate::{Clipboard, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("cell.set", "Enter Cell", [], None, "{cell?: \"B2\", input: \"text, number or =formula\", sheet?, array?: bool}", has_doc, cell_set),
        cmd!(
            "range.setValues",
            "Set Values",
            [],
            None,
            "{range?: \"A1\", values: [[...]] (rows of numbers/strings/bools/null; strings starting with = are formulas)}",
            has_doc,
            range_set_values
        ),
        cmd!(
            "range.fill",
            "Fill Range With Input",
            [],
            Some("Ctrl+Enter"),
            "{range?, input}: enters the same input in every selected cell (relative formulas adjust)",
            has_doc,
            range_fill
        ),
        cmd!(noundo "selection.set", "Select", [], None, "{range: \"A1:B2,D4\" | cell, active?: \"A1\", sheet?}", has_doc, selection_set),
        cmd!(noundo "selection.move", "Move Selection", [], None, "{dr, dc, extend?: bool, jump?: bool (Ctrl+arrow), page?: bool}", has_doc, selection_move),
        cmd!(noundo "selection.next", "Next Cell in Selection", [], None, "{forward?: true, byRow?: false} (Enter/Tab inside a selection)", has_doc, selection_next),
        cmd!(noundo "edit.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{} (current region first, then the whole sheet)", has_doc, select_all),
        cmd!(noundo "selection.currentRegion", "Select Current Region", [], Some("Ctrl+Shift+8"), "{}", has_doc, select_region),
        cmd!(noundo "selection.row", "Select Entire Row", [], Some("Shift+Space"), "{}", has_doc, select_row),
        cmd!(noundo "selection.column", "Select Entire Column", [], Some("Ctrl+Space"), "{}", has_doc, select_col),
        cmd!(noundo "edit.goTo", "Go To…", ["Home", "Editing", "Find & Select"], Some("Ctrl+G"), "{reference: \"B5\" | \"Sheet2!A1:C3\" | name}", has_doc, go_to),
        cmd!(noundo "edit.goToSpecial", "Go To Special…", ["Home", "Editing", "Find & Select"], None, "{kind: blanks|constants|formulas|comments|lastCell|currentRegion|visible|conditionalFormats|dataValidation|errors|numbers|text}", has_doc, go_to_special),
        cmd!(noundo "edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{steps?: 1}", can_undo, undo),
        cmd!(noundo "edit.redo", "Redo", ["Edit"], Some("Cmd+Y"), "{steps?: 1}", can_redo, redo),
        cmd!(noundo "edit.copy", "Copy", ["Home", "Clipboard"], Some("Cmd+C"), "{range?, html?: bool} → {range, text, html?} (html: a styled HTML table, only when requested and small enough)", has_doc, copy),
        cmd!(noundo "edit.cut", "Cut", ["Home", "Clipboard"], Some("Cmd+X"), "{range?, html?: bool} → {range, text, html?} (html: a styled HTML table, only when requested and small enough)", has_doc, cut),
        cmd!(
            "edit.paste",
            "Paste",
            ["Home", "Clipboard"],
            Some("Cmd+V"),
            "{at?: \"C3\", text?: \"tab-separated text from the system clipboard\"}",
            has_doc,
            paste
        ),
        cmd!(
            "edit.pasteSpecial",
            "Paste Special…",
            ["Home", "Clipboard"],
            Some("Ctrl+Cmd+V"),
            "{what: all|formulas|values|formats|comments|validation|allExceptBorders|columnWidths|formulasAndNumberFormats|valuesAndNumberFormats, operation?: none|add|subtract|multiply|divide, skipBlanks?, transpose?, link?}",
            has_clipboard,
            paste_special
        ),
        cmd!(noundo "edit.clearClipboard", "Cancel Copy", [], Some("Escape"), "{}", has_doc, clear_clipboard),
        cmd!("edit.clearAll", "Clear All", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "all")),
        cmd!("edit.clearFormats", "Clear Formats", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "formats")),
        cmd!("edit.clearContents", "Clear Contents", ["Home", "Editing", "Clear"], Some("Delete"), "{range?}", has_doc, |s, p| clear(
            s, p, "contents"
        )),
        cmd!("edit.clearComments", "Clear Comments and Notes", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(
            s, p, "comments"
        )),
        cmd!("edit.clearHyperlinks", "Clear Hyperlinks", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "hyperlinks")),
        cmd!("edit.fillDown", "Fill Down", ["Home", "Editing", "Fill"], Some("Cmd+D"), "{range?}", has_doc, |s, p| fill_dir(s, p, 1, 0)),
        cmd!("edit.fillRight", "Fill Right", ["Home", "Editing", "Fill"], Some("Cmd+R"), "{range?}", has_doc, |s, p| fill_dir(s, p, 0, 1)),
        cmd!("edit.fillUp", "Fill Up", ["Home", "Editing", "Fill"], None, "{range?}", has_doc, |s, p| fill_dir(s, p, -1, 0)),
        cmd!("edit.fillLeft", "Fill Left", ["Home", "Editing", "Fill"], None, "{range?}", has_doc, |s, p| fill_dir(s, p, 0, -1)),
        cmd!(
            "edit.autoFill",
            "AutoFill",
            [],
            None,
            "{source: \"A1:A2\", target: \"A1:A10\", mode?: series|copy|formats|values}: the fill-handle drag",
            has_doc,
            auto_fill
        ),
        cmd!(
            "edit.fillSeries",
            "Series…",
            ["Home", "Editing", "Fill"],
            None,
            "{range?, direction?: columns|rows, type?: linear|growth|date|autofill, step?: 1, stop?, dateUnit?: day|weekday|month|year}",
            has_doc,
            fill_series
        ),
        cmd!("edit.flashFill", "Flash Fill", ["Home", "Editing", "Fill"], Some("Cmd+E"), "{range?}", has_doc, flash_fill),
        cmd!(noundo "edit.find", "Find…", ["Home", "Editing", "Find & Select"], Some("Cmd+F"), "{what, matchCase?, wholeCell?, lookIn?: formulas|values, byColumns?, all?: bool, within?: sheet|workbook}", has_doc, find),
        cmd!(
            "edit.replace",
            "Replace…",
            ["Home", "Editing", "Find & Select"],
            Some("Ctrl+H"),
            "{what, with, matchCase?, wholeCell?, all?: true, within?: sheet|workbook}",
            has_doc,
            replace
        ),
        cmd!(
            "edit.formatPainter",
            "Format Painter",
            ["Home", "Clipboard"],
            None,
            "{sticky?: bool} — first call picks up the selection's formats; with {apply: \"C3:D4\"} pastes them",
            has_doc,
            format_painter
        ),
        cmd!(noundo "edit.beginEdit", "Edit Cell", [], Some("F2"), "{}", has_doc, begin_edit),
    ]
}

// ---------------------------------------------------------------- entry

/// What typing `input` into a cell produces (constant or formula, plus an automatic number
/// format). Inherits the destination's cell, row or column style, including its number
/// format, before parsing. Errors when the formula can't be parsed.
pub(crate) fn input_to_cell(input: &str, sheet: usize, at: CellRef, wb: &mut gridcraft_model::Workbook) -> Result<Option<Cell>> {
    let style = wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
    if input.is_empty() {
        let c = Cell { value: Value::Empty, formula: None, style };
        return Ok(if c.is_blank() { None } else { Some(c) });
    }
    let fmt_is_text = wb.styles.get(style).num_fmt.as_str() == "@";
    if fmt_is_text {
        return Ok(Some(Cell { value: Value::text(input), formula: None, style }));
    }
    let is_formula = input.starts_with('=')
        || (input.len() > 1
            && (input.starts_with('+') || input.starts_with('-'))
            && gridcraft_core::parse::parse_number_text(input).is_none()
            && input.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '('));
    if is_formula {
        let body = input.strip_prefix('=').unwrap_or(input);
        // Excel closes missing parentheses for you.
        let mut text = body.to_string();
        let mut parsed = gridcraft_formula::parse(&text);
        for _ in 0..8 {
            if parsed.is_ok() {
                break;
            }
            text.push(')');
            parsed = gridcraft_formula::parse(&text);
        }
        let expr = parsed.map_err(|e| EngineError::Other(format!("There's a problem with this formula: {e}")))?;
        let expr = wb.name_call_case(expr);
        let mut cell = Cell::formula(Formula::from_expr(expr));
        cell.style = style;
        // Formulas whose result is a date/time get a format from functions like TODAY().
        if wb.styles.get(style).num_fmt.as_str() == "General" {
            let f = cell.formula.as_ref().map(|f| f.text.to_ascii_uppercase()).unwrap_or_default();
            let auto = if f.starts_with("TODAY(")
                || f.starts_with("DATE(")
                || f.starts_with("EDATE(")
                || f.starts_with("EOMONTH(")
                || f.starts_with("WORKDAY(")
            {
                Some("m/d/yyyy")
            } else if f.starts_with("NOW(") {
                Some("m/d/yyyy h:mm")
            } else if f.starts_with("TIME(") {
                Some("h:mm AM/PM")
            } else {
                None
            };
            if let Some(code) = auto {
                cell.style = wb.styles.derive(style, |s| s.num_fmt = gridcraft_model::NumFmt::new(code));
            }
        }
        return Ok(Some(cell));
    }
    let parsed = gridcraft_core::parse::parse_input(input, wb.date_system);
    let mut style = style;
    if let Some(code) = parsed.format {
        let cur = wb.styles.get(style).num_fmt.as_str().to_string();
        if cur == "General" {
            style = wb.styles.derive(style, |s| s.num_fmt = gridcraft_model::NumFmt::new(code));
        }
    }
    // Wrap text automatically when the input has line breaks (Alt+Enter).
    if input.contains('\n') && !wb.styles.get(style).align.wrap {
        style = wb.styles.derive(style, |s| s.align.wrap = true);
    }
    Ok(Some(Cell { value: parsed.value, formula: None, style }))
}

fn cell_set(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = match cell_param(p, "cell") {
        Some(c) => c,
        None => s.doc()?.selection.active,
    };
    let input =
        str_param(p, "input").or_else(|| str_param(p, "value")).map(str::to_string).or_else(|| p.get("value").map(json_to_input)).unwrap_or_default();
    let array = bool_param(p, "array").unwrap_or(false);
    let protected =
        s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.is_protected() && s.doc().is_ok_and(|d| d.wb.styles.get(sh.style_id(at)).protection.locked));
    if protected {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    edit(s, |cx| {
        if array {
            // Legacy Ctrl+Shift+Enter array formula over the selection.
            let range = cx.sel.current();
            let mut cell = input_to_cell(&input, sheet, range.start, &mut cx.wb)?.unwrap_or_default();
            if let Some(f) = cell.formula.as_mut() {
                std::sync::Arc::make_mut(f).array = Some(range);
            }
            let sh = cx.sheet_mut(sheet)?;
            // The array takes the whole range: other cells in it lose their contents.
            for c in range.iter().skip(1).take(1_000_000) {
                if let Some(old) = sh.cells.get(c).filter(|x| !x.value.is_empty() || x.formula.is_some()) {
                    let style = old.style;
                    sh.set_cell(c, Cell { style, ..Cell::default() });
                }
            }
            sh.set_cell(range.start, cell);
            cx.touch(sheet, range.start);
            return Ok(Json::Null);
        }
        let cell = input_to_cell(&input, sheet, at, &mut cx.wb)?;
        let sh = cx.sheet_mut(sheet)?;
        match cell {
            Some(c) => sh.set_cell(at, c),
            None => {
                sh.remove_cell(at);
            }
        }
        cx.touch(sheet, at);
        Ok(Json::Null)
    })?;
    let v = s.doc()?.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": v.display()}))
}

pub(crate) fn json_to_input(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Json::Number(n) => n.to_string(),
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn range_set_values(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let start = target_range(s, p)?.start;
    let Some(rows) = p.get("values").and_then(Json::as_array) else { return Err(bad("range.setValues", "missing `values` (array of rows)")) };
    if rows.len() > MAX_ROWS as usize {
        return Err(bad("range.setValues", "too many rows"));
    }
    let mut count = 0;
    edit(s, |cx| {
        for (ri, row) in rows.iter().enumerate() {
            let cols: Vec<&Json> = match row.as_array() {
                Some(a) => a.iter().collect(),
                None => vec![row],
            };
            for (ci, v) in cols.iter().enumerate() {
                let Some(at) = start.offset(ri as i64, ci as i64) else { continue };
                let input = json_to_input(v);
                let cell = match v {
                    Json::Number(n) => Some(Cell {
                        value: Value::number(n.as_f64().unwrap_or(0.0)),
                        formula: None,
                        style: cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default(),
                    }),
                    Json::Bool(b) => {
                        Some(Cell { value: Value::Bool(*b), formula: None, style: cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default() })
                    }
                    _ => input_to_cell(&input, sheet, at, &mut cx.wb)?,
                };
                let sh = cx.sheet_mut(sheet)?;
                match cell {
                    Some(c) => sh.set_cell(at, c),
                    None => {
                        sh.remove_cell(at);
                    }
                }
                cx.touch(sheet, at);
                count += 1;
            }
        }
        Ok(())
    })?;
    Ok(json!({"cells": count}))
}

fn range_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let input = str_param(p, "input").unwrap_or("").to_string();
    let origin = s.doc()?.selection.active;
    edit(s, |cx| {
        let base = input_to_cell(&input, sheet, origin, &mut cx.wb)?;
        for r in &ranges {
            if r.count() > 2_000_000 {
                return Err(bad("range.fill", "range too large"));
            }
            for at in r.iter() {
                let mut cell = base.clone().unwrap_or_default();
                if let Some(f) = &cell.formula {
                    cell.formula = Some(std::sync::Arc::new(f.moved(at.row as i64 - origin.row as i64, at.col as i64 - origin.col as i64)));
                }
                cell.style = cx.wb.sheet(sheet).and_then(|sh| sh.cell(at)).map(|c| c.style).unwrap_or(cell.style);
                cx.sheet_mut(sheet)?.set_cell(at, cell);
                cx.touch(sheet, at);
            }
        }
        Ok(Json::Null)
    })
}

// ---------------------------------------------------------------- selection

fn selection_set(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("sheet").is_some() {
        let i = target_sheet(s, p)?;
        s.doc_mut()?.wb_switch_sheet(i);
    } else if let Some(r) = str_param(p, "range").or_else(|| str_param(p, "cell"))
        && let (Some(_), _) = split_sheet(r)
    {
        let i = target_sheet(s, p)?;
        s.doc_mut()?.wb_switch_sheet(i);
    }
    let ranges = target_ranges(s, p)?;
    let d = s.doc_mut()?;
    let active = cell_param(p, "active").or_else(|| ranges.last().map(|r| r.start)).unwrap_or_default();
    let mut sel = crate::Selection { active, anchor: ranges.last().map(|r| r.start).unwrap_or(active), ranges };
    if let Some(sh) = d.wb.active() {
        sel.expand_merges(sh);
    }
    d.selection = sel;
    Ok(json!({"selection": d.selection.a1(), "active": d.selection.active.a1()}))
}

impl crate::DocState {
    /// Switches the active sheet, remembering each sheet's selection.
    pub fn wb_switch_sheet(&mut self, i: usize) {
        if i >= self.wb.sheets.len() || i == self.wb.active_sheet {
            return;
        }
        let cur = self.wb.active_sheet;
        if self.sheet_selections.len() < self.wb.sheets.len() {
            self.sheet_selections.resize(self.wb.sheets.len(), crate::Selection::default());
        }
        if let Some(slot) = self.sheet_selections.get_mut(cur) {
            *slot = self.selection.clone();
        }
        // The active sheet is view state: change it without making the workbook dirty.
        let dirty = self.is_dirty();
        let wb = std::sync::Arc::make_mut(&mut self.wb);
        wb.active_sheet = i;
        if !dirty {
            self.saved = self.wb.clone();
        }
        self.selection = self.sheet_selections.get(i).cloned().unwrap_or_default();
    }
}

fn selection_move(s: &mut Session, p: &Json) -> Result<Json> {
    let dr = p.get("dr").and_then(Json::as_i64).unwrap_or(0);
    let dc = p.get("dc").and_then(Json::as_i64).unwrap_or(0);
    let extend = bool_param(p, "extend").unwrap_or(false);
    let jmp = bool_param(p, "jump").unwrap_or(false);
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let sel = &mut d.selection;
    // Extension moves the far corner, not the active cell.
    let from = if extend {
        let r = sel.current();
        let far_row = if sel.anchor.row == r.start.row { r.end.row } else { r.start.row };
        let far_col = if sel.anchor.col == r.start.col { r.end.col } else { r.start.col };
        CellRef::new(far_row, far_col)
    } else {
        sel.active
    };
    // Skip hidden rows/columns and step over merged areas.
    let mut to = if jmp {
        jump(sheet, from, dr.signum(), dc.signum())
    } else {
        let mut t = from.offset_clamped(dr, dc);
        let mut guard = 0;
        while (sheet.is_row_hidden(t.row) || sheet.is_col_hidden(t.col)) && guard < 100_000 {
            let n = t.offset_clamped(dr.signum(), dc.signum());
            if n == t {
                break;
            }
            t = n;
            guard += 1;
        }
        if !extend && let Some(m) = sheet.merge_at(from) {
            // Leaving a merged cell moves from its edge.
            if dr > 0 {
                t = CellRef::new((m.end.row + dr as u32).min(MAX_ROWS - 1), from.col);
            } else if dc > 0 {
                t = CellRef::new(from.row, (m.end.col + dc as u32).min(MAX_COLS - 1));
            }
        }
        t
    };
    if let Some(m) = sheet.merge_at(to)
        && !extend
    {
        to = m.start;
    }
    if extend {
        sel.extend_to(to);
        sel.expand_merges(sheet);
    } else {
        *sel = crate::Selection::at(to);
        if let Some(m) = sheet.merge_at(to) {
            sel.ranges = vec![m];
        }
    }
    Ok(json!({"selection": sel.a1(), "active": sel.active.a1()}))
}

fn selection_next(s: &mut Session, p: &Json) -> Result<Json> {
    let fwd = bool_param(p, "forward").unwrap_or(true);
    let by_row = bool_param(p, "byRow").unwrap_or(false);
    let d = s.doc_mut()?;
    d.selection.cycle(fwd, by_row);
    Ok(json!({"active": d.selection.active.a1()}))
}

fn select_all(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let region = current_region(sheet, d.selection.active);
    let all = RangeRef::all();
    let r = if region.is_single() || d.selection.current() == region { all } else { region };
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_region(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let r = current_region(sheet, d.selection.active);
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_row(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(r.start.row, 0), ranges: vec![RangeRef::rows(r.start.row, r.end.row)] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_col(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(0, r.start.col), ranges: vec![RangeRef::cols(r.start.col, r.end.col)] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn go_to(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(reference) = str_param(p, "reference").or_else(|| str_param(p, "range")) else { return Err(bad("edit.goTo", "missing `reference`")) };
    // A defined name or table?
    let d = s.doc()?;
    let resolved = if RangeRef::parse(split_sheet(reference).1).is_none() {
        let sheet = d.wb.active_sheet;
        if let Some(n) = d.wb.name(reference, sheet) {
            n.formula.clone()
        } else if let Some((si, ti)) = d.wb.table(reference) {
            // As in Excel, a table's name stands for its data, without the header and totals rows.
            let t = d.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| t.data_range().unwrap_or(t.range).a1()).unwrap_or_default();
            format!("'{}'!{}", d.wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default(), t)
        } else {
            return Err(EngineError::Other("Reference isn't valid.".into()));
        }
    } else {
        reference.to_string()
    };
    let resolved = resolved.replace('$', "");
    selection_set(s, &json!({"range": resolved}))
}

fn go_to_special(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = str_param(p, "kind").unwrap_or("blanks");
    let d = s.doc()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let scope = if d.selection.is_single_cell() { sheet.used_range().unwrap_or(RangeRef::cell(d.selection.active)) } else { d.selection.bounds() };
    if kind == "lastCell" {
        let c = sheet.used_range().map(|r| r.end).unwrap_or_default();
        return selection_set(s, &json!({"cell": c.a1()}));
    }
    if kind == "currentRegion" {
        return select_region(s, &json!({}));
    }
    let mut hits: Vec<CellRef> = Vec::new();
    if scope.count() > 4_000_000 {
        return Err(bad("edit.goToSpecial", "range too large"));
    }
    for c in scope.iter() {
        let cell = sheet.cell(c);
        let v = sheet.value(c);
        let ok = match kind {
            "blanks" => v.is_empty(),
            "constants" => cell.is_some_and(|x| x.formula.is_none() && !x.value.is_empty()),
            "formulas" => cell.is_some_and(|x| x.formula.is_some()),
            "comments" | "notes" => sheet.comments.contains_key(&c),
            "errors" => v.is_error(),
            "numbers" => v.is_number(),
            "text" => v.is_text(),
            "visible" => !sheet.is_row_hidden(c.row) && !sheet.is_col_hidden(c.col),
            "conditionalFormats" => sheet.cond_formats.iter().any(|cf| cf.ranges.iter().any(|r| r.contains(c))),
            "dataValidation" => sheet.validations.iter().any(|dv| dv.ranges.iter().any(|r| r.contains(c))),
            _ => return Err(bad("edit.goToSpecial", format!("unknown kind `{kind}`"))),
        };
        if ok {
            hits.push(c);
        }
    }
    if hits.is_empty() {
        return Err(EngineError::Other("No cells were found.".into()));
    }
    let ranges = coalesce(&hits);
    let d = s.doc_mut()?;
    d.selection = crate::Selection { active: hits[0], anchor: hits[0], ranges };
    Ok(json!({"selection": d.selection.a1(), "count": hits.len()}))
}

/// Merges cells into vertical runs per column (a compact multi-area selection).
fn coalesce(cells: &[CellRef]) -> Vec<RangeRef> {
    let mut sorted = cells.to_vec();
    sorted.sort_by_key(|c| (c.col, c.row));
    let mut out: Vec<RangeRef> = Vec::new();
    for c in sorted {
        if let Some(last) = out.last_mut()
            && last.start.col == c.col
            && last.end.col == c.col
            && last.end.row + 1 == c.row
        {
            last.end.row = c.row;
            continue;
        }
        out.push(RangeRef::cell(c));
    }
    if out.len() > 2000 {
        out.truncate(2000);
    }
    out
}

// ---------------------------------------------------------------- history

fn undo(s: &mut Session, p: &Json) -> Result<Json> {
    let steps = u32_param(p, "steps").unwrap_or(1).max(1);
    let d = s.doc_mut()?;
    let before = d.wb.clone();
    let mut label = String::new();
    for _ in 0..steps {
        let Some(e) = d.undo.pop() else { break };
        let cur = crate::HistoryEntry { label: e.label.clone(), wb: d.wb.clone(), selection: d.selection.clone() };
        d.redo.push(cur);
        label = e.label.clone();
        d.wb = e.wb;
        d.selection = e.selection;
    }
    d.calc.sync(&before, &d.wb);
    d.revision += 1;
    Ok(json!({"undone": label}))
}

fn redo(s: &mut Session, p: &Json) -> Result<Json> {
    let steps = u32_param(p, "steps").unwrap_or(1).max(1);
    let d = s.doc_mut()?;
    let before = d.wb.clone();
    let mut label = String::new();
    for _ in 0..steps {
        let Some(e) = d.redo.pop() else { break };
        let cur = crate::HistoryEntry { label: e.label.clone(), wb: d.wb.clone(), selection: d.selection.clone() };
        d.undo.push(cur);
        label = e.label.clone();
        d.wb = e.wb;
        d.selection = e.selection;
    }
    d.calc.sync(&before, &d.wb);
    d.revision += 1;
    Ok(json!({"redone": label}))
}

// ---------------------------------------------------------------- clipboard

fn copy_impl(s: &mut Session, p: &Json, cut: bool) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if ranges.len() > 1 {
        return Err(EngineError::Other("This action won't work on multiple selections.".into()));
    }
    let range = ranges.first().copied().unwrap_or_default();
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let range = if range.is_full_cols() || range.is_full_rows() {
        let used = sh.used_range().unwrap_or(RangeRef::cell(range.start));
        range.intersection(&used.union(&RangeRef::cell(range.start))).unwrap_or(range)
    } else {
        range
    };
    let text = crate::display::range_text(&d.wb, sheet, range);
    // The HTML rendering is for the host clipboard (UI); it can reach 4 MB, so agents and
    // scripts get it only when they ask for it.
    let html = if p.get("html").and_then(Json::as_bool) == Some(true) { crate::io::range_html(&d.wb, sheet, range, &text) } else { None };
    let clip = Clipboard { wb: d.wb.clone(), sheet, range, cut, doc_uid: d.uid, text: text.clone() };
    s.clipboard = Some(clip);
    let mut result = json!({"range": range.a1(), "text": text});
    if let Some(html) = html {
        result["html"] = Json::String(html);
    }
    Ok(result)
}

fn copy(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, false)
}
fn cut(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, true)
}
fn clear_clipboard(s: &mut Session, _: &Json) -> Result<Json> {
    s.clipboard = None;
    s.format_painter = None;
    ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum What {
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

fn paste(s: &mut Session, p: &Json) -> Result<Json> {
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
    let at = cell_param(p, "at").unwrap_or(s.doc()?.selection.current().start);
    let rows: Vec<Vec<String>> = parse_tsv(text);
    let h = rows.len();
    let w = rows.iter().map(Vec::len).max().unwrap_or(0);
    edit(s, |cx| {
        for (ri, row) in rows.iter().enumerate() {
            for (ci, v) in row.iter().enumerate() {
                let Some(c) = at.offset(ri as i64, ci as i64) else { continue };
                let cell = input_to_cell(v, sheet, c, &mut cx.wb).unwrap_or_else(|_| Some(Cell::value(Value::text(v.as_str()))));
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

fn paste_special(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(clip) = s.clipboard.clone() else { return Err(EngineError::Other("Nothing to paste.".into())) };
    let what = match str_param(p, "what").unwrap_or("all") {
        "all" => What::All,
        "formulas" => What::Formulas,
        "values" => What::Values,
        "formats" => What::Formats,
        "comments" | "notes" => What::Comments,
        "validation" => What::Validation,
        "allExceptBorders" => What::AllExceptBorders,
        "columnWidths" => What::ColumnWidths,
        "formulasAndNumberFormats" => What::FormulasAndNumberFormats,
        "valuesAndNumberFormats" => What::ValuesAndNumberFormats,
        other => return Err(bad("edit.pasteSpecial", format!("unknown `what`: {other}"))),
    };
    let op = str_param(p, "operation").unwrap_or("none").to_string();
    let skip_blanks = bool_param(p, "skipBlanks").unwrap_or(false);
    let transpose = bool_param(p, "transpose").unwrap_or(false);
    let link = bool_param(p, "link").unwrap_or(false);
    let dest_sheet = target_sheet(s, p)?;
    let sel = s.doc()?.selection.current();
    let at = cell_param(p, "at").unwrap_or(sel.start);
    let src = clip.range;
    let (h, w) = if transpose { (src.width(), src.height()) } else { (src.height(), src.width()) };
    // Tile the copy over a larger destination that is a multiple of its size.
    let tile = sel.height() % h == 0
        && sel.width() % w == 0
        && !sel.is_single()
        && cell_param(p, "at").is_none()
        && !sel.is_full_cols()
        && !sel.is_full_rows()
        && sel.count() <= 4_000_000;
    let (tiles_r, tiles_c) = if tile { (sel.height() / h, sel.width() / w) } else { (1, 1) };
    let same_doc = s.doc()?.uid == clip.doc_uid;
    let cut_move = clip.cut && same_doc && what == What::All;
    let src_wb = clip.wb.clone();
    let Some(src_sheet) = src_wb.sheet(clip.sheet) else { return Err(EngineError::Other("The copied sheet no longer exists.".into())) };
    let src_sheet_name = src_sheet.name.clone();
    edit(s, |cx| {
        // Styles from another workbook must be re-interned.
        let mut style_map = std::collections::HashMap::new();
        let mut map_style = |id: StyleId, wb: &mut gridcraft_model::Workbook| -> StyleId {
            if same_doc {
                return id;
            }
            *style_map.entry(id).or_insert_with(|| wb.styles.intern(src_wb.styles.get(id).clone()))
        };
        let mut moved_formulas = Vec::new();
        let mut moved_pictures = std::collections::BTreeMap::new();
        if cut_move {
            // Remove the source first (contents and formats), and fix references to it.
            let source = cx.sheet_mut(clip.sheet)?;
            moved_pictures = source.cell_pictures.iter().filter(|(c, _)| src.contains(**c)).map(|(c, p)| (*c, p.clone())).collect();
            let taken = source.take_cells(src);
            for (c, _) in &taken {
                cx.touch(clip.sheet, *c);
            }
            moved_formulas = taken;
        }
        for tr in 0..tiles_r {
            for tc in 0..tiles_c {
                for r in 0..src.height() {
                    for c in 0..src.width() {
                        let sc = CellRef::new(src.start.row + r, src.start.col + c);
                        let (dr, dc) = if transpose { (c, r) } else { (r, c) };
                        let Some(dest) = at.offset((tr * h + dr) as i64, (tc * w + dc) as i64) else { continue };
                        let src_cell = if cut_move {
                            moved_formulas.iter().find(|(k, _)| *k == sc).map(|(_, v)| v.clone())
                        } else {
                            src_sheet.cell(sc).cloned()
                        };
                        let src_val = src_sheet.value(sc);
                        let src_picture = if cut_move { moved_pictures.get(&sc) } else { src_sheet.cell_pictures.get(&sc) }.cloned();
                        if skip_blanks && src_picture.is_none() && src_val.is_empty() && src_cell.as_ref().is_none_or(|x| x.formula.is_none()) {
                            continue;
                        }
                        let old = cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell(dest)).cloned().unwrap_or_default();
                        let old_picture = cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell_pictures.get(&dest)).cloned();
                        let mut new_picture = old_picture.clone();
                        let mut new = old.clone();
                        let shift = |cell: &Cell| -> Option<std::sync::Arc<Formula>> {
                            let f = cell.formula.as_ref()?;
                            if cut_move {
                                let e = f.expr()?;
                                // A moved formula still reads cells on its original sheet. Qualify
                                // those references before the workbook-wide move adjustment below.
                                let e = if dest_sheet != clip.sheet {
                                    e.map(&mut |x| match x {
                                        gridcraft_formula::Expr::Ref(mut r) if r.sheet == gridcraft_formula::SheetSel::Current => {
                                            r.sheet = gridcraft_formula::SheetSel::Named(src_sheet_name.clone());
                                            gridcraft_formula::Expr::Ref(r)
                                        }
                                        other => other,
                                    })
                                } else {
                                    e
                                };
                                let mut moved = Formula::from_expr(e);
                                moved.array = f.array;
                                return Some(std::sync::Arc::new(moved));
                            }
                            f.parsed()?;
                            Some(std::sync::Arc::new(f.moved(dest.row as i64 - sc.row as i64, dest.col as i64 - sc.col as i64)))
                        };
                        if link {
                            let sheet_prefix = if dest_sheet == clip.sheet && same_doc {
                                String::new()
                            } else {
                                format!("{}!", gridcraft_formula::quote_sheet(&src_sheet_name))
                            };
                            new.formula = Some(std::sync::Arc::new(Formula::new(&format!("={sheet_prefix}{}", sc.a1()))));
                            new.value = Value::Empty;
                            new_picture = None;
                        } else {
                            let sc_cell = src_cell.clone().unwrap_or_default();
                            match what {
                                What::All | What::AllExceptBorders => {
                                    new_picture = src_picture.clone();
                                    new.formula = shift(&sc_cell);
                                    new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
                                    let st = map_style(sc_cell.style, &mut cx.wb);
                                    new.style = if what == What::AllExceptBorders {
                                        let borders = cx.wb.styles.get(old.style).borders;
                                        cx.wb.styles.derive(st, |s| s.borders = borders)
                                    } else {
                                        st
                                    };
                                }
                                What::Formulas | What::FormulasAndNumberFormats => {
                                    new_picture = src_picture.clone();
                                    new.formula = shift(&sc_cell);
                                    new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
                                    if what == What::FormulasAndNumberFormats {
                                        let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                                        new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                                    }
                                }
                                What::Values | What::ValuesAndNumberFormats => {
                                    new_picture = src_picture.clone();
                                    new.formula = None;
                                    new.value = src_val.clone();
                                    if what == What::ValuesAndNumberFormats {
                                        let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                                        new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                                    }
                                }
                                What::Formats => new.style = map_style(sc_cell.style, &mut cx.wb),
                                What::Comments | What::Validation | What::ColumnWidths => {}
                            }
                            // Operations combine numbers with what is already there.
                            if op != "none"
                                && matches!(what, What::All | What::Values | What::ValuesAndNumberFormats | What::Formulas)
                                && new.formula.is_none()
                            {
                                if old_picture.is_some() || new_picture.is_some() {
                                    new_picture = None;
                                    new.value = Value::Error(gridcraft_core::CellError::Value);
                                }
                                let a = old.value.clone();
                                let b = new.value.clone();
                                // Numbers stored as text take part (multiplying by 1 turns them into
                                // numbers); other text and errors stay as they are.
                                if let (Ok(x), Ok(y)) = (a.to_number(), b.to_number())
                                    && (a.is_number() || a.is_empty() || a.is_text())
                                    && (b.is_number() || b.is_empty())
                                {
                                    new.value = match op.as_str() {
                                        "add" => Value::number(x + y),
                                        "subtract" => Value::number(x - y),
                                        "multiply" => Value::number(x * y),
                                        "divide" => {
                                            if y == 0.0 {
                                                Value::Error(gridcraft_core::CellError::Div0)
                                            } else {
                                                Value::number(x / y)
                                            }
                                        }
                                        _ => b.clone(),
                                    };
                                } else if (a.is_text() || a.is_error()) && (b.is_number() || b.is_empty()) {
                                    new.value = a;
                                }
                                // A formula already there is kept and combined with the pasted number.
                                if let Some(f) = &old.formula
                                    && let Ok(y) = b.to_number()
                                    && (b.is_number() || b.is_empty())
                                    && let Some(sym) = match op.as_str() {
                                        "add" => Some('+'),
                                        "subtract" => Some('-'),
                                        "multiply" => Some('*'),
                                        "divide" => Some('/'),
                                        _ => None,
                                    }
                                {
                                    let text = format!("=({}){sym}{}", f.text, gridcraft_core::number_to_text(y));
                                    new.formula = Some(std::sync::Arc::new(Formula::new(&text)));
                                    new.value = Value::Empty;
                                }
                            }
                        }
                        match what {
                            What::Comments => {
                                if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                                    cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                                }
                            }
                            What::ColumnWidths => {
                                if let Some(info) = src_sheet.cols.get(&sc.col).copied() {
                                    cx.sheet_mut(dest_sheet)?.cols.insert(dest.col, info);
                                }
                            }
                            What::All => {
                                if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                                    cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                                }
                                cx.sheet_mut(dest_sheet)?.set_cell(dest, new);
                            }
                            _ => cx.sheet_mut(dest_sheet)?.set_cell(dest, new),
                        }
                        if let Some(picture) = new_picture {
                            cx.sheet_mut(dest_sheet)?.cell_pictures.insert(dest, picture);
                        }
                        cx.touch(dest_sheet, dest);
                    }
                }
            }
        }
        // Merged areas inside the copied block travel with it.
        if matches!(what, What::All | What::Formats) && !transpose {
            let merges: Vec<RangeRef> = src_sheet.merges.iter().filter(|m| src.contains_range(m)).copied().collect();
            for m in merges {
                let dr = at.row as i64 - src.start.row as i64;
                let dc = at.col as i64 - src.start.col as i64;
                if let (Some(a), Some(b)) = (m.start.offset(dr, dc), m.end.offset(dr, dc)) {
                    let nm = RangeRef::new(a, b);
                    let sh = cx.sheet_mut(dest_sheet)?;
                    sh.merges.retain(|x| !x.intersects(&nm));
                    sh.merges.push(nm);
                }
            }
        }
        if cut_move {
            // References elsewhere to the moved block now point to its new place.
            let dest_name = cx.wb.sheet(dest_sheet).map(|s| s.name.clone()).unwrap_or_default();
            let e = if dest_sheet == clip.sheet {
                gridcraft_formula::adjust::Edit::Move { from: src, to_row: at.row, to_col: at.col }
            } else {
                gridcraft_formula::adjust::Edit::MoveToSheet { from: src, to_sheet: dest_name, to_row: at.row, to_col: at.col }
            };
            rewrite_all_formulas(&mut cx.wb, &src_sheet_name, &e);
            cx.structural = true;
        }
        let end = at.offset_clamped((tiles_r * h) as i64 - 1, (tiles_c * w) as i64 - 1);
        *cx.sel = crate::Selection { active: at, anchor: at, ranges: vec![RangeRef::new(at, end)] };
        Ok(())
    })?;
    if cut_move {
        s.clipboard = None;
    }
    Ok(json!({"pasted": s.doc()?.selection.a1()}))
}

/// Applies a reference adjustment to every cell formula and defined name in the workbook.
pub(crate) fn rewrite_all_formulas(wb: &mut gridcraft_model::Workbook, target: &str, e: &gridcraft_formula::adjust::Edit) {
    for si in 0..wb.sheets.len() {
        let host = wb.sheets.get(si).map(|s| s.name.clone()).unwrap_or_default();
        let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
            wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, cell)| cell.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
        if keys.is_empty() {
            continue;
        }
        // Each formula is adjusted on its own, so a large sheet's are spread over threads; only
        // the ones whose references moved are printed and stored again.
        let adjusted = gridcraft_calc::par::filter_map(&keys, |(c, f)| {
            let expr = f.expr_arc()?;
            let new = gridcraft_formula::adjust::adjust((*expr).clone(), &host, target, e);
            (new != *expr).then(|| {
                let mut nf = Formula::from_expr(new);
                nf.array = f.array;
                let key = gridcraft_model::FormulaSharer::key(*c, &nf);
                (*c, nf, key)
            })
        });
        // Formulas that were copies of one another mostly still are: share them again.
        let mut sharer = gridcraft_model::FormulaSharer::default();
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (c, nf, key) in adjusted {
            if let Some(cell) = sheet.cells.get_mut(c) {
                cell.formula = Some(sharer.share_keyed(si, c, nf, key));
            }
        }
    }
    for n in wb.names.iter_mut() {
        if let Ok(expr) = gridcraft_formula::parse(&n.formula) {
            // Names are workbook-level: unqualified refs don't occur; qualified ones adjust.
            let new = gridcraft_formula::adjust::adjust(expr, "\u{0}", target, e);
            n.formula = gridcraft_formula::print(&new);
        }
    }
}

// ---------------------------------------------------------------- clear

fn clear(s: &mut Session, p: &Json, what: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    edit(s, |cx| {
        for r in &ranges {
            let r = match cx.wb.sheet(sheet).and_then(|sh| sh.used_range()) {
                Some(u) => match r.intersection(&u) {
                    Some(x) => x,
                    None => continue,
                },
                None => continue,
            };
            let cells: Vec<(CellRef, Cell)> =
                cx.wb.sheet(sheet).map(|sh| sh.cells.iter_range(r).map(|(c, x)| (c, x.clone())).collect()).unwrap_or_default();
            let sh = cx.sheet_mut(sheet)?;
            for (c, mut cell) in cells {
                match what {
                    "all" => {
                        sh.remove_cell(c);
                    }
                    "contents" => {
                        cell.value = Value::Empty;
                        cell.formula = None;
                        sh.set_cell(c, cell);
                    }
                    "formats" => {
                        cell.style = StyleId::DEFAULT;
                        sh.cells.set(c, cell);
                    }
                    _ => {}
                }
            }
            if matches!(what, "all" | "comments") {
                sh.comments.retain(|c, _| !r.contains(*c));
            }
            if matches!(what, "all" | "hyperlinks") {
                sh.hyperlinks.retain(|c, _| !r.contains(*c));
            }
            if matches!(what, "all" | "formats") {
                sh.merges.retain(|m| !r.contains_range(m));
            }
            for c in r.iter().take(1_000_000) {
                cx.changed.push((sheet, c));
            }
        }
        Ok(Json::Null)
    })
}

// ---------------------------------------------------------------- fill

fn fill_dir(s: &mut Session, p: &Json, dr: i64, dc: i64) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let mut r = target_range(s, p)?;
    // Whole rows or columns fill only where the sheet has content (nothing on an empty sheet).
    if r.is_full_cols() || r.is_full_rows() {
        match s.doc()?.wb.sheet(sheet).and_then(|sh| sh.used_range()).and_then(|u| r.intersection(&u)) {
            Some(u) => r = u,
            None => return Ok(Json::Null),
        }
    }
    if r.count() > 5_000_000 {
        return Err(bad("edit.fill", "range too large"));
    }
    // With one row selected, Fill Down copies from the row above (Excel behaviour).
    if dr == 1 && r.height() == 1 {
        r.start.row = r.start.row.saturating_sub(1);
    }
    if dc == 1 && r.width() == 1 {
        r.start.col = r.start.col.saturating_sub(1);
    }
    let src = if dr == 1 {
        RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col))
    } else if dr == -1 {
        RangeRef::new(CellRef::new(r.end.row, r.start.col), r.end)
    } else if dc == 1 {
        RangeRef::new(r.start, CellRef::new(r.end.row, r.start.col))
    } else {
        RangeRef::new(CellRef::new(r.start.row, r.end.col), r.end)
    };
    edit(s, |cx| {
        crate::fill::fill(cx, sheet, src, r, crate::fill::FillMode::Copy)?;
        Ok(Json::Null)
    })
}

fn auto_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let src = str_param(p, "source").and_then(RangeRef::parse).map_or_else(|| target_range(s, p), Ok)?;
    let Some(target) = str_param(p, "target").and_then(RangeRef::parse) else { return Err(bad("edit.autoFill", "missing `target`")) };
    let mode = match str_param(p, "mode").unwrap_or("series") {
        "copy" => crate::fill::FillMode::Copy,
        "formats" => crate::fill::FillMode::Formats,
        "values" => crate::fill::FillMode::ValuesOnly,
        _ => crate::fill::FillMode::Series,
    };
    if target.count() > 5_000_000 || src.count() > 5_000_000 {
        return Err(bad("edit.autoFill", "range too large"));
    }
    edit(s, |cx| {
        crate::fill::fill(cx, sheet, src, target, mode)?;
        *cx.sel = crate::Selection { active: src.start, anchor: src.start, ranges: vec![target.union(&src)] };
        Ok(Json::Null)
    })
}

fn fill_series(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let rows = str_param(p, "direction").map(|d| d == "rows").unwrap_or(r.width() > r.height());
    let kind = str_param(p, "type").unwrap_or("linear").to_string();
    let step = f64_param(p, "step").unwrap_or(1.0);
    let stop = f64_param(p, "stop");
    let unit = str_param(p, "dateUnit").unwrap_or("day").to_string();
    edit(s, |cx| {
        crate::fill::series(cx, sheet, r, rows, &kind, step, stop, &unit)?;
        Ok(Json::Null)
    })
}

fn flash_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let n = edit(s, |cx| crate::fill::flash_fill(cx, sheet, r.start))?;
    if n == 0 {
        return Err(EngineError::Other(
            "We looked at all the data next to your selection and didn't see a pattern for filling in values for you.".into(),
        ));
    }
    Ok(json!({"filled": n}))
}

// ---------------------------------------------------------------- find & replace

fn matches(hay: &str, what: &str, case: bool, whole: bool) -> bool {
    let (h, w) = if case { (hay.to_string(), what.to_string()) } else { (hay.to_lowercase(), what.to_lowercase()) };
    let has_wild = w.contains(['*', '?']);
    if has_wild {
        let pat = if whole { w } else { format!("*{w}*") };
        return wildcard(&h, &pat);
    }
    if whole { h == w } else { h.contains(&w) }
}

/// `*`/`?` wildcard match with `~` escapes.
pub(crate) fn wildcard(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let mut p: Vec<(char, bool)> = Vec::new();
    let mut it = pat.chars();
    while let Some(c) = it.next() {
        if c == '~' {
            if let Some(n) = it.next() {
                p.push((n, true));
            }
        } else {
            p.push((c, false));
        }
    }
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == ('?', false) || (p[pi].0 == t[ti] && p[pi] != ('*', false))) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == ('*', false) {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == ('*', false) {
        pi += 1;
    }
    pi == p.len()
}

fn find(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.find", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let values = str_param(p, "lookIn") == Some("values");
    let by_cols = bool_param(p, "byColumns").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(false);
    let workbook = str_param(p, "within") == Some("workbook");
    s.last_find = Some(p.clone());
    let d = s.doc()?;
    let sheets: Vec<usize> = if workbook { (0..d.wb.sheets.len()).collect() } else { vec![d.wb.active_sheet] };
    let mut hits: Vec<(usize, CellRef, String)> = Vec::new();
    for si in sheets {
        let Some(sh) = d.wb.sheet(si) else { continue };
        let mut cells: Vec<(CellRef, String)> = sh
            .cells
            .iter()
            .filter_map(|(c, cell)| {
                let text = if values || cell.formula.is_none() || crate::display::formula_hidden(&d.wb, sh, c) {
                    crate::display::cell_text(&d.wb, sh, c)
                } else {
                    cell.input_text()
                };
                matches(&text, &what, case, whole).then_some((c, text))
            })
            .collect();
        if by_cols {
            cells.sort_by_key(|(c, _)| (c.col, c.row));
        }
        hits.extend(cells.into_iter().map(|(c, t)| (si, c, t)));
    }
    if hits.is_empty() {
        return Err(EngineError::Other(format!("We couldn't find what you were looking for: {what}")));
    }
    if all {
        let list: Vec<Json> =
            hits.iter().map(|(si, c, t)| json!({"sheet": d.wb.sheet(*si).map(|s| s.name.clone()), "cell": c.a1(), "value": t})).collect();
        return Ok(json!({"count": list.len(), "results": list}));
    }
    // Next hit after the active cell.
    let cur = (d.wb.active_sheet, d.selection.active);
    let key = |si: usize, c: CellRef| if by_cols { (si, c.col, c.row) } else { (si, c.row, c.col) };
    let next = hits.iter().find(|(si, c, _)| key(*si, *c) > key(cur.0, cur.1)).or_else(|| hits.first()).cloned();
    let Some((si, c, t)) = next else { return Err(EngineError::Other("not found".into())) };
    let dd = s.doc_mut()?;
    dd.wb_switch_sheet(si);
    dd.selection = crate::Selection::at(c);
    Ok(json!({"cell": c.a1(), "sheet": si, "value": t, "count": hits.len()}))
}

fn replace(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    let with = str_param(p, "with").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.replace", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(true);
    let workbook = str_param(p, "within") == Some("workbook");
    let active = s.doc()?.selection.active;
    let n = edit(s, |cx| {
        let sheets: Vec<usize> = if workbook { (0..cx.wb.sheets.len()).collect() } else { vec![cx.wb.active_sheet] };
        let mut n = 0;
        for si in sheets {
            let cells: Vec<(CellRef, String)> =
                cx.wb.sheet(si).map(|sh| sh.cells.iter().map(|(c, _)| (c, sh.input_text(c))).collect()).unwrap_or_default();
            for (c, text) in cells {
                if !all && c != active {
                    continue;
                }
                if !matches(&text, &what, case, whole) {
                    continue;
                }
                let new = if whole {
                    with.clone()
                } else if case {
                    text.replace(&what, &with)
                } else {
                    replace_ci(&text, &what, &with)
                };
                let cell = input_to_cell(&new, si, c, &mut cx.wb).unwrap_or_else(|_| Some(Cell::value(Value::text(new.as_str()))));
                let sh = cx.sheet_mut(si)?;
                match cell {
                    Some(cell) => sh.set_cell(c, cell),
                    None => {
                        sh.remove_cell(c);
                    }
                }
                cx.touch(si, c);
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"replaced": n}))
}

fn replace_ci(text: &str, what: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let w = what.to_lowercase();
    if lower.len() != text.len() || w.is_empty() {
        return text.replace(what, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower.get(i..).and_then(|s| s.find(&w)) {
        out.push_str(text.get(i..i + pos).unwrap_or(""));
        out.push_str(with);
        i += pos + w.len();
    }
    out.push_str(text.get(i..).unwrap_or(""));
    out
}

// ---------------------------------------------------------------- format painter

fn format_painter(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(dest) = str_param(p, "apply").and_then(RangeRef::parse) {
        let Some((wb, sheet, src, sticky)) = s.format_painter.clone() else { return Err(EngineError::Other("Pick up a format first.".into())) };
        let dest_sheet = s.doc()?.wb.active_sheet;
        let same = std::sync::Arc::ptr_eq(&wb, &s.doc()?.wb) || s.doc()?.wb.styles == wb.styles;
        edit(s, |cx| {
            let Some(src_sheet) = wb.sheet(sheet) else { return Ok(()) };
            let (h, w) = (src.height(), src.width());
            let dest = if dest.is_single() { RangeRef::new(dest.start, dest.start.offset_clamped(h as i64 - 1, w as i64 - 1)) } else { dest };
            for c in dest.iter().take(2_000_000) {
                let sc = CellRef::new(src.start.row + (c.row - dest.start.row) % h, src.start.col + (c.col - dest.start.col) % w);
                let st = src_sheet.style_id(sc);
                let st = if same { st } else { cx.wb.styles.intern(wb.styles.get(st).clone()) };
                cx.sheet_mut(dest_sheet)?.set_style(c, st);
                cx.touch(dest_sheet, c);
            }
            *cx.sel = crate::Selection::range(dest);
            Ok(())
        })?;
        if !sticky {
            s.format_painter = None;
        }
        return ok();
    }
    let sticky = bool_param(p, "sticky").unwrap_or(false);
    let d = s.doc()?;
    s.format_painter = Some((d.wb.clone(), d.wb.active_sheet, d.selection.current(), sticky));
    ok()
}

fn begin_edit(s: &mut Session, _: &Json) -> Result<Json> {
    s.ui_requests.push(crate::UiRequest::EditCell(None));
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_array_entry() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "B2", "values": [[1], [2]]})).unwrap();
        s.execute("cell.set", json!({"cell": "C4", "input": "old"})).unwrap();
        s.execute("selection.set", json!({"range": "C2:C4"})).unwrap();
        s.execute("cell.set", json!({"cell": "C2", "input": "=B2:B3*2", "array": true})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "C3")["value"], json!(4.0));
        // Past the result Excel shows #N/A; the old content of the range is replaced.
        assert_eq!(get(&mut s, "C4")["text"], "#N/A");
        let bar = s.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("C2").unwrap()).map(|c| c.bar_text());
        assert_eq!(bar.as_deref(), Some("{=B2:B3*2}"));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("hello", "h*o"));
        assert!(wildcard("hello", "h?llo"));
        assert!(!wildcard("hello", "h?lo"));
        assert!(wildcard("a*b", "a~*b"));
        assert!(!wildcard("axb", "a~*b"));
        assert!(wildcard("", "*"));
    }

    #[test]
    fn tsv() {
        assert_eq!(parse_tsv("a\tb\n1\t2\n"), vec![vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(parse_tsv("\"x\ty\"\tz"), vec![vec!["x\ty", "z"]]);
        assert_eq!(parse_tsv("\"he said \"\"hi\"\"\""), vec![vec!["he said \"hi\""]]);
    }

    #[test]
    fn paste_special_operations_keep_text_errors_and_formulas() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["'123", "Total", "=NA()", "=B3*2", 5], [1, null, null, null, null], [null, 4, null, null, null]]}),
        )
        .unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], "123");
        s.execute("edit.copy", json!({"range": "A2"})).unwrap();
        s.execute("selection.set", json!({"range": "A1:E1"})).unwrap();
        s.execute("edit.pasteSpecial", json!({"what": "values", "operation": "multiply"})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], json!(123.0));
        assert_eq!(get(&mut s, "B1")["value"], "Total");
        assert_eq!(get(&mut s, "C1")["value"]["error"], "#N/A");
        assert_eq!(get(&mut s, "D1")["formula"], "=(B3*2)*1");
        assert_eq!(get(&mut s, "D1")["value"], json!(8.0));
        assert_eq!(get(&mut s, "E1")["value"], json!(5.0));
    }

    #[test]
    fn replace_case_insensitive() {
        assert_eq!(replace_ci("Apple apple APPLE", "apple", "pear"), "pear pear pear");
    }
}
