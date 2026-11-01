//! The sheet in a dark theme: after the cells, objects and charts are painted (in their own,
//! light colours), every colour they used is mapped to a dark equivalent. Neutrals run between
//! the theme's dark sheet background and its text colour (white paper becomes the background,
//! black ink the text); other colours keep their hue and saturation with their lightness turned
//! over, so a pale fill becomes a deep one and a deep accent a bright one. The workbook's own
//! colours are untouched: this is how they are shown, not what they are.

use egui::Color32;
use egui::epaint::{ColorMode, Shape};

use crate::theme::Tokens;

/// The dark sheet's paper and ink for the current theme.
pub(crate) fn sheet_colors(t: &Tokens) -> (Color32, Color32) {
    (t.sheet_bg(true), t.text)
}

/// The dark-sheet colour for `c`, given the dark paper `bg` and ink `fg`.
pub(crate) fn adapt(c: Color32, bg: Color32, fg: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 0 {
        return c;
    }
    let (rf, gf, bf) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d < 1e-4 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-4) };
    let out = if s < 0.12 || d < 0.06 {
        // Grey: from the ink (black) to the paper (white).
        let [br, bgc, bb, _] = bg.to_srgba_unmultiplied();
        let [fr, fgc, fb, _] = fg.to_srgba_unmultiplied();
        let k = l;
        let mix = |p: u8, q: u8| (f32::from(q) + (f32::from(p) - f32::from(q)) * k).round().clamp(0.0, 255.0) as u8;
        [mix(br, fr), mix(bgc, fgc), mix(bb, fb)]
    } else {
        let h = if d < 1e-4 {
            0.0
        } else if max == rf {
            ((gf - bf) / d).rem_euclid(6.0)
        } else if max == gf {
            (bf - rf) / d + 2.0
        } else {
            (rf - gf) / d + 4.0
        } * 60.0;
        // Lightness turned over, kept off the extremes so tints stay visible on the dark paper.
        let l2 = (0.94 - 0.84 * l).clamp(0.12, 0.86);
        let s2 = s.min(0.85);
        hsl(h, s2, l2)
    };
    Color32::from_rgba_unmultiplied(out[0], out[1], out[2], a)
}

fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b)]
}

/// Paints the sheet's background (dark when the theme is and `follow` is on) and returns whether
/// the sheet is dark.
pub(super) fn paint_background(p: &egui::Painter, rect: egui::Rect, t: &Tokens, follow: bool) -> bool {
    let dark = t.dark && follow;
    p.rect_filled(rect, 0.0, if dark { sheet_colors(t).0 } else { t.grid_bg });
    dark
}

/// Maps everything painted since shape `from` for the dark sheet, when it is dark.
pub(super) fn finish(p: &egui::Painter, from: usize, dark: bool, t: &Tokens) {
    if dark {
        let to = p.add(Shape::Noop);
        darken_painted(p.ctx(), p.layer_id(), from..to.0, t);
    }
}

/// Maps the colours of the shapes `range` painted on `layer` for the dark sheet.
pub(super) fn darken_painted(ctx: &egui::Context, layer: egui::LayerId, range: std::ops::Range<usize>, t: &Tokens) {
    let (bg, fg) = sheet_colors(t);
    let f = |c: Color32| adapt(c, bg, fg);
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for i in range {
            list.mutate_shape(egui::layers::ShapeIdx(i), |cs| recolor(&mut cs.shape, &f));
        }
    });
}

fn mode(m: &mut ColorMode, f: &dyn Fn(Color32) -> Color32) {
    if let ColorMode::Solid(c) = m {
        *c = f(*c);
    }
}

fn recolor(shape: &mut Shape, f: &dyn Fn(Color32) -> Color32) {
    match shape {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(v) => v.iter_mut().for_each(|s| recolor(s, f)),
        Shape::Circle(c) => {
            c.fill = f(c.fill);
            c.stroke.color = f(c.stroke.color);
        }
        Shape::Ellipse(e) => {
            e.fill = f(e.fill);
            e.stroke.color = f(e.stroke.color);
        }
        Shape::LineSegment { stroke, .. } => stroke.color = f(stroke.color),
        Shape::Path(p) => {
            p.fill = f(p.fill);
            mode(&mut p.stroke.color, f);
        }
        Shape::Band(b) => {
            b.fill = f(b.fill);
            b.stroke.color = f(b.stroke.color);
        }
        Shape::Rect(r) => {
            r.fill = f(r.fill);
            r.stroke.color = f(r.stroke.color);
        }
        Shape::Text(t) => {
            // A galley's colours are baked into its glyphs: override with the adapted colour of
            // its first section (cells are single-coloured; rich text keeps one colour here).
            let base = t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color);
            let base = if base == Color32::PLACEHOLDER { t.fallback_color } else { base };
            t.override_text_color = Some(f(t.override_text_color.unwrap_or(base)));
            t.underline.color = f(t.underline.color);
        }
        Shape::Mesh(m) => {
            // Pictures keep their pixels; vertex-coloured meshes (gradients) are recoloured.
            if m.texture_id == egui::TextureId::default() {
                let mesh = std::sync::Arc::make_mut(m);
                for v in &mut mesh.vertices {
                    v.color = f(v.color);
                }
            }
        }
        Shape::QuadraticBezier(q) => {
            q.fill = f(q.fill);
            mode(&mut q.stroke.color, f);
        }
        Shape::CubicBezier(c) => {
            c.fill = f(c.fill);
            mode(&mut c.stroke.color, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rgba;

    #[test]
    fn paper_becomes_the_background_and_ink_the_text() {
        let (bg, fg) = (Color32::from_rgb(0x1A, 0x1D, 0x1B), Color32::from_rgb(0xE8, 0xEC, 0xE9));
        assert_eq!(adapt(Color32::WHITE, bg, fg), bg);
        assert_eq!(adapt(Color32::BLACK, bg, fg), fg);
        // A pale green fill turns deep, a deep green bright; the hue stays green.
        let pale = adapt(Color32::from_rgb(0xE6, 0xF2, 0xEB), bg, fg);
        let deep = adapt(Color32::from_rgb(0x10, 0x7C, 0x41), bg, fg);
        assert!(Rgba::from(pale).intensity() < 0.2 && pale.g() > pale.r());
        assert!(Rgba::from(deep).intensity() > 0.3 && deep.g() > deep.r());
        assert_eq!(adapt(Color32::TRANSPARENT, bg, fg), Color32::TRANSPARENT);
    }
}
