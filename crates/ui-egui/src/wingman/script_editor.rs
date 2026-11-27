//! Access the unsaved, typed draft in Automate's Macro Editor. Never substitute a workbook
//! script with the same name or interpret an imported vbaProject.bin as editable source.
mod flow;

use super::workbook;
use crate::{
    SheetApp,
    macro_editor::{self, Editor},
};
use gridcraft_engine::model::{Macro, Step, StepKind};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{VecDeque, hash_map::DefaultHasher},
    hash::Hasher,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_ARGS: usize = 64 * 1024;
static INSTANCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
struct Snapshot {
    draft: Macro,
    dirty: bool,
    selected: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DraftHistory {
    instance: u64,
    expected: Option<u64>,
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
}

impl Default for DraftHistory {
    fn default() -> Self {
        Self { instance: INSTANCE.fetch_add(1, Ordering::Relaxed), expected: None, undo: VecDeque::new(), redo: VecDeque::new() }
    }
}

fn fingerprint(ed: &Editor) -> Result<u64, String> {
    // Only on explicit reads, edits, or history actions, never on egui's draw loop.
    // Bound the original before copying any undo snapshot (a hand-edited draft can be very large).
    gridcraft_engine::macros::check(&ed.draft)?;
    let bytes = serde_json::to_vec(&ed.draft).map_err(|_| "Cannot read the current macro draft.")?;
    let mut h = DefaultHasher::new();
    h.write(&bytes);
    Ok(h.finish())
}

fn ticket(ed: &Editor, hash: u64) -> String {
    format!("{:x}-{:x}-{hash:016x}", ed.doc, ed.wingman_history.instance)
}

pub(super) fn owner(app: &SheetApp) -> Option<u64> {
    let ed = app.macros.editor.as_ref()?;
    (app.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") && app.session.active().is_some_and(|d| d.uid == ed.doc))
        .then_some(ed.wingman_history.instance)
}

pub(super) fn available(app: &SheetApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| d.name == "macroEditor")
        && app.message.is_none()
        && app.macros.question.is_none()
        && app.session.macro_debug.is_none()
        && app.macros.editor.as_ref().is_some_and(|ed| app.session.active().is_some_and(|d| d.uid == ed.doc) && ready(ed))
}

fn ready(ed: &Editor) -> bool {
    ed.debug.is_none()
        && !ed.start_check
        && !ed.start_debug
        && !ed.close_requested
        && !ed.closing
        && !ed.ask_close
        && !ed.ask_stop
        && !ed.quit
        && ed.then_close.is_none()
        && ed.picker.is_none()
        && ed.refs.pick.is_none()
        && ed.refs.collapsed.is_none()
}

pub(super) fn is_write(name: &str) -> Option<bool> {
    match name {
        "read_macro_draft" => Some(false),
        "edit_macro_draft" => Some(true),
        _ => None,
    }
}

pub(super) fn schemas(allow_write: bool) -> Vec<Value> {
    let list =
        json!({"type":"string","description":"Empty string for top-level steps; e.g. 2 for a Repeat body, 4.then or 4.else for an If branch."});
    let schema = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
        "type":"function","function":{"name":name,"description":description,
            "parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
    };
    let mut out = vec![schema(
        "read_macro_draft",
        "Inspect the actual open Macro Editor draft, not VBA. Returns a revision token and 1-based step indices.",
        json!({"parent":list,"offset":{"type":"integer","minimum":0}}),
        &[],
    )];
    if allow_write {
        out.push(schema("edit_macro_draft","Edit the open native macro draft without saving or running it. Use its revision. Set description; splice steps at index (count+1 to append); or set fields at index to edit a step in place. New If/Repeat bodies are empty: insert children by parent path in the next edit.",
            json!({"revision":{"type":"string"},"parent":list,"description":{"type":"string","maxLength":4096},"index":{"type":"integer","minimum":1},
                "fields":flow::update_schema(),"remove":{"type":"integer","minimum":0,"maximum":64},"steps":{"type":"array","maxItems":64,"items":flow::step_schema()}}),
            &["revision"]));
    }
    out
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct Read {
    parent: String,
    offset: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    revision: String,
    #[serde(default)]
    parent: String,
    description: Option<String>,
    index: Option<usize>,
    remove: Option<usize>,
    steps: Option<Vec<NewStep>>,
    fields: Option<serde_json::Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewStep {
    tool: String,
    args: Value,
    note: Option<String>,
    enabled: Option<bool>,
    breakpoint: Option<bool>,
}

fn preview(step: &Step) -> Value {
    let kind = match &step.kind {
        StepKind::Command { id, params } => {
            // Existing user-authored commands can contain credentials, file paths, or arbitrary JSON.
            // Inspect only the same parameters Wingman could author, never dump the rest.
            let pair: Option<(&str, &[&str])> = match id.as_str() {
                "cell.set" => Some(("set_cell", &["sheet", "cell", "input"])),
                "range.setValues" => Some(("set_values", &["sheet", "range", "values", "parse"])),
                "home.formatCells" => Some(("format_range", &["sheet", "range", "style"])),
                "home.fillColor" => Some(("set_fill", &["sheet", "range", "color"])),
                "home.fontColor" => Some(("set_ink", &["sheet", "range", "color"])),
                _ => None,
            };
            if let Some((tool, keys)) = pair {
                let mut p = serde_json::Map::new();
                for &k in keys {
                    if let Some(v) = params.get(k)
                        && workbook::valid_field(k, v)
                    {
                        p.insert(k.into(), v.clone());
                    }
                }
                json!({"tool":tool,"args":p})
            } else {
                json!({"command":id,"parametersOmitted":true})
            }
        }
        other => flow::header(other),
    };
    json!({"step":kind,"note":step.note,"enabled":step.enabled,"breakpoint":step.breakpoint})
}

pub(super) fn inspect(app: &SheetApp, args: &Value) -> Result<Value, String> {
    if !available(app) {
        return Err("Open Automate → Macros → Edit and finish any editor prompt first.".into());
    }
    let ed = app.macros.editor.as_ref().ok_or("The macro editor has closed.")?;
    let opts: Read = serde_json::from_value(args.clone()).map_err(|_| "Invalid draft read arguments.")?;
    let steps = macro_editor::list_at(&ed.draft.steps, &opts.parent).ok_or("Unknown step list in this draft.")?;
    let revision = ticket(ed, fingerprint(ed)?);
    let mut page = Vec::new();
    let mut bytes = 0;
    for step in steps.iter().skip(opts.offset).take(16) {
        let mut item = preview(step);
        let n = item.to_string().len();
        if n > 20 * 1024 {
            item = json!({"parametersOmitted":true,"reason":"step exceeds the preview limit"});
        }
        if bytes + n.min(128 + 20 * 1024) > 25 * 1024 && !page.is_empty() {
            break;
        }
        bytes += item.to_string().len();
        page.push(json!({"index":opts.offset+page.len()+1,"data":item}));
    }
    let end = opts.offset.saturating_add(page.len());
    let sheet = app
        .session
        .active()
        .and_then(|d| d.wb.active())
        .filter(|sh| sh.visibility == gridcraft_engine::model::Visibility::Visible)
        .map(|sh| &sh.name);
    Ok(json!({"name":ed.draft.name,"description":ed.draft.description,"dirty":ed.dirty,"revision":revision,"parent":opts.parent,
        "activeSheet":sheet,"selected":ed.selected,"count":steps.len(),"steps":page,"nextOffset":(end<steps.len()).then_some(end)}))
}

fn new_steps(app: &SheetApp, steps: &[NewStep]) -> Result<Vec<Step>, String> {
    if steps.len() > 64 {
        return Err("Edit at most 64 macro steps at once.".into());
    }
    let mut result = Vec::new();
    for s in steps {
        // Reuse the worksheet-tool allowlist and validation used when saving native scripts.
        let mut step = if let Some(step) = flow::create(&s.tool, &s.args)? {
            step
        } else {
            let one = workbook::script_steps(app, &json!({"name":"draft","steps":[{"tool":s.tool,"args":s.args}]}))?;
            let c = &one["commands"][0];
            Step::command(c["command"].as_str().ok_or("Invalid step command.")?, c["params"].clone())
        };
        step.note = s.note.clone().unwrap_or_default();
        step.enabled = s.enabled.unwrap_or(true);
        step.breakpoint = s.breakpoint.unwrap_or(false);
        result.push(step);
    }
    Ok(result)
}

pub(super) fn execute(app: &mut SheetApp, name: &str, args: &Value, approved: bool) -> Result<Value, String> {
    if args.to_string().len() > MAX_ARGS {
        return Err("Macro draft arguments exceed 64 KiB.".into());
    }
    if name == "read_macro_draft" {
        return inspect(app, args);
    }
    if name != "edit_macro_draft" || !approved {
        return Err("A draft edit requires explicit approval.".into());
    }
    if !available(app) {
        return Err("The Macro Editor is no longer ready for edits.".into());
    }
    if app.session.active().is_none_or(|d| d.read_only_source.is_some() || d.wb.protected_structure) {
        return Err("Cannot edit a macro in a read-only or structure-protected workbook.".into());
    }
    let edit: Edit = serde_json::from_value(args.clone()).map_err(|_| "Invalid draft edit arguments.")?;
    let ed = app.macros.editor.as_ref().ok_or("The Macro Editor has closed.")?;
    let before = fingerprint(ed)?;
    if ticket(ed, before) != edit.revision {
        return Err("Macro draft changed. Read it again before proposing an edit.".into());
    }
    let mut draft = ed.draft.clone();
    if let Some(desc) = edit.description {
        draft.description = desc;
    }
    let mut selected = ed.selected.clone();
    if let Some(index) = edit.index {
        let list = macro_editor::list_mut(&mut draft.steps, &edit.parent).ok_or("Unknown parent step list.")?;
        let i = index.checked_sub(1).filter(|i| *i <= list.len()).ok_or("Step index is outside this draft.")?;
        if let Some(fields) = edit.fields {
            if edit.steps.is_some() || edit.remove.is_some() {
                return Err("Use fields or a splice, not both.".into());
            }
            flow::update(list.get_mut(i).ok_or("No step at this index to update.")?, &fields)?;
        } else {
            let insert = new_steps(app, edit.steps.as_deref().unwrap_or_default())?;
            let count = edit.remove.unwrap_or(0);
            if count > 64 || count > list.len() - i || (insert.is_empty() && count == 0) {
                return Err("Invalid number of steps to replace.".into());
            }
            list.splice(i..i + count, insert);
        }
        let n = index.min(list.len());
        selected = (n > 0).then(|| if edit.parent.is_empty() { n.to_string() } else { format!("{}.{n}", edit.parent) });
    } else if edit.fields.is_some() || edit.steps.is_some() || edit.remove.is_some() || !edit.parent.is_empty() {
        return Err("Specify an index to change macro steps.".into());
    }
    gridcraft_engine::macros::check(&draft)?;
    let ed = app.macros.editor.as_mut().ok_or("The Macro Editor has closed.")?;
    if draft == ed.draft {
        return Err("The proposed edit makes no change.".into());
    }
    if ed.wingman_history.expected != Some(before) {
        ed.wingman_history.undo.clear();
    }
    ed.wingman_history.redo.clear();
    let snapshot =
        Snapshot { draft: std::mem::replace(&mut ed.draft, draft), dirty: ed.dirty, selected: std::mem::replace(&mut ed.selected, selected) };
    ed.wingman_history.undo.push_back(snapshot);
    if ed.wingman_history.undo.len() > 8 {
        ed.wingman_history.undo.pop_front();
    }
    ed.dirty = true;
    invalidate(ed);
    let hash = fingerprint(ed)?;
    ed.wingman_history.expected = Some(hash);
    Ok(json!({"editedDraft":ed.draft.name,"revision":ticket(ed,hash),"saved":false,"executed":false}))
}

fn invalidate(ed: &mut Editor) {
    ed.check = None;
    ed.errors.clear();
    ed.outcome = None;
    ed.new_param = Default::default();
}

pub(crate) fn shift(ed: &mut Editor, redo: bool) -> Option<Result<Value, String>> {
    if !ready(ed) || (if redo { ed.wingman_history.redo.is_empty() } else { ed.wingman_history.undo.is_empty() }) {
        return None;
    }
    if fingerprint(ed).ok() != ed.wingman_history.expected {
        ed.wingman_history.undo.clear();
        ed.wingman_history.redo.clear();
        return Some(Err("The macro draft was changed outside Wingman; it has been kept as edited.".into()));
    }
    let prior = if redo { ed.wingman_history.redo.pop_back() } else { ed.wingman_history.undo.pop_back() }?;
    let current = Snapshot {
        draft: std::mem::replace(&mut ed.draft, prior.draft),
        dirty: std::mem::replace(&mut ed.dirty, prior.dirty),
        selected: std::mem::replace(&mut ed.selected, prior.selected),
    };
    if redo {
        ed.wingman_history.undo.push_back(current);
    } else {
        ed.wingman_history.redo.push_back(current);
    }
    invalidate(ed);
    ed.wingman_history.expected = fingerprint(ed).ok();
    Some(Ok(json!({"macroDraft":ed.draft.name, "dirty":ed.dirty})))
}

pub(crate) fn history(app: &mut SheetApp, id: &str, from_dock: bool) -> Option<Result<Value, String>> {
    if !matches!(id, "edit.undo" | "edit.redo") || !app.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") {
        return None;
    }
    let ed = app.macros.editor.as_ref()?;
    if !from_dock && !app.wingman.open && ed.wingman_history.undo.is_empty() && ed.wingman_history.redo.is_empty() {
        return None; // A standalone Macro Editor keeps its usual command handling.
    }
    if !available(app) {
        return Some(Err("Finish the current macro editor action before using draft history.".into()));
    }
    shift(app.macros.editor.as_mut()?, id == "edit.redo")
        .or_else(|| Some(Err("No draft edit to undo or redo. Close the Macro Editor to use workbook history for saved changes.".into())))
}

pub(crate) fn controls(app: &mut SheetApp, ui: &mut egui::Ui, ed: &mut Editor) {
    let l = app.ui.language;
    if app.ui.show_wingman_pane && ui.button(l.tr("Wingman")).on_hover_text(l.tr("Work on this macro draft with Wingman")).clicked() {
        app.run_or_alert("view.wingman", json!({"on":true}));
    }
    if !ready(ed) || app.session.macro_debug.is_some() || app.macros.question.is_some() || app.message.is_some() {
        return;
    }
    let keyboard = !ui.ctx().text_edit_focused();
    for (redo, label) in [(true, "Redo draft edit"), (false, "Undo draft edit")] {
        if if redo { ed.wingman_history.redo.is_empty() } else { ed.wingman_history.undo.is_empty() } {
            continue;
        }
        let pressed = keyboard
            && ui.input_mut(|i| {
                if redo {
                    i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)
                        || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
                } else {
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)
                }
            });
        let clicked = ui.button(l.tr(label)).clicked();
        if (pressed || clicked)
            && let Some(Err(e)) = shift(ed, redo)
        {
            ed.outcome = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_a_conditional_header_preserves_both_branches_and_their_flags() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.open_dialog("macroEditor", json!({"new":"Two ways"}));
        fn edit(app: &mut SheetApp, mut change: Value) -> Result<Value, String> {
            change["revision"] = inspect(app, &json!({}))?["revision"].clone();
            execute(app, "edit_macro_draft", &change, true)
        }
        edit(&mut app, json!({"index":1,"steps":[{"tool":"if","args":{"cell":"A1","op":"gt","value":7}}]})).unwrap();
        edit(&mut app, json!({"parent":"1.then","index":1,"steps":[{"tool":"repeat","breakpoint":true,"args":{"over":"Input.data","max":2}}]}))
            .unwrap();
        edit(&mut app, json!({"parent":"1.then.1","index":1,"steps":[{"tool":"stop","args":{"message":"hold","rollback":true}}]})).unwrap();
        edit(&mut app, json!({"parent":"1.else","index":1,"steps":[{"tool":"stop","args":{"message":"skip"}}]})).unwrap();
        let branch = |app: &SheetApp, path: &str| inspect(app, &json!({"parent":path})).unwrap()["steps"].clone();
        let left = branch(&app, "1.then");
        let right = branch(&app, "1.else");
        let nested = branch(&app, "1.then.1");
        edit(&mut app, json!({"index":1,"fields":{"op":"contains","value":"ok","note":"gate"}})).unwrap();
        assert_eq!(branch(&app, "1.then"), left);
        assert_eq!(branch(&app, "1.else"), right);
        assert_eq!(branch(&app, "1.then.1"), nested);
        assert_eq!(branch(&app, "")[0]["data"]["step"]["args"], json!({"cell":"A1","op":"contains","value":"ok"}));
        edit(&mut app, json!({"parent":"1.then","index":1,"fields":{"of":"columns","max":9}})).unwrap();
        assert_eq!(branch(&app, "1.then")[0]["data"]["breakpoint"], true);
        assert_eq!(branch(&app, "1.then.1"), nested);
        assert!(edit(&mut app, json!({"index":1,"fields":{"then":[]}})).is_err());
        assert!(edit(&mut app, json!({"parent":"1.then","index":1,"fields":{"cell":"B5"}})).is_err());
    }

    #[test]
    fn an_approved_macro_edit_updates_only_the_observed_draft_and_undo_restores_manual_work() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.open_dialog("macroEditor", json!({"new":"Report"}));
        let mut change = json!({"revision":inspect(&app,&json!({})).unwrap()["revision"],"description":"Agent revision"});
        app.macros.editor.as_mut().unwrap().draft.description = "Manual revision".into();
        assert!(execute(&mut app, "edit_macro_draft", &change, true).is_err());
        change["revision"] = inspect(&app, &json!({})).unwrap()["revision"].clone();
        assert!(execute(&mut app, "edit_macro_draft", &change, false).is_err());
        execute(&mut app, "edit_macro_draft", &change, true).unwrap();
        assert_eq!(app.macros.editor.as_ref().unwrap().draft.description, "Agent revision");
        assert!(app.session.active().unwrap().wb.macros.is_empty(), "editing a draft must not save or run it");
        history(&mut app, "edit.undo", true).unwrap().unwrap();
        assert_eq!(app.macros.editor.as_ref().unwrap().draft.description, "Manual revision");
        history(&mut app, "edit.redo", true).unwrap().unwrap();
        assert_eq!(app.macros.editor.as_ref().unwrap().draft.description, "Agent revision");
        // Reopen a different editor instance containing identical draft text.
        let revision = inspect(&app, &json!({})).unwrap()["revision"].clone();
        app.dialog = None;
        app.macros.editor = None;
        app.open_dialog("macroEditor", json!({"new":"Report"}));
        app.macros.editor.as_mut().unwrap().draft.description = "Agent revision".into();
        change["revision"] = revision;
        change["description"] = json!("Must not be applied to the reopened editor");
        assert!(execute(&mut app, "edit_macro_draft", &change, true).unwrap_err().contains("changed"));
        assert_eq!(app.macros.editor.as_ref().unwrap().draft.description, "Agent revision");
    }
}
