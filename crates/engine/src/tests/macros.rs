//! Macros: running, input checks, the footprint dry run, the policy and the limits.

use std::sync::Arc;

use serde_json::{Value as Json, json};

use super::{s, v};
use crate::{EngineError, Session};

fn undo_labels(s: &Session) -> Vec<String> {
    s.doc().unwrap().undo.iter().map(|e| e.label.clone()).collect()
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

/// Puts a macro into the active workbook (replacing one with the same name).
pub(super) fn put(s: &mut Session, m: Json) {
    let m = crate::macros::parse_macro(&m).unwrap();
    let d = s.doc_mut().unwrap();
    let mut wb = (*d.wb).clone();
    wb.macros.retain(|x| !x.name.eq_ignore_ascii_case(&m.name));
    wb.macros.push(m);
    d.wb = Arc::new(wb);
    d.saved = d.wb.clone();
}

/// Names in B2:B5 and amounts in C2:C5 under a header row.
fn data(s: &mut Session) {
    for (cell, input) in [("B2", "Name"), ("C2", "Amount"), ("B3", "Ann"), ("C3", "120"), ("B4", "Bo"), ("C4", "80"), ("B5", "Cy"), ("C5", "200")] {
        set(s, cell, input);
    }
}

/// A macro that writes a label and a total next to its input and bolds the header.
fn totals() -> Json {
    json!({
        "name": "Totals",
        "inputs": [{"name": "Input", "header": true, "cols": [2, 2], "colTypes": ["text", "number"]}],
        "output": {"anchor": "Input.right(2).cell(1,2)"},
        "steps": [
            {"command": "cell.set", "params": {"cell": "Output", "input": "Total"}},
            {"command": "cell.set", "params": {"cell": "Output.offset(1,0)", "input": "=SUM({{Input.data.col(2)}})"}},
            {"command": "home.bold", "params": {"range": "Input.row(1)"}}
        ]
    })
}

fn bold(s: &Session, a: &str) -> bool {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(gridcraft_core::CellRef::parse(a).unwrap())).font.bold
}

#[test]
fn a_run_resolves_tokens_and_is_one_undo_step() {
    let mut s = s();
    data(&mut s);
    put(&mut s, totals());
    let before = undo_labels(&s);
    let r = s.execute("macro.run", json!({"name": "totals", "inputs": {"Input": "B2:C5"}})).unwrap();
    assert_eq!(r["ran"], 3);
    assert_eq!(r["inputs"], json!({"Input": "B2:C5"}));
    assert_eq!(v(&s, "E2").display(), "Total");
    assert_eq!(v(&s, "E3").display(), "400");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().input_text(gridcraft_core::CellRef::parse("E3").unwrap()), "=SUM(C3:C5)");
    assert!(bold(&s, "B2") && bold(&s, "C2"));
    let after = undo_labels(&s);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.last().unwrap(), "Run Macro: Totals");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "E2").is_empty() && v(&s, "E3").is_empty() && !bold(&s, "B2"));
    assert_eq!(undo_labels(&s), before);
    // The selection is the default input.
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("macro.run", json!({"name": "Totals"})).unwrap();
    assert_eq!(v(&s, "E3").display(), "400");
}

