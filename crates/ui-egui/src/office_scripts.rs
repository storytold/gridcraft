//! Automate ▸ Office Scripts: the script list, the TypeScript editor, Run/Save/Delete, the
//! runtime's state and the recorder. TypeScript is never built here: the script is text, checked
//! for obvious damage by the engine, and run by the Bun host (or refused with an actionable
//! message when Bun is missing).
//!
//! The pane keeps its draft in [`Pane`] (`SheetApp::office_scripts`), so Run can run unsaved text
//! and a script that fails leaves the text in place with the failing line marked.

use std::ops::Range;

use egui::{Align, Id, Layout, RichText, Ui, vec2};
use gridcraft_script::{RuntimeInfo, ScriptRecord};
use serde_json::{Value as Json, json};

use crate::code_editor::{self, Lang};
use crate::dialogs::Dialog;
use crate::theme::Tokens;
use crate::{SheetApp, clean_error};

/// The pane's state, kept between frames and dialogs.
#[derive(Default)]
pub struct Pane {
    pub list: Vec<ScriptRecord>,
    pub selected: Option<String>,
    pub name: String,
    pub code: String,
    pub dirty: bool,
    pub runtime: Option<RuntimeInfo>,
    pub folder: Option<String>,
    pub recording: bool,
    pub calls: usize,
    pub output: Vec<String>,
    pub status: Option<Status>,
    pub error_line: Option<u32>,
    /// The list and runtime have been read at least once.
    loaded: bool,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub error: bool,
    pub text: String,
}

impl Status {
    fn ok(text: impl Into<String>) -> Self {
        Status { error: false, text: text.into() }
    }

    fn failed(text: impl Into<String>) -> Self {
        Status { error: true, text: text.into() }
    }
}

/// Opens the pane (Automate ▸ Office Scripts).
pub fn open(app: &mut SheetApp) -> Dialog {
    refresh(app);
    Dialog::custom("officeScripts", "Office Scripts", json!({}))
}

/// Re-reads the script list and the runtime's state. Called when the pane opens and after an
/// action that can change either.
pub fn refresh(app: &mut SheetApp) {
    ensure_host(app);
    let list = app.session.run("script.list", json!({})).ok().and_then(|v| serde_json::from_value::<Vec<ScriptRecord>>(v).ok()).unwrap_or_default();
    let runtime = app.session.run("script.runtime", json!({})).ok().and_then(|v| serde_json::from_value::<RuntimeInfo>(v).ok());
    let folder = app.session.run("script.folder", json!({})).ok().and_then(|v| v.get("folder").and_then(Json::as_str).map(str::to_string));
    let pane = &mut app.office_scripts;
    pane.list = list;
    if pane.selected.as_ref().is_some_and(|id| !pane.list.iter().any(|r| &r.id == id)) {
        pane.selected = None;
    }
    pane.runtime = runtime;
    pane.folder = folder;
    pane.loaded = true;
}

/// Finds the runtime again when the pane opens or the user installed Bun while GridCraft runs.
fn ensure_host(app: &mut SheetApp) {
    if let Some(make) = &app.services.bun_host {
        app.session.script_host = make();
    }
}

/// Loads a saved script into the editor. A dirty draft is not replaced: the person saves it (or
/// clears it) first, so switching scripts never loses edits.
fn select(app: &mut SheetApp, id: &str) {
    if app.office_scripts.dirty && app.office_scripts.selected.as_deref() != Some(id) {
        app.office_scripts.status = Some(Status::failed("Save the draft before opening another script."));
        return;
    }
    if let Some(record) = app.office_scripts.list.iter().find(|r| r.id == id).cloned() {
        let pane = &mut app.office_scripts;
        pane.selected = Some(record.id.clone());
        pane.name = record.name.clone();
        pane.code = record.code.clone();
        pane.dirty = false;
        pane.error_line = None;
        pane.status = None;
        pane.output.clear();
        pane.calls = 0;
    }
}

