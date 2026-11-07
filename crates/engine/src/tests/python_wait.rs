//! A save that waits for a workbook's Python cells, through the fake worker: what the file holds
//! after, how long a wait may last, and what ends one early.
#![allow(clippy::disallowed_methods)] // native tests that time a wait

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gridcraft_core::{CellError, Value};
use gridcraft_python::Limits;
use serde_json::{Value as Json, json};

use super::python_run::{c, capability, py, read_requests, session, set, settle, with_limits};
use super::v;
use crate::Session;
use crate::python::{MAX_WAIT, PythonCapability, WaitBound, WaitState, note_unfinished, reply_may_be_held, wait_asked};

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gridcraft-python-wait-{}-{name}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// A session with no Python that has opened the file: what the file holds.
fn opened(path: &std::path::Path) -> Session {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s
}

fn code_in(s: &mut Session, cell: &str) -> Json {
    s.execute("cell.get", json!({"cell": cell})).unwrap()["python"]["code"].clone()
}

fn doc(s: &Session) -> u64 {
    s.doc().unwrap().uid
}

#[test]
fn a_save_with_wait_holds_until_the_cells_finish_and_the_file_has_their_results() {
    let (mut s, _h) = session();
    let path = temp("holds");
    set(&mut s, "A1", "1");
    py(&mut s, "B1", "ret 7");
    py(&mut s, "B2", "ret 8");
    assert!(s.python_workbook_busy(doc(&s)));
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert_eq!(r["unfinished"], json!([]), "{r}");
    assert_eq!(r["unfinishedCount"], 0, "{r}");
    assert!(!s.python_workbook_busy(doc(&s)));
    assert!(!s.doc().unwrap().is_dirty());
    let o = opened(&path);
    assert_eq!(v(&o, "B1"), Value::Number(7.0));
    assert_eq!(v(&o, "B2"), Value::Number(8.0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_as_takes_wait_too() {
    let (mut s, _h) = session();
    let path = temp("save-as");
    py(&mut s, "B1", "ret 7");
    s.execute("file.saveAs", json!({"path": path, "waitSeconds": 30})).unwrap();
    assert_eq!(v(&opened(&path), "B1"), Value::Number(7.0));
    let _ = std::fs::remove_file(&path);
}

/// Without `wait` it saves at once, as it always did: the last result and the code, and the reply
/// says the cell wasn't finished.
#[test]
fn a_save_without_wait_stores_the_last_result_and_says_the_cell_was_not_finished() {
    let (mut s, _h) = session();
    let path = temp("at-once");
    py(&mut s, "B1", "ret 7");
    settle(&mut s);
    // A cell before it holds it up, so it is due with its last result still there.
    py(&mut s, "A1", "hang");
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Busy));
    let r = s.execute("file.save", json!({"path": path})).unwrap();
    assert_eq!(r["unfinished"], json!(["Sheet1!A1", "Sheet1!B1"]), "{r}");
    let mut o = opened(&path);
    assert_eq!(code_in(&mut o, "A1"), json!("hang"), "the code is never lost");
    assert_eq!(code_in(&mut o, "B1"), json!("ret 7"));
    assert_eq!(v(&o, "B1"), Value::Number(7.0), "the last result, not #BUSY!");
    let _ = std::fs::remove_file(&path);
}

/// A cell that never finishes: the save comes at the bound, with what is there.
#[test]
fn a_cell_that_never_finishes_ends_at_the_bound_and_the_reply_names_it() {
    let (mut s, _h) = session();
    let path = temp("bound");
    py(&mut s, "B1", "ret 7");
    settle(&mut s);
    py(&mut s, "A1", "hang");
    let started = Instant::now();
    let r = s.execute("file.save", json!({"path": path, "waitSeconds": 0.2})).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(200), "it waited");
    assert!(started.elapsed() < Duration::from_secs(10), "and not longer");
    assert_eq!(r["unfinished"], json!(["Sheet1!A1", "Sheet1!B1"]), "{r}");
    assert_eq!(r["unfinishedCount"], 2, "{r}");
    let mut o = opened(&path);
    assert_eq!(v(&o, "B1"), Value::Number(7.0));
    assert_eq!(code_in(&mut o, "A1"), json!("hang"));
    assert_eq!(code_in(&mut o, "B1"), json!("ret 7"));
    let _ = std::fs::remove_file(&path);
}

