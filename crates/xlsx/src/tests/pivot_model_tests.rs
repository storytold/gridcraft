//! Data-model PivotTables: write → read round trips, the package parts Excel needs, attach-and-save
//! persistence, and hostile or absent pivot parts.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Sheet, Workbook};

use super::minimal;
use crate::{
    IoError, ModelPivotColumn, ModelPivotDataType, ModelPivotFieldRef, ModelPivotFilter, ModelPivotLayout, ModelPivotMeasure, ModelPivotRelationship,
    ModelPivotScalar, ModelPivotSpec, ModelPivotTable, ModelPivotValue, attach_model_pivot, read_model_pivots, read_xlsx, write_model_pivot,
    write_xlsx,
};
use gridcraft_model::preserved::{Preserved, PreservedPart};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn column(name: &str, caption: &str, data_type: ModelPivotDataType) -> ModelPivotColumn {
    ModelPivotColumn { name: name.into(), caption: caption.into(), data_type }
}

fn field(table: &str, column: &str) -> ModelPivotFieldRef {
    ModelPivotFieldRef { table: table.into(), column: column.into() }
}

/// The model of the fixture: sales rows joined to products and customers.
fn tables() -> Vec<ModelPivotTable> {
    use ModelPivotDataType::*;
    vec![
        ModelPivotTable {
            name: "Sales".into(),
            connection: "WorksheetConnection_book.xlsx!Sales".into(),
            columns: vec![
                column("Region", "Region", Text),
                column("Amount", "Amount", Number),
                column("Date", "Date", Date),
                column("Product", "Product", Text),
                column("Customer", "Customer", Text),
            ],
        },
        ModelPivotTable {
            name: "Products".into(),
            connection: "WorksheetConnection_book.xlsx!Products".into(),
            columns: vec![column("Product", "Product", Text), column("Category", "Category", Text)],
        },
        ModelPivotTable {
            name: "Customers".into(),
            connection: "WorksheetConnection_book.xlsx!Customers".into(),
            columns: vec![column("Customer", "Customer", Text), column("Segment", "Segment", Text)],
        },
    ]
}

fn relationships() -> Vec<ModelPivotRelationship> {
    vec![
        ModelPivotRelationship {
            from_table: "Sales".into(),
            from_column: "Product".into(),
            to_table: "Products".into(),
            to_column: "Product".into(),
        },
        ModelPivotRelationship {
            from_table: "Sales".into(),
            from_column: "Customer".into(),
            to_table: "Customers".into(),
            to_column: "Customer".into(),
        },
    ]
}

fn spec() -> ModelPivotSpec {
    ModelPivotSpec {
        name: "ModelPivot".into(),
        cache_id: 1,
        connection_id: 1,
        tables: tables(),
        relationships: relationships(),
        measures: vec![ModelPivotMeasure {
            table: "Sales".into(),
            name: "Total Amount".into(),
            formula: "SUM(Sales[Amount])".into(),
            number_format: None,
        }],
        row_fields: vec![field("Sales", "Region")],
        row_members: vec![vec![
            ModelPivotScalar::Text("East".into()),
            ModelPivotScalar::Text("North".into()),
            ModelPivotScalar::Text("South".into()),
            ModelPivotScalar::Text("West".into()),
        ]],
        column_fields: vec![field("Products", "Category")],
        column_members: vec![vec![ModelPivotScalar::Text("Hardware".into()), ModelPivotScalar::Text("Software".into())]],
        filters: vec![ModelPivotFilter {
            field: field("Customers", "Segment"),
            allowed: vec![ModelPivotScalar::Text("Consumer".into()), ModelPivotScalar::Text("SMB".into())],
            include_blank: false,
        }],
        values: vec![ModelPivotValue { measure: "Total Amount".into(), caption: "Amount".into(), number_format: None }],
        layout: ModelPivotLayout::Tabular,
        row_subtotals: true,
        column_subtotals: false,
        subtotal_top: false,
        grand_totals: true,
        style: Some("PivotStyleMedium9".into()),
    }
}

/// The evaluated grid the engine hands the writer.
fn report() -> Vec<Vec<Value>> {
    vec![
        vec![Value::text("Segment"), Value::text("Consumer")],
        vec![Value::text("Region"), Value::text("Hardware"), Value::text("Software"), Value::text("Grand Total")],
        vec![Value::text("East"), Value::number(10.0), Value::number(20.0), Value::number(30.0)],
        vec![Value::text("West"), Value::number(40.0), Value::number(50.0), Value::number(90.0)],
        vec![Value::text("Grand Total"), Value::number(50.0), Value::number(70.0), Value::number(120.0)],
    ]
}

fn workbook() -> Workbook {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().name = "Data".into();
    wb.sheets.push(Arc::new(Sheet::new("Pivot")));
    wb
}

fn written() -> Vec<u8> {
    write_model_pivot(&mut workbook(), "Pivot", at("A3"), &spec(), &report(), None).unwrap()
}

fn entries(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut m = HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        m.insert(f.name().to_string(), b);
    }
    m
}

fn text(parts: &HashMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts.get(name).unwrap_or_else(|| panic!("missing part {name}")).clone()).unwrap()
}

/// The target of the workbook relationship `rid`.
fn wb_rel_target(parts: &HashMap<String, Vec<u8>>, rid: &str) -> Option<String> {
    let rels = text(parts, "xl/_rels/workbook.xml.rels");
    let at = rels.find(&format!("Id=\"{rid}\""))?;
    let tag = rels.get(at..)?.split('>').next()?;
    let target = tag.split("Target=\"").nth(1)?.split('"').next()?;
    Some(target.to_string())
}

