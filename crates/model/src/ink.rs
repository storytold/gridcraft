//! Brush textures: images a person imports to paint ink with, repeating along the stroke.
//!
//! They are a GridCraft extension: Excel has eight built-in ink effects and no way to carry an
//! imported image, so a texture lives in the workbook model and travels in a veryHidden worksheet
//! (`GridCraft Ink`) rather than in the InkML part. The store is workbook-wide, so one imported
//! image serves every stroke that names it, and each stroke keeps only its id and the scale it
//! paints at.
//!
//! Ids are content-addressed (128 bits of the image's SHA-256), so importing the same file twice
//! reuses the entry without the caller having to track anything.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The largest encoded image a texture may hold.
pub const MAX_INK_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
/// The most encoded image bytes a workbook's whole texture store may hold. The per-texture cap
/// alone would let [`MAX_INK_TEXTURES`] textures of 16 MiB each fill about 16 GiB, so the store
/// has a budget of its own: import, JSON loading and XLSX reading all refuse entries beyond it.
pub const MAX_INK_TEXTURE_STORE_BYTES: usize = 64 * 1024 * 1024;
/// The largest width or height, in pixels, a texture may decode to.
pub const MAX_INK_TEXTURE_SIDE: u32 = 8192;
/// The largest pixel count a texture may decode to.
pub const MAX_INK_TEXTURE_PIXELS: u64 = 16_000_000;
/// The longest name kept; longer ones are cut.
pub const MAX_INK_TEXTURE_NAME_CHARS: usize = 120;
/// How many textures a workbook may hold. Imported patterns are small; the cap keeps a hostile
/// file from making the reader hold an unbounded store.
pub const MAX_INK_TEXTURES: usize = 1024;
/// Ids are 128 bits of SHA-256 in lowercase hex: fixed length, so a hostile id can't bloat a file.
const ID_CHARS: usize = 32;

/// Whether an image of `data_len` bytes fits the store beside the `held` bytes it already holds.
/// Import, JSON loading and XLSX reading all ask this before keeping an entry.
pub fn fits_store(held: usize, data_len: usize) -> bool {
    held.saturating_add(data_len) <= MAX_INK_TEXTURE_STORE_BYTES
}

/// A repeating brush texture: an imported PNG or JPEG, decoded once and kept encoded here.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InkTexture {
    /// Content-derived identity ([`InkTexture::id_for`]); strokes name it in `InkStyle::texture`.
    pub id: String,
    /// The file name shown in the pen gallery.
    pub name: String,
    /// `image/png` or `image/jpeg`.
    pub mime: String,
    /// The encoded image, base64 in JSON. Shared, so an undo step doesn't copy the pixels.
    #[serde(with = "crate::b64::shared")]
    pub data: Arc<Vec<u8>>,
    /// Decoded width in pixels.
    pub width: u32,
    /// Decoded height in pixels.
    pub height: u32,
}

/// A stored texture is read back through [`InkTexture::from_parts`], so a hand-edited or hostile
/// JSON file cannot put an id, mime, size or payload the store refuses into the workbook.
impl<'de> Deserialize<'de> for InkTexture {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored {
            id: String,
            #[serde(default)]
            name: String,
            mime: String,
            #[serde(with = "crate::b64::capped")]
            data: Arc<Vec<u8>>,
            width: u32,
            height: u32,
        }
        let stored = Stored::deserialize(d)?;
        InkTexture::from_parts(stored.id, &stored.name, &stored.mime, stored.data, stored.width, stored.height)
            .ok_or_else(|| serde::de::Error::custom("ink texture outside the store's caps"))
    }
}

