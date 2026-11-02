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
    wb.preserved = Some(std::sync::Arc::new(foreign_connections_preserved()));
    let original = wb.preserved.as_ref().expect("preserved").parts[0].data.clone();
    let bytes = write_xlsx(&wb).expect("saves");
    let (back, _r) = read_xlsx(&bytes).expect("reopens");
    assert!(back.web_queries.is_empty(), "a foreign connection is not claimed by the model");
    // The saved workbook keeps the relationship pointing at the part (an orphaned part
    // would hold the same bytes and still be unreachable).
    assert!(
        back.preserved.as_ref().is_some_and(|p| p.rels.iter().any(|r| p.parts.iter().any(|q| q.name == "xl/connections.xml" && normalize_target(&r.target) == q.name))),
        "the connections relationship survives the save"
    );
    // Verbatim means byte for byte: the saved part is the file's own, not a rewrite.
    let mut pkg = package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the foreign part is kept");
    assert_eq!(&part[..], &original[..], "the foreign part is byte-identical");
}

/// The relationship target as a part name: the reader stores targets resolved against the
/// owner part's folder, so `xl/connections.xml` is the name to compare with.
fn normalize_target(target: &str) -> String {
    target.trim_start_matches('/').to_string()
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
            target: "xl/connections.xml".into(),
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
    assert!(text.contains("xmlns:xel="), "the root keeps its xel binding: {text}");
    assert!(text.contains("<xel:connection"), "the prefixed foreign connection is kept");
    // The stale GridCraft connection from the fixture: gone by name, and no extension
    // marker left (the GC_URI check also proves the fixture's marker matches the real
    // constant - a mismatched fixture marker would make `Old` look foreign and kept).
    assert!(!text.contains("name=\"Old\""), "the stale GridCraft connection is dropped");
    assert!(!text.contains(crate::write::GC_URI), "no GridCraft extension marker remains");
    assert!(!text.contains("https://example.com"), "the removed web query's URL is gone");
    // The part parses: the prefix is bound.
    assert!(crate::xml::parse(&part).is_ok(), "the merged part is well-formed XML");
}

#[test]
/// An attribute-less `<connection/>` (valid XML) is scanned, not skipped: a name-terminator
/// set of only whitespace would drop it from the merge.
fn an_attribute_less_connection_is_scanned() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection/><xel:connection/><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
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
    assert!(text.matches("<connection/>").count() == 1, "the attribute-less connection is kept verbatim");
    assert!(text.contains("<xel:connection/>"), "the attribute-less prefixed connection is kept verbatim");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
    assert_eq!(text.matches("<connection").count(), 3, "three connection elements total (the two foreign plus ours)");
}

#[test]
/// A CDATA section inside a foreign connection may hold a literal `</connection>`: the depth
/// scan skips the section whole, so the block ends at its real close and the merge still
/// applies (without CDATA awareness, the scan fails and stale GridCraft connections would
/// survive the save).
fn a_cdata_section_holding_a_closing_tag_does_not_end_the_block() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Data" type="5"><parameter><![CDATA[a </connection> b]]></parameter></connection><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
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
    // Our current connection replaces the file's stale one; the CDATA one is foreign and kept.
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("name=\"Data\""), "the CDATA-holding foreign connection is kept");
    assert!(text.contains("a </connection> b"), "the CDATA content is kept verbatim");
    assert!(!text.contains("name=\"Old\""), "the stale GridCraft connection is replaced (the merge ran)");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
}

#[test]
/// Adding a web query to a reopened workbook (whose connections part is foreign-owned and
/// kept) sticks: the merge writes ours into the kept part alongside the foreign one.
fn adding_a_web_query_after_reopen_survives_the_save() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Flags" type="4"/></connections>"#;
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
    // Round one: save and reopen (the part stays foreign-kept).
    let bytes = write_xlsx(&wb).expect("saves");
    let (mut reopened, _) = read_xlsx(&bytes).expect("reopens");
    assert!(reopened.web_queries.is_empty());
    // The user imports a table now.
    reopened.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: Some(5), url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: true });
    let bytes2 = write_xlsx(&reopened).expect("saves again");
    let (final_wb, _) = read_xlsx(&bytes2).expect("reopens again");
    let q = final_wb.web_queries.iter().find(|q| q.name == "prices").expect("the added connection survived the save");
    assert_eq!(q.table_id, Some(5));
    assert!(q.refresh_on_open);
    // And the foreign one is still not claimed.
    assert_eq!(final_wb.web_queries.len(), 1);
}

