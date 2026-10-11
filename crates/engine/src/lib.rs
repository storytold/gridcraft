//! The GridCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`cell.set`, `home.bold`,
//! `data.sortAscending`…) and JSON parameters. The egui UI, the CLI, the control channel and MCP
//! all go through [`Session::execute`], so every action is scriptable and journaled.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod catalog;
pub mod cf;
pub mod cmd;
pub mod display;
pub mod fill;
pub mod io;
pub mod pivot;
pub mod sample;
pub mod selection;
pub mod tables;

use std::sync::Arc;

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::Workbook;
use serde::Serialize;
use serde_json::Value as Json;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use gridcraft_calc as calc;
pub use gridcraft_core as core;
pub use gridcraft_formula as formula;
pub use gridcraft_model as model;
pub use gridcraft_numfmt as numfmt;
pub use selection::Selection;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no workbook open")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    #[error("internal error in `{0}` (the workbook was kept as it was): {1}")]
    Internal(String, String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub wb: Arc<Workbook>,
    pub selection: Selection,
}

/// The clipboard: a block of cells (with formulas and styles) or marching-ants reference.
#[derive(Clone, Debug)]
pub struct Clipboard {
    /// Source workbook snapshot, sheet and range.
    pub wb: Arc<Workbook>,
    pub sheet: usize,
    pub range: RangeRef,
    pub cut: bool,
    /// Document uid of the source (a cut only moves within the same workbook).
    pub doc_uid: u64,
    /// Plain text (tab-separated) for the system clipboard.
    pub text: String,
}

/// In-cell edit mode as shown in the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Mode {
    #[default]
    Ready,
    Enter,
    Edit,
    Point,
}

#[derive(Clone, Debug)]
pub struct DocState {
    pub wb: Arc<Workbook>,
    pub calc: Calc,
    pub selection: Selection,
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    pub path: Option<String>,
    pub saved: Arc<Workbook>,
    pub revision: u64,
    pub uid: u64,
    pub title: String,
    /// Per-sheet selection memory.
    pub sheet_selections: Vec<Selection>,
    /// Formulas still to recalculate, and rows to fit afterwards: what an edit leaves for a
    /// background recalculation (see [`Session::set_background_calc`]).
    pub(crate) deferred: Vec<gridcraft_calc::Key>,
    pub(crate) deferred_rows: Vec<(usize, u32)>,
    /// Whether edits may leave large recalculations for another thread.
    pub(crate) background: bool,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const UNDO_LIMIT: usize = 100;

impl DocState {
    pub fn new(wb: Workbook, path: Option<String>, title: String) -> DocState {
        DocState::opened(wb, path, title, None)
    }

    /// A document for a workbook read from a file: `stale` as [`crate::io::open_bytes_with_plan`]
    /// gives it (`None`: recalculate everything; otherwise keep the file's values except for
    /// those formulas and what depends on them).
    pub fn opened(mut wb: Workbook, path: Option<String>, title: String, stale: Option<Vec<gridcraft_calc::Key>>) -> DocState {
        let mut calc = Calc::new();
        match stale {
            Some(stale) => calc.load(&mut wb, &stale),
            None => calc.recalc_all(&mut wb),
        }
        let wb = Arc::new(wb);
        let sel = Selection::at(wb.active().map(|s| s.view_active).unwrap_or_default());
        DocState {
            saved: wb.clone(),
            wb,
            calc,
            selection: sel,
            undo: vec![],
            redo: vec![],
            path,
            revision: 1,
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            title,
            sheet_selections: vec![],
            deferred: Vec::new(),
            deferred_rows: Vec::new(),
            background: false,
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.wb, &self.saved)
    }
    pub fn sheet_index(&self) -> usize {
        self.wb.active_sheet
    }
    pub fn display_title(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.title.clone())
    }
}

/// Engine-side preferences.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// After pressing Enter, move the selection: "down", "right", "up", "left", "none".
    pub enter_direction: String,
    pub autocomplete: bool,
    pub user_name: String,
    pub default_font: String,
    pub default_font_size: f32,
    pub sheets_in_new_workbook: usize,
    pub r1c1: bool,
    /// Words added with Add to Dictionary.
    pub user_dictionary: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            enter_direction: "down".into(),
            autocomplete: true,
            user_name: "GridCraft User".into(),
            default_font: gridcraft_model::DEFAULT_FONT.into(),
            default_font_size: gridcraft_model::DEFAULT_FONT_SIZE,
            sheets_in_new_workbook: 1,
            r1c1: false,
            user_dictionary: vec![],
        }
    }
}

