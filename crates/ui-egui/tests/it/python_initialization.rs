//! Initialization: the buttons, the editor window on the workbook's code, its ways out, and what says
//! the code failed. A fake worker and a given report stand in for Python.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::{Array, Value};
use gridcraft_engine::model::PyEnvironment;
use gridcraft_engine::model::python::{MAX_INIT_CHARS, MAX_INIT_CHARS_KEPT};
use gridcraft_engine::python::{Interpreter, InterpreterKind, Version};
use gridcraft_engine::{PythonEnvironment, PythonState, Session};
use gridcraft_python::fake::{FakeHandle, FakeTransport};
use gridcraft_python::protocol::{Output, PyError, Request};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::python_ui::{self, PROBLEMS, RUN_LOG};
use gridcraft_ui_egui::python_window::{self, APPLY, ASK_INIT, CANCEL, CLOSE, DISCARD, INITIALIZATION, Pending, PyWindow};
use serde_json::{Value as Json, json};

use crate::common::Harness;

const FAILED: &str = "The initialization code failed, so Python cells can't run.";
const OPEN: &str = "Open Initialization";

/// Code that says `fail` fails on its second line; `skips` reports a package it went on without.
fn worker() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let Request::Run { id, code, init, .. } = req else { continue };
            let result = if code.contains("fail") {
                Err(PyError { message: "NameError: name 'nope' is not defined".into(), line: Some(2), ..PyError::default() })
            } else if init.is_some() && code.contains("skips") {
                let rows = Array::new(1, 2, vec![Value::text("scipy"), Value::Number(3.0)]).map(Value::from).unwrap_or(Value::Empty);
                Ok(Output::Value(rows))
            } else {
                Ok(Output::Value(Value::Number(1.0)))
            };
            if !w.done(id, result) {
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

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn app_with(session: Session) -> (SheetApp, FakeHandle) {
    let t = worker();
    let handle = t.handle();
    let mut app = crate::common::prepare(SheetApp::new(session, gridcraft_ui_egui::Services::default()));
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    (app, handle)
}

fn sized(session: Session, w: f32, h: f32) -> (Harness, FakeHandle) {
    let (app, handle) = app_with(session);
    (crate::common::harness_with(crate::common::builder().with_size(vec2(w, h)), app, 3), handle)
}

fn harness(session: Session) -> Harness {
    sized(session, 1300.0, 800.0).0
}

/// A workbook with a Python cell in A1, selected.
fn with_a_cell() -> Session {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    s
}

fn settle(h: &mut Harness) {
    for _ in 0..1000 {
        h.step();
        if !h.state().session.python_busy() {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Python never settled");
}

fn run(h: &mut Harness, id: &str, p: Json) -> Result<Json, String> {
    let r = h.state_mut().run(id, p);
    h.run_steps(3);
    r
}

fn win(h: &Harness) -> Option<PyWindow> {
    h.state().py_window.clone()
}

fn text(h: &Harness) -> String {
    win(h).map(|w| w.text).unwrap_or_default()
}

fn stored(h: &Harness) -> String {
    let d = h.state().session.active();
    d.and_then(|d| d.wb.python_env.as_ref()).map(|e| e.code.clone()).unwrap_or_default()
}

fn send(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn key(h: &mut Harness, k: Key, modifiers: Modifiers) {
    let press = |pressed| Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers };
    send(h, [press(true), press(false)]);
}

fn type_text(h: &mut Harness, t: &str) {
    send(h, [Event::Text(t.into())]);
}

fn click(h: &mut Harness, label: &str) {
    let area = win(h).and_then(|w| w.rect).expect("the window is on screen");
    let node = h.query_all_by_label(label).find(|n| area.contains(n.rect().center()));
    let Some(node) = node else { panic!("no {label:?} in the window") };
    node.click();
    h.run_steps(3);
}

/// Opens the window on the workbook's code, with the caret at the end of it.
fn open(h: &mut Harness) {
    run(h, "ui.pythonInitialization", json!({"on": true})).unwrap();
    if let Some(w) = h.state_mut().py_window.as_mut() {
        w.caret = Some(w.text.chars().count());
    }
    h.run_steps(3);
}

fn from_file(s: &mut Session, code: &str) {
    let d = s.active_mut().unwrap();
    Arc::make_mut(&mut d.wb).python_env = Some(Arc::new(PyEnvironment { code: code.into(), ..PyEnvironment::default() }));
}

// ---------------------------------------------------------------- the ways in

#[test]
fn the_python_tabs_button_opens_it() {
    let mut h = harness(with_a_cell());
    h.state_mut().ui.ribbon_tab = python_ui_tab();
    h.run_steps(3);
    assert!(win(&h).is_none());
    h.get_by_role_and_label(Role::Button, INITIALIZATION).click();
    h.run_steps(3);
    let w = win(&h).expect("the window opened");
    assert!(w.initialization);
    assert_eq!(w.title(), INITIALIZATION);
    assert!(h.query_all_by_label(INITIALIZATION).count() >= 1, "it is titled Initialization");
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    h.check_accessibility();
    // With the window open, the Python tab stays up.
    assert!(python_ui::tab_shows(h.state()));
}

/// The name of the Python contextual tab, as the ribbon knows it.
fn python_ui_tab() -> String {
    "Python".to_string()
}

#[test]
fn formulas_python_has_it_with_a_label_in_a_wide_window_and_its_icon_in_a_narrow_one() {
    let (mut h, _) = sized(blank(), 1700.0, 800.0);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, INITIALIZATION).click();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.initialization));
    run(&mut h, "ui.pythonInitialization", json!({"on": false})).unwrap();
    let (mut h, _) = sized(blank(), 1200.0, 800.0);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, INITIALIZATION).is_some(), "no room for its label, and it keeps its name");
    // Its key tip reaches it there too.
    press_sequence(&mut h, &[(Key::M, "m"), (Key::Z, "z"), (Key::I, "i")]);
    assert!(win(&h).is_some_and(|w| w.initialization), "M, Z, I opens it");
    assert_eq!(h.state().keytips.prefix(), None);
    assert_eq!(text(&h), "");
}

