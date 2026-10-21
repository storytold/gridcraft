//! Running a macro.
//!
//! 1. **Gate**: every step's command goes through [`macro_policy`]. Denied commands stop the macro
//!    before anything runs; commands that touch files must be listed in the macro's permissions;
//!    a macro from a file someone else made, or one that touches files, runs only once the person
//!    approves it (once per macro content, for the session).
//! 2. **Inputs**: each declared input is bound to a range (given, its default, or the selection)
//!    and checked against its shape and column types, with messages that name the cells.
//! 3. **Footprint**: unless the macro may overwrite, it first runs on the real workbook in dry-run
//!    mode, the changes are compared with `CellStore::diff`, and everything is put back. Cells it
//!    would write outside its inputs that already hold data are conflicts, handled by the output
//!    rule: ask, fail, overwrite, or shift the data down or right (and check again). A dry run
//!    that stops early (a file step, Solver) proves nothing about later steps, so the run then
//!    checks each step as it goes.
//! 4. **Run**: one undo step (`Session::grouped`); an error at any step rolls everything back and
//!    names the step.
//!
//! The steps run on a small machine with an explicit frame stack, so the debugger can stop
//! between any two steps and pick up again on a later frame.

use std::collections::BTreeSet;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Cell, CompareOp, InputKind, Macro, OnConflict, RepeatOf, Sheet, Step, StepKind, Workbook};
use serde::Serialize;
use serde_json::{Value as Json, json};

use super::policy::{Policy, macro_policy, previewable};
use super::resolve::{self, Env, Place, ResolveError};
use crate::{EngineError, Result, Selection, Session, Trust, UiRequest};

/// The most steps one run executes (counting each pass through a Repeat).
pub const MAX_EXECUTED_STEPS: u64 = 100_000;
/// How long a run may take, in milliseconds. The web build has no clock; there only the step
/// limit applies.
pub const DEFAULT_TIME_LIMIT_MS: f64 = 60_000.0;
/// The most cells an input may have for its content to be checked.
pub const MAX_INPUT_CELLS: u64 = 1_000_000;
/// The most ranges a footprint lists per sheet (the counts stay exact).
const MAX_LISTED_RANGES: usize = 500;

/// An input bound to a range: name, where, whether its first row is a header.
pub type Bound = (String, Place, bool);

/// Where and why a step failed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StepError {
    /// The step's path: `3`, `2.1` inside a Repeat, `4.then.1` inside an If.
    pub step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub message: String,
    /// The step ran past the edge of the sheet.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub edge: bool,
    /// The step would write over data and the conflict rule is ask: overwriting answers it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
}

impl StepError {
    fn new(step: &str, command: Option<&str>, message: impl Into<String>) -> StepError {
        StepError { step: step.to_string(), command: command.map(str::to_string), message: message.into(), edge: false, confirm: false }
    }

    fn resolve(step: &str, command: Option<&str>, e: ResolveError) -> StepError {
        let edge = matches!(e, ResolveError::Edge(_));
        StepError { edge, ..StepError::new(step, command, e.message()) }
    }

    /// `Step 3 (cell.set): …`
    pub fn describe(&self) -> String {
        match &self.command {
            Some(c) => format!("Step {} ({c}): {}", self.step, self.message),
            None => format!("Step {}: {}", self.step, self.message),
        }
    }

    pub fn into_engine(self, name: &str) -> EngineError {
        let text = format!("Macro '{name}' stopped. {} Nothing was changed.", self.describe());
        if self.confirm { EngineError::Confirm(text) } else { EngineError::Other(text) }
    }
}

// ---------------------------------------------------------------- gate

/// Checks every enabled step against the policy: denied commands and undeclared file access are
/// errors; otherwise returns what the person must approve (empty when nothing).
pub fn gate(m: &Macro, trust: Trust) -> Result<Vec<String>> {
    let mut asks = BTreeSet::new();
    let mut err = None;
    walk_enabled(&m.steps, "", &mut |path, id, params| {
        if err.is_some() {
            return;
        }
        match macro_policy(id, params, trust) {
            Policy::Allow => {}
            Policy::Ask(why) => {
                asks.insert(why);
            }
            Policy::NeedsPermission(what) => {
                if m.permissions.iter().any(|p| p == id) {
                    asks.insert(format!("runs {id}, which {what}"));
                } else {
                    err = Some(format!("Step {path} ({id}) {what}. Add {id} to the macro's permissions to allow it."));
                }
            }
            Policy::Deny(why) => err = Some(format!("Step {path} ({id}) can't run in a macro: {why}.")),
        }
    });
    match err {
        Some(e) => Err(EngineError::Other(format!("Macro '{}' can't run. {e}", m.name))),
        None => Ok(asks.into_iter().collect()),
    }
}

/// Calls `f(path, id, params)` for each enabled command step (skipping disabled steps' bodies).
fn walk_enabled(steps: &[Step], prefix: &str, f: &mut impl FnMut(&str, &str, &Json)) {
    for (i, st) in steps.iter().enumerate() {
        if !st.enabled {
            continue;
        }
        let path = child_path(prefix, i);
        match &st.kind {
            StepKind::Command { id, params } => f(&path, id, params),
            StepKind::Repeat { body, .. } => walk_enabled(body, &path, f),
            StepKind::If { then, otherwise, .. } => {
                walk_enabled(then, &format!("{path}.then"), f);
                walk_enabled(otherwise, &format!("{path}.else"), f);
            }
            StepKind::Stop { .. } => {}
        }
    }
}

fn child_path(prefix: &str, i: usize) -> String {
    if prefix.is_empty() { (i + 1).to_string() } else { format!("{prefix}.{}", i + 1) }
}

/// Approves the macro for this session when `approve` is set, or asks (an
/// [`EngineError::Approve`]) when it needs approval it doesn't have.
pub fn approve(s: &mut Session, m: &Macro, doc: u64, asks: &[String], approve: bool) -> Result<()> {
    if asks.is_empty() {
        return Ok(());
    }
    let key = (doc, super::fingerprint(m));
    if s.macro_approvals.contains(&key) {
        return Ok(());
    }
    if approve {
        s.macro_approvals.insert(key);
        return Ok(());
    }
    Err(EngineError::Approve(format!("Run macro '{}'? It {}.", m.name, join_and(asks))))
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

// ---------------------------------------------------------------- inputs

/// A macro ready to run: its inputs bound and checked, its output anchor resolved.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub mac: Macro,
    pub doc: u64,
    pub trust: Trust,
    pub inputs: Vec<Bound>,
    pub output: Option<Place>,
}

impl Prepared {
    pub fn inputs_json(&self, wb: &Workbook) -> Json {
        let env = Env { wb, active_sheet: wb.active_sheet, inputs: &[], output: None, item: None };
        Json::Object(self.inputs.iter().map(|(n, p, _)| (n.clone(), json!(resolve::a1(*p, &env)))).collect())
    }
}

