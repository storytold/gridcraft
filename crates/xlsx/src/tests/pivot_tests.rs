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
        data.cells.set(CellRef::new(r, 2), Cell { value: Value::number(*d), formula: None, style: date_style });
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
    // Pivot 1: tabular, page filter, custom and built-in value formats, collapsed item.
    let p1 = PivotTable {
        id: 1,
        name: "Sales by Region".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("A4"),
        rows: vec![PivotField { source_col: "Region".into(), sort: PivotSort::Asc, collapsed_items: vec!["East".into()], ..PivotField::default() }],
        columns: vec![PivotField { sort: PivotSort::None, ..field("Product") }],
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
        filters: vec![PivotFilter { source_col: "Channel".into(), selected: Some(vec!["Online".into()]), ..PivotFilter::default() }],
        layout: PivotLayout::Tabular,
        grand_totals_rows: false,
        grand_totals_cols: true,
        subtotals: false,
        subtotals_top: false,
        style: "PivotStyleMedium9".into(),
        show_headers: false,
        last_range: Some(rr("A2:H12")),
        calculated_fields: vec![],
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
            PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Years, sort: PivotSort::None, collapsed_items: vec!["2024".into()] },
            PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Months, sort: PivotSort::Asc, collapsed_items: vec![] },
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
            PivotFilter { source_col: "Region".into(), selected: Some(vec!["East".into(), "North".into()]), ..PivotFilter::default() },
            PivotFilter { source_col: "Product".into(), selected: Some(vec!["A".into(), "B".into()]), ..PivotFilter::default() },
        ],
        layout: PivotLayout::Outline,
        grand_totals_rows: true,
        grand_totals_cols: false,
        subtotals: true,
        subtotals_top: true,
        style: "PivotStyleDark3".into(),
        show_headers: true,
        last_range: Some(rr("K2:Z30")),
        calculated_fields: vec![],
        values_on_rows: false,
        values_position: None,
    };
    // Pivot 3: defaults, defined-name source, compact.
    let p3 = PivotTable {
        id: 3,
        name: "PivotTable3".into(),
        source: "SalesData".into(),
        anchor: at("AB1"),
        rows: vec![field("Product")],
        values: vec![PivotValue { source_col: "Units".into(), func: PivotFunc::Product, name: "Product of Units".into(), ..PivotValue::default() }],
        filters: vec![PivotFilter { source_col: "Channel".into(), ..PivotFilter::default() }],
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
        m.insert(f.name().to_string(), b);
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
    let bytes = write_xlsx(&workbook()).unwrap();
    let parts = unzip(&bytes);
    let ct = xml::parse(parts.get("[Content_Types].xml").unwrap()).unwrap();
    let overrides: HashMap<String, String> = ct
        .kids("Override")
        .map(|o| (o.attr("PartName").unwrap().trim_start_matches('/').to_string(), o.attr("ContentType").unwrap().to_string()))
        .collect();
    let defaults: HashSet<String> = ct.kids("Default").map(|d| d.attr("Extension").unwrap().to_string()).collect();
    // Every override names an existing part; every part has a content type.
    for p in overrides.keys() {
        assert!(parts.contains_key(p), "override for missing part {p}");
    }
    for p in parts.keys() {
        if p == "[Content_Types].xml" {
            continue;
        }
        let ext = p.rsplit('.').next().unwrap();
        assert!(overrides.contains_key(p) || defaults.contains(ext), "no content type for {p}");
    }
    let count = |ct: &str| overrides.values().filter(|v| v.as_str() == ct).count();
    assert_eq!(count(crate::pivot::CT_PIVOT_TABLE), 3);
    assert_eq!(count(crate::pivot::CT_PIVOT_CACHE_DEF), 3);
    assert_eq!(count(crate::pivot::CT_PIVOT_CACHE_REC), 3);
    // Every relationship target exists.
    let mut kinds: Vec<String> = vec![];
    for (name, data) in &parts {
        if !name.ends_with(".rels") {
            continue;
        }
        let dir = name.trim_end_matches(".rels").rsplit_once("/_rels/").map(|(d, _)| d).unwrap_or("");
        let rels = xml::parse(data).unwrap();
        for r in rels.kids("Relationship") {
            if r.attr("TargetMode") == Some("External") {
                continue;
            }
            let target = crate::package::resolve(dir, r.attr("Target").unwrap());
            assert!(parts.contains_key(&target), "{name} points at missing {target}");
            kinds.push(r.attr("Type").unwrap().rsplit('/').next().unwrap().to_string());
        }
    }
    for k in ["pivotTable", "pivotCacheDefinition", "pivotCacheRecords"] {
        assert!(kinds.iter().any(|x| x == k), "no {k} relationship");
    }
    // The workbook lists the caches by id with relationships of the right type.
    let wbx = xml::parse(parts.get("xl/workbook.xml").unwrap()).unwrap();
    let wb_rels = xml::parse(parts.get("xl/_rels/workbook.xml.rels").unwrap()).unwrap();
    let caches: Vec<&xml::El> = wbx.child("pivotCaches").unwrap().kids("pivotCache").collect();
    assert_eq!(caches.len(), 3);
    let mut ids = HashSet::new();
    for c in &caches {
        assert!(ids.insert(c.attr("cacheId").unwrap().to_string()));
        let rid = c.attr("id").unwrap();
        let rel = wb_rels.kids("Relationship").find(|r| r.attr("Id") == Some(rid)).unwrap();
        assert!(rel.attr("Type").unwrap().ends_with("/pivotCacheDefinition"));
    }
    // Each table's cacheId is one of the workbook's caches and its field counts agree.
    for t in 1..=3 {
        let pt = xml::parse(parts.get(&format!("xl/pivotTables/pivotTable{t}.xml")).unwrap()).unwrap();
        assert!(ids.contains(pt.attr("cacheId").unwrap()));
        let trels = xml::parse(parts.get(&format!("xl/pivotTables/_rels/pivotTable{t}.xml.rels")).unwrap()).unwrap();
        let target = crate::package::resolve("xl/pivotTables", trels.kids("Relationship").next().unwrap().attr("Target").unwrap());
        let cache = xml::parse(parts.get(&target).unwrap()).unwrap();
        let nf = cache.child("cacheFields").unwrap().kids("cacheField").count();
        assert_eq!(pt.child("pivotFields").unwrap().kids("pivotField").count(), nf);
        assert_eq!(cache.child("cacheFields").unwrap().attr_u32("count"), Some(nf as u32));
        // Item indices stay inside the shared/group items.
        for (i, pf) in pt.child("pivotFields").unwrap().kids("pivotField").enumerate() {
            let cf = cache.child("cacheFields").unwrap().kids("cacheField").nth(i).unwrap();
            let n = cf
                .path(&["fieldGroup", "groupItems"])
                .map(|g| g.children.len())
                .unwrap_or_else(|| cf.child("sharedItems").map(|s| s.children.len()).unwrap_or(0));
            if let Some(items) = pf.child("items") {
                assert_eq!(items.attr_u32("count"), Some(items.children.len() as u32));
                for it in items.kids("item") {
                    if let Some(x) = it.attr_u32("x") {
                        assert!((x as usize) < n, "item x={x} out of {n} in table {t} field {i}");
                    }
                }
            }
        }
        // Records match the database fields.
        if let Some(rid) = cache.attr("id") {
            let crels_name = target.replace("pivotCache/", "pivotCache/_rels/") + ".rels";
            let crels = xml::parse(parts.get(&crels_name).unwrap()).unwrap();
            let rel = crels.kids("Relationship").find(|r| r.attr("Id") == Some(rid)).unwrap();
            let rec = xml::parse(parts.get(&crate::package::resolve("xl/pivotCache", rel.attr("Target").unwrap())).unwrap()).unwrap();
            let db = cache.child("cacheFields").unwrap().kids("cacheField").filter(|f| f.attr("databaseField") != Some("0")).count();
            assert_eq!(rec.attr_u32("count"), cache.attr_u32("recordCount"));
            assert_eq!(rec.kids("r").count(), 8);
            for r in rec.kids("r") {
                assert_eq!(r.children.len(), db);
            }
        }
    }
    // Sheet 2 (Report) relates to all three tables.
    let srels = xml::parse(parts.get("xl/worksheets/_rels/sheet2.xml.rels").unwrap()).unwrap();
    assert_eq!(srels.kids("Relationship").filter(|r| r.attr("Type").unwrap().ends_with("/pivotTable")).count(), 3);
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

/// `workbook()` plus a slicer on "Report" over the Product field of "Sales by Region", with
/// "A" and "C" selected (Product is a column field, so the selection is its hidden items).
fn slicer_workbook() -> Workbook {
    let mut wb = workbook();
    let report = wb.sheet_mut(1).unwrap();
    let pi = report.pivots.iter().position(|p| p.name == "Sales by Region").unwrap();
    report.pivots[pi].filters.push(PivotFilter {
        source_col: "Product".into(),
        selected: Some(vec!["A".into(), "C".into()]),
        ..PivotFilter::default()
    });
    report.slicers.push(Slicer {
        id: 1,
        source: SlicerSource::Pivot {
            pivots: vec![SlicerPivot { sheet: "Report".into(), name: "Sales by Region".into() }],
            field: "Product".into(),
            sort: PivotSort::Desc,
            cross_filter: SlicerCrossFilter::ShowItemsWithNoData,
            show_missing: false,
        },
        caption: "Pick a product".into(),
        anchor: Anchor { cell: at("M3"), dx: 4.0, dy: 2.0, width: 180.0, height: 240.0, mode: AnchorMode::default() },
        columns: 2,
        name: "Product".into(),
        style: "SlicerStyleDark1".into(),
        row_height: Some(30.0),
    });
    wb
}

#[test]
fn pivot_slicer_round_trips() {
    let wb = slicer_workbook();
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let text = |k: &str| String::from_utf8(parts.get(k).unwrap_or_else(|| panic!("missing {k}")).clone()).unwrap();
    let cache = text("xl/slicerCaches/slicerCache1.xml");
    assert!(cache.contains("sourceName=\"Product\""), "{cache}");
    assert!(cache.contains("name=\"Sales by Region\""), "{cache}");
    assert!(cache.contains("<tabular pivotCacheId="), "{cache}");
    assert!(cache.contains(" s=\"1\"") && cache.contains("sortOrder=\"descending\""), "{cache}");
    let slicer = text("xl/slicers/slicer1.xml");
    assert!(slicer.contains("caption=\"Pick a product\"") && slicer.contains("columnCount=\"2\""), "{slicer}");
    let ct = text("[Content_Types].xml");
    assert!(ct.contains("/xl/slicers/slicer1.xml") && ct.contains("/xl/slicerCaches/slicerCache1.xml"), "{ct}");
    assert!(text("xl/workbook.xml").contains("slicerCaches"));
    let ws = text("xl/worksheets/sheet2.xml");
    assert!(ws.contains("x14:slicerList"), "{ws}");
    let drawing =
        parts.keys().filter(|k| k.starts_with("xl/drawings/drawing")).map(|k| text(k)).find(|d| d.contains("sle:slicer")).expect("slicer frame");
    assert!(drawing.contains("mc:AlternateContent") && drawing.contains("mc:Fallback"), "{drawing}");

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("slicer") && !w.contains("embedded object")), "{:?}", report.warnings);
    let a = &wb.sheet(1).unwrap();
    let b = &back.sheet(1).unwrap();
    assert_eq!(b.slicers.len(), 1);
    let (x, y) = (&a.slicers[0], &b.slicers[0]);
    assert_eq!((&y.source, &y.caption, y.columns, &y.name, &y.style), (&x.source, &x.caption, x.columns, &x.name, &x.style));
    assert_eq!(y.anchor.cell, x.anchor.cell);
    assert!((y.anchor.width - x.anchor.width).abs() < 1.0 && (y.anchor.height - x.anchor.height).abs() < 1.0);
    assert!(y.row_height.is_some_and(|h| (h - 30.0).abs() < 1.0), "{:?}", y.row_height);
    let pa = a.pivots.iter().find(|p| p.name == "Sales by Region").unwrap();
    let pb = b.pivots.iter().find(|p| p.name == "Sales by Region").unwrap();
    assert_eq!(pa.filters, pb.filters, "the slicer's selection is the pivot's hidden items");
    // A second save is stable.
    let (again, _) = read_xlsx(&write_xlsx(&back).unwrap()).unwrap();
    assert_eq!(again.sheet(1).unwrap().slicers, b.slicers);
}

