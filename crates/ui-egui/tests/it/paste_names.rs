//! F3 opens the Paste Name dialog when the workbook has names: OK puts the selected name into the
//! formula being edited, and Paste List writes every name and what it refers to, as in Excel.

use egui::{Event, Key, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(names: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    if names {
        session.execute("formulas.defineName", json!({"name": "Sales", "refersTo": "=Sheet1!$A$1:$A$3"})).unwrap();
        session.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=0.07"})).unwrap();
    }
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn dialog(h: &Harness) -> Option<&str> {
    h.state().dialog.as_ref().map(|d| d.name())
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn press(h: &mut Harness, key: Key) {
    h.key_press(key);
    h.run_steps(3);
}

#[test]
fn f3_opens_paste_name_only_when_the_workbook_has_names() {
    let mut h = harness(false);
    press(&mut h, Key::F3);
    assert_eq!(dialog(&h), None, "no names: F3 does nothing");
    let mut h = harness(true);
    press(&mut h, Key::F3);
    assert_eq!(dialog(&h), Some("pasteName"));
    // The names are listed in alphabetical order.
    let rate = h.get_by_label("Rate").rect().top();
    let sales = h.get_by_label("Sales").rect().top();
    assert!(rate < sales, "Rate is listed above Sales");
}

#[test]
fn ok_starts_a_formula_with_the_selected_name() {
    let mut h = harness(true);
    press(&mut h, Key::F3);
    h.get_by_label("Sales").click();
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(editor_text(&h).as_deref(), Some("=Sales"));
}

#[test]
fn the_control_channel_confirms_the_dialog_like_ok() {
    let mut h = harness(true);
    let ctx = h.ctx.clone();
    for (method, params) in
        [("ui.dialog", json!({"name": "pasteName"})), ("ui.dialog.set", json!({"field": "name", "value": "Sales"})), ("ui.dialog.confirm", json!({}))]
    {
        match handle(h.state_mut(), &ctx, method, &params) {
            Outcome::Done(v) => assert_eq!(v["ok"], json!(true), "{method}: {v}"),
            Outcome::Screenshot { .. } => panic!("{method} took a screenshot"),
        }
    }
    h.run_steps(2);
    assert_eq!(dialog(&h), None);
    assert_eq!(editor_text(&h).as_deref(), Some("=Sales"));
}

#[test]
fn arrow_keys_choose_the_name_and_leave_the_active_cell() {
    let mut h = harness(true);
    press(&mut h, Key::F3);
    press(&mut h, Key::ArrowDown);
    press(&mut h, Key::Enter);
    assert_eq!(dialog(&h), None);
    assert_eq!(editor_text(&h).as_deref(), Some("=Sales"), "Down moved from Rate to Sales");
    let active = h.state().session.active().unwrap().selection.active;
    assert_eq!(active, CellRef::new(0, 0), "the arrow went to the dialog, not the grid");
}

#[test]
fn while_editing_the_name_goes_in_at_the_caret_and_the_edit_goes_on() {
    let mut h = harness(true);
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(4);
    press(&mut h, Key::F3);
    assert_eq!(dialog(&h), Some("pasteName"), "F3 in the cell editor opens the dialog");
    // While the dialog is open, typed text and arrow keys do not change the formula being edited.
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    press(&mut h, Key::ArrowLeft);
    assert_eq!(editor_text(&h).as_deref(), Some("=SUM("), "the formula is unchanged while the dialog is open");
    h.get_by_label("Sales").click();
    h.run_steps(2);
    press(&mut h, Key::Enter);
    assert_eq!(dialog(&h), None, "Enter is the dialog's OK");
    assert_eq!(editor_text(&h).as_deref(), Some("=SUM(Sales"), "the cell is still being edited");
    assert_eq!(value(&h, "A1"), Value::Empty, "nothing was entered yet");
}

#[test]
fn cancel_leaves_the_selected_text_selected() {
    let mut h = harness(true);
    h.state_mut().begin_edit(Some("=SUM(A1".into()), false);
    // Edit mode (F2): the arrow keys move the caret instead of leaving the cell.
    if let Some(ed) = h.state_mut().editor.as_mut() {
        ed.enter_mode = false;
    }
    h.run_steps(4);
    for _ in 0..2 {
        h.key_press_modifiers(egui::Modifiers::SHIFT, Key::ArrowLeft);
        h.run_steps(2);
    }
    press(&mut h, Key::F3);
    assert_eq!(dialog(&h), Some("pasteName"));
    h.get_by_label(" Cancel ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    h.input_mut().events.push(Event::Text("B2".into()));
    h.run_steps(2);
    assert_eq!(editor_text(&h).as_deref(), Some("=SUM(B2"), "typing replaces the text selected before the dialog opened");
}

#[test]
fn paste_list_writes_the_names_from_the_active_cell() {
    let mut h = harness(true);
    h.state_mut().run("selection.set", json!({"range": "C2"})).unwrap();
    h.run_steps(2);
    press(&mut h, Key::F3);
    h.get_by_label("Paste List").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(value(&h, "C2"), Value::text("Rate"));
    assert_eq!(value(&h, "D2"), Value::text("=0.07"));
    assert_eq!(value(&h, "C3"), Value::text("Sales"));
    assert_eq!(value(&h, "D3"), Value::text("=Sheet1!$A$1:$A$3"));
    assert_eq!(editor_text(&h), None);
}
