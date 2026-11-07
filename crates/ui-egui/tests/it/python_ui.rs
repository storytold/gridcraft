//! Turning Python on and understanding it: the hookup, File › Options › Python, the Enable bar, the
//! Python tab, the panes, the status bar item, and that nothing programmatic can turn Python on.
//! A fake worker and a given report stand in for Python, so these run on every system.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::python::{Diagnostic, Interpreter, InterpreterKind, Version};
use gridcraft_engine::{FoundPython, PythonEnvironment, PythonSearch, PythonState, Session, Trust};
use gridcraft_python::fake::{FakeHandle, FakeTransport};
use gridcraft_python::protocol::{Output, PyError, Reply, Request};
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::keytips::keytip_table;
use gridcraft_ui_egui::ribbon::{PYTHON_TAB, RibbonOverflow, TABS, contextual_tabs};
use gridcraft_ui_egui::{Services, SheetApp, python_ui};
use serde_json::{Value as Json, json};

use crate::common::Harness;

fn worker() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let (id, code) = match req {
                Request::Run { id, code, .. } => (id, code),
                Request::Render { id, .. } | Request::Reset { id } => {
                    w.done(id, Ok(Output::Value(gridcraft_engine::core::Value::Empty)));
                    continue;
                }
            };
            let number = code.strip_prefix("ret ").and_then(|n| n.parse().ok()).unwrap_or(1.0);
            let value = Ok(Output::Value(gridcraft_engine::core::Value::Number(number)));
            let sent = match code.as_str() {
                "hang" => {
                    w.stall();
                    false
                }
                "spill" => {
                    let cells = vec![gridcraft_engine::core::Value::Number(1.0), gridcraft_engine::core::Value::Number(2.0)];
                    let array = gridcraft_engine::core::Array { rows: 2, cols: 1, data: cells };
                    w.done(id, Ok(Output::Value(gridcraft_engine::core::Value::Array(Arc::new(array)))))
                }
                "fail" => w.done(id, Err(PyError { message: "ValueError: boom".into(), line: Some(2), ..PyError::default() })),
                other => match other.strip_prefix("print ") {
                    Some(text) => w.send(&Reply::Done { id, result: value, stdout: format!("{text}\n"), peak: None, touched: None }),
                    None => w.done(id, value),
                },
            };
            if !sent {
                return;
            }
        }
    })
}

fn interpreter() -> Interpreter {
    Interpreter {
        path: PathBuf::from("/home/you/.gridcraft/python/bin/python"),
        run_as: PathBuf::from("/home/you/.local/share/uv/python/cpython-3.12.7/bin/python3.12"),
        version: Version { major: 3, minor: 12, patch: 7 },
        kind: InterpreterKind::UvEnvironment,
        site_dirs: vec![PathBuf::from("/home/you/.gridcraft/python/lib/python3.12/site-packages")],
        read_paths: vec![PathBuf::from("/home/you/.gridcraft/python")],
    }
}

fn report(state: PythonState) -> PythonEnvironment {
    PythonEnvironment {
        state,
        diagnostic: None,
        sandbox: "a test sandbox".into(),
        uv: Some(PathBuf::from("/home/you/.local/bin/uv")),
        interpreter: Some(interpreter()),
        packages: vec![("numpy".into(), "2.1.3".into()), ("pandas".into(), "2.2.3".into())],
    }
}

fn no_python() -> PythonEnvironment {
    PythonEnvironment {
        state: PythonState::NotSetUp,
        diagnostic: Some(Diagnostic::new("notSetUp.noInterpreter", "", None)),
        sandbox: "a test sandbox".into(),
        uv: None,
        interpreter: None,
        packages: Vec::new(),
    }
}

/// What a test sets up: the session, whether Python is on, the report, the services, the window.
struct Setup {
    session: Session,
    enabled: bool,
    report: PythonEnvironment,
    services: Services,
    size: egui::Vec2,
    sandbox: Result<String, String>,
    tab: &'static str,
    search: PythonSearch,
}

impl Setup {
    /// The active cell is a Python cell, so the Python tab is up.
    fn new() -> Setup {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("python.set", json!({"cell": "A1", "code": "ret 1"})).unwrap();
        Setup::on(session)
    }

    /// A workbook with no Python cells: the tab isn't up, and the ribbon opens on Home.
    fn plain() -> Setup {
        let mut session = Session::new();
        session.new_workbook();
        Setup::on(session)
    }

    fn on(session: Session) -> Setup {
        Setup {
            session,
            enabled: true,
            report: report(PythonState::Ready),
            services: Services::default(),
            size: vec2(1400.0, 800.0),
            sandbox: Ok("a test sandbox".into()),
            tab: "Python",
            search: PythonSearch {
                found: vec![FoundPython {
                    path: interpreter().path,
                    version: Some(interpreter().version),
                    kind: Some(InterpreterKind::UvEnvironment),
                }],
                uv: Some(PathBuf::from("/home/you/.local/bin/uv")),
            },
        }
    }

    fn off(mut self) -> Setup {
        self.enabled = false;
        self
    }

    fn report(mut self, report: PythonEnvironment) -> Setup {
        self.report = report;
        self
    }

    fn with_session(mut self, session: Session) -> Setup {
        self.session = session;
        self
    }

    fn build(self) -> (Harness, FakeHandle) {
        let t = worker();
        let handle = t.handle();
        let mut app = crate::common::prepare(SheetApp::new(self.session, self.services));
        app.ui.ribbon_tab = self.tab.into();
        app.ui.python.enabled = self.enabled;
        app.python.set_sandbox(self.sandbox);
        app.python.report = Some(self.report);
        app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
        app.python.search_answer = Some(self.search);
        let mut h = crate::common::harness_with(crate::common::builder().with_size(self.size), app, 3);
        h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
        h.run_steps(2);
        settle(&mut h);
        (h, handle)
    }
}

/// A session whose active workbook came from a file and holds a Python cell.
fn from_file() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "A1", "code": "ret 5"})).unwrap();
    s.active_mut().unwrap().trust = Trust::FromFile;
    s
}

/// Steps until Python has nothing left to do, as the app's frames do.
fn settle(h: &mut Harness) {
    for _ in 0..400 {
        h.step();
        if !h.state().session.python_busy() {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Python never settled");
}

/// Steps until the worker reads a request, for a cell that goes on running.
fn until_running(h: &mut Harness) {
    for _ in 0..400 {
        h.step();
        if h.state().session.python_running().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no cell started");
}

/// Shows the Python tab, as a click on it does.
fn show_tab(h: &mut Harness) {
    h.state_mut().ui.ribbon_tab = PYTHON_TAB.into();
    h.run_steps(3);
}

fn button<'a>(h: &'a Harness, label: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(Role::Button, label)
}

fn click(h: &mut Harness, label: &str) {
    button(h, label).click();
    h.run_steps(3);
}

fn journaled(h: &Harness, id: &str) -> bool {
    h.state().session.journal.iter().any(|(j, _)| j == id)
}

fn pane(h: &Harness) -> Option<String> {
    h.state().grid.pane.clone()
}

fn inspect(h: &Harness) -> Json {
    control::inspect(h.state(), &h.ctx)["python"].clone()
}

/// What the person's settings gave the session: on, the interpreter and the workbook's switch.
fn turned_on(h: &Harness) -> (bool, Option<PathBuf>, bool, bool) {
    let app = h.state();
    let c = app.session.python_capability();
    (c.is_some_and(|c| c.enabled), c.and_then(|c| c.interpreter.clone()), app.session.python_workbook_enabled(), app.ui.python.enabled)
}

// ---------------------------------------------------------------- the hookup

#[test]
fn the_running_app_gives_the_session_a_capability_that_is_off_by_default() {
    let mut s = Session::new();
    s.new_workbook();
    assert!(s.python_capability().is_none(), "a session on its own has none, as in the CLI and MCP");
    let h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(s), 3);
    let capability = h.state().session.python_capability().expect("the app gives one");
    assert!(!capability.enabled);
    assert_eq!(h.state().session.python_state(), PythonState::Off);
    assert_eq!(h.state().session.python_report().diagnostic.as_ref().map(|d| d.kind.id), Some("off.app"));
}

#[test]
fn a_result_appears_without_input() {
    let (mut h, handle) = Setup::new().build();
    let value = |h: &Harness| h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("B2").unwrap());
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "ret 7"})).unwrap();
    assert_eq!(value(&h), gridcraft_engine::core::Value::Error(gridcraft_engine::core::CellError::Busy));
    settle(&mut h);
    assert_eq!(value(&h), gridcraft_engine::core::Value::Number(7.0), "no input, only frames");
    assert!(handle.starts() >= 1);
}

