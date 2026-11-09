//! Which Python cells are due. After anything changed a workbook this brings the list of its Python
//! cells up to date from what changed (the copy-on-write cell store makes that cheap), finds the
//! first cell in run order that must run again, and marks it and every cell after it, since they
//! share variables.
//!
//! The cost of one edit is linear in the number of Python cells at worst, and a workbook without
//! any only pays for comparing the bands the edit touched. Only a reference the edit reaches is read
//! again, and an area several cells read is read once.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{Cell, LineInfo, PyCode, Sheet, StyleId, Workbook};
use gridcraft_python::Client;
use gridcraft_python::diagnostics::Diagnostic;

use super::dates::{laid_out, sync_formats};
use super::inputs::{Lit, Reader, Target, code_hash, is_volatile, literals, text_hash};
use super::runner::Inflight;
use super::{PythonCalculation, PythonRun, PythonStats, python_order_of_sheet};
use crate::{DocState, Trust};

/// How many changed positions a reference is compared with one by one; past it the box around them
/// decides. A false hit only costs reading the area again.
const EXACT_CHECK: usize = 1024;
/// How many times a result may send cells before it back to run, in one go. A cell that reads a
/// later cell settles in a pass or two; cells that feed each other never do, and are left stale.
const MAX_PASSES: u32 = 8;

/// Whether cells may start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Gate {
    Go,
    /// The check of Python hasn't answered yet: due cells wait, showing `#BUSY!`.
    Wait,
    /// Python can't run: due cells keep what they show, marked stale.
    No,
}

/// What a cell's run state is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CellState {
    /// Its result is up to date.
    Clean,
    /// It is due and doesn't run on its own (manual calculation, or Python can't run).
    Stale,
    /// It is waiting to run, or running.
    Queued,
}

/// What led to a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Edit,
    /// The result of the cell at this index was written.
    Result(usize),
}

/// What a plan needs to know besides the workbook.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cx {
    pub gate: Gate,
    pub manual: bool,
}

/// One Python cell of a workbook, in run order.
pub(super) struct Entry {
    pub sheet: usize,
    /// The sheet's identity ([`gridcraft_model::SheetUid`]), which its index doesn't keep.
    pub uid: u64,
    pub at: CellRef,
    pub text_hash: u64,
    pub code_hash: u64,
    pub lits: Vec<Lit>,
    pub volatile: bool,
    /// What the cell's code and inputs are now.
    pub fp: u64,
    /// What they were when it last ran here; `None` when that doesn't count any more.
    pub ran: Option<u64>,
    pub state: CellState,
    /// A later cell's result changed what this cell read, and the passes ran out.
    pub unsettled: bool,
    /// It waits to run again because of a later cell's result, and shows `#BUSY!` only when its turn
    /// comes. Were it to show that at once, a formula that reads it would give `#BUSY!` to the cell
    /// that runs first, and that cell would never see its result.
    pub quiet: bool,
    /// The current worker has run this cell's code, so its variables are there.
    pub in_worker: bool,
    /// The cell's flags as written: (busy, stale).
    pub shown: (bool, bool),
    pub diag: Option<Diagnostic>,
    /// The earlier cell whose figure this one changes, which has to run before it runs again.
    pub run_from: Option<(u64, CellRef)>,
    /// The place the worker knows this cell by, from the last time it ran there.
    pub placed: Option<[u32; 3]>,
}

impl Entry {
    fn new(wb: &Workbook, sheet: usize, uid: u64, at: CellRef, py: &PyCode) -> Entry {
        Entry {
            sheet,
            uid,
            at,
            text_hash: text_hash(&py.code, py.object),
            code_hash: code_hash(&py.code, py.object),
            lits: literals(wb, sheet, at, &py.code),
            volatile: is_volatile(&py.code),
            fp: 0,
            ran: None,
            state: CellState::Clean,
            unsettled: false,
            quiet: false,
            in_worker: false,
            shown: (py.busy, py.stale),
            diag: None,
            run_from: None,
            placed: None,
        }
    }

    /// Takes on the run state of the entry this one continues.
    fn inherit(&mut self, from: Entry) {
        self.ran = from.ran;
        self.state = from.state;
        self.unsettled = from.unsettled;
        self.quiet = from.quiet;
        self.in_worker = from.in_worker;
        self.diag = from.diag;
    }

