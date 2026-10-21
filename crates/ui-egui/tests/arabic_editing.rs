//! Arabic word navigation must not panic or delete unrelated cell text.

use egui::{Key, Modifiers, vec2};
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn editing(text: &str, formula_bar: bool) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    if formula_bar {
        h.state_mut().begin_edit(None, true);
    } else {
        h.key_press(Key::F2);
    }
    h.run_steps(2);
    h.key_press(Key::End);
    h.run_steps(2);
    h
}

#[test]
fn previous_word_in_arabic_cell_does_not_panic() {
    let mut h = editing("Aurora بب", false);
    h.key_press_modifiers(Modifiers::CTRL, Key::ArrowLeft);
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Text("x".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora xبب"));
}

#[test]
fn delete_previous_arabic_word_keeps_the_latin_prefix() {
    let mut h = editing("Aurora بب", false);
    h.key_press_modifiers(Modifiers::CTRL, Key::Backspace);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora "));
}

#[test]
fn delete_previous_arabic_word_in_formula_bar_keeps_the_prefix() {
    let mut h = editing("Aurora بب", true);
    h.key_press_modifiers(Modifiers::CTRL, Key::Backspace);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora "));
}
