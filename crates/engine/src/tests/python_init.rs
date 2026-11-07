//! Initialization code, through the fake worker: when it runs, what a failure does, and how a person
//! gets out of one. Its code speaks the fake worker's little language, as cells' code does.

use std::sync::Arc;
use std::time::Duration;

use gridcraft_core::{Array, CellError, Value};
use gridcraft_model::python::{MAX_INIT_CHARS, MAX_INIT_CHARS_KEPT, is_guid};
use gridcraft_model::{PyEnvironment, Workbook};
use gridcraft_python::Limits;
use gridcraft_python::fake::{FakeHandle, FakeTransport};
use gridcraft_python::protocol::{Init, Request};
use gridcraft_python::{Output, PyError};
use serde_json::{Value as Json, json};

use super::python_run::{capability, diagnostic, py, read_requests, runs, session, set, settle, with_limits, worker};
use super::v;
use crate::python::{PythonCalculation, PythonCapability, PythonState};
use crate::{Session, Trust};

const NUM: fn(f64) -> Value = Value::Number;
const PYTHON_ERROR: Value = Value::Error(CellError::Python);

fn init(s: &mut Session, code: &str) -> Json {
    s.execute("python.initialization", json!({"code": code})).unwrap()
}

fn code_of(s: &mut Session) -> String {
    s.execute("python.initialization", json!({})).unwrap()["code"].as_str().unwrap().to_string()
}

/// The rows of `python.diagnostics` about the initialization code.
fn init_rows(s: &mut Session) -> Vec<Json> {
    let rows = s.execute("python.diagnostics", json!({})).unwrap();
    rows.as_array().unwrap().iter().filter(|r| r["initialization"] == true).cloned().collect()
}

/// The workbook's environment, as a file left it.
fn from_file(s: &mut Session, env: PyEnvironment) {
    let d = s.active_mut().unwrap();
    Arc::make_mut(&mut d.wb).python_env = Some(Arc::new(env));
}