#[allow(clippy::cognitive_complexity)] // One round trip asserts every setting the writer stores.
#[test]
fn round_trip_keeps_every_representable_setting() {
    let bytes = written();
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1, "warnings: {:?}", report.warnings);
    let read = &pivots[0].spec;
    assert_eq!(pivots[0].sheet, "Pivot");
    assert_eq!(pivots[0].location, at("A3"));
    assert_eq!(read.name, "ModelPivot");
    assert_eq!(read.cache_id, 1);
    assert_eq!(read.connection_id, 1);

    // Axes.
    assert_eq!(read.row_fields, vec![field("Sales", "Region")]);
    assert_eq!(read.column_fields, vec![field("Products", "Category")]);
    let names = |list: &[ModelPivotScalar]| list.iter().map(|v| v.label()).collect::<Vec<_>>();
    assert_eq!(names(&read.row_members[0]), vec!["East", "North", "South", "West"]);
    assert_eq!(names(&read.column_members[0]), vec!["Hardware", "Software"]);

    // Filters: a multi-value page filter keeps both members.
    assert_eq!(read.filters.len(), 1);
    assert_eq!(read.filters[0].field, field("Customers", "Segment"));
    assert_eq!(read.filters[0].allowed, vec![ModelPivotScalar::Text("Consumer".into()), ModelPivotScalar::Text("SMB".into())]);
    assert!(!read.filters[0].include_blank);

    // Layout and totals.
    assert_eq!(read.layout, ModelPivotLayout::Tabular);
    assert!(read.row_subtotals);
    assert!(!read.column_subtotals);
    assert!(!read.subtotal_top);
    assert!(read.grand_totals);
    assert_eq!(read.style.as_deref(), Some("PivotStyleMedium9"));

    // Values: the measure name and its caption survive; the DAX formula lives in the model.
    assert_eq!(read.values.len(), 1);
    assert_eq!(read.values[0].measure, "Total Amount");
    assert_eq!(read.values[0].caption, "Amount");
    assert_eq!(read.measures.len(), 1);
    assert_eq!(read.measures[0].name, "Total Amount");
    assert_eq!(read.measures[0].table, "Sales");
    assert_eq!(read.measures[0].formula, "");
    assert!(report.warnings.iter().any(|w| w.contains("measure formulas were not read")), "{:?}", report.warnings);

    // Tables, columns and relationships come back from the cache hierarchies and the model ext.
    assert_eq!(read.relationships, relationships());
    let by_name = |name: &str| read.tables.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no table {name}"));
    assert_eq!(by_name("Sales").connection, "WorksheetConnection_book.xlsx!Sales");
    assert_eq!(
        by_name("Sales").columns,
        vec![
            column("Region", "Region", ModelPivotDataType::Text),
            column("Amount", "Amount", ModelPivotDataType::Number),
            column("Date", "Date", ModelPivotDataType::Date),
            column("Product", "Product", ModelPivotDataType::Text),
            column("Customer", "Customer", ModelPivotDataType::Text),
        ]
    );
    assert_eq!(by_name("Products").columns.len(), 2);
    assert_eq!(by_name("Customers").columns.len(), 2);
}

#[test]
fn report_cells_are_written_to_the_sheet() {
    let bytes = written();
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let sheet = wb.sheet_index("Pivot").and_then(|i| wb.sheet(i)).unwrap();
    assert_eq!(sheet.value(at("A3")), Value::text("Segment"));
    assert_eq!(sheet.value(at("B3")), Value::text("Consumer"));
    assert_eq!(sheet.value(at("A4")), Value::text("Region"));
    assert_eq!(sheet.value(at("A5")), Value::text("East"));
    assert_eq!(sheet.value(at("B5")), Value::number(10.0));
    assert_eq!(sheet.value(at("D7")), Value::number(120.0));
    // Nothing outside the report.
    assert_eq!(sheet.value(at("A8")), Value::default());
}

