use egui_kittest::kittest::Queryable;
use gridcraft_engine::model::ChartKind;
use serde_json::json;

#[test]
fn histogram_chart_output_checkbox_creates_the_report_and_chart() {
    let mut s = crate::support::blank_session();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [3], [3], [4], [5], [6], [8]]})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [[2], [4], [6]]})).unwrap();
    let mut h = crate::support::harness(s, 4);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "histogram"}));
    h.run_steps(2);
    h.check_accessibility();
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("range".into(), json!("A1:A7"));
    d.values.insert("bins".into(), json!("B1:B3"));
    d.values.insert("newSheet".into(), json!("Distribution"));
    assert_eq!(d.values.get("chartOutput"), Some(&json!(false)));
    h.run_steps(2);
    for (key, label) in [("chartOutput", "Chart Output"), ("pareto", "Pareto (sorted histogram)"), ("cumulative", "Cumulative Percentage")] {
        h.get_by_label(label).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values.get(key), Some(&json!(true)), "{label} is checked");
    }
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none());
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(sh.name, "Distribution");
    assert_eq!(sh.charts.len(), 1);
    assert_eq!(sh.charts[0].kind, ChartKind::Combo);
    let chart = gridcraft_chart::resolve(&d.wb, d.wb.active_sheet(), &sh.charts[0]);
    assert_eq!(chart.categories, ["4", "6", "2", "More"]);
    assert_eq!(chart.series[0].values, [Some(3.0), Some(2.0), Some(1.0), Some(1.0)]);
    assert!(chart.series[1].secondary);
}