    /// Notes what the cell holds after it changed. Returns whether its code did.
    fn note_cell(&mut self, wb: &Workbook, sh: &Sheet, cell: &Cell, py: &PyCode) -> bool {
        self.shown = (py.busy, py.stale);
        let text = text_hash(&py.code, py.object);
        let new_text = text != self.text_hash;
        if new_text {
            self.text_hash = text;
            self.lits = literals(wb, self.sheet, self.at, &py.code);
            self.volatile = is_volatile(&py.code);
            let code = code_hash(&py.code, py.object);
            if code != self.code_hash {
                self.code_hash = code;
                self.ran = None;
                self.diag = None;
            }
        }
        // New code keeps no result and shows #BLOCKED!: it runs, even where the code differs only
        // in what its references spell and they read the same values. A cell that says it waits, or
        // that it is out of date, comes from a copy of the workbook taken then (Undo brings one
        // back): what it keeps isn't what this cell last gave.
        let never_ran = py.output.is_none() && cell.value == Value::Error(CellError::Blocked);
        let copied_while_due = py.busy || (py.stale && !self.unsettled);
        if self.state == CellState::Clean && (never_ran || copied_while_due) {
            self.ran = None;
        }
        self.sync_spill(sh, cell, py);
        new_text
    }

    /// `#SPILL!` comes and goes with what is in the way, without the cell running.
    fn sync_spill(&mut self, sh: &Sheet, cell: &Cell, py: &PyCode) {
        let spill = cell.value == Value::Error(CellError::Spill) && py.output.as_ref().is_some_and(|o| matches!(o.value, Value::Array(_)));
        let says = self.diag.as_ref().is_some_and(|d| d.kind.id == "result.spill");
        if spill && !says {
            self.diag = Some(Diagnostic::new("result.spill", spill_detail(sh, self.at, py), None));
        } else if says && !spill {
            self.diag = None;
        }
    }
}

/// What is in a blocked result's way.
fn spill_detail(sh: &Sheet, at: CellRef, py: &PyCode) -> String {
    let Some(Value::Array(a)) = py.output.as_ref().map(|o| &o.value) else { return String::new() };
    let end = at.offset(i64::try_from(a.rows).unwrap_or(i64::MAX).saturating_sub(1), i64::try_from(a.cols).unwrap_or(i64::MAX).saturating_sub(1));
    let Some(end) = end else { return "The result would run past the edge of the sheet.".into() };
    let blocker = sh.cells.iter_range(RangeRef::new(at, end)).find(|(c, cell)| *c != at && cell.has_content()).map(|(c, _)| c);
    blocker.map_or_else(String::new, |c| format!("{} is in the way.", c.a1()))
}

/// What changed on one sheet.
pub(super) struct SheetChange {
    /// Positions whose cells differ, in order.
    pub cells: Vec<CellRef>,
    /// Positions whose spilled values differ.
    spills: Vec<CellRef>,
    /// The sheet is new here: all of it counts as changed.
    whole: bool,
    /// A row or column format changed under a spill, whose values have no cells to show it. What
    /// reads them reads a date or a number by that format.
    lines: bool,
    /// The box around `cells` and `spills`.
    bounds: Option<RangeRef>,
}

impl SheetChange {
    fn between(old: &Sheet, new: &Sheet) -> Option<SheetChange> {
        let mut cells = old.cells.diff(&new.cells);
        let spills = spill_changes(&old.spill, &new.spill);
        let lines = !new.spill.is_empty() && (line_styles(&old.rows).ne(line_styles(&new.rows)) || line_styles(&old.cols).ne(line_styles(&new.cols)));
        if cells.is_empty() && spills.is_empty() && !lines {
            return None;
        }
        cells.sort_unstable();
        let bounds = cells.iter().chain(&spills).fold(None, |b: Option<RangeRef>, c| {
            let one = RangeRef::cell(*c);
            Some(b.map_or(one, |x| x.union(&one)))
        });
        Some(SheetChange { cells, spills, whole: false, lines, bounds })
    }

    fn hits(&self, r: RangeRef) -> bool {
        if self.whole || self.lines {
            return true;
        }
        if !self.bounds.is_some_and(|b| b.intersects(&r)) {
            return false;
        }
        self.cells.len().saturating_add(self.spills.len()) > EXACT_CHECK || self.cells.iter().chain(&self.spills).any(|c| r.contains(*c))
    }

    fn has(&self, c: CellRef) -> bool {
        self.whole || self.cells.binary_search(&c).is_ok()
    }
}

