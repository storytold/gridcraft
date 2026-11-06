//! The WebSocket a session runs on (`docs/collab/rfc.md` §4).
//!
//! The first frame must be `hello` with the cabinet's signed token; until it is verified the
//! connection speaks for nobody. After it, every frame goes through
//! [`gridcraft_collab::Gateway`] under the state lock, so operations of one document keep the
//! order the room assigned them. A connection that falls behind loses frames — the client sees a
//! version gap and asks for a resync, which is the same path a reconnect takes.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use gridcraft_collab::{AuthError, ClientMessage, Connection, Expected, LeaseInfo, ServerMessage};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::{App, now_secs};

/// Frames buffered for one connection before it counts as slow.
const OUTBOX: usize = 256;

/// Runs one connection to the end. The session must already exist; the HTTP upgrade checked.
pub(crate) async fn run(app: Arc<App>, session_id: String, mut socket: WebSocket) {
    let Some((token, name, last_version)) = read_hello(&app, &mut socket).await else { return };
    let (connection, welcome) = match handshake(&app, &session_id, token, name).await {
        Ok(value) => value,
        Err(failure) => {
            fail(&mut socket, failure).await;
            return;
        }
    };
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(OUTBOX);
    let (conn_id, others, participants) = register(&app, &session_id, &connection.actor, tx).await;
    if send(&mut socket, &welcome).await.is_err() {
        leave(&app, &session_id, &connection.actor, Some(&conn_id)).await;
        return;
    }
    broadcast(&app.metrics, &others, &ServerMessage::Presence { participants });
    if last_version != 0 && last_version < version_of(&welcome) {
        let resync = ServerMessage::Error { code: "resync".into(), message: "reopen the document: the tail is not kept yet".into(), retryable: true };
        if send(&mut socket, &resync).await.is_err() {
            leave(&app, &session_id, &connection.actor, Some(&conn_id)).await;
            return;
        }
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(frame)) = incoming else { break };
                let Message::Text(text) = frame else { continue };
                if !pump(&app, &session_id, &connection, &conn_id, &mut socket, text.as_str()).await {
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
    leave(&app, &session_id, &connection.actor, Some(&conn_id)).await;
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

async fn read_hello(app: &App, socket: &mut WebSocket) -> Option<(String, String, u64)> {
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
        Ok(ClientMessage::Hello { token, name, last_version }) => Some((token, name, last_version)),
        _ => {
            fail(socket, Refused::policy(1008, "hello-required", "the first message must be hello")).await;
            None
        }
    }
}

async fn handshake(app: &App, session_id: &str, token: String, name: String) -> Result<(Connection, ServerMessage), Refused> {
    let claims = app.jwt.verify(&token, &app.cfg.audience).map_err(|e| {
        app.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        Refused::auth(e.code())
    })?;
    let now = now_secs();
    let mut inner = app.inner.lock().await;
    let meta = inner.sessions.get(session_id).cloned().ok_or_else(|| Refused::policy(4404, "session-unknown", "no such session"))?;
    let expected = Expected { aud: &app.cfg.audience, app: &app.cfg.app, file_id: meta.file_id, session_id };
    let lease = inner
        .leases
        .touch(meta.file_id, session_id, now, app.cfg.lease_ttl.as_secs())
        .ok_or_else(|| Refused::policy(4409, "lease", "the file is not held by this session"))?;
    let present = inner.gateway.hub().participants(session_id).map_err(|_| Refused::policy(4404, "session-unknown", "no such session"))?;
    let known = present.iter().any(|p| p.actor == claims.sub);
    if !known && present.len() >= app.cfg.max_participants {
        return Err(Refused::policy(4429, "too-many", "the session is full"));
    }
    let info = LeaseInfo { owner: lease.owner.clone(), expires_at: lease.expires_at };
    let (connection, welcome) = inner.gateway.connect(&claims, now, expected, &name, Some(info)).map_err(|e: AuthError| {
        app.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        Refused::auth(e.code())
    })?;
    Ok((connection, welcome))
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
    tx: mpsc::Sender<ServerMessage>,
) -> (u64, Vec<(u64, mpsc::Sender<ServerMessage>)>, Vec<gridcraft_collab::Participant>) {
    let mut inner = app.inner.lock().await;
    let id = inner.next_conn;
    inner.next_conn = inner.next_conn.saturating_add(1);
    let others = inner.conns.get(session_id).map(|senders| senders.iter().map(|(id, tx)| (*id, tx.clone())).collect::<Vec<_>>()).unwrap_or_default();
    inner.conns.entry(session_id.to_string()).or_default().insert(id, tx);
    let actors = inner.actors.entry(session_id.to_string()).or_default();
    *actors.entry(actor.to_string()).or_insert(0) += 1;
    let participants = inner.gateway.hub().participants(session_id).unwrap_or_default();
    app.metrics.connections.fetch_add(1, Ordering::Relaxed);
    (id, others, participants)
}

/// Handles one frame. `false` closes the connection.
async fn pump(app: &App, session_id: &str, connection: &Connection, conn_id: &u64, socket: &mut WebSocket, text: &str) -> bool {
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
    let now = now_secs();
    let operations = matches!(&message, ClientMessage::Op { .. });
    let (handled, others, lease_lost) = {
        let mut inner = app.inner.lock().await;
        let Some(file_id) = inner.sessions.get(session_id).map(|meta| meta.file_id) else {
            return false;
        };
        if inner.leases.touch(file_id, session_id, now, app.cfg.lease_ttl.as_secs()).is_none() {
            (Vec::new(), Vec::new(), true)
        } else {
            if operations {
                if let Some(meta) = inner.sessions.get_mut(session_id) {
                    meta.updated_at = now;
                }
                app.metrics.ops.fetch_add(1, Ordering::Relaxed);
            }
            let messages = inner.gateway.handle(connection, message);
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
            let others = inner
                .conns
                .get(session_id)
                .map(|senders| senders.iter().filter(|(id, _)| *id != conn_id).map(|(id, tx)| (*id, tx.clone())).collect::<Vec<_>>())
                .unwrap_or_default();
            (messages, others, false)
        }
    };
    if lease_lost {
        return false;
    }
    for message in handled {
        match &message {
            ServerMessage::Applied { .. } => {
                broadcast(&app.metrics, &others, &message);
                if send(socket, &message).await.is_err() {
                    return false;
                }
            }
            ServerMessage::Presence { .. } => {
                // Presence is state of a connection, not of the document: the sender already
                // knows its own cursor.
                broadcast(&app.metrics, &others, &message);
            }
            _ => {
                if send(socket, &message).await.is_err() {
                    return false;
                }
            }
        }
    }
    true
}

async fn leave(app: &App, session_id: &str, actor: &str, conn_id: Option<&u64>) {
    let (participants, others) = {
        let mut inner = app.inner.lock().await;
        if let Some(id) = conn_id {
            let empty = match inner.conns.get_mut(session_id) {
                Some(senders) => {
                    senders.remove(id);
                    senders.is_empty()
                }
                None => false,
            };
            if empty {
                inner.conns.remove(session_id);
            }
        }
        let mut participants = None;
        let mut removed = false;
        if let Some(actors) = inner.actors.get_mut(session_id) {
            if let Some(count) = actors.get_mut(actor) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    actors.remove(actor);
                    removed = true;
                }
            }
            if actors.is_empty() {
                inner.actors.remove(session_id);
            }
        }
        if removed && let Ok(left) = inner.gateway.hub_mut().leave(session_id, actor) {
            participants = Some(left);
        }
        let others = inner.conns.get(session_id).map(|senders| senders.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
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
