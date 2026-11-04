//! Draw tab: pen and eraser tools, ink strokes, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Color, Shape, ShapeKind, Sheet};
use serde_json::{Value as Json, json};

use super::ink_shape::{InkShape, recognise};
use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, ok, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.pen", "Pen", ["Draw", "Tools"], None, "{on?: bool, color?: hex, width?: 2}", has_doc, pen),
        cmd!(noundo "draw.eraser", "Eraser", ["Draw", "Tools"], None, "{on?: bool}", has_doc, eraser),
        cmd!(noundo "draw.lasso", "Lasso Select", ["Draw", "Tools"], None, "{} (selects ink by dragging; same as Select Objects)", has_doc, |s, _| {
            s.draw_tool = String::new();
            ok()
        }),
        cmd!("draw.stroke", "Ink Stroke", [], None, "{points: [[x,y],…] in sheet points, color?: hex, width?: 2}", has_doc, stroke),
        cmd!(
            "draw.inkToShape",
            "Ink to Shape",
            ["Draw", "Convert"],
            None,
            "{on?: bool} turns the conversion mode on or off; {id?} converts that ink stroke (default: the last)",
            has_doc,
            ink_to_shape
        ),
    ]
}

fn pen(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "pen");
    s.draw_tool = if on { "pen".into() } else { String::new() };
    if let Some(c) = str_param(p, "color") {
        s.draw_color = c.to_string();
    }
    if let Some(w) = f64_param(p, "width") {
        s.draw_width = w.clamp(0.5, 40.0) as f32;
    }
    Ok(json!({"tool": s.draw_tool, "color": s.draw_color, "width": s.draw_width}))
}

fn eraser(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "eraser");
    s.draw_tool = if on { "eraser".into() } else { String::new() };
    Ok(json!({"tool": s.draw_tool}))
}

fn parse_points(t: &str) -> Vec<[f32; 2]> {
    t.split(';').filter_map(|pt| pt.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    let pts: Vec<[f64; 2]> = p
        .get("points")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|q| Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]))
                .filter(|q| q[0].is_finite() && q[1].is_finite())
                .take(20_000)
                .collect()
        })
        .unwrap_or_default();
    if pts.len() < 2 {
        return Err(bad("draw.stroke", "a stroke needs at least two points"));
    }
    let minx = pts.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min).max(0.0);
    let miny = pts.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min).max(0.0);
    let maxx = pts.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max);
    let maxy = pts.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
    let color =
        str_param(p, "color").map(str::to_string).unwrap_or_else(|| if s.draw_color.is_empty() { "#1F5FC9".into() } else { s.draw_color.clone() });
    let width = f64_param(p, "width").map(|w| w as f32).unwrap_or(if s.draw_width <= 0.0 { 2.0 } else { s.draw_width }).clamp(0.5, 40.0);
    let d = s.doc()?;
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let id = d.wb.next_object_id();
    let shape = Shape {
        math: Default::default(),
        id,
        kind: ShapeKind::Ink,
        anchor: Anchor {
            cell,
            dx,
            dy,
            width: (maxx - minx).max(1.0) as f32,
            height: (maxy - miny).max(1.0) as f32,
            // Ink is a freehand mark: pinned where it was drawn, it does not follow cells.
            mode: gridcraft_model::AnchorMode::Absolute,
        },
        fill: Color::from_hex(&format!("#{:02X}0000", width.round().clamp(0.0, 255.0) as u8)).unwrap_or_default(),
        line: Color::from_hex(&color).unwrap_or(Color::rgb(0x1F, 0x5F, 0xC9)),
        text,
        format: Default::default(),
    };
    // Draw ▸ Ink to Shape is on: convert each completed stroke, as Excel does while the mode is
    // on. The ink and its conversion are one edit: a stroke that isn't a shape stays ink and the
    // command still succeeds (its result names no shape), so CLI, MCP and control callers never
    // see a failure for a document that changed.
    let convert = s.ink_to_shape;
    let kind = edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        sh.shapes.push(shape);
        let idx = sh.shapes.len() - 1;
        Ok(if convert { convert_at(sh, idx) } else { None })
    })?;
    // A stroke that stays ink is reported as such: the command succeeded and changed the sheet.
    let kind = kind.map_or_else(|| "Ink".to_string(), |kind| format!("{kind:?}"));
    Ok(json!({"shape": id, "kind": kind}))
}

