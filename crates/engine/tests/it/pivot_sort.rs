//! Ordering a field's items: by a value field's totals (Sort › More Sort Options › by values) and
//! by a custom order (dragged items). The source (Region, Product, Sales):
//!
//! | Region | Apples | Pears | Plums | Sum | Max |
//! |--------|-------:|------:|------:|----:|----:|
//! | East   |    160 |   120 |    60 | 340 | 120 |
//! | North  |     90 |    30 |     — | 120 |  90 |
//! | West   |     50 |    45 |    70 | 165 |  70 |

use gridcraft_core::{CellRef, Value};
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

fn pivot(s: &mut Session, rows: &[&str], columns: &[&str], values: Json) {
    s.execute("insert.pivotTable", json!({"rows": rows, "columns": columns, "values": values, "destination": "Sheet1!G3"})).unwrap();
}

fn sort(s: &mut Session, params: Json) -> Result<Json, String> {
    s.execute("pivot.sort", params).map_err(|e| e.to_string())
}

fn grid(s: &Session) -> Vec<Vec<Value>> {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let r = sh.pivots[0].last_range.unwrap();
    (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.value(CellRef::new(row, col))).collect()).collect()
}

/// The row labels below the header row, without the grand total.
fn labels(s: &Session) -> Vec<String> {
    grid(s).iter().skip(1).map(|r| r[0].display()).filter(|l| l != "Grand Total").collect()
}

fn field(s: &mut Session) -> Json {
    s.execute("pivot.fields", json!({})).unwrap()["rows"][0].clone()
}

// ---------------------------------------------------------------- by values

#[test]
fn items_sort_by_a_value_fields_totals() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    assert_eq!(labels(&s), ["East", "West", "North"]);
    sort(&mut s, json!({"field": "Region", "order": "asc", "by": "sales"})).unwrap();
    assert_eq!(labels(&s), ["North", "West", "East"]);
    assert_eq!(field(&mut s)["sortBy"], "Sum of Sales", "a source field names its value field");
    sort(&mut s, json!({"field": "Region", "order": "desc"})).unwrap();
    assert_eq!(labels(&s), ["West", "North", "East"], "without `by` the labels sort again");
    assert_eq!(field(&mut s)["sortBy"], Json::Null);
}

#[test]
fn inner_items_sort_within_each_group() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Product", "order": "desc", "by": "Sum of Sales"})).unwrap();
    assert_eq!(labels(&s), ["East", "Apples", "Pears", "Plums", "North", "Apples", "Pears", "West", "Plums", "Apples", "Pears"]);
}

#[test]
fn the_chosen_value_field_decides_not_the_first() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales", {"field": "Sales", "func": "max"}]));
    sort(&mut s, json!({"field": "Region", "order": "desc", "by": "Max of Sales"})).unwrap();
    assert_eq!(labels(&s), ["East", "North", "West"]);
}

#[test]
fn column_items_sort_by_their_column_totals() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &["Product"], json!(["Sales"]));
    sort(&mut s, json!({"field": "Product", "order": "asc", "by": "Sum of Sales"})).unwrap();
    let header: Vec<String> = grid(&s)[1].iter().map(Value::display).collect();
    assert_eq!(header, ["Row Labels", "Plums", "Pears", "Apples", "Grand Total"]);
}

#[test]
fn ties_keep_label_order_and_items_without_numbers_go_last() {
    let mut s = session_with(&[("C", "x", json!(5)), ("A", "x", json!(5)), ("B", "x", json!(9)), ("E", "x", json!(1)), ("D", "x", json!(2))]);
    s.execute("cell.set", json!({"cell": "C6", "input": "=NA()"})).unwrap();
    pivot(&mut s, &["Region"], &[], json!([{"field": "Sales", "func": "sum"}]));
    sort(&mut s, json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    assert_eq!(labels(&s), ["B", "A", "C", "E", "D"], "D is #N/A");
    sort(&mut s, json!({"field": "Region", "order": "asc", "by": "Sum of Sales"})).unwrap();
    assert_eq!(labels(&s), ["E", "A", "C", "B", "D"]);
}

#[test]
fn renaming_the_value_field_keeps_the_sort_and_removing_it_drops_it() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales", {"field": "Sales", "func": "max"}]));
    sort(&mut s, json!({"field": "Region", "order": "desc", "by": "Max of Sales"})).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 1, "name": "Biggest"})).unwrap();
    assert_eq!(field(&mut s)["sortBy"], "Biggest");
    assert_eq!(labels(&s), ["East", "North", "West"]);
    s.execute("pivot.removeField", json!({"field": "Biggest", "area": "values"})).unwrap();
    assert_eq!(field(&mut s)["sortBy"], Json::Null);
    assert_eq!(labels(&s), ["West", "North", "East"], "descending by label");
}

