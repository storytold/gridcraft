//! The WebSocket a session runs on (`docs/collab/rfc.md` §4).
//!
//! The first frame must be `hello` with the cabinet's signed token; until it is verified the
//! connection speaks for nobody. After it, every frame goes through
//! [`gridcraft_collab::Gateway`] under the lock of its document (ADR-0007 §3), so operations of one
//! document keep the order the room assigned them and a busy book does not hold up the others. A
//! connection that falls behind loses frames — the client sees a version gap and asks for a
//! resync, which is the same path a reconnect takes.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use base64::Engine;
use gridcraft_collab::{AuthError, ClientMessage, Connection, Expected, IMAGE_PART, LeaseInfo, ServerMessage, SnapshotMode, split_applied};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::{App, B64, now_secs};

/// Frames buffered for one connection before it counts as slow.
const OUTBOX: usize = 256;

/// Runs one connection to the end. The session must already exist; the HTTP upgrade checked.
pub(crate) async fn run(app: Arc<App>, session_id: String, mut socket: WebSocket) {
    let Some((token, name, last_version, window_id)) = read_hello(&app, &mut socket).await else { return };
    let (connection, welcome) = match handshake(&app, &session_id, token, name, window_id).await {
        Ok(value) => value,
        Err(failure) => {
            fail(&mut socket, failure).await;
            return;
        }
    };
    if last_version > 0 {
        app.metrics.resumes.fetch_add(1, Ordering::Relaxed);
    }
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(OUTBOX);
    let Some((conn_id, others, participants)) = register(&app, &session_id, &connection.actor, &connection.window_id, tx.clone()).await else {
        close(&mut socket, "session-unknown").await;
        return;
    };
    if send(&mut socket, &welcome).await.is_err() {
        leave(&app, &session_id, &connection.actor, &connection.window_id, Some(&conn_id)).await;
        return;
    }
    broadcast(&app.metrics, &others, &ServerMessage::Presence { participants });
    // A replica that is behind takes the catch-up. One that is not — but joins a room whose
    // document came from the cabinet at version 0 — takes it too: the journal has nothing to
    // replay, and without the workbook the two replicas would keep different documents while
    // both claiming version 0 (CodeRabbit 5846575558, `collab_ui.rs:36`).
    let behind = last_version < version_of(&welcome) || (last_version == 0 && seeded(&app, &session_id).await);
    if behind && !resync(&app, &session_id, &conn_id, &tx, &mut socket, last_version).await {
        leave(&app, &session_id, &connection.actor, &connection.window_id, Some(&conn_id)).await;
        return;
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(frame)) = incoming else { break };
                let Message::Text(text) = frame else { continue };
                if !pump(&app, &session_id, &connection, &conn_id, &tx, &mut socket, text.as_str()).await {
                    break;
                }
            }
            outgoing = rx.recv() => {
                let Some(message) = outgoing else { break };
                if send(&mut socket, &message).await.is_err() {
                    break;
                }
            }
        }
    }
    close(&mut socket, "connection-closed").await;
    leave(&app, &session_id, &connection.actor, &connection.window_id, Some(&conn_id)).await;
}

/// What stops the handshake, and how the socket says it.
struct Refused {
    ws_code: u16,
    code: &'static str,
    message: &'static str,
}

impl Refused {
    fn auth(code: &'static str) -> Refused {
        Refused { ws_code: 4401, code, message: "the session token is not accepted" }
    }

    fn policy(ws_code: u16, code: &'static str, message: &'static str) -> Refused {
        Refused { ws_code, code, message }
    }
}

async fn read_hello(app: &App, socket: &mut WebSocket) -> Option<(String, String, u64, String)> {
    let frame = match timeout(app.cfg.hello_timeout, socket.recv()).await {
        Ok(Some(Ok(frame))) => frame,
        Ok(_) => {
            fail(socket, Refused::policy(1008, "hello-required", "the first message must be hello")).await;
            return None;
        }
        Err(_) => {
            fail(socket, Refused::policy(1008, "hello-timeout", "no hello within the handshake timeout")).await;
            return None;
        }
    };
    let Message::Text(text) = frame else {
        fail(socket, Refused::policy(1008, "hello-required", "the first message must be hello")).await;
        return None;
    };
    match serde_json::from_str::<ClientMessage>(text.as_str()) {
        Ok(ClientMessage::Hello { token, name, last_version, window_id }) => Some((token, name, last_version, window_id)),
        _ => {
            fail(socket, Refused::policy(1008, "hello-required", "the first message must be hello")).await;
            None
        }
    }
}

