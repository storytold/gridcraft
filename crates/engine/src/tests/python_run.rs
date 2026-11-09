//! Running Python cells, through the fake worker: what runs, when, and what the cells show after.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use gridcraft_core::{Array, CellError, CellRef, Value};
use gridcraft_python::fake::{FakeHandle, FakeTransport, FakeWorker};
use gridcraft_python::protocol::{MAX_ARRAY_CELLS, Reply, Request};
use gridcraft_python::run::{Pipes, Transport};
use gridcraft_python::{Input, Limits, Output, PyError};
use serde_json::json;

use super::v;
use crate::python::{PythonCalculation, PythonCapability, PythonState, python_order};
use crate::{DocState, Session, Trust};

pub(super) fn c(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

fn number(v: &Input) -> f64 {
    match &v.value {
        Value::Number(n) => *n,
        Value::Array(a) => a.data.iter().map(|x| if let Value::Number(n) = x { *n } else { 0.0 }).sum(),
        _ => 0.0,
    }
}

/// A little language for the fake worker's cells: the first word says what the cell does.
///
/// `ret 5`, `set a 5` (a variable, returning nothing), `setdie a 5`, `setdieall a 5`, `get a` (fails as an undefined name),
/// `sum xl("A1") xl("B1:B3")`, `say xl("A1")` (the input as it came), `shape xl("A:A")` (its rows),
/// `spill 3 1`, `obj`, `fail`, `print 100` (bytes), `failprint`, `num`, `empty`, `none`, `hang`,
/// `die`, `flood`, `garbage`, `tick` (a number that grows with each run).
fn answer(w: &mut FakeWorker, id: u64, code: &str, inputs: &BTreeMap<String, Input>, vars: &mut HashMap<String, f64>) -> bool {
    let words: Vec<&str> = code.split_whitespace().collect();
    let value = |v: Value| Ok(Output::Value(v));
    let result = match words.first().copied().unwrap_or("") {
        "ret" => value(Value::Number(words.get(1).and_then(|n| n.parse().ok()).unwrap_or(0.0))),
        "set" => {
            vars.insert(words[1].into(), words[2].parse().unwrap());
            value(Value::Empty)
        }
        // Answers, then the worker goes away: the first one to run it, or every one.
        "setdie" | "setdieall" => {
            vars.insert(words[1].into(), words[2].parse().unwrap());
            let dies = words[0] == "setdieall" || w.start_number() == 1;
            return w.done(id, value(Value::Empty)) && !dies;
        }
        "get" => match vars.get(words[1]) {
            Some(n) => value(Value::Number(*n)),
            None => Err(PyError {
                message: format!("NameError: name '{}' is not defined", words[1]),
                line: Some(1),
                kind: Some("python.name".into()),
                subject: Some(words[1].into()),
            }),
        },
        "sum" => value(Value::Number(inputs.values().map(number).sum())),
        "say" => value(inputs.values().next().map_or(Value::Empty, |i| i.value.clone())),
        "shape" => value(Value::Number(match inputs.values().next().map(|i| &i.value) {
            Some(Value::Array(a)) => a.rows as f64,
            _ => -1.0,
        })),
        "spill" => {
            let (r, c): (usize, usize) = (words[1].parse().unwrap(), words[2].parse().unwrap());
            let data = (0..r * c).map(|i| Value::Number(10.0 * (i + 1) as f64)).collect();
            value(Value::from(Array::new(r, c, data).unwrap()))
        }
        "obj" => Ok(Output::object("dict", "{'a': 1}")),
        "picture" => {
            let size = |i: usize| words.get(i).and_then(|n| n.parse().ok()).unwrap_or(1);
            let density = size(3);
            Ok(Output::picture(gridcraft_python::Picture::from_bytes(picture_bytes(size(1), size(2)), density).unwrap()))
        }
        // A picture while its input is 1, else a number.
        "picturewhen" => {
            if inputs.values().map(number).sum::<f64>() == 1.0 {
                Ok(Output::picture(gridcraft_python::Picture::from_bytes(picture_bytes(4, 3), 1).unwrap()))
            } else {
                value(Value::Number(2.0))
            }
        }
        // A picture the first time, then a number.
        "pictureonce" => {
            let n = vars.entry("pictureonce".into()).or_insert(0.0);
            *n += 1.0;
            if *n == 1.0 {
                Ok(Output::picture(gridcraft_python::Picture::from_bytes(picture_bytes(4, 3), 1).unwrap()))
            } else {
                value(Value::Number(1.0))
            }
        }
        // A figure whose size is up to GridCraft: width, height and the smallest it can be drawn at.
        "figure" => {
            let n = |i: usize| words[i].parse::<u32>().unwrap();
            let mut picture = gridcraft_python::Picture::from_bytes(picture_bytes(n(1), n(2)), 1).unwrap();
            picture.floor = Some([n(1) / 2, n(2) / 2]);
            Ok(Output::picture(picture))
        }
        // It changed a figure an earlier cell made: sheet, row and column.
        "touch" => {
            let n = |i: usize| words[i].parse().unwrap();
            return w.send(&Reply::Done {
                id,
                result: value(Value::Number(1.0)),
                stdout: String::new(),
                peak: None,
                touched: Some([n(1), n(2), n(3)]),
            });
        }
        "fail" => Err(PyError { message: "ValueError: boom".into(), line: Some(1), kind: None, subject: None }),
        "num" => value(Value::Error(CellError::Num)),
        "empty" => value(Value::Error(CellError::Calc)),
        "none" => value(Value::Empty),
        "print" => {
            let n: usize = words[1].parse().unwrap();
            return w.send(&Reply::Done { id, result: value(Value::Number(1.0)), stdout: "x".repeat(n), peak: None, touched: None });
        }
        "failprint" => {
            let error = PyError { message: "ValueError: boom".into(), ..PyError::default() };
            return w.send(&Reply::Done { id, result: Err(error), stdout: "before the error\n".into(), peak: None, touched: None });
        }
        "tick" => {
            let n = vars.entry("tick".into()).or_insert(0.0);
            *n += 1.0;
            value(Value::Number(*n))
        }
        "hang" => {
            w.stall();
            return false;
        }
        "die" => return false,
        "flood" => {
            w.flood(8 * 1024 * 1024, b'x');
            w.stall();
            return false;
        }
        "garbage" => return w.write_raw(b"{not json\n"),
        other => value(Value::text(other)),
    };
    w.done(id, result)
}

/// A real PNG of `width` x `height`, one colour.
pub(super) fn picture_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

pub(super) fn worker() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        let mut vars = HashMap::new();
        while let Some(req) = w.next_request() {
            let go = match &req {
                Request::Run { id, code, inputs, .. } => answer(&mut w, *id, code, inputs, &mut vars),
                // The figure at the size asked for, unless the code says it can't be drawn.
                Request::Render { id, width, height, .. } => {
                    if vars.contains_key("die_on_render") {
                        return;
                    }
                    if vars.contains_key("no_render") {
                        w.done(*id, Err(PyError { message: "ValueError: no".into(), ..PyError::default() }))
                    } else {
                        w.done(*id, Ok(Output::picture(gridcraft_python::Picture::from_bytes(picture_bytes(*width, *height), 1).unwrap())))
                    }
                }
                Request::Reset { id } => w.done(*id, Ok(Output::Value(Value::Empty))),
            };
            if !go {
                return;
            }
        }
    })
}

pub(super) fn capability(limits: Limits) -> (PythonCapability, FakeHandle) {
    let t = worker();
    let h = t.handle();
    (PythonCapability { enabled: true, limits, transport: Some(Arc::new(move || Box::new(t.sharing()))), ..PythonCapability::default() }, h)
}

pub(super) fn with_limits(limits: Limits) -> (Session, FakeHandle) {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    let (cap, h) = capability(limits);
    s.set_python_capability(cap);
    (s, h)
}

pub(super) fn session() -> (Session, FakeHandle) {
    with_limits(Limits::default())
}

/// Waits until the workers have read this many requests (a worker reads on its own thread).
pub(super) fn read_requests(h: &FakeHandle, n: usize) {
    let mut left = 2_000;
    while runs(h).len() < n {
        std::thread::sleep(Duration::from_millis(1));
        left -= 1;
        assert!(left > 0, "the worker never read the request");
    }
}

/// Polls until Python has nothing left to do.
pub(super) fn settle(s: &mut Session) {
    let mut left = 4_000;
    while s.python_busy() {
        s.python_poll(Duration::from_millis(5));
        left -= 1;
        assert!(left > 0, "Python never settled");
    }
}

pub(super) fn py(s: &mut Session, cell: &str, code: &str) {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap();
}

pub(super) fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

/// The code the workers have read so far.
pub(super) fn runs(h: &FakeHandle) -> Vec<String> {
    h.requests()
        .into_iter()
        .filter_map(|r| match r {
            Request::Run { code, .. } => Some(code),
            Request::Render { .. } | Request::Reset { .. } => None,
        })
        .collect()
}

