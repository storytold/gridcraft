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

/// A legacy array formula outside the table keeps its entered range, and a sheet-qualified bare
/// table name is rewritten too.
#[test]
fn convert_to_range_keeps_array_range_and_rewrites_qualified_names() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Amount"], ["a", 10], ["b", 20]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true, "name": "Orders"})).unwrap();
    s.execute("selection.set", json!({"range": "D1:D2"})).unwrap();
    s.execute("cell.set", json!({"input": "=SUM(Orders[Amount])", "array": true})).unwrap();
    s.execute("cell.set", json!({"cell": "F1", "input": "=SUM(Sheet1!Orders)"})).unwrap();
    assert_eq!(read(&mut s, "Sheet1", "F1", false)[0].as_f64(), Some(30.0));

    s.execute("table.convertToRange", json!({"table": "Orders"})).unwrap();
    let doc = s.doc().unwrap();
    let arr = doc.wb.sheet(0).and_then(|sh| sh.cells.get(gridcraft_core::CellRef::new(0, 3)).and_then(|c| c.formula.clone())).unwrap();
    assert!(arr.array.is_some(), "array range lost");
    assert_eq!(read(&mut s, "Sheet1", "D1:D2", false), vec![json!(30.0), json!(30.0)]);
    let f = read(&mut s, "Sheet1", "F1", true);
    assert!(!f[0].as_str().unwrap().contains("Orders"), "{f:?}");
    assert_eq!(read(&mut s, "Sheet1", "F1", false)[0].as_f64(), Some(30.0));
}

/// A name that a LET or LAMBDA binds locally is not a reference to the table of the same name, so
/// the rewrite leaves it alone; where the name is not bound it still means the table.
#[test]
fn convert_to_range_keeps_local_let_and_lambda_names() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Amount"], ["a", 10], ["b", 20]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true, "name": "Orders"})).unwrap();
    // D3 reads the table by name in the value of the very binding that then shadows it, and D4's
    // LAMBDA binding ends with its body, so the `Orders` beside it is still the table.
    s.execute(
        "range.setValues",
        json!({"range": "D1", "values": [
            ["=LET(Orders,7,Orders)"],
            ["=LAMBDA(Orders,Orders*2)(21)"],
            ["=LET(Orders,COUNTA(Orders),Orders)"],
            ["=SUM(LAMBDA(Orders,Orders)(5),COUNTA(Orders))"],
        ]}),
    )
    .unwrap();
    assert_eq!(read(&mut s, "Sheet1", "D1:D4", false), vec![json!(7.0), json!(42.0), json!(4.0), json!(9.0)]);

    s.execute("table.convertToRange", json!({"table": "Orders"})).unwrap();
    let values = read(&mut s, "Sheet1", "D1:D4", false);
    assert_eq!(values, vec![json!(7.0), json!(42.0), json!(4.0), json!(9.0)]);
    let formulas = read(&mut s, "Sheet1", "D1:D4", true);
    let formulas: Vec<&str> = formulas.iter().map(|f| f.as_str().unwrap_or_default()).collect();
    assert_eq!(
        formulas,
        vec![
            "=LET(Orders,7,Orders)",
            "=LAMBDA(Orders,Orders*2)(21)",
            "=LET(Orders,COUNTA($A$2:$B$3),Orders)",
            "=SUM(LAMBDA(Orders,Orders)(5),COUNTA($A$2:$B$3))",
        ]
    );
}

/// A conversion that would push a formula past the parser's length limit is refused whole.
#[test]
fn convert_to_range_refuses_formulas_that_would_not_reparse() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Amount"], ["a", 10], ["b", 20]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true, "name": "Orders"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    // Each `Orders[Amount]` becomes the longer `Sheet1!$B$2:$B$3`.
    let long = format!("=SUM({})", vec!["Orders[Amount]"; 500].join("+"));
    assert!(long.len() < gridcraft_formula::MAX_FORMULA_LEN);
    s.execute("cell.set", json!({"sheet": "Other", "cell": "A1", "input": long})).unwrap();
    assert!(s.execute("table.convertToRange", json!({"table": "Orders"})).is_err());
    assert!(s.doc().unwrap().wb.table("Orders").is_some());
}
