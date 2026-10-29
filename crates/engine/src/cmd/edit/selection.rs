use super::{
    CellRef, EngineError, Json, RangeRef, Result, Session, bool_param, cell_param, current_region, json, jump, split_sheet, str_param, target_ranges,
    target_sheet,
};

// ---------------------------------------------------------------- selection

/// Whether the cell may be selected: a protected sheet can forbid selecting locked and/or unlocked cells.
fn cell_selectable(wb: &gridcraft_model::Workbook, sheet: &gridcraft_model::Sheet, c: CellRef) -> bool {
    let Some(p) = sheet.protection.as_ref() else { return true };
    if wb.styles.get(sheet.style_id(c)).protection.locked { p.select_locked } else { p.select_unlocked }
}

pub(super) fn selection_set(s: &mut Session, p: &Json) -> Result<Json> {
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
    let active = cell_param(s, p, "active")?;
    let d = s.doc_mut()?;
    let active = active.or_else(|| ranges.last().map(|r| r.start)).unwrap_or_default();
    let mut sel = crate::Selection { active, anchor: ranges.last().map(|r| r.start).unwrap_or(active), ranges };
    if let Some(sh) = d.wb.active() {
        sel.expand_merges(sh);
        if !cell_selectable(&d.wb, sh, sel.active) {
            return Err(EngineError::Other("The cell you are trying to change is on a protected sheet that does not allow selecting it.".into()));
        }
    }
    d.selection = sel;
    Ok(json!({"selection": d.selection.a1(), "active": d.selection.active.a1()}))
}

impl crate::DocState {
    /// Switches the active sheet, remembering each sheet's selection.
    pub fn wb_switch_sheet(&mut self, i: usize) {
        if i >= self.wb.sheets.len() || i == self.wb.active_sheet() {
            return;
        }
        let cur = self.wb.active_sheet();
        if self.sheet_selections.len() < self.wb.sheets.len() {
            self.sheet_selections.resize(self.wb.sheets.len(), crate::Selection::default());
        }
        if let Some(slot) = self.sheet_selections.get_mut(cur) {
            *slot = self.selection.clone();
        }
        // View state: set in place, so the workbook is neither copied nor made dirty.
        self.wb.set_active_sheet(i);
        self.selection = self.sheet_selections.get(i).cloned().unwrap_or_default();
    }
}

pub(super) fn selection_move(s: &mut Session, p: &Json) -> Result<Json> {
    let dr = p.get("dr").and_then(Json::as_i64).unwrap_or(0);
    let dc = p.get("dc").and_then(Json::as_i64).unwrap_or(0);
    let extend = bool_param(p, "extend").unwrap_or(false);
    let jmp = bool_param(p, "jump").unwrap_or(false);
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let sel = &mut d.selection;
    // Extension moves the far corner, not the active cell.
    let from = if extend { sel.moving_end() } else { sel.active };
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
                t = CellRef::new(m.end.offset_clamped(dr, 0).row, from.col);
            } else if dc > 0 {
                t = CellRef::new(from.row, m.end.offset_clamped(0, dc).col);
            }
        }
        t
    };
    if let Some(m) = sheet.merge_at(to)
        && !extend
    {
        to = m.start;
    }
    if !extend && !cell_selectable(&d.wb, sheet, to) {
        // Step on to the next selectable cell in the same direction; stay put if there is none.
        let (sr, sc) = (dr.signum(), dc.signum());
        let mut t = to;
        let mut found = false;
        if !jmp && (sr, sc) != (0, 0) {
            for _ in 0..10_000 {
                let n = t.offset_clamped(sr, sc);
                if n == t {
                    break;
                }
                t = n;
                if !sheet.is_row_hidden(t.row) && !sheet.is_col_hidden(t.col) && cell_selectable(&d.wb, sheet, t) {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Ok(json!({"selection": sel.a1(), "active": sel.active.a1()}));
        }
        to = t;
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

pub(super) fn selection_next(s: &mut Session, p: &Json) -> Result<Json> {
    let fwd = bool_param(p, "forward").unwrap_or(true);
    let by_row = bool_param(p, "byRow").unwrap_or(false);
    let d = s.doc_mut()?;
    d.selection.cycle(fwd, by_row);
    Ok(json!({"active": d.selection.active.a1()}))
}

pub(super) fn select_all(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let region = current_region(sheet, d.selection.active);
    let all = RangeRef::all();
    let r = if region.is_single() || d.selection.current() == region { all } else { region };
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

pub(super) fn select_region(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let r = current_region(sheet, d.selection.active);
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

pub(super) fn select_row(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(r.start.row, 0), ranges: vec![RangeRef::rows(r.start.row, r.end.row)] };
    Ok(json!({"selection": d.selection.a1()}))
}

pub(super) fn select_col(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(0, r.start.col), ranges: vec![RangeRef::cols(r.start.col, r.end.col)] };
    Ok(json!({"selection": d.selection.a1()}))
}
