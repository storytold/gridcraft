//! Issue #3763: a copied worksheet's formulas that name the sheet they were copied from refer to
//! the copy, as in Excel.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn read(s: &mut Session, sheet: &str, range: &str, formulas: bool) -> Json {
    s.execute("sheet.read", json!({"sheet": sheet, "range": range, "formulas": formulas})).unwrap()["values"][0].clone()
}

fn source() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "sheet.rename", json!({"name": "Source"}));
    run(&mut s, "home.insertSheet", json!({"name": "Other"}));
    run(&mut s, "cell.set", json!({"sheet": "Other", "cell": "A1", "input": "7"}));
    run(&mut s, "sheet.activate", json!({"sheet": "Source"}));
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [[10, "=Source!A1*2", "=A1*2", "=SUM(Source!A1:A2)", "=Other!A1*3", "=SUM(Source:Other!A1)"], [20]]}),
    );
    s
}

#[test]
fn explicit_self_references_follow_the_copy() {
    let mut s = source();
    run(&mut s, "sheet.move", json!({"sheet": "Source", "to": 2, "copy": true}));
    run(&mut s, "sheet.rename", json!({"name": "Copied"}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "50"}));

    // The copy reads its own cells, so editing its input changes the results.
    assert_eq!(read(&mut s, "Copied", "B1:D1", true), json!(["=Copied!A1*2", "=A1*2", "=SUM(Copied!A1:A2)"]));
    assert_eq!(read(&mut s, "Copied", "B1:D1", false), json!([100.0, 100.0, 70.0]));
    // The original is untouched.
    assert_eq!(read(&mut s, "Source", "B1:D1", true), json!(["=Source!A1*2", "=A1*2", "=SUM(Source!A1:A2)"]));
    assert_eq!(read(&mut s, "Source", "B1:D1", false), json!([20.0, 20.0, 30.0]));
}

#[test]
fn the_generated_copy_name_is_quoted_and_other_sheets_are_left_alone() {
    let mut s = source();
    run(&mut s, "sheet.move", json!({"sheet": "Source", "to": 2, "copy": true}));
    let name = s.doc().unwrap().wb.sheets[2].name.clone();
    assert_eq!(name, "Source (2)");
    let f = read(&mut s, "Source (2)", "B1:F1", true);
    assert_eq!(f[0], json!("='Source (2)'!A1*2"));
    // A reference to another sheet, and a 3-D span, stay as they were.
    assert_eq!(f[3], json!("=Other!A1*3"));
    assert_eq!(f[4], json!("=SUM(Source:Other!A1)"));
}

#[test]
fn the_retargeted_copy_survives_undo_and_redo() {
    let mut s = source();
    run(&mut s, "sheet.move", json!({"sheet": "Source", "to": 2, "copy": true}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(read(&mut s, "Source (2)", "B1", true), json!(["='Source (2)'!A1*2"]));
}
