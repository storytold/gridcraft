//! Draw › Action Pen: handwriting to text.
//!
//! Two things live here. [`start`] and [`poll`] wire the host's model directory to the engine's
//! [`gridcraft_ink::Recognizer`]: at startup when the files are already there, and after a
//! download or an import. [`show`] is the model panel the Draw tab's Action Pen button opens
//! when the model is absent: what the model is, its licence and size, where its files go, a
//! Download button (after an explicit click, on a worker) and a hand-import picker.
//!
//! Nothing here runs at startup beyond a `stat` per model file: no network call, no model read
//! until a load is asked for (`docs/action-pen.md`).

use std::sync::mpsc::Receiver;

use gridcraft_ink::TROCR_SMALL_HANDWRITTEN;
use serde_json::json;

use crate::SheetApp;

/// What a worker thread finished: a model download, or loading the recognizer.
pub enum Job {
    Downloaded(Result<(), String>),
    Loaded(Result<std::sync::Arc<dyn gridcraft_ink::Recognizer>, String>),
}

/// Starts the Action Pen if the host has a model directory: sets `Session::models_dir` and, when
/// the model's files are present, loads the recognizer on a worker thread.
pub fn start(app: &mut SheetApp) {
    let Some(host) = app.services.models.clone() else {
        app.session.ink = None;
        return;
    };
    let root = host.models_dir();
    app.session.models_dir = Some(root.display().to_string());
    if app.session.ink.is_none() && TROCR_SMALL_HANDWRITTEN.check_sizes(&root).is_installed() {
        load(app);
    }
}

/// Loads the recognizer from the configured model directory on a worker thread, replacing any
/// recognizer already there when it succeeds.
fn load(app: &mut SheetApp) {
    let (Some(loader), Some(root)) = (app.services.ink_loader.clone(), app.session.models_dir.clone()) else { return };
    if app.grid.ink_job.is_some() {
        return;
    }
    let path = std::path::PathBuf::from(root);
    app.grid.ink_job = spawn(move || Job::Loaded(loader(&path)));
    if app.grid.ink_job.is_none() {
        app.grid.ink_error = Some("The handwriting model can't be loaded in this build.".to_string());
    }
}

/// Runs `job` on a worker and returns the channel its answer arrives on. Without threads (the
/// web app) there is nothing to run: every model job needs a native host.
#[cfg(not(target_arch = "wasm32"))]
fn spawn<F: FnOnce() -> Job + Send + 'static>(job: F) -> Option<Receiver<Job>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    Some(rx)
}

#[cfg(target_arch = "wasm32")]
fn spawn<F: FnOnce() -> Job + Send + 'static>(_job: F) -> Option<Receiver<Job>> {
    None
}

/// Answers a finished worker job. Called every frame; cheap when nothing is running.
pub fn poll(app: &mut SheetApp) {
    let Some(rx) = app.grid.ink_job.take() else { return };
    let answer = match rx.try_recv() {
        Ok(job) => job,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            app.grid.ink_job = Some(rx);
            return;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            app.grid.ink_error = Some("The handwriting model download stopped unexpectedly. Try again.".to_string());
            return;
        }
    };
    match answer {
        Job::Downloaded(Ok(())) => {
            app.grid.ink_error = None;
            refresh(app);
            start(app);
        }
        Job::Downloaded(Err(e)) => app.grid.ink_error = Some(e),
        Job::Loaded(Ok(recognizer)) => {
            app.grid.ink_error = None;
            app.session.ink = Some(recognizer);
            refresh(app);
        }
        Job::Loaded(Err(e)) => {
            app.session.ink = None;
            app.grid.ink_error = Some(e);
            refresh(app);
        }
    }
}

/// Asks the engine for the current `ink.status` (three `stat` calls; no reading, no hashing).
fn refresh(app: &mut SheetApp) {
    if let Ok(status) = app.run("ink.status", json!({})) {
        app.grid.ink_status = Some(status);
    }
}

/// Opens the model panel (the Draw tab's Action Pen button when the tool can't be used).
pub fn open(app: &mut SheetApp) {
    app.grid.ink_panel = true;
    app.grid.ink_status = None;
    refresh(app);
}

/// Whether the engine has a recognizer, so the Draw tab can show the tool as available.
pub fn ready(app: &SheetApp) -> bool {
    app.session.ink.is_some()
}

/// The model panel, while it is open.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    if !app.grid.ink_panel {
        return;
    }
    if app.grid.ink_status.is_none() {
        refresh(app);
    }
    let mut open = true;
    let title = format!("Action Pen: {}", TROCR_SMALL_HANDWRITTEN.name);
    egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).default_width(440.0).show(ctx, |ui| panel(app, ui));
    if !open {
        app.grid.ink_panel = false;
    }
}