/// One pass of `logic`, and how soon it asks to be called again.
fn repaint_delay(ctx: &egui::Context, app: &mut SheetApp) -> Duration {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
    out.viewport_output.get(&egui::ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay)
}

#[test]
fn an_idle_app_stays_idle_and_a_busy_one_wakes_at_a_low_rate() {
    let t = worker();
    let mut s = Session::new();
    s.new_workbook();
    let mut app = crate::common::app(s);
    app.fonts_ready = true;
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report(PythonState::Ready));
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    let ctx = egui::Context::default();
    for _ in 0..4 {
        repaint_delay(&ctx, &mut app);
    }
    assert_eq!(repaint_delay(&ctx, &mut app), Duration::MAX, "nothing to do, so nothing asks for a frame");
    app.run("python.set", json!({"cell": "A1", "code": "hang"})).unwrap();
    for _ in 0..3 {
        repaint_delay(&ctx, &mut app);
    }
    assert!(app.session.python_busy());
    let delay = repaint_delay(&ctx, &mut app);
    assert!(delay >= Duration::from_millis(50) && delay <= Duration::from_millis(100), "a low rate, not every frame: {delay:?}");
    app.run("python.stop", json!({})).unwrap();
    repaint_delay(&ctx, &mut app);
    repaint_delay(&ctx, &mut app);
    assert_eq!(repaint_delay(&ctx, &mut app), Duration::MAX, "idle again");
}

// ---------------------------------------------------------------- File › Options › Python

fn open_options(h: &mut Harness) {
    h.state_mut().run("ui.backstage", json!({"page": "options", "section": "python"})).unwrap();
    h.run_steps(3);
}

#[test]
fn the_options_page_writes_the_preference_and_hands_the_session_a_capability() {
    let (mut h, _) = Setup::new().off().build();
    open_options(&mut h);
    assert_eq!(inspect(&h)["optionsPage"], true);
    assert!(!turned_on(&h).0);
    h.get_by_label("Enable Python in cells").click();
    h.run_steps(3);
    assert!(turned_on(&h).0, "the capability");
    assert!(h.state().ui.python.enabled, "the preference");
    // A1 is a Python cell, which runs now.
    settle(&mut h);
    assert_eq!(h.state().session.python_state(), PythonState::Ready);
    // It goes with the other preferences.
    let saved: gridcraft_ui_egui::UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
    assert!(saved.python.enabled);
    h.get_by_label("Enable Python in cells").click();
    h.run_steps(3);
    assert!(!turned_on(&h).0);
    assert_eq!(h.state().session.python_state(), PythonState::Off);
    h.check_accessibility();
}

#[test]
fn the_page_says_what_automatic_found_or_how_to_set_python_up_with_uv() {
    let (mut h, _) = Setup::new().build();
    open_options(&mut h);
    assert!(h.query_by_label("/home/you/.gridcraft/python/bin/python").is_some());
    assert!(h.query_by_label("Python 3.12.7, uv environment").is_some());
    assert!(h.query_by_label("uv venv ~/.gridcraft/python --python 3.12").is_none(), "there is a Python, so no setup");
    drop(h);

    let mut setup = Setup::new().report(no_python());
    setup.search = PythonSearch::default();
    let (mut h, _) = setup.build();
    open_options(&mut h);
    assert!(h.query_by_label("uv venv ~/.gridcraft/python --python 3.12").is_some());
    assert!(h.query_by_label("uv pip install --compile-bytecode --python ~/.gridcraft/python numpy pandas matplotlib seaborn statsmodels").is_some());
    assert!(h.query_by_label("curl -LsSf https://astral.sh/uv/install.sh | sh").is_some(), "no uv, so how to get it");
    // Copying is all it does.
    let copy = h.get_all_by_label("Copy").count();
    assert_eq!(copy, 3);
    h.get_all_by_label("Copy").next().unwrap().click();
    h.step();
    assert!(
        h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.starts_with("curl"))),
        "the first command is on the clipboard"
    );
    h.run_steps(2);
    assert!(h.state().toast.is_some());
    // pip stays behind a secondary choice.
    assert!(h.query_by_label("python3 -m pip install numpy pandas matplotlib seaborn statsmodels").is_none());
    h.get_by_label("Other package managers").click();
    h.run_steps(3);
    assert!(h.query_by_label("python3 -m pip install numpy pandas matplotlib seaborn statsmodels").is_some());
}

#[test]
fn with_python_off_nothing_is_started_and_automatic_names_what_it_would_try() {
    let (mut h, _) = Setup::new().off().build();
    open_options(&mut h);
    assert!(h.query_by_label("While Python is off, GridCraft starts nothing, so it can't yet say which version it would use.").is_some());
    assert!(h.query_by_label("/home/you/.gridcraft/python/bin/python").is_some());
    assert!(h.query_by_label("Python 3.12.7, uv environment").is_none());
}

#[test]
fn browse_chooses_an_interpreter_with_the_file_picker() {
    let mut setup = Setup::new();
    setup.services.pick_interpreter = Some(Box::new(|| Some("/opt/py/bin/python3".into())));
    let (mut h, _) = setup.build();
    open_options(&mut h);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.python.interpreter.as_deref(), Some("/opt/py/bin/python3"));
    assert_eq!(turned_on(&h).1, Some(PathBuf::from("/opt/py/bin/python3")), "the session has it");
}

