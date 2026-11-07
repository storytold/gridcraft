//! Running Python cells.
//!
//! The session holds what the app allows ([`PythonCapability`], set by the app and by nothing a
//! command, a file or the control channel can reach) and, per open workbook, one worker and the state
//! of its cells. Nothing here blocks: a command only marks what is due; [`Session::python_poll`],
//! which the interface calls every frame, takes one finished result, writes it, and sends the next
//! cell. Results make no undo step, and stay out of the journal. The one exception is `file.save` with
//! `wait`, for a caller with no frames to keep going: the interface takes `wait` off.
//!
//! - `inputs` resolves a cell's `xl("…")` references and fingerprints what they read.
//! - `dates` turns serial numbers into dates by their cells' formats, and dates in results back into
//!   serial numbers, in the workbook's date system.
//! - `plan` keeps the list of Python cells up to date and decides what is due.
//! - `runner` submits cells and writes results.
//! - `initialization` runs the workbook's initialization code before the cells.
//! - `environment` works out on a thread of its own whether Python can run here.
//! - `wait` waits for a workbook's cells, so that a save gets their results.
//!
//! A due cell, with its kept result, shows `#BUSY!` until the new one arrives; what the result
//! spilled stays in place until then ([`gridcraft_model::PyCode::busy`]).

mod dates;
mod environment;
mod initialization;
mod inputs;
pub mod links;
mod packages;
mod plan;
mod runner;
mod wait;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gridcraft_core::CellRef;
use gridcraft_model::{CalcMode, Sheet, Workbook};
pub use gridcraft_python::Limits;
pub use gridcraft_python::Transport;
pub use gridcraft_python::describe::{
    Interpreter, Kind as InterpreterKind, STARTER_PACKAGES, UV_INSTALL, pip_install_command, uv_install_commands, uv_setup_commands,
};
pub use gridcraft_python::diagnostics::{Action, CATALOG, Diagnostic, Kind, Scope, Severity};
pub use gridcraft_python::interp::{MIN_VERSION, Version};
pub use gridcraft_python::lint::{Finding, install_name, lint, missing_modules};
pub use gridcraft_python::run::MAX_TIMEOUT;
pub use gridcraft_python::sandbox_support;
use serde_json::{Value as Json, json};

use environment::Environment;
pub use environment::{PythonEnvironment, PythonState};
use plan::{CellState, Cx, DocRun, Gate, Source};
use runner::{Finish, Starter};
pub use wait::{MAX_WAIT, PythonWait, WaitBound, WaitState, note_unfinished, reply_may_be_held, wait_asked};

use crate::{DocState, Result, Session};

/// Makes a worker's transport, in place of the real one: a seam for tests.
#[doc(hidden)]
pub type TransportFactory = Arc<dyn Fn() -> Box<dyn Transport> + Send + Sync>;

/// What the app allows Python to do in this session. Only the app sets it
/// ([`Session::set_python_capability`]); there is no command, control method or file content that
/// can turn Python on or choose the interpreter. A session without one (the CLI, MCP) never starts
/// Python: its cells keep their saved values or show `#BLOCKED!`.
#[derive(Clone, Default)]
pub struct PythonCapability {
    /// The person turned Python on.
    pub enabled: bool,
    /// The interpreter the person chose; without one, GridCraft looks for one (uv's first).
    pub interpreter: Option<PathBuf>,
    /// The executable that starts the confined worker, by default this one. It must hand its
    /// arguments to `gridcraft_sandbox::handle_args` first.
    pub launcher: Option<PathBuf>,
    /// What one worker and its requests may use.
    pub limits: Limits,
    /// Workers from this factory instead of a confined interpreter (tests).
    #[doc(hidden)]
    pub transport: Option<TransportFactory>,
    /// This answer instead of a check of Python, for an enabled capability (tests).
    #[doc(hidden)]
    pub report: Option<PythonEnvironment>,
}

/// When a workbook's Python cells run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PythonCalculation {
    /// As soon as they are due.
    #[default]
    Automatic,
    /// Due cells are marked stale and keep their value until `python.run` or `python.runAll`.
    Manual,
}

/// One run of a cell, in a workbook's run log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonRun {
    pub sheet: String,
    pub cell: CellRef,
    pub duration: Duration,
    /// What the code printed, at most 64 KiB, kept also when it failed.
    pub stdout: String,
    /// The printed output was longer than that, or than the worker keeps.
    pub stdout_cut: bool,
    /// The id of what is worth saying about the run ([`Kind::id`]).
    pub diagnostic: Option<&'static str>,
    /// This was the initialization code rather than a cell: `sheet` is empty, and `cell` means nothing.
    pub initialization: bool,
}

