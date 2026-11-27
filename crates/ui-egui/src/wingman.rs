//! A native, workbook-scoped assistant. The editor interaction reference is the
//! [Zed agent panel](https://zed.dev/docs/ai/agent-panel); the tools operate on GridCraft documents.
mod chat;
mod composer;
mod panel;
mod provider;
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
}

impl Mode {
    fn initial(agent: Option<&Agent>) -> Self {
        match agent.and_then(|a| a.default_profile.as_deref()) {
            Some("write") => Self::Edit,
            Some("minimal") => Self::Chat,
            _ => Self::Read,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat only",
            Self::Read => "Read only",
            Self::Edit => "Read / write",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AgentKey(String, String, String, String);
impl From<&Agent> for AgentKey {
    fn from(a: &Agent) -> Self {
        Self(a.name.clone(), a.provider.clone(), a.model.clone(), a.endpoint.clone())
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
    window: u64,
    doc: u64,
    sheet: u64,
    revision: u64,
    selection: gridcraft_engine::Selection,
}

impl Target {
    fn matches(&self, app: &SheetApp) -> bool {
        app.session.active_window() == Some(self.window)
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
    calls: usize,
    // Messages from the start of this turn, distinct from older completed history.
    wire: Vec<Value>,
    job: Option<Job>,
    partial: String,
    queue: VecDeque<ToolCall>,
}

#[derive(Default)]
pub(crate) struct PanelState {
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
            self.stop();
            self.discovery = None;
        }
        self.open = open;
        Ok(json!({"on": open}))
    }

    fn stop(&mut self) {
        if let Some(t) = self.turn.take() {
            if !t.partial.is_empty() {
                self.thread.line("Assistant (partial)", t.partial);
            }
            self.thread.line("Wingman", "Stopped. Previously approved edits, if any, are still in the workbook.");
        }
    }

    fn fresh(&mut self) {
        self.stop();
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
        self.discovery = None;
        self.loaded = true;
        self.note.clear();
        match provider::load() {
            Ok(agents) => self.catalog(agents),
            Err(error) => {
                self.catalog(Vec::new());
                self.note = error;
            }
        }
    }
}
