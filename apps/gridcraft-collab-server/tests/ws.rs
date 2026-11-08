//! End-to-end tests over a real socket: two clients, a viewer, a bad token, the lease and a
//! commit, against the routes the cabinet calls.

mod support;

use base64::Engine;
use futures_util::SinkExt;
use gridcraft_collab::Role;
use serde_json::{Value, json};
use support::{SERVICE, create, http, next, now, op, open, sheet_key, start};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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
