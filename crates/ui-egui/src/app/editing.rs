use super::{SheetApp, clean_error, editor, formula_bar, formula_locale, json, now_ms};

/// Why a draft whose sheet was deleted while it was open is dropped.
pub(crate) const DRAFT_SHEET_GONE: &str = "The sheet of the cell being edited was deleted, so the entry was discarded.";

/// Why a draft of Python code that sheet protection hid while it was open is dropped.
pub(crate) const DRAFT_CODE_HIDDEN: &str = "Sheet protection now hides the code of the cell being edited, so the entry was discarded.";

/// Why a Ctrl+Enter fill over an area too large to fill was refused: `range.fill` refuses it too,
/// and its cells' data validation was not checked (an unchecked fill is never treated as passed).
pub(crate) const FILL_NOT_CHECKED: &str =
    "The selection is too large to fill: an area holds more than 2,000,000 cells. Nothing was entered, and its data validation was not checked.";

/// The Yes/No question a destination's Warning alert asks before a Ctrl+Enter fill.
pub(crate) const FILL_WARNING_DIALOG: &str = "fillValidationWarning";

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
        // Editing a Python cell edits its code, in Python mode; typing replaces it as in Excel.
        if text.is_none()
            && let Some(py) = sh.cell(at).and_then(|c| c.py.as_deref())
        {
            // Code hidden on a protected sheet isn't loaded into an edit, and a blank edit would
            // write over it on commit: no edit starts. Typing still replaces the cell.
            if gridcraft_engine::display::formula_hidden(&d.wb, sh, at) {
                return;
            }
            let mut ed = editor::EditState::python(d.uid, d.wb.active_sheet(), at, py.code.clone()).on_sheet(&d.wb);
            ed.py_cell = true;
            ed.window = self.session.active_window();
            self.editor = Some(ed);
            self.session.mode = gridcraft_engine::Mode::Edit;
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
        let mut ed = editor::EditState::new(d.uid, d.wb.active_sheet(), at, text, replace, from_formula_bar).on_sheet(&d.wb);
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
        // Python code goes in through `python.set`, whichever way the edit is committed (the
        // formula bar's Enter button, a click on the sheet).
        if self.editor.as_ref().is_some_and(|e| e.python) {
            let ok = self.commit_python_edit();
            if ok && (dr != 0 || dc != 0) {
                self.move_after_enter(dr, dc);
            }
            return ok;
        }
        let Some(mut ed) = self.editor.take() else { return true };
        self.session.mode = gridcraft_engine::Mode::Ready;
        if !self.follow_draft_sheet(&mut ed) {
            return true;
        }
        let text = match &ed.completion {
            // A code edit commits the code as typed, whatever a completion holds.
            Some(full) if !ed.python && full.to_lowercase().starts_with(&ed.text.to_lowercase()) => full.clone(),
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
        // Data validation of the cell typed in. A Ctrl+Enter fill asks every cell it writes instead,
        // below, once its sheet is showing.
        if !fill_selection
            && let Some(d) = self.session.active()
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
        // Ctrl+Enter: no write command applies data validation, so the rule of every cell the fill
        // writes is asked here, not only the typed-in cell's (#3670). A Stop anywhere refuses the
        // whole fill; a Warning asks Yes/No first; an Information shows and the fill goes ahead.
        if fill_selection {
            use gridcraft_engine::cmd::data::{FillValidation, check_fill_validation};
            use gridcraft_engine::model::ErrorStyle;
            let accepted = ed.fill_warning_accepted.take();
            let check = self.session.active().map(|d| check_fill_validation(&d.wb, ed.sheet, &d.selection.ranges, &text));
            let refusal = match check {
                Some(FillValidation::NotChecked) => Some(("GridCraft".to_string(), FILL_NOT_CHECKED.to_string())),
                Some(FillValidation::Alert(a)) => {
                    let title = if a.rule.error_title.is_empty() { "GridCraft".to_string() } else { a.rule.error_title.clone() };
                    match a.rule.error_style {
                        ErrorStyle::Stop => Some((title, a.message)),
                        ErrorStyle::Warning if accepted.as_deref() != Some(text.as_str()) => {
                            let ask =
                                json!({"title": title, "message": a.message, "text": text, "doc": ed.doc, "cell": ed.cell.a1(), "dr": dr, "dc": dc});
                            self.editor = Some(ed);
                            self.session.mode = gridcraft_engine::Mode::Edit;
                            self.open_dialog(FILL_WARNING_DIALOG, ask);
                            return false;
                        }
                        ErrorStyle::Warning => None,
                        ErrorStyle::Information => {
                            self.toast = Some((a.message, now_ms()));
                            None
                        }
                    }
                }
                Some(FillValidation::Passed) | None => None,
            };
            if let Some(message) = refusal {
                self.message = Some(message);
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
        }
        // Committing nothing into an empty cell changes nothing: no undo step (#1855, Backspace has
        // already cleared the cell as its own step).
        let unchanged = !fill_selection
            && !array
            && text.is_empty()
            && self.session.active().and_then(|d| d.wb.sheet(ed.sheet)).is_some_and(|sh| {
                sh.cell(ed.cell).is_none_or(|c| c.formula.is_none() && c.py.is_none() && c.value == gridcraft_engine::core::Value::Empty)
            });
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

    /// Answers the Warning alert a Ctrl+Enter fill raised ([`FILL_WARNING_DIALOG`]; `ask` is its
    /// values). No keeps the draft open and writes nothing. Yes fills the selection with the entry
    /// the question was about; it doesn't ask that question again, while a Stop is still refused.
    /// The answer applies only to the draft it was asked for: another draft (or none) is left alone.
    /// Returns whether the fill was written.
    pub fn answer_fill_warning(&mut self, yes: bool, ask: &serde_json::Map<String, serde_json::Value>) -> bool {
        let text = ask.get("text").and_then(serde_json::Value::as_str);
        let same = |ed: &editor::EditState| {
            ask.get("doc").and_then(serde_json::Value::as_u64) == Some(ed.doc)
                && ask.get("cell").and_then(serde_json::Value::as_str) == Some(ed.cell.a1().as_str())
        };
        let Some(ed) = self.editor.as_mut().filter(|ed| same(ed)) else { return false };
        let Some(text) = text.filter(|_| yes) else {
            ed.request_focus = true;
            return false;
        };
        ed.fill_warning_accepted = Some(text.to_string());
        let step = |k: &str| ask.get(k).and_then(serde_json::Value::as_i64).unwrap_or(0);
        self.commit_edit(step("dr"), step("dc"), false, true)
    }

    /// After a command, keeps the open draft on its own sheet: a sheet inserted, deleted or moved
    /// before it (an undo or a macro too) changes which sheet sits at its index. The draft is then
    /// drawn, pointed from and committed on its sheet, and the point-mode anchor, held by sheet
    /// index, is let go. A draft is dropped at once with a message when its sheet is gone, or when
    /// it holds Python code that sheet protection now hides (a draft in Python mode, or one that
    /// began on a Python cell's code): neither editor shows hidden code, as the Python editor
    /// window closes then too. A draft typed over a Python cell holds only what was typed, so it
    /// stays open.
    pub(crate) fn follow_open_draft(&mut self) {
        let Some(ed) = self.editor.as_ref() else { return };
        let Some(d) = self.session.documents().iter().find(|d| d.uid == ed.doc) else { return };
        let order = editor::sheet_order(&d.wb);
        let moved = ed.sheet_uid != 0 && order != ed.sheet_order;
        let found = if moved { ed.sheet_in(&d.wb) } else { Some(ed.sheet) };
        let hidden = found
            .and_then(|i| d.wb.sheet(i))
            .is_some_and(|sh| (ed.python || ed.py_cell) && gridcraft_engine::display::formula_hidden(&d.wb, sh, ed.cell));
        let why = match found {
            None => Some(DRAFT_SHEET_GONE),
            Some(_) if hidden => Some(DRAFT_CODE_HIDDEN),
            Some(_) => None,
        };
        if let Some(why) = why {
            self.editor = None;
            self.session.mode = gridcraft_engine::Mode::Ready;
            self.message = Some(("GridCraft".into(), why.into()));
            return;
        }
        if let (true, Some(sheet), Some(ed)) = (moved, found, self.editor.as_mut()) {
            ed.sheet = sheet;
            ed.sheet_order = order;
            ed.point_anchor = None;
        }
    }

    /// Points a draft being committed at its sheet, found by identity: commands (a sheet inserted
    /// or deleted before it, a move, an undo, a macro) change which sheet sits at its index while
    /// it is open. A draft whose sheet is gone is dropped with a message, as one whose cell
    /// protection locks is, rather than being entered on whichever sheet took its place; `false`
    /// then.
    fn follow_draft_sheet(&mut self, ed: &mut editor::EditState) -> bool {
        let Some(d) = self.session.active() else { return true };
        match ed.sheet_in(&d.wb) {
            Some(sheet) => {
                ed.sheet = sheet;
                true
            }
            None => {
                self.message = Some(("GridCraft".into(), DRAFT_SHEET_GONE.into()));
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

    /// Starts editing the active cell in Python mode, in the formula bar: its code, or no code
    /// yet for a cell that isn't a Python cell. A cell inside a spill isn't edited, as with the
    /// formula bar's other entry paths.
    pub fn begin_python_edit(&mut self) {
        if formula_bar::active_spill_anchor(self).is_some() {
            return;
        }
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some(("Text Box".into(), clean_error(&e)));
            return;
        }
        let Some(d) = self.session.active() else { return };
        let Some(sh) = d.wb.active() else { return };
        let at = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
        // A locked cell on a protected sheet is refused before a draft exists, as `begin_edit`
        // refuses it (#2945).
        if let Some(m) = self.session.locked_cell_message(d.wb.active_sheet(), at) {
            self.message = Some(("GridCraft".into(), m));
            return;
        }
        let py = sh.cell(at).and_then(|c| c.py.as_deref());
        // A Python cell's code hidden on a protected sheet stays out of view, and Python mode
        // doesn't start on it with no code, which a commit would write over the hidden code.
        if py.is_some() && gridcraft_engine::display::formula_hidden(&d.wb, sh, at) {
            return;
        }
        let code = py.map(|py| py.code.clone()).unwrap_or_default();
        // The draft belongs to this window, as `begin_edit`'s do: switching windows commits it here.
        let mut ed = editor::EditState::python(d.uid, d.wb.active_sheet(), at, code).on_sheet(&d.wb);
        ed.py_cell = py.is_some();
        ed.window = self.session.active_window();
        self.editor = Some(ed);
        self.session.mode = gridcraft_engine::Mode::Edit;
    }

    /// Ctrl+Alt+Shift+P (and the PY badge): Python mode on or off. An edit in progress keeps its
    /// text; with no edit, Python mode starts on the active cell.
    pub fn toggle_python_mode(&mut self) {
        if self.editor.as_ref().is_some_and(|ed| !ed.python && self.draft_in_spill(ed)) {
            return;
        }
        match self.editor.as_mut() {
            Some(ed) if ed.python => ed.leave_python(),
            Some(ed) => ed.enter_python(),
            None => return self.begin_python_edit(),
        }
        self.session.mode = gridcraft_engine::Mode::Edit;
    }

    /// Whether a draft's cell lies inside another cell's spill. A draft typed there stays an
    /// ordinary entry: Python mode starts on no cell inside a spill, as the formula bar's entry
    /// paths don't.
    pub(crate) fn draft_in_spill(&self, ed: &editor::EditState) -> bool {
        let doc = self.session.documents().iter().find(|d| d.uid == ed.doc);
        doc.and_then(|d| d.wb.sheet(ed.sheet)).is_some_and(|sh| sh.spill_anchor(ed.cell).is_some())
    }

    /// Why Python mode didn't start where a command asked for it: the cell lies inside another
    /// cell's spill, it is locked on a protected sheet, or its code is hidden there.
    pub(crate) fn python_mode_refused(&self) -> String {
        let in_spill = match self.editor.as_ref() {
            Some(ed) => self.draft_in_spill(ed),
            None => formula_bar::active_spill_anchor(self).is_some(),
        };
        if in_spill {
            return "Python mode doesn't start on a cell inside another cell's spill.".into();
        }
        let locked = self.session.active().and_then(|d| {
            let at = d.wb.active().and_then(|sh| sh.merge_at(d.selection.active)).map(|m| m.start).unwrap_or(d.selection.active);
            self.session.locked_cell_message(d.wb.active_sheet(), at)
        });
        locked.unwrap_or_else(|| "Python mode can't start on this cell: its code is hidden on a protected sheet.".into())
    }

    /// Commits a Python mode edit through `python.set`, on the sheet the edit started on; with no
    /// code left, a Python cell is cleared through `cell.set`. When the command refuses (code
    /// longer than a cell holds), its message shows and the edit stays open with the code as
    /// typed.
    pub fn commit_python_edit(&mut self) -> bool {
        let Some(mut ed) = self.editor.take() else { return true };
        self.session.mode = gridcraft_engine::Mode::Ready;
        if !self.follow_draft_sheet(&mut ed) {
            return true;
        }
        // The code of a Python cell committed unchanged: nothing to write (no undo step, the
        // workbook stays clean), as with F2 then Enter on any other cell.
        let code = gridcraft_engine::model::python::normalize_code(&ed.text);
        let old = self.session.active().and_then(|d| d.wb.sheet(ed.sheet)).and_then(|sh| sh.cell(ed.cell)).and_then(|c| c.py.as_deref());
        let python_cell = old.is_some();
        if old.is_some_and(|py| py.code == code) {
            return true;
        }
        // A draft on a cell sheet protection now locks can never be entered: it is dropped with
        // the warning, as `commit_edit_here` drops an ordinary one (#2945).
        if let Some(m) = self.session.locked_cell_message(ed.sheet, ed.cell) {
            self.message = Some(("GridCraft".into(), m));
            return true;
        }
        // No code at all: a Python cell is cleared through cell.set, as an empty entry clears any
        // other cell (code of spaces is still code). On another cell there is nothing to write.
        let r = match (ed.text.is_empty(), python_cell) {
            (true, false) => return true,
            (true, true) => self.session.run("cell.set", json!({"sheet": ed.sheet, "cell": ed.cell.a1(), "input": ""})),
            (false, _) => self.session.run("python.set", json!({"sheet": ed.sheet, "cell": ed.cell.a1(), "code": ed.text})),
        };
        match r {
            Ok(_) => {
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

/// Ctrl+Enter asks the data validation of every cell the fill writes, not only the typed-in one
/// (#3670).
#[cfg(test)]
mod fill_validation_tests {
    use super::*;
    use gridcraft_engine::Session;
    use gridcraft_engine::core::{CellRef, Value};

    fn book() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        SheetApp::new(session, Default::default())
    }

    /// A "whole number greater than 10" rule on `range`, with this alert style, title and message.
    fn rule(app: &mut SheetApp, range: &str, style: &str) {
        let p = json!({
            "range": range, "type": "whole", "operator": "greaterThan", "formula1": "10",
            "errorStyle": style, "errorTitle": format!("{style} title"), "errorMessage": format!("{style} at {range}"),
        });
        app.run("data.validation", p).unwrap();
    }

    fn value(app: &SheetApp, a1: &str) -> Value {
        let at = CellRef::parse(a1).unwrap();
        app.session.active().and_then(|d| d.wb.sheet(0)).map(|sh| sh.value(at)).unwrap_or_default()
    }

    fn column(app: &SheetApp, cells: &[&str]) -> Vec<Value> {
        cells.iter().map(|c| value(app, c)).collect()
    }

    fn undo_len(app: &SheetApp) -> usize {
        app.session.active().unwrap().undo.len()
    }

    /// Types `text` in the active cell of `range` and commits it with Ctrl+Enter.
    fn ctrl_enter(app: &mut SheetApp, range: &str, active: &str, text: &str) -> bool {
        app.run("selection.set", json!({"range": range, "active": active})).unwrap();
        app.begin_edit(Some(text.into()), false);
        app.commit_edit(0, 0, false, true)
    }

    fn draft(app: &SheetApp) -> Option<&str> {
        app.editor.as_ref().map(|ed| ed.text.as_str())
    }

    fn warning_values(app: &SheetApp) -> serde_json::Map<String, serde_json::Value> {
        let d = app.dialog.as_ref().expect("the Warning question is open");
        assert_eq!(d.name(), FILL_WARNING_DIALOG);
        d.values.clone()
    }

    #[test]
    fn a_stop_rule_on_another_destination_refuses_the_whole_fill() {
        // The issue's repro: A1:A3 selected, typing in A1 (no rule), a Stop rule on A2 only.
        let mut app = book();
        rule(&mut app, "A2", "stop");
        let undo = undo_len(&app);

        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "5"), "the fill is refused");

        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3], "nothing is written, not even A1");
        assert_eq!(app.message, Some(("stop title".into(), "stop at A2".into())), "A2's own alert");
        assert_eq!(draft(&app), Some("5"), "the draft stays in the cell");
        assert_eq!(undo_len(&app), undo, "no undo step");
    }

    #[test]
    fn a_warning_on_another_destination_asks_and_no_writes_nothing() {
        let mut app = book();
        rule(&mut app, "A2", "warning");
        let undo = undo_len(&app);

        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "5"), "the fill waits for the answer");
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3], "nothing written before the answer");
        let ask = warning_values(&app);
        assert_eq!(ask.get("title"), Some(&json!("warning title")));
        assert_eq!(ask.get("message"), Some(&json!("warning at A2")));
        assert_eq!(app.dialog.as_ref().map(|d| d.title.as_str()), Some("warning title"));

        assert!(!app.answer_fill_warning(false, &ask), "No writes nothing");
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3]);
        assert_eq!(draft(&app), Some("5"), "No keeps the draft");
        assert_eq!(undo_len(&app), undo);
    }

    #[test]
    fn a_warning_answered_yes_fills_the_whole_selection() {
        let mut app = book();
        rule(&mut app, "A2", "warning");
        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "5"));
        let ask = warning_values(&app);

        assert!(app.answer_fill_warning(true, &ask), "Yes writes the fill");
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Number(5.0); 3]);
        assert!(app.editor.is_none(), "the draft is committed");
    }

    #[test]
    fn a_yes_answer_does_not_carry_over_to_another_draft() {
        let mut app = book();
        rule(&mut app, "A2", "warning");
        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "5"));
        let ask = warning_values(&app);
        // The draft is cancelled and another started elsewhere before Yes arrives.
        app.cancel_edit();
        app.run("selection.set", json!({"range": "C1"})).unwrap();
        app.begin_edit(Some("other".into()), false);

        assert!(!app.answer_fill_warning(true, &ask), "the answer was for another draft");
        assert_eq!(column(&app, &["A1", "A2", "A3", "C1"]), vec![Value::Empty; 4]);
        assert_eq!(draft(&app), Some("other"));
    }

    #[test]
    fn an_information_rule_shows_its_message_and_the_fill_goes_ahead() {
        let mut app = book();
        rule(&mut app, "A3", "information");

        assert!(ctrl_enter(&mut app, "A1:A3", "A1", "5"));

        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Number(5.0); 3]);
        assert_eq!(app.toast.as_ref().map(|(m, _)| m.as_str()), Some("information at A3"));
        assert!(app.message.is_none() && app.dialog.is_none());
    }

    #[test]
    fn a_stop_wins_over_a_warning_met_first() {
        let mut app = book();
        rule(&mut app, "A2", "warning");
        rule(&mut app, "A3", "stop");

        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "5"));

        assert!(app.dialog.is_none(), "no Yes/No question when a Stop refuses anyway");
        assert_eq!(app.message, Some(("stop title".into(), "stop at A3".into())));
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3]);
    }

    #[test]
    fn a_stop_on_a_filtered_out_row_does_not_block_and_that_row_stays_untouched() {
        let mut app = book();
        let rows = json!([["Val", "Tag"], ["", "keep"], ["orig", "hide"], ["", "keep"]]);
        app.run("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        app.run("data.filter", json!({"range": "A1:B4"})).unwrap();
        app.run("data.filterBy", json!({"column": "B", "values": ["keep"]})).unwrap();
        rule(&mut app, "A3", "stop");

        assert!(ctrl_enter(&mut app, "A2:A4", "A2", "5"), "the hidden row's rule doesn't decide the fill");

        assert_eq!(column(&app, &["A2", "A3", "A4"]), vec![Value::Number(5.0), Value::text("orig"), Value::Number(5.0)]);
        assert!(app.message.is_none());

        // A Stop on a visible row of the same filtered range still refuses it.
        rule(&mut app, "A4", "stop");
        assert!(!ctrl_enter(&mut app, "A2:A4", "A2", "7"));
        assert_eq!(column(&app, &["A2", "A3", "A4"]), vec![Value::Number(5.0), Value::text("orig"), Value::Number(5.0)]);
        assert_eq!(app.message, Some(("stop title".into(), "stop at A4".into())));
    }

    #[test]
    fn a_literal_fill_is_checked_as_the_fill_leaves_the_sheet() {
        // At most one cell of A1:A3 filled: each cell alone accepts "x", the three filled don't.
        let mut app = book();
        app.run("data.validation", json!({"range": "A1:A3", "type": "custom", "formula1": "=COUNTA($A$1:$A$3)<=1"})).unwrap();

        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "x"));
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3]);
    }

    #[test]
    fn a_formula_fill_is_checked_at_each_destination() {
        let mut app = book();
        app.run("data.validation", json!({"range": "A2", "type": "custom", "formula1": "=FALSE"})).unwrap();

        assert!(!ctrl_enter(&mut app, "A1:A3", "A1", "=ROW()"), "A2's Stop refuses a formula fill too");
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Empty; 3]);
        assert_eq!(draft(&app), Some("=ROW()"));
    }

    #[test]
    fn an_area_too_large_to_check_is_refused_not_written_unchecked() {
        let mut app = book();
        rule(&mut app, "A1", "stop");
        let undo = undo_len(&app);

        assert!(!ctrl_enter(&mut app, "A1:B1000001", "A1", "50"));

        assert_eq!(app.message.as_ref().map(|(_, m)| m.as_str()), Some(FILL_NOT_CHECKED));
        assert_eq!(value(&app, "A1"), Value::Empty);
        assert_eq!(draft(&app), Some("50"));
        assert_eq!(undo_len(&app), undo);
    }

    #[test]
    fn plain_enter_still_asks_only_the_typed_in_cell() {
        // Enter (not Ctrl+Enter) writes one cell: a Stop elsewhere in the selection is not asked.
        let mut app = book();
        rule(&mut app, "A2", "stop");
        app.run("selection.set", json!({"range": "A1:A3", "active": "A1"})).unwrap();
        app.begin_edit(Some("5".into()), false);
        assert!(app.commit_edit(0, 0, false, false));
        assert_eq!(column(&app, &["A1", "A2", "A3"]), vec![Value::Number(5.0), Value::Empty, Value::Empty]);
    }
}
