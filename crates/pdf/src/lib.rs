//! A tiny, self-contained PDF 1.7 writer for GridCraft's print and PDF export.
//!
//! - The 14 standard fonts (no embedding) with WinAnsiEncoding, plus approximate width tables
//!   ([`text_width`]) so callers can measure, align and wrap text.
//! - Text WinAnsiEncoding can't carry (Greek, Cyrillic, …) is set in an embedded TrueType font
//!   (an explicit CID-to-glyph map and ToUnicode map keep it searchable and copyable) when the app has
//!   registered one with [`set_unicode_font_loader`]; otherwise it uses the non-embedded CID fallback.
//! - Explicit [`TextFonts`] replace all text faces per document, including Latin, and supply
//!   matching advance widths for layout.
//! - Vector drawing: rectangles, lines (solid or dashed), polygons, circles (Bézier), clipping.
//! - RGBA images as RGB image XObjects with a soft mask for alpha (run-length encoded).
//! - Coordinates are points with a **top-left origin** (y grows down); text `y` is the baseline.
//!
//! Streams are written uncompressed (images use the trivial RunLengthDecode filter) so the crate
//! needs no dependencies.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod metrics;
mod system_fonts;
mod truetype;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

pub use system_fonts::{pdf_unicode_fonts, system_cell_font_candidates};
pub use truetype::{MAX_FONT_BYTES, UnicodeFont};

/// Maximum number of explicitly mapped workbook families in one export.
pub const MAX_FONT_FAMILIES: usize = 32;
/// Total bytes of explicit font faces accepted for one export.
pub const MAX_TOTAL_FONT_BYTES: u64 = 256 * 1024 * 1024;
/// Bounds the character maps for each embedded face, including Unicode aliases.
pub const MAX_FONT_CHARACTERS: usize = 65_535;

/// Primary regular/bold and alternate regular/bold faces for text outside WinAnsiEncoding.
pub type UnicodeFonts = [Option<Arc<UnicodeFont>>; 4];

/// Loads primary regular/bold and alternate regular/bold `.ttf` faces. Runs at most once,
/// the first time a PDF needs them after registration.
pub type UnicodeFontLoader = fn() -> [Option<Vec<u8>>; 4];

static SYSTEM_FONTS: SystemFontCache = SystemFontCache::new();

/// Keeps registered faces immutable without caching the absence of a loader.
struct SystemFontCache {
    loader: OnceLock<UnicodeFontLoader>,
    unicode: OnceLock<UnicodeFonts>,
    text: OnceLock<TextFonts>,
    unregistered: OnceLock<TextFonts>,
}

impl SystemFontCache {
    const fn new() -> Self {
        Self { loader: OnceLock::new(), unicode: OnceLock::new(), text: OnceLock::new(), unregistered: OnceLock::new() }
    }

    fn unicode(&self) -> &UnicodeFonts {
        static EMPTY: UnicodeFonts = [None, None, None, None];
        let Some(load) = self.loader.get() else { return &EMPTY };
        self.unicode.get_or_init(|| load().map(|bytes| bytes.and_then(UnicodeFont::parse).map(Arc::new)))
    }

    fn text(&self) -> &TextFonts {
        if self.loader.get().is_none() {
            return self.unregistered.get_or_init(TextFonts::default);
        }
        self.text.get_or_init(|| TextFonts::fallback(self.unicode().clone()))
    }
}

/// Registers how to find the system fonts used for non-Latin text (call once at start-up; the
/// first registration wins). Without a loader, such text uses the non-embedded CID fallback. On wasm
/// there are no system fonts, so apps there simply don't register one. Registration after an
/// earlier lookup takes effect on subsequent lookups; existing documents keep their font set.
pub fn set_unicode_font_loader(loader: UnicodeFontLoader) {
    let _ = SYSTEM_FONTS.loader.set(loader);
}

/// The registered fallback fonts, loaded and parsed once on first use after registration.
/// A lookup before registration returns no faces without preventing later registration.
pub fn default_unicode_fonts() -> &'static UnicodeFonts {
    SYSTEM_FONTS.unicode()
}

/// Whether `text` has characters WinAnsiEncoding can't represent.
fn needs_unicode(text: &str) -> bool {
    text.chars().any(|c| c != '?' && win_ansi(c) == b'?')
}

/// Per-document text faces and measurement policy. Custom faces replace every standard family
/// for this export, including Latin text; fallback faces are used only for non-WinAnsi runs.
#[derive(Clone, Debug)]
pub struct TextFonts {
    faces: Vec<Option<Arc<UnicodeFont>>>,
    custom: bool,
    families: BTreeMap<String, usize>,
}

impl Default for TextFonts {
    fn default() -> Self {
        Self { faces: vec![None; 4], custom: false, families: BTreeMap::new() }
    }
}

impl TextFonts {
    /// The registered Unicode fallbacks, with the standard PDF fonts for Latin.
    pub fn system() -> Self {
        SYSTEM_FONTS.text().clone()
    }

    fn fallback(fonts: UnicodeFonts) -> Self {
        Self { faces: Vec::from(fonts), ..Self::default() }
    }

    /// Explicit regular, bold, italic and bold-italic faces. Omitted styles use regular.
    pub fn custom(
        regular: Arc<UnicodeFont>,
        bold: Option<Arc<UnicodeFont>>,
        italic: Option<Arc<UnicodeFont>>,
        bold_italic: Option<Arc<UnicodeFont>>,
    ) -> Self {
        Self { faces: vec![Some(regular), bold, italic, bold_italic], custom: true, ..Self::default() }
    }

