//! The GridCraft collaboration server: the WebSocket transport, the signature check of the
//! cabinet's token and the file lease around [`gridcraft_collab::Gateway`] (RFC §9).
//!
//! The protocol itself — the envelope, the allowlist, the delta, the room — lives in
//! `crates/collab` and is tested without a socket. This crate adds what a process needs: an HTTP
//! API for the cabinet to open, inspect and commit a session, a WebSocket that speaks the message
//! loop, JWT signature verification, the one-session-per-file lease, health checks and metrics.
//!
//! Storage (journal, snapshots, restart recovery) is the next slice: documents live in this
//! process for now, and a restart closes every session with `error{code:"internal"}`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cabinet;
pub mod config;
pub mod http;
mod jwt;
pub mod lease;
pub mod logging;
pub mod metrics;
pub mod store;
mod ws;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use gridcraft_collab::{Applied, CabinetCommit, Gateway, Room, RoomState, ServerMessage, SheetKeys};
use gridcraft_engine::{DocState, Session, Trust};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, mpsc};

pub use config::{Config, ConfigError};
pub use jwt::{JwtConfig, VerifyError};
pub use lease::Lease;
pub use store::{JournalEntry, JournalOp, Store, StoreError, StoredMeta};

use crate::cabinet::{Cabinet, Publication, PublishError, Published};
use crate::lease::Leases;
use crate::metrics::{Gauges, Metrics};

/// The base64 alphabet of the wire: `initialBytes` and commit `bytes`.
pub(crate) const B64: base64::engine::general_purpose::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// What the cabinet posts to `POST /v1/sessions` (`INTEGRATION.md` §4.1).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    pub file_id: i64,
    pub app: String,
    pub owner: String,
    #[serde(default)]
    pub initial_name: Option<String>,
    /// Base64 `.xlsx`, `.csv`, … as the cabinet holds it. Without it the session opens blank
    /// (tests and local runs).
    #[serde(default)]
    pub initial_bytes: Option<String>,
    /// The owner replaces a live session of the same file instead of getting `409`.
    #[serde(default)]
    pub takeover: bool,
}

/// What the cabinet gets back.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSession {
    pub session_id: String,
    pub ws_url: String,
    pub expires_at: u64,
}

/// `GET /v1/sessions/{id}`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub file_id: i64,
    pub app: String,
    pub owner: String,
    pub name: String,
    pub state: &'static str,
    pub version: u64,
    pub epoch: u64,
    pub participants: Vec<gridcraft_collab::Participant>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// `POST /v1/sessions/{id}/commit`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReply {
    pub name: String,
    pub bytes: String,
    pub sha256: String,
    pub version: u64,
    pub warnings: Vec<String>,
    /// The cabinet's copy, when the room posted the file itself (`INTEGRATION.md` §5.2). Absent
    /// without a callback: the host owns the stored file then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cabinet: Option<CabinetCommit>,
}

/// What can go wrong on the HTTP side. The body is `{code, message}`; no message quotes a token
/// or the contents of a cell.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, code, message: message.into() }
    }

    pub fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not-found", message)
    }

    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid", message)
    }

    /// A failure of this server (storage, replay). The message is a code or a path, never content.
    pub fn internal(message: impl std::fmt::Display) -> ApiError {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "code": self.code, "message": self.message });
        (self.status, Json(body)).into_response()
    }
}

/// One open document.
#[derive(Clone, Debug)]
pub(crate) struct DocMeta {
    pub file_id: i64,
    pub app: String,
    pub owner: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// The cabinet's copy of the last accepted commit: the etag the next commit is checked
    /// against. Written with `meta.json`, so a restart keeps the conflict check.
    pub cabinet: Option<Published>,
}

/// A session read back from the volume, replayed to its head.
struct Loaded {
    session: Session,
    keys: SheetKeys,
    state: RoomState,
    journal: Vec<JournalEntry>,
    meta: StoredMeta,
}

/// One open document: the protocol state of its room, the sockets that follow it, the operations
/// after its base and the version the volume is at. A document has its own lock (ADR-0007 §3), so
/// an operation on one book never waits for a recalculation in another.
pub(crate) struct Doc {
    pub meta: DocMeta,
    /// The protocol rules and the room of this document. The hub inside holds one document —
    /// this one — so the message loop of a connection only needs this document's lock.
    pub gateway: Gateway,
    pub conns: BTreeMap<u64, mpsc::Sender<ServerMessage>>,
    /// The open connections per `(actor, windowId)`: two windows of one person are two entries,
    /// and a reconnect of the same window is the same entry (RFC §13.6).
    pub actors: BTreeMap<(String, String), usize>,
    /// The applied operations after the base on disk, kept in memory for catch-up and trimmed by
    /// every snapshot.
    pub journal: Vec<JournalEntry>,
    /// The version `base.xlsx` is at.
    pub snapshot_version: u64,
    /// Whether a base for this document has been written (at `snapshot_version`) or was restored
    /// from the volume. A snapshot with nothing newer than that base has nothing to do.
    pub persisted: bool,
    /// Whether this room's document came from outside — the cabinet's `initialBytes` or the volume
    /// of an earlier run — rather than from an empty workbook the first participant scales up. A
    /// replica that joins a seeded room at version 0 has to adopt that document, because the
    /// journal holds nothing to replay (RFC §4.6; CodeRabbit 5846575558, `collab_ui.rs:36`).
    pub seeded: bool,
    /// Serialises the writes of this document's volume state — a snapshot's base, journal and
    /// metadata, and the metadata a commit or a takeover writes. Kept out of the document lock so
    /// the (blocking) file write does not stop the message loop.
    pub snapshot_lock: Arc<Mutex<()>>,
}

