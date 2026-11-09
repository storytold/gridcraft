//! The WebSocket client of `docs/collab/rfc.md`: one connection, its pending operations and the
//! events that keep a local document in step.
//!
//! The transport owns the socket and the reconnect loop. A caller never touches a frame: it sends
//! [`ClientOp`]s and reads [`Event`]s. The connection resends unacknowledged operations with the
//! same `opId` after a reconnect, so the server's dedup window makes the retry idempotent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use gridcraft_collab::{ClientMessage, ClientOp, Presence, ServerMessage};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long the first `welcome` may take before the connection counts as failed.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Backoff ceiling between reconnects.
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const FIRST_BACKOFF: Duration = Duration::from_millis(500);

/// What a caller needs to start a session.
#[derive(Clone, Debug)]
pub struct Config {
    /// `ws://…/v1/sessions/{id}/ws`.
    pub url: String,
    /// The cabinet's signed token; never logged.
    pub token: String,
    pub name: String,
    /// The version the local document is at; the server catches the connection up from here.
    pub last_version: u64,
    /// `None` closes on the first transport failure; `Some` retries with backoff.
    pub reconnect: Option<Duration>,
}

use crate::link::{Event, Link, State, Welcome};

/// Why a session could not be opened.
#[derive(Debug)]
pub enum ConnectError {
    Transport(String),
    Refused(String),
    NoWelcome,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Transport(e) => write!(f, "transport: {e}"),
            ConnectError::Refused(code) => write!(f, "refused: {code}"),
            ConnectError::NoWelcome => f.write_str("the server did not send a welcome"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// One live collaboration session.
pub struct Client {
    ops: mpsc::UnboundedSender<ClientMessage>,
    events: mpsc::UnboundedReceiver<Event>,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}

impl Client {
    /// Connects, authenticates and waits for `welcome`. Frames that arrive before the caller polls
    /// (the catch-up) are queued as events.
    pub async fn connect(cfg: Config) -> Result<(Client, Welcome), ConnectError> {
        let mut socket = open(&cfg).await.map_err(ConnectError::Transport)?;
        let hello = ClientMessage::Hello { token: cfg.token.clone(), name: cfg.name.clone(), last_version: cfg.last_version };
        send(&mut socket, &hello).await?;
        let welcome = loop {
            let frame = tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.next())
                .await
                .map_err(|_| ConnectError::Transport("the handshake timed out".into()))?
                .ok_or(ConnectError::NoWelcome)?
                .map_err(|e| ConnectError::Transport(e.to_string()))?;
            let Some(text) = text(frame) else { continue };
            match serde_json::from_str::<ServerMessage>(&text) {
                Ok(ServerMessage::Welcome { session_id, actor_id, role, version, epoch, sheets, participants, .. }) => {
                    break Welcome { session_id, actor_id, role, version, epoch, sheets, participants };
                }
                Ok(ServerMessage::Error { code, .. }) => return Err(ConnectError::Refused(code)),
                _ => continue,
            }
        };
        let state = State {
            session_id: welcome.session_id.clone(),
            actor: welcome.actor_id.clone(),
            role: Some(welcome.role),
            version: welcome.version,
            epoch: welcome.epoch,
            sheets: welcome.sheets.clone(),
            participants: welcome.participants.clone(),
            connected: true,
        };
        let shared = Arc::new(Mutex::new(state));
        let (ops, ops_rx) = mpsc::unbounded_channel();
        let (events_tx, events) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(socket, cfg, shared.clone(), ops_rx, events_tx));
        Ok((Client { ops, events, state: shared, task }, welcome))
    }

    pub fn state(&self) -> State {
        self.state.lock().map(|state| state.clone()).unwrap_or_default()
    }

    /// Queues an operation. The connection sends it and resends it after a reconnect until the
    /// server answers with `applied` or `rejected`.
    pub fn send(&self, op: ClientOp) -> bool {
        self.ops.send(ClientMessage::Op { op }).is_ok()
    }

    /// Sends any message of the protocol (presence, resync). `hello` is the transport's own.
    pub fn send_message(&self, message: ClientMessage) -> bool {
        self.ops.send(message).is_ok()
    }

    pub fn send_presence(&self, presence: gridcraft_collab::Presence) -> bool {
        self.ops.send(ClientMessage::Presence { presence: Some(presence) }).is_ok()
    }

