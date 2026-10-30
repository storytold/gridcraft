//! Embedded TrueType fallback fonts for scripts the standard 14 fonts lack (Thai first of all).
//!
//! The host registers a font file once ([`register_fallback_font`]); text runs the font covers
//! are then shaped with harfrust (so Thai vowels and tone marks stack on their consonants) and
//! written as a `CIDFontType2` with `Identity-H`, a `/W` width array and a `ToUnicode` CMap, so
//! the text in the PDF can be searched and copied. The whole font file is embedded (no subset).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, RwLock};

use harfrust::{BufferClusterLevel, FontRef, ShapeOptions, ShaperData, UnicodeBuffer};
use read_fonts::TableProvider;
use read_fonts::types::{GlyphId, NameId};

/// Largest font file accepted.
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
/// Longest run shaped at once, in bytes; longer runs are cut (a cell holds at most 32,767 characters).
const MAX_SHAPE_BYTES: usize = 256 * 1024;
/// `OS/2.fsType` bits: restricted licence embedding, and bitmap embedding only.
const FS_RESTRICTED: u16 = 0x0002;
const FS_BITMAP_ONLY: u16 = 0x0200;

/// Why a font file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    TooLarge,
    Malformed(String),
    /// The font's licence bits (`OS/2.fsType`) forbid embedding it in a document.
    EmbeddingNotAllowed,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::TooLarge => write!(f, "the font file is larger than {} MB", MAX_FONT_BYTES / (1024 * 1024)),
            FontError::Malformed(e) => write!(f, "not a usable TrueType/OpenType font: {e}"),
            FontError::EmbeddingNotAllowed => f.write_str("the font's licence does not allow embedding it in a PDF"),
        }
    }
}

impl std::error::Error for FontError {}

/// One shaped glyph, in font units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyph {
    pub gid: u16,
    /// Byte offset of the glyph's cluster in the shaped text.
    pub cluster: usize,
    pub x_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

/// A registered fallback font.
pub(crate) struct Face {
    bytes: Vec<u8>,
    data: ShaperData,
    upem: u16,
    pub name: String,
    ascent: i16,
    descent: i16,
    cap_height: i16,
    bbox: [i16; 4],
}

impl Face {
    fn parse(bytes: Vec<u8>, fallback_name: &str) -> Result<Face, FontError> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(FontError::TooLarge);
        }
        let bad = |e: read_fonts::ReadError| FontError::Malformed(e.to_string());
        let font = FontRef::new(&bytes).map_err(bad)?;
        let head = font.head().map_err(bad)?;
        let hhea = font.hhea().map_err(bad)?;
        font.hmtx().map_err(bad)?;
        font.cmap().map_err(bad)?;
        let fs_type = font.os2().map(|o| o.fs_type()).unwrap_or(0);
        if fs_type & (FS_RESTRICTED | FS_BITMAP_ONLY) != 0 {
            return Err(FontError::EmbeddingNotAllowed);
        }
        let upem = head.units_per_em();
        if !(16..=16384).contains(&upem) {
            return Err(FontError::Malformed(format!("unitsPerEm {upem}")));
        }
        let ascent = hhea.ascender().to_i16();
        let descent = hhea.descender().to_i16();
        let cap_height = font.os2().ok().and_then(|o| o.s_cap_height()).filter(|h| *h > 0).unwrap_or(ascent);
        let bbox = [head.x_min(), head.y_min(), head.x_max(), head.y_max()];
        let name = postscript_name(&font).unwrap_or_else(|| fallback_name.to_string());
        let data = ShaperData::new(&font);
        let face = Face { bytes: Vec::new(), data, upem, name, ascent, descent, cap_height, bbox };
        Ok(Face { bytes, ..face })
    }

    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::new(&self.bytes).ok()
    }

    /// Whether the font has a glyph for `c`.
    pub fn covers(&self, c: char) -> bool {
        self.font().and_then(|f| f.cmap().ok()).and_then(|m| m.map_codepoint(c)).is_some_and(|g| g.to_u32() != 0)
    }

    /// The glyph's advance in 1/1000 em, as `/W` gives it.
    pub fn advance_1000(&self, gid: u16) -> i64 {
        let units = self.font().and_then(|f| f.hmtx().ok()).and_then(|h| h.advance(GlyphId::new(u32::from(gid)))).unwrap_or(0);
        self.to_1000(i64::from(units))
    }

    /// Font units → 1/1000 em, rounded.
    pub fn to_1000(&self, units: i64) -> i64 {
        let upem = i64::from(self.upem.max(1));
        (units.saturating_mul(1000) + upem / 2 * units.signum()) / upem
    }

    /// Shapes `text` (direction, script and language guessed from the text). An empty result
    /// means the font could not be read.
    pub fn shape(&self, text: &str) -> Vec<Glyph> {
        let Some(font) = self.font() else { return Vec::new() };
        let text = cut(text, MAX_SHAPE_BYTES);
        let shaper = self.data.shaper(&font).build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        // One cluster per character, not per syllable, so each Thai vowel and tone mark glyph
        // maps back to its own character in the ToUnicode CMap.
        buffer.set_cluster_level(BufferClusterLevel::Characters);
        let out = shaper.shape(buffer, ShapeOptions::new());
        out.glyph_infos()
            .iter()
            .zip(out.glyph_positions())
            .map(|(i, p)| Glyph {
                gid: u16::try_from(i.glyph_id).unwrap_or(0),
                cluster: i.cluster as usize,
                x_advance: p.x_advance,
                x_offset: p.x_offset,
                y_offset: p.y_offset,
            })
            .collect()
    }

    /// Width of shaped `text` in 1/1000 em.
    pub fn width_1000(&self, text: &str) -> u64 {
        let units: i64 = self.shape(text).iter().map(|g| i64::from(g.x_advance)).sum();
        u64::try_from(self.to_1000(units)).unwrap_or(0)
    }

    /// `Ascent`, `Descent`, `CapHeight` and `FontBBox` in 1/1000 em, for the font descriptor.
    pub fn metrics_1000(&self) -> (i64, i64, i64, [i64; 4]) {
        let s = |v: i16| self.to_1000(i64::from(v));
        (s(self.ascent), s(self.descent), s(self.cap_height), self.bbox.map(s))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The longest prefix of `text` of at most `max` bytes, cut on a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let end = (0..=max).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
    text.get(..end).unwrap_or_default()
}

