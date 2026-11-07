//! What can be wrong with Python here, in one vocabulary: why Python can't run on this system, in
//! this workbook or in this cell, and what a line of code will run into. The cell's card, the
//! editors, the Python tab, the control channel and the docs all read this catalog, so a person and
//! an agent are told the same thing.
//!
//! A [`Kind`]'s `id` is stable: agents and other tools can match on it. Its `message` is the English
//! source text, which the interface translates.

use crate::protocol::PyError;
use crate::run::Outcome;

/// How serious it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    /// Python can't run at all until something about the system or setup changes.
    Blocked,
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn id(self) -> &'static str {
        match self {
            Severity::Blocked => "blocked",
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// How far it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The app, on this machine.
    Environment,
    /// This workbook.
    Workbook,
    /// One cell, when it runs.
    Cell,
    /// One place in a cell's code, before it runs.
    Line,
}

impl Scope {
    pub fn id(self) -> &'static str {
        match self {
            Scope::Environment => "environment",
            Scope::Workbook => "workbook",
            Scope::Cell => "cell",
            Scope::Line => "line",
        }
    }
}

/// What the person can do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    None,
    /// Open the Python settings.
    OpenSettings,
    /// Choose another interpreter.
    ChooseInterpreter,
    /// Copy the command that installs what is missing.
    CopyInstallCommand,
    /// Turn Python on for this workbook.
    EnableWorkbook,
    /// Run the cells again.
    RunAgain,
    /// Open the initialization code, or clear the variables and run the cells again.
    OpenInitialization,
    /// Go to the cell that is in the way.
    GoToCell,
}

impl Action {
    pub fn id(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::OpenSettings => "openSettings",
            Action::ChooseInterpreter => "chooseInterpreter",
            Action::CopyInstallCommand => "copyInstallCommand",
            Action::EnableWorkbook => "enableWorkbook",
            Action::RunAgain => "runAgain",
            Action::OpenInitialization => "openInitialization",
            Action::GoToCell => "goToCell",
        }
    }
}

/// One entry of the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kind {
    pub id: &'static str,
    pub severity: Severity,
    pub scope: Scope,
    /// The sentence the person reads (English source text).
    pub message: &'static str,
    pub action: Action,
}

const fn k(id: &'static str, severity: Severity, scope: Scope, message: &'static str, action: Action) -> Kind {
    Kind { id, severity, scope, message, action }
}

use Action as A;
use Scope::{Cell, Environment, Line, Workbook};
use Severity::{Blocked, Error, Info, Warning};

