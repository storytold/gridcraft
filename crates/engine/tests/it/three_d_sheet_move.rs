//! Issue #3741: moving an end sheet of a 3-D reference past its other end.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

/// Jan, Feb, Mar, Extra, Audit; `Audit!A1` is `=SUM(Jan:Mar!A1)` over 10, 20, 30 (Extra holds 100).
fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "sheet.rename", json!({"name": "Jan"}));
    for name in ["Feb", "Mar", "Extra", "Audit"] {
        run(&mut s, "home.insertSheet", json!({"name": name}));
    }
    for (sheet, input) in [("Jan", "10"), ("Feb", "20"), ("Mar", "30"), ("Extra", "100"), ("Audit", "=SUM(Jan:Mar!A1)")] {
        run(&mut s, "cell.set", json!({"sheet": sheet, "cell": "A1", "input": input}));
    }
    s
}

fn audit(s: &mut Session) -> (String, Json) {
    let r = s.execute("sheet.read", json!({"sheet": "Audit", "range": "A1", "formulas": true})).unwrap();
    let formula = r["values"][0][0].as_str().unwrap_or_default().to_string();
    let v = s.execute("sheet.read", json!({"sheet": "Audit", "range": "A1"})).unwrap();
    (formula, v["values"][0][0].clone())
}

#[test]
fn an_end_sheet_moved_past_the_other_end_leaves_the_span() {
    // Jan after Mar: Feb, Mar, Jan. Jan is no longer in the span, which shrinks to Feb:Mar.
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Jan", "to": 2}));
    assert_eq!(audit(&mut s), ("=SUM(Feb:Mar!A1)".to_string(), json!(50.0)));

    // Mar before Jan: Mar, Jan, Feb. Mar is no longer in the span, which shrinks to Jan:Feb.
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Mar", "to": 0}));
    assert_eq!(audit(&mut s), ("=SUM(Jan:Feb!A1)".to_string(), json!(30.0)));
}

#[test]
fn the_shrunk_span_survives_save_undo_and_redo() {
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Jan", "to": 2}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(audit(&mut s), ("=SUM(Jan:Mar!A1)".to_string(), json!(60.0)));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(audit(&mut s), ("=SUM(Feb:Mar!A1)".to_string(), json!(50.0)));
}

#[test]
fn moves_that_keep_the_ends_in_order_leave_the_span_alone() {
    // Extra into the span: it is now between the ends, so it is summed.
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Extra", "to": 1}));
    assert_eq!(audit(&mut s), ("=SUM(Jan:Mar!A1)".to_string(), json!(160.0)));

    // Feb inside the span, moved elsewhere inside it: nothing changes.
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Feb", "to": 1}));
    assert_eq!(audit(&mut s), ("=SUM(Jan:Mar!A1)".to_string(), json!(60.0)));

    // A sheet that is not an end of the span.
    let mut s = book();
    run(&mut s, "sheet.move", json!({"sheet": "Audit", "to": 0}));
    assert_eq!(audit(&mut s), ("=SUM(Jan:Mar!A1)".to_string(), json!(60.0)));
}

#[test]
fn a_two_sheet_span_whose_end_moves_past_the_other_becomes_one_sheet() {
    // Jan:Feb with Jan moved after Feb: only Feb is left inside, as when an end sheet is deleted.
    let mut s = book();
    run(&mut s, "cell.set", json!({"sheet": "Audit", "cell": "A1", "input": "=SUM(Jan:Feb!A1)"}));
    run(&mut s, "sheet.move", json!({"sheet": "Jan", "to": 1}));
    assert_eq!(audit(&mut s), ("=SUM(Feb!A1)".to_string(), json!(20.0)));
}

#[test]
fn a_defined_name_over_a_span_follows_the_move() {
    let mut s = book();
    run(&mut s, "formulas.defineName", json!({"name": "FirstQuarter", "refersTo": "=Jan:Mar!$A$1"}));
    run(&mut s, "sheet.move", json!({"sheet": "Jan", "to": 2}));
    let formula = s.doc().unwrap().wb.names.iter().find(|n| n.name == "FirstQuarter").map(|n| n.formula.clone()).unwrap();
    assert_eq!(formula, "Feb:Mar!$A$1");
}
