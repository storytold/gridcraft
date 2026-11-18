//! The headless caller must not report completion for a missing-input dialog.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use gridcraft_engine::{EngineError, Session, UiRequest};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn incomplete_headless_commands_fail_without_changing_workbook_or_history() {
    for command in ["home.formatCells", "insert.picture", "insert.link", "home.conditionalFormat", "data.validation", "formulas.defineName"] {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "2"})).unwrap();
        s.execute("edit.undo", json!({})).unwrap();
        let d = s.doc().unwrap();
        let (before, undo, redo, dirty) = (d.wb.clone(), d.undo.len(), d.redo.len(), d.is_dirty());
        s.start_recording();

        let error = s.execute_headless(command, json!({})).expect_err(command);
        assert!(matches!(error, EngineError::BadParams { .. }), "{command}: {error}");
        let message = error.to_string();
        assert!(message.contains("headless") && message.contains("parameters"), "{command}: {message}");
        let d = s.doc().unwrap();
        assert!(Arc::ptr_eq(&before, &d.wb), "{command} changed the workbook");
        assert_eq!((d.undo.len(), d.redo.len(), d.is_dirty()), (undo, redo, dirty), "{command}");
        assert!(s.stop_recording().unwrap().steps.is_empty(), "{command} was recorded");
        assert!(s.take_ui_requests().is_empty(), "{command} left a UI request");
    }
}

#[test]
fn complete_headless_commands_still_apply_their_parameters() {
    // A one-pixel red PNG made from an IHDR, a compressed RGB scanline and an IEND chunk.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    for (command, params) in [
        ("home.formatCells", json!({"range": "A1", "style": {"font": {"bold": true}}})),
        ("insert.picture", json!({"base64": PNG, "at": "A1"})),
        ("insert.link", json!({"cell": "A1", "target": "https://example.org"})),
        ("home.conditionalFormat", json!({"range": "A1", "rule": {"type": "cellIs", "operator": "greater", "value": 0}})),
        ("data.validation", json!({"range": "A1", "type": "whole", "formula1": "0", "formula2": "10"})),
        ("formulas.defineName", json!({"name": "WatchTotal", "refersTo": "Sheet1!$A$1"})),
    ] {
        let mut s = session();
        s.execute_headless(command, params).unwrap_or_else(|e| panic!("{command}: {e}"));
        assert!(s.doc().unwrap().is_dirty(), "{command} did not apply its parameters");
        assert!(s.take_ui_requests().is_empty(), "{command} left a UI request");
    }
}

#[test]
fn explicit_inspection_and_noncompletion_results_are_preserved() {
    let mut s = session();
    for (command, key) in [("data.solver", "model"), ("data.analysisToolPak", "tools"), ("file.options", "calculation")] {
        let result = s.execute_headless(command, json!({})).unwrap();
        assert!(result.get(key).is_some(), "{command}: {result}");
        assert!(s.take_ui_requests().is_empty());
    }
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    let result = s.execute_headless("file.close", json!({})).unwrap();
    assert_eq!(result, json!({"closed": false, "dirty": true}));
    assert_eq!(s.documents().len(), 1);
    assert!(s.take_ui_requests().is_empty());
}

#[test]
fn old_dialogs_do_not_turn_a_completed_null_result_into_an_error() {
    let mut s = session();
    s.ui_requests.push(UiRequest::Dialog("open".into(), json!({})));
    s.ui_requests.push(UiRequest::Dialog("insertLink".into(), json!({})));
    let result = s.execute_headless("home.formatCells", json!({"range": "A1", "style": {"font": {"bold": true}}})).unwrap();
    assert!(result.is_null());
    assert!(s.doc().unwrap().is_dirty());
    assert!(s.take_ui_requests().is_empty());
}

#[test]
fn command_errors_are_not_replaced_by_dialog_errors() {
    let mut s = session();
    s.ui_requests.push(UiRequest::Dialog("open".into(), json!({})));
    let error = s.execute_headless("data.validation", json!({"type": "no-such-type"})).unwrap_err();
    assert!(error.to_string().contains("unknown type"), "{error}");
    assert!(s.take_ui_requests().is_empty());
}
