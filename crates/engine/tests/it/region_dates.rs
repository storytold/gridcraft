//! Region changes recalculate formula dates without changing canonical command input.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellError, CellRef, Value};
use serde_json::json;

/// Read a calculated cell in the active sheet.
fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// Produce a clean, opened workbook whose formula depends on the regional date order.
fn clean_workbook() -> Session {
    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({"cell": "A1", "input": "=DATEVALUE(\"02/01/2026\")"})).unwrap();
    source.execute("cell.set", json!({"cell": "A2", "input": "=DAY(A1)"})).unwrap();
    let saved = source.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "regional-dates.xlsx", "base64": saved["base64"]})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    s
}

#[test]
/// Accept canonical region aliases and reject unknown tags without changing existing values.
fn region_command_validates_tags_and_works_before_a_document_is_created() {
    let mut s = Session::new();
    assert_eq!(s.execute("app.region", json!({})).unwrap()["region"], "en-US");
    assert_eq!(s.execute("app.region", json!({"region": "it_IT"})).unwrap()["region"], "it-IT");
    assert_eq!(s.execute("app.region", json!({"region": null})).unwrap()["region"], "it-IT");
    for invalid in [json!(42), json!(true), json!([]), json!({})] {
        assert!(s.execute("app.region", json!({"region": invalid})).is_err());
        assert_eq!(s.region().tag, "it-IT");
    }
    for invalid in ["", "xx-YY", "it-IT-extra"] {
        assert!(s.execute("app.region", json!({"region": invalid})).is_err());
        assert_eq!(s.execute("app.region", json!({})).unwrap()["region"], "it-IT");
    }
    s.new_workbook();
    assert_eq!(s.doc().unwrap().wb.region.tag, "it-IT");
    s.execute("cell.set", json!({"cell": "A1", "input": "=DATEVALUE(\"13/01/2026\")"})).unwrap();
    assert_eq!(value(&s, "A1"), Value::Number(46035.0));
    for invalid in ["", "xx-YY"] {
        assert!(s.set_region(invalid).is_err());
        assert_eq!(s.region().tag, "it-IT");
        assert_eq!(value(&s, "A1"), Value::Number(46035.0));
    }
}

#[test]
/// Recalculate every open document while retaining each document's existing saved state.
fn region_changes_recalculate_dependents_and_preserve_clean_and_dirty_documents() {
    let mut s = clean_workbook();
    let clean_index = 0;
    s.new_workbook();
    let dirty_index = s.documents().len() - 1;
    s.execute("cell.set", json!({"cell": "A1", "input": "=DATEVALUE(\"02/01/2026\")"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "=DAY(A1)"})).unwrap();
    assert!(s.doc().unwrap().is_dirty());
    for (region, expected, day) in [("it-IT", 46024.0, 2.0), ("en-US", 46054.0, 1.0), ("en-GB", 46024.0, 2.0)] {
        s.set_region(region).unwrap();
        for index in [clean_index, dirty_index] {
            s.set_active(index);
            assert_eq!(s.doc().unwrap().wb.region.tag, region);
            assert_eq!(value(&s, "A1"), Value::Number(expected));
            assert_eq!(value(&s, "A2"), Value::Number(day));
            assert_eq!(s.doc().unwrap().is_dirty(), index == dirty_index);
        }
    }
}

#[test]
/// Keep history snapshots on the current region and preserve undo's return to the save point.
fn undo_and_redo_keep_the_current_region_and_the_original_save_point() {
    let mut s = clean_workbook();
    s.execute("cell.set", json!({"cell": "B1", "input": "5"})).unwrap();
    assert!(s.doc().unwrap().is_dirty());
    s.set_region("it-IT").unwrap();
    assert!(s.doc().unwrap().is_dirty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Empty);
    assert_eq!(value(&s, "A1"), Value::Number(46024.0));
    assert_eq!(value(&s, "A2"), Value::Number(2.0));
    assert_eq!(s.doc().unwrap().wb.region.tag, "it-IT");
    assert!(!s.doc().unwrap().is_dirty(), "undo to the saved snapshot stays clean after a region change");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(5.0));
    assert_eq!(value(&s, "A1"), Value::Number(46024.0));
    assert!(s.doc().unwrap().is_dirty());
    s.set_region("en-US").unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "A1"), Value::Number(46054.0));
    assert_eq!(value(&s, "A2"), Value::Number(1.0));
    assert!(!s.doc().unwrap().is_dirty(), "changing back also preserves the saved snapshot");
    s.set_region("ja-JP").unwrap();
    assert!(!s.doc().unwrap().is_dirty());
}

#[test]
/// Keep programmatic cell and range input canonical when formula interpretation changes.
fn canonical_command_input_is_independent_of_the_formula_region() {
    for region in ["it-IT", "ja-JP"] {
        let mut s = Session::new();
        s.set_region(region).unwrap();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "02/01/2026"})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "1,234.5"})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "13/01/2026"})).unwrap();
        s.execute("range.setValues", json!({"range": "B1", "values": [["02/01/2026", "1,234.5"]]})).unwrap();
        assert_eq!(value(&s, "A1"), Value::Number(46054.0), "canonical cell.set in {region}");
        assert_eq!(value(&s, "A2"), Value::Number(1234.5));
        assert_eq!(value(&s, "A3"), Value::text("13/01/2026"));
        assert_eq!(value(&s, "B1"), Value::Number(46054.0), "canonical range.setValues in {region}");
        assert_eq!(value(&s, "C1"), Value::Number(1234.5));
    }
}