    /// Registers faces for a workbook family, matched case-insensitively after trimming.
    /// Named mappings take precedence over the global custom face. Missing styles use regular.
    pub fn add_family(
        &mut self,
        name: &str,
        regular: Arc<UnicodeFont>,
        bold: Option<Arc<UnicodeFont>>,
        italic: Option<Arc<UnicodeFont>>,
        bold_italic: Option<Arc<UnicodeFont>>,
    ) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().take(256).count() > 255 {
            return Err("font family names must contain 1 to 255 characters");
        }
        if self.families.len() >= MAX_FONT_FAMILIES {
            return Err("at most 32 PDF font families are supported per export");
        }
        let key = name.to_lowercase();
        if self.families.contains_key(&key) {
            return Err("duplicate PDF font family name (case-insensitive)");
        }
        let faces = [Some(regular), bold, italic, bold_italic];
        let total: u64 = self.faces.iter().chain(faces.iter()).flatten().map(|f| f.data.len() as u64).sum();
        if total > MAX_TOTAL_FONT_BYTES {
            return Err("PDF font faces exceed the 256 MiB export limit");
        }
        self.families.insert(key, self.faces.len());
        self.faces.extend(faces);
        Ok(())
    }

    /// Resolves the original workbook family before reducing it to a standard PDF family.
    /// The returned handle belongs to this font set and the pages created from it.
    pub fn font(&self, name: &str, bold: bool, italic: bool) -> Font {
        let name = name.trim();
        if !self.families.is_empty()
            && name.len() <= 1024
            && let Some(base) = self.families.get(&name.to_lowercase())
        {
            let want = base + usize::from(bold) + 2 * usize::from(italic);
            let slot = if self.faces.get(want).is_some_and(Option::is_some) { want } else { *base };
            return Font::Embedded(slot);
        }
        Font::styled(Font::family_for(name), bold, italic)
    }

    fn slot(&self, font: Font, text: &str) -> Option<usize> {
        if let Font::Embedded(slot) = font {
            return self.faces.get(slot).is_some_and(Option::is_some).then_some(slot);
        }
        if self.custom {
            let want = usize::from(font.is_bold()) + 2 * usize::from(font.is_italic());
            return Some(if self.faces.get(want).is_some_and(Option::is_some) { want } else { 0 });
        }
        if !needs_unicode(text) {
            return None;
        }
        let want = usize::from(font.is_bold());
        [want, want + 2]
            .into_iter()
            .find(|i| self.faces.get(*i).is_some_and(|f| f.as_ref().is_some_and(|f| text.chars().all(|c| c.is_control() || f.has(c)))))
    }

    /// Width using exactly the face that will draw the text, in points.
    pub fn text_width(&self, font: Font, size: f32, text: &str) -> f32 {
        let slot = self.slot(font, text);
        let units =
            if slot.is_some() || self.custom || matches!(font, Font::Embedded(_)) || !needs_unicode(text) || self.faces.iter().all(Option::is_none) {
                self.run_units(font, text, slot)
            } else {
                let mut rest = text;
                let mut units = 0u64;
                while let Some((slot, run)) = self.fallback_run(font, rest) {
                    units = units.saturating_add(self.run_units(font, run, slot));
                    rest = rest.get(run.len()..).unwrap_or_default();
                }
                units
            };
        units as f32 * size / 1000.0
    }

    fn run_units(&self, font: Font, text: &str, slot: Option<usize>) -> u64 {
        match slot.and_then(|i| self.faces.get(i)?.as_deref()) {
            Some(f) => text.chars().map(|c| u64::from(f.char_width(if c == '\t' { ' ' } else { c }))).sum(),
            None => text.chars().map(|c| u64::from(font.char_width(c))).sum(),
        }
    }

    /// The next contiguous fallback run when no single face covers the complete text.
    fn fallback_run<'a>(&self, font: Font, text: &'a str) -> Option<(Option<usize>, &'a str)> {
        let slot_for = |c: char| {
            let mut bytes = [0; 4];
            let text = c.encode_utf8(&mut bytes);
            if !needs_unicode(text) {
                return None;
            }
            let want = usize::from(font.is_bold());
            [want, want + 2, 1 - want, 3 - want]
                .into_iter()
                .find(|i| self.faces.get(*i).and_then(Option::as_ref).is_some_and(|face| c.is_control() || face.has(c)))
        };
        let slot = slot_for(text.chars().next()?);
        let end = text.char_indices().find(|(_, c)| slot_for(*c) != slot).map_or(text.len(), |(i, _)| i);
        Some((slot, text.get(..end)?))
    }

    /// A custom export must not silently render missing characters as .notdef.
    pub fn missing_glyph(&self, font: Font, text: &str) -> Option<char> {
        if !self.custom && !matches!(font, Font::Embedded(_)) {
            return None;
        }
        let face = self.slot(font, text).and_then(|i| self.faces.get(i)?.as_deref())?;
        text.chars().map(|c| if c == '\t' { ' ' } else { c }).find(|c| !c.is_control() && !face.has(*c))
    }
}

/// RGB colour, 0–255 per channel.
pub type Rgb = [u8; 3];

/// A point in page coordinates, measured from the top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A page-space rectangle described by its top-left corner and extent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }
}

/// The standard 14 fonts minus Symbol and ZapfDingbats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Font {
    #[default]
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
    /// A face resolved by [`TextFonts::font`], local to that document's font set.
    Embedded(usize),
}

