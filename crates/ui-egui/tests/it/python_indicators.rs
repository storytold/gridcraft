//! What tells a person what is wrong: the PY tag's state, the card's message and action, the state
//! chip, and the checks of the code in both editors. A fake worker and a given report stand in for
//! Python.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Color32, Event, Modifiers, Pos2, Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::python::{Diagnostic, Interpreter, InterpreterKind, Version};
use gridcraft_engine::{PythonEnvironment, PythonState, Session, Trust};
use gridcraft_python::fake::FakeTransport;
use gridcraft_python::protocol::{Output, PyError, Reply, Request};
use gridcraft_ui_egui::python_card::{self, TagState};
use gridcraft_ui_egui::theme::Tokens;
use gridcraft_ui_egui::{SheetApp, control};
use serde_json::{Value as Json, json};

use crate::common::Harness;

const PYTHON: &str = "/home/you/.gridcraft/python/bin/python";

fn worker() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let (id, code) = match req {
                Request::Run { id, code, .. } => (id, code),
                Request::Render { id, .. } | Request::Reset { id } => {
                    w.done(id, Ok(Output::Value(Value::Empty)));
                    continue;
                }
            };
            let error = |kind: &str, message: &str, subject: Option<&str>, line| PyError {
                message: message.into(),
                kind: Some(kind.into()),
                subject: subject.map(str::to_string),
                line,
            };
            let value = Ok(Output::Value(Value::Number(1.0)));
            let sent = if code == "hang" {
                w.stall();
                false
            } else if code == "fail" || code.contains("raise") {
                w.done(id, Err(error("python.exception", "ValueError: boom", None, Some(2))))
            } else if code == "printfail" {
                w.send(&Reply::Done {
                    id,
                    result: Err(error("python.exception", "ValueError: boom", None, Some(1))),
                    stdout: "printed before it failed\n".into(),
                    peak: None,
                    touched: None,
                })
            } else if code == "denied" {
                w.done(id, Err(error("denied.file", "[Errno 1] Operation not permitted: '/home/you/data.csv'", Some("/home/you/data.csv"), Some(1))))
            } else if code == "missing" {
                w.done(id, Err(error("module.missing", "No module named 'sklearn'", Some("sklearn"), Some(1))))
            } else if code == "spill" {
                let rows =
                    gridcraft_engine::core::Array::new(3, 1, vec![1.0.into(), 2.0.into(), 3.0.into()]).map(Value::from).unwrap_or(Value::Empty);
                w.done(id, Ok(Output::Value(rows)))
            } else if code == "obj" {
                w.done(id, Ok(Output::object("DataFrame", "   a  b\n0  1  2")))
            } else {
                w.done(id, value)
            };
            if !sent {
                return;
            }
        }
    })
}

fn report(state: PythonState, diagnostic: Option<Diagnostic>) -> PythonEnvironment {
    PythonEnvironment {
        state,
        diagnostic,
        sandbox: "a test sandbox".into(),
        uv: Some(PathBuf::from("/home/you/.local/bin/uv")),
        interpreter: Some(Interpreter {
            path: PathBuf::from(PYTHON),
            run_as: PathBuf::from(PYTHON),
            version: Version { major: 3, minor: 12, patch: 7 },
            kind: InterpreterKind::UvEnvironment,
            site_dirs: Vec::new(),
            read_paths: Vec::new(),
        }),
        packages: vec![("numpy".into(), "2.1.3".into()), ("pandas".into(), "2.2.3".into())],
    }
}

fn app_on(session: Session, enabled: bool, report: PythonEnvironment) -> SheetApp {
    let t = worker();
    let mut app = crate::common::prepare(SheetApp::new(session, gridcraft_ui_egui::Services::default()));
    app.ui.python.enabled = enabled;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report);
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    app
}

fn harness_of(app: SheetApp) -> Harness {
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1300.0, 800.0)), app, 3);
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.run_steps(2);
    h
}

