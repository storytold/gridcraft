//! A local engine document kept in step with an authoritative session.
//!
//! The caller runs commands through [`Synced::execute`]: a merge command is applied locally at
//! once (the preview), sent as an operation, and corrected by the server's delta when the answer
//! comes. A structural command is not previewed — the server decides where the rows land — and is
//! applied from the command echo of `applied`, with the sheet map that comes with it.
//!
//! Remote operations arrive by [`Synced::poll`]: the command is re-run through
//! [`gridcraft_engine::Session::apply_remote`] (so structure and formulas land), then the
//! authoritative values patch the cached results (volatile functions, Python, precision as
//! displayed). A rejected preview is rolled back with the engine's own undo; remote operations
//! never enter the local undo history.
//!
//! The [`Session`] stays with the caller (the UI owns it); this type only holds the connection,
//! the key map and the participant's own undo stack.

use std::collections::{BTreeMap, VecDeque};

use gridcraft_collab::{Applied, ClientMessage, ClientOp, Kind, Payload, Presence, RejectCode, Role, SheetDelta, SheetName, Target, classify};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::{Value as Json, json};

use crate::client::{Client, Event, Welcome};

/// A command the caller ran, and whether a local preview of it exists.
struct Pending {
    op_id: String,
    previewed: bool,
}

/// Why a command could not be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The session is read-only for this participant.
    Viewer,
    /// The command cannot travel in a shared session (not in the allowlist).
    NotShared(String),
    /// The engine refused the local preview.
    Engine(String),
    /// The connection is gone.
    Disconnected,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Viewer => f.write_str("viewers cannot edit"),
            EditError::NotShared(id) => write!(f, "`{id}` does not travel in a shared session"),
            EditError::Engine(e) => f.write_str(e),
            EditError::Disconnected => f.write_str("the session is not connected"),
        }
    }
}

impl std::error::Error for EditError {}

/// One local document and the session it belongs to.
pub struct Synced {
    client: Client,
    actor: String,
    doc: String,
    /// Sheet key ↔ index of the local workbook, kept from `welcome` and every structural answer.
    keys: BTreeMap<String, usize>,
    names: BTreeMap<String, String>,
    seq: u64,
    client_id: String,
    pending: Vec<Pending>,
    /// The applied steps of this participant that `undo` may reverse, oldest first.
    own: VecDeque<String>,
    /// The last refusal of an undo/redo, for the UI to show once.
    notice: Option<String>,
}

impl Synced {
    pub fn new(client: Client, welcome: &Welcome) -> Synced {
        let mut synced = Synced {
            client,
            actor: welcome.actor_id.clone(),
            doc: welcome.session_id.clone(),
            keys: BTreeMap::new(),
            names: BTreeMap::new(),
            seq: 0,
            client_id: format!("{}-{}", welcome.actor_id, now_millis()),
            pending: Vec::new(),
            own: VecDeque::new(),
            notice: None,
        };
        synced.apply_sheets(&welcome.sheets);
        synced
    }

    pub fn state(&self) -> crate::client::State {
        self.client.state()
    }

    pub fn actor(&self) -> &str {
        &self.actor
    }

    /// The local index of a sheet key, for drawing another participant's cursor.
    pub fn sheet_index(&self, key: &str) -> Option<usize> {
        self.keys.get(key).copied()
    }

    /// The name of the sheet a key belongs to, as this replica knows it now.
    pub fn sheet_name(&self, key: &str) -> Option<&str> {
        self.names.get(key).map(String::as_str)
    }

    /// The session key of a sheet name.
    pub fn key_for_name(&self, name: &str) -> Option<String> {
        self.names.iter().find(|(_, sheet)| sheet.as_str() == name).map(|(key, _)| key.clone())
    }

    /// Asks the room to reverse this participant's last step. The answer arrives as an `applied`
    /// without a command echo (the server broadcasts the restored cells as a delta), so every
    /// replica, this one included, patches them the same way.
    pub fn undo(&mut self) -> Result<Json, EditError> {
        if self.client.state().role == Some(Role::Viewer) {
            return Err(EditError::Viewer);
        }
        let Some(op_id) = self.own.back().cloned() else {
            return Err(EditError::Engine("there is nothing to undo".into()));
        };
        if !self.client.send_message(ClientMessage::Undo { op_id: Some(op_id) }) {
            return Err(EditError::Disconnected);
        }
        Ok(Json::Null)
    }

