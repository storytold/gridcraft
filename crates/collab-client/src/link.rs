//! What [`Synced`](crate::Synced) needs from a connection, and the wire events it reads.
//!
//! The transport itself is platform-specific: the desktop dials with tokio
//! ([`Client`](crate::Client), native only), the browser build uses the
//! `WebSocket` the host page opened (`gridcraft-collab-web`). Both hand the
//! same [`Event`]s to the same state machine, so a local document is kept in
//! step identically in the desktop app and behind the host bridge.

use gridcraft_collab::{Applied, CabinetCommit, ClientMessage, ClientOp, Participant, Presence, Rejected, Role, SheetName};

/// The first message a session gets.
#[derive(Clone, Debug)]
pub struct Welcome {
    pub session_id: String,
    pub actor_id: String,
    pub role: Role,
    pub version: u64,
    pub epoch: u64,
    pub sheets: Vec<SheetName>,
    pub participants: Vec<Participant>,
}

/// What happened to the session, in order.
#[derive(Clone, Debug)]
pub enum Event {
    Applied(Applied),
    Rejected(Rejected),
    Presence(Vec<Participant>),
    /// The catch-up is over: the local document is at `version`.
    Resynced {
        version: u64,
        epoch: u64,
    },
    /// The whole authoritative workbook (`.xlsx` bytes) at `version`.
    Snapshot {
        version: u64,
        epoch: u64,
        bytes: Vec<u8>,
    },
    /// The room's commit was stored by the cabinet (`INTEGRATION.md` §5.2): `cabinet` is the
    /// version and etag the next commit is checked against. The document is unchanged.
    Committed {
        version: u64,
        sha256: String,
        cabinet: CabinetCommit,
    },
    /// The socket came back; catch-up frames follow.
    Reconnected {
        version: u64,
    },
    Disconnected {
        reason: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    Closed,
}

/// The state of the session, cheap to clone and safe to read from another thread.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub session_id: String,
    /// The person (the JWT subject): operations, role and undo belong to it.
    pub actor: String,
    /// The window of that person this replica is: presence belongs to it (RFC §13.6). Empty when
    /// the client sent no id.
    pub window_id: String,
    pub role: Option<Role>,
    pub version: u64,
    pub epoch: u64,
    pub sheets: Vec<SheetName>,
    pub participants: Vec<Participant>,
    pub connected: bool,
}

/// The connection a [`Synced`](crate::Synced) talks through.
///
/// The native client implements it over tokio; the browser build implements it
/// over the page's `WebSocket`. Everything above this trait — previews,
/// authoritative values, rollback, sheet keys — is shared.
pub trait Link {
    fn state(&self) -> State;
    /// Queues an operation; `false` means the connection is gone and the
    /// operation was not sent.
    fn send(&self, op: ClientOp) -> bool;
    /// Sends any message of the protocol (presence, resync).
    fn send_message(&self, message: ClientMessage) -> bool;
    fn send_presence(&self, presence: Presence) -> bool;
    /// The next event, without waiting.
    fn poll(&mut self) -> Option<Event>;
}

/// Advances the state a transport shares with its callers from one `applied`.
///
/// A part of a chunked sheet image (ADR-0013) changes nothing yet: the operation is not applied
/// and its version is not true until the last part, so the replica keeps reporting the version
/// before the first part and a broken sequence is caught up from there. The last part carries the
/// same `version`, `epoch` and sheet map as the first, so one rule covers both.
pub fn note_applied(state: &mut State, applied: &Applied) {
    if applied.more {
        return;
    }
    state.version = state.version.max(applied.version);
    state.epoch = applied.epoch;
    if let Some(sheets) = &applied.sheets {
        state.sheets = sheets.clone();
    }
}
