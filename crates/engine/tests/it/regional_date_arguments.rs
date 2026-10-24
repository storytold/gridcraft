//! Generated regressions for regional text in implicit date arguments.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// Keep date text intact while changing its interpreted year/month and dependent formula.
#[test]
fn referenced_italian_dates_recalculate_on_open_and_region_change() {
    let mut source = Session::new();
    source.new_workbook();
    source
        .execute(
            "range.setValues",
            json!({"range":"A1", "values":[
                ["'31/01/2026", "=YEAR(A1)", "=MONTH(A1)", "=DAY(A1)", "=B1&\"-\"&RIGHT(\"00\"&C1,2)"],
                ["'03/02/2026", "=YEAR(A2)", "=MONTH(A2)", "=DAY(A2)", "=B2&\"-\"&RIGHT(\"00\"&C2,2)"]
            ]}),
        )
        .unwrap();
    let saved = source.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    let mut s = Session::new();
    s.set_region("it-IT").unwrap();
    s.execute("file.open", json!({"name":"fictional-regional-parts.xlsx", "base64":saved["base64"]})).unwrap();
    for (cell, expected) in [("B1", 2026.0), ("C1", 1.0), ("D1", 31.0), ("B2", 2026.0), ("C2", 2.0), ("D2", 3.0)] {
        assert_eq!(value(&s, cell), Value::Number(expected), "{cell}");
    }
    assert_eq!(value(&s, "E1"), Value::text("2026-01"));
    assert_eq!(value(&s, "E2"), Value::text("2026-02"));
    assert!(!s.doc().unwrap().is_dirty(), "regional recalculation is not a document edit");
    s.set_region("en-US").unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(3.0));
    assert_eq!(value(&s, "D2"), Value::Number(2.0));
    s.set_region("it-IT").unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(2.0));
    assert_eq!(value(&s, "A1"), Value::text("31/01/2026"));
    assert_eq!(value(&s, "A2"), Value::text("03/02/2026"));
    assert!(!s.doc().unwrap().is_dirty());
}