// ---------------------------------------------------------------- custom order

#[test]
fn a_custom_order_puts_the_listed_items_first() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Region", "order": "custom", "items": ["west", "East", "WEST"]})).unwrap();
    assert_eq!(labels(&s), ["West", "East", "North"]);
    assert_eq!(field(&mut s)["customOrder"], json!(["West", "East"]), "canonical labels, without repeats");
    sort(&mut s, json!({"field": "Region", "order": "asc"})).unwrap();
    assert_eq!(labels(&s), ["East", "North", "West"]);
    assert_eq!(field(&mut s)["customOrder"], json!([]));
}

#[test]
fn a_custom_order_applies_in_every_group() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Product", "order": "custom", "items": ["Plums", "Apples"]})).unwrap();
    assert_eq!(labels(&s), ["East", "Plums", "Apples", "Pears", "North", "Apples", "Pears", "West", "Plums", "Apples", "Pears"]);
}

#[test]
fn sort_parameters_are_validated() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    let err = |s: &mut Session, p: Json| sort(s, p).unwrap_err();
    assert!(err(&mut s, json!({"field": "Region", "order": "custom"})).contains("needs `items`"));
    assert!(err(&mut s, json!({"field": "Region", "order": "custom", "items": []})).contains("needs `items`"));
    assert!(err(&mut s, json!({"field": "Region", "order": "custom", "items": [3]})).contains("item labels"));
    assert!(err(&mut s, json!({"field": "Region", "order": "custom", "items": ["South"]})).contains("isn't an item"));
    assert!(err(&mut s, json!({"field": "Region", "order": "asc", "items": ["East"]})).contains("order: custom"));
    assert!(err(&mut s, json!({"field": "Region", "order": "none", "by": "Sum of Sales"})).contains("ascending or descending"));
    assert!(err(&mut s, json!({"field": "Region", "order": "asc", "by": "Profit"})).contains("isn't in the values area"));
    assert!(err(&mut s, json!({"field": "Region", "order": "sideways"})).contains("unknown order"));
    let too_many: Vec<String> = (0..100_001).map(|i| format!("x{i}")).collect();
    assert!(err(&mut s, json!({"field": "Region", "order": "custom", "items": too_many})).contains("at most 100000"), "refused, not cut short");
    assert!(err(&mut s, json!({"field": "Sales", "order": "asc"})).contains("isn't in the rows or columns"));
    assert_eq!(labels(&s), ["East", "North", "West"], "nothing changed");
}

#[test]
fn a_custom_order_takes_labels_with_spaces_as_they_are() {
    // #3399 review: the requested label was trimmed first, so "East " in the source never matched.
    let mut s = session_with(&[("East ", "x", json!(1)), ("West", "x", json!(2)), ("North", "x", json!(3))]);
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Region", "order": "custom", "items": ["East ", "  west  "]})).unwrap();
    assert_eq!(field(&mut s)["customOrder"], json!(["East ", "West"]), "exact first, then without padding");
    assert_eq!(labels(&s), ["East ", "West", "North"]);
}

#[test]
fn manual_still_means_source_order() {
    // #3398 review: "manual" was an alias of source order before custom orders; it stays one.
    let mut s = session_with(&[("West", "x", json!(1)), ("East", "x", json!(2)), ("North", "x", json!(3))]);
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Region", "order": "manual"})).unwrap();
    assert_eq!(labels(&s), ["West", "East", "North"]);
    assert_eq!(field(&mut s)["sort"], "none");
}

