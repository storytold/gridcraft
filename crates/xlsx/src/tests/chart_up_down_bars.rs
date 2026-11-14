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
    assert_eq!(
        c.up_down_bars,
        Some(ChartUpDownBars {
            down_shadow: None,
            up_shadow: None,
            down_outline: Default::default(),
            up_outline: Default::default(),
            gap_width: 50,
            up: None,
            down: None,
            up_opacity: None,
            down_opacity: None
        })
    );
    let c = chart(
        "<c:upDownBars><c:gapWidth val=\"150%\"/><c:upBars/><c:downBars><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:downBars></c:upDownBars>",
    );
    assert_eq!(
        c.up_down_bars,
        Some(ChartUpDownBars {
            down_shadow: None,
            up_shadow: None,
            down_outline: Default::default(),
            up_outline: Default::default(),
            gap_width: 150,
            up: None,
            down: Some(Color::Rgb(0xFF0000)),
            up_opacity: None,
            down_opacity: None
        })
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
        ChartUpDownBars {
            down_shadow: None,
            up_shadow: None,
            down_outline: Default::default(),
            up_outline: Default::default(),
            gap_width: 150,
            up: None,
            down: None,
            up_opacity: None,
            down_opacity: None,
        },
        ChartUpDownBars {
            down_shadow: None,
            up_shadow: None,
            down_outline: Default::default(),
            up_outline: Default::default(),
            gap_width: 40,
            up: Some(Color::Rgb(0x00B050)),
            down: Some(Color::Rgb(0xFF0000)),
            up_opacity: None,
            down_opacity: Some(67),
        },
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

/// Bars on a group on the secondary axes aren't kept: they're drawn and written back only on the
/// primary axes, so a round trip would drop them anyway.
#[test]
fn up_down_bars_on_a_secondary_group_are_left_out() {
    let ser = |i: u32| {
        format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser>"
        )
    };
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea>\
        <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
        <c:lineChart><c:grouping val=\"standard\"/>{}{}<c:upDownBars><c:gapWidth val=\"150\"/><c:upBars/><c:downBars/></c:upDownBars><c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>\
        <c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1),
        ser(2)
    );
    assert_eq!(read_chart(&parse(xml.as_bytes()).unwrap()).unwrap().up_down_bars, None);
}

/// A bar's outline and inner shadow, as in a workbook formatted in Excel: no `a:ln` keeps
/// Excel's default outline, `a:noFill` removes it; the shadow's lengths are in points.
#[test]
fn up_down_bar_outlines_and_shadows_are_read_and_written_back() {
    use gridcraft_model::{BarOutline, InnerShadow};
    let shadow = r#"<a:effectLst><a:innerShdw blurRad="711200" dist="50800" dir="10800000"><a:prstClr val="black"><a:alpha val="50000"/></a:prstClr></a:innerShdw></a:effectLst>"#;
    let c = chart(&format!(
        r#"<c:upDownBars><c:gapWidth val="150"/><c:upBars><c:spPr><a:ln w="19050"><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill></a:ln></c:spPr></c:upBars><c:downBars><c:spPr><a:solidFill><a:srgbClr val="FF0000"><a:alpha val="67000"/></a:srgbClr></a:solidFill>{shadow}</c:spPr></c:downBars></c:upDownBars>"#
    ));
    let u = c.up_down_bars.unwrap();
    assert_eq!(u.up_outline, BarOutline::Solid { color: Some(Color::Rgb(0x0000FF)), width: Some(1.5) });
    assert_eq!(u.down_outline, BarOutline::Auto);
    assert_eq!(u.up_shadow, None);
    assert_eq!(u.down_shadow, Some(InnerShadow { blur: 56.0, distance: 4.0, direction: 180.0, color: Color::Rgb(0), opacity: 50 }));
    let none = chart(r#"<c:upDownBars><c:upBars/><c:downBars><c:spPr><a:ln><a:noFill/></a:ln></c:spPr></c:downBars></c:upDownBars>"#);
    assert_eq!(none.up_down_bars.unwrap().down_outline, BarOutline::None);

    let mut wb = gridcraft_model::Workbook::new();
    let mut c = c;
    c.series = vec![gridcraft_model::Series { values: "Sheet1!$A$1:$A$2".into(), ..Default::default() }; 2];
    c.up_down_bars.as_mut().unwrap().up_outline = BarOutline::None;
    std::sync::Arc::make_mut(&mut wb.sheets[0]).charts.push(c);
    let bytes = write_xlsx(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/charts/chart1.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains(r#"<c:upBars><c:spPr><a:ln><a:noFill/></a:ln></c:spPr></c:upBars>"#), "{xml}");
    assert!(
        xml.contains(r#"<a:effectLst><a:innerShdw blurRad="711200" dist="50800" dir="10800000"><a:srgbClr val="000000"><a:alpha val="50000"/></a:srgbClr></a:innerShdw></a:effectLst>"#),
        "{xml}"
    );
    let back = read_xlsx(&bytes).unwrap().0.sheets[0].charts[0].up_down_bars.unwrap();
    assert_eq!(back.up_outline, BarOutline::None);
    assert_eq!(back.down_shadow, u.down_shadow);
    assert_eq!((back.down, back.down_opacity), (Some(Color::Rgb(0xFF0000)), Some(67)));
}

/// Shadow lengths and widths from a file stay bounded; a direction wraps to 0..360.
#[test]
fn up_down_bar_shadow_values_are_bounded() {
    let c = chart(
        r#"<c:upDownBars><c:upBars/><c:downBars><c:spPr><a:ln w="-5"/><a:effectLst><a:innerShdw blurRad="99999999999" dist="-3" dir="-5400000"><a:srgbClr val="FFFFFF"/></a:innerShdw></a:effectLst></c:spPr></c:downBars></c:upDownBars>"#,
    );
    let u = c.up_down_bars.unwrap();
    assert_eq!(u.down_outline, gridcraft_model::BarOutline::Solid { color: None, width: None });
    let s = u.down_shadow.unwrap();
    assert_eq!((s.blur, s.distance, s.direction, s.opacity, s.color), (1584.0, 0.0, 270.0, 100, Color::Rgb(0xFFFFFF)));
}
