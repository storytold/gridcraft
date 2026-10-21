//! Engine building blocks for multi-command actions such as macros: undo groups, the recording
//! buffer, document trust and chunked storage of workbook data in hidden names.

use serde_json::json;

use super::{s, v};
use crate::{EngineError, Session, Trust};

fn undo_labels(s: &Session) -> Vec<String> {
    s.doc().unwrap().undo.iter().map(|e| e.label.clone()).collect()
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

// ---------------------------------------------------------------- undo groups

#[test]
fn grouped_commands_are_one_undo_step() {
    let mut s = s();
    set(&mut s, "A1", "before");
    let r = s.grouped("Fill Three", |s| {
        set(s, "A1", "1");
        set(s, "A2", "2");
        s.execute("home.bold", json!({"range": "A1:A2"}))?;
        Ok(7)
    });
    assert_eq!(r.unwrap(), 7);
    assert_eq!(undo_labels(&s), ["Enter Cell", "Fill Three"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1").display(), "before");
    assert!(v(&s, "A2").is_empty());
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A2").display(), "2");
    assert!(!s.in_group(s.doc().unwrap().uid));
}

#[test]
fn a_failing_group_rolls_everything_back() {
    let mut s = s();
    set(&mut s, "B1", "base");
    set(&mut s, "A1", "undone");
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "C3"})).unwrap();
    let redo_before = s.doc().unwrap().redo.len();
    assert_eq!(redo_before, 1);
    let r: crate::Result<()> = s.grouped("Doomed", |s| {
        set(s, "B1", "changed");
        s.execute("selection.set", json!({"range": "D4"}))?;
        set(s, "B2", "new");
        Err(EngineError::Other("step 3 failed".into()))
    });
    assert!(r.unwrap_err().to_string().contains("step 3 failed"));
    assert_eq!(v(&s, "B1").display(), "base");
    assert!(v(&s, "B2").is_empty());
    assert_eq!(s.doc().unwrap().selection.a1(), "C3");
    assert_eq!(undo_labels(&s), ["Enter Cell"]);
    assert_eq!(s.doc().unwrap().redo.len(), redo_before, "the redo stack comes back too");
    // The recalculation follows the restored workbook.
    set(&mut s, "C1", "=B1&\"!\"");
    assert_eq!(v(&s, "C1").display(), "base!");
}

#[test]
fn groups_nest_and_the_outermost_makes_the_step() {
    let mut s = s();
    let r = s.grouped("Outer", |s| {
        set(s, "A1", "1");
        s.grouped("Inner", |s| {
            set(s, "A2", "2");
            Ok(())
        })?;
        // A failing inner group rolls back only its own changes; the outer one carries on.
        let inner: crate::Result<()> = s.grouped("Inner fails", |s| {
            set(s, "A3", "3");
            Err(EngineError::Other("no".into()))
        });
        assert!(inner.is_err());
        assert!(v(s, "A3").is_empty());
        set(s, "A4", "4");
        Ok(())
    });
    r.unwrap();
    assert_eq!(undo_labels(&s), ["Outer"]);
    assert_eq!(v(&s, "A2").display(), "2");
    s.execute("edit.undo", json!({})).unwrap();
    for c in ["A1", "A2", "A3", "A4"] {
        assert!(v(&s, c).is_empty(), "{c}");
    }
}