async fn handshake(app: &App, session_id: &str, token: String, name: String, window_id: String) -> Result<(Connection, ServerMessage), Refused> {
    let claims = app.jwt.verify(&token, &app.cfg.audience).map_err(|e| {
        app.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        Refused::auth(e.code())
    })?;
    let now = now_secs();
    let Some(doc) = app.doc(session_id).await else { return Err(Refused::policy(4404, "session-unknown", "no such session")) };
    let (file_id, owner) = {
        let doc = doc.lock().await;
        (doc.meta.file_id, doc.meta.owner.clone())
    };
    let expected = Expected { aud: &app.cfg.audience, app: &app.cfg.app, file_id, session_id };
    let ttl = app.cfg.lease_ttl.as_secs();
    let lease = {
        let mut inner = app.inner.lock().await;
        match inner.leases.touch(file_id, session_id, now, ttl) {
            Some(lease) => lease,
            // A session restored from the volume, or one whose lease idled out, takes the lease
            // back on its first connection. A live lease of another session still refuses.
            None => inner
                .leases
                .acquire(file_id, session_id, &owner, now, ttl, false)
                .map_err(|_| Refused::policy(4409, "lease", "the file is held by another session"))?,
        }
    };
    let mut doc = doc.lock().await;
    let present = doc.gateway.hub().participants(session_id).map_err(|_| Refused::policy(4404, "session-unknown", "no such session"))?;
    let known = present.iter().any(|p| p.actor == claims.sub);
    // The limit counts people, not windows: a second or third window of somebody already in the
    // session is allowed (RFC §13.6), a new person is not while the session is full.
    let people = present.iter().map(|p| p.actor.as_str()).collect::<std::collections::BTreeSet<_>>().len();
    if !known && people >= app.cfg.max_participants {
        return Err(Refused::policy(4429, "too-many", "the session is full"));
    }
    let info = LeaseInfo { owner: lease.owner.clone(), expires_at: lease.expires_at };
    let (connection, welcome) = doc.gateway.connect(&claims, now, expected, &name, &window_id, Some(info)).map_err(|e: AuthError| {
        app.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        Refused::auth(e.code())
    })?;
    Ok((connection, welcome))
}

/// Bytes of one snapshot chunk before base64. The encoded frame stays under the 1 MiB frame
/// limit the transport sets (RFC §4.1 says chunks of 1 MiB; base64 costs a third).
const SNAPSHOT_CHUNK: usize = 768 * 1024;

/// Catches a connection up to the head: the journal tail when it covers `from`, the whole
/// workbook otherwise, then every operation that landed while the catch-up was running.
///
/// The connection is taken out of the live broadcast for the duration — otherwise a delta could
/// overtake the snapshot — and put back only after the last missing operation has been sent.
async fn resync(app: &Arc<App>, session_id: &str, conn_id: &u64, tx: &mpsc::Sender<ServerMessage>, socket: &mut WebSocket, from: u64) -> bool {
    let (head, base, seeded) = unregister(app, session_id, conn_id).await;
    let mut sent = from;
    let mut used_snapshot = false;
    // A replica that holds no document of its own joins at version 0; when the room's document
    // came from the cabinet and no operation was applied yet, the journal holds nothing to send,
    // so the workbook itself is the answer. Without it both sides claim version 0 and keep
    // different documents (CodeRabbit 5846575558, `collab_ui.rs:36`).
    let mut want_base = from > head || from < base || (from == 0 && head == 0 && seeded);
    loop {
        if want_base {
            used_snapshot = true;
            app.metrics.resync_snapshots.fetch_add(1, Ordering::Relaxed);
            match send_base(app, session_id, socket).await {
                Ok(version) => sent = version,
                Err(BaseError::Refused(why)) => return reject_resync(app, session_id, conn_id, tx, socket, why).await,
                Err(BaseError::Gone) => return false,
            }
            want_base = false;
        }
        match catch_up(app, session_id, conn_id, tx, sent).await {
            CatchUp::Tail(entries) => {
                // A snapshot trimmed the journal past `sent` between two iterations: the tail no
                // longer covers what this connection missed, so the workbook is the answer.
                if entries.first().is_some_and(|applied| applied.version > sent.saturating_add(1)) {
                    want_base = true;
                    continue;
                }
                for applied in &entries {
                    for frame in frames(&ServerMessage::Applied { applied: applied.clone() }) {
                        if send(socket, &frame).await.is_err() {
                            return false;
                        }
                    }
                    sent = applied.version;
                }
            }
            CatchUp::Registered { version, epoch } => {
                if !used_snapshot {
                    app.metrics.catchups.fetch_add(1, Ordering::Relaxed);
                }
                return send(socket, &ServerMessage::Resynced { version, epoch }).await.is_ok();
            }
        }
    }
}