#[test]
fn the_limits_go_to_the_session_within_their_bounds() {
    let (mut h, _) = Setup::new().build();
    open_options(&mut h);
    let field = h.get_by_label("Time limit for one cell");
    field.click();
    h.run_steps(2);
    h.input_mut().events.extend([
        Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
        Event::Text("99999".into()),
    ]);
    h.step();
    h.input_mut().events.push(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let limits = h.state().session.python_capability().unwrap().limits.clone();
    assert_eq!(limits.timeout, Duration::from_secs(3600), "cut to the field's upper end");
    assert_eq!(h.state().ui.python.time_limit_secs, 3600);
}

// ---------------------------------------------------------------- the Enable bar

const ENABLE: &str = "Enable";

#[test]
fn the_enable_bar_shows_for_a_workbook_from_a_file_and_enables_it() {
    let (mut h, handle) = Setup::new().with_session(from_file()).build();
    assert_eq!(inspect(&h)["enableBar"], true);
    assert_eq!(handle.starts(), 0, "nothing runs until it is enabled");
    assert!(
        h.query_by_label("This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file.").is_some()
    );
    h.check_accessibility();
    click(&mut h, ENABLE);
    assert!(h.state().session.python_workbook_enabled());
    assert_eq!(inspect(&h)["enableBar"], false);
    settle(&mut h);
    assert!(handle.starts() >= 1, "it ran");
}

#[test]
fn the_enable_bar_stays_away_when_it_has_nothing_to_offer() {
    // Python off in Settings: enabling this workbook wouldn't run it.
    let (h, _) = Setup::new().off().with_session(from_file()).build();
    assert_eq!(inspect(&h)["enableBar"], false);
    // On a system with no sandbox it would run nothing either, whatever a saved preference says.
    let mut setup = Setup::new().with_session(from_file());
    setup.sandbox = Err("no sandbox".into());
    let (h, _) = setup.build();
    assert_eq!(inspect(&h)["enableBar"], false);
    // A workbook made here runs already.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "A1", "code": "ret 5"})).unwrap();
    let (h, _) = Setup::new().with_session(s).build();
    assert_eq!(inspect(&h)["enableBar"], false);
    // A file with no Python cells has nothing to enable.
    let mut s = Session::new();
    s.new_workbook();
    s.active_mut().unwrap().trust = Trust::FromFile;
    let (h, _) = Setup::new().with_session(s).build();
    assert_eq!(inspect(&h)["enableBar"], false);
}

#[test]
fn the_enable_bar_can_be_dismissed_and_leaves_the_workbook_off() {
    let (mut h, _) = Setup::new().with_session(from_file()).build();
    h.get_all_by_label("Dismiss").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(inspect(&h)["enableBar"], false);
    assert!(!h.state().session.python_workbook_enabled());
}

// ---------------------------------------------------------------- the ribbon tab

#[test]
fn the_state_button_shows_each_state() {
    let blocked = PythonEnvironment {
        state: PythonState::Blocked,
        diagnostic: Some(Diagnostic::new("blocked.sandboxCheck", "read the canary", None)),
        ..report(PythonState::Blocked)
    };
    let not_set_up = no_python();
    let running = report(PythonState::Running);
    for (setup, label) in [
        (Setup::new().off(), "Off"),
        (Setup::new().report(not_set_up), "Not Set Up"),
        (Setup::new(), "Ready"),
        (Setup::new().report(running), "Running"),
        (Setup::new().report(blocked), "Blocked"),
    ] {
        let (mut h, _) = setup.build();
        assert!(h.query_by_role_and_label(Role::Button, label).is_some(), "{label}");
        h.check_accessibility();
        // It opens the Environment pane, which says the same.
        click(&mut h, label);
        assert_eq!(pane(&h).as_deref(), Some(python_ui::ENVIRONMENT), "{label}");
        h.check_accessibility();
    }
}

#[test]
fn a_sandbox_that_did_not_hold_is_never_called_on() {
    let failed = PythonEnvironment {
        state: PythonState::Blocked,
        diagnostic: Some(Diagnostic::new("blocked.sandboxCheck", "read the canary", None)),
        ..report(PythonState::Blocked)
    };
    let (mut h, _) = Setup::new().report(failed).build();
    h.state_mut().run("ui.pythonEnvironment", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Reading files outside Python: blocked").is_none(), "that didn't hold");
    assert!(h.query_by_label("The sandbox didn't hold when GridCraft checked it, so Python won't run. (read the canary)").is_some());
}

#[test]
fn what_comes_from_python_is_cut_to_what_a_row_can_show() {
    let huge = "x".repeat(100_000);
    let many = PythonEnvironment { packages: vec![(huge.clone(), huge)], ..report(PythonState::Ready) };
    let (mut h, _) = Setup::new().report(many).build();
    h.state_mut().run("ui.pythonPackages", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label(&"x".repeat(200)).count(), 2, "the name and the version");
    h.check_accessibility();
}

#[test]
fn a_cell_running_is_the_running_state() {
    let (mut h, _) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "hang"})).unwrap();
    until_running(&mut h);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::Button, "Running").is_some());
    assert_eq!(inspect(&h)["state"], "running");
    assert_eq!(inspect(&h)["running"]["cell"], "A1");
    h.state_mut().run("python.stop", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Ready").is_some());
}

#[test]
fn each_button_runs_its_command() {
    let (mut h, _) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "ret 5"})).unwrap();
    settle(&mut h);
    click(&mut h, "Run Cell");
    assert!(journaled(&h, "python.run"));
    settle(&mut h);
    click(&mut h, "Run All");
    assert!(journaled(&h, "python.runAll"));
    settle(&mut h);
    click(&mut h, "Reset");
    assert!(journaled(&h, "python.reset"));
    settle(&mut h);
    click(&mut h, "Clear");
    assert!(journaled(&h, "python.clearRunLog"));
    assert_eq!(h.state().session.python_run_log().count(), 0);
    // Python Calculation.
    click(&mut h, "Python Calculation");
    h.get_by_label("Manual").click();
    h.run_steps(3);
    assert_eq!(h.state().session.python_calculation(), gridcraft_engine::PythonCalculation::Manual);
    click(&mut h, "Python Calculation");
    h.get_by_label("Automatic").click();
    h.run_steps(3);
    assert_eq!(h.state().session.python_calculation(), gridcraft_engine::PythonCalculation::Automatic);
    // Output As.
    click(&mut h, "Output As");
    h.get_by_label("Python Object").click();
    h.run_steps(3);
    assert!(journaled(&h, "python.output"));
    settle(&mut h);
    // Stop, while a cell runs.
    h.state_mut().run("python.set", json!({"cell": "A2", "code": "hang"})).unwrap();
    until_running(&mut h);
    h.run_steps(2);
    click(&mut h, "Stop");
    assert!(journaled(&h, "python.stop"));
    assert!(h.state().session.python_running().is_none());
    // Check Sandbox checks again and shows the report.
    click(&mut h, "Check Sandbox");
    assert!(journaled(&h, "python.refreshEnvironment"));
    assert_eq!(pane(&h).as_deref(), Some(python_ui::ENVIRONMENT));
    // Editor.
    click(&mut h, "Editor");
    assert!(h.state().py_window.is_some());
}

#[test]
fn insert_python_starts_python_mode() {
    let (mut h, _) = Setup::new().build();
    click(&mut h, "Insert Python");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python));
}

#[test]
fn the_buttons_for_a_cell_say_when_there_is_none() {
    // A draft on a plain cell brings the tab up with no Python cell to act on.
    let (mut h, _) = Setup::plain().build();
    h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap();
    show_tab(&mut h);
    let run = button(&h, "Run Cell");
    assert!(run.accesskit_node().is_disabled());
    run.hover();
    h.run_steps(4);
    assert!(h.query_by_label("Select a Python cell first.").is_some());
    assert!(button(&h, "Output As").accesskit_node().is_disabled());
    assert!(button(&h, "Stop").accesskit_node().is_disabled(), "nothing is running");
}

#[test]
fn initialization_opens_its_editor() {
    let (mut h, _) = Setup::new().build();
    click(&mut h, "Initialization");
    assert!(h.state().py_window.as_ref().is_some_and(|w| w.initialization));
    assert!(h.state().message.is_none());
}

#[test]
fn the_interpreter_menu_lists_what_was_found_and_writes_the_preference() {
    let (mut h, _) = Setup::new().build();
    click(&mut h, "Interpreter");
    let row = "/home/you/.gridcraft/python/bin/python  (3.12.7, uv environment)";
    assert!(h.query_by_label(row).is_some());
    h.get_by_label(row).click();
    h.run_steps(3);
    assert_eq!(h.state().ui.python.interpreter.as_deref(), Some("/home/you/.gridcraft/python/bin/python"));
    assert_eq!(turned_on(&h).1, Some(interpreter().path));
    click(&mut h, "Interpreter");
    h.get_by_label("Automatic").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.python.interpreter, None);
    assert_eq!(turned_on(&h).1, None);
}

#[test]
fn the_workbook_switch_turns_the_workbook_on_and_off() {
    let (mut h, handle) = Setup::new().with_session(from_file()).build();
    assert!(h.query_by_label("This Workbook: Disabled").is_some());
    h.get_by_label("This Workbook: Disabled").click();
    h.run_steps(3);
    assert!(h.state().session.python_workbook_enabled());
    settle(&mut h);
    assert!(handle.starts() >= 1);
    h.get_by_label("This Workbook: Enabled").click();
    h.run_steps(3);
    assert!(!h.state().session.python_workbook_enabled());
    assert!(h.query_by_label("This Workbook: Disabled").is_some());
}

