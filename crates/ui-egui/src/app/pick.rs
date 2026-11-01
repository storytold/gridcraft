//! File pickers that answer at once (Windows, Linux) or on a later frame (macOS sheets, #1911,
//! #2042, #2906): every Open, Save As, Export and Save a Copy asks through [`SheetApp::pick_path`]
//! and runs its follow-up ([`AfterPick`]) when the path arrives.

use super::{Json, SheetApp, clean_error, json};
use crate::PathPick;

/// What a picked path is for, run once the picker answers ([`SheetApp::pick_path`]).
pub(crate) enum AfterPick {
    Open,
    SaveAs,
    /// Save As for the Save Changes question's Save button on a workbook that has no file yet:
    /// once saved, the workbook closes, and while quitting the next unsaved one is asked about.
    SaveAsAndClose {
        quitting: bool,
    },
    /// `file.exportPdf` with the caller's other params (range, sheets, fitToPage).
    ExportPdf(Json),
    /// A Backstage export: the command and the extension it writes.
    Export(&'static str, String),
    /// Save a Copy, in this format.
    SaveCopy(String),
}

/// An asynchronous picker still open: what its answer is for, the workbook that asked (by uid)
/// and where the answer comes.
pub(crate) type PendingPick = (AfterPick, Option<u64>, std::sync::mpsc::Receiver<Option<String>>);

impl SheetApp {
    /// Asks for a path and runs `then` with it. A blocking picker answers at once and this returns
    /// whether `then` wrote or opened something; the asynchronous one (macOS) answers on a later
    /// frame and this returns `None`. While one is open, another request is refused (`Some(false)`).
    pub(crate) fn pick_path(&mut self, pick: PathPick, then: AfterPick) -> Option<bool> {
        if self.pending_pick.is_some() {
            return Some(false);
        }
        if let Some(start) = &self.services.pick_async {
            let (answer, rx) = std::sync::mpsc::channel();
            start(pick, answer);
            self.pending_pick = Some((then, self.session.active().map(|d| d.uid), rx));
            return None;
        }
        let path = match &pick {
            PathPick::Open => self.services.pick_open.as_ref().and_then(|p| p()),
            PathPick::Save(suggested) => self.services.pick_save.as_ref().and_then(|p| p(suggested)),
        };
        Some(self.after_pick(then, path))
    }

    /// Whether an asynchronous picker is open and its answer still to come.
    pub(crate) fn picking(&self) -> bool {
        self.pending_pick.is_some()
    }

    /// Hands the asynchronous picker's answer to what asked for it, once it has arrived.
    pub(super) fn poll_pick(&mut self, ctx: &egui::Context) {
        let Some((_, _, rx)) = &self.pending_pick else { return };
        match rx.try_recv() {
            Ok(path) => {
                if let Some((then, uid, _)) = self.pending_pick.take() {
                    let quitting = matches!(then, AfterPick::SaveAsAndClose { quitting: true });
                    // Saved and closed: go on to the next unsaved workbook. Cancelled: the quit stops.
                    if self.after_pick_for(uid, then, path) && quitting {
                        self.continue_quit(ctx);
                    }
                }
            }
            // The host gave up on the picker without answering: nothing to do.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending_pick = None,
            // Nothing wakes the frame loop when the answer comes, so keep looking for it.
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }

    /// [`Self::after_pick`] for the workbook that asked: the frame loop kept running while the
    /// picker was up, so another workbook may be active now (the control channel can switch or
    /// close one). Saves and exports go to the one that asked, and the active one stays active.
    fn after_pick_for(&mut self, uid: Option<u64>, then: AfterPick, path: Option<String>) -> bool {
        let active = self.session.active().map(|d| d.uid);
        if matches!(then, AfterPick::Open) || path.is_none() || active == uid {
            return self.after_pick(then, path);
        }
        let Some(asked) = self.session.documents().iter().position(|d| Some(d.uid) == uid) else {
            self.message = Some(("GridCraft".into(), "The workbook was closed before a file was chosen, so nothing was saved.".into()));
            return false;
        };
        // A text-box draft belongs to the active workbook: commit it before the switch, as
        // window.activate does, or the save refuses it as off-sheet.
        if let Err(e) = self.commit_text_box_edit() {
            self.alert(e);
            return false;
        }
        self.session.set_active(asked);
        let done = self.after_pick(then, path);
        if let Some(i) = self.session.documents().iter().position(|d| Some(d.uid) == active) {
            self.session.set_active(i);
        }
        done
    }

    /// Runs what a path was picked for. `false` when the picker was cancelled or the command failed.
    pub(crate) fn after_pick(&mut self, then: AfterPick, path: Option<String>) -> bool {
        let Some(path) = path else { return false };
        match then {
            AfterPick::Open => {
                self.open_path(&path);
                true
            }
            AfterPick::SaveAs => {
                self.save_to(path);
                true
            }
            AfterPick::SaveAsAndClose { .. } => {
                self.save_to(path);
                if self.session.active().is_some_and(|d| d.is_dirty()) {
                    return false;
                }
                self.run_or_alert("file.close", json!({"force": true, "document": true}));
                true
            }
            AfterPick::ExportPdf(mut params) => {
                // The picker may suggest a workbook extension; the export writes a PDF.
                let path = std::path::Path::new(&path).with_extension("pdf").to_string_lossy().into_owned();
                match &mut params {
                    Json::Object(map) => {
                        map.insert("path".into(), Json::String(path));
                    }
                    other => *other = json!({"path": path}),
                }
                match self.run("file.exportPdf", params) {
                    Ok(_) => true,
                    Err(e) => {
                        self.alert(e);
                        false
                    }
                }
            }
            AfterPick::Export(id, fmt) => {
                // The picker may suggest a workbook extension; the export writes the chosen format.
                let path = std::path::Path::new(&path).with_extension(&fmt).to_string_lossy().into_owned();
                match self.run(id, json!({"path": path})) {
                    Ok(_) => true,
                    Err(e) => {
                        self.message = Some(("GridCraft".into(), clean_error(&e)));
                        false
                    }
                }
            }
            AfterPick::SaveCopy(fmt) => {
                let id = match fmt.as_str() {
                    "csv" => "file.exportCsv",
                    "html" => "file.exportHtml",
                    _ => "file.saveAs",
                };
                self.run_or_alert(id, json!({"path": path}));
                true
            }
        }
    }

    /// Save As to `path` (an `.xlsx` when the picker gave no extension).
    fn save_to(&mut self, path: String) {
        let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.xlsx") } else { path };
        self.run_or_alert("file.save", json!({"path": path}));
    }
}
