//! Page Setup opens on the active sheet's saved margins, so OK without edits keeps them (issue #524).

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value, json};

fn margins(app: &SheetApp) -> [f32; 6] {
    app.session.active().and_then(|d| d.wb.active()).map(|s| s.print.margins).unwrap_or_default()
}

fn app_with_custom_margins() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.run("pageLayout.margins", json!({"left": 2.0, "right": 2.5, "top": 3.0, "bottom": 0.25, "header": 0.1, "footer": 0.2})).unwrap();
    assert_eq!(margins(&app), [2.0, 2.5, 3.0, 0.25, 0.1, 0.2]);
    app
}

#[test]
fn page_setup_ok_without_edits_keeps_custom_margins() {
    // Custom Margins / File > Page Setup open it bare; Print Titles opens it on the Sheet tab.
    for params in [json!({}), json!({"tab": "Sheet"})] {
        let mut app = app_with_custom_margins();
        app.open_dialog("pageSetup", params);
        let d = app.dialog.take().unwrap();
        let shown = json!({"left": 2.0, "right": 2.5, "top": 3.0, "bottom": 0.25, "header": 0.1, "footer": 0.2});
        assert_eq!(Value::Object(d.values.clone()), shown);
        app.run("pageLayout.margins", Value::Object(d.values)).unwrap();
        assert_eq!(margins(&app), [2.0, 2.5, 3.0, 0.25, 0.1, 0.2]);
    }
}

#[test]
fn page_setup_one_edited_field_keeps_the_other_margins() {
    let mut app = app_with_custom_margins();
    app.open_dialog("pageSetup", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("left".into(), json!(1.5));
    app.run("pageLayout.margins", Value::Object(d.values)).unwrap();
    assert_eq!(margins(&app), [1.5, 2.5, 3.0, 0.25, 0.1, 0.2]);
}
