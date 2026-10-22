#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::disallowed_methods)] // Native worker deadlines, never executed on WASM.

use gridcraft_engine::Session;
use gridcraft_engine::{
    core::{CellRef, Value},
    odbc::OdbcManager,
};
use gridcraft_odbc::{Backend, Column, ColumnKind, Connection, DataSet, Limits, OdbcError, Sources};
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct Database {
    data: Mutex<DataSet>,
    blocked: AtomicBool,
    entered: AtomicBool,
}
impl Backend for Database {
    fn sources(&self) -> Result<Sources, OdbcError> {
        Ok(Sources { data_sources: vec!["Postgres".into()], drivers: vec!["PostgreSQL Unicode".into()] })
    }
    fn test(&self, _: &Connection) -> Result<(), OdbcError> {
        Ok(())
    }
    fn query(&self, connection: &Connection, sql: &str, _: Limits, _: bool, _: &AtomicBool) -> Result<DataSet, OdbcError> {
        self.entered.store(true, Ordering::Release);
        while self.blocked.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if sql == "SELECT failure" {
            return Err(OdbcError::QueryFailed);
        }
        if connection.dsn.as_deref() == Some("Another") {
            return Ok(data(vec![vec![Value::text("other database")]]));
        }
        Ok(self.data.lock().unwrap().clone())
    }
}

fn data(rows: Vec<Vec<Value>>) -> DataSet {
    DataSet { columns: vec![Column { name: "Value".into(), kind: ColumnKind::Text }], rows, truncated: false }
}

fn setup(rows: Vec<Vec<Value>>) -> (Session, Arc<Database>) {
    let database = Arc::new(Database { data: Mutex::new(data(rows)), blocked: AtomicBool::new(false), entered: AtomicBool::new(false) });
    let mut session = Session::new();
    session.odbc = OdbcManager::new(database.clone());
    session.new_workbook();
    (session, database)
}

fn wait(session: &mut Session, id: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = session.execute("data.odbcStatus", json!({"jobId": id})).unwrap();
        if status["state"] != "running" {
            return status;
        }
        assert!(Instant::now() < deadline, "ODBC worker did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn query(session: &mut Session, command: &str, params: serde_json::Value) -> String {
    let result = session.execute(command, params).unwrap();
    let id = result["jobId"].as_str().unwrap().to_string();
    assert_eq!(wait(session, &id)["state"], "ready");
    id
}

fn load(session: &mut Session) {
    let id = query(session, "data.fromOdbc", json!({"dsn":"Postgres", "user":"reader", "password":"do-not-save", "sql":"SELECT values"}));
    session.execute("data.odbcApply", json!({"jobId": id})).unwrap();
}

#[test]
fn odbc_load_is_literal_atomic_undoable_and_survives_xlsx() {
    let (mut session, _) = setup(vec![vec![Value::text("=SUM(A1:A2)")], vec![Value::text("0012")], vec![Value::Empty]]);
    let before = session.doc().unwrap().wb.clone();
    let id = query(&mut session, "data.fromOdbc", json!({"connectionString":"Driver={example};PWD=do-not-save", "sql":"SELECT values"}));
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().wb));
    session.execute("data.odbcApply", json!({"jobId":id})).unwrap();
    let imported = session.doc().unwrap().wb.clone();
    assert_eq!(session.doc().unwrap().undo.len(), 1);
    let sheet = imported.active().unwrap();
    assert_eq!(sheet.cell(CellRef::new(1, 0)).unwrap().value, Value::text("=SUM(A1:A2)"));
    assert!(sheet.cell(CellRef::new(1, 0)).unwrap().formula.is_none());
    assert_eq!(sheet.tables[0].query.as_ref().unwrap().sql, "SELECT values");
    assert!(!serde_json::to_string(&*imported).unwrap().contains("do-not-save"));
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&imported).unwrap()).unwrap();
    assert_eq!(reopened.active().unwrap().tables[0].query, sheet.tables[0].query);
    assert_eq!(reopened.active().unwrap().cell(CellRef::new(1, 0)).unwrap().value, Value::text("=SUM(A1:A2)"));
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*session.doc().unwrap().wb, *before);
    session.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*session.doc().unwrap().wb, *imported);
}

