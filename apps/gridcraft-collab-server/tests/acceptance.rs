//! Acceptance scenarios of `TASK.md`: dynamic arrays, an operation with an old epoch, a large
//! range, and a committed file that opens outside GridCraft.

mod support;

use std::io::Write;

use futures_util::SinkExt;
use gridcraft_collab::Role;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::{Value as Json, json};
use support::{SERVICE, create, http, next, op, open, sheet_key, start, start_stored, temp_root};
use tokio_tungstenite::tungstenite::Message;

fn cell(workbook: &gridcraft_engine::model::Workbook, a1: &str) -> Value {
    let at = CellRef::parse(a1).unwrap();
    workbook.sheet(0).map(|sheet| sheet.value(at)).unwrap_or(Value::Empty)
}

fn decode(bytes: &str) -> Vec<u8> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bytes).unwrap()
}

#[tokio::test]
async fn a_dynamic_array_spills_and_the_spill_survives_a_commit() {
    let server = start().await;
    let session = create(&server, 51, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 51).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": "=SEQUENCE(3,1,1)" }), &sheet, "A1"))).await.unwrap();
    let applied = next(&mut ann).await;
    assert_eq!(applied["applied"]["version"], 1);
    let cells = applied["applied"]["result"]["delta"][0]["cells"].as_array().unwrap();
    assert!(cells.iter().any(|c| c["a1"] == "A2"), "the spill cells travel in the delta: {applied}");

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: Json = serde_json::from_str(&body).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &decode(reply["bytes"].as_str().unwrap())).unwrap();
    // A dynamic array's cached spill values are not stored in the file: Excel (and GridCraft on
    // open) fills them from the anchor. Check the file as the application shows it.
    let doc = gridcraft_engine::DocState::new(workbook, None, "plan.xlsx".into(), gridcraft_engine::Trust::FromFile);
    let sheet = doc.wb.sheet(0).unwrap();
    assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::Number(1.0));
    assert_eq!(sheet.value(CellRef::parse("A2").unwrap()), Value::Number(2.0));
    assert_eq!(sheet.value(CellRef::parse("A3").unwrap()), Value::Number(3.0));
    server.stop().await;
}

#[tokio::test]
async fn an_operation_with_an_old_epoch_is_refused() {
    let server = start().await;
    let session = create(&server, 52, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 52).await;
    let sheet = sheet_key(&next(&mut ann).await);
    let structural = json!({
        "type": "op",
        "op": {
            "opId": "op-rename",
            "doc": "book",
            "baseVersion": 0,
            "epoch": 0,
            "kind": "structural",
            "target": { "sheet": sheet, "a1": "A1" },
            "payload": { "cmd": "sheet.rename", "params": { "name": "Renamed" } }
        }
    });
    ann.send(Message::text(structural.to_string())).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["epoch"], 1);
    // Epoch 0 is now old: the client must resync before addressing cells again.
    ann.send(Message::text(op("op-2", "cell.set", json!({ "input": 1 }), &sheet, "A1"))).await.unwrap();
    let rejected = next(&mut ann).await;
    assert_eq!(rejected["rejected"]["code"], "stale");
    assert_eq!(rejected["rejected"]["retryable"], false);
    server.stop().await;
}

#[tokio::test]
async fn a_large_range_travels_and_commits() {
    let server = start().await;
    let session = create(&server, 53, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 53).await;
    let sheet = sheet_key(&next(&mut ann).await);
    let rows: Vec<Vec<Json>> = (0..2000).map(|r| (0..5).map(|c| Json::from(r * 5 + c)).collect()).collect();
    let params = json!({ "range": "A1:E2000", "values": rows });
    ann.send(Message::text(op("op-1", "range.setValues", params, &sheet, "A1:E2000"))).await.unwrap();
    let applied = next(&mut ann).await;
    assert_eq!(applied["applied"]["version"], 1);
    let cells = applied["applied"]["result"]["delta"][0]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 10_000);

    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: Json = serde_json::from_str(&body).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &decode(reply["bytes"].as_str().unwrap())).unwrap();
    assert_eq!(cell(&workbook, "E2000"), Value::Number(9999.0));
    server.stop().await;
}

#[tokio::test]
async fn a_restarted_session_commits_the_same_workbook() {
    let root = temp_root("acceptance");
    {
        let server = start_stored(&root).await;
        let session = create(&server, 54, false).await;
        let mut ann = open(&server, &session, "ann", Role::Editor, 54).await;
        let sheet = sheet_key(&next(&mut ann).await);
        ann.send(Message::text(op("op-1", "cell.set", json!({ "input": "=1+2" }), &sheet, "B2"))).await.unwrap();
        assert_eq!(next(&mut ann).await["applied"]["version"], 1);
        ann.close(None).await.unwrap();
        server.app.snapshot(&session).await.unwrap();
        server.stop().await;
    }
    let meta: Json = serde_json::from_slice(&std::fs::read(root.join("sessions/54/meta.json")).unwrap()).unwrap();
    let session = meta["sessionId"].as_str().unwrap().to_string();
    let server = start_stored(&root).await;
    assert_eq!(server.restored, 1);
    let (status, body) = http(&server, "GET", &format!("/v1/sessions/{session}"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Json>(&body).unwrap()["version"], 1);
    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200, "{body}");
    let reply: Json = serde_json::from_str(&body).unwrap();
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsx", &decode(reply["bytes"].as_str().unwrap())).unwrap();
    assert_eq!(cell(&workbook, "B2"), Value::Number(3.0));
    server.stop().await;
}

/// The committed file opens in LibreOffice and its values are there. Skipped when `soffice` is
/// not installed; the check is about the file, not about GridCraft's reader.
#[tokio::test]
async fn a_committed_file_opens_in_libreoffice() {
    if std::process::Command::new("soffice").arg("--version").output().map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("skipped: soffice is not available");
        return;
    }
    let server = start().await;
    let session = create(&server, 55, false).await;
    let mut ann = open(&server, &session, "ann", Role::Editor, 55).await;
    let sheet = sheet_key(&next(&mut ann).await);
    ann.send(Message::text(op("op-1", "cell.set", json!({ "input": "GridCraft" }), &sheet, "A1"))).await.unwrap();
    ann.send(Message::text(op("op-2", "cell.set", json!({ "input": 2026 }), &sheet, "B1"))).await.unwrap();
    assert_eq!(next(&mut ann).await["applied"]["version"], 1);
    assert_eq!(next(&mut ann).await["applied"]["version"], 2);
    let (status, body) = http(&server, "POST", &format!("/v1/sessions/{session}/commit"), Some(SERVICE), None).await;
    assert_eq!(status, 200);
    let reply: Json = serde_json::from_str(&body).unwrap();

    let dir = temp_root("libreoffice");
    let book = dir.join("committed.xlsx");
    let mut file = std::fs::File::create(&book).unwrap();
    file.write_all(&decode(reply["bytes"].as_str().unwrap())).unwrap();
    drop(file);
    let profile = format!("-env:UserInstallation=file://{}", dir.join("profile").display());
    let output = std::process::Command::new("soffice")
        .arg("--headless")
        .arg(&profile)
        .arg("--convert-to")
        .arg("csv")
        .arg("--outdir")
        .arg(&dir)
        .arg(&book)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let csv = std::fs::read_to_string(dir.join("committed.csv")).unwrap();
    assert!(csv.contains("GridCraft"), "{csv}");
    assert!(csv.contains("2026"), "{csv}");
    let _ = std::fs::remove_dir_all(&dir);
    server.stop().await;
}
