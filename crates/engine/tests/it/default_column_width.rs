use gridcraft_engine::Session;
use serde_json::{Value, json};

/// Helper that writes `defaultColumnWidth` into `Session::prefs` via JSON round-trip.
fn set_preferred_width(session: &mut Session, width: f64) {
    let mut prefs = serde_json::to_value(&session.prefs).unwrap();
    prefs["defaultColumnWidth"] = json!(width);
    session.prefs = serde_json::from_value(prefs).unwrap();
}

/// Every sheet in a newly created workbook inherits `Prefs::default_column_width`.
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

/// Updating the new-workbook default width leaves already-open workbooks untouched.
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

/// `options.newWorkbookColumnWidth` validates bounds and persists across preference reloads.
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

/// Hand-edited invalid `defaultColumnWidth` falls back to 64 without discarding other preferences,
/// and a configured width of `0` survives an XLSX save and reopen.
#[test]
fn invalid_pref_keeps_sibling_settings_and_zero_width_roundtrips_xlsx() {
    let prefs: gridcraft_engine::Prefs = serde_json::from_value(json!({
        "enterDirection": "right",
        "defaultColumnWidth": "wide"
    }))
    .unwrap();
    assert_eq!(prefs.enter_direction, "right");
    assert_eq!(prefs.default_column_width, 64.0);

    let mut session = Session::new();
    session.execute("options.newWorkbookColumnWidth", json!({"width": 0.0})).unwrap();
    let idx = session.new_workbook();
    assert_eq!(session.documents()[idx].wb.sheets[0].default_col_width, 0.0);

    let bytes = gridcraft_xlsx::write_xlsx(&session.documents()[idx].wb).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("zero.xlsx", &bytes).unwrap();
    assert_eq!(reopened.sheets[0].default_col_width, 0.0, "explicit zero default column width must survive XLSX round-trip");

    // Malformed JSON syntax errors still fail deserialization, while tiny negative floats fall back to 64.
    assert!(serde_json::from_str::<gridcraft_engine::Prefs>(r#"{"enterDirection":"right","defaultColumnWidth":}"#).is_err());
    let neg_prefs: gridcraft_engine::Prefs = serde_json::from_str(r#"{"defaultColumnWidth":-1e-50}"#).unwrap();
    assert_eq!(neg_prefs.default_column_width, 64.0);

    // Positive subpixel widths remain positive across XLSX round-trip rather than collapsing to zero,
    // including styled `<col>` records that inherit the sheet's default column width.
    let mut subpx = Session::new();
    subpx.execute("options.newWorkbookColumnWidth", json!({"width": 0.4})).unwrap();
    let sidx = subpx.new_workbook();
    subpx.execute("home.bold", json!({"range": "B:B"})).unwrap();
    let subpx_bytes = gridcraft_xlsx::write_xlsx(&subpx.documents()[sidx].wb).unwrap();
    let (reopened_subpx, _) = gridcraft_engine::io::open_bytes("subpx.xlsx", &subpx_bytes).unwrap();
    let reopened_sh = &reopened_subpx.sheets[0];
    assert!(reopened_sh.default_col_width > 0.0, "positive subpixel width must stay positive on reopen");
    assert_eq!(reopened_sh.cols.get(&1).and_then(|c| c.size), None, "styled column B must keep inheriting default width");
    assert!(reopened_sh.col_width(1) > 0.0, "inherited column B width must stay positive on reopen");
}
