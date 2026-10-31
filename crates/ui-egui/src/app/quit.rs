use super::{SaveAnswer, SheetApp, json};

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
                // Whether the workbook's own file was written. One never saved has none: Save asks
                // where to put it and the workbook stays open.
                let written = match self.run("file.save", json!({})) {
                    Ok(answer) => answer.get("bytes").is_some(),
                    // A refused cell draft already shows its own message, as in `run_or_alert`.
                    Err(e) if self.message.as_ref().is_some_and(|(_, m)| *m == e) => false,
                    Err(e) => {
                        self.alert(e);
                        false
                    }
                };
                // A text file (CSV) holds only the active sheet's values, so the workbook still
                // reads as edited after saving one (#2607). Save asked for that file, so the workbook
                // closes once it's written, unless it has sheets the file left out.
                let Some((dirty, sheets)) = self.session.active().map(|d| (d.is_dirty(), d.wb.sheets.len())) else { return true };
                if !dirty || (written && sheets == 1) {
                    self.run_or_alert("file.close", json!({"force": true, "document": true}));
                    if quitting {
                        self.continue_quit(ctx);
                    }
                } else if written {
                    let text = "This file type keeps only the active sheet, so the other sheets weren't saved. To keep them, save the workbook as an Excel Workbook (.xlsx) with Save As, or close it with Don't Save.";
                    self.message = Some(("GridCraft".into(), self.ui.language.tr(text).into()));
                }
            }
        }
        true
    }

    /// Quit (the macOS app menu, ⌘Q): commits every window's cell draft as Enter would, then asks
    /// about each unsaved workbook in its window; Cancel on any of them stops the quit. A draft its
    /// workbook refuses stops the quit too, with its window in front and the reason shown.
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        while let Some(window) = self.parked_window_with_draft() {
            self.enter_window(window);
            if self.settle_drafts().is_err() {
                return;
            }
        }
        if self.settle_drafts().is_err() {
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
