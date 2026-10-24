//! The macro debugger: a run that stops between steps.
//!
//! `macro.debug` starts a run that pauses before its first step, then steps, continues to the next
//! breakpoint, runs to a chosen step, or stops. The run lives on the session between calls (one
//! action per call) inside an undo group that spans them, so a debugged run is still
//! one undo step; Stop keeps the changes or rolls them back. While a macro is being debugged,
//! other edits of the workbooks it touched (its own, and any other its steps changed) are refused
//! ("Finish or stop the macro first"); other workbooks stay editable.

use std::collections::BTreeSet;
use std::sync::Arc;

use gridcraft_model::Workbook;
use serde_json::{Value as Json, json};

use super::run::{Machine, StepError};
use crate::{EngineError, GroupToken, Result, Session};

/// A macro being debugged.
pub struct Debugger {
    pub name: String,
    pub doc: u64,
    /// Out of the session only while a step runs.
    machine: Option<Machine>,
    token: Option<GroupToken>,
    /// Breakpoints set for this run (step paths), on top of the steps' own.
    pub breakpoints: BTreeSet<String>,
    /// Only `breakpoints` stop the run; the steps' own flags are ignored (an editor that shows
    /// its own breakpoints sends them all, so clearing one takes effect at once).
    pub only_listed: bool,
    /// The workbook when the run started, for "changed so far".
    start: Arc<Workbook>,
    /// Every step path in the macro, for checking `runTo` targets.
    paths: BTreeSet<String>,
    /// The workbooks the run has touched (by uid): its own, and any other a step changed. Edits
    /// of these wait until the run ends.
    touched: BTreeSet<u64>,
}

impl Debugger {
    /// Whether edits of the workbook with this uid wait for the run to end.
    pub fn locks(&self, doc: u64) -> bool {
        self.touched.contains(&doc)
    }

    /// Whether one of the run's steps is running (its machine is out of the session).
    pub fn stepping(&self) -> bool {
        self.machine.is_none()
    }
}

/// Each open workbook's uid and its current version, to see which ones a step changes.
fn versions(s: &Session) -> Vec<(u64, Arc<Workbook>)> {
    s.docs.iter().map(|d| (d.uid, d.wb.clone())).collect()
}

/// Adds to the run's touched log the workbooks that changed since `before`.
fn note_touched(s: &mut Session, before: &[(u64, Arc<Workbook>)]) {
    let changed: Vec<u64> =
        s.docs.iter().filter(|d| before.iter().any(|(uid, wb)| *uid == d.uid && !Arc::ptr_eq(wb, &d.wb))).map(|d| d.uid).collect();
    if let Some(dbg) = s.macro_debug.as_mut() {
        dbg.touched.extend(changed);
    }
}

/// The message for edits refused while a macro is being debugged.
pub const BUSY: &str = "Finish or stop the macro first";

enum Mode {
    Step,
    Continue,
    RunTo(String),
}

/// `macro.debug {name?, inputs?, action: start|step|continue|runTo|stop|state, step?, keep?,
/// breakpoints?}`.
pub fn debug(s: &mut Session, p: &Json) -> Result<Json> {
    let action = match p.get("action") {
        Some(Json::String(a)) => a.clone(),
        // `{"action": {"runTo": {"step": "3"}}}`
        Some(Json::Object(o)) => o.keys().next().cloned().unwrap_or_default(),
        _ if s.macro_debug.is_some() => "state".into(),
        _ => "start".into(),
    };
    if action != "start"
        && let (Some(d), Some(bps)) = (s.macro_debug.as_mut(), p.get("breakpoints"))
    {
        d.breakpoints = paths_param(bps)?;
        if let Some(only) = p.get("breakpointsOnly").and_then(Json::as_bool) {
            d.only_listed = only;
        }
    }
    match action.as_str() {
        "start" => start(s, p),
        "step" => advance(s, Mode::Step),
        "continue" => advance(s, Mode::Continue),
        "runTo" => {
            let target = p
                .get("step")
                .or_else(|| p.pointer("/action/runTo/step"))
                .and_then(path_of)
                .ok_or_else(|| bad("`runTo` needs `step`: the path of the step to stop at"))?;
            let d = s.macro_debug.as_ref().ok_or_else(not_debugging)?;
            if !d.paths.contains(&target) {
                return Err(bad(format!("macro '{}' has no step {target}", d.name)));
            }
            advance(s, Mode::RunTo(target))
        }
        "stop" => stop(s, p.get("keep").and_then(Json::as_bool).unwrap_or(false)),
        "state" => state(s),
        other => Err(bad(format!("action must be start, step, continue, runTo, stop or state, not '{other}'"))),
    }
}

