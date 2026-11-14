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

#[test]
fn series_lines_are_read_with_their_style() {
    assert_eq!(bars("stacked", "").series_lines, None);
    assert_eq!(bars("stacked", "<c:serLines/>").series_lines, Some(Default::default()));
    let styled = bars(
        "stacked",
        "<c:serLines><c:spPr><a:ln xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" w=\"9525\"><a:solidFill><a:srgbClr val=\"808080\"/></a:solidFill></a:ln></c:spPr></c:serLines>",
    );
    let lines = styled.series_lines.unwrap();
    assert_eq!(lines.width, Some(0.75));
    assert!(lines.color.is_some());
}

/// In a combo chart the first group's axis decides, wherever the file lists it.
#[test]
fn the_hidden_value_axis_is_the_first_groups() {
    let combo = |axes: &str| {
        let xml = format!(
            "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
            <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/></c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
            <c:lineChart><c:grouping val=\"standard\"/><c:ser><c:idx val=\"1\"/><c:order val=\"1\"/></c:ser><c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
            <c:catAx><c:axId val=\"1\"/></c:catAx><c:catAx><c:axId val=\"3\"/></c:catAx>{axes}</c:plotArea></c:chart></c:chartSpace>"
        );
        read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().value_axis_hidden
    };
    let primary = |hidden: u8| format!("<c:valAx><c:axId val=\"2\"/><c:delete val=\"{hidden}\"/></c:valAx>");
    let secondary = |hidden: u8| format!("<c:valAx><c:axId val=\"4\"/><c:delete val=\"{hidden}\"/></c:valAx>");
    assert!(!combo(&format!("{}{}", secondary(1), primary(0))), "the secondary listed first doesn't decide");
    assert!(combo(&format!("{}{}", secondary(0), primary(1))));
    assert!(combo(&format!("{}{}", primary(1), secondary(0))));
}

/// A combo chart can hold a clustered bar group before the stacked one that has the lines.
#[test]
fn series_lines_come_from_the_bar_group_that_has_them() {
    let group = |grouping: &str, axes: &str, extra: &str| {
        format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"{grouping}\"/>\
            <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser>\
            {extra}{axes}</c:barChart>"
        )
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}{}\
        <c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>",
        group("clustered", "<c:axId val=\"1\"/><c:axId val=\"2\"/>", "<c:gapWidth val=\"80\"/>"),
        group("stacked", "<c:axId val=\"3\"/><c:axId val=\"4\"/>", "<c:overlap val=\"100\"/><c:serLines/>"),
    );
    let chart = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(chart.series_lines, Some(Default::default()));
    assert_eq!(chart.gap_width, Some(80), "spacing still comes from the first group");
}
