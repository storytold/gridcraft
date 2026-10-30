//! Database requests (Data › From Database, Refresh) off the UI thread, so the window keeps
//! drawing while a server answers.
//!
//! The session's data source is wrapped once. While the interface runs a database command
//! ([`SheetApp::run_db`]) the wrapper starts each new request on a thread of its own and
//! answers [`PENDING`]: the command fails before changing anything. Each frame the app looks
//! at the requests ([`SheetApp::poll_db`]); once they are all in, the same command runs again
//! and the wrapper hands back the stored answers at once. Calls from elsewhere (automation,
//! macros) keep waiting for the answer, as before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use gridcraft_engine::datasource::{DataSource, DbRequest, DbTable, PENDING};
use serde_json::Value as Json;

use crate::SheetApp;

type Answer = Option<Result<DbTable, String>>;

/// The wrapper's shared state.
#[derive(Clone, Default)]
pub struct DbJobs {
    /// Requests started in the background and their answers (`None` while running).
    jobs: Arc<Mutex<Vec<(DbRequest, Answer)>>>,
    /// Set while the interface runs a database command.
    defer: Arc<AtomicBool>,
}

/// A database command waiting for its answers, and what started it.
pub struct Waiting {
    pub id: String,
    pub params: Json,
    /// The dialog that ran it (its OK, Connect or table pick), or `None` for the ribbon,
    /// keytips and command search.
    pub dialog: Option<String>,
}