/// The rows or columns that have a format of their own, with it.
fn line_styles(lines: &BTreeMap<u32, LineInfo>) -> impl Iterator<Item = (u32, StyleId)> {
    lines.iter().filter_map(|(line, info)| info.style.map(|style| (*line, style)))
}

/// The positions of spilled values that differ between two sheets. The maps are in order, so the
/// walk is linear.
fn spill_changes(a: &BTreeMap<CellRef, Value>, b: &BTreeMap<CellRef, Value>) -> Vec<CellRef> {
    if a == b {
        return Vec::new();
    }
    let mut out = Vec::new();
    let (mut x, mut y) = (a.iter().peekable(), b.iter().peekable());
    loop {
        match (x.peek(), y.peek()) {
            (None, None) => return out,
            (Some((c, _)), None) => {
                out.push(**c);
                x.next();
            }
            (None, Some((c, _))) => {
                out.push(**c);
                y.next();
            }
            (Some((c, v)), Some((e, w))) => match c.cmp(e) {
                std::cmp::Ordering::Less => {
                    out.push(**c);
                    x.next();
                }
                std::cmp::Ordering::Greater => {
                    out.push(**e);
                    y.next();
                }
                std::cmp::Ordering::Equal => {
                    if v != w {
                        out.push(**c);
                    }
                    x.next();
                    y.next();
                }
            },
        }
    }
}

/// What changed in a workbook.
pub(super) struct Diff {
    /// The same sheets in the same order.
    same_layout: bool,
    /// A sheet or the defined names changed in any way (a table can move without a cell changing).
    structure: bool,
    /// By the new workbook's sheet index.
    sheets: Vec<Option<SheetChange>>,
    /// Each new sheet's index in the old workbook.
    old_index: Vec<Option<usize>>,
}

impl Diff {
    pub fn between(old: &Workbook, new: &Workbook) -> Diff {
        let same_layout = old.sheets.len() == new.sheets.len() && old.sheets.iter().zip(&new.sheets).all(|(a, b)| a.uid.get() == b.uid.get());
        let by_uid: HashMap<u64, usize> =
            if same_layout { HashMap::new() } else { old.sheets.iter().enumerate().map(|(i, s)| (s.uid.get(), i)).collect() };
        let mut structure = !same_layout || old.names != new.names;
        let mut sheets = Vec::with_capacity(new.sheets.len());
        let mut old_index = Vec::with_capacity(new.sheets.len());
        for (i, sh) in new.sheets.iter().enumerate() {
            let at = if same_layout { Some(i) } else { by_uid.get(&sh.uid.get()).copied() };
            old_index.push(at);
            sheets.push(match at.and_then(|j| old.sheets.get(j)) {
                None => Some(SheetChange { cells: Vec::new(), spills: Vec::new(), whole: true, lines: false, bounds: None }),
                Some(o) if Arc::ptr_eq(o, sh) => None,
                Some(o) => {
                    structure = true;
                    SheetChange::between(o, sh)
                }
            });
        }
        Diff { same_layout, structure, sheets, old_index }
    }

    /// Whether any cell changed, or the sheets did.
    fn any(&self) -> bool {
        !self.same_layout || self.sheets.iter().any(Option::is_some)
    }

    fn sheet(&self, i: usize) -> Option<&SheetChange> {
        self.sheets.get(i).and_then(Option::as_ref)
    }

    fn hits(&self, sheet: usize, r: RangeRef) -> bool {
        self.sheet(sheet).is_some_and(|c| c.hits(r))
    }

    fn has(&self, sheet: usize, c: CellRef) -> bool {
        self.sheet(sheet).is_some_and(|ch| ch.has(c))
    }

    /// The changed cells of the sheet at this index, with the sheet as it was.
    pub fn changed_cells<'a>(&'a self, old: &'a Workbook, i: usize) -> (&'a [CellRef], Option<&'a Sheet>) {
        let cells = self.sheet(i).map_or(&[][..], |c| c.cells.as_slice());
        (cells, self.old_index.get(i).copied().flatten().and_then(|j| old.sheet(j)))
    }
}

fn python_of(cell: Option<&Cell>) -> Option<&PyCode> {
    cell.and_then(|c| c.py.as_deref())
}

