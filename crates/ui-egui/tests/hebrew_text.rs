//! Hebrew (right-to-left) text in the cell editor: the caret follows the text the user sees.
//!
//! In a cell holding Hebrew, Edit mode's Left and Right arrows move the caret the way they point,
//! as in Excel; Enter mode's arrows still commit and move to the next cell.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
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

fn with_a1(value: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[value]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s
}

fn key(h: &mut egui_kittest::Harness<'static, SheetApp>, k: Key) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

/// F2 at the end of A1, one arrow, Backspace, Enter: what A1 then holds.
fn edit_with_arrow(text: &str, arrow: Key) -> Value {
    let mut h = harness(with_a1(text));
    key(&mut h, Key::F2);
    key(&mut h, arrow);
    key(&mut h, Key::Backspace);
    key(&mut h, Key::Enter);
    value(&h, "A1")
}

#[test]
fn edit_mode_arrows_move_the_way_they_point_in_hebrew() {
    // The caret starts after the last letter, at the left end of "שלום עולם". Right moves it one
    // letter towards the start of the text, so Backspace deletes the letter before that: ל.
    assert_eq!(edit_with_arrow("שלום עולם", Key::ArrowRight), Value::text("שלום עום"));
    // Left at the left end stays, and Backspace deletes the last letter.
    assert_eq!(edit_with_arrow("שלום עולם", Key::ArrowLeft), Value::text("שלום עול"));
}

#[test]
fn edit_mode_arrows_are_unchanged_in_left_to_right_text() {
    assert_eq!(edit_with_arrow("hello", Key::ArrowLeft), Value::text("helo"));
    assert_eq!(edit_with_arrow("hello", Key::ArrowRight), Value::text("hell"));
}

#[test]
fn enter_mode_arrows_still_move_to_the_next_cell_after_hebrew() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = harness(s);
    h.input_mut().events.push(Event::Text("שלום".into()));
    h.run_steps(2);
    key(&mut h, Key::ArrowRight);
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, "A1"), Value::text("שלום"));
    assert_eq!(value(&h, "B1"), Value::text("x"), "Right went to B1, as on a left-to-right sheet");
}
