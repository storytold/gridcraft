//! A cell edit the sheet UI commits by itself is a session operation when the app is in one.
//!
//! The editor's commit and a form control's value used to call `Session::run` straight, so in a
//! live session the edit stayed on this replica while the room and every other replica kept the
//! old cell. Both now go through the collaboration layer first, which is what these tests see.

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::collab::{Collab, PresenceView, SyncStatus};
use serde_json::{Value as Json, json};

/// A stand-in for a live session: it records the commands the sheet UI handed it and runs them
/// the way the real adapter's local preview does.
#[derive(Clone, Default)]
struct Recording(Rc<RefCell<Vec<String>>>);

impl Collab for Recording {
    fn execute(&mut self, session: &mut Session, id: &str, params: Json) -> Option<Result<Json, String>> {
        self.0.borrow_mut().push(id.to_string());
        Some(session.execute(id, params).map_err(|error| error.to_string()))
    }

    fn poll(&mut self, _session: &mut Session) -> Vec<PresenceView> {
        Vec::new()
    }

    fn status(&self) -> SyncStatus {
        SyncStatus::Online { version: 1, participants: 1 }
    }

    fn read_only(&self) -> bool {
        false
    }
}

fn app() -> (gridcraft_ui_egui::SheetApp, Recording) {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    let seen = Recording::default();
    app.collab = Some(Box::new(seen.clone()));
    (app, seen)
}

#[test]
fn a_committed_cell_edit_goes_through_the_session() {
    let (mut app, seen) = app();
    app.begin_edit(Some("7".into()), false);
    assert!(app.commit_edit(0, 0, false, false), "the edit commits");
    assert_eq!(*seen.0.borrow(), ["cell.set"], "the edit is the session's operation, not a local run");
    let at = CellRef::parse("A1").expect("a valid address");
    assert_eq!(app.session.active().and_then(|d| d.wb.active()).map(|sheet| sheet.value(at)), Some(Value::Number(7.0)));
}

#[test]
fn a_fill_committed_from_the_editor_goes_through_the_session() {
    let (mut app, seen) = app();
    app.session.execute("selection.set", json!({ "range": "B2:C3" })).expect("a selection");
    app.begin_edit(Some("5".into()), false);
    assert!(app.commit_edit(0, 0, false, true), "the fill commits");
    assert_eq!(*seen.0.borrow(), ["range.fill"], "the fill is the session's operation, not a local run");
    for a1 in ["B2", "C2", "B3", "C3"] {
        let at = CellRef::parse(a1).expect("a valid address");
        assert_eq!(app.session.active().and_then(|d| d.wb.active()).map(|sheet| sheet.value(at)), Some(Value::Number(5.0)), "{a1}");
    }
}