#[test]
/// A prefixed root with no default namespace (`<x:connections xmlns:x="...">`) keeps the
/// SpreadsheetML namespace for the model's unprefixed `<connection>` elements: the rebuilt
/// root emits the default when the original had none.
fn a_prefixed_root_without_a_default_namespace_gains_one() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><x:connections xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:connection id="7" name="PQ" type="4"/></x:connections>"#;
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
    assert!(text.contains("xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\""), "the default SpreadsheetML namespace is emitted: {text}");
    assert!(text.contains("<x:connection "), "the prefixed foreign connection is kept");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
}

#[test]
/// A connections relationship targeting a differently-named part (`xl/connections2.xml`)
/// is detected by its type, and the writer never adds a second connections part or
/// relationship: the merge lands in the file's own part.
fn a_connections_part_with_a_custom_name_is_merged_not_duplicated() {
    use gridcraft_model::preserved::*;
    let xml = br#"<?xml version="1.0"?><connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
    let mut wb = Workbook::default();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S")));
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("S2")));
    wb.preserved = Some(std::sync::Arc::new(Preserved {
        package_rels: vec![],
        rels: vec![PreservedRel { id: "rId9".into(), rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections".into(), target: "xl/connections2.xml".into(), external: false }],
        parts: vec![PreservedPart { name: "xl/connections2.xml".into(), content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(), data: xml.to_vec().into(), rels: vec![] }],
        elements: vec![],
        vml: None,
        sheets: vec![],
    }));
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections2.xml").expect("read").expect("the file's own part, kept under its own name");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("name=\"prices\""), "the merge landed in the custom-named part");
    assert!(!text.contains("name=\"Old\""), "the stale connection is replaced");
    // No second part at the conventional name.
    assert!(pkg.read("xl/connections.xml").ok().flatten().is_none(), "no duplicate connections part is added");
    // And only one connections relationship.
    let mut rels = pkg.read("xl/_rels/workbook.xml.rels").expect("rels").expect("the rels");
    let text = String::from_utf8_lossy(&rels).to_string();
    assert_eq!(text.matches("/connections\"").count(), 1, "exactly one connections relationship: {text}");
    let _ = &mut rels;
}

#[test]
/// A single-quoted default namespace (`xmlns='...'`) is recognized, not doubled: the
/// rebuilt root keeps the original declaration and adds no second xmlns.
fn a_single_quoted_default_namespace_is_not_doubled() {
    use gridcraft_model::preserved::*;
    let xml = br#"<connections xmlns='http://schemas.openxmlformats.org/spreadsheetml/2006/main'><connection id="1" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
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
    // Exactly one default xmlns (an `xmlns:gc` prefixed declaration is not one).
    assert_eq!(text.matches("xmlns='").count(), 1, "the original single-quoted default is kept: {text}");
    assert!(!text.contains("xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\""), "no second default was added: {text}");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
}

#[test]
/// A foreign root-level `xmlns:gc` binding (another add-in's own gc prefix) is preserved,
/// and GridCraft's generated elements declare their namespace inline: the foreign `gc:`
/// elements keep their original XML identity, and ours never hijack the root's prefix.
fn a_foreign_gc_prefix_binding_is_preserved() {
    use gridcraft_model::preserved::*;
    let xml = br#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:gc="http://foreign-addin.example/ns"><gc:connection id="4" name="Theirs" type="4"><gc:parameter/></gc:connection><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></connections>"#;
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
    assert!(text.contains("xmlns:gc=\"http://foreign-addin.example/ns\""), "the foreign gc binding is preserved on the root: {text}");
    assert!(text.contains("<gc:connection "), "the foreign gc-prefixed connection is kept");
    assert!(text.contains("xmlns:gc=\"https://gridcraft.app/ns/webquery\""), "GridCraft's namespace is declared inline on its own ext");
    assert_eq!(text.matches("http://foreign-addin.example/ns").count(), 1, "the foreign binding appears exactly once");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
    // The merged part reads back: our connection is ours, the foreign one not claimed.
    let (back, _) = crate::read_xlsx(&bytes).expect("reopens");
    assert!(back.web_queries.iter().any(|q| q.name == "prices"), "our connection reads back through the inline declaration");
}

#[test]
/// A prefixed root with a foreign default namespace keeps its prefix, and each generated
/// connection binds the SpreadsheetML namespace inline: the root and the generated
/// elements resolve to the namespaces the file meant, never inheriting the foreign default.
fn a_prefixed_root_with_a_foreign_default_keeps_its_prefix() {
    use gridcraft_model::preserved::*;
    let xml = br#"<x:connections xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns="urn:foreign"><x:connection id="4" name="Theirs" type="4"/></x:connections>"#;
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
    assert!(text.contains("<x:connections "), "the root keeps its prefix: {text}");
    assert!(text.contains("</x:connections>"), "the root's close keeps its prefix");
    assert!(text.contains("xmlns=\"urn:foreign\""), "the foreign default is kept");
    // Each generated connection binds the SpreadsheetML namespace inline.
    assert!(text.contains("<connection xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" "), "the generated connection binds SpreadsheetML inline");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
    // And the reopen reads our connection back.
    let (back, _) = crate::read_xlsx(&bytes).expect("reopens");
    assert!(back.web_queries.iter().any(|q| q.name == "prices"), "our connection reads back");
}

#[test]
/// Removing the last GridCraft connection from a prefixed-root, foreign-default part keeps
/// the root's identity too: the rewrite still honors the prefix and the default.
fn removing_the_last_connection_from_a_prefixed_foreign_default_root_keeps_it() {
    use gridcraft_model::preserved::*;
    let xml = br#"<x:connections xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns="urn:foreign"><x:connection id="4" name="Theirs" type="4"/><connection id="2" name="Old" type="5"><extLst><ext uri="{b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq}"><gc:webQuery/></ext></extLst></connection></x:connections>"#;
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
    // Save with a connection, reopen, remove it, save again: the rewrite must keep the
    // root's shape (this is the second case the finding names).
    wb.web_queries.push(gridcraft_model::WebQuery { name: "prices".into(), table_id: None, url: "https://example.com/t.html".into(), table: "prices".into(), occurrence: 0, sheet: "S".into(), last_synced: None, refresh_on_open: false });
    let bytes = write_xlsx(&wb).expect("saves");
    let (mut reopened, _) = read_xlsx(&bytes).expect("reopens");
    reopened.web_queries.clear();
    let bytes2 = write_xlsx(&reopened).expect("saves again");
    let mut pkg = crate::package::Package::open(&bytes2).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(text.contains("<x:connections "), "the root keeps its prefix after the removal: {text}");
    assert!(text.contains("</x:connections>"), "the close keeps its prefix");
    assert!(text.contains("name=\"Theirs\""), "the foreign connection is kept");
    assert!(!text.contains("name=\"Old\"") && !text.contains("name=\"prices\""), "both GridCraft connections are gone");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
}

#[test]
/// A markup-compatibility attribute (`mc:Ignorable`, whatever prefix binds the MC
/// namespace) survives the root rebuild: the foreign blocks may rely on it to tell an
/// importer which namespaces it may safely ignore.
fn a_markup_compatibility_attribute_survives_the_rebuild() {
    use gridcraft_model::preserved::*;
    let xml = br#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:x16="http://schemas.microsoft.com/office/spreadsheetml/2016/ext" mc:Ignorable="x16"><x16:connection id="4" name="Theirs" type="4"/></connections>"#;
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
    assert!(text.contains("mc:Ignorable=\"x16\""), "the markup-compatibility attribute is kept: {text}");
    assert!(text.contains("name=\"prices\""), "our connection is merged in");
    assert!(crate::xml::parse(&part).is_ok(), "the part is well-formed");
}

#[test]
/// A foreign-only connections part in an unsupported encoding (Latin-1 bytes, say) does
/// not fail an open-and-save: the model has nothing to merge into it (no GridCraft marker
/// in the bytes, no web queries in the model), so the part is kept exactly as it was.
fn a_non_utf8_foreign_only_part_does_not_fail_the_save() {
    use gridcraft_model::preserved::*;
    // Latin-1 text: not valid UTF-8, not BOM-marked, not UTF-16.
    let xml: &[u8] = b"<connections xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><connection id=\"1\" name=\"Caf\xe9\" type=\"4\"/></connections>";
    assert!(std::str::from_utf8(xml).is_err());
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
    // No web queries in the model: the save must succeed and keep the part byte for byte.
    let bytes = write_xlsx(&wb).expect("the save succeeds");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    assert_eq!(&part[..], &xml[..], "the foreign part in its own encoding is kept byte for byte");
}

#[test]
/// An entity-encoded GridCraft marker (`&#x7b;...` for the ext uri) is ours to the reader,
/// so the merge's ownership checks match it by parsed value: with the model emptied, the
/// stale entity-encoded connection is removed, not left behind by a raw-text check.
fn an_entity_encoded_gridcraft_connection_is_removed() {
    use gridcraft_model::preserved::*;
    // The marker, with `{` and `}` spelled as character references.
    let marker = "&#x7b;b7f3a1d2-6c5e-4a8b-9d0f-gridcraftwebq&#x7d;";
    let xml = format!(r#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><connection id="1" name="Old" type="5"><extLst><ext uri="{marker}"><gc:webQuery/></ext></extLst></connection></connections>"#).into_bytes();
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
    // The model holds nothing: the stale, entity-encoded connection must go.
    let bytes = write_xlsx(&wb).expect("saves");
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let part = pkg.read("xl/connections.xml").expect("read").expect("the part");
    let text = String::from_utf8_lossy(&part);
    assert!(!text.contains("name=\"Old\""), "the entity-encoded GridCraft connection is removed, not kept by the raw-text check: {text}");

    assert!(!text.contains("gridcraftwebq") && !text.contains("&#x7d;"), "no marker survives");
}

#[test]
/// Two tables sharing a model id (a copied sheet, say) never emit duplicate `table/@id`:
/// the second gets the next unused value, and the reader's reopened ids stay unique.
fn duplicate_table_ids_never_collide_in_the_package() {
    use gridcraft_model::*;
    let mut wb = Workbook::default();
    for sheet in ["A", "B"] {
        let sh = Sheet::new(sheet);
        let mut sh = sh;
        sh.tables.push(Table { id: 7, name: format!("T{sheet}"), range: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), header_row: true, totals_row: false, columns: vec![], style: String::new(), banded_rows: false, banded_cols: false, first_col: false, last_col: false, filter_button: false, filter: vec![], table_type: None, connection_id: None });
        wb.sheets.push(std::sync::Arc::new(sh));
    }
    let bytes = write_xlsx(&wb).expect("saves");
    // Every table part's id attribute is unique.
    let mut pkg = crate::package::Package::open(&bytes).expect("opens");
    let mut ids = vec![];
    for n in 1..=2 {
        let part = pkg.read(&format!("xl/tables/table{n}.xml")).expect("read").expect("the part");
        let text = String::from_utf8_lossy(&part);
        let id = text.split("id=\"").nth(1).and_then(|r| r.split('"').next()).expect("an id").to_string();
        ids.push(id);
    }
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "the duplicate model id never emits twice: {ids:?}");
    assert_eq!(ids[0], "7", "the first table keeps the model id");
    // And the reopen hands back unique ids.
    let (back, _) = read_xlsx(&bytes).expect("reopens");
    let mut seen: Vec<u32> = back.sheets.iter().flat_map(|sh| sh.tables.iter().map(|t| t.id)).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 2, "the reopened model ids are unique: {seen:?}");
}
