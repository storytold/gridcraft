//! Scrolling as far as it goes shows the last row and column, at any zoom (issue #3065).

use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS};

/// Scrolls as far as the grid allows and checks the last row's bottom edge and the last column's
/// right edge sit on the far edges of the cells. The scroll offset is an `f32` near 2×10⁷ points at
/// the last row, where one step of `f32` is 2 points, so the edges match to within that, in sheet
/// points (not on-screen ones, which grow with the zoom).
fn assert_end_reachable(h: &mut egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>, what: &str) {
    h.run_steps(10);
    h.state_mut().view_mut().unwrap().scroll = egui::vec2(1.0e9, 1.0e9);
    h.run_steps(10);
    let app = h.state();
    let geo = app.grid.geo.clone().unwrap();
    let wb = app.session.active().unwrap().wb.clone();
    let sh = wb.sheet(wb.active_sheet()).unwrap();
    let last = geo.cell_rect(sh, CellRef::new(MAX_ROWS - 1, MAX_COLS - 1));
    assert!(
        (last.bottom() - geo.cells.bottom()).abs() / geo.z <= 2.0,
        "{what}: the last row ends at {} but the cells end at {}",
        last.bottom(),
        geo.cells.bottom()
    );
    assert!(
        (last.right() - geo.cells.right()).abs() / geo.z <= 2.0,
        "{what}: the last column ends at {} but the cells end at {}",
        last.right(),
        geo.cells.right()
    );
}

#[test]
fn the_last_row_and_column_are_reachable_at_every_zoom() {
    let mut h = crate::support::blank_harness(4);
    for zoom in [10u32, 70, 100, 150, 400] {
        h.state_mut().session.execute("view.zoom", serde_json::json!({"percent": zoom})).unwrap();
        assert_end_reachable(&mut h, &format!("{zoom}%"));
    }
}

#[test]
fn the_last_row_and_column_are_reachable_below_frozen_panes() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().session.execute("selection.set", serde_json::json!({"range": "D6"})).unwrap();
    h.state_mut().session.execute("view.freezePanes", serde_json::json!({})).unwrap();
    for zoom in [70u32, 100, 200] {
        h.state_mut().session.execute("view.zoom", serde_json::json!({"percent": zoom})).unwrap();
        assert_end_reachable(&mut h, &format!("frozen at D6, {zoom}%"));
    }
}
