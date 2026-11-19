//! Synthetic DrawingML only: a series' own data labels (`c:ser/c:dLbls`), over the chart's.

use crate::{chart::read_chart, read_xlsx, write_xlsx, xml::parse};
use gridcraft_model::{LabelPosition, SeriesLabels};

fn chart(ser_labels: &str, group_labels: &str) -> gridcraft_model::Chart {
    let ser = |i: u32| {
        format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{ser_labels}<c:val><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser>"
        )
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><c:chart><c:plotArea>\
        <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}{}{group_labels}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx></c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1)
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

const GROUP_VALUES: &str =
    "<c:dLbls><c:showLegendKey val=\"0\"/><c:showVal val=\"1\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/></c:dLbls>";

/// As in a workbook saved by Excel: the group shows values, but every series deletes its labels,
/// and Excel draws none.
#[test]
fn a_series_deleting_its_labels_overrides_the_group() {
    let c = chart("<c:dLbls><c:delete val=\"1\"/></c:dLbls>", GROUP_VALUES);
    assert!(c.data_labels, "the group's setting");
    assert!(c.series.iter().all(|s| s.labels == Some(SeriesLabels::default())), "each series shows nothing");
    assert!(!c.series[0].labels.unwrap().shows_anything());
}

/// Series names inside the base of each column, reading bottom to top (`rot="-5400000"`).
#[test]
fn a_series_label_reads_its_content_place_and_angle() {
    let c = chart(
        "<c:dLbls><c:txPr><a:bodyPr rot=\"-5400000\" vert=\"horz\"/><a:lstStyle/><a:p><a:pPr><a:defRPr/></a:pPr></a:p></c:txPr><c:dLblPos val=\"inBase\"/><c:showLegendKey val=\"0\"/><c:showVal val=\"0\"/><c:showCatName val=\"0\"/><c:showSerName val=\"1\"/></c:dLbls>",
        "",
    );
    let l = c.series[0].labels.unwrap();
    assert_eq!(l, SeriesLabels { value: false, series_name: true, category_name: false, position: Some(LabelPosition::InsideBase), rotation: 90.0 });
    assert!(!c.data_labels);
}

#[test]
fn series_labels_are_written_back() {
    let mut c = chart("", GROUP_VALUES);
    c.series[0].values = "Sheet1!$A$1:$A$2".into();
    c.series[1].values = "Sheet1!$B$1:$B$2".into();
    let named = SeriesLabels { series_name: true, position: Some(LabelPosition::InsideBase), rotation: 90.0, ..Default::default() };
    c.series[0].labels = Some(named);
    c.series[1].labels = Some(SeriesLabels::default());
    let mut wb = gridcraft_model::Workbook::new();
    std::sync::Arc::make_mut(&mut wb.sheets[0]).charts.push(c);
    let bytes = write_xlsx(&wb).unwrap();
    let back = read_xlsx(&bytes).unwrap().0.sheets[0].charts[0].clone();
    assert_eq!(back.series[0].labels, Some(named));
    assert_eq!(back.series[1].labels, Some(SeriesLabels::default()));
    assert!(back.data_labels);
    // Out-of-range angles are kept within a quarter turn.
    let c = chart("<c:dLbls><c:txPr><a:bodyPr rot=\"-99999999\"/></c:txPr><c:showVal val=\"1\"/></c:dLbls>", "");
    assert_eq!(c.series[0].labels.unwrap().rotation, 90.0);
}
