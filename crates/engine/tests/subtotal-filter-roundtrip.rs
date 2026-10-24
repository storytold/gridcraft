//! SUBTOTAL over filtered rows keeps its value through an XLSX save and reopen.
//!
//! Excel (observed in Windows Excel 16 with synthetic workbooks) saves rows a filter hides as plain
//! `hidden="1"` rows; a hidden row counts as filtered when it lies in an AutoFilter (sheet or table)
//! that has criteria. Excel can't hold a sheet AutoFilter that overlaps a table, so GridCraft saves
//! a table's filter in the table part and never creates a sheet AutoFilter over part of a table.

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::calc::Calc;
use gridcraft_engine::model::Workbook;
use gridcraft_engine::{Session, io};
use serde_json::{Value as Json, json};

fn reopened(wb: &Workbook) -> Workbook {
    let bytes = io::save_bytes(wb, "t.xlsx").unwrap();
    let (mut back, _) = io::open_bytes("t.xlsx", &bytes).unwrap();
    Calc::new().recalc_all(&mut back);
    back
}

/// Every formula value in memory against the same cell after a save and reopen.
fn assert_values_survive(wb: &Workbook) {
    let back = reopened(wb);
    for (a, b) in wb.sheets.iter().zip(&back.sheets) {
        for (c, cell) in a.cells.iter() {
            if cell.formula.is_some() {
                assert_eq!(cell.value, b.value(c), "{}!{} ({:?})", a.name, c.a1(), cell.formula.as_ref().map(|f| &f.text));
            }
        }
    }
}

fn session(sample: &str, steps: &[(&str, Json)]) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": sample})).unwrap();
    for (id, p) in steps {
        // A refused step leaves the book as it was; the asserts check what survives.
        let _ = s.execute(id, p.clone());
    }
    s
}

fn wb(s: &Session) -> &Workbook {
    &s.doc().unwrap().wb
}

#[test]
fn bead_repro_filter_then_replace_on_the_sales_sample() {
    let s =
        session("sales", &[("data.filterBy", json!({"column":"G","top":{"count":2}})), ("edit.replace", json!({"all":true,"what":"1","with":""}))]);
    let sh = wb(&s).active().unwrap();
    assert!(sh.autofilter.as_ref().is_none_or(|af| !sh.tables.iter().any(|t| t.range.intersects(&af.range))));
    assert_values_survive(wb(&s));
}

#[test]
fn bead_repro_subtotal_then_filter_on_the_grades_sample() {
    let s = session(
        "grades",
        &[
            ("home.insertColumns", json!({})),
            ("data.subtotal", json!({"columns":["G"],"function":"sum","groupBy":"J"})),
            ("data.filterBy", json!({"column":"B","values":["x"]})),
        ],
    );
    let sh = wb(&s).active().unwrap();
    assert!(sh.autofilter.as_ref().is_none_or(|af| !sh.tables.iter().any(|t| t.range.intersects(&af.range))));
    assert_values_survive(wb(&s));
}

/// A table A1:B7 (Name, Qty = 1, 10, 100...) with SUBTOTAL(9) and SUBTOTAL(109) beside it.
fn table_book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1","values":[["Name","Qty"],["a",1],["b",10],["c",100],["d",1000],["e",10000],["f",100000]]}))
        .unwrap();
    s.execute("insert.table", json!({"range":"A1:B7","header":true})).unwrap();
    s.execute("cell.set", json!({"cell":"E1","input":"=SUBTOTAL(9,B2:B7)"})).unwrap();
    s.execute("cell.set", json!({"cell":"E2","input":"=SUBTOTAL(109,B2:B7)"})).unwrap();
    s.execute("selection.set", json!({"cell":"A2"})).unwrap();
    s
}

fn e1_e2(wb: &Workbook) -> (Value, Value) {
    let sh = wb.active().unwrap();
    (sh.value(CellRef::new(0, 4)), sh.value(CellRef::new(1, 4)))
}

#[test]
fn table_filter_keeps_subtotal_and_criteria_through_save() {
    let mut s = table_book();
    s.execute("data.filterBy", json!({"column":"A","values":["a","c","e","f"]})).unwrap();
    // Like Excel, a row hidden by hand inside an active filter range is left out by SUBTOTAL 9 too.
    s.execute("home.hideRows", json!({"rows":"6:6"})).unwrap();
    assert_eq!(e1_e2(wb(&s)), (Value::Number(100101.0), Value::Number(100101.0)));
    assert_values_survive(wb(&s));
    let back = reopened(wb(&s));
    assert_eq!(e1_e2(&back), (Value::Number(100101.0), Value::Number(100101.0)));
    let t = &back.active().unwrap().tables[0];
    assert_eq!(t.filter.len(), 1, "the criteria are saved with the table: {:?}", t.filter);
}

