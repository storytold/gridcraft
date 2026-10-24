//! The window commands: listing, activating, cycling and hiding the open workbooks' windows, by document uid.

use serde_json::json;

use super::s;
use crate::{DocState, Session, Trust};

/// Three workbooks: Book1 (blank), `budget.xlsx` (has a path, clean) and Book2 (dirty, active).
fn three_windows() -> Session {
    let mut s = s();
    s.add_document(DocState::new(gridcraft_model::Workbook::new(), Some("/tmp/budget.xlsx".into()), "budget.xlsx".into(), Trust::Local));
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s
}

fn uid(s: &Session, i: usize) -> u64 {
    s.documents()[i].uid
}

fn hidden(s: &mut Session) -> Vec<bool> {
    s.execute("window.list", json!({})).unwrap().as_array().unwrap().iter().map(|w| w["hidden"].as_bool().unwrap()).collect()
}

#[test]
fn window_list_reports_id_index_title_path_dirty_active_hidden() {
    let mut s = three_windows();
    let list = s.execute("window.list", json!({})).unwrap();
    let w = list.as_array().unwrap();
    assert_eq!(w.len(), 3);
    assert_eq!(w[0], json!({"id": uid(&s, 0), "index": 0, "title": "Book1", "path": null, "dirty": false, "active": false, "hidden": false}));
    assert_eq!(
        w[1],
        json!({"id": uid(&s, 1), "index": 1, "title": "budget.xlsx", "path": "/tmp/budget.xlsx", "dirty": false, "active": false, "hidden": false})
    );
    assert_eq!(w[2], json!({"id": uid(&s, 2), "index": 2, "title": "Book2", "path": null, "dirty": true, "active": true, "hidden": false}));
    assert!(uid(&s, 0) != uid(&s, 1) && uid(&s, 1) != uid(&s, 2), "uids are unique");
    // The legacy query is the same list.
    assert_eq!(s.execute("view.switchWindows", json!({})).unwrap(), list);
    // With no document open the list is empty rather than an error.
    assert_eq!(Session::new().execute("window.list", json!({})).unwrap(), json!([]));
}

#[test]
fn window_activate_by_id_survives_an_index_shift() {
    let mut s = three_windows();
    let (book1, budget, book2) = (uid(&s, 0), uid(&s, 1), uid(&s, 2));
    assert_eq!(s.execute("window.activate", json!({"index": 0})).unwrap(), json!({"id": book1, "index": 0}));
    assert_eq!(s.doc().unwrap().uid, book1);
    // Closing Book1 shifts the others down one; their ids stay.
    assert_eq!(s.execute("file.close", json!({})).unwrap()["closed"], true);
    assert_eq!(s.execute("window.activate", json!({"id": book2})).unwrap(), json!({"id": book2, "index": 1}));
    assert_eq!(s.active_index(), 1);
    assert_eq!(s.execute("window.activate", json!({"id": budget})).unwrap(), json!({"id": budget, "index": 0}));
    // `id` wins over `index`.
    assert_eq!(s.execute("window.activate", json!({"id": book2, "index": 0})).unwrap(), json!({"id": book2, "index": 1}));
    // Activating by index still works.
    assert_eq!(s.execute("window.activate", json!({"index": 0})).unwrap(), json!({"id": budget, "index": 0}));
    // Unknown id, unknown index, and neither are errors that leave the active window alone.
    for p in [json!({"id": book1}), json!({"id": 0}), json!({"index": 2}), json!({}), json!({"id": "1"})] {
        assert!(s.execute("window.activate", p.clone()).is_err(), "{p}");
        assert_eq!(s.doc().unwrap().uid, budget, "{p}");
    }
    assert_eq!(s.document_index(book1), None);
    assert_eq!(s.document_index(book2), Some(1));
}

#[test]
fn window_next_and_previous_wrap_and_skip_hidden_windows() {
    let mut s = three_windows();
    let ids: Vec<u64> = (0..3).map(|i| uid(&s, i)).collect();
    let go = |s: &mut Session, cmd: &str| s.execute(cmd, json!({})).unwrap();
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[0], "index": 0}));
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[1], "index": 1}));
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[2], "index": 2}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[1], "index": 1}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[0], "index": 0}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[2], "index": 2}));
    // Hiding the middle window activates the next visible one and takes it out of the cycle.
    s.execute("window.activate", json!({"index": 1})).unwrap();
    assert_eq!(s.execute("view.hideWindow", json!({})).unwrap(), json!({"id": ids[2], "index": 2}));
    assert_eq!(hidden(&mut s), vec![false, true, false]);
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[0], "index": 0}));
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[2], "index": 2}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[0], "index": 0}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[2], "index": 2}));
    // With one visible window left, next and previous return it unchanged, and hiding it is refused.
    s.execute("window.activate", json!({"index": 0})).unwrap();
    assert_eq!(s.execute("view.hideWindow", json!({})).unwrap(), json!({"id": ids[2], "index": 2}));
    assert_eq!(hidden(&mut s), vec![true, true, false]);
    assert_eq!(go(&mut s, "window.next"), json!({"id": ids[2], "index": 2}));
    assert_eq!(go(&mut s, "window.previous"), json!({"id": ids[2], "index": 2}));
    let err = s.execute("view.hideWindow", json!({})).unwrap_err().to_string();
    assert!(err.contains("last visible"), "{err}");
    assert_eq!(hidden(&mut s), vec![true, true, false]);
    assert_eq!(s.active_index(), 2);
    // A single document can't be hidden either.
    let mut one = Session::new();
    one.new_workbook();
    assert!(one.execute("view.hideWindow", json!({})).is_err());
}

