//! The GridCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`cell.set`, `home.bold`,
//! `data.sortAscending`…) and JSON parameters. The egui UI, the CLI, the control channel and MCP
//! all go through [`Session::execute`], so every action is scriptable and journaled.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod catalog;
pub mod cf;
mod clip_html;
pub mod cmd;
pub mod display;
pub mod fill;
pub mod io;
pub mod links;
pub mod macros;
pub mod outline;
pub mod paper;
pub mod pivot;
pub mod pivot_calc;
pub mod record;
pub mod sample;
pub mod selection;
pub mod tables;

use std::sync::Arc;

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::Workbook;
use serde::Serialize;
use serde_json::Value as Json;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command, register_test_command};
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
    /// The command would replace something; run it again with `overwrite: true` to go ahead.
    #[error("confirm: {0} (run again with `overwrite: true`)")]
    Confirm(String),
    /// Active content (a macro) asks to run; run it again with `approve: true` to allow it.
    #[error("approve: {0} (run again with `approve: true`)")]
    Approve(String),
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

/// An undo group opened with [`Session::group_begin`]. Hand it back to [`Session::group_end`]
/// (keep the changes as one undo step) or [`Session::group_rollback`] (undo them all).
#[derive(Debug, PartialEq, Eq)]
#[must_use = "an undo group stays open until it is ended or rolled back"]
pub struct GroupToken {
    id: u64,
    doc: u64,
}

impl GroupToken {
    /// The uid of the document the group belongs to.
    pub fn doc(&self) -> u64 {
        self.doc
    }
}

/// An open undo group: what runs on `doc` until it ends becomes one undo step.
#[derive(Clone, Debug)]
struct OpenGroup {
    id: u64,
    doc: u64,
    label: String,
    /// Undo depth when the group began; the group's own steps are cut back to it.
    start: usize,
    wb: Arc<Workbook>,
    selection: Selection,
    /// The redo stack when the group began (a rollback puts it back).
    redo: Vec<HistoryEntry>,
}

/// The clipboard: a block of cells (with formulas and styles) or marching-ants reference.
#[derive(Clone, Debug)]
pub struct Clipboard {
    /// Source workbook snapshot, sheet and range.
    pub wb: Arc<Workbook>,
    pub sheet: usize,
    pub range: RangeRef,
    /// The range as copied, before whole rows/columns were trimmed to the used range
    /// (Insert Copied Cells inserts whole lines for a row or column copy).
    pub selected: RangeRef,
    pub cut: bool,
    /// Document uid of the source (a cut only moves within the same workbook).
    pub doc_uid: u64,
    /// Plain text (tab-separated) for the system clipboard.
    pub text: String,
    /// A multi-area copy: the areas as selected (for the marquee). `wb` then holds them slid
    /// together into one block at `range`. Empty for a single-range copy.
    pub areas: Vec<RangeRef>,
    /// Trust of the source document: pasting content from a file someone else made carries it.
    pub trust: Trust,
}

/// Whether a document's active content (macros) may run without asking the person first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Trust {
    /// Made in this session: a new workbook or a sample.
    #[default]
    Local,
    /// Opened from a file, or holding content pasted from one: active content asks before it
    /// runs.
    FromFile,
}

impl Trust {
    /// Whether active content may run without asking.
    pub fn allows_code(self) -> bool {
        self == Trust::Local
    }

