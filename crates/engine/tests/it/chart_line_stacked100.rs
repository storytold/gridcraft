//! The 100% stacked line charts: `insert.chart` and `chart.set` take type `line` with subtype
//! `stacked100` or `stacked100Markers`.

use gridcraft_engine::Session;
use gridcraft_model::ChartKind;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "North", "South"], ["Q1", "10", "30"], ["Q2", "30", "10"]]})).unwrap();
    s
}

fn kinds(s: &Session) -> Vec<ChartKind> {
    s.doc().unwrap().wb.sheet(0).unwrap().charts.iter().map(|c| c.kind).collect()
}

#[test]
fn insert_chart_makes_hundred_percent_stacked_lines() {
    let mut s = session();
    s.execute("insert.chart", json!({"range": "A1:C3", "type": "line", "subtype": "stacked100"})).unwrap();
    s.execute("insert.chart", json!({"range": "A1:C3", "type": "line", "subtype": "stacked100Markers"})).unwrap();
    assert_eq!(kinds(&s), [ChartKind::LineStacked100, ChartKind::LineStacked100Markers]);
}

#[test]
fn chart_set_changes_a_chart_to_a_hundred_percent_stacked_line_and_back() {
    let mut s = session();
    s.execute("insert.chart", json!({"range": "A1:C3", "type": "line", "subtype": "stacked"})).unwrap();
    assert_eq!(kinds(&s), [ChartKind::LineStacked]);
    s.execute("chart.set", json!({"type": "line", "subtype": "stacked100Markers"})).unwrap();
    assert_eq!(kinds(&s), [ChartKind::LineStacked100Markers]);
    s.execute("chart.set", json!({"type": "line", "subtype": "stacked100"})).unwrap();
    assert_eq!(kinds(&s), [ChartKind::LineStacked100]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(kinds(&s), [ChartKind::LineStacked100Markers]);
}
