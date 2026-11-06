//! Synthetic DrawingML only: how a chart shows empty cells (`c:dispBlanksAs`), read and written.

use crate::{chart::read_chart, read_xlsx, write_xlsx, xml::parse};
use gridcraft_model::{ChartEmptyCells, Workbook};

fn chart(blanks: &str) -> gridcraft_model::Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
        <c:lineChart><c:grouping val=\"standard\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
        <c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"2\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val>\
        </c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart><c:catAx/><c:valAx/></c:plotArea>{blanks}</c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

#[test]
fn empty_cells_are_read_with_the_schemas_defaults() {
    assert_eq!(chart("").empty_cells, ChartEmptyCells::Gap, "no element: gaps");
    assert_eq!(chart("<c:dispBlanksAs/>").empty_cells, ChartEmptyCells::Zero, "an element without val: zero");
    assert_eq!(chart("<c:dispBlanksAs val=\"zero\"/>").empty_cells, ChartEmptyCells::Zero);
    assert_eq!(chart("<c:dispBlanksAs val=\"span\"/>").empty_cells, ChartEmptyCells::Span);
    assert_eq!(chart("<c:dispBlanksAs val=\"gap\"/>").empty_cells, ChartEmptyCells::Gap);
}

#[test]
fn empty_cells_survive_a_save() {
    for blanks in [ChartEmptyCells::Gap, ChartEmptyCells::Zero, ChartEmptyCells::Span] {
        let mut wb = Workbook::new();
        let mut c = chart("");
        c.empty_cells = blanks;
        wb.sheet_mut(0).unwrap().charts.push(c);
        let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(back.sheet(0).unwrap().charts.first().map(|c| c.empty_cells), Some(blanks));
    }
}