#[test]
fn hidden_window_is_the_document_not_its_index() {
    let mut s = three_windows();
    let (book1, budget, book2) = (uid(&s, 0), uid(&s, 1), uid(&s, 2));
    s.execute("window.activate", json!({"index": 0})).unwrap();
    assert_eq!(s.execute("view.hideWindow", json!({})).unwrap(), json!({"id": budget, "index": 1}));
    assert!(s.is_hidden(book1) && !s.is_hidden(budget));
    // Closing budget.xlsx (index 1) must not make a different window hidden.
    assert_eq!(s.execute("file.close", json!({})).unwrap()["closed"], true);
    assert_eq!(hidden(&mut s), vec![true, false]);
    assert_eq!(s.doc().unwrap().uid, book2);
    assert_eq!(s.hidden_windows, vec![book1]);
    // Activating the hidden window by id unhides it.
    assert_eq!(s.execute("window.activate", json!({"id": book1})).unwrap(), json!({"id": book1, "index": 0}));
    assert_eq!(hidden(&mut s), vec![false, false]);
    assert!(s.hidden_windows.is_empty());
    // Closing the hidden document drops its uid from the hidden list.
    s.execute("view.hideWindow", json!({})).unwrap();
    assert_eq!(s.hidden_windows, vec![book1]);
    s.close_document(0);
    assert!(s.hidden_windows.is_empty());
    assert_eq!(s.doc().unwrap().uid, book2);
}

#[test]
fn close_document_keeps_the_active_document_and_never_leaves_a_hidden_one_active() {
    // Closing a lower document keeps the same document active (its index moves down).
    let mut s = three_windows();
    let budget = uid(&s, 1);
    s.set_active(1);
    s.close_document(0);
    assert_eq!((s.active_index(), s.doc().unwrap().uid), (0, budget));
    // Closing the active document next to a hidden one activates a visible window instead.
    let mut s = three_windows();
    let (book1, budget) = (uid(&s, 0), uid(&s, 1));
    s.execute("window.activate", json!({"index": 1})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    assert_eq!(s.active_index(), 2);
    s.close_document(2);
    assert_eq!(s.doc().unwrap().uid, book1);
    assert!(s.is_hidden(budget));
    // Out-of-range indices are ignored.
    s.close_document(7);
    assert_eq!(s.documents().len(), 2);
}

#[test]
fn unhide_window_takes_an_id_an_index_or_the_most_recently_hidden() {
    let mut s = three_windows();
    let (book1, budget) = (uid(&s, 0), uid(&s, 1));
    let err = s.execute("view.unhideWindow", json!({})).unwrap_err().to_string();
    assert!(err.contains("no hidden windows"), "{err}");
    s.execute("window.activate", json!({"index": 0})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    assert_eq!(hidden(&mut s), vec![true, true, false]);
    // No params: the most recently hidden window.
    assert_eq!(s.execute("view.unhideWindow", json!({})).unwrap(), json!({"id": budget, "index": 1}));
    assert_eq!(hidden(&mut s), vec![true, false, false]);
    // By id, then by index; unknown ones are errors.
    assert!(s.execute("view.unhideWindow", json!({"id": 0})).is_err());
    assert!(s.execute("view.unhideWindow", json!({"index": 9})).is_err());
    assert_eq!(hidden(&mut s), vec![true, false, false]);
    assert_eq!(s.execute("view.unhideWindow", json!({"id": book1})).unwrap(), json!({"id": book1, "index": 0}));
    assert_eq!(hidden(&mut s), vec![false, false, false]);
    s.execute("view.hideWindow", json!({})).unwrap();
    assert_eq!(s.execute("view.unhideWindow", json!({"index": 0})).unwrap(), json!({"id": book1, "index": 0}));
    assert_eq!(hidden(&mut s), vec![false, false, false]);
    assert_eq!(s.doc().unwrap().uid, book1);
}

#[test]
fn closing_the_last_visible_window_unhides_the_one_that_takes_over() {
    // Found by a randomized probe: with every other window hidden, closing the visible one left
    // a hidden window active and nothing listed as visible.
    let mut s = three_windows();
    s.execute("window.activate", json!({"index": 0})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    assert_eq!(s.hidden_windows.len(), 2);
    s.execute("file.close", json!({"force": true})).unwrap();
    let active = s.active().unwrap().uid;
    assert!(!s.is_hidden(active), "the window that takes over is visible");
    let list = s.execute("window.list", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().iter().filter(|w| w["hidden"] == false).count(), 1);
    // Activating a hidden window by any route unhides it (`file.open` of an open file included).
    let hidden = s.hidden_windows[0];
    let i = s.document_index(hidden).unwrap();
    s.set_active(i);
    assert!(!s.is_hidden(hidden) && s.hidden_windows.is_empty());
}

#[test]
fn a_window_selector_that_is_given_but_unusable_is_an_error() {
    let mut s = three_windows();
    s.execute("window.activate", json!({"index": 0})).unwrap();
    s.execute("view.hideWindow", json!({})).unwrap();
    let hidden = s.hidden_windows.clone();
    for p in [json!({"id": -1}), json!({"id": "1"}), json!({"id": 0.5}), json!({"index": -1}), json!({"index": 1e30}), json!({"index": [1]})] {
        assert!(s.execute("view.unhideWindow", p.clone()).is_err(), "{p}");
        assert!(s.execute("window.activate", p.clone()).is_err(), "{p}");
        assert_eq!(s.hidden_windows, hidden, "{p}: nothing unhidden");
    }
    // `null` counts as not given: the most recently hidden window is unhidden.
    s.execute("view.unhideWindow", json!({"id": null})).unwrap();
    assert!(s.hidden_windows.is_empty());
}