fn bad(msg: impl Into<String>) -> EngineError {
    crate::cmd::bad("macro.debug", msg)
}

fn not_debugging() -> EngineError {
    EngineError::Other("No macro is being debugged. Start one with action: start.".into())
}

/// A step path from JSON: `"2.1"` or `3`.
fn path_of(v: &Json) -> Option<String> {
    match v {
        Json::String(t) if !t.trim().is_empty() => Some(t.trim().to_string()),
        Json::Number(n) => n.as_u64().map(|n| n.to_string()),
        _ => None,
    }
}

fn paths_param(v: &Json) -> Result<BTreeSet<String>> {
    let list = v.as_array().ok_or_else(|| bad("`breakpoints` must be a list of step paths"))?;
    list.iter().take(10_000).map(|x| path_of(x).ok_or_else(|| bad("a breakpoint is a step path such as \"3\" or \"2.1\""))).collect()
}

fn start(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(d) = &s.macro_debug {
        return Err(EngineError::Other(format!("Macro '{}' is being debugged. Stop it first.", d.name)));
    }
    let name = p.get("name").and_then(Json::as_str).filter(|n| !n.trim().is_empty()).ok_or_else(|| bad("missing `name`"))?;
    let b = super::run::begin(s, name, p)?;
    let breakpoints = match p.get("breakpoints") {
        Some(v) => paths_param(v)?,
        None => BTreeSet::new(),
    };
    let start_wb = s.doc()?.wb.clone();
    let token = s.group_begin(&format!("Run Macro: {}", b.mac.name))?;
    // The run's machine: it checks each step for writes over data when the footprint was partial.
    let mut machine = match super::run::make_way(s, &b) {
        Ok((machine, _)) => machine,
        Err(e) => {
            let _ = s.group_rollback(token);
            return Err(e);
        }
    };
    machine.restart_clock();
    let paths = all_paths(&b.mac.steps, "");
    s.macro_debug = Some(Debugger {
        name: b.mac.name.clone(),
        doc: b.prepared.doc,
        machine: Some(machine),
        token: Some(token),
        breakpoints,
        only_listed: p.get("breakpointsOnly").and_then(Json::as_bool).unwrap_or(false),
        start: start_wb,
        paths,
        touched: BTreeSet::from([b.prepared.doc]),
    });
    if s.macro_debug.as_ref().and_then(|d| d.machine.as_ref()).is_some_and(Machine::is_done) {
        return finish(s, None);
    }
    state(s)
}

fn all_paths(steps: &[gridcraft_model::Step], prefix: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, st) in steps.iter().enumerate() {
        let path = if prefix.is_empty() { (i + 1).to_string() } else { format!("{prefix}.{}", i + 1) };
        match &st.kind {
            gridcraft_model::StepKind::Repeat { body, .. } => out.extend(all_paths(body, &path)),
            gridcraft_model::StepKind::If { then, otherwise, .. } => {
                out.extend(all_paths(then, &format!("{path}.then")));
                out.extend(all_paths(otherwise, &format!("{path}.else")));
            }
            _ => {}
        }
        out.insert(path);
    }
    out
}

/// Runs one step (`Step`), or steps until a breakpoint, the target step or the end.
fn advance(s: &mut Session, mode: Mode) -> Result<Json> {
    let d = s.macro_debug.as_mut().ok_or_else(not_debugging)?;
    let name = d.name.clone();
    let breakpoints = d.breakpoints.clone();
    let only_listed = d.only_listed;
    let mut machine = d.machine.take().ok_or_else(not_debugging)?;
    machine.restart_clock();
    let before = versions(s);
    let mut first = true;
    let mut hit = false;
    let mut outcome = Ok(());
    while !machine.is_done() {
        if !first {
            let Some((path, st)) = machine.current() else { break };
            if matches!(mode, Mode::Step) {
                break;
            }
            if (st.breakpoint && !only_listed) || breakpoints.contains(&path) {
                hit = true;
                break;
            }
            if let Mode::RunTo(target) = &mode
                && *target == path
            {
                break;
            }
        }
        first = false;
        outcome = machine.step(s);
        if outcome.is_err() {
            break;
        }
    }
    let done = machine.is_done();
    let executed = machine.executed;
    match s.macro_debug.as_mut() {
        Some(d) => d.machine = Some(machine),
        // A step panicked and `recover` has already rolled the run back.
        None => {
            return match outcome {
                Err(e) => Ok(failed(&name, executed, &e)),
                Ok(()) => Err(not_debugging()),
            };
        }
    }
    note_touched(s, &before);
    if let Err(e) = outcome {
        return finish(s, Some(e));
    }
    if done {
        return finish(s, None);
    }
    let mut out = state(s)?;
    out["atBreakpoint"] = json!(hit);
    Ok(out)
}