/// Binds the macro's inputs on the active document (`given`: `{"Input": "B2:D9"}`, or a reference
/// for the first input) and checks them. Errors list every problem found.
pub fn prepare(s: &Session, m: &Macro, given: Option<&Json>) -> Result<Prepared> {
    let d = s.doc()?;
    let wb = &d.wb;
    let mut inputs: Vec<Bound> = Vec::new();
    let mut problems = Vec::new();
    for (i, input) in m.inputs.iter().enumerate() {
        let named = given.and_then(Json::as_object).and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(&input.name)).map(|(_, v)| v));
        let text = named.and_then(Json::as_str).or_else(|| if i == 0 { given.and_then(Json::as_str) } else { None }).or(input.default_ref.as_deref());
        let place = match text {
            Some(t) => {
                let env = Env { wb, active_sheet: wb.active_sheet, inputs: &inputs, output: None, item: None };
                match resolve::resolve_ref(t, &env) {
                    Ok(p) => p,
                    Err(e) => {
                        problems.push(format!("{}: {}", input.name, e.message()));
                        continue;
                    }
                }
            }
            None if i == 0 => Place { sheet: wb.active_sheet, range: d.selection.current() },
            None => {
                problems.push(format!("{0} needs a range: pass inputs: {{\"{0}\": \"A1:B5\"}}.", input.name));
                continue;
            }
        };
        let place = trim(wb, place);
        problems.extend(check_input(wb, input, place));
        inputs.push((input.name.clone(), place, input.header));
    }
    if !problems.is_empty() {
        return Err(EngineError::Other(format!("Macro '{}' can't run on these inputs. {}", m.name, problems.join(" "))));
    }
    let output = match &m.output.anchor {
        Some(a) if !a.trim().is_empty() => {
            let env = Env { wb, active_sheet: wb.active_sheet, inputs: &inputs, output: None, item: None };
            Some(
                resolve::resolve_ref(a, &env)
                    .map_err(|e| EngineError::Other(format!("Macro '{}': its output anchor {}", m.name, lower_first(e.message()))))?,
            )
        }
        _ => None,
    };
    Ok(Prepared { mac: m.clone(), doc: d.uid, trust: d.trust, inputs, output })
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Whole rows or columns shrink to the part of the sheet in use.
fn trim(wb: &Workbook, p: Place) -> Place {
    if !(p.range.is_full_cols() || p.range.is_full_rows()) {
        return p;
    }
    let used = wb.sheet(p.sheet).and_then(Sheet::used_range);
    let range = used.and_then(|u| p.range.intersection(&u)).unwrap_or(RangeRef::cell(p.range.start));
    Place { sheet: p.sheet, range }
}

/// What is wrong with `place` as `input`, one sentence each.
fn check_input(wb: &Workbook, input: &gridcraft_model::NamedInput, place: Place) -> Vec<String> {
    let mut out = Vec::new();
    let r = place.range;
    let name = &input.name;
    let at = r.a1();
    if input.kind == InputKind::Cell && !r.is_single() {
        out.push(format!("{name} must be a single cell; {at} is {} cells.", r.count()));
        return out;
    }
    let header = u32::from(input.header && r.height() > 0);
    let rows = r.height().saturating_sub(header);
    let between = |what: &str, n: u32, [min, max]: [u32; 2]| -> Option<String> {
        let plural = |k: u32| if k == 1 { what.to_string() } else { format!("{what}s") };
        if max > 0 && min == max && n != min {
            Some(format!("exactly {min} {}", plural(min)))
        } else if n < min {
            Some(format!("at least {min} {}", plural(min)))
        } else if max > 0 && n > max {
            Some(format!("at most {max} {}", plural(max)))
        } else {
            None
        }
    };
    if let Some(want) = between("data row", rows, input.rows) {
        out.push(format!("{name} needs {want}; {at} has {rows}."));
    }
    if let Some(want) = between("column", r.width(), input.cols) {
        out.push(format!("{name} needs {want}; {at} has {}.", r.width()));
    }
    if r.count() > MAX_INPUT_CELLS {
        out.push(format!("{name} is too large to check ({} cells; at most {}).", r.count(), MAX_INPUT_CELLS));
        return out;
    }
    let Some(sh) = wb.sheet(place.sheet) else { return out };
    let mut blanks = Vec::new();
    for (ci, col) in (r.start.col..=r.end.col).enumerate() {
        let ty = input.col_types.get(ci).copied().unwrap_or_default();
        let mut wrong = Vec::new();
        for row in r.start.row + header..=r.end.row {
            let c = CellRef::new(row, col);
            let v = sh.value(c);
            if v.is_empty() || v.as_text() == Some("") {
                if input.non_empty {
                    blanks.push(c);
                }
                continue;
            }
            let date = || wb.styles.get(sh.style_id(c)).num_fmt.as_str().to_string();
            let ok = match ty {
                gridcraft_model::ColType::Any => true,
                gridcraft_model::ColType::Number => v.is_number(),
                gridcraft_model::ColType::Text => v.is_text(),
                gridcraft_model::ColType::Bool => matches!(v, Value::Bool(_)),
                gridcraft_model::ColType::Date => v.is_number() && crate::display::number_format(&date()).is_date(),
            };
            if !ok {
                wrong.push((c, holds(&v, ty)));
            }
        }
        if !wrong.is_empty() {
            let head = if input.header {
                Some(crate::display::cell_text(wb, sh, CellRef::new(r.start.row, col))).filter(|h| !h.trim().is_empty())
            } else {
                None
            };
            let column = match head {
                Some(h) => format!("{name} column {} ({})", ci + 1, h.trim()),
                None => format!("{name} column {}", ci + 1),
            };
            let listed: Vec<String> = wrong.iter().take(3).map(|(c, what)| format!("{} holds {what}", c.a1())).collect();
            let more = if wrong.len() > 3 { format!(" and {} more cells don't", wrong.len() - 3) } else { String::new() };
            out.push(format!("{column} must hold {}; {}{more}.", ty.noun(), listed.join(", ")));
        }
    }
    if let Some(first) = blanks.first() {
        let more = if blanks.len() > 1 { format!(" (and {} more)", blanks.len() - 1) } else { String::new() };
        out.push(format!("{name} must have no blank cells; {} is blank{more}.", first.a1()));
    }
    out
}

/// What a cell holds, for "C7 holds text".
fn holds(v: &Value, wanted: gridcraft_model::ColType) -> &'static str {
    match v {
        Value::Number(_) if wanted == gridcraft_model::ColType::Date => "a number that isn't a date",
        Value::Number(_) => "a number",
        Value::Text(_) => "text",
        Value::Bool(_) => "TRUE or FALSE",
        Value::Error(_) => "an error",
        Value::Array(_) => "an array",
        Value::Empty => "nothing",
    }
}

