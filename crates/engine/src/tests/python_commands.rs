//! The python.* commands.

use gridcraft_core::{CellError, Value};
use serde_json::json;

use super::{s, v};

fn set(s: &mut crate::Session, cell: &str, code: &str) -> serde_json::Value {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap()
}

#[test]
fn set_stores_the_code_and_blocks_the_cell_until_it_can_run() {
    let mut s = s();
    let r = set(&mut s, "B1", "xl('A1') + 1");
    assert_eq!(r["cell"], "B1");
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Blocked));
    let got = s.execute("cell.get", json!({"cell": "B1"})).unwrap();
    assert_eq!(got["python"]["code"], "xl('A1') + 1");
    assert_eq!(got["python"]["output"], "value");
    assert_eq!(got["input"], "xl('A1') + 1");
    // The active cell by default.
    s.execute("selection.set", json!({"cell": "D4"})).unwrap();
    s.execute("python.set", json!({"code": "2"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "D4"})).unwrap()["python"]["code"], "2");
    // One undo step takes the cell back.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.execute("cell.get", json!({"cell": "D4"})).unwrap()["python"].is_null());
}

#[test]
fn unchanged_code_keeps_its_cached_result() {
    let mut s = s();
    set(&mut s, "A1", "21 * 2");
    // As a file would have cached it.
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let sh = wb.sheet_mut(0).unwrap();
        let cell = sh.cells.get_mut(gridcraft_core::CellRef::new(0, 0)).unwrap();
        let py = std::sync::Arc::make_mut(cell.py.as_mut().unwrap());
        py.output = Some(gridcraft_model::PyOutput { value: Value::Number(42.0), type_name: "int".into(), preview: "42".into(), convertible: true });
        cell.value = Value::Number(42.0);
    }
    set(&mut s, "A1", "21 * 2");
    assert_eq!(v(&s, "A1"), Value::Number(42.0), "same code, same result");
    s.execute("python.set", json!({"cell": "A1", "code": "21 * 2", "output": "object"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "object");
    set(&mut s, "A1", "21 * 3");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked), "new code has no result yet");
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "object", "the return type stays");
}

#[test]
fn set_refuses_bad_params_long_code_headers_and_protected_cells() {
    let mut s = s();
    for p in [
        json!({"cell": "A1"}),
        json!({"cell": "A1", "code": 5}),
        json!({"cell": "not a cell", "code": "1"}),
        json!({"cell": "A1", "code": "1", "output": "sideways"}),
    ] {
        assert!(s.execute("python.set", p.clone()).is_err(), "{p}");
    }
    // The limit is the stored PY(…) formula's 8,192 characters.
    assert!(s.execute("python.set", json!({"cell": "A1", "code": "y".repeat(8_080)})).is_err());
    s.execute("python.set", json!({"cell": "A1", "code": "y".repeat(8_079)})).unwrap();
    let quoted = format!("{}{}", "\"".repeat(1_200), "y".repeat(5_800));
    let e = s.execute("python.set", json!({"cell": "A2", "code": quoted})).unwrap_err();
    assert!(e.to_string().contains("8192") || e.to_string().contains("8,192"), "{e}");
    // A table header is the column's name.
    s.execute("range.setValues", json!({"range": "C1", "values": [["Name", "Score"], ["a", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "C1:D2", "headers": true})).unwrap();
    assert!(s.execute("python.set", json!({"cell": "D1", "code": "1"})).is_err());
    assert_eq!(v(&s, "D1"), Value::text("Score"));
    set(&mut s, "D2", "1");
    // A locked cell on a protected sheet refuses it.
    s.execute("review.protectSheet", json!({})).unwrap();
    let e = s.execute("python.set", json!({"cell": "F1", "code": "1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
}

#[test]
fn set_over_a_spilling_formula_takes_the_spill_away() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(3)"})).unwrap();
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
    set(&mut s, "A1", "[1, 2, 3]");
    assert_eq!(v(&s, "A3"), Value::Empty);
    assert!(s.doc().unwrap().wb.sheet(0).unwrap().spill_ranges.is_empty());
}
