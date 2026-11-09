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
    /// Keyed by the texture's content-addressed id, which is enough on its own: the same bytes
    /// always give the same id, so a changed payload can never hide under an old key. Keying by
    /// `String` lets a frame look up by a borrowed id instead of cloning one per stroke.
    entries: HashMap<String, Entry>,
    bytes: usize,
}

impl Tier {
    /// A tier that decodes at `side` and drops its least recently drawn uploads once `max_items`
    /// entries or `max_bytes` bytes would be exceeded.
    fn new(side: u32, max_bytes: usize, max_items: usize) -> Self {
        Self { side, max_bytes, max_items, entries: HashMap::new(), bytes: 0 }
    }

    /// The upload for `key` when this tier holds it, stamped as drawn this frame.
    fn get(&mut self, key: &str, frame: u64) -> Option<Option<TextureHandle>> {
        let entry = self.entries.get_mut(key)?;
        entry.used = frame;
        Some(entry.texture.clone())
    }

    /// Uploads `image` for `key` and trims the tier to its budgets. A hard ceiling applies to every
    /// entry, the ones drawn this frame included: only the upload being inserted is spared, so one
    /// frame painting many textures cannot grow the tier past `max_items`/`max_bytes`.
    fn insert(&mut self, ctx: &egui::Context, key: &str, image: Option<image::RgbaImage>, frame: u64) -> Option<TextureHandle> {
        let texture = image.as_ref().map(|image| upload(ctx, image));
        let bytes = texture.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
        while self.entries.len() >= self.max_items || self.bytes.saturating_add(bytes) > self.max_bytes {
            let oldest = self.entries.iter().min_by_key(|(_, e)| e.used).map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(key.to_string(), Entry { texture: texture.clone(), bytes, used: frame });
        texture
    }

    /// The upload for `key`, decoded at this tier's [`Self::side`] on first use and kept until the
    /// budget drops it. The newest frame that drew it is stamped so eviction takes the least
    /// recently painted textures.
    fn texture(&mut self, ctx: &egui::Context, key: &str, data: &[u8]) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        if let Some(texture) = self.get(key, frame) {
            return texture;
        }
        self.insert(ctx, key, decode(data, self.side), frame)
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
    /// The repeat-quality upload a stroke paints with. The gallery preview is derived from the same
    /// decode, so a texture the canvas paints is not decoded a second time when its gallery opens.
    fn stroke(&mut self, ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        if let Some(texture) = self.strokes.get(&ink.id, frame) {
            return texture;
        }
        let image = decode(&ink.data, MAX_SIDE);
        if !self.thumbs.entries.contains_key(&ink.id) {
            let thumb = image.as_ref().map(|image| shrink_to(image, THUMB_SIDE));
            self.thumbs.insert(ctx, &ink.id, thumb, frame);
        }
        self.strokes.insert(ctx, &ink.id, image, frame)
    }

    /// The small preview a gallery tile shows.
    fn thumb(&mut self, ctx: &egui::Context, ink: &InkTexture) -> Option<TextureHandle> {
        self.thumbs.texture(ctx, &ink.id, &ink.data)
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
/// store never should have accepted, a format the store does not hold, or an image over the
/// engine's own caps. The engine caps bound the decode itself, so a huge source can't allocate more
/// than a capped image before it is shrunk.
fn decode(data: &[u8], side: u32) -> Option<image::RgbaImage> {
    let format = image::guess_format(data).ok()?;
    // The store holds PNG and JPEG only; anything else is a damaged file, not a texture.
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return None;
    }
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
    let picture = reader.decode().ok()?;
    Some(shrink_to(&picture.into_rgba8(), side))
}

/// `image` shrunk to fit a `side`-pixel box, keeping its aspect ratio.
fn shrink_to(image: &image::RgbaImage, side: u32) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    if w.max(h) <= side {
        return image.clone();
    }
    let scale = f64::from(side) / f64::from(w.max(h));
    let tw = ((f64::from(w) * scale).round() as u32).max(1);
    let th = ((f64::from(h) * scale).round() as u32).max(1);
    image::imageops::thumbnail(image, tw, th)
}

/// Uploads a decoded image with the repeat wrap the tiled mesh needs.
fn upload(ctx: &egui::Context, image: &image::RgbaImage) -> TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
    let options = egui::TextureOptions {
        magnification: egui::TextureFilter::Linear,
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::Repeat,
        mipmap_mode: None,
    };
    ctx.load_texture("ink texture", color, options)
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
///
/// Every tile is a focusable button: a screen reader hears the texture's name and whether the pen
/// paints with it, and a focused or hovered tile gets an accent ring, so the gallery can be worked
/// from the keyboard.
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
            let is_selected = selected == Some(tile.id.as_str());
            if is_selected {
                painter.rect_stroke(rect, 3.0, egui::Stroke::new(2.0, tokens.accent), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, tokens.accent), egui::StrokeKind::Inside);
            }
            if resp.has_focus() {
                painter.rect_stroke(rect.expand(1.0), 3.0, egui::Stroke::new(2.0, tokens.accent), egui::StrokeKind::Outside);
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), is_selected, tile.name.clone()));
            let resp = resp.on_hover_text(&tile.name);
            // Enter or Space on a focused tile selects it, so the gallery is keyboard-operable.
            let keyboard = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
            if resp.clicked() || keyboard {
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
        let import = ui.add(egui::Button::new("+").min_size(egui::vec2(26.0, 26.0)));
        import.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Import a texture…"));
        if import.on_hover_text(tr.tr("Import a texture…")).clicked() {
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

    /// The item ceiling is hard: a frame that paints more textures than the tier holds trims the
    /// least recently drawn ones, this frame's included, so one frame cannot grow it without bound.
    #[test]
    fn the_item_ceiling_holds_even_for_textures_the_frame_paints() {
        let ctx = egui::Context::default();
        let mut tier = Tier::new(THUMB_SIDE, usize::MAX, 2);
        advance(&ctx);
        let textures: Vec<InkTexture> = (0..5).map(|i| distinct(32, 32, i)).collect();
        // One frame paints all five.
        for t in &textures {
            assert!(tier.texture(&ctx, &t.id, &t.data).is_some());
        }
        assert_eq!(tier.entries.len(), 2, "the ceiling holds even for uploads this frame drew");
        assert!(tier.entries.contains_key(&textures[4].id), "the upload just inserted is kept");
    }

    /// The byte ceiling is hard too: a frame painting textures past it drops older uploads rather
    /// than growing the tier.
    #[test]
    fn the_byte_ceiling_holds_even_for_textures_the_frame_paints() {
        let ctx = egui::Context::default();
        let one = 32 * 32 * 4;
        let mut tier = Tier::new(THUMB_SIDE, one * 2, 64);
        advance(&ctx);
        let textures: Vec<InkTexture> = (0..4).map(|i| distinct(32, 32, i)).collect();
        for t in &textures {
            tier.texture(&ctx, &t.id, &t.data);
        }
        assert_eq!(tier.entries.len(), 2, "at most two uploads fit the byte budget");
        assert!(tier.bytes <= one * 2, "the budget is not exceeded: {}", tier.bytes);
        assert!(tier.entries.contains_key(&textures[3].id), "the upload just inserted is kept");
    }

    /// Eviction takes the preview no frame has drawn for longest, keeping the ones still on screen.
    #[test]
    fn the_gallery_tier_drops_the_preview_no_frame_drew_for_longest() {
        let ctx = egui::Context::default();
        let mut tier = Tier::new(THUMB_SIDE, usize::MAX, 2);
        let a = distinct(32, 32, 1);
        let b = distinct(32, 32, 2);
        let c = distinct(32, 32, 3);
        advance(&ctx);
        tier.texture(&ctx, &a.id, &a.data);
        advance(&ctx);
        tier.texture(&ctx, &b.id, &b.data);
        advance(&ctx);
        tier.texture(&ctx, &a.id, &a.data); // still on screen, so it is not the oldest
        advance(&ctx);
        tier.texture(&ctx, &c.id, &c.data); // needs room: the preview nothing drew goes
        let keys: Vec<&String> = tier.entries.keys().collect();
        assert!(keys.contains(&&a.id) && keys.contains(&&c.id), "previews painted this frame stay");
        assert!(!keys.contains(&&b.id), "the preview no frame drew for longest is dropped");
        assert_eq!(tier.entries.len(), 2);
        assert_eq!(tier.bytes, 2 * 32 * 32 * 4, "evicted bytes leave the budget");
    }

    /// Bytes that never decoded give no texture rather than a broken upload.
    #[test]
    fn undecodable_bytes_give_no_texture() {
        let ctx = egui::Context::default();
        // A PNG header with no pixels behind it: the store accepts the header, the decoder does not.
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let t = ink(bytes, 4, 2);
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

    /// A mesh needs two points and a positive finite width and tile; anything else gives an empty
    /// mesh rather than geometry with NaNs in it.
    #[test]
    fn a_mesh_needs_two_points_and_positive_sizes() {
        let tex = TextureId::Managed(1);
        let one = [egui::pos2(1.0, 1.0)];
        let two = [egui::pos2(0.0, 0.0), egui::pos2(10.0, 0.0)];
        for (pts, width, tile) in [
            (&one[..], 4.0, 5.0),
            (&two[..], 0.0, 5.0),
            (&two[..], -4.0, 5.0),
            (&two[..], f32::NAN, 5.0),
            (&two[..], f32::INFINITY, 5.0),
            (&two[..], 4.0, 0.0),
            (&two[..], 4.0, -5.0),
            (&two[..], 4.0, f32::NAN),
        ] {
            let m = stroke_mesh(pts, width, Color32::WHITE, tex, 1.0, tile);
            assert!(m.vertices.is_empty() && m.indices.is_empty(), "no mesh for {} points, width {width}, tile {tile}", pts.len());
        }
    }

    /// A non-finite aspect falls back to square instead of poisoning the texture coordinates.
    #[test]
    fn a_non_finite_aspect_falls_back_to_square() {
        let pts = [egui::pos2(0.0, 0.0), egui::pos2(10.0, 0.0)];
        let square = stroke_mesh(&pts, 4.0, Color32::WHITE, TextureId::Managed(1), 1.0, 5.0);
        let nan = stroke_mesh(&pts, 4.0, Color32::WHITE, TextureId::Managed(1), f32::NAN, 5.0);
        assert_eq!(nan.vertices.len(), square.vertices.len());
        assert!(nan.vertices.iter().zip(&square.vertices).all(|(a, b)| a.uv == b.uv));
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
