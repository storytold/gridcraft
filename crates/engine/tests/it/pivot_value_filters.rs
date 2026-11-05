//! Value filters (Value Filters, Top 10): items kept by their totals, within each group of the
//! outer fields, with filtered items leaving the totals too. The source (Region, Product, Sales):
//!
//! | Region | Apples | Pears | Plums | Total | Count |
//! |--------|-------:|------:|------:|------:|------:|
//! | East   |    160 |   120 |    60 |   340 |     4 |
//! | North  |     90 |    30 |     — |   120 |     2 |
//! | West   |     50 |    45 |    70 |   165 |     3 |
//! | Total  |    300 |   195 |   130 |   625 |     9 |

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

fn sales() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let mut rows = vec![json!(["Region", "Product", "Sales"])];
    rows.extend(RECORDS.iter().map(|(r, p, v)| json!([r, p, v])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s
}

fn pivot(s: &mut Session, rows: &[&str], columns: &[&str], values: Json) {
    s.execute("insert.pivotTable", json!({"rows": rows, "columns": columns, "values": values, "destination": "Sheet1!G3"})).unwrap();
}

fn vf(s: &mut Session, params: Json) -> Result<Json, String> {
    s.execute("pivot.valueFilter", params).map_err(|e| e.to_string())
}

fn grid(s: &Session) -> Vec<Vec<Value>> {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let r = sh.pivots[0].last_range.unwrap();
    (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.value(CellRef::new(row, col))).collect()).collect()
}

/// (row label, first value) of each body row below the header.
fn rows(s: &Session) -> Vec<(String, f64)> {
    grid(s).iter().skip(1).map(|r| (r[0].display(), r[1].as_f64().unwrap_or(f64::NAN))).collect()
}

fn labels(s: &Session) -> Vec<String> {
    rows(s).into_iter().map(|(l, _)| l).collect()
}

fn filters(s: &mut Session) -> Json {
    s.execute("pivot.fields", json!({})).unwrap()["filters"].clone()
}

fn row(label: &str, v: f64) -> (String, f64) {
    (label.to_string(), v)
}

// ---------------------------------------------------------------- top and bottom

#[test]
fn top_items_keep_the_largest_and_the_totals_follow() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 2})).unwrap();
    assert_eq!(rows(&s), [row("East", 340.0), row("West", 165.0), row("Grand Total", 505.0)]);
    vf(&mut s, json!({"field": "Region", "rule": "bottom", "count": 1})).unwrap();
    assert_eq!(rows(&s), [row("North", 120.0), row("Grand Total", 120.0)]);
}

#[test]
fn top_percent_and_sum_add_up_from_the_largest() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    // 50% of 625 is 312.5: East's 340 reaches it.
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 50, "of": "percent"})).unwrap();
    assert_eq!(labels(&s), ["East", "Grand Total"]);
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 400, "of": "sum"})).unwrap();
    assert_eq!(labels(&s), ["East", "West", "Grand Total"]);
    vf(&mut s, json!({"field": "Region", "rule": "bottom", "count": 200, "of": "sum"})).unwrap();
    assert_eq!(labels(&s), ["North", "West", "Grand Total"]);
}

#[test]
fn comparisons_test_each_items_total() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    for (params, want) in [
        (json!({"rule": "greater", "value": 150}), vec!["East", "West"]),
        (json!({"rule": "greaterOrEqual", "value": 165}), vec!["East", "West"]),
        (json!({"rule": "less", "value": 165}), vec!["North"]),
        (json!({"rule": "lessOrEqual", "value": 165}), vec!["North", "West"]),
        (json!({"rule": "equal", "value": 120}), vec!["North"]),
        (json!({"rule": "notEqual", "value": 120}), vec!["East", "West"]),
        (json!({"rule": "between", "value": 200, "value2": 100}), vec!["North", "West"]),
        (json!({"rule": "notBetween", "value": 100, "value2": 200}), vec!["East"]),
    ] {
        let mut p = params.clone();
        p["field"] = json!("Region");
        vf(&mut s, p).unwrap();
        let mut got = labels(&s);
        got.pop();
        assert_eq!(got, want, "{params}");
    }
}

#[test]
fn a_filter_that_keeps_nothing_leaves_an_empty_report() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Region", "rule": "greater", "value": 1e9})).unwrap();
    let g = grid(&s);
    assert!(g.iter().flatten().all(|v| v.as_f64().is_none_or(|n| n == 0.0)), "{g:?}");
}

// ---------------------------------------------------------------- groups and combinations

#[test]
fn an_inner_field_is_filtered_within_each_outer_item() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Product", "rule": "top", "count": 1})).unwrap();
    assert_eq!(
        rows(&s),
        [
            row("East", 160.0),
            row("Apples", 160.0),
            row("North", 90.0),
            row("Apples", 90.0),
            row("West", 70.0),
            row("Plums", 70.0),
            row("Grand Total", 320.0)
        ]
    );
}

