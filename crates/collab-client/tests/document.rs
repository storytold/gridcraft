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

    fn sent(&self) -> Vec<ClientMessage> {
        self.sent.borrow().clone()
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
    let synced = Synced::with_link(Box::new(FakeLink(sink.clone())), &welcome, "w1".into(), 0);
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

/// An operation for a document that is no longer open here must not be written into another
/// workbook: `enter_shared` has nowhere to go once the session's document was closed, and the
/// answer belongs to a file that the other open workbook is not. The version stays too — the
/// replica does not hold content it never wrote (CodeRabbit 5855372513, `synced.rs:193`).
#[test]
fn an_operation_for_a_closed_document_is_not_written_into_another_workbook() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");
    session.new_workbook();
    assert_eq!(session.documents().len(), 2);
    session.close_document(0);
    assert_eq!(session.documents().len(), 1, "only the unrelated workbook is still open");

    sink.deliver(&applied_set("bob-1", 2, 7));
    let events = synced.poll(&mut session);

    assert_eq!(value(&session, 0, "A1"), Value::Empty, "the remaining workbook is not written through the session");
    assert_eq!(synced.document_version(), 0, "no version is claimed for a document this replica does not hold");
    assert!(synced.take_notice().is_some_and(|notice| notice.contains("closed")), "the user is told the session's document is gone");
    assert!(!events.iter().any(|event| matches!(event, Event::Applied(_))), "the caller is not told a change happened");
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

/// The transport's cursor can be ahead of the document when the replica is built: the connection
/// task, or a host's frame queue, handled the catch-up frames and queued them as events before the
/// caller got the `welcome`. Starting from the cursor made every queued answer look already
/// covered, so the window kept the workbook from before the catch-up and no version of it ever
/// arrived (CodeRabbit 5852406613, the finding outside the diff of that review).
#[test]
fn a_catch_up_queued_before_the_session_was_built_still_lands() {
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
    // The connection is already running: the room's three operations are queued and the cursor
    // sits on the head by the time the caller builds the replica.
    for (op_id, version, input) in [("bob-1", 1, 7), ("bob-2", 2, 8), ("bob-3", 3, 9)] {
        sink.deliver(&applied_set(op_id, version, input));
    }
    assert_eq!(sink.state.borrow().version, 3, "the cursor is ahead of the document");
    let welcome = Welcome {
        session_id: "session".into(),
        actor_id: "ann".into(),
        role: Role::Editor,
        version: 3,
        epoch: 0,
        sheets: sheets(),
        participants: Vec::new(),
    };
    // The workbook in front is at version 0: that is what the caller passes, not the cursor.
    let mut synced = Synced::with_link(Box::new(FakeLink(sink.clone())), &welcome, "w1".into(), 0);

    let mut session = Session::new();
    session.new_workbook();
    let events = synced.poll(&mut session);

    assert_eq!(value(&session, 0, "A1"), Value::Number(9.0), "the last queued answer is in the document");
    assert_eq!(events.iter().filter(|event| matches!(event, Event::Applied(_))).count(), 3, "every queued answer reaches the caller");
    assert_eq!(synced.document_version(), 3, "the document ends at the version its content is at");
}

/// A workbook the client cannot open leaves the document and its version alone, and the replica
/// asks for the workbook again: the transport has already moved its cursor onto the snapshot, so
/// claiming that version with the old content would hide every operation still missing
/// (CodeRabbit 5852406613, `synced.rs:398`).
#[test]
fn a_snapshot_that_cannot_be_opened_claims_nothing_and_asks_again() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");

    // The transport moves its cursor onto the snapshot before it hands the event over.
    sink.state.borrow_mut().version = 4;
    sink.push(Event::Snapshot { version: 4, epoch: 0, bytes: b"PK\x03\x04 broken workbook".to_vec() });
    let _ = synced.poll(&mut session);

    assert_eq!(value(&session, 0, "A1"), Value::Number(1.0), "the document is unchanged");
    assert_eq!(synced.document_version(), 0, "the replica does not claim the snapshot's version");
    assert!(synced.take_notice().is_some(), "the failure is reported, not swallowed");
    assert!(
        sink.sent().iter().any(|message| matches!(message, ClientMessage::Snapshot)),
        "the whole workbook is asked for again, not a tail from the advanced cursor: {:?}",
        sink.sent()
    );

    // The `resynced` that closes the same catch-up must not move the replica onto the version
    // either: an answer the document never saw would then be skipped as already covered.
    sink.push(Event::Resynced { version: 4, epoch: 0 });
    let _ = synced.poll(&mut session);
    assert_eq!(synced.document_version(), 0, "a failed snapshot is not a version the replica holds");

    sink.deliver(&applied_set("bob-3", 3, 7));
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, 0, "A1"), Value::Number(7.0), "the operation the document missed still lands");
}

