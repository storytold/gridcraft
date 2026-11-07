//! Database queries behind tables survive a round trip; queries of deleted tables are not written.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::*;

use crate::db_queries::{MAX_PART, NS_DB_QUERIES, Stored, attach};
use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

/// A one-column table with a header row.
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

/// A query as the import keeps it, with characters XML has to escape.
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

#[test]
fn queries_take_free_part_names_next_to_kept_custom_xml() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::parse("A1").unwrap(), Value::text("id"));
    sh.set_value(CellRef::parse("A2").unwrap(), Value::Number(1.0));
    sh.tables.push(table(7, "Orders", "A1:A2"));
    wb.db_queries = vec![query(7)];
    // Another tool's custom XML that happens to use the same part name, kept from the source file.
    let other = b"<?xml version=\"1.0\"?><other xmlns=\"urn:example:other\">kept</other>".to_vec();
    wb.preserved = Some(std::sync::Arc::new(preserved::Preserved {
        rels: vec![preserved::PreservedRel {
            id: "rId90".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml".into(),
            target: "customXml/gridcraftDbQueries.xml".into(),
            external: false,
        }],
        parts: vec![preserved::PreservedPart {
            name: "customXml/gridcraftDbQueries.xml".into(),
            content_type: "application/xml".into(),
            data: other.into(),
            rels: vec![],
        }],
        ..Default::default()
    }));
    let bytes = write_xlsx(&wb).expect("saving must not fail on a duplicate part name");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let tid = back.sheet(0).unwrap().tables[0].id;
    assert_eq!(back.db_queries, vec![DbQuery { table_id: tid, ..query(7) }]);
    let kept = back.preserved.as_ref().map(|p| p.parts.iter().map(|x| x.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
    assert!(kept.iter().any(|n| n == "customXml/gridcraftDbQueries.xml"), "the other part is kept: {kept:?}");
    assert!(write_xlsx(&back).is_ok());
}

/// One sheet named `sheet`, with a one-column table `name` at `range`.
fn workbook_with(sheet: &str, name: &str, range: &str, id: u32) -> Workbook {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = sheet.into();
    let r = RangeRef::parse(range).unwrap();
    sh.set_value(r.start, Value::text("id"));
    sh.set_value(CellRef::new(r.start.row + 1, r.start.col), Value::Number(1.0));
    sh.tables.push(table(id, name, range));
    wb
}

/// A query as read from the part, naming its table by `table` and by `sheet` and `anchor`.
fn stored(table: Option<&str>, sheet: &str, anchor: &str) -> Stored {
    Stored { table: table.map(str::to_string), sheet: sheet.into(), anchor: anchor.into(), query: query(0) }
}

#[test]
fn queries_follow_their_table_by_name_when_the_sheet_is_renamed_or_the_table_moves() {
    // As Excel saves the file after renaming the sheet and inserting rows above the table:
    // the part still names the old sheet and top-left cell.
    let mut wb = workbook_with("Renamed", "Orders", "B5:B6", 3);
    attach(&mut wb, vec![stored(Some("orders"), "Old", "A1")]);
    assert_eq!(wb.db_queries.iter().map(|q| q.table_id).collect::<Vec<_>>(), [3]);
    // Files from before the name was stored: sheet and top-left cell.
    let mut wb = workbook_with("Data", "Orders", "A1:A2", 3);
    attach(&mut wb, vec![stored(None, "Data", "A1")]);
    assert_eq!(wb.db_queries.len(), 1);
    // A name that no table has, or that two tables share, attaches to nothing, even where the
    // sheet and cell would match.
    let mut wb = workbook_with("Data", "Renamed", "A1:A2", 3);
    attach(&mut wb, vec![stored(Some("Orders"), "Data", "A1")]);
    assert!(wb.db_queries.is_empty());
    let mut wb = workbook_with("Data", "Orders", "A1:A2", 3);
    wb.sheets.push(Sheet::new("Other").into());
    wb.sheet_mut(1).unwrap().tables.push(table(4, "Orders", "A1:A2"));
    attach(&mut wb, vec![stored(Some("Orders"), "Data", "A1")]);
    assert!(wb.db_queries.is_empty());
}

#[test]
fn the_name_stored_is_the_one_written() {
    // The writer turns the space into `_`: the query names `My_Orders`, as the table part does.
    let mut wb = workbook_with("Data", "My Orders", "A1:A2", 7);
    wb.db_queries = vec![query(7)];
    let bytes = write_xlsx(&wb).unwrap();
    let part = Package::open(&bytes).unwrap().read_xml("customXml/gridcraftDbQueries.xml").unwrap().unwrap();
    assert!(part.text.contains(r#""table":"My_Orders""#), "{}", part.text);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.db_queries.len(), 1);
    assert_eq!(back.db_queries[0].table_id, back.sheet(0).unwrap().tables[0].id);
}

#[test]
fn a_name_two_sheets_use_falls_back_to_sheet_and_cell() {
    let mut wb = workbook_with("One", "Orders", "A1:A2", 7);
    let mut two = Sheet::new("Two");
    two.set_value(CellRef::parse("C3").unwrap(), Value::text("id"));
    two.set_value(CellRef::parse("C4").unwrap(), Value::Number(2.0));
    two.tables.push(table(8, "Orders", "C3:C4"));
    wb.sheets.push(two.into());
    wb.db_queries = vec![query(8)];
    let bytes = write_xlsx(&wb).unwrap();
    let part = Package::open(&bytes).unwrap().read_xml("customXml/gridcraftDbQueries.xml").unwrap().unwrap();
    assert!(!part.text.contains(r#""table""#), "no name that two tables share: {}", part.text);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.db_queries.iter().map(|q| q.table_id).collect::<Vec<_>>(), [back.sheet(1).unwrap().tables[0].id]);
}

/// A workbook keeping `data` as custom XML part `name` (as read from another file).
fn keeping(mut wb: Workbook, name: &str, data: &[u8]) -> Workbook {
    wb.preserved = Some(std::sync::Arc::new(preserved::Preserved {
        rels: vec![preserved::PreservedRel {
            id: "rId90".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml".into(),
            target: name.into(),
            external: false,
        }],
        parts: vec![preserved::PreservedPart { name: name.into(), content_type: "application/xml".into(), data: data.to_vec().into(), rels: vec![] }],
        ..Default::default()
    }));
    wb
}

/// The bytes of kept part `name`, if the workbook keeps it.
fn kept_data(wb: &Workbook, name: &str) -> Option<Vec<u8>> {
    wb.preserved.as_ref()?.parts.iter().find(|p| p.name == name).map(|p| p.data.to_vec())
}

#[test]
fn custom_xml_that_only_looks_like_ours_is_kept() {
    let foreign = [
        // Our element name nested in another document, our namespace mentioned in the first bytes.
        format!(r#"<?xml version="1.0"?><doc xmlns="urn:example" ref="{NS_DB_QUERIES}"><gridcraftDbQueries>[]</gridcraftDbQueries></doc>"#),
        // Our element name at the root, in another namespace.
        format!(r#"<?xml version="1.0"?><gridcraftDbQueries xmlns="urn:example" note="{NS_DB_QUERIES}">[]</gridcraftDbQueries>"#),
        format!(r#"<?xml version="1.0"?><g:gridcraftDbQueries xmlns:g="urn:example" xmlns:o="{NS_DB_QUERIES}">[]</g:gridcraftDbQueries>"#),
    ];
    for xml in foreign {
        let wb = keeping(workbook_with("Data", "Orders", "A1:A2", 7), "customXml/item1.xml", xml.as_bytes());
        let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(kept_data(&back, "customXml/item1.xml").as_deref(), Some(xml.as_bytes()), "{xml}");
        assert!(report.warnings.iter().all(|w| !w.contains("database queries")), "{:?}", report.warnings);
        let (again, _) = read_xlsx(&write_xlsx(&back).unwrap()).unwrap();
        assert_eq!(kept_data(&again, "customXml/item1.xml").as_deref(), Some(xml.as_bytes()), "byte for byte after a second save");
    }
}

#[test]
fn ours_with_a_prefix_is_read_and_ours_unreadable_is_kept_with_a_warning() {
    // Any prefix bound to our namespace is ours.
    let mut wb = workbook_with("Data", "Orders", "A1:A2", 7);
    wb.db_queries = vec![query(7)];
    let ours = Package::open(&write_xlsx(&wb).unwrap()).unwrap().read("customXml/gridcraftDbQueries.xml").unwrap().unwrap();
    let ours = String::from_utf8(ours).unwrap();
    let prefixed =
        ours.replace("<gridcraftDbQueries xmlns=", "<q:gridcraftDbQueries xmlns:q=").replace("</gridcraftDbQueries>", "</q:gridcraftDbQueries>");
    let (back, _) =
        read_xlsx(&write_xlsx(&keeping(workbook_with("Data", "Orders", "A1:A2", 7), "customXml/item1.xml", prefixed.as_bytes())).unwrap()).unwrap();
    assert_eq!(back.db_queries.len(), 1, "{prefixed}");
    assert!(kept_data(&back, "customXml/item1.xml").is_none(), "read into the model, not kept twice");
    // Ours, but its JSON is broken: kept as it is, with a warning that says so; a valid part
    // after it still loads.
    let broken = format!(r#"<?xml version="1.0"?><gridcraftDbQueries xmlns="{NS_DB_QUERIES}" version="1">[{{"sheet":</gridcraftDbQueries>"#);
    let mut wb = keeping(workbook_with("Data", "Orders", "A1:A2", 7), "customXml/item1.xml", broken.as_bytes());
    wb.db_queries = vec![query(7)];
    let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(kept_data(&back, "customXml/item1.xml").as_deref(), Some(broken.as_bytes()));
    assert!(report.warnings.iter().any(|w| w.contains("customXml/item1.xml") && w.contains("kept as it is")), "{:?}", report.warnings);
    assert_eq!(back.db_queries.len(), 1, "the valid part loads");
}

/// The size of the queries part written for `sql`, or the save error.
fn part_size(sql: &str) -> Result<usize, crate::IoError> {
    let mut wb = workbook_with("Data", "Orders", "A1:A2", 7);
    wb.db_queries = vec![DbQuery { query: sql.into(), ..query(7) }];
    let bytes = write_xlsx(&wb)?;
    Ok(Package::open(&bytes).unwrap().read("customXml/gridcraftDbQueries.xml").unwrap().unwrap().len())
}

#[test]
fn queries_up_to_the_readers_limit_are_written_and_none_beyond() {
    let max = MAX_PART as usize;
    // Everything but the SQL, measured on a short one.
    let overhead = part_size("x").unwrap() - 1;
    let at_limit = "x".repeat(max - overhead);
    assert_eq!(part_size(&at_limit).unwrap(), max);
    let mut wb = workbook_with("Data", "Orders", "A1:A2", 7);
    wb.db_queries = vec![DbQuery { query: at_limit, ..query(7) }];
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.db_queries.len(), 1, "a part at the limit opens again");
    let e = part_size(&"x".repeat(max - overhead + 1)).unwrap_err();
    assert!(e.to_string().contains("database queries"), "{e}");
    // `&` is one byte of JSON but five of XML: escaping alone takes it over the limit.
    let amps = "&".repeat(max / 4);
    assert!(serde_json::to_string(&amps).unwrap().len() < max);
    assert!(part_size(&amps).is_err());
}
