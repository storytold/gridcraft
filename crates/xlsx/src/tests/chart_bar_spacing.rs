//! Gap width and overlap of column and bar charts, as files write them.

use gridcraft_model::Chart;

use crate::chart::read_chart;
use crate::xml::parse;

fn bars(grouping: &str, spacing: &str) -> Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
        <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"{grouping}\"/>\
        <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>12</c:v></c:pt></c:numLit></c:val></c:ser>\
        {spacing}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

#[test]
fn bar_spacing_is_read_in_both_notations() {
    let plain = bars("clustered", "<c:gapWidth val=\"219\"/><c:overlap val=\"-27\"/>");
    assert_eq!((plain.gap_width, plain.overlap), (Some(219), Some(-27)));
    let strict = bars("clustered", "<c:gapWidth val=\"219%\"/><c:overlap val=\"-27%\"/>");
    assert_eq!((strict.gap_width, strict.overlap), (Some(219), Some(-27)));
    let none = bars("clustered", "");
    assert_eq!((none.gap_width, none.overlap), (None, None));
}

/// A stacked chart's 100 isn't kept: changed to clustered, its series would lie on top of each other.
#[test]
fn a_stacked_charts_overlap_is_not_kept() {
    for grouping in ["stacked", "percentStacked"] {
        assert_eq!(bars(grouping, "<c:gapWidth val=\"50\"/><c:overlap val=\"100\"/>").overlap, None, "{grouping}");
        assert_eq!(bars(grouping, "<c:overlap val=\"150\"/>").overlap, None, "{grouping}: 150 is 100 once clamped");
        assert_eq!(bars(grouping, "<c:overlap val=\"-27\"/>").overlap, Some(-27), "{grouping}: the file's own value stays");
        assert_eq!(bars(grouping, "<c:overlap val=\"50\"/>").overlap, Some(50), "{grouping}: the file's own value stays");
    }
}
