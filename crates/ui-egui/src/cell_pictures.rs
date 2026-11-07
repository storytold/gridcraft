//! Cell pictures use the current cell geometry, independent of floating objects.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, Weak};

use egui::{Color32, Painter, Rect, TextureHandle, pos2, vec2};
use gridcraft_engine::model::CellPicture;

const MAX_ENCODED: usize = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
// The byte budget controls texture memory; sparse entries have a separate modest bound.
const MAX_CACHE_ITEMS: usize = 4096;

#[derive(Default)]
struct Cache {
    entries: HashMap<usize, Entry>,
    bytes: usize,
}

struct Entry {
    // Holding a Weak prevents this allocation address from being reused. The
    // immutable Arc is the content identity: copy/undo share it; replacement
    // creates another allocation. No source bytes are cloned or hashed per frame.
    source: Weak<CellPicture>,
    texture: Option<TextureHandle>,
    /// Reduced copies by how many times they are halved, made when one is first needed.
    reduced: Vec<(u32, TextureHandle)>,
    bytes: usize,
    used: u64,
}

impl Cache {
    fn texture(&mut self, ctx: &egui::Context, picture: &Arc<CellPicture>) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        let key = Arc::as_ptr(picture) as usize;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = frame;
            return entry.texture.clone();
        }
        // Prune discarded workbooks/payloads before allocating another texture.
        self.entries.retain(|_, entry| {
            if entry.source.strong_count() == 0 {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                false
            } else {
                true
            }
        });
        // Read only the header before admission. Never decode an image just to evict it
        // again while painting a larger visible set on the next frame.
        let source = dimensions(picture);
        let estimate = source.map_or(0, |(_, (w, h))| w as usize * h as usize * 4);
        while self.entries.len() >= MAX_CACHE_ITEMS || self.bytes.saturating_add(estimate) > MAX_CACHE_BYTES {
            let oldest = self.entries.iter().filter(|(_, e)| e.used.saturating_add(1) < frame).min_by_key(|(_, e)| e.used).map(|(key, _)| *key);
            let Some(oldest) = oldest else {
                // Keep visible images stable under pressure; show the placeholder for
                // excess content instead of re-decoding it each frame.
                return None;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        let decoded = source.and_then(|(format, _)| decode_format(picture, format));
        let bytes = decoded.as_ref().map_or(0, |image| image.pixels.len() * 4);
        let texture = decoded.map(|image| ctx.load_texture("cell picture", image, egui::TextureOptions::LINEAR));
        self.bytes += bytes;
        self.entries.insert(key, Entry { source: Arc::downgrade(picture), texture: texture.clone(), reduced: Vec::new(), bytes, used: frame });
        texture
    }

    /// Frees room for `bytes` more, from pictures not drawn lately besides `keep`. Whether there is room
    /// now: pictures in view stay, as for a whole picture.
    fn make_room(&mut self, bytes: usize, keep: usize, frame: u64) -> bool {
        while self.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            let oldest =
                self.entries.iter().filter(|(k, e)| **k != keep && e.used.saturating_add(1) < frame).min_by_key(|(_, e)| e.used).map(|(k, _)| *k);
            let Some(oldest) = oldest else { return false };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        true
    }

    /// The picture's texture for drawing it inside `room` pixels: a reduced copy when it would
    /// otherwise shrink by more than half, because the renderer samples a texture without mipmaps and
    /// thin lines would break into dots.
    fn texture_for(&mut self, ctx: &egui::Context, picture: &Arc<CellPicture>, room: egui::Vec2) -> Option<TextureHandle> {
        let full = self.texture(ctx, picture)?;
        let halvings = halvings_to_fit(full.size(), room);
        if halvings == 0 {
            return Some(full);
        }
        let key = Arc::as_ptr(picture) as usize;
        if let Some((_, texture)) = self.entries.get(&key)?.reduced.iter().find(|(n, _)| *n == halvings) {
            return Some(texture.clone());
        }
        // One that can't be made, or that there is no room for, falls back to the whole picture, and
        // isn't tried again each frame.
        let image = reduced(picture, halvings).filter(|image| self.make_room(image.pixels.len() * 4, key, ctx.cumulative_frame_nr()));
        let (texture, bytes) = match image {
            Some(image) => {
                let bytes = image.pixels.len() * 4;
                (ctx.load_texture("cell picture, reduced", image, egui::TextureOptions::LINEAR), bytes)
            }
            None => (full, 0),
        };
        let entry = self.entries.get_mut(&key)?;
        entry.reduced.push((halvings, texture.clone()));
        entry.bytes += bytes;
        self.bytes += bytes;
        Some(texture)
    }
}

/// How many times a picture of `size` pixels can be halved and still be at least as large as it is
/// drawn, fitted inside `room` pixels.
fn halvings_to_fit(size: [usize; 2], room: egui::Vec2) -> u32 {
    if size.contains(&0) || room.x < 1.0 || room.y < 1.0 {
        return 0;
    }
    let scale = (room.x / size[0] as f32).min(room.y / size[1] as f32);
    if scale >= 0.5 { 0 } else { ((1.0 / scale).log2().floor() as u32).min(usize::BITS - 1 - size[0].min(size[1]).leading_zeros()) }
}

