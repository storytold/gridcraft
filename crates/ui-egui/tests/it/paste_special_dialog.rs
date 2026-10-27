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
