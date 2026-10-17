use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, control};
use serde_json::json;

#[test]
fn odbc_dialog_credentials_are_hidden_from_control_responses() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    let context = egui::Context::default();
    app.open_dialog("odbc", json!({"password":"do-not-expose", "connectionString":"Driver=example;PWD=do-not-expose"}));
    assert_eq!(app.dialog.as_ref().unwrap().name, "odbc");
    assert!(!control::inspect(&app, &context).to_string().contains("do-not-expose"));
    match control::handle(&mut app, &context, "ui.dialog.set", &json!({"field":"password", "value":"do-not-expose"})) {
        control::Outcome::Done(value) => assert!(!value.to_string().contains("do-not-expose")),
        _ => panic!("expected a control response"),
    }
}

#[cfg(target_os = "windows")]
#[allow(clippy::disallowed_methods)] // Native worker deadlines, never executed on WASM.
mod interactions {
    use super::*;
    use egui::vec2;
    use egui_kittest::kittest::Queryable;
    use gridcraft_odbc::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    struct Database {
        blocked: AtomicBool,
        entered: AtomicBool,
    }
    impl Backend for Database {
        fn sources(&self) -> Result<Sources, OdbcError> {
            Ok(Sources { data_sources: vec!["Postgres".into()], drivers: vec![] })
        }
        fn test(&self, _: &Connection) -> Result<(), OdbcError> {
            Ok(())
        }
        fn query(&self, _: &Connection, _: &str, _: Limits, _: bool, _: &AtomicBool) -> Result<DataSet, OdbcError> {
            self.entered.store(true, Ordering::Release);
            while self.blocked.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(DataSet {
                columns: vec![Column { name: "Value".into(), kind: ColumnKind::Text }],
                rows: vec![vec![gridcraft_engine::core::Value::text("=1")]],
                truncated: false,
            })
        }
    }
    fn harness(database: Arc<Database>) -> egui_kittest::Harness<'static, SheetApp> {
        let mut session = Session::new();
        session.new_workbook();
        session.odbc = gridcraft_engine::odbc::OdbcManager::new(database);
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1280.0, 900.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            SheetApp::new(session, Default::default()),
        );
        harness.run_steps(6);
        harness
    }
    fn database(blocked: bool) -> Arc<Database> {
        Arc::new(Database { blocked: AtomicBool::new(blocked), entered: AtomicBool::new(false) })
    }
    fn finish(harness: &mut egui_kittest::Harness<'static, SheetApp>, predicate: impl Fn(&SheetApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate(harness.state()) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
            harness.run_steps(2);
        }
    }

    #[test]
    fn odbc_preview_does_not_edit_and_load_creates_a_literal_table() {
        let mut harness = harness(database(false));
        harness.state_mut().open_dialog("odbc", json!({"dsn":"Postgres", "sql":"SELECT 1"}));
        harness.run_steps(4);
        let before = harness.state().session.doc().unwrap().wb.clone();
        harness.get_by_label("Preview (100 rows)").click();
        harness.run_steps(4);
        finish(&mut harness, |app| app.session.odbc.status("odbc-1").is_some_and(|j| j.state == gridcraft_engine::odbc::JobState::Completed));
        assert!(Arc::ptr_eq(&before, &harness.state().session.doc().unwrap().wb));
        let _ = harness.get_by_label("=1");
        harness.get_by_label("Load as Table").click();
        harness.run_steps(4);
        finish(&mut harness, |app| app.dialog.is_none());
        let sheet = harness.state().session.doc().unwrap().wb.active().unwrap();
        assert_eq!(sheet.tables.len(), 1);
        assert!(sheet.cell(gridcraft_engine::core::CellRef::new(1, 0)).unwrap().formula.is_none());
        assert_eq!(harness.state().session.doc().unwrap().undo.len(), 1);
    }

    #[test]
    fn closing_the_odbc_dialog_cancels_a_running_load() {
        let database = database(true);
        let mut harness = harness(database.clone());
        harness.state_mut().open_dialog("odbc", json!({"dsn":"Postgres", "sql":"SELECT 1"}));
        harness.run_steps(4);
        harness.get_by_label("Load as Table").click();
        harness.run_steps(4);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !database.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let context = harness.ctx.clone();
        control::handle(harness.state_mut(), &context, "ui.dialog.cancel", &json!({}));
        database.blocked.store(false, Ordering::Release);
        finish(&mut harness, |app| app.session.odbc.status("odbc-1").is_some_and(|j| j.state == gridcraft_engine::odbc::JobState::Cancelled));
        assert_eq!(harness.state().session.doc().unwrap().wb.sheets.len(), 1);
    }

    #[test]
    fn missing_connection_settings_keep_the_form_open() {
        let mut harness = harness(database(false));
        harness.state_mut().open_dialog("odbc", json!({}));
        harness.run_steps(4);
        harness.get_by_label("Load as Table").click();
        harness.run_steps(4);
        assert!(harness.state().dialog.is_some());
        assert_eq!(harness.state().session.doc().unwrap().wb.sheets.len(), 1);
    }
}
