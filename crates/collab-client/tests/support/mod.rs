//! A collaboration server on an ephemeral port and signed tokens for the client tests.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use gridcraft_collab::{Claims, Role};
use gridcraft_collab_server::http::router;
use gridcraft_collab_server::{App, Config, JwtConfig};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const SECRET: &[u8] = b"client-test-secret";
pub const SERVICE: &str = "service-token";

pub struct Server {
    pub addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

pub async fn start() -> Server {
    let cfg =
        Config { addr: "127.0.0.1:0".parse().unwrap(), service_token: Some(SERVICE.into()), lease_ttl: Duration::from_secs(60), ..Config::default() };
    let app = Arc::new(App::new(cfg, JwtConfig::hs256(SECRET).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router(app)).await;
    });
    Server { addr, task }
}

impl Server {
    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }

    pub fn url(&self, session: &str) -> String {
        format!("ws://{}/v1/sessions/{session}/ws", self.addr)
    }
}

fn now() -> u64 {
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

pub async fn create(server: &Server, file_id: i64) -> String {
    let body = serde_json::json!({ "fileId": file_id, "app": "gridcraft", "owner": "ann", "initialName": "plan.xlsx" });
    let (status, response) = http(server, "POST", "/v1/sessions", Some(SERVICE), Some(&body.to_string())).await;
    assert_eq!(status, 201, "{response}");
    serde_json::from_str::<Value>(&response).unwrap()["sessionId"].as_str().unwrap().to_string()
}

/// Opens a session from a workbook the cabinet holds, so both replicas start from the same package
/// state (VBA project, chart sheets, styles, calculation settings).
pub async fn create_with_bytes(server: &Server, file_id: i64, bytes: &[u8]) -> String {
    let body = serde_json::json!({
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
