//! Floating pictures (Insert › Pictures) as textures: decoded once at about the size they are
//! drawn, and kept in a cache per egui context that is bounded by bytes. The cache used to be
//! process-wide, with no limit, decoding every picture at full size and keeping it until the app
//! closed, after its workbook was closed too.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::TextureHandle;
use gridcraft_engine::model::Image;

/// Larger pictures are not decoded: the header is read first, so nothing is allocated for them.
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 64_000_000;
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
/// A texture is never wider or taller than this (nor than the GPU allows).
const MAX_TEXTURE_SIDE: u32 = 4096;
/// The smallest texture side, so a picture drawn small and zoomed in a little isn't decoded again.
const MIN_TEXTURE_SIDE: u32 = 64;
/// Pictures drawn in the last frame stay whatever this says, so none turns into a placeholder;
/// the others are dropped, least recently drawn first, to keep the cache under it.
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_CACHE_ITEMS: usize = 4096;

/// A picture: its workbook, its object id there, and its size in bytes (object ids are per
/// workbook, so two open workbooks can both have a picture 1).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Key {
    doc: u64,
    id: u32,
    len: usize,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    /// The open workbooks when the cache was last pruned.
    docs: Vec<u64>,
}

struct Entry {
    /// `None` when the picture can't be decoded: kept, so it isn't tried again every frame.
    texture: Option<TextureHandle>,
    bytes: usize,
    /// The longest side it was decoded for; `u32::MAX` when it holds all there is (the source
    /// size or the largest texture allowed), or when it can't be decoded.
    side: u32,
    used: u64,
}

impl Cache {
    /// Drops the pictures of workbooks that are no longer open.
    fn forget_closed(&mut self, open: &[u64]) {
        if self.docs == open {
            return;
        }
        self.entries.retain(|key, entry| {
            let keep = open.contains(&key.doc);
            if !keep {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
            keep
        });
        self.docs = open.to_vec();
    }

    /// The texture for picture `key` drawn with its longer side `want` pixels long.
    fn texture(&mut self, ctx: &egui::Context, key: Key, data: &[u8], want: u32) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        let side = texture_side(want, ctx.input(|i| i.max_texture_side));
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = frame;
            if entry.side >= side {
                return entry.texture.clone();
            }
        }
        // Not cached, or drawn larger than it was decoded for: decode it again at the new size.
        if let Some(old) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        let decoded = decode(data, side);
        let bytes = decoded.as_ref().map_or(0, |(image, _)| image.pixels.len().saturating_mul(4));
        while self.entries.len() >= MAX_CACHE_ITEMS || self.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            let oldest = self.entries.iter().filter(|(_, e)| e.used.saturating_add(1) < frame).min_by_key(|(_, e)| e.used).map(|(key, _)| *key);
            let Some(oldest) = oldest else { break };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        let (texture, side) = match decoded {
            Some((image, complete)) => {
                (Some(ctx.load_texture(format!("img{}", key.id), image, egui::TextureOptions::LINEAR)), if complete { u32::MAX } else { side })
            }
            None => (None, u32::MAX),
        };
        // Eviction only frees entries used before this frame, so a workbook covered in same-frame
        // pictures can leave the cache unable to make room. Then draw the texture without
        // retaining it: the handle is dropped at the end of the frame and the GPU memory goes
        // with it, instead of the budget being outflanked (finding: cache admission).
        if self.entries.len() >= MAX_CACHE_ITEMS || self.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            return texture;
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(key, Entry { texture: texture.clone(), bytes, side, used: frame });
        texture
    }
}

/// The texture side for a picture drawn `want` pixels long: the next power of two or three
/// quarters of one (…, 768, 1024, 1536, 2048), so zooming in decodes it again only at every
/// half or third larger, within the smallest and largest sides allowed.
fn texture_side(want: u32, gpu_max: usize) -> u32 {
    let max = MAX_TEXTURE_SIDE.min(u32::try_from(gpu_max).unwrap_or(u32::MAX)).max(1);
    let want = want.max(MIN_TEXTURE_SIDE);
    let Some(p) = want.checked_next_power_of_two() else { return max };
    let side = if want <= p / 4 * 3 { p / 4 * 3 } else { p };
    side.min(max)
}

