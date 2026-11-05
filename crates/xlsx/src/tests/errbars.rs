//! Chart error bars (`<c:errBars>`) round-trip through XLSX in every chart group that has them.

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{
    Anchor, Chart, ChartKind, Color, ErrorAmount, ErrorBarDirection, ErrorBarSides, ErrorBars, LegendPos, LineDash, Series, Sheet, Trendline,
    Workbook,
};

use crate::{read_xlsx, write_xlsx};

fn y_bars() -> ErrorBars {
    ErrorBars {
        direction: ErrorBarDirection::Y,
        sides: ErrorBarSides::Plus,
        amount: ErrorAmount::Custom { plus: Some("Data!$C$2:$C$6".into()), minus: Some("{0.5,1,1.5,2,2.5}".into()) },
        no_end_cap: true,
        color: Some(Color::Rgb(0xC00000)),
        width: Some(1.5),
        dash: Some(LineDash::Dash),
    }
}

fn x_bars() -> ErrorBars {
    ErrorBars { direction: ErrorBarDirection::X, sides: ErrorBarSides::Minus, amount: ErrorAmount::StdDev(2.0), ..ErrorBars::default() }
}

/// Every amount type, each in Y.
fn amounts() -> Vec<ErrorAmount> {
    vec![
        ErrorAmount::Fixed(1.5),
        ErrorAmount::Percentage(10.0),
        ErrorAmount::StdDev(1.0),
        ErrorAmount::StdErr,
        ErrorAmount::Custom { plus: Some("Data!$C$2:$C$6".into()), minus: None },
    ]
}

fn chart(id: u32, kind: ChartKind, error_bars: Vec<ErrorBars>) -> Chart {
    Chart {
        id,
        kind,
        anchor: Anchor { cell: CellRef::new(id * 16, 3), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series {
            name: Some("Data!$B$1".into()),
            categories: Some("Data!$A$2:$A$6".into()),
            values: "Data!$B$2:$B$6".into(),
            color: Some(Color::Rgb(0x4472C4)),
            trendlines: vec![Trendline::default()],
            error_bars,
            ..Series::default()
        }],
        legend: LegendPos::Bottom,
        data_labels: false,
        data_table: None,
        gridlines: true,
        style: 2,
        hole_size: 50,
        first_slice_angle: 0,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    }
}

/// Chart groups whose series hold one set of error bars (CT_BarSer, CT_LineSer) and those that
/// hold one per direction (CT_AreaSer, CT_ScatterSer, CT_BubbleSer).
const ONE: [ChartKind; 6] =
    [ChartKind::ColumnClustered, ChartKind::ColumnStacked, ChartKind::BarClustered, ChartKind::Line, ChartKind::LineMarkers, ChartKind::LineStacked];
const TWO: [ChartKind; 5] = [ChartKind::Area, ChartKind::AreaStacked, ChartKind::Scatter, ChartKind::ScatterLines, ChartKind::Bubble];

fn data_sheet() -> Sheet {
    let mut s = Sheet::new("Data");
    s.set_value(CellRef::new(0, 1), Value::Text("Sales".into()));
    for r in 1..=5u32 {
        s.set_value(CellRef::new(r, 0), Value::Number(f64::from(r)));
        s.set_value(CellRef::new(r, 1), Value::Number(f64::from(r * r)));
        s.set_value(CellRef::new(r, 2), Value::Number(f64::from(r) / 4.0));
    }
    s
}

fn workbook(charts: Vec<Chart>) -> Workbook {
    let mut s = data_sheet();
    s.charts = charts;
    Workbook { sheets: vec![Arc::new(s)], ..Workbook::default() }
}

