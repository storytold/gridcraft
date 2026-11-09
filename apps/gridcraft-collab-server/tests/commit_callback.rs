//! The cabinet commit callback (`INTEGRATION.md` §4.1/§5.2, mail-frontend M87): the room posts
//! the built `.xlsx` to the cabinet, stores the etag it answers, shows it to every subscriber, and
//! keeps the contract's `409`/`401`/`413`/`5xx` answers. The cabinet here is a loopback listener,
//! not a real service.

mod support;

use std::time::Duration;

use base64::Engine as _;
use futures_util::SinkExt;
use gridcraft_collab::Role;
use gridcraft_collab_server::Config;
use gridcraft_collab_server::cabinet::CabinetConfig;
use serde_json::{Value, json};
use support::{SERVICE, cabinet_reply, create, fake_cabinet, http, next, op, open, request_body, sheet_key, start_stored_config, temp_root};
use tokio_tungstenite::tungstenite::Message;

fn with_cabinet(url: &str, attempts: u32) -> Config {
    Config {
        cabinet: Some(CabinetConfig { url: url.to_string(), token: "callback-token".into(), timeout: Duration::from_secs(5), attempts }),
        ..Config::default()
    }
}

fn decode(bytes: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(bytes).expect("base64")
}

/// Reads frames until a `committed` arrives; presence and revisions may sit in between.
async fn committed(socket: &mut support::Socket) -> Value {
    loop {
        let frame = next(socket).await;
        if frame["type"] == "committed" {
            return frame;
        }
    }
}

/// Reads frames until an `error` arrives.
async fn error_frame(socket: &mut support::Socket) -> Value {
    loop {
        let frame = next(socket).await;
        if frame["type"] == "error" {
            return frame;
        }
    }
}

#[tokio::test]
async fn a_commit_is_published_to_the_cabinet_and_every_peer_sees_its_version() {
    let first = cabinet_reply(r#"{"version":1794186677,"etag":"1794186677"}"#);
    let second = cabinet_reply(r#"{"version":1794186678,"etag":"1794186678"}"#);
    let (url, mut requests) = fake_cabinet(vec![Some(first), Some(second)]).await;
    let root = temp_root("cabinet-publish");
    let server = start_stored_config(&root, with_cabinet(&url, 2)).await;
    let session = create(&server, 61, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 61).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 42 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{reply}");
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["version"], 1);
    assert_eq!(reply["cabinet"]["version"], 1_794_186_677);
    assert_eq!(reply["cabinet"]["etag"], "1794186677");
    let sha256 = reply["sha256"].as_str().unwrap().to_string();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &decode(reply["bytes"].as_str().unwrap())).unwrap();
    assert_eq!(
        workbook.sheet(0).map(|sheet| sheet.value(gridcraft_engine::core::CellRef::new(0, 0))),
        Some(gridcraft_engine::core::Value::Number(42.0))
    );

    let stored = committed(&mut ann).await;
    assert_eq!(stored["version"], 1, "the room's version is unchanged by the publication");
    assert_eq!(stored["sha256"], sha256);
    assert_eq!(stored["cabinet"]["version"], 1_794_186_677);
    assert_eq!(stored["cabinet"]["etag"], "1794186677", "the peers show the cabinet's copy");

    let request = requests.recv().await.expect("the cabinet was called");
    assert!(request.starts_with(&format!("POST /api/v1/internal/office/sessions/{session}/commit")), "the endpoint is the session's: {request}");
    assert!(request.contains("Authorization: Bearer callback-token"), "{request}");
    let body: Value = serde_json::from_str(&request_body(&request)).unwrap();
    assert_eq!(body["fileId"], "61");
    assert_eq!(body["version"], 1);
    assert_eq!(body["sha256"], sha256);
    assert!(body.get("expectedVersion").is_none(), "the first commit of a room expects nothing: {body}");
    assert!(decode(body["bytes"].as_str().unwrap()).starts_with(b"PK\x03\x04"), "the body carries the built workbook");

    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["cabinet"]["etag"], "1794186678");
    let second = requests.recv().await.expect("the cabinet was called again");
    assert_eq!(serde_json::from_str::<Value>(&request_body(&second)).unwrap()["expectedVersion"], "1794186677");
    server.stop().await;
}

