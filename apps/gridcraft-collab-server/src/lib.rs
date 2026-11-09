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
use gridcraft_collab::{Applied, Gateway, Room, RoomState, ServerMessage, SheetKeys};
use gridcraft_engine::{DocState, Session, Trust};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, mpsc};

pub use config::{Config, ConfigError};
pub use jwt::{JwtConfig, VerifyError};
pub use lease::Lease;
pub use store::{JournalEntry, JournalOp, Store, StoreError, StoredMeta};

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
}

/// A session read back from the volume, replayed to its head.
struct Loaded {
    session: Session,
    keys: SheetKeys,
    state: RoomState,
    journal: Vec<JournalEntry>,
    meta: StoredMeta,
}

/// State behind one mutex: the protocol, the documents and the sockets. A command runs under this
/// lock, because a room applies to one engine session in process order (ADR-0003).
pub(crate) struct Inner {
    pub gateway: Gateway,
    pub sessions: BTreeMap<String, DocMeta>,
    pub leases: Leases,
    pub conns: BTreeMap<String, BTreeMap<u64, mpsc::Sender<ServerMessage>>>,
    pub actors: BTreeMap<String, BTreeMap<String, usize>>,
    pub next_conn: u64,
    /// The applied operations after the base on disk, kept in memory for catch-up and trimmed by
    /// every snapshot.
    pub journal: BTreeMap<String, Vec<JournalEntry>>,
    /// The version `base.xlsx` is at.
    pub snapshot_version: BTreeMap<String, u64>,
}

impl Inner {
    fn new() -> Inner {
        Inner {
            gateway: Gateway::new(),
            sessions: BTreeMap::new(),
            leases: Leases::new(),
            conns: BTreeMap::new(),
            actors: BTreeMap::new(),
            next_conn: 0,
            journal: BTreeMap::new(),
            snapshot_version: BTreeMap::new(),
        }
    }

    /// Drops a document and returns the senders of its connections, so the caller can tell them
    /// the session is gone once the lock is released.
    pub fn drop_session(&mut self, session_id: &str) -> Vec<mpsc::Sender<ServerMessage>> {
        self.gateway.hub_mut().close(session_id);
        self.sessions.remove(session_id);
        self.actors.remove(session_id);
        self.leases.release(session_id);
        self.journal.remove(session_id);
        self.snapshot_version.remove(session_id);
        self.conns.remove(session_id).map(|senders| senders.into_values().collect()).unwrap_or_default()
    }

    pub fn participants_total(&self) -> u64 {
        self.actors.values().map(|actors| actors.len() as u64).sum()
    }

    /// The operations after `from`, oldest first. Empty when the journal was compacted past it;
    /// the caller then sends a snapshot.
    pub fn journal_after(&self, session_id: &str, from: u64) -> Vec<Applied> {
        self.journal
            .get(session_id)
            .map(|entries| entries.iter().filter(|entry| entry.applied.version > from).map(|entry| entry.applied.clone()).collect())
            .unwrap_or_default()
    }
}

/// The server: configuration, the JWT key, one lock over the protocol state and metrics.
pub struct App {
    pub cfg: Config,
    pub jwt: JwtConfig,
    pub store: Option<Store>,
    pub(crate) inner: Mutex<Inner>,
    pub metrics: Metrics,
}

impl App {
    /// A server without a volume: everything lives in the process (tests, local runs).
    pub fn new(cfg: Config, jwt: JwtConfig) -> App {
        App { cfg, jwt, store: None, inner: Mutex::new(Inner::new()), metrics: Metrics::default() }
    }

