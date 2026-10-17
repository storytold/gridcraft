use gridcraft_calc::eval::{Evaluator, Host, MAX_CELLS};
use gridcraft_core::{CellError, CellRef, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, LineInfo, Workbook};

/// An oversized sparse rectangle must be rejected before even its first value is read.
/// The guard makes the regression safe on the old implementation too: it fails immediately
/// instead of allocating a dense vector of millions of blank values.
struct NoReads {
    wb: Workbook,
    reads: usize,
}

impl NoReads {
    fn sparse_rectangle() -> Self {
        let mut wb = Workbook::new();
        let end = CellRef::new(MAX_ROWS - 1, 15);
        assert!(RangeRef::new(CellRef::new(0, 0), end).count() > MAX_CELLS);
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_cell(CellRef::new(0, 0), Cell::value(Value::Error(CellError::NA)));
        sh.set_cell(end, Cell::value(Value::Number(2.0)));
        // Skipping hidden rows or errors must not bypass the scan budget.
        sh.rows.insert(0, LineInfo { hidden: true, ..LineInfo::default() });
        Self { wb, reads: 0 }
    }

    fn evaluate(&mut self, formula: &str) -> Value {
        let expr = gridcraft_formula::parse(formula).unwrap();
        Evaluator::new(self, 0, CellRef::new(0, 16)).value(&expr)
    }
}

impl Host for NoReads {
    fn workbook(&self) -> &Workbook {
        &self.wb
    }

    fn cell_value(&mut self, _sheet: usize, _cell: CellRef) -> Value {
        self.reads += 1;
        panic!("oversized aggregate input was read before its budget was checked");
    }

    fn now_serial(&self) -> f64 {
        0.0
    }

    fn random(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn subtotal_refuses_oversized_sparse_input_before_reading_values() {
    for function in [9, 109] {
        let mut host = NoReads::sparse_rectangle();
        let formula = format!("=SUBTOTAL({function},A1:P{MAX_ROWS})");
        assert_eq!(host.evaluate(&formula), Value::Error(CellError::Num), "{formula}");
        assert_eq!(host.reads, 0, "{formula}");
    }
}

#[test]
fn aggregate_refuses_oversized_sparse_input_for_every_skip_option() {
    for option in 0..=7 {
        for (function, k) in [(9, ""), (14, ",1")] {
            let mut host = NoReads::sparse_rectangle();
            let formula = format!("=AGGREGATE({function},{option},A1:P{MAX_ROWS}{k})");
            assert_eq!(host.evaluate(&formula), Value::Error(CellError::Num), "{formula}");
            assert_eq!(host.reads, 0, "{formula}");
        }
    }
}
