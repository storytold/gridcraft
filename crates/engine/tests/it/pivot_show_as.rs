//! Show Values As, checked against numbers worked out by hand rather than against "the report
//! still exists". The source (Region, Product, Sales):
//!
//! | Region | Apples | Pears | Plums | Total |
//! |--------|-------:|------:|------:|------:|
//! | East   |    160 |   120 |    60 |   340 |
//! | North  |     90 |    30 |     — |   120 |
//! | West   |     50 |    45 |    70 |   165 |
//! | Total  |    300 |   195 |   130 |   625 |
//!
//! North has no Plums: a base item with no records beside a cell must leave it empty.

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const RECORDS: [(&str, &str, f64); 9] = [
    ("East", "Apples", 100.0),
    ("West", "Apples", 50.0),
    ("East", "Pears", 120.0),
    ("North", "Pears", 30.0),
    ("West", "Plums", 70.0),
    ("East", "Plums", 60.0),
    ("North", "Apples", 90.0),
    ("West", "Pears", 45.0),
    ("East", "Apples", 60.0),
];

fn session_with(records: &[(&str, &str, Json)]) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let mut rows = vec![json!(["Region", "Product", "Sales"])];
    rows.extend(records.iter().map(|(r, p, v)| json!([r, p, v])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s
}

fn sales() -> Session {
    let records: Vec<(&str, &str, Json)> = RECORDS.iter().map(|(r, p, v)| (*r, *p, json!(v))).collect();
    session_with(&records)
}

fn pivot(s: &mut Session, rows: &[&str], columns: &[&str]) {
    s.execute("insert.pivotTable", json!({"rows": rows, "columns": columns, "values": ["Sales"], "destination": "Sheet1!G3"})).unwrap();
}

fn settings(s: &mut Session, params: Json) {
    let mut p = json!({"index": 0});
    for (k, v) in params.as_object().unwrap() {
        p[k] = v.clone();
    }
    s.execute("pivot.valueSettings", p).unwrap();
}

/// The report as written on the sheet, top-left first.
fn grid(s: &Session) -> Vec<Vec<Value>> {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let r = sh.pivots[0].last_range.unwrap();
    (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.value(CellRef::new(row, col))).collect()).collect()
}

/// The value cell of the row reached by following `path` down the row labels (compact form:
/// each label is searched for below the previous one) in the data column headed `column`
/// (`None`: the first data column).
fn at(s: &Session, path: &[&str], column: Option<&str>) -> Value {
    let g = grid(s);
    let col = match column {
        None => 1,
        Some(h) => g.iter().find_map(|row| row.iter().position(|v| v.display() == h)).unwrap_or_else(|| panic!("no column {h}")),
    };
    let mut from = 0;
    for label in path {
        from += g[from..].iter().position(|row| row[0].display() == *label).unwrap_or_else(|| panic!("no row {path:?}"));
    }
    g[from][col].clone()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v:?}"))
}

#[track_caller]
fn close(v: Value, want: f64) {
    let got = num(&v);
    assert!((got - want).abs() < 1e-9, "got {got}, want {want}");
}

#[track_caller]
fn empty(v: Value) {
    assert_eq!(v, Value::Empty);
}

// ---------------------------------------------------------------- totals and index

#[test]
fn percent_of_parent_row_total_is_the_share_of_the_parent_group() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "percentOfParentRowTotal"}));
    close(at(&s, &["East"], None), 340.0 / 625.0);
    close(at(&s, &["East", "Apples"], None), 160.0 / 340.0);
    close(at(&s, &["West", "Plums"], None), 70.0 / 165.0);
    close(at(&s, &["Grand Total"], None), 1.0);
}

#[test]
fn index_weighs_a_cell_by_its_row_and_column_totals() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &["Product"]);
    settings(&mut s, json!({"showAs": "index"}));
    close(at(&s, &["East"], Some("Apples")), 160.0 * 625.0 / (340.0 * 300.0));
    close(at(&s, &["West"], Some("Plums")), 70.0 * 625.0 / (165.0 * 130.0));
    close(at(&s, &["East"], Some("Grand Total")), 1.0);
    close(at(&s, &["Grand Total"], Some("Grand Total")), 1.0);
    empty(at(&s, &["North"], Some("Plums")));
}

// ---------------------------------------------------------------- base item modes

