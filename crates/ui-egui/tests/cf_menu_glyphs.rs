//! The Conditional Formatting menu draws a picture beside every item (#1394).

use egui::{Color32, Rect, Shape, pos2, vec2};
use gridcraft_ui_egui::cf_glyphs::{self, CfGlyph};

fn paint(glyph: CfGlyph) -> (Rect, Vec<Shape>) {
    let ctx = egui::Context::default();
    let r = Rect::from_min_size(pos2(20.0, 20.0), vec2(cf_glyphs::width(glyph), 18.0));
    let mut output = ctx.run_ui(Default::default(), |ui| cf_glyphs::paint(ui.painter(), r, glyph, Color32::BLACK));
    output.textures_delta.clear();
    (r, output.shapes.into_iter().map(|s| s.shape).collect())
}

fn fills(shapes: &[Shape]) -> Vec<Color32> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r) => Some(r.fill),
            Shape::Path(p) => Some(p.fill),
            Shape::Circle(c) => Some(c.fill),
            _ => None,
        })
        .collect()
}

#[test]
fn every_item_has_a_picture_inside_its_room() {
    let scale = [Color32::RED, Color32::GREEN];
    for glyph in [
        CfGlyph::HighlightCells,
        CfGlyph::TopBottom,
        CfGlyph::DataBars,
        CfGlyph::ColorScales,
        CfGlyph::IconSets,
        CfGlyph::NewRule,
        CfGlyph::ClearRules,
        CfGlyph::ManageRules,
        CfGlyph::Cell(">"),
        CfGlyph::Rank { top: true, percent: true },
        CfGlyph::Average { above: false },
        CfGlyph::DataBar(Color32::BLUE),
        CfGlyph::ColorScale(&scale),
        CfGlyph::IconSet("5Arrows"),
    ] {
        let (r, shapes) = paint(glyph);
        assert!(!shapes.is_empty(), "{glyph:?} paints nothing");
        for s in &shapes {
            let b = s.visual_bounding_rect();
            assert!(r.expand(2.0).contains_rect(b), "{glyph:?} draws outside its room: {b:?} not in {r:?}");
        }
    }
}

#[test]
fn previews_show_the_preset() {
    let (_, bars) = paint(CfGlyph::DataBar(Color32::from_rgb(0x63, 0xBE, 0x7B)));
    assert!(fills(&bars).contains(&Color32::from_rgb(0x63, 0xBE, 0x7B)), "data bar starts in its colour");

    let scale = [Color32::from_rgb(0x63, 0xBE, 0x7B), Color32::from_rgb(0xFF, 0xEB, 0x84), Color32::from_rgb(0xF8, 0x69, 0x6B)];
    let got = fills(&paint(CfGlyph::ColorScale(&scale)).1);
    assert_eq!(got.first(), Some(&scale[0]), "scale starts at its low colour");
    assert!(got.contains(&scale[2]), "scale ends at its high colour");

    for (set, n) in [("3Arrows", 3), ("4Rating", 4), ("5Quarters", 5)] {
        assert_eq!(cf_glyphs::width(CfGlyph::IconSet(set)), 18.0 * n as f32 - 4.0, "{set} leaves room for {n} icons");
    }
    let arrows = paint(CfGlyph::IconSet("3Arrows")).1;
    assert_eq!(arrows.iter().filter(|s| matches!(s, Shape::Path(_))).count(), 6, "a head and a stem per arrow");
}