impl PythonRun {
    fn new(
        d: &DocState,
        sheet: usize,
        cell: CellRef,
        duration: Duration,
        stdout: String,
        stdout_cut: bool,
        diagnostic: Option<&'static str>,
    ) -> PythonRun {
        PythonRun {
            sheet: d.wb.sheet(sheet).map(|s| s.name.clone()).unwrap_or_default(),
            cell,
            duration,
            stdout,
            stdout_cut,
            diagnostic,
            initialization: false,
        }
    }
}

/// What a workbook's worker is running now: a cell, or the initialization code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonRunning {
    pub sheet: String,
    pub cell: CellRef,
    /// How long it has been running.
    pub elapsed: Duration,
    /// This is the initialization code rather than a cell: `sheet` is empty, and `cell` means nothing.
    pub initialization: bool,
}

/// One Python the search found, for a person to choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundPython {
    pub path: PathBuf,
    /// What it answered, when it was asked. `None` when it wasn't asked or didn't answer.
    pub version: Option<Version>,
    pub kind: Option<InterpreterKind>,
}

/// What [`search_pythons`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PythonSearch {
    /// In the order the search tries them, so the first usable one is what Automatic takes.
    pub found: Vec<FoundPython>,
    /// Where uv is.
    pub uv: Option<PathBuf>,
}

/// The most Pythons a search reports.
const MAX_FOUND: usize = 16;

/// Looks for the Pythons on this machine, which blocks, so the interface calls it on a thread of its
/// own. With `ask` each one is started, with a deadline, to tell its version and where it came from:
/// a fixed script runs, never workbook code. Without it nothing is started, and only the paths are
/// known: the interface uses that while Python is off. The web build finds none.
pub fn search_pythons(ask: bool) -> PythonSearch {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use gridcraft_python::interp::{self, SearchEnv};
        let env = SearchEnv::from_process();
        let uv = interp::find_uv(&env);
        let found = interp::candidates(&env)
            .into_iter()
            .take(MAX_FOUND)
            .map(|path| {
                let told = if ask { gridcraft_python::describe::describe(&path, interp::PROBE_TIMEOUT).ok() } else { None };
                FoundPython { version: told.as_ref().map(|i| i.version), kind: told.as_ref().map(|i| i.kind), path }
            })
            .collect();
        PythonSearch { found, uv }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ask, MAX_FOUND);
        PythonSearch::default()
    }
}

/// A diagnostic with the place it is about, for `python.diagnostics`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonDiagnosticEntry {
    pub diagnostic: Diagnostic,
    /// The sheet's index and the cell; `None` for what is about the whole workbook or the app.
    pub cell: Option<(usize, CellRef)>,
    /// It is about the initialization code.
    pub initialization: bool,
}

/// How much work the runtime has done, for tests of what it must not do.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PythonStats {
    /// Whole workbooks looked through for Python cells.
    pub full_scans: u64,
    /// Areas read to fingerprint them.
    pub areas_hashed: u64,
    /// Clients created.
    pub clients: u64,
    /// Pieces of code read for the imports they need.
    pub code_reads: u64,
}

static STATE_BUSY: LazyLock<Diagnostic> = LazyLock::new(|| Diagnostic::new("state.busy", "", None));
static STATE_STALE: LazyLock<Diagnostic> = LazyLock::new(|| Diagnostic::new("state.stale", "", None));
static OFF_APP: LazyLock<Diagnostic> = LazyLock::new(|| Diagnostic::new("off.app", "", None));
static OFF_WORKBOOK: LazyLock<Diagnostic> = LazyLock::new(|| Diagnostic::new("off.workbook", "", None));
static OFF_HEADLESS: LazyLock<Diagnostic> = LazyLock::new(|| Diagnostic::new("off.headless", "", None));

/// The Python state of a session.
#[derive(Default)]
pub(crate) struct Runtime {
    capability: Option<PythonCapability>,
    /// A capability was set in this session, so workbooks may hold Python state. Until then Python
    /// costs a command one check of this.
    ever: bool,
    env: Environment,
    docs: HashMap<u64, DocRun>,
    /// Which workbook goes first at the next poll, so that one doesn't starve the others.
    rotor: usize,
    stats: PythonStats,
}

impl Runtime {
    /// The workbook closed: its worker stops with its state.
    pub(crate) fn forget(&mut self, uid: u64) {
        self.docs.remove(&uid);
    }

    fn enabled(&self) -> bool {
        self.capability.as_ref().is_some_and(|c| c.enabled)
    }

