//! `range.fill` with a `range` parameter reads the input as typed in that range's top-left cell,
//! so relative references adjust from there, wherever the selection is. With the active cell
//! inside the target (Ctrl+Enter), they adjust from the active cell, as before.

use gridcraft_engine::Session;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B2", "values": [[2, 10], [3, 10], [4, 10]]})).unwrap();
    s
}

fn get(s: &mut Session, a: &str) -> Value {
    s.execute("cell.get", json!({"cell": a})).unwrap()
}

#[test]
fn an_explicit_range_anchors_relative_references_at_its_top_left_cell() {
    let mut s = session();
    // The selection stays where a new workbook puts it (A1), outside the target, as in a script
    // or a `gridcraft-cli run --cmd 'range.fill=…'`.
    s.execute("range.fill", json!({"range": "D2:D4", "input": "=B2*C2"})).unwrap();
    for (a, formula, value) in [("D2", "=B2*C2", 20.0), ("D3", "=B3*C3", 30.0), ("D4", "=B4*C4", 40.0)] {
        assert_eq!(get(&mut s, a)["formula"], formula, "{a}");
        assert_eq!(get(&mut s, a)["value"].as_f64(), Some(value), "{a}");
    }
}

#[test]
fn an_explicit_range_on_another_sheet_anchors_at_its_top_left_cell() {
    let mut s = session();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    s.execute("range.fill", json!({"range": "Sheet1!D2:D3", "input": "=B2+1"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!(get(&mut s, "D2")["formula"], "=B2+1");
    assert_eq!(get(&mut s, "D3")["formula"], "=B3+1");
}

#[test]
fn the_active_cell_inside_the_target_stays_the_anchor() {
    // Ctrl+Enter: the input is typed in the active cell, which need not be the top-left one.
    let mut s = session();
    s.execute("selection.set", json!({"range": "D2:D4", "active": "D3"})).unwrap();
    s.execute("range.fill", json!({"input": "=B3*C3"})).unwrap();
    assert_eq!(get(&mut s, "D2")["formula"], "=B2*C2");
    assert_eq!(get(&mut s, "D4")["formula"], "=B4*C4");
    // The same when a script names the selected range.
    s.execute("range.fill", json!({"range": "D2:D4", "input": "=C3"})).unwrap();
    assert_eq!(get(&mut s, "D2")["formula"], "=C2");
    assert_eq!(get(&mut s, "D4")["formula"], "=C4");
}
