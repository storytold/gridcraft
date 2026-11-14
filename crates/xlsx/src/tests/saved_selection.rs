//! The selected areas saved with a sheet (`<selection sqref activeCellId>`): read as Excel opens
//! them and written back.

use gridcraft_core::{CellRef, RangeRef};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

fn at(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

fn rr(a1: &str) -> RangeRef {
    RangeRef::parse(a1).unwrap()
}

/// The first sheet as read from a file whose sheet view holds `selection`.
fn read(selection: &str) -> gridcraft_model::Sheet {
    let body = format!(r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0">{selection}</sheetView></sheetViews><sheetData/>"#);
    let (wb, _) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    wb.sheet(0).unwrap().clone()
}

/// The `<selection …/>` element of the first sheet as written.
fn written_selection(sheet: gridcraft_model::Sheet) -> String {
    let (mut wb, _) = read_xlsx(&minimal("<sheetData/>", &[], "", "")).unwrap();
    *wb.sheet_mut(0).unwrap() = sheet;
    let bytes = write_xlsx(&wb).unwrap();
    let xml = String::from_utf8(crate::package::Package::open(&bytes).unwrap().read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap();
    let start = xml.find("<selection").unwrap();
    let len = xml[start..].find("/>").unwrap() + 2;
    xml[start..start + len].to_string()
}

#[test]
fn a_selected_range_opens_selected() {
    let s = read(r#"<selection activeCell="B6" sqref="A6:B6"/>"#);
    assert_eq!(s.view_active, at("B6"));
    assert_eq!(s.view_ranges, vec![rr("A6:B6")]);
    assert_eq!(written_selection(s), r#"<selection activeCell="B6" sqref="A6:B6"/>"#);
}

#[test]
fn several_areas_keep_the_active_cells_area_last() {
    // Excel opens both areas with D3 active: the active cell's area is `activeCellId`.
    let s = read(r#"<selection activeCell="D3" activeCellId="1" sqref="A6:B6 D2:D4"/>"#);
    assert_eq!((s.view_active, s.view_ranges.clone()), (at("D3"), vec![rr("A6:B6"), rr("D2:D4")]));
    assert_eq!(written_selection(s), r#"<selection activeCell="D3" activeCellId="1" sqref="A6:B6 D2:D4"/>"#);
    // Without `activeCellId` the active cell is in the first area.
    let s = read(r#"<selection activeCell="B6" sqref="A6:B6 D2:D4"/>"#);
    assert_eq!((s.view_active, s.view_ranges), (at("B6"), vec![rr("D2:D4"), rr("A6:B6")]));
}

#[test]
fn an_active_cell_outside_its_area_moves_to_the_areas_first_cell() {
    // As Excel opens it.
    let s = read(r#"<selection activeCell="F9" sqref="A6:B6"/>"#);
    assert_eq!((s.view_active, s.view_ranges), (at("A6"), vec![rr("A6:B6")]));
}

#[test]
fn whole_columns_and_a_single_cell() {
    let s = read(r#"<selection activeCell="C1" sqref="C:D"/>"#);
    assert_eq!((s.view_active, s.view_ranges.clone()), (at("C1"), vec![RangeRef::cols(2, 3)]));
    assert_eq!(written_selection(s), r#"<selection activeCell="C1" sqref="C:D"/>"#);
    // The active cell alone keeps no areas.
    let s = read(r#"<selection activeCell="C5" sqref="C5"/>"#);
    assert_eq!((s.view_active, s.view_ranges.is_empty()), (at("C5"), true));
}

#[test]
fn malformed_selections_keep_the_active_cell() {
    // An area index past the list, and areas that do not parse.
    for sel in [r#"<selection activeCell="C5" activeCellId="3" sqref="A1:B2"/>"#, r#"<selection activeCell="C5" sqref="?? !!"/>"#] {
        let s = read(sel);
        assert_eq!((s.view_active, s.view_ranges.is_empty()), (at("C5"), true), "{sel}");
    }
    // Areas that no longer hold the active cell are not written.
    let mut s = read(r#"<selection activeCell="B6" sqref="A6:B6"/>"#);
    s.view_active = at("H8");
    assert_eq!(written_selection(s), r#"<selection activeCell="H8" sqref="H8"/>"#);
}

#[test]
fn a_huge_sqref_keeps_a_bounded_number_of_areas() {
    let areas: Vec<String> = (1..=5000).map(|r| format!("A{r}")).collect();
    let s = read(&format!(r#"<selection activeCell="A1" sqref="{}"/>"#, areas.join(" ")));
    assert!(s.view_ranges.len() <= 1024, "{}", s.view_ranges.len());
}
