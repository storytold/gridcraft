use egui::epaint::text::{Fonts, Galley};
use egui::{Align2, Color32, FontId, Pos2, Vec2};
use image::{Pixel, Rgba, RgbaImage};
use std::sync::Arc;

/// A local, one-pixel-per-point text surface; no access to the desktop or GPU framebuffer.
pub(super) struct TextRaster(Fonts);

impl TextRaster {
    pub(super) fn new() -> Self {
        Self(Fonts::new(Default::default(), Default::default()))
    }

    fn layout(&mut self, text: &str, font: FontId, width: f32) -> Option<Arc<Galley>> {
        if text.len() > 64 * 1024 || !font.size.is_finite() || !(1.0..=256.0).contains(&font.size) || width.is_nan() || width <= 0.0 {
            return None;
        }
        let mut fonts = self.0.with_pixels_per_point(1.0);
        let mut seen = std::collections::HashSet::new();
        if text.chars().any(|c| !c.is_whitespace() && seen.insert(c) && !fonts.has_glyph(&font, c)) {
            return None;
        }
        Some(fonts.layout(text.to_owned(), font, Color32::WHITE, width))
    }

    pub(super) fn measure(&mut self, text: &str, font: FontId, width: f32) -> Option<Vec2> {
        Some(self.layout(text, font, width)?.size())
    }

    pub(super) fn stamp(&mut self, out: &mut RgbaImage, text: &str, at: (Pos2, Align2), font: FontId, color: [u8; 4], width: f32) -> bool {
        let Some(galley) = self.layout(text, font, width) else { return false };
        let origin = at.1.anchor_size(at.0, galley.size()).min;
        let atlas = self.0.texture_atlas().image();
        for row in &galley.rows {
            let mesh = &row.visuals.mesh;
            for quad in mesh.vertices[row.visuals.glyph_vertex_range.clone()].as_chunks::<4>().0 {
                let a = &quad[0];
                let b = &quad[3];
                let x = (origin.x + row.pos.x + a.pos.x).round() as i32;
                let y = (origin.y + row.pos.y + a.pos.y).round() as i32;
                let w = (b.pos.x - a.pos.x).round() as i32;
                let h = (b.pos.y - a.pos.y).round() as i32;
                for dy in 0.max(y.saturating_neg())..h.min((out.height() as i32).saturating_sub(y)) {
                    for dx in 0.max(x.saturating_neg())..w.min((out.width() as i32).saturating_sub(x)) {
                        let u = (a.uv.x + (dx as f32 + 0.5) * (b.uv.x - a.uv.x) / w as f32) as usize;
                        let v = (a.uv.y + (dy as f32 + 0.5) * (b.uv.y - a.uv.y) / h as f32) as usize;
                        if u < atlas.size[0] && v < atlas.size[1] {
                            let alpha = u16::from(atlas[(u, v)].a()) * u16::from(color[3]) / 255;
                            if alpha > 0 {
                                out.get_pixel_mut((x + dx) as u32, (y + dy) as u32).blend(&Rgba([color[0], color[1], color[2], alpha as u8]));
                            }
                        }
                    }
                }
            }
        }
        true
    }
}