/// The run state of one open workbook. It is created when first needed, and dropped with the
/// workbook, which stops its worker.
#[derive(Default)]
pub(super) struct DocRun {
    /// The person's choice for this workbook; without one, where the workbook came from decides.
    pub switch: Option<bool>,
    pub mode: PythonCalculation,
    /// The workbook as last planned.
    pub seen: Option<Arc<Workbook>>,
    /// `entries` reflect `seen`.
    pub tracked: bool,
    pub entries: Vec<Entry>,
    pub client: Option<Client>,
    pub inflight: Option<Inflight>,
    /// A figure being drawn again, with nothing else running.
    pub(super) render: Option<super::links::RenderFlight>,
    /// A figure isn't drawn at the size it is shown at.
    pub render_waiting: bool,
    /// Every cell before this one has run in the current worker, or never will.
    pub cursor: usize,
    /// How many cells are queued.
    pub due: usize,
    pub passes: u32,
    pub replays: u32,
    /// How many times in a row a worker didn't start.
    pub start_tries: u32,
    /// The current worker has been sent a cell, so there are variables to lose.
    pub worker_used: bool,
    /// Cells that stopped the worker, with what they were then and the error they show. They don't
    /// run again, and can't stop it again, until that changes or someone asks.
    pub refused: HashMap<(u64, CellRef), (u64, CellError)>,
    /// What the workbook's initialization code did.
    pub init: super::initialization::InitRun,
    /// Which cells assign which names, for the `python.nameOrder` diagnostic.
    pub assigns: Option<HashMap<String, Vec<u32>>>,
    pub log: VecDeque<PythonRun>,
    /// While the list of cells isn't kept ([`DocRun::tracked`] is false): the workbook holds Python
    /// code. Kept from what each change touched, so a workbook without any is never scanned twice.
    pub holds_code: bool,
    /// What the code imports that this Python doesn't have.
    pub packages: super::packages::PackageCheck,
}

impl DocRun {
    /// Whether Python is on for this workbook, given a capability.
    pub fn on(&self, trust: Trust) -> bool {
        self.switch.unwrap_or_else(|| trust.allows_code())
    }

    /// The index of the Python cell at this place.
    pub fn index_of(&self, sheet: usize, at: CellRef) -> Option<usize> {
        self.entries.binary_search_by_key(&(sheet, at), |e| (e.sheet, e.at)).ok()
    }

    /// Brings the run state up to date with the workbook and marks what is due. Returns whether it
    /// wrote to the workbook.
    pub fn plan(&mut self, d: &mut DocState, cx: Cx, source: Source, stats: &mut PythonStats) -> bool {
        let new = d.wb.clone();
        let Some(old) = self.seen.clone().filter(|_| self.tracked) else { return self.first_look(d, cx, stats) };
        if Arc::ptr_eq(&old, &new) {
            return false;
        }
        self.seen = Some(new.clone());
        let diff = Diff::between(&old, &new);
        let new_init = !super::initialization::same_run(&old, &new);
        let mut cx = cx;
        if new_init {
            self.start_over_for_initialization();
            cx.manual = super::manual(self, d);
        }
        if !diff.any() && !diff.structure && !new_init {
            return false;
        }
        // A spill a cell gets in the way of, or that one no longer blocks, takes its formats with it.
        // What reads those cells goes by the formats, so the look at what changed comes after.
        let formatted = sync_changed_formats(d, &old, &diff);
        let (new, diff) = if formatted {
            let new = d.wb.clone();
            self.seen = Some(new.clone());
            let diff = Diff::between(&old, &new);
            (new, diff)
        } else {
            (new, diff)
        };
        let (seq_first, touched) = {
            let mut reader = Reader::new(&new, stats);
            let (seq_first, moved) = self.absorb(&old, &new, &diff, &mut reader);
            let refreshed = self.refresh(&new, &diff, &mut reader);
            (seq_first, moved || refreshed)
        };
        if !touched && !new_init {
            return formatted;
        }
        self.derive(d, cx, source, seq_first) | formatted
    }

    /// Everything Python on this workbook is due: nothing has run here yet.
    fn first_look(&mut self, d: &mut DocState, cx: Cx, stats: &mut PythonStats) -> bool {
        stats.full_scans = stats.full_scans.saturating_add(1);
        let wb = d.wb.clone();
        self.entries = wb
            .sheets
            .iter()
            .enumerate()
            .flat_map(|(i, sh)| python_order_of_sheet(sh).into_iter().map(move |c| (i, sh, c)))
            .filter_map(|(i, sh, c)| python_of(sh.cell(c)).map(|py| Entry::new(&wb, i, sh.uid.get(), c, py)))
            .collect();
        self.tracked = true;
        self.assigns = None;
        self.cursor = 0;
        self.due = 0;
        self.seen = Some(wb);
        self.derive(d, cx, Source::Edit, None)
    }

