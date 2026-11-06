//! Turning Python on and understanding it: the preference and what the session gets from it, the
//! Enable bar, the status bar item, the interpreter list and the words all of them share. The
//! Options page is `backstage/python.rs`, the panes are `python_ui/panes.rs`, the ribbon tab is
//! `ribbon/python.rs`.
//!
//! # Who can turn Python on
//!
//! Only a person, with the widgets here: "Enable Python in cells", the interpreter, the limits, and
//! the workbook's switch. None of them is a command. Everything that writes them goes through
//! [`write_settings`] or [`set_workbook`], and both refuse while [`locked`]: a synthetic event from
//! the control channel (`ui.click`, `ui.drag`, `ui.key`, `ui.text`) is queued or was just applied.
//! The widgets are drawn disabled for those frames too, so such a click never reaches them.
//!
//! This keeps out what talks to the running app: commands, agents, MCP, the control channel, macros
//! and files. Its limit: someone who can edit the preferences file on disk, or who operates the real
//! mouse and keyboard or the system's accessibility interface, acts as the person does, and nothing
//! here can tell the difference.

use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use egui::{Label, RichText, Sense, Ui};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::python::{
    self, Action, Diagnostic, Interpreter, InterpreterKind, Limits, MAX_TIMEOUT, MIN_VERSION, TransportFactory, install_name, pip_install_command,
    uv_install_commands, uv_setup_commands,
};
use gridcraft_engine::{FoundPython, PythonCapability, PythonEnvironment, PythonSearch, PythonState};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n::Language;
use crate::icons::Icon;
use crate::theme::{self, Tokens};

pub mod checks;
pub mod panes;

/// How often the app wakes while Python works, so results appear without input. An idle app stays
/// idle: nothing asks for a repaint then.
pub(crate) const BUSY_REPAINT: Duration = Duration::from_millis(100);

/// Frames the switches stay refused after the last synthetic input.
const LOCK_FRAMES: u64 = 2;

/// What the time limit field takes, in seconds. The upper end is what the client accepts.
pub const TIME_LIMIT_SECS: RangeInclusive<u64> = 1..=MAX_TIMEOUT.as_secs();

/// What the memory limit field takes, in MiB: below this a Python doesn't start with common
/// packages, and above it the limit stops meaning anything on any machine.
pub const MEMORY_LIMIT_MB: RangeInclusive<u64> = 128..=262_144;

const MIB: u64 = 1024 * 1024;

/// The panes, as `app.grid.pane` names them.
pub const ENVIRONMENT: &str = "pythonEnvironment";

pub const PACKAGES: &str = "pythonPackages";

pub const PROBLEMS: &str = "pythonProblems";

pub const RUN_LOG: &str = "pythonRunLog";

pub const PERMISSIONS: &str = "pythonPermissions";

/// The person's Python settings, kept with the other preferences (`UiState::python`). Python is off
/// until a person turns it on: an unreadable value reads as the default, so off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PythonPrefs {
    /// Enable Python in cells.
    pub enabled: bool,
    /// The interpreter chosen by path; without one, Automatic.
    pub interpreter: Option<String>,
    pub time_limit_secs: u64,
    pub memory_limit_mb: u64,
}

impl Default for PythonPrefs {
    fn default() -> Self {
        let limits = Limits::default();
        PythonPrefs { enabled: false, interpreter: None, time_limit_secs: limits.timeout.as_secs(), memory_limit_mb: limits.memory_bytes / MIB }
    }
}

impl PythonPrefs {
    /// The limits a worker gets, within what the fields take, whatever a saved file says.
    pub fn limits(&self) -> Limits {
        let time = self.time_limit_secs.clamp(*TIME_LIMIT_SECS.start(), *TIME_LIMIT_SECS.end());
        let memory = self.memory_limit_mb.clamp(*MEMORY_LIMIT_MB.start(), *MEMORY_LIMIT_MB.end());
        Limits { timeout: Duration::from_secs(time), memory_bytes: memory.saturating_mul(MIB), ..Limits::default() }
    }
}

