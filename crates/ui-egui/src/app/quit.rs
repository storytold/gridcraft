use craft_ui::shell::embed::CloseDecision;
use gridcraft_engine::cmd::file::{SaveLosses, save_losses, xlsx_save_losses};

use super::{AfterSave, Json, PendingSave, SaveAnswer, SaveLossAnswer, SaveRequest, SheetApp, json};

impl SheetApp {
    /// The root window was asked to close (the close button, Alt+F4, the OS quitting the app): whether it closes
    /// now. A running print job holds it. With a native window per workbook the close is that window's
    /// ([`crate::doc_windows`]) unless the app is quitting. Otherwise unsaved changes hold it while they are asked
    /// about.
    pub fn close_decision(&mut self, ctx: &egui::Context) -> CloseDecision {
        if self.printing.busy() {
            return CloseDecision::Hold;
        }
        if self.allow_close {
            return CloseDecision::Close;
        }
        if self.native_windows {
            return CloseDecision::Hold;
        }
        // A cell still being edited is part of the workbook, as for File › Close.
        if !self.commit_edit(0, 0, false, false) {
            return CloseDecision::Hold;
        }
        // The macro step editor asks about its own unsaved steps and debug run first; once it has
        // closed, the window close goes on.
        if self.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") {
            crate::macro_editor::request_close(self, true);
            return CloseDecision::Hold;
        }
        if !self.session.documents().iter().any(|d| d.is_dirty()) {
            return CloseDecision::Close;
        }
        match self.dialog.as_mut() {
            // Already asking about a workbook (File › Close): answering it now goes on to quit.
            Some(d) if d.name() == "saveChanges" => {
                d.values.insert("quit".into(), json!(true));
            }
            _ => self.continue_quit(ctx),
        }
        CloseDecision::Hold
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
                let request = SaveRequest::Command { id: "file.save".into(), params: json!({}) };
                self.save_then(request, AfterSave { close: true, quit: quitting }, ctx);
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

    /// Runs a save and then `after`. When the save (or the Save As it leads to) has to ask what it
    /// may lose, the question takes `after` along and the answer goes on with it.
    fn save_then(&mut self, request: SaveRequest, after: AfterSave, ctx: &egui::Context) {
        self.after_save = Some(after);
        match request {
            SaveRequest::Command { id, params } => self.run_or_alert(&id, params),
            SaveRequest::Download => {
                if !self.hold_save(SaveRequest::Download) {
                    self.download_workbook("xlsx");
                }
            }
        }
        if let Some(after) = self.after_save.take()
            && after.close
        {
            self.close_if_saved(after.quit, ctx);
        }
    }