// ---------------------------------------------------------------- the machine

/// A stopwatch for the time limit. `Instant::now` traps on wasm, so there it measures nothing.
#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl Stopwatch {
    pub fn start() -> Stopwatch {
        #[cfg(not(target_arch = "wasm32"))]
        #[allow(clippy::disallowed_methods)] // never on wasm (see above)
        let w = Stopwatch { start: std::time::Instant::now() };
        #[cfg(target_arch = "wasm32")]
        let w = Stopwatch {};
        w
    }

    /// Milliseconds since the start, or `None` where there is no clock.
    fn elapsed_ms(&self) -> Option<f64> {
        #[cfg(not(target_arch = "wasm32"))]
        let ms = Some(self.start.elapsed().as_secs_f64() * 1000.0);
        #[cfg(target_arch = "wasm32")]
        let ms = None;
        ms
    }
}

#[derive(Clone, Debug)]
struct Frame {
    steps: Arc<Vec<Step>>,
    next: usize,
    /// Path of the step that opened this frame (`""` at the top, `3` for a Repeat's body,
    /// `4.then` for an If's branch).
    prefix: String,
    repeat: Option<Loop>,
}

#[derive(Clone, Copy, Debug)]
struct Loop {
    of: RepeatOf,
    over: Place,
    /// The current item (0-based) and how many there are.
    at: u32,
    count: u32,
}

impl Loop {
    fn item(&self) -> Place {
        let r = self.over.range;
        let range = match self.of {
            RepeatOf::Rows => {
                let row = r.start.row.saturating_add(self.at);
                RangeRef::new(CellRef::new(row, r.start.col), CellRef::new(row, r.end.col))
            }
            RepeatOf::Columns => {
                let col = r.start.col.saturating_add(self.at);
                RangeRef::new(CellRef::new(r.start.row, col), CellRef::new(r.end.row, col))
            }
        };
        Place { sheet: self.over.sheet, range }
    }
}

/// A macro run in progress: which step is next, and what has run.
#[derive(Clone, Debug)]
pub struct Machine {
    pub name: String,
    pub doc: u64,
    pub inputs: Vec<Bound>,
    pub output: Option<Place>,
    permissions: Vec<String>,
    trust: Trust,
    frames: Vec<Frame>,
    /// Steps executed so far (each pass through a Repeat counts).
    pub executed: u64,
    clock: Stopwatch,
    pub time_limit_ms: f64,
    /// Dry run (the footprint check): steps that would touch files or can't be previewed stop
    /// the run early instead of running.
    pub dry_run: bool,
    /// A Stop step ended the run: its message, and whether it asked for a rollback.
    pub stopped: Option<(String, bool)>,
    /// A dry run stopped before this step (path, reason): what follows isn't in the footprint.
    pub partial: Option<(String, String)>,
    /// The last step's resolved parameters (for the debugger).
    pub last_params: Option<Json>,
    /// Set when the footprint was partial: each step is checked for writes over data as it runs.
    guard: Option<Guard>,
}

/// Checks each step of a real run for writes over data the footprint couldn't see.
#[derive(Clone, Debug)]
struct Guard {
    /// The workbook when the run started, and for each sheet now, its index there (`None` for a
    /// sheet the run added).
    start: Arc<Workbook>,
    origin: Vec<Option<usize>>,
    /// Where the footprint stopped: "step 2 (file.exportCsv writes a file)".
    partial: String,
    /// The conflict rule is ask.
    ask: bool,
}

impl Guard {
    fn new(start: Arc<Workbook>, partial: &Json, ask: bool) -> Guard {
        let text = |k: &str| partial.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
        let partial = format!("step {} ({})", text("step"), text("reason"));
        Guard { origin: (0..start.sheets.len()).map(Some).collect(), start, partial, ask }
    }

    /// What one step wrote over that held data when the run started: cells outside the inputs,
    /// sheets and objects it deleted ("B1 on Sheet1; 1 object"), or `""`.
    fn check(&mut self, prev: &Workbook, after: &Workbook, inputs: &[Bound]) -> String {
        let (pairs, _) = pair_sheets(prev, after);
        let mut origin = vec![None; after.sheets.len()];
        let mut parts = Vec::new();
        let mut objects = 0usize;
        for (pi, ai) in pairs {
            // A sheet the run added holds nothing of the person's.
            let Some(si) = self.origin.get(pi).copied().flatten() else { continue };
            let Some(ss) = self.start.sheets.get(si) else { continue };
            let Some(ai) = ai else {
                parts.push(format!("the sheet {}", ss.name));
                continue;
            };
            if let Some(o) = origin.get_mut(ai) {
                *o = Some(si);
            }
            let (Some(ps), Some(sa)) = (prev.sheets.get(pi), after.sheets.get(ai)) else { continue };
            if Arc::ptr_eq(ps, sa) {
                continue;
            }
            let in_input = |c: CellRef| inputs.iter().any(|(_, p, _)| p.sheet == si && p.range.contains(c));
            let mut hit: Vec<CellRef> = ps
                .cells
                .diff(&sa.cells)
                .into_iter()
                .filter(|&c| !in_input(c) && !recalculated_only(ps.cell(c), sa.cell(c)))
                .filter(|&c| ss.cell(c).is_some_and(has_content) && content_differs(ss.cell(c), sa.cell(c)))
                .collect();
            if !hit.is_empty() {
                let mut listed = ranges(&mut hit);
                if listed.len() > 3 {
                    listed.truncate(3);
                    listed.push("…".into());
                }
                parts.push(format!("{} on {}", listed.join(", "), sa.name));
            }
            objects += deleted(&ss.charts, &ps.charts, &sa.charts, |o| o.id)
                + deleted(&ss.images, &ps.images, &sa.images, |o| o.id)
                + deleted(&ss.shapes, &ps.shapes, &sa.shapes, |o| o.id);
        }
        if objects > 0 {
            parts.push(format!("{objects} object{}", if objects == 1 { "" } else { "s" }));
        }
        self.origin = origin;
        parts.join("; ")
    }
}

/// Objects of `prev` that `after` no longer has and that were there at the start.
fn deleted<T>(start: &[T], prev: &[T], after: &[T], id: impl Fn(&T) -> u32) -> usize {
    let kept: BTreeSet<u32> = after.iter().map(&id).collect();
    let had: BTreeSet<u32> = start.iter().map(&id).collect();
    prev.iter().map(&id).filter(|i| !kept.contains(i) && had.contains(i)).count()
}