/// A font family for [`Font::styled`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl Font {
    pub const ALL: [Font; 12] = [
        Font::Helvetica,
        Font::HelveticaBold,
        Font::HelveticaOblique,
        Font::HelveticaBoldOblique,
        Font::TimesRoman,
        Font::TimesBold,
        Font::TimesItalic,
        Font::TimesBoldItalic,
        Font::Courier,
        Font::CourierBold,
        Font::CourierOblique,
        Font::CourierBoldOblique,
    ];

    /// PostScript base font name.
    pub fn base_name(self) -> &'static str {
        match self {
            Font::Helvetica => "Helvetica",
            Font::HelveticaBold => "Helvetica-Bold",
            Font::HelveticaOblique => "Helvetica-Oblique",
            Font::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Font::TimesRoman => "Times-Roman",
            Font::TimesBold => "Times-Bold",
            Font::TimesItalic => "Times-Italic",
            Font::TimesBoldItalic => "Times-BoldItalic",
            Font::Courier => "Courier",
            Font::CourierBold => "Courier-Bold",
            Font::CourierOblique => "Courier-Oblique",
            Font::CourierBoldOblique => "Courier-BoldOblique",
            Font::Embedded(_) => "Helvetica",
        }
    }

    fn index(self) -> usize {
        Font::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    /// The face of `family` with the given weight and slant.
    pub fn styled(family: Family, bold: bool, italic: bool) -> Font {
        match (family, bold, italic) {
            (Family::Sans, false, false) => Font::Helvetica,
            (Family::Sans, true, false) => Font::HelveticaBold,
            (Family::Sans, false, true) => Font::HelveticaOblique,
            (Family::Sans, true, true) => Font::HelveticaBoldOblique,
            (Family::Serif, false, false) => Font::TimesRoman,
            (Family::Serif, true, false) => Font::TimesBold,
            (Family::Serif, false, true) => Font::TimesItalic,
            (Family::Serif, true, true) => Font::TimesBoldItalic,
            (Family::Mono, false, false) => Font::Courier,
            (Family::Mono, true, false) => Font::CourierBold,
            (Family::Mono, false, true) => Font::CourierOblique,
            (Family::Mono, true, true) => Font::CourierBoldOblique,
        }
    }

    /// Picks a standard family for a font name: serif faces map to Times, monospaced ones to
    /// Courier, everything else to Helvetica.
    pub fn family_for(name: &str) -> Family {
        let n = name.to_ascii_lowercase();
        const SERIF: [&str; 10] = ["times", "cambria", "georgia", "garamond", "serif", "book", "palatino", "baskerville", "tinos", "caladea"];
        const MONO: [&str; 8] = ["courier", "consolas", "mono", "menlo", "lucida console", "cousine", "code", "typewriter"];
        if MONO.iter().any(|m| n.contains(m)) {
            Family::Mono
        } else if SERIF.iter().any(|s| n.contains(s)) && !n.contains("sans") {
            Family::Serif
        } else {
            Family::Sans
        }
    }

    fn is_bold(self) -> bool {
        matches!(
            self,
            Font::HelveticaBold | Font::HelveticaBoldOblique | Font::TimesBold | Font::TimesBoldItalic | Font::CourierBold | Font::CourierBoldOblique
        )
    }

    fn is_italic(self) -> bool {
        matches!(
            self,
            Font::HelveticaOblique
                | Font::HelveticaBoldOblique
                | Font::TimesItalic
                | Font::TimesBoldItalic
                | Font::CourierOblique
                | Font::CourierBoldOblique
        )
    }

    /// Advance width of a character in 1/1000 em.
    pub fn char_width(self, c: char) -> u16 {
        let table: Option<&[u16; 95]> = match self {
            Font::Helvetica | Font::HelveticaOblique => Some(&metrics::HELVETICA),
            Font::HelveticaBold | Font::HelveticaBoldOblique => Some(&metrics::HELVETICA_BOLD),
            Font::TimesRoman | Font::TimesItalic => Some(&metrics::TIMES),
            Font::TimesBold | Font::TimesBoldItalic => Some(&metrics::TIMES_BOLD),
            _ => None,
        };
        if needs_cid(c) {
            return CID_WIDTH;
        }
        let Some(table) = table else { return 600 };
        let ascii = |c: char| -> Option<u16> {
            let i = (c as u32).checked_sub(32)? as usize;
            table.get(i).copied()
        };
        if let Some(w) = ascii(c) {
            return w;
        }
        match c {
            '—' | '…' | '‰' | '™' => 1000,
            _ => metrics::fold(c).and_then(ascii).unwrap_or(match self {
                Font::TimesRoman | Font::TimesItalic | Font::TimesBold | Font::TimesBoldItalic => 500,
                _ => 556,
            }),
        }
    }
}

/// Width of `text` in points when set in `font` at `size` points.
pub fn text_width(font: Font, size: f32, text: &str) -> f32 {
    SYSTEM_FONTS.text().text_width(font, size, text)
}

/// Unicode → WinAnsiEncoding (CP1252) byte; unmappable characters become `?`.
pub fn win_ansi(c: char) -> u8 {
    let u = c as u32;
    if (0x20..0x7F).contains(&u) || (0xA0..=0xFF).contains(&u) {
        return u as u8;
    }
    match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        '\t' => b' ',
        _ => b'?',
    }
}

/// Index of the CID font in a page's font flags (after the [`Font::ALL`] faces).
const CID_FONT: usize = Font::ALL.len();
/// Number of font resources a page can use.
const FONT_SLOTS: usize = CID_FONT + 1;
/// Base font of the CID font used for characters WinAnsiEncoding lacks.
const CID_BASE_FONT: &str = "STSong-Light";
/// Default glyph width of the CID font (full-width), in 1/1000 em.
const CID_WIDTH: u16 = 1000;

/// True when `c` has no WinAnsiEncoding byte and is drawn with the CID font.
fn needs_cid(c: char) -> bool {
    !c.is_control() && c != '?' && win_ansi(c) == b'?'
}

/// A PDF hex string `< … >` of `text` in UTF-16BE (the `UniGB-UTF16-H` encoding).
fn pdf_utf16_string(text: &str) -> String {
    let mut s = String::with_capacity(text.len() * 4 + 2);
    s.push('<');
    for unit in text.encode_utf16() {
        let _ = write!(s, "{unit:04X}");
    }
    s.push('>');
    s
}

/// A PDF literal string `( … )` for `text` in WinAnsiEncoding.
fn pdf_string(text: &str) -> String {
    let mut s = String::with_capacity(text.len() + 2);
    s.push('(');
    for c in text.chars() {
        let b = win_ansi(c);
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7E => s.push(b as char),
            _ => {
                let _ = write!(s, "\\{b:03o}");
            }
        }
    }
    s.push(')');
    s
}

/// URI actions use byte strings. Percent-encode non-ASCII UTF-8 before PDF string escaping.
fn pdf_uri_string(uri: &str) -> String {
    let mut ascii = String::with_capacity(uri.len());
    for byte in uri.bytes() {
        if byte.is_ascii() {
            ascii.push(byte as char);
        } else {
            let _ = write!(ascii, "%{byte:02X}");
        }
    }
    pdf_string(&ascii)
}