/// Ends the run after its last step (kept as one undo step, unless a Stop step asked for a
/// rollback) or a failed step (rolled back).
fn finish(s: &mut Session, error: Option<StepError>) -> Result<Json> {
    let Some(mut d) = s.macro_debug.take() else { return Err(not_debugging()) };
    let machine = d.machine.take();
    let changed = changed(s, &d);
    let stopped = machine.as_ref().and_then(|m| m.stopped.clone());
    let executed = machine.as_ref().map_or(0, |m| m.executed);
    let rollback = error.is_some() || matches!(stopped, Some((_, true)));
    if let Some(token) = d.token.take() {
        // The group is gone only if its workbook was closed; then there is nothing to end.
        let _ = if rollback { s.group_rollback(token) } else { s.group_end(token).map(|_| ()) };
    }
    if let Some(e) = &error {
        return Ok(failed(&d.name, executed, e));
    }
    let mut out = json!({"macro": d.name, "executed": executed});
    match &stopped {
        Some((msg, rolled)) => {
            out["state"] = json!("stopped");
            out["stopped"] = json!(msg);
            out["kept"] = json!(!rolled);
        }
        None => out["state"] = json!("finished"),
    }
    if !rollback {
        out["changed"] = json!(changed);
    }
    Ok(out)
}

/// The result of a run that a failed step rolled back.
fn failed(name: &str, executed: u64, e: &StepError) -> Json {
    json!({
        "macro": name,
        "executed": executed,
        "state": "failed",
        "error": serde_json::to_value(e).unwrap_or(Json::Null),
        "message": format!("{} Nothing was changed.", e.describe()),
    })
}

/// Stops the run: keeps what it did as one undo step, or rolls it back.
fn stop(s: &mut Session, keep: bool) -> Result<Json> {
    let Some(mut d) = s.macro_debug.take() else { return Err(not_debugging()) };
    let changed = changed(s, &d);
    if let Some(token) = d.token.take() {
        let _ = if keep { s.group_end(token).map(|_| ()) } else { s.group_rollback(token) };
    }
    let executed = d.machine.as_ref().map_or(0, |m| m.executed);
    let mut out = json!({"state": "stopped", "macro": d.name, "kept": keep, "executed": executed});
    if keep {
        out["changed"] = json!(changed);
    }
    Ok(out)
}

/// Where the run is: the next step with its parameters as they would run, the step that just
/// ran, and the cells changed so far.
fn state(s: &mut Session) -> Result<Json> {
    let d = s.macro_debug.as_ref().ok_or_else(not_debugging)?;
    let machine = d.machine.as_ref().ok_or_else(not_debugging)?;
    let wb = s.docs.iter().find(|x| x.uid == d.doc).map(|x| x.wb.clone()).ok_or(EngineError::NoDocument)?;
    Ok(json!({
        "state": "paused",
        "macro": d.name,
        "current": machine.current_json(&wb),
        "lastParams": machine.last_params,
        "executed": machine.executed,
        "breakpoints": d.breakpoints,
        "changed": changed(s, d),
    }))
}

/// The cells changed since the run started, as `Sheet!A1:B2` ranges.
fn changed(s: &Session, d: &Debugger) -> Vec<String> {
    let Some(now) = s.docs.iter().find(|x| x.uid == d.doc).map(|x| x.wb.clone()) else { return vec![] };
    let mut out = Vec::new();
    for sh in &now.sheets {
        let Some(before) = d.start.sheet_index(&sh.name).and_then(|i| d.start.sheets.get(i)) else {
            out.push(format!("{} (new sheet)", sh.name));
            continue;
        };
        if Arc::ptr_eq(before, sh) {
            continue;
        }
        let mut cells = before.cells.diff(&sh.cells);
        let q = super::resolve::quote_sheet(&sh.name);
        out.extend(super::run::ranges(&mut cells).into_iter().map(|r| format!("{q}!{r}")));
    }
    out
}

