//! Draw tab: pen and eraser tools, ink strokes, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::ink_shape::{InkShape, recognise};
use super::*;

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
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let id = d.wb.next_object_id();
    let shape = Shape {
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
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })?;
    // Draw ▸ Ink to Shape is on: convert each completed stroke, as Excel does while the mode is on.
    if s.ink_to_shape {
        let kind = convert_ink(s, Some(id))?;
        return Ok(json!({"shape": id, "kind": format!("{kind:?}")}));
    }
    Ok(json!({"shape": id}))
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
    if let Some(id) = u32_param(p, "id") {
        let kind = convert_ink(s, Some(id))?;
        return Ok(json!({"kind": format!("{kind:?}"), "on": s.ink_to_shape}));
    }
    if let Some(on) = bool_param(p, "on") {
        s.ink_to_shape = on;
        return Ok(json!({"on": on}));
    }
    // No parameters: convert the last stroke (the command's programmatic form).
    let kind = convert_ink(s, None)?;
    Ok(json!({"kind": format!("{kind:?}"), "on": s.ink_to_shape}))
}

/// Convert one ink stroke (by id, or the last one) to the nearest shape; returns the kind.
fn convert_ink(s: &mut Session, id: Option<u32>) -> Result<ShapeKind> {
    let si = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let idx = match id {
            Some(id) => sh.shapes.iter().position(|x| x.id == id && x.kind == ShapeKind::Ink),
            None => sh.shapes.iter().rposition(|x| x.kind == ShapeKind::Ink),
        }
        .ok_or_else(|| bad("draw.inkToShape", "no ink stroke to convert"))?;
        let ink = sh.shapes.get_mut(idx).ok_or_else(|| bad("draw.inkToShape", "gone"))?;
        let pts = parse_points(&ink.text);
        let Some(found) = recognise(&pts) else {
            return Err(bad("draw.inkToShape", "the stroke is not a shape"));
        };
        let [_, _, bw, bh] = found.bounds();
        let kind = match &found {
            InkShape::Line { .. } => ShapeKind::Line,
            InkShape::Arrow { head_at_start, .. } => {
                if *head_at_start {
                    ShapeKind::ArrowDouble
                } else {
                    ShapeKind::Arrow
                }
            }
            InkShape::Rectangle { .. } => ShapeKind::Rectangle,
            InkShape::Ellipse { .. } => ShapeKind::Ellipse,
            InkShape::Polygon(c) => polygon_kind(c).ok_or_else(|| bad("draw.inkToShape", "the polygon has no GridCraft preset"))?,
        };
        // Excel's square and circle are the same presets with equal sides: take the recogniser's
        // snapped bounds so the converted shape reads as a square/circle.
        if matches!(kind, ShapeKind::Rectangle | ShapeKind::Ellipse) {
            ink.anchor.width = bw.max(1.0);
            ink.anchor.height = bh.max(1.0);
        }
        let color = ink.line;
        ink.kind = kind;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        Ok(kind)
    })
}

/// Map a recognised polygon onto GridCraft's preset shapes: triangles and quads use the same
/// corner tests as before (right triangle; diamond/parallelogram/trapezoid), 5 and 6 sides are
/// the pentagon/hexagon presets. Seven or eight corners have no preset.
fn polygon_kind(c: &[[f32; 2]]) -> Option<ShapeKind> {
    match c.len() {
        3 => Some(triangle_kind(c)),
        4 => Some(quad_kind(c)),
        5 => Some(ShapeKind::Pentagon),
        6 => Some(ShapeKind::Hexagon),
        _ => None,
    }
}

fn triangle_kind(p: &[[f32; 2]]) -> ShapeKind {
    if p.len() != 3 {
        return ShapeKind::Triangle;
    }
    let sides = [dist(p[0], p[1]), dist(p[1], p[2]), dist(p[2], p[0])];
    let mut sorted = sides;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (a, b, c) = (sorted[0], sorted[1], sorted[2]);
    // The right angle sits opposite the longest side.
    if (a * a + b * b - c * c).abs() < 0.08 * c * c { ShapeKind::RightTriangle } else { ShapeKind::Triangle }
}

fn quad_kind(p: &[[f32; 2]]) -> ShapeKind {
    if p.len() != 4 {
        return ShapeKind::Rectangle;
    }
    let edge = |i: usize| sub(p[(i + 1) % 4], p[i]);
    let e = [edge(0), edge(1), edge(2), edge(3)];
    let parallel = |a: [f32; 2], b: [f32; 2]| {
        let (na, nb) = (norm(a), norm(b));
        na > 0.0 && nb > 0.0 && (a[0] * b[0] + a[1] * b[1]).abs() / (na * nb) > 0.94
    };
    let square = |a: [f32; 2], b: [f32; 2]| {
        let (na, nb) = (norm(a), norm(b));
        na > 0.0 && nb > 0.0 && (a[0] * b[0] + a[1] * b[1]).abs() / (na * nb) < 0.25
    };
    if square(e[0], e[1]) && square(e[1], e[2]) && square(e[2], e[3]) && square(e[3], e[0]) {
        return ShapeKind::Rectangle;
    }
    let (p02, p13) = (parallel(e[0], e[2]), parallel(e[1], e[3]));
    if p02 && p13 {
        let sides = [norm(e[0]), norm(e[1]), norm(e[2]), norm(e[3])];
        let mean = sides.iter().sum::<f32>() / 4.0;
        return if mean > 0.0 && sides.iter().all(|s| (s - mean).abs() < 0.08 * mean) { ShapeKind::Diamond } else { ShapeKind::Parallelogram };
    }
    if p02 || p13 { ShapeKind::Trapezoid } else { ShapeKind::Rectangle }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn norm(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