#[test]
fn it_runs_before_the_cells_in_the_variables_they_share() {
    let (mut s, h) = session();
    init(&mut s, "set rate 2");
    assert_eq!(h.starts(), 0, "with no cell, nothing starts: setting it runs nothing by itself");
    py(&mut s, "A1", "get rate");
    py(&mut s, "A2", "ret 5");
    settle(&mut s);
    assert_eq!(runs(&h), ["set rate 2", "get rate", "ret 5"]);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (NUM(2.0), NUM(5.0)));
    assert_eq!(h.starts(), 1, "one worker, which keeps them");
    // It is the initialization, and says whose it is.
    let first = h.requests().remove(0);
    assert!(matches!(first, Request::Run { init: Some(Init::Person), .. }), "{first:?}");
    assert!(matches!(h.requests().remove(1), Request::Run { init: None, .. }));
    // It holds no cell.
    assert_eq!(s.python_tracked_cells().len(), 2);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.iter().filter(|(_, c)| c.py.is_some()).count(), 2);
    // It has a row in the log, the first, as Initialization.
    let log: Vec<_> = s.python_run_log().cloned().collect();
    assert!(log[0].initialization && !log[1].initialization, "{log:?}");
    let rows = s.execute("python.runLog", json!({})).unwrap();
    assert_eq!((rows[0]["initialization"].clone(), rows[0]["sheet"].clone(), rows[0]["cell"].clone()), (json!(true), Json::Null, Json::Null));
    assert_eq!(rows[1]["initialization"], false);
    assert_eq!(rows[1]["cell"], "A1");
    // An edit further on doesn't run it again: the worker has it.
    py(&mut s, "A3", "ret 6");
    settle(&mut s);
    assert_eq!(runs(&h).iter().filter(|c| *c == "set rate 2").count(), 1);
    // Run All keeps the variables, so it doesn't either; Reset starts a worker, which does.
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).iter().filter(|c| *c == "set rate 2").count(), 1);
    s.execute("python.reset", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).iter().filter(|c| *c == "set rate 2").count(), 2);
    assert_eq!(v(&s, "A1"), NUM(2.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn no_code_runs_nothing_for_it() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(runs(&h), ["ret 1"]);
    // Blank code is no code: nothing is asked of the worker.
    init(&mut s, " \n\t\n");
    settle(&mut s);
    assert_eq!(runs(&h), ["ret 1"]);
    assert!(!s.python_run_log().any(|r| r.initialization));
    assert_eq!(v(&s, "A1"), NUM(1.0));
}

#[test]
fn changing_it_runs_everything_again_from_the_top() {
    let (mut s, h) = session();
    init(&mut s, "set rate 2");
    py(&mut s, "A1", "get rate");
    py(&mut s, "A2", "tick");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (NUM(2.0), NUM(1.0)));
    let before = runs(&h).len();
    let undo = s.doc().unwrap().undo.len();
    // The same code is no change: no step and no run.
    let r = init(&mut s, "set rate 2");
    assert_eq!((r["code"].as_str(), r["userModified"].as_bool()), (Some("set rate 2"), Some(true)));
    settle(&mut s);
    assert_eq!(runs(&h).len(), before);
    assert_eq!(s.doc().unwrap().undo.len(), undo);
    // Line breaks as code has them are the same code.
    init(&mut s, "set rate 3\r\n");
    assert_eq!(code_of(&mut s), "set rate 3\n");
    assert!(s.python_busy(), "cells wait for the new code");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["set rate 3\n", "get rate", "tick"]);
    assert_eq!(h.starts(), 2, "in a fresh worker: nothing of the old code's stays");
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (NUM(3.0), NUM(1.0)));
    // Cleared: everything runs again, with nothing first, and a name it set is gone.
    let r = init(&mut s, "");
    assert_eq!(r["code"], "");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), PYTHON_ERROR);
    assert_eq!(diagnostic(&s, "A1"), Some("python.name"));
    assert_eq!(h.starts(), 3);
    // Changing it while it runs doesn't wait for it, or take its answer.
    init(&mut s, "hang");
    s.python_poll(Duration::ZERO);
    init(&mut s, "set rate 4");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(4.0));
    assert_eq!(s.python_init_failure(), None);
}

#[test]
fn a_failure_shows_in_every_cell_until_it_is_fixed() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    py(&mut s, "A2", "ret 2");
    set(&mut s, "B1", "=A1+1");
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(2.0));
    init(&mut s, "fail");
    settle(&mut s);
    // Every cell, with Python's message and line.
    for cell in ["A1", "A2"] {
        assert_eq!(v(&s, cell), PYTHON_ERROR, "{cell}");
        let d = s.python_diagnostic(0, super::python_run::c(cell)).unwrap();
        assert_eq!((d.kind.id, d.detail.as_str(), d.line), ("init.failed", "ValueError: boom", Some(1)));
    }
    assert_eq!(v(&s, "B1"), PYTHON_ERROR, "what reads them follows");
    assert_eq!(runs(&h).last().map(String::as_str), Some("fail"), "no cell ran");
    // Said once, about the workbook, however many cells show it.
    let rows = init_rows(&mut s);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "init.failed");
    assert_eq!((rows[0]["scope"].as_str(), rows[0]["action"].as_str()), (Some("workbook"), Some("openInitialization")));
    assert_eq!((rows[0]["cell"].clone(), rows[0]["detail"].clone(), rows[0]["line"].clone()), (Json::Null, json!("ValueError: boom"), json!(1)));
    let all = s.execute("python.diagnostics", json!({})).unwrap();
    assert_eq!(all.as_array().unwrap().iter().filter(|r| r["kind"] == "init.failed").count(), 1);
    assert_eq!(s.execute("python.initialization", json!({})).unwrap()["diagnostic"]["kind"], "init.failed");
    // The log has the failure, as Initialization.
    let last = s.python_run_log().last().unwrap();
    assert_eq!((last.initialization, last.diagnostic), (true, Some("init.failed")));
    // Python is still ready: the failure is this workbook's, and Python can run.
    assert_eq!(s.python_environment().state, PythonState::Ready);
    // An edit doesn't try it again, and a new cell shows it too.
    let count = h.requests().len();
    set(&mut s, "C1", "9");
    py(&mut s, "A3", "ret 3");
    assert!(s.python_busy());
    settle(&mut s);
    assert_eq!(h.requests().len(), count, "nothing ran");
    assert_eq!(v(&s, "A3"), PYTHON_ERROR);
    assert_eq!(diagnostic(&s, "A3"), Some("init.failed"));
    assert_eq!(s.doc().unwrap().undo.last().map(|_| ()), Some(()), "edits still make their steps");
    // Fixed, everything runs again, in a worker that has none of what the failed code left.
    let starts = h.starts();
    init(&mut s, "set rate 2");
    settle(&mut s);
    assert_eq!(h.starts(), starts + 1);
    assert_eq!((v(&s, "A1"), v(&s, "A2"), v(&s, "A3"), v(&s, "B1")), (NUM(1.0), NUM(2.0), NUM(3.0), NUM(2.0)));
    assert_eq!(diagnostic(&s, "A1"), None);
    assert!(init_rows(&mut s).is_empty());
}

