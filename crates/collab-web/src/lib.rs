//! Web-side collaboration session.
//!
//! The browser build gets its session from the cabinet through the host bridge
//! (`docs/collab/host-bridge.md`): `gridcraft:session` carries `session_id`,
//! `ws_url`, the JWT and the role. This crate keeps the same transport-free
//! state machine as the desktop (`gridcraft_collab_client::Synced`), dials the
//! URL with the browser's `WebSocket`, and exposes a `Collab` with the surface
//! the participants panel and the command gate already use.
//!
//! The token lives in the link and nowhere else: it is never put in the socket
//! URL, a log line, `localStorage` or preferences, and everything that prints a
//! session redacts it.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gridcraft_collab::{ClientMessage, ClientOp, Participant, Presence, Role, classify};
use gridcraft_collab_client::{Event, Link, State, Synced, Welcome};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::collab::{Collab as CollabTrait, PresenceView, SyncStatus};
use gridcraft_ui_egui::monotonic_ms;

mod link;

pub use link::SessionLink;

#[cfg(target_arch = "wasm32")]
mod socket;

/// The session the host bridge opened. A credential-bearing value, so it has a
/// redacted `Debug` and no `Serialize`: it must never reach preferences, a log
/// line or a URL.
#[derive(Clone, PartialEq, Eq)]
pub struct HostSession {
    session_id: String,
    ws_url: String,
    token: String,
    role: String,
}

impl HostSession {
    pub fn new(session_id: impl Into<String>, ws_url: impl Into<String>, token: impl Into<String>, role: impl Into<String>) -> HostSession {
        HostSession { session_id: session_id.into(), ws_url: ws_url.into(), token: token.into(), role: role.into() }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The sync server's WebSocket URL. The token is not part of it.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Replace the JWT: the cabinet pushes a fresh one before the old expires.
    pub fn set_token(&mut self, token: impl Into<String>) {
        self.token = token.into();
    }

    #[cfg(target_arch = "wasm32")]
    fn parsed_role(&self) -> Result<Role, String> {
        serde_json::from_value::<Role>(serde_json::Value::String(self.role.clone()))
            .map_err(|_| format!("unknown collaboration role {:?}", self.role))
    }
}

impl fmt::Debug for HostSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostSession")
            .field("session_id", &self.session_id)
            .field("ws_url", &self.ws_url)
            .field("token", &"<redacted>")
            .field("role", &self.role)
            .finish()
    }
}

/// What the host bridge delivered to the app.
#[derive(Clone, Debug)]
pub enum HostEvent {
    /// `gridcraft:session` (host → app): open a collaboration session.
    Session(HostSession),
    /// `gridcraft:committed` (host → app): the cabinet stored the commit.
    Committed { version: u64, etag: String },
}

/// What the app must do after one host event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Dial the session; the host opened it, so the app owns no network call
    /// before this point.
    Connect(HostSession),
    /// The cabinet stored the commit; show its version and etag.
    Committed { version: u64, etag: String },
}

/// The host-side collaboration state the shell keeps between frames.
///
/// It is deliberately transport-free, so the `gridcraft:session` /
/// `gridcraft:committed` handling is tested headlessly without a browser.
#[derive(Default, Debug)]
pub struct HostState {
    /// The session the host opened, if any. Without it the app stays
    /// single-user and makes no network call.
    pub session: Option<HostSession>,
    /// The last commit the cabinet confirmed.
    pub committed: Option<(u64, String)>,
    /// Whether the socket has joined the room at least once.
    pub connected: bool,
}

impl HostState {
    /// Apply one host message. Anything else leaves the app single-user.
    pub fn on_event(&mut self, event: HostEvent) -> HostAction {
        match event {
            HostEvent::Session(session) => {
                self.connected = false;
                self.session = Some(session.clone());
                HostAction::Connect(session)
            }
            HostEvent::Committed { version, etag } => {
                self.committed = Some((version, etag.clone()));
                HostAction::Committed { version, etag }
            }
        }
    }

    pub fn mark_connected(&mut self) {
        self.connected = true;
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }
}

/// The cabinet's sync-status word (`session-state.status`): the contract names
/// `synced`, `pending`, `offline` and `error`, and the cabinet shows it as is.
pub fn host_status(status: &SyncStatus) -> &'static str {
    match status {
        SyncStatus::Off => "offline",
        SyncStatus::Error(_) => "error",
        SyncStatus::Connecting => "pending",
        SyncStatus::Online { .. } => "synced",
    }
}

/// One participant in the shape the host bridge sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostParticipant {
    pub client_id: String,
    pub role: String,
    pub color: Option<String>,
}