#[test]
fn formulas_python_opens_the_same_things() {
    let mut setup = Setup::new();
    setup.tab = "Formulas";
    let (mut h, _) = setup.build();
    h.state_mut().run("python.set", json!({"cell": "A1", "code": "ret 5"})).unwrap();
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::Button, "Insert Python").is_some(), "Excel's item stays");
    click(&mut h, "Diagnostics");
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PROBLEMS));
    click(&mut h, "Reset");
    assert!(journaled(&h, "python.reset"));
    h.check_accessibility();
}

/// Reset, Initialization and Diagnostics keep their labels at any width; a window too narrow for the
/// tab scrolls the ribbon.
#[test]
fn formulas_keeps_the_labels_and_scrolls() {
    let overflow = |width| {
        let mut setup = Setup::new();
        setup.tab = "Formulas";
        setup.size = vec2(width, 800.0);
        let (h, _) = setup.build();
        assert!(h.query_by_role_and_label(Role::Button, "Reset").is_some(), "{width}");
        assert!(h.query_by_role_and_label(Role::Button, "Diagnostics").is_some(), "{width}");
        let width_of = |label| button(&h, label).rect().width();
        (RibbonOverflow::get(&h.ctx), [width_of("Reset"), width_of("Initialization"), width_of("Diagnostics")])
    };
    let (narrow, narrow_widths) = overflow(1000.0);
    let (wide, wide_widths) = overflow(2000.0);
    assert_eq!(narrow_widths, wide_widths, "the same buttons at either width");
    assert!(narrow_widths.iter().all(|w| *w > 50.0), "wider than an icon: {narrow_widths:?}");
    assert!(narrow.overflows(), "{narrow:?}");
    assert!(!wide.overflows(), "{wide:?}");
}

// ---------------------------------------------------------------- the tab comes and goes

fn select(h: &mut Harness, cell: &str) {
    h.state_mut().run("selection.set", json!({"range": cell})).unwrap();
    h.run_steps(3);
}

fn tabs(h: &Harness) -> Vec<&'static str> {
    contextual_tabs(h.state())
}

/// A plain cell at A1 and a Python cell at A2.
fn python_at_a2() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "A2", "code": "ret 1"})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s
}

#[test]
fn the_tab_is_only_there_on_a_python_cell_however_it_is_reached() {
    assert!(!TABS.contains(&PYTHON_TAB), "it isn't a permanent tab");
    let (mut h, _) = Setup::on(python_at_a2()).build();
    assert!(tabs(&h).is_empty(), "a plain cell");
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    assert!(h.query_by_role_and_label(Role::Tab, "Python").is_none());
    select(&mut h, "A2");
    assert_eq!(tabs(&h), [PYTHON_TAB], "by a command");
    assert!(h.query_by_role_and_label(Role::Tab, "Python").is_some());
    press(&mut h, egui::Key::ArrowUp);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A1");
    assert!(tabs(&h).is_empty(), "the keyboard, away");
    press(&mut h, egui::Key::ArrowDown);
    assert_eq!(tabs(&h), [PYTHON_TAB], "the keyboard, back");
    select(&mut h, "C5");
    assert!(control_request(&mut h, "ui.cell.click", json!({"cell": "A2"})).is_some_and(|r| r["ok"] == true));
    assert_eq!(tabs(&h), [PYTHON_TAB], "a click");
}

#[test]
fn the_tab_is_there_while_python_code_is_being_edited() {
    let (mut h, _) = Setup::plain().build();
    assert!(tabs(&h).is_empty());
    h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert_eq!(tabs(&h), [PYTHON_TAB], "a draft in the formula bar");
    h.state_mut().cancel_edit();
    h.run_steps(3);
    assert!(tabs(&h).is_empty());
    h.state_mut().run("ui.pythonEditor", json!({"on": true})).unwrap();
    h.run_steps(3);
    assert_eq!(tabs(&h), [PYTHON_TAB], "the editor window");
    h.state_mut().run("ui.pythonEditor", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(tabs(&h).is_empty());
}

/// Like Table Design and Chart Design: selecting a Python cell adds the tab and leaves the ribbon on
/// the tab it was on, so arrowing across a Python cell doesn't flip the ribbon.
#[test]
fn the_tab_doesnt_take_over_when_it_comes_up() {
    let (mut h, _) = Setup::on(python_at_a2()).build();
    select(&mut h, "A2");
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    select(&mut h, "A1");
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(2);
    click(&mut h, "Insert Python");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python), "Formulas › Python is the way in");
    assert_eq!(tabs(&h), [PYTHON_TAB]);
    assert_eq!(h.state().ui.ribbon_tab, "Formulas");
    // A click on it shows it.
    h.get_by_role_and_label(Role::Tab, "Python").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, PYTHON_TAB);
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.content_height <= o.visible_height + 0.5, "{o:?}");
}

#[test]
fn the_tab_is_the_rightmost_beside_the_other_contextual_tabs() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    s.execute("python.set", json!({"cell": "B2", "code": "ret 1"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let (mut h, _) = Setup::on(s).build();
    assert_eq!(tabs(&h), ["Table Design", PYTHON_TAB]);
    let table = h.get_by_role_and_label(Role::Tab, "Table Design").rect();
    let python = h.get_by_role_and_label(Role::Tab, "Python").rect();
    assert!(python.left() >= table.right(), "{table:?} then {python:?}");
    h.check_accessibility();
    // Chart Design comes before it too.
    h.state_mut().run("insert.chart", json!({"type": "column", "range": "A1:B3"})).unwrap();
    h.run_steps(3);
    assert_eq!(tabs(&h), ["Table Design", "Chart Design", PYTHON_TAB]);
}

#[test]
fn leaving_the_cell_takes_the_tab_away_and_leaves_its_pane() {
    let (mut h, _) = Setup::on(python_at_a2()).build();
    select(&mut h, "A2");
    show_tab(&mut h);
    click(&mut h, "What Python Can Do");
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PERMISSIONS));
    select(&mut h, "A1");
    assert_eq!(h.state().ui.ribbon_tab, "Home", "the ribbon doesn't stay on a tab that is gone");
    assert!(tabs(&h).is_empty());
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PERMISSIONS), "the pane stays open");
}

#[test]
fn what_is_reachable_without_the_tab_stays_reachable() {
    let (mut h, _) = Setup::plain().build();
    assert!(tabs(&h).is_empty());
    for (id, name) in [
        ("ui.pythonEnvironment", python_ui::ENVIRONMENT),
        ("ui.pythonPackages", python_ui::PACKAGES),
        ("ui.pythonProblems", python_ui::PROBLEMS),
        ("ui.pythonRunLog", python_ui::RUN_LOG),
        ("ui.pythonPermissions", python_ui::PERMISSIONS),
    ] {
        h.state_mut().run(id, json!({"on": true})).unwrap();
        h.run_steps(2);
        assert_eq!(pane(&h).as_deref(), Some(name));
    }
    h.get_by_label("Python: Ready").click();
    h.run_steps(3);
    assert_eq!(pane(&h).as_deref(), Some(python_ui::ENVIRONMENT), "the status bar item");
    open_options(&mut h);
    assert!(h.query_by_label("Enable Python in cells").is_some(), "File › Options › Python");
    h.state_mut().backstage = None;
    h.run_steps(2);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(2);
    click(&mut h, "Diagnostics");
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PROBLEMS), "Formulas › Python");
}

#[test]
fn on_a_system_with_no_sandbox_it_comes_and_goes_the_same_way_and_says_blocked() {
    let mut setup = Setup::on(python_at_a2()).off();
    setup.sandbox = Err("no sandbox".into());
    let (mut h, _) = setup.build();
    assert!(tabs(&h).is_empty());
    select(&mut h, "A2");
    assert_eq!(tabs(&h), [PYTHON_TAB]);
    show_tab(&mut h);
    assert!(h.query_by_role_and_label(Role::Button, "Blocked").is_some());
    select(&mut h, "A1");
    assert!(tabs(&h).is_empty());
}