impl Machine {
    pub fn new(p: &Prepared, dry_run: bool) -> Machine {
        let mut m = Machine {
            name: p.mac.name.clone(),
            doc: p.doc,
            inputs: p.inputs.clone(),
            output: p.output,
            permissions: p.mac.permissions.clone(),
            trust: p.trust,
            frames: vec![Frame { steps: Arc::new(p.mac.steps.clone()), next: 0, prefix: String::new(), repeat: None }],
            executed: 0,
            clock: Stopwatch::start(),
            time_limit_ms: DEFAULT_TIME_LIMIT_MS,
            dry_run,
            stopped: None,
            partial: None,
            last_params: None,
            guard: None,
        };
        m.settle();
        m
    }

    /// Moves to the next enabled step: past disabled steps, finished frames and loop passes.
    fn settle(&mut self) {
        while let Some(f) = self.frames.last_mut() {
            if let Some(st) = f.steps.get(f.next) {
                if st.enabled {
                    return;
                }
                f.next += 1;
                continue;
            }
            if let Some(l) = f.repeat.as_mut()
                && l.at.saturating_add(1) < l.count
            {
                l.at += 1;
                f.next = 0;
                continue;
            }
            self.frames.pop();
        }
    }

    pub fn is_done(&self) -> bool {
        self.frames.is_empty()
    }

    /// Starts the time limit again (the debugger gives each action its own).
    pub fn restart_clock(&mut self) {
        self.clock = Stopwatch::start();
    }

    /// The current step for the debugger: its path, kind, command, parameters as they would run
    /// now, and the Repeat pass.
    pub fn current_json(&self, wb: &Workbook) -> Json {
        let Some((path, st)) = self.current() else { return Json::Null };
        let env = self.env(wb);
        let mut out = json!({"step": path, "breakpoint": st.breakpoint});
        if !st.note.is_empty() {
            out["note"] = json!(st.note);
        }
        match &st.kind {
            StepKind::Command { id, params } => {
                out["kind"] = json!("command");
                out["command"] = json!(id);
                match resolve::resolve_params(params, &env) {
                    Ok(p) => out["params"] = p,
                    Err(e) => out["paramsError"] = json!(e.message()),
                }
            }
            StepKind::Repeat { over, of, .. } => {
                out["kind"] = json!("repeat");
                out["over"] = json!(resolve::resolve_ref(over, &env).map(|p| resolve::a1(p, &env)).unwrap_or_else(|e| e.message().to_string()));
                out["of"] = json!(if *of == RepeatOf::Rows { "rows" } else { "columns" });
            }
            StepKind::If { cell, .. } => {
                out["kind"] = json!("if");
                out["cell"] = json!(resolve::resolve_ref(cell, &env).map(|p| resolve::a1(p, &env)).unwrap_or_else(|e| e.message().to_string()));
            }
            StepKind::Stop { message, .. } => {
                out["kind"] = json!("stop");
                out["message"] = json!(message);
            }
        }
        if let Some((at, count)) = self.iteration() {
            out["iteration"] = json!({"at": at, "of": count});
        }
        out
    }

    /// The step that runs next, with its path.
    pub fn current(&self) -> Option<(String, &Step)> {
        let f = self.frames.last()?;
        f.steps.get(f.next).map(|st| (child_path(&f.prefix, f.next), st))
    }

    /// The innermost Repeat's pass (1-based) and count.
    pub fn iteration(&self) -> Option<(u32, u32)> {
        self.frames.iter().rev().find_map(|f| f.repeat).map(|l| (l.at + 1, l.count))
    }

    fn item(&self) -> Option<(RepeatOf, Place)> {
        self.frames.iter().rev().find_map(|f| f.repeat).map(|l| (l.of, l.item()))
    }

    /// Runs every remaining step.
    pub fn run_to_end(&mut self, s: &mut Session) -> std::result::Result<(), StepError> {
        while !self.is_done() {
            self.step(s)?;
        }
        Ok(())
    }

    /// Runs the current step and moves to the next one.
    pub fn step(&mut self, s: &mut Session) -> std::result::Result<(), StepError> {
        let Some((path, st)) = self.current().map(|(p, st)| (p, st.clone())) else { return Ok(()) };
        if let Some(f) = self.frames.last_mut() {
            f.next += 1;
        }
        let r = self.exec(s, &path, &st);
        if r.is_err() {
            self.frames.clear();
        }
        self.settle();
        r
    }

    fn exec(&mut self, s: &mut Session, path: &str, st: &Step) -> std::result::Result<(), StepError> {
        self.executed += 1;
        let at = |msg: String| StepError::new(path, None, msg);
        if self.executed > MAX_EXECUTED_STEPS {
            return Err(at(format!("The macro ran more than {MAX_EXECUTED_STEPS} steps and was stopped.")));
        }
        if self.clock.elapsed_ms().is_some_and(|ms| ms > self.time_limit_ms) {
            return Err(at(format!("The macro ran longer than {} seconds and was stopped.", (self.time_limit_ms / 1000.0).round())));
        }
        // Steps run on the macro's workbook, whichever one is in front.
        let Some(i) = s.docs.iter().position(|d| d.uid == self.doc) else { return Err(at("The workbook was closed.".into())) };
        s.set_active(i);
        match &st.kind {
            StepKind::Command { id, params } => self.command(s, path, id, params),
            StepKind::Repeat { over, of, max, body } => {
                let place = {
                    let wb = &s.doc().map_err(|e| at(e.to_string()))?.wb;
                    let env = self.env(wb);
                    let p = resolve::resolve_ref(over, &env).map_err(|e| StepError::resolve(path, None, e))?;
                    trim(wb, p)
                };
                let items = match of {
                    RepeatOf::Rows => place.range.height(),
                    RepeatOf::Columns => place.range.width(),
                };
                let cap = (*max).min(MAX_EXECUTED_STEPS as u32);
                if items > cap {
                    let what = if *of == RepeatOf::Rows { "rows" } else { "columns" };
                    return Err(at(format!("Repeat over {} has {items} {what}, more than its limit of {cap}.", place.range.a1())));
                }
                if items > 0 && body.iter().any(|b| b.enabled) {
                    let repeat = Some(Loop { of: *of, over: place, at: 0, count: items });
                    self.frames.push(Frame { steps: Arc::new(body.clone()), next: 0, prefix: path.to_string(), repeat });
                }
                Ok(())
            }
            StepKind::If { cell, op, value, then, otherwise } => {
                let v = {
                    let wb = &s.doc().map_err(|e| at(e.to_string()))?.wb;
                    let env = self.env(wb);
                    let p = resolve::resolve_ref(cell, &env).map_err(|e| StepError::resolve(path, None, e))?;
                    if !p.range.is_single() {
                        return Err(at(format!("If looks at one cell; {} is {} cells.", p.range.a1(), p.range.count())));
                    }
                    wb.sheet(p.sheet).map(|sh| sh.value(p.range.start)).unwrap_or_default()
                };
                let (branch, name) = if compare(&v, *op, value) { (then, "then") } else { (otherwise, "else") };
                if branch.iter().any(|b| b.enabled) {
                    self.frames.push(Frame { steps: Arc::new(branch.clone()), next: 0, prefix: format!("{path}.{name}"), repeat: None });
                }
                Ok(())
            }
            StepKind::Stop { message, rollback } => {
                self.stopped = Some((message.clone(), *rollback));
                self.frames.clear();
                Ok(())
            }
        }
    }