#[test]
fn an_unchanged_group_adds_no_step() {
    let mut s = s();
    set(&mut s, "A1", "x");
    s.grouped("Look Around", |s| {
        s.execute("selection.set", json!({"range": "B2"}))?;
        s.execute("cell.get", json!({"cell": "A1"}))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(undo_labels(&s), ["Enter Cell"]);
}

#[test]
fn a_group_spans_calls_and_blocks_undo_until_it_ends() {
    let mut s = s();
    set(&mut s, "A1", "start");
    let token = s.group_begin("Debug Macro").unwrap();
    // Each step is its own call, as a debugger makes them.
    set(&mut s, "A1", "step 1");
    set(&mut s, "A2", "step 2");
    let undo = s.execute("edit.undo", json!({}));
    assert!(matches!(undo, Err(EngineError::Disabled(..))), "{undo:?}");
    assert!(matches!(s.execute("edit.redo", json!({})), Err(EngineError::Disabled(..))));
    assert_eq!(s.execute("history.list", json!({})).unwrap()["grouping"], json!(true));
    assert!(s.group_end(token).unwrap());
    assert_eq!(s.execute("history.list", json!({})).unwrap()["grouping"], json!(false));
    assert_eq!(undo_labels(&s), ["Enter Cell", "Debug Macro"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1").display(), "start");
    assert!(v(&s, "A2").is_empty());
}

#[test]
fn rolling_back_a_spanning_group_restores_the_start() {
    let mut s = s();
    set(&mut s, "A1", "start");
    let token = s.group_begin("Debug Macro").unwrap();
    set(&mut s, "A1", "step 1");
    s.execute("home.insertSheet", json!({})).unwrap();
    s.group_rollback(token).unwrap();
    assert_eq!(v(&s, "A1").display(), "start");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(undo_labels(&s), ["Enter Cell"]);
}

#[test]
fn ending_an_outer_group_ends_the_inner_ones() {
    let mut s = s();
    let outer = s.group_begin("Outer").unwrap();
    let inner = s.group_begin("Inner").unwrap();
    set(&mut s, "A1", "1");
    s.group_end(outer).unwrap();
    assert_eq!(undo_labels(&s), ["Outer"]);
    assert!(s.group_end(inner).is_err(), "the inner group ended with its outer group");
    assert!(!s.in_group(s.doc().unwrap().uid));
}

#[test]
fn a_long_group_keeps_the_history_before_it() {
    let mut s = s();
    for i in 0..100 {
        set(&mut s, "A1", &i.to_string());
    }
    let first = s.doc().unwrap().undo.first().map(|e| e.wb.clone()).unwrap();
    s.grouped("Many Steps", |s| {
        for i in 0..250 {
            set(s, "B1", &i.to_string());
        }
        Ok(())
    })
    .unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.undo.len(), 100, "the undo limit still holds");
    assert_eq!(d.undo.last().map(|e| e.label.as_str()), Some("Many Steps"));
    // Only the oldest entry made room for the group's one step.
    assert!(!d.undo.iter().any(|e| std::sync::Arc::ptr_eq(&e.wb, &first)));
    assert_eq!(d.undo.iter().filter(|e| e.label == "Enter Cell").count(), 99);
}

#[test]
fn closing_a_document_drops_its_groups() {
    let mut s = s();
    let token = s.group_begin("Debug Macro").unwrap();
    let uid = token.doc();
    s.close_document(s.active_index());
    assert!(!s.in_group(uid));
    assert!(s.group_end(token).is_err());
    assert!(matches!(s.group_begin("x"), Err(EngineError::NoDocument)));
}

#[test]
fn a_group_owns_the_steps_of_commands_that_run_commands() {
    // Insert Copied Cells runs nested steps and folds them into its own undo step; inside a group
    // it must leave the history from before the group alone.
    let mut s = s();
    set(&mut s, "A1", "kept");
    set(&mut s, "A5", "copied");
    s.execute("edit.copy", json!({"range": "5:5"})).unwrap();
    s.grouped("Insert Inside", |s| {
        s.execute("selection.set", json!({"range": "A2"}))?;
        s.execute("home.insertCopiedCells", json!({}))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(v(&s, "A2").display(), "copied");
    assert_eq!(undo_labels(&s), ["Enter Cell", "Enter Cell", "Insert Inside"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "A2").is_empty());
    assert_eq!(v(&s, "A1").display(), "kept");
}

fn put(s: &mut Session, cell: &str, text: &str) -> crate::Result<()> {
    s.execute("cell.set", json!({"cell": cell, "input": text})).map(|_| ())
}

fn register_group_commands() {
    use crate::cmd::register_undoable_test_command as register;
    // Edits before its group are part of the step.
    register("test.editThenGroup", |s, _| {
        put(s, "A1", "outside")?;
        s.grouped("Inner", |s| put(s, "A2", "inside"))?;
        Ok(json!({}))
    });
    // A group that fails and rolls back, then an edit outside it.
    register("test.failThenEdit", |s, _| {
        let failed: crate::Result<()> = s.grouped("Doomed", |s| {
            put(s, "B1", "gone")?;
            Err(EngineError::Other("no".into()))
        });
        assert!(failed.is_err());
        put(s, "B2", "kept")?;
        Ok(json!({}))
    });
    // A command that is one group keeps the group's name.
    register("test.onlyGroup", |s, _| s.grouped("Named Step", |s| put(s, "D1", "x")).map(|_| json!({})));
    // An edit, then the end of the innermost group that was open before the command.
    register("test.endOpenGroup", |s, _| {
        put(s, "E1", "end")?;
        let token = s.groups.last().map(|g| crate::GroupToken { id: g.id, doc: g.doc });
        s.group_end(token.ok_or_else(|| EngineError::Other("no group".into()))?)?;
        Ok(json!({}))
    });
}

#[test]
fn a_command_that_runs_its_own_group_makes_one_step() {
    register_group_commands();
    let mut s = s();
    s.execute("test.onlyGroup", json!({})).unwrap();
    assert_eq!(undo_labels(&s), ["Named Step"]);
    s.execute("edit.undo", json!({})).unwrap();
    set(&mut s, "C1", "base");
    s.execute("test.editThenGroup", json!({})).unwrap();
    assert_eq!(undo_labels(&s), ["Enter Cell", "test.editThenGroup"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "A1").is_empty() && v(&s, "A2").is_empty());
    assert_eq!(v(&s, "C1").display(), "base");

    s.execute("test.failThenEdit", json!({})).unwrap();
    assert!(v(&s, "B1").is_empty());
    assert_eq!(v(&s, "B2").display(), "kept");
    assert_eq!(undo_labels(&s), ["Enter Cell", "test.failThenEdit"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "B2").is_empty());
}

/// Fills the undo history to the limit with edits of Z1 ("v0" to "v99"), and returns its snapshots.
fn fill_history(s: &mut Session) -> Vec<std::sync::Arc<gridcraft_model::Workbook>> {
    for i in 0..crate::UNDO_LIMIT {
        set(s, "Z1", &format!("v{i}"));
    }
    let d = s.doc().unwrap();
    assert_eq!(d.undo.len(), crate::UNDO_LIMIT);
    d.undo.iter().map(|e| e.wb.clone()).collect()
}

/// The history is still full, ends with one step labelled `label`, and kept every older step but
/// the oldest.
fn assert_one_step_replaced_the_oldest(s: &Session, before: &[std::sync::Arc<gridcraft_model::Workbook>], label: &str) {
    let d = s.doc().unwrap();
    assert_eq!(d.undo.len(), crate::UNDO_LIMIT);
    assert_eq!(d.undo.last().map(|e| e.label.as_str()), Some(label));
    assert_eq!(d.undo.iter().filter(|e| e.label == label).count(), 1);
    let kept = d.undo.iter().take(crate::UNDO_LIMIT - 1);
    assert!(kept.zip(before.iter().skip(1)).all(|(e, wb)| std::sync::Arc::ptr_eq(&e.wb, wb)));
}

/// Undo goes back to the state before the step, then to the edit before it; Redo comes back.
fn assert_undo_redo(s: &mut Session, cells: &[(&str, &str)]) {
    let after = |s: &Session| cells.iter().all(|(c, t)| v(s, c).display() == *t);
    let gone = |s: &Session| cells.iter().all(|(c, _)| v(s, c).is_empty());
    assert!(after(s));
    s.execute("edit.undo", json!({})).unwrap();
    assert!(gone(s) && v(s, "Z1").display() == "v99", "the first Undo restores the state before the command");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(gone(s) && v(s, "Z1").display() == "v98", "the second Undo restores the edit before it");
    s.execute("edit.redo", json!({})).unwrap();
    assert!(gone(s) && v(s, "Z1").display() == "v99");
    s.execute("edit.redo", json!({})).unwrap();
    assert!(after(s) && v(s, "Z1").display() == "v99");
}

#[test]
fn a_group_only_command_makes_one_step_when_the_history_is_full() {
    register_group_commands();
    let mut s = s();
    let before = fill_history(&mut s);
    s.execute("test.onlyGroup", json!({})).unwrap();
    assert_one_step_replaced_the_oldest(&s, &before, "Named Step");
    assert_undo_redo(&mut s, &[("D1", "x")]);
}

#[test]
fn an_edit_then_a_group_makes_one_step_when_the_history_is_full() {
    register_group_commands();
    let mut s = s();
    let before = fill_history(&mut s);
    s.execute("test.editThenGroup", json!({})).unwrap();
    assert_one_step_replaced_the_oldest(&s, &before, "test.editThenGroup");
    assert_undo_redo(&mut s, &[("A1", "outside"), ("A2", "inside")]);
}

#[test]
fn a_group_ended_at_the_history_limit_replaces_only_the_oldest_step() {
    register_group_commands();
    // A command ends a group that was open before it.
    let mut s = s();
    let before = fill_history(&mut s);
    let _open = s.group_begin("Debug Macro").unwrap();
    set(&mut s, "E2", "step");
    assert_eq!(s.doc().unwrap().undo.len(), crate::UNDO_LIMIT);
    s.execute("test.endOpenGroup", json!({})).unwrap();
    assert!(!s.in_group(s.doc().unwrap().uid));
    assert_one_step_replaced_the_oldest(&s, &before, "Debug Macro");
    assert_undo_redo(&mut s, &[("E1", "end"), ("E2", "step")]);

    // A group ended directly, outside any command, across calls.
    let mut s = super::s();
    let before = fill_history(&mut s);
    let token = s.group_begin("Spanning").unwrap();
    set(&mut s, "F1", "one");
    set(&mut s, "F2", "two");
    assert!(s.group_end(token).unwrap());
    assert_one_step_replaced_the_oldest(&s, &before, "Spanning");
    assert_undo_redo(&mut s, &[("F1", "one"), ("F2", "two")]);
}

// ---------------------------------------------------------------- recording buffer

fn recorded_ids(s: &Session) -> Vec<String> {
    s.recorder.as_ref().unwrap().steps.iter().map(|st| st.id.clone()).collect()
}

#[test]
fn the_recorder_keeps_top_level_calls_with_where_they_ran() {
    let mut s = s();
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "B2:C3"})).unwrap();
    s.start_recording();
    assert!(s.is_recording());
    assert_eq!(s.depth(), 0);
    set(&mut s, "B2", "10");
    s.execute("cell.get", json!({"cell": "B2"})).unwrap(); // a query: not recorded
    assert!(s.execute("sheet.move", json!({})).is_err()); // failed: not recorded
    s.execute("view.customViews", json!({"save": "Mine"})).unwrap();
    // Show runs sheet.activate, selection.set and view.zoom inside it: one step.
    s.execute("view.customViews", json!({"show": "Mine"})).unwrap();
    assert_eq!(recorded_ids(&s), ["cell.set", "view.customViews", "view.customViews"]);
    let first = &s.recorder.as_ref().unwrap().steps[0];
    let d = s.doc().unwrap();
    assert_eq!((first.doc, first.sheet), (d.uid, d.wb.active_sheet));
    assert_eq!(first.sheet_name, d.wb.active().unwrap().name);
    assert_eq!(first.selection.a1(), "B2:C3", "the selection before the command ran");
    assert_eq!(first.params, json!({"cell": "B2", "input": "10"}));
    assert_eq!(s.depth(), 0);
    let rec = s.stop_recording().unwrap();
    assert_eq!(rec.dropped, 0);
    set(&mut s, "A1", "after");
    assert!(s.recorder.is_none() && !s.is_recording());
}

#[test]
fn the_recorder_survives_journal_trimming() {
    let mut s = s();
    s.start_recording();
    for i in 0..10_050u32 {
        s.execute("selection.set", json!({"range": format!("A{}", i % 50 + 1)})).unwrap();
    }
    set(&mut s, "C1", "last");
    assert!(s.journal.len() < 10_051, "the journal was trimmed");
    let steps = &s.recorder.as_ref().unwrap().steps;
    assert_eq!(steps.len(), 10_051);
    assert_eq!(steps[0].params, json!({"range": "A1"}));
    assert_eq!(steps[10_050].id, "cell.set");
}

#[test]
fn the_recorder_has_a_cap() {
    let mut rec = crate::record::Recorder::default();
    let step = crate::record::RecordedStep {
        doc: 1,
        sheet: 0,
        sheet_name: "Sheet1".into(),
        selection: crate::Selection::default(),
        id: "cell.set".into(),
        params: json!({}),
    };
    for _ in 0..crate::record::RECORD_LIMIT + 3 {
        rec.push(step.clone());
    }
    assert_eq!(rec.steps.len(), crate::record::RECORD_LIMIT);
    assert_eq!(rec.dropped, 3);
}

#[test]
fn passwords_stay_out_of_the_recording_buffer() {
    const PW: &str = "synthetic-pw-5521";
    let mut s = s();
    s.start_recording();
    s.execute("review.protectSheet", json!({"password": PW, "formatCells": true})).unwrap();
    s.execute("review.unprotectSheet", json!({"password": PW})).unwrap();
    let rec = s.stop_recording().unwrap();
    let text = serde_json::to_string(&rec.steps).unwrap();
    assert!(!text.contains(PW), "{text}");
    assert_eq!(rec.steps[0].params, json!({"formatCells": true}));
}

#[test]
fn record_actions_is_unchanged_by_the_buffer() {
    // Automate › Record Actions keeps recording through the journal; the buffer is separate.
    let mut s = s();
    s.execute("automate.recordActions", json!({})).unwrap();
    assert!(!s.is_recording());
    set(&mut s, "A1", "typed");
    let r = s.execute("automate.recordActions", json!({"stop": true, "name": "Typed"})).unwrap();
    assert_eq!(r["steps"], 1);
    assert!(s.recorder.is_none());
}

// ---------------------------------------------------------------- trust

fn open_json(s: &mut Session, wb: &gridcraft_model::Workbook) {
    let bytes = serde_json::to_vec(wb).unwrap();
    s.execute("file.open", json!({"name": "theirs.json", "base64": crate::io::base64_encode(&bytes)})).unwrap();
}

fn trust(s: &Session) -> Trust {
    s.doc().unwrap().trust
}

#[test]
fn documents_know_where_they_came_from() {
    let mut s = s();
    assert_eq!(trust(&s), Trust::Local);
    assert!(Trust::Local.allows_code() && !Trust::FromFile.allows_code());
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    assert_eq!(trust(&s), Trust::Local);
    assert_eq!(s.execute("document.inspect", json!({})).unwrap()["trust"], "local");
    open_json(&mut s, &gridcraft_model::Workbook::new());
    assert_eq!(trust(&s), Trust::FromFile);
    assert_eq!(s.execute("document.inspect", json!({})).unwrap()["trust"], "fromFile");
    // A second window on the same workbook is the same workbook.
    s.execute("view.newWindow", json!({})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile);
    // Moving and copying sheets inside it changes nothing.
    s.execute("sheet.move", json!({"to": 0, "copy": true})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile);
}

#[test]
fn pasting_from_a_file_carries_its_trust() {
    let mut s = s(); // document 0: Local
    open_json(&mut s, &gridcraft_model::Workbook::new()); // document 1: FromFile
    let theirs = s.active_index();
    set(&mut s, "A1", "=1+1");
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.new_workbook();
    let mine = s.active_index();
    // Values carry nothing that runs.
    s.execute("edit.pasteSpecial", json!({"what": "values", "at": "B1"})).unwrap();
    assert_eq!(trust(&s), Trust::Local);
    // Formats carry cell styles and merged areas, nothing that runs.
    s.execute("edit.pasteSpecial", json!({"what": "formats", "at": "B3"})).unwrap();
    assert_eq!(trust(&s), Trust::Local);
    // A linked paste of values still writes formulas.
    s.execute("edit.pasteSpecial", json!({"what": "values", "link": true, "at": "B2"})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile);
    s.new_workbook();
    s.execute("edit.paste", json!({"at": "C1"})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile);
    assert_eq!(v(&s, "C1").display(), "2");

    // Pasting within a workbook changes nothing, and content from a local workbook leaves the
    // destination as it was.
    s.set_active(theirs);
    s.execute("edit.paste", json!({"at": "A3"})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile, "pasting within a workbook");
    s.new_workbook();
    set(&mut s, "A1", "=3");
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.set_active(mine);
    s.execute("edit.paste", json!({"at": "D1"})).unwrap();
    assert_eq!(trust(&s), Trust::FromFile, "still untrusted from the earlier paste");
    s.new_workbook();
    s.execute("edit.paste", json!({"at": "D1"})).unwrap();
    assert_eq!(trust(&s), Trust::Local);
    assert_eq!(Trust::Local.receive(Trust::FromFile), Trust::FromFile);
    assert_eq!(Trust::FromFile.receive(Trust::Local), Trust::FromFile);
    assert_eq!(Trust::Local.receive(Trust::Local), Trust::Local);
}