/// A cell that never had a result stores none: not `#BUSY!`, which is the state of this session.
#[test]
fn a_cell_that_never_had_a_result_stores_none_and_keeps_its_code() {
    let (mut s, _h) = session();
    let path = temp("never");
    py(&mut s, "B1", "hang");
    s.execute("file.save", json!({"path": path, "waitSeconds": 0})).unwrap();
    let mut o = opened(&path);
    assert_eq!(code_in(&mut o, "B1"), json!("hang"));
    let cell = o.doc().unwrap().wb.active().unwrap().cell(c("B1")).cloned().unwrap();
    let py = cell.py.as_deref().unwrap();
    assert_eq!(py.output, None, "no result was stored");
    assert!(!py.busy, "and no state of the session that saved it");
    // The format has no code for #BUSY!: what a cell without a result caches, as Excel caches #SPILL!.
    assert_eq!(cell.value, Value::Error(CellError::Value));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_cell_stale_under_manual_calculation_does_not_make_a_save_wait() {
    let (mut s, h) = session();
    let path = temp("manual");
    py(&mut s, "B1", "ret 1");
    settle(&mut s);
    s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    py(&mut s, "B1", "hang");
    assert!(!s.python_workbook_busy(doc(&s)), "stale is not busy");
    assert!(s.python_wait_begin(doc(&s), WaitBound::Cells, false).is_none());
    let started = Instant::now();
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r["unfinished"], json!([]), "{r}");
    assert_eq!(super::python_run::runs(&h), ["ret 1"], "nothing ran for it");
    // A cell a person asked to run under manual calculation is running, and a save waits for it.
    s.execute("python.set", json!({"cell": "B2", "code": "ret 2"})).unwrap();
    s.execute("python.run", json!({"cell": "B2"})).unwrap();
    assert!(s.python_workbook_busy(doc(&s)));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn another_workbooks_cells_do_not_hold_a_save_up() {
    let (mut s, _h) = session();
    let path = temp("other");
    py(&mut s, "A1", "hang");
    let first = doc(&s);
    s.execute("file.new", json!({})).unwrap();
    py(&mut s, "B1", "ret 7");
    let started = Instant::now();
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "the other workbook's cell hangs");
    assert_eq!(r["unfinished"], json!([]), "{r}");
    assert!(s.python_workbook_busy(first));
    assert_eq!(v(&opened(&path), "B1"), Value::Number(7.0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn stop_during_a_wait_ends_it_and_the_cells_it_left_are_named() {
    let (mut s, h) = session();
    py(&mut s, "B1", "hang");
    py(&mut s, "B2", "ret 2");
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap();
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::Waiting);
    s.python_poll(Duration::ZERO);
    read_requests(&h, 1);
    s.execute("python.stop", json!({})).unwrap();
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::Idle);
    assert_eq!(s.python_wait_unfinished(&wait), ["Sheet1!B1", "Sheet1!B2"]);
}

/// A cell that kills its worker gets an error, which is a result, and the wait goes on to the next.
#[test]
fn the_worker_dying_during_a_wait_does_not_end_it_early() {
    let (mut s, _h) = session();
    let path = temp("dies");
    py(&mut s, "B1", "die");
    py(&mut s, "B2", "ret 2");
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert_eq!(r["unfinished"], json!([]), "{r}");
    let o = opened(&path);
    assert_eq!(v(&o, "B2"), Value::Number(2.0));
    let _ = std::fs::remove_file(&path);
}

/// Workers that keep going away end the go with the cells stale: the wait ends, and says so.
#[test]
fn workers_that_keep_dying_leave_the_cells_named() {
    let (mut s, _h) = session();
    let path = temp("keeps-dying");
    py(&mut s, "B1", "setdieall a 1");
    py(&mut s, "B2", "setdieall b 2");
    py(&mut s, "B3", "setdieall c 3");
    py(&mut s, "B4", "ret 4");
    let started = Instant::now();
    let r = s.execute("file.save", json!({"path": path, "waitSeconds": 30})).unwrap();
    assert!(started.elapsed() < Duration::from_secs(29), "the go ended by itself");
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
    // How far it got depends on timing: a cell sent in the moment between a worker's reply and its
    // exit goes down with the worker and shows the error, and otherwise the go gives up with cells
    // waiting. Either way the reply names every cell left waiting.
    let named: Vec<&str> = r["unfinished"].as_array().unwrap().iter().filter_map(Json::as_str).collect();
    assert_eq!(r["unfinishedCount"], named.len(), "{r}");
    let d = s.doc().unwrap();
    for cell in ["B1", "B2", "B3", "B4"] {
        let busy = d.wb.sheet(0).unwrap().cell(c(cell)).and_then(|x| x.py.as_deref()).is_some_and(|py| py.busy);
        assert!(!busy || named.contains(&format!("Sheet1!{cell}").as_str()), "{cell}: {r}");
    }
}

#[test]
fn python_turned_off_during_a_wait_ends_it_and_the_cells_are_named() {
    let (mut s, _h) = session();
    py(&mut s, "B1", "hang");
    py(&mut s, "B2", "ret 2");
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap();
    let (mut off, _) = capability(Limits::default());
    off.enabled = false;
    s.set_python_capability(off);
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::Idle);
    assert_eq!(s.python_wait_unfinished(&wait), ["Sheet1!B1", "Sheet1!B2"]);

    // For this workbook alone.
    let (cap, _) = capability(Limits::default());
    s.set_python_capability(cap);
    s.execute("python.runAll", json!({})).unwrap();
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap();
    s.set_python_workbook_enabled(false);
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::Idle);
    assert_eq!(s.python_wait_unfinished(&wait), ["Sheet1!B1", "Sheet1!B2"]);
}

