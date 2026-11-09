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
