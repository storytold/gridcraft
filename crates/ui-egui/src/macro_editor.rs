//! The macro step editor (Automate › New Macro; Macros › Edit, Create, Step Into): a large,
//! resizable window.
//!
//! - **Left, the step list.** Each row has its number, a breakpoint dot in the gutter (click to
//!   toggle), a category icon, a one-line summary, an error marker and an enabled checkbox. Rows
//!   drag to reorder within their list (the shared [`crate::dnd`] helpers); Add Step opens a
//!   Command Palette picker; Duplicate and Delete act on the selected step.
//! - **Right, the inspector.** Step: the selected step's fields, with RefEdit boxes that switch
//!   between relative tokens (`Input.col(2)`) and absolute A1, and token suggestions. Inputs: the
//!   shape, header and column types each input must have, from an example. Output: the anchor and
//!   what to do when results would land on data.
//! - **The debug bar.** Check (a dry run that colours the grid), Run, Step, Continue, Run to Here
//!   and Stop (keep the changes or undo them). While debugging, the current step is highlighted
//!   and its parameters are shown as they will run.
//!
//! The editor works on a draft and saves it with `macro.save`; checking, running and debugging
//! save first, then go through `macro.check`, `macro.run` and `macro.debug`.

use std::collections::BTreeMap;

use egui::{Color32, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::core::RangeRef;
use gridcraft_engine::macros::policy::{Policy, macro_policy};
use gridcraft_engine::macros::resolve::{self, Env, Place};
use gridcraft_engine::model::{ColType, CompareOp, InputKind, Macro, NamedInput, OnConflict, RepeatOf, Step, StepKind, Workbook};
use serde_json::{Map, Value as Json, json};

use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::theme::{self, Cat, Tokens};
use crate::widgets::small_button;
use crate::{SheetApp, dnd, macro_ui};

/// The inspector's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Step,
    Inputs,
    Output,
}

/// The Add Step picker (or Change, which replaces a step's command).
#[derive(Clone, Debug, Default)]
pub struct Picker {
    pub search: String,
    /// The step whose command is being changed; `None` adds a step.
    pub replace: Option<String>,
    pub index: usize,
}

/// The editor's state while it is open.
#[derive(Clone, Debug)]
pub struct Editor {
    pub draft: Macro,
    /// The name the macro is saved under (`None` until it is first saved).
    pub saved_as: Option<String>,
    pub dirty: bool,
    /// The selected row: a step path (`3`, `2.1`, `4.then.1`) or an If branch (`4.then`).
    pub selected: Option<String>,
    pub tab: Tab,
    /// The input selected on the Inputs tab.
    pub input: usize,
    pub picker: Option<Picker>,
    /// The last `macro.check` result (the grid overlays come from it).
    pub check: Option<Json>,
    /// The debugger's state while paused.
    pub debug: Option<Json>,
    /// What the last run, check or debug session ended with.
    pub outcome: Option<String>,
    /// Errors from the last check or run, by step path.
    pub errors: BTreeMap<String, String>,
    /// Asking whether to keep what a stopped debug run did (and then close, if `closing`).
    pub ask_stop: bool,
    pub closing: bool,
    /// Asking whether to save before closing.
    pub ask_close: bool,
    /// Start debugging on the first frame (Macros › Step Into).
    pub start_debug: bool,
    /// Check on the first frame (`{"check": true}`).
    pub start_check: bool,
    /// The example the first input stands for (the selection when the editor opened, or the
    /// input's default range): Check, Run and Debug use it, and tokens are shown against it.
    pub example: String,
    /// The parameter being added to the selected step: (key, value).
    pub new_param: (String, String),
    /// The reference boxes' picking state.
    pub refs: RefPick,
}

/// Which reference box grid selections write into, and the selection to put back on closing.
#[derive(Clone, Debug, Default)]
pub struct RefPick {
    /// The reference box that grid selections currently write into.
    pub pick: Option<String>,
    /// The selection reference last written by (or seen while) picking.
    pub seen: String,
    /// Sheet, ranges and active cell from before picking, put back when the editor closes.
    pub home: Option<(usize, String, String)>,
    /// The reference box the window is collapsed to (only that box shows).
    pub collapsed: Option<String>,
}

// ---------------------------------------------------------------- step paths

fn child(parent: &str, i: usize) -> String {
    if parent.is_empty() { (i + 1).to_string() } else { format!("{parent}.{}", i + 1) }
}

/// `"4.then.2"` → (`"4.then"`, 1). `None` for a branch key or anything else that isn't a step.
pub fn split_path(path: &str) -> Option<(&str, usize)> {
    let (parent, last) = path.rsplit_once('.').unwrap_or(("", path));
    Some((parent, last.parse::<usize>().ok()?.checked_sub(1)?))
}

/// The step list a parent key names: `""` (the top), `"2"` (a Repeat's body), `"4.then"`.
pub fn list_mut<'a>(mut cur: &'a mut Vec<Step>, parent: &str) -> Option<&'a mut Vec<Step>> {
    let toks: Vec<&str> = parent.split('.').filter(|t| !t.is_empty()).collect();
    let mut i = 0;
    while let Some(tok) = toks.get(i) {
        let idx = tok.parse::<usize>().ok()?.checked_sub(1)?;
        let branch = toks.get(i + 1).copied();
        let st = cur.get_mut(idx)?;
        cur = match (&mut st.kind, branch) {
            (StepKind::If { then, .. }, Some("then")) => {
                i += 2;
                then
            }
            (StepKind::If { otherwise, .. }, Some("else")) => {
                i += 2;
                otherwise
            }
            (StepKind::Repeat { body, .. }, _) => {
                i += 1;
                body
            }
            _ => return None,
        };
    }
    Some(cur)
}

pub fn step_mut<'a>(steps: &'a mut Vec<Step>, path: &str) -> Option<&'a mut Step> {
    let (parent, i) = split_path(path)?;
    list_mut(steps, parent)?.get_mut(i)
}

pub fn step_at(steps: &[Step], path: &str) -> Option<Step> {
    let mut copy = steps.to_vec();
    step_mut(&mut copy, path).map(|s| s.clone())
}

/// The paths of the steps with a breakpoint.
pub fn breakpoints(steps: &[Step], parent: &str, out: &mut Vec<String>) {
    for (i, st) in steps.iter().enumerate() {
        let path = child(parent, i);
        if st.breakpoint {
            out.push(path.clone());
        }
        match &st.kind {
            StepKind::Repeat { body, .. } => breakpoints(body, &path, out),
            StepKind::If { then, otherwise, .. } => {
                breakpoints(then, &format!("{path}.then"), out);
                breakpoints(otherwise, &format!("{path}.else"), out);
            }
            _ => {}
        }
    }
}

/// One row of the step list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The step path, or the branch key for an If's Then/Else row.
    pub key: String,
    /// The list it is in, and its place there.
    pub parent: String,
    pub index: usize,
    pub depth: usize,
    /// `Some("Then")` / `Some("Else")` for a branch row.
    pub branch: Option<&'static str>,
}

/// The step list flattened into rows, nested steps indented under their Repeat or If.
pub fn rows(steps: &[Step], parent: &str, depth: usize, out: &mut Vec<Row>) {
    if depth > 16 {
        return;
    }
    for (i, st) in steps.iter().enumerate() {
        let key = child(parent, i);
        out.push(Row { key: key.clone(), parent: parent.to_string(), index: i, depth, branch: None });
        match &st.kind {
            StepKind::Repeat { body, .. } => rows(body, &key, depth + 1, out),
            StepKind::If { then, otherwise, .. } => {
                for (label, list, b) in [("Then", then, "then"), ("Else", otherwise, "else")] {
                    let bk = format!("{key}.{b}");
                    out.push(Row { key: bk.clone(), parent: key.clone(), index: 0, depth: depth + 1, branch: Some(label) });
                    rows(list, &bk, depth + 2, out);
                }
            }
            _ => {}
        }
    }
}

/// Moves the step at `from` in list `parent` so it lands before row `to` (an index among the
/// list's steps as they are now). Returns the step's new path.
pub fn move_step(steps: &mut Vec<Step>, parent: &str, from: usize, to: usize) -> Option<String> {
    let list = list_mut(steps, parent)?;
    if from >= list.len() {
        return None;
    }
    let st = list.remove(from);
    let to = if to > from { to - 1 } else { to }.min(list.len());
    list.insert(to, st);
    Some(child(parent, to))
}

// ---------------------------------------------------------------- summaries

fn short(v: &Json) -> String {
    let text = match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.chars().count() > 40 { format!("{}…", text.chars().take(39).collect::<String>()) } else { text }
}

/// The label of command `id` (`Set Cell`), or the id when it isn't a command.
pub fn command_label(id: &str) -> String {
    gridcraft_engine::find_command(id).map(|c| c.label.to_string()).unwrap_or_else(|| id.to_string())
}

/// A step in one line: "Set D2 to =B2*C2", "Repeat for each row of Input.data".
pub fn summary(st: &Step) -> String {
    match &st.kind {
        StepKind::Command { id, params } => {
            let p = |k: &str| params.get(k).map(short);
            match (id.as_str(), p("cell"), p("input"), p("range")) {
                ("cell.set", Some(c), Some(i), _) => format!("Set {c} to {i}"),
                ("selection.set", _, _, Some(r)) => format!("Select {r}"),
                _ => {
                    let label = command_label(id);
                    let args: Vec<String> =
                        params.as_object().map(|o| o.iter().take(3).map(|(k, v)| format!("{k}: {}", short(v))).collect()).unwrap_or_default();
                    if args.is_empty() { label } else { format!("{label} · {}", args.join(", ")) }
                }
            }
        }
        StepKind::Repeat { over, of, .. } => {
            format!("Repeat for each {} of {}", if *of == RepeatOf::Rows { "row" } else { "column" }, if over.is_empty() { "…" } else { over })
        }
        StepKind::If { cell, op, value, .. } => {
            let op = match op {
                CompareOp::Eq => "=",
                CompareOp::Ne => "≠",
                CompareOp::Gt => ">",
                CompareOp::Ge => "≥",
                CompareOp::Lt => "<",
                CompareOp::Le => "≤",
                CompareOp::Empty => return format!("If {cell} is empty"),
                CompareOp::NotEmpty => return format!("If {cell} isn't empty"),
                CompareOp::Contains => "contains",
            };
            format!("If {cell} {op} {}", short(value))
        }
        StepKind::Stop { message, .. } => {
            if message.is_empty() {
                "Stop".into()
            } else {
                format!("Stop: {message}")
            }
        }
    }
}

/// The icon and colour of a step's category.
pub fn category(st: &Step) -> (Icon, Cat) {
    match &st.kind {
        StepKind::Repeat { .. } => (Icon::Redo, Cat::Purple),
        StepKind::If { .. } => (Icon::Validation, Cat::Amber),
        StepKind::Stop { .. } => (Icon::StopSquare, Cat::Red),
        StepKind::Command { id, .. } => {
            let prefix = id.split('.').next().unwrap_or("");
            match prefix {
                "cell" | "range" | "edit" => (Icon::Sheet, Cat::Blue),
                "selection" => (Icon::Find, Cat::Slate),
                "home" => (Icon::Format, Cat::Orange),
                "data" | "table" | "pivot" => (Icon::SortFilter, Cat::Green),
                "insert" | "chart" | "object" => (Icon::Insert, Cat::Purple),
                "formulas" => (Icon::Function, Cat::Teal),
                "sheet" | "view" | "pageLayout" => (Icon::Sheet, Cat::Amber),
                _ => (Icon::Script, Cat::Slate),
            }
        }
    }
}