#[test]
fn the_tab_stays_reachable_in_a_narrow_window() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    s.execute("python.set", json!({"cell": "B2", "code": "ret 1"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut setup = Setup::on(s);
    setup.tab = "Home";
    setup.size = vec2(640.0, 460.0);
    let (mut h, _) = setup.build();
    let python = h.get_by_role_and_label(Role::Tab, "Python").rect();
    assert!(python.right() <= 640.0 && python.width() > 30.0, "{python:?}");
    let table = h.get_by_role_and_label(Role::Tab, "Table Design").rect();
    assert!(table.left() >= 0.0 && table.right() <= python.left(), "{table:?}");
    h.get_by_role_and_label(Role::Tab, "Python").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, PYTHON_TAB);
    assert!(h.query_by_role_and_label(Role::Button, "Ready").is_some());
    h.check_accessibility();
}

#[test]
fn the_key_tip_answers_only_while_the_tab_is_up() {
    assert_eq!(keytip_table::tab_sequence(PYTHON_TAB), Some("Y"));
    let (mut h, _) = Setup::on(python_at_a2()).build();
    press(&mut h, egui::Key::F10);
    press(&mut h, egui::Key::Y);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "no tab to go to");
    assert_eq!(h.state().keytips.prefix(), Some(""), "the letter leads nowhere, and key tips stay armed");
    press(&mut h, egui::Key::M);
    assert_eq!(h.state().ui.ribbon_tab, "Formulas");
    press(&mut h, egui::Key::Escape);
    press(&mut h, egui::Key::Escape);
    select(&mut h, "A2");
    press(&mut h, egui::Key::F10);
    press(&mut h, egui::Key::Y);
    assert_eq!(h.state().ui.ribbon_tab, PYTHON_TAB);
    assert_eq!(h.state().keytips.prefix(), Some("Y"));
}

// ---------------------------------------------------------------- a system with no sandbox

fn no_sandbox() -> Harness {
    let mut setup = Setup::new().off();
    setup.sandbox = Err("GridCraft has no sandbox for this test".into());
    setup.build().0
}

const NO_SANDBOX_TIP: &str = "Python cells need a sandbox, and GridCraft has none for this system. (GridCraft has no sandbox for this test)";

#[test]
fn a_blocked_system_shows_the_tab_with_its_controls_off_and_why() {
    let mut h = no_sandbox();
    assert_eq!(inspect(&h)["state"], "blocked");
    assert!(h.query_by_role_and_label(Role::Button, "Blocked").is_some(), "the state button says so whatever the setting");
    for label in ["Run All", "Reset", "Packages", "Stop", "Python Calculation", "Interpreter"] {
        assert!(button(&h, label).accesskit_node().is_disabled(), "{label} is off");
    }
    assert!(!button(&h, "Insert Python").accesskit_node().is_disabled(), "code can still be written");
    assert!(!button(&h, "What Python Can Do").accesskit_node().is_disabled());
    for label in ["Run All", "Packages"] {
        button(&h, label).hover();
        h.run_steps(4);
        assert!(h.query_by_label(NO_SANDBOX_TIP).is_some(), "{label}'s tooltip gives the reason");
    }
    h.check_accessibility();
    // The panes say why too, and the Options page can't turn it on.
    click(&mut h, "What Python Can Do");
    assert!(h.query_by_label(NO_SANDBOX_TIP).is_some());
    click(&mut h, "Blocked");
    assert!(h.query_by_label("Python cells need a sandbox, and GridCraft has none for this system.").is_some());
    open_options(&mut h);
    assert!(h.get_by_label("Enable Python in cells").accesskit_node().is_disabled());
    assert!(h.query_by_label(NO_SANDBOX_TIP).is_some(), "the sandbox's status, in words");
}

// ---------------------------------------------------------------- the panes

#[test]
fn panes_open_and_close_by_button_and_by_command() {
    let (mut h, _) = Setup::new().build();
    for (label, id, pane_name, key) in [
        ("Packages", "ui.pythonPackages", python_ui::PACKAGES, "packages"),
        ("Problems", "ui.pythonProblems", python_ui::PROBLEMS, "problems"),
        ("Run Log", "ui.pythonRunLog", python_ui::RUN_LOG, "runLog"),
        ("What Python Can Do", "ui.pythonPermissions", python_ui::PERMISSIONS, "permissions"),
    ] {
        click(&mut h, label);
        assert_eq!(pane(&h).as_deref(), Some(pane_name));
        assert_eq!(inspect(&h)["panes"][key], true, "{key}");
        h.check_accessibility();
        click(&mut h, label);
        assert_eq!(pane(&h), None, "the same button closes it");
        assert_eq!(h.state_mut().run(id, json!({"on": true})).unwrap()["open"], true);
        h.run_steps(2);
        assert_eq!(pane(&h).as_deref(), Some(pane_name));
        assert_eq!(h.state_mut().run(id, json!({})).unwrap()["open"], false, "without `on` it switches");
        assert!(h.state_mut().run(id, json!({"on": 3})).is_err());
        h.run_steps(2);
    }
    assert_eq!(h.state_mut().run("ui.pythonEnvironment", json!({"on": true})).unwrap()["open"], true);
    h.run_steps(2);
    assert_eq!(inspect(&h)["panes"]["environment"], true);
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert_eq!(pane(&h), None);
}

#[test]
fn the_environment_pane_says_everything_about_the_python_that_runs() {
    let (mut h, _) = Setup::new().build();
    h.state_mut().run("ui.pythonEnvironment", json!({})).unwrap();
    h.run_steps(3);
    for text in [
        "/home/you/.gridcraft/python/bin/python",
        "/home/you/.local/share/uv/python/cpython-3.12.7/bin/python3.12",
        "Python 3.12.7, uv environment",
        "Reading files outside Python: blocked",
        "a test sandbox",
        "/home/you/.gridcraft/python/lib/python3.12/site-packages",
        "/home/you/.gridcraft/python",
        "/home/you/.local/bin/uv",
    ] {
        assert!(h.query_by_label(text).is_some(), "{text}");
    }
    h.check_accessibility();
}

#[test]
fn the_environment_pane_says_how_to_get_uv_and_acts_on_the_reason() {
    let (mut h, _) = Setup::new().report(no_python()).build();
    h.state_mut().run("ui.pythonEnvironment", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("curl -LsSf https://astral.sh/uv/install.sh | sh").is_some());
    assert!(h.query_by_label("GridCraft can't find Python 3.11 or newer. Set one up with uv, or choose one.").is_some());
    click(&mut h, "Choose Interpreter");
    assert_eq!(inspect(&h)["optionsPage"], true, "the reason's action goes where it can be fixed");
}

fn many_packages(n: usize) -> PythonEnvironment {
    PythonEnvironment { packages: (0..n).map(|i| (format!("package-{i:05}"), format!("1.{i}"))).collect(), ..report(PythonState::Ready) }
}

#[test]
fn the_packages_list_filters_and_draws_only_what_shows() {
    let (mut h, _) = Setup::new().report(many_packages(1000)).build();
    h.state_mut().run("ui.pythonPackages", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Packages: 1000").is_some());
    assert!(h.query_by_label("package-00000").is_some());
    let drawn = h.state().python.rows_drawn();
    assert!(drawn > 5 && drawn < 60, "{drawn} rows of 1,000 drawn");
    h.get_by_role_and_label(Role::TextInput, "Filter").click();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("00999".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Packages: 1").is_some());
    assert!(h.query_by_label("package-00999").is_some());
    assert_eq!(h.state().python.rows_drawn(), 1);
}

#[test]
fn the_packages_list_handles_having_none() {
    let (mut h, _) = Setup::new().report(no_python()).build();
    h.state_mut().run("ui.pythonPackages", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Packages: 0").is_some());
    h.check_accessibility();
}

#[test]
fn copy_install_command_speaks_uv_and_takes_only_package_names() {
    let (mut h, _) = Setup::new().build();
    h.state_mut().run("ui.pythonPackages", json!({})).unwrap();
    h.run_steps(3);
    let copy = "Copy Install Command";
    assert!(button(&h, copy).accesskit_node().is_disabled(), "no name yet");
    h.get_by_role_and_label(Role::TextInput, "Package name").click();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("; rm -rf x".into()));
    h.run_steps(3);
    assert!(button(&h, copy).accesskit_node().is_disabled(), "that isn't a package");
    h.input_mut().events.push(Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    h.input_mut().events.push(Event::Text("polars".into()));
    h.run_steps(3);
    button(&h, copy).click();
    h.step();
    let expected = "uv pip install --compile-bytecode --python /home/you/.gridcraft/python/bin/python polars";
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == expected)));
    h.run_steps(2);
    // pip is behind the secondary choice.
    let pip = "/home/you/.gridcraft/python/bin/python -m pip install polars";
    assert!(h.query_by_label(pip).is_none());
    h.get_by_label("Other package managers").click();
    h.run_steps(3);
    assert!(h.query_by_label(pip).is_some());
}

