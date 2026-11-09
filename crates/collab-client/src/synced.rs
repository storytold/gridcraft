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

use base64::Engine;
use gridcraft_collab::{
    Applied, ClientMessage, ClientOp, IMAGE_LIMIT, IMAGE_PART, ImageBundle, Kind, Payload, Presence, RejectCode, Role, SheetDelta, SheetImage,
    SheetName, Target, classify,
};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::{Value as Json, json};

use crate::link::{Event, Link, State, Welcome};

/// A command the caller ran, and whether a local preview of it exists.
struct Pending {
    op_id: String,
    previewed: bool,
}

/// A structural undo/redo whose sheet image is arriving in parts (ADR-0013).
///
/// Nothing is applied while the parts are coming in: a half of an image is not a document state.
/// `from` is the version of this replica before the first part, so a sequence that breaks can ask
/// for a catch-up from there instead of guessing.
struct ImageParts {
    op_id: String,
    version: u64,
    epoch: u64,
    total: u32,
    next: u32,
    data: Vec<u8>,
    from: u64,
}

/// The most parts an image may arrive in: the room's operation bound ([`IMAGE_LIMIT`]) counted in
/// [`IMAGE_PART`] bytes, plus the two parts an exact bound can straddle. A sequence that announces
/// more is refused before a byte of it is buffered.
const MAX_IMAGE_PARTS: u32 = (IMAGE_LIMIT / IMAGE_PART + 2) as u32;

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
    link: Box<dyn Link>,
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
    /// The sheet image of a structural undo/redo that is still arriving in parts.
    image_parts: Option<ImageParts>,
}

