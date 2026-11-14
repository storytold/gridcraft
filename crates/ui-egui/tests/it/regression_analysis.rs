//! Regression-specific form/report integration. Shared picker contracts live in ref_edit.rs.
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{
    SheetApp,
    control::{Outcome, handle},
};
use serde_json::json;

#[test]
fn power_model_survives_shared_refedit_collapse_and_control_confirmation() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1","values":[[4,1],[16,2],[36,3],[64,4]]})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.run_steps(4);
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(3);
    h.get_all_by_label("Power / Geometric").last().expect("power model option").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["model"], "power");
    assert!(h.query_by_label("Diagnostics and the intercept use ln(Y); a zero intercept fixes the amplitude to 1.").is_some());
    assert_eq!(h.get_all_by_label("Collapse dialog").count(), 3);
    h.get_all_by_label("Collapse dialog").nth(1).unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), Some("inputX"));
    h.get_by_label("Expand dialog").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["model"], "power");

    // Use the same control path as automation; no private picker or confirmation state.
    let ctx = h.ctx.clone();
    for (field, value) in [("inputY", "A1:A4"), ("inputX", "B1:B4"), ("newSheet", "Fit")] {
        let Outcome::Done(result) = handle(h.state_mut(), &ctx, "ui.dialog.set", &json!({"field":field,"value":value})) else {
            panic!("unexpected screenshot response");
        };
        assert_eq!(result["ok"], true);
    }
    let Outcome::Done(result) = handle(h.state_mut(), &ctx, "ui.dialog.confirm", &json!({})) else {
        panic!("unexpected screenshot response");
    };
    assert_eq!(result["ok"], true);
    assert!(h.state().dialog.is_none());
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let number = |cell| sheet.value(CellRef::parse(cell).unwrap()).as_f64().unwrap();
    assert!((number("B17") - 4.0_f64.ln()).abs() < 1e-10);
    assert!((number("B18") - 2.0).abs() < 1e-10);
    assert_eq!(sheet.value(CellRef::parse("A3").unwrap()), gridcraft_engine::core::Value::text("Regression Statistics (ln(Y) domain)"));
}

#[test]
fn regression_model_popup_keeps_enter_and_escape_inside_the_popup() {
    for key in [egui::Key::Enter, egui::Key::Escape] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1","values":[[2,1],[4,2],[6,3],[8,4]]})).unwrap();
        let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            crate::common::app(session),
        );
        h.run_steps(4);
        h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
        h.run_steps(4);
        let ctx = h.ctx.clone();
        for (field, value) in [("inputY", "A1:A4"), ("inputX", "B1:B4"), ("newSheet", "Fit")] {
            let Outcome::Done(result) = handle(h.state_mut(), &ctx, "ui.dialog.set", &json!({"field":field,"value":value})) else {
                panic!("unexpected dialog-set response");
            };
            assert_eq!(result["ok"], true);
        }
        h.get_by_role(egui::accesskit::Role::ComboBox).click();
        h.run_steps(2);
        h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);

        let dialog = h.state().dialog.as_ref().unwrap_or_else(|| panic!("{key:?} must not confirm or close the dialog"));
        assert_eq!(dialog.values.get("tool").and_then(serde_json::Value::as_str), Some("regression"));
        assert!(h.state().message.is_none(), "valid regression inputs must not produce an error");
    }
}

#[test]
fn rendering_regression_form_preserves_an_invalid_non_string_model() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.state_mut().dialog.as_mut().unwrap().values.insert("model".into(), serde_json::Value::Null);
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("model"), Some(&serde_json::Value::Null));
}