#[test]
fn problems_are_grouped_by_kind_and_a_click_goes_to_the_cell() {
    let (mut h, _) = Setup::new().build();
    for (cell, code) in [("A1", "ret 1"), ("A2", "fail"), ("C7", "fail")] {
        h.state_mut().run("python.set", json!({"cell": cell, "code": code})).unwrap();
    }
    settle(&mut h);
    h.state_mut().run("ui.pythonProblems", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("python.exception (2)").is_some());
    assert_eq!(h.get_all_by_label("ValueError: boom").count(), 2);
    assert_eq!(h.get_all_by_label("The code raised an error.").count(), 2);
    h.get_by_label("Sheet1!C7").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "C7");
    assert!(journaled(&h, "selection.set"), "through the command");
}

#[test]
fn the_problems_list_handles_having_none_and_having_many() {
    let (mut h, _) = Setup::plain().build();
    h.state_mut().run("ui.pythonProblems", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("No problems.").is_some());
    // Under manual calculation every new cell is stale, with something to say.
    h.state_mut().run("python.calculation", json!({"mode": "manual"})).unwrap();
    for row in 1..=1000 {
        h.state_mut().run("python.set", json!({"cell": format!("B{row}"), "code": "ret 1"})).unwrap();
    }
    h.run_steps(3);
    assert!(h.query_by_label("state.stale (1000)").is_some());
    let drawn = h.state().python.rows_drawn();
    assert!(drawn > 3 && drawn < 40, "{drawn} rows of 1,001 drawn");
}

#[test]
fn the_run_log_shows_each_run_and_clears_through_a_command() {
    let (mut h, _) = Setup::plain().build();
    h.state_mut().run("ui.pythonRunLog", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("No runs yet.").is_some());
    for (cell, code) in [("A1", "print hello"), ("A2", "fail")] {
        h.state_mut().run("python.set", json!({"cell": cell, "code": code})).unwrap();
    }
    settle(&mut h);
    show_tab(&mut h);
    let run = h.state().session.python_run_log().next().unwrap().clone();
    assert_eq!(run.stdout, "hello\n");
    assert!(h.query_by_role_and_label(Role::Button, "Sheet1!A1").is_some());
    assert!(h.query_by_label("hello").is_some(), "the printed output");
    assert!(h.query_all_by_label_contains("python.exception").next().is_some(), "the diagnostic's id");
    // A run shows all it printed, and goes to its cell.
    button(&h, "Sheet1!A1").click();
    h.run_steps(3);
    click(&mut h, "Go To Cell");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A1");
    // The pane's Clear, and the same on the ribbon.
    h.get_all_by_label("Clear").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().session.python_run_log().count(), 0);
    assert!(h.query_by_label("No runs yet.").is_some());
    h.check_accessibility();
}

#[test]
fn what_python_can_do_lists_both_and_names_what_enforces_each() {
    let (mut h, _) = Setup::new().build();
    click(&mut h, "What Python Can Do");
    for text in [
        "Read cells through xl()",
        "Import installed packages",
        "Compute",
        "Print",
        "Use threads",
        "Read your files",
        "Write files",
        "Use the network",
        "Start programs",
        "See environment variables",
        "Open windows",
        "Draw plots. The data goes to a sheet and a chart is built from it.",
        "GridCraft starts Python with an empty environment",
    ] {
        assert!(h.query_by_label(text).is_some(), "{text}");
    }
    h.check_accessibility();
}

// ---------------------------------------------------------------- the status bar

#[test]
fn the_status_bar_has_no_item_for_a_person_with_neither() {
    let (h, _) = Setup::plain().off().build();
    assert!(h.query_by_label_contains("Python: ").is_none());
}

#[test]
fn the_status_bar_says_the_state_and_opens_the_environment_pane() {
    let (mut h, _) = Setup::new().build();
    assert!(h.query_by_label("Python: Ready").is_some());
    h.get_by_label("Python: Ready").click();
    h.run_steps(3);
    assert_eq!(pane(&h).as_deref(), Some(python_ui::ENVIRONMENT));
}

#[test]
fn the_status_bar_item_shows_for_a_workbook_with_python_cells_with_python_off() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "A1", "code": "ret 5"})).unwrap();
    let (h, _) = Setup::new().off().with_session(s).build();
    assert!(h.query_by_label("Python: Off").is_some());
}

#[test]
fn the_status_bar_names_the_running_cell_and_its_time() {
    let (mut h, _) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "B3", "code": "hang"})).unwrap();
    until_running(&mut h);
    h.run_steps(2);
    assert!(h.query_by_label("Python: running Sheet1!B3, 0 s").is_some());
    h.state_mut().run("python.stop", json!({})).unwrap();
    h.run_steps(2);
}

// ---------------------------------------------------------------- a narrow window

#[test]
fn the_options_page_keeps_browse_in_a_narrow_window() {
    let mut setup = Setup::new();
    setup.size = vec2(640.0, 420.0);
    setup.services.pick_interpreter = Some(Box::new(|| None));
    let (mut h, _) = setup.build();
    open_options(&mut h);
    let browse = h.get_by_label("Browse…").rect();
    assert!(browse.right() <= 640.0, "{browse:?}");
    h.check_accessibility();
}

#[test]
fn the_tab_works_in_a_narrow_window() {
    let mut setup = Setup::new().with_session(from_file());
    setup.size = vec2(620.0, 500.0);
    let (mut h, _) = setup.build();
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.overflows(), "its groups don't fit, so the ribbon scrolls");
    assert!(o.content_height <= o.visible_height + 0.5);
    // The last button comes into view with the arrows, and works.
    for _ in 0..8 {
        if h.query_by_label("Scroll ribbon right").is_none() {
            break;
        }
        h.get_by_label("Scroll ribbon right").click();
        h.run_steps(3);
    }
    let last = button(&h, "Run Log").rect();
    assert!(last.right() <= 620.0, "{last:?}");
    click(&mut h, "Run Log");
    assert_eq!(pane(&h).as_deref(), Some(python_ui::RUN_LOG));
    h.check_accessibility();
    // The Enable bar keeps its button in the window, and its text wraps ahead of it.
    let enable = button(&h, ENABLE).rect();
    assert!(enable.right() <= 620.0 && enable.left() > 0.0, "{enable:?}");
    click(&mut h, ENABLE);
    assert!(h.state().session.python_workbook_enabled());
}

// ---------------------------------------------------------------- key tips

fn press(h: &mut Harness, key: egui::Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn key_tips_reach_the_tab_and_open_its_panes() {
    let mut setup = Setup::new();
    setup.tab = "Home";
    let (mut h, _) = setup.build();
    press(&mut h, egui::Key::F10);
    press(&mut h, egui::Key::Y);
    assert_eq!(h.state().ui.ribbon_tab, "Python");
    press(&mut h, egui::Key::W);
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PERMISSIONS));
}

