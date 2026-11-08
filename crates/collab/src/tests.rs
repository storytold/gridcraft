//! Protocol tests: no socket, no GUI — a room over one engine session.

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::protocol::{ClientOp, Kind, Payload, RejectCode, Target};
use crate::{Class, Room, classify};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn key_of(room: &Room, s: &Session) -> String {
    let d = s.doc().unwrap();
    room.sheet_keys().key_for_index(&d.wb, 0).unwrap().to_string()
}

fn op(op_id: &str, cmd: &str, params: Json, sheet: &str, a1: &str) -> ClientOp {
    ClientOp {
        op_id: op_id.to_string(),
        doc: "book".into(),
        base_version: 0,
        epoch: 0,
        kind: Kind::Command,
        target: Target { sheet: sheet.to_string(), a1: Some(a1.to_string()) },
        payload: Payload { cmd: cmd.to_string(), params },
        preview: None,
    }
}

#[test]
fn a_cell_operation_comes_back_as_an_authoritative_delta() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    let applied = room.apply(&mut s, op("op-1", "cell.set", json!({"input": 2}), &sheet, "A1"), "alice").unwrap();
    assert_eq!(applied.version, 1);
    let delta = applied.result.delta.expect("cells changed");
    let cell = delta.iter().flat_map(|d| &d.cells).find(|c| c.a1 == "A1").expect("A1 is reported");
    let value: Value = serde_json::from_value(cell.value.clone()).unwrap();
    assert_eq!(value.as_f64(), Some(2.0));
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap()).as_f64(), Some(2.0));
}

#[test]
fn a_repeated_op_id_is_not_applied_twice() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    let first = room.apply(&mut s, op("same", "cell.set", json!({"input": 1}), &sheet, "A1"), "alice").unwrap();
    let again = room.apply(&mut s, op("same", "cell.set", json!({"input": 9}), &sheet, "A1"), "alice").unwrap();
    assert_eq!(first, again);
    assert_eq!(room.version(), 1);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap()).as_f64(), Some(1.0));
}

#[test]
fn a_dependent_formula_value_travels_with_the_delta() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 2}), &sheet, "A1"), "alice").unwrap();
    room.apply(&mut s, op("b", "cell.set", json!({"input": "=A1*3"}), &sheet, "B2"), "alice").unwrap();
    let applied = room.apply(&mut s, op("c", "cell.set", json!({"input": 5}), &sheet, "A1"), "alice").unwrap();
    let delta = applied.result.delta.expect("cells changed");
    let addresses: Vec<&str> = delta.iter().flat_map(|d| d.cells.iter().map(|c| c.a1.as_str())).collect();
    assert!(addresses.contains(&"A1"), "{addresses:?}");
    assert!(addresses.contains(&"B2"), "the dependent formula is part of the delta: {addresses:?}");
    let b2 = delta.iter().flat_map(|d| &d.cells).find(|c| c.a1 == "B2").unwrap();
    let value: Value = serde_json::from_value(b2.value.clone()).unwrap();
    assert_eq!(value.as_f64(), Some(15.0));
}

#[test]
fn a_structural_operation_needs_the_head_version() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("one", "cell.set", json!({"input": 1}), &sheet, "A1"), "alice").unwrap();
    let mut structural = op("two", "home.insertRows", json!({"rows": "2"}), &sheet, "A2");
    structural.kind = Kind::Structural;
    structural.base_version = 0;
    let refused = room.apply(&mut s, structural, "alice").expect_err("stale base is refused");
    match refused.rejected().code {
        RejectCode::Stale => {}
        other => panic!("expected stale, got {other:?}"),
    }
    assert!(refused.rejected().retryable, "resending against the new head can work");
    assert_eq!(room.version(), 1, "the refused operation did not advance the version");
}

#[test]
fn a_structural_operation_at_the_head_bumps_the_epoch() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    let mut structural = op("ins", "home.insertRows", json!({"rows": "2", "count": 1}), &sheet, "A2");
    structural.kind = Kind::Structural;
    structural.base_version = 0;
    let applied = room.apply(&mut s, structural, "alice").unwrap();
    assert_eq!(applied.version, 1);
    assert_eq!(applied.epoch, 1);
}

#[test]
fn commands_outside_the_allowlist_are_refused() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    for (id, cmd) in [("f", "file.save"), ("u", "edit.undo"), ("m", "macro.run")] {
        let refused = room.apply(&mut s, op(id, cmd, json!({}), &sheet, "A1"), "alice").expect_err("refused");
        assert_eq!(refused.rejected().code, RejectCode::Forbidden, "{cmd}");
    }
    assert_eq!(room.version(), 0);
}

