//! Ask Agent: the person selects cells and asks their own agent (connected over MCP or the control
//! channel) to work on them. `hub.rs` and `ui.rs` are shared with the other apps; this file is
//! GridCraft's part: captured cells, document ownership, consecutive undo groups, holds and response notes.

pub mod hub;
pub mod ui;

use egui::{Key, Pos2, Rect, Vec2};
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::{Sheet, Workbook};
use serde_json::{Value, json};

pub use hub::{AgentHub, HELD, HubError, INVALID_PARAMS, NO_REQUEST, NOT_OWNER, READ_ONLY_REQUEST, Reply, Request, Snapshot, Status, Turn};
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
    document: u64,
    selection: gridcraft_engine::Selection,
    edited_revision: Option<u64>,
    review_safe: bool,
    /// Undo depth before the current uninterrupted group of agent edits.
    edited_from: Option<usize>,
    /// Undo depth just after the thread's last merged step, to tell whether a person edited since.
    edited_to: Option<usize>,
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
    #[cfg(not(target_arch = "wasm32"))]
    launched: Option<std::process::Child>,
}

impl AskAgent {
    pub(crate) fn agent_turn_active(&self) -> bool {
        self.agent_turn
    }
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
    // A bubble that owns a request keeps it: replacing it would leave the request in the hub with
    // nothing to close it, and every write without its id would stay refused as held.
    if app.ask_agent.open.as_ref().is_some_and(|o| o.id.is_some()) {
        return;
    }
    let (Some(d), Some(sh)) = (app.session.active(), app.session.active().and_then(|d| d.wb.active())) else { return };
    if app.ask_agent.hub.is_none() {
        return;
    }
    let range = d.selection.current();
    let anchor = app.grid.geo.as_ref().map_or(Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(100.0, 24.0)), |g| g.range_rect(sh, range));
    let sheet = sh.name.clone();
    app.ask_agent.draft.clear();
    app.ask_agent.open = Some(Open {
        id: None,
        anchor,
        turns: vec![],
        sheet,
        range,
        document: d.uid,
        selection: d.selection.clone(),
        edited_revision: Some(d.revision),
        review_safe: true,
        asked: String::new(),
        edited_from: None,
        edited_to: None,
        commented: false,
    });
}

/// Control methods that only read.
pub fn is_query_method(cmd: &str) -> bool {
    matches!(
        cmd,
        "cell.get" | "sheet.read" | "selection.stats" | "document.inspect" | "engine.commands" | "engine.journal" | "ui.inspect" | "ui.screenshot"
    )
}

/// Where this thread's next undo step starts: `depth` before the command, unless that thread
/// already has a group the history no longer reaches, in which case a person edited in between
/// and their step starts a group of its own.
fn group_start(app: &SheetApp, request: u64) -> usize {
    let Some(d) = app.session.active() else { return 0 };
    app.ask_agent
        .open
        .as_ref()
        .filter(|o| o.id == Some(request) && o.document == d.uid && o.edited_to == Some(d.undo.len()) && o.edited_revision == Some(d.revision))
        .and_then(|o| o.edited_from)
        .unwrap_or(d.undo.len())
}

/// Collapse only consecutive agent changes; retain a conservative history-memory estimate.
fn group_done(app: &mut SheetApp, request: u64, from: usize, before: u64, depth: usize) {
    let Some(d) = app.session.active_mut() else { return };
    if d.revision == before {
        return;
    }
    if d.undo.len() <= from {
        if let Some(o) = app.ask_agent.open.as_mut().filter(|o| o.id == Some(request) && o.document == d.uid) {
            o.review_safe = false;
            o.edited_revision = Some(d.revision);
            o.edited_to = Some(d.undo.len());
        }
        return;
    }
    let Some(o) = app.ask_agent.open.as_mut().filter(|o| o.id == Some(request) && o.document == d.uid) else { return };
    if o.edited_to.is_some() && (o.edited_revision != Some(before) || o.edited_to != Some(depth)) {
        o.review_safe = false;
    }
    let retained = d.undo.iter().skip(from).fold(0usize, |sum, e| sum.saturating_add(e.retained_bytes));
    d.undo.truncate(from + 1);
    if let Some(entry) = d.undo.get_mut(from) {
        entry.label = "Ask Agent".into();
        entry.retained_bytes = retained;
    }
    o.edited_from = Some(from);
    o.edited_to = Some(d.undo.len());
    o.edited_revision = Some(d.revision);
}