#[test]
fn no_key_tip_reaches_a_switch() {
    // A key tip is something the control channel can press (`ui.key`).
    for (sequence, _) in keytip_table::SEQUENCES {
        if !sequence.starts_with('Y') {
            continue;
        }
        let target = match keytip_table::lookup(sequence) {
            Some(keytip_table::Entry::Finish(gridcraft_ui_egui::keytips::Action::Control(id) | gridcraft_ui_egui::keytips::Action::Command(id))) => {
                id
            }
            _ => continue,
        };
        assert!(!["workbook", "enable", "enabled", "switch"].iter().any(|word| target.to_lowercase().contains(word)), "{sequence}: {target}");
    }
}

// ---------------------------------------------------------------- nothing programmatic turns Python on

/// Hands a request to the control channel's handler, then applies what it queued as the app's frames
/// do: one synthetic batch per frame, through `raw_input_hook`.
fn control_request(h: &mut Harness, method: &str, params: Json) -> Option<Json> {
    let ctx = h.ctx.clone();
    let out = control::handle(h.state_mut(), &ctx, method, &params);
    pump(h);
    match out {
        Outcome::Done(v) => Some(v),
        Outcome::Screenshot { .. } | Outcome::Hold(_) => None,
    }
}

fn pump(h: &mut Harness) {
    for _ in 0..16 {
        let ctx = h.ctx.clone();
        let mut raw = egui::RawInput::default();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        h.input_mut().events.extend(raw.events);
        h.step();
    }
}

fn control_click(h: &mut Harness, at: Pos2) {
    assert!(control_request(h, "ui.click", json!({"x": at.x, "y": at.y})).is_some_and(|r| r["ok"] == true));
}

/// A person's click, at the same place: what the refusals are measured against.
fn person_click(h: &mut Harness, at: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
}

#[test]
fn a_synthetic_click_works_on_what_is_safe_and_is_refused_on_the_enable_checkbox() {
    let (mut h, _) = Setup::new().off().build();
    // Synthetic clicks do reach widgets, here a pane's button.
    let problems = button(&h, "Problems").rect().center();
    control_click(&mut h, problems);
    assert_eq!(pane(&h).as_deref(), Some(python_ui::PROBLEMS), "the click arrived");
    open_options(&mut h);
    let at = h.get_by_label("Enable Python in cells").rect().center();
    let before = turned_on(&h);
    control_click(&mut h, at);
    assert_eq!(turned_on(&h), before, "ui.click");
    let to = at + vec2(1.0, 0.0);
    assert!(control_request(&mut h, "ui.drag", json!({"x": at.x, "y": at.y, "toX": to.x, "toY": to.y})).is_some());
    assert_eq!(turned_on(&h), before, "ui.drag");
    // A person's click at the same place does it: the place was right, and the refusal doesn't last.
    pump(&mut h);
    person_click(&mut h, at);
    assert_ne!(turned_on(&h), before);
    assert!(h.state().ui.python.enabled);
    // And nothing programmatic turns it off either.
    let on = turned_on(&h);
    control_click(&mut h, at);
    assert_eq!(turned_on(&h), on);
}

#[test]
fn keys_from_the_control_channel_are_refused_on_a_focused_switch() {
    let (mut h, _) = Setup::new().off().build();
    open_options(&mut h);
    h.get_by_label("Enable Python in cells").focus();
    h.run_steps(2);
    let before = turned_on(&h);
    for key in ["space", "enter"] {
        assert!(control_request(&mut h, "ui.key", json!({"key": key})).is_some());
        assert_eq!(turned_on(&h), before, "{key}");
    }
    pump(&mut h);
    h.get_by_label("Enable Python in cells").focus();
    h.run_steps(2);
    press(&mut h, egui::Key::Space);
    assert!(h.state().ui.python.enabled, "a person's key does it");
}

#[test]
fn a_synthetic_click_is_refused_on_the_enable_bar_and_the_workbook_switch() {
    let (mut h, handle) = Setup::new().with_session(from_file()).build();
    let before = turned_on(&h);
    assert!(!before.2);
    let enable = button(&h, ENABLE).rect().center();
    control_click(&mut h, enable);
    assert_eq!(turned_on(&h), before, "the Enable bar");
    let switch = h.get_by_label("This Workbook: Disabled").rect().center();
    control_click(&mut h, switch);
    assert_eq!(turned_on(&h), before, "the ribbon's switch");
    // The panel's checkbox.
    h.state_mut().run("ui.pythonPermissions", json!({"on": true})).unwrap();
    h.run_steps(3);
    let checkbox = "Run this workbook's Python cells";
    for _ in 0..12 {
        let rect = h.get_by_label(checkbox).rect();
        if rect.bottom() < 780.0 {
            break;
        }
        h.get_by_label(checkbox).scroll_to_me();
        h.run_steps(2);
    }
    let at = h.get_by_label(checkbox).rect().center();
    control_click(&mut h, at);
    assert_eq!(turned_on(&h), before, "the pane's switch");
    assert_eq!(handle.starts(), 0, "nothing ran");
    pump(&mut h);
    person_click(&mut h, enable);
    assert!(h.state().session.python_workbook_enabled(), "a person's click does it");
}

#[test]
fn a_synthetic_click_is_refused_on_the_interpreter_choices_and_opens_no_picker() {
    let picked = Rc::new(Cell::new(0));
    let count = picked.clone();
    let mut setup = Setup::new();
    setup.services.pick_interpreter = Some(Box::new(move || {
        count.set(count.get() + 1);
        Some("/planted/python".into())
    }));
    let (mut h, _) = setup.build();
    open_options(&mut h);
    let before = turned_on(&h);
    let browse = h.get_by_label("Browse…").rect().center();
    control_click(&mut h, browse);
    assert_eq!(picked.get(), 0, "no picker opened");
    assert_eq!(turned_on(&h), before);
    // The list, from the ribbon: a person opens it, and a click sent into it does nothing.
    h.state_mut().backstage = None;
    h.run_steps(3);
    click(&mut h, "Interpreter");
    let row = h.get_by_label("/home/you/.gridcraft/python/bin/python  (3.12.7, uv environment)").rect().center();
    control_click(&mut h, row);
    assert_eq!(turned_on(&h), before);
    assert_eq!(h.state().ui.python.interpreter, None);
    let browse = h.query_by_label("Browse…").map(|n| n.rect().center());
    if let Some(browse) = browse {
        control_click(&mut h, browse);
    }
    assert_eq!(picked.get(), 0);
    assert_eq!(h.state().ui.python.interpreter, None);
}

#[test]
fn typed_input_from_the_control_channel_is_refused_on_the_limits() {
    let (mut h, _) = Setup::new().build();
    open_options(&mut h);
    let before = h.state().ui.python.clone();
    let at = h.get_by_label("Time limit for one cell").rect().center();
    control_click(&mut h, at);
    assert!(control_request(&mut h, "ui.text", json!({"text": "7"})).is_some());
    assert!(control_request(&mut h, "ui.key", json!({"key": "enter"})).is_some());
    assert_eq!(h.state().ui.python, before);
}

