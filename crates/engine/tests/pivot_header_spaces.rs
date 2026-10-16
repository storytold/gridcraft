//! #1447: a source header keeps its leading and trailing spaces through a PivotTable, a refresh
//! and an XLSX save.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) -> Json {
    s.execute(command, params).unwrap()
}

fn source() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [["Region ", " Sales ", "Qty"], ["East", 10, 1], ["West", 20, 2], ["East", 5, 3]]}),
    );
    s
}

fn first_pivot(s: &Session) -> &gridcraft_engine::model::PivotTable {
    s.doc().unwrap().wb.sheets.iter().flat_map(|sh| sh.pivots.iter()).next().expect("a PivotTable")
}

fn text(s: &Session, sheet: usize, cell: &str) -> Value {
    s.doc().unwrap().wb.sheet(sheet).unwrap().value(CellRef::parse(cell).unwrap())
}

#[test]
fn headers_with_spaces_keep_their_fields_through_refresh_and_save() {
    let mut s = source();
    run(&mut s, "selection.set", json!({"range": "A1"}));
    let made = run(&mut s, "insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales", "Qty"]}));
    let pivot = s.doc().unwrap().wb.sheets.iter().flat_map(|sh| sh.pivots.iter()).next().unwrap_or_else(|| panic!("no pivot: {made}"));
    assert_eq!(pivot.rows[0].source_col, "Region ");
    assert_eq!(pivot.values[0].source_col, " Sales ");

    let saved = run(&mut s, "file.saveBytes", json!({"format": "xlsx"}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"name": "p.xlsx", "base64": saved["base64"]}));
    let pivot = first_pivot(&reopened);
    assert_eq!((pivot.rows.len(), pivot.values.len()), (1, 2), "{pivot:?}");
    assert_eq!(pivot.rows[0].source_col, "Region ");
    run(&mut reopened, "pivot.refresh", json!({}));
    let pivot = first_pivot(&reopened);
    assert_eq!((pivot.rows.len(), pivot.values.len()), (1, 2), "{pivot:?}");
    let _ = text(&reopened, 1, "A1");
}
