//! A session belongs to one workbook, wherever the user goes afterwards.
//!
//! The room holds one document. The desktop shell can open or switch to another workbook while a
//! session is live, and the replica used to resolve everything through the *active* one: a local
//! edit in another file was sent as an operation with a sheet name that happened to match, and a
//! remote operation was applied to whatever was in front. These tests pin the two directions and
//! the snapshot that replaces the shared workbook only.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gridcraft_collab::{Applied, CellChange, ClientMessage, ClientOp, CommandEcho, Outcome, Presence, Role, SheetDelta, SheetName};
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

    fn deliver(&self, applied: &Applied) {
        let mut state = self.state.borrow_mut();
        gridcraft_collab_client::note_applied(&mut state, applied);
        drop(state);
        self.push(Event::Applied(applied.clone()));
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

fn sheets() -> Vec<SheetName> {
    vec![SheetName { key: "0".into(), name: "Sheet1".into() }]
}

/// One replica in a session on the first workbook of `session`, bound by its first operation.
fn replica() -> (Synced, Sink, Session) {
    let sink = Sink {
        state: Rc::new(RefCell::new(State {
            session_id: "session".into(),
            actor: "ann".into(),
            window_id: "w1".into(),
            role: Some(Role::Editor),
            version: 0,
            epoch: 0,
            sheets: sheets(),
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
        sheets: sheets(),
        participants: Vec::new(),
    };
    let synced = Synced::with_link(Box::new(FakeLink(sink.clone())), &welcome, "w1".into());
    let mut session = Session::new();
    session.new_workbook();
    (synced, sink, session)
}

fn value(session: &Session, index: usize, a1: &str) -> Value {
    let at = CellRef::parse(a1).expect("a valid address");
    session.documents()[index].wb.active().expect("a sheet").value(at)
}

#[test]
fn an_edit_in_another_workbook_stays_local() {
    let (mut synced, _sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");
    session.new_workbook();
    assert!(!synced.shares_active_document(&session), "the window in front is not the session's workbook");
    assert_eq!(synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 2 })), Err(gridcraft_collab_client::EditError::OtherDocument));
    assert_eq!(value(&session, 1, "A1"), Value::Empty, "the other workbook is not written through the session");
}

#[test]
fn a_remote_operation_lands_in_the_shared_workbook() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");
    session.new_workbook();
    assert_eq!(session.active_index(), 1);

    let applied = Applied {
        op_id: "bob-0".into(),
        version: 2,
        epoch: 0,
        actor: "bob".into(),
        result: Outcome {
            command: Some(CommandEcho { id: "cell.set".into(), params: json!({ "sheet": "Sheet1", "cell": "A1", "input": 42 }) }),
            delta: Some(vec![SheetDelta {
                sheet: "0".into(),
                cells: vec![CellChange {
                    a1: "A1".into(),
                    value: serde_json::to_value(Value::Number(42.0)).expect("a cell value"),
                    formula: None,
                    style: Some(0),
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
    };
    sink.deliver(&applied);
    let _ = synced.poll(&mut session);

    assert_eq!(value(&session, 0, "A1"), Value::Number(42.0), "the shared workbook gets the edit");
    assert_eq!(value(&session, 1, "A1"), Value::Empty, "the other open workbook is untouched");
    assert_eq!(session.active_index(), 1, "the window the user was in stays in front");
}

#[test]
fn a_command_less_undo_restores_the_formula_on_a_peer() {
    let (mut synced, sink, mut session) = replica();
    let _ = synced.poll(&mut session);
    session.execute("cell.set", json!({ "cell": "A1", "input": 10 })).expect("the constant the step left");

    let applied = Applied {
        op_id: "undo-ann-0".into(),
        version: 3,
        epoch: 0,
        actor: "ann".into(),
        result: Outcome {
            command: None,
            delta: Some(vec![SheetDelta {
                sheet: "0".into(),
                cells: vec![CellChange {
                    a1: "A1".into(),
                    value: serde_json::to_value(Value::Number(3.0)).expect("a cell value"),
                    formula: Some("=1+2".into()),
                    style: Some(0),
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
    };
    sink.deliver(&applied);
    let _ = synced.poll(&mut session);

    let at = CellRef::parse("A1").expect("a valid address");
    let cell = session.documents()[0].wb.active().and_then(|sheet| sheet.cell(at)).cloned().expect("the cell");
    assert_eq!(cell.value, Value::Number(3.0), "the restored value landed");
    assert_eq!(cell.formula.as_ref().map(|formula| formula.text.clone()), Some("1+2".into()), "the formula comes back, not only the value");
}

#[test]
fn a_snapshot_replaces_only_the_shared_workbook() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");
    session.prefs.user_name = "Ann".into();
    session.new_workbook();
    assert_eq!(session.documents().len(), 2);

    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({ "cell": "A1", "input": 99 })).expect("the file's cell");
    let authoritative = source.doc().expect("a document").wb.clone();
    let bytes = gridcraft_engine::io::save_bytes(&authoritative, "session.xlsx").expect("the snapshot bytes");
    sink.push(Event::Snapshot { version: 4, epoch: 0, bytes });
    let _ = synced.poll(&mut session);

    assert_eq!(session.documents().len(), 2, "the other open workbook is still open");
    assert_eq!(value(&session, 0, "A1"), Value::Number(99.0), "the shared workbook is the snapshot");
    assert_eq!(value(&session, 1, "A1"), Value::Empty, "the other workbook keeps its content");
    assert_eq!(session.prefs.user_name, "Ann", "the session's preferences stay");
}
