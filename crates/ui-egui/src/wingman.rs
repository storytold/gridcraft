//! A native, workbook-scoped assistant. The editor interaction reference is the
//! [Zed agent panel](https://zed.dev/docs/ai/agent-panel); the tools operate on GridCraft documents.
mod chat;
mod composer;
mod history;
mod mcp;
mod panel;
mod provider;
pub(crate) mod script_editor;
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

#[derive(Clone, PartialEq, Eq)]
struct AgentKey(String, String, String, String, [u8; 32]);
impl From<&Agent> for AgentKey {
    fn from(a: &Agent) -> Self {
        Self(a.name.clone(), a.provider.clone(), a.model.clone(), a.endpoint.clone(), a.credential_identity)
    }
}

#[derive(Default)]
struct Thread {
    title: String,
    owner: Option<AgentKey>,
    // Only complete, protocol-valid turns are replayed. A stopped turn stays visible in the transcript.
    history: VecDeque<Vec<Value>>,
    log: VecDeque<Line>,
}

struct Line {
    speaker: &'static str,
    text: String,
}

impl Thread {
    fn line(&mut self, speaker: &'static str, text: impl Into<String>) {
        self.log.push_back(Line { speaker, text: text.into() });
        while self.log.len() > 160 || (self.log.len() > 1 && self.log.iter().map(|l| l.text.len()).sum::<usize>() > 512 * 1024) {
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
}

impl Target {
    fn matches(&self, app: &SheetApp) -> bool {
        self.editor == script_editor::owner(app)
            && app.session.active_window() == Some(self.window)
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
        })
    }
}

struct Turn {
    agent: Agent,
    target: Target,
    rounds: usize,
    ribbon: workbook::ribbon::Ribbon,
    native_offered: std::collections::BTreeSet<String>,
    /// End of the last exchange with all tool calls answered (safe to replay on interruption).
    completed_wire: usize,
    final_answer: bool,
    calls: usize,
    // Messages from the start of this turn, distinct from older completed history.
    wire: Vec<Value>,
    job: Option<Job>,
    partial: String,
    queue: VecDeque<ToolCall>,
    external: Option<mcp::Pending>,
    search: Option<chat::SearchBatch>,
    offered: mcp::Offered,
}

#[derive(Default)]
pub(crate) struct PanelState {
    dock_rect: Option<egui::Rect>,
    launcher_id: Option<egui::Id>,
    open: bool,
    loaded: bool,
    mode: Mode,
    agents: Vec<Agent>,
    selected: usize,
    discovery: Option<Discovery>,
    note: String,
    thread: Thread,
    archives: VecDeque<Thread>,
    turn: Option<Turn>,
    composer: Composer,
    guide: panel::RibbonGuide,
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
        self.discovery = None;
        self.mcp.disconnect();
        self.dock_rect = None;
    }

    fn stop(&mut self) {
        if let Some(mut t) = self.turn.take() {
            if t.completed_wire > 0 {
                t.wire.truncate(t.completed_wire);
                self.remember(t.wire);
            }
            if !t.partial.is_empty() {
                self.thread.line("Assistant (partial)", t.partial);
            }
            self.thread.line(
                "Wingman",
                if t.external.is_some() {
                    "Stopped waiting for the external tool. It may already have run; GridCraft cannot undo external actions."
                } else {
                    "Stopped. Previously approved edits, if any, are unchanged."
                },
            );
        }
    }

    fn fresh(&mut self) {
        self.stop();
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
            self.mcp.disconnect();
            self.mode = mode;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, UiState};

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
