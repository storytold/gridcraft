//! Embedded, subset TrueType faces for text `WinAnsiEncoding` cannot carry.
//!
//! The standard 14 PDF fonts use `WinAnsiEncoding` (CP1252): `à` and `ê` are there, but Vietnamese
//! `ạ ế ơ đ`, Czech `č ř ě`, Greek `ά ς` and Cyrillic `Ж` are not. Those characters used to go to
//! the non-embedded Adobe-GB1 CID font, which has no glyphs for them, so a reader drew missing-glyph
//! boxes (and before the CID font existed, a literal `?`).
//!
//! A run of such text is now set in an embedded, subset TrueType face standing in for the standard
//! font: a Type0 font with a CIDFontType2 descendant, `Identity-H` encoding (two-byte glyph ids in
//! the content stream) and a ToUnicode CMap, so the text stays searchable and copyable. The subset
//! keeps every glyph id (unused glyph outlines are emptied) and includes the components of composite
//! glyphs, so the content stream needs no remapping.
//!
//! The face is the metric-compatible system family of the standard font — Arial for Helvetica,
//! Times New Roman for Times, Courier New for Courier, or Liberation/DejaVu where those are not
//! installed — so [`crate::advance`] and [`crate::text_width`] measure with the same widths the PDF
//! draws. Only `glyf` outlines can be embedded; a CFF-flavoured face (`.otf`) is skipped and its
//! characters keep the CID fallback. Nothing is shipped in the repository: the faces are read from
//! the operating system on first use, and on the web there are no font files, so the web build keeps
//! the CID fallback.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

use skrifa::{FontRef, MetadataProvider, Tag};

use crate::{Error, FONT_COUNT, Font};

/// Largest font file read.
const MAX_FONT_BYTES: usize = 64 << 20;
/// Most glyphs a font may claim (the `maxp` limit).
const MAX_GLYPHS: usize = 65_535;
/// Largest subset this writer builds.
const MAX_SUBSET_BYTES: usize = 64 << 20;

/// Glyphs of one embedded face used on a page: glyph id → a character it draws (the ToUnicode
/// fallback; the page's `ActualText` preserves the exact source text, including characters that
/// share a glyph).
pub(crate) type Usage = BTreeMap<u16, char>;

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn bei16(data: &[u8], at: usize) -> Option<i16> {
    be16(data, at).map(|v| v as i16)
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*data.get(at)?, *data.get(at + 1)?, *data.get(at + 2)?, *data.get(at + 3)?]))
}

/// A parsed, embeddable TrueType face.
pub(crate) struct Face {
    data: Vec<u8>,
    index: u32,
    /// PostScript-safe base name (no spaces or name delimiters).
    name: String,
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    cap_height: i16,
    bbox: [i16; 4],
    italic_angle: f32,
    fixed_pitch: bool,
    /// Advance widths by glyph id, in font units.
    advances: Vec<u16>,
}

