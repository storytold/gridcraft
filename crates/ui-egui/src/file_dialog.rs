//! System file dialogs (Open, Save As, Export, Browse… for pictures) that answer on a later frame
//! (#901, #385).
//!
//! The host used to run its dialog in line, inside the frame. On macOS that is `-[NSSavePanel
//! runModal]`, a nested run loop inside winit's event handler: a trackpad gesture or a drag in it
//! runs the closures winit queued meanwhile, winit's re-entrancy panic fires inside an
//! Objective-C block, where it can't unwind, and the app aborts (winit #3992). Now the host only
//! starts the dialog (a sheet on macOS) and answers through a channel; the app keeps what the
//! answer is for and acts on it in [`SheetApp::logic`].

use serde_json::{Value as Json, json};

use crate::{Picked, SheetApp};

/// What a file dialog's answer is for.
pub(crate) enum Then {
    Open,
    /// Save As; `close` closes the workbook once it is saved (Close › Save on a new workbook).
    SaveAs {
        close: bool,
    },
    /// `file.exportPdf` with these params and the chosen path.
    ExportPdf(Json),
    /// File › Export: the command and the format it writes.
    Export(&'static str, String),
    /// Browse… in Insert Picture: the path goes into that dialog.
    Picture,
    /// Save a Copy: the command that writes the copy.
    SaveCopy(&'static str),
}

/// A file dialog that is open: its answer, what it is for and the workbook it was asked from.
pub(crate) struct Pending {
    answer: Picked,
    then: Then,
    doc: Option<u64>,
}

impl SheetApp {
    /// Whether a system file dialog is open; another one isn't asked for until it answers.
    pub(crate) fn file_dialog_open(&self) -> bool {
        self.file_dialog.is_some()
    }

    /// Asks the host's Open dialog for a workbook.
    pub(crate) fn ask_open(&mut self) {
        if self.file_dialog_open() {
            return;
        }
        if let Some(pick) = &self.services.pick_open {
            let answer = pick();
            self.wait_for(answer, Then::Open);
        }
    }

    /// Asks the host's Save dialog for a path, suggesting `suggested`.
    pub(crate) fn ask_save(&mut self, suggested: &str, then: Then) {
        if self.file_dialog_open() {
            return;
        }
        if let Some(pick) = &self.services.pick_save {
            let answer = pick(suggested);
            self.wait_for(answer, then);
        }
    }

    /// Asks the host's picture dialog for a PNG or JPEG.
    pub(crate) fn ask_picture(&mut self) {
        if self.file_dialog_open() {
            return;
        }
        if let Some(pick) = &self.services.pick_picture {
            let answer = pick();
            self.wait_for(answer, Then::Picture);
        }
    }

    /// Close › Save: a workbook that was never saved closes once the Save As it asked for has
    /// saved it. A cancelled dialog keeps it open.
    pub(crate) fn close_after_save_as(&mut self) {
        if let Some(Pending { then: Then::SaveAs { close }, .. }) = &mut self.file_dialog {
            *close = true;
        }
    }

    fn wait_for(&mut self, answer: Picked, then: Then) {
        let doc = self.session.active().map(|d| d.uid);
        self.file_dialog = Some(Pending { answer, then, doc });
    }

    /// Acts on the open file dialog's answer once it has come. A cancel (or a host that dropped
    /// the dialog) does nothing.
    pub(crate) fn poll_file_dialog(&mut self) {
        let Some(pending) = &self.file_dialog else { return };
        let path = match pending.answer.try_recv() {
            Ok(path) => path,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let Some(Pending { then, doc, .. }) = self.file_dialog.take() else { return };
        let Some(path) = path else { return };
        // A save or export is for the workbook it was asked from; the menu bar still works while
        // the dialog is up, so another workbook may be active by now.
        if !matches!(then, Then::Open | Then::Picture) && self.session.active().map(|d| d.uid) != doc {
            self.message = Some(("GridCraft".into(), "The workbook changed while the file dialog was open, so nothing was saved.".into()));
            return;
        }
        match then {
            Then::Open => self.open_path(&path),
            Then::SaveAs { close } => {
                let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.xlsx") } else { path };
                let saved = self.run("file.save", json!({"path": path}));
                self.written(saved);
                if close && self.session.active().is_some_and(|d| !d.is_dirty()) {
                    self.run_or_alert("file.close", json!({"force": true}));
                }
            }
            Then::ExportPdf(mut params) => {
                // The picker may suggest a workbook extension; the export writes a PDF.
                let path = std::path::Path::new(&path).with_extension("pdf").to_string_lossy().into_owned();
                match &mut params {
                    Json::Object(map) => {
                        map.insert("path".into(), Json::String(path));
                    }
                    other => *other = json!({"path": path}),
                }
                let exported = self.run("file.exportPdf", params);
                self.written(exported);
            }
            Then::Export(id, fmt) => {
                // The picker may suggest a workbook extension; the export writes the chosen format.
                let path = std::path::Path::new(&path).with_extension(&fmt).to_string_lossy().into_owned();
                let exported = self.run(id, json!({"path": path}));
                self.written(exported);
            }
            Then::Picture => {
                if let Some(d) = self.dialog.as_mut().filter(|d| d.name() == "insertPicture") {
                    d.values.remove("base64");
                    let alt = std::path::Path::new(&path).file_name().and_then(|name| name.to_str()).unwrap_or("");
                    d.values.insert("alt".into(), json!(alt));
                    d.values.insert("path".into(), json!(path));
                }
            }
            Then::SaveCopy(id) => self.run_or_alert(id, json!({"path": path})),
        }
    }

    /// A file written from the File tab returns to the sheet; an error is shown.
    fn written(&mut self, result: Result<Json, String>) {
        match result {
            Ok(_) => self.backstage = None,
            Err(e) => self.alert(e),
        }
    }
}