pub fn host_participants(participants: &[Participant]) -> Vec<HostParticipant> {
    participants
        .iter()
        .map(|participant| HostParticipant { client_id: participant.actor.clone(), role: role_word(participant.role).into(), color: None })
        .collect()
}

fn role_word(role: Role) -> &'static str {
    match role {
        Role::Viewer => "viewer",
        Role::Editor => "editor",
        Role::Owner => "owner",
    }
}

/// A handle to the link [`Synced`] owns. The app keeps one too, so it can feed
/// the protocol (`on_text`) and read the participant list the link holds.
#[derive(Clone)]
struct LinkHandle(Rc<RefCell<SessionLink>>);

impl Link for LinkHandle {
    fn state(&self) -> State {
        self.0.borrow().state()
    }

    fn send(&self, op: ClientOp) -> bool {
        self.0.borrow().send(op)
    }

    fn send_message(&self, message: ClientMessage) -> bool {
        self.0.borrow().send_message(message)
    }

    fn send_presence(&self, presence: Presence) -> bool {
        self.0.borrow().send_presence(presence)
    }

    fn poll(&mut self) -> Option<Event> {
        self.0.borrow_mut().poll()
    }
}

/// A live web collaboration session.
pub struct Collab {
    sync: Synced,
    link: LinkHandle,
    role: Role,
    session_id: String,
    status: SyncStatus,
    participants: Vec<PresenceView>,
    committed: Option<(u64, String)>,
    last_presence_ms: f64,
    last_error: Option<String>,
}

impl Collab {
    /// Dial the session the host sent. The token goes in the first frame, never
    /// in the URL.
    #[cfg(target_arch = "wasm32")]
    pub fn connect(session: &HostSession, actor: &str, client_id: &str) -> Result<Collab, String> {
        let role = session.parsed_role()?;
        let name = actor.to_string();
        let link = SessionLink::open(session.session_id(), session.ws_url(), session.token(), role, actor, &name, client_id)?;
        Ok(Collab::with_link(link, session.session_id(), actor, role, client_id))
    }

    /// A session without a socket: the tests feed it the server's frames.
    pub fn offline(session_id: &str, actor: &str, role: Role, client_id: &str) -> Collab {
        let link = SessionLink::offline(session_id, actor, role, actor, client_id);
        Collab::with_link(link, session_id, actor, role, client_id)
    }

    fn with_link(link: SessionLink, session_id: &str, actor: &str, role: Role, client_id: &str) -> Collab {
        let handle = LinkHandle(Rc::new(RefCell::new(link)));
        let welcome = Welcome {
            session_id: session_id.to_string(),
            actor_id: actor.to_string(),
            role,
            version: 0,
            epoch: 0,
            sheets: Vec::new(),
            participants: Vec::new(),
        };
        let sync = Synced::with_link(Box::new(handle.clone()), &welcome, client_id.to_string());
        Collab {
            sync,
            link: handle,
            role,
            session_id: session_id.to_string(),
            status: SyncStatus::Connecting,
            participants: Vec::new(),
            committed: None,
            last_presence_ms: 0.0,
            last_error: None,
        }
    }

    /// Drive the state machine with one server frame without a browser. Returns
    /// whether the frame changed anything.
    pub fn on_text(&mut self, text: &str, session: &mut Session) -> bool {
        if !self.link.0.borrow_mut().on_text(text) {
            return false;
        }
        let _ = self.sync.poll(session);
        true
    }

