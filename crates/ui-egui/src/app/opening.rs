//! Opening a file on another thread, with a progress card: reading and calculating a large
//! workbook must not leave the window unresponsive. The workbook joins the session only once it
//! is ready, so cancelling leaves everything as it was.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use gridcraft_engine::cmd::file::{OpenProgress, PreparedOpen, prepare_open};

use super::{SheetApp, clean_error, json};

/// The stack `Calc` runs on: as on the main thread, deep formula chains are bounded by its budget.
const WORKER_STACK: usize = 8 << 20;

pub(crate) struct Opening {
    path: String,
    progress: Arc<OpenProgress>,
    result: Receiver<Result<PreparedOpen, String>>,
}

impl SheetApp {
    /// Starts opening `path` on another thread, or queues it behind the one being opened. `false`
    /// leaves it to the synchronous path: background opening is off, the file is already open
    /// (which only switches to it), or no thread could be started.
    pub(super) fn open_in_background(&mut self, path: &str) -> bool {
        if !self.background_open || self.session.documents().iter().any(|d| d.path.as_deref() == Some(path)) {
            return false;
        }
        // As `file.open` does: what is being typed belongs to the workbook showing, which the file
        // may replace if it looks untouched. A draft that can't be entered stays, with the reason.
        if let Err(e) = self.settle_drafts() {
            self.message.get_or_insert_with(|| ("GridCraft".into(), clean_error(&e)));
            return true;
        }
        if self.opening.is_some() {
            self.open_queue.push_back(path.to_string());
            return true;
        }
        let (send, result) = channel();
        let progress = Arc::new(OpenProgress::default());
        let (worker_progress, params) = (Arc::clone(&progress), json!({"path": path}));
        let started = std::thread::Builder::new().name("gridcraft-open".into()).stack_size(WORKER_STACK).spawn(move || {
            // No one listening any more means the opening was cancelled.
            let _ = send.send(prepare_open(&params, &worker_progress).map_err(|e| e.to_string()));
        });
        if started.is_err() {
            return false;
        }
        self.opening = Some(Opening { path: path.to_string(), progress, result });
        true
    }

    /// Takes in a finished opening, and starts the next one asked for.
    pub(super) fn poll_opening(&mut self, ctx: &egui::Context) {
        let Some(opening) = &self.opening else { return };
        let outcome = match opening.result.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => {
                // Nothing else wakes the window while the other thread works.
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("We can't open this file: reading it stopped unexpectedly.".to_string()),
        };
        let path = opening.path.clone();
        self.opening = None;
        let opened = outcome.and_then(|prepared| {
            self.session.offer_prepared_open(path.clone(), prepared);
            let opened = self.run("file.open", json!({"path": path}));
            self.session.discard_prepared_open();
            opened
        });
        match opened {
            Ok(r) => self.after_open(Some(&path), &r),
            // A password-protected workbook: ask for the password; the dialog opens it.
            Err(e) if e.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => self.open_dialog("openPassword", json!({"path": path})),
            Err(e) => self.message = Some(("GridCraft".into(), clean_error(&e))),
        }
        self.after_engine();
        ctx.request_repaint();
        while let Some(next) = self.open_queue.pop_front() {
            self.open_path(&next);
            if self.opening.is_some() {
                break;
            }
        }
    }

    /// Cancel on the progress card: the file being opened and those queued behind it stay closed.
    pub fn cancel_opening(&mut self) {
        if let Some(opening) = self.opening.take() {
            opening.progress.cancel();
        }
        self.open_queue.clear();
    }
}

/// The card over the window while a file opens: its name, what is happening and Cancel.
pub(super) fn progress_card(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(opening) = &app.opening else { return };
    let lang = app.ui.language;
    let name = std::path::Path::new(&opening.path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(&opening.path).to_owned();
    let step = if opening.progress.is_calculating() { "Calculating the formulas…" } else { "Reading the file…" };
    let mut cancel = false;
    egui::Modal::new(egui::Id::unique("gridcraft.opening")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(egui::RichText::new(lang.tr("Opening {name}").replace("{name}", &name)).strong());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(lang.tr(step));
        });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            cancel = ui.button(lang.tr("Cancel")).clicked();
        });
    });
    if cancel {
        app.cancel_opening();
    }
}