/// What one critical section over the document decided for a connection that is catching up.
enum CatchUp {
    /// Operations the connection has not seen. It stays out of the live broadcast while they go.
    Tail(Vec<gridcraft_collab::Applied>),
    /// Nothing is missing: the connection is back in the broadcast, and `version` is its head.
    Registered { version: u64, epoch: u64 },
}

/// One critical section over the document: the operations the connection still needs, or — when
/// there is none — the head, with the connection put back into the live broadcast.
///
/// Reading the tail and re-registering under one lock is what makes the answer sound. Between a
/// separate read and a separate insert an operation of another connection can be applied,
/// broadcast to a set that does not contain this one and journaled; the client is then told the
/// new head in `resynced` without ever receiving the delta, and it has no version gap to notice
/// (CodeRabbit 5846575558, `ws.rs:202`).
async fn catch_up(app: &App, session_id: &str, conn_id: &u64, tx: &mpsc::Sender<ServerMessage>, sent: u64) -> CatchUp {
    let Some(doc) = app.doc(session_id).await else { return CatchUp::Registered { version: 0, epoch: 0 } };
    let mut doc = doc.lock().await;
    let entries = doc.journal_after(sent);
    if !entries.is_empty() {
        return CatchUp::Tail(entries);
    }
    doc.conns.insert(*conn_id, tx.clone());
    CatchUp::Registered { version: doc.gateway.hub().version(session_id).unwrap_or(0), epoch: doc.gateway.hub().epoch(session_id).unwrap_or(0) }
}

/// Why the workbook could not be sent: the connection is gone, or it can be told why not.
enum BaseError {
    Refused(&'static str),
    Gone,
}

/// Sends the authoritative workbook of the session at its head, chunked.
async fn send_base(app: &Arc<App>, session_id: &str, socket: &mut WebSocket) -> Result<u64, BaseError> {
    let Some((version, epoch, workbook)) = snapshot_source(app, session_id).await else { return Err(BaseError::Gone) };
    let written = tokio::task::spawn_blocking(move || gridcraft_xlsx::write_xlsx(&workbook)).await;
    let bytes = match written {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            log::error!("snapshot for resync failed session={session_id}: {e}");
            return Err(BaseError::Refused("the snapshot cannot be built"));
        }
        Err(e) => {
            log::error!("snapshot task failed session={session_id}: {e}");
            return Err(BaseError::Refused("the snapshot cannot be built"));
        }
    };
    if !send_snapshot(socket, version, epoch, &bytes).await {
        return Err(BaseError::Gone);
    }
    Ok(version)
}

async fn unregister(app: &App, session_id: &str, conn_id: &u64) -> (u64, u64, bool) {
    let Some(doc) = app.doc(session_id).await else { return (0, 0, false) };
    let mut doc = doc.lock().await;
    doc.conns.remove(conn_id);
    let head = doc.gateway.hub().version(session_id).unwrap_or(0);
    (head, doc.snapshot_version, doc.seeded)
}

async fn snapshot_source(app: &App, session_id: &str) -> Option<(u64, u64, std::sync::Arc<gridcraft_model::Workbook>)> {
    let doc = app.doc(session_id).await?;
    let doc = doc.lock().await;
    let version = doc.gateway.hub().version(session_id)?;
    let epoch = doc.gateway.hub().epoch(session_id).unwrap_or(0);
    let workbook = doc.gateway.hub().workbook(session_id)?;
    Some((version, epoch, workbook))
}

async fn reject_resync(
    app: &App,
    session_id: &str,
    conn_id: &u64,
    tx: &mpsc::Sender<ServerMessage>,
    socket: &mut WebSocket,
    why: &'static str,
) -> bool {
    let _ = catch_up(app, session_id, conn_id, tx, u64::MAX).await;
    let error = ServerMessage::Error { code: "internal".into(), message: why.into(), retryable: true };
    send(socket, &error).await.is_ok()
}

async fn send_snapshot(socket: &mut WebSocket, version: u64, epoch: u64, bytes: &[u8]) -> bool {
    let total = bytes.len().max(1).div_ceil(SNAPSHOT_CHUNK) as u32;
    for seq in 0..total as usize {
        let start = (seq * SNAPSHOT_CHUNK).min(bytes.len());
        let end = ((seq + 1) * SNAPSHOT_CHUNK).min(bytes.len());
        let chunk = &bytes[start..end];
        let message = ServerMessage::Snapshot { mode: SnapshotMode::Base, version, epoch, total, seq: seq as u32, bytes: B64.encode(chunk) };
        if send(socket, &message).await.is_err() {
            return false;
        }
    }
    true
}

