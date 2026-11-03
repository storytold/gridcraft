//! Advanced Filter's dialog must route both actions to the same engine command.

use egui_kittest::kittest::Queryable;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(6)
}

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
/// Checks that the copy destination field appears and that a unique extract writes `B` to C3, closing the dialog without a message.
fn dialog_offers_copy_action_and_runs_unique_extract() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Name"], ["A"], ["A"], ["B"]]})).unwrap();
    h.state_mut().open_dialog("advancedFilter", json!({}));
    h.run_steps(6);
    h.get_by_label("Copy matching rows elsewhere").click();
    h.run_steps(6);
    let rect = h.ctx.memory(|m| m.area_rect(egui::Id::unique("dialog").with("advancedFilter"))).unwrap();
    assert!(rect.width() < 500.0 && rect.height() < 400.0, "dialog must fit its controls: {rect:?}");
    assert!(
        h.state()
            .dialog
            .as_ref()
            .is_some_and(|d| d.fields.iter().any(|f| matches!(f, gridcraft_ui_egui::dialogs::Field::Text { key: "copyTo", .. })))
    );
    assert_eq!(h.state().dialog.as_ref().unwrap().values["range"], "A1:A4");
    control(&mut h, "ui.dialog.set", json!({"field": "copyTo", "value": "C1"}));
    control(&mut h, "ui.dialog.set", json!({"field": "unique", "value": true}));
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none());
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("C3").unwrap()).display(), "B");
}

#[test]
fn in_place_action_does_not_use_stale_copy_destination() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Name"], ["A"], ["B"]]})).unwrap();
    h.state_mut().run("range.setValues", json!({"range": "H1", "values": [["Name"], ["B"]]})).unwrap();
    control(&mut h, "ui.dialog", json!({"name": "advancedFilter"}));
    control(&mut h, "ui.dialog.set", json!({"field": "range", "value": "A1:A3"}));
    control(&mut h, "ui.dialog.set", json!({"field": "criteria", "value": "H1:H2"}));
    control(&mut h, "ui.dialog.set", json!({"field": "copyTo", "value": "K1"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.state().session.active().unwrap().wb.active().unwrap().is_row_hidden(1));
    assert!(h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("K1").unwrap()).is_empty());
}
