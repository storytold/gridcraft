//! Tests for prompt to save changes before closing the window (issue #1172).

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(services: Services) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

fn click_button(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

#[test]
fn clean_workbook_allows_immediate_close() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    assert!(app.close_requested(), "a clean workbook closes without asking");
    assert!(app.dialog.is_none());
    assert!(!app.quit_confirmed);
}

#[test]
fn dirty_workbook_prompts_to_save_on_close() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.run("cell.set", json!({"cell": "A1", "input": "Hello"})).unwrap();
    assert!(!app.close_requested(), "an edited workbook asks first");
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(app.dialog.as_ref().unwrap().values.get("closingWindow").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn active_cell_edit_commits_and_prompts_on_close() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.begin_edit(Some("typing in cell".into()), false);
    assert!(app.editor.is_some(), "editor is active");
    assert!(!app.close_requested(), "cell edit must prompt before closing");
    assert!(app.editor.is_none(), "cell edit was committed");
    assert!(app.session.active().unwrap().is_dirty(), "workbook is dirty from edit");
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
}

#[test]
fn cancel_button_keeps_window_and_workbook_open() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Unsaved"})).unwrap();
    assert!(!h.state_mut().close_requested());
    h.run_steps(2);

    click_button(&mut h, "Cancel");
    assert!(h.state().dialog.is_none(), "dialog closes on Cancel");
    assert!(!h.state().quit_confirmed, "quit is not confirmed");
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(h.state().session.active().unwrap().is_dirty());
}

#[test]
fn dont_save_button_closes_dirty_workbook_and_confirms_quit() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Unsaved"})).unwrap();
    assert!(!h.state_mut().close_requested());
    h.run_steps(2);

    click_button(&mut h, "Don't Save");
    assert!(h.state().quit_confirmed, "quit is confirmed when Don't Save is clicked");
    assert!(h.state().dialog.is_none());
    assert!(h.state_mut().close_requested(), "subsequent close request proceeds");
}

#[test]
fn save_button_with_path_saves_and_confirms_quit() {
    let dir = std::env::temp_dir().join(format!("gridcraft-save-on-close-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("TestBook.xlsx");

    let mut h = harness(Services::default());
    h.state_mut().session.active_mut().unwrap().path = Some(file_path.to_string_lossy().into_owned());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Important Data"})).unwrap();
    assert!(!h.state_mut().close_requested());
    h.run_steps(2);

    click_button(&mut h, "Save");
    assert!(h.state().quit_confirmed, "quit is confirmed after saving");
    assert!(h.state().dialog.is_none());
    assert!(file_path.exists(), "file was written to disk");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multiple_dirty_workbooks_prompt_sequentially() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Doc 1"})).unwrap();
    h.state_mut().session.new_workbook();
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Doc 2"})).unwrap();
    assert_eq!(h.state().session.documents().len(), 2);

    assert!(!h.state_mut().close_requested());
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));

    // Dismiss first dirty workbook without saving
    click_button(&mut h, "Don't Save");
    assert!(!h.state().quit_confirmed, "quit not confirmed yet while another doc is dirty");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"), "second dirty doc is prompted");

    // Dismiss second dirty workbook without saving
    click_button(&mut h, "Don't Save");
    assert!(h.state().quit_confirmed, "quit confirmed after all dirty docs handled");
    assert!(h.state().dialog.is_none());
}

#[test]
fn command_w_closes_app_when_clean() {
    let mut h = harness(Services::default());
    assert_eq!(h.state().session.documents().len(), 1);
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().quit_confirmed, "Command+W on clean workbook closes the app");
    assert!(h.state().session.documents().is_empty());
}

#[test]
fn command_w_on_dirty_workbook_and_dont_save_closes_app() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Unsaved"})).unwrap();
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));

    click_button(&mut h, "Don't Save");
    assert!(h.state().quit_confirmed, "Don't Save on Command+W closes the app");
    assert!(h.state().dialog.is_none());
    assert!(h.state().session.documents().is_empty());
}