impl Synced {
    /// A desktop session over the native client. The window id the client sent in `hello` is the
    /// id of this replica (RFC §13.6): operation ids are prefixed with it, so two windows of one
    /// actor never collide, and presence stays per window.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(client: crate::Client, welcome: &Welcome) -> Synced {
        let window_id = client.window_id().to_string();
        Synced::with_link(Box::new(client), welcome, window_id)
    }

    /// A session over any transport. The browser build passes its own `WebSocket`
    /// link and its window id from the page; the desktop `new` wraps the native
    /// client the same way.
    pub fn with_link(link: Box<dyn Link>, welcome: &Welcome, client_id: String) -> Synced {
        let mut synced = Synced {
            link,
            actor: welcome.actor_id.clone(),
            doc: welcome.session_id.clone(),
            keys: BTreeMap::new(),
            names: BTreeMap::new(),
            seq: 0,
            client_id,
            pending: Vec::new(),
            own: VecDeque::new(),
            notice: None,
            image_parts: None,
        };
        synced.apply_sheets(&welcome.sheets);
        synced
    }

    pub fn state(&self) -> State {
        self.link.state()
    }

    /// Replaces the sheet map after `welcome` (the first one carries the keys,
    /// and every structural answer may rename or reorder them).
    pub fn set_sheets(&mut self, sheets: &[SheetName]) {
        self.apply_sheets(sheets);
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
        if self.link.state().role == Some(Role::Viewer) {
            return Err(EditError::Viewer);
        }
        let Some(op_id) = self.own.back().cloned() else {
            return Err(EditError::Engine("there is nothing to undo".into()));
        };
        if !self.link.send_message(ClientMessage::Undo { op_id: Some(op_id) }) {
            return Err(EditError::Disconnected);
        }
        Ok(Json::Null)
    }

    /// Puts back the step this participant last undid.
    pub fn redo(&mut self) -> Result<Json, EditError> {
        if self.link.state().role == Some(Role::Viewer) {
            return Err(EditError::Viewer);
        }
        if !self.link.send_message(ClientMessage::Redo { op_id: None }) {
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
        let state = self.link.state();
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
        if !self.link.send(op) {
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
        self.link.send_presence(presence);
    }

    /// Applies every event the connection delivered and hands them to the caller.
    ///
    /// A structural undo/redo may arrive in parts (ADR-0013). The parts are collected here and the
    /// caller sees one completed `applied`; until the last part nothing touches the document.
    pub fn poll(&mut self, session: &mut Session) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = self.link.poll() {
            let event = match event {
                Event::Applied(applied) => match self.accept_image_parts(&applied) {
                    Some(applied) => Event::Applied(applied),
                    None => continue,
                },
                Event::Disconnected { reason } => {
                    if self.image_parts.take().is_some() {
                        self.notice = Some("the sheet image was interrupted; the document is unchanged".into());
                    }
                    Event::Disconnected { reason }
                }
                Event::Reconnected { version } => {
                    // The catch-up restarts from the version before the first part, so the old
                    // half of the image is never continued.
                    self.image_parts = None;
                    Event::Reconnected { version }
                }
                Event::Snapshot { version, epoch, bytes } => {
                    self.image_parts = None;
                    Event::Snapshot { version, epoch, bytes }
                }
                Event::Resynced { version, epoch } => {
                    self.image_parts = None;
                    Event::Resynced { version, epoch }
                }
                other => other,
            };
            match &event {
                Event::Applied(applied) => {
                    self.apply(session, applied);
                    self.pending.retain(|pending| pending.op_id != applied.op_id);
                    if applied.actor == self.actor {
                        if applied.result.command.is_some() {
                            if !applied.op_id.starts_with("undo-") && !applied.op_id.starts_with("redo-") {
                                self.own.push_back(applied.op_id.clone());
                                while self.own.len() > 200 {
                                    self.own.pop_front();
                                }
                            }
                        } else if let Some(step) = applied.op_id.strip_prefix("undo-") {
                            if self.own.back().is_some_and(|id| id == step) {
                                self.own.pop_back();
                            }
                        } else if let Some(step) = applied.op_id.strip_prefix("redo-") {
                            // Redo puts the step back on the undo stack: the next undo reverses it
                            // again, and a later edit still ends the branch.
                            self.own.push_back(step.to_string());
                            while self.own.len() > 200 {
                                self.own.pop_front();
                            }
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
                            self.link.send_message(ClientMessage::Resync { from: self.link.state().version });
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
                        let sheets = self.link.state().sheets;
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

    /// One `applied` narrowed to the operation to apply: a part of a chunked sheet image is
    /// buffered (`None`), the last part becomes the whole operation (ADR-0013).
    ///
    /// A sequence that breaks — a missing part, another operation in the middle, undecodable
    /// data — drops the buffer and asks for a catch-up from the version before the first part.
    /// The document is not touched on a partial image.
    fn accept_image_parts(&mut self, applied: &Applied) -> Option<Applied> {
        let Some(part) = &applied.image_part else {
            if applied.more {
                // A part of a chunked delta this client does not assemble: never apply a prefix
                // (ADR-0013 §3).
                self.notice = Some("a partial operation arrived; the document is unchanged".into());
                self.link.send_message(ClientMessage::Resync { from: self.link.state().version });
                return None;
            }
            if self.image_parts.is_some() {
                return self.refuse_image();
            }
            return Some(applied.clone());
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&part.data) else { return self.refuse_image() };

        if self.image_parts.is_none() {
            if part.seq != 0 {
                return self.refuse_image();
            }
            if part.total > MAX_IMAGE_PARTS {
                // The server is bound by the same limit; a sequence that says otherwise is not one
                // this replica will buffer. Nothing was applied; ask for a catch-up from the
                // version before the operation (part 0 carries `more:true`, so the version has not
                // moved).
                self.notice = Some("the sheet image is over the operation limit; the document is unchanged".into());
                self.link.send_message(ClientMessage::Resync { from: self.link.state().version });
                return None;
            }
            let total = part.total;
            self.image_parts = Some(ImageParts {
                op_id: applied.op_id.clone(),
                version: applied.version,
                epoch: applied.epoch,
                total,
                next: 1,
                data: bytes,
                from: self.link.state().version,
            });
        } else {
            let mut broken = false;
            if let Some(buffer) = self.image_parts.as_mut() {
                broken = buffer.op_id != applied.op_id
                    || buffer.version != applied.version
                    || buffer.epoch != applied.epoch
                    || buffer.total != part.total
                    || part.seq != buffer.next;
                if !broken {
                    buffer.next += 1;
                    buffer.data.extend_from_slice(&bytes);
                }
            }
            if broken {
                return self.refuse_image();
            }
        }
        if self.image_parts.as_ref().is_some_and(|buffer| buffer.data.len() > IMAGE_LIMIT) {
            // The announced bound was not the real one: drop what arrived rather than grow without
            // a limit (ADR-0013 §6, the document is unchanged).
            return self.refuse_image();
        }
        if applied.more {
            return None;
        }
        let buffer = self.image_parts.take()?;
        if buffer.total != buffer.next {
            // The last part says the image is complete, but a part before it never arrived.
            self.image_parts = Some(buffer);
            return self.refuse_image();
        }
        self.finish_image(buffer, applied)
    }

    /// Turns a complete sequence of parts back into the operation it carried.
    fn finish_image(&mut self, buffer: ImageParts, applied: &Applied) -> Option<Applied> {
        match serde_json::from_slice::<ImageBundle>(&buffer.data) {
            Ok(bundle) => {
                let mut done = applied.clone();
                done.images = Some(bundle.images);
                done.book = bundle.book;
                done.image_part = None;
                done.more = false;
                Some(done)
            }
            Err(_) => {
                self.notice = Some("the sheet image arrived broken; the document is unchanged".into());
                self.link.send_message(ClientMessage::Resync { from: buffer.from });
                None
            }
        }
    }

    /// Drops a half-received image and asks for a catch-up from the version before its first
    /// part (ADR-0013 §6). Returns `None`: nothing is applied.
    fn refuse_image(&mut self) -> Option<Applied> {
        if let Some(buffer) = self.image_parts.take() {
            self.notice = Some("the sheet image arrived broken; the document is unchanged".into());
            self.link.send_message(ClientMessage::Resync { from: buffer.from });
        }
        None
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
        // A structural undo/redo is not a command: the server sends the whole sheets it restored,
        // in the session's order, and the client replaces its own before the map below moves. The
        // workbook-level state of the same boundary (defined names, structure protection) comes
        // with them and is applied in the same call, so the replica never sits between two shapes
        // of the document.
        if let Some(images) = &applied.images {
            let sheets = images.iter().map(SheetImage::restored).collect();
            if session.restore_structural(sheets, applied.book.as_deref().cloned()).is_err() {
                self.notice = Some("the sheet image does not fit this replica; requesting a catch-up".into());
                self.link.send_message(ClientMessage::Resync { from: applied.version.saturating_sub(1) });
                return;
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