/// Whether the room's document came from outside (the cabinet's bytes or the volume), so a
/// replica that joins at version 0 has to adopt it.
async fn seeded(app: &App, session_id: &str) -> bool {
    let Some(doc) = app.doc(session_id).await else { return false };
    doc.lock().await.seeded
}

/// The version the welcome message reports, without re-encoding the whole message.
fn version_of(welcome: &ServerMessage) -> u64 {
    match welcome {
        ServerMessage::Welcome { version, .. } => *version,
        _ => 0,
    }
}

async fn register(
    app: &App,
    session_id: &str,
    actor: &str,
    window_id: &str,
    tx: mpsc::Sender<ServerMessage>,
) -> Option<(u64, Vec<(u64, mpsc::Sender<ServerMessage>)>, Vec<gridcraft_collab::Participant>)> {
    let doc = app.doc(session_id).await?;
    let id = app.next_conn.fetch_add(1, Ordering::Relaxed);
    let mut doc = doc.lock().await;
    let others = doc.conns.iter().map(|(id, tx)| (*id, tx.clone())).collect::<Vec<_>>();
    doc.conns.insert(id, tx);
    *doc.actors.entry((actor.to_string(), window_id.to_string())).or_insert(0) += 1;
    let participants = doc.gateway.hub().participants(session_id).unwrap_or_default();
    app.metrics.connections.fetch_add(1, Ordering::Relaxed);
    Some((id, others, participants))
}

/// Handles one frame. `false` closes the connection.
async fn pump(
    app: &Arc<App>,
    session_id: &str,
    connection: &Connection,
    conn_id: &u64,
    tx: &mpsc::Sender<ServerMessage>,
    socket: &mut WebSocket,
    text: &str,
) -> bool {
    let started = tokio::time::Instant::now();
    let Ok(message) = serde_json::from_str::<ClientMessage>(text) else {
        return send(socket, &ServerMessage::Error { code: "invalid".into(), message: "a frame is one JSON message".into(), retryable: false })
            .await
            .is_ok();
    };
    if matches!(&message, ClientMessage::Hello { .. }) {
        let error = ServerMessage::Error { code: "already-joined".into(), message: "the handshake was already done".into(), retryable: false };
        return send(socket, &error).await.is_ok();
    }
    if matches!(&message, ClientMessage::Leave) {
        return false;
    }
    if let ClientMessage::Resync { from } = message {
        return resync(app, session_id, conn_id, tx, socket, from).await;
    }
    // The client holds no usable document — a refused preview the journal tail cannot cover — and
    // asks for the workbook itself: the same catch-up from above the head, which sends the base.
    if matches!(&message, ClientMessage::Snapshot) {
        return resync(app, session_id, conn_id, tx, socket, u64::MAX).await;
    }
    let now = now_secs();
    let journal_op = match &message {
        ClientMessage::Op { op } => Some(crate::JournalOp::Op { op: op.clone() }),
        ClientMessage::Undo { .. } => Some(crate::JournalOp::Undo),
        ClientMessage::Redo { .. } => Some(crate::JournalOp::Redo),
        _ => None,
    };
    // An undo/redo is an operation of the document like any other: it takes a version and the
    // session counts as changed, even though it carries no command of its own.
    let writes = journal_op.is_some();
    let (doc, file_id) = {
        let inner = app.inner.lock().await;
        match inner.docs.get(session_id) {
            Some(entry) => (entry.doc.clone(), entry.file_id),
            None => return false,
        }
    };
    let lease_held = {
        let mut inner = app.inner.lock().await;
        inner.leases.touch(file_id, session_id, now, app.cfg.lease_ttl.as_secs()).is_some()
    };
    if !lease_held {
        return false;
    }
    let (out, due) = {
        let mut doc = doc.lock().await;
        if writes {
            doc.meta.updated_at = now;
            app.metrics.ops.fetch_add(1, Ordering::Relaxed);
        }
        let messages = doc.gateway.handle(connection, message);
        for message in &messages {
            match message {
                ServerMessage::Applied { .. } => {
                    app.metrics.applied.fetch_add(1, Ordering::Relaxed);
                }
                ServerMessage::Rejected { rejected } => {
                    app.metrics.rejection(rejected.code.code());
                }
                _ => {}
            }
        }
        let others = doc.conns.iter().filter(|(id, _)| *id != conn_id).map(|(id, tx)| (*id, tx.clone())).collect::<Vec<_>>();
        // Split and queue for the peers while the document lock is still held: the parts of one
        // applied must not be separated by another operation (ADR-0013 §2), and two pumps of the
        // same document run on different worker threads. `broadcast` is synchronous, so no await
        // happens here. Another document's pump waits on its own lock, not this one.
        let out: Vec<ServerMessage> = messages.iter().flat_map(frames).collect();
        for frame in &out {
            match frame {
                ServerMessage::Applied { .. } => broadcast(&app.metrics, &others, frame),
                ServerMessage::Presence { .. } => {
                    broadcast(&app.metrics, &others, frame);
                    // A presence answer carries every window of every person, this
                    // connection's own row included (RFC §13.6). It goes to the sender through
                    // the same queue as the peers' frames, so every frame of the document is
                    // applied in the order the room produced it: a frame computed before this
                    // window's cursor cannot land after it and blank the row. Presence is still
                    // state of a window, never of the document, and dropping one is harmless.
                    let _ = tx.try_send(frame.clone());
                }
                _ => {}
            }
        }
        // The journal entry is written under the same lock as the operation it records, so two
        // operations of one document cannot reach the journal out of order.
        let due = match (
            &journal_op,
            messages.iter().find_map(|message| match message {
                ServerMessage::Applied { applied } => Some(applied),
                _ => None,
            }),
        ) {
            (Some(op), Some(applied)) => app.record_locked(&mut doc, &connection.actor, op.clone(), applied, now),
            _ => false,
        };
        (out, due)
    };
    if due {
        let app = app.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = app.snapshot(&session_id).await {
                log::error!("periodic snapshot failed session={session_id}: {e}");
            }
        });
    }
    let mut replied = false;
    for frame in &out {
        match frame {
            ServerMessage::Applied { .. } => {
                if send(socket, frame).await.is_err() {
                    return false;
                }
                replied = true;
            }
            ServerMessage::Presence { .. } => {
                // Queued for this connection under the room lock, in order with the peers' frames
                // (see above): nothing to write here.
            }
            _ => {
                if send(socket, frame).await.is_err() {
                    return false;
                }
            }
        }
    }
    if replied {
        app.metrics.op_latency.observe(started.elapsed());
    }
    true
}

