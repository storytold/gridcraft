//! `ui.dialog.confirm` runs a dialog's command with the parameters its OK button sends (#1459).

use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_engine::model::CfRule;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
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

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn confirming_a_highlight_cells_rule_adds_the_rule_like_ok() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [[1], [3]]})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:A2"})).unwrap();
    // Home › Conditional Formatting › Highlight Cells Rules › Greater Than…
    control(&mut h, "ui.dialog", json!({"name": "cfQuick", "type": "cellIs", "operator": "greaterThan", "title": "Greater Than"}));
    control(&mut h, "ui.dialog.set", json!({"field": "value", "value": "2"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().dialog.is_none(), "the dialog closed");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.cond_formats.len(), 1, "{:?}", sheet.cond_formats);
    let rule = &sheet.cond_formats[0].rule;
    assert!(matches!(rule, CfRule::CellIs { a, .. } if a == "2"), "{rule:?}");
}

#[test]
fn confirming_text_to_columns_splits_on_the_ticked_delimiter() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["a;b"], ["c;d"]]})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:A2"})).unwrap();
    control(&mut h, "ui.dialog", json!({"name": "textToColumns"}));
    control(&mut h, "ui.dialog.set", json!({"field": "semicolon", "value": true}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    let read = h.state_mut().run("sheet.read", json!({"range": "A1:B2"})).unwrap();
    assert_eq!(read["values"], json!([["a", "b"], ["c", "d"]]), "{read}");
}