/// A snapshot for a workbook that is no longer open cannot be installed: the replica keeps its
/// version, asks for the whole workbook once, then for the tail from the version it really holds,
/// and stops instead of pulling full workbooks forever (CodeRabbit 5852406613, `synced.rs:398`).
#[test]
fn a_snapshot_for_a_workbook_that_was_closed_is_never_claimed() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");
    session.close_document(0);

    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({ "cell": "A1", "input": 99 })).expect("the room's cell");
    let bytes = gridcraft_engine::io::save_bytes(&source.doc().expect("a document").wb, "session.xlsx").expect("the snapshot bytes");

    sink.state.borrow_mut().version = 4;
    sink.push(Event::Snapshot { version: 4, epoch: 0, bytes: bytes.clone() });
    let _ = synced.poll(&mut session);

    assert!(session.documents().is_empty(), "the closed window is not recreated");
    assert_eq!(synced.document_version(), 0, "the snapshot's version is not claimed");
    assert!(synced.take_notice().is_some(), "the failure is reported");
    assert!(
        sink.sent().iter().any(|message| matches!(message, ClientMessage::Snapshot)),
        "the whole workbook, not a resync from the cursor the snapshot already moved: {:?}",
        sink.sent()
    );

    // The second workbook fails as well: the catch-up restarts from the version the document
    // holds, where the journal tail can still repair it.
    sink.push(Event::Snapshot { version: 5, epoch: 0, bytes });
    let _ = synced.poll(&mut session);
    assert!(
        sink.sent().iter().any(|message| matches!(message, ClientMessage::Resync { from: 0 })),
        "a tail from the version the document holds: {:?}",
        sink.sent()
    );
    assert_eq!(synced.document_version(), 0, "still nothing claimed");

    // A third one stops the requests: the replica says the session has to be reopened.
    let before = sink.sent().len();
    sink.push(Event::Snapshot { version: 6, epoch: 0, bytes: b"PK\x03\x04 broken workbook".to_vec() });
    let _ = synced.poll(&mut session);
    assert_eq!(sink.sent().len(), before, "no endless stream of workbooks");
    assert!(synced.take_notice().is_some_and(|notice| notice.contains("reopen")), "the user is told what to do");
}

/// An answer the document already covers must not run its command again: the server answers a
/// resent operation out of its dedup window with the original `applied`, whose version is below
/// the snapshot that replaced the document, and a frame queued before a snapshot can land after
/// it. An inserted row would be inserted twice (CodeRabbit 5851190259, `synced.rs:565`).
#[test]
fn an_answer_at_or_below_the_snapshot_changes_nothing() {
    let (mut synced, sink, mut session) = replica();
    synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 })).expect("the shared workbook is bound");

    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({ "cell": "A1", "input": 99 })).expect("the file's cell");
    let authoritative = source.doc().expect("a document").wb.clone();
    let bytes = gridcraft_engine::io::save_bytes(&authoritative, "session.xlsx").expect("the snapshot bytes");
    sink.push(Event::Snapshot { version: 4, epoch: 0, bytes });
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, 0, "A1"), Value::Number(99.0), "the snapshot is the document");

    // The deduplicated answer to the operation of version 3, which the snapshot already contains.
    sink.deliver(&applied_set("ann-0", 3, 7));
    let events = synced.poll(&mut session);
    assert_eq!(value(&session, 0, "A1"), Value::Number(99.0), "a covered command is not run again");
    assert!(!events.iter().any(|event| matches!(event, Event::Applied(_))), "the caller is not told a change happened");

    // An operation above the snapshot still lands.
    sink.deliver(&applied_set("bob-0", 5, 5));
    let _ = synced.poll(&mut session);
    assert_eq!(value(&session, 0, "A1"), Value::Number(5.0), "an operation newer than the snapshot is applied");
}

/// The web host opens its file over the blank workbook the shell starts with: `file.open` closes
/// the untouched blank one and adds the file, so a session that bound to the blank workbook before
/// the room's copy arrived held a document that no longer exists. Every edit then stayed local
/// (`OtherDocument`) and the snapshot install had nowhere to go. While nothing has been shared
/// yet, the session follows the workbook now in front.
#[test]
fn a_workbook_that_replaces_the_blank_start_becomes_the_shared_one() {
    let (mut synced, _sink, mut session) = replica();
    // The shell's first frame binds the session to the blank workbook.
    let _ = synced.poll(&mut session);
    // The host's file arrives: the blank one is closed and the file takes its place.
    session.new_workbook();
    session.close_document(0);
    assert_eq!(session.documents().len(), 1, "the file is the only workbook open");
    // The frame after the open rebinds: a session holds the workbook in front while nothing has
    // been shared yet.
    let _ = synced.poll(&mut session);

    assert!(synced.shares_active_document(&session), "the workbook the host opened is the session's document");
    let sent = synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 1 }));
    assert!(sent.is_ok(), "an edit in it reaches the room: {sent:?}");
    assert_eq!(value(&session, 0, "A1"), Value::Number(1.0), "the edit is previewed on the shared workbook");
}

/// The rule above stops where the session really held the document: once the room's copy is in
/// it, closing that workbook ends the session for this replica instead of moving it to another
/// file (the notice says so, and no version is claimed for a document it does not hold).
#[test]
fn the_replacement_rule_does_not_move_a_joined_session_to_another_workbook() {
    let (mut synced, sink, mut session) = replica();
    let _ = synced.poll(&mut session);
    sink.deliver(&applied_set("bob-0", 1, 1));
    let _ = synced.poll(&mut session);
    session.new_workbook();
    session.close_document(0);
    assert!(!synced.shares_active_document(&session), "another workbook is not the session's, once the document was joined");
    assert_eq!(synced.execute(&mut session, "cell.set", json!({ "cell": "A1", "input": 2 })), Err(gridcraft_collab_client::EditError::OtherDocument));
}

/// A confirmed `cell.set` of `A1` at `version`, as the server answers it.
fn applied_set(op_id: &str, version: u64, input: i64) -> Applied {
    Applied {
        op_id: op_id.into(),
        version,
        epoch: 0,
        actor: "bob".into(),
        result: Outcome {
            command: Some(CommandEcho { id: "cell.set".into(), params: json!({ "sheet": "Sheet1", "cell": "A1", "input": input }) }),
            delta: None,
        },
        sheets: None,
        images: None,
        book: None,
        more: false,
        image_part: None,
    }
}