/// Cells at these places, with this code, Python on and ready.
fn with_cells(cells: &[(&str, &str)]) -> Harness {
    let mut h = harness_of(app_on(blank(), true, report(PythonState::Ready, None)));
    for (cell, code) in cells {
        h.state_mut().run("python.set", json!({"cell": cell, "code": code})).unwrap();
    }
    settle(&mut h);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
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

fn c(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

fn state_of(h: &Harness, a1: &str) -> TagState {
    let app = h.state();
    let d = app.session.active().unwrap();
    let cell = d.wb.sheet(0).unwrap().cell(c(a1)).unwrap();
    python_card::tag_state(&app.session, 0, c(a1), cell.py.as_deref().unwrap(), &cell.value)
}

/// The fills of what the frame painted.
fn fills(h: &Harness) -> Vec<Color32> {
    fn walk(shape: &Shape, out: &mut Vec<Color32>) {
        match shape {
            Shape::Rect(r) => out.push(r.fill),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

// ---------------------------------------------------------------- the tag

#[test]
fn the_tag_shows_a_ready_a_failed_and_a_running_cell() {
    let mut h = with_cells(&[("A1", "ret"), ("A2", "fail")]);
    assert_eq!(state_of(&h, "A1"), TagState::Ready);
    assert_eq!(state_of(&h, "A2"), TagState::Error);
    let t = Tokens::get(&h.ctx);
    assert!(fills(&h).contains(&t.sheet_py_tag_error), "a failed cell's tag has a colour of its own");
    h.state_mut().run("python.set", json!({"cell": "A3", "code": "hang"})).unwrap();
    for _ in 0..200 {
        h.step();
        if h.state().session.python_running().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(state_of(&h, "A3"), TagState::Running);
    h.state_mut().run("python.stop", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(state_of(&h, "A3"), TagState::NotRunning, "stopped, it isn't running and has no new result");
}

#[test]
fn the_tag_shows_a_stale_cell_as_not_running() {
    let mut h = with_cells(&[("A1", "ret")]);
    assert_eq!(state_of(&h, "A1"), TagState::Ready);
    h.state_mut().run("python.calculation", json!({"mode": "manual"})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "A2", "code": "ret"})).unwrap();
    h.run_steps(3);
    assert_eq!(state_of(&h, "A2"), TagState::NotRunning);
}

#[test]
fn the_tag_shows_python_off_as_not_running_and_a_system_that_cant_run_it_as_blocked() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    let h = harness_of(app_on(s, false, report(PythonState::Ready, None)));
    assert_eq!(state_of(&h, "A1"), TagState::NotRunning);
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    let blocked = report(PythonState::Blocked, Some(Diagnostic::new("blocked.sandboxCheck", "read the canary", None)));
    let h = harness_of(app_on(s, true, blocked));
    assert_eq!(state_of(&h, "A1"), TagState::Blocked);
    let t = Tokens::get(&h.ctx);
    assert!(fills(&h).contains(&t.sheet_py_tag_blocked));
}

#[test]
fn a_tag_with_a_mark_is_painted_where_it_is_hit_on_a_right_to_left_sheet() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "B2", "code": "fail"})).unwrap();
    s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    let mut h = harness_of(app_on(s, true, report(PythonState::Ready, None)));
    settle(&mut h);
    assert_eq!(state_of(&h, "B2"), TagState::Error);
    h.check_accessibility();
    let found = h.output().shapes.iter().any(|clipped| matches!(&clipped.shape, Shape::Text(t) if t.galley.job.text == "PY"));
    assert!(found, "its PY still reads");
}

// ---------------------------------------------------------------- the card

fn card(h: &mut Harness, cell: &str) {
    h.state_mut().run("selection.set", json!({"range": cell})).unwrap();
    h.run_steps(2);
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
}

fn journaled(h: &Harness, id: &str) -> bool {
    h.state().session.journal.iter().any(|(j, _)| j == id)
}

#[test]
fn the_card_says_a_denied_file_with_its_detail_and_line() {
    let mut h = with_cells(&[("A1", "denied")]);
    card(&mut h, "A1");
    assert!(h.query_by_label("Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl().").is_some());
    assert!(h.query_by_label_contains("Operation not permitted: '/home/you/data.csv'").is_some(), "the detail, as it came");
    assert!(h.query_by_label("Line 1").is_some());
    assert!(h.query_by_label("Copy Install Command").is_none(), "this kind has no action");
    h.check_accessibility();
}

