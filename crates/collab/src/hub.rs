//! Documents, participants and roles: the state a sync server keeps besides the room itself.
//!
//! A [`Hub`] owns one [`Room`] and one engine [`Session`] per document, the participants of each
//! document, their role and their ephemeral presence, and it is the place where a viewer is
//! refused before the engine sees the operation (ADR-0008). The socket layer around it only
//! carries messages; everything here is testable without a network.

use std::collections::BTreeMap;

use gridcraft_engine::Session;
use serde::{Deserialize, Serialize};

use crate::protocol::{Applied, ClientOp, RejectCode, Rejected};
use crate::room::{Room, RoomError, RoomState};

/// What a participant may do (ADR-0008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Editor,
    Owner,
}

impl Role {
    pub fn can_write(self) -> bool {
        self != Role::Viewer
    }
}

/// Where a participant is looking (ADR-0006). Never journaled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    /// Session key of the sheet.
    pub sheet_key: String,
    pub active: String,
    #[serde(default)]
    pub ranges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub top_left: String,
    pub rows: u32,
    pub cols: u32,
}

/// One person in one document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Participant {
    pub actor: String,
    pub name: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
}

/// Why the hub refused a call.
#[derive(Debug)]
pub enum HubError {
    UnknownDocument,
    /// The participant is not in the document.
    UnknownParticipant,
    /// The role may not write, or the engine refused the edit.
    Forbidden,
    Rejected(Rejected),
    Commit(String),
}

impl From<RoomError> for HubError {
    fn from(e: RoomError) -> HubError {
        HubError::Rejected(e.rejected().clone())
    }
}

struct Document {
    session: Session,
    room: Room,
    participants: BTreeMap<String, Participant>,
}

/// All documents of one server process.
#[derive(Default)]
pub struct Hub {
    documents: BTreeMap<String, Document>,
}

impl Hub {
    pub fn new() -> Hub {
        Hub::default()
    }

    /// Adds an open document. Returns `false` when the document already exists (the caller then
    /// joins it instead of reloading the file).
    pub fn open(&mut self, doc: &str, session: Session) -> bool {
        if self.documents.contains_key(doc) {
            return false;
        }
        let room = Room::new(&session);
        self.documents.insert(doc.to_string(), Document { session, room, participants: BTreeMap::new() });
        true
    }

    /// Whether a document is open.
    pub fn is_open(&self, doc: &str) -> bool {
        self.documents.contains_key(doc)
    }

    /// Adds a document whose room and sheet keys were restored from a store (a restart, or a
    /// takeover of a persisted session). Returns `false` when the document already exists.
    pub fn open_restored(&mut self, doc: &str, session: Session, keys: crate::SheetKeys, state: RoomState) -> bool {
        if self.documents.contains_key(doc) {
            return false;
        }
        let room = Room::restore(keys, state);
        self.documents.insert(doc.to_string(), Document { session, room, participants: BTreeMap::new() });
        true
    }

    /// The authoritative workbook of a document, for a snapshot.
    pub fn workbook(&self, doc: &str) -> Option<std::sync::Arc<gridcraft_model::Workbook>> {
        self.documents.get(doc)?.session.active().map(|d| d.wb.clone())
    }

    /// The keys issued in this session, for a snapshot.
    pub fn sheet_keys(&self, doc: &str) -> Option<crate::SheetKeys> {
        self.documents.get(doc).map(|d| d.room.sheet_keys().clone())
    }

    /// The room counters and dedup window, for a snapshot.
    pub fn room_state(&self, doc: &str) -> Option<RoomState> {
        self.documents.get(doc).map(|d| d.room.state())
    }

    /// Drops a document and its room. The caller has already told the participants (a takeover by
    /// the owner of the file, or the cabinet closing the session); nothing is saved here.
    pub fn close(&mut self, doc: &str) -> bool {
        self.documents.remove(doc).is_some()
    }

