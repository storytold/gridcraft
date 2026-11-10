//! Synthetic DrawingML only: a chart's style as Excel saves it, read and written back.

use crate::{chart::read_chart, xml::parse};

fn style(before_chart: &str) -> u32 {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">{before_chart}<c:chart><c:plotArea><c:lineChart><c:grouping val=\"standard\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().style
}

/// Excel 2010 and later write the style inside `mc:AlternateContent` (seen in a workbook saved by
/// Excel): `c14:style` is the style plus 100, `c:style` the fallback.
#[test]
fn the_style_is_read_where_excel_writes_it() {
    assert_eq!(style(""), 2, "Excel's default");
    assert_eq!(style(r#"<c:style val="26"/>"#), 26);
    let alternate = |choice: &str, fallback: &str| {
        format!(
            r#"<mc:AlternateContent><mc:Choice Requires="c14" xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart">{choice}</mc:Choice><mc:Fallback>{fallback}</mc:Fallback></mc:AlternateContent>"#
        )
    };
    assert_eq!(style(&alternate(r#"<c14:style val="126"/>"#, r#"<c:style val="26"/>"#)), 26);
    assert_eq!(style(&alternate(r#"<c14:style val="141"/>"#, "")), 41, "the Choice alone");
    // Out of range: the default.
    assert_eq!(style(&alternate(r#"<c14:style val="7"/>"#, "")), 2);
    assert_eq!(style(r#"<c:style val="99"/>"#), 2);
    // An invalid candidate gives way to the next valid one.
    assert_eq!(style(&format!(r#"<c:style val="99"/>{}"#, alternate("", r#"<c:style val="26"/>"#))), 26);
    assert_eq!(style(&alternate(r#"<c14:style val="141"/>"#, r#"<c:style val="0"/>"#)), 41);
}
