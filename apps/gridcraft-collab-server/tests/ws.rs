//! End-to-end tests over a real socket: two clients, a viewer, a bad token, the lease and a
//! commit, against the routes the cabinet calls.

mod support;

use base64::Engine;
use futures_util::SinkExt;
use gridcraft_collab::Role;
use serde_json::{Value, json};
use support::{SERVICE, create, create_with_bytes, http, next, now, op, open, open_at, read_snapshot, send_json, sheet_key, start};
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
async fn two_books_are_isolated_and_each_converges() {
    let server = start().await;
    let one = create(&server, 71, false).await;
    let two = create(&server, 72, false).await;

    let mut ann = open(&server, &one, "ann", Role::Editor, 71).await;
    let one_sheet = sheet_key(&next(&mut ann).await);
    let mut bob = open(&server, &one, "bob", Role::Editor, 71).await;
    assert_eq!(next(&mut bob).await["participants"].as_array().unwrap().len(), 2);
    assert_eq!(next(&mut ann).await["type"], "presence");

    let mut cy = open(&server, &two, "cy", Role::Editor, 72).await;
    let two_sheet = sheet_key(&next(&mut cy).await);
    let mut dee = open(&server, &two, "dee", Role::Editor, 72).await;
    assert_eq!(next(&mut dee).await["participants"].as_array().unwrap().len(), 2);
    assert_eq!(next(&mut cy).await["type"], "presence");

    // Each document has its own version sequence: both first edits are version 1.
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &one_sheet, "A1"))).await.unwrap();
    cy.send(Message::text(op("op-2", "cell.set", json!({ "input": 2 }), &two_sheet, "A1"))).await.unwrap();

    let applied_ann = next(&mut ann).await;
    let applied_bob = next(&mut bob).await;
    let applied_cy = next(&mut cy).await;
    let applied_dee = next(&mut dee).await;
    assert_eq!(applied_ann["applied"]["version"], 1);
    assert_eq!(applied_bob["applied"]["version"], 1);
    assert_eq!(applied_cy["applied"]["version"], 1);
    assert_eq!(applied_dee["applied"]["version"], 1);
    // Every replica of a document sees its own value and never the other book's.
    assert_eq!(applied_bob["applied"]["result"]["delta"][0]["cells"][0]["value"]["v"], 1.0);
    assert_eq!(applied_dee["applied"]["result"]["delta"][0]["cells"][0]["value"]["v"], 2.0);

    // The cabinet sees two independent sessions with one participant pair each.
    for (session, file_id) in [(&one, 71), (&two, 72)] {
        let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
        assert_eq!(status, 200, "{body}");
        let info: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(info["fileId"], file_id);
        assert_eq!(info["version"], 1);
        assert_eq!(info["participants"].as_array().unwrap().len(), 2);
    }

    // Each commit carries that book and only that book.
    let value_of = |bytes: &str| {
        let reply: Value = serde_json::from_str(bytes).unwrap();
        let raw = base64::engine::general_purpose::STANDARD.decode(reply["bytes"].as_str().unwrap()).unwrap();
        let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &raw).unwrap();
        let doc = gridcraft_engine::DocState::new(workbook, None, "plan.xlsx".into(), gridcraft_engine::Trust::FromFile);
        doc.wb.sheet(0).map(|sheet| sheet.value(gridcraft_engine::core::CellRef::parse("A1").unwrap())).unwrap()
    };
    let (status, one_commit) = http(&server, "POST", &format!("/v1/sessions/{one}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{one_commit}");
    let (status, two_commit) = http(&server, "POST", &format!("/v1/sessions/{two}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{two_commit}");
    assert_eq!(value_of(&one_commit), gridcraft_engine::core::Value::Number(1.0));
    assert_eq!(value_of(&two_commit), gridcraft_engine::core::Value::Number(2.0));

    // A second edit on one book still leaves the other at its version and value.
    bob.send(Message::text(op("op-3", "cell.set", json!({ "input": 3 }), &one_sheet, "A2"))).await.unwrap();
    assert_eq!(next(&mut bob).await["applied"]["version"], 2);
    assert_eq!(next(&mut ann).await["applied"]["version"], 2);
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{two}"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["version"], 1);
    server.stop().await;
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
    assert!(body.contains("gridcraft_collab_journal_ops"));
    assert!(body.contains("gridcraft_collab_op_latency_seconds_count"));
    assert!(!body.contains("test-secret"));
}

#[tokio::test]
async fn metrics_count_op_latency_and_reconnects() {
    let server = start().await;
    let session = create(&server, 13, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 13).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    // A client that comes back with the version it has needs no catch-up.
    let mut bob = open_at(&server, &session, "bob", Role::Editor, 13, 1).await;
    assert_eq!(next(&mut bob).await["type"], "welcome");

    // One that is behind catches up from the journal tail.
    let mut cy = open_at(&server, &session, "cy", Role::Editor, 13, 0).await;
    assert_eq!(next(&mut cy).await["type"], "welcome");
    assert_eq!(next(&mut cy).await["applied"]["version"], 1);
    assert_eq!(next(&mut cy).await["type"], "resynced");

    let (status, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("gridcraft_collab_resumes_total 1"), "{page}");
    assert!(page.contains("gridcraft_collab_catchups_total 1"), "{page}");
    assert!(page.contains("gridcraft_collab_resync_snapshots_total 0"), "{page}");
    assert!(page.contains("gridcraft_collab_op_latency_seconds_count 1"), "{page}");
    assert!(page.contains("gridcraft_collab_op_latency_seconds_bucket{le=\"+Inf\"} 1"), "{page}");
}

fn a1(workbook: &gridcraft_engine::model::Workbook) -> gridcraft_engine::core::Value {
    workbook.sheet(0).map(|sheet| sheet.value(gridcraft_engine::core::CellRef::new(0, 0))).unwrap_or(gridcraft_engine::core::Value::Empty)
}

fn a2(workbook: &gridcraft_engine::model::Workbook) -> gridcraft_engine::core::Value {
    workbook.sheet(0).map(|sheet| sheet.value(gridcraft_engine::core::CellRef::new(1, 0))).unwrap_or(gridcraft_engine::core::Value::Empty)
}

/// A replica that joins a room the cabinet opened from a file has to adopt that file: at version
/// 0 the journal holds nothing to replay, so a join with `lastVersion: 0` that got no workbook
/// would keep its own document and the two replicas would silently diverge.
#[tokio::test]
async fn a_join_of_a_session_opened_from_bytes_receives_the_workbook() {
    let server = start().await;
    let mut source = gridcraft_engine::Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({ "cell": "A1", "input": 7 })).expect("the file's cell");
    let bytes = gridcraft_engine::io::save_bytes(&source.doc().expect("a document").wb, "plan.xlsx").expect("the file");
    let session = create_with_bytes(&server, 91, &bytes).await;

    let mut ann = open(&server, &session, "ann", Role::Editor, 91).await;
    let welcome = next(&mut ann).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["version"], 0, "no operation has been applied yet");
    let (version, _epoch, workbook) = read_snapshot(&mut ann).await;
    assert_eq!(version, 0);
    assert_eq!(next(&mut ann).await["type"], "resynced");
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &workbook).expect("the room's workbook");
    assert_eq!(a1(&workbook), gridcraft_engine::core::Value::Number(7.0), "the replica gets the document the room holds");
}