impl Doc {
    /// The operations after `from`, oldest first. Empty when the journal was compacted past it;
    /// the caller then sends a snapshot.
    pub fn journal_after(&self, from: u64) -> Vec<Applied> {
        self.journal.iter().filter(|entry| entry.applied.version > from).map(|entry| entry.applied.clone()).collect()
    }
}

/// One registry entry: the file a session serves and its document. The file id sits here too, so
/// a lease can be checked without taking the document's lock.
pub(crate) struct Entry {
    pub file_id: i64,
    pub doc: Arc<Mutex<Doc>>,
}

/// The registry behind the process lock: which documents are open and which file each session
/// leased. Everything that changes per operation lives in the document's own [`Doc`] lock; this
/// one is taken only to look a session up, to open or close one, and for the lease.
pub(crate) struct Inner {
    pub docs: BTreeMap<String, Entry>,
    pub leases: Leases,
}

impl Inner {
    fn new() -> Inner {
        Inner { docs: BTreeMap::new(), leases: Leases::new() }
    }
}

/// The server: configuration, the JWT key, the registry of open documents and metrics.
pub struct App {
    pub cfg: Config,
    pub jwt: JwtConfig,
    pub store: Option<Store>,
    /// The cabinet commit callback, when configured (`INTEGRATION.md` §5.2).
    cabinet: Option<Cabinet>,
    /// The open-document registry and the leases: short critical sections only.
    pub(crate) inner: Mutex<Inner>,
    /// The next connection id of the process.
    pub(crate) next_conn: AtomicU64,
    pub metrics: Metrics,
}

impl App {
    /// A server without a volume: everything lives in the process (tests, local runs).
    pub fn new(cfg: Config, jwt: JwtConfig) -> App {
        let cabinet = cfg.cabinet.as_ref().map(Cabinet::new);
        App { cfg, jwt, store: None, cabinet, inner: Mutex::new(Inner::new()), next_conn: AtomicU64::new(0), metrics: Metrics::default() }
    }

    /// A server that keeps its sessions on `store` and restores them at startup.
    pub fn with_store(cfg: Config, jwt: JwtConfig, store: Store) -> App {
        let cabinet = cfg.cabinet.as_ref().map(Cabinet::new);
        App { cfg, jwt, store: Some(store), cabinet, inner: Mutex::new(Inner::new()), next_conn: AtomicU64::new(0), metrics: Metrics::default() }
    }

    /// The document behind `session_id`, when it is open. The caller locks it for as long as it
    /// works on that book; another document's lock is untouched by that.
    pub(crate) async fn doc(&self, session_id: &str) -> Option<Arc<Mutex<Doc>>> {
        self.inner.lock().await.docs.get(session_id).map(|entry| entry.doc.clone())
    }

    /// Removes a document from the registry and returns the senders of its connections, so the
    /// caller can tell them the session is gone once the locks are released. The document's lock
    /// is taken without the registry held: a long operation there must not hold up other books.
    pub(crate) async fn drop_session(&self, session_id: &str) -> Vec<mpsc::Sender<ServerMessage>> {
        let removed = {
            let mut inner = self.inner.lock().await;
            inner.leases.release(session_id);
            inner.docs.remove(session_id)
        };
        let Some(entry) = removed else { return Vec::new() };
        let doc = entry.doc.lock().await;
        doc.conns.values().cloned().collect()
    }

