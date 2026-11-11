//! Importing a combination chart keeps each series' own group kind, the first group's included.

use gridcraft_model::ChartKind;

use crate::chart::read_chart;
use crate::xml::parse;

fn ser(i: usize) -> String {
    format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
}

const AX: &str = "<c:axId val=\"1\"/><c:axId val=\"2\"/>";

fn bar(sers: &str) -> String {
    format!("<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{sers}{AX}</c:barChart>")
}

fn line(sers: &str) -> String {
    format!("<c:lineChart><c:grouping val=\"standard\"/>{sers}{AX}</c:lineChart>")
}

fn kinds(first: &str, second: &str) -> Vec<Option<ChartKind>> {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
         <c:chart><c:plotArea>{first}{second}</c:plotArea></c:chart></c:chartSpace>"
    );
    let root = parse(xml.as_bytes()).unwrap();
    let ch = read_chart(&root).unwrap();
    assert_eq!(ch.kind, ChartKind::Combo);
    ch.series.iter().map(|s| s.kind).collect()
}

#[test]
fn first_group_with_two_columns_stays_columns() {
    let k = kinds(&bar(&(ser(0) + &ser(1))), &line(&ser(2)));
    assert_eq!(k[0], Some(ChartKind::ColumnClustered));
    assert_eq!(k[1], Some(ChartKind::ColumnClustered));
    assert!(matches!(k[2], Some(ChartKind::Line | ChartKind::LineMarkers)));
}

#[test]
fn line_first_group_stays_a_line() {
    let k = kinds(&line(&ser(0)), &bar(&ser(1)));
    assert!(matches!(k[0], Some(ChartKind::Line | ChartKind::LineMarkers)));
    assert_eq!(k[1], Some(ChartKind::ColumnClustered));
}

/// Outside a combo, a series keeps its own markers or lines where they differ from its chart's,
/// read and written. The chart's kind is its first series'.
#[test]
fn series_keep_their_own_markers_and_lines() {
    use gridcraft_model::{Chart, Series, Workbook};
    let no_marker = "<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:marker><c:symbol val=\"none\"/></c:marker><c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>";
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}</c:plotArea></c:chart></c:chartSpace>",
        line(&(no_marker.to_string() + &ser(1)))
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!((ch.kind, ch.series.iter().map(|s| s.kind).collect::<Vec<_>>()), (ChartKind::Line, vec![None, Some(ChartKind::LineMarkers)]));
    let series = |kind| Series { values: "Sheet1!$A$1:$A$3".into(), kind, ..Series::default() };
    for (kind, kinds) in
        [(ChartKind::Line, vec![None, Some(ChartKind::LineMarkers)]), (ChartKind::ScatterLines, vec![Some(ChartKind::Scatter), None])]
    {
        let chart = Chart { kind, series: kinds.iter().map(|k| series(*k)).collect(), ..ch.clone() };
        let written = crate::chart::write_chart(&Workbook::new(), 0, &chart, "Book1");
        let back = read_chart(&parse(written.as_bytes()).unwrap()).unwrap();
        // XLSX has no chart-wide kind besides its series': each series is drawn as it was.
        let drawn = |c: &Chart| (0..c.series.len()).map(|i| c.series_kind(i)).collect::<Vec<_>>();
        assert_eq!(drawn(&back), drawn(&chart), "{kind:?}");
    }
}

