//! The message loop a socket runs, without the socket (PR 4 of `docs/collab/rfc.md`).
//!
//! A transport decodes one [`ClientMessage`], hands it to [`Gateway::handle`] with the
//! [`Connection`] the handshake produced, and encodes the [`ServerMessage`]s it returns. Every
//! rule that matters — the token, the role, the operation, presence, ping — is here and is tested
//! without a network.

use serde::{Deserialize, Serialize};

use crate::auth::{AuthError, Claims, Expected};
use crate::hub::{Hub, HubError, Participant, Presence, Role, viewer_rejection};
use crate::protocol::{Applied, ClientOp, Rejected, SheetName};

/// One message from a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClientMessage {
    /// The first message: the signed token, the display name and the version the client holds.
    /// The token is a string here — its signature is checked by the transport before a [`Claims`]
    /// value exists, and it is never logged.
    Hello {
        token: String,
        name: String,
        last_version: u64,
    },
    Op {
        op: ClientOp,
    },
    /// Undo the last edit of this participant (ADR-0010). The room answers with `applied` like
    /// any other operation, or with `conflict` when somebody else changed a cell of that step.
    Undo {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Put back the last step this participant undid (ADR-0010).
    Redo {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    Presence {
        presence: Option<Presence>,
    },
    /// Ask for everything after `from`. v1 has no snapshot in the gateway yet, so it answers with
    /// `error` and the client reopens the document.
    Resync {
        from: u64,
    },
    Ping {
        nonce: u64,
    },
    Leave,
}

/// How a snapshot travels to a client that cannot be caught up from the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotMode {
    /// The whole authoritative workbook; the client replaces its document with it.
    Base,
}

/// The lease of the file (ADR-0007 §1): who holds the one active session and until when. The
/// transport re-reads it on every `ping` and operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseInfo {
    pub owner: String,
    pub expires_at: u64,
}

/// One message to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerMessage {
    Welcome {
        session_id: String,
        actor_id: String,
        role: Role,
        version: u64,
        epoch: u64,
        sheets: Vec<SheetName>,
        participants: Vec<Participant>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease: Option<LeaseInfo>,
    },
    Applied {
        applied: Applied,
    },
    Rejected {
        rejected: Rejected,
    },
    /// One chunk of the authoritative workbook. `bytes` is base64; `total` is the number of
    /// chunks of this snapshot and `seq` counts from 0. A client that receives the last chunk and
    /// then `resynced` has the document at `version`.
    Snapshot {
        mode: SnapshotMode,
        version: u64,
        epoch: u64,
        total: u32,
        seq: u32,
        bytes: String,
    },
    /// The catch-up is over: whatever came before (`applied` frames or a snapshot) is the state
    /// at `version`.
    Resynced {
        version: u64,
        epoch: u64,
    },
    Presence {
        participants: Vec<Participant>,
    },
    Pong {
        nonce: u64,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}

/// Who a connection speaks for, fixed at the handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub doc: String,
    pub actor: String,
    pub role: Role,
}

/// The hub plus the message rules.
#[derive(Default)]
pub struct Gateway {
    hub: Hub,
}

impl Gateway {
    pub fn new() -> Gateway {
        Gateway::default()
    }

    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    pub fn hub_mut(&mut self) -> &mut Hub {
        &mut self.hub
    }

    /// Accepts a verified token: validates its claims against this server, joins the participant
    /// and builds the welcome message the client needs before it sends anything else. `lease` is
    /// the lease of the file, when the transport keeps one.
    pub fn connect(
        &mut self,
        claims: &Claims,
        now: u64,
        expected: Expected<'_>,
        name: &str,
        lease: Option<LeaseInfo>,
    ) -> Result<(Connection, ServerMessage), AuthError> {
        let role = claims.validate(now, expected)?;
        let doc = expected.session_id;
        let participants = self.hub.join(doc, &claims.sub, name, role).map_err(|_| AuthError::WrongFile)?;
        let sheets: Vec<SheetName> = self.hub.sheet_map(doc).unwrap_or_default().into_iter().map(|(key, name)| SheetName { key, name }).collect();
        let connection = Connection { doc: doc.to_string(), actor: claims.sub.clone(), role };
        let welcome = ServerMessage::Welcome {
            session_id: doc.to_string(),
            actor_id: claims.sub.clone(),
            role,
            version: self.hub.version(doc).unwrap_or(0),
            epoch: self.hub.epoch(doc).unwrap_or(0),
            sheets,
            participants,
            lease,
        };
        Ok((connection, welcome))
    }

    /// Handles one message. The returned messages go to every connection of the document unless
    /// the call says otherwise; `rejected` and `pong` concern the sender only.
    pub fn handle(&mut self, connection: &Connection, message: ClientMessage) -> Vec<ServerMessage> {
        match message {
            ClientMessage::Hello { .. } => {
                vec![ServerMessage::Error { code: "already-joined".into(), message: "the handshake was already done".into(), retryable: false }]
            }
            ClientMessage::Op { op } => match self.hub.apply(&connection.doc, &connection.actor, op.clone()) {
                Ok(applied) => vec![ServerMessage::Applied { applied }],
                Err(HubError::Forbidden) => vec![ServerMessage::Rejected { rejected: viewer_rejection(&op.op_id) }],
                Err(HubError::Rejected(rejected)) => vec![ServerMessage::Rejected { rejected }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: true }],
            },
            ClientMessage::Undo { op_id } => match self.hub.undo(&connection.doc, &connection.actor, op_id.as_deref()) {
                Ok(applied) => vec![ServerMessage::Applied { applied }],
                Err(HubError::Forbidden) => vec![ServerMessage::Rejected { rejected: viewer_rejection("undo") }],
                Err(HubError::Rejected(rejected)) => vec![ServerMessage::Rejected { rejected }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: false }],
            },
            ClientMessage::Redo { op_id } => match self.hub.redo(&connection.doc, &connection.actor, op_id.as_deref()) {
                Ok(applied) => vec![ServerMessage::Applied { applied }],
                Err(HubError::Forbidden) => vec![ServerMessage::Rejected { rejected: viewer_rejection("redo") }],
                Err(HubError::Rejected(rejected)) => vec![ServerMessage::Rejected { rejected }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: false }],
            },
            ClientMessage::Presence { presence } => match self.hub.presence(&connection.doc, &connection.actor, presence) {
                Ok(participants) => vec![ServerMessage::Presence { participants }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: false }],
            },
            ClientMessage::Resync { from } => vec![ServerMessage::Error {
                code: "resync".into(),
                message: format!("resync is served by the transport, not the room (from {from})"),
                retryable: true,
            }],
            ClientMessage::Ping { nonce } => vec![ServerMessage::Pong { nonce }],
            ClientMessage::Leave => match self.hub.leave(&connection.doc, &connection.actor) {
                Ok(participants) => vec![ServerMessage::Presence { participants }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: false }],
            },
        }
    }
}
