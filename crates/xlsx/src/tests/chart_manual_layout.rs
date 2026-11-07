//! Synthetic DrawingML only: chart parts the file places by hand.

use crate::{chart::read_chart, xml::parse};
use gridcraft_model::ChartRect;

fn chart(title_layout: &str, plot_layout: &str) -> gridcraft_model::Chart {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart>\
        <c:title><c:tx><c:rich><a:p xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:r><a:t>T</a:t></a:r></a:p></c:rich></c:tx>{title_layout}</c:title>\
        <c:plotArea>{plot_layout}<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
        <c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>12</c:v></c:pt></c:numLit></c:val>\
        </c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"
    );
    read_chart(&parse(xml.as_bytes()).unwrap()).unwrap()
}

fn layout(target: &str, modes: &str, xywh: &str) -> String {
    format!("<c:layout><c:manualLayout>{target}{modes}{xywh}</c:manualLayout></c:layout>")
}

const EDGE: &str = "<c:xMode val=\"edge\"/><c:yMode val=\"edge\"/>";

/// As Excel saves a plot area and a title moved by hand (observed on a synthetic workbook).
#[test]
fn hand_placed_parts_are_read() {
    let c = chart(
        &layout("", EDGE, "<c:x val=\"0.04\"/><c:y val=\"0.035\"/>"),
        &layout("<c:layoutTarget val=\"inner\"/>", EDGE, "<c:x val=\"0.0615\"/><c:y val=\"0.178\"/><c:w val=\"0.5\"/><c:h val=\"0.5\"/>"),
    );
    assert_eq!(c.layout.title, Some([0.04, 0.035]));
    assert_eq!(c.layout.plot, Some(ChartRect { x: 0.0615, y: 0.178, w: 0.5, h: 0.5 }));
    assert!(c.layout.plot_inner);
    // Without a target the plot area holds its axis labels too.
    let c = chart("", &layout("", EDGE, "<c:x val=\"0.1\"/><c:y val=\"0.1\"/><c:w val=\"0.8\"/><c:h val=\"0.7\"/>"));
    assert!(c.layout.plot.is_some() && !c.layout.plot_inner);
}

/// `wMode` and `hMode` set to `edge` make `w` and `h` the right and bottom edges, not a size.
#[test]
fn edges_given_for_width_and_height_become_a_size() {
    let modes = "<c:xMode val=\"edge\"/><c:yMode val=\"edge\"/><c:wMode val=\"edge\"/><c:hMode val=\"edge\"/>";
    let c = chart(
        "",
        &layout("<c:layoutTarget val=\"inner\"/>", modes, "<c:x val=\"0.25\"/><c:y val=\"0.125\"/><c:w val=\"0.75\"/><c:h val=\"0.625\"/>"),
    );
    assert_eq!(c.layout.plot, Some(ChartRect { x: 0.25, y: 0.125, w: 0.5, h: 0.5 }));
    // A right edge left of the plot's own left edge leaves no size, and so no plot area.
    let c = chart("", &layout("", modes, "<c:x val=\"0.5\"/><c:y val=\"0.1\"/><c:w val=\"0.25\"/><c:h val=\"0.9\"/>"));
    assert_eq!(c.layout.plot, None);
}

#[test]
fn layouts_relative_to_the_automatic_place_or_without_a_size_are_left_out() {
    // `factor` (the default mode) moves a part from where it would go by itself: not kept.
    let c = chart(
        &layout("", "", "<c:x val=\"0.1\"/><c:y val=\"0.1\"/>"),
        &layout("", "", "<c:x val=\"0.1\"/><c:y val=\"0.1\"/><c:w val=\"0.5\"/><c:h val=\"0.5\"/>"),
    );
    assert_eq!(c.layout, Default::default());
    // A plot area needs a size.
    assert_eq!(chart("", &layout("", EDGE, "<c:x val=\"0.1\"/><c:y val=\"0.1\"/>")).layout.plot, None);
    assert_eq!(chart("", "").layout, Default::default());
}