/// Ink width is stored in the red channel of `fill` (shapes have no width field).
pub fn ink_width(sh: &Shape) -> f32 {
    match sh.fill {
        Color::Rgb(v) => ((v >> 16) & 0xFF).max(1) as f32,
        _ => 2.0,
    }
}

pub fn ink_points(sh: &Shape) -> Vec<[f32; 2]> {
    parse_points(&sh.text)
}

fn ink_to_shape(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(id) = p.get("id") {
        // An explicit id that is not a stroke id is a bad parameter, not "the last stroke".
        let id = id.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("draw.inkToShape", "`id` must be an ink stroke id"))?;
        let kind = convert_ink(s, Some(id))?;
        return Ok(json!({"kind": format!("{kind:?}"), "on": s.ink_to_shape}));
    }
    // A present `on` that is not a boolean is a bad parameter, as an invalid `id` is: a caller that
    // mistypes the mode must not have the last stroke converted instead.
    if let Some(on) = p.get("on") {
        let on = on.as_bool().ok_or_else(|| bad("draw.inkToShape", "`on` must be true or false"))?;
        s.ink_to_shape = on;
        return Ok(json!({"on": on}));
    }
    // No parameters: convert the last stroke (the command's programmatic form).
    let kind = convert_ink(s, None)?;
    Ok(json!({"kind": format!("{kind:?}"), "on": s.ink_to_shape}))
}

/// Convert one ink stroke (by id, or the last one) to the nearest shape; returns the kind.
fn convert_ink(s: &mut Session, id: Option<u32>) -> Result<ShapeKind> {
    let si = s.doc()?.wb.active_sheet();
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let idx = match id {
            Some(id) => sh.shapes.iter().position(|x| x.id == id && x.kind == ShapeKind::Ink),
            None => sh.shapes.iter().rposition(|x| x.kind == ShapeKind::Ink),
        }
        .ok_or_else(|| bad("draw.inkToShape", "no ink stroke to convert"))?;
        convert_at(sh, idx).ok_or_else(|| bad("draw.inkToShape", "the stroke is not a shape"))
    })
}

