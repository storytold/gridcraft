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
    // A sort moves cells, styles, row heights and object anchors: the engine runs it as a
    // structural edit (`cx.structural`), so it may not be ordered like a value edit.
    assert_eq!(classify("data.sort"), Some(Class::Structural));
    assert_eq!(classify("data.sortAscending"), Some(Class::Structural));
    assert_eq!(classify("data.sortDescending"), Some(Class::Structural));
    assert_eq!(classify("data.validation"), Some(Class::Merge));
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
    hub.join("book", "ann", "w1", "Ann", crate::Role::Viewer).unwrap();
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
    hub.join("book", "ann", "w1", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "bob", "w1", "Bob", crate::Role::Viewer).unwrap();
    let sheet = hub.sheet_key("book", 0).unwrap();
    let applied = hub.apply("book", "ann", op("e1", "cell.set", json!({"input": 4}), &sheet, "A1")).unwrap();
    assert_eq!(applied.version, 1);
    assert_eq!(applied.actor, "ann");
    let presence = crate::Presence { sheet_key: sheet.clone(), active: "A1".into(), ranges: vec![], viewport: None };
    let all = hub.presence("book", "bob", "w1", Some(presence)).unwrap();
    let bob = all.iter().find(|p| p.actor == "bob").unwrap();
    assert_eq!(bob.presence.as_ref().unwrap().active, "A1");
    assert_eq!(all.len(), 2, "both participants are listed");
}

#[test]
fn two_windows_of_one_actor_are_two_participants() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "w1", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "ann", "w2", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "bob", "w1", "Bob", crate::Role::Viewer).unwrap();

    let all = hub.participants("book").unwrap();
    assert_eq!(all.len(), 3, "two windows of one person are two rows, plus bob");
    let ann: Vec<&crate::Participant> = all.iter().filter(|p| p.actor == "ann").collect();
    assert_eq!(ann.len(), 2);
    assert_eq!(ann[0].role, crate::Role::Editor);
    let mut windows: Vec<&str> = ann.iter().map(|p| p.window_id.as_str()).collect();
    windows.sort();
    assert_eq!(windows, ["w1", "w2"], "the rows differ by window, not by actor");

    // Presence of one window is that window's: the other window of the same person has none and
    // the document's version does not move.
    let presence = crate::Presence { sheet_key: "1".into(), active: "B2".into(), ranges: vec![], viewport: None };
    let after = hub.presence("book", "ann", "w1", Some(presence.clone())).unwrap();
    assert_eq!(after.len(), 3, "presence does not add a row");
    let first = after.iter().find(|p| p.actor == "ann" && p.window_id == "w1").unwrap();
    let second = after.iter().find(|p| p.actor == "ann" && p.window_id == "w2").unwrap();
    assert_eq!(first.presence, Some(presence));
    assert_eq!(second.presence, None, "the other window keeps its own cursor");
    assert_eq!(hub.version("book"), Some(0), "presence never moves the version");

    let left = hub.leave("book", "ann", "w1").unwrap();
    assert_eq!(left.len(), 2, "closing one window leaves the other open");
    assert!(left.iter().any(|p| p.actor == "ann" && p.window_id == "w2"));
}

#[test]
fn a_reconnecting_window_keeps_its_row_and_a_client_without_one_keeps_a_single_row() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "w1", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "bob", "w1", "Bob", crate::Role::Editor).unwrap();
    // The socket dropped and came back with the same window id: the row is the same one.
    hub.join("book", "ann", "w1", "Ann", crate::Role::Editor).unwrap();
    assert_eq!(hub.participants("book").unwrap().len(), 2, "a reconnect does not add a third row");

    // An older client sends no window id: it is one implicit window, and re-joining it replaces
    // the row instead of adding another (the behaviour before RFC §13.6).
    hub.join("book", "ann", "", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "ann", "", "Ann", crate::Role::Editor).unwrap();
    let ids: Vec<String> = hub.participants("book").unwrap().into_iter().map(|p| format!("{}/{}", p.actor, p.window_id)).collect();
    assert_eq!(ids, ["ann/", "ann/w1", "bob/w1"]);
}

#[test]
fn a_new_window_does_not_take_the_actors_undo_stack_away() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "w1", "Ann", crate::Role::Editor).unwrap();
    let sheet = hub.sheet_key("book", 0).unwrap();
    hub.apply("book", "ann", op("e1", "cell.set", json!({"input": 7}), &sheet, "A1")).unwrap();
    assert_eq!(hub.version("book"), Some(1));

    // The second and third windows of the same person join: the undo stack is the actor's, so it
    // is still there for any of them, and undo stays one operation of the document.
    hub.join("book", "ann", "w2", "Ann", crate::Role::Editor).unwrap();
    hub.join("book", "ann", "w3", "Ann", crate::Role::Editor).unwrap();
    assert_eq!(hub.participants("book").unwrap().len(), 3);
    let undone = hub.undo("book", "ann", None).unwrap();
    assert_eq!(undone.actor, "ann");
    assert_eq!(undone.version, 2, "undo is an operation of the document like any other");
    let delta = undone.result.delta.expect("the undone cell is reported");
    assert!(delta.iter().any(|d| d.cells.iter().any(|c| c.a1 == "A1")), "the step of the actor was reversed");
}

