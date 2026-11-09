//! Shared helpers for the socket tests: a server on an ephemeral port, a cabinet token, HTTP by
//! hand (no client library) and tungstenite frames.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use futures_util::SinkExt;
use gridcraft_collab::{Claims, Role};
use gridcraft_collab_server::http::router;
use gridcraft_collab_server::store::Store;
use gridcraft_collab_server::{App, Config, JwtConfig};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub const SECRET: &[u8] = b"test-secret";
pub const SERVICE: &str = "service-token";

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct Server {
    pub addr: SocketAddr,
    pub app: Arc<App>,
    pub restored: usize,
    task: tokio::task::JoinHandle<()>,
}

fn config() -> Config {
    Config { addr: "127.0.0.1:0".parse().unwrap(), service_token: Some(SERVICE.into()), lease_ttl: Duration::from_secs(60), ..Config::default() }
}

fn jwt() -> JwtConfig {
    JwtConfig::hs256(SECRET).unwrap()
}

async fn launch(app: Arc<App>) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router(served)).await;
    });
    Server { addr, app, restored: 0, task }
}

/// A server with no volume.
pub async fn start() -> Server {
    launch(Arc::new(App::new(config(), jwt()))).await
}

/// A server over `root`, with the sessions the volume kept already restored.
pub async fn start_stored(root: &Path) -> Server {
    let app = Arc::new(App::with_store(config(), jwt(), Store::open(root).unwrap()));
    let restored = app.restore().await;
    let mut server = launch(app).await;
    server.restored = restored;
    server
}

/// A stored server with a custom configuration; the address and service token are filled in.
pub async fn start_stored_config(root: &Path, mut cfg: Config) -> Server {
    cfg.addr = "127.0.0.1:0".parse().unwrap();
    if cfg.service_token.is_none() {
        cfg.service_token = Some(SERVICE.into());
    }
    let app = Arc::new(App::with_store(cfg, jwt(), Store::open(root).unwrap()));
    let restored = app.restore().await;
    let mut server = launch(app).await;
    server.restored = restored;
    server
}

impl Server {
    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

pub fn now() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0)
}

pub fn token(actor: &str, role: Role, file_id: i64, session_id: &str) -> String {
    let claims = Claims {
        iss: "mail.lzmkm.ru".into(),
        sub: actor.into(),
        aud: "office-sync".into(),
        file_id,
        app: "gridcraft".into(),
        role,
        session_id: session_id.into(),
        iat: now(),
        exp: now() + 3_600,
    };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(SECRET)).unwrap()
}

pub async fn http(server: &Server, method: &str, path: &str, auth: Option<&str>, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let body = body.unwrap_or("");
    let auth = auth.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let text = String::from_utf8_lossy(&response).to_string();
    let status = text.lines().next().and_then(|line| line.split(' ').nth(1)).and_then(|code| code.parse().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, rest)| rest.to_string()).unwrap_or_default();
    (status, body)
}

pub async fn create(server: &Server, file_id: i64, takeover: bool) -> String {
    let body = json!({ "fileId": file_id, "app": "gridcraft", "owner": "ann", "initialName": "plan.xlsx", "takeover": takeover });
    let (status, response) = http(server, "POST", "/v1/sessions", Some(SERVICE), Some(&body.to_string())).await;
    assert_eq!(status, 201, "{response}");
    serde_json::from_str::<Value>(&response).unwrap()["sessionId"].as_str().unwrap().to_string()
}