/// Reads saved settings leniently: anything malformed reads as the default, so Python is off and the
/// rest of the preferences survive.
pub fn lenient_prefs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PythonPrefs, D::Error> {
    let v = Json::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// The search for Pythons, which runs on a thread of its own.
#[derive(Default)]
enum Search {
    #[default]
    NotStarted,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Running(Receiver<PythonSearch>, bool),
    Done(PythonSearch, bool),
}

/// The interface's Python state. What the person chose is in `UiState::python`.
#[derive(Default)]
pub struct PythonUi {
    /// What the session was last given.
    given: Option<PythonPrefs>,
    /// Whether this system can confine a worker, asked once.
    sandbox: Option<Result<String, String>>,
    search: Search,
    /// The frame the switches stay refused until, after synthetic input.
    locked_until: Option<u64>,
    /// Workbooks whose Enable bar was dismissed.
    bar_dismissed: HashSet<u64>,
    /// What each pane keeps between frames.
    pub(crate) panes: panes::PaneState,
    /// The checks of the code in the formula bar and in the editor window.
    #[doc(hidden)]
    pub bar_checks: checks::CodeChecks,
    #[doc(hidden)]
    pub window_checks: checks::CodeChecks,
    /// The names of the installed packages, for the package list it was made from.
    pub(crate) installed: Option<(u64, Option<std::sync::Arc<Vec<String>>>)>,
    /// The card whose printed output is showing.
    pub(crate) card_output: Option<crate::python_card::Card>,
    /// How wide someone dragged the card of a picture.
    pub(crate) card_image_width: Option<f32>,
    /// Workers from this factory instead of a real interpreter (tests).
    #[doc(hidden)]
    pub transport: Option<TransportFactory>,
    /// This report instead of a check of Python (tests).
    #[doc(hidden)]
    pub report: Option<PythonEnvironment>,
    /// This answer instead of a search (tests).
    #[doc(hidden)]
    pub search_answer: Option<PythonSearch>,
}

impl PythonUi {
    /// For tests: this answer about the sandbox, on any system.
    #[doc(hidden)]
    pub fn set_sandbox(&mut self, answer: Result<String, String>) {
        self.sandbox = Some(answer);
    }

    /// For tests: gives the session a new capability at the next frame.
    #[doc(hidden)]
    pub fn give_again(&mut self) {
        self.given = None;
    }

    /// Why this system can't confine a worker, if it can't.
    pub fn no_sandbox(&self) -> Option<&str> {
        self.sandbox.as_ref().and_then(|s| s.as_ref().err()).map(String::as_str)
    }

    /// Whether the Pythons are being looked for.
    pub fn searching(&self) -> bool {
        matches!(self.search, Search::Running(..))
    }

    /// What the last search found.
    pub fn found(&self) -> Option<&PythonSearch> {
        match &self.search {
            Search::Done(found, _) => Some(found),
            _ => None,
        }
    }
}

/// What the app does for Python every frame, from `SheetApp::logic`: gives the session what the
/// person chose when that changed, and picks up what is ready. Nothing here waits, so a frame never
/// blocks on Python.
pub fn tick(app: &mut SheetApp, ctx: &egui::Context) {
    if app.python.sandbox.is_none() {
        app.python.sandbox = Some(python::sandbox_support());
    }
    if app.synthetic_pending || !app.synthetic.is_empty() {
        app.python.locked_until = Some(ctx.cumulative_frame_nr().saturating_add(LOCK_FRAMES));
    }
    sync(app);
    if app.session.python_poll(Duration::ZERO) {
        ctx.request_repaint();
    }
    take_search(app, ctx);
    if app.session.python_busy() || app.python.searching() {
        ctx.request_repaint_after(BUSY_REPAINT);
    }
}

/// Gives the session what the person chose, when that changed since it was last given.
fn sync(app: &mut SheetApp) {
    if app.python.given.as_ref() == Some(&app.ui.python) {
        return;
    }
    let prefs = app.ui.python.clone();
    let capability = PythonCapability {
        enabled: prefs.enabled,
        interpreter: prefs.interpreter.as_ref().map(PathBuf::from),
        launcher: None,
        limits: prefs.limits(),
        transport: app.python.transport.clone(),
        report: app.python.report.clone(),
    };
    app.session.set_python_capability(capability);
    // What is found depends on whether Python may be asked, and which is chosen.
    if app.python.given.as_ref().is_none_or(|g| g.enabled != prefs.enabled) {
        app.python.search = Search::NotStarted;
    }
    app.python.given = Some(prefs);
}

/// Whether synthetic input is about: the switches refuse it, whatever it is aimed at.
pub fn locked(app: &SheetApp, ctx: &egui::Context) -> bool {
    app.synthetic_pending || !app.synthetic.is_empty() || app.python.locked_until.is_some_and(|until| ctx.cumulative_frame_nr() <= until)
}

/// A write to the person's Python settings, by widget code only. Refused while [`locked`]. Returns
/// whether it was written.
pub(crate) fn write_settings(app: &mut SheetApp, ctx: &egui::Context, write: impl FnOnce(&mut PythonPrefs)) -> bool {
    if locked(app, ctx) {
        return false;
    }
    write(&mut app.ui.python);
    sync(app);
    true
}

/// The active workbook's switch, by widget code only. Refused while [`locked`]. Returns whether it
/// was set.
pub(crate) fn set_workbook(app: &mut SheetApp, ctx: &egui::Context, on: bool) -> bool {
    if locked(app, ctx) {
        return false;
    }
    app.session.set_python_workbook_enabled(on);
    true
}

/// The state to show: on a system with no sandbox Python is blocked, whatever the setting says.
pub fn state(app: &SheetApp) -> PythonState {
    if app.python.no_sandbox().is_some() { PythonState::Blocked } else { app.session.python_state() }
}

/// The state's name, as a translation key.
pub fn state_label(state: PythonState) -> &'static str {
    match state {
        PythonState::Off => "Python|Off",
        PythonState::NotSetUp => "Python|Not Set Up",
        PythonState::Ready => "Python|Ready",
        PythonState::Running => "Python|Running",
        PythonState::Blocked => "Python|Blocked",
    }
}

/// The state's icon. Each has its own shape as well as its own colour.
pub fn state_icon(state: PythonState) -> Icon {
    match state {
        PythonState::Off => Icon::PythonOff,
        PythonState::NotSetUp => Icon::PythonNotSetUp,
        PythonState::Ready => Icon::PythonReady,
        PythonState::Running => Icon::PythonRunning,
        PythonState::Blocked => Icon::PythonBlocked,
    }
}

/// Why the state is what it is, or what is worth saying although it is fine.
pub fn reason(app: &SheetApp) -> Option<Diagnostic> {
    if let Some(why) = app.python.no_sandbox() {
        return Some(Diagnostic::new("blocked.platform", why, None));
    }
    let report = app.session.python_report().diagnostic.clone();
    let urgent = report.as_ref().is_some_and(|d| d.kind.id != "packages.noPandas");
    if !urgent && !app.session.python_workbook_enabled() && app.session.python_has_cells() {
        return Some(Diagnostic::new("off.workbook", "", None));
    }
    if !urgent && let Some(failed) = app.session.python_init_failure() {
        return Some(failed.clone());
    }
    report
}

/// Whether Python is being checked rather than running a cell.
pub fn checking(app: &SheetApp) -> bool {
    state(app) == PythonState::Running && app.session.python_running().is_none() && app.session.python_report().interpreter.is_none()
}

