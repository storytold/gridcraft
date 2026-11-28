//! PivotTables: write → read round trips, package structure, hostile pivot parts.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::*;

use super::minimal;
use crate::xml;
use crate::{read_xlsx, write_xlsx};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}
fn rr(a: &str) -> RangeRef {
    RangeRef::parse(a).unwrap()
}

fn date(y: i64, m: i64, d: i64) -> f64 {
    serial_from_ymd(gridcraft_core::DateSystem::D1900, y, m, d).unwrap()
}

fn field(name: &str) -> PivotField {
    PivotField { source_col: name.into(), ..PivotField::default() }
}

/// Source data on "Data" (A1:F9, also table "SalesTbl" and defined name "SalesData"), three
/// PivotTables on "Report" with a few hand-written report cells.
fn workbook() -> Workbook {
    let mut wb = Workbook::new();
    let date_style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("m/d/yyyy"));
    let data = wb.sheet_mut(0).unwrap();
    data.name = "Data".into();
    let headers = ["Region", "Product", "Date", "Units", "Sales", "Channel"];
    for (c, h) in headers.iter().enumerate() {
        data.cells.set(CellRef::new(0, c as u32), Cell::value(Value::text(*h)));
    }
    let rows: [(&str, &str, f64, f64, f64, &str); 8] = [
        ("East", "A", date(2024, 1, 5), 3.0, 30.5, "Online"),
        ("West", "B", date(2024, 2, 9), 5.0, 50.0, "Store"),
        ("East", "B", date(2024, 3, 1), 2.0, 20.0, "Online"),
        ("North", "C", date(2025, 4, 12), 7.0, 70.25, "Store"),
        ("west", "A", date(2025, 5, 30), 1.0, 10.0, "Online"),
        ("East", "C", date(2025, 6, 2), 4.0, 40.0, "Phone"),
        ("North", "A", date(2024, 7, 7), 6.0, 60.0, "Online"),
        ("West", "C", date(2025, 12, 31), 8.0, 80.0, "Store"),
    ];
    for (r, (a, b, d, u, s, ch)) in rows.iter().enumerate() {
        let r = r as u32 + 1;
        data.cells.set(CellRef::new(r, 0), Cell::value(Value::text(*a)));
        data.cells.set(CellRef::new(r, 1), Cell::value(Value::text(*b)));
        data.cells.set(CellRef::new(r, 2), Cell { value: Value::number(*d), formula: None, style: date_style, py: None });
        data.cells.set(CellRef::new(r, 3), Cell::value(Value::number(*u)));
        data.cells.set(CellRef::new(r, 4), Cell::value(Value::number(*s)));
        data.cells.set(CellRef::new(r, 5), Cell::value(Value::text(*ch)));
    }
    data.tables.push(Table {
        id: 1,
        name: "SalesTbl".into(),
        range: rr("A1:F9"),
        header_row: true,
        totals_row: false,
        columns: headers
            .iter()
            .map(|h| TableColumn { name: (*h).into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None })
            .collect(),
        style: "TableStyleMedium2".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    wb.names.push(DefinedName { name: "SalesData".into(), scope: None, formula: "Data!$A$1:$F$9".into(), comment: String::new(), hidden: false });

    let mut report = Sheet::new("Report");
    // Pivot 1: tabular, page filter, custom and built-in value formats, collapsed item, Enable
    // show details off.
    let p1 = PivotTable {
        id: 1,
        name: "Sales by Region".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("A4"),
        rows: vec![PivotField {
            source_col: "Region".into(),
            sort: PivotSort::Desc,
            collapsed_items: vec!["East".into()],
            caption: Some("Area".into()),
            sort_by: Some("Sum of Sales".into()),
            sort_scope: vec![PivotScopeItem { source_col: "Product".into(), item: "B".into(), ..PivotScopeItem::default() }],
            item_captions: vec![("West".into(), "**West".into())],
            subtotal: Some(true),
            repeat_labels: true,
            number_format: Some("0.0".into()),
            ..PivotField::default()
        }],
        columns: vec![PivotField { sort: PivotSort::None, item_order: vec!["C".into(), "A".into(), "B".into()], ..field("Product") }],
        values: vec![
            PivotValue {
                source_col: "Sales".into(),
                func: PivotFunc::Sum,
                name: "Sum of Sales".into(),
                number_format: Some("#,##0.00".into()),
                ..PivotValue::default()
            },
            PivotValue {
                source_col: "Units".into(),
                func: PivotFunc::Count,
                name: "Count of Units".into(),
                number_format: Some("0.0\" u\"".into()),
                ..PivotValue::default()
            },
        ],
        filters: vec![PivotFilter { source_col: "Channel".into(), selected: Some(vec!["Online".into()]) }],
        layout: PivotLayout::Tabular,
        enable_show_details: false,
        grand_totals_rows: false,
        grand_totals_cols: true,
        subtotals: false,
        subtotals_top: false,
        style: "PivotStyleMedium9".into(),
        show_headers: false,
        last_range: Some(rr("A2:H12")),
        row_header_caption: Some("Vehicle".into()),
        col_header_caption: Some("Type".into()),
        data_caption: Some("Data".into()),
        grand_total_caption: Some("Days".into()),
        empty_caption: Some("0".into()),
        show_values_row: false,
        classic_layout: true,
        calculated_fields: vec![],
        data_index: Default::default(),
        values_on_rows: false,
        values_position: None,
    };
    // Pivot 2: table source, date grouping on two levels, show-values-as, multi-select page
    // filter and a hidden-items filter on a column field.
    let p2 = PivotTable {
        id: 2,
        name: "Trend".into(),
        source: "SalesTbl".into(),
        anchor: at("K4"),
        rows: vec![
            PivotField {
                source_col: "Date".into(),
                date_group: PivotDateGroup::Years,
                sort: PivotSort::None,
                collapsed_items: vec!["2024".into()],
                ..PivotField::default()
            },
            PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Months, sort: PivotSort::Asc, ..PivotField::default() },
        ],
        columns: vec![PivotField { sort: PivotSort::Desc, ..field("Product") }],
        values: vec![
            PivotValue {
                source_col: "Sales".into(),
                func: PivotFunc::Sum,
                name: "Share".into(),
                show_as: PivotShowAs::PercentOfGrandTotal,
                number_format: None,
            },
            PivotValue { source_col: "Sales".into(), func: PivotFunc::Max, name: "Rank".into(), show_as: PivotShowAs::Rank, number_format: None },
            PivotValue {
                source_col: "Sales".into(),
                func: PivotFunc::Average,
                name: "Running".into(),
                show_as: PivotShowAs::RunningTotal,
                number_format: None,
            },
            PivotValue {
                source_col: "Units".into(),
                func: PivotFunc::StdDevP,
                name: "Spread".into(),
                show_as: PivotShowAs::PercentOfColumnTotal,
                number_format: None,
            },
            PivotValue {
                source_col: "Units".into(),
                func: PivotFunc::VarP,
                name: "RowPct".into(),
                show_as: PivotShowAs::PercentOfRowTotal,
                number_format: None,
            },
        ],
        filters: vec![
            PivotFilter { source_col: "Region".into(), selected: Some(vec!["East".into(), "North".into()]) },
            PivotFilter { source_col: "Product".into(), selected: Some(vec!["A".into(), "B".into()]) },
        ],
        layout: PivotLayout::Outline,
        enable_show_details: true,
        grand_totals_rows: true,
        grand_totals_cols: false,
        subtotals: true,
        subtotals_top: true,
        style: "PivotStyleDark3".into(),
        show_headers: true,
        last_range: Some(rr("K2:Z30")),
        ..PivotTable::default()
    };
    // Pivot 3: defaults, defined-name source, compact.
    let p3 = PivotTable {
        id: 3,
        name: "PivotTable3".into(),
        source: "SalesData".into(),
        anchor: at("AB1"),
        rows: vec![field("Product")],
        values: vec![PivotValue { source_col: "Units".into(), func: PivotFunc::Product, name: "Product of Units".into(), ..PivotValue::default() }],
        filters: vec![PivotFilter { source_col: "Channel".into(), selected: None }],
        last_range: Some(rr("AB1:AC6")),
        ..PivotTable::default()
    };
    report.pivots = vec![p1, p2, p3];
    // Hand-written report cells (the engine's output stays plain cells).
    report.cells.set(at("A2"), Cell::value(Value::text("Channel")));
    report.cells.set(at("B2"), Cell::value(Value::text("Online")));
    report.cells.set(at("A5"), Cell::value(Value::text("East")));
    report.cells.set(at("B5"), Cell::value(Value::number(50.5)));
    wb.sheets.push(Arc::new(report));
    wb
}

fn unzip(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut m = HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        m.insert(f.name().unwrap().to_string(), b);
    }
    m
}

#[test]
fn pivots_round_trip() {
    let wb = workbook();
    let bytes = write_xlsx(&wb).unwrap();
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.to_lowercase().contains("pivot")), "{:?}", report.warnings);
    let orig = &wb.sheet(1).unwrap().pivots;
    let read = &back.sheet(1).unwrap().pivots;
    assert_eq!(read.len(), 3);
    for (a, b) in orig.iter().zip(read.iter()) {
        assert_eq!(a, b, "pivot {} differs", a.name);
    }
    // Report cells stay as written.
    let s = back.sheet(1).unwrap();
    assert_eq!(s.value(at("B5")), Value::number(50.5));
    assert_eq!(s.value(at("A5")), Value::text("East"));
    // A second trip is stable.
    let (again, _) = read_xlsx(&write_xlsx(&back).unwrap()).unwrap();
    assert_eq!(&again.sheet(1).unwrap().pivots, read);
}

#[test]
fn pivot_package_structure() {
    let parts = unzip(&write_xlsx(&workbook()).unwrap());
    assert_pivot_content_types(&parts);
    assert_relationship_targets_exist(&parts);
    let cache_ids = assert_workbook_cache_relationships(&parts);
    assert_pivot_tables_reference_caches(&parts, &cache_ids);
    assert_sheet_pivot_relationships(&parts);
}

fn assert_pivot_content_types(parts: &HashMap<String, Vec<u8>>) {
    let ct = xml::parse(parts.get("[Content_Types].xml").unwrap()).unwrap();
    let overrides: HashMap<String, String> = ct
        .kids("Override")
        .map(|o| (o.attr("PartName").unwrap().trim_start_matches('/').to_string(), o.attr("ContentType").unwrap().to_string()))
        .collect();
    let defaults: HashSet<String> = ct.kids("Default").map(|d| d.attr("Extension").unwrap().to_string()).collect();
    for part in overrides.keys() {
        assert!(parts.contains_key(part), "override for missing part {part}");
    }
    for part in parts.keys().filter(|p| p.as_str() != "[Content_Types].xml") {
        let ext = part.rsplit('.').next().unwrap();
        assert!(overrides.contains_key(part) || defaults.contains(ext), "no content type for {part}");
    }
    let count = |content_type: &str| overrides.values().filter(|value| value.as_str() == content_type).count();
    assert_eq!(count(crate::pivot::CT_PIVOT_TABLE), 3);
    assert_eq!(count(crate::pivot::CT_PIVOT_CACHE_DEF), 3);
    assert_eq!(count(crate::pivot::CT_PIVOT_CACHE_REC), 3);
}

fn assert_relationship_targets_exist(parts: &HashMap<String, Vec<u8>>) {
    let mut kinds: Vec<String> = Vec::new();
    for (name, data) in parts.iter().filter(|(name, _)| name.ends_with(".rels")) {
        let dir = name.trim_end_matches(".rels").rsplit_once("/_rels/").map(|(d, _)| d).unwrap_or("");
        for relation in xml::parse(data).unwrap().kids("Relationship") {
            if relation.attr("TargetMode") == Some("External") {
                continue;
            }
            let target = crate::package::resolve(dir, relation.attr("Target").unwrap());
            assert!(parts.contains_key(&target), "{name} points at missing {target}");
            kinds.push(relation.attr("Type").unwrap().rsplit('/').next().unwrap().to_string());
        }
    }
    for kind in ["pivotTable", "pivotCacheDefinition", "pivotCacheRecords"] {
        assert!(kinds.iter().any(|found| found == kind), "no {kind} relationship");
    }
}

fn assert_workbook_cache_relationships(parts: &HashMap<String, Vec<u8>>) -> HashSet<String> {
    let workbook = xml::parse(parts.get("xl/workbook.xml").unwrap()).unwrap();
    let relationships = xml::parse(parts.get("xl/_rels/workbook.xml.rels").unwrap()).unwrap();
    let caches: Vec<&xml::El> = workbook.child("pivotCaches").unwrap().kids("pivotCache").collect();
    assert_eq!(caches.len(), 3);
    let mut ids = HashSet::new();
    for cache in &caches {
        assert!(ids.insert(cache.attr("cacheId").unwrap().to_string()));
        let rid = cache.attr("id").unwrap();
        let relation = relationships.kids("Relationship").find(|r| r.attr("Id") == Some(rid)).unwrap();
        assert!(relation.attr("Type").unwrap().ends_with("/pivotCacheDefinition"));
    }
    ids
}

fn assert_pivot_tables_reference_caches(parts: &HashMap<String, Vec<u8>>, ids: &HashSet<String>) {
    for table_number in 1..=3 {
        let table = xml::parse(parts.get(&format!("xl/pivotTables/pivotTable{table_number}.xml")).unwrap()).unwrap();
        assert!(ids.contains(table.attr("cacheId").unwrap()));
        let table_rels = xml::parse(parts.get(&format!("xl/pivotTables/_rels/pivotTable{table_number}.xml.rels")).unwrap()).unwrap();
        let cache_path = crate::package::resolve("xl/pivotTables", table_rels.kids("Relationship").next().unwrap().attr("Target").unwrap());
        let cache = xml::parse(parts.get(&cache_path).unwrap()).unwrap();
        assert_pivot_table_fields_match_cache(&table, &cache);
        assert_pivot_cache_records_match_fields(parts, &cache, &cache_path);
    }
}

