//! Per-point chart formatting (`c:dPt`), Invert if Negative (with the c14 inverted fill) and
//! vary colors by point: read, written in schema order, and round-tripped.

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Chart, ChartKind, Color, DataPoint, LegendPos, MarkerFormat, MarkerSymbol, Series, Sheet, Workbook};

use crate::chart::read_chart;
use crate::xml::parse;
use crate::{read_xlsx, write_xlsx};

const RED: Color = Color::Rgb(0xFF0000);
const BLUE: Color = Color::Rgb(0x0000FF);
const GREEN: Color = Color::Rgb(0x008000);
const YELLOW: Color = Color::Rgb(0xFFFF00);

fn chart(id: u32, kind: ChartKind, series: Vec<Series>) -> Chart {
    Chart {
        id,
        kind,
        anchor: Anchor { cell: CellRef::new(id * 16, 4), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series,
        legend: LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        vary_colors: None,
    }
}

fn series(col: char) -> Series {
    Series {
        name: Some(format!("Data!${col}$1")),
        categories: Some("Data!$A$2:$A$5".into()),
        values: format!("Data!${col}$2:${col}$5"),
        ..Series::default()
    }
}

fn book(charts: Vec<Chart>) -> Workbook {
    let mut wb = Workbook::default();
    let mut s = Sheet::new("Data");
    for (r, row) in [["", "A", "B"], ["w", "5", "4"], ["x", "-3", "-2"], ["y", "4", "6"], ["z", "-2", "3"]].iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let cell = CellRef::new(r as u32, c as u32);
            match v.parse::<f64>() {
                Ok(n) => s.set_value(cell, Value::Number(n)),
                Err(_) if !v.is_empty() => s.set_value(cell, Value::Text((*v).into())),
                Err(_) => {}
            }
        }
    }
    s.charts = charts;
    wb.sheets = vec![Arc::new(s)];
    wb
}

/// The written chart parts, in part-name order.
fn chart_parts(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = z.file_names().filter(|n| n.starts_with("xl/charts/chart")).map(str::to_string).collect();
    names.sort_by_key(|n| n.trim_start_matches("xl/charts/chart").trim_end_matches(".xml").parse::<u32>().unwrap_or(0));
    names
        .iter()
        .map(|n| {
            let mut s = String::new();
            z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
            s
        })
        .collect()
}

fn round_trip(charts: Vec<Chart>) -> (Vec<Chart>, Vec<String>) {
    let bytes = write_xlsx(&book(charts)).unwrap();
    let parts = chart_parts(&bytes);
    let (back, _) = read_xlsx(&bytes).unwrap();
    (back.sheets[0].charts.clone(), parts)
}

fn pos(hay: &str, needle: &str) -> usize {
    hay.find(needle).unwrap_or_else(|| panic!("`{needle}` missing from {hay}"))
}