/// Current document token, obtained only from the document owner.
pub fn current_target(app: &SheetApp) -> Option<hub::Target> {
    app.session.active().map(|d| hub::Target { document: d.uid.to_string(), revision: d.revision })
}

/// Route replies on the UI owner, serializing live revision validation and acceptance.
pub fn handle_reply(app: &SheetApp, conn: u64, params: &Value) -> Value {
    let Some(hub) = &app.ask_agent.hub else { return json!({"ok":false,"error":"No agent hub","errorCode":"NO_REQUEST"}) };
    hub.handle(
        conn,
        "agent.reply",
        params,
        || false,
        |expected| {
            app.session
                .documents()
                .iter()
                .find(|d| d.uid.to_string() == expected.document)
                .map(|d| hub::Target { document: d.uid.to_string(), revision: d.revision })
        },
    )
    .unwrap_or(Value::Null)
}

/// Check the actual command geometry against the immutable captured selection.
fn contains_command(selection: &Value, command: &str, params: &Value, fallback: RangeRef, array_range: Option<RangeRef>) -> bool {
    let Some(sheet) = selection["sheet"].as_str() else { return false };
    if params.get("sheet").is_some_and(|v| v.as_str() != Some(sheet)) {
        return false;
    }
    let areas: Vec<_> = selection["areas"].as_array().into_iter().flatten().filter_map(|v| v.as_str().and_then(RangeRef::parse)).collect();
    if areas.is_empty() {
        return false;
    }
    if is_query_method(command) {
        return true;
    }
    let allowed = matches!(
        command,
        "cell.set"
            | "range.setValues"
            | "range.fill"
            | "edit.clearContents"
            | "edit.clearFormats"
            | "review.newComment"
            | "review.replyComment"
            | "review.newNote"
            | "home.bold"
            | "home.italic"
            | "home.underline"
            | "home.strikethrough"
            | "home.fontName"
            | "home.fontSize"
            | "home.fontColor"
            | "home.fillColor"
            | "home.numberFormat"
            | "home.alignLeft"
            | "home.alignCenter"
            | "home.alignRight"
            | "home.justify"
            | "home.wrapText"
            | "home.borders"
            | "home.cellStyle"
    );
    if !allowed {
        return false;
    }
    let cell_command = command == "cell.set" || command.starts_with("review.");
    let actual = if cell_command { params.get("cell") } else { params.get("range").or_else(|| params.get("cell")) };
    let target = if let Some(v) = actual {
        let Some(r) = v.as_str().and_then(RangeRef::parse) else { return false };
        r
    } else {
        fallback
    };
    let target = if command == "cell.set" && params.get("array") == Some(&Value::Bool(true)) {
        array_range.filter(|r| r.contains(target.start)).unwrap_or(target)
    } else if command == "range.setValues" {
        let Some(rows) = params.get("values").and_then(Value::as_array) else { return false };
        let width = rows.iter().filter_map(Value::as_array).map(Vec::len).max().unwrap_or(0);
        if rows.is_empty() || width == 0 || rows.iter().any(|r| r.as_array().is_none()) {
            return false;
        }
        let Some(end) = target.start.offset(rows.len() as i64 - 1, width as i64 - 1) else { return false };
        RangeRef::new(target.start, end)
    } else {
        target
    };
    areas.iter().any(|area| area.contains_range(&target))
}