fn press_sequence(h: &mut Harness, keys: &[(Key, &str)]) {
    key(h, Key::F10, Modifiers::NONE);
    for (k, c) in keys {
        let event = |pressed| Event::Key { key: *k, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        h.input_mut().events.push(event(true));
        h.input_mut().events.push(Event::Text((*c).into()));
        h.input_mut().events.push(event(false));
        h.run_steps(2);
    }
    h.run_steps(2);
}

#[test]
fn its_python_tab_key_tip_is_y_t() {
    let mut h = harness(with_a_cell());
    h.state_mut().ui.ribbon_tab = python_ui_tab();
    h.run_steps(3);
    press_sequence(&mut h, &[(Key::Y, "y"), (Key::T, "t")]);
    assert!(win(&h).is_some_and(|w| w.initialization));
    assert_eq!(text(&h), "", "the key tip's letter isn't typed into the code");
}

#[test]
fn the_control_channel_reaches_it_and_never_gets_a_question() {
    let mut h = harness(with_a_cell());
    h.state_mut().run("python.initialization", json!({"code": "x = 1\ny = 2"})).unwrap();
    // The state says what it is, on the reply and in ui.inspect.
    let r = run(&mut h, "ui.pythonInitialization", json!({})).unwrap();
    assert_eq!((r["open"].as_bool(), r["initialization"].as_bool(), r["title"].as_str()), (Some(true), Some(true), Some(INITIALIZATION)));
    assert_eq!((r["sheet"].clone(), r["cell"].clone(), r["text"].as_str()), (Json::Null, Json::Null, Some("x = 1\ny = 2")));
    let inspect = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspect["pythonEditor"]["initialization"], true);
    assert_eq!(inspect["pythonEditor"]["text"], "x = 1\ny = 2");
    assert_eq!(inspect["python"]["initialization"]["userModified"], true);
    assert_eq!(inspect["python"]["initialization"]["chars"], 11);
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    // Without `on` it closes what is open.
    assert_eq!(run(&mut h, "ui.pythonInitialization", json!({})).unwrap()["open"], false);
    assert!(win(&h).is_none());
    // Unsaved code: refused, not asked; `force` discards it.
    open(&mut h);
    type_text(&mut h, "\nz = 3");
    let e = run(&mut h, "ui.pythonInitialization", json!({"on": false})).unwrap_err();
    assert!(e.contains("force") && e.contains(INITIALIZATION), "{e}");
    assert!(win(&h).is_some_and(|w| w.pending.is_none()), "no question");
    let e = run(&mut h, "ui.pythonEditor", json!({"on": true, "cell": "A1"})).unwrap_err();
    assert!(e.contains("force"), "{e}");
    assert!(win(&h).is_some_and(|w| w.initialization));
    assert_eq!(run(&mut h, "ui.pythonInitialization", json!({"on": false, "force": true})).unwrap()["open"], false);
    assert_eq!(stored(&h), "x = 1\ny = 2");
    // A window on a cell isn't its to close, and hostile parameters are errors.
    run(&mut h, "ui.pythonEditor", json!({"on": true, "cell": "A1"})).unwrap();
    assert_eq!(run(&mut h, "ui.pythonInitialization", json!({"on": false})).unwrap()["cell"], "A1");
    for bad in [json!({"on": "yes"}), json!({"on": 1})] {
        assert!(run(&mut h, "ui.pythonInitialization", bad).is_err());
    }
    run(&mut h, "ui.pythonEditor", json!({"on": false})).unwrap();
    for odd in [json!(null), json!([]), json!({"force": "x"})] {
        let _ = run(&mut h, "ui.pythonInitialization", odd);
        let _ = run(&mut h, "ui.pythonInitialization", json!({"on": false, "force": true}));
    }
}

