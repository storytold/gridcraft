//! Ribbon dropdown menus share one row style: 30 px rows, all as wide as the menu card.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "A1", "values": [[3], [1], [2]]})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

#[test]
fn menu_rows_are_thirty_points_tall_and_as_wide_as_the_card() {
    let mut h = harness();
    h.get_by_label("Sort & Filter").click();
    h.run_steps(6);
    let rows: Vec<egui::Rect> =
        ["Sort A to Z", "Sort Z to A", "Custom Sort…", "Filter", "Reapply"].iter().map(|l| h.get_by_label(l).rect()).collect();
    for r in &rows {
        assert!((r.height() - 30.0).abs() < 0.5, "row height {}", r.height());
        assert!((r.width() - rows[0].width()).abs() < 0.5, "rows share the card's width");
    }
    // A group gap separates Custom Sort… from Filter, wider than the touching rows above it.
    assert!(rows[1].top() - rows[0].bottom() < 0.5);
    assert!(rows[3].top() - rows[2].bottom() >= 6.0);
}

#[test]
fn border_presets_are_menu_rows_that_still_apply() {
    let mut h = harness();
    h.get_by_label("Border presets").click();
    h.run_steps(6);
    let all = h.get_by_label("All Borders");
    assert!((all.rect().height() - 30.0).abs() < 0.5);
    all.click();
    h.run_steps(4);
    assert_eq!(h.state().grid.last_border, "all");
}
