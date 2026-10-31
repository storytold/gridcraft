//! Cell grid size preference: a persistent scale for the grid alone, like a display DPI setting.
//! It multiplies the sheet zoom on screen; the ribbon and the workbook's saved zoom don't change.

use gridcraft_engine::Session;
use gridcraft_ui_egui::grid::{self, Geo};
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

/// Puts the factor back when the test ends (even on failure): it is per thread, and a test
/// runner may reuse the thread.
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        grid::set_grid_scale(1.0);
    }
}

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    )
}

fn geo(h: &egui_kittest::Harness<'static, SheetApp>) -> Geo {
    let app = h.state();
    let sh = app.session.active().and_then(|d| d.wb.active()).unwrap();
    Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll)
}

#[test]
fn grid_size_scales_cells_and_headings_but_not_the_zoom() {
    let _reset = Reset;
    let mut h = harness();
    h.run_steps(2);
    let before = geo(&h);
    let zoom_factor = h.ctx.zoom_factor();

    assert_eq!(h.state_mut().run("view.gridScale", json!({"percent": 130})).unwrap()["percent"], 130);
    h.run_steps(2);
    let after = geo(&h);
    assert!((after.z / before.z - 1.3).abs() < 1e-4, "cells drawn 30% larger: {} → {}", before.z, after.z);
    assert!(after.header_h > before.header_h, "headings grow with the cells");
    assert_eq!(h.ctx.zoom_factor(), zoom_factor, "the rest of the interface keeps its size");
    let sheet_zoom = h.state().session.active().and_then(|d| d.wb.active()).map(|s| s.zoom);
    assert_eq!(sheet_zoom, Some(100), "the workbook's zoom is unchanged");

    let saved: UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
    assert_eq!(saved.grid_scale, 130);

    // Hostile and missing values: clamped or refused, never applied raw.
    assert_eq!(h.state_mut().run("view.gridScale", json!({"percent": 1e12})).unwrap()["percent"], 300);
    assert_eq!(h.state_mut().run("view.gridScale", json!({"percent": -5})).unwrap()["percent"], 50);
    assert!(h.state_mut().run("view.gridScale", json!({})).is_err(), "no value is an error, not a dialog");
    assert!(h.state_mut().run("view.gridScale", json!({"percent": "huge"})).is_err());
    h.state_mut().run("view.gridScale", json!({"percent": "default"})).unwrap();
    h.run_steps(2);
    assert!((geo(&h).z - before.z).abs() < 1e-6);

    // A malformed, out-of-range or missing value in ui.json keeps the other preferences.
    let ui: UiState = serde_json::from_str(r#"{"dark": true, "gridScale": "big"}"#).unwrap();
    assert!(ui.dark);
    assert_eq!(ui.grid_scale, 100);
    let ui: UiState = serde_json::from_str(r#"{"gridScale": 900}"#).unwrap();
    assert_eq!(ui.grid_scale, 300);
    let ui: UiState = serde_json::from_str(r#"{"dark": true}"#).unwrap();
    assert_eq!(ui.grid_scale, 100);
}

/// Below 100% the headings shrink with the cells: the 75% floor that keeps them readable when
/// zoomed out applies to the workbook zoom, before the Cell grid size.
#[test]
fn headings_shrink_with_a_small_grid_size() {
    let _reset = Reset;
    let mut h = harness();
    h.run_steps(2);
    let before = geo(&h);
    h.state_mut().run("view.gridScale", json!({"percent": 50})).unwrap();
    h.run_steps(2);
    let after = geo(&h);
    assert!((after.header_h - before.header_h * 0.5).abs() <= 1.0, "column headings at 50%: {} → {}", before.header_h, after.header_h);
    assert!((after.header_w - before.header_w * 0.5).abs() <= 1.0, "row headings at 50%: {} → {}", before.header_w, after.header_w);
}
