//! Synthetic DrawingML only: the sizes, weights and capitals a chart's text takes from its file.

use crate::{chart::read_chart, xml::parse};
use gridcraft_model::ChartTextStyle;

const A: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

fn tx_pr(attrs: &str) -> String {
    format!("<c:txPr><a:bodyPr {A}/><a:p {A}><a:pPr><a:defRPr {attrs}/></a:pPr></a:p></c:txPr>")
}

fn chart(title: &str, legend: &str, labels: &str, cat: &str, val: &str) -> gridcraft_model::Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart>{title}<c:plotArea>\
        <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{labels}\
        <c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>12</c:v></c:pt></c:numLit></c:val>\
        </c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx><c:axId val=\"1\"/>{cat}</c:catAx><c:valAx><c:axId val=\"2\"/>{val}</c:valAx>\
        </c:plotArea>{legend}</c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

#[test]
fn each_part_takes_its_own_text_style() {
    let title = format!(
        "<c:title><c:tx><c:rich><a:bodyPr {A}/><a:p {A}><a:pPr><a:defRPr sz=\"1400\" b=\"0\"/></a:pPr>\
        <a:r><a:rPr sz=\"1800\" b=\"1\"/><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title>"
    );
    let legend = format!("<c:legend><c:legendPos val=\"t\"/>{}</c:legend>", tx_pr("sz=\"1050\" b=\"1\""));
    let labels = format!("<c:dLbls>{}<c:showVal val=\"1\"/></c:dLbls>", tx_pr("sz=\"900\" b=\"1\""));
    let c = chart(&title, &legend, &labels, &tx_pr("sz=\"800\" b=\"0\" cap=\"all\""), &tx_pr("sz=\"700\""));
    let style = |size: f32, bold: Option<bool>, caps: bool| ChartTextStyle { size: Some(size), bold, caps };
    assert_eq!(c.text.title, style(18.0, Some(true), false), "the run's over the paragraph's defaults");
    assert_eq!(c.text.legend, style(10.5, Some(true), false));
    assert_eq!(c.text.data_labels, style(9.0, Some(true), false));
    assert_eq!(c.text.category_axis, style(8.0, Some(false), true));
    assert_eq!(c.text.value_axis, style(7.0, None, false));
}

/// A title with both rich text and `txPr`: each property from the nearest place that sets it.
#[test]
fn a_titles_run_paragraph_and_txpr_each_give_what_they_set() {
    let title = format!(
        "<c:title><c:tx><c:rich><a:bodyPr {A}/><a:p {A}><a:pPr><a:defRPr b=\"1\"/></a:pPr>        <a:r><a:rPr sz=\"1600\"/><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx>{}</c:title>",
        tx_pr("sz=\"1200\" b=\"0\" cap=\"all\"")
    );
    let c = chart(&title, "", "", "", "");
    assert_eq!(c.text.title, ChartTextStyle { size: Some(16.0), bold: Some(true), caps: true });
}

#[test]
fn text_the_file_leaves_out_keeps_the_defaults() {
    let c = chart("", "<c:legend><c:legendPos val=\"r\"/></c:legend>", "", "", "");
    assert_eq!(c.text, Default::default());
    // A size out of range is ignored rather than drawn.
    let c = chart("", "", "", &tx_pr("sz=\"0\""), &tx_pr("sz=\"99999\""));
    assert_eq!((c.text.category_axis.size, c.text.value_axis.size), (None, None));
}