/// Problems the editor can see without running: an unknown command, one a macro may not run,
/// one that needs a permission the macro doesn't list, a Repeat or If with nothing to work on.
pub fn static_error(m: &Macro, st: &Step) -> Option<String> {
    match &st.kind {
        StepKind::Command { id, params } => {
            if gridcraft_engine::find_command(id).is_none() {
                return Some(format!("There is no command “{id}”."));
            }
            match macro_policy(id, params, gridcraft_engine::Trust::Local) {
                Policy::Deny(why) => Some(format!("{id} can't run in a macro: {why}.")),
                Policy::NeedsPermission(what) if !m.permissions.iter().any(|p| p == id) => {
                    Some(format!("{id} {what}: the macro must list it in its permissions."))
                }
                _ => None,
            }
        }
        StepKind::Repeat { over, .. } if over.trim().is_empty() => Some("Choose the range to repeat over.".into()),
        StepKind::If { cell, .. } if cell.trim().is_empty() => Some("Choose the cell to test.".into()),
        _ => None,
    }
}

/// The parameters a new step starts with, from the command's documentation: the required ones,
/// plus an optional `cell` or `range` (`{cell?, input, array?}` → cell, input).
pub fn required_keys(doc: &str) -> Vec<String> {
    let Some(start) = doc.find('{') else { return vec![] };
    let mut depth = 0i32;
    let mut quoted = false;
    let mut part = String::new();
    let mut parts = Vec::new();
    for c in doc.get(start + 1..).unwrap_or("").chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' if depth == 0 => {
                parts.push(std::mem::take(&mut part));
                break;
            }
            '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut part));
                continue;
            }
            _ => {}
        }
        if depth == 0 {
            part.push(c);
        }
    }
    parts
        .iter()
        .filter_map(|p| {
            let p = p.trim();
            let key: String = p.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let rest = p.get(key.len()..).unwrap_or("").trim_start();
            let optional = rest.starts_with('?') || rest.starts_with('|');
            (!key.is_empty() && (!optional || key == "cell" || key == "range")).then_some(key)
        })
        .take(8)
        .collect()
}

// ---------------------------------------------------------------- tokens

/// The token for `target` relative to the input called `name` at `input`: `Input`, `Input.col(2)`,
/// `Input.rows(2..3)`, `Input.cell(1,4)`, `Input.offset(0,4)`, or a cell offset from the input's
/// first cell (`Input.cell(1,1).offset(-1,0)`) with a `resize` when it is larger than one cell.
pub fn token_for(name: &str, input: RangeRef, target: RangeRef) -> String {
    let (i, r) = (input, target);
    if r == i {
        return name.to_string();
    }
    let dr = i64::from(r.start.row) - i64::from(i.start.row);
    let dc = i64::from(r.start.col) - i64::from(i.start.col);
    let (h, w) = (r.height(), r.width());
    if h == i.height() && w == i.width() {
        return format!("{name}.offset({dr},{dc})");
    }
    let inside = i.contains(r.start) && i.contains(r.end);
    if inside {
        let (r0, r1) = (dr + 1, dr + i64::from(h));
        let (c0, c1) = (dc + 1, dc + i64::from(w));
        if r.start.row == i.start.row && r.end.row == i.end.row {
            return if w == 1 { format!("{name}.col({c0})") } else { format!("{name}.cols({c0}..{c1})") };
        }
        if r.start.col == i.start.col && r.end.col == i.end.col {
            return if h == 1 { format!("{name}.row({r0})") } else { format!("{name}.rows({r0}..{r1})") };
        }
        let cell = format!("{name}.cell({r0},{c0})");
        return if r.is_single() { cell } else { format!("{cell}.resize({h},{w})") };
    }
    let cell = format!("{name}.cell(1,1).offset({dr},{dc})");
    if r.is_single() { cell } else { format!("{cell}.resize({h},{w})") }
}

/// `Sheet1!$B$2:$D$9,C4` → [(Some("Sheet1"), B2:D9), (None, C4)].
pub fn parse_refs(text: &str) -> Vec<(Option<String>, RangeRef)> {
    text.split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (sheet, body) = match part.rsplit_once('!') {
                Some((s, b)) => (Some(s.trim().trim_matches('\'').replace("''", "'")), b),
                None => (None, part),
            };
            RangeRef::parse(&body.replace('$', "")).map(|r| (sheet, r))
        })
        .take(500)
        .collect()
}

/// The inputs bound to their examples, for showing tokens as A1 and A1 as tokens.
fn bound_inputs(wb: &Workbook, ed: &Editor) -> Vec<(String, Place, bool)> {
    let env = Env { wb, active_sheet: wb.active_sheet, inputs: &[], output: None, item: None };
    ed.draft
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(k, i)| {
            let example = if k == 0 { Some(ed.example.as_str()) } else { i.default_ref.as_deref() };
            let place = resolve::resolve_ref(example?, &env).ok()?;
            Some((i.name.clone(), place, i.header))
        })
        .collect()
}

/// The suggestions offered beside a reference box.
fn token_suggestions(wb: &Workbook, ed: &Editor, in_repeat: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (k, i) in ed.draft.inputs.iter().enumerate() {
        let n = &i.name;
        out.push(n.clone());
        if i.header {
            out.push(format!("{n}.data"));
        }
        let width = if k == 0 { parse_refs(&ed.example).first().map_or(3, |(_, r)| r.width()) } else { 3 }.clamp(1, 6);
        out.extend((1..=width).map(|c| format!("{n}.col({c})")));
        out.push(format!("{n}.rows(2..)"));
        out.push(format!("{n}.cell(1,1)"));
        out.push(format!("{n}.right"));
        out.push(format!("{n}.below"));
    }
    if ed.draft.output.anchor.is_some() {
        out.push("Output".into());
        out.push("Output.offset(1,0)".into());
    }
    if in_repeat {
        out.push("Row".into());
        out.push("Row.right".into());
    }
    out.extend(wb.names.iter().take(20).map(|n| format!("Name:{}", n.name)));
    out
}

// ---------------------------------------------------------------- opening

/// Opens the editor: `{macro}` edits a saved macro (`debug: true` also starts debugging it,
/// `check: true` checks it), `{new}` starts one with that name, `{}` one with the next free name.
pub fn open(app: &mut SheetApp, params: &Json) -> Option<Dialog> {
    let doc = app.session.active()?;
    let wb = &doc.wb;
    let selection = doc.selection.current().a1();
    let found = params["macro"].as_str().and_then(|n| gridcraft_engine::macros::find(wb, n)).and_then(|i| wb.macros.get(i)).cloned();
    let (draft, saved_as) = match found {
        Some(m) => {
            let name = m.name.clone();
            (m, Some(name))
        }
        None => {
            let name = params["new"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| gridcraft_engine::macros::unique_name(wb, n))
                .unwrap_or_else(|| gridcraft_engine::macros::next_name(wb));
            (Macro { name, inputs: vec![NamedInput::default()], ..Default::default() }, None)
        }
    };
    let example = draft.inputs.first().and_then(|i| i.default_ref.clone()).unwrap_or(selection);
    let start_debug = params["debug"].as_bool() == Some(true) && saved_as.is_some();
    let title = format!("Macro: {}", draft.name);
    let selected = (!draft.steps.is_empty()).then(|| "1".to_string());
    app.macros.editor = Some(Editor {
        draft,
        dirty: saved_as.is_none(),
        saved_as,
        selected,
        tab: Tab::Step,
        input: 0,
        picker: None,
        check: None,
        debug: None,
        outcome: None,
        errors: BTreeMap::new(),
        ask_stop: false,
        closing: false,
        ask_close: false,
        start_debug,
        start_check: params["check"].as_bool() == Some(true),
        example,
        new_param: Default::default(),
        refs: RefPick::default(),
    });
    Some(Dialog::custom("macroEditor", &title, json!({})))
}

// ---------------------------------------------------------------- actions

/// Saves the draft (renaming the saved macro if its name changed). False when it couldn't.
pub fn save(app: &mut SheetApp, ed: &mut Editor) -> bool {
    ed.draft.name = ed.draft.name.trim().to_string();
    if let Err(e) = ed.draft.check_limits() {
        app.message = Some(("Save Macro".into(), e));
        return false;
    }
    if let Some(old) = ed.saved_as.clone()
        && !old.eq_ignore_ascii_case(&ed.draft.name)
    {
        if let Err(e) = app.run("macro.rename", json!({"name": old, "to": ed.draft.name})) {
            app.message = Some(("Save Macro".into(), crate::clean_error(&e)));
            return false;
        }
        // The workbook has it under the new name now, even if the save below fails.
        ed.saved_as = Some(ed.draft.name.clone());
    }
    // A new draft never replaces a macro that has its name.
    let replace = ed.saved_as.is_some();
    match app.run("macro.save", json!({"macro": gridcraft_engine::macros::macro_json(&ed.draft), "replace": replace})) {
        Ok(_) => {
            ed.saved_as = Some(ed.draft.name.clone());
            ed.dirty = false;
            true
        }
        Err(e) => {
            app.message = Some(("Save Macro".into(), crate::clean_error(&e)));
            false
        }
    }
}

fn ensure_saved(app: &mut SheetApp, ed: &mut Editor) -> bool {
    if ed.dirty || ed.saved_as.is_none() { save(app, ed) } else { true }
}

/// The inputs a run from the editor uses: the first input's example.
fn run_inputs(ed: &Editor) -> Json {
    match ed.draft.inputs.first() {
        Some(i) if !ed.example.trim().is_empty() => json!({ i.name.clone(): ed.example }),
        _ => json!({}),
    }
}

/// The step path an error message names (`Step 3 (cell.set): …` → `3`).
fn error_step(msg: &str) -> Option<(String, String)> {
    let rest = msg.get(msg.find("Step ")? + 5..)?;
    let end = rest.find([' ', ':']).unwrap_or(rest.len());
    let path = rest.get(..end)?.to_string();
    split_path(&path)?;
    Some((path, msg.to_string()))
}

/// Check: validates the inputs and dry-runs the macro; the grid shows what it would touch.
pub fn check(app: &mut SheetApp, ed: &mut Editor) {
    if !ensure_saved(app, ed) {
        return;
    }
    ed.errors.clear();
    let name = ed.draft.name.clone();
    match app.run("macro.check", json!({"name": name, "inputs": run_inputs(ed)})) {
        Ok(v) => {
            for e in v["errors"].as_array().into_iter().flatten().filter_map(Json::as_str) {
                if let Some((path, msg)) = error_step(e) {
                    ed.errors.insert(path, msg);
                }
            }
            if let Some(e) = v["footprint"]["error"].as_object()
                && let Some(step) = e.get("step").and_then(Json::as_str)
            {
                ed.errors.insert(step.to_string(), e.get("message").and_then(Json::as_str).unwrap_or("").to_string());
            }
            ed.outcome = Some(check_text(&v));
            ed.check = Some(v);
        }
        Err(e) => {
            ed.check = None;
            ed.outcome = Some(crate::clean_error(&e));
        }
    }
}