/// Requests from commands to the UI (open a dialog, show a message…).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum UiRequest {
    Dialog(String, Json),
    Message(String),
    /// Start editing the active cell (F2) with optional initial text.
    EditCell(Option<String>),
    OpenUrl(String),
}

#[derive(Default)]
pub struct Session {
    docs: Vec<DocState>,
    active: usize,
    pub clipboard: Option<Clipboard>,
    pub prefs: Prefs,
    pub mode: Mode,
    pub journal: Vec<(String, Json)>,
    pub ui_requests: Vec<UiRequest>,
    /// Format painter source style (sticky when double-clicked).
    pub format_painter: Option<(Arc<Workbook>, usize, RangeRef, bool)>,
    /// Find state (last search).
    pub last_find: Option<Json>,
    /// Workbook view: normal, pageLayout or pageBreakPreview.
    pub view_mode: String,
    /// Journal position where action recording started.
    pub recording: Option<usize>,
    /// Windows hidden with View › Hide.
    pub hidden_windows: Vec<usize>,
    /// Draw tab tool: "" (select), "pen" or "eraser"; pen colour and width.
    pub draw_tool: String,
    pub draw_color: String,
    pub draw_width: f32,
    /// Large recalculations run on another thread (see [`Session::set_background_calc`]).
    background_calc: bool,
    jobs: Vec<CalcJob>,
    /// Nesting of `execute` (commands run other commands).
    depth: u32,
}

/// A recalculation running on another thread: it evaluates a copy of the workbook as it was
/// after the edit (`base`) and sends the copy back with the calculation state.
struct CalcJob {
    uid: u64,
    base: Arc<Workbook>,
    rows: Vec<(usize, u32)>,
    progress: Arc<gridcraft_calc::CalcProgress>,
    rx: std::sync::mpsc::Receiver<std::thread::Result<(Workbook, Calc)>>,
}

/// How long a command waits for its recalculation before letting it finish in the background:
/// short enough that the window keeps responding, long enough that ordinary edits show their
/// results at once.
const BACKGROUND_WAIT: std::time::Duration = std::time::Duration::from_millis(80);

