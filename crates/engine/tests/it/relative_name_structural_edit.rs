//! A defined name keeps the relative parts of its references as offsets from A1 (see
//! `define_name`), so inserting or deleting rows or columns must not move them: the name still
//! means "the cell in my row" or "the cell to my left" for the cell that uses it. Only the absolute
//! parts follow the edit. The Name Manager shows the stored text, which is what these tests read.

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "sheet.rename", json!({"name": "Source"}));
    s
}

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn put(s: &mut Session, at: &str, values: Json) {
    run(s, "range.setValues", json!({"range": at, "values": values}));
}

/// Defines `name` as seen from the active cell `at`, as the Define Name dialog does.
fn define(s: &mut Session, at: &str, name: &str, refers_to: &str) {
    run(s, "selection.set", json!({"range": at}));
    run(s, "formulas.defineName", json!({"name": name, "refersTo": refers_to}));
}

fn add_sheet(s: &mut Session, name: &str) {
    run(s, "home.insertSheet", json!({"name": name}));
    run(s, "sheet.activate", json!({"sheet": "Source"}));
}

fn insert_rows(s: &mut Session, rows: &str) {
    run(s, "home.insertRows", json!({"rows": rows}));
}

fn insert_cols(s: &mut Session, cols: &str) {
    run(s, "home.insertColumns", json!({"cols": cols}));
}

fn delete_rows(s: &mut Session, rows: &str) {
    run(s, "home.deleteRows", json!({"rows": rows}));
}

fn delete_cols(s: &mut Session, cols: &str) {
    run(s, "home.deleteColumns", json!({"cols": cols}));
}

fn num(v: f64) -> Value {
    Value::Number(v)
}

fn value(s: &Session, cell: &str) -> Value {
    let wb = &s.doc().unwrap().wb;
    wb.sheet(wb.sheet_index("Source").unwrap()).unwrap().value(CellRef::parse(cell).unwrap())
}

/// The definition as the Name Manager lists it: the stored text, relative to A1.
fn stored(s: &mut Session, name: &str) -> String {
    let list = s.execute("formulas.nameManager", json!({})).unwrap();
    let entry = list.as_array().unwrap().iter().find(|n| n["name"] == name).unwrap().clone();
    entry["refersTo"].as_str().unwrap().to_string()
}

// ---- Relative references stay put -------------------------------------------------------------

/// `=Source!$A1` from A1: column A (absolute) of the calling cell's own row.
fn row_name() -> Session {
    let mut s = workbook();
    put(&mut s, "A1", json!([[10], [20], [30]]));
    define(&mut s, "A1", "Amount", "=Source!$A1");
    put(&mut s, "B1", json!([["=Amount*2"], ["=Amount*2"], ["=Amount*2"]]));
    assert_eq!((value(&s, "B1"), value(&s, "B2"), value(&s, "B3")), (num(20.0), num(40.0), num(60.0)));
    s
}

#[test]
fn a_relative_row_name_survives_inserting_a_row_above() {
    let mut s = row_name();
    let before = stored(&mut s, "Amount");
    insert_rows(&mut s, "1:1");
    // The inputs and the formulas moved down together, so the results do not change.
    assert_eq!((value(&s, "B2"), value(&s, "B3"), value(&s, "B4")), (num(20.0), num(40.0), num(60.0)));
    assert_eq!(stored(&mut s, "Amount"), before);
}

#[test]
fn a_relative_row_name_survives_inserting_a_row_in_the_middle() {
    let mut s = row_name();
    insert_rows(&mut s, "2:2");
    assert_eq!((value(&s, "B1"), value(&s, "B3"), value(&s, "B4")), (num(20.0), num(40.0), num(60.0)));
    assert_eq!(stored(&mut s, "Amount"), "=Source!$A1");
}

#[test]
fn a_relative_row_name_survives_deleting_a_row_above() {
    let mut s = row_name();
    delete_rows(&mut s, "1:1");
    // Row 1 went with its input and formula; the other two keep their own row's input.
    assert_eq!((value(&s, "B1"), value(&s, "B2")), (num(40.0), num(60.0)));
    assert_eq!(stored(&mut s, "Amount"), "=Source!$A1");
}

/// `=Source!A$1` from A1: row 1 (absolute) of the calling cell's own column.
fn column_name() -> Session {
    let mut s = workbook();
    put(&mut s, "A1", json!([[10, 20, 30]]));
    define(&mut s, "A1", "Top", "=Source!A$1");
    put(&mut s, "A2", json!([["=Top*2", "=Top*2", "=Top*2"]]));
    assert_eq!((value(&s, "A2"), value(&s, "B2"), value(&s, "C2")), (num(20.0), num(40.0), num(60.0)));
    s
}

#[test]
fn a_relative_column_name_survives_inserting_a_column_to_the_left() {
    let mut s = column_name();
    let before = stored(&mut s, "Top");
    insert_cols(&mut s, "A:A");
    assert_eq!((value(&s, "B2"), value(&s, "C2"), value(&s, "D2")), (num(20.0), num(40.0), num(60.0)));
    assert_eq!(stored(&mut s, "Top"), before);
}

