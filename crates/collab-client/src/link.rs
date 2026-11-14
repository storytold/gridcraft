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

/// The most chunks one snapshot may arrive in.
///
/// RFC §4.1 caps a document at 512 MB and the server sends 768 KiB per chunk (683 full chunks);
/// 1024 leaves room for a server that picks a smaller chunk and is still a bound. `total` comes
/// from the wire and sizes a `Vec`, so it is never trusted as it stands
/// (CodeRabbit 5851190259, `crates/collab-web/src/link.rs:169`).
pub const MAX_SNAPSHOT_CHUNKS: u32 = 1024;

/// A chunked snapshot being assembled (RFC §4.1).
pub struct Snapshot {
    version: u64,
    epoch: u64,
    chunks: Vec<Option<Vec<u8>>>,
}

impl Snapshot {
    /// `None` when `total` cannot describe a document: no chunks at all, or more than
    /// [`MAX_SNAPSHOT_CHUNKS`].
    pub fn new(version: u64, epoch: u64, total: u32) -> Option<Snapshot> {
        if total == 0 || total > MAX_SNAPSHOT_CHUNKS {
            return None;
        }
        Some(Snapshot { version, epoch, chunks: vec![None; total as usize] })
    }
}

/// Takes one chunk of a snapshot: the workbook bytes when the last one is in.
///
/// A chunk that does not belong to the buffer — another `version`, `epoch` or `total` — starts a
/// fresh one, so the parts of two snapshots are never woven together. The caller drops the
/// buffer when the socket closes or a new `welcome` starts a new catch-up: a snapshot is about one
/// connection, and half of the old one must not continue into the next
/// (CodeRabbit 5851190259, `crates/collab-web/src/link.rs:169`).
pub fn snapshot_chunk(buffer: &mut Option<Snapshot>, version: u64, epoch: u64, total: u32, seq: u32, chunk: Vec<u8>) -> Option<(u64, u64, Vec<u8>)> {
    let fits = buffer.as_ref().is_some_and(|held| held.version == version && held.epoch == epoch && held.chunks.len() == total as usize);
    if !fits {
        *buffer = Snapshot::new(version, epoch, total);
    }
    let held = buffer.as_mut()?;
    if let Some(slot) = held.chunks.get_mut(seq as usize) {
        *slot = Some(chunk);
    }
    if !held.chunks.iter().all(Option::is_some) {
        return None;
    }
    let done = buffer.take()?;
    let mut bytes = Vec::new();
    for part in done.chunks.iter().flatten() {
        bytes.extend_from_slice(part);
    }
    Some((done.version, done.epoch, bytes))
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

#[cfg(test)]
mod tests {
    use super::{MAX_SNAPSHOT_CHUNKS, snapshot_chunk};

    /// Chunks of two snapshots must never be woven into one workbook: a socket that dropped in
    /// the middle of one catch-up and a new connection's snapshot are separate documents.
    #[test]
    fn chunks_of_two_snapshots_are_not_woven_together() {
        let mut buffer = None;
        assert_eq!(snapshot_chunk(&mut buffer, 4, 0, 2, 0, b"aa".to_vec()), None);
        // Another snapshot resets the buffer instead of filling half of it.
        assert_eq!(snapshot_chunk(&mut buffer, 5, 0, 2, 0, b"bb".to_vec()), None);
        assert_eq!(snapshot_chunk(&mut buffer, 5, 0, 2, 1, b"cc".to_vec()), Some((5, 0, b"bbcc".to_vec())));
        assert!(buffer.is_none(), "the buffer is gone once the snapshot is whole");
    }

    /// `total` comes from the wire and sizes a `Vec`: a zero or an impossible count allocates
    /// nothing and starts no buffer.
    #[test]
    fn a_total_off_the_wire_is_bounded() {
        let mut buffer = None;
        assert_eq!(snapshot_chunk(&mut buffer, 4, 0, 0, 0, Vec::new()), None, "a snapshot of no chunks");
        assert_eq!(snapshot_chunk(&mut buffer, 4, 0, MAX_SNAPSHOT_CHUNKS + 1, 0, b"aa".to_vec()), None, "more chunks than a document can have");
        assert!(buffer.is_none(), "nothing was allocated or kept");
        assert_eq!(snapshot_chunk(&mut buffer, 4, 0, 1, 0, b"aa".to_vec()), Some((4, 0, b"aa".to_vec())), "a count that fits still works");
    }
}
