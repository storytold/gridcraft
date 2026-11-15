//! Insert ▸ Library: the pictures in the user's own folders, on the Library core the Craft apps
//! share ([`craft_library`], first asked for in storytold/deckcraft#958). This module is what is
//! GridCraft's own: the files its Pictures tab lists ([`Pictures`]: the PNG and JPEG pictures
//! Insert ▸ Pictures takes, one picture per file), how it opens one and draws its thumbnail, and
//! its file access ([`Confined`]), which goes through [`crate::io`] so the scripts' file sandbox
//! applies to every folder listing and file read. The `library.*` commands are in
//! [`crate::cmd::library`].

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use craft_library::{
    EntryKind, Item, MAX_DEPTH, MAX_ENTRIES, MAX_FILE_BYTES, MAX_FOLDERS, MAX_ITEMS, MAX_PATH, MAX_THUMB_HEIGHT, Nouns, Stamp, THUMB_WIDTH,
    THUMB_WIDTHS, Thumbnail, sanitized_folders,
};
use image::{DynamicImage, ImageFormat};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::io;

/// The files the Pictures tab lists: the pictures `insert.picture` takes.
pub const EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The items' names, singular and plural: the keys of the `library.*` answers.
pub const NOUNS: Nouns = Nouns { one: "picture", many: "pictures" };

/// What every `library.*` command says in the browser build, which has no folders to read.
pub const NOT_IN_BROWSER: &str =
    "the Library is not available in the browser build: it reads folders of pictures on this computer, which a web page cannot";

/// The largest picture file the Library offers: what Insert ▸ Pictures places over cells.
pub const MAX_PICTURE_BYTES: usize = 64 << 20;

/// Pictures larger than this are not decoded for a thumbnail (the sheet doesn't draw them either):
/// the header is read first, so nothing is allocated for them.
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 64_000_000;
const MAX_DECODE_BYTES: u64 = 512 << 20;

/// GridCraft's Library and what it has read (in [`crate::Session`]).
pub type Cache = craft_library::Library<Pictures>;

/// Is `name` a file the Pictures tab lists? Hidden files and Office lock files are not.
pub fn is_picture(name: &str) -> bool {
    craft_library::is_listed(name, &EXTENSIONS)
}

/// Reads the saved folder list leniently: paths only, without repeats, at most [`MAX_FOLDERS`]. A
/// value that is not a list gives none, so a hand-edited or hostile `prefs.json` loses this list,
/// never the other preferences.
pub(crate) fn lenient_folders<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let raw = Json::deserialize(d)?;
    let paths = raw.as_array().map(|a| a.iter().filter_map(Json::as_str).take(MAX_FOLDERS * 4).map(String::from).collect()).unwrap_or_default();
    Ok(sanitized_folders(paths))
}

/// File access through [`crate::io`]: refused inside a script or macro step, as every file read is.
pub struct Confined;

impl craft_library::Fs for Confined {
    fn read_dir(&self, path: &Path, max: usize) -> Result<(Vec<craft_library::DirEntry>, bool), String> {
        io::library_entries(path, max)
    }
    fn stat(&self, path: &Path) -> Result<(EntryKind, Stamp), String> {
        io::library_stat(path)
    }
    fn canonical(&self, path: &Path) -> Result<PathBuf, String> {
        io::library_canonical(path)
    }
    fn read(&self, path: &Path, max: u64) -> Result<Vec<u8>, String> {
        io::library_read(path, max)
    }
}

/// A picture file as the Library holds it: its bytes, which an insert puts into the workbook as
/// they are, and what its header says. Nothing is decoded until a thumbnail is drawn.
#[derive(Clone, Debug)]
pub struct Picture {
    /// The file name, which is the item's title.
    pub name: String,
    pub bytes: Vec<u8>,
    /// The format's name, for search (`PNG`, `JPEG`).
    pub format: &'static str,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Picture files, each holding one picture.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pictures;

impl craft_library::Kind for Pictures {
    type Doc = Picture;

    fn extensions(&self) -> &[&str] {
        &EXTENSIONS
    }