    /// Opens a session for a file (ADR-0007 §1): loads the workbook, takes the lease and adds a
    /// room. An existing live session of the same file is `409` unless `takeover`.
    ///
    /// When the volume already holds this file, that state is authoritative: the cabinet's
    /// `initialBytes` may be older than the room, so a takeover continues the stored document.
    pub async fn create_session(&self, request: CreateSession, now: u64) -> Result<CreatedSession, ApiError> {
        if request.app != self.cfg.app {
            return Err(ApiError::invalid(format!("this server serves {}", self.cfg.app)));
        }
        if request.file_id <= 0 {
            return Err(ApiError::invalid("fileId must be positive"));
        }
        let requested_name = request.initial_name.clone().unwrap_or_else(|| format!("Book{}", request.file_id));
        let stored = self.load_stored(request.file_id).map_err(ApiError::internal)?;
        // The room's document is authoritative when it came from the cabinet or from the volume;
        // an empty workbook a first participant opened is not (see `Doc::seeded`).
        let seeded = stored.is_some() || request.initial_bytes.is_some();
        let (session, keys, state, journal, created_at, name, base_version, cabinet) = match stored {
            Some(loaded) => (
                loaded.session,
                Some(loaded.keys),
                Some(loaded.state),
                loaded.journal,
                loaded.meta.created_at,
                loaded.meta.name,
                loaded.meta.base_version,
                loaded.meta.cabinet,
            ),
            None => (self.load(&requested_name, request.initial_bytes.as_deref())?, None, None, Vec::new(), now, requested_name, 0, None),
        };
        let fresh = keys.is_none();
        let session_id = new_session_id(request.file_id, &request.owner, now);
        let ttl = self.cfg.lease_ttl.as_secs();

        let mut replaced = None;
        let expires_at = {
            let mut inner = self.inner.lock().await;
            if let Some(lease) = inner.leases.live(request.file_id, now)
                && lease.session_id != session_id
            {
                if !request.takeover {
                    return Err(ApiError::new(StatusCode::CONFLICT, "lease", format!("the file is open in session {}", lease.session_id)));
                }
                inner.leases.release(&lease.session_id);
                // The room leaves the registry here; its sockets are told below, once its own lock
                // is free — a long operation inside it must not hold up other documents.
                replaced = inner.docs.remove(&lease.session_id);
            }
            let mut gateway = Gateway::new();
            match (keys, state) {
                (Some(keys), Some(state)) => {
                    gateway.hub_mut().open_restored(&session_id, session, keys, state);
                }
                _ => {
                    gateway.hub_mut().open(&session_id, session);
                }
            }
            let lease = inner
                .leases
                .acquire(request.file_id, &session_id, &request.owner, now, ttl, request.takeover)
                .map_err(|_| ApiError::new(StatusCode::CONFLICT, "lease", "the file is open in another session"))?;
            let meta = DocMeta {
                file_id: request.file_id,
                app: request.app.clone(),
                owner: request.owner.clone(),
                name: name.clone(),
                created_at,
                updated_at: now,
                cabinet,
            };
            let doc = Doc {
                meta,
                gateway,
                conns: BTreeMap::new(),
                actors: BTreeMap::new(),
                journal,
                snapshot_version: base_version,
                persisted: false,
                seeded,
                snapshot_lock: Arc::new(Mutex::new(())),
            };
            inner.docs.insert(session_id.clone(), Entry { file_id: request.file_id, doc: Arc::new(Mutex::new(doc)) });
            lease.expires_at
        };
        let closed = match replaced {
            Some(entry) => {
                let doc = entry.doc.lock().await;
                doc.conns.values().cloned().collect::<Vec<_>>()
            }
            None => Vec::new(),
        };
        for sender in closed {
            let _ = sender.try_send(ServerMessage::Error {
                code: "session-closed".into(),
                message: "another session took over".into(),
                retryable: false,
            });
        }
        if self.store.is_some() {
            // A new session writes its base at once, so a crash before the first periodic
            // snapshot still leaves a restorable room. A continued one only needs its id updated.
            let written = if fresh { self.snapshot(&session_id).await } else { self.write_meta(&session_id).await };
            if let Err(e) = written {
                log::error!("session {} not persisted: {e}", session_id);
            }
        }
        log::info!("session opened fileId={} session={} fresh={fresh}", request.file_id, session_id);
        Ok(CreatedSession { ws_url: format!("{}/v1/sessions/{session_id}/ws", self.cfg.public_url.trim_end_matches('/')), session_id, expires_at })
    }

    /// Reads the initial bytes, or makes an empty workbook. Active content is never trusted:
    /// everything opened here is [`Trust::FromFile`] (RFC §12).
    fn load(&self, name: &str, initial: Option<&str>) -> Result<Session, ApiError> {
        let mut session = Session::new();
        match initial {
            Some(encoded) => {
                let bytes = B64.decode(encoded).map_err(|_| ApiError::invalid("initialBytes is not base64"))?;
                let (workbook, _warnings) =
                    gridcraft_engine::io::open_bytes(name, &bytes).map_err(|e| ApiError::invalid(format!("the file cannot be opened: {e}")))?;
                session.add_document(DocState::new(workbook, None, name.to_string(), Trust::FromFile));
            }
            None => {
                session.new_workbook();
            }
        }
        Ok(session)
    }

