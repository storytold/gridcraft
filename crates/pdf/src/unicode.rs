//! Native Unicode fallback fonts. Font files remain outside the source repository.
//! Embed TrueType outlines and a ToUnicode map, rather than relying on a reader's CJK fonts.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

/// Owned, embeddable TrueType data with a bounded cache of glyph metrics.
pub(super) struct UnicodeFont {
    bytes: Vec<u8>,
    glyphs: Mutex<BTreeMap<char, Option<(u16, u16)>>>,
}

/// Limit process-wide cache growth even for input containing many distinct characters.
const GLYPH_CACHE_LIMIT: usize = 4096;

static FONTS: OnceLock<[Option<Arc<UnicodeFont>>; 4]> = OnceLock::new();

pub(super) const FONT_LIMIT: usize = 36;
type NamedFace = (String, bool, bool, Arc<UnicodeFont>);
static NAMED: Mutex<Vec<NamedFace>> = Mutex::new(Vec::new());
type MissingFace = (String, bool, bool);
static MISSING: Mutex<BTreeSet<MissingFace>> = Mutex::new(BTreeSet::new());
/// Missing names do not consume the limited embedded-font slots.
const MISSING_CACHE_LIMIT: usize = 256;

/// Return an installed fallback face (slots 0..4) or a resolved named face.
pub(super) fn font(style: usize) -> Option<Arc<UnicodeFont>> {
    if style < 4 {
        return FONTS.get_or_init(load).get(style)?.clone();
    }
    NAMED.lock().ok()?.get(style - 4).map(|entry| Arc::clone(&entry.3))
}

/// Fallback slot matching the actual weight and slant, independent of allocation order.
pub(super) fn style(index: usize) -> usize {
    if index < 4 {
        return index;
    }
    NAMED.lock().ok().and_then(|fonts| fonts.get(index - 4).map(|e| usize::from(e.1) | (usize::from(e.2) << 1))).unwrap_or(0)
}

/// Resolve by the font's internal family name, never by an input-derived path.
pub(super) fn named(name: &str, bold: bool, italic: bool) -> Option<usize> {
    resolve_named(&NAMED, &MISSING, name, bold, italic, || load_named(name, bold, italic))
}

/// Keep filesystem work outside the cache locks and recheck concurrent insertions.
fn resolve_named(
    named: &Mutex<Vec<NamedFace>>,
    missing: &Mutex<BTreeSet<MissingFace>>,
    name: &str,
    bold: bool,
    italic: bool,
    load: impl FnOnce() -> Option<UnicodeFont>,
) -> Option<usize> {
    if name.len() > 256 {
        return None;
    }
    {
        let fonts = named.lock().ok()?;
        if let Some(i) = fonts.iter().position(|e| e.0.eq_ignore_ascii_case(name) && e.1 == bold && e.2 == italic) {
            return Some(i + 4);
        }
        if fonts.len() >= FONT_LIMIT - 4 {
            return None;
        }
    }
    let key = (name.to_ascii_lowercase(), bold, italic);
    if missing.lock().ok()?.contains(&key) {
        return None;
    }
    let Some(font) = load() else {
        let mut cache = missing.lock().ok()?;
        if cache.len() < MISSING_CACHE_LIMIT {
            cache.insert(key);
        }
        return None;
    };
    let mut fonts = named.lock().ok()?;
    if let Some(i) = fonts.iter().position(|e| e.0.eq_ignore_ascii_case(name) && e.1 == bold && e.2 == italic) {
        return Some(i + 4);
    }
    if fonts.len() >= FONT_LIMIT - 4 {
        return None;
    }
    let index = fonts.len() + 4;
    fonts.push((name.to_owned(), bold, italic, Arc::new(font)));
    Some(index)
}

/// Native system-font discovery is unavailable on the web.
#[cfg(target_arch = "wasm32")]
fn load_named(_: &str, _: bool, _: bool) -> Option<UnicodeFont> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
type CatalogFace = (Vec<String>, bool, bool, std::path::PathBuf);