    /// A server that keeps its sessions on `store` and restores them at startup.
    pub fn with_store(cfg: Config, jwt: JwtConfig, store: Store) -> App {
        App { cfg, jwt, store: Some(store), inner: Mutex::new(Inner::new()), metrics: Metrics::default() }
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
        let (session, keys, state, journal, created_at, name, base_version) = match stored {
            Some(loaded) => (
                loaded.session,
                Some(loaded.keys),
                Some(loaded.state),
                loaded.journal,
                loaded.meta.created_at,
                loaded.meta.name,
                loaded.meta.base_version,
            ),
            None => (self.load(&requested_name, request.initial_bytes.as_deref())?, None, None, Vec::new(), now, requested_name, 0),
        };
        let fresh = keys.is_none();
        let session_id = new_session_id(request.file_id, &request.owner, now);
        let ttl = self.cfg.lease_ttl.as_secs();

        let mut closed = Vec::new();
        let expires_at = {
            let mut inner = self.inner.lock().await;
            let existing = inner.leases.live(request.file_id, now);
            if let Some(lease) = &existing
                && lease.session_id != session_id
            {
                if !request.takeover {
                    return Err(ApiError::new(StatusCode::CONFLICT, "lease", format!("the file is open in session {}", lease.session_id)));
                }
                closed = inner.drop_session(&lease.session_id);
            }
            match (keys, state) {
                (Some(keys), Some(state)) => {
                    inner.gateway.hub_mut().open_restored(&session_id, session, keys, state);
                }
                _ => {
                    inner.gateway.hub_mut().open(&session_id, session);
                }
            }
            let lease = inner
                .leases
                .acquire(request.file_id, &session_id, &request.owner, now, ttl, request.takeover)
                .map_err(|_| ApiError::new(StatusCode::CONFLICT, "lease", "the file is open in another session"))?;
            inner.sessions.insert(
                session_id.clone(),
                DocMeta {
                    file_id: request.file_id,
                    app: request.app.clone(),
                    owner: request.owner.clone(),
                    name: name.clone(),
                    created_at,
                    updated_at: now,
                },
            );
            inner.journal.insert(session_id.clone(), journal);
            inner.snapshot_version.insert(session_id.clone(), base_version);
            lease.expires_at
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
            let persisted = if fresh { self.snapshot(&session_id).await } else { self.write_meta(&session_id).await };
            if let Err(e) = persisted {
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
        let bytes = store.read_base(file_id)?.ok_or_else(|| StoreError::Load(format!("fileId {file_id} has meta but no base")))?;
        let (workbook, _warnings) = gridcraft_engine::io::open_bytes(&meta.name, &bytes).map_err(|e| StoreError::Xlsx(e.to_string()))?;
        let keys = store.read_keys(file_id)?.ok_or_else(|| StoreError::Load(format!("fileId {file_id} has no sheet keys")))?;
        let keys = SheetKeys::from_file(&workbook, keys);
        let state = store.read_state(file_id)?.unwrap_or_default();
        let mut session = Session::new();
        session.add_document(DocState::from_saved(workbook, None, meta.name.clone(), Trust::FromFile));
        let mut room = Room::restore(keys, state);
        let journal = store.read_journal(file_id)?;
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
        let meta = {
            let inner = self.inner.lock().await;
            let Some(meta) = inner.sessions.get(session_id) else { return Ok(()) };
            let version = inner.gateway.hub().version(session_id).unwrap_or(0);
            let epoch = inner.gateway.hub().epoch(session_id).unwrap_or(0);
            stored_meta(session_id, meta, version, epoch, inner.snapshot_version.get(session_id).copied().unwrap_or(0))
        };
        store.write_meta(meta.file_id, &meta)
    }

    /// Writes the authoritative state to the volume and trims the journal it covers. Called every
    /// `snapshot_ops` operations, on the periodic tick (`snapshot_interval`), at shutdown, and once
    /// when a session opens.
    pub async fn snapshot(&self, session_id: &str) -> Result<(), StoreError> {
        let Some(store) = self.store.clone() else { return Ok(()) };
        let snapshot = {
            let inner = self.inner.lock().await;
            let Some(meta) = inner.sessions.get(session_id) else { return Ok(()) };
            let Some(workbook) = inner.gateway.hub().workbook(session_id) else { return Ok(()) };
            let Some(keys) = inner.gateway.hub().sheet_keys(session_id) else { return Ok(()) };
            let Some(state) = inner.gateway.hub().room_state(session_id) else { return Ok(()) };
            let base_version = state.version;
            store::Snapshot { meta: stored_meta(session_id, meta, state.version, state.epoch, base_version), workbook, keys, state }
        };
        let file_id = snapshot.meta.file_id;
        let version = snapshot.meta.version;
        let writer = store.clone();
        tokio::task::spawn_blocking(move || writer.write_snapshot(&snapshot))
            .await
            .map_err(|e| StoreError::Load(format!("snapshot task: {e}")))??;
        let mut inner = self.inner.lock().await;
        if let Some(journal) = inner.journal.get_mut(session_id) {
            // Operations applied while the snapshot was being written stay: the file is
            // rewritten atomically, so either the old journal or the trimmed one is on disk.
            journal.retain(|entry| entry.applied.version > version);
            store.write_journal(file_id, journal)?;
        }
        inner.snapshot_version.insert(session_id.to_string(), version);
        drop(inner);
        self.metrics.snapshots.fetch_add(1, Ordering::Relaxed);
        self.metrics.last_snapshot_at.store(now_secs(), Ordering::Relaxed);
        Ok(())
    }

    /// Appends one applied operation to the journal. Returns whether a periodic snapshot is due.
    /// The journal is kept even without a volume: catch-up inside the process needs it.
    ///
    /// An undo/redo is journaled like any other operation: it took a version, and a client that
    /// reconnects after it — or a restart that replays the tail — must reach the same document.
    pub(crate) async fn record(&self, session_id: &str, actor: &str, op: JournalOp, applied: &Applied, now: u64) -> bool {
        let mut inner = self.inner.lock().await;
        let Some(file_id) = inner.sessions.get(session_id).map(|meta| meta.file_id) else { return false };
        let Some(journal) = inner.journal.get_mut(session_id) else { return false };
        if journal.last().is_some_and(|entry| applied.version <= entry.applied.version) {
            return false;
        }
        let entry = JournalEntry { at: now, actor: actor.to_string(), op, applied: applied.clone() };
        if let Some(store) = &self.store
            && let Err(e) = store.append_journal(file_id, &entry)
        {
            log::error!("journal append failed fileId={file_id}: {e}");
            return false;
        }
        journal.push(entry);
        if self.store.is_none() {
            return false;
        }
        let base = inner.snapshot_version.get(session_id).copied().unwrap_or(0);
        applied.version.saturating_sub(base) >= self.cfg.snapshot_ops
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
                    let mut inner = self.inner.lock().await;
                    if inner.gateway.hub_mut().open_restored(&scanned.session_id, loaded.session, loaded.keys, loaded.state) {
                        inner.sessions.insert(
                            scanned.session_id.clone(),
                            DocMeta {
                                file_id: scanned.file_id,
                                app: scanned.app.clone(),
                                owner: scanned.owner.clone(),
                                name: scanned.name.clone(),
                                created_at: scanned.created_at,
                                updated_at: now,
                            },
                        );
                        inner.journal.insert(scanned.session_id.clone(), loaded.journal);
                        inner.snapshot_version.insert(scanned.session_id.clone(), scanned.base_version);
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
            inner
                .sessions
                .keys()
                .filter(|id| {
                    let base = inner.snapshot_version.get(*id).copied().unwrap_or(0);
                    let head = inner.gateway.hub().version(id).unwrap_or(0);
                    head > base
                })
                .cloned()
                .collect()
        };
        let mut saved = 0;
        for id in ids {
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
        let inner = self.inner.lock().await;
        let meta = inner.sessions.get(session_id).ok_or_else(|| ApiError::not_found("no such session"))?;
        let participants = inner.gateway.hub().participants(session_id).unwrap_or_default();
        let version = inner.gateway.hub().version(session_id).unwrap_or(0);
        let epoch = inner.gateway.hub().epoch(session_id).unwrap_or(0);
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
    pub async fn commit(&self, session_id: &str, expected_version: Option<u64>) -> Result<CommitReply, ApiError> {
        let (name, version, bytes) = {
            let inner = self.inner.lock().await;
            let meta = inner.sessions.get(session_id).ok_or_else(|| ApiError::not_found("no such session"))?;
            let version = inner.gateway.hub().version(session_id).unwrap_or(0);
            if let Some(expected) = expected_version
                && expected != version
            {
                return Err(ApiError::new(StatusCode::CONFLICT, "conflict", format!("the session is at version {version}")));
            }
            let bytes =
                inner.gateway.hub().commit(session_id).map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", format!("{e:?}")))?;
            (meta.name.clone(), version, bytes)
        };
        self.metrics.commits.fetch_add(1, Ordering::Relaxed);
        Ok(CommitReply { name, bytes: B64.encode(&bytes), sha256: hex(&Sha256::digest(&bytes)), version, warnings: Vec::new() })
    }

    /// `DELETE /v1/sessions/{id}`: the cabinet closes the session and its lease.
    pub async fn close_session(&self, session_id: &str) -> Result<(), ApiError> {
        let (senders, file_id) = {
            let mut inner = self.inner.lock().await;
            let Some(meta) = inner.sessions.get(session_id) else {
                return Err(ApiError::not_found("no such session"));
            };
            let file_id = meta.file_id;
            (inner.drop_session(session_id), file_id)
        };
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
        let (files, mut gauges) = {
            let inner = self.inner.lock().await;
            let now = now_secs();
            let gauges = Gauges {
                sessions: inner.sessions.len() as u64,
                participants: inner.participants_total(),
                leases: inner.leases.live_leases(now).len() as u64,
                journal_ops: inner.journal.values().map(|entries| entries.len() as u64).sum(),
                ..Gauges::default()
            };
            let files: Vec<i64> = inner.sessions.values().map(|meta| meta.file_id).collect();
            (files, gauges)
        };
        if let Some(store) = &self.store {
            gauges.journal_bytes = files.iter().filter_map(|file_id| store.journal_size(*file_id).ok().flatten()).sum();
            gauges.base_bytes = files.iter().filter_map(|file_id| store.base_size(*file_id).ok().flatten()).sum();
        }
        self.metrics.render(&gauges, env!("CARGO_PKG_VERSION"))
    }
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
}
