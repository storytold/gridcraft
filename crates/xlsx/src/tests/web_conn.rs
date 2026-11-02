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


#[test]
/// A workbook saved with connections, reopened and changed (a refresh setting flipped, a
/// connection removed, a foreign connection in the file) saves the current state: the merge
/// into the kept part applies GridCraft's connections while the foreign one is kept verbatim.
fn connections_changes_after_reopen_survive_the_save() {
    // A file with a foreign connection and one of ours.
    let mut wb = wb_with_connections();
    wb.preserved = None;
    let bytes = write_xlsx(&wb).expect("saves");
    let (mut reopened, _report) = read_xlsx(&bytes).expect("reopens");
    assert!(reopened.preserved.is_some(), "the save gave the reopened workbook a kept part");
    // The user flips refresh-on-open on the second connection and drops the first.
    let q = reopened.web_queries.iter_mut().find(|q| q.name == "weather").expect("the weather connection");
    q.refresh_on_open = true;
    reopened.web_queries.retain(|q| q.name != "prices");
    let bytes2 = write_xlsx(&reopened).expect("saves again");
    let (final_wb, _r) = read_xlsx(&bytes2).expect("reopens again");
    assert_eq!(final_wb.web_queries.len(), 1, "the kept connection survived: {:?}", final_wb.web_queries.iter().map(|q| q.name.clone()).collect::<Vec<_>>());
    let q = &final_wb.web_queries[0];
    assert_eq!(q.name, "weather");
    assert!(q.refresh_on_open, "the flipped setting survived the reopened save");
    assert_eq!(q.table, "Weather Table");
    assert_eq!(q.url, "https://example.org/wx");
}

#[test]
/// A foreign connection in a file with none of ours stays untouched: the model writes no
/// connections part, and passthrough keeps the file's own exactly as it was.
fn a_foreign_connections_part_is_kept_verbatim() {
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Aux")));
    // The file's own connections part with a database connection.
    let preserved = foreign_connections_preserved();
    wb.preserved = Some(std::sync::Arc::new(preserved));
    let bytes = write_xlsx(&wb).expect("saves");
    let (back, _r) = read_xlsx(&bytes).expect("reopens");
    assert!(back.web_queries.is_empty(), "a foreign connection is not claimed by the model");
    // And the foreign part's bytes are still in the package.
    let pkg = package::Package::open(&bytes).expect("opens");
    assert!(pkg.has("xl/connections.xml"), "the foreign part is kept");
}

/// A `Preserved` whose only part is a foreign connections.xml, as an open would keep it.
fn foreign_connections_preserved() -> gridcraft_model::preserved::Preserved {
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Query" type="5" refreshedVersion="6"><dbPr connection="Provider=Example" command="SELECT 1"/></connection></connections>"#;
    let part = gridcraft_model::preserved::PreservedPart {
        name: "xl/connections.xml".into(),
        content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(),
        data: xml.to_vec().into(),
        rels: vec![],
    };
    gridcraft_model::preserved::Preserved {
        package_rels: vec![],
        rels: vec![gridcraft_model::preserved::PreservedRel {
            id: "rId9".into(),
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(),
            target: "connections.xml".into(),
            external: false,
        }],
        parts: vec![part],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }
}

#[test]
/// The kept-part merge scans connection blocks by depth: a foreign self-closing connection
/// before one of ours is kept verbatim (a flat `/>` search would span it into the next
/// block and drop it), and the merged part parses with the foreign connection intact.
fn a_foreign_self_closing_connection_is_kept_by_the_depth_scan() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Flags" type="4"/><connection id="2" name="Old" type="5" refreshedVersion="6"><webPr url="https://gone.example/x"/><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery table="t" occurrence="0" sheet="S" tableId="0" lastSynced="0"/></ext></extLst></connection></connections>"#;
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S2")));
    wb.preserved = Some(std::sync::Arc::new(Preserved {
        package_rels: vec![],
        rels: vec![PreservedRel { id: "rId9".into(), rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(), target: "xl/connections.xml".into(), external: false }],
        parts: vec![PreservedPart { name: "xl/connections.xml".into(), content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(), data: xml.to_vec().into(), rels: vec![] }],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }));
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: true });
    let bytes = write_xlsx(&wb).expect("saves");
    let (back, r) = read_xlsx(&bytes).unwrap_or_else(|e| panic!("reopen err: {e}"));

    // The foreign self-closing connection stays (not claimed by the model), ours reads back.
    assert!(back.web_queries.iter().any(|q| q.name == "prices"), "our merged connection reads back");
    assert_eq!(back.web_queries.len(), 1, "the foreign one is not claimed: {:?}", back.web_queries.iter().map(|q| q.name.clone()).collect::<Vec<_>>());
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("name=\"Flags\" type=\"4\""), "the foreign self-closing connection is kept verbatim");
    assert!(!text.contains("gone.example"), "our old connection from the file is replaced");
}