#[test]
fn outer_filters_apply_first() {
    let mut s = sales();
    pivot(&mut s, &["Region", "Product"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Product", "rule": "top", "count": 1})).unwrap();
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 2})).unwrap();
    // Regions by their full totals (East 340, West 165), then each one's top product.
    assert_eq!(labels(&s), ["East", "Apples", "West", "Plums", "Grand Total"]);
    assert_eq!(rows(&s).last(), Some(&row("Grand Total", 230.0)));
}

#[test]
fn a_value_filter_judges_only_the_items_the_label_filter_shows() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["North", "West"]})).unwrap();
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 1})).unwrap();
    assert_eq!(labels(&s), ["West", "Grand Total"]);
    // Clearing one keeps the other.
    s.execute("pivot.filter", json!({"field": "Region", "selected": null})).unwrap();
    assert_eq!(labels(&s), ["East", "Grand Total"]);
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["North"]})).unwrap();
    vf(&mut s, json!({"field": "Region", "rule": "none"})).unwrap();
    assert_eq!(labels(&s), ["North", "Grand Total"]);
    s.execute("pivot.filter", json!({"field": "Region", "selected": null})).unwrap();
    assert_eq!(filters(&mut s), json!([]), "nothing left to keep");
}

#[test]
fn the_chosen_value_field_measures_with_its_own_function() {
    // The #2554 review: counting a text field read its values as zeros, so every item tied.
    let mut s = sales();
    pivot(&mut s, &["Product"], &[], json!(["Sales", "Region"]));
    vf(&mut s, json!({"field": "Product", "rule": "top", "count": 1, "by": "Count of Region"})).unwrap();
    assert_eq!(labels(&s), ["Apples", "Grand Total"], "Apples has 4 records, Pears 3, Plums 2");
    vf(&mut s, json!({"field": "Product", "rule": "bottom", "count": 1, "by": "Count of Region"})).unwrap();
    assert_eq!(labels(&s), ["Plums", "Grand Total"]);
}

#[test]
fn column_items_are_filtered_by_their_column_totals() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &["Product"], json!(["Sales"]));
    vf(&mut s, json!({"field": "Product", "rule": "top", "count": 2})).unwrap();
    let header: Vec<String> = grid(&s)[1].iter().map(Value::display).collect();
    assert_eq!(header, ["Row Labels", "Apples", "Pears", "Grand Total"]);
    assert_eq!(grid(&s).last().and_then(|r| r.last()).and_then(Value::as_f64), Some(495.0));
}

#[test]
fn show_values_as_uses_the_filtered_totals() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 2})).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "percentOfGrandTotal"})).unwrap();
    let east = rows(&s)[0].1;
    assert!((east - 340.0 / 505.0).abs() < 1e-12, "{east}");
}

// ---------------------------------------------------------------- keeping it consistent

#[test]
fn value_filters_follow_their_fields() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales", {"field": "Sales", "func": "max"}]));
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 1, "by": "Max of Sales"})).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 1, "name": "Biggest"})).unwrap();
    assert_eq!(filters(&mut s)[0]["valueFilter"]["by"], "Biggest", "a new caption carries over");
    s.execute("pivot.removeField", json!({"field": "Biggest", "area": "values"})).unwrap();
    assert_eq!(filters(&mut s), json!([]), "its value field is gone");
    assert_eq!(labels(&s), ["East", "North", "West", "Grand Total"]);
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 1})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!(filters(&mut s)[0]["valueFilter"]["rule"], "top", "moving between rows and columns keeps it");
    s.execute("pivot.moveField", json!({"field": "Region", "from": "columns", "to": "filters"})).unwrap();
    assert_eq!(filters(&mut s)[0]["valueFilter"], Json::Null, "a report filter has no value filter");
}

#[test]
fn value_filter_parameters_are_validated() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    let err = |s: &mut Session, p: Json| vf(s, p).unwrap_err();
    assert!(err(&mut s, json!({"field": "Region"})).contains("missing `rule`"));
    assert!(err(&mut s, json!({"field": "Region", "rule": "around"})).contains("unknown rule"));
    for count in [json!(0), json!(-3), json!(2.5), json!("x"), json!(2_000_000)] {
        assert!(err(&mut s, json!({"field": "Region", "rule": "top", "count": count})).contains("count"), "{count}");
    }
    assert!(err(&mut s, json!({"field": "Region", "rule": "top", "count": 101, "of": "percent"})).contains("count"));
    assert!(err(&mut s, json!({"field": "Region", "rule": "top", "of": "dozen"})).contains("unknown `of`"));
    assert!(err(&mut s, json!({"field": "Region", "rule": "greater"})).contains("`value` is a number"));
    assert!(err(&mut s, json!({"field": "Region", "rule": "between", "value": 1})).contains("`value2` is a number"));
    assert!(err(&mut s, json!({"field": "Region", "rule": "top", "by": "Profit"})).contains("isn't in the values area"));
    assert!(err(&mut s, json!({"field": "Sales", "rule": "top"})).contains("isn't in the rows or columns"));
    assert_eq!(filters(&mut s), json!([]), "nothing changed");
    let set = vf(&mut s, json!({"field": "region", "rule": "top"})).unwrap();
    assert_eq!(
        set["valueFilter"],
        json!({"by": "Sum of Sales", "rule": "top", "count": 10.0, "of": "items"}),
        "defaults: top 10 items by the first value field"
    );
}

