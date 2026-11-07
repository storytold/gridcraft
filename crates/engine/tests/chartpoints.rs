//! Format Data Point (`chart.formatSelection` with `point`), Invert if Negative and vary colors
//! by point: set, undone, saved and reopened.

use gridcraft_engine::Session;
use gridcraft_engine::model::{Chart, Color, MarkerSymbol};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["", "Plan", "Actual"], ["w", 5, 4], ["x", -3, -2], ["y", 4, 6], ["z", -2, 3]]}))
        .unwrap();
    s.execute("insert.chart", json!({"type": "column", "range": "A1:C5"})).unwrap();
    s
}

fn chart(s: &Session) -> Chart {
    s.doc().unwrap().wb.active().unwrap().charts[0].clone()
}

fn reopen(s: &mut Session) {
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
}

#[test]
fn a_point_fill_is_set_saved_reset_and_undone() {
    let mut s = session();
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2, "color": "#FF0000", "line": "#0000FF"})).unwrap();
    let p = chart(&s).series[1].points.clone();
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].index, p[0].fill, p[0].line), (2, Some(Color::Rgb(0xFF0000)), Some(Color::Rgb(0x0000FF))));
    assert!(chart(&s).series[0].points.is_empty());
    // Shape Fill on a selected point does the same.
    s.execute("chart.shapeFill", json!({"series": 0, "point": 0, "color": "#00FF00"})).unwrap();
    assert_eq!(chart(&s).series[0].points[0].fill, Some(Color::Rgb(0x00FF00)));
    assert_eq!(chart(&s).series[0].color, None, "the series keeps its own fill");
    reopen(&mut s);
    assert_eq!(chart(&s).series[1].points, p);
    // Reset to Match Style, then undo it.
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2})).unwrap();
    assert!(chart(&s).series[1].points.is_empty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(chart(&s).series[1].points, p);
}

#[test]
fn markers_explosion_invert_and_vary_colors() {
    let mut s = session();
    s.execute("chart.formatSelection", json!({"series": 0, "invertIfNegative": true, "invertColor": "#FFFF00"})).unwrap();
    let c = chart(&s);
    assert!(c.series[0].invert_if_negative);
    assert_eq!(c.series[0].invert_color, Some(Color::Rgb(0xFFFF00)));
    assert_eq!(c.series[0].color, None, "setting invert alone leaves the fill");
    s.execute("chart.formatSelection", json!({"series": 0, "point": 1, "invertIfNegative": false})).unwrap();
    assert_eq!(chart(&s).series[0].points[0].invert_if_negative, Some(false));
    s.execute("chart.formatSelection", json!({"series": 1, "varyColors": true})).unwrap();
    assert_eq!(chart(&s).vary_colors, Some(true));
    s.execute("chart.changeType", json!({"type": "line", "subtype": "markers"})).unwrap();
    s.execute("chart.formatSelection", json!({"series": 1, "point": 3, "marker": "diamond", "markerSize": 300, "markerColor": "#FF0000"})).unwrap();
    let m = chart(&s).series[1].points[0].marker.clone().unwrap();
    assert_eq!((m.symbol, m.size, m.fill), (MarkerSymbol::Diamond, Some(72), Some(Color::Rgb(0xFF0000))));
    s.execute("chart.changeType", json!({"type": "pie"})).unwrap();
    s.execute("chart.formatSelection", json!({"point": 0, "explosion": 9999})).unwrap();
    assert_eq!(chart(&s).series[0].points.iter().find(|p| p.index == 0).and_then(|p| p.explosion), Some(400));
    reopen(&mut s);
    let c = chart(&s);
    assert_eq!(c.series[0].points.iter().find(|p| p.index == 0).and_then(|p| p.explosion), Some(400));
}

#[test]
fn bad_point_parameters_are_errors_not_panics() {
    let mut s = session();
    for p in [
        json!({"point": -1, "color": "#FF0000"}),
        json!({"point": "two", "color": "#FF0000"}),
        json!({"point": 1e12, "color": "#FF0000"}),
        json!({"point": 1, "marker": "hexagon"}),
    ] {
        assert!(s.execute("chart.formatSelection", p.clone()).is_err(), "{p}");
    }
    // A point past the data is kept (Excel keeps positional point formats) and harmless.
    s.execute("chart.formatSelection", json!({"series": 0, "point": 1_048_575, "color": "#FF0000"})).unwrap();
    // A series that doesn't exist changes nothing.
    s.execute("chart.formatSelection", json!({"series": 99, "point": 0, "color": "#FF0000"})).unwrap();
    assert!(chart(&s).series.iter().all(|x| x.points.len() <= 1));
    assert!(s.execute("chart.formatSelection", json!({})).is_ok());
}

#[test]
fn chart_wide_and_series_settings_leave_a_named_point_alone() {
    let mut s = session();
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2, "color": "#FF0000"})).unwrap();
    // varyColors with a point named is a chart-wide setting: the point keeps its fill.
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2, "varyColors": true})).unwrap();
    assert_eq!(chart(&s).series[1].points.len(), 1);
    assert_eq!(chart(&s).vary_colors, Some(true));
    // invertColor with a point named is a series setting: it is applied and the point stays.
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2, "invertColor": "#FFFF00"})).unwrap();
    assert_eq!(chart(&s).series[1].invert_color, Some(Color::Rgb(0xFFFF00)));
    assert_eq!(chart(&s).series[1].points.len(), 1);
    // With no other setting the call still resets the point to Match Style.
    s.execute("chart.formatSelection", json!({"series": 1, "point": 2})).unwrap();
    assert!(chart(&s).series[1].points.is_empty());
}