/// Run an owned command against the real request document, checking scope before mutation.
pub fn run_for(app: &mut SheetApp, conn: u64, request: u64, command: &str, mut params: Value) -> Result<Value, String> {
    if params.is_null() {
        params = json!({});
    }
    if !params.is_object() {
        return Err("INVALID_PARAMS: command params must be an object".into());
    }
    let hub = app.ask_agent.hub.clone().ok_or("NO_REQUEST: no agent hub")?;
    let routing = hub.request_target(request).ok_or("NO_REQUEST: unknown request")?;
    let uid = routing.document.parse::<u64>().map_err(|_| "STALE_REQUEST: invalid request document")?;
    let guard = app.session.enter_target(gridcraft_engine::Target::Doc(uid)).map_err(|e| format!("STALE_REQUEST: {e}"))?;
    let result = (|| {
        let current = current_target(app).ok_or("STALE_REQUEST: document closed")?;
        let captured =
            app.ask_agent.open.as_ref().filter(|o| o.id == Some(request) && o.document == uid).ok_or("NO_REQUEST: no matching review thread")?;
        let fallback = captured.range;
        let array_range = app.session.active().filter(|d| d.wb.active().is_some_and(|sh| sh.name == captured.sheet)).map(|d| d.selection.current());
        let name = hub.snapshot(request).and_then(|s| s.agent).unwrap_or_default();
        if let Some(p) = params.as_object_mut() {
            let index = app.session.active().and_then(|d| d.wb.sheet_index(&captured.sheet));
            if p.get("sheet").and_then(Value::as_u64).is_some_and(|i| Some(i as usize) == index) {
                p.insert("sheet".into(), json!(captured.sheet));
            }
            p.entry("sheet").or_insert(json!(captured.sheet));
            if matches!(command, "cell.set" | "cell.get") || command.starts_with("review.") {
                p.entry("cell").or_insert(json!(fallback.start.a1()));
            } else {
                p.entry("range").or_insert(json!(fallback.a1()));
            }
            if command.starts_with("review.") {
                p.insert("author".into(), json!(format!("Agent ({name})")));
            }
        }
        let selection = json!({"documentId":uid,"sheet":captured.sheet,"range":captured.range.a1(),"areas":captured.selection.ranges.iter().map(RangeRef::a1).collect::<Vec<_>>()});
        hub.may_edit(conn, Some(request), &current, |sel| sel["documentId"] == uid && contains_command(sel, command, &params, fallback, array_range))
            .map_err(|e| e.to_string())?;
        let depth = app.session.active().map_or(0, |d| d.undo.len());
        let from = group_start(app, request);
        app.ask_agent.agent_turn = true;
        let result = app.run(command, params);
        app.ask_agent.agent_turn = false;
        let next = current_target(app).ok_or("STALE_REQUEST: document closed")?;
        if next.revision != current.revision {
            group_done(app, request, from, current.revision, depth);
            hub.advance(conn, request, &current, next, selection).map_err(|e| e.to_string())?;
        }
        result
    })();
    app.session.leave_target(guard);
    result
}

/// Request-scoped reads use the same document routing and owner validation.
pub fn read_for(app: &mut SheetApp, conn: u64, request: u64, method: &str, params: Value) -> Result<Value, String> {
    if !is_query_method(method) {
        return Err("OUTSIDE_SELECTION: request-scoped UI actions are not document queries".into());
    }
    run_for(app, conn, request, method, params)
}

/// The cells each agent is working on, with the agent's colour.
pub fn held_ranges(app: &SheetApp) -> Vec<(String, RangeRef, [u8; 3])> {
    let Some(hub) = &app.ask_agent.hub else { return vec![] };
    let range = |sel: &Value| Some((sel["sheet"].as_str()?.to_string(), RangeRef::parse(sel["range"].as_str()?)?));
    hub.held()
        .into_iter()
        .filter(|(sel, _)| app.session.active().is_some_and(|d| sel["documentId"] == d.uid))
        .filter_map(|(sel, name)| range(&sel).map(|(sheet, r)| (sheet, r, hub::colour(&name))))
        .collect()
}

/// Why the person may not run `command` targeting `target` now: it would change cells an agent is working on.
pub fn held_for(app: &SheetApp, command: &str, target: Option<RangeRef>) -> Option<String> {
    let hub = app.ask_agent.hub.as_ref().filter(|_| !app.ask_agent.agent_turn)?;
    if is_query_method(command)
        || command.starts_with("selection.")
        || gridcraft_engine::find_command(command).is_none()
        || !matches!(command, "edit.undo" | "edit.redo") && gridcraft_engine::find_command(command).is_some_and(|spec| !spec.journal)
    {
        return None;
    }
    let d = app.session.active()?;
    let sheet = d.wb.active()?.name.clone();
    let mine = target.unwrap_or_else(|| d.selection.current());
    hub.held().into_iter().find_map(|(sel, name)| {
        if sel["documentId"] != d.uid {
            return None;
        }
        if matches!(command, "edit.undo" | "edit.redo") {
            return Some(format!("Held by Agent ({name})"));
        }
        let theirs = RangeRef::parse(sel["range"].as_str()?)?;
        (sel["sheet"].as_str() == Some(sheet.as_str()) && theirs.intersection(&mine).is_some()).then(|| format!("Held by Agent ({name})"))
    })
}