/// Convert the ink stroke at `idx` in `sheet` to its preset shape, returning the kind. `None`
/// leaves the stroke as ink when the recogniser or its polygons don't map to a preset, so callers
/// that already changed the document can keep the stroke instead of failing.
fn convert_at(sheet: &mut Sheet, idx: usize) -> Option<ShapeKind> {
    let found = {
        let ink = sheet.shapes.get(idx)?;
        recognise(&ink_points(ink))?
    };
    let (kind, arrow) = match &found {
        InkShape::Line { a, b } => {
            // The line is the recognised chord, not the stroke's bounds (which may stray from it).
            let (anchor, flip_h, flip_v) = segment_anchor(sheet, idx, *a, *b)?;
            (ShapeKind::Line, Some((anchor, flip_h, flip_v)))
        }
        InkShape::Arrow { a, b, head_at_end, head_at_start } => {
            // A head at the start only means the stroke was drawn tip first: the arrow points
            // from `b` to `a`. A head at each end is Excel's double arrow.
            let double = *head_at_end && *head_at_start;
            let (tail, tip) = if !*head_at_end && *head_at_start { (*b, *a) } else { (*a, *b) };
            // The box holds the shaft, not the barbs, so a horizontal stroke stays horizontal.
            let (anchor, flip_h, flip_v) = segment_anchor(sheet, idx, tail, tip)?;
            // The line preset runs from the box's top-left corner, so mirroring the box is what
            // makes the arrow point back along the way it was drawn.
            let kind = if double { ShapeKind::ArrowDouble } else { ShapeKind::Arrow };
            (kind, Some((anchor, flip_h, flip_v)))
        }
        InkShape::Rectangle { .. } => (ShapeKind::Rectangle, None),
        InkShape::Ellipse { .. } => (ShapeKind::Ellipse, None),
        InkShape::Polygon(c) => {
            // The recognised kind and the flips that place its preset. A polygon with no preset
            // orientation stays ink instead of being drawn at the wrong corners.
            let (kind, flip_h, flip_v) = polygon_kind(c)?;
            // A polygon preset is inscribed in its box the way the recognised corners are in
            // theirs, so the box is the corners' bounds, not the stroke's (which strays outside
            // them where the simplifier dropped the wobble).
            let [x, y, w, h] = found.bounds();
            (kind, Some((box_anchor(sheet, idx, x, y, w.max(1.0), h.max(1.0))?, flip_h, flip_v)))
        }
    };
    // Excel's square and circle are the same presets with equal sides: take the recogniser's
    // snapped bounds (origin included, it centres a grown square) so the shape reads as one.
    let anchor = match kind {
        ShapeKind::Rectangle | ShapeKind::Ellipse => {
            let [x, y, w, h] = found.bounds();
            Some((box_anchor(sheet, idx, x, y, w.max(1.0), h.max(1.0))?, false, false))
        }
        _ => arrow,
    };
    let ink = sheet.shapes.get_mut(idx)?;
    let color = ink.line;
    ink.kind = kind;
    if let Some((anchor, flip_h, flip_v)) = anchor {
        ink.anchor = anchor;
        ink.format.flip_h = flip_h;
        ink.format.flip_v = flip_v;
    }
    ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
    ink.text = String::new();
    Some(kind)
}

/// The anchor of a box `w` × `h` at `x, y` (ink-local points), moved from the stroke's own box
/// and given a cell and offsets for its new origin.
fn box_anchor(sheet: &Sheet, idx: usize, x: f32, y: f32, w: f32, h: f32) -> Option<Anchor> {
    let base = &sheet.shapes.get(idx)?.anchor;
    let x0 = sheet.col_left(base.cell.col) + base.dx as f64 + x as f64;
    let y0 = sheet.row_top(base.cell.row) + base.dy as f64 + y as f64;
    let cell = CellRef::new(sheet.row_at(y0), sheet.col_at(x0));
    Some(Anchor { cell, dx: (x0 - sheet.col_left(cell.col)) as f32, dy: (y0 - sheet.row_top(cell.row)) as f32, width: w, height: h, mode: base.mode })
}

/// The anchor for a segment from `tail` to `tip` (ink-local points): the box between the two
/// ends, moved from the stroke's own box. Line and arrow geometry is that segment, not the
/// stroke's bounds (which include a line's stray points and an arrow's barbs). The flips say
/// which diagonal of the box the segment takes, so the renderer keeps its slope and direction.
fn segment_anchor(sheet: &Sheet, idx: usize, tail: [f32; 2], tip: [f32; 2]) -> Option<(Anchor, bool, bool)> {
    // The raw extents: a zero-extent box is what keeps a horizontal stroke horizontal and a
    // vertical one vertical (`insert.shape` makes a line the same way).
    let anchor = box_anchor(sheet, idx, tail[0].min(tip[0]), tail[1].min(tip[1]), (tip[0] - tail[0]).abs(), (tip[1] - tail[1]).abs())?;
    Some((anchor, tip[0] < tail[0], tip[1] < tail[1]))
}

/// Map a recognised polygon onto GridCraft's preset shapes, with the flips that place it: a
/// triangle is a right or an upright isosceles one, a quad is a diamond, parallelogram or
/// trapezoid, and 5 and 6 sides are the pentagon/hexagon presets. Seven or eight corners have no
/// preset, and neither has a polygon whose corners do not sit where the preset draws them: the
/// stroke stays ink rather than being drawn at corners it did not have.
fn polygon_kind(c: &[[f32; 2]]) -> Option<(ShapeKind, bool, bool)> {
    match c.len() {
        3 => triangle_kind(c),
        4 => preset_flips(c, &[ShapeKind::Diamond, ShapeKind::Parallelogram, ShapeKind::Trapezoid]),
        5 => preset_flips(c, &[ShapeKind::Pentagon]),
        6 => preset_flips(c, &[ShapeKind::Hexagon]),
        _ => None,
    }
}