/// A combo chart is written with one group per kind its series are drawn as, and read back in
/// the series' own order.
#[test]
fn combo_series_keep_their_kinds_and_order() {
    use gridcraft_model::{Chart, Series, Workbook};
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}{}</c:plotArea></c:chart></c:chartSpace>",
        bar(&ser(0)),
        line(&ser(1))
    );
    let base = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    for kinds in [
        vec![ChartKind::ColumnClustered, ChartKind::Area],
        vec![ChartKind::ColumnStacked, ChartKind::LineMarkers],
        vec![ChartKind::LineMarkers, ChartKind::ColumnClustered],
    ] {
        let series: Vec<Series> = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| Series { values: format!("Sheet1!${}$1:${}$3", ["B", "C"][i], ["B", "C"][i]), kind: Some(*k), ..Series::default() })
            .collect();
        let chart = Chart { kind: ChartKind::Combo, series, ..base.clone() };
        let back = read_chart(&parse(crate::chart::write_chart(&Workbook::new(), 0, &chart, "Book1").as_bytes()).unwrap()).unwrap();
        let drawn = |c: &Chart| (0..c.series.len()).map(|i| (c.series_kind(i), c.series[i].values.clone())).collect::<Vec<_>>();
        assert_eq!(drawn(&back), drawn(&chart), "{kinds:?}");
    }
    // Groups that can't share one pair of axes are drawn in the first one's kind: a pie beside
    // columns as columns, its axes all present. (A combination's bars are drawn as columns.)
    {
        let (kinds, want) = (vec![ChartKind::Pie, ChartKind::ColumnClustered], ChartKind::ColumnClustered);
        let series: Vec<Series> = kinds.iter().map(|k| Series { values: "Sheet1!$B$1:$B$3".into(), kind: Some(*k), ..Series::default() }).collect();
        let chart = Chart { kind: ChartKind::Combo, series, ..base.clone() };
        let xml = crate::chart::write_chart(&Workbook::new(), 0, &chart, "Book1");
        let back = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
        assert!((0..2).all(|i| back.series_kind(i) == want), "{kinds:?}: {xml}");
        assert!(back.series.iter().all(|s| !s.secondary), "{kinds:?}");
        assert!(xml.contains("<c:valAx>"), "{kinds:?}: {xml}");
    }
}

/// A chart of one kind with a series on the secondary axis keeps it, and reads back as that
/// kind, not a combination.
#[test]
fn one_kind_on_two_axes_stays_one_kind() {
    use gridcraft_model::{Chart, Series, Workbook};
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}</c:plotArea></c:chart></c:chartSpace>",
        bar(&ser(0))
    );
    let base = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let series = vec![
        Series { values: "Sheet1!$B$1:$B$3".into(), ..Series::default() },
        Series { values: "Sheet1!$C$1:$C$3".into(), secondary: true, ..Series::default() },
    ];
    let chart = Chart { kind: ChartKind::ColumnClustered, series, ..base };
    let back = read_chart(&parse(crate::chart::write_chart(&Workbook::new(), 0, &chart, "Book1").as_bytes()).unwrap()).unwrap();
    assert_eq!(back.kind, ChartKind::ColumnClustered);
    assert_eq!(back.series.iter().map(|s| (s.kind, s.secondary)).collect::<Vec<_>>(), vec![(None, false), (None, true)]);
}

/// A stock chart with volume: the stock group of 4 series stays a stock group beside the
/// volume columns.
#[test]
fn stock_with_volume_keeps_its_stock_group() {
    use gridcraft_model::{Chart, Series, Workbook};
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}</c:plotArea></c:chart></c:chartSpace>",
        bar(&ser(0))
    );
    let base = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let mut series = vec![Series { values: "Sheet1!$B$1:$B$3".into(), kind: Some(ChartKind::ColumnClustered), ..Series::default() }];
    series.extend((0..4).map(|_| Series { values: "Sheet1!$C$1:$C$3".into(), kind: Some(ChartKind::Stock), secondary: true, ..Series::default() }));
    let chart = Chart { kind: ChartKind::Combo, series, ..base };
    let written = crate::chart::write_chart(&Workbook::new(), 0, &chart, "Book1");
    assert!(written.contains("<c:stockChart>"), "{written}");
    let back = read_chart(&parse(written.as_bytes()).unwrap()).unwrap();
    assert_eq!((1..5).map(|i| back.series_kind(i)).collect::<Vec<_>>(), vec![ChartKind::Stock; 4]);
}