#[test]
fn a_relative_column_name_survives_deleting_a_column_to_the_left() {
    let mut s = column_name();
    delete_cols(&mut s, "A:A");
    assert_eq!((value(&s, "A2"), value(&s, "B2")), (num(40.0), num(60.0)));
    assert_eq!(stored(&mut s, "Top"), "=Source!A$1");
}

// ---- "The cell to my left": stored as a wrapped offset (XFD) ----------------------------------

#[test]
fn a_name_for_the_cell_to_the_left_survives_inserting_a_row() {
    let mut s = workbook();
    put(&mut s, "A1", json!([[1], [2], [3]]));
    define(&mut s, "B2", "Left", "=Source!A2");
    put(&mut s, "B1", json!([["=Left*10"], ["=Left*10"], ["=Left*10"]]));
    assert_eq!(stored(&mut s, "Left"), "=Source!XFD1");
    assert_eq!((value(&s, "B1"), value(&s, "B2"), value(&s, "B3")), (num(10.0), num(20.0), num(30.0)));

    insert_rows(&mut s, "1:1");
    assert_eq!((value(&s, "B2"), value(&s, "B3"), value(&s, "B4")), (num(10.0), num(20.0), num(30.0)));
    assert_eq!(stored(&mut s, "Left"), "=Source!XFD1");
}

/// Inserting a column used to push the wrapped column offset past the edge of the sheet and turn
/// the whole definition into #REF!.
#[test]
fn a_name_for_the_cell_to_the_left_survives_inserting_a_column() {
    let mut s = workbook();
    put(&mut s, "A1", json!([[1], [2], [3]]));
    define(&mut s, "B2", "Left", "=Source!A2");
    put(&mut s, "B1", json!([["=Left*10"], ["=Left*10"], ["=Left*10"]]));

    insert_cols(&mut s, "A:A");
    assert_eq!((value(&s, "C1"), value(&s, "C2"), value(&s, "C3")), (num(10.0), num(20.0), num(30.0)));
    assert_eq!(stored(&mut s, "Left"), "=Source!XFD1");
}

#[test]
fn a_name_for_the_cell_to_the_left_survives_deleting_a_column() {
    let mut s = workbook();
    put(&mut s, "B1", json!([[1], [2], [3]]));
    define(&mut s, "C2", "Left", "=Source!B2");
    put(&mut s, "C1", json!([["=Left*10"], ["=Left*10"], ["=Left*10"]]));
    assert_eq!((value(&s, "C1"), value(&s, "C2"), value(&s, "C3")), (num(10.0), num(20.0), num(30.0)));

    delete_cols(&mut s, "A:A");
    assert_eq!((value(&s, "B1"), value(&s, "B2"), value(&s, "B3")), (num(10.0), num(20.0), num(30.0)));
    assert_eq!(stored(&mut s, "Left"), "=Source!XFD1");
}

// ---- Absolute references follow the edit ------------------------------------------------------

/// `Fixed` is the third input, `=Source!$A$3`, used from C3.
fn absolute_name() -> Session {
    let mut s = workbook();
    put(&mut s, "A1", json!([[10], [20], [30]]));
    define(&mut s, "A1", "Fixed", "=Source!$A$3");
    put(&mut s, "C3", json!([["=Fixed"]]));
    assert_eq!(value(&s, "C3"), num(30.0));
    s
}

#[test]
fn an_absolute_name_follows_an_inserted_row() {
    let mut s = absolute_name();
    insert_rows(&mut s, "1:1");
    assert_eq!(stored(&mut s, "Fixed"), "=Source!$A$4");
    assert_eq!(value(&s, "C4"), num(30.0));
}

#[test]
fn an_absolute_name_follows_an_inserted_column() {
    let mut s = absolute_name();
    insert_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Fixed"), "=Source!$B$3");
    assert_eq!(value(&s, "D3"), num(30.0));
}

#[test]
fn an_absolute_name_follows_a_deleted_row() {
    let mut s = absolute_name();
    delete_rows(&mut s, "1:1");
    assert_eq!(stored(&mut s, "Fixed"), "=Source!$A$2");
    assert_eq!(value(&s, "C2"), num(30.0));
}

#[test]
fn an_absolute_name_turns_into_ref_when_its_row_is_deleted() {
    let mut s = absolute_name();
    delete_rows(&mut s, "3:3");
    assert_eq!(stored(&mut s, "Fixed"), "=#REF!");
}

#[test]
fn an_absolute_name_turns_into_ref_when_its_column_is_deleted() {
    let mut s = absolute_name();
    delete_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Fixed"), "=#REF!");
    assert_eq!(value(&s, "B3"), Value::Error(CellError::Ref));
}

// ---- Mixed references: the absolute half moves, the relative half stays -----------------------