fn assert_pivot_table_fields_match_cache(table: &xml::El, cache: &xml::El) {
    let cache_fields = cache.child("cacheFields").unwrap();
    let field_count = cache_fields.kids("cacheField").count();
    assert_eq!(table.child("pivotFields").unwrap().kids("pivotField").count(), field_count);
    assert_eq!(cache_fields.attr_u32("count"), Some(field_count as u32));
    for (field_index, pivot_field) in table.child("pivotFields").unwrap().kids("pivotField").enumerate() {
        let cache_field = cache_fields.kids("cacheField").nth(field_index).unwrap();
        let item_count = cache_field
            .path(&["fieldGroup", "groupItems"])
            .map(|items| items.children.len())
            .unwrap_or_else(|| cache_field.child("sharedItems").map(|items| items.children.len()).unwrap_or(0));
        if let Some(items) = pivot_field.child("items") {
            assert_eq!(items.attr_u32("count"), Some(items.children.len() as u32));
            for item in items.kids("item") {
                if let Some(item_index) = item.attr_u32("x") {
                    assert!((item_index as usize) < item_count, "item x={item_index} out of {item_count} in field {field_index}");
                }
            }
        }
    }
}

fn assert_pivot_cache_records_match_fields(parts: &HashMap<String, Vec<u8>>, cache: &xml::El, cache_path: &str) {
    let Some(rid) = cache.attr("id") else { return };
    let rels_path = cache_path.replace("pivotCache/", "pivotCache/_rels/") + ".rels";
    let cache_rels = xml::parse(parts.get(&rels_path).unwrap()).unwrap();
    let relation = cache_rels.kids("Relationship").find(|r| r.attr("Id") == Some(rid)).unwrap();
    let records_path = crate::package::resolve("xl/pivotCache", relation.attr("Target").unwrap());
    let records = xml::parse(parts.get(&records_path).unwrap()).unwrap();
    let database_fields = cache.child("cacheFields").unwrap().kids("cacheField").filter(|f| f.attr("databaseField") != Some("0")).count();
    assert_eq!(records.attr_u32("count"), cache.attr_u32("recordCount"));
    assert_eq!(records.kids("r").count(), 8);
    for record in records.kids("r") {
        assert_eq!(record.children.len(), database_fields);
    }
}

fn assert_sheet_pivot_relationships(parts: &HashMap<String, Vec<u8>>) {
    let sheet_rels = xml::parse(parts.get("xl/worksheets/_rels/sheet2.xml.rels").unwrap()).unwrap();
    assert_eq!(sheet_rels.kids("Relationship").filter(|r| r.attr("Type").unwrap().ends_with("/pivotTable")).count(), 3);
}

#[test]
fn pivot_xml_details() {
    let parts = unzip(&write_xlsx(&workbook()).unwrap());
    let t1 = String::from_utf8(parts.get("xl/pivotTables/pivotTable1.xml").unwrap().clone()).unwrap();
    assert!(
        t1.contains("<location ref=\"A4:H12\" firstHeaderRow=\"1\" firstDataRow=\"3\" firstDataCol=\"1\" rowPageCount=\"1\" colPageCount=\"1\"/>"),
        "{t1}"
    );
    assert!(t1.contains("<field x=\"-2\"/>"));
    // Sort by the values in the "B" column: Product's item B sits third in its `<items>`.
    assert!(
        t1.contains("<references count=\"2\"><reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"0\"/></reference><reference field=\"1\" count=\"1\" selected=\"0\"><x v=\"2\"/></reference></references>"),
        "{t1}"
    );
    assert!(t1.contains("subtotal=\"count\""));
    assert!(t1.contains("numFmtId=\"4\""));
    assert!(t1.contains("axis=\"axisPage\""));
    assert!(t1.contains("<pivotTableStyleInfo name=\"PivotStyleMedium9\""));
    let c1 = String::from_utf8(parts.get("xl/pivotCache/pivotCacheDefinition1.xml").unwrap().clone()).unwrap();
    assert!(c1.contains("<worksheetSource ref=\"A1:F9\" sheet=\"Data\"/>"), "{c1}");
    assert!(c1.contains("refreshOnLoad=\"1\""));
    let c2 = String::from_utf8(parts.get("xl/pivotCache/pivotCacheDefinition2.xml").unwrap().clone()).unwrap();
    assert!(c2.contains("<worksheetSource name=\"SalesTbl\"/>"));
    assert!(c2.contains("groupBy=\"years\"") && c2.contains("groupBy=\"months\""));
    let t2 = String::from_utf8(parts.get("xl/pivotTables/pivotTable2.xml").unwrap().clone()).unwrap();
    assert!(t2.contains("pivotShowAs=\"rankDescending\""));
    assert!(t2.contains("showDataAs=\"runTotal\""));
    // Pivot 1 has no Grand Total column (`colGrandTotals`), pivot 2 no Grand Total row
    // (`rowGrandTotals`).
    assert!(t1.contains(" colGrandTotals=\"0\"") && !t1.contains("rowGrandTotals"), "{t1}");
    assert!(t2.contains(" rowGrandTotals=\"0\"") && !t2.contains("colGrandTotals"), "{t2}");
    // The custom value format is registered in the styles part.
    let styles = String::from_utf8(parts.get("xl/styles.xml").unwrap().clone()).unwrap();
    assert!(styles.contains("formatCode=\"0.0&quot; u&quot;\""));
}

#[test]
fn unresolvable_pivot_source_is_skipped() {
    let mut wb = workbook();
    let s = wb.sheet_mut(1).unwrap();
    s.pivots[0].source = "Nowhere!A1:B2".into();
    s.pivots.truncate(1);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(back.sheet(1).unwrap().pivots.is_empty());
}

#[test]
fn large_cache_is_written_without_records() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.cells.set(at("A1"), Cell::value(Value::text("K")));
    s.cells.set(at("B1"), Cell::value(Value::text("V")));
    s.cells.set(at("C1"), Cell::value(Value::text("W")));
    // 700,000 records x 3 fields is over the records limit.
    for r in 1..=700_000u32 {
        s.cells.set(CellRef::new(r, 1), Cell::value(Value::number(r as f64)));
        s.cells.set(CellRef::new(r, 2), Cell::value(Value::number(0.5)));
        s.cells.set(CellRef::new(r, 0), Cell::value(Value::number((r % 3) as f64)));
    }
    s.pivots.push(PivotTable {
        id: 1,
        source: "Sheet1!$A$1:$C$700001".into(),
        anchor: at("E1"),
        rows: vec![field("K")],
        values: vec![PivotValue { source_col: "V".into(), name: "Sum of V".into(), ..PivotValue::default() }],
        last_range: Some(rr("E1:F5")),
        ..PivotTable::default()
    });
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    assert!(!parts.keys().any(|k| k.contains("pivotCacheRecords")));
    let c = String::from_utf8(parts.get("xl/pivotCache/pivotCacheDefinition1.xml").unwrap().clone()).unwrap();
    assert!(c.contains("saveData=\"0\""));
    assert!(c.contains("<sharedItems containsSemiMixedTypes=\"0\" containsString=\"0\" containsNumber=\"1\" containsInteger=\"1\" minValue=\"0\" maxValue=\"2\" count=\"3\">"), "{}", &c[..c.len().min(2000)]);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().pivots.len(), 1);
}

/// #1447 item 1: Excel counts booleans and errors as strings in `<sharedItems>`; writing
/// `containsString="0"` for them makes a file Excel can't open.
#[test]
fn boolean_and_error_cache_flags_match_excel() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    let na = || Value::Error(gridcraft_core::CellError::NA);
    let cols: [(&str, [Value; 3]); 7] = [
        ("K", [Value::text("a"), Value::text("b"), Value::text("a")]),
        ("BoolOnly", [Value::Bool(true), Value::Bool(false), Value::Bool(true)]),
        ("NumBool", [Value::number(1.0), Value::Bool(true), Value::number(2.5)]),
        ("ErrOnly", [na(), na(), na()]),
        ("NumErr", [Value::number(1.0), na(), Value::number(3.0)]),
        ("TextBool", [Value::text("x"), Value::Bool(true), Value::text("y")]),
        ("BoolBlank", [Value::Bool(true), Value::Empty, Value::Bool(false)]),
    ];
    for (c, (h, vals)) in cols.iter().enumerate() {
        s.cells.set(CellRef::new(0, c as u32), Cell::value(Value::text(*h)));
        for (r, v) in vals.iter().enumerate() {
            if !matches!(v, Value::Empty) {
                s.cells.set(CellRef::new(r as u32 + 1, c as u32), Cell::value(v.clone()));
            }
        }
    }
    s.pivots.push(PivotTable {
        id: 1,
        source: "Sheet1!$A$1:$G$4".into(),
        anchor: at("J1"),
        rows: vec![field("K")],
        values: vec![PivotValue { source_col: "NumBool".into(), name: "Sum of NumBool".into(), ..PivotValue::default() }],
        last_range: Some(rr("J1:K4")),
        ..PivotTable::default()
    });
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let c = String::from_utf8(parts.get("xl/pivotCache/pivotCacheDefinition1.xml").unwrap().clone()).unwrap();
    let start = |name: &str| -> String {
        let i = c.find(&format!("<cacheField name=\"{name}\"")).unwrap();
        let j = i + c[i..].find("<sharedItems").unwrap();
        let k = j + c[j..].find('>').unwrap();
        c[j..=k].trim_end_matches('>').trim_end_matches('/').to_string()
    };
    let flags = |name: &str| -> String {
        let t = start(name);
        // Drop the trailing count attribute, which depends on itemization.
        match t.find(" count=") {
            Some(i) => t[..i].to_string(),
            None => t,
        }
    };
    assert_eq!(flags("BoolOnly"), "<sharedItems");
    assert_eq!(flags("NumBool"), "<sharedItems containsMixedTypes=\"1\" containsNumber=\"1\" minValue=\"1\" maxValue=\"2.5\"");
    assert_eq!(flags("ErrOnly"), "<sharedItems");
    assert_eq!(flags("NumErr"), "<sharedItems containsMixedTypes=\"1\" containsNumber=\"1\" containsInteger=\"1\" minValue=\"1\" maxValue=\"3\"");
    assert_eq!(flags("TextBool"), "<sharedItems");
    assert_eq!(flags("BoolBlank"), "<sharedItems containsBlank=\"1\"");
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().pivots.len(), 1);
    assert!(!report.warnings.iter().any(|w| w.to_lowercase().contains("pivot")), "{:?}", report.warnings);
}

const PT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#;
const PT_CACHE_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#;

fn with_pivot(table: &str, cache: Option<&str>) -> Vec<u8> {
    let mut extra: Vec<(&str, &str)> = vec![
        ("xl/worksheets/_rels/sheet1.xml.rels", PT_RELS),
        ("xl/pivotTables/pivotTable1.xml", table),
        ("xl/pivotTables/_rels/pivotTable1.xml.rels", PT_CACHE_RELS),
    ];
    if let Some(c) = cache {
        extra.push(("xl/pivotCache/pivotCacheDefinition1.xml", c));
    }
    minimal("<sheetData/>", &extra, "", "")
}

const GOOD_CACHE: &str = r#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="worksheet"><worksheetSource ref="A1:B3"/></cacheSource><cacheFields count="2"><cacheField name="K"><sharedItems><s v="x"/><n v="2"/><m/><b v="1"/><d v="2024-01-01T00:00:00"/></sharedItems></cacheField><cacheField name="V"><sharedItems/><fieldGroup base="0"><rangePr startNum="0" endNum="10" groupInterval="5"/><groupItems><s v="0-4"/></groupItems></fieldGroup></cacheField></cacheFields></pivotCacheDefinition>"#;

#[test]
fn hostile_pivot_parts_warn_not_panic() {
    let tables = [
        "garbage <<<",
        r#"<notAPivot/>"#,
        r#"<pivotTableDefinition name="P" cacheId="1"/>"#,
        r#"<pivotTableDefinition name="P" cacheId="1"><location ref="nonsense"/></pivotTableDefinition>"#,
        r#"<pivotTableDefinition name="" cacheId="9"><location ref="$D$3:$F$9"/><pivotFields count="1"><pivotField axis="axisRow" sortType="descending"><items><item x="99" h="1" sd="0"/><item x="-4"/><item t="default"/></items></pivotField></pivotFields><rowFields><field x="0"/><field x="4294967295"/><field x="-2"/><field x="77"/></rowFields><colFields><field x="1"/></colFields><pageFields><pageField fld="0" item="9999"/><pageField fld="-1"/><pageField fld="55"/></pageFields><dataFields><dataField fld="1" subtotal="bogus" showDataAs="index" numFmtId="99999"/><dataField fld="12"/><dataField name="E" fld="0"><extLst><ext><dataField pivotShowAs="percentOfParent"/></ext></extLst></dataField></dataFields></pivotTableDefinition>"#,
    ];
    let caches = [
        None,
        Some("garbage"),
        Some(r#"<pivotCacheDefinition><cacheSource type="external"/></pivotCacheDefinition>"#),
        Some(r#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource ref="zz"/></cacheSource></pivotCacheDefinition>"#),
        Some(r#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource ref="A1:B2"/></cacheSource></pivotCacheDefinition>"#),
        Some(r#"<somethingElse/>"#),
        Some(GOOD_CACHE),
    ];
    for t in &tables {
        for c in &caches {
            let bytes = with_pivot(t, *c);
            let (wb, report) = read_xlsx(&bytes).unwrap();
            let pivots = &wb.sheet(0).unwrap().pivots;
            if pivots.is_empty() {
                assert!(!report.warnings.is_empty(), "no warning for table {t} / cache {c:?}");
            }
        }
    }
    // The fully hostile table against a good cache still yields a pivot, with warnings.
    let (wb, report) = read_xlsx(&with_pivot(tables[4], Some(GOOD_CACHE))).unwrap();
    let pt = &wb.sheet(0).unwrap().pivots[0];
    assert_eq!(pt.anchor, at("D3"));
    assert_eq!(pt.source, "Data!$A$1:$B$3");
    assert_eq!(pt.rows.len(), 1);
    assert_eq!(pt.rows[0].sort, PivotSort::Desc);
    assert!(pt.values.iter().any(|v| v.func == PivotFunc::Sum && v.source_col == "V"));
    assert!(report.warnings.iter().any(|w| w.contains("bogus")), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("grouping")), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("index")), "{:?}", report.warnings);
}

/// #2159: a field repeated on the axes or as a page field is used once (each use walks the
/// field's items and copied its labels), and the lists are capped.
#[test]
fn repeated_pivot_fields_are_used_once() {
    let items: String = (0..2_000).map(|_| r#"<item x="0" sd="0"/>"#).collect();
    let rows = r#"<field x="0"/>"#.repeat(20_000);
    let pages = r#"<pageField fld="0"/>"#.repeat(20_000);
    let table = format!(
        r#"<pivotTableDefinition name="P" cacheId="1"><location ref="D5:F9"/><pivotFields count="1"><pivotField axis="axisRow"><items>{items}</items></pivotField></pivotFields><rowFields>{rows}<field x="-2"/></rowFields><colFields>{rows}</colFields><pageFields>{pages}</pageFields></pivotTableDefinition>"#
    );
    let (wb, report) = read_xlsx(&with_pivot(&table, Some(GOOD_CACHE))).unwrap();
    let pt = &wb.sheet(0).unwrap().pivots[0];
    assert_eq!(pt.rows.len(), 1, "{:?}", report.warnings);
    assert!(pt.columns.is_empty());
    assert_eq!(pt.rows[0].collapsed_items.len(), 2_000);
    assert_eq!(pt.filters.iter().filter(|f| f.source_col == "K").count(), 1);
    // One page field: the table's range starts two rows above its location.
    assert_eq!(pt.last_range, Some(rr("D3:F9")));
}

/// #2159: a field list cut at the 16,384-entry cap is reported, so an incomplete PivotTable can be
/// told apart from a complete one.
#[test]
fn capped_pivot_field_lists_warn() {
    let fields = r#"<pivotField/>"#.repeat(16_385);
    let table = format!(
        r#"<pivotTableDefinition name="P" cacheId="1"><location ref="D5:F9"/><pivotFields count="16385">{fields}</pivotFields></pivotTableDefinition>"#
    );
    let (_, report) = read_xlsx(&with_pivot(&table, Some(GOOD_CACHE))).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("field lists past 16384 entries")), "{:?}", report.warnings);
    let table = r#"<pivotTableDefinition name="P" cacheId="1"><location ref="D5:F9"/><pivotFields count="1"><pivotField/></pivotFields></pivotTableDefinition>"#;
    let (_, report) = read_xlsx(&with_pivot(table, Some(GOOD_CACHE))).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("field lists past")), "{:?}", report.warnings);
}