#[test]
fn the_card_copies_the_uv_command_for_a_missing_module_and_keeps_pip_behind_a_section() {
    let mut h = with_cells(&[("A1", "missing")]);
    card(&mut h, "A1");
    assert!(h.query_by_label("This Python doesn't have that package.").is_some());
    h.get_by_label("Copy Install Command").click();
    h.step();
    let uv = format!("uv pip install --compile-bytecode --python {PYTHON} scikit-learn");
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if *t == uv)), "{uv}");
    h.run_steps(2);
    let pip = format!("{PYTHON} -m pip install scikit-learn");
    assert!(h.query_by_label(&pip).is_none());
    h.get_by_label("Other package managers").click();
    h.run_steps(3);
    assert!(h.query_by_label(&pip).is_some());
    h.check_accessibility();
}

#[test]
fn the_card_runs_a_cell_again_after_a_limit() {
    // The limit is the worker's outcome; the catalog's wording and Run Again come with its kind.
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "fail"})).unwrap();
    let not_ready = report(PythonState::NotSetUp, Some(Diagnostic::new("connect.failed", "Python stopped before it said it was ready", None)));
    let mut h = harness_of(app_on(s, true, not_ready));
    card(&mut h, "A1");
    assert!(h.query_by_label("Python didn't start.").is_some());
    h.get_by_label("Run Again").click();
    h.run_steps(3);
    assert!(journaled(&h, "python.run"));
}

#[test]
fn the_card_shows_a_time_limit_with_run_again() {
    let mut app = app_on(blank(), true, report(PythonState::Ready, None));
    app.ui.python.time_limit_secs = 1;
    let mut h = harness_of(app);
    // A worker that has started has no more than the limit.
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    settle(&mut h);
    h.state_mut().run("python.set", json!({"cell": "A2", "code": "hang"})).unwrap();
    settle(&mut h);
    card(&mut h, "A2");
    assert!(h.query_by_label("The code ran past the time limit. Python was restarted, so its variables were cleared.").is_some());
    assert!(h.query_by_label("Run Again").is_some());
    assert_eq!(state_of(&h, "A2"), TagState::Error);
}

#[test]
fn the_card_opens_the_settings_and_chooses_an_interpreter_from_its_buttons() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    let mut h = harness_of(app_on(s, false, report(PythonState::Ready, None)));
    card(&mut h, "A1");
    assert!(h.query_by_label("Python is off. Turn it on in Settings to run Python cells.").is_some());
    h.get_by_label("Open Settings").click();
    h.run_steps(3);
    assert!(h.state().backstage.is_some(), "File › Options");
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    let none = PythonEnvironment { interpreter: None, ..report(PythonState::NotSetUp, Some(Diagnostic::new("notSetUp.noInterpreter", "", None))) };
    let mut h = harness_of(app_on(s, true, none));
    card(&mut h, "A1");
    h.get_by_label("Choose Interpreter").click();
    h.run_steps(3);
    assert!(h.state().backstage.is_some());
}

#[test]
fn the_card_enables_a_workbook_from_a_file() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    s.active_mut().unwrap().trust = Trust::FromFile;
    let mut h = harness_of(app_on(s, true, report(PythonState::Ready, None)));
    card(&mut h, "A1");
    assert!(h.query_by_label("Python isn't on for this workbook, so its cells keep the values saved in the file.").is_some());
    // The bar has an Enable too: the card's is the second.
    h.get_all_by_label("Enable").last().unwrap().click();
    h.run_steps(3);
    assert!(h.state().session.python_workbook_enabled());
}

#[test]
fn the_card_resets_when_the_kind_asks_for_it() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "ret"})).unwrap();
    let failed = report(PythonState::NotSetUp, Some(Diagnostic::new("init.failed", "NameError: x", None)));
    let mut h = harness_of(app_on(s, true, failed));
    card(&mut h, "A1");
    h.get_by_label("Reset").click();
    h.run_steps(3);
    assert!(h.state().message.is_some() || journaled(&h, "python.reset"), "it asked for python.reset");
    // The kind offers the code it is about, too.
    card(&mut h, "A1");
    h.get_by_label("Open Initialization").click();
    h.run_steps(3);
    assert!(h.state().py_window.as_ref().is_some_and(|w| w.initialization));
}