/// A slicer on Product connected to "Sales by Region" and a second PivotTable on the same source
/// that doesn't show Product (so its selection there is a hidden filter).
#[test]
fn pivot_slicer_keeps_every_connected_pivot_table() {
    let mut wb = slicer_workbook();
    let report = wb.sheet_mut(1).unwrap();
    report.pivots.push(PivotTable {
        id: 4,
        name: "Units by Region".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("AF1"),
        rows: vec![field("Region")],
        values: vec![PivotValue { source_col: "Units".into(), func: PivotFunc::Sum, name: "Sum of Units".into(), ..PivotValue::default() }],
        filters: vec![PivotFilter {
            source_col: "Product".into(),
            selected: Some(vec!["A".into(), "C".into()]),
            hidden: true,
            ..PivotFilter::default()
        }],
        ..PivotTable::default()
    });
    if let SlicerSource::Pivot { pivots, .. } = &mut report.slicers[0].source {
        pivots.push(SlicerPivot { sheet: "Report".into(), name: "Units by Region".into() });
    }
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let text = |k: &str| String::from_utf8(parts.get(k).unwrap_or_else(|| panic!("missing {k}")).clone()).unwrap();
    let cache = text("xl/slicerCaches/slicerCache1.xml");
    assert!(cache.contains("name=\"Sales by Region\"") && cache.contains("name=\"Units by Region\""), "{cache}");
    // The connected PivotTables share one pivot cache, as Excel requires.
    let cache_id = |name: &str| {
        let t = parts
            .keys()
            .filter(|k| k.starts_with("xl/pivotTables/pivotTable"))
            .map(|k| text(k))
            .find(|t| t.contains(&format!("name=\"{name}\"")))
            .unwrap();
        t.split("cacheId=\"").nth(1).and_then(|s| s.split('"').next()).unwrap().to_string()
    };
    assert_eq!(cache_id("Sales by Region"), cache_id("Units by Region"));

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("slicer")), "{:?}", report.warnings);
    let (a, b) = (wb.sheet(1).unwrap(), back.sheet(1).unwrap());
    assert_eq!(b.slicers.len(), 1);
    assert_eq!(b.slicers[0].source, a.slicers[0].source);
    for name in ["Sales by Region", "Units by Region", "Trend", "PivotTable3"] {
        let pa = a.pivots.iter().find(|p| p.name == name).unwrap();
        let pb = b.pivots.iter().find(|p| p.name == name).unwrap();
        assert_eq!((&pa.rows, &pa.columns, &pa.filters), (&pb.rows, &pb.columns, &pb.filters), "{name}");
    }
}

