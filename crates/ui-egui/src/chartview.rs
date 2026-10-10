//! Floating objects on the sheet: charts (rendered by `gridcraft-chart` into primitives that
//! we paint with egui), pictures and shapes.

use std::collections::HashMap;
use std::sync::Mutex;

use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_chart::{HAlign, Prim, VAlign};
use gridcraft_engine::model::{Anchor, ShapeKind, Sheet, Workbook};

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme;

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

struct EguiMeasure<'a> {
    painter: &'a Painter,
    z: f32,
}

impl gridcraft_chart::Measure for EguiMeasure<'_> {
    fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let font = if bold { theme::ui_bold(size * self.z) } else { theme::ui_font(size * self.z) };
        self.painter.layout_no_wrap(text.to_string(), font, Color32::BLACK).size().x / self.z.max(0.01)
    }
}

/// Paints chart primitives at `origin`, scaled by `z`.
pub fn paint_prims(p: &Painter, origin: Pos2, prims: &[Prim], z: f32) {
    let pt = |x: f32, y: f32| pos2(origin.x + x * z, origin.y + y * z);
    for prim in prims {
        match prim {
            Prim::Rect { x, y, w, h, fill, stroke, radius } => {
                let r = Rect::from_min_size(pt(*x, *y), vec2(w * z, h * z));
                if let Some(f) = fill {
                    p.rect_filled(r, radius * z, rgba(*f));
                }
                if let Some((c, wd)) = stroke {
                    p.rect_stroke(r, radius * z, Stroke::new(wd * z.max(0.5), rgba(*c)), StrokeKind::Inside);
                }
            }
            Prim::Line { pts, color, width, dash } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 2 {
                    continue;
                }
                if *dash {
                    p.extend(egui::Shape::dashed_line(&v, Stroke::new(width * z.max(0.5), rgba(*color)), 4.0 * z, 3.0 * z));
                } else {
                    p.add(PathShape::line(v, PathStroke::new(width * z.max(0.5), rgba(*color))));
                }
            }
            Prim::Polygon { pts, fill, stroke } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 3 {
                    continue;
                }
                // Polygons may be concave (areas): triangulate as a fan from the first point
                // when convex, else draw through a mesh of the closed path.
                let shape = PathShape { points: v.clone(), closed: true, fill: rgba(*fill), stroke: PathStroke::NONE };
                if is_convex(&v) {
                    p.add(shape);
                } else {
                    p.add(egui::Shape::mesh(area_mesh(&v, rgba(*fill))));
                }
                if let Some((c, w)) = stroke {
                    p.add(PathShape::closed_line(v, PathStroke::new(w * z.max(0.5), rgba(*c))));
                }
            }
            Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, fill, stroke } => {
                let c = pt(*cx, *cy);
                let steps = (((a1 - a0).abs() / 0.05).ceil() as usize).clamp(2, 400);
                let at = |a: f32, r: f32| pos2(c.x + r * z * a.sin(), c.y - r * z * a.cos());
                let mut mesh = egui::Mesh::default();
                let col = rgba(*fill);
                for i in 0..=steps {
                    let a = a0 + (a1 - a0) * i as f32 / steps as f32;
                    mesh.colored_vertex(at(a, *r_outer), col);
                    mesh.colored_vertex(if *r_inner > 0.0 { at(a, *r_inner) } else { c }, col);
                }
                for i in 0..steps as u32 {
                    let b = i * 2;
                    mesh.add_triangle(b, b + 1, b + 2);
                    mesh.add_triangle(b + 1, b + 3, b + 2);
                }
                p.add(egui::Shape::mesh(mesh));
                if let Some((sc, w)) = stroke {
                    let mut outline: Vec<Pos2> = (0..=steps).map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_outer)).collect();
                    if *r_inner > 0.0 {
                        outline.extend((0..=steps).rev().map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_inner)));
                    } else {
                        outline.push(c);
                    }
                    p.add(PathShape::closed_line(outline, PathStroke::new(w * z.max(0.5), rgba(*sc))));
                }
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                if let Some(f) = fill {
                    p.circle_filled(pt(*cx, *cy), r * z, rgba(*f));
                }
                if let Some((c, w)) = stroke {
                    p.circle_stroke(pt(*cx, *cy), r * z, Stroke::new(w * z.max(0.5), rgba(*c)));
                }
            }
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation } => {
                let font: FontId = if *bold { theme::ui_bold(size * z) } else { theme::ui_font(size * z) };
                let galley = p.layout_no_wrap(text.clone(), font, rgba(*color));
                let sz = galley.size();
                let ax = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => 0.5,
                    HAlign::Right => 1.0,
                };
                let ay = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => 0.5,
                    VAlign::Bottom => 1.0,
                };
                let anchor = pt(*x, *y);
                if *rotation == 0.0 {
                    p.galley(pos2(anchor.x - sz.x * ax, anchor.y - sz.y * ay), galley, rgba(*color));
                } else {
                    // Rotate about the anchor: offset the galley origin by the rotated alignment.
                    let (s, c) = rotation.sin_cos();
                    let off = vec2(-sz.x * ax, -sz.y * ay);
                    let rot = vec2(off.x * c - off.y * s, off.x * s + off.y * c);
                    let mut shape = egui::epaint::TextShape::new(anchor + rot, galley, rgba(*color));
                    shape.angle = *rotation;
                    p.add(shape);
                }
            }
        }
    }
}

