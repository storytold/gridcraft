//! Scrolling as far as possible always reaches the last row and column, at any zoom (#3065).

use gridcraft_engine::core::{MAX_COLS, MAX_ROWS};

#[test]
fn last_row_and_column_are_reachable_at_any_zoom() {
    let mut h = crate::support::blank_harness(4);
    for zoom in [100u32, 70, 200] {
        h.state_mut().session.execute("view.zoom", serde_json::json!({"percent": zoom})).expect("view.zoom");
        h.run_steps(30);
        h.state_mut().view_mut().unwrap().scroll = egui::vec2(1.0e9, 1.0e9);
        h.run_steps(10);
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = app.grid.geo.as_ref().unwrap();
        let scroll = app.view().scroll;
        let end_y = sheet.row_top(MAX_ROWS) as f32 - (geo.cells.height() - geo.frozen_h) / geo.z;
        let end_x = sheet.col_left(MAX_COLS) as f32 - (geo.cells.width() - geo.frozen_w) / geo.z;
        assert!(scroll.y >= end_y - 1.0, "zoom {zoom}: scroll.y {} stops before the last row (end {end_y})", scroll.y);
        assert!(scroll.x >= end_x - 1.0, "zoom {zoom}: scroll.x {} stops before the last column (end {end_x})", scroll.x);
        assert!(scroll.y <= end_y + 1.0, "zoom {zoom}: scroll.y {} runs past the last row (end {end_y})", scroll.y);
        assert!(scroll.x <= end_x + 1.0, "zoom {zoom}: scroll.x {} runs past the last column (end {end_x})", scroll.x);
    }
}
