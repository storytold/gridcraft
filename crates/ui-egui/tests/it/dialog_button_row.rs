//! Form dialogs fit their content: OK and Cancel sit right under the last field, not halfway down
//! a window stretched to the screen's height.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
fn ok_and_cancel_sit_under_the_last_field() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.state_mut().open_dialog("cfQuick", json!({"type": "cellIs", "operator": "greaterThan", "title": "Greater Than…"}));
    h.run_steps(4);
    let last = h.get_by_label("Red Border").rect();
    let ok = h.get_by_label_contains("OK").rect();
    let gap = ok.top() - last.bottom();
    assert!((0.0..40.0).contains(&gap), "OK is {gap} points under the last option");
}
