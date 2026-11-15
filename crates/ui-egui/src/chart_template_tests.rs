use crate::{SheetApp, chart_tools};
use gridcraft_engine::Session;
use serde_json::json;

#[test]
fn templates_preserve_the_visible_series_palette_and_explicit_overrides() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1","values":[["X","North","South"],[1,2,3],[2,4,6],[3,6,9]]})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    let first = app.run("insert.chart", json!({"range":"A1:C4","type":"line"})).unwrap()["chart"].as_u64().unwrap() as u32;
    app.run("chart.changeColors", json!({"chart":first,"palette":"monochrome"})).unwrap();
    app.run("chart.set", json!({"chart":first,"format":{"series":{"1":{"line_color":[11,22,33,255],"line_width":3.0}}}})).unwrap();
    let original = app.selected_chart_snapshot().unwrap();
    chart_tools::save_template(&mut app, first, "Monochrome lines").unwrap();
    app.run("insert.chart", json!({"range":"A1:C4","type":"line"})).unwrap();
    let wb = &app.session.doc().unwrap().wb;
    let applied = wb.active().unwrap().charts.last().unwrap();
    let draw = |chart| {
        gridcraft_chart::render(chart, &gridcraft_chart::resolve(wb, wb.active_sheet(), chart), 480.0, 300.0, &gridcraft_chart::ApproxMeasure)
    };
    assert_eq!(draw(&original), draw(applied));
    assert_eq!(applied.format.series[&1].line_color, Some([11, 22, 33, 255]));
    assert_eq!(wb.active().unwrap().charts[0], original, "saving a template must not mutate its source");
}

#[test]
fn saved_template_is_applied_to_future_charts_of_the_same_kind_and_persisted() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1", "values":[["X", "Y"], [1, 2], [2, 4]]})).unwrap();
    let mut app = SheetApp::new(session, Default::default());

    let first = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    app.run("chart.set", json!({"chart":first, "format":{"border_width":2.5, "vertical_gridlines":true}})).unwrap();
    chart_tools::save_template(&mut app, first, "Thesis columns").unwrap();

    let second = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    let chart = app.session.active().unwrap().wb.active().unwrap().charts.iter().find(|chart| chart.id == second).unwrap();
    assert_eq!(chart.format.border_width, Some(2.5));
    assert!(chart.format.vertical_gridlines);

    let encoded = serde_json::to_vec(&app.ui).unwrap();
    let restored: crate::UiState = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(restored.chart_templates.len(), 1);
    assert_eq!(restored.chart_templates[0].name, "Thesis columns");
    assert!(restored.chart_templates[0].use_for_new);

    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts.len(), 1);
    app.run("edit.redo", json!({})).unwrap();
    let restored_chart = app.session.doc().unwrap().wb.active().unwrap().charts.iter().find(|chart| chart.id == second).unwrap();
    assert_eq!(restored_chart.format.border_width, Some(2.5));
    assert!(restored_chart.format.vertical_gridlines);

    // Replay must see the same single insertion, including Undo, without UI preferences.
    let mut replay = Session::new();
    replay.new_workbook();
    for (command, params) in &app.session.journal {
        replay.execute(command, params.clone()).unwrap();
    }
    assert_eq!(serde_json::to_value(&replay.doc().unwrap().wb).unwrap(), serde_json::to_value(&app.session.doc().unwrap().wb).unwrap());
    app.run("edit.undo", json!({})).unwrap();
    replay.execute("edit.undo", json!({})).unwrap();
    assert_eq!(serde_json::to_value(&replay.doc().unwrap().wb).unwrap(), serde_json::to_value(&app.session.doc().unwrap().wb).unwrap());
}

