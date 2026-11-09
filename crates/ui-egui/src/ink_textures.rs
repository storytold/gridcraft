//! Imported brush textures as egui textures, and the mesh that tiles one along an ink stroke.
//!
//! The decoded image is cached per egui context (so offscreen, native and browser contexts never
//! share texture handles) and uploaded once with [`egui::TextureWrapMode::Repeat`], which is what
//! lets the mesh use texture coordinates outside 0–1 and repeat the pattern along the stroke.
//! A frame therefore allocates only the mesh, never the pixels.
//!
//! Strokes and gallery tiles upload from separate budgets: a stroke repeats the pattern across the
//! pen width and wants the full detail, while a tile is 26 points wide and only needs a small
//! preview. Scrolling a long gallery therefore rebuilds cheap thumbnails without evicting the
//! textures the canvas is painting with.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::{Color32, Pos2, TextureHandle, TextureId};
use gridcraft_engine::model::InkTexture;

/// Patterns are shown small; decoding them larger than this costs memory for no visible detail.
const MAX_SIDE: u32 = 1024;
/// A gallery tile is 26 points wide, so a preview this large already fills it at 2× scaling; the
/// full-sized pattern behind it would only cost memory.
const THUMB_SIDE: u32 = 64;
/// The stroke cache's texture-memory budget; beyond it the least recently drawn textures are
/// dropped.
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CACHE_ITEMS: usize = 64;
/// The gallery cache's own budget: [`THUMB_SIDE`] uploads are 16 KiB each, so this holds a whole
/// scroll's worth for a fraction of one stroke texture.
const MAX_THUMB_BYTES: usize = 2 * 1024 * 1024;
const MAX_THUMB_ITEMS: usize = 128;

/// One cached upload: the texture handle (`None` when the bytes never decoded, so a damaged file is
/// not retried every frame), the bytes it holds, and the last frame that drew it.
struct Entry {
    texture: Option<TextureHandle>,
    bytes: usize,
    used: u64,
}

/// One budgeted set of uploads sharing a decode side. Eviction is least-recently-drawn-first and
/// stops at this tier's entries, so a gallery scrolled through a long list cannot evict the
/// canvas's strokes.
struct Tier {
    /// The largest side (in pixels) a decode is downsampled to for this use.
    side: u32,
    max_bytes: usize,
    max_items: usize,
    entries: HashMap<(String, usize), Entry>,
    bytes: usize,
}

impl Tier {
    /// A tier that decodes at `side` and drops its least recently drawn uploads once `max_items`
    /// entries or `max_bytes` bytes would be exceeded.
    fn new(side: u32, max_bytes: usize, max_items: usize) -> Self {
        Self { side, max_bytes, max_items, entries: HashMap::new(), bytes: 0 }
    }

    /// The upload for `key`, decoded at this tier's [`Self::side`] on first use and kept until the
    /// budget drops it. The newest frame that drew it is stamped so eviction takes the least
    /// recently painted textures.
    fn texture(&mut self, ctx: &egui::Context, key: (String, usize), data: &[u8]) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = frame;
            return entry.texture.clone();
        }
        let texture = decode(data, self.side).map(|image| {
            let options = egui::TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::Repeat,
                mipmap_mode: None,
            };
            ctx.load_texture("ink texture", image, options)
        });
        let bytes = texture.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
        while self.entries.len() >= self.max_items || self.bytes.saturating_add(bytes) > self.max_bytes {
            let oldest =
                self.entries.iter().filter(|(_, e)| e.used.saturating_add(1) < frame).min_by_key(|(_, e)| e.used).map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        // Entries drawn this frame or the last cannot be evicted, so a frame that fills the budget
        // leaves no room to trim. Keep the new upload anyway: dropping it would decode and upload
        // the same texture again on every frame it stays on screen, which is the thrash the cache
        // exists to prevent. The next insert once those textures leave the screen trims back to the
        // budget.
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(key, Entry { texture: texture.clone(), bytes, used: frame });
        texture
    }
}

/// The uploads this context keeps: [`Tier`] for the patterns strokes repeat, and a second tier for
/// the small previews the pen gallery shows, each with its own budget.
struct Cache {
    strokes: Tier,
    thumbs: Tier,
}

impl Default for Cache {
    /// Both tiers at their budgets: repeat-quality strokes and small gallery previews.
    fn default() -> Self {
        Self { strokes: Tier::new(MAX_SIDE, MAX_CACHE_BYTES, MAX_CACHE_ITEMS), thumbs: Tier::new(THUMB_SIDE, MAX_THUMB_BYTES, MAX_THUMB_ITEMS) }
    }
}

