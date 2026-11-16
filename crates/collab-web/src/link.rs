//! The collaboration link the browser build dials from the host's `session`
//! message.
//!
//! The frame handling is the desktop client's (`gridcraft_collab_client::Client`):
//! `hello` first, then operations with acknowledged ids, catch-up frames and
//! chunked snapshots turned into [`Event`]s. Only the transport differs — the
//! page's `WebSocket` pushes into a queue that [`SessionLink::poll`] drains once
//! per frame. [`SessionLink::on_text`] is public and transport-free, so the
//! protocol half is tested headlessly on the host.

use std::cell::RefCell;
use std::collections::VecDeque;

use base64::Engine;
use gridcraft_collab::{ClientMessage, ClientOp, Presence, Role, ServerMessage};
use gridcraft_collab_client::{Event, Link, Snapshot, State, snapshot_chunk};

#[cfg(target_arch = "wasm32")]
use crate::socket;

/// The token is a credential: a debug print of a link must not put it in a log.
impl std::fmt::Debug for SessionLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionLink")
            .field("session_id", &self.state.session_id)
            .field("actor", &self.state.actor)
            .field("client_id", &self.client_id)
            .field("token", &"<redacted>")
            .field("connected", &self.state.connected)
            .finish()
    }
}

/// One live connection, as the page's `WebSocket` and the protocol need it.
pub struct SessionLink {
    client_id: String,
    #[allow(dead_code, reason = "the browser transport sends it in hello")]
    name: String,
    #[allow(dead_code, reason = "the browser transport sends it in hello")]
    token: String,
    state: State,
    /// Operations the server has not answered with `applied`/`rejected` yet;
    /// they are resent with their ids after a reconnect.
    pending: RefCell<Vec<ClientOp>>,
    events: VecDeque<Event>,
    snapshot: Option<Snapshot>,
    #[cfg(target_arch = "wasm32")]
    transport: Option<socket::Transport>,
}

impl SessionLink {
    /// Dial the URL the cabinet sent. The token goes in the first frame, never
    /// in the URL.
    #[cfg(target_arch = "wasm32")]
    pub fn open(session_id: &str, ws_url: &str, token: &str, role: Role, actor: &str, name: &str, client_id: &str) -> Result<SessionLink, String> {
        let transport = socket::Transport::open(ws_url)?;
        Ok(SessionLink {
            client_id: client_id.to_string(),
            name: name.to_string(),
            token: token.to_string(),
            state: State {
                session_id: session_id.to_string(),
                actor: actor.to_string(),
                window_id: client_id.to_string(),
                role: Some(role),
                connected: false,
                ..State::default()
            },
            pending: RefCell::new(Vec::new()),
            events: VecDeque::new(),
            snapshot: None,
            transport: Some(transport),
        })
    }

    /// A session without a socket: the tests feed it the server's frames with
    /// [`SessionLink::on_text`]. No network call is made.
    pub fn offline(session_id: &str, actor: &str, role: Role, name: &str, client_id: &str) -> SessionLink {
        SessionLink {
            client_id: client_id.to_string(),
            name: name.to_string(),
            token: String::new(),
            state: State {
                session_id: session_id.to_string(),
                actor: actor.to_string(),
                window_id: client_id.to_string(),
                role: Some(role),
                connected: false,
                ..State::default()
            },
            pending: RefCell::new(Vec::new()),
            events: VecDeque::new(),
            snapshot: None,
            #[cfg(target_arch = "wasm32")]
            transport: None,
        }
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Replace the JWT and dial again. Queued operations survive and are resent
    /// with their ids after the next welcome.
    pub fn renew_token(&mut self, token: &str) {
        self.token = token.to_string();
        #[cfg(target_arch = "wasm32")]
        if let Some(transport) = &self.transport {
            transport.reconnect();
        }
    }

    /// Ask for a fresh document at the current version (a rejection that moved
    /// its target, a catch-up that says `resync`).
    pub fn request_snapshot(&self) {
        let _ = <Self as Link>::send_message(self, ClientMessage::Resync { from: self.state.version });
    }

    /// A transport-free entry point for tests: handle one server frame.
    pub fn on_text(&mut self, text: &str) -> bool {
        let Ok(message) = serde_json::from_str::<ServerMessage>(text) else { return false };
        self.handle(message);
        true
    }

    fn handle(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Welcome { session_id, actor_id, role, version, epoch, sheets, participants, .. } => {
                // A welcome starts a catch-up on this connection. Half of an older snapshot must
                // not continue into it: its `version`, `epoch` and chunk count would apply to the
                // new chunks (CodeRabbit 5851190259, `link.rs:169`).
                self.snapshot = None;
                self.state.session_id = session_id;
                self.state.actor = actor_id;
                self.state.role = Some(role);
                // `version` is the room's head, delivered before the catch-up: taking it here
                // would make the next `hello` claim operations this replica never received if the
                // socket drops first (CodeRabbit 5848894080, `client.rs:121`). Only `applied`,
                // `snapshot` and `resynced` move the cursor.
                self.state.epoch = epoch;
                self.state.sheets = sheets;
                self.state.participants = participants;
                self.state.connected = true;
                self.events.push_back(Event::Reconnected { version });
                for op in self.pending.borrow().iter() {
                    let _ = self.send_raw(&ClientMessage::Op { op: op.clone() });
                }
            }
            ServerMessage::Applied { applied } => {
                gridcraft_collab_client::note_applied(&mut self.state, &applied);
                self.pending.borrow_mut().retain(|op| op.op_id != applied.op_id);
                self.events.push_back(Event::Applied(applied));
            }
            ServerMessage::Rejected { rejected } => {
                self.pending.borrow_mut().retain(|op| op.op_id != rejected.op_id);
                self.events.push_back(Event::Rejected(rejected));
            }
            ServerMessage::Presence { participants } => {
                self.state.participants = participants.clone();
                self.events.push_back(Event::Presence(participants));
            }
            ServerMessage::Resynced { version, epoch } => {
                self.state.version = version;
                self.state.epoch = epoch;
                self.events.push_back(Event::Resynced { version, epoch });
            }
            ServerMessage::Snapshot { version, epoch, total, seq, bytes, mode: _ } => {
                let Ok(chunk) = base64::engine::general_purpose::STANDARD.decode(bytes) else { return };
                let Some((version, epoch, bytes)) = snapshot_chunk(&mut self.snapshot, version, epoch, total, seq, chunk) else { return };
                self.state.version = version;
                self.state.epoch = epoch;
                self.events.push_back(Event::Snapshot { version, epoch, bytes });
            }
            ServerMessage::Committed { version, sha256, cabinet } => {
                self.events.push_back(Event::Committed { version, sha256, cabinet });
            }
            ServerMessage::Pong { .. } => {}
            ServerMessage::Error { code, message, retryable } => {
                if code == "resync" {
                    return;
                }
                let fatal = matches!(
                    code.as_str(),
                    "token-invalid"
                        | "token-expired"
                        | "token-audience"
                        | "token-app"
                        | "token-file"
                        | "token-session"
                        | "lease"
                        | "session-unknown"
                        | "too-many"
                        | "session-closed"
                );
                self.events.push_back(Event::Error { code, message, retryable });
                if fatal {
                    self.state.connected = false;
                    self.events.push_back(Event::Closed);
                }
            }
        }
    }

