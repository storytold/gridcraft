//! Two replicas against a real server: merge commands converge, formulas follow, a structural
//! command lands on both, undo reverses only its actor's step, and a viewer is refused.

mod support;

use std::time::Duration;

use gridcraft_collab::Role;
use gridcraft_collab_client::{Client, Config, EditError, Synced};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

struct Peer {
    session: Session,
    synced: Synced,
}

async fn join(server: &support::Server, file_id: i64, session: &str, who: &str, role: Role) -> Peer {
    let cfg = Config {
        url: server.url(session),
        token: support::token(who, role, file_id, session),
        name: who.to_string(),
        last_version: 0,
        reconnect: Some(Duration::from_millis(200)),
    };
    let (client, welcome) = Client::connect(cfg).await.unwrap();
    let mut local = Session::new();
    local.new_workbook();
    let synced = Synced::new(client, &welcome);
    Peer { session: local, synced }
}

fn value(peer: &Peer, a1: &str) -> Value {
    let at = CellRef::parse(a1).unwrap();
    peer.session.doc().unwrap().wb.active().unwrap().value(at)
}

fn sheet_name(peer: &Peer) -> String {
    peer.session.doc().unwrap().wb.active().unwrap().name.clone()
}

/// Lets both connections deliver what the server sent. The two quiet rounds absorb the server's
/// processing between a send and the broadcast.
async fn settle(a: &mut Peer, b: &mut Peer) {
    for _ in 0..200 {
        let busy = !a.synced.poll(&mut a.session).is_empty() | !b.synced.poll(&mut b.session).is_empty();
        if busy {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        if a.synced.poll(&mut a.session).is_empty() && b.synced.poll(&mut b.session).is_empty() {
            return;
        }
    }
    panic!("the replicas did not settle");
}

#[tokio::test]
async fn two_replicas_converge_on_cells_and_formulas() {
    let server = support::start().await;
    let session = support::create(&server, 41).await;
    let mut ann = join(&server, 41, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 41, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 42 })).unwrap();
    assert_eq!(value(&ann, "A1"), Value::Number(42.0), "the preview is immediate");
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&bob, "A1"), Value::Number(42.0));
    assert_eq!(ann.synced.state().version, 1);
    assert_eq!(bob.synced.state().version, 1);

    bob.synced.execute(&mut bob.session, "cell.set", json!({ "cell": "B1", "input": "=A1*2" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "B1"), Value::Number(84.0));
    assert_eq!(value(&bob, "B1"), Value::Number(84.0));

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 10 })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "B1"), Value::Number(20.0), "the dependent formula follows");
    assert_eq!(value(&bob, "B1"), Value::Number(20.0));
    assert_eq!(ann.synced.state().version, 3);
    assert_eq!(bob.synced.state().version, 3);
    server.stop().await;
}

#[tokio::test]
async fn a_structural_command_lands_on_both_replicas() {
    let server = support::start().await;
    let session = support::create(&server, 42).await;
    let mut ann = join(&server, 42, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 42, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "sheet.rename", json!({ "name": "Renamed" })).unwrap();
    assert_eq!(sheet_name(&ann), "Sheet1", "a structural edit is not previewed");
    settle(&mut ann, &mut bob).await;
    assert_eq!(sheet_name(&ann), "Renamed");
    assert_eq!(sheet_name(&bob), "Renamed");
    assert_eq!(ann.synced.state().epoch, 1, "a structural edit bumps the epoch");
    server.stop().await;
}

#[tokio::test]
async fn a_viewer_is_refused_locally_and_still_receives() {
    let server = support::start().await;
    let session = support::create(&server, 43).await;
    let mut ann = join(&server, 43, &session, "ann", Role::Editor).await;
    let mut vic = join(&server, 43, &session, "vic", Role::Viewer).await;

    let refused = vic.synced.execute(&mut vic.session, "cell.set", json!({ "cell": "A1", "input": 1 }));
    assert_eq!(refused.unwrap_err(), EditError::Viewer);
    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 7 })).unwrap();
    settle(&mut ann, &mut vic).await;
    assert_eq!(value(&vic, "A1"), Value::Number(7.0));
    server.stop().await;
}