#[test]
fn a_long_custom_order_on_a_large_field_is_accepted() {
    // #3398 review: validating each label against every item was quadratic.
    let mut s = Session::new();
    s.new_workbook();
    let n = 20_000;
    let mut rows = vec![json!(["Key", "Sales"])];
    rows.extend((0..n).map(|i| json!([format!("k{i:05}"), i])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute(
        "insert.pivotTable",
        json!({"source": format!("Sheet1!$A$1:$B${}", n + 1), "rows": ["Key"], "values": ["Sales"], "destination": "Sheet1!D3"}),
    )
    .unwrap();
    let order: Vec<String> = (0..n).rev().map(|i| format!("K{i:05}")).collect();
    sort(&mut s, json!({"field": "Key", "order": "custom", "items": order})).unwrap();
    assert_eq!(grid(&s)[1][0].display(), "k19999");
}

#[test]
fn undo_restores_the_previous_order() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    sort(&mut s, json!({"field": "Region", "order": "custom", "items": ["North"]})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(labels(&s), ["East", "North", "West"]);
    assert_eq!(field(&mut s)["customOrder"], json!([]));
}

// ---------------------------------------------------------------- file round trip

fn reopen(s: &mut Session) -> Session {
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut back = Session::new();
    back.execute("file.open", json!({"name": "s.xlsx", "base64": saved["base64"]})).unwrap();
    back.execute("pivot.refresh", json!({"pivot": "PivotTable1", "replace": true})).unwrap();
    back
}

#[test]
fn sorts_survive_an_xlsx_round_trip() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[], json!(["Sales", {"field": "Sales", "func": "max"}]));
    sort(&mut s, json!({"field": "Region", "order": "desc", "by": "Max of Sales"})).unwrap();
    sort(&mut s, json!({"field": "Product", "order": "custom", "items": ["Plums"]})).unwrap();
    let before = grid(&s);
    let mut back = reopen(&mut s);
    let fields = back.execute("pivot.fields", json!({})).unwrap();
    assert_eq!(fields["rows"][0]["sortBy"], "Max of Sales");
    assert_eq!(fields["rows"][1]["customOrder"], json!(["Plums", "Apples", "Pears"]), "the saved order lists every item");
    assert_eq!(grid(&back), before);
}

// ---------------------------------------------------------------- every combination

/// Every way of sorting, on a row or a column field, in every layout: the grand total holds and,
/// sorted by values, the outer items' totals are in order.
#[test]
fn every_sort_in_every_layout_keeps_the_totals() {
    let sorts = [
        json!({"order": "asc"}),
        json!({"order": "desc"}),
        json!({"order": "none"}),
        json!({"order": "asc", "by": "Sum of Sales"}),
        json!({"order": "desc", "by": "Sum of Sales"}),
        json!({"order": "desc", "by": "Count of Sales"}),
        json!({"order": "custom", "items": ["West"]}),
    ];
    for (rows, columns) in [(vec!["Region", "Product"], vec![]), (vec!["Product"], vec!["Region"]), (vec![], vec!["Region"])] {
        let mut s = sales();
        pivot(&mut s, &rows, &columns, json!(["Sales", {"field": "Sales", "func": "count"}]));
        for layout in ["compact", "outline", "tabular"] {
            s.execute("pivot.layout", json!({"layout": layout})).unwrap();
            for spec in &sorts {
                let mut p = spec.clone();
                p["field"] = json!("Region");
                sort(&mut s, p).unwrap_or_else(|e| panic!("{rows:?}/{columns:?} {layout} {spec}: {e}"));
                let g = grid(&s);
                let totals: Vec<f64> = g.iter().flatten().filter_map(Value::as_f64).collect();
                assert!(totals.contains(&625.0), "{rows:?}/{columns:?} {layout} {spec}: {g:?}");
                if rows.first() == Some(&"Region") && layout == "compact" && spec["by"] == "Sum of Sales" {
                    let outer: Vec<f64> = ["East", "North", "West"]
                        .iter()
                        .filter_map(|r| g.iter().position(|row| row[0].display() == *r).map(|i| (i, g[i][1].as_f64().unwrap())))
                        .collect::<std::collections::BTreeMap<_, _>>()
                        .into_values()
                        .collect();
                    let mut want = outer.clone();
                    want.sort_by(f64::total_cmp);
                    if spec["order"] == "desc" {
                        want.reverse();
                    }
                    assert_eq!(outer, want, "{layout} {spec}");
                }
            }
        }
    }
}

/// Sorting by values costs one lookup per item: 20,000 items sort at once.
#[test]
#[allow(clippy::disallowed_methods)] // the clock is read natively; this test doesn't run on wasm
fn many_items_sort_by_value_fast() {
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
    sort(&mut s, json!({"field": "Key", "order": "desc", "by": "Sum of Sales"})).unwrap();
    let first = grid(&s)[1][1].as_f64().unwrap();
    assert_eq!(first, 999.0);
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}