    fn env<'a>(&'a self, wb: &'a Workbook) -> Env<'a> {
        Env { wb, active_sheet: wb.active_sheet, inputs: &self.inputs, output: self.output, item: self.item() }
    }

    fn command(&mut self, s: &mut Session, path: &str, id: &str, params: &Json) -> std::result::Result<(), StepError> {
        let fail = |msg: String| StepError::new(path, Some(id), msg);
        // The policy sees the parameters the command will get, after tokens became A1 text.
        let resolved = {
            let wb = &s.doc().map_err(|e| fail(e.to_string()))?.wb;
            resolve::resolve_params(params, &self.env(wb)).map_err(|e| StepError::resolve(path, Some(id), e))?
        };
        match (macro_policy(id, params, self.trust), macro_policy(id, &resolved, self.trust)) {
            (_, Policy::Deny(why)) | (Policy::Deny(why), _) => return Err(fail(format!("{id} can't run in a macro: {why}."))),
            (written, Policy::NeedsPermission(what)) => {
                if !self.permissions.iter().any(|p| p == id) {
                    return Err(fail(format!("{id} {what}. Add it to the macro's permissions to allow it.")));
                }
                // The gate asked the person about this step only if it saw the file access.
                if !matches!(written, Policy::NeedsPermission(_)) {
                    return Err(fail(format!("{id} {what} only once its parameters are filled in, so it wasn't approved.")));
                }
                if self.dry_run {
                    self.partial = Some((path.to_string(), format!("{id} {what}")));
                    self.frames.clear();
                    return Ok(());
                }
            }
            (_, Policy::Allow | Policy::Ask(_)) => {}
        }
        if self.dry_run && !previewable(id) {
            self.partial = Some((path.to_string(), format!("{id} can't be previewed")));
            self.frames.clear();
            return Ok(());
        }
        self.last_params = Some(resolved.clone());
        let prev = self.guard.as_ref().and_then(|_| s.docs.iter().find(|d| d.uid == self.doc)).map(|d| d.wb.clone());
        let queued = s.ui_requests.len();
        let r = s.execute(id, resolved);
        // Macros never open dialogs or web pages; messages for the person stay queued.
        let mut blocked = None;
        let asked: Vec<UiRequest> = s.ui_requests.drain(queued.min(s.ui_requests.len())..).collect();
        for q in asked {
            match q {
                UiRequest::Dialog(name, _) => blocked = Some(format!("{id} would open the {name} dialog; give the step the parameters it needs.")),
                UiRequest::OpenUrl(_) => blocked = Some(format!("{id} would open a web page, which macros don't do.")),
                UiRequest::EditCell(_) => {}
                m @ UiRequest::Message(_) => s.ui_requests.push(m),
            }
        }
        r.map_err(|e| fail(e.to_string()))?;
        if let Some(b) = blocked {
            return Err(fail(b));
        }
        if let (Some(g), Some(prev)) = (self.guard.as_mut(), prev) {
            let after = s.docs.iter().find(|d| d.uid == self.doc).map(|d| d.wb.clone()).ok_or_else(|| fail("The workbook was closed.".into()))?;
            let what = g.check(&prev, &after, &self.inputs);
            if !what.is_empty() {
                let msg = format!("It would write over data ({what}); the check before the run stopped at {} and couldn't see this.", g.partial);
                return Err(StepError { confirm: g.ask, ..fail(msg) });
            }
        }
        Ok(())
    }
}

/// If's comparison of a cell's value with a step's `value`, the way Excel's `=`, `<`… compare.
fn compare(v: &Value, op: CompareOp, want: &Json) -> bool {
    let empty = v.is_empty() || v.as_text() == Some("");
    let want_text = || match want {
        Json::String(t) => t.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    };
    let ord = || -> Option<std::cmp::Ordering> {
        let w = match want {
            Json::Null => Value::Empty,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::number(n.as_f64()?),
            // "5" compares as a number with a number in the cell.
            Json::String(t) => match (v, t.trim().parse::<f64>()) {
                (Value::Number(_), Ok(n)) => Value::number(n),
                _ => Value::text(t.as_str()),
            },
            _ => return None,
        };
        match (v, &w) {
            (Value::Error(e), Value::Text(t)) => {
                Some(if e.as_str().eq_ignore_ascii_case(t.trim()) { std::cmp::Ordering::Equal } else { std::cmp::Ordering::Less })
            }
            (Value::Error(_), _) | (_, Value::Error(_)) | (Value::Array(_), _) => None,
            _ => Some(gridcraft_core::value::compare(v, &w)),
        }
    };
    use std::cmp::Ordering::*;
    match op {
        CompareOp::Empty => empty,
        CompareOp::NotEmpty => !empty,
        CompareOp::Contains => v.display().to_lowercase().contains(&want_text().to_lowercase()),
        CompareOp::Eq => ord() == Some(Equal),
        CompareOp::Ne => ord() != Some(Equal),
        CompareOp::Gt => ord() == Some(Greater),
        CompareOp::Ge => matches!(ord(), Some(Greater | Equal)),
        CompareOp::Lt => ord() == Some(Less),
        CompareOp::Le => matches!(ord(), Some(Less | Equal)),
    }
}

// ---------------------------------------------------------------- footprint

/// What a macro would change, from a dry run on the real workbook.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Footprint {
    pub sheets: Vec<SheetFootprint>,
    pub sheets_added: Vec<String>,
    pub sheets_removed: Vec<String>,
    pub objects_added: usize,
    pub objects_changed: usize,
    pub objects_removed: usize,
    /// The dry run stopped early before this step; later steps aren't in the footprint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<Json>,
    /// The step the run would fail at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StepError>,
    /// A Stop step's message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
}

/// The cells a macro would write on one sheet, outside its inputs.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetFootprint {
    pub sheet: String,
    #[serde(skip)]
    pub index: usize,
    /// The written cells as ranges (at most 500 listed), and how many cells.
    pub cells: Vec<String>,
    pub changed: usize,
    /// Written cells that hold data now.
    pub conflicts: Vec<String>,
    pub conflict_count: usize,
    /// The block that holds every written cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<String>,
    #[serde(skip)]
    pub bounds_range: Option<RangeRef>,
}

