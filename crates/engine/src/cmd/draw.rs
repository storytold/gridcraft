//! Draw tab: pen and eraser tools, ink strokes, the lasso and Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Color, Shape, ShapeKind};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, str_param, u32_param};

/// Most points read from a lasso loop's parameters (a longer loop is cut off there).
const MAX_LASSO_INPUT: usize = 20_000;
/// Most points of a lasso loop that are kept (a longer loop is thinned out).
const MAX_LASSO_POINTS: usize = 512;
/// Largest sheet coordinate (points) a lasso loop point or a selection move may name.
const MAX_LASSO_COORD: f64 = 1e7;
/// Points of a stroke tested against the lasso (spread along it).
const LASSO_SAMPLES: usize = 64;
/// Farthest the lasso moves its selection in one step, sheet points.
const MAX_MOVE: f64 = 100_000.0;

/// The ink picked with the lasso on `sheet`: shape ids (view state, never saved or undone) and
/// the loop that picked them (sheet points).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LassoSelection {
    pub sheet: usize,
    pub ids: Vec<u32>,
    pub polygon: Vec<[f32; 2]>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.pen", "Pen", ["Draw", "Tools"], None, "{on?: bool, color?: hex, width?: 2}", has_doc, pen),
        cmd!(noundo "draw.eraser", "Eraser", ["Draw", "Tools"], None, "{on?: bool}", has_doc, eraser),
        cmd!(
            noundo "draw.lasso",
            "Lasso Select",
            ["Draw", "Tools"],
            None,
            "{on?: bool} (picks up the lasso; draw a loop to select ink, drag a selected stroke to move it)",
            has_doc,
            lasso
        ),
        cmd!(
            noundo "draw.lassoSelect",
            "Select Ink with the Lasso",
            [],
            None,
            "{points: [[x, y], …] (sheet points; at least three) , clear?: bool}",
            has_doc,
            lasso_select
        ),
        cmd!(
            query "draw.lassoSelection",
            "Lasso Selection",
            [],
            None,
            "{} → {sheet, count, selected: [shape ids], polygon}",
            has_doc,
            lasso_selection
        ),
        cmd!(
            "draw.moveSelection",
            "Move Selected Ink",
            ["Draw", "Tools"],
            None,
            "{dx: pt, dy: pt} (moves the ink selected with the lasso; one undo step)",
            has_doc,
            move_selection
        ),
        cmd!(
            "draw.recolorSelection",
            "Recolour Selected Ink",
            ["Draw", "Tools"],
            None,
            "{color: hex} (recolours the ink selected with the lasso; one undo step)",
            has_doc,
            recolor_selection
        ),
        cmd!(
            "draw.deleteSelection",
            "Delete Selected Ink",
            [],
            None,
            "{} (deletes the ink selected with the lasso; one undo step)",
            has_doc,
            delete_selection
        ),
        cmd!("draw.stroke", "Ink Stroke", [], None, "{points: [[x,y],…] in sheet points, color?: hex, width?: 2}", has_doc, stroke),
        cmd!(
            "draw.inkToShape",
            "Ink to Shape",
            ["Draw", "Convert"],
            None,
            "{id?} converts an ink stroke (default: the last) into a rectangle, ellipse, triangle or line",
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

/// Whether a shape is freehand ink the lasso can pick: ink strokes only, never another object
/// (a picture, chart, or a shape converted from ink that is now a rectangle, and so on).
pub fn is_ink(sh: &Shape) -> bool {
    sh.kind == ShapeKind::Ink
}

// --- Lasso -------------------------------------------------------------------------------

fn lasso(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "lasso");
    s.draw_tool = if on { "lasso".into() } else { String::new() };
    let selected = lasso_ids(s);
    Ok(json!({"tool": s.draw_tool, "count": selected.len(), "selected": selected}))
}

/// The ids picked with the lasso (view state; empty when nothing is selected).
fn lasso_ids(s: &Session) -> Vec<u32> {
    s.lasso.as_ref().map(|l| l.ids.clone()).unwrap_or_default()
}