    fn nouns(&self) -> Nouns {
        NOUNS
    }

    /// Reads only the header: a file that isn't a PNG or JPEG picture (whatever its name says), or
    /// is larger than Insert ▸ Pictures takes, is listed with why.
    fn open(&self, name: &str, bytes: &[u8]) -> Result<Picture, String> {
        let (format, mime) = kind_of(bytes).ok_or_else(|| format!("{name} is not a PNG or JPEG picture"))?;
        if bytes.len() > MAX_PICTURE_BYTES {
            return Err(format!("larger than {} MB, the most Insert ▸ Pictures takes", MAX_PICTURE_BYTES >> 20));
        }
        let (width, height) = dimensions(bytes, format).ok_or_else(|| format!("{name}: the picture's size can't be read"))?;
        Ok(Picture { name: name.to_string(), bytes: bytes.to_vec(), format: format_name(format), mime, width, height })
    }

    fn title(&self, _doc: &Picture) -> Option<String> {
        None
    }

    fn count(&self, _doc: &Picture) -> usize {
        1
    }

    fn item(&self, doc: &Picture, _index: usize) -> Item {
        Item { title: doc.name.clone(), hidden: false, text: format!("{} {} × {}", doc.format, doc.width, doc.height) }
    }

    fn thumbnail(&self, doc: &Picture, _index: usize, width: u32) -> Result<Thumbnail, String> {
        draw_thumbnail(&doc.bytes, width)
    }
}

/// The picture format of `bytes` by their content, when it is one GridCraft inserts, and its type.
fn kind_of(bytes: &[u8]) -> Option<(ImageFormat, &'static str)> {
    match image::guess_format(bytes).ok()? {
        ImageFormat::Png => Some((ImageFormat::Png, "image/png")),
        ImageFormat::Jpeg => Some((ImageFormat::Jpeg, "image/jpeg")),
        _ => None,
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        _ => "JPEG",
    }
}

/// The picture's size from its header; `None` when it can't be read or has no pixels.
fn dimensions(bytes: &[u8], format: ImageFormat) -> Option<(u32, u32)> {
    let (w, h) = image::ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// The size a `w` × `h` picture is drawn at for a thumbnail `width` pixels wide: never wider than
/// the picture itself (the pane scales a small one up) and never taller than
/// [`MAX_THUMB_HEIGHT`], keeping its shape; at least a pixel each way.
pub fn thumbnail_size(w: u32, h: u32, width: u32) -> (u32, u32) {
    let (fw, fh) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let mut scale = (f64::from(width.max(1)) / fw).min(1.0);
    if fh * scale > f64::from(MAX_THUMB_HEIGHT) {
        scale = f64::from(MAX_THUMB_HEIGHT) / fh;
    }
    // `as` saturates, and the clamps keep both sides within the thumbnail's bounds.
    let tw = ((fw * scale).round() as u32).clamp(1, width.max(1));
    let th = ((fh * scale).round() as u32).clamp(1, MAX_THUMB_HEIGHT);
    (tw, th)
}

/// Draw the picture in `bytes` at most `width` pixels wide as PNG. It is decoded within the bounds
/// the sheet draws pictures in, so a picture the sheet can't draw has no thumbnail either.
pub fn draw_thumbnail(bytes: &[u8], width: u32) -> Result<Thumbnail, String> {
    let (format, _) = kind_of(bytes).ok_or("not a PNG or JPEG picture")?;
    let (w, h) = dimensions(bytes, format).ok_or("the picture's size can't be read")?;
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("{w} × {h} pixels is too large to draw (at most {MAX_SIDE} a side and 64 million in all)"));
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("the picture can't be decoded: {e}"))?;
    let (tw, th) = thumbnail_size(img.width(), img.height(), width.clamp(*THUMB_WIDTHS.start(), *THUMB_WIDTHS.end()));
    let small = DynamicImage::ImageRgba8(img.thumbnail_exact(tw, th).to_rgba8());
    let mut png = Cursor::new(Vec::new());
    small.write_to(&mut png, ImageFormat::Png).map_err(|e| format!("the thumbnail can't be drawn: {e}"))?;
    Ok(Thumbnail { png: Arc::new(png.into_inner()), width: small.width(), height: small.height() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_library::Kind;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(w, h, image::Rgba([20, 120, 200, 255])).write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_are_named_by_extension() {
        for yes in ["a.png", "A.PNG", "b.jpg", "c.JPEG"] {
            assert!(is_picture(yes), "{yes}");
        }
        for no in ["~$a.png", ".a.png", "a.svg", "a.gif", "a.webp", "a.xlsx", "png", "a.png.txt", ".png"] {
            assert!(!is_picture(no), "{no}");
        }
    }

    #[test]
    fn thumbnails_keep_the_shape_within_bounds() {
        assert_eq!(thumbnail_size(1600, 900, 160), (160, 90));
        // A small picture is not enlarged.
        assert_eq!(thumbnail_size(40, 20, 160), (40, 20));
        // A very tall one is drawn narrower, no taller than the limit.
        assert_eq!(thumbnail_size(10, 1_000_000, 512), (1, MAX_THUMB_HEIGHT));
        // Hostile sizes stay finite and at least a pixel.
        assert_eq!(thumbnail_size(0, 0, 0), (1, 1));
        assert_eq!(thumbnail_size(u32::MAX, 1, 512), (512, 1));
        assert_eq!(thumbnail_size(1, u32::MAX, 512), (1, MAX_THUMB_HEIGHT));
    }

    #[test]
    fn a_picture_is_known_by_its_content_not_its_name() {
        let p = Pictures.open("logo.png", &png(30, 20)).unwrap();
        assert_eq!((p.format, p.mime, p.width, p.height), ("PNG", "image/png", 30, 20));
        assert_eq!(Pictures.count(&p), 1);
        assert_eq!(Pictures.item(&p, 0), Item { title: "logo.png".into(), hidden: false, text: "PNG 30 × 20".into() });
        for (name, bytes) in [("text.png", b"not a picture at all".to_vec()), ("empty.jpg", vec![]), ("gif.png", b"GIF89a\x01\x00\x01\x00".to_vec())]
        {
            assert!(Pictures.open(name, &bytes).is_err(), "{name}");
        }
        // A PNG cut short after its signature has no size to read.
        assert!(Pictures.open("cut.png", &png(30, 20)[..12]).is_err());
    }

    #[test]
    fn thumbnails_are_png_no_wider_than_asked() {
        let t = draw_thumbnail(&png(800, 400), 160).unwrap();
        assert_eq!((t.width, t.height), (160, 80));
        assert!(t.png.starts_with(&[0x89, b'P', b'N', b'G']));
        // A header that claims more pixels than the sheet draws is refused before decoding.
        let mut huge = png(1, 1);
        huge.get_mut(16..20).unwrap().copy_from_slice(&100_000u32.to_be_bytes());
        huge.get_mut(20..24).unwrap().copy_from_slice(&100_000u32.to_be_bytes());
        // The IHDR chunk's CRC covers its type and data (bytes 12..29), and sits at 29..33.
        let crc = crc32(&huge[12..29]);
        huge.get_mut(29..33).unwrap().copy_from_slice(&crc.to_be_bytes());
        assert!(draw_thumbnail(&huge, 160).unwrap_err().contains("too large"), "{:?}", draw_thumbnail(&huge, 160).err());
    }

    /// The CRC-32 PNG chunks carry.
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    #[test]
    fn saved_folders_are_read_leniently() {
        #[derive(Deserialize)]
        struct P {
            #[serde(deserialize_with = "lenient_folders")]
            f: Vec<String>,
        }
        let read = |v: Json| serde_json::from_value::<P>(serde_json::json!({ "f": v })).unwrap().f;
        assert_eq!(read(serde_json::json!(["/a", "", "/a", 5, "/b"])), ["/a", "/b"]);
        assert!(read(serde_json::json!("not a list")).is_empty());
        let many: Vec<String> = (0..100).map(|i| format!("/f{i}")).collect();
        assert_eq!(read(serde_json::json!(many)).len(), MAX_FOLDERS);
    }
}