impl Face {
    /// Parses face `index` of a TrueType/OpenType file (or collection). `None` for anything this
    /// writer cannot embed: no `glyf` outlines, an embedding licence that forbids it, or malformed
    /// or out-of-range tables. A hostile file never panics here; it is refused.
    pub(crate) fn parse(data: Vec<u8>, index: u32) -> Option<Face> {
        if data.len() > MAX_FONT_BYTES {
            return None;
        }
        let font = FontRef::from_index(&data, index).ok()?;
        let table = |tag: &[u8; 4]| font.table_data(Tag::new(tag)).map(|data| data.as_bytes());
        let head = table(b"head")?;
        let hhea = table(b"hhea")?;
        let hmtx = table(b"hmtx")?;
        let maxp = table(b"maxp")?;
        let glyf = table(b"glyf")?;
        let loca = table(b"loca")?;
        let units_per_em = be16(head, 18).filter(|units| (16..=16_384).contains(units))?;
        let num_glyphs = usize::from(be16(maxp, 4)?);
        if num_glyphs == 0 || num_glyphs > MAX_GLYPHS {
            return None;
        }
        let num_hmetrics = usize::from(be16(hhea, 34)?).clamp(1, num_glyphs);
        // Validate the outlines now, so building a subset later cannot find a bad offset.
        validate_glyph_ranges(loca, glyf.len(), num_glyphs, be16(head, 50)?)?;
        let mut advances = Vec::with_capacity(num_glyphs);
        let mut last = 0u16;
        for glyph in 0..num_glyphs {
            if glyph < num_hmetrics {
                last = be16(hmtx, glyph * 4)?;
            }
            advances.push(last);
        }
        let (mut ascender, mut descender) = (bei16(hhea, 4)?, bei16(hhea, 6)?);
        let mut cap_height = ascender;
        if let Some(os2) = table(b"OS/2") {
            let fs_type = be16(os2, 8).unwrap_or(0);
            // Bit 1: restricted embedding. Bit 8: no subsetting (which this writer does).
            // Bit 9: bitmap embedding only, so no outlines.
            if fs_type & 0x0302 != 0 {
                return None;
            }
            if let (Some(a), Some(d)) = (bei16(os2, 68), bei16(os2, 70))
                && a > 0
            {
                ascender = a;
                descender = d;
            }
            if be16(os2, 0).unwrap_or(0) >= 2
                && let Some(cap) = bei16(os2, 88).filter(|cap| *cap > 0)
            {
                cap_height = cap;
            }
        }
        let (italic_angle, fixed_pitch) = match table(b"post") {
            Some(post) => (be32(post, 4).map_or(0.0, |v| v as i32 as f32 / 65_536.0), be32(post, 12).unwrap_or(0) != 0),
            None => (0.0, false),
        };
        let bbox = [bei16(head, 36)?, bei16(head, 38)?, bei16(head, 40)?, bei16(head, 42)?];
        let name = postscript_name(&font).unwrap_or_else(|| "EmbeddedFont".to_string());
        Some(Face { data, index, name, units_per_em, ascender, descender, cap_height, bbox, italic_angle, fixed_pitch, advances })
    }

    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index).ok()
    }

    /// The glyph id for each character of `text`, `None` where the face has no glyph. The whole
    /// string is looked up in one pass over the font's character map.
    pub(crate) fn glyphs(&self, text: &str) -> Vec<(char, Option<u16>)> {
        let Some(font) = self.font() else {
            return text.chars().map(|c| (c, None)).collect();
        };
        let charmap = font.charmap();
        text.chars().map(|c| (c, glyph_id(charmap.map(c).map(|glyph| glyph.to_u32()), self.advances.len()))).collect()
    }

    /// The glyph id for `c`, or `None` where the face has no glyph.
    pub(crate) fn glyph(&self, c: char) -> Option<u16> {
        glyph_id(self.font()?.charmap().map(c).map(|glyph| glyph.to_u32()), self.advances.len())
    }

    /// Advance width of glyph `glyph`, in 1/1000 em.
    pub(crate) fn width(&self, glyph: u16) -> u16 {
        let advance = self.advances.get(usize::from(glyph)).copied().unwrap_or(0);
        (u32::from(advance) * 1000 + u32::from(self.units_per_em) / 2)
            .checked_div(u32::from(self.units_per_em))
            .and_then(|width| u16::try_from(width).ok())
            .unwrap_or(1000)
    }

    /// A value in 1/1000 em.
    fn scaled(&self, value: i16) -> i32 {
        i32::from(value) * 1000 / i32::from(self.units_per_em)
    }

    /// A TrueType file holding only the glyph outlines of `usage` (plus `.notdef` and the
    /// components of composite glyphs); every glyph id is kept, so the content stream needs no
    /// remapping. A `cmap`, a `post` and the original hinting tables are included so the program
    /// has the tables a PDF reader expects.
    fn subset(&self, usage: &Usage) -> Option<Vec<u8>> {
        let font = self.font()?;
        let table = |tag: &[u8; 4]| font.table_data(Tag::new(tag)).map(|data| data.as_bytes());
        let head = table(b"head")?;
        let loca = table(b"loca")?;
        let glyf = table(b"glyf")?;
        let long = be16(head, 50)? != 0;
        let num_glyphs = self.advances.len();
        let range = |glyph: usize| -> Option<(usize, usize)> {
            let (start, end) = if long {
                (be32(loca, glyph * 4)? as usize, be32(loca, glyph * 4 + 4)? as usize)
            } else {
                (usize::from(be16(loca, glyph * 2)?) * 2, usize::from(be16(loca, glyph * 2 + 2)?) * 2)
            };
            (start <= end && end <= glyf.len()).then_some((start, end))
        };
        let keep = self.keep_set(usage, num_glyphs, range, glyf)?;
        let mut new_glyf: Vec<u8> = Vec::new();
        let mut new_loca: Vec<u8> = Vec::with_capacity((num_glyphs + 1) * 4);
        for glyph in 0..num_glyphs {
            new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
            if keep.contains(&glyph)
                && let Some((start, end)) = range(glyph)
                && let Some(bytes) = glyf.get(start..end)
            {
                new_glyf.extend_from_slice(bytes);
                while !new_glyf.len().is_multiple_of(4) {
                    new_glyf.push(0);
                }
                if new_glyf.len() > MAX_SUBSET_BYTES {
                    return None;
                }
            }
        }
        new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
        let mut new_head = head.to_vec();
        new_head.get_mut(8..12)?.copy_from_slice(&[0; 4]);
        // `loca` is rewritten with long offsets.
        new_head.get_mut(50..52)?.copy_from_slice(&1u16.to_be_bytes());
        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cmap", character_map(usage, num_glyphs)?),
            (*b"glyf", new_glyf),
            (*b"head", new_head),
            (*b"loca", new_loca),
            (*b"post", post_table(self.italic_angle, self.fixed_pitch)),
        ];
        for tag in [b"OS/2", b"cvt ", b"fpgm", b"hhea", b"hmtx", b"maxp", b"prep"] {
            if let Some(data) = table(tag) {
                tables.push((*tag, data.to_vec()));
            }
        }
        tables.sort_by_key(|(tag, _)| *tag);
        sfnt(&tables)
    }

    /// Glyph ids kept in a subset: the used ones, `.notdef`, and the complete transitive closure of
    /// the components of composite glyphs. A worklist with a visited set bounded by `num_glyphs`
    /// finishes every chain without a depth limit; a malformed composite (truncated data or a
    /// component id the face does not have) refuses the face instead of silently dropping an
    /// outline the kept glyph still refers to.
    fn keep_set(&self, usage: &Usage, num_glyphs: usize, range: impl Fn(usize) -> Option<(usize, usize)>, glyf: &[u8]) -> Option<BTreeSet<usize>> {
        let mut keep: BTreeSet<usize> = usage.keys().map(|glyph| usize::from(*glyph)).filter(|glyph| *glyph < num_glyphs).collect();
        keep.insert(0);
        let mut pending: Vec<usize> = keep.iter().copied().collect();
        while let Some(glyph) = pending.pop() {
            let (start, end) = range(glyph)?;
            let bytes = glyf.get(start..end)?;
            for component in components(bytes)? {
                if component >= num_glyphs {
                    return None;
                }
                if keep.insert(component) {
                    pending.push(component);
                }
            }
        }
        Some(keep)
    }
}

