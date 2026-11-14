use super::{SaveAnswer, SheetApp, json};
use serde_json::Value as Json;

/// What tells one cell draft from another: its workbook, window, sheet and cell.
pub(crate) type DraftId = (u64, Option<u64>, u64, gridcraft_engine::core::CellRef);

fn draft_id(ed: &crate::editor::EditState) -> DraftId {
    (ed.doc, ed.window, ed.sheet_uid, ed.cell)
}

const NO_DRAFT: &str = "No cell is being edited.";
const NO_WINDOW: &str = "That window is closed, or isn't the one editing the cell.";

impl SheetApp {
    /// Holds back a window close (the close button, Alt+F4, the OS quitting the app) while a
    /// workbook has unsaved changes, and asks about them first.
    pub(super) fn guard_window_close(&mut self, ctx: &egui::Context) {
        if self.native_windows || self.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        // A cell still being edited is part of the workbook, as for File › Close.
        if !self.commit_edit(0, 0, false, false) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ask_about_refused_draft(None);
            return;
        }
        // The macro step editor asks about its own unsaved steps and debug run first; once it has
        // closed, the window close goes on.
        if self.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::macro_editor::request_close(self, true);
            return;
        }
        if !self.session.documents().iter().any(|d| d.is_dirty()) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        match self.dialog.as_mut() {
            // Already asking about a workbook (File › Close): answering it now goes on to quit.
            Some(d) if d.name() == "saveChanges" => {
                d.values.insert("quit".into(), json!(true));
            }
            _ => self.continue_quit(ctx),
        }
    }

    /// The cell being edited can't be entered (a formula with an error, a validation alert) and it
    /// holds up closing: the reason goes into a question instead, so a bad draft never keeps the
    /// app from closing. `window` is the native window being closed; `None` is quitting.
    pub(crate) fn ask_about_refused_draft(&mut self, window: Option<u64>) {
        if self.editor.is_none() {
            return;
        }
        let reason = self.message.take().map(|(_, reason)| reason).unwrap_or_default();
        self.open_dialog("refusedDraft", json!({"reason": reason, "window": window}));
    }

    /// `ui.editRefusedDraft`: the question closes and the draft stays open to fix.
    pub(super) fn edit_refused_draft(&mut self) -> Result<Json, String> {
        let Some(ed) = self.editor.as_mut() else { return Err(NO_DRAFT.into()) };
        ed.request_focus = true;
        // The later answer wins over an Exit Anyway still waiting for its frame.
        self.pending_exit_anyway = None;
        if self.dialog.as_ref().is_some_and(|d| d.name() == "refusedDraft") {
            self.dialog = None;
        }
        Ok(Json::Null)
    }

    /// `ui.exitAnyway`: answers the question for its window (`window` when it isn't open, as while
    /// its own button is drawn). Closing needs the frame's context, so the next `logic` goes on.
    pub(super) fn request_exit_anyway(&mut self, p: &Json) -> Result<Json, String> {
        let Some(draft) = self.editor.as_ref().map(draft_id) else { return Err(NO_DRAFT.into()) };
        // An open question knows what it is closing, and no window there means quitting.
        let window = match self.dialog.as_ref().filter(|d| d.name() == "refusedDraft") {
            Some(question) => question.values.get("window").and_then(Json::as_u64),
            None => p.get("window").and_then(Json::as_u64),
        };
        if !self.can_exit_anyway(window) {
            return Err(NO_WINDOW.into());
        }
        if self.dialog.as_ref().is_some_and(|d| d.name() == "refusedDraft") {
            self.dialog = None;
        }
        self.pending_exit_anyway = Some((window, draft));
        Ok(Json::Null)
    }

    /// Whether Exit Anyway may drop the draft: its target window is still open and holds that draft.
    fn can_exit_anyway(&self, window: Option<u64>) -> bool {
        window.is_none_or(|window| {
            self.session.window(window).is_some() && self.editor.as_ref().is_none_or(|ed| ed.window.is_none_or(|owner| owner == window))
        })
    }

    /// Goes on with an Exit Anyway asked since the last frame.
    pub(super) fn run_pending_exit_anyway(&mut self, ctx: &egui::Context) {
        let Some((window, draft)) = self.pending_exit_anyway.take() else { return };
        // A window switch since may have brought another window's draft forward: that one stays.
        if self.editor.as_ref().map(draft_id) == Some(draft) {
            self.exit_anyway(window, ctx);
        }
    }

    /// Exit Anyway: drops the draft, leaving its cell as it was, and goes on closing. Unsaved
    /// workbooks are still asked about.
    pub(crate) fn exit_anyway(&mut self, window: Option<u64>, ctx: &egui::Context) {
        // Asked a frame ago: the window may have closed since.
        if !self.can_exit_anyway(window) {
            return;
        }
        self.cancel_edit();
        match window {
            Some(window) => self.run_or_alert("window.close", json!({"window": window})),
            None => self.request_quit(ctx),
        }
    }

    /// Asks about the next unsaved workbook before quitting, in its window, or closes the app
    /// once none is left.
    pub(crate) fn continue_quit(&mut self, ctx: &egui::Context) {
        let dirty = self.session.documents().iter().find(|d| d.is_dirty()).map(|d| d.uid);
        match dirty.and_then(|uid| self.session.doc_window(uid)) {
            Some(window) => {
                // Its window comes forward (the native window too: see `doc_windows`).
                self.enter_window(window);
                let title = self.session.active().map(|d| d.display_title()).unwrap_or_default();
                self.open_dialog("saveChanges", json!({"title": title, "quit": true}));
                self.bring_forward = true;
            }
            None => {
                self.allow_close = true;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
        }
    }

    /// Answers the Save Changes question about the active workbook (its buttons). While quitting,
    /// the next unsaved workbook is asked about once this one is answered; Cancel stops the quit.
    /// Returns whether the question is settled: Save keeps it open when the cell being edited
    /// can't be entered.
    pub fn answer_save_changes(&mut self, answer: SaveAnswer, quitting: bool, ctx: &egui::Context) -> bool {
        match answer {
            SaveAnswer::Cancel => {}
            SaveAnswer::DontSave => {
                self.run_or_alert("file.close", json!({"force": true, "document": true}));
                if quitting {
                    self.continue_quit(ctx);
                }
            }
            SaveAnswer::Save => {
                // File operations include the cell still being edited, as File › Save does.
                if !self.commit_edit(0, 0, false, false) {
                    return false;
                }
                self.run_or_alert("file.save", json!({}));
                if self.session.active().is_some_and(|x| !x.is_dirty()) {
                    self.run_or_alert("file.close", json!({"force": true, "document": true}));
                    if quitting {
                        self.continue_quit(ctx);
                    }
                }
            }
        }
        true
    }

    /// Quit (the macOS app menu, ⌘Q): commits every window's cell draft as Enter would, lets the
    /// macro step editor ask about its unsaved steps, then asks about each unsaved workbook in its
    /// window; Cancel on any of them stops the quit. A draft its workbook refuses brings its window
    /// to the front and asks whether to edit it or exit anyway.
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        while let Some(window) = self.parked_window_with_draft() {
            self.enter_window(window);
            if self.settle_drafts().is_err() {
                self.ask_about_refused_draft(None);
                return;
            }
        }
        if self.settle_drafts().is_err() {
            self.ask_about_refused_draft(None);
            return;
        }
        // The macro step editor asks about its own unsaved steps and debug run first, as for the
        // window's close button, in the window it is shown in; once it has closed, the quit goes
        // on, and Cancel stops it.
        if let Some(window) = self.window_with_dialog("macroEditor") {
            if self.ui_window() != Some(window) {
                self.enter_window(window);
            }
            crate::macro_editor::request_close(self, true);
            return;
        }
        if let Some(d) = self.dialog.as_mut().filter(|d| d.name() == "saveChanges") {
            // Already asking about a workbook (File › Close): answering it goes on to quit.
            d.values.insert("quit".into(), json!(true));
            return;
        }
        self.continue_quit(ctx);
    }
}