/// PivotTables a slicer connects share one cache only when they have the same calculated fields:
/// a cache's calculated fields are read back into each of its tables, so sharing would add one
/// table's calculated field to the other.
#[test]
fn pivot_slicer_shares_no_cache_across_calculated_fields() {
    let mut wb = slicer_workbook();
    let report = wb.sheet_mut(1).unwrap();
    let pi = report.pivots.iter().position(|p| p.name == "Sales by Region").unwrap();
    report.pivots[pi].calculated_fields.push(PivotCalcField { name: "Price".into(), formula: "=Sales/Units".into() });
    report.pivots.push(PivotTable {
        id: 4,
        name: "Units by Region".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("AF1"),
        rows: vec![field("Region")],
        values: vec![PivotValue { source_col: "Units".into(), func: PivotFunc::Sum, name: "Sum of Units".into(), ..PivotValue::default() }],
        filters: vec![PivotFilter {
            source_col: "Product".into(),
            selected: Some(vec!["A".into(), "C".into()]),
            hidden: true,
            ..PivotFilter::default()
        }],
        ..PivotTable::default()
    });
    if let SlicerSource::Pivot { pivots, .. } = &mut report.slicers[0].source {
        pivots.push(SlicerPivot { sheet: "Report".into(), name: "Units by Region".into() });
    }
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let calc = |name: &str| back.sheet(1).unwrap().pivots.iter().find(|p| p.name == name).unwrap().calculated_fields.clone();
    assert_eq!(calc("Sales by Region"), vec![PivotCalcField { name: "Price".into(), formula: "=Sales/Units".into() }]);
    assert_eq!(calc("Units by Region"), vec![]);

    // With the same calculated fields they share one cache and stay connected.
    let report = wb.sheet_mut(1).unwrap();
    let units = report.pivots.iter().position(|p| p.name == "Units by Region").unwrap();
    report.pivots[units].calculated_fields.push(PivotCalcField { name: "Price".into(), formula: "=Sales/Units".into() });
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(1).unwrap().slicers[0].source, wb.sheet(1).unwrap().slicers[0].source);
}