fn panel(app: &mut SheetApp, ui: &mut egui::Ui) {
    let manifest = &TROCR_SMALL_HANDWRITTEN;
    let status = app.grid.ink_status.clone().unwrap_or_else(|| json!({}));
    let ready = status["ready"].as_bool().unwrap_or(false);
    let present = status["filesPresent"].as_bool().unwrap_or(false);
    let directory = status["directory"].as_str().unwrap_or_default().to_string();
    let missing: Vec<String> =
        status["missing"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();

    ui.label("Handwriting to text: write in a cell with the Action Pen and the writing is typed into it.");
    ui.add_space(6.0);
    egui::Grid::new("action-pen-model").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.strong("Model");
        ui.label(format!("{} ({})", manifest.name, manifest.id));
        ui.end_row();
        ui.strong("Licence");
        ui.label(manifest.licence);
        ui.end_row();
        ui.strong("Size");
        ui.label(format!("{:.0} MB in {} files", manifest.total_bytes() as f64 / 1_000_000.0, manifest.files.len()));
        ui.end_row();
        ui.strong("Source");
        ui.add(egui::Label::new(egui::RichText::new(manifest.source).monospace()).truncate());
        ui.end_row();
        ui.strong("Folder");
        ui.add(egui::Label::new(egui::RichText::new(&directory).monospace()).truncate());
        ui.end_row();
        ui.strong("Runs on");
        ui.label(status["accelerator"].as_str().unwrap_or("cpu").to_uppercase());
        ui.end_row();
        ui.strong("Status");
        let (text, color) = if ready {
            ("Ready".to_string(), ui.visuals().hyperlink_color)
        } else if present {
            ("Files present, not loaded".to_string(), ui.visuals().warn_fg_color)
        } else {
            ("Not installed".to_string(), ui.visuals().warn_fg_color)
        };
        ui.label(egui::RichText::new(text).color(color));
        ui.end_row();
    });
    if !missing.is_empty() {
        ui.add_space(4.0);
        ui.label(format!("Missing: {}", missing.join(", ")));
    }
    if let Some(hint) = status["hint"].as_str() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(hint).italics());
    }
    if let Some(err) = &app.grid.ink_error {
        ui.add_space(4.0);
        ui.colored_label(ui.visuals().error_fg_color, err.as_str());
    }
    ui.add_space(8.0);

    let busy = app.grid.ink_job.is_some();
    let has_host = app.services.models.is_some();
    ui.horizontal(|ui| {
        let download = ui
            .add_enabled(has_host && !busy && !ready, egui::Button::new("Download"))
            .on_hover_text("Download the model from its published source (about 250 MB)");
        if download.clicked()
            && let Some(host) = app.services.models.clone()
        {
            app.grid.ink_error = None;
            let id = manifest.id.to_string();
            app.grid.ink_job = spawn(move || Job::Downloaded(host.download(&id)));
            if app.grid.ink_job.is_none() {
                app.grid.ink_error = Some("Downloads need the desktop app.".to_string());
            }
        }
        let import = ui.add_enabled(app.services.pick_model_files.is_some() && !busy && !ready, egui::Button::new("Import…"));
        if import.clicked() {
            let picked = app.services.pick_model_files.as_ref().map(|pick| pick()).unwrap_or_default();
            if !picked.is_empty() {
                app.grid.ink_error = None;
                let mut installed = false;
                for path in picked {
                    match app.run("ink.modelImport", json!({"path": path})) {
                        Ok(r) => installed = r["installed"].as_bool().unwrap_or(false),
                        Err(e) => {
                            app.grid.ink_error = Some(e);
                            break;
                        }
                    }
                }
                if installed {
                    start(app);
                }
                refresh(app);
            }
        }
        let remove = ui.add_enabled(present && !busy, egui::Button::new("Remove"));
        if remove.clicked() {
            match app.run("ink.modelRemove", json!({})) {
                Ok(_) => {
                    app.session.ink = None;
                    app.grid.ink_error = None;
                    refresh(app);
                }
                Err(e) => app.grid.ink_error = Some(e),
            }
        }
        if busy {
            ui.spinner();
        }
    });
    if !has_host {
        ui.add_space(4.0);
        ui.label("Downloading is available in the desktop app. You can copy the model's files into the folder above and use Import… .");
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!(
            "The model is not bundled with GridCraft. {} — {} — about {} MB.",
            manifest.licence,
            manifest.source,
            manifest.total_bytes() / 1_000_000
        ))
        .small(),
    );
}