/// #2159: a cache cut at 16,384 fields is reported too, as the table field lists are.
#[test]
fn capped_cache_fields_warn() {
    let fields = r#"<cacheField name="F"><sharedItems/></cacheField>"#.repeat(16_385);
    let cache = format!(
        r#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="worksheet"><worksheetSource ref="A1:B3"/></cacheSource><cacheFields count="16385">{fields}</cacheFields></pivotCacheDefinition>"#
    );
    let table = r#"<pivotTableDefinition name="P" cacheId="1"><location ref="D5:F9"/><pivotFields count="1"><pivotField/></pivotFields></pivotTableDefinition>"#;
    let (_, report) = read_xlsx(&with_pivot(table, Some(&cache))).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("cache fields past 16384")), "{:?}", report.warnings);
    let (_, report) = read_xlsx(&with_pivot(table, Some(GOOD_CACHE))).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("cache fields past")), "{:?}", report.warnings);
}

/// #2159: axis filters and calculated fields are deduplicated through a set of lower-cased names;
/// the first of names equal but for case still wins, as with the scan it replaced.
#[test]
fn axis_filters_and_calculated_fields_dedupe_ignoring_case() {
    let cache = r#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheSource type="worksheet"><worksheetSource ref="A1:B3"/></cacheSource><cacheFields count="4"><cacheField name="K"><sharedItems><s v="a"/><s v="b"/></sharedItems></cacheField><cacheField name="k"><sharedItems><s v="a"/><s v="b"/></sharedItems></cacheField><cacheField name="Price" formula="1" databaseField="0"/><cacheField name="PRICE" formula="2" databaseField="0"/></cacheFields></pivotCacheDefinition>"#;
    let table = r#"<pivotTableDefinition name="P" cacheId="1"><location ref="D5:F9"/><pivotFields count="2"><pivotField axis="axisRow"><items><item x="0"/><item x="1" h="1"/></items></pivotField><pivotField axis="axisCol"><items><item x="0" h="1"/><item x="1"/></items></pivotField></pivotFields><rowFields><field x="0"/></rowFields><colFields><field x="1"/></colFields></pivotTableDefinition>"#;
    let (wb, report) = read_xlsx(&with_pivot(table, Some(cache))).unwrap();
    let pt = &wb.sheet(0).unwrap().pivots[0];
    assert_eq!((pt.rows.len(), pt.columns.len()), (1, 1), "{:?}", report.warnings);
    assert_eq!(pt.filters, vec![PivotFilter { source_col: "K".into(), selected: Some(vec!["a".into()]) }]);
    assert_eq!(pt.calculated_fields, vec![PivotCalcField { name: "Price".into(), formula: "=1".into() }]);
}

#[test]
fn cache_found_by_workbook_cache_id() {
    let rels_extra = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition1.xml"/>"#;
    let table = r#"<pivotTableDefinition name="ById" cacheId="7"><location ref="C1:D4"/><pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><rowFields><field x="0"/></rowFields><dataFields><dataField name="Total" fld="1" subtotal="average"/></dataFields></pivotTableDefinition>"#;
    let bytes = minimal(
        "<sheetData/>",
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", PT_RELS),
            ("xl/pivotTables/pivotTable1.xml", table),
            ("xl/pivotCache/pivotCacheDefinition1.xml", GOOD_CACHE),
        ],
        rels_extra,
        "",
    );
    // `minimal` puts wb_extra before <sheets>; pivotCaches belongs after, but readers locate it by name.
    let bytes2 = {
        let mut parts = unzip(&bytes);
        let wbx = String::from_utf8(parts.remove("xl/workbook.xml").unwrap()).unwrap();
        let wbx = wbx.replace("</workbook>", r#"<pivotCaches><pivotCache cacheId="7" r:id="rId9"/></pivotCaches></workbook>"#);
        let mut list: Vec<(String, Vec<u8>)> = parts.into_iter().collect();
        list.push(("xl/workbook.xml".into(), wbx.into_bytes()));
        let refs: Vec<(&str, &[u8])> = list.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        super::make_zip(&refs)
    };
    let (wb, report) = read_xlsx(&bytes2).unwrap();
    let pts = &wb.sheet(0).unwrap().pivots;
    assert_eq!(pts.len(), 1, "{:?}", report.warnings);
    assert_eq!(pts[0].name, "ById");
    assert_eq!(pts[0].values[0].func, PivotFunc::Average);
    assert_eq!(pts[0].rows[0].source_col, "K");
}

#[test]
fn calculated_fields_round_trip() {
    let mut wb = workbook();
    {
        let p = wb.sheet_mut(1).unwrap().pivots.get_mut(0).unwrap();
        p.calculated_fields.push(PivotCalcField { name: "Price".into(), formula: "=Sales/Units".into() });
        p.calculated_fields.push(PivotCalcField { name: "Unit Bonus".into(), formula: "='Price'*0.1".into() });
        p.values.push(PivotValue { source_col: "Price".into(), name: "Sum of Price".into(), ..PivotValue::default() });
    }
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let c1 = String::from_utf8(parts.get("xl/pivotCache/pivotCacheDefinition1.xml").unwrap().clone()).unwrap();
    assert!(c1.contains("<cacheField name=\"Price\" numFmtId=\"0\" formula=\"Sales/Units\" databaseField=\"0\">"), "{c1}");
    assert!(c1.contains("formula=\"&apos;Price&apos;*0.1\"") || c1.contains("formula=\"'Price'*0.1\""), "{c1}");
    let t1 = String::from_utf8(parts.get("xl/pivotTables/pivotTable1.xml").unwrap().clone()).unwrap();
    assert!(t1.contains("<dataField name=\"Sum of Price\" fld=\"6\""), "{t1}");
    assert!(t1.contains("dataField=\"1\" dragToRow=\"0\""), "{t1}");
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.to_lowercase().contains("pivot")), "{:?}", report.warnings);
    let orig = &wb.sheet(1).unwrap().pivots;
    let read = &back.sheet(1).unwrap().pivots;
    for (a, b) in orig.iter().zip(read.iter()) {
        assert_eq!(a, b, "pivot {} differs", a.name);
    }
}

/// The `x` attributes of an axis list (`rowFields`/`colFields`) in a pivotTable part.
fn axis_xs(table: &str, tag: &str) -> Vec<i64> {
    let Some(start) = table.find(&format!("<{tag} ")) else { return vec![] };
    let end = table[start..].find(&format!("</{tag}>")).map_or(table.len(), |e| start + e);
    table[start..end].split("<field x=\"").skip(1).filter_map(|s| s.split('"').next()?.parse().ok()).collect()
}

#[test]
fn values_on_rows_round_trip() {
    let mut wb = workbook();
    {
        let p = wb.sheet_mut(1).unwrap().pivots.get_mut(1).unwrap();
        p.values_on_rows = true;
        p.values_position = Some(1);
    }
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let t2 = String::from_utf8(parts.get("xl/pivotTables/pivotTable2.xml").unwrap().clone()).unwrap();
    assert!(t2.contains(" dataOnRows=\"1\""), "{t2}");
    // Σ Values (-2) between the two row fields; Columns keep only Product.
    let rows = axis_xs(&t2, "rowFields");
    assert_eq!(rows.len(), 3, "{t2}");
    assert_eq!(rows[1], -2, "{t2}");
    assert!(!axis_xs(&t2, "colFields").contains(&-2), "{t2}");
    assert!(t2.contains("firstDataRow=\"2\" firstDataCol=\"3\""), "{t2}");
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.to_lowercase().contains("pivot")), "{:?}", report.warnings);
    assert_eq!(back.sheet(1).unwrap().pivots, wb.sheet(1).unwrap().pivots);

    // After the last row field.
    wb.sheet_mut(1).unwrap().pivots[1].values_position = None;
    let bytes = write_xlsx(&wb).unwrap();
    let t2 = String::from_utf8(unzip(&bytes).get("xl/pivotTables/pivotTable2.xml").unwrap().clone()).unwrap();
    assert_eq!(axis_xs(&t2, "rowFields").last(), Some(&-2), "{t2}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(1).unwrap().pivots, wb.sheet(1).unwrap().pivots);

    // Σ Values in Columns is written after the last column field, its only place in the layout.
    {
        let p = wb.sheet_mut(1).unwrap().pivots.get_mut(1).unwrap();
        p.values_on_rows = false;
        p.values_position = Some(0);
    }
    let bytes = write_xlsx(&wb).unwrap();
    let t2 = String::from_utf8(unzip(&bytes).get("xl/pivotTables/pivotTable2.xml").unwrap().clone()).unwrap();
    assert!(!t2.contains("dataOnRows"), "{t2}");
    assert_eq!(axis_xs(&t2, "colFields").last(), Some(&-2), "{t2}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(1).unwrap().pivots[1].values_position, None);
}

#[test]
fn values_ahead_of_column_fields_read_last() {
    let table = r#"<pivotTableDefinition name="ValuesFirst" cacheId="1"><location ref="C1:F4"/><pivotFields><pivotField axis="axisCol"><items><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><colFields count="2"><field x="-2"/><field x="0"/></colFields><dataFields><dataField name="Sum of V" fld="1"/><dataField name="Count of V" fld="1" subtotal="count"/></dataFields></pivotTableDefinition>"#;
    let (wb, _) = read_xlsx(&with_pivot(table, Some(GOOD_CACHE))).unwrap();
    let pt = &wb.sheet(0).unwrap().pivots[0];
    assert_eq!((pt.columns.len(), pt.values.len()), (1, 2));
    assert!(!pt.values_on_rows);
    assert_eq!(pt.values_position, None);
}

// As Excel saves them: `rowGrandTotals="0"` drops the Grand Total row under the report (leaving
// Excel's "On for Rows Only"), `colGrandTotals="0"` the Grand Total column at its right ("On for
// Columns Only"). LibreOffice's Pivot2_Row_Subtotals.xlsx (`rowGrandTotals="0"`) shows no Grand
// Total row; ClosedXML's TemplateWithTableSourcePivotTables.xlsx pivotTable5 (`colGrandTotals="0"`,
// row and column fields) shows a Grand Total row and no Grand Total column.
#[test]
fn grand_totals_read_in_excel_direction() {
    let table = |attr: &str| {
        format!(
            r#"<pivotTableDefinition name="GT" cacheId="1" {attr}><location ref="C1:F4"/><pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><rowFields><field x="0"/></rowFields><dataFields><dataField name="Sum of V" fld="1"/></dataFields></pivotTableDefinition>"#
        )
    };
    for (attr, rows, cols) in [("rowGrandTotals=\"0\"", true, false), ("colGrandTotals=\"0\"", false, true), ("", true, true)] {
        let (wb, _) = read_xlsx(&with_pivot(&table(attr), Some(GOOD_CACHE))).unwrap();
        let pt = &wb.sheet(0).unwrap().pivots[0];
        assert_eq!((pt.grand_totals_rows, pt.grand_totals_cols), (rows, cols), "{attr}");
    }
}

// `enableDrill="0"` is PivotTable Options › Data › Enable show details turned off; without the
// attribute the option is on.
#[test]
fn enable_show_details_reads_and_writes_enable_drill() {
    let table = |attr: &str| {
        format!(
            r#"<pivotTableDefinition name="Drill" cacheId="1" {attr}><location ref="C1:F4"/><pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><rowFields><field x="0"/></rowFields><dataFields><dataField name="Sum of V" fld="1"/></dataFields></pivotTableDefinition>"#
        )
    };
    for (attr, on) in [("enableDrill=\"0\"", false), ("enableDrill=\"1\"", true), ("", true)] {
        let (wb, _) = read_xlsx(&with_pivot(&table(attr), Some(GOOD_CACHE))).unwrap();
        assert_eq!(wb.sheet(0).unwrap().pivots[0].enable_show_details, on, "{attr}");
        let parts = unzip(&write_xlsx(&wb).unwrap());
        let written = String::from_utf8(parts.get("xl/pivotTables/pivotTable1.xml").unwrap().clone()).unwrap();
        assert_eq!(written.contains(" enableDrill=\"0\""), !on, "{attr}: {written}");
    }
}

#[test]
fn data_on_rows_with_one_value_field_reads_without_warning() {
    let table = r#"<pivotTableDefinition name="OneValue" cacheId="1" dataOnRows="1"><location ref="C1:D4"/><pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><rowFields><field x="0"/></rowFields><dataFields><dataField name="Total" fld="1"/></dataFields></pivotTableDefinition>"#;
    let (wb, report) = read_xlsx(&with_pivot(table, Some(GOOD_CACHE))).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("values on rows")), "{:?}", report.warnings);
    let pt = &wb.sheet(0).unwrap().pivots[0];
    assert!(pt.values_on_rows);
    assert_eq!(pt.values_position, None);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(back.sheet(0).unwrap().pivots[0].values_on_rows);
}