#[test]
fn undo_restores_the_unfiltered_report() {
    let mut s = sales();
    pivot(&mut s, &["Region"], &[], json!(["Sales"]));
    vf(&mut s, json!({"field": "Region", "rule": "top", "count": 1})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(labels(&s), ["East", "North", "West", "Grand Total"]);
    assert_eq!(filters(&mut s), json!([]));
}

// ---------------------------------------------------------------- file round trip

#[test]
fn every_rule_survives_an_xlsx_round_trip() {
    for params in [
        json!({"rule": "top", "count": 2}),
        json!({"rule": "bottom", "count": 1}),
        json!({"rule": "top", "count": 50, "of": "percent"}),
        json!({"rule": "bottom", "count": 200, "of": "sum"}),
        json!({"rule": "equal", "value": 120}),
        json!({"rule": "notEqual", "value": 120}),
        json!({"rule": "greater", "value": 150}),
        json!({"rule": "greaterOrEqual", "value": 165}),
        json!({"rule": "less", "value": 165}),
        json!({"rule": "lessOrEqual", "value": 165}),
        json!({"rule": "between", "value": 100, "value2": 200}),
        json!({"rule": "notBetween", "value": 100, "value2": 200}),
    ] {
        let mut s = sales();
        pivot(&mut s, &["Region", "Product"], &[], json!(["Sales", {"field": "Sales", "func": "max"}]));
        let mut p = params.clone();
        p["field"] = json!("Region");
        p["by"] = json!("Max of Sales");
        vf(&mut s, p).unwrap();
        s.execute("pivot.filter", json!({"field": "Region", "selected": ["East", "West"]})).unwrap();
        let before = grid(&s);
        let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        let mut back = Session::new();
        back.execute("file.open", json!({"name": "f.xlsx", "base64": saved["base64"]})).unwrap();
        back.execute("pivot.refresh", json!({"pivot": "PivotTable1", "replace": true})).unwrap();
        assert_eq!(filters(&mut back), filters(&mut s), "{params}");
        assert_eq!(grid(&back), before, "{params}");
    }
}

// ---------------------------------------------------------------- every combination

/// Every rule on an outer, inner or column field in every layout: the grand total is the sum of
/// the items left, and the report never shows a filtered item.
#[test]
fn every_rule_in_every_layout_keeps_the_totals_consistent() {
    let rules = [
        json!({"rule": "top", "count": 1}),
        json!({"rule": "bottom", "count": 2}),
        json!({"rule": "top", "count": 30, "of": "percent"}),
        json!({"rule": "top", "count": 100, "of": "sum"}),
        json!({"rule": "greater", "value": 60}),
        json!({"rule": "between", "value": 40, "value2": 130}),
        json!({"rule": "none"}),
    ];
    for (rows_, columns, field) in [
        (vec!["Region"], vec![], "Region"),
        (vec!["Region", "Product"], vec![], "Product"),
        (vec!["Region", "Product"], vec![], "Region"),
        (vec!["Region"], vec!["Product"], "Product"),
        (vec![], vec!["Region"], "Region"),
    ] {
        let mut s = sales();
        pivot(&mut s, &rows_, &columns, json!(["Sales", {"field": "Sales", "func": "count"}]));
        for layout in ["compact", "outline", "tabular"] {
            s.execute("pivot.layout", json!({"layout": layout})).unwrap();
            for rule in &rules {
                let mut p = rule.clone();
                p["field"] = json!(field);
                vf(&mut s, p).unwrap_or_else(|e| panic!("{rows_:?}/{columns:?} {field} {layout} {rule}: {e}"));
                let g = grid(&s);
                for v in g.iter().flatten() {
                    if let Value::Number(n) = v {
                        assert!(n.is_finite() && *n >= 0.0 && *n <= 625.0, "{rule}: {n}");
                    }
                }
                if rows_ == ["Region"] && columns.is_empty() && layout == "compact" {
                    let body = rows(&s);
                    let (items, total) = body.split_at(body.len() - 1);
                    let sum: f64 = items.iter().map(|(_, v)| v).sum();
                    assert_eq!(total.first().map(|(_, v)| *v), Some(sum), "{rule}");
                }
            }
        }
    }
}