#[test]
fn package_has_every_part_excel_needs() {
    let parts = entries(&written());
    for name in [
        "[Content_Types].xml",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/worksheets/_rels/sheet2.xml.rels",
        "xl/pivotCache/pivotCacheDefinition1.xml",
        "xl/pivotTables/pivotTable1.xml",
        "xl/pivotTables/_rels/pivotTable1.xml.rels",
        "xl/connections.xml",
    ] {
        assert!(parts.contains_key(name), "missing part {name}; have {:?}", parts.keys().collect::<Vec<_>>());
    }
    // No model database was given: none invented.
    assert!(!parts.contains_key("xl/model/item.data"));
    assert!(!text(&parts, "[Content_Types].xml").contains("Extension=\"data\""));

    let ct = text(&parts, "[Content_Types].xml");
    assert!(ct.contains("PartName=\"/xl/pivotCache/pivotCacheDefinition1.xml\""));
    assert!(ct.contains("PartName=\"/xl/pivotTables/pivotTable1.xml\""));
    assert!(ct.contains("PartName=\"/xl/connections.xml\""));

    // The workbook points its <pivotCaches> at the cache, and the sheet at its PivotTable.
    let wb = text(&parts, "xl/workbook.xml");
    let caches_at = wb.find("<pivotCaches>").expect("no <pivotCaches>");
    assert!(caches_at > wb.find("<calcPr").expect("no <calcPr>"), "pivotCaches must follow calcPr");
    assert!(caches_at < wb.find("<extLst>").expect("no <extLst>"));
    assert_eq!(wb.matches("<pivotCaches").count(), 1, "the caches must not nest: {wb}");
    assert!(wb.contains("<pivotCache cacheId=\"1\" r:id=\"rId"), "workbook XML: {wb}");
    assert!(wb.contains("<x15:dataModel>"));
    assert!(wb.contains("<x15:modelTable id=\"Sales_"));
    assert!(wb.contains("<x15:modelRelationship fromTable=\"Sales\" fromColumn=\"Product\""));
    let rid = wb.split("<pivotCache cacheId=\"1\" r:id=\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(wb_rel_target(&parts, rid).as_deref(), Some("/xl/pivotCache/pivotCacheDefinition1.xml"));

    let sheet_rels = text(&parts, "xl/worksheets/_rels/sheet2.xml.rels");
    assert!(sheet_rels.contains("relationships/pivotTable"));
    assert!(sheet_rels.contains("Target=\"/xl/pivotTables/pivotTable1.xml\""));

    let table_rels = text(&parts, "xl/pivotTables/_rels/pivotTable1.xml.rels");
    assert!(table_rels.contains("relationships/pivotCacheDefinition"));
    assert!(table_rels.contains("Target=\"/xl/pivotCache/pivotCacheDefinition1.xml\""));

    let cache = text(&parts, "xl/pivotCache/pivotCacheDefinition1.xml");
    assert!(cache.contains("<cacheSource type=\"external\" connectionId=\"1\"/>"));
    let connections = text(&parts, "xl/connections.xml");
    assert!(connections.contains("name=\"ThisWorkbookDataModel\""));
    assert!(connections.contains("id=\"1\""));
}

#[test]
fn model_database_is_placed_and_declared_by_extension() {
    let mut wb = workbook();
    let bytes = write_model_pivot(&mut wb, "Pivot", at("A1"), &spec(), &report(), Some(b"model bytes")).unwrap();
    let parts = entries(&bytes);
    assert_eq!(parts.get("xl/model/item.data").map(Vec::as_slice), Some(&b"model bytes"[..]));
    let ct = text(&parts, "[Content_Types].xml");
    assert!(ct.contains("<Default Extension=\"data\" ContentType=\"application/vnd.openxmlformats-officedocument.model+data\"/>"));
    let wb_rels = text(&parts, "xl/_rels/workbook.xml.rels");
    assert!(wb_rels.contains("relationships/powerPivotData"));
    assert!(wb_rels.contains("Target=\"/xl/model/item.data\""));
}

#[test]
fn attach_survives_a_plain_save_and_a_reopen() {
    let mut wb = workbook();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    // The normal save path, called twice: attaching must not depend on write_model_pivot.
    let first = write_xlsx(&wb).unwrap();
    let second = write_xlsx(&wb).unwrap();
    for bytes in [first, second] {
        let parts = entries(&bytes);
        assert!(parts.contains_key("xl/pivotCache/pivotCacheDefinition1.xml"));
        assert!(parts.contains_key("xl/pivotTables/pivotTable1.xml"));
        assert!(text(&parts, "xl/workbook.xml").contains("<pivotCache cacheId=\"1\""));
        let (pivots, warnings) = read_model_pivots(&bytes).unwrap();
        assert_eq!(pivots.len(), 1, "{:?}", warnings.warnings);
        assert_eq!(pivots[0].spec.name, "ModelPivot");
        assert_eq!(pivots[0].location, at("A3"));
    }
}

#[test]
fn two_pivots_share_one_pivot_caches_element() {
    let mut wb = workbook();
    let mut second = spec();
    second.name = "ModelPivot2".into();
    second.cache_id = 2;
    second.connection_id = 2;
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    attach_model_pivot(&mut wb, "Pivot", at("A12"), &second, &report(), None).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    let wb_xml = text(&parts, "xl/workbook.xml");
    assert_eq!(wb_xml.matches("<pivotCaches").count(), 1, "the caches must not nest: {wb_xml}");
    assert!(wb_xml.contains("cacheId=\"1\""));
    assert!(wb_xml.contains("cacheId=\"2\""));
    // One <extLst> holding one data-model declaration.
    assert_eq!(wb_xml.matches("<x15:dataModel>").count(), 1);
    for name in [
        "xl/pivotCache/pivotCacheDefinition1.xml",
        "xl/pivotCache/pivotCacheDefinition2.xml",
        "xl/pivotTables/pivotTable1.xml",
        "xl/pivotTables/pivotTable2.xml",
    ] {
        assert!(parts.contains_key(name), "missing {name}");
    }
    assert!(text(&parts, "xl/connections.xml").contains("name=\"ThisWorkbookDataModel\""));
    let (pivots, _) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 2);
}

#[test]
fn an_existing_connections_part_is_reused_and_patched() {
    let mut wb = workbook();
    let kept = "<connections xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><connection id=\"4\" name=\"Query - Sales\" type=\"1\"/></connections>";
    wb.preserved = Some(Arc::new(gridcraft_model::Preserved {
        parts: vec![gridcraft_model::PreservedPart {
            name: "xl/connections.xml".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(),
            data: Arc::from(kept.as_bytes().to_vec()),
            rels: vec![],
        }],
        ..Default::default()
    }));
    let mut spec = spec();
    spec.connection_id = 7;
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    let connections = text(&parts, "xl/connections.xml");
    assert!(connections.contains("Query - Sales"), "the existing connection was dropped");
    assert!(connections.contains("id=\"7\""), "{connections}");
    assert!(connections.contains("name=\"ThisWorkbookDataModel\""), "{connections}");
    // The cache points at the connection the file really declares.
    assert!(text(&parts, "xl/pivotCache/pivotCacheDefinition1.xml").contains("connectionId=\"7\""));
    // And the file still opens with a single connections part.
    assert_eq!(parts.keys().filter(|n| n.eq_ignore_ascii_case("xl/connections.xml")).count(), 1);
}

#[test]
fn an_existing_model_connection_id_wins() {
    let mut wb = workbook();
    let kept = "<connections xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><connection id=\"1\" keepAlive=\"1\" name=\"ThisWorkbookDataModel\" type=\"5\"/></connections>";
    wb.preserved = Some(Arc::new(gridcraft_model::Preserved {
        parts: vec![gridcraft_model::PreservedPart {
            name: "xl/connections.xml".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(),
            data: Arc::from(kept.as_bytes().to_vec()),
            rels: vec![],
        }],
        ..Default::default()
    }));
    let mut spec = spec();
    spec.connection_id = 3;
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let parts = entries(&write_xlsx(&wb).unwrap());
    assert!(text(&parts, "xl/pivotCache/pivotCacheDefinition1.xml").contains("connectionId=\"1\""));
}

#[test]
fn empty_workbooks_read_as_no_model_pivots() {
    let bytes = write_xlsx(&Workbook::new()).unwrap();
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert!(pivots.is_empty());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_range_pivot_cache_is_skipped_with_a_warning() {
    let cache = r#"<?xml version="1.0"?><pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="worksheet"><worksheetSource ref="A1:B2" sheet="Data"/></cacheSource><cacheFields count="0"/></pivotCacheDefinition>"#;
    let table = r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"><location ref="A3:B4" firstHeaderRow="0" firstDataRow="1" firstDataCol="0"/></pivotTableDefinition>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotCache/pivotCacheDefinition1.xml", cache),
        (
            "xl/pivotTables/_rels/pivotTable1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#,
        ),
    ];
    let bytes = minimal("", &parts, "", "");
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert!(pivots.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("worksheet PivotTable cache")), "{:?}", report.warnings);
}