/// A diagnostic in the interface's language, with its specifics. The specifics are data from the
/// system or from Python, and are shown as they came, on one line and cut.
pub fn sentence(lang: Language, d: &Diagnostic) -> String {
    let message = lang.tr(d.kind.message);
    let detail: String = d.detail.lines().next().unwrap_or_default().chars().take(200).collect();
    if detail.is_empty() { message.to_string() } else { format!("{message} ({detail})") }
}

/// Why cells can't be asked to run now, in the interface's language. A worker that didn't start can
/// be tried again, so that isn't a reason.
pub fn run_blocked(app: &SheetApp) -> Option<String> {
    let lang = app.ui.language;
    match reason(app) {
        // Running is how a failed initialization is tried again.
        Some(d) if matches!(d.kind.id, "connect.failed" | "packages.noPandas" | "init.failed") => {}
        Some(d) => return Some(sentence(lang, &d)),
        None => {}
    }
    (!app.session.python_workbook_enabled()).then(|| lang.tr(python_off_workbook()).to_string())
}

fn python_off_workbook() -> &'static str {
    python::CATALOG.iter().find(|k| k.id == "off.workbook").map_or("", |k| k.message)
}

/// Why this system can't run Python, in the interface's language.
pub fn system_blocked(app: &SheetApp) -> Option<String> {
    app.python.no_sandbox().map(|why| sentence(app.ui.language, &Diagnostic::new("blocked.platform", why, None)))
}

/// Starts looking for Pythons when nothing current has been found. Asks each one only while Python
/// is on; with Python off, nothing is started.
pub(crate) fn ensure_search(app: &mut SheetApp) {
    let ask = app.ui.python.enabled;
    match &app.python.search {
        Search::Done(_, asked) | Search::Running(_, asked) if *asked == ask => return,
        Search::Running(..) => return,
        _ => {}
    }
    if let Some(answer) = app.python.search_answer.clone() {
        app.python.search = Search::Done(answer, ask);
        return;
    }
    app.python.search = spawn_search(ask);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_search(ask: bool) -> Search {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("python-search".into()).spawn(move || {
        // The interface may have moved on by now.
        let _ = tx.send(gridcraft_engine::search_pythons(ask));
    });
    match spawned {
        Ok(_) => Search::Running(rx, ask),
        Err(e) => {
            log::warn!("Python: the search didn't start: {e}");
            Search::Done(PythonSearch::default(), ask)
        }
    }
}

/// The web build has no processes and no threads to spare: there is nothing to find.
#[cfg(target_arch = "wasm32")]
fn spawn_search(ask: bool) -> Search {
    Search::Done(PythonSearch::default(), ask)
}

/// Looks again at the next chance, after Check Sandbox.
pub(crate) fn search_again(app: &mut SheetApp) {
    app.python.search = Search::NotStarted;
}

fn take_search(app: &mut SheetApp, ctx: &egui::Context) {
    let Search::Running(rx, ask) = &app.python.search else { return };
    let ask = *ask;
    match rx.try_recv() {
        Ok(found) => {
            app.python.search = Search::Done(found, ask);
            ctx.request_repaint();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => app.python.search = Search::Done(PythonSearch::default(), ask),
    }
}

/// An interpreter's kind in the interface's language.
pub fn kind_label(lang: Language, kind: InterpreterKind) -> &'static str {
    lang.tr(kind.label())
}

/// The rest of a found Python's row: its version and kind when it was asked, or why it won't do.
pub fn found_note(lang: Language, f: &FoundPython) -> Option<String> {
    let version = f.version?;
    let kind = f.kind.map_or("", |k| kind_label(lang, k));
    Some(if version < MIN_VERSION {
        lang.tr("{version}, too old: Python cells need 3.11 or newer").replace("{version}", &version.to_string())
    } else {
        format!("{version}, {kind}")
    })
}

/// The list of Pythons to choose from: Automatic, what the search found, the chosen one when it
/// isn't among those, and Browse…. Choosing writes the preference. For a menu.
pub(crate) fn interpreter_items(app: &mut SheetApp, ui: &mut Ui) {
    ensure_search(app);
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    let current = app.ui.python.interpreter.clone();
    let mut choice: Option<Option<String>> = None;
    ui.add_enabled_ui(!locked(app, &ctx), |ui| {
        if ui.selectable_label(current.is_none(), lang.tr("Automatic")).clicked() {
            choice = Some(None);
        }
        let rows: Vec<(String, Option<String>)> =
            app.python.found().map_or_else(Vec::new, |s| s.found.iter().map(|f| (f.path.display().to_string(), found_note(lang, f))).collect());
        if app.python.searching() {
            ui.label(RichText::new(lang.tr("Looking for Python…")).weak());
        }
        for (path, note) in &rows {
            let text = match note {
                Some(note) => format!("{path}  ({note})"),
                None => path.clone(),
            };
            if ui.selectable_label(current.as_deref() == Some(path), text).clicked() {
                choice = Some(Some(path.clone()));
            }
        }
        if let Some(path) = current.as_ref().filter(|p| !rows.iter().any(|(r, _)| r == *p)) {
            let _ = ui.selectable_label(true, path.as_str());
        }
        ui.separator();
        ui.add_enabled_ui(app.services.pick_interpreter.is_some(), |ui| {
            if ui.button(lang.tr("Browse…")).clicked() {
                browse(app, &ctx);
                ui.close();
            }
        });
    });
    if let Some(interpreter) = choice {
        write_settings(app, &ctx, |p| p.interpreter = interpreter);
        ui.close();
    }
}

/// Browse…: chooses an interpreter with the native file picker. Refused while [`locked`], before the
/// picker opens.
pub(crate) fn browse(app: &mut SheetApp, ctx: &egui::Context) {
    if locked(app, ctx) {
        return;
    }
    let Some(path) = app.services.pick_interpreter.as_ref().and_then(|pick| pick()) else { return };
    write_settings(app, ctx, |p| p.interpreter = Some(path));
}

/// Opens File › Options on its Python page.
pub fn open_options(app: &mut SheetApp) {
    app.open_backstage(crate::backstage::Page::Options);
    if let Some(b) = app.backstage.as_mut() {
        b.options_tab = crate::backstage::OptionsTab::Python;
    }
}

/// Opens a pane, in place of any other.
pub fn open_pane(app: &mut SheetApp, pane: &str) {
    app.grid.pane = Some(pane.to_string());
}

/// Opens the pane, or closes it when it is the one open.
pub(crate) fn toggle_pane(app: &mut SheetApp, pane: &str) {
    app.grid.pane = if app.grid.pane.as_deref() == Some(pane) { None } else { Some(pane.to_string()) };
}

/// The pane a UI-only command opens.
pub fn pane_of_command(id: &str) -> Option<&'static str> {
    Some(match id {
        "ui.pythonEnvironment" => ENVIRONMENT,
        "ui.pythonPackages" => PACKAGES,
        "ui.pythonProblems" => PROBLEMS,
        "ui.pythonRunLog" => RUN_LOG,
        "ui.pythonPermissions" => PERMISSIONS,
        _ => return None,
    })
}

