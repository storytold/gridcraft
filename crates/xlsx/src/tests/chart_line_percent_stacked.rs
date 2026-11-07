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
    read_chart(&root).unwrap().kind
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
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(ch.kind, ChartKind::Combo);
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers)]);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
    let kinds: Vec<Option<ChartKind>> = again.series.iter().map(|s| s.kind).collect();
    assert_eq!(kinds.first(), Some(&Some(ChartKind::ColumnClustered)), "{saved}");
    assert_eq!(kinds, [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers)], "{saved}");
}

#[test]
fn combo_line_series_keep_their_kinds_order_and_shared_percent_stack_when_saved() {
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
    let want = [Some(ChartKind::ColumnClustered), Some(ChartKind::LineStacked100Markers), Some(ChartKind::Line), Some(ChartKind::LineStacked100)];
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    assert_eq!(saved.matches("percentStacked").count(), 1, "both 100% stacked series share one stack: {saved}");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
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
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert!(ch.series.get(1).is_some_and(|s| s.secondary && s.kind == Some(ChartKind::LineStacked100Markers)));
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
    let got: Vec<(Option<ChartKind>, bool)> = again.series.iter().map(|s| (s.kind, s.secondary)).collect();
    assert_eq!(got, [(Some(ChartKind::ColumnClustered), false), (Some(ChartKind::LineStacked100Markers), true)], "{saved}");
}

#[test]
fn secondary_columns_lines_and_hundred_percent_stacked_lines_keep_their_kinds_when_saved() {
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
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let want = [
        (Some(ChartKind::ColumnClustered), false),
        (Some(ChartKind::ColumnClustered), true),
        (Some(ChartKind::LineStacked100Markers), true),
        (Some(ChartKind::Line), true),
    ];
    assert_eq!(ch.series.iter().map(|s| (s.kind, s.secondary)).collect::<Vec<_>>(), want);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
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
    let want = [Some(ChartKind::LineStacked100Markers), Some(ChartKind::LineStacked100)];
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(ch.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want);
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
    assert_eq!(again.series.iter().map(|s| s.kind).collect::<Vec<_>>(), want, "{saved}");
}

#[test]
fn combo_areas_marked_lines_and_mixed_column_groupings_keep_their_kinds_when_saved() {
    let ser = |i: usize, marker: &str| {
        format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{marker}<c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:areaChart><c:grouping val=\"stacked\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:areaChart>\
         <c:lineChart><c:grouping val=\"standard\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>{}<c:overlap val=\"100\"/><c:axId val=\"3\"/><c:axId val=\"4\"/></c:barChart>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:barChart>\
         <c:areaChart><c:grouping val=\"percentStacked\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:areaChart>\
         <c:lineChart><c:grouping val=\"standard\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
         </c:plotArea></c:chart></c:chartSpace>",
        ser(0, ""),
        ser(1, ""),
        ser(2, ""),
        ser(3, ""),
        ser(4, ""),
        ser(5, ""),
        ser(6, NO_MARKER)
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let got: Vec<(Option<ChartKind>, bool)> = ch.series.iter().map(|s| (s.kind, s.secondary)).collect();
    assert_eq!(got[1], (Some(ChartKind::AreaStacked), false));
    assert_eq!(got[3], (Some(ChartKind::ColumnStacked), true));
    assert_eq!(got[4], (Some(ChartKind::ColumnClustered), true));
    assert_eq!(got[5], (Some(ChartKind::AreaStacked100), true));
    assert_eq!(got[6], (Some(ChartKind::Line), true));
    let saved = write_chart(&Workbook::default(), 0, &ch, "Book1");
    let again = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();
    assert_eq!(again.series.iter().map(|s| (s.kind, s.secondary)).collect::<Vec<_>>(), got, "{saved}");
}
