//! Rolling back a refused preview: only the preview the server refused, never a later one.
//!
//! The transport is a fake queue here; the engine and `Synced` are the real ones.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gridcraft_collab::{
    Applied, CellChange, ClientMessage, ClientOp, CommandEcho, Outcome, Presence, RejectCode, Rejected, Role, SheetDelta, SheetName,
};
use gridcraft_collab_client::{Event, Link, State, Synced, Welcome};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

#[derive(Clone)]
struct Sink {
    state: Rc<RefCell<State>>,
    events: Rc<RefCell<VecDeque<Event>>>,
    sent: Rc<RefCell<Vec<ClientMessage>>>,
}

impl Sink {
    fn push(&self, event: Event) {
        self.events.borrow_mut().push_back(event);
    }

    fn sent(&self) -> Vec<ClientMessage> {
        self.sent.borrow().clone()
    }
}

struct FakeLink(Sink);

impl Link for FakeLink {
    fn state(&self) -> State {
        self.0.state.borrow().clone()
    }

    fn send(&self, _op: ClientOp) -> bool {
        true
    }

    fn send_message(&self, message: ClientMessage) -> bool {
        self.0.sent.borrow_mut().push(message);
        true
    }

    fn send_presence(&self, _presence: Presence) -> bool {
        true
    }

    fn poll(&mut self) -> Option<Event> {
        self.0.events.borrow_mut().pop_front()
    }
}

/// One replica in a session on `Sheet1`, at version 0.
fn replica() -> (Synced, Sink, Session) {
    let sink = Sink {
        state: Rc::new(RefCell::new(State {
            session_id: "session".into(),
            actor: "ann".into(),
            window_id: "w1".into(),
            role: Some(Role::Editor),
            version: 0,
            epoch: 0,
            sheets: vec![SheetName { key: "0".into(), name: "Sheet1".into() }],
            participants: Vec::new(),
            connected: true,
        })),
        events: Rc::new(RefCell::new(VecDeque::new())),
        sent: Rc::new(RefCell::new(Vec::new())),
    };
    let welcome = Welcome {
        session_id: "session".into(),
        actor_id: "ann".into(),
        role: Role::Editor,
        version: 0,
        epoch: 0,
        sheets: vec![SheetName { key: "0".into(), name: "Sheet1".into() }],
        participants: Vec::new(),
    };
    let link = FakeLink(sink.clone());
    let synced = Synced::with_link(Box::new(link), &welcome, "w1".into());
    let mut session = Session::new();
    session.new_workbook();
    (synced, sink, session)
}

fn value(session: &Session, a1: &str) -> Value {
    let at = CellRef::parse(a1).expect("a valid address");
    session.doc().expect("a document").wb.active().expect("a sheet").value(at)
}

/// What the formula bar shows for a cell: the formula, the Python code or the value.
fn input(session: &Session, a1: &str) -> String {
    let at = CellRef::parse(a1).expect("a valid address");
    session.doc().expect("a document").wb.active().and_then(|sheet| sheet.cell(at)).map(|cell| cell.input_text()).unwrap_or_default()
}

#[test]
fn a_refused_earlier_preview_does_not_undo_the_later_one() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("preview A");
    let a = "w1-0".to_string();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 2 })).expect("preview B");
    assert_eq!(value(&session, "A1"), Value::Number(2.0), "B is the last preview");

    sink.push(Event::Rejected(Rejected { op_id: a, code: RejectCode::Invalid, retryable: false, hint: None, message: None }));
    let events = synced.poll(&mut session);
    assert!(matches!(events.first(), Some(Event::Rejected(_))));
    assert_eq!(value(&session, "A1"), Value::Number(2.0), "B's preview is not the one that was refused");
    assert!(
        sink.sent().iter().any(|message| matches!(message, ClientMessage::Snapshot)),
        "the replica asks for the authoritative workbook, not a tail that cannot hold the refused edit: {:?}",
        sink.sent()
    );
}

