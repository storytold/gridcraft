use super::*;

#[test]
fn chart_axis_title_font_steps_from_the_displayed_default_without_changing_cells() {
    for element in [chartview::ChartElement::XAxisTitle, chartview::ChartElement::YAxisTitle] {
        for (command, expected) in [("home.increaseFontSize", 11.0), ("home.decreaseFontSize", 9.0)] {
            let mut session = Session::new();
            session.new_workbook();
            session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
            let id = session.execute("insert.chart", json!({"range":"A1", "type":"column"})).unwrap()["chart"].as_u64().unwrap() as u32;
            let mut app = SheetApp::new(session, Default::default());
            let ctx = egui::Context::default();
            chartview::select_element(&ctx, id, element);
            app.chart_context = Some(ctx);
            app.selected_chart = Some(id);
            let before = serde_json::to_value(&app.session.doc().unwrap().wb.active().unwrap().cells).unwrap();
            assert_eq!(app.selected_chart_font().unwrap().size, Some(10.0));
            app.run(command, json!({})).unwrap();
            assert_eq!(app.selected_chart_font().unwrap().size, Some(expected));
            assert_eq!(serde_json::to_value(&app.session.doc().unwrap().wb.active().unwrap().cells).unwrap(), before);
        }
    }
}

#[test]
fn chart_home_font_targets_text_without_touching_source_cells_and_supports_undo() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
    let result = session.execute("insert.chart", json!({"range":"A1","type":"column","title":"Sales"})).unwrap();
    let id = result["chart"].as_u64().unwrap() as u32;
    let mut app = SheetApp::new(session, Default::default());
    let ctx = egui::Context::default();
    chartview::select_element(&ctx, id, chartview::ChartElement::Title);
    app.chart_context = Some(ctx);
    app.selected_chart = Some(id);
    let cells = serde_json::to_value(&app.session.doc().unwrap().wb.active().unwrap().cells).unwrap();
    for (command, params) in [
        ("home.fontName", json!({"name":"DejaVu Sans"})),
        ("home.fontSize", json!({"size":24})),
        ("home.fontColor", json!({"color":"#123456"})),
        ("home.bold", json!({"on":true})),
    ] {
        app.run(command, params).unwrap();
    }
    let doc = app.session.doc().unwrap();
    assert_eq!(serde_json::to_value(&doc.wb.active().unwrap().cells).unwrap(), cells);
    let chart = &doc.wb.active().unwrap().charts[0];
    let data = gridcraft_chart::resolve(&doc.wb, 0, chart);
    let prims = gridcraft_chart::render(chart, &data, 640.0, 400.0, &gridcraft_chart::ApproxMeasure);
    assert!(prims.iter().any(|p|matches!(p,gridcraft_chart::Prim::Text {role,size,color,bold,format,..} if role=="title" && *size==24.0 && *color==[0x12,0x34,0x56,255] && *bold && format.name.as_deref()==Some("DejaVu Sans"))));
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.selected_chart_font().unwrap().bold, None);
    assert!(app.run("home.fontSize", json!({"size":0})).is_err());
    assert_eq!(serde_json::to_value(&app.session.doc().unwrap().wb.active().unwrap().cells).unwrap(), cells);
    app.selected_chart = None;
    app.run("home.fontSize", json!({"size":18})).unwrap();
    assert_ne!(serde_json::to_value(&app.session.doc().unwrap().wb.active().unwrap().cells).unwrap(), cells);
}

#[test]
fn explicit_cell_font_target_stays_a_cell_command_when_chart_is_selected() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"1"})).unwrap();
    let result = session.execute("insert.chart", json!({"range":"A1", "type":"column", "title":"Sales"})).unwrap();
    let id = result["chart"].as_u64().unwrap() as u32;
    let mut app = SheetApp::new(session, Default::default());
    app.selected_chart = Some(id);
    let before_cells = serde_json::to_value(&app.session.active().unwrap().wb.active().unwrap().cells).unwrap();
    let before_format = app.session.active().unwrap().wb.active().unwrap().charts[0].format.clone();
    app.run("home.bold", json!({"range":"B2", "on":true})).unwrap();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    assert_ne!(serde_json::to_value(&sheet.cells).unwrap(), before_cells);
    assert_eq!(sheet.charts[0].format, before_format);
}
