//! Regional formula dates are selected before a workbook's first recalculation.

use gridcraft_engine::core::{CellError, CellRef, DateSystem, Value};
use gridcraft_engine::{DocState, Prefs, Session, Trust, find_region};
use gridcraft_model::Workbook;
use serde_json::json;

/// Read a calculated cell in the active sheet.
fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// Build the test file entirely in Rust, retaining the default region's cached results.
fn cached_date_workbook(sys: DateSystem, date_text: &str) -> serde_json::Value {
    let mut source = Session::new();
    let mut wb = Workbook::new();
    wb.date_system = sys;
    source.add_document(DocState::new(wb, None, "regional-dates.xlsx".into(), Trust::Local));
    source.execute("cell.set", json!({"cell": "A1", "input": format!("'{date_text}")})).unwrap();
    source.execute("cell.set", json!({"cell": "B1", "input": "=DATEVALUE(A1)"})).unwrap();
    source.execute("cell.set", json!({"cell": "C1", "input": "=MONTH(B1)"})).unwrap();
    source.execute("cell.set", json!({"cell": "D1", "input": format!("=TIMEVALUE(\"{date_text} 18:00\")")})).unwrap();
    source.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()
}

#[test]
/// Keep serialized defaults and accepted region tags compatible with the shared registry.
fn regional_preferences_default_and_validate_supported_tags() {
    assert_eq!(Prefs::default().region, "en-US");
    assert_eq!(serde_json::from_value::<Prefs>(json!({})).unwrap().region, "en-US");
    for (tag, canonical) in [("en-US", "en-US"), ("it_IT", "it-IT"), ("EN-gb", "en-GB"), ("ja-JP", "ja-JP")] {
        assert_eq!(find_region(tag).unwrap().tag, canonical);
    }
    for unsupported in ["", "xx-YY", "it-IT-extra"] {
        assert!(find_region(unsupported).is_err(), "unsupported region {unsupported}");
    }
    let mut s = Session::new();
    s.prefs.region = "not-a-region".into();
    assert_eq!(s.region().tag, "en-US", "unknown saved preferences use the canonical default");
}

#[test]
/// Apply a region chosen before creation to MDY, DMY and YMD formula evaluation.
fn new_workbooks_use_the_regional_preference_before_calculating_formulas() {
    for (region, text, month, day) in [("en-US", "02/01/2026", 2, 1), ("it-IT", "02/01/2026", 1, 2), ("ja-JP", "26/01/02", 1, 2)] {
        let mut s = Session::new();
        s.prefs.region = region.into();
        assert_eq!(s.region().tag, region);
        s.new_workbook();
        assert_eq!(s.doc().unwrap().wb.region.tag, region);
        s.execute("cell.set", json!({"cell": "A1", "input": format!("=DATEVALUE(\"{text}\")")})).unwrap();
        let expected = gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1900, 2026, month, day).unwrap();
        assert_eq!(value(&s, "A1"), Value::Number(expected), "new workbook in {region}");
    }
}

#[test]
/// Recalculate cached date errors and dependent formulas under the selected opening region.
fn opening_recalculates_cached_dates_in_mdy_dmy_and_ymd_for_both_date_systems() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        for (region, text, month, day) in [("en-US", "02/01/2026", 2, 1), ("it-IT", "13/01/2026", 1, 13), ("ja-JP", "26/01/02", 1, 2)] {
            let saved = cached_date_workbook(sys, text);
            let mut s = Session::new();
            s.prefs.region = region.into();
            s.execute("file.open", json!({"name": "regional-dates.xlsx", "base64": saved["base64"]})).unwrap();
            let doc = s.doc().unwrap();
            assert_eq!(doc.wb.region.tag, region);
            assert_eq!(doc.wb.date_system, sys);
            assert!(!doc.is_dirty(), "opening under {region} is not an edit");
            let expected = gridcraft_engine::core::date::serial_from_ymd(sys, 2026, month, day).unwrap();
            assert_eq!(value(&s, "B1"), Value::Number(expected), "DATEVALUE on open in {region}, {sys:?}");
            assert_eq!(value(&s, "C1"), Value::Number(month as f64), "dependent MONTH on open in {region}");
            assert_eq!(value(&s, "D1"), Value::Number(0.75), "TIMEVALUE on open in {region}, {sys:?}");
            assert_eq!(value(&s, "A1"), Value::text(text));
        }
    }
}

#[test]
/// Preserve v0.9.0's explicit CSV text import while applying the session's formula region.
fn csv_text_import_preserves_identifiers_and_uses_the_selected_formula_region() {
    let csv = gridcraft_engine::io::base64_encode(b"00123,13/01/2026\n");
    for as_text in [false, true] {
        let mut s = Session::new();
        s.prefs.region = "it-IT".into();
        s.execute("file.open", json!({"name": "regional-dates.csv", "base64": csv, "csvAsText": as_text})).unwrap();
        assert_eq!(s.doc().unwrap().wb.region.tag, "it-IT");
        assert!(!s.doc().unwrap().is_dirty(), "opening is not an edit");
        assert_eq!(value(&s, "A1"), if as_text { Value::text("00123") } else { Value::Number(123.0) });
        assert_eq!(value(&s, "B1"), Value::text("13/01/2026"));
        s.execute("cell.set", json!({"cell": "C1", "input": "=MONTH(DATEVALUE(B1))"})).unwrap();
        assert_eq!(value(&s, "C1"), Value::Number(1.0));
    }
}

#[test]
/// Treat the region as an application preference rather than an imported XLSX property.
fn xlsx_does_not_replace_the_applications_region() {
    let mut source = Session::new();
    source.prefs.region = "it-IT".into();
    source.new_workbook();
    source.execute("cell.set", json!({"cell": "A1", "input": "=DATEVALUE(\"13/01/2026\")"})).unwrap();
    let saved = source.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "regional-dates.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(reopened.region().tag, "en-US");
    assert_eq!(reopened.doc().unwrap().wb.region.tag, "en-US");
    assert_eq!(value(&reopened, "A1"), Value::Error(CellError::Value));
    assert!(!reopened.doc().unwrap().is_dirty());
}