/// The workbook the client asked for replaces the document: the preview the room refused is gone,
/// and what the room did keep is what this replica now shows.
#[test]
fn the_workbook_a_client_asked_for_replaces_the_refused_preview() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("preview A");
    synced.execute(&mut session, "cell.set", json!({ "cell": "B1", "input": 2 })).expect("preview B");
    sink.push(Event::Rejected(Rejected { op_id: "w1-0".into(), code: RejectCode::Invalid, retryable: false, hint: None, message: None }));
    let _ = synced.poll(&mut session);
    assert!(sink.sent().iter().any(|message| matches!(message, ClientMessage::Snapshot)));

    // The server's answer: the workbook as the room has it — A1 and B1 empty, C1 set by a peer.
    let mut room = Session::new();
    room.new_workbook();
    room.execute("cell.set", json!({ "cell": "C1", "input": 7 })).expect("the room's own edit");
    let bytes = gridcraft_engine::io::save_bytes(&room.doc().expect("a document").wb, "session.xlsx").expect("a workbook");
    sink.push(Event::Snapshot { version: 3, epoch: 0, bytes });
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, "A1"), Value::Empty, "the refused preview is gone with the document it lived in");
    assert_eq!(value(&session, "B1"), Value::Empty, "so is the preview that sat on top of it");
    assert_eq!(value(&session, "C1"), Value::Number(7.0), "the replica now holds the room's document");
}

#[test]
fn a_refused_last_preview_is_rolled_back_locally() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("preview A");
    let a = "w1-0".to_string();
    assert_eq!(value(&session, "A1"), Value::Number(1.0));

    sink.push(Event::Rejected(Rejected { op_id: a, code: RejectCode::Invalid, retryable: false, hint: None, message: None }));
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, "A1"), Value::Empty, "the refused preview is gone");
    assert!(
        !sink.sent().iter().any(|message| matches!(message, ClientMessage::Resync { .. })),
        "a plain rollback needs no catch-up: {:?}",
        sink.sent()
    );
}

/// A preview made while the workbook this replica asked for is in transit keeps its place in the
/// room. The socket is handled in order, so the server runs the snapshot request before this
/// preview and the workbook that comes back does not contain it; the answer to the preview then
/// arrives with the command echo only, because the document it lands in is no longer the one the
/// preview was made on (CodeRabbit 5848894080, `synced.rs:385`).
#[test]
fn a_preview_made_while_the_workbook_was_in_transit_is_replayed() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": "=1+1" })).expect("preview C");
    assert_eq!(input(&session, "A1"), "=1+1", "the preview holds the formula");

    let mut room = Session::new();
    room.new_workbook();
    let bytes = gridcraft_engine::io::save_bytes(&room.doc().expect("a document").wb, "session.xlsx").expect("a workbook");
    sink.push(Event::Snapshot { version: 3, epoch: 0, bytes });
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, "A1"), Value::Empty, "the workbook the room sent does not hold the preview");

    sink.push(Event::Applied(Applied {
        op_id: "w1-0".into(),
        version: 4,
        epoch: 0,
        actor: "ann".into(),
        result: Outcome {
            command: Some(CommandEcho { id: "cell.set".into(), params: json!({ "cell": "A1", "input": "=1+1" }) }),
            delta: Some(vec![SheetDelta {
                sheet: "0".into(),
                cells: vec![CellChange {
                    a1: "A1".into(),
                    value: json!({ "t": "Number", "v": 2.0 }),
                    formula: Some("1+1".into()),
                    style: None,
                    py: None,
                    array: None,
                    dynamic: false,
                }],
                objects_deleted: Vec::new(),
                objects_created: Vec::new(),
            }]),
        },
        sheets: None,
        images: None,
        book: None,
        more: false,
        image_part: None,
    }));
    let _ = synced.poll(&mut session);
    assert_eq!(input(&session, "A1"), "=1+1", "the command is replayed; patching the value alone would leave a constant");
    assert_eq!(value(&session, "A1"), Value::Number(2.0));
}
