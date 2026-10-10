//! Liquid dark glass: floating surfaces (menus, popups, dialogs, tooltips) frost whatever of the
//! app lies beneath them. Drawn by GridCraft itself, no compositor help: the background layer is
//! redrawn under each surface as a multi-tap blur, the surface's translucent tint goes over it, and
//! a specular rim catches the light along the top edge.
//! ponytail: a shape-replay blur (17 taps of the background layer), not a GPU blur pass; cost scales
//! with the shapes under open popups and only runs while one is open.

use egui::epaint::ClippedShape;
use egui::{Color32, LayerId, Mesh, Order, Rect, Shape, pos2, vec2};

use crate::theme::Tokens;

/// Blur reach in points.
const RADIUS: f32 = 10.0;

/// Tap offsets: centre plus two rings reaching `radius`, a cheap Gaussian approximation.
fn taps(radius: f32) -> Vec<egui::Vec2> {
    let mut v = vec![egui::Vec2::ZERO];
    for (n, r, phase) in [(6, radius * 0.45, 0.0), (10, radius, 0.5)] {
        for i in 0..n {
            let a = std::f32::consts::TAU * (i as f32 + phase) / n as f32;
            v.push(vec2(a.cos(), a.sin()) * r);
        }
    }
    v
}

fn fade(shape: &mut Shape, a: f32) {
    egui::epaint::shape_transform::adjust_colors(shape, move |c| {
        if *c != Color32::PLACEHOLDER {
            *c = c.gamma_multiply(a);
        }
    });
    fade_text(shape, a);
}

/// Galley glyph colours live in the galley; text fades through its opacity factor instead.
fn fade_text(shape: &mut Shape, a: f32) {
    match shape {
        Shape::Text(t) => t.opacity_factor *= a,
        Shape::Vec(v) => v.iter_mut().for_each(|s| fade_text(s, a)),
        _ => {}
    }
}

/// Floating surfaces showing this frame, with their rects.
fn surfaces(ctx: &egui::Context) -> Vec<(LayerId, Rect)> {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| matches!(l.order, Order::Middle | Order::Foreground | Order::Tooltip))
            .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
            .filter(|(_, r)| r.is_positive() && r.width() > 8.0 && r.height() > 8.0)
            .collect()
    })
}

/// Call once per frame, after everything is painted.
pub fn pass(ctx: &egui::Context) {
    let surfaces = surfaces(ctx);
    if surfaces.is_empty() {
        return;
    }
    let t = Tokens::get(ctx);
    let bg = LayerId::background();
    let src: Vec<ClippedShape> = ctx.graphics(|g| g.get(bg).map(|l| l.all_entries().cloned().collect()).unwrap_or_default());
    let radius = RADIUS;
    let taps = taps(radius);
    let mut under = Vec::new();
    let mut over = Vec::new();
    for (layer, r) in &surfaces {
        // Inset so the square clip stays inside the surface's rounded corners.
        let clip = r.shrink(3.5);
        let reach = r.expand(radius);
        let near: Vec<&ClippedShape> =
            src.iter().filter(|cs| !matches!(cs.shape, Shape::Callback(_)) && cs.shape.visual_bounding_rect().intersects(reach)).collect();
        // Over-compositing tap i at alpha 1/(i+1) leaves the running average of all taps.
        for (i, off) in taps.iter().enumerate() {
            let a = 1.0 / (i + 1) as f32;
            for cs in &near {
                let c = cs.clip_rect.translate(*off).intersect(clip);
                if !c.is_positive() {
                    continue;
                }
                let mut s = cs.shape.clone();
                s.translate(*off);
                if i > 0 {
                    fade(&mut s, a);
                }
                under.push(ClippedShape { clip_rect: c, shape: s });
            }
        }
        over.push((*layer, *r, rim(*r, t.dark)));
    }
    ctx.graphics_mut(|g| {
        let l = g.entry(bg);
        for cs in under {
            l.add(cs.clip_rect, cs.shape);
        }
        for (layer, r, rim) in over {
            g.entry(layer).add(r, rim);
        }
    });
}

/// Specular highlight along the top edge: brightest at the centre, gone at the corners.
pub fn rim(r: Rect, dark: bool) -> Shape {
    let peak = if dark { Color32::from_rgba_unmultiplied(205, 255, 228, 70) } else { Color32::from_white_alpha(230) };
    let (l, rgt, y) = (r.left() + 12.0, r.right() - 12.0, r.top() + 1.0);
    let mut m = Mesh::default();
    let xs = [l, l + (rgt - l) * 0.3, l + (rgt - l) * 0.7, rgt];
    let cs = [Color32::TRANSPARENT, peak, peak, Color32::TRANSPARENT];
    for (x, c) in xs.iter().zip(cs) {
        m.colored_vertex(pos2(*x, y - 0.5), c);
        m.colored_vertex(pos2(*x, y + 0.5), c);
    }
    for i in 0..3u32 {
        let b = i * 2;
        m.add_triangle(b, b + 1, b + 2);
        m.add_triangle(b + 1, b + 3, b + 2);
    }
    Shape::mesh(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_cover_both_rings() {
        let t = taps(10.0);
        assert_eq!(t.len(), 17);
        assert_eq!(t[0], egui::Vec2::ZERO);
        assert!(t.iter().all(|o| o.length() <= 10.0 + 1e-3));
        // Over-compositing at alpha 1/(i+1) is an equal-weight average: check with scalars.
        let samples = [0.0_f32, 1.0, 0.5, 0.25];
        let mut acc = 0.0;
        for (i, s) in samples.iter().enumerate() {
            let a = 1.0 / (i + 1) as f32;
            acc = acc * (1.0 - a) + s * a;
        }
        assert!((acc - samples.iter().sum::<f32>() / 4.0).abs() < 1e-6);
    }
}
