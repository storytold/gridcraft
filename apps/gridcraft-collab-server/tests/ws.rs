//! End-to-end tests over a real socket: two clients, a viewer, a bad token, the lease and a
//! commit, against the routes the cabinet calls.
//!
//! HTTP is spoken by hand so the test does not pull a client library; WebSocket uses the same
//! tungstenite version the server does.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use gridcraft_collab::{Claims, Role};
use gridcraft_collab_server::http::router;
use gridcraft_collab_server::{App, Config, JwtConfig};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const SECRET: &[u8] = b"test-secret";
const SERVICE: &str = "service-token";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Server {
    addr: SocketAddr,
}

async fn start() -> Server {
    let cfg =
        Config { addr: "127.0.0.1:0".parse().unwrap(), service_token: Some(SERVICE.into()), lease_ttl: Duration::from_secs(60), ..Config::default() };
    let app = Arc::new(App::new(cfg, JwtConfig::hs256(SECRET).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(app)).await;
    });
    Server { addr }
}

fn now() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0)
}

fn token(actor: &str, role: Role, file_id: i64, session_id: &str) -> String {
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

async fn http(server: &Server, method: &str, path: &str, auth: Option<&str>, body: Option<&str>) -> (u16, String) {
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

async fn create(server: &Server, file_id: i64, takeover: bool) -> String {
    let body = json!({ "fileId": file_id, "app": "gridcraft", "owner": "ann", "initialName": "plan.xlsx", "takeover": takeover });
    let (status, response) = http(server, "POST", "/v1/sessions", Some(SERVICE), Some(&body.to_string())).await;
    assert_eq!(status, 201, "{response}");
    serde_json::from_str::<Value>(&response).unwrap()["sessionId"].as_str().unwrap().to_string()
}

async fn open(server: &Server, session_id: &str, who: &str, role: Role, file_id: i64) -> Socket {
    let request = format!("ws://{}/v1/sessions/{session_id}/ws", server.addr).into_client_request().unwrap();
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let hello = json!({ "type": "hello", "token": token(who, role, file_id, session_id), "name": who, "lastVersion": 0 });
    socket.send(Message::text(hello.to_string())).await.unwrap();
    socket
}

async fn next(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

fn sheet_key(welcome: &Value) -> String {
    welcome["sheets"][0]["key"].as_str().unwrap().to_string()
}

fn op(op_id: &str, cmd: &str, params: Value, sheet: &str, a1: &str) -> String {
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

#[tokio::test]
async fn two_editors_converge_on_one_book() {
    let server = start().await;
    let session = create(&server, 7, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 7).await;
    let mut bob = open(&server, &session, "bob", Role::Editor, 7).await;

    let welcome = next(&mut ann).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["role"], json!("editor"));
    assert!(welcome["lease"]["expiresAt"].as_u64().unwrap() > now());
    let sheet = sheet_key(&welcome);
    let bob_welcome = next(&mut bob).await;
    assert_eq!(bob_welcome["participants"].as_array().unwrap().len(), 2);
    let joined = next(&mut ann).await;
    assert_eq!(joined["type"], "presence");
    assert_eq!(joined["participants"].as_array().unwrap().len(), 2);

    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    let applied_ann = next(&mut ann).await;
    let applied_bob = next(&mut bob).await;
    assert_eq!(applied_ann["type"], "applied");
    assert_eq!(applied_ann["applied"]["version"], 1);
    assert_eq!(applied_bob["applied"]["version"], 1);
    assert_eq!(applied_bob["applied"]["actor"], "ann");
    assert_eq!(applied_bob["applied"]["result"]["delta"][0]["cells"][0]["value"]["v"], 1.0);

    bob.send(Message::text(op("op-2", "cell.set", json!({ "input": 2 }), &sheet, "A2"))).await.unwrap();
    let applied_bob = next(&mut bob).await;
    let applied_ann = next(&mut ann).await;
    assert_eq!(applied_bob["applied"]["version"], 2);
    assert_eq!(applied_ann["applied"]["actor"], "bob");

    ann.close(None).await.unwrap();
    let presence = next(&mut bob).await;
    assert_eq!(presence["type"], "presence");
    assert_eq!(presence["participants"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_viewer_is_refused_before_the_engine() {
    let server = start().await;
    let session = create(&server, 8, false).await;
    let mut viewer = open(&server, &session, "vic", Role::Viewer, 8).await;
    let welcome = next(&mut viewer).await;
    let sheet = sheet_key(&welcome);
    viewer.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    let rejected = next(&mut viewer).await;
    assert_eq!(rejected["type"], "rejected");
    assert_eq!(rejected["rejected"]["code"], "forbidden");
    assert_eq!(rejected["rejected"]["retryable"], false);

    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["version"], 0);
}

#[tokio::test]
async fn a_bad_token_never_joins() {
    let server = start().await;
    let session = create(&server, 9, false).await;
    let request = format!("ws://{}/v1/sessions/{session}/ws", server.addr).into_client_request().unwrap();
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let forged = json!({ "type": "hello", "token": "not-a-token", "name": "eve", "lastVersion": 0 });
    socket.send(Message::text(forged.to_string())).await.unwrap();
    let error = next(&mut socket).await;
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "token-invalid");

    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["participants"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn the_lease_allows_one_session_and_the_owner_takes_over() {
    let server = start().await;
    let first = create(&server, 10, false).await;
    let (status, body) =
        http(&server, "POST", "/v1/sessions", Some(SERVICE), Some(&json!({ "fileId": 10, "app": "gridcraft", "owner": "ann" }).to_string())).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["code"], "lease");

    let second = create(&server, 10, true).await;
    assert_ne!(first, second);
    let (status, _) = http(&server, "GET", &format!("/v1/sessions/{first}"), Some(SERVICE), None).await;
    assert_eq!(status, 404);

    let (status, _) =
        http(&server, "POST", "/v1/sessions", None, Some(&json!({ "fileId": 11, "app": "gridcraft", "owner": "ann" }).to_string())).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn commit_returns_the_authoritative_file() {
    let server = start().await;
    let session = create(&server, 12, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 12).await;
    let welcome = next(&mut ann).await;
    let sheet = sheet_key(&welcome);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 42 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit?expectedVersion=1"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["version"], 1);
    assert_eq!(reply["sha256"].as_str().unwrap().len(), 64);
    let bytes = base64::engine::general_purpose::STANDARD.decode(reply["bytes"].as_str().unwrap()).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    assert_eq!(workbook.sheets.len(), 1);

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit?expectedVersion=99"), Some(SERVICE), None).await;
    assert_eq!(status, 409, "{body}");
}

#[tokio::test]
async fn health_and_metrics_are_open_to_the_supervisor() {
    let server = start().await;
    let (status, body) = http(&server, "GET", "/healthz", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "ok");
    let (status, body) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("gridcraft_collab_sessions_open"));
    assert!(!body.contains("test-secret"));
}
