//! Quick Analysis: where the corner button sits, and when it keeps out of the way.

use egui::{Rect, Vec2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::RangeRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::Geo;
use gridcraft_ui_egui::quick_analysis::{button_rect, is_hidden, toggle};
use serde_json::json;

fn app() -> SheetApp {
    let mut s = Session::new();
    s.new_workbook();
    SheetApp::new(s, Default::default())
}

fn geo(app: &SheetApp) -> Geo {
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0, 800.0)), Vec2::ZERO)
}

#[test]
fn the_button_sits_below_right_of_the_corner_clear_of_the_fill_handle() {
    let app = app();
    let (g, sh) = (geo(&app), app.session.active().unwrap().wb.active().unwrap());
    let sel = RangeRef::parse("B2:D5").unwrap();
    let corner = g.range_rect(sh, sel).right_bottom();
    let (button, visible) = button_rect(&g, sh, sel);
    assert!(visible);
    assert!(button.left() > corner.x && button.top() > corner.y, "below and right of the corner");
    // The fill handle is a 9 x 9 square centred on the corner.
    assert!(!button.intersects(Rect::from_center_size(corner, vec2(9.0, 9.0))), "clear of the fill handle");
}

#[test]
fn at_the_grid_edge_the_button_flips_inside_and_far_corners_are_not_visible() {
    let app = app();
    let (g, sh) = (geo(&app), app.session.active().unwrap().wb.active().unwrap());
    let near_edge = RangeRef::parse("A1:ZZ1").unwrap();
    let (far, visible) = button_rect(&g, sh, near_edge);
    assert!(!visible, "the corner is scrolled out of view");
    assert!(g.cells.contains_rect(far), "the popup still has an anchor inside the grid");
    let (b, _) = button_rect(&g, sh, RangeRef::parse("A1:A200").unwrap());
    assert!(g.cells.contains_rect(b));
}

#[test]
fn the_button_hides_for_single_cells_several_areas_editing_dialogs_and_page_layout() {
    let mut app = app();
    app.run("selection.set", json!({"cell": "B2"})).unwrap();
    let hidden = |app: &SheetApp| {
        let g = geo(app);
        is_hidden(app, &g, app.session.active().unwrap().wb.active().unwrap())
    };
    assert!(hidden(&app), "a single cell");
    app.run("selection.set", json!({"range": "B2:D5"})).unwrap();
    assert!(!hidden(&app));
    app.begin_edit(None, false);
    assert!(hidden(&app), "while editing");
    app.editor = None;
    app.open_dialog("find", json!({}));
    assert!(hidden(&app), "while a dialog is open");
    app.dialog = None;
    assert!(!hidden(&app));
    app.run("view.pageLayout", json!({})).unwrap();
    assert!(hidden(&app), "Page Layout view");
    app.run("view.normal", json!({})).unwrap();
    assert!(!hidden(&app));
}

#[test]
fn several_areas_are_refused_with_a_message_rather_than_guessed() {
    let mut app = app();
    app.run("selection.set", json!({"range": "B2:D5,F2:G5"})).unwrap();
    let err = toggle(&mut app, Some(true)).unwrap_err();
    assert!(err.contains("one block"), "{err}");
    assert!(app.grid.quick_analysis.open.is_none());
    assert!(hidden_for_areas(&app));
}

fn hidden_for_areas(app: &SheetApp) -> bool {
    let g = geo(app);
    is_hidden(app, &g, app.session.active().unwrap().wb.active().unwrap())
}
