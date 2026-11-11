//! Grouping a field's items: number ranges (Group › Starting at, Ending at, By) and the time
//! parts of date serials (hours, minutes, seconds).

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

/// Age, Time (a time of day or a date and time), Sales; one row per record.
const RECORDS: [(f64, f64, f64); 8] = [
    (23.0, 0.25, 10.0),          // 6:00 AM
    (35.0, 0.5, 20.0),           // 12:00 PM
    (37.0, 0.520_833_333, 30.0), // 12:30 PM
    (41.0, 1.75, 40.0),          // the next day, 6:00 PM
    (58.0, 0.0, 50.0),           // midnight
    (62.0, 0.999_988_426, 60.0), // 11:59:59 PM
    (79.0, 45_000.25, 70.0),     // a date at 6:00 AM
    (79.5, 0.5, 80.0),           // 12:00 PM
];

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let mut rows = vec![json!(["Age", "Time", "Sales", "Name"])];
    rows.extend(RECORDS.iter().enumerate().map(|(i, (a, t, v))| json!([a, t, v, format!("n{i}")])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s
}

fn pivot(s: &mut Session, rows: &[&str], columns: &[&str]) {
    s.execute("insert.pivotTable", json!({"rows": rows, "columns": columns, "values": ["Sales"], "destination": "Sheet1!H3"})).unwrap();
}

fn group(s: &mut Session, params: Json) -> Result<Json, String> {
    s.execute("pivot.group", params).map_err(|e| e.to_string())
}

fn grid(s: &Session) -> Vec<Vec<Value>> {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let r = sh.pivots[0].last_range.unwrap();
    (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.value(CellRef::new(row, col))).collect()).collect()
}

/// (row label, first value) of each body row, without the header row.
fn rows(s: &Session) -> Vec<(String, f64)> {
    grid(s).iter().skip(1).map(|r| (r[0].display(), r[1].as_f64().unwrap_or(f64::NAN))).collect()
}

fn labels(s: &Session) -> Vec<String> {
    rows(s).into_iter().map(|(l, _)| l).collect()
}

fn field_json(s: &mut Session) -> Json {
    s.execute("pivot.fields", json!({})).unwrap()["rows"][0].clone()
}

// ---------------------------------------------------------------- number ranges

#[test]
fn numbers_fall_into_ranges_with_their_sums() {
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 10})).unwrap();
    assert_eq!(
        rows(&s),
        [
            ("20-29".into(), 10.0),
            ("30-39".into(), 50.0),
            ("40-49".into(), 40.0),
            ("50-59".into(), 50.0),
            ("60-69".into(), 60.0),
            ("70-79".into(), 70.0),
            (">79".into(), 80.0),
            ("Grand Total".into(), 360.0),
        ]
    );
    let f = field_json(&mut s);
    assert_eq!((f["group"].clone(), f["numberGroup"].clone()), (json!("numbers"), json!({"start": 20.0, "end": 79.0, "interval": 10.0})));
}

#[test]
fn the_field_list_samples_read_like_the_report() {
    // #3395 review: the samples ignored the field's grouping and listed raw numbers.
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 10})).unwrap();
    let fields = s.execute("pivot.fields", json!({})).unwrap()["fields"].clone();
    let age = fields.as_array().unwrap().iter().find(|f| f["name"] == "Age").unwrap().clone();
    assert_eq!(age["samples"], json!(["20-29", "30-39", "40-49", "50-59", "60-69"]));
    let sales = fields.as_array().unwrap().iter().find(|f| f["name"] == "Sales").unwrap().clone();
    assert_eq!(sales["samples"][0], "10", "a field off the axes lists its values");
}

#[test]
fn numbers_below_the_start_come_first_and_above_the_end_last() {
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 30, "end": 60, "interval": 15})).unwrap();
    // 62 is past the end: the 60-74 range has no records and doesn't show.
    assert_eq!(labels(&s), ["<30", "30-44", "45-59", ">60", "Grand Total"]);
    s.execute("pivot.sort", json!({"field": "Age", "order": "desc"})).unwrap();
    assert_eq!(labels(&s), [">60", "45-59", "30-44", "<30", "Grand Total"], "descending reverses the ranges");
}

#[test]
fn the_default_range_runs_from_the_smallest_to_the_largest_number() {
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers"})).unwrap();
    let f = field_json(&mut s);
    assert_eq!(f["numberGroup"], json!({"start": 23.0, "end": 79.5, "interval": 10.0}));
    // 79.5 isn't a whole number but the start and interval are: ranges name their last whole number.
    assert_eq!(labels(&s), ["23-32", "33-42", "53-62", "73-82", "Grand Total"]);
}

