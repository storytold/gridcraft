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
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    let applied = room.apply(&mut s, op("op-1", "cell.set", json!({"input": 2}), &sheet, "A1")).unwrap();
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
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    let first = room.apply(&mut s, op("same", "cell.set", json!({"input": 1}), &sheet, "A1")).unwrap();
    let again = room.apply(&mut s, op("same", "cell.set", json!({"input": 9}), &sheet, "A1")).unwrap();
    assert_eq!(first, again);
    assert_eq!(room.version(), 1);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap()).as_f64(), Some(1.0));
}

#[test]
fn a_dependent_formula_value_travels_with_the_delta() {
    let mut s = session();
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("a", "cell.set", json!({"input": 2}), &sheet, "A1")).unwrap();
    room.apply(&mut s, op("b", "cell.set", json!({"input": "=A1*3"}), &sheet, "B2")).unwrap();
    let applied = room.apply(&mut s, op("c", "cell.set", json!({"input": 5}), &sheet, "A1")).unwrap();
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
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    room.apply(&mut s, op("one", "cell.set", json!({"input": 1}), &sheet, "A1")).unwrap();
    let mut structural = op("two", "home.insertRows", json!({"rows": "2"}), &sheet, "A2");
    structural.kind = Kind::Structural;
    structural.base_version = 0;
    let refused = room.apply(&mut s, structural).expect_err("stale base is refused");
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
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    let mut structural = op("ins", "home.insertRows", json!({"rows": "2", "count": 1}), &sheet, "A2");
    structural.kind = Kind::Structural;
    structural.base_version = 0;
    let applied = room.apply(&mut s, structural).unwrap();
    assert_eq!(applied.version, 1);
    assert_eq!(applied.epoch, 1);
}

#[test]
fn commands_outside_the_allowlist_are_refused() {
    let mut s = session();
    let mut room = Room::new(&s, "alice");
    let sheet = key_of(&room, &s);
    for (id, cmd) in [("f", "file.save"), ("u", "edit.undo"), ("m", "macro.run")] {
        let refused = room.apply(&mut s, op(id, cmd, json!({}), &sheet, "A1")).expect_err("refused");
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
    let mut room = Room::new(&s, "alice");
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