/// A number for a content stream: finite, short.
fn n(v: f32) -> String {
    let v = if v.is_finite() { v.clamp(-1.0e7, 1.0e7) } else { 0.0 };
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// True when every value is finite (NaN and infinities are rejected).
fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// A positive, finite extent (a width, height or radius).
fn extent(v: f32) -> bool {
    v > 0.0 && v.is_finite()
}

/// A stroke width: negative, NaN and infinite become 0 (the thinnest line).
fn stroke_width(w: f32) -> f32 {
    if w.is_finite() { w.max(0.0) } else { 0.0 }
}

/// The entries of a dash array (`[on, off, …]`, at most 8). A pattern the PDF specification
/// forbids (any non-finite entry, or no entry that survives rounding to a positive number) is
/// replaced by an empty array, i.e. a solid line.
fn dash_values(dash: &[f32]) -> String {
    let d: Vec<f32> = dash.iter().take(8).map(|v| if v.is_finite() { v.max(0.0) } else { f32::NAN }).collect();
    if !finite(&d) || !d.iter().any(|v| (v.min(1.0e7) * 1000.0).round() > 0.0) {
        return String::new();
    }
    d.iter().map(|v| n(*v)).collect::<Vec<_>>().join(" ")
}

fn rgb_op(c: Rgb, stroke: bool) -> String {
    let f = |v: u8| n(v as f32 / 255.0);
    format!("{} {} {} {}", f(c[0]), f(c[1]), f(c[2]), if stroke { "RG" } else { "rg" })
}

struct ImageData {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

enum LinkTarget {
    Uri(String),
    Page { index: usize, x: f32, y: f32 },
}

struct LinkAnnotation {
    /// PDF coordinates, with the origin at the bottom left.
    rect: [f32; 4],
    target: LinkTarget,
}

/// One document-wide character-code allocator per face. CID 0 stays .notdef; every Unicode
/// character gets its own CID even when the TrueType cmap shares its glyph with another.
#[derive(Default)]
struct FontEncoding {
    characters: BTreeMap<char, u16>,
}

impl FontEncoding {
    fn code(&mut self, c: char) -> Option<u16> {
        if let Some(code) = self.characters.get(&c) {
            return Some(*code);
        }
        if self.characters.len() >= MAX_FONT_CHARACTERS {
            return None;
        }
        let code = u16::try_from(self.characters.len().checked_add(1)?).ok()?;
        self.characters.insert(c, code);
        Some(code)
    }
}

/// A page under construction. Obtain one with [`PdfDoc::add_page`].
pub struct Page {
    width: f32,
    height: f32,
    content: String,
    fonts: [bool; FONT_SLOTS],
    /// The fallback fonts and, per slot, the glyphs used with the character each one shows.
    unicode: TextFonts,
    glyphs: Vec<BTreeMap<u16, (char, u16)>>,
    encodings: Vec<Arc<Mutex<FontEncoding>>>,
    images: Vec<ImageData>,
    links: Vec<LinkAnnotation>,
    depth: u32,
    missing_glyph: Option<char>,
    text_limit_exceeded: bool,
}

/// Most bytes of content per page before further drawing is dropped (keeps hostile inputs from
/// exhausting memory).
const MAX_PAGE_CONTENT: usize = 64 * 1024 * 1024;
/// Largest image (pixels) embedded.
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const MAX_PAGE_LINKS: usize = 4096;
const MAX_LINK_URI_BYTES: usize = 4096;

impl Page {
    pub fn width(&self) -> f32 {
        self.width
    }
    pub fn height(&self) -> f32 {
        self.height
    }
    fn y(&self, y: f32) -> f32 {
        self.height - y
    }
    fn full(&self) -> bool {
        self.content.len() > MAX_PAGE_CONTENT
    }
    fn op(&mut self, s: &str) {
        if !self.full() {
            self.content.push_str(s);
            self.content.push('\n');
        }
    }

    fn link_rect(&self, rect: Rect) -> Option<[f32; 4]> {
        let Rect { x, y, width, height } = rect;
        if !finite(&[x, y, width, height]) || !extent(width) || !extent(height) || self.links.len() >= MAX_PAGE_LINKS {
            return None;
        }
        let (right, bottom) = (x + width, y + height);
        if !finite(&[right, bottom]) {
            return None;
        }
        let (left, right) = (x.max(0.0), right.min(self.width));
        let (top, bottom) = (y.max(0.0), bottom.min(self.height));
        (right > left && bottom > top).then(|| [left, self.y(bottom), right, self.y(top)])
    }

    /// Adds an invisible clickable area for an external URI. Invalid or excessive links are ignored.
    pub fn link_uri(&mut self, x: f32, y: f32, w: f32, h: f32, uri: &str) {
        let allowed = uri
            .split_once(':')
            .is_some_and(|(scheme, rest)| matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "mailto") && !rest.is_empty());
        if !allowed || uri.len() > MAX_LINK_URI_BYTES || uri.chars().any(char::is_control) {
            return;
        }
        if let Some(rect) = self.link_rect(Rect::new(x, y, w, h)) {
            self.links.push(LinkAnnotation { rect, target: LinkTarget::Uri(uri.to_string()) });
        }
    }

    /// Adds a link to a location on another page of this document (zero-based page index).
    pub fn link_page(&mut self, rect: Rect, page: usize, target: Point) {
        if !finite(&[target.x, target.y]) {
            return;
        }
        if let Some(rect) = self.link_rect(rect) {
            self.links.push(LinkAnnotation { rect, target: LinkTarget::Page { index: page, x: target.x, y: target.y } });
        }
    }

    /// Saves the graphics state (pair with [`Page::restore`]).
    pub fn save(&mut self) {
        self.depth = self.depth.saturating_add(1);
        self.content.push_str("q\n");
    }
    /// Restores the graphics state saved by [`Page::save`]; extra calls are ignored.
    pub fn restore(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
            self.content.push_str("Q\n");
        }
    }
    /// Intersects the clip with a rectangle (until the next [`Page::restore`]).
    pub fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let s = format!("{} {} {} {} re W n", n(x), n(self.y(y + h)), n(w.max(0.0)), n(h.max(0.0)));
        self.op(&s);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        if !(extent(w) && extent(h) && finite(&[x, y])) {
            return;
        }
        let s = format!("{}\n{} {} {} {} re f", rgb_op(color, false), n(x), n(self.y(y + h)), n(w), n(h));
        self.op(&s);
    }

    pub fn rect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: Rgb) {
        if !finite(&[x, y, w, h]) {
            return;
        }
        let s = format!("{}\n{} w [] 0 d\n{} {} {} {} re S", rgb_op(color, true), n(stroke_width(width)), n(x), n(self.y(y + h)), n(w), n(h));
        self.op(&s);
    }

    pub fn stroke_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgb) {
        self.stroke_line_dashed(Point::new(x0, y0), Point::new(x1, y1), width, color, &[]);
    }

    /// A line with a dash pattern (`[on, off, …]` in points; empty = solid).
    pub fn stroke_line_dashed(&mut self, start: Point, end: Point, width: f32, color: Rgb, dash: &[f32]) {
        if !finite(&[start.x, start.y, end.x, end.y]) {
            return;
        }
        let s = format!(
            "{}\n{} w [{}] 0 d\n{} {} m {} {} l S",
            rgb_op(color, true),
            n(stroke_width(width)),
            dash_values(dash),
            n(start.x),
            n(self.y(start.y)),
            n(end.x),
            n(self.y(end.y))
        );
        self.op(&s);
    }

    /// An open polyline.
    pub fn polyline(&mut self, pts: &[[f32; 2]], width: f32, color: Rgb, dash: &[f32]) {
        if pts.len() < 2 || !pts.iter().all(|p| finite(p)) {
            return;
        }
        let mut s = format!("{}\n{} w [{}] 0 d 1 j\n", rgb_op(color, true), n(stroke_width(width)), dash_values(dash));
        self.path(&mut s, pts, false);
        s.push_str(" S");
        self.op(&s);
    }

    fn path(&self, s: &mut String, pts: &[[f32; 2]], close: bool) {
        for (i, p) in pts.iter().enumerate() {
            let _ = write!(s, "{} {} {} ", n(p[0]), n(self.y(p[1])), if i == 0 { "m" } else { "l" });
        }
        if close {
            s.push('h');
        }
    }

    /// A closed polygon, filled and/or stroked.
    pub fn polygon(&mut self, pts: &[[f32; 2]], fill: Option<Rgb>, stroke: Option<(Rgb, f32)>) {
        if pts.len() < 2 || (fill.is_none() && stroke.is_none()) || !pts.iter().all(|p| finite(p)) {
            return;
        }
        let mut s = String::new();
        if let Some(f) = fill {
            s.push_str(&rgb_op(f, false));
            s.push('\n');
        }
        if let Some((c, w)) = stroke {
            let _ = writeln!(s, "{}\n{} w [] 0 d 1 j", rgb_op(c, true), n(stroke_width(w)));
        }
        self.path(&mut s, pts, true);
        s.push_str(match (fill.is_some(), stroke.is_some()) {
            (true, true) => " B",
            (true, false) => " f",
            _ => " S",
        });
        self.op(&s);
    }

    /// A circle approximated by four cubic Béziers.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Option<Rgb>, stroke: Option<(Rgb, f32)>) {
        if !extent(r) || !finite(&[cx, cy]) || (fill.is_none() && stroke.is_none()) {
            return;
        }
        const K: f32 = 0.552_284_8;
        let k = r * K;
        let y = self.y(cy);
        let mut s = String::new();
        if let Some(f) = fill {
            s.push_str(&rgb_op(f, false));
            s.push('\n');
        }
        if let Some((c, w)) = stroke {
            let _ = writeln!(s, "{}\n{} w [] 0 d", rgb_op(c, true), n(stroke_width(w)));
        }
        let _ = write!(
            s,
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h",
            n(cx + r),
            n(y),
            n(cx + r),
            n(y + k),
            n(cx + k),
            n(y + r),
            n(cx),
            n(y + r),
            n(cx - k),
            n(y + r),
            n(cx - r),
            n(y + k),
            n(cx - r),
            n(y),
            n(cx - r),
            n(y - k),
            n(cx - k),
            n(y - r),
            n(cx),
            n(y - r),
            n(cx + k),
            n(y - r),
            n(cx + r),
            n(y - k),
            n(cx + r),
            n(y)
        );
        s.push_str(match (fill.is_some(), stroke.is_some()) {
            (true, true) => " B",
            (true, false) => " f",
            _ => " S",
        });
        self.op(&s);
    }

    /// Text with its baseline starting at `(x, y)`.
    pub fn text(&mut self, x: f32, y: f32, size: f32, font: Font, color: Rgb, text: &str) {
        self.text_rotated(Point::new(x, y), size, font, color, text, 0.0);
    }

    /// Text rotated by `angle` radians (clockwise on the page, like the y-down coordinate
    /// system) about its baseline origin.
    pub fn text_rotated(&mut self, origin: Point, size: f32, font: Font, color: Rgb, text: &str, angle: f32) {
        if text.is_empty() || !extent(size) || !finite(&[origin.x, origin.y]) {
            return;
        }
        self.missing_glyph = self.missing_glyph.or_else(|| self.unicode.missing_glyph(font, text));
        let slot = self.unicode.slot(font, text);
        if slot.is_none()
            && !self.unicode.custom
            && !matches!(font, Font::Embedded(_))
            && needs_unicode(text)
            && self.unicode.faces.iter().any(Option::is_some)
        {
            let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
            let mut rest = text;
            let mut offset = 0.0;
            while let Some((slot, run)) = self.unicode.fallback_run(font, rest) {
                let width = self.unicode.run_units(font, run, slot) as f32 * size.min(10_000.0) / 1000.0;
                self.text_run(Point::new(origin.x + offset * cs, origin.y + offset * sn), size, font, color, run, (angle, slot));
                offset += width;
                rest = rest.get(run.len()..).unwrap_or_default();
            }
        } else {
            self.text_run(origin, size, font, color, text, (angle, slot));
        }
    }

    fn text_run(&mut self, origin: Point, size: f32, font: Font, color: Rgb, text: &str, placement: (f32, Option<usize>)) {
        let (angle, slot) = placement;
        if let Some(slot) = slot {
            let shown = self.unicode_string(slot, text);
            let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
            let s = format!(
                "BT\n{}\n/U{} {} Tf\n{} {} {} {} {} {} Tm\n{} Tj\nET",
                rgb_op(color, false),
                slot + 1,
                n(size.min(10_000.0)),
                n(cs),
                n(-sn),
                n(sn),
                n(cs),
                n(origin.x),
                n(self.y(origin.y)),
                shown
            );
            self.op(&s);
            return;
        }
        let fi = font.index();
        let size = n(size.min(10_000.0));
        let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
        // Clockwise in y-down = counter-clockwise negated in PDF's y-up space.
        let mut s = format!("BT\n{}\n{} {} {} {} {} {} Tm\n", rgb_op(color, false), n(cs), n(-sn), n(sn), n(cs), n(origin.x), n(self.y(origin.y)));
        // Runs of WinAnsi text use `font`; runs it cannot encode use the CID font. Each `Tj`
        // advances the text position, so the runs follow each other on the baseline.
        let mut rest = text;
        while let Some(first) = rest.chars().next() {
            let cid = needs_cid(first);
            let end = rest.char_indices().find(|(_, c)| needs_cid(*c) != cid).map_or(rest.len(), |(i, _)| i);
            let (run, tail) = rest.split_at(end);
            let (slot, string) = if cid { (CID_FONT, pdf_utf16_string(run)) } else { (fi, pdf_string(run)) };
            if let Some(f) = self.fonts.get_mut(slot) {
                *f = true;
            }
            let _ = writeln!(s, "/F{} {} Tf\n{} Tj", slot + 1, size, string);
            rest = tail;
        }
        s.push_str("ET");
        self.op(&s);
    }

    /// Assigns document-wide two-byte character codes, independently of TrueType glyph ids.
    fn unicode_string(&mut self, slot: usize, text: &str) -> String {
        let mut s = String::with_capacity(text.len().saturating_mul(4).saturating_add(2).min(MAX_PAGE_CONTENT));
        s.push('<');
        let font = self.unicode.faces.get(slot).cloned().flatten();
        let Some(encoding) = self.encodings.get(slot).cloned() else {
            self.text_limit_exceeded = true;
            return "<>".into();
        };
        let mut encoding = encoding.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for c in text.chars().take(MAX_PAGE_CONTENT / 4) {
            let c = if c == '\t' { ' ' } else { c };
            let g = font.as_deref().map_or(0, |f| f.glyph(c));
            let code = match encoding.code(c) {
                Some(code) => code,
                None => {
                    self.text_limit_exceeded = true;
                    0
                }
            };
            if code != 0
                && let Some(used) = self.glyphs.get_mut(slot)
            {
                used.insert(code, (c, g));
            }
            let _ = write!(s, "{code:04X}");
        }
        s.push('>');
        s
    }

    /// The faces used to draw and measure this page.
    pub fn text_fonts(&self) -> &TextFonts {
        &self.unicode
    }

    /// Width using this page's text faces.
    pub fn text_width(&self, font: Font, size: f32, text: &str) -> f32 {
        self.unicode.text_width(font, size, text)
    }

    /// Draws an RGBA image (`pw`×`ph` pixels, row-major, straight alpha) into the box
    /// `(x, y, w, h)`. Malformed or oversized buffers are ignored.
    pub fn image_rgba(&mut self, destination: Rect, rgba: &[u8], pixel_size: [u32; 2]) {
        let Rect { x, y, width: w, height: h } = destination;
        let [pw, ph] = pixel_size;
        let px = pw as u64 * ph as u64;
        if pw == 0 || ph == 0 || px > MAX_IMAGE_PIXELS || (rgba.len() as u64) < px * 4 || !(extent(w) && extent(h) && finite(&[x, y])) {
            return;
        }
        let px = px as usize;
        let mut rgb = Vec::with_capacity(px * 3);
        let mut alpha = Vec::with_capacity(px);
        for p in rgba.as_chunks::<4>().0.iter().take(px) {
            rgb.extend_from_slice(&[p[0], p[1], p[2]]);
            alpha.push(p[3]);
        }
        let alpha = if alpha.iter().all(|a| *a == 255) { None } else { Some(alpha) };
        let idx = self.images.len();
        self.images.push(ImageData { width: pw, height: ph, rgb, alpha });
        let s = format!("q {} 0 0 {} {} {} cm /Im{} Do Q", n(w), n(h), n(x), n(self.y(y + h)), idx + 1);
        self.op(&s);
    }
}