/// Decodes a picture to fit a `side` × `side` square, and says whether that is the whole picture
/// (it was no larger, or `side` is as large as a texture may be).
fn decode(data: &[u8], side: u32) -> Option<(egui::ColorImage, bool)> {
    let format = image::guess_format(data).ok()?;
    let (w, h) = image::ImageReader::with_format(Cursor::new(data), format).into_dimensions().ok()?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut picture = reader.decode().ok()?;
    let complete = w.max(h) <= side || side >= MAX_TEXTURE_SIDE;
    if w.max(h) > side {
        picture = picture.thumbnail(side, side);
    }
    let rgba = picture.into_rgba8();
    Some((egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()), complete))
}

fn cache(ctx: &egui::Context) -> Arc<Mutex<Cache>> {
    // Context-local, like cell pictures: texture handles never leak between offscreen, native
    // and browser contexts.
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::new("gridcraft.floating_pictures")).clone())
}

/// Drops the textures of workbooks that are no longer open. Called every frame the grid paints.
pub(crate) fn forget_closed(ctx: &egui::Context, open: &[u64]) {
    cache(ctx).lock().unwrap_or_else(std::sync::PoisonError::into_inner).forget_closed(open);
}

/// The texture of picture `image` of workbook `doc`, drawn in `rect` (points); `None` when it
/// can't be decoded or is too large to be.
pub(crate) fn texture(ctx: &egui::Context, doc: u64, image: &Image, rect: egui::Rect) -> Option<TextureHandle> {
    let px = (rect.width().max(rect.height()) * ctx.pixels_per_point()).ceil();
    let want = if px.is_finite() && px > 0.0 { px.min(u32::MAX as f32) as u32 } else { 1 };
    let key = Key { doc, id: image.id, len: image.data.len() };
    cache(ctx).lock().unwrap_or_else(std::sync::PoisonError::into_inner).texture(ctx, key, &image.data, want)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn key(doc: u64, id: u32, data: &[u8]) -> Key {
        Key { doc, id, len: data.len() }
    }

    /// Runs one frame, so the frame number moves on as it does when the app paints.
    fn frame(ctx: &egui::Context) {
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear(); // nothing consumes the uploads in a headless test
    }

    #[test]
    fn pictures_are_decoded_at_about_the_size_drawn() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let data = png(3000, 1000, [10, 120, 200, 255]);
        let k = key(1, 1, &data);
        let small = cache.texture(&ctx, k, &data, 300).unwrap();
        assert_eq!(small.size(), [384, 128], "drawn 300 px long: decoded for 384");
        assert_eq!(cache.texture(&ctx, k, &data, 380).unwrap().id(), small.id(), "still within its size: not decoded again");
        let larger = cache.texture(&ctx, k, &data, 900).unwrap();
        assert_eq!(larger.size(), [1024, 341], "drawn larger: decoded again for 1024");
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 1024 * 341 * 4, "the smaller texture's bytes are given back");
        assert_eq!(texture_side(1280, 8192), 1536);
        assert_eq!(texture_side(1600, 8192), 2048);
        assert_eq!(texture_side(1, 8192), MIN_TEXTURE_SIDE);
        assert_eq!(texture_side(u32::MAX, 8192), MAX_TEXTURE_SIDE);
        // Never past the GPU's limit (2048 in a default context) nor the source.
        assert_eq!(cache.texture(&ctx, k, &data, 50_000).unwrap().size(), [2048, 683]);
        let tiny = png(40, 20, [0, 0, 0, 255]);
        assert_eq!(cache.texture(&ctx, key(1, 2, &tiny), &tiny, 4000).unwrap().size(), [40, 20]);
        assert_eq!(cache.entries.get(&key(1, 2, &tiny)).map(|e| e.side), Some(u32::MAX), "the whole picture: never decoded again");
    }

    #[test]
    fn the_cache_stays_within_its_budget_but_keeps_what_is_on_screen() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        // 40 pictures of 2048 × 2048 drawn in turn, one frame each: 640 MB decoded in all.
        let data: Vec<Vec<u8>> = (0..40u8).map(|i| png(2048, 2048, [i, 0, 0, 255])).collect();
        for (i, d) in data.iter().enumerate() {
            assert!(cache.texture(&ctx, key(1, i as u32, d), d, 2048).is_some());
            frame(&ctx);
        }
        assert!(cache.bytes <= MAX_CACHE_BYTES, "{} bytes kept", cache.bytes);
        assert!(cache.entries.len() < data.len());
        // Many on screen at once, over the budget: each still gets a texture this frame, but the
        // cache cannot grow past its budget whatever the workbook shows. Eight of the 16 MB
        // textures fit the 128 MB budget, and those stay pinned; the rest re-upload each frame
        // instead of being retained over it (finding: cache admission).
        // Two frames pass so the rehearsal's entries are old enough to be evictable.
        frame(&ctx);
        frame(&ctx);
        let shown: Vec<_> = data.iter().enumerate().take(12).map(|(i, d)| cache.texture(&ctx, key(1, i as u32, d), d, 2048).unwrap().id()).collect();
        for _ in 0..3 {
            frame(&ctx);
            for (i, d) in data.iter().enumerate().take(12) {
                let id = cache.texture(&ctx, key(1, i as u32, d), d, 2048).unwrap().id();
                if i < 8 {
                    assert_eq!(id, shown[i], "a picture that fits keeps its texture");
                }
            }
            assert!(cache.bytes <= MAX_CACHE_BYTES, "{} bytes kept", cache.bytes);
            assert!(cache.entries.len() <= 8, "{} entries kept", cache.entries.len());
        }
    }

    #[test]
    fn a_closed_workbook_takes_its_pictures_with_it() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let data = png(64, 64, [0, 200, 0, 255]);
        let first = cache.texture(&ctx, key(1, 1, &data), &data, 64).unwrap();
        // Another workbook's picture 1, of the same size: a texture of its own.
        let other = png(64, 64, [200, 0, 0, 255]);
        assert_eq!(other.len(), data.len());
        assert_ne!(cache.texture(&ctx, key(2, 1, &other), &other, 64).unwrap().id(), first.id());
        cache.forget_closed(&[1, 2]);
        assert_eq!(cache.entries.len(), 2);
        cache.forget_closed(&[2]);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 64 * 64 * 4);
        cache.forget_closed(&[]);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn broken_and_oversized_pictures_are_refused_once() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let garbage = vec![0xAB; 100];
        assert!(cache.texture(&ctx, key(1, 1, &garbage), &garbage, 100).is_none());
        assert_eq!(cache.entries.get(&key(1, 1, &garbage)).map(|e| (e.side, e.bytes)), Some((u32::MAX, 0)), "remembered, not retried");
        // A PNG whose header says 20,000 × 20,000 is refused from its header: nothing is decoded.
        let mut huge = png(1, 1, [0, 0, 0, 255]);
        let ihdr = huge.windows(4).position(|w| w == b"IHDR").unwrap();
        huge[ihdr + 4..ihdr + 8].copy_from_slice(&20_000u32.to_be_bytes());
        huge[ihdr + 8..ihdr + 12].copy_from_slice(&20_000u32.to_be_bytes());
        let crc = crc32(&huge[ihdr..ihdr + 17]);
        huge[ihdr + 17..ihdr + 21].copy_from_slice(&crc.to_be_bytes());
        assert_eq!(image::ImageReader::with_format(Cursor::new(&huge), image::ImageFormat::Png).into_dimensions().unwrap(), (20_000, 20_000));
        assert!(decode(&huge, 64).is_none());
        assert!(decode(&png(4, 2, [255, 0, 0, 255]), 64).is_some_and(|(i, complete)| i.size == [4, 2] && complete));
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
}