impl Footprint {
    /// Cells that hold data and would be written, plus sheets and objects that would go.
    pub fn conflict_count(&self) -> usize {
        self.sheets.iter().map(|s| s.conflict_count).sum::<usize>() + self.sheets_removed.len() + self.objects_removed
    }

    /// "C2:C9 on Sheet1, the sheet Old, 2 objects".
    pub fn describe_conflicts(&self) -> String {
        let mut parts: Vec<String> = self
            .sheets
            .iter()
            .filter(|s| s.conflict_count > 0)
            .map(|s| {
                let mut listed = s.conflicts.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                if s.conflicts.len() > 3 {
                    listed.push_str(", …");
                }
                format!("{listed} on {}", s.sheet)
            })
            .collect();
        parts.extend(self.sheets_removed.iter().map(|n| format!("the sheet {n}")));
        if self.objects_removed > 0 {
            parts.push(format!("{} object{}", self.objects_removed, if self.objects_removed == 1 { "" } else { "s" }));
        }
        parts.join("; ")
    }
}

/// Session state a dry run may touch, put back afterwards. The workbook, its selection and its
/// history are put back by the dry run's undo group.
struct Saved {
    active: usize,
    docs: Vec<u64>,
    sheet_selections: Vec<Selection>,
    unlocked_ranges: Vec<(String, String)>,
    trust: Trust,
    clipboard: Option<crate::Clipboard>,
    prefs: crate::Prefs,
    mode: crate::Mode,
    journal: usize,
    recording: Option<usize>,
    ui_requests: usize,
    format_painter: Option<(Arc<Workbook>, usize, RangeRef, bool)>,
    last_find: Option<Json>,
    hide_ruler: bool,
    hidden_windows: Vec<usize>,
    draw: (String, String, f32),
    comments_pane: bool,
    show_all_notes: bool,
}

impl Saved {
    fn take(s: &mut Session, doc: u64) -> Saved {
        let d = s.docs.iter().find(|d| d.uid == doc);
        Saved {
            active: s.active,
            docs: s.docs.iter().map(|d| d.uid).collect(),
            sheet_selections: d.map(|d| d.sheet_selections.clone()).unwrap_or_default(),
            unlocked_ranges: d.map(|d| d.unlocked_ranges.clone()).unwrap_or_default(),
            trust: d.map_or(Trust::Local, |d| d.trust),
            clipboard: s.clipboard.clone(),
            prefs: s.prefs.clone(),
            mode: s.mode,
            journal: s.journal.len(),
            recording: s.recording,
            ui_requests: s.ui_requests.len(),
            format_painter: s.format_painter.clone(),
            last_find: s.last_find.clone(),
            hide_ruler: s.hide_ruler,
            hidden_windows: s.hidden_windows.clone(),
            draw: (s.draw_tool.clone(), s.draw_color.clone(), s.draw_width),
            comments_pane: s.comments_pane,
            show_all_notes: s.show_all_notes,
        }
    }

    fn restore(self, s: &mut Session, doc: u64) {
        s.docs.retain(|d| self.docs.contains(&d.uid));
        if let Some(d) = s.docs.iter_mut().find(|d| d.uid == doc) {
            d.sheet_selections = self.sheet_selections;
            d.unlocked_ranges = self.unlocked_ranges;
            d.trust = self.trust;
        }
        s.active = self.active.min(s.docs.len().saturating_sub(1));
        s.clipboard = self.clipboard;
        s.prefs = self.prefs;
        s.mode = self.mode;
        s.journal.truncate(self.journal);
        s.recording = self.recording;
        s.ui_requests.truncate(self.ui_requests);
        s.format_painter = self.format_painter;
        s.last_find = self.last_find;
        s.hide_ruler = self.hide_ruler;
        s.hidden_windows = self.hidden_windows;
        (s.draw_tool, s.draw_color, s.draw_width) = self.draw;
        s.comments_pane = self.comments_pane;
        s.show_all_notes = self.show_all_notes;
    }
}

/// Runs the macro in dry-run mode on its workbook, puts everything back, and says what it would
/// change. The dry run stops once `time_limit_ms` has passed on `clock`.
pub fn footprint(s: &mut Session, p: &Prepared, clock: Stopwatch, time_limit_ms: f64) -> Result<Footprint> {
    let i = s.docs.iter().position(|d| d.uid == p.doc).ok_or(EngineError::NoDocument)?;
    s.set_active(i);
    let saved = Saved::take(s, p.doc);
    let before = s.doc()?.wb.clone();
    let token = s.group_begin("Check Macro")?;
    if let Some(d) = s.docs.iter_mut().find(|d| d.uid == p.doc) {
        d.touched = Some(Vec::new());
    }
    let mut m = Machine::new(p, true);
    (m.clock, m.time_limit_ms) = (clock, time_limit_ms);
    let outcome = m.run_to_end(s);
    let (after, touched) = match s.docs.iter_mut().find(|d| d.uid == p.doc) {
        Some(d) => (d.wb.clone(), d.touched.take().unwrap_or_default()),
        None => (before.clone(), Vec::new()),
    };
    let rolled = s.group_rollback(token);
    saved.restore(s, p.doc);
    rolled?;
    let (after, touched) = if matches!(m.stopped, Some((_, true))) { (before.clone(), Vec::new()) } else { (after, touched) };
    let mut fp = diff(&before, &after, &p.inputs, &touched);
    fp.error = outcome.err();
    fp.stopped = m.stopped.map(|(msg, _)| msg);
    fp.partial = m.partial.map(|(step, reason)| json!({"step": step, "reason": reason}));
    Ok(fp)
}