    /// The reason Python doesn't run in a workbook, when it is about the app or the workbook.
    fn off_reason(&self, d: &DocState) -> Option<&'static Diagnostic> {
        match &self.capability {
            None => Some(&OFF_HEADLESS),
            Some(c) if !c.enabled => Some(&OFF_APP),
            Some(_) if !self.docs.get(&d.uid).map_or_else(|| d.trust.allows_code(), |r| r.on(d.trust)) => Some(&OFF_WORKBOOK),
            Some(_) => None,
        }
    }
}

fn manual(r: &DocRun, d: &DocState) -> bool {
    r.mode == PythonCalculation::Manual || d.wb.calc.mode == CalcMode::Manual || r.init.stopped
}

/// The Python cells of a sheet, row by row.
fn python_order_of_sheet(sh: &Sheet) -> Vec<CellRef> {
    sh.cells.iter().filter(|(_, c)| c.py.is_some()).map(|(c, _)| c).collect()
}

/// The workbook's Python cells in the order they run, as in Excel: sheets in tab order, each row by
/// row, left to right. Cells share their variables, so the order decides what each one sees.
pub fn python_order(wb: &Workbook) -> Vec<(usize, CellRef)> {
    wb.sheets.iter().enumerate().flat_map(|(i, sh)| python_order_of_sheet(sh).into_iter().map(move |c| (i, c))).collect()
}

impl Session {
    /// Gives this session what the app allows: `None` for a session that never runs Python. Stops
    /// every worker; cells run again in the new setup. An enabled capability starts the check of
    /// Python in the background ([`Session::python_environment`]). For the app only: no command
    /// reaches this.
    pub fn set_python_capability(&mut self, capability: impl Into<Option<PythonCapability>>) {
        self.python.ever = true;
        for d in &mut self.docs {
            if let Some(r) = self.python.docs.get_mut(&d.uid) {
                r.turn_off(d);
            }
        }
        self.python.capability = capability.into();
        let capability = self.python.capability.clone();
        self.python.env.restart(capability.as_ref());
        self.python_guarded(Session::python_refresh);
    }

    /// What the app allows now.
    pub fn python_capability(&self) -> Option<&PythonCapability> {
        self.python.capability.as_ref()
    }

    /// Turns Python on or off for the active workbook. A workbook made in the app is on when the
    /// capability is; one opened from a file, or holding Python code pasted from one, is off until
    /// the person turns it on here. For the app only: no command reaches this. Turning it on makes
    /// every cell of the workbook due; turning it off stops its worker, and its cells keep what they
    /// show.
    pub fn set_python_workbook_enabled(&mut self, on: bool) {
        let Some(uid) = self.active().map(|d| d.uid) else { return };
        self.python.ever = true;
        self.python.docs.entry(uid).or_default().switch = Some(on);
        self.python_guarded(Session::python_refresh);
    }

    /// Whether Python is on for the active workbook, whatever the capability says.
    pub fn python_workbook_enabled(&self) -> bool {
        self.active().is_some_and(|d| self.python.docs.get(&d.uid).map_or_else(|| d.trust.allows_code(), |r| r.on(d.trust)))
    }

    /// What the interface calls every frame, with no wait: picks up the check of Python, takes at
    /// most one finished result, writes it, recalculates, and sends the next due cell. With a wait
    /// it blocks up to that long for something to happen (for headless callers and tests). Returns
    /// whether anything changed. A session that was never given a capability does nothing.
    pub fn python_poll(&mut self, wait: Duration) -> bool {
        if !self.python.ever {
            return false;
        }
        self.python_guarded(|s| s.python_poll_inner(wait))
    }

    fn python_poll_inner(&mut self, wait: Duration) -> bool {
        let mut changed = self.python_take_environment(Duration::ZERO);
        changed |= self.python_refresh();
        if self.python_take_one_result(Duration::ZERO) {
            return true;
        }
        changed |= self.python_pump();
        if changed || wait.is_zero() {
            return changed;
        }
        self.python_wait(wait)
    }

    /// Whether Python is working or about to: a request runs, cells wait for one, or the check of
    /// Python hasn't answered. The interface keeps polling while it is true.
    pub fn python_busy(&self) -> bool {
        self.python.env.checking() || self.python.docs.values().any(|r| r.requesting() || r.due > 0 || r.render_waiting)
    }

    /// Whether a client holds a worker. One that stopped counts until its client has seen that.
    #[cfg(test)]
    pub(crate) fn python_worker_alive(&self) -> bool {
        self.python.docs.values().any(|r| r.client.as_ref().is_some_and(gridcraft_python::Client::alive))
    }