pub(super) fn diagnostic(s: &Session, cell: &str) -> Option<&'static str> {
    s.python_diagnostic(0, c(cell)).map(|d| d.kind.id)
}

const NUM: fn(f64) -> Value = Value::Number;

#[test]
fn cells_run_in_order_row_by_row_and_sheet_by_sheet() {
    let (mut s, h) = session();
    s.execute("home.insertSheet", json!({})).unwrap();
    let second = s.doc().unwrap().wb.sheet(1).unwrap().name.clone();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    py(&mut s, "B2", "ret 3");
    py(&mut s, "A2", "ret 2");
    py(&mut s, "C1", "ret 1");
    s.execute("python.set", json!({"cell": format!("{second}!A1"), "code": "ret 4"})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).iter().map(String::as_str).collect::<Vec<_>>().last(), Some(&"ret 4"));
    let wb = s.doc().unwrap().wb.clone();
    assert_eq!(python_order(&wb), [(0, c("C1")), (0, c("A2")), (0, c("B2")), (1, c("A1"))]);
    assert_eq!(s.python_tracked_cells(), python_order(&wb));
    // Asked for all at once, they go in that order, whatever order they were made in.
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).split_off(runs(&h).len() - 4), ["ret 1", "ret 2", "ret 3", "ret 4"]);
    assert_eq!(v(&s, "B2"), NUM(3.0));
    // Moving a sheet changes the order, and the cells from there on run again.
    s.execute("sheet.move", json!({"sheet": 1, "to": 0})).unwrap();
    assert_eq!(python_order(&s.doc().unwrap().wb)[0], (0, c("A1")));
    settle(&mut s);
    assert_eq!(s.python_tracked_cells(), python_order(&s.doc().unwrap().wb));
    assert_eq!(runs(&h).split_off(runs(&h).len() - 4), ["ret 4", "ret 1", "ret 2", "ret 3"]);
}

#[test]
fn a_new_cell_is_busy_until_its_result_arrives_with_no_undo_step() {
    let (mut s, _h) = session();
    let r = s.execute("python.set", json!({"cell": "A1", "code": "ret 7"})).unwrap();
    assert_eq!(r["value"]["error"], "#BUSY!", "the command already says so");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
    assert_eq!(diagnostic(&s, "A1"), Some("state.busy"));
    assert!(s.python_busy());
    let undo = s.doc().unwrap().undo.len();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(7.0));
    assert_eq!(diagnostic(&s, "A1"), None);
    assert_eq!(s.doc().unwrap().undo.len(), undo, "a result makes no undo step");
    let got = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!(got["python"]["typeName"], "float");
    assert_eq!(got["python"]["busy"], false);
    // What reads the cell follows it.
    set(&mut s, "B1", "=A1*2");
    assert_eq!(v(&s, "B1"), NUM(14.0));
    py(&mut s, "A1", "ret 8");
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Busy), "a reader sees the wait");
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(16.0));
}

#[test]
fn a_cell_that_reruns_takes_the_cells_after_it_with_it_and_only_those() {
    let (mut s, h) = session();
    py(&mut s, "A1", "set a 1");
    py(&mut s, "A2", "get a");
    py(&mut s, "A3", "ret 3");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), NUM(1.0));
    let before = runs(&h).len();
    py(&mut s, "A2", "ret 22");
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["ret 22", "ret 3"], "not A1, and A3 too");
    let before = runs(&h).len();
    py(&mut s, "A1", "set a 2");
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["set a 2", "ret 22", "ret 3"]);
    // The worker went on all along: the variables are shared by one.
    assert_eq!(h.starts(), 1);
}

#[test]
fn only_a_change_to_what_a_cell_reads_makes_it_run() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    set(&mut s, "A2", "2");
    py(&mut s, "C1", "sum xl(\"A1\") xl(\"A2\")");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(3.0));
    let before = runs(&h).len();
    set(&mut s, "A5", "99");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before, "a cell it doesn't read");
    s.execute("home.bold", json!({"range": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).len(), before, "a format");
    set(&mut s, "A2", "2");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before, "the same value");
    set(&mut s, "A2", "5");
    assert_eq!(v(&s, "C1"), Value::Error(CellError::Busy));
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(6.0));
    // A formula's value is an input too, and so is what spills.
    set(&mut s, "D1", "=A1*10");
    py(&mut s, "C2", "say xl(\"D1\")");
    py(&mut s, "F1", "spill 3 1");
    py(&mut s, "C3", "sum xl(\"F1:F3\")");
    settle(&mut s);
    assert_eq!((v(&s, "C2"), v(&s, "C3")), (NUM(10.0), NUM(60.0)));
    set(&mut s, "A1", "4");
    settle(&mut s);
    assert_eq!(v(&s, "C2"), NUM(40.0));
    // A return type is part of it.
    let before = runs(&h).len();
    s.execute("python.output", json!({"cell": "C2", "mode": "object"})).unwrap();
    settle(&mut s);
    assert!(runs(&h).len() > before);
}

#[test]
fn moving_cells_around_runs_nothing() {
    let (mut s, h) = session();
    set(&mut s, "A5", "2");
    py(&mut s, "C5", "sum xl(\"A5\")");
    py(&mut s, "C7", "ret 7");
    settle(&mut s);
    let before = runs(&h).len();
    // The cell moves, its reference is rewritten, and it reads the same.
    s.execute("home.insertRows", json!({"rows": "2:2"})).unwrap();
    assert_eq!(v(&s, "C6"), NUM(2.0), "it keeps its result");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before);
    assert_eq!(s.python_tracked_cells(), [(0, c("C6")), (0, c("C8"))]);
    // Its diagnostics move with it: what it says doesn't stay at the old address.
    set(&mut s, "A6", "3");
    settle(&mut s);
    assert_eq!(v(&s, "C6"), NUM(3.0));
    assert_eq!(runs(&h).len(), before + 2, "it reads the cell under the new address");
}

#[test]
fn removing_a_cell_runs_the_ones_after_it() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    py(&mut s, "A2", "ret 2");
    py(&mut s, "A3", "ret 3");
    settle(&mut s);
    let before = runs(&h).len();
    s.execute("edit.clearContents", json!({"range": "A2"})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["ret 3"]);
    assert_eq!(s.python_tracked_cells(), [(0, c("A1")), (0, c("A3"))]);
}

#[test]
fn under_manual_calculation_due_cells_are_stale_until_asked() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    py(&mut s, "C1", "say xl(\"A1\")");
    py(&mut s, "C2", "ret 2");
    settle(&mut s);
    let r = s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    assert_eq!(r["mode"], "manual");
    assert_eq!(s.python_calculation(), PythonCalculation::Manual);
    let before = runs(&h).len();
    set(&mut s, "A1", "5");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before, "nothing runs");
    assert_eq!(v(&s, "C1"), NUM(1.0), "and it keeps its value");
    assert_eq!(diagnostic(&s, "C1"), Some("state.stale"));
    assert_eq!(s.execute("cell.get", json!({"cell": "C1"})).unwrap()["python"]["stale"], true);
    assert_eq!(diagnostic(&s, "C2"), Some("state.stale"), "it comes after a due cell");
    // New code has no value to keep.
    py(&mut s, "D5", "ret 5");
    assert_eq!(v(&s, "D5"), Value::Error(CellError::Blocked));
    // Putting the input back makes it up to date again.
    set(&mut s, "A1", "1");
    assert_eq!(diagnostic(&s, "C1"), None);
    set(&mut s, "A1", "6");
    // python.run runs the one cell; the ones after it are stale.
    let r = s.execute("python.run", json!({"cell": "C1"})).unwrap();
    assert_eq!(r["queued"], 1);
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(6.0));
    assert_eq!(diagnostic(&s, "C1"), None);
    assert_eq!(diagnostic(&s, "C2"), Some("state.stale"));
    // python.runAll runs them all.
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((diagnostic(&s, "C2"), diagnostic(&s, "D5")), (None, None));
    assert_eq!(v(&s, "D5"), NUM(5.0));
    // Back to automatic, what is due runs at once.
    set(&mut s, "A1", "9");
    assert_eq!(v(&s, "C1"), NUM(6.0));
    s.execute("python.calculation", json!({"mode": "automatic"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(9.0));
    assert!(s.execute("python.calculation", json!({"mode": "sideways"})).is_err());
    assert_eq!(s.execute("python.calculation", json!({})).unwrap()["mode"], "automatic");
}

#[test]
fn a_workbook_on_manual_calculation_runs_its_python_the_same_way() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    let before = runs(&h).len();
    py(&mut s, "A1", "ret 2");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before);
    assert_eq!(diagnostic(&s, "A1"), Some("state.stale"));
}

