//! Sheet names that Excel quotes in references stay quoted in formulas, saved XLSX and PivotTable
//! sources (#1458). A bare `.x!A1` in a saved `<f>` makes Excel refuse to open the file.

use std::io::Read;

use gridcraft_engine::Session;
use serde_json::{Value, json};

fn workbook(sheets: &[&str]) -> Session {
    let mut session = Session::new();
    session.new_workbook();
    for name in sheets {
        session.execute("home.insertSheet", json!({"name": name})).unwrap();
    }
    session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    session
}

fn saved_part(session: &Session, part: &str) -> String {
    let bytes = gridcraft_engine::io::save_bytes(&session.doc().unwrap().wb, "quoting.xlsx").unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(part).unwrap().read_to_string(&mut text).unwrap();
    text
}

#[test]
fn formulas_into_dot_and_r1c1_sheet_names_keep_their_quotes() {
    let mut s = workbook(&[".x", "R1C1", "rc"]);
    s.execute("cell.set", json!({"cell": "Sheet1!A1", "input": "='.x'!A1+'R1C1'!A1+'rc'!B2"})).unwrap();
    let got = s.execute("cell.get", json!({"cell": "Sheet1!A1"})).unwrap();
    assert_eq!(got["formula"], json!("='.x'!A1+'R1C1'!A1+'rc'!B2"));
    let xml = saved_part(&s, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<f>'.x'!A1+'R1C1'!A1+'rc'!B2</f>"), "saved formula: {xml}");
}

#[test]
fn pivot_source_on_a_sheet_named_like_a_cell_is_quoted() {
    let mut s = workbook(&["A1"]);
    s.execute("sheet.activate", json!({"sheet": "A1"})).unwrap();
    s.execute("range.setValues", json!({"range": "A1:B3", "values": [["K", "V"], ["a", 1], ["b", 2]]})).unwrap();
    s.execute("selection.set", json!({"range": "A2"})).unwrap();
    let out: Value = s.execute("insert.pivotTable", json!({"rows": ["K"], "values": ["V"]})).unwrap();
    assert_eq!(out["source"], json!("'A1'!$A$1:$B$3"), "{out}");
}