/// `glyph` as a `u16` within the face, ignoring glyph 0 and ids the face does not have.
fn glyph_id(glyph: Option<u32>, num_glyphs: usize) -> Option<u16> {
    let glyph = u16::try_from(glyph?).ok()?;
    (usize::from(glyph) < num_glyphs && glyph != 0).then_some(glyph)
}

/// Checks that every glyph's `loca` range lies in `glyf` and that the offsets are monotonic, so
/// building a subset cannot find an out-of-range glyph. A short `loca` stores half-offsets.
fn validate_glyph_ranges(loca: &[u8], glyf_len: usize, num_glyphs: usize, long: u16) -> Option<()> {
    if long > 1 {
        return None;
    }
    let byte =
        |at: usize| -> Option<usize> { if long == 1 { be32(loca, at).map(|v| v as usize) } else { be16(loca, at).map(|v| usize::from(v) * 2) } };
    let stride = if long == 1 { 4 } else { 2 };
    let mut previous = byte(0)?;
    for glyph in 0..num_glyphs {
        let end = byte((glyph + 1) * stride)?;
        if end < previous || end > glyf_len {
            return None;
        }
        previous = end;
    }
    Some(())
}

/// The glyph ids a composite glyph refers to (none for a simple glyph). The list is read until the
/// component records stop asking for more, so a valid composite of any length is accepted; each
/// record advances the cursor by at least six bytes through checked reads, which bounds the walk by
/// the glyph's own bytes. `None` when the component data is truncated or malformed: the face is
/// refused rather than emitting an incomplete glyph.
fn components(glyph: &[u8]) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    if glyph.is_empty() {
        return Some(out);
    }
    if bei16(glyph, 0)? >= 0 {
        return Some(out);
    }
    let mut at = 10usize;
    loop {
        let flags = be16(glyph, at)?;
        let component = be16(glyph, at + 2)?;
        out.push(usize::from(component));
        at = at.checked_add(4)?;
        at = at.checked_add(if flags & 0x0001 != 0 { 4 } else { 2 })?;
        let arguments = if flags & 0x0008 != 0 {
            2
        } else if flags & 0x0040 != 0 {
            4
        } else if flags & 0x0080 != 0 {
            8
        } else {
            0
        };
        at = at.checked_add(arguments)?;
        if at > glyph.len() {
            return None;
        }
        if flags & 0x0020 == 0 {
            return Some(out);
        }
    }
}

/// A `post` table version 3.0: no glyph names, but the style fields a font descriptor mirrors.
fn post_table(italic_angle: f32, fixed_pitch: bool) -> Vec<u8> {
    let mut table = Vec::with_capacity(32);
    table.extend_from_slice(&0x0003_0000u32.to_be_bytes());
    let angle = if italic_angle.is_finite() { (italic_angle * 65_536.0) as i32 } else { 0 };
    table.extend_from_slice(&angle.to_be_bytes());
    table.extend_from_slice(&(-100i16).to_be_bytes());
    table.extend_from_slice(&50i16.to_be_bytes());
    table.extend_from_slice(&u32::from(fixed_pitch).to_be_bytes());
    table.extend_from_slice(&[0; 16]);
    table
}