/// Why the person may not run `command` now: it would change cells an agent is working on.
pub fn held(app: &SheetApp, command: &str) -> Option<String> {
    held_for(app, command, None)
}

/// Queue a bounded request for the captured document and selection, preserving any busy claim.
fn ask(app: &mut SheetApp, instruction: String) -> Result<(), String> {
    let hub = app.ask_agent.hub.clone().ok_or("NO_REQUEST: no agent hub")?;
    let open = app.ask_agent.open.as_ref().ok_or("NO_REQUEST: no open thread")?;
    let d = app.session.active().filter(|d| d.uid == open.document).ok_or("STALE_REQUEST: activate the request document first")?;
    if open.id.and_then(|id| hub.snapshot(id)).is_some_and(|s| matches!(s.status, Status::Queued | Status::Working { .. })) {
        return Err("HELD: this thread already owns an unfinished request".into());
    }
    if open.selection.ranges.len() > 100 {
        return Err("INVALID_PARAMS: select at most 100 areas for one request".into());
    }
    let sh = d.wb.sheet_index(&open.sheet).and_then(|i| d.wb.sheet(i)).ok_or("STALE_REQUEST: requested sheet no longer exists")?;
    let mut selection = selection_json(&d.wb, sh, &open.selection);
    selection["documentId"] = json!(d.uid);
    let target = hub::Target { document: d.uid.to_string(), revision: d.revision };
    let document = document_json(app);
    let mut history = open.turns.clone();
    if let Some(answer) = open.id.and_then(|id| hub.snapshot(id)).filter(|s| s.status == Status::Answered).map(|s| s.review_text()) {
        history.push(Turn { instruction: open.asked.clone(), answer });
    }
    let asked_revision = target.revision;
    let id = hub
        .ask(Request {
            id: 0,
            instruction: instruction.clone(),
            history: history.clone(),
            selection,
            document,
            target,
            agent: app.ask_agent.picked.clone(),
        })
        .map_err(|e| e.to_string())?;
    let Some(open) = &mut app.ask_agent.open else {
        hub.close(id);
        return Err("NO_REQUEST: thread closed".into());
    };
    if let Some(previous) = open.id.replace(id) {
        hub.close(previous);
    }
    if open.edited_to.is_none() {
        open.edited_revision = Some(asked_revision);
    }
    open.turns = history;
    open.asked = instruction;
    open.commented = false;
    #[cfg(not(target_arch = "wasm32"))]
    launch_if_nobody(app, &hub);
    Ok(())
}

/// Resolve only the person's explicit launcher, without model settings or shell expansion.
#[cfg(not(target_arch = "wasm32"))]
fn launcher() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("GRIDCRAFT_AGENT_LAUNCH").filter(|p| !p.is_empty()) {
        return Some(path.into());
    }
    let name = if cfg!(windows) { "launch.cmd" } else { "launch" };
    let path = std::env::current_exe().ok()?.parent()?.join("agent").join(name);
    path.is_file().then_some(path)
}

/// Whether native configuration supplies a launcher; the browser never starts programs.
fn can_launch() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        launcher().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// Start the configured program once; reap its previous process before a later launch.
#[cfg(not(target_arch = "wasm32"))]
fn launch_if_nobody(app: &mut SheetApp, hub: &AgentHub) {
    if !hub.agents().is_empty() {
        return;
    }
    let Some(program) = launcher() else { return };
    if let Err(e) = start_launcher(&mut app.ask_agent.launched, &program) {
        app.toast = Some((format!("Could not start your agent: {e}"), crate::now_ms()));
    }
}

/// One configured executable at a time, with no shell interpolation or inherited input/output.
#[cfg(not(target_arch = "wasm32"))]
fn start_launcher(slot: &mut Option<std::process::Child>, program: &std::path::Path) -> std::io::Result<bool> {
    if let Some(child) = slot
        && child.try_wait()?.is_none()
    {
        return Ok(false);
    }
    let mut command = std::process::Command::new(program);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    *slot = Some(command.spawn()?);
    Ok(true)
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
    if p.get("close").and_then(Value::as_bool) == Some(true)
        && p.get("_externalAgent") == Some(&Value::Bool(true))
        && app.ask_agent.open.as_ref().is_some_and(|o| o.id.is_some())
    {
        return Err("HUMAN_REVIEW_REQUIRED: closing a request's review is the person's action in the window".into());
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
        ask(app, text.to_string())?;
    }
    Ok(json!({"open": true, "id": app.ask_agent.open.as_ref().and_then(|o| o.id)}))
}

