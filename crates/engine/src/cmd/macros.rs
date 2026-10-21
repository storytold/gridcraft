//! Macros: typed command lists that run as one undo step. See [`crate::macros`] for the model,
//! tokens, runner, debugger, recorder and policy. Automate scripts (`automate.*`) are a separate,
//! older feature with their own storage; these commands neither read nor change them.

use serde_json::{Value as Json, json};

use super::*;
use crate::macros::recorder::RecordingStart;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "macro.list", "List Macros", [], None, "{} → [{name, description, shortcut?, steps, inputs: [names], onConflict, permissions}]", has_doc, list),
        cmd!(query "macro.get", "Get Macro", [], None, "{name} → the macro: {v, name, description, shortcut?, inputs, steps, output, permissions}", has_doc, get),
        cmd!(
            "macro.save",
            "Save Macro",
            [],
            None,
            "{macro: {name, description?, shortcut?, inputs?: [{name, kind: cell|range, rows: [min, max], cols: [min, max], header, colTypes: [number|text|date|bool|any], nonEmpty, defaultRef?}], steps: [{type: command, id, params, enabled?, breakpoint?, note?} | {type: repeat, over, of: rows|columns, max, body} | {type: if, cell, op: eq|ne|gt|ge|lt|le|empty|notEmpty|contains, value, then, else} | {type: stop, message, rollback?}], output?: {anchor?, onConflict: ask|fail|overwrite|shiftDown|shiftRight}, permissions?: [command ids]} (or those fields at the top level), replace?: bool (default true)} → {name, steps}",
            has_doc,
            save
        ),
        cmd!("macro.delete", "Delete Macro", [], None, "{name}", has_doc, delete),
        cmd!("macro.rename", "Rename Macro", [], None, "{name, to}", has_doc, rename),
        cmd!(
            "macro.run",
            "Run Macro",
            [],
            None,
            "{name, inputs?: {Input: \"B2:D9\"} | \"B2:D9\", onConflict?: ask|fail|overwrite|shiftDown|shiftRight, overwrite?: bool, approve?: bool, timeLimit?: seconds} runs as one undo step; any error rolls it all back → {macro, ran, inputs, stopped?, shifted?}",
            has_doc,
            run
        ),
        cmd!(
            query "macro.check",
            "Check Macro",
            [],
            None,
            "{name, inputs?, timeLimit?: seconds} validates the inputs and dry-runs the macro on the workbook (then puts everything back) → {ok, errors?, needsApproval?, inputs, output?, conflicts, footprint}",
            has_doc,
            check
        ),
        cmd!(
            noundo "macro.record",
            "Record Macro",
            ["View", "Macros"],
            None,
            "{stop?: bool, relative?: bool (default true; when starting), name?, shortcut?: \"Ctrl+Shift+K\" | \"K\", description?} toggles recording; when starting, the name (new and valid) and the shortcut (free) are checked and kept for the save, and without a name the macro gets the next MacroN; when stopping, a name or description given then wins (a taken name gets a suffix); with relative references the selection when it starts is the example input → {recording, name?, relative?, input?} or {recording: false, saved, steps, dropped, skipped, inputs}",
            has_doc,
            record
        ),
        cmd!(
            "macro.debug",
            "Step Into",
            [],
            None,
            "{action: start|step|continue|runTo|stop|state, name? and inputs?, onConflict?, approve? (start), step: path (runTo), keep?: bool (stop), breakpoints?: [\"3\", \"2.1\"], breakpointsOnly?: bool (the list replaces the steps' own breakpoints)} debugs a macro one step at a time as one undo step; edits of the workbooks it touched wait until it finishes or stops (other workbooks stay editable) → {state: paused|finished|stopped|failed, current: {step, kind, command?, params?, iteration?}, lastParams, executed, changed, atBreakpoint?, error?}",
            has_doc,
            |s, p| crate::macros::debug::debug(s, p)
        ),
    ]
}

fn name_param<'a>(cmd: &str, p: &'a Json) -> Result<&'a str> {
    str_param(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(cmd, "missing `name`"))
}