/// A save that fails after its wait is an error, with the workbook as it was: the work is kept, and
/// so is what it was saved to before.
#[test]
fn a_failed_save_after_a_wait_keeps_the_work() {
    let (mut s, _h) = session();
    let blocker = temp("blocker");
    std::fs::write(&blocker, "a file").unwrap();
    py(&mut s, "B1", "ret 7");
    let e = s.execute("file.save", json!({"path": blocker.join("book.xlsx"), "wait": true})).unwrap_err().to_string();
    assert!(!e.is_empty());
    let d = s.doc().unwrap();
    assert!(d.is_dirty(), "the work isn't marked saved");
    assert_eq!(d.path, None);
    assert_eq!(v(&s, "B1"), Value::Number(7.0), "the wait did what it could");
    let _ = std::fs::remove_file(&blocker);
}

/// A cell the person deleted during the wait isn't a cell that wasn't finished.
#[test]
fn a_cell_deleted_during_a_wait_is_not_named() {
    let (mut s, _h) = session();
    py(&mut s, "B1", "hang");
    py(&mut s, "B2", "ret 2");
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::At(Duration::ZERO), false).unwrap();
    set(&mut s, "B2", "");
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::AtCap);
    assert_eq!(s.python_wait_unfinished(&wait), ["Sheet1!B1"]);
}