#[test]
fn odbc_refresh_shrinks_only_its_owned_cells_and_requires_replacing_edits() {
    let (mut session, database) = setup(vec![vec![Value::text("one")], vec![Value::text("two")]]);
    load(&mut session);
    session.execute("cell.set", json!({"cell":"B4", "input":"neighbor"})).unwrap();
    {
        let sheet = Arc::make_mut(&mut session.doc_mut().unwrap().wb).sheet_mut(1).unwrap();
        sheet.autofilter = Some(gridcraft_model::AutoFilter {
            range: gridcraft_engine::core::RangeRef::parse("B1:B4").unwrap(),
            criteria: vec![(0, gridcraft_model::FilterCriterion::Values { values: vec!["neighbor".into()], blanks: false })],
        });
        sheet.rows.entry(2).or_default().hidden = true;
    }
    *database.data.lock().unwrap() = data(vec![vec![Value::text("new")]]);
    let id = query(&mut session, "data.refreshQuery", json!({"table":"Query1"}));
    session.execute("data.odbcApply", json!({"jobId":id})).unwrap();
    let sheet = session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sheet.tables[0].range.a1(), "A1:A2");
    assert!(sheet.cell(CellRef::new(2, 0)).is_none());
    assert_eq!(sheet.cell(CellRef::new(3, 1)).unwrap().value, Value::text("neighbor"));
    assert!(sheet.is_row_hidden(2));
    assert_eq!(sheet.autofilter.as_ref().unwrap().range.a1(), "B1:B4");
    session.execute("cell.set", json!({"cell":"A2", "input":"local edit"})).unwrap();
    let id = query(&mut session, "data.refreshQuery", json!({"table":"Query1"}));
    let before = session.doc().unwrap().wb.clone();
    assert!(session.execute("data.odbcApply", json!({"jobId":id})).is_err());
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().wb));
    session.execute("data.odbcApply", json!({"jobId":id, "replace":true})).unwrap();
    assert_eq!(session.doc().unwrap().wb.active().unwrap().cell(CellRef::new(1, 0)).unwrap().value, Value::text("new"));
    Arc::make_mut(&mut session.doc_mut().unwrap().wb).sheet_mut(1).unwrap().tables[0].filter =
        vec![(0, gridcraft_model::FilterCriterion::Values { values: vec!["new".into()], blanks: false })];
    let id = query(&mut session, "data.refreshQuery", json!({"table":"Query1"}));
    let before = session.doc().unwrap().wb.clone();
    assert!(session.execute("data.odbcApply", json!({"jobId":id})).is_err());
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().wb));
    session.execute("data.odbcCancel", json!({"jobId":id})).unwrap();
}

#[test]
fn odbc_refresh_refuses_occupied_expansion_and_formulas() {
    let (mut session, database) = setup(vec![vec![Value::text("one")]]);
    load(&mut session);
    session.execute("cell.set", json!({"cell":"A3", "input":"neighbor"})).unwrap();
    *database.data.lock().unwrap() = data(vec![vec![Value::text("new")], vec![Value::text("new row")]]);
    let id = query(&mut session, "data.refreshQuery", json!({"table":"Query1"}));
    assert!(session.execute("data.odbcApply", json!({"jobId":id,"replace":true})).is_err());
    session.execute("data.odbcCancel", json!({"jobId":id})).unwrap();
    session.execute("cell.set", json!({"cell":"A2", "input":"=1+1"})).unwrap();
    let id = query(&mut session, "data.refreshQuery", json!({"table":"Query1"}));
    assert!(session.execute("data.odbcApply", json!({"jobId":id,"replace":true})).is_err());
    assert!(session.doc().unwrap().wb.active().unwrap().cell(CellRef::new(1, 0)).unwrap().formula.is_some());
}

#[test]
fn odbc_empty_failure_and_stale_results_preserve_the_workbook() {
    let (mut session, _) = setup(vec![]);
    load(&mut session);
    assert!(session.doc().unwrap().wb.active().unwrap().tables[0].data_range().is_none());
    let before = session.doc().unwrap().wb.clone();
    let result = session.execute("data.fromOdbc", json!({"dsn":"Postgres", "sql":"SELECT failure"})).unwrap();
    assert_eq!(wait(&mut session, result["jobId"].as_str().unwrap())["state"], "failed");
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().wb));
    let id = query(&mut session, "data.fromOdbc", json!({"dsn":"Postgres", "sql":"SELECT values"}));
    session.execute("cell.set", json!({"cell":"C1", "input":"changed"})).unwrap();
    assert!(session.execute("data.odbcApply", json!({"jobId":id})).is_err());
    session.new_workbook();
    assert!(session.execute("data.odbcApply", json!({"jobId":id})).is_err());
    assert_eq!(session.doc().unwrap().wb.sheets.len(), 1);
}

