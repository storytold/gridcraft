use super::{Json, SaveAnswer, SaveLossAnswer, SheetApp, json};

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
                // The save may ask about what it would lose: the close goes on once that is answered.
                if let Some(d) = self.dialog.as_mut().filter(|d| d.name() == "saveLoss") {
                    d.values.insert("close".into(), json!(true));
                    d.values.insert("quit".into(), json!(quitting));
                    return true;
                }
                self.close_if_saved(quitting, ctx);
            }
        }
        true
    }

    fn close_if_saved(&mut self, quitting: bool, ctx: &egui::Context) {
        if self.session.active().is_some_and(|x| !x.is_dirty()) {
            self.run_or_alert("file.close", json!({"force": true, "document": true}));
            if quitting {
                self.continue_quit(ctx);
            }
        }
    }

    /// Asks before a save that would lose something the user hasn't agreed to losing. Returns
    /// whether the save is held back for the answer. `download` is the web's Save, which hands the
    /// file to the browser.
    pub(super) fn ask_about_save_losses(&mut self, params: &Json, download: bool) -> bool {
        if self.save_losses_to_ask(params).is_empty() {
            return false;
        }
        self.open_dialog("saveLoss", json!({"params": params, "download": download}));
        true
    }

    /// What the save would lose, unless the user already agreed to exactly that for this document.
    pub(super) fn save_losses_to_ask(&self, params: &Json) -> Vec<String> {
        let losses = gridcraft_engine::cmd::file::save_losses(&self.session, params);
        let agreed = self.session.active().and_then(|d| self.accepted_losses.get(&d.uid));
        if agreed == Some(&losses) { vec![] } else { losses }
    }

    /// Answers the question [`Self::ask_about_save_losses`] asked, with the dialog's values.
    /// Returns whether it is settled: a cancelled file picker keeps it open.
    pub fn answer_save_loss(&mut self, answer: SaveLossAnswer, values: &serde_json::Map<String, Json>, ctx: &egui::Context) -> bool {
        let flag = |k: &str| values.get(k).and_then(Json::as_bool) == Some(true);
        let params = values.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
        match answer {
            SaveLossAnswer::Cancel => return true,
            SaveLossAnswer::Anyway => {
                if let Some(uid) = self.session.active().map(|d| d.uid) {
                    let losses = gridcraft_engine::cmd::file::save_losses(&self.session, &params);
                    self.accepted_losses.insert(uid, losses);
                }
                if flag("download") {
                    self.download_workbook("xlsx");
                } else {
                    self.run_or_alert("file.save", params);
                }
            }
            SaveLossAnswer::Json => {
                let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
                let name = std::path::Path::new(&title).with_extension("json").to_string_lossy().into_owned();
                if let Some(pick) = &self.services.pick_save {
                    let Some(path) = pick(&name) else { return false };
                    // The picked name is kept (the picker asked about replacing that file): another
                    // type goes by its extension, and asks again if it loses something.
                    let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.json") } else { path };
                    self.run_or_alert("file.save", json!({"path": path}));
                } else if self.services.download.is_some() {
                    self.download_workbook("json");
                } else {
                    self.message =
                        Some(("GridCraft".into(), "Saving under another name needs a file picker, which this window doesn't have.".into()));
                    return false;
                }
            }
        }
        if flag("close") {
            self.close_if_saved(flag("quit"), ctx);
        }
        true
    }

    /// Hands the workbook, encoded as `format`, to the browser.
    pub(super) fn download_workbook(&mut self, format: &str) {
        match self.run("file.saveBytes", json!({"format": format})) {
            Ok(r) => {
                if let Some(dl) = &self.services.download
                    && let Some(b) = r.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
                {
                    dl(r.get("name").and_then(Json::as_str).unwrap_or("Book.xlsx"), &b);
                }
            }
            Err(e) => self.alert(e),
        }
    }

    /// Quit (the macOS app menu, ⌘Q): commits every window's cell draft as Enter would, lets the
    /// macro step editor ask about its unsaved steps, then asks about each unsaved workbook in its
    /// window; Cancel on any of them stops the quit. A draft its
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