    /// A session the volume remembers, replayed to its head: the base `.xlsx`, the sheet keys,
    /// the room counters and the journal after the base.
    fn load_stored(&self, file_id: i64) -> Result<Option<Loaded>, StoreError> {
        let Some(store) = &self.store else { return Ok(None) };
        let Some(meta) = store.read_meta(file_id)? else { return Ok(None) };
        // The three files of a snapshot are named after the version `meta.json` records: a crash
        // between two writes of the next snapshot leaves this one whole.
        let bytes = store
            .read_base(file_id, meta.base_version)?
            .ok_or_else(|| StoreError::Load(format!("fileId {file_id} has meta but no base at version {}", meta.base_version)))?;
        let (workbook, _warnings) = gridcraft_engine::io::open_bytes(&meta.name, &bytes).map_err(|e| StoreError::Xlsx(e.to_string()))?;
        let keys = store
            .read_keys(file_id, meta.base_version)?
            .ok_or_else(|| StoreError::Load(format!("fileId {file_id} has no sheet keys at version {}", meta.base_version)))?;
        let keys = SheetKeys::from_file(&workbook, keys);
        let state = store.read_state(file_id, meta.base_version)?.unwrap_or_default();
        let mut session = Session::new();
        session.add_document(DocState::from_saved(workbook, None, meta.name.clone(), Trust::FromFile));
        let mut room = Room::restore(keys, state);
        // A torn tail a crash left behind is dropped on disk here, before this session appends.
        let journal = store.read_journal_repairing(file_id)?;
        // The base is at `base_version` and the journal holds every version after it, undo and
        // redo included: a gap would replay a document that never existed.
        let mut expected = meta.base_version;
        for entry in &journal {
            if entry.applied.version <= meta.base_version {
                continue;
            }
            if entry.applied.version != expected.saturating_add(1) {
                return Err(StoreError::Load(format!("fileId {file_id}: the journal jumps from {expected} to {}", entry.applied.version)));
            }
            let replayed = match &entry.op {
                JournalOp::Op { op } => room.apply(&mut session, op.clone(), &entry.actor).map(|_| ()),
                // An undo/redo's step lives in the room's memory only; its answer carries the
                // cells or sheets it put back, which is all a replay needs.
                JournalOp::Undo | JournalOp::Redo => room.replay_reversal(&mut session, &entry.applied),
            };
            replayed.map_err(|e| {
                let rejected = e.rejected();
                StoreError::Load(format!(
                    "replay of version {} refused: {} {}",
                    entry.applied.version,
                    rejected.code.code(),
                    rejected.message.as_deref().unwrap_or("")
                ))
            })?;
            expected = entry.applied.version;
        }
        if room.version() != expected {
            return Err(StoreError::Load(format!("fileId {file_id}: the journal ends at {}, replay reached {}", expected, room.version())));
        }
        // `meta.version` is the head at the last snapshot; a crash after a snapshot leaves it at
        // `base_version` while the journal is ahead. It may not claim more than the volume holds.
        if meta.version > expected {
            return Err(StoreError::Load(format!("fileId {file_id}: meta claims version {}, the volume ends at {expected}", meta.version)));
        }
        // The stacks a replay rebuilt are not the ones the operations left: a journaled undo
        // dropped a step the volume never had. A restarted room starts with empty stacks.
        room.forget_history();
        Ok(Some(Loaded { session, keys: room.sheet_keys().clone(), state: room.state(), journal, meta }))
    }

    /// Writes `meta.json` for a live session without rebuilding the base (a takeover changed only
    /// the session id).
    async fn write_meta(&self, session_id: &str) -> Result<(), StoreError> {
        let Some(store) = &self.store else { return Ok(()) };
        let Some(doc) = self.doc(session_id).await else { return Ok(()) };
        // The same serialisation a snapshot takes, so a snapshot that captured older metadata
        // cannot overwrite the session id or the cabinet etag this write carries.
        let lock = { doc.lock().await.snapshot_lock.clone() };
        let _serialised = lock.lock().await;
        let meta = {
            let doc = doc.lock().await;
            let version = doc.gateway.hub().version(session_id).unwrap_or(0);
            let epoch = doc.gateway.hub().epoch(session_id).unwrap_or(0);
            stored_meta(session_id, &doc.meta, version, epoch, doc.snapshot_version)
        };
        store.write_meta(meta.file_id, &meta)
    }

