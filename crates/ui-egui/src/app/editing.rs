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
        // A locked cell on a protected sheet can't be entered (Excel's warning, and the engine
        // refuses `cell.set` anyway): refuse before a draft exists, rather than leaving one that
        // can never be committed and blocks Save (#2945).
        if let Some(m) = self.session.locked_cell_message(d.wb.active_sheet(), at) {
            self.message = Some(("GridCraft".into(), m));
            return;
        }
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
        // A draft on a cell sheet protection now locks (the sheet was protected while it was
        // open) can't be entered: drop it with the warning instead of keeping it, which would
        // block Save with a draft that can never be committed (#2945).
        if let Some(m) = self.session.locked_cell_message(ed.sheet, ed.cell) {
            self.message = Some(("GridCraft".into(), m));
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

#[cfg(test)]
mod protected_cell_tests {
    use super::*;
    use gridcraft_engine::Session;
    use gridcraft_engine::core::{CellRef, Value};

    /// A protected sheet whose C2 is unlocked; C2 holds "keep" as the last undo step.
    fn protected_book() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.run("cell.set", json!({"cell": "C2", "input": "keep"})).unwrap();
        app.run("home.formatCells", json!({"range": "C2", "style": {"protection": {"locked": false}}})).unwrap();
        app.run("review.protectSheet", json!({})).unwrap();
        app
    }

    fn value(app: &SheetApp, a1: &str) -> Value {
        let at = CellRef::parse(a1).unwrap();
        app.session.active().and_then(|d| d.wb.sheet(0)).map(|sh| sh.value(at)).unwrap_or_default()
    }

    fn protected_message(app: &SheetApp) -> Option<&str> {
        app.message.as_ref().map(|(_, m)| m.as_str())
    }

    #[test]
    fn typing_in_a_locked_cell_does_not_leave_a_draft_that_blocks_undo_and_save() {
        let mut app = protected_book();
        assert_eq!(app.session.active().unwrap().selection.active.a1(), "A1", "the locked A1 is the active cell");
        let undo = app.session.active().unwrap().undo.len();
        let dirty = app.session.active().unwrap().is_dirty();

        app.begin_edit(Some("locked".into()), false);

        assert!(app.editor.is_none(), "a locked cell must not open an editor");
        assert!(protected_message(&app).is_some_and(|m| m.contains("protected sheet")), "the warning names the protected sheet");
        assert_eq!(app.session.active().unwrap().undo.len(), undo, "a refused edit adds no undo step");
        assert_eq!(app.session.active().unwrap().is_dirty(), dirty, "a refused edit leaves the workbook as it was");
        assert!(app.run("file.saveBytes", json!({"format": "xlsx"})).is_ok(), "Save is not blocked by a draft that can never be entered");
    }

    #[test]
    fn an_unlocked_cell_on_a_protected_sheet_still_edits_undoes_and_saves() {
        let mut app = protected_book();
        app.run("selection.set", json!({"cell": "C2"})).unwrap();
        app.begin_edit(Some("typed".into()), false);
        assert!(app.editor.is_some(), "an unlocked cell opens its editor");
        assert!(app.commit_edit(0, 0, false, false));
        assert_eq!(value(&app, "C2"), Value::text("typed"));

        assert!(app.run("edit.undo", json!({})).is_ok());
        assert_eq!(value(&app, "C2"), Value::text("keep"), "Undo of the typed edit still works");
        assert!(app.run("file.saveBytes", json!({"format": "xlsx"})).is_ok());
    }

    #[test]
    fn a_draft_that_becomes_locked_is_cancelled_instead_of_staying_open() {
        let mut app = protected_book();
        app.run("selection.set", json!({"cell": "C2"})).unwrap();
        app.begin_edit(Some("typed".into()), false);
        // The sheet is unprotected, C2 locked, and the sheet protected again while the draft is open.
        app.run("review.unprotectSheet", json!({})).unwrap();
        app.run("home.formatCells", json!({"range": "C2", "style": {"protection": {"locked": true}}})).unwrap();
        app.run("review.protectSheet", json!({})).unwrap();

        assert!(app.commit_edit(0, 0, false, false), "the refused draft is handled, not left open");
        assert!(app.editor.is_none());
        assert!(protected_message(&app).is_some_and(|m| m.contains("protected sheet")));
        assert_eq!(value(&app, "C2"), Value::text("keep"));
        assert!(app.run("file.saveBytes", json!({"format": "xlsx"})).is_ok());
    }
}