    /// What is wrong, or worth saying, about a Python cell of the active workbook. `None` for a
    /// cell with nothing to say, and for a cell that isn't a Python cell.
    pub fn python_diagnostic(&self, sheet: usize, cell: CellRef) -> Option<&Diagnostic> {
        let d = self.active()?;
        d.wb.sheet(sheet)?.cell(cell)?.py.as_ref()?;
        if let Some(off) = self.python.off_reason(d) {
            return Some(off);
        }
        let r = self.python.docs.get(&d.uid)?;
        let e = r.entries.get(r.index_of(sheet, cell)?)?;
        if let Some(failed) = &r.init.failed {
            return Some(failed);
        }
        match e.state {
            CellState::Queued => Some(&STATE_BUSY),
            CellState::Stale if self.python.env.gate() == Gate::No => self.python.env.report.diagnostic.as_ref().or(Some(&STATE_STALE)),
            CellState::Stale => Some(&STATE_STALE),
            CellState::Clean if e.unsettled => e.diag.as_ref().filter(|x| x.kind.severity == Severity::Error).or(Some(&STATE_STALE)),
            CellState::Clean => e.diag.as_ref(),
        }
    }

    /// Everything [`Session::python_diagnostic`] says about the active workbook, in run order, after
    /// what is about the app or the workbook.
    pub fn python_diagnostics(&self) -> Vec<PythonDiagnosticEntry> {
        let Some(d) = self.active() else { return Vec::new() };
        let entry = |diagnostic: &Diagnostic, cell| PythonDiagnosticEntry { diagnostic: diagnostic.clone(), cell, initialization: false };
        let mut out = Vec::new();
        if let Some(off) = self.python.off_reason(d) {
            if !python_order(&d.wb).is_empty() {
                out.push(entry(off, None));
            }
            return out;
        }
        // That pandas is missing is said once, about the workbook, when it has something to say it about.
        out.extend(self.python.env.report.diagnostic.iter().filter(|x| x.kind.id != "packages.noPandas").map(|x| entry(x, None)));
        out.extend(self.python_init_diagnostics());
        out.extend(self.python_workbook_diagnostics());
        let Some(r) = self.python.docs.get(&d.uid) else { return out };
        for e in &r.entries {
            // That the initialization code failed is said once, about the workbook.
            if let Some(x) = self.python_diagnostic(e.sheet, e.at).filter(|x| x.kind.scope != Scope::Environment && x.kind.id != "init.failed") {
                out.push(entry(x, Some((e.sheet, e.at))));
            }
        }
        out
    }

    /// What the active workbook's initialization code did: `init.failed`, with Python's message and
    /// line, or an `init.skipped` for each package it went on without, with the package as the detail
    /// and the line that wanted it.
    pub fn python_init_diagnostics(&self) -> Vec<PythonDiagnosticEntry> {
        let Some(r) = self.active().and_then(|d| self.python.docs.get(&d.uid)) else { return Vec::new() };
        r.init
            .failed
            .iter()
            .chain(&r.init.skipped)
            .map(|x| PythonDiagnosticEntry { diagnostic: x.clone(), cell: None, initialization: true })
            .collect()
    }

    /// What the active workbook's initialization code last said when it failed: what every Python
    /// cell shows until it is fixed.
    pub fn python_init_failure(&self) -> Option<&Diagnostic> {
        self.active().and_then(|d| self.python.docs.get(&d.uid)).and_then(|r| r.init.failed.as_ref())
    }

    /// What is worth saying about the active workbook's code as a whole, once the package list is
    /// known: `packages.missing` for each module it imports that this Python doesn't have, with the
    /// module as the detail and the first cell that imports it (none for the initialization code),
    /// and `packages.noPandas`. An empty list isn't known, so it reports nothing.
    pub fn python_workbook_diagnostics(&self) -> Vec<PythonDiagnosticEntry> {
        let Some(r) = self.active().and_then(|d| self.python.docs.get(&d.uid)).filter(|r| r.tracked && !r.entries.is_empty()) else {
            return Vec::new();
        };
        let packages = &self.python.env.report.packages;
        if packages.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<PythonDiagnosticEntry> = r
            .packages
            .missing
            .iter()
            .map(|m| PythonDiagnosticEntry {
                diagnostic: Diagnostic::new("packages.missing", m.module.clone(), None),
                cell: m.place,
                initialization: m.place.is_none(),
            })
            .collect();
        if !packages.iter().any(|(name, _)| name.eq_ignore_ascii_case("pandas")) {
            out.push(PythonDiagnosticEntry { diagnostic: Diagnostic::new("packages.noPandas", "", None), cell: None, initialization: false });
        }
        out
    }

    /// Moves on when the package list may have changed, for what is worked out from it.
    pub fn python_packages_generation(&self) -> u64 {
        self.python.env.generation
    }

