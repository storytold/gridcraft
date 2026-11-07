//! The environment report: can Python cells run here, and in what. Working it out starts Python
//! (asked its version, its folders and, inside the sandbox, its packages), which can take seconds,
//! so it happens on a thread of its own; `Session::python_poll` picks up the answer.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use gridcraft_python::describe::Interpreter;
use gridcraft_python::diagnostics::Diagnostic;

use super::PythonCapability;
use super::plan::Gate;

/// How long listing the packages may take once Python has started.
#[cfg(not(target_arch = "wasm32"))]
const PACKAGES_TIMEOUT: Duration = Duration::from_secs(30);

/// Where Python stands on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PythonState {
    /// This session doesn't run Python, or Python is turned off.
    Off,
    /// There is no Python to run, or it doesn't start.
    NotSetUp,
    /// Python can run.
    Ready,
    /// Python is being checked (`interpreter` is empty until the check ends), or cells are running.
    Running,
    /// The sandbox isn't there or didn't hold, so Python won't run.
    Blocked,
}

/// The environment report: see [`crate::Session::python_environment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonEnvironment {
    pub state: PythonState,
    /// Why it isn't ready, or what is worth saying although it is.
    pub diagnostic: Option<Diagnostic>,
    /// What confines the worker on this system, or why nothing does.
    pub sandbox: String,
    /// Where uv is.
    pub uv: Option<PathBuf>,
    /// The Python that runs cells.
    pub interpreter: Option<Interpreter>,
    /// What is installed in it, as cells see it: (name, version).
    pub packages: Vec<(String, String)>,
}

