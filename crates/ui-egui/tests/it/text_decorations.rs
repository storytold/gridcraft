//! Underline and strikethrough in the grid sit where the font puts them (#4401): the strike line
//! through the lowercase letters, the underline just under the baseline, both from the text's
//! own baseline rather than a fixed fraction of its line height.

use egui::{Color32, Pos2, Shape};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Where the grid drew `text`: its baseline, font size and horizontal extent, and its colour.
struct Painted {
    baseline: f32,
    size: f32,
    left: f32,
    right: f32,
    color: Color32,
}

fn painted_text(h: &Harness, text: &str) -> Painted {
    let output = h.output();
    let shape = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == text => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the grid draws {text:?}"));
    let row = shape.galley.rows.first().expect("one row");
    let glyph = row.glyphs.first().expect("a glyph");
    let size = shape.galley.job.sections.first().expect("a section").format.font_id.size;
    let color = shape.galley.job.sections.first().expect("a section").format.color;
    let rect = row.rect().translate(shape.pos.to_vec2());
    Painted { baseline: shape.pos.y + row.pos.y + glyph.pos.y, size, left: rect.left(), right: rect.right(), color }
}

/// Horizontal lines in `color` spanning the text, as (y, width).
fn lines_under(h: &Harness, t: &Painted) -> Vec<(f32, f32)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::LineSegment { points: [a, b], stroke } if stroke.color == t.color && is_span(*a, *b, t) => Some((a.y, stroke.width)),
            _ => None,
        })
        .collect()
}

fn is_span(a: Pos2, b: Pos2, t: &Painted) -> bool {
    (a.y - b.y).abs() < 0.01 && (a.x - t.left).abs() < 0.5 && (b.x - t.right).abs() < 0.5
}

fn harness(zoom: u32) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Underlined"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Struck"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": "Twice"})).unwrap();
    s.execute("home.underline", json!({"range": "A1"})).unwrap();
    s.execute("home.strikethrough", json!({"range": "A2"})).unwrap();
    s.execute("home.underline", json!({"range": "A3", "style": "double"})).unwrap();
    s.execute("view.zoom", json!({"percent": zoom})).unwrap();
    s.execute("selection.set", json!({"cell": "E9"})).unwrap();
    crate::support::harness(s, 4)
}

#[test]
fn strikethrough_crosses_the_lowercase_letters_at_every_zoom() {
    for zoom in [100, 300] {
        let h = harness(zoom);
        let t = painted_text(&h, "Struck");
        let lines = lines_under(&h, &t);
        assert_eq!(lines.len(), 1, "one strike line at {zoom}%: {lines:?}");
        let (y, width) = lines[0];
        // Above the baseline by a quarter of the font size or so: through the x-height, not on the baseline.
        let above = t.baseline - y;
        assert!((0.15 * t.size..=0.45 * t.size).contains(&above), "strike {above:.1} px above the baseline of a {:.1} px font at {zoom}%", t.size);
        assert!(width >= 1.0 && width <= (0.1 * t.size).max(1.0), "strike width {width} at {zoom}%");
    }
}

#[test]
fn underline_sits_just_under_the_baseline_at_every_zoom() {
    for zoom in [100, 300] {
        let h = harness(zoom);
        let t = painted_text(&h, "Underlined");
        let lines = lines_under(&h, &t);
        assert_eq!(lines.len(), 1, "one underline at {zoom}%: {lines:?}");
        let below = lines[0].0 - t.baseline;
        // Clear of the letters by a share of the font size, not a fixed pixel.
        assert!((0.08 * t.size..=0.2 * t.size).contains(&below), "underline {below:.1} px under the baseline of a {:.1} px font at {zoom}%", t.size);
    }
}

#[test]
fn double_underline_draws_two_lines_under_the_baseline_that_scale_with_zoom() {
    let gap = |zoom| {
        let h = harness(zoom);
        let t = painted_text(&h, "Twice");
        let mut ys: Vec<f32> = lines_under(&h, &t).into_iter().map(|(y, _)| y).collect();
        ys.sort_by(f32::total_cmp);
        assert_eq!(ys.len(), 2, "two underlines at {zoom}%: {ys:?}");
        assert!(ys[0] > t.baseline && ys[1] - t.baseline <= 0.35 * t.size, "both lines just under the baseline at {zoom}%: {ys:?}");
        ys[1] - ys[0]
    };
    let (small, large) = (gap(100), gap(300));
    assert!(large > 2.5 * small, "the gap grows with the font: {small} at 100%, {large} at 300%");
}