    /// Can Python cells run here, and in what. The check takes seconds and runs in the background:
    /// until it answers the state is [`PythonState::Running`] with no interpreter. It runs again
    /// when the capability changes and on `python.refreshEnvironment`. The state is also `Running`
    /// while a cell runs. On the web and on systems without a sandbox the answer, `blocked.platform`,
    /// is there at once.
    pub fn python_environment(&self) -> PythonEnvironment {
        let mut report = self.python.env.report.clone();
        if self.python.docs.values().any(DocRun::requesting) {
            report.state = PythonState::Running;
        }
        report
    }

    /// [`Session::python_environment`]'s state without its copy of the report, for what asks every
    /// frame.
    pub fn python_state(&self) -> PythonState {
        if self.python.docs.values().any(DocRun::requesting) { PythonState::Running } else { self.python.env.report.state }
    }

    /// The report as the last check left it, without a copy. Its `state` doesn't say whether a cell
    /// is running: [`Session::python_state`] does.
    pub fn python_report(&self) -> &PythonEnvironment {
        &self.python.env.report
    }

    /// What the active workbook's worker is running, with how long it has run: a cell, or the
    /// initialization code. Other workbooks' cells aren't this workbook's to report.
    pub fn python_running(&self) -> Option<PythonRunning> {
        let d = self.active()?;
        let r = self.python.docs.get(&d.uid)?;
        if let Some((_, started)) = &r.init.flight {
            return Some(PythonRunning { sheet: String::new(), cell: CellRef::new(0, 0), elapsed: started.elapsed(), initialization: true });
        }
        let flight = r.inflight.as_ref()?;
        let e = r.entries.get(flight.index)?;
        Some(PythonRunning { sheet: d.wb.sheet(e.sheet)?.name.clone(), cell: e.at, elapsed: flight.started.elapsed(), initialization: false })
    }

    /// Whether the active workbook holds a Python cell. This is cheap once the session has seen the
    /// workbook: what each change touched keeps the answer, so it can be asked every frame. Before
    /// that it looks through the workbook, up to the first Python cell.
    pub fn python_has_cells(&self) -> bool {
        let Some(d) = self.active() else { return false };
        match self.python.docs.get(&d.uid).filter(|r| r.seen.is_some()) {
            Some(r) if r.tracked => !r.entries.is_empty(),
            Some(r) => r.holds_code,
            None => plan::holds_code(&d.wb),
        }
    }

    /// The active workbook's last 200 runs, oldest first, the initialization code's among them.
    pub fn python_run_log(&self) -> impl Iterator<Item = &PythonRun> {
        self.active().and_then(|d| self.python.docs.get(&d.uid)).into_iter().flat_map(|r| r.log.iter())
    }

    /// When the active workbook's Python cells run.
    pub fn python_calculation(&self) -> PythonCalculation {
        self.active().and_then(|d| self.python.docs.get(&d.uid)).map_or_else(PythonCalculation::default, |r| r.mode)
    }

    #[doc(hidden)]
    pub fn python_stats(&self) -> PythonStats {
        self.python.stats
    }

    /// The Python cells the runtime tracks in the active workbook, in the order it runs them.
    #[doc(hidden)]
    pub fn python_tracked_cells(&self) -> Vec<(usize, CellRef)> {
        self.active().and_then(|d| self.python.docs.get(&d.uid)).map_or_else(Vec::new, |r| r.entries.iter().map(|e| (e.sheet, e.at)).collect())
    }

    /// After a top-level command: works out what the command made due. A session that was never
    /// given a capability stops at its first check.
    pub(crate) fn python_after_command(&mut self) {
        if self.python.ever {
            self.python_guarded(Session::python_refresh);
        }
    }