    /// Puts back the step this participant last undid.
    pub fn redo(&mut self) -> Result<Json, EditError> {
        if self.client.state().role == Some(Role::Viewer) {
            return Err(EditError::Viewer);
        }
        if !self.client.send_message(ClientMessage::Redo { op_id: None }) {
            return Err(EditError::Disconnected);
        }
        Ok(Json::Null)
    }

    /// The last refused undo/redo, taken once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// Runs a command locally and shares it.
    ///
    /// A merge command is previewed, then sent; `applied` patches the values, `rejected` rolls
    /// the preview back. A structural command is only sent: it is exclusive on the server and the
    /// client must not guess the resulting addresses.
    pub fn execute(&mut self, session: &mut Session, id: &str, params: Json) -> Result<Json, EditError> {
        if id == "edit.undo" {
            return self.undo();
        }
        if id == "edit.redo" {
            return self.redo();
        }
        let state = self.client.state();
        if state.role == Some(Role::Viewer) {
            return Err(EditError::Viewer);
        }
        if state.role.is_none() {
            return Err(EditError::Disconnected);
        }
        let class = classify(id).ok_or_else(|| EditError::NotShared(id.to_string()))?;
        let structural = class == gridcraft_collab::Class::Structural;
        let (sheet_key, sheet_name, a1) = self.target(session)?;
        let mut params = params;
        normalise(&mut params, id, &sheet_name, &a1);
        let previewed = !structural;
        if previewed {
            session.execute(id, params.clone()).map_err(|e| EditError::Engine(e.to_string()))?;
        }
        let op_id = format!("{}-{}", self.client_id, self.seq);
        self.seq = self.seq.saturating_add(1);
        let op = ClientOp {
            op_id: op_id.clone(),
            doc: self.doc.clone(),
            base_version: state.version,
            epoch: state.epoch,
            kind: if structural { Kind::Structural } else { Kind::Command },
            target: Target { sheet: sheet_key, a1: Some(a1) },
            payload: Payload { cmd: id.to_string(), params: params.clone() },
            preview: None,
        };
        if !self.client.send(op) {
            if previewed {
                let _ = session.execute("edit.undo", json!({}));
            }
            return Err(EditError::Disconnected);
        }
        self.pending.push(Pending { op_id, previewed });
        Ok(if structural { Json::Null } else { json!({ "queued": true }) })
    }

    /// Tells the other participants where this one is looking.
    pub fn send_presence(&self, presence: Presence) {
        self.client.send_presence(presence);
    }

