use crate::{SheetApp, chart_tools};
use gridcraft_engine::Session;
use serde_json::json;

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