/// The index of macro `name` in the active workbook, or an error that says there is none.
fn index_of(s: &Session, name: &str) -> Result<usize> {
    crate::macros::find(&s.doc()?.wb, name).ok_or_else(|| EngineError::Other(format!("There is no macro called '{name}'.")))
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let wb = &s.doc()?.wb;
    Ok(Json::Array(
        wb.macros
            .iter()
            .map(|m| {
                let mut o = json!({
                    "name": m.name,
                    "description": m.description,
                    "steps": m.step_count(),
                    "inputs": m.inputs.iter().map(|i| i.name.clone()).collect::<Vec<_>>(),
                    "onConflict": m.output.on_conflict.name(),
                    "permissions": m.permissions,
                });
                if let Some(k) = &m.shortcut {
                    o["shortcut"] = json!(k);
                }
                o
            })
            .collect(),
    ))
}

fn get(s: &mut Session, p: &Json) -> Result<Json> {
    let i = index_of(s, name_param("macro.get", p)?)?;
    let wb = &s.doc()?.wb;
    Ok(wb.macros.get(i).map(crate::macros::macro_json).unwrap_or(Json::Null))
}

fn save(s: &mut Session, p: &Json) -> Result<Json> {
    let v = p.get("macro").filter(|m| m.is_object()).unwrap_or(p);
    let m = crate::macros::parse_macro(v).map_err(|e| bad("macro.save", e))?;
    let replace = bool_param(p, "replace").unwrap_or(true);
    let (name, steps) = (m.name.clone(), m.step_count());
    edit(s, |cx| {
        match crate::macros::find(&cx.wb, &m.name) {
            Some(_) if !replace => return Err(EngineError::Other(format!("A macro called '{}' already exists.", m.name))),
            Some(i) => {
                if let Some(old) = cx.wb.macros.get_mut(i) {
                    *old = m;
                }
            }
            None if cx.wb.macros.len() >= gridcraft_model::MAX_MACROS => {
                return Err(EngineError::Other(format!("A workbook can hold at most {} macros.", gridcraft_model::MAX_MACROS)));
            }
            None => cx.wb.macros.push(m),
        }
        Ok(())
    })?;
    Ok(json!({"name": name, "steps": steps}))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let i = index_of(s, name_param("macro.delete", p)?)?;
    edit(s, |cx| {
        if i < cx.wb.macros.len() {
            cx.wb.macros.remove(i);
        }
        Ok(Json::Null)
    })
}

fn rename(s: &mut Session, p: &Json) -> Result<Json> {
    let i = index_of(s, name_param("macro.rename", p)?)?;
    let to = str_param(p, "to").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("macro.rename", "missing `to`"))?.to_string();
    if to.chars().any(char::is_control) {
        return Err(bad("macro.rename", "a macro name can't hold line breaks or other control characters"));
    }
    if crate::macros::find(&s.doc()?.wb, &to).is_some_and(|j| j != i) {
        return Err(EngineError::Other(format!("A macro called '{to}' already exists.")));
    }
    edit(s, |cx| {
        let Some(m) = cx.wb.macros.get_mut(i) else { return Ok(()) };
        let mut renamed = m.clone();
        renamed.name = to.clone();
        renamed.check_limits().map_err(EngineError::Other)?;
        *m = renamed;
        Ok(())
    })?;
    Ok(json!({"name": to}))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let name = name_param("macro.run", p)?;
    crate::macros::run::run(s, name, p)
}

fn check(s: &mut Session, p: &Json) -> Result<Json> {
    let name = name_param("macro.check", p)?;
    crate::macros::run::check(s, name, p)
}

fn record(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "stop").unwrap_or(false) || s.is_recording() {
        return stop_recording(s, p);
    }
    start_recording(s, p, bool_param(p, "relative").unwrap_or(true))
}

/// Starts recording. `name`, `shortcut` and `description` (from the Record Macro dialog) are
/// checked now, so a bad name or a taken shortcut is refused before anything is recorded.
fn start_recording(s: &mut Session, p: &Json, relative: bool) -> Result<Json> {
    let d = s.doc()?;
    let name = match str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => Some(crate::macros::check_new_name(&d.wb, n).map_err(|e| bad("macro.record", e))?),
        None => None,
    };
    let shortcut = match str_param(p, "shortcut").map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            let letter = crate::macros::parse_shortcut(k)
                .ok_or_else(|| bad("macro.record", "`shortcut` must be one letter from A to Z (used as Ctrl+Shift+letter)"))?;
            if let Some(why) = crate::macros::shortcut_conflict(&d.wb, name.as_deref().unwrap_or(""), letter) {
                return Err(bad("macro.record", why));
            }
            Some(crate::macros::shortcut_text(letter))
        }
        None => None,
    };
    let description = match str_param(p, "description") {
        Some(t) if t.chars().count() > gridcraft_model::MAX_MACRO_TEXT => {
            return Err(bad("macro.record", format!("`description` can be at most {} characters long", gridcraft_model::MAX_MACRO_TEXT)));
        }
        Some(t) => Some(t.to_string()),
        None => None,
    };
    let start = RecordingStart {
        doc: d.uid,
        sheet: d.wb.active_sheet,
        sheet_name: d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(),
        selection: d.selection.clone(),
        relative,
        name: name.clone(),
        shortcut,
        description,
    };
    let input = start.selection.current().a1();
    s.start_recording();
    s.macro_recording = Some(start);
    let mut out = if relative { json!({"recording": true, "relative": true, "input": input}) } else { json!({"recording": true, "relative": false}) };
    if let Some(n) = name {
        out["name"] = json!(n);
    }
    Ok(out)
}