/// Load the first eligible family/style match in directory priority order.
#[cfg(not(target_arch = "wasm32"))]
fn load_named(name: &str, bold: bool, italic: bool) -> Option<UnicodeFont> {
    static CATALOG: OnceLock<Vec<CatalogFace>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| scan_fonts(font_dirs()));
    load_catalog_match(catalog, name, bold, italic)
}

/// A stale or unreadable catalog entry must not hide a later eligible match.
#[cfg(not(target_arch = "wasm32"))]
fn load_catalog_match(catalog: &[CatalogFace], name: &str, bold: bool, italic: bool) -> Option<UnicodeFont> {
    for (names, b, i, path) in catalog {
        if *b == bold && *i == italic && names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            let Some(bytes) = read_font(path) else { continue };
            let Ok(face) = ttf_parser::Face::parse(&bytes, 0) else { continue };
            if embeddable(&face) {
                return Some(UnicodeFont::new(bytes));
            }
        }
    }
    None
}

/// Scan bounded directory trees, keeping earlier roots ahead of later roots.
#[cfg(not(target_arch = "wasm32"))]
fn scan_fonts(dirs: Vec<std::path::PathBuf>) -> Vec<CatalogFace> {
    // The pending list is a stack, so push roots in reverse priority order.
    let mut pending = dirs.into_iter().rev().map(|p| (p, 0)).collect::<Vec<_>>();
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
}

/// Reject oversized font files before allocating their contents.
#[cfg(not(target_arch = "wasm32"))]
fn read_font(path: &std::path::Path) -> Option<Vec<u8>> {
    if !std::fs::metadata(path).is_ok_and(|m| m.len() <= 16 * 1024 * 1024) {
        return None;
    }
    std::fs::read(path).ok()
}

/// Require TrueType outlines and permission to embed them in printable documents.
#[cfg(not(target_arch = "wasm32"))]
fn embeddable(face: &ttf_parser::Face<'_>) -> bool {
    face.tables().glyf.is_some()
        && face.is_outline_embedding_allowed()
        && matches!(
            face.permissions(),
            Some(ttf_parser::Permissions::Installable | ttf_parser::Permissions::PreviewAndPrint | ttf_parser::Permissions::Editable)
        )
}

/// Ordered font roots, with the explicit override ahead of system and user directories.
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

/// Web exports keep the existing standard-font/CID path.
#[cfg(target_arch = "wasm32")]
fn load() -> [Option<Arc<UnicodeFont>>; 4] {
    [None, None, None, None]
}

/// Find a permitted Unicode fallback for each weight/slant combination.
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
                    return Some(Arc::new(UnicodeFont::new(bytes)));
                }
            }
        }
        None
    })
}

impl UnicodeFont {
    /// Keep parsed metrics separate from the immutable font bytes.
    #[cfg(any(test, not(target_arch = "wasm32")))]
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, glyphs: Mutex::new(BTreeMap::new()) }
    }

    /// Parse a borrowed view without copying the font data.
    fn face(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.bytes, 0).ok()
    }

    /// Cache present and absent glyphs; widths use the PDF's 1000-unit em scale.
    pub(super) fn glyph(&self, c: char) -> Option<(u16, u16)> {
        if let Ok(cache) = self.glyphs.lock()
            && let Some(value) = cache.get(&c)
        {
            return *value;
        }
        let value = self.uncached_glyph(c);
        if let Ok(mut cache) = self.glyphs.lock()
            && cache.len() < GLYPH_CACHE_LIMIT
        {
            cache.insert(c, value);
        }
        value
    }

    /// Read a glyph and its horizontal advance when it is not already cached.
    fn uncached_glyph(&self, c: char) -> Option<(u16, u16)> {
        let face = self.face()?;
        let glyph = face.glyph_index(c)?;
        let advance = u32::from(face.glyph_hor_advance(glyph)?);
        let width = (advance * 1000 / u32::from(face.units_per_em())).min(u32::from(u16::MAX)) as u16;
        Some((glyph.0, width))
    }

    /// Seven consecutive objects: Type0, descendant, descriptor, file, widths, ToUnicode, CID-to-glyph map.
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
        let gid_map = id + 6;
        let mut w = String::from("[");
        let mut gids = vec![0u8; (usize::from(glyphs.keys().next_back().copied().unwrap_or(0)) + 1) * 2];
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /GridCraftUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries: Vec<_> = glyphs.iter().collect();
        for chunk in entries.chunks(100) {
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for &(code, c) in chunk {
                let (gid, width) = self.glyph(*c).unwrap_or((0, 0));
                let _ = write!(w, "{code} [{width}] ");
                let offset = usize::from(*code) * 2;
                gids[offset..offset + 2].copy_from_slice(&gid.to_be_bytes());
                let mut utf16 = [0u16; 2];
                let encoded = c.encode_utf16(&mut utf16);
                let _ = write!(cmap, "<{code:04X}> <");
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
            (cid, format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /CIDToGIDMap {gid_map} 0 R /W {widths} 0 R >>").into_bytes()),
            (desc, format!("<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",metric(bbox.x_min),metric(bbox.y_min),metric(bbox.x_max),metric(bbox.y_max),face.italic_angle(),metric(face.ascender()),metric(face.descender()),metric(face.capital_height().unwrap_or(face.ascender()))).into_bytes()),
            (file, stream(&self.bytes, &format!(" /Length1 {}",self.bytes.len()))),
            (widths, w.into_bytes()),
            (unicode, stream(cmap.as_bytes(), "")),
            (gid_map, stream(&gids, "")),
        ]
    }
}