    /// Updates the list of cells for what changed. Returns the first index whose place in the run
    /// order isn't the one it had, and whether any cell's code or place changed.
    fn absorb(&mut self, old: &Workbook, new: &Workbook, diff: &Diff, reader: &mut Reader) -> (Option<usize>, bool) {
        if !diff.same_layout || self.set_changes(old, new, diff) {
            self.assigns = None;
            return (self.rebuild(new, diff, reader), true);
        }
        let mut touched = false;
        for (i, sh) in new.sheets.iter().enumerate() {
            let (cells, _) = diff.changed_cells(old, i);
            for c in cells {
                let (Some(cell), Some(at)) = (sh.cell(*c), self.index_of(i, *c)) else { continue };
                let (Some(py), Some(e)) = (cell.py.as_deref(), self.entries.get_mut(at)) else { continue };
                if e.note_cell(new, sh, cell, py) {
                    e.fp = reader.fingerprint(e.code_hash, &e.lits);
                    touched = true;
                    self.assigns = None;
                }
                touched |= e.state == CellState::Clean && e.ran != Some(e.fp);
            }
        }
        (None, touched)
    }

    /// Whether a change added or removed a Python cell.
    fn set_changes(&self, old: &Workbook, new: &Workbook, diff: &Diff) -> bool {
        new.sheets.iter().enumerate().any(|(i, sh)| {
            let (cells, before) = diff.changed_cells(old, i);
            let whole = diff.sheet(i).is_some_and(|c| c.whole);
            whole || cells.iter().any(|c| python_of(sh.cell(*c)).is_some() != before.is_some_and(|b| python_of(b.cell(*c)).is_some()))
        })
    }

    /// The Python cells of the new workbook, in run order.
    fn next_keys(&self, new: &Workbook, diff: &Diff) -> Vec<(usize, CellRef)> {
        let mut by_uid: HashMap<u64, BTreeSet<CellRef>> = HashMap::new();
        for e in &self.entries {
            by_uid.entry(e.uid).or_default().insert(e.at);
        }
        let mut out = Vec::with_capacity(self.entries.len());
        for (i, sh) in new.sheets.iter().enumerate() {
            let mut set = by_uid.remove(&sh.uid.get()).unwrap_or_default();
            match diff.sheet(i) {
                Some(ch) if ch.whole => set = python_order_of_sheet(sh).into_iter().collect(),
                Some(ch) => {
                    for c in &ch.cells {
                        if python_of(sh.cell(*c)).is_some() {
                            set.insert(*c);
                        } else {
                            set.remove(c);
                        }
                    }
                }
                None => {}
            }
            out.extend(set.into_iter().map(|c| (i, c)));
        }
        out
    }

    /// Rebuilds the list. A cell keeps its run state when it is the one it was, and when it stands
    /// where the same code and inputs stood in the run order: moving cells around doesn't make them
    /// run. Returns the first index where the order is new.
    fn rebuild(&mut self, new: &Workbook, diff: &Diff, reader: &mut Reader) -> Option<usize> {
        let keys = self.next_keys(new, diff);
        let mut old: Vec<Option<Entry>> = std::mem::take(&mut self.entries).into_iter().map(Some).collect();
        let by_key: HashMap<(u64, CellRef), usize> = old.iter().enumerate().filter_map(|(j, e)| e.as_ref().map(|e| ((e.uid, e.at), j))).collect();
        let mut seq_first = None;
        let mut out = Vec::with_capacity(keys.len());
        for (sheet, at) in keys {
            let i = out.len();
            let Some(sh) = new.sheet(sheet) else { continue };
            let Some(cell) = sh.cell(at) else { continue };
            let Some(py) = cell.py.as_deref() else { continue };
            let known = by_key.get(&(sh.uid.get(), at)).copied();
            let place = Place { new, sheet, sh, at, py, known, changed: diff.has(sheet, at), index: i, aligned: seq_first.is_none() };
            let (mut e, origin) = place.entry(&mut old, reader);
            if !diff.same_layout {
                e.lits = literals(new, sheet, at, &py.code);
            }
            if place.changed {
                e.note_cell(new, sh, cell, py);
            }
            if origin != Some(i) {
                seq_first.get_or_insert(i);
            }
            out.push(e);
        }
        self.entries = out;
        seq_first
    }