/// Every diagnostic, by level.
pub const CATALOG: &[Kind] = &[
    // The app, on this machine.
    k("blocked.platform", Blocked, Environment, "Python cells need a sandbox, and GridCraft has none for this system.", A::None),
    k("blocked.sandboxCheck", Blocked, Environment, "The sandbox didn't hold when GridCraft checked it, so Python won't run.", A::OpenSettings),
    k("off.app", Blocked, Environment, "Python is off. Turn it on in Settings to run Python cells.", A::OpenSettings),
    k(
        "notSetUp.noInterpreter",
        Blocked,
        Environment,
        "GridCraft can't find Python 3.11 or newer. Set one up with uv, or choose one.",
        A::ChooseInterpreter,
    ),
    k("notSetUp.tooOld", Blocked, Environment, "This Python is too old: Python cells need 3.11 or newer.", A::ChooseInterpreter),
    k(
        "notSetUp.stub",
        Blocked,
        Environment,
        "This is Apple's placeholder for Python, which GridCraft doesn't use. Set up Python with uv, or choose another.",
        A::ChooseInterpreter,
    ),
    k("notSetUp.unreadable", Blocked, Environment, "GridCraft couldn't work out what this Python needs to run.", A::ChooseInterpreter),
    k("connect.failed", Error, Environment, "Python didn't start.", A::RunAgain),
    // This workbook.
    k("off.workbook", Blocked, Workbook, "Python isn't on for this workbook, so its cells keep the values saved in the file.", A::EnableWorkbook),
    k("off.headless", Info, Workbook, "This session doesn't run Python: cells keep the values saved in the file.", A::None),
    k("packages.missing", Warning, Workbook, "This workbook imports packages this Python doesn't have.", A::CopyInstallCommand),
    k("packages.noPandas", Info, Workbook, "pandas isn't installed in this Python, so ranges come back as lists of rows.", A::CopyInstallCommand),
    k("init.failed", Error, Workbook, "The initialization code failed, so Python cells can't run.", A::OpenInitialization),
    k(
        "init.skipped",
        Warning,
        Workbook,
        "The initialization code uses a package this Python doesn't have. GridCraft skipped the lines that need it and ran the rest.",
        A::CopyInstallCommand,
    ),
    // One cell, when it runs.
    k("python.syntax", Error, Cell, "This code isn't valid Python.", A::None),
    k("python.exception", Error, Cell, "The code raised an error.", A::None),
    k("python.name", Error, Cell, "The code uses a name that isn't defined.", A::None),
    k("python.nameOrder", Error, Cell, "This name is defined in a later cell. Python cells run in row order, sheet by sheet.", A::GoToCell),
    k("module.missing", Error, Cell, "This Python doesn't have that package.", A::CopyInstallCommand),
    k("module.blockedLibrary", Error, Cell, "This package needs a library outside what the sandbox lets Python read.", A::ChooseInterpreter),
    k("denied.file", Error, Cell, "Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl().", A::None),
    k("denied.network", Error, Cell, "Python cells can't use the network. Put the data on a sheet and read it with xl().", A::None),
    k("denied.process", Error, Cell, "Python cells can't start other programs or processes.", A::None),
    k("plot.tooLarge", Error, Cell, "This picture is too large to draw. Make figsize or dpi smaller.", A::None),
    k("xl.notLiteral", Error, Cell, "xl() needs its reference written as text in the code, such as xl(\"A1:B5\").", A::None),
    k("xl.tooLarge", Error, Cell, "This range is too large to send to Python.", A::None),
    k("xl.badReference", Error, Cell, "xl() names something that isn't a cell, range, table or defined name here.", A::None),
    k("limit.time", Error, Cell, "The code ran past the time limit. Python was restarted, so its variables were cleared.", A::RunAgain),
    k("limit.memory", Error, Cell, "The code went over the memory limit. Python was restarted, so its variables were cleared.", A::RunAgain),
    k("limit.result", Error, Cell, "The result is too large to bring back to the sheet.", A::None),
    k("limit.output", Warning, Cell, "The printed output was cut off at its limit.", A::None),
    k("worker.lost", Error, Cell, "Python stopped unexpectedly and was restarted, so its variables were cleared.", A::RunAgain),
    k("result.empty", Error, Cell, "The result is empty.", A::None),
    k("result.spill", Error, Cell, "The result needs cells that aren't empty.", A::GoToCell),
    k("result.number", Error, Cell, "The result isn't a number a cell can hold.", A::None),
    k("result.date", Error, Cell, "This date or length of time is outside what a cell can hold.", A::None),
    k("state.busy", Info, Cell, "This cell is running.", A::None),
    k("state.stale", Info, Cell, "What this cell reads has changed since it last ran.", A::RunAgain),
    k("state.stopped", Info, Cell, "This cell was stopped before it finished.", A::RunAgain),
    // One place in the code, before it runs.
    k(
        "lint.xlNotLiteral",
        Warning,
        Line,
        "xl() needs its reference written as text, such as xl(\"A1:B5\"). A computed reference can't be read.",
        A::None,
    ),
    k("lint.importBlocked", Warning, Line, "This module needs the network or other programs, which Python cells can't use.", A::None),
    k("lint.importMissing", Warning, Line, "This Python doesn't have this package.", A::CopyInstallCommand),
    k(
        "lint.fileAccess",
        Warning,
        Line,
        "Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl().",
        A::None,
    ),
    k("lint.savefig", Info, Line, "The picture appears in the cell by itself. Python cells can't write files, so this line would fail.", A::None),
    k("lint.browserPlots", Warning, Line, "This package draws in a web browser, which Python cells can't open. Use matplotlib or seaborn.", A::None),
    k("lint.window", Warning, Line, "Python cells can't open windows or ask for input.", A::None),
    k("lint.environ", Info, Line, "Python cells don't see your environment variables.", A::None),
];

/// The catalog's entry with this id.
pub fn kind(id: &str) -> Option<&'static Kind> {
    CATALOG.iter().find(|k| k.id == id)
}

/// One thing that is wrong, or worth saying.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: &'static Kind,
    /// Python's own message, the module or file concerned, or how much: the specifics.
    pub detail: String,
    /// The 1-based line in the cell's code, when it has one.
    pub line: Option<u32>,
}

impl Diagnostic {
    /// A diagnostic of the catalog's kind `id`. An id that isn't in the catalog reads as a plain
    /// Python error, since ids can come from the worker, which is untrusted.
    pub fn new(id: &str, detail: impl Into<String>, line: Option<u32>) -> Diagnostic {
        let kind = kind(id).unwrap_or(PYTHON_EXCEPTION);
        Diagnostic { kind, detail: detail.into(), line }
    }
}

// The catalog is a constant list whose entries the tests check; a missing id falls back to this.
const PYTHON_EXCEPTION: &Kind = &k("python.exception", Error, Cell, "The code raised an error.", A::None);

/// Which kinds a worker may report for an error it raised: only the cell-level ones about code.
/// Anything else it names (it is untrusted) reads as a plain error.
const WORKER_KINDS: [&str; 10] = [
    "python.syntax",
    "python.exception",
    "python.name",
    "module.missing",
    "module.blockedLibrary",
    "denied.file",
    "denied.network",
    "denied.process",
    "plot.tooLarge",
    "xl.notLiteral",
];

/// The diagnostic for an error the code raised.
pub fn for_error(error: &PyError) -> Diagnostic {
    let id = error.kind.as_deref().filter(|id| WORKER_KINDS.contains(id)).unwrap_or("python.exception");
    let detail = match &error.subject {
        Some(subject) if id != "python.exception" && !error.message.contains(subject.as_str()) => format!("{subject}: {}", error.message),
        _ => error.message.clone(),
    };
    Diagnostic::new(id, detail, error.line)
}

