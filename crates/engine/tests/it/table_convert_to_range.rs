use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn read(s: &mut Session, sheet: &str, range: &str, formulas: bool) -> Vec<Json> {
    let out = s.execute("sheet.read", json!({"sheet": sheet, "range": range, "formulas": formulas})).unwrap();
    out["values"].as_array().unwrap().iter().flat_map(|r| r.as_array().unwrap().clone()).collect()
}

/// Converting a table to a range rewrites its structured references as ordinary cell references,
/// so formulas inside the former table, beside it and on another sheet keep their results.
#[test]
fn convert_to_range_rewrites_structured_references() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Amount"], ["a", 10], ["b", 20]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true, "name": "Orders"})).unwrap();
    s.execute("table.totalRow", json!({"table": "Orders", "on": true})).unwrap();
    s.execute("table.totalFunction", json!({"table": "Orders", "column": "Amount", "function": "sum"})).unwrap();
    s.execute(
        "range.setValues",
        json!({"range": "D1", "values": [["=SUM(Orders[Amount])"], ["=Orders[[#Headers],[Amount]]"], ["=Orders[[#Totals],[Amount]]"]]}),
    )
    .unwrap();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("range.setValues", json!({"sheet": "Other", "range": "A1", "values": [["=SUM(Orders[Amount])"]]})).unwrap();
    let sheet1 = read(&mut s, "Sheet1", "B4:D4", false);
    assert_eq!(sheet1[0].as_f64(), Some(30.0));

    s.execute("table.convertToRange", json!({"table": "Orders"})).unwrap();
    assert!(s.doc().unwrap().wb.table("Orders").is_none());
    let formulas = [read(&mut s, "Sheet1", "B4", true), read(&mut s, "Sheet1", "D1:D3", true), read(&mut s, "Other", "A1", true)].concat();
    for f in &formulas {
        let f = f.as_str().unwrap();
        assert!(!f.contains("Orders"), "{f} still names the removed table");
    }
    let values = [read(&mut s, "Sheet1", "B4", false), read(&mut s, "Sheet1", "D1:D3", false), read(&mut s, "Other", "A1", false)].concat();
    assert_eq!(values, vec![json!(30.0), json!(30.0), json!("Amount"), json!(30.0), json!(30.0)], "{formulas:?}");

    // The rewritten references still follow the cells they point at.
    s.execute("cell.set", json!({"sheet": "Sheet1", "cell": "B2", "input": "15"})).unwrap();
    let values = [read(&mut s, "Sheet1", "B4", false), read(&mut s, "Sheet1", "D1:D3", false), read(&mut s, "Other", "A1", false)].concat();
    assert_eq!(values, vec![json!(35.0), json!(35.0), json!("Amount"), json!(35.0), json!(35.0)]);
}
