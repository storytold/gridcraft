//! A native, workbook-scoped assistant. The editor interaction reference is the
//! [Zed agent panel](https://zed.dev/docs/ai/agent-panel); the tools operate on GridCraft documents.
mod chat;
mod code_preview;
mod compact;
mod composer;
mod history;
mod math_preview;
mod mcp;
mod mermaid;
mod message_math;
mod message_view;
mod panel;
mod pixels;
mod provider;
pub(crate) mod script_editor;
mod snap;
mod transcript;
mod vision;
mod workbook;

pub(crate) use panel::{ribbon_tile, show};
pub use provider::set_config_dir;

use crate::SheetApp;
use composer::Composer;
use provider::{Agent, Discovery, Job, ToolCall};
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    Chat,
    #[default]
    Read,
    Edit,
    Mcp,
}

impl Mode {
    fn writes(self) -> bool {
        matches!(self, Self::Edit | Self::Mcp)
    }
    fn initial(agent: Option<&Agent>) -> Self {
        match agent.and_then(|a| a.default_profile.as_deref()) {
            Some("write") => Self::Edit,
            Some("write_mcp") => Self::Mcp,
            Some("minimal") => Self::Chat,
            _ => Self::Read,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat only",
            Self::Read => "Read only",
            Self::Edit => "Read / write",
            Self::Mcp => "Read / write / MCP",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ToolRounds(Option<usize>);

impl Default for ToolRounds {
    fn default() -> Self {
        Self(Some(12))
    }
}

impl ToolRounds {
    fn final_request(self, completed: usize) -> bool {
        self.0.is_some_and(|limit| completed >= limit.saturating_sub(1))
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Approval {
    #[default]
    Ask,
    AlwaysAllow,
}

impl Approval {
    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask every time",
            Self::AlwaysAllow => "Always allow",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AgentKey(String, String, String, String, [u8; 32]);
impl From<&Agent> for AgentKey {
    fn from(a: &Agent) -> Self {
        Self(a.name.clone(), a.provider.clone(), a.model.clone(), a.endpoint.clone(), a.credential_identity)
    }
}

#[derive(Default)]
struct Thread {
    summary: String,
    title: String,
    owner: Option<AgentKey>,
    // Only protocol-complete exchanges are replayed, including receipts saved when a turn stops.
    history: VecDeque<Vec<Value>>,
    log: VecDeque<Line>,
}

struct Line {
    speaker: &'static str,
    text: String,
    references: Vec<std::ops::Range<usize>>,
    detail: Option<ToolDetail>,
}

struct ToolDetail {
    text: String,
    expanded: Option<bool>,
}

impl Thread {
    fn line(&mut self, speaker: &'static str, text: impl Into<String>) {
        let text = text.into();
        let references =
            if matches!(speaker, "You" | "Assistant") && text.contains('@') { composer::mention_spans(&text).collect() } else { Vec::new() };
        self.record(Line { speaker, text, references, detail: None });
    }

    fn tool(&mut self, speaker: &'static str, caption: impl Into<String>, details: String) {
        self.record(Line { speaker, text: caption.into(), references: Vec::new(), detail: Some(ToolDetail { text: details, expanded: None }) });
    }

    fn record(&mut self, line: Line) {
        self.log.push_back(line);
        while self.log.len() > 160
            || (self.log.len() > 1
                && self
                    .log
                    .iter()
                    .map(|l| l.text.len() + size_of_val(l.references.as_slice()) + l.detail.as_ref().map_or(0, |d| d.text.len()))
                    .sum::<usize>()
                    > 512 * 1024)
        {
            self.log.pop_front();
        }
    }
}

#[derive(PartialEq, Eq)]
struct Target {
    editor: Option<u64>,
    window: u64,
    doc: u64,
    sheet: u64,
    revision: u64,
    selection: gridcraft_engine::Selection,
    object: Option<gridcraft_engine::model::ObjRef>,
}

impl Target {
    fn matches(&self, app: &SheetApp) -> bool {
        self.editor == script_editor::owner(app)
            && app.session.active_window() == Some(self.window)
            && self.object == app.selected_object
            && app.session.active().is_some_and(|d| {
                self.doc == d.uid
                    && self.revision == d.revision
                    && d.wb.active().is_some_and(|s| self.sheet == s.uid.get())
                    && self.selection == d.selection
            })
    }

    fn capture(app: &SheetApp) -> Option<Self> {
        let d = app.session.active()?;
        Some(Self {
            editor: script_editor::owner(app),
            window: app.session.active_window()?,
            doc: d.uid,
            sheet: d.wb.active()?.uid.get(),
            revision: d.revision,
            selection: d.selection.clone(),
            object: app.selected_object,
        })
    }
}

struct Turn {
    maintenance: Option<compact::Work>,
    manual_compact: bool,
    overflow_retry: bool,
    prompt_overhead: usize,
    snap_cache: std::sync::Arc<std::sync::Mutex<snap::Cache>>,
    agent: Agent,
    target: Target,
    rounds: usize,
    ribbon: workbook::ribbon::Ribbon,
    native_offered: std::collections::BTreeSet<String>,
    final_answer: bool,
    approval: Approval,
    // Messages from the start of this turn, distinct from older completed history.
    wire: Vec<Value>,
    job: Option<Job>,
    partial: String,
    partial_thinking: String,
    // Each accepted call is queued, dispatched externally, or already has a receipt in wire.
    queue: VecDeque<ToolCall>,
    external: Option<mcp::Pending>,
    search: Option<chat::SearchBatch>,
    visual_job: Option<vision::Pending>,
    visual: Option<vision::Attachment>,
    offered: mcp::Offered,
}

#[derive(Default)]
pub(crate) struct PanelState {
    dock_rect: Option<egui::Rect>,
    launcher_id: Option<egui::Id>,
    open: bool,
    loaded: bool,
    mode: Mode,
    tool_rounds: ToolRounds,
    approval: Approval,
    agents: Vec<Agent>,
    selected: usize,
    discovery: Option<Discovery>,
    note: String,
    thread: Thread,
    archives: VecDeque<Thread>,
    turn: Option<Turn>,
    composer: Composer,
    guide: panel::RibbonGuide,
    expand_tools: bool,
    show_thinking: bool,
    mcp: mcp::Hub,
}

pub(crate) fn dock_left(app: &SheetApp) -> Option<f32> {
    (app.ui.show_wingman_pane && app.wingman.open).then_some(app.wingman.dock_rect?).map(|r| r.left())
}

impl PanelState {
    pub(crate) fn toggle(&mut self, params: &Value) -> Result<Value, String> {
        let open = match params.get("on") {
            Some(v) => v.as_bool().ok_or("on must be true or false")?,
            None => !self.open,
        };
        if open && !self.open {
            if !self.loaded {
                self.reload();
            }
            self.composer.request_focus();
        } else if !open {
            self.close();
        }
        self.open = open;
        Ok(json!({"on": open}))
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.stop();
        self.approval = Approval::Ask;
        self.discovery = None;
        self.mcp.disconnect();
        self.dock_rect = None;
    }

    fn stop(&mut self) {
        if let Some(t) = self.turn.take() {
            let reason = if t.external.is_some() {
                "Stopped waiting for the external tool. It may already have run; GridCraft cannot undo external actions."
            } else {
                "Stopped. Previously approved edits, if any, are unchanged."
            };
            self.recover(t, "Not applied: the turn was stopped.");
            self.thread.line("Wingman", reason);
        }
    }

    fn recover(&mut self, t: Turn, reason: &str) {
        self.note.clear();
        let external = t.external.map(|mut p| {
            let outcome = p.poll();
            interrupted_external(&p.id, outcome)
        });
        if let Some(wire) = interrupted_history(t.wire, external, t.queue, reason) {
            self.remember(wire);
        }
        self.thread.assistant_line("Assistant (partial)", &t.partial_thinking, &t.partial, false);
    }

    fn fresh(&mut self) {
        self.stop();
        self.approval = Approval::Ask;
        self.mcp.disconnect();
        if !self.thread.log.is_empty() {
            self.archives.push_front(std::mem::take(&mut self.thread));
            self.archives.truncate(8);
        }
        self.thread = Thread::default();
        self.composer = Composer::default();
        self.composer.request_focus();
        self.note.clear();
    }

    fn catalog(&mut self, agents: Vec<Agent>) {
        let old = self.agents.get(self.selected).map(AgentKey::from);
        let idx = old.as_ref().and_then(|key| agents.iter().position(|a| AgentKey::from(a) == *key)).unwrap_or(0);
        if old != agents.get(idx).map(AgentKey::from) {
            self.fresh();
            self.mode = Mode::initial(agents.get(idx));
        }
        self.agents = agents;
        self.selected = idx;
    }

    fn reload(&mut self) {
        self.approval = Approval::Ask;
        self.mcp.reload();
        self.discovery = None;
        self.loaded = true;
        self.note.clear();
        match provider::load() {
            Ok(agents) => {
                self.catalog(agents);
                self.change_mode(Mode::initial(self.agents.get(self.selected)));
            }
            Err(error) => {
                self.catalog(Vec::new());
                self.note = error;
            }
        }
    }

    fn change_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.stop();
            self.approval = Approval::Ask;
            self.mcp.disconnect();
            self.mode = mode;
        }
    }
}

// This is bookkeeping only. Never dispatch calls when closing the exchange: the next
// user message needs the original receipts, not another attempt to perform those edits.
fn interrupted_history(mut wire: Vec<Value>, external: Option<Value>, queue: VecDeque<ToolCall>, reason: &str) -> Option<Vec<Value>> {
    wire.extend(external);
    for call in queue {
        wire.push(json!({"role":"tool","tool_call_id":call.id,"content":reason}));
    }
    // No accepted provider response yet: don't replay the unfulfilled user request.
    (wire.len() > 1).then_some(wire)
}

fn interrupted_external(id: &str, outcome: Option<Result<Value, String>>) -> Value {
    let data = match outcome {
        Some(Ok(receipt)) => receipt,
        Some(Err(e)) => json!({"error":e,"outcome":"unknown"}),
        None => json!({"outcome":"unknown","message":"External call dispatched; no receipt before interruption. Verify before repeating."}),
    };
    json!({"role":"tool","tool_call_id":id,"content":data.to_string()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, UiState};

    #[test]
    fn stopping_a_mixed_batch_preserves_receipts_without_retrying() {
        let user = json!({"role":"user","content":"apply the changes"});
        let calls = ["edited", "external", "waiting"].map(|id| json!({"type":"function","id":id,"function":{"name":"tool","arguments":"{}"}}));
        let assistant = json!({"role":"assistant","tool_calls":calls});
        let applied = json!({"role":"tool","tool_call_id":"edited","content":"original receipt"});
        let mut queued = ToolCall::default();
        queued.id = "waiting".into();
        let wire = interrupted_history(
            vec![user.clone(), assistant.clone(), applied.clone()],
            Some(interrupted_external("external", None)),
            VecDeque::from([queued]),
            "Not applied: stopped.",
        )
        .unwrap();
        assert_eq!(wire[..3], [user.clone(), assistant, applied]);
        assert_eq!(wire[3]["tool_call_id"], "external");
        assert_eq!(serde_json::from_str::<Value>(wire[3]["content"].as_str().unwrap()).unwrap()["outcome"], "unknown");
        assert_eq!(wire[4], json!({"role":"tool","tool_call_id":"waiting","content":"Not applied: stopped."}));
        assert_eq!(wire.len(), 5);
        assert!(interrupted_history(vec![user], None, VecDeque::new(), "Stopped.").is_none());
        let receipt = json!({"content":[{"type":"text","text":"complete"}]});
        assert_eq!(interrupted_external("external", Some(Ok(receipt.clone())))["content"], receipt.to_string());
    }

    #[test]
    fn changing_access_or_leaving_a_thread_revokes_standing_consent() {
        let mut pane = PanelState { mode: Mode::Edit, approval: Approval::AlwaysAllow, tool_rounds: ToolRounds(None), ..Default::default() };
        pane.change_mode(Mode::Mcp);
        assert!(pane.approval == Approval::Ask, "workbook consent cannot carry into external tools");
        pane.approval = Approval::AlwaysAllow;
        pane.fresh();
        assert!(pane.approval == Approval::Ask);
        pane.approval = Approval::AlwaysAllow;
        pane.close();
        assert!(pane.approval == Approval::Ask);
        assert!(!pane.tool_rounds.final_request(usize::MAX), "Unlimited has no count-based cutoff");
        let finite = ToolRounds(Some(4));
        assert!(!finite.final_request(2));
        assert!(finite.final_request(3), "the fourth request must be able to answer from the receipts");
    }

    #[test]
    fn opting_out_closes_parked_docks_and_survives_restart() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        app.ui = serde_json::from_value(json!({"ribbonCollapsed":true})).unwrap();
        app.follow_active_window();
        assert!(!UiState::default().show_wingman_pane);
        assert!(app.run("view.wingman", json!({"on":true})).is_err());
        app.run("options.showWingmanPane", json!({"on":true})).unwrap();
        let saved: UiState = serde_json::from_value(serde_json::to_value(&app.ui).unwrap()).unwrap();
        assert!(saved.at_startup().show_wingman_pane);
        let mut windows = vec![app.ui_window().unwrap()];
        for _ in 0..2 {
            app.wingman.loaded = true;
            app.run("view.wingman", json!({"on":true})).unwrap();
            windows.push(app.run("view.newWindow", json!({})).unwrap()["window"].as_u64().unwrap());
        }
        app.wingman.loaded = true;
        app.run("view.wingman", json!({"on":true})).unwrap();
        app.run("options.showWingmanPane", json!({"on":false})).unwrap();
        let saved: UiState = serde_json::from_value(serde_json::to_value(&app.ui).unwrap()).unwrap();
        assert!(!saved.at_startup().show_wingman_pane);
        assert!(!app.wingman.open);
        assert!(app.run("view.wingman", json!({})).is_err());
        app.enter_window(windows[0]);
        app.run("options.showWingmanPane", json!({"on":true})).unwrap();
        for id in windows {
            app.enter_window(id);
            assert!(!app.wingman.open);
        }
    }
}
