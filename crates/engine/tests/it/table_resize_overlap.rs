use gridcraft_engine::Session;
use serde_json::json;

fn table() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1", "values":[["Item", "Qty"], ["x", "7"]]})).unwrap();
    s.execute("insert.table", json!({"range":"A1:B2", "header":true, "name":"T"})).unwrap();
    s
}

fn table_range(s: &Session) -> String {
    s.doc().unwrap().wb.active().unwrap().tables[0].range.a1()
}

#[test]
fn resize_to_disjoint_range_is_rejected_and_keeps_table() {
    let mut s = table();
    assert!(s.execute("table.resize", json!({"table":"T", "range":"D1:E2"})).is_err());
    assert_eq!(table_range(&s), "A1:B2");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[1].name, "Qty");
}

#[test]
fn resize_to_overlapping_range_still_works() {
    let mut s = table();
    s.execute("table.resize", json!({"table":"T", "range":"B1:C2"})).unwrap();
    assert_eq!(table_range(&s), "B1:C2");
}

fn two_tables() -> Session {
    let mut s = table();
    s.execute("range.setValues", json!({"range":"D1", "values":[["P", "Q"], ["y", "8"]]})).unwrap();
    s.execute("insert.table", json!({"range":"D1:E2", "header":true, "name":"U"})).unwrap();
    s
}

fn ranges(s: &Session) -> Vec<String> {
    s.doc().unwrap().wb.active().unwrap().tables.iter().map(|t| t.range.a1()).collect()
}

#[test]
fn resize_over_another_table_is_rejected() {
    let mut s = two_tables();
    assert!(s.execute("table.resize", json!({"table":"T", "range":"A1:E2"})).is_err());
    assert_eq!(ranges(&s), ["A1:B2", "D1:E2"]);
    assert!(s.execute("table.resize", json!({"table":"U", "range":"A1:E2"})).is_err());
    assert_eq!(ranges(&s), ["A1:B2", "D1:E2"]);
}

#[test]
fn resize_next_to_another_table_still_works() {
    let mut s = two_tables();
    s.execute("table.resize", json!({"table":"T", "range":"A1:C3"})).unwrap();
    assert_eq!(ranges(&s), ["A1:C3", "D1:E2"]);
}

fn sheet(s: &Session) -> &gridcraft_engine::model::Sheet {
    s.doc().unwrap().wb.active().unwrap()
}

fn names(s: &Session, table: usize) -> Vec<String> {
    sheet(s).tables[table].columns.iter().map(|c| c.name.clone()).collect()
}

fn headers(s: &Session, table: usize) -> Vec<String> {
    let sh = sheet(s);
    let r = sh.tables[table].range;
    (r.start.col..=r.end.col).map(|c| sh.value(gridcraft_core::CellRef::new(r.start.row, c)).display()).collect()
}

fn with_headers(range: &str, row: &[&str]) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = vec![row.iter().map(|h| json!(h)).collect::<Vec<_>>(), row.iter().map(|_| json!(1)).collect(), row.iter().map(|_| json!(2)).collect()];
    s.execute("range.setValues", json!({"range": range, "values": rows})).unwrap();
    s.execute("insert.table", json!({"range": range, "header": true, "name": "T"})).unwrap();
    s
}

#[test]
fn resize_onto_another_table_is_rejected() {
    // From the sales sample, growing Table2 over Table1 used to leave two overlapping tables.
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    s.execute("insert.table", json!({"header": true, "range": "M2:N5"})).unwrap();
    s.execute("insert.table", json!({"header": true, "range": "P2:Q5"})).unwrap();
    let e = s.execute("table.resize", json!({"range": "M2:Q5", "table": "Table2"})).unwrap_err().to_string();
    assert!(e.contains("overlap another table"), "{e}");
    let ranges: Vec<String> = sheet(&s).tables.iter().map(|t| t.range.a1()).collect();
    assert!(ranges.contains(&"M2:N5".to_string()) && ranges.contains(&"P2:Q5".to_string()), "{ranges:?}");
    // Down over a table below, or around a whole table, is refused too; next to it is fine.
    let mut s = table();
    s.execute("range.setValues", json!({"range": "A5", "values": [["c"], ["1"]]})).unwrap();
    s.execute("insert.table", json!({"range": "A5:A6", "header": true, "name": "U"})).unwrap();
    assert!(s.execute("table.resize", json!({"table": "T", "range": "A1:B5"})).is_err());
    assert!(s.execute("table.resize", json!({"table": "T", "range": "A1:C9"})).is_err());
    s.execute("table.resize", json!({"table": "T", "range": "A1:B4"})).unwrap();
    assert_eq!(table_range(&s), "A1:B4");
}

