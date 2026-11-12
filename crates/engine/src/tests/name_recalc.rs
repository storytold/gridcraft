//! Recalculation after a hidden name is rewritten without a structural edit (custom views, the
//! Watch Window list): only the formulas reading the name are recalculated.

use super::*;

#[test]
fn rewriting_a_hidden_name_recalculates_only_its_readers() {
    // Saving a custom view rewrites a hidden name without a structural edit. The formulas that
    // read that name, directly or through another name, are recalculated; the rest of the
    // workbook isn't (a full recalculation of a 50K-formula workbook for a saved view would
    // take as long as opening it).
    let mut s = Session::new();
    s.new_workbook();
    for r in 1..=50 {
        s.execute("cell.set", json!({"cell": format!("B{r}"), "input": format!("=ROW()+{r}")})).unwrap();
    }
    // The name two steps away is defined before the name it reads: it is found through which
    // names mention which, not by the order the names are listed in.
    s.execute("formulas.defineName", json!({"name": "Twice_removed", "refersTo": "=Alias+1"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Alias", "refersTo": "=LEN(_sc_custom_views)"})).unwrap();
    s.execute("view.customViews", json!({"save": "first"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_custom_views)"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=Alias"})).unwrap();
    s.execute("cell.set", json!({"cell": "E1", "input": "=Twice_removed"})).unwrap();
    // A LAMBDA reading the name, called from a cell and from another name's definition.
    s.execute("formulas.defineName", json!({"name": "View_length", "refersTo": "=LAMBDA(extra,LEN(_sc_custom_views)+extra)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Called_length", "refersTo": "=View_length(1)"})).unwrap();
    s.execute("cell.set", json!({"cell": "F1", "input": "=View_length(0)"})).unwrap();
    s.execute("cell.set", json!({"cell": "G1", "input": "=Called_length"})).unwrap();
    let len = |s: &Session, at: &str| match s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(at).unwrap()) {
        Value::Number(n) => n,
        other => panic!("{at}: expected a length, got {other:?}"),
    };
    let (a_before, d_before, e_before) = (len(&s, "A1"), len(&s, "D1"), len(&s, "E1"));
    assert!(a_before > 0.0 && d_before == a_before && e_before == a_before + 1.0);
    assert_eq!((len(&s, "F1"), len(&s, "G1")), (a_before, a_before + 1.0));
    s.execute("view.customViews", json!({"save": "second view with a longer name"})).unwrap();
    assert!(len(&s, "A1") > a_before, "the direct reader sees the new definition");
    assert!(len(&s, "D1") > d_before, "the reader through another name sees it too");
    assert!(len(&s, "E1") > e_before, "and the reader two names away");
    assert_eq!(len(&s, "F1"), len(&s, "A1"), "the cell calling a LAMBDA that reads it");
    assert_eq!(len(&s, "G1"), len(&s, "A1") + 1.0, "and the name calling it");
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 5, "only the five readers were recalculated");
}

#[test]
fn a_formula_reading_the_watch_list_name_follows_the_list() {
    // The list is kept in a hidden defined name and changed without an undo step; a formula
    // that reads that name is recalculated like the reader of any other name.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(gridcraft_core::CellError::Name));
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number("[\"Sheet1!B1\"]".len() as f64));
    let steps = s.doc().unwrap().undo.len();
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    assert_eq!(s.doc().unwrap().undo.len(), steps, "no undo step for the watch list");
}

#[test]
fn undo_and_redo_keep_the_watch_list_and_recalculate_its_readers() {
    // Undo and redo restore an earlier workbook but carry the current watch list into it. The
    // restored workbook was calculated with its own list, so a formula reading the list's name
    // must be recalculated for the carried one; nothing else depends on the name.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    let one = Value::Number("[\"Sheet1!B1\"]".len() as f64);
    assert_eq!(v(&s, "A1"), one);
    // The workbook before B1 was set had no watch list and A1 = #NAME?.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), one, "the carried list is read after undo");
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    // The workbook redo restores had the one-entry list and A1 = 13.
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0), "the carried list is read after redo");
}