#[test]
fn stop_kills_the_worker_and_the_waiting_cells_keep_what_they_kept() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    py(&mut s, "A3", "ret 3");
    settle(&mut s);
    py(&mut s, "A2", "hang");
    // A2 runs, A3 waits.
    s.python_poll(Duration::ZERO);
    read_requests(&h, 3);
    assert!(s.python_busy());
    assert_eq!((v(&s, "A2"), v(&s, "A3")), (Value::Error(CellError::Busy), Value::Error(CellError::Busy)));
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], true);
    assert!(!s.python_busy());
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python), "no longer #BUSY!");
    assert_eq!(diagnostic(&s, "A2"), Some("state.stopped"));
    assert_eq!(v(&s, "A3"), NUM(3.0), "it had a result");
    assert_eq!(diagnostic(&s, "A3"), Some("state.stale"));
    assert_eq!(s.python_run_log().last().map(|r| r.diagnostic), Some(Some("state.stopped")));
    // Nothing to stop now.
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], false);
    // An edit elsewhere doesn't start it again; asking does, in a fresh worker that first runs what
    // came before it.
    let before = runs(&h).len();
    set(&mut s, "E9", "1");
    settle(&mut s);
    assert_eq!(runs(&h).len(), before);
    py(&mut s, "A2", "ret 2");
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["ret 1", "ret 2", "ret 3"]);
    assert_eq!((v(&s, "A2"), v(&s, "A3")), (NUM(2.0), NUM(3.0)));
    assert_eq!(h.starts(), 2);
}

#[test]
fn reset_gives_fresh_variables_and_runs_everything() {
    let (mut s, h) = session();
    py(&mut s, "A1", "set a 1");
    py(&mut s, "A2", "get a");
    py(&mut s, "A3", "tick");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), NUM(1.0));
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A3"), NUM(2.0), "the same variables");
    assert_eq!(h.starts(), 1);
    let r = s.execute("python.reset", json!({})).unwrap();
    assert_eq!(r["queued"], 3);
    settle(&mut s);
    assert_eq!(v(&s, "A3"), NUM(1.0), "fresh variables");
    assert_eq!(v(&s, "A2"), NUM(1.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_result_spills_and_a_blocked_one_says_what_is_in_the_way() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "spill 3 1");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "A2"), v(&s, "A3")), (NUM(10.0), NUM(20.0), NUM(30.0)));
    set(&mut s, "B1", "=SUM(A1:A3)");
    assert_eq!(v(&s, "B1"), NUM(60.0));
    // A cell in the way blocks it, without it running.
    set(&mut s, "A3", "x");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!(diagnostic(&s, "A1"), Some("result.spill"));
    assert!(s.python_diagnostic(0, c("A1")).unwrap().detail.contains("A3"));
    set(&mut s, "A3", "");
    settle(&mut s);
    assert_eq!(v(&s, "A3"), NUM(30.0));
    assert_eq!(diagnostic(&s, "A1"), None);
    assert_eq!(v(&s, "B1"), NUM(60.0));
    // While it waits for a new result, what it spilled stays.
    py(&mut s, "A1", "spill 2 1");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
    settle(&mut s);
    assert_eq!((v(&s, "A2"), v(&s, "A3")), (NUM(20.0), Value::Empty));
}

#[test]
fn an_object_shows_its_type_and_keeps_its_preview_out_of_the_file() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "obj");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("dict"), "an object with no cell form shows its type");
    s.execute("python.output", json!({"cell": "A1", "mode": "object"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::text("dict"));
    let got = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!((got["python"]["typeName"].as_str(), got["python"]["preview"].as_str()), (Some("dict"), Some("{'a': 1}")));
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "o.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("o.xlsx", &bytes).unwrap();
    let cell = back.sheet(0).unwrap().cell(c("A1")).unwrap();
    assert_eq!(cell.value, Value::text("dict"));
    assert_eq!(cell.py.as_ref().and_then(|p| p.output.as_ref()), None, "the preview isn't in the file");
}

#[test]
fn what_a_result_holds_is_explained() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "num");
    py(&mut s, "A2", "empty");
    py(&mut s, "A3", "none");
    settle(&mut s);
    assert_eq!((diagnostic(&s, "A1"), v(&s, "A1")), (Some("result.number"), Value::Error(CellError::Num)));
    assert_eq!((diagnostic(&s, "A2"), v(&s, "A2")), (Some("result.empty"), Value::Error(CellError::Calc)));
    assert_eq!((diagnostic(&s, "A3"), v(&s, "A3")), (None, Value::Empty), "a cell that only defines names is fine");
}

#[test]
fn each_way_a_worker_can_end_shows_its_error() {
    let limits = Limits { timeout: Duration::from_millis(150), start_timeout: Duration::from_secs(5), line_bytes: 1024 * 1024, ..Limits::default() };
    for (code, value, kind) in [
        ("hang", CellError::Timeout, "limit.time"),
        ("flood", CellError::Python, "limit.result"),
        ("garbage", CellError::Python, "worker.lost"),
        ("die", CellError::Python, "worker.lost"),
        ("fail", CellError::Python, "python.exception"),
    ] {
        let (mut s, _h) = with_limits(limits.clone());
        py(&mut s, "A1", "ret 1");
        settle(&mut s);
        py(&mut s, "A2", code);
        settle(&mut s);
        assert_eq!(v(&s, "A2"), Value::Error(value), "{code}");
        assert_eq!(diagnostic(&s, "A2"), Some(kind), "{code}");
        assert_eq!(s.python_run_log().last().and_then(|r| r.diagnostic), Some(kind), "{code}");
    }
}

#[test]
fn going_over_the_memory_limit_shows_its_error() {
    let (mut s, h) = with_limits(Limits { memory_bytes: 1024, ..Limits::default() });
    py(&mut s, "A1", "hang");
    h.set_rss(1024 * 1024);
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Python));
    assert_eq!(diagnostic(&s, "A1"), Some("limit.memory"));
}

/// A transport that never starts.
pub(super) struct NoStart(pub(super) &'static str);

impl Transport for NoStart {
    fn start(&mut self) -> Result<Pipes, String> {
        Err(self.0.into())
    }
    fn kill(&mut self) {}
}

fn failing(message: &'static str) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(NoStart(message)))),
        ..PythonCapability::default()
    });
    s
}

/// A worker that doesn't start the first few times, as when another thread's start leaked a
/// descriptor to the launcher.
struct StartsLate {
    left: Arc<std::sync::atomic::AtomicU32>,
    message: &'static str,
    then: FakeTransport,
}

const REFUSED: &str = "Python stopped: gridcraft-sandbox: refused: descriptors other than standard input, output and error are open";

impl Transport for StartsLate {
    fn start(&mut self) -> Result<Pipes, String> {
        let left = self.left.load(std::sync::atomic::Ordering::SeqCst);
        if left > 0 {
            self.left.store(left - 1, std::sync::atomic::Ordering::SeqCst);
            return Err(self.message.into());
        }
        self.then.start()
    }
    fn kill(&mut self) {
        self.then.kill();
    }
}

#[test]
fn a_launch_refused_for_a_moment_is_tried_again() {
    let t = worker();
    let left = Arc::new(std::sync::atomic::AtomicU32::new(crate::python::START_RETRIES_FOR_TEST));
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(move || Box::new(StartsLate { left: left.clone(), message: REFUSED, then: t.sharing() }))),
        ..PythonCapability::default()
    });
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(1.0), "the worker started on a later try");
    assert_eq!(s.python_environment().state, PythonState::Ready);
    assert_eq!(s.python_run_log().count(), 1, "the tries that didn't start aren't runs");
}

#[test]
fn a_cell_that_could_not_start_runs_once_python_can() {
    // More failures than a cell tries: Python doesn't count as ready, until a new check.
    for (message, value, kind) in [(REFUSED, CellError::Blocked, "blocked.sandboxCheck"), ("no such file", CellError::Connect, "connect.failed")] {
        let t = worker();
        let left = Arc::new(std::sync::atomic::AtomicU32::new(crate::python::START_RETRIES_FOR_TEST + 1));
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        s.set_python_capability(PythonCapability {
            enabled: true,
            transport: Some(Arc::new(move || Box::new(StartsLate { left: left.clone(), message, then: t.sharing() }))),
            ..PythonCapability::default()
        });
        py(&mut s, "A1", "ret 1");
        settle(&mut s);
        assert_eq!(v(&s, "A1"), Value::Error(value), "{kind}");
        assert_eq!(diagnostic(&s, "A1"), Some(kind));
        s.execute("python.refreshEnvironment", json!({})).unwrap();
        settle(&mut s);
        assert_eq!(v(&s, "A1"), NUM(1.0), "{kind}");
        assert_eq!(diagnostic(&s, "A1"), None);
    }
}

#[test]
fn a_refusal_by_the_launcher_shows_blocked_and_stops_trying() {
    let mut s = failing("Python stopped: gridcraft-sandbox: refused: the sandbox is not enforced");
    py(&mut s, "A1", "ret 1");
    py(&mut s, "A2", "ret 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked));
    assert_eq!(diagnostic(&s, "A1"), Some("blocked.sandboxCheck"));
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Blocked), "the rest doesn't try");
    assert_eq!(diagnostic(&s, "A2"), Some("blocked.sandboxCheck"));
    assert_eq!(s.python_environment().state, PythonState::Blocked);
    assert!(s.execute("python.runAll", json!({})).unwrap_err().to_string().contains("sandbox"));
    // A new check clears it.
    s.execute("python.refreshEnvironment", json!({})).unwrap();
    assert_eq!(s.python_environment().state, PythonState::Ready);
}

