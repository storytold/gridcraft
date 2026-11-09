//! Waiting for a workbook's Python cells, so that a save gets their results. A [`PythonWait`] is a
//! deadline and the cells it has seen; it never blocks, and the interface polls it every frame. The
//! one call that blocks is `file.save` with `wait`, for a caller with no frames to keep going
//! ([`Session::python_settle_for_save`]): the interface takes `wait` off before it runs a save.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use gridcraft_core::{CellRef, quote_sheet};
use serde_json::{Value as Json, json};
use web_time::Instant;

use super::plan::{CellState, DocRun};
use super::runner::now;
use crate::{DocState, Session};

/// The longest a wait lasts, whatever the cells or the caller ask.
pub const MAX_WAIT: Duration = Duration::from_secs(120);
/// How long a result may take to arrive once a time limit has stopped its worker.
const GRACE: Duration = Duration::from_secs(1);
/// How many cells a reply names; `unfinishedCount` says how many there are.
const MAX_NAMED: usize = 1_000;
/// How long a blocked caller waits for a result before it looks at the time again.
const BLOCKED_POLL: Duration = Duration::from_millis(50);

/// How long a wait may last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitBound {
    /// As long as the cells left may take: the time Python may need to start, plus the time limit
    /// for each of them, plus a second, and never past [`MAX_WAIT`].
    Cells,
    /// This long, and never past [`MAX_WAIT`].
    At(Duration),
}

/// Where a wait stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitState {
    Waiting,
    /// Nothing is running or queued any more.
    Idle,
    /// The cap came first.
    AtCap,
}

/// A wait for the Python cells of one workbook.
#[derive(Debug)]
pub struct PythonWait {
    doc: u64,
    started: Instant,
    /// How long it lasts from its start.
    cap: Duration,
    /// The caller chose the cap, so more work doesn't add to it.
    fixed: bool,
    /// It also waits for the check of Python to answer.
    environment: bool,
    /// How much work was left at the last look.
    left: usize,
    /// Every cell it has seen queued or running, by sheet identity.
    seen: HashSet<(u64, CellRef)>,
}

impl PythonWait {
    /// The workbook it waits for ([`crate::DocState::uid`]).
    pub fn doc(&self) -> u64 {
        self.doc
    }

    /// How long it lasts from its start.
    pub fn cap(&self) -> Duration {
        self.cap
    }

    /// How long it has lasted.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Ends the wait within `bound` from now, if it would otherwise last longer.
    pub fn limit_to(&mut self, bound: Duration) {
        let end = self.elapsed().saturating_add(bound);
        if end < self.cap {
            self.cap = end;
            self.fixed = true;
        }
    }

    /// A shorter cap, so a test doesn't sit through the real one.
    #[doc(hidden)]
    pub fn set_cap(&mut self, cap: Duration) {
        self.cap = cap;
        self.fixed = true;
    }
}

/// What `wait` and `waitSeconds` in these params ask for: `None` for no wait. `wait: false` decides,
/// `waitSeconds` implies `wait`, and past [`MAX_WAIT`] it is cut to that.
pub fn wait_asked(p: &Json) -> Result<Option<WaitBound>, String> {
    let wait = match p.get("wait") {
        None | Some(Json::Null) => None,
        Some(Json::Bool(b)) => Some(*b),
        Some(_) => return Err("`wait` must be true or false".into()),
    };
    let seconds = match p.get("waitSeconds") {
        None | Some(Json::Null) => None,
        Some(v) => match v.as_f64().filter(|s| s.is_finite() && *s >= 0.0) {
            Some(s) => Some(s),
            None => return Err("`waitSeconds` must be a number of seconds, 0 or more".into()),
        },
    };
    Ok(match (wait, seconds) {
        (Some(false), _) | (None, None) => None,
        (_, Some(s)) => Some(WaitBound::At(Duration::try_from_secs_f64(s).map_or(MAX_WAIT, |d| d.min(MAX_WAIT)))),
        (Some(true), None) => Some(WaitBound::Cells),
    })
}

