//! Line chart groups by `grouping` and markers: `percentStacked` opens as a 100% stacked line.

use gridcraft_model::{ChartKind, Workbook};

use crate::chart::{read_chart, write_chart};
use crate::xml::parse;

/// Reads a line chart whose one group has `grouping` and whose one series holds `marker`.
fn kind(grouping: &str, marker: &str) -> ChartKind {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea><c:lineChart>\
         <c:grouping val=\"{grouping}\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{marker}\
         <c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser><c:marker val=\"1\"/>\
         <c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart><c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx>\
         </c:plotArea></c:chart></c:chartSpace>"
    );
    let root = parse(xml.as_bytes()).unwrap();
    read_chart(&root).unwrap().0.kind
}

const NO_MARKER: &str = "<c:marker><c:symbol val=\"none\"/></c:marker>";

#[test]
fn percent_stacked_lines_open_as_hundred_percent_stacked() {
    assert_eq!(kind("percentStacked", NO_MARKER), ChartKind::LineStacked100);
    assert_eq!(kind("percentStacked", ""), ChartKind::LineStacked100Markers);
}

#[test]
fn standard_and_stacked_lines_keep_their_kinds() {
    assert_eq!(kind("standard", NO_MARKER), ChartKind::Line);
    assert_eq!(kind("standard", ""), ChartKind::LineMarkers);
    assert_eq!(kind("stacked", NO_MARKER), ChartKind::LineStacked);
    assert_eq!(kind("stacked", ""), ChartKind::LineStacked);
}

#[test]
fn a_hundred_percent_stacked_line_series_in_a_combo_chart_keeps_its_kind_when_saved() {
    let ser =
        |i: usize| format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>");
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1)
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(ch.kind, ChartKind::Combo);
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers)]);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap().0;
    let kinds: Vec<Option<ChartKind>> = again.series.iter().map(|s| s.kind).collect();
    assert_eq!(kinds.first(), Some(&Some(ChartKind::ColumnClustered)), "{saved}");
    assert_eq!(kinds, [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers)], "{saved}");
}

/// The lines on one pair of axes share the first one's grouping: a standard line after a 100%
/// stacked one is stacked to 100% too, keeping its own markers.
#[test]
fn combo_line_series_keep_their_order_and_share_one_percent_stack_when_saved() {
    let ser = |i: usize, marker: &str| {
        format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{marker}<c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:lineChart><c:grouping val=\"standard\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0, ""),
        ser(2, NO_MARKER),
        ser(1, ""),
        ser(3, NO_MARKER)
    );
    let want =
        [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers), Some(ChartKind::LineStacked100), Some(ChartKind::LineStacked100)];
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    assert_eq!(saved.matches("percentStacked").count(), 1, "both 100% stacked series share one stack: {saved}");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(again.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want, "{saved}");
}

#[test]
fn a_hundred_percent_stacked_line_on_the_secondary_axis_keeps_its_kind_when_saved() {
    let ser =
        |i: usize| format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>");
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1)
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    assert!(ch.series.get(1).is_some_and(|s| s.secondary && s.kind == Some(ChartKind::LineStacked100Markers)));
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap().0;
    let got: Vec<(Option<ChartKind>, bool)> = again.series.iter().map(|s| (s.kind, s.secondary)).collect();
    assert_eq!(got, [(Some(ChartKind::ColumnClustered), false), (Some(ChartKind::LineStacked100Markers), true)], "{saved}");
}

/// On the secondary axes too, a standard line after a 100% stacked one is stacked to 100%.
#[test]
fn secondary_columns_and_hundred_percent_stacked_lines_keep_their_kinds_when_saved() {
    let ser = |i: usize, marker: &str| {
        format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{marker}<c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:barChart>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
         <c:lineChart><c:grouping val=\"standard\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0, ""),
        ser(1, ""),
        ser(2, ""),
        ser(3, NO_MARKER)
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    let want = [
        (Some(ChartKind::ColumnClustered), false),
        (Some(ChartKind::ColumnClustered), true),
        (Some(ChartKind::LineStacked100Markers), true),
        (Some(ChartKind::LineStacked100), true),
    ];
    assert_eq!(ch.series.iter().map(|s| (s.kind, s.secondary)).collect::<Vec<_>>(), want);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(again.series.iter().map(|s| (s.kind, s.secondary)).collect::<Vec<_>>(), want, "{saved}");
}

#[test]
fn a_hundred_percent_stacked_line_group_with_mixed_markers_keeps_each_series_markers() {
    let ser = |i: usize, marker: &str| {
        format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{marker}<c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0, ""),
        ser(1, NO_MARKER)
    );
    // A 100% stacked line chart with markers; the second series keeps its own lack of them.
    let want = [None, Some(ChartKind::LineStacked100)];
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(ch.kind, ChartKind::LineStacked100Markers);
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want);
    assert_eq!(ch.series_kinds(), [ChartKind::LineStacked100Markers, ChartKind::LineStacked100]);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(again.kind, ChartKind::LineStacked100Markers, "{saved}");
    assert_eq!(again.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want, "{saved}");
}

/// The lines on one pair of axes share the first one's grouping: a 100% stacked line group after
/// a stacked one is drawn stacked.
#[test]
fn a_hundred_percent_stacked_line_group_after_a_stacked_one_on_the_same_axes_is_stacked() {
    let ser = |i: usize| {
        format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{NO_MARKER}<c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:lineChart><c:grouping val=\"stacked\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         <c:lineChart><c:grouping val=\"percentStacked\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1)
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().0;
    assert_eq!(ch.series_kinds(), [ChartKind::LineStacked, ChartKind::LineStacked]);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    assert!(!saved.contains("percentStacked"), "{saved}");
}