#[test]
fn commit_writes_the_authoritative_workbook_as_a_zip_package() {
    let s = session();
    let mut hub = crate::Hub::new();
    assert!(hub.open("book", s));
    hub.join("book", "ann", "w1", "Ann", crate::Role::Owner).unwrap();
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
    let (connection, welcome) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", "w1", None).unwrap();
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
    assert_eq!(gw.connect(&wrong_file, 1_000, expected(), "Ann", "w1", None).unwrap_err(), crate::AuthError::WrongFile);
    let mut expired = claims("ann", crate::Role::Editor);
    expired.exp = 999;
    assert_eq!(gw.connect(&expired, 1_000, expected(), "Ann", "w1", None).unwrap_err(), crate::AuthError::Expired);
    let mut wrong_app = claims("ann", crate::Role::Editor);
    wrong_app.app = "pdfcraft".into();
    assert_eq!(gw.connect(&wrong_app, 1_000, expected(), "Ann", "w1", None).unwrap_err(), crate::AuthError::WrongApp);
}

#[test]
fn a_viewer_gets_a_rejection_from_the_message_loop() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let (connection, welcome) = gw.connect(&claims("vic", crate::Role::Viewer), 1_000, expected(), "Vic", "w1", None).unwrap();
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
    let (ann, _) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", "w1", None).unwrap();
    let (bob, _) = gw.connect(&claims("bob", crate::Role::Viewer), 1_000, expected(), "Bob", "w1", None).unwrap();
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
fn two_connections_of_one_actor_are_two_windows_in_the_gateway() {
    let mut gw = crate::Gateway::new();
    assert!(gw.hub_mut().open("book", session()));
    let (ann_one, _) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", "w1", None).unwrap();
    let (ann_two, welcome) = gw.connect(&claims("ann", crate::Role::Editor), 1_000, expected(), "Ann", "w2", None).unwrap();
    assert_eq!(ann_one.actor, ann_two.actor, "the person is the token's subject in both windows");
    assert_ne!(ann_one.window_id, ann_two.window_id);
    match welcome {
        crate::ServerMessage::Welcome { participants, .. } => {
            assert_eq!(participants.len(), 2, "the second window sees both rows");
            assert!(participants.iter().all(|p| p.actor == "ann"));
        }
        other => panic!("expected welcome, got {other:?}"),
    }

    let presence = crate::Presence { sheet_key: "1".into(), active: "C3".into(), ranges: vec![], viewport: None };
    let messages = gw.handle(&ann_two, crate::ClientMessage::Presence { presence: Some(presence) });
    match &messages[0] {
        crate::ServerMessage::Presence { participants } => {
            assert_eq!(participants.len(), 2);
            let one = participants.iter().find(|p| p.window_id == "w1").unwrap();
            let two = participants.iter().find(|p| p.window_id == "w2").unwrap();
            assert_eq!(one.presence, None, "the first window keeps its own cursor");
            assert_eq!(two.presence.as_ref().map(|p| p.active.as_str()), Some("C3"));
        }
        other => panic!("expected presence, got {other:?}"),
    }

    // Closing the second window leaves the first one in the session.
    let left = gw.handle(&ann_two, crate::ClientMessage::Leave);
    match &left[0] {
        crate::ServerMessage::Presence { participants } => {
            assert_eq!(participants.len(), 1);
            assert_eq!(participants[0].window_id, "w1");
        }
        other => panic!("expected presence, got {other:?}"),
    }
}

#[test]
fn a_participant_row_carries_its_window_id_on_the_wire() {
    let participant =
        crate::Participant { actor: "ann".into(), window_id: "w2".into(), name: "Ann".into(), role: crate::Role::Editor, presence: None };
    let text = serde_json::to_string(&participant).unwrap();
    assert!(text.contains("\"windowId\":\"w2\""), "{text}");
    let back: crate::Participant = serde_json::from_str(&text).unwrap();
    assert_eq!(back, participant);
    // A frame from a server that does not know windows still parses.
    let old = r#"{"actor":"ann","name":"Ann","role":"editor"}"#;
    let back: crate::Participant = serde_json::from_str(old).unwrap();
    assert_eq!(back.window_id, "", "no window id is the one implicit window");
}

