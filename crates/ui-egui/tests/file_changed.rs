//! A file another program rewrote under a running GridCraft (a Git checkout, an editor): the
//! prompt appears, and both answers do what they say.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// A private temp folder for one test.
fn temp_dir(tag: &str) -> std::path::PathBuf {
    #[allow(clippy::disallowed_methods)] // a native test: a unique temp dir name
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("gridcraft-ui-{tag}-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    dir
}

/// `A1` set to `text`, written to `path` as a real `.xlsx`: what another program leaves behind.
fn write_workbook(path: &str, text: &str) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    let bytes = gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "model.xlsx").unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The app with `path` open, drawn for real.
fn harness(path: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.run("file.open", json!({"path": path})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn value_at(h: &egui_kittest::Harness<'static, SheetApp>, cell: &str) -> Value {
    h.state().session.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// The value of a cell in the workbook as it is on disk, what another program would see.
fn on_disk(path: &str, cell: &str) -> Value {
    let bytes = std::fs::read(path).unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("model.xlsx", &bytes).unwrap();
    wb.sheet(0).unwrap().value(CellRef::parse(cell).unwrap())
}

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

/// The prompt appears once, and "Keep Current Version" keeps the workbook and settles the change.
#[test]
fn a_changed_file_asks_and_keeps_the_version_in_gridcraft() {
    let dir = temp_dir("changed-keep");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    assert!(h.state().dialog.is_none(), "a file nobody touched asks nothing");

    write_workbook(&path, "the disk version");
    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("fileChanged"));
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("title")), Some(&json!("model.xlsx")));
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("dirty")), Some(&json!(false)));

    click(&mut h, "Keep Current Version");
    assert!(h.state().dialog.is_none());
    assert_eq!(value_at(&h, "A1"), Value::from("first"));
    // Settled: the next check doesn't ask again.
    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// "Use Disk Version" replaces the workbook, window identity and all.
#[test]
fn the_disk_version_replaces_the_workbook() {
    let dir = temp_dir("changed-disk");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    let uid = h.state().session.doc().unwrap().uid;

    write_workbook(&path, "the disk version");
    h.state_mut().check_external_changes();
    h.run_steps(2);
    click(&mut h, "Use Disk Version");
    assert!(h.state().dialog.is_none());
    assert_eq!(value_at(&h, "A1"), Value::from("the disk version"));
    let d = h.state().session.doc().unwrap();
    assert!(!d.is_dirty(), "the workbook matches the file it came from");
    assert_eq!(d.uid, uid, "the window and its view state stay");
    let _ = std::fs::remove_dir_all(dir);
}

/// Unsaved work: the prompt says the disk version discards it, and taking it does.
#[test]
fn unsaved_changes_are_warned_about_and_discarded_by_the_disk_choice() {
    let dir = temp_dir("changed-dirty");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "mine"})).unwrap();
    write_workbook(&path, "the disk version");

    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("dirty")), Some(&json!(true)));
    let _ = h.get_by_label("The workbook has unsaved changes: the disk version discards them.");

    click(&mut h, "Use Disk Version");
    assert_eq!(value_at(&h, "A1"), Value::from("the disk version"));
    assert!(value_at(&h, "B1").is_empty(), "the unsaved edit is gone, as the prompt said");
    let _ = std::fs::remove_dir_all(dir);
}

/// AutoSave never overwrites the version the prompt is offering.
#[test]
fn autosave_waits_while_the_prompt_is_open() {
    let dir = temp_dir("changed-autosave");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "mine"})).unwrap();
    write_workbook(&path, "the disk version");
    h.state_mut().grid.autosave = true;

    h.state_mut().check_external_changes();
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("fileChanged"));
    assert_eq!(on_disk(&path, "A1"), Value::from("the disk version"), "AutoSave held off while the prompt was open");
    assert_eq!(on_disk(&path, "B1"), Value::Empty, "the version in GridCraft was not written over it");

    // Answering lets AutoSave run again: the version in GridCraft is what gets saved.
    click(&mut h, "Keep Current Version");
    h.run_steps(4);
    assert_eq!(on_disk(&path, "A1"), Value::from("first"));
    assert_eq!(on_disk(&path, "B1"), Value::from("mine"));
    let _ = std::fs::remove_dir_all(dir);
}

/// Closing the prompt (its ✕, or Escape) is the "keep" answer, not a question asked again.
#[test]
fn closing_the_prompt_keeps_the_version_in_gridcraft() {
    let dir = temp_dir("changed-close");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    write_workbook(&path, "the disk version");
    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("fileChanged"));

    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "no nagging for a change that was already answered");
    assert_eq!(value_at(&h, "A1"), Value::from("first"));
    let _ = std::fs::remove_dir_all(dir);
}

/// An agent closing the prompt (`ui.dialog.cancel`) settles the change, like the ✕ does.
#[test]
fn cancelling_the_prompt_through_the_control_channel_settles_it() {
    let dir = temp_dir("changed-cancel");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut session = Session::new();
    session.new_workbook();
    session.run("file.open", json!({"path": path})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    write_workbook(&path, "the disk version");
    app.check_external_changes();
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("fileChanged"));

    let gridcraft_ui_egui::control::Outcome::Done(reply) =
        gridcraft_ui_egui::control::handle(&mut app, &egui::Context::default(), "ui.dialog.cancel", &json!({}))
    else {
        panic!("ui.dialog.cancel answers immediately");
    };
    assert_eq!(reply["ok"], true);
    assert!(app.dialog.is_none());
    assert_eq!(app.session.run("file.externalChanges", json!({})).unwrap()["changed"], json!(false), "the change is settled, not asked about again");
    let _ = std::fs::remove_dir_all(dir);
}

/// The prompt follows the interface language: its labels go through the catalog, and the file name
/// is filled into the translated line.
#[test]
fn the_prompt_follows_the_interface_language() {
    let dir = temp_dir("changed-zh");
    let path = dir.join("model.xlsx").to_str().unwrap().to_string();
    write_workbook(&path, "first");
    let mut h = harness(&path);
    h.state_mut().ui.language = gridcraft_ui_egui::i18n::Language::Zh;
    h.run_steps(2);
    // An unsaved edit, so the warning line shows too.
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "mine"})).unwrap();
    write_workbook(&path, "the disk version");

    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("fileChanged"));
    // The window title, the `{name}` line and the warning are the catalog's Chinese.
    let _ = h.get_by_label("文件已在磁盘上更改");
    let _ = h.get_by_label("model.xlsx 在 GridCraft 读取之后被其他程序（Git 检出或其他编辑器）改动过。");
    let _ = h.get_by_label("工作簿有未保存的修改：使用磁盘版本会丢弃这些修改。");

    // The buttons answer in Chinese too.
    click(&mut h, "保留当前版本");
    assert!(h.state().dialog.is_none());
    assert_eq!(value_at(&h, "A1"), Value::from("first"));
    assert_eq!(h.state_mut().session.run("file.externalChanges", json!({})).unwrap()["changed"], json!(false), "the change is settled");
    let _ = std::fs::remove_dir_all(dir);
}

/// A workbook that was never saved has no file to watch, and the browser has no file system.
#[test]
fn a_workbook_without_a_file_never_asks() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().check_external_changes();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
}