    pub fn state(&self) -> State {
        self.link.state()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// The last commit the cabinet confirmed.
    pub fn committed(&self) -> Option<&(u64, String)> {
        self.committed.as_ref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Replace the JWT the cabinet pushed before the old one expired and dial
    /// again; unconfirmed operations keep their ids and are resent.
    pub fn renew_token(&mut self, token: &str) {
        self.link.0.borrow_mut().renew_token(token);
    }

    /// Ask the cabinet to commit: the host owns the stored file, so no bytes
    /// travel (the contract's `bytes?` is for a host that does not hold it).
    pub fn request_commit(&self) -> (String, Option<Vec<u8>>) {
        (self.session_id.clone(), None)
    }

    /// Send this participant's cursor and selection at most every 100 ms.
    fn publish_presence(&mut self, session: &Session) {
        if !matches!(self.status, SyncStatus::Online { .. }) {
            return;
        }
        let now = monotonic_ms();
        if now - self.last_presence_ms < 100.0 {
            return;
        }
        self.last_presence_ms = now;
        let Some(doc) = session.active() else { return };
        let Some(sheet) = doc.wb.sheet(doc.wb.active_sheet()) else { return };
        let Some(key) = self.sync.key_for_name(&sheet.name) else { return };
        let selection = &doc.selection;
        let ranges: Vec<String> = selection.ranges.iter().map(RangeRef::a1).collect();
        self.link.send_presence(Presence { sheet_key: key, active: selection.active.to_string(), ranges, viewport: None });
    }

    /// Rebuild the drawing list from the server's participants.
    fn views(&mut self, state: &State) {
        let own = &state.actor;
        self.participants = state
            .participants
            .iter()
            .filter(|participant| &participant.actor != own)
            .filter_map(|participant| {
                let presence = participant.presence.as_ref()?;
                let sheet = state.sheets.iter().find(|sheet| sheet.key == presence.sheet_key)?.name.clone();
                let active = CellRef::parse(&presence.active)?;
                let ranges = presence.ranges.iter().filter_map(|range| RangeRef::parse(range)).collect();
                Some(PresenceView {
                    actor: participant.actor.clone(),
                    name: participant.name.clone(),
                    color: PresenceView::colour_for(&participant.actor),
                    sheet,
                    active,
                    ranges,
                })
            })
            .collect();
    }
}

impl CollabTrait for Collab {
    fn session_participants(&self) -> Vec<gridcraft_ui_egui::collab::SessionParticipant> {
        host_participants(&self.link.state().participants)
            .into_iter()
            .map(|participant| gridcraft_ui_egui::collab::SessionParticipant {
                client_id: participant.client_id,
                role: participant.role,
                color: participant.color,
            })
            .collect()
    }

    fn execute(&mut self, session: &mut Session, id: &str, params: serde_json::Value) -> Option<Result<serde_json::Value, String>> {
        classify(id)?;
        Some(self.sync.execute(session, id, params).map_err(|error| error.to_string()))
    }

    fn poll(&mut self, session: &mut Session) -> Vec<PresenceView> {
        let events = self.sync.poll(session);
        let state = self.link.state();
        if !state.sheets.is_empty() {
            self.sync.set_sheets(&state.sheets);
        }
        for event in &events {
            match event {
                Event::Reconnected { version } => {
                    self.status = SyncStatus::Online { version: *version, participants: state.participants.len() };
                    self.last_error = None;
                }
                Event::Applied(applied) => {
                    self.status = SyncStatus::Online { version: applied.version, participants: state.participants.len() };
                }
                Event::Rejected(rejected) => {
                    self.last_error = rejected.message.clone();
                }
                Event::Disconnected { reason } => {
                    self.status = SyncStatus::Connecting;
                    self.last_error = Some(reason.clone());
                }
                Event::Error { message, .. } => {
                    self.status = SyncStatus::Error(message.clone());
                    self.last_error = Some(message.clone());
                }
                Event::Snapshot { version, .. } => {
                    self.status = SyncStatus::Online { version: *version, participants: state.participants.len() };
                }
                Event::Resynced { version, .. } => {
                    self.status = SyncStatus::Online { version: *version, participants: state.participants.len() };
                }
                Event::Presence(_) | Event::Closed => {}
            }
        }
        self.views(&state);
        self.publish_presence(session);
        self.participants.clone()
    }

    fn status(&self) -> SyncStatus {
        self.status.clone()
    }

    fn read_only(&self) -> bool {
        self.role == Role::Viewer
    }

    fn take_notice(&mut self) -> Option<String> {
        self.sync.take_notice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_collab::{ClientOp, Kind, Payload, Room, ServerMessage, SheetName, Target};
    use gridcraft_engine::core::{CellRef, Value};
    use gridcraft_ui_egui::collab::Collab as CollabTrait;
    use serde_json::json;

    fn replica() -> Session {
        let mut session = Session::new();
        session.new_workbook();
        session
    }

    fn value(session: &Session, a1: &str) -> Value {
        let at = CellRef::parse(a1).expect("a valid address");
        session.doc().expect("a document").wb.active().expect("a sheet").value(at)
    }

    fn sheet_map(room: &Room, server: &Session) -> Vec<SheetName> {
        let Ok(doc) = server.doc() else { return Vec::new() };
        (0..doc.wb.sheets.len())
            .filter_map(|index| {
                let key = room.sheet_keys().key_for_index(&doc.wb, index)?;
                Some(SheetName { key: key.to_string(), name: doc.wb.sheets[index].name.clone() })
            })
            .collect()
    }

    fn wire(message: &ServerMessage) -> String {
        serde_json::to_string(message).expect("a server frame")
    }

    #[test]
    fn a_session_message_is_stored_and_asks_for_a_connection() {
        let mut state = HostState::default();
        assert!(!state.has_session());
        let session = HostSession::new("s1", "wss://collab.example/ws", "jwt-secret", "editor");
        assert_eq!(state.on_event(HostEvent::Session(session.clone())), HostAction::Connect(session));
        assert!(state.has_session());
        let debug = format!("{:?}", state.session.as_ref().expect("the session is stored"));
        assert!(debug.contains("s1"), "{debug}");
        assert!(!debug.contains("jwt-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }

    #[test]
    fn a_commit_message_is_recorded_for_the_ui() {
        let mut state = HostState::default();
        let action = state.on_event(HostEvent::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(action, HostAction::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(state.committed, Some((7, "\"e7\"".to_string())));
    }

    #[test]
    fn status_words_follow_the_host_contract() {
        assert_eq!(host_status(&SyncStatus::Online { version: 3, participants: 2 }), "synced");
        assert_eq!(host_status(&SyncStatus::Connecting), "pending");
        assert_eq!(host_status(&SyncStatus::Off), "offline");
        assert_eq!(host_status(&SyncStatus::Error("boom".into())), "error");
    }

    /// Two browser replicas on one room: the frames the server broadcasts decide
    /// the cells — no desktop `Client`, no socket. This is the protocol half of
    /// a `gridcraft:session` opened by the host page. Both replicas see a third
    /// participant's edits, which is the path a web client applies in full
    /// (its own edits are previewed locally first).
    #[test]
    fn two_web_replicas_converge_on_authoritative_cells() {
        let mut server = replica();
        let mut room = Room::new(&server);
        let sheets = sheet_map(&room, &server);
        let key = sheets.first().expect("a sheet key").key.clone();
        let mut ann = Collab::offline("s1", "ann", Role::Editor, "ann-1");
        let mut bob = Collab::offline("s1", "bob", Role::Editor, "bob-1");
        let mut ann_session = replica();
        let mut bob_session = replica();

        let welcome = wire(&ServerMessage::Welcome {
            session_id: "s1".into(),
            actor_id: "ann".into(),
            role: Role::Editor,
            version: 0,
            epoch: 0,
            sheets: sheets.clone(),
            participants: Vec::new(),
            lease: None,
        });
        assert!(ann.on_text(&welcome, &mut ann_session));
        let welcome = wire(&ServerMessage::Welcome {
            session_id: "s1".into(),
            actor_id: "bob".into(),
            role: Role::Editor,
            version: 0,
            epoch: 0,
            sheets,
            participants: Vec::new(),
            lease: None,
        });
        assert!(bob.on_text(&welcome, &mut bob_session));

        let set = |op_id: &str, a1: &str, input: serde_json::Value| ClientOp {
            op_id: op_id.into(),
            doc: "s1".into(),
            base_version: 0,
            epoch: 0,
            kind: Kind::Command,
            target: Target { sheet: key.clone(), a1: Some(a1.into()) },
            payload: Payload { cmd: "cell.set".into(), params: json!({ "input": input }) },
            preview: None,
        };
        let applied = room.apply(&mut server, set("carol-0", "A1", json!(42)), "carol").expect("the room applies A1");
        let applied_frame = wire(&ServerMessage::Applied { applied });
        assert!(ann.on_text(&applied_frame, &mut ann_session));
        assert!(bob.on_text(&applied_frame, &mut bob_session));

        let applied = room.apply(&mut server, set("carol-1", "B1", json!("=A1*2")), "carol").expect("the room applies B1");
        let applied_frame = wire(&ServerMessage::Applied { applied });
        assert!(ann.on_text(&applied_frame, &mut ann_session));
        assert!(bob.on_text(&applied_frame, &mut bob_session));

        assert_eq!(value(&ann_session, "A1"), Value::Number(42.0));
        assert_eq!(value(&bob_session, "A1"), Value::Number(42.0), "the other browser applies the authoritative delta");
        assert_eq!(value(&bob_session, "B1"), Value::Number(84.0), "the formula follows on both replicas");
        assert_eq!(ann.state().version, 2);
        assert_eq!(bob.state().version, 2);
    }

    /// A viewer's browser replica is read-only; the room still feeds it.
    #[test]
    fn a_viewer_is_read_only() {
        let mut collab = Collab::offline("s1", "vic", Role::Viewer, "vic-1");
        let mut session = replica();
        assert!(collab.read_only());
        assert!(collab.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).is_some_and(|result| result.is_err()));
    }
}
