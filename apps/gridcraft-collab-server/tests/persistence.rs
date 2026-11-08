//! Restart, catch-up and takeover: the volume is authoritative, a reconnecting client gets the
//! journal tail, and a client that cannot be caught up gets the workbook.

mod support;

use futures_util::SinkExt;
use gridcraft_collab::Role;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;
use support::{SERVICE, create, http, next, op, open, open_at, read_snapshot, send_json, sheet_key, start, start_stored, temp_root};
use tokio_tungstenite::tungstenite::Message;

fn a1(workbook: &gridcraft_engine::model::Workbook) -> Value {
    workbook.sheet(0).map(|sheet| sheet.value(CellRef::new(0, 0))).unwrap_or(Value::Empty)
}

/// One gauge or counter of the `/metrics` page.
fn metric(page: &str, name: &str) -> u64 {
    page.lines().find_map(|line| line.strip_prefix(&format!("{name} ")).and_then(|value| value.trim().parse().ok())).unwrap_or(0)
}

#[tokio::test]
async fn a_restart_restores_the_room_and_sends_the_workbook() {
    let root = temp_root("restart");
    let session;
    {
        let server = start_stored(&root).await;
        assert_eq!(server.restored, 0);
        session = create(&server, 21, false).await;
        let mut ann = open(&server, &session, "ann", Role::Editor, 21).await;
        let welcome = next(&mut ann).await;
        let sheet = sheet_key(&welcome);
        ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 42 }), &sheet, "A1"))).await.unwrap();
        assert_eq!(next(&mut ann).await["applied"]["version"], 1);
        ann.close(None).await.unwrap();
        server.app.snapshot(&session).await.unwrap();
        server.stop().await;
    }

    let server = start_stored(&root).await;
    assert_eq!(server.restored, 1);
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["version"], 1);

    // A client that has nothing local cannot be caught up from a journal that starts at 1.
    let mut bob = open_at(&server, &session, "bob", Role::Editor, 21, 0).await;
    let hello = next(&mut bob).await;
    assert_eq!(hello["version"], 1, "{hello}");
    let (version, _epoch, bytes) = read_snapshot(&mut bob).await;
    assert_eq!(version, 1);
    assert_eq!(next(&mut bob).await["type"], "resynced");
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    assert_eq!(a1(&workbook), Value::Number(42.0));

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, reply["bytes"].as_str().unwrap()).unwrap();
    let (committed, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    assert_eq!(a1(&committed), Value::Number(42.0));

    // The catch-up above needed the whole workbook, because the journal starts after the base.
    let (_, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(metric(&page, "gridcraft_collab_resync_snapshots_total"), 1, "{page}");
    assert_eq!(metric(&page, "gridcraft_collab_catchups_total"), 0, "{page}");
    server.stop().await;
}

#[tokio::test]
async fn a_reconnect_gets_the_journal_tail_not_the_workbook() {
    let server = start().await;
    let session = create(&server, 22, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 22).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);
    ann.send(Message::text(op("op-2", "cell.set", json!({ "input": 2 }), &sheet, "A2"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 2);

    let mut bob = open_at(&server, &session, "bob", Role::Editor, 22, 1).await;
    assert_eq!(next(&mut bob).await["version"], 2);
    let tail = next(&mut bob).await;
    assert_eq!(tail["type"], "applied", "{tail}");
    assert_eq!(tail["applied"]["version"], 2);
    let done = next(&mut bob).await;
    assert_eq!(done["type"], "resynced");
    assert_eq!(done["version"], 2);
    server.stop().await;
}

#[tokio::test]
async fn a_takeover_continues_the_stored_session() {
    let root = temp_root("takeover");
    let server = start_stored(&root).await;
    let first = create(&server, 23, false).await;
    let mut ann = open(&server, &first, "ann", Role::Editor, 23).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 7 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);
    ann.close(None).await.unwrap();
    server.app.snapshot(&first).await.unwrap();

    let second = create(&server, 23, true).await;
    assert_ne!(first, second);
    assert_eq!(http(&server, "GET", &format!("/v1/sessions/{first}"), Some(SERVICE), None).await.0, 404);
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{second}"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["version"], 1);

    let mut bob = open_at(&server, &second, "bob", Role::Editor, 23, 1).await;
    assert_eq!(next(&mut bob).await["version"], 1);
    send_json(&mut bob, &json!({ "type": "ping", "nonce": 1 })).await;
    assert_eq!(next(&mut bob).await["type"], "pong");
    bob.send(Message::text(op("op-2", "cell.set", json!({ "input": 8 }), &sheet, "A2"))).await.unwrap();
    let applied = next(&mut bob).await;
    assert_eq!(applied["applied"]["version"], 2, "{applied}");
    server.stop().await;
}

#[tokio::test]
async fn a_quiet_room_is_snapshotted_by_flush_and_the_journal_gauges_drop() {
    let root = temp_root("flush");
    let server = start_stored(&root).await;
    let session = create(&server, 24, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 24).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 5 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let (_, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 1, "{page}");
    assert!(metric(&page, "gridcraft_collab_journal_bytes") > 0, "{page}");
    assert!(metric(&page, "gridcraft_collab_base_bytes") > 0, "{page}");
    assert_eq!(metric(&page, "gridcraft_collab_snapshots_total"), 1, "{page}");

    // The periodic tick and the shutdown path both call flush.
    assert_eq!(server.app.flush().await, 1);
    let (_, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 0, "{page}");
    assert_eq!(metric(&page, "gridcraft_collab_journal_bytes"), 0, "{page}");
    assert_eq!(metric(&page, "gridcraft_collab_snapshots_total"), 2, "{page}");
    assert!(metric(&page, "gridcraft_collab_last_snapshot_timestamp_seconds") > 0, "{page}");

    // Nothing changed since: a second tick writes nothing.
    assert_eq!(server.app.flush().await, 0);
    let (_, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(metric(&page, "gridcraft_collab_snapshots_total"), 2, "{page}");

    // The room still answers and keeps its state.
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["version"], 1);
    server.stop().await;
}

#[tokio::test]
async fn the_periodic_tick_snapshots_a_quiet_room() {
    use std::time::Duration;

    use gridcraft_collab_server::Config;
    use support::start_stored_config;

    let root = temp_root("tick");
    let server = start_stored_config(&root, Config { snapshot_interval: Duration::from_millis(40), ..Config::default() }).await;
    let session = create(&server, 25, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 25).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 3 }), &sheet, "A1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);

    let ticker = server.app.spawn_snapshots();
    let mut snapshotted = false;
    for _ in 0..100 {
        let (_, page) = http(&server, "GET", "/metrics", None, None).await;
        if metric(&page, "gridcraft_collab_snapshots_total") >= 2 {
            assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 0, "{page}");
            snapshotted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ticker.abort();
    assert!(snapshotted, "the periodic tick never wrote a snapshot");
    server.stop().await;
}
