//! A row delete leaves the rows an applied filter hides, whatever way the rows are named: the
//! selection, a list of rows, Delete Cells > Entire row. Only the rows that are shown (or hidden
//! by hand) go, and one Undo brings them back.

use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::model::FilterCriterion;
use serde_json::{Value as Json, json};

/// A list whose Amount column is filtered for its blanks: under the header, rows 3 and 5 are
/// shown and rows 2, 4 and 6 hidden. D1 reads the hidden row 4.
fn blanks_filtered() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1:C6", "values": [["Name", "Amount", "Note"], ["r1", 10, "x1"], ["r2", null, "x2"], ["r3", 22, "x3"], ["r4", null, "x4"], ["r5", 7, "x5"]]}),
    )
    .unwrap();
    s.execute("range.setValues", json!({"range": "D1", "values": [["=B4*2"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:C6"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
    assert_eq!(hidden(&s, 6), [false, true, false, true, false, true]);
    s
}

fn get(s: &mut Session, a1: &str) -> Json {
    s.execute("cell.get", json!({"cell": a1})).unwrap()
}

fn column(s: &mut Session, col: &str, rows: u32) -> Vec<String> {
    (1..=rows).map(|r| get(s, &format!("{col}{r}"))["text"].as_str().unwrap_or("").to_string()).collect()
}

/// Rows 1..=n of the active sheet: hidden or not.
fn hidden(s: &Session, n: u32) -> Vec<bool> {
    let sh = s.doc().unwrap().wb.active().unwrap();
    (0..n).map(|r| sh.is_row_hidden(r)).collect()
}

/// After the shown rows 3 and 5 went: the hidden rows keep their values and stay hidden, D1
/// follows its cell to the new row, and the filter's range lost the two deleted rows.
fn assert_shown_rows_went(s: &mut Session, way: &str) {
    assert_eq!(column(s, "A", 5), ["Name", "r1", "r3", "r5", ""], "{way}");
    assert_eq!(column(s, "B", 4), ["Amount", "10", "22", "7"], "{way}");
    assert_eq!(column(s, "C", 4), ["Note", "x1", "x3", "x5"], "{way}");
    assert_eq!(get(s, "D1")["formula"], json!("=B3*2"), "{way}");
    assert_eq!(get(s, "D1")["value"], json!(44.0), "{way}");
    assert_eq!(hidden(s, 4), [false, true, true, true], "{way}: the rows that stayed are still hidden by the filter");
    let af = s.doc().unwrap().wb.active().unwrap().autofilter.clone().unwrap();
    assert_eq!(af.range.a1(), "A1:C4", "{way}");
}

#[test]
fn every_way_of_deleting_rows_leaves_the_rows_the_filter_hides() {
    let ways: [(&str, Option<&str>, &str, Json); 6] = [
        ("row headers selected", Some("3:5"), "home.deleteRows", json!({})),
        ("cells selected, Delete Sheet Rows", Some("A3:B5"), "home.deleteRows", json!({})),
        ("a list of rows", None, "home.deleteRows", json!({"rows": "3:5"})),
        ("rows and a count", None, "home.deleteRows", json!({"rows": "3", "count": 3})),
        ("Delete Cells > Entire row", Some("A3:B5"), "home.deleteCells", json!({"shift": "row"})),
        ("Delete Cells > Entire row, a range named", None, "home.deleteCells", json!({"range": "A3:C5", "shift": "entireRow"})),
    ];
    for (way, sel, cmd, params) in ways {
        let mut s = blanks_filtered();
        if let Some(r) = sel {
            s.execute("selection.set", json!({"range": r})).unwrap();
        }
        s.execute(cmd, params).unwrap();
        assert_shown_rows_went(&mut s, way);
        // Clear Filter shows every row that was hidden, with its values.
        s.execute("data.clearFilter", json!({})).unwrap();
        assert_eq!(hidden(&s, 4), [false; 4], "{way}");
        assert_eq!(column(&mut s, "A", 5), ["Name", "r1", "r3", "r5", ""], "{way}");
    }
}

/// The issue's example: a column filtered for one value, the visible range selected and
/// deleted with Delete Cells > Entire row. Every row the filter hid is still there.
#[test]
fn delete_cells_entire_row_over_the_visible_range_keeps_the_filtered_out_rows() {
    let mut s = Session::new();
    s.new_workbook();
    let mut values = vec![json!(["number", "letter"])];
    for n in 2..=21 {
        values.push(json!([n, if n % 2 == 0 { "b" } else { "a" }]));
    }
    s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B21"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": ["b"]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B20"})).unwrap();
    s.execute("home.deleteCells", json!({"shift": "row"})).unwrap();
    let hidden_before: Vec<String> = (3..=19).step_by(2).map(|n| n.to_string()).collect();
    let mut left = hidden_before.clone();
    left.push("21".into());
    assert_eq!(column(&mut s, "A", left.len() as u32), left, "the filtered-out rows stay, in order, and row 21 after them");
    assert!(column(&mut s, "B", left.len() as u32).iter().all(|v| v == "a"), "only the rows the filter showed went");
}

#[test]
fn one_undo_brings_back_every_deleted_row_and_redo_deletes_them_again() {
    let mut s = blanks_filtered();
    s.execute("selection.set", json!({"range": "3:5"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_shown_rows_went(&mut s, "delete");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r2", "r3", "r4", "r5"]);
    assert_eq!(column(&mut s, "C", 6), ["Note", "x1", "x2", "x3", "x4", "x5"]);
    assert_eq!(get(&mut s, "D1")["formula"], json!("=B4*2"));
    assert_eq!(hidden(&s, 6), [false, true, false, true, false, true]);
    s.execute("edit.redo", json!({})).unwrap();
    assert_shown_rows_went(&mut s, "redo");
    // One step: the second Undo is the filter's own.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(hidden(&s, 6), [false; 6], "the second Undo took the filter's criterion back, not a second part of the delete");
}

/// A longer list with every third Amount blank and a formula in each row reading its own row and
/// the last row that stays; filtered for the blanks, every data row selected and deleted.
#[test]
fn a_longer_list_keeps_every_hidden_row_and_its_formulas() {
    let mut s = Session::new();
    s.new_workbook();
    let n = 300u32;
    let mut values = vec![json!(["Id", "Amount", "Note", "Twice"])];
    for i in 1..=n {
        let amount = if i % 3 == 0 { Json::Null } else { json!(i * 10) };
        values.push(json!([format!("id{i}"), amount, format!("n{i}"), format!("=B{}*2+B{}", i + 1, n)]));
    }
    s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    s.execute("data.filter", json!({"range": format!("A1:D{}", n + 1)})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
    s.execute("selection.set", json!({"range": format!("2:{}", n + 1)})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    let kept: Vec<u32> = (1..=n).filter(|i| i % 3 != 0).collect();
    let last = kept.len() as u32 + 1;
    for (k, i) in kept.iter().enumerate() {
        let row = k as u32 + 2;
        assert_eq!(get(&mut s, &format!("A{row}"))["text"], json!(format!("id{i}")));
        assert_eq!(get(&mut s, &format!("B{row}"))["value"], json!((i * 10) as f64));
        assert_eq!(get(&mut s, &format!("D{row}"))["formula"], json!(format!("=B{row}*2+B{last}")), "id{i}");
    }
    assert_eq!(get(&mut s, &format!("A{}", last + 1))["text"], json!(""), "the shown rows are gone");
    assert!(hidden(&s, last).iter().skip(1).all(|h| *h), "the rows that stayed are the hidden ones");
}

/// A row hidden by hand goes when it lies outside a filter's rows. Inside them it stays: a row has
/// one `hidden` flag, so it cannot be told from a row the filter hides.
#[test]
fn rows_hidden_by_hand_go_outside_the_filters_rows_and_stay_inside_them() {
    // Without a filter: a row hidden by hand goes with the block.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a"], ["b"], ["c"], ["d"]]})).unwrap();
    s.execute("home.hideRows", json!({"rows": "2:2"})).unwrap();
    s.execute("selection.set", json!({"range": "1:3"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 2), ["d", ""]);
    // Under a filter: a row hidden by hand below the filtered list goes; the filtered rows stay.
    let mut s = blanks_filtered();
    s.execute("range.setValues", json!({"range": "A8:A9", "values": [["below1"], ["below2"]]})).unwrap();
    s.execute("home.hideRows", json!({"rows": "8:8"})).unwrap();
    s.execute("selection.set", json!({"range": "3:8"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r3", "r5", "below2", ""], "rows 3, 5, 7 (shown) and 8 (hidden by hand) went");
    // Inside the filter's rows: the shown row 3 hidden by hand stays with the rows the filter hides.
    let mut s = blanks_filtered();
    s.execute("home.hideRows", json!({"rows": "3:3"})).unwrap();
    s.execute("selection.set", json!({"range": "3:5"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r2", "r3", "r5", ""], "only the shown row 5 went");
}

#[test]
fn two_filtered_columns_separate_rows_and_the_whole_sheet() {
    // Two criteria: A keeps a and b, B keeps its blanks. Shown: rows 2, 5 and 8.
    let two = || {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["K", "V"], ["a", null], ["a", 1], ["c", null], ["b", null], ["b", 2], ["c", 3], ["a", null]]}),
        )
        .unwrap();
        s.execute("data.filter", json!({"range": "A1:B8"})).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "values": ["a", "b"]})).unwrap();
        s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
        assert_eq!(hidden(&s, 8), [false, false, true, true, false, true, true, false]);
        s
    };
    let mut s = two();
    s.execute("selection.set", json!({"range": "2:8"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["K", "a", "c", "b", "c", ""]);
    assert_eq!(column(&mut s, "B", 5), ["V", "1", "", "2", "3"]);
    // Separate areas: row 2 and rows 5 to 8.
    let mut s = two();
    s.execute("selection.set", json!({"range": "A2,A5:B8"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["K", "a", "c", "b", "c", ""]);
    // The whole sheet selected: the header and the shown rows go; the hidden rows move up.
    let mut s = two();
    s.execute("selection.set", json!({"range": "1:1048576"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 5), ["a", "c", "b", "c", ""]);
    assert_eq!(column(&mut s, "B", 4), ["1", "", "2", "3"]);
}

#[test]
fn the_header_row_in_the_selection_goes_with_the_shown_rows() {
    let mut s = blanks_filtered();
    s.execute("selection.set", json!({"range": "1:6"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 4), ["r1", "r3", "r5", ""], "the header and the shown rows went");
    assert_eq!(column(&mut s, "B", 3), ["10", "22", "7"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r2", "r3", "r4", "r5"]);
}

#[test]
fn a_filtered_table_keeps_its_hidden_rows() {
    // A table filtered from its header's drop-down.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["K", "V"], ["a", 1], ["b", 2], ["a", 3], ["b", 4], ["a", 5]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B6", "header": true})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "values": ["b"]})).unwrap();
    assert_eq!(hidden(&s, 6), [false, true, false, true, false, true]);
    s.execute("selection.set", json!({"range": "3:5"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "B", 5), ["V", "1", "3", "5", ""]);
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.tables[0].range.a1(), "A1:B4");

    // A table with a filter of its own, as a file can bring it.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["K", "V"], ["a", 1], ["b", 2], ["a", 3], ["b", 4], ["a", 5]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B6", "header": true})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let wb = Arc::make_mut(&mut d.wb);
        let sh = wb.sheet_mut(0).unwrap();
        sh.tables[0].filter = vec![(0, FilterCriterion::Values { values: vec!["b".into()], blanks: false })];
        for r in [1u32, 3, 5] {
            sh.rows.entry(r).or_default().hidden = true;
        }
    }
    s.execute("selection.set", json!({"range": "2:6"})).unwrap();
    s.execute("home.deleteRows", json!({})).unwrap();
    assert_eq!(column(&mut s, "B", 5), ["V", "1", "3", "5", ""]);
}

/// Cut takes the rows a filter hides; Insert Cut Cells moves every cut row and leaves none of
/// them behind, so closing the gap they left must not keep the hidden ones.
#[test]
fn insert_cut_cells_moves_every_cut_row_hidden_ones_too() {
    let mut s = blanks_filtered();
    s.execute("selection.set", json!({"range": "2:4"})).unwrap();
    s.execute("edit.cut", json!({})).unwrap();
    s.execute("selection.set", json!({"cell": "A8"})).unwrap();
    s.execute("home.insertCopiedCells", json!({})).unwrap();
    assert_eq!(column(&mut s, "A", 8), ["Name", "r4", "r5", "", "r1", "r2", "r3", ""]);
}

#[test]
fn deleting_only_hidden_rows_changes_nothing() {
    let mut s = blanks_filtered();
    let undo = s.doc().unwrap().undo.len();
    s.execute("home.deleteRows", json!({"rows": "4:4"})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r2", "r3", "r4", "r5"]);
    assert_eq!(s.doc().unwrap().undo.len(), undo, "no undo step for a delete that deleted nothing");
}

#[test]
fn deletes_the_filter_does_not_touch_are_as_before() {
    // No filter: the block goes.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a"], ["b"], ["c"], ["d"]]})).unwrap();
    s.execute("home.deleteRows", json!({"rows": "2:3"})).unwrap();
    assert_eq!(column(&mut s, "A", 3), ["a", "d", ""]);
    // A filter that hides nothing: every selected row goes.
    let mut s = blanks_filtered();
    s.execute("data.filterBy", json!({"column": "B", "clear": true})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "values": ["r1", "r2", "r3", "r4", "r5"]})).unwrap();
    assert_eq!(hidden(&s, 6), [false; 6]);
    s.execute("home.deleteRows", json!({"rows": "3:5"})).unwrap();
    assert_eq!(column(&mut s, "A", 4), ["Name", "r1", "r5", ""]);
    // A protected sheet that does not allow deleting rows refuses.
    let mut s = blanks_filtered();
    s.execute("review.protectSheet", json!({})).unwrap();
    assert!(s.execute("home.deleteRows", json!({"rows": "3:5"})).is_err());
    assert_eq!(column(&mut s, "A", 6), ["Name", "r1", "r2", "r3", "r4", "r5"]);
    // Columns under a filter: the column's cells go in every row, hidden ones too.
    let mut s = blanks_filtered();
    s.execute("home.deleteColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(column(&mut s, "A", 6), ["Amount", "10", "", "22", "", "7"]);
    assert_eq!(column(&mut s, "B", 6), ["Note", "x1", "x2", "x3", "x4", "x5"]);
}