fn chart_parts(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut names: Vec<String> =
        (0..z.len()).map(|i| z.name_for_index(i).unwrap().to_string()).filter(|n| n.starts_with("xl/charts/chart")).collect();
    // chart2.xml before chart10.xml.
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

#[test]
fn every_amount_type_round_trips_in_every_chart_group() {
    let mut charts = Vec::new();
    let mut want = Vec::new();
    for k in ONE.iter().chain(TWO.iter()) {
        for a in amounts() {
            let e = ErrorBars { amount: a, ..ErrorBars::default() };
            charts.push(chart(charts.len() as u32 + 1, *k, vec![e.clone()]));
            want.push((*k, vec![e]));
        }
        // Every option at once.
        charts.push(chart(charts.len() as u32 + 1, *k, vec![y_bars()]));
        want.push((*k, vec![y_bars()]));
    }
    for k in TWO {
        // X and Y together.
        charts.push(chart(charts.len() as u32 + 1, k, vec![x_bars(), y_bars()]));
        want.push((k, vec![x_bars(), y_bars()]));
    }
    let bytes = write_xlsx(&workbook(charts)).unwrap();
    let (back, _rep) = read_xlsx(&bytes).unwrap();
    let got = &back.sheets[0].charts;
    assert_eq!(got.len(), want.len());
    for (c, (k, bars)) in got.iter().zip(&want) {
        assert_eq!(c.kind, *k);
        assert_eq!(&c.series[0].error_bars, bars, "{k:?}");
        assert_eq!(c.series[0].trendlines.len(), 1, "{k:?}: the trendline is kept beside the error bars");
    }
    // A second save writes the same parts (nothing drifts on the way round).
    assert_eq!(chart_parts(&write_xlsx(&back).unwrap()), chart_parts(&bytes));
}

#[test]
fn error_bars_follow_the_series_schema_sequence() {
    let mut charts: Vec<Chart> = ONE.iter().map(|k| chart(0, *k, vec![y_bars()])).collect();
    charts.extend(TWO.iter().map(|k| chart(0, *k, vec![x_bars(), y_bars()])));
    for (i, c) in charts.iter_mut().enumerate() {
        c.id = i as u32 + 1;
    }
    let kinds: Vec<ChartKind> = charts.iter().map(|c| c.kind).collect();
    let parts = chart_parts(&write_xlsx(&workbook(charts)).unwrap());
    assert_eq!(parts.len(), kinds.len());
    for (xml, k) in parts.iter().zip(&kinds) {
        let at = |tag: &str| xml.find(tag).unwrap_or_else(|| panic!("{k:?}: no {tag}"));
        let first = at("<c:errBars>");
        // After the trendlines, before the data references.
        let data = if matches!(k, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble) { "<c:xVal>" } else { "<c:cat>" };
        assert!(at("</c:trendline>") < first && first < at(data), "{k:?}");
        let n = if TWO.contains(k) { 2 } else { 1 };
        assert_eq!(xml.matches("<c:errBars>").count(), n, "{k:?}");
    }
    // Column and bar series leave errDir out, as Excel writes them; the others name it.
    assert!(!parts[0].contains("<c:errDir") && !parts[2].contains("<c:errDir"));
    assert_eq!(kinds[3], ChartKind::Line);
    // Children in CT_ErrBars order.
    let xml = &parts[3];
    let e = &xml[xml.find("<c:errBars>").unwrap()..];
    let e = &e[..e.find("</c:errBars>").unwrap()];
    let pos: Vec<usize> = [
        "<c:errDir val=\"y\"/>",
        "<c:errBarType val=\"plus\"/>",
        "<c:errValType val=\"cust\"/>",
        "<c:noEndCap val=\"1\"/>",
        "<c:plus><c:numRef><c:f>Data!$C$2:$C$6</c:f><c:numCache>",
        "<c:minus><c:numLit>",
        "<c:spPr><a:ln w=\"19050\"",
        "<a:prstDash val=\"dash\"/>",
    ]
    .iter()
    .map(|t| e.find(t).unwrap_or_else(|| panic!("missing {t} in {e}")))
    .collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{e}");
    // A value amount writes c:val after the type.
    let parts = chart_parts(
        &write_xlsx(&workbook(vec![chart(1, ChartKind::Line, vec![ErrorBars { amount: ErrorAmount::Percentage(7.5), ..ErrorBars::default() }])]))
            .unwrap(),
    );
    assert!(parts[0].contains("<c:errValType val=\"percentage\"/><c:noEndCap val=\"0\"/><c:val val=\"7.5\"/></c:errBars>"), "{}", parts[0]);
}

#[test]
fn groups_without_error_bars_write_none_and_one_set_groups_keep_the_y_bars() {
    let charts = vec![
        chart(1, ChartKind::Pie, vec![y_bars()]),
        chart(2, ChartKind::Radar, vec![y_bars()]),
        // A column series holds only Y bars: X bars are left out, the Y bars kept.
        chart(3, ChartKind::ColumnClustered, vec![x_bars(), y_bars()]),
    ];
    let bytes = write_xlsx(&workbook(charts)).unwrap();
    let parts = chart_parts(&bytes);
    assert!(!parts[0].contains("errBars") && !parts[1].contains("errBars"));
    assert_eq!(parts[2].matches("<c:errBars>").count(), 1);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheets[0].charts[2].series[0].error_bars, vec![y_bars()]);
}

