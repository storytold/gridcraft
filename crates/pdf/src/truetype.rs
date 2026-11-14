//! Just enough TrueType parsing to embed a system font for text the standard 14 fonts can't
//! show (Greek, Cyrillic, …): units per em, metrics, the character map and advance widths.
//! Every read is bounds-checked; a malformed or unsupported file is rejected, never trusted.

/// Largest font file accepted for embedding.
pub const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;

/// A parsed TrueType (glyf-outline) font, embedded whole as a CID font.
#[derive(Debug)]
pub struct UnicodeFont {
    pub(crate) data: Vec<u8>,
    units_per_em: u16,
    /// FontBBox, ascent, descent and cap height in 1/1000 em.
    pub(crate) bbox: [i32; 4],
    pub(crate) ascent: i32,
    pub(crate) descent: i32,
    pub(crate) cap_height: i32,
    num_glyphs: u16,
    hmtx: usize,
    num_h_metrics: u16,
    cmap: Cmap,
}

#[derive(Debug, Clone, Copy)]
enum Cmap {
    /// Offset of a format 4 subtable (BMP).
    Format4(usize, usize),
    /// Offset of a format 12 subtable (full Unicode).
    Format12(usize, usize),
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o.checked_add(1)?)?]))
}

fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    u16_at(d, o).map(|v| v as i16)
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*d.get(o)?, *d.get(o.checked_add(1)?)?, *d.get(o.checked_add(2)?)?, *d.get(o.checked_add(3)?)?]))
}

/// Offset and length of table `tag`, when it lies inside the file.
fn table(d: &[u8], tag: &[u8; 4]) -> Option<(usize, usize)> {
    let count = usize::from(u16_at(d, 4)?);
    for i in 0..count {
        let rec = 12usize.checked_add(i.checked_mul(16)?)?;
        if d.get(rec..rec.checked_add(4)?)? == tag {
            let off = u32_at(d, rec.checked_add(8)?)? as usize;
            let len = u32_at(d, rec.checked_add(12)?)? as usize;
            d.get(off..off.checked_add(len)?)?;
            return Some((off, len));
        }
    }
    None
}

impl UnicodeFont {
    /// Parses a `.ttf` file. `None` for CFF-flavoured (`OTTO`) or collection files, fonts whose
    /// licence forbids embedding (`fsType` restricted), and anything malformed.
    pub fn parse(data: Vec<u8>) -> Option<UnicodeFont> {
        if data.len() > MAX_FONT_BYTES {
            return None;
        }
        let version = u32_at(&data, 0)?;
        if version != 0x0001_0000 && data.get(0..4)? != b"true" {
            return None;
        }
        // Validate every directory record, including optional licensing tables. An invalid
        // OS/2 offset must not be mistaken for a font without embedding restrictions.
        let count = usize::from(u16_at(&data, 4)?);
        let directory = data.get(12..12usize.checked_add(count.checked_mul(16)?)?)?;
        for record in directory.as_chunks::<16>().0 {
            let offset = u32_at(record, 8)? as usize;
            let length = u32_at(record, 12)? as usize;
            data.get(offset..offset.checked_add(length)?)?;
        }
        let (head, head_len) = table(&data, b"head")?;
        let (hhea, hhea_len) = table(&data, b"hhea")?;
        let (maxp, maxp_len) = table(&data, b"maxp")?;
        if head_len < 54 || hhea_len < 36 || maxp_len < 6 {
            return None;
        }
        let (hmtx, hmtx_len) = table(&data, b"hmtx")?;
        table(&data, b"glyf")?;
        table(&data, b"loca")?;
        let units_per_em = u16_at(&data, head + 18).filter(|u| (16..=16384).contains(u))?;
        let num_glyphs = u16_at(&data, maxp + 4)?;
        let num_h_metrics = u16_at(&data, hhea + 34).filter(|n| *n > 0 && *n <= num_glyphs)?;
        if usize::from(num_h_metrics) * 4 + usize::from(num_glyphs - num_h_metrics) * 2 > hmtx_len {
            return None;
        }
        let scale = |v: i16| i32::from(v) * 1000 / i32::from(units_per_em);
        let bbox =
            [scale(i16_at(&data, head + 36)?), scale(i16_at(&data, head + 38)?), scale(i16_at(&data, head + 40)?), scale(i16_at(&data, head + 42)?)];
        let ascent = scale(i16_at(&data, hhea + 4)?);
        let descent = scale(i16_at(&data, hhea + 6)?);
        let mut cap_height = ascent;
        if let Some((os2, os2_len)) = table(&data, b"OS/2") {
            // fsType: restricted licence or bitmap-only embedding forbids these outlines.
            if os2_len < 10 || u16_at(&data, os2 + 8)? & 0x0202 != 0 {
                return None;
            }
            if u16_at(&data, os2)? >= 2 && os2_len >= 90 {
                cap_height = scale(i16_at(&data, os2 + 88)?);
            }
        }
        let cmap = find_cmap(&data)?;
        Some(UnicodeFont { data, units_per_em, bbox, ascent, descent, cap_height, num_glyphs, hmtx, num_h_metrics, cmap })
    }

