//! Excel's Power Query queries (the `DataMashup` part): listed read-only, kept byte for byte.
//! The packages, including the [MS-QDEFF] binary stream, are built here from the public spec.

use std::collections::HashMap;
use std::io::Read;

use super::{make_zip, minimal};
use crate::testing::{mashup_part, mashup_stream as stream};
use crate::{read_xlsx, write_xlsx};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const SECTION: &str = "section Section1;\r\n\r\nshared Sales = let\r\n    Source = Excel.CurrentWorkbook(){[Name=\"Raw\"]}[Content],\r\n    Typed = Table.TransformColumnTypes(Source, {{\"Amount\", type number}})\r\nin\r\n    Typed;\r\n\r\n[ Description = \"Sum per region\" ]\r\nshared #\"Region Totals\" = let\r\n    Grouped = Table.Group(Sales, {\"Region\"}, {{\"Total\", each List.Sum([Amount]), type number}})\r\nin\r\n    Grouped;\r\n";

const METADATA: &str = r#"<?xml version="1.0" encoding="utf-8"?><LocalPackageMetadataFile><Items><Item><ItemLocation><ItemType>AllFormulas</ItemType><ItemPath /></ItemLocation><StableEntries /></Item><Item><ItemLocation><ItemType>Formula</ItemType><ItemPath>Section1/Sales</ItemPath></ItemLocation><StableEntries><Entry Type="FillEnabled" Value="l1" /><Entry Type="FillTarget" Value="sSales" /><Entry Type="FillCount" Value="l2" /><Entry Type="FillLastUpdated" Value="d2026-10-01T08:30:00.0000000Z" /></StableEntries></Item><Item><ItemLocation><ItemType>Formula</ItemType><ItemPath>Section1/Region%20Totals</ItemPath></ItemLocation><StableEntries><Entry Type="FillEnabled" Value="l0" /><Entry Type="FillToDataModelEnabled" Value="l1" /></StableEntries></Item><Item><ItemLocation><ItemType>Formula</ItemType><ItemPath>Section1/Sales/Source</ItemPath></ItemLocation><StableEntries /></Item></Items></LocalPackageMetadataFile>"#;

fn connections() -> String {
    format!(
        r#"<?xml version="1.0"?><connections xmlns="{NS_MAIN}"><connection id="1" keepAlive="1" name="Query - Sales" type="5" refreshedVersion="8" background="1" refreshOnLoad="1" saveData="1"><dbPr connection="Provider=Microsoft.Mashup.OleDb.1;Data Source=$Workbook$;Location=Sales;Extended Properties=&quot;&quot;" command="SELECT * FROM [Sales]"/></connection><connection id="2" keepAlive="1" name="Query - Region Totals" type="5" refreshedVersion="8" background="1"><dbPr connection="Provider=Microsoft.Mashup.OleDb.1;Data Source=$Workbook$;Location=&quot;Region Totals&quot;;Extended Properties=&quot;&quot;" command="SELECT * FROM [Region Totals]"/></connection></connections>"#
    )
}

/// A workbook whose query `Sales` loads into table `Sales` on sheet Data (through a query table
/// and connection 1); `Region Totals` only goes to the Data Model.
fn package(mashup: &[u8]) -> Vec<u8> {
    let body = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Region</t></is></c><c r="B1" t="inlineStr"><is><t>Amount</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>North</t></is></c><c r="B2"><v>5</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>South</t></is></c><c r="B3"><v>7</v></c></row></sheetData><tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#;
    let sheet_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/table" Target="../tables/table1.xml"/></Relationships>"#
    );
    let table = format!(
        r#"<?xml version="1.0"?><table xmlns="{NS_MAIN}" id="1" name="Sales" displayName="Sales" ref="A1:B3" tableType="queryTable" totalsRowShown="0"><autoFilter ref="A1:B3"/><tableColumns count="2"><tableColumn id="1" uniqueName="1" name="Region" queryTableFieldId="1"/><tableColumn id="2" uniqueName="2" name="Amount" queryTableFieldId="2"/></tableColumns></table>"#
    );
    let table_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/queryTable" Target="../queryTables/queryTable1.xml"/></Relationships>"#
    );
    let query_table = format!(
        r#"<?xml version="1.0"?><queryTable xmlns="{NS_MAIN}" name="ExternalData_1" connectionId="1" autoFormatId="16" applyNumberFormats="0" applyBorderFormats="0" applyFontFormats="0" applyPatternFormats="0" applyAlignmentFormats="0" applyWidthHeightFormats="0"><queryTableRefresh nextId="3"><queryTableFields count="2"><queryTableField id="1" name="Region" tableColumnId="1"/><queryTableField id="2" name="Amount" tableColumnId="2"/></queryTableFields></queryTableRefresh></queryTable>"#
    );
    let wb_rels = format!(
        r#"<Relationship Id="rId8" Type="{REL}/customXml" Target="../customXml/item1.xml"/><Relationship Id="rId9" Type="{REL}/connections" Target="connections.xml"/>"#
    );
    let mut pkg = unzip(&minimal(
        body,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", &sheet_rels),
            ("xl/tables/table1.xml", &table),
            ("xl/tables/_rels/table1.xml.rels", &table_rels),
            ("xl/queryTables/queryTable1.xml", &query_table),
            ("xl/connections.xml", &connections()),
        ],
        &wb_rels,
        "",
    ));
    pkg.insert("customXml/item1.xml".into(), mashup.to_vec());
    let parts: Vec<(&str, &[u8])> = pkg.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect();
    make_zip(&parts)
}