fn check_text(v: &Json) -> String {
    if let Some(errors) = v["errors"].as_array().filter(|e| !e.is_empty()) {
        return errors.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" ");
    }
    if let Some(asks) = v["needsApproval"].as_array() {
        return format!("Needs approval before it can run: it {}.", asks.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(", "));
    }
    let cells: u64 = v["footprint"]["sheets"].as_array().into_iter().flatten().filter_map(|s| s["changed"].as_u64()).sum();
    let conflicts = v["conflicts"].as_u64().unwrap_or(0);
    let mut text = format!("Check passed: writes {cells} cell{}", if cells == 1 { "" } else { "s" });
    if conflicts > 0 {
        text.push_str(&format!(", {conflicts} of them hold data (red)"));
    } else {
        text.push_str(", over no data");
    }
    if v["footprint"]["partial"].is_object() {
        text.push_str(". Later steps can't be previewed.");
    }
    text
}

/// Run: runs the saved macro as one undo step.
pub fn run(app: &mut SheetApp, ed: &mut Editor) {
    if !ensure_saved(app, ed) {
        return;
    }
    ed.errors.clear();
    let name = ed.draft.name.clone();
    match macro_ui::call(app, "Run Macro", "macro.run", json!({"name": name, "inputs": run_inputs(ed)})) {
        Some(v) => ed.outcome = Some(format!("Ran {} step{}.", v["ran"].as_u64().unwrap_or(0), if v["ran"] == 1 { "" } else { "s" })),
        None => {
            if let Some((_, text)) = app.message.as_ref()
                && let Some((path, msg)) = error_step(text)
            {
                ed.errors.insert(path, msg);
            }
        }
    }
}

/// The breakpoints sent with each debugger action: the draft's (they replace the saved ones, so
/// a dot cleared while paused takes effect at once).
fn bp_params(ed: &Editor) -> Json {
    let mut list = Vec::new();
    breakpoints(&ed.draft.steps, "", &mut list);
    json!({"breakpoints": list, "breakpointsOnly": true})
}

fn with(mut base: Json, extra: Json) -> Json {
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

/// Step, Continue or Run to Here: starts debugging if needed (paused before step 1), then acts.
pub fn debug_action(app: &mut SheetApp, ed: &mut Editor, action: &str) {
    let debugging = app.session.macro_debug.is_some();
    // Starting selects the first step: take Run to Here's target before that.
    let target = ed.selected.clone().filter(|s| split_path(s).is_some());
    if !debugging {
        if !ensure_saved(app, ed) {
            return;
        }
        ed.errors.clear();
        // A dry run first, so the grid shows the inputs and what the run will write.
        check(app, ed);
        let name = ed.draft.name.clone();
        let p = with(json!({"name": name, "inputs": run_inputs(ed), "action": "start"}), bp_params(ed));
        let Some(v) = macro_ui::call(app, "Debug Macro", "macro.debug", p) else { return };
        apply_debug(ed, &v);
        if action == "step" || v["state"] != "paused" {
            return;
        }
    }
    let p = match action {
        "runTo" => match target {
            Some(step) => json!({"action": "runTo", "step": step}),
            None => return,
        },
        other => json!({"action": other}),
    };
    if let Some(v) = macro_ui::call(app, "Debug Macro", "macro.debug", with(p, bp_params(ed))) {
        apply_debug(ed, &v);
    }
}

/// Stop: ends the debugged run, keeping its changes as one undo step or undoing them.
pub fn stop(app: &mut SheetApp, ed: &mut Editor, keep: bool) {
    if let Some(v) = macro_ui::call(app, "Debug Macro", "macro.debug", json!({"action": "stop", "keep": keep})) {
        apply_debug(ed, &v);
    }
}

/// Takes in a `macro.debug` result: paused (select the current step), or ended (say how).
pub fn apply_debug(ed: &mut Editor, v: &Json) {
    match v["state"].as_str() {
        Some("paused") => {
            if let Some(step) = v["current"]["step"].as_str() {
                ed.selected = Some(step.to_string());
                ed.tab = Tab::Step;
            }
            ed.outcome = None;
            ed.debug = Some(v.clone());
        }
        Some(state) => {
            ed.debug = None;
            let executed = v["executed"].as_u64().unwrap_or(0);
            ed.outcome = Some(match state {
                "finished" => format!("Finished: {executed} step{} ran, one undo step.", if executed == 1 { "" } else { "s" }),
                "failed" => {
                    if let Some(step) = v["error"]["step"].as_str() {
                        let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
                        ed.errors.insert(step.to_string(), msg);
                        ed.selected = Some(step.to_string());
                    }
                    v["message"].as_str().unwrap_or("The macro failed; nothing was changed.").to_string()
                }
                _ if v["kept"] == true => format!("Stopped after {executed} step{}; the changes were kept.", if executed == 1 { "" } else { "s" }),
                _ => match v["stopped"].as_str() {
                    Some(msg) => format!("Stopped: {msg}"),
                    None => "Stopped; the changes were undone.".into(),
                },
            });
        }
        None => {}
    }
}

// ---------------------------------------------------------------- the window

/// Draws the editor window. `open` turns false when it closes.
pub fn window(app: &mut SheetApp, ctx: &egui::Context, d: &mut Dialog, open: &mut bool) {
    let Some(mut ed) = app.macros.editor.take() else {
        *open = false;
        return;
    };
    // A prompt's button re-ran a debugger action (making room for the output, approving).
    if let Some(v) = app.macros.debug_result.take() {
        apply_debug(&mut ed, &v);
    }
    if ed.start_check {
        ed.start_check = false;
        check(app, &mut ed);
    }
    if ed.start_debug {
        ed.start_debug = false;
        debug_action(app, &mut ed, "step");
    }
    // The run ended elsewhere (the status bar, an agent).
    if ed.debug.is_some() && app.session.macro_debug.is_none() {
        ed.debug = None;
    }
    let dirty_mark = if ed.dirty { " •" } else { "" };
    let screen = ctx.content_rect();
    let mut win_open = true;
    egui::Window::new(format!("Macro: {}{dirty_mark}", ed.draft.name))
        .id(Id::new("gridcraft.macro_editor"))
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(840.0, 560.0))
        .min_size(vec2(660.0, 380.0))
        // To the right, so the data the overlays mark (usually top left) stays in view.
        .default_pos(pos2((screen.right() - 860.0).max(screen.left() + 20.0), screen.top() + 190.0))
        .open(&mut win_open)
        .show(ctx, |ui| {
            if let Some(key) = ed.refs.collapsed.clone() {
                ref_edit(app, ui, d, &mut ed.refs, &key, 340.0);
                apply_ref_value(&mut ed, d, &key);
                return;
            }
            body(app, ui, d, &mut ed);
        });
    if !win_open {
        if app.session.macro_debug.is_some() {
            ed.ask_stop = true;
            ed.closing = true;
        } else if ed.dirty {
            ed.ask_close = true;
        } else {
            *open = false;
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && ed.picker.is_some() {
        ed.picker = None;
    }
    prompts(app, ctx, &mut ed, open);
    app.grid.macro_overlay = if *open { overlay(app, &ed) } else { None };
    if *open {
        app.macros.editor = Some(ed);
    } else {
        restore_home(app, &ed.refs);
    }
}

// ---------------------------------------------------------------- reference boxes

/// A range as an absolute reference: `$B$2:$D$9`.
fn abs(r: &RangeRef) -> String {
    let a = |c: gridcraft_engine::core::CellRef| format!("${}${}", gridcraft_engine::core::col_to_letters(c.col), c.row + 1);
    if r.is_single() { a(r.start) } else { format!("{}:{}", a(r.start), a(r.end)) }
}

/// The grid selection as a reference: absolute, comma-joined areas, with the sheet name when it
/// isn't the sheet picking started on.
fn selection_ref(app: &SheetApp, home_sheet: Option<usize>) -> String {
    let Some(doc) = app.session.active() else { return String::new() };
    let areas: Vec<String> = doc.selection.ranges.iter().map(abs).collect();
    match (home_sheet, doc.wb.active()) {
        (Some(h), Some(sh)) if h != doc.wb.active_sheet => {
            let q = gridcraft_engine::formula::quote_sheet(&sh.name);
            areas.iter().map(|a| format!("{q}!{a}")).collect::<Vec<_>>().join(",")
        }
        _ => areas.join(","),
    }
}

/// Starts picking into `key`: later grid selections are written into the box. The first pick
/// remembers the selection, so closing the editor can put it back.
fn begin_pick(app: &SheetApp, refs: &mut RefPick, key: &str) {
    if refs.home.is_none()
        && let Some(doc) = app.session.active()
    {
        let ranges: Vec<String> = doc.selection.ranges.iter().map(|r| r.a1()).collect();
        refs.home = Some((doc.wb.active_sheet, ranges.join(","), doc.selection.active.a1()));
    }
    refs.pick = Some(key.to_string());
    refs.seen = selection_ref(app, refs.home.as_ref().map(|h| h.0));
}

/// Puts back the sheet and selection from before picking started.
fn restore_home(app: &mut SheetApp, refs: &RefPick) {
    if let Some((sheet, ranges, active)) = &refs.home {
        let _ = app.session.run("selection.set", json!({"sheet": sheet, "range": ranges, "active": active}));
    }
}

/// A reference box (Excel's RefEdit): a text field that grid selections fill while it is the
/// pick target, with a button that collapses the window to just this box and expands it again.
/// The text is kept in `d.values[key]`.
fn ref_edit(app: &SheetApp, ui: &mut Ui, d: &mut Dialog, refs: &mut RefPick, key: &str, width: f32) {
    let mut s = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
    let sel = selection_ref(app, refs.home.as_ref().map(|h| h.0));
    let picking = refs.pick.as_deref() == Some(key);
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(width - 30.0).id_salt(("refedit", key)));
        if r.gained_focus() || (r.clicked() && !picking) {
            begin_pick(app, refs, key);
        } else if picking && !r.has_focus() && sel != refs.seen {
            // A selection made on the grid becomes the reference.
            s = sel.clone();
            refs.seen = sel;
        }
        let collapsed = refs.collapsed.as_deref() == Some(key);
        let t = Tokens::get(ui.ctx());
        let (icon, tip) = if collapsed { (Icon::Expand, "Expand the editor") } else { (Icon::Collapse, "Collapse the editor to this box") };
        if crate::widgets::icon_button(ui, icon, t.text_dim, tip, vec2(22.0, 20.0)).clicked() {
            refs.collapsed = if collapsed { None } else { Some(key.to_string()) };
            if !collapsed {
                begin_pick(app, refs, key);
            }
        }
    });
    d.values.insert(key.into(), json!(s));
}

/// Keep changes? (stopping a debugged run) and Save changes? (closing with unsaved edits).
fn prompts(app: &mut SheetApp, ctx: &egui::Context, ed: &mut Editor, open: &mut bool) {
    if ed.ask_stop {
        let mut answer: Option<Option<bool>> = None;
        egui::Modal::new(Id::new("gridcraft.macro_editor.stop")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(egui::RichText::new("Stop debugging").strong());
            ui.label("Keep the changes the macro has made so far? They are one undo step either way.");
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(None);
                }
                if ui.button("Undo Changes").clicked() {
                    answer = Some(Some(false));
                }
                if ui.button("Keep Changes").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    answer = Some(Some(true));
                }
            });
        });
        match answer {
            Some(Some(keep)) => {
                ed.ask_stop = false;
                stop(app, ed, keep);
                if ed.closing {
                    if ed.dirty { ed.ask_close = true } else { *open = false }
                }
                ed.closing = false;
            }
            Some(None) => {
                ed.ask_stop = false;
                ed.closing = false;
            }
            None => {}
        }
        return;
    }
    if ed.ask_close {
        let mut answer: Option<Option<bool>> = None;
        egui::Modal::new(Id::new("gridcraft.macro_editor.close")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(egui::RichText::new(format!("Save changes to macro “{}”?", ed.draft.name)).strong());
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(None);
                }
                if ui.button("Don't Save").clicked() {
                    answer = Some(Some(false));
                }
                if ui.button("Save").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    answer = Some(Some(true));
                }
            });
        });
        match answer {
            Some(Some(true)) => {
                ed.ask_close = false;
                if save(app, ed) {
                    *open = false;
                }
            }
            Some(Some(false)) => {
                ed.ask_close = false;
                *open = false;
            }
            Some(None) => ed.ask_close = false,
            None => {}
        }
    }
}

