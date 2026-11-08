//! #3519: saving a workbook adds it to Recent, like opening one does.

use gridcraft_engine::Session;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A scratch folder that goes away with the test, even when an assertion fails first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        // The process id separates parallel test binaries; the counter separates parallel
        // tests in this one. A failed run leaves nothing behind: `Drop` cleans up too.
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        let run = RUNS.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gridcraft-recent-{name}-{}-{run}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch folder");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let dir = Scratch::new("save");
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
}

#[test]
fn a_copy_that_is_not_the_workbook_file_is_not_recent() {
    let dir = Scratch::new("copy");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).expect("type");

    app.run("file.saveAs", serde_json::json!({"path": p(&dir.join("report.pdf"))})).expect("pdf copy");
    assert!(app.ui.recent.is_empty(), "a PDF is a print, not the workbook's file");

    app.run("file.saveAs", serde_json::json!({"path": p(&dir.join("report.csv"))})).expect("csv copy");
    assert!(app.ui.recent.is_empty(), "a CSV copy leaves the workbook without a file of its own");

    let own = dir.join("report.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&own)})).expect("save");
    assert_eq!(app.ui.recent, vec![p(&own)], "the workbook's own file is recent");
}

#[test]
fn a_failed_save_does_not_touch_recent() {
    let dir = Scratch::new("failed");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).expect("type");

    let good = dir.join("good.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&good)})).expect("save as");
    assert_eq!(app.ui.recent, vec![p(&good)]);

    // A path whose parent is a regular file: the file system refuses the write.
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"not a folder").expect("blocker file");
    let bad = blocker.join("bad.xlsx");
    assert!(app.run("file.saveAs", serde_json::json!({"path": p(&bad)})).is_err(), "saving under a regular file fails");
    assert_eq!(app.ui.recent, vec![p(&good)], "a failed Save As leaves Recent alone");

    // A plain Save whose folder is gone fails the same way.
    app.run("cell.set", serde_json::json!({"cell": "A2", "input": "7"})).expect("type");
    std::fs::remove_dir_all(dir.path()).expect("remove scratch folder");
    assert!(app.run("file.save", serde_json::json!({})).is_err(), "saving into a removed folder fails");
    assert_eq!(app.ui.recent, vec![p(&good)], "a failed Save leaves Recent alone");
}

#[test]
fn saving_more_than_twenty_workbooks_keeps_the_twenty_newest() {
    let dir = Scratch::new("cap");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "1"})).expect("type");

    for i in 0..25 {
        let path = dir.join(&format!("book{i}.xlsx"));
        app.run("file.saveAs", serde_json::json!({"path": p(&path)})).expect("save as");
    }

    let expected: Vec<String> = (5..25).rev().map(|i| p(&dir.join(&format!("book{i}.xlsx")))).collect();
    assert_eq!(app.ui.recent.len(), 20, "the list is capped at twenty, even when saving");
    assert_eq!(app.ui.recent, expected, "the twenty newest, oldest dropped");
}

#[test]
fn a_save_through_the_session_does_not_reorder_recent() {
    let dir = Scratch::new("autosave");
    let mut app = app();
    app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).expect("type");

    // Two workbooks: the first owns `a.xlsx`, the second `b.xlsx`.
    let a = dir.join("a.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&a)})).expect("save as");
    app.run("file.new", serde_json::json!({})).expect("new workbook");
    let b = dir.join("b.xlsx");
    app.run("file.saveAs", serde_json::json!({"path": p(&b)})).expect("save as");
    assert_eq!(app.ui.recent, vec![p(&b), p(&a)], "the second workbook saved is the most recent");

    // Work on the first workbook again: it becomes active, but `Recent` is not a file-open list.
    let first = app.session.documents().iter().find(|d| d.path.as_deref() == Some(&p(&a))).expect("first workbook").uid;
    app.run("window.activate", serde_json::json!({"doc": first})).expect("activate");
    app.session.run("file.save", serde_json::json!({})).expect("the autosave's own save");
    assert_eq!(app.ui.recent, vec![p(&b), p(&a)], "AutoSave saves through the session, so Recent keeps its order");
}