/// Opens a session from bytes the cabinet holds: a workbook with a VBA project, a chart sheet, a
/// style and calculation settings that are not the defaults.
pub async fn create_with_bytes(server: &Server, file_id: i64, bytes: &[u8]) -> String {
    let body = json!({
        "fileId": file_id,
        "app": "gridcraft",
        "owner": "ann",
        "initialName": "plan.xlsm",
        "initialBytes": base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    let (status, response) = http(server, "POST", "/v1/sessions", Some(SERVICE), Some(&body.to_string())).await;
    assert_eq!(status, 201, "{response}");
    serde_json::from_str::<Value>(&response).unwrap()["sessionId"].as_str().unwrap().to_string()
}

/// Connects and sends `hello` with `last_version`. The first frame read is the welcome.
pub async fn open_at(server: &Server, session_id: &str, who: &str, role: Role, file_id: i64, last_version: u64) -> Socket {
    let request = format!("ws://{}/v1/sessions/{session_id}/ws", server.addr).into_client_request().unwrap();
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let hello = json!({ "type": "hello", "token": token(who, role, file_id, session_id), "name": who, "lastVersion": last_version });
    socket.send(Message::text(hello.to_string())).await.unwrap();
    socket
}

pub async fn open(server: &Server, session_id: &str, who: &str, role: Role, file_id: i64) -> Socket {
    open_at(server, session_id, who, role, file_id, 0).await
}

pub async fn next(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(5), futures_util::StreamExt::next(socket)).await.unwrap().unwrap().unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

pub async fn send_json(socket: &mut Socket, value: &Value) {
    socket.send(Message::text(value.to_string())).await.unwrap();
}

pub fn sheet_key(welcome: &Value) -> String {
    welcome["sheets"][0]["key"].as_str().unwrap().to_string()
}

/// A unique directory for a store, cleaned before use. The OS removes it with the temp volume;
/// tests that need two servers use the same root across a restart.
pub fn temp_root(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gridcraft-collab-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads a chunked snapshot and returns its version, epoch and bytes. Stops after the last chunk;
/// the following frame is the `resynced`.
pub async fn read_snapshot(socket: &mut Socket) -> (u64, u64, Vec<u8>) {
    use base64::Engine;
    let first = next(socket).await;
    assert_eq!(first["type"], "snapshot", "{first}");
    let version = first["version"].as_u64().unwrap();
    let epoch = first["epoch"].as_u64().unwrap();
    let total = first["total"].as_u64().unwrap();
    let mut seq = first["seq"].as_u64().unwrap();
    let mut bytes = base64::engine::general_purpose::STANDARD.decode(first["bytes"].as_str().unwrap()).unwrap();
    while seq + 1 < total {
        let frame = next(socket).await;
        assert_eq!(frame["type"], "snapshot", "{frame}");
        assert_eq!(frame["seq"].as_u64().unwrap(), seq + 1);
        bytes.extend(base64::engine::general_purpose::STANDARD.decode(frame["bytes"].as_str().unwrap()).unwrap());
        seq += 1;
    }
    (version, epoch, bytes)
}

pub fn op(op_id: &str, cmd: &str, params: Value, sheet: &str, a1: &str) -> String {
    json!({
        "type": "op",
        "op": {
            "opId": op_id,
            "doc": "book",
            "baseVersion": 0,
            "epoch": 0,
            "kind": "command",
            "target": { "sheet": sheet, "a1": a1 },
            "payload": { "cmd": cmd, "params": params }
        }
    })
    .to_string()
}

/// A cabinet on loopback: each accepted connection is answered with the next reply (`None` closes
/// without an answer, a lost response) and its request text is queued for the test. The URL keeps
/// `{sessionId}` for the server to substitute.
pub async fn fake_cabinet(replies: Vec<Option<&'static str>>) -> (String, tokio::sync::mpsc::Receiver<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let mut request = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stream.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..split]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&request).to_string()).await;
            if let Some(reply) = reply {
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        }
    });
    (format!("http://{address}/api/v1/internal/office/sessions/{{sessionId}}/commit"), receiver)
}

/// A complete `200` answer for the fake cabinet; `Content-Length` must match the body.
pub fn cabinet_reply(body: &'static str) -> &'static str {
    Box::leak(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_boxed_str())
}

/// One request body of the callback (everything after the head).
pub fn request_body(request: &str) -> String {
    request.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default()
}
