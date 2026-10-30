//! Embedded TrueType fonts for text the standard fonts cannot encode (Cyrillic, Greek, …).
//!
//! The application installs a TrueType font per standard face (usually found among the
//! system fonts: Arial, Liberation, DejaVu). Characters without a WinAnsiEncoding byte are then
//! set in that font, embedded as a subset (`CIDFontType2`, `Identity-H`, with a `ToUnicode`
//! map so the text can be searched and copied). Characters the installed font lacks fall back
//! to the non-embedded CID font, as before.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, RwLock};

use crate::Font;

/// Largest font file accepted.
pub const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;

/// Why a font cannot be embedded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// Larger than the 64 MiB this writer accepts.
    TooLarge,
    /// Not a readable OpenType/TrueType font (or the collection index is out of range).
    Unreadable,
    /// The font has CFF outlines; only TrueType (`glyf`) outlines are embedded.
    NotTrueType,
    /// The font's licence bits forbid embedding (`OS/2.fsType` restricted).
    EmbeddingRestricted,
    /// The font maps no characters.
    NoCharacters,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FontError::TooLarge => "font file is too large",
            FontError::Unreadable => "not a readable TrueType font",
            FontError::NotTrueType => "font has no TrueType outlines",
            FontError::EmbeddingRestricted => "font licence does not allow embedding",
            FontError::NoCharacters => "font maps no characters",
        })
    }
}

impl std::error::Error for FontError {}

/// A parsed TrueType font ready to be embedded.
pub struct EmbeddedFont {
    data: Vec<u8>,
    index: u32,
    /// PostScript name, reduced to characters a PDF name may hold unescaped.
    name: String,
    glyphs: HashMap<char, u16>,
    /// Advance width per glyph id, in 1/1000 em.
    widths: Vec<u16>,
    ascent: i32,
    descent: i32,
    cap_height: i32,
    bbox: [i32; 4],
    italic_angle: f32,
    fixed_pitch: bool,
    serif: bool,
}

impl std::fmt::Debug for EmbeddedFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedFont").field("name", &self.name).field("glyphs", &self.glyphs.len()).finish()
    }
}