/// The kind a three-corner polygon maps to, with its flips: a right angle goes to the
/// `RightTriangle` preset, and a triangle matching the centred-apex `Triangle` preset keeps its
/// orientation, mirrored down when the apex is on the bottom edge. A scalene or rotated triangle
/// would be drawn with its apex moved, so it stays ink.
fn triangle_kind(p: &[[f32; 2]]) -> Option<(ShapeKind, bool, bool)> {
    if p.len() != 3 {
        return None;
    }
    let sides = [dist(p[0], p[1]), dist(p[1], p[2]), dist(p[2], p[0])];
    let mut sorted = sides;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (a, b, c) = (sorted[0], sorted[1], sorted[2]);
    // The right angle sits opposite the longest side. GridCraft's preset has it at the box's
    // bottom-left; flips move it to the corner the stroke drew it at. A right angle that is not
    // at a corner has no such preset orientation, so the triangle is tried against the `Triangle`
    // preset below instead.
    if (a * a + b * b - c * c).abs() < 0.08 * c * c
        && let Some((flip_h, flip_v)) = right_triangle_flips(p)
    {
        return Some((ShapeKind::RightTriangle, flip_h, flip_v));
    }
    // The `Triangle` preset is a centred apex over a base along one box edge. Only a triangle
    // that is one, within hand wobble, is inscribed in its box that way: anything else would lose
    // its apex.
    let x0 = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min);
    let x1 = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max);
    let y0 = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
    let y1 = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
    let tol = 0.15 * (x1 - x0).min(y1 - y0).max(1.0);
    let near = |v: f32, at: f32| (v - at).abs() <= tol;
    // One corner on the top edge is an apex up; two put the apex on the bottom edge, which the
    // preset reaches by mirroring the box down.
    let top: Vec<usize> = (0..3).filter(|&i| near(p[i][1], y0)).collect();
    let (apex, flip_v) = match top[..] {
        [i] => (i, false),
        [_, _] => ((0..3).find(|i| !top.contains(i))?, true),
        _ => return None,
    };
    let base: Vec<usize> = (0..3).filter(|&i| i != apex).collect();
    let (mut left, mut right) = (p[base[0]], p[base[1]]);
    if left[0] > right[0] {
        (left, right) = (right, left);
    }
    let base_y = if flip_v { y0 } else { y1 };
    let centred = near(p[apex][0], (x0 + x1) / 2.0) && near(p[apex][1], if flip_v { y1 } else { y0 });
    let seated = near(left[0], x0) && near(right[0], x1) && near(left[1], base_y) && near(right[1], base_y);
    (centred && seated).then_some((ShapeKind::Triangle, false, flip_v))
}

/// The flips that move GridCraft's `RightTriangle` preset (right angle at the box's bottom-left)
/// onto the recognised right angle: across for a corner on the right half, down for one at the
/// top. `None` when the right angle is not at a bounds corner (a rotated right triangle), whose
/// orientation the axis-aligned preset cannot take: the stroke stays ink.
fn right_triangle_flips(p: &[[f32; 2]]) -> Option<(bool, bool)> {
    if p.len() != 3 {
        return None;
    }
    // Each side is opposite the corner with the same index; the right angle is opposite the
    // hypotenuse, the longest side.
    let sides = [dist(p[1], p[2]), dist(p[2], p[0]), dist(p[0], p[1])];
    let right = (1..3).fold(0, |m, i| if sides[i] > sides[m] { i } else { m });
    let v = p.get(right)?;
    let x0 = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min);
    let x1 = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max);
    let y0 = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
    let y1 = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
    // Preset corners only: a leg along each box edge. Hand wobble is tolerated up to a share of
    // the shorter side (a long thin box must not accept a corner halfway along its short side);
    // a rotated leg (the right angle at an edge or inside) is not.
    let corner = [[x0, y0], [x1, y0], [x0, y1], [x1, y1]].into_iter().map(|c| dist(*v, c)).fold(f32::INFINITY, f32::min);
    if corner > 0.15 * (x1 - x0).min(y1 - y0).max(1.0) {
        return None;
    }
    Some((v[0] > (x0 + x1) / 2.0, v[1] < (y0 + y1) / 2.0))
}

