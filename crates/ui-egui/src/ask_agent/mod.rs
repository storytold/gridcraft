//! Ask Agent: the person selects cells and asks their own agent (connected over MCP or the control
//! channel) to work on them. `hub.rs` and `ui.rs` are shared with the other apps; this file is
//! GridCraft's part: the cells as JSON, the agent's edits as one undo step, the hold, its note.

pub mod hub;
pub mod ui;

use egui::{Key, Pos2, Rect, Vec2};
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::{Sheet, Workbook};
use serde_json::{Value, json};

pub use hub::{AgentHub, HELD, HubError, INVALID_PARAMS, NO_REQUEST, NOT_OWNER, READ_ONLY_REQUEST, Request, Snapshot, Status, Turn};
pub use ui::Action;

/// Editable starting points for requests to the connected agent.
const QUICK_ACTIONS: &[ui::QuickAction] = &[
    ui::QuickAction {
        label: "Clean up cells",
        group: "",
        instruction: "Clean up the selected cells: trim unnecessary spaces and make text capitalization consistent. Preserve numbers and formulas.",
    },
    ui::QuickAction {
        label: "Create formula",
        group: "",
        instruction: "Create a formula for the selected cells. Explain any assumptions and ask if the intended calculation is unclear.",
    },
    ui::QuickAction {
        label: "Explain formula",
        group: "",
        instruction: "Explain the formulas in the selected cells, including their references and results. Do not change the cells.",
    },
    ui::QuickAction {
        label: "Summarize selection",
        group: "",
        instruction: "Summarize the selected cells and highlight the key values or patterns. Do not change the cells.",
    },
    ui::QuickAction {
        label: "Readable table",
        group: "Format",
        instruction: "Format the selected cells as a readable table with clear headings and appropriate number formats. Preserve values and formulas.",
    },
    ui::QuickAction {
        label: "Highlight outliers",
        group: "Format",
        instruction: "Identify outliers in the selected numeric cells, explain the rule used, and highlight them without changing values.",
    },
];

use crate::SheetApp;

/// The open bubble: its current request, the thread of earlier turns, and the cells it is about.
#[derive(Clone, Debug)]
pub struct Open {
    pub id: Option<u64>,
    pub anchor: Rect,
    pub turns: Vec<Turn>,
    pub sheet: String,
    pub range: RangeRef,
    asked: String,
    /// Undo depth before the agent's first edit, once it has edited.
    edited_from: Option<usize>,
    commented: bool,
}

#[derive(Default)]
pub struct AskAgent {
    pub hub: Option<AgentHub>,
    pub open: Option<Open>,
    pub draft: String,
    pub picked: Option<String>,
    /// Set while a command from an agent runs, so the hold does not refuse it.
    agent_turn: bool,
}

impl AskAgent {
    pub fn inspect(&self) -> Value {
        let agents = self.hub.as_ref().map(|h| h.agents()).unwrap_or_default();
        json!({"open": self.open.is_some(), "requestId": self.open.as_ref().and_then(|o| o.id), "agents": agents})
    }
}

pub fn selection_json(wb: &Workbook, sh: &Sheet, sel: &gridcraft_engine::Selection) -> Value {
    let range = sel.current();
    let (values, values_range) = match sh.used_range().and_then(|u| range.intersection(&u)) {
        Some(area) => {
            let cols = (area.width() as usize).clamp(1, 100);
            let (max_rows, mut vals, mut bytes) = ((2000 / cols).max(1), Vec::new(), 0usize);
            for row in area.start.row..=area.end.row {
                if vals.len() >= max_rows || bytes >= 100_000 {
                    break;
                }
                vals.push(Value::Array(
                    (area.start.col..=(area.start.col + cols as u32 - 1))
                        .map(|col| {
                            let s: String = gridcraft_engine::display::cell_text(wb, sh, CellRef::new(row, col)).chars().take(1000).collect();
                            bytes += s.len();
                            Value::String(s)
                        })
                        .collect(),
                ));
            }
            (Value::Array(vals), Value::String(area.a1()))
        }
        None => (Value::Array(Vec::new()), Value::Null),
    };
    json!({"sheet": sh.name, "range": range.a1(), "areas": sel.ranges.iter().take(100).map(RangeRef::a1).collect::<Vec<_>>(), "active": sel.active.a1(), "values": values, "valuesRange": values_range})
}