#[test]
fn difference_from_the_previous_item_leaves_the_first_empty() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom"}));
    empty(at(&s, &["East"], None));
    close(at(&s, &["North"], None), 120.0 - 340.0);
    close(at(&s, &["West"], None), 165.0 - 120.0);
    empty(at(&s, &["Grand Total"], None));
}

#[test]
fn difference_from_the_next_item_leaves_the_last_empty() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseItem": "(Next)"}));
    close(at(&s, &["East"], None), 340.0 - 120.0);
    close(at(&s, &["North"], None), 120.0 - 165.0);
    empty(at(&s, &["West"], None));
}

#[test]
fn difference_from_a_named_item_leaves_that_item_empty() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseItem": "east"}));
    empty(at(&s, &["East"], None));
    close(at(&s, &["North"], None), 120.0 - 340.0);
    close(at(&s, &["West"], None), 165.0 - 340.0);
}

#[test]
fn percent_of_shows_the_base_item_as_one_hundred_percent() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "percentOf", "baseItem": "West"}));
    close(at(&s, &["East"], None), 340.0 / 165.0);
    close(at(&s, &["West"], None), 1.0);
    empty(at(&s, &["Grand Total"], None));
}

#[test]
fn percent_difference_from_divides_by_the_base() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "percentDifferenceFrom"}));
    empty(at(&s, &["East"], None));
    close(at(&s, &["North"], None), (120.0 - 340.0) / 340.0);
    close(at(&s, &["West"], None), (165.0 - 120.0) / 120.0);
}

#[test]
fn percent_difference_from_a_zero_base_is_div0() {
    let mut s = session_with(&[("A", "x", json!(0)), ("B", "x", json!(5))]);
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "percentDifferenceFrom"}));
    assert_eq!(at(&s, &["B"], None), Value::Error(CellError::Div0));
    settings(&mut s, json!({"showAs": "percentOf"}));
    assert_eq!(at(&s, &["B"], None), Value::Error(CellError::Div0));
    settings(&mut s, json!({"showAs": "differenceFrom"}));
    close(at(&s, &["B"], None), 5.0);
}

#[test]
fn an_error_in_the_cell_or_its_base_shows_through() {
    let mut s = session_with(&[("A", "x", json!(0)), ("B", "x", json!(5)), ("C", "x", json!(7))]);
    s.execute("cell.set", json!({"cell": "C2", "input": "=NA()"})).unwrap();
    pivot(&mut s, &["Region"], &[]);
    // The column holds an error, so it is not numeric and would be counted: sum it.
    settings(&mut s, json!({"func": "sum", "showAs": "differenceFrom"}));
    assert_eq!(at(&s, &["B"], None), Value::Error(CellError::NA), "the base A is #N/A");
    close(at(&s, &["C"], None), 2.0);
}

#[test]
fn an_inner_base_field_skips_the_totals_above_it() {
    // #2554 review: a base field below the outermost row field indexed past the path of the
    // subtotal and grand total lines and panicked.
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseField": "Product", "baseItem": "Pears"}));
    close(at(&s, &["East", "Apples"], None), 160.0 - 120.0);
    empty(at(&s, &["East", "Pears"], None));
    close(at(&s, &["East", "Plums"], None), 60.0 - 120.0);
    close(at(&s, &["North", "Apples"], None), 90.0 - 30.0);
    empty(at(&s, &["East"], None));
    empty(at(&s, &["Grand Total"], None));
}

#[test]
fn a_base_item_missing_from_a_group_leaves_that_group_empty() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "percentOf", "baseField": "Product", "baseItem": "Plums"}));
    close(at(&s, &["East", "Apples"], None), 160.0 / 60.0);
    empty(at(&s, &["North", "Apples"], None));
    empty(at(&s, &["North", "Pears"], None));
    close(at(&s, &["West", "Pears"], None), 45.0 / 70.0);
}

#[test]
fn an_outer_base_field_compares_the_same_inner_item_across_groups() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseField": "Region", "baseItem": "West"}));
    close(at(&s, &["East"], None), 340.0 - 165.0);
    close(at(&s, &["East", "Apples"], None), 160.0 - 50.0);
    close(at(&s, &["East", "Plums"], None), 60.0 - 70.0);
    close(at(&s, &["North", "Pears"], None), 30.0 - 45.0);
    empty(at(&s, &["West"], None));
    empty(at(&s, &["West", "Apples"], None));
}