#[test]
fn a_hello_on_the_wire_carries_the_signed_token_not_claims() {
    let text = r#"{"type":"hello","token":"a.b.c","name":"Ann","lastVersion":3}"#;
    let message: crate::ClientMessage = serde_json::from_str(text).unwrap();
    assert!(matches!(message, crate::ClientMessage::Hello { .. }));
    if let crate::ClientMessage::Hello { token, name, last_version, window_id } = message {
        assert_eq!(token, "a.b.c");
        assert_eq!(name, "Ann");
        assert_eq!(last_version, 3);
        assert_eq!(window_id, "", "an older client sends no window id and has one implicit window");
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

#[test]
fn an_undo_does_not_wait_for_a_recalculated_dependent() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("ann-b1", "cell.set", json!({"input": "=A1+C1"}), &sheet, "B1"), "ann").unwrap();
    room.apply(&mut s, op("ann-a1", "cell.set", json!({"input": 3}), &sheet, "A1"), "ann").unwrap();
    // Bob's edit moves B1's value without touching the cell ann's step wrote.
    room.apply(&mut s, op("bob-c1", "cell.set", json!({"input": 5}), &sheet, "C1"), "bob").unwrap();
    let undone = room.undo(&mut s, "ann", Some("ann-a1")).expect("a formula the recalculation evaluated is not part of the step");
    assert_eq!(undone.version, 4);
    assert_eq!(value_of(&s, 0, "A1"), Value::Empty);
    assert_eq!(value_of(&s, 0, "B1"), Value::Number(5.0), "the recalculated dependent follows the restored cell");
}

#[test]
fn an_undo_delta_carries_the_restored_formula_and_the_dependents() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("b1", "cell.set", json!({"input": "=A1*2"}), &sheet, "B1"), "ann").unwrap();
    room.apply(&mut s, op("a1", "cell.set", json!({"input": 4}), &sheet, "A1"), "ann").unwrap();
    let undone = room.undo(&mut s, "ann", Some("a1")).unwrap();
    let delta = undone.result.delta.expect("cells changed");
    let change = |a1: &str| delta.iter().flat_map(|d| &d.cells).find(|c| c.a1 == a1).cloned();
    let a1 = change("A1").expect("A1 is in the delta");
    assert!(a1.formula.is_none(), "the cell is a constant again");
    let b1 = change("B1").expect("the recalculated dependent travels too");
    assert_eq!(b1.formula.as_deref(), Some("A1*2"), "the dependent carries its formula, not only a value");
    assert_eq!(serde_json::from_value::<Value>(b1.value.clone()).unwrap().as_f64(), Some(0.0));
}

#[test]
fn a_cell_state_puts_python_code_and_an_array_formula_back() {
    use gridcraft_core::RangeRef;
    use gridcraft_engine::CellState;

    let mut s = session();
    let at = CellRef::new(0, 0);
    let py = CellState { value: Value::Number(3.0), formula: None, style: 0, py: Some("1 + 2".into()), array: None, dynamic: false };
    s.restore_cells(0, &[(at, py)]).expect("a Python cell is restored");
    let area = RangeRef::new(at, CellRef::new(0, 1));
    let array = CellState { value: Value::Number(0.0), formula: Some("=SUM(B1:C1)".into()), style: 0, py: None, array: Some(area), dynamic: true };
    s.restore_cells(0, &[(CellRef::new(1, 0), array)]).expect("an array formula is restored");
    let doc = s.doc().unwrap().wb.clone();
    assert_eq!(doc.sheet(0).and_then(|sh| sh.cell(at)).and_then(|c| c.py.as_ref()).map(|p| p.code.clone()), Some("1 + 2".into()));
    let formula = doc.sheet(0).and_then(|sh| sh.cell(CellRef::new(1, 0))).and_then(|c| c.formula.as_ref()).cloned().expect("the formula");
    assert_eq!(formula.array, Some(area), "the array range survives the round trip");
    assert!(formula.dynamic);

    // The same two fields travel through a step of the room: the undo puts the code back.
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("set", "cell.set", json!({"input": 7}), &sheet, "A1"), "ann").unwrap();
    room.undo(&mut s, "ann", Some("set")).unwrap();
    let code = s.doc().unwrap().wb.sheet(0).and_then(|sh| sh.cell(at)).and_then(|c| c.py.as_ref()).map(|p| p.code.clone());
    assert_eq!(code, Some("1 + 2".into()), "the undo restores the Python code, not the constant the cell showed");
}

#[test]
fn redo_puts_back_what_undo_took_away_and_a_new_edit_ends_the_branch() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    room.undo(&mut s, "ann", Some("op-1")).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Empty);
    let redone = room.redo(&mut s, "ann", Some("op-1")).unwrap();
    assert_eq!(redone.version, 3);
    assert!(redone.result.command.is_none());
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(5.0));
    assert!(room.redo(&mut s, "ann", None).is_err(), "the redo stack is empty");

    room.undo(&mut s, "ann", Some("op-1")).unwrap();
    room.apply(&mut s, op("op-2", "cell.set", json!({"input": 7}), &sheet, "A1"), "ann").unwrap();
    assert!(room.redo(&mut s, "ann", None).is_err(), "a new edit ends the redo branch");
}