    /// Reads again what the change reached, for the cells whose fingerprint counts now. Returns
    /// whether one changed.
    fn refresh(&mut self, new: &Workbook, diff: &Diff, reader: &mut Reader) -> bool {
        let running = self.inflight.as_ref().map(|f| f.index);
        let mut any = false;
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.state == CellState::Queued && running != Some(i) {
                continue;
            }
            let volatile = e.volatile && diff.structure;
            if !volatile && !e.lits.iter().any(|l| matches!(l.target, Target::Area { sheet, range } if diff.hits(sheet, range))) {
                continue;
            }
            if volatile && let Some(py) = python_of(new.sheet(e.sheet).and_then(|s| s.cell(e.at))) {
                e.lits = literals(new, e.sheet, e.at, &py.code);
            }
            let fp = reader.fingerprint(e.code_hash, &e.lits);
            any |= fp != e.fp;
            e.fp = fp;
        }
        any
    }

    /// Marks what is due now and writes the flags. Returns whether it wrote to the workbook.
    pub fn derive(&mut self, d: &mut DocState, cx: Cx, source: Source, seq_first: Option<usize>) -> bool {
        if cx.gate == Gate::No {
            self.demote_queued();
        }
        let mut first = self.first_due(seq_first);
        let mut quiet = false;
        if let (Source::Result(k), Some(f), false) = (source, first, cx.manual)
            && f <= k
        {
            first = self.pass_or_settle(f, k);
            quiet = first.is_some_and(|f| f <= k);
        }
        for e in &mut self.entries {
            if e.state == CellState::Stale && e.ran == Some(e.fp) {
                e.state = CellState::Clean;
            }
        }
        if let Some(f) = first {
            self.mark_due(f, cx, quiet);
        }
        self.recount();
        self.sync_flags(d)
    }

    /// The first cell that isn't queued and whose code or inputs aren't what it ran with, or whose
    /// place is new, or that runs now on inputs that have changed since.
    fn first_due(&self, seq_first: Option<usize>) -> Option<usize> {
        let by_fingerprint = self.entries.iter().position(|e| e.state != CellState::Queued && e.ran != Some(e.fp));
        let running = self.inflight.as_ref().filter(|f| !f.discard && self.entries.get(f.index).is_none_or(|e| e.fp != f.fp)).map(|f| f.index);
        [seq_first, by_fingerprint, running].into_iter().flatten().min()
    }

    /// A result changed what the cell at `first`, at or before the one that gave it (`done`), reads.
    /// It runs again, up to the limit; past that, those cells stay as they are, marked stale, and the
    /// cells after go on. Returns the first cell due then.
    fn pass_or_settle(&mut self, first: usize, done: usize) -> Option<usize> {
        self.passes = self.passes.saturating_add(1);
        if self.passes <= MAX_PASSES {
            return Some(first);
        }
        for e in self.entries.iter_mut().take(done.saturating_add(1)).skip(first) {
            if e.state != CellState::Queued {
                e.ran = Some(e.fp);
                e.unsettled = true;
                e.state = CellState::Clean;
            }
        }
        self.first_due(None).filter(|f| *f > done)
    }

    /// The first cell to run so that every cell from `first` on starts clean: a cell that changes an
    /// earlier cell's figure needs that cell to run first.
    fn with_figures(&self, first: usize) -> usize {
        let mut first = first;
        let mut from = first;
        // Each pass can only move `first` back, so this ends. Earlier cells run first anyway.
        while let Some(e) = self.entries.get(from) {
            if let Some((uid, at)) = e.run_from
                && let Some(owner) = self.entries.iter().position(|o| o.uid == uid && o.at == at)
                && owner < first
            {
                first = owner;
                from = owner;
                continue;
            }
            from += 1;
        }
        first
    }

    fn mark_due(&mut self, first: usize, cx: Cx, quiet: bool) {
        let first = self.with_figures(first);
        let target = if cx.manual || cx.gate == Gate::No { CellState::Stale } else { CellState::Queued };
        for e in self.entries.iter_mut().skip(first) {
            // A cell someone asked to run stays queued under manual calculation.
            if e.state != CellState::Queued {
                e.state = target;
                e.unsettled = false;
                e.quiet = quiet && target == CellState::Queued;
            }
        }
        self.cursor = self.cursor.min(first);
        if let Some(f) = self.inflight.as_mut().filter(|f| f.index >= first) {
            f.discard = true;
        }
    }

    /// Takes queued cells back: they are due, and don't run for now.
    pub fn demote_queued(&mut self) {
        let running = self.inflight.as_ref().filter(|f| !f.discard).map(|f| f.index);
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.state == CellState::Queued && running != Some(i) {
                e.state = CellState::Stale;
                e.ran = None;
                e.quiet = false;
            }
        }
        self.recount();
    }

    pub fn recount(&mut self) {
        self.due = self.entries.iter().filter(|e| e.state == CellState::Queued).count();
    }

    /// Writes the flags that differ from what the cells' states say.
    pub fn sync_flags(&mut self, d: &mut DocState) -> bool {
        let writes: Vec<(usize, CellRef, (bool, bool))> = self
            .entries
            .iter_mut()
            .filter_map(|e| {
                let want = (e.state == CellState::Queued && !e.quiet, e.state == CellState::Stale || e.unsettled);
                (e.shown != want).then(|| {
                    e.shown = want;
                    (e.sheet, e.at, want)
                })
            })
            .collect();
        if writes.is_empty() {
            return false;
        }
        write_flags(d, &writes);
        self.seen = Some(d.wb.clone());
        true
    }

    /// Stops everything for this workbook, leaving its cells with what they keep. Returns whether
    /// it wrote to the workbook.
    pub fn turn_off(&mut self, d: &mut DocState) -> bool {
        self.client = None;
        self.inflight = None;
        self.init = Default::default();
        self.due = 0;
        self.cursor = 0;
        self.passes = 0;
        self.replays = 0;
        self.assigns = None;
        self.refused.clear();
        let writes: Vec<(usize, CellRef, (bool, bool))> =
            self.entries.iter().filter(|e| e.shown != (false, false)).map(|e| (e.sheet, e.at, (false, false))).collect();
        self.entries.clear();
        self.tracked = false;
        if !writes.is_empty() {
            write_flags(d, &writes);
        }
        self.seen = Some(d.wb.clone());
        self.holds_code = holds_code(&d.wb);
        !writes.is_empty()
    }

    /// Forgets the worker and the cells, as if the workbook had just been opened here, and keeps what
    /// the person chose. The next plan starts over.
    pub fn forget_cells(&mut self) {
        let (switch, mode, log) = (self.switch, self.mode, std::mem::take(&mut self.log));
        *self = DocRun { switch, mode, log, ..DocRun::default() };
    }

    /// With Python off here, only clears what can arrive from elsewhere: a cell that was waiting,
    /// brought back by Undo or a paste. No worker runs and nothing is scanned.
    pub fn stay_off(&mut self, d: &mut DocState) -> bool {
        if self.tracked || self.client.is_some() {
            return self.turn_off(d);
        }
        let new = d.wb.clone();
        let Some(old) = self.seen.replace(new.clone()) else {
            self.holds_code = holds_code(&new);
            return false;
        };
        if Arc::ptr_eq(&old, &new) {
            return false;
        }
        let diff = Diff::between(&old, &new);
        self.note_code(&old, &new, &diff);
        let mut writes = Vec::new();
        for (i, sh) in new.sheets.iter().enumerate() {
            let (cells, _) = diff.changed_cells(&old, i);
            writes.extend(cells.iter().filter(|c| python_of(sh.cell(**c)).is_some_and(|p| p.busy)).map(|c| (i, *c, (false, false))));
        }
        if writes.is_empty() {
            return false;
        }
        write_flags(d, &writes);
        self.seen = Some(d.wb.clone());
        true
    }
}