#[test]
fn resize_onto_merged_cells_pivot_or_array_is_rejected() {
    let mut s = table();
    s.execute("home.mergeCenter", json!({"range": "D2:E2"})).unwrap();
    let e = s.execute("table.resize", json!({"table": "T", "range": "A1:D2"})).unwrap_err().to_string();
    assert!(e.contains("merged"), "{e}");
    assert_eq!(table_range(&s), "A1:B2");

    let mut s = table();
    s.execute("selection.set", json!({"range": "D1:D2"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=ROW(1:2)", "array": true})).unwrap();
    let e = s.execute("table.resize", json!({"table": "T", "range": "A1:D2"})).unwrap_err().to_string();
    assert!(e.contains("array"), "{e}");
    assert_eq!(table_range(&s), "A1:B2");

    let mut s = table();
    s.execute("range.setValues", json!({"range": "H1", "values": [["k", "v"], ["x", "1"], ["y", "2"]]})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$H$1:$I$3", "destination": "Sheet1!D3", "filters": ["k"], "values": ["v"]})).unwrap();
    assert!(!sheet(&s).pivots.is_empty());
    let e = s.execute("table.resize", json!({"table": "T", "range": "A1:E2"})).unwrap_err().to_string();
    assert!(e.contains("PivotTable"), "{e}");
    assert_eq!(table_range(&s), "A1:B2");
}

#[test]
fn resize_onto_a_spill_is_allowed() {
    // Excel lets a table take in a spilled formula (the spill then shows #SPILL!).
    let mut s = table();
    s.execute("cell.set", json!({"cell": "D2", "input": "=SEQUENCE(1,2)"})).unwrap();
    s.execute("table.resize", json!({"table": "T", "range": "A1:D2"})).unwrap();
    assert_eq!(table_range(&s), "A1:D2");
}

#[test]
fn resized_in_columns_get_excel_names_written_into_the_header_row() {
    // A resize whose new columns repeated a name left the header row and the column
    // names out of step ("Column3" in memory, "Column32" after reopening).
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "grades"})).unwrap();
    s.execute("home.formatAsTable", json!({"header": "x", "range": "I3:K9", "style": false})).unwrap();
    s.execute("table.resize", json!({"range": "K3:N7", "table": "Table1"})).unwrap();
    let ti = sheet(&s).tables.iter().position(|t| t.name == "Table1").unwrap();
    assert_eq!(names(&s, ti), headers(&s, ti));
    let mut seen = std::collections::HashSet::new();
    assert!(names(&s, ti).iter().all(|n| seen.insert(n.to_lowercase())), "{:?}", names(&s, ti));
    let bytes = gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "t.xlsx").unwrap();
    let (back, _) = gridcraft_engine::io::open_bytes("t.xlsx", &bytes).unwrap();
    let t = back.active().unwrap().tables.iter().find(|t| t.name == "Table1").unwrap();
    assert_eq!(t.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), names(&s, ti));

    // Names Excel gives (observed): an empty header continues a ColumnN on its left, else takes
    // the lowest free ColumnN; a repeated header gets the next free suffix.
    let cases: [(&str, &[&str], &str, &[&str]); 5] = [
        ("A1:B3", &["a", "b"], "A1:E3", &["a", "b", "Column1", "Column2", "Column3"]),
        ("A1:C3", &["Column2", "b", "c"], "A1:F3", &["Column2", "b", "c", "Column1", "Column12", "Column13"]),
        ("A1:B3", &["Column3", "b"], "A1:E3", &["Column3", "b", "Column1", "Column2", "Column22"]),
        ("A1:B3", &["Column1", "b"], "A1:D3", &["Column1", "b", "Column2", "Column3"]),
        ("A1:C3", &["Column1", "Column2", "Column3"], "C1:F3", &["Column3", "Column4", "Column5", "Column6"]),
    ];
    for (range, row, to, want) in cases {
        let mut s = with_headers(range, row);
        s.execute("table.resize", json!({"table": "T", "range": to})).unwrap();
        assert_eq!(names(&s, 0), want.iter().map(|w| w.to_string()).collect::<Vec<_>>(), "{row:?} -> {to}");
        assert_eq!(headers(&s, 0), names(&s, 0), "{row:?} -> {to}");
    }

    // Headers already in the new columns are kept, made unique and turned into text.
    let mut s = with_headers("A1:B3", &["x", "y"]);
    s.execute("range.setValues", json!({"range": "C1", "values": [["X", "x", 5]]})).unwrap();
    s.execute("cell.set", json!({"cell": "F1", "input": "=1+1"})).unwrap();
    s.execute("table.resize", json!({"table": "T", "range": "A1:F3"})).unwrap();
    assert_eq!(names(&s, 0), ["x", "y", "X2", "x3", "5", "2"]);
    assert_eq!(headers(&s, 0), names(&s, 0));
    assert!(sheet(&s).cell(gridcraft_core::CellRef::new(0, 5)).is_some_and(|c| c.formula.is_none()));
}