/// A structural operation against the current head: the tests below are about its undo, not about
/// the exclusivity rule, which has its own test.
fn structural_op(room: &Room, op_id: &str, cmd: &str, params: Json, sheet: &str) -> ClientOp {
    ClientOp {
        op_id: op_id.to_string(),
        doc: "book".into(),
        base_version: room.version(),
        epoch: room.epoch(),
        kind: Kind::Structural,
        target: Target { sheet: sheet.to_string(), a1: Some("A1".into()) },
        payload: Payload { cmd: cmd.to_string(), params },
        preview: None,
    }
}

fn value_of(s: &Session, sheet: usize, a1: &str) -> Value {
    s.doc().unwrap().wb.sheet(sheet).unwrap().value(CellRef::parse(a1).unwrap())
}

#[test]
fn a_row_insert_undoes_as_one_operation_with_the_whole_sheet_image() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 1}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, op("b", "cell.set", json!({"input": 2}), &sheet, "A2"), "ann").unwrap();
    let inserted = room.apply(&mut s, structural_op(&room, "ins", "home.insertRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    assert_eq!(value_of(&s, 0, "A1"), Value::Empty);
    assert_eq!(value_of(&s, 0, "A2"), Value::Number(1.0));

    let undone = room.undo(&mut s, "ann", Some("ins")).unwrap();
    assert_eq!(undone.version, 4);
    assert_eq!(undone.epoch, inserted.epoch + 1, "an undo that moves addresses bumps the epoch");
    assert!(undone.result.command.is_none());
    let images = undone.images.expect("the undo carries the sheets it restored");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].name, "Sheet1");
    assert!(undone.sheets.is_some(), "the client gets the sheet map of the restored workbook");
    assert_eq!(value_of(&s, 0, "A1"), Value::Number(1.0));
    assert_eq!(value_of(&s, 0, "A2"), Value::Number(2.0));
    assert_eq!(value_of(&s, 0, "A3"), Value::Empty);
}

#[test]
fn a_row_delete_undoes_with_the_cells_it_removed() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 1}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, op("b", "cell.set", json!({"input": 2}), &sheet, "A2"), "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "del", "home.deleteRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    assert_eq!(value_of(&s, 0, "A1"), Value::Number(2.0), "the row above was removed");

    room.undo(&mut s, "ann", Some("del")).unwrap();
    assert_eq!(value_of(&s, 0, "A1"), Value::Number(1.0), "the deleted content is back");
    assert_eq!(value_of(&s, 0, "A2"), Value::Number(2.0));
}

#[test]
fn a_sheet_insert_undoes_to_the_sheet_set_before_it() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    assert_eq!(s.doc().unwrap().wb.sheets[1].name, "Extra");

    let undone = room.undo(&mut s, "ann", Some("add")).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Sheet1");
    let images = undone.images.expect("the sheet image restores the one-sheet workbook");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].name, "Sheet1");
}

#[test]
fn a_sheet_delete_undoes_with_the_sheet_image_and_keeps_its_key() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let extra_key = {
        let d = s.doc().unwrap();
        let at = d.wb.sheet_index("Extra").unwrap();
        room.sheet_keys().key_for_index(&d.wb, at).unwrap()
    };
    let mut fill = op("fill", "cell.set", json!({"input": 7}), &extra_key.to_string(), "B1");
    fill.base_version = room.version();
    fill.epoch = room.epoch();
    room.apply(&mut s, fill, "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "del", "home.deleteSheet", json!({"sheet": "Extra"}), &extra_key.to_string()), "ann").unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);

    room.undo(&mut s, "ann", Some("del")).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 2);
    let at = d.wb.sheet_index("Extra").expect("the deleted sheet is back");
    assert_eq!(d.wb.sheets[at].value(CellRef::parse("B1").unwrap()), Value::Number(7.0));
    assert_eq!(room.sheet_keys().key_for_index(&d.wb, at), Some(extra_key), "the restored sheet keeps its session key");
    assert!(!room.sheet_keys().is_dead(extra_key));
}

#[test]
fn a_structural_undo_is_refused_when_another_edit_came_after_it() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 1}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "ins", "home.insertRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    let mut later = op("b", "cell.set", json!({"input": 9}), &sheet, "B2");
    later.base_version = room.version();
    later.epoch = room.epoch();
    room.apply(&mut s, later, "bob").unwrap();
    let refused = room.undo(&mut s, "ann", Some("ins")).unwrap_err();
    assert_eq!(refused.rejected().code, RejectCode::Conflict);
    assert!(refused.rejected().retryable, "the client can resync and try again");
    assert_eq!(value_of(&s, 0, "A2"), Value::Number(1.0), "the refused undo changed nothing");
    assert_eq!(value_of(&s, 0, "B2"), Value::Number(9.0));
}

