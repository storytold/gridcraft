//! Draw tab: pen and eraser tools, ink strokes, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::*;
use serde_json::{Value as Json, json};

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
        let (w, h) = (ink.anchor.width.max(1.0), ink.anchor.height.max(1.0));
        let kind = classify(&pts, w, h);
        // Excel's square and circle are the same presets with equal sides: snap a near-square
        // stroke so the converted shape reads as a square/circle.
        if matches!(kind, ShapeKind::Rectangle | ShapeKind::Ellipse) && (w - h).abs() <= 0.12 * w.max(h) {
            let side = ((w + h) / 2.0).max(1.0);
            ink.anchor.width = side;
            ink.anchor.height = side;
        }
        let color = ink.line;
        ink.kind = kind;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        Ok(kind)
    })
}

/// The shape Excel's Ink to Shape would make of the stroke: closed strokes by their corner
/// structure, open ones as lines, arrows or double arrows when a barb doubles the stroke back.
fn classify(points: &[[f32; 2]], width: f32, height: f32) -> ShapeKind {
    if points.len() < 2 {
        return ShapeKind::Line;
    }
    let diag = (width * width + height * height).sqrt().max(1.0);
    let closed = match (points.first(), points.last()) {
        (Some(a), Some(b)) => dist(*a, *b) < (0.12 * diag).max(6.0),
        _ => false,
    };
    if !closed {
        return match (arrow_head(points, true), arrow_head(points, false)) {
            (true, true) => ShapeKind::ArrowDouble,
            (_, true) => ShapeKind::Arrow,
            _ => ShapeKind::Line,
        };
    }
    let hull = convex_hull(points);
    let poly = simplify_closed(&hull, (diag * 0.03).max(1.0));
    let area = polygon_area(&hull).abs();
    let bbox_fill = area / (width * height).max(1.0);
    let roundness = area / (std::f32::consts::PI * (width / 2.0) * (height / 2.0)).max(1.0);
    match poly.len() {
        3 => triangle_kind(&poly),
        4 => quad_kind(&poly),
        5 if bbox_fill < 0.9 => ShapeKind::Pentagon,
        6 if bbox_fill < 0.9 => ShapeKind::Hexagon,
        _ if roundness > 0.72 => ShapeKind::Ellipse,
        // A near-rectangle fills its bounding box; anything else with many corners is an ellipse.
        _ if bbox_fill > 0.9 => ShapeKind::Rectangle,
        _ => ShapeKind::Ellipse,
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

/// A barb: a sharp turn within the first or last quarter of the stroke, where the pen doubles
/// back instead of continuing straight.
fn arrow_head(points: &[[f32; 2]], at_start: bool) -> bool {
    let ordered: Vec<[f32; 2]> = if at_start { points.to_vec() } else { points.iter().rev().copied().collect() };
    let n = ordered.len();
    if n < 4 {
        return false;
    }
    let total: f32 = ordered.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total < 24.0 {
        return false;
    }
    let mut acc = 0.0;
    for i in 1..n - 1 {
        acc += dist(ordered[i - 1], ordered[i]);
        if acc > total * 0.25 {
            break;
        }
        let incoming = sub(ordered[i], ordered[i - 1]);
        let outgoing = sub(ordered[i + 1], ordered[i]);
        let (ni, no) = (norm(incoming), norm(outgoing));
        if ni <= 0.0 || no <= 0.0 {
            continue;
        }
        let turn = (incoming[0] * outgoing[0] + incoming[1] * outgoing[1]) / (ni * no);
        if turn < 0.17 {
            // More than ~80° between the two segments: the stroke doubles back (a barb).
            return true;
        }
    }
    false
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

/// Monotone-chain convex hull; the result is counter-clockwise without repeating the first point.
fn convex_hull(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| {
        a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal).then(a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<[f32; 2]> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 {
            let n = lower.len();
            if cross(lower[n - 2], lower[n - 1], p) > 0.0 {
                break;
            }
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f32; 2]> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 {
            let n = upper.len();
            if cross(upper[n - 2], upper[n - 1], p) > 0.0 {
                break;
            }
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Douglas–Peucker simplification of a closed polygon: rotate to the point farthest from the
/// centroid so the split does not start on an edge, then simplify the open ring.
fn simplify_closed(points: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    if points.len() < 4 {
        return points.to_vec();
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for p in points {
        cx += p[0];
        cy += p[1];
    }
    let n = points.len() as f32;
    let centroid = [cx / n, cy / n];
    let start = points
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| dist(**a, centroid).partial_cmp(&dist(**b, centroid)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let mut ring: Vec<[f32; 2]> = points[start..].iter().chain(points[..start].iter()).copied().collect();
    ring.push(points[start]);
    let mut out = simplify(&ring, eps);
    out.pop();
    if out.len() < 3 { points.to_vec() } else { out }
}

fn simplify(points: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[n - 1]);
    let mut max_d = 0.0;
    let mut idx = 0;
    for (i, p) in points.iter().enumerate().take(n - 1).skip(1) {
        let d = line_distance(*p, a, b);
        if d > max_d {
            max_d = d;
            idx = i;
        }
    }
    if max_d > eps {
        let mut left = simplify(&points[..=idx], eps);
        let right = simplify(&points[idx..], eps);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![a, b]
    }
}

fn line_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = sub(b, a);
    let len = norm(ab);
    if len <= 0.0 {
        return dist(p, a);
    }
    ((ab[0] * (p[1] - a[1]) - ab[1] * (p[0] - a[0])) / len).abs()
}

fn polygon_area(points: &[[f32; 2]]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(mut pts: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
        let first = pts[0];
        pts.push(first);
        pts
    }

    #[test]
    fn closed_strokes_become_their_polygons() {
        let cases: Vec<(Vec<[f32; 2]>, ShapeKind)> = vec![
            (ring(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]]), ShapeKind::Rectangle),
            (ring(vec![[50.0, 0.0], [100.0, 30.0], [50.0, 60.0], [0.0, 30.0]]), ShapeKind::Diamond),
            (ring(vec![[20.0, 0.0], [100.0, 0.0], [80.0, 60.0], [0.0, 60.0]]), ShapeKind::Parallelogram),
            (ring(vec![[20.0, 0.0], [80.0, 0.0], [100.0, 60.0], [0.0, 60.0]]), ShapeKind::Trapezoid),
            (ring(vec![[50.0, 0.0], [97.6, 34.5], [79.4, 90.5], [20.6, 90.5], [2.4, 34.5]]), ShapeKind::Pentagon),
            (ring(vec![[25.0, 0.0], [75.0, 0.0], [100.0, 30.0], [75.0, 60.0], [25.0, 60.0], [0.0, 30.0]]), ShapeKind::Hexagon),
            (ring(vec![[0.0, 0.0], [100.0, 0.0], [0.0, 60.0]]), ShapeKind::RightTriangle),
            (ring(vec![[50.0, 0.0], [100.0, 60.0], [0.0, 60.0]]), ShapeKind::Triangle),
        ];
        for (pts, want) in cases {
            let w = pts.iter().map(|p| p[0]).fold(0.0_f32, f32::max);
            let h = pts.iter().map(|p| p[1]).fold(0.0_f32, f32::max);
            let got = classify(&pts, w, h);
            assert_eq!(got, want, "points {pts:?}");
        }
    }

    #[test]
    fn circles_are_ellipses_and_open_strokes_are_lines_or_arrows() {
        let circle: Vec<[f32; 2]> = (0..=48)
            .map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                [40.0 + 40.0 * a.cos(), 40.0 + 40.0 * a.sin()]
            })
            .collect();
        assert_eq!(classify(&circle, 80.0, 80.0), ShapeKind::Ellipse);
        assert_eq!(classify(&[[0.0, 0.0], [50.0, 10.0], [120.0, 30.0]], 120.0, 30.0), ShapeKind::Line);
        let arrow = vec![[0.0, 0.0], [50.0, 0.0], [100.0, 0.0], [88.0, 9.0]];
        assert_eq!(classify(&arrow, 100.0, 9.0), ShapeKind::Arrow);
        let both = vec![[9.0, 9.0], [0.0, 0.0], [50.0, 0.0], [100.0, 0.0], [91.0, 9.0]];
        assert_eq!(classify(&both, 100.0, 9.0), ShapeKind::ArrowDouble);
    }
}