/// A `cmap` table with one format 12 subtable mapping each used character to its glyph id. A PDF
/// reader does not need it (the CIDToGIDMap is Identity), but a TrueType font program is expected
/// to carry a character map.
fn character_map(usage: &Usage, num_glyphs: usize) -> Option<Vec<u8>> {
    let mut by_character: Vec<(u32, u16)> =
        usage.iter().filter(|(glyph, _)| usize::from(**glyph) < num_glyphs).map(|(glyph, c)| (u32::from(*c), *glyph)).collect();
    by_character.sort_unstable();
    by_character.dedup_by_key(|(c, _)| *c);
    let groups = u32::try_from(by_character.len()).ok()?;
    let subtable_len = 16u32.checked_add(12u32.checked_mul(groups)?)?;
    let mut table = Vec::with_capacity(12 + subtable_len as usize);
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&3u16.to_be_bytes());
    table.extend_from_slice(&10u16.to_be_bytes());
    table.extend_from_slice(&12u32.to_be_bytes());
    table.extend_from_slice(&12u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&subtable_len.to_be_bytes());
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&groups.to_be_bytes());
    for (c, glyph) in by_character {
        table.extend_from_slice(&c.to_be_bytes());
        table.extend_from_slice(&c.to_be_bytes());
        table.extend_from_slice(&u32::from(glyph).to_be_bytes());
    }
    Some(table)
}

/// An sfnt file from `(tag, data)` tables sorted by tag, with table checksums and the `head`
/// checksum adjustment filled in (calculated with the adjustment field zero, as the spec says).
fn sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Option<Vec<u8>> {
    let count = u16::try_from(tables.len()).ok()?;
    let mut power = 1u16;
    let mut log = 0u16;
    while power.saturating_mul(2) <= count {
        power = power.saturating_mul(2);
        log += 1;
    }
    let search = power.checked_mul(16)?;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for value in [count, search, log, count.checked_mul(16)?.saturating_sub(search)] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offsets = Vec::with_capacity(tables.len());
    let mut offset = 12usize.checked_add(16usize.checked_mul(tables.len())?)?;
    for (_, data) in tables {
        offsets.push(offset);
        offset = offset.checked_add(data.len().div_ceil(4).checked_mul(4)?)?;
    }
    for ((tag, data), offset) in tables.iter().zip(&offsets) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&u32::try_from(*offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    let head = tables.iter().position(|(tag, _)| tag == b"head")?;
    let head_at = *offsets.get(head)?;
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out.get_mut(head_at.checked_add(8)?..head_at.checked_add(12)?)?.copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}

/// The sfnt checksum of `data`: the sum of its big-endian 32-bit words, zero-padded.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        for (slot, byte) in word.iter_mut().zip(chunk) {
            *slot = *byte;
        }
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// The face's PostScript name (name id 6), reduced to characters a PDF name allows unescaped.
fn postscript_name(font: &FontRef<'_>) -> Option<String> {
    let name = font.localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME).english_or_first()?.chars().collect::<String>();
    let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(60).collect();
    (!name.is_empty()).then_some(name)
}

/// A subset tag (`ABCDEF+`) derived from the glyphs used, as PDF requires for subset fonts. Stable
/// for the same usage, so an unchanged export writes the same bytes.
fn subset_tag(usage: &Usage) -> String {
    let mut hash: u32 = 2_166_136_261;
    for (glyph, c) in usage {
        for byte in glyph.to_be_bytes().into_iter().chain(u32::from(*c).to_be_bytes()) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
        }
    }
    (0..6)
        .map(|index| {
            let letter = ((hash >> (index * 5)) % 26) as u8;
            char::from(b'A' + letter)
        })
        .collect()
}

/// Writes the five PDF objects of `face` with the glyphs in `usage` into `out`, and returns the
/// object id of the Type0 font. Object ids are taken from `next`.
pub(crate) fn write_objects(face: &Face, usage: &Usage, next: &mut u32, out: &mut craft_pdf::classic::Writer) -> Result<u32, Error> {
    let Some(end) = next.checked_add(5) else { return Err(Error("PDF font object count exceeded".into())) };
    let (type0, descendant, descriptor, file, to_unicode) = (*next, *next + 1, *next + 2, *next + 3, *next + 4);
    *next = end;
    let tag = subset_tag(usage);
    let base = format!("{tag}+{}", face.name);
    out.object(
        type0,
        &format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{base} /Encoding /Identity-H /DescendantFonts [{descendant} 0 R] /ToUnicode {to_unicode} 0 R >>"
        ),
    )?;
    let mut widths = String::new();
    for glyph in usage.keys() {
        let _ = write!(widths, "{glyph} [{}] ", face.width(*glyph));
    }
    out.object(
        descendant,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{base} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /DW {} /W [{widths}] /CIDToGIDMap /Identity >>",
            face.width(0)
        ),
    )?;
    // Flags: 4 symbolic (Identity-H requires it), 1 fixed pitch, 64 italic.
    let flags = 4 | u32::from(face.fixed_pitch) | if face.italic_angle != 0.0 { 64 } else { 0 };
    let [x0, y0, x1, y1] = face.bbox;
    out.object(
        descriptor,
        &format!(
            "<< /Type /FontDescriptor /FontName /{base} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",
            face.scaled(x0),
            face.scaled(y0),
            face.scaled(x1),
            face.scaled(y1),
            crate::n(face.italic_angle),
            face.scaled(face.ascender),
            face.scaled(face.descender),
            face.scaled(face.cap_height),
        ),
    )?;
    let program = face.subset(usage).ok_or_else(|| Error("PDF export could not build the embedded font subset".into()))?;
    out.stream(file, &format!("/Length1 {}", program.len()), &program)?;
    let cmap = to_unicode_cmap(usage);
    out.stream(to_unicode, "", cmap.as_bytes())?;
    Ok(type0)
}