// ---------------------------------------------------------------- the editor

#[test]
fn it_applies_through_the_command_as_one_undo_step() {
    let mut h = harness(with_a_cell());
    settle(&mut h);
    open(&mut h);
    let undo = h.state().session.active().map(|d| d.undo.len());
    type_text(&mut h, "x = 1");
    assert_eq!(text(&h), "x = 1");
    assert!(win(&h).is_some_and(|w| w.dirty()));
    assert_eq!(stored(&h), "", "typing changes nothing yet");
    click(&mut h, APPLY);
    assert_eq!(stored(&h), "x = 1");
    assert!(win(&h).is_some_and(|w| !w.dirty()));
    assert_eq!(h.state().session.active().map(|d| d.undo.len()), undo.map(|n| n + 1));
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "python.initialization"));
    let env = h.state().session.active().and_then(|d| d.wb.python_env.clone()).unwrap();
    assert!(env.user_modified, "a person gave it");
    settle(&mut h);
    // The keys, too, with the window staying open.
    type_text(&mut h, "\ny = 2");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(stored(&h), "x = 1\ny = 2");
    assert!(win(&h).is_some());
    // An undo elsewhere comes into a window with nothing of its own.
    run(&mut h, "edit.undo", json!({})).unwrap();
    assert_eq!(text(&h), "x = 1", "it follows the workbook's code");
    assert!(win(&h).is_some_and(|w| !w.dirty()));
    // Esc closes it when there is nothing to lose.
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(win(&h).is_none());
}

#[test]
fn every_way_out_asks_first() {
    let mut h = harness(with_a_cell());
    open(&mut h);
    type_text(&mut h, "x = 1");
    // Close, Esc: the question is about the code, with its three answers.
    click(&mut h, CLOSE);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Close));
    assert!(h.query_by_label(ASK_INIT).is_some(), "it says what would be lost");
    click(&mut h, CANCEL);
    assert_eq!(text(&h), "x = 1");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Close));
    click(&mut h, DISCARD);
    assert!(win(&h).is_none());
    assert_eq!(stored(&h), "");
    // Apply at the question keeps it.
    open(&mut h);
    type_text(&mut h, "x = 2");
    click(&mut h, CLOSE);
    click(&mut h, APPLY);
    assert!(win(&h).is_none());
    assert_eq!(stored(&h), "x = 2");
    // Another way in, on a cell.
    open(&mut h);
    type_text(&mut h, "\ny = 3");
    h.state_mut().session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    h.run_steps(2);
    python_window::open_from_menu(h.state_mut());
    h.run_steps(3);
    assert!(matches!(win(&h).and_then(|w| w.pending), Some(Pending::Open { .. })), "it asks before the cell's code replaces it");
    assert!(win(&h).is_some_and(|w| w.initialization));
    click(&mut h, DISCARD);
    assert!(win(&h).is_some_and(|w| !w.initialization && w.cell.a1() == "A1"), "then it opens on the cell");
    // And from a cell's unsaved code, the Initialization button asks too.
    type_text(&mut h, "more");
    python_window::open_initialization(h.state_mut());
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| !w.initialization && matches!(w.pending, Some(Pending::Open { .. }))));
    click(&mut h, DISCARD);
    assert!(win(&h).is_some_and(|w| w.initialization));
    assert_eq!(text(&h), "x = 2");
}

