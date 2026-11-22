//! A Save that finds the workbook's own file changed on disk since it was opened here (#4078):
//! nothing is written until the person answers the question, Save a Copy (Enter), Overwrite or
//! Cancel; a close goes on only after a save that went through; AutoSave stops for the workbook
//! and says why. A file nobody else changed is saved as before, without a question.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::{Session, io::CHANGED_ON_DISK};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// A folder of its own under the system's temporary folder, removed afterwards.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(tag: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("gridcraft-changed-on-disk-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Folder(dir)
}

/// Writes a one-sheet workbook with `a1` in A1 to `path`.
fn write_workbook(path: &Path, a1: &str) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": a1})).unwrap();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
}

/// Puts a workbook with `a1` in A1 at `path` as many programs save a file: written under a temporary name
/// beside it, then renamed over it. Returns its bytes.
fn replace_by_rename(path: &Path, a1: &str) -> Vec<u8> {
    let tmp = path.with_file_name("newer-from-another-program.xlsx");
    write_workbook(&tmp, a1);
    let bytes = std::fs::read(&tmp).unwrap();
    std::fs::rename(&tmp, path).unwrap();
    bytes
}

/// A1 of the workbook in the file at `path`, read by a session of its own.
fn a1_on_disk(path: &Path) -> Value {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0))
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// The names Save As proposed, and what it answers: `pick`, or a cancelled window.
#[derive(Default)]
struct Picker {
    proposed: Vec<String>,
    pick: Option<String>,
}

fn services(picker: &Arc<Mutex<Picker>>) -> Services {
    let picker = picker.clone();
    Services {
        pick_save: Some(Box::new(move |proposed: &str| {
            let mut p = picker.lock().unwrap();
            p.proposed.push(proposed.to_string());
            p.pick.clone()
        })),
        ..Default::default()
    }
}

/// The app with `path` (holding "as opened" in A1) opened, as a double-click opens it.
fn opened(path: &Path, picker: &Arc<Mutex<Picker>>) -> Harness {
    write_workbook(path, "as opened");
    let mut h = crate::support::services_harness(services(picker), 4);
    h.state_mut().open_path(&path.to_string_lossy());
    h.run_steps(3);
    assert!(h.state().message.is_none());
    h
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn set(h: &mut Harness, cell: &str, input: &str) {
    h.state_mut().run("cell.set", json!({"cell": cell, "input": input})).unwrap();
    h.run_steps(2);
}

/// The name of the dialog shown, if any.
fn dialog(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name.clone())
}

/// Clicks the button `label` of the dialog shown, and gathers the commands sent to the window over
/// the next frames.
fn click(h: &mut Harness, label: &str) -> Vec<egui::ViewportCommand> {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    step_commands(h)
}

/// The window's close button, as a person presses it, and the commands sent to the window.
fn request_window_close(h: &mut Harness) -> Vec<egui::ViewportCommand> {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    step_commands(h)
}

/// Runs a few frames and gathers the commands sent to the window.
fn step_commands(h: &mut Harness) -> Vec<egui::ViewportCommand> {
    let mut out = Vec::new();
    for _ in 0..3 {
        h.step();
        out.extend(h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    }
    out
}

fn dirty(h: &Harness) -> bool {
    h.state().session.active().unwrap().is_dirty()
}

#[test]
fn ctrl_s_asks_before_anything_is_written_and_cancel_writes_nothing() {
    let dir = folder("cancel");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");

    // With no edit: the question, nothing written, no message box.
    key(&mut h, Key::S, CTRL);
    assert_eq!(dialog(&h).as_deref(), Some("fileChanged"));
    assert!(h.state().message.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), newer);
    // Its words: newer on disk than on the screen; Save a Copy keeps both; Overwrite throws it away.
    for words in ["Budget.xlsx on disk is newer than what is on your screen", "Save a Copy keeps both", "thrown away"] {
        assert!(h.query_by_label_contains(words).is_some(), "{words}");
    }
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(dialog(&h), None);
    assert_eq!(std::fs::read(&path).unwrap(), newer, "Cancel wrote nothing");

    // With an edit: the same.
    set(&mut h, "B1", "my edit");
    key(&mut h, Key::S, CTRL);
    assert_eq!(dialog(&h).as_deref(), Some("fileChanged"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(std::fs::read(&path).unwrap(), newer, "Cancel wrote nothing");
    assert!(dirty(&h), "the edit is still unsaved");
    assert!(picker.lock().unwrap().proposed.is_empty(), "no Save As window");
}

#[test]
fn enter_saves_a_copy_beside_the_newer_file_which_stays() {
    let dir = folder("copy");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "my work");
    // A file already has the first copy's name: the next free one is proposed.
    std::fs::write(dir.0.join("Budget (copy).xlsx"), b"someone's").unwrap();
    let copy = dir.0.join("Budget (copy 2).xlsx");
    picker.lock().unwrap().pick = Some(copy.to_string_lossy().into_owned());

    key(&mut h, Key::S, CTRL);
    assert_eq!(dialog(&h).as_deref(), Some("fileChanged"));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(picker.lock().unwrap().proposed, ["Budget (copy 2).xlsx"], "the Save As window, on a copy's name no file in the folder has");
    assert_eq!(dialog(&h), None);
    assert_eq!(std::fs::read(&path).unwrap(), newer, "the newer file stays");
    assert_eq!(a1_on_disk(&copy), text("my work"));
    assert_eq!(std::fs::read(dir.0.join("Budget (copy).xlsx")).unwrap(), b"someone's");
    assert!(!dirty(&h));
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(copy.to_string_lossy().as_ref()), "the workbook is the copy now");
}