#[test]
fn bar_points_invert_and_inverted_fill_round_trip() {
    let mut a = series('B');
    a.invert_if_negative = true;
    a.points = vec![
        DataPoint { index: 0, fill: Some(RED), ..DataPoint::default() },
        DataPoint { index: 2, line: Some(BLUE), ..DataPoint::default() },
        DataPoint { index: 3, fill: Some(RED), invert_if_negative: Some(false), ..DataPoint::default() },
    ];
    let mut b = series('C');
    b.color = Some(GREEN);
    b.invert_if_negative = true;
    b.invert_color = Some(YELLOW);
    let (back, parts) = round_trip(vec![chart(1, ChartKind::ColumnClustered, vec![a.clone(), b.clone()])]);
    assert_eq!(back[0].series[0].points, a.points);
    assert!(back[0].series[0].invert_if_negative);
    assert_eq!(back[0].series[0].invert_color, None, "automatic fill: no inverted colour written");
    assert_eq!(back[0].series[1].invert_color, Some(YELLOW));
    assert!(back[0].series[1].invert_if_negative);
    assert_eq!(back[0].vary_colors, None);

    let x = &parts[0];
    // CT_BarSer: spPr, invertIfNegative, dPt*, dLbls, trendline, cat, val, ..., extLst.
    let ser = &x[pos(x, "<c:ser>")..];
    assert!(pos(ser, "<c:invertIfNegative val=\"1\"/>") < pos(ser, "<c:dPt>"));
    assert!(pos(ser, "<c:dPt>") < pos(ser, "<c:cat>"));
    // CT_DPt: idx, invertIfNegative, marker, bubble3D, explosion, spPr.
    let dpt = &ser[pos(ser, "<c:dPt>")..pos(ser, "</c:dPt>")];
    assert!(pos(dpt, "<c:idx val=\"0\"/>") < pos(dpt, "<c:invertIfNegative"));
    assert!(pos(dpt, "<c:invertIfNegative") < pos(dpt, "<c:bubble3D"));
    assert!(pos(dpt, "<c:bubble3D") < pos(dpt, "<c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/>"));
    assert!(x.contains("<c:dPt><c:idx val=\"3\"/><c:invertIfNegative val=\"0\"/>"), "a point's own invert setting is written");
    // The c14 inverted fill ends the second series, after its values.
    let second = &x[x.rfind("<c:ser>").unwrap()..];
    assert!(pos(second, "</c:val>") < pos(second, "{6F2FDCE9-48DA-4B69-8628-5D25D57E5C99}"));
    assert!(second.contains("<c14:invertSolidFillFmt><c14:spPr><a:solidFill><a:srgbClr val=\"FFFF00\"/>"));
    assert_eq!(x.matches("invertSolidFillFmt>").count(), 2, "only the series with its own fill gets the extension");
}

#[test]
fn explicit_fill_without_inverted_colour_writes_white() {
    let mut s = series('B');
    s.color = Some(GREEN);
    s.invert_if_negative = true;
    let (back, parts) = round_trip(vec![chart(1, ChartKind::BarClustered, vec![s])]);
    assert!(parts[0].contains("<c14:spPr><a:solidFill><a:srgbClr val=\"FFFFFF\"/>"));
    assert_eq!(back[0].series[0].invert_color, Some(Color::Rgb(0xFFFFFF)));
}

#[test]
fn line_and_scatter_point_markers_and_segments_round_trip() {
    let marker = MarkerFormat { symbol: MarkerSymbol::Square, size: Some(9), fill: Some(RED), line: Some(Color::Rgb(0)) };
    let mut s = series('B');
    s.points = vec![
        DataPoint { index: 1, marker: Some(marker.clone()), ..DataPoint::default() },
        DataPoint { index: 3, line: Some(GREEN), ..DataPoint::default() },
    ];
    let mut sc = series('C');
    sc.points = vec![DataPoint { index: 2, marker: Some(MarkerFormat { fill: Some(RED), ..MarkerFormat::default() }), ..DataPoint::default() }];
    let (back, parts) = round_trip(vec![
        chart(1, ChartKind::LineMarkers, vec![s.clone()]),
        chart(2, ChartKind::Scatter, vec![sc.clone()]),
        chart(3, ChartKind::Line, vec![s.clone()]),
    ]);
    assert_eq!(back[0].series[0].points, s.points);
    assert_eq!(back[1].series[0].points, sc.points);
    assert_eq!(back[2].series[0].points, s.points, "a line without markers keeps its point markers");
    let x = &parts[0];
    let dpt = &x[pos(x, "<c:dPt><c:idx val=\"1\"/>")..];
    assert!(pos(dpt, "<c:marker><c:symbol val=\"square\"/><c:size val=\"9\"/><c:spPr>") < pos(dpt, "<c:bubble3D"));
    assert!(!dpt[..pos(dpt, "</c:dPt>")].contains("invertIfNegative"), "CT_LineSer points have no invertIfNegative");
    assert!(
        x.contains("<c:dPt><c:idx val=\"3\"/><c:bubble3D val=\"0\"/><c:spPr><a:ln w=\"28575\" cap=\"rnd\"><a:solidFill><a:srgbClr val=\"008000\"/>")
    );
    // The no-marker line writes its series marker before the points.
    let y = &parts[2];
    assert!(pos(y, "<c:marker><c:symbol val=\"none\"/></c:marker>") < pos(y, "<c:dPt>"));
}