/// Document metadata.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub title: String,
    pub author: String,
    pub creator: String,
}

/// A PDF document under construction.
#[derive(Default)]
pub struct PdfDoc {
    pages: Vec<Page>,
    pub info: Info,
    unicode: TextFonts,
    encodings: Vec<Arc<Mutex<FontEncoding>>>,
}

/// PackBits-style run-length encoding (PDF `RunLengthDecode`).
fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run = 1;
        while i + run < data.len() && run < 128 && data[i + run] == b {
            run += 1;
        }
        if run >= 2 {
            out.push((257 - run) as u8);
            out.push(b);
            i += run;
            continue;
        }
        // Literal run until the next repeat of 3+ or 128 bytes.
        let start = i;
        let mut len = 0;
        while i < data.len() && len < 128 {
            if i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2] {
                break;
            }
            i += 1;
            len += 1;
        }
        if len == 0 {
            // Next is a repeat; loop will handle it.
            continue;
        }
        out.push((len - 1) as u8);
        out.extend_from_slice(&data[start..start + len]);
    }
    out.push(128);
    out
}

/// Longest Info string written, in characters; longer values are cut on a character boundary.
const MAX_INFO_CHARS: usize = 1024;

/// A PDF text string (document Info entries) for `text`: a literal `( … )` for printable ASCII,
/// otherwise UTF-16BE with a byte-order mark as a hex string (ISO 32000-1 7.9.2.2).
fn pdf_text_string(text: &str) -> String {
    let text: String = text.chars().take(MAX_INFO_CHARS).collect();
    if text.chars().all(|c| matches!(c, ' '..='~')) {
        return pdf_string(&text);
    }
    let mut s = String::with_capacity(text.len() * 4 + 6);
    s.push_str("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(s, "{unit:04X}");
    }
    s.push('>');
    s
}

