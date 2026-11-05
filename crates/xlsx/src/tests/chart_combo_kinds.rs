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