/// `ui.pythonEnvironment` and the like, `{on?}`: opens or closes a pane, which is safe from any
/// caller. Without `on` it switches. Replies `{open}`.
pub fn pane_command(app: &mut SheetApp, pane: &str, p: &Json) -> Result<Json, String> {
    let open = app.grid.pane.as_deref() == Some(pane);
    let on = match p.get("on") {
        None | Some(Json::Null) => !open,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err("`on` is true or false".into()),
    };
    if on {
        open_pane(app, pane);
    } else if open {
        app.grid.pane = None;
    }
    Ok(json!({"open": on}))
}

/// Puts a command on the clipboard, and says so. GridCraft copies commands; it never runs them.
pub(crate) fn copy_command(app: &mut SheetApp, ctx: &egui::Context, command: &str) {
    ctx.copy_text(command.to_string());
    app.toast = Some(("Copied to the clipboard.".into(), crate::now_ms()));
}

/// One command to copy: the text, and a button.
pub(crate) fn command_row(app: &mut SheetApp, ui: &mut Ui, command: &str) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    ui.horizontal_top(|ui| {
        if ui.button(lang.tr("Copy")).clicked() {
            copy_command(app, &ctx, command);
        }
        ui.add(Label::new(RichText::new(command).monospace()).wrap());
    });
}

/// The command that adds `packages`, on one line, for the chosen Python or for a new environment.
pub fn install_command(interpreter: Option<&Interpreter>, packages: &[&str]) -> String {
    match interpreter {
        Some(i) => uv_install_commands(i, packages).join(" && "),
        None => uv_setup_commands(packages).join(" && "),
    }
}

/// The same with pip, for the chosen Python, or for whatever `python3` is.
pub fn pip_command(interpreter: Option<&Interpreter>, packages: &[&str]) -> String {
    match interpreter {
        Some(i) => pip_install_command(i, packages),
        None => format!("python3 -m pip install {}", python::shell_words(packages)),
    }
}

/// The sandbox in plain words, on every system: what each line of it means for a cell.
pub(crate) fn sandbox_lines(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let failed = app.session.python_report().diagnostic.as_ref().filter(|d| d.kind.id == "blocked.sandboxCheck").map(|d| sentence(lang, d));
    match system_blocked(app).or(failed) {
        Some(why) => {
            ui.label(why);
        }
        None => {
            for line in
                ["Reading files outside Python: blocked", "Writing files: blocked", "Using the network: blocked", "Starting other programs: blocked"]
            {
                ui.label(lang.tr(line));
            }
            dim(ui, lang.tr("Each time Python starts, GridCraft tries each of these first, and doesn't run Python unless every one is refused."));
        }
    }
}

/// A line of secondary text.
pub(crate) fn dim(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::ui_font(12.0)).color(t.text_dim));
}

/// Whether the Python tab is up: the active cell is a Python cell, or Python code is being edited,
/// in the formula bar or in the editor window. It doesn't depend on whether Python can run here.
pub fn tab_shows(app: &SheetApp) -> bool {
    app.editor.as_ref().is_some_and(|e| e.python) || app.py_window.is_some() || crate::python_card::active_python_cell(app).is_some()
}

/// Whether the Enable bar shows: Python is on in Settings on a system that can run it, and this
/// workbook has Python cells and isn't enabled.
pub fn bar_shows(app: &SheetApp) -> bool {
    app.ui.python.enabled
        && app.python.no_sandbox().is_none()
        && !app.session.python_workbook_enabled()
        && app.session.active().is_some_and(|d| !app.python.bar_dismissed.contains(&d.uid))
        && app.session.python_has_cells()
}

/// The message bar of a workbook with Python cells that isn't enabled.
pub fn enable_bar(app: &mut SheetApp, ui: &mut Ui) {
    if !bar_shows(app) {
        return;
    }
    let ctx = ui.ctx().clone();
    let lang = app.ui.language;
    let text = lang.tr("This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file.");
    let (clicked, dismissed) = crate::widgets::message_bar(ui, "python_enable_bar", crate::NoticeKind::Warning, text, Some(lang.tr("Enable")));
    if clicked {
        set_workbook(app, &ctx, true);
    }
    if dismissed && let Some(uid) = app.session.active().map(|d| d.uid) {
        app.python.bar_dismissed.insert(uid);
    }
}