/// A slicer on a date field grouped by quarters lists the groups, in date order.
#[test]
fn pivot_slicer_on_grouped_dates_writes_the_groups() {
    let mut wb = workbook();
    let report = wb.sheet_mut(1).unwrap();
    report.pivots.push(PivotTable {
        id: 4,
        name: "By Quarter".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("AF1"),
        rows: vec![PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Quarters, ..PivotField::default() }],
        values: vec![PivotValue { source_col: "Sales".into(), func: PivotFunc::Sum, name: "Sum of Sales".into(), ..PivotValue::default() }],
        filters: vec![PivotFilter { source_col: "Date".into(), selected: Some(vec!["Qtr2".into()]), ..PivotFilter::default() }],
        ..PivotTable::default()
    });
    report.slicers.push(Slicer {
        id: 1,
        source: SlicerSource::Pivot {
            pivots: vec![SlicerPivot { sheet: "Report".into(), name: "By Quarter".into() }],
            field: "Date".into(),
            sort: PivotSort::Asc,
            cross_filter: SlicerCrossFilter::ShowItemsWithDataAtTop,
            show_missing: true,
        },
        caption: "Date".into(),
        anchor: Anchor { cell: at("AJ1"), dx: 0.0, dy: 0.0, width: 180.0, height: 240.0, mode: AnchorMode::default() },
        columns: 1,
        name: "Date".into(),
        style: String::new(),
        row_height: None,
    });
    let bytes = write_xlsx(&wb).unwrap();
    let parts = unzip(&bytes);
    let cache = String::from_utf8(parts["xl/slicerCaches/slicerCache1.xml"].clone()).unwrap();
    // Items: <start, Qtr1..Qtr4, >end, listed in that order, with only Qtr2 selected.
    let items = cache.split("<items").nth(1).unwrap();
    let xs: Vec<&str> = items.split("<i x=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    assert_eq!(xs, ["0", "1", "2", "3", "4", "5"], "{cache}");
    assert!(items.contains("<i x=\"2\" s=\"1\"/>") && items.matches("s=\"1\"").count() == 1, "{cache}");

    let (back, _) = read_xlsx(&bytes).unwrap();
    let b = back.sheet(1).unwrap();
    assert_eq!(b.slicers[0].source, wb.sheet(1).unwrap().slicers[0].source);
    let pb = b.pivots.iter().find(|p| p.name == "By Quarter").unwrap();
    assert_eq!(pb.filters, vec![PivotFilter { source_col: "Date".into(), selected: Some(vec!["Qtr2".into()]), ..PivotFilter::default() }]);
}

