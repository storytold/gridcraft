//! Structural edits (insert and delete of lines, sheet rename and delete) rewrite only the formulas
//! whose tree they change. A formula they leave alone keeps its stored text and its flags.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_engine::Session;
use gridcraft_model::{Cell, Formula};
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "home.insertSheet", json!({"name": "Other"}));
    run(&mut s, "home.insertSheet", json!({"name": "Spare"}));
    run(&mut s, "sheet.activate", json!({"sheet": "Sheet1"}));
    s
}

fn formula(s: &Session, sheet: &str, cell: &str) -> Arc<Formula> {
    let wb = &s.doc().unwrap().wb;
    wb.sheet(wb.sheet_index(sheet).unwrap()).unwrap().cell(CellRef::parse(cell).unwrap()).unwrap().formula.clone().unwrap()
}

/// Stores a formula with this exact text and no cached tree, as a JSON workbook loads it.
fn put_raw(s: &mut Session, cell: &str, text: &str) {
    let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    let i = wb.sheet_index("Sheet1").unwrap();
    let f = Formula { text: text.to_string(), expr: None, array: None, dynamic: false };
    wb.sheet_mut(i).unwrap().cells.set(CellRef::parse(cell).unwrap(), Cell::formula(f));
}

#[test]
fn an_edit_that_leaves_a_formula_alone_keeps_its_stored_text() {
    // The rewrite compares trees, not printed text, so a stored non-canonical text is not
    // re-printed by an edit that does not change the formula. An edit that changes it prints it.
    let mut s = workbook();
    put_raw(&mut s, "C1", "sum(a1,  Other!b2)");
    run(&mut s, "home.insertRows", json!({"rows": "5:5"}));
    run(&mut s, "home.insertColumns", json!({"cols": "F:F"}));
    run(&mut s, "sheet.rename", json!({"sheet": "Spare", "name": "Spare2"}));
    run(&mut s, "home.deleteSheet", json!({"sheet": "Spare2"}));
    assert_eq!(formula(&s, "Sheet1", "C1").text, "sum(a1,  Other!b2)");

    run(&mut s, "home.insertRows", json!({"rows": "1:1"}));
    assert_eq!(formula(&s, "Sheet1", "C2").text, "SUM(A2,Other!B2)");
}

#[test]
fn renaming_a_referenced_sheet_rewrites_the_formula() {
    let mut s = workbook();
    put_raw(&mut s, "C1", "sum(a1,  Other!b2)");
    run(&mut s, "sheet.rename", json!({"sheet": "Other", "name": "Data"}));
    assert_eq!(formula(&s, "Sheet1", "C1").text, "SUM(A1,Data!B2)");
    run(&mut s, "home.deleteSheet", json!({"sheet": "Data"}));
    assert_eq!(formula(&s, "Sheet1", "C1").text, "SUM(A1,#REF!)");
}

#[test]
fn inserting_rows_above_a_legacy_array_moves_its_range_when_the_formula_is_unchanged() {
    let mut s = workbook();
    run(&mut s, "range.setValues", json!({"range": "B1", "values": [[1], [2]]}));
    run(&mut s, "selection.set", json!({"range": "C5:C6"}));
    run(&mut s, "cell.set", json!({"cell": "C5", "input": "=B1:B2*2", "array": true}));
    assert_eq!(formula(&s, "Sheet1", "C5").array, Some(RangeRef::parse("C5:C6").unwrap()));
    run(&mut s, "home.insertRows", json!({"rows": "3:3"}));
    let f = formula(&s, "Sheet1", "C6");
    assert_eq!(f.text, "B1:B2*2");
    assert_eq!(f.array, Some(RangeRef::parse("C6:C7").unwrap()));
}

#[test]
fn deleting_an_unrelated_sheet_keeps_legacy_arrays() {
    let mut s = workbook();
    run(&mut s, "range.setValues", json!({"range": "B1", "values": [[1], [2]]}));
    run(&mut s, "selection.set", json!({"range": "C1:C2"}));
    run(&mut s, "cell.set", json!({"cell": "C1", "input": "=B1:B2*2", "array": true}));
    run(&mut s, "home.deleteSheet", json!({"sheet": "Spare"}));
    assert_eq!(formula(&s, "Sheet1", "C1").array, Some(RangeRef::parse("C1:C2").unwrap()));
}

#[test]
fn reorder_keeps_unaffected_formula_text_and_shared_storage() {
    for rows in [true, false] {
        let mut s = workbook();
        put_raw(&mut s, "H8", "sum(a1,  Other!b2)");
        let original = formula(&s, "Sheet1", "H8");
        run(
            &mut s,
            if rows { "sheet.moveRows" } else { "sheet.moveColumns" },
            if rows { json!({"rows":"2:3", "before":5}) } else { json!({"cols":"B:C", "before":5}) },
        );
        let after = formula(&s, "Sheet1", "H8");
        assert_eq!(after.text, "sum(a1,  Other!b2)");
        assert!(Arc::ptr_eq(&original, &after), "unaffected formulas must keep their shared allocation");
    }
}

#[test]
fn reorder_moves_array_range_when_its_expression_is_unchanged() {
    for rows in [true, false] {
        for cached in [true, false] {
            let mut s = workbook();
            let (source, at, destination, new_at) = if rows { ("C5:C6", "C5", "C2:C3", "C2") } else { ("E3:F3", "E3", "B3:C3", "B3") };
            run(&mut s, "cell.set", json!({"cell":"A1", "input":"5"}));
            run(&mut s, "selection.set", json!({"range":source}));
            run(&mut s, "cell.set", json!({"cell":at, "input":"=$A$1+1", "array":true}));
            // Preserve a noncanonical stored spelling with either a parsed or text-only tree.
            // The reference is outside the permutation, but the array's own range must move.
            let wb = Arc::make_mut(&mut s.doc_mut().unwrap().wb);
            let i = wb.sheet_index("Sheet1").unwrap();
            let cell = wb.sheet_mut(i).unwrap().cells.get_mut(CellRef::parse(at).unwrap()).unwrap();
            let f = Arc::make_mut(cell.formula.as_mut().unwrap());
            f.text = "$a$1 + 1".into();
            if !cached {
                f.expr = None;
            }
            let original = s.doc().unwrap().wb.clone();
            let undo = s.doc().unwrap().undo.len();
            run(
                &mut s,
                if rows { "sheet.moveRows" } else { "sheet.moveColumns" },
                if rows { json!({"rows":"5:6", "before":1}) } else { json!({"cols":"E:F", "before":1}) },
            );
            let f = formula(&s, "Sheet1", new_at);
            assert_eq!(f.text, "$a$1 + 1");
            assert_eq!(f.array, Some(RangeRef::parse(destination).unwrap()));
            assert!(!f.dynamic);
            assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
            let moved = s.doc().unwrap().wb.clone();
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(*s.doc().unwrap().wb, *original);
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(*s.doc().unwrap().wb, *moved);
        }
    }
}
