//! The strip beside the column headings, above the vertical scrollbar, takes the bar colour in
//! dark mode instead of showing a light square (#2086).

use egui::{Shape, pos2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme::Tokens};
use serde_json::json;

#[test]
fn dark_theme_fills_the_strip_right_of_the_last_column_heading() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.state_mut().run("view.theme", json!({"mode": "dark"})).unwrap();
    h.run_steps(6);
    let grid = h.state().grid.rect.expect("grid rect");
    // Inside the scrollbar column, at the height of the column headings.
    let probe = pos2(grid.right() + 7.0, grid.top() + 5.0);
    let fill = Tokens::dark().tab_bar;
    let covered = h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r) if r.fill == fill && r.rect.contains(probe)));
    assert!(covered, "no tab_bar fill covers {probe:?}");
}
