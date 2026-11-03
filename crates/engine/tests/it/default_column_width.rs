use gridcraft_engine::Session;
use serde_json::{Value, json};

fn set_preferred_width(session: &mut Session, width: f64) {
    let mut prefs = serde_json::to_value(&session.prefs).unwrap();
    prefs["defaultColumnWidth"] = json!(width);
    session.prefs = serde_json::from_value(prefs).unwrap();
}

#[test]
fn configured_width_applies_to_every_sheet_in_a_new_workbook() {
    let mut session = Session::new();
    session.prefs.sheets_in_new_workbook = 3;
    set_preferred_width(&mut session, 96.0);

    let index = session.new_workbook();
    let sheets = &session.documents()[index].wb.sheets;

    assert_eq!(sheets.len(), 3);

    for sheet in sheets {
        assert_eq!(sheet.default_col_width, 96.0, "{} must use the configured width", sheet.name);
    }

    let saved = serde_json::to_value(&session.prefs).unwrap();
    assert_eq!(saved.get("defaultColumnWidth").and_then(Value::as_f64), Some(96.0), "The preference must survive JSON serialization");
}

#[test]
fn changing_default_width_does_not_modify_existing_workbooks() {
    let mut session = Session::new();

    let old = session.new_workbook();
    assert_eq!(session.documents()[old].wb.sheets[0].default_col_width, 64.0);

    set_preferred_width(&mut session, 120.0);

    let new = session.new_workbook();

    assert_eq!(session.documents()[new].wb.sheets[0].default_col_width, 120.0, "New workbooks must use the new preference");

    assert_eq!(session.documents()[old].wb.sheets[0].default_col_width, 64.0, "Existing workbooks must retain their original width");
}
#[test]
fn command_updates_new_workbook_column_width() {
    let mut session = Session::new();
    let original = session.new_workbook();

    let command = "options.newWorkbookColumnWidth";

    assert_eq!(session.execute(command, json!({})).unwrap(), json!({"width": 64.0}));

    assert_eq!(session.execute(command, json!({"width": 112.5})).unwrap(), json!({"width": 112.5}));

    for invalid in [json!({"width": -1.0}), json!({"width": 1800.5}), json!({"width": "112"}), json!({"width": true}), json!({"width": null})] {
        assert!(session.execute(command, invalid).is_err(), "Invalid width must be refused");
        assert_eq!(session.prefs.default_column_width, 112.5);
    }

    assert_eq!(session.documents()[original].wb.sheets[0].default_col_width, 64.0, "Changing a preference must not resize an existing workbook");

    let created = session.new_workbook();
    assert_eq!(session.documents()[created].wb.sheets[0].default_col_width, 112.5);

    // The sheet-level command must not change the application preference.
    session.execute("home.defaultWidth", json!({"width": 80.0})).unwrap();
    assert_eq!(session.prefs.default_column_width, 112.5);

    // Preferences must survive save/reload.
    let saved = serde_json::to_string(&session.prefs).unwrap();
    let mut restored = Session::new();
    restored.prefs = serde_json::from_str(&saved).unwrap();
    let next = restored.new_workbook();

    assert_eq!(restored.documents()[next].wb.sheets[0].default_col_width, 112.5);
}