/// Whether a control request with these params may hold its reply for up to [`MAX_WAIT`]: a client
/// that gives up sooner would see a save that is still coming as a failure.
pub fn reply_may_be_held(p: &Json) -> bool {
    let asked = |q: &Json| matches!(wait_asked(q), Ok(Some(_)));
    asked(p) || p.get("params").is_some_and(asked)
}

/// Says in a reply which cells weren't finished, when the caller asked to wait or there are any.
pub fn note_unfinished(reply: &mut Json, asked: bool, names: &[String]) {
    if !asked && names.is_empty() {
        return;
    }
    let named: Vec<&String> = names.iter().take(MAX_NAMED).collect();
    if !reply.is_object() {
        *reply = json!({"result": reply.take()});
    }
    if let Some(o) = reply.as_object_mut() {
        o.insert("unfinished".into(), json!(named));
        o.insert("unfinishedCount".into(), json!(names.len()));
    }
}

/// The work left in a workbook, to set a cap by: the cells queued or running, the ones before them
/// that a fresh worker must run again, and the initialization code when it has yet to run.
fn work_left(r: &DocRun, d: Option<&DocState>) -> usize {
    let cells = r
        .entries
        .iter()
        .rposition(|e| e.state == CellState::Queued)
        .map_or(0, |last| r.entries.iter().take(last.saturating_add(1)).filter(|e| e.state == CellState::Queued || !e.in_worker).count());
    let init = d.is_some_and(|d| d.wb.python_env.as_ref().is_some_and(|e| e.has_code())) && !r.init.in_worker;
    cells.saturating_add(usize::from(init)).saturating_add(usize::from(r.requesting() && r.due == 0))
}

/// A cell's name as a reply gives it.
fn name_of(d: &DocState, sheet: usize, at: CellRef) -> Option<String> {
    d.wb.sheet(sheet).map(|sh| format!("{}!{}", quote_sheet(&sh.name), at.a1()))
}

impl Session {
    /// Whether a workbook's Python cells are running or queued. A cell stale under manual calculation
    /// is neither. Saving waits for these, and AutoSave doesn't start.
    pub fn python_workbook_busy(&self, doc: u64) -> bool {
        self.python.docs.get(&doc).is_some_and(|r| r.requesting() || r.due > 0)
    }

    /// The cells of a workbook that are running or queued now, in run order, as `Sheet1!B2`.
    pub fn python_unfinished(&self, doc: u64) -> Vec<String> {
        let (Some(d), Some(r)) = (self.docs.iter().find(|d| d.uid == doc), self.python.docs.get(&doc)) else { return Vec::new() };
        r.entries.iter().filter(|e| e.state == CellState::Queued).filter_map(|e| name_of(d, e.sheet, e.at)).collect()
    }

    fn python_cap(&self, left: usize) -> Duration {
        let limits = self.python.capability.as_ref().map_or_else(super::Limits::default, |c| c.limits.clamped());
        let each = limits.timeout.saturating_mul(u32::try_from(left.max(1)).unwrap_or(u32::MAX));
        limits.start_timeout.saturating_add(each).saturating_add(GRACE).min(MAX_WAIT)
    }

    /// Starts a wait for a workbook's Python cells, with the check of Python as well when
    /// `environment`. `None` when there is nothing to wait for.
    pub fn python_wait_begin(&self, doc: u64, bound: WaitBound, environment: bool) -> Option<PythonWait> {
        if !self.python_workbook_busy(doc) && !(environment && self.python.env.checking()) {
            return None;
        }
        let d = self.docs.iter().find(|d| d.uid == doc);
        let left = self.python.docs.get(&doc).map_or(0, |r| work_left(r, d));
        let (cap, fixed) = match bound {
            WaitBound::Cells => (self.python_cap(left), false),
            WaitBound::At(d) => (d.min(MAX_WAIT), true),
        };
        let mut wait = PythonWait { doc, started: now(), cap, fixed, environment, left, seen: HashSet::new() };
        self.python_wait_note(&mut wait);
        Some(wait)
    }