#[test]
fn asking_cells_to_run_tries_a_failure_again() {
    for ask in ["python.reset", "python.runAll", "python.run"] {
        let (mut s, h) = session();
        py(&mut s, "A1", "ret 1");
        py(&mut s, "A2", "ret 2");
        settle(&mut s);
        init(&mut s, "fail");
        settle(&mut s);
        let fails = |h: &FakeHandle| runs(h).iter().filter(|c| *c == "fail").count();
        assert_eq!(fails(&h), 1);
        let params = if ask == "python.run" { json!({"cell": "A2"}) } else { json!({}) };
        s.execute(ask, params).unwrap();
        settle(&mut s);
        assert_eq!(fails(&h), 2, "{ask} asked for it");
        assert_eq!(diagnostic(&s, "A1"), Some("init.failed"), "{ask}: it failed again");
    }
    // A new list of packages may be what it lacked: Check Again tries it.
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    init(&mut s, "fail");
    settle(&mut s);
    s.execute("python.refreshEnvironment", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).iter().filter(|c| *c == "fail").count(), 2);
}

/// Fails the first time, then answers: code that needed something that came later.
fn fails_once() -> (Session, FakeHandle) {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        let first = w.start_number() == 1;
        while let Some(req) = w.next_request() {
            let Request::Run { id, init, .. } = req else { continue };
            let result = if init.is_some() && first {
                Err(PyError { message: "ModuleNotFoundError: No module named 'x'".into(), line: Some(1), ..PyError::default() })
            } else {
                Ok(Output::Value(NUM(1.0)))
            };
            w.done(id, result);
        }
    });
    let h = t.handle();
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    (s, h)
}

#[test]
fn a_failure_that_passes_clears_when_tried_again() {
    let (mut s, h) = fails_once();
    py(&mut s, "A1", "ret");
    init(&mut s, "import x");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), Some("init.failed"));
    assert_eq!(v(&s, "A1"), PYTHON_ERROR);
    s.execute("python.reset", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), None);
    assert_eq!(v(&s, "A1"), NUM(1.0));
    assert_eq!(h.starts(), 2, "the worker of the run that failed is gone");
}

