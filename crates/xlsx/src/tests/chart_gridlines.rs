//! Synthetic DrawingML only: the colour and width of a chart's major gridlines, as Excel draws
//! them and as GridCraft writes them back.

use crate::{chart::read_chart, read_xlsx, write_xlsx, xml::parse};
use gridcraft_model::Color;

fn chart(before_chart: &str, gridlines: &str) -> gridcraft_model::Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">{before_chart}<c:chart><c:plotArea><c:lineChart><c:grouping val=\"standard\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart><c:catAx><c:axId val=\"1\"/><c:axPos val=\"b\"/></c:catAx><c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/>{gridlines}</c:valAx></c:plotArea></c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

const C14: &str = r#"<mc:AlternateContent><mc:Choice Requires="c14" xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent>"#;

/// Measured in Excel on a synthetic chart without gridline formatting: mid grey (#898989) when the
/// file has a `c14:style`, as Excel 2010 and later save it, for every style; black with only a
/// `c:style`, or none.
#[test]
fn unformatted_gridlines_take_excels_default_for_the_file() {
    let c = chart(C14, "<c:majorGridlines/>");
    assert!(c.gridlines);
    assert_eq!((c.gridline_color, c.gridline_width), (Some(Color::Rgb(0x898989)), None));
    for older in [r#"<c:style val="26"/>"#, ""] {
        let c = chart(older, "<c:majorGridlines/>");
        assert_eq!(c.gridline_color, Some(Color::Rgb(0)), "{older}");
    }
    let none = chart(C14, "");
    assert!(!none.gridlines);
    assert_eq!(none.gridline_color, None);
}

#[test]
fn formatted_gridlines_keep_their_line() {
    let c = chart(
        C14,
        r#"<c:majorGridlines><c:spPr><a:ln w="19050"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln></c:spPr></c:majorGridlines>"#,
    );
    assert_eq!((c.gridlines, c.gridline_color, c.gridline_width), (true, Some(Color::Rgb(0xFF0000)), Some(1.5)));
    // A line with no fill hides them.
    let c = chart(C14, r#"<c:majorGridlines><c:spPr><a:ln><a:noFill/></a:ln></c:spPr></c:majorGridlines>"#);
    assert!(!c.gridlines);
}

/// Written with their line, so Excel draws them as GridCraft does: GridCraft's light grey and
/// 0.75 pt when the chart doesn't set them.
#[test]
fn gridlines_are_written_with_their_line() {
    let written = |color: Option<Color>, width: Option<f32>| {
        let mut c = chart(C14, "<c:majorGridlines/>");
        c.series[0].values = "Sheet1!$A$1:$A$2".into();
        (c.gridline_color, c.gridline_width) = (color, width);
        let mut wb = gridcraft_model::Workbook::new();
        std::sync::Arc::make_mut(&mut wb.sheets[0]).charts.push(c);
        let bytes = write_xlsx(&wb).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("xl/charts/chart1.xml").unwrap(), &mut xml).unwrap();
        let back = read_xlsx(&bytes).unwrap().0.sheets[0].charts[0].clone();
        (xml, back)
    };
    let (xml, back) = written(None, None);
    assert!(
        xml.contains(
            r#"<c:majorGridlines><c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="D9D9D9"/></a:solidFill></a:ln></c:spPr></c:majorGridlines>"#
        ),
        "{xml}"
    );
    assert_eq!((back.gridline_color, back.gridline_width), (Some(Color::Rgb(0xD9D9D9)), Some(0.75)));
    let (xml, back) = written(Some(Color::Rgb(0x898989)), Some(2.0));
    assert!(xml.contains(r#"<a:ln w="25400"><a:solidFill><a:srgbClr val="898989"/>"#), "{xml}");
    assert_eq!((back.gridline_color, back.gridline_width), (Some(Color::Rgb(0x898989)), Some(2.0)));
}