    /// Notes the cells queued now, and more work than the cap was set for.
    fn python_wait_note(&self, w: &mut PythonWait) {
        let Some(r) = self.python.docs.get(&w.doc) else { return };
        w.seen.extend(r.entries.iter().filter(|e| e.state == CellState::Queued).map(|e| (e.uid, e.at)));
        let left = work_left(r, self.docs.iter().find(|d| d.uid == w.doc));
        if !w.fixed && left > w.left {
            let more = u32::try_from(left.saturating_sub(w.left)).unwrap_or(u32::MAX);
            let limits = self.python.capability.as_ref().map_or_else(super::Limits::default, |c| c.limits.clamped());
            w.cap = w.cap.saturating_add(limits.timeout.saturating_mul(more)).min(MAX_WAIT);
        }
        w.left = left;
    }

    /// Where a wait stands. It takes no results: [`Session::python_poll`] does that.
    pub fn python_wait_poll(&self, w: &mut PythonWait) -> WaitState {
        self.python_wait_note(w);
        if !self.python_workbook_busy(w.doc) && !(w.environment && self.python.env.checking()) {
            WaitState::Idle
        } else if w.started.elapsed() >= w.cap {
            WaitState::AtCap
        } else {
            WaitState::Waiting
        }
    }

    /// The cells a wait saw that have no result of their own now, in run order: still queued or
    /// running, stale because someone pressed Stop or a worker kept going away, stopped, left stale
    /// by cells that feed each other, or waiting when Python was turned off. A cell that was deleted
    /// meanwhile isn't one.
    pub fn python_wait_unfinished(&self, w: &PythonWait) -> Vec<String> {
        let Some(d) = self.docs.iter().find(|d| d.uid == w.doc) else { return Vec::new() };
        let r = self.python.docs.get(&w.doc);
        let sheets: HashMap<u64, usize> = d.wb.sheets.iter().enumerate().map(|(i, s)| (s.uid.get(), i)).collect();
        let finished = |sheet: usize, at: CellRef| {
            let entry = r.and_then(|r| r.entries.get(r.index_of(sheet, at)?));
            entry.is_some_and(|e| e.state == CellState::Clean && !e.unsettled && e.diag.as_ref().is_none_or(|x| x.kind.id != "state.stopped"))
        };
        let cells: BTreeSet<(usize, CellRef)> = w
            .seen
            .iter()
            .filter_map(|(uid, at)| sheets.get(uid).map(|i| (*i, *at)))
            .filter(|(i, at)| d.wb.sheet(*i).and_then(|sh| sh.cell(*at)).is_some_and(|c| c.py.is_some()))
            .filter(|(i, at)| !finished(*i, *at))
            .collect();
        cells.into_iter().filter_map(|(i, at)| name_of(d, i, at)).collect()
    }

    /// What `file.save` does before it writes: with `bound`, in a command of its own, it blocks until
    /// the active workbook's cells have finished or the cap comes. A command inside another (a
    /// script's step) doesn't block. Returns the cells that weren't finished.
    pub(crate) fn python_settle_for_save(&mut self, bound: Option<WaitBound>) -> Vec<String> {
        let Some(doc) = self.active().map(|d| d.uid) else { return Vec::new() };
        let begun = bound.filter(|_| self.depth() <= 1).and_then(|b| self.python_wait_begin(doc, b, false));
        let Some(mut wait) = begun else { return self.python_unfinished(doc) };
        while self.python_wait_poll(&mut wait) == WaitState::Waiting {
            if !self.python_poll(BLOCKED_POLL) {
                // Nothing was ready and nothing could be waited on (the web build never gets here:
                // its cells don't run).
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.python_wait_unfinished(&wait)
    }
}