#[test]
fn malformed_pivot_parts_warn_instead_of_panicking() {
    let sheet_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#;
    let table_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#;
    let cache = r#"<?xml version="1.0"?><pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="external" connectionId="1"/><cacheFields count="0"/></pivotCacheDefinition>"#;
    for (name, table) in [
        ("not a pivot table", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#),
        (
            "no location",
            r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"/>"#,
        ),
        (
            "bad location",
            r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"><location ref="not a range"/></pivotTableDefinition>"#,
        ),
    ] {
        let parts = [
            ("xl/pivotTables/pivotTable1.xml", table),
            ("xl/pivotCache/pivotCacheDefinition1.xml", cache),
            ("xl/pivotTables/_rels/pivotTable1.xml.rels", table_rels),
            ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels),
        ];
        let bytes = minimal("", &parts, "", "");
        let (pivots, report) = read_model_pivots(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(pivots.is_empty(), "{name}: {:?}", pivots);
        assert!(!report.warnings.is_empty(), "{name}: no warning");
    }

    // A field index that no cache field has is dropped, not trusted.
    let huge = r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"><location ref="A3"/><pivotFields count="1"><pivotField axis="axisRow"/></pivotFields><rowFields count="1"><field x="4294967295"/></rowFields><pivotFields><pivotField axis="axisCol"/></pivotFields><colFields count="1"><field x="-2"/></colFields><dataFields count="1"><dataField fld="4294967295"/></dataFields></pivotTableDefinition>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", huge),
        ("xl/pivotCache/pivotCacheDefinition1.xml", cache),
        ("xl/pivotTables/_rels/pivotTable1.xml.rels", table_rels),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels),
    ];
    let bytes = minimal("", &parts, "", "");
    let (pivots, _) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1);
    assert!(pivots[0].spec.row_fields.is_empty());
    assert!(pivots[0].spec.column_fields.is_empty());
    assert!(pivots[0].spec.values.is_empty());

    // A cache part that is not XML at all is a hard error, never a panic.
    let table = r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"><location ref="A3"/></pivotTableDefinition>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotCache/pivotCacheDefinition1.xml", "<not xml"),
        ("xl/pivotTables/_rels/pivotTable1.xml.rels", table_rels),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels),
    ];
    let bytes = minimal("", &parts, "", "");
    let err = read_model_pivots(&bytes).expect_err("invalid XML must fail");
    assert!(matches!(err, IoError::Xml(_)), "{err:?}");
}

#[test]
fn deeply_nested_pivot_xml_is_refused() {
    let mut cache = String::from(r#"<?xml version="1.0"?><pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#);
    for _ in 0..600 {
        cache.push_str("<a>");
    }
    cache.push_str("</pivotCacheDefinition>");
    let sheet_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#;
    let table_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#;
    let table = r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="P" cacheId="1"><location ref="A3"/></pivotTableDefinition>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotCache/pivotCacheDefinition1.xml", &cache),
        ("xl/pivotTables/_rels/pivotTable1.xml.rels", table_rels),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels),
    ];
    let bytes = minimal("", &parts, "", "");
    assert!(matches!(read_model_pivots(&bytes), Err(IoError::Xml(_))));
}

#[allow(clippy::type_complexity)] // Each case carries the mutation that makes the spec invalid.
#[test]
fn invalid_specs_are_refused_before_anything_is_written() {
    let mutations: Vec<(&str, Box<dyn Fn(&mut ModelPivotSpec)>)> = vec![
        ("unknown column", Box::new(|s| s.row_fields.push(field("Sales", "Nope")))),
        ("twice on an axis", Box::new(|s| s.column_fields.push(field("Sales", "Region")))),
        (
            "not a declared measure",
            Box::new(|s| s.values.push(ModelPivotValue { measure: "Nope".into(), caption: String::new(), number_format: None })),
        ),
        ("no name", Box::new(|s| s.name = "  ".into())),
        ("filter on an axis", Box::new(|s| s.filters[0].field = field("Sales", "Region"))),
        ("filter keeps nothing", Box::new(|s| s.filters[0].allowed.clear())),
    ];
    for (why, change) in mutations {
        let mut wb = workbook();
        let mut spec = spec();
        change(&mut spec);
        let err = write_model_pivot(&mut wb, "Pivot", at("A1"), &spec, &report(), None).expect_err(why);
        assert!(matches!(err, IoError::Format(_)), "{why}: {err:?}");
        // The workbook is untouched: no cell, no kept content.
        assert_eq!(wb.sheet(1).map(|s| s.value(at("A1"))), Some(Value::default()), "{why}");
        assert!(wb.preserved.is_none(), "{why}");
    }
    // A missing sheet, and a report that runs off the grid.
    let mut wb = workbook();
    assert!(write_model_pivot(&mut wb, "Nope", at("A1"), &spec(), &report(), None).is_err());
    assert!(write_model_pivot(&mut wb, "Pivot", at("XFD1048576"), &spec(), &report(), None).is_err());
}