#[test]
fn a_structural_redo_puts_the_edit_back_from_the_after_image() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 1}), &sheet, "A1"), "ann").unwrap();
    let inserted = room.apply(&mut s, structural_op(&room, "ins", "home.insertRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    room.undo(&mut s, "ann", Some("ins")).unwrap();
    assert_eq!(value_of(&s, 0, "A1"), Value::Number(1.0));
    let redone = room.redo(&mut s, "ann", Some("ins")).unwrap();
    assert_eq!(redone.version, inserted.version + 2);
    assert_eq!(redone.epoch, inserted.epoch + 2, "undo and redo both move addresses");
    assert_eq!(value_of(&s, 0, "A1"), Value::Empty);
    assert_eq!(value_of(&s, 0, "A2"), Value::Number(1.0));
    let images = redone.images.expect("a structural redo carries the after image too");
    assert_eq!(images.len(), 1);
}

#[test]
fn a_structural_undo_with_an_image_over_eight_megabytes_is_not_refused() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    // One cell of 9 MiB makes the stage image of every sheet larger than the old `too_large`
    // limit; the image now travels in parts instead (ADR-0013).
    room.apply(&mut s, op("big", "cell.set", json!({"input": "x".repeat(9 * 1024 * 1024)}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();

    let undone = room.undo(&mut s, "ann", Some("add")).expect("the image is no longer refused as too_large");
    let images = undone.images.clone().expect("the undo carries the sheets it restored");
    assert!(serde_json::to_vec(&images).expect("JSON").len() > 8 * 1024 * 1024);

    let frames = crate::split_applied(undone, crate::IMAGE_PART);
    assert!(frames.len() > 1, "an image over the frame limit travels in parts");
    assert!(frames.iter().all(|frame| !frame.more || frame.image_part.is_some()));
}

#[test]
fn a_sheet_image_round_trips_through_the_wire() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": "=1+1"}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "ins", "home.insertRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    let undone = room.undo(&mut s, "ann", Some("ins")).unwrap();
    let images = undone.images.clone().unwrap();
    let text = serde_json::to_string(&images).expect("the image is JSON");
    let back: Vec<crate::SheetImage> = serde_json::from_str(&text).expect("the image reads back");
    assert_eq!(back.len(), images.len());
    assert_eq!(back[0].name, images[0].name);
    let frame = serde_json::to_string(&crate::ServerMessage::Applied { applied: undone.clone() }).expect("the frame is JSON");
    let message: crate::ServerMessage = serde_json::from_str(&frame).expect("the frame reads back");
    match message {
        crate::ServerMessage::Applied { applied } => {
            assert_eq!(applied.version, undone.version);
            assert_eq!(applied.images.expect("the images survive the frame").len(), 1);
        }
        other => panic!("expected applied, got {other:?}"),
    }
}

/// The journal records an undo as an operation, so a restart that replays the tail must reach the
/// same document. The step it reversed lived in the room's memory only; its answer carries the
/// cells it put back, which is what a replay needs.
#[test]
fn a_journaled_undo_and_redo_replay_from_their_answers_after_a_restart() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    let keys = room.sheet_keys().clone();
    let base_wb = s.doc().unwrap().wb.as_ref().clone();
    let undone = room.undo(&mut s, "ann", Some("op-1")).unwrap();
    let redone = room.redo(&mut s, "ann", Some("op-1")).unwrap();

    // The restart: the base is the workbook as the edit left it, and the room starts with no
    // history at all. The journal holds the undo and the redo.
    let mut base = Session::new();
    base.add_document(gridcraft_engine::DocState::new(base_wb, None, "book".into(), gridcraft_engine::Trust::FromFile));
    let mut restored = Room::restore(keys, crate::RoomState { version: 1, epoch: 0, seen: Vec::new() });
    restored.replay_reversal(&mut base, &undone).unwrap();
    assert_eq!(restored.version(), 2);
    assert_eq!(value_of(&base, 0, "A1"), Value::Empty, "the undone cell is gone after the replay");
    restored.replay_reversal(&mut base, &redone).unwrap();
    assert_eq!(restored.version(), 3);
    assert_eq!(value_of(&base, 0, "A1"), Value::Number(5.0), "the redo puts the step back");
}

/// A structural undo replays its sheet images the same way: the sheets of the step replace the
/// workbook, and the room's epoch moves with them.
#[test]
fn a_journaled_structural_undo_replays_its_sheet_image() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 1}), &sheet, "A1"), "ann").unwrap();
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let keys = room.sheet_keys().clone();
    let base_wb = s.doc().unwrap().wb.as_ref().clone();
    let undone = room.undo(&mut s, "ann", Some("add")).unwrap();
    assert_eq!(undone.epoch, 2);

    let mut base = Session::new();
    base.add_document(gridcraft_engine::DocState::new(base_wb, None, "book".into(), gridcraft_engine::Trust::FromFile));
    let mut restored = Room::restore(keys, crate::RoomState { version: 2, epoch: 1, seen: Vec::new() });
    restored.replay_reversal(&mut base, &undone).unwrap();
    assert_eq!(base.doc().unwrap().wb.sheets.len(), 1, "the inserted sheet is undone");
    assert_eq!(value_of(&base, 0, "A1"), Value::Number(1.0));
    assert_eq!(restored.version(), 3);
    assert_eq!(restored.epoch(), 2, "the undo moved addresses");
    assert_eq!(restored.sheet_keys().key_for_index(&base.doc().unwrap().wb, 0).map(|key| key.to_string()), Some(sheet));
}

