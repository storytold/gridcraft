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
/// The selected accounting format survives `UiState` serialization, as written to ui.json.
fn accounting_format_survives_a_restart() {
    let mut app = crate::common::app(Session::new());
    app.ui.accounting_format = "_-[$£-809]* #,##0.00_-;-[$£-809]* #,##0.00_-;_-[$£-809]* \"-\"??_-;_-@_-".into();
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.accounting_format, app.ui.accounting_format);
}

#[test]
/// A code outside the accounting currency list is refused by validation and leaves the preference alone.
fn invalid_accounting_format_does_not_change_the_preference() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    let original = app.ui.accounting_format.clone();
    let result = app.run("ui.accountingFormat", serde_json::json!({"code": "_-[$£-809]* 0.00"}));
    assert_eq!(result.unwrap_err(), "unknown accounting format");
    assert_eq!(app.ui.accounting_format, original);
}

#[test]
/// A ui.json whose accounting format the menu doesn't list (an older version's, or edited by hand)
/// goes back to the default at startup, so the button and Alt+H, A, N keep working.
fn an_unlisted_accounting_format_resets_at_startup() {
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(r#"{"accountingFormat": "_-[$£-809]* 0.00"}"#).unwrap();
    let default = gridcraft_ui_egui::UiState::default().accounting_format;
    assert_eq!(restored.at_startup().accounting_format, default);
}

#[test]
/// A currency picked from the menu is kept even when the selection can't take it (no workbook
/// open here); the formatting error is still reported.
fn a_picked_accounting_format_is_kept_when_formatting_fails() {
    let mut app = crate::common::app(Session::new());
    let pound = "_-[$£-809]* #,##0.00_-;-[$£-809]* #,##0.00_-;_-[$£-809]* \"-\"??_-;_-@_-";
    assert!(app.run("ui.accountingFormat", serde_json::json!({"code": pound})).is_err());
    assert_eq!(app.ui.accounting_format, pound);
}