impl Cache {
    /// The repeat-quality upload a stroke paints with.
    fn stroke(&mut self, ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
        self.strokes.texture(ctx, (ink.id.clone(), ink.data.len()), &ink.data)
    }

    /// The small preview a gallery tile shows.
    fn thumb(&mut self, ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
        self.thumbs.texture(ctx, (ink.id.clone(), ink.data.len()), &ink.data)
    }
}

/// The cache this context owns, so offscreen, native and browser contexts never share handles.
fn cache(ctx: &egui::Context) -> Arc<Mutex<Cache>> {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::unique("gridcraft.ink_textures")).clone())
}

/// The repeat-quality texture for an imported brush pattern, the one a stroke paints with; `None`
/// when its bytes can't be decoded (the store already validated them, so this is only a safety net
/// for a damaged file).
pub(crate) fn texture(ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
    #[cfg(test)]
    note_resolve();
    cache(ctx).lock().unwrap_or_else(std::sync::PoisonError::into_inner).stroke(ctx, ink)
}

/// The small preview a gallery tile shows: the same image decoded to [`THUMB_SIDE`], cached
/// separately from the stroke textures so browsing a long gallery cannot evict what the canvas is
/// painting with.
pub(crate) fn thumbnail(ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
    #[cfg(test)]
    note_resolve();
    cache(ctx).lock().unwrap_or_else(std::sync::PoisonError::into_inner).thumb(ctx, ink)
}

