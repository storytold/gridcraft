//! Form dialogs fit their content: OK and Cancel sit right under the last field, not halfway down
//! a window stretched to the screen's height.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
/// OK sits between 0 and 40 points below the last dialog option.
fn ok_and_cancel_sit_under_the_last_field() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.state_mut().open_dialog("cfQuick", json!({"type": "cellIs", "operator": "greaterThan", "title": "Greater Than…"}));
    h.run_steps(4);
    let last = h.get_by_label("Red Border").rect();
    let ok = h.get_by_label_contains("OK").rect();
    let gap = ok.top() - last.bottom();
    assert!((0.0..40.0).contains(&gap), "OK is {gap} points under the last option");
}

#[test]
/// Solver Options: OK sits just under Require Bounds on Variables, so the window fits its
/// content rather than covering the sheet.
fn solver_options_buttons_sit_under_the_last_option() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.state_mut().open_dialog("solver", json!({"optionsView": true}));
    h.run_steps(4);
    let last = h.get_by_label("Require Bounds on Variables").rect();
    let ok = h.get_by_label("  OK  ").rect();
    let gap = ok.top() - last.bottom();
    assert!((0.0..40.0).contains(&gap), "OK is {gap} points under the last option");
    // The Solver Parameters' Solve button sits under its last row too.
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(4);
    let gap = |h: &egui_kittest::Harness<'_, SheetApp>| h.get_by_label("  Solve  ").rect().top() - h.get_by_label("Load/Save").rect().bottom();
    let fresh = gap(&h);
    assert!((0.0..40.0).contains(&fresh), "Solve is {fresh} points under the last row");
    // It stays there when the form gets shorter again: Load/Save shown, then hidden.
    for _ in 0..2 {
        h.get_by_label("Load/Save").click();
        h.run_steps(4);
    }
    assert!((gap(&h) - fresh).abs() < 1.0, "Solve is {} points under the last row, not {fresh}", gap(&h));
}