#[test]
fn the_card_goes_to_the_cell_in_the_way_of_a_spill() {
    let mut h = with_cells(&[]);
    h.state_mut().run("cell.set", json!({"cell": "A3", "input": "in the way"})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "spill"})).unwrap();
    settle(&mut h);
    card(&mut h, "A1");
    assert!(h.query_by_label("The result needs cells that aren't empty.").is_some());
    assert!(h.query_by_label("A3 is in the way.").is_some());
    h.get_by_label("Go To Cell").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A3");
}

#[test]
fn the_card_shows_what_a_failed_run_printed() {
    let mut h = with_cells(&[("A1", "printfail"), ("A2", "fail")]);
    card(&mut h, "A1");
    assert!(h.query_by_label("printed before it failed").is_none());
    h.get_by_label("Show Printed Output").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("printed before it failed").is_some());
    assert!(h.query_by_label("Hide Printed Output").is_some());
    // A failure that printed nothing has no such button.
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    h.run_steps(2);
    card(&mut h, "A2");
    assert!(h.query_by_label("Show Printed Output").is_none());
}

#[test]
fn a_result_keeps_its_type_and_preview() {
    let mut h = with_cells(&[("A1", "obj")]);
    card(&mut h, "A1");
    assert!(h.query_by_label("DataFrame").is_some());
    assert!(h.query_by_label_contains("a  b").is_some());
}

// ---------------------------------------------------------------- the chip

#[test]
fn the_chip_in_python_mode_says_the_state_and_opens_the_environment_pane() {
    let mut h = with_cells(&[]);
    h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Python 3.12, ready").is_some());
    h.get_by_role_and_label(Role::Button, "Python 3.12, ready").click();
    h.run_steps(3);
    assert_eq!(h.state().grid.pane.as_deref(), Some(gridcraft_ui_egui::python_ui::ENVIRONMENT));
    // With a blocked system it says so.
    let mut app = app_on(blank(), false, report(PythonState::Ready, None));
    app.python.set_sandbox(Err("no sandbox".into()));
    let mut h = harness_of(app);
    h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Blocked").is_some());
}

#[test]
fn no_chip_outside_python_mode() {
    let h = with_cells(&[]);
    assert!(h.query_by_role_and_label(Role::Button, "Python 3.12, ready").is_none());
}

// ---------------------------------------------------------------- the checks

fn findings(h: &Harness, editor: &str) -> Vec<Json> {
    control::inspect(h.state(), &h.ctx)["python"]["findings"][editor].as_array().cloned().unwrap_or_default()
}

fn kinds(h: &Harness, editor: &str) -> Vec<String> {
    findings(h, editor).iter().filter_map(|f| f["kind"].as_str().map(str::to_string)).collect()
}

fn hover(h: &mut Harness, at: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(4);
}

/// Where a char of the first line of a code field is, in the monospace code font.
fn char_position(h: &Harness, field: egui::Rect, chars: f32) -> Pos2 {
    let width = h.ctx.fonts_mut(|f| f.glyph_width(&gridcraft_ui_egui::code_editor::font(), '0'));
    field.left_top() + vec2(chars * width, 8.0)
}

fn draft(h: &mut Harness, code: &str) {
    h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text(code.into()));
    h.run_steps(4);
}

#[test]
fn the_formula_bar_underlines_a_finding_and_says_it_on_hover() {
    let mut h = with_cells(&[]);
    draft(&mut h, "import socket");
    assert_eq!(kinds(&h, "formulaBar"), ["lint.importBlocked"]);
    let finding = &findings(&h, "formulaBar")[0];
    assert_eq!((finding["line"].as_u64(), finding["start"].as_u64(), finding["end"].as_u64()), (Some(1), Some(7), Some(13)));
    assert!(wavy(&h), "a wavy line is painted");
    let field = h.get_by_label("Formula Bar").rect();
    let at = char_position(&h, field, 9.0);
    hover(&mut h, at);
    assert!(h.query_by_label_contains("This module needs the network or other programs").is_some());
    h.check_accessibility();
}

