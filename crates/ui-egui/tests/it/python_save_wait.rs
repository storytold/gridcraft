//! A save that waits for a workbook's Python cells: the bar, Save Now and Cancel, what ends a wait,
//! closing and quitting during one, which workbook is active after, and the control channel. A fake
//! worker stands in for Python.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::python::{Interpreter, InterpreterKind, MAX_WAIT, Version};
use gridcraft_engine::{PythonEnvironment, PythonState, Session};
use gridcraft_python::fake::{FakeHandle, FakeTransport};
use gridcraft_python::protocol::{Output, Request};
use gridcraft_ui_egui::control::{self, ControlRequest};
use gridcraft_ui_egui::save_wait::{SAVE_NOW, WAITING};
use gridcraft_ui_egui::{SaveAnswer, Services, SheetApp};
use serde_json::{Value as Json, json};

use crate::common::Harness;

/// What the fake worker's cells do: `wait n` answers `n` once the test opens the gate, `hang` never
/// answers, `die` loses the worker, and `ret n` answers `n` at once.
fn worker(gate: Arc<AtomicBool>) -> FakeTransport {
    FakeTransport::new(move |mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let (id, code) = match req {
                Request::Run { id, code, .. } => (id, code),
                Request::Render { id, .. } | Request::Reset { id } => (id, String::new()),
            };
            let words: Vec<&str> = code.split_whitespace().collect();
            let number = || Output::Value(Value::Number(words.get(1).and_then(|n| n.parse().ok()).unwrap_or(0.0)));
            match words.first().copied() {
                Some("wait") => {
                    while !gate.load(Ordering::SeqCst) && !w.killed() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                Some("hang") => {
                    w.stall();
                    return;
                }
                Some("die") => return,
                _ => {}
            }
            if !w.done(id, Ok(number())) {
                return;
            }
        }
    })
}

fn report() -> PythonEnvironment {
    let python = PathBuf::from("/home/you/.gridcraft/python/bin/python");
    PythonEnvironment {
        state: PythonState::Ready,
        diagnostic: None,
        sandbox: "a test sandbox".into(),
        uv: None,
        interpreter: Some(Interpreter {
            path: python.clone(),
            run_as: python,
            version: Version { major: 3, minor: 12, patch: 7 },
            kind: InterpreterKind::UvEnvironment,
            site_dirs: Vec::new(),
            read_paths: Vec::new(),
        }),
        packages: vec![("pandas".into(), "2.2.3".into())],
    }
}

/// An app with Python on and a workbook, and the gate its cells wait at.
struct Bench {
    h: Harness,
    gate: Arc<AtomicBool>,
    _handle: FakeHandle,
    control: Sender<ControlRequest>,
}

fn bench_with(services: Services, size: egui::Vec2) -> Bench {
    let mut session = Session::new();
    session.new_workbook();
    let gate = Arc::new(AtomicBool::new(false));
    let t = worker(gate.clone());
    let handle = t.handle();
    let mut app = crate::common::prepare(SheetApp::new(session, services));
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    let (control, rx) = channel();
    app.control_rx = Some(rx);
    let mut h = crate::common::harness_with(crate::common::builder().with_size(size), app, 3);
    h.run_steps(2);
    Bench { h, gate, _handle: handle, control }
}

fn bench() -> Bench {
    bench_with(Services::default(), vec2(1300.0, 800.0))
}

