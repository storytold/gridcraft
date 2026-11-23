//! The selection's outline lies on the gridlines: each side covers the gridline that bounds the
//! selected cells and reaches outward from it, a whole number of device pixels wide, whatever
//! the display scale. Nothing of a gridline shows inside it.
//!
//! The grid works out where the renderer will put each gridline (`grid::gridline_span`). These
//! tests ask the renderer itself, by tessellating what was painted, so a renderer that changes
//! how it moves a line onto device pixels is noticed here.

use egui::epaint::{ClippedShape, Primitive};
use egui::{Rect, Shape};
use gridcraft_engine::Session;
use gridcraft_engine::core::RangeRef;
use gridcraft_ui_egui::grid::{Geo, outline_sides};
use gridcraft_ui_egui::theme::Tokens;
use serde_json::json;

use crate::common::{Harness, app, builder, harness_with};

/// The display scales tried: pixels to a point.
const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

fn harness(ppp: f32, select: &str) -> Harness {
    harness_on(ppp, select, false)
}

fn harness_on(ppp: f32, select: &str, right_to_left: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.rightToLeft", json!({"on": right_to_left})).unwrap();
    session.execute("selection.set", json!({"range": select})).unwrap();
    harness_with(builder().with_size(egui::vec2(900.0, 600.0)).with_pixels_per_point(ppp), app(session), 4)
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

/// Where a cell or range is on screen (before a right-to-left sheet is mirrored).
fn cells(h: &Harness, a1: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    geo(h).range_rect(sheet, RangeRef::parse(a1).unwrap())
}

/// The shapes of the last frame, in the order they were painted.
fn painted(h: &Harness) -> Vec<&Shape> {
    fn flat<'a>(shape: &'a Shape, out: &mut Vec<&'a Shape>) {
        match shape {
            Shape::Vec(inner) => inner.iter().for_each(|s| flat(s, out)),
            other => out.push(other),
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        flat(&clipped.shape, &mut out);
    }
    out
}

/// Where the renderer puts a shape: the bounds of its mesh, in points. A line or a filled
/// rectangle is tessellated evenly about its own middle, so the middle of these bounds is the
/// middle of what is drawn, after the renderer has moved it onto device pixels.
fn rendered(h: &Harness, shape: &Shape) -> Rect {
    let clipped = ClippedShape::new(Rect::EVERYTHING, shape.clone());
    let mut bounds = Rect::NOTHING;
    for primitive in h.ctx.tessellate(vec![clipped], h.ctx.pixels_per_point()) {
        if let Primitive::Mesh(mesh) = primitive.primitive {
            mesh.vertices.iter().for_each(|v| bounds.extend_with(v.pos));
        }
    }
    bounds
}

/// The device pixels a line `width` points wide covers half of or more, when the renderer
/// centres it at `centre`: the first, and the one past the last.
fn pixels(centre: f32, width: f32, ppp: f32) -> (f32, f32) {
    let first = ((centre - width / 2.0) * ppp).round();
    (first, ((centre + width / 2.0) * ppp).round().max(first + 1.0))
}

/// The gridline the grid painted nearest the cell edge at `edge`: an upright one for an edge
/// along x, a level one for an edge along y. Its pixels, as the renderer draws it.
fn gridline(h: &Harness, edge: f32, upright: bool) -> (f32, f32) {
    gridline_near(h, edge - 0.5, upright)
}

/// The gridline the grid painted nearest `centre`, before the renderer moved it onto pixels.
fn gridline_near(h: &Harness, centre: f32, upright: bool) -> (f32, f32) {
    let colour = Tokens::get(&h.ctx).gridline;
    let along = |points: &[egui::Pos2; 2]| if upright { (points[0].x, points[1].x) } else { (points[0].y, points[1].y) };
    let found = painted(h)
        .into_iter()
        .filter(|shape| matches!(shape, Shape::LineSegment { points, stroke } if stroke.color == colour && along(points).0 == along(points).1))
        .min_by(|a, b| {
            let off = |shape: &Shape| match shape {
                Shape::LineSegment { points, .. } => (along(points).0 - centre).abs(),
                _ => f32::INFINITY,
            };
            off(a).total_cmp(&off(b))
        })
        .expect("a gridline is painted");
    let Shape::LineSegment { points, stroke } = found else { unreachable!() };
    assert!((along(points).0 - centre).abs() < 1.0, "the gridline near {centre} is at {}", along(points).0);
    let middle = rendered(h, found).center();
    pixels(if upright { middle.x } else { middle.y }, stroke.width, h.ctx.pixels_per_point())
}

/// A side of the outline as the renderer draws it: its pixels across its width.
fn side_pixels(h: &Harness, side: Rect, upright: bool) -> (f32, f32) {
    let green = Tokens::get(&h.ctx).sel_border;
    let near = |a: Rect, b: Rect| (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01;
    let shape = painted(h)
        .into_iter()
        .find(|shape| matches!(shape, Shape::Rect(r) if r.fill == green && near(r.rect, side)))
        .unwrap_or_else(|| panic!("the side {side:?} is painted"));
    let middle = rendered(h, shape).center();
    let ppp = h.ctx.pixels_per_point();
    if upright { pixels(middle.x, side.width(), ppp) } else { pixels(middle.y, side.height(), ppp) }
}

#[test]
fn each_side_of_the_outline_covers_its_gridline_and_reaches_outward_at_every_scale() {
    for ppp in SCALES {
        for select in ["C4", "B3:D6"] {
            let h = harness(ppp, select);
            let at = cells(&h, select);
            let [left, top, right, bottom] = outline_sides(at, ppp, None);
            let what = format!("{select} at {ppp} pixels a point");
            // The near sides end where their gridline ends, and start at or before it.
            for (side, edge, upright) in [(left, at.left(), true), (top, at.top(), false)] {
                let (line, drawn) = (gridline(&h, edge, upright), side_pixels(&h, side, upright));
                assert_eq!(drawn.1, line.1, "{what}: the side {drawn:?} ends with its gridline {line:?}");
                assert!(drawn.0 <= line.0, "{what}: and covers it: {drawn:?} over {line:?}");
            }
            // The far sides start where their gridline starts, and end at or after it.
            for (side, edge, upright) in [(right, at.right(), true), (bottom, at.bottom(), false)] {
                let (line, drawn) = (gridline(&h, edge, upright), side_pixels(&h, side, upright));
                assert_eq!(drawn.0, line.0, "{what}: the side {drawn:?} starts with its gridline {line:?}");
                assert!(drawn.1 >= line.1, "{what}: and covers it: {drawn:?} over {line:?}");
            }
            // Every side is the same whole number of pixels.
            let weights: Vec<f32> = [(left, true), (top, false), (right, true), (bottom, false)]
                .map(|(side, upright)| side_pixels(&h, side, upright))
                .iter()
                .map(|p| p.1 - p.0)
                .collect();
            assert!(weights.iter().all(|w| *w == weights[0] && *w >= 1.0), "{what}: {weights:?}");
            assert_eq!(weights[0], (2.0 * ppp).round(), "{what}: two points, to the nearest pixel");
        }
    }
}

#[test]
fn on_a_right_to_left_sheet_the_mirrored_outline_still_covers_its_gridlines() {
    for ppp in SCALES {
        for select in ["C4", "B3:D6"] {
            let h = harness_on(ppp, select, true);
            let g = geo(&h);
            assert!(g.rtl);
            let at = cells(&h, select);
            let [left, top, right, bottom] = outline_sides(at, ppp, g.mirror_axis());
            let what = format!("{select} at {ppp} pixels a point, right to left");
            // Everything is painted as for a left-to-right sheet and then mirrored, sides and
            // gridlines alike, so the cells' left side is drawn on the right. The renderer puts
            // the mirrored lines on pixels.
            let line = |edge: f32| gridline_near(&h, g.mirror_x(edge.round() - 0.5), true);
            // The side drawn on the left starts at or before its gridline and ends with it.
            let (drawn, under) = (side_pixels(&h, g.mirror_rect(right), true), line(at.right()));
            assert_eq!(drawn.1, under.1, "{what}: the left side {drawn:?} ends with its gridline {under:?}");
            assert!(drawn.0 <= under.0, "{what}: and covers it: {drawn:?} over {under:?}");
            // The side drawn on the right starts with its gridline and ends at or after it.
            let (drawn, under) = (side_pixels(&h, g.mirror_rect(left), true), line(at.left()));
            assert_eq!(drawn.0, under.0, "{what}: the right side {drawn:?} starts with its gridline {under:?}");
            assert!(drawn.1 >= under.1, "{what}: and covers it: {drawn:?} over {under:?}");
            // The top and bottom are not moved up or down.
            for (side, edge, near) in [(top, at.top(), true), (bottom, at.bottom(), false)] {
                let (drawn, under) = (side_pixels(&h, g.mirror_rect(side), false), gridline(&h, edge, false));
                let lies_on = if near { drawn.1 == under.1 && drawn.0 <= under.0 } else { drawn.0 == under.0 && drawn.1 >= under.1 };
                assert!(lies_on, "{what}: {drawn:?} on its gridline {under:?}");
            }
            // Every side is the same whole number of pixels.
            let weights: Vec<f32> = [(left, true), (top, false), (right, true), (bottom, false)]
                .map(|(side, upright)| side_pixels(&h, g.mirror_rect(side), upright))
                .iter()
                .map(|p| p.1 - p.0)
                .collect();
            assert!(weights.iter().all(|w| *w == weights[0] && *w >= 1.0), "{what}: {weights:?}");
        }
    }
}

#[test]
fn the_outline_goes_round_the_inside_of_the_cells() {
    // At a pixel a point the arithmetic is plain: the gridlines are the last point of the cell
    // before and the last point of the cell itself, and the outline is each of those and the
    // point outside it.
    let h = harness(1.0, "C4");
    let at = cells(&h, "C4");
    let (l, t, r, b) = (at.left().round(), at.top().round(), at.right().round(), at.bottom().round());
    let [left, top, right, bottom] = outline_sides(at, 1.0, None);
    assert_eq!((left.left(), left.right()), (l - 2.0, l));
    assert_eq!((top.top(), top.bottom()), (t - 2.0, t));
    assert_eq!((right.left(), right.right()), (r - 1.0, r + 1.0));
    assert_eq!((bottom.top(), bottom.bottom()), (b - 1.0, b + 1.0));
}