#[test]
fn a_worker_that_does_not_start_shows_connect() {
    let mut s = failing("no such file");
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Connect));
    assert_eq!(diagnostic(&s, "A1"), Some("connect.failed"));
    assert_eq!(s.python_environment().state, PythonState::NotSetUp);
    // Running again tries again.
    let r = s.execute("python.run", json!({"cell": "A1"}));
    assert!(r.is_ok(), "{r:?}");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Connect));
}

#[test]
fn undo_and_redo_around_a_run() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    py(&mut s, "B1", "say xl(\"A1\")");
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(1.0));
    set(&mut s, "A1", "2");
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(2.0));
    // Undo puts back the input and the result of that time; the cell runs, as its variables may have
    // changed, and comes to the same.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), NUM(1.0));
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(1.0));
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "B1")), (NUM(2.0), NUM(2.0)));
    // Undoing the cell itself removes it from what runs, and takes the result with it.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "B1"), Value::Empty);
    assert!(s.python_tracked_cells().is_empty());
    // An edit made while a cell waits: undo brings back a cell marked busy, which then runs.
    py(&mut s, "B1", "say xl(\"A1\")");
    set(&mut s, "A1", "3");
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Busy));
    set(&mut s, "A2", "4");
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(3.0));
    // The same with the result already in: the copy says it waits, and the result in it is old.
    py(&mut s, "C1", "say xl(\"A1\")");
    set(&mut s, "A3", "5");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(3.0));
    s.execute("edit.undo", json!({})).unwrap();
    assert_ne!(v(&s, "C1"), Value::Error(CellError::Blocked));
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(3.0), "it runs, rather than showing no result");
    assert!(!h.requests().is_empty());
    let undo = s.doc().unwrap().undo.len();
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.doc().unwrap().undo.len(), undo, "a run is no step");
}

/// A workbook as a file would bring it: Python cells with their cached results, from elsewhere.
fn opened_from_file(s: &mut Session) {
    let mut source = Session::new();
    source.execute("file.new", json!({})).unwrap();
    source.execute("python.set", json!({"cell": "A1", "code": "ret 9"})).unwrap();
    let bytes = crate::io::save_bytes(&source.doc().unwrap().wb, "f.xlsx").unwrap();
    let (wb, _) = crate::io::open_bytes("f.xlsx", &bytes).unwrap();
    s.add_document(DocState::from_saved(wb, Some("f.xlsx".into()), "f".into(), Trust::FromFile));
}

#[test]
fn a_workbook_from_a_file_stays_off_until_the_person_turns_it_on() {
    let (mut s, h) = session();
    opened_from_file(&mut s);
    let value = v(&s, "A1");
    assert!(!s.python_workbook_enabled());
    set(&mut s, "B1", "1");
    py(&mut s, "A1", "ret 10");
    settle(&mut s);
    assert_eq!(h.starts(), 0, "no worker starts for it");
    assert_eq!(diagnostic(&s, "A1"), Some("off.workbook"));
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked));
    assert_ne!(value, NUM(10.0));
    assert!(s.execute("python.runAll", json!({})).unwrap_err().to_string().contains("isn't on for this workbook"));
    assert!(s.execute("python.run", json!({"cell": "A1"})).is_err());
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], false);
    let list = s.execute("python.diagnostics", json!({})).unwrap();
    assert_eq!(list[0]["kind"], "off.workbook");
    // The person turns it on: its cells run.
    s.set_python_workbook_enabled(true);
    assert!(s.python_workbook_enabled());
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(10.0));
    assert_eq!(h.starts(), 1);
    // And off again: the worker stops, and the cell keeps what it shows.
    py(&mut s, "A2", "hang");
    s.python_poll(Duration::ZERO);
    s.set_python_workbook_enabled(false);
    assert!(!s.python_busy());
    assert_eq!(v(&s, "A1"), NUM(10.0));
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Blocked), "no result, and no longer busy");
    assert_eq!(diagnostic(&s, "A2"), Some("off.workbook"));
}

#[test]
fn python_code_pasted_from_a_file_turns_python_off_here() {
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    let here = s.doc().unwrap().uid;
    opened_from_file(&mut s);
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.set_active(s.document_index(here).unwrap());
    let starts = h.starts();
    s.execute("edit.paste", json!({"at": "D1"})).unwrap();
    assert!(!s.python_workbook_enabled());
    settle(&mut s);
    assert_eq!(h.starts(), starts);
    assert_eq!(diagnostic(&s, "D1"), Some("off.workbook"));
    // Even where the person had turned it on.
    s.set_python_workbook_enabled(true);
    settle(&mut s);
    s.execute("file.new", json!({})).unwrap();
    let plain = s.doc().unwrap().uid;
    s.set_python_workbook_enabled(true);
    let from = s.documents().iter().find(|d| d.trust == Trust::FromFile && d.uid != here).map(|d| d.uid).unwrap();
    s.set_active(s.document_index(from).unwrap());
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.set_active(s.document_index(plain).unwrap());
    s.execute("edit.paste", json!({"at": "A1"})).unwrap();
    assert!(!s.python_workbook_enabled());
}

#[test]
fn values_pasted_from_a_file_leave_python_on() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    let here = s.doc().unwrap().uid;
    opened_from_file(&mut s);
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    set(&mut s, "C3", "=1+1");
    s.execute("edit.copy", json!({"range": "C3"})).unwrap();
    s.set_active(s.document_index(here).unwrap());
    s.execute("edit.paste", json!({"at": "D1"})).unwrap();
    assert_eq!(s.doc().unwrap().trust, Trust::FromFile, "macros treat it as it came");
    assert!(s.python_workbook_enabled(), "there is no Python code in it");
}