/// Compares the workbook before and after a dry run. `touched` adds the cells commands wrote
/// without changing them (a formula written again), so a shift moves everything the macro writes.
fn diff(before: &Workbook, after: &Workbook, inputs: &[Bound], touched: &[(String, CellRef)]) -> Footprint {
    let mut fp = Footprint::default();
    let (pairs, added) = pair_sheets(before, after);
    fp.sheets_added = added.into_iter().filter_map(|a| after.sheet(a).map(|s| s.name.clone())).collect();
    for (bi, ai) in pairs {
        let Some(bs) = before.sheets.get(bi) else { continue };
        let Some(ai) = ai else {
            fp.sheets_removed.push(bs.name.clone());
            continue;
        };
        let Some(after_sheet) = after.sheets.get(ai) else { continue };
        let rewritten: BTreeSet<CellRef> = touched.iter().filter(|(n, _)| *n == after_sheet.name).map(|(_, c)| *c).collect();
        if Arc::ptr_eq(bs, after_sheet) && rewritten.is_empty() {
            continue;
        }
        let (added, changed, removed) = objects(bs, after_sheet);
        fp.objects_added += added;
        fp.objects_changed += changed;
        fp.objects_removed += removed;
        let in_input = |c: CellRef| inputs.iter().any(|(_, p, _)| p.sheet == bi && p.range.contains(c));
        let mut written = Vec::new();
        let mut conflicts = Vec::new();
        let mut cells: BTreeSet<CellRef> = bs.cells.diff(&after_sheet.cells).into_iter().collect();
        cells.extend(rewritten.iter().copied());
        for c in cells {
            if in_input(c) {
                continue;
            }
            let (b, a) = (bs.cell(c), after_sheet.cell(c));
            if recalculated_only(b, a) && !rewritten.contains(&c) {
                continue;
            }
            written.push(c);
            if b.is_some_and(has_content) && content_differs(b, a) {
                conflicts.push(c);
            }
        }
        let Some(first) = written.first() else { continue };
        let bounds = written.iter().fold(RangeRef::cell(*first), |r, c| r.union(&RangeRef::cell(*c)));
        fp.sheets.push(SheetFootprint {
            sheet: bs.name.clone(),
            index: bi,
            changed: written.len(),
            conflict_count: conflicts.len(),
            cells: ranges(&mut written),
            conflicts: ranges(&mut conflicts),
            bounds: Some(bounds.a1()),
            bounds_range: Some(bounds),
        });
    }
    fp
}

/// Each sheet of `before` with its index in `after` (`None` when deleted), and the sheets of
/// `after` that are new. Sheets are matched by name; what's left is matched in order (a renamed
/// sheet).
fn pair_sheets(before: &Workbook, after: &Workbook) -> (Vec<(usize, Option<usize>)>, Vec<usize>) {
    let mut pairs: Vec<(usize, Option<usize>)> = before.sheets.iter().enumerate().map(|(bi, sh)| (bi, after.sheet_index(&sh.name))).collect();
    let matched: BTreeSet<usize> = pairs.iter().filter_map(|(_, a)| *a).collect();
    let mut spare = (0..after.sheets.len()).filter(|a| !matched.contains(a));
    for (_, a) in pairs.iter_mut().filter(|(_, a)| a.is_none()) {
        *a = spare.next();
    }
    (pairs, spare.collect())
}

/// Only a formula's cached value changed (a recalculation, not a write).
fn recalculated_only(b: Option<&Cell>, a: Option<&Cell>) -> bool {
    match (b, a) {
        (Some(b), Some(a)) => match (&b.formula, &a.formula) {
            (Some(x), Some(y)) => x.text == y.text && x.array == y.array && b.style == a.style,
            _ => false,
        },
        _ => false,
    }
}

fn has_content(c: &Cell) -> bool {
    c.formula.is_some() || !(c.value.is_empty() || c.value.as_text() == Some(""))
}

fn content_differs(b: Option<&Cell>, a: Option<&Cell>) -> bool {
    match (b, a) {
        (Some(b), Some(a)) => match (&b.formula, &a.formula) {
            (Some(x), Some(y)) => x.text != y.text,
            (None, None) => b.value != a.value,
            _ => true,
        },
        (Some(b), None) => has_content(b),
        _ => false,
    }
}

/// Objects added, changed and removed between two versions of a sheet.
fn objects(b: &Sheet, a: &Sheet) -> (usize, usize, usize) {
    fn count<T: PartialEq>(b: &[T], a: &[T], id: impl Fn(&T) -> u32) -> (usize, usize, usize) {
        let added = a.iter().filter(|x| !b.iter().any(|y| id(y) == id(x))).count();
        let removed = b.iter().filter(|x| !a.iter().any(|y| id(y) == id(x))).count();
        let changed = b.iter().filter(|x| a.iter().any(|y| id(y) == id(x) && y != *x)).count();
        (added, changed, removed)
    }
    let parts = [count(&b.charts, &a.charts, |o| o.id), count(&b.images, &a.images, |o| o.id), count(&b.shapes, &a.shapes, |o| o.id)];
    parts.iter().fold((0, 0, 0), |t, p| (t.0 + p.0, t.1 + p.1, t.2 + p.2))
}

/// Cells as A1 ranges: runs along each row, then runs with the same columns on following rows.
pub(crate) fn ranges(cells: &mut [CellRef]) -> Vec<String> {
    cells.sort_unstable_by_key(|c| (c.row, c.col));
    let mut runs: Vec<RangeRef> = Vec::new();
    for c in cells.iter() {
        match runs.last_mut() {
            Some(r) if r.end.row == c.row && r.end.col.checked_add(1) == Some(c.col) => r.end.col = c.col,
            _ => runs.push(RangeRef::cell(*c)),
        }
    }
    let mut merged: Vec<RangeRef> = Vec::new();
    for r in runs {
        // The block above with the same columns that ends on the row before.
        match merged
            .iter_mut()
            .rev()
            .take(64)
            .find(|m| m.start.col == r.start.col && m.end.col == r.end.col && m.end.row.checked_add(1) == Some(r.start.row))
        {
            Some(m) => m.end.row = r.end.row,
            None => merged.push(r),
        }
    }
    merged.iter().take(MAX_LISTED_RANGES).map(RangeRef::a1).collect()
}

