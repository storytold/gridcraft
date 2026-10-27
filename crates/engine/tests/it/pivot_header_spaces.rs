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

/// The field commands take a field by its name without the header's outer spaces, as
/// `insert.pivotTable` does (`Region` for the header `Region `).
#[test]
fn field_commands_find_a_spaced_field_by_its_trimmed_name() {
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [["Region ", "Date ", "Sales "], ["East", 45000, 10], ["West", 45100, 20], ["East", 45200, 5]]}),
    );
    run(&mut s, "selection.set", json!({"range": "A1"}));
    run(&mut s, "insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]}));
    assert_eq!(first_pivot(&s).rows[0].source_col, "Region ");

    run(&mut s, "pivot.sort", json!({"field": "Region", "order": "desc"}));
    run(&mut s, "pivot.collapse", json!({"field": "Region", "items": ["East"]}));
    run(&mut s, "pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"}));
    assert_eq!(first_pivot(&s).columns[0].source_col, "Region ");
    run(&mut s, "pivot.moveField", json!({"field": "Region", "from": "columns", "to": "rows"}));
    run(&mut s, "pivot.addField", json!({"field": "Date", "area": "rows"}));
    run(&mut s, "pivot.group", json!({"field": "Date", "by": "months"}));
    run(&mut s, "pivot.valueSettings", json!({"field": "Sales", "func": "average"}));
    run(&mut s, "pivot.removeField", json!({"field": "Region", "area": "rows"}));
    let pivot = first_pivot(&s);
    assert_eq!(pivot.rows.len(), 1, "{pivot:?}");
    assert_eq!(pivot.rows[0].source_col, "Date ");
}

/// A value caption that equals an axis field's trimmed name doesn't stand in for that field.
#[test]
fn axis_command_does_not_pick_a_value_caption_of_the_same_name() {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [["Sum of Sales ", "Sales"], ["a", 1], ["b", 2]]}));
    run(&mut s, "selection.set", json!({"range": "A1"}));
    run(&mut s, "insert.pivotTable", json!({"rows": ["Sum of Sales"], "values": ["Sales"]}));
    let pivot = first_pivot(&s);
    assert_eq!(pivot.rows[0].source_col, "Sum of Sales ");
    assert_eq!(pivot.values[0].name, "Sum of Sales");
    run(&mut s, "pivot.sort", json!({"field": "Sum of Sales", "order": "desc"}));
    run(&mut s, "pivot.valueSettings", json!({"field": "Sum of Sales", "func": "average"}));
    let pivot = first_pivot(&s);
    assert_eq!(pivot.rows[0].sort, gridcraft_engine::model::PivotSort::Desc);
    assert_eq!(pivot.values[0].func, gridcraft_engine::model::PivotFunc::Average);
}
