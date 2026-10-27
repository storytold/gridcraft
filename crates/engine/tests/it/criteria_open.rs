use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::{Cell, Formula, Sheet, Workbook};
use serde_json::json;

#[test]
fn opening_synthetic_criteria_workbook_recalculates_and_accepts_edits() {
    let rows = 2_860;
    let mut wb = Workbook::new();
    wb.sheets.push(Arc::new(Sheet::new("Criteria")));
    wb.sheets.push(Arc::new(Sheet::new("Summary")));
    for r in 0..rows {
        let key = Value::from(format!("Customer-{:05}-North", r % 97));
        let data = wb.sheet_mut(0).unwrap();
        data.cells.set(CellRef::new(r, 0), Cell::value(key.clone()));
        data.cells.set(CellRef::new(r, 1), Cell::value(Value::Number(1.0)));
        let criteria = wb.sheet_mut(1).unwrap();
        criteria.cells.set(CellRef::new(r, 0), Cell::value(key));
        let row = r + 1;
        for (col, formula) in [
            format!("=COUNTIF(Sheet1!A:A,A{row})"),
            format!("=SUMIF(Sheet1!A$1:A${rows},A{row},Sheet1!B$1:B${rows})"),
            format!("=AVERAGEIF(Sheet1!A:A,A{row},Sheet1!B:B)"),
        ]
        .into_iter()
        .enumerate()
        {
            criteria.cells.set(CellRef::new(r, col as u32 + 1), Cell::formula(Formula::new(&formula)));
        }
    }
    // Same sheet extents as the reported private file, with fewer formula columns for CI.
    wb.sheet_mut(0).unwrap().cells.set(CellRef::new(rows - 1, 60), Cell::value(Value::Number(1.0)));
    wb.sheet_mut(1).unwrap().cells.set(CellRef::new(rows + 10, 58), Cell::value(Value::Number(1.0)));
    wb.sheet_mut(2).unwrap().cells.set(CellRef::new(6, 16), Cell::value(Value::Number(1.0)));
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let mut session = Session::new();
    session.execute("file.open", json!({"name": "synthetic-2751.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    for r in 0..rows {
        let sheet = session.doc().unwrap().wb.sheet(1).unwrap();
        let count = f64::from((rows - 1 - r % 97) / 97 + 1);
        assert_eq!(sheet.value(CellRef::new(r, 1)), Value::Number(count));
        assert_eq!(sheet.value(CellRef::new(r, 2)), Value::Number(count));
        assert_eq!(sheet.value(CellRef::new(r, 3)), Value::Number(1.0));
    }
    session.execute("cell.set", json!({"cell": "B1", "input": "100"})).unwrap();
    let sheet = session.doc().unwrap().wb.sheet(1).unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 1)), Value::Number(30.0));
    assert_eq!(sheet.value(CellRef::new(0, 2)), Value::Number(129.0));
    assert_eq!(sheet.value(CellRef::new(0, 3)), Value::Number(129.0 / 30.0));
}