    fn send_raw(&self, message: &ClientMessage) -> Result<(), String> {
        let text = serde_json::to_string(message).map_err(|err| format!("cannot encode a collaboration message: {err}"))?;
        #[cfg(target_arch = "wasm32")]
        {
            match &self.transport {
                Some(transport) => transport.send(&text),
                None => Err("the web transport is not available".into()),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = text;
            Err("the web transport is not available on this target".into())
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pump(&mut self) {
        let mut opened = false;
        let Some(transport) = &self.transport else { return };
        let incoming = transport.poll();
        for incoming in incoming {
            match incoming {
                socket::Incoming::Opened => opened = true,
                socket::Incoming::Text(text) => {
                    self.on_text(&text);
                }
                socket::Incoming::Closed => {
                    self.state.connected = false;
                    // A snapshot belongs to the connection that started it: half of it must not
                    // wait for the next one (CodeRabbit 5851190259, `link.rs:169`).
                    self.snapshot = None;
                    self.events.push_back(Event::Disconnected { reason: "the socket closed; reconnecting".into() });
                }
            }
        }
        if opened {
            let hello = ClientMessage::Hello {
                token: self.token.clone(),
                name: self.name.clone(),
                last_version: self.state.version,
                window_id: self.client_id.clone(),
            };
            let _ = self.send_raw(&hello);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pump(&mut self) {}
}

impl Link for SessionLink {
    fn state(&self) -> State {
        self.state.clone()
    }

    fn send(&self, op: ClientOp) -> bool {
        // Remember the operation before the transport: the link resends every
        // unconfirmed id after the next welcome, so a dropped socket loses
        // nothing and the server's dedup window makes the retry idempotent.
        if !self.pending.borrow().iter().any(|held| held.op_id == op.op_id) {
            self.pending.borrow_mut().push(op.clone());
        }
        let sent = self.send_raw(&ClientMessage::Op { op });
        if sent.is_err() {
            // Nothing reached the server: forget it so its retry is not a
            // duplicate that the caller already rolled back.
            self.pending.borrow_mut().pop();
        }
        sent.is_ok()
    }

    fn send_message(&self, message: ClientMessage) -> bool {
        self.send_raw(&message).is_ok()
    }

    fn send_presence(&self, presence: Presence) -> bool {
        self.send_raw(&ClientMessage::Presence { presence: Some(presence) }).is_ok()
    }

    fn poll(&mut self) -> Option<Event> {
        self.pump();
        self.events.pop_front()
    }
}

impl SessionLink {
    pub fn undo(&mut self, op_id: Option<String>) -> bool {
        self.send_raw(&ClientMessage::Undo { op_id }).is_ok()
    }

    pub fn redo(&mut self, op_id: Option<String>) -> bool {
        self.send_raw(&ClientMessage::Redo { op_id }).is_ok()
    }
}