/// Commands that run while a recalculation is in progress: they move around and don't read or
/// change values. Every other command waits for the recalculation first, so it sees final
/// values and every undo step is calculated.
fn runs_while_calculating(id: &str) -> bool {
    matches!(
        id,
        "selection.set"
            | "selection.move"
            | "selection.next"
            | "selection.currentRegion"
            | "selection.row"
            | "selection.column"
            | "sheet.activate"
            | "sheet.next"
            | "sheet.previous"
    )
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }
    pub fn documents(&self) -> &[DocState] {
        &self.docs
    }
    pub fn active_index(&self) -> usize {
        self.active
    }
    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() {
            self.active = i;
        }
    }
    pub fn active(&self) -> Option<&DocState> {
        self.docs.get(self.active)
    }
    pub fn active_mut(&mut self) -> Option<&mut DocState> {
        self.docs.get_mut(self.active)
    }
    pub fn doc(&self) -> Result<&DocState> {
        self.active().ok_or(EngineError::NoDocument)
    }
    pub fn doc_mut(&mut self) -> Result<&mut DocState> {
        self.active_mut().ok_or(EngineError::NoDocument)
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
        self.docs.push(d);
        self.active = self.docs.len() - 1;
        self.active
    }
    pub fn close_document(&mut self, i: usize) {
        if i < self.docs.len() {
            self.docs.remove(i);
            if self.active >= self.docs.len() {
                self.active = self.docs.len().saturating_sub(1);
            }
        }
    }
    /// A new blank workbook titled `BookN`.
    pub fn new_workbook(&mut self) -> usize {
        let n = (1..).find(|n| !self.docs.iter().any(|d| d.title == format!("Book{n}") && d.path.is_none())).unwrap_or(1);
        let mut wb = Workbook::new();
        for _ in 1..self.prefs.sheets_in_new_workbook.clamp(1, 255) {
            let name = wb.next_sheet_name();
            wb.sheets.push(Arc::new(gridcraft_model::Sheet::new(name)));
        }
        self.add_document(DocState::new(wb, None, format!("Book{n}")))
    }

    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }

    /// Runs a command by id. Undoable commands record a history step when the workbook changed.
    /// Panics inside commands are caught and reported; the workbook is restored.
    pub fn execute(&mut self, id: &str, params: Json) -> Result<Json> {
        let top = self.depth == 0;
        if top {
            if !self.jobs.is_empty() && !runs_while_calculating(id) {
                self.finish_calc();
            }
            let background = self.background_calc;
            for d in &mut self.docs {
                d.background = background;
            }
        }
        self.depth += 1;
        let r = self.execute_command(id, params);
        self.depth -= 1;
        if top {
            self.start_deferred();
        }
        r
    }

    fn execute_command(&mut self, id: &str, params: Json) -> Result<Json> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if let Err(why) = (spec.enabled)(self) {
            return Err(EngineError::Disabled(id.to_string(), why));
        }
        let before = self.active().map(|d| (d.uid, d.wb.clone(), d.selection.clone()));
        let params = if params.is_null() { Json::Object(Default::default()) } else { params };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, &params)));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                // Restore the workbook as it was.
                if let Some((uid, wb, sel)) = &before
                    && let Some(d) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    d.wb = wb.clone();
                    d.selection = sel.clone();
                    d.calc.rebuild(&d.wb);
                    d.deferred.clear();
                    d.deferred_rows.clear();
                }
                return Err(EngineError::Internal(id.to_string(), msg));
            }
        };
        if spec.journal && result.is_ok() {
            self.journal.push((id.to_string(), params.clone()));
            if self.journal.len() > 10_000 {
                self.journal.drain(..5_000);
            }
        }
        if spec.undoable
            && let Some((uid, wb, sel)) = before
            && let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&d.wb, &wb)
        {
            d.undo.push(HistoryEntry { label: spec.label.trim_end_matches('…').to_string(), wb, selection: sel });
            if d.undo.len() > UNDO_LIMIT {
                d.undo.remove(0);
            }
            d.redo.clear();
            d.revision += 1;
        }
        result
    }

    pub fn run(&mut self, id: &str, params: Json) -> std::result::Result<Json, String> {
        self.execute(id, params).map_err(|e| e.to_string())
    }

    /// Lets large recalculations run on another thread, as an interactive app wants: a command
    /// waits a moment for its recalculation and then returns, leaving it to finish in the
    /// background (the grid shows the previous values meanwhile, and [`Session::poll_calc`]
    /// brings the results in). Commands that move around keep working; any other command waits
    /// for the recalculation. Off by default, so programmatic callers (CLI, MCP) always see
    /// final values; never on wasm.
    pub fn set_background_calc(&mut self, on: bool) {
        self.background_calc = on && !cfg!(target_arch = "wasm32");
    }

    /// Whether a recalculation is running in the background.
    pub fn calculating(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// The active workbook's background recalculation: formulas done, formulas to do, threads.
    pub fn calc_progress(&self) -> Option<(usize, usize, usize)> {
        let uid = self.active()?.uid;
        let p = &self.jobs.iter().find(|j| j.uid == uid)?.progress;
        let get = |a: &std::sync::atomic::AtomicUsize| a.load(std::sync::atomic::Ordering::Relaxed);
        Some((get(&p.done), get(&p.total), get(&p.threads)))
    }

    /// Brings in the results of background recalculations that have finished. Returns whether
    /// any did (the window should repaint).
    pub fn poll_calc(&mut self) -> bool {
        let mut done = Vec::new();
        let mut i = 0;
        while i < self.jobs.len() {
            let r = match self.jobs.get(i).map(|j| j.rx.try_recv()) {
                Some(Ok(r)) => Some(r),
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    Some(Err(Box::new("calculation thread ended") as Box<dyn std::any::Any + Send>))
                }
                _ => None,
            };
            match r {
                Some(r) => done.push((self.jobs.remove(i), r)),
                None => i += 1,
            }
        }
        let any = !done.is_empty();
        for (job, r) in done {
            self.apply_calc(job, r);
        }
        any
    }

    /// Waits for every background recalculation and brings its results in.
    pub fn finish_calc(&mut self) {
        for job in std::mem::take(&mut self.jobs) {
            let r = job.rx.recv().unwrap_or_else(|_| Err(Box::new("calculation thread ended")));
            self.apply_calc(job, r);
        }
    }

    /// Starts the recalculations that the last command left (see `cmd::recalc`), waiting
    /// [`BACKGROUND_WAIT`] for each before leaving it to run.
    fn start_deferred(&mut self) {
        for i in 0..self.docs.len() {
            let Some(d) = self.docs.get_mut(i) else { continue };
            if d.deferred.is_empty() {
                continue;
            }
            let dirty = std::mem::take(&mut d.deferred);
            let rows = std::mem::take(&mut d.deferred_rows);
            let progress = Arc::new(gridcraft_calc::CalcProgress::default());
            let mut calc = std::mem::take(&mut d.calc);
            calc.progress = Some(Arc::clone(&progress));
            let base = Arc::clone(&d.wb);
            let input = Arc::clone(&base);
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("gridcraft-recalc".into()).stack_size(16 << 20).spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut wb = (*input).clone();
                    calc.run_dirty(&mut wb, dirty);
                    (wb, calc)
                }));
                let _ = tx.send(r);
            });
            let job = CalcJob { uid: d.uid, base, rows, progress, rx };
            if spawned.is_err() {
                // The thread didn't start: its closure, and the calculation state with it, is gone.
                self.apply_calc(job, Err(Box::new("calculation thread did not start")));
                continue;
            }
            match job.rx.recv_timeout(BACKGROUND_WAIT) {
                Ok(r) => self.apply_calc(job, r),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => self.jobs.push(job),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => self.apply_calc(job, Err(Box::new("calculation thread ended"))),
            }
        }
    }

    /// Brings a finished background recalculation into its workbook: the values and spills it
    /// worked out replace those of the cells it evaluated, whatever the commands that ran
    /// meanwhile (moving around) changed elsewhere. If it failed, the workbook is recalculated
    /// here instead.
    fn apply_calc(&mut self, job: CalcJob, r: std::thread::Result<(Workbook, Calc)>) {
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == job.uid) else { return };
        match r {
            Ok((done, mut calc)) => {
                calc.progress = None;
                d.calc = calc;
                let wb = Arc::make_mut(&mut d.wb);
                for (si, after) in done.sheets.iter().enumerate() {
                    let (Some(before), Some(live)) = (job.base.sheets.get(si), wb.sheets.get_mut(si)) else { continue };
                    let changed = after.cells.diff(&before.cells);
                    let spills = after.spill != before.spill || after.spill_ranges != before.spill_ranges;
                    if changed.is_empty() && !spills {
                        continue;
                    }
                    let live = Arc::make_mut(live);
                    for c in changed {
                        if let (Some(new), Some(cell)) = (after.cells.get(c), live.cells.get_mut(c))
                            && cell.formula.is_some()
                        {
                            cell.value = new.value.clone();
                        }
                    }
                    if spills {
                        live.spill = after.spill.clone();
                        live.spill_ranges = after.spill_ranges.clone();
                    }
                }
            }
            Err(_) => {
                log::warn!("background recalculation failed; recalculating on this thread");
                let fixed_now = d.calc.fixed_now;
                d.calc = Calc::new();
                d.calc.fixed_now = fixed_now;
                let wb = Arc::make_mut(&mut d.wb);
                d.calc.recalc_all(wb);
            }
        }
        let mut rows = job.rows;
        rows.sort_unstable();
        rows.dedup();
        let wb = Arc::make_mut(&mut d.wb);
        for (si, row) in rows {
            cmd::auto_row_height(wb, si, row);
        }
    }

    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    /// Runs a command for a caller with no UI (MCP, CLI): queued UI requests are dropped, and a command that only
    /// queued a file dialog because it got no file (`file.open`, `file.saveAs`, `file.save` on a workbook with no
    /// file yet, `data.getData`) is an error instead of a silent success.
    pub fn execute_headless(&mut self, id: &str, params: Json) -> Result<Json> {
        let r = self.execute(id, params);
        let requests = self.take_ui_requests();
        let asked_for_file = requests.iter().any(|q| matches!(q, UiRequest::Dialog(name, _) if name == "open" || name == "saveAs"));
        if r.is_ok() && asked_for_file {
            let what = match id {
                "file.open" => "`path` or `base64`",
                "data.getData" | "data.fromTextCsv" => "`path` or `text`",
                _ => "`path`",
            };
            let msg = format!("missing {what}: programmatic calls never open dialogs");
            return Err(EngineError::BadParams { cmd: id.to_string(), msg });
        }
        r
    }
}

/// Convenience: the active workbook's active sheet.
pub fn active_sheet(s: &Session) -> Option<&gridcraft_model::Sheet> {
    s.active().and_then(|d| d.wb.active())
}

pub fn cell(r: u32, c: u32) -> CellRef {
    CellRef::new(r, c)
}

#[cfg(test)]
mod tests;
