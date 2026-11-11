//! The chart area's border (`c:chartSpace/c:spPr/a:ln`): none, a line of its own, or Excel's
//! automatic one, read and written back.

use std::io::{Read, Write};
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Chart, ChartBorder, ChartKind, Color, LegendPos, Series, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn chart(id: u32, border: ChartBorder) -> Chart {
    Chart {
        id,
        kind: ChartKind::ColumnClustered,
        anchor: Anchor { cell: CellRef::new(id * 16, 3), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series {
            name: Some("Data!$B$1".into()),
            categories: Some("Data!$A$2:$A$4".into()),
            values: "Data!$B$2:$B$4".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            quartiles: Default::default(),
            smooth: false,
            trendlines: Vec::new(),
        }],
        legend: LegendPos::Bottom,
        data_labels: false,
        data_table: None,
        gridlines: true,
        style: 2,
        hole_size: 75,
        first_slice_angle: 0,
        border,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    }
}

fn borders() -> Vec<ChartBorder> {
    vec![
        ChartBorder::Auto,
        ChartBorder::None,
        ChartBorder::Line { color: Some(Color::Rgb(0xFF0000)), width: Some(3.0) },
        ChartBorder::Line { color: None, width: Some(0.75) },
        ChartBorder::Line { color: Some(Color::Rgb(0x1F4E79)), width: None },
    ]
}

fn workbook(borders: &[ChartBorder]) -> Workbook {
    let mut s = Sheet::new("Data");
    s.set_value(CellRef::new(0, 1), Value::Text("Sales".into()));
    for r in 1..=3u32 {
        s.set_value(CellRef::new(r, 0), Value::Text(format!("c{r}").into()));
        s.set_value(CellRef::new(r, 1), Value::Number(f64::from(r)));
    }
    for (i, b) in borders.iter().enumerate() {
        s.charts.push(chart(i as u32 + 1, b.clone()));
    }
    Workbook { sheets: vec![Arc::new(s)], ..Workbook::default() }
}

/// The chart parts, by name.
fn chart_parts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut names: Vec<String> =
        (0..z.len()).map(|i| z.name_for_index(i).unwrap().to_string()).filter(|n| n.starts_with("xl/charts/chart")).collect();
    names.sort_by_key(|n| n.trim_start_matches("xl/charts/chart").trim_end_matches(".xml").parse::<u32>().unwrap_or(0));
    names
        .into_iter()
        .map(|n| {
            let mut s = String::new();
            z.by_name(&n).unwrap().read_to_string(&mut s).unwrap();
            (n, s)
        })
        .collect()
}

/// `bytes` with the first chart part's chart-space `c:spPr` replaced by `sp_pr`.
fn with_chart_space_sp_pr(bytes: &[u8], sp_pr: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == "xl/charts/chart1.xml" {
            let xml = String::from_utf8(data).unwrap();
            let (head, tail) = xml.split_once("</c:chart>").unwrap();
            let tail = match (tail.find("<c:spPr>"), tail.find("</c:spPr>")) {
                (Some(a), Some(b)) => format!("{}{}", &tail[..a], &tail[b + "</c:spPr>".len()..]),
                _ => tail.to_string(),
            };
            data = format!("{head}</c:chart>{sp_pr}{tail}").into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn chart_area_borders_round_trip() {
    let bytes = write_xlsx(&workbook(&borders())).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let read: Vec<ChartBorder> = back.sheets[0].charts.iter().map(|c| c.border.clone()).collect();
    assert_eq!(read, borders());
    // A second save writes the same parts.
    assert_eq!(chart_parts(&write_xlsx(&back).unwrap()), chart_parts(&bytes));
}

/// The automatic border writes no `c:spPr`; the others write theirs after `c:chart`, as the
/// chart space's schema orders them.
#[test]
fn chart_area_border_xml() {
    let parts = chart_parts(&write_xlsx(&workbook(&borders())).unwrap());
    let space_sp_pr = |xml: &str| xml.split_once("</c:chart>").map(|(_, t)| t.to_string()).filter(|t| t.contains("<c:spPr>"));
    assert_eq!(space_sp_pr(&parts[0].1), None, "automatic");
    assert!(space_sp_pr(&parts[1].1).unwrap().starts_with("<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>"));
    assert!(
        space_sp_pr(&parts[2].1)
            .unwrap()
            .starts_with(r#"<c:spPr><a:ln w="38100"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln></c:spPr>"#)
    );
    assert!(space_sp_pr(&parts[3].1).unwrap().starts_with(r#"<c:spPr><a:ln w="9525"></a:ln></c:spPr>"#));
}

/// What Excel files hold: a fill with no line, a line with no fill of its own, a gradient line.
#[test]
fn chart_area_borders_as_excel_writes_them() {
    let bytes = write_xlsx(&workbook(&[ChartBorder::Auto])).unwrap();
    let border = |sp_pr: &str| read_xlsx(&with_chart_space_sp_pr(&bytes, sp_pr)).unwrap().0.sheets[0].charts[0].border.clone();
    // A background and no line: the automatic border.
    assert_eq!(border(r#"<c:spPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></c:spPr>"#), ChartBorder::Auto);
    assert_eq!(border(r#"<c:spPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:ln><a:noFill/></a:ln></c:spPr>"#), ChartBorder::None);
    // A width with no colour: Excel draws it in the automatic colour.
    assert_eq!(border(r#"<c:spPr><a:ln w="9525"/></c:spPr>"#), ChartBorder::Line { color: None, width: Some(0.75) });
    assert!(matches!(
        border(r#"<c:spPr><a:ln w="12700"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:ln></c:spPr>"#),
        ChartBorder::Line { color: Some(_), width: Some(w) } if (w - 1.0).abs() < 1e-6
    ));
    // A gradient line isn't drawn as one: the automatic border stands in.
    assert_eq!(border(r#"<c:spPr><a:ln><a:gradFill><a:gsLst/></a:gradFill></a:ln></c:spPr>"#), ChartBorder::Auto);
    // An empty line element sets nothing.
    assert_eq!(border(r#"<c:spPr><a:ln/></c:spPr>"#), ChartBorder::Auto);
}
