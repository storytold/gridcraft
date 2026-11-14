//! Opening a file in the background: the window stays alive, and the workbook joins the session
//! only when it is ready.
#![cfg(not(target_arch = "wasm32"))]

use std::time::Duration;

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.state_mut().background_open = true;
    h.run_steps(3);
    h
}

/// A workbook on disk: `formula` in A1.
fn file(name: &str, formula: &str) -> String {
    let dir = std::env::temp_dir().join(format!("gridcraft-background-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name).to_string_lossy().into_owned();
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": formula})).unwrap();
    s.execute("file.save", json!({"path": path})).unwrap();
    path
}

fn wait(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    for _ in 0..6000 {
        if !h.state().is_opening() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    assert!(!h.state().is_opening(), "the opening never finished");
    h.run_steps(2);
}

fn a1(h: &egui_kittest::Harness<'static, SheetApp>) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap())
}

#[test]
fn a_file_opens_and_replaces_the_blank_workbook() {
    let mut h = harness();
    let path = file("open.xlsx", "=1+41");
    h.state_mut().open_path(&path);
    assert!(h.state().is_opening());
    assert_eq!(h.state().session.documents().len(), 1, "nothing joins the session before it is ready");
    wait(&mut h);
    assert_eq!(h.state().session.documents().len(), 1, "the untouched blank workbook made way");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(path.as_str()));
    assert_eq!(a1(&h), Value::Number(42.0));
    assert_eq!(h.state().ui.recent.first(), Some(&path), "it joins the Recent list");
}

/// Without `logic` a finished opening isn't taken in, so the card stays for as long as a test
/// needs, however fast the file is read.
fn harness_without_logic() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h =
        crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut SheetApp| app.ui(ui), crate::common::app(session));
    h.state_mut().background_open = true;
    h.run_steps(3);
    h
}

#[test]
fn the_card_shows_the_file_and_cancel_keeps_everything_as_it_was() {
    let mut h = harness_without_logic();
    let path = file("cancelled.xlsx", "=1");
    h.state_mut().session.execute("cell.set", json!({"cell": "A1", "input": "kept"})).unwrap();
    h.state_mut().open_path(&path);
    h.run_steps(2);
    h.get_by_label("Opening cancelled.xlsx");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
    h.run_steps(3);
    assert!(!h.state().is_opening());
    std::thread::sleep(Duration::from_millis(100));
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(a1(&h), Value::text("kept"));
    assert!(h.state().message.is_none());
}

#[test]
fn cancel_also_drops_the_files_queued_behind() {
    let mut h = harness();
    let (one, two) = (file("first.xlsx", "=1"), file("second.xlsx", "=2"));
    h.state_mut().open_path(&one);
    h.state_mut().open_path(&two);
    h.state_mut().cancel_opening();
    h.run_steps(3);
    assert!(!h.state().is_opening());
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(h.state().session.active().unwrap().path.is_none());
}

#[test]
fn a_file_that_cant_be_read_shows_its_error() {
    let mut h = harness();
    h.state_mut().open_path("no such folder/missing.xlsx");
    wait(&mut h);
    assert!(h.state().message.is_some());
    assert_eq!(h.state().session.documents().len(), 1);
}

#[test]
fn files_asked_for_while_one_opens_open_after_it() {
    let mut h = harness();
    let (one, two) = (file("one.xlsx", "=1"), file("two.xlsx", "=2"));
    h.state_mut().open_path(&one);
    h.state_mut().open_path(&two);
    wait(&mut h);
    let paths: Vec<_> = h.state().session.documents().iter().filter_map(|d| d.path.clone()).collect();
    assert_eq!(paths, [one, two]);
}

#[test]
fn a_file_already_open_is_switched_to_at_once() {
    let mut h = harness();
    let path = file("twice.xlsx", "=3");
    h.state_mut().open_path(&path);
    wait(&mut h);
    h.state_mut().open_path(&path);
    assert!(!h.state().is_opening());
    assert_eq!(h.state().session.documents().len(), 1);
}

#[test]
fn without_background_opening_it_is_synchronous() {
    let mut h = harness();
    h.state_mut().background_open = false;
    let path = file("sync.xlsx", "=4");
    h.state_mut().open_path(&path);
    assert!(!h.state().is_opening());
    assert_eq!(a1(&h), Value::Number(4.0));
}

#[test]
fn a_cell_being_typed_is_entered_before_the_blank_workbook_makes_way() {
    let mut h = harness();
    let path = file("draft.xlsx", "=1");
    h.state_mut().begin_edit(Some("typed".into()), false);
    h.run_steps(2);
    h.state_mut().open_path(&path);
    assert!(h.state().editor.is_none());
    wait(&mut h);
    assert_eq!(h.state().session.documents().len(), 2, "the workbook has an entry now: it stays");
    let typed = h.state().session.documents()[0].wb.active().unwrap().value(CellRef::parse("A1").unwrap());
    assert_eq!(typed, Value::text("typed"));
}

#[test]
fn a_draft_that_cant_be_entered_stays_and_nothing_opens() {
    let mut h = harness();
    let path = file("refused.xlsx", "=1");
    h.state_mut().begin_edit(Some("=1*/2".into()), false);
    h.run_steps(2);
    h.state_mut().open_path(&path);
    assert!(!h.state().is_opening());
    assert!(h.state().message.is_some());
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("=1*/2"));
    assert_eq!(h.state().session.documents().len(), 1);
}

#[test]
fn the_open_is_in_the_journal_as_the_command_it_is() {
    let mut h = harness();
    let path = file("journal.xlsx", "=1");
    h.state_mut().open_path(&path);
    wait(&mut h);
    let journal = &h.state().session.journal;
    assert!(journal.iter().any(|(id, p)| id == "file.open" && p["path"] == json!(path)), "{journal:?}");
}

#[test]
fn a_file_opened_another_way_meanwhile_isnt_added_twice() {
    let mut h = harness_without_logic();
    let path = file("twice-meanwhile.xlsx", "=5");
    h.state_mut().open_path(&path);
    assert!(h.state().is_opening());
    // Another entry point (a command, the control channel) opens it first.
    h.state_mut().run("file.open", json!({"path": path})).unwrap();
    assert_eq!(h.state().session.documents().len(), 1);
    let ctx = h.ctx.clone();
    for _ in 0..2000 {
        h.state_mut().logic(&ctx);
        if !h.state().is_opening() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!h.state().is_opening());
    let same = h.state().session.documents().iter().filter(|d| d.path.as_deref() == Some(path.as_str())).count();
    assert_eq!(same, 1);
}
