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

/// Reads a cell from the bytes a commit returned.
async fn committed_a1(server: &support::Server, session: &str) -> Value {
    let (status, body) = http(server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, reply["bytes"].as_str().unwrap()).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    a1(&workbook)
}

/// The undo of an operation the base already covers is a version of its own: the journal must
/// carry it, or a restart (or a client catching up) brings the undone cell back.
#[tokio::test]
async fn an_undo_replays_from_the_journal_after_a_restart() {
    let root = temp_root("undo-restart");
    let session;
    {
        let server = start_stored(&root).await;
        session = create(&server, 26, false).await;
        let mut ann = open(&server, &session, "ann", Role::Editor, 26).await;
        let sheet = sheet_key(&next(&mut ann).await);
        ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 42 }), &sheet, "A1"))).await.unwrap();
        assert_eq!(next(&mut ann).await["applied"]["version"], 1);
        // The edit is compacted into base.xlsx; the undo then lands in the journal, above it.
        server.app.snapshot(&session).await.unwrap();
        let (_, page) = http(&server, "GET", "/metrics", None, None).await;
        assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 0, "{page}");

        send_json(&mut ann, &json!({ "type": "undo", "opId": "op-1" })).await;
        let undone = next(&mut ann).await;
        assert_eq!(undone["applied"]["version"], 2, "{undone}");
        assert_eq!(undone["applied"]["opId"], "undo-op-1", "{undone}");
        let (_, page) = http(&server, "GET", "/metrics", None, None).await;
        assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 1, "the undo is a journaled operation: {page}");
        ann.close(None).await.unwrap();
        // No flush and no snapshot on the way out: this is the crash the journal is there for.
        server.stop().await;
    }

    let server = start_stored(&root).await;
    assert_eq!(server.restored, 1, "the volume restores to the journal's head, not to the base");
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["version"], 2);
    let committed = committed_a1(&server, &session).await;
    assert_eq!(committed, Value::Empty, "the undone cell does not come back");

    // A client that still holds version 1 is caught up by the undo, not sent the workbook.
    let mut bob = open_at(&server, &session, "bob", Role::Editor, 26, 1).await;
    assert_eq!(next(&mut bob).await["version"], 2);
    let tail = next(&mut bob).await;
    assert_eq!(tail["type"], "applied", "{tail}");
    assert_eq!(tail["applied"]["version"], 2);
    assert_eq!(tail["applied"]["opId"], "undo-op-1");
    assert_eq!(next(&mut bob).await["type"], "resynced");
    let (_, page) = http(&server, "GET", "/metrics", None, None).await;
    assert_eq!(metric(&page, "gridcraft_collab_catchups_total"), 1, "{page}");
    server.stop().await;
}

/// A snapshot after the undo compacts the undo into the base: the journal is trimmed to nothing
/// and a restart still sees the undone document.
#[tokio::test]
async fn a_snapshot_after_an_undo_keeps_the_step_in_the_base() {
    let root = temp_root("undo-compact");
    let session;
    {
        let server = start_stored(&root).await;
        session = create(&server, 27, false).await;
        let mut ann = open(&server, &session, "ann", Role::Editor, 27).await;
        let sheet = sheet_key(&next(&mut ann).await);
        ann.send(Message::text(op("op-1", "cell.set", json!({ "input": 7 }), &sheet, "A1"))).await.unwrap();
        assert_eq!(next(&mut ann).await["applied"]["version"], 1);
        send_json(&mut ann, &json!({ "type": "undo", "opId": "op-1" })).await;
        assert_eq!(next(&mut ann).await["applied"]["version"], 2);
        send_json(&mut ann, &json!({ "type": "redo", "opId": "op-1" })).await;
        assert_eq!(next(&mut ann).await["applied"]["version"], 3);
        ann.close(None).await.unwrap();
        // Compaction: the base moves to version 3 and the journal it covers is dropped.
        assert_eq!(server.app.flush().await, 1);
        let (_, page) = http(&server, "GET", "/metrics", None, None).await;
        assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 0, "{page}");
        assert_eq!(metric(&page, "gridcraft_collab_journal_bytes"), 0, "{page}");
        server.stop().await;
    }

    let server = start_stored(&root).await;
    assert_eq!(server.restored, 1);
    let committed = committed_a1(&server, &session).await;
    assert_eq!(committed, Value::Number(7.0), "the redo is part of the base");
    let mut bob = open_at(&server, &session, "bob", Role::Editor, 27, 0).await;
    assert_eq!(next(&mut bob).await["version"], 3);
    let (version, _epoch, bytes) = read_snapshot(&mut bob).await;
    assert_eq!(version, 3);
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    assert_eq!(a1(&workbook), Value::Number(7.0));
    server.stop().await;
}