#[test]
fn closing_a_workbook_stops_its_worker_while_it_runs() {
    let (mut s, h) = session();
    let first = s.active_index();
    py(&mut s, "A1", "hang");
    s.python_poll(Duration::ZERO);
    assert!(s.python_busy());
    assert_eq!(h.starts(), 1);
    s.execute("file.new", json!({})).unwrap();
    s.close_document(first);
    assert!(!s.python_busy(), "its work went with it");
    assert!(!s.python_poll(Duration::ZERO));
    // The other workbook goes on, in a worker of its own.
    py(&mut s, "A1", "ret 2");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(2.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn each_workbook_has_its_own_worker_and_variables() {
    let (mut s, h) = session();
    py(&mut s, "A1", "set a 1");
    settle(&mut s);
    s.execute("file.new", json!({})).unwrap();
    py(&mut s, "A1", "get a");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Python), "the variable is in the other workbook");
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_workbook_without_python_cells_costs_no_work() {
    let (mut s, h) = session();
    let scans = s.python_stats().full_scans;
    for i in 1..50 {
        set(&mut s, &format!("A{i}"), "1");
        assert!(!s.python_busy());
        s.python_poll(Duration::ZERO);
    }
    s.execute("home.insertRows", json!({"rows": "2:2"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let stats = s.python_stats();
    assert_eq!(stats.full_scans, scans, "the workbook is looked through once, when Python turns on for it");
    assert_eq!(stats.clients, 0);
    assert_eq!(stats.areas_hashed, 0);
    assert_eq!(h.starts(), 0, "no worker starts");
    // A session that was given no capability doesn't even keep state.
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    py(&mut s, "A1", "ret 1");
    assert!(!s.python_poll(Duration::from_millis(1)));
    assert_eq!(s.python_stats(), Default::default());
    assert!(!s.python_busy());
}

#[test]
fn thousands_of_cells_cost_one_read_of_what_they_share() {
    const CELLS: u32 = 3_000;
    let (mut s, h) = session();
    s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    set(&mut s, "A1", "1");
    let code: Vec<Vec<serde_json::Value>> = vec![vec![json!("x")]];
    s.execute("range.setValues", json!({"range": "H1", "values": code})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        let sh = wb.sheet_mut(0).unwrap();
        for row in 0..CELLS {
            let py = gridcraft_model::PyCode::new("sum xl(\"A1\") xl(\"H:H\")");
            sh.set_cell(CellRef::new(row, 2), gridcraft_model::Cell { value: Value::Number(0.0), py: Some(Arc::new(py)), ..Default::default() });
        }
        d.calc.rebuild(&wb);
        d.wb = Arc::new(wb);
    }
    // Picks up the cells, which all came at once.
    s.python_poll(Duration::ZERO);
    assert_eq!(s.python_tracked_cells().len(), CELLS as usize);
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).len(), CELLS as usize);
    s.execute("python.calculation", json!({"mode": "automatic"})).unwrap();
    // An edit nothing reads reads nothing.
    let hashed = s.python_stats().areas_hashed;
    set(&mut s, "E1", "1");
    settle(&mut s);
    assert_eq!(s.python_stats().areas_hashed, hashed, "no area is read again");
    assert_eq!(runs(&h).len(), CELLS as usize, "and nothing runs");
    // An edit they all read reads each area once for all of them, not once per cell.
    s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    set(&mut s, "A1", "2");
    assert_eq!(s.python_stats().areas_hashed, hashed + 2, "A1 and H:H");
    assert_eq!(s.python_stats().full_scans, 1);
}

#[test]
fn references_read_cells_ranges_other_sheets_names_and_tables() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    set(&mut s, "A2", "2");
    set(&mut s, "A4", "4");
    s.execute("home.insertSheet", json!({})).unwrap();
    let other = s.doc().unwrap().wb.sheet(1).unwrap().name.clone();
    s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    set(&mut s, "B2", "20");
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("formulas.defineName", json!({"name": "rates", "refersTo": "Sheet1!$A$1:$A$2"})).unwrap();
    s.execute("range.setValues", json!({"range": "K1", "values": [["n"], [5], [6]]})).unwrap();
    s.execute("insert.table", json!({"range": "K1:K3", "headers": true, "name": "Scores"})).unwrap();
    // A single cell is a scalar, a range an array; an empty cell comes as empty.
    py(&mut s, "C1", "say xl(\"A1\")");
    py(&mut s, "C2", "shape xl(\"A1:A2\")");
    py(&mut s, "C3", &format!("say xl(\"{other}!B2\")"));
    // A whole column stops at what the sheet uses.
    py(&mut s, "C4", "shape xl(\"A:A\")");
    py(&mut s, "C5", "sum xl(\"rates\")");
    py(&mut s, "C6", "sum xl(\"Scores[n]\")");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(1.0));
    assert_eq!(v(&s, "C2"), NUM(2.0));
    assert_eq!(v(&s, "C3"), NUM(20.0));
    assert!(matches!(v(&s, "C4"), Value::Number(n) if (6.0..100.0).contains(&n)), "{:?}", v(&s, "C4"));
    assert_eq!(v(&s, "C5"), NUM(3.0));
    assert_eq!(v(&s, "C6"), NUM(11.0));
    let inputs = h.requests().into_iter().find_map(|r| match r {
        Request::Run { code, inputs, .. } if code.starts_with("say xl(\"A1\")") => Some(inputs),
        _ => None,
    });
    assert_eq!(inputs.unwrap()["A1"].value, NUM(1.0), "keyed by the reference as written");
    // What they read through a name follows the name.
    s.execute("formulas.defineName", json!({"name": "rates", "oldName": "rates", "refersTo": "Sheet1!$A$1:$A$1"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "C5"), NUM(1.0));
}

#[test]
fn too_many_cells_and_bad_references_do_not_run() {
    let (mut s, h) = session();
    // A block past the limit, with no whole-column cap to save it.
    s.execute("home.insertSheet", json!({})).unwrap();
    let other = s.doc().unwrap().wb.sheet(1).unwrap().name.clone();
    set(&mut s, "XFD1048575", "1");
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    const { assert!(26 * 50_000 > MAX_ARRAY_CELLS) };
    py(&mut s, "A1", &format!("say xl(\"{other}!A1:Z50000\")"));
    py(&mut s, "A2", &format!("say xl(\"{other}!A:XFD\")"));
    py(&mut s, "A3", "say xl(\"Nowhere!A1\")");
    py(&mut s, "A4", "say xl(\"#REF!\")");
    py(&mut s, "A5", "say xl(\"no_such_name\")");
    py(&mut s, "A6", "say xl(\"SUM(A1)\")");
    py(&mut s, "A8", "ret 8");
    settle(&mut s);
    for cell in ["A1", "A2"] {
        assert_eq!(v(&s, cell), Value::Error(CellError::Python), "{cell}");
        assert_eq!(diagnostic(&s, cell), Some("xl.tooLarge"), "{cell}");
    }
    for cell in ["A3", "A4", "A5", "A6"] {
        assert_eq!(v(&s, cell), Value::Error(CellError::Ref), "{cell}");
        assert_eq!(diagnostic(&s, cell), Some("xl.badReference"), "{cell}");
    }
    assert!(s.python_diagnostic(0, c("A3")).unwrap().detail.contains("Nowhere!A1"));
    assert_eq!(runs(&h), ["ret 8"], "only the cell that can run ran");
    assert_eq!(v(&s, "A8"), NUM(8.0));
}

#[test]
fn a_name_defined_further_down_is_explained() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "get total");
    py(&mut s, "A2", "ret 2");
    py(&mut s, "A3", "set total 3\ntotal = 3");
    settle(&mut s);
    // The third cell's run defines it, and the first has already failed.
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Python));
    let d = s.python_diagnostic(0, c("A1")).unwrap();
    assert_eq!(d.kind.id, "python.nameOrder");
    assert!(d.detail.contains("A3") && d.detail.contains("total"), "{}", d.detail);
    assert_eq!(d.line, Some(1));
    // With no such cell, it stays a plain undefined name.
    py(&mut s, "A4", "get never");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "A4"), Some("python.name"));
}

#[test]
fn the_names_code_assigns_are_found_by_a_plain_look() {
    use crate::python::assigned_names_for_test as names;
    assert_eq!(names("total = 1"), ["total"]);
    assert_eq!(names("x == 1\ny.z = 2\n# w = 3\ns = 'v = 4'"), ["s"]);
    assert_eq!(names("def f(): pass\nclass C: pass\nimport m\nimport a as b\nfor i in r: pass"), ["f", "C", "m", "a", "b", "i"]);
    assert_eq!(names("é = 1\n\"unterminated"), ["é"]);
    assert!(names("").is_empty());
}

#[test]
fn the_run_log_is_bounded_and_keeps_printed_output() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "print 100");
    settle(&mut s);
    let last = s.python_run_log().last().cloned().unwrap();
    assert_eq!((last.cell, last.stdout.len(), last.stdout_cut, last.diagnostic), (c("A1"), 100, false, None));
    assert_eq!(diagnostic(&s, "A1"), None);
    // An error keeps what it printed before.
    py(&mut s, "A1", "failprint");
    settle(&mut s);
    let last = s.python_run_log().last().cloned().unwrap();
    assert_eq!((last.stdout.as_str(), last.diagnostic), ("before the error\n", Some("python.exception")));
    // Output past the log's 64 KiB is cut, and the cell says so.
    py(&mut s, "A1", "print 100000");
    settle(&mut s);
    let last = s.python_run_log().last().cloned().unwrap();
    assert_eq!((last.stdout.len(), last.stdout_cut, last.diagnostic), (64 * 1024, true, Some("limit.output")));
    assert_eq!(diagnostic(&s, "A1"), Some("limit.output"));
    assert_eq!(v(&s, "A1"), NUM(1.0), "a warning, not an error");
    let json = s.execute("python.runLog", json!({})).unwrap();
    assert_eq!(json.as_array().unwrap().last().unwrap()["stdoutCut"], true);
    // The last 200 runs.
    for _ in 0..210 {
        s.execute("python.runAll", json!({})).unwrap();
        settle(&mut s);
    }
    assert_eq!(s.python_run_log().count(), 200);
}

#[test]
fn a_cell_that_stopped_the_worker_waits_and_the_rest_still_finds_its_variables() {
    let (mut s, h) = with_limits(Limits { timeout: Duration::from_millis(100), start_timeout: Duration::from_secs(5), ..Limits::default() });
    py(&mut s, "A1", "set a 1");
    py(&mut s, "A2", "hang");
    py(&mut s, "A3", "get a");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Timeout));
    // The worker was lost at A2: a fresh one ran A1 again before A3, and didn't run A2 again.
    assert_eq!(v(&s, "A3"), NUM(1.0));
    assert_eq!(runs(&h), ["set a 1", "hang", "set a 1", "get a"]);
    assert_eq!(h.starts(), 2);
    // A cell before it changes: the later cells run, but it doesn't stop the worker again.
    let before = runs(&h).len();
    py(&mut s, "A1", "set a 2");
    settle(&mut s);
    assert_eq!(runs(&h)[before..], ["set a 2", "get a"]);
    assert_eq!(v(&s, "A3"), NUM(2.0));
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Timeout), "it keeps its error");
    assert_eq!(diagnostic(&s, "A2"), Some("limit.time"));
    // Asking for it runs it.
    let before = runs(&h).len();
    s.execute("python.run", json!({"cell": "A2"})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h)[before], "hang");
    // Changing it runs it.
    py(&mut s, "A2", "ret 22");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), NUM(22.0));
}

#[test]
fn a_cell_reading_a_later_cell_settles_and_cells_feeding_each_other_stop() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "sum xl(\"A2\")");
    py(&mut s, "A2", "ret 5");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(5.0), "it runs again once the later cell has its result");
    assert_eq!(diagnostic(&s, "A1"), None);
    // Two cells that feed each other through the sheet never settle: it ends, and says so.
    let (mut s, h2) = session();
    py(&mut s, "A1", "tick xl(\"A2\")");
    py(&mut s, "A2", "tick xl(\"A1\")");
    settle(&mut s);
    assert!(runs(&h2).len() < 40, "{} runs", runs(&h2).len());
    assert_eq!(diagnostic(&s, "A1"), Some("state.stale"));
}