#[test]
fn the_allowlist_separates_structural_from_merge_and_denies_code() {
    assert_eq!(classify("cell.set"), Some(Class::Merge));
    assert_eq!(classify("home.bold"), Some(Class::Merge));
    assert_eq!(classify("home.insertRows"), Some(Class::Structural));
    assert_eq!(classify("home.deleteSheet"), Some(Class::Structural));
    assert_eq!(classify("table.style"), Some(Class::Structural));
    assert_eq!(classify("file.open"), None);
    assert_eq!(classify("python.run"), None);
    assert_eq!(classify("edit.undo"), None);
    assert_eq!(classify("something.unknown"), None);
}

#[test]
fn a_deleted_sheet_key_is_a_tombstone() {
    let mut s = session();
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("home.insertSheet", json!({})).unwrap();
    let mut room = Room::new(&s);
    let first = {
        let d = s.doc().unwrap();
        room.sheet_keys().key_for_index(&d.wb, 0).unwrap()
    };
    s.apply_remote("home.deleteSheet", json!({"sheet": 0})).unwrap();
    room.sync_keys(&s);
    let d = s.doc().unwrap();
    assert!(room.sheet_keys().is_dead(first), "the key of a deleted sheet stays dead");
    assert!(room.sheet_keys().index_for_key(&d.wb, first).is_none());
}

#[test]
fn a_viewer_cannot_write_even_with_a_valid_operation() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "Ann", crate::Role::Viewer).unwrap();
    let sheet = hub.sheet_key("book", 0).unwrap();
    let refused = hub.apply("book", "ann", op("v1", "cell.set", json!({"input": 1}), &sheet, "A1")).expect_err("viewer is refused");
    match refused {
        crate::HubError::Forbidden => {}
        other => panic!("expected forbidden, got {other:?}"),
    }
    assert_eq!(hub.version("book"), Some(0));
}

#[test]
fn an_editor_changes_the_document_and_presence_is_returned_to_others() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "bob", "Bob", crate::Role::Viewer).unwrap();
    let sheet = hub.sheet_key("book", 0).unwrap();
    let applied = hub.apply("book", "ann", op("e1", "cell.set", json!({"input": 4}), &sheet, "A1")).unwrap();
    assert_eq!(applied.version, 1);
    assert_eq!(applied.actor, "ann");
    let presence = crate::Presence { sheet_key: sheet.clone(), active: "A1".into(), ranges: vec![], viewport: None };
    let all = hub.presence("book", "bob", Some(presence)).unwrap();
    let bob = all.iter().find(|p| p.actor == "bob").unwrap();
    assert_eq!(bob.presence.as_ref().unwrap().active, "A1");
    assert_eq!(all.len(), 2, "both participants are listed");
}

#[test]
fn commit_writes_the_authoritative_workbook_as_a_zip_package() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "Ann", crate::Role::Owner).unwrap();
    let bytes = hub.commit("book").unwrap();
    assert!(bytes.starts_with(b"PK"), "an .xlsx is a zip package");
}

fn claims(sub: &str, role: crate::Role) -> crate::Claims {
    crate::Claims {
        iss: "mail.lzmkm.ru".into(),
        sub: sub.into(),
        aud: "office-sync".into(),
        file_id: 7,
        app: "gridcraft".into(),
        role,
        session_id: "book".into(),
        iat: 900,
        exp: 2_000,
    }
}

fn expected<'a>() -> crate::Expected<'a> {
    crate::Expected { aud: "office-sync", app: "gridcraft", file_id: 7, session_id: "book" }
}

#[test]
fn a_valid_token_joins_and_gets_the_sheets_and_version() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let (connection, welcome) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", None).unwrap();
    assert_eq!(connection.actor, "ann");
    match welcome {
        crate::ServerMessage::Welcome { role, version, sheets, participants, .. } => {
            assert_eq!(role, crate::Role::Editor);
            assert_eq!(version, 0);
            assert_eq!(sheets.len(), 1);
            assert_eq!(participants.len(), 1);
        }
        other => panic!("expected welcome, got {other:?}"),
    }
}

#[test]
fn a_token_for_another_file_or_an_expired_one_is_refused() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let mut wrong_file = claims("ann", crate::Role::Editor);
    wrong_file.file_id = 8;
    assert_eq!(gw.connect(&wrong_file, 1_000, expected(), "Ann", None).unwrap_err(), crate::AuthError::WrongFile);
    let mut expired = claims("ann", crate::Role::Editor);
    expired.exp = 999;
    assert_eq!(gw.connect(&expired, 1_000, expected(), "Ann", None).unwrap_err(), crate::AuthError::Expired);
    let mut wrong_app = claims("ann", crate::Role::Editor);
    wrong_app.app = "pdfcraft".into();
    assert_eq!(gw.connect(&wrong_app, 1_000, expected(), "Ann", None).unwrap_err(), crate::AuthError::WrongApp);
}