#[test]
fn a_row_absolute_name_follows_a_row_insert_but_not_a_column_insert() {
    let mut s = column_name();
    insert_rows(&mut s, "1:1");
    assert_eq!(stored(&mut s, "Top"), "=Source!A$2");
    assert_eq!((value(&s, "A3"), value(&s, "B3"), value(&s, "C3")), (num(20.0), num(40.0), num(60.0)));

    insert_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Top"), "=Source!A$2");
    assert_eq!((value(&s, "B3"), value(&s, "C3"), value(&s, "D3")), (num(20.0), num(40.0), num(60.0)));
}

#[test]
fn a_column_absolute_name_follows_a_column_insert_but_not_a_row_insert() {
    let mut s = row_name();
    insert_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Amount"), "=Source!$B1");
    assert_eq!((value(&s, "C1"), value(&s, "C2"), value(&s, "C3")), (num(20.0), num(40.0), num(60.0)));

    insert_rows(&mut s, "1:1");
    assert_eq!(stored(&mut s, "Amount"), "=Source!$B1");
    assert_eq!((value(&s, "C2"), value(&s, "C3"), value(&s, "C4")), (num(20.0), num(40.0), num(60.0)));
}

// ---- Ranges and their boundaries --------------------------------------------------------------

#[test]
fn an_absolute_range_grows_moves_or_stays_with_the_insertion_point() {
    let mut s = workbook();
    define(&mut s, "A1", "Rng", "=Source!$A$1:$A$3");
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$1:$A$3");

    insert_rows(&mut s, "2:2"); // inside: the range grows
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$1:$A$4");
    insert_rows(&mut s, "6:6"); // past the end: nothing changes
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$1:$A$4");
    insert_rows(&mut s, "5:5"); // just past the end (row 5 is one after row 4): nothing changes
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$1:$A$4");
    insert_rows(&mut s, "1:1"); // at the first row: the whole range moves
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$2:$A$5");
}

#[test]
fn an_absolute_range_shrinks_when_rows_inside_it_are_deleted() {
    let mut s = workbook();
    define(&mut s, "A1", "Rng", "=Source!$A$2:$A$6");
    delete_rows(&mut s, "3:4");
    assert_eq!(stored(&mut s, "Rng"), "=Source!$A$2:$A$4");
    delete_rows(&mut s, "2:4"); // the whole range
    assert_eq!(stored(&mut s, "Rng"), "=#REF!");
}

#[test]
fn a_relative_range_is_not_stretched_by_an_insert_or_delete_inside_it() {
    let mut s = workbook();
    define(&mut s, "A1", "Rel", "=Source!A1:A3");
    assert_eq!(stored(&mut s, "Rel"), "=Source!A1:A3");
    insert_rows(&mut s, "2:2");
    assert_eq!(stored(&mut s, "Rel"), "=Source!A1:A3");
    delete_rows(&mut s, "1:2");
    assert_eq!(stored(&mut s, "Rel"), "=Source!A1:A3");
    insert_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Rel"), "=Source!A1:A3");
    delete_cols(&mut s, "A:A");
    assert_eq!(stored(&mut s, "Rel"), "=Source!A1:A3");
}

#[test]
fn a_range_with_one_absolute_end_moves_only_that_end() {
    let mut s = workbook();
    define(&mut s, "A1", "Half", "=Source!$A$1:$A3");
    assert_eq!(stored(&mut s, "Half"), "=Source!$A$1:$A3");
    insert_rows(&mut s, "1:1");
    assert_eq!(stored(&mut s, "Half"), "=Source!$A$2:$A3");
    delete_rows(&mut s, "2:2"); // the absolute end's row goes
    assert_eq!(stored(&mut s, "Half"), "=#REF!");
}

// ---- References to another sheet --------------------------------------------------------------

#[test]
fn names_that_point_at_another_sheet_follow_edits_on_that_sheet_only() {
    let mut s = workbook();
    add_sheet(&mut s, "Other");
    define(&mut s, "A1", "OtherAbs", "=Other!$A$1");
    define(&mut s, "A1", "OtherRel", "=Other!$A1");

    // An edit on the sheet the names are not about leaves both alone.
    insert_rows(&mut s, "1:1");
    assert_eq!((stored(&mut s, "OtherAbs"), stored(&mut s, "OtherRel")), ("=Other!$A$1".to_string(), "=Other!$A1".to_string()));

    // An edit on the sheet they point at moves the absolute one; the relative one is an offset.
    run(&mut s, "home.insertRows", json!({"rows": "1:1", "sheet": "Other"}));
    assert_eq!((stored(&mut s, "OtherAbs"), stored(&mut s, "OtherRel")), ("=Other!$A$2".to_string(), "=Other!$A1".to_string()));

    run(&mut s, "home.insertColumns", json!({"cols": "A:A", "sheet": "Other"}));
    assert_eq!((stored(&mut s, "OtherAbs"), stored(&mut s, "OtherRel")), ("=Other!$B$2".to_string(), "=Other!$B1".to_string()));
}
