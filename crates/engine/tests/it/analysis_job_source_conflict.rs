//! Public embedding APIs must not leave a discarded analysis blocking its session.
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::json;

/// An early finish keeps its job; a ready job with an externally changed source is discarded atomically.
#[test]
fn finish_discards_changed_source_but_retains_an_early_job() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
    session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    let params = json!({"inputs":[{"cell":"Sheet1!A1","min":0,"max":1}],"outputs":["Sheet1!B1"],"iterations":8,"samples":false,"seed":42});
    let job = session.execute("analysis.start", json!({"params":params,"autoAdvance":false})).unwrap()["job"].clone();
    assert!(session.execute("analysis.finish", json!({"job":job})).is_err());
    assert!(session.analysis_active(), "early finish must preserve pending work");
    for _ in 0..8 {
        if session.execute("analysis.step", json!({"job":job,"trials":2})).unwrap()["state"] == "ready" {
            break;
        }
    }
    assert_eq!(session.analysis_progress().unwrap()["state"], "ready");
    let mut changed = (*session.doc().unwrap().wb).clone();
    changed.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), Value::Number(99.0));
    let changed = Arc::new(changed);
    session.doc_mut().unwrap().wb = changed.clone();
    let (undo, redo, revision) = {
        let doc = session.doc().unwrap();
        (doc.undo.len(), doc.redo.len(), doc.revision)
    };
    let error = session.execute("analysis.finish", json!({"job":job})).unwrap_err();
    assert!(error.to_string().contains("source workbook changed"));
    assert!(!session.analysis_active(), "source rejection must discard the unusable job");
    let doc = session.doc().unwrap();
    assert!(Arc::ptr_eq(&doc.wb, &changed));
    assert_eq!((doc.undo.len(), doc.redo.len(), doc.revision), (undo, redo, revision));
    assert_eq!(doc.wb.sheets.len(), 1, "a rejected finish must not publish a report");
    assert_eq!(doc.wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(99.0));
    session.execute("cell.set", json!({"cell":"A1","input":"12"})).unwrap();
    let next = session.execute("analysis.start", json!({"params":params,"autoAdvance":false})).unwrap()["job"].clone();
    assert_ne!(next, job);
    session.execute("analysis.cancel", json!({"job":next})).unwrap();
}

/// Keep another workbook before the source so closing it also changes the source's index.
fn source_close_fixture(ready: bool) -> (Session, serde_json::Value, serde_json::Value) {
    let mut session = Session::new();
    session.new_workbook();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
    session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
    let params = json!({"inputs":[{"cell":"Sheet1!A1","min":0,"max":1}],"outputs":["Sheet1!B1"],"iterations":8,"samples":false,"seed":42});
    let job = session.execute("analysis.start", json!({"params":params,"autoAdvance":false})).unwrap()["job"].clone();
    if ready {
        for _ in 0..8 {
            if session.execute("analysis.step", json!({"job":job,"trials":2})).unwrap()["state"] == "ready" {
                break;
            }
        }
        assert_eq!(session.analysis_progress().unwrap()["state"], "ready");
    } else {
        assert!(session.execute("analysis.finish", json!({"job":job})).unwrap_err().to_string().contains("not ready"));
        assert!(session.analysis_active(), "an early finish still retains its job");
    }
    (session, params, job)
}

/// Closing another book preserves identity/progress; closing the last source releases a no-doc session.
#[test]
fn source_close_after_an_unrelated_close_clears_pending_and_ready_jobs() {
    for ready in [false, true] {
        let (mut session, params, job) = source_close_fixture(ready);
        let source = session.doc().unwrap().wb.clone();
        let uid = session.doc().unwrap().uid;
        let progress = session.analysis_progress().unwrap();
        let cancellation = session.analysis_cancellation().unwrap();
        session.close_document(0);
        assert_eq!(session.documents().len(), 1);
        assert_eq!(session.active_index(), 0);
        assert_eq!(session.doc().unwrap().uid, uid);
        assert!(Arc::ptr_eq(&source, &session.doc().unwrap().wb));
        assert!(session.analysis_active());
        assert!(!cancellation.is_cancelled(), "closing another workbook must not cancel the source job");
        let after = session.analysis_progress().unwrap();
        for field in ["job", "state", "completed"] {
            assert_eq!(after[field], progress[field]);
        }

        session.close_document(0);
        assert!(session.documents().is_empty());
        assert!(session.active().is_none());
        assert!(!session.analysis_active());
        assert!(session.analysis_progress().is_none());
        assert!(session.analysis_cancellation().is_none());
        assert!(cancellation.is_cancelled());
        assert_eq!(source.sheets.len(), 1, "closing a source must not publish a report");
        assert_eq!(source.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(10.0));

        session.execute("file.new", json!({})).unwrap();
        session.execute("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
        session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
        let next = session.execute("analysis.start", json!({"params":params,"autoAdvance":false})).unwrap()["job"].clone();
        assert_ne!(next, job);
        session.execute("analysis.cancel", json!({"job":next})).unwrap();
    }
}

/// Closing the source also releases the barrier when another workbook remains open and active.
#[test]
fn closing_source_leaves_other_workbook_editable_and_allows_a_new_analysis() {
    for ready in [false, true] {
        let (mut session, params, job) = source_close_fixture(ready);
        let other_uid = session.documents().first().unwrap().uid;
        let source = session.doc().unwrap().wb.clone();
        let cancellation = session.analysis_cancellation().unwrap();
        session.close_document(session.active_index());
        assert_eq!(session.documents().len(), 1);
        assert_eq!(session.doc().unwrap().uid, other_uid);
        assert!(!session.analysis_active());
        assert!(session.analysis_progress().is_none());
        assert!(cancellation.is_cancelled());
        assert_eq!(source.sheets.len(), 1);
        assert_eq!(session.doc().unwrap().wb.sheets.len(), 1);
        session.execute("cell.set", json!({"cell":"A1","input":"10"})).unwrap();
        session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
        let next = session.execute("analysis.start", json!({"params":params,"autoAdvance":false})).unwrap()["job"].clone();
        assert_ne!(next, job);
        session.execute("analysis.cancel", json!({"job":next})).unwrap();
    }
}