#[test]
fn a_column_field_can_be_the_base_field() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &["Product"]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseField": "Product", "baseItem": "Apples"}));
    close(at(&s, &["East"], Some("Pears")), 120.0 - 160.0);
    close(at(&s, &["West"], Some("Plums")), 70.0 - 50.0);
    empty(at(&s, &["East"], Some("Apples")));
    empty(at(&s, &["North"], Some("Plums")));
    empty(at(&s, &["East"], Some("Grand Total")));
    close(at(&s, &["Grand Total"], Some("Pears")), 195.0 - 300.0);
}

#[test]
fn a_row_field_is_the_base_in_a_report_with_columns_too() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &["Product"]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseField": "Region", "baseItem": "East"}));
    close(at(&s, &["West"], Some("Apples")), 50.0 - 160.0);
    close(at(&s, &["North"], Some("Grand Total")), 120.0 - 340.0);
    empty(at(&s, &["North"], Some("Plums")));
    empty(at(&s, &["Grand Total"], Some("Apples")));
}

// ---------------------------------------------------------------- running totals and ranks

#[test]
fn running_total_in_the_first_row_field_by_default() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "runningTotal"}));
    close(at(&s, &["East"], None), 340.0);
    close(at(&s, &["North"], None), 460.0);
    close(at(&s, &["West"], None), 625.0);
    // Each product runs over the regions.
    close(at(&s, &["East", "Apples"], None), 160.0);
    close(at(&s, &["North", "Apples"], None), 250.0);
    close(at(&s, &["West", "Apples"], None), 300.0);
    close(at(&s, &["West", "Plums"], None), 130.0);
    close(at(&s, &["Grand Total"], None), 625.0);
}

#[test]
fn running_total_in_an_inner_field_restarts_in_each_group() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "runningTotal", "baseField": "Product"}));
    close(at(&s, &["East", "Apples"], None), 160.0);
    close(at(&s, &["East", "Pears"], None), 280.0);
    close(at(&s, &["East", "Plums"], None), 340.0);
    close(at(&s, &["North", "Apples"], None), 90.0);
    close(at(&s, &["East"], None), 340.0);
}

#[test]
fn percent_running_total_ends_at_one_hundred_percent() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "percentRunningTotal", "baseField": "Product"}));
    close(at(&s, &["East", "Apples"], None), 160.0 / 340.0);
    close(at(&s, &["East", "Pears"], None), 280.0 / 340.0);
    close(at(&s, &["East", "Plums"], None), 1.0);
    close(at(&s, &["East"], None), 1.0);
    close(at(&s, &["Grand Total"], None), 1.0);
}

#[test]
fn ranks_follow_the_base_field_in_both_directions() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "rank", "baseField": "Product"}));
    close(at(&s, &["East", "Apples"], None), 1.0);
    close(at(&s, &["East", "Pears"], None), 2.0);
    close(at(&s, &["East", "Plums"], None), 3.0);
    empty(at(&s, &["East"], None));
    empty(at(&s, &["Grand Total"], None));
    settings(&mut s, json!({"showAs": "rankAscending"}));
    close(at(&s, &["East", "Apples"], None), 3.0);
    close(at(&s, &["East", "Plums"], None), 1.0);
    close(at(&s, &["North", "Pears"], None), 1.0);
    close(at(&s, &["North", "Apples"], None), 2.0);
}

#[test]
fn rank_in_the_outer_field_ranks_each_inner_item_across_groups() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "rank"}));
    close(at(&s, &["East"], None), 1.0);
    close(at(&s, &["North"], None), 3.0);
    close(at(&s, &["West"], None), 2.0);
    close(at(&s, &["East", "Apples"], None), 1.0);
    close(at(&s, &["North", "Apples"], None), 2.0);
    close(at(&s, &["West", "Apples"], None), 3.0);
    close(at(&s, &["West", "Plums"], None), 1.0);
}

#[test]
fn ascending_ranks_are_dense() {
    let mut s = session_with(&[("A", "x", json!(5)), ("B", "x", json!(4)), ("C", "x", json!(3)), ("D", "x", json!(3)), ("E", "x", json!(4))]);
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "rankAscending"}));
    let ranks: Vec<f64> = ["A", "B", "C", "D", "E"].iter().map(|k| num(&at(&s, &[k], None))).collect();
    assert_eq!(ranks, [3.0, 2.0, 1.0, 1.0, 2.0]);
}