#[test]
/// Round-trip F2 editor text through the canonical input parser without changing date serials.
fn editing_an_imported_numeric_date_keeps_its_value_in_every_region() {
    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({"cell": "A1", "input": 46024})).unwrap();
    source.execute("home.numberFormat", json!({"range": "A1", "code": "m/d/yyyy"})).unwrap();
    let saved = source.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    for region in ["en-US", "it-IT", "ja-JP"] {
        let mut s = Session::new();
        s.set_region(region).unwrap();
        s.execute("file.open", json!({"name": "imported-date.xlsx", "base64": saved["base64"]})).unwrap();
        let editor_text = {
            let d = s.doc().unwrap();
            let sh = d.wb.active().unwrap();
            let at = CellRef::parse("A1").unwrap();
            assert_eq!(gridcraft_engine::display::cell_text(&d.wb, sh, at), "1/2/2026");
            gridcraft_engine::display::edit_text(&d.wb, sh, at)
        };
        s.execute("cell.set", json!({"cell": "A1", "input": editor_text})).unwrap();
        assert_eq!(value(&s, "A1"), Value::Number(46024.0), "F2 then Enter in {region}");
    }
}

/// Generate the original date-text source and PivotTable without a binary fixture.
fn datevalue_month_pivot(january_text: &str) -> Session {
    let mut s = Session::new();
    s.set_region("it-IT").unwrap();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Date text", "Date", "Month", "Amount"],
            [format!("'{january_text}"), "=DATEVALUE(A2)", "=YEAR(B2)&\"-\"&RIGHT(\"00\"&MONTH(B2),2)", 17],
            ["'02/02/2026", "=DATEVALUE(A3)", "=YEAR(B3)&\"-\"&RIGHT(\"00\"&MONTH(B3),2)", 23]
        ]}),
    )
    .unwrap();
    s.execute(
        "insert.pivotTable",
        json!({"source": "Sheet1!$A$1:$D$3", "destination": "Sheet1!F3", "name": "Months", "rows": ["Month"], "columns": [], "values": ["Amount"]}),
    )
    .unwrap();
    s
}

/// Find a report label and assert its adjacent numeric total.
fn assert_report_value(s: &Session, label: &str, expected: f64) {
    let sh = s.doc().unwrap().wb.active().unwrap();
    let range = sh.pivots[0].last_range.unwrap();
    let found: Vec<_> = range.iter().filter(|c| sh.value(*c) == Value::text(label)).collect();
    assert_eq!(found.len(), 1, "one report row for {label}");
    let at = found[0];
    assert_eq!(sh.value(CellRef::new(at.row, at.col + 1)), Value::Number(expected), "report value for {label}");
}

/// Assert source values, source formulas and all three fictional report totals.
fn assert_datevalue_month_report(s: &Session, january_text: &str, january_serial: f64) {
    for (cell, expected) in [
        ("A2", Value::text(january_text)),
        ("B2", Value::Number(january_serial)),
        ("C2", Value::text("2026-01")),
        ("A3", Value::text("02/02/2026")),
        ("B3", Value::Number(46055.0)),
        ("C3", Value::text("2026-02")),
        ("D2", Value::Number(17.0)),
        ("D3", Value::Number(23.0)),
    ] {
        assert_eq!(value(s, cell), expected, "{january_text}: {cell}");
    }
    assert_report_value(s, "2026-01", 17.0);
    assert_report_value(s, "2026-02", 23.0);
    assert_report_value(s, "Grand Total", 40.0);
    let sh = s.doc().unwrap().wb.active().unwrap();
    for (cell, formula) in [
        ("B2", "DATEVALUE(A2)"),
        ("C2", "YEAR(B2)&\"-\"&RIGHT(\"00\"&MONTH(B2),2)"),
        ("B3", "DATEVALUE(A3)"),
        ("C3", "YEAR(B3)&\"-\"&RIGHT(\"00\"&MONTH(B3),2)"),
    ] {
        assert_eq!(sh.cell(CellRef::parse(cell).unwrap()).unwrap().formula.as_ref().unwrap().text, formula, "{january_text}: {cell}");
    }
}

#[test]
/// Preserve both ambiguous and unambiguous January dates through two XLSX refresh cycles.
fn regional_datevalue_keeps_january_after_xlsx_open_and_pivot_refresh() {
    for (january_text, january_serial) in [("13/01/2026", 46035.0), ("02/01/2026", 46024.0)] {
        let mut s = datevalue_month_pivot(january_text);
        assert_datevalue_month_report(&s, january_text, january_serial);
        for _ in 0..2 {
            let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
            let mut reopened = Session::new();
            reopened.set_region("it-IT").unwrap();
            reopened.execute("file.open", json!({"name": "months.xlsx", "base64": saved["base64"]})).unwrap();
            assert_datevalue_month_report(&reopened, january_text, january_serial);
            assert!(!reopened.doc().unwrap().is_dirty());
            let refreshed = reopened.execute("data.refreshAll", json!({})).unwrap();
            assert_eq!(refreshed, json!({"refreshed": 1, "errors": []}));
            assert_datevalue_month_report(&reopened, january_text, january_serial);
            s = reopened;
        }
    }
}

#[test]
/// Recover the missing January row by changing the region and explicitly refreshing the report.
fn changing_region_recovers_january_without_rewriting_source_formulas() {
    let mut source = datevalue_month_pivot("13/01/2026");
    let saved = source.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "months.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(value(&s, "B2"), Value::Error(CellError::Value));
    s.execute("data.refreshAll", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.pivots[0].last_range.unwrap().iter().all(|c| sh.value(c) != Value::text("2026-01")));
    s.execute("app.region", json!({"region": "it-IT"})).unwrap();
    assert_eq!(value(&s, "B2"), Value::Number(46035.0));
    let refreshed = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(refreshed, json!({"refreshed": 1, "errors": []}));
    assert_datevalue_month_report(&s, "13/01/2026", 46035.0);
}