    /// Runs Python's work outside any command, so outside the guard a command runs in. A panic there
    /// leaves the workbooks as they are, since a write only takes effect when it is done, and stops
    /// Python for the session, with the reason in the report: losing Python never loses the work.
    fn python_guarded(&mut self, work: impl FnOnce(&mut Session) -> bool) -> bool {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(self))) {
            Ok(changed) => changed,
            Err(panic) => {
                let why =
                    panic.downcast_ref::<&str>().map(ToString::to_string).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                log::error!("Python cells stopped after an internal error: {why}");
                for r in self.python.docs.values_mut() {
                    r.forget_cells();
                }
                self.python.env.note_failure(Diagnostic::new("connect.failed", format!("an internal error stopped Python cells: {why}"), None));
                true
            }
        }
    }

    /// Inside a command that reports the cell's value: the same, so the report says `#BUSY!`.
    pub(crate) fn python_after_edit(&mut self) {
        if self.python.ever && self.depth() <= 1 {
            self.python_refresh();
        }
    }

    /// Before content from another workbook arrives in the active one and it takes on that
    /// workbook's trust: Python code from a file turns Python off here until the person turns it on
    /// again, even if they had; content without code leaves Python as it was.
    pub(crate) fn python_note_pasted(&mut self, clip: &crate::Clipboard) {
        if !self.python.ever || clip.trust.allows_code() {
            return;
        }
        let Some(d) = self.docs.get(self.active_index()) else { return };
        let code = clip.wb.sheet(clip.sheet).is_some_and(|sh| sh.cells.iter_range(clip.range).any(|(_, c)| c.py.is_some()));
        let r = self.python.docs.entry(d.uid).or_default();
        let was_on = r.on(d.trust);
        if code {
            r.switch = None;
        } else if was_on {
            r.switch = Some(true);
        }
    }

    /// Plans every workbook that changed. Returns whether a workbook was written to.
    fn python_refresh(&mut self) -> bool {
        let enabled = self.python.enabled();
        let gate = self.python.env.gate();
        let Runtime { docs: runs, stats, env, .. } = &mut self.python;
        let docs = &mut self.docs;
        runs.retain(|uid, _| docs.iter().any(|d| d.uid == *uid));
        let mut changed = false;
        for d in docs.iter_mut() {
            let r = runs.entry(d.uid).or_default();
            changed |= if enabled && r.on(d.trust) {
                let cx = Cx { gate, manual: manual(r, d) };
                r.plan(d, cx, Source::Edit, stats)
            } else {
                r.stay_off(d)
            };
            r.check_packages(&d.wb, env.generation, &env.report.packages, stats);
            r.note_render_due(d, gate == Gate::Go);
        }
        changed
    }

    /// Picks up the check of Python, and has every workbook act on the answer.
    fn python_take_environment(&mut self, wait: Duration) -> bool {
        if !self.python.env.take(wait) {
            return false;
        }
        // The package the initialization code lacked may be there now.
        for r in self.python.docs.values_mut() {
            r.retry_failed_initialization();
        }
        self.python_rederive();
        true
    }

    /// The gate or a mode changed: every workbook marks again what is due.
    fn python_rederive(&mut self) -> bool {
        let gate = self.python.env.gate();
        let enabled = self.python.enabled();
        let Runtime { docs: runs, .. } = &mut self.python;
        let mut changed = false;
        for d in &mut self.docs {
            if let Some(r) = runs.get_mut(&d.uid).filter(|r| enabled && r.tracked) {
                let cx = Cx { gate, manual: manual(r, d) };
                changed |= r.derive(d, cx, Source::Edit, None);
            }
        }
        changed
    }

    /// Takes at most one finished result, from any workbook, starting with the next in turn.
    fn python_take_one_result(&mut self, wait: Duration) -> bool {
        let count = self.docs.len();
        for offset in 0..count {
            let i = self.python.rotor.saturating_add(offset) % count;
            let Some(uid) = self.docs.get(i).map(|d| d.uid) else { continue };
            let Some(result) = self.python.docs.get_mut(&uid).and_then(|r| r.poll_worker(wait)) else { continue };
            self.python.rotor = i.saturating_add(1);
            self.python_finish(i, uid, result);
            self.python_pump();
            return true;
        }
        false
    }

    fn python_finish(&mut self, doc: usize, uid: u64, result: gridcraft_python::RunResult) {
        let gate = self.python.env.gate();
        let Some(d) = self.docs.get_mut(doc) else { return };
        let Runtime { docs: runs, stats, .. } = &mut self.python;
        let Some(r) = runs.get_mut(&uid) else { return };
        let cx = Cx { gate, manual: manual(r, d) };
        let Finish { env_problem, .. } = r.finish(d, result, cx, stats);
        if let Some(problem) = env_problem {
            self.python.env.note_failure(problem);
            self.python_rederive();
        }
    }

    /// Sends the next due cell of every workbook that has none running.
    fn python_pump(&mut self) -> bool {
        let gate = self.python.env.gate();
        if gate != Gate::Go {
            return false;
        }
        let Runtime { docs: runs, stats, capability, env, .. } = &mut self.python;
        let Some(capability) = capability.as_ref() else { return false };
        let starter = Starter { capability, interpreter: env.interpreter() };
        let mut changed = false;
        for d in &mut self.docs {
            if let Some(r) = runs.get_mut(&d.uid).filter(|r| r.tracked) {
                let cx = Cx { gate, manual: manual(r, d) };
                changed |= r.pump(d, &starter, cx, stats);
            }
        }
        changed
    }

    /// Nothing is ready: waits up to `wait` on the first thing that will be.
    fn python_wait(&mut self, wait: Duration) -> bool {
        if self.python.docs.values().any(DocRun::requesting) {
            return self.python_take_one_result_waiting(wait);
        }
        if self.python.env.checking() {
            return self.python_take_environment(wait);
        }
        false
    }

    fn python_take_one_result_waiting(&mut self, wait: Duration) -> bool {
        let Some((i, uid)) =
            self.docs.iter().enumerate().find(|(_, d)| self.python.docs.get(&d.uid).is_some_and(DocRun::requesting)).map(|(i, d)| (i, d.uid))
        else {
            return false;
        };
        let Some(result) = self.python.docs.get_mut(&uid).and_then(|r| r.poll_worker(wait)) else { return false };
        self.python_finish(i, uid, result);
        self.python_pump();
        true
    }

    /// The error a command gives when the active workbook's Python cells can't run now.
    fn python_refuse(&self) -> Result<()> {
        let d = self.doc()?;
        let cannot = |diag: &Diagnostic| {
            let detail = if diag.detail.is_empty() { String::new() } else { format!(" ({})", diag.detail) };
            crate::EngineError::Other(format!("{}{detail}", diag.kind.message))
        };
        if let Some(off) = self.python.off_reason(d) {
            return Err(cannot(off));
        }
        if self.python.env.gate() == Gate::No {
            return Err(self
                .python
                .env
                .report
                .diagnostic
                .as_ref()
                .map_or_else(|| crate::EngineError::Other("Python can't run here.".into()), cannot));
        }
        Ok(())
    }

    /// `python.run`: the cell runs now, whatever the calculation mode.
    pub(crate) fn python_run_cell(&mut self, sheet: usize, at: CellRef) -> Result<usize> {
        self.python_ready_to_run()?;
        let gate = self.python.env.gate();
        let d = self.docs.get_mut(self.active).ok_or(crate::EngineError::NoDocument)?;
        let Some(r) = self.python.docs.get_mut(&d.uid) else { return Ok(0) };
        let Some(index) = r.index_of(sheet, at) else {
            return Err(crate::EngineError::Other(format!("{} isn't a Python cell, so there is nothing to run", at.a1())));
        };
        let cx = Cx { gate, manual: manual(r, d) };
        Ok(r.run_one(d, index, cx))
    }

    /// `python.runAll` and `python.reset`: every cell runs, in a fresh worker when `fresh`.
    pub(crate) fn python_run_all(&mut self, fresh: bool) -> Result<usize> {
        self.python_ready_to_run()?;
        let d = self.docs.get_mut(self.active).ok_or(crate::EngineError::NoDocument)?;
        let Some(r) = self.python.docs.get_mut(&d.uid) else { return Ok(0) };
        Ok(r.run_everything(d, fresh))
    }

    /// Calculate Now (F9): Python cells marked stale run. Nothing happens where Python isn't running
    /// or none is stale; F9 isn't a Python command, so it reports no error about Python.
    pub(crate) fn python_calculate_now(&mut self) {
        let Some(d) = self.docs.get(self.active) else { return };
        let stale = d.wb.sheets.iter().any(|sh| sh.cells.iter().any(|(_, c)| c.py.as_deref().is_some_and(|py| py.stale)));
        if stale {
            let _ = self.python_run_all(false);
        }
    }

    /// Brings the list up to date for a command that asks cells to run. A worker that failed to
    /// start gets another try.
    fn python_ready_to_run(&mut self) -> Result<()> {
        if self.python.ever {
            self.python.env.retry();
            self.python_refresh();
        }
        self.python_refuse()
    }

    /// `python.stop`: stops the running cell and leaves the waiting ones stale.
    pub(crate) fn python_stop(&mut self) -> bool {
        let Some(d) = self.docs.get_mut(self.active) else { return false };
        self.python.docs.get_mut(&d.uid).is_some_and(|r| r.stop(d))
    }

    /// `python.calculation`: sets when the active workbook's cells run. Stays with the open workbook.
    pub(crate) fn python_set_calculation(&mut self, mode: PythonCalculation) {
        let gate = self.python.env.gate();
        let enabled = self.python.enabled();
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let r = self.python.docs.entry(d.uid).or_default();
        if r.mode == mode {
            return;
        }
        r.mode = mode;
        if !enabled || !r.tracked {
            return;
        }
        let cx = Cx { gate, manual: manual(r, d) };
        if cx.manual {
            r.demote_queued();
            r.sync_flags(d);
        } else {
            r.derive(d, cx, Source::Edit, None);
        }
    }

    /// `python.clearRunLog`: forgets the active workbook's runs. Returns how many there were.
    pub(crate) fn python_clear_run_log(&mut self) -> usize {
        let Some(uid) = self.active().map(|d| d.uid) else { return 0 };
        self.python.docs.get_mut(&uid).map_or(0, |r| {
            let n = r.log.len();
            r.log.clear();
            n
        })
    }

    /// `python.refreshEnvironment`: checks Python again.
    pub(crate) fn python_refresh_environment(&mut self) {
        let capability = self.python.capability.clone();
        self.python.env.restart(capability.as_ref());
        // The package the initialization code lacked may be there now, where the answer is there at
        // once; otherwise this happens when it comes.
        if !self.python.env.checking() {
            for r in self.python.docs.values_mut() {
                r.retry_failed_initialization();
            }
        }
        self.python_rederive();
    }

    /// `python.initialization` with code: sets the active workbook's initialization code, which keeps
    /// the rest of what its file said about the environment. Code that is what it was changes nothing,
    /// so it costs no undo step and runs nothing.
    pub(crate) fn python_set_initialization(&mut self, code: &str) -> Result<()> {
        let old = self.doc()?.wb.python_env.clone();
        if old.as_ref().map_or("", |e| e.code.as_str()) == code {
            return Ok(());
        }
        let env = gridcraft_model::PyEnvironment {
            id: old.as_ref().and_then(|e| e.id.clone()).or_else(|| Some(gridcraft_model::python::environment_id(code))),
            kept: old.as_ref().and_then(|e| e.kept.clone()),
            ..gridcraft_model::PyEnvironment::from_person(code)
        };
        crate::cmd::edit(self, |cx| {
            cx.wb.python_env = Some(Arc::new(env));
            Ok(())
        })
    }
}