/// The primary axes are the pair whose value axis is on the left, whichever group the file
/// lists first.
#[test]
fn a_secondary_scatter_listed_first_stays_secondary() {
    let sc = |s: &str, a: u32, b: u32| {
        format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/>{s}<c:axId val=\"{a}\"/><c:axId val=\"{b}\"/></c:scatterChart>")
    };
    let ax =
        |id: u32, cross: u32, pos: &str| format!("<c:valAx><c:axId val=\"{id}\"/><c:axPos val=\"{pos}\"/><c:crossAx val=\"{cross}\"/></c:valAx>");
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}{}{}{}{}{}</c:plotArea></c:chart></c:chartSpace>",
        sc(&ser(0), 30, 40),
        sc(&ser(1), 10, 20),
        ax(10, 20, "b"),
        ax(20, 10, "l"),
        ax(30, 40, "b"),
        ax(40, 30, "r")
    );
    let ch = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(ch.series.iter().map(|s| s.secondary).collect::<Vec<_>>(), vec![true, false]);
}

/// A chart built from `kinds` (with secondary flags), written to XLSX.
fn written(kind: ChartKind, kinds: &[(Option<ChartKind>, bool)]) -> String {
    use gridcraft_model::{Chart, Series, Workbook};
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>{}</c:plotArea></c:chart></c:chartSpace>",
        bar(&ser(0))
    );
    let base = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let series = kinds
        .iter()
        .map(|(k, secondary)| Series { values: "Sheet1!$B$1:$B$3".into(), kind: *k, secondary: *secondary, ..Series::default() })
        .collect();
    crate::chart::write_chart(&Workbook::new(), 0, &Chart { kind, series, ..base }, "Book1")
}

/// Pies and doughnuts have no axes: a series marked secondary (left from a combo before the
/// type changed) stays in the one group, so Excel draws one pie, not two on top of each other.
#[test]
fn pies_and_doughnuts_are_one_group_whatever_the_secondary_flags() {
    for (kind, tag) in [(ChartKind::Pie, "<c:pieChart>"), (ChartKind::Doughnut, "<c:doughnutChart>")] {
        let xml = written(kind, &[(None, false), (None, true)]);
        assert_eq!(xml.matches(tag).count(), 1, "{xml}");
        assert!(!xml.contains("<c:valAx>"), "{xml}");
        let back = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
        assert_eq!((back.kind, back.series.len()), (kind, 2));
    }
}

/// Excel keeps one group per type on a pair of axes: clustered and stacked columns (or lines
/// and stacked lines) on the same axes are written as one group.
#[test]
fn one_group_per_type_on_a_pair_of_axes() {
    use ChartKind::*;
    for (kinds, tag) in [
        (vec![(Some(ColumnStacked), false), (Some(Pie), false), (Some(Line), false)], "<c:barChart>"),
        (vec![(Some(ColumnClustered), false), (Some(ColumnStacked), false)], "<c:barChart>"),
        (vec![(Some(Line), false), (Some(LineStacked), false), (Some(ColumnClustered), false)], "<c:lineChart>"),
        (vec![(Some(Area), false), (Some(AreaStacked), false), (Some(ColumnClustered), false)], "<c:areaChart>"),
    ] {
        let xml = written(Combo, &kinds);
        assert_eq!(xml.matches(tag).count(), 1, "{kinds:?}: {xml}");
    }
    // On separate axes each keeps its own group.
    let xml = written(Combo, &[(Some(ColumnClustered), false), (Some(ColumnStacked), true), (Some(Line), false)]);
    assert_eq!(xml.matches("<c:barChart>").count(), 2, "{xml}");
}

/// A markers-only scatter series drawn among lines (a combo on category axes) keeps its
/// markers.
#[test]
fn a_scatter_series_among_lines_keeps_its_markers() {
    use ChartKind::*;
    let xml = written(Combo, &[(Some(ColumnClustered), false), (Some(Scatter), false)]);
    let back = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    assert_eq!(back.series_kind(1), LineMarkers, "{xml}");
}