#[test]
fn manual_order_in_cache_order_is_kept() {
    // Not auto-sorted, items written in cache order: the order is still the file's, so a
    // refresh over reordered source rows keeps it.
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots[0].columns[0].item_order.clear();
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(1).unwrap().pivots[0].columns[0].item_order, ["A", "B", "C"]);
}

#[test]
fn early_dates_in_caches_count_real_days() {
    // Excel writes serial 1 as 1899-12-31 (ISO dates count days from 1899-12-30), not as the
    // 1900 calendar's 1900-01-01, which it reads as serial 2.
    let mut wb = workbook();
    let date_style = wb.sheet(0).unwrap().cells.get(at("C2")).unwrap().style;
    wb.sheet_mut(0).unwrap().cells.set(at("C2"), Cell { value: Value::number(1.5), formula: None, style: date_style, py: None });
    let parts = unzip(&write_xlsx(&wb).unwrap());
    let caches: String =
        parts.iter().filter(|(k, _)| k.contains("pivotCacheDefinition")).map(|(_, v)| String::from_utf8_lossy(v).into_owned()).collect();
    assert!(caches.contains("minDate=\"1899-12-31T12:00:00\"") && caches.contains("startDate=\"1899-12-31T00:00:00\""), "{caches}");
    assert!(!caches.contains("1900-01-01T"), "{caches}");
}

#[test]
fn location_without_field_headers_has_no_values_row() {
    let mut wb = workbook();
    {
        let p = &mut wb.sheet_mut(1).unwrap().pivots[2];
        p.values.push(PivotValue { source_col: "Sales".into(), name: "Sum of Sales".into(), ..PivotValue::default() });
        p.layout = PivotLayout::Outline;
        p.show_headers = false;
    }
    let parts = unzip(&write_xlsx(&wb).unwrap());
    let t3 = String::from_utf8(parts.get("xl/pivotTables/pivotTable3.xml").unwrap().clone()).unwrap();
    assert!(t3.contains("firstHeaderRow=\"1\" firstDataRow=\"1\""), "{t3}");
    wb.sheet_mut(1).unwrap().pivots[2].show_headers = true;
    let parts = unzip(&write_xlsx(&wb).unwrap());
    let t3 = String::from_utf8(parts.get("xl/pivotTables/pivotTable3.xml").unwrap().clone()).unwrap();
    assert!(t3.contains("firstHeaderRow=\"1\" firstDataRow=\"2\""), "{t3}");
    {
        let p = &mut wb.sheet_mut(1).unwrap().pivots[2];
        p.values.push(PivotValue { source_col: "Sales".into(), name: "Sum of Sales".into(), ..PivotValue::default() });
        p.show_values_row = false;
    }
    let parts = unzip(&write_xlsx(&wb).unwrap());
    let t3 = String::from_utf8(parts.get("xl/pivotTables/pivotTable3.xml").unwrap().clone()).unwrap();
    assert!(t3.contains("firstHeaderRow=\"0\""), "{t3}");
    assert!(t3.contains("<x14:pivotTableDefinition hideValuesRow=\"1\"/>"), "{t3}");
}

const SIMPLE_TABLE: &str = r#"<pivotTableDefinition name="Sales"><location ref="C1:D4"/><pivotFields><pivotField axis="axisRow"><items><item x="0"/><item t="default"/></items></pivotField><pivotField dataField="1"/></pivotFields><rowFields><field x="0"/></rowFields><dataFields><dataField name="Total" fld="1"/></dataFields></pivotTableDefinition>"#;
const CACHE_PART: &str = "xl/pivotCache/pivotCacheDefinition1.xml";

/// Rewrites the uncompressed size a zip's central directory declares for `name`.
fn declare_size(mut zip: Vec<u8>, name: &str, size: u32) -> Vec<u8> {
    let at = (0..zip.len().saturating_sub(46))
        .find(|&i| {
            zip[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] && {
                let len = u16::from_le_bytes([zip[i + 28], zip[i + 29]]) as usize;
                zip.get(i + 46..i + 46 + len) == Some(name.as_bytes())
            }
        })
        .unwrap();
    zip[at + 24..at + 28].copy_from_slice(&size.to_le_bytes());
    zip
}

#[test]
fn unreadable_pivot_cache_warns() {
    // Control: a readable cache gives a PivotTable and no cache warning.
    let (wb, report) = read_xlsx(&with_pivot(SIMPLE_TABLE, Some(GOOD_CACHE))).unwrap();
    assert_eq!(wb.sheet(0).unwrap().pivots.len(), 1, "{:?}", report.warnings);
    assert!(!report.warnings.iter().any(|w| w.contains("PivotTable's cache")), "{:?}", report.warnings);
    // Malformed and missing caches: the workbook opens and the lost PivotTable is reported.
    let lost = format!("a PivotTable's cache {CACHE_PART} is missing or could not be read; kept as plain cells");
    for cache in [Some("garbage <<<"), None] {
        let (wb, report) = read_xlsx(&with_pivot(SIMPLE_TABLE, cache)).unwrap();
        assert!(wb.sheet(0).unwrap().pivots.is_empty());
        assert!(report.warnings.contains(&lost), "{cache:?}: {:?}", report.warnings);
    }
}

#[test]
fn oversized_pivot_cache_warns() {
    // The cache declares more than the per-part limit, so it is refused before it inflates.
    let declared = u32::try_from(crate::package::MAX_PART + 1).unwrap();
    let bytes = declare_size(with_pivot(SIMPLE_TABLE, Some(GOOD_CACHE)), CACHE_PART, declared);
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().name, "Data");
    assert!(wb.sheet(0).unwrap().pivots.is_empty());
    let limit = crate::package::MAX_PART;
    let lost = format!("a PivotTable's cache is too large to read ({CACHE_PART} expands to more than {limit} bytes); kept as plain cells");
    assert!(report.warnings.contains(&lost), "{:?}", report.warnings);
}

/// A one-sheet-of-data workbook with a pivot on "Report": `columns` are the source columns
/// (header, values), `pt` the pivot built on `Data!A1:<last>`.
fn small_pivot(columns: &[(&str, Vec<Value>)], pt: impl FnOnce(&mut PivotTable)) -> HashMap<String, String> {
    let mut wb = Workbook::new();
    let data = wb.sheet_mut(0).unwrap();
    data.name = "Data".into();
    let mut rows = 0;
    for (c, (h, vals)) in columns.iter().enumerate() {
        data.cells.set(CellRef::new(0, c as u32), Cell::value(Value::text(*h)));
        for (r, v) in vals.iter().enumerate() {
            if *v != Value::Empty {
                data.cells.set(CellRef::new(r as u32 + 1, c as u32), Cell::value(v.clone()));
            }
        }
        rows = rows.max(vals.len());
    }
    let last = format!("{}{}", gridcraft_core::col_to_letters(columns.len() as u32 - 1), rows + 1);
    let mut table = PivotTable { id: 1, name: "PT".into(), source: format!("Data!$A$1:{last}"), anchor: at("A3"), ..PivotTable::default() };
    pt(&mut table);
    let mut report = Sheet::new("Report");
    report.pivots = vec![table];
    wb.sheets.push(Arc::new(report));
    unzip(&write_xlsx(&wb).unwrap()).into_iter().map(|(k, v)| (k, String::from_utf8_lossy(&v).into_owned())).collect()
}

/// #1447: two value fields and no column field: Excel 365 adds a `Values` row unless told not to.
#[test]
fn two_value_fields_without_columns_hide_the_values_row() {
    let cols = [
        ("Region", vec![Value::text("East"), Value::text("West"), Value::text("East")]),
        ("Sales", vec![Value::number(10.0), Value::number(20.0), Value::number(5.0)]),
        ("Qty", vec![Value::number(1.0), Value::number(2.0), Value::number(3.0)]),
    ];
    let value = |c: &str| PivotValue { source_col: c.into(), name: format!("Sum of {c}"), ..PivotValue::default() };
    let two = small_pivot(&cols, |p| {
        p.rows = vec![field("Region")];
        p.values = vec![value("Sales"), value("Qty")];
    });
    let t = &two["xl/pivotTables/pivotTable1.xml"];
    assert!(t.contains("firstHeaderRow=\"0\" firstDataRow=\"1\""), "{t}");
    assert!(!t.contains("hideValuesRow"), "{t}");
    let one = small_pivot(&cols, |p| {
        p.rows = vec![field("Region")];
        p.values = vec![value("Sales")];
    });
    let t = &one["xl/pivotTables/pivotTable1.xml"];
    assert!(t.contains("firstHeaderRow=\"1\""), "{t}");
    assert!(!t.contains("hideValuesRow"), "{t}");

    let mut wb = workbook();
    let p = &mut wb.sheet_mut(1).unwrap().pivots[2];
    p.filters.clear();
    p.layout = PivotLayout::Outline;
    p.show_headers = true;
    p.show_values_row = true;
    p.values.push(value("Sales"));
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(back.sheet(1).unwrap().pivots[2].show_values_row);
}

/// The "Trend" pivot of [`workbook`] with `values` as its value fields, written; returns the file
/// and its PivotTable and cache parts.
fn trend_with(values: impl FnOnce(&PivotValue) -> Vec<PivotValue>) -> (Vec<u8>, String, String) {
    let mut wb = workbook();
    let pivot = wb.sheet_mut(1).unwrap().pivots.iter_mut().find(|p| p.name == "Trend").unwrap();
    pivot.values = values(&pivot.values[0]);
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let (path, table) = parts
        .iter()
        .filter(|(path, _)| path.starts_with("xl/pivotTables/pivotTable"))
        .map(|(path, xml)| (path.clone(), String::from_utf8_lossy(xml).into_owned()))
        .find(|(_, xml)| xml.contains("name=\"Trend\""))
        .unwrap();
    let n = path.trim_start_matches("xl/pivotTables/pivotTable").trim_end_matches(".xml");
    let cache_part = format!("xl/pivotCache/pivotCacheDefinition{n}.xml");
    let rels = String::from_utf8_lossy(&parts[&format!("xl/pivotTables/_rels/pivotTable{n}.xml.rels")]).into_owned();
    assert!(rels.contains(&format!("../pivotCache/pivotCacheDefinition{n}.xml")), "{rels}");
    let cache = String::from_utf8_lossy(&parts[&cache_part]).into_owned();
    (bytes, table, cache)
}

fn trend_values(bytes: &[u8]) -> Vec<PivotValue> {
    let (back, report) = read_xlsx(bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.to_lowercase().contains("pivot")), "{:?}", report.warnings);
    back.sheet(1).unwrap().pivots.iter().find(|p| p.name == "Trend").unwrap().values.clone()
}

const GC_DISTINCT: &str = "<ext uri=\"{80B1AD42-0C9F-45FF-B109-D57FB0F4F0B4}\" xmlns:gc=\"https://github.com/storytold/gridcraft/ns/pivot/2026\"><gc:summary func=\"distinctCount\"/></ext>";

#[test]
fn distinct_count_round_trips_through_a_gridcraft_extension() {
    // The file format's subtotal list has no distinct count: the field is written as Count, which
    // other readers use, with a GridCraft marker that brings Distinct Count back on open.
    let (bytes, table, cache) =
        trend_with(|v| vec![PivotValue { func: PivotFunc::DistinctCount, name: "Distinct Count of Sales".into(), ..v.clone() }]);
    assert!(table.contains("<dataField name=\"Distinct Count of Sales\" fld=\"4\" subtotal=\"count\""), "{table}");
    assert!(table.contains(&format!("<extLst>{GC_DISTINCT}</extLst></dataField>")), "{table}");
    // A refresh on open would recompute it as Count in Excel, over the values on the sheet.
    assert!(!cache.contains("refreshOnLoad"), "{cache}");
    let values = trend_values(&bytes);
    assert_eq!((values[0].func, values[0].name.as_str()), (PivotFunc::DistinctCount, "Distinct Count of Sales"));
}

#[test]
fn rank_and_distinct_count_write_one_extension_each() {
    let (bytes, table, _) = trend_with(|v| {
        vec![PivotValue { func: PivotFunc::DistinctCount, show_as: PivotShowAs::Rank, name: "Rank of Distinct Sales".into(), ..v.clone() }]
    });
    let rank = "<ext uri=\"{E15A36E0-9728-4e99-A89B-3F7291B0FE68}\" xmlns:x14=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main\"><x14:dataField pivotShowAs=\"rankDescending\"/></ext>";
    assert!(table.contains(&format!("<extLst>{rank}{GC_DISTINCT}</extLst></dataField>")), "{table}");
    let values = trend_values(&bytes);
    assert_eq!((values[0].func, values[0].show_as), (PivotFunc::DistinctCount, PivotShowAs::Rank));
}