#[test]
fn undo_and_redo_bring_its_code_back_and_run_from_the_top() {
    let (mut s, h) = session();
    py(&mut s, "A1", "get rate");
    settle(&mut s);
    let start = s.doc().unwrap().undo.len();
    init(&mut s, "set rate 2");
    assert_eq!(s.doc().unwrap().undo.len(), start + 1, "one step");
    settle(&mut s);
    assert_eq!(s.doc().unwrap().undo.len(), start + 1, "running makes none");
    assert_eq!(v(&s, "A1"), NUM(2.0));
    init(&mut s, "set rate 3");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(3.0));
    let top = |h: &FakeHandle| runs(h).iter().rev().find(|c| c.starts_with("set rate")).cloned();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(code_of(&mut s), "set rate 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(2.0), "the cell ran again with the old code");
    assert_eq!(top(&h).as_deref(), Some("set rate 2"));
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(code_of(&mut s), "set rate 3");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(3.0));
    // Back to before there was any: nothing runs first, and a failure goes with its code.
    init(&mut s, "fail");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), Some("init.failed"));
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), None);
    assert_eq!(v(&s, "A1"), NUM(3.0));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(code_of(&mut s), "");
    assert!(s.doc().unwrap().wb.python_env.is_none());
    assert_eq!(diagnostic(&s, "A1"), Some("python.name"));
    assert_eq!(s.doc().unwrap().undo.len(), start);
}

#[test]
#[allow(clippy::disallowed_methods)] // the clock: this test exists only off wasm
fn a_worker_lost_and_rebuilt_runs_it_again() {
    let (mut s, h) = session();
    init(&mut s, "set rate 2");
    py(&mut s, "A1", "get rate");
    py(&mut s, "A2", "die");
    py(&mut s, "A3", "get rate");
    settle(&mut s);
    // A2 took the worker with it; A3's worker runs what came before it, the initialization first.
    assert_eq!(v(&s, "A2"), PYTHON_ERROR);
    assert_eq!(v(&s, "A3"), NUM(2.0));
    let after_die = runs(&h).iter().position(|c| c == "die").unwrap();
    assert_eq!(runs(&h)[after_die + 1], "set rate 2");
    assert_eq!(runs(&h).last().map(String::as_str), Some("get rate"));
    assert_eq!(h.starts(), 2);

    // A worker that goes while nothing runs: the next cell finds a fresh one, with the code run.
    let (mut s, h) = session();
    init(&mut s, "set rate 2");
    py(&mut s, "A1", "setdie a 1");
    settle(&mut s);
    assert_eq!(h.starts(), 1);
    // It has replied and is on its way out: a cell sent in that moment goes down with it, which is
    // the case above. So this waits for it to go, and for the client to see that.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while h.exited() == 0 || s.python_worker_alive() {
        assert!(std::time::Instant::now() < until, "the client never saw the worker go");
        s.python_poll(std::time::Duration::ZERO);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    py(&mut s, "A2", "get rate");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), NUM(2.0));
    assert_eq!(h.starts(), 2);
    assert_eq!(runs(&h).iter().filter(|c| *c == "set rate 2").count(), 2);
}

#[test]
fn it_counts_toward_the_limits_as_a_cell_does() {
    // The time limit: every cell says so, as #PYTHON! rather than as a cell's own #TIMEOUT!.
    let (mut s, h) = with_limits(Limits { timeout: Duration::from_millis(20), start_timeout: Duration::ZERO, ..Limits::default() });
    py(&mut s, "A1", "ret 1");
    init(&mut s, "hang");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), PYTHON_ERROR);
    let d = s.python_diagnostic(0, super::python_run::c("A1")).unwrap().clone();
    assert_eq!(d.kind.id, "init.failed");
    assert!(d.detail.contains("time limit"), "{}", d.detail);
    // Like a cell that stopped the worker, it doesn't start again with the next edit.
    let starts = h.starts();
    set(&mut s, "B9", "1");
    py(&mut s, "A1", "ret 2");
    settle(&mut s);
    assert_eq!(h.starts(), starts);
    assert_eq!(diagnostic(&s, "A1"), Some("init.failed"));

    // Memory.
    let (mut s, h) = with_limits(Limits { memory_bytes: 1024, ..Limits::default() });
    py(&mut s, "A1", "ret 1");
    h.set_rss(1024 * 1024);
    init(&mut s, "hang");
    settle(&mut s);
    assert!(s.python_diagnostic(0, super::python_run::c("A1")).unwrap().detail.contains("memory limit"));

    // It takes the worker down.
    for code in ["die", "garbage", "flood"] {
        let (mut s, _h) = with_limits(Limits { line_bytes: 1024, ..Limits::default() });
        py(&mut s, "A1", "ret 1");
        init(&mut s, code);
        settle(&mut s);
        assert_eq!(diagnostic(&s, "A1"), Some("init.failed"), "{code}");
        assert_eq!(s.python_run_log().last().map(|r| r.initialization), Some(true));
    }

    // A worker that goes after the code each time may never reach a cell: the replay limit ends it,
    // and the code didn't fail. Each loss costs two starts at most: the client's own replacement goes
    // as well, since it hasn't run the code.
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    init(&mut s, "setdieall a 1");
    settle(&mut s);
    assert!(h.starts() <= 12, "it gave up: {}", h.starts());
    assert!(matches!(diagnostic(&s, "A1"), Some("state.stale" | "worker.lost")), "{:?}", diagnostic(&s, "A1"));
    assert_eq!(s.python_init_failure(), None, "the code didn't fail");
}