/// Reads the workbook's stored textures: an entry that no longer satisfies the caps (or does not
/// look like a texture at all) is dropped rather than failing the open, and the list stops at
/// [`MAX_INK_TEXTURES`] as it is read, so a hostile file cannot grow the store past its cap.
pub(crate) fn deserialize_store<'de, D>(d: D) -> Result<Vec<InkTexture>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{IgnoredAny, SeqAccess, Visitor};

    /// One stored entry: a texture the caps accept, or anything else, consumed and forgotten.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Texture(InkTexture),
        Ignored(IgnoredAny),
    }

    struct Textures;

    impl<'de> Visitor<'de> for Textures {
        type Value = Vec<InkTexture>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a list of imported brush textures")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut textures = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut bytes = 0usize;
            while textures.len() < MAX_INK_TEXTURES {
                match seq.next_element::<Stored>()? {
                    // Ids are content-derived: a repeat is the same image, kept once, so copies
                    // cannot crowd out other textures or survive a removal of the first entry.
                    Some(Stored::Texture(texture)) => {
                        // The store's total budget is checked as the list is read, so a file of
                        // many small images cannot grow the workbook past it either.
                        if !fits_store(bytes, texture.data.len()) {
                            continue;
                        }
                        if seen.insert(texture.id.clone()) {
                            bytes = bytes.saturating_add(texture.data.len());
                            textures.push(texture);
                        }
                    }
                    Some(Stored::Ignored(_)) => {}
                    None => break,
                }
            }
            // Nothing past the cap is kept, but the sequence is read to its end: a reader that
            // wants one fully consumed list (serde_json's) still accepts it.
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(textures)
        }
    }

    d.deserialize_seq(Textures)
}

impl InkTexture {
    /// The id the image bytes give a texture: 128 bits of their SHA-256, hex. The same bytes
    /// always get the same id, which is what makes a second import reuse the first entry.
    pub fn id_for(data: &[u8]) -> String {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(data);
        let mut id = String::with_capacity(ID_CHARS);
        for byte in digest.iter().take(ID_CHARS / 2) {
            let _ = std::fmt::write(&mut id, format_args!("{byte:02x}"));
        }
        id
    }

    /// Whether `id` has the shape [`InkTexture::id_for`] gives: a file that claims another one is
    /// refused rather than trusted, so ids stay fixed-length and content-derived.
    pub fn is_valid_id(id: &str) -> bool {
        id.len() == ID_CHARS && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }

    /// A texture from an image the caller has already decoded and bounded. Its id comes from the
    /// bytes.
    pub fn new(name: &str, mime: &str, data: Arc<Vec<u8>>, width: u32, height: u32) -> Option<InkTexture> {
        let id = Self::id_for(&data);
        Self::store(id, name, mime, data, width, height)
    }

    /// A texture with a given id, from data read back from a file.
    ///
    /// Answers `None` for an id of the wrong shape or one that is not the id of `data`, and for a
    /// mime, payload or size the store can't hold: the caps are checked here too, so nothing (an
    /// engine command, a file read back) slips past them, and a file that labels one image with
    /// another's id cannot make a stroke paint the wrong picture.
    pub fn from_parts(id: String, name: &str, mime: &str, data: Arc<Vec<u8>>, width: u32, height: u32) -> Option<InkTexture> {
        if !Self::is_valid_id(&id) || id != Self::id_for(&data) {
            return None;
        }
        Self::store(id, name, mime, data, width, height)
    }

    /// The entry itself, once its id is known to be the payload's own: the mime, format, size and
    /// name caps applied.
    fn store(id: String, name: &str, mime: &str, data: Arc<Vec<u8>>, width: u32, height: u32) -> Option<InkTexture> {
        let mime = Self::normalize_mime(mime)?;
        if data.is_empty() || data.len() > MAX_INK_TEXTURE_BYTES {
            return None;
        }
        // The bytes must be the format the mime names and as large as the caller claims: a
        // hand-edited or hostile file cannot smuggle a GIF in as a PNG, or label a picture with
        // dimensions it does not have (the painting code sizes its tiles from them).
        let (format, header_width, header_height) = Self::image_header(&data)?;
        if mime != format || (width, height) != (header_width, header_height) {
            return None;
        }
        if width == 0 || height == 0 || width > MAX_INK_TEXTURE_SIDE || height > MAX_INK_TEXTURE_SIDE {
            return None;
        }
        if u64::from(width) * u64::from(height) > MAX_INK_TEXTURE_PIXELS {
            return None;
        }
        let name: String = name.trim().chars().take(MAX_INK_TEXTURE_NAME_CHARS).collect();
        let name = if name.is_empty() { "Texture".to_string() } else { name };
        Some(InkTexture { id, name, mime, data, width, height })
    }