// Counts calls into the texture caches, so a test can assert which tiles a frame resolved. Kept
// per thread (the test harness runs each test on its own) to stay out of other tests' counts.
#[cfg(test)]
thread_local! {
    static RESOLVES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Adds one to this thread's resolve count.
#[cfg(test)]
fn note_resolve() {
    RESOLVES.with(|count| count.set(count.get() + 1));
}

/// This thread's resolve count so far.
#[cfg(test)]
fn resolve_count() -> usize {
    RESOLVES.with(std::cell::Cell::get)
}

/// Decodes an imported image into RGBA at no more than `side` pixels a side; `None` for bytes the
/// store never should have accepted, or an image over the engine's own caps. The engine caps bound
/// the decode itself, so a huge source can't allocate more than a capped image before it is shrunk.
fn decode(data: &[u8], side: u32) -> Option<egui::ColorImage> {
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
    if w.max(h) > side {
        picture = picture.thumbnail(side, side);
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
/// removing it, and the last tile imports another. A tile scrolled out of the dropdown is neither
/// painted nor resolved, so a long gallery costs only the tiles the person can see.
pub(crate) fn gallery(ui: &mut egui::Ui, textures: &[InkTexture], selected: Option<&str>) -> Option<GalleryAction> {
    use crate::theme::Tokens;
    let tr = crate::i18n::current(ui.ctx());
    let mut action = None;
    let tokens = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tr.tr("Imported Textures")).small());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for tile in textures {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let painter = ui.painter();
            painter.rect_filled(rect, 3.0, tokens.grid_bg);
            // Culling before the cache: a clipped tile's preview would otherwise be decoded and
            // uploaded even though nothing of it reaches the screen.
            if ui.is_rect_visible(rect)
                && let Some(handle) = thumbnail(ui.ctx(), tile)
            {
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

    /// A solid PNG of `w`×`h` at the tests' colour.
    fn png(w: u32, h: u32) -> Vec<u8> {
        png_tinted(w, h, [200, 30, 60, 255])
    }

    /// A solid PNG of `w`×`h` in `rgba`, the bytes two textures can differ by.
    fn png_tinted(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    /// A texture holding `bytes`, named as a PNG.
    fn ink(bytes: Vec<u8>, w: u32, h: u32) -> InkTexture {
        InkTexture::new("t.png", "image/png", Arc::new(bytes), w, h).unwrap()
    }

    /// Distinct bytes, so each texture gets its own content-addressed id.
    fn distinct(w: u32, h: u32, seed: u8) -> InkTexture {
        ink(png_tinted(w, h, [seed.wrapping_mul(37).wrapping_add(1), 30, 60, 255]), w, h)
    }

    /// Runs an empty frame, so the next cache use is stamped a later frame — what scrolling does.
    fn advance(ctx: &egui::Context) {
        ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
    }

    /// A pattern is decoded and uploaded once, and the second call reuses that upload.
    #[test]
    fn a_texture_is_decoded_once_and_reused() {
        let ctx = egui::Context::default();
        let t = ink(png(64, 32), 64, 32);
        let a = texture(&ctx, &t).expect("decodes");
        assert_eq!(a.size(), [64, 32]);
        let b = texture(&ctx, &t).expect("cached");
        assert_eq!(a.id(), b.id(), "the second call reuses the upload");
        assert_eq!(cache(&ctx).lock().unwrap().strokes.entries.len(), 1);
    }

    /// A pattern larger than the stroke cache's side is downsampled to it.
    #[test]
    fn huge_textures_are_drawn_at_the_cached_side() {
        let ctx = egui::Context::default();
        let t = ink(png(2048, 1024), 2048, 1024);
        let a = texture(&ctx, &t).expect("decodes");
        assert_eq!(a.size()[0], MAX_SIDE as usize, "patterns are downsampled to the cache side");
    }

    /// A gallery wider than the dropdown's clip resolves a texture for the visible tile only.
    #[test]
    fn a_gallery_clipped_to_one_tile_resolves_only_that_tile() {
        let ctx = egui::Context::default();
        let textures: Vec<InkTexture> = (0..12).map(|i| distinct(32, 32, i)).collect();
        let before = resolve_count();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            // A dropdown shows a strip one tile wide: the remaining tiles are scrolled out.
            ui.set_clip_rect(egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(20.0, 400.0)));
            gallery(ui, &textures, None);
        });
        out.drop_without_applying_deltas();
        assert_eq!(resolve_count() - before, 1, "only the tile inside the clip uploads a preview");
    }

    /// A tile's preview is a small upload of its own, cached apart from the stroke's repeat-quality one.
    #[test]
    fn gallery_tiles_use_small_previews_and_leave_stroke_textures_alone() {
        let ctx = egui::Context::default();
        let t = ink(png(512, 512), 512, 512);
        let stroke = texture(&ctx, &t).expect("decodes");
        assert_eq!(stroke.size(), [512, 512], "a stroke repeats the pattern at its own detail");
        let thumb = thumbnail(&ctx, &t).expect("decodes");
        assert_eq!(thumb.size(), [THUMB_SIDE as usize; 2], "a 26-point tile needs no larger preview");
        assert_ne!(thumb.id(), stroke.id(), "the preview is a separate upload");
        assert_eq!(texture(&ctx, &t).expect("cached").id(), stroke.id(), "the stroke keeps its upload");
        let cache = cache(&ctx);
        let cache = cache.lock().unwrap();
        assert_eq!(cache.strokes.entries.len(), 1);
        assert_eq!(cache.thumbs.entries.len(), 1, "each use is cached in its own tier");
    }

    /// A 2048-pixel source gives a 64-pixel preview and a 1024-pixel stroke texture, never the source size.
    #[test]
    fn a_huge_import_decodes_bounded_for_a_gallery_preview() {
        let ctx = egui::Context::default();
        let t = ink(png(2048, 1024), 2048, 1024);
        let thumb = thumbnail(&ctx, &t).expect("decodes");
        assert_eq!(thumb.size(), [THUMB_SIDE as usize, THUMB_SIDE as usize / 2], "a preview is downsampled to the tile's side");
        let stroke = texture(&ctx, &t).expect("decodes");
        assert_eq!(stroke.size(), [MAX_SIDE as usize, MAX_SIDE as usize / 2], "a stroke keeps up to MAX_SIDE");
    }

    /// Filling the gallery tier past its budget drops previews but never the canvas's stroke textures.
    #[test]
    fn scrolling_thumbnails_evict_only_older_thumbnails() {
        let ctx = egui::Context::default();
        let mut c = Cache::default();
        c.strokes.max_items = 2;
        c.thumbs.max_items = 3;
        c.strokes.max_bytes = usize::MAX;
        c.thumbs.max_bytes = usize::MAX;
        let strokes: Vec<InkTexture> = (0..2).map(|i| distinct(256, 256, i)).collect();
        let ids: Vec<TextureId> = strokes.iter().map(|t| c.stroke(&ctx, t).expect("decodes").id()).collect();
        for i in 0..10 {
            advance(&ctx);
            let tile = distinct(256, 256, 100 + i);
            assert!(c.thumb(&ctx, &tile).is_some());
        }
        assert_eq!(c.thumbs.entries.len(), 3, "the gallery tier holds its own item budget");
        assert_eq!(c.strokes.entries.len(), 2, "scrolling never reaches the stroke tier");
        for (t, id) in strokes.iter().zip(ids) {
            assert_eq!(c.stroke(&ctx, t).expect("still cached").id(), id, "the canvas's texture is the same upload");
        }
    }

    /// A texture the frame is painting is kept even when the budget is full of textures the frame
    /// is also painting: dropping it would decode and upload it again on every repaint.
    #[test]
    fn a_full_budget_keeps_a_texture_the_frame_is_painting() {
        let ctx = egui::Context::default();
        let mut tier = Tier::new(THUMB_SIDE, usize::MAX, 1);
        let a = distinct(32, 32, 1);
        let b = distinct(32, 32, 2);
        let key = |t: &InkTexture| (t.id.clone(), t.data.len());
        advance(&ctx);
        assert!(tier.texture(&ctx, key(&a), &a.data).is_some());
        assert!(tier.texture(&ctx, key(&b), &b.data).is_some()); // no entry can be trimmed yet
        assert_eq!(tier.entries.len(), 2, "both uploads stay while the frame paints them");
        // The next frame paints only `b`, so a tile nothing draws any more can go.
        advance(&ctx);
        tier.texture(&ctx, key(&b), &b.data);
        advance(&ctx);
        let c = distinct(32, 32, 3);
        assert!(tier.texture(&ctx, key(&c), &c.data).is_some());
        assert!(!tier.entries.contains_key(&key(&a)), "a texture no frame draws any more is dropped");
        assert!(tier.entries.contains_key(&key(&c)), "and the one just painted takes its room");
    }

    /// Eviction takes the preview no frame has drawn for longest, keeping the ones still on screen.
    #[test]
    fn the_gallery_tier_drops_the_preview_no_frame_drew_for_longest() {
        let ctx = egui::Context::default();
        let mut tier = Tier::new(THUMB_SIDE, usize::MAX, 2);
        let a = distinct(32, 32, 1);
        let b = distinct(32, 32, 2);
        let c = distinct(32, 32, 3);
        let key = |t: &InkTexture| (t.id.clone(), t.data.len());
        advance(&ctx);
        tier.texture(&ctx, key(&a), &a.data);
        advance(&ctx);
        tier.texture(&ctx, key(&b), &b.data);
        advance(&ctx);
        tier.texture(&ctx, key(&a), &a.data); // still on screen, so it is not the oldest
        advance(&ctx);
        tier.texture(&ctx, key(&c), &c.data); // needs room: the preview nothing drew goes
        let keys: Vec<&String> = tier.entries.keys().map(|(id, _)| id).collect();
        assert!(keys.contains(&&a.id) && keys.contains(&&c.id), "previews painted this frame stay");
        assert!(!keys.contains(&&b.id), "the preview no frame drew for longest is dropped");
        assert_eq!(tier.entries.len(), 2);
        assert_eq!(tier.bytes, 2 * 32 * 32 * 4, "evicted bytes leave the budget");
    }

    /// Bytes that never decoded give no texture rather than a broken upload.
    #[test]
    fn undecodable_bytes_give_no_texture() {
        let ctx = egui::Context::default();
        let t = ink(vec![0x89, b'P', b'N', b'G'], 4, 2);
        assert!(texture(&ctx, &t).is_none());
    }

    /// The mesh repeats the pattern along the stroke and across its width, as a band rather than a line.
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

    /// A stroke with no direction still yields a mesh with finite positions and texture coordinates.
    #[test]
    fn a_flat_stroke_keeps_a_finite_mesh() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(0.0, 0.0), egui::pos2(0.0, 0.0)];
        let m = stroke_mesh(&pts, 2.0, Color32::WHITE, TextureId::Managed(1), 1.0, 4.0);
        assert!(m.vertices.iter().all(|v| v.pos.x.is_finite() && v.pos.y.is_finite() && v.uv.x.is_finite() && v.uv.y.is_finite()));
    }

    /// A non-square pattern keeps its aspect across the stroke instead of stretching.
    #[test]
    fn a_non_square_pattern_keeps_its_aspect_across_the_width() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(10.0, 0.0)];
        // A 2:1 (tall) image repeats half as often across the same width.
        let m = stroke_mesh(&pts, 4.0, Color32::WHITE, TextureId::Managed(1), 2.0, 5.0);
        assert!((m.vertices[1].uv.y - 0.4).abs() < 1e-5);
    }

    /// A diagonal stroke is a band the stroke's width wide, not a collapsed line.
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