    /// Writes the authoritative state to the volume and trims the journal it covers. Called every
    /// `snapshot_ops` operations, on the periodic tick (`snapshot_interval`), at shutdown, and once
    /// when a session opens.
    pub async fn snapshot(&self, session_id: &str) -> Result<(), StoreError> {
        let Some(store) = self.store.clone() else { return Ok(()) };
        let Some(doc) = self.doc(session_id).await else { return Ok(()) };
        // One snapshot of a document at a time. Every operation past `snapshot_ops` reports the
        // snapshot as due, and the periodic tick, `flush` and a session open can all fire: without
        // this, a capture at version 5 could write its base after a capture at 10 had trimmed the
        // journal to 11, and the volume would hold a base and a journal a restart refuses.
        let lock = { doc.lock().await.snapshot_lock.clone() };
        let _serialised = lock.lock().await;
        let snapshot = {
            let doc = doc.lock().await;
            let (Some(workbook), Some(keys), Some(state)) =
                (doc.gateway.hub().workbook(session_id), doc.gateway.hub().sheet_keys(session_id), doc.gateway.hub().room_state(session_id))
            else {
                return Ok(());
            };
            // Nothing happened since the last base was written: the volume is this version.
            if doc.persisted && state.version <= doc.snapshot_version {
                return Ok(());
            }
            let base_version = state.version;
            store::Snapshot { meta: stored_meta(session_id, &doc.meta, state.version, state.epoch, base_version), workbook, keys, state }
        };
        let file_id = snapshot.meta.file_id;
        let version = snapshot.meta.version;
        let writer = store.clone();
        tokio::task::spawn_blocking(move || writer.write_snapshot(&snapshot))
            .await
            .map_err(|e| StoreError::Load(format!("snapshot task: {e}")))??;
        let mut doc = doc.lock().await;
        // Operations applied while the snapshot was being written stay: the file is rewritten
        // atomically, so either the old journal or the trimmed one is on disk.
        doc.journal.retain(|entry| entry.applied.version > version);
        store.write_journal(file_id, &doc.journal)?;
        doc.snapshot_version = version;
        doc.persisted = true;
        drop(doc);
        self.metrics.snapshots.fetch_add(1, Ordering::Relaxed);
        self.metrics.last_snapshot_at.store(now_secs(), Ordering::Relaxed);
        Ok(())
    }

    /// Appends one applied operation to the journal of `doc`, **while its lock is held**. The
    /// caller is the message loop that just applied it, so two operations of one document cannot
    /// reach the journal out of order and leave a version gap a restart would refuse.
    ///
    /// An undo/redo is journaled like any other operation: it took a version, and a client that
    /// reconnects after it — or a restart that replays the tail — must reach the same document.
    /// Returns whether a periodic snapshot is due.
    pub(crate) fn record_locked(&self, doc: &mut Doc, actor: &str, op: JournalOp, applied: &Applied, now: u64) -> bool {
        if doc.journal.last().is_some_and(|entry| applied.version <= entry.applied.version) {
            return false;
        }
        let file_id = doc.meta.file_id;
        let entry = JournalEntry { at: now, actor: actor.to_string(), op, applied: applied.clone() };
        if let Some(store) = &self.store
            && let Err(e) = store.append_journal(file_id, &entry)
        {
            log::error!("journal append failed fileId={file_id}: {e}");
            return false;
        }
        doc.journal.push(entry);
        if self.store.is_none() {
            return false;
        }
        applied.version.saturating_sub(doc.snapshot_version) >= self.cfg.snapshot_ops
    }

    /// Reopens the sessions the volume kept. A damaged directory is logged and skipped: one bad
    /// file must not stop the server from serving the others.
    pub async fn restore(&self) -> usize {
        let Some(store) = &self.store else { return 0 };
        let metas = match store.sessions() {
            Ok(metas) => metas,
            Err(e) => {
                log::error!("store scan failed: {e}");
                return 0;
            }
        };
        let mut restored = 0;
        for scanned in metas {
            match self.load_stored(scanned.file_id) {
                Ok(Some(loaded)) => {
                    let now = now_secs();
                    let mut gateway = Gateway::new();
                    let mut inner = self.inner.lock().await;
                    if gateway.hub_mut().open_restored(&scanned.session_id, loaded.session, loaded.keys, loaded.state) {
                        let meta = DocMeta {
                            file_id: scanned.file_id,
                            app: scanned.app.clone(),
                            owner: scanned.owner.clone(),
                            name: scanned.name.clone(),
                            created_at: scanned.created_at,
                            updated_at: now,
                            cabinet: scanned.cabinet.clone(),
                        };
                        let doc = Doc {
                            meta,
                            gateway,
                            conns: BTreeMap::new(),
                            actors: BTreeMap::new(),
                            journal: loaded.journal,
                            snapshot_version: scanned.base_version,
                            persisted: true,
                            // The volume holds this document's base: a joining replica adopts it.
                            seeded: true,
                            snapshot_lock: Arc::new(Mutex::new(())),
                        };
                        inner.docs.insert(scanned.session_id.clone(), Entry { file_id: scanned.file_id, doc: Arc::new(Mutex::new(doc)) });
                        restored += 1;
                        log::info!("session restored fileId={} session={}", scanned.file_id, scanned.session_id);
                    }
                }
                Ok(None) => {}
                Err(e) => log::error!("session {} not restored: {e}", scanned.session_id),
            }
        }
        restored
    }