#[test]
fn inputs_are_checked_with_messages_that_name_the_cells() {
    let mut s = s();
    data(&mut s);
    set(&mut s, "C4", "eighty");
    set(&mut s, "C5", "TRUE");
    put(&mut s, totals());
    let start = s.doc().unwrap().wb.clone();
    let err = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5"})).unwrap_err().to_string();
    assert!(err.contains("Input column 2 (Amount) must hold numbers; C4 holds text, C5 holds TRUE or FALSE."), "{err}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start), "nothing ran");

    // Shape: width, rows, a single cell, blanks, dates.
    let mut m = totals();
    m["inputs"][0] = json!({"name": "Input", "header": true, "rows": [1, 2], "cols": [3, 3], "nonEmpty": true, "colTypes": ["any", "date"]});
    put(&mut s, m);
    set(&mut s, "B4", "");
    let err = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5"})).unwrap_err().to_string();
    for needle in [
        "Input needs at most 2 data rows; B2:C5 has 3.",
        "Input needs exactly 3 columns; B2:C5 has 2.",
        "Input column 2 (Amount) must hold dates; C3 holds a number that isn't a date, C4 holds text, C5 holds TRUE or FALSE.",
        "Input must have no blank cells; B4 is blank.",
    ] {
        assert!(err.contains(needle), "missing `{needle}` in {err}");
    }
    let mut m = totals();
    m["inputs"][0] = json!({"name": "Input", "kind": "cell"});
    put(&mut s, m);
    let err = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5"})).unwrap_err().to_string();
    assert!(err.contains("Input must be a single cell; B2:C5 is 8 cells."), "{err}");
    // A second input with no range, and a bad reference.
    put(&mut s, json!({"name": "Two", "inputs": [{"name": "Input"}, {"name": "Rates"}], "steps": []}));
    let err = s.execute("macro.run", json!({"name": "Two"})).unwrap_err().to_string();
    assert!(err.contains("Rates needs a range"), "{err}");
    let err = s.execute("macro.run", json!({"name": "Two", "inputs": {"Rates": "Nowhere!A1"}})).unwrap_err().to_string();
    assert!(err.contains("Rates: There is no sheet named 'Nowhere'."), "{err}");
    // Whole columns shrink to the cells in use.
    put(
        &mut s,
        json!({"name": "Col", "inputs": [{"name": "Input", "rows": [1, 10]}], "steps": [{"command": "cell.set", "params": {"cell": "Input.below", "input": "end"}}], "output": {"onConflict": "overwrite"}}),
    );
    s.execute("macro.run", json!({"name": "Col", "inputs": "B:B"})).unwrap();
    assert_eq!(v(&s, "B6").display(), "end");
    assert!(s.execute("macro.run", json!({"name": "nope"})).unwrap_err().to_string().contains("no macro called 'nope'"));
}

#[test]
fn an_error_rolls_everything_back_and_names_the_step() {
    let mut s = s();
    data(&mut s);
    let mut m = totals();
    m["output"]["onConflict"] = json!("overwrite");
    m["steps"][2] = json!({"type": "repeat", "over": "Input.data", "body": [
        {"command": "home.italic", "params": {"range": "Row"}},
        {"command": "cell.set", "params": {"cell": "Row.cell(1,9)", "input": "=1/"}}
    ]});
    put(&mut s, m);
    let (before, undo) = (s.doc().unwrap().wb.clone(), undo_labels(&s));
    let err = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5"})).unwrap_err().to_string();
    assert!(err.starts_with("Macro 'Totals' stopped. Step 3.2 (cell.set):"), "{err}");
    assert!(err.ends_with("Nothing was changed."), "{err}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &before));
    assert_eq!(undo_labels(&s), undo);
    assert!(s.doc().unwrap().redo.is_empty());
}

#[test]
fn conflicts_ask_fail_or_overwrite() {
    let mut s = s();
    data(&mut s);
    set(&mut s, "E3", "keep me");
    // A formula that reads what the macro writes: its value changes, but it isn't written.
    set(&mut s, "H1", "=E3");
    put(&mut s, totals());
    let start = s.doc().unwrap().wb.clone();
    let r = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5"}));
    let Err(EngineError::Confirm(q)) = r else { panic!("{r:?}") };
    assert!(q.contains("would write over cells that hold data (E3 on Sheet1)"), "{q}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start), "programmatic runs default to failing");
    let err = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5", "onConflict": "fail"})).unwrap_err().to_string();
    assert!(err.contains("Nothing was changed"), "{err}");
    assert!(s.execute("macro.run", json!({"name": "Totals", "onConflict": "sideways"})).is_err());

    // macro.check shows the footprint and the conflict, and changes nothing.
    let journal = s.journal.len();
    let sel = s.doc().unwrap().selection.clone();
    let c = s.execute("macro.check", json!({"name": "Totals", "inputs": "B2:C5"})).unwrap();
    assert_eq!(c["ok"], true);
    assert_eq!(c["conflicts"], 1);
    assert_eq!(c["output"], "E2");
    assert_eq!(c["footprint"]["sheets"][0]["cells"], json!(["E2:E3"]));
    assert_eq!(c["footprint"]["sheets"][0]["conflicts"], json!(["E3"]));
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start));
    assert_eq!((s.journal.len(), &s.doc().unwrap().selection), (journal, &sel));

    // The answer to the question overwrites.
    s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5", "overwrite": true})).unwrap();
    assert_eq!(v(&s, "E3").display(), "400");
    assert_eq!(v(&s, "H1").display(), "400");
}

#[test]
fn shifting_makes_room_and_is_checked_again() {
    for (rule, moved) in [("shiftDown", ["E4", "E5"]), ("shiftRight", ["F2", "F3"])] {
        let mut s = s();
        data(&mut s);
        set(&mut s, "E2", "old 1");
        set(&mut s, "E3", "old 2");
        put(&mut s, totals());
        let r = s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5", "onConflict": rule})).unwrap();
        assert_eq!(r["shifted"], rule);
        assert_eq!((v(&s, "E2").display(), v(&s, "E3").display()), ("Total".into(), "400".into()), "{rule}");
        assert_eq!((v(&s, moved[0]).display(), v(&s, moved[1]).display()), ("old 1".into(), "old 2".into()), "{rule}");
        // The shift and the run are one undo step.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(v(&s, "E2").display(), "old 1", "{rule}");
        assert!(v(&s, moved[1]).is_empty(), "{rule}");
    }
    // A cell written with what it already holds isn't a conflict, but it moves with the others,
    // so rows stay together.
    let mut s = super::s();
    for (cell, input) in [("F2", "x"), ("G2", "b"), ("F3", "f3"), ("G3", "g3")] {
        set(&mut s, cell, input);
    }
    put(
        &mut s,
        json!({"name": "Pair", "output": {"onConflict": "shiftDown"}, "steps": [
            {"command": "cell.set", "params": {"cell": "F2", "input": "a"}},
            {"command": "cell.set", "params": {"cell": "G2", "input": "b"}}
        ]}),
    );
    let c = s.execute("macro.check", json!({"name": "Pair"})).unwrap();
    assert_eq!((c["conflicts"].as_u64(), &c["footprint"]["sheets"][0]["cells"]), (Some(1), &json!(["F2:G2"])));
    s.execute("macro.run", json!({"name": "Pair"})).unwrap();
    let row = |s: &Session, r: u32| (v(s, &format!("F{r}")).display(), v(s, &format!("G{r}")).display());
    assert_eq!(row(&s, 2), ("a".into(), "b".into()));
    assert_eq!(row(&s, 3), ("x".into(), "b".into()));
    assert_eq!(row(&s, 4), ("f3".into(), "g3".into()));
    // An anchor that moves with the shifted data still lands on it: the second check refuses.
    let mut s = super::s();
    data(&mut s);
    set(&mut s, "E2", "old");
    s.execute("formulas.defineName", json!({"name": "Target", "refersTo": "=Sheet1!$E$2"})).unwrap();
    put(
        &mut s,
        json!({"name": "Named", "steps": [{"command": "cell.set", "params": {"cell": "Name:Target", "input": "new"}}], "output": {"onConflict": "shiftDown"}}),
    );
    let start = s.doc().unwrap().wb.clone();
    let err = s.execute("macro.run", json!({"name": "Named"})).unwrap_err().to_string();
    assert!(err.contains("shifting the data didn't make room (E3 on Sheet1)"), "{err}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start));
}

#[test]
fn closing_the_debugged_workbook_ends_the_debugger() {
    // `file.close` is not an edit, so the debugger's lock lets it through; the group went with
    // the document, and so must the debugger, or the next workbook could not debug anything and
    // the state would report that no document is open.
    let mut s = s();
    data(&mut s);
    put(&mut s, totals());
    s.execute("macro.debug", json!({"action": "start", "name": "Totals", "inputs": "B2:C5", "breakpoints": ["1"]})).unwrap();
    assert!(s.macro_debug.is_some());
    s.execute("file.close", json!({"force": true})).unwrap();
    assert!(s.macro_debug.is_none(), "the debugger ended with its workbook");
    s.new_workbook();
    data(&mut s);
    put(&mut s, totals());
    s.execute("macro.debug", json!({"action": "start", "name": "Totals", "inputs": "B2:C5", "breakpoints": ["1"]})).unwrap();
    assert!(s.execute("macro.debug", json!({})).is_ok(), "the new workbook can be debugged");
    s.execute("macro.debug", json!({"action": "stop"})).unwrap();
}

#[test]
fn a_shift_that_would_move_an_input_is_refused() {
    // A shift inserts cells over the conflicting block, which moves the block's columns (or rows)
    // from there on. When one of the macro's own inputs lies in the way, its data would move and
    // shear while the run keeps the old coordinates, so the shift is refused and nothing changes.
    let mut s = s();
    data(&mut s);
    set(&mut s, "B1", "Old title");
    put(
        &mut s,
        json!({"name": "Title", "inputs": [{"name": "Input", "header": true}], "steps": [
            {"command": "cell.set", "params": {"cell": "Input.cell(0,1)", "input": "Report"}}
        ]}),
    );
    let start = s.doc().unwrap().wb.clone();
    let err = s.execute("macro.run", json!({"name": "Title", "inputs": "B2:C5", "onConflict": "shiftDown"})).unwrap_err().to_string();
    assert!(err.contains("would move Input (") && err.contains("B2:C5"), "{err}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start), "nothing changed");
    assert_eq!((v(&s, "B1").display(), v(&s, "B3").display()), ("Old title".into(), "Ann".into()));
    // Writing left of the input and shifting right would move it too.
    put(
        &mut s,
        json!({"name": "Left", "inputs": [{"name": "Input", "header": true}], "steps": [
            {"command": "cell.set", "params": {"cell": "Input.offset(1,-1).cell(1,1)", "input": "x"}}
        ]}),
    );
    set(&mut s, "A3", "in the way");
    let err = s.execute("macro.run", json!({"name": "Left", "inputs": "B2:C5", "onConflict": "shiftRight"})).unwrap_err().to_string();
    assert!(err.contains("would move Input"), "{err}");
    // The output counts as well.
    put(
        &mut s,
        json!({"name": "Out", "inputs": [{"name": "Input", "header": true}], "output": {"anchor": "Input.below(1)"}, "steps": [
            {"command": "cell.set", "params": {"cell": "Input.cell(0,1)", "input": "Report"}}
        ]}),
    );
    let err = s.execute("macro.run", json!({"name": "Out", "inputs": "B2:C5", "onConflict": "shiftDown"})).unwrap_err().to_string();
    assert!(err.contains("would move Input"), "{err}");
    // A shift that moves nothing of the macro's own still works, over the block of every written
    // cell, so a row the macro writes stays together.
    set(&mut s, "H1", "elsewhere");
    put(
        &mut s,
        json!({"name": "Wide", "inputs": [{"name": "Input", "header": true}], "steps": [
            {"command": "cell.set", "params": {"cell": "H1", "input": "over"}},
            {"command": "cell.set", "params": {"cell": "H9", "input": "far"}}
        ]}),
    );
    s.execute("macro.run", json!({"name": "Wide", "inputs": "B2:C5", "onConflict": "shiftDown"})).unwrap();
    assert_eq!((v(&s, "H1").display(), v(&s, "H9").display(), v(&s, "H10").display()), ("over".into(), "far".into(), "elsewhere".into()));
    assert_eq!(v(&s, "B3").display(), "Ann", "the input did not move");
}

#[test]
fn a_shift_is_refused_when_the_footprint_cannot_follow_every_write() {
    // The touch log keeps at most a million writes; past that, a write that kept a cell's value
    // is missing from the footprint, so a shift could split a row the macro writes. The footprint
    // says so and the shift is refused (overwrite, with no dry run, still runs).
    let mut s = s();
    set(&mut s, "A1", "x");
    set(&mut s, "E2", "old");
    put(
        &mut s,
        json!({"name": "Flood", "steps": [
            {"command": "edit.fillDown", "params": {"range": "A1:A1000001"}},
            {"command": "cell.set", "params": {"cell": "E2", "input": "new"}}
        ]}),
    );
    let c = s.execute("macro.check", json!({"name": "Flood"})).unwrap();
    assert_eq!(c["footprint"]["logFull"], true, "{c}");
    let err = s.execute("macro.run", json!({"name": "Flood", "onConflict": "shiftDown"})).unwrap_err().to_string();
    assert!(err.contains("more cells than the footprint can follow"), "{err}");
    assert_eq!(v(&s, "E2").display(), "old", "nothing changed");
}

#[test]
fn a_dry_run_leaves_a_clean_workbook_clean() {
    // `view.zoom` and `sheet.activate` move the saved mark to the current workbook on a clean
    // document; the dry run's rollback puts the workbook back, so the mark must come back too.
    let mut s = s();
    data(&mut s);
    let d = s.doc_mut().unwrap();
    d.saved = d.wb.clone();
    assert!(!s.doc().unwrap().is_dirty());
    put(&mut s, json!({"name": "Look", "steps": [{"command": "view.zoom", "params": {"percent": 120}}]}));
    s.execute("macro.check", json!({"name": "Look"})).unwrap();
    assert!(!s.doc().unwrap().is_dirty(), "a dry run changed nothing");
    // A run refused by the conflict rule changed nothing either.
    put(&mut s, totals());
    set(&mut s, "E3", "taken");
    let d = s.doc_mut().unwrap();
    d.saved = d.wb.clone();
    assert!(s.execute("macro.run", json!({"name": "Totals", "inputs": "B2:C5", "onConflict": "fail"})).is_err());
    assert!(!s.doc().unwrap().is_dirty());
}

#[test]
fn a_dry_run_puts_the_journal_back_even_after_it_trimmed_it() {
    // Every nested command a dry run executes is journaled, so a long macro pushes the journal
    // past its limit and the journal drops its oldest entries. Putting the journal back by its
    // old length would then keep entries from the dry run and point a recording at the wrong
    // place; the restore counts what was dropped.
    let mut s = s();
    data(&mut s);
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    let journal = s.journal.clone();
    s.recording = Some(journal.len().saturating_sub(1));
    put(
        &mut s,
        json!({"name": "Long", "steps": [{"type": "repeat", "over": "A1:A200", "body": [
            {"type": "repeat", "over": "A1:A60", "body": [{"command": "cell.set", "params": {"cell": "Row.cell(1,1)", "input": "x"}}]}
        ]}]}),
    );
    s.execute("macro.check", json!({"name": "Long"})).unwrap();
    let sets = |j: &[(String, Json)]| j.iter().filter(|(id, _)| id == "cell.set").count();
    assert!(sets(&s.journal) <= sets(&journal), "no dry-run entry stays");
    assert!(s.journal.len() <= journal.len(), "{} entries, had {}", s.journal.len(), journal.len());
    assert!(journal.ends_with(&s.journal), "what stays is the journal's own tail");
    assert!(s.recording.is_some_and(|r| r <= s.journal.len()), "{:?}", s.recording);
    // A short dry run leaves the journal exactly as it was.
    put(&mut s, json!({"name": "Short", "steps": [{"command": "cell.set", "params": {"cell": "H1", "input": "x"}}]}));
    let (journal, recording) = (s.journal.clone(), s.recording);
    s.execute("macro.check", json!({"name": "Short"})).unwrap();
    assert_eq!((&s.journal, s.recording), (&journal, recording));
}

#[test]
fn the_answer_to_the_question_counts_however_ask_was_given() {
    let mut s = s();
    data(&mut s);
    put(&mut s, totals());
    set(&mut s, "E3", "taken");
    let p = json!({"name": "Totals", "inputs": "B2:C5", "onConflict": "ask"});
    assert!(matches!(s.execute("macro.run", p.clone()), Err(EngineError::Confirm(_))));
    let mut answered = p;
    answered["overwrite"] = json!(true);
    s.execute("macro.run", answered).unwrap();
    assert_eq!(v(&s, "E3").display(), "400");
}

#[test]
fn the_step_limit_is_one_budget_across_the_dry_run_and_the_run() {
    // 300 x 300 passes is under the limit for one phase; the dry run and the run together are not.
    let mut s = s();
    put(
        &mut s,
        json!({"name": "Spin", "steps": [{"type": "repeat", "over": "A1:A300", "body": [
            {"type": "repeat", "over": "A1:A300", "body": [{"type": "if", "cell": "A1", "op": "empty"}]}
        ]}]}),
    );
    let err = s.execute("macro.run", json!({"name": "Spin"})).unwrap_err().to_string();
    assert!(err.contains("more than 100000 steps"), "{err}");
    // With no dry run (overwrite), the same macro fits.
    s.execute("macro.run", json!({"name": "Spin", "onConflict": "overwrite"})).unwrap();
}

#[test]
fn the_footprint_covers_other_sheets_objects_and_edges() {
    let mut s = s();
    data(&mut s);
    s.execute("home.insertSheet", json!({"name": "Notes"})).unwrap();
    set(&mut s, "A1", "note");
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("insert.chart", json!({"range": "B2:C5", "type": "column", "at": "H2"})).unwrap();
    let chart = s.doc().unwrap().wb.sheet(0).unwrap().charts[0].id;
    put(
        &mut s,
        json!({"name": "Wide", "output": {"onConflict": "fail"}, "steps": [
            {"command": "cell.set", "params": {"cell": "Notes!A1", "input": "x"}},
            {"command": "cell.set", "params": {"cell": "Notes!B2", "input": "y"}},
            {"command": "home.insertSheet", "params": {"name": "Extra"}},
            {"command": "sheet.activate", "params": {"sheet": "Sheet1"}},
            {"command": "insert.shape", "params": {"kind": "ellipse", "at": "J2"}},
            {"command": "object.delete", "params": {"kind": "chart", "id": chart}}
        ]}),
    );
    let c = s.execute("macro.check", json!({"name": "Wide", "inputs": {}})).unwrap();
    let fp = &c["footprint"];
    assert_eq!(
        fp["sheets"],
        json!([{"sheet": "Notes", "cells": ["A1", "B2"], "changed": 2, "conflicts": ["A1"], "conflictCount": 1, "bounds": "A1:B2"}])
    );
    assert_eq!(fp["sheetsAdded"], json!(["Extra"]));
    assert_eq!((fp["objectsAdded"].as_u64(), fp["objectsRemoved"].as_u64()), (Some(1), Some(1)));
    assert_eq!(c["conflicts"], 2, "the deleted chart and the note");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2, "the dry run's new sheet went away");
    let err = s.execute("macro.run", json!({"name": "Wide"})).unwrap_err().to_string();
    assert!(err.contains("(A1 on Notes; 1 object)"), "{err}");

    // Running past the edge of the sheet fails before anything changes.
    put(
        &mut s,
        json!({"name": "Edge", "inputs": [{"name": "Input"}], "steps": [{"command": "cell.set", "params": {"cell": "Input.offset(-3,0)", "input": "x"}}]}),
    );
    let err = s.execute("macro.run", json!({"name": "Edge", "inputs": "B2"})).unwrap_err().to_string();
    assert!(err.contains("Step 1 (cell.set): 'Input.offset(-3,0)' runs past the top of the sheet."), "{err}");
    let c = s.execute("macro.check", json!({"name": "Edge", "inputs": "B2"})).unwrap();
    assert_eq!((c["ok"].as_bool(), c["footprint"]["error"]["edge"].as_bool()), (Some(false), Some(true)));
}

#[test]
fn repeat_if_and_stop() {
    let mut s = s();
    data(&mut s);
    put(
        &mut s,
        json!({"name": "Flag", "inputs": [{"name": "Input", "header": true}], "output": {"onConflict": "overwrite"}, "steps": [
            {"type": "repeat", "over": "Input.data", "body": [
                {"type": "if", "cell": "Row.cell(1,2)", "op": "gt", "value": 100,
                 "then": [{"command": "cell.set", "params": {"cell": "Row.right", "input": "big"}}],
                 "else": [{"command": "cell.set", "params": {"cell": "Row.right", "input": "small"}}]},
                {"type": "if", "cell": "Row.cell(1,1)", "op": "eq", "value": "bo", "then": [{"type": "stop", "message": "Found Bo."}]}
            ]},
            {"command": "cell.set", "params": {"cell": "A1", "input": "not reached"}}
        ]}),
    );
    let r = s.execute("macro.run", json!({"name": "Flag", "inputs": "B2:C5"})).unwrap();
    assert_eq!(r["stopped"], "Found Bo.");
    assert_eq!((v(&s, "D3").display(), v(&s, "D4").display()), ("big".into(), "small".into()));
    assert!(v(&s, "D5").is_empty() && v(&s, "A1").is_empty());
    assert_eq!(undo_labels(&s).last().unwrap(), "Run Macro: Flag", "a stop keeps the changes");
    // A Stop that rolls back is an error that changes nothing.
    put(
        &mut s,
        json!({"name": "Guard", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "x"}},
            {"type": "if", "cell": "C3", "op": "notEmpty", "then": [{"type": "stop", "message": "C3 must be empty.", "rollback": true}]}
        ]}),
    );
    let err = s.execute("macro.run", json!({"name": "Guard"})).unwrap_err().to_string();
    assert_eq!(err, "Macro 'Guard' stopped: C3 must be empty. Nothing was changed.");
    assert!(v(&s, "A1").is_empty());
    // Comparisons.
    for (op, value, want) in [
        ("eq", json!(120), true),
        ("eq", json!("120"), true),
        ("ne", json!(120), false),
        ("ge", json!(120), true),
        ("lt", json!(120.5), true),
        ("le", json!(100), false),
        ("contains", json!("12"), true),
        ("empty", json!(null), false),
    ] {
        put(
            &mut s,
            json!({"name": "Cmp", "output": {"onConflict": "overwrite"}, "steps": [
                {"type": "if", "cell": "C3", "op": op, "value": value, "then": [{"command": "cell.set", "params": {"cell": "A9", "input": "yes"}}], "else": [{"command": "cell.set", "params": {"cell": "A9", "input": "no"}}]}
            ]}),
        );
        s.execute("macro.run", json!({"name": "Cmp"})).unwrap();
        assert_eq!(v(&s, "A9").display(), if want { "yes" } else { "no" }, "{op} {value}");
    }
    // If needs one cell.
    put(&mut s, json!({"name": "Many", "steps": [{"type": "if", "cell": "A1:A2", "op": "empty"}]}));
    assert!(s.execute("macro.run", json!({"name": "Many"})).unwrap_err().to_string().contains("If looks at one cell"));
    // Disabled steps are skipped.
    put(
        &mut s,
        json!({"name": "Off", "output": {"onConflict": "overwrite"}, "steps": [{"command": "cell.set", "params": {"cell": "A8", "input": "x"}, "enabled": false}]}),
    );
    assert_eq!(s.execute("macro.run", json!({"name": "Off"})).unwrap()["ran"], 0);
}