#[test]
fn count_without_the_gridcraft_extension_reads_as_count() {
    // Plain `subtotal="count"`, as Excel writes it, stays Count; so does a stale marker on a field
    // whose function was changed to something else.
    let (bytes, table, cache) = trend_with(|v| vec![PivotValue { func: PivotFunc::Count, name: "Count of Sales".into(), ..v.clone() }]);
    assert!(!table.contains("extLst"), "{table}");
    assert!(cache.contains("refreshOnLoad=\"1\""), "{cache}");
    assert_eq!(trend_values(&bytes)[0].func, PivotFunc::Count);
    let marked = |subtotal: &str| {
        format!(
            r#"<pivotTableDefinition name="P" cacheId="1"><location ref="C1:D4"/><pivotFields><pivotField/><pivotField dataField="1"/></pivotFields><dataFields><dataField name="V" fld="1"{subtotal}><extLst>{GC_DISTINCT}</extLst></dataField></dataFields></pivotTableDefinition>"#
        )
    };
    let func = |table: &str| read_xlsx(&with_pivot(table, Some(GOOD_CACHE))).unwrap().0.sheet(0).unwrap().pivots[0].values[0].func;
    assert_eq!(func(&marked(" subtotal=\"count\"")), PivotFunc::DistinctCount);
    assert_eq!(func(&marked("")), PivotFunc::Sum);
    assert_eq!(func(&marked(" subtotal=\"max\"")), PivotFunc::Max);
}

#[test]
fn ooxml_row_col_items_build_getpivotdata_index_and_page_field_uses_pivot_item_order() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let raw = write_xlsx(&wb).unwrap();
    let mut parts = unzip(&raw);

    // Patch pivotCacheDefinition1.xml to give field 0 (Region) sharedItems [East, West, North],
    // field 1 (Product) sharedItems [Apples, Pears], and field 2 (Type) sharedItems [Alpha, Beta].
    // Reorder field 2's `<pivotField><items>` so `item="1"` points to `x="0"` ("Alpha"), not
    // cache index 1 ("Beta"), and add `<rowItems>` + `<colItems>` matching a 2-row + grand report.
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:D4" sheet="Data"/></cacheSource>
  <cacheFields count="4">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Product" numFmtId="0"><sharedItems count="2"><s v="Apples"/><s v="Pears"/></sharedItems></cacheField>
    <cacheField name="Type" numFmtId="0"><sharedItems count="2"><s v="Alpha"/><s v="Beta"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;
    parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.as_bytes().to_vec());

    let pt_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A3:D7" firstHeaderRow="1" firstDataRow="2" firstDataCol="1" rowPageCount="1" colPageCount="1"/>
  <pivotFields count="4">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="1"/><item x="0"/><item t="default"/></items></pivotField>
    <pivotField axis="axisCol" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField axis="axisPage" showAll="0"><items count="3"><item x="1"/><item x="0"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3">
    <i><x v="0"/></i>
    <i><x v="1"/></i>
    <i t="grand"><x/></i>
  </rowItems>
  <colFields count="1"><field x="1"/></colFields>
  <colItems count="3">
    <i><x v="0"/></i>
    <i><x v="1"/></i>
    <i t="grand"><x/></i>
  </colItems>
  <pageFields count="1"><pageField fld="2" item="1" hier="-1"/></pageFields>
  <dataFields count="1"><dataField name="Sum of Sales" fld="3"/></dataFields>
</pivotTableDefinition>"#;
    parts.insert("xl/pivotTables/pivotTable1.xml".into(), pt_xml.as_bytes().to_vec());

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in &parts {
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, data).unwrap();
        }
        zw.finish().unwrap();
    }
    let (back, _) = read_xlsx(buf.get_ref()).unwrap();
    let pt = &back.sheet(1).unwrap().pivots[0];

    // `pageField item="1"` must resolve via `<pivotField><items>`[1] (`x="0"`) -> "Alpha", not "Beta".
    let page = pt.filters.iter().find(|f| f.source_col == "Type").expect("page filter read");
    assert_eq!(page.selected.as_deref(), Some(&["Alpha".to_string()][..]));

    // `<rowItems>`/`<colItems>` must populate `data_index` directly on read, with `above = 2`
    // (`rowPageCount=1` + spacer) + `firstDataRow=2` => data starts at relative row 4, col 1.
    assert!(pt.data_index.cells().is_some());
    let west_apples = pt.data_index.find("Sales", &[("Region", "West", false), ("Product", "Apples", false)]).unwrap();
    assert_eq!((west_apples.row, west_apples.col), (4, 1));
    let east_pears = pt.data_index.find("Sum of Sales", &[("Region", "East", false), ("Product", "Pears", false)]).unwrap();
    assert_eq!((east_pears.row, east_pears.col), (5, 2));
    let grand = pt.data_index.find("Sales", &[]).unwrap();
    assert_eq!((grand.row, grand.col), (6, 3));
}

fn repack_pivot1(wb: &Workbook, cache_xml: &str, pt_xml: &str) -> Workbook {
    let raw = write_xlsx(wb).unwrap();
    let mut parts = unzip(&raw);
    parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.as_bytes().to_vec());
    parts.insert("xl/pivotTables/pivotTable1.xml".into(), pt_xml.as_bytes().to_vec());
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in &parts {
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, data).unwrap();
        }
        zw.finish().unwrap();
    }
    read_xlsx(buf.get_ref()).unwrap().0
}

#[test]
fn ooxml_bottom_subtotals_and_subtotals_off_skip_blank_item_headers() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:C4" sheet="Data"/></cacheSource>
  <cacheFields count="3">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Product" numFmtId="0"><sharedItems count="2"><s v="Apples"/><s v="Pears"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;

    // 1. Outline/compact layout with bottom subtotals (`subtotalTop="0"`):
    // row 0: blank East header, row 1: East/Apples, row 2: East/Pears, row 3: East Total, row 4: Grand Total.
    // `firstDataRow="1"`, `rowPageCount="0"` => relative rows are 1, 2, 3, 4, 5.
    let bottom_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B6" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="3">
    <pivotField axis="axisRow" subtotalTop="0" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="2"><field x="0"/><field x="1"/></rowFields>
  <rowItems count="5">
    <i><x v="0"/></i>
    <i r="1"><x v="0"/></i>
    <i r="1"><x v="1"/></i>
    <i t="default"><x v="0"/></i>
    <i t="grand"><x/></i>
  </rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="2"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, bottom_pt);
    let pt = &back.sheet(1).unwrap().pivots[0];
    // East subtotal MUST point to rel row 4 (the `<i t="default">` row), NOT rel row 1 (blank header)!
    let east_sub = pt.data_index.find("Sales", &[("Region", "East", false)]).expect("East subtotal indexed");
    assert_eq!((east_sub.row, east_sub.col), (4, 1));
    let east_apples = pt.data_index.find("Sales", &[("Region", "East", false), ("Product", "Apples", false)]).unwrap();
    assert_eq!((east_apples.row, east_apples.col), (2, 1));
    let grand = pt.data_index.find("Sales", &[]).unwrap();
    assert_eq!((grand.row, grand.col), (5, 1));

    // 2. Outline/compact layout with subtotals OFF (`defaultSubtotal="0"`):
    // row 0: blank East header, row 1: East/Apples, row 2: East/Pears, row 3: Grand Total.
    let no_sub_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B5" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="3">
    <pivotField axis="axisRow" defaultSubtotal="0" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField axis="axisRow" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="2"><field x="0"/><field x="1"/></rowFields>
  <rowItems count="4">
    <i><x v="0"/></i>
    <i r="1"><x v="0"/></i>
    <i r="1"><x v="1"/></i>
    <i t="grand"><x/></i>
  </rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="2"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, no_sub_pt);
    let pt = &back.sheet(1).unwrap().pivots[0];
    // Subtotals are OFF, so Region=East MUST NOT resolve to the blank header row (returns #REF!).
    assert!(pt.data_index.find("Sales", &[("Region", "East", false)]).is_none());
    let east_pears = pt.data_index.find("Sales", &[("Region", "East", false), ("Product", "Pears", false)]).unwrap();
    assert_eq!((east_pears.row, east_pears.col), (3, 1));

    // 3. Top subtotals ON (default `subtotalTop="1"`, `defaultSubtotal="1"`):
    // row 0 holds the top subtotal for East, so Region=East MUST resolve to rel row 1.
    let top_sub_pt = bottom_pt.replace(r#"subtotalTop="0""#, "");
    let back = repack_pivot1(&wb, cache_xml, &top_sub_pt.replace("<i t=\"default\"><x v=\"0\"/></i>", ""));
    let pt = &back.sheet(1).unwrap().pivots[0];
    let east_top = pt.data_index.find("Sales", &[("Region", "East", false)]).expect("top subtotal indexed");
    assert_eq!((east_top.row, east_top.col), (1, 1));
}

#[test]
fn ooxml_values_on_rows_skips_blank_item_header_row() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:C4" sheet="Data"/></cacheSource>
  <cacheFields count="3">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
    <cacheField name="Units" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;
    // Σ Values (`-2`) on rows with 2 data fields (`Sum of Sales`, `Sum of Units`):
    // row 0 (rel 1): blank East header `<i><x v="0"/></i>` (no @i)
    // row 1 (rel 2): East / Sum of Sales `<i r="1"><x/></i>`
    // row 2 (rel 3): East / Sum of Units `<i r="1" i="1"><x v="1"/></i>`
    // row 3 (rel 4): Grand Total Sum of Sales
    // row 4 (rel 5): Grand Total Sum of Units
    let pt_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataOnRows="1" dataCaption="Values">
  <location ref="A1:B6" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="3">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="2"><field x="0"/><field x="-2"/></rowFields>
  <rowItems count="5">
    <i><x v="0"/></i>
    <i r="1"><x/></i>
    <i r="1" i="1"><x v="1"/></i>
    <i t="grand"><x/></i>
    <i t="grand" i="1"><x/></i>
  </rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="2">
    <dataField name="Sum of Sales" fld="1"/>
    <dataField name="Sum of Units" fld="2"/>
  </dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, pt_xml);
    let pt = &back.sheet(1).unwrap().pivots[0];
    let east_sales = pt.data_index.find("Sum of Sales", &[("Region", "East", false)]).expect("Sales row indexed");
    assert_eq!((east_sales.row, east_sales.col), (2, 1), "must hit Sales row (2), not blank header (1)");
    let east_units = pt.data_index.find("Sum of Units", &[("Region", "East", false)]).expect("Units row indexed");
    assert_eq!((east_units.row, east_units.col), (3, 1));
    assert_eq!(pt.data_index.find("Sum of Sales", &[]).unwrap().row, 4);
    assert_eq!(pt.data_index.find("Sum of Units", &[]).unwrap().row, 5);
}

#[test]
fn ooxml_renamed_axis_field_uses_source_col_and_malformed_items_bail_out() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource>
  <cacheFields count="2">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;

    // M1: `<pivotField name="Area">` on cache field `"Region"` must store `"Region"` in `PivotItemRef.field`.
    let renamed_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField name="Area" axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, renamed_pt);
    let pt = &back.sheet(1).unwrap().pivots[0];
    assert_eq!(pt.rows[0].source_col, "Region");
    assert_eq!(pt.rows[0].caption.as_deref(), Some("Area"));
    let east = pt.data_index.find("Sales", &[("Region", "East", false)]).expect("source name resolves");
    assert_eq!((east.row, east.col), (1, 1));

    // M2: axis `<pivotField>` without `<items>` must return `None` from `build_ooxml_data_index`
    // instead of pushing truncated keys that claim the Grand Total!
    let missing_items_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"/>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, missing_items_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "missing <items> must bail out cleanly");

    // L6: invalid/negative `@r` or `@v` must bail out cleanly rather than defaulting to 0.
    let bad_attr_pt = renamed_pt.replace("<i><x v=\"0\"/></i>", "<i r=\"-1\"><x v=\"0\"/></i>");
    let back = repack_pivot1(&wb, cache_xml, &bad_attr_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "negative @r must bail out");

    // L1: a skipped `<dataField>` (OOB `fld="99"`) must NOT fall back to `pt_values[0]`.
    let skipped_df_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:C4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colFields count="1"><field x="-2"/></colFields>
  <colItems count="2"><i><x v="0"/></i><i i="1"><x v="1"/></i></colItems>
  <dataFields count="2">
    <dataField name="Broken" fld="99"/>
    <dataField name="Sum of Sales" fld="1"/>
  </dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, skipped_df_pt);
    let pt = &back.sheet(1).unwrap().pivots[0];
    let east = pt.data_index.find("Sum of Sales", &[("Region", "East", false)]).expect("valid dataField indexed");
    assert_eq!((east.row, east.col), (1, 2), "skipped dataField 0 must not claim col 1");
}