/// With a date grouped at several levels, Excel keeps each coarser level in its own cache field
/// ("Years" over "Date"), and a slicer on that level names it as its source. Such a slicer is
/// skipped with one warning that says so, and the PivotTable is kept.
#[test]
fn pivot_slicer_on_a_coarser_date_level_is_skipped_with_a_warning() {
    let mut wb = workbook();
    let report = wb.sheet_mut(1).unwrap();
    report.pivots.push(PivotTable {
        id: 4,
        name: "By Year".into(),
        source: "Data!$A$1:$F$9".into(),
        anchor: at("AF1"),
        rows: vec![
            PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Years, ..PivotField::default() },
            PivotField { source_col: "Date".into(), date_group: PivotDateGroup::Months, ..PivotField::default() },
        ],
        values: vec![PivotValue { source_col: "Sales".into(), func: PivotFunc::Sum, name: "Sum of Sales".into(), ..PivotValue::default() }],
        ..PivotTable::default()
    });
    report.slicers.push(Slicer {
        id: 1,
        source: SlicerSource::Pivot {
            pivots: vec![SlicerPivot { sheet: "Report".into(), name: "By Year".into() }],
            field: "Date".into(),
            sort: PivotSort::Asc,
            cross_filter: SlicerCrossFilter::ShowItemsWithDataAtTop,
            show_missing: true,
        },
        caption: "Years".into(),
        anchor: Anchor { cell: at("AJ1"), dx: 0.0, dy: 0.0, width: 180.0, height: 240.0, mode: AnchorMode::default() },
        columns: 1,
        name: "Years".into(),
        style: String::new(),
        row_height: None,
    });
    let mut parts = unzip(&write_xlsx(&wb).unwrap());
    let defs: Vec<String> = parts
        .iter()
        .filter(|(k, _)| k.starts_with("xl/pivotCache/pivotCacheDefinition"))
        .map(|(_, v)| String::from_utf8(v.clone()).unwrap())
        .collect();
    assert!(defs.iter().any(|d| d.contains("<cacheField name=\"Years\"") && d.contains("databaseField=\"0\"")), "{defs:?}");
    let key = "xl/slicerCaches/slicerCache1.xml".to_string();
    let cache = String::from_utf8(parts.remove(&key).unwrap()).unwrap();
    let edited = cache.replace("sourceName=\"Date\"", "sourceName=\"Years\"");
    assert_ne!(cache, edited);
    parts.insert(key, edited.into_bytes());
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let (back, report) = read_xlsx(&super::make_zip(&refs)).unwrap();
    let b = back.sheet(1).unwrap();
    assert!(b.slicers.is_empty(), "{:?}", b.slicers);
    let about: Vec<&String> = report.warnings.iter().filter(|w| w.contains("slicer")).collect();
    assert_eq!(about.len(), 1, "{:?}", report.warnings);
    assert!(about[0].contains("coarser date level of \"Date\""), "{about:?}");
    let pb = b.pivots.iter().find(|p| p.name == "By Year").unwrap();
    assert_eq!(pb.rows.iter().map(|f| f.date_group).collect::<Vec<_>>(), [PivotDateGroup::Years, PivotDateGroup::Months]);
    let _ = write_xlsx(&back).unwrap();
}

