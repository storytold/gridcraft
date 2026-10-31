use super::{SheetApp, clean_error, editor, formula_bar, formula_locale, json, now_ms};

impl SheetApp {
    /// Starts editing the active cell. `text` replaces the content (typing) or pre-fills it.
    pub fn begin_edit(&mut self, text: Option<String>, from_formula_bar: bool) {
        // The source formula is display-only on spill children. Grid replacement keeps its
        // existing behavior, but no formula-bar entry path may edit that displayed formula.
        if from_formula_bar && formula_bar::active_spill_anchor(self).is_some() {
            return;
        }
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some(("Text Box".into(), clean_error(&e)));
            return;
        }
        let Some(d) = self.session.active() else { return };
        let Some(sh) = d.wb.active() else { return };
        let at = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
        // A picture has no editable scalar text. Selecting it, F2, or clicking
        // the formula bar must not turn its fallback value into a text edit.
        // Typing still starts an intentional replacement.
        if text.is_none() && sh.cell_pictures.contains_key(&at) {
            return;
        }
        let locale = self.ui.language.formula_locale();
        let current = if sh.cell_pictures.contains_key(&at) {
            String::new()
        } else {
            match sh.cell(at) {
                Some(c) if c.formula.is_some() => formula_locale::display(&c.input_text(), locale, &d.wb, d.wb.active_sheet()),
                _ => gridcraft_engine::display::edit_text(&d.wb, sh, at),
            }
        };
        let (text, replace) = match text {
            Some(t) => (t, true),
            None => (current.clone(), false),
        };
        let mut ed = editor::EditState::new(d.uid, d.wb.active_sheet(), at, text, replace, from_formula_bar);
        ed.window = self.session.active_window();
        ed.locale = locale;
        if replace && self.ui.autocomplete && !ed.is_formula() {
            ed.completion = editor::column_completion(sh, at, &ed.text);
        }
        self.editor = Some(ed);
        self.session.mode = if replace { gridcraft_engine::Mode::Enter } else { gridcraft_engine::Mode::Edit };
    }

    /// Commits the edit; `then` moves the selection (dr, dc) afterwards.
    ///
    /// The draft goes into the window and workbook it was typed in. If another window is
    /// showing, that window stays active afterwards; a draft its own workbook refuses
    /// (validation, a bad formula) brings its window back to the front with the draft still open.
    pub fn commit_edit(&mut self, dr: i64, dc: i64, array: bool, fill_selection: bool) -> bool {
        let Some((doc, window)) = self.editor.as_ref().map(|ed| (ed.doc, ed.window)) else { return true };
        if !self.draft_elsewhere() {
            return self.commit_edit_here(dr, dc, array, fill_selection);
        }
        // Its window, or (when that window closed) its workbook's most recent one.
        let Some(owner) = window.filter(|w| self.session.window(*w).is_some()).or_else(|| self.session.doc_window(doc)) else {
            self.cancel_edit();
            return true;
        };
        let back = self.session.active_window();
        self.session.activate_window(owner);
        if !self.commit_edit_here(dr, dc, array, fill_selection) {
            // Refused: its window stays in front with the draft open and the reason shown.
            return false;
        }
        if let Some(back) = back {
            self.session.activate_window(back);
        }
        true
    }

    /// The cell draft belongs to a window or workbook other than the one showing.
    pub(super) fn draft_elsewhere(&self) -> bool {
        self.editor.as_ref().is_some_and(|ed| {
            self.session.active().map(|d| d.uid) != Some(ed.doc) || ed.window.is_some_and(|w| self.session.active_window() != Some(w))
        })
    }

    /// Commits the cell and text-box drafts, as Enter would, before a command that can show
    /// another workbook or write this one to a file, so a draft never lands in another workbook.
    /// A draft its workbook refuses stays open with the reason shown, and the command doesn't run.
    pub(crate) fn settle_drafts(&mut self) -> Result<(), String> {
        if !self.commit_edit(0, 0, false, false) {
            return Err(self.message.as_ref().map(|(_, m)| m.clone()).unwrap_or_else(|| "The cell being edited can't be entered.".into()));
        }
        self.commit_text_box_edit()
    }

    fn commit_edit_here(&mut self, dr: i64, dc: i64, array: bool, fill_selection: bool) -> bool {
        let Some(ed) = self.editor.take() else { return true };
        self.session.mode = gridcraft_engine::Mode::Ready;
        let text = match &ed.completion {
            Some(full) if full.to_lowercase().starts_with(&ed.text.to_lowercase()) => full.clone(),
            _ => ed.text.clone(),
        };
        // F2 then Enter (or a file command) on an untouched draft: nothing to write.
        if !fill_selection && !array && ed.original.as_ref() == Some(&text) {
            if dr != 0 || dc != 0 {
                self.move_after_enter(dr, dc);
            }
            return true;
        }
        let text = match self.session.active().map(|d| formula_locale::canonical(&text, ed.locale, &d.wb, ed.sheet, ed.cell)) {
            Some(Ok(text)) => text,
            Some(Err(error)) => {
                self.message = Some(("GridCraft".into(), error.to_string()));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
            None => text,
        };
        // Data validation.
        if let Some(d) = self.session.active()
            && let Some((dv, msg)) = gridcraft_engine::cmd::data::check_validation(&d.wb, ed.sheet, ed.cell, &text)
        {
            if dv.error_style == gridcraft_engine::model::ErrorStyle::Stop {
                let title = if dv.error_title.is_empty() { "GridCraft".to_string() } else { dv.error_title.clone() };
                self.message = Some((title, msg));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
            self.toast = Some((msg, now_ms()));
        }
        let cmd = if fill_selection { "range.fill" } else { "cell.set" };
        let target = if fill_selection { None } else { Some(gridcraft_engine::core::RangeRef::cell(ed.cell)) };
        if let Some(why) = crate::ask_agent::held_for(self, cmd, target) {
            self.toast = Some((why, now_ms()));
            self.editor = Some(ed);
            self.session.mode = gridcraft_engine::Mode::Edit;
            return false;
        }
        // Point mode can leave another sheet active; the edit belongs to the sheet it started on.
        if self.session.active().is_some_and(|d| d.wb.active_sheet() != ed.sheet)
            && let Err(e) = self.session.run("sheet.activate", json!({"sheet": ed.sheet}))
        {
            self.message = Some(("GridCraft".into(), clean_error(&e)));
            self.editor = Some(ed);
            self.session.mode = gridcraft_engine::Mode::Edit;
            return false;
        }
        // Committing nothing into an empty cell changes nothing: no undo step (#1855, Backspace has
        // already cleared the cell as its own step).
        let unchanged = !fill_selection
            && !array
            && text.is_empty()
            && self
                .session
                .active()
                .and_then(|d| d.wb.sheet(ed.sheet))
                .is_some_and(|sh| sh.cell(ed.cell).is_none_or(|c| c.formula.is_none() && c.value == gridcraft_engine::core::Value::Empty));
        let r = if unchanged {
            Ok(json!({}))
        } else if fill_selection {
            self.session.run("range.fill", json!({"input": text}))
        } else {
            self.session.run("cell.set", json!({"cell": ed.cell.a1(), "input": text, "array": array}))
        };
        match r {
            Ok(_) => {
                if dr != 0 || dc != 0 {
                    self.move_after_enter(dr, dc);
                }
                self.after_engine();
                true
            }
            Err(e) => {
                self.message = Some(("GridCraft".into(), clean_error(&e)));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                false
            }
        }
    }

    /// Commits the edit with an arrow key. The arrow is an ordinary move, so it ends a run of Tabs
    /// instead of continuing it (Right) or going back to its column like Enter (Down), as in Excel.
    pub fn commit_edit_arrow(&mut self, dr: i64, dc: i64) -> bool {
        let run = self.tab_run.take();
        let ok = self.commit_edit(dr, dc, false, false);
        self.tab_run = if ok { None } else { run };
        ok
    }

    pub fn cancel_edit(&mut self) {
        self.editor = None;
        self.session.mode = gridcraft_engine::Mode::Ready;
    }

    /// Enter/Tab move inside a multi-cell selection, otherwise to the next cell.
    pub fn move_after_enter(&mut self, dr: i64, dc: i64) {
        let multi = self.session.active().is_some_and(|d| !d.selection.is_single_cell());
        // Any other move (arrows, clicks, another sheet) leaves the selection off the cell the
        // last Tab landed on, which ends the run.
        let here = self.session.active().map(|d| (d.uid, d.wb.active_sheet(), d.selection.active));
        let run = self.tab_run.take().filter(|&(uid, sheet, _, at)| here == Some((uid, sheet, at)));
        if multi {
            let fwd = dr > 0 || dc > 0;
            let _ = self.session.run("selection.next", json!({"forward": fwd, "byRow": dc != 0}));
        } else if let (Some((_, _, col, at)), 1, 0) = (run, dr, dc) {
            let back = i64::from(col) - i64::from(at.col);
            let _ = self.session.run("selection.move", json!({"dr": 1, "dc": back}));
        } else {
            let _ = self.session.run("selection.move", json!({"dr": dr, "dc": dc}));
            if dr == 0 && dc != 0 {
                let start = run.map(|r| r.2);
                self.tab_run = self.session.active().and_then(|d| {
                    let (uid, sheet) = (d.uid, d.wb.active_sheet());
                    let col = start.or(here.map(|h| h.2.col))?;
                    Some((uid, sheet, col, d.selection.active))
                });
            }
        }
        self.grid.ensure_visible = true;
    }
}