#[test]
fn stop_stops_it_and_the_cells_wait_for_someone_to_ask() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    init(&mut s, "hang");
    s.python_poll(Duration::ZERO);
    read_requests(&h, 2);
    assert!(s.python_busy());
    assert_eq!(s.python_environment().state, PythonState::Running);
    let running = s.python_running().unwrap();
    assert!(running.initialization, "{running:?}");
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], true);
    assert!(!s.python_busy());
    assert_eq!(s.python_running(), None);
    // The cells keep what they kept, stale; it isn't a failure.
    assert_eq!(v(&s, "A1"), NUM(1.0));
    assert_eq!(diagnostic(&s, "A1"), Some("state.stale"));
    assert_eq!(s.python_init_failure(), None);
    let last = s.python_run_log().last().unwrap();
    assert_eq!((last.initialization, last.diagnostic), (true, Some("state.stopped")));
    // An edit doesn't start code that never ends again; asking does.
    let count = h.requests().len();
    set(&mut s, "B9", "1");
    py(&mut s, "A2", "ret 2");
    settle(&mut s);
    assert_eq!(h.requests().len(), count);
    assert_eq!(diagnostic(&s, "A2"), Some("state.stale"));
    init(&mut s, "set rate 2");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (NUM(1.0), NUM(2.0)));
    assert_eq!(diagnostic(&s, "A1"), None);
    // Stopped again, Run All asks for it.
    init(&mut s, "hang");
    s.python_poll(Duration::ZERO);
    s.execute("python.stop", json!({})).unwrap();
    init(&mut s, "set rate 3");
    s.execute("python.stop", json!({})).unwrap();
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).last().map(String::as_str), Some("ret 2"));
    assert_eq!(diagnostic(&s, "A2"), None);
}