    /// Holds back a save that would lose something the user hasn't agreed to losing, and asks.
    /// Returns whether it was held: the answer runs it ([`Self::answer_save_loss`]).
    pub(super) fn hold_save(&mut self, request: SaveRequest) -> bool {
        let losses = self.save_losses_to_ask(&request);
        let Some(doc) = self.session.active().map(|d| d.uid).filter(|_| !losses.is_empty()) else { return false };
        let destination = self.save_destination(&request);
        let pending = PendingSave { doc, request, destination, losses, after: self.after_save.take().unwrap_or_default() };
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some(("Text Box".into(), super::clean_error(&e)));
        } else {
            self.replace_dialog(|_| Some(crate::dialogs::Dialog::save_loss(pending)));
        }
        true
    }

    /// The file a held save writes when it names none: the active workbook's own file.
    fn save_destination(&self, request: &SaveRequest) -> Option<String> {
        match request {
            SaveRequest::Command { id, params } if id == "file.save" && params.get("path").is_none() => {
                self.session.active().and_then(|d| d.path.clone())
            }
            _ => None,
        }
    }

    /// What the save would lose, unless the user already agreed to exactly that for this document.
    pub(super) fn save_losses_to_ask(&self, request: &SaveRequest) -> SaveLosses {
        let losses = match request {
            SaveRequest::Command { id, params } => save_losses(&self.session, id, params),
            SaveRequest::Download => xlsx_save_losses(&self.session),
        };
        let agreed = self.session.active().and_then(|d| self.accepted_losses.get(&d.uid));
        if agreed == Some(&losses) { SaveLosses::default() } else { losses }
    }

    /// Answers the question [`Self::hold_save`] asked. Returns whether it is settled: a cancelled
    /// file picker keeps it open.
    pub(crate) fn answer_save_loss(&mut self, answer: SaveLossAnswer, pending: PendingSave, ctx: &egui::Context) -> bool {
        if answer == SaveLossAnswer::Cancel {
            return true;
        }
        // The question stays in its window, so its workbook is the active one; should another be,
        // nothing is saved that the user wasn't asked about.
        if self.session.active().map(|d| d.uid) != Some(pending.doc) {
            self.message = Some(("GridCraft".into(), "The workbook this question was about is no longer in front, so nothing was saved.".into()));
            return true;
        }
        match answer {
            SaveLossAnswer::Cancel => {}
            // The workbook's file changed while the question was open (a Save As through
            // automation): the save is asked about again for the file it would write now.
            SaveLossAnswer::Anyway if self.save_destination(&pending.request) != pending.destination => {
                self.save_then(pending.request, pending.after, ctx);
            }
            SaveLossAnswer::Anyway => {
                self.accepted_losses.insert(pending.doc, pending.losses);
                self.save_then(pending.request, pending.after, ctx);
            }
            SaveLossAnswer::Json => {
                let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
                let name = std::path::Path::new(&title).with_extension("json").to_string_lossy().into_owned();
                if let Some(pick) = &self.services.pick_save {
                    let Some(path) = pick(&name) else { return false };
                    // The picked name is kept (the picker asked about replacing that file): another
                    // type goes by its extension, and asks again if it loses something.
                    let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.json") } else { path };
                    self.save_then(SaveRequest::Command { id: "file.save".into(), params: json!({"path": path}) }, pending.after, ctx);
                } else if self.services.download.is_some() {
                    self.download_workbook("json");
                    if pending.after.close {
                        self.close_if_saved(pending.after.quit, ctx);
                    }
                } else {
                    self.message =
                        Some(("GridCraft".into(), "Saving under another name needs a file picker, which this window doesn't have.".into()));
                    return false;
                }
            }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Save Anyway answers for the workbook that asked: with another one active, nothing is
    /// written and nothing is agreed to for it.
    #[test]
    fn a_save_loss_answer_never_acts_on_another_workbook() {
        let dir = std::env::temp_dir().join(format!("gridcraft-loss-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = |n: &str| dir.join(n).to_string_lossy().into_owned();
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        app.run("file.save", json!({"path": at("First.xlsx")})).unwrap();
        app.run("pageLayout.pageSetup", json!({"excluded": true})).unwrap();
        let first = app.session.active().unwrap().uid;
        app.run_or_alert("file.save", json!({}));
        let pending = app.dialog.as_ref().and_then(|d| d.pending_save()).unwrap();
        assert_eq!(pending.doc, first);

        app.run("file.new", json!({})).unwrap();
        app.run("file.save", json!({"path": at("Second.xlsx")})).unwrap();
        app.run("pageLayout.pageSetup", json!({"excluded": true})).unwrap();
        let (second, before) = (app.session.active().unwrap().uid, std::fs::read(at("Second.xlsx")).unwrap());
        assert!(app.answer_save_loss(SaveLossAnswer::Anyway, pending, &egui::Context::default()));
        assert_eq!(std::fs::read(at("Second.xlsx")).unwrap(), before);
        assert!(app.session.active().is_some_and(|d| d.uid == second && d.is_dirty()));
        assert!(!app.accepted_losses.contains_key(&second) && app.message.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// A held Save is about the workbook's file when it asked: should that file change before the
    /// answer, Save Anyway writes neither file and asks again about the new one.
    #[test]
    fn save_anyway_asks_again_when_the_workbooks_file_changed() {
        let dir = std::env::temp_dir().join(format!("gridcraft-loss-moved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = |n: &str| dir.join(n).to_string_lossy().into_owned();
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        app.run("file.save", json!({"path": at("First.xlsx")})).unwrap();
        app.run("pageLayout.pageSetup", json!({"excluded": true})).unwrap();
        app.run_or_alert("file.save", json!({}));
        let pending = app.dialog.as_ref().and_then(|d| d.pending_save()).unwrap();

        // While the question is open, automation gives the same workbook another file and changes it.
        app.run("file.saveAs", json!({"path": at("Moved.xlsx")})).unwrap();
        app.run("cell.set", json!({"cell": "A1", "input": "changed"})).unwrap();
        let (first, moved) = (std::fs::read(at("First.xlsx")).unwrap(), std::fs::read(at("Moved.xlsx")).unwrap());
        assert!(app.answer_save_loss(SaveLossAnswer::Anyway, pending, &egui::Context::default()));
        assert_eq!(std::fs::read(at("First.xlsx")).unwrap(), first);
        assert_eq!(std::fs::read(at("Moved.xlsx")).unwrap(), moved);
        assert!(app.session.active().is_some_and(|d| d.is_dirty()));
        let again = app.dialog.as_ref().and_then(|d| d.pending_save()).unwrap();
        assert_eq!(again.destination.as_deref(), Some(at("Moved.xlsx").as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
