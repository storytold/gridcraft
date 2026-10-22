//! A data validation alert over the cell editor answers Enter and Esc itself, and closing it
//! puts the keyboard back in the entry so it can be fixed straight away.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const MESSAGE: &str = "This value doesn't match the data validation restrictions defined for this cell.";

/// A1 holds 5 and accepts whole numbers from 1 to 10 (Stop alert).
fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    s.execute("data.validation", json!({"range": "A1", "type": "whole", "operator": "between", "formula1": "1", "formula2": "10"})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut app = SheetApp::new(s, Default::default());
    app.ui.language = Language::En;
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn key(h: &mut Harness, k: Key) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Types `50` into A1 and presses Enter, which opens the alert.
fn enter_50(h: &mut Harness) {
    h.state_mut().begin_edit(Some("50".into()), false);
    h.run_steps(2);
    key(h, Key::Enter);
    assert!(alert_shown(h));
}

/// Types text the way a keyboard does (into whatever has focus).
fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn alert_shown(h: &Harness) -> bool {
    h.query_by_label(MESSAGE).is_some()
}

fn entry(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn a1(h: &Harness) -> Value {
    h.state().session.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0))
}

/// The first Esc closes the alert and the entry is still there to fix, with the keyboard in it; a
/// second Esc cancels the edit as usual. Before, the first Esc cancelled the edit behind the alert
/// and the second closed it, and then typing went nowhere until the cell was clicked.
#[test]
fn esc_closes_a_validation_alert_and_leaves_the_entry() {
    let mut h = harness();
    enter_50(&mut h);
    key(&mut h, Key::Escape);
    assert!(!alert_shown(&h), "one Esc closes the alert");
    assert_eq!(entry(&h), Some("50".to_string()), "the entry is still open behind it");
    type_text(&mut h, "9");
    assert_eq!(entry(&h), Some("509".to_string()), "typing goes into the entry");
    key(&mut h, Key::Escape);
    assert_eq!(entry(&h), None, "the next Esc cancels the edit");
    assert_eq!(a1(&h), Value::Number(5.0));
}

/// Enter closes the alert too, without touching the entry, and the entry can be fixed and
/// committed from the keyboard. Before, the editor took Enter, tried the same entry again and the
/// alert stayed.
#[test]
fn enter_closes_a_validation_alert_and_leaves_the_entry() {
    let mut h = harness();
    enter_50(&mut h);
    key(&mut h, Key::Enter);
    assert!(!alert_shown(&h), "Enter closes the alert");
    assert_eq!(entry(&h), Some("50".to_string()));
    assert_eq!(a1(&h), Value::Number(5.0));
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Backspace);
    type_text(&mut h, "8");
    assert_eq!(entry(&h), Some("8".to_string()), "the entry is fixed from the keyboard");
    key(&mut h, Key::Enter);
    assert_eq!(entry(&h), None);
    assert_eq!(a1(&h), Value::Number(8.0), "the fixed entry goes in");
    assert!(!alert_shown(&h));
}
