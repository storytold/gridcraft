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
pub mod metrics;
mod ws;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use gridcraft_collab::{Gateway, ServerMessage};
use gridcraft_engine::{DocState, Session, Trust};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, mpsc};

pub use config::{Config, ConfigError};
pub use jwt::{JwtConfig, VerifyError};
pub use lease::Lease;

use crate::lease::Leases;
use crate::metrics::Metrics;

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

/// State behind one mutex: the protocol, the documents and the sockets. A command runs under this
/// lock, because a room applies to one engine session in process order (ADR-0003).
pub(crate) struct Inner {
    pub gateway: Gateway,
    pub sessions: BTreeMap<String, DocMeta>,
    pub leases: Leases,
    pub conns: BTreeMap<String, BTreeMap<u64, mpsc::Sender<ServerMessage>>>,
    pub actors: BTreeMap<String, BTreeMap<String, usize>>,
    pub next_conn: u64,
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
        }
    }

    /// Drops a document and returns the senders of its connections, so the caller can tell them
    /// the session is gone once the lock is released.
    pub fn drop_session(&mut self, session_id: &str) -> Vec<mpsc::Sender<ServerMessage>> {
        self.gateway.hub_mut().close(session_id);
        self.sessions.remove(session_id);
        self.actors.remove(session_id);
        self.leases.release(session_id);
        self.conns.remove(session_id).map(|senders| senders.into_values().collect()).unwrap_or_default()
    }

    pub fn participants_total(&self) -> u64 {
        self.actors.values().map(|actors| actors.len() as u64).sum()
    }
}

/// The server: configuration, the JWT key, one lock over the protocol state and metrics.
pub struct App {
    pub cfg: Config,
    pub jwt: JwtConfig,
    pub(crate) inner: Mutex<Inner>,
    pub metrics: Metrics,
}

impl App {
    pub fn new(cfg: Config, jwt: JwtConfig) -> App {
        App { cfg, jwt, inner: Mutex::new(Inner::new()), metrics: Metrics::default() }
    }

    /// Opens a session for a file (ADR-0007 §1): loads the workbook, takes the lease and adds a
    /// room. An existing live session of the same file is `409` unless `takeover`.
    pub async fn create_session(&self, request: CreateSession, now: u64) -> Result<CreatedSession, ApiError> {
        if request.app != self.cfg.app {
            return Err(ApiError::invalid(format!("this server serves {}", self.cfg.app)));
        }
        if request.file_id <= 0 {
            return Err(ApiError::invalid("fileId must be positive"));
        }
        let name = request.initial_name.clone().unwrap_or_else(|| format!("Book{}", request.file_id));
        let session = self.load(&name, request.initial_bytes.as_deref())?;
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
            inner.gateway.hub_mut().open(&session_id, session);
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
                    created_at: now,
                    updated_at: now,
                },
            );
            lease.expires_at
        };
        for sender in closed {
            let _ = sender.try_send(ServerMessage::Error {
                code: "session-closed".into(),
                message: "another session took over".into(),
                retryable: false,
            });
        }
        log::info!("session opened fileId={} session={}", request.file_id, session_id);
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
        let senders = {
            let mut inner = self.inner.lock().await;
            if !inner.sessions.contains_key(session_id) {
                return Err(ApiError::not_found("no such session"));
            }
            inner.drop_session(session_id)
        };
        for sender in senders {
            let _ =
                sender.try_send(ServerMessage::Error { code: "session-closed".into(), message: "the session was closed".into(), retryable: false });
        }
        log::info!("session closed session={session_id}");
        Ok(())
    }

    /// The `/metrics` body, with the gauges read under the state lock.
    pub async fn metrics_page(&self) -> String {
        let (sessions, participants, leases) = {
            let inner = self.inner.lock().await;
            let now = now_secs();
            (inner.sessions.len() as u64, inner.participants_total(), inner.leases.live_leases(now).len() as u64)
        };
        self.metrics.render(sessions, participants, leases, env!("CARGO_PKG_VERSION"))
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