#[test]
fn excel_shaped_cache_and_table_are_understood() {
    // The shape Excel 16 writes for a model PivotTable: all hierarchies in the cache, only the used
    // fields as cache fields, the pivot fields in the same order as the cache fields, a page filter
    // named by its member (the shared items are empty), and an implicit count measure.
    let cache = r#"<?xml version="1.0"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" saveData="0" refreshedVersion="8" minRefreshableVersion="3" recordCount="0" supportSubquery="1" supportAdvancedDrill="1"><cacheSource type="external" connectionId="1"/><cacheFields count="4"><cacheField name="[Sales].[Region].[Region]" caption="Region" numFmtId="0" hierarchy="3" level="1"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField><cacheField name="[Products].[Category].[Category]" caption="Category" numFmtId="0" hierarchy="2" level="1"><sharedItems count="1"><s v="Hardware"/></sharedItems></cacheField><cacheField name="[Customers].[Segment].[Segment]" caption="Segment" numFmtId="0" hierarchy="1" level="1"><sharedItems containsSemiMixedTypes="0" containsNonDate="0" containsString="0"/></cacheField><cacheField name="[Measures].[__XL_Count Sales]" caption="__XL_Count Sales" numFmtId="0" hierarchy="5" level="32767"/></cacheFields><cacheHierarchies count="20"><cacheHierarchy uniqueName="[Customers].[Customer]" caption="Customer" attribute="1" dimensionUniqueName="[Customers]" memberValueDatatype="130"/><cacheHierarchy uniqueName="[Customers].[Segment]" caption="Segment" attribute="1" dimensionUniqueName="[Customers]" count="0" memberValueDatatype="130"/><cacheHierarchy uniqueName="[Products].[Category]" caption="Category" attribute="1" dimensionUniqueName="[Products]" count="1" memberValueDatatype="130"><fieldsUsage count="2"><fieldUsage x="-1"/><fieldUsage x="1"/></fieldsUsage></cacheHierarchy><cacheHierarchy uniqueName="[Sales].[Region]" caption="Region" attribute="1" dimensionUniqueName="[Sales]" count="2" memberValueDatatype="130"><fieldsUsage count="2"><fieldUsage x="-1"/><fieldUsage x="0"/></fieldsUsage></cacheHierarchy><cacheHierarchy uniqueName="[Sales].[Amount]" caption="Amount" attribute="1" dimensionUniqueName="[Sales]" memberValueDatatype="5"/><cacheHierarchy uniqueName="[Measures].[__XL_Count Sales]" caption="__XL_Count Sales" measure="1" measureGroup="Sales" count="0" oneField="1" hidden="1"><fieldsUsage count="1"><fieldUsage x="3"/></fieldsUsage></cacheHierarchy></cacheHierarchies><kpis count="0"/></pivotCacheDefinition>"#;
    let table = r#"<?xml version="1.0"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="ModelPivot" cacheId="42" dataCaption="Values" updatedVersion="8" minRefreshableVersion="3" createdVersion="5" indent="0" outline="1" outlineData="1"><location ref="A3:D9" firstHeaderRow="1" firstDataRow="2" firstDataCol="1" rowPageCount="1" colPageCount="1"/><pivotFields count="4"><pivotField axis="axisRow" showAll="0" defaultSubtotal="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField><pivotField axis="axisCol" showAll="0" defaultSubtotal="0"><items count="1"><item x="0"/></items></pivotField><pivotField axis="axisPage" showAll="0" defaultSubtotal="0"/><pivotField dataField="1" showAll="0" defaultSubtotal="0"/></pivotFields><rowFields count="1"><field x="0"/></rowFields><colFields count="1"><field x="1"/></colFields><pageFields count="1"><pageField fld="2" hier="1" name="[Customers].[Segment].&amp;[SMB]" cap="SMB"/></pageFields><dataFields count="1"><dataField fld="3" subtotal="count" baseField="0" baseItem="0"/></dataFields><pivotTableStyleInfo name="PivotStyleLight16" showRowHeaders="1" showColHeaders="1" showRowStripes="0" showColStripes="0" showLastColumn="1"/><rowHierarchiesUsage count="1"><rowHierarchyUsage hierarchyUsage="3"/></rowHierarchiesUsage><colHierarchiesUsage count="1"><colHierarchyUsage hierarchyUsage="2"/></colHierarchiesUsage></pivotTableDefinition>"#;
    let model = r#"<extLst><ext uri="{FCE2AD5D-F65C-4FA6-A056-5C36A1767C68}" xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main"><x15:dataModel><x15:modelTables><x15:modelTable id="Sales" name="Sales" connection="book.xlsx!Sales"/><x15:modelTable id="Products" name="Products" connection="book.xlsx!Products"/><x15:modelTable id="Customers" name="Customers" connection="book.xlsx!Customers"/></x15:modelTables><x15:modelRelationships><x15:modelRelationship fromTable="Sales" fromColumn="Product" toTable="Products" toColumn="Product"/></x15:modelRelationships></x15:dataModel></ext></extLst>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotCache/pivotCacheDefinition1.xml", cache),
        (
            "xl/pivotTables/_rels/pivotTable1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#,
        ),
    ];
    let bytes = minimal("", &parts, "", model);
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1, "{:?}", report.warnings);
    let spec = &pivots[0].spec;
    assert_eq!(spec.name, "ModelPivot");
    assert_eq!(spec.cache_id, 42);
    assert_eq!(pivots[0].location, at("A3"));
    assert_eq!(spec.row_fields, vec![field("Sales", "Region")]);
    assert_eq!(spec.column_fields, vec![field("Products", "Category")]);
    assert_eq!(spec.filters.len(), 1);
    assert_eq!(spec.filters[0].field, field("Customers", "Segment"));
    assert_eq!(spec.filters[0].allowed, vec![ModelPivotScalar::Text("SMB".into())]);
    // The implicit count measure is not a model measure; the value is reported as unreadable.
    assert!(spec.values.is_empty());
    assert!(spec.measures.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("implicit measures")), "{:?}", report.warnings);
    // Tables: model declaration plus the cache's columns with their data types.
    let products = spec.tables.iter().find(|t| t.name == "Products").unwrap();
    assert_eq!(products.connection, "book.xlsx!Products");
    assert_eq!(products.columns, vec![column("Category", "Category", ModelPivotDataType::Text)]);
    let sales = spec.tables.iter().find(|t| t.name == "Sales").unwrap();
    assert_eq!(sales.columns, vec![column("Region", "Region", ModelPivotDataType::Text), column("Amount", "Amount", ModelPivotDataType::Number)]);
    assert_eq!(spec.relationships.len(), 1);
}