#[test]
fn overwrite_replaces_the_newer_file() {
    let dir = folder("overwrite");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "my work");
    key(&mut h, Key::S, CTRL);
    h.get_by_label("Overwrite").click();
    h.run_steps(2);
    assert_eq!(dialog(&h), None);
    assert_eq!(a1_on_disk(&path), text("my work"));
    assert!(!dirty(&h));
    // From then on an ordinary Save asks nothing.
    set(&mut h, "A1", "more work");
    key(&mut h, Key::S, CTRL);
    assert_eq!(dialog(&h), None);
    assert_eq!(a1_on_disk(&path), text("more work"));
}

#[test]
fn a_file_nobody_changed_is_saved_without_a_question() {
    let dir = folder("ordinary");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    for n in 0..5 {
        set(&mut h, "A1", &format!("entry {n}"));
        key(&mut h, Key::S, CTRL);
        assert_eq!(dialog(&h), None);
        assert!(h.state().message.is_none());
        assert_eq!(a1_on_disk(&path), text(&format!("entry {n}")));
        // The same bytes written again by another program in between: still no question.
        let same = std::fs::read(&path).unwrap();
        let tmp = dir.0.join("same.tmp");
        std::fs::write(&tmp, &same).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    }
}

#[test]
fn closing_with_changes_asks_first_and_closes_only_after_the_save() {
    let dir = folder("close");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "my work");

    // Save in the Save Changes question: the question about the file comes before anything is
    // written; Cancel keeps the workbook open.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(2);
    assert_eq!(dialog(&h).as_deref(), Some("saveChanges"));
    click(&mut h, "Save");
    assert_eq!(dialog(&h).as_deref(), Some("fileChanged"));
    assert_eq!(std::fs::read(&path).unwrap(), newer);
    click(&mut h, "Cancel");
    assert_eq!(dialog(&h), None);
    assert_eq!(std::fs::read(&path).unwrap(), newer);
    assert!(h.state().session.active().is_some_and(|d| d.is_dirty()), "still open, still changed");

    // Again, and Overwrite: written, then closed.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(2);
    click(&mut h, "Save");
    click(&mut h, "Overwrite");
    assert_eq!(a1_on_disk(&path), text("my work"));
    assert!(h.state().session.documents().iter().all(|d| d.path.as_deref() != Some(path.to_string_lossy().as_ref())), "closed");
}

#[test]
fn a_quit_goes_on_only_after_a_save_that_went_through() {
    let dir = folder("quit");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "my work");

    // The window's close asks about the changed workbook; Save meets the file changed on disk.
    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    assert_eq!(dialog(&h).as_deref(), Some("saveChanges"));
    click(&mut h, "Save");
    assert_eq!(dialog(&h).as_deref(), Some("fileChanged"));
    // Cancel stops the quit: the workbook is open and still changed, the newer file untouched.
    let sent = click(&mut h, "Cancel");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert!(!h.state().allow_close);
    assert_eq!(dialog(&h), None);
    assert!(dirty(&h));
    assert_eq!(std::fs::read(&path).unwrap(), newer);

    // Again, and Overwrite: written, and the quit goes on.
    request_window_close(&mut h);
    click(&mut h, "Save");
    let sent = click(&mut h, "Overwrite");
    assert_eq!(a1_on_disk(&path), text("my work"));
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
}

