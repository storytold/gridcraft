//! Rolling back a refused preview: only the preview the server refused, never a later one.
//!
//! The transport is a fake queue here; the engine and `Synced` are the real ones.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gridcraft_collab::{ClientMessage, ClientOp, Presence, RejectCode, Rejected, Role, SheetName};
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
        sink.sent().iter().any(|message| matches!(message, ClientMessage::Resync { .. })),
        "the replica asks for the document instead of guessing: {:?}",
        sink.sent()
    );
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