/// The same rule once the room has moved on: a seeded document with an edit on top still has to
/// reach a version-zero joiner as a workbook. The tail alone would replay the edit onto whatever
/// document that joiner happened to hold and then tell it it sits at the room's version
/// (CodeRabbit 5848894080, `ws.rs:179`).
#[tokio::test]
async fn a_version_zero_join_of_a_seeded_room_with_edits_receives_the_workbook() {
    let server = start().await;
    let mut source = gridcraft_engine::Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({ "cell": "A1", "input": 7 })).expect("the file's cell");
    let bytes = gridcraft_engine::io::save_bytes(&source.doc().expect("a document").wb, "plan.xlsx").expect("the file");
    let session = create_with_bytes(&server, 94, &bytes).await;

    let mut ann = open(&server, &session, "ann", Role::Editor, 94).await;
    let sheet = sheet_key(&next(&mut ann).await);
    let _ = read_snapshot(&mut ann).await;
    assert_eq!(next(&mut ann).await["type"], "resynced");
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 8 }), &sheet, "A2"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let mut bob = open_at(&server, &session, "bob", Role::Editor, 94, 0).await;
    let welcome = next(&mut bob).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["version"], 1, "the room has one edit on top of the seed");
    let (version, _epoch, workbook_bytes) = read_snapshot(&mut bob).await;
    assert_eq!(version, 1, "the workbook is the room's head, not the seed");
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &workbook_bytes).expect("the room's workbook");
    assert_eq!(a1(&workbook), gridcraft_engine::core::Value::Number(7.0), "the seed is in the workbook");
    assert_eq!(a2(&workbook), gridcraft_engine::core::Value::Number(8.0), "and so is the edit");
}

/// The other side of the same rule: a room a participant opened as an empty workbook has no
/// authoritative copy to hand out, so the first client's document stays its own.
#[tokio::test]
async fn a_join_of_a_room_that_started_empty_gets_no_workbook() {
    let server = start().await;
    let session = create(&server, 92, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 92).await;
    assert_eq!(next(&mut ann).await["type"], "welcome");
    let early = tokio::time::timeout(std::time::Duration::from_millis(300), next(&mut ann)).await;
    assert!(early.is_err(), "nothing follows a welcome into an empty room: {:?}", early.ok());
}

/// A replica that cannot be repaired from the tail asks for the workbook itself and gets it at
/// the head, then the ordinary resynced marker.
#[tokio::test]
async fn a_client_can_ask_for_the_whole_workbook() {
    let server = start().await;
    let session = create(&server, 93, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 93).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 5 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    send_json(&mut ann, &json!({ "type": "snapshot" })).await;
    let (version, _epoch, bytes) = read_snapshot(&mut ann).await;
    assert_eq!(version, 1);
    assert_eq!(next(&mut ann).await["type"], "resynced");
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).expect("the room's workbook");
    assert_eq!(a1(&workbook), gridcraft_engine::core::Value::Number(5.0));
}