/// The status bar's item: the state when Python is relevant here, and while a cell runs, the cell
/// and how long it has run. A person with no Python cells and Python off sees nothing.
pub fn status_item(app: &mut SheetApp, ui: &mut Ui) {
    if !(app.ui.python.enabled || app.session.python_has_cells()) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let shown = state(app);
    let text = match app.session.python_running() {
        Some(r) => {
            let cell = if r.initialization {
                lang.tr(crate::python_window::INITIALIZATION).to_string()
            } else {
                format!("{}!{}", gridcraft_engine::formula::quote_sheet(&r.sheet), r.cell.a1())
            };
            lang.tr("Python: running {cell}, {seconds} s").replace("{cell}", &cell).replace("{seconds}", &r.elapsed.as_secs().to_string())
        }
        None => lang.tr("Python: {state}").replace("{state}", lang.tr(state_label(shown))),
    };
    let tip = reason(app).map_or_else(|| lang.tr("Open the Python environment").to_string(), |d| sentence(lang, &d));
    ui.add_space(12.0);
    let color = if matches!(shown, PythonState::Blocked) { t.danger } else { t.text_dim };
    let item = ui.add(Label::new(RichText::new(text).font(theme::ui_font(12.5)).color(color)).sense(Sense::click()).selectable(false));
    if item.on_hover_text(tip).clicked() {
        open_pane(app, ENVIRONMENT);
    }
}

/// What `ui.inspect` says about Python.
pub fn inspect(app: &SheetApp) -> Json {
    let report = app.session.python_report();
    let pane = app.grid.pane.as_deref();
    let d = reason(app);
    let running = app.session.python_running().map(|r| {
        let ms = r.elapsed.as_secs_f64() * 1000.0;
        if r.initialization { json!({"initialization": true, "ms": ms}) } else { json!({"sheet": r.sheet, "cell": r.cell.a1(), "ms": ms}) }
    });
    let initialization = app.session.active().and_then(|d| d.wb.python_env.as_ref()).map(|e| {
        json!({
            "userModified": e.user_modified,
            "fits": e.fits(),
            "chars": e.code.chars().count(),
            "failed": app.session.python_init_failure().map(|f| json!({"detail": f.detail, "line": f.line})),
            "skipped": app.session.python_init_diagnostics().iter().filter(|x| x.diagnostic.kind.id == "init.skipped").map(|x| x.diagnostic.detail.clone()).collect::<Vec<_>>(),
        })
    });
    // What the checks found in code being edited, unless that code is out of view.
    let bar_shown = app.editor.as_ref().is_some_and(|e| e.python && !crate::control::draft_hidden(app, e));
    let window_shown = app.py_window.is_some() && crate::python_window::state(app).get("hidden").and_then(Json::as_bool) != Some(true);
    let findings = |shown: bool, checks: &checks::CodeChecks| if shown { checks.json() } else { json!([]) };
    json!({
        "state": state(app),
        "reason": d.as_ref().map(|d| d.kind.id),
        "enabled": app.ui.python.enabled,
        "workbookEnabled": app.session.python_workbook_enabled(),
        "hasCells": app.session.python_has_cells(),
        "enableBar": bar_shows(app),
        "calculation": app.session.python_calculation(),
        "interpreter": report.interpreter.as_ref().map(|i| i.path.display().to_string()),
        "busy": app.session.python_busy(),
        "unfinished": app.session.active().map_or_else(Vec::new, |d| app.session.python_unfinished(d.uid)),
        "running": running,
        "initialization": initialization,
        "findings": {"formulaBar": findings(bar_shown, &app.python.bar_checks), "editor": findings(window_shown, &app.python.window_checks)},
        "optionsPage": app.backstage.as_ref().is_some_and(|b| b.page == crate::backstage::Page::Options && b.options_tab == crate::backstage::OptionsTab::Python),
        "panes": {
            "environment": pane == Some(ENVIRONMENT),
            "packages": pane == Some(PACKAGES),
            "problems": pane == Some(PROBLEMS),
            "runLog": pane == Some(RUN_LOG),
            "permissions": pane == Some(PERMISSIONS),
        },
    })
}

/// What confines the worker on this system, for one kind of thing it can't do.
pub fn enforcer(what: Guard) -> &'static str {
    let linux = cfg!(target_os = "linux");
    let macos = cfg!(target_os = "macos");
    match what {
        _ if !linux && !macos => "Nothing on this system: Python doesn't run here",
        Guard::Environment => "GridCraft starts Python with an empty environment",
        _ if macos => "macOS sandbox profile",
        Guard::Files => "Linux Landlock",
        Guard::System => "Linux seccomp filter",
    }
}

/// What kind of thing a line of the permissions list is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Files on the computer, and what can't work without them.
    Files,
    /// The network, other programs and windows.
    System,
    /// The app's environment variables.
    Environment,
}

/// The packages a diagnostic is asking for, by the names to install them under.
pub fn packages_for(d: &Diagnostic) -> Vec<String> {
    let names: Vec<String> = match d.kind.id {
        "packages.noPandas" => vec!["pandas".to_string()],
        "packages.missing" | "init.skipped" => python::install_name_of(&d.detail).into_iter().collect(),
        // A finding's detail is the name to install already.
        "lint.importMissing" => vec![d.detail.clone()],
        "module.missing" => module_named(&d.detail).map(|m| install_name(&m)).into_iter().collect(),
        _ => Vec::new(),
    };
    // A workbook's code chooses these names, and a person runs the command: only what can be a package
    // is offered at all.
    names.into_iter().filter(|name| panes::package_spec(name)).collect()
}

/// The module Python's message names: `No module named 'x.y'`, or the detail when it is one word.
fn module_named(detail: &str) -> Option<String> {
    let quoted = detail.split('\'').nth(1).filter(|m| !m.is_empty());
    let word = (!detail.is_empty() && !detail.contains(char::is_whitespace)).then_some(detail);
    quoted.or(word).map(str::to_string)
}