/// Starts or stops recording from the journal (the Automate tab and the pane share this).
pub fn toggle_recording(app: &mut SheetApp) {
    let on = !app.office_scripts.recording;
    match app.session.run("script.record", json!({"on": on})) {
        Ok(result) => {
            app.office_scripts.recording = result.get("recording").and_then(Json::as_bool).unwrap_or(false);
            if on {
                app.office_scripts.status = Some(Status::ok("Recording. Every command you run becomes a step."));
            } else if let Some(code) = result.get("code").and_then(Json::as_str) {
                refresh(app);
                let steps = result.get("steps").and_then(Json::as_u64).unwrap_or(0);
                let skipped = result.get("skipped").and_then(Json::as_array).map_or(0, Vec::len);
                // A dirty draft is the person's work: the recording is saved and listed, but the
                // editor keeps what it had instead of being overwritten by the recorded code.
                if app.office_scripts.dirty {
                    app.office_scripts.status =
                        Some(Status::ok(format!("Recorded {steps} steps; {skipped} skipped. Saved as a new script; the draft is unsaved.")));
                    return;
                }
                if let Some(id) = result.get("id").and_then(Json::as_str) {
                    select(app, id);
                }
                app.office_scripts.code = code.to_string();
                app.office_scripts.dirty = false;
                app.office_scripts.status = Some(Status::ok(format!("Recorded {steps} steps; {skipped} skipped (see the comments in the script).")));
            } else {
                app.office_scripts.status = Some(Status::ok("Recording stopped."));
            }
        }
        Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
    }
}

/// Runs the pane's draft (`code`), marking the failing line when the runtime names one.
pub fn run(app: &mut SheetApp) {
    if app.office_scripts.code.trim().is_empty() {
        app.office_scripts.status = Some(Status::failed("Write or select a script first."));
        return;
    }
    let code = app.office_scripts.code.clone();
    match app.session.run("script.run", json!({"code": code})) {
        Ok(result) => {
            let calls = result.get("calls").and_then(Json::as_u64).unwrap_or(0);
            let output: Vec<String> = result
                .get("output")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let pane = &mut app.office_scripts;
            pane.calls = calls as usize;
            pane.output = output;
            pane.error_line = None;
            pane.status = Some(Status::ok(format!("Ran {calls} workbook calls.")));
        }
        Err(e) => {
            let text = clean_error(&e);
            let pane = &mut app.office_scripts;
            pane.error_line = error_line(&text).and_then(|n| u32::try_from(n).ok());
            // The previous run's console output is not this run's: clear it with the failure.
            pane.output.clear();
            pane.calls = 0;
            pane.status = Some(Status::failed(text));
        }
    }
}

/// The pane's body. `open` closes the dialog.
pub fn pane(app: &mut SheetApp, ui: &mut Ui, _d: &mut Dialog, open: &mut bool) {
    if !app.office_scripts.loaded {
        refresh(app);
    }
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(190.0, 430.0), Layout::top_down(Align::Min), |ui| {
            list(app, ui);
        });
        ui.separator();
        ui.vertical(|ui| editor(app, ui, open));
    });
}

fn list(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(app.ui.language.tr("Scripts")).strong());
    let records = app.office_scripts.list.clone();
    if records.is_empty() {
        ui.label(RichText::new("No saved scripts").italics().color(t.text_dim));
    }
    egui::ScrollArea::vertical().id_salt("office_scripts_list").max_height(330.0).auto_shrink([false, false]).show(ui, |ui| {
        for record in &records {
            let selected = app.office_scripts.selected.as_deref() == Some(record.id.as_str());
            let label = format!("{}\n{}", record.name, modified(record.modified));
            if ui.selectable_label(selected, label).clicked() {
                select(app, &record.id);
            }
        }
    });
    if ui.add_enabled(!records.is_empty(), egui::Button::new(app.ui.language.tr("Delete")).min_size(vec2(180.0, 24.0))).clicked()
        && let Some(id) = app.office_scripts.selected.clone()
    {
        match app.session.run("script.delete", json!({"id": id})) {
            Ok(_) => {
                // The editor is empty afterwards: clear everything that belonged to the script, so
                // no stale error marker or output survives the delete.
                let pane = &mut app.office_scripts;
                pane.selected = None;
                pane.name.clear();
                pane.code.clear();
                pane.dirty = false;
                pane.error_line = None;
                pane.output.clear();
                pane.calls = 0;
                refresh(app);
                app.office_scripts.status = Some(Status::ok("Deleted."));
            }
            Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
        }
    }
}

