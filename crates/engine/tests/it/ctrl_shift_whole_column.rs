//! Ctrl/Cmd+Shift+arrow on a whole column or row extends to the data end, not the sheet edge.

use gridcraft_engine::Session;
use serde_json::json;

#[test]
fn ctrl_shift_arrow_on_whole_column_stops_at_data_end() {
    let mut s = Session::new();
    s.new_workbook();
    for i in 1..=20 {
        s.execute("cell.set", json!({"cell": format!("A{i}"), "input": "1"})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "A:A"})).unwrap();
    s.execute("selection.move", json!({"dr": 1, "dc": 0, "extend": true, "jump": true})).unwrap();
    assert_eq!(s.doc().unwrap().selection.a1(), "A1:A20");
}

#[test]
fn ctrl_shift_arrow_on_whole_row_stops_at_data_end() {
    let mut s = Session::new();
    s.new_workbook();
    for c in ["A", "B", "C", "D"] {
        s.execute("cell.set", json!({"cell": format!("{c}1"), "input": "1"})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "1:1"})).unwrap();
    s.execute("selection.move", json!({"dr": 0, "dc": 1, "extend": true, "jump": true})).unwrap();
    assert_eq!(s.doc().unwrap().selection.a1(), "A1:D1");
}

#[test]
fn ctrl_shift_up_on_whole_column_keeps_active_cell_selected() {
    let mut s = Session::new();
    s.new_workbook();
    for i in 1..=20 {
        s.execute("cell.set", json!({"cell": format!("A{i}"), "input": "1"})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "A:A", "active": "A10"})).unwrap();
    s.execute("selection.move", json!({"dr": -1, "dc": 0, "extend": true, "jump": true})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.selection.a1(), "A1:A10");
    assert_eq!(d.selection.active.a1(), "A10");
}

#[test]
fn ctrl_shift_down_on_several_columns_keeps_their_extent() {
    let mut s = Session::new();
    s.new_workbook();
    for i in 1..=20 {
        s.execute("cell.set", json!({"cell": format!("A{i}"), "input": "1"})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "A:C", "active": "A1"})).unwrap();
    s.execute("selection.move", json!({"dr": 1, "dc": 0, "extend": true, "jump": true})).unwrap();
    assert_eq!(s.doc().unwrap().selection.a1(), "A1:C20");
}

#[test]
fn ctrl_shift_right_on_several_rows_keeps_their_extent() {
    let mut s = Session::new();
    s.new_workbook();
    for c in ["A", "B", "C", "D"] {
        s.execute("cell.set", json!({"cell": format!("{c}1"), "input": "1"})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "1:3", "active": "A1"})).unwrap();
    s.execute("selection.move", json!({"dr": 0, "dc": 1, "extend": true, "jump": true})).unwrap();
    assert_eq!(s.doc().unwrap().selection.a1(), "A1:D3");
}
