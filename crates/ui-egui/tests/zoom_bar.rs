//! The status bar's zoom slider stays put when the percentage gains a digit (99% → 100%), so it
//! doesn't move under the pointer while being dragged.

use egui::vec2;
use egui_kittest::kittest::{Queryable, by};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn slider_at(percent: u32) -> egui::Rect {
    let mut session = Session::new();
    session.new_workbook();
    session.run("view.zoom", json!({"percent": percent})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    let sliders: Vec<egui::Rect> = h.query_all(by().role(egui::accesskit::Role::Slider)).map(|n| n.rect()).collect();
    *sliders.iter().max_by(|a, b| a.min.y.total_cmp(&b.min.y)).expect("the status bar has a zoom slider")
}

#[test]
fn zoom_slider_does_not_move_when_the_percentage_gains_a_digit() {
    let (narrow, wide) = (slider_at(99), slider_at(100));
    assert_eq!(narrow.min.x, wide.min.x, "{narrow:?} vs {wide:?}");
    assert_eq!(slider_at(10).min.x, slider_at(400).min.x);
}