fn document_json(app: &SheetApp) -> Value {
    app.session
        .active()
        .map_or_else(|| json!({"title": null, "saved": false}), |d| json!({"id": d.uid, "title": d.display_title(), "saved": d.path.is_some()}))
}

/// Open the bubble at the selection (chip, ribbon, context menu, shortcut).
pub fn open_from_ui(app: &mut SheetApp) {
    let (Some(d), Some(sh)) = (app.session.active(), app.session.active().and_then(|d| d.wb.active())) else { return };
    if app.ask_agent.hub.is_none() {
        return;
    }
    let range = d.selection.current();
    let anchor = app.grid.geo.as_ref().map_or(Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(100.0, 24.0)), |g| g.range_rect(sh, range));
    let sheet = sh.name.clone();
    app.ask_agent.draft.clear();
    app.ask_agent.open = Some(Open { id: None, anchor, turns: vec![], sheet, range, asked: String::new(), edited_from: None, commented: false });
}

/// Control methods that only read.
pub fn is_query_method(cmd: &str) -> bool {
    matches!(
        cmd,
        "cell.get" | "sheet.read" | "selection.stats" | "document.inspect" | "engine.commands" | "engine.journal" | "ui.inspect" | "ui.screenshot"
    )
}

/// Run `command` for request `request_id` from connection `conn`: its edits join one undo step and
/// its comments and notes are authored "Agent (<name>)".
pub fn run_for(app: &mut SheetApp, conn: u64, request_id: u64, command: &str, mut params: Value) -> Result<Value, String> {
    let hub = app.ask_agent.hub.clone().ok_or("no agent hub")?;
    hub.may_edit(conn, Some(request_id)).map_err(|e| e.to_string())?;
    let name = hub.snapshot(request_id).and_then(|s| s.agent).unwrap_or_default();
    if command.starts_with("review.new")
        && let Some(o) = params.as_object_mut()
    {
        o.entry("author").or_insert(json!(format!("Agent ({name})")));
    }
    let depth = app.session.active().map_or(0, |d| d.undo.len());
    let from = match app.ask_agent.open.as_mut().filter(|o| o.id == Some(request_id)) {
        Some(o) => *o.edited_from.get_or_insert(depth),
        None => depth,
    };
    app.ask_agent.agent_turn = true;
    let result = app.run(command, params);
    app.ask_agent.agent_turn = false;
    if let Some(d) = app.session.active_mut()
        && d.undo.len() > from
    {
        // One undo step for everything the agent did for this thread.
        d.undo.truncate(from + 1);
        d.undo[from].label = "Ask Agent".into();
    }
    result
}

pub fn read_for(app: &mut SheetApp, conn: u64, request_id: u64, method: &str, params: Value) -> Result<Value, String> {
    app.ask_agent.hub.as_ref().ok_or("no agent hub")?.may_edit(conn, Some(request_id)).map_err(|e| e.to_string())?;
    app.session.run(method, params)
}

/// The cells each agent is working on, with the agent's colour.
pub fn held_ranges(app: &SheetApp) -> Vec<(String, RangeRef, [u8; 3])> {
    let Some(hub) = &app.ask_agent.hub else { return vec![] };
    let range = |sel: &Value| Some((sel["sheet"].as_str()?.to_string(), RangeRef::parse(sel["range"].as_str()?)?));
    hub.held().into_iter().filter_map(|(sel, name)| range(&sel).map(|(sheet, r)| (sheet, r, hub::colour(&name)))).collect()
}

