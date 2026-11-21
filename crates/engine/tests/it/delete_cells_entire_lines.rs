//! Delete Cells > Entire row and Entire column take the lines of every area of the target, as
//! Delete Sheet Rows and Delete Sheet Columns do, not only those of the first area.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn get(s: &mut Session, a1: &str) -> Json {
    s.execute("cell.get", json!({"cell": a1})).unwrap()
}

fn column(s: &mut Session, col: &str, rows: u32) -> Vec<String> {
    (1..=rows).map(|r| get(s, &format!("{col}{r}"))["text"].as_str().unwrap_or("").to_string()).collect()
}

fn row(s: &mut Session, r: u32, cols: &[&str]) -> Vec<String> {
    cols.iter().map(|c| get(s, &format!("{c}{r}"))["text"].as_str().unwrap_or("").to_string()).collect()
}

/// The issue's example: the blanks of a column selected with Go To Special, then Delete Cells >
/// Entire row. Every blank row goes, in one undo step.
#[test]
fn entire_row_deletes_the_row_of_every_blank_go_to_special_selected() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:A5", "values": [["a"], [""], ["b"], [""], ["c"]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:A5"})).unwrap();
    s.execute("edit.goToSpecial", json!({"kind": "blanks"})).unwrap();
    assert!(s.doc().unwrap().selection.ranges.len() > 1, "Go To Special selected the blanks as separate areas");
    s.execute("home.deleteCells", json!({"shift": "row"})).unwrap();
    assert_eq!(column(&mut s, "A", 4), ["a", "b", "c", ""]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 5), ["a", "", "b", "", "c"], "one undo step brings both rows back");
}

#[test]
fn entire_row_and_entire_column_take_every_area_of_a_named_range() {
    let fill = |s: &mut Session| {
        s.execute(
            "range.setValues",
            json!({"range": "A1:D4", "values": [["a1", "b1", "c1", "d1"], ["a2", "b2", "c2", "d2"], ["a3", "b3", "c3", "d3"], ["a4", "b4", "c4", "d4"]]}),
        )
        .unwrap();
    };
    for shift in ["row", "entireRow"] {
        let mut s = Session::new();
        s.new_workbook();
        fill(&mut s);
        s.execute("home.deleteCells", json!({"range": "B1,C3:C4", "shift": shift})).unwrap();
        assert_eq!(column(&mut s, "A", 2), ["a2", ""], "{shift}: rows 1, 3 and 4 went");
    }
    for shift in ["column", "entireColumn"] {
        let mut s = Session::new();
        s.new_workbook();
        fill(&mut s);
        s.execute("home.deleteCells", json!({"range": "A2,C3", "shift": shift})).unwrap();
        assert_eq!(row(&mut s, 1, &["A", "B", "C"]), ["b1", "d1", ""], "{shift}: columns A and C went");
    }
}

#[test]
fn entire_row_and_entire_column_take_every_area_of_the_selection() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:C3", "values": [["a1", "b1", "c1"], ["a2", "b2", "c2"], ["a3", "b3", "c3"]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1,B3"})).unwrap();
    s.execute("home.deleteCells", json!({"shift": "row"})).unwrap();
    assert_eq!(column(&mut s, "A", 2), ["a2", ""]);
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "A1,C2"})).unwrap();
    s.execute("home.deleteCells", json!({"shift": "column"})).unwrap();
    assert_eq!(row(&mut s, 1, &["A", "B"]), ["b1", ""]);
}

/// One area, and a shift up, are as before.
#[test]
fn one_area_and_shifts_are_as_before() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:A4", "values": [["a"], ["b"], ["c"], ["d"]]})).unwrap();
    s.execute("home.deleteCells", json!({"range": "A2:A3", "shift": "row"})).unwrap();
    assert_eq!(column(&mut s, "A", 3), ["a", "d", ""]);
    s.execute("home.deleteCells", json!({"range": "A1", "shift": "up"})).unwrap();
    assert_eq!(column(&mut s, "A", 2), ["d", ""]);
}