/// A sheet name that needs quotes gets them.
#[test]
fn cells_are_named_in_run_order_with_their_sheets() {
    let (mut s, _h) = session();
    s.execute("home.insertSheet", json!({})).unwrap();
    let second = s.doc().unwrap().wb.sheet(1).unwrap().name.clone();
    s.execute("sheet.rename", json!({"sheet": second, "name": "My Data"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    py(&mut s, "B2", "hang");
    s.execute("python.set", json!({"cell": "'My Data'!A1", "code": "ret 1"})).unwrap();
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::At(Duration::ZERO), false).unwrap();
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::AtCap);
    let names = s.python_wait_unfinished(&wait);
    assert_eq!(names, ["Sheet1!B2", "'My Data'!A1"]);
    assert_eq!(names, s.python_unfinished(doc(&s)));
}

fn cap_of(limits: Limits, cells: usize) -> Duration {
    let (mut s, _h) = with_limits(limits);
    for row in 1..=cells {
        py(&mut s, &format!("B{row}"), "hang");
    }
    s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap().cap()
}

/// The cap: the time Python may need to start, the time limit for each cell left and a second, and
/// never past two minutes, whatever the limits and however many cells.
#[test]
fn the_cap_follows_the_time_limit_and_the_cells_left_up_to_two_minutes() {
    assert_eq!(MAX_WAIT, Duration::from_secs(120));
    let default = Limits::default();
    assert_eq!(cap_of(default.clone(), 1), Duration::from_secs(10 + 30 + 1));
    assert_eq!(cap_of(default.clone(), 3), Duration::from_secs(10 + 30 * 3 + 1));
    assert_eq!(cap_of(default.clone(), 4), MAX_WAIT);
    assert_eq!(cap_of(default.clone(), 300), MAX_WAIT);
    let short = Limits { timeout: Duration::from_secs(1), start_timeout: Duration::ZERO, ..default.clone() };
    assert_eq!(cap_of(short.clone(), 2), Duration::from_secs(1 + 2 + 1 - 1));
    // The most a person can choose can't overflow it, or pass it.
    let long = Limits { timeout: crate::python::MAX_TIMEOUT, start_timeout: crate::python::MAX_TIMEOUT, ..default };
    assert_eq!(cap_of(long, 50), MAX_WAIT);
    // The initialization code runs first, and is timed like a cell.
    let (mut s, _h) = with_limits(Limits { timeout: Duration::from_secs(5), start_timeout: Duration::ZERO, ..Limits::default() });
    s.execute("python.initialization", json!({"code": "hang"})).unwrap();
    py(&mut s, "B1", "ret 1");
    assert_eq!(s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap().cap(), Duration::from_secs(5 * 2 + 1));
}

/// More cells during the wait get their time, up to the hard cap; a chosen bound doesn't move.
#[test]
fn more_cells_during_a_wait_add_to_the_cap_but_not_past_the_most() {
    let (mut s, _h) = with_limits(Limits { timeout: Duration::from_secs(10), start_timeout: Duration::ZERO, ..Limits::default() });
    py(&mut s, "B1", "hang");
    let mut wait = s.python_wait_begin(doc(&s), WaitBound::Cells, false).unwrap();
    let mut chosen = s.python_wait_begin(doc(&s), WaitBound::At(Duration::from_secs(3)), false).unwrap();
    assert_eq!(wait.cap(), Duration::from_secs(11));
    py(&mut s, "B2", "ret 2");
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::Waiting);
    assert_eq!(wait.cap(), Duration::from_secs(21));
    for row in 3..40 {
        py(&mut s, &format!("B{row}"), "ret 2");
    }
    s.python_wait_poll(&mut wait);
    assert_eq!(wait.cap(), MAX_WAIT);
    s.python_wait_poll(&mut chosen);
    assert_eq!(chosen.cap(), Duration::from_secs(3));
    // A second caller's shorter bound holds; a longer one moves nothing.
    wait.limit_to(Duration::from_secs(600));
    assert_eq!(wait.cap(), MAX_WAIT);
    wait.limit_to(Duration::ZERO);
    assert!(wait.cap() < Duration::from_secs(5));
    assert_eq!(s.python_wait_poll(&mut wait), WaitState::AtCap);
}

