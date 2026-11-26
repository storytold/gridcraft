//! Regional formula dates are selected before a workbook's first recalculation.

use gridcraft_engine::core::{CellError, CellRef, DateSystem, Value};
use gridcraft_engine::{DocState, Prefs, Session, Trust, find_region};
use gridcraft_model::Workbook;
use serde_json::json;
use std::sync::Arc;

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
/// Preserve explicit CSV text import while applying the selected formula region.
fn csv_text_import_preserves_identifiers_and_uses_the_selected_formula_region() {
    let csv = gridcraft_engine::io::base64_encode(b"00123,13/01/2026\n");
    let mut s = Session::new();
    s.prefs.region = "it-IT".into();
    s.execute("file.open", json!({"name": "regional-dates.csv", "base64": csv, "csvAsText": true})).unwrap();
    assert_eq!(s.doc().unwrap().wb.region.tag, "it-IT");
    assert!(!s.doc().unwrap().is_dirty(), "opening is not an edit");
    assert_eq!(value(&s, "A1"), Value::text("00123"));
    assert_eq!(value(&s, "B1"), Value::text("13/01/2026"));
    s.execute("cell.set", json!({"cell": "C1", "input": "=MONTH(DATEVALUE(B1))"})).unwrap();
    assert_eq!(value(&s, "C1"), Value::Number(1.0));
}

#[test]
/// Keep main's per-column CSV date inference separate from subsequent regional formula parsing.
fn csv_automatic_date_inference_remains_independent_of_formula_region() {
    let csv = gridcraft_engine::io::base64_encode(b"00123,13/01/2026\n");
    let expected = gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1900, 2026, 1, 13).unwrap();
    for (region, expected_day) in [("en-US", 1.0), ("it-IT", 2.0)] {
        let mut s = Session::new();
        s.prefs.region = region.into();
        s.execute("file.open", json!({"name": "regional-dates.csv", "base64": csv, "csvAsText": false})).unwrap();
        assert_eq!(s.doc().unwrap().wb.region.tag, region);
        assert!(!s.doc().unwrap().is_dirty(), "opening is not an edit");
        assert_eq!(value(&s, "A1"), Value::Number(123.0));
        assert_eq!(value(&s, "B1"), Value::Number(expected), "CSV's unambiguous DMY column in {region}");
        s.execute("cell.set", json!({"cell": "C1", "input": "=MONTH(B1)"})).unwrap();
        assert_eq!(value(&s, "C1"), Value::Number(1.0));
        s.execute("cell.set", json!({"cell": "D1", "input": "=DAY(DATEVALUE(\"02/01/2026\"))"})).unwrap();
        assert_eq!(value(&s, "D1"), Value::Number(expected_day), "formula text still uses {region}");
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

#[test]
/// Keep an embedder's cached values when its workbook already uses the session region.
fn saved_workbook_with_matching_region_keeps_cached_values_until_an_edit_reaches_them() {
    let mut source = Session::new();
    source.set_region("it-IT").unwrap();
    source.new_workbook();
    source.execute("range.setValues", json!({"range": "A1", "values": [["'02/01/2026", "=DATEVALUE(A1)", "=DAY(B1)", "=10"]]})).unwrap();
    let mut wb = (*source.doc().unwrap().wb).clone();
    // Deliberately distinct caches prove that attaching the document does not recalculate it.
    for (address, cached) in [("B1", 123.0), ("C1", 77.0), ("D1", 7.0)] {
        let sheet = Arc::make_mut(&mut wb.sheets[0]);
        let at = CellRef::parse(address).unwrap();
        let mut cell = sheet.cell(at).unwrap().clone();
        cell.value = Value::Number(cached);
        sheet.set_cell(at, cell);
    }
    let doc = DocState::from_saved(wb, None, "cached-regional-dates.xlsx".into(), Trust::FromFile);
    let original = doc.wb.clone();
    let mut s = Session::new();
    s.set_region("it_IT").unwrap();
    s.add_document(doc);
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().wb), "a matching region must not replace the cached workbook");
    s.execute("app.region", json!({"region": "it-IT"})).unwrap();
    for (address, cached) in [("B1", 123.0), ("C1", 77.0), ("D1", 7.0)] {
        assert_eq!(value(&s, address), Value::Number(cached));
    }
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("cell.set", json!({"cell": "A1", "input": "'03/01/2026"})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(46025.0));
    assert_eq!(value(&s, "C1"), Value::Number(3.0));
    assert_eq!(value(&s, "D1"), Value::Number(7.0), "an unrelated cached formula remains untouched");
    assert!(s.doc().unwrap().is_dirty());
}

#[test]
/// Apply a different session region to an attached saved document without marking it edited.
fn saved_workbook_with_different_region_recalculates_and_opens_a_window() {
    let mut source = Session::new();
    source.new_workbook();
    source.execute("range.setValues", json!({"range": "A1", "values": [["'02/01/2026", "=DATEVALUE(A1)", "=DAY(B1)"]]})).unwrap();
    let doc = DocState::from_saved((*source.doc().unwrap().wb).clone(), None, "regional-dates.xlsx".into(), Trust::FromFile);
    assert_eq!(doc.wb.active().unwrap().value(CellRef::parse("B1").unwrap()), Value::Number(46054.0));
    let uid = doc.uid;
    let mut s = Session::new();
    s.set_region("it-IT").unwrap();
    assert_eq!(s.add_document(doc), 0);
    assert_eq!(s.windows().len(), 1);
    assert_eq!(s.window(s.active_window().unwrap()).unwrap().doc_uid, uid);
    assert_eq!(s.doc().unwrap().wb.region.tag, "it-IT");
    assert_eq!(value(&s, "B1"), Value::Number(46024.0));
    assert_eq!(value(&s, "C1"), Value::Number(2.0));
    assert!(!s.doc().unwrap().is_dirty());
    assert!(s.doc().unwrap().undo.is_empty());
    assert!(s.doc().unwrap().redo.is_empty());
}