#[test]
fn closing_the_workbook_and_switching_away_ask_first() {
    let mut s = with_a_cell();
    s.new_workbook();
    s.set_active(0);
    let mut h = harness(s);
    open(&mut h);
    type_text(&mut h, "x = 1");
    let first = h.state().session.active().map(|d| d.uid);
    // File › Close.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    assert!(matches!(win(&h).and_then(|w| w.pending), Some(Pending::CloseWorkbook(_))));
    click(&mut h, CANCEL);
    assert_eq!(h.state().session.active().map(|d| d.uid), first);
    // Another workbook.
    h.state_mut().session.set_active(1);
    h.run_steps(3);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Switched));
    assert!(h.query_all_by_label(INITIALIZATION).count() >= 1);
    click(&mut h, CANCEL);
    assert_eq!(h.state().session.active().map(|d| d.uid), first, "back to its workbook");
    assert_eq!(text(&h), "x = 1");
    // Without changes it goes quietly with the switch.
    run(&mut h, "ui.pythonInitialization", json!({"on": false, "force": true})).unwrap();
    open(&mut h);
    h.state_mut().session.set_active(1);
    h.run_steps(3);
    assert!(win(&h).is_none());
    // A programmatic quit is refused.
    h.state_mut().session.set_active(0);
    h.run_steps(2);
    open(&mut h);
    type_text(&mut h, "y");
    let refused = python_window::quit_refused(h.state()).unwrap();
    assert!(refused.contains(INITIALIZATION), "{refused}");
}

#[test]
fn code_changed_elsewhere_isnt_overwritten_by_apply() {
    let mut h = harness(with_a_cell());
    open(&mut h);
    type_text(&mut h, "mine = 1");
    h.state_mut().run("python.initialization", json!({"code": "theirs = 1"})).unwrap();
    h.run_steps(3);
    assert_eq!(text(&h), "mine = 1", "what was typed stays");
    assert!(h.query_by_label(python_window::CHANGED_INIT).is_some(), "it says so");
    click(&mut h, APPLY);
    assert_eq!(stored(&h), "theirs = 1", "not applied");
    assert!(h.query_by_label(python_window::STALE_INIT).is_some());
    assert_eq!(text(&h), "mine = 1");
    // Typed to match, it's the same code, and the window takes that in.
    if let Some(w) = h.state_mut().py_window.as_mut() {
        w.text = "theirs = 1".into();
        w.notice = None;
    }
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| !w.dirty() && w.saved == "theirs = 1"));
}

#[test]
fn a_protected_workbook_refuses_apply_and_the_code_stays() {
    let mut h = harness(with_a_cell());
    h.state_mut().run("python.initialization", json!({"code": "x = 1"})).unwrap();
    h.state_mut().run("review.protectWorkbook", json!({})).unwrap();
    open(&mut h);
    assert_eq!(text(&h), "x = 1", "it can still be read");
    type_text(&mut h, "\ny = 2");
    click(&mut h, APPLY);
    assert_eq!(stored(&h), "x = 1");
    assert_eq!(text(&h), "x = 1\ny = 2", "nothing typed is lost");
    let notice = python_window::state(h.state())["notice"].as_str().unwrap().to_string();
    assert!(notice.contains("protected"), "{notice}");
    assert!(h.query_by_label(notice.as_str()).is_some(), "it shows under the code");
}