    /// The next event, without waiting.
    pub fn poll(&mut self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Closes the socket and stops the task.
    pub async fn close(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

impl Link for Client {
    fn state(&self) -> State {
        Client::state(self)
    }

    fn send(&self, op: ClientOp) -> bool {
        Client::send(self, op)
    }

    fn send_message(&self, message: ClientMessage) -> bool {
        Client::send_message(self, message)
    }

    fn send_presence(&self, presence: Presence) -> bool {
        Client::send_presence(self, presence)
    }

    fn poll(&mut self) -> Option<Event> {
        Client::poll(self)
    }
}

/// The session loop: the first socket, then a reconnect for as long as the caller keeps the
/// client.
async fn run(
    mut socket: Socket,
    cfg: Config,
    shared: Arc<Mutex<State>>,
    mut ops: mpsc::UnboundedReceiver<ClientMessage>,
    events: mpsc::UnboundedSender<Event>,
) {
    let mut pending: Vec<ClientOp> = Vec::new();
    let mut backoff = FIRST_BACKOFF;
    loop {
        match session_once(&mut socket, &shared, &mut ops, &events, &mut pending).await {
            Outcome::Closed => {
                let _ = events.send(Event::Closed);
                return;
            }
            Outcome::Reconnect(reason) => {
                let _ = events.send(Event::Disconnected { reason });
                if cfg.reconnect.is_none() {
                    return;
                }
                if let Ok(mut state) = shared.lock() {
                    state.connected = false;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                let Ok(mut fresh) = open(&cfg).await else { continue };
                let hello = {
                    let state = shared.lock().map(|state| state.clone()).unwrap_or_default();
                    ClientMessage::Hello { token: cfg.token.clone(), name: cfg.name.clone(), last_version: state.version }
                };
                if send(&mut fresh, &hello).await.is_err() {
                    continue;
                }
                for op in &pending {
                    if send(&mut fresh, &ClientMessage::Op { op: op.clone() }).await.is_err() {
                        break;
                    }
                }
                socket = fresh;
            }
        }
    }
}

enum Outcome {
    Closed,
    Reconnect(String),
}

async fn open(cfg: &Config) -> Result<Socket, String> {
    let url = cfg.url.clone().into_client_request().map_err(|e| e.to_string())?;
    let (socket, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| e.to_string())?;
    Ok(socket)
}

async fn session_once(
    socket: &mut Socket,
    shared: &Arc<Mutex<State>>,
    ops: &mut mpsc::UnboundedReceiver<ClientMessage>,
    events: &mpsc::UnboundedSender<Event>,
    pending: &mut Vec<ClientOp>,
) -> Outcome {
    let mut snapshot: Option<Snapshot> = None;
    loop {
        tokio::select! {
            incoming = socket.next() => {
                let Some(Ok(frame)) = incoming else { return Outcome::Reconnect("the socket closed".into()) };
                let Some(text) = text(frame) else { continue };
                let Ok(message) = serde_json::from_str::<ServerMessage>(&text) else { continue };
                match handle(message, shared, events, &mut snapshot) {
                    Handled::Continue => {}
                    Handled::Closed => return Outcome::Closed,
                }
            }
            message = ops.recv() => {
                let Some(message) = message else { return Outcome::Closed };
                if let ClientMessage::Op { op } = &message
                    && !pending.iter().any(|held| held.op_id == op.op_id)
                {
                    pending.push(op.clone());
                }
                if send(socket, &message).await.is_err() {
                    return Outcome::Reconnect("send failed".into());
                }
            }
        }
    }
}

enum Handled {
    Continue,
    Closed,
}

/// Where a chunked snapshot is being assembled.
struct Snapshot {
    version: u64,
    epoch: u64,
    chunks: Vec<Option<Vec<u8>>>,
}

fn handle(message: ServerMessage, shared: &Arc<Mutex<State>>, events: &mpsc::UnboundedSender<Event>, snapshot: &mut Option<Snapshot>) -> Handled {
    match message {
        ServerMessage::Welcome { session_id, actor_id, role, version, epoch, sheets, participants, .. } => {
            if let Ok(mut state) = shared.lock() {
                state.session_id = session_id;
                state.actor = actor_id;
                state.role = Some(role);
                state.version = version;
                state.epoch = epoch;
                state.sheets = sheets;
                state.participants = participants;
                state.connected = true;
            }
            let _ = events.send(Event::Reconnected { version });
        }
        ServerMessage::Applied { applied } => {
            if let Ok(mut state) = shared.lock() {
                crate::link::note_applied(&mut state, &applied);
            }
            let _ = events.send(Event::Applied(applied));
        }
        ServerMessage::Rejected { rejected } => {
            let _ = events.send(Event::Rejected(rejected));
        }
        ServerMessage::Presence { participants } => {
            if let Ok(mut state) = shared.lock() {
                state.participants = participants.clone();
            }
            let _ = events.send(Event::Presence(participants));
        }
        ServerMessage::Resynced { version, epoch } => {
            if let Ok(mut state) = shared.lock() {
                state.version = version;
                state.epoch = epoch;
            }
            let _ = events.send(Event::Resynced { version, epoch });
        }
        ServerMessage::Snapshot { version, epoch, total, seq, bytes, mode: _ } => {
            let Ok(chunk) = base64::engine::general_purpose::STANDARD.decode(bytes) else { return Handled::Continue };
            let buffer = snapshot.get_or_insert_with(|| Snapshot { version, epoch, chunks: vec![None; total as usize] });
            if (seq as usize) < buffer.chunks.len() {
                buffer.chunks[seq as usize] = Some(chunk);
            }
            if buffer.chunks.iter().all(Option::is_some) {
                let mut bytes = Vec::new();
                for chunk in buffer.chunks.iter().flatten() {
                    bytes.extend_from_slice(chunk);
                }
                let (version, epoch) = (buffer.version, buffer.epoch);
                *snapshot = None;
                if let Ok(mut state) = shared.lock() {
                    state.version = version;
                    state.epoch = epoch;
                }
                let _ = events.send(Event::Snapshot { version, epoch, bytes });
            }
        }
        ServerMessage::Pong { .. } => {}
        ServerMessage::Error { code, message, retryable } => {
            if code == "resync" {
                return Handled::Continue;
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
            let _ = events.send(Event::Error { code, message, retryable });
            if fatal {
                return Handled::Closed;
            }
        }
    }
    Handled::Continue
}

async fn send(socket: &mut Socket, message: &ClientMessage) -> Result<(), ConnectError> {
    let text = serde_json::to_string(message).map_err(|e| ConnectError::Transport(e.to_string()))?;
    socket.send(Message::text(text)).await.map_err(|e| ConnectError::Transport(e.to_string()))
}

fn text(frame: Message) -> Option<String> {
    match frame {
        Message::Text(text) => Some(text.to_string()),
        _ => None,
    }
}