#[test]
fn no_method_or_command_writes_the_settings() {
    let (mut h, _) = Setup::new().off().with_session(from_file()).build();
    let before = turned_on(&h);
    let prefs = h.state().ui.python.clone();
    let everything = json!({
        "on": true, "enabled": true, "enable": true, "python": {"enabled": true, "interpreter": "/planted/python"},
        "interpreter": "/planted/python", "workbook": true, "value": true, "field": "enabled",
    });
    // The methods that write the interface's state.
    for method in ["ui.set", "ui.dialog.set", "ui.dialog.confirm", "ui.ribbon", "ui.notice"] {
        control_request(&mut h, method, everything.clone());
        assert_eq!((turned_on(&h), &h.state().ui.python), (before.clone(), &prefs), "{method}");
    }
    // The dialogs and pages they open are only pages.
    for (method, params) in [
        ("ui.dialog", json!({"name": "options"})),
        ("ui.dialog", json!({"name": "backstage", "page": "options"})),
        ("ui.backstage", json!({"page": "options", "section": "python", "enabled": true})),
    ] {
        control_request(&mut h, method, params);
        assert_eq!((turned_on(&h), &h.state().ui.python), (before.clone(), &prefs), "{method}");
    }
    h.state_mut().backstage = None;
    // There is no such command, by any name it might have.
    for id in
        ["python.enable", "python.setEnabled", "python.enableWorkbook", "python.setInterpreter", "ui.pythonEnable", "options.python", "ui.python"]
    {
        let r = control_request(&mut h, "engine.execute", json!({"command": id, "params": everything})).unwrap();
        assert_eq!(r["ok"], false, "{id}");
        let r = control_request(&mut h, id, everything.clone()).unwrap();
        assert_eq!(r["ok"], false, "{id}");
    }
    // And no command that does exist does it, with every param it might read.
    let ids: Vec<String> = match control_request(&mut h, "engine.commands", json!({"search": "python"})) {
        Some(r) => r["result"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect(),
        None => Vec::new(),
    };
    assert!(ids.len() > 10, "{ids:?}");
    for id in ids {
        control_request(&mut h, "engine.execute", json!({"command": id, "params": everything}));
        assert_eq!((turned_on(&h), &h.state().ui.python), (before.clone(), &prefs), "{id}");
    }
}

#[test]
fn ui_inspect_reports_the_state_and_the_panes() {
    let (mut h, _) = Setup::new().with_session(from_file()).build();
    let python = inspect(&h);
    assert_eq!(python["state"], "ready");
    assert_eq!(python["enabled"], true);
    assert_eq!(python["workbookEnabled"], false);
    assert_eq!(python["hasCells"], true);
    assert_eq!(python["reason"], "off.workbook");
    assert_eq!(python["calculation"], "automatic");
    assert_eq!(python["interpreter"], "/home/you/.gridcraft/python/bin/python");
    assert_eq!(python["panes"]["environment"], false);
    h.state_mut().run("ui.pythonEnvironment", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(inspect(&h)["panes"]["environment"], true);
    let listed = match control_request(&mut h, "engine.commands", json!({"search": "ui.pythonPackages"})) {
        Some(r) => r["result"].as_array().unwrap().len(),
        None => 0,
    };
    assert_eq!(listed, 1, "the panes' commands are listed");
}

// ---------------------------------------------------------------- every language

#[test]
fn the_tab_panes_and_page_draw_in_every_language() {
    let languages = gridcraft_ui_egui::i18n::Language::ALL.into_iter().filter(|l| {
        gridcraft_ui_egui::theme::craft_fonts_loaded()
            || !matches!(l, gridcraft_ui_egui::i18n::Language::Ja | gridcraft_ui_egui::i18n::Language::Ko | gridcraft_ui_egui::i18n::Language::Zh)
    });
    let (mut h, _) = Setup::new().report(no_python()).with_session(from_file()).build();
    for language in languages {
        h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
        h.run_steps(2);
        h.check_accessibility();
        for id in ["ui.pythonEnvironment", "ui.pythonPackages", "ui.pythonProblems", "ui.pythonRunLog", "ui.pythonPermissions"] {
            h.state_mut().run(id, json!({"on": true})).unwrap();
            h.run_steps(2);
            h.check_accessibility();
        }
        h.state_mut().run("ui.backstage", json!({"page": "options", "section": "python"})).unwrap();
        h.run_steps(2);
        h.check_accessibility();
        h.state_mut().run("ui.backstage", json!({"page": "close"})).unwrap();
        h.run_steps(1);
    }
}

// ---------------------------------------------------------------- the formula bar

fn bar(h: &Harness) -> gridcraft_ui_egui::formula_bar::BarLayout {
    gridcraft_ui_egui::formula_bar::BarLayout::get(&h.ctx)
}

fn click_at(h: &mut Harness, at: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
}

fn select_cell(h: &mut Harness, cell: &str) {
    h.state_mut().run("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(30);
}

fn runs_of(h: &Harness) -> usize {
    h.state().session.python_run_log().count()
}

/// On a Python cell the PY badge takes the place of fx, and Run Cell sits a line below it. Everywhere
/// else fx is there, as in Excel, and Run Cell isn't.
#[test]
fn on_a_python_cell_py_replaces_fx_with_run_cell_below_it() {
    let (mut h, _handle) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "ret 7"})).unwrap();
    settle(&mut h);
    select_cell(&mut h, "H9");
    let plain = bar(&h);
    let fx = plain.fx.expect("fx on a plain cell");
    assert!(plain.run_cell.is_none());
    assert_eq!(plain.lines, 1);
    select_cell(&mut h, "B2");
    let on_python = bar(&h);
    assert!(on_python.fx.is_none(), "PY replaces fx");
    let run = on_python.run_cell.expect("Run Cell on a Python cell");
    // The Python Output menu comes first, between the Enter button and PY.
    assert!((run.left() - fx.left()).abs() < 44.0, "beside fx's place: {run:?} and {fx:?}");
    assert!(run.top() > fx.top() + 10.0, "a line below the badge: {run:?}");
    assert_eq!(on_python.lines, 1, "one line of code, and the bar still has Run Cell's line");
    assert!(run.bottom() <= on_python.height + on_python.name_box.top() + 3.0, "it fits: {run:?} in {}", on_python.height);
    select_cell(&mut h, "H9");
    assert!(bar(&h).fx.is_some(), "fx comes back");
}

#[test]
fn run_cell_in_the_bar_runs_the_cell() {
    let (mut h, _handle) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "ret 7"})).unwrap();
    settle(&mut h);
    select_cell(&mut h, "B2");
    let before = runs_of(&h);
    let at = bar(&h).run_cell.expect("Run Cell").center();
    click_at(&mut h, at);
    settle(&mut h);
    assert_eq!(runs_of(&h), before + 1);
}

/// Pressed on a draft, Run Cell enters the code and runs it, also under manual calculation.
#[test]
fn run_cell_on_a_draft_enters_the_code_and_runs_it() {
    let (mut h, _handle) = Setup::new().build();
    h.state_mut().run("python.calculation", json!({"mode": "manual"})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "ret 1"})).unwrap();
    settle(&mut h);
    select_cell(&mut h, "B2");
    h.state_mut().toggle_python_mode();
    h.run_steps(30);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python), "a draft is open");
    h.state_mut().editor.as_mut().unwrap().text = "ret 9".into();
    h.run_steps(2);
    let at = bar(&h).run_cell.expect("Run Cell").center();
    click_at(&mut h, at);
    settle(&mut h);
    assert!(h.state().editor.is_none());
    let value = h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("B2").unwrap());
    assert_eq!(value, gridcraft_engine::core::Value::Number(9.0));
}

/// A cell a Python result spilled into shows the code in the code's font, greyed and read-only. A
/// formula's spill stays in the interface's font.
#[test]
fn a_python_spill_shows_its_code_in_the_codes_font() {
    let (mut h, _handle) = Setup::new().build();
    h.state_mut().run("python.set", json!({"cell": "D4", "code": "spill"})).unwrap();
    settle(&mut h);
    select_cell(&mut h, "D5");
    assert!(bar(&h).code_font, "the spill child");
    assert!(h.state().editor.is_none());
    let field = bar(&h).field.center();
    click_at(&mut h, field);
    assert!(h.state().editor.is_none(), "read-only: the code is edited at its anchor");
    h.state_mut().run("cell.set", json!({"cell": "F4", "input": "=SEQUENCE(2)"})).unwrap();
    h.run_steps(3);
    select_cell(&mut h, "F5");
    assert!(!bar(&h).code_font, "a formula's spill child");
    select_cell(&mut h, "H9");
    assert!(!bar(&h).code_font, "a plain cell");
    select_cell(&mut h, "D4");
    assert!(bar(&h).code_font, "the Python cell itself");
}