#[test]
fn the_limit_holds_while_typing_and_code_past_it_can_still_shrink() {
    let mut h = harness(blank());
    let at = "x".repeat(MAX_INIT_CHARS);
    h.state_mut().run("python.initialization", json!({"code": at})).unwrap();
    open(&mut h);
    type_text(&mut h, "y");
    assert_eq!(text(&h).chars().count(), MAX_INIT_CHARS, "refused");
    let notice = python_window::state(h.state())["notice"].as_str().unwrap().to_string();
    assert!(notice.contains("65536"), "{notice}");
    assert!(h.query_by_label(format!("{MAX_INIT_CHARS} / {MAX_INIT_CHARS}").as_str()).is_some(), "the count, in characters");
    // A paste is held to it as well.
    run(&mut h, "ui.pythonInitialization", json!({"on": false, "force": true})).unwrap();
    h.state_mut().run("python.initialization", json!({"code": "ok"})).unwrap();
    open(&mut h);
    send(&mut h, [Event::Paste("z".repeat(MAX_INIT_CHARS))]);
    assert_eq!(text(&h), "ok", "a paste past the limit is refused whole");
    run(&mut h, "ui.pythonInitialization", json!({"on": false, "force": true})).unwrap();
    // Code a file cut, past the limit: it opens, says it can't run, and only shrinks.
    let mut s = with_a_cell();
    from_file(&mut s, &"x".repeat(MAX_INIT_CHARS_KEPT));
    let mut h = harness(s);
    settle(&mut h);
    open(&mut h);
    assert_eq!(text(&h).chars().count(), MAX_INIT_CHARS_KEPT);
    type_text(&mut h, "y");
    assert_eq!(text(&h).chars().count(), MAX_INIT_CHARS_KEPT, "not longer");
    assert_eq!(python_ui_checks(&h), ["init.failed"]);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(text(&h).chars().count(), MAX_INIT_CHARS, "shorter is fine");
    click(&mut h, APPLY);
    settle(&mut h);
    assert_eq!(h.state().session.python_init_failure(), None);
    assert!(h.state().session.active().and_then(|d| d.wb.python_env.clone()).is_some_and(|e| e.fits() && e.user_modified));
}

/// The kinds the window's problems strip lists.
fn python_ui_checks(h: &Harness) -> Vec<&'static str> {
    h.state().python.window_checks.entries.iter().map(|e| e.kind.id).collect()
}

#[test]
fn the_checks_and_the_failure_are_in_the_problems_strip() {
    let mut h = harness(with_a_cell());
    settle(&mut h);
    open(&mut h);
    type_text(&mut h, "a = 1\nfail\nimport socket");
    click(&mut h, APPLY);
    settle(&mut h);
    assert_eq!(h.state().session.python_init_failure().map(|d| d.line), Some(Some(2)));
    // The failure, on its line, with what the code's checks find.
    let checks = &h.state().python.window_checks.entries;
    assert_eq!(checks.iter().map(|e| e.kind.id).collect::<Vec<_>>(), ["init.failed", "lint.importBlocked"]);
    assert_eq!(checks[0].line, Some(2));
    assert_eq!(checks[0].char_at, Some(6), "a click goes to its line");
    assert!(h.query_by_label_contains("Line 2:").is_some(), "the strip says so");
    // ui.inspect has them.
    let inspect = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspect["python"]["findings"]["editor"][0]["kind"], "init.failed");
    assert_eq!(inspect["python"]["initialization"]["failed"]["line"], 2);
    // Edited, its line no longer points at anything.
    key(&mut h, Key::Home, Modifiers::COMMAND);
    type_text(&mut h, "# moved\n");
    assert_eq!(h.state().python.window_checks.entries[0].line, None);
}

// ---------------------------------------------------------------- what says it failed

#[test]
fn a_cells_card_says_so_and_offers_it() {
    let mut h = harness(with_a_cell());
    settle(&mut h);
    h.state_mut().run("python.initialization", json!({"code": "fail"})).unwrap();
    settle(&mut h);
    run(&mut h, "selection.set", json!({"cell": "A1"})).unwrap();
    run(&mut h, "ui.pythonCard", json!({"on": true})).unwrap();
    assert!(h.query_by_label(FAILED).is_some(), "the card says what is wrong");
    assert!(h.query_by_label_contains("NameError").is_some(), "with Python's message");
    assert!(h.query_by_label("Line 2").is_some(), "and its line");
    h.get_by_label(OPEN).click();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.initialization));
    assert_eq!(text(&h), "fail");
    assert!(h.state().grid.py_card.is_none(), "the card is done");
    // The window's editor on the cell doesn't take the failure's line for one of its own.
    run(&mut h, "ui.pythonEditor", json!({"on": true, "cell": "A1", "force": true})).unwrap();
    let entry = h.state().python.window_checks.entries[0].clone();
    assert_eq!((entry.kind.id, entry.line), ("init.failed", None));
    run(&mut h, "ui.pythonEditor", json!({"on": false})).unwrap();
    // Its Reset tries again.
    run(&mut h, "ui.pythonCard", json!({"on": true})).unwrap();
    h.get_by_label("Reset").click();
    h.run_steps(2);
    settle(&mut h);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "python.reset"));
}