    /// The trust of a document after it receives content from a document with trust `source`:
    /// content from an untrusted file leaves the receiver untrusted.
    pub fn receive(self, source: Trust) -> Trust {
        if source == Trust::FromFile { Trust::FromFile } else { self }
    }
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
    /// Allow Edit Ranges unlocked with their password in this session: (sheet name, title).
    pub unlocked_ranges: Vec<(String, String)>,
    /// The format `path`'s content was recognised as when its extension doesn't name one (an
    /// XLSX called `budget.dat`): Save writes it back in that format.
    pub content_kind: Option<io::FileKind>,
    /// Whether the workbook's active content may run without asking.
    pub trust: Trust,
    /// While a macro's footprint is measured: every cell a command wrote, by sheet name, even
    /// where it wrote what was already there.
    pub touched: Option<Vec<(String, CellRef)>>,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const UNDO_LIMIT: usize = 100;

impl DocState {
    pub fn new(mut wb: Workbook, path: Option<String>, title: String) -> DocState {
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
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
            unlocked_ranges: vec![],
            content_kind: None,
            trust: Trust::Local,
            touched: None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.wb, &self.saved)
    }
    pub fn sheet_index(&self) -> usize {
        self.wb.active_sheet
    }
    /// Records the view state that is saved with the file (`sheetView topLeftCell`, the active
    /// cell): the first visible cell of the scrolling area per sheet index in `tops`, and the
    /// active sheet's active cell. View state never makes the workbook dirty.
    pub fn store_view(&mut self, tops: &[(usize, CellRef)]) {
        let active = self.selection.active;
        let si = self.wb.active_sheet;
        let changed = tops.iter().any(|(i, c)| self.wb.sheets.get(*i).is_some_and(|sh| sh.view_top_left != *c))
            || self.wb.sheets.get(si).is_some_and(|sh| sh.view_active != active);
        if !changed {
            return;
        }
        let dirty = self.is_dirty();
        let wb = Arc::make_mut(&mut self.wb);
        for (i, c) in tops {
            if let Some(sh) = wb.sheets.get_mut(*i)
                && sh.view_top_left != *c
            {
                Arc::make_mut(sh).view_top_left = *c;
            }
        }
        if let Some(sh) = wb.sheets.get_mut(si)
            && sh.view_active != active
        {
            Arc::make_mut(sh).view_active = active;
        }
        if !dirty {
            self.saved = self.wb.clone();
        }
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
    /// View › Show › Formula Bar (an application option in Excel, not saved in the file).
    pub formula_bar: bool,
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
            formula_bar: true,
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
    /// Commands run so far, without their passwords ([`cmd::journal_params`]).
    pub journal: Vec<(String, Json)>,
    pub ui_requests: Vec<UiRequest>,
    /// Format painter source style (sticky when double-clicked).
    pub format_painter: Option<(Arc<Workbook>, usize, RangeRef, bool)>,
    /// Find state (last search).
    pub last_find: Option<Json>,
    /// View › Show › Ruler is off (rulers in Page Layout view; an application option).
    pub hide_ruler: bool,
    /// Journal position where Automate › Record Actions started.
    pub recording: Option<usize>,
    /// The recording buffer while a recorder runs ([`Session::start_recording`]): top-level
    /// commands with the place they ran in, kept apart from the journal.
    pub recorder: Option<record::Recorder>,
    /// How many `execute` calls are running (0 between top-level commands).
    depth: u32,
    /// Macros the person allowed to run in this session: (document uid, the macro's content as
    /// canonical JSON, from [`macros::fingerprint`]).
    pub macro_approvals: std::collections::HashSet<(u64, String)>,
    /// The macro being debugged, between debugger actions.
    pub macro_debug: Option<macros::debug::Debugger>,
    /// Where the macro recorder started (beside `recorder`, its buffer).
    pub macro_recording: Option<macros::recorder::RecordingStart>,
    /// Windows hidden with View › Hide.
    pub hidden_windows: Vec<usize>,
    /// Draw tab tool: "" (select), "pen" or "eraser"; pen colour and width.
    pub draw_tool: String,
    pub draw_color: String,
    pub draw_width: f32,
    /// Review › Show Comments: the Comments pane is open.
    pub comments_pane: bool,
    /// Review › Notes › Show All Notes: every note is displayed. An application display setting
    /// (like Excel's comment indicator option), so it's not saved in the file.
    pub show_all_notes: bool,
    /// Open undo groups, innermost last.
    groups: Vec<OpenGroup>,
    next_group: u64,
    /// Counts group begins and ends, so `execute` can tell that a command opened or closed one.
    group_epoch: u64,
    /// The step the last group made: document, its undo length after the step, and the epoch.
    group_step: Option<(u64, usize, u64)>,
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
            let d = self.docs.remove(i);
            self.groups.retain(|g| g.doc != d.uid);
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

    /// Runs a command by id. Undoable commands record a history step when the workbook changed,
    /// unless an undo group is open on the document (the group then makes the step).
    /// Panics inside commands are caught and reported; the workbook is restored.
    pub fn execute(&mut self, id: &str, params: Json) -> Result<Json> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if let Err(why) = (spec.enabled)(self) {
            return Err(EngineError::Disabled(id.to_string(), why));
        }
        // While a macro is being debugged its undo group is open: other edits of the workbooks it
        // touched wait, also when another command runs them (a script, or a recording that stops
        // and saves). Only the run's own steps go ahead. Other workbooks stay editable.
        let locked = self.active().is_some_and(|d| self.macro_debug.as_ref().is_some_and(|m| m.locks(d.uid) && !m.stepping()));
        // Stopping Record Actions saves a script into the workbook; refuse it before it takes the
        // recording, so stopping again after the run keeps what was recorded.
        let stops_record_actions =
            id == "automate.recordActions" && (params.get("stop").and_then(Json::as_bool) == Some(true) || self.recording.is_some());
        if (spec.undoable || stops_record_actions) && id != "macro.debug" && locked {
            return Err(EngineError::Disabled(id.to_string(), macros::debug::BUSY.into()));
        }
        let before = self.active().map(|d| (d.uid, d.wb.clone(), d.selection.clone()));
        let params = if params.is_null() { Json::Object(Default::default()) } else { params };
        let (first_group, group_epoch) = (self.next_group, self.group_epoch);
        // Whether a group was open on the document before the command, and how long its history was.
        let start = before.as_ref().and_then(|(uid, ..)| self.docs.iter().find(|d| d.uid == *uid).map(|d| (self.in_group(*uid), d.undo.len())));
        // The recorder keeps top-level calls with the place they ran in.
        let place = (self.depth == 0 && self.recorder.is_some()).then(|| match self.active() {
            Some(d) => (d.uid, d.wb.active_sheet, d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(), d.selection.clone()),
            None => (0, 0, String::new(), Selection::default()),
        });
        self.depth = self.depth.saturating_add(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, &params)));
        self.depth = self.depth.saturating_sub(1);
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                // Restore the workbook as it was, and drop the undo groups the command opened.
                self.groups.retain(|g| g.id < first_group);
                if let Some((uid, wb, sel)) = &before
                    && let Some(d) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    d.wb = wb.clone();
                    d.selection = sel.clone();
                    d.calc.rebuild(&d.wb);
                }
                macros::debug::recover(self);
                if self.depth == 0 {
                    self.trim_history();
                }
                return Err(EngineError::Internal(id.to_string(), msg));
            }
        };
        if spec.journal && result.is_ok() {
            // A command that started recording is not part of the recording; one that stopped it
            // finds no recorder.
            if let Some((doc, sheet, sheet_name, selection)) = place
                && let Some(rec) = self.recorder.as_mut()
            {
                // A toggle (Bold) is recorded with the state it set, as Excel records
                // `Bold = True`: playing it back sets that state rather than flipping it again.
                let mut params = cmd::journal_params(&params);
                if let (Ok(Json::Object(r)), Json::Object(p)) = (&result, &mut params)
                    && !p.contains_key("on")
                    && let Some(on @ Json::Bool(_)) = r.get("on")
                {
                    p.insert("on".into(), on.clone());
                }
                rec.push(record::RecordedStep { doc, sheet, sheet_name, selection, id: id.to_string(), params });
            }
            self.journal.push((id.to_string(), cmd::journal_params(&params)));
            if self.journal.len() > 10_000 {
                self.journal.drain(..5_000);
            }
        }
        // Inside an undo group, or when the command ended one that was open before it, the group
        // owns the step. A command that opened and closed its own groups makes one step of its
        // own, which replaces the steps those groups made.
        let epoch_changed = self.group_epoch != group_epoch;
        let in_group_now = before.as_ref().is_some_and(|(uid, ..)| self.in_group(*uid));
        let own_groups = epoch_changed && !in_group_now && start.is_some_and(|(was_open, _)| !was_open);
        let grouped = !own_groups && (epoch_changed || in_group_now);
        if spec.undoable
            && let Some((uid, wb, sel)) = before
            && let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&d.wb, &wb)
        {
            if !grouped {
                let mut label = spec.label.trim_end_matches('…').to_string();
                if own_groups && let Some((_, len)) = start {
                    // A command whose only step is its group's keeps that name ("Run Macro: Totals").
                    if d.undo.len() == len + 1
                        && self.group_step.is_some_and(|(doc, n, epoch)| doc == uid && n == len + 1 && epoch != group_epoch)
                        && let Some(last) = d.undo.last()
                    {
                        label = last.label.clone();
                    }
                    d.undo.truncate(len);
                }
                d.undo.push(HistoryEntry { label, wb, selection: sel });
            }
            d.redo.clear();
            d.revision += 1;
        }
        // The history limit applies once the outermost command has collapsed its steps, so the
        // lengths it compared stay true.
        if self.depth == 0 {
            self.trim_history();
        }
        result
    }

    /// Runs `f` as one undo step labelled `label` on the active document: whatever commands `f`
    /// runs, one Undo brings back the workbook as it was before. If `f` fails, every change it made
    /// is rolled back and its error is returned. Groups nest; the outermost one makes the step.
    pub fn grouped<R>(&mut self, label: &str, f: impl FnOnce(&mut Session) -> Result<R>) -> Result<R> {
        let token = self.group_begin(label)?;
        match f(self) {
            Ok(r) => {
                self.group_end(token)?;
                Ok(r)
            }
            Err(e) => {
                // The error from `f` is the one to report. A rollback fails only when the group
                // has already ended, and then there is nothing left to undo.
                let _ = self.group_rollback(token);
                Err(e)
            }
        }
    }

    /// Opens an undo group on the active document that can span several calls (for example a
    /// debugger that runs one step per call). Until [`Session::group_end`] or
    /// [`Session::group_rollback`], undoable commands on that document add no undo steps of their
    /// own, and Undo and Redo are unavailable there.
    pub fn group_begin(&mut self, label: &str) -> Result<GroupToken> {
        let d = self.doc()?;
        let g = OpenGroup {
            id: self.next_group,
            doc: d.uid,
            label: label.trim_end_matches('…').to_string(),
            start: d.undo.len(),
            wb: d.wb.clone(),
            selection: d.selection.clone(),
            redo: d.redo.clone(),
        };
        let token = GroupToken { id: g.id, doc: g.doc };
        self.next_group = self.next_group.saturating_add(1);
        self.group_epoch = self.group_epoch.wrapping_add(1);
        self.groups.push(g);
        Ok(token)
    }

    /// Ends an undo group. If the workbook changed, the group becomes one undo step, unless it is
    /// nested in another open group on the same document, which then owns the step. Groups opened
    /// inside it and still open end with it. Returns whether the workbook changed.
    pub fn group_end(&mut self, token: GroupToken) -> Result<bool> {
        let g = self.take_group(&token)?;
        let outer = self.in_group(g.doc);
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == g.doc) else { return Ok(false) };
        // Steps pushed directly while the group was open fold into the group's step.
        d.undo.truncate(g.start);
        let changed = !Arc::ptr_eq(&d.wb, &g.wb);
        if changed && !outer {
            d.undo.push(HistoryEntry { label: g.label, wb: g.wb, selection: g.selection });
            self.group_step = Some((g.doc, d.undo.len(), self.group_epoch));
            d.redo.clear();
            d.revision += 1;
        }
        // Inside a command, the outermost command applies the history limit when it finishes.
        if self.depth == 0 {
            self.trim_history();
        }
        Ok(changed)
    }

    /// Drops the oldest undo steps past [`UNDO_LIMIT`]. A document with an open group waits for
    /// the group to end, because the group remembers where its steps start.
    fn trim_history(&mut self) {
        let groups = &self.groups;
        for d in self.docs.iter_mut().filter(|d| !groups.iter().any(|g| g.doc == d.uid)) {
            let excess = d.undo.len().saturating_sub(UNDO_LIMIT);
            if excess > 0 {
                d.undo.drain(..excess);
            }
        }
    }

    /// Ends an undo group by putting back the workbook, selection, undo and redo history as they
    /// were when it began.
    pub fn group_rollback(&mut self, token: GroupToken) -> Result<()> {
        let g = self.take_group(&token)?;
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == g.doc) else { return Ok(()) };
        if !Arc::ptr_eq(&d.wb, &g.wb) {
            d.wb = g.wb;
            d.calc.rebuild(&d.wb);
        }
        d.selection = g.selection;
        d.undo.truncate(g.start);
        d.redo = g.redo;
        d.revision += 1;
        Ok(())
    }

    /// Starts recording top-level commands into an empty buffer, replacing any recording in
    /// progress. Queries, failed commands and commands run by other commands are not recorded.
    pub fn start_recording(&mut self) {
        self.recorder = Some(record::Recorder::default());
    }

    /// Stops recording and returns what was recorded (`None` when nothing was recording).
    pub fn stop_recording(&mut self) -> Option<record::Recorder> {
        self.recorder.take()
    }

    /// Whether the recording buffer is collecting commands.
    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// How many commands are running: 0 between top-level commands, 1 inside one, 2 inside a
    /// command run by a command, and so on.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Whether an undo group is open on the document with this uid.
    pub fn in_group(&self, doc: u64) -> bool {
        self.groups.iter().any(|g| g.doc == doc)
    }

    /// Removes the token's group and any group on the same document opened after it.
    fn take_group(&mut self, token: &GroupToken) -> Result<OpenGroup> {
        let ended = || EngineError::Other("That undo group has already ended.".into());
        let at = self.groups.iter().position(|g| g.id == token.id).ok_or_else(ended)?;
        let mut later = self.groups.split_off(at).into_iter();
        let g = later.next().ok_or_else(ended)?;
        self.groups.extend(later.filter(|o| o.doc != g.doc));
        self.group_epoch = self.group_epoch.wrapping_add(1);
        Ok(g)
    }

    pub fn run(&mut self, id: &str, params: Json) -> std::result::Result<Json, String> {
        self.execute(id, params).map_err(|e| e.to_string())
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