#[test]
fn a_cell_reading_a_later_blocked_result_settles() {
    // While the later cell waits it reads as what it will show again, #SPILL!, not as its first value.
    let (mut s, h) = session();
    set(&mut s, "A3", "in the way");
    py(&mut s, "A1", "say xl(\"A2\")");
    py(&mut s, "A2", "spill 2 1");
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Spill));
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Spill));
    assert_eq!(diagnostic(&s, "A1"), None, "it isn't left stale");
    assert!(runs(&h).len() <= 6, "{:?}", runs(&h));
    // Everything marked busy at once: each runs once, as nothing changes when the wait ends.
    let before = runs(&h).len();
    s.execute("python.runAll", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).len(), before + 2, "{:?}", runs(&h));
}

#[test]
fn a_cell_reading_a_later_failing_or_empty_cell_settles_soon() {
    // While the later cell waits it reads as what its last run left, so nothing changes when the
    // wait ends the same way: the first cell runs twice, not until the passes run out.
    for later in ["fail", "none"] {
        let (mut s, h) = session();
        py(&mut s, "A1", "say xl(\"A2\")");
        py(&mut s, "A2", later);
        settle(&mut s);
        assert_eq!(diagnostic(&s, "A1"), None, "{later}");
        assert!(runs(&h).len() <= 6, "{later}: {:?}", runs(&h));
        // An edit that sends them round again changes nothing either.
        let before = runs(&h).len();
        s.execute("python.runAll", json!({})).unwrap();
        settle(&mut s);
        assert_eq!(runs(&h).len(), before + 2, "{later}");
    }
}

#[test]
fn a_cell_reading_a_formula_over_a_later_cell_gets_its_result() {
    // No cycle: A1 reads C1, which reads A2, which runs after A1. A1 ran first on #BUSY!, runs again
    // once A2 has its result, and A2 going round again must not take that from it.
    let (mut s, h) = session();
    set(&mut s, "C1", "=A2");
    py(&mut s, "A1", "say xl(\"C1\")");
    py(&mut s, "A2", "ret 5");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(5.0), "the result, not the wait");
    assert_eq!(diagnostic(&s, "A1"), None);
    assert!(runs(&h).len() <= 6, "{:?}", runs(&h));
}

#[test]
fn a_cell_reading_a_formula_over_itself_settles_when_its_result_does() {
    let (mut s, h) = session();
    set(&mut s, "C1", "=A1+1");
    py(&mut s, "A1", "ret 3 xl(\"C1\")");
    settle(&mut s);
    assert_eq!((v(&s, "A1"), v(&s, "C1")), (NUM(3.0), NUM(4.0)));
    assert_eq!(diagnostic(&s, "A1"), None);
    assert!(runs(&h).len() <= 3, "{:?}", runs(&h));
}

#[test]
fn a_waiting_cell_that_had_failed_shows_its_error_again_when_stopped() {
    let (mut s, h) = session();
    py(&mut s, "A1", "hang");
    py(&mut s, "A2", "fail");
    s.python_poll(Duration::ZERO);
    read_requests(&h, 1);
    s.execute("python.stop", json!({})).unwrap();
    s.execute("python.run", json!({"cell": "A2"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python));
    s.execute("python.runAll", json!({})).unwrap();
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Busy));
    s.python_poll(Duration::ZERO);
    s.execute("python.stop", json!({})).unwrap();
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Python), "not #BLOCKED!: it has run");
}

#[test]
fn an_edit_while_a_cell_runs_discards_what_it_ran_on() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    py(&mut s, "B1", "say xl(\"A1\")");
    // It is sent, and the input changes before the result is taken.
    s.python_poll(Duration::ZERO);
    set(&mut s, "A1", "2");
    settle(&mut s);
    assert_eq!(v(&s, "B1"), NUM(2.0), "the old result didn't stick");
    assert_eq!(runs(&h).len(), 2);
    assert!(s.python_run_log().count() >= 2);
}

#[test]
fn a_python_cell_waiting_for_its_result_saves_the_result_it_keeps() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "ret 7");
    settle(&mut s);
    s.execute("python.runAll", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "b.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("b.xlsx", &bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().value(c("A1")), NUM(7.0), "not #VALUE!");
    assert!(!back.sheet(0).unwrap().cell(c("A1")).unwrap().py.as_ref().unwrap().busy);
}

#[test]
fn no_session_without_a_capability_starts_python() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    py(&mut s, "A1", "ret 1");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked));
    assert_eq!(diagnostic(&s, "A1"), Some("off.headless"));
    let env = s.python_environment();
    assert_eq!((env.state, env.diagnostic.map(|d| d.kind.id)), (PythonState::Off, Some("off.headless")));
    for cmd in ["python.run", "python.runAll", "python.reset"] {
        let e = s.execute(cmd, json!({"cell": "A1"})).unwrap_err();
        assert!(e.to_string().contains("doesn't run Python"), "{cmd}: {e}");
    }
    assert_eq!(s.execute("python.stop", json!({})).unwrap()["stopped"], false);
    assert_eq!(s.execute("python.environment", json!({})).unwrap()["state"], "off");
    assert_eq!(s.execute("python.refreshEnvironment", json!({})).unwrap()["state"], "off");
    assert_eq!(s.execute("python.diagnostics", json!({})).unwrap()[0]["kind"], "off.headless");
    assert_eq!(s.execute("python.runLog", json!({})).unwrap(), json!([]));
    assert_eq!(s.execute("python.calculation", json!({"mode": "manual"})).unwrap()["mode"], "manual");
    assert!(!s.python_busy());
    // A capability that is off is the person's choice, and says that instead.
    s.set_python_capability(PythonCapability::default());
    assert_eq!(s.python_environment().diagnostic.map(|d| d.kind.id), Some("off.app"));
    assert_eq!(diagnostic(&s, "A1"), Some("off.app"));
    assert!(!s.python_busy());
    // Taking a capability away leaves cells with what they keep.
    let (mut s, _h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    py(&mut s, "A2", "hang");
    s.python_poll(Duration::ZERO);
    s.set_python_capability(None);
    assert!(!s.python_busy());
    assert_eq!((v(&s, "A1"), v(&s, "A2")), (NUM(1.0), Value::Error(CellError::Blocked)));
    // Undo can bring back a cell marked busy; it doesn't stay that way.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.redo", json!({})).unwrap();
    assert!(!matches!(v(&s, "A2"), Value::Error(CellError::Busy)));
}

#[test]
fn no_command_turns_python_on_or_chooses_an_interpreter() {
    // Whatever a command is given, by a person, an agent or a file's macro, a session without a
    // capability has none afterwards, and starts nothing.
    let hostile = [
        json!({}),
        json!({"enabled": true, "on": true, "interpreter": "/tmp/evil", "launcher": "/tmp/evil", "python": "/tmp/evil", "path": "/tmp/evil"}),
        json!({"capability": {"enabled": true, "interpreter": "/tmp/evil"}, "mode": "automatic", "cell": "A1", "code": "ret 1"}),
    ];
    let skip = ["file.close", "window.close", "file.open", "file.save", "file.saveAs", "file.exportCsv", "file.exportHtml", "insert.picture"];
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    for spec in crate::command_specs() {
        // What a command says it takes is part of the claim: nothing names the setting.
        let doc = spec.params.to_ascii_lowercase();
        let id = spec.id.to_ascii_lowercase();
        for word in ["interpreter", "launcher", "capability"] {
            let says_read_only = spec.id == "python.environment" || spec.id == "python.refreshEnvironment";
            assert!(says_read_only || !doc.contains(word), "{}: {}", spec.id, spec.params);
            assert!(!id.contains(word), "{}", spec.id);
        }
        if skip.contains(&spec.id) {
            continue;
        }
        for params in &hostile {
            let _ = s.execute(spec.id, params.clone());
            s.take_ui_requests();
            if s.documents().is_empty() {
                s.execute("file.new", json!({})).unwrap();
            }
            assert!(s.python_capability().is_none(), "{}", spec.id);
            assert_eq!(s.python_stats().clients, 0, "{}", spec.id);
            assert!(!s.python_busy(), "{}", spec.id);
        }
    }
    // Nor can a preference carry it: what preferences hold has no such field.
    let prefs = serde_json::to_value(&s.prefs).unwrap().to_string().to_ascii_lowercase();
    assert!(!prefs.contains("python") && !prefs.contains("interpreter"), "{prefs}");
}

#[test]
fn a_session_with_a_capability_keeps_it_whatever_commands_run() {
    let (mut s, h) = session();
    opened_from_file(&mut s);
    let skip =
        ["file.close", "window.close", "file.open", "file.save", "file.saveAs", "file.exportCsv", "file.exportHtml", "insert.picture", "file.new"];
    for spec in crate::command_specs().iter().filter(|c| !skip.contains(&c.id) && !c.id.starts_with("window.")) {
        let _ = s.execute(spec.id, json!({"enabled": true, "on": true, "interpreter": "/tmp/evil"}));
        s.take_ui_requests();
    }
    assert!(s.python_capability().is_some_and(|c| c.interpreter.is_none() && c.launcher.is_none()));
    assert!(!s.python_workbook_enabled(), "a file's workbook stays off");
    assert_eq!(h.starts(), 0);
}