/// Whether the frame painted a line with enough points to be a wave.
fn wavy(h: &Harness) -> bool {
    h.output().shapes.iter().any(|clipped| matches!(&clipped.shape, Shape::Path(p) if p.points.len() >= 4 && !p.closed))
}

fn open_window(h: &mut Harness, cell: &str) {
    h.state_mut().run("ui.pythonEditor", json!({"on": true, "cell": cell})).unwrap();
    h.run_steps(6);
}

#[test]
fn the_editor_window_marks_the_error_line_and_the_findings() {
    let mut h = with_cells(&[("A1", "x = 1\nraise ValueError()\nimport socket")]);
    open_window(&mut h, "A1");
    assert_eq!(kinds(&h, "editor"), ["python.exception", "lint.importBlocked"]);
    let error = &findings(&h, "editor")[0];
    assert_eq!((error["line"].as_u64(), error["start"].as_u64(), error["end"].as_u64()), (Some(2), Some(6), Some(24)));
    assert!(wavy(&h));
    // The problems strip lists both.
    assert!(h.query_by_label_contains("Line 2: The code raised an error.").is_some());
    assert!(h.query_by_label_contains("Line 3: This module needs the network").is_some());
    // The chip and the interpreter are in the header.
    assert!(h.query_by_role_and_label(Role::Button, "Python 3.12, ready").is_some());
    assert!(h.query_by_label(PYTHON).is_some());
    h.check_accessibility();
}

#[test]
fn the_editor_window_says_a_finding_on_hover_on_its_span_and_beside_its_line() {
    let mut h = with_cells(&[]);
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "import socket"})).unwrap();
    settle(&mut h);
    open_window(&mut h, "A1");
    let w = h.state().py_window.as_ref().unwrap();
    let gutter = w.gutter;
    let field = h.get_by_role_and_label(Role::MultilineTextInput, "Python Editor").rect();
    // The problems strip says it once; a tooltip says it again.
    let said = |h: &Harness| h.query_all_by_label_contains("This module needs the network or other programs").count();
    assert_eq!(said(&h), 1);
    let at = char_position(&h, field, 9.0);
    hover(&mut h, at);
    assert_eq!(said(&h), 2, "on the span");
    hover(&mut h, Pos2::new(0.0, 0.0));
    assert_eq!(said(&h), 1);
    // The gutter's mark is at the left of the field.
    let at = field.left_top() + vec2(5.5 - gutter, 8.0);
    hover(&mut h, at);
    assert_eq!(said(&h), 2, "beside its line");
}

#[test]
fn a_click_in_the_problems_strip_goes_to_the_line() {
    let mut h = with_cells(&[("A1", "x = 1\ny = 2\nimport socket")]);
    open_window(&mut h, "A1");
    h.get_by_label_contains("Line 3:").click();
    h.run_steps(4);
    let w = h.state().py_window.as_ref().unwrap();
    let state = egui::TextEdit::load_state(&h.ctx, gridcraft_ui_egui::python_window::editor_id(w)).unwrap();
    let caret: usize = state.cursor.char_range().unwrap().primary.index.into();
    assert_eq!(caret, "x = 1\ny = 2\nimport ".chars().count());
}

#[test]
fn a_span_follows_the_text_when_it_changes_above_it_with_a_multi_byte_character() {
    let mut h = with_cells(&[]);
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "x = 1\nraise ValueError()\nimport socket"})).unwrap();
    settle(&mut h);
    open_window(&mut h, "A1");
    assert_eq!(kinds(&h, "editor"), ["python.exception", "lint.importBlocked"]);
    // The caret goes to the start and a line with a multi-byte character goes in above.
    h.state_mut().py_window.as_mut().unwrap().caret = Some(0);
    h.state_mut().py_window.as_mut().unwrap().focus = true;
    h.run_steps(3);
    h.input_mut().events.push(Event::Text("é字 = 1".into()));
    h.input_mut().events.push(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
    let text = h.state().py_window.as_ref().unwrap().text.clone();
    assert!(text.starts_with("é字 = 1\n"), "{text:?}");
    // The run's line no longer means a line of this text; the finding moved with it.
    let found = findings(&h, "editor");
    let lint = found.iter().find(|f| f["kind"] == "lint.importBlocked").unwrap();
    assert_eq!(lint["line"].as_u64(), Some(4));
    let (start, end) = (lint["start"].as_u64().unwrap() as usize, lint["end"].as_u64().unwrap() as usize);
    assert_eq!(&text[start..end], "socket", "byte offsets on char boundaries, into the new text");
    let run = found.iter().find(|f| f["kind"] == "python.exception").unwrap();
    assert!(run["line"].is_null(), "{run}");
    // A span from before an edit that takes the text away is no panic.
    h.state_mut().py_window.as_mut().unwrap().text = "é".into();
    h.run_steps(3);
    assert!(findings(&h, "editor").iter().all(|f| f["start"].is_null()));
}