fn body(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor) {
    let t = Tokens::get(ui.ctx());
    let debugging = app.session.macro_debug.is_some();
    // Name, description, Save.
    ui.horizontal(|ui| {
        ui.label("Name:");
        let name = ui.add_enabled(!debugging, egui::TextEdit::singleline(&mut ed.draft.name).desired_width(180.0).id_salt("me_name"));
        ui.label("Description:");
        let desc = ui.add_enabled(
            !debugging,
            egui::TextEdit::singleline(&mut ed.draft.description).desired_width((ui.available_width() - 90.0).max(80.0)).id_salt("me_desc"),
        );
        if name.changed() || desc.changed() {
            ed.dirty = true;
        }
        if ui.add_enabled(ed.dirty && !debugging, egui::Button::new("Save")).clicked() {
            save(app, ed);
        }
    });
    ui.add_space(2.0);
    toolbar(app, ui, ed, debugging);
    status_line(app, ui, ed, &t);
    ui.separator();
    let avail = ui.available_size();
    let left_w = (avail.x * 0.52).max(300.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(left_w, avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(vec2(left_w, avail.y));
            if ed.picker.is_some() {
                picker(ui, ed);
            } else {
                step_list(app, ui, ed, debugging);
            }
        });
        ui.separator();
        ui.allocate_ui_with_layout(vec2(ui.available_width(), avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical().id_salt("me_inspector").auto_shrink([false, false]).show(ui, |ui| {
                inspector(app, ui, d, ed, debugging);
            });
        });
    });
}

fn toolbar(app: &mut SheetApp, ui: &mut Ui, ed: &mut Editor, debugging: bool) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let editing = !debugging;
        if ui.add_enabled_ui(editing, |ui| small_button(ui, Icon::Plus, "Add Step", "Add a step after the selected one", false)).inner.clicked() {
            ed.picker = Some(Picker::default());
        }
        let has_step = ed.selected.as_deref().and_then(split_path).is_some();
        if ui
            .add_enabled_ui(editing && has_step, |ui| small_button(ui, Icon::Copy, "Duplicate", "Duplicate the selected step", false))
            .inner
            .clicked()
        {
            duplicate(ed);
        }
        if ui.add_enabled_ui(editing && has_step, |ui| small_button(ui, Icon::Delete, "Delete", "Delete the selected step", false)).inner.clicked() {
            delete(ed);
        }
        sep(ui, &t);
        if ui
            .add_enabled_ui(editing, |ui| {
                small_button(ui, Icon::Check, "Check", "Check the inputs and show on the grid what the macro would change", false)
            })
            .inner
            .clicked()
        {
            check(app, ed);
        }
        if ui.add_enabled_ui(editing, |ui| small_button(ui, Icon::Play, "Run", "Run the macro as one undo step", false)).inner.clicked() {
            run(app, ed);
        }
        sep(ui, &t);
        if small_button(ui, Icon::Step, "Step", if debugging { "Run the current step" } else { "Start debugging: stop before the first step" }, false)
            .clicked()
        {
            debug_action(app, ed, "step");
        }
        if small_button(ui, Icon::Continue, "Continue", "Run to the next breakpoint or the end", false).clicked() {
            debug_action(app, ed, "continue");
        }
        let can_run_to = ed.selected.as_deref().and_then(split_path).is_some();
        if ui.add_enabled_ui(can_run_to, |ui| small_button(ui, Icon::RunTo, "Run to Here", "Run until the selected step", false)).inner.clicked() {
            debug_action(app, ed, "runTo");
        }
        if ui.add_enabled_ui(debugging, |ui| small_button(ui, Icon::StopSquare, "Stop", "Stop debugging", false)).inner.clicked() {
            ed.ask_stop = true;
        }
    });
}

fn sep(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 2.0), pos2(r.center().x, r.bottom() - 2.0)], Stroke::new(1.0, t.separator));
}

/// Where the run is (while debugging) or what the last check, run or debug session said, with
/// the overlay legend.
fn status_line(app: &SheetApp, ui: &mut Ui, ed: &Editor, t: &Tokens) {
    if let Some(st) = &ed.debug {
        let cur = &st["current"];
        let step = cur["step"].as_str().unwrap_or("");
        let what = match cur["kind"].as_str() {
            Some("command") => command_label(cur["command"].as_str().unwrap_or("")),
            Some(k) => k.to_string(),
            None => String::new(),
        };
        let mut head = format!("Paused before step {step} · {what}");
        if let (Some(at), Some(of)) = (cur["iteration"]["at"].as_u64(), cur["iteration"]["of"].as_u64()) {
            head.push_str(&format!(" · pass {at} of {of}"));
        }
        if st["atBreakpoint"] == true {
            head.push_str(" · at a breakpoint");
        }
        ui.label(egui::RichText::new(head).strong().color(t.text));
        let resolved = cur.get("params").or(cur.get("over")).or(cur.get("cell")).map(|p| p.to_string());
        if let Some(p) = resolved {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Runs with:").color(t.text_dim));
                ui.add(egui::Label::new(egui::RichText::new(p).monospace().color(t.text)).truncate());
            });
        } else if let Some(e) = cur["paramsError"].as_str() {
            ui.label(egui::RichText::new(e).color(t.danger));
        }
        let changed: Vec<&str> = st["changed"].as_array().into_iter().flatten().filter_map(Json::as_str).take(6).collect();
        ui.label(
            egui::RichText::new(format!("Changed so far: {}", if changed.is_empty() { "nothing".into() } else { changed.join(", ") }))
                .color(t.text_dim),
        );
    } else if let Some(text) = &ed.outcome {
        let bad = !ed.errors.is_empty() || ed.check.as_ref().is_some_and(|c| c["ok"] == false);
        ui.add(egui::Label::new(egui::RichText::new(text).color(if bad { t.danger } else { t.text })).wrap());
    } else {
        ui.label(
            egui::RichText::new("Check shows on the grid what the macro reads and writes. Click a step's gutter to set a breakpoint.")
                .color(t.text_dim),
        );
    }
    if app.grid.macro_overlay.is_some() {
        ui.horizontal(|ui| {
            for (label, c) in [
                ("Inputs", t.sheet_macro_input),
                ("Writes", t.sheet_macro_footprint),
                ("Writes over data", t.sheet_macro_conflict),
                ("Current step", t.sheet_macro_target),
            ] {
                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, c);
                ui.label(egui::RichText::new(label).small().color(t.text_dim));
                ui.add_space(6.0);
            }
        });
    }
}

// ---------------------------------------------------------------- the step list

/// A step being dragged: the list it is in and its place there.
#[derive(Clone, Debug)]
struct StepDrag {
    parent: String,
    index: usize,
    label: String,
}

fn step_list(app: &mut SheetApp, ui: &mut Ui, ed: &mut Editor, debugging: bool) {
    let t = Tokens::get(ui.ctx());
    let mut list = Vec::new();
    rows(&ed.draft.steps, "", 0, &mut list);
    if list.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("No steps yet. Add Step picks a command, or record one with Automate › Record Macro.").color(t.text_dim));
        return;
    }
    let current = ed.debug.as_ref().and_then(|s| s["current"]["step"].as_str().map(str::to_string));
    let dragging = egui::DragAndDrop::payload::<StepDrag>(ui.ctx());
    let pointer = ui.ctx().pointer_latest_pos();
    let mut placed: Vec<(String, Rect)> = Vec::new();
    egui::ScrollArea::vertical().id_salt("me_steps").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for row in &list {
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click_and_drag());
            placed.push((row.parent.clone(), rect));
            let selected = ed.selected.as_deref() == Some(row.key.as_str());
            let is_current = current.as_deref() == Some(row.key.as_str());
            let p = ui.painter();
            if is_current {
                p.rect_filled(rect, 3.0, t.accent_soft);
            } else if selected {
                p.rect_filled(rect, 3.0, t.pressed);
            } else if resp.hovered() {
                p.rect_filled(rect, 3.0, t.hover);
            }
            if resp.clicked() {
                ed.selected = Some(row.key.clone());
                ed.tab = Tab::Step;
            }
            let indent = row.depth as f32 * 16.0;
            if let Some(label) = row.branch {
                p.text(pos2(rect.left() + 58.0 + indent, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::ui_bold(12.0), t.text_dim);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{label} branch {}", row.parent)));
                continue;
            }
            let Some(st) = step_at(&ed.draft.steps, &row.key) else { continue };
            // Gutter: the breakpoint dot (click to toggle), the current-step arrow.
            let gutter = Rect::from_min_size(rect.min, vec2(18.0, rect.height()));
            let g = ui.interact(gutter, Id::new(("me_bp", &row.key)), Sense::click());
            if st.breakpoint {
                p.circle_filled(gutter.center(), 5.0, t.breakpoint);
            } else if g.hovered() {
                p.circle_stroke(gutter.center(), 4.5, Stroke::new(1.0, t.breakpoint));
            }
            if is_current {
                let c = pos2(gutter.right() + 1.0, gutter.center().y);
                p.add(egui::Shape::convex_polygon(vec![c + vec2(-6.0, -5.0), c + vec2(1.0, 0.0), c + vec2(-6.0, 5.0)], t.accent, Stroke::NONE));
            }
            g.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, st.breakpoint, format!("Breakpoint {}", row.key)));
            if g.on_hover_text("Breakpoint: the debugger stops before this step").clicked()
                && let Some(s) = step_mut(&mut ed.draft.steps, &row.key)
            {
                s.breakpoint = !s.breakpoint;
                ed.dirty = true;
            }
            p.text(pos2(rect.left() + 48.0, rect.center().y), egui::Align2::RIGHT_CENTER, &row.key, theme::ui_font(11.5), t.text_dim);
            let (icon, cat) = category(&st);
            icons::paint(p, Rect::from_center_size(pos2(rect.left() + 62.0 + indent, rect.center().y), vec2(16.0, 16.0)), icon, t.cat(cat));
            let text = summary(&st);
            let text_color = if st.enabled { t.text } else { t.text_disabled };
            let text_left = rect.left() + 76.0 + indent;
            let text_clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2(rect.right() - 46.0, rect.bottom()));
            p.with_clip_rect(text_clip).text(pos2(text_left, rect.center().y), egui::Align2::LEFT_CENTER, &text, theme::ui_font(12.5), text_color);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Step {}: {text}", row.key)));
            // Error marker.
            let error = ed.errors.get(&row.key).cloned().or_else(|| static_error(&ed.draft, &st));
            if let Some(err) = error {
                let c = pos2(rect.right() - 36.0, rect.center().y);
                p.circle_filled(c, 7.0, t.danger);
                p.text(c, egui::Align2::CENTER_CENTER, "!", theme::ui_bold(11.0), t.window);
                ui.interact(Rect::from_center_size(c, vec2(16.0, 16.0)), Id::new(("me_err", &row.key)), Sense::hover()).on_hover_text(err);
            }
            // Enabled.
            let mut on = st.enabled;
            let cb =
                ui.put(Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(18.0, 18.0)), egui::Checkbox::without_text(&mut on));
            if cb.on_hover_text("Run this step (a disabled step is skipped)").changed()
                && let Some(s) = step_mut(&mut ed.draft.steps, &row.key)
            {
                s.enabled = on;
                ed.dirty = true;
            }
            if !debugging {
                dnd::drag_source(ui, &resp, StepDrag { parent: row.parent.clone(), index: row.index, label: text.clone() });
            }
        }
    });
    // Dropping a dragged step among its own list's rows.
    if let (Some(drag), Some(p)) = (dragging, pointer) {
        let group: Vec<Rect> =
            list.iter().zip(&placed).filter(|(r, _)| r.branch.is_none() && r.parent == drag.parent).map(|(_, (_, rect))| *rect).collect();
        let zone = group.iter().fold(Rect::NOTHING, |a, r| a.union(*r));
        let area = placed.iter().fold(Rect::NOTHING, |a, (_, r)| a.union(*r)).expand(12.0);
        dnd::ghost(ui, &drag.label, p);
        if area.contains(p) && zone.is_positive() {
            dnd::drop_marker(ui, zone, &group, p.y);
        }
        if ui.input(|i| i.pointer.any_released()) {
            let _ = egui::DragAndDrop::take_payload::<StepDrag>(ui.ctx());
            if area.contains(p) {
                let to = dnd::insert_index(&group, p.y);
                if to != drag.index
                    && to != drag.index + 1
                    && let Some(path) = move_step(&mut ed.draft.steps, &drag.parent, drag.index, to)
                {
                    ed.selected = Some(path);
                    ed.errors.clear();
                    ed.dirty = true;
                }
            }
        }
    }
    let _ = app;
}

