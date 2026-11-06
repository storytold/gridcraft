//! Paste Special: options in two columns, translated, OK pastes with them and Paste Link links.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[2], [3]]})).unwrap();
    s.execute("range.setValues", json!({"range": "C1", "values": [[10], [10]]})).unwrap();
    s.execute("edit.copy", json!({"range": "A1:A2"})).unwrap();
    s.execute("selection.set", json!({"cell": "C1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.state_mut().open_dialog("pasteSpecial", json!({}));
    h.run_steps(4);
    h
}

fn cell(h: &mut Harness, a1: &str) -> serde_json::Value {
    h.state_mut().session.execute("cell.get", json!({"cell": a1})).unwrap()
}

#[test]
fn an_operation_chosen_in_the_dialog_is_applied() {
    let mut h = harness();
    h.get_by_label("Values (V)").click();
    h.get_by_label("Multiply").click();
    h.run_steps(3);
    h.get_by_label_contains("OK").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(cell(&mut h, "C1")["value"], json!(20.0));
    assert_eq!(cell(&mut h, "C2")["value"], json!(30.0));
}

#[test]
fn paste_link_pastes_references_at_once() {
    let mut h = harness();
    h.get_by_label("Paste Link").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let c1 = cell(&mut h, "C1");
    assert_eq!(c1["value"], json!(2.0));
    assert!(c1["formula"].as_str().is_some_and(|f| f.contains("A1")), "{c1}");
}

fn press(h: &mut Harness, key: egui::Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(2);
}

fn value(h: &Harness, field: &str) -> serde_json::Value {
    h.state().dialog.as_ref().and_then(|d| d.values.get(field).cloned()).unwrap_or_default()
}

#[test]
fn every_option_has_its_excel_access_key() {
    // #1482: only F, V, T, C and E worked.
    let mut h = harness();
    for (key, field, want) in [
        (egui::Key::N, "what", json!("validation")),
        (egui::Key::X, "what", json!("allExceptBorders")),
        (egui::Key::W, "what", json!("columnWidths")),
        (egui::Key::R, "what", json!("formulasAndNumberFormats")),
        (egui::Key::U, "what", json!("valuesAndNumberFormats")),
        (egui::Key::A, "what", json!("all")),
        (egui::Key::D, "operation", json!("add")),
        (egui::Key::S, "operation", json!("subtract")),
        (egui::Key::I, "operation", json!("divide")),
        (egui::Key::O, "operation", json!("none")),
        (egui::Key::M, "operation", json!("multiply")),
        (egui::Key::B, "skipBlanks", json!(true)),
    ] {
        press(&mut h, key);
        assert_eq!(value(&h, field), want, "{key:?}");
    }
    press(&mut h, egui::Key::B);
    assert_eq!(value(&h, "skipBlanks"), json!(false), "B flips Skip blanks back");
    // V then Enter: values multiplied, as with the mouse.
    press(&mut h, egui::Key::V);
    press(&mut h, egui::Key::Enter);
    assert!(h.state().dialog.is_none());
    assert_eq!(cell(&mut h, "C1")["value"], json!(20.0));
    // L is the Paste Link button: it pastes at once.
    let mut h = harness();
    press(&mut h, egui::Key::L);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert!(cell(&mut h, "C1")["formula"].as_str().is_some_and(|f| f.contains("A1")));
}
