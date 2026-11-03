//! Line chart groups by `grouping` and markers: `percentStacked` opens as a 100% stacked line.

use gridcraft_model::{Anchor, Chart, ChartKind, LegendPos, Series, Workbook};

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
fn percent_stacked_chart_writer_defaults_round_trip_without_series_overrides() {
    let wb = Workbook::default();
    for kind in
        [ChartKind::LineStacked100, ChartKind::LineStacked100Markers, ChartKind::AreaStacked100, ChartKind::BarStacked, ChartKind::BarStacked100]
    {
        let chart = Chart {
            id: 1,
            kind,
            anchor: Anchor { cell: gridcraft_core::CellRef::new(0, 0), width: 480.0, height: 288.0, ..Default::default() },
            title: None,
            series: vec![Series { values: "Data!$A$1:$A$3".into(), ..Default::default() }],
            legend: LegendPos::Bottom,
            data_labels: false,
            gridlines: true,
            style: 2,
            x_title: None,
            y_title: None,
            source: None,
            by_rows: false,
            pivot: None,
            data_table: None,
            format: Default::default(),
        };

        let xml = write_chart(&wb, 0, &chart, "Book1");
        let round_trip = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();

        assert_eq!(round_trip.kind, kind, "{kind:?}");
        assert!(round_trip.format.series.is_empty(), "writer defaults should not become explicit style overrides for {kind:?}");
        if matches!(kind, ChartKind::BarStacked | ChartKind::BarStacked100) {
            assert_eq!(round_trip.format.overlap, None, "the writer's default stacked overlap should remain implicit");
        }
    }
}

#[test]
fn native_default_stacked_bar_overlap_is_not_loaded_as_an_explicit_override() {
    let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="stacked"/><c:overlap val="100"/><c:ser><c:val><c:numRef><c:f>Data!$A$1:$A$3</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let chart = crate::chart::read_chart(&crate::xml::parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(chart.kind, ChartKind::ColumnStacked);
    assert_eq!(chart.format.overlap, None);
}

#[test]
fn a_hundred_percent_stacked_line_series_in_a_combo_chart_is_saved_as_a_line() {
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
    assert!(kinds.get(1).is_some_and(|k| !matches!(k, Some(ChartKind::ColumnClustered))), "the line series stays out of the column group: {saved}");
}
