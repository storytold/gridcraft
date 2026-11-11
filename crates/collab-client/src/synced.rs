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
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::{CellState, Session};
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

/// How many authoritative workbooks in a row this replica may ask for before it stops and says
/// the session's document cannot be loaded here. The transport has already moved its cursor onto
/// a snapshot that failed, so the replica has to repair itself; an endless stream of full
/// workbooks would be the alternative (CodeRabbit 5852406613, `synced.rs:398`).
const SNAPSHOT_ATTEMPTS: u32 = 3;

/// Why a command could not be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The session is read-only for this participant.
    Viewer,
    /// The command cannot travel in a shared session (not in the allowlist).
    NotShared(String),
    /// The engine refused the local preview.
    Engine(String),
    /// The session belongs to another open workbook than the one in front.
    OtherDocument,
    /// The connection is gone.
    Disconnected,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Viewer => f.write_str("viewers cannot edit"),
            EditError::NotShared(id) => write!(f, "`{id}` does not travel in a shared session"),
            EditError::Engine(e) => f.write_str(e),
            EditError::OtherDocument => f.write_str("this session belongs to another open workbook"),
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
    /// The workbook this session shares, by `DocState::uid`, fixed when the replica first acts:
    /// the room holds one document, and the user may open or switch to another one afterwards.
    doc_uid: Option<u64>,
    pending: Vec<Pending>,
    /// The applied steps of this participant that `undo` may reverse, oldest first.
    own: VecDeque<String>,
    /// The last refusal of an undo/redo, for the UI to show once.
    notice: Option<String>,
    /// The sheet image of a structural undo/redo that is still arriving in parts.
    image_parts: Option<ImageParts>,
    /// The version of the document this replica holds: `applied`, `snapshot` and `resynced`
    /// move it. An `applied` at or below it is already in the document — a deduplicated answer to
    /// a resent operation, or a frame that was queued before a snapshot overtook it — and its
    /// command must not run a second time (CodeRabbit 5851190259, `synced.rs:565`).
    document_version: u64,
    /// Authoritative workbooks this replica could not install, in a row. While it is above zero
    /// the document is behind the transport's cursor, so no catch-up may move
    /// [`document_version`](Self::document_version) onto it; a workbook that installs clears it
    /// (CodeRabbit 5852406613, `synced.rs:398`).
    snapshot_failures: u32,
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
        // What this replica holds, not the room's head the welcome reports (RFC §4.4).
        let held = link.state().version;
        let mut synced = Synced {
            link,
            actor: welcome.actor_id.clone(),
            doc: welcome.session_id.clone(),
            keys: BTreeMap::new(),
            names: BTreeMap::new(),
            seq: 0,
            client_id,
            doc_uid: None,
            pending: Vec::new(),
            own: VecDeque::new(),
            notice: None,
            image_parts: None,
            document_version: held,
            snapshot_failures: 0,
        };
        synced.apply_sheets(&welcome.sheets);
        synced
    }

    pub fn state(&self) -> State {
        self.link.state()
    }

    /// The version of the document this replica holds. It stays below the link's cursor while a
    /// snapshot could not be installed: the cursor is not the document
    /// (CodeRabbit 5852406613, `synced.rs:398`).
    pub fn document_version(&self) -> u64 {
        self.document_version
    }

    /// Replaces the sheet map after `welcome` (the first one carries the keys,
    /// and every structural answer may rename or reorder them).
    pub fn set_sheets(&mut self, sheets: &[SheetName]) {
        self.apply_sheets(sheets);
    }

    pub fn actor(&self) -> &str {
        &self.actor
    }

    /// Replaces the shared workbook with the authoritative one, or leaves the document as it was.
    ///
    /// Either the workbook the session sent replaces the shared document, or nothing changes: a
    /// parse error, or a document that is no longer open, leaves the old content and the old
    /// version in place, and the caller asks again instead of claiming what it does not hold
    /// (CodeRabbit 5852406613, `synced.rs:398`).
    fn install_snapshot(&mut self, session: &mut Session, bytes: &[u8]) -> Option<()> {
        let (workbook, _warnings) = gridcraft_engine::io::open_bytes("session.xlsx", bytes).ok()?;
        self.bind(session);
        let uid = self.doc_uid?;
        // Only the shared document is replaced: the other open workbooks, their unsaved edits and
        // the session's preferences stay.
        let next = gridcraft_engine::DocState::from_saved(workbook, None, "session.xlsx".into(), gridcraft_engine::Trust::FromFile);
        session.swap_document(uid, next).ok()?;
        Some(())
    }

    /// Reports a workbook that could not be installed and asks for a way back, or stops asking.
    ///
    /// The cursor already sits on the snapshot's version, so a catch-up from there is empty by
    /// definition. The first answer is therefore the whole workbook again; the second restarts
    /// from the version the document really holds, where a journal tail can still be replayed;
    /// past [`SNAPSHOT_ATTEMPTS`] the replica says so and waits for the session to be reopened
    /// (CodeRabbit 5852406613, `synced.rs:398`).
    fn snapshot_failed(&mut self) {
        self.snapshot_failures = self.snapshot_failures.saturating_add(1);
        match self.snapshot_failures {
            n if n >= SNAPSHOT_ATTEMPTS => {
                self.notice = Some("the session's document could not be loaded; reopen the session to reload it".into());
            }
            1 => {
                self.notice = Some("the session's document could not be loaded; reloading it from the session".into());
                self.link.send_message(ClientMessage::Snapshot);
            }
            _ => {
                self.notice = Some(format!("the session's document could not be loaded; catching up from version {}", self.document_version));
                self.link.send_message(ClientMessage::Resync { from: self.document_version });
            }
        }
    }

    /// Fixes this replica on the workbook it joined with, once. The session holds one document;
    /// everything the replica applies or sends is that workbook's, not whichever window the user
    /// switched to afterwards.
    fn bind(&mut self, session: &Session) {
        if self.doc_uid.is_none() {
            self.doc_uid = session.active().map(|doc| doc.uid);
        }
    }

    /// The document of this session, whatever window is in front; `None` once it was closed.
    pub fn shared_document<'a>(&self, session: &'a Session) -> Option<&'a gridcraft_engine::DocState> {
        let uid = self.doc_uid?;
        session.documents().iter().find(|doc| doc.uid == uid)
    }

    /// Whether an edit made in the active window belongs to this session. A command for another
    /// open workbook stays local: it is not the document the room shares.
    pub fn shares_active_document(&self, session: &Session) -> bool {
        self.doc_uid.is_none_or(|uid| session.active().is_some_and(|doc| doc.uid == uid))
    }

    /// Switches the session to the shared workbook while a remote operation is applied and
    /// returns the window that was in front. A remote operation belongs to one document; applying
    /// it to whichever workbook is active would write cells into an unrelated file.
    fn enter_shared(&self, session: &mut Session) -> Option<usize> {
        let uid = self.doc_uid?;
        let index = session.document_index(uid)?;
        let was = session.active_index();
        if index != was {
            session.set_active(index);
        }
        Some(was)
    }

    /// Puts the window that was in front back after `enter_shared`, unless the document went away
    /// in between (a close command cannot run here, but a restore may drop it).
    fn leave_shared(&self, session: &mut Session, was: Option<usize>) {
        let Some(was) = was else { return };
        if was < session.documents().len() && self.doc_uid.is_some_and(|uid| session.document_index(uid).is_some()) {
            session.set_active(was);
        }
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
        self.bind(session);
        if id == "edit.undo" {
            return self.undo();
        }
        if id == "edit.redo" {
            return self.redo();
        }
        if !self.shares_active_document(session) {
            return Err(EditError::OtherDocument);
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
        self.bind(session);
        // The person is the server's actor (the JWT subject), not the display name a host page
        // sent: operations, the undo stack and the role belong to the actor.
        let actor = self.link.state().actor;
        if !actor.is_empty() && actor != self.actor {
            self.actor = actor;
        }
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
                    // An answer the document already covers — the server answering a resent
                    // operation out of its dedup window, or a frame queued before the snapshot
                    // overtook it — must not run its command again: an inserted row would be
                    // inserted twice, a `cell.set` would overwrite what came after it. The ack
                    // still leaves the resend queue, and the step still counts as this person's
                    // undo step, because the document does hold it (CodeRabbit 5851190259).
                    let stale = applied.version <= self.document_version;
                    if !stale {
                        self.apply(session, applied);
                        self.document_version = applied.version;
                    }
                    self.pending.retain(|pending| pending.op_id != applied.op_id);
                    self.note_own(applied);
                    if stale {
                        // The caller is not told a change happened: the document did not move.
                        continue;
                    }
                }
                Event::Rejected(rejected) => {
                    if rejected.op_id == "undo" || rejected.op_id == "redo" {
                        self.notice = Some(rejected.message.clone().unwrap_or_else(|| format!("{:?}", rejected.code)));
                    }
                    if let Some(index) = self.pending.iter().position(|pending| pending.op_id == rejected.op_id) {
                        // A later preview sits on top of this one in the engine's history, so the
                        // engine's undo would take back that edit and leave this one in the
                        // document. Undo is only the rollback of the last preview; otherwise this
                        // replica cannot rebuild the document and asks for the server's copy.
                        let later_preview = self.pending[index.saturating_add(1)..].iter().any(|pending| pending.previewed);
                        let pending = self.pending.remove(index);
                        if pending.previewed && later_preview {
                            // The refused preview sits under a later one in this replica's history
                            // and never reached the journal, so no tail can take it back: only the
                            // authoritative workbook can (RFC §4.6).
                            self.notice = Some("an earlier edit was refused; reloading the document from the session".into());
                            self.link.send_message(ClientMessage::Snapshot);
                        } else if pending.previewed {
                            let _ = session.execute("edit.undo", json!({}));
                        }
                        if rejected.retryable && rejected.code == RejectCode::Stale {
                            // The address moved under the preview: the safest answer is a fresh
                            // document from the server.
                            self.link.send_message(ClientMessage::Resync { from: self.link.state().version });
                        }
                    }
                }
                Event::Snapshot { version, bytes, .. } => {
                    // The transport moved its cursor onto this snapshot's version before the event
                    // was handed over, so the install is all-or-nothing: a document left behind
                    // while the cursor is ahead would make this replica claim a version whose
                    // content it does not hold (CodeRabbit 5852406613, `synced.rs:398`).
                    match self.install_snapshot(session, bytes) {
                        Some(()) => {
                            // The shared document is now the workbook the snapshot carries: every
                            // answer at or below its version is already in it.
                            self.document_version = self.document_version.max(*version);
                            self.snapshot_failures = 0;
                            let sheets = self.link.state().sheets;
                            self.apply_sheets(&sheets);
                            // The document the previews sat on is gone. An `applied` still in
                            // flight may have been sent before the snapshot request and so is not
                            // in this workbook: it has to be replayed in full, not patched as if
                            // this replica's preview were still in front (CodeRabbit 5848894080,
                            // `synced.rs:385`). The entry stays, with its preview unmarked, until
                            // the answer to the operation arrives.
                            for pending in &mut self.pending {
                                pending.previewed = false;
                            }
                        }
                        None => self.snapshot_failed(),
                    }
                }
                Event::Reconnected { .. } => {
                    // Catch-up frames follow; the pending queue is resent by the transport.
                }
                Event::Resynced { version, .. } => {
                    // The catch-up is over: the tail (or the snapshot branch above) left the
                    // document at the room's version. Anything at or below it is in the document.
                    // A snapshot in that catch-up that could not be installed is the exception:
                    // the document is then behind the transport's cursor, and claiming the version
                    // is exactly what makes the operations it still needs look already covered.
                    if self.snapshot_failures == 0 {
                        self.document_version = self.document_version.max(*version);
                    }
                }
                Event::Presence(_) | Event::Committed { .. } | Event::Disconnected { .. } | Event::Error { .. } | Event::Closed => {}
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

    /// Records a confirmed step of this participant: `undo` may reverse it, and a redo puts it
    /// back on the stack. An answer already recorded — a duplicate of the same op id — changes
    /// nothing.
    fn note_own(&mut self, applied: &Applied) {
        if applied.actor != self.actor {
            return;
        }
        if applied.result.command.is_some() {
            if !applied.op_id.starts_with("undo-") && !applied.op_id.starts_with("redo-") && !self.own.contains(&applied.op_id) {
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
            // Redo puts the step back on the undo stack: the next undo reverses it again, and a
            // later edit still ends the branch.
            if !self.own.iter().any(|id| id.as_str() == step) {
                self.own.push_back(step.to_string());
                while self.own.len() > 200 {
                    self.own.pop_front();
                }
            }
        }
    }

    /// Applies one confirmed operation: the command first (structure, formulas, styles), then the
    /// authoritative values of every cell the server listed.
    fn apply(&mut self, session: &mut Session, applied: &Applied) {
        let restore = self.enter_shared(session);
        self.apply_here(session, applied);
        self.leave_shared(session, restore);
    }

    fn apply_here(&mut self, session: &mut Session, applied: &Applied) {
        if let Some(command) = &applied.result.command {
            // The author previewed a merge command; nobody previewed a structural one, and another
            // window of the same person did not preview it here.
            let structural = applied.sheets.is_some();
            // The preview has to still be in the document, not merely have this window's id in
            // the operation: a workbook that replaced the document took every preview with it, so
            // an answer from before that swap is replayed like a remote command. An operation the
            // transport did not queue here has no preview to lose — this replica never made one
            // (CodeRabbit 5848894080, `synced.rs:385`).
            let mine = applied.actor == self.actor
                && self.previewed_here(applied)
                && self.pending.iter().find(|pending| pending.op_id == applied.op_id).is_none_or(|pending| pending.previewed);
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
            if applied.result.command.is_none() {
                // A command-less answer is a cell undo or redo: the delta is the document, so a
                // restored formula, its array flags, its Python code and its style land here too.
                // Patching values alone left a peer showing the step's old numbers under a formula
                // it never got back.
                self.restore(session, delta);
            } else {
                self.patch(session, delta);
            }
        }
    }

    /// Applies a command-less delta as cell state: every cell the undo or redo put back, with its
    /// formula and flags, then the authoritative values of any spilled children (they live in the
    /// sheet's spill map, not in a cell of their own). `restore_cells` recalculates, so the
    /// dependents of a restored formula come out right without a second message.
    fn restore(&mut self, session: &mut Session, delta: &[SheetDelta]) {
        for sheet in delta {
            let Some(index) = self.sheet_index(&sheet.sheet) else { continue };
            let spill: Vec<CellRef> = self
                .shared_document(session)
                .and_then(|doc| doc.wb.sheet(index).map(|sheet| sheet.spill.keys().copied().collect()))
                .unwrap_or_default();
            let mut cells: Vec<(CellRef, CellState)> = Vec::with_capacity(sheet.cells.len());
            let mut spilled: Vec<(CellRef, Value)> = Vec::new();
            for change in &sheet.cells {
                let Some(at) = CellRef::parse(&change.a1) else { continue };
                let Ok(value) = serde_json::from_value::<Value>(change.value.clone()) else { continue };
                if spill.contains(&at) {
                    spilled.push((at, value));
                    continue;
                }
                cells.push((
                    at,
                    CellState {
                        value,
                        formula: change.formula.clone(),
                        style: change.style.unwrap_or(0),
                        py: change.py.clone(),
                        array: change.array,
                        dynamic: change.dynamic,
                    },
                ));
            }
            if !cells.is_empty() {
                let _ = session.restore_cells(index, &cells);
            }
            if !spilled.is_empty() {
                let _ = session.apply_values(index, &spilled);
            }
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

    /// Whether this window is the one that previewed `applied`: the operation id carries the
    /// window that made it (RFC §13.6), so another window of the same person is not this one.
    fn previewed_here(&self, applied: &Applied) -> bool {
        applied.op_id.strip_prefix(&self.client_id).is_some_and(|rest| rest.starts_with('-'))
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
        let doc = self.shared_document(session).ok_or(EditError::OtherDocument)?;
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
