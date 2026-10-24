//! Whole-column PivotTable sources include appended rows after repeated XLSX saves.
//! All source data is fictional and constructed here; no Excel-authored fixture is used.

use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_model::{DefinedName, Sheet};
use serde_json::json;

fn source(source: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Month", "Amount"], ["2026-01", 17], ["2026-02", 23]]})).unwrap();
    let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    wb.sheets.truncate(1);
    wb.sheets.push(Arc::new(Sheet::new("Report")));
    for (name, scope) in [("LocalColumns", Some(1)), ("GlobalColumns", None)] {
        wb.names.push(DefinedName { name: name.into(), scope, formula: "Sheet1!$A:$B".into(), comment: String::new(), hidden: false });
    }
    s.execute("sheet.activate", json!({"sheet": "Report"})).unwrap();
    s.execute("insert.pivotTable", json!({"source": source, "destination": "Report!A3", "name": "Monthly", "rows": ["Month"], "values": ["Amount"]}))
        .unwrap();
    s
}

#[test]
fn bounded_source_stays_bounded_after_xlsx_round_trip() {
    let mut s = source("Sheet1!$A$1:$B$3");
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    s = Session::new();
    s.execute("file.open", json!({"name": "fictional-bounded-source.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(1).unwrap().pivots[0].source, "Sheet1!$A$1:$B$3");
    s.execute("range.setValues", json!({"range": "Sheet1!A20", "values": [["2026-01", 5]]})).unwrap();
    assert_eq!(s.execute("data.refreshAll", json!({})).unwrap(), json!({"refreshed": 1, "errors": []}));
    assert_report(&s, 17.0, 40.0);
}

fn assert_report(s: &Session, january: f64, total: f64) {
    let sh = s.doc().unwrap().wb.sheet(1).unwrap();
    for (cell, expected) in [
        ("A4", Value::text("2026-01")),
        ("B4", Value::Number(january)),
        ("A5", Value::text("2026-02")),
        ("B5", Value::Number(23.0)),
        ("A6", Value::text("Grand Total")),
        ("B6", Value::Number(total)),
    ] {
        assert_eq!(sh.value(CellRef::parse(cell).unwrap()), expected, "{cell}");
    }
}

#[test]
fn whole_columns_include_appended_rows_after_two_xlsx_round_trips() {
    for (declared, expected) in [
        ("Sheet1!$A:$B", "Sheet1!$A:$B"),
        ("Sheet1!$A$1:$B$1048576", "Sheet1!$A:$B"),
        ("LocalColumns", "Sheet1!$A:$B"),
        ("GlobalColumns", "GlobalColumns"),
    ] {
        let mut s = source(declared);
        assert_report(&s, 17.0, 40.0);
        for (row, amount, january, total) in [(20, 5, 22.0, 45.0), (30, 7, 29.0, 52.0)] {
            let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
            s = Session::new();
            s.execute("file.open", json!({"name": "fictional-whole-columns.xlsx", "base64": saved["base64"]})).unwrap();
            assert_eq!(s.doc().unwrap().wb.sheet(1).unwrap().pivots[0].source, expected, "{declared}, before row {row}");
            // The new row is beyond the used range at the previous save, with a gap on purpose.
            s.execute("range.setValues", json!({"range": format!("Sheet1!A{row}"), "values": [["2026-01", amount]]})).unwrap();
            assert_eq!(s.execute("data.refreshAll", json!({})).unwrap(), json!({"refreshed": 1, "errors": []}));
            assert_report(&s, january, total);
        }
    }
}