/// Inserts blank cells where the footprint lands (moving what is there down or right), so the
/// macro writes into empty cells.
fn make_room(s: &mut Session, fp: &Footprint, down: bool) -> Result<()> {
    if !fp.sheets_removed.is_empty() || fp.objects_removed > 0 {
        return Err(EngineError::Other("Shifting can't make room: the macro deletes sheets or objects.".into()));
    }
    let wb = s.doc()?.wb.clone();
    for sh in fp.sheets.iter().filter(|s| s.conflict_count > 0) {
        let Some(bounds) = sh.bounds_range else { continue };
        let env = Env { wb: &wb, active_sheet: usize::MAX, inputs: &[], output: None, item: None };
        let range = resolve::a1(Place { sheet: sh.index, range: bounds }, &env);
        s.execute("home.insertCells", json!({"range": range, "shift": if down { "down" } else { "right" }}))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- run

/// A macro that passed its checks and is about to run.
pub struct Begun {
    pub mac: Macro,
    pub prepared: Prepared,
    pub rule: OnConflict,
    pub time_limit_ms: f64,
    pub inputs: Json,
}

/// The checks before a run, on the active workbook: the macro exists, the policy allows it, the
/// person approved it if it needs that, and its inputs are right.
pub fn begin(s: &mut Session, name: &str, p: &Json) -> Result<Begun> {
    let d = s.doc()?;
    let (doc, trust) = (d.uid, d.trust);
    let m = super::find(&d.wb, name)
        .and_then(|i| d.wb.macros.get(i))
        .cloned()
        .ok_or_else(|| EngineError::Other(format!("There is no macro called '{name}'.")))?;
    let asks = gate(&m, trust)?;
    approve(s, &m, doc, &asks, p.get("approve").and_then(Json::as_bool).unwrap_or(false))?;
    let prepared = prepare(s, &m, p.get("inputs"))?;
    let rule = conflict_rule(p, &m)?;
    let time_limit_ms = time_limit(p);
    let inputs = prepared.inputs_json(&s.doc()?.wb);
    Ok(Begun { mac: m, prepared, rule, time_limit_ms, inputs })
}

/// The footprint check and the output rule, inside the run's undo group: an error when the macro
/// would fail or write over data it may not, after shifting the data if the rule says so. Returns
/// the machine for the run and whether it shifted. The dry runs count towards the run's time
/// limit.
///
/// A partial footprint proves nothing about the steps it didn't reach, so then the machine
/// checks each step as it runs and stops the run at one that writes over data.
pub fn make_way(s: &mut Session, b: &Begun) -> Result<(Machine, bool)> {
    let (m, rule) = (&b.mac, b.rule);
    let mut machine = Machine::new(&b.prepared, false);
    machine.time_limit_ms = b.time_limit_ms;
    if rule == OnConflict::Overwrite {
        return Ok((machine, false));
    }
    let mut fp = footprint(s, &b.prepared, machine.clock, b.time_limit_ms)?;
    if let Some(e) = fp.error.take() {
        return Err(e.into_engine(&m.name));
    }
    let mut shifted = false;
    if fp.conflict_count() > 0 {
        let what = fp.describe_conflicts();
        match rule {
            OnConflict::Ask => {
                return Err(EngineError::Confirm(format!(
                    "Macro '{}' would write over cells that hold data ({what}). Overwrite them, or run again with onConflict shiftDown, shiftRight or fail",
                    m.name
                )));
            }
            OnConflict::Fail | OnConflict::Overwrite => {
                return Err(EngineError::Other(format!("Macro '{}' would write over cells that hold data ({what}). Nothing was changed.", m.name)));
            }
            OnConflict::ShiftDown | OnConflict::ShiftRight => {
                make_room(s, &fp, rule == OnConflict::ShiftDown)?;
                fp = footprint(s, &b.prepared, machine.clock, b.time_limit_ms)?;
                if let Some(e) = fp.error.take() {
                    return Err(e.into_engine(&m.name));
                }
                if fp.conflict_count() > 0 {
                    return Err(EngineError::Other(format!(
                        "Macro '{}': shifting the data didn't make room ({}). Nothing was changed.",
                        m.name,
                        fp.describe_conflicts()
                    )));
                }
                shifted = true;
            }
        }
    }
    if let Some(p) = &fp.partial {
        let start = s.docs.iter().find(|d| d.uid == b.prepared.doc).ok_or(EngineError::NoDocument)?.wb.clone();
        machine.guard = Some(Guard::new(start, p, rule == OnConflict::Ask));
    }
    Ok((machine, shifted))
}

/// Runs macro `name` on the active workbook: `{inputs?, onConflict?, overwrite?, approve?,
/// timeLimit?}` (see `macro.run`).
pub fn run(s: &mut Session, name: &str, p: &Json) -> Result<Json> {
    let b = begin(s, name, p)?;
    let label = format!("Run Macro: {}", b.mac.name);
    s.grouped(&label, |s| {
        let (mut machine, shifted) = make_way(s, &b)?;
        let name = &b.mac.name;
        machine.run_to_end(s).map_err(|e| e.into_engine(name))?;
        match machine.stopped {
            Some((msg, true)) => Err(EngineError::Other(format!("Macro '{name}' stopped: {msg} Nothing was changed."))),
            stopped => {
                let mut out = json!({"macro": name, "ran": machine.executed, "inputs": b.inputs});
                if let Some((msg, _)) = stopped {
                    out["stopped"] = json!(msg);
                }
                if shifted {
                    out["shifted"] = json!(b.rule.name());
                }
                Ok(out)
            }
        }
    })
}

/// The run's `timeLimit` (seconds) in milliseconds, or the default.
fn time_limit(p: &Json) -> f64 {
    p.get("timeLimit").and_then(Json::as_f64).map(|t| (t * 1000.0).clamp(1.0, 600_000.0)).unwrap_or(DEFAULT_TIME_LIMIT_MS)
}

/// The conflict rule for this run: `onConflict`, or `overwrite: true` (the answer to the
/// question), else the macro's own.
fn conflict_rule(p: &Json, m: &Macro) -> Result<OnConflict> {
    if let Some(t) = p.get("onConflict").and_then(Json::as_str) {
        return OnConflict::parse(t)
            .ok_or_else(|| EngineError::Other(format!("onConflict must be ask, fail, overwrite, shiftDown or shiftRight, not '{t}'.")));
    }
    if p.get("overwrite").and_then(Json::as_bool) == Some(true) {
        return Ok(OnConflict::Overwrite);
    }
    Ok(m.output.on_conflict)
}

/// `macro.check`: what running the macro would do, without changing anything.
pub fn check(s: &mut Session, name: &str, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (doc, trust) = (d.uid, d.trust);
    let m = super::find(&d.wb, name)
        .and_then(|i| d.wb.macros.get(i))
        .cloned()
        .ok_or_else(|| EngineError::Other(format!("There is no macro called '{name}'.")))?;
    let mut out = json!({"macro": m.name, "ok": false, "onConflict": m.output.on_conflict.name()});
    let asks = match gate(&m, trust) {
        Ok(a) => a,
        Err(e) => {
            out["errors"] = json!([e.to_string()]);
            return Ok(out);
        }
    };
    let approved = s.macro_approvals.contains(&(doc, super::fingerprint(&m)));
    if !asks.is_empty() && !approved {
        // Nothing from an unapproved macro runs, not even in a dry run.
        out["needsApproval"] = json!(asks);
    }
    let prepared = match prepare(s, &m, p.get("inputs")) {
        Ok(pr) => pr,
        Err(e) => {
            out["errors"] = json!([e.to_string()]);
            return Ok(out);
        }
    };
    out["inputs"] = prepared.inputs_json(&s.doc()?.wb);
    if let Some(o) = prepared.output {
        let wb = &s.doc()?.wb;
        out["output"] = json!(resolve::a1(o, &Env { wb, active_sheet: wb.active_sheet, inputs: &[], output: None, item: None }));
    }
    if !asks.is_empty() && !approved {
        return Ok(out);
    }
    let fp = footprint(s, &prepared, Stopwatch::start(), time_limit(p))?;
    out["ok"] = json!(fp.error.is_none());
    out["conflicts"] = json!(fp.conflict_count());
    if let Some(e) = &fp.error {
        out["errors"] = json!([e.describe()]);
    }
    out["footprint"] = serde_json::to_value(&fp).unwrap_or(Json::Null);
    Ok(out)
}