/// Esc closes the bubble.
pub fn keys(app: &mut SheetApp, ctx: &egui::Context) {
    if app.ask_agent.open.as_ref().is_some_and(|o| app.session.active().is_some_and(|d| o.document == d.uid))
        && ctx.input(|i| i.key_pressed(Key::Escape))
    {
        close(app);
    }
}

/// The agent's answer, once, as a note on the first cell it was asked about, authored like its edits.
fn note_once(app: &mut SheetApp, s: &Snapshot) {
    let Some(open) = app.ask_agent.open.as_mut().filter(|o| !o.commented) else { return };
    if app.session.active().is_none_or(|d| d.uid != open.document) {
        return;
    }
    let (answer, Some(name)) = (s.review_text(), s.agent.clone()) else { return };
    let (sheet, at) = (open.sheet.clone(), open.range.start);
    let cell = at.a1();
    let text = format!("You: {}\n\nAgent ({name}): {answer}", open.asked);
    let depth = app.session.active().map_or(0, |d| d.undo.len());
    let from = group_start(app, s.id);
    let before = app.session.active().map_or(0, |d| d.revision);
    let existing = app
        .session
        .active()
        .is_some_and(|d| d.wb.sheet_index(&sheet).and_then(|index| d.wb.sheet(index)).is_some_and(|sh| sh.comments.contains_key(&at)));
    let params = json!({"sheet": sheet, "cell": cell, "text": text, "author": format!("Agent ({name})")});
    app.ask_agent.agent_turn = true;
    let result = app.session.run(if existing { "review.replyComment" } else { "review.newComment" }, params);
    app.ask_agent.agent_turn = false;
    if result.is_ok() {
        if let Some(o) = &mut app.ask_agent.open {
            o.commented = true;
        }
        group_done(app, s.id, from, before, depth);
    }
}

/// Review actions require the same document and the exact uninterrupted agent history.
fn review_allowed(app: &SheetApp) -> bool {
    app.ask_agent.open.as_ref().is_some_and(|o| {
        app.session.active().is_some_and(|d| {
            d.uid == o.document && o.review_safe && o.edited_revision.is_none_or(|r| r == d.revision) && o.edited_to.is_none_or(|n| n == d.undo.len())
        })
    })
}

