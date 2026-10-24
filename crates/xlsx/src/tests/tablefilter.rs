//! A sheet AutoFilter that filters a table is saved as the table's own filter (a file can't hold
//! a sheet AutoFilter that overlaps a table), so the rows it hides reopen as filtered rows.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::*;

use crate::{read_xlsx, write_xlsx};

fn rr(a: &str) -> RangeRef {
    RangeRef::parse(a).unwrap()
}

fn table(range: &str, cols: &[&str]) -> Table {
    Table {
        id: 1,
        name: "T".into(),
        range: rr(range),
        header_row: true,
        totals_row: false,
        columns: cols
            .iter()
            .map(|n| TableColumn { name: (*n).into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None })
            .collect(),
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    }
}

/// Table B2:C6 (Name, Qty) with rows 4 and 6 hidden by a filter on Name.
fn filtered(af_range: &str, af_col: u32) -> Workbook {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.set_value(CellRef::new(1, 1), Value::text("Name"));
    s.set_value(CellRef::new(1, 2), Value::text("Qty"));
    for (i, n) in ["a", "b", "c", "d"].iter().enumerate() {
        let row = 2 + i as u32;
        s.set_value(CellRef::new(row, 1), Value::text(*n));
        s.set_value(CellRef::new(row, 2), Value::Number(10f64.powi(i as i32)));
    }
    let mut t = table("B2:C6", &["Name", "Qty"]);
    // A criterion the table already had (from the file it was opened from) on the Qty column.
    t.filter = vec![(1, FilterCriterion::Custom { a: (">".into(), "0".into()), b: None, and: true })];
    s.tables.push(t);
    s.autofilter = Some(AutoFilter {
        range: rr(af_range),
        criteria: vec![(af_col, FilterCriterion::Values { values: vec!["a".into(), "c".into()], blanks: false })],
    });
    for row in [3, 5] {
        s.rows.entry(row).or_default().hidden = true;
    }
    wb
}

#[test]
fn table_filter_through_the_sheet_autofilter_is_saved_with_the_table() {
    let wb = filtered("B2:C6", 0);
    assert!(wb.sheets[0].is_row_filtered(3));
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let s = back.sheet(0).unwrap();
    assert!(s.autofilter.is_none(), "no sheet AutoFilter over a table");
    let t = &s.tables[0];
    assert_eq!(t.filter.len(), 2, "{:?}", t.filter);
    assert_eq!(t.filter[0].0, 0);
    assert!(matches!(&t.filter[0].1, FilterCriterion::Values { values, .. } if values == &["a".to_string(), "c".to_string()]));
    assert!(matches!(&t.filter[1].1, FilterCriterion::Custom { .. }), "the table's own criterion is kept");
    for row in [3, 5] {
        assert!(s.is_row_filtered(row), "row {} reopens as filtered", row + 1);
    }
    assert!(!s.is_row_hidden(2) && !s.is_row_hidden(4));
}

#[test]
fn sheet_autofilter_starting_left_of_the_table_maps_its_columns() {
    // The filter starts one column left of the table on its header row: Name is column 1 of the
    // filter and column 0 of the table; a criterion on the column outside the table is dropped.
    let mut wb = filtered("A2:C6", 1);
    if let Some(af) = wb.sheet_mut(0).unwrap().autofilter.as_mut() {
        af.criteria.push((0, FilterCriterion::Values { values: vec!["zz".into()], blanks: true }));
    }
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let t = &back.sheet(0).unwrap().tables[0];
    let cols: Vec<u32> = t.filter.iter().map(|(c, _)| *c).collect();
    assert_eq!(cols, [0, 1]);
    assert!(matches!(&t.filter[0].1, FilterCriterion::Values { values, .. } if values == &["a".to_string(), "c".to_string()]));
}

#[test]
fn sheet_autofilter_not_on_the_header_row_is_not_folded() {
    let wb = filtered("B1:C6", 0);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let s = back.sheet(0).unwrap();
    assert!(s.autofilter.is_none());
    let cols: Vec<u32> = s.tables[0].filter.iter().map(|(c, _)| *c).collect();
    assert_eq!(cols, [1], "only the table's own criterion");
}
