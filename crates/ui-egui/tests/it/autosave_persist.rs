//! #1901: AutoSave is a preference — it must survive a restart like the other UI settings.

use gridcraft_engine::Session;

#[test]
/// AutoSave defaults off, toggles on and off, and survives `UiState` serialization.
fn autosave_survives_a_restart() {
    let mut app = crate::common::app(Session::new());
    assert!(!app.ui.autosave, "off by default");
    app.toggle_autosave();
    assert!(app.ui.autosave, "the toggle flips the persisted flag");
    // What the desktop app writes to ui.json, and reads back on the next launch.
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&saved).unwrap();
    assert!(restored.autosave, "a saved ui.json restores AutoSave on (#1901)");
    app.toggle_autosave();
    assert!(!app.ui.autosave, "toggling back persists off too");
}

#[test]
fn an_old_ui_json_without_autosave_still_loads() {
    // Files saved by earlier versions have no autosave key; serde(default) reads them as off.
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(r#"{"dark": true}"#).unwrap();
    assert!(!restored.autosave);
    assert!(restored.dark, "the rest of the old file still applies");
}