#[test]
fn save_a_copy_with_the_save_as_window_cancelled_writes_nothing() {
    let dir = folder("copy-cancelled");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "my work");
    let files = |dir: &Path| std::fs::read_dir(dir).unwrap().count();
    let before = files(&dir.0);

    // From Ctrl+S: the question, Enter (Save a Copy), and the Save As window answers nothing.
    key(&mut h, Key::S, CTRL);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(picker.lock().unwrap().proposed.len(), 1, "the Save As window was shown");
    assert_eq!(dialog(&h), None);
    assert!(h.state().message.is_none());
    assert!(dirty(&h));
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()), "still on its own file");
    assert_eq!(std::fs::read(&path).unwrap(), newer);
    assert_eq!(files(&dir.0), before, "no file written");

    // From a close: the workbook stays open.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(2);
    click(&mut h, "Save");
    click(&mut h, "Save a Copy");
    assert_eq!(dialog(&h), None);
    assert!(h.state().session.documents().iter().any(|d| d.path.as_deref() == Some(path.to_string_lossy().as_ref())), "still open");
    assert!(dirty(&h));
    assert_eq!(files(&dir.0), before, "no file written");
}

#[test]
fn autosave_stops_for_the_workbook_says_why_and_never_asks() {
    let dir = folder("autosave");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    h.state_mut().ui.autosave = true;
    let due = |h: &mut Harness| {
        h.state_mut().grid.last_autosave = -1e12;
        h.run_steps(2);
    };
    // As before: a change is saved by AutoSave.
    set(&mut h, "A1", "autosaved");
    due(&mut h);
    assert_eq!(a1_on_disk(&path), text("autosaved"));

    let newer = replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "not autosaved");
    for _ in 0..3 {
        due(&mut h);
        assert_eq!(std::fs::read(&path).unwrap(), newer, "AutoSave never writes over it");
        assert_eq!(dialog(&h), None, "AutoSave never asks");
    }
    let strip = h.state().notice.as_ref().map(|n| n.text.clone()).unwrap_or_default();
    assert!(strip.starts_with("AutoSave is off for this workbook"), "{strip}");
    assert!(strip.contains("changed on disk"), "{strip}");

    // Ctrl+S asks; Overwrite saves; AutoSave goes on and its line goes.
    key(&mut h, Key::S, CTRL);
    h.get_by_label("Overwrite").click();
    h.run_steps(2);
    assert_eq!(a1_on_disk(&path), text("not autosaved"));
    assert!(h.state().notice.is_none(), "{:?}", h.state().notice.as_ref().map(|n| &n.text));
    set(&mut h, "A1", "autosaved again");
    due(&mut h);
    assert_eq!(a1_on_disk(&path), text("autosaved again"));
}

#[test]
fn a_script_gets_the_refusal_and_no_question() {
    let dir = folder("script");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    let newer = replace_by_rename(&path, "newer from elsewhere");
    let e = h.state_mut().run("file.save", json!({})).unwrap_err();
    h.run_steps(2);
    assert!(e.contains(CHANGED_ON_DISK), "{e}");
    assert_eq!(dialog(&h), None);
    assert_eq!(std::fs::read(&path).unwrap(), newer);
}

/// The line that AutoSave stopped shows in every window of the workbook, one already open and one
/// opened after it stopped, and goes from each once a save went through.
#[test]
fn the_autosave_line_shows_in_every_window_of_the_workbook() {
    let dir = folder("autosave-windows");
    let path = dir.0.join("Budget.xlsx");
    let picker = Arc::new(Mutex::new(Picker::default()));
    let mut h = opened(&path, &picker);
    h.state_mut().ui.autosave = true;
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(2);
    let first = h.state().session.windows()[0].id;
    let second = h.state().session.windows()[1].id;
    h.state_mut().enter_window(first);
    h.run_steps(2);
    let line = |h: &Harness| h.state().notice.as_ref().is_some_and(|n| n.text.starts_with("AutoSave is off for this workbook"));

    replace_by_rename(&path, "newer from elsewhere");
    set(&mut h, "A1", "not autosaved");
    h.state_mut().grid.last_autosave = -1e12;
    h.run_steps(2);
    assert!(line(&h), "the window AutoSave ran in");
    h.state_mut().enter_window(second);
    h.run_steps(2);
    assert!(line(&h), "the other window of the workbook");
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(2);
    assert!(line(&h), "a window opened after AutoSave stopped");

    // Overwrite from one window: the line goes from each.
    key(&mut h, Key::S, CTRL);
    click(&mut h, "Overwrite");
    assert!(!line(&h));
    for w in [first, second] {
        h.state_mut().enter_window(w);
        h.run_steps(2);
        assert!(!line(&h), "window {w}");
    }
}