#[test]
fn multi_select_filters_are_read_from_the_field_items() {
    let cache = r#"<?xml version="1.0"?><pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="external" connectionId="1"/><cacheFields count="2"><cacheField name="[Sales].[Region].[Region]" caption="Region" hierarchy="0" level="1"><sharedItems count="3"><s v="East"/><s v="North"/><s v="West"/></sharedItems></cacheField><cacheField name="[Measures].[Total Amount]" caption="Total Amount" hierarchy="1" level="32767"/></cacheFields><cacheHierarchies count="2"><cacheHierarchy uniqueName="[Sales].[Region]" caption="Region" attribute="1" dimensionUniqueName="[Sales]" count="3" memberValueDatatype="130"/><cacheHierarchy uniqueName="[Measures].[Total Amount]" caption="Total Amount" measure="1" measureGroup="Sales" count="0"/></cacheHierarchies></pivotCacheDefinition>"#;
    let table = r#"<?xml version="1.0"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="MS" cacheId="1"><location ref="A3"/><pivotFields count="2"><pivotField axis="axisPage" multipleItemSelectionAllowed="1" defaultSubtotal="0"><items count="2"><item x="0"/><item x="2"/></items></pivotField><pivotField dataField="1" defaultSubtotal="0"/></pivotFields><pageFields count="1"><pageField fld="0" hier="0" name="[Sales].[Region].[All]" cap="All"/></pageFields><dataFields count="1"><dataField fld="1" subtotal="count" baseField="0" baseItem="0"/></dataFields></pivotTableDefinition>"#;
    let parts = [
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotCache/pivotCacheDefinition1.xml", cache),
        (
            "xl/pivotTables/_rels/pivotTable1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#,
        ),
    ];
    let bytes = minimal("", &parts, "", "");
    let (pivots, _) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].spec.filters.len(), 1);
    assert_eq!(pivots[0].spec.filters[0].allowed, vec![ModelPivotScalar::Text("East".into()), ModelPivotScalar::Text("West".into())]);
    assert_eq!(pivots[0].spec.values[0].measure, "Total Amount");
}

#[test]
fn a_written_multi_value_filter_round_trips_through_the_items() {
    let bytes = written();
    let cache = text(&entries(&bytes), "xl/pivotCache/pivotCacheDefinition1.xml");
    assert!(cache.contains("<s v=\"Consumer\"/>") && cache.contains("<s v=\"SMB\"/>"), "{cache}");
    let table = text(&entries(&bytes), "xl/pivotTables/pivotTable1.xml");
    assert!(table.contains("multipleItemSelectionAllowed=\"1\""), "{table}");
    assert!(table.contains("<items count=\"2\"><item x=\"0\"/><item x=\"1\"/></items>"), "{table}");
    // The DAX measure field carries no shared items.
    assert!(
        !cache.contains(
            "<cacheField name=\"[Measures].[Total Amount]\" caption=\"Amount\" numFmtId=\"0\" hierarchy=\"9\" level=\"32767\"><sharedItems"
        ),
        "{cache}"
    );
}

#[test]
fn a_supplied_model_part_replaces_the_kept_one() {
    // A workbook read from disk keeps `xl/model/item.data`; the caller's bytes carry the session's
    // measures, so attaching must replace the kept part instead of silently keeping the old bytes.
    let mut wb = workbook();
    let preserved = wb.preserved.get_or_insert_with(|| Arc::new(Preserved::default()));
    Arc::make_mut(preserved).parts.push(PreservedPart {
        name: crate::pivot_model::MODEL_DATA_PART.to_string(),
        content_type: crate::pivot_model::CT_MODEL_DATA.to_string(),
        data: Arc::from(b"stale model".to_vec()),
        ..Default::default()
    });
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), Some(b"fresh model")).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    assert_eq!(parts.get(crate::pivot_model::MODEL_DATA_PART).map(Vec::as_slice), Some(b"fresh model".as_slice()));
}

#[test]
fn axis_fields_are_marked_all_drilled() {
    // Excel refuses the package unless every row/column field is marked fully drilled; the data
    // field must not carry the attribute. Both directions were proven by an Excel bisect.
    let bytes = written();
    let table = text(&entries(&bytes), "xl/pivotTables/pivotTable1.xml");
    for axis in ["axisRow", "axisCol"] {
        let rest = table.split(&format!("axis=\"{axis}\"")).nth(1).expect("an axis field");
        let tag = rest.split('>').next().unwrap_or_default();
        assert!(tag.contains("allDrilled=\"1\""), "{axis} field lacks allDrilled: {tag}");
    }
    let data = table.split("dataField=\"1\"").nth(1).expect("a data field");
    let tag = data.split('>').next().unwrap_or_default();
    assert!(!tag.contains("allDrilled"), "a data field must not be allDrilled: {tag}");
}

#[test]
fn a_single_value_filter_is_named_by_its_member() {
    let mut spec = spec();
    spec.filters[0].allowed = vec![ModelPivotScalar::Text("SMB".into())];
    let bytes = write_model_pivot(&mut workbook(), "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let table = text(&entries(&bytes), "xl/pivotTables/pivotTable1.xml");
    assert!(table.contains("name=\"[Customers].[Segment].&amp;[SMB]\" cap=\"SMB\""), "{table}");
    assert!(!table.contains("multipleItemSelectionAllowed"), "{table}");
    let (pivots, _) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots[0].spec.filters[0].allowed, vec![ModelPivotScalar::Text("SMB".into())]);
}

#[test]
fn zip_is_deterministic() {
    let first = written();
    let second = written();
    let a_parts = entries(&first);
    let b_parts = entries(&second);
    assert_eq!(a_parts.len(), b_parts.len());
    let mut a: Vec<&String> = a_parts.keys().collect();
    let mut b: Vec<&String> = b_parts.keys().collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    for name in &a {
        assert_eq!(a_parts.get(*name), b_parts.get(*name), "{name} differs between two writes");
    }
}