#[test]
fn a_viewer_gets_a_rejection_from_the_message_loop() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let (connection, welcome) = gw.connect(&claims("vic", crate::Role::Viewer), 1_000, expected(), "Vic", None).unwrap();
    let sheet = match welcome {
        crate::ServerMessage::Welcome { sheets, .. } => sheets[0].key.clone(),
        other => panic!("expected welcome, got {other:?}"),
    };
    let messages = gw.handle(&connection, crate::ClientMessage::Op { op: op("v", "cell.set", json!({"input": 1}), &sheet, "A1") });
    match &messages[0] {
        crate::ServerMessage::Rejected { rejected } => assert_eq!(rejected.code, crate::RejectCode::Forbidden),
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[test]
fn presence_and_ping_round_trip_through_the_gateway() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let (ann, _) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", None).unwrap();
    let (bob, _) = gw.connect(&claims("bob", crate::Role::Viewer), 1_000, expected(), "Bob", None).unwrap();
    let presence = crate::Presence { sheet_key: "1".into(), active: "B2".into(), ranges: vec![], viewport: None };
    let messages = gw.handle(&ann, crate::ClientMessage::Presence { presence: Some(presence.clone()) });
    match &messages[0] {
        crate::ServerMessage::Presence { participants } => {
            assert_eq!(participants.len(), 2);
            assert_eq!(participants.iter().find(|p| p.actor == "ann").unwrap().presence, Some(presence));
        }
        other => panic!("expected presence, got {other:?}"),
    }
    match gw.handle(&bob, crate::ClientMessage::Ping { nonce: 42 })[0] {
        crate::ServerMessage::Pong { nonce } => assert_eq!(nonce, 42),
        ref other => panic!("expected pong, got {other:?}"),
    }
}

#[test]
fn a_hello_on_the_wire_carries_the_signed_token_not_claims() {
    let text = r#"{"type":"hello","token":"a.b.c","name":"Ann","lastVersion":3}"#;
    let message: crate::ClientMessage = serde_json::from_str(text).unwrap();
    assert!(matches!(message, crate::ClientMessage::Hello { .. }));
    if let crate::ClientMessage::Hello { token, name, last_version } = message {
        assert_eq!(token, "a.b.c");
        assert_eq!(name, "Ann");
        assert_eq!(last_version, 3);
    }
}

#[test]
fn the_wire_uses_the_camel_case_names_of_the_contract() {
    let welcome = crate::ServerMessage::Welcome {
        session_id: "s-1".into(),
        actor_id: "ann".into(),
        role: crate::Role::Editor,
        version: 1,
        epoch: 0,
        sheets: vec![crate::SheetName { key: "7".into(), name: "Sheet1".into() }],
        participants: vec![],
        lease: Some(crate::LeaseInfo { owner: "ann".into(), expires_at: 60 }),
    };
    let text = serde_json::to_string(&welcome).unwrap();
    assert!(text.contains("\"sessionId\":\"s-1\""), "{text}");
    assert!(text.contains("\"actorId\":\"ann\""), "{text}");
    assert!(text.contains("\"expiresAt\":60"), "{text}");

    let presence = crate::Presence {
        sheet_key: "7".into(),
        active: "A1".into(),
        ranges: vec!["A1:B2".into()],
        viewport: Some(crate::Viewport { top_left: "A1".into(), rows: 20, cols: 8 }),
    };
    let text = serde_json::to_string(&presence).unwrap();
    assert!(text.contains("\"sheetKey\":\"7\""), "{text}");
    assert!(text.contains("\"topLeft\":\"A1\""), "{text}");
}

#[test]
fn a_structural_operation_carries_the_new_sheet_map() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    let op = ClientOp {
        op_id: "op-rename".into(),
        doc: "book".into(),
        base_version: 0,
        epoch: 0,
        kind: Kind::Structural,
        target: Target { sheet: sheet.clone(), a1: None },
        payload: Payload { cmd: "sheet.rename".into(), params: json!({ "name": "Renamed" }) },
        preview: None,
    };
    let applied = room.apply(&mut s, op, "ann").unwrap();
    assert_eq!(applied.version, 1);
    assert_eq!(applied.epoch, 1);
    let sheets = applied.sheets.expect("a structural operation sends the map");
    assert_eq!(sheets.len(), 1);
    assert_eq!(sheets[0].key, sheet);
    assert_eq!(sheets[0].name, "Renamed");
}

#[test]
fn undo_reverses_the_last_step_of_its_actor_as_a_new_operation() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(5.0));
    let undone = room.undo(&mut s, "ann", Some("op-1")).unwrap();
    assert_eq!(undone.version, 2);
    assert!(undone.result.command.is_none(), "undo is not a command echo");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Empty);
    assert_eq!(room.version(), 2);
    assert!(room.undo(&mut s, "ann", None).is_err(), "one step, one undo");
}

#[test]
fn undo_is_refused_when_another_participant_holds_the_cell() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, op("op-2", "cell.set", json!({"input": 9}), &sheet, "A1"), "bob").unwrap();
    let refused = room.undo(&mut s, "ann", Some("op-1")).unwrap_err();
    assert_eq!(refused.rejected().code, RejectCode::Conflict);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(9.0));
}