#[test]
fn hostile_slicer_parts_warn_not_panic() {
    let bytes = write_xlsx(&slicer_workbook()).unwrap();
    let cache = "xl/slicerCaches/slicerCache1.xml";
    let edits: [(&str, &dyn Fn(&str) -> String); 7] = [
        // The cache names a PivotTable that doesn't exist.
        (cache, &|s| s.replace("name=\"Sales by Region\"", "name=\"Nowhere\"")),
        (cache, &|s| s.replace("tabId=\"", "tabId=\"99")),
        (cache, &|s| s.replace("pivotCacheId=\"", "pivotCacheId=\"77")),
        (cache, &|s| s.replace("<i x=\"", "<i x=\"4294967")),
        (cache, &|_| "garbage <<<".into()),
        ("xl/slicers/slicer1.xml", &|s| s.replace("cache=\"", "cache=\"Other")),
        ("xl/slicers/slicer1.xml", &|_| "<slicers/>".into()),
    ];
    for (part, f) in edits {
        let mut parts = unzip(&bytes);
        let old = String::from_utf8(parts.remove(part).unwrap()).unwrap();
        let new = f(&old);
        assert_ne!(old, new, "edit to {part} did nothing");
        let mut list: Vec<(String, Vec<u8>)> = parts.into_iter().collect();
        list.push((part.to_string(), new.into_bytes()));
        let refs: Vec<(&str, &[u8])> = list.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let (wb, report) = read_xlsx(&super::make_zip(&refs)).unwrap();
        let sh = wb.sheet(1).unwrap();
        if sh.slicers.is_empty() {
            // One warning names the dropped slicer, not a second one for its frame.
            let about = report.warnings.iter().filter(|w| w.contains("\"Product\"")).count();
            assert_eq!(about, 1, "after editing {part}: {:?}", report.warnings);
        }
        // Whatever was read saves again.
        let _ = write_xlsx(&wb).unwrap();
    }
}

#[test]
fn unresolved_slicer_cache_is_skipped_and_dropped_on_save() {
    let bytes = write_xlsx(&slicer_workbook()).unwrap();
    let cache = "xl/slicerCaches/slicerCache1.xml";
    for replacement in [("pivotCacheId=\"", "pivotCacheId=\"77"), (" pivotCacheId=\"1\"", "")] {
        let mut parts = unzip(&bytes);
        let original = String::from_utf8(parts.remove(cache).unwrap()).unwrap();
        let edited = original.replace(replacement.0, replacement.1);
        assert_ne!(original, edited);
        parts.insert(cache.into(), edited.into_bytes());
        let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let (wb, report) = read_xlsx(&super::make_zip(&refs)).unwrap();
        assert!(wb.sheet(1).unwrap().slicers.is_empty());
        assert!(report.warnings.iter().any(|w| w.contains("has no resolvable PivotTable cache; skipped")), "{:?}", report.warnings);
        let saved = unzip(&write_xlsx(&wb).unwrap());
        assert!(!saved.keys().any(|k| k.starts_with("xl/slicers/") || k.starts_with("xl/slicerCaches/")));
        assert!(!saved.values().any(|v| String::from_utf8_lossy(v).contains("x14:slicerList")));
    }
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
    assert!(t.contains("<x14:pivotTableDefinition hideValuesRow=\"1\"/>"), "{t}");
    let one = small_pivot(&cols, |p| {
        p.rows = vec![field("Region")];
        p.values = vec![value("Sales")];
    });
    let t = &one["xl/pivotTables/pivotTable1.xml"];
    assert!(t.contains("firstHeaderRow=\"1\""), "{t}");
    assert!(!t.contains("hideValuesRow"), "{t}");
}
