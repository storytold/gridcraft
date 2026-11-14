//! A number stored as text gets Excel's green corner and a warning button whose menu converts it (#1994).

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(active: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "B2", "input": "'123"})).unwrap();
    s.execute("cell.set", json!({"cell": "B3", "input": "'abc"})).unwrap();
    s.execute("selection.set", json!({"cell": active})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

#[test]
fn the_warning_button_converts_a_number_stored_as_text() {
    let mut h = harness("B2");
    h.get_by_label("Error checking options").click();
    h.run_steps(3);
    h.get_by_label("Convert to Number").click();
    h.run_steps(3);
    let v = h.state_mut().session.execute("cell.get", json!({"cell": "B2"})).unwrap()["value"].clone();
    assert_eq!(v, json!(123.0));
    assert!(h.query_by_label("Error checking options").is_none(), "the button goes once the number is a number");
}

#[test]
fn text_that_is_not_a_number_gets_no_button() {
    let h = harness("B3");
    assert!(h.query_by_label("Error checking options").is_none());
}

/// A formula that evaluates to an error gets the same button, naming the error, with Show
/// Calculation Steps; `#N/A` is not flagged, as in Excel.
#[test]
fn a_formula_that_evaluates_to_an_error_gets_the_button() {
    let mut h = harness("B2");
    h.state_mut().session.execute("cell.set", json!({"cell": "B5", "input": "=1/0"})).unwrap();
    h.state_mut().session.execute("cell.set", json!({"cell": "B6", "input": "=NA()"})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "B6"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Error checking options").is_none(), "#N/A is not flagged");
    h.state_mut().session.execute("selection.set", json!({"cell": "B5"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Error checking options").click();
    h.run_steps(3);
    assert!(h.query_by_label("Division by Zero (#DIV/0!)").is_some());
    assert!(h.query_by_label("Convert to Number").is_none());
    h.get_by_label("Show Calculation Steps…").click();
    h.run_steps(3);
    let dialog = h.state().dialog.as_ref().map(|d| d.name.clone());
    assert_eq!(dialog.as_deref(), Some("evaluateFormula"));
}
