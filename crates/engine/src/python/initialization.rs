//! The workbook's initialization code ([`gridcraft_model::PyEnvironment`]): a fresh worker runs it
//! before any cell, in the variables the cells share, and changing it starts everything over. It
//! holds no cell, so what it did is the workbook's state here, and every Python cell shows a failure
//! until it is fixed: by changing the code, by someone asking cells to run, or by a new list of
//! packages. It counts as a lost worker and is timed like a cell, and each run has a row in the log.

use std::sync::Arc;
use std::time::Duration;

use gridcraft_core::{CellError, Value};
use gridcraft_model::python::MAX_INIT_CHARS;
use gridcraft_model::{PyEnvironment, Workbook};
use gridcraft_python::diagnostics::{Diagnostic, for_outcome};
use gridcraft_python::protocol::Init;
use gridcraft_python::{Outcome, Output, RunResult};
use web_time::Instant;

use super::inputs::Reader;
use super::plan::{CellState, DocRun};
use super::runner::{Finish, START_RETRIES, Starter, Step, cut_output};
use super::{PythonRun, PythonStats};
use crate::DocState;

/// How many skipped packages a run's report keeps. What a worker sends is untrusted.
const MAX_SKIPPED: usize = 100;
/// The most characters of a package's name that are kept.
const MAX_MODULE_CHARS: usize = 200;

/// What the initialization code did in this workbook.
#[derive(Default)]
pub(super) struct InitRun {
    /// When the request running it was sent, with its id.
    pub flight: Option<(u64, Instant)>,
    /// The current worker has run it.
    pub in_worker: bool,
    /// It failed: what every Python cell says until it is fixed.
    pub failed: Option<Diagnostic>,
    /// Someone stopped it: the cells wait, stale, until someone asks them to run, so that code that
    /// never ends doesn't start again with the next edit.
    pub stopped: bool,
    /// What it skipped, one `init.skipped` for each package, as of its last run.
    pub skipped: Vec<Diagnostic>,
}

/// Whose code it is, which decides what a failing line means.
fn whose(env: &PyEnvironment) -> Init {
    if env.user_modified { Init::Person } else { Init::File }
}

/// What in the workbook decides how the initialization runs: its code, and whose it is. Anything
/// else a file says about the environment doesn't make cells run again.
pub(super) fn same_run(old: &Workbook, new: &Workbook) -> bool {
    match (&old.python_env, &new.python_env) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || (a.code == b.code && a.user_modified == b.user_modified),
        (None, None) => true,
        (Some(e), None) | (None, Some(e)) => !e.has_code(),
    }
}

/// The packages the worker's reply says it skipped: rows of `module, line`.
fn skipped_lines(output: &Output) -> Vec<Diagnostic> {
    let Output::Value(Value::Array(rows)) = output else { return Vec::new() };
    let row = |r: usize| {
        let Some(Value::Text(module)) = rows.get(r, 0) else { return None };
        let line = match rows.get(r, 1) {
            Some(Value::Number(n)) if *n >= 1.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Some(*n as u32),
            _ => None,
        };
        Some(Diagnostic::new("init.skipped", module.chars().take(MAX_MODULE_CHARS).collect::<String>(), line))
    };
    (0..rows.rows.min(MAX_SKIPPED)).filter_map(row).collect()
}

impl PythonRun {
    fn initialization(duration: Duration, stdout: String, stdout_cut: bool, diagnostic: Option<&'static str>) -> PythonRun {
        PythonRun { sheet: String::new(), cell: gridcraft_core::CellRef::new(0, 0), duration, stdout, stdout_cut, diagnostic, initialization: true }
    }
}

impl DocRun {
    /// Whether a request is out to the worker: a cell's, or the initialization's.
    pub fn requesting(&self) -> bool {
        self.inflight.is_some() || self.init.flight.is_some() || self.render.is_some()
    }

    /// What a step does before the next cell can go: `None` once the cells can run. Settles what
    /// is due when the code has failed, and otherwise runs it in a worker that hasn't.
    pub(super) fn initialization_step(&mut self, d: &mut DocState, starter: &Starter, stats: &mut PythonStats) -> Option<Step> {
        if self.init.failed.is_some() {
            self.show_failure(d, stats);
            return Some(Step::Wrote);
        }
        let env = d.wb.python_env.clone().filter(|e| e.has_code() && !self.init.in_worker)?;
        if !env.fits() {
            let why = format!("The code is longer than {MAX_INIT_CHARS} characters, so it doesn't run until it is shortened.");
            let diag = Diagnostic::new("init.failed", why, None);
            self.log_run(PythonRun::initialization(Duration::ZERO, String::new(), false, Some(diag.kind.id)));
            self.init.failed = Some(diag);
            self.show_failure(d, stats);
            return Some(Step::Wrote);
        }
        match self.send(starter, stats, |client| client.submit_init(&env.code, whose(&env))) {
            Ok(id) => {
                self.init.flight = Some((id, super::runner::now()));
                Some(Step::Submitted)
            }
            Err(step) => Some(step),
        }
    }