/// Stops recording and saves what was recorded as a macro, in the workbook where recording
/// started. When saving fails (a bad name, a full workbook, a macro being debugged there) the
/// recording goes on, so stopping again keeps what was recorded; when that workbook was closed,
/// the recording ends unsaved.
fn stop_recording(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(rec) = s.stop_recording() else { return Ok(json!({"recording": false})) };
    let start = s.macro_recording.take();
    let at = match &start {
        Some(st) => s
            .docs
            .iter()
            .position(|d| d.uid == st.doc)
            .ok_or_else(|| EngineError::Other("The workbook where recording started was closed, so the recording was not saved.".into()))?,
        None => s.active_index(),
    };
    let saved = save_recording(s, p, &rec, start.clone(), at);
    if saved.is_err() {
        s.recorder = Some(rec);
        s.macro_recording = start;
    }
    saved
}

/// Saves the recording into the workbook at index `at`.
fn save_recording(s: &mut Session, p: &Json, rec: &crate::record::Recorder, start: Option<RecordingStart>, at: usize) -> Result<Json> {
    let d = s.docs.get(at).ok_or(EngineError::NoDocument)?;
    // A macro being debugged there could still roll the saved macro back with its run.
    if s.macro_debug.as_ref().is_some_and(|m| m.locks(d.uid)) {
        return Err(EngineError::Disabled("macro.record".into(), crate::macros::debug::BUSY.into()));
    }
    let start = start.unwrap_or_else(|| RecordingStart {
        doc: d.uid,
        sheet: d.wb.active_sheet,
        sheet_name: d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(),
        selection: d.selection.clone(),
        relative: true,
        name: None,
        shortcut: None,
        description: None,
    });
    let wb = d.wb.clone();
    // A name given when stopping wins over the one given when starting; either one, if another
    // macro has it, gets a suffix, so the save never replaces a macro.
    let name = match str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).or(start.name.as_deref()) {
        Some(n) => crate::macros::unique_name(&wb, n),
        None => crate::macros::next_name(&wb),
    };
    let mut converted = crate::macros::recorder::convert(&name, &start, &rec.steps, &wb);
    // Steps past the macro limit are left out and counted with the ones the recorder dropped.
    let cut = converted.mac.steps.len().saturating_sub(gridcraft_model::MAX_MACRO_STEPS);
    converted.mac.steps.truncate(gridcraft_model::MAX_MACRO_STEPS);
    if let Some(desc) = str_param(p, "description").or(start.description.as_deref()) {
        converted.mac.description = desc.chars().take(gridcraft_model::MAX_MACRO_TEXT).collect();
    }
    // The shortcut, unless another macro took it while recording.
    if let Some(k) = &start.shortcut
        && crate::macros::parse_shortcut(k).is_some_and(|c| crate::macros::shortcut_conflict(&wb, &name, c).is_none())
    {
        converted.mac.shortcut = Some(k.clone());
    }
    let steps = converted.mac.steps.len();
    let inputs: Vec<String> = converted.mac.inputs.iter().map(|i| i.name.clone()).collect();
    // Saved into that workbook even when another one is active now.
    let active = s.active_index();
    s.set_active(at);
    let saved = s.execute("macro.save", json!({"macro": crate::macros::macro_json(&converted.mac), "replace": false}));
    s.set_active(active);
    saved?;
    Ok(
        json!({"recording": false, "saved": name, "steps": steps, "dropped": rec.dropped.saturating_add(cut), "skipped": converted.skipped, "inputs": inputs}),
    )
}