#[test]
fn checks_run_once_per_edit_even_on_long_code() {
    let long = "import socket\n".repeat(500);
    let mut h = with_cells(&[]);
    draft(&mut h, &long);
    let lints = h.state().python.bar_checks.lints();
    h.run_steps(20);
    assert_eq!(h.state().python.bar_checks.lints(), lints, "frames don't lint");
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(4);
    assert_eq!(h.state().python.bar_checks.lints(), lints + 1, "an edit lints once");
    assert_eq!(findings(&h, "formulaBar").len(), 100, "the first hundred");
}

#[test]
fn checks_work_with_python_off_and_with_no_sandbox() {
    let mut h = harness_of(app_on(blank(), false, report(PythonState::Ready, None)));
    draft(&mut h, "import socket");
    assert_eq!(kinds(&h, "formulaBar"), ["lint.importBlocked"]);
    let mut app = app_on(blank(), false, report(PythonState::Ready, None));
    app.python.set_sandbox(Err("no sandbox".into()));
    let mut h = harness_of(app);
    draft(&mut h, "import socket");
    assert_eq!(kinds(&h, "formulaBar"), ["lint.importBlocked"]);
}

#[test]
fn an_empty_package_list_reports_nothing_missing() {
    let empty = PythonEnvironment { packages: Vec::new(), ..report(PythonState::Ready, None) };
    let mut h = harness_of(app_on(blank(), true, empty));
    draft(&mut h, "import nope");
    assert_eq!(kinds(&h, "formulaBar"), Vec::<String>::new());
    h.state_mut().cancel_edit();
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "import nope"})).unwrap();
    settle(&mut h);
    h.state_mut().run("ui.pythonProblems", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label_contains("packages.missing").is_none());
}

#[test]
fn a_package_the_list_lacks_is_found_in_both_places_with_its_command() {
    let mut h = with_cells(&[]);
    draft(&mut h, "import sklearn");
    assert_eq!(kinds(&h, "formulaBar"), ["lint.importMissing"]);
    assert_eq!(findings(&h, "formulaBar")[0]["detail"], "scikit-learn");
    h.state_mut().cancel_edit();
    h.run_steps(2);
    assert!(findings(&h, "formulaBar").is_empty(), "no draft, no findings");
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "import sklearn"})).unwrap();
    settle(&mut h);
    h.state_mut().run("ui.pythonProblems", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("packages.missing (1)").is_some());
    assert!(h.query_by_label("This workbook imports packages this Python doesn't have.").is_some());
    h.get_by_label("Copy Install Command").click();
    h.step();
    let uv = format!("uv pip install --compile-bytecode --python {PYTHON} scikit-learn");
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if *t == uv)));
}

#[test]
fn ui_inspect_reports_the_findings_of_each_editor() {
    let mut h = with_cells(&[("A1", "open('a.txt')")]);
    assert!(findings(&h, "editor").is_empty(), "no window");
    open_window(&mut h, "A1");
    let found = findings(&h, "editor");
    assert!(
        found.iter().any(|f| f["kind"] == "lint.fileAccess"
            && f["severity"] == "warning"
            && f["detail"].as_str().is_some()
            && f["message"].as_str().is_some()),
        "{found:?}"
    );
    h.state_mut().run("ui.pythonEditor", json!({"on": false, "force": true})).unwrap();
    h.run_steps(3);
    assert!(findings(&h, "editor").is_empty(), "the window closed");
}
