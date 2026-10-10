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
        self.entries.insert(key, Entry { source: Arc::downgrade(picture), texture: texture.clone(), bytes, used: frame });
        texture
    }
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

pub(crate) fn paint(painter: &Painter, cell: Rect, picture: &Arc<CellPicture>) {
    if cell.width() <= 0.0 || cell.height() <= 0.0 || !cell.intersects(painter.clip_rect()) {
        return;
    }
    // Context-local handles must never leak between offscreen, native or browser
    // egui contexts, even when a workbook shares its Arc payload with another view.
    let cache = painter.ctx().data_mut(|data| data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::new("gridcraft.cell_pictures")).clone());
    let texture = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).texture(painter.ctx(), picture);
    let painter = painter.with_clip_rect(cell.intersect(painter.clip_rect()));
    if let Some(texture) = texture {
        if let Some(rect) = fitted_rect(cell, texture.size()) {
            painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    } else {
        crate::icons::paint(&painter, cell.shrink(2.0), crate::icons::Icon::Picture, Color32::from_gray(130));
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
}