#[test]
fn a_replayed_undo_is_refused_when_the_journal_skips_a_version() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    let undone = room.undo(&mut s, "ann", Some("op-1")).unwrap();

    // A room at version 0 that is handed version 2 has lost the operation between them.
    let mut fresh = session();
    let mut empty = Room::new(&fresh);
    let refused = empty.replay_reversal(&mut fresh, &undone).expect_err("a gap is refused");
    assert_eq!(refused.rejected().code, RejectCode::Internal);
    assert_eq!(empty.version(), 0, "nothing was applied");
}

/// A room restored from the volume starts with empty undo stacks: the volume never holds a step a
/// later undo took away (ADR-0010 §6), so a stack rebuilt by replay would not be the one the
/// operations left.
#[test]
fn a_restored_room_starts_with_empty_undo_stacks() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("op-1", "cell.set", json!({"input": 5}), &sheet, "A1"), "ann").unwrap();
    room.undo(&mut s, "ann", Some("op-1")).unwrap();
    room.apply(&mut s, op("op-2", "cell.set", json!({"input": 6}), &sheet, "A2"), "ann").unwrap();
    room.forget_history();
    let refused = room.undo(&mut s, "ann", None).expect_err("the stacks are gone");
    assert_eq!(refused.rejected().code, RejectCode::Invalid);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("A2").unwrap()).as_f64(), Some(6.0));
}

/// The unmodelled parts a deleted sheet carried: what a save would lose without them.
const KEPT_PART: &[u8] = b"<controls><control id=\"1\"/></controls>";

/// The package-level content of a workbook: a chart sheet with its part and relationship, package
/// relationships and the VBA project. A structural step does not change them, but the image of a
/// step is the whole boundary: an undo that reaches a replica or a replayed journal restores them
/// with the sheets rather than leaving whatever the receiving workbook happened to hold.
const VBA: &[u8] = b"\xd0\xcf\x11\xe0 synthetic vbaProject";
const CHART_PART: &[u8] = b"<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"/>";

fn keep_workbook_parts(s: &mut Session) {
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    wb.vba_project = Some(std::sync::Arc::new(VBA.to_vec()));
    wb.preserved = Some(std::sync::Arc::new(gridcraft_model::Preserved {
        package_rels: vec![gridcraft_model::PreservedRel {
            id: "rId4".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties".into(),
            target: "docProps/app.xml".into(),
            external: false,
        }],
        rels: vec![gridcraft_model::PreservedRel {
            id: "rId9".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet".into(),
            target: "xl/chartsheets/sheet1.xml".into(),
            external: false,
        }],
        parts: vec![gridcraft_model::PreservedPart {
            name: "xl/chartsheets/sheet1.xml".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml".into(),
            data: std::sync::Arc::from(CHART_PART),
            rels: Vec::new(),
        }],
        elements: Vec::new(),
        vml: None,
        sheets: vec![gridcraft_model::PreservedSheet {
            name: "Chart1".into(),
            visibility: gridcraft_model::Visibility::Visible,
            rel_id: "rId9".into(),
            before: 1,
            sheet_id: 2,
        }],
    }));
}

fn keep_part(s: &mut Session, index: usize) {
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let Some(sheet) = wb.sheets.get_mut(index) else { return };
    std::sync::Arc::make_mut(sheet).preserved = Some(std::sync::Arc::new(gridcraft_model::Preserved {
        parts: vec![gridcraft_model::PreservedPart {
            name: "xl/ctrlProps/ctrlProps1.xml".into(),
            content_type: "application/vnd.ms-excel.controlproperties+xml".into(),
            data: std::sync::Arc::from(KEPT_PART),
            rels: Vec::new(),
        }],
        ..Default::default()
    }));
}

fn extra_key(room: &Room, s: &Session, name: &str) -> String {
    let d = s.doc().unwrap();
    let index = d.wb.sheet_index(name).expect("the sheet is there");
    room.sheet_keys().key_for_index(&d.wb, index).expect("the sheet has a key").to_string()
}