/// The chip beside the selection, or the open bubble with its request.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    // Closing the original document also releases its claim; it cannot be requeued forever.
    if app.ask_agent.open.as_ref().is_some_and(|o| !app.session.documents().iter().any(|d| d.uid == o.document)) {
        close(app);
    }
    let (Some(hub), Some(d)) = (app.ask_agent.hub.clone(), app.session.active()) else { return };
    hub.invalidate(&hub::Target { document: d.uid.to_string(), revision: d.revision });
    let Some(sh) = d.wb.active() else { return };
    let Some(open) = &app.ask_agent.open else {
        let anchor = app.grid.geo.as_ref().map(|g| g.range_rect(sh, d.selection.current()));
        if let Some(anchor) = anchor
            && (!hub.agents().is_empty() || can_launch())
            && ui::chip(ctx, anchor, &|text| crate::i18n::current(ctx).tr(text).to_string())
        {
            open_from_ui(app);
        }
        return;
    };
    if open.document != d.uid {
        return;
    }
    let (anchor, snapshot) = (open.anchor, open.id.and_then(|id| hub.snapshot(id)));
    if snapshot.as_ref().is_some_and(|s| matches!(s.status, Status::Queued | Status::Working { .. })) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(s) = snapshot.as_ref().filter(|s| s.status == Status::Answered || s.status == Status::Failed && review_allowed(app)) {
        note_once(app, s);
    }
    let agents = hub.agents();
    let (history, instruction) = app.ask_agent.open.as_ref().map(|open| (open.turns.clone(), open.asked.clone())).unwrap_or_default();
    let view = ui::View { snapshot: snapshot.as_ref(), agents: &agents, actions: QUICK_ACTIONS, history: &history, instruction: &instruction };
    match ui::bubble(ctx, anchor, &view, &mut app.ask_agent.draft, &mut app.ask_agent.picked, &|text| crate::i18n::current(ctx).tr(text).to_string())
    {
        Some(Action::Send(text)) => {
            if let Err(e) = ask(app, text) {
                app.toast = Some((e, crate::now_ms()));
            }
        }
        Some(Action::Undo) => {
            let from = app.ask_agent.open.as_ref().and_then(|o| o.edited_from);
            if let Some(from) = from {
                if review_allowed(app) && app.session.active().is_some_and(|d| d.undo.len() == from + 1) {
                    if app.run("edit.undo", json!({})).is_ok() {
                        close(app);
                    }
                } else {
                    app.toast = Some(("The cells changed since the agent's edits; use Undo".into(), crate::now_ms()));
                }
            } else if review_allowed(app) {
                close(app);
            }
        }
        Some(Action::Keep) => {
            if review_allowed(app) {
                close(app);
            } else {
                app.toast = Some(("The cells changed since the agent edits; review the document first".into(), crate::now_ms()));
            }
        }
        Some(Action::Close) => close(app),
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
        let first = hub.listen(1, Some("Codex"), Duration::ZERO, || true).unwrap().unwrap();
        hub.reply(1, first.id, Reply { text: "Cleaned", ..Default::default() }, current_target(&app).as_ref()).unwrap();
        note_once(&mut app, &hub.snapshot(first.id).unwrap());
        ask(&mut app, "Add a total".into()).unwrap();
        let next = hub.listen(1, Some("Codex"), Duration::ZERO, || true).unwrap().unwrap();
        hub.reply(1, next.id, Reply { text: "Total added", ..Default::default() }, current_target(&app).as_ref()).unwrap();
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

    /// The request names its workbook: when another one becomes active, its command still lands
    /// in the one it was asked about.
    #[test]
    fn a_requests_command_runs_in_the_workbook_it_was_asked_about() {
        let hub = AgentHub::default();
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let first_uid = session.active().unwrap().uid;
        let mut app = SheetApp::new(session, Default::default()).with_agent_hub(hub.clone());
        app.run("selection.set", json!({"range":"B2"})).unwrap();
        command(&mut app, &json!({"instruction": "Fill B2"})).unwrap();
        let request = hub.listen(1, Some("Codex"), Duration::ZERO, || true).unwrap().unwrap();
        assert_eq!(request.document["id"].as_u64(), Some(first_uid), "the request carries its document");

        // A second workbook becomes active before the agent's command arrives.
        app.session.new_workbook();
        let second_uid = app.session.active().unwrap().uid;
        assert_ne!(second_uid, first_uid);
        run_for(&mut app, 1, request.id, "cell.set", json!({"cell": "B2", "input": "north"})).unwrap();

        let text = |app: &mut SheetApp, uid: u64| -> String {
            let p = json!({"doc": uid, "cell": "B2"});
            let guard = app.session.enter_target(gridcraft_engine::Target::from_json(&p).unwrap()).unwrap();
            let r = app.session.run("cell.get", p).map(|v| v["text"].as_str().unwrap_or_default().to_string());
            app.session.leave_target(guard);
            r.unwrap_or_default()
        };
        assert_eq!(text(&mut app, first_uid), "north", "the edit landed in the requested workbook");
        assert_eq!(text(&mut app, second_uid), "", "the active workbook was left alone");
    }
    #[cfg(unix)]
    #[test]
    /// Check that a live child is retained and an exited child may be replaced.
    fn a_configured_launcher_is_started_only_once_while_alive() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("gridcraft-agent-{}-launch test.sh", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut slot = None;
        assert!(start_launcher(&mut slot, &path).unwrap());
        let first = slot.as_ref().unwrap().id();
        let duplicate = start_launcher(&mut slot, &path).unwrap();
        let retained = slot.as_ref().unwrap().id();
        slot.as_mut().unwrap().kill().unwrap();
        slot.as_mut().unwrap().wait().unwrap();
        let restarted = start_launcher(&mut slot, &path).unwrap();
        slot.as_mut().unwrap().kill().unwrap();
        slot.as_mut().unwrap().wait().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!duplicate);
        assert_eq!(first, retained);
        assert!(restarted);
    }
}
