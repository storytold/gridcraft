use std::io::Read;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const EXT: &str = "{6CCD98E4-6E9C-4F4A-9C93-B1DA490DC66F}";
const QUERY: &str = r#"<gc:query dsn="Postgres" connectionRef="query-1" sql="SELECT 1" maxRows="100000" fingerprint=""/>"#;

fn fixture(query: &str) -> Vec<u8> {
    let table = format!(
        r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Query1" displayName="Query1" ref="A1:A2"><autoFilter ref="A1:A2"/><tableColumns count="1"><tableColumn id="1" name="Value"/></tableColumns><tableStyleInfo name="TableStyleMedium2"/><extLst><ext uri="{EXT}" xmlns:gc="urn:gridcraft:odbc:1">{query}</ext></extLst></table>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#;
    minimal(
        r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Value</t></is></c></row><row r="2"><c r="A2"><v>1</v></c></row></sheetData><tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#,
        &[("xl/tables/table1.xml", &table), ("xl/worksheets/_rels/sheet1.xml.rels", rels)],
        "",
        "",
    )
}

#[test]
fn odbc_source_definition_survives_a_table_round_trip() {
    let (workbook, report) = read_xlsx(&fixture(QUERY)).unwrap();
    assert!(!report.warnings.iter().any(|warning| warning.contains("ODBC")), "{:?}", report.warnings);
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

#[test]
fn rejected_odbc_definitions_warn_and_keep_cached_table_data() {
    for (case, query) in [
        ("missing query", String::new()),
        ("missing sql", QUERY.replace(r#" sql="SELECT 1""#, "")),
        ("oversized sql", QUERY.replace("SELECT 1", &"x".repeat(1_000_001))),
        ("oversized sql attribute", QUERY.replace("SELECT 1", &"x".repeat(1_048_577))),
        ("invalid connectionRef", QUERY.replace("query-1", "query;1")),
        ("oversized connectionRef", QUERY.replace("query-1", &"x".repeat(129))),
        ("unknown columnTypes", QUERY.replace("/>", r#" columnTypes="Integer,Unknown"/>"#)),
    ] {
        let (workbook, report) = read_xlsx(&fixture(&query)).unwrap();
        let sheet = &workbook.sheets[0];
        assert_eq!(sheet.tables.len(), 1, "{case}");
        assert!(sheet.tables[0].query.is_none(), "{case}");
        assert_eq!(sheet.cell(gridcraft_core::CellRef::new(1, 0)).unwrap().value, gridcraft_core::Value::Number(1.0), "{case}");
        assert!(
            report.warnings.iter().any(|warning| warning.contains("ODBC query definition") && warning.contains("ignored")),
            "{case}: {:?}",
            report.warnings
        );
    }
}

#[test]
fn cleared_or_deleted_query_tables_do_not_write_odbc_definitions() {
    for delete_table in [false, true] {
        let (mut workbook, _) = read_xlsx(&fixture(QUERY)).unwrap();
        let sheet = workbook.sheet_mut(0).unwrap();
        assert!(sheet.tables[0].query.is_some());
        if delete_table {
            sheet.tables.clear();
        } else {
            sheet.tables[0].query = None;
        }
        let saved = write_xlsx(&workbook).unwrap();
        let mut package = zip::ZipArchive::new(std::io::Cursor::new(&saved)).unwrap();
        for index in 0..package.len() {
            let mut part = package.by_index(index).unwrap();
            if part.name().ends_with(".xml") {
                let mut xml = String::new();
                part.read_to_string(&mut xml).unwrap();
                assert!(!xml.contains("gc:query"), "stale query in {}", part.name());
            }
        }
        let (again, _) = read_xlsx(&saved).unwrap();
        if delete_table {
            assert!(again.sheets[0].tables.is_empty());
        } else {
            assert_eq!(again.sheets[0].tables.len(), 1);
            assert!(again.sheets[0].tables[0].query.is_none());
        }
    }
}
