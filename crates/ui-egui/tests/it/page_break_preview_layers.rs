//! Page Break Preview paints as Excel does: outside the printed area the cells turn an opaque
//! grey over their fills and gridlines, under their text and borders; objects stay on top of it,
//! and a page's number sits under the objects on that page.

use egui::{Color32, Shape};
use serde_json::json;

const RED: Color32 = Color32::from_rgb(0xFF, 0x00, 0x00);
const GREY: Color32 = Color32::from_gray(0xA0);

fn harness() -> crate::common::Harness {
    let mut session = crate::support::blank_session();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Inside", 5], [5, 5], [5, 5]]})).unwrap();
    session.execute("cell.set", json!({"cell": "H2", "input": "Outside"})).unwrap();
    session.execute("home.fillColor", json!({"range": "H2:J3", "color": "#FF0000"})).unwrap();
    session.execute("pageLayout.printArea", json!({"range": "A1:F30"})).unwrap();
    // One chart on page 1, over its middle, and one beside the printed area.
    session.execute("insert.chart", json!({"range": "A1:B3", "type": "column", "at": "A8", "width": 360, "height": 300})).unwrap();
    session.execute("insert.chart", json!({"range": "A1:B3", "type": "column", "at": "L6", "width": 240, "height": 160})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    session.execute("view.pageBreakPreview", json!({})).unwrap();
    crate::support::harness(session, 4)
}

/// Paint-order positions of the shapes `f` picks.
fn positions(h: &crate::common::Harness, f: impl Fn(&Shape) -> bool) -> Vec<usize> {
    h.output().shapes.iter().enumerate().filter(|(_, s)| f(&s.shape)).map(|(i, _)| i).collect()
}

fn filled(s: &Shape, c: Color32) -> bool {
    matches!(s, Shape::Rect(r) if r.fill == c)
}

fn text(s: &Shape, t: &str) -> bool {
    matches!(s, Shape::Text(x) if x.galley.job.text == t)
}

#[test]
fn unprinted_cells_are_grey_over_their_fills_and_under_their_text() {
    let h = harness();
    let red = positions(&h, |s| filled(s, RED));
    let grey = positions(&h, |s| filled(s, GREY));
    let outside = positions(&h, |s| text(s, "Outside"));
    assert!(!red.is_empty() && !grey.is_empty() && !outside.is_empty(), "red {red:?} grey {grey:?} text {outside:?}");
    // The fill is painted, then covered; the text comes after the grey.
    assert!(red.iter().max() < grey.iter().max(), "the grey covers the fill: red {red:?} grey {grey:?}");
    assert!(grey.iter().max() < outside.iter().min(), "the text is over the grey: grey {grey:?} text {outside:?}");
}

#[test]
fn objects_and_page_numbers_layer_as_in_excel() {
    let h = harness();
    let page = positions(&h, |s| text(s, "Page 1"));
    let charts = positions(&h, |s| text(s, "Series1"));
    let grey = positions(&h, |s| filled(s, GREY));
    assert_eq!(page.len(), 1, "one page label");
    assert_eq!(charts.len(), 2, "both charts' legends");
    assert!(page[0] < charts[0], "the page number is under the charts: {page:?} {charts:?}");
    // Nothing greys the chart beside the printed area.
    assert!(grey.iter().all(|g| g < &charts[0]), "grey {grey:?} after the charts {charts:?}");
    // Nor a dark veil (the objects' own soft shadows are fainter).
    let veil = positions(
        &h,
        |s| matches!(s, Shape::Rect(r) if r.fill.a() >= 40 && r.fill.a() < 255 && r.fill.r() == 0 && r.fill.g() == 0 && r.fill.b() == 0),
    );
    assert!(veil.iter().all(|v| v < &charts[0]), "no dark veil over the charts: {veil:?}");
}