#[test]
fn pie_slices_explosion_and_vary_colors_round_trip() {
    let mut s = series('B');
    s.points = vec![
        DataPoint { index: 1, fill: Some(RED), ..DataPoint::default() },
        DataPoint { index: 2, explosion: Some(25), line: Some(BLUE), ..DataPoint::default() },
    ];
    let mut plain = chart(2, ChartKind::Doughnut, vec![series('B')]);
    plain.vary_colors = Some(false);
    let (back, parts) = round_trip(vec![chart(1, ChartKind::Pie, vec![s.clone()]), plain]);
    assert_eq!(back[0].series[0].points, s.points);
    assert_eq!(back[0].vary_colors, None, "pie default (on) is not stored");
    assert_eq!(back[1].vary_colors, Some(false));
    assert!(parts[0].contains("<c:pieChart><c:varyColors val=\"1\"/>"));
    assert!(parts[1].contains("<c:doughnutChart><c:varyColors val=\"0\"/>"));
    let x = &parts[0];
    let dpt = &x[pos(x, "<c:dPt><c:idx val=\"2\"/>")..];
    assert!(pos(dpt, "<c:bubble3D val=\"0\"/>") < pos(dpt, "<c:explosion val=\"25\"/>"));
    assert!(pos(dpt, "<c:explosion val=\"25\"/>") < pos(dpt, "<c:spPr>"));
    assert!(!x.contains("invertIfNegative"), "CT_PieSer has no invertIfNegative");
}

#[test]
fn vary_colors_on_a_column_chart_round_trips() {
    let mut c = chart(1, ChartKind::ColumnClustered, vec![series('B')]);
    c.vary_colors = Some(true);
    let (back, parts) = round_trip(vec![c]);
    assert_eq!(back[0].vary_colors, Some(true));
    assert!(parts[0].contains("<c:varyColors val=\"1\"/>"));
    // New charts keep writing the kind default.
    let (_, parts) = round_trip(vec![chart(1, ChartKind::LineMarkers, vec![series('B')])]);
    assert!(parts[0].contains("<c:varyColors val=\"0\"/>"));
}

/// A one-group chart part around `group` (a `<c:barChart>`, `<c:pieChart>`...).
fn read_group(group: &str) -> Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:c14=\"http://schemas.microsoft.com/office/drawing/2007/8/2/chart\"><c:chart><c:plotArea>{group}<c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

const VAL: &str = "<c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val>";

#[test]
fn absent_vary_colors_is_on() {
    let bar = read_group(&format!(
        "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{VAL}</c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>"
    ));
    assert_eq!(bar.vary_colors, Some(true));
    let pie = read_group(&format!("<c:pieChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{VAL}</c:ser></c:pieChart>"));
    assert_eq!(pie.vary_colors, None, "on is the pie default");
    let off = read_group(&format!(
        "<c:barChart><c:barDir val=\"col\"/><c:varyColors val=\"0\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{VAL}</c:ser></c:barChart>"
    ));
    assert_eq!(off.vary_colors, None);
}