#[test]
fn empty_table_connections_never_produce_an_empty_attribute() {
    let mut spec = spec();
    for table in &mut spec.tables {
        table.connection.clear();
    }
    let bytes = write_model_pivot(&mut workbook(), "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let parts = entries(&bytes);
    let wb = text(&parts, "xl/workbook.xml");
    assert!(!wb.contains("connection=\"\""), "{wb}");
    assert!(!wb.contains("x15:dataModel"), "{wb}");
    // The pivot itself still reads back.
    let (pivots, _) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].spec.name, "ModelPivot");
    assert_eq!(pivots[0].spec.row_fields, vec![field("Sales", "Region")]);
}

#[test]
fn one_real_connection_keeps_the_extension_but_omits_the_blank_ones() {
    let mut spec = spec();
    spec.tables[0].connection = "WorksheetConnection_book.xlsx!Sales".into();
    spec.tables[1].connection.clear();
    let bytes = write_model_pivot(&mut workbook(), "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let wb = text(&entries(&bytes), "xl/workbook.xml");
    assert!(wb.contains("<x15:dataModel>"), "{wb}");
    assert!(wb.contains("connection=\"WorksheetConnection_book.xlsx!Sales\""), "{wb}");
    assert!(!wb.contains("connection=\"\""), "{wb}");
}

#[test]
fn connection_names_are_resolved_from_the_kept_connections_part() {
    let kept = "<connections xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><connection id=\"1\" keepAlive=\"1\" name=\"ThisWorkbookDataModel\" type=\"5\"/><connection id=\"2\" name=\"WorksheetConnection_book.xlsx!Sales\" type=\"102\"/><connection id=\"3\" name=\"WorksheetConnection_book.xlsx!Products\" type=\"102\"/><connection id=\"4\" name=\"WorksheetConnection_book.xlsx!Customers\" type=\"102\"/></connections>";
    let mut wb = workbook();
    wb.preserved = Some(Arc::new(gridcraft_model::Preserved {
        parts: vec![gridcraft_model::PreservedPart {
            name: "xl/connections.xml".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml".into(),
            data: Arc::from(kept.as_bytes().to_vec()),
            ..Default::default()
        }],
        ..Default::default()
    }));
    let mut spec = spec();
    for table in &mut spec.tables {
        table.connection.clear();
    }
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let parts = entries(&write_xlsx(&wb).unwrap());
    let wb_xml = text(&parts, "xl/workbook.xml");
    assert!(wb_xml.contains("<x15:dataModel>"), "{wb_xml}");
    for name in ["WorksheetConnection_book.xlsx!Sales", "WorksheetConnection_book.xlsx!Products", "WorksheetConnection_book.xlsx!Customers"] {
        assert!(wb_xml.contains(&format!("connection=\"{name}\"")), "{name} missing: {wb_xml}");
    }
    assert!(!wb_xml.contains("connection=\"\""), "{wb_xml}");
}

/// A package with a workbook-level data-model database and its `powerPivotData` relationship.
fn with_model_part() -> Vec<u8> {
    let rel = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/powerPivotData" Target="model/item.data"/>"#;
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="data" ContentType="application/vnd.openxmlformats-officedocument.model+data"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let wb = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>{rel}</Relationships>"#
    );
    let sheet = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;
    super::make_zip(&[
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", super::ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ("xl/model/item.data", b"the model database"),
    ])
}

#[test]
fn the_model_database_survives_a_read_and_save() {
    let bytes = with_model_part();
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let out = write_xlsx(&wb).unwrap();
    let parts = entries(&out);
    assert_eq!(parts.get("xl/model/item.data").map(Vec::as_slice), Some(&b"the model database"[..]), "parts {:?}", parts.keys().collect::<Vec<_>>());
    let ct = text(&parts, "[Content_Types].xml");
    assert!(ct.contains("Extension=\"data\""), "{ct}");
    let rels = text(&parts, "xl/_rels/workbook.xml.rels");
    assert!(rels.contains("relationships/powerPivotData"), "{rels}");
}

#[test]
fn attach_without_model_data_keeps_the_existing_one() {
    let bytes = with_model_part();
    let (mut wb, _) = read_xlsx(&bytes).unwrap();
    let mut spec = spec();
    for table in &mut spec.tables {
        table.connection.clear();
    }
    attach_model_pivot(&mut wb, "Data", at("A3"), &spec, &report(), None).unwrap();
    let parts = entries(&write_xlsx(&wb).unwrap());
    assert_eq!(parts.get("xl/model/item.data").map(Vec::as_slice), Some(&b"the model database"[..]));
    assert!(text(&parts, "[Content_Types].xml").contains("Extension=\"data\""));
    let rels = text(&parts, "xl/_rels/workbook.xml.rels");
    assert!(rels.contains("relationships/powerPivotData"), "{rels}");
    // The pivot itself is there too.
    let (pivots, _) = read_model_pivots(&entries_to_bytes(&parts)).unwrap();
    assert_eq!(pivots.len(), 1);
}