#[test]
fn macros_are_bounded() {
    let mut s = s();
    put(&mut s, json!({"name": "Long", "steps": [{"type": "repeat", "over": "A1:A20", "max": 10, "body": [{"type": "stop"}]}]}));
    let err = s.execute("macro.run", json!({"name": "Long"})).unwrap_err().to_string();
    assert!(err.contains("Repeat over A1:A20 has 20 rows, more than its limit of 10."), "{err}");
    // 400 × 400 passes is over the step limit.
    put(
        &mut s,
        json!({"name": "Spin", "output": {"onConflict": "overwrite"}, "steps": [{"type": "repeat", "over": "A1:A400", "body": [
            {"type": "repeat", "over": "A1:A400", "body": [{"type": "if", "cell": "A1", "op": "empty"}]}
        ]}]}),
    );
    let err = s.execute("macro.run", json!({"name": "Spin"})).unwrap_err().to_string();
    assert!(err.contains("ran more than 100000 steps"), "{err}");
    let err = s.execute("macro.run", json!({"name": "Spin", "timeLimit": 0.001})).unwrap_err().to_string();
    assert!(err.contains("ran longer than"), "{err}");
    // The footprint's dry run keeps to the time limit too, and so does the check's.
    put(
        &mut s,
        json!({"name": "Spin", "output": {"onConflict": "fail"}, "steps": [{"type": "repeat", "over": "A1:A400", "body": [
            {"type": "repeat", "over": "A1:A400", "body": [{"type": "if", "cell": "A1", "op": "empty"}]}
        ]}]}),
    );
    let err = s.execute("macro.run", json!({"name": "Spin", "timeLimit": 0.001})).unwrap_err().to_string();
    assert!(err.contains("ran longer than"), "{err}");
    let c = s.execute("macro.check", json!({"name": "Spin", "timeLimit": 0.001})).unwrap();
    assert!(c["errors"][0].as_str().unwrap().contains("ran longer than"), "{c}");
}