#[test]
fn the_environment_says_why_python_cannot_run() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability { enabled: true, interpreter: Some("/nonexistent/python3".into()), ..PythonCapability::default() });
    py(&mut s, "A1", "ret 1");
    let mut left = 2_000;
    while s.python_busy() {
        // Until it answers, a due cell shows it is waiting.
        assert_eq!(v(&s, "A1"), Value::Error(CellError::Busy));
        assert_eq!(s.python_environment().state, PythonState::Running);
        s.python_poll(Duration::from_millis(5));
        left -= 1;
        assert!(left > 0);
    }
    let env = s.python_environment();
    let id = env.diagnostic.as_ref().map(|d| d.kind.id);
    if cfg!(any(target_os = "linux", target_os = "macos")) && gridcraft_python::sandbox_support().is_ok() {
        assert_eq!((env.state, id), (PythonState::NotSetUp, Some("notSetUp.noInterpreter")));
    } else {
        assert_eq!((env.state, id), (PythonState::Blocked, Some("blocked.platform")));
    }
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked), "no result, no worker");
    assert_eq!(diagnostic(&s, "A1"), id);
    assert!(env.interpreter.is_none());
    let json = s.execute("python.environment", json!({})).unwrap();
    assert_eq!(json["diagnostic"]["kind"], json!(id));
    assert_eq!(s.python_stats().clients, 0);
}

#[test]
fn hostile_parameters_leave_the_commands_standing() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    for cmd in [
        "python.run",
        "python.runAll",
        "python.stop",
        "python.reset",
        "python.calculation",
        "python.environment",
        "python.diagnostics",
        "python.runLog",
        "python.refreshEnvironment",
    ] {
        for params in [json!({}), json!([1, 2]), json!({"range": "ZZZ999999999", "cell": "", "sheet": 99}), json!({"cell": 5, "mode": 5})] {
            let r = s.execute(cmd, params.clone());
            assert!(!matches!(r, Err(crate::EngineError::Internal(..))), "{cmd} {params}: {r:?}");
        }
    }
    assert!(s.take_ui_requests().is_empty(), "programmatic calls open no dialog");
    settle(&mut s);
    assert_eq!(v(&s, "A1"), NUM(1.0));
    let r = s.execute("python.run", json!({"cell": "Z9"}));
    assert!(r.unwrap_err().to_string().contains("isn't a Python cell"));
}

#[test]
fn losing_the_worker_again_and_again_ends_with_the_rest_stale() {
    let limits = Limits { timeout: Duration::from_millis(40), start_timeout: Duration::ZERO, ..Limits::default() };
    let (mut s, h) = with_limits(limits);
    for i in 1..=6 {
        py(&mut s, &format!("A{i}"), "hang");
    }
    py(&mut s, "A7", "ret 7");
    settle(&mut s);
    for cell in ["A1", "A2", "A3", "A4"] {
        assert_eq!(v(&s, cell), Value::Error(CellError::Timeout), "{cell}");
    }
    assert_eq!(diagnostic(&s, "A5"), Some("state.stale"), "the rest wait for someone to ask");
    assert_eq!(diagnostic(&s, "A7"), Some("state.stale"));
    assert_eq!(h.starts(), 4);
    // Asking starts over.
    py(&mut s, "A5", "ret 5");
    settle(&mut s);
    assert_eq!(v(&s, "A5"), NUM(5.0));
}

#[test]
fn a_reference_can_name_a_formulas_spill() {
    let (mut s, _h) = session();
    set(&mut s, "A1", "=SEQUENCE(3)");
    py(&mut s, "C1", "sum xl(\"A1#\")");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(6.0));
    assert_eq!(diagnostic(&s, "C1"), None);
    set(&mut s, "A1", "=SEQUENCE(4)");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(10.0), "it follows the spill as it grows");
}

#[test]
fn a_panic_in_python_work_stops_python_and_keeps_the_workbook() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability {
        enabled: true,
        transport: Some(Arc::new(|| panic!("the worker's transport broke"))),
        ..PythonCapability::default()
    });
    set(&mut s, "A1", "1");
    py(&mut s, "B1", "ret 2");
    assert!(s.python_poll(Duration::ZERO), "the panic stays here");
    assert!(!s.python_busy());
    let env = s.python_environment();
    assert_eq!(env.state, PythonState::NotSetUp);
    assert!(env.diagnostic.unwrap().detail.contains("transport broke"));
    assert_eq!(v(&s, "A1"), NUM(1.0));
    // The session goes on, and the cell isn't left waiting.
    set(&mut s, "A2", "2");
    s.python_poll(Duration::ZERO);
    assert_eq!(v(&s, "A2"), NUM(2.0));
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Blocked));
    assert!(!s.python_busy());
}

#[test]
fn code_that_differs_only_in_what_its_references_spell_still_runs() {
    // It reads the same values, so its fingerprint is the same; but new code keeps no result.
    let (mut s, h) = session();
    set(&mut s, "A1", "5");
    set(&mut s, "B1", "5");
    py(&mut s, "C1", "say xl(\"A1\")");
    settle(&mut s);
    let before = runs(&h).len();
    py(&mut s, "C1", "say xl(\"B1\")");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(5.0));
    assert_eq!(runs(&h).len(), before + 1);
    // It reads B1 now.
    set(&mut s, "B1", "6");
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(6.0));
}

/// F9 is how a person runs waiting cells under manual calculation, as in Excel.
#[test]
fn calculate_now_runs_python_cells_that_wait() {
    let (mut s, h) = session();
    set(&mut s, "A1", "1");
    py(&mut s, "C1", "say xl(\"A1\")");
    settle(&mut s);
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    set(&mut s, "A1", "5");
    settle(&mut s);
    assert_eq!(diagnostic(&s, "C1"), Some("state.stale"));
    assert_eq!(v(&s, "C1"), NUM(1.0));
    let before = runs(&h).len();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "C1"), NUM(5.0));
    assert_eq!(diagnostic(&s, "C1"), None);
    assert!(runs(&h).len() > before);
    // With nothing waiting, F9 runs nothing.
    let before = runs(&h).len();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(runs(&h).len(), before);
}

/// The running cell is what the status bar names, and it goes when the cell stops.
#[test]
fn the_running_cell_is_reported_with_how_long_it_has_run() {
    let (mut s, h) = session();
    assert_eq!(s.python_running(), None);
    py(&mut s, "B2", "hang");
    s.python_poll(Duration::ZERO);
    read_requests(&h, 1);
    let running = s.python_running().unwrap();
    assert_eq!((running.sheet.as_str(), running.cell), ("Sheet1", c("B2")));
    assert_eq!(s.python_state(), PythonState::Running);
    s.execute("python.stop", json!({})).unwrap();
    assert_eq!(s.python_running(), None);
    assert_eq!(s.python_state(), PythonState::Ready);
    assert_eq!(s.python_state(), s.python_environment().state);
}

/// The log can be emptied, by a command that is safe with no params and on an empty log.
#[test]
fn the_run_log_clears_and_the_cells_keep_what_they_show() {
    let (mut s, _) = session();
    assert_eq!(s.execute("python.clearRunLog", json!({})).unwrap()["cleared"], 0);
    py(&mut s, "A1", "ret 5");
    settle(&mut s);
    assert_eq!(s.python_run_log().count(), 1);
    assert_eq!(s.execute("python.clearRunLog", json!({})).unwrap()["cleared"], 1);
    assert_eq!(s.python_run_log().count(), 0);
    assert_eq!(v(&s, "A1"), NUM(5.0));
    assert_eq!(s.execute("python.clearRunLog", json!({})).unwrap()["cleared"], 0);
}

fn off_session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability::default());
    s
}

/// With Python off the workbook's code is still known, from what each edit touched.
#[test]
fn whether_a_workbook_holds_code_follows_every_way_code_comes_and_goes() {
    let mut s = off_session();
    s.python_poll(Duration::ZERO);
    assert!(!s.python_has_cells());
    set(&mut s, "A1", "1");
    assert!(!s.python_has_cells(), "a plain value isn't code");
    py(&mut s, "B1", "ret 1");
    assert!(s.python_has_cells());
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!s.python_has_cells(), "undo takes it away");
    s.execute("edit.redo", json!({})).unwrap();
    assert!(s.python_has_cells(), "redo brings it back");
    s.execute("python.clear", json!({"cell": "B1"})).unwrap();
    assert!(!s.python_has_cells(), "converted to a normal cell");
    py(&mut s, "C3", "ret 1");
    s.execute("home.insertSheet", json!({})).unwrap();
    assert!(s.python_has_cells(), "another sheet being active changes nothing");
    py(&mut s, "A1", "ret 2");
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("cell.set", json!({"cell": "C3", "input": ""})).unwrap();
    assert!(s.python_has_cells(), "the code on the other sheet is still there");
    s.execute("home.deleteSheet", json!({"sheet": 1})).unwrap();
    assert!(!s.python_has_cells(), "deleting its sheet takes the last of it");
}