#[test]
fn ooxml_hostile_axis_bounds_and_iteration_cap_bail_out() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource>
  <cacheFields count="2">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;

    // 1. Multiple `-2` (Values) fields on one axis (both 2 and 200 entries) must bail out cleanly.
    for dup_values in [2usize, 200] {
        let fields: String = (0..dup_values).map(|_| r#"<field x="-2"/>"#).collect();
        let xs: String = (0..dup_values).map(|_| r#"<x v="0"/>"#).collect();
        let pt_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="{dup_values}">{fields}</rowFields>
  <rowItems count="1"><i>{xs}</i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#
        );
        let back = repack_pivot1(&wb, cache_xml, &pt_xml);
        assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "{dup_values} Values (-2) fields on one axis must bail out");
    }

    // 2. Axis deeper than MAX_AXIS_FIELDS (33 single-axis, or 17 row + 16 col = 33 combined) must bail out cleanly.
    let deep_cache_fields: String =
        (0..34).map(|i| format!(r#"<cacheField name="F{i}" numFmtId="0"><sharedItems count="1"><s v="A"/></sharedItems></cacheField>"#)).collect();
    let deep_cache_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:AH2" sheet="Data"/></cacheSource>
  <cacheFields count="34">{deep_cache_fields}</cacheFields>
</pivotCacheDefinition>"#
    );
    let deep_pfs: String = (0..33)
        .map(|_| r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#)
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .collect();
    let deep_row_fields: String = (0..33).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let deep_xs: String = (0..33).map(|_| r#"<x v="0"/>"#).collect();
    let deep_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="34">{deep_pfs}</pivotFields>
  <rowFields count="33">{deep_row_fields}</rowFields>
  <rowItems count="1"><i>{deep_xs}</i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of F33" fld="33"/></dataFields>
</pivotTableDefinition>"#
    );
    let back = repack_pivot1(&wb, &deep_cache_xml, &deep_pt_xml);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "33-deep single axis must bail out");

    let split_pfs: String = (0..17)
        .map(|_| r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#)
        .chain((0..16).map(|_| r#"<pivotField axis="axisCol" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#))
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .collect();
    let split_rows: String = (0..17).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let split_cols: String = (17..33).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let split_rxs: String = (0..17).map(|_| r#"<x v="0"/>"#).collect();
    let split_cxs: String = (0..16).map(|_| r#"<x v="0"/>"#).collect();
    let split_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="34">{split_pfs}</pivotFields>
  <rowFields count="17">{split_rows}</rowFields>
  <rowItems count="1"><i>{split_rxs}</i></rowItems>
  <colFields count="16">{split_cols}</colFields>
  <colItems count="1"><i>{split_cxs}</i></colItems>
  <dataFields count="1"><dataField name="Sum of F33" fld="33"/></dataFields>
</pivotTableDefinition>"#
    );
    let back = repack_pivot1(&wb, &deep_cache_xml, &split_pt_xml);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "17 row + 16 col = 33 combined axis fields must bail out");
    // 3. Large duplicate + skipped row * col product (> 1_000_000 iterations) must bail out cleanly.
    let dup_rows: String = (0..1005).map(|_| r#"<i t="grand"><x/></i>"#).collect();
    let dup_and_blank_cols: String = (0..1005).map(|i| if i % 2 == 0 { r#"<i t="grand"><x/></i>"# } else { r#"<i t="blank"/>"# }).collect();
    let huge_product_pt = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:AMQ1050" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="1005">{dup_rows}</rowItems>
  <colItems count="1005">{dup_and_blank_cols}</colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#
    );
    let back = repack_pivot1(&wb, cache_xml, &huge_product_pt);
    assert!(
        back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(),
        "row * col iteration cap must bail out even with duplicate keys and blank cols"
    );
}

#[test]
fn ooxml_missing_axis_items_with_non_empty_axis_fields_bails_out() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource>
  <cacheFields count="2">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;

    // Non-empty `<rowFields>` with absent `<rowItems>` must return `None` instead of synthesizing an empty row entry.
    let missing_row_items = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, missing_row_items);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "missing <rowItems> with <rowFields> must bail out");

    // Symmetric check: non-empty `<colFields>` with absent `<colItems>` must also return `None`.
    let missing_col_items = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:C3" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisCol" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowItems count="1"><i><x/></i></rowItems>
  <colFields count="1"><field x="0"/></colFields>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, missing_col_items);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "missing <colItems> with <colFields> must bail out");
}

#[test]
fn ooxml_range_pr_group_is_not_stripped_as_manual_parent() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource>
  <cacheFields count="2">
    <cacheField name="Amount" numFmtId="0">
      <sharedItems containsNumber="1" count="2"><n v="5"/><n v="15"/></sharedItems>
      <fieldGroup base="0">
        <rangePr startNum="0" endNum="20" groupInterval="10"/>
        <groupItems count="2"><s v="0-10"/><s v="10-20"/></groupItems>
      </fieldGroup>
    </cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;
    let pt_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" colGrandTotals="0" rowGrandTotals="0" dataCaption="Values">
  <location ref="A1:B3" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="2"><i><x v="0"/></i><i><x v="1"/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, pt_xml);
    let pt = &back.sheet(1).unwrap().pivots[0];
    assert!(pt.data_index.find("Sum of Sales", &[]).is_none(), "rangePr field must not be stripped into a fake Grand Total");
    let bucket0 = pt.data_index.find("Sum of Sales", &[("Amount", "0-10", false)]).expect("first bucket indexed");
    assert_eq!((bucket0.row, bucket0.col), (1, 1));
}