    /// Applies every event the connection delivered and hands them to the caller.
    pub fn poll(&mut self, session: &mut Session) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = self.client.poll() {
            match &event {
                Event::Applied(applied) => {
                    self.apply(session, applied);
                    self.pending.retain(|pending| pending.op_id != applied.op_id);
                    if applied.actor == self.actor {
                        if applied.result.command.is_some() {
                            if !applied.op_id.starts_with("undo-") {
                                self.own.push_back(applied.op_id.clone());
                                while self.own.len() > 200 {
                                    self.own.pop_front();
                                }
                            }
                        } else {
                            self.own.pop_back();
                        }
                    }
                }
                Event::Rejected(rejected) => {
                    if rejected.op_id == "undo" || rejected.op_id == "redo" {
                        self.notice = Some(rejected.message.clone().unwrap_or_else(|| format!("{:?}", rejected.code)));
                    }
                    if let Some(index) = self.pending.iter().position(|pending| pending.op_id == rejected.op_id) {
                        let pending = self.pending.remove(index);
                        if pending.previewed {
                            let _ = session.execute("edit.undo", json!({}));
                        }
                        if rejected.retryable && rejected.code == RejectCode::Stale {
                            // The address moved under the preview: the safest answer is a fresh
                            // document from the server.
                            self.client.send_message(ClientMessage::Resync { from: self.client.state().version });
                        }
                    }
                }
                Event::Snapshot { bytes, .. } => {
                    if let Ok((workbook, _warnings)) = gridcraft_engine::io::open_bytes("session.xlsx", bytes) {
                        *session = Session::new();
                        session.add_document(gridcraft_engine::DocState::from_saved(
                            workbook,
                            None,
                            "session.xlsx".into(),
                            gridcraft_engine::Trust::FromFile,
                        ));
                        let sheets = self.client.state().sheets;
                        self.apply_sheets(&sheets);
                        self.pending.clear();
                    }
                }
                Event::Reconnected { .. } => {
                    // Catch-up frames follow; the pending queue is resent by the transport.
                }
                Event::Presence(_) | Event::Resynced { .. } | Event::Disconnected { .. } | Event::Error { .. } | Event::Closed => {}
            }
            out.push(event);
        }
        out
    }

    /// Applies one confirmed operation: the command first (structure, formulas, styles), then the
    /// authoritative values of every cell the server listed.
    fn apply(&mut self, session: &mut Session, applied: &Applied) {
        if let Some(command) = &applied.result.command {
            // The author previewed a merge command; nobody previewed a structural one.
            let structural = applied.sheets.is_some();
            let mine = applied.actor == self.actor;
            if !mine || structural {
                let _ = session.apply_remote(&command.id, command.params.clone());
            }
        }
        if let Some(sheets) = &applied.sheets {
            self.apply_sheets(sheets);
        }
        if let Some(delta) = &applied.result.delta {
            self.patch(session, delta);
        }
    }

    fn patch(&mut self, session: &mut Session, delta: &[SheetDelta]) {
        for sheet in delta {
            let Some(index) = self.sheet_index(&sheet.sheet) else { continue };
            let mut values: Vec<(CellRef, Value)> = Vec::with_capacity(sheet.cells.len());
            for change in &sheet.cells {
                let Some(at) = CellRef::parse(&change.a1) else { continue };
                let Ok(value) = serde_json::from_value::<Value>(change.value.clone()) else { continue };
                values.push((at, value));
            }
            let _ = session.apply_values(index, &values);
        }
    }

    fn apply_sheets(&mut self, sheets: &[SheetName]) {
        self.keys.clear();
        self.names.clear();
        for (index, sheet) in sheets.iter().enumerate() {
            self.keys.insert(sheet.key.clone(), index);
            self.names.insert(sheet.key.clone(), sheet.name.clone());
        }
    }

    /// The sheet key, name and address the command applies to, taken from the UI's selection when
    /// the parameters do not carry them.
    fn target(&self, session: &Session) -> Result<(String, String, String), EditError> {
        let doc = session.doc().map_err(|e| EditError::Engine(e.to_string()))?;
        let index = doc.wb.active_sheet();
        let sheet = doc.wb.sheet(index).ok_or_else(|| EditError::Engine("no such sheet".into()))?;
        let key = self
            .names
            .iter()
            .find(|(_, name)| **name == sheet.name)
            .map(|(key, _)| key.clone())
            .ok_or_else(|| EditError::Engine("the sheet has no session key".into()))?;
        let selection = &doc.selection;
        let range = selection.ranges.last().copied().unwrap_or_else(|| gridcraft_engine::core::RangeRef::cell(selection.active));
        let a1 = if range.start == range.end { format!("{}", selection.active) } else { format!("{range}") };
        Ok((key, sheet.name.clone(), a1))
    }
}

/// Fills in what the UI carried implicitly (the same rule the server applies to an operation).
fn normalise(params: &mut Json, cmd: &str, sheet: &str, a1: &str) {
    let Some(object) = params.as_object_mut() else { return };
    if !object.contains_key("sheet") {
        object.insert("sheet".to_string(), Json::String(sheet.to_string()));
    }
    if object.contains_key("cell") || object.contains_key("range") {
        return;
    }
    if cmd.starts_with("cell.") || cmd.starts_with("selection.") {
        object.insert("cell".to_string(), Json::String(a1.to_string()));
    } else {
        object.insert("range".to_string(), Json::String(a1.to_string()));
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