impl DbJobs {
    /// Wraps `inner`; `wake` asks for a frame when an answer comes in.
    fn wrap(&self, inner: DataSource, wake: Arc<dyn Fn() + Send + Sync>) -> DataSource {
        let (jobs, defer) = (self.jobs.clone(), self.defer.clone());
        Arc::new(move |req: &DbRequest| {
            let mut list = jobs.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, answer)) = list.iter().find(|(r, _)| r == req) {
                return answer.clone().unwrap_or_else(|| Err(PENDING.to_string()));
            }
            if !defer.load(Ordering::SeqCst) {
                drop(list);
                return inner(req);
            }
            list.push((req.clone(), None));
            drop(list);
            let (jobs, inner, wake, req) = (jobs.clone(), inner.clone(), wake.clone(), req.clone());
            std::thread::spawn(move || {
                let answer = inner(&req);
                // A cancelled request is no longer listed: its answer is dropped.
                if let Some(slot) = jobs.lock().unwrap_or_else(PoisonError::into_inner).iter_mut().find(|(r, _)| *r == req) {
                    slot.1 = Some(answer);
                }
                wake();
            });
            Err(PENDING.to_string())
        })
    }

    /// Some request is still running.
    pub fn busy(&self) -> bool {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(_, a)| a.is_none())
    }

    /// Forgets every request and answer (the command that used them finished, or was cancelled).
    pub fn clear(&self) {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

/// The commands that reach a database.
pub fn is_db_command(id: &str) -> bool {
    matches!(id, "data.fromDatabase" | "data.databaseTables" | "data.databaseColumns" | "data.refresh" | "data.refreshAll")
}

impl SheetApp {
    /// Wraps the session's data source the first time a frame runs (desktop app only: the web
    /// build has no data source and no threads).
    pub(crate) fn install_db_jobs(&mut self, ctx: &egui::Context) {
        if cfg!(target_arch = "wasm32") || self.db_jobs.is_some() {
            return;
        }
        let Some(inner) = self.session.data_source.clone() else { return };
        let jobs = DbJobs::default();
        let ctx = ctx.clone();
        self.session.data_source = Some(jobs.wrap(inner, Arc::new(move || ctx.request_repaint())));
        self.db_jobs = Some(jobs);
    }

    /// Runs a database command for the interface: `None` while its requests run in the
    /// background (the command changed nothing; `poll_db` runs it again), else its result.
    pub(crate) fn run_db(&mut self, id: &str, params: Json) -> Option<Result<Json, String>> {
        let Some(jobs) = self.db_jobs.clone() else { return Some(self.run(id, params)) };
        jobs.defer.store(true, Ordering::SeqCst);
        let r = self.run(id, params);
        jobs.defer.store(false, Ordering::SeqCst);
        match r {
            Err(e) if e.contains(PENDING) => None,
            r => {
                jobs.clear();
                Some(r)
            }
        }
    }

    /// A request of the background wrapper is still running (dialogs wait on this).
    pub(crate) fn db_running(&self) -> bool {
        self.db_jobs.as_ref().is_some_and(DbJobs::busy)
    }

    /// A database request is running in the background.
    pub fn db_busy(&self) -> bool {
        self.db_waiting.is_some() || self.db_jobs.as_ref().is_some_and(DbJobs::busy)
    }

    /// Stops waiting: the running requests finish on their own, and their answers are dropped.
    pub fn cancel_db(&mut self) {
        if let Some(jobs) = &self.db_jobs {
            jobs.clear();
        }
        self.db_waiting = None;
    }

    /// Runs a database command from the ribbon, a keytip or command search: in the background,
    /// with its result shown as `run_or_alert` shows it once the answers are in.
    pub(crate) fn run_db_or_alert(&mut self, id: &str, params: Json) {
        match self.run_db(id, params.clone()) {
            None => self.db_waiting = Some(Waiting { id: id.to_string(), params, dialog: None }),
            Some(r) => self.after_db(id, r),
        }
    }

    fn after_db(&mut self, id: &str, r: Result<Json, String>) {
        match (id, r) {
            ("data.refreshAll", r) => crate::refresh_all_result(self, r),
            (_, Err(e)) => self.alert(e),
            (_, Ok(_)) => {}
        }
    }

    /// Each frame: once the requests of a waiting command are in, runs it again. A dialog's
    /// command is run again by the dialog itself (it watches `db_busy`).
    pub(crate) fn poll_db(&mut self) {
        let Some(jobs) = self.db_jobs.clone() else { return };
        if jobs.busy() {
            return;
        }
        let Some(w) = self.db_waiting.take_if(|w| w.dialog.is_none()) else { return };
        match self.run_db(&w.id, w.params.clone()) {
            None => self.db_waiting = Some(w),
            Some(r) => self.after_db(&w.id, r),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gridcraft_engine::Session;
    use gridcraft_engine::datasource::DbValue;
    use serde_json::json;

    use super::*;
    use crate::Services;

    /// An app whose data source answers after `delay`, with a table imported from it.
    fn app(delay: Duration) -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        let table = DbTable { columns: vec!["a".into()], rows: vec![vec![DbValue::Int(1)]], truncated: false };
        session.data_source = Some(Arc::new(move |_: &DbRequest| {
            std::thread::sleep(delay);
            Ok(table.clone())
        }));
        session.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q"})).unwrap();
        let mut app = SheetApp::new(session, Services::default());
        app.install_db_jobs(&egui::Context::default());
        app
    }

    /// Polls as frames do until the waiting command has run.
    fn settle(app: &mut SheetApp) {
        for _ in 0..500 {
            app.poll_db();
            if !app.db_busy() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the database command never finished");
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ribbon_refresh_waits_in_the_background() {
        let mut app = app(Duration::from_millis(300));
        let undo = app.session.active().unwrap().undo.len();
        let started = std::time::Instant::now();
        app.run_or_alert("data.refreshAll", json!({}));
        assert!(started.elapsed() < Duration::from_millis(200), "the interface waited for the database");
        assert!(app.db_busy());
        assert_eq!(app.session.active().unwrap().undo.len(), undo, "nothing changed yet");
        settle(&mut app);
        assert!(app.message.is_none(), "{:?}", app.message);
        assert_eq!(app.session.active().unwrap().undo.len(), undo + 1, "refreshed once");
    }

    #[test]
    fn cancel_drops_the_answer() {
        let mut app = app(Duration::from_millis(100));
        let undo = app.session.active().unwrap().undo.len();
        app.run_or_alert("data.refresh", json!({}));
        assert!(app.db_busy());
        app.cancel_db();
        assert!(!app.db_busy());
        std::thread::sleep(Duration::from_millis(200));
        app.poll_db();
        assert_eq!(app.session.active().unwrap().undo.len(), undo);
    }

    #[test]
    fn automation_still_waits_for_the_answer() {
        let mut app = app(Duration::from_millis(50));
        let r = app.run("data.refresh", json!({}));
        assert!(r.is_ok(), "{r:?}");
        assert!(!app.db_busy());
    }
}