#[test]
fn fractional_intervals_name_both_ends() {
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 79, "end": 80, "interval": 0.5})).unwrap();
    assert_eq!(labels(&s), ["<79", "79-79.5", "79.5-80", "Grand Total"]);
}

#[test]
fn a_range_can_be_filtered_and_collapsed_by_its_label() {
    let mut s = session();
    pivot(&mut s, &["Age", "Name"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 20})).unwrap();
    s.execute("pivot.filter", json!({"field": "Age", "selected": ["20-39"]})).unwrap();
    let shown: Vec<String> = labels(&s);
    assert_eq!(shown, ["20-39", "n0", "n1", "n2", "Grand Total"]);
    s.execute("pivot.collapse", json!({"field": "Age", "items": ["20-39"]})).unwrap();
    assert_eq!(labels(&s), ["20-39", "Grand Total"]);
}

#[test]
fn regrouping_forgets_hidden_and_collapsed_items() {
    let mut s = session();
    pivot(&mut s, &["Age", "Name"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 20})).unwrap();
    s.execute("pivot.filter", json!({"field": "Age", "selected": ["20-39"]})).unwrap();
    s.execute("pivot.collapse", json!({"field": "Age", "items": ["20-39"]})).unwrap();
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 0, "end": 100, "interval": 50})).unwrap();
    assert_eq!(labels(&s).len(), 8 + 2 + 1, "both ranges with their names, and the grand total");
    let f = field_json(&mut s);
    assert_eq!(f["collapsed"], json!([]));
    assert!(s.execute("pivot.fields", json!({})).unwrap()["filters"].as_array().unwrap().is_empty());
}

#[test]
fn a_new_grouping_replaces_the_old_and_none_ungroups() {
    let mut s = session();
    pivot(&mut s, &["Time"], &[]);
    group(&mut s, json!({"field": "Time", "by": "numbers", "start": 0, "end": 2, "interval": 1})).unwrap();
    group(&mut s, json!({"field": "Time", "by": "hours"})).unwrap();
    let f = field_json(&mut s);
    assert_eq!((f["group"].clone(), f["numberGroup"].clone()), (json!("hours"), Json::Null));
    group(&mut s, json!({"field": "Time", "by": "numbers", "start": 0, "end": 2, "interval": 1})).unwrap();
    assert_eq!(field_json(&mut s)["group"], "numbers");
    group(&mut s, json!({"field": "Time", "by": "none"})).unwrap();
    let f = field_json(&mut s);
    assert_eq!((f["group"].clone(), f["numberGroup"].clone()), (json!("none"), Json::Null));
    assert_eq!(labels(&s).len(), 8, "seven distinct times and the grand total");
}

#[test]
fn number_grouping_is_validated() {
    let mut s = session();
    pivot(&mut s, &["Age", "Name"], &[]);
    let err = |s: &mut Session, p: Json| group(s, p).unwrap_err();
    assert!(err(&mut s, json!({"field": "Name", "by": "numbers"})).contains("Cannot group"), "text");
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "interval": 0})).contains("greater than 0"));
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "interval": -5})).contains("greater than 0"));
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "start": 50, "end": 10})).contains("ending number"));
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "start": 0, "end": 1e9, "interval": 1})).contains("ranges"));
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "start": "x"})).contains("is a number"));
    assert!(err(&mut s, json!({"field": "Age", "by": "numbers", "interval": 1e308, "start": -1e308, "end": 1e308})).contains("ranges"));
    assert!(err(&mut s, json!({"field": "Age", "by": "weeks"})).contains("unknown grouping"));
    assert!(err(&mut s, json!({"field": "Sales", "by": "numbers"})).contains("isn't in the rows or columns"));
    // A failed call changes nothing.
    assert_eq!(field_json(&mut s)["group"], "none");
}

#[test]
fn blanks_and_column_fields_group_too() {
    let mut s = session();
    s.execute("cell.set", json!({"cell": "A3", "input": ""})).unwrap();
    pivot(&mut s, &["Name"], &["Age"]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 0, "end": 99, "interval": 50})).unwrap();
    let header: Vec<String> = grid(&s)[1].iter().map(Value::display).collect();
    assert_eq!(header, ["Row Labels", "0-49", "50-99", "(blank)", "Grand Total"]);
}

// ---------------------------------------------------------------- time parts

