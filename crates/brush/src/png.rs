use image::ImageEncoder;

use crate::{Error, MAX_SAMPLES, Raster, Sample, Source, Style};

const VERSION: u8 = 1;
const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
// Place our ancillary, private chunk just after IHDR so recognition never scans image data.
const CHUNK: &[u8; 4] = b"gcBR";
const CHUNK_START: usize = 33;
const MAX_SOURCE_BYTES: usize = 800_000;

pub fn encode(raster: &Raster, style: Style, samples: &[Sample], mirrored: bool) -> Result<Vec<u8>, Error> {
    let first = samples.first().ok_or(Error::Empty)?;
    let samples = samples.iter().map(|s| Sample { x: s.x - first.x, y: s.y - first.y, ..*s }).collect();
    let payload =
        Source { version: VERSION, style, samples, offset: raster.offset, size: raster.size, mirrored, pixels_per_unit: raster.pixels_per_unit };
    let payload = serde_json::to_vec(&payload).map_err(|_| Error::Encode)?;
    if payload.len() > MAX_SOURCE_BYTES {
        return Err(Error::Limit);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&raster.rgba, raster.size[0], raster.size[1], image::ExtendedColorType::Rgba8)
        .map_err(|_| Error::Encode)?;
    let (head, tail) = png.split_at_checked(CHUNK_START).ok_or(Error::Encode)?;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(CHUNK);
    hasher.update(&payload);
    let mut result = Vec::with_capacity(png.len() + payload.len() + 12);
    result.extend(head);
    result.extend((payload.len() as u32).to_be_bytes());
    result.extend(CHUNK);
    result.extend(payload);
    result.extend(hasher.finalize().to_be_bytes());
    result.extend(tail);
    Ok(result)
}

pub fn has_source(png: &[u8]) -> bool {
    png.starts_with(SIGNATURE) && png.get(CHUNK_START + 4..CHUNK_START + 8) == Some(CHUNK.as_slice())
}

pub fn source(png: &[u8]) -> Option<Source> {
    if !has_source(png) {
        return None;
    }
    let size = u32::from_be_bytes(png.get(CHUNK_START..CHUNK_START + 4)?.try_into().ok()?) as usize;
    if size > MAX_SOURCE_BYTES {
        return None;
    }
    let start = CHUNK_START + 8;
    let end = start + size;
    let crc = u32::from_be_bytes(png.get(end..end + 4)?.try_into().ok()?);
    if crc32fast::hash(png.get(start - 4..end)?) != crc {
        return None;
    }
    let payload: Source = serde_json::from_slice(png.get(start..end)?).ok()?;
    let dimensions = [u32::from_be_bytes(png.get(16..20)?.try_into().ok()?), u32::from_be_bytes(png.get(20..24)?.try_into().ok()?)];
    if payload.version != VERSION || payload.samples.is_empty() || payload.samples.len() > MAX_SAMPLES || payload.size != dimensions {
        return None;
    }
    payload.style.validate().ok()?;
    if !payload.samples.iter().all(|s| s.valid())
        || !crate::renderer::valid_resolution(payload.pixels_per_unit)
        || !payload.offset.iter().all(|o| o.abs_diff(0) <= 2048)
        || !payload.size.iter().all(|d| (1..=2048).contains(d))
    {
        return None;
    }
    Some(payload)
}