// ---------------------------------------------------------------- formats

fn data_format(s: &Session, path: &[&str]) -> String {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let r = sh.pivots[0].last_range.unwrap();
    let g = grid(s);
    let mut from = 0;
    for label in path {
        from += g[from..].iter().position(|row| row[0].display() == *label).unwrap();
    }
    d.wb.styles.get(sh.style_id(CellRef::new(r.start.row + from as u32, r.start.col + 1))).num_fmt.as_str().to_string()
}

#[test]
fn ratios_are_percentages_and_ranks_drop_the_source_format() {
    let mut s = sales();
    s.execute("home.formatCells", json!({"range": "C2:C10", "style": {"numFmt": "$#,##0.00"}})).unwrap();
    pivot(&mut s, &["Region"], &[]);
    for (mode, want) in [
        ("percentOf", "0.00%"),
        ("percentDifferenceFrom", "0.00%"),
        ("percentRunningTotal", "0.00%"),
        ("percentOfParentRowTotal", "0.00%"),
        ("differenceFrom", "$#,##0.00"),
        ("runningTotal", "$#,##0.00"),
        ("rank", "General"),
        ("rankAscending", "General"),
        ("index", "General"),
    ] {
        settings(&mut s, json!({"showAs": mode, "baseItem": "East"}));
        assert_eq!(data_format(&s, &["West"]), want, "{mode}");
    }
    settings(&mut s, json!({"showAs": "percentOf", "numberFormat": "0.0%"}));
    assert_eq!(data_format(&s, &["West"]), "0.0%", "an explicit format wins");
}

// ---------------------------------------------------------------- other report shapes

#[test]
fn a_calculated_field_can_be_shown_as_a_difference() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    s.execute("pivot.calculatedField", json!({"name": "Double", "formula": "=Sales*2"})).unwrap();
    s.execute("pivot.valueSettings", json!({"field": "Sum of Double", "showAs": "differenceFrom", "baseItem": "East"})).unwrap();
    close(at(&s, &["West"], Some("Sum of Double")), 2.0 * (165.0 - 340.0));
}

#[test]
fn values_on_rows_keep_each_value_fields_base() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales", "Sales"], "destination": "Sheet1!G3"})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows"})).unwrap();
    settings(&mut s, json!({"index": 1, "showAs": "differenceFrom", "baseItem": "East"}));
    close(at(&s, &["West", "Sum of Sales"], None), 165.0);
    close(at(&s, &["West", "Sum of Sales2"], None), 165.0 - 340.0);
}

// ---------------------------------------------------------------- parameters

fn value_json(s: &mut Session) -> Json {
    s.execute("pivot.fields", json!({})).unwrap()["values"][0].clone()
}

#[test]
fn base_field_and_item_are_validated_and_reported() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    let err = |s: &mut Session, p: Json| s.execute("pivot.valueSettings", p).unwrap_err().to_string();
    assert!(err(&mut s, json!({"index": 0, "baseField": "Sales"})).contains("isn't a row or column field"));
    assert!(err(&mut s, json!({"index": 0, "baseField": 3})).contains("name or null"));
    assert!(err(&mut s, json!({"index": 0, "baseItem": "Pears"})).contains("isn't an item of `Region`"), "the default base is Region");
    settings(&mut s, json!({"showAs": "percentOf", "baseField": "product", "baseItem": "pears"}));
    let v = value_json(&mut s);
    assert_eq!((v["showAs"].as_str(), v["baseField"].as_str(), v["baseItem"].as_str()), (Some("percentOf"), Some("Product"), Some("Pears")));
    // A new base field forgets an item of the old one; null clears.
    settings(&mut s, json!({"baseField": "Region"}));
    assert_eq!(value_json(&mut s)["baseItem"], "(previous)");
    settings(&mut s, json!({"baseItem": "(next)"}));
    assert_eq!(value_json(&mut s)["baseItem"], "(next)");
    settings(&mut s, json!({"baseField": null, "baseItem": null}));
    let v = value_json(&mut s);
    assert_eq!((v["baseField"].clone(), v["baseItem"].clone()), (Json::Null, json!("(previous)")));
}