/// Asked before the session has seen the workbook, it looks through it; once on, the list of cells
/// answers.
#[test]
fn whether_a_workbook_holds_code_is_known_before_and_while_python_runs() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("python.set", json!({"cell": "A1", "code": "ret 1"})).unwrap();
    assert!(s.python_has_cells(), "a session with no capability looks");
    let (mut on, _) = session();
    assert!(!on.python_has_cells());
    py(&mut on, "A1", "ret 1");
    settle(&mut on);
    assert!(on.python_has_cells());
    on.execute("cell.set", json!({"cell": "A1", "input": ""})).unwrap();
    settle(&mut on);
    assert!(!on.python_has_cells());
}

/// A given report answers instead of a check: the interface's tests reach every state with it.
#[test]
fn a_given_report_answers_for_an_enabled_capability_only() {
    let report = crate::python::PythonEnvironment { state: PythonState::NotSetUp, ..crate::python::PythonEnvironment::default() };
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.set_python_capability(PythonCapability { enabled: true, report: Some(report), ..PythonCapability::default() });
    assert_eq!(s.python_state(), PythonState::NotSetUp);
    assert!(!s.python_busy(), "nothing is being checked");
    let report = crate::python::PythonEnvironment { state: PythonState::Ready, ..crate::python::PythonEnvironment::default() };
    s.set_python_capability(PythonCapability { enabled: false, report: Some(report), ..PythonCapability::default() });
    assert_eq!(s.python_state(), PythonState::Off);
}

/// A search that may not start anything finds paths and asks nothing.
#[test]
fn a_search_while_python_is_off_starts_nothing() {
    for found in crate::python::search_pythons(false).found {
        assert_eq!((found.version, found.kind), (None, None), "{}", found.path.display());
        assert!(found.path.is_absolute());
    }
}

#[test]
fn a_worker_lost_right_after_a_result_is_rebuilt_before_the_next_cell() {
    // The reply and the closed pipe are both waiting when the poll takes the result and goes straight
    // on to the next cell: the client replaces the worker as it sends, and says nothing. A1 gets the
    // worker's first line out of the way, as the queue from the worker holds only two.
    let (mut s, h) = session();
    py(&mut s, "A1", "ret 0");
    py(&mut s, "A2", "setdie x 1");
    py(&mut s, "A3", "get x");
    s.python_poll(Duration::ZERO);
    read_requests(&h, 1);
    std::thread::sleep(Duration::from_millis(50));
    s.python_poll(Duration::ZERO);
    assert_eq!(v(&s, "A1"), NUM(0.0));
    read_requests(&h, 2);
    std::thread::sleep(Duration::from_millis(50));
    settle(&mut s);
    assert_eq!(v(&s, "A3"), NUM(1.0), "the variable is there: {:?}", runs(&h));
    assert_eq!(diagnostic(&s, "A3"), None);
    assert_eq!(diagnostic(&s, "A2"), None, "nothing is held against the cell before it");
}

#[test]
fn a_worker_lost_while_idle_is_rebuilt_before_the_next_cell() {
    let (mut s, h) = with_limits(Limits { memory_bytes: 1024, ..Limits::default() });
    py(&mut s, "A1", "set x 1");
    py(&mut s, "A2", "get x");
    s.execute("python.calculation", json!({"mode": "manual"})).unwrap();
    s.execute("python.run", json!({"cell": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!(h.starts(), 1);
    // The worker goes over its limit with nothing running. The watcher reads it slowly then.
    h.set_rss(1024 * 1024);
    std::thread::sleep(Duration::from_millis(200));
    h.set_rss(0);
    s.execute("python.run", json!({"cell": "A2"})).unwrap();
    settle(&mut s);
    assert_eq!(v(&s, "A2"), NUM(1.0), "{:?}", runs(&h));
    assert_eq!(runs(&h), ["set x 1", "set x 1", "get x"]);
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_worker_that_keeps_going_away_between_cells_ends_with_the_rest_stale() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "setdieall x 1");
    py(&mut s, "A2", "get x");
    // It ends, and the next cell never ran without its variable. Which it says depends on whether the
    // worker went before the cell was sent or after.
    settle(&mut s);
    let said = diagnostic(&s, "A2");
    assert!(matches!(said, Some("state.stale" | "worker.lost")), "{said:?}");
}

fn with_packages(names: &[&str]) -> Session {
    let t = worker();
    let report = crate::python::PythonEnvironment {
        state: PythonState::Ready,
        diagnostic: None,
        packages: names.iter().map(|n| ((*n).to_string(), "1.0".to_string())).collect(),
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
    s
}

fn workbook_kinds(s: &Session) -> Vec<(&'static str, String)> {
    s.python_workbook_diagnostics().into_iter().map(|e| (e.diagnostic.kind.id, e.diagnostic.detail)).collect()
}

/// A module is missing once, with the first cell that imports it, and goes when the code does.
#[test]
fn what_the_code_imports_is_checked_against_the_package_list() {
    let mut s = with_packages(&["numpy", "pandas"]);
    py(&mut s, "A1", "import numpy\nret 1");
    settle(&mut s);
    assert_eq!(workbook_kinds(&s), [], "numpy is there, and pandas");
    py(&mut s, "A2", "from sklearn.linear_model import X\nret 1");
    py(&mut s, "B7", "import sklearn\nimport json\nret 1");
    settle(&mut s);
    assert_eq!(workbook_kinds(&s), [("packages.missing", "sklearn".to_string())], "the standard library isn't missing");
    let entries = s.python_workbook_diagnostics();
    assert_eq!(entries[0].cell, Some((0, c("A2"))));
    let row = &s.execute("python.diagnostics", json!({})).unwrap()[0];
    assert_eq!(
        (row["kind"].as_str(), row["install"].as_str(), row["cell"].as_str()),
        (Some("packages.missing"), Some("scikit-learn"), Some("Sheet1!A2"))
    );
    s.execute("cell.set", json!({"cell": "A2", "input": ""})).unwrap();
    assert_eq!(workbook_kinds(&s).len(), 1, "B7 still imports it");
    assert_eq!(s.python_workbook_diagnostics()[0].cell, Some((0, c("B7"))));
    s.execute("cell.set", json!({"cell": "B7", "input": ""})).unwrap();
    assert_eq!(workbook_kinds(&s), []);
}

#[test]
fn an_empty_package_list_reports_nothing_missing() {
    let mut s = with_packages(&[]);
    py(&mut s, "A1", "import sklearn\nret 1");
    settle(&mut s);
    assert_eq!(workbook_kinds(&s), [], "the list isn't known");
    assert!(s.python_diagnostics().iter().all(|e| !e.diagnostic.kind.id.starts_with("packages.")));
}

#[test]
fn no_pandas_is_said_once_and_only_for_a_workbook_with_python_cells() {
    let mut s = with_packages(&["numpy"]);
    assert_eq!(workbook_kinds(&s), [], "nothing to say it about");
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert_eq!(workbook_kinds(&s), [("packages.noPandas", String::new())]);
    assert_eq!(s.python_diagnostics().iter().filter(|e| e.diagnostic.kind.id == "packages.noPandas").count(), 1);
}

/// Reading code is what costs: code already read isn't read again, until the package list changes.
#[test]
fn the_package_check_reads_only_what_changed() {
    let mut s = with_packages(&["pandas"]);
    py(&mut s, "A1", "import nope\nret 1");
    settle(&mut s);
    assert_eq!(s.python_stats().code_reads, 1);
    for _ in 0..5 {
        s.python_poll(Duration::ZERO);
    }
    set(&mut s, "C1", "5");
    settle(&mut s);
    assert_eq!(s.python_stats().code_reads, 1, "polling and other edits read nothing");
    py(&mut s, "A2", "ret 2");
    settle(&mut s);
    assert_eq!(s.python_stats().code_reads, 2, "the new cell alone");
    py(&mut s, "A3", "ret 2");
    settle(&mut s);
    assert_eq!(s.python_stats().code_reads, 2, "the same code isn't read twice");
    assert_eq!(workbook_kinds(&s).len(), 1);
    // A new list: "nope" is installed now.
    let t = worker();
    let report = crate::python::PythonEnvironment {
        state: PythonState::Ready,
        diagnostic: None,
        packages: vec![("pandas".to_string(), "1.0".to_string()), ("nope".to_string(), "1.0".to_string())],
        ..crate::python::PythonEnvironment::default()
    };
    s.set_python_capability(PythonCapability {
        enabled: true,
        report: Some(report),
        transport: Some(Arc::new(move || Box::new(t.sharing()))),
        ..PythonCapability::default()
    });
    settle(&mut s);
    assert_eq!(workbook_kinds(&s), []);
    assert!(s.python_stats().code_reads >= 4, "everything is read again: {}", s.python_stats().code_reads);
}