    /// Adds a participant. The role comes from the JWT, never from the client's message.
    pub fn join(&mut self, doc: &str, actor: &str, name: &str, role: Role) -> Result<Vec<Participant>, HubError> {
        let document = self.documents.get_mut(doc).ok_or(HubError::UnknownDocument)?;
        document.participants.insert(actor.to_string(), Participant { actor: actor.to_string(), name: name.to_string(), role, presence: None });
        Ok(document.participants.values().cloned().collect())
    }

    pub fn leave(&mut self, doc: &str, actor: &str) -> Result<Vec<Participant>, HubError> {
        let document = self.documents.get_mut(doc).ok_or(HubError::UnknownDocument)?;
        document.participants.remove(actor);
        Ok(document.participants.values().cloned().collect())
    }

    pub fn participants(&self, doc: &str) -> Result<Vec<Participant>, HubError> {
        let document = self.documents.get(doc).ok_or(HubError::UnknownDocument)?;
        Ok(document.participants.values().cloned().collect())
    }

    pub fn version(&self, doc: &str) -> Option<u64> {
        self.documents.get(doc).map(|d| d.room.version())
    }

    pub fn epoch(&self, doc: &str) -> Option<u64> {
        self.documents.get(doc).map(|d| d.room.epoch())
    }

    /// Every sheet of the document with its session key, for the welcome message.
    pub fn sheet_map(&self, doc: &str) -> Option<Vec<(String, String)>> {
        let document = self.documents.get(doc)?;
        let workbook = document.session.active()?.wb.as_ref();
        let mut out = Vec::with_capacity(workbook.sheets.len());
        for (index, sheet) in workbook.sheets.iter().enumerate() {
            if let Some(key) = document.room.sheet_keys().key_for_index(workbook, index) {
                out.push((key.to_string(), sheet.name.clone()));
            }
        }
        Some(out)
    }

    /// The session key of the sheet at `index`, as the wire carries it. A client that has just
    /// joined gets the map from the welcome message.
    pub fn sheet_key(&self, doc: &str, index: usize) -> Option<String> {
        let document = self.documents.get(doc)?;
        let workbook = document.session.active()?.wb.as_ref();
        document.room.sheet_keys().key_for_index(workbook, index).map(|k| k.to_string())
    }

    /// Applies one operation on behalf of `actor`, after the role check of ADR-0008.
    pub fn apply(&mut self, doc: &str, actor: &str, op: ClientOp) -> Result<Applied, HubError> {
        let document = self.documents.get_mut(doc).ok_or(HubError::UnknownDocument)?;
        let participant = document.participants.get(actor).ok_or(HubError::UnknownParticipant)?;
        if !participant.role.can_write() {
            return Err(HubError::Forbidden);
        }
        Ok(document.room.apply(&mut document.session, op, actor)?)
    }

    /// Stores the participant's presence and returns everyone else's, so the caller can broadcast
    /// it. Presence is state of a connection, never of the document.
    pub fn presence(&mut self, doc: &str, actor: &str, presence: Option<Presence>) -> Result<Vec<Participant>, HubError> {
        let document = self.documents.get_mut(doc).ok_or(HubError::UnknownDocument)?;
        let participant = document.participants.get_mut(actor).ok_or(HubError::UnknownParticipant)?;
        participant.presence = presence;
        Ok(document.participants.values().cloned().collect())
    }

    /// The document as a `.xlsx` file (ADR-0009): the server writes it from the authoritative
    /// workbook, never from a client's copy.
    pub fn commit(&self, doc: &str) -> Result<Vec<u8>, HubError> {
        let document = self.documents.get(doc).ok_or(HubError::UnknownDocument)?;
        let workbook = document.session.active().map(|d| &d.wb).ok_or(HubError::UnknownDocument)?;
        gridcraft_xlsx::write_xlsx(workbook).map_err(|e| HubError::Commit(e.to_string()))
    }
}

/// The rejection a viewer's operation gets. The transport sends it as it is; there is no room to
/// run it in, so the code is the whole answer.
pub fn viewer_rejection(op_id: &str) -> Rejected {
    Rejected { op_id: op_id.to_string(), code: RejectCode::Forbidden, retryable: false, hint: None, message: Some("viewers cannot edit".into()) }
}