#[tokio::test]
async fn a_cabinet_conflict_is_not_retried_and_leaves_the_stored_etag_alone() {
    let first = cabinet_reply(r#"{"version":1794186677,"etag":"1794186677"}"#);
    let conflict = "HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}";
    let second = cabinet_reply(r#"{"version":1794186679,"etag":"1794186679"}"#);
    let (url, mut requests) = fake_cabinet(vec![Some(first), Some(conflict), Some(second)]).await;
    let root = temp_root("cabinet-conflict");
    let server = start_stored_config(&root, with_cabinet(&url, 3)).await;
    let session = create(&server, 62, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 62).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let (status, _) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    let _ = requests.recv().await.expect("the accepted commit");
    let _ = committed(&mut ann).await;

    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 409, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["code"], "cabinetConflict");
    let notice = error_frame(&mut ann).await;
    assert_eq!(notice["code"], "cabinetConflict");
    assert_eq!(notice["retryable"], true, "the clients must resync: {notice}");
    let _ = requests.recv().await.expect("the refused commit");
    assert!(requests.try_recv().is_err(), "409 gets exactly one attempt");

    let (status, _) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    let next_request = requests.recv().await.expect("the next commit");
    assert_eq!(
        serde_json::from_str::<Value>(&request_body(&next_request)).unwrap()["expectedVersion"],
        "1794186677",
        "a refusal does not advance the etag"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_transient_cabinet_failure_is_retried_with_the_same_body() {
    let ok = cabinet_reply(r#"{"version":1794186677,"etag":"1794186677"}"#);
    let (url, mut requests) = fake_cabinet(vec![Some("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n"), Some(ok)]).await;
    let root = temp_root("cabinet-retry");
    let server = start_stored_config(&root, with_cabinet(&url, 2)).await;
    let session = create(&server, 63, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 63).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": "=1+1" }), &sheet, "B2"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(committed(&mut ann).await["cabinet"]["etag"], "1794186677");

    let first = requests.recv().await.expect("first attempt");
    let second = requests.recv().await.expect("second attempt");
    assert_eq!(request_body(&first), request_body(&second), "a retry is the same body, so it cannot become a second version");
    server.stop().await;
}

#[tokio::test]
async fn a_refused_token_and_a_size_refusal_keep_their_meaning() {
    let unauthorized = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
    let (url, mut requests) = fake_cabinet(vec![Some(unauthorized)]).await;
    let root = temp_root("cabinet-token");
    let server = start_stored_config(&root, with_cabinet(&url, 3)).await;
    let session = create(&server, 64, false).await;
    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 403, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["code"], "cabinetUnauthorized");
    let request = requests.recv().await.expect("one attempt");
    assert!(!request.lines().next().unwrap_or_default().contains("callback-token"), "the token never goes in the URL: {request}");
    assert!(requests.try_recv().is_err(), "401 gets exactly one attempt");
    server.stop().await;

    let too_large = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n";
    let (url, mut requests) = fake_cabinet(vec![Some(too_large)]).await;
    let root = temp_root("cabinet-large");
    let server = start_stored_config(&root, with_cabinet(&url, 3)).await;
    let session = create(&server, 65, false).await;
    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 413, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["code"], "cabinetTooLarge");
    let _ = requests.recv().await.expect("one attempt");
    assert!(requests.try_recv().is_err(), "413 gets exactly one attempt");
    server.stop().await;
}

#[tokio::test]
async fn a_spent_retry_is_a_gateway_failure_and_publishes_nothing() {
    let down = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
    let (url, mut requests) = fake_cabinet(vec![Some(down), Some(down)]).await;
    let root = temp_root("cabinet-down");
    let server = start_stored_config(&root, with_cabinet(&url, 2)).await;
    let session = create(&server, 66, false).await;
    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 502, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["code"], "cabinetUnavailable");
    let first = requests.recv().await.expect("first attempt");
    let second = requests.recv().await.expect("second attempt");
    assert_eq!(request_body(&first), request_body(&second));
    server.stop().await;
}

#[tokio::test]
async fn the_etag_survives_a_restart_so_the_next_commit_still_checks_it() {
    let first = cabinet_reply(r#"{"version":1794186677,"etag":"1794186677"}"#);
    let second = cabinet_reply(r#"{"version":1794186678,"etag":"1794186678"}"#);
    let (url, mut requests) = fake_cabinet(vec![Some(first), Some(second)]).await;
    let root = temp_root("cabinet-restart");
    let session;
    {
        let server = start_stored_config(&root, with_cabinet(&url, 2)).await;
        session = create(&server, 67, false).await;
        let (status, _) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
        assert_eq!(status, 200);
        let _ = requests.recv().await.expect("the accepted commit");
        // The snapshot writes the base; the commit wrote its etag to `meta.json`.
        server.app.snapshot(&session).await.unwrap();
        server.stop().await;
    }

    let server = start_stored_config(&root, with_cabinet(&url, 2)).await;
    assert_eq!(server.restored, 1);
    let (status, reply) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["cabinet"]["etag"], "1794186678");
    let next_request = requests.recv().await.expect("the commit after the restart");
    assert_eq!(
        serde_json::from_str::<Value>(&request_body(&next_request)).unwrap()["expectedVersion"],
        "1794186677",
        "a restart does not lose the cabinet's expected etag"
    );
    server.stop().await;
}
