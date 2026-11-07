//! A frozen sheet can be scrolled two ways at once: the frozen (top-left) pane itself can be
//! scrolled past column A (its own `<sheetView topLeftCell>`), independently of where the
//! scrolling (bottom-right) pane sits (`<pane topLeftCell>`). Before this fix only the second
//! was kept - reopening a file like that snapped the frozen pane back to column A, exposing
//! whatever the author had scrolled past (seen on a real file: helper/debug cells in A1:D7
//! became visible because the frozen header rows' own horizontal scroll, away from A, was lost).

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn frozen_book() -> Workbook {
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("ЗИП3");
    sheet.freeze = Some((8, 12));
    sheet.view_top_left = CellRef::parse("Q16").unwrap(); // scrolling (bottom-right) pane
    sheet.view_top_left_frozen = Some(CellRef::parse("E8").unwrap()); // frozen (top-left) pane
    wb.sheets = vec![Arc::new(sheet)];
    wb
}

#[test]
fn frozen_pane_keeps_its_own_scroll_independent_of_the_scrolling_pane() {
    let (read, _) = read_xlsx(&write_xlsx(&frozen_book()).unwrap()).unwrap();
    let sheet = &read.sheets[0];
    assert_eq!(sheet.view_top_left_frozen, Some(CellRef::parse("E8").unwrap()));
    assert_eq!(sheet.view_top_left, CellRef::parse("Q16").unwrap());
    assert_eq!(sheet.freeze, Some((8, 12)));
}

/// The written XML shape matches what Excel itself writes: `topLeftCell` on `<sheetView>` is
/// the frozen pane's own scroll, `topLeftCell` on `<pane>` is the scrolling pane's.
#[test]
fn written_xml_has_both_top_left_cells() {
    let bytes = write_xlsx(&frozen_book()).unwrap();
    let mut xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap().by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut xml).unwrap();
    let sheet_view = xml.split("<pane").next().unwrap();
    assert!(sheet_view.contains("topLeftCell=\"E8\""), "{xml}");
    assert!(xml.contains("topLeftCell=\"Q16\" activePane="), "{xml}");
}

/// A sheet with no frozen panes keeps writing `topLeftCell` on `<sheetView>` as before (no
/// regression on the common, unfrozen case).
#[test]
fn unfrozen_sheet_still_writes_top_left_cell_on_sheet_view() {
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Sheet1");
    sheet.view_top_left = CellRef::parse("C10").unwrap();
    wb.sheets = vec![Arc::new(sheet)];
    let (read, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(read.sheets[0].view_top_left, CellRef::parse("C10").unwrap());
    assert_eq!(read.sheets[0].view_top_left_frozen, None);
}

/// A frozen sheet whose frozen pane was never scrolled away from A1 writes no `topLeftCell` on
/// `<sheetView>` (nothing to recover - a plain frozen-header sheet stays unchanged).
#[test]
fn frozen_sheet_with_no_scroll_writes_no_sheet_view_top_left_cell() {
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Sheet1");
    sheet.freeze = Some((1, 0));
    wb.sheets = vec![Arc::new(sheet)];
    let bytes = write_xlsx(&wb).unwrap();
    let mut xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap().by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut xml).unwrap();
    let sheet_view = xml.split("<pane").next().unwrap();
    assert!(!sheet_view.contains("topLeftCell"), "{xml}");
}