impl PdfDoc {
    /// A document that uses the registered fallback fonts ([`set_unicode_font_loader`]).
    pub fn new() -> PdfDoc {
        Self::with_text_fonts(TextFonts::system())
    }

    /// A document using the supplied text faces for all pages.
    pub fn with_text_fonts(fonts: TextFonts) -> PdfDoc {
        PdfDoc { unicode: fonts, ..PdfDoc::default() }
    }

    /// Replaces fallback fonts for text outside WinAnsiEncoding before adding pages.
    /// Returns an error without changing the document once any page exists.
    pub fn set_unicode_fonts(&mut self, fonts: UnicodeFonts) -> Result<(), &'static str> {
        self.set_text_fonts(TextFonts::fallback(fonts))
    }

    /// Sets text faces before adding pages. One font set is used for the entire document.
    /// Returns an error without changing the document once any page exists.
    pub fn set_text_fonts(&mut self, fonts: TextFonts) -> Result<(), &'static str> {
        if !self.pages.is_empty() {
            return Err("PDF text fonts cannot change after adding a page");
        }
        self.unicode = fonts;
        Ok(())
    }

    /// Adds a page of `width_pt` × `height_pt` points and returns it for drawing.
    pub fn add_page(&mut self, width_pt: f32, height_pt: f32) -> &mut Page {
        let width = if width_pt.is_finite() { width_pt.clamp(3.0, 14_400.0) } else { 612.0 };
        let height = if height_pt.is_finite() { height_pt.clamp(3.0, 14_400.0) } else { 792.0 };
        self.encodings.resize_with(self.unicode.faces.len(), || Arc::new(Mutex::new(FontEncoding::default())));
        self.pages.push(Page {
            width,
            height,
            content: String::new(),
            fonts: [false; FONT_SLOTS],
            unicode: self.unicode.clone(),
            glyphs: vec![BTreeMap::new(); self.unicode.faces.len()],
            encodings: self.encodings.clone(),
            images: Vec::new(),
            links: Vec::new(),
            depth: 0,
            missing_glyph: None,
            text_limit_exceeded: false,
        });
        let i = self.pages.len() - 1;
        &mut self.pages[i]
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The page at `i`, to keep drawing on it.
    pub fn page_mut(&mut self, i: usize) -> Option<&mut Page> {
        self.pages.get_mut(i)
    }

    /// First character unavailable in a custom face used by this document.
    pub fn missing_glyph(&self) -> Option<char> {
        self.pages.iter().find_map(|p| p.missing_glyph)
    }

    /// Whether an embedded face exceeds its bounded Unicode map across all pages.
    pub fn text_limit_exceeded(&self) -> bool {
        self.pages.iter().any(|p| p.text_limit_exceeded)
    }

    fn embed_text_fonts(&self, next: &mut u32, font_dict: &mut String, objects: &mut Vec<(u32, Vec<u8>)>) {
        let slots = self.pages.iter().map(|p| p.unicode.faces.len()).max().unwrap_or(0);
        for slot in 0..slots {
            let mut glyphs: BTreeMap<u16, (char, u16)> = BTreeMap::new();
            let mut font = None;
            for p in &self.pages {
                if let (Some(used), Some(Some(f))) = (p.glyphs.get(slot), p.unicode.faces.get(slot))
                    && !used.is_empty()
                {
                    glyphs.extend(used.iter().map(|(code, glyph)| (*code, *glyph)));
                    font = Some(f.clone());
                }
            }
            if let Some(font) = font {
                let id = *next;
                *next += 6;
                let _ = write!(font_dict, "/U{} {} 0 R ", slot + 1, id);
                objects.extend(unicode_font_objects(&font, &glyphs, slot, id));
            }
        }
    }

    /// Serializes the document. A document without pages gets one blank Letter page.
    pub fn finish(mut self) -> Vec<u8> {
        if self.pages.is_empty() {
            self.add_page(612.0, 792.0);
        }
        // Object numbering: 1 catalog, 2 page tree, 3 info, then fonts, then per page.
        let mut used = [false; FONT_SLOTS];
        for p in &self.pages {
            for (u, f) in used.iter_mut().zip(p.fonts.iter()) {
                *u |= *f;
            }
        }
        let mut next = 4u32;
        let mut font_ids = [0u32; FONT_SLOTS];
        for (i, u) in used.iter().enumerate() {
            if *u {
                font_ids[i] = next;
                // The CID font also needs its descendant font and font descriptor.
                next += if i == CID_FONT { 3 } else { 1 };
            }
        }
        let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut kids = Vec::new();
        let mut annotations = Vec::new();
        let page_count = self.pages.len();
        let mut font_dict = String::new();
        for (i, id) in font_ids.iter().enumerate() {
            if *id != 0 {
                let _ = write!(font_dict, "/F{} {} 0 R ", i + 1, id);
                if i == CID_FONT {
                    let (cid_id, desc_id) = (id + 1, id + 2);
                    objects.push((
                        *id,
                        format!(
                            "<< /Type /Font /Subtype /Type0 /BaseFont /{CID_BASE_FONT} /Encoding /UniGB-UTF16-H /DescendantFonts [{cid_id} 0 R] >>"
                        )
                        .into_bytes(),
                    ));
                    objects.push((
                        cid_id,
                        format!(
                            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /{CID_BASE_FONT} /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 4 >> /FontDescriptor {desc_id} 0 R /DW {CID_WIDTH} >>"
                        )
                        .into_bytes(),
                    ));
                    objects.push((
                        desc_id,
                        format!(
                            "<< /Type /FontDescriptor /FontName /{CID_BASE_FONT} /Flags 6 /FontBBox [-25 -254 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 880 /StemV 93 >>"
                        )
                        .into_bytes(),
                    ));
                    continue;
                }
                let name = Font::ALL.get(i).copied().unwrap_or_default().base_name();
                let enc = if name.starts_with("Symbol") { "" } else { " /Encoding /WinAnsiEncoding" };
                objects.push((*id, format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name}{enc} >>").into_bytes()));
            }
        }
        self.embed_text_fonts(&mut next, &mut font_dict, &mut objects);
        for page in &mut self.pages {
            while page.depth > 0 {
                page.restore();
            }
            let mut xobjects = String::new();
            for (k, img) in page.images.iter().enumerate() {
                let img_id = next;
                next += 1;
                let smask = match &img.alpha {
                    Some(a) => {
                        let id = next;
                        next += 1;
                        let data = run_length(a);
                        let mut o = format!(
                            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /RunLengthDecode /Length {} >>\nstream\n",
                            img.width,
                            img.height,
                            data.len()
                        )
                        .into_bytes();
                        o.extend_from_slice(&data);
                        o.extend_from_slice(b"\nendstream");
                        objects.push((id, o));
                        format!(" /SMask {id} 0 R")
                    }
                    None => String::new(),
                };
                let data = run_length(&img.rgb);
                let mut o = format!(
                    "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8{} /Filter /RunLengthDecode /Length {} >>\nstream\n",
                    img.width,
                    img.height,
                    smask,
                    data.len()
                )
                .into_bytes();
                o.extend_from_slice(&data);
                o.extend_from_slice(b"\nendstream");
                objects.push((img_id, o));
                let _ = write!(xobjects, "/Im{} {} 0 R ", k + 1, img_id);
            }
            let content_id = next;
            let page_id = next + 1;
            next += 2;
            let body = std::mem::take(&mut page.content);
            let mut o = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
            o.extend_from_slice(body.as_bytes());
            o.extend_from_slice(b"endstream");
            objects.push((content_id, o));
            let xo = if xobjects.is_empty() { String::new() } else { format!(" /XObject << {xobjects}>>") };
            let mut link_refs = String::new();
            for link in std::mem::take(&mut page.links) {
                if matches!(&link.target, LinkTarget::Page { index, .. } if *index >= page_count) {
                    continue;
                }
                let id = next;
                next += 1;
                let _ = write!(link_refs, "{id} 0 R ");
                annotations.push((id, link));
            }
            let annots = if link_refs.is_empty() { String::new() } else { format!(" /Annots [{link_refs}]") };
            objects.push((
                page_id,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /ProcSet [/PDF /Text /ImageB /ImageC] /Font << {}>>{} >> /Contents {} 0 R{} >>",
                    n(page.width),
                    n(page.height),
                    font_dict,
                    xo,
                    content_id,
                    annots
                )
                .into_bytes(),
            ));
            kids.push(page_id);
        }
        for (id, link) in annotations {
            let [x0, y0, x1, y1] = link.rect;
            let action = match link.target {
                LinkTarget::Uri(uri) => format!("/A << /S /URI /URI {} >>", pdf_uri_string(&uri)),
                LinkTarget::Page { index, x, y } => {
                    let Some(&page_id) = kids.get(index) else { continue };
                    let height = self.pages.get(index).map_or(0.0, |p| p.height);
                    format!("/Dest [{page_id} 0 R /XYZ {} {} null]", n(x), n(height - y))
                }
            };
            objects.push((
                id,
                format!("<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] {action} >>", n(x0), n(y0), n(x1), n(y1)).into_bytes(),
            ));
        }
        objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
        let kids_s: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
        objects.push((2, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids_s.join(" "), kids.len()).into_bytes()));
        let creator = if self.info.creator.is_empty() { "GridCraft" } else { &self.info.creator };
        let mut info = format!("<< /Producer {} /Creator {}", pdf_text_string("GridCraft PDF"), pdf_text_string(creator));
        if !self.info.title.is_empty() {
            let _ = write!(info, " /Title {}", pdf_text_string(&self.info.title));
        }
        if !self.info.author.is_empty() {
            let _ = write!(info, " /Author {}", pdf_text_string(&self.info.author));
        }
        info.push_str(" >>");
        objects.push((3, info.into_bytes()));
        objects.sort_by_key(|(id, _)| *id);

        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0usize; next as usize];
        for (id, body) in &objects {
            if let Some(slot) = offsets.get_mut(*id as usize) {
                *slot = out.len();
            }
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", next);
        for off in offsets.iter().skip(1) {
            let _ = writeln!(x, "{off:010} 00000 n ");
        }
        let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{}\n%%EOF\n", next, xref);
        out.extend_from_slice(x.as_bytes());
        out
    }
}

