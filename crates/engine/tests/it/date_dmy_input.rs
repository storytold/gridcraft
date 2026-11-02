//! Issue #3085: Date inputs in cells formatted as dd/mm/yyyy.

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::display::{cell_text, edit_text};
use serde_json::json;

#[test]
fn custom_formatted_dd_mm_yyyy_accepts_dmy_inputs() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    // Format A1 and A2 with custom format "dd/mm/yyyy"
    s.execute("home.numberFormat", json!({"range": "A1:A2", "code": "dd/mm/yyyy"})).unwrap();

    // Input 25/09/2026 into A1 (day > 12)
    s.execute("cell.set", json!({"cell": "A1", "input": "25/09/2026"})).unwrap();
    // Input 05/04/2026 into A2 (ambiguous day/month: must be 5 April 2026 in dd/mm/yyyy)
    s.execute("cell.set", json!({"cell": "A2", "input": "05/04/2026"})).unwrap();

    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();

    // A1 must be serial 46290.0 (2026-09-25)
    let cell_a1 = sh.cell(CellRef::new(0, 0)).unwrap();
    assert_eq!(cell_a1.value, Value::Number(46290.0));
    assert_eq!(cell_text(&d.wb, sh, CellRef::new(0, 0)), "25/09/2026");
    assert_eq!(edit_text(&d.wb, sh, CellRef::new(0, 0)), "25/09/2026");

    // A2 must be serial 46117.0 (2026-04-05: 5 April 2026), NOT 46146.0 (May 4)
    let cell_a2 = sh.cell(CellRef::new(1, 0)).unwrap();
    assert_eq!(cell_a2.value, Value::Number(46117.0));
    assert_eq!(cell_text(&d.wb, sh, CellRef::new(1, 0)), "05/04/2026");
    assert_eq!(edit_text(&d.wb, sh, CellRef::new(1, 0)), "05/04/2026");
}

#[test]
fn general_cell_accepts_unambiguous_day_first_date() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();

    // In a General cell, 25/09/2026 has day 25 (> 12) so it must parse as 25 September 2026
    s.execute("cell.set", json!({"cell": "A1", "input": "25/09/2026"})).unwrap();

    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();

    let cell_a1 = sh.cell(CellRef::new(0, 0)).unwrap();
    assert_eq!(cell_a1.value, Value::Number(46290.0));
    assert_eq!(d.wb.styles.get(cell_a1.style).num_fmt.as_str(), "dd/mm/yyyy");
    assert_eq!(cell_text(&d.wb, sh, CellRef::new(0, 0)), "25/09/2026");
}

#[test]
fn range_set_values_respects_dd_mm_yyyy_cell_format() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("home.numberFormat", json!({"range": "A1:B1", "code": "dd/mm/yyyy"})).unwrap();
    s.execute("range.setValues", json!({"range": "A1:B1", "values": [["05/04/2026", "25/09/2026"]]})).unwrap();

    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();

    // 05/04/2026 in dd/mm/yyyy is 5 April 2026
    assert_eq!(sh.cell(CellRef::new(0, 0)).unwrap().value, Value::Number(46117.0));
    assert_eq!(cell_text(&d.wb, sh, CellRef::new(0, 0)), "05/04/2026");

    // 25/09/2026 in dd/mm/yyyy is 25 September 2026
    assert_eq!(sh.cell(CellRef::new(0, 1)).unwrap().value, Value::Number(46290.0));
    assert_eq!(cell_text(&d.wb, sh, CellRef::new(0, 1)), "25/09/2026");
}
