//! A copy over the old 2M-cell guard reaches the clipboard text in full; one over the byte limit
//! is an error, never a cut-short text.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_engine::display::range_text;
use gridcraft_model::Workbook;

fn filled(rows: u32, cols: u32) -> Workbook {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for r in 0..rows {
        for c in 0..cols {
            sh.set_value(CellRef::new(r, c), Value::Number(1.0));
        }
    }
    wb
}

#[test]
fn three_million_cells_copy_in_full() {
    let wb = filled(200_000, 15);
    let r = RangeRef::new(CellRef::new(0, 0), CellRef::new(199_999, 14));
    let text = range_text(&wb, 0, r).unwrap();
    assert_eq!(text.lines().count(), 200_000);
    assert_eq!(text.len(), 200_000 * (15 + 14 + 1));
}

#[test]
fn a_copy_over_the_byte_limit_is_an_error() {
    // An empty range still costs a tab per cell, so 17G cells pass the limit quickly.
    let wb = Workbook::new();
    let r = RangeRef::new(CellRef::new(0, 0), CellRef::new(1_048_575, 16_383));
    let err = range_text(&wb, 0, r).unwrap_err().to_string();
    assert!(err.contains("too big to copy") && err.contains("256 MiB"), "{err}");
}
