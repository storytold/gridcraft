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
    let last = h.get_by_label("Load/Save").rect();
    let solve = h.get_by_label("  Solve  ").rect();
    let gap = solve.top() - last.bottom();
    assert!((0.0..40.0).contains(&gap), "Solve is {gap} points under the last row");
}
