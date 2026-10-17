use std::io::Read;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const EXT: &str = "{6CCD98E4-6E9C-4F4A-9C93-B1DA490DC66F}";

#[test]
fn odbc_source_definition_survives_a_table_round_trip() {
    let table = format!(
        r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Query1" displayName="Query1" ref="A1:A2"><autoFilter ref="A1:A2"/><tableColumns count="1"><tableColumn id="1" name="Value"/></tableColumns><tableStyleInfo name="TableStyleMedium2"/><extLst><ext uri="{EXT}" xmlns:gc="urn:gridcraft:odbc:1"><gc:query dsn="Postgres" connectionRef="query-1" sql="SELECT 1" maxRows="100000" fingerprint=""/></ext></extLst></table>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Value</t></is></c></row><row r="2"><c r="A2"><v>1</v></c></row></sheetData><tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#,
        &[("xl/tables/table1.xml", &table), ("xl/worksheets/_rels/sheet1.xml.rels", rels)],
        "",
        "",
    );
    let (workbook, _) = read_xlsx(&bytes).unwrap();
    let saved = write_xlsx(&workbook).unwrap();
    let mut package = zip::ZipArchive::new(std::io::Cursor::new(&saved)).unwrap();
    let mut definition = String::new();
    package.by_name("xl/tables/table1.xml").unwrap().read_to_string(&mut definition).unwrap();
    assert!(definition.contains("gc:query"), "ODBC source was lost: {definition}");
    assert!(definition.contains("dsn=\"Postgres\""));
    assert!(!definition.contains("password"));
    let (again, _) = read_xlsx(&saved).unwrap();
    assert_eq!(again.sheets[0].tables[0], workbook.sheets[0].tables[0]);
}
