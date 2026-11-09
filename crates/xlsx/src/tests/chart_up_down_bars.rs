//! Synthetic DrawingML only: up/down bars of a line chart, as Excel saves them, and round trips.

use crate::{chart::read_chart, read_xlsx, write_xlsx, xml::parse};
use gridcraft_model::{ChartUpDownBars, Color};

fn chart(extra: &str) -> gridcraft_model::Chart {
    let ser = |i: u32, v: &str| {
        format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{v}</c:v></c:pt></c:numLit></c:val></c:ser>"
        )
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><c:chart><c:plotArea>\
        <c:lineChart><c:grouping val=\"standard\"/>{}{}{extra}<c:marker val=\"1\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>",
        ser(0, "10"),
        ser(1, "30")
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

/// As Excel saves them (observed on a synthetic workbook): a gap width and empty `upBars` and
/// `downBars` at its default colours, or a fill of their own.
#[test]
fn up_down_bars_are_read() {
    assert_eq!(chart("").up_down_bars, None);
    let c = chart("<c:upDownBars><c:gapWidth val=\"50\"/><c:upBars/><c:downBars/></c:upDownBars>");
    assert_eq!(c.up_down_bars, Some(ChartUpDownBars { gap_width: 50, up: None, down: None, up_opacity: None, down_opacity: None }));
    let c = chart(
        "<c:upDownBars><c:gapWidth val=\"150%\"/><c:upBars/><c:downBars><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:downBars></c:upDownBars>",
    );
    assert_eq!(
        c.up_down_bars,
        Some(ChartUpDownBars { gap_width: 150, up: None, down: Some(Color::Rgb(0xFF0000)), up_opacity: None, down_opacity: None })
    );
    // A fill's alpha, in thousandths of a percent, is its opacity.
    let c = chart(
        "<c:upDownBars><c:upBars/><c:downBars><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"><a:alpha val=\"67000\"/></a:srgbClr></a:solidFill></c:spPr></c:downBars></c:upDownBars>",
    );
    assert_eq!(c.up_down_bars.map(|u| (u.up_opacity, u.down_opacity)), Some((None, Some(67))));
    // No gap width is Excel's 150; an out-of-range one is held to 500.
    assert_eq!(chart("<c:upDownBars><c:upBars/><c:downBars/></c:upDownBars>").up_down_bars.map(|u| u.gap_width), Some(150));
    assert_eq!(chart("<c:upDownBars><c:gapWidth val=\"9000\"/></c:upDownBars>").up_down_bars.map(|u| u.gap_width), Some(500));
}

#[test]
fn up_down_bars_survive_xlsx_save_reopen() {
    for bars in [
        ChartUpDownBars { gap_width: 150, up: None, down: None, up_opacity: None, down_opacity: None },
        ChartUpDownBars { gap_width: 40, up: Some(Color::Rgb(0x00B050)), down: Some(Color::Rgb(0xFF0000)), up_opacity: None, down_opacity: Some(67) },
    ] {
        let mut c = chart("");
        c.id = 1;
        c.up_down_bars = Some(bars);
        let mut wb = gridcraft_model::Workbook::new();
        wb.sheet_mut(0).unwrap().charts.push(c);
        let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(back.sheets[0].charts[0].up_down_bars, Some(bars));
    }
    // A chart without them gains none.
    let mut c = chart("");
    c.id = 1;
    let mut wb = gridcraft_model::Workbook::new();
    wb.sheet_mut(0).unwrap().charts.push(c);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheets[0].charts[0].up_down_bars, None);
}
