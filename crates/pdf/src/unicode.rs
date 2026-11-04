//! Native Unicode fallback fonts. Font files remain outside the source repository.
//! Embed TrueType outlines and a ToUnicode map, rather than relying on a reader's CJK fonts.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

pub(super) struct UnicodeFont {
    bytes: Vec<u8>,
}

static FONTS: OnceLock<[Option<Arc<UnicodeFont>>; 4]> = OnceLock::new();

pub(super) const FONT_LIMIT: usize = 36;
type NamedFace = (String, bool, bool, Arc<UnicodeFont>);
static NAMED: Mutex<Vec<NamedFace>> = Mutex::new(Vec::new());

pub(super) fn font(style: usize) -> Option<Arc<UnicodeFont>> {
    if style < 4 {
        return FONTS.get_or_init(load).get(style)?.clone();
    }
    NAMED.lock().ok()?.get(style - 4).map(|entry| Arc::clone(&entry.3))
}

/// Resolve by the font's internal family name, never by an input-derived path.
pub(super) fn named(name: &str, bold: bool, italic: bool) -> Option<usize> {
    let mut fonts = NAMED.lock().ok()?;
    if let Some(i) = fonts.iter().position(|e| e.0.eq_ignore_ascii_case(name) && e.1 == bold && e.2 == italic) {
        return Some(i + 4);
    }
    if fonts.len() >= FONT_LIMIT - 4 || name.len() > 256 {
        return None;
    }
    let font = load_named(name, bold, italic)?;
    let index = fonts.len() + 4;
    fonts.push((name.to_owned(), bold, italic, Arc::new(font)));
    Some(index)
}

#[cfg(target_arch = "wasm32")]
fn load_named(_: &str, _: bool, _: bool) -> Option<UnicodeFont> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn load_named(name: &str, bold: bool, italic: bool) -> Option<UnicodeFont> {
    type CatalogFace = (Vec<String>, bool, bool, std::path::PathBuf);
    static CATALOG: OnceLock<Vec<CatalogFace>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        let mut pending = font_dirs().into_iter().map(|p| (p, 0)).collect::<Vec<_>>();
        let mut catalog = Vec::new();
        let mut visited = 0usize;
        while let Some((dir, depth)) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                visited += 1;
                if visited > 20_000 {
                    return catalog;
                }
                let path = entry.path();
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() && depth < 8 {
                    pending.push((path, depth + 1));
                    continue;
                }
                if !kind.is_file() || !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf")) {
                    continue;
                }
                let Some(bytes) = read_font(&path) else { continue };
                let Ok(face) = ttf_parser::Face::parse(&bytes, 0) else { continue };
                if !embeddable(&face) {
                    continue;
                }
                let names = face
                    .names()
                    .into_iter()
                    .filter(|n| n.name_id == ttf_parser::name_id::FAMILY || n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                    .filter_map(|n| n.to_string())
                    .take(16)
                    .collect();
                catalog.push((names, face.is_bold(), face.is_italic(), path));
            }
        }
        catalog
    });
    for (names, b, i, path) in catalog {
        if *b == bold && *i == italic && names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            let bytes = read_font(path)?;
            let face = ttf_parser::Face::parse(&bytes, 0).ok()?;
            if embeddable(&face) {
                return Some(UnicodeFont { bytes });
            }
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn read_font(path: &std::path::Path) -> Option<Vec<u8>> {
    if !std::fs::metadata(path).is_ok_and(|m| m.len() <= 16 * 1024 * 1024) {
        return None;
    }
    std::fs::read(path).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn embeddable(face: &ttf_parser::Face<'_>) -> bool {
    face.tables().glyf.is_some()
        && face.is_outline_embedding_allowed()
        && matches!(
            face.permissions(),
            Some(ttf_parser::Permissions::Installable | ttf_parser::Permissions::PreviewAndPrint | ttf_parser::Permissions::Editable)
        )
}

#[cfg(not(target_arch = "wasm32"))]
fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("GRIDCRAFT_PDF_FONT_DIR") {
        dirs.push(dir.into());
    }
    if let Some(root) = std::env::var_os("WINDIR") {
        dirs.push(std::path::PathBuf::from(root).join("Fonts"));
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(std::path::PathBuf::from(root).join("Microsoft/Windows/Fonts"));
    }
    dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts", "/Library/Fonts", "/System/Library/Fonts"].map(Into::into));
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.extend([home.join(".fonts"), home.join(".local/share/fonts"), home.join("Library/Fonts")]);
    }
    dirs
}

#[cfg(target_arch = "wasm32")]
fn load() -> [Option<Arc<UnicodeFont>>; 4] {
    [None, None, None, None]
}