fn editor(app: &mut SheetApp, ui: &mut Ui, open: &mut bool) {
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        ui.label(lang.tr("Name"));
        let name = &mut app.office_scripts.name;
        if ui.add(egui::TextEdit::singleline(name).desired_width(190.0).hint_text("Recorded Script")).changed() {
            app.office_scripts.dirty = true;
        }
        if ui.button(lang.tr("Save")).clicked() {
            save(app);
        }
        let runnable = app.office_scripts.runtime.as_ref().is_some_and(|r| r.found);
        if ui.add_enabled(runnable, egui::Button::new(lang.tr("Run Script"))).clicked() {
            run(app);
        }
        let recording = app.office_scripts.recording;
        let label = if recording { lang.tr("Stop") } else { lang.tr("Record Script") };
        if ui.button(label).clicked() {
            toggle_recording(app);
        }
    });

    runtime_row(app, ui);
    folder_row(app, ui);
    ui.separator();

    let mark = app.office_scripts.error_line.and_then(|line| line_range(&app.office_scripts.code, line));
    let response = code_editor::code_editor_marked(ui, Id::unique("office_scripts_code"), &mut app.office_scripts.code, Lang::Plain, 18, mark);
    if response.changed() {
        app.office_scripts.dirty = true;
        app.office_scripts.error_line = None;
    }

    if let Some(status) = &app.office_scripts.status {
        let t = Tokens::get(ui.ctx());
        let colour = if status.error { t.danger } else { t.text_dim };
        ui.label(RichText::new(&status.text).color(colour).small());
    }
    if !app.office_scripts.output.is_empty() {
        ui.push_id("office_scripts_output", |ui| {
            egui::ScrollArea::vertical().max_height(80.0).auto_shrink([false, true]).show(ui, |ui| {
                for line in &app.office_scripts.output {
                    ui.monospace(line);
                }
            });
        });
    }
    ui.horizontal(|ui| {
        if ui.button(lang.tr("Close")).clicked() {
            *open = false;
        }
        ui.label(
            RichText::new("Every script runs as one undo step; an error or timeout changes nothing.").small().color(Tokens::get(ui.ctx()).text_dim),
        );
    });
}

fn runtime_row(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let runtime = app.office_scripts.runtime.clone();
    let found = runtime.as_ref().is_some_and(|r| r.found);
    ui.horizontal_wrapped(|ui| {
        match &runtime {
            Some(info) if info.found => {
                let version = info.version.as_deref().unwrap_or("?");
                let path = info.path.as_deref().unwrap_or("");
                ui.label(format!("Bun {version} — {path}"));
            }
            Some(info) => {
                let t = Tokens::get(ui.ctx());
                ui.label(RichText::new(info.message.clone()).color(t.danger).small());
            }
            None => {
                ui.label(lang.tr("Bun is not installed"));
            }
        }
        if !found {
            if ui.button(lang.tr("Download Bun…")).clicked() {
                download_bun(app);
            }
            if app.services.import_bun.is_some() && ui.button(lang.tr("Import Bun…")).clicked() {
                import_bun(app);
            }
            if ui.button(lang.tr("Check Again")).clicked() {
                refresh(app);
            }
        }
    });
    if let Some(info) = &app.office_scripts.runtime
        && !info.found
        && !info.expected_path.is_empty()
    {
        ui.label(RichText::new(format!("Expected at {}", info.expected_path)).small().color(Tokens::get(ui.ctx()).text_dim));
    }
}