fn is_convex(v: &[Pos2]) -> bool {
    let n = v.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = v[i];
        let b = v[(i + 1) % n];
        let c = v[(i + 2) % n];
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() > 1e-3 {
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
    }
    true
}

/// Area charts produce "mountain" polygons whose bottom edge is a straight baseline: fill them
/// as vertical strips (exact for such shapes, acceptable for others).
fn area_mesh(v: &[Pos2], col: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let n = v.len();
    if n < 3 {
        return mesh;
    }
    let half = n / 2;
    for i in 0..half.saturating_sub(1) {
        let a = v[i];
        let b = v[i + 1];
        let c = v[n - 2 - i];
        let d = v[n - 1 - i];
        let base = mesh.vertices.len() as u32;
        for q in [a, b, c, d] {
            mesh.colored_vertex(q, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

pub(crate) fn anchor_rect(geo: &Geo, sh: &Sheet, a: &Anchor) -> Rect {
    let x = geo.x(sh, a.cell.col) + a.dx * geo.z;
    let y = geo.y(sh, a.cell.row) + a.dy * geo.z;
    Rect::from_min_size(pos2(x, y), vec2(a.width * geo.z, a.height.max(1.0) * geo.z))
}

static TEXTURES: std::sync::LazyLock<Mutex<HashMap<(u32, usize), egui::TextureHandle>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn paint_objects(app: &SheetApp, p: &Painter, geo: &Geo, wb: &Workbook, si: usize, sh: &Sheet) {
    for sp in &sh.shapes {
        let r = anchor_rect(geo, sh, &sp.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let fill = sp.fill.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::TRANSPARENT);
        let line = sp.line.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::DARK_GRAY);
        match sp.kind {
            ShapeKind::Rectangle | ShapeKind::TextBox => {
                p.rect_filled(r, 0.0, fill);
                p.rect_stroke(r, 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
            }
            ShapeKind::RoundedRectangle => {
                p.rect_filled(r, 10.0 * geo.z, fill);
                p.rect_stroke(r, 10.0 * geo.z, Stroke::new(1.0, line), StrokeKind::Inside);
            }
            ShapeKind::Ellipse => {
                let pts: Vec<Pos2> = (0..64)
                    .map(|i| {
                        let a = i as f32 / 64.0 * std::f32::consts::TAU;
                        pos2(r.center().x + r.width() / 2.0 * a.cos(), r.center().y + r.height() / 2.0 * a.sin())
                    })
                    .collect();
                p.add(PathShape::convex_polygon(pts, fill, Stroke::new(1.0, line)));
            }
            ShapeKind::Triangle => {
                p.add(PathShape::convex_polygon(vec![pos2(r.center().x, r.top()), r.right_bottom(), r.left_bottom()], fill, Stroke::new(1.0, line)));
            }
            ShapeKind::Ink => {
                let w = gridcraft_engine::cmd::draw::ink_width(sp) * geo.z;
                let pts: Vec<Pos2> =
                    gridcraft_engine::cmd::draw::ink_points(sp).iter().map(|q| pos2(r.left() + q[0] * geo.z, r.top() + q[1] * geo.z)).collect();
                if pts.len() >= 2 {
                    let col = if w > 8.0 { line.gamma_multiply(0.45) } else { line };
                    p.add(PathShape::line(pts, PathStroke::new(w, col)));
                }
                continue;
            }
            ShapeKind::Icon => {
                if let Some(icon) = crate::icons::from_name(&sp.text) {
                    crate::icons::paint(p, r, icon, if fill == Color32::TRANSPARENT { Color32::BLACK } else { fill });
                }
                continue;
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                p.line_segment([r.left_top(), r.right_bottom()], Stroke::new(2.0, line));
                if sp.kind == ShapeKind::Arrow {
                    let d = (r.right_bottom() - r.left_top()).normalized();
                    let n = vec2(-d.y, d.x);
                    let tip = r.right_bottom();
                    p.add(PathShape::convex_polygon(vec![tip, tip - d * 10.0 + n * 5.0, tip - d * 10.0 - n * 5.0], line, Stroke::NONE));
                }
            }
        }
        if !sp.text.is_empty() && !app.editing_text_box(sp.id) {
            let job = egui::text::LayoutJob::simple(
                sp.text.clone(),
                FontId::new(14.0 * geo.z, egui::FontFamily::Name(theme::CELL.into())),
                if sp.kind == ShapeKind::TextBox { Color32::BLACK } else { Color32::WHITE },
                r.width() - 8.0,
            );
            let g = p.layout_job(job);
            let pos = if sp.kind == ShapeKind::TextBox { r.min + vec2(4.0, 4.0) } else { r.center() - g.size() / 2.0 };
            p.galley(pos, g, Color32::BLACK);
        }
        if sp.kind == ShapeKind::TextBox && app.selected_chart == Some(sp.id) {
            selection_frame(p, r);
        }
    }
    for im in &sh.images {
        let r = anchor_rect(geo, sh, &im.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let key = (im.id, im.data.len());
        let mut cache = TEXTURES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !cache.contains_key(&key)
            && let Ok(img) = image::load_from_memory(&im.data)
        {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            let tex = p.ctx().load_texture(format!("img{}", im.id), ci, egui::TextureOptions::LINEAR);
            cache.insert(key, tex);
        }
        if let Some(tex) = cache.get(&key) {
            p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.rect_filled(r, 0.0, Color32::from_gray(230));
        }
        if app.selected_chart == Some(im.id) {
            selection_frame(p, r);
        }
    }
    for ch in &sh.charts {
        let r = anchor_rect(geo, sh, &ch.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        // Soft shadow + white card.
        p.rect_filled(r.translate(vec2(0.0, 1.5)), 2.0, Color32::from_black_alpha(18));
        p.rect_filled(r, 0.0, Color32::WHITE);
        let data = gridcraft_chart::resolve(wb, si, ch);
        let m = EguiMeasure { painter: p, z: geo.z };
        let prims = gridcraft_chart::render(ch, &data, ch.anchor.width, ch.anchor.height, &m);
        paint_prims(&p.with_clip_rect(r.intersect(p.clip_rect())), r.min, &prims, geo.z);
        if app.selected_chart == Some(ch.id) {
            selection_frame(p, r);
            // Highlight the source data like Excel does.
            for s in &ch.series {
                if let Ok(gridcraft_engine::formula::Expr::Ref(rf)) = gridcraft_engine::formula::parse(&s.values)
                    && rf.sheet_name().is_none_or(|n| n.eq_ignore_ascii_case(&sh.name))
                {
                    let rr = geo.range_rect(sh, rf.range());
                    for pane in geo.range_panes(rf.range()) {
                        let pp = p.with_clip_rect(pane.intersect(p.clip_rect()));
                        pp.rect_stroke(rr, 0.0, Stroke::new(1.5, Color32::from_rgb(0x2E, 0x6F, 0xD8)), StrokeKind::Inside);
                    }
                }
            }
        }
    }
}

fn selection_frame(p: &Painter, r: Rect) {
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(120)), StrokeKind::Outside);
    for c in [r.left_top(), r.center_top(), r.right_top(), r.left_center(), r.right_center(), r.left_bottom(), r.center_bottom(), r.right_bottom()] {
        p.circle_filled(c, 4.0, Color32::WHITE);
        p.circle_stroke(c, 4.0, Stroke::new(1.0, Color32::from_gray(100)));
    }
    let _ = Align2::CENTER_CENTER;
}

/// The topmost object under `p`: (kind, id, rect).
pub fn hit(app: &SheetApp, geo: &Geo, sh: &Sheet, p: Pos2) -> Option<(&'static str, u32, Rect)> {
    let _ = app;
    for ch in sh.charts.iter().rev() {
        let r = anchor_rect(geo, sh, &ch.anchor);
        if r.expand(4.0).contains(p) && geo.cells.contains(p) {
            return Some(("chart", ch.id, r));
        }
    }
    for im in sh.images.iter().rev() {
        let r = anchor_rect(geo, sh, &im.anchor);
        if r.contains(p) && geo.cells.contains(p) {
            return Some(("image", im.id, r));
        }
    }
    for s in sh.shapes.iter().rev() {
        let r = anchor_rect(geo, sh, &s.anchor);
        if r.expand(3.0).contains(p) && geo.cells.contains(p) {
            return Some(("shape", s.id, r));
        }
    }
    None
}