async fn leave(app: &App, session_id: &str, actor: &str, window_id: &str, conn_id: Option<&u64>) {
    let (participants, others) = {
        let Some(doc) = app.doc(session_id).await else { return };
        let mut doc = doc.lock().await;
        if let Some(id) = conn_id {
            doc.conns.remove(id);
        }
        let mut participants = None;
        let mut removed = false;
        let key = (actor.to_string(), window_id.to_string());
        if let Some(count) = doc.actors.get_mut(&key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                doc.actors.remove(&key);
                removed = true;
            }
        }
        if removed && let Ok(left) = doc.gateway.hub_mut().leave(session_id, actor, window_id) {
            participants = Some(left);
        }
        let others = doc.conns.values().cloned().collect::<Vec<_>>();
        (participants, others)
    };
    app.metrics.connections.fetch_sub(1, Ordering::Relaxed);
    if let Some(participants) = participants {
        let message = ServerMessage::Presence { participants };
        for tx in others {
            let _ = tx.try_send(message.clone());
        }
    }
}

/// The frames one handled message becomes: an `applied` whose sheet image does not fit one frame
/// is written as its parts, back to back (ADR-0013); every other message is one frame.
fn frames(message: &ServerMessage) -> Vec<ServerMessage> {
    match message {
        ServerMessage::Applied { applied } if applied.images.is_some() => {
            split_applied(applied.clone(), IMAGE_PART).into_iter().map(|applied| ServerMessage::Applied { applied }).collect()
        }
        _ => vec![message.clone()],
    }
}

async fn send(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(message) else { return Err(()) };
    socket.send(Message::Text(text.into())).await.map_err(|_| ())
}

async fn fail(socket: &mut WebSocket, refused: Refused) {
    let error = ServerMessage::Error { code: refused.code.into(), message: refused.message.into(), retryable: false };
    let _ = send(socket, &error).await;
    let frame = CloseFrame { code: refused.ws_code, reason: Utf8Bytes::from(refused.code) };
    let _ = socket.send(Message::Close(Some(frame))).await;
}

async fn close(socket: &mut WebSocket, reason: &str) {
    let frame = CloseFrame { code: 1008, reason: Utf8Bytes::from(reason) };
    let _ = socket.send(Message::Close(Some(frame))).await;
}

fn broadcast(metrics: &crate::metrics::Metrics, others: &[(u64, mpsc::Sender<ServerMessage>)], message: &ServerMessage) {
    for (_, tx) in others {
        if tx.try_send(message.clone()).is_err() {
            metrics.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