/// Deleting a sheet changes workbook-level state too: a name scoped to it is dropped or turns into
/// `#REF!`. The step image carries that state beside the sheets, so an undo on a peer or after a
/// restart restores the boundary whole — names, structure protection, and the sheet's kept parts.
#[test]
fn a_sheet_delete_image_carries_the_workbook_meta_and_the_kept_parts() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let extra = extra_key(&room, &s, "Extra");
    s.execute("formulas.defineName", json!({"name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Total", "refersTo": "=1+1"})).unwrap();
    keep_part(&mut s, 1);
    let names_before = s.doc().unwrap().wb.names.clone();

    let deleted = room.apply(&mut s, structural_op(&room, "del", "home.deleteSheet", json!({"sheet": "Extra"}), &extra), "ann").unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.doc().unwrap().wb.names.len(), 1, "the name scoped to the deleted sheet went with it");
    let after_delete = s.doc().unwrap().wb.as_ref().clone();

    let undone = room.undo(&mut s, "ann", Some("del")).unwrap();
    let images = undone.images.clone().expect("the undo carries the sheets");
    assert!(images.iter().any(|image| image.preserved.is_some()), "the image carries the kept part of the sheet");
    let book = undone.book.clone().expect("the image carries the workbook-level state");
    assert_eq!(book.names, names_before, "the names of the boundary travel with the image");
    assert_eq!(s.doc().unwrap().wb.names, names_before, "the author's replica restores the names");

    // The frame and the journal put the operation through JSON: what a peer receives and what a
    // restart reads back must still restore the sheet, its kept part and the workbook state.
    let wire = serde_json::to_vec(&undone).expect("the applied is JSON");
    let journaled: crate::Applied = serde_json::from_slice(&wire).expect("the applied reads back");

    let mut base = Session::new();
    base.add_document(gridcraft_engine::DocState::new(after_delete, None, "book".into(), gridcraft_engine::Trust::FromFile));
    let mut restored =
        Room::restore(room.sheet_keys().clone(), crate::RoomState { version: deleted.version, epoch: deleted.epoch, seen: Vec::new() });
    restored.replay_reversal(&mut base, &journaled).expect("the journaled undo replays");

    let d = base.doc().unwrap();
    assert_eq!(d.wb.names, names_before, "the replayed undo restores the names");
    let index = d.wb.sheet_index("Extra").expect("the deleted sheet is back");
    let kept = d.wb.sheets[index].preserved.as_ref().expect("the kept part of the sheet is back");
    assert_eq!(kept.parts[0].name, "xl/ctrlProps/ctrlProps1.xml");
    assert_eq!(kept.parts[0].data.as_ref(), KEPT_PART);
}

/// A row delete leaves the names alone, but the same image still carries the workbook state: a
/// boundary is restored whole or not at all.
#[test]
fn a_row_delete_undo_keeps_the_structure_protection_of_the_boundary() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    s.execute("formulas.defineName", json!({"name": "Total", "refersTo": "=4"})).unwrap();
    s.execute("review.protectWorkbook", json!({"on": true, "password": "pa55"})).unwrap();
    room.apply(&mut s, structural_op(&room, "ins", "home.insertRows", json!({"rows": "1:1"}), &sheet), "ann").unwrap();
    let undone = room.undo(&mut s, "ann", Some("ins")).unwrap();
    let book = undone.book.expect("a row step carries the boundary's workbook state");
    assert!(book.protected_structure);
    assert_eq!(book.names.len(), 1);
    assert!(s.doc().unwrap().wb.protected_structure);
}