/// Where a new step goes: after the selected step in its list, at the end of a selected branch
/// or Repeat, or at the end of the macro.
fn insertion_point(ed: &Editor) -> (String, usize) {
    let Some(sel) = ed.selected.as_deref() else { return (String::new(), ed.draft.steps.len()) };
    let mut steps = ed.draft.steps.clone();
    if split_path(sel).is_none() {
        // A branch row: the end of that branch.
        let len = list_mut(&mut steps, sel).map_or(0, |l| l.len());
        return (sel.to_string(), len);
    }
    if let Some(st) = step_mut(&mut steps, sel)
        && let StepKind::Repeat { body, .. } = &st.kind
    {
        return (sel.to_string(), body.len());
    }
    match split_path(sel) {
        Some((parent, i)) => (parent.to_string(), i + 1),
        None => (String::new(), ed.draft.steps.len()),
    }
}

fn insert_step(ed: &mut Editor, step: Step) {
    let (parent, at) = insertion_point(ed);
    if let Some(list) = list_mut(&mut ed.draft.steps, &parent) {
        let at = at.min(list.len());
        list.insert(at, step);
        ed.selected = Some(child(&parent, at));
        ed.tab = Tab::Step;
        ed.errors.clear();
        ed.dirty = true;
    }
}

fn duplicate(ed: &mut Editor) {
    let Some(sel) = ed.selected.clone() else { return };
    let Some((parent, i)) = split_path(&sel) else { return };
    if let Some(list) = list_mut(&mut ed.draft.steps, parent)
        && let Some(st) = list.get(i).cloned()
    {
        list.insert(i + 1, st);
        ed.selected = Some(child(parent, i + 1));
        ed.errors.clear();
        ed.dirty = true;
    }
}

fn delete(ed: &mut Editor) {
    let Some(sel) = ed.selected.clone() else { return };
    let Some((parent, i)) = split_path(&sel) else { return };
    if let Some(list) = list_mut(&mut ed.draft.steps, parent)
        && i < list.len()
    {
        list.remove(i);
        ed.selected = if list.is_empty() { (!parent.is_empty()).then(|| parent.to_string()) } else { Some(child(parent, i.min(list.len() - 1))) };
        ed.errors.clear();
        ed.dirty = true;
    }
}

// ---------------------------------------------------------------- Add Step

/// The steps the picker offers for `query`: control steps first, then commands a macro may run.
pub fn picker_items(query: &str) -> Vec<(String, String, String)> {
    let q = query.trim().to_ascii_lowercase();
    let mut out: Vec<(String, String, String)> = [
        ("@repeat", "Repeat for each row", "Control: run steps once per row of a range"),
        ("@if", "If", "Control: run steps when a cell meets a condition"),
        ("@stop", "Stop with message", "Control: end the run with a message"),
    ]
    .iter()
    .filter(|(_, l, _)| q.is_empty() || l.to_ascii_lowercase().contains(&q))
    .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
    .collect();
    for c in gridcraft_engine::command_specs() {
        if matches!(macro_policy(c.id, &json!({}), gridcraft_engine::Trust::Local), Policy::Deny(_)) || c.id.starts_with("ui.") {
            continue;
        }
        if q.is_empty() || c.label.to_ascii_lowercase().contains(&q) || c.id.to_ascii_lowercase().contains(&q) {
            out.push((c.id.to_string(), c.label.to_string(), c.menu.join(" › ")));
        }
        if out.len() >= 60 {
            break;
        }
    }
    out
}

/// A new step for a picker item, with the command's required parameters filled in as blanks.
pub fn new_step(item: &str) -> Step {
    match item {
        "@repeat" => Step::new(StepKind::Repeat { over: "Input".into(), of: RepeatOf::Rows, max: 10_000, body: vec![] }),
        "@if" => {
            Step::new(StepKind::If { cell: "Input.cell(1,1)".into(), op: CompareOp::NotEmpty, value: Json::Null, then: vec![], otherwise: vec![] })
        }
        "@stop" => Step::new(StepKind::Stop { message: String::new(), rollback: false }),
        id => {
            let doc = gridcraft_engine::find_command(id).map(|c| c.params).unwrap_or("");
            let params: Map<String, Json> = required_keys(doc).into_iter().map(|k| (k, json!(""))).collect();
            Step::command(id, Json::Object(params))
        }
    }
}

fn picker(ui: &mut Ui, ed: &mut Editor) {
    let Some(pk) = ed.picker.as_mut() else { return };
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(if pk.replace.is_some() { "Change the command" } else { "Add a step" }).strong());
        if ui.small_button("Cancel").clicked() {
            pk.search = "\u{1}".into();
        }
    });
    if pk.search == "\u{1}" {
        ed.picker = None;
        return;
    }
    let r = ui.add(
        egui::TextEdit::singleline(&mut pk.search)
            .hint_text("Search commands, e.g. bold, sort, set…")
            .desired_width(f32::INFINITY)
            .id_salt("me_pick"),
    );
    r.request_focus();
    if r.changed() {
        pk.index = 0;
    }
    let mut items = picker_items(&pk.search);
    // Change swaps a command step's command; it can't turn the step into Repeat, If or Stop.
    if pk.replace.is_some() {
        items.retain(|(id, ..)| !id.starts_with('@'));
    }
    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        pk.index = (pk.index + 1).min(items.len().saturating_sub(1));
    }
    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
        pk.index = pk.index.saturating_sub(1);
    }
    let mut chosen = ui.input(|i| i.key_pressed(egui::Key::Enter)).then_some(pk.index);
    egui::ScrollArea::vertical().id_salt("me_pick_list").auto_shrink([false, false]).show(ui, |ui| {
        for (i, (id, label, menu)) in items.iter().enumerate() {
            let r = ui.add(egui::Button::selectable(i == pk.index, label.as_str()).min_size(vec2(ui.available_width(), 20.0)));
            if r.clicked() {
                chosen = Some(i);
            }
            r.on_hover_text(format!("{id}  {menu}"));
        }
        if items.is_empty() {
            ui.label(egui::RichText::new("No command matches.").color(t.text_dim));
        }
    });
    let Some(item) = chosen.and_then(|i| items.get(i)).map(|x| x.0.clone()) else { return };
    let replace = pk.replace.clone();
    ed.picker = None;
    match replace {
        Some(path) => {
            let fresh = new_step(&item);
            if let Some(st) = step_mut(&mut ed.draft.steps, &path) {
                st.kind = fresh.kind;
                ed.dirty = true;
            }
        }
        None => insert_step(ed, new_step(&item)),
    }
}

// ---------------------------------------------------------------- the inspector

fn inspector(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor, debugging: bool) {
    ui.horizontal(|ui| {
        for (tab, label) in [(Tab::Step, "Details"), (Tab::Inputs, "Inputs"), (Tab::Output, "Output")] {
            if ui.selectable_label(ed.tab == tab, label).clicked() {
                ed.tab = tab;
            }
        }
    });
    ui.separator();
    ui.add_enabled_ui(!debugging, |ui| match ed.tab {
        Tab::Step => step_tab(app, ui, d, ed),
        Tab::Inputs => inputs_tab(app, ui, d, ed),
        Tab::Output => output_tab(app, ui, d, ed),
    });
}

/// The dialog value key of a reference box.
fn ref_key(path: &str, field: &str) -> String {
    format!("ref:{path}:{field}")
}

