//! The binary installs a logger: `log::*` from the library reaches stderr as one line per record,
//! and `RUST_LOG=off` leaves stderr empty (`docs/collab/server.md`, §«Логирование»).
//!
//! These tests spawn the real process, not the in-process router, because the logger is global
//! state installed by `main`.

mod support;

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::SinkExt;
use gridcraft_collab::Role;
use serde_json::{Value, json};
use support::{SERVICE, next, op, sheet_key, temp_root, token};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// One running server: its stderr, collected by a thread so a test can read it while it runs.
struct Server {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Server {
    fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|lines| lines.clone()).unwrap_or_default()
    }

    async fn wait_for(&self, needle: &str, within: Duration) -> String {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(line) = self.lines().into_iter().find(|line| line.contains(needle)) {
                return line;
            }
            assert!(tokio::time::Instant::now() < deadline, "no line with {needle:?} in {:?}", self.lines());
            sleep(Duration::from_millis(20)).await;
        }
    }
}

/// Starts the binary on an ephemeral port with `RUST_LOG`, one-second periodic snapshots and a
/// fresh volume.
fn spawn(rust_log: &str) -> Server {
    let root = temp_root("logging");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gridcraft-collab-server"))
        .env("GRIDCRAFT_COLLAB_JWT_SECRET", "test-secret")
        .env("GRIDCRAFT_COLLAB_SERVICE_TOKEN", SERVICE)
        .env("GRIDCRAFT_COLLAB_STORE", &root)
        .env("GRIDCRAFT_COLLAB_ADDR", "127.0.0.1:0")
        .env("GRIDCRAFT_COLLAB_SNAPSHOT_SECS", "1")
        .env("RUST_LOG", rust_log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary this crate builds runs");
    let stderr = child.stderr.take().expect("stderr is piped");
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&lines);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut lines) = sink.lock() {
                lines.push(line);
            }
        }
    });
    Server { child, lines, root }
}

/// The address from the `listening on` line; the process binds port 0 and says what it got.
async fn listening_addr(server: &Server) -> SocketAddr {
    let line = server.wait_for("listening on ", Duration::from_secs(20)).await;
    line.rsplit_once("listening on ")
        .and_then(|(_, rest)| rest.split(' ').next())
        .and_then(|addr| addr.parse().ok())
        .unwrap_or_else(|| panic!("the listening line names the address: {line}"))
}

/// `POST /v1/sessions` by hand, as the cabinet does.
async fn create(addr: SocketAddr, file_id: i64) -> String {
    let body = json!({ "fileId": file_id, "app": "gridcraft", "owner": "ann", "initialName": "plan.xlsx" }).to_string();
    let request = format!(
        "POST /v1/sessions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {SERVICE}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).await.expect("the server accepts");
    stream.write_all(request.as_bytes()).await.expect("the request is written");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.expect("the response is read");
    let response = String::from_utf8_lossy(&bytes).to_string();
    let (status, body) = response.split_once("\r\n\r\n").expect("a status line and a body");
    assert!(status.starts_with("HTTP/1.1 201"), "{response}");
    serde_json::from_str::<Value>(body).expect("a session body")["sessionId"].as_str().expect("a session id").to_string()
}

#[tokio::test]
async fn the_binary_logs_the_session_and_the_periodic_snapshot_to_stderr() {
    let server = spawn("info");
    let addr = listening_addr(&server).await;
    let listening = server.wait_for("listening on ", Duration::from_secs(1)).await;
    let ts = listening.split(' ').next().expect("a timestamp first");
    assert!(ts.ends_with('Z') && ts.contains('T'), "{listening}");
    assert!(listening.contains(" INFO  gridcraft_collab_server: gridcraft-collab-server listening on "), "{listening}");
    assert!(!listening.contains('\u{1b}'), "no ANSI escapes: {listening:?}");

    let session = create(addr, 31).await;
    let opened = server.wait_for("session opened", Duration::from_secs(5)).await;
    assert!(opened.contains(&format!("session opened fileId=31 session={session} fresh=true")), "{opened}");
    assert!(!opened.contains(SERVICE) && !opened.contains("test-secret"), "no secrets in a record: {opened}");

    // One operation moves the room past its snapshot base, so the one-second tick writes a snapshot.
    let request = format!("ws://{addr}/v1/sessions/{session}/ws").into_client_request().expect("a WebSocket URL");
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.expect("the server upgrades");
    let hello = json!({ "type": "hello", "token": token("ann", Role::Editor, 31, &session), "name": "ann", "lastVersion": 0 });
    socket.send(Message::text(hello.to_string())).await.expect("the hello is written");
    let welcome = next(&mut socket).await;
    socket.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet_key(&welcome), "A1"))).await.expect("the op is written");
    assert_eq!(next(&mut socket).await["type"], "applied");

    let snapshot = server.wait_for("periodic snapshot", Duration::from_secs(10)).await;
    assert!(snapshot.contains(" INFO  gridcraft_collab_server: periodic snapshot: 1 sessions"), "{snapshot}");
}

#[tokio::test]
async fn rust_log_off_leaves_stderr_empty() {
    let mut server = spawn("off");
    sleep(Duration::from_millis(800)).await;
    assert!(server.child.try_wait().expect("the child is running").is_none(), "the process is up");
    assert!(server.lines().is_empty(), "{:?}", server.lines());
}