    /// Snapshots every session that has operations the base does not cover. Called on the
    /// periodic tick and on shutdown.
    pub async fn flush(&self) -> usize {
        if self.store.is_none() {
            return 0;
        }
        let ids: Vec<String> = {
            let inner = self.inner.lock().await;
            inner.docs.keys().cloned().collect()
        };
        let mut saved = 0;
        for id in ids {
            let due = {
                let Some(doc) = self.doc(&id).await else { continue };
                let doc = doc.lock().await;
                let head = doc.gateway.hub().version(&id).unwrap_or(0);
                head > doc.snapshot_version
            };
            if !due {
                continue;
            }
            match self.snapshot(&id).await {
                Ok(()) => saved += 1,
                Err(e) => log::error!("snapshot failed session={id}: {e}"),
            }
        }
        saved
    }

    /// Writes a snapshot of every changed session every `cfg.snapshot_interval`, so a room idle
    /// on the operation count still bounds what a restart has to replay. The first tick is
    /// skipped: opening a session already writes its base.
    pub fn spawn_snapshots(self: &Arc<App>) -> tokio::task::JoinHandle<()> {
        let app = self.clone();
        // `interval` panics on a zero period; the configuration never produces one, but a caller
        // constructing `Config` by hand can.
        let every = if self.cfg.snapshot_interval.is_zero() { std::time::Duration::from_secs(5 * 60) } else { self.cfg.snapshot_interval };
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await;
            loop {
                tick.tick().await;
                let saved = app.flush().await;
                if saved > 0 {
                    log::info!("periodic snapshot: {saved} sessions");
                }
            }
        })
    }

    /// `GET /v1/sessions/{id}`.
    pub async fn session_info(&self, session_id: &str) -> Result<SessionInfo, ApiError> {
        let Some(doc) = self.doc(session_id).await else { return Err(ApiError::not_found("no such session")) };
        let doc = doc.lock().await;
        let participants = doc.gateway.hub().participants(session_id).unwrap_or_default();
        let version = doc.gateway.hub().version(session_id).unwrap_or(0);
        let epoch = doc.gateway.hub().epoch(session_id).unwrap_or(0);
        let meta = &doc.meta;
        Ok(SessionInfo {
            session_id: session_id.to_string(),
            file_id: meta.file_id,
            app: meta.app.clone(),
            owner: meta.owner.clone(),
            name: meta.name.clone(),
            state: "open",
            version,
            epoch,
            participants,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
        })
    }

    /// `POST /v1/sessions/{id}/commit` (ADR-0009): the file is written from the authoritative
    /// workbook, not from any client's copy. `expectedVersion` guards against a stale caller.
    ///
    /// With `COLLAB_CABINET_URL` set the built file is posted to the cabinet (`INTEGRATION.md`
    /// §5.2): the cabinet owns the file version, and its `{version, etag}` is returned to the
    /// caller, stored with the snapshot and shown to every subscriber. Without it the host owns
    /// the commit and only the built file's digest travels back.
    pub async fn commit(&self, session_id: &str, expected_version: Option<u64>) -> Result<CommitReply, ApiError> {
        let (name, version, bytes, file_id, expected) = {
            let Some(doc) = self.doc(session_id).await else { return Err(ApiError::not_found("no such session")) };
            let doc = doc.lock().await;
            let version = doc.gateway.hub().version(session_id).unwrap_or(0);
            if let Some(expected) = expected_version
                && expected != version
            {
                return Err(ApiError::new(StatusCode::CONFLICT, "conflict", format!("the session is at version {version}")));
            }
            let bytes =
                doc.gateway.hub().commit(session_id).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", format!("{e:?}")))?;
            (doc.meta.name.clone(), version, bytes, doc.meta.file_id, doc.meta.cabinet.as_ref().map(|published| published.etag.clone()))
        };
        self.metrics.commits.fetch_add(1, Ordering::Relaxed);
        let sha256 = hex(&Sha256::digest(&bytes));
        let published = match &self.cabinet {
            Some(cabinet) => {
                let file = file_id.to_string();
                let publication = Publication { session_id, file_id: &file, bytes: &bytes, sha256: &sha256, version, expected: expected.as_deref() };
                match cabinet.publish(&publication).await {
                    Ok(published) => Some(published),
                    Err(PublishError::Conflict(message)) => {
                        self.metrics.cabinet_conflicts.fetch_add(1, Ordering::Relaxed);
                        log::warn!("cabinet refused the commit of fileId={file_id} as a conflict: {message}");
                        let notice = ServerMessage::Error {
                            code: "cabinetConflict".into(),
                            message: format!("{message}; reopen the file to continue"),
                            retryable: true,
                        };
                        self.notify(session_id, &notice).await;
                        return Err(ApiError::new(StatusCode::CONFLICT, "cabinetConflict", message));
                    }
                    Err(error) => {
                        self.metrics.cabinet_errors.fetch_add(1, Ordering::Relaxed);
                        // The token and the body stay out of the log; the status does not.
                        log::warn!("cabinet did not store the commit of fileId={file_id}: {error}");
                        return Err(cabinet_error(error));
                    }
                }
            }
            None => None,
        };
        let cabinet = match published {
            Some(published) => {
                self.metrics.cabinet_published.fetch_add(1, Ordering::Relaxed);
                self.remember_commit(session_id, &published, expected.as_deref()).await;
                let wire = CabinetCommit { version: published.version, etag: published.etag };
                let notice = ServerMessage::Committed { version, sha256: sha256.clone(), cabinet: wire.clone() };
                self.notify(session_id, &notice).await;
                Some(wire)
            }
            None => None,
        };
        Ok(CommitReply { name, bytes: B64.encode(&bytes), sha256, version, warnings: Vec::new(), cabinet })
    }

    /// Stores the cabinet's answer with the session and with the snapshot, so the next commit —
    /// after a restart too — sends the etag it expects. Two commits may await the cabinet at once;
    /// only the answer that still matches the etag this one started from replaces it.
    async fn remember_commit(&self, session_id: &str, published: &Published, expected: Option<&str>) {
        let stored = {
            let Some(doc) = self.doc(session_id).await else { return };
            let mut doc = doc.lock().await;
            let current = doc.meta.cabinet.as_ref().map(|published| published.etag.as_str());
            if current == expected {
                doc.meta.cabinet = Some(published.clone());
                true
            } else {
                false
            }
        };
        if stored && let Err(e) = self.write_meta(session_id).await {
            log::error!("cabinet etag not stored session={session_id}: {e}");
        }
    }

    /// Queues one message for every socket of the session. The document lock is not held: a slow
    /// subscriber loses the frame, the document is unaffected.
    async fn notify(&self, session_id: &str, message: &ServerMessage) {
        let senders: Vec<mpsc::Sender<ServerMessage>> = {
            let Some(doc) = self.doc(session_id).await else { return };
            let doc = doc.lock().await;
            doc.conns.values().cloned().collect()
        };
        for sender in senders {
            if sender.try_send(message.clone()).is_err() {
                self.metrics.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// `DELETE /v1/sessions/{id}`: the cabinet closes the session and its lease.
    pub async fn close_session(&self, session_id: &str) -> Result<(), ApiError> {
        let file_id = {
            let Some(doc) = self.doc(session_id).await else {
                return Err(ApiError::not_found("no such session"));
            };
            let doc = doc.lock().await;
            doc.meta.file_id
        };
        let senders = self.drop_session(session_id).await;
        for sender in senders {
            let _ =
                sender.try_send(ServerMessage::Error { code: "session-closed".into(), message: "the session was closed".into(), retryable: false });
        }
        if let Some(store) = &self.store
            && let Err(e) = store.remove(file_id)
        {
            log::error!("session {session_id} closed but its files stayed: {e}");
        }
        log::info!("session closed session={session_id}");
        Ok(())
    }

    /// The `/metrics` body, with the gauges read under the state lock. File sizes come from the
    /// volume; reading a few directory entries per scrape is cheap next to the page itself.
    pub async fn metrics_page(&self) -> String {
        let now = now_secs();
        let (docs, leases) = {
            let inner = self.inner.lock().await;
            (inner.docs.iter().map(|(id, entry)| (id.clone(), entry.doc.clone())).collect::<Vec<_>>(), inner.leases.live_leases(now).len() as u64)
        };
        let mut participants = 0;
        let mut journal_ops = 0;
        let mut files = Vec::with_capacity(docs.len());
        for (_, doc) in &docs {
            let doc = doc.lock().await;
            participants += doc.actors.len() as u64;
            journal_ops += doc.journal.len() as u64;
            files.push(doc.meta.file_id);
        }
        let mut gauges = Gauges { sessions: docs.len() as u64, participants, leases, journal_ops, ..Gauges::default() };
        if let Some(store) = &self.store {
            gauges.journal_bytes = files.iter().filter_map(|file_id| store.journal_size(*file_id).ok().flatten()).sum();
            gauges.base_bytes = files.iter().filter_map(|file_id| store.base_size(*file_id).ok().flatten()).sum();
        }
        self.metrics.render(&gauges, env!("CARGO_PKG_VERSION"))
    }
}

/// What a failed callback means for the caller of the commit endpoint. The cabinet's answer is
/// reported as a gateway failure of the commit itself: `409` stays a conflict, a refused token and
/// a size refusal keep their own codes, and a spent retry is a bad gateway. The message never
/// quotes the token or the file.
fn cabinet_error(error: PublishError) -> ApiError {
    let (status, code) = match &error {
        PublishError::Unauthorized(_) => (StatusCode::FORBIDDEN, "cabinetUnauthorized"),
        PublishError::TooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "cabinetTooLarge"),
        PublishError::Unavailable(_) => (StatusCode::BAD_GATEWAY, "cabinetUnavailable"),
        PublishError::Refused(_) => (StatusCode::BAD_GATEWAY, "cabinetRefused"),
        PublishError::Conflict(_) => (StatusCode::CONFLICT, "cabinetConflict"),
    };
    ApiError::new(status, code, error.to_string())
}

/// The `meta.json` of a live session.
fn stored_meta(session_id: &str, meta: &DocMeta, version: u64, epoch: u64, base_version: u64) -> StoredMeta {
    StoredMeta {
        session_id: session_id.to_string(),
        file_id: meta.file_id,
        app: meta.app.clone(),
        owner: meta.owner.clone(),
        name: meta.name.clone(),
        created_at: meta.created_at,
        updated_at: meta.updated_at,
        version,
        epoch,
        base_version,
        cabinet: meta.cabinet.clone(),
    }
}

/// Seconds since the Unix epoch, `0` when the clock is before it. Never panics.
pub fn now_secs() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0)
}