#[test]
fn a_token_in_a_path_needs_the_permission_and_approval() {
    let mut s = s();
    let dir = std::env::temp_dir().join(format!("gc-macro-token-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
    let in_dir = format!("{}/{{{{Input}}}}", dir.to_string_lossy());
    for path in [json!({"$ref": "Input"}), json!(in_dir), json!(" ")] {
        let step = json!({"command": "file.exportCsv", "params": {"path": path}});
        put(&mut s, json!({"name": "Export", "inputs": [{"name": "Input"}], "steps": [step]}));
        // Without the permission nothing runs, not even the check's dry run.
        let err = s.execute("macro.run", json!({"name": "Export"})).unwrap_err().to_string();
        assert!(err.contains("Step 1 (file.exportCsv) writes a file. Add file.exportCsv to the macro's permissions"), "{path}: {err}");
        let c = s.execute("macro.check", json!({"name": "Export"})).unwrap();
        assert!(c["errors"][0].as_str().unwrap().contains("writes a file"), "{path}: {c}");
        assert!(c.get("footprint").is_none());
        // With it, the person approves first; the check doesn't run it before then.
        put(&mut s, json!({"name": "Export", "inputs": [{"name": "Input"}], "steps": [step], "permissions": ["file.exportCsv"]}));
        let r = s.execute("macro.run", json!({"name": "Export"}));
        let Err(EngineError::Approve(q)) = &r else { panic!("{path}: {r:?}") };
        assert!(q.contains("file.exportCsv, which writes a file"), "{q}");
        let c = s.execute("macro.check", json!({"name": "Export"})).unwrap();
        assert_eq!(c["needsApproval"], json!(["runs file.exportCsv, which writes a file"]), "{path}");
        assert!(c.get("footprint").is_none());
        assert_eq!(files(&dir), 0, "{path}");
        assert!(!std::path::Path::new("A1.csv").exists() && !std::path::Path::new(" .csv").exists());
    }
    // Once approved, the check's dry run stops before the step, and only the run writes the file.
    let step = json!({"command": "file.exportCsv", "params": {"path": in_dir}});
    put(&mut s, json!({"name": "Export", "inputs": [{"name": "Input"}], "steps": [step], "permissions": ["file.exportCsv"]}));
    s.execute("macro.run", json!({"name": "Export", "approve": true, "inputs": "B2"})).unwrap();
    assert_eq!(files(&dir), 1);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let c = s.execute("macro.check", json!({"name": "Export", "inputs": "B2"})).unwrap();
    assert_eq!(c["footprint"]["partial"]["step"], "1", "{c}");
    assert_eq!(files(&dir), 0, "the check writes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_debugger_runs_a_token_path_only_with_the_permission_and_approval() {
    let mut s = s();
    let dir = std::env::temp_dir().join(format!("gc-macro-debug-token-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
    let step = json!({"command": "file.exportCsv", "params": {"path": format!("{}/{{{{Input}}}}", dir.to_string_lossy())}});
    put(&mut s, json!({"name": "Export", "inputs": [{"name": "Input"}], "steps": [step]}));
    let err = s.execute("macro.debug", json!({"action": "start", "name": "Export"})).unwrap_err().to_string();
    assert!(err.contains("writes a file"), "{err}");
    put(&mut s, json!({"name": "Export", "inputs": [{"name": "Input"}], "steps": [step], "permissions": ["file.exportCsv"]}));
    assert!(matches!(s.execute("macro.debug", json!({"action": "start", "name": "Export"})), Err(EngineError::Approve(_))));
    assert_eq!(files(&dir), 0);
    // Approved, it pauses before the step and writes the file only when the step runs.
    s.execute("macro.debug", json!({"action": "start", "name": "Export", "approve": true, "inputs": "B2", "breakpoints": ["1"]})).unwrap();
    assert_eq!(files(&dir), 0, "paused before the step");
    s.execute("macro.debug", json!({"action": "step"})).unwrap();
    assert_eq!(files(&dir), 1, "the debugger runs the approved step");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_policy_keeps_macros_away_from_files_and_the_app() {
    let mut s = s();
    for (id, params) in [
        ("file.saveAs", json!({"path": "/tmp/x.xlsx"})),
        ("file.close", json!({})),
        ("app.language.set", json!({"language": "de"})),
        ("automate.runScript", json!({"name": "Script 1"})),
        ("edit.undo", json!({})),
        ("macro.run", json!({"name": "Self"})),
        ("view.newWindow", json!({})),
        ("view.hideWindow", json!({})),
    ] {
        put(
            &mut s,
            json!({"name": "Bad", "steps": [{"command": "cell.set", "params": {"cell": "A1", "input": "x"}}, {"command": id, "params": params}]}),
        );
        let err = s.execute("macro.run", json!({"name": "Bad"})).unwrap_err().to_string();
        assert!(err.contains(&format!("Step 2 ({id}) can't run in a macro")), "{err}");
        assert!(v(&s, "A1").is_empty(), "{id}: nothing ran");
    }
    // A disabled step isn't checked.
    put(&mut s, json!({"name": "Bad", "steps": [{"command": "file.close", "enabled": false}]}));
    s.execute("macro.run", json!({"name": "Bad"})).unwrap();

    // Reading a file needs the permission, then the person's approval (once per macro content).
    let dir = std::env::temp_dir().join(format!("gc-macro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("in.csv");
    std::fs::write(&csv, "a,b\n1,2\n").unwrap();
    let step = json!({"command": "data.fromTextCsv", "params": {"path": csv.to_string_lossy(), "sheet": "Imported"}});
    put(&mut s, json!({"name": "Import", "steps": [step]}));
    let err = s.execute("macro.run", json!({"name": "Import"})).unwrap_err().to_string();
    assert!(err.contains("Step 1 (data.fromTextCsv) reads a file. Add data.fromTextCsv to the macro's permissions"), "{err}");
    put(&mut s, json!({"name": "Import", "steps": [step], "permissions": ["data.fromTextCsv"]}));
    let r = s.execute("macro.run", json!({"name": "Import"}));
    let Err(EngineError::Approve(q)) = &r else { panic!("{r:?}") };
    assert_eq!(q, "Run macro 'Import'? It runs data.fromTextCsv, which reads a file.");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    let c = s.execute("macro.check", json!({"name": "Import"})).unwrap();
    assert_eq!(c["needsApproval"], json!(["runs data.fromTextCsv, which reads a file"]));
    assert!(c.get("footprint").is_none(), "nothing unapproved runs, even dry");
    s.execute("macro.run", json!({"name": "Import", "approve": true})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("macro.run", json!({"name": "Import"})).unwrap();
    // The dry run skips the file step and says so.
    let c = s.execute("macro.check", json!({"name": "Import"})).unwrap();
    assert_eq!(c["footprint"]["partial"]["step"], "1");
    // Changing the macro asks again.
    put(&mut s, json!({"name": "Import", "description": "changed", "steps": [step], "permissions": ["data.fromTextCsv"]}));
    assert!(matches!(s.execute("macro.run", json!({"name": "Import"})), Err(EngineError::Approve(_))));
    let _ = std::fs::remove_dir_all(&dir);

    // Steps that would open a dialog fail instead.
    put(&mut s, json!({"name": "Pic", "output": {"onConflict": "overwrite"}, "steps": [{"command": "insert.picture"}]}));
    let err = s.execute("macro.run", json!({"name": "Pic"})).unwrap_err().to_string();
    assert!(err.contains("would open the insertPicture dialog"), "{err}");
    assert!(s.ui_requests.is_empty());
}

#[test]
fn macros_from_a_file_ask_before_running() {
    let mut s = s();
    let mut wb = gridcraft_model::Workbook::new();
    wb.macros.push(crate::macros::parse_macro(&json!({"name": "Theirs", "output": {"onConflict": "overwrite"}, "steps": [{"command": "cell.set", "params": {"cell": "A1", "input": "ran"}}]})).unwrap());
    let bytes = serde_json::to_vec(&wb).unwrap();
    s.execute("file.open", json!({"name": "theirs.json", "base64": crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(s.doc().unwrap().trust, crate::Trust::FromFile);
    let r = s.execute("macro.run", json!({"name": "Theirs"}));
    let Err(EngineError::Approve(q)) = &r else { panic!("{r:?}") };
    assert_eq!(q, "Run macro 'Theirs'? It came with a workbook you opened.");
    assert!(v(&s, "A1").is_empty());
    s.execute("macro.run", json!({"name": "Theirs", "approve": true})).unwrap();
    assert_eq!(v(&s, "A1").display(), "ran");
    // Approved once per macro content: the second run doesn't ask.
    s.execute("macro.run", json!({"name": "Theirs"})).unwrap();
    // The approval holds the macro's whole content, not a hash two macros could share.
    let m = s.doc().unwrap().wb.macros[0].clone();
    let doc = s.doc().unwrap().uid;
    let key: (u64, String) = (doc, crate::macros::macro_json(&m).to_string());
    assert!(s.macro_approvals.contains(&key));
    // The JSON is canonical: the same macro with its keys in another order is the same macro.
    let a = crate::macros::parse_macro(&json!({"name": "M", "steps": [{"command": "cell.set", "params": {"cell": "A1", "input": "x"}}]})).unwrap();
    let b = crate::macros::parse_macro(&json!({"steps": [{"params": {"input": "x", "cell": "A1"}, "command": "cell.set"}], "name": "M"})).unwrap();
    assert_eq!(crate::macros::fingerprint(&a), crate::macros::fingerprint(&b));
}

#[test]
fn a_partial_footprint_doesnt_clear_later_writes() {
    let dir = std::env::temp_dir().join(format!("gc-macro-partial-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("out.csv").to_string_lossy().to_string();
    let export = json!({"command": "file.exportCsv", "params": {"path": csv}});
    let solver = json!({"command": "data.solver", "params": {"objective": "C1", "variables": "A1", "constraints": ["A1 <= 5"], "method": "simplex", "saveModel": false}});
    let late = json!({"command": "cell.set", "params": {"cell": "B1", "input": "over"}});
    // The dry run stops at an approved file step or at Solver; the step after it writes over B1.
    for first in [&export, &solver] {
        let mut s = super::s();
        set(&mut s, "A1", "1");
        set(&mut s, "C1", "=A1");
        set(&mut s, "B1", "keep");
        put(&mut s, json!({"name": "Late", "inputs": [{"name": "Input"}], "permissions": ["file.exportCsv"], "steps": [first, late]}));
        let start = s.doc().unwrap().wb.clone();
        let c = s.execute("macro.check", json!({"name": "Late", "inputs": "A1"})).unwrap();
        if c.get("needsApproval").is_none() {
            assert_eq!((c["conflicts"].as_u64(), &c["footprint"]["partial"]["step"]), (Some(0), &json!("1")), "{c}");
        }
        for rule in ["fail", "ask"] {
            let r = s.execute("macro.run", json!({"name": "Late", "inputs": "A1", "onConflict": rule, "approve": true}));
            let e = match (rule, r) {
                ("fail", Err(EngineError::Other(e))) | ("ask", Err(EngineError::Confirm(e))) => e,
                (_, r) => panic!("{first} {rule}: {r:?}"),
            };
            assert!(e.contains("Step 2 (cell.set): It would write over data (B1 on Sheet1)"), "{e}");
            assert_eq!(v(&s, "B1").display(), "keep", "{first} {rule}");
            assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start), "{first} {rule}: rolled back");
        }
        // The answer to the question overwrites.
        s.execute("macro.run", json!({"name": "Late", "inputs": "A1", "overwrite": true})).unwrap();
        assert_eq!(v(&s, "B1").display(), "over");
    }
    // A step that writes over what the macro itself wrote isn't a conflict.
    let mut s = super::s();
    let again = |input: &str| json!({"command": "cell.set", "params": {"cell": "E1", "input": input}});
    put(
        &mut s,
        json!({"name": "Twice", "permissions": ["file.exportCsv"], "output": {"onConflict": "fail"}, "steps": [again("a"), export, again("b")]}),
    );
    s.execute("macro.run", json!({"name": "Twice", "approve": true})).unwrap();
    assert_eq!(v(&s, "E1").display(), "b");
    // After a shift, the steps past the file step are checked too.
    let mut s = super::s();
    set(&mut s, "C1", "old");
    set(&mut s, "D1", "keep");
    let step = |cell: &str| json!({"command": "cell.set", "params": {"cell": cell, "input": "new"}});
    put(
        &mut s,
        json!({"name": "Shift", "permissions": ["file.exportCsv"], "output": {"onConflict": "shiftDown"}, "steps": [step("C1"), export, step("D1")]}),
    );
    let start = s.doc().unwrap().wb.clone();
    let err = s.execute("macro.run", json!({"name": "Shift", "approve": true})).unwrap_err().to_string();
    assert!(err.contains("Step 3 (cell.set): It would write over data (D1 on Sheet1)"), "{err}");
    assert_eq!((v(&s, "C1").display(), v(&s, "D1").display()), ("old".into(), "keep".into()));
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start), "the shift is rolled back too");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- debugger

fn debug(s: &mut Session, p: Json) -> Json {
    s.execute("macro.debug", p).unwrap()
}

/// Four steps: two plain ones, a Repeat over the data rows (with a breakpoint on its body), and
/// a final one.
fn stepper(s: &mut Session) {
    data(s);
    put(
        s,
        json!({"name": "Walk", "inputs": [{"name": "Input", "header": true}], "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "E1", "input": "start"}},
            {"command": "cell.set", "params": {"cell": "Input.right.cell(1,1)", "input": "flag"}},
            {"type": "repeat", "over": "Input.data", "body": [
                {"command": "cell.set", "params": {"cell": "Row.right", "input": "seen"}, "breakpoint": true}
            ]},
            {"command": "cell.set", "params": {"cell": "E9", "input": "end"}}
        ]}),
    );
}

#[test]
fn the_debugger_steps_continues_and_runs_to_a_step_as_one_undo_step() {
    let mut s = s();
    stepper(&mut s);
    let undo = undo_labels(&s);
    let st = debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5", "action": "start"}));
    assert_eq!(st["state"], "paused");
    assert_eq!(
        st["current"],
        json!({"step": "1", "kind": "command", "command": "cell.set", "params": {"cell": "E1", "input": "start"}, "breakpoint": false})
    );
    assert_eq!(st["executed"], 0);

    let st = debug(&mut s, json!({"action": "step"}));
    assert_eq!(st["current"]["step"], "2");
    assert_eq!(st["current"]["params"]["cell"], "D2", "tokens resolve for the next step");
    assert_eq!(st["lastParams"], json!({"cell": "E1", "input": "start"}));
    assert_eq!(st["changed"], json!(["Sheet1!E1"]));
    assert_eq!(v(&s, "E1").display(), "start");

    // Other edits wait; selecting cells and queries don't.
    let r = s.execute("cell.set", json!({"cell": "A1", "input": "x"}));
    assert!(matches!(&r, Err(EngineError::Disabled(_, why)) if why == "Finish or stop the macro first"), "{r:?}");
    assert!(s.execute("macro.run", json!({"name": "Walk"})).is_err());
    assert!(s.execute("edit.undo", json!({})).is_err());
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    assert!(s.execute("macro.debug", json!({"name": "Walk", "action": "start"})).unwrap_err().to_string().contains("is being debugged"));

    // Continue stops at the breakpoint inside the Repeat, once per row.
    let st = debug(&mut s, json!({"action": "continue"}));
    assert_eq!((st["current"]["step"].as_str(), st["atBreakpoint"].as_bool()), (Some("3.1"), Some(true)));
    assert_eq!(st["current"]["iteration"], json!({"at": 1, "of": 3}));
    assert_eq!(st["current"]["params"]["cell"], "D3");
    let st = debug(&mut s, json!({"action": "continue"}));
    assert_eq!(st["current"]["iteration"], json!({"at": 2, "of": 3}));
    // Run To stops at a breakpoint on the way, then at its step.
    let st = debug(&mut s, json!({"action": "runTo", "step": "4", "breakpoints": []}));
    assert_eq!((st["current"]["step"].as_str(), st["atBreakpoint"].as_bool()), (Some("3.1"), Some(true)));
    assert_eq!(st["current"]["iteration"], json!({"at": 3, "of": 3}));
    let st = debug(&mut s, json!({"action": "runTo", "step": "4"}));
    assert_eq!((st["current"]["step"].as_str(), st["atBreakpoint"].as_bool()), (Some("4"), Some(false)));
    assert_eq!((v(&s, "D4").display(), v(&s, "D5").display()), ("seen".into(), "seen".into()));
    // Breakpoints from the call: none left before the end.
    let st = debug(&mut s, json!({"action": "continue"}));
    assert_eq!(st["state"], "finished");
    assert_eq!(st["executed"], 7);
    assert!(st["changed"].as_array().unwrap().contains(&json!("Sheet1!E9")));
    assert_eq!(v(&s, "E9").display(), "end");
    let after = undo_labels(&s);
    assert_eq!(after.len(), undo.len() + 1, "{after:?}");
    assert_eq!(after.last().unwrap(), "Run Macro: Walk");
    // Edits work again; one undo takes the whole run back.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "E1").is_empty() && v(&s, "D3").is_empty() && v(&s, "E9").is_empty());
    set(&mut s, "A1", "free");
}

#[test]
fn a_breakpoint_list_can_replace_the_steps_own() {
    let mut s = s();
    stepper(&mut s);
    // Only step 4 stops the run: the flag on 3.1 is ignored.
    let st = debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5", "breakpoints": ["4"], "breakpointsOnly": true}));
    assert_eq!(st["current"]["step"], "1");
    let st = debug(&mut s, json!({"action": "continue"}));
    assert_eq!((st["current"]["step"].as_str(), st["atBreakpoint"].as_bool()), (Some("4"), Some(true)));
    debug(&mut s, json!({"action": "stop"}));
    // Clearing the list mid-run (an editor removing its last breakpoint) runs to the end.
    debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5", "breakpoints": ["3.1"], "breakpointsOnly": true}));
    let st = debug(&mut s, json!({"action": "continue"}));
    assert_eq!(st["current"]["step"], "3.1");
    let st = debug(&mut s, json!({"action": "continue", "breakpoints": [], "breakpointsOnly": true}));
    assert_eq!(st["state"], "finished");
}

#[test]
fn stopping_the_debugger_keeps_or_rolls_back() {
    for keep in [false, true] {
        let mut s = s();
        stepper(&mut s);
        let (start, undo) = (s.doc().unwrap().wb.clone(), undo_labels(&s));
        debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5", "breakpoints": ["2"]}));
        let st = debug(&mut s, json!({"action": "continue"}));
        assert_eq!((st["current"]["step"].as_str(), st["atBreakpoint"].as_bool()), (Some("2"), Some(true)));
        let st = debug(&mut s, json!({"action": "stop", "keep": keep}));
        assert_eq!((st["state"].as_str(), st["kept"].as_bool()), (Some("stopped"), Some(keep)));
        if keep {
            assert_eq!(v(&s, "E1").display(), "start");
            assert_eq!(undo_labels(&s).len(), undo.len() + 1);
            assert_eq!(st["changed"], json!(["Sheet1!E1"]));
        } else {
            assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start));
            assert_eq!(undo_labels(&s), undo);
        }
        set(&mut s, "A1", "free");
        assert!(s.execute("macro.debug", json!({"action": "step"})).unwrap_err().to_string().contains("No macro is being debugged"));
    }
}

#[test]
fn a_debugged_run_after_a_partial_footprint_stops_before_writing_over_data() {
    let mut s = s();
    set(&mut s, "A1", "1");
    set(&mut s, "C1", "=A1");
    set(&mut s, "B1", "keep");
    let solver = json!({"command": "data.solver", "params": {"objective": "C1", "variables": "A1", "constraints": ["A1 <= 5"], "method": "simplex", "saveModel": false}});
    let late = json!({"command": "cell.set", "params": {"cell": "B1", "input": "over"}});
    put(&mut s, json!({"name": "Late", "inputs": [{"name": "Input"}], "output": {"onConflict": "fail"}, "steps": [solver, late]}));
    let start = s.doc().unwrap().wb.clone();
    debug(&mut s, json!({"name": "Late", "action": "start", "inputs": "A1"}));
    debug(&mut s, json!({"action": "step"}));
    let st = debug(&mut s, json!({"action": "step"}));
    assert_eq!(st["state"], "failed", "{st}");
    assert!(st["message"].as_str().unwrap().contains("It would write over data (B1 on Sheet1)"), "{st}");
    assert_eq!(v(&s, "B1").display(), "keep");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start));
}

#[test]
fn a_failing_step_ends_the_debugged_run_and_rolls_it_back() {
    let mut s = s();
    data(&mut s);
    put(
        &mut s,
        json!({"name": "Breaks", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "one"}},
            {"command": "cell.set", "params": {"cell": "Nowhere!A2", "input": "two"}},
            {"type": "stop", "message": "never"}
        ]}),
    );
    let start = s.doc().unwrap().wb.clone();
    debug(&mut s, json!({"name": "Breaks"}));
    assert!(s.execute("macro.debug", json!({"action": "runTo", "step": "9"})).unwrap_err().to_string().contains("has no step 9"));
    assert!(s.execute("macro.debug", json!({"action": "leap"})).is_err());
    debug(&mut s, json!({"action": "step"}));
    let st = debug(&mut s, json!({"action": "step"}));
    assert_eq!(st["state"], "failed");
    assert_eq!(st["error"]["step"], "2");
    assert!(st["message"].as_str().unwrap().starts_with("Step 2 (cell.set):"), "{st}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &start));
    set(&mut s, "B1", "free again");
    // A run that ends at once (a Stop first) finishes on start.
    put(&mut s, json!({"name": "Quick", "steps": [{"type": "stop", "message": "bye"}]}));
    let st = debug(&mut s, json!({"name": "Quick"}));
    assert_eq!(st["state"], "paused");
    let st = debug(&mut s, json!({"action": "step"}));
    assert_eq!((st["state"].as_str(), st["stopped"].as_str()), (Some("stopped"), Some("bye")));
    put(&mut s, json!({"name": "Empty", "steps": []}));
    assert_eq!(debug(&mut s, json!({"name": "Empty"}))["state"], "finished");
}

// ---------------------------------------------------------------- commands and the recorder

#[test]
fn macros_are_saved_listed_renamed_and_deleted() {
    let mut s = s();
    s.execute("macro.save", json!({"macro": totals()})).unwrap();
    // The fields can also come at the top level.
    let r = s
        .execute("macro.save", json!({"name": "Second", "description": "two", "shortcut": "Ctrl+Shift+Q", "steps": [{"command": "home.bold"}]}))
        .unwrap();
    assert_eq!(r, json!({"name": "Second", "steps": 1}));
    let list = s.execute("macro.list", json!({})).unwrap();
    assert_eq!(list[0], json!({"name": "Totals", "description": "", "steps": 3, "inputs": ["Input"], "onConflict": "ask", "permissions": []}));
    assert_eq!(list[1]["shortcut"], "Ctrl+Shift+Q");
    let got = s.execute("macro.get", json!({"name": "totals"})).unwrap();
    assert_eq!(crate::macros::parse_macro(&got).unwrap(), crate::macros::parse_macro(&totals()).unwrap());
    assert!(s.execute("macro.save", json!({"macro": totals(), "replace": false})).unwrap_err().to_string().contains("already exists"));
    assert!(s.execute("macro.save", json!({"macro": {"name": "Bad", "steps": [{"type": "warp"}]}})).is_err());
    assert!(s.execute("macro.rename", json!({"name": "Second", "to": "TOTALS"})).unwrap_err().to_string().contains("already exists"));
    s.execute("macro.rename", json!({"name": "Second", "to": "Third"})).unwrap();
    s.execute("macro.rename", json!({"name": "third", "to": "THIRD"})).unwrap();
    s.execute("macro.delete", json!({"name": "Totals"})).unwrap();
    let names: Vec<String> =
        s.execute("macro.list", json!({})).unwrap().as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["THIRD"]);
    assert!(s.execute("macro.delete", json!({"name": "Totals"})).unwrap_err().to_string().contains("no macro called"));
    // Saving, renaming and deleting are undoable.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("macro.list", json!({})).unwrap().as_array().unwrap().len(), 2);
    // Macros are kept apart from Automate scripts: View › Macros still lists the scripts.
    assert_eq!(s.execute("view.macros", json!({})).unwrap(), json!([]));
    assert!(s.execute("macro.rename", json!({"name": "THIRD", "to": "a\nb"})).is_err());
}

#[test]
fn macros_and_automate_scripts_live_side_by_side() {
    let mut s = s();
    let steps = json!([{"command": "cell.set", "params": {"cell": "B1", "input": "from script"}}]);
    s.execute("automate.newScript", json!({"name": "Inner", "commands": steps})).unwrap();
    s.execute("macro.save", json!({"name": "Inner", "steps": [{"command": "cell.set", "params": {"cell": "C1", "input": "from macro"}}], "output": {"onConflict": "overwrite"}})).unwrap();
    // Recording a script run keeps the script's steps, so the macro stands on its own.
    s.execute("macro.record", json!({"relative": false})).unwrap();
    set(&mut s, "A1", "typed");
    s.execute("automate.runScript", json!({"name": "Inner"})).unwrap();
    s.execute("automate.recordActions", json!({})).unwrap();
    s.execute("automate.recordActions", json!({"stop": true, "name": "Empty"})).unwrap();
    let r = s.execute("macro.record", json!({"stop": true, "name": "Recorded"})).unwrap();
    assert_eq!(r["skipped"], json!(["automate.recordActions", "automate.recordActions"]));
    let m = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": "Recorded"})).unwrap()).unwrap();
    let ids: Vec<String> = crate::macros::command_steps(&m.steps).iter().map(|(_, id, p)| format!("{id} {p}")).collect();
    assert_eq!(ids, ["cell.set {\"cell\":\"A1\",\"input\":\"typed\"}", "cell.set {\"cell\":\"B1\",\"input\":\"from script\"}"]);
    // Both lists keep their own entries, under the same name if need be.
    let scripts = s.execute("automate.allScripts", json!({})).unwrap();
    assert_eq!(scripts, json!([{"name": "Empty", "steps": 0, "trusted": true}, {"name": "Inner", "steps": 1, "trusted": true}]));
    let macros: Vec<Json> = s.execute("macro.list", json!({})).unwrap().as_array().unwrap().iter().map(|m| m["name"].clone()).collect();
    assert_eq!(macros, [json!("Inner"), json!("Recorded")]);
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("automate.runScript", json!({"name": "Inner"})).unwrap();
    s.execute("macro.run", json!({"name": "Inner"})).unwrap();
    assert_eq!((v(&s, "B1").display(), v(&s, "C1").display()), ("from script".into(), "from macro".into()));
    // Both survive an xlsx round trip unchanged.
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "both.xlsx", "base64": b64})).unwrap();
    // Opened from a file, the scripts are not trusted yet (#2178); their names and steps are the same.
    let names_and_steps = |v: &Json| v.as_array().unwrap().iter().map(|s| (s["name"].clone(), s["steps"].clone())).collect::<Vec<_>>();
    assert_eq!(names_and_steps(&s.execute("automate.allScripts", json!({})).unwrap()), names_and_steps(&scripts));
    assert_eq!(s.execute("macro.list", json!({})).unwrap().as_array().unwrap().len(), 2);
}

