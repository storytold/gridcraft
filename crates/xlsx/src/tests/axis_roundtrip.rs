//! A series on the secondary axis, fixed axis bounds and units, and the secondary axis title
//! round-trip through XLSX, and read back from Excel's own chart XML.

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, AxisScale, Chart, ChartKind, LegendPos, Series, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn series(col: &str, kind: Option<ChartKind>, secondary: bool) -> Series {
    Series {
        name: Some(format!("Data!${col}$1")),
        categories: Some("Data!$A$2:$A$6".into()),
        values: format!("Data!${col}$2:${col}$6"),
        bubble_sizes: None,
        color: None,
        secondary,
        kind,
        smooth: false,
        trendlines: Vec::new(),
    }
}

fn workbook(chart: Chart) -> Workbook {
    let mut s = Sheet::new("Data");
    for (c, name) in ["Date", "Conductivity", "pH"].iter().enumerate() {
        s.set_value(CellRef::new(0, c as u32), Value::Text((*name).into()));
    }
    for r in 1..=5u32 {
        s.set_value(CellRef::new(r, 0), Value::Number(f64::from(r)));
        s.set_value(CellRef::new(r, 1), Value::Number(1000.0 + 300.0 * f64::from(r)));
        s.set_value(CellRef::new(r, 2), Value::Number(7.0 + 0.3 * f64::from(r)));
    }
    s.charts.push(chart);
    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(s)];
    wb
}

fn chart(kind: ChartKind, series: Vec<Series>) -> Chart {
    Chart {
        id: 1,
        kind,
        anchor: Anchor { cell: CellRef::new(8, 1), width: 480.0, height: 300.0, ..Anchor::default() },
        title: Some("Water quality".into()),
        series,
        legend: LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: Some("Conductivity (uS/cm)".into()),
        source: None,
        by_rows: false,
        pivot: None,
        y2_title: Some("pH".into()),
        y_axis: AxisScale { min: Some(0.0), max: Some(3000.0), major_unit: Some(500.0) },
        y2_axis: AxisScale { min: Some(6.0), max: Some(10.0), major_unit: None },
    }
}

fn chart_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut f = z.by_name("xl/charts/chart1.xml").unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    s
}

#[test]
fn combo_chart_with_secondary_axis_bounds_and_title_round_trips() {
    let wb = workbook(chart(ChartKind::Combo, vec![series("B", Some(ChartKind::ColumnClustered), false), series("C", Some(ChartKind::Line), true)]));
    let bytes = write_xlsx(&wb).unwrap();
    let xml = chart_xml(&bytes);
    // CT_Scaling order is orientation, max, min; the major unit follows crossBetween.
    assert!(xml.contains("<c:orientation val=\"minMax\"/><c:max val=\"3000\"/><c:min val=\"0\"/>"), "{xml}");
    assert!(xml.contains("<c:orientation val=\"minMax\"/><c:max val=\"10\"/><c:min val=\"6\"/>"), "{xml}");
    assert!(xml.contains("<c:crossBetween val=\"between\"/><c:majorUnit val=\"500\"/></c:valAx>"), "{xml}");
    assert_eq!(xml.matches("<c:majorUnit").count(), 1, "the secondary axis has an automatic unit: {xml}");
    assert!(xml.contains("<c:axPos val=\"r\"/><c:title>"), "the secondary axis carries its title: {xml}");

    let (back, _) = read_xlsx(&bytes).unwrap();
    let c = &back.sheet(0).unwrap().charts[0];
    assert_eq!(c.kind, ChartKind::Combo);
    assert_eq!(c.series.iter().map(|s| s.secondary).collect::<Vec<_>>(), [false, true]);
    assert_eq!(c.series[1].kind, Some(ChartKind::Line));
    assert_eq!(c.y_title.as_deref(), Some("Conductivity (uS/cm)"));
    assert_eq!(c.y2_title.as_deref(), Some("pH"));
    assert_eq!(c.y_axis, AxisScale { min: Some(0.0), max: Some(3000.0), major_unit: Some(500.0) });
    assert_eq!(c.y2_axis, AxisScale { min: Some(6.0), max: Some(10.0), major_unit: None });
}

#[test]
fn automatic_axes_write_no_bounds_and_read_back_as_auto() {
    let mut ch = chart(ChartKind::ColumnClustered, vec![series("B", None, false), series("C", None, false)]);
    ch.y_axis = AxisScale::default();
    ch.y2_axis = AxisScale::default();
    ch.y2_title = None;
    let bytes = write_xlsx(&workbook(ch)).unwrap();
    let xml = chart_xml(&bytes);
    assert!(!xml.contains("<c:min") && !xml.contains("<c:max") && !xml.contains("<c:majorUnit"), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let c = &back.sheet(0).unwrap().charts[0];
    assert!(c.y_axis.is_auto() && c.y2_axis.is_auto() && c.y2_title.is_none());
}

#[test]
fn a_fixed_bound_on_one_end_only_round_trips() {
    let mut ch = chart(ChartKind::Line, vec![series("C", None, false)]);
    ch.y_axis = AxisScale { min: Some(6.0), max: None, major_unit: None };
    ch.y2_axis = AxisScale::default();
    ch.y2_title = None;
    let bytes = write_xlsx(&workbook(ch)).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().charts[0].y_axis, AxisScale { min: Some(6.0), max: None, major_unit: None });
}

