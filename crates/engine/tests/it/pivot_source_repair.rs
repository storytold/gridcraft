//! Change Data Source can repair an imported PivotTable without accepting invalid headers.

use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::{Value as Json, json};

const OLD_SOURCE: &str = "Sheet1!$A$1:$D$3";
const REPAIRED_SOURCE: &str = "Sheet1!$A$1:$C$3";
const REPORTS: [(&str, &str, &str); 4] = [("Repair1", "F3", "G6"), ("Repair2", "M3", "N6"), ("Repair3", "F12", "G15"), ("Repair4", "M12", "N15")];

/// Build fictional source data with an unused fourth field for repair regressions.
fn source() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Month", "Status", "Amount", "Unused"],
            ["2026-01", "Open", 17, ""],
            ["2026-02", "Open", 23, ""]
        ]}),
    )
    .unwrap();
    s
}

/// Insert a named monthly report at a chosen anchor so reports can be tested independently.
fn add_report(s: &mut Session, source: &str, name: &str, anchor: &str) {
    s.execute(
        "insert.pivotTable",
        json!({"source": source, "destination": format!("Sheet1!{anchor}"), "name": name, "rows": ["Month"], "values": ["Amount"]}),
    )
    .unwrap();
}

/// Create up to four reports, then invalidate their shared source by clearing an unused header.
fn broken_reports(count: usize) -> Session {
    let mut s = source();
    for (name, anchor, total) in REPORTS.iter().take(count) {
        add_report(&mut s, OLD_SOURCE, name, anchor);
        assert_report(&s, total);
    }
    // The fourth field is unused by every report. Its blank header still makes the old
    // four-column source invalid; the user must explicitly select the valid three columns.
    s.execute("edit.clearContents", json!({"range": "D1:D3"})).unwrap();
    s
}

/// Read an active-sheet value by A1 address for concise report and source assertions.
fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// Check the two monthly rows and grand total relative to a report's total cell.
fn assert_report(s: &Session, total: &str) {
    let total = CellRef::parse(total).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(CellRef::new(total.row, total.col - 1)), Value::text("Grand Total"));
    assert_eq!(sh.value(total), Value::Number(40.0));
    assert_eq!(sh.value(CellRef::new(total.row - 2, total.col - 1)), Value::text("2026-01"));
    assert_eq!(sh.value(CellRef::new(total.row - 2, total.col)), Value::Number(17.0));
    assert_eq!(sh.value(CellRef::new(total.row - 1, total.col - 1)), Value::text("2026-02"));
    assert_eq!(sh.value(CellRef::new(total.row - 1, total.col)), Value::Number(23.0));
}

/// Inspect a named report's stored source to distinguish repair from cached output.
fn pivot_source<'a>(s: &'a Session, name: &str) -> &'a str {
    &s.doc().unwrap().wb.active().unwrap().pivots.iter().find(|p| p.name == name).unwrap().source
}

/// Round-trip a session through XLSX bytes to exercise imported pivot state without fixtures.
fn reopen(s: &mut Session) -> Session {
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "source-repair.xlsx", "base64": saved["base64"]})).unwrap();
    reopened
}

/// Require the expected command error while preserving workbook identity and undo/redo lengths.
fn rejects_without_edit(s: &mut Session, command: &str, params: Json, message: &str) {
    let d = s.doc().unwrap();
    let before = d.wb.clone();
    let (undo, redo) = (d.undo.len(), d.redo.len());
    let error = s.execute(command, params).unwrap_err().to_string();
    assert!(error.contains(message), "{error}");
    let d = s.doc().unwrap();
    assert!(Arc::ptr_eq(&before, &d.wb), "a rejected operation must preserve the workbook");
    assert_eq!((d.undo.len(), d.redo.len()), (undo, redo), "a rejected operation must preserve history");
}