impl PythonEnvironment {
    fn state(state: PythonState, id: &str, detail: impl Into<String>) -> PythonEnvironment {
        PythonEnvironment {
            state,
            diagnostic: Some(Diagnostic::new(id, detail, None)),
            sandbox: String::new(),
            uv: None,
            interpreter: None,
            packages: Vec::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn failed(mut self, state: PythonState, id: &str, detail: impl Into<String>) -> PythonEnvironment {
        self.state = state;
        self.diagnostic = Some(Diagnostic::new(id, detail, None));
        self
    }
}

impl Default for PythonEnvironment {
    /// A session that was given no capability.
    fn default() -> PythonEnvironment {
        PythonEnvironment::state(PythonState::Off, "off.headless", "")
    }
}

/// The report, and the check under way.
#[derive(Default)]
pub(super) struct Environment {
    pub report: PythonEnvironment,
    pending: Option<Receiver<PythonEnvironment>>,
    /// Workers come from the test seam: no interpreter is looked for.
    seam: bool,
    /// Moves on each time the report is replaced, so what is worked out from its package list
    /// knows when to work it out again.
    pub generation: u64,
}

impl Environment {
    /// Whether a check is under way.
    pub fn checking(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether cells can start.
    pub fn gate(&self) -> Gate {
        match (self.pending.is_some(), self.report.state) {
            (true, _) => Gate::Wait,
            (false, PythonState::Ready) => Gate::Go,
            _ => Gate::No,
        }
    }

    /// Starts over for this capability. Without a sandbox the answer is there at once; otherwise
    /// the check starts on its own thread. No Python starts without an enabled capability.
    pub fn restart(&mut self, capability: Option<&PythonCapability>) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.seam = capability.is_some_and(|c| c.transport.is_some() || c.report.is_some());
        self.report = match capability {
            None => PythonEnvironment::default(),
            Some(c) if !c.enabled => PythonEnvironment::state(PythonState::Off, "off.app", ""),
            Some(PythonCapability { report: Some(report), .. }) => report.clone(),
            Some(_) if self.seam => PythonEnvironment {
                state: PythonState::Ready,
                diagnostic: None,
                sandbox: "not used by this test".into(),
                ..PythonEnvironment::default()
            },
            Some(c) => match gridcraft_python::sandbox_support() {
                Ok(how) => self.start_check(c, how),
                Err(why) => PythonEnvironment { sandbox: why.clone(), ..PythonEnvironment::state(PythonState::Blocked, "blocked.platform", why) },
            },
        };
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_check(&mut self, capability: &PythonCapability, how: String) -> PythonEnvironment {
        let (tx, rx) = std::sync::mpsc::channel();
        let capability = capability.clone();
        let sandbox = how.clone();
        let spawned = std::thread::Builder::new().name("python-environment".into()).spawn(move || {
            // The session may be gone by now, or have started another check.
            let _ = tx.send(check(&capability, sandbox));
        });
        match spawned {
            Ok(_) => {
                self.pending = Some(rx);
                PythonEnvironment { state: PythonState::Running, diagnostic: None, sandbox: how, ..PythonEnvironment::default() }
            }
            Err(e) => PythonEnvironment { sandbox: how, ..PythonEnvironment::state(PythonState::NotSetUp, "connect.failed", e.to_string()) },
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_check(&mut self, _capability: &PythonCapability, how: String) -> PythonEnvironment {
        PythonEnvironment { sandbox: how.clone(), ..PythonEnvironment::state(PythonState::Blocked, "blocked.platform", how) }
    }

    /// Takes the check's answer when it has come, waiting up to `wait`. Returns whether it came.
    pub fn take(&mut self, wait: Duration) -> bool {
        let Some(rx) = self.pending.as_ref() else { return false };
        let got = if wait.is_zero() {
            match rx.try_recv() {
                Ok(report) => Some(report),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => None,
            }
        } else {
            match rx.recv_timeout(wait) {
                Ok(report) => Some(report),
                Err(RecvTimeoutError::Timeout) => return false,
                Err(RecvTimeoutError::Disconnected) => None,
            }
        };
        self.pending = None;
        self.generation = self.generation.wrapping_add(1);
        self.report = got.unwrap_or_else(|| {
            let sandbox = std::mem::take(&mut self.report.sandbox);
            PythonEnvironment { sandbox, ..PythonEnvironment::state(PythonState::NotSetUp, "connect.failed", "the check of Python stopped") }
        });
        true
    }

    /// A cell's worker couldn't start: Python isn't ready after all, and no more cells try.
    pub fn note_failure(&mut self, diagnostic: Diagnostic) {
        self.report.state = if diagnostic.kind.id == "blocked.sandboxCheck" { PythonState::Blocked } else { PythonState::NotSetUp };
        self.report.diagnostic = Some(diagnostic);
    }

    /// Someone asked to run cells again: a worker that didn't start may start now. A sandbox that
    /// didn't hold needs a new check first.
    pub fn retry(&mut self) {
        let can_start = self.seam || self.report.interpreter.is_some();
        if can_start && !self.checking() && self.report.diagnostic.as_ref().is_some_and(|d| d.kind.id == "connect.failed") {
            self.report.state = PythonState::Ready;
            self.report.diagnostic = None;
        }
    }

    /// The Python to start, once the check has found it.
    pub fn interpreter(&self) -> Option<&Interpreter> {
        self.report.interpreter.as_ref()
    }
}

/// Works out the report, on the check's own thread.
#[cfg(not(target_arch = "wasm32"))]
fn check(capability: &PythonCapability, sandbox: String) -> PythonEnvironment {
    use PythonState::NotSetUp;
    use gridcraft_python::describe::describe;
    use gridcraft_python::interp::{self, Detection, SearchEnv};

    let env = SearchEnv::from_process();
    let report = PythonEnvironment { state: NotSetUp, diagnostic: None, sandbox, uv: interp::find_uv(&env), interpreter: None, packages: Vec::new() };
    let path = match interp::detect(&env, capability.interpreter.as_deref(), interp::PROBE_TIMEOUT) {
        Detection::Found { path, .. } => path,
        Detection::NoneFound => {
            let detail = capability.interpreter.as_ref().map(|p| format!("{} didn't answer", p.display())).unwrap_or_default();
            return report.failed(NotSetUp, "notSetUp.noInterpreter", detail);
        }
        Detection::TooOld { path, version } => return report.failed(NotSetUp, "notSetUp.tooOld", format!("{} is {version}", path.display())),
        Detection::Stub { path } => return report.failed(NotSetUp, "notSetUp.stub", path.display().to_string()),
    };
    match describe(&path, interp::PROBE_TIMEOUT) {
        Ok(interpreter) => list_packages(PythonEnvironment { interpreter: Some(interpreter), ..report }, capability),
        Err(_) => report.failed(NotSetUp, "notSetUp.unreadable", path.display().to_string()),
    }
}

/// Lists the packages through a confined worker, which also proves that the sandbox and the worker
/// start. A refusal is the report's answer; code that fails after it started isn't.
#[cfg(not(target_arch = "wasm32"))]
fn list_packages(mut report: PythonEnvironment, capability: &PythonCapability) -> PythonEnvironment {
    use std::collections::BTreeMap;

    use gridcraft_python::diagnostics::{Scope, for_outcome};
    use gridcraft_python::interp::LIST_PACKAGES;
    use gridcraft_python::process::ProcessTransport;
    use gridcraft_python::{Client, Limits, Outcome};

    let Some(interpreter) = report.interpreter.as_ref() else { return report };
    let limits = Limits { timeout: PACKAGES_TIMEOUT, ..capability.limits.clone() };
    let mut client = Client::new(Box::new(ProcessTransport::confined(interpreter, capability.launcher.clone())), limits);
    let mut outcome = client.run(LIST_PACKAGES, BTreeMap::new());
    for _ in 0..super::runner::START_RETRIES {
        if !matches!(outcome, Ok(Outcome::FailedToStart(_))) {
            break;
        }
        outcome = client.run(LIST_PACKAGES, BTreeMap::new());
    }
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(busy) => return report.failed(PythonState::NotSetUp, "connect.failed", busy.to_string()),
    };
    if let Some(d) = for_outcome(&outcome).filter(|d| d.kind.scope == Scope::Environment) {
        report.state = if d.kind.id == "blocked.sandboxCheck" { PythonState::Blocked } else { PythonState::NotSetUp };
        report.diagnostic = Some(d);
        return report;
    }
    if let Outcome::Ok { output, .. } = &outcome {
        report.packages = package_rows(output);
    }
    report.state = PythonState::Ready;
    if !report.packages.iter().any(|(name, _)| name.eq_ignore_ascii_case("pandas")) {
        report.diagnostic = Some(Diagnostic::new("packages.noPandas", "", None));
    }
    report
}

/// The rows the listing gave: (name, version). What a worker sends is untrusted, and the protocol
/// has capped it.
#[cfg(not(target_arch = "wasm32"))]
fn package_rows(output: &gridcraft_python::Output) -> Vec<(String, String)> {
    use gridcraft_core::Value;
    let gridcraft_python::Output::Value(Value::Array(rows)) = output else { return Vec::new() };
    let text = |r: usize, c: usize| match rows.get(r, c) {
        Some(Value::Text(t)) => t.to_string(),
        _ => String::new(),
    };
    (0..rows.rows).map(|r| (text(r, 0), text(r, 1))).filter(|(name, _)| !name.is_empty()).collect()
}