#[test]
fn under_manual_calculation_it_waits_with_the_cells() {
    let (mut s, h) = session();
    s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    assert_eq!(s.python_calculation(), PythonCalculation::Manual);
    py(&mut s, "A1", "get rate");
    init(&mut s, "set rate 2");
    settle(&mut s);
    assert_eq!(h.starts(), 0);
    assert_eq!(diagnostic(&s, "A1"), Some("state.stale"));
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h), ["set rate 2", "get rate"]);
    assert_eq!(v(&s, "A1"), NUM(2.0));
    // A change leaves the cell stale and its old worker gone: the variables are the new code's when
    // someone runs it, and a single cell's run gets the code first too.
    init(&mut s, "set rate 3");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), Some("state.stale"));
    assert_eq!(v(&s, "A1"), NUM(2.0), "it keeps what it showed");
    s.execute("python.run", json!({"cell": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(3.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_workbook_from_a_file_stays_off_until_a_person_turns_it_on() {
    let (mut s, h) = session();
    py(&mut s, "A1", "get rate");
    settle(&mut s);
    assert_eq!(h.starts(), 1);
    // Reading the file's workbook, as `file.open` leaves it: nothing runs, with or without a command.
    let (mut s, h) = {
        let (mut f, h2) = session();
        f.active_mut().unwrap().trust = Trust::FromFile;
        from_file(&mut f, PyEnvironment { code: "set rate 2".into(), ..PyEnvironment::default() });
        py(&mut f, "A1", "get rate");
        (f, h2)
    };
    settle(&mut s);
    s.execute("python.initialization", json!({"code": "set rate 3"})).unwrap();
    settle(&mut s);
    assert_eq!(h.starts(), 0, "no command starts it");
    assert_eq!(code_of(&mut s), "set rate 3");
    assert_eq!(diagnostic(&s, "A1"), Some("off.workbook"));
    assert!(s.execute("python.reset", json!({})).is_err());
    assert_eq!(h.starts(), 0);
    s.set_python_workbook_enabled(true);
    settle(&mut s);
    assert_eq!(runs(&h), ["set rate 3", "get rate"]);
    assert_eq!(v(&s, "A1"), NUM(3.0));
    // Turned off again, what it did goes with the worker, and its failure too.
    s.set_python_workbook_enabled(false);
    s.set_python_workbook_enabled(true);
    settle(&mut s);
    assert_eq!(h.starts(), 2);
    assert_eq!(v(&s, "A1"), NUM(3.0));
}

/// A session whose worker answers initialization code with `rows` of what it skipped, and every cell
/// with 1, and that says whose code it was given.
fn skipping(rows: Vec<Value>) -> (Session, FakeHandle) {
    let t = FakeTransport::new(move |mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let Request::Run { id, init, .. } = req else { continue };
            let value = match init {
                Some(_) => Value::from(Array::new(rows.len() / 2, 2, rows.clone()).unwrap()),
                None => NUM(1.0),
            };
            if !w.done(id, Ok(Output::Value(value))) {
                return;
            }
        }
    });
    let h = t.handle();
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    (s, h)
}

#[test]
fn code_from_a_file_goes_on_without_what_it_cant_import_and_says_so() {
    let (mut s, h) = skipping(vec![Value::text("excel"), NUM(6.0), Value::text("scipy.stats"), NUM(9.0)]);
    from_file(&mut s, PyEnvironment { code: "import excel\nimport scipy.stats".into(), ..PyEnvironment::default() });
    py(&mut s, "A1", "ret");
    settle(&mut s);
    assert!(matches!(h.requests().remove(0), Request::Run { init: Some(Init::File), .. }), "it isn't the person's");
    assert_eq!(v(&s, "A1"), NUM(1.0), "the cells ran");
    assert_eq!(diagnostic(&s, "A1"), None);
    assert_eq!(s.python_init_failure(), None);
    // Reported about the workbook, with the package and the line, and what to install where a package
    // gives it: nothing does for Excel's own module.
    let rows = init_rows(&mut s);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["kind"], "init.skipped");
    assert_eq!((rows[0]["detail"].as_str(), rows[0]["line"].as_u64(), rows[0]["install"].clone()), (Some("excel"), Some(6), Json::Null));
    assert_eq!(
        (rows[1]["detail"].as_str(), rows[1]["line"].as_u64(), rows[1]["install"].as_str()),
        (Some("scipy.stats"), Some(9), Some("scipy.stats"))
    );
    assert_eq!(rows[0]["severity"], "warning");
    // And in the log.
    let run = s.python_run_log().next().unwrap();
    assert_eq!((run.initialization, run.diagnostic), (true, Some("init.skipped")));
    // A person's change makes it theirs: their failures are the code's, and the report starts over.
    init(&mut s, "import excel");
    settle(&mut s);
    assert!(matches!(
        h.requests().iter().rev().find(|r| matches!(r, Request::Run { init: Some(_), .. })),
        Some(Request::Run { init: Some(Init::Person), .. })
    ));
}

#[test]
fn what_a_worker_says_it_skipped_is_kept_within_bounds() {
    let mut rows = Vec::new();
    for i in 0..500 {
        rows.push(Value::text(format!("{}{i}", "m".repeat(300))));
        rows.push(if i % 2 == 0 { NUM(f64::from(i)) } else { Value::text("no line") });
    }
    let (mut s, _h) = skipping(rows);
    from_file(&mut s, PyEnvironment { code: "import m".into(), ..PyEnvironment::default() });
    py(&mut s, "A1", "ret");
    settle(&mut s);
    let rows = init_rows(&mut s);
    assert_eq!(rows.len(), 100);
    assert!(rows.iter().all(|r| r["detail"].as_str().unwrap().chars().count() <= 200));
    // Line 0, half a line and a line past what a file can hold aren't lines.
    assert_eq!(rows[0]["line"], Json::Null);
    assert_eq!(rows[1]["line"], Json::Null);
    assert_eq!(rows[2]["line"], 2);
}

#[test]
fn code_too_long_to_run_says_so_and_can_be_fixed() {
    let (mut s, h) = session();
    let long = "x".repeat(MAX_INIT_CHARS_KEPT);
    from_file(&mut s, PyEnvironment { code: long, ..PyEnvironment::default() });
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(h.starts(), 0, "no worker is asked to run part of a program");
    assert_eq!(v(&s, "A1"), PYTHON_ERROR);
    let d = s.python_diagnostic(0, super::python_run::c("A1")).unwrap();
    assert_eq!(d.kind.id, "init.failed");
    assert!(d.detail.contains("longer than 65536 characters"), "{}", d.detail);
    assert_eq!(s.execute("python.initialization", json!({})).unwrap()["fits"], false);
    assert!(s.python_run_log().last().unwrap().initialization);
    // Shortened, it runs.
    init(&mut s, &"# ".repeat(MAX_INIT_CHARS / 2));
    assert_eq!(s.execute("python.initialization", json!({})).unwrap()["fits"], true);
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(1.0));
}

