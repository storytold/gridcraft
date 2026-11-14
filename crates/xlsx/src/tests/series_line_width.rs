//! The width of a series line (`<c:ser><c:spPr><a:ln w>`): read, bounded and written back.

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Chart, ChartKind, Color, LegendPos, Series, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn workbook(kind: ChartKind, color: Option<Color>, line_width: Option<f32>) -> Workbook {
    let mut wb = Workbook::default();
    let mut s = Sheet::new("Data");
    s.charts.push(Chart {
        id: 1,
        kind,
        anchor: Anchor { cell: CellRef::new(0, 3), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series { values: "Data!$B$2:$B$5".into(), color, line_width, ..Series::default() }],
        legend: LegendPos::Bottom,
        data_labels: false,
        data_table: None,
        gridlines: true,
        style: 2,
        hole_size: 75,
        first_slice_angle: 0,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    });
    wb.sheets = vec![Arc::new(s)];
    wb
}

fn chart_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("xl/charts/chart1.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// The file with the chart's `<a:ln w>` set to `w`.
fn with_width(w: &str) -> Vec<u8> {
    let bytes = write_xlsx(&workbook(ChartKind::Line, Some(Color::Rgb(0x00B0F0)), Some(5.0))).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if f.name() == "xl/charts/chart1.xml" {
            data = String::from_utf8(data).unwrap().replace(r#"w="63500""#, &format!(r#"w="{w}""#)).into_bytes();
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut out, &data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn read_width(bytes: &[u8]) -> Option<f32> {
    read_xlsx(bytes).unwrap().0.sheets[0].charts[0].series[0].line_width
}

#[test]
fn a_series_line_width_round_trips() {
    for kind in [ChartKind::Line, ChartKind::ScatterLines, ChartKind::Radar] {
        let bytes = write_xlsx(&workbook(kind, Some(Color::Rgb(0x00B0F0)), Some(5.0))).unwrap();
        assert!(chart_xml(&bytes).contains(r#"<a:ln w="63500""#), "{kind:?}");
        assert_eq!(read_width(&bytes), Some(5.0), "{kind:?}");
    }
    // With the automatic colour, the width alone is written.
    let bytes = write_xlsx(&workbook(ChartKind::Line, None, Some(0.75))).unwrap();
    assert!(chart_xml(&bytes).contains(r#"<c:spPr><a:ln w="9525" cap="rnd"><a:round/></a:ln></c:spPr>"#));
    assert_eq!(read_width(&bytes), Some(0.75));
    // No width keeps Excel's default line.
    let bytes = write_xlsx(&workbook(ChartKind::Line, Some(Color::Rgb(0x00B0F0)), None)).unwrap();
    assert!(chart_xml(&bytes).contains(r#"<a:ln w="28575""#));
}

#[test]
fn a_series_line_width_from_a_file_is_bounded() {
    assert_eq!(read_width(&with_width("0")), Some(0.0), "a hairline");
    assert_eq!(read_width(&with_width("-12700")), None);
    assert_eq!(read_width(&with_width("NaN")), None);
    assert_eq!(read_width(&with_width("99999999999")), Some(1584.0), "DrawingML's widest line");
}