#[test]
fn rows_hidden_by_hand_without_a_filter_still_count_for_subtotal_9() {
    let mut s = table_book();
    s.execute("home.hideRows", json!({"rows":"3:3"})).unwrap();
    s.execute("home.hideRows", json!({"rows":"5:5"})).unwrap();
    assert_eq!(e1_e2(wb(&s)), (Value::Number(111111.0), Value::Number(110101.0)));
    let back = reopened(wb(&s));
    assert_eq!(e1_e2(&back), (Value::Number(111111.0), Value::Number(110101.0)));
}

#[test]
fn a_narrower_range_inside_a_filtered_table_is_refused_not_dropped() {
    let mut s = table_book();
    s.execute("data.filterBy", json!({"column":"A","values":["a"]})).unwrap();
    let back = reopened(wb(&s));
    let mut s = Session::new();
    s.add_document(gridcraft_engine::DocState::new(back, None, "t".into(), gridcraft_engine::Trust::FromFile));
    s.execute("selection.set", json!({"cell":"D1"})).unwrap();
    assert!(s.execute("data.filter", json!({"range":"B1:B7"})).is_err());
    assert_eq!(wb(&s).active().unwrap().tables[0].filter.len(), 1, "the table's criteria are kept");
}

#[test]
fn filtering_a_reopened_table_again_keeps_both_criteria() {
    let mut s = table_book();
    s.execute("data.filterBy", json!({"column":"A","values":["a","c","e","f"]})).unwrap();
    let back = reopened(wb(&s));
    let mut s = Session::new();
    s.add_document(gridcraft_engine::DocState::new(back, None, "t".into(), gridcraft_engine::Trust::FromFile));
    s.execute("selection.set", json!({"cell":"A2"})).unwrap();
    s.execute("data.filterBy", json!({"column":"B","custom":{"op":">","value":"50"}})).unwrap();
    let sh = wb(&s).active().unwrap();
    assert!(sh.tables[0].filter.is_empty(), "the table's criteria moved into the live filter");
    assert_eq!(sh.autofilter.as_ref().map(|af| af.criteria.len()), Some(2));
    assert_eq!(e1_e2(wb(&s)), (Value::Number(110100.0), Value::Number(110100.0)));
    assert_values_survive(wb(&s));
    assert_eq!(reopened(wb(&s)).active().unwrap().tables[0].filter.len(), 2);
}

#[test]
fn a_sheet_filter_from_outside_a_table_leaves_the_table_out() {
    // Excel: table B4:C8 with text in A1:A3, A5:A8 and B3. AutoFilter from A1 covers A1:A3, from
    // A6 A5:A8, from B3 A1:B3; an explicit range over part of the table is refused.
    let layout = || {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range":"A1","values":[["Title"],["x"],["y"]]})).unwrap();
        s.execute("cell.set", json!({"cell":"B3","input":"z"})).unwrap();
        s.execute("range.setValues", json!({"range":"B4","values":[["Name","Qty"],["n5","v5"],["n6","v6"],["n7","v7"],["n8","v8"]]})).unwrap();
        s.execute("range.setValues", json!({"range":"A5","values":[["a5"],["a6"],["a7"],["a8"]]})).unwrap();
        s.execute("insert.table", json!({"range":"B4:C8","header":true})).unwrap();
        s
    };
    for (cell, want) in [("A1", "A1:A3"), ("A6", "A5:A8"), ("B3", "A1:B3")] {
        let mut s = layout();
        s.execute("selection.set", json!({"cell": cell})).unwrap();
        s.execute("data.filter", json!({})).unwrap();
        let af = wb(&s).active().unwrap().autofilter.clone();
        assert_eq!(af.map(|a| a.range.a1()), Some(want.to_string()), "from {cell}");
    }
    let mut s = layout();
    assert!(s.execute("data.filter", json!({"range":"A1:C8"})).is_err());
    assert!(wb(&s).active().unwrap().autofilter.is_none());
}