/// A lasso loop's points, from `points` in sheet points. Non-finite points are dropped (never
/// tested, so they cannot select anything), coordinates are kept within [`MAX_LASSO_COORD`], and
/// a loop longer than [`MAX_LASSO_POINTS`] is thinned evenly. Fewer than three points is an
/// error: there is no loop to test against.
fn lasso_polygon(p: &Json) -> Result<Vec<[f64; 2]>> {
    let raw = p.get("points").and_then(Json::as_array).ok_or_else(|| bad("draw.lassoSelect", "`points` is a list of [x, y] points"))?;
    let mut pts: Vec<[f64; 2]> = raw
        .iter()
        .take(MAX_LASSO_INPUT)
        .filter_map(|q| Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]))
        .filter(|q| q[0].is_finite() && q[1].is_finite())
        .map(|q| [q[0].clamp(-MAX_LASSO_COORD, MAX_LASSO_COORD), q[1].clamp(-MAX_LASSO_COORD, MAX_LASSO_COORD)])
        .collect();
    if pts.len() > MAX_LASSO_POINTS {
        let step = pts.len().div_ceil(MAX_LASSO_POINTS);
        pts = pts.into_iter().step_by(step).collect();
    }
    if pts.len() < 3 {
        return Err(bad("draw.lassoSelect", "a lasso loop needs at least three points"));
    }
    Ok(pts)
}

/// Whether (`x`, `y`) is inside the closed loop `poly` (even–odd rule).
fn inside(poly: &[[f64; 2]], x: f64, y: f64) -> bool {
    let n = poly.len();
    let mut odd = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + n - 1) % n)) else { continue };
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            odd = !odd;
        }
    }
    odd
}

/// Whether the stroke `sh` lies inside the loop: as in WordCraft, most of its geometry must be
/// inside it, sampled up to [`LASSO_SAMPLES`] points spread along the stroke, and a stroke is
/// selected when at least 60% of those samples are inside. A stroke that merely crosses the loop
/// (half in, half out) is not selected. `parse_points` gives the stroke's points relative to its
/// anchor, which is where the stroke is drawn on the sheet.
fn stroke_inside(sh: &Shape, sheet: &gridcraft_model::Sheet, poly: &[[f64; 2]]) -> bool {
    let pts = parse_points(&sh.text);
    if pts.is_empty() {
        return false;
    }
    let ox = sheet.col_left(sh.anchor.cell.col) + f64::from(sh.anchor.dx);
    let oy = sheet.row_top(sh.anchor.cell.row) + f64::from(sh.anchor.dy);
    let step = pts.len().div_ceil(LASSO_SAMPLES).max(1);
    let (n, hits) = pts.iter().step_by(step).fold((0usize, 0usize), |(n, hits), q| {
        let (x, y) = (ox + f64::from(q[0]), oy + f64::from(q[1]));
        (n + 1, hits + usize::from(inside(poly, x, y)))
    });
    n > 0 && hits * 10 >= n * 6
}

/// Select the ink inside a loop drawn on the active sheet; `clear` drops the selection.
fn lasso_select(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "clear") == Some(true) {
        s.lasso = None;
        return Ok(json!({"selected": [], "count": 0}));
    }
    let poly = lasso_polygon(p)?;
    let si = s.doc()?.wb.active_sheet();
    let ids = {
        let d = s.doc()?;
        let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
        sh.shapes.iter().filter(|x| is_ink(x) && stroke_inside(x, sh, &poly)).map(|x| x.id).collect::<Vec<_>>()
    };
    let polygon: Vec<[f32; 2]> = poly.iter().map(|q| [q[0] as f32, q[1] as f32]).collect();
    s.lasso = (!ids.is_empty()).then(|| LassoSelection { sheet: si, ids: ids.clone(), polygon });
    Ok(json!({"sheet": si, "count": ids.len(), "selected": ids}))
}

/// The lasso selection on the active sheet, for the UI to paint and for scripts to read.
fn lasso_selection(s: &mut Session, _: &Json) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet();
    let sel = s.lasso.as_ref().filter(|l| l.sheet == si);
    let (ids, polygon) = match sel {
        Some(l) => (l.ids.clone(), json!(l.polygon)),
        None => (Vec::new(), json!([])),
    };
    Ok(json!({"sheet": si, "count": ids.len(), "selected": ids, "polygon": polygon}))
}

/// The ids picked with the lasso, or an error when nothing is selected.
fn selection_required(s: &Session, cmd: &str) -> Result<LassoSelection> {
    s.lasso.clone().ok_or_else(|| bad(cmd, "nothing is selected with the lasso"))
}