#[tokio::test]
async fn undo_reverses_only_the_actors_own_step() {
    let server = support::start().await;
    let session = support::create(&server, 44).await;
    let mut ann = join(&server, 44, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 44, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 5 })).unwrap();
    settle(&mut ann, &mut bob).await;
    bob.synced.execute(&mut bob.session, "cell.set", json!({ "cell": "B2", "input": 9 })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "A1"), Value::Number(5.0));

    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "A1"), Value::Empty);
    assert_eq!(value(&bob, "A1"), Value::Empty);
    assert_eq!(value(&bob, "B2"), Value::Number(9.0), "bob's edit stays");
    assert_eq!(ann.synced.state().version, 3);

    // Nothing left to undo: the room refuses, the replica does not change.
    assert!(ann.synced.undo().unwrap_err().to_string().contains("nothing to undo"));
    server.stop().await;
}

#[tokio::test]
async fn redo_puts_the_step_back_on_every_replica() {
    let server = support::start().await;
    let session = support::create(&server, 45).await;
    let mut ann = join(&server, 45, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 45, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 5 })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "A1"), Value::Empty);
    ann.synced.redo().unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&ann, "A1"), Value::Number(5.0));
    assert_eq!(value(&bob, "A1"), Value::Number(5.0));
    assert_eq!(ann.synced.state().version, 3);
    server.stop().await;
}

fn names(peer: &Peer) -> Vec<String> {
    peer.session.doc().unwrap().wb.sheets.iter().map(|sheet| sheet.name.clone()).collect()
}

fn value_in(peer: &Peer, sheet: &str, a1: &str) -> Value {
    let d = peer.session.doc().unwrap();
    let index = d.wb.sheet_index(sheet).unwrap_or_else(|| panic!("no sheet {sheet}"));
    d.wb.sheets[index].value(CellRef::parse(a1).unwrap())
}

/// A structural undo is one operation with the sheets as they were: rows move back and a deleted
/// sheet comes back, on the author's replica and on the peer's alike.
#[tokio::test]
async fn structural_undo_restores_rows_and_sheets_on_every_replica() {
    let server = support::start().await;
    let session = support::create(&server, 46).await;
    let mut ann = join(&server, 46, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 46, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 1 })).unwrap();
    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A2", "input": 2 })).unwrap();
    settle(&mut ann, &mut bob).await;

    // A row insert is structural: it is not previewed and moves the cells down.
    ann.synced.execute(&mut ann.session, "home.insertRows", json!({ "rows": "1:1" })).unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(value(peer, "A1"), Value::Empty);
        assert_eq!(value(peer, "A2"), Value::Number(1.0));
        assert_eq!(value(peer, "A3"), Value::Number(2.0));
        assert_eq!(peer.synced.state().epoch, 1);
    }

    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(value(peer, "A1"), Value::Number(1.0), "the row edit is undone from the sheet image");
        assert_eq!(value(peer, "A2"), Value::Number(2.0));
        assert_eq!(value(peer, "A3"), Value::Empty);
        assert_eq!(peer.synced.state().epoch, 2, "the undo moves addresses too");
    }

    ann.synced.redo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(value(peer, "A1"), Value::Empty);
        assert_eq!(value(peer, "A2"), Value::Number(1.0));
        assert_eq!(peer.synced.state().epoch, 3);
    }
    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value(&bob, "A1"), Value::Number(1.0));

    // Sheet insert and delete: the set of sheets is part of the same operation.
    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(names(&ann), ["Sheet1", "Extra"]);
    assert_eq!(names(&bob), ["Sheet1", "Extra"]);
    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(names(&ann), ["Sheet1"]);
    assert_eq!(names(&bob), ["Sheet1"]);

    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "B1", "input": 7 })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(value_in(&bob, "Extra", "B1"), Value::Number(7.0));
    ann.synced.execute(&mut ann.session, "home.deleteSheet", json!({ "sheet": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(names(&bob), ["Sheet1"]);

    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(names(peer), ["Sheet1", "Extra"], "the deleted sheet comes back");
        assert_eq!(value_in(peer, "Extra", "B1"), Value::Number(7.0), "with the cells it held");
        assert_eq!(peer.synced.state().epoch, 9);
    }
    server.stop().await;
}