/// A file name for this test in the temporary directory, removed first.
fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gridcraft-save-wait-{}-{name}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// What a cell of the file at `path` holds, as a session without Python reads it.
fn in_file(path: &Path, cell: &str) -> Value {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

/// The code a cell of the file at `path` keeps.
fn code_in_file(path: &Path, cell: &str) -> Json {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.execute("cell.get", json!({"cell": cell})).unwrap()["python"]["code"].clone()
}

const SEVEN: Value = Value::Number(7.0);

impl Bench {
    fn app(&self) -> &SheetApp {
        self.h.state()
    }

    fn app_mut(&mut self) -> &mut SheetApp {
        self.h.state_mut()
    }

    fn doc(&self) -> u64 {
        self.app().session.active().unwrap().uid
    }

    /// A Python cell in the active workbook.
    fn py(&mut self, cell: &str, code: &str) {
        self.app_mut().run("python.set", json!({"cell": cell, "code": code})).unwrap();
    }

    /// A person's Save, to a file named here.
    fn save_to(&mut self, path: &Path) {
        self.app_mut().run_or_alert("file.save", json!({"path": path}));
    }

    /// Gives the active workbook a file, with nothing running.
    fn with_file(&mut self, path: &Path) {
        self.settle();
        self.app_mut().run("file.save", json!({"path": path})).unwrap();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.h.step();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn until(&mut self, what: &str, done: impl Fn(&SheetApp) -> bool) {
        for _ in 0..2_000 {
            if done(self.app()) {
                return;
            }
            self.h.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("never came about: {what}");
    }

    fn settle(&mut self) {
        self.until("Python settled", |a| !a.session.python_busy());
    }

    fn open_gate(&mut self) {
        self.gate.store(true, Ordering::SeqCst);
    }

    fn waiting(&self, doc: u64) -> bool {
        self.app().save_waits.waiting(doc)
    }

    /// Until the waits are over, and a frame after.
    fn until_saved(&mut self) {
        self.until("the wait ended", |a| !a.save_waits.any());
        self.h.step();
    }

    fn shows_bar(&self) -> bool {
        self.h.query_by_label(WAITING).is_some()
    }

    fn message(&self) -> Option<String> {
        self.app().message.as_ref().map(|(_, m)| m.clone())
    }

    fn dialog(&self) -> Option<&str> {
        self.app().dialog.as_ref().map(|d| d.name())
    }

    /// Answers the Save Changes question showing, as its buttons do.
    fn answer(&mut self, answer: SaveAnswer) {
        let ctx = self.h.ctx.clone();
        let d = self.app_mut().dialog.take().expect("the question is showing");
        assert_eq!(d.name(), "saveChanges");
        let quitting = d.values.get("quit").and_then(Json::as_bool) == Some(true);
        self.app_mut().answer_save_changes(answer, quitting, &ctx);
        self.h.step();
    }

    fn quit(&mut self) {
        let ctx = self.h.ctx.clone();
        self.app_mut().request_quit(&ctx);
        self.h.step();
    }

    /// A control request: its reply comes through the receiver, on a frame.
    fn ask(&mut self, method: &str, params: Json) -> Receiver<Json> {
        let (req, rx) = ControlRequest::new(method, params);
        self.control.send(req).unwrap();
        self.h.step();
        rx
    }

    fn inspect(&self) -> Json {
        control::inspect(self.app(), &self.h.ctx)
    }

    fn titles(&self) -> Vec<String> {
        self.app().session.documents().iter().map(|d| d.display_title()).collect()
    }

    fn active_title(&self) -> String {
        self.app().session.active().unwrap().display_title()
    }

    /// Brings a workbook forward by its uid, as a person does.
    fn go_to(&mut self, doc: u64) {
        let window = self.app().session.doc_window(doc).unwrap();
        self.app_mut().enter_window(window);
        self.h.step();
    }
}

fn answered(reply: &Receiver<Json>, b: &mut Bench) -> Json {
    for _ in 0..2_000 {
        if let Ok(v) = reply.try_recv() {
            return v;
        }
        b.h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no reply came");
}

// ---------------------------------------------------------------- the wait

#[test]
fn a_save_waits_with_the_bar_and_the_window_keeps_working_until_the_file_has_the_result() {
    let mut b = bench();
    let path = temp("waits");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.frames(3);
    assert!(b.shows_bar(), "it says it is waiting");
    assert!(b.h.query_by_label(SAVE_NOW).is_some());
    assert!(!path.exists(), "nothing is written while the cell runs");

    // Frames go on, and so does work.
    b.app_mut().run("cell.set", json!({"cell": "D1", "input": "typed meanwhile"})).unwrap();
    b.frames(5);
    assert!(b.shows_bar());

    b.open_gate();
    b.until_saved();
    assert!(!b.shows_bar());
    assert_eq!(in_file(&path, "A1"), SEVEN);
    assert_eq!(in_file(&path, "D1"), Value::text("typed meanwhile"));
    assert!(!b.app().session.active().unwrap().is_dirty());
    assert_eq!(b.message(), None, "everything finished: nothing to say");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_save_with_nothing_running_saves_at_once() {
    let mut b = bench();
    let path = temp("at-once");
    b.py("A1", "ret 7");
    b.settle();
    b.save_to(&path);
    assert!(path.exists());
    assert!(!b.waiting(b.doc()));
    let _ = std::fs::remove_file(&path);
}

/// Ctrl+S (Cmd+S on a Mac) on the grid.
#[test]
fn the_shortcut_waits() {
    let mut b = bench();
    let path = temp("shortcut");
    b.py("A1", "ret 1");
    b.with_file(&path);
    b.py("B1", "wait 7");
    b.frames(2);
    b.h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    b.frames(2);
    assert!(b.waiting(b.doc()));
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "B1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// The cell being edited goes in first, as for any save, and it can make cells due: the save waits
/// for those too. One its workbook refuses stays open with the reason, and nothing is saved.
#[test]
fn a_draft_goes_in_first_and_a_cell_it_makes_due_is_waited_for() {
    let mut b = bench();
    let path = temp("draft-first");
    b.py("B1", "ret 1");
    b.with_file(&path);
    b.py("A1", "wait 7");
    b.settle_after_gate();
    b.gate.store(false, Ordering::SeqCst);
    b.app_mut().run("selection.set", json!({"range": "C1"})).unwrap();
    b.app_mut().begin_edit(Some("typed".into()), false);
    b.h.step();
    b.app_mut().run("python.runAll", json!({})).unwrap();
    b.app_mut().run_or_alert("file.save", json!({}));
    assert!(b.waiting(b.doc()));
    assert!(b.app().editor.is_none(), "the draft was entered");
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "C1"), Value::text("typed"));
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// A cell that never finishes: the save comes at the cap and names it, with the last result in the
/// file and the code kept.
#[test]
fn at_the_cap_it_saves_what_is_there_and_says_which_cells_were_not_finished() {
    let mut b = bench();
    let path = temp("cap");
    b.py("B1", "ret 7");
    b.with_file(&path);
    b.py("A1", "hang");
    b.frames(2);
    b.save_to(&path);
    let cap = b.inspect()["saveWaits"][0]["capMs"].as_f64().unwrap();
    assert!(cap > 40_000.0 && cap <= MAX_WAIT.as_secs_f64() * 1000.0, "{cap}");
    let doc = b.doc();
    b.frames(2);
    assert!(b.waiting(doc), "the cap is far off");
    b.app_mut().save_waits.set_cap(doc, Duration::ZERO);
    b.until_saved();
    let message = b.message().unwrap();
    assert!(message.contains("weren't finished"), "{message}");
    assert!(message.contains("Sheet1!A1") && message.contains("Sheet1!B1"), "{message}");
    assert_eq!(in_file(&path, "B1"), SEVEN, "its last result");
    assert_eq!(code_in_file(&path, "A1"), json!("hang"));
    assert_eq!(code_in_file(&path, "B1"), json!("ret 7"));
    assert!(b.app().session.python_busy(), "the cell is still at it: the save didn't stop it");
    let _ = std::fs::remove_file(&path);
}

/// A long list is cut, with the rest counted.
#[test]
fn a_long_list_of_cells_is_cut_and_counted() {
    let mut b = bench();
    let path = temp("many");
    b.py("A1", "hang");
    for row in 2..=30 {
        b.py(&format!("A{row}"), "ret 1");
    }
    b.frames(2);
    b.save_to(&path);
    let doc = b.doc();
    b.app_mut().save_waits.set_cap(doc, Duration::ZERO);
    b.until_saved();
    let message = b.message().unwrap();
    assert!(message.contains("Sheet1!A10"), "{message}");
    assert!(!message.contains("Sheet1!A11"), "{message}");
    assert!(message.contains("and 20 more"), "{message}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_now_saves_without_waiting_and_says_what_was_not_finished() {
    let mut b = bench();
    let path = temp("save-now");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.frames(2);
    b.h.get_by_label(SAVE_NOW).click();
    b.h.step();
    b.until_saved();
    assert!(path.exists());
    assert!(b.message().is_some_and(|m| m.contains("Sheet1!A1")), "{:?}", b.message());
    assert_eq!(code_in_file(&path, "A1"), json!("wait 7"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cancel_ends_the_wait_and_nothing_is_saved_when_the_cells_finish() {
    let mut b = bench();
    let path = temp("cancel");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.frames(2);
    b.bar_cancel().click();
    b.h.step();
    b.frames(2);
    assert!(!b.waiting(b.doc()));
    assert!(!b.shows_bar());
    b.open_gate();
    b.settle();
    b.frames(2);
    assert!(!path.exists(), "the save was called off");
    assert!(b.app().session.active().unwrap().is_dirty(), "and the work is still there");
    assert_eq!(b.app().session.active().unwrap().path, None);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_cell_stale_under_manual_calculation_does_not_make_a_save_wait() {
    let mut b = bench();
    let path = temp("manual");
    b.py("A1", "ret 1");
    b.settle();
    b.app_mut().run("python.calculation", json!({"mode": "manual"})).unwrap();
    b.py("A1", "hang");
    b.frames(2);
    b.save_to(&path);
    assert!(path.exists(), "it saves at once");
    assert!(!b.waiting(b.doc()));
    assert!(!b.shows_bar());
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- what ends a wait early

#[test]
fn stop_during_the_wait_ends_it_and_the_save_says_what_it_left() {
    let mut b = bench();
    let path = temp("stop");
    b.py("A1", "hang");
    b.py("A2", "ret 2");
    b.frames(3);
    b.save_to(&path);
    b.app_mut().run("python.stop", json!({})).unwrap();
    b.until_saved();
    assert!(path.exists());
    let message = b.message().unwrap();
    assert!(message.contains("Sheet1!A1") && message.contains("Sheet1!A2"), "{message}");
    assert_eq!(code_in_file(&path, "A1"), json!("hang"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_worker_dying_during_the_wait_goes_on_to_the_next_cell_and_saves() {
    let mut b = bench();
    let path = temp("dies");
    b.py("A1", "die");
    b.py("A2", "ret 7");
    b.save_to(&path);
    b.until_saved();
    assert_eq!(in_file(&path, "A2"), SEVEN);
    assert_eq!(b.message(), None);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn python_turned_off_during_the_wait_ends_it_and_the_save_says_what_it_left() {
    let mut b = bench();
    let path = temp("off");
    b.py("A1", "hang");
    b.frames(2);
    b.save_to(&path);
    b.app_mut().ui.python.enabled = false;
    b.until_saved();
    assert!(path.exists());
    assert!(b.message().is_some_and(|m| m.contains("Sheet1!A1")), "{:?}", b.message());
    assert_eq!(code_in_file(&path, "A1"), json!("hang"));
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- saves during a wait

#[test]
fn a_second_save_joins_the_wait_and_does_not_start_it_over() {
    let mut b = bench();
    let path = temp("second");
    b.py("A1", "wait 7");
    b.with_file_while_running(&path);
    b.app_mut().run_or_alert("file.save", json!({}));
    let first = b.inspect()["saveWaits"][0]["ms"].as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    b.app_mut().run_or_alert("file.save", json!({}));
    b.app_mut().run_or_alert("file.save", json!({"path": path}));
    let waits = b.inspect()["saveWaits"].as_array().unwrap().clone();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0]["saves"].as_array().unwrap().len(), 1, "the same file, once");
    assert!(waits[0]["ms"].as_f64().unwrap() >= first + 15.0, "the wait goes on: {waits:?}");
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

impl Bench {
    /// Gives the active workbook a file while its cells run: a programmatic save, which doesn't wait.
    fn with_file_while_running(&mut self, path: &Path) {
        self.app_mut().run("file.save", json!({"path": path})).unwrap();
        self.h.step();
    }

    /// Lets the cells at the gate finish.
    fn settle_after_gate(&mut self) {
        self.open_gate();
        self.settle();
    }

    /// The bar's Cancel: the formula bar has a button of that name too.
    fn bar_cancel(&self) -> egui_kittest::Node<'_> {
        let row = self.h.get_by_label(SAVE_NOW).rect().center().y;
        self.h.get_all_by_label("Cancel").find(|n| (n.rect().center().y - row).abs() < 6.0).expect("the bar's Cancel")
    }
}

/// Save As during a wait doesn't start a second one: the wait writes both files, in order, and the
/// workbook's file is the new one, as if each had been saved in turn.
#[test]
fn save_as_with_a_new_name_during_a_wait_joins_it() {
    let first = temp("as-first");
    let second = temp("as-second");
    let answer = second.to_string_lossy().into_owned();
    let services = Services { pick_save: Some(Box::new(move |_| Some(answer.clone()))), ..Services::default() };
    let mut b = bench_with(services, vec2(1300.0, 800.0));
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&first);
    b.app_mut().run_or_alert("file.saveAs", json!({}));
    b.h.step();
    assert_eq!(b.inspect()["saveWaits"].as_array().unwrap().len(), 1);
    assert!(!first.exists() && !second.exists());
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&first, "A1"), SEVEN);
    assert_eq!(in_file(&second, "A1"), SEVEN);
    assert_eq!(b.app().session.active().unwrap().path.as_deref(), Some(second.to_string_lossy().as_ref()));
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

/// A workbook with no file asks where first, then waits; a Save while it waits doesn't ask again.
#[test]
fn a_workbook_with_no_file_asks_where_before_it_waits_and_only_once() {
    let path = temp("asks");
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (counted, answer) = (asked.clone(), path.to_string_lossy().into_owned());
    let services = Services {
        pick_save: Some(Box::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Some(answer.clone())
        })),
        ..Services::default()
    };
    let mut b = bench_with(services, vec2(1300.0, 800.0));
    b.py("A1", "wait 7");
    b.frames(2);
    b.app_mut().run_or_alert("file.save", json!({}));
    b.h.step();
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert!(b.waiting(b.doc()));
    b.app_mut().run_or_alert("file.save", json!({}));
    b.h.step();
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the save waiting has the file");
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// A save that fails keeps the work and says why, also a Save and Close, which then doesn't close.
#[test]
fn a_failed_save_keeps_the_work_and_the_message_and_the_workbook_open() {
    let mut b = bench();
    // A folder that is a file: no one can write there, whoever runs the test.
    let blocker = temp("blocker");
    std::fs::write(&blocker, "a file").unwrap();
    let path = blocker.join("book.xlsx");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.app_mut().run("file.new", json!({})).unwrap();
    let other = b.doc();
    let doc = b.app().session.documents()[0].uid;
    b.go_to(doc);
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    assert_eq!(b.dialog(), Some("saveChanges"));
    b.answer(SaveAnswer::Save);
    assert!(b.waiting(doc));
    b.open_gate();
    b.until_saved();
    assert!(b.app().session.document_index(doc).is_some(), "it stays open");
    assert!(b.app().session.documents().iter().any(|d| d.uid == other));
    assert_eq!(b.doc(), doc);
    assert!(b.app().session.active().unwrap().is_dirty(), "the work is kept");
    assert!(b.message().is_some(), "and the reason shows");
    let _ = std::fs::remove_file(&blocker);
}

/// A draft begun during the wait isn't in the file, so a close after the save would lose it.
#[test]
fn a_draft_begun_during_the_wait_stays_open_and_keeps_the_workbook_open() {
    let mut b = bench();
    let path = temp("draft");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.app_mut().run("file.new", json!({})).unwrap();
    let doc = b.app().session.documents()[0].uid;
    b.go_to(doc);
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    b.answer(SaveAnswer::Save);
    b.app_mut().begin_edit(Some("still typing".into()), false);
    b.h.step();
    b.open_gate();
    b.until_saved();
    assert!(path.exists(), "the save happened");
    assert_eq!(b.doc(), doc, "the workbook stays open");
    assert_eq!(b.app().editor.as_ref().map(|e| e.text.as_str()), Some("still typing"), "and the draft");
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- AutoSave

#[test]
fn autosave_waits_for_the_next_idle_moment_and_never_shows_the_bar() {
    let mut b = bench();
    let path = temp("autosave");
    b.py("A1", "ret 1");
    b.with_file(&path);
    b.app_mut().ui.autosave = true;
    b.py("B1", "wait 7");
    b.app_mut().grid.last_autosave = -1.0e9;
    b.frames(6);
    assert!(!b.shows_bar(), "AutoSave asks nothing");
    assert!(!b.waiting(b.doc()));
    assert!(in_file(&path, "B1") == Value::Empty, "it holds off while the cell runs");
    b.open_gate();
    b.settle();
    b.app_mut().grid.last_autosave = -1.0e9;
    b.until("AutoSave saved", |a| !a.session.active().unwrap().is_dirty());
    assert_eq!(in_file(&path, "B1"), SEVEN);
    assert!(!b.shows_bar());
    let _ = std::fs::remove_file(&path);
}

/// While a person's save waits, AutoSave doesn't save behind it.
#[test]
fn autosave_does_not_save_while_a_save_waits() {
    let mut b = bench();
    let path = temp("autosave-wait");
    b.py("A1", "ret 1");
    b.with_file(&path);
    b.app_mut().ui.autosave = true;
    b.py("B1", "hang");
    b.app_mut().run_or_alert("file.save", json!({}));
    b.app_mut().grid.last_autosave = -1.0e9;
    b.frames(4);
    assert!(b.waiting(b.doc()));
    assert_eq!(code_in_file(&path, "B1"), Json::Null, "nothing saved it yet");
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- closing and quitting

#[test]
fn quit_during_a_wait_asks_and_cancel_returns_to_the_workbook_with_the_save_still_coming() {
    let mut b = bench();
    let path = temp("quit-cancel");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.quit();
    assert_eq!(b.dialog(), Some("saveChanges"));
    b.answer(SaveAnswer::Cancel);
    assert!(!b.app().allow_close, "the quit stopped");
    assert!(b.waiting(b.doc()), "the save still comes");
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "A1"), SEVEN);
    assert!(!b.app().allow_close);
    assert_eq!(b.titles().len(), 1, "the workbook stays open");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn quit_during_a_wait_with_save_quits_once_it_has_saved() {
    let mut b = bench();
    let path = temp("quit-save");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.quit();
    b.answer(SaveAnswer::Save);
    b.frames(2);
    assert!(!b.app().allow_close, "it saves first");
    // Asking again doesn't ask again: it has been answered for.
    b.quit();
    assert_eq!(b.dialog(), None);
    assert!(!b.app().allow_close);
    b.open_gate();
    b.until("the app quit", |a| a.allow_close);
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn quit_during_a_wait_with_dont_save_goes_on_with_no_save() {
    let mut b = bench();
    let path = temp("quit-dont");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.quit();
    b.answer(SaveAnswer::DontSave);
    b.until("the app quit", |a| a.allow_close);
    assert!(!path.exists());
    assert!(!b.app().save_waits.any());
}

/// A save that fails during a quit stops the quit: the work isn't left behind.
#[test]
fn a_failed_save_during_a_quit_stops_the_quit() {
    let mut b = bench();
    let blocker = temp("quit-blocker");
    std::fs::write(&blocker, "a file").unwrap();
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&blocker.join("book.xlsx"));
    b.quit();
    b.answer(SaveAnswer::Save);
    b.open_gate();
    b.until_saved();
    assert!(!b.app().allow_close, "the quit stopped");
    assert!(b.message().is_some());
    assert_eq!(b.titles().len(), 1);
    let _ = std::fs::remove_file(&blocker);
}

/// The cap during a Save and Close: the message would go with the workbook's window, and the cells'
/// results with its worker, so it stays open and says so. A quit stops the same way.
#[test]
fn a_close_or_a_quit_stops_at_the_cap_and_says_which_cells_were_not_finished() {
    for quitting in [false, true] {
        let mut b = bench();
        let path = temp(if quitting { "cap-quit" } else { "cap-close" });
        b.py("A1", "hang");
        b.frames(2);
        b.save_to(&path);
        if quitting {
            b.quit();
        } else {
            b.app_mut().run_or_alert("file.close", json!({}));
            b.h.step();
        }
        b.answer(SaveAnswer::Save);
        let doc = b.doc();
        b.app_mut().save_waits.set_cap(doc, Duration::ZERO);
        b.until_saved();
        assert!(path.exists(), "it saved");
        assert_eq!(b.doc(), doc, "it stays open");
        assert!(!b.app().allow_close, "the quit stopped");
        assert!(b.message().is_some_and(|m| m.contains("Sheet1!A1")), "{:?}", b.message());
        let _ = std::fs::remove_file(&path);
    }
}

/// Save Now says not to wait for them: the close and the quit go on.
#[test]
fn save_now_during_a_close_or_a_quit_goes_on_with_it() {
    let mut b = bench();
    let path = temp("now-quit");
    b.py("A1", "hang");
    b.frames(2);
    b.save_to(&path);
    b.quit();
    b.answer(SaveAnswer::Save);
    b.frames(1);
    b.h.get_by_label(SAVE_NOW).click();
    b.h.step();
    b.until("the app quit", |a| a.allow_close);
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
}

/// A workbook with no unsaved changes and a save waiting asks too.
#[test]
fn closing_a_clean_workbook_whose_save_waits_asks_as_for_unsaved_work() {
    let mut b = bench();
    let path = temp("clean");
    b.py("A1", "hang");
    b.frames(2);
    b.with_file_while_running(&path);
    assert!(!b.app().session.active().unwrap().is_dirty());
    b.app_mut().run_or_alert("file.save", json!({}));
    assert!(b.waiting(b.doc()));
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    assert_eq!(b.dialog(), Some("saveChanges"), "a close asks");
    b.answer(SaveAnswer::Cancel);
    assert_eq!(b.titles().len(), 1);
    assert!(b.waiting(b.doc()));
    b.app_mut().run_or_alert("window.close", json!({}));
    b.h.step();
    assert_eq!(b.dialog(), Some("saveChanges"), "and the window's close button");
    b.answer(SaveAnswer::Cancel);
    b.quit();
    assert_eq!(b.dialog(), Some("saveChanges"), "and a quit");
    b.answer(SaveAnswer::Cancel);
    assert!(!b.app().allow_close);
    // A force asks nothing.
    b.app_mut().run_or_alert("file.close", json!({"force": true, "document": true}));
    b.frames(2);
    assert!(!b.app().save_waits.any(), "the wait went with the workbook");
    let _ = std::fs::remove_file(&path);
}

/// The window's close button, on a clean workbook with a save waiting: the window stays and it asks.
#[test]
fn the_window_close_button_during_a_wait_asks() {
    let mut b = bench();
    let path = temp("close-button");
    b.py("A1", "hang");
    b.frames(2);
    b.with_file_while_running(&path);
    b.app_mut().run_or_alert("file.save", json!({}));
    b.h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    let mut sent = Vec::new();
    for _ in 0..3 {
        b.h.step();
        sent.extend(b.h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    }
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    assert_eq!(b.dialog(), Some("saveChanges"));
    b.answer(SaveAnswer::Cancel);
    assert!(!b.app().allow_close);
    assert!(b.waiting(b.doc()));
    let _ = std::fs::remove_file(&path);
}

/// One of several windows on a workbook closes without asking; its save goes on in another.
#[test]
fn closing_one_of_a_workbooks_windows_leaves_its_wait() {
    let mut b = bench();
    let path = temp("windows");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.app_mut().run("view.newWindow", json!({})).unwrap();
    b.h.step();
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    assert_eq!(b.dialog(), None);
    assert!(b.waiting(b.doc()));
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// With nothing to wait for, the same order holds.
#[test]
fn after_save_and_close_with_nothing_running_the_previously_active_workbook_is_active_again() {
    let mut b = bench();
    let path = temp("three-plain");
    let a = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let middle = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    b.go_to(a);
    b.go_to(middle);
    b.app_mut().run("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    b.with_file(&path);
    b.app_mut().run("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    b.answer(SaveAnswer::Save);
    assert!(b.app().session.document_index(middle).is_none());
    assert_eq!(b.doc(), a);
    let _ = std::fs::remove_file(&path);
}

/// Three workbooks, the person came from A to B, and C is the neighbour and the last opened: Save
/// and Close on B goes back to A.
#[test]
fn after_save_and_close_the_previously_active_workbook_is_active_again() {
    let mut b = bench();
    let path = temp("three");
    let a = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let middle = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let c = b.doc();
    b.go_to(a);
    b.go_to(middle);
    b.py("A1", "wait 7");
    b.with_file_while_running(&path);
    let title_a = b.app().session.documents()[0].display_title();
    b.app_mut().run_or_alert("file.save", json!({}));
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    b.answer(SaveAnswer::Save);
    b.frames(3);
    assert_eq!(b.doc(), middle, "still open while the cell runs");
    b.open_gate();
    b.until_saved();
    assert!(b.app().session.document_index(middle).is_none(), "closed once saved");
    assert_eq!(b.doc(), a, "back where the person came from, not {c}");
    assert_eq!(b.active_title(), title_a);
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// The person went on to C during the wait: B saves and closes behind them, and they stay on C.
#[test]
fn a_save_and_close_behind_the_person_leaves_them_where_they_are() {
    let mut b = bench();
    let path = temp("behind");
    let a = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let middle = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let c = b.doc();
    b.go_to(a);
    b.go_to(middle);
    b.py("A1", "wait 7");
    b.with_file_while_running(&path);
    b.app_mut().run_or_alert("file.save", json!({}));
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    b.answer(SaveAnswer::Save);
    b.go_to(c);
    b.frames(2);
    assert!(!b.shows_bar(), "the bar is the waiting workbook's");
    b.open_gate();
    b.until_saved();
    assert!(b.app().session.document_index(middle).is_none());
    assert_eq!(b.doc(), c, "they stay where they are");
    // And the order they came through is what it was.
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    assert_eq!(b.doc(), a);
    let _ = std::fs::remove_file(&path);
}

/// A save behind the person doesn't count as coming back to its workbook: closing C goes to A, the
/// workbook they were on before C, and not to B, which only saved.
#[test]
fn a_save_behind_the_person_does_not_change_the_order_they_came_through() {
    let mut b = bench();
    let path = temp("order");
    let a = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let middle = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let c = b.doc();
    b.go_to(middle);
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.go_to(a);
    b.go_to(c);
    b.open_gate();
    b.until_saved();
    assert!(path.exists());
    assert_eq!(b.doc(), c);
    b.app_mut().run_or_alert("file.close", json!({}));
    b.h.step();
    assert_eq!(b.doc(), a);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_workbook_closed_while_waiting_cancels_the_wait_and_nothing_is_written() {
    let mut b = bench();
    let path = temp("closed");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    let reply = b.ask("file.save", json!({"path": temp("closed-2"), "wait": true}));
    let doc = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    let other = b.doc();
    b.go_to(doc);
    b.app_mut().run("file.close", json!({"force": true, "document": true})).unwrap();
    b.frames(2);
    assert!(!b.app().save_waits.any());
    assert_eq!(b.doc(), other);
    b.open_gate();
    b.frames(3);
    assert!(!path.exists(), "never saved into a closed document");
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(answer["error"].as_str().unwrap().contains("closed"), "{answer}");
}

// ---------------------------------------------------------------- other workbooks

#[test]
fn other_workbooks_keep_working_and_each_has_its_own_wait() {
    let mut b = bench();
    let (first, second) = (temp("one"), temp("two"));
    b.py("A1", "wait 7");
    let waiting = b.doc();
    b.frames(2);
    b.save_to(&first);
    b.frames(1);
    assert!(b.shows_bar());
    b.app_mut().run("file.new", json!({})).unwrap();
    b.frames(2);
    assert!(!b.shows_bar(), "not on this workbook");
    b.app_mut().run("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
    b.save_to(&second);
    assert!(second.exists(), "this one saves at once");
    assert_eq!(in_file(&second, "A1"), Value::Number(42.0));
    b.go_to(waiting);
    b.frames(1);
    assert!(b.shows_bar(), "back on the workbook that waits");
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&first, "A1"), SEVEN);
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

// ---------------------------------------------------------------- the control channel

#[test]
fn a_save_with_no_file_is_an_error_and_never_a_dialog() {
    let asked = Arc::new(AtomicBool::new(false));
    let counted = asked.clone();
    let services = Services {
        pick_save: Some(Box::new(move |_| {
            counted.store(true, Ordering::SeqCst);
            None
        })),
        ..Services::default()
    };
    let mut b = bench_with(services, vec2(1300.0, 800.0));
    for (method, params) in [
        ("engine.execute", json!({"command": "file.save", "params": {}})),
        ("engine.execute", json!({"command": "file.saveAs", "params": {}})),
        ("engine.execute", json!({"command": "file.save", "params": {"wait": true}})),
        ("file.save", json!({})),
        ("file.saveAs", json!({})),
        ("app.save", json!({})),
        ("app.save", json!({"wait": true})),
    ] {
        let reply = b.ask(method, params.clone());
        let answer = reply.try_recv().unwrap_or_else(|_| panic!("{method} {params} holds its reply"));
        assert_eq!(answer["ok"], false, "{method} {params}: {answer}");
        assert!(answer["error"].as_str().unwrap().contains("never open dialogs"), "{answer}");
        assert_eq!(b.dialog(), None, "{method} {params}");
    }
    assert!(!asked.load(Ordering::SeqCst), "no file picker");
    // A workbook with a file saves there, Save As still wants the new name.
    let path = temp("control-file");
    b.with_file(&path);
    let answer = b.ask("file.saveAs", json!({})).try_recv().unwrap();
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(b.ask("file.save", json!({})).try_recv().unwrap()["ok"], true);
    assert!(!asked.load(Ordering::SeqCst));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn without_wait_a_save_happens_at_once_and_says_what_was_not_finished() {
    let mut b = bench();
    let path = temp("control-now");
    b.py("A1", "wait 7");
    b.frames(2);
    let answer = b.ask("engine.execute", json!({"command": "file.save", "params": {"path": path}})).try_recv().unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(path.exists());
    assert_eq!(answer["result"]["unfinished"], json!(["Sheet1!A1"]), "{answer}");
    assert!(!b.app().save_waits.any());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn with_wait_the_reply_holds_until_the_save_and_no_bar_shows() {
    let mut b = bench();
    let path = temp("control-wait");
    b.py("A1", "wait 7");
    b.frames(2);
    let reply = b.ask("engine.execute", json!({"command": "file.save", "params": {"path": path, "wait": true}}));
    b.frames(3);
    assert!(reply.try_recv().is_err(), "the reply waits for the save");
    assert!(!path.exists());
    assert!(!b.shows_bar(), "a programmatic call shows nothing");
    assert_eq!(b.dialog(), None);
    let seen = b.inspect();
    assert_eq!(seen["python"]["busy"], true, "{seen}");
    assert_eq!(seen["python"]["unfinished"], json!(["Sheet1!A1"]), "{seen}");
    assert_eq!(seen["saveWaits"][0]["prompt"], false, "{seen}");
    assert_eq!(seen["saveWaits"][0]["unfinished"], json!(["Sheet1!A1"]), "{seen}");
    b.open_gate();
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!([]), "{answer}");
    assert_eq!(answer["result"]["path"], json!(path), "{answer}");
    assert_eq!(in_file(&path, "A1"), SEVEN);
    assert_eq!(b.inspect()["saveWaits"], json!([]));
    assert_eq!(b.inspect()["python"]["busy"], false);
    let _ = std::fs::remove_file(&path);
}

/// `wait` may be in the request or in the save's params; `app.save` and the command called by its id
/// take it too; with nothing running the reply comes at once.
#[test]
fn every_way_of_asking_for_a_save_takes_wait() {
    let mut b = bench();
    let path = temp("control-ways");
    b.py("A1", "ret 1");
    b.settle();
    for (method, params) in [
        ("engine.execute", json!({"command": "file.save", "wait": true, "params": {"path": path}})),
        ("engine.execute", json!({"command": "file.saveAs", "params": {"path": path, "waitSeconds": 5}})),
        ("file.save", json!({"path": path, "wait": true})),
        ("file.saveAs", json!({"path": path, "wait": true})),
        ("app.save", json!({"path": path, "wait": true})),
    ] {
        let _ = std::fs::remove_file(&path);
        let answer = b.ask(method, params.clone()).try_recv().unwrap_or_else(|_| panic!("{method} {params} held its reply"));
        assert_eq!(answer["ok"], true, "{method} {params}: {answer}");
        assert_eq!(answer["result"]["unfinished"], json!([]), "{method} {params}: {answer}");
        assert!(path.exists(), "{method} {params}");
    }
    // And each holds while something runs.
    for (cell, method, params) in [
        ("B1", "engine.execute", json!({"command": "file.save", "wait": true, "params": {}})),
        ("B2", "file.save", json!({"waitSeconds": 60})),
        ("B3", "app.save", json!({"wait": true})),
    ] {
        b.gate.store(false, Ordering::SeqCst);
        b.py(cell, "wait 7");
        b.frames(1);
        let reply = b.ask(method, params.clone());
        assert!(reply.try_recv().is_err(), "{method} {params}");
        b.open_gate();
        let answer = answered(&reply, &mut b);
        assert_eq!(answer["ok"], true, "{method} {params}: {answer}");
        assert_eq!(in_file(&path, cell), SEVEN);
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn wait_seconds_bounds_the_hold_and_the_reply_names_the_cells() {
    let mut b = bench();
    let path = temp("control-bound");
    b.py("A1", "hang");
    b.frames(2);
    let reply = b.ask("file.save", json!({"path": path, "waitSeconds": 0.05}));
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!(["Sheet1!A1"]), "{answer}");
    assert_eq!(answer["result"]["unfinishedCount"], 1, "{answer}");
    assert!(path.exists());
    assert_eq!(b.message(), None, "the reply says it; nothing shows");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_bad_wait_is_refused_and_nothing_is_saved() {
    let mut b = bench();
    let path = temp("control-bad");
    b.py("A1", "hang");
    b.frames(2);
    for params in [
        json!({"command": "file.save", "params": {"path": path, "waitSeconds": -1}}),
        json!({"command": "file.save", "params": {"path": path, "waitSeconds": "soon"}}),
        json!({"command": "file.save", "params": {"path": path, "wait": "yes"}}),
        json!({"command": "file.save", "waitSeconds": -1, "params": {"path": path}}),
        json!({"command": "python.runAll", "wait": 1, "params": {}}),
    ] {
        let answer = b.ask("engine.execute", params.clone()).try_recv().unwrap();
        assert_eq!(answer["ok"], false, "{params}: {answer}");
    }
    assert!(!path.exists());
    assert!(!b.app().save_waits.any());
}

/// A second request joins the wait: both replies come, one file is written twice at most, and a
/// shorter bound still holds.
#[test]
fn a_second_request_joins_the_wait_and_its_own_bound_holds() {
    let mut b = bench();
    let (one, two) = (temp("join-1"), temp("join-2"));
    b.py("A1", "hang");
    b.frames(2);
    let first = b.ask("file.save", json!({"path": one, "wait": true}));
    let second = b.ask("file.save", json!({"path": two, "waitSeconds": 0.05}));
    assert_eq!(b.inspect()["saveWaits"].as_array().unwrap().len(), 1);
    let (a, c) = (answered(&second, &mut b), answered(&first, &mut b));
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(c["ok"], true, "{c}");
    assert!(one.exists() && two.exists());
    assert_eq!(a["result"]["unfinished"], json!(["Sheet1!A1"]));
    let _ = std::fs::remove_file(&one);
    let _ = std::fs::remove_file(&two);
}

/// A programmatic save that fails says so in its reply, and shows nothing: no message box either.
#[test]
fn a_failed_programmatic_save_replies_and_shows_nothing() {
    let mut b = bench();
    let blocker = temp("control-blocker");
    std::fs::write(&blocker, "a file").unwrap();
    b.py("A1", "wait 7");
    b.frames(2);
    let reply = b.ask("file.save", json!({"path": blocker.join("book.xlsx"), "wait": true}));
    b.open_gate();
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(b.message(), None);
    assert!(b.app().session.active().unwrap().is_dirty(), "the work is kept");
    let _ = std::fs::remove_file(&blocker);
}

/// Each connection holds one reply at a time, so this only stops many connections piling up: the one
/// too many is told so, and nothing else changes.
#[test]
fn too_many_held_replies_are_refused() {
    let mut b = bench();
    let path = temp("too-many");
    b.py("A1", "hang");
    b.frames(2);
    let held: Vec<_> = (0..64).map(|_| b.ask("file.save", json!({"path": path, "wait": true}))).collect();
    assert!(held.iter().all(|r| r.try_recv().is_err()));
    let answer = b.ask("file.save", json!({"path": path, "wait": true})).try_recv().unwrap();
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(answer["error"].as_str().unwrap().contains("too many"), "{answer}");
    // A command that has run says so, and what isn't finished yet.
    let answer = b.ask("engine.execute", json!({"command": "cell.set", "wait": true, "params": {"cell": "C1", "input": "1"}})).try_recv().unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!(["Sheet1!A1"]), "{answer}");
    b.app_mut().run("ui.saveWait", json!({"action": "saveNow"})).unwrap();
    b.h.step();
    for reply in &held {
        assert_eq!(reply.try_recv().unwrap()["ok"], true);
    }
    let _ = std::fs::remove_file(&path);
}

/// A programmatic save into a person's wait: the person's Cancel tells whoever waits.
#[test]
fn cancel_answers_the_replies_it_holds() {
    let mut b = bench();
    let path = temp("control-cancel");
    b.py("A1", "hang");
    b.frames(2);
    let reply = b.ask("file.save", json!({"path": path, "wait": true}));
    assert!(!b.shows_bar());
    b.save_to(&path);
    b.frames(1);
    assert!(b.shows_bar(), "now a person is waiting too");
    let state = b.ask("engine.execute", json!({"command": "ui.saveWait", "params": {}})).try_recv().unwrap();
    assert_eq!(state["result"]["waiting"], true, "{state}");
    assert_eq!(state["result"]["prompt"], true, "{state}");
    let done = b.ask("engine.execute", json!({"command": "ui.saveWait", "params": {"action": "cancel"}})).try_recv().unwrap();
    assert_eq!(done["result"]["waiting"], false, "{done}");
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(answer["error"].as_str().unwrap().contains("cancelled"), "{answer}");
    assert!(!path.exists());
}

#[test]
fn save_now_and_unknown_actions_through_the_control_channel() {
    let mut b = bench();
    let path = temp("control-now-action");
    b.py("A1", "hang");
    b.frames(2);
    let reply = b.ask("file.save", json!({"path": path, "wait": true}));
    let answer = b.ask("engine.execute", json!({"command": "ui.saveWait", "params": {"action": "later"}})).try_recv().unwrap();
    assert_eq!(answer["ok"], false, "{answer}");
    b.ask("engine.execute", json!({"command": "ui.saveWait", "params": {"action": "saveNow"}})).try_recv().unwrap();
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!(["Sheet1!A1"]), "{answer}");
    assert!(path.exists());
    // With nothing waiting they change nothing.
    for action in ["saveNow", "cancel"] {
        let answer = b.ask("engine.execute", json!({"command": "ui.saveWait", "params": {"action": action}})).try_recv().unwrap();
        assert_eq!(answer["result"]["waiting"], false, "{answer}");
    }
    let listed = b.ask("engine.commands", json!({"search": "ui.saveWait"})).try_recv().unwrap();
    assert_eq!(listed["result"].as_array().unwrap().len(), 1, "{listed}");
    let _ = std::fs::remove_file(&path);
}

/// `wait` on `engine.execute` for any command: the reply holds until Python is idle, and says what
/// the cell holds now.
#[test]
fn engine_execute_with_wait_holds_until_python_is_idle() {
    let mut b = bench();
    let reply = b.ask("engine.execute", json!({"command": "python.set", "wait": true, "params": {"cell": "A1", "code": "wait 7"}}));
    b.frames(2);
    assert!(reply.try_recv().is_err(), "the cell runs");
    assert_eq!(b.inspect()["python"]["busy"], true);
    b.open_gate();
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["value"], json!(7.0), "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!([]), "{answer}");

    // Bounded, with what wasn't finished.
    let answer = {
        let reply = b.ask("engine.execute", json!({"command": "python.set", "waitSeconds": 0.05, "params": {"cell": "A2", "code": "hang"}}));
        answered(&reply, &mut b)
    };
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["unfinished"], json!(["Sheet1!A2"]), "{answer}");

    // A command that fails answers at once, with its error.
    let answer = b.ask("engine.execute", json!({"command": "no.such", "wait": true, "params": {}})).try_recv().unwrap();
    assert_eq!(answer["ok"], false, "{answer}");

    // A workbook closed during the hold: the reply says so.
    let reply = b.ask("engine.execute", json!({"command": "python.runAll", "wait": true, "params": {}}));
    let closing = b.doc();
    b.app_mut().run("file.new", json!({})).unwrap();
    b.app_mut().run("window.close", json!({"doc": closing, "force": true})).unwrap();
    let answer = answered(&reply, &mut b);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["result"]["workbookClosed"], true, "{answer}");
}

/// The control server waits for a held reply as long as it may be held, and no longer for others.
#[test]
fn a_held_reply_gets_the_time_it_may_take() {
    let plain = control::reply_timeout(&json!({"command": "cell.set", "params": {}}));
    let held = control::reply_timeout(&json!({"command": "file.save", "params": {"wait": true}}));
    assert_eq!(plain, Duration::from_secs(60));
    assert_eq!(held, plain + MAX_WAIT);
    assert_eq!(control::reply_timeout(&json!({"wait": true})), held);
    assert_eq!(control::reply_timeout(&json!({"waitSeconds": 5})), held);
}

/// `app.quit` asks nothing, so a save waiting makes it refuse rather than lose the save.
#[test]
fn app_quit_is_refused_while_a_save_waits() {
    let mut b = bench();
    let path = temp("control-quit");
    b.py("A1", "hang");
    b.frames(2);
    let _reply = b.ask("file.save", json!({"path": path, "wait": true}));
    let answer = b.ask("app.quit", json!({})).try_recv().unwrap();
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(!b.app().allow_close);
    b.app_mut().run("ui.saveWait", json!({"action": "cancel"})).unwrap();
    b.frames(1);
    assert_eq!(b.ask("app.quit", json!({})).try_recv().unwrap()["ok"], true);
    assert!(b.app().allow_close);
}

/// `app.run` has nothing to hold a reply with: `wait` doesn't reach the engine, whose call would
/// block the frame.
#[test]
#[allow(clippy::disallowed_methods)] // a native test that times a call
fn a_save_through_run_with_wait_does_not_block() {
    let mut b = bench();
    let path = temp("run-no-block");
    b.py("A1", "hang");
    b.frames(2);
    let started = std::time::Instant::now();
    let r = b.app_mut().run("file.save", json!({"path": path, "wait": true})).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r["unfinished"], json!(["Sheet1!A1"]), "{r}");
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- how it looks

/// A narrow window: the text wraps ahead of the buttons, which stay in the window.
#[test]
fn the_bar_keeps_its_buttons_in_a_narrow_window() {
    let mut b = bench_with(Services::default(), vec2(420.0, 600.0));
    let path = temp("narrow");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.frames(3);
    for rect in [b.h.get_by_label(SAVE_NOW).rect(), b.bar_cancel().rect()] {
        assert!(rect.left() >= 0.0 && rect.right() <= 420.0, "{rect:?}");
    }
    b.open_gate();
    b.until_saved();
    let _ = std::fs::remove_file(&path);
}

/// The File tab replaces the ribbon: the bar shows there too.
#[test]
fn the_bar_shows_on_the_file_page() {
    let mut b = bench();
    let path = temp("backstage");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.app_mut().run("ui.backstage", json!({"page": "info"})).unwrap();
    b.frames(2);
    assert!(b.shows_bar());
    b.open_gate();
    b.until_saved();
    let _ = std::fs::remove_file(&path);
}

/// File › Save As with a name: the page goes back to the sheet, where the bar says what is happening,
/// as it does for a save that happens at once. A Save As called off leaves the page.
#[test]
fn save_as_on_the_file_page_goes_back_to_the_sheet_when_the_save_waits() {
    let path = temp("page");
    let answer = Arc::new(std::sync::Mutex::new(None::<String>));
    let given = answer.clone();
    let services = Services { pick_save: Some(Box::new(move |_| given.lock().unwrap().clone())), ..Services::default() };
    let mut b = bench_with(services, vec2(1300.0, 800.0));
    b.py("A1", "wait 7");
    b.frames(2);
    b.app_mut().run("ui.backstage", json!({"page": "saveAs"})).unwrap();
    b.frames(2);
    b.h.get_by_label("Browse…").click();
    b.frames(2);
    assert!(b.app().backstage.is_some(), "called off");
    assert!(!b.waiting(b.doc()));
    *answer.lock().unwrap() = Some(path.to_string_lossy().into_owned());
    b.h.get_by_label("Browse…").click();
    b.frames(2);
    assert!(b.waiting(b.doc()));
    assert!(b.app().backstage.is_none());
    assert!(b.shows_bar());
    b.open_gate();
    b.until_saved();
    assert_eq!(in_file(&path, "A1"), SEVEN);
    let _ = std::fs::remove_file(&path);
}

/// The bar in every language, with Save Now translated: the strings are in the catalogs.
#[test]
fn the_new_words_are_in_every_catalog() {
    use gridcraft_ui_egui::i18n::Language;
    use gridcraft_ui_egui::save_wait::{AND_MORE, UNFINISHED};
    for language in Language::ALL.iter().filter(|l| **l != Language::En) {
        for key in [WAITING, SAVE_NOW, UNFINISHED, AND_MORE] {
            assert_ne!(language.tr(key), key, "{} shows {key:?} in English", language.code());
        }
        assert!(language.tr(UNFINISHED).contains("{cells}"));
        assert!(language.tr(AND_MORE).contains("{cells}") && language.tr(AND_MORE).contains("{count}"));
    }
}

/// Run with `GRIDCRAFT_SAVE_WAIT_SHOT=path.png` to look at the bar.
#[test]
fn a_picture_of_the_bar_when_asked_for() {
    let Ok(shot) = std::env::var("GRIDCRAFT_SAVE_WAIT_SHOT") else { return };
    let mut b = bench();
    let path = temp("shot");
    b.py("A1", "wait 7");
    b.frames(2);
    b.save_to(&path);
    b.frames(3);
    b.h.render().unwrap().save(&shot).unwrap();
    b.open_gate();
    b.until_saved();
    let _ = std::fs::remove_file(&path);
}