#[test]
fn the_problems_pane_lists_it_once_and_opens_it() {
    let mut s = with_a_cell();
    s.execute("python.set", json!({"cell": "A2", "code": "ret"})).unwrap();
    let mut h = harness(s);
    settle(&mut h);
    h.state_mut().run("python.initialization", json!({"code": "fail"})).unwrap();
    settle(&mut h);
    python_ui::open_pane(h.state_mut(), PROBLEMS);
    h.run_steps(3);
    assert_eq!(h.query_all_by_label(FAILED).count(), 1, "once, however many cells show it");
    h.get_by_label(INITIALIZATION).click();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.initialization));
}

#[test]
fn what_code_from_a_file_went_on_without_is_in_the_problems_pane() {
    let mut s = with_a_cell();
    from_file(&mut s, "import scipy  # skips");
    let mut h = harness(s);
    settle(&mut h);
    let d = h.state().session.active().and_then(|d| d.wb.sheet(0).and_then(|sh| sh.cell(gridcraft_engine::core::CellRef::new(0, 0)).cloned()));
    assert_eq!(d.map(|c| c.value), Some(Value::Number(1.0)), "the cell ran");
    python_ui::open_pane(h.state_mut(), PROBLEMS);
    h.run_steps(3);
    assert!(h.query_by_label_contains("skipped the lines that need it").is_some());
    assert!(h.query_by_label("Line 3").is_some(), "the line");
    assert!(h.query_by_label("scipy").is_some(), "the package");
    assert!(h.query_by_label("Copy Install Command").is_some(), "and what to do about it");
    let inspect = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspect["python"]["initialization"]["skipped"], json!(["scipy"]));
    assert_eq!(inspect["python"]["initialization"]["userModified"], false);
    // The package a run wants is what the kind's button offers, except Excel's own module.
    let row = |module: &str| gridcraft_engine::python::Diagnostic::new("init.skipped", module, Some(1));
    assert_eq!(python_ui::packages_for(&row("scipy.stats")), ["scipy.stats"]);
    assert!(python_ui::packages_for(&row("excel")).is_empty());
    // A workbook chooses this name and a person runs the command, so what can't be a package isn't offered.
    for name in ["x;id", "x y", "$(id)", "`id`", "a|b", "../x", "-e"] {
        assert!(python_ui::packages_for(&row(name)).is_empty(), "{name}");
    }
    let finding = gridcraft_engine::python::Diagnostic::new("lint.importMissing", "x;id", Some(1));
    assert!(python_ui::packages_for(&finding).is_empty());
}

#[test]
fn the_run_log_names_it_and_its_row_leads_to_it() {
    let mut h = harness(with_a_cell());
    h.state_mut().run("python.initialization", json!({"code": "x = 1"})).unwrap();
    settle(&mut h);
    python_ui::open_pane(h.state_mut(), RUN_LOG);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, INITIALIZATION).click();
    h.run_steps(3);
    h.get_by_label(OPEN).click();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.initialization));
}

#[test]
fn the_status_bar_says_it_is_running() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        if w.next_request().is_some() {
            w.stall();
        }
    });
    let mut app = crate::common::prepare(SheetApp::new(with_a_cell(), gridcraft_ui_egui::Services::default()));
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1300.0, 800.0)), app, 3);
    h.state_mut().run("python.initialization", json!({"code": "x = 1"})).unwrap();
    for _ in 0..200 {
        h.step();
        if h.state().session.python_running().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.state().session.python_running().is_some_and(|r| r.initialization));
    h.run_steps(2);
    assert!(h.query_by_label_contains("Python: running Initialization").is_some());
    let inspect = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspect["python"]["running"]["initialization"], true);
    h.state_mut().run("python.stop", json!({})).unwrap();
    h.run_steps(2);
}