/// A step image is the whole boundary, not just the sheets: the package-level kept content (a
/// chart sheet, its part and the package relationships), the VBA project, the style table and the
/// calculation settings travel with it. A replica or a replayed journal that applies the image ends
/// with exactly the boundary's values, and a base that lacks them gets them back.
#[test]
fn a_sheet_delete_image_carries_the_package_parts_vba_styles_and_calc() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let extra = extra_key(&room, &s, "Extra");
    s.execute("formulas.defineName", json!({"name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra"})).unwrap();
    keep_workbook_parts(&mut s);
    // A style the boundary uses and calculation settings that are not the defaults.
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
    s.execute("formulas.calculationOptions", json!({"iterative": true, "maxIterations": 7, "maxChange": 0.25})).unwrap();
    let boundary = s.doc().unwrap().wb.workbook_meta();
    assert!(boundary.preserved.is_some(), "the boundary has package content");
    assert!(boundary.vba_project.is_some(), "the boundary has a VBA project");
    assert!(boundary.styles.as_ref().is_some_and(|styles| styles.len() > 1), "the boundary has a non-default style");
    assert!(boundary.calc.as_ref().is_some_and(|calc| calc.iterative));

    let deleted = room.apply(&mut s, structural_op(&room, "del", "home.deleteSheet", json!({"sheet": "Extra"}), &extra), "ann").unwrap();
    let after_delete = s.doc().unwrap().wb.as_ref().clone();
    let undone = room.undo(&mut s, "ann", Some("del")).unwrap();
    assert_eq!(undone.book.as_deref(), Some(&boundary), "the undo carries the whole boundary");
    assert_eq!(s.doc().unwrap().wb.workbook_meta(), boundary, "the author restores the whole boundary");

    // The wire and the journal put the operation through JSON: the fields survive the round trip.
    let wire = serde_json::to_vec(&undone).expect("the applied is JSON");
    let journaled: crate::Applied = serde_json::from_slice(&wire).expect("the applied reads back");
    assert_eq!(journaled.book, undone.book, "the journal entry carries the package state, VBA, styles and calc");

    // A base that lacks the package state (the workbook before the boundary, or a peer that never
    // had it) gets it from the answer alone.
    let mut stripped = after_delete;
    stripped.preserved = None;
    stripped.vba_project = None;
    stripped.styles = gridcraft_model::StyleTable::default();
    stripped.calc = gridcraft_model::CalcSettings::default();
    let mut base = Session::new();
    base.add_document(gridcraft_engine::DocState::new(stripped, None, "book".into(), gridcraft_engine::Trust::FromFile));
    let mut restored =
        Room::restore(room.sheet_keys().clone(), crate::RoomState { version: deleted.version, epoch: deleted.epoch, seen: Vec::new() });
    restored.replay_reversal(&mut base, &journaled).expect("the journaled undo replays the whole boundary");

    let d = base.doc().unwrap();
    assert_eq!(d.wb.workbook_meta(), boundary, "the replayed undo restores the package state, VBA, styles and calc");
    let kept = d.wb.preserved.as_ref().expect("the package content is back");
    assert_eq!(kept.sheets.len(), 1, "the chart sheet is back");
    assert_eq!(kept.sheets[0].name, "Chart1");
    assert_eq!(kept.sheets[0].before, 1);
    assert_eq!(kept.parts[0].data.as_ref(), CHART_PART);
    assert_eq!(d.wb.vba_project.as_deref().map(Vec::as_slice), Some(VBA));
    assert!(d.wb.calc.iterative, "the calculation settings of the boundary are back");
    assert_eq!(d.wb.calc.max_iterations, 7);
}

/// An image over the operation limit is refused as `too_large` before the document changes and
/// before the version moves: the reversal is atomic or it does not happen.
#[test]
fn an_image_over_the_operation_limit_is_refused_before_the_document_changes() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let version = room.version();
    let epoch = room.epoch();

    // A test bound well under any real image: the room refuses instead of sending it.
    room.set_max_image(1);
    let refused = room.undo(&mut s, "ann", Some("add")).expect_err("the image is over the limit");
    assert_eq!(refused.rejected().code, RejectCode::TooLarge);
    assert!(!refused.rejected().retryable, "sending the same step again cannot make it smaller");
    assert_eq!(room.version(), version, "a refused reversal takes no version");
    assert_eq!(room.epoch(), epoch, "and moves no addresses");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2, "the inserted sheet is still there");
    assert!(s.doc().unwrap().wb.sheet_index("Extra").is_some());
}

/// A foreign edit after a structural step still refuses the undo as `conflict`, and nothing of the
/// image — sheets, names, kept parts — lands on the document.
#[test]
fn a_sheet_delete_undo_after_a_foreign_edit_changes_nothing() {
    let mut s = session();
    let mut room = Room::new(&s);
    let sheet = key_of(&room, &s);
    room.apply(&mut s, structural_op(&room, "add", "home.insertSheet", json!({"name": "Extra"}), &sheet), "ann").unwrap();
    let extra = extra_key(&room, &s, "Extra");
    s.execute("formulas.defineName", json!({"name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra"})).unwrap();
    keep_part(&mut s, 1);
    keep_workbook_parts(&mut s);
    room.apply(&mut s, structural_op(&room, "del", "home.deleteSheet", json!({"sheet": "Extra"}), &extra), "ann").unwrap();
    let names_after_delete = s.doc().unwrap().wb.names.clone();
    let preserved_after_delete = s.doc().unwrap().wb.preserved.clone();
    let vba_after_delete = s.doc().unwrap().wb.vba_project.clone();

    // Somebody edits the remaining sheet after the step: the room is no longer at its version.
    let mut later = op("later", "cell.set", json!({"input": 9}), &sheet, "B2");
    later.base_version = room.version();
    later.epoch = room.epoch();
    room.apply(&mut s, later, "bob").unwrap();

    let refused = room.undo(&mut s, "ann", Some("del")).expect_err("a later edit keeps the step from being reversed");
    assert_eq!(refused.rejected().code, RejectCode::Conflict);
    assert!(refused.rejected().retryable);
    let d = s.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 1, "the deleted sheet is not back");
    assert_eq!(d.wb.names, names_after_delete, "the names are untouched");
    assert_eq!(d.wb.preserved, preserved_after_delete, "the package content is untouched");
    assert_eq!(d.wb.vba_project, vba_after_delete, "the VBA project is untouched");
    assert_eq!(value_of(&s, 0, "B2"), Value::Number(9.0));
}