    /// The format the image bytes actually are, and the width and height their own header
    /// declares. `None` for anything that is not a PNG or JPEG whose header can be read, so the
    /// store never holds a file its painters cannot decode.
    fn image_header(data: &[u8]) -> Option<(&'static str, u32, u32)> {
        const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        if data.starts_with(&PNG_MAGIC) {
            // A PNG's first chunk is always IHDR: a 13-byte length, the type, then width and
            // height as big-endian `u32`s. No CRC or pixel data is read here.
            let chunk = data.get(8..24)?;
            if chunk.get(..8)? != b"\0\0\0\rIHDR" {
                return None;
            }
            let be = |at: usize| -> Option<u32> { Some(u32::from_be_bytes(chunk.get(at..at + 4)?.try_into().ok()?)) };
            return Some(("image/png", be(8)?, be(12)?));
        }
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            let (width, height) = jpeg_size(data)?;
            return Some(("image/jpeg", width, height));
        }
        None
    }

    /// `image/png` or `image/jpeg` from a mime spelling that may carry parameters or differ in
    /// case; `None` for anything else (the store holds no other format).
    pub fn normalize_mime(mime: &str) -> Option<String> {
        let base = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        match base.as_str() {
            "image/png" | "image/x-png" => Some("image/png".into()),
            "image/jpeg" | "image/jpg" | "image/pjpeg" => Some("image/jpeg".into()),
            _ => None,
        }
    }
}