#[cfg(not(target_arch = "wasm32"))]
fn load() -> [Option<Arc<UnicodeFont>>; 4] {
    let configured = std::env::var_os("GRIDCRAFT_PDF_FONT_DIR").map(std::path::PathBuf::from);
    let mut dirs = configured.into_iter().collect::<Vec<_>>();
    if let Some(root) = std::env::var_os("WINDIR") {
        dirs.push(std::path::PathBuf::from(root).join("Fonts"));
    }
    dirs.extend([
        std::path::PathBuf::from("/usr/share/fonts/truetype/liberation2"),
        std::path::PathBuf::from("/usr/share/fonts/truetype/liberation"),
        std::path::PathBuf::from("/Library/Fonts"),
    ]);
    let names = [
        ["arial.ttf", "Arial.ttf", "LiberationSans-Regular.ttf", "Helvetica.ttf"],
        ["arialbd.ttf", "Arial Bold.ttf", "LiberationSans-Bold.ttf", "Helvetica-Bold.ttf"],
        ["ariali.ttf", "Arial Italic.ttf", "LiberationSans-Italic.ttf", "Helvetica-Oblique.ttf"],
        ["arialbi.ttf", "Arial Bold Italic.ttf", "LiberationSans-BoldItalic.ttf", "Helvetica-BoldOblique.ttf"],
    ];
    std::array::from_fn(|style| {
        for dir in &dirs {
            for name in names[style] {
                let path = dir.join(name);
                // Bound allocations before reading a font supplied by the environment.
                if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= 16 * 1024 * 1024) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(path) else { continue };
                let Ok(face) = ttf_parser::Face::parse(&bytes, 0) else { continue };
                if face.tables().glyf.is_some()
                    && face.glyph_index('ă').is_some()
                    && face.is_outline_embedding_allowed()
                    && matches!(
                        face.permissions(),
                        Some(ttf_parser::Permissions::Installable | ttf_parser::Permissions::PreviewAndPrint | ttf_parser::Permissions::Editable)
                    )
                {
                    return Some(Arc::new(UnicodeFont { bytes }));
                }
            }
        }
        None
    })
}

impl UnicodeFont {
    fn face(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.bytes, 0).ok()
    }

    pub(super) fn glyph(&self, c: char) -> Option<(u16, u16)> {
        let face = self.face()?;
        let glyph = face.glyph_index(c)?;
        let advance = u32::from(face.glyph_hor_advance(glyph)?);
        let width = (advance * 1000 / u32::from(face.units_per_em())).min(u32::from(u16::MAX)) as u16;
        Some((glyph.0, width))
    }

    /// Six consecutive PDF objects: Type0, descendant, descriptor, file, widths, ToUnicode.
    pub(super) fn objects(&self, id: u32, glyphs: &BTreeMap<u16, char>) -> Vec<(u32, Vec<u8>)> {
        let Some(face) = self.face() else { return Vec::new() };
        let name = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .and_then(|n| n.to_string())
            .map(|n| n.chars().filter(char::is_ascii_alphanumeric).take(127).collect::<String>())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("GridCraftUnicode{id}"));
        let scale = 1000.0 / f32::from(face.units_per_em());
        let bbox = face.global_bounding_box();
        let metric = |v: i16| (f32::from(v) * scale).round() as i32;
        let (cid, desc, file, widths, unicode) = (id + 1, id + 2, id + 3, id + 4, id + 5);
        let mut w = String::from("[");
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /GridCraftUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries: Vec<_> = glyphs.iter().collect();
        for chunk in entries.chunks(100) {
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for &(gid, c) in chunk {
                let width = self.glyph(*c).map_or(0, |(_, w)| w);
                let _ = write!(w, "{gid} [{width}] ");
                let mut utf16 = [0u16; 2];
                let encoded = c.encode_utf16(&mut utf16);
                let _ = write!(cmap, "<{gid:04X}> <");
                for unit in encoded {
                    let _ = write!(cmap, "{unit:04X}");
                }
                cmap.push_str(">\n");
            }
            cmap.push_str("endbfchar\n");
        }
        w.push(']');
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        vec![
            (id, format!("<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {unicode} 0 R >>").into_bytes()),
            (cid, format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /CIDToGIDMap /Identity /W {widths} 0 R >>").into_bytes()),
            (desc, format!("<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",metric(bbox.x_min),metric(bbox.y_min),metric(bbox.x_max),metric(bbox.y_max),face.italic_angle(),metric(face.ascender()),metric(face.descender()),metric(face.capital_height().unwrap_or(face.ascender()))).into_bytes()),
            (file, stream(&self.bytes, &format!(" /Length1 {}",self.bytes.len()))),
            (widths, w.into_bytes()),
            (unicode, stream(cmap.as_bytes(), "")),
        ]
    }
}

fn stream(bytes: &[u8], extra: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {}{extra} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}