fn folder_row(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    ui.horizontal_wrapped(|ui| {
        let folder = app.office_scripts.folder.clone().unwrap_or_default();
        ui.label(format!("{}: {folder}", lang.tr("Scripts")));
        if ui.button(lang.tr("Change Folder…")).clicked()
            && let Some(pick) = &app.services.pick_folder
            && let Some(path) = pick()
        {
            match app.session.run("script.folder", json!({"path": path})) {
                Ok(_) => {
                    refresh(app);
                    forget_selection(app);
                    app.office_scripts.status = Some(Status::ok("Scripts folder changed."));
                }
                Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
            }
        }
        if ui.button(lang.tr("Reset Folder")).clicked() {
            match app.session.run("script.folder", json!({"path": ""})) {
                Ok(_) => {
                    refresh(app);
                    forget_selection(app);
                    app.office_scripts.status = Some(Status::ok("Scripts folder reset."));
                }
                Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
            }
        }
        if app.services.pick_script.is_some()
            && ui.button(lang.tr("Import Script…")).clicked()
            && let Some(path) = app.services.pick_script.as_ref().and_then(|pick| pick())
        {
            match app.session.run("script.import", json!({"path": path})) {
                Ok(record) => {
                    if let Some(id) = record.get("id").and_then(Json::as_str).map(str::to_string) {
                        refresh(app);
                        select(app, &id);
                    }
                    app.office_scripts.status = Some(Status::ok("Script imported."));
                }
                Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
            }
        }
        let exportable = app.office_scripts.selected.clone();
        if ui.add_enabled(exportable.is_some(), egui::Button::new(lang.tr("Export Script…"))).clicked() && app.services.pick_save.is_some() {
            // Export writes the saved record, not the editor: save a dirty draft first so the
            // `.osts` file holds what is on screen.
            if app.office_scripts.dirty {
                save(app);
            }
            if app.office_scripts.dirty {
                // The save failed and its error is in the status: exporting now would write the
                // old record, not what is on screen.
                return;
            }
            let Some(id) = app.office_scripts.selected.clone() else { return };
            let Some(pick) = app.services.pick_save.as_ref() else { return };
            let name = app.office_scripts.name.clone();
            let suggested = if name.trim().is_empty() { "Script.osts".to_string() } else { format!("{}.osts", name.trim()) };
            if let Some(mut path) = pick(&suggested) {
                if std::path::Path::new(&path).extension().is_none() {
                    path.push_str(".osts");
                }
                match app.session.run("script.export", json!({"id": id, "path": path})) {
                    Ok(_) => app.office_scripts.status = Some(Status::ok("Script exported.")),
                    Err(e) => app.office_scripts.status = Some(Status::failed(clean_error(&e))),
                }
            }
        }
    });
}

/// After the folder changes, the ids in the list mean different scripts: drop the selection (and
/// the editor with it, unless the draft is dirty — that work is the person's).
fn forget_selection(app: &mut SheetApp) {
    let pane = &mut app.office_scripts;
    pane.selected = None;
    if !pane.dirty {
        pane.name.clear();
        pane.code.clear();
        pane.error_line = None;
        pane.output.clear();
        pane.calls = 0;
    }
}

fn download_bun(app: &mut SheetApp) {
    if let Some(install) = &app.services.install_bun {
        match install() {
            Ok(_) => {
                refresh(app);
                app.office_scripts.status = Some(Status::ok("Bun installed."));
            }
            Err(e) => app.office_scripts.status = Some(Status::failed(e)),
        }
        return;
    }
    // No downloader in this shell: open Bun's page and say where the file goes.
    if let Some(open) = &app.services.open_url {
        open(gridcraft_script::BUN_DOWNLOAD_URL);
    }
    let expected = app.office_scripts.runtime.as_ref().map(|r| r.expected_path.clone()).unwrap_or_default();
    app.office_scripts.status =
        Some(Status::ok(format!("Download Bun from {}. Then use Import Bun… to put it at {}.", gridcraft_script::BUN_DOWNLOAD_URL, expected)));
}