/// The width and height a JPEG's frame header declares: the first SOF marker after the start of
/// image. Segments are walked by their declared lengths; `None` when the bytes are truncated or
/// the scan starts before any frame header.
fn jpeg_size(data: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 4 <= data.len() {
        if data.get(at)? != &0xFF {
            return None;
        }
        let mut marker = *data.get(at + 1)?;
        // Any number of 0xFF fill bytes may precede a marker.
        while marker == 0xFF {
            at += 1;
            marker = *data.get(at + 1)?;
        }
        match marker {
            // Standalone markers: no length follows.
            0xD8 | 0x01 | 0xD0..=0xD7 => at += 2,
            // End of image, or the scan begins before any frame header: no dimensions to read.
            0xD9 | 0xDA => return None,
            _ => {
                let length = u16::from_be_bytes([*data.get(at + 2)?, *data.get(at + 3)?]) as usize;
                if length < 2 {
                    return None;
                }
                // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC), which share the range.
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let height = u16::from_be_bytes([*data.get(at + 5)?, *data.get(at + 6)?]) as u32;
                    let width = u16::from_be_bytes([*data.get(at + 7)?, *data.get(at + 8)?]) as u32;
                    return Some((width, height));
                }
                at = at.checked_add(2 + length)?;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header (the magic and the IHDR chunk) claiming `w`×`h`. The store reads only the
    /// header, so no pixel data is needed.
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&[0, 0, 0, 13]);
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        // Bit depth, colour type, compression, filter and interlace, then a CRC the store ignores.
        v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    /// A JPEG header (SOI, an APP0 segment, then the SOF0 frame header) claiming `w`×`h`.
    fn jpeg_header(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        v.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        v.extend_from_slice(&(h as u16).to_be_bytes());
        v.extend_from_slice(&(w as u16).to_be_bytes());
        v.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        v
    }

    /// A few bytes that carry a PNG's magic; enough for the store, which never decodes.
    fn png_bytes() -> Vec<u8> {
        png_header(4, 2)
    }

    /// An id is derived from the bytes and is always the fixed length the store expects.
    /// A workbook field carrying the stored textures, as JSON loading reads it.
    #[derive(Deserialize)]
    struct Store {
        #[serde(default, deserialize_with = "deserialize_store")]
        textures: Vec<InkTexture>,
    }

    /// The one texture a test stores.
    fn stored() -> InkTexture {
        InkTexture::new("a.png", "image/png", Arc::new(png_bytes()), 4, 2).expect("stores")
    }

    #[test]
    fn json_keeps_a_valid_texture_and_its_fields() {
        let texture = stored();
        let json = serde_json::json!({"textures": [serde_json::to_value(&texture).expect("serialises")]});
        let store: Store = serde_json::from_value(json).expect("reads");
        assert_eq!(store.textures, vec![texture]);
    }

    #[test]
    fn json_textures_outside_the_caps_are_dropped_not_fatal() {
        let good = serde_json::to_value(stored()).expect("serialises");
        let mut forged = good.clone();
        forged["id"] = serde_json::json!("not-an-id");
        let mut gif = good.clone();
        gif["mime"] = serde_json::json!("image/gif");
        let mut zero = good.clone();
        zero["width"] = serde_json::json!(0);
        let mut huge = good.clone();
        huge["data"] = serde_json::json!("A".repeat(MAX_INK_TEXTURE_BYTES.div_ceil(3) * 4 + 4));
        let store: Store = serde_json::from_value(serde_json::json!({"textures": [good.clone(), forged, gif, zero, huge, 7, "junk"]}))
            .expect("a list with junk still reads");
        assert_eq!(store.textures.len(), 1, "only the entry inside the caps is kept");
        assert_eq!(store.textures.first().map(|t| t.id.as_str()), good["id"].as_str());
    }

    #[test]
    fn json_stops_at_the_texture_count_limit() {
        // Distinct payloads give distinct ids; repeats of one fill the serialised entry, not the store.
        let list: Vec<_> = (0..MAX_INK_TEXTURES + 5)
            .map(|i| {
                let mut texture = stored();
                let mut data = png_bytes();
                data.extend_from_slice(&i.to_le_bytes());
                texture.data = Arc::new(data);
                texture.id = InkTexture::id_for(&texture.data);
                serde_json::to_value(&texture).expect("serialises")
            })
            .collect();
        let store: Store = serde_json::from_value(serde_json::json!({"textures": list})).expect("reads");
        assert_eq!(store.textures.len(), MAX_INK_TEXTURES, "the store cannot grow past its cap");
    }

    #[test]
    fn json_repeated_texture_ids_are_kept_once() {
        // Ids are content-derived, so a hostile file repeating one entry must not fill the store
        // with copies: removal drops the first match, and the survivors would keep painting.
        let one = serde_json::to_value(stored()).expect("serialises");
        let store: Store = serde_json::from_value(serde_json::json!({"textures": vec![one; 3]})).expect("reads");
        assert_eq!(store.textures.len(), 1, "repeats of one id are the same image: {textures:?}", textures = store.textures);
    }

    #[test]
    fn id_is_content_derived_and_fixed_length() {
        let a = InkTexture::id_for(b"abc");
        assert_eq!(a.len(), ID_CHARS);
        assert!(InkTexture::is_valid_id(&a));
        assert_eq!(a, InkTexture::id_for(b"abc"));
        assert_ne!(a, InkTexture::id_for(b"abd"));
        assert!(!InkTexture::is_valid_id(&a.to_uppercase()));
        assert!(!InkTexture::is_valid_id("abc"));
    }

    /// The same bytes give the same id, so a second import reuses the entry.
    #[test]
    fn new_reuses_the_id_of_the_same_bytes() {
        let data = Arc::new(png_bytes());
        let a = InkTexture::new("a.png", "image/png", data.clone(), 4, 2).expect("valid");
        let b = InkTexture::new("b.png", "IMAGE/PNG; charset=binary", data, 4, 2).expect("valid");
        assert_eq!(a.id, b.id);
        assert_eq!(b.mime, "image/png");
    }

    /// The byte, side and pixel caps are enforced when a texture is made.
    #[test]
    fn caps_are_enforced() {
        let data = Arc::new(png_bytes());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(png_header(0, 2)), 0, 2).is_none());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(png_header(MAX_INK_TEXTURE_SIDE + 1, 1)), MAX_INK_TEXTURE_SIDE + 1, 1).is_none());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(png_header(8192, 8192)), 8192, 8192).is_none(), "over the pixel cap");
        assert!(InkTexture::new("a.png", "image/gif", data.clone(), 4, 2).is_none());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(vec![]), 4, 2).is_none());
    }

    /// The bytes must be the format the mime names and as large as the entry claims.
    #[test]
    fn the_bytes_must_be_the_format_and_size_the_entry_claims() {
        let png = Arc::new(png_bytes());
        assert!(InkTexture::new("a.png", "image/jpeg", png.clone(), 4, 2).is_none(), "a PNG is not labelled JPEG");
        assert!(InkTexture::new("a.png", "image/png", png.clone(), 4, 3).is_none(), "a claimed height the header does not have");
        assert!(InkTexture::new("a.png", "image/png", png.clone(), 5, 2).is_none());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(b"GIF89a\x01\x00\x01\x00".to_vec()), 1, 1).is_none(), "not a PNG or JPEG");
        // A JPEG's own frame header gives the dimensions.
        let jpeg = Arc::new(jpeg_header(7, 3));
        let t = InkTexture::new("b.jpg", "image/jpeg", jpeg.clone(), 7, 3).expect("a JPEG header is read");
        assert_eq!((t.width, t.height, t.mime.as_str()), (7, 3, "image/jpeg"));
        assert!(InkTexture::new("b.jpg", "image/jpeg", jpeg, 7, 4).is_none());
        // A truncated header is refused rather than trusted.
        assert!(InkTexture::new("c.png", "image/png", Arc::new(vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), 4, 2).is_none());
    }

    /// The store's total byte budget refuses an entry that would push it over, whatever the
    /// per-texture and count caps allow.
    #[test]
    fn the_store_byte_budget_is_a_ceiling() {
        assert!(fits_store(0, MAX_INK_TEXTURE_BYTES));
        assert!(fits_store(MAX_INK_TEXTURE_STORE_BYTES - 1, 1));
        assert!(!fits_store(MAX_INK_TEXTURE_STORE_BYTES, 1), "no room left");
        assert!(!fits_store(MAX_INK_TEXTURE_STORE_BYTES - 1, 2));
        assert!(!fits_store(usize::MAX, 1), "the arithmetic cannot wrap");
    }

    /// A read-back texture with an id of the wrong shape is refused.
    #[test]
    fn from_parts_refuses_an_id_that_is_not_the_bytes() {
        let data = Arc::new(png_bytes());
        // A well-formed id, but of another image: accepting it would let a stroke paint the wrong
        // picture for the id it names.
        let foreign = InkTexture::id_for(b"another image");
        assert!(InkTexture::is_valid_id(&foreign));
        assert!(InkTexture::from_parts(foreign, "a.png", "image/png", data.clone(), 4, 2).is_none());
        assert!(InkTexture::from_parts(InkTexture::id_for(&data), "a.png", "image/png", data, 4, 2).is_some());
    }

    #[test]
    fn from_parts_refuses_a_forged_id() {
        let data = Arc::new(png_bytes());
        let good = InkTexture::id_for(&data);
        assert!(InkTexture::from_parts("abc".into(), "a.png", "image/png", data.clone(), 4, 2).is_none());
        assert!(InkTexture::from_parts(good.to_uppercase(), "a.png", "image/png", data.clone(), 4, 2).is_none());
        assert!(InkTexture::from_parts(good, "a.png", "image/png", data, 4, 2).is_some());
    }

    /// Names are cut to the cap and never stored blank.
    #[test]
    fn names_are_bounded_and_never_blank() {
        let data = Arc::new(png_bytes());
        let long = "x".repeat(MAX_INK_TEXTURE_NAME_CHARS * 2);
        let t = InkTexture::new(&long, "image/png", data.clone(), 4, 2).expect("valid");
        assert_eq!(t.name.chars().count(), MAX_INK_TEXTURE_NAME_CHARS);
        let t = InkTexture::new("   ", "image/png", data, 4, 2).expect("valid");
        assert_eq!(t.name, "Texture");
    }
}