/// After a RefEdit box changed (typed, or picked on the grid): writes it into the draft.
fn apply_ref_value(ed: &mut Editor, d: &Dialog, key: &str) {
    let Some(v) = d.values.get(key).and_then(Json::as_str).map(str::to_string) else { return };
    let mut parts = key.splitn(3, ':');
    let (_, path, field) = (parts.next(), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if set_ref(ed, path, field, &v) {
        ed.dirty = true;
    }
}

/// Writes reference text into the field a ref box edits. True when it changed.
fn set_ref(ed: &mut Editor, path: &str, field: &str, v: &str) -> bool {
    match path {
        "@output" => {
            let new = (!v.trim().is_empty()).then(|| v.trim().to_string());
            let changed = ed.draft.output.anchor != new;
            ed.draft.output.anchor = new;
            changed
        }
        "@example" => {
            let changed = ed.example != v;
            ed.example = v.to_string();
            if let Some(i) = ed.draft.inputs.first_mut()
                && i.default_ref.is_some()
            {
                i.default_ref = Some(v.to_string());
            }
            changed
        }
        _ => {
            let Some(st) = step_mut(&mut ed.draft.steps, path) else { return false };
            let slot: Option<&mut String> = match (&mut st.kind, field) {
                (StepKind::Repeat { over, .. }, "over") => Some(over),
                (StepKind::If { cell, .. }, "cell") => Some(cell),
                _ => None,
            };
            if let Some(s) = slot {
                let changed = s != v;
                *s = v.to_string();
                return changed;
            }
            if let StepKind::Command { params, .. } = &mut st.kind
                && let Some(o) = params.as_object_mut()
            {
                let changed = o.get(field).and_then(Json::as_str) != Some(v);
                o.insert(field.to_string(), json!(v));
                return changed;
            }
            false
        }
    }
}

/// A reference box: RefEdit (type, or select on the grid), Relative/Absolute, and suggestions.
fn ref_field(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor, path: &str, field: &str, value: &str, in_repeat: bool) {
    let key = ref_key(path, field);
    if ed.refs.pick.as_deref() != Some(key.as_str()) || d.values.get(&key).is_none() {
        d.values.insert(key.clone(), json!(value));
    }
    let width = (ui.available_width() - 96.0).clamp(120.0, 320.0);
    ui.horizontal(|ui| {
        ref_edit(app, ui, d, &mut ed.refs, &key, width);
        apply_ref_value(ed, d, &key);
        let current = d.values.get(&key).and_then(Json::as_str).unwrap_or("").to_string();
        let Some(doc) = app.session.active() else { return };
        let wb = doc.wb.clone();
        let inputs = bound_inputs(&wb, ed);
        let env = Env { wb: &wb, active_sheet: wb.active_sheet, inputs: &inputs, output: None, item: None };
        let relative = !current.trim().is_empty() && resolve::is_token(&current, &env);
        if path != "@example" {
            if ui.selectable_label(relative, "Rel").on_hover_text("Relative: a token that follows the input the macro runs on").clicked() && !relative
            {
                let place = resolve::resolve_ref(&current, &env).ok();
                if let (Some(p), Some((name, input, _))) = (place, inputs.first())
                    && p.sheet == input.sheet
                {
                    let tok = token_for(name, input.range, p.range);
                    set_ref(ed, path, field, &tok);
                    d.values.insert(key.clone(), json!(tok));
                    ed.dirty = true;
                }
            }
            if ui.selectable_label(!relative && !current.trim().is_empty(), "Abs").on_hover_text("Absolute: always these cells").clicked()
                && relative
                && let Ok(p) = resolve::resolve_ref(&current, &env)
            {
                let a = resolve::a1(p, &env);
                set_ref(ed, path, field, &a);
                d.values.insert(key.clone(), json!(a));
                ed.dirty = true;
            }
            let menu = ui.button("▾").on_hover_text("Tokens");
            egui::Popup::menu(&menu).show(|ui| {
                ui.set_max_height(300.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for tok in token_suggestions(&wb, ed, in_repeat) {
                        let shown =
                            resolve::resolve_ref(&tok, &env).map(|p| format!("{tok}    {}", resolve::a1(p, &env))).unwrap_or_else(|_| tok.clone());
                        if ui.button(shown).clicked() {
                            set_ref(ed, path, field, &tok);
                            d.values.insert(key.clone(), json!(tok));
                            ed.dirty = true;
                        }
                    }
                });
            });
        }
    });
    // What a token stands for on the example.
    let current = d.values.get(&key).and_then(Json::as_str).unwrap_or("").to_string();
    if let Some(doc) = app.session.active() {
        let wb = &doc.wb;
        let inputs = bound_inputs(wb, ed);
        let env = Env { wb, active_sheet: wb.active_sheet, inputs: &inputs, output: None, item: None };
        if !current.trim().is_empty() && resolve::is_token(&current, &env) && !current.starts_with("Row") && !current.starts_with("Output") {
            let t = Tokens::get(ui.ctx());
            match resolve::resolve_ref(&current, &env) {
                Ok(p) => ui.label(egui::RichText::new(format!("= {} on the example", resolve::a1(p, &env))).small().color(t.text_dim)),
                Err(e) => ui.label(egui::RichText::new(e.message()).small().color(t.danger)),
            };
        }
    }
}

fn step_tab(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor) {
    let t = Tokens::get(ui.ctx());
    let Some(path) = ed.selected.clone() else {
        ui.label(egui::RichText::new("Select a step to see its settings.").color(t.text_dim));
        return;
    };
    let Some(st) = step_at(&ed.draft.steps, &path) else {
        ui.label(
            egui::RichText::new(format!(
                "Steps here run when the If is {}. Add Step puts a step in this branch.",
                if path.ends_with("then") { "true" } else { "false" }
            ))
            .color(t.text_dim),
        );
        return;
    };
    let in_repeat = path.contains('.');
    let (icon, cat) = category(&st);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, t.cat(cat));
        let title = match &st.kind {
            StepKind::Command { id, .. } => command_label(id),
            StepKind::Repeat { .. } => "Repeat".into(),
            StepKind::If { .. } => "If".into(),
            StepKind::Stop { .. } => "Stop".into(),
        };
        ui.label(egui::RichText::new(format!("Step {path} · {title}")).strong());
        if let StepKind::Command { id, .. } = &st.kind {
            ui.label(egui::RichText::new(id).monospace().color(t.text_dim));
            if ui.small_button("Change…").clicked() {
                ed.picker = Some(Picker { replace: Some(path.clone()), ..Default::default() });
            }
        }
    });
    let mut enabled = st.enabled;
    let mut bp = st.breakpoint;
    ui.horizontal(|ui| {
        let a = ui.checkbox(&mut enabled, "Enabled").changed();
        let b = ui.checkbox(&mut bp, "Breakpoint").changed();
        if (a || b)
            && let Some(s) = step_mut(&mut ed.draft.steps, &path)
        {
            s.enabled = enabled;
            s.breakpoint = bp;
            ed.dirty = true;
        }
    });
    if let Some(e) = ed.errors.get(&path).cloned().or_else(|| static_error(&ed.draft, &st)) {
        ui.label(egui::RichText::new(e).color(t.danger));
    }
    ui.add_space(4.0);
    match st.kind.clone() {
        StepKind::Command { id, params } => command_fields(app, ui, d, ed, &path, &id, &params, in_repeat),
        StepKind::Repeat { over, of, max, .. } => {
            ui.label("For each");
            let mut of2 = of;
            ui.horizontal(|ui| {
                ui.radio_value(&mut of2, RepeatOf::Rows, "row");
                ui.radio_value(&mut of2, RepeatOf::Columns, "column");
                ui.label("of:");
            });
            ref_field(app, ui, d, ed, &path, "over", &over, in_repeat);
            let mut max2 = max;
            ui.horizontal(|ui| {
                ui.label("At most");
                ui.add(egui::DragValue::new(&mut max2).range(1..=100_000));
                ui.label("times");
            });
            if (of2 != of || max2 != max)
                && let Some(StepKind::Repeat { of, max, .. }) = step_mut(&mut ed.draft.steps, &path).map(|s| &mut s.kind)
            {
                *of = of2;
                *max = max2;
                ed.dirty = true;
            }
            ui.label(egui::RichText::new("Inside, Row (or Column) stands for the current one, e.g. Row.right.").small().color(t.text_dim));
        }
        StepKind::If { cell, op, value, .. } => {
            ui.label("If cell");
            ref_field(app, ui, d, ed, &path, "cell", &cell, in_repeat);
            let mut op2 = op;
            let ops = [
                (CompareOp::Eq, "equals"),
                (CompareOp::Ne, "doesn't equal"),
                (CompareOp::Gt, "is greater than"),
                (CompareOp::Ge, "is at least"),
                (CompareOp::Lt, "is less than"),
                (CompareOp::Le, "is at most"),
                (CompareOp::Empty, "is empty"),
                (CompareOp::NotEmpty, "isn't empty"),
                (CompareOp::Contains, "contains"),
            ];
            let label = ops.iter().find(|(o, _)| *o == op).map_or("", |x| x.1);
            egui::ComboBox::from_id_salt(("me_op", &path)).selected_text(label).show_ui(ui, |ui| {
                for (o, l) in ops {
                    ui.selectable_value(&mut op2, o, l);
                }
            });
            let mut text = match &value {
                Json::String(s) => s.clone(),
                Json::Null => String::new(),
                other => other.to_string(),
            };
            let changed_value = if matches!(op2, CompareOp::Empty | CompareOp::NotEmpty) {
                false
            } else {
                ui.add(egui::TextEdit::singleline(&mut text).hint_text("value").id_salt(("me_ifv", &path))).changed()
            };
            if (op2 != op || changed_value)
                && let Some(StepKind::If { op, value, .. }) = step_mut(&mut ed.draft.steps, &path).map(|s| &mut s.kind)
            {
                *op = op2;
                if changed_value {
                    *value = scalar(&text);
                }
                ed.dirty = true;
            }
        }
        StepKind::Stop { message, rollback } => {
            let (mut m, mut rb) = (message.clone(), rollback);
            ui.label("Message:");
            let a = ui.add(egui::TextEdit::singleline(&mut m).desired_width(f32::INFINITY).id_salt(("me_stop", &path))).changed();
            let b = ui.checkbox(&mut rb, "Undo what the macro did before stopping").changed();
            if (a || b)
                && let Some(StepKind::Stop { message, rollback }) = step_mut(&mut ed.draft.steps, &path).map(|s| &mut s.kind)
            {
                *message = m;
                *rollback = rb;
                ed.dirty = true;
            }
        }
    }
    ui.add_space(6.0);
    ui.label("Note:");
    let mut note = st.note.clone();
    if ui.add(egui::TextEdit::multiline(&mut note).desired_rows(2).desired_width(f32::INFINITY).id_salt(("me_note", &path))).changed()
        && let Some(s) = step_mut(&mut ed.draft.steps, &path)
    {
        s.note = note;
        ed.dirty = true;
    }
}

/// Typed text as a JSON value: a number, TRUE/FALSE, or text.
fn scalar(text: &str) -> Json {
    let t = text.trim();
    if let Ok(n) = t.parse::<f64>()
        && n.is_finite()
    {
        return json!(n);
    }
    match t.to_ascii_lowercase().as_str() {
        "true" => json!(true),
        "false" => json!(false),
        _ => json!(text),
    }
}

#[allow(clippy::too_many_arguments)]
fn command_fields(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor, path: &str, id: &str, params: &Json, in_repeat: bool) {
    let t = Tokens::get(ui.ctx());
    let fields: Vec<(String, Json)> = params.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    let mut remove: Option<String> = None;
    let mut set: Option<(String, Json)> = None;
    egui::Grid::new(("me_params", path)).num_columns(3).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        for (k, v) in &fields {
            ui.label(k.as_str());
            match v {
                Json::String(s) if resolve::is_ref_key(k) => {
                    ui.vertical(|ui| ref_field(app, ui, d, ed, path, k, s, in_repeat));
                }
                Json::String(s) => {
                    let mut text = s.clone();
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0).id_salt(("me_p", path, k))).changed() {
                        set = Some((k.clone(), json!(text)));
                    }
                }
                Json::Bool(b) => {
                    let mut on = *b;
                    if ui.checkbox(&mut on, "").changed() {
                        set = Some((k.clone(), json!(on)));
                    }
                }
                Json::Number(n) => {
                    let mut x = n.as_f64().unwrap_or(0.0);
                    if ui.add(egui::DragValue::new(&mut x).speed(0.1)).changed() {
                        set = Some((k.clone(), json!(x)));
                    }
                }
                other => {
                    let jk = format!("json:{path}:{k}");
                    let mut text = d.values.get(&jk).and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| other.to_string());
                    let ok = serde_json::from_str::<Json>(&text).is_ok();
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(220.0)
                            .font(egui::TextStyle::Monospace)
                            .text_color(if ok { t.text } else { t.danger })
                            .id_salt(("me_pj", path, k)),
                    );
                    if r.changed() {
                        if let Ok(v) = serde_json::from_str::<Json>(&text) {
                            set = Some((k.clone(), v));
                        }
                        d.values.insert(jk, json!(text));
                    }
                }
            }
            if crate::widgets::icon_button(ui, Icon::Close, t.text_dim, "Remove this parameter", vec2(18.0, 18.0)).clicked() {
                remove = Some(k.clone());
            }
            ui.end_row();
        }
    });
    if let Some(StepKind::Command { params, .. }) = step_mut(&mut ed.draft.steps, path).map(|s| &mut s.kind) {
        if !params.is_object() {
            *params = json!({});
        }
        if let Some(o) = params.as_object_mut() {
            if let Some((k, v)) = set {
                o.insert(k, v);
                ed.dirty = true;
            }
            if let Some(k) = remove {
                o.remove(&k);
                ed.dirty = true;
            }
        }
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut ed.new_param.0).hint_text("parameter").desired_width(90.0).id_salt("me_np_k"));
        ui.add(egui::TextEdit::singleline(&mut ed.new_param.1).hint_text("value").desired_width(140.0).id_salt("me_np_v"));
        let key = ed.new_param.0.trim().to_string();
        if ui.add_enabled(!key.is_empty(), egui::Button::new("Add")).clicked() {
            let raw = ed.new_param.1.clone();
            let v = serde_json::from_str::<Json>(&raw).unwrap_or(Json::String(raw));
            if let Some(StepKind::Command { params, .. }) = step_mut(&mut ed.draft.steps, path).map(|s| &mut s.kind)
                && let Some(o) = params.as_object_mut()
            {
                o.insert(key, v);
                ed.dirty = true;
            }
            ed.new_param = Default::default();
        }
    });
    if let Some(spec) = gridcraft_engine::find_command(id)
        && !spec.params.is_empty()
    {
        ui.add_space(4.0);
        ui.add(egui::Label::new(egui::RichText::new(spec.params).small().color(t.text_dim)).wrap());
    }
}

