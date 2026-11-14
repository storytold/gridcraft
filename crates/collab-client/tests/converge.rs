//! Two replicas against a real server: merge commands converge, formulas follow, a structural
//! command lands on both, undo reverses only its actor's step, and a viewer is refused.

mod support;

use std::time::Duration;

use gridcraft_collab::{Presence, Role};
use gridcraft_collab_client::{Client, Config, EditError, Event, Synced};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

struct Peer {
    session: Session,
    synced: Synced,
}

async fn join(server: &support::Server, file_id: i64, session: &str, who: &str, role: Role) -> Peer {
    let mut local = Session::new();
    local.new_workbook();
    reconnect(server, file_id, session, who, role, 0, local).await
}

/// Joins with the same package state the session was opened from: a real client opens the file it
/// has locally and then talks to the room, so both replicas start from the same VBA project, chart
/// sheet, style table and calculation settings.
async fn join_with(server: &support::Server, file_id: i64, session: &str, who: &str, role: Role, bytes: &[u8]) -> Peer {
    let (workbook, _warnings) = gridcraft_engine::io::open_bytes("plan.xlsm", bytes).expect("the test workbook opens");
    let mut local = Session::new();
    local.add_document(gridcraft_engine::DocState::from_saved(workbook, None, "plan.xlsm".into(), gridcraft_engine::Trust::FromFile));
    reconnect(server, file_id, session, who, role, 0, local).await
}

/// Comes back as `who` at `last_version`, keeping the local document the peer already had: what a
/// client does after a dropped connection (`Config::last_version` is the catch-up point).
async fn reconnect(server: &support::Server, file_id: i64, session: &str, who: &str, role: Role, last_version: u64, local: Session) -> Peer {
    reconnect_window(server, file_id, session, who, role, last_version, String::new(), local).await
}

/// Joins as one named window of `who` (RFC §13.6): two windows of one person share the actor and
/// differ in the window id.
async fn join_window(server: &support::Server, file_id: i64, session: &str, who: &str, role: Role, window_id: &str) -> Peer {
    let mut local = Session::new();
    local.new_workbook();
    reconnect_window(server, file_id, session, who, role, 0, window_id.to_string(), local).await
}