#[test]
fn excel_style_error_bars_are_read() {
    // As Excel writes them: theme-coloured line, an empty noEndCap (true), a custom amount with a
    // literal minus, a value type left out (fixed) with no c:val (1), an unknown type that is
    // skipped and a second set in a direction already taken that is dropped.
    let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:scatterChart><c:scatterStyle val="lineMarker"/><c:ser><c:idx val="0"/><c:order val="0"/>
<c:errBars><c:errDir val="x"/><c:errBarType val="minus"/><c:errValType val="cust"/><c:noEndCap/><c:plus><c:numRef><c:f>=Data!$C$2:$C$6</c:f></c:numRef></c:plus><c:minus><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>0.5</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:minus><c:spPr><a:ln w="12700" cap="flat"><a:solidFill><a:schemeClr val="tx1"><a:lumMod val="65000"/><a:lumOff val="35000"/></a:schemeClr></a:solidFill><a:round/></a:ln></c:spPr></c:errBars>
<c:errBars><c:errDir val="y"/><c:errBarType val="both"/><c:errValType val="fancy"/></c:errBars>
<c:errBars><c:errBarType val="plus"/></c:errBars>
<c:errBars><c:errDir val="y"/><c:errValType val="stdErr"/></c:errBars>
<c:xVal><c:numRef><c:f>Data!$A$2:$A$6</c:f></c:numRef></c:xVal><c:yVal><c:numRef><c:f>Data!$B$2:$B$6</c:f></c:numRef></c:yVal></c:ser><c:axId val="1"/><c:axId val="2"/></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
    let root = crate::xml::parse(xml.as_bytes()).unwrap();
    let c = crate::chart::read_chart(&root).unwrap();
    let e = &c.series[0].error_bars;
    assert_eq!(e.len(), 2, "{e:?}");
    assert_eq!(e[0].direction, ErrorBarDirection::X);
    assert_eq!(e[0].sides, ErrorBarSides::Minus);
    assert!(e[0].no_end_cap, "an empty CT_Boolean means true");
    assert_eq!(e[0].amount, ErrorAmount::Custom { plus: Some("Data!$C$2:$C$6".into()), minus: Some("{0.5,2}".into()) });
    assert_eq!(e[0].width, Some(1.0));
    assert!(matches!(e[0].color, Some(Color::Theme(..))), "{:?}", e[0].color);
    // The direction defaults to Y and the type to a fixed value of 1; the later Y set is dropped.
    assert_eq!((e[1].direction, e[1].sides, e[1].amount.clone()), (ErrorBarDirection::Y, ErrorBarSides::Plus, ErrorAmount::Fixed(1.0)));
}

#[test]
fn hostile_error_bar_values_do_not_break_the_file() {
    let bars = vec![
        ErrorBars { amount: ErrorAmount::Fixed(f64::NAN), ..ErrorBars::default() },
        ErrorBars { direction: ErrorBarDirection::X, amount: ErrorAmount::Percentage(f64::INFINITY), width: Some(f32::NAN), ..ErrorBars::default() },
    ];
    let custom =
        vec![ErrorBars { amount: ErrorAmount::Custom { plus: Some("   ".into()), minus: Some("not a ref".into()) }, ..ErrorBars::default() }];
    let wb = workbook(vec![chart(1, ChartKind::Scatter, bars), chart(2, ChartKind::Line, custom)]);
    let bytes = write_xlsx(&wb).unwrap();
    let parts = chart_parts(&bytes);
    assert!(!parts[0].contains("NaN") && !parts[0].contains("inf"), "{}", parts[0]);
    assert!(!parts[1].contains("errBars"), "custom bars with nothing writable are left out: {}", parts[1]);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheets[0].charts.len(), 2);
}

#[test]
fn sparse_literal_points_keep_their_index_through_read_and_save() {
    // Custom amounts for points 0 and 2 of 4 (1 and 3 left out), and categories with a hole at 1.
    let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:ser><c:idx val="0"/><c:order val="0"/>
<c:errBars><c:errDir val="y"/><c:errBarType val="plus"/><c:errValType val="cust"/><c:plus><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="4"/><c:pt idx="0"><c:v>5</c:v></c:pt><c:pt idx="2"><c:v>7</c:v></c:pt></c:numLit></c:plus></c:errBars>
<c:cat><c:strLit><c:ptCount val="3"/><c:pt idx="0"><c:v>a</c:v></c:pt><c:pt idx="2"><c:v>c</c:v></c:pt></c:strLit></c:cat>
<c:val><c:numRef><c:f>Data!$B$2:$B$6</c:f></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
    let c = crate::chart::read_chart(&crate::xml::parse(xml.as_bytes()).unwrap()).unwrap();
    let s = &c.series[0];
    assert_eq!(s.error_bars[0].amount, ErrorAmount::Custom { plus: Some(r#"{5,"",7,""}"#.into()), minus: None });
    assert_eq!(s.categories.as_deref(), Some(r#"{"a","","c"}"#));
    // Saving writes the points back at their own indices under the same ptCount.
    let wb = workbook(vec![Chart { series: vec![s.clone()], ..chart(1, ChartKind::Line, vec![]) }]);
    let part = chart_parts(&write_xlsx(&wb).unwrap()).remove(0);
    assert!(part.contains(r#"<c:ptCount val="4"/><c:pt idx="0"><c:v>5</c:v></c:pt><c:pt idx="2"><c:v>7</c:v></c:pt></c:numLit>"#), "{part}");
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheets[0].charts[0].series[0].error_bars, s.error_bars);
}

#[test]
fn a_theme_coloured_bar_line_keeps_its_scheme_name() {
    let e = ErrorBars { color: Some(Color::Theme(4, 0)), ..ErrorBars::default() };
    let wb = workbook(vec![chart(1, ChartKind::Line, vec![e.clone()])]);
    let bytes = write_xlsx(&wb).unwrap();
    assert!(chart_parts(&bytes)[0].contains(r#"<a:schemeClr val="accent1"/>"#));
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheets[0].charts[0].series[0].error_bars, vec![e]);
}