impl EmbeddedFont {
    /// Parses font `index` of `data` (a `.ttf`, or a `.ttc` collection).
    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<EmbeddedFont, FontError> {
        use skrifa::raw::TableProvider;
        use skrifa::{MetadataProvider, Tag};
        if data.len() > MAX_FONT_BYTES {
            return Err(FontError::TooLarge);
        }
        let font = skrifa::FontRef::from_index(&data, index).map_err(|_| FontError::Unreadable)?;
        if font.table_data(Tag::new(b"glyf")).is_none() || font.table_data(Tag::new(b"loca")).is_none() {
            return Err(FontError::NotTrueType);
        }
        let head = font.head().map_err(|_| FontError::Unreadable)?;
        let upem = f32::from(head.units_per_em());
        if !(16.0..=16384.0).contains(&upem) {
            return Err(FontError::Unreadable);
        }
        let os2 = font.os2().ok();
        // fsType bit 1 alone is "restricted licence embedding": the font must not be embedded.
        // Bits 2 and 3 (preview/print, editable) override it. Bit 8 forbids subsetting, which
        // this writer always does, and bit 9 allows embedding bitmaps only, which it never writes.
        if os2.as_ref().is_some_and(|t| t.fs_type() & 0x000E == 0x0002 || t.fs_type() & 0x0300 != 0) {
            return Err(FontError::EmbeddingRestricted);
        }
        let scale = |v: f32| -> i32 {
            let s = v * 1000.0 / upem;
            if s.is_finite() { s.round().clamp(-100_000.0, 100_000.0) as i32 } else { 0 }
        };
        let glyph_count = font.maxp().map(|m| usize::from(m.num_glyphs())).unwrap_or(0);
        let metrics = font.glyph_metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        let widths: Vec<u16> = (0..glyph_count)
            .map(|g| {
                let gid = skrifa::GlyphId::new(g as u32);
                metrics.advance_width(gid).map_or(0, |w| scale(w).clamp(0, i32::from(u16::MAX)) as u16)
            })
            .collect();
        let mut glyphs = HashMap::new();
        for (code, gid) in font.charmap().mappings() {
            let (Some(c), Ok(g)) = (char::from_u32(code), u16::try_from(gid.to_u32())) else { continue };
            if g != 0 && usize::from(g) < widths.len() {
                glyphs.entry(c).or_insert(g);
            }
        }
        if glyphs.is_empty() {
            return Err(FontError::NoCharacters);
        }
        let hhea = font.hhea().ok();
        let ascent = os2.as_ref().map(|t| t.s_typo_ascender()).or(hhea.as_ref().map(|h| h.ascender().to_i16())).unwrap_or(800);
        let descent = os2.as_ref().map(|t| t.s_typo_descender()).or(hhea.as_ref().map(|h| h.descender().to_i16())).unwrap_or(-200);
        let cap_height = os2.as_ref().and_then(|t| t.s_cap_height()).unwrap_or(ascent);
        let post = font.post().ok();
        let italic_angle = post.as_ref().map_or(0.0, |p| p.italic_angle().to_f32());
        let fixed_pitch = post.as_ref().is_some_and(|p| p.is_fixed_pitch() != 0);
        // OS/2 sFamilyClass 1–7 are the serif classes.
        let serif = os2.as_ref().is_some_and(|t| (1..=7).contains(&(t.s_family_class() >> 8)));
        let raw_name = font
            .localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
            .english_or_first()
            .map(|s| s.chars().collect::<String>())
            .unwrap_or_default();
        let mut name: String = raw_name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).take(63).collect();
        if name.is_empty() {
            name.push_str("EmbeddedFont");
        }
        Ok(EmbeddedFont {
            name,
            glyphs,
            widths,
            ascent: scale(f32::from(ascent)),
            descent: scale(f32::from(descent)),
            cap_height: scale(f32::from(cap_height)),
            bbox: [scale(f32::from(head.x_min())), scale(f32::from(head.y_min())), scale(f32::from(head.x_max())), scale(f32::from(head.y_max()))],
            italic_angle: if italic_angle.is_finite() { italic_angle } else { 0.0 },
            fixed_pitch,
            serif,
            data,
            index,
        })
    }

    /// The font's PostScript name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The glyph for `c`, when the font has one.
    pub fn glyph(&self, c: char) -> Option<u16> {
        self.glyphs.get(&c).copied()
    }

    /// Advance width of glyph `gid` in 1/1000 em.
    pub(crate) fn width(&self, gid: u16) -> u16 {
        self.widths.get(usize::from(gid)).copied().unwrap_or(0)
    }

    /// The PDF objects for this font as used on the pages: `glyphs` maps each used glyph to the
    /// character it was drawn for. Returns `(id, body)` pairs numbered from `first` (the Type0
    /// font itself, which pages reference), always [`OBJECTS_PER_FONT`] of them.
    pub(crate) fn objects(&self, first: u32, glyphs: &BTreeMap<u16, char>) -> Vec<(u32, Vec<u8>)> {
        let [font_id, cid_id, desc_id, file_id, map_id, unicode_id] = [first, first + 1, first + 2, first + 3, first + 4, first + 5];
        // The content streams use the original glyph ids as CIDs; the subset renumbers glyphs,
        // so a CIDToGIDMap translates. If subsetting fails, the whole font is embedded instead.
        let used: Vec<u16> = glyphs.keys().copied().collect();
        let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&used);
        let (file, cid_to_gid) = match subsetter::subset(&self.data, self.index, &remapper) {
            Ok(subset) => {
                let max = used.last().copied().unwrap_or(0);
                let mut map = vec![0u8; (usize::from(max) + 1) * 2];
                for &g in &used {
                    let new = remapper.get(g).unwrap_or(0);
                    let at = usize::from(g) * 2;
                    if let Some(slot) = map.get_mut(at..at + 2) {
                        slot.copy_from_slice(&new.to_be_bytes());
                    }
                }
                (subset, Some(map))
            }
            Err(_) => (self.data.clone(), None),
        };
        let tag = subset_tag(&used);
        let base = format!("{tag}+{}", self.name);
        let mut widths = String::new();
        for &g in &used {
            let _ = write!(widths, "{g} [{}] ", self.width(g));
        }
        let map_ref = if cid_to_gid.is_some() { format!("{map_id} 0 R") } else { "/Identity".to_string() };
        let mut flags = 32u32; // nonsymbolic
        if self.fixed_pitch {
            flags |= 1;
        }
        if self.serif {
            flags |= 2;
        }
        if self.italic_angle != 0.0 {
            flags |= 64;
        }
        let [x0, y0, x1, y1] = self.bbox;
        vec![
            (
                font_id,
                format!("<< /Type /Font /Subtype /Type0 /BaseFont /{base} /Encoding /Identity-H /DescendantFonts [{cid_id} 0 R] /ToUnicode {unicode_id} 0 R >>")
                    .into_bytes(),
            ),
            (
                cid_id,
                format!(
                    "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{base} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc_id} 0 R /DW 1000 /W [{widths}] /CIDToGIDMap {map_ref} >>"
                )
                .into_bytes(),
            ),
            (
                desc_id,
                format!(
                    "<< /Type /FontDescriptor /FontName /{base} /Flags {flags} /FontBBox [{x0} {y0} {x1} {y1}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file_id} 0 R >>",
                    crate::n(self.italic_angle),
                    self.ascent,
                    self.descent,
                    self.cap_height
                )
                .into_bytes(),
            ),
            (file_id, stream(&format!("/Length1 {}", file.len()), &file)),
            (map_id, stream("", cid_to_gid.as_deref().unwrap_or_default())),
            (unicode_id, stream("", to_unicode(glyphs).as_bytes())),
        ]
    }
}