#[test]
fn odbc_cancel_keeps_a_busy_driver_accounted_for_until_it_returns() {
    let (mut session, database) = setup(vec![]);
    database.blocked.store(true, Ordering::Release);
    let result = session.execute("data.fromOdbc", json!({"dsn":"Postgres", "sql":"SELECT values"})).unwrap();
    let id = result["jobId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !database.entered.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    session.execute("data.odbcCancel", json!({"jobId":id})).unwrap();
    let busy = session.execute("data.fromOdbc", json!({"dsn":"Postgres", "sql":"SELECT values"}));
    database.blocked.store(false, Ordering::Release);
    assert!(busy.is_err());
    assert_eq!(wait(&mut session, id)["state"], "cancelled");
    assert!(session.execute("data.odbcApply", json!({"jobId":id})).is_err());
    assert_eq!(session.doc().unwrap().wb.sheets.len(), 1);
}

#[test]
fn odbc_refresh_all_applies_successes_once_and_preserves_failed_queries() {
    let (mut session, database) = setup(vec![vec![Value::text("cached")]]);
    load(&mut session);
    load(&mut session);
    let wb = Arc::make_mut(&mut session.doc_mut().unwrap().wb);
    wb.sheet_mut(2).unwrap().tables[0].query.as_mut().unwrap().sql = "SELECT failure".into();
    *database.data.lock().unwrap() = data(vec![vec![Value::text("updated")]]);
    let undo_count = session.doc().unwrap().undo.len();
    let before = session.doc().unwrap().wb.clone();
    let id = query(&mut session, "data.refreshAll", json!({"dsn":"Another"}));
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().wb));
    let result = session.execute("data.odbcApply", json!({"jobId":id})).unwrap();
    assert_eq!(result["queriesRefreshed"], 1);
    assert_eq!(result["errors"].as_array().unwrap().len(), 1);
    assert_eq!(session.doc().unwrap().undo.len(), undo_count + 1);
    assert_eq!(session.doc().unwrap().wb.sheet(1).unwrap().cell(CellRef::new(1, 0)).unwrap().value, Value::text("other database"));
    assert_eq!(session.doc().unwrap().wb.sheet(2).unwrap().cell(CellRef::new(1, 0)).unwrap().value, Value::text("cached"));
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*session.doc().unwrap().wb, *before);
    assert_eq!(
        session
            .odbc
            .profile(session.doc().unwrap().uid, &before.sheet(2).unwrap().tables[0].query.as_ref().unwrap().connection_ref)
            .unwrap()
            .dsn
            .as_deref(),
        Some("Postgres")
    );
}

#[test]
fn reopened_queries_do_not_pick_up_credentials_from_a_new_connection() {
    let (mut first, database) = setup(vec![vec![Value::text("original database")]]);
    load(&mut first);
    let bytes = gridcraft_xlsx::write_xlsx(&first.doc().unwrap().wb).unwrap();
    let (workbook, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let mut reopened = Session::new();
    reopened.odbc = OdbcManager::new(database);
    reopened.add_document(gridcraft_engine::DocState::new(workbook, None, "Reopened".into()));
    let id = query(&mut reopened, "data.fromOdbc", json!({"dsn":"Another", "sql":"SELECT values"}));
    reopened.execute("data.odbcApply", json!({"jobId":id})).unwrap();
    let id = query(&mut reopened, "data.refreshQuery", json!({"table":"Query1"}));
    reopened.execute("data.odbcApply", json!({"jobId":id})).unwrap();
    assert_eq!(reopened.doc().unwrap().wb.sheet(1).unwrap().cell(CellRef::new(1, 0)).unwrap().value, Value::text("original database"));
}

#[test]
fn odbc_commands_are_registered_and_validate_before_starting_work() {
    let mut session = Session::new();
    session.new_workbook();
    for command in ["data.odbcTest", "data.odbcPreview", "data.fromOdbc", "data.refreshQuery", "data.odbcStatus", "data.odbcApply", "data.odbcCancel"]
    {
        let error = session.execute(command, json!({})).expect_err("missing parameters");
        assert!(!matches!(error, gridcraft_engine::EngineError::UnknownCommand(_)), "{command}: {error}");
    }
}

#[test]
fn odbc_secrets_are_removed_recursively_from_recorded_parameters() {
    let recorded = gridcraft_engine::cmd::journal_params(&json!({
        "sql": "SELECT 1", "connectionString": "Driver=example;PWD=top-secret",
        "connection": {"dsn": "Example", "password": "top-secret", "nested": [{"apiPassword": "top-secret"}]}
    }));
    assert!(!recorded.to_string().contains("top-secret"));
    assert_eq!(recorded["sql"], "SELECT 1");
    assert_eq!(recorded["connection"]["dsn"], "Example");
}

#[test]
fn recorded_parameter_redaction_has_a_depth_limit() {
    let mut input = json!({"PWD":"do-not-save"});
    for _ in 0..100 {
        input = json!([input]);
    }
    let recorded = gridcraft_engine::cmd::journal_params(&input).to_string();
    assert!(!recorded.contains("do-not-save"));
    assert!(recorded.contains("null"));
}
