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

#[test]
fn accounting_format_survives_a_restart() {
    let mut app = crate::common::app(Session::new());
    app.ui.accounting_format = "_-[$£-809]* #,##0.00_-".into();
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.accounting_format, app.ui.accounting_format);
}

#[test]
fn invalid_accounting_format_does_not_change_the_preference() {
    let mut app = crate::common::app(Session::new());
    let original = app.ui.accounting_format.clone();
    assert!(app.run("ui.accountingFormat", serde_json::json!({"code": "_-[$£-809]* 0.00"})).is_err());
    assert_eq!(app.ui.accounting_format, original);
}
