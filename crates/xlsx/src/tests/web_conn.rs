//! From Web connections in XLSX: `xl/connections.xml` holds one type-5 web-query connection
//! per imported table, with GridCraft's extension element carrying the parts Excel's own
//! attributes cannot (the page table, its occurrence, the target sheet, the stable id, the
//! last sync). A workbook with no connections writes no part at all.

use crate::*;

fn wb_with_connections() -> Workbook {
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Web")));
    wb.web_queries.push(gridcraft_model::WebQuery {
        name: "prices".into(),
        table_id: Some(7),
        url: "https://example.com/table.html".into(),
        table: "prices".into(),
        occurrence: 2,
        sheet: "Web".into(),
        last_synced: Some(1_700_000_000_000),
        refresh_on_open: true,
    });
    wb.web_queries.push(gridcraft_model::WebQuery {
        name: "weather".into(),
        table_id: None,
        url: "https://example.org/wx".into(),
        table: "Weather Table".into(),
        occurrence: 0,
        sheet: "Web".into(),
        last_synced: None,
        refresh_on_open: false,
    });
    wb
}

#[test]
/// A saved connection round-trips through XLSX: the URL, the page table, its occurrence, the
/// target sheet, the stable id, the last sync and Excel's "refresh on open" all survive a
/// save and reopen, so Sync and refresh-on-open keep working.
fn web_connections_round_trip_through_xlsx() {
    let bytes = write_xlsx(&wb_with_connections()).expect("the workbook saves");
    let (wb, _report) = read_xlsx(&bytes).expect("it reopens");
    assert_eq!(wb.web_queries.len(), 2, "both connections came back");
    let q = wb.web_queries.iter().find(|q| q.name == "prices").expect("the first connection");
    assert_eq!(q.url, "https://example.com/table.html");
    assert_eq!(q.table, "prices");
    assert_eq!(q.occurrence, 2);
    assert_eq!(q.sheet, "Web");
    assert_eq!(q.table_id, Some(7));
    assert_eq!(q.last_synced, Some(1_700_000_000_000));
    assert!(q.refresh_on_open, "refreshOnLoad survives the round trip");
    let w = wb.web_queries.iter().find(|q| q.name == "weather").expect("the second connection");
    assert_eq!(q_occ(w), 0);
    assert!(!w.refresh_on_open);
    assert_eq!(w.table, "Weather Table");
    assert_eq!(w.last_synced, None);
}

fn q_occ(q: &gridcraft_model::WebQuery) -> usize {
    q.occurrence
}

#[test]
/// A workbook with no connections writes no connections part, and a file whose connections
/// part holds a foreign connection (a database query) keeps it on save without this model
/// claiming it.
fn no_connections_writes_no_part() {
    let wb = wb_with_connections();
    let bytes = write_xlsx(&wb).expect("saves");
    let pkg = package::Package::open(&bytes).expect("opens");
    assert!(pkg.has("xl/connections.xml"), "the connections part is there when connections are");
    // A workbook without them writes none: the round trip above added them.
    let mut plain = Workbook::default();
    plain.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    plain.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Aux")));
    let bytes2 = write_xlsx(&plain).expect("saves");
    let pkg2 = package::Package::open(&bytes2).expect("opens");
    assert!(!pkg2.has("xl/connections.xml"), "no connections part without connections");
}