/// Why the person may not run `command` targeting `target` now: it would change cells an agent is working on.
pub fn held_for(app: &SheetApp, command: &str, target: Option<RangeRef>) -> Option<String> {
    let hub = app.ask_agent.hub.as_ref().filter(|_| !app.ask_agent.agent_turn)?;
    if is_query_method(command) || gridcraft_engine::find_command(command).is_none() || command.starts_with("select") {
        return None;
    }
    let d = app.session.active()?;
    let sheet = d.wb.active()?.name.clone();
    let mine = target.unwrap_or_else(|| d.selection.current());
    hub.held().into_iter().find_map(|(sel, name)| {
        let theirs = RangeRef::parse(sel["range"].as_str()?)?;
        (sel["sheet"].as_str() == Some(sheet.as_str()) && theirs.intersection(&mine).is_some()).then(|| format!("Held by Agent ({name})"))
    })
}

/// Why the person may not run `command` now: it would change cells an agent is working on.
pub fn held(app: &SheetApp, command: &str) -> Option<String> {
    held_for(app, command, None)
}

fn ask(app: &mut SheetApp, instruction: String) {
    let (Some(hub), Some(d)) = (app.ask_agent.hub.clone(), app.session.active()) else { return };
    let Some(sh) = d.wb.active() else { return };
    let (selection, document) = (selection_json(&d.wb, sh, &d.selection), document_json(app));
    let agent = app.ask_agent.picked.clone();
    let Some(open) = &mut app.ask_agent.open else { return };
    // After an answer, this is a follow-up: the thread so far goes with it.
    if let Some(answer) = open.id.and_then(|id| hub.snapshot(id)).and_then(|s| s.answer) {
        open.turns.push(Turn { instruction: std::mem::take(&mut open.asked), answer });
        open.id.take().into_iter().for_each(|id| hub.close(id));
    }
    open.asked = instruction.clone();
    open.commented = false;
    open.id = Some(hub.ask(Request { id: 0, instruction, history: open.turns.clone(), selection, document, agent }));
}

fn close(app: &mut SheetApp) {
    if let (Some(hub), Some(id)) = (&app.ask_agent.hub, app.ask_agent.open.take().and_then(|o| o.id)) {
        hub.close(id);
    }
}

/// `ui.askAgent {instruction?, close?}`: open the bubble at the selection, and send right away when
/// an instruction is given. Keep and Undo are the person's, in the window only.
pub fn command(app: &mut SheetApp, p: &Value) -> Result<Value, String> {
    if ["accept", "discard", "keep", "undo"].iter().any(|k| p.get(*k).is_some()) {
        return Err("HUMAN_REVIEW_REQUIRED: Keep and Undo are the person's, in the window".into());
    }
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        close(app);
        return Ok(json!({"open": false}));
    }
    open_from_ui(app);
    if app.ask_agent.open.is_none() {
        return Err("no document open or no agent hub".into());
    }
    if let Some(text) = p.get("instruction").and_then(Value::as_str) {
        ask(app, text.to_string());
    }
    Ok(json!({"open": true, "id": app.ask_agent.open.as_ref().and_then(|o| o.id)}))
}

/// Esc closes the bubble.
pub fn keys(app: &mut SheetApp, ctx: &egui::Context) {
    if app.ask_agent.open.is_some() && ctx.input(|i| i.key_pressed(Key::Escape)) {
        close(app);
    }
}