#[test]
fn a_named_base_item_pins_its_field() {
    // #3394 review: an item named against the default base field kept no field, so a new first
    // row field turned "East" into an item of the wrong field and emptied every cell.
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseItem": "East"}));
    assert_eq!(value_json(&mut s)["baseField"], "Region");
    s.execute("pivot.addField", json!({"field": "Product", "area": "rows", "position": 0})).unwrap();
    assert_eq!(value_json(&mut s)["baseField"], "Region");
    close(at(&s, &["Apples", "West"], None), 50.0 - 160.0);
    empty(at(&s, &["Apples", "East"], None));
    // (previous) and (next) don't depend on a field: they keep following the default.
    settings(&mut s, json!({"baseField": null, "baseItem": "(previous)"}));
    assert_eq!(value_json(&mut s)["baseField"], Json::Null);
    // The pinned field survives a file round trip.
    settings(&mut s, json!({"baseItem": "Pears"}));
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut back = Session::new();
    back.execute("file.open", json!({"name": "p.xlsx", "base64": saved["base64"]})).unwrap();
    let v = back.execute("pivot.fields", json!({"pivot": "PivotTable1"})).unwrap()["values"][0].clone();
    assert_eq!((v["baseField"].clone(), v["baseItem"].clone()), (json!("Product"), json!("Pears")));
}

#[test]
fn a_base_field_that_leaves_the_report_is_forgotten() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseField": "Product", "baseItem": "Pears"}));
    s.execute("pivot.moveField", json!({"field": "Product", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!(value_json(&mut s)["baseField"], "Product", "still an axis field");
    s.execute("pivot.removeField", json!({"field": "Product"})).unwrap();
    let v = value_json(&mut s);
    assert_eq!((v["baseField"].clone(), v["baseItem"].clone()), (Json::Null, json!("(previous)")));
    // The report now measures each region against the previous one.
    close(at(&s, &["North"], None), 120.0 - 340.0);
}

#[test]
fn undo_restores_the_previous_mode() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[]);
    settings(&mut s, json!({"showAs": "differenceFrom", "baseItem": "East"}));
    s.execute("edit.undo", json!({})).unwrap();
    close(at(&s, &["West"], None), 165.0);
    assert_eq!(value_json(&mut s)["showAs"], "normal");
}

// ---------------------------------------------------------------- file round trip

#[test]
fn every_mode_and_base_survives_an_xlsx_round_trip() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[]);
    for (mode, base_field, base_item) in [
        ("percentOf", "Product", "Pears"),
        ("differenceFrom", "Region", "(next)"),
        ("percentDifferenceFrom", "Product", "(previous)"),
        ("runningTotal", "Product", "(previous)"),
        ("percentRunningTotal", "Region", "(previous)"),
        ("rank", "Product", "(previous)"),
        ("rankAscending", "Region", "(previous)"),
        ("index", "", "(previous)"),
        ("percentOfParentRowTotal", "", "(previous)"),
    ] {
        let base_field = if base_field.is_empty() { Json::Null } else { json!(base_field) };
        settings(&mut s, json!({"showAs": mode, "baseField": base_field, "baseItem": base_item}));
        let before = grid(&s);
        let bytes = gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "show-as.xlsx").unwrap();
        let (wb, _) = gridcraft_engine::io::open_bytes("show-as.xlsx", &bytes).unwrap();
        let value = &wb.sheets.iter().find_map(|sh| sh.pivots.first()).unwrap().values[0];
        let want = s.doc().unwrap().wb.active().unwrap().pivots[0].values[0].clone();
        assert_eq!(value.show_as, want.show_as, "{mode}");
        if want.show_as.uses_base_field() {
            // Region, the first row field, is the default base field: it may read back as unset.
            let base = |v: &gridcraft_model::PivotValue| v.base_field.clone().unwrap_or_else(|| "Region".into());
            assert_eq!(base(value), base(&want), "{mode}");
        }
        if want.show_as.uses_base_item() {
            assert_eq!(value.base_item, want.base_item, "{mode}");
        }
        // The reopened report computes the same cells.
        let mut reopened = Session::new();
        let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        reopened.execute("file.open", json!({"name": "q.xlsx", "base64": saved["base64"]})).unwrap();
        let sheet = reopened.doc().unwrap().wb.sheets.iter().position(|sh| !sh.pivots.is_empty()).unwrap();
        reopened.execute("sheet.activate", json!({"sheet": sheet})).unwrap();
        reopened.execute("pivot.refresh", json!({"replace": true})).unwrap();
        assert_eq!(grid(&reopened), before, "{mode}");
    }
}