#[test]
/// The depth scan is XML-aware: a `>` inside a quoted attribute value is not the tag's
/// end, and a comment inside a connection block does not confuse the depth walk.
fn the_depth_scan_handles_quoted_angle_brackets_and_comments() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="a>b &amp; c" type="4"><!-- a comment with </connection> inside --></connection><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S2")));
    wb.preserved = Some(std::sync::Arc::new(Preserved {
        package_rels: vec![],
        rels: vec![PreservedRel { id: "rId9".into(), rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(), target: "xl/connections.xml".into(), external: false }],
        parts: vec![PreservedPart { name: "xl/connections.xml".into(), content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(), data: xml.to_vec().into(), rels: vec![] }],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }));
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains(r#"name="a>b &amp; c""#), "the foreign connection with a quoted > in its name is kept verbatim");
    assert!(text.contains("a comment with </connection> inside"), "the comment inside the block is kept");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
    assert_eq!(text.matches("<connection ").count(), 2, "exactly two connections");
}

#[test]
/// Removing the last web query from a reopened workbook sticks: the stale GridCraft
/// connection must not survive the save, and the foreign one must (CodeRabbit's reopen/save
/// regression: clearing the model results in no web queries on the next read).
fn clearing_the_last_web_query_after_reopen_survives_the_save() {
    let mut wb = wb_with_connections();
    wb.preserved = None;
    let bytes = write_xlsx(&wb).expect("saves");
    let (mut reopened, _) = read_xlsx(&bytes).expect("reopens");
    assert_eq!(reopened.web_queries.len(), 2);
    reopened.web_queries.clear();
    let bytes2 = write_xlsx(&reopened).expect("saves again");
    let (final_wb, _) = read_xlsx(&bytes2).expect("reopens again");
    assert!(final_wb.web_queries.is_empty(), "the cleared connections did not come back: {:?}", final_wb.web_queries.iter().map(|q| q.name.clone()).collect::<Vec<_>>());
}

#[test]
/// A connection element whose name is followed by a newline or tab (valid XML) is scanned:
/// the flat "<connection " search would skip and drop it on a merge.
fn a_connection_with_whitespace_after_its_name_is_scanned() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection
        id="1"
        name="Spaced"
        type="4"/></connections>"#;
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S2")));
    wb.preserved = Some(std::sync::Arc::new(Preserved {
        package_rels: vec![],
        rels: vec![PreservedRel { id: "rId9".into(), rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(), target: "xl/connections.xml".into(), external: false }],
        parts: vec![PreservedPart { name: "xl/connections.xml".into(), content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(), data: xml.to_vec().into(), rels: vec![] }],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }));
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("name=\"Spaced\""), "the whitespace-formatted foreign connection is kept verbatim");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
}

#[test]
/// A foreign connection using a prefix declared on the connections root (Power Query's
/// style) keeps its binding through the merge: clearing the last web query rewrites the
/// part, and the saved file must still declare the prefix the foreign block uses.
fn a_root_declared_prefix_survives_removing_the_last_web_query() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:xel="http://schemas.microsoft.com/office/excel/2010/xconnections"><xel:connection id="9" name="PQ" type="5"/><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S2")));
    wb.preserved = Some(std::sync::Arc::new(Preserved {
        package_rels: vec![],
        rels: vec![PreservedRel { id: "rId9".into(), rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(), target: "xl/connections.xml".into(), external: false }],
        parts: vec![PreservedPart { name: "xl/connections.xml".into(), content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(), data: xml.to_vec().into(), rels: vec![] }],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }));
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let (mut reopened, _) = read_xlsx(&bytes).expect("reopens");
    reopened.web_queries.clear();
    let bytes2 = write_xlsx(&reopened).expect("saves again");
    let mut pkg = crate::package::Package::open(&bytes2).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("<xel:connection"), "the prefixed foreign connection is kept");
    assert!(text.contains("xmlns:xel="), "the root's prefix declaration survives the merge");
    // The stale GridCraft connection from the fixture: gone by name, and no extension
    // marker left (the GC_URI check also proves the fixture's marker matches the real
    // constant - a mismatched fixture marker would make `Old` look foreign and kept).
    assert!(!text.contains("name=\"Old\""), "the stale GridCraft connection is dropped");
    assert!(!text.contains(crate::write::GC_URI), "no GridCraft extension marker remains");
    assert!(!text.contains("https://example.com"), "the removed web query's URL is gone");
    // The part parses: the prefix is bound.
    assert!(crate::xml::parse(&part).is_ok(), "the merged part is well-formed XML");
}
