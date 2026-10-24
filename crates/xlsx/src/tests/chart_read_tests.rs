//! Reading charts as Excel saved them: PivotChart series and deleted automatic titles.

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Chart, ChartKind, Sheet, Workbook};

const NS: &str =
    r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

/// Reads a chart part as part of `wb` (sheet 0): PivotChart series whose cells don't hold their
/// cached points plot the cache.
fn read_in(wb: &Workbook, xml: &str) -> Chart {
    let (mut c, caches) = crate::chart::read_chart(&crate::xml::parse(xml.as_bytes()).unwrap()).unwrap();
    crate::chart::plot_cached(wb, 0, &mut c, &caches);
    c
}

fn read(xml: &str) -> Chart {
    read_in(&Workbook::new(), xml)
}

/// A workbook with one sheet holding `cells` (A1 address, value).
fn book(name: &str, cells: &[(&str, Value)]) -> Workbook {
    let mut sheet = Sheet::new(name);
    for (a, v) in cells {
        sheet.set_value(CellRef::parse(a).unwrap(), v.clone());
    }
    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(sheet)];
    wb
}

fn str_cache(f: &str, pts: &[&str]) -> String {
    let p: String = pts.iter().enumerate().map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)).collect();
    format!(r#"<c:strRef><c:f>{f}</c:f><c:strCache><c:ptCount val="{}"/>{p}</c:strCache></c:strRef>"#, pts.len())
}

/// A numeric cache of `count` points; `pts` are (idx, value), so gaps can be left out.
fn num_cache(f: &str, count: usize, pts: &[(usize, &str)]) -> String {
    let p: String = pts.iter().map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)).collect();
    format!(r#"<c:numRef><c:f>{f}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{count}"/>{p}</c:numCache></c:numRef>"#)
}

fn ser(tx: &str, cat: &str, val: &str) -> String {
    format!("<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx>{tx}</c:tx><c:cat>{cat}</c:cat><c:val>{val}</c:val></c:ser>")
}

fn chart_xml(pivot: bool, plot: &str, title: &str) -> String {
    let src = if pivot { "<c:pivotSource><c:name>[Book.xlsx]Graph!PivotTable1</c:name><c:fmtId val=\"0\"/></c:pivotSource>" } else { "" };
    format!(
        r#"<c:chartSpace {NS}>{src}<c:chart>{title}<c:plotArea>{plot}<c:catAx><c:axId val="1"/></c:catAx><c:valAx><c:axId val="2"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#
    )
}