#[test]
fn what_wait_and_wait_seconds_ask_for() {
    let ok = |p: Json| wait_asked(&p).unwrap();
    assert_eq!(ok(json!({})), None);
    assert_eq!(ok(json!({"wait": null, "waitSeconds": null})), None);
    assert_eq!(ok(json!({"wait": false})), None);
    assert_eq!(ok(json!({"wait": false, "waitSeconds": 5})), None, "false decides");
    assert_eq!(ok(json!({"wait": true})), Some(WaitBound::Cells));
    assert_eq!(ok(json!({"wait": true, "waitSeconds": 5})), Some(WaitBound::At(Duration::from_secs(5))));
    assert_eq!(ok(json!({"waitSeconds": 0.5})), Some(WaitBound::At(Duration::from_millis(500))));
    assert_eq!(ok(json!({"waitSeconds": 0})), Some(WaitBound::At(Duration::ZERO)));
    // Numbers past the most, up to what JSON holds, are the most.
    for huge in [json!(121), json!(1e308), json!(u64::MAX)] {
        assert_eq!(ok(json!({"waitSeconds": huge})), Some(WaitBound::At(MAX_WAIT)));
    }
    for bad in [json!({"wait": 1}), json!({"wait": "true"}), json!({"waitSeconds": -1}), json!({"waitSeconds": "5"}), json!({"waitSeconds": true})] {
        assert!(wait_asked(&bad).is_err(), "{bad}");
    }
    // A bad value refuses the save, and writes nothing.
    let (mut s, _h) = session();
    let path = temp("refused");
    assert!(s.execute("file.save", json!({"path": path, "waitSeconds": -1})).is_err());
    assert!(!path.exists());

    assert!(reply_may_be_held(&json!({"wait": true})));
    assert!(reply_may_be_held(&json!({"waitSeconds": 1})));
    assert!(reply_may_be_held(&json!({"command": "file.save", "params": {"path": "x", "wait": true}})));
    assert!(!reply_may_be_held(&json!({"command": "file.save", "params": {"path": "x"}})));
    assert!(!reply_may_be_held(&json!({"wait": false})));
}

#[test]
fn a_reply_gives_a_bounded_list_and_the_count() {
    let names: Vec<String> = (0..1500).map(|i| format!("Sheet1!A{i}")).collect();
    let mut reply = json!({"path": "x"});
    note_unfinished(&mut reply, true, &names);
    assert_eq!(reply["unfinished"].as_array().unwrap().len(), 1000);
    assert_eq!(reply["unfinishedCount"], 1500);
    let mut not_an_object = json!(5);
    note_unfinished(&mut not_an_object, true, &[]);
    assert_eq!(not_an_object, json!({"result": 5, "unfinished": [], "unfinishedCount": 0}));
    let mut untouched = json!({"path": "x"});
    note_unfinished(&mut untouched, false, &[]);
    assert_eq!(untouched, json!({"path": "x"}));
}

/// A command inside another (a script's step) doesn't hold the whole frame up.
#[test]
fn a_save_inside_another_command_does_not_block() {
    let (mut s, _h) = session();
    let path = temp("nested");
    py(&mut s, "B1", "hang");
    s.depth = 1;
    let started = Instant::now();
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    s.depth = 0;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r["unfinished"], json!(["Sheet1!B1"]), "{r}");
    let _ = std::fs::remove_file(&path);
}

/// The CLI and MCP never run Python: nothing waits, and a reply is what it always was.
#[test]
fn a_session_without_python_saves_at_once() {
    let path = temp("headless");
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("python.set", json!({"cell": "B1", "code": "1 + 1"})).unwrap();
    assert!(!s.python_workbook_busy(doc(&s)));
    assert!(s.python_wait_begin(doc(&s), WaitBound::Cells, true).is_none());
    assert!(s.python_unfinished(doc(&s)).is_empty());
    let r = s.execute_headless("file.save", json!({"path": path})).unwrap();
    assert_eq!(r.as_object().unwrap().keys().collect::<Vec<_>>(), ["bytes", "path"], "{r}");
    let started = Instant::now();
    let r = s.execute_headless("file.save", json!({"path": path, "wait": true})).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r["unfinished"], json!([]), "{r}");
    // A workbook with no file: an error, with or without `wait`, never a dialog.
    s.execute("file.new", json!({})).unwrap();
    for p in [json!({}), json!({"wait": true})] {
        let e = s.execute_headless("file.save", p).unwrap_err().to_string();
        assert!(e.contains("programmatic calls never open dialogs"), "{e}");
    }
    assert!(s.take_ui_requests().is_empty());
    let _ = std::fs::remove_file(&path);
}

/// A session with a capability that was turned off has nothing to wait for either.
#[test]
fn python_off_saves_at_once() {
    let path = temp("off");
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability { enabled: false, ..PythonCapability::default() });
    s.execute("python.set", json!({"cell": "B1", "code": "1 + 1"})).unwrap();
    assert!(!s.python_workbook_busy(doc(&s)));
    let r = s.execute("file.save", json!({"path": path, "wait": true})).unwrap();
    assert_eq!(r["unfinished"], json!([]), "{r}");
    let _ = std::fs::remove_file(&path);
}