impl DocRun {
    /// Brings [`DocRun::holds_code`] up to date from what changed. A cell that gained code settles
    /// it; only a cell that lost its code, or a change too wide to list, looks through the workbook,
    /// and that stops at the first Python cell.
    fn note_code(&mut self, old: &Workbook, new: &Workbook, diff: &Diff) {
        let mut look = !diff.same_layout;
        let mut gained = false;
        for (i, sh) in new.sheets.iter().enumerate() {
            let Some(change) = diff.sheet(i) else { continue };
            look |= change.whole;
            let (cells, before) = diff.changed_cells(old, i);
            for c in cells {
                if python_of(sh.cell(*c)).is_some() {
                    gained = true;
                } else if before.is_some_and(|o| python_of(o.cell(*c)).is_some()) {
                    look = true;
                }
            }
        }
        if look {
            self.holds_code = holds_code(new);
        } else if gained {
            self.holds_code = true;
        }
    }
}

/// Brings the formats of the Python cells whose results came onto the sheet or left it in line with
/// them. A format the person changed on a cell doesn't count: that is theirs. Returns whether it
/// wrote to the workbook.
fn sync_changed_formats(d: &mut DocState, old: &Workbook, diff: &Diff) -> bool {
    let shown = |sh: &Sheet, c: CellRef| sh.cell(c).zip(python_of(sh.cell(c))).is_some_and(|(cell, py)| laid_out(sh, c, cell, py));
    let new = d.wb.clone();
    let mut wrote = false;
    for (i, sh) in new.sheets.iter().enumerate() {
        let (cells, before) = diff.changed_cells(old, i);
        for c in cells {
            if python_of(sh.cell(*c)).is_some_and(|py| !py.dates.is_empty()) && shown(sh, *c) != before.is_some_and(|b| shown(b, *c)) {
                wrote |= sync_formats(d, i, *c);
            }
        }
    }
    wrote
}