#[test]
fn pivot_charts_plot_their_cached_points_when_the_references_do_not_cover_them() {
    // Excel points every formula of this combo PivotChart at the PivotTable's corner cell and
    // draws it from the PivotTable; the cached points are what it shows.
    let cats = ["Truck 1", "Van \"B\"", "Car"];
    let bar =
        ser(&str_cache("Graph!$J$2", &["Events"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "18"), (1, "12"), (2, "5")]));
    // A point the cache leaves out stays a gap, so later points keep their positions.
    let line = ser(&str_cache("Graph!$J$2", &["Share"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "5.3E-2"), (2, "0.5")]));
    let plot = format!(
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{bar}<c:axId val="1"/><c:axId val="2"/></c:barChart><c:lineChart><c:grouping val="standard"/>{line}<c:axId val="1"/><c:axId val="2"/></c:lineChart>"#
    );
    let c = read(&chart_xml(true, &plot, ""));
    assert_eq!(c.pivot.as_deref(), Some("Graph!PivotTable1"));
    assert_eq!(c.kind, ChartKind::Combo);
    let s = &c.series;
    assert_eq!(s[0].name.as_deref(), Some("\"Events\""));
    assert_eq!(s[0].categories.as_deref(), Some(r#"{"Truck 1","Van ""B""","Car"}"#));
    assert_eq!(s[0].values, "{18,12,5}");
    assert_eq!(s[1].name.as_deref(), Some("\"Share\""));
    assert_eq!(s[1].values, "{0.053,#N/A,0.5}");

    // The block Excel writes takes in the grand total row (4 cells for 3 points): the total is
    // not plotted.
    let totals = ser(
        &str_cache("Summary!$B$4", &["Total"]),
        &str_cache("Summary!$A$5:$A$8", &cats),
        &num_cache("Summary!$B$5:$B$8", 3, &[(0, "1010"), (1, "185.14"), (2, "120")]).replace("General", "\"$\"#,##0"),
    );
    let c = read(&chart_xml(true, &format!(r#"<c:pieChart>{totals}</c:pieChart>"#), ""));
    assert_eq!(c.series[0].values, "{1010,185.14,120}");
    assert_eq!(c.series[0].categories.as_deref(), Some(r#"{"Truck 1","Van ""B""","Car"}"#));
    // The cached points keep the cache's number format (data labels show "$185"), and so does
    // a save.
    assert_eq!(c.series[0].number_format.as_deref(), Some("\"$\"#,##0"));
    let back = read(&crate::chart::write_chart(&Workbook::new(), 0, &c, ""));
    assert_eq!(back.series[0].number_format, c.series[0].number_format);

    // References whose cells hold the cached points stay live, and so do an ordinary chart's.
    let live = ser(&str_cache("Summary!$B$4", &["Total"]), &str_cache("Summary!$A$5:$A$7", &cats), &num_cache("Summary!$B$5:$B$7", 3, &[(0, "1")]));
    let mut cells = vec![("B4", Value::text("Total")), ("B5", Value::Number(1.0))];
    cells.extend(["A5", "A6", "A7"].into_iter().zip(cats).map(|(a, t)| (a, Value::text(t))));
    let c = read_in(&book("Summary", &cells), &chart_xml(true, &format!(r#"<c:pieChart>{live}</c:pieChart>"#), ""));
    assert_eq!((c.series[0].name.as_deref(), c.series[0].values.as_str()), (Some("Summary!$B$4"), "Summary!$B$5:$B$7"));
    assert_eq!(c.series[0].number_format, None, "live references use their cells' format");
    let plain = ser(&str_cache("Graph!$J$2", &["Events"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "18")]));
    let c = read(&chart_xml(false, &format!(r#"<c:pieChart>{plain}</c:pieChart>"#), ""));
    assert_eq!(c.series[0].values, "Graph!$J$2");

    // Saved again, the cached points come back as they were: quotes, gaps and all. (A literal
    // name is saved as plain text, which names the series the same way.)
    let c = read(&chart_xml(true, &plot, ""));
    let back = read(&crate::chart::write_chart(&Workbook::new(), 0, &c, ""));
    assert_eq!(back.pivot, c.pivot);
    let parts = |c: &Chart| c.series.iter().map(|s| (shown(s.name.as_deref()), s.categories.clone(), s.values.clone())).collect::<Vec<_>>();
    assert_eq!(parts(&back), parts(&c));
}

#[test]
fn a_one_point_pivot_chart_at_the_corner_cell_plots_its_cache() {
    // A PivotTable with one row item: every formula points at the corner cell, which holds the
    // table's header, and each cache has one point, like the cell count. Excel plots the cache.
    let bar = ser(&str_cache("Data!$J$2", &["Events"]), &str_cache("Data!$J$2", &["Truck 1"]), &num_cache("Data!$J$2", 1, &[(0, "18")]));
    let chart = chart_xml(true, &format!(r#"<c:barChart><c:barDir val="col"/>{bar}<c:axId val="1"/><c:axId val="2"/></c:barChart>"#), "");
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>4</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="4572000" cy="2743200"/><xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="C"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:oneCellAnchor></xdr:wsDr>"#;
    let rel = |kind: &str, target: &str| {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"/></Relationships>"#
        )
    };
    let (srels, drels) = (rel("drawing", "../drawings/drawing1.xml"), rel("chart", "../charts/chart1.xml"));
    let body = r#"<sheetData><row r="2"><c r="J2" t="inlineStr"><is><t>Sum of Events</t></is></c></row></sheetData><drawing r:id="rId1"/>"#;
    let parts = [
        ("xl/worksheets/_rels/sheet1.xml.rels", srels.as_str()),
        ("xl/drawings/drawing1.xml", drawing),
        ("xl/drawings/_rels/drawing1.xml.rels", drels.as_str()),
        ("xl/charts/chart1.xml", chart.as_str()),
    ];
    let (wb, _) = crate::read_xlsx(&super::minimal(body, &parts, "", "")).unwrap();
    let s = &wb.sheets[0].charts[0].series[0];
    assert_eq!((s.name.as_deref(), s.categories.as_deref(), s.values.as_str()), (Some("\"Events\""), Some(r#"{"Truck 1"}"#), "{18}"));
}

#[test]
fn multi_level_pivot_categories_plot_their_innermost_level() {
    // Two levels of row labels in A4:B7, the outer one (country) on the left; the cache keeps
    // the innermost level first.
    let cities = ["Ottawa", "Toronto", "Boston", "NYC"];
    let leaf: String = cities.iter().enumerate().map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)).collect();
    let outer = r#"<c:pt idx="0"><c:v>Canada</c:v></c:pt><c:pt idx="2"><c:v>USA</c:v></c:pt>"#;
    let xml = |f: &str, levels: &str| {
        let cat =
            format!(r#"<c:multiLvlStrRef><c:f>{f}</c:f><c:multiLvlStrCache><c:ptCount val="4"/>{levels}</c:multiLvlStrCache></c:multiLvlStrRef>"#);
        let val = num_cache("Graph!$C$4:$C$7", 4, &[(0, "1"), (1, "2"), (2, "3"), (3, "4")]);
        chart_xml(
            true,
            &format!(
                r#"<c:barChart><c:barDir val="col"/>{}<c:axId val="1"/><c:axId val="2"/></c:barChart>"#,
                ser(&str_cache("Graph!$C$3", &["Total"]), &cat, &val)
            ),
            "",
        )
    };
    let mut cells = vec![("A4", Value::text("Canada")), ("A6", Value::text("USA")), ("C3", Value::text("Total"))];
    for (i, city) in cities.iter().enumerate() {
        cells.push((["B4", "B5", "B6", "B7"][i], Value::text(*city)));
        cells.push((["C4", "C5", "C6", "C7"][i], Value::Number(i as f64 + 1.0)));
    }
    let wb = book("Graph", &cells);
    // The innermost column of the two-level block holds the cache: the series stays live, its
    // references (categories and name) kept as saved, and only the innermost level is drawn.
    let two_levels = xml("Graph!$A$4:$B$7", &format!("<c:lvl>{leaf}</c:lvl><c:lvl>{outer}</c:lvl>"));
    let c = read_in(&wb, &two_levels);
    let s = &c.series[0];
    assert_eq!(
        (s.name.as_deref(), s.categories.as_deref(), s.values.as_str(), s.innermost_categories),
        (Some("Graph!$C$3"), Some("Graph!$A$4:$B$7"), "Graph!$C$4:$C$7", true)
    );
    // A save keeps the references and writes the block as a `multiLvlStrRef` whose cache has
    // both levels, innermost first, so it reads back the same way.
    let saved = crate::chart::write_chart(&wb, 0, &c, "");
    let cat = format!(
        r#"<c:cat><c:multiLvlStrRef><c:f>Graph!$A$4:$B$7</c:f><c:multiLvlStrCache><c:ptCount val="4"/><c:lvl>{leaf}</c:lvl><c:lvl>{outer}</c:lvl></c:multiLvlStrCache></c:multiLvlStrRef></c:cat>"#
    );
    assert!(saved.contains(&cat) && saved.contains("<c:f>Graph!$C$3</c:f>"), "{saved}");
    let back = read_in(&wb, &saved).series[0].clone();
    assert_eq!((back.name, back.categories, back.innermost_categories), (s.name.clone(), s.categories.clone(), true));

    // When the innermost column shows another label, the block is stale: the innermost level
    // is plotted from the cache, one label per point.
    let labels = r#"{"Ottawa","Toronto","Boston","NYC"}"#;
    let mut moved = cells.clone();
    moved.push(("B5", Value::text("Montreal")));
    let s = read_in(&book("Graph", &moved), &two_levels).series[0].clone();
    assert_eq!((s.categories.as_deref(), s.innermost_categories), (Some(labels), false));

    // A one-level reference whose cells hold the cache stays live; when a cell shows another
    // label, the cache is plotted.
    let one_level = xml("Graph!$B$4:$B$7", &format!("<c:lvl>{leaf}</c:lvl>"));
    assert_eq!(read_in(&wb, &one_level).series[0].categories.as_deref(), Some("Graph!$B$4:$B$7"));
    assert_eq!(read_in(&book("Graph", &moved), &one_level).series[0].categories.as_deref(), Some(labels));
}

/// A series name as the chart shows it: a quoted literal without its quotes, `""` read as `"`.
fn shown(name: Option<&str>) -> Option<String> {
    name.map(|n| n.strip_prefix('"').and_then(|q| q.strip_suffix('"')).map_or_else(|| n.to_string(), |q| q.replace("\"\"", "\"")))
}

#[test]
fn series_names_with_quotes_survive_a_save() {
    // A stale PivotChart series takes its cached name, quotes and all.
    let s =
        ser(&str_cache("Graph!$J$2", &["Sales \"East\""]), &str_cache("Graph!$J$2", &["a", "b"]), &num_cache("Graph!$J$2", 2, &[(0, "1"), (1, "2")]));
    let c = read(&chart_xml(true, &format!("<c:pieChart>{s}</c:pieChart>"), ""));
    assert_eq!(shown(c.series[0].name.as_deref()).as_deref(), Some("Sales \"East\""));
    let back = read(&crate::chart::write_chart(&Workbook::new(), 0, &c, ""));
    assert_eq!(shown(back.series[0].name.as_deref()).as_deref(), Some("Sales \"East\""));

    // So does a literal name on a chartex chart.
    let mut c = Chart { kind: ChartKind::Waterfall, ..c };
    c.series[0].categories = None;
    let mut names = Vec::new();
    let xml = crate::chartex::write_chartex(&Workbook::new(), 0, &c, &mut names);
    let back = crate::chartex::read_chartex(&crate::xml::parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(shown(back.series[0].name.as_deref()).as_deref(), Some("Sales \"East\""));
}

#[test]
fn literal_series_keep_their_points_without_a_valid_pt_count() {
    // Not valid by the schema, but readable: the points present set the count, and points
    // without an index go in document order.
    let values = |lit: &str| {
        let s = format!("<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit>{lit}</c:numLit></c:val></c:ser>");
        read(&chart_xml(false, &format!("<c:pieChart>{s}</c:pieChart>"), "")).series[0].values.clone()
    };
    let pts = r#"<c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt>"#;
    assert_eq!(values(&format!(r#"<c:ptCount val="x"/>{pts}"#)), "{1,2}");
    assert_eq!(values(&format!(r#"<c:ptCount val="0"/>{pts}"#)), "{1,2}");
    assert_eq!(values(&format!(r#"<c:ptCount val="1"/>{pts}"#)), "{1,2}");
    assert_eq!(values("<c:pt><c:v>1</c:v></c:pt><c:pt><c:v>2</c:v></c:pt>"), "{1,2}");
    // A valid count still keeps trailing gaps.
    assert_eq!(values(&format!(r#"<c:ptCount val="3"/>{pts}"#)), "{1,2,#N/A}");
}

#[test]
fn chart_cache_counts_and_series_are_bounded() {
    let sparse = r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numLit><c:ptCount val="40"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="9"><c:v>10</c:v></c:pt></c:numLit></c:val></c:ser>"#;
    let chart = read(&chart_xml(false, &format!("<c:pieChart>{sparse}</c:pieChart>"), ""));
    assert_eq!(chart.series[0].values.split(',').count(), 40);

    let huge = r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numLit><c:ptCount val="2000000"/><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:val></c:ser>"#;
    let chart = read(&chart_xml(false, &format!("<c:pieChart>{huge}</c:pieChart>"), ""));
    assert_eq!(chart.series[0].values.split(',').count(), gridcraft_core::MAX_ROWS as usize);

    let series: String = (0..300)
        .map(|i| format!(r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>{i}</c:v></c:pt></c:numLit></c:val></c:ser>"#))
        .collect();
    let chart = read(&chart_xml(false, &format!("<c:pieChart>{series}</c:pieChart>"), ""));
    assert_eq!(chart.series.len(), 300);
}

#[test]
fn chart_cache_budget_bounds_many_empty_series() {
    let series: String = (0..1_000)
        .map(|i| format!(r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/><c:val><c:numLit><c:ptCount val="1048576"/></c:numLit></c:val></c:ser>"#))
        .collect();
    let root = crate::xml::parse(chart_xml(false, &format!("<c:pieChart>{series}</c:pieChart>"), "").as_bytes()).unwrap();
    let (chart, _, warned) = crate::chart::read_chart_with_budget(&root).unwrap();
    assert_eq!(chart.series.len(), 1_000);
    assert!(chart.series.iter().map(|s| s.values.split(',').count()).sum::<usize>() <= 16_777_216 + chart.series.len());
    assert!(warned);
}

#[test]
fn chart_cache_budget_drops_points_beyond_boundary() {
    let series: String = (0..17)
        .map(|i| format!(r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/><c:val><c:numLit><c:ptCount val="1048576"/><c:pt idx="1048575"><c:v>{i}</c:v></c:pt></c:numLit></c:val></c:ser>"#))
        .collect();
    let root = crate::xml::parse(chart_xml(false, &format!("<c:pieChart>{series}</c:pieChart>"), "").as_bytes()).unwrap();
    let (chart, _, warned) = crate::chart::read_chart_with_budget(&root).unwrap();
    assert_eq!(chart.series.len(), 17);
    assert!(chart.series.iter().take(16).all(|s| !s.values.is_empty()));
    assert_eq!(chart.series[16].values, "");
    assert!(warned);
}

#[test]
fn a_deleted_automatic_title_stays_deleted() {
    let one = ser("<c:v>Vehicles</c:v>", &str_cache("S!$A$1:$A$2", &["a", "b"]), &num_cache("S!$B$1:$B$2", 2, &[]));
    let pie = format!("<c:pieChart>{one}</c:pieChart>");
    // A lone series names an automatic title unless the file turned it off.
    assert_eq!(read(&chart_xml(false, &pie, "")).title, None);
    assert_eq!(read(&chart_xml(false, &pie, r#"<c:autoTitleDeleted val="0"/>"#)).title, None);
    let deleted = read(&chart_xml(false, &pie, r#"<c:autoTitleDeleted val="1"/>"#));
    assert_eq!(deleted.title.as_deref(), Some(""));
    // An explicit title wins over the flag.
    let t = r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>Risk</a:t></a:r></a:p></c:rich></c:tx></c:title><c:autoTitleDeleted val="1"/>"#;
    assert_eq!(read(&chart_xml(false, &pie, t)).title.as_deref(), Some("Risk"));

    // Both states survive a save.
    let wb = Workbook::new();
    assert_eq!(read(&crate::chart::write_chart(&wb, 0, &deleted, "")).title.as_deref(), Some(""));
    let auto = read(&chart_xml(false, &pie, ""));
    assert_eq!(read(&crate::chart::write_chart(&wb, 0, &auto, "")).title, None);
}