#[test]
fn hours_minutes_and_seconds_caption_like_excel() {
    let mut s = session();
    pivot(&mut s, &["Time"], &[]);
    group(&mut s, json!({"field": "Time", "by": "hours"})).unwrap();
    assert_eq!(
        rows(&s),
        [
            ("12 AM".into(), 50.0),
            ("6 AM".into(), 80.0),
            ("12 PM".into(), 130.0),
            ("6 PM".into(), 40.0),
            ("11 PM".into(), 60.0),
            ("Grand Total".into(), 360.0)
        ]
    );
    group(&mut s, json!({"field": "Time", "by": "minutes"})).unwrap();
    assert_eq!(labels(&s), [":00", ":30", ":59", "Grand Total"]);
    group(&mut s, json!({"field": "Time", "by": "seconds"})).unwrap();
    assert_eq!(labels(&s), [":00", ":59", "Grand Total"]);
}

// ---------------------------------------------------------------- file round trip

fn reopen(s: &mut Session) -> Session {
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut back = Session::new();
    back.execute("file.open", json!({"name": "g.xlsx", "base64": saved["base64"]})).unwrap();
    back.execute("pivot.refresh", json!({"pivot": "PivotTable1", "replace": true})).unwrap();
    back
}

#[test]
fn number_ranges_survive_an_xlsx_round_trip_with_their_hidden_items() {
    let mut s = session();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 10})).unwrap();
    s.execute("pivot.filter", json!({"field": "Age", "selected": ["30-39", ">79"]})).unwrap();
    let before = grid(&s);
    let mut back = reopen(&mut s);
    assert_eq!(field_json(&mut back)["numberGroup"], json!({"start": 20.0, "end": 79.0, "interval": 10.0}));
    assert_eq!(grid(&back), before);
}

#[test]
fn a_blank_item_survives_an_xlsx_round_trip_with_the_ranges() {
    // #3395 review: the saved groups had no "(blank)" item, so a selection of it was lost.
    let mut s = session();
    s.execute("cell.set", json!({"cell": "A3", "input": ""})).unwrap();
    pivot(&mut s, &["Age"], &[]);
    group(&mut s, json!({"field": "Age", "by": "numbers", "start": 20, "end": 79, "interval": 30})).unwrap();
    s.execute("pivot.filter", json!({"field": "Age", "selected": ["(blank)", "20-49"]})).unwrap();
    assert_eq!(labels(&s), ["20-49", "(blank)", "Grand Total"]);
    let before = grid(&s);
    let mut back = reopen(&mut s);
    assert_eq!(grid(&back), before);
    assert_eq!(back.execute("pivot.fields", json!({})).unwrap()["filters"][0]["selected"], json!(["20-49", "(blank)"]));
}

#[test]
fn time_parts_survive_an_xlsx_round_trip_with_their_hidden_items() {
    for by in ["hours", "minutes", "seconds"] {
        let mut s = session();
        pivot(&mut s, &["Time"], &[]);
        group(&mut s, json!({"field": "Time", "by": by})).unwrap();
        let first = labels(&s)[0].clone();
        let second = labels(&s)[1].clone();
        s.execute("pivot.filter", json!({"field": "Time", "selected": [first, second]})).unwrap();
        let before = grid(&s);
        let mut back = reopen(&mut s);
        assert_eq!(field_json(&mut back)["group"], by);
        assert_eq!(grid(&back), before, "{by}");
    }
}

// ---------------------------------------------------------------- every combination

/// Every grouping, on a row or a column field, in every layout: the report renders and the
/// grand total stays the sum of the source.
#[test]
fn every_grouping_in_every_layout_keeps_the_grand_total() {
    let groupings = [
        json!({"by": "numbers", "start": 20, "end": 79, "interval": 10}),
        json!({"by": "numbers", "start": 0.1, "end": 0.9, "interval": 0.1}),
        json!({"by": "numbers", "start": -5, "end": -1, "interval": 2}),
        json!({"by": "numbers"}),
        json!({"by": "hours"}),
        json!({"by": "minutes"}),
        json!({"by": "seconds"}),
        json!({"by": "days"}),
        json!({"by": "none"}),
    ];
    for field in ["Age", "Time"] {
        for (rows, columns) in [(vec![field, "Name"], vec![]), (vec!["Name"], vec![field]), (vec![], vec![field])] {
            let mut s = session();
            pivot(&mut s, &rows, &columns);
            for layout in ["compact", "outline", "tabular"] {
                s.execute("pivot.layout", json!({"layout": layout})).unwrap();
                for g in &groupings {
                    let mut p = g.clone();
                    p["field"] = json!(field);
                    group(&mut s, p).unwrap_or_else(|e| panic!("{field} {layout} {g}: {e}"));
                    let g2 = grid(&s);
                    let total = g2.last().and_then(|r| r.last()).and_then(Value::as_f64);
                    assert_eq!(total, Some(360.0), "{field} {rows:?}/{columns:?} {layout} {g}");
                }
            }
        }
    }
}