#[test]
fn the_command_is_safe_from_any_caller() {
    let (mut s, _h) = session();
    // With nothing, it reports.
    let r = s.execute("python.initialization", json!({})).unwrap();
    assert_eq!((r["code"].as_str(), r["userModified"].as_bool(), r["fits"].as_bool()), (Some(""), Some(false), Some(true)));
    assert_eq!(r["diagnostic"], Json::Null);
    assert_eq!(s.execute("python.initialization", json!({"code": null})).unwrap()["code"], "");
    assert!(s.doc().unwrap().wb.python_env.is_none(), "asking makes nothing");
    assert!(!s.doc().unwrap().is_dirty());
    assert!(s.take_ui_requests().is_empty(), "it opens no dialog");
    // What isn't code is refused.
    for bad in [json!({"code": 5}), json!({"code": ["x"]}), json!({"code": true})] {
        let e = s.execute("python.initialization", bad).unwrap_err().to_string();
        assert!(e.contains("must be text"), "{e}");
    }
    // The limit is on the code, and exact.
    let at = "x".repeat(MAX_INIT_CHARS);
    assert_eq!(init(&mut s, &at)["fits"], true);
    let e = s.execute("python.initialization", json!({"code": format!("{at}y")})).unwrap_err().to_string();
    assert!(e.contains("65536"), "{e}");
    assert_eq!(code_of(&mut s), at);
    // Measured as code has its line breaks, so another system's don't count twice.
    let crlf = "x\r\n".repeat(MAX_INIT_CHARS / 2);
    assert_eq!(init(&mut s, &crlf)["fits"], true);
    assert_eq!(code_of(&mut s).chars().count(), MAX_INIT_CHARS);
    // Multi-byte characters count as one.
    let wide = "é".repeat(MAX_INIT_CHARS);
    assert_eq!(init(&mut s, &wide)["fits"], true);
    // A person gave it, and the environment has its GUID, which stays through changes.
    let id = s.doc().unwrap().wb.python_env.as_ref().and_then(|e| e.id.clone()).unwrap();
    assert!(is_guid(&id), "{id}");
    init(&mut s, "x = 1");
    assert_eq!(s.doc().unwrap().wb.python_env.as_ref().and_then(|e| e.id.clone()), Some(id));
    assert_eq!(s.execute("python.initialization", json!({})).unwrap()["userModified"], true);
}