/// The first of `kinds` whose preset outline `p`'s corners are, with the flips that put them
/// there. `None` when none fits: the axis-aligned preset would move the corners.
fn preset_flips(p: &[[f32; 2]], kinds: &[ShapeKind]) -> Option<(ShapeKind, bool, bool)> {
    kinds.iter().find_map(|&kind| {
        let unit = unit_outline(kind)?;
        let (x0, y0, x1, y1) = bounds_of(p);
        let (w, h) = (x1 - x0, y1 - y0);
        if p.len() != unit.len() || w <= 0.0 || h <= 0.0 {
            return None;
        }
        // Hand wobble, as for the triangles: a share of the shorter side.
        let tol = 0.15 * w.min(h).max(1.0);
        [(false, false), (true, false), (false, true), (true, true)].into_iter().find_map(|(flip_h, flip_v)| {
            let want: Vec<[f32; 2]> = unit
                .iter()
                .map(|q| {
                    let u = if flip_h { 1.0 - q[0] } else { q[0] };
                    let v = if flip_v { 1.0 - q[1] } else { q[1] };
                    [x0 + u * w, y0 + v * h]
                })
                .collect();
            corners_match(p, &want, tol).then_some((kind, flip_h, flip_v))
        })
    })
}

/// Whether `p`'s corners are `want`'s, in the same order around the loop (either way round), each
/// within `tol`. Order matters: a stroke that visits a pentagon's corners in star order is a
/// pentagram, and filling it as the preset would draw edges it never had.
fn corners_match(p: &[[f32; 2]], want: &[[f32; 2]], tol: f32) -> bool {
    let n = want.len();
    if p.len() != n {
        return false;
    }
    (0..n).any(|offset| {
        [1isize, -1].iter().any(|&dir| {
            (0..n).all(|i| {
                let j = (offset as isize + dir * i as isize).rem_euclid(n as isize) as usize;
                dist(p[i], want[j]) <= tol
            })
        })
    })
}

/// A preset polygon's outline in the unit box, as fractions of its width and height. The same
/// geometry the renderers and the PDF writer draw (`chartview::preset_geometry`,
/// `print::objects::preset_outline`).
fn unit_outline(kind: ShapeKind) -> Option<&'static [[f32; 2]]> {
    Some(match kind {
        ShapeKind::Diamond => &[[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]],
        ShapeKind::Parallelogram => &[[0.25, 0.0], [1.0, 0.0], [0.75, 1.0], [0.0, 1.0]],
        ShapeKind::Trapezoid => &[[0.2, 0.0], [0.8, 0.0], [1.0, 1.0], [0.0, 1.0]],
        ShapeKind::Pentagon => &[[0.5, 0.0], [1.0, 0.381_966], [0.809_017, 1.0], [0.190_983, 1.0], [0.0, 0.381_966]],
        ShapeKind::Hexagon => &[[0.25, 0.0], [0.75, 0.0], [1.0, 0.5], [0.75, 1.0], [0.25, 1.0], [0.0, 0.5]],
        _ => return None,
    })
}

/// The corners' box as `(x0, y0, x1, y1)`.
fn bounds_of(p: &[[f32; 2]]) -> (f32, f32, f32, f32) {
    p.iter().fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |(x0, y0, x1, y1), q| {
        (x0.min(q[0]), y0.min(q[1]), x1.max(q[0]), y1.max(q[1]))
    })
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