/// A diagnostic as JSON, with the catalog's words.
pub(crate) fn diagnostic_json(d: &Diagnostic) -> Json {
    json!({
        "kind": d.kind.id,
        "severity": d.kind.severity.id(),
        "scope": d.kind.scope.id(),
        "message": d.kind.message,
        "action": d.kind.action.id(),
        "detail": d.detail,
        "line": d.line,
    })
}

/// The environment report as JSON (`python.environment`).
pub(crate) fn environment_json(e: &PythonEnvironment) -> Json {
    let path = |p: &std::path::Path| p.display().to_string();
    json!({
        "state": e.state,
        "diagnostic": e.diagnostic.as_ref().map(diagnostic_json),
        "sandbox": e.sandbox,
        "uv": e.uv.as_deref().map(path),
        "interpreter": e.interpreter.as_ref().map(|i| json!({
            "path": path(&i.path),
            "runAs": path(&i.run_as),
            "version": i.version.to_string(),
            "kind": i.kind.label(),
            "siteDirs": i.site_dirs.iter().map(|p| path(p)).collect::<Vec<_>>(),
            "readPaths": i.read_paths.iter().map(|p| path(p)).collect::<Vec<_>>(),
        })),
        "packages": e.packages.iter().map(|(name, version)| json!({"name": name, "version": version})).collect::<Vec<_>>(),
    })
}