/// A session id that cannot be guessed from a file id: a hash of the time, a counter and the
/// owner. The token, not the id, is the authority.
fn new_session_id(file_id: i64, owner: &str, now: u64) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hash = Sha256::new();
    hash.update(now.to_le_bytes());
    hash.update(file_id.to_le_bytes());
    hash.update(counter.to_le_bytes());
    hash.update(owner.as_bytes());
    hex(&hash.finalize())
}

/// Lowercase hex, without a panic path.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A constant-time comparison of two secrets (`Authorization: Bearer …`), hashed first so the
/// length itself does not leak.
pub(crate) fn token_matches(actual: &str, expected: &str) -> bool {
    let a = Sha256::digest(actual.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_differ_and_are_hex() {
        let a = new_session_id(7, "ann", 1);
        let b = new_session_id(7, "ann", 1);
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_are_compared_by_hash() {
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("secret", "secreT"));
        assert!(!token_matches("secret", "secret "));
    }

    /// Two documents do not share a lock (ADR-0007 §3). While one book sits inside a long
    /// operation, the other opens sessions, reports and commits, and the busy one answers as soon
    /// as its own lock is back.
    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn a_busy_document_does_not_hold_up_another() {
        use std::time::Duration;

        let cfg = Config::default();
        let app = App::new(cfg.clone(), JwtConfig::hs256(b"test-secret").unwrap());
        let request = |file_id: i64| CreateSession {
            file_id,
            app: cfg.app.clone(),
            owner: "ann".into(),
            initial_name: None,
            initial_bytes: None,
            takeover: false,
        };
        let first = app.create_session(request(101), 1).await.unwrap();
        let second = app.create_session(request(102), 1).await.unwrap();

        let busy = app.doc(&first.session_id).await.unwrap();
        let guard = busy.lock().await; // one long recalculation of the first document

        // The registry is not behind that lock: another session opens while the first is busy.
        let third = tokio::time::timeout(Duration::from_secs(5), app.create_session(request(103), 1)).await.expect("the registry answers");
        assert!(third.is_ok(), "a third document opens while the first is busy: {third:?}");

        // So does every per-document path of the second document.
        let info = tokio::time::timeout(Duration::from_secs(5), app.session_info(&second.session_id)).await.expect("the second document answers");
        assert_eq!(info.unwrap().file_id, 102);
        let commit = tokio::time::timeout(Duration::from_secs(5), app.commit(&second.session_id, None)).await.expect("the second document commits");
        assert!(!commit.unwrap().bytes.is_empty());

        // The busy document itself still waits for its own lock, and answers once it is back.
        assert!(tokio::time::timeout(Duration::from_millis(100), app.session_info(&first.session_id)).await.is_err());
        drop(guard);
        assert!(app.session_info(&first.session_id).await.is_ok());
    }

    /// A snapshot only writes when the volume does not already hold that version: two snapshots
    /// with no operation in between, a periodic tick racing a session open, and a stale capture
    /// cannot put an older base on disk than the journal it trims.
    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn a_snapshot_with_nothing_new_does_not_rewrite_the_base() {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("gridcraft-collab-snapshot-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cfg = Config::default();
        let app = App::with_store(cfg.clone(), JwtConfig::hs256(b"test-secret").unwrap(), Store::open(&root).unwrap());
        let created = app
            .create_session(
                CreateSession { file_id: 7, app: cfg.app.clone(), owner: "ann".into(), initial_name: None, initial_bytes: None, takeover: false },
                1,
            )
            .await
            .unwrap();
        let after_open = app.metrics.snapshots.load(Ordering::Relaxed);
        assert_eq!(after_open, 1, "a fresh session writes its base at once");
        app.snapshot(&created.session_id).await.unwrap();
        assert_eq!(app.metrics.snapshots.load(Ordering::Relaxed), after_open, "nothing happened since the base");
        let _ = std::fs::remove_dir_all(&root);
    }
}