fn inputs_tab(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("The ranges the macro works on. Each is checked before the macro runs.").small().color(t.text_dim));
    ui.horizontal_wrapped(|ui| {
        for (k, i) in ed.draft.inputs.iter().enumerate() {
            if ui.selectable_label(ed.input == k, i.name.as_str()).clicked() {
                ed.input = k;
            }
        }
        if ui.small_button("Add").clicked() && ed.draft.inputs.len() < 64 {
            let n = (2..).map(|k| format!("Input{k}")).find(|n| !ed.draft.inputs.iter().any(|i| i.name.eq_ignore_ascii_case(n))).unwrap_or_default();
            ed.draft.inputs.push(NamedInput { name: n, ..Default::default() });
            ed.input = ed.draft.inputs.len() - 1;
            ed.dirty = true;
        }
        if ui.add_enabled(!ed.draft.inputs.is_empty(), egui::Button::new("Remove").small()).clicked() && ed.input < ed.draft.inputs.len() {
            ed.draft.inputs.remove(ed.input);
            ed.input = ed.input.saturating_sub(1);
            ed.dirty = true;
        }
    });
    let k = ed.input;
    let Some(mut i) = ed.draft.inputs.get(k).cloned() else {
        ui.label(egui::RichText::new("No inputs: the macro works on fixed cells only.").color(t.text_dim));
        return;
    };
    let before = i.clone();
    ui.separator();
    egui::Grid::new("me_input").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label("Name:");
        ui.add(egui::TextEdit::singleline(&mut i.name).desired_width(140.0).id_salt("me_in_name"));
        ui.end_row();
        ui.label("Shape:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut i.kind, InputKind::Range, "range");
            ui.radio_value(&mut i.kind, InputKind::Cell, "single cell");
        });
        ui.end_row();
        if i.kind == InputKind::Range {
            ui.label("Data rows:");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut i.rows[0]).range(0..=1_048_576).prefix("at least "));
                ui.add(
                    egui::DragValue::new(&mut i.rows[1])
                        .range(0..=1_048_576)
                        .prefix("at most ")
                        .custom_formatter(|v, _| if v == 0.0 { "any".into() } else { format!("{v}") }),
                );
            });
            ui.end_row();
            ui.label("Columns:");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut i.cols[0]).range(0..=16_384).prefix("at least "));
                ui.add(
                    egui::DragValue::new(&mut i.cols[1])
                        .range(0..=16_384)
                        .prefix("at most ")
                        .custom_formatter(|v, _| if v == 0.0 { "any".into() } else { format!("{v}") }),
                );
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut i.header, "First row holds column names");
            ui.end_row();
        }
        ui.label("");
        ui.checkbox(&mut i.non_empty, "Every cell must hold something");
        ui.end_row();
    });
    // Column types, one per column of the example (or of the list so far).
    let ncols = if k == 0 { parse_refs(&ed.example).first().map_or(0, |(_, r)| r.width() as usize) } else { 0 }.max(i.col_types.len()).min(16);
    if i.kind == InputKind::Range && ncols > 0 {
        ui.label("Each column must hold:");
        let headers = if k == 0 { example_headers(app, &ed.example, i.header) } else { vec![] };
        egui::Grid::new("me_types").num_columns(2).show(ui, |ui| {
            for c in 0..ncols {
                let label = headers.get(c).filter(|h| !h.is_empty()).cloned().unwrap_or_else(|| format!("Column {}", c + 1));
                ui.label(label);
                let mut ty = i.col_types.get(c).copied().unwrap_or(ColType::Any);
                let before_ty = ty;
                egui::ComboBox::from_id_salt(("me_ty", c)).selected_text(type_name(ty)).show_ui(ui, |ui| {
                    for x in [ColType::Any, ColType::Number, ColType::Text, ColType::Date, ColType::Bool] {
                        ui.selectable_value(&mut ty, x, type_name(x));
                    }
                });
                if ty != before_ty {
                    while i.col_types.len() <= c {
                        i.col_types.push(ColType::Any);
                    }
                    if let Some(slot) = i.col_types.get_mut(c) {
                        *slot = ty;
                    }
                }
                ui.end_row();
            }
        });
    }
    if i != before {
        if let Some(slot) = ed.draft.inputs.get_mut(k) {
            *slot = i;
        }
        ed.dirty = true;
    }
    if k == 0 {
        ui.separator();
        ui.label("Example (what Check, Run and Debug use here):");
        let example = ed.example.clone();
        ref_field(app, ui, d, ed, "@example", "example", &example, false);
        ui.horizontal(|ui| {
            if ui.button("Use current selection as example").clicked() {
                use_selection_as_example(app, ed);
            }
            let mut fixed = ed.draft.inputs.first().is_some_and(|i| i.default_ref.is_some());
            if ui.checkbox(&mut fixed, "Always run on this range").on_hover_text("Otherwise the macro runs on the selection").changed()
                && let Some(first) = ed.draft.inputs.first_mut()
            {
                first.default_ref = fixed.then(|| ed.example.clone());
                ed.dirty = true;
            }
        });
    }
}

fn type_name(t: ColType) -> &'static str {
    match t {
        ColType::Any => "anything",
        ColType::Number => "numbers",
        ColType::Text => "text",
        ColType::Date => "dates",
        ColType::Bool => "TRUE or FALSE",
    }
}

/// The header texts of the example's first row (when it has a header).
fn example_headers(app: &SheetApp, example: &str, header: bool) -> Vec<String> {
    let (Some(doc), true) = (app.session.active(), header) else { return vec![] };
    let wb = &doc.wb;
    let Some((sheet, r)) = parse_refs(example).into_iter().next() else { return vec![] };
    let si = sheet.and_then(|s| wb.sheet_index(&s)).unwrap_or(wb.active_sheet);
    let Some(sh) = wb.sheet(si) else { return vec![] };
    (r.start.col..=r.end.col.min(r.start.col.saturating_add(15)))
        .map(|c| gridcraft_engine::display::cell_text(wb, sh, gridcraft_engine::core::CellRef::new(r.start.row, c)))
        .collect()
}

/// "Use current selection as example": the selection becomes the first input's example, and its
/// shape, header and column types are read from it.
pub fn use_selection_as_example(app: &SheetApp, ed: &mut Editor) {
    let Some(doc) = app.session.active() else { return };
    let r = doc.selection.current();
    let Some(sh) = doc.wb.active() else { return };
    ed.example = r.a1();
    let Some(first) = ed.draft.inputs.first_mut() else { return };
    let inferred = infer_input(sh, r);
    first.kind = if r.is_single() { InputKind::Cell } else { InputKind::Range };
    first.header = inferred.0;
    first.col_types = inferred.1;
    first.cols = [r.width(), r.width()];
    first.rows = [1, 0];
    if first.default_ref.is_some() {
        first.default_ref = Some(ed.example.clone());
    }
    ed.dirty = true;
}

/// Whether the first row of `r` looks like a header (all text over a row that isn't), and what
/// each column holds below it.
pub fn infer_input(sh: &gridcraft_engine::model::Sheet, r: RangeRef) -> (bool, Vec<ColType>) {
    use gridcraft_engine::core::{CellRef, Value};
    let kind = |v: &Value| match v {
        Value::Number(_) => Some(ColType::Number),
        Value::Text(_) => Some(ColType::Text),
        Value::Bool(_) => Some(ColType::Bool),
        _ => None,
    };
    let rows = r.height().min(2_000);
    let cols = r.width().min(16);
    let first: Vec<Option<ColType>> = (0..cols).map(|c| kind(&sh.value(CellRef::new(r.start.row, r.start.col + c)))).collect();
    let header = rows > 1
        && first.iter().all(|k| *k == Some(ColType::Text))
        && (0..cols).any(|c| matches!(kind(&sh.value(CellRef::new(r.start.row + 1, r.start.col + c))), Some(ColType::Number | ColType::Bool)));
    let start = r.start.row + u32::from(header);
    let types = (0..cols)
        .map(|c| {
            let mut seen: Option<ColType> = None;
            for row in start..r.start.row + rows {
                match (kind(&sh.value(CellRef::new(row, r.start.col + c))), seen) {
                    (None, _) => {}
                    (Some(k), None) => seen = Some(k),
                    (Some(k), Some(s)) if k == s => {}
                    _ => return ColType::Any,
                }
            }
            seen.unwrap_or(ColType::Any)
        })
        .collect();
    (header, types)
}

fn output_tab(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, ed: &mut Editor) {
    let t = Tokens::get(ui.ctx());
    ui.label("Output anchor (what the Output token stands for):");
    let anchor = ed.draft.output.anchor.clone().unwrap_or_default();
    ref_field(app, ui, d, ed, "@output", "anchor", &anchor, false);
    ui.label(
        egui::RichText::new("Leave it empty when no step uses Output. A token such as Input.right keeps it beside the data.")
            .small()
            .color(t.text_dim),
    );
    ui.add_space(6.0);
    ui.label("When results would land on cells that hold data:");
    let mut rule = ed.draft.output.on_conflict;
    for (r, label) in [
        (OnConflict::Ask, "Ask (overwrite, shift down or shift right)"),
        (OnConflict::Fail, "Don't run"),
        (OnConflict::Overwrite, "Overwrite the data"),
        (OnConflict::ShiftDown, "Shift the data down"),
        (OnConflict::ShiftRight, "Shift the data right"),
    ] {
        ui.radio_value(&mut rule, r, label);
    }
    if rule != ed.draft.output.on_conflict {
        ed.draft.output.on_conflict = rule;
        ed.dirty = true;
    }
    ui.label(egui::RichText::new("Agents and other programs that run the macro get an error instead of the question.").small().color(t.text_dim));
    if !ed.draft.permissions.is_empty() {
        ui.add_space(6.0);
        ui.label(format!("May use files through: {}", ed.draft.permissions.join(", ")));
    }
}

// ---------------------------------------------------------------- grid overlays

/// What the grid shows while the editor is open: the inputs (blue), the cells the macro writes
/// (green), the ones among them that hold data (red), and the current step's target (outlined).
/// Each range is on the named sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MacroOverlay {
    pub inputs: Vec<(String, RangeRef)>,
    pub footprint: Vec<(String, RangeRef)>,
    pub conflicts: Vec<(String, RangeRef)>,
    pub target: Vec<(String, RangeRef)>,
}