fn import_bun(app: &mut SheetApp) {
    let Some(pick) = &app.services.pick_bun else { return };
    let Some(path) = pick() else { return };
    let Some(import) = &app.services.import_bun else { return };
    match import(&path) {
        Ok(_) => {
            ensure_host(app);
            refresh(app);
            app.office_scripts.status = Some(Status::ok("Bun imported."));
        }
        Err(e) => app.office_scripts.status = Some(Status::failed(e)),
    }
}

fn save(app: &mut SheetApp) {
    let params = json!({"name": app.office_scripts.name, "code": app.office_scripts.code});
    match app.session.run("script.save", params) {
        Ok(saved) => {
            match serde_json::from_value::<ScriptRecord>(saved) {
                Ok(record) => {
                    app.office_scripts.selected = Some(record.id.clone());
                    app.office_scripts.name = record.name.clone();
                    app.office_scripts.code = record.code.clone();
                    app.office_scripts.dirty = false;
                    app.office_scripts.status = Some(Status::ok("Saved."));
                }
                Err(e) => app.office_scripts.status = Some(Status::failed(e.to_string())),
            }
            refresh(app);
        }
        Err(e) => {
            let text = clean_error(&e);
            app.office_scripts.error_line = error_line(&text).and_then(|n| u32::try_from(n).ok());
            app.office_scripts.status = Some(Status::failed(text));
        }
    }
}

/// The line number in a `line N` (or `script.ts:N:M`) message.
fn error_line(message: &str) -> Option<usize> {
    let rest = message
        .split_once("line ")
        .map(|(_, r)| r)
        .or_else(|| message.split(':').find(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())))?;
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// The byte range of 1-based `line`, for the editor's underline.
fn line_range(text: &str, line: u32) -> Option<Range<usize>> {
    if line == 0 {
        return None;
    }
    let mut start = 0usize;
    for _ in 1..line {
        let rest = text.get(start..)?;
        let nl = rest.find('\n')?;
        start = start.saturating_add(nl).saturating_add(1);
    }
    let rest = text.get(start..)?;
    let end = rest.find('\n').map_or(text.len(), |n| start.saturating_add(n));
    Some(start..end)
}

fn modified(ms: u64) -> String {
    // Days since the epoch to a civil date, by the standard 400-year cycle; the pane only needs a
    // readable date, and the engine stores UTC milliseconds.
    let days = i64::try_from(ms / 86_400_000).unwrap_or(0);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days`, offset for the Unix epoch.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days.saturating_add(719_468);
    let era = if z >= 0 { z } else { z.saturating_sub(146_096) } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_found() {
        let text = "one\ntwo\nthree";
        assert_eq!(line_range(text, 1), Some(0..3));
        assert_eq!(line_range(text, 2), Some(4..7));
        assert_eq!(line_range(text, 3), Some(8..13));
        assert_eq!(line_range(text, 4), None);
        assert_eq!(line_range(text, 0), None);
    }

    #[test]
    fn failing_lines_are_read_from_messages() {
        assert_eq!(error_line("line 4: no such sheet"), Some(4));
        assert_eq!(error_line("invalid parameters for `script.run`: unterminated string (line 2, column 3)"), Some(2));
        assert_eq!(error_line("script.ts:7:9: x is not defined"), Some(7));
        assert_eq!(error_line("no line here"), None);
    }

    #[test]
    fn dates_are_readable() {
        assert_eq!(modified(0), "1970-01-01");
        assert_eq!(modified(1_700_000_000_000), "2023-11-14");
    }
}
