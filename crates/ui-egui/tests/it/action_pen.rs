//! Draw › Action Pen: the model panel stays reachable once the model is loaded.
//!
//! The Action Pen button only toggles the tool when the model is ready, so a separate control has
//! to open model management (Download, Import…, Remove) without touching the tool.

use egui_kittest::kittest::Queryable;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    crate::support::blank_harness(4)
}

#[test]
fn the_model_control_opens_management_without_toggling_the_tool() {
    let mut h = harness();
    h.state_mut().ui.ribbon_tab = "Draw".into();
    h.run_steps(2);
    h.get_by_label("Model…").click();
    h.run_steps(1);
    assert!(h.state().grid.ink_panel, "the Model… control opens the model panel");
    assert_ne!(h.state().session.draw_tool, "actionPen", "and does not select the tool");
}