    /// Writes what a finished run of the code gave.
    pub(super) fn finish_initialization(&mut self, d: &mut DocState, result: RunResult, stats: &mut PythonStats) -> Finish {
        let mut out = Finish::default();
        let Some((_, started)) = self.init.flight.take() else { return out };
        let outcome = result.outcome;
        if matches!(outcome, Outcome::FailedToStart(_)) {
            // It didn't run. A worker that won't start is Python's problem, not the code's: the cells
            // stay as they are, marked stale, and the report says why.
            self.worker_gone();
            if self.start_tries < START_RETRIES {
                self.start_tries += 1;
            } else {
                self.start_tries = 0;
                out.env_problem = for_outcome(&outcome);
            }
            return out;
        }
        self.start_tries = 0;
        let duration = started.elapsed();
        match outcome {
            Outcome::Ok { output, stdout } => {
                self.init.in_worker = true;
                self.init.skipped = skipped_lines(&output);
                let said = self.init.skipped.first().map(|x| x.kind.id);
                let (stdout, cut) = cut_output(stdout);
                self.log_run(PythonRun::initialization(duration, stdout, cut, said));
                // A worker that goes each time before a cell reaches it never gives a cell's result.
                self.give_up_after_losses();
            }
            Outcome::PythonError { error, stdout } => {
                let (stdout, cut) = cut_output(stdout);
                self.failed(d, stats, Diagnostic::new("init.failed", error.message, error.line), duration, stdout, cut);
                out.wrote = true;
            }
            lost => {
                // It stopped the worker: like a cell that does, it doesn't run again until something
                // changes or someone asks. That cost a worker.
                self.replays = self.replays.saturating_add(1);
                let said = for_outcome(&lost).map_or_else(String::new, |x| {
                    if x.detail.is_empty() { x.kind.message.to_string() } else { format!("{} ({})", x.kind.message, x.detail) }
                });
                self.failed(d, stats, Diagnostic::new("init.failed", said, None), duration, String::new(), false);
                out.wrote = true;
            }
        }
        out
    }

    /// The code failed: its worker goes, with whatever it had got done, and every cell says so.
    fn failed(&mut self, d: &mut DocState, stats: &mut PythonStats, diag: Diagnostic, duration: Duration, stdout: String, cut: bool) {
        self.log_run(PythonRun::initialization(duration, stdout, cut, Some(diag.kind.id)));
        if let Some(client) = self.client.as_mut() {
            client.stop();
        }
        self.worker_gone();
        self.init.skipped.clear();
        self.init.failed = Some(diag);
        self.show_failure(d, stats);
    }

    /// Every cell shows `#PYTHON!` and needs nothing more for now. One write for all of them, so a
    /// failure costs one recalculation however many cells there are. A cell that already shows it
    /// isn't written again.
    fn show_failure(&mut self, d: &mut DocState, stats: &mut PythonStats) {
        let wb = d.wb.clone();
        let mut reader = Reader::new(&wb, stats);
        let mut writes = Vec::new();
        for e in &mut self.entries {
            let py = wb.sheet(e.sheet).and_then(|sh| sh.cell(e.at)).and_then(|c| c.py.as_deref());
            let shows = py.is_some_and(|py| py.failed == Some(CellError::Python) && py.output.is_none() && !py.busy && !py.stale);
            if !shows {
                writes.push((e.sheet, e.at));
            }
            e.fp = reader.fingerprint(e.code_hash, &e.lits);
            e.ran = Some(e.fp);
            e.state = CellState::Clean;
            e.unsettled = false;
            e.quiet = false;
            e.in_worker = false;
            e.diag = None;
            e.shown = (false, false);
        }
        drop(reader);
        self.due = 0;
        self.cursor = 0;
        if writes.is_empty() {
            return;
        }
        let done = crate::cmd::commit(d, |cx| {
            for (sheet, at) in writes {
                let Some(cell) = cx.sheet_mut(sheet)?.cells.get_mut(at) else { continue };
                let Some(py) = cell.py.as_mut() else { continue };
                let py = Arc::make_mut(py);
                py.busy = false;
                py.stale = false;
                py.output = None;
                py.failed = Some(CellError::Python);
                cell.value = Value::Error(CellError::Python);
                cx.touch(sheet, at);
            }
            Ok(())
        });
        if let Err(e) = done {
            log::warn!("Python cells: {e}");
        }
        d.revision = d.revision.saturating_add(1);
        self.seen = Some(d.wb.clone());
    }

    /// The code changed: nothing that ran with the old code counts, so the worker goes and every cell
    /// is due.
    pub(super) fn start_over_for_initialization(&mut self) {
        if let Some(client) = self.client.as_mut() {
            client.stop();
        }
        self.inflight = None;
        self.init = InitRun::default();
        self.worker_gone();
        self.refused.clear();
        for e in &mut self.entries {
            e.ran = None;
            e.unsettled = false;
            e.quiet = false;
            e.diag = None;
        }
        self.passes = 0;
        self.replays = 0;
        self.start_tries = 0;
    }

    /// Someone asked cells to run: what the code did last no longer holds them back.
    pub(super) fn asked_to_run(&mut self) {
        self.init.failed = None;
        self.init.stopped = false;
    }

    /// `python.stop` while the code runs: its worker goes and the cells wait, stale. Returns whether it
    /// was running.
    pub(super) fn stop_initialization(&mut self, d: &mut DocState) -> bool {
        let Some((_, started)) = self.init.flight.take() else { return false };
        if let Some(client) = self.client.as_mut() {
            client.stop();
        }
        self.log_run(PythonRun::initialization(started.elapsed(), String::new(), false, Some("state.stopped")));
        self.init.stopped = true;
        self.worker_gone();
        self.demote_queued();
        self.sync_flags(d);
        self.seen = Some(d.wb.clone());
        true
    }

    /// A new list of packages may be what the code lacked: a failure is tried again, and cells become
    /// due. Returns whether there was one.
    pub(super) fn retry_failed_initialization(&mut self) -> bool {
        if self.init.failed.take().is_none() {
            return false;
        }
        for e in &mut self.entries {
            e.ran = None;
        }
        true
    }
}
