//! The region chosen with `app.region.set` is a preference: it is written to `ui.json` and comes
//! back on the next launch instead of the system region.

use gridcraft_engine::Session;
use serde_json::json;

#[test]
fn chosen_region_survives_a_restart() {
    let mut app = crate::common::app(Session::new());
    assert_eq!(app.ui.region, None, "no saved region by default: the system region applies");
    app.run("app.region.set", json!({"region": "pt-BR"})).unwrap();
    // What the desktop app writes to ui.json, and reads back on the next launch.
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.region.as_deref(), Some("pt-BR"));
}