    /// The glyph for `c`, or 0 (`.notdef`) when the font lacks it.
    pub fn glyph(&self, c: char) -> u16 {
        let g = match self.cmap {
            Cmap::Format4(o, len) => self.data.get(o..o + len).and_then(|d| format4(d, 0, c as u32)),
            Cmap::Format12(o, len) => self.data.get(o..o + len).and_then(|d| format12(d, 0, c as u32)),
        };
        g.filter(|g| *g < self.num_glyphs).unwrap_or(0)
    }

    /// Whether the font has a glyph for `c`.
    pub fn has(&self, c: char) -> bool {
        self.glyph(c) != 0
    }

    /// Advance width of glyph `g` in 1/1000 em.
    pub fn glyph_width(&self, g: u16) -> u16 {
        let i = usize::from(g.min(self.num_h_metrics - 1));
        let adv = u16_at(&self.data, self.hmtx + i * 4).unwrap_or(0);
        (u32::from(adv) * 1000 / u32::from(self.units_per_em)).min(u32::from(u16::MAX)) as u16
    }

    /// Advance width of `c` in 1/1000 em.
    pub fn char_width(&self, c: char) -> u16 {
        self.glyph_width(self.glyph(c))
    }
}

/// The best Unicode character map: full-repertoire format 12, else a BMP format 4.
fn find_cmap(d: &[u8]) -> Option<Cmap> {
    let (offset, length) = table(d, b"cmap")?;
    let d = d.get(offset..offset.checked_add(length)?)?;
    let count = usize::from(u16_at(d, 2)?);
    d.get(4..4usize.checked_add(count.checked_mul(8)?)?)?;
    let mut bmp = None;
    for i in 0..count.min(64) {
        let rec = 4 + i * 8;
        let (platform, encoding) = (u16_at(d, rec)?, u16_at(d, rec + 2)?);
        let sub = u32_at(d, rec + 4)? as usize;
        if !(platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10))) {
            continue;
        }
        // Work only inside the declared cmap and subtable bounds, including on 32-bit WASM.
        let bytes = d.get(sub..)?;
        match u16_at(bytes, 0) {
            Some(12) => {
                let len = u32_at(bytes, 4)? as usize;
                bytes.get(..len)?;
                let groups = u32_at(bytes, 12)? as usize;
                if groups > (1 << 20) || groups.checked_mul(12)?.checked_add(16)? > len {
                    return None;
                }
                return Some(Cmap::Format12(offset + sub, len));
            }
            Some(4) if bmp.is_none() => {
                let len = usize::from(u16_at(bytes, 2)?);
                bytes.get(..len)?;
                let seg_x2 = usize::from(u16_at(bytes, 6)?);
                if seg_x2 == 0 || seg_x2 % 2 != 0 || 16 + seg_x2 * 4 > len {
                    return None;
                }
                bmp = Some(Cmap::Format4(offset + sub, len));
            }
            _ => {}
        }
    }
    bmp
}

fn format4(d: &[u8], o: usize, c: u32) -> Option<u16> {
    let c = u16::try_from(c).ok()?;
    let seg_x2 = usize::from(u16_at(d, o + 6)?);
    let ends = o + 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    // Segments are sorted by end code: binary search for the first end >= c.
    let (mut lo, mut hi) = (0usize, seg_x2 / 2);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if u16_at(d, ends + mid * 2)? < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let seg = lo;
    if seg >= seg_x2 / 2 {
        return None;
    }
    let start = u16_at(d, starts + seg * 2)?;
    if c < start {
        return None;
    }
    let delta = u16_at(d, deltas + seg * 2)?;
    let range_pos = ranges + seg * 2;
    let range = usize::from(u16_at(d, range_pos)?);
    if range == 0 {
        return Some(c.wrapping_add(delta));
    }
    let g = u16_at(d, range_pos + range + usize::from(c - start) * 2)?;
    (g != 0).then(|| g.wrapping_add(delta))
}

fn format12(d: &[u8], o: usize, c: u32) -> Option<u16> {
    let groups = u32_at(d, o + 12)? as usize;
    let (mut lo, mut hi) = (0usize, groups.min(1 << 20));
    while lo < hi {
        let mid = (lo + hi) / 2;
        let g = o + 16 + mid * 12;
        let (start, end) = (u32_at(d, g)?, u32_at(d, g + 4)?);
        if c < start {
            hi = mid;
        } else if c > end {
            lo = mid + 1;
        } else {
            return u16::try_from(u32_at(d, g + 8)?.checked_add(c - start)?).ok();
        }
    }
    None
}