/// A ToUnicode CMap mapping each used glyph id to the character it draws.
fn to_unicode_cmap(usage: &Usage) -> String {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in usage.iter().collect::<Vec<_>>().chunks(100) {
        let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
        for (glyph, c) in chunk {
            let source = crate::arabic::unicode_string(&c.to_string(), false);
            let _ = writeln!(cmap, "<{glyph:04X}> {source}");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    cmap
}

/// The embedded faces standing in for each of the standard fonts. Faces are read on first use and
/// cached for the document's lifetime; [`PdfDoc::set_unicode_font`](crate::PdfDoc::set_unicode_font)
/// can supply one instead.
pub(crate) struct Faces {
    slots: Vec<OnceLock<Option<Face>>>,
}

impl Default for Faces {
    fn default() -> Faces {
        Faces { slots: (0..FONT_COUNT).map(|_| OnceLock::new()).collect() }
    }
}

impl Faces {
    /// The face standing in for `font`, loaded from the system on first use.
    pub(crate) fn get(&self, font: Font) -> Option<&Face> {
        if font == Font::Symbol {
            return None;
        }
        self.slots.get(font.index())?.get_or_init(|| system_face(font)).as_ref()
    }

    /// Supplies the face for `font`; false when that slot was already used. A supplied face is used
    /// for drawing, while [`crate::advance`] keeps measuring with the system's face.
    pub(crate) fn set(&self, font: Font, face: Face) -> bool {
        if font == Font::Symbol {
            return false;
        }
        self.slots.get(font.index()).is_some_and(|slot| slot.set(Some(face)).is_ok())
    }
}

/// The face standing in for `font` for measurement ([`crate::advance`]), from one process-wide cache.
pub(crate) fn default_face(font: Font) -> Option<&'static Face> {
    static FACES: OnceLock<Faces> = OnceLock::new();
    FACES.get_or_init(Faces::default).get(font)
}

#[cfg(not(target_arch = "wasm32"))]
fn system_face(font: Font) -> Option<Face> {
    use std::io::Read as _;
    for path in candidates(font) {
        let Ok(file) = std::fs::File::open(&path) else { continue };
        // Read from the open handle with a hard bound: a candidate that is a FIFO or a device can
        // pass a metadata size check and then never reach EOF.
        let mut data = Vec::new();
        if file.take(MAX_FONT_BYTES as u64 + 1).read_to_end(&mut data).is_err() || data.len() > MAX_FONT_BYTES {
            continue;
        }
        if let Some(face) = Face::parse(data, 0) {
            return Some(face);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn system_face(_font: Font) -> Option<Face> {
    None
}

/// Candidate font files for standard font `font`, most preferred first: the metric-compatible
/// family in the requested style (then its regular face), then families with broad Latin coverage.
#[cfg(not(target_arch = "wasm32"))]
fn candidates(font: Font) -> Vec<std::path::PathBuf> {
    let (family, style) = family_and_style(font);
    let dirs = font_dirs();
    let mut out = Vec::new();
    for style in [style, 0] {
        for name in candidate_names(family, style) {
            for dir in &dirs {
                out.push(dir.join(&name));
            }
        }
    }
    out
}

/// The family (0 sans, 1 serif, 2 mono) and style (0 regular, 1 bold, 2 italic, 3 bold italic) of
/// the standard font `font`.
#[cfg(not(target_arch = "wasm32"))]
fn family_and_style(font: Font) -> (usize, usize) {
    let bold = matches!(
        font,
        Font::HelveticaBold | Font::HelveticaBoldOblique | Font::TimesBold | Font::TimesBoldItalic | Font::CourierBold | Font::CourierBoldOblique
    );
    let italic = matches!(
        font,
        Font::HelveticaOblique
            | Font::HelveticaBoldOblique
            | Font::TimesItalic
            | Font::TimesBoldItalic
            | Font::CourierOblique
            | Font::CourierBoldOblique
    );
    let family = match font {
        Font::Helvetica | Font::HelveticaBold | Font::HelveticaOblique | Font::HelveticaBoldOblique => 0,
        Font::TimesRoman | Font::TimesBold | Font::TimesItalic | Font::TimesBoldItalic => 1,
        _ => 2,
    };
    (family, usize::from(bold) + 2 * usize::from(italic))
}

/// File names of `style` for `family`, in preference order: the metric-compatible families first
/// (Windows, then macOS names), then Liberation, DejaVu, Noto and FreeFont where installed.
#[cfg(not(target_arch = "wasm32"))]
fn candidate_names(family: usize, style: usize) -> Vec<String> {
    // The metric-compatible family of the standard faces, per style (regular, bold, italic, bold
    // italic) in Windows names, macOS names, then the Linux packages.
    let windows = [
        ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
        ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
        ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"],
    ];
    let mac_family = ["Arial", "Times New Roman", "Courier New"];
    let mac_style = ["", " Bold", " Italic", " Bold Italic"];
    let liberation = [("LiberationSans", "LiberationSans"), ("LiberationSerif", "LiberationSerif"), ("LiberationMono", "LiberationMono")];
    let liberation_style = ["Regular", "Bold", "Italic", "BoldItalic"];
    let dejavu = [
        ("DejaVuSans", ["", "-Bold", "-Oblique", "-BoldOblique"]),
        ("DejaVuSerif", ["", "-Bold", "-Italic", "-BoldItalic"]),
        ("DejaVuSansMono", ["", "-Bold", "-Oblique", "-BoldOblique"]),
    ];
    let noto = [
        ("NotoSans", ["-Regular", "-Bold", "-Italic", "-BoldItalic"]),
        ("NotoSerif", ["-Regular", "-Bold", "-Italic", "-BoldItalic"]),
        ("NotoSansMono", ["-Regular", "-Bold", "-Italic", "-BoldItalic"]),
    ];
    let free = [
        ("FreeSans", ["", "Bold", "Oblique", "BoldOblique"]),
        ("FreeSerif", ["", "Bold", "Italic", "BoldItalic"]),
        ("FreeMono", ["", "Bold", "Oblique", "BoldOblique"]),
    ];
    let windows_fallback: [[&[&str]; 4]; 3] = [
        [
            &["tahoma.ttf", "segoeui.ttf", "calibri.ttf"],
            &["tahomabd.ttf", "segoeuib.ttf", "calibrib.ttf"],
            &["segoeuii.ttf", "calibrii.ttf", "tahoma.ttf"],
            &["segoeuiz.ttf", "calibriz.ttf", "tahomabd.ttf"],
        ],
        [&["georgia.ttf", "calibri.ttf"], &["georgiab.ttf", "calibrib.ttf"], &["georgiai.ttf", "calibrii.ttf"], &["georgiaz.ttf", "calibriz.ttf"]],
        [&["consola.ttf", "lucon.ttf"], &["consolab.ttf", "luconb.ttf"], &["consolai.ttf", "luconi.ttf"], &["consolaz.ttf", "luconbi.ttf"]],
    ];
    let style = style.min(3);
    let mut out = Vec::with_capacity(8);
    if let (Some(family), Some(name)) = (windows.get(family), mac_family.get(family)) {
        if let Some(file) = family.get(style) {
            out.push((*file).to_string());
        }
        out.push(format!("{name}{}.ttf", mac_style.get(style).copied().unwrap_or("")));
    }
    if let (Some((family, _)), Some(suffix)) = (liberation.get(family), liberation_style.get(style)) {
        out.push(format!("{family}-{suffix}.ttf"));
    }
    if let Some((family, styles)) = dejavu.get(family) {
        out.push(format!("{family}{}.ttf", styles.get(style).copied().unwrap_or("")));
    }
    if let Some((family, styles)) = noto.get(family) {
        out.push(format!("{family}{}.ttf", styles.get(style).copied().unwrap_or("-Regular")));
    }
    if let Some((family, styles)) = free.get(family) {
        out.push(format!("{family}{}.ttf", styles.get(style).copied().unwrap_or("")));
    }
    if let Some(names) = windows_fallback.get(family).and_then(|styles| styles.get(style)) {
        out.extend(names.iter().map(|name| (*name).to_string()));
    }
    if style == 0 {
        // Last resorts with broad coverage: macOS's collection and the pan-Unicode face.
        out.push("Helvetica.ttc".to_string());
        out.push("Arial Unicode.ttf".to_string());
    }
    out
}

/// Directories that hold the candidate font files, best first.
#[cfg(not(target_arch = "wasm32"))]
fn font_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
        dirs.push(PathBuf::from(root).join("Fonts"));
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(root).join("Microsoft/Windows/Fonts"));
    }
    for dir in ["/System/Library/Fonts/Supplemental", "/Library/Fonts", "/System/Library/Fonts"] {
        dirs.push(dir.into());
    }
    for dir in [
        "/usr/share/fonts/truetype/liberation",
        "/usr/share/fonts/truetype/liberation2",
        "/usr/share/fonts/liberation",
        "/usr/share/fonts/liberation-sans",
        "/usr/share/fonts/liberation-serif",
        "/usr/share/fonts/liberation-mono",
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/dejavu",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/noto",
        "/usr/share/fonts/truetype/freefont",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/usr/local/share/fonts/TTF",
    ] {
        dirs.push(dir.into());
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs
}

#[cfg(test)]
pub(crate) mod synthetic {
    //! A tiny, deterministic TrueType font built in code, so the embedding tests need no system
    //! font and no font file committed to the repository.

    /// A font whose character map holds `chars` (character and glyph id, glyph 0 being `.notdef`),
    /// with one simple rectangular outline per glyph and an advance of 700 font units. Two
    /// characters may name the same glyph, as some faces do for look-alike letters.
    pub(crate) fn font(chars: &[(char, u16)]) -> Vec<u8> {
        let glyphs = chars.iter().map(|(_, glyph)| usize::from(*glyph)).max().unwrap_or(0) + 1;
        let outlines: Vec<Vec<u8>> = (0..glyphs).map(|glyph| if glyph == 0 { Vec::new() } else { rectangle() }).collect();
        build(chars, &outlines)
    }

    /// A font whose glyph 1 is a composite referring to glyph 2, which refers to glyph 3, and so on
    /// down a chain of 12 glyphs; the last one is a simple rectangle. Used to check that a subset
    /// keeps the whole component closure however deep the chain is.
    pub(crate) fn composite_chain() -> Vec<u8> {
        let mut outlines: Vec<Vec<u8>> = vec![Vec::new()];
        for glyph in 1..=11u16 {
            outlines.push(composite(glyph + 1));
        }
        outlines.push(rectangle());
        build(&[('ạ', 1)], &outlines)
    }

    /// A font whose glyph 1 is one composite with `count` references to the simple glyph 2. OpenType
    /// does not cap the number of component records, so a fixed traversal limit would refuse a
    /// valid composite.
    pub(crate) fn many_components(count: usize) -> Vec<u8> {
        let mut composite = Vec::new();
        composite.extend_from_slice(&(-1i16).to_be_bytes()); // composite
        for value in [0i16, 0, 600, 700] {
            composite.extend_from_slice(&value.to_be_bytes()); // bbox
        }
        for index in 0..count {
            let more = if index + 1 < count { 0x0020u16 } else { 0 };
            composite.extend_from_slice(&(0x0002u16 | more).to_be_bytes()); // ARGS_ARE_XY_VALUES
            composite.extend_from_slice(&2u16.to_be_bytes());
            composite.extend_from_slice(&[0, 0]);
        }
        build(&[('ạ', 1)], &[Vec::new(), composite, rectangle()])
    }

    /// One composite glyph drawing `component` at the same offset.
    fn composite(component: u16) -> Vec<u8> {
        let mut glyph = Vec::new();
        glyph.extend_from_slice(&(-1i16).to_be_bytes()); // composite
        for value in [0i16, 0, 600, 700] {
            glyph.extend_from_slice(&value.to_be_bytes()); // bbox
        }
        glyph.extend_from_slice(&0x0002u16.to_be_bytes()); // ARGS_ARE_XY_VALUES, int8 arguments
        glyph.extend_from_slice(&component.to_be_bytes());
        glyph.extend_from_slice(&[0, 0]);
        glyph
    }

    /// Builds a TrueType file from `chars` and one outline per glyph, each glyph keeping its index.
    fn build(chars: &[(char, u16)], outlines: &[Vec<u8>]) -> Vec<u8> {
        let glyphs = outlines.len();
        let mut glyf = Vec::new();
        let mut loca: Vec<u8> = Vec::new();
        let mut hmtx: Vec<u8> = Vec::new();
        for outline in outlines {
            loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
            hmtx.extend_from_slice(&700u16.to_be_bytes());
            hmtx.extend_from_slice(&0i16.to_be_bytes());
            glyf.extend_from_slice(outline);
        }
        // The character map groups must be sorted by character, which need not be glyph order.
        let mut groups: Vec<(char, u16)> = chars.to_vec();
        groups.sort_by_key(|(character, _)| *character);
        groups.dedup_by_key(|(character, _)| *character);
        let mut cmap = Vec::new();
        for (character, glyph) in groups {
            cmap.extend_from_slice(&u32::from(character).to_be_bytes());
            cmap.extend_from_slice(&u32::from(character).to_be_bytes());
            cmap.extend_from_slice(&u32::from(glyph).to_be_bytes());
        }
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cmap", cmap_table(&cmap)),
            (*b"glyf", glyf),
            (*b"head", head_table()),
            (*b"hhea", hhea_table(glyphs as u16)),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp_table(glyphs as u16)),
            (*b"post", super::post_table(0.0, false)),
        ];
        tables.sort_by_key(|(tag, _)| *tag);
        super::sfnt(&tables).unwrap_or_default()
    }

    /// One simple glyph: a square from (100, 0) to (600, 700).
    fn rectangle() -> Vec<u8> {
        let mut glyph = Vec::new();
        glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
        for value in [100i16, 0, 600, 700] {
            glyph.extend_from_slice(&value.to_be_bytes()); // bbox
        }
        glyph.extend_from_slice(&3u16.to_be_bytes()); // end point of the contour
        glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
        glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // four on-curve points, full words
        for x in [100i16, 600, 600, 100] {
            glyph.extend_from_slice(&x.to_be_bytes());
        }
        for y in [0i16, 0, 700, 700] {
            glyph.extend_from_slice(&y.to_be_bytes());
        }
        glyph
    }

    fn head_table() -> Vec<u8> {
        let mut table = vec![0u8; 54];
        table[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        table[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        table[18..20].copy_from_slice(&1000u16.to_be_bytes());
        table[50..52].copy_from_slice(&1u16.to_be_bytes());
        table
    }

    fn hhea_table(glyphs: u16) -> Vec<u8> {
        let mut table = vec![0u8; 36];
        table[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        table[4..6].copy_from_slice(&800i16.to_be_bytes());
        table[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        table[10..12].copy_from_slice(&700u16.to_be_bytes());
        table[18..20].copy_from_slice(&1i16.to_be_bytes());
        table[34..36].copy_from_slice(&glyphs.to_be_bytes());
        table
    }

    fn maxp_table(glyphs: u16) -> Vec<u8> {
        let mut table = vec![0u8; 32];
        table[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        table[4..6].copy_from_slice(&glyphs.to_be_bytes());
        table
    }

    fn cmap_table(groups: &[u8]) -> Vec<u8> {
        let count = groups.len() / 12;
        let length = 16 + groups.len();
        let mut table = Vec::new();
        table.extend_from_slice(&0u16.to_be_bytes());
        table.extend_from_slice(&1u16.to_be_bytes());
        table.extend_from_slice(&3u16.to_be_bytes());
        table.extend_from_slice(&10u16.to_be_bytes());
        table.extend_from_slice(&(12u32).to_be_bytes());
        table.extend_from_slice(&12u16.to_be_bytes());
        table.extend_from_slice(&0u16.to_be_bytes());
        table.extend_from_slice(&(length as u32).to_be_bytes());
        table.extend_from_slice(&0u32.to_be_bytes());
        table.extend_from_slice(&(count as u32).to_be_bytes());
        table.extend_from_slice(groups);
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vietnamese_face() -> Face {
        let data = synthetic::font(&[('ệ', 1), ('ạ', 2), ('đ', 3)]);
        Face::parse(data, 0).expect("synthetic face parses")
    }

    #[test]
    fn a_synthetic_face_maps_its_characters_to_glyphs_and_widths() {
        let face = vietnamese_face();
        assert_eq!(face.glyph('ệ'), Some(1));
        assert_eq!(face.glyph('ạ'), Some(2));
        assert_eq!(face.glyph('đ'), Some(3));
        assert_eq!(face.glyph('a'), None, "a character the face lacks has no glyph");
        assert_eq!(face.glyph('中'), None);
        assert_eq!(face.width(1), 700);
    }

    #[test]
    fn the_subset_keeps_glyph_ids_and_is_a_valid_font() {
        let face = vietnamese_face();
        let mut usage = Usage::new();
        usage.insert(2, 'ạ');
        let subset = face.subset(&usage).expect("subset builds");
        assert!(subset.len() < 4096, "only the used outline is kept: {} bytes", subset.len());
        let font = FontRef::from_index(&subset, 0).expect("subset parses as a font");
        let head = font.table_data(Tag::new(b"head")).expect("head kept").as_bytes();
        let maxp = font.table_data(Tag::new(b"maxp")).expect("maxp kept").as_bytes();
        let loca = font.table_data(Tag::new(b"loca")).expect("loca kept").as_bytes();
        assert_eq!(be16(head, 50), Some(1), "long loca offsets");
        assert_eq!(be16(maxp, 4), Some(4), "every glyph id is kept");
        // Glyph 2 has an outline, glyph 1 (a character that was not used) does not.
        let offset = |glyph: usize| be32(loca, glyph * 4).unwrap_or_default();
        assert!(offset(3) > offset(2));
        assert_eq!(offset(2), offset(1));
        assert_eq!(offset(1), offset(0));
        // The character map maps the used character to its glyph.
        let font = FontRef::from_index(&subset, 0).expect("subset parses");
        assert_eq!(font.charmap().map('ạ').map(|glyph| glyph.to_u32()), Some(2));
    }

    #[test]
    fn a_hostile_font_is_refused_without_panicking() {
        assert!(Face::parse(Vec::new(), 0).is_none());
        assert!(Face::parse(vec![0xFF; 64], 0).is_none());
        assert!(Face::parse(vec![0u8; 5_000_000], 0).is_none());
        let mut data = synthetic::font(&[('ạ', 1)]);
        data.truncate(data.len() / 2);
        assert!(Face::parse(data, 7).is_none(), "a broken face and a missing collection are refused");
    }

    #[test]
    fn subset_tags_are_letters_and_depend_on_the_glyphs_used() {
        let mut usage = Usage::new();
        usage.insert(1, 'ệ');
        let tag = subset_tag(&usage);
        assert_eq!(tag.len(), 6);
        assert!(tag.chars().all(|c| c.is_ascii_uppercase()), "{tag}");
        usage.insert(2, 'ạ');
        assert_ne!(subset_tag(&usage), tag);
    }

    #[test]
    fn to_unicode_maps_each_glyph_to_its_character() {
        let mut usage = Usage::new();
        usage.insert(1, 'ệ');
        usage.insert(2, 'ạ');
        let cmap = to_unicode_cmap(&usage);
        assert!(cmap.contains("<0001> <1EC7>"));
        assert!(cmap.contains("<0002> <1EA1>"));
        assert!(cmap.contains("endbfchar"));
    }
}
