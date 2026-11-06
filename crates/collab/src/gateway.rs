//! The message loop a socket runs, without the socket (PR 4 of `docs/collab/rfc.md`).
//!
//! A transport decodes one [`ClientMessage`], hands it to [`Gateway::handle`] with the
//! [`Connection`] the handshake produced, and encodes the [`ServerMessage`]s it returns. Every
//! rule that matters — the token, the role, the operation, presence, ping — is here and is tested
//! without a network.

use serde::{Deserialize, Serialize};

use crate::auth::{AuthError, Claims, Expected};
use crate::hub::{Hub, HubError, Participant, Presence, Role, viewer_rejection};
use crate::protocol::{Applied, ClientOp, Rejected};

/// One message from a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// The first message: the verified token, the display name and the version the client holds.
    Hello {
        jwt: Claims,
        name: String,
        last_version: u64,
    },
    Op {
        op: ClientOp,
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

/// One sheet of the welcome message: its session key and its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetName {
    pub key: String,
    pub name: String,
}

/// One message to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    Welcome { session_id: String, actor_id: String, role: Role, version: u64, epoch: u64, sheets: Vec<SheetName>, participants: Vec<Participant> },
    Applied { applied: Applied },
    Rejected { rejected: Rejected },
    Presence { participants: Vec<Participant> },
    Pong { nonce: u64 },
    Error { code: String, message: String, retryable: bool },
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
    /// and builds the welcome message the client needs before it sends anything else.
    pub fn connect(&mut self, claims: &Claims, now: u64, expected: Expected<'_>, name: &str) -> Result<(Connection, ServerMessage), AuthError> {
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
            ClientMessage::Presence { presence } => match self.hub.presence(&connection.doc, &connection.actor, presence) {
                Ok(participants) => vec![ServerMessage::Presence { participants }],
                Err(e) => vec![ServerMessage::Error { code: "internal".into(), message: format!("{e:?}"), retryable: false }],
            },
            ClientMessage::Resync { from } => vec![ServerMessage::Error {
                code: "resync".into(),
                message: format!("no snapshot after version {from} in v1; reopen the document"),
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