#[test]
fn it_is_refused_on_a_protected_workbook_and_can_still_be_read() {
    let (mut s, _h) = session();
    init(&mut s, "x = 1");
    s.execute("review.protectWorkbook", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.protected_structure);
    let e = s.execute("python.initialization", json!({"code": "x = 2"})).unwrap_err().to_string();
    assert!(e.contains("protected"), "{e}");
    assert_eq!(code_of(&mut s), "x = 1");
    assert_eq!(s.execute("python.initialization", json!({})).unwrap()["code"], "x = 1");
    // Not even a change to nothing, or to the same code, gets through as a change.
    assert!(s.execute("python.initialization", json!({"code": ""})).is_err());
    assert!(s.execute("python.initialization", json!({"code": "x = 1"})).is_err());
    s.execute("review.protectWorkbook", json!({})).unwrap();
    assert!(!s.doc().unwrap().wb.protected_structure);
    assert_eq!(init(&mut s, "x = 2")["code"], "x = 2");
}

#[test]
fn a_worker_that_wont_start_isnt_the_codes_failure() {
    let (cap, _h) = capability(Limits::default());
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability { transport: Some(Arc::new(|| Box::new(super::python_run::NoStart("no such file")))), ..cap });
    py(&mut s, "A1", "ret 1");
    init(&mut s, "set rate 2");
    settle(&mut s);
    assert_eq!(s.python_init_failure(), None);
    assert_eq!(s.python_environment().diagnostic.as_ref().map(|d| d.kind.id), Some("connect.failed"));
    assert_eq!(diagnostic(&s, "A1"), Some("connect.failed"));
    assert!(!s.python_run_log().any(|r| r.initialization), "the tries that didn't start aren't runs");
}

#[test]
fn each_workbook_has_its_own() {
    let (mut s, h) = session();
    init(&mut s, "fail");
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A1"), Some("init.failed"));
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(code_of(&mut s), "");
    assert_eq!(s.python_init_failure(), None);
    assert!(init_rows(&mut s).is_empty());
    py(&mut s, "A1", "ret 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(2.0));
    assert!(h.starts() >= 2);
}

#[test]
fn the_packages_a_persons_code_lacks_are_reported_about_the_workbook() {
    let t = worker();
    let report = crate::python::PythonEnvironment {
        state: PythonState::Ready,
        diagnostic: None,
        packages: vec![("pandas".to_string(), "1.0".to_string())],
        ..crate::python::PythonEnvironment::default()
    };
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        report: Some(report),
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    py(&mut s, "A1", "ret 1");
    init(&mut s, "ret 2\nimport nope\nimport plotly\nimport excel");
    settle(&mut s);
    let rows = init_rows(&mut s);
    assert_eq!(rows.len(), 1, "not what draws in a browser, and not Excel's own module: {rows:?}");
    assert_eq!((rows[0]["kind"].as_str(), rows[0]["detail"].as_str(), rows[0]["cell"].clone()), (Some("packages.missing"), Some("nope"), Json::Null));
    assert_eq!(s.python_stats().code_reads, 2, "the cell's code and the initialization's");
    // What the application that wrote the file put there says what it lacks when it runs: the names
    // the worker gives lazily ask for nothing.
    from_file(&mut s, PyEnvironment { code: "import numpy as np\nimport statsmodels as sm".into(), ..PyEnvironment::default() });
    s.execute("cell.set", json!({"cell": "C9", "input": "1"})).unwrap();
    settle(&mut s);
    assert!(init_rows(&mut s).is_empty());
}

#[test]
fn a_new_workbook_has_none_and_copies_of_it_share_the_code() {
    assert!(Workbook::new().python_env.is_none());
    let (mut s, _h) = session();
    init(&mut s, "x = 1");
    let first = s.doc().unwrap().wb.python_env.clone().unwrap();
    set(&mut s, "A1", "1");
    let after = s.doc().unwrap().wb.python_env.clone().unwrap();
    assert!(Arc::ptr_eq(&first, &after), "an edit doesn't copy it");
}
