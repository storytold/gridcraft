//! A structural undo whose sheet image does not fit one frame (ADR-0013).
//!
//! The image arrives in parts; a replica applies the operation only when the last part is in,
//! and a sequence that breaks leaves the document and its version exactly where they were. The
//! transport is a fake queue here: the desktop and browser links decode the same `applied` frames
//! into the same events.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gridcraft_collab::{Applied, ClientMessage, ClientOp, IMAGE_PART, Outcome, Presence, Role, SheetImage, SheetName, split_applied};
use gridcraft_collab_client::{Event, Link, State, Synced, Welcome};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

/// The parts of a fake connection the test keeps a handle on.
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

    /// What a transport does with one `applied`: advance the shared state (only a complete
    /// operation does) and hand the event to the replica.
    fn deliver(&self, applied: &Applied) {
        let mut state = self.state.borrow_mut();
        gridcraft_collab_client::note_applied(&mut state, applied);
        drop(state);
        self.push(Event::Applied(applied.clone()));
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

/// One replica at version 3, epoch 1, with a local cell that an image must not touch by accident.
fn replica() -> (Synced, Sink, Session) {
    let sink = Sink {
        state: Rc::new(RefCell::new(State {
            session_id: "session".into(),
            actor: "ann".into(),
            window_id: "ann-1".into(),
            role: Some(Role::Editor),
            version: 3,
            epoch: 1,
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
        version: 3,
        epoch: 1,
        sheets: vec![SheetName { key: "0".into(), name: "Sheet1".into() }],
        participants: Vec::new(),
    };
    let synced = Synced::with_link(Box::new(FakeLink(sink.clone())), &welcome, "ann-1".into());
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "Z9", "input": 42})).expect("a local cell");
    (synced, sink, session)
}

/// A sheet whose image is over `bytes` long: one cell holding a string of that size.
fn big_image(bytes: usize) -> SheetImage {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "x".repeat(bytes)})).expect("the cell takes the string");
    let doc = session.doc().expect("a document");
    let sheet = doc.wb.active().expect("a sheet").clone();
    SheetImage { key: "0".into(), name: sheet.name.clone(), index: 0, sheet, preserved: None }
}

fn applied_image(images: Vec<SheetImage>) -> Applied {
    Applied {
        op_id: "undo-step".into(),
        version: 4,
        epoch: 2,
        actor: "ann".into(),
        result: Outcome { command: None, delta: None },
        sheets: Some(vec![SheetName { key: "0".into(), name: "Sheet1".into() }]),
        images: Some(images),
        book: None,
        more: false,
        image_part: None,
    }
}

fn text_at(session: &Session, a1: &str) -> Value {
    let doc = session.doc().expect("a document");
    doc.wb.active().expect("a sheet").value(CellRef::parse(a1).expect("an address"))
}

#[test]
fn an_eight_megabyte_image_is_applied_on_two_replicas_only_when_the_last_part_is_in() {
    let image = big_image(9 * 1024 * 1024);
    let parts = split_applied(applied_image(vec![image]), IMAGE_PART);
    assert!(parts.len() > 1, "the image does not fit one frame");
    let last = parts.len() - 1;

    let (mut ann, ann_sink, mut ann_session) = replica();
    let (mut bob, bob_sink, mut bob_session) = replica();
    for (index, part) in parts.iter().enumerate() {
        ann_sink.deliver(part);
        bob_sink.deliver(part);
        let ann_events = ann.poll(&mut ann_session);
        let bob_events = bob.poll(&mut bob_session);
        if index < last {
            assert!(ann_events.is_empty() && bob_events.is_empty(), "a part is not a document change");
            assert_eq!(ann.state().version, 3, "the version waits for the last part");
            assert_eq!(text_at(&ann_session, "A1"), Value::Empty, "nothing is applied on a prefix");
        } else {
            assert_eq!(ann_events.len(), 1, "the complete operation is one event");
            assert_eq!(bob_events.len(), 1);
        }
    }

    for (peer, session) in [(&ann, &ann_session), (&bob, &bob_session)] {
        assert_eq!(peer.state().version, 4, "the last part advances the version");
        assert_eq!(peer.state().epoch, 2);
        match text_at(session, "A1") {
            Value::Text(text) => assert_eq!(text.len(), 9 * 1024 * 1024),
            other => panic!("the image did not land: {other:?}"),
        }
        assert_eq!(text_at(session, "Z9"), Value::Empty, "the image is the whole sheet, not a patch");
    }
}

