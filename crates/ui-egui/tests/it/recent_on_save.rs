//! #3519: saving a workbook adds it to Recent, like opening one does.

use gridcraft_engine::Session;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-recent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn p(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn app() -> gridcraft_ui_egui::SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    crate::common::app(session)
}

#[test]
fn saving_a_new_workbook_records_it_in_recent() {
    let dir = scratch("save");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).expect("type");
    assert!(app.ui.recent.is_empty(), "a workbook that was neither opened nor saved is not recent");

    let first = dir.join("financeiro.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&first)})).expect("save as");
    assert_eq!(app.ui.recent, vec![p(&first)], "saving a new workbook makes it recent");

    let second = dir.join("orcamento.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&second)})).expect("save as");
    assert_eq!(app.ui.recent, vec![p(&second), p(&first)], "newest first");

    app.run("file.save", serde_json::json!({})).expect("save");
    assert_eq!(app.ui.recent, vec![p(&second), p(&first)], "a plain Save does not duplicate the entry");

    app.run("file.saveAs", serde_json::json!({"path": p(&first)})).expect("save as");
    assert_eq!(app.ui.recent, vec![p(&first), p(&second)], "saving as an earlier file moves it to the front");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_that_is_not_the_workbook_file_is_not_recent() {
    let dir = scratch("copy");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).expect("type");

    app.run("file.saveAs", serde_json::json!({"path": p(&dir.join("report.pdf"))})).expect("pdf copy");
    assert!(app.ui.recent.is_empty(), "a PDF is a print, not the workbook's file");

    app.run("file.saveAs", serde_json::json!({"path": p(&dir.join("report.csv"))})).expect("csv copy");
    assert!(app.ui.recent.is_empty(), "a CSV copy leaves the workbook without a file of its own");

    let own = dir.join("report.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&own)})).expect("save");
    assert_eq!(app.ui.recent, vec![p(&own)], "the workbook's own file is recent");

    let _ = std::fs::remove_dir_all(&dir);
}
