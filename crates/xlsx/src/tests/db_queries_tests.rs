//! Database queries behind tables survive a round trip; queries of deleted tables are not written.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::*;

use crate::{read_xlsx, write_xlsx};

fn table(id: u32, name: &str, range: &str) -> Table {
    Table {
        id,
        name: name.into(),
        range: RangeRef::parse(range).unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![TableColumn { name: "id".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None }],
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

fn query(table_id: u32) -> DbQuery {
    DbQuery {
        table_id,
        driver: "mysql".into(),
        connection: "mysql://10.0.0.5:3306/r_lab".into(),
        user: Some("bob".into()),
        query: "SELECT `id` FROM `orders` WHERE x < 'a&b'".into(),
        password_required: true,
        max_rows: Some(500),
        timeout_secs: None,
        trusted: false,
    }
}

#[test]
fn queries_round_trip_and_follow_their_tables() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::parse("A1").unwrap(), Value::text("id"));
    sh.set_value(CellRef::parse("A2").unwrap(), Value::Number(1.0));
    sh.tables.push(table(7, "Orders", "A1:A2"));
    wb.db_queries = vec![query(7), query(99)];
    let bytes = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let tid = back.sheet(0).unwrap().tables[0].id;
    assert_eq!(back.db_queries, vec![DbQuery { table_id: tid, ..query(7) }], "the query of the missing table 99 is dropped");
    // The part is read into the model, not kept a second time.
    assert!(back.preserved.as_ref().is_none_or(|p| p.parts.iter().all(|x| !x.name.contains("gridcraftDbQueries"))));
    let (again, _) = read_xlsx(&write_xlsx(&back).unwrap()).unwrap();
    assert_eq!(again.db_queries, back.db_queries);
}

#[test]
fn no_part_without_queries() {
    let bytes = write_xlsx(&Workbook::new()).unwrap();
    let pkg = String::from_utf8_lossy(&bytes);
    assert!(!pkg.contains("gridcraftDbQueries"));
}