#[test]
fn ooxml_hostile_long_strings_byte_budget_bails_out() {
    // 32 axis fields (16 row + 16 col) with 64 KiB field names and 64 KiB `<s v>` item strings
    // across 1,000 × 1,000 axis items: without `MAX_TABLE_WORK_BYTES` (128 MiB),
    // `MAX_RETAINED_INDEX_BYTES` (64 MiB), and `MAX_WORKBOOK_WORK_BYTES` (2 GiB), this would try
    // to allocate terabytes; with the work/retained budgets it bails out cleanly and rapidly.
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let long_name = "F".repeat(64 * 1024);
    let long_val = "V".repeat(64 * 1024);
    let cache_fields: String = (0..32)
        .map(|i| {
            format!(r#"<cacheField name="{i}_{long_name}" numFmtId="0"><sharedItems count="1"><s v="{i}_{long_val}"/></sharedItems></cacheField>"#)
        })
        .chain(std::iter::once(r#"<cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
        .collect();
    let cache_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:AG2" sheet="Data"/></cacheSource>
  <cacheFields count="33">{cache_fields}</cacheFields>
</pivotCacheDefinition>"#
    );
    let pfs: String = (0..16)
        .map(|_| r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#)
        .chain((0..16).map(|_| r#"<pivotField axis="axisCol" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#))
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .collect();
    let row_fields: String = (0..16).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let col_fields: String = (16..32).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let xs16: String = (0..16).map(|_| r#"<x v="0"/>"#).collect();
    let one_i = format!("<i>{xs16}</i>");
    let row_items = one_i.repeat(1000);
    let col_items = one_i.repeat(1000);
    let pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:AMQ1050" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{pfs}</pivotFields>
  <rowFields count="16">{row_fields}</rowFields>
  <rowItems count="1000">{row_items}</rowItems>
  <colFields count="16">{col_fields}</colFields>
  <colItems count="1000">{col_items}</colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="32"/></dataFields>
</pivotTableDefinition>"#
    );
    let back = repack_pivot1(&wb, &cache_xml, &pt_xml);
    assert!(
        back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(),
        "hostile 64 KiB names/items across 1000x1000 grid must bail out under MAX_TABLE_WORK_BYTES"
    );
}

#[test]
fn ooxml_numfmt_formats_date_and_number_item_labels_and_keeps_raw_serial() {
    let fmt_opts = crate::ReadOptions {
        format_value: Some(|v, code, sys| gridcraft_numfmt::format_value(v, &gridcraft_numfmt::NumberFormat::parse(code), sys).text),
        ..Default::default()
    };
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let raw = write_xlsx(&wb).unwrap();
    let mut parts = unzip(&raw);

    // Add custom numFmtId="164" (`yyyy-mm-dd`) in styles.xml to test custom `<numFmts>` override on `<pivotField>`.
    let styles_xml = String::from_utf8(parts["xl/styles.xml"].clone()).unwrap();
    let patched_styles = if styles_xml.contains("<numFmts") {
        styles_xml.replacen("</numFmts>", r#"<numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts>"#, 1)
    } else {
        styles_xml.replacen("<fonts", r#"<numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><fonts"#, 1)
    };
    parts.insert("xl/styles.xml".into(), patched_styles.into_bytes());

    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:E2" sheet="Data"/></cacheSource>
  <cacheFields count="5">
    <cacheField name="Day" numFmtId="14"><sharedItems containsDate="1" count="1"><d v="2023-03-15T00:00:00"/></sharedItems></cacheField>
    <cacheField name="Amount" numFmtId="3"><sharedItems containsNumber="1" count="1"><n v="1000"/></sharedItems></cacheField>
    <cacheField name="Rate" numFmtId="9"><sharedItems containsNumber="1" count="1"><n v="0.05"/></sharedItems></cacheField>
    <cacheField name="CustomDay" numFmtId="14"><sharedItems containsDate="1" count="1"><d v="2023-03-15T00:00:00"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;
    let pt_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:E4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="5">
    <pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>
    <pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>
    <pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>
    <pivotField axis="axisRow" numFmtId="164" showAll="0"><items count="1"><item x="0"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="4"><field x="0"/><field x="1"/><field x="2"/><field x="3"/></rowFields>
  <rowItems count="1"><i><x v="0"/><x v="0"/><x v="0"/><x v="0"/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="4"/></dataFields>
</pivotTableDefinition>"#;
    parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.as_bytes().to_vec());
    parts.insert("xl/pivotTables/pivotTable1.xml".into(), pt_xml.as_bytes().to_vec());

    let zip_parts = |parts: &HashMap<String, Vec<u8>>| -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in parts {
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, data).unwrap();
        }
        zw.finish().unwrap();
        buf.into_inner()
    };

    // 1900 date system: serial for 2023-03-15 is 45000.
    let (back_1900, _) = crate::read_xlsx_with(&zip_parts(&parts), fmt_opts).unwrap();
    let pt_1900 = &back_1900.sheet(1).unwrap().pivots[0];
    let cell = pt_1900
        .data_index
        .find("Sales", &[("Day", "3/15/2023", false), ("Amount", "1,000", false), ("Rate", "5%", false), ("CustomDay", "2023-03-15", false)])
        .expect("displayed date/number labels indexed");
    assert_eq!(cell.items[0].value, Value::Number(45000.0));
    assert_eq!(cell.items[1].value, Value::Number(1000.0));
    assert_eq!(cell.items[2].value, Value::Number(0.05));
    assert_eq!(cell.items[3].value, Value::Number(45000.0));

    // 1904 date system (`date1904="1"`): serial for 2023-03-15 is 43538, still formats as "3/15/2023".
    let wb_xml = String::from_utf8(parts["xl/workbook.xml"].clone()).unwrap();
    let wb_1904 = if wb_xml.contains("<workbookPr") {
        wb_xml.replacen("<workbookPr", r#"<workbookPr date1904="1""#, 1)
    } else {
        wb_xml.replacen("<sheets", r#"<workbookPr date1904="1"/><sheets"#, 1)
    };
    parts.insert("xl/workbook.xml".into(), wb_1904.into_bytes());
    let (back_1904, _) = crate::read_xlsx_with(&zip_parts(&parts), fmt_opts).unwrap();
    let pt_1904 = &back_1904.sheet(1).unwrap().pivots[0];
    let cell_1904 = pt_1904
        .data_index
        .find("Sales", &[("Day", "3/15/2023", false), ("Amount", "1,000", false), ("Rate", "5%", false), ("CustomDay", "2023-03-15", false)])
        .expect("date1904 displayed labels indexed");
    assert_eq!(cell_1904.items[0].value, Value::Number(43538.0));
    assert_eq!(cell_1904.items[3].value, Value::Number(43538.0));
}

#[test]
fn ooxml_hostile_short_string_struct_budget_and_rel_target_dedup() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);

    let row_items_shared: String = (0..200).map(|i| format!(r#"<s v="r{i}"/>"#)).collect();
    let col_items_shared: String = (0..200).map(|i| format!(r#"<s v="c{i}"/>"#)).collect();
    let cache_fields: String = (0..32)
        .map(|i| match i {
            0 => format!(r#"<cacheField name="f0" numFmtId="0"><sharedItems count="200">{row_items_shared}</sharedItems></cacheField>"#),
            16 => format!(r#"<cacheField name="f16" numFmtId="0"><sharedItems count="200">{col_items_shared}</sharedItems></cacheField>"#),
            _ => format!(r#"<cacheField name="f{i}" numFmtId="0"><sharedItems count="1"><s v="x"/></sharedItems></cacheField>"#),
        })
        .chain(std::iter::once(r#"<cacheField name="S" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
        .collect();
    let cache_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:AG2" sheet="Data"/></cacheSource>
  <cacheFields count="33">{cache_fields}</cacheFields>
</pivotCacheDefinition>"#
    );
    let pf_items_200: String = (0..200).map(|i| format!(r#"<item x="{i}"/>"#)).collect();
    let pfs: String = (0..16)
        .map(|i| {
            if i == 0 {
                format!(r#"<pivotField axis="axisRow" showAll="0"><items count="200">{pf_items_200}</items></pivotField>"#)
            } else {
                r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#.to_string()
            }
        })
        .chain((16..32).map(|i| {
            if i == 16 {
                format!(r#"<pivotField axis="axisCol" showAll="0"><items count="200">{pf_items_200}</items></pivotField>"#)
            } else {
                r#"<pivotField axis="axisCol" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#.to_string()
            }
        }))
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#.to_string()))
        .collect();
    let row_fields: String = (0..16).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let col_fields: String = (16..32).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let tail15: String = (0..15).map(|_| r#"<x v="0"/>"#).collect();
    let distinct_is = |n: usize| -> String { (0..n).map(|i| format!(r#"<i><x v="{i}"/>{tail15}</i>"#)).collect() };

    // 1. Short-string 16x16 grid (200x200 = 40,000 distinct cells): string+key bytes stay ~16 MiB,
    //    but charging `size_of::<PivotItemRef>()` + `size_of::<PivotDataCell>()` (~95.5 MiB retained)
    //    trips the 64 MiB retained cap and emits a warning.
    let dense_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="DensePivot" cacheId="1" dataCaption="Values">
  <location ref="A1:HV250" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{pfs}</pivotFields>
  <rowFields count="16">{row_fields}</rowFields>
  <rowItems count="200">{}</rowItems>
  <colFields count="16">{col_fields}</colFields>
  <colItems count="200">{}</colItems>
  <dataFields count="1"><dataField name="S" fld="32"/></dataFields>
</pivotTableDefinition>"#,
        distinct_is(200),
        distinct_is(200)
    );
    let raw = write_xlsx(&wb).unwrap();
    let mut dense_parts = unzip(&raw);
    dense_parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.as_bytes().to_vec());
    dense_parts.insert("xl/pivotTables/pivotTable1.xml".into(), dense_pt_xml.into_bytes());
    let zip_parts = |parts: &HashMap<String, Vec<u8>>| -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in parts {
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, data).unwrap();
        }
        zw.finish().unwrap();
        buf.into_inner()
    };
    let (back, report) = read_xlsx(&zip_parts(&dense_parts)).unwrap();
    assert!(
        back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(),
        "dense short-string 16x16 grid must bail out under struct-size + string MAX_RETAINED_INDEX_BYTES"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("DensePivot") && w.contains("retained budget")),
        "retained-budget bailout must emit a warning, got: {:?}",
        report.warnings
    );

    // 2. Duplicate pivotTable rels pointing at the same `pivotTable1.xml` part are deduplicated
    //    in `read_pivots` so the part is parsed and indexed only once.
    let medium_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:AZ50" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{pfs}</pivotFields>
  <rowFields count="16">{row_fields}</rowFields>
  <rowItems count="25">{}</rowItems>
  <colFields count="16">{col_fields}</colFields>
  <colItems count="25">{}</colItems>
  <dataFields count="1"><dataField name="S" fld="32"/></dataFields>
</pivotTableDefinition>"#,
        distinct_is(25),
        distinct_is(25)
    );
    let mut dup_parts = unzip(&raw);
    dup_parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.as_bytes().to_vec());
    dup_parts.insert("xl/pivotTables/pivotTable1.xml".into(), medium_pt_xml.into_bytes());
    let rels_key = dup_parts
        .iter()
        .find(|(k, v)| k.starts_with("xl/worksheets/_rels/") && String::from_utf8_lossy(v).contains("pivotTable1.xml"))
        .map(|(k, _)| k.clone())
        .expect("sheet rels referencing pivotTable1.xml");
    let rels_xml = String::from_utf8(dup_parts[&rels_key].clone()).unwrap();
    let extra_rels: String = (2..=30)
        .map(|k| {
            let target = if k % 2 == 0 { "/xl/pivotTables/pivotTable1.xml" } else { "../pivotTables/pivotTable1.xml" };
            format!(
                r#"<Relationship Id="rIdDup{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="{target}"/>"#
            )
        })
        .collect();
    dup_parts.insert(rels_key, rels_xml.replacen("</Relationships>", &format!("{extra_rels}</Relationships>"), 1).into_bytes());
    let (dedup_wb, dedup_report) = read_xlsx(&zip_parts(&dup_parts)).unwrap();
    let pivots = &dedup_wb.sheet(1).unwrap().pivots;
    assert_eq!(pivots.len(), 1, "duplicate rel.target entries must be deduplicated in read_pivots");
    assert!(pivots[0].data_index.cells().is_some(), "deduplicated pivot is indexed");
    assert!(dedup_report.warnings.is_empty(), "deduplicated pivot emits no warnings, got: {:?}", dedup_report.warnings);
}

struct OoxmlBudgetFixture {
    base_raw: Vec<u8>,
    cache_xml: String,
    pfs: String,
    row_fields: String,
    col_fields: String,
    tail15: String,
}

impl OoxmlBudgetFixture {
    fn new() -> Self {
        let mut wb = workbook();
        wb.sheet_mut(1).unwrap().pivots.truncate(1);
        let row_items_shared: String = (0..200).map(|i| format!(r#"<s v="r{i}"/>"#)).collect();
        let col_items_shared: String = (0..200).map(|i| format!(r#"<s v="c{i}"/>"#)).collect();
        let cache_fields: String = (0..32)
            .map(|i| match i {
                0 => format!(r#"<cacheField name="f0" numFmtId="0"><sharedItems count="200">{row_items_shared}</sharedItems></cacheField>"#),
                16 => format!(r#"<cacheField name="f16" numFmtId="0"><sharedItems count="200">{col_items_shared}</sharedItems></cacheField>"#),
                _ => format!(r#"<cacheField name="f{i}" numFmtId="0"><sharedItems count="1"><s v="x"/></sharedItems></cacheField>"#),
            })
            .chain(std::iter::once(r#"<cacheField name="S" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
            .collect();
        let cache_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:AG2" sheet="Data"/></cacheSource>
  <cacheFields count="33">{cache_fields}</cacheFields>
</pivotCacheDefinition>"#
        );
        let pf_items_200: String = (0..200).map(|i| format!(r#"<item x="{i}"/>"#)).collect();
        let pfs: String = (0..16)
            .map(|i| {
                if i == 0 {
                    format!(r#"<pivotField axis="axisRow" showAll="0"><items count="200">{pf_items_200}</items></pivotField>"#)
                } else {
                    r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#.to_string()
                }
            })
            .chain((16..32).map(|i| {
                if i == 16 {
                    format!(r#"<pivotField axis="axisCol" showAll="0"><items count="200">{pf_items_200}</items></pivotField>"#)
                } else {
                    r#"<pivotField axis="axisCol" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#.to_string()
                }
            }))
            .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#.to_string()))
            .collect();
        let row_fields: String = (0..16).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
        let col_fields: String = (16..32).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
        let tail15: String = (0..15).map(|_| r#"<x v="0"/>"#).collect();
        let base_raw = write_xlsx(&wb).unwrap();
        Self { base_raw, cache_xml, pfs, row_fields, col_fields, tail15 }
    }

    fn distinct_is(&self, n: usize) -> String {
        (0..n).map(|i| format!(r#"<i><x v="{i}"/>{}</i>"#, self.tail15)).collect()
    }

    fn make_pt(&self, name: &str, n: usize, loc: &str, items_xml: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="{name}" cacheId="1" dataCaption="Values">
  <location ref="{loc}" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{}</pivotFields>
  <rowFields count="16">{}</rowFields>
  <rowItems count="{n}">{items_xml}</rowItems>
  <colFields count="16">{}</colFields>
  <colItems count="{n}">{items_xml}</colItems>
  <dataFields count="1"><dataField name="S" fld="32"/></dataFields>
</pivotTableDefinition>"#,
            self.pfs, self.row_fields, self.col_fields
        )
    }

    fn build_multi(&self, tables: Vec<(String, String)>) -> Vec<u8> {
        let mut parts = unzip(&self.base_raw);
        parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), self.cache_xml.as_bytes().to_vec());
        let rels_key = parts
            .iter()
            .find(|(k, v)| k.starts_with("xl/worksheets/_rels/") && String::from_utf8_lossy(v).contains("pivotTable1.xml"))
            .map(|(k, _)| k.clone())
            .expect("sheet rels referencing pivotTable1.xml");
        let pt1_rels = parts["xl/pivotTables/_rels/pivotTable1.xml.rels"].clone();
        let mut extra_rels = String::new();
        for (idx, (_, xml)) in tables.into_iter().enumerate() {
            let n = idx + 1;
            parts.insert(format!("xl/pivotTables/pivotTable{n}.xml"), xml.into_bytes());
            if n > 1 {
                parts.insert(format!("xl/pivotTables/_rels/pivotTable{n}.xml.rels"), pt1_rels.clone());
                extra_rels.push_str(&format!(
                    r#"<Relationship Id="rIdPt{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable{n}.xml"/>"#
                ));
            }
        }
        let rels_xml = String::from_utf8(parts[&rels_key].clone()).unwrap();
        parts.insert(rels_key, rels_xml.replacen("</Relationships>", &format!("{extra_rels}</Relationships>"), 1).into_bytes());
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in &parts {
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, data).unwrap();
        }
        zw.finish().unwrap();
        buf.into_inner()
    }
}

#[test]
fn ooxml_split_retained_and_work_budgets_rollback_failed_table_and_warn() {
    let fx = OoxmlBudgetFixture::new();

    // A. Four legitimate medium pivots (~1.5 MiB retained each), then one oversized pivot
    //    (200x200 distinct = ~95.5 MiB retained -> trips 64 MiB cap and rolls back retained),
    //    followed by two more legitimate medium pivots: all six legitimate pivots are indexed
    //    and only the oversized pivot emits a warning.
    let med_items = fx.distinct_is(25);
    let big_items = fx.distinct_is(200);
    let mut tables = Vec::new();
    for i in 1..=4 {
        let name = format!("MedBefore{i}");
        tables.push((name.clone(), fx.make_pt(&name, 25, "A1:AZ50", &med_items)));
    }
    tables.push(("OversizedPivot".into(), fx.make_pt("OversizedPivot", 200, "A1:HV250", &big_items)));
    for i in 1..=2 {
        let name = format!("MedAfter{i}");
        tables.push((name.clone(), fx.make_pt(&name, 25, "A1:AZ50", &med_items)));
    }
    let (wb_a, report_a) = read_xlsx(&fx.build_multi(tables)).unwrap();
    let pivots_a = &wb_a.sheet(1).unwrap().pivots;
    assert_eq!(pivots_a.len(), 7);
    for idx in [0, 1, 2, 3, 5, 6] {
        assert!(
            pivots_a[idx].data_index.cells().is_some(),
            "legitimate pivot {} ({}) must be indexed even after an oversized pivot failed",
            idx,
            pivots_a[idx].name
        );
    }
    assert!(pivots_a[4].data_index.cells().is_none(), "oversized pivot must bail out");
    assert_eq!(report_a.warnings.len(), 1, "only the oversized pivot should warn, got: {:?}", report_a.warnings);
    assert!(report_a.warnings[0].contains("OversizedPivot"), "warning should name OversizedPivot: {:?}", report_a.warnings);

    // B. Many distinct medium pivots exhausting the shared 64 MiB retained budget:
    //    46 tables x ~1.5 MiB retained (~69 MiB) -> early pivots indexed, last pivot bails out with warning.
    let many_tables: Vec<(String, String)> = (1..=46)
        .map(|i| {
            let name = format!("Med{i}");
            (name.clone(), fx.make_pt(&name, 25, "A1:AZ50", &med_items))
        })
        .collect();
    let (wb_b, report_b) = read_xlsx(&fx.build_multi(many_tables)).unwrap();
    let pivots_b = &wb_b.sheet(1).unwrap().pivots;
    assert_eq!(pivots_b.len(), 46);
    assert!(pivots_b[0].data_index.cells().is_some(), "early pivots fit in 64 MiB retained budget");
    assert!(pivots_b[45].data_index.cells().is_none(), "later distinct pivots must bail out past 64 MiB retained budget");
    assert!(
        report_b.warnings.iter().any(|w| w.contains("Med46") && w.contains("retained budget")),
        "over-budget pivot must emit warning, got: {:?}",
        report_b.warnings
    );

    // C. Duplicate-key work bombing (250x250 = 62,500 identical 32-field cells -> 1 retained cell,
    //    but ~170 MiB per-table work) still bails out under the 128 MiB per-table work budget with
    //    a warning, and because work bytes are per-table, later pivots (`MedAfter1`/`MedAfter2`)
    //    are unaffected.
    let dup_i = format!(r#"<i><x v="0"/>{}</i>"#, fx.tail15);
    let dup_items = dup_i.repeat(250);
    let work_bomb = vec![
        ("WorkBomb".into(), fx.make_pt("WorkBomb", 250, "A1:JZ300", &dup_items)),
        ("MedAfter1".into(), fx.make_pt("MedAfter1", 25, "A1:AZ50", &med_items)),
        ("MedAfter2".into(), fx.make_pt("MedAfter2", 25, "A1:AZ50", &med_items)),
    ];
    let (wb_c, report_c) = read_xlsx(&fx.build_multi(work_bomb)).unwrap();
    let pivots_c = &wb_c.sheet(1).unwrap().pivots;
    assert_eq!(pivots_c.len(), 3);
    assert!(pivots_c[0].data_index.cells().is_none(), "duplicate-key bombing must trip 128 MiB per-table work budget");
    assert!(pivots_c[1].data_index.cells().is_some(), "MedAfter1 must stay indexed after WorkBomb");
    assert!(pivots_c[2].data_index.cells().is_some(), "MedAfter2 must stay indexed after WorkBomb");
    assert_eq!(report_c.warnings.len(), 1, "only WorkBomb should warn, got: {:?}", report_c.warnings);
    assert!(
        report_c.warnings[0].contains("WorkBomb") && report_c.warnings[0].contains("work budget"),
        "work-budget bailout must emit warning, got: {:?}",
        report_c.warnings
    );

    // D. Two oversized pivots placed FIRST before two legitimate medium pivots: each oversized
    //    pivot bails on the 64 MiB retained cap and rolls back its retained bytes, while its
    //    per-table work budget is discarded on bailout, so neither starves `MedAfter1`/`MedAfter2`.
    let oversized_first = vec![
        ("OversizedFirst1".into(), fx.make_pt("OversizedFirst1", 200, "A1:HV250", &big_items)),
        ("OversizedFirst2".into(), fx.make_pt("OversizedFirst2", 200, "A1:HV250", &big_items)),
        ("MedAfter1".into(), fx.make_pt("MedAfter1", 25, "A1:AZ50", &med_items)),
        ("MedAfter2".into(), fx.make_pt("MedAfter2", 25, "A1:AZ50", &med_items)),
    ];
    let (wb_d, report_d) = read_xlsx(&fx.build_multi(oversized_first)).unwrap();
    let pivots_d = &wb_d.sheet(1).unwrap().pivots;
    assert_eq!(pivots_d.len(), 4);
    assert!(pivots_d[0].data_index.cells().is_none(), "OversizedFirst1 must bail out");
    assert!(pivots_d[1].data_index.cells().is_none(), "OversizedFirst2 must bail out");
    assert!(pivots_d[2].data_index.cells().is_some(), "MedAfter1 must stay indexed after two oversized pivots");
    assert!(pivots_d[3].data_index.cells().is_some(), "MedAfter2 must stay indexed after two oversized pivots");
    assert_eq!(report_d.warnings.len(), 2, "both oversized pivots must warn, got: {:?}", report_d.warnings);
    assert!(report_d.warnings[0].contains("OversizedFirst1") && report_d.warnings[0].contains("retained budget"));
    assert!(report_d.warnings[1].contains("OversizedFirst2") && report_d.warnings[1].contains("retained budget"));
}

#[test]
fn ooxml_tall_pivot_work_budget_and_workbook_iteration_cap() {
    let fx = OoxmlBudgetFixture::new();
    let med_items = fx.distinct_is(25);

    // E. Cumulative workbook iteration cap (`MAX_WORKBOOK_INDEX_ITERATIONS = 16_000_000`) counts
    //    BOTH decoded axis `<i>` entries and row x col cell-loop iterations across all tables:
    //    16 tables of 998 rows x 1000 cols (1 data col + 999 blank cols) consume
    //    998 + 1,000 + 998,000 = 999,998 iterations each (15,999,968 total, <= 16M).
    //    The 17th table has only 50 row `<i>` entries x 1 col (would fit if axis entries were not
    //    counted, since 16 * 998,000 + 50 < 16M), so it proves axis decoding is charged and bails
    //    at 16,000,000 with a "workbook iteration limit" warning.
    let row_998 = r#"<i><x v="0"/></i>"#.repeat(998);
    let col_1000 = format!(r#"<i><x v="0"/></i>{}"#, r#"<i t="blank"/>"#.repeat(999));
    let make_iter_pt = |name: &str, r_count: usize, r_xml: &str, c_count: usize, c_xml: &str| -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="{name}" cacheId="1" dataCaption="Values">
  <location ref="A1:AML1000" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{}</pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="{r_count}">{r_xml}</rowItems>
  <colFields count="1"><field x="16"/></colFields>
  <colItems count="{c_count}">{c_xml}</colItems>
  <dataFields count="1"><dataField name="S" fld="32"/></dataFields>
</pivotTableDefinition>"#,
            fx.pfs
        )
    };
    let mut iter_tables: Vec<(String, String)> = (1..=16)
        .map(|i| {
            let name = format!("IterPt{i}");
            (name.clone(), make_iter_pt(&name, 998, &row_998, 1000, &col_1000))
        })
        .collect();
    let row_50 = r#"<i><x v="0"/></i>"#.repeat(50);
    iter_tables.push(("IterPt17".into(), make_iter_pt("IterPt17", 50, &row_50, 1, r#"<i><x v="0"/></i>"#)));
    let (wb_e, report_e) = read_xlsx(&fx.build_multi(iter_tables)).unwrap();
    let pivots_e = &wb_e.sheet(1).unwrap().pivots;
    assert_eq!(pivots_e.len(), 17);
    for (idx, pt) in pivots_e.iter().enumerate().take(16) {
        assert!(pt.data_index.cells().is_some(), "IterPt{} fits in 16M workbook iteration cap", idx + 1);
    }
    assert!(pivots_e[16].data_index.cells().is_none(), "IterPt17 must bail out at MAX_WORKBOOK_INDEX_ITERATIONS");
    assert!(
        report_e.warnings.iter().any(|w| w.contains("IterPt17") && w.contains("workbook iteration limit")),
        "workbook iteration cap bailout must emit warning, got: {:?}",
        report_e.warnings
    );

    // F. Tall 1-field ~320K-row pivot placed FIRST before two legitimate medium pivots: the tall
    //    pivot exceeds the per-table 128 MiB work budget and bails with a warning, while both
    //    subsequent tables (`MedAfter1`/`MedAfter2`) stay indexed because work bytes reset per table.
    let tall_rows = r#"<i><x v="0"/></i>"#.repeat(320_000);
    let tall_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="TallPivot" cacheId="1" dataCaption="Values">
  <location ref="A1:C320005" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="33">{}</pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="320000">{tall_rows}</rowItems>
  <colFields count="1"><field x="16"/></colFields>
  <colItems count="1"><i><x v="0"/></i></colItems>
  <dataFields count="1"><dataField name="S" fld="32"/></dataFields>
</pivotTableDefinition>"#,
        fx.pfs
    );
    let tall_first = vec![
        ("TallPivot".into(), tall_pt_xml),
        ("MedAfter1".into(), fx.make_pt("MedAfter1", 25, "A1:AZ50", &med_items)),
        ("MedAfter2".into(), fx.make_pt("MedAfter2", 25, "A1:AZ50", &med_items)),
    ];
    let (wb_f, report_f) = read_xlsx(&fx.build_multi(tall_first)).unwrap();
    let pivots_f = &wb_f.sheet(1).unwrap().pivots;
    assert_eq!(pivots_f.len(), 3);
    assert!(pivots_f[0].data_index.cells().is_none(), "TallPivot must bail out on per-table 128 MiB work budget");
    assert!(pivots_f[1].data_index.cells().is_some(), "MedAfter1 must stay indexed after TallPivot");
    assert!(pivots_f[2].data_index.cells().is_some(), "MedAfter2 must stay indexed after TallPivot");
    assert_eq!(report_f.warnings.len(), 1, "only TallPivot should warn, got: {:?}", report_f.warnings);
    assert!(
        report_f.warnings[0].contains("TallPivot") && report_f.warnings[0].contains("work budget"),
        "TallPivot bailout must warn about work budget, got: {:?}",
        report_f.warnings
    );
}

#[test]
fn ooxml_32_real_axis_fields_plus_values_placeholder_is_allowed() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_fields: String = (0..32)
        .map(|i| format!(r#"<cacheField name="F{i}" numFmtId="0"><sharedItems count="1"><s v="A"/></sharedItems></cacheField>"#))
        .chain(std::iter::once(r#"<cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
        .chain(std::iter::once(r#"<cacheField name="Units" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
        .collect();
    let cache_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:AH2" sheet="Data"/></cacheSource>
  <cacheFields count="34">{cache_fields}</cacheFields>
</pivotCacheDefinition>"#
    );
    let pfs: String = (0..16)
        .map(|_| r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#)
        .chain((0..16).map(|_| r#"<pivotField axis="axisCol" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#))
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .collect();
    // 16 real row fields + `-2` Values placeholder + 16 real col fields = 32 real axis fields (exact cap).
    let row_fields: String = (0..16).map(|i| format!(r#"<field x="{i}"/>"#)).chain(std::iter::once(r#"<field x="-2"/>"#.to_string())).collect();
    let col_fields: String = (16..32).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let rxs0: String = (0..16).map(|_| r#"<x v="0"/>"#).chain(std::iter::once(r#"<x v="0"/>"#)).collect();
    let rxs1: String = (0..16).map(|_| r#"<x v="0"/>"#).chain(std::iter::once(r#"<x v="1"/>"#)).collect();
    let cxs: String = (0..16).map(|_| r#"<x v="0"/>"#).collect();
    let pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:D5" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="34">{pfs}</pivotFields>
  <rowFields count="17">{row_fields}</rowFields>
  <rowItems count="2"><i>{rxs0}</i><i>{rxs1}</i></rowItems>
  <colFields count="16">{col_fields}</colFields>
  <colItems count="1"><i>{cxs}</i></colItems>
  <dataFields count="2"><dataField name="Sum of Sales" fld="32"/><dataField name="Sum of Units" fld="33"/></dataFields>
</pivotTableDefinition>"#
    );
    let back = repack_pivot1(&wb, &cache_xml, &pt_xml);
    let pt = &back.sheet(1).unwrap().pivots[0];
    assert!(pt.data_index.cells().is_some(), "32 real axis fields + -2 Values placeholder must be allowed");
    let query: Vec<(String, &str, bool)> = (0..32).map(|i| (format!("F{i}"), "A", false)).collect();
    let qrefs: Vec<(&str, &str, bool)> = query.iter().map(|(f, l, b)| (f.as_str(), *l, *b)).collect();
    assert_eq!(pt.data_index.find("Sum of Sales", &qrefs).map(|c| (c.row, c.col)), Some((1, 1)));
    assert_eq!(pt.data_index.find("Sum of Units", &qrefs).map(|c| (c.row, c.col)), Some((2, 1)));
}

#[test]
fn ooxml_workbook_work_bytes_ceiling_across_20_sheets() {
    // 20 worksheets each linking the same long-string WorkBomb PivotTable part (~128 MiB transient
    // work per sheet -> ~2.56 GiB cumulative): earlier sheets bail on the 128 MiB per-table work
    // budget, and once cumulative workbook work reaches `MAX_WORKBOOK_WORK_BYTES` (2 GiB), later
    // sheets bail immediately with "workbook work limit reached" and stay `None`.
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    for k in 2..=21 {
        wb.sheets.push(Arc::new(Sheet::new(format!("Report{k}"))));
    }
    let raw = write_xlsx(&wb).unwrap();
    let mut parts = unzip(&raw);

    let long_name = "F".repeat(64 * 1024);
    let long_val = "V".repeat(64 * 1024);
    let cache_fields: String = (0..16)
        .map(|i| {
            format!(r#"<cacheField name="{i}_{long_name}" numFmtId="0"><sharedItems count="1"><s v="{i}_{long_val}"/></sharedItems></cacheField>"#)
        })
        .chain(std::iter::once(r#"<cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>"#.to_string()))
        .collect();
    let cache_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="1">
  <cacheSource type="worksheet"><worksheetSource ref="A1:Q2" sheet="Data"/></cacheSource>
  <cacheFields count="17">{cache_fields}</cacheFields>
</pivotCacheDefinition>"#
    );
    let pfs: String = (0..16)
        .map(|_| r#"<pivotField axis="axisRow" showAll="0"><items count="1"><item x="0"/></items></pivotField>"#)
        .chain(std::iter::once(r#"<pivotField dataField="1" showAll="0"/>"#))
        .collect();
    let row_fields: String = (0..16).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
    let xs16: String = (0..16).map(|_| r#"<x v="0"/>"#).collect();
    // 65 row items x 16 fields x (64 KiB name + 64 KiB value) = ~130 MiB > MAX_TABLE_WORK_BYTES (128 MiB).
    let row_items = format!("<i>{xs16}</i>").repeat(65);
    let pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="WorkBomb" cacheId="1" dataCaption="Values">
  <location ref="A1:C100" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="17">{pfs}</pivotFields>
  <rowFields count="16">{row_fields}</rowFields>
  <rowItems count="65">{row_items}</rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="16"/></dataFields>
</pivotTableDefinition>"#
    );
    let small_pt_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="SmallAfter" cacheId="1" dataCaption="Values">
  <location ref="A1:C5" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="17">{pfs}</pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="1"><i><x v="0"/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="16"/></dataFields>
</pivotTableDefinition>"#
    );
    parts.insert("xl/pivotCache/pivotCacheDefinition1.xml".into(), cache_xml.into_bytes());
    parts.insert("xl/pivotTables/pivotTable1.xml".into(), pt_xml.into_bytes());
    parts.insert("xl/pivotTables/pivotTable2.xml".into(), small_pt_xml.into_bytes());
    let pt1_rels = parts["xl/pivotTables/_rels/pivotTable1.xml.rels"].clone();
    parts.insert("xl/pivotTables/_rels/pivotTable2.xml.rels".into(), pt1_rels);

    let sheet2_rels = parts["xl/worksheets/_rels/sheet2.xml.rels"].clone();
    for sheet_num in 3..=21 {
        parts.insert(format!("xl/worksheets/_rels/sheet{sheet_num}.xml.rels"), sheet2_rels.clone());
    }
    let small_sheet_rels = String::from_utf8(sheet2_rels).unwrap().replace("pivotTable1.xml", "pivotTable2.xml");
    parts.insert("xl/worksheets/_rels/sheet22.xml.rels".into(), small_sheet_rels.into_bytes());

    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in &parts {
        zw.start_file(name, opts).unwrap();
        std::io::Write::write_all(&mut zw, data).unwrap();
    }
    zw.finish().unwrap();
    let bytes = buf.into_inner();

    let (back, report) = read_xlsx(&bytes).unwrap();
    for idx in 1..=20 {
        let pivots = &back.sheet(idx).unwrap().pivots;
        assert_eq!(pivots.len(), 1, "sheet {idx} should link WorkBomb");
        assert!(pivots[0].data_index.cells().is_none(), "sheet {idx} WorkBomb must stay None");
    }
    let trailing = &back.sheet(21).unwrap().pivots;
    assert_eq!(trailing.len(), 1, "sheet 21 should link SmallAfter");
    assert!(trailing[0].data_index.cells().is_none(), "SmallAfter must stay None once workbook 2 GiB work limit is reached");
    assert!(
        report.warnings.iter().any(|w| w.contains("WorkBomb") && w.contains("table work budget exceeded")),
        "early WorkBomb sheets must trip per-table work budget, got: {:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("WorkBomb") && w.contains("workbook work limit reached")),
        "later WorkBomb sheets must trip workbook work limit, got: {:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("SmallAfter") && w.contains("workbook work limit reached")),
        "trailing small pivot must hit pre-check workbook work limit, got: {:?}",
        report.warnings
    );
}

#[test]
fn ooxml_sentinel_1048832_and_out_of_bounds_axis_items_bail_cleanly() {
    let mut wb = workbook();
    wb.sheet_mut(1).unwrap().pivots.truncate(1);
    let cache_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" refreshOnLoad="0" recordCount="2">
  <cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource>
  <cacheFields count="2">
    <cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="East"/><s v="West"/></sharedItems></cacheField>
    <cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1" count="0"/></cacheField>
  </cacheFields>
</pivotCacheDefinition>"#;

    // 1a. `<item x="1048832"/>` sentinel must bail cleanly (`None`), never publishing a partial index.
    let sentinel_item_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1048832"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, sentinel_item_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "item x=1048832 must bail cleanly");

    // 1b. `<x v="1048832"/>` sentinel in `<rowItems>` (including on `-2` Values axis) must bail cleanly.
    let sentinel_row_x_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:C4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1048832"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, sentinel_row_x_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "rowItems <x v=1048832> must bail cleanly");

    let sentinel_values_x_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:C4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colFields count="1"><field x="-2"/></colFields>
  <colItems count="2"><i><x v="0"/></i><i><x v="1048832"/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, sentinel_values_x_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "Values axis <x v=1048832> must bail cleanly");

    // 2a. `<rowItems>` running past `<location>` (`A1:B3` with `firstDataRow="1"` holds only 2 data rows,
    //     while `<rowItems>` has 3 entries) must bail cleanly instead of publishing a partial index.
    let oob_row_items_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B3" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="1"><i><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, oob_row_items_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "rowItems past location must bail cleanly");

    // 2b. `<colItems>` running past `<location>` (`A1:B4` with `firstDataCol="1"` holds only 1 data col,
    //     while `<colItems>` has 2 entries) must bail cleanly instead of publishing a partial index.
    let oob_col_items_pt = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="PivotTable1" cacheId="1" dataCaption="Values">
  <location ref="A1:B4" firstHeaderRow="1" firstDataRow="1" firstDataCol="1" rowPageCount="0" colPageCount="0"/>
  <pivotFields count="2">
    <pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
    <pivotField dataField="1" showAll="0"/>
  </pivotFields>
  <rowFields count="1"><field x="0"/></rowFields>
  <rowItems count="3"><i><x v="0"/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems>
  <colItems count="2"><i><x/></i><i t="grand"><x/></i></colItems>
  <dataFields count="1"><dataField name="Sum of Sales" fld="1"/></dataFields>
</pivotTableDefinition>"#;
    let back = repack_pivot1(&wb, cache_xml, oob_col_items_pt);
    assert!(back.sheet(1).unwrap().pivots[0].data_index.cells().is_none(), "colItems past location must bail cleanly");
}
