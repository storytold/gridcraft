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

/// A repeating brush texture: an imported PNG or JPEG, decoded once and kept encoded here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
        Self::from_parts(id, name, mime, data, width, height)
    }

    /// A texture with a given id, from data read back from a file.
    ///
    /// Answers `None` for an id of the wrong shape, a mime, payload or size the store can't hold:
    /// the caps are checked here too, so nothing (an engine command, a file read back) slips past
    /// them.
    pub fn from_parts(id: String, name: &str, mime: &str, data: Arc<Vec<u8>>, width: u32, height: u32) -> Option<InkTexture> {
        let mime = Self::normalize_mime(mime)?;
        if !Self::is_valid_id(&id) {
            return None;
        }
        if data.is_empty() || data.len() > MAX_INK_TEXTURE_BYTES {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        // The magic and a tail are enough for the store: decoding is the engine's job.
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(b"texture");
        v
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

    #[test]
    fn new_reuses_the_id_of_the_same_bytes() {
        let data = Arc::new(png_bytes());
        let a = InkTexture::new("a.png", "image/png", data.clone(), 4, 2).expect("valid");
        let b = InkTexture::new("b.png", "IMAGE/PNG; charset=binary", data, 4, 2).expect("valid");
        assert_eq!(a.id, b.id);
        assert_eq!(b.mime, "image/png");
    }

    #[test]
    fn caps_are_enforced() {
        let data = Arc::new(png_bytes());
        assert!(InkTexture::new("a.png", "image/png", data.clone(), 0, 2).is_none());
        assert!(InkTexture::new("a.png", "image/png", data.clone(), MAX_INK_TEXTURE_SIDE + 1, 1).is_none());
        assert!(InkTexture::new("a.png", "image/png", data.clone(), 8192, 8192).is_none(), "over the pixel cap");
        assert!(InkTexture::new("a.png", "image/gif", data.clone(), 4, 2).is_none());
        assert!(InkTexture::new("a.png", "image/png", Arc::new(vec![]), 4, 2).is_none());
    }

    #[test]
    fn from_parts_refuses_a_forged_id() {
        let data = Arc::new(png_bytes());
        let good = InkTexture::id_for(&data);
        assert!(InkTexture::from_parts("abc".into(), "a.png", "image/png", data.clone(), 4, 2).is_none());
        assert!(InkTexture::from_parts(good.to_uppercase(), "a.png", "image/png", data.clone(), 4, 2).is_none());
        assert!(InkTexture::from_parts(good, "a.png", "image/png", data, 4, 2).is_some());
    }

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