/// The six objects of an embedded font: Type0, CIDFontType2, descriptor, TrueType file,
/// ToUnicode and the explicit CID-to-glyph map.
fn unicode_font_objects(font: &UnicodeFont, glyphs: &BTreeMap<u16, (char, u16)>, slot: usize, id: u32) -> Vec<(u32, Vec<u8>)> {
    let name = format!("GridCraftUnicode{}", slot + 1);
    let mut widths = String::new();
    let map_len = glyphs.last_key_value().map_or(2, |(cid, _)| (usize::from(*cid) + 1) * 2);
    let mut cid_to_gid = vec![0u8; map_len];
    for (code, (_, glyph)) in glyphs {
        let _ = write!(widths, "{} [{}] ", code, font.glyph_width(*glyph));
        let offset = usize::from(*code) * 2;
        if let Some(entry) = cid_to_gid.get_mut(offset..offset + 2) {
            entry.copy_from_slice(&glyph.to_be_bytes());
        }
    }
    let [x0, y0, x1, y1] = font.bbox;
    let mut file = format!("<< /Length {0} /Length1 {0} >>\nstream\n", font.data.len()).into_bytes();
    file.extend_from_slice(&font.data);
    file.extend_from_slice(b"\nendstream");
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &(char, u16))> = glyphs.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
        for (code, (c, _)) in chunk {
            let mut units = [0u16; 2];
            let hex: String = c.encode_utf16(&mut units).iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(cmap, "<{code:04X}> <{hex}>");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
    let mut to_unicode = format!("<< /Length {} >>\nstream\n", cmap.len()).into_bytes();
    to_unicode.extend_from_slice(cmap.as_bytes());
    to_unicode.extend_from_slice(b"\nendstream");
    let mut mapping = format!("<< /Length {} >>\nstream\n", cid_to_gid.len()).into_bytes();
    mapping.extend_from_slice(&cid_to_gid);
    mapping.extend_from_slice(b"\nendstream");
    vec![
        (id, format!("<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>", id + 1, id + 4).into_bytes()),
        (
            id + 1,
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /DW 1000 /W [{widths}] /CIDToGIDMap {} 0 R >>",
                id + 2, id + 5
            )
            .into_bytes(),
        ),
        (
            id + 2,
            format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{x0} {y0} {x1} {y1}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {} 0 R >>",
                font.ascent,
                font.descent,
                font.cap_height,
                id + 3
            )
            .into_bytes(),
        ),
        (id + 3, file),
        (id + 4, to_unicode),
        (id + 5, mapping),
    ]
}

#[cfg(test)]
mod tests;
