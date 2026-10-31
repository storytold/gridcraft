//! Get & Transform in the window: the Queries & Connections pane, the Power Query Editor (edit a
//! draft, Close & Load), the Security Warning bar with Enable Content, and German labels.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness_with(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn with_sales_query() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("data.fromTextCsv", json!({"text": "Region,Units\nNorth,10\nSouth,7\n", "name": "Sales"})).unwrap();
    harness_with(session)
}

fn cell(h: &Harness, sheet: &str, a1: &str) -> Value {
    let d = h.state().session.active().unwrap();
    let si = d.wb.sheet_index(sheet).unwrap();
    d.wb.sheet(si).unwrap().value(CellRef::parse(a1).unwrap())
}

#[test]
fn the_queries_pane_lists_queries_with_their_status() {
    let mut h = with_sales_query();
    h.state_mut().run("data.queriesConnections", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Queries & Connections").is_some(), "the pane's title");
    assert!(h.query_by_label("Sales").is_some(), "the query is listed");
    assert!(h.query_by_label("2 rows loaded.").is_some(), "with its row count");
    // The pane closes with the command again.
    h.state_mut().run("data.queriesConnections", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("2 rows loaded.").is_none());
}

#[test]
fn the_editor_edits_a_draft_and_close_and_load_saves_it() {
    let mut h = with_sales_query();
    gridcraft_ui_egui::power_query::open_editor(h.state_mut(), Some("Sales"), None);
    h.run_steps(4);
    assert!(h.query_by_label("APPLIED STEPS").is_some(), "Query Settings shows the steps");
    assert!(h.query_by_label("Changed Type").is_some(), "the steps Power Query added");
    // Select the Region column, remove it: a new applied step, not yet saved.
    h.get_by_label("ABC  Region").click();
    h.run_steps(2);
    h.get_by_label("Remove Columns").click();
    h.run_steps(4);
    assert!(h.query_by_label("Removed Columns").is_some(), "the new step is listed");
    assert_eq!(cell(&h, "Sales", "A1"), Value::text("Region"), "the workbook isn't changed before Close & Load");
    // Close & Load saves the draft and refreshes the table.
    h.get_by_label("Close & Load ▾").click();
    h.run_steps(2);
    h.get_by_label("Close & Load").click();
    h.run_steps(4);
    assert!(h.state().power_query.is_none(), "the editor closed");
    let d = h.state().session.active().unwrap();
    assert_eq!(d.wb.queries[0].steps.last().map(|s| s.name.as_str()), Some("Removed Columns"));
    assert_eq!(cell(&h, "Sales", "A1"), Value::text("Units"));
}

#[test]
fn the_security_warning_offers_enable_content() {
    // A workbook saved with a query that reads a file, opened from disk.
    let dir = std::env::temp_dir().join(format!("gridcraft-pq-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("data.csv");
    std::fs::write(&csv, "K\nold\n").unwrap();
    let mut maker = Session::new();
    maker.new_workbook();
    maker.execute("data.fromTextCsv", json!({"path": csv.to_string_lossy()})).unwrap();
    let book = dir.join("book.xlsx");
    maker.execute("file.saveAs", json!({"path": book.to_string_lossy()})).unwrap();
    std::fs::write(&csv, "K\nnew\n").unwrap();

    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness_with(session);
    h.state_mut().open_path(&book.to_string_lossy());
    h.run_steps(3);
    let notice = h.state().notice.clone().expect("the Security Warning bar");
    assert!(notice.text.contains("External data connections have been disabled"), "{}", notice.text);
    assert!(h.state_mut().run("data.refreshAll", json!({})).is_err(), "refresh is refused while blocked");
    h.get_by_label("Enable Content").click();
    h.run_steps(3);
    assert!(h.state().notice.is_none());
    assert!(!h.state().session.active().unwrap().external_blocked);
    h.state_mut().run("data.refreshAll", json!({})).unwrap();
    assert_eq!(cell(&h, "data", "A2"), Value::text("new"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn german_labels() {
    let mut h = with_sales_query();
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.state_mut().run("data.queriesConnections", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Abfragen und Verbindungen").is_some());
    assert!(h.query_by_label("2 Zeilen geladen.").is_some());
    gridcraft_ui_egui::power_query::open_editor(h.state_mut(), Some("Sales"), None);
    h.run_steps(4);
    assert!(h.query_by_label("ANGEWENDETE SCHRITTE").is_some());
    assert!(h.query_by_label("Schließen und laden ▾").is_some());
}
