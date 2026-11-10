//! Synthetic DrawingML only: the sizes, weights and capitals a chart's text takes from its file.

use crate::{chart::read_chart, xml::parse};
use gridcraft_model::{ChartTextStyle, Color};

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
    // Parts other than the title take Excel's black where the file gives no colour.
    let black = Some(Color::Rgb(0));
    let style = |size: f32, bold: Option<bool>, caps: bool, color| ChartTextStyle { size: Some(size), bold, caps, color };
    assert_eq!(c.text.title, style(18.0, Some(true), false, None), "the run's over the paragraph's defaults");
    assert_eq!(c.text.legend, style(10.5, Some(true), false, black));
    assert_eq!(c.text.data_labels, style(9.0, Some(true), false, black));
    assert_eq!(c.text.category_axis, style(8.0, Some(false), true, black));
    assert_eq!(c.text.value_axis, style(7.0, None, false, black));
}

/// A title with both rich text and `txPr`: each property from the nearest place that sets it.
#[test]
fn a_titles_run_paragraph_and_txpr_each_give_what_they_set() {
    let title = format!(
        "<c:title><c:tx><c:rich><a:bodyPr {A}/><a:p {A}><a:pPr><a:defRPr b=\"1\"/></a:pPr>        <a:r><a:rPr sz=\"1600\"/><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx>{}</c:title>",
        tx_pr("sz=\"1200\" b=\"0\" cap=\"all\"")
    );
    let c = chart(&title, "", "", "", "");
    assert_eq!(c.text.title, ChartTextStyle { size: Some(16.0), bold: Some(true), caps: true, color: None });
}

/// Measured in Excel on a synthetic chart without `txPr`, for every style: legend, labels and
/// axes are 10 pt black (white on the black chart of styles 41-48, not drawn by GridCraft yet).
/// The title keeps GridCraft's.
#[test]
fn text_the_file_leaves_out_takes_excels_defaults() {
    let c = chart("", "<c:legend><c:legendPos val=\"r\"/></c:legend>", "", "", "");
    let excel = ChartTextStyle { size: Some(10.0), color: Some(Color::Rgb(0)), ..Default::default() };
    assert_eq!(c.text.title, ChartTextStyle::default());
    for t in [c.text.legend, c.text.data_labels, c.text.category_axis, c.text.value_axis] {
        assert_eq!(t, excel);
    }
    // A size out of range is ignored for Excel's.
    let c = chart("", "", "", &tx_pr("sz=\"0\""), &tx_pr("sz=\"99999\""));
    assert_eq!((c.text.category_axis.size, c.text.value_axis.size), (Some(10.0), Some(10.0)));
}

/// Colours: RGB, and Excel's own grey (`tx1` at 65% plus 35%) as a tint of the text colour; the
/// chart's `txPr` is the default every part inherits.
#[test]
fn text_colours_are_read_and_the_charts_default_is_inherited() {
    let fill = |c: &str| {
        format!("<c:txPr><a:bodyPr {A}/><a:p {A}><a:pPr><a:defRPr sz=\"900\"><a:solidFill>{c}</a:solidFill></a:defRPr></a:pPr></a:p></c:txPr>")
    };
    let c = chart(
        "",
        "",
        "",
        &fill(r#"<a:srgbClr val="FF0000"/>"#),
        &fill(r#"<a:schemeClr val="tx1"><a:lumMod val="65000"/><a:lumOff val="35000"/></a:schemeClr>"#),
    );
    assert_eq!(c.text.category_axis.color, Some(Color::Rgb(0xFF0000)));
    assert_eq!(c.text.value_axis.color, Some(Color::Theme(1, 350)));
    assert_eq!(Color::Theme(1, 350).resolve(&gridcraft_model::Theme::default()), Some([0x59, 0x59, 0x59]), "Excel's grey on the default theme");
    // Inherited from the chart's own txPr.
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea><c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx></c:plotArea></c:chart>{}</c:chartSpace>",
        fill(r#"<a:srgbClr val="0000FF"/>"#)
    );
    let c = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!((c.text.value_axis.size, c.text.value_axis.color), (Some(9.0), Some(Color::Rgb(0x0000FF))));
}

/// Written back with each part's text, GridCraft's 9 pt dark grey where the chart sets none, so
/// Excel draws them as GridCraft does; read back the same.
#[test]
fn text_is_written_with_its_style() {
    let mut c = chart("", "<c:legend><c:legendPos val=\"r\"/></c:legend>", "", "", "");
    c.text = Default::default();
    c.text.value_axis = ChartTextStyle { size: Some(12.0), bold: Some(true), caps: true, color: Some(Color::Rgb(0xFF0000)) };
    c.series[0].values = "Sheet1!$A$1:$A$1".into();
    let mut wb = gridcraft_model::Workbook::new();
    std::sync::Arc::make_mut(&mut wb.sheets[0]).charts.push(c);
    let bytes = crate::write_xlsx(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/charts/chart1.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains(r#"<a:defRPr sz="1200" b="1" cap="all"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:defRPr>"#), "{xml}");
    assert!(xml.contains(r#"<a:defRPr sz="900" b="0"><a:solidFill><a:srgbClr val="595959"/></a:solidFill></a:defRPr>"#), "{xml}");
    let back = crate::read_xlsx(&bytes).unwrap().0.sheets[0].charts[0].text;
    assert_eq!(back.value_axis, ChartTextStyle { size: Some(12.0), bold: Some(true), caps: true, color: Some(Color::Rgb(0xFF0000)) });
    let grey = ChartTextStyle { size: Some(9.0), bold: Some(false), caps: false, color: Some(Color::Rgb(0x595959)) };
    assert_eq!((back.legend, back.category_axis), (grey, grey));
}
