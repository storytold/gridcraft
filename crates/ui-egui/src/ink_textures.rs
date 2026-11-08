//! Imported brush textures as egui textures, and the mesh that tiles one along an ink stroke.
//!
//! The decoded image is cached per egui context (so offscreen, native and browser contexts never
//! share texture handles) and uploaded once with [`egui::TextureWrapMode::Repeat`], which is what
//! lets the mesh use texture coordinates outside 0–1 and repeat the pattern along the stroke.
//! A frame therefore allocates only the mesh, never the pixels.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::{Color32, Pos2, TextureHandle, TextureId};
use gridcraft_engine::model::InkTexture;

/// Patterns are shown small; decoding them larger than this costs memory for no visible detail.
const MAX_SIDE: u32 = 1024;
/// The cache's texture-memory budget; beyond it the least recently drawn textures are dropped.
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CACHE_ITEMS: usize = 64;

struct Entry {
    texture: Option<TextureHandle>,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(String, usize), Entry>,
    bytes: usize,
}

impl Cache {
    /// The upload for `key`, decoded on first use and kept until the budget drops it. The newest
    /// frame that drew it is stamped so eviction takes the least recently painted textures.
    fn texture(&mut self, ctx: &egui::Context, key: (String, usize), data: &[u8]) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = frame;
            return entry.texture.clone();
        }
        let texture = decode(data).map(|image| {
            let options = egui::TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::Repeat,
                mipmap_mode: None,
            };
            ctx.load_texture("ink texture", image, options)
        });
        let bytes = texture.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
        while self.entries.len() >= MAX_CACHE_ITEMS || self.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            let oldest =
                self.entries.iter().filter(|(_, e)| e.used.saturating_add(1) < frame).min_by_key(|(_, e)| e.used).map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        if self.entries.len() < MAX_CACHE_ITEMS && self.bytes.saturating_add(bytes) <= MAX_CACHE_BYTES {
            self.bytes = self.bytes.saturating_add(bytes);
            self.entries.insert(key, Entry { texture: texture.clone(), bytes, used: frame });
        }
        texture
    }
}

/// The cache this context owns, so offscreen, native and browser contexts never share handles.
fn cache(ctx: &egui::Context) -> Arc<Mutex<Cache>> {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::unique("gridcraft.ink_textures")).clone())
}

/// The texture for an imported brush pattern; `None` when its bytes can't be decoded (the store
/// already validated them, so this is only a safety net for a damaged file).
pub(crate) fn texture(ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
    let key = (ink.id.clone(), ink.data.len());
    cache(ctx).lock().unwrap_or_else(std::sync::PoisonError::into_inner).texture(ctx, key, &ink.data)
}