impl MacroOverlay {
    fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.footprint.is_empty() && self.conflicts.is_empty() && self.target.is_empty()
    }
}

fn named_refs(text: &str, default_sheet: &str) -> Vec<(String, RangeRef)> {
    parse_refs(text).into_iter().map(|(s, r)| (s.unwrap_or_else(|| default_sheet.to_string()), r)).collect()
}

/// The overlay for the editor's last check and debugger state.
pub fn overlay(app: &SheetApp, ed: &Editor) -> Option<MacroOverlay> {
    let sheet = app.session.active()?.wb.active()?.name.clone();
    let mut o = MacroOverlay::default();
    if let Some(c) = &ed.check {
        for v in c["inputs"].as_object().into_iter().flat_map(|m| m.values()).filter_map(Json::as_str) {
            o.inputs.extend(named_refs(v, &sheet));
        }
        for s in c["footprint"]["sheets"].as_array().into_iter().flatten() {
            let name = s["sheet"].as_str().unwrap_or(&sheet).to_string();
            for v in s["cells"].as_array().into_iter().flatten().filter_map(Json::as_str) {
                o.footprint.extend(named_refs(v, &name));
            }
            for v in s["conflicts"].as_array().into_iter().flatten().filter_map(Json::as_str) {
                o.conflicts.extend(named_refs(v, &name));
            }
        }
    }
    if let Some(st) = &ed.debug {
        let cur = &st["current"];
        if let Some(p) = cur["params"].as_object() {
            for (k, v) in p {
                if resolve::is_ref_key(k)
                    && let Some(s) = v.as_str()
                {
                    o.target.extend(named_refs(s, &sheet));
                }
            }
        }
        for k in ["over", "cell"] {
            if let Some(s) = cur[k].as_str() {
                o.target.extend(named_refs(s, &sheet));
            }
        }
    }
    (!o.is_empty()).then_some(o)
}

/// Paints the overlay's ranges on `sh` (called from the grid's overlay pass).
pub fn paint_overlay(p: &egui::Painter, geo: &crate::grid::Geo, sh: &gridcraft_engine::model::Sheet, o: &MacroOverlay) {
    let t = Tokens::get(p.ctx());
    let wash = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    let layers: [(&Vec<(String, RangeRef)>, Color32, u8, f32); 3] = [
        (&o.footprint, t.sheet_macro_footprint, 30, 1.5),
        (&o.inputs, t.sheet_macro_input, 26, 2.0),
        (&o.conflicts, t.sheet_macro_conflict, 60, 2.0),
    ];
    for (list, c, alpha, width) in layers {
        for (name, r) in list.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&sh.name)) {
            let rect = geo.range_rect(sh, *r);
            if !p.clip_rect().intersects(rect) {
                continue;
            }
            p.rect_filled(rect, 0.0, wash(c, alpha));
            p.rect_stroke(rect, 0.0, Stroke::new(width, c), StrokeKind::Inside);
            let _ = name;
        }
    }
    for (_, r) in o.target.iter().filter(|(n, _)| n.eq_ignore_ascii_case(&sh.name)) {
        let rect = geo.range_rect(sh, *r).expand(1.0);
        p.rect_stroke(rect, 1.0, Stroke::new(2.5, t.sheet_macro_target), StrokeKind::Outside);
        for corner in [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()] {
            p.rect_filled(Rect::from_center_size(corner, vec2(5.0, 5.0)), 0.0, t.sheet_macro_target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<Step> {
        vec![
            Step::command("cell.set", json!({"cell": "A1", "input": "x"})),
            Step::new(StepKind::Repeat {
                over: "Input".into(),
                of: RepeatOf::Rows,
                max: 10,
                body: vec![Step::command("cell.set", json!({"cell": "Row.right", "input": "a"})), Step::command("home.bold", json!({}))],
            }),
            Step::new(StepKind::If {
                cell: "B1".into(),
                op: CompareOp::Empty,
                value: Json::Null,
                then: vec![Step::new(StepKind::Stop { message: "empty".into(), rollback: false })],
                otherwise: vec![],
            }),
        ]
    }

    #[test]
    fn paths_reach_nested_steps() {
        let mut s = steps();
        assert_eq!(split_path("3.then.1"), Some(("3.then", 0)));
        assert_eq!(split_path("3.then"), None);
        assert_eq!(split_path("0"), None);
        assert!(matches!(step_at(&s, "2.2").map(|s| s.kind), Some(StepKind::Command { id, .. }) if id == "home.bold"));
        assert!(matches!(step_at(&s, "3.then.1").map(|s| s.kind), Some(StepKind::Stop { .. })));
        assert!(step_at(&s, "3.else.1").is_none());
        assert!(step_at(&s, "1.1").is_none(), "a command has no body");
        assert!(step_at(&s, "9").is_none());
        assert!(list_mut(&mut s, "3.else").is_some_and(|l| l.is_empty()));
    }

    #[test]
    fn rows_flatten_with_branches() {
        let mut out = Vec::new();
        rows(&steps(), "", 0, &mut out);
        let keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["1", "2", "2.1", "2.2", "3", "3.then", "3.then.1", "3.else"]);
        assert_eq!(out.iter().find(|r| r.key == "2.2").map(|r| (r.parent.as_str(), r.index, r.depth)), Some(("2", 1, 1)));
        assert_eq!(out.iter().find(|r| r.key == "3.then").and_then(|r| r.branch), Some("Then"));
    }

    #[test]
    fn moving_a_step_within_its_list() {
        let mut s = steps();
        // Step 1 dropped after step 2 (before row 2 of 0..3).
        assert_eq!(move_step(&mut s, "", 0, 2).as_deref(), Some("2"));
        assert!(matches!(&s[0].kind, StepKind::Repeat { .. }));
        // Inside the Repeat: the second step to the top.
        assert_eq!(move_step(&mut s, "1", 1, 0).as_deref(), Some("1.1"));
        assert!(matches!(step_at(&s, "1.1").map(|s| s.kind), Some(StepKind::Command { id, .. }) if id == "home.bold"));
        assert_eq!(move_step(&mut s, "", 7, 0), None);
        // Past the end lands last.
        assert_eq!(move_step(&mut s, "", 0, 99).as_deref(), Some("3"));
    }

    #[test]
    fn breakpoints_are_listed_by_path() {
        let mut s = steps();
        for p in ["1", "2.2", "3.then.1"] {
            if let Some(st) = step_mut(&mut s, p) {
                st.breakpoint = true;
            }
        }
        let mut out = Vec::new();
        breakpoints(&s, "", &mut out);
        assert_eq!(out, ["1", "2.2", "3.then.1"]);
    }

    #[test]
    fn summaries_and_categories() {
        let s = steps();
        assert_eq!(summary(&s[0]), "Set A1 to x");
        assert_eq!(summary(&s[1]), "Repeat for each row of Input");
        assert_eq!(summary(&s[2]), "If B1 is empty");
        assert_eq!(category(&s[0]), (Icon::Sheet, Cat::Blue));
        assert_eq!(category(&s[1]).1, Cat::Purple);
        let long = Step::command("home.bold", json!({"range": "A1:B2"}));
        assert!(summary(&long).contains("range: A1:B2"), "{}", summary(&long));
    }

    #[test]
    fn static_errors_flag_unknown_and_denied_commands() {
        let m = Macro::default();
        assert!(static_error(&m, &Step::command("no.such", json!({}))).is_some_and(|e| e.contains("no command")));
        assert!(static_error(&m, &Step::command("file.save", json!({}))).is_some_and(|e| e.contains("can't run in a macro")));
        assert!(static_error(&m, &Step::command("data.getData", json!({"path": "/tmp/x.csv"}))).is_some_and(|e| e.contains("permissions")));
        assert_eq!(static_error(&m, &Step::command("cell.set", json!({"cell": "A1"}))), None);
    }

    #[test]
    fn required_keys_come_from_the_params_doc() {
        assert_eq!(required_keys("{cell?: \"B2\", input: \"text, number or =formula\", sheet?, array?: bool}"), ["cell", "input"]);
        assert_eq!(required_keys("{range?, options: {a, b}, on?: bool}"), ["range", "options"]);
        assert!(required_keys("no braces").is_empty());
        assert!(required_keys("{").is_empty());
    }

    #[test]
    fn the_picker_offers_control_steps_and_allowed_commands_only() {
        let all = picker_items("");
        assert_eq!(all.first().map(|x| x.0.as_str()), Some("@repeat"));
        // Denied commands aren't offered (exports without a path are allowed).
        let all = picker_items("e");
        assert!(!all.iter().any(|x| x.0 == "file.save" || x.0 == "edit.undo" || x.0.starts_with("macro.")));
        let bold = picker_items("bold");
        assert!(bold.iter().any(|x| x.0 == "home.bold"));
        assert!(matches!(new_step("@if").kind, StepKind::If { .. }));
        assert!(matches!(new_step("cell.set").kind, StepKind::Command { params, .. } if params.get("cell").is_some()));
    }

    #[test]
    fn tokens_for_ranges_resolve_back_to_them() {
        let wb = Workbook::default();
        let input = RangeRef::parse("B2:D9").unwrap_or(RangeRef::cell(gridcraft_engine::core::CellRef::new(0, 0)));
        let inputs = vec![("Input".to_string(), Place { sheet: 0, range: input }, false)];
        let env = Env { wb: &wb, active_sheet: 0, inputs: &inputs, output: None, item: None };
        for (target, want) in [
            ("B2:D9", "Input"),
            ("C2:C9", "Input.col(2)"),
            ("B2:C9", "Input.cols(1..2)"),
            ("B3:D9", "Input.rows(2..8)"),
            ("B4:D4", "Input.row(3)"),
            ("C3", "Input.cell(2,2)"),
            ("C3:D4", "Input.cell(2,2).resize(2,2)"),
            ("F2:H9", "Input.offset(0,4)"),
            ("E2", "Input.cell(1,1).offset(0,3)"),
            ("E2:E9", "Input.cell(1,1).offset(0,3).resize(8,1)"),
        ] {
            let r = RangeRef::parse(target).unwrap_or(input);
            let tok = token_for("Input", input, r);
            assert_eq!(tok, want, "{target}");
            assert_eq!(resolve::resolve_ref(&tok, &env).map(|p| p.range), Ok(r), "{tok}");
        }
    }

    #[test]
    fn references_parse_with_sheets_and_dollars() {
        let refs = parse_refs("'My Sheet'!$B$2:$C$3, D4");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].0.as_deref(), Some("My Sheet"));
        assert_eq!(refs[0].1.a1(), "B2:C3");
        assert_eq!(refs[1], (None, RangeRef::parse("D4").unwrap_or(refs[0].1)));
        assert!(parse_refs("not a ref").is_empty());
    }

    #[test]
    fn inputs_are_inferred_from_an_example() {
        use gridcraft_engine::core::{CellRef, Value};
        let mut sh = gridcraft_engine::model::Sheet::new("S");
        for (c, h) in ["Item", "Qty"].iter().enumerate() {
            sh.set_value(CellRef::new(0, c as u32), Value::Text((*h).into()));
        }
        for r in 1..4 {
            sh.set_value(CellRef::new(r, 0), Value::Text(format!("x{r}").into()));
            sh.set_value(CellRef::new(r, 1), Value::Number(f64::from(r)));
        }
        let (header, types) = infer_input(&sh, RangeRef::parse("A1:B4").unwrap_or(RangeRef::cell(CellRef::new(0, 0))));
        assert!(header);
        assert_eq!(types, [ColType::Text, ColType::Number]);
    }
}