#[test]
fn excel_shaped_points_read() {
    // The shapes Excel writes: an invertIfNegative echo and bubble3D in every bar point, a
    // c16 id, an effect list, a border with no line, and a point that only echoes the series.
    let c = read_group(&format!(
        "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
         <c:spPr><a:solidFill><a:srgbClr val=\"800080\"/></a:solidFill></c:spPr><c:invertIfNegative val=\"1\"/>\
         <c:dPt><c:idx val=\"4\"/><c:invertIfNegative val=\"1\"/><c:bubble3D val=\"0\"/><c:spPr><a:solidFill><a:schemeClr val=\"accent2\"/></a:solidFill><a:ln><a:noFill/></a:ln><a:effectLst/></c:spPr><c:extLst><c:ext uri=\"{{C3380CC4-5D6E-409C-BE32-E72D297353CC}}\"/></c:extLst></c:dPt>\
         <c:dPt><c:idx val=\"1\"/><c:invertIfNegative val=\"1\"/><c:bubble3D val=\"0\"/></c:dPt>\
         <c:dPt><c:idx val=\"2\"/><c:invertIfNegative val=\"0\"/><c:bubble3D val=\"0\"/></c:dPt>\
         <c:dPt><c:bubble3D val=\"0\"/></c:dPt>\
         {VAL}<c:extLst><c:ext uri=\"{{6F2FDCE9-48DA-4B69-8628-5D25D57E5C99}}\"><c14:invertSolidFillFmt><c14:spPr><a:solidFill><a:srgbClr val=\"FFFF00\"/></a:solidFill></c14:spPr></c14:invertSolidFillFmt></c:ext></c:extLst></c:ser>\
         <c:gapWidth val=\"150\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>"
    ));
    let s = &c.series[0];
    assert!(s.invert_if_negative);
    assert_eq!(s.invert_color, Some(YELLOW));
    assert_eq!(
        s.points,
        vec![
            DataPoint { index: 2, invert_if_negative: Some(false), ..DataPoint::default() },
            DataPoint { index: 4, fill: Some(Color::Theme(5, 0)), ..DataPoint::default() },
        ],
        "sorted; the echo-only point and the point without idx are dropped"
    );
}

#[test]
fn hostile_point_lists_are_bounded() {
    let mut dpts = String::new();
    for i in 0..6000u32 {
        // Duplicates, out-of-range values and a huge explosion.
        let idx = if i % 2 == 0 { i } else { u32::MAX };
        dpts.push_str(&format!(
            "<c:dPt><c:idx val=\"{idx}\"/><c:marker><c:size val=\"999\"/></c:marker><c:explosion val=\"4000000000\"/><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:dPt>"
        ));
    }
    let c =
        read_group(&format!("<c:pieChart><c:varyColors val=\"1\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{dpts}{VAL}</c:ser></c:pieChart>"));
    let pts = c.series[0].points.clone();
    assert!(pts.len() <= crate::chart::MAX_POINT_FORMATS);
    assert!(pts.windows(2).all(|w| w[0].index < w[1].index), "sorted, one per index");
    assert!(pts.iter().all(|p| p.explosion.is_some_and(|e| e <= 400)));
    // Written back without panicking, and the point indexes survive.
    let (back, _) = round_trip(vec![c]);
    assert_eq!(back[0].series[0].points.len(), pts.len());
}

#[test]
fn combo_charts_write_vary_colors_on_the_first_group_only() {
    let mut line = series('C');
    line.kind = Some(ChartKind::Line);
    let mut c = chart(1, ChartKind::Combo, vec![series('B'), line]);
    c.vary_colors = Some(true);
    let (back, parts) = round_trip(vec![c]);
    let x = &parts[0];
    assert!(x.contains("<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"1\"/>"));
    assert!(x.contains("<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>"));
    assert_eq!(back[0].vary_colors, Some(true));
}

#[test]
fn repeated_indices_do_not_use_up_the_point_cap() {
    // Thousands of entries for index 0, then a distinct point: the cap counts distinct indices,
    // so the later point is kept.
    let mut dpts = String::new();
    for _ in 0..(crate::chart::MAX_POINT_FORMATS + 904) {
        dpts.push_str("<c:dPt><c:idx val=\"0\"/><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:dPt>");
    }
    dpts.push_str("<c:dPt><c:idx val=\"7\"/><c:spPr><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></c:spPr></c:dPt>");
    let c =
        read_group(&format!("<c:pieChart><c:varyColors val=\"1\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{dpts}{VAL}</c:ser></c:pieChart>"));
    let idx: Vec<u32> = c.series[0].points.iter().map(|p| p.index).collect();
    assert_eq!(idx, vec![0, 7]);
}
