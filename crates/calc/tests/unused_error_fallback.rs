//! Fictional regressions for IFERROR fallback reads during bounded-stack suspension.

use std::sync::Arc;

use gridcraft_calc::Calc;
use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, Formula, Sheet, Workbook};

fn put(wb: &mut Workbook, sheet: usize, row: u32, col: u32, formula: &str) {
    wb.sheet_mut(sheet).unwrap().cells.set(CellRef::new(row, col), Cell::formula(Formula::new(formula)));
}

fn workbook(fallback: &str) -> Workbook {
    let mut wb = Workbook::new();
    wb.sheets.push(Arc::new(Sheet::new("Data")));
    for row in 1..64 {
        put(&mut wb, 0, row - 1, 0, &format!("=A{}+1", row + 1));
    }
    put(&mut wb, 0, 63, 0, &format!("=IFERROR(SUM(Data!B1:B128),{fallback})"));
    for row in 0..128 {
        put(&mut wb, 1, row, 1, "=1");
    }
    put(&mut wb, 1, 0, 3, "=Sheet1!A1+1");
    wb
}

fn value(wb: &Workbook, sheet: usize, row: u32, col: u32) -> Value {
    wb.sheet(sheet).unwrap().value(CellRef::new(row, col))
}

fn assert_acyclic(wb: &mut Workbook, calc: &mut Calc) {
    calc.recalc_all(wb);
    assert!(calc.circular.is_empty(), "unused fallback must not create a cycle: {:?}", calc.circular);
    assert_eq!(value(wb, 0, 0, 0), Value::Number(191.0));
    assert_eq!(value(wb, 1, 0, 3), Value::Number(192.0));
}

#[test]
fn unused_direct_fallback_is_acyclic() {
    assert_acyclic(&mut workbook("Data!D1"), &mut Calc::new());
}

#[test]
fn unused_streamed_range_fallback_is_acyclic() {
    assert_acyclic(&mut workbook("SUM(Data!D1:D2)"), &mut Calc::new());
}

#[test]
fn unused_materialized_range_fallback_is_acyclic() {
    assert_acyclic(&mut workbook("SUMPRODUCT(Data!D1:D2)"), &mut Calc::new());
}

#[test]
fn genuinely_used_cyclic_fallback_is_reported_and_can_recover() {
    for fallback in ["Data!D1", "SUM(Data!D1:D2)", "SUMPRODUCT(Data!D1:D2)"] {
        let mut wb = workbook(fallback);
        put(&mut wb, 1, 0, 1, "=1/0");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert!(!calc.circular.is_empty(), "{fallback}");
        assert_eq!(value(&wb, 0, 0, 0), Value::Error(CellError::Circ), "{fallback}");
        assert_eq!(value(&wb, 1, 0, 3), Value::Error(CellError::Circ), "{fallback}");
        put(&mut wb, 1, 0, 1, "=1");
        assert_acyclic(&mut wb, &mut calc);
    }
}

#[test]
fn cyclic_range_dependencies_are_reported_and_can_recover() {
    let mut wb = workbook("0");
    // A real back-edge from the range to an ancestor in the suspended chain.
    put(&mut wb, 1, 0, 1, "=Sheet1!A10+1");
    let mut calc = Calc::new();
    calc.recalc_all(&mut wb);
    assert!(!calc.circular.is_empty());
    assert_eq!(value(&wb, 1, 0, 1), Value::Error(CellError::Circ));
    put(&mut wb, 1, 0, 1, "=1");
    assert_acyclic(&mut wb, &mut calc);
    // Two dependencies batched from the same range must still detect their mutual cycle.
    put(&mut wb, 1, 0, 1, "=B128+1");
    put(&mut wb, 1, 127, 1, "=B1+1");
    calc.recalc_all(&mut wb);
    assert!(!calc.circular.is_empty());
    assert_eq!(value(&wb, 1, 0, 1), Value::Error(CellError::Circ));
    assert_eq!(value(&wb, 1, 127, 1), Value::Error(CellError::Circ));
    put(&mut wb, 1, 0, 1, "=1");
    put(&mut wb, 1, 127, 1, "=1");
    assert_acyclic(&mut wb, &mut calc);
}

#[test]
fn genuinely_used_acyclic_fallback_returns_its_value() {
    let mut wb = workbook("7");
    put(&mut wb, 1, 0, 1, "=1/0");
    let mut calc = Calc::new();
    calc.recalc_all(&mut wb);
    assert!(calc.circular.is_empty());
    assert_eq!(value(&wb, 0, 0, 0), Value::Number(70.0));
    assert_eq!(value(&wb, 1, 0, 3), Value::Number(71.0));
}