// ---------------------------------------------------------------- every combination

/// Every mode, against every base field and kind of base item, in every layout and total
/// setting, with values on rows or columns: the report renders, and every number is finite.
#[test]
fn every_combination_renders_finite_numbers() {
    let modes = [
        "normal",
        "percentOfGrandTotal",
        "percentOfColumnTotal",
        "percentOfRowTotal",
        "percentOf",
        "percentOfParentRowTotal",
        "differenceFrom",
        "percentDifferenceFrom",
        "runningTotal",
        "percentRunningTotal",
        "rankAscending",
        "rank",
        "index",
    ];
    let shapes: [(&[&str], &[&str]); 5] =
        [(&["Region", "Product"], &[]), (&["Region"], &["Product"]), (&[], &["Region", "Product"]), (&["Product"], &[]), (&[], &[])];
    let mut checked = 0;
    for (rows, columns) in shapes {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": rows, "columns": columns, "values": ["Sales", "Sales"], "destination": "Sheet1!G3"})).unwrap();
        let axes: Vec<&str> = rows.iter().chain(columns.iter()).copied().collect();
        let mut bases: Vec<Json> = axes.iter().map(|f| json!(f)).collect();
        bases.push(Json::Null);
        for layout in ["compact", "outline", "tabular"] {
            for (totals, values_on_rows) in [(true, false), (false, true), (true, true)] {
                s.execute(
                    "pivot.layout",
                    json!({"layout": layout, "grandTotalsRows": totals, "grandTotalsCols": totals, "subtotals": if totals { "top" } else { "none" }}),
                )
                .unwrap();
                if values_on_rows != s.doc().unwrap().wb.active().unwrap().pivots[0].values_on_rows {
                    let (from, to) = if values_on_rows { ("columns", "rows") } else { ("rows", "columns") };
                    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": from, "to": to})).unwrap();
                }
                for mode in modes {
                    for base in &bases {
                        for item in ["(previous)", "(next)", "Pears", "East"] {
                            let mut p = json!({"index": 1, "showAs": mode, "baseField": base});
                            let named_ok = match base.as_str() {
                                Some("Product") => item == "Pears",
                                Some("Region") => item == "East",
                                _ => !axes.is_empty() && ((axes[0] == "Product") == (item == "Pears")),
                            };
                            if item.starts_with('(') || named_ok {
                                p["baseItem"] = json!(item);
                            }
                            s.execute("pivot.valueSettings", p).unwrap_or_else(|e| panic!("{rows:?}/{columns:?} {layout} {mode} {base} {item}: {e}"));
                            for v in grid(&s).iter().flatten() {
                                if let Value::Number(n) = v {
                                    assert!(n.is_finite(), "{mode} {base} {item}: {n}");
                                }
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 2000, "{checked}");
}

/// Running totals, ranks and base items look each cell up in its run of items once the run is
/// known: with a pass over the run per cell, 20,000 items took minutes.
#[test]
#[allow(clippy::disallowed_methods)] // the clock is read natively; this test doesn't run on wasm
fn many_items_stay_fast() {
    let mut s = Session::new();
    s.new_workbook();
    let n = 20_000;
    let mut rows = vec![json!(["Key", "Sales"])];
    rows.extend((0..n).map(|i| json!([format!("k{i:05}"), (i * 7919) % 1000])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute(
        "insert.pivotTable",
        json!({"source": format!("Sheet1!$A$1:$B${}", n + 1), "rows": ["Key"], "values": ["Sales"], "destination": "Sheet1!D3"}),
    )
    .unwrap();
    let started = std::time::Instant::now();
    for (mode, item) in [("rank", "(previous)"), ("rankAscending", "(previous)"), ("percentRunningTotal", "(previous)"), ("percentOf", "k00001")] {
        settings(&mut s, json!({"showAs": mode, "baseItem": item}));
    }
    close(at(&s, &["k00002"], None), (2 * 7919 % 1000) as f64 / 919.0);
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}