/// Whether any cell of the workbook holds Python code.
pub(super) fn holds_code(wb: &Workbook) -> bool {
    wb.sheets.iter().any(|sh| sh.cells.iter().any(|(_, c)| c.py.is_some()))
}

/// One cell's place in a rebuilt list.
struct Place<'a> {
    new: &'a Workbook,
    sheet: usize,
    sh: &'a Sheet,
    at: CellRef,
    py: &'a PyCode,
    /// Its index in the old list, when a Python cell stood at this address.
    known: Option<usize>,
    /// What stands at this address changed.
    changed: bool,
    index: usize,
    /// The list is the same as the old one up to here.
    aligned: bool,
}

impl Place<'_> {
    /// The entry for this place, and the index in the old list it continues.
    fn entry(&self, old: &mut [Option<Entry>], reader: &mut Reader) -> (Entry, Option<usize>) {
        let text = text_hash(&self.py.code, self.py.object);
        if let Some(j) = self.known {
            let same = !self.changed || (j == self.index && old.get(j).and_then(Option::as_ref).is_some_and(|e| e.text_hash == text));
            if same && let Some(mut e) = old.get_mut(j).and_then(Option::take) {
                e.sheet = self.sheet;
                return (e, Some(j));
            }
        }
        let mut fresh = Entry::new(self.new, self.sheet, self.sh.uid.get(), self.at, self.py);
        if self.aligned {
            fresh.fp = reader.fingerprint(fresh.code_hash, &fresh.lits);
            if let Some(o) = old.get_mut(self.index).and_then(|o| o.take_if(|o| o.code_hash == fresh.code_hash && o.fp == fresh.fp)) {
                fresh.inherit(o);
                return (fresh, Some(self.index));
            }
        }
        if let Some(j) = self.known
            && let Some(o) = old.get_mut(j).and_then(|o| o.take_if(|o| o.code_hash == fresh.code_hash))
        {
            fresh.inherit(o);
            return (fresh, Some(j));
        }
        (fresh, None)
    }
}

/// Writes a cell's flags: (busy, stale). A cell without a kept result shows `#BUSY!` while it is
/// busy, and when that ends without one, the error of its last run, or `#BLOCKED!` if it hasn't run.
/// No undo step, and what reads the cells is recalculated.
pub(super) fn write_flags(d: &mut DocState, writes: &[(usize, CellRef, (bool, bool))]) {
    let done = crate::cmd::commit(d, |cx| {
        for (sheet, at, (busy, stale)) in writes {
            let Some(cell) = cx.sheet_mut(*sheet)?.cells.get_mut(*at) else { continue };
            let Some(py) = cell.py.as_mut() else { continue };
            let py = Arc::make_mut(py);
            py.busy = *busy;
            py.stale = *stale;
            if py.output.is_none() {
                if *busy {
                    cell.value = Value::Error(CellError::Busy);
                } else if cell.value == Value::Error(CellError::Busy) {
                    cell.value = Value::Error(py.failed.unwrap_or(CellError::Blocked));
                }
            }
            cx.touch(*sheet, *at);
        }
        Ok(())
    });
    if let Err(e) = done {
        log::warn!("Python cells: {e}");
    }
    d.revision = d.revision.saturating_add(1);
}