/// The launcher's line when it refused to run the program.
const REFUSED: &str = "gridcraft-sandbox: refused: ";

/// The diagnostic for how a request ended, when it wasn't a clean result.
pub fn for_outcome(outcome: &Outcome) -> Option<Diagnostic> {
    Some(match outcome {
        Outcome::Ok { .. } => return None,
        Outcome::PythonError { error, .. } => for_error(error),
        Outcome::TimedOut => Diagnostic::new("limit.time", "", None),
        Outcome::OverMemory { rss_bytes } => Diagnostic::new("limit.memory", format!("{} MB in use", rss_bytes / (1024 * 1024)), None),
        Outcome::OutputTooLarge => Diagnostic::new("limit.result", "", None),
        Outcome::ProtocolViolation(why) | Outcome::Crashed(why) => Diagnostic::new("worker.lost", why.clone(), None),
        Outcome::FailedToStart(why) => match why.lines().find_map(|l| l.split_once(REFUSED)) {
            Some((_, reason)) => Diagnostic::new("blocked.sandboxCheck", reason, None),
            None => Diagnostic::new("connect.failed", why.clone(), None),
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::run::Outcome;

    #[test]
    fn ids_are_unique_and_shaped_alike() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|k| k.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
        for k in CATALOG {
            assert!(k.id.split_once('.').is_some(), "{}", k.id);
            assert!(k.message.ends_with('.'), "{}", k.id);
        }
    }

    #[test]
    fn the_fallback_matches_the_catalog() {
        assert_eq!(kind("python.exception"), Some(PYTHON_EXCEPTION));
    }

    #[test]
    fn a_kind_from_the_worker_is_not_trusted_past_its_own_cell() {
        let e = |kind: &str| PyError { message: "boom".into(), kind: Some(kind.into()), ..PyError::default() };
        assert_eq!(for_error(&e("denied.network")).kind.id, "denied.network");
        assert_eq!(for_error(&e("denied.process")).kind.id, "denied.process");
        assert_eq!(for_error(&e("xl.notLiteral")).kind.id, "xl.notLiteral");
        // Code that names an environment state, or something made up, is only an error.
        for forged in ["blocked.sandboxCheck", "off.app", "made.up", ""] {
            assert_eq!(for_error(&e(forged)).kind.id, "python.exception", "{forged}");
        }
    }

    #[test]
    fn every_kind_the_worker_names_is_accepted_and_in_the_catalog() {
        let shim = include_str!("shim.py");
        let classify = &shim[shim.find("def classify").unwrap()..shim.find("def socket_errors").unwrap()];
        let mut named: Vec<&str> = WORKER_KINDS.to_vec();
        for piece in classify.split('"').skip(1).step_by(2).filter(|p| {
            p.split_once('.').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && p.chars().all(|c| c.is_ascii_alphabetic() || c == '.'))
        }) {
            named.push(piece);
            assert!(WORKER_KINDS.contains(&piece), "{piece} is raised by the shim but not accepted");
        }
        for id in named {
            assert!(kind(id).is_some(), "{id} isn't in the catalog");
        }
        assert!(WORKER_KINDS.contains(&"denied.process"), "denied_kind's answers are accepted too");
        assert!(WORKER_KINDS.contains(&"denied.network"));
        assert!(WORKER_KINDS.contains(&"denied.file"));
    }

    #[test]
    fn the_subject_joins_the_message_once() {
        let e = PyError { message: "No module named 'x'".into(), kind: Some("module.missing".into()), subject: Some("x".into()), line: Some(3) };
        let d = for_error(&e);
        assert_eq!((d.detail.as_str(), d.line), ("No module named 'x'", Some(3)));
        let e = PyError { message: "not permitted".into(), kind: Some("denied.file".into()), subject: Some("/a/b".into()), line: None };
        assert_eq!(for_error(&e).detail, "/a/b: not permitted");
    }

    #[test]
    fn how_a_run_ended_maps_to_a_kind() {
        let id = |o: Outcome| for_outcome(&o).map(|d| d.kind.id);
        assert_eq!(id(Outcome::TimedOut), Some("limit.time"));
        assert_eq!(id(Outcome::OverMemory { rss_bytes: 5 }), Some("limit.memory"));
        assert_eq!(id(Outcome::OutputTooLarge), Some("limit.result"));
        assert_eq!(id(Outcome::Crashed("x".into())), Some("worker.lost"));
        assert_eq!(id(Outcome::FailedToStart("no such file".into())), Some("connect.failed"));
        let refused = Outcome::FailedToStart("Python stopped: gridcraft-sandbox: refused: the sandbox is not enforced: read the canary".into());
        let d = for_outcome(&refused).unwrap();
        assert_eq!((d.kind.id, d.detail.as_str()), ("blocked.sandboxCheck", "the sandbox is not enforced: read the canary"));
    }
}