fn entries_to_bytes(parts: &HashMap<String, Vec<u8>>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        z.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn a_zero_cache_id_is_clamped_to_one() {
    let mut spec = spec();
    spec.cache_id = 0;
    let bytes = write_model_pivot(&mut workbook(), "Pivot", at("A3"), &spec, &report(), None).unwrap();
    let parts = entries(&bytes);
    let wb = text(&parts, "xl/workbook.xml");
    assert!(wb.contains("<pivotCache cacheId=\"1\""), "{wb}");
    let table = text(&parts, "xl/pivotTables/pivotTable1.xml");
    assert!(table.contains("cacheId=\"1\""), "{table}");
}

// ---------------------------------------------------------------- re-applying a pivot

/// A spec whose fields differ from [`spec`]'s, so an update in place is visible in what is read.
fn spec_with_fields(table: &str, column: &str) -> ModelPivotSpec {
    let mut s = spec();
    s.row_fields = vec![field(table, column)];
    s.row_members.clear();
    s.column_fields.clear();
    s.column_members.clear();
    s
}

/// Drops a part the workbook keeps, as a damaged package would have it missing.
fn remove_kept_part(wb: &mut Workbook, name: &str) {
    let p = Arc::make_mut(wb.preserved.as_mut().expect("the workbook keeps content"));
    p.parts.retain(|q| !q.name.eq_ignore_ascii_case(name));
}

/// Every internal relationship target exists in the package and every `<pivotCaches>` entry names
/// a workbook relationship: the integrity a kept package must not lose.
fn assert_package_is_consistent(bytes: &[u8]) {
    let parts = entries(bytes);
    for (name, data) in &parts {
        if !name.ends_with(".rels") {
            continue;
        }
        let src = name.replace("_rels/", "");
        let src = src.trim_end_matches(".rels").to_string();
        let dir = crate::package::split_dir(&src).0;
        let root = crate::xml::parse(data).unwrap_or_else(|e| panic!("{name}: {e}"));
        for r in root.kids("Relationship") {
            if r.attr("TargetMode") == Some("External") {
                continue;
            }
            let target = crate::package::resolve(dir, r.attr("Target").unwrap_or(""));
            assert!(parts.contains_key(&target), "{name}: dangling {target}");
        }
    }
    let wb_xml = crate::xml::parse(parts.get("xl/workbook.xml").expect("no workbook part")).expect("workbook XML");
    if let Some(caches) = wb_xml.child("pivotCaches") {
        let rels = text(&parts, "xl/_rels/workbook.xml.rels");
        for c in caches.kids("pivotCache") {
            let id = c.attr("id").unwrap_or("");
            assert!(!id.is_empty(), "a <pivotCaches> entry has no r:id");
            assert!(rels.contains(&format!("Id=\"{id}\"")), "the <pivotCaches> entry {id} names no workbook relationship");
        }
    }
}

fn pivot_parts(parts: &HashMap<String, Vec<u8>>, dir: &str, stem: &str) -> Vec<String> {
    let mut out: Vec<String> = parts.keys().filter(|n| n.starts_with(&format!("{dir}/{stem}")) && n.ends_with(".xml")).cloned().collect();
    out.sort();
    out
}

#[test]
fn applying_the_same_pivot_twice_updates_it_in_place() {
    let mut wb = workbook();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec_with_fields("Sales", "Product"), &report(), None).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    assert_eq!(pivot_parts(&parts, "xl/pivotTables", "pivotTable"), vec!["xl/pivotTables/pivotTable1.xml"]);
    assert_eq!(pivot_parts(&parts, "xl/pivotCache", "pivotCacheDefinition"), vec!["xl/pivotCache/pivotCacheDefinition1.xml"]);
    let wb_xml = text(&parts, "xl/workbook.xml");
    assert_eq!(wb_xml.matches("<pivotCache ").count(), 1, "{wb_xml}");
    let sheet_rels = text(&parts, "xl/worksheets/_rels/sheet2.xml.rels");
    assert_eq!(sheet_rels.matches("relationships/pivotTable").count(), 1, "{sheet_rels}");
    // The second apply's fields are the ones the package carries.
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1, "{:?}", report.warnings);
    assert_eq!(pivots[0].spec.name, "ModelPivot");
    assert_eq!(pivots[0].spec.row_fields, vec![field("Sales", "Product")]);
    assert!(pivots[0].spec.column_fields.is_empty());
    assert_eq!(pivots[0].spec.filters.len(), 1, "the unchanged filter stays");
    assert_package_is_consistent(&bytes);
}

#[test]
fn pivots_with_different_names_stay_separate() {
    let mut wb = workbook();
    let mut second = spec();
    second.name = "ModelPivot2".into();
    second.cache_id = 2;
    second.connection_id = 2;
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    attach_model_pivot(&mut wb, "Pivot", at("A12"), &second, &report(), None).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    assert_eq!(pivot_parts(&parts, "xl/pivotTables", "pivotTable").len(), 2);
    assert_eq!(pivot_parts(&parts, "xl/pivotCache", "pivotCacheDefinition").len(), 2);
    let wb_xml = text(&parts, "xl/workbook.xml");
    assert_eq!(wb_xml.matches("<pivotCache ").count(), 2, "{wb_xml}");
    let sheet_rels = text(&parts, "xl/worksheets/_rels/sheet2.xml.rels");
    assert_eq!(sheet_rels.matches("relationships/pivotTable").count(), 2, "{sheet_rels}");
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 2, "{:?}", report.warnings);
    assert_eq!(pivots[0].spec.name, "ModelPivot");
    assert_eq!(pivots[1].spec.name, "ModelPivot2");
    assert_package_is_consistent(&bytes);
}

#[test]
fn a_stale_pivot_part_is_replaced_without_leaving_dangling_relationships() {
    let mut wb = workbook();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    // The name matches but the cache behind it is gone: the part cannot be updated in place.
    remove_kept_part(&mut wb, "xl/pivotCache/pivotCacheDefinition1.xml");
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec_with_fields("Sales", "Product"), &report(), None).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = entries(&bytes);
    assert_eq!(pivot_parts(&parts, "xl/pivotTables", "pivotTable").len(), 1);
    assert_eq!(pivot_parts(&parts, "xl/pivotCache", "pivotCacheDefinition").len(), 1);
    assert_eq!(text(&parts, "xl/workbook.xml").matches("<pivotCache ").count(), 1);
    assert_package_is_consistent(&bytes);
    let (pivots, report) = read_model_pivots(&bytes).unwrap();
    assert_eq!(pivots.len(), 1, "{:?}", report.warnings);
    assert_eq!(pivots[0].spec.row_fields, vec![field("Sales", "Product")]);
}

#[test]
fn a_save_after_two_applies_is_deterministic() {
    let mut wb = workbook();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec(), &report(), None).unwrap();
    attach_model_pivot(&mut wb, "Pivot", at("A3"), &spec_with_fields("Sales", "Product"), &report(), None).unwrap();
    let a_parts = entries(&write_xlsx(&wb).unwrap());
    let b_parts = entries(&write_xlsx(&wb).unwrap());
    let (mut a, mut b): (Vec<&String>, Vec<&String>) = (a_parts.keys().collect(), b_parts.keys().collect());
    a.sort();
    b.sort();
    assert_eq!(a, b);
    for name in a {
        assert_eq!(a_parts.get(name), b_parts.get(name), "{name} differs between two writes");
    }
}
