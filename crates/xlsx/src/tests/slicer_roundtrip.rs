//! Table slicers survive a save → reopen: the slicer widget, its table/column binding and its
//! selection (carried by the table column's filter) all round-trip, and the file carries the real
//! Excel slicer and slicer-cache parts.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Anchor, AnchorMode, Cell, FilterCriterion, Slicer, Table, TableColumn, TotalsFn, Workbook};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

/// A workbook with a `Sales` table (Region/Amount over A1:B4) and one slicer on `Region`.
fn book_with_slicer(selected: Option<Vec<String>>) -> Workbook {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for (c, v) in [("A1", "Region"), ("B1", "Amount"), ("A2", "East"), ("A3", "West"), ("A4", "East")] {
        sh.cells.set(at(c), Cell::value(Value::text(v)));
    }
    for (c, n) in [("B2", 10.0), ("B3", 20.0), ("B4", 30.0)] {
        sh.cells.set(at(c), Cell::value(Value::Number(n)));
    }
    sh.tables.push(Table {
        id: 1,
        name: "Sales".into(),
        range: RangeRef::parse("A1:B4").unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![
            TableColumn { name: "Region".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None },
            TableColumn { name: "Amount".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None },
        ],
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    sh.slicers.push(Slicer {
        id: 42,
        table: "Sales".into(),
        column: "Region".into(),
        caption: "Region".into(),
        anchor: Anchor { cell: at("D1"), dx: 4.0, dy: 4.0, width: 168.0, height: 200.0, mode: AnchorMode::MoveOnly },
        selected,
        columns: 2,
    });
    wb
}

#[test]
fn slicer_with_selection_round_trips() {
    let wb = book_with_slicer(Some(vec!["East".into()]));
    let bytes = write_xlsx(&wb).unwrap();

    // The real Excel parts are present.
    let mut pkg = Package::open(&bytes).unwrap();
    let slicers = pkg.read_xml("xl/slicers/slicer1.xml").unwrap().expect("slicer part");
    let sl = slicers.kids("slicer").next().expect("a <slicer>");
    assert_eq!(sl.attr("caption"), Some("Region"));
    assert_eq!(sl.attr("columnCount"), Some("2"));
    let cache = pkg.read_xml("xl/slicerCaches/slicerCache1.xml").unwrap().expect("cache part");
    let tsc = cache.child("extLst").unwrap().kids("ext").find_map(|e| e.child("tableSlicerCache")).expect("tableSlicerCache");
    assert_eq!(tsc.attr("tableId"), Some("1"));
    assert_eq!(tsc.attr("column"), Some("1"));
    // The selection persisted as the table column's own filter.
    let table = pkg.read_xml("xl/tables/table1.xml").unwrap().unwrap();
    assert!(table.child("autoFilter").and_then(|af| af.child("filterColumn")).is_some(), "table carries the slicer filter");

    // And it reads back as a slicer with the same binding and selection.
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.is_empty(), "no read warnings: {:?}", report.warnings);
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.slicers.len(), 1, "one slicer restored");
    let s = &sh.slicers[0];
    assert_eq!(s.table, "Sales");
    assert_eq!(s.column, "Region");
    assert_eq!(s.caption, "Region");
    assert_eq!(s.columns, 2);
    assert_eq!(s.selected.as_deref(), Some(["East".to_string()].as_slice()));
    // The drawing anchor round-trips (read back through the graphicFrame / sle:slicer path).
    assert_eq!(s.anchor.cell, at("D1"), "slicer keeps its on-sheet position");
    // The bound column keeps its filter, so the rows stay filtered rather than orphaned.
    assert!(matches!(sh.tables[0].filter.first(), Some((0, FilterCriterion::Values { .. }))));
}

#[test]
fn slicer_showing_all_round_trips_without_a_filter() {
    let wb = book_with_slicer(None);
    let bytes = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.slicers.len(), 1);
    assert_eq!(sh.slicers[0].selected, None, "no selection means no filter");
    assert!(sh.tables[0].filter.is_empty(), "no orphaned filter when everything is selected");
}