/// A diagnostic row as JSON (`python.diagnostics`).
pub(crate) fn diagnostic_entry_json(wb: &Workbook, e: &PythonDiagnosticEntry) -> Json {
    let mut row = diagnostic_json(&e.diagnostic);
    let cell = e.cell.and_then(|(sheet, at)| wb.sheet(sheet).map(|sh| format!("{}!{}", gridcraft_core::quote_sheet(&sh.name), at.a1())));
    if let Some(o) = row.as_object_mut() {
        o.insert("cell".into(), json!(cell));
        o.insert("initialization".into(), json!(e.initialization));
        if matches!(e.diagnostic.kind.id, "packages.missing" | "init.skipped") {
            o.insert("install".into(), json!(install_name_of(&e.diagnostic.detail)));
        }
    }
    row
}

/// The name to install a package under, for what a diagnostic says is missing. Excel's own module has
/// none: no package gives it.
pub fn install_name_of(module: &str) -> Option<String> {
    (module.split('.').next() != Some("excel")).then(|| install_name(module))
}

/// A run as JSON (`python.runLog`).
pub(crate) fn run_json(r: &PythonRun) -> Json {
    let place = |text: String| if r.initialization { Json::Null } else { json!(text) };
    json!({
        "initialization": r.initialization,
        "sheet": place(r.sheet.clone()),
        "cell": place(r.cell.a1()),
        "ms": r.duration.as_secs_f64() * 1000.0,
        "stdout": r.stdout,
        "stdoutCut": r.stdout_cut,
        "diagnostic": r.diagnostic,
    })
}

#[cfg(test)]
pub(crate) fn assigned_names_for_test(code: &str) -> Vec<&str> {
    runner::assigned_names(code)
}

#[cfg(test)]
pub(crate) const START_RETRIES_FOR_TEST: u32 = runner::START_RETRIES;
