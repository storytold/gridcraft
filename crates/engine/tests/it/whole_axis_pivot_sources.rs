//! Whole-column PivotTable sources include appended rows after repeated XLSX saves.
//! All source data is fictional and constructed here; no Excel-authored fixture is used.

use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_model::{Cell, DefinedName, Sheet};
use serde_json::json;

/// Builds a fictional two-month source and a separate report using the requested source reference.
/// Sheet-local and workbook-scoped names let the same fixture exercise name resolution.
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

/// A saved fixed rectangle excludes appended rows and retains January 17 and the grand total 40.
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

/// Checks report labels and totals, including the unchanged February amount of 23.
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

/// Whole-column references and names retain their extent through two saves and refresh new rows.
/// January grows from 17 to 22 to 29 as amounts 5 and 7 are appended beyond each saved used range.
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

/// An over-budget cache fails all XLSX save paths before overwriting files or marking work saved.
#[test]
fn oversized_pivot_cache_save_preserves_files_and_unsaved_document() {
    let dir = std::env::temp_dir().join(format!("gridcraft-pivot-cache-budget-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let original_path = dir.join("original.xlsx");
    let save_as_path = dir.join("save-as.xlsx");
    let mut s = source("Sheet1!$A:$B");
    s.execute("file.saveAs", json!({"path": original_path})).unwrap();
    let original_bytes = std::fs::read(&original_path).unwrap();
    std::fs::write(&save_as_path, &original_bytes).unwrap();
    assert!(!s.doc().unwrap().is_dirty());

    // Model an imported wide, sparse source without refreshing or allocating the whole range.
    let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    wb.sheet_mut(1).unwrap().pivots[0].source = "Sheet1!$A:$XFD".into();
    wb.sheet_mut(0).unwrap().cells.set(CellRef::parse("XFD2000").unwrap(), Cell::value(Value::Number(17.0)));
    let d = s.doc().unwrap();
    let (before, saved, path, revision, undo, redo) = (d.wb.clone(), d.saved.clone(), d.path.clone(), d.revision, d.undo.len(), d.redo.len());
    assert!(d.is_dirty());

    for (command, params) in [("file.save", json!({})), ("file.saveAs", json!({"path": save_as_path})), ("file.saveBytes", json!({"format": "xlsx"}))]
    {
        let result = s.execute(command, params);
        assert!(result.is_err(), "{command} must report the oversized cache");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("PivotTable"), "{error}");
        assert!(error.contains("10000000"), "{error}");
        assert_eq!(std::fs::read(&original_path).unwrap(), original_bytes, "{command}");
        assert_eq!(std::fs::read(&save_as_path).unwrap(), original_bytes, "{command}");
        let d = s.doc().unwrap();
        assert!(Arc::ptr_eq(&d.wb, &before), "{command}");
        assert!(Arc::ptr_eq(&d.saved, &saved), "{command}");
        assert_eq!(d.path, path, "{command}");
        assert_eq!((d.revision, d.undo.len(), d.redo.len()), (revision, undo, redo), "{command}");
        assert!(d.is_dirty(), "{command}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