#[test]
fn default_template_preserves_capped_undo_history() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    let first = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    app.run("chart.set", json!({"chart":first, "format":{"border_width":2.5}})).unwrap();
    chart_tools::save_template(&mut app, first, "Columns").unwrap();
    for value in 0..110 {
        app.run("cell.set", json!({"cell":"B1", "input":value.to_string()})).unwrap();
    }
    assert_eq!(app.session.doc().unwrap().undo.len(), 100);
    let previous_second = app.session.doc().unwrap().undo[1].wb.clone();
    app.run("insert.chart", json!({"type":"column"})).unwrap();
    let doc = app.session.doc().unwrap();
    assert_eq!(doc.undo.len(), 100);
    assert!(std::sync::Arc::ptr_eq(&doc.undo[0].wb, &previous_second), "template must not evict another history entry");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts.len(), 1);
}

#[test]
fn explicit_chart_appearance_takes_precedence_over_saved_defaults() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    let first = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    app.run("chart.set", json!({"chart":first, "style":2, "format":{"border_width":2.5}})).unwrap();
    chart_tools::save_template(&mut app, first, "Defaults").unwrap();
    app.run("insert.chart", json!({"type":"column", "style":6, "legend":"none", "format":{"border_width":4.0}})).unwrap();
    let chart = &app.session.doc().unwrap().wb.active().unwrap().charts[1];
    assert_eq!(chart.style, 6);
    assert_eq!(chart.legend, gridcraft_engine::model::LegendPos::None);
    assert_eq!(chart.format.border_width, Some(4.0));
}

#[test]
fn invalid_default_template_reports_error_and_leaves_inserted_chart_undoable() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    let first = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    chart_tools::save_template(&mut app, first, "Invalid columns").unwrap();
    app.ui.chart_templates[0].format.text.insert("title".into(), gridcraft_engine::model::ChartTextFormat { size: Some(0.0), ..Default::default() });
    let before = app.session.doc().unwrap().undo.len();
    app.run("insert.chart", json!({"type":"column"})).unwrap();
    assert!(app.message.as_ref().unwrap().1.contains("Invalid chart text format"));
    assert_eq!(app.session.doc().unwrap().undo.len(), before + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts.len(), 1);
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts.len(), 2);
    assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts[1].format, Default::default());
}

#[test]
fn malformed_saved_chart_templates_do_not_discard_other_ui_preferences() {
    let valid = crate::ChartTemplate {
        name: "Reusable chart".into(),
        kind: gridcraft_engine::model::ChartKind::ColumnClustered,
        style: 4,
        legend: gridcraft_engine::model::LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        format: Default::default(),
        use_for_new: true,
    };
    let mut saved = serde_json::to_value(crate::UiState { dark: true, chart_templates: vec![valid.clone()], ..Default::default() }).unwrap();
    saved["chartTemplates"] = json!([{"name":"incomplete"}, serde_json::to_value(&valid).unwrap()]);

    let restored: crate::UiState = serde_json::from_value(saved).unwrap();
    assert!(restored.dark);
    assert_eq!(restored.chart_templates.len(), 1);
    assert_eq!(restored.chart_templates[0].name, valid.name);
    assert_eq!(restored.chart_templates[0].style, valid.style);
    assert!(restored.chart_templates[0].use_for_new);

    let malformed: crate::UiState = serde_json::from_value(json!({
        "dark": true,
        "chartTemplates": {"unexpected": "object"}
    }))
    .unwrap();
    assert!(malformed.dark);
    assert!(malformed.chart_templates.is_empty());
}

#[test]
fn template_name_limit_counts_unicode_characters_instead_of_utf8_bytes() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    let chart = app.run("insert.chart", json!({"type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;

    let eighty_characters = "é".repeat(80);
    assert!(chart_tools::save_template(&mut app, chart, &eighty_characters).is_ok());
    assert_eq!(app.ui.chart_templates[0].name, eighty_characters);
    assert!(chart_tools::save_template(&mut app, chart, &"é".repeat(81)).is_err());
    assert_eq!(app.ui.chart_templates.len(), 1);
}