/// Move the lasso-selected ink by (`dx`, `dy`) sheet points (one undo step). The anchor's offset
/// moves, so the stroke's points (relative to the anchor) and its position stay consistent.
fn move_selection(s: &mut Session, p: &Json) -> Result<Json> {
    let sel = selection_required(s, "draw.moveSelection")?;
    let d = |k: &str| {
        f64_param(p, k)
            .ok_or_else(|| bad("draw.moveSelection", format!("`{k}` (points) is required")))
            .map(|v| if v.is_finite() { v.clamp(-MAX_MOVE, MAX_MOVE) } else { 0.0 })
    };
    let (dx, dy) = (d("dx")?, d("dy")?);
    let si = sel.sheet;
    let ids = sel.ids;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let mut moved = 0;
        for id in &ids {
            if let Some(sp) = sh.shapes.iter_mut().find(|x| x.id == *id && is_ink(x)) {
                sp.anchor.dx = (f64::from(sp.anchor.dx) + dx).clamp(-MAX_LASSO_COORD, MAX_LASSO_COORD) as f32;
                sp.anchor.dy = (f64::from(sp.anchor.dy) + dy).clamp(-MAX_LASSO_COORD, MAX_LASSO_COORD) as f32;
                moved += 1;
            }
        }
        if moved == 0 {
            return Err(bad("draw.moveSelection", "nothing selected can be moved"));
        }
        Ok(json!({"moved": moved, "dx": dx, "dy": dy}))
    })
}

/// Give the lasso-selected ink this colour (one undo step).
fn recolor_selection(s: &mut Session, p: &Json) -> Result<Json> {
    let sel = selection_required(s, "draw.recolorSelection")?;
    let hex = str_param(p, "color").ok_or_else(|| bad("draw.recolorSelection", "`color` is RRGGBB"))?.to_string();
    let color = Color::from_hex(&hex).ok_or_else(|| bad("draw.recolorSelection", "`color` is RRGGBB"))?;
    let si = sel.sheet;
    let ids = sel.ids;
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let mut changed = 0;
        for id in &ids {
            if let Some(sp) = sh.shapes.iter_mut().find(|x| x.id == *id && is_ink(x)) {
                sp.line = color;
                changed += 1;
            }
        }
        if changed == 0 {
            return Err(bad("draw.recolorSelection", "nothing selected can be recoloured"));
        }
        Ok(json!({"recolored": changed, "color": hex}))
    })
}

/// Delete the lasso-selected ink (one undo step), then drop the selection.
fn delete_selection(s: &mut Session, _: &Json) -> Result<Json> {
    let sel = selection_required(s, "draw.deleteSelection")?;
    let si = sel.sheet;
    let ids = sel.ids;
    let r = edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let before = sh.shapes.len();
        sh.shapes.retain(|x| !(is_ink(x) && ids.contains(&x.id)));
        let deleted = before - sh.shapes.len();
        if deleted == 0 {
            return Err(bad("draw.deleteSelection", "nothing selected can be deleted"));
        }
        Ok(json!({"deleted": deleted}))
    })?;
    s.lasso = None;
    Ok(r)
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
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
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
    let si = s.doc()?.wb.active_sheet();
    let id = u32_param(p, "id");
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
        let closed = match (pts.first(), pts.last()) {
            (Some(a), Some(b)) => ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() < 0.25 * w.max(h),
            _ => false,
        };
        let kind = if !closed {
            ShapeKind::Line
        } else {
            // Corners hugging the bounding box mean a rectangle; one apex at the top a triangle.
            let near = |cx: f32, cy: f32| pts.iter().any(|q| ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt() < 0.15 * w.min(h).max(8.0));
            let corners = [near(0.0, 0.0), near(w, 0.0), near(0.0, h), near(w, h)].iter().filter(|b| **b).count();
            let apex = near(w / 2.0, 0.0);
            if corners >= 3 {
                ShapeKind::Rectangle
            } else if apex && near(0.0, h) && near(w, h) {
                ShapeKind::Triangle
            } else {
                ShapeKind::Ellipse
            }
        };
        let color = ink.line;
        ink.kind = kind;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        if kind == ShapeKind::Line && pts.first().zip(pts.last()).is_some_and(|(a, b)| a[1] > b[1]) {
            // Lines are drawn top-left to bottom-right; keep the stroke direction simple.
        }
        Ok(json!({"kind": format!("{kind:?}")}))
    })
}