/// The picture halved `halvings` times, each pixel an average of those it covers.
fn reduced(picture: &CellPicture, halvings: u32) -> Option<egui::ColorImage> {
    let rgba = rgba(picture)?;
    let small = image::imageops::thumbnail(&rgba, (rgba.width() >> halvings).max(1), (rgba.height() >> halvings).max(1));
    Some(egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw()))
}

fn dimensions(picture: &CellPicture) -> Option<(image::ImageFormat, (u32, u32))> {
    if picture.data.len() > MAX_ENCODED {
        return None;
    }
    let format = image::guess_format(&picture.data).ok()?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return None;
    }
    let dimensions = image::ImageReader::with_format(Cursor::new(&picture.data), format).into_dimensions().ok()?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > 8192
        || dimensions.1 > 8192
        || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS
    {
        return None;
    }
    Some((format, dimensions))
}

/// The picture's pixels, within the same limits as for drawing it.
pub(crate) fn rgba(picture: &CellPicture) -> Option<image::RgbaImage> {
    let (format, _) = dimensions(picture)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(&picture.data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Some(reader.decode().ok()?.into_rgba8())
}

#[cfg(test)]
fn decode(picture: &CellPicture) -> Option<egui::ColorImage> {
    let (format, _) = dimensions(picture)?;
    decode_format(picture, format)
}

fn decode_format(picture: &CellPicture, format: image::ImageFormat) -> Option<egui::ColorImage> {
    let mut reader = image::ImageReader::with_format(Cursor::new(&picture.data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.into_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

fn fitted_rect(cell: Rect, size: [usize; 2]) -> Option<Rect> {
    if cell.width() <= 0.0 || cell.height() <= 0.0 || size.contains(&0) {
        return None;
    }
    let source = vec2(size[0] as f32, size[1] as f32);
    let scale = (cell.width() / source.x).min(cell.height() / source.y);
    Some(Rect::from_center_size(cell.center(), source * scale))
}

/// Draws a cached cell picture fitted inside the cell and clipped to it, or paints the picture fallback icon when no texture is available.
pub(crate) fn paint(painter: &Painter, cell: Rect, picture: &Arc<CellPicture>) {
    if cell.width() <= 0.0 || cell.height() <= 0.0 || !cell.intersects(painter.clip_rect()) {
        return;
    }
    // Context-local handles must never leak between offscreen, native or browser
    // egui contexts, even when a workbook shares its Arc payload with another view.
    let cache = painter.ctx().data_mut(|data| data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::unique("gridcraft.cell_pictures")).clone());
    let room = cell.size() * painter.ctx().pixels_per_point();
    let texture = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).texture_for(painter.ctx(), picture, room);
    let painter = painter.with_clip_rect(cell.intersect(painter.clip_rect()));
    if let Some(texture) = texture {
        if let Some(rect) = fitted_rect(cell, texture.size()) {
            painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    } else {
        crate::icons::paint_on_sheet(
            &painter,
            cell.shrink(2.0),
            crate::icons::Icon::Picture,
            crate::theme::Tokens::get(painter.ctx()).sheet_control_border,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(color: [u8; 4]) -> Result<Arc<CellPicture>, image::ImageError> {
        let image = image::RgbaImage::from_pixel(4, 2, image::Rgba(color));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(Arc::new(CellPicture { data: bytes.into_inner(), mime: "image/png".into(), alt: "Test picture".into() }))
    }

    #[test]
    fn aspect_fit_and_bounded_content_cache() -> Result<(), Box<dyn std::error::Error>> {
        let cell = Rect::from_min_size(pos2(10.0, 20.0), vec2(100.0, 100.0));
        assert_eq!(fitted_rect(cell, [200, 100]), Some(Rect::from_min_size(pos2(10.0, 45.0), vec2(100.0, 50.0))));
        assert_eq!(fitted_rect(cell, [100, 200]), Some(Rect::from_min_size(pos2(35.0, 20.0), vec2(50.0, 100.0))));
        assert!(fitted_rect(Rect::NOTHING, [200, 100]).is_none());
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let red = picture([255, 0, 0, 255])?;
        let blue = picture([0, 0, 255, 255])?;
        // Distinct content with identical dimensions must receive distinct handles.
        let red_id = cache.texture(&ctx, &red).ok_or("missing red texture")?.id();
        assert_eq!(cache.texture(&ctx, &red).ok_or("missing red texture")?.id(), red_id);
        assert_ne!(cache.texture(&ctx, &blue).ok_or("missing blue texture")?.id(), red_id);
        let payloads: Vec<_> = (0..133).map(|_| picture([0, 0, 255, 255])).collect::<Result<_, _>>()?;
        for payload in &payloads {
            cache.texture(&ctx, payload);
        }
        let ids: Vec<_> =
            payloads.iter().map(|p| cache.texture(&ctx, p).ok_or("missing visible texture").map(|t| t.id())).collect::<Result<_, _>>()?;
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |_| {
                for (payload, id) in payloads.iter().zip(&ids) {
                    assert_eq!(cache.texture(&ctx, payload).map(|t| t.id()), Some(*id));
                }
            });
            // This test observes handles without a renderer consuming their upload deltas.
            output.textures_delta.clear();
        }
        assert!(cache.entries.len() <= MAX_CACHE_ITEMS);
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        assert!(decode(&CellPicture { data: vec![], mime: "image/png".into(), alt: String::new() }).is_none());
        assert_eq!(decode(&red).ok_or("decode failed")?.pixels.first(), Some(&Color32::RED));
        Ok(())
    }

    /// Black lines one pixel wide, every fourth pixel, on white.
    fn thin_lines(width: u32, height: u32) -> Result<Arc<CellPicture>, image::ImageError> {
        let image = image::RgbaImage::from_fn(width, height, |x, _| if x % 4 == 0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255; 4]) });
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(Arc::new(CellPicture { data: bytes.into_inner(), mime: "image/png".into(), alt: "Thin lines".into() }))
    }

    #[test]
    fn a_reduced_copy_stays_within_the_budget() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let (seen, older) = (thin_lines(64, 64)?, thin_lines(64, 64)?);
        let small = vec2(8.0, 8.0);
        assert!(cache.texture(&ctx, &older).is_some());
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |_| {});
            output.textures_delta.clear();
        }
        // With a picture not drawn lately, that one makes the room.
        let whole = cache.texture(&ctx, &seen).ok_or("no texture")?.id();
        cache.bytes = MAX_CACHE_BYTES;
        let reduced = cache.texture_for(&ctx, &seen, small).ok_or("no texture")?;
        assert_ne!(reduced.id(), whole);
        assert!(!cache.entries.contains_key(&(Arc::as_ptr(&older) as usize)));
        assert!(cache.bytes <= MAX_CACHE_BYTES, "{}", cache.bytes);
        // With none to give up, the whole picture is drawn, and nothing goes past the budget.
        let last = thin_lines(64, 64)?;
        cache.bytes = 0;
        let whole = cache.texture(&ctx, &last).ok_or("no texture")?.id();
        cache.bytes = MAX_CACHE_BYTES;
        assert_eq!(cache.texture_for(&ctx, &last, small).ok_or("no texture")?.id(), whole);
        assert_eq!(cache.bytes, MAX_CACHE_BYTES);
        Ok(())
    }

    #[test]
    fn a_picture_is_halved_only_while_it_stays_at_least_as_large_as_it_is_drawn() {
        assert_eq!(halvings_to_fit([100, 100], vec2(100.0, 100.0)), 0);
        assert_eq!(halvings_to_fit([100, 100], vec2(50.0, 50.0)), 0, "shrinking by up to half samples cleanly");
        assert_eq!(halvings_to_fit([100, 100], vec2(49.0, 49.0)), 1);
        assert_eq!(halvings_to_fit([1136, 868], vec2(64.0, 20.0)), 5, "the fitted size decides: 868 to 20 is 43 times");
        assert_eq!(halvings_to_fit([8192, 1], vec2(10.0, 10.0)), 0, "a side of one pixel can't be halved");
        assert_eq!(halvings_to_fit([0, 10], vec2(10.0, 10.0)), 0);
        assert_eq!(halvings_to_fit([100, 100], vec2(0.0, 10.0)), 0);
    }

    #[test]
    fn thin_lines_stay_lines_when_a_picture_is_drawn_small() -> Result<(), Box<dyn std::error::Error>> {
        let picture = thin_lines(512, 256)?;
        let small = reduced(&picture, 4).ok_or("not reduced")?;
        assert_eq!(small.size, [32, 16]);
        // Sampling the whole picture would give black or white. An average of what each pixel covers
        // gives the same grey everywhere: a quarter black.
        for pixel in &small.pixels {
            assert!((pixel.r() as i32 - 191).abs() <= 2, "{pixel:?}");
        }
        Ok(())
    }

    #[test]
    fn a_small_drawing_uses_a_small_texture_once_and_counts_its_memory() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let picture = thin_lines(512, 256)?;
        let full = cache.texture_for(&ctx, &picture, vec2(512.0, 256.0)).ok_or("no texture")?;
        assert_eq!(full.size(), [512, 256]);
        let whole = cache.bytes;
        let small = cache.texture_for(&ctx, &picture, vec2(64.0, 32.0)).ok_or("no texture")?;
        assert_eq!(small.size(), [64, 32]);
        assert_eq!(cache.bytes, whole + 64 * 32 * 4);
        assert_eq!(cache.texture_for(&ctx, &picture, vec2(64.0, 32.0)).ok_or("no texture")?.id(), small.id(), "made once");
        assert_eq!(cache.bytes, whole + 64 * 32 * 4);
        assert_eq!(cache.texture_for(&ctx, &picture, vec2(512.0, 256.0)).ok_or("no texture")?.id(), full.id());
        Ok(())
    }
}
