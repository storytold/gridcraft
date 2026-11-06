//! The existing warning bar receives insertion-budget failures at the common command boundary.
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
/// Menu and dialog commands share run; its JSON result stays identical to the engine response.
fn insertion_format_budget_uses_the_existing_warning_without_changing_json() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.run("home.bold", json!({"range":"A1:Z1"})).unwrap();
    let result = app.run("home.insertRows", json!({"rows":"2:40002"})).unwrap();
    assert_eq!(result, json!({"formattingSkipped":true,"formattingCellLimit":1_000_000,"formattingComparisonLimit":4_000_000}));
    let warning = app.run("ui.notice", json!({})).unwrap();
    assert_eq!(warning["kind"], "warning");
    assert!(warning["text"].as_str().unwrap().contains("formatting"));
    app.run("ui.notice.dismiss", json!({})).unwrap();
    assert_eq!(app.run("home.insertRows", json!({"rows":"2:2"})).unwrap(), serde_json::Value::Null);
    assert_eq!(app.run("ui.notice", json!({})).unwrap(), serde_json::Value::Null, "a normal insert must not create a warning");
}

#[test]
/// A formatting warning preserves an existing warning and its action; ordinary commands leave it alone.
fn insertion_warning_preserves_existing_notices() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    let existing = gridcraft_ui_egui::Notice {
        text: "Existing file warning.".into(),
        kind: gridcraft_ui_egui::NoticeKind::Warning,
        action: Some(("Details".into(), "macros".into())),
    };
    app.notice = Some(existing.clone());
    app.run("home.bold", json!({"range":"A1:Z1"})).unwrap();
    assert_eq!(app.notice, Some(existing.clone()));
    app.run("home.insertRows", json!({"rows":"2:40002"})).unwrap();
    let notice = app.notice.as_ref().unwrap();
    assert!(notice.text.starts_with(&existing.text) && notice.text.contains("formatting"));
    assert_eq!(notice.action, existing.action);
}

#[test]
#[ignore = "manual screenshot of the existing warning bar; needs a graphics adapter"]
/// Save the actual existing warning bar through the repository's offscreen renderer.
fn capture_the_existing_insert_format_warning() {
    let path = std::env::var("GRIDCRAFT_CAPTURE_INSERT_WARNING").expect("explicit artifact path");
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.run("home.bold", json!({"range":"A1:Z1"})).unwrap();
    assert_eq!(app.run("home.insertRows", json!({"rows":"2:40002"})).unwrap()["formattingSkipped"], true);
    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_pixels_per_point(1.0).with_max_steps(12).wgpu().build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
    harness.run();
    harness.render().expect("offscreen graphics adapter").save(path).unwrap();
}