fn unzip(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut v = vec![];
        f.read_to_end(&mut v).unwrap();
        out.insert(f.name().to_string(), v);
    }
    out
}

#[test]
fn excel_queries_are_listed_with_formula_and_load() {
    let (wb, report) = read_xlsx(&package(&mashup_part(&stream(SECTION, METADATA)))).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("Power Query")), "{:?}", report.warnings);
    let q = &wb.excel_queries;
    assert_eq!(q.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(), ["Sales", "Region Totals"]);
    assert!(q[0].formula.starts_with("let\r\n    Source = Excel.CurrentWorkbook()") && q[0].formula.ends_with("Typed"));
    assert_eq!(q[0].connection.as_deref(), Some("Query - Sales"));
    assert_eq!((q[0].table.as_deref(), q[0].sheet.as_deref()), (Some("Sales"), Some("Data")));
    assert_eq!((q[0].rows, q[0].refresh_on_open, q[0].load_kind()), (Some(2), true, "table"));
    assert_eq!(q[0].last_refreshed.as_deref(), Some("2026-10-01T08:30:00.0000000Z"));
    assert_eq!(q[1].description, "Sum per region");
    assert_eq!(q[1].connection.as_deref(), Some("Query - Region Totals"));
    assert_eq!((q[1].table.as_deref(), q[1].load_kind(), q[1].refresh_on_open), (None, "dataModel", false));
    // GridCraft's own (refreshable) queries are separate.
    assert!(wb.queries.is_empty());
}

#[test]
fn excel_query_parts_round_trip_byte_for_byte() {
    let mashup = mashup_part(&stream(SECTION, METADATA));
    let (wb, _) = read_xlsx(&package(&mashup)).unwrap();
    let out = write_xlsx(&wb).unwrap();
    let parts = unzip(&out);
    assert!(parts.values().any(|v| *v == mashup), "the DataMashup part is kept unchanged");
    assert_eq!(parts.get("xl/connections.xml").map(Vec::as_slice), Some(connections().as_bytes()));
    // Read back: the queries are still there (the table is found through the metadata).
    let (again, _) = read_xlsx(&out).unwrap();
    assert_eq!(again.excel_queries.len(), 2);
    assert_eq!(again.excel_queries[0].formula, wb.excel_queries[0].formula);
    assert_eq!(again.excel_queries[0].table.as_deref(), Some("Sales"));
}

#[test]
fn damaged_mashups_cost_only_the_listing() {
    let good = stream(SECTION, METADATA);
    let not_base64 = {
        let text = r#"<?xml version="1.0"?><DataMashup xmlns="http://schemas.microsoft.com/DataMashup">AAAA*not base64*</DataMashup>"#;
        text.as_bytes().to_vec()
    };
    let cases: Vec<Vec<u8>> = vec![
        not_base64,
        mashup_part(&[]),
        mashup_part(&[1, 0, 0, 0]),
        mashup_part(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x7F, 1, 2]),
        mashup_part(&[0, 0, 0, 0, 4, 0, 0, 0, b'P', b'K', 3, 4]),
        mashup_part(&good[..good.len() / 2]),
        mashup_part(&stream(SECTION, "<not xml")),
        mashup_part(&stream("section;;;shared ;#\"\"=;[", METADATA)),
    ];
    for (i, m) in cases.iter().enumerate() {
        let (wb, _) = read_xlsx(&package(m)).unwrap_or_else(|e| panic!("case {i}: {e}"));
        assert!(wb.sheets.len() == 1, "case {i}");
        let parts = unzip(&write_xlsx(&wb).unwrap());
        assert!(parts.values().any(|v| v == m), "case {i}: the part is kept unchanged");
    }
    // A damaged metadata document only loses the load facts.
    let (wb, _) = read_xlsx(&package(&mashup_part(&stream(SECTION, "<not xml")))).unwrap();
    assert_eq!(wb.excel_queries.len(), 2);
}

#[test]
fn mutated_mashups_never_panic() {
    let good = stream(SECTION, METADATA);
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..200 {
        let mut bin = good.clone();
        for _ in 0..1 + next() % 8 {
            let at = (next() % bin.len() as u64) as usize;
            bin[at] = next() as u8;
        }
        if next() % 4 == 0 {
            bin.truncate((next() % bin.len() as u64) as usize);
        }
        let _ = read_xlsx(&package(&mashup_part(&bin)));
    }
}