#[test]
fn a_relative_recording_plays_back_on_another_block() {
    let mut s = s();
    data(&mut s);
    put(
        &mut s,
        json!({"name": "Plain", "steps": [{"command": "cell.set", "params": {"cell": "A20", "input": "plain"}}], "output": {"onConflict": "overwrite"}}),
    );
    put(&mut s, json!({"name": "Shaped", "inputs": [{"name": "Input"}], "steps": []}));
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    let r = s.execute("macro.record", json!({})).unwrap();
    assert_eq!(r, json!({"recording": true, "relative": true, "input": "B2:C5"}));
    // Clicking a cell and typing, a formula over the amounts, bold header, italic on a selection.
    s.execute("selection.set", json!({"cell": "E2"})).unwrap();
    set(&mut s, "E2", "Total");
    set(&mut s, "E3", "=SUM(C3:C5)");
    s.execute("home.bold", json!({"range": "B2:C2"})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    s.execute("selection.set", json!({"cell": "C4"})).unwrap();
    s.execute("home.italic", json!({})).unwrap();
    set(&mut s, "C6", "end");
    // Not kept: commands macros don't run, and a macro with inputs; a plain macro's steps are
    // inlined.
    s.execute("window.activate", json!({"index": 0})).unwrap();
    s.execute("macro.run", json!({"name": "Plain"})).unwrap();
    s.execute("macro.run", json!({"name": "Shaped", "inputs": "A1"})).unwrap();
    // The journal can be trimmed meanwhile.
    s.journal.clear();
    let r = s.execute("macro.record", json!({"stop": true, "name": "Sum it", "description": "made by recording"})).unwrap();
    assert!(!s.is_recording());
    assert_eq!(r["saved"], "Sum it");
    assert_eq!(r["skipped"], json!(["window.activate", "macro.run"]));
    assert_eq!(r["inputs"], json!(["Input"]));
    let m = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": "Sum it"})).unwrap()).unwrap();
    assert_eq!(m.description, "made by recording");
    let input = &m.inputs[0];
    assert!(input.header);
    assert_eq!((input.cols, input.col_types.clone()), ([2, 2], vec![gridcraft_model::ColType::Text, gridcraft_model::ColType::Number]));
    let steps: Vec<(String, Json)> = m
        .steps
        .iter()
        .map(|st| match &st.kind {
            gridcraft_model::StepKind::Command { id, params } => (id.clone(), params.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        steps,
        vec![
            ("selection.set".to_string(), json!({"cell": "Input.cell(1,4)"})),
            ("cell.set".into(), json!({"cell": "Input.cell(1,4)", "input": "Total"})),
            ("cell.set".into(), json!({"cell": "Input.cell(2,4)", "input": "=SUM({{Input.rows(2..).col(2)}})"})),
            ("home.bold".into(), json!({"range": "Input.row(1)", "on": true})),
            ("selection.set".into(), json!({"cell": "Input.cell(3,2)"})),
            ("home.italic".into(), json!({"on": true})),
            ("cell.set".into(), json!({"cell": "Input.col(2).below", "input": "end"})),
            ("cell.set".into(), json!({"cell": "A20", "input": "plain"})),
        ]
    );
    // Play it back on a longer block somewhere else.
    for (cell, input) in [
        ("G10", "Who"),
        ("H10", "Paid"),
        ("G11", "A"),
        ("H11", "1"),
        ("G12", "B"),
        ("H12", "2"),
        ("G13", "C"),
        ("H13", "3"),
        ("G14", "D"),
        ("H14", "4"),
    ] {
        set(&mut s, cell, input);
    }
    s.execute("macro.run", json!({"name": "Sum it", "inputs": "G10:H14", "onConflict": "overwrite"})).unwrap();
    assert_eq!((v(&s, "J10").display(), v(&s, "J11").display()), ("Total".into(), "10".into()));
    assert!(bold(&s, "G10") && bold(&s, "H10"));
    assert_eq!(v(&s, "H15").display(), "end");
    // Toggles were recorded with the state they set: a second run keeps the header bold.
    s.execute("macro.run", json!({"name": "Sum it", "inputs": "G10:H14", "onConflict": "overwrite"})).unwrap();
    assert!(bold(&s, "G10") && bold(&s, "H10"));
    let d = s.doc().unwrap();
    assert!(d.wb.styles.get(d.wb.active().unwrap().style_id(gridcraft_core::CellRef::parse("H12").unwrap())).font.italic);
    // The input is checked like any other: text in the amounts is refused.
    set(&mut s, "H12", "two");
    let err = s.execute("macro.run", json!({"name": "Sum it", "inputs": "G10:H14"})).unwrap_err().to_string();
    assert!(err.contains("Input column 2 (Paid) must hold numbers; H12 holds text."), "{err}");
}

#[test]
fn an_absolute_recording_keeps_its_references() {
    let mut s = s();
    s.execute("selection.set", json!({"range": "B2:C3"})).unwrap();
    s.execute("macro.record", json!({"relative": false})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    set(&mut s, "B2", "x");
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    set(&mut s, "C3", "y");
    let r = s.execute("macro.record", json!({})).unwrap();
    assert_eq!(r["saved"], "Macro1");
    let m = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": "Macro1"})).unwrap()).unwrap();
    assert!(m.inputs.is_empty());
    let ids: Vec<String> = crate::macros::command_steps(&m.steps).iter().map(|(_, id, p)| format!("{id} {p}")).collect();
    assert_eq!(
        ids,
        [
            "home.insertSheet {\"name\":\"Two\"}",
            "sheet.activate {\"sheet\":\"Two\"}",
            "selection.set {\"active\":\"A1\",\"range\":\"A1\"}",
            "cell.set {\"cell\":\"B2\",\"input\":\"x\"}",
            "sheet.activate {\"sheet\":\"Sheet1\"}",
            "selection.set {\"active\":\"A1\",\"range\":\"A1\"}",
            "cell.set {\"cell\":\"C3\",\"input\":\"y\"}",
        ]
    );
    // Stopping when nothing records says so.
    assert_eq!(s.execute("macro.record", json!({"stop": true})).unwrap(), json!({"recording": false}));
}

#[test]
fn only_the_workbooks_a_debugged_run_touched_wait() {
    let mut s = s();
    stepper(&mut s);
    debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5"}));
    debug(&mut s, json!({"action": "step"}));
    // Another workbook stays editable, macros included.
    s.new_workbook();
    set(&mut s, "A1", "elsewhere");
    put(
        &mut s,
        json!({"name": "Here", "output": {"onConflict": "overwrite"}, "steps": [{"command": "cell.set", "params": {"cell": "A2", "input": "ran"}}]}),
    );
    s.execute("macro.run", json!({"name": "Here"})).unwrap();
    assert_eq!(v(&s, "A2").display(), "ran");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "A2").is_empty());
    // The debugged one waits.
    s.set_active(0);
    let r = s.execute("cell.set", json!({"cell": "A1", "input": "x"}));
    assert!(matches!(&r, Err(EngineError::Disabled(_, why)) if why == "Finish or stop the macro first"), "{r:?}");
    // The run carries on in its own workbook whichever one is in front.
    s.set_active(1);
    let st = debug(&mut s, json!({"action": "continue", "breakpoints": []}));
    assert_eq!(st["state"], "paused");
    assert_eq!(s.active_index(), 0);
    debug(&mut s, json!({"action": "stop"}));
    set(&mut s, "A1", "free");
}

/// The commands of saved macro `name`, as `id params`.
fn recorded(s: &mut Session, name: &str) -> Vec<String> {
    let m = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": name})).unwrap()).unwrap();
    crate::macros::command_steps(&m.steps).iter().map(|(_, id, p)| format!("{id} {p}")).collect()
}

#[test]
fn a_recording_is_kept_when_saving_it_fails() {
    let mut s = s();
    s.execute("macro.record", json!({"relative": false})).unwrap();
    set(&mut s, "A1", "typed");
    // A name the macro check refuses: still recording, and a good name saves what was recorded.
    assert!(s.execute("macro.record", json!({"stop": true, "name": "Bad\u{1}name"})).is_err());
    assert!(s.is_recording());
    set(&mut s, "A2", "more");
    let r = s.execute("macro.record", json!({"stop": true, "name": "Good"})).unwrap();
    assert_eq!(r["saved"], "Good");
    assert!(!s.is_recording());
    assert_eq!(recorded(&mut s, "Good"), ["cell.set {\"cell\":\"A1\",\"input\":\"typed\"}", "cell.set {\"cell\":\"A2\",\"input\":\"more\"}"]);

    // A workbook at its macro limit: freeing a slot and stopping again saves it.
    s.execute("macro.record", json!({"relative": false})).unwrap();
    set(&mut s, "B1", "full");
    let d = s.doc_mut().unwrap();
    let mut wb = (*d.wb).clone();
    let filler = wb.macros[0].clone();
    while wb.macros.len() < gridcraft_model::MAX_MACROS {
        let mut m = filler.clone();
        m.name = format!("Filler{}", wb.macros.len());
        wb.macros.push(m);
    }
    d.wb = Arc::new(wb);
    let err = s.execute("macro.record", json!({"stop": true, "name": "Late"})).unwrap_err().to_string();
    assert!(err.contains("at most"), "{err}");
    assert!(s.is_recording());
    let d = s.doc_mut().unwrap();
    let mut wb = (*d.wb).clone();
    wb.macros.pop();
    d.wb = Arc::new(wb);
    s.execute("macro.record", json!({"stop": true, "name": "Late"})).unwrap();
    assert_eq!(recorded(&mut s, "Late"), ["cell.set {\"cell\":\"B1\",\"input\":\"full\"}"]);
}

#[test]
fn a_relative_recording_keeps_references_on_another_sheet() {
    let mut s = s();
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    data(&mut s);
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    s.execute("home.bold", json!({"range": "B2:C2"})).unwrap();
    // On the other sheet, C4 and C3:C5 are that sheet's cells, not the input's.
    s.execute("sheet.activate", json!({"sheet": "Two"})).unwrap();
    set(&mut s, "C4", "x");
    set(&mut s, "D2", "=SUM(C3:C5)");
    s.execute("macro.record", json!({"stop": true, "name": "Both"})).unwrap();
    let steps = recorded(&mut s, "Both");
    assert!(steps.contains(&"cell.set {\"cell\":\"C4\",\"input\":\"x\"}".to_string()), "{steps:?}");
    assert!(steps.contains(&"cell.set {\"cell\":\"D2\",\"input\":\"=SUM(C3:C5)\"}".to_string()), "{steps:?}");
    // Played back on another block of Sheet1, it writes Two's cells.
    set(&mut s, "C4", "");
    set(&mut s, "D2", "");
    set(&mut s, "C3", "5");
    set(&mut s, "C5", "7");
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    for (cell, input) in [("G10", "Who"), ("H10", "Paid"), ("G11", "A"), ("H11", "100"), ("G12", "B"), ("H12", "200"), ("G13", "C"), ("H13", "300")] {
        set(&mut s, cell, input);
    }
    s.execute("macro.run", json!({"name": "Both", "inputs": "G10:H13", "onConflict": "overwrite"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert!(bold(&s, "G10") && bold(&s, "H10"));
    // Nothing was written into or beside the input.
    assert_eq!((v(&s, "H12").display(), v(&s, "I10").display()), ("200".into(), String::new()));
    s.execute("sheet.activate", json!({"sheet": "Two"})).unwrap();
    assert_eq!((v(&s, "C4").display(), v(&s, "D2").display()), ("x".into(), "12".into()));
}

#[test]
fn a_step_that_names_another_sheet_keeps_its_references() {
    // `sheet` names where the step runs whatever sheet is active, so its C4 is that sheet's C4.
    let mut s = s();
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    data(&mut s);
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    s.execute("cell.set", json!({"sheet": "Two", "cell": "C4", "input": "x"})).unwrap();
    s.execute("cell.set", json!({"sheet": "Sheet1", "cell": "C4", "input": "90"})).unwrap();
    s.execute("macro.record", json!({"stop": true, "name": "Named"})).unwrap();
    let steps = recorded(&mut s, "Named");
    assert!(steps.contains(&"cell.set {\"cell\":\"C4\",\"input\":\"x\",\"sheet\":\"Two\"}".to_string()), "{steps:?}");
    assert!(steps.contains(&"cell.set {\"cell\":\"Input.cell(3,2)\",\"input\":\"90\",\"sheet\":\"Sheet1\"}".to_string()), "{steps:?}");
}

#[test]
fn inlining_recorded_runs_stops_at_the_step_limit() {
    // A recorded run of a long macro adds all of its steps; the conversion gathers at most the
    // macro's step limit and counts the rest, instead of expanding every run first.
    let mut s = s();
    let steps: Vec<Json> = (0..6_000).map(|_| json!({"command": "cell.set", "params": {"cell": "A1", "input": "x"}})).collect();
    put(&mut s, json!({"name": "Big", "output": {"onConflict": "overwrite"}, "steps": steps}));
    s.execute("macro.record", json!({})).unwrap();
    s.execute("macro.run", json!({"name": "Big"})).unwrap();
    s.execute("macro.run", json!({"name": "Big"})).unwrap();
    let r = s.execute("macro.record", json!({"stop": true, "name": "Twice"})).unwrap();
    assert_eq!((r["steps"].as_u64(), r["dropped"].as_u64()), (Some(10_000), Some(2_000)), "{r}");
}

#[test]
fn a_recording_can_be_cancelled() {
    let mut s = s();
    data(&mut s);
    let before = s.execute("macro.list", json!({})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    set(&mut s, "E2", "x");
    let r = s.execute("macro.record", json!({"cancel": true})).unwrap();
    assert_eq!(r, json!({"recording": false, "cancelled": true}));
    assert!(!s.is_recording() && s.macro_recording.is_none());
    assert_eq!(s.execute("macro.list", json!({})).unwrap(), before, "nothing was saved");
    assert_eq!(s.execute("macro.record", json!({"cancel": true})).unwrap()["cancelled"], false);
    // Recording works again afterwards.
    s.execute("macro.record", json!({})).unwrap();
    assert!(s.is_recording());
    s.execute("macro.record", json!({"cancel": true})).unwrap();
}

#[test]
fn formulas_are_tokenized_by_reading_their_references() {
    // An example as large as a sheet has a million rows: each reference in the formula is read
    // and matched on its own, instead of a list of every column and row of the example.
    let mut s = s();
    s.execute("selection.set", json!({"range": "A1:Z1000000"})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "AB1", "input": "=SUM(A1:A1000000)+SUM(A1:A3)+B2"})).unwrap();
    s.execute("cell.set", json!({"cell": "AB2", "input": "=SUM(A1:Z1000000)&\"A1:A1000000\""})).unwrap();
    s.execute("macro.record", json!({"stop": true, "name": "Wide"})).unwrap();
    let steps = recorded(&mut s, "Wide");
    // A single cell inside a formula stays as it is: only the input, its columns and its rows
    // become tokens there.
    assert!(steps.iter().any(|st| st.contains("=SUM({{Input.col(1)}})+SUM(A1:A3)+B2")), "{steps:?}");
    assert!(steps.iter().any(|st| st.contains("=SUM({{Input}})&\\\"A1:A1000000\\\"")), "{steps:?}");
}

#[test]
fn a_selection_of_several_areas_keeps_its_active_cell_as_it_is() {
    let mut s = s();
    data(&mut s);
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    // Several areas picked with the mouse: no command, so the recording adds a selection.set
    // with the active cell before the next step.
    let (a, b) = (gridcraft_core::RangeRef::parse("B2:C5").unwrap(), gridcraft_core::RangeRef::parse("E2:E5").unwrap());
    s.doc_mut().unwrap().selection = crate::Selection { active: b.start, anchor: b.start, ranges: vec![a, b] };
    s.execute("home.bold", json!({})).unwrap();
    s.execute("macro.record", json!({"stop": true, "name": "Areas"})).unwrap();
    let steps = recorded(&mut s, "Areas");
    assert!(steps.contains(&"selection.set {\"active\":\"E2\",\"range\":\"B2:C5,E2:E5\"}".to_string()), "{steps:?}");
    assert!(steps.iter().all(|st| !st.contains("\"active\":\"Input")), "{steps:?}");
}

#[test]
fn steps_recorded_in_another_workbook_are_counted() {
    let mut s = s();
    data(&mut s);
    let home = s.active_index();
    s.execute("macro.record", json!({})).unwrap();
    s.new_workbook();
    set(&mut s, "A1", "elsewhere");
    set(&mut s, "A2", "too");
    s.set_active(home);
    set(&mut s, "E2", "here");
    let r = s.execute("macro.record", json!({"stop": true, "name": "Home"})).unwrap();
    assert_eq!((r["steps"].as_u64(), r["dropped"].as_u64()), (Some(1), Some(2)), "{r}");
}

#[test]
fn a_recorded_macro_run_is_inlined_only_when_its_steps_stand_alone() {
    let mut s = s();
    put(
        &mut s,
        json!({"name": "Branchy", "output": {"onConflict": "overwrite"}, "steps": [
            {"type": "if", "cell": "A1", "op": "empty",
             "then": [{"command": "cell.set", "params": {"cell": "B1", "input": "empty"}}],
             "else": [{"command": "cell.set", "params": {"cell": "B1", "input": "full"}}]}
        ]}),
    );
    put(
        &mut s,
        json!({"name": "Placed", "output": {"anchor": "D1", "onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "Output", "input": "out"}}
        ]}),
    );
    put(
        &mut s,
        json!({"name": "Plain", "output": {"onConflict": "overwrite"}, "steps": [
            {"command": "cell.set", "params": {"cell": "C1", "input": "plain"}},
            {"command": "cell.set", "params": {"cell": "C2", "input": "off"}, "enabled": false}
        ]}),
    );
    s.execute("macro.record", json!({"relative": false})).unwrap();
    for name in ["Branchy", "Placed", "Plain"] {
        s.execute("macro.run", json!({"name": name})).unwrap();
    }
    let r = s.execute("macro.record", json!({"stop": true, "name": "Rec"})).unwrap();
    assert_eq!(r["skipped"], json!(["macro.run", "macro.run"]));
    assert_eq!(recorded(&mut s, "Rec"), ["cell.set {\"cell\":\"C1\",\"input\":\"plain\"}"]);
}

#[test]
fn a_recorded_script_run_leaves_out_the_steps_a_macro_may_not_run() {
    let mut s = s();
    let steps = json!([
        {"command": "cell.set", "params": {"cell": "B1", "input": "kept"}},
        {"command": "window.activate", "params": {"index": 0}},
        {"command": "macro.list", "params": {}}
    ]);
    s.execute("automate.newScript", json!({"name": "Mixed", "commands": steps})).unwrap();
    s.execute("macro.record", json!({"relative": false})).unwrap();
    s.execute("automate.runScript", json!({"name": "Mixed"})).unwrap();
    let r = s.execute("macro.record", json!({"stop": true, "name": "Rec"})).unwrap();
    assert_eq!(r["skipped"], json!(["window.activate", "macro.list"]));
    assert_eq!(recorded(&mut s, "Rec"), ["cell.set {\"cell\":\"B1\",\"input\":\"kept\"}"]);
    s.execute("macro.run", json!({"name": "Rec", "onConflict": "overwrite"})).unwrap();
}

#[test]
fn a_recording_is_saved_in_the_workbook_where_it_started() {
    let mut s = s();
    data(&mut s);
    let first = s.doc().unwrap().uid;
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("macro.record", json!({})).unwrap();
    set(&mut s, "E2", "Total");
    s.new_workbook();
    let r = s.execute("macro.record", json!({"stop": true, "name": "Here"})).unwrap();
    assert_eq!(r["saved"], "Here");
    // The active workbook stays active and gets nothing.
    assert_ne!(s.doc().unwrap().uid, first);
    assert!(s.doc().unwrap().wb.macros.is_empty());
    s.set_active(0);
    let m = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": "Here"})).unwrap()).unwrap();
    // The input's shape comes from the workbook where recording started.
    assert!(m.inputs[0].header);
    assert_eq!(recorded(&mut s, "Here"), ["cell.set {\"cell\":\"Input.cell(1,4)\",\"input\":\"Total\"}"]);

    // When that workbook was closed, stopping says so and ends the recording.
    s.execute("macro.record", json!({})).unwrap();
    set(&mut s, "E3", "x");
    s.set_active(1);
    s.close_document(0);
    let err = s.execute("macro.record", json!({"stop": true, "name": "Gone"})).unwrap_err().to_string();
    assert!(err.contains("was closed"), "{err}");
    assert!(!s.is_recording());
    assert!(s.doc().unwrap().wb.macros.is_empty());
}

#[test]
fn a_name_given_when_recording_stops_never_replaces_a_macro() {
    let mut s = s();
    let original = json!({"command": "cell.set", "params": {"cell": "Z9", "input": "mine"}});
    put(&mut s, json!({"name": "Tidy", "description": "keep me", "steps": [original]}));
    s.execute("macro.record", json!({"relative": false})).unwrap();
    set(&mut s, "A1", "recorded");
    let r = s.execute("macro.record", json!({"stop": true, "name": "tidy"})).unwrap();
    assert_eq!(r["saved"], "tidy (2)");
    let kept = crate::macros::parse_macro(&s.execute("macro.get", json!({"name": "Tidy"})).unwrap()).unwrap();
    assert_eq!((kept.name.as_str(), kept.description.as_str(), kept.steps.len()), ("Tidy", "keep me", 1));
    assert_eq!(recorded(&mut s, "Tidy"), ["cell.set {\"cell\":\"Z9\",\"input\":\"mine\"}"]);
    assert_eq!(recorded(&mut s, "tidy (2)"), ["cell.set {\"cell\":\"A1\",\"input\":\"recorded\"}"]);
}

#[test]
fn steps_past_the_macro_limit_are_counted_as_dropped() {
    let mut s = s();
    s.execute("macro.record", json!({"relative": false})).unwrap();
    let (doc, selection) = (s.doc().unwrap().uid, s.doc().unwrap().selection.clone());
    let rec = s.recorder.as_mut().unwrap();
    for i in 0..gridcraft_model::MAX_MACRO_STEPS + 5 {
        rec.push(crate::record::RecordedStep {
            doc,
            sheet: 0,
            sheet_name: "Sheet1".into(),
            selection: selection.clone(),
            id: "cell.set".into(),
            params: json!({"cell": "A1", "input": i.to_string()}),
        });
    }
    rec.dropped = 3;
    let r = s.execute("macro.record", json!({"stop": true, "name": "Long"})).unwrap();
    assert_eq!(r["steps"], gridcraft_model::MAX_MACRO_STEPS);
    assert_eq!(r["dropped"], 8, "{r}");
}

#[test]
fn a_recording_waits_while_a_macro_in_its_workbook_is_debugged() {
    let mut s = s();
    stepper(&mut s);
    s.execute("macro.record", json!({"relative": false})).unwrap();
    set(&mut s, "Z1", "typed");
    debug(&mut s, json!({"name": "Walk", "inputs": "B2:C5"}));
    debug(&mut s, json!({"action": "step"}));
    // Saved now, the macro would go into the run's undo group, and stopping the run without
    // keeping its changes would delete it again.
    let r = s.execute("macro.record", json!({"stop": true, "name": "Typed"}));
    assert!(matches!(&r, Err(EngineError::Disabled(id, why)) if id == "macro.record" && why == crate::macros::debug::BUSY), "{r:?}");
    assert!(s.is_recording());
    debug(&mut s, json!({"action": "stop"}));
    let r = s.execute("macro.record", json!({"stop": true, "name": "Typed"})).unwrap();
    assert_eq!(r["saved"], "Typed");
    assert_eq!(recorded(&mut s, "Typed"), ["cell.set {\"cell\":\"Z1\",\"input\":\"typed\"}"]);
}