/// The cell a diagnostic points at: `(sheet name, cell)`, the sheet when it names one.
pub fn cell_it_points_at(d: &Diagnostic) -> Option<(Option<String>, String)> {
    let place = match d.kind.id {
        "python.nameOrder" => d.detail.rsplit_once(" is assigned in ")?.1,
        "result.spill" => d.detail.split_whitespace().next()?,
        _ => return None,
    };
    match place.rsplit_once('!') {
        Some((sheet, cell)) => CellRef::parse(cell).map(|_| (Some(gridcraft_engine::core::unquote_sheet(sheet)), cell.to_string())),
        None => CellRef::parse(place).map(|_| (None, place.to_string())),
    }
}

/// The button for what a diagnostic's kind offers, when there is something to do: Open Settings,
/// Choose Interpreter, Copy Install Command with pip behind a collapsed section, Enable, Run Again,
/// Open Initialization and Reset, Go To Cell. `cell` is the cell it is about, for what runs. Returns whether it was pressed.
pub(crate) fn action_button(app: &mut SheetApp, ui: &mut Ui, d: &Diagnostic, cell: Option<(usize, CellRef)>) -> bool {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    match d.kind.action {
        Action::None => false,
        Action::OpenSettings => {
            let pressed = ui.button(lang.tr("Open Settings")).clicked();
            if pressed {
                open_options(app);
            }
            pressed
        }
        Action::ChooseInterpreter => {
            let pressed = ui.button(lang.tr("Choose Interpreter")).clicked();
            if pressed {
                open_options(app);
            }
            pressed
        }
        Action::CopyInstallCommand => {
            let packages = packages_for(d);
            if packages.is_empty() {
                return false;
            }
            let names: Vec<&str> = packages.iter().map(String::as_str).collect();
            let interpreter = app.session.python_report().interpreter.clone();
            let pressed = ui.button(lang.tr("Copy Install Command")).clicked();
            if pressed {
                copy_command(app, &ctx, &install_command(interpreter.as_ref(), &names));
            }
            egui::CollapsingHeader::new(lang.tr("Other package managers")).id_salt(("python_action_other", d.kind.id)).default_open(false).show(
                ui,
                |ui| {
                    command_row(app, ui, &pip_command(interpreter.as_ref(), &names));
                },
            );
            false
        }
        Action::EnableWorkbook => {
            let pressed = ui.button(lang.tr("Enable")).clicked();
            if pressed {
                set_workbook(app, &ctx, true);
            }
            pressed
        }
        Action::RunAgain => {
            let pressed = ui.button(lang.tr("Run Again")).clicked();
            if pressed {
                match cell {
                    Some((sheet, at)) => app.run_or_alert("python.run", json!({"cell": at.a1(), "sheet": sheet})),
                    None => app.run_or_alert("python.runAll", json!({})),
                }
            }
            pressed
        }
        Action::OpenInitialization => {
            // The code can be fixed there, or was fine and lacked something that has come since.
            let (open, reset) = ui
                .horizontal_wrapped(|ui| {
                    let open = ui.button(lang.tr("Open Initialization")).clicked();
                    (open, ui.button(lang.tr("Reset")).clicked())
                })
                .inner;
            if open {
                crate::python_window::open_initialization(app);
            }
            if reset {
                app.run_or_alert("python.reset", json!({}));
            }
            open || reset
        }
        Action::GoToCell => {
            let Some((sheet, target)) = cell_it_points_at(d) else { return false };
            let pressed = ui.button(lang.tr("Go To Cell")).clicked();
            if pressed {
                let on = sheet.map(Json::from).or(cell.map(|(s, _)| Json::from(s))).unwrap_or(Json::Null);
                app.run_or_alert("selection.set", json!({"range": target, "sheet": on}));
                app.grid.ensure_visible = true;
            }
            pressed
        }
    }
}

/// What the state chip says: the Python version with its state when one is known, or the state.
pub fn chip_text(app: &SheetApp) -> String {
    let lang = app.ui.language;
    let shown = state(app);
    let version = app.session.python_report().interpreter.as_ref().map(|i| format!("{}.{}", i.version.major, i.version.minor));
    match (shown, version) {
        (PythonState::Ready, Some(v)) => lang.tr("Python {version}, ready").replace("{version}", &v),
        (PythonState::Running, Some(v)) => lang.tr("Python {version}, running").replace("{version}", &v),
        _ => lang.tr(state_label(shown)).to_string(),
    }
}

const CHIP_FONT: f32 = 11.5;
/// The chip's icon, and its margins.
const CHIP_CHROME: f32 = 26.0;

/// The state chip's width, for the row it goes in.
pub fn chip_width(app: &SheetApp, ui: &Ui) -> f32 {
    let t = Tokens::get(ui.ctx());
    ui.painter().layout_no_wrap(chip_text(app), theme::ui_font(CHIP_FONT), t.text).size().x + CHIP_CHROME
}

/// A small chip with Python's state, in the editors: a click opens the Environment pane.
pub fn state_chip(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let shown = state(app);
    let text = chip_text(app);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(chip_width(app, ui), 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &text));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 4.0, t.hover);
    }
    p.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 12.0, rect.center().y), egui::vec2(14.0, 14.0));
    crate::icons::paint(p, icon, state_icon(shown), t.text);
    p.text(egui::pos2(rect.left() + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, &text, theme::ui_font(CHIP_FONT), t.text);
    let tip = reason(app).map_or_else(|| lang.tr("Open the Python environment").to_string(), |d| sentence(lang, &d));
    if resp.on_hover_text(tip).clicked() {
        open_pane(app, ENVIRONMENT);
    }
}