/// A structural undo's journal line carries the workbook-level state of the boundary it restored
/// (defined names, structure protection), not only the sheets. A restart that replays it must end
/// with the same names the undo put back, and the committed file must open with them.
#[tokio::test]
async fn a_structural_undo_replays_the_workbook_metadata_after_a_restart() {
    let root = temp_root("structural-undo-restart");
    let session;
    {
        let server = start_stored(&root).await;
        session = create(&server, 60, false).await;
        let mut ann = open(&server, &session, "ann", Role::Editor, 60).await;
        let sheet = sheet_key(&next(&mut ann).await);

        let op_at = |op_id: &str, base_version: u64, epoch: u64, kind: &str, cmd: &str, params: serde_json::Value, target: &str| {
            json!({
                "type": "op",
                "op": {
                    "opId": op_id,
                    "doc": "book",
                    "baseVersion": base_version,
                    "epoch": epoch,
                    "kind": kind,
                    "target": { "sheet": target, "a1": "A1" },
                    "payload": { "cmd": cmd, "params": params }
                }
            })
        };

        send_json(&mut ann, &op_at("add", 0, 0, "structural", "home.insertSheet", json!({ "name": "Extra" }), &sheet)).await;
        let added = next(&mut ann).await;
        assert_eq!(added["applied"]["version"], 1, "{added}");
        let extra = added["applied"]["sheets"][1]["key"].as_str().expect("the sheet map names the new sheet").to_string();

        let named =
            op_at("name", 1, 1, "command", "formulas.defineName", json!({ "name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra" }), &sheet);
        send_json(&mut ann, &named).await;
        assert_eq!(next(&mut ann).await["applied"]["version"], 2);
        // The base now holds the insert and the name; the delete and its undo go to the journal.
        server.app.snapshot(&session).await.unwrap();

        send_json(&mut ann, &op_at("del", 2, 1, "structural", "home.deleteSheet", json!({ "sheet": "Extra" }), &extra)).await;
        assert_eq!(next(&mut ann).await["applied"]["version"], 3);
        let (_, page) = http(&server, "GET", "/metrics", None, None).await;
        assert_eq!(metric(&page, "gridcraft_collab_journal_ops"), 1, "{page}");

        send_json(&mut ann, &json!({ "type": "undo", "opId": "del" })).await;
        let undone = next(&mut ann).await;
        assert_eq!(undone["applied"]["version"], 4, "{undone}");
        let names = undone["applied"]["book"]["names"].as_array().expect("the undo carries the names of the boundary");
        assert_eq!(names.len(), 1, "{undone}");
        assert_eq!(names[0]["name"], "ExtraRef", "{undone}");
        ann.close(None).await.unwrap();
        // No flush and no snapshot: this is the crash the journal is there for.
        server.stop().await;
    }

    let server = start_stored(&root).await;
    assert_eq!(server.restored, 1, "the volume restores to the journal's head");
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["version"], 4);

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, reply["bytes"].as_str().unwrap()).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &bytes).unwrap();
    assert!(workbook.sheet_index("Extra").is_some(), "the undone delete leaves the sheet in the file");
    let name = workbook.names.iter().find(|name| name.name == "ExtraRef").expect("the replayed undo restores the name");
    assert_eq!(name.formula, "Extra!$B$1");
    assert_eq!(name.scope, Some(1), "the name keeps the sheet it is scoped to");
    server.stop().await;
}