/// Repair four imported reports with blank unused headers, including undo, redo, and refresh.
#[test]
fn four_imported_pivots_with_blank_unused_headers_can_be_repaired() {
    let mut original = broken_reports(4);
    let mut s = reopen(&mut original);
    let refreshed = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(refreshed["refreshed"], 0);
    let errors = refreshed["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 4);
    assert!(errors.iter().all(|e| e["error"].as_str().unwrap().contains("field name is not valid")));
    for (name, _, total) in REPORTS {
        let repaired = s.execute("pivot.changeSource", json!({"pivot": name, "source": REPAIRED_SOURCE})).unwrap();
        assert_eq!(repaired["dropped"], json!([]), "no selected field was removed");
        assert_eq!(pivot_source(&s, name), REPAIRED_SOURCE);
        assert_report(&s, total);
    }
    // Repair is one ordinary undoable edit; undo restores the invalid source rather than
    // silently rewriting its missing header, and redo restores the chosen replacement.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(pivot_source(&s, "Repair4"), OLD_SOURCE);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(pivot_source(&s, "Repair4"), REPAIRED_SOURCE);
    let refreshed = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(refreshed["refreshed"], 4);
    assert_eq!(refreshed["errors"], json!([]));
    for (_, _, total) in REPORTS {
        assert_report(&s, total);
    }
    assert_eq!(value(&s, "D1"), Value::Empty, "repair does not invent a header in the old source");
}

/// Allow an explicit valid replacement when a report's original defined source name is gone.
#[test]
fn a_pivot_whose_defined_source_name_was_deleted_can_be_repaired() {
    let mut s = source();
    s.execute("formulas.defineName", json!({"name": "SourceData", "refersTo": OLD_SOURCE})).unwrap();
    add_report(&mut s, "SourceData", "Repair1", "F3");
    s.execute("formulas.deleteName", json!({"name": "SourceData"})).unwrap();
    assert!(s.execute("pivot.refresh", json!({"pivot": "Repair1"})).is_err());
    let repaired = s.execute("pivot.changeSource", json!({"pivot": "Repair1", "source": REPAIRED_SOURCE})).unwrap();
    assert_eq!(repaired["dropped"], json!([]));
    assert_eq!(pivot_source(&s, "Repair1"), REPAIRED_SOURCE);
    assert_report(&s, "G6");
}

/// Reject invalid creation, refresh, and replacement without changing the workbook or history.
#[test]
fn invalid_sources_still_refuse_creation_refresh_and_replacement() {
    let mut s = broken_reports(1);
    s.execute("cell.set", json!({"cell": "Z20", "input": "temporary edit"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!s.doc().unwrap().redo.is_empty(), "failed repairs must also preserve an existing redo branch");
    rejects_without_edit(&mut s, "pivot.refresh", json!({"pivot": "Repair1"}), "field name is not valid");
    rejects_without_edit(
        &mut s,
        "insert.pivotTable",
        json!({"source": OLD_SOURCE, "destination": "Sheet1!M3", "name": "Invalid"}),
        "field name is not valid",
    );
    rejects_without_edit(&mut s, "pivot.changeSource", json!({"pivot": "Repair1", "source": OLD_SOURCE}), "field name is not valid");
    rejects_without_edit(&mut s, "pivot.changeSource", json!({"pivot": "Repair1", "source": "MissingSource"}), "reference isn't valid");
    assert_eq!(pivot_source(&s, "Repair1"), OLD_SOURCE);
}

/// Reject blank replacements instead of silently repairing from the active selection.
#[test]
fn blank_replacements_preserve_invalid_source_and_history() {
    let mut s = broken_reports(1);
    s.execute("cell.set", json!({"cell": "Z20", "input": "temporary edit"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "A1:C3"})).unwrap();
    assert!(!s.doc().unwrap().redo.is_empty(), "rejected replacements must preserve an existing redo branch");
    rejects_without_edit(&mut s, "pivot.changeSource", json!({"pivot": "Repair1"}), "missing `source`");
    for field in ["source", "range"] {
        for blank in ["", " ", "\t\r\n", "\u{2003}"] {
            let mut params = json!({"pivot": "Repair1"});
            params[field] = json!(blank);
            rejects_without_edit(&mut s, "pivot.changeSource", params, "missing `source`");
            assert_eq!(pivot_source(&s, "Repair1"), OLD_SOURCE);
            assert_report(&s, "G6");
        }
    }
    // A nonblank explicit range still repairs the pivot, with surrounding whitespace allowed.
    s.execute("pivot.changeSource", json!({"pivot": "Repair1", "range": format!(" {REPAIRED_SOURCE} ")})).unwrap();
    assert_eq!(pivot_source(&s, "Repair1"), REPAIRED_SOURCE);
    assert_report(&s, "G6");
}

/// Reject source repair on a protected sheet or when report expansion would overwrite data.
#[test]
fn repairing_a_source_preserves_protection_and_existing_target_data() {
    for protected in [false, true] {
        let mut s = broken_reports(1);
        // The new valid source adds March, so the report needs F7:G7 beyond its old extent.
        s.execute(
            "range.setValues",
            json!({"range": "I1", "values": [["Month", "Status", "Amount"], ["2026-01", "Open", 17], ["2026-02", "Open", 23], ["2026-03", "Open", 5]]}),
        )
        .unwrap();
        s.execute("cell.set", json!({"cell": "F7", "input": "Keep this note"})).unwrap();
        if protected {
            s.execute("review.protectSheet", json!({})).unwrap();
        }
        rejects_without_edit(
            &mut s,
            "pivot.changeSource",
            json!({"pivot": "Repair1", "source": "Sheet1!$I$1:$K$4"}),
            if protected { "protected sheet" } else { "already data in Sheet1!F7" },
        );
        assert_eq!(pivot_source(&s, "Repair1"), OLD_SOURCE);
        assert_eq!(value(&s, "F7"), Value::text("Keep this note"));
    }
}

/// Preserve the repaired source across XLSX round-trips and refresh from subsequent source edits.
#[test]
fn repaired_source_and_report_values_survive_save_open_and_refresh() {
    let mut s = broken_reports(1);
    s.execute("pivot.changeSource", json!({"pivot": "Repair1", "source": REPAIRED_SOURCE})).unwrap();
    let mut total = 40.0;
    for january in [19.0, 21.0] {
        s = reopen(&mut s);
        assert_eq!(pivot_source(&s, "Repair1"), REPAIRED_SOURCE);
        assert_eq!(value(&s, "G6"), Value::Number(total), "the rendered report survives saving and reopening");
        s.execute("cell.set", json!({"cell": "C2", "input": january.to_string()})).unwrap();
        let refreshed = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(refreshed["refreshed"], 1);
        assert_eq!(refreshed["errors"], json!([]));
        total = january + 23.0;
        assert_eq!(value(&s, "G4"), Value::Number(january));
        assert_eq!(value(&s, "G5"), Value::Number(23.0));
        assert_eq!(value(&s, "G6"), Value::Number(total), "refresh reads the repaired source, not a stale imported report");
    }
}