/// The plot area Excel writes for a clustered-column + line combo where the line is on the
/// secondary axis with fixed bounds 6..10 and an axis title (trimmed to what the reader uses).
const EXCEL_PLOT: &str = r#"<c:plotArea><c:layout/>
<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>
<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>Water!$B$1</c:f></c:strRef></c:tx><c:cat><c:numRef><c:f>Water!$A$2:$A$13</c:f></c:numRef></c:cat><c:val><c:numRef><c:f>Water!$B$2:$B$13</c:f></c:numRef></c:val></c:ser>
<c:gapWidth val="219"/><c:overlap val="-27"/><c:axId val="1213443568"/><c:axId val="1213451856"/></c:barChart>
<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>
<c:ser><c:idx val="1"/><c:order val="1"/><c:tx><c:strRef><c:f>Water!$C$1</c:f></c:strRef></c:tx><c:cat><c:numRef><c:f>Water!$A$2:$A$13</c:f></c:numRef></c:cat><c:val><c:numRef><c:f>Water!$C$2:$C$13</c:f></c:numRef></c:val><c:smooth val="0"/></c:ser>
<c:marker val="1"/><c:axId val="1213444400"/><c:axId val="1213444816"/></c:lineChart>
<c:catAx><c:axId val="1213443568"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="dd/mm/yyyy" sourceLinked="1"/><c:crossAx val="1213451856"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx>
<c:valAx><c:axId val="1213451856"/><c:scaling><c:orientation val="minMax"/><c:max val="3000"/><c:min val="0"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:majorGridlines/><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Conductivity (uS/cm)</a:t></a:r></a:p></c:rich></c:tx></c:title><c:numFmt formatCode="General" sourceLinked="1"/><c:crossAx val="1213443568"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>
<c:valAx><c:axId val="1213444816"/><c:scaling><c:orientation val="minMax"/><c:max val="10"/><c:min val="6"/></c:scaling><c:delete val="0"/><c:axPos val="r"/><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>pH</a:t></a:r></a:p></c:rich></c:tx></c:title><c:numFmt formatCode="General" sourceLinked="1"/><c:crossAx val="1213444400"/><c:crosses val="max"/><c:crossBetween val="between"/><c:majorUnit val="0.5"/></c:valAx>
<c:catAx><c:axId val="1213444400"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="1"/><c:axPos val="b"/><c:crossAx val="1213444816"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx>
</c:plotArea>"#;

#[test]
fn excels_secondary_axis_order_is_read_by_axis_id_not_position() {
    // Excel lists the secondary value axis before the secondary category axis: matching by
    // axId (not by document order or axPos alone) keeps the primary and secondary straight.
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><c:chart><c:autoTitleDeleted val=\"1\"/>{EXCEL_PLOT}<c:legend><c:legendPos val=\"b\"/></c:legend></c:chart></c:chartSpace>"
    );
    let root = crate::xml::parse(xml.as_bytes()).unwrap();
    let c = crate::chart::read_chart(&root).unwrap();
    assert_eq!(c.kind, ChartKind::Combo);
    assert_eq!(c.series.iter().map(|s| s.secondary).collect::<Vec<_>>(), [false, true]);
    assert_eq!(c.y_title.as_deref(), Some("Conductivity (uS/cm)"));
    assert_eq!(c.y2_title.as_deref(), Some("pH"));
    assert_eq!(c.y_axis, AxisScale { min: Some(0.0), max: Some(3000.0), major_unit: None });
    assert_eq!(c.y2_axis, AxisScale { min: Some(6.0), max: Some(10.0), major_unit: Some(0.5) });
}

#[test]
fn reversed_or_zero_axis_settings_read_as_auto() {
    let plot = EXCEL_PLOT
        .replace("<c:max val=\"10\"/><c:min val=\"6\"/>", "<c:max val=\"6\"/><c:min val=\"10\"/>")
        .replace("<c:majorUnit val=\"0.5\"/>", "<c:majorUnit val=\"0\"/>");
    let xml = format!(
        "<?xml version=\"1.0\"?><c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><c:chart>{plot}</c:chart></c:chartSpace>"
    );
    let root = crate::xml::parse(xml.as_bytes()).unwrap();
    let c = crate::chart::read_chart(&root).unwrap();
    assert!(c.y2_axis.is_auto(), "{:?}", c.y2_axis);
}