/// After a command panicked while a macro was being debugged: if the run lost its place, roll
/// it back so its undo group doesn't stay open.
pub(crate) fn recover(s: &mut Session) {
    if s.macro_debug.as_ref().is_some_and(Debugger::stepping)
        && let Some(mut d) = s.macro_debug.take()
        && let Some(token) = d.token.take()
    {
        let _ = s.group_rollback(token);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn a_run_that_lost_its_place_is_rolled_back() {
        let mut s = Session::new();
        s.new_workbook();
        let m = crate::macros::parse_macro(&json!({"name": "M", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "x"}},
            {"command": "cell.set", "params": {"cell": "A2", "input": "y"}}
        ]}))
        .unwrap();
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        wb.macros.push(m);
        d.wb = std::sync::Arc::new(wb);
        let start = s.doc().unwrap().wb.clone();
        s.execute("macro.debug", json!({"name": "M"})).unwrap();
        s.execute("macro.debug", json!({"action": "step"})).unwrap();
        // As if a step had panicked with the machine out of the session.
        s.macro_debug.as_mut().unwrap().machine = None;
        super::recover(&mut s);
        assert!(s.macro_debug.is_none());
        assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().wb, &start));
        assert!(!s.in_group(s.doc().unwrap().uid));
        s.execute("cell.set", json!({"cell": "B1", "input": "free"})).unwrap();
    }

    #[test]
    fn a_step_that_panics_reports_the_step_failure() {
        crate::register_test_command("test.debugStepPanics", |_, _| panic!("boom"));
        let mut s = Session::new();
        s.new_workbook();
        let m = crate::macros::parse_macro(&json!({"name": "M", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "x"}},
            {"command": "test.debugStepPanics", "params": {}}
        ]}))
        .unwrap();
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        wb.macros.push(m);
        d.wb = std::sync::Arc::new(wb);
        let start = s.doc().unwrap().wb.clone();
        s.execute("macro.debug", json!({"name": "M"})).unwrap();
        s.execute("macro.debug", json!({"action": "step"})).unwrap();
        let r = s.execute("macro.debug", json!({"action": "step"})).unwrap();
        assert_eq!(r["state"], "failed", "{r}");
        assert_eq!(r["macro"], "M");
        assert_eq!(r["error"]["step"], "2");
        assert!(r["message"].as_str().unwrap().contains("boom"), "{r}");
        assert!(s.macro_debug.is_none());
        assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().wb, &start));
        assert!(!s.in_group(s.doc().unwrap().uid));
    }

    #[test]
    fn a_workbook_in_the_touched_log_waits_until_the_run_ends() {
        let mut s = Session::new();
        s.new_workbook();
        let m = crate::macros::parse_macro(&json!({"name": "M", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "x"}},
            {"command": "cell.set", "params": {"cell": "A2", "input": "y"}}
        ]}))
        .unwrap();
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        wb.macros.push(m);
        d.wb = std::sync::Arc::new(wb);
        s.new_workbook();
        let other = s.doc().unwrap().uid;
        s.set_active(0);
        s.execute("macro.debug", json!({"name": "M"})).unwrap();
        assert!(!s.macro_debug.as_ref().unwrap().locks(other));
        // As if a step had changed the other workbook too.
        let before = super::versions(&s);
        let d = s.docs.iter_mut().find(|d| d.uid == other).unwrap();
        let mut wb = (*d.wb).clone();
        wb.props.title = "changed by the macro".into();
        d.wb = std::sync::Arc::new(wb);
        super::note_touched(&mut s, &before);
        assert!(s.macro_debug.as_ref().unwrap().locks(other));
        s.set_active(1);
        assert!(s.execute("cell.set", json!({"cell": "B1", "input": "no"})).is_err());
        s.execute("macro.debug", json!({"action": "stop", "keep": true})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "free"})).unwrap();
    }

    #[test]
    fn edits_run_by_another_command_wait_too() {
        let mut s = Session::new();
        s.new_workbook();
        let m = crate::macros::parse_macro(&json!({"name": "M", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "x"}},
            {"command": "cell.set", "params": {"cell": "A2", "input": "y"}}
        ]}))
        .unwrap();
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        wb.macros.push(m);
        d.wb = std::sync::Arc::new(wb);
        s.execute("automate.recordActions", json!({})).unwrap();
        s.execute("cell.set", json!({"cell": "C1", "input": "1"})).unwrap();
        s.execute("macro.debug", json!({"name": "M"})).unwrap();
        s.execute("macro.debug", json!({"action": "step"})).unwrap();
        // Record Actions changes nothing itself; stopping it saves the script with a nested
        // command, which would land in the run's undo group.
        let e = s.execute("automate.recordActions", json!({"stop": true, "name": "S"})).unwrap_err().to_string();
        assert!(e.contains(super::BUSY), "{e}");
        assert!(s.execute("automate.allScripts", json!({})).unwrap().as_array().unwrap().is_empty());
        // The run's own steps still go ahead.
        let r = s.execute("macro.debug", json!({"action": "step"})).unwrap();
        assert_eq!(r["executed"], 2, "{r}");
        assert!(s.macro_debug.is_none());
        // The recording was kept: stopping now saves it.
        let saved = s.execute("automate.recordActions", json!({"stop": true, "name": "S"})).unwrap();
        assert_eq!(saved["saved"], "S", "{saved}");
        let scripts = s.execute("automate.allScripts", json!({})).unwrap();
        assert_eq!(scripts[0]["name"], "S", "{scripts}");
    }
}
