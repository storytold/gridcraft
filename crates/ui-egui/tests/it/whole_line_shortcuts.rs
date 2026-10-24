//! Ctrl+Shift+= and Ctrl+- on whole rows or columns insert or delete them at once, as in Excel;
//! any other selection opens the Insert or Delete dialog.

use egui::{Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(selection: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["a1", "b1"], ["a2", "b2"], ["a3", "b3"]]})).unwrap();
    session.execute("selection.set", json!({"range": selection})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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

fn value(h: &Harness, a1: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

fn press(h: &mut Harness, key: Key, shift: bool) {
    let modifiers = Modifiers { shift, ..Modifiers::COMMAND };
    h.key_press_modifiers(modifiers, key);
    h.run_steps(3);
}

#[test]
fn whole_rows_are_inserted_and_deleted_at_once() {
    let mut h = harness("2:2");
    press(&mut h, Key::Equals, true);
    assert!(h.state().dialog.is_none(), "no dialog for whole rows");
    assert!(value(&h, "A2").is_empty(), "a row was inserted above row 2");
    assert_eq!(value(&h, "A3"), Value::from("a2"));
    press(&mut h, Key::Minus, false);
    assert_eq!(value(&h, "A2"), Value::from("a2"), "and deleted again");
}

#[test]
fn whole_columns_are_inserted_and_deleted_at_once() {
    let mut h = harness("A:A");
    press(&mut h, Key::Equals, true);
    assert!(h.state().dialog.is_none(), "no dialog for whole columns");
    assert_eq!(value(&h, "B1"), Value::from("a1"));
    press(&mut h, Key::Minus, false);
    assert_eq!(value(&h, "A1"), Value::from("a1"));
}

#[test]
fn other_selections_still_ask_which_way_to_shift() {
    let mut h = harness("A2:B2");
    press(&mut h, Key::Minus, false);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("deleteCells"));
}