#[test]
fn resizing_a_table_without_a_header_row_keeps_its_names_and_data() {
    let mut s = with_headers("A1:B3", &["a", "b"]);
    // As a file can have it: the table starts at its first data row (A2:B3), with no header row.
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        let t = &mut std::sync::Arc::make_mut(&mut wb.sheets[0]).tables[0];
        t.header_row = false;
        t.range.start.row = 1;
    }
    let r = sheet(&s).tables[0].range;
    assert!(!sheet(&s).tables[0].header_row);
    let to = gridcraft_core::RangeRef::new(r.start, gridcraft_core::CellRef::new(r.end.row, r.end.col + 1));
    let first = sheet(&s).value(r.start).display();
    s.execute("table.resize", json!({"table": "T", "range": to.a1()})).unwrap();
    assert_eq!(names(&s, 0), ["a", "b", "Column1"]);
    assert_eq!(sheet(&s).value(r.start).display(), first);
}

#[test]
fn resized_formula_header_is_named_by_its_value_when_formulas_show() {
    let mut s = with_headers("A1:B3", &["x", "y"]);
    s.execute("cell.set", json!({"cell": "C1", "input": "=1+1"})).unwrap();
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        std::sync::Arc::make_mut(&mut wb.sheets[0]).show_formulas = true;
    }
    s.execute("table.resize", json!({"table": "T", "range": "A1:C3"})).unwrap();
    assert_eq!(names(&s, 0), ["x", "y", "2"]);
    assert_eq!(headers(&s, 0), names(&s, 0));
}

#[test]
fn resized_picture_header_is_replaced_and_its_picture_removed() {
    let mut s = with_headers("A1:B3", &["x", "y"]);
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        let picture = gridcraft_engine::model::CellPicture { data: vec![1, 2, 3], mime: "image/png".into(), alt: String::new() };
        std::sync::Arc::make_mut(&mut wb.sheets[0]).set_picture(gridcraft_core::CellRef::new(0, 2), std::sync::Arc::new(picture));
    }
    s.execute("table.resize", json!({"table": "T", "range": "A1:C3"})).unwrap();
    assert!(!sheet(&s).cell_pictures.contains_key(&gridcraft_core::CellRef::new(0, 2)));
    assert_eq!(headers(&s, 0), names(&s, 0));
}
