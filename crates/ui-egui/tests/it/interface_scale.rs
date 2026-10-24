//! Interface size preference (#230): a persistent zoom for the whole window, clamped.

use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

#[test]
fn interface_size_applies_persists_and_clamps() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.0);

    h.state_mut().run("view.interfaceScale", json!({"percent": 125})).unwrap();
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    let saved: UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
    assert_eq!(saved.interface_scale, 125);

    // Hostile and missing values: clamped or refused, never applied raw.
    assert_eq!(h.state_mut().run("view.interfaceScale", json!({"percent": 1e12})).unwrap()["percent"], 200);
    assert_eq!(h.state_mut().run("view.interfaceScale", json!({"percent": -5})).unwrap()["percent"], 75);
    assert!(h.state_mut().run("view.interfaceScale", json!({})).is_err(), "no value is an error, not a dialog");
    assert!(h.state_mut().run("view.interfaceScale", json!({"percent": "huge"})).is_err());
    h.state_mut().run("view.interfaceScale", json!({"percent": "auto"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.0);

    // A malformed or out-of-range ui.json keeps the other preferences.
    let ui: UiState = serde_json::from_str(r#"{"dark": true, "interfaceScale": "big"}"#).unwrap();
    assert!(ui.dark);
    assert_eq!(ui.interface_scale, 100);
    let ui: UiState = serde_json::from_str(r#"{"interfaceScale": 900}"#).unwrap();
    assert_eq!(ui.interface_scale, 200);
}