/// The agent's answer, once, as a note on the first cell it was asked about, authored like its edits.
fn note_once(app: &mut SheetApp, s: &Snapshot) {
    let Some(open) = app.ask_agent.open.as_mut().filter(|o| !o.commented) else { return };
    open.commented = true;
    let (Some(answer), Some(name)) = (s.answer.clone(), s.agent.clone()) else { return };
    let (sheet, at) = (open.sheet.clone(), open.range.start);
    let cell = at.a1();
    let text = format!("You: {}\n\nAgent ({name}): {answer}", open.asked);
    let depth = app.session.active().map_or(0, |d| d.undo.len());
    let from = *open.edited_from.get_or_insert(depth);
    let existing = app
        .session
        .active()
        .is_some_and(|d| d.wb.sheet_index(&sheet).and_then(|index| d.wb.sheet(index)).is_some_and(|sh| sh.comments.contains_key(&at)));
    let params = json!({"sheet": sheet, "cell": cell, "text": text, "author": format!("Agent ({name})")});
    app.ask_agent.agent_turn = true;
    let _ = app.session.run(if existing { "review.replyComment" } else { "review.newComment" }, params);
    app.ask_agent.agent_turn = false;
    if let Some(d) = app.session.active_mut()
        && d.undo.len() > from + 1
    {
        d.undo.truncate(from + 1);
    }
}

/// The chip beside the selection, or the open bubble with its request.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let (Some(hub), Some(d)) = (app.ask_agent.hub.clone(), app.session.active()) else { return };
    let Some(sh) = d.wb.active() else { return };
    let Some(open) = &app.ask_agent.open else {
        let anchor = app.grid.geo.as_ref().map(|g| g.range_rect(sh, d.selection.current()));
        if let Some(anchor) = anchor
            && !hub.agents().is_empty()
            && ui::chip(ctx, anchor)
        {
            open_from_ui(app);
        }
        return;
    };
    let (anchor, snapshot) = (open.anchor, open.id.and_then(|id| hub.snapshot(id)));
    if snapshot.as_ref().is_some_and(|s| matches!(s.status, Status::Queued | Status::Working { .. })) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(s) = snapshot.as_ref().filter(|s| s.status == Status::Answered) {
        note_once(app, s);
    }
    let agents = hub.agents();
    let (history, instruction) = app.ask_agent.open.as_ref().map(|open| (open.turns.clone(), open.asked.clone())).unwrap_or_default();
    let view = ui::View { snapshot: snapshot.as_ref(), agents: &agents, actions: QUICK_ACTIONS, history: &history, instruction: &instruction };
    match ui::bubble(ctx, anchor, &view, &mut app.ask_agent.draft, &mut app.ask_agent.picked) {
        Some(Action::Send(text)) => ask(app, text),
        Some(Action::Undo) => {
            let from = app.ask_agent.open.as_ref().and_then(|o| o.edited_from);
            if let Some(from) = from {
                if app.session.active().is_some_and(|d| d.undo.len() == from + 1) {
                    let _ = app.run("edit.undo", json!({}));
                } else {
                    app.toast = Some(("The cells changed since the agent's edits; use Undo".into(), crate::now_ms()));
                }
            }
            close(app);
        }
        Some(Action::Keep | Action::Close) => close(app),
        None => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn comments_keep_both_instructions_and_answers() {
        let hub = AgentHub::default();
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default()).with_agent_hub(hub.clone());
        command(&mut app, &json!({"instruction": "Clean these cells"})).unwrap();
        let first = hub.listen(1, Some("Codex"), Duration::ZERO, || true).unwrap();
        hub.reply(1, first.id, "Cleaned", true, None).unwrap();
        note_once(&mut app, &hub.snapshot(first.id).unwrap());
        ask(&mut app, "Add a total".into());
        let next = hub.listen(1, Some("Codex"), Duration::ZERO, || true).unwrap();
        hub.reply(1, next.id, "Total added", true, None).unwrap();
        note_once(&mut app, &hub.snapshot(next.id).unwrap());
        note_once(&mut app, &hub.snapshot(next.id).unwrap());
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let comment = sheet.comments.values().next().unwrap();
        assert_eq!(comment.text, "You: Clean these cells\n\nAgent (Codex): Cleaned");
        assert_eq!(comment.replies.len(), 1);
        assert_eq!(comment.replies[0].0, "Agent (Codex)");
        assert_eq!(comment.replies[0].1, "You: Add a total\n\nAgent (Codex): Total added");
        close(&mut app);
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.active().unwrap().wb.active().unwrap().comments.is_empty());
    }
}