/// Serialize a PDF stream with byte-accurate length and optional dictionary entries.
fn stream(bytes: &[u8], extra: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {}{extra} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

#[cfg(test)]
/// Build a small original metrics-only face: space, NBSP and an astral character share glyph 1.
/// No installed fonts or redistributed font fixtures are needed for the mapping regressions.
pub(super) fn test_font(bold: bool, italic: bool) -> super::Font {
    let mut head = vec![0; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut hhea = vec![0; 36];
    hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
    let maxp = vec![0, 0, 0x50, 0, 0, 2];
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
    cmap.extend_from_slice(&[0, 12, 0, 0]);
    for n in [52u32, 0, 3, 32, 32, 1, 160, 160, 1, 0x1f600, 0x1f600, 1] {
        cmap.extend_from_slice(&n.to_be_bytes());
    }
    let tables = [(b"cmap", cmap), (b"head", head), (b"hhea", hhea), (b"hmtx", vec![0, 0, 0, 0, 1, 244, 0, 0]), (b"maxp", maxp)];
    let mut bytes = vec![0; 12 + tables.len() * 16];
    bytes[..4].copy_from_slice(&0x00010000u32.to_be_bytes());
    bytes[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        let start = 12 + i * 16;
        bytes[start..start + 4].copy_from_slice(tag);
        let offset = bytes.len() as u32;
        bytes[start + 8..start + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[start + 12..start + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend(data);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
    }
    let face = Arc::new(UnicodeFont::new(bytes));
    assert_eq!(face.glyph(' '), Some((1, 500)));
    assert_eq!(face.glyph('\u{a0}'), Some((1, 500)));
    assert_eq!(face.glyph('\u{1f600}'), Some((1, 500)));
    let mut fonts = NAMED.lock().unwrap();
    let index = fonts.len() + 4;
    assert!(index < FONT_LIMIT);
    fonts.push((format!("GridCraft synthetic {index}"), bold, italic, face));
    super::Font::Embedded(index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Failed lookups are reused across case variants, bounded, and isolated by style.
    #[test]
    fn missing_names_are_cached_without_holding_locks() {
        let named = Mutex::new(Vec::new());
        let missing = Mutex::new(BTreeSet::new());
        let calls = std::cell::Cell::new(0);
        let load = || {
            assert!(named.try_lock().is_ok());
            assert!(missing.try_lock().is_ok());
            calls.set(calls.get() + 1);
            None
        };
        assert_eq!(resolve_named(&named, &missing, "Absent", false, false, load), None);
        assert_eq!(resolve_named(&named, &missing, "ABSENT", false, false, load), None);
        assert_eq!(calls.get(), 1);
        assert_eq!(resolve_named(&named, &missing, "Absent", true, false, load), None);
        assert_eq!(calls.get(), 2);
        for i in 0..MISSING_CACHE_LIMIT + 10 {
            resolve_named(&named, &missing, &format!("Absent{i}"), false, false, load);
        }
        assert_eq!(missing.lock().unwrap().len(), MISSING_CACHE_LIMIT);
        assert!(named.lock().unwrap().is_empty());
    }

    /// A concurrent resolver wins without allocating a duplicate font slot.
    #[test]
    fn named_resolution_rechecks_after_loading() {
        let named = Mutex::new(Vec::new());
        let missing = Mutex::new(BTreeSet::new());
        let result = resolve_named(&named, &missing, "Concurrent", false, false, || {
            named.lock().unwrap().push(("Concurrent".into(), false, false, Arc::new(UnicodeFont::new(Vec::new()))));
            Some(UnicodeFont::new(Vec::new()))
        });
        assert_eq!(result, Some(4));
        assert_eq!(named.lock().unwrap().len(), 1);
    }

    /// Missing and malformed files are skipped before the next matching font.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn catalog_skips_unreadable_and_invalid_matches() -> std::io::Result<()> {
        let Some(installed) = font(0) else { return Ok(()) };
        let root = std::env::temp_dir().join(format!("gridcraft-font-load-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        let invalid = root.join("invalid.ttf");
        let valid = root.join("valid.ttf");
        std::fs::write(&invalid, b"invalid font")?;
        std::fs::write(&valid, &installed.bytes)?;
        let catalog = [root.join("missing.ttf"), invalid.clone(), valid.clone()].map(|path| (vec!["Test Family".into()], false, false, path));
        let loaded = load_catalog_match(&catalog, "Test Family", false, false).unwrap();
        assert_eq!(loaded.bytes, installed.bytes);
        std::fs::remove_file(invalid)?;
        std::fs::remove_file(valid)?;
        std::fs::remove_dir(root)?;
        Ok(())
    }

    /// Both successful metrics and missing glyphs are cached without unbounded growth.
    #[test]
    fn glyph_cache_reuses_metrics_and_caps_missing_entries() {
        let Some(installed) = font(0) else { return };
        let mut local = UnicodeFont::new(installed.bytes.clone());
        let expected = local.uncached_glyph('A');
        assert!(expected.is_some());
        assert_eq!(local.glyph('A'), expected);
        assert_eq!(local.glyph('\u{10ffff}'), None);
        assert_eq!(local.glyphs.lock().unwrap().get(&'\u{10ffff}'), Some(&None));
        // Destroy only this private test copy: a cached hit must not parse it again.
        local.bytes.clear();
        assert_eq!(local.glyph('A'), expected);
        for c in (0..5000).filter_map(char::from_u32) {
            assert_eq!(local.glyph(c), if c == 'A' { expected } else { None });
        }
        assert_eq!(local.glyphs.lock().unwrap().len(), GLYPH_CACHE_LIMIT);
    }

    /// Earlier directory roots retain priority even when their font is nested.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn catalog_visits_override_before_other_roots() -> std::io::Result<()> {
        let Some(installed) = font(0) else { return Ok(()) };
        let root = std::env::temp_dir().join(format!("gridcraft-font-priority-{}", std::process::id()));
        let first = root.join("override").join("nested");
        let second = root.join("system");
        std::fs::create_dir_all(&first)?;
        std::fs::create_dir_all(&second)?;
        let preferred = first.join("face.ttf");
        let other = second.join("face.ttf");
        std::fs::write(&preferred, &installed.bytes)?;
        std::fs::write(&other, &installed.bytes)?;
        let catalog = scan_fonts(vec![root.join("override"), second]);
        let paths: Vec<_> = catalog.iter().map(|e| &e.3).collect();
        assert_eq!(paths, vec![&preferred, &other]);
        std::fs::remove_file(preferred)?;
        std::fs::remove_file(other)?;
        std::fs::remove_dir(first)?;
        std::fs::remove_dir(root.join("override"))?;
        std::fs::remove_dir(root.join("system"))?;
        std::fs::remove_dir(root)?;
        Ok(())
    }
}
