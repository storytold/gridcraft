//! Issue #3781: sorting keeps a formula's reference to another worksheet with its record, while a
//! reference into the sorted sheet moves with the row, as in Excel.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn read(s: &mut Session, range: &str, formulas: bool) -> Json {
    s.execute("sheet.read", json!({"range": range, "formulas": formulas})).unwrap()["values"].clone()
}

/// `Rates!A1:A3` holds 10, 100, 1000. `Data!A1:D4` is a header and three records: the key, the
/// rate it looks up on `Rates`, the same with a fixed column, and a formula on its own row.
fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "sheet.rename", json!({"name": "Data"}));
    run(&mut s, "home.insertSheet", json!({"name": "Rates"}));
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [[10], [100], [1000]]}));
    run(&mut s, "sheet.activate", json!({"sheet": "Data"}));
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Key", "Rate", "Fixed column", "Own row"],
            ["c", "=Rates!A1", "=Rates!$A1", "=B2*2"],
            ["a", "=Rates!A2", "=Rates!$A2", "=B3*2"],
            ["b", "=Rates!A3", "=Rates!$A3", "=B4*2"],
        ]}),
    );
    s
}

fn sort(s: &mut Session, order: &str) {
    run(s, "data.sort", json!({"range": "A1:D4", "header": true, "keys": [{"column": "A", "order": order}]}));
}

#[test]
fn a_reference_to_another_sheet_stays_with_its_record() {
    let mut s = book();
    sort(&mut s, "asc");
    // Each record keeps the rate it looked up: a = 100, b = 1000, c = 10.
    assert_eq!(read(&mut s, "A2:D4", false), json!([["a", 100.0, 100.0, 200.0], ["b", 1000.0, 1000.0, 2000.0], ["c", 10.0, 10.0, 20.0]]));
    // The formulas still point at the same Rates cells; the one on its own row follows the row.
    assert_eq!(
        read(&mut s, "A2:D4", true),
        json!([["a", "=Rates!A2", "=Rates!$A2", "=B2*2"], ["b", "=Rates!A3", "=Rates!$A3", "=B3*2"], ["c", "=Rates!A1", "=Rates!$A1", "=B4*2"]])
    );
}

#[test]
fn descending_sort_undo_and_redo_agree() {
    let mut s = book();
    sort(&mut s, "desc");
    assert_eq!(read(&mut s, "A2:C4", false), json!([["c", 10.0, 10.0], ["b", 1000.0, 1000.0], ["a", 100.0, 100.0]]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(read(&mut s, "A2:B4", false), json!([["c", 10.0], ["a", 100.0], ["b", 1000.0]]));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(read(&mut s, "A2:B4", false), json!([["c", 10.0], ["b", 1000.0], ["a", 100.0]]));
}

#[test]
fn a_reference_into_the_sorted_sheet_still_moves_with_the_row() {
    let mut s = book();
    run(&mut s, "range.setValues", json!({"range": "F2", "values": [["=Data!B2"], ["=Data!B3"], ["=Data!B4"]]}));
    run(&mut s, "data.sort", json!({"range": "A1:F4", "header": true, "keys": [{"column": "A", "order": "asc"}]}));
    // `Data!B2` is a reference into the sheet itself, so it follows its row like `B2` does.
    assert_eq!(read(&mut s, "F2:F4", true), json!([["=Data!B2"], ["=Data!B3"], ["=Data!B4"]]));
    assert_eq!(read(&mut s, "F2:F4", false), json!([[100.0], [1000.0], [10.0]]));
}