/// Objects each embedded font takes (see [`EmbeddedFont::objects`]).
pub(crate) const OBJECTS_PER_FONT: u32 = 6;

fn stream(extra: &str, data: &[u8]) -> Vec<u8> {
    let mut o = format!("<< /Length {}{}{extra} >>\nstream\n", data.len(), if extra.is_empty() { "" } else { " " }).into_bytes();
    o.extend_from_slice(data);
    o.extend_from_slice(b"\nendstream");
    o
}

/// Six capital letters naming the subset, derived from the glyphs it holds (ISO 32000 9.6.4).
fn subset_tag(glyphs: &[u16]) -> String {
    // FNV-1a over the glyph ids.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for g in glyphs {
        for b in g.to_be_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    (0..6)
        .map(|i| {
            let letter = (h >> (i * 5)) % 26;
            char::from(b'A' + letter as u8)
        })
        .collect()
}

/// A `ToUnicode` CMap from glyph ids (the 2-byte codes of `Identity-H`) to characters.
fn to_unicode(glyphs: &BTreeMap<u16, char>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = glyphs.iter().collect();
    // At most 100 entries per bfchar block.
    for chunk in entries.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, c) in chunk {
            let mut units = [0u16; 2];
            let hex: String = c.encode_utf16(&mut units).iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(s, "<{g:04X}> <{hex}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

/// The fonts installed per standard face, indexed like [`Font::ALL`].
static INSTALLED: RwLock<[Option<Arc<EmbeddedFont>>; Font::ALL.len()]> = RwLock::new([const { None }; Font::ALL.len()]);

/// A snapshot of the installed fonts, so a whole string is measured and drawn with one set.
pub(crate) type Installed = [Option<Arc<EmbeddedFont>>; Font::ALL.len()];

pub(crate) fn installed() -> Installed {
    with_installed(Installed::clone)
}

/// Runs `f` on the installed fonts without copying them (measuring runs for every cell).
pub(crate) fn with_installed<R>(f: impl FnOnce(&Installed) -> R) -> R {
    // A poisoned lock still holds a consistent array of `Arc`s: use it.
    f(&INSTALLED.read().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Uses `font` for the characters `face` cannot encode. Applies to every document from now on.
pub fn install_font(face: Font, font: EmbeddedFont) {
    let mut fonts = INSTALLED.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(slot) = fonts.get_mut(face.index()) {
        *slot = Some(Arc::new(font));
    }
}

/// Removes every installed font: non-WinAnsi text goes back to the non-embedded CID font.
pub fn clear_fonts() {
    let mut fonts = INSTALLED.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    *fonts = [const { None }; Font::ALL.len()];
}

/// Whether any font is installed for `face`'s family or the sans regular face.
pub fn has_font(face: Font) -> bool {
    with_installed(|fonts| resolve(fonts, face, None).is_some())
}

/// Whether an installed font draws `c` for `face` (the face, its family's regular face or the
/// sans regular face).
pub fn can_draw(face: Font, c: char) -> bool {
    with_installed(|fonts| resolve(fonts, face, Some(c)).is_some())
}

/// The installed font that draws `c` for `face`: the face itself, then the family's regular
/// face, then the sans regular face. `None` (when `c` is `Some`) also when none has the glyph.
pub(crate) fn resolve(fonts: &Installed, face: Font, c: Option<char>) -> Option<(usize, u16)> {
    let regular = Font::styled(face.family(), false, false);
    let order = [face.index(), regular.index(), Font::Helvetica.index()];
    for (k, i) in order.iter().enumerate() {
        // The face may be its family's regular face, or Helvetica itself.
        if order.iter().take(k).any(|before| before == i) {
            continue;
        }
        let Some(Some(font)) = fonts.get(*i) else { continue };
        match c {
            None => return Some((*i, 0)),
            Some(c) => {
                if let Some(g) = font.glyph(c) {
                    return Some((*i, g));
                }
            }
        }
    }
    None
}
