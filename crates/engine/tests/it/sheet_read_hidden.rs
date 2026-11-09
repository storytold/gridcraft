//! sheet.read sees what a viewer of the sheet sees: hidden rows, columns and sheets are read
//! only with `includeHidden` (the gridcraft mirror of wordcraft#4993). Save-as-CSV still writes
//! hidden rows — that matches Excel and is not a read surface.

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn reads_skip_hidden_rows_cols_unless_included() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a", "b", "c"], ["d", "e", "f"], ["g", "h", "i"]]})).unwrap();
    // Hide row 2 and column B by setting their size to 0, as the UI's Hide does.
    s.execute("home.rowHeight", json!({"rows": "2:2", "height": 0})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "B:B", "width": 0})).unwrap();

    let r = s.execute("sheet.read", json!({"range": "A1:C3"})).unwrap();
    assert_eq!(r["values"], json!([["a", "c"], ["g", "i"]]), "{}", r);
    assert_eq!(r["visibility"], "Visible");

    let all = s.execute("sheet.read", json!({"range": "A1:C3", "includeHidden": true})).unwrap();
    assert_eq!(all["values"], json!([["a", "b", "c"], ["d", "e", "f"], ["g", "h", "i"]]), "{}", all);

    // Rows 1 and 3 stay visible; the only column in the range is hidden, so each
    // visible row reads as an empty row.
    let col = s.execute("sheet.read", json!({"range": "B1:B3"})).unwrap();
    assert_eq!(col["values"], json!([[], []]), "{}", col);
    let col = s.execute("sheet.read", json!({"range": "B1:B3", "includeHidden": true})).unwrap();
    assert_eq!(col["values"], json!([["b"], ["e"], ["h"]]), "{}", col);
}

#[test]
fn a_hidden_sheet_is_read_only_with_includehidden() {
    let mut s = session();
    s.execute("home.insertSheet", json!({"name": "Vault"})).unwrap();
    s.execute("range.setValues", json!({"range": "A1", "values": [["secret"]]})).unwrap();
    s.execute("sheet.hide", json!({"sheet": "Vault"})).unwrap();

    let r = s.execute("sheet.read", json!({"sheet": "Vault", "range": "A1"})).unwrap();
    assert_eq!(r["visibility"], "Hidden");
    assert_eq!(r["values"], json!([["secret"]]), "{}", r);
}

#[test]
fn an_empty_sheet_reports_its_visibility() {
    let mut s = session();
    s.execute("home.insertSheet", json!({"name": "Blank"})).unwrap();
    s.execute("sheet.hide", json!({"sheet": "Blank"})).unwrap();

    let r = s.execute("sheet.read", json!({"sheet": "Blank"})).unwrap();
    assert_eq!(r["visibility"], "Hidden", "{}", r);
    assert_eq!(r["values"], json!([]), "{}", r);
}

#[test]
fn cell_get_reports_hidden_row_and_col_flags() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a", "b"], ["c", "d"]]})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "2:2", "height": 0})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "B:B", "width": 0})).unwrap();

    let r = s.execute("cell.get", json!({"cell": "B2"})).unwrap();
    assert_eq!(r["hiddenRow"], json!(true), "{}", r);
    assert_eq!(r["hiddenCol"], json!(true), "{}", r);
    let r = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!(r["hiddenRow"], json!(false), "{}", r);
    assert_eq!(r["hiddenCol"], json!(false), "{}", r);
}