#[test]
fn a_sequence_that_breaks_at_the_socket_leaves_the_document_alone() {
    let parts = split_applied(applied_image(vec![big_image(2 * 1024 * 1024)]), IMAGE_PART);
    assert!(parts.len() >= 4, "the image needs several frames");
    let (mut ann, sink, mut session) = replica();

    for part in parts.iter().take(2) {
        sink.deliver(part);
        assert!(ann.poll(&mut session).is_empty(), "a prefix is not applied");
    }
    sink.push(Event::Disconnected { reason: "the socket closed".into() });
    let events = ann.poll(&mut session);
    assert!(matches!(events.as_slice(), [Event::Disconnected { .. }]));
    assert_eq!(ann.state().version, 3, "the replica reports the version before the first part");
    assert_eq!(text_at(&session, "A1"), Value::Empty, "the half image never became the document");
    assert_eq!(text_at(&session, "Z9"), Value::Number(42.0), "the local document is untouched");
    assert!(ann.take_notice().is_some(), "the interruption is reported");
}

#[test]
fn a_missing_part_is_refused_with_a_catch_up_from_before_the_first_one() {
    let parts = split_applied(applied_image(vec![big_image(2 * 1024 * 1024)]), IMAGE_PART);
    assert!(parts.len() >= 4);
    let (mut ann, sink, mut session) = replica();

    sink.deliver(&parts[0]);
    assert!(ann.poll(&mut session).is_empty());
    // Part 1 is lost, part 2 arrives: the sequence no longer holds together.
    sink.deliver(&parts[2]);
    assert!(ann.poll(&mut session).is_empty(), "a gap never becomes a document state");

    let sent = sink.sent();
    match sent.last() {
        Some(ClientMessage::Resync { from }) => assert_eq!(*from, 3, "the catch-up starts before the first part"),
        other => panic!("expected a resync, got {other:?}"),
    }
    assert_eq!(ann.state().version, 3);
    assert_eq!(text_at(&session, "A1"), Value::Empty);
    assert_eq!(text_at(&session, "Z9"), Value::Number(42.0));
}

#[test]
fn a_part_of_an_operation_the_client_cannot_assemble_is_never_applied() {
    let (mut ann, sink, mut session) = replica();
    let mut partial = applied_image(vec![big_image(1024)]);
    partial.more = true;
    partial.images = None;

    sink.push(Event::Applied(partial));
    assert!(ann.poll(&mut session).is_empty(), "a prefix is not a document state");
    match sink.sent().last() {
        Some(ClientMessage::Resync { from }) => assert_eq!(*from, 3),
        other => panic!("expected a resync, got {other:?}"),
    }
    assert_eq!(ann.state().version, 3);
    assert_eq!(text_at(&session, "Z9"), Value::Number(42.0));
}

/// The chunked payload is the whole image, not only the sheets: the workbook-level state and a
/// sheet's kept file content travel inside the parts and land on both replicas together with the
/// operation. A part that is missing still applies nothing of them.
#[test]
fn a_chunked_image_carries_the_book_metadata_and_the_kept_parts_to_both_replicas() {
    use gridcraft_engine::model::{DefinedName, Preserved, PreservedPart, WorkbookMeta};

    let mut image = big_image(2 * 1024 * 1024);
    image.preserved = Some(std::sync::Arc::new(Preserved {
        parts: vec![PreservedPart {
            name: "xl/ctrlProps/ctrlProps1.xml".into(),
            content_type: "application/vnd.ms-excel.controlproperties+xml".into(),
            data: std::sync::Arc::from(&b"<controls/>"[..]),
            rels: Vec::new(),
        }],
        ..Default::default()
    }));
    let mut applied = applied_image(vec![image]);
    applied.book = Some(Box::new(WorkbookMeta {
        names: vec![DefinedName { name: "Total".into(), scope: None, formula: "Sheet1!$A$1".into(), comment: String::new(), hidden: false }],
        protected_structure: true,
        structure_password: None,
        ..Default::default()
    }));
    let parts = split_applied(applied, IMAGE_PART);
    assert!(parts.len() > 1, "the image must travel in parts for this test");

    let (mut ann, ann_sink, mut ann_session) = replica();
    let (mut bob, bob_sink, mut bob_session) = replica();
    for part in &parts[..parts.len() - 1] {
        ann_sink.deliver(part);
        bob_sink.deliver(part);
        assert!(ann.poll(&mut ann_session).is_empty(), "a prefix is not applied");
        assert!(bob.poll(&mut bob_session).is_empty());
    }
    let last = &parts[parts.len() - 1];
    ann_sink.deliver(last);
    bob_sink.deliver(last);
    assert_eq!(ann.poll(&mut ann_session).len(), 1);
    assert_eq!(bob.poll(&mut bob_session).len(), 1);

    for (peer, session) in [(&ann, &ann_session), (&bob, &bob_session)] {
        assert_eq!(peer.state().version, 4);
        let d = session.doc().unwrap();
        assert_eq!(d.wb.names.len(), 1, "the name of the boundary is restored");
        assert_eq!(d.wb.names[0].name, "Total");
        assert!(d.wb.protected_structure, "structure protection is restored");
        let kept = d.wb.sheet(0).and_then(|sheet| sheet.preserved.as_ref()).expect("the kept part of the sheet is back");
        assert_eq!(kept.parts[0].name, "xl/ctrlProps/ctrlProps1.xml");
    }
}
