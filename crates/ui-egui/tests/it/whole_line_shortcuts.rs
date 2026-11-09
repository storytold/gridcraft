//! Ctrl+Shift+= (or Ctrl with the numpad's +) and Ctrl+- on whole rows or columns insert or delete them at once, as in Excel;
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
    let mut h = crate::common::builder().with_os(egui::os::OperatingSystem::Windows).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn value(h: &Harness, a1: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

fn press(h: &mut Harness, key: Key, shift: bool) {
    let modifiers = Modifiers { shift, ctrl: true, ..Modifiers::COMMAND };
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
fn numpad_plus_inserts_without_shift() {
    let mut h = harness("2:2");
    press(&mut h, Key::Plus, false);
    assert!(h.state().dialog.is_none(), "no dialog for whole rows");
    assert!(value(&h, "A2").is_empty(), "a row was inserted above row 2");
    assert_eq!(value(&h, "A3"), Value::from("a2"));
}

#[test]
fn unshifted_equals_inserts_nothing() {
    let mut h = harness("2:2");
    press(&mut h, Key::Equals, false);
    assert_eq!(value(&h, "A2"), Value::from("a2"));
}

#[test]
fn other_selections_still_ask_which_way_to_shift() {
    let mut h = harness("A2:B2");
    press(&mut h, Key::Minus, false);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("deleteCells"));
}

#[test]
fn cut_columns_insert_before_the_destination_and_undo_as_one_move() {
    for (source, destination, expected) in [("B:B", "A:A", ["b1", "a1", "c1"]), ("A:A", "C:C", ["b1", "a1", "c1"])] {
        for (key, shift) in [(Key::Equals, true), (Key::Plus, false)] {
            let mut h = harness(source);
            h.state_mut().session.execute("cell.set", json!({"cell": "C1", "input": "c1"})).unwrap();
            h.state_mut().session.execute("cell.set", json!({"cell": "D1", "input": "=A1"})).unwrap();
            h.input_mut().events.push(egui::Event::Cut);
            h.run_steps(2);
            h.state_mut().session.execute("selection.set", json!({"range": destination})).unwrap();
            let undo_before = h.state().session.active().unwrap().undo.len();
            press(&mut h, key, shift);
            assert!(h.state().dialog.is_none() && h.state().message.is_none());
            for (cell, expected) in ["A1", "B1", "C1"].into_iter().zip(expected) {
                assert_eq!(value(&h, cell), Value::from(expected), "{source} before {destination}");
            }
            assert_eq!(value(&h, "D1"), Value::from("a1"), "the formula follows the moved column");
            assert!(h.state().session.clipboard.is_none());
            assert_eq!(h.state().session.active().unwrap().undo.len(), undo_before + 1);
            press(&mut h, Key::Z, false);
            assert_eq!(value(&h, "A1"), Value::from("a1"));
            assert_eq!(value(&h, "B1"), Value::from("b1"));
            assert_eq!(value(&h, "C1"), Value::from("c1"));
        }
    }
}

#[test]
fn copied_columns_and_cut_rows_use_insert_paste() {
    for (source, destination, event, expected) in
        [("A:A", "B1", egui::Event::Copy, ["a1", "a1", "b1"]), ("3:3", "A1", egui::Event::Cut, ["a3", "a1", "a2"])]
    {
        let mut h = harness(source);
        h.input_mut().events.push(event);
        h.run_steps(2);
        h.state_mut().session.execute("selection.set", json!({"range": destination})).unwrap();
        press(&mut h, Key::Equals, true);
        let cells = if source == "A:A" { ["A1", "B1", "C1"] } else { ["A1", "A2", "A3"] };
        for (cell, expected) in cells.into_iter().zip(expected) {
            assert_eq!(value(&h, cell), Value::from(expected));
        }
        assert!(h.state().session.clipboard.is_none());
    }
}

#[test]
fn copied_cell_blocks_open_insert_paste_and_can_shift_right() {
    let mut h = harness("A1");
    h.input_mut().events.push(egui::Event::Copy);
    h.run_steps(2);
    h.state_mut().session.execute("selection.set", json!({"range": "B1"})).unwrap();
    press(&mut h, Key::Equals, true);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("insertPaste"));
    h.state_mut().dialog.as_mut().unwrap().values.insert("shift".into(), json!("right"));
    let ctx = h.ctx.clone();
    gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "ui.dialog.confirm", &json!({}));
    h.run_steps(2);
    assert_eq!(value(&h, "B1"), Value::from("a1"));
    assert_eq!(value(&h, "C1"), Value::from("b1"));
}

#[test]
fn inserting_cut_columns_inside_the_source_keeps_the_data_and_cut() {
    let mut h = harness("A:B");
    h.input_mut().events.push(egui::Event::Cut);
    h.run_steps(2);
    h.state_mut().session.execute("selection.set", json!({"range": "B:B"})).unwrap();
    press(&mut h, Key::Equals, true);
    assert!(h.state().message.is_some() || h.state().toast.is_some());
    assert_eq!(value(&h, "A1"), Value::from("a1"));
    assert_eq!(value(&h, "B1"), Value::from("b1"));
    assert!(h.state().session.clipboard.as_ref().is_some_and(|clip| clip.cut));
}

#[test]
fn copied_nonadjacent_columns_are_refused_without_overwriting_the_destination() {
    let mut h = harness("A:A,C:C");
    h.state_mut().session.execute("cell.set", json!({"cell": "C1", "input": "c1"})).unwrap();
    h.input_mut().events.push(egui::Event::Copy);
    h.run_steps(2);
    h.state_mut().session.execute("selection.set", json!({"range": "B:B"})).unwrap();
    let undo_before = h.state().session.active().unwrap().undo.len();
    press(&mut h, Key::Equals, true);
    assert!(h.state().message.is_some() || h.state().toast.is_some());
    assert_eq!(value(&h, "A1"), Value::from("a1"));
    assert_eq!(value(&h, "B1"), Value::from("b1"));
    assert_eq!(value(&h, "C1"), Value::from("c1"));
    assert!(h.state().session.clipboard.as_ref().is_some_and(|clip| clip.areas.len() == 2));
    assert_eq!(h.state().session.active().unwrap().undo.len(), undo_before);
}