/// The font's PostScript name (name ID 6), reduced to characters a PDF name may hold.
fn postscript_name(font: &FontRef) -> Option<String> {
    let name = font.name().ok()?;
    let record = name.name_record().iter().find(|r| r.name_id() == NameId::POSTSCRIPT_NAME)?;
    let s: String = record.string(name.string_data()).ok()?.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(63).collect();
    (!s.is_empty()).then_some(s)
}

/// Registered faces: regular, then bold.
static FACES: RwLock<[Option<Arc<Face>>; 2]> = RwLock::new([None, None]);

/// Registers the TrueType/OpenType font PDF export uses for characters the standard fonts lack
/// (Thai, for example). `bold` registers the face for bold text; without one, bold text uses the
/// regular face drawn bolder. Returns the font's name. Replaces an earlier registration.
pub fn register_fallback_font(bytes: Vec<u8>, bold: bool) -> Result<String, FontError> {
    let fallback = if bold { "GridCraftFallback-Bold" } else { "GridCraftFallback" };
    let face = Face::parse(bytes, fallback)?;
    // A malformed table can still trip the shaper; find out now rather than during an export.
    let probe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| face.shape("กข้ำ Aé").len()));
    if probe.is_err() {
        return Err(FontError::Malformed("the shaper cannot read this font".into()));
    }
    let name = face.name.clone();
    let mut faces = match FACES.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(slot) = faces.get_mut(usize::from(bold)) {
        *slot = Some(Arc::new(face));
    }
    Ok(name)
}

/// Forgets the registered fallback fonts.
pub fn clear_fallback_fonts() {
    let mut faces = match FACES.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *faces = [None, None];
}

/// Whether a fallback font is registered.
pub fn has_fallback_font() -> bool {
    face(false).is_some()
}

/// The face for regular or bold text, and whether bold must be drawn by stroking the regular one.
pub(crate) fn face(bold: bool) -> Option<(Arc<Face>, bool)> {
    let faces = match FACES.read() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let [regular, bold_face] = &*faces;
    match (bold, bold_face, regular) {
        (true, Some(b), _) => Some((b.clone(), false)),
        (true, None, Some(r)) => Some((r.clone(), true)),
        (false, _, Some(r)) => Some((r.clone(), false)),
        _ => None,
    }
}

/// The glyphs a document drew with one face: glyph id → (width in 1/1000 em, the text it shows).
#[derive(Default)]
pub(crate) struct Usage {
    pub glyphs: BTreeMap<u16, (i64, String)>,
}

impl Usage {
    /// Records the glyphs of shaped `text`. The first glyph of a cluster stands for the
    /// cluster's text (a one-glyph cluster wins over a longer one for the same glyph).
    pub fn record(&mut self, face: &Face, text: &str, glyphs: &[Glyph]) {
        let mut starts: Vec<usize> = glyphs.iter().map(|g| g.cluster).collect();
        starts.sort_unstable();
        starts.dedup();
        let mut seen = std::collections::HashSet::new();
        for g in glyphs {
            let entry = self.glyphs.entry(g.gid).or_insert_with(|| (face.advance_1000(g.gid), String::new()));
            if !seen.insert(g.cluster) {
                continue;
            }
            let end = starts.iter().find(|s| **s > g.cluster).copied().unwrap_or(text.len());
            let Some(chars) = text.get(g.cluster..end) else { continue };
            if entry.1.is_empty() || chars.chars().count() < entry.1.chars().count() {
                entry.1 = chars.to_string();
            }
        }
    }

    pub fn merge(&mut self, other: &Usage) {
        for (gid, (w, s)) in &other.glyphs {
            let e = self.glyphs.entry(*gid).or_insert_with(|| (*w, String::new()));
            if e.1.is_empty() || (!s.is_empty() && s.chars().count() < e.1.chars().count()) {
                e.1 = s.clone();
            }
        }
    }

    /// The `/W` array: `[gid [w] gid [w] …]`.
    pub fn widths(&self) -> String {
        let mut s = String::from("[");
        for (gid, (w, _)) in &self.glyphs {
            let _ = write!(s, "{gid} [{w}] ");
        }
        s.push(']');
        s
    }

    /// The `ToUnicode` CMap.
    pub fn to_unicode(&self) -> String {
        let mut s = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let mapped: Vec<(&u16, &String)> = self.glyphs.iter().filter(|(_, (_, t))| !t.is_empty()).map(|(g, (_, t))| (g, t)).collect();
        // A bfchar block holds at most 100 entries.
        for block in mapped.chunks(100) {
            let _ = writeln!(s, "{} beginbfchar", block.len());
            for (gid, text) in block {
                let _ = write!(s, "<{gid:04X}> <");
                for unit in text.encode_utf16() {
                    let _ = write!(s, "{unit:04X}");
                }
                s.push_str(">\n");
            }
            s.push_str("endbfchar\n");
        }
        s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
        s
    }
}
