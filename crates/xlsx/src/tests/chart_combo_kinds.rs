//! Importing a combination chart keeps each series' own group kind, the first group's included.

use gridcraft_model::{Chart, ChartKind, ChartSeriesFormat, Color, Series, Workbook};

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

#[test]
fn chart_combo_horizontal_bars_keep_titles_on_category_and_value_axes_after_save() {
    let title = |text: &str| format!("<c:title><c:tx><c:rich><a:p><a:r><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx></c:title>");
    let bars = bar(&ser(0)).replace("val=\"col\"", "val=\"bar\"");
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><c:chart><c:plotArea>\
         {bars}{}<c:catAx><c:axId val=\"1\"/><c:axPos val=\"l\"/>{}</c:catAx>\
         <c:valAx><c:axId val=\"2\"/><c:axPos val=\"b\"/>{}</c:valAx>\
         </c:plotArea></c:chart></c:chartSpace>",
        line(&ser(1)),
        title("Region"),
        title("Sales")
    );
    let mut chart = read_chart(&parse(xml.as_bytes()).unwrap()).unwrap();
    let wb = gridcraft_model::Workbook::default();
    for _ in 0..2 {
        assert_eq!(chart.kind, ChartKind::Combo);
        assert_eq!(chart.x_title.as_deref(), Some("Region"));
        assert_eq!(chart.y_title.as_deref(), Some("Sales"));
        let saved = crate::chart::write_chart(&wb, 0, &chart, "Book.xlsx");
        let root = parse(saved.as_bytes()).unwrap();
        let plot = root.path(&["chart", "plotArea"]).unwrap();
        assert_eq!(plot.path(&["catAx", "title", "tx", "rich", "p", "r", "t"]).unwrap().text, "Region");
        assert_eq!(plot.path(&["valAx", "title", "tx", "rich", "p", "r", "t"]).unwrap().text, "Sales");
        chart = read_chart(&root).unwrap();
    }
}

#[test]
fn combo_reordered_series_keep_their_own_formats_across_repeated_saves() {
    let styles = [
        ChartSeriesFormat {
            line_width: Some(3.0),
            marker: Some("square".into()),
            marker_size: Some(7.0),
            fill_color: Some([18, 52, 86, 255]),
            line_color: Some([18, 52, 86, 255]),
            marker_fill: Some([18, 52, 86, 255]),
            marker_border_color: Some([86, 52, 18, 255]),
            marker_border_width: Some(2.0),
            smooth: Some(true),
            ..Default::default()
        },
        ChartSeriesFormat {
            line_width: Some(4.0),
            no_line: true,
            border_width: Some(0.0),
            fill_color: Some([35, 69, 103, 255]),
            line_color: Some([38, 38, 38, 255]),
            ..Default::default()
        },
        ChartSeriesFormat {
            line_width: Some(1.5),
            dashed: true,
            marker: Some("diamond".into()),
            marker_border_width: Some(1.25),
            ..Default::default()
        },
        ChartSeriesFormat {
            line_width: Some(5.0),
            border_width: Some(2.5),
            fill_color: Some([52, 86, 120, 255]),
            line_color: Some([38, 38, 38, 255]),
            ..Default::default()
        },
    ];
    let original_series: Vec<Series> = [
        ("Styled line", ChartKind::LineMarkers, Some(Color::Rgb(0x123456))),
        ("Hidden border column", ChartKind::ColumnClustered, Some(Color::Rgb(0x234567))),
        ("Automatic line", ChartKind::LineMarkers, None),
        ("Explicit border column", ChartKind::ColumnClustered, Some(Color::Rgb(0x345678))),
    ]
    .into_iter()
    .map(|(name, kind, color)| Series {
        name: Some(name.into()),
        categories: Some("{1,2,3}".into()),
        values: "{2,4,6}".into(),
        kind: Some(kind),
        color,
        ..Default::default()
    })
    .collect();
    let mut chart = Chart {
        id: 1,
        kind: ChartKind::Combo,
        anchor: Default::default(),
        title: None,
        series: original_series.clone(),
        legend: Default::default(),
        data_labels: false,
        gridlines: false,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        format: gridcraft_model::ChartFormat { series: styles.iter().cloned().enumerate().collect(), ..Default::default() },
    };
    let wb = Workbook::default();
    for save in 0..3 {
        let saved = crate::chart::write_chart(&wb, 0, &chart, "Book.xlsx");
        let root = parse(saved.as_bytes()).unwrap();
        let plot = root.path(&["chart", "plotArea"]).unwrap();
        let indices: Vec<_> = plot
            .children
            .iter()
            .filter(|group| matches!(group.name.as_str(), "barChart" | "lineChart"))
            .flat_map(|group| group.kids("ser"))
            .map(|series| series.child_val("idx").unwrap())
            .collect();
        assert_eq!(indices, if save == 0 { ["1", "3", "0", "2"] } else { ["0", "1", "2", "3"] });

        chart = read_chart(&root).unwrap();
        assert_eq!(chart.kind, ChartKind::Combo);
        assert_eq!(chart.series.len(), original_series.len());
        assert_eq!(chart.format.series.len(), styles.len());
        for (position, series) in chart.series.iter().enumerate() {
            let original_index = original_series.iter().position(|original| original.name == series.name).unwrap();
            let original = &original_series[original_index];
            assert_eq!(chart.format.series.get(&position), Some(&styles[original_index]), "save {save}, series {:?}", series.name);
            assert_eq!(series.kind, original.kind);
            assert_eq!(series.color, original.color);
            assert_eq!(series.smooth, styles[original_index].smooth.unwrap_or(original.smooth));
        }
    }
}