/// `python.refreshEnvironment` from the interface: Check Sandbox and Check Again.
pub(crate) fn check_again(app: &mut SheetApp) {
    app.run_or_alert("python.refreshEnvironment", json!({}));
    search_again(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python_card;

    /// What the Python interface says, besides the catalog's messages.
    const STRINGS: &[&str] = &[
        "Python|Off",
        "Python|Not Set Up",
        "Python|Ready",
        "Python|Running",
        "Python|Blocked",
        "Enable Python in cells",
        "{version}, too old: Python cells need 3.11 or newer",
        "Automatic",
        "Looking for Python…",
        "Browse…",
        "Copied to the clipboard.",
        "Copy",
        "This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file.",
        "Enable",
        "Python: running {cell}, {seconds} s",
        "Python: {state}",
        "Open the Python environment",
        "Nothing on this system: Python doesn't run here",
        "GridCraft starts Python with an empty environment",
        "macOS sandbox profile",
        "Linux Landlock",
        "Linux seccomp filter",
        "Open Python Settings",
        "Choose Interpreter",
        "Copy Install Command",
        python_card::COPY_CSV,
        python_card::COPY_MARKDOWN,
        python_card::SAVE_CSV,
        python_card::SPILL_HERE,
        python_card::SPILL_LINES,
        python_card::RUN_TO_SEE,
        python_card::NO_COLUMNS,
        python_card::NO_ROWS,
        python_card::COPIED_CUT,
        python_card::SAVED_CUT,
        python_card::SAVED,
        "Only the first {rows} of {total} rows are shown.",
        "Only the first {cols} of {total} columns are shown.",
        "Some long text is cut.",
        "Copying and saving take only what is shown. Spill Here gives the whole table.",
        "Save Python Table as CSV",
        "Python Table",
        crate::python_plots::OUTPUT_MENU,
        crate::python_plots::DISPLAY_OVER_CELLS,
        crate::python_plots::CREATE_REFERENCE,
        crate::python_plots::COPY_IMAGE,
        crate::python_plots::SAVE_IMAGE,
        crate::python_plots::IMAGE_SAVED,
        crate::python_plots::IMAGE_COPIED,
        "Image",
        "size",
        "Resize",
        "This picture shows the last result that cell gave.",
        "Select a Python cell that gave a picture first.",
        "Run Again",
        "Reset",
        "Python Environment",
        "Python Packages",
        "Python Problems",
        "Python Run Log",
        "What Python Can Do",
        "GridCraft is checking Python.",
        "Check Again",
        "Interpreter",
        "Chosen by you:",
        "Automatic found:",
        "What runs:",
        "None found yet.",
        "Sandbox",
        "Package folders",
        "What Python can read",
        "uv manages Python versions and packages. To get it, run:",
        "Reading files outside Python: blocked",
        "Writing files: blocked",
        "Using the network: blocked",
        "Starting other programs: blocked",
        "Each time Python starts, GridCraft tries each of these first, and doesn't run Python unless every one is refused.",
        "Install a package with uv. GridCraft only copies the command; you run it in a terminal, then choose Check Again.",
        "Package name",
        "Other package managers",
        "For people who manage Python another way: choose any interpreter by path in File › Options › Python. With pip:",
        "Filter",
        "Packages: {count}",
        "No problems.",
        "This workbook",
        "Clear",
        "No runs yet.",
        "Go To Cell",
        "Open Settings",
        "Show Printed Output",
        "Hide Printed Output",
        "Line {line}",
        "Line {line}: {message}",
        "Python {version}, ready",
        "Python {version}, running",
        "This run printed nothing.",
        "Python cells can",
        "Read cells through xl()",
        "GridCraft sends only the cells that xl() names",
        "Import installed packages",
        "The sandbox lets Python read its own installation",
        "Compute",
        "Within the time and memory limits in File › Options › Python",
        "Print",
        "Printed output is kept in the Run Log",
        "Use threads",
        "Threads stay inside the one Python process",
        "Python cells cannot",
        "Read your files",
        "Write files",
        "Use the network",
        "Start programs",
        "See environment variables",
        "Open windows",
        "Draw plots. The data goes to a sheet and a chart is built from it.",
        "Run this workbook's Python cells",
        "Off by default. Only you can turn this on, here: no file, macro or agent can. A workbook opened from a file also needs its own Enable.",
        "Limits",
        "While Python is off, GridCraft starts nothing, so it can't yet say which version it would use.",
        "Automatic would try first:",
        "Set up Python with uv",
        "Run these in a terminal, in order. GridCraft looks in this environment first, so there is nothing more to choose. It only copies the commands; it never runs them.",
        "Any Python 3.11 or newer can be chosen by path with Browse…. To add packages to it with pip:",
        "Time limit for one cell",
        " seconds",
        "Memory limit",
        " MB",
        "At a limit Python is restarted and its variables are cleared; the next run starts fresh.",
        "On macOS the memory limit is a check twice a second, not a hard cap.",
        "Editor",
        "Initialization",
        "The code that runs before this workbook's Python cells",
        "Open Initialization",
        "The initialization code has unsaved changes.",
        "The initialization code of {workbook} has unsaved changes.",
        "The code can't be longer: initialization code holds {max} characters.",
        "The initialization code changed while it was open in the editor. Copy your code, or close the editor and open it again.",
        "The initialization code changed while it was open in the editor, so your code was not applied. Copy it, or close the editor and open it again.",
        "Select a Python cell first.",
        "Output As",
        "Run\nCell",
        "Run the selected Python cell",
        "Run\nAll",
        "Run every Python cell in the workbook, in order",
        "Nothing is running.",
        "Stop",
        "Stop the running cell",
        "Clear Python's variables and run every cell again",
        "Python Calculation",
        "When this workbook's Python cells run",
        "Manual",
        "A Python cell is running.",
        "Python is ready.",
        "Which Python runs cells",
        "Packages",
        "What this Python has installed",
        "Check Sandbox",
        "Check Python and the sandbox again, and show the report",
        "What Python\nCan Do",
        "What Python cells can and can't do here",
        "This Workbook: Enabled",
        "This Workbook: Disabled",
        "Only a person can change this.",
        "Run Log",
        "Each run's cell, time and printed output",
        "Clear the Run Log",
        "Problems",
        "Every Python cell with something to say",
        "Diagnostics",
        "uv environment",
        "uv Python",
        "virtual environment",
        "conda environment",
        "system",
    ];

    /// A string a catalog lacks shows in English, which the Python interface doesn't allow: every
    /// catalog holds every one of them, and the catalog of diagnostics.
    #[test]
    fn every_catalog_translates_the_python_interface() {
        let catalogs = [
            ("cs", include_str!("i18n/cs.rs")),
            ("de", include_str!("i18n/de.rs")),
            ("el", include_str!("i18n/el.rs")),
            ("es", include_str!("i18n/es.rs")),
            ("fr", include_str!("i18n/fr.rs")),
            ("it", include_str!("i18n/it.rs")),
            ("ja", include_str!("i18n/ja.rs")),
            ("ko", include_str!("i18n/ko.rs")),
            ("nl", include_str!("i18n/nl.rs")),
            ("pl", include_str!("i18n/pl.rs")),
            ("pt", include_str!("i18n/pt.rs")),
            ("ro", include_str!("i18n/ro.rs")),
            ("ru", include_str!("i18n/ru.rs")),
            ("sr", include_str!("i18n/sr.rs")),
            ("tr", include_str!("i18n/tr.rs")),
            ("uk", include_str!("i18n/uk.rs")),
            ("vi", include_str!("i18n/vi.rs")),
            ("zh", include_str!("i18n/zh.rs")),
        ];
        assert_eq!(catalogs.len() + 1, Language::ALL.len());
        let messages = python::CATALOG.iter().map(|k| k.message);
        let keys: Vec<&str> = STRINGS.iter().copied().chain(messages).collect();
        for (code, source) in catalogs {
            let language = Language::parse(code).unwrap();
            for key in &keys {
                assert!(source.contains(&format!("{key:?}")), "{code} doesn't translate {key:?}");
                assert!(!language.tr(key).contains('|'), "{code} shows a context prefix for {key:?}");
                let label = key.rsplit_once('|').map_or(*key, |(_, label)| label);
                assert_eq!(label.contains('\n'), language.tr(key).contains('\n'), "{code}: the lines of {key:?}");
                for placeholder in [
                    "{cell}",
                    "{seconds}",
                    "{state}",
                    "{count}",
                    "{version}",
                    "{line}",
                    "{message}",
                    "{workbook}",
                    "{max}",
                    "{rows}",
                    "{cols}",
                    "{total}",
                ] {
                    assert_eq!(key.contains(placeholder), language.tr(key).contains(placeholder), "{code}: {placeholder} in {key:?}");
                }
            }
        }
    }

    #[test]
    fn saved_settings_are_read_leniently_and_python_stays_off() {
        // Nothing in a file that isn't what this writes turns Python on, or loses the rest.
        for bad in [r#"{"dark": true, "python": 7}"#, r#"{"dark": true, "python": {"enabled": "yes"}}"#, r#"{"dark": true, "python": null}"#] {
            let ui: crate::UiState = serde_json::from_str(bad).unwrap();
            assert!(ui.dark, "{bad}");
            assert_eq!(ui.python, PythonPrefs::default(), "{bad}");
        }
        let ui: crate::UiState = serde_json::from_str("{}").unwrap();
        assert!(!ui.python.enabled, "off by default");
        let ui: crate::UiState = serde_json::from_str(r#"{"python": {"enabled": true, "timeLimitSecs": 5}}"#).unwrap();
        assert_eq!((ui.python.enabled, ui.python.time_limit_secs, ui.python.memory_limit_mb), (true, 5, 1024));
        let again: crate::UiState = serde_json::from_str(&serde_json::to_string(&ui).unwrap()).unwrap();
        assert_eq!(again.python, ui.python);
    }

    #[test]
    fn limits_stay_within_what_the_fields_take() {
        let limits = |time_limit_secs, memory_limit_mb| PythonPrefs { time_limit_secs, memory_limit_mb, ..PythonPrefs::default() }.limits();
        assert_eq!(limits(0, 0).timeout, Duration::from_secs(1));
        assert_eq!(limits(0, 0).memory_bytes, 128 * MIB);
        assert_eq!(limits(u64::MAX, u64::MAX).timeout, MAX_TIMEOUT);
        assert_eq!(limits(u64::MAX, u64::MAX).memory_bytes, 262_144 * MIB);
        assert_eq!(PythonPrefs::default().limits(), Limits::default());
    }

    #[test]
    fn a_too_old_python_says_so_and_a_python_not_asked_says_nothing() {
        let v = |minor| Some(python::Version { major: 3, minor, patch: 0 });
        let row = |version, kind| FoundPython { path: "/p".into(), version, kind };
        assert_eq!(found_note(Language::En, &row(None, None)), None);
        assert_eq!(found_note(Language::En, &row(v(12), Some(InterpreterKind::UvEnvironment))).unwrap(), "3.12.0, uv environment");
        assert!(found_note(Language::En, &row(v(9), Some(InterpreterKind::System))).unwrap().contains("too old"));
    }

    #[test]
    fn install_commands_speak_uv_first() {
        assert_eq!(
            install_command(None, &["pandas"]),
            "uv venv ~/.gridcraft/python --python 3.12 && uv pip install --compile-bytecode --python ~/.gridcraft/python pandas"
        );
        assert_eq!(pip_command(None, &["pandas"]), "python3 -m pip install pandas");
    }

    #[test]
    fn a_copied_command_reads_each_package_as_one_word() {
        assert_eq!(pip_command(None, &["a>=1,<2", "x;id"]), "python3 -m pip install 'a>=1,<2' 'x;id'");
        assert!(install_command(None, &["x;id $(y)"]).ends_with(" 'x;id $(y)'"), "{}", install_command(None, &["x;id $(y)"]));
        assert_eq!(python::shell_words(&["it's"]), r"'it'\''s'");
    }
}