#[allow(clippy::too_many_arguments)]
async fn reconnect_window(
    server: &support::Server,
    file_id: i64,
    session: &str,
    who: &str,
    role: Role,
    last_version: u64,
    window_id: String,
    local: Session,
) -> Peer {
    let cfg = Config {
        url: server.url(session),
        token: support::token(who, role, file_id, session),
        name: who.to_string(),
        last_version,
        reconnect: Some(Duration::from_millis(200)),
        window_id,
    };
    let (client, welcome) = Client::connect(cfg).await.unwrap();
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

/// Lets one connection deliver what the server sent, while nobody else is listening.
async fn settle_one(peer: &mut Peer) {
    for _ in 0..200 {
        if !peer.synced.poll(&mut peer.session).is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        if peer.synced.poll(&mut peer.session).is_empty() {
            return;
        }
    }
    panic!("the replica did not settle");
}

/// Polls both connections until `ready` holds, so a test waits for the state it asserts on
/// instead of for a number of quiet rounds.
async fn wait_until(a: &mut Peer, b: &mut Peer, label: &str, mut ready: impl FnMut(&Peer, &Peer) -> bool) {
    for _ in 0..300 {
        a.synced.poll(&mut a.session);
        b.synced.poll(&mut b.session);
        if ready(a, b) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "the replicas did not reach the expected state ({label}): a={:?} b={:?}",
        a.synced
            .state()
            .participants
            .iter()
            .map(|p| format!("{}/{:?}", p.window_id, p.presence.as_ref().map(|x| x.active.clone())))
            .collect::<Vec<_>>(),
        b.synced
            .state()
            .participants
            .iter()
            .map(|p| format!("{}/{:?}", p.window_id, p.presence.as_ref().map(|x| x.active.clone())))
            .collect::<Vec<_>>()
    );
}

/// Lets three connections deliver what the server sent (a third window joins two others).
async fn settle_three(a: &mut Peer, b: &mut Peer, c: &mut Peer) {
    for _ in 0..200 {
        let busy = !a.synced.poll(&mut a.session).is_empty() | !b.synced.poll(&mut b.session).is_empty() | !c.synced.poll(&mut c.session).is_empty();
        if busy {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        if a.synced.poll(&mut a.session).is_empty() && b.synced.poll(&mut b.session).is_empty() && c.synced.poll(&mut c.session).is_empty() {
            return;
        }
    }
    panic!("the replicas did not settle");
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

/// Two windows of one person (RFC §13.6): the same actor id, different window ids, two rows in the
/// participant list that each window sees, presence per window, undo still on the actor.
#[tokio::test]
async fn two_windows_of_one_actor_are_two_rows_and_share_the_undo_stack() {
    let server = support::start().await;
    let session = support::create(&server, 55).await;
    let mut one = join_window(&server, 55, &session, "ann", Role::Editor, "w1").await;
    let mut two = join_window(&server, 55, &session, "ann", Role::Editor, "w2").await;

    let rows = |peer: &Peer| {
        let mut windows: Vec<String> = peer.synced.state().participants.iter().filter(|p| p.actor == "ann").map(|p| p.window_id.clone()).collect();
        windows.sort();
        windows
    };
    wait_until(&mut one, &mut two, "two rows", |one, two| rows(one).len() == 2 && rows(two).len() == 2).await;
    assert_eq!(rows(&one), ["w1", "w2"], "the first window sees both rows");
    assert_eq!(rows(&two), ["w1", "w2"], "the second window sees both rows too");

    // Presence is per window: the two cursors coexist, and neither moves the version.
    let sheet = one.synced.key_for_name(&sheet_name(&one)).expect("the sheet key");
    one.synced.send_presence(Presence { sheet_key: sheet.clone(), active: "A1".into(), ranges: vec![], viewport: None });
    two.synced.send_presence(Presence { sheet_key: sheet.clone(), active: "B2".into(), ranges: vec![], viewport: None });
    let cursor = |peer: &Peer, window: &str| {
        peer.synced.state().participants.iter().find(|p| p.window_id == window).and_then(|p| p.presence.clone()).map(|presence| presence.active)
    };
    wait_until(&mut one, &mut two, "two cursors", |one, two| {
        cursor(one, "w1").as_deref() == Some("A1")
            && cursor(one, "w2").as_deref() == Some("B2")
            && cursor(two, "w1").as_deref() == Some("A1")
            && cursor(two, "w2").as_deref() == Some("B2")
    })
    .await;
    assert_eq!(one.synced.state().version, 0, "presence is not an operation");

    // The step of one window is the person's step: the other window can undo it, because the
    // room's stack belongs to the actor and a new window does not take it away.
    one.synced.execute(&mut one.session, "cell.set", json!({ "cell": "D1", "input": 9 })).unwrap();
    settle(&mut one, &mut two).await;
    assert_eq!(value(&two, "D1"), Value::Number(9.0));
    two.synced.undo().unwrap();
    settle(&mut one, &mut two).await;
    assert_eq!(value(&one, "D1"), Value::Empty, "the actor's step is undone from the other window");

    // A third window joins: it is a third row and nothing of the first two is lost.
    let mut three = join_window(&server, 55, &session, "ann", Role::Editor, "w3").await;
    settle_three(&mut one, &mut two, &mut three).await;
    let mut windows: Vec<String> = three.synced.state().participants.iter().filter(|p| p.actor == "ann").map(|p| p.window_id.clone()).collect();
    windows.sort();
    assert_eq!(windows, ["w1", "w2", "w3"]);
    one.synced.redo().unwrap();
    settle(&mut one, &mut two).await;
    assert_eq!(value(&one, "D1"), Value::Number(9.0), "the first window still has the step to redo");
    server.stop().await;
}

/// A window that reconnects keeps its id, so its row is the same one: the participant list does
/// not grow a third entry, and the other window sees it as before (RFC §13.6).
#[tokio::test]
async fn a_reconnecting_window_keeps_its_row() {
    let server = support::start().await;
    let session = support::create(&server, 56).await;
    let mut one = join_window(&server, 56, &session, "ann", Role::Editor, "w1").await;
    let mut two = join_window(&server, 56, &session, "ann", Role::Editor, "w2").await;
    let rows = |peer: &Peer| peer.synced.state().participants.iter().filter(|p| p.actor == "ann").map(|p| p.window_id.clone()).collect::<Vec<_>>();
    wait_until(&mut one, &mut two, "two rows", |one, two| rows(one).len() == 2 && rows(two).len() == 2).await;

    // The second window drops and comes back with the same window id, as the client does after a
    // socket failure.
    let local = std::mem::replace(&mut two.session, Session::new());
    let away_at = two.synced.state().version;
    drop(two);
    let mut back = reconnect_window(&server, 56, &session, "ann", Role::Editor, away_at, "w2".into(), local).await;
    wait_until(&mut one, &mut back, "reconnect rows", |one, back| rows(one).len() == 2 && rows(back).len() == 2).await;
    // Let any late frames from the closed socket land, then count again: the row is still one row
    // per window, never a third one.
    settle(&mut one, &mut back).await;
    assert_eq!(one.synced.state().participants.len(), 2, "a reconnect does not leave a stale row");
    assert_eq!(back.synced.state().participants.len(), 2);
    let on_wire = back.synced.state().participants.iter().filter(|p| p.actor == "ann").map(|p| p.window_id.clone()).collect::<Vec<_>>();
    assert_eq!(on_wire, ["w1", "w2"], "the reconnected window is still w2");
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
/// Lets both connections deliver what the server sent, up to `version`, and keeps the events.
///
/// Unlike `settle`, this waits for a version instead of a quiet window and never drops a frame:
/// a caller that asserts on the events of one operation must not lose them to an earlier wait,
/// and a large operation is many frames whose arrival may be spread over more than a quiet
/// window (`an_eight_megabyte_sheet_image_travels_in_parts_and_converges` failed exactly that
/// way: `settle` had already consumed the author's applied frames, so the collection saw none).
/// The quiet round after the version catches the frames of the same operation that trail behind
/// the last part, so `applied` counts every operation once.
async fn settle_events(a: &mut Peer, b: &mut Peer, version: u64) -> (Vec<Event>, Vec<Event>) {
    let mut from_a = Vec::new();
    let mut from_b = Vec::new();
    let mut quiet = false;
    for _ in 0..1200 {
        let mut events_a = a.synced.poll(&mut a.session);
        let mut events_b = b.synced.poll(&mut b.session);
        let busy = !events_a.is_empty() || !events_b.is_empty();
        from_a.append(&mut events_a);
        from_b.append(&mut events_b);
        if busy {
            quiet = false;
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        }
        let at_version = a.synced.state().version >= version && b.synced.state().version >= version;
        if at_version && quiet {
            return (from_a, from_b);
        }
        if at_version {
            quiet = true;
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    panic!("the replicas did not reach version {version}: a={} b={}", a.synced.state().version, b.synced.state().version);
}

/// The applied frames of the collected events, in order.
fn applied(events: &[Event]) -> Vec<&gridcraft_collab::Applied> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Applied(applied) => Some(applied),
            _ => None,
        })
        .collect()
}

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

/// The image of a structural step can be over the 8 MiB that used to be refused as `too_large`.
/// It travels in parts (ADR-0013) and every replica applies the operation once, when the last
/// part is in, whatever its size.
#[tokio::test]
async fn an_eight_megabyte_sheet_image_travels_in_parts_and_converges() {
    let server = support::start().await;
    let session = support::create(&server, 47).await;
    let mut ann = join(&server, 47, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 47, &session, "bob", Role::Editor).await;

    // Twelve cells of 700 KiB: the whole-sheet image of the step below is over 8 MiB.
    let big = "x".repeat(700 * 1024);
    for row in 1..=12 {
        ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": format!("A{row}"), "input": big.clone() })).unwrap();
    }
    let (ann_setup, bob_setup) = settle_events(&mut ann, &mut bob, 12).await;
    let rejected: Vec<String> = ann_setup
        .iter()
        .chain(bob_setup.iter())
        .filter_map(|event| match event {
            Event::Rejected(rejected) => Some(format!("{}:{:?}:{:?}", rejected.op_id, rejected.code, rejected.message)),
            _ => None,
        })
        .collect();
    assert_eq!(applied(&ann_setup).len(), 12, "one applied per edit on the author; rejected {rejected:?}");
    assert_eq!(applied(&bob_setup).len(), 12, "one applied per edit on the peer; rejected {rejected:?}");
    match value(&bob, "A1") {
        Value::Text(text) => assert_eq!(text.len(), big.len()),
        other => panic!("the peer did not get the big value: {other:?}; rejected {rejected:?}"),
    }

    // A sheet insert is structural: its undo carries Sheet1 whole, in parts.
    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle_events(&mut ann, &mut bob, 13).await;
    assert_eq!(names(&bob), ["Sheet1", "Extra"]);

    ann.synced.undo().unwrap();
    let (ann_events, bob_events) = settle_events(&mut ann, &mut bob, 14).await;
    for events in [&ann_events, &bob_events] {
        let applied = applied(events);
        assert_eq!(applied.len(), 1, "the parts arrive as one operation, not as document changes");
        let undo = applied[0];
        assert!(undo.op_id.starts_with("undo-"), "the answer is the undo, got {}", undo.op_id);
        assert!(!undo.more, "the caller only sees the completed operation");
        assert!(undo.image_part.is_none());
        let images = undo.images.as_ref().expect("the undo carries the sheet image");
        let size = serde_json::to_vec(images).expect("JSON").len();
        assert!(size > 8 * 1024 * 1024, "the image is {size} bytes, not over the old limit");
    }
    for peer in [&ann, &bob] {
        assert_eq!(names(peer), ["Sheet1"], "the inserted sheet is undone on every replica");
        match value(peer, "A1") {
            Value::Text(text) => assert_eq!(text.len(), big.len(), "the big cell comes back whole"),
            other => panic!("the restored sheet lost its cell: {other:?}"),
        }
        assert_eq!(peer.synced.state().version, 14, "twelve edits, the insert and the undo");
        assert_eq!(peer.synced.state().epoch, 2, "the undo moves addresses");
    }

    ann.synced.redo().unwrap();
    let (ann_events, bob_events) = settle_events(&mut ann, &mut bob, 15).await;
    for events in [&ann_events, &bob_events] {
        let applied = applied(events);
        assert_eq!(applied.len(), 1);
        assert!(applied[0].op_id.starts_with("redo-"));
        let images = applied[0].images.as_ref().expect("the redo carries the after image");
        assert!(serde_json::to_vec(images).expect("JSON").len() > 8 * 1024 * 1024);
    }
    for peer in [&ann, &bob] {
        assert_eq!(names(peer), ["Sheet1", "Extra"]);
        assert_eq!(peer.synced.state().epoch, 3);
    }
    server.stop().await;
}

/// An undo is a version of its own, so a peer that reconnects between it and the next snapshot has
/// to be caught up by it: the journal carries the undo like any other operation. Before that, the
/// catch-up said "caught up" while the undone cell was still in the reconnecting replica.
#[tokio::test]
async fn a_peer_that_reconnects_after_an_undo_sees_the_undone_state() {
    let server = support::start().await;
    let session = support::create(&server, 48).await;
    let mut ann = join(&server, 48, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 48, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 5 })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(bob.synced.state().version, 1);
    assert_eq!(value(&bob, "A1"), Value::Number(5.0));

    // Bob's connection goes away at version 1; the author undoes while he is away.
    let local = std::mem::replace(&mut bob.session, Session::new());
    let away_at = bob.synced.state().version;
    drop(bob);
    ann.synced.undo().unwrap();
    settle_one(&mut ann).await;
    assert_eq!(ann.synced.state().version, 2);
    assert_eq!(value(&ann, "A1"), Value::Empty);

    let mut back = reconnect(&server, 48, &session, "bob", Role::Editor, away_at, local).await;
    settle(&mut ann, &mut back).await;
    assert_eq!(value(&back, "A1"), Value::Empty, "the catch-up carries the undo");
    assert_eq!(value(&ann, "A1"), Value::Empty, "both replicas see the undone step");
    assert_eq!(back.synced.state().version, 2);

    // The redo after the reconnect puts the step back on both.
    ann.synced.redo().unwrap();
    settle(&mut ann, &mut back).await;
    assert_eq!(value(&ann, "A1"), Value::Number(5.0));
    assert_eq!(value(&back, "A1"), Value::Number(5.0));
    assert_eq!(back.synced.state().version, 3);
    server.stop().await;
}

/// A structural undo is journaled with the sheets it put back, so the same catch-up works when the
/// reconnecting replica has to replace whole sheets rather than patch cells.
#[tokio::test]
async fn a_peer_that_reconnects_after_a_structural_undo_gets_the_sheet_image() {
    let server = support::start().await;
    let session = support::create(&server, 49).await;
    let mut ann = join(&server, 49, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 49, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": 1 })).unwrap();
    settle(&mut ann, &mut bob).await;
    // A structural edit waits for the head, so it goes out after the first answer.
    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(names(&bob), ["Sheet1", "Extra"]);
    let away_at = bob.synced.state().version;

    let local = std::mem::replace(&mut bob.session, Session::new());
    drop(bob);
    ann.synced.undo().unwrap();
    settle_one(&mut ann).await;
    assert_eq!(names(&ann), ["Sheet1"]);

    let mut back = reconnect(&server, 49, &session, "bob", Role::Editor, away_at, local).await;
    settle(&mut ann, &mut back).await;
    assert_eq!(names(&back), ["Sheet1"], "the inserted sheet is undone on the reconnecting replica");
    assert_eq!(value(&back, "A1"), Value::Number(1.0));
    assert_eq!(back.synced.state().version, 3);
    server.stop().await;
}

fn defined_names(peer: &Peer) -> Vec<(String, Option<usize>, String)> {
    peer.session.doc().unwrap().wb.names.iter().map(|name| (name.name.clone(), name.scope, name.formula.clone())).collect()
}

fn protected(peer: &Peer) -> bool {
    peer.session.doc().unwrap().wb.protected_structure
}

/// Deleting a sheet also changes the workbook-level state of the file: a name scoped to it is
/// dropped and a name referring to it turns into `#REF!`. The structural image carries that state,
/// so the author and the peer restore the same names on undo and drop them again on redo.
#[tokio::test]
async fn structural_undo_restores_the_workbook_metadata_on_every_replica() {
    let server = support::start().await;
    let session = support::create(&server, 50).await;
    let mut ann = join(&server, 50, &session, "ann", Role::Editor).await;
    let mut bob = join(&server, 50, &session, "bob", Role::Editor).await;

    ann.synced.execute(&mut ann.session, "formulas.defineName", json!({ "name": "Total", "refersTo": "=1+1" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(defined_names(&bob), [("Total".to_string(), None, "1+1".to_string())], "the peer gets the name before any structural step");

    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.execute(&mut ann.session, "formulas.defineName", json!({ "name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    let before = defined_names(&ann);
    assert_eq!(defined_names(&bob), before, "both replicas start the step with the same names");
    assert_eq!(before.len(), 2);

    ann.synced.execute(&mut ann.session, "home.deleteSheet", json!({ "sheet": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    let after = defined_names(&ann);
    assert_ne!(after, before, "the delete changes the names of the workbook");
    assert_eq!(defined_names(&bob), after, "the peer sees the delete's names, not the old ones");

    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(defined_names(peer), before, "the undo restores the names of the boundary on every replica");
        assert_eq!(names(peer), ["Sheet1", "Extra"], "and the sheet they belong to");
    }

    ann.synced.redo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(defined_names(peer), after, "the redo drops them again on every replica");
        assert_eq!(names(peer), ["Sheet1"]);
    }

    // Structure protection is part of the same boundary and travels with the image as well.
    ann.synced.execute(&mut ann.session, "review.protectWorkbook", json!({ "on": true })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.execute(&mut ann.session, "home.insertRows", json!({ "rows": "1:1" })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert!(protected(peer), "the structural undo keeps the structure protection of the boundary");
    }
    server.stop().await;
}

mod package {
    use std::sync::Arc;

    pub const VBA: &[u8] = b"\xd0\xcf\x11\xe0 synthetic vbaProject";
    const CHARTSHEET: &[u8] = br#"<?xml version="1.0"?><chartsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetViews><sheetView workbookViewId="0"/></sheetViews><drawing r:id="rId1"/></chartsheet>"#;
    const DRAWING: &[u8] = br#"<?xml version="1.0"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"/>"#;
    const CHART: &[u8] = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:ser><c:val><c:numRef><c:f>Sheet1!$A$1:$A$3</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;

    /// A workbook with the content a structural image used to leave behind: a VBA project, a chart
    /// sheet with its drawing and chart, a style that is not the default and calculation settings
    /// that are not the defaults. It is written with the real writer (as a macro-enabled
    /// workbook, the only kind that carries a VBA project), so the file the room opens is the
    /// package the two replicas start from.
    pub fn workbook() -> Vec<u8> {
        use gridcraft_engine::model::{DefinedName, Preserved, PreservedPart, PreservedRel, PreservedSheet, Style, StyleTable, Visibility, Workbook};
        let mut wb = Workbook::new();
        wb.sheet_mut(0).expect("a sheet").set_value(gridcraft_engine::core::CellRef::new(0, 0), gridcraft_engine::core::Value::text("plan"));
        wb.names.push(DefinedName { name: "Total".into(), scope: None, formula: "1+1".into(), comment: String::new(), hidden: false });
        wb.vba_project = Some(Arc::new(VBA.to_vec()));
        wb.preserved = Some(Arc::new(Preserved {
            rels: vec![PreservedRel {
                id: "rId9".into(),
                rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet".into(),
                target: "xl/chartsheets/sheet1.xml".into(),
                external: false,
            }],
            parts: vec![
                PreservedPart {
                    name: "xl/chartsheets/sheet1.xml".into(),
                    content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml".into(),
                    data: Arc::from(CHARTSHEET),
                    rels: vec![drawing_rel()],
                },
                PreservedPart {
                    name: "xl/drawings/drawing1.xml".into(),
                    content_type: "application/vnd.openxmlformats-officedocument.drawing+xml".into(),
                    data: Arc::from(DRAWING),
                    rels: vec![chart_rel()],
                },
                PreservedPart {
                    name: "xl/charts/chart1.xml".into(),
                    content_type: "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".into(),
                    data: Arc::from(CHART),
                    rels: Vec::new(),
                },
            ],
            sheets: vec![PreservedSheet { name: "Chart1".into(), visibility: Visibility::Visible, rel_id: "rId9".into(), before: 1, sheet_id: 2 }],
            ..Default::default()
        }));
        let mut styles = StyleTable::default();
        let mut bold = Style::default();
        bold.font.bold = true;
        styles.intern(bold);
        wb.styles = styles;
        wb.calc.iterative = true;
        wb.calc.max_iterations = 7;
        wb.calc.max_change = 0.25;
        gridcraft_engine::io::save_bytes(&wb, "plan.xlsm").expect("the test workbook writes")
    }

    fn drawing_rel() -> gridcraft_engine::model::PreservedRel {
        gridcraft_engine::model::PreservedRel {
            id: "rId1".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing".into(),
            target: "xl/drawings/drawing1.xml".into(),
            external: false,
        }
    }

    fn chart_rel() -> gridcraft_engine::model::PreservedRel {
        gridcraft_engine::model::PreservedRel {
            id: "rId1".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart".into(),
            target: "xl/charts/chart1.xml".into(),
            external: false,
        }
    }
}

/// What a step image now carries besides the sheets, in one comparable value: the VBA project, the
/// chart sheets, their kept parts, the calculation settings and the size of the style table.
fn package_state(peer: &Peer) -> String {
    let wb = &peer.session.doc().unwrap().wb;
    format!(
        "vba={:?}; charts={:?}; parts={:?}; calc={}/{:?}/{:?}; styles={}",
        wb.vba_project.as_deref().map(Vec::as_slice),
        wb.preserved.as_ref().map(|p| p.sheets.iter().map(|s| (s.name.clone(), s.before)).collect::<Vec<_>>()),
        wb.preserved.as_ref().map(|p| p.parts.iter().map(|part| part.data.to_vec()).collect::<Vec<_>>()),
        wb.calc.iterative,
        wb.calc.max_iterations,
        wb.calc.max_change,
        wb.styles.len(),
    )
}

/// The structural image is the whole boundary, not just the sheets: the package-level content
/// (chart sheet, kept part, its relationship), the VBA project, the style table and the calculation
/// settings travel with it, so the author and the peer see the same workbook before the step, after
/// the undo and after the redo.
#[tokio::test]
async fn structural_undo_restores_the_package_state_and_vba_on_every_replica() {
    let server = support::start().await;
    let bytes = package::workbook();
    let session = support::create_with_bytes(&server, 51, &bytes).await;
    let mut ann = join_with(&server, 51, &session, "ann", Role::Editor, &bytes).await;
    let mut bob = join_with(&server, 51, &session, "bob", Role::Editor, &bytes).await;

    let before = package_state(&ann);
    assert!(before.contains("vba=Some"), "the replicas start from a workbook with a VBA project: {before}");
    assert!(before.contains("Chart1"), "and a chart sheet: {before}");
    assert_eq!(package_state(&bob), before, "both replicas start from the same package");

    ann.synced.execute(&mut ann.session, "home.insertSheet", json!({ "name": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    ann.synced.execute(&mut ann.session, "formulas.defineName", json!({ "name": "ExtraRef", "refersTo": "=Extra!$B$1", "scope": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    assert_eq!(package_state(&bob), before, "the merge commands leave the package alone");

    ann.synced.execute(&mut ann.session, "home.deleteSheet", json!({ "sheet": "Extra" })).unwrap();
    settle(&mut ann, &mut bob).await;
    let after = package_state(&ann);
    assert_eq!(package_state(&bob), after, "the peer sees the delete");

    ann.synced.undo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(names(peer), ["Sheet1", "Extra"], "the deleted sheet is back on every replica");
        assert_eq!(package_state(peer), before, "and so is the whole package state of the boundary");
    }
    assert_eq!(ann.synced.state().version, bob.synced.state().version);

    ann.synced.redo().unwrap();
    settle(&mut ann, &mut bob).await;
    for peer in [&ann, &bob] {
        assert_eq!(names(peer), ["Sheet1"], "the redo drops the sheet again on every replica");
        assert_eq!(package_state(peer), after, "the package state is the after-image's again");
    }
    assert_eq!(ann.synced.state().version, bob.synced.state().version);
    server.stop().await;
}

/// A replica starts at the version its own workbook is at, not at the transport's cursor.
///
/// The connection task handles the catch-up as soon as `welcome` is in, and the caller builds the
/// replica afterwards: by then `state().version` is the room's head while the document in front is
/// still the file the user opened. Starting from the cursor made the queued catch-up look already
/// covered, so the window showed the pre-catch-up workbook and no version of it ever arrived
/// (CodeRabbit 5852406613, the finding outside the diff of that review).
#[tokio::test]
async fn a_late_replica_starts_where_its_workbook_is_not_where_the_cursor_moved() {
    let server = support::start().await;
    let session = support::create(&server, 46).await;
    let mut ann = join(&server, 46, &session, "ann", Role::Editor).await;
    for input in 1..=3 {
        ann.synced.execute(&mut ann.session, "cell.set", json!({ "cell": "A1", "input": input })).unwrap();
    }
    settle_one(&mut ann).await;
    assert_eq!(ann.synced.state().version, 3);

    // Bob opens the blank workbook he has and dials: the room's three operations are the catch-up.
    let mut local = Session::new();
    local.new_workbook();
    let cfg = Config {
        url: server.url(&session),
        token: support::token("bob", Role::Editor, 46, &session),
        name: "bob".into(),
        last_version: 0,
        reconnect: Some(Duration::from_millis(200)),
        window_id: String::new(),
    };
    let (client, welcome) = Client::connect(cfg).await.expect("bob joins");
    // Wait for the connection task to handle the catch-up, which is what moves the cursor before
    // the caller can build the replica.
    for _ in 0..200 {
        if client.state().version == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(client.state().version, 3, "the catch-up moved the cursor before the replica existed");
    assert_eq!(client.start_version(), 0, "the client remembers the version its workbook is at");

    let mut bob = Peer { session: local, synced: Synced::new(client, &welcome) };
    for _ in 0..200 {
        bob.synced.poll(&mut bob.session);
        if value(&bob, "A1") == Value::Number(3.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(value(&bob, "A1"), Value::Number(3.0), "the queued catch-up landed in the document");
    assert_eq!(bob.synced.document_version(), 3, "the document ends at the version its content is at");
    server.stop().await;
}