/// Decodes an imported image into RGBA at no more than [`MAX_SIDE`] a side; `None` for bytes the
/// store never should have accepted, or an image over the engine's own caps.
fn decode(data: &[u8]) -> Option<egui::ColorImage> {
    let format = image::guess_format(data).ok()?;
    let (w, h) = image::ImageReader::with_format(Cursor::new(data), format).into_dimensions().ok()?;
    if w == 0 || h == 0 || w > gridcraft_engine::model::MAX_INK_TEXTURE_SIDE || h > gridcraft_engine::model::MAX_INK_TEXTURE_SIDE {
        return None;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(gridcraft_engine::model::MAX_INK_TEXTURE_SIDE);
    limits.max_image_height = Some(gridcraft_engine::model::MAX_INK_TEXTURE_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut picture = reader.decode().ok()?;
    if w.max(h) > MAX_SIDE {
        picture = picture.thumbnail(MAX_SIDE, MAX_SIDE);
    }
    let rgba = picture.into_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// A mesh that paints a textured stroke through `pts` (screen points): the pattern is one repeat
/// across the stroke (`width`), `tile` points per repeat along it, and `tex_aspect` (height over
/// width) keeps a non-square image from stretching. `tint` multiplies the pattern, so the brush
/// colour still colours a textured pen.
pub(crate) fn stroke_mesh(pts: &[Pos2], width: f32, tint: Color32, tex_id: TextureId, tex_aspect: f32, tile: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(tex_id);
    if pts.len() < 2 || !width.is_finite() || width <= 0.0 || !tile.is_finite() || tile <= 0.0 {
        return mesh;
    }
    let aspect = if tex_aspect.is_finite() && tex_aspect > 0.0 { tex_aspect } else { 1.0 };
    let half = width / 2.0;
    let across_repeats = width / (tile * aspect);
    // A normal per point: across the average of its neighbouring segments, so joins have no gap.
    // (The average direction is the tangent at the join; the ribbon is built perpendicular to it.)
    let mut normals: Vec<egui::Vec2> = Vec::with_capacity(pts.len());
    for i in 0..pts.len() {
        let prev = if i > 0 { pts[i] - pts[i - 1] } else { pts[i + 1] - pts[i] };
        let next = if i + 1 < pts.len() { pts[i + 1] - pts[i] } else { pts[i] - pts[i - 1] };
        let unit = |v: egui::Vec2| {
            let len = v.length();
            if len.is_finite() && len > 1e-6 { v / len } else { egui::Vec2::X }
        };
        let (a, b) = (unit(prev), unit(next));
        let mut tangent = a + b;
        let len = tangent.length();
        tangent = if len.is_finite() && len > 1e-6 { tangent / len } else { b };
        normals.push(egui::vec2(-tangent.y, tangent.x));
    }
    let mut along = 0.0;
    for i in 0..pts.len() - 1 {
        let (p0, p1) = (pts[i], pts[i + 1]);
        let seg = p1 - p0;
        let len = seg.length();
        if !len.is_finite() {
            continue;
        }
        let u0 = along / tile;
        let u1 = (along + len) / tile;
        along += len;
        let base = mesh.vertices.len() as u32;
        for (p, n, u) in [(p0, normals[i], u0), (p1, normals[i + 1], u1)] {
            mesh.vertices.push(egui::epaint::Vertex { pos: p + n * half, uv: egui::pos2(u, 0.0), color: tint });
            mesh.vertices.push(egui::epaint::Vertex { pos: p - n * half, uv: egui::pos2(u, across_repeats), color: tint });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    mesh
}

/// What a click in the imported-texture part of a pen dropdown asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GalleryAction {
    /// Paint with this imported texture.
    Select(String),
    /// Open the import dialog.
    Import,
    /// Remove this texture from the workbook.
    Remove(String),
}

/// The imported textures of a pen dropdown, in the same list as Excel's eight built-in effects:
/// each tile shows the image itself, the one the pen paints with is outlined, a right-click offers
/// removing it, and the last tile imports another.
pub(crate) fn gallery(ui: &mut egui::Ui, textures: &[InkTexture], selected: Option<&str>) -> Option<GalleryAction> {
    use crate::theme::Tokens;
    let tr = crate::i18n::current(ui.ctx());
    let mut action = None;
    let tokens = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tr.tr("Textures")).small());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for tile in textures {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let painter = ui.painter();
            painter.rect_filled(rect, 3.0, tokens.grid_bg);
            if let Some(handle) = texture(ui.ctx(), tile) {
                painter.image(handle.id(), rect.shrink(2.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            if selected == Some(tile.id.as_str()) {
                painter.rect_stroke(rect, 3.0, egui::Stroke::new(2.0, tokens.accent), egui::StrokeKind::Inside);
            }
            let resp = resp.on_hover_text(&tile.name);
            if resp.clicked() {
                action = Some(GalleryAction::Select(tile.id.clone()));
            }
            let id = tile.id.clone();
            resp.context_menu(|ui| {
                if ui.button(tr.tr("Remove Texture")).clicked() {
                    action = Some(GalleryAction::Remove(id.clone()));
                    ui.close();
                }
            });
        }
        if ui.add(egui::Button::new("+").min_size(egui::vec2(26.0, 26.0))).on_hover_text(tr.tr("Import a texture…")).clicked() {
            action = Some(GalleryAction::Import);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 60, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn ink(bytes: Vec<u8>, w: u32, h: u32) -> InkTexture {
        InkTexture::new("t.png", "image/png", Arc::new(bytes), w, h).unwrap()
    }

    #[test]
    fn a_texture_is_decoded_once_and_reused() {
        let ctx = egui::Context::default();
        let t = ink(png(64, 32), 64, 32);
        let a = texture(&ctx, &t).expect("decodes");
        assert_eq!(a.size(), [64, 32]);
        let b = texture(&ctx, &t).expect("cached");
        assert_eq!(a.id(), b.id(), "the second call reuses the upload");
        assert_eq!(cache(&ctx).lock().unwrap().entries.len(), 1);
    }

    #[test]
    fn huge_textures_are_drawn_at_the_cached_side() {
        let ctx = egui::Context::default();
        let t = ink(png(2048, 1024), 2048, 1024);
        let a = texture(&ctx, &t).expect("decodes");
        assert_eq!(a.size()[0], MAX_SIDE as usize, "patterns are downsampled to the cache side");
    }

    #[test]
    fn undecodable_bytes_give_no_texture() {
        let ctx = egui::Context::default();
        let t = ink(vec![0x89, b'P', b'N', b'G'], 4, 2);
        assert!(texture(&ctx, &t).is_none());
    }

    #[test]
    fn the_mesh_tiles_along_the_stroke_and_across_its_width() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(5.0, 0.0), egui::pos2(10.0, 0.0)];
        let m = stroke_mesh(&pts, 4.0, Color32::WHITE, TextureId::Managed(1), 1.0, 5.0);
        assert_eq!(m.vertices.len(), 8, "two points per segment end");
        assert_eq!(m.indices.len(), 12, "two triangles per segment");
        // The ribbon is as wide as the stroke: 4 points across, centred on the line. (A mesh built
        // along the tangent instead of across it collapsed to a line, so nothing showed.)
        let (lo, hi) = m.vertices.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v.pos.y), hi.max(v.pos.y)));
        assert!((hi - lo - 4.0).abs() < 1e-5, "across the stroke: {lo}..{hi}");
        let (xlo, xhi) = m.vertices.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v.pos.x), hi.max(v.pos.x)));
        assert!((xlo - 0.0).abs() < 1e-5 && (xhi - 10.0).abs() < 1e-5, "the ribbon runs the stroke, not across it: {xlo}..{xhi}");
        // Two repeats along 20 points with a 5-point tile; one repeat across a 4-point width.
        assert!((m.vertices[4].uv.x - 1.0).abs() < 1e-5);
        assert!((m.vertices[6].uv.x - 2.0).abs() < 1e-5);
        assert!((m.vertices[1].uv.y - 0.8).abs() < 1e-5, "4 / (5 × 1)");
    }

    #[test]
    fn a_flat_stroke_keeps_a_finite_mesh() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(0.0, 0.0), egui::pos2(0.0, 0.0)];
        let m = stroke_mesh(&pts, 2.0, Color32::WHITE, TextureId::Managed(1), 1.0, 4.0);
        assert!(m.vertices.iter().all(|v| v.pos.x.is_finite() && v.pos.y.is_finite() && v.uv.x.is_finite() && v.uv.y.is_finite()));
    }

    #[test]
    fn a_non_square_pattern_keeps_its_aspect_across_the_width() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(10.0, 0.0)];
        // A 2:1 (tall) image repeats half as often across the same width.
        let m = stroke_mesh(&pts, 4.0, Color32::WHITE, TextureId::Managed(1), 2.0, 5.0);
        assert!((m.vertices[1].uv.y - 0.4).abs() < 1e-5);
    }

    #[test]
    fn a_diagonal_ribbon_is_as_wide_as_the_stroke() {
        // The ribbon is built across the stroke whatever its direction: every vertex lies one
        // half-width from the line, so a diagonal is a band, not a line.
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0)];
        let m = stroke_mesh(&pts, 2.0, Color32::WHITE, TextureId::Managed(1), 1.0, 5.0);
        // Each vertex sits one unit (half a width) off the line x = y, so |x - y| = sqrt(2).
        let expected = 2.0_f32.sqrt();
        for v in &m.vertices {
            let off = (v.pos.x - v.pos.y).abs();
            assert!((off - expected).abs() < 1e-4, "{:?} is {off} off the line, not one half-width ({expected})", v.pos);
        }
    }
}
