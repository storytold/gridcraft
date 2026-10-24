//! OpenType font validation, shaping, and script-aware face lookup for PDF output.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, OnceLock},
};

use fontdb::{Database, Family, Query, Style, Weight};
use harfrust::{FontRef, ShapeOptions, ShaperData, UnicodeBuffer};
use ttf_parser::Face;
use unicode_segmentation::UnicodeSegmentation;

const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_COVERAGE_SCALARS: usize = 256;
const MAX_LOOKUP_CACHE_ENTRIES: usize = 512;
const MAX_LOOKUP_CACHE_BYTES: usize = 64 * 1024;
const MAX_LOADED_FACE_CACHE_ENTRIES: usize = 128;
const MAX_LOADED_FACE_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SCALAR_COVERAGE_ENTRIES: usize = 4096;
const SCALAR_COVERAGE_WEIGHT: usize = std::mem::size_of::<(char, bool)>();
const MAX_SCALAR_PROBES_PER_MISS: usize = 8;
const MAX_COLLECTION_FACES: u32 = 128;

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

fn collection_face_count(bytes: &[u8]) -> u32 {
    if !bytes.get(..4).is_some_and(|magic| magic == b"ttcf") {
        return 1;
    }
    let Some(header) = bytes.get(8..12) else {
        return 0;
    };
    let Ok(header) = <[u8; 4]>::try_from(header) else {
        return 0;
    };
    let count = u32::from_be_bytes(header);
    if count == 0 || count > MAX_COLLECTION_FACES { 0 } else { count }
}

/// A validated, embeddable sfnt face. Font bytes remain shared with the PDF writer.
#[derive(Clone)]
pub struct PdfFont {
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) face_index: u32,
    pub(crate) family: String,
    pub(crate) postscript_name: String,
    pub(crate) units_per_em: u16,
    pub(crate) ascent: i16,
    pub(crate) descent: i16,
    pub(crate) bbox: [i16; 4],
    pub(crate) fs_type: u16,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) italic_angle: f32,
    identity: Arc<()>,
    coverage: Arc<Mutex<CoverageCache>>,
    shaper_data: Arc<OnceLock<Arc<ShaperData>>>,
}

#[derive(Default)]
struct CoverageCache {
    glyphs: HashMap<char, bool>,
    order: std::collections::VecDeque<char>,
    hits: usize,
    misses: usize,
    parses: usize,
}

impl fmt::Debug for PdfFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfFont")
            .field("family", &self.family)
            .field("postscript_name", &self.postscript_name)
            .field("units_per_em", &self.units_per_em)
            .field("bold", &self.bold)
            .field("italic", &self.italic)
            .finish_non_exhaustive()
    }
}

impl CoverageCache {
    fn insert(&mut self, ch: char, covered: bool) {
        if self.glyphs.contains_key(&ch) {
            return;
        }
        if self.glyphs.len() == MAX_COVERAGE_SCALARS
            && let Some(oldest) = self.order.pop_front()
        {
            self.glyphs.remove(&oldest);
        }
        self.order.push_back(ch);
        self.glyphs.insert(ch, covered);
    }
}

/// Shaped glyph and source-text positions, in font units and source UTF-8 byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub cluster: usize,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

impl PdfFont {
    /// Validates a single TrueType/OpenType face and rejects restricted or bitmap-only embedding.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>, face_index: u32) -> Result<Self, String> {
        let mut bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err(format!("font face is empty or larger than {MAX_FONT_BYTES} bytes"));
        }
        if bytes.get(..4) == Some(b"ttcf") {
            bytes = Arc::<[u8]>::from(extract_collection_face(&bytes, face_index)?);
        } else {
            if face_index != 0 {
                return Err(format!("font is not a collection; face index {face_index} is invalid (only face index 0 is supported)"));
            }
            if !matches!(bytes.get(..4), Some(b"\0\x01\0\0" | b"true" | b"typ1" | b"OTTO")) {
                return Err("unsupported font file type; expected a TrueType/OpenType font or font collection".to_string());
            }
        }
        let face = Face::parse(&bytes, 0).map_err(|e| format!("invalid OpenType face: {e:?}"))?;
        if face.tables().glyf.is_none() {
            return Err("PDF embedding currently requires TrueType `glyf` outlines; this face uses another outline format".to_string());
        }
        let fs_type = fs_type(&bytes, 0).unwrap_or(0);
        let family = face
            .names()
            .into_iter()
            .filter(|name| name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .find_map(|name| name.to_string())
            .or_else(|| face.names().into_iter().filter(|name| name.name_id == ttf_parser::name_id::FAMILY).find_map(|name| name.to_string()))
            .unwrap_or_else(|| "Unknown".to_string());
        if fs_type & 0x0002 != 0 || fs_type & 0x0200 != 0 {
            return Err(format!("font face `{family}` forbids PDF embedding (OS/2 fsType {fs_type:#06x})"));
        }
        let postscript_name = face
            .names()
            .into_iter()
            .filter(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .find_map(|name| name.to_string())
            .unwrap_or_else(|| family.clone());
        let units_per_em = face.units_per_em();
        let ascent = face.ascender();
        let descent = face.descender();
        let bold = face.is_bold();
        let italic = face.is_italic();
        let italic_angle = face.italic_angle();
        let bounds = face.global_bounding_box();
        let bbox = [bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max];
        Ok(Self {
            bytes,
            face_index: 0,
            family,
            postscript_name,
            units_per_em,
            ascent,
            descent,
            bbox,
            fs_type,
            bold,
            italic,
            italic_angle,
            identity: Arc::new(()),
            coverage: Arc::new(Mutex::new(CoverageCache::default())),
            shaper_data: Arc::new(OnceLock::new()),
        })
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn postscript_name(&self) -> &str {
        &self.postscript_name
    }

    pub fn embedding_permissions(&self) -> u16 {
        self.fs_type
    }

    pub fn is_bold(&self) -> bool {
        self.bold
    }

    pub fn is_italic(&self) -> bool {
        self.italic
    }

    /// Returns true only for clones of this validated font-face instance.
    pub fn same_face(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    /// Conservative line extents from the font's full glyph bounds, in points.
    pub fn vertical_extents(&self, size: f32) -> (f32, f32) {
        let scale = size / f32::from(self.units_per_em.max(1));
        (
            (f32::from(self.bbox[3]).max(f32::from(self.ascent)) * scale).max(0.0),
            ((-f32::from(self.bbox[1])).max(-f32::from(self.descent)) * scale).max(0.0),
        )
    }

    /// Shapes text using the OpenType tables in this face. Cluster values map back to UTF-8 bytes.
    pub fn shape(&self, text: &str) -> Result<Vec<PositionedGlyph>, String> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let font = FontRef::from_index(&self.bytes, self.face_index).map_err(|e| format!("cannot shape `{}`: {e:?}", self.family))?;
        let shaper_data = self.shaper_data.get_or_init(|| Arc::new(ShaperData::new(&font)));
        let shaper = shaper_data.shaper(&font).build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, ShapeOptions::new());
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        if infos.len() != positions.len() {
            return Err("shaper returned mismatched glyph and position arrays".to_string());
        }
        Ok(infos
            .iter()
            .zip(positions)
            .map(|(info, pos)| PositionedGlyph {
                glyph_id: info.glyph_id,
                cluster: (info.cluster as usize).min(text.len()),
                x_advance: pos.x_advance,
                y_advance: pos.y_advance,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
            })
            .collect())
    }

    pub fn text_width(&self, text: &str, size: f32) -> Result<f32, String> {
        let advance: i64 = self.shape(text)?.iter().map(|glyph| i64::from(glyph.x_advance)).sum();
        Ok(advance as f32 * size / f32::from(self.units_per_em))
    }

    pub fn covers(&self, text: &str) -> bool {
        let mut cache = self.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut missing = Vec::new();
        for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
            if let Some(covered) = cache.glyphs.get(&ch).copied() {
                cache.hits = cache.hits.saturating_add(1);
                if !covered {
                    return false;
                }
            } else {
                cache.misses = cache.misses.saturating_add(1);
                if !missing.contains(&ch) {
                    missing.push(ch);
                }
            }
        }
        if missing.is_empty() {
            return true;
        }
        let Ok(face) = Face::parse(&self.bytes, self.face_index) else { return false };
        cache.parses = cache.parses.saturating_add(1);
        let mut covered = true;
        for ch in missing {
            let has_glyph = face.glyph_index(ch).is_some_and(|glyph| glyph.0 != 0);
            cache.insert(ch, has_glyph);
            covered &= has_glyph;
        }
        covered
    }
}

fn collection_face_offset(data: &[u8], index: u32) -> Option<usize> {
    if data.get(..4)? != b"ttcf" {
        return (index == 0).then_some(0);
    }
    let num = usize::try_from(u32::from_be_bytes(data.get(8..12)?.try_into().ok()?)).ok()?;
    if usize::try_from(index).ok()? >= num {
        return None;
    }
    let offset = 12usize.checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?;
    let face_offset = usize::try_from(u32::from_be_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?)).ok()?;
    let header_end = face_offset.checked_add(12)?;
    let header = data.get(face_offset..header_end)?;
    let table_count = usize::from(u16::from_be_bytes([*header.get(4)?, *header.get(5)?]));
    if table_count == 0 || table_count > 256 {
        return None;
    }
    let directory_end = header_end.checked_add(table_count.checked_mul(16)?)?;
    data.get(face_offset..directory_end)?;
    Some(face_offset)
}

fn extract_collection_face(data: &[u8], index: u32) -> Result<Vec<u8>, String> {
    let version = data
        .get(4..8)
        .and_then(|raw| <[u8; 4]>::try_from(raw).ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| format!("truncated font collection header while validating face index {index}"))?;
    if !matches!(version, 0x0001_0000 | 0x0002_0000) {
        return Err(format!("font collection has unsupported version {version:#010x} for face index {index}"));
    }
    let face_count = data
        .get(8..12)
        .and_then(|raw| <[u8; 4]>::try_from(raw).ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| format!("truncated font collection header while validating face index {index}"))?;
    if index >= face_count {
        return Err(format!("font collection has {face_count} faces; requested face index {index} is out of range"));
    }
    let face_offset = collection_face_offset(data, index)
        .ok_or_else(|| format!("font collection face index {index} has a truncated or invalid offset/table directory"))?;
    let count = usize::from(u16::from_be_bytes(
        data.get(
            face_offset.checked_add(4).ok_or("font collection face offset overflow")?
                ..face_offset.checked_add(6).ok_or("font collection face offset overflow")?,
        )
        .ok_or("truncated font collection face")?
        .try_into()
        .map_err(|_| "truncated font collection face")?,
    ));
    if count == 0 || count > 256 {
        return Err("font collection has an invalid table count".to_string());
    }
    let table_start = face_offset.checked_add(12).ok_or("font table offset overflow")?;
    let mut out = vec![0u8; 12 + count * 16];
    out.get_mut(..12).ok_or("truncated font header")?.copy_from_slice(data.get(face_offset..face_offset + 12).ok_or("truncated font header")?);
    let mut head_offset = None;
    for i in 0..count {
        let src_rec = table_start.checked_add(i.checked_mul(16).ok_or("font table offset overflow")?).ok_or("font table offset overflow")?;
        let rec_end = src_rec.checked_add(16).ok_or("font table offset overflow")?;
        let rec = data.get(src_rec..rec_end).ok_or("truncated font table directory")?;
        let src = usize::try_from(u32::from_be_bytes(
            rec.get(8..12).ok_or("truncated font table record")?.try_into().map_err(|_| "truncated font table record")?,
        ))
        .map_err(|_| "font table offset overflow")?;
        let len = usize::try_from(u32::from_be_bytes(
            rec.get(12..16).ok_or("truncated font table record")?.try_into().map_err(|_| "truncated font table record")?,
        ))
        .map_err(|_| "font table length overflow")?;
        let table_end = src.checked_add(len).ok_or("font table length overflow")?;
        let table = data.get(src..table_end).ok_or("font table lies outside the font collection")?;
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let dst = u32::try_from(out.len()).map_err(|_| "font table offset exceeds OpenType limit")?;
        if rec.get(..4) == Some(b"head") {
            head_offset = Some(out.len());
        }
        let record = 12usize.checked_add(i.checked_mul(16).ok_or("font table offset overflow")?).ok_or("font table offset overflow")?;
        out.get_mut(record..record.checked_add(4).ok_or("output table directory offset overflow")?)
            .ok_or("truncated output table directory")?
            .copy_from_slice(rec.get(..4).ok_or("truncated font table tag")?);
        out.get_mut(
            record.checked_add(4).ok_or("output table directory offset overflow")?
                ..record.checked_add(8).ok_or("output table directory offset overflow")?,
        )
        .ok_or("truncated output table directory")?
        .copy_from_slice(rec.get(4..8).ok_or("truncated font table checksum")?);
        out.get_mut(
            record.checked_add(8).ok_or("output table directory offset overflow")?
                ..record.checked_add(12).ok_or("output table directory offset overflow")?,
        )
        .ok_or("truncated output table directory")?
        .copy_from_slice(&dst.to_be_bytes());
        out.get_mut(
            record.checked_add(12).ok_or("output table directory offset overflow")?
                ..record.checked_add(16).ok_or("output table directory offset overflow")?,
        )
        .ok_or("truncated output table directory")?
        .copy_from_slice(&u32::try_from(len).map_err(|_| "font table length exceeds OpenType limit")?.to_be_bytes());
        out.extend_from_slice(table);
        if out.len() > MAX_FONT_BYTES {
            return Err("extracted collection face exceeds the PDF font limit".to_string());
        }
    }
    let head = head_offset.ok_or("font collection face has no TrueType head table")?;
    out.get_mut(head + 8..head + 12).ok_or("truncated TrueType head table")?.fill(0);
    let checksum = out.chunks(4).fold(0u32, |sum, word| {
        let mut padded = [0u8; 4];
        padded[..word.len()].copy_from_slice(word);
        sum.wrapping_add(u32::from_be_bytes(padded))
    });
    out.get_mut(head + 8..head + 12).ok_or("truncated TrueType head table")?.copy_from_slice(&0xB1B0_AFBAu32.wrapping_sub(checksum).to_be_bytes());
    Ok(out)
}

fn fs_type(data: &[u8], index: u32) -> Option<u16> {
    let mut face_offset = 0usize;
    if data.get(..4)? == b"ttcf" {
        face_offset = collection_face_offset(data, index)?;
    } else if index != 0 {
        return None;
    }
    let count = usize::from(u16::from_be_bytes(data.get(face_offset + 4..face_offset + 6)?.try_into().ok()?));
    let records = face_offset.checked_add(12)?;
    for i in 0..count {
        let record = records.checked_add(i.checked_mul(16)?)?;
        if data.get(record..record + 4)? == b"OS/2" {
            let offset = usize::try_from(u32::from_be_bytes(data.get(record + 8..record + 12)?.try_into().ok()?)).ok()?;
            let table = data.get(offset..offset + 10)?;
            return Some(u16::from_be_bytes([*table.get(8)?, *table.get(9)?]));
        }
    }
    Some(0)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LookupKey {
    family_only: bool,
    family: String,
    bold: bool,
    italic: bool,
    cluster: String,
}

#[derive(Clone, Copy, Debug)]
enum FaceRef {
    Supplied(usize),
    System(fontdb::ID),
}

struct CacheEntry<V> {
    value: V,
    weight: usize,
    stamp: u64,
}

struct BoundedCache<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
    clock: u64,
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> BoundedCache<K, V> {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self { entries: HashMap::new(), bytes: 0, max_entries, max_bytes, clock: 0 }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.stamp = self.clock;
        Some(entry.value.clone())
    }

    fn insert(&mut self, key: K, value: V, weight: usize) {
        if self.max_entries == 0 || weight > self.max_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.weight);
        }
        while self.entries.len() >= self.max_entries || self.bytes.saturating_add(weight) > self.max_bytes {
            let oldest = self.entries.iter().min_by_key(|(_, entry)| entry.stamp).map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(old) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old.weight);
            }
        }
        self.clock = self.clock.saturating_add(1);
        self.bytes = self.bytes.saturating_add(weight);
        self.entries.insert(key, CacheEntry { value, weight, stamp: self.clock });
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[derive(Default, Clone, Copy, Debug)]
struct CatalogCounters {
    lookup_hits: usize,
    lookup_misses: usize,
    system_face_loads: usize,
    system_face_cache_hits: usize,
    system_face_parses: usize,
    coverage_checks: usize,
}

struct CatalogCache {
    lookups: BoundedCache<LookupKey, Option<FaceRef>>,
    loaded_faces: BoundedCache<fontdb::ID, Option<PdfFont>>,
    scalar_coverage: BoundedCache<char, bool>,
    counters: CatalogCounters,
}

impl Default for CatalogCache {
    fn default() -> Self {
        Self {
            lookups: BoundedCache::new(MAX_LOOKUP_CACHE_ENTRIES, MAX_LOOKUP_CACHE_BYTES),
            loaded_faces: BoundedCache::new(MAX_LOADED_FACE_CACHE_ENTRIES, MAX_LOADED_FACE_CACHE_BYTES),
            scalar_coverage: BoundedCache::new(MAX_SCALAR_COVERAGE_ENTRIES, MAX_SCALAR_COVERAGE_ENTRIES * SCALAR_COVERAGE_WEIGHT),
            counters: CatalogCounters::default(),
        }
    }
}

/// Resolves native system faces. WASM callers must use [`PdfFont::from_bytes`] with explicitly
/// supplied font data; WASM has no implicit system-font directory. Face and lookup caches live
/// only as long as this catalog (normally one PDF export).
pub struct FontCatalog {
    db: Database,
    supplied: Vec<PdfFont>,
    cache: Mutex<CatalogCache>,
}

impl FontCatalog {
    pub fn system() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut db = Database::new();
        #[cfg(not(target_arch = "wasm32"))]
        db.load_system_fonts();
        #[cfg(target_arch = "wasm32")]
        let db = Database::new();
        let mut catalog = Self { db, supplied: Vec::new(), cache: Mutex::new(CatalogCache::default()) };
        for (_source, bytes) in CRAFT_FONTS {
            let face_count = collection_face_count(bytes);
            let shared: Arc<[u8]> = Arc::from(*bytes);
            for face_index in 0..face_count {
                let _ = catalog.add_font(Arc::clone(&shared), face_index);
            }
        }
        catalog
    }

    pub fn empty() -> Self {
        Self { db: Database::new(), supplied: Vec::new(), cache: Mutex::new(CatalogCache::default()) }
    }

    pub fn add_font(&mut self, bytes: impl Into<Arc<[u8]>>, face_index: u32) -> Result<(), String> {
        self.supplied.push(PdfFont::from_bytes(bytes, face_index)?);
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.lookups.clear();
        cache.scalar_coverage.clear();
        Ok(())
    }

    pub fn has_family(&self, family: &str) -> bool {
        self.supplied.iter().any(|face| face.family.eq_ignore_ascii_case(family))
            || self.db.faces().any(|face| face.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(family)))
    }

    /// Selects the requested family/style where it covers the cluster, then an installed face
    /// that covers it. Workbook Latin characters therefore remain in the selected family.
    pub fn face_for(&self, family: &str, bold: bool, italic: bool, cluster: &str) -> Result<PdfFont, String> {
        self.lookup_face(family, bold, italic, cluster, false)
    }

    /// Resolves only this family and exact style. Latin callers can use Standard-14 metrics
    /// when that face is missing, disallows embedding, or does not cover the cluster.
    pub fn family_face_for(&self, family: &str, bold: bool, italic: bool, cluster: &str) -> Result<PdfFont, String> {
        self.lookup_face(family, bold, italic, cluster, true)
    }

    fn lookup_face(&self, family: &str, bold: bool, italic: bool, cluster: &str, family_only: bool) -> Result<PdfFont, String> {
        let key_weight = family.len().saturating_add(cluster.len());
        let key = (key_weight <= MAX_LOOKUP_CACHE_BYTES).then(|| LookupKey {
            family_only,
            family: family.to_string(),
            bold,
            italic,
            cluster: cluster.to_string(),
        });
        let cached = {
            let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(key) = key.as_ref() {
                if let Some(face_ref) = cache.lookups.get(key) {
                    cache.counters.lookup_hits = cache.counters.lookup_hits.saturating_add(1);
                    Some(face_ref)
                } else {
                    cache.counters.lookup_misses = cache.counters.lookup_misses.saturating_add(1);
                    None
                }
            } else {
                cache.counters.lookup_misses = cache.counters.lookup_misses.saturating_add(1);
                None
            }
        };
        if let Some(face_ref) = cached {
            return face_ref.map_or_else(|| Err(no_face_error(family, bold, italic, cluster)), |face_ref| self.materialize_face(face_ref));
        }

        let result = if family_only {
            self.resolve_requested_face(family, bold, italic, cluster)?.ok_or_else(|| no_face_error(family, bold, italic, cluster))
        } else if self.has_uncovered_scalar(cluster) {
            Err(no_face_error(family, bold, italic, cluster))
        } else {
            let result = self.resolve_face(family, bold, italic, cluster);
            if result.is_err() {
                self.remember_scalar_coverage(family, bold, italic, cluster);
            }
            result
        };
        if let Some(key) = key {
            let face_ref = result.as_ref().ok().copied();
            self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).lookups.insert(key, face_ref, key_weight);
        }
        result.and_then(|face_ref| self.materialize_face(face_ref))
    }

    /// Coverage proofs outlive evicted font bytes and apply to every requested family/style.
    fn has_uncovered_scalar(&self, cluster: &str) -> bool {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cluster.chars().filter(|ch| !ch.is_whitespace()).any(|ch| cache.scalar_coverage.get(&ch) == Some(false))
    }

    /// A compound miss need not mean any individual scalar is absent. Probe scalars separately
    /// before retaining a negative result; cap extra work for hostile combining sequences.
    fn remember_scalar_coverage(&self, family: &str, bold: bool, italic: bool, cluster: &str) {
        let singleton = cluster.chars().filter(|ch| !ch.is_whitespace()).take(2).count() == 1;
        for ch in cluster.chars().filter(|ch| !ch.is_whitespace()).take(MAX_SCALAR_PROBES_PER_MISS) {
            let cached = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).scalar_coverage.get(&ch);
            if cached.is_some() {
                continue;
            }
            let mut utf8 = [0; 4];
            let covered = !singleton && self.resolve_face(family, bold, italic, ch.encode_utf8(&mut utf8)).is_ok();
            self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).scalar_coverage.insert(ch, covered, SCALAR_COVERAGE_WEIGHT);
            if !covered {
                break;
            }
        }
    }

    fn materialize_face(&self, face_ref: FaceRef) -> Result<PdfFont, String> {
        match face_ref {
            FaceRef::Supplied(index) => {
                self.supplied.get(index).cloned().ok_or_else(|| "cached supplied font face is no longer available".to_string())
            }
            FaceRef::System(id) => self.load(id)?.ok_or_else(|| "cached system font face is no longer embeddable".to_string()),
        }
    }

    fn resolve_requested_face(&self, family: &str, bold: bool, italic: bool, cluster: &str) -> Result<Option<FaceRef>, String> {
        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let style = if italic { Style::Italic } else { Style::Normal };
        let requested = Query { families: &[Family::Name(family)], weight, stretch: fontdb::Stretch::Normal, style };
        if let Some(id) = self.db.query(&requested)
            && self.db.face(id).is_some_and(|face_info| face_info.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(family)))
            && let Some(face) = self.load(id)?
            && face.bold == bold
            && face.italic == italic
            && self.covers(&face, cluster)
        {
            return Ok(Some(FaceRef::System(id)));
        }
        if let Some((index, _)) =
            self.supplied.iter().enumerate().find(|(_, face)| {
                face.family.eq_ignore_ascii_case(family) && face.bold == bold && face.italic == italic && self.covers(face, cluster)
            })
        {
            return Ok(Some(FaceRef::Supplied(index)));
        }
        Ok(None)
    }

    fn resolve_face(&self, family: &str, bold: bool, italic: bool, cluster: &str) -> Result<FaceRef, String> {
        if let Some(face) = self.resolve_requested_face(family, bold, italic, cluster)? {
            return Ok(face);
        }
        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let style = if italic { Style::Italic } else { Style::Normal };
        for require_style in [true, false] {
            for face_info in self.db.faces() {
                if require_style && (face_info.weight != weight || face_info.style != style) {
                    continue;
                }
                if let Some(face) = self.load(face_info.id)?
                    && self.covers(&face, cluster)
                {
                    return Ok(FaceRef::System(face_info.id));
                }
            }
        }
        if let Some((index, _)) =
            self.supplied.iter().enumerate().find(|(_, face)| face.bold == bold && face.italic == italic && self.covers(face, cluster))
        {
            return Ok(FaceRef::Supplied(index));
        }
        if let Some((index, _)) = self.supplied.iter().enumerate().find(|(_, face)| self.covers(face, cluster)) {
            return Ok(FaceRef::Supplied(index));
        }
        Err(no_face_error(family, bold, italic, cluster))
    }

    fn load(&self, id: fontdb::ID) -> Result<Option<PdfFont>, String> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(face) = cache.loaded_faces.get(&id) {
            cache.counters.system_face_cache_hits = cache.counters.system_face_cache_hits.saturating_add(1);
            return Ok(face);
        }
        cache.counters.system_face_loads = cache.counters.system_face_loads.saturating_add(1);
        let mut result = None;
        let family_name = self.db.face(id).and_then(|face| face.families.first().map(|(name, _)| name.clone()));
        self.db.with_face_data(id, |data, index| {
            if data.len() <= MAX_FONT_BYTES {
                cache.counters.system_face_parses = cache.counters.system_face_parses.saturating_add(1);
                result = PdfFont::from_bytes(Arc::<[u8]>::from(data), index).ok().map(|mut font| {
                    if let Some(name) = family_name {
                        font.family = name;
                        if font.postscript_name == "Unknown" {
                            font.postscript_name = font.family.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
                        }
                    }
                    font
                });
            }
        });
        let weight = result.as_ref().map_or(1, |face| face.bytes.len().saturating_add(1));
        cache.loaded_faces.insert(id, result.clone(), weight);
        Ok(result)
    }

    fn covers(&self, face: &PdfFont, cluster: &str) -> bool {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.counters.coverage_checks = cache.counters.coverage_checks.saturating_add(1);
        drop(cache);
        face.covers(cluster)
    }
}

fn no_face_error(family: &str, bold: bool, italic: bool, cluster: &str) -> String {
    format!(
        "no installed embeddable font covers `{cluster}` (requested family: `{family}`, bold: {bold}, italic: {italic}); provide licensed font files to the PDF font catalog"
    )
}

/// Returns extended grapheme clusters with stable UTF-8 source offsets.
pub fn clusters(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.grapheme_indices(true)
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn minimal_test_ttf(units_per_em: u16) -> Vec<u8> {
        test_ttf_with_names(units_per_em, &[])
    }

    fn test_ttf_with_names(units_per_em: u16, names: &[(u16, &str)]) -> Vec<u8> {
        let mut head = vec![0; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&units_per_em.to_be_bytes());
        head[50..52].copy_from_slice(&0i16.to_be_bytes());

        let mut maxp = vec![0; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&1u16.to_be_bytes());

        let mut hhea = vec![0; 36];
        hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes());

        let hmtx = [0x01, 0xF4, 0, 0];
        let loca = [0, 0, 0, 0];
        let glyf: [u8; 0] = [];
        let cmap = [
            0, 0, 0, 1, // cmap version and encoding record count
            0, 3, 0, 1, 0, 0, 0, 12, // Windows Unicode BMP encoding record
            0, 4, 0, 0, 0, 24, 0, 0, 0, 2, // format 4 header, two segments
            0, 0, 0, 0, // searchRange, entrySelector, rangeShift
            0xFF, 0xFF, 0, 0, // endCode[] and reservedPad
            0xFF, 0xFF, // startCode[]
            0, 1, // idDelta[]: maps U+FFFF to glyph zero
            0, 0, // idRangeOffset[]
        ];
        let mut name = Vec::new();
        for value in [0u16, names.len() as u16, (6 + names.len() * 12) as u16] {
            name.extend_from_slice(&value.to_be_bytes());
        }
        let mut strings = Vec::new();
        for (id, text) in names {
            let encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            for value in [3, 1, 0x0409, *id, encoded.len() as u16, strings.len() as u16] {
                name.extend_from_slice(&value.to_be_bytes());
            }
            strings.extend_from_slice(&encoded);
        }
        name.extend_from_slice(&strings);
        let tables: [(&[u8; 4], &[u8]); 8] = [
            (b"head", &head),
            (b"maxp", &maxp),
            (b"hhea", &hhea),
            (b"hmtx", &hmtx),
            (b"loca", &loca),
            (b"glyf", &glyf),
            (b"name", &name),
            (b"cmap", &cmap),
        ];
        let count = tables.len();
        let mut out = vec![0; 12 + count * 16];
        out[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        out[4..6].copy_from_slice(&(count as u16).to_be_bytes());
        let mut offset = out.len();
        for (index, (tag, table)) in tables.iter().enumerate() {
            while !offset.is_multiple_of(4) {
                out.push(0);
                offset += 1;
            }
            let record = 12 + index * 16;
            out[record..record + 4].copy_from_slice(*tag);
            out[record + 8..record + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            out[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
            out.extend_from_slice(table);
            offset = out.len();
        }
        out
    }

    #[test]
    fn typographic_family_takes_precedence_over_legacy_family() {
        let named = test_ttf_with_names(1000, &[(1, "Legacy Family"), (16, "Typographic Family")]);
        let font = PdfFont::from_bytes(Arc::<[u8]>::from(named), 0).unwrap();
        assert_eq!(font.family(), "Typographic Family");
        let legacy = test_ttf_with_names(1000, &[(1, "Legacy Family")]);
        let font = PdfFont::from_bytes(Arc::<[u8]>::from(legacy), 0).unwrap();
        assert_eq!(font.family(), "Legacy Family");
    }

    fn two_face_test_ttc() -> Vec<u8> {
        let faces = [minimal_test_ttf(1000), minimal_test_ttf(2000)];
        let header_len = 12 + faces.len() * 4;
        let face_offsets = [header_len, header_len + faces[0].len()];
        let mut out = vec![0; header_len];
        out[0..4].copy_from_slice(b"ttcf");
        out[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        out[8..12].copy_from_slice(&(faces.len() as u32).to_be_bytes());
        for (index, offset) in face_offsets.iter().enumerate() {
            out[12 + index * 4..16 + index * 4].copy_from_slice(&(*offset as u32).to_be_bytes());
        }
        for (face, base) in faces.into_iter().zip(face_offsets) {
            let mut face = face;
            let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
            for index in 0..count {
                let record = 12 + index * 16;
                let old = u32::from_be_bytes(face[record + 8..record + 12].try_into().unwrap_or([0; 4]));
                face[record + 8..record + 12].copy_from_slice(&old.saturating_add(base as u32).to_be_bytes());
            }
            out.extend_from_slice(&face);
        }
        out
    }

    fn glyph_zero_mapped_test_ttf() -> Vec<u8> {
        let mut bytes = minimal_test_ttf(1000);
        let table_count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let record = (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|record| bytes.get(*record..*record + 4) == Some(b"cmap"))
            .expect("test font has a cmap table");
        let offset = usize::try_from(u32::from_be_bytes(bytes[record + 8..record + 12].try_into().expect("cmap offset"))).expect("cmap offset fits");
        let mut format4 = Vec::with_capacity(32);
        format4.extend_from_slice(&4u16.to_be_bytes());
        format4.extend_from_slice(&32u16.to_be_bytes());
        format4.extend_from_slice(&0u16.to_be_bytes());
        format4.extend_from_slice(&4u16.to_be_bytes());
        format4.extend_from_slice(&4u16.to_be_bytes());
        format4.extend_from_slice(&1u16.to_be_bytes());
        format4.extend_from_slice(&0u16.to_be_bytes());
        format4.extend_from_slice(&65u16.to_be_bytes());
        format4.extend_from_slice(&u16::MAX.to_be_bytes());
        format4.extend_from_slice(&0u16.to_be_bytes());
        format4.extend_from_slice(&65u16.to_be_bytes());
        format4.extend_from_slice(&u16::MAX.to_be_bytes());
        format4.extend_from_slice(&(-65i16).to_be_bytes());
        format4.extend_from_slice(&1u16.to_be_bytes());
        format4.extend_from_slice(&0u16.to_be_bytes());
        format4.extend_from_slice(&0u16.to_be_bytes());
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12];
        cmap.extend_from_slice(&format4);
        bytes.truncate(offset);
        bytes.extend_from_slice(&cmap);
        bytes[record + 12..record + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
        bytes
    }

    #[test]
    fn standalone_face_index_is_validated_before_parsing() {
        let bytes = Arc::<[u8]>::from(minimal_test_ttf(1000));
        let face = PdfFont::from_bytes(Arc::clone(&bytes), 0).expect("minimal standalone TrueType face parses");
        assert_eq!(face.units_per_em, 1000);
        for index in [1, 99, u32::MAX] {
            let error = PdfFont::from_bytes(Arc::clone(&bytes), index).unwrap_err();
            assert!(error.contains(&index.to_string()), "error should report requested face index: {error}");
            assert!(error.contains("not a collection"), "error should explain a standalone font has only face zero: {error}");
        }
    }

    #[test]
    fn valid_collection_selects_nonzero_faces_and_reports_bad_indices() {
        let bytes = Arc::<[u8]>::from(two_face_test_ttc());
        assert_eq!(collection_face_count(&bytes), 2, "embedded collections expose every face index");
        assert_eq!(PdfFont::from_bytes(Arc::clone(&bytes), 0).unwrap().units_per_em, 1000);
        assert_eq!(PdfFont::from_bytes(Arc::clone(&bytes), 1).unwrap().units_per_em, 2000);
        for index in [2, u32::MAX] {
            let error = PdfFont::from_bytes(Arc::clone(&bytes), index).unwrap_err();
            assert!(error.contains(&index.to_string()), "error should report requested face index: {error}");
            assert!(error.contains("2 faces"), "error should report actual collection size: {error}");
        }
    }

    #[test]
    fn malformed_collections_return_errors_without_panicking() {
        let mut truncated_offsets = b"ttcf\0\x01\0\0\0\0\0\x02\0\0\0\x14".to_vec();
        assert!(PdfFont::from_bytes(Arc::<[u8]>::from(truncated_offsets.clone()), 1).is_err());
        truncated_offsets.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(PdfFont::from_bytes(Arc::<[u8]>::from(truncated_offsets), 0).is_err());

        let mut bad_offset = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x14".to_vec();
        bad_offset.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(PdfFont::from_bytes(Arc::<[u8]>::from(bad_offset), 0).is_err());

        let mut truncated_directory = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
        truncated_directory.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        truncated_directory.extend_from_slice(&3u16.to_be_bytes());
        assert!(PdfFont::from_bytes(Arc::<[u8]>::from(truncated_directory), 0).is_err());
    }

    #[test]
    fn glyph_zero_is_not_treated_as_covered_character() {
        let bytes = Arc::<[u8]>::from(glyph_zero_mapped_test_ttf());
        let parsed = Face::parse(&bytes, 0).expect("generated test face parses");
        assert_eq!(parsed.glyph_index('A').map(|glyph| glyph.0), Some(0), "fixture maps A to .notdef explicitly");
        let font = PdfFont::from_bytes(Arc::clone(&bytes), 0).expect("generated test face is embeddable");
        assert!(!font.covers("A"), ".notdef is a missing glyph, not font coverage");

        let mut catalog = FontCatalog::empty();
        catalog.add_font(bytes, 0).expect("generated test face is accepted as supplied data");
        assert!(catalog.face_for("Unknown", false, false, "A").is_err(), "the missing glyph must not select the supplied face");
    }

    #[test]
    fn repeated_face_lookup_does_not_repeat_system_load_parse_or_coverage_work() {
        let catalog = FontCatalog::system();
        let first = match catalog.face_for("Calibri", false, false, "a") {
            Ok(face) => face,
            Err(error) => {
                eprintln!("skipping system-font cache regression; no embeddable Latin fallback: {error}");
                return;
            }
        };
        let first_counts = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counters;
        for _ in 0..199 {
            let face = catalog.face_for("Calibri", false, false, "a").expect("cached lookup keeps returning its selected face");
            assert!(first.same_face(&face), "cache returns clones of the same selected face");
        }
        let cache = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(cache.counters.lookup_misses, 1);
        assert_eq!(cache.counters.lookup_hits, 199);
        assert_eq!(cache.counters.system_face_loads, first_counts.system_face_loads);
        assert_eq!(cache.counters.system_face_parses, first_counts.system_face_parses);
        assert_eq!(cache.counters.coverage_checks, first_counts.coverage_checks);
        assert!(cache.loaded_faces.entries.len() <= MAX_LOADED_FACE_CACHE_ENTRIES);
        assert!(cache.loaded_faces.bytes <= MAX_LOADED_FACE_CACHE_BYTES);
        assert!(cache.lookups.entries.len() <= MAX_LOOKUP_CACHE_ENTRIES);
        assert!(cache.lookups.bytes <= MAX_LOOKUP_CACHE_BYTES);
    }

    #[test]
    fn coverage_and_shaper_tables_are_shared_by_face_clones() {
        let catalog = FontCatalog::system();
        let face = match catalog.face_for("Calibri", false, false, "a") {
            Ok(face) => face,
            Err(error) => {
                eprintln!("skipping face-cache regression; no embeddable Latin font: {error}");
                return;
            }
        };
        assert!(face.covers("a"));
        for _ in 0..199 {
            assert!(face.covers("a"));
        }
        let coverage = face.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(coverage.parses, 1, "coverage parsing happens once for a repeated scalar");
        assert_eq!(coverage.misses, 1);
        assert_eq!(coverage.hits, 200, "the initial family lookup also populates coverage for `a`");
        drop(coverage);

        let clone = face.clone();
        assert!(face.shape("a").is_ok());
        assert!(clone.shape("a").is_ok());
        assert!(Arc::ptr_eq(&face.shaper_data, &clone.shaper_data));
        assert!(face.shaper_data.get().is_some(), "shaping tables initialize lazily and persist on the shared face");
    }

    #[test]
    fn adding_a_supplied_font_invalidates_cached_negative_resolution() {
        let source_catalog = FontCatalog::system();
        let source = match source_catalog.face_for("Arial", false, false, "a") {
            Ok(face) => face,
            Err(error) => {
                eprintln!("skipping supplied-font cache invalidation regression; no embeddable font: {error}");
                return;
            }
        };
        let family = source.family.clone();
        let mut catalog = FontCatalog::empty();
        assert!(catalog.face_for(&family, false, false, "a").is_err());
        catalog.add_font(Arc::clone(&source.bytes), 0).expect("validated system face can be supplied");
        let resolved = catalog.face_for(&family, false, false, "a").expect("new supplied face takes precedence after invalidation");
        assert!(resolved.same_face(&catalog.supplied[0]));
        let cache = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(cache.counters.lookup_misses, 2, "the addition clears the prior negative result");
        assert_eq!(cache.counters.lookup_hits, 0);
    }

    #[test]
    fn installed_fallback_precedes_supplied_faces_for_an_unavailable_family() {
        let mut catalog = FontCatalog::system();
        let mut candidates: Vec<(fontdb::ID, PdfFont)> = Vec::new();
        for info in catalog.db.faces() {
            if let Some(face) = catalog.load(info.id).expect("system face loading returns a result")
                && !face.bold
                && !face.italic
                && face.covers("A")
                && info.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(&face.family))
                && candidates.iter().all(|(_, prior)| !prior.family.eq_ignore_ascii_case(&face.family))
            {
                candidates.push((info.id, face));
                if candidates.len() == 2 {
                    break;
                }
            }
        }
        if candidates.len() < 2 {
            eprintln!("skipping installed-font precedence regression; fewer than two embeddable system families are available");
            return;
        }

        let supplied_face = candidates.remove(1).1;
        catalog.add_font(Arc::clone(&supplied_face.bytes), supplied_face.face_index).expect("system font can be re-added as supplied data");

        let mut missing_family = "GridCraft Missing Family".to_string();
        while catalog.has_family(&missing_family) {
            missing_family.push('x');
        }
        let resolved = catalog.resolve_face(&missing_family, false, false, "A").expect("installed family fallback is available");
        assert!(matches!(resolved, FaceRef::System(_)), "installed system faces take precedence over supplied fallback faces");
    }

    #[test]
    fn negative_lookups_are_cached_but_oversized_keys_are_not_retained() {
        let catalog = FontCatalog::empty();
        assert!(catalog.face_for("MissingFamily", false, false, "x").is_err());
        assert!(catalog.face_for("MissingFamily", false, false, "x").is_err());
        let long_cluster = "x".repeat(MAX_LOOKUP_CACHE_BYTES + 1);
        assert!(catalog.face_for("MissingFamily", false, false, &long_cluster).is_err());
        let cache = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(cache.counters.lookup_misses, 2);
        assert_eq!(cache.counters.lookup_hits, 1);
        assert_eq!(cache.lookups.entries.len(), 1);
        assert!(cache.lookups.bytes <= MAX_LOOKUP_CACHE_BYTES);
    }

    #[test]
    fn uncovered_scalars_do_not_rescan_faces_after_lookup_and_face_cache_eviction() {
        let source_catalog = FontCatalog::system();
        let source = match source_catalog.face_for("Arial", false, false, "A") {
            Ok(font) => font,
            Err(error) => {
                eprintln!("skipping catalog scan regression; no embeddable source face: {error}");
                return;
            }
        };
        let mut catalog = FontCatalog::empty();
        catalog.db.load_font_data(source.bytes.to_vec());
        assert!(catalog.db.faces().next().is_some(), "the regression must exercise a real system-font scan");
        assert!(catalog.face_for("Missing", false, false, "A\u{10ffff}").is_err());
        let before = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counters;
        assert!(before.system_face_loads > 0);
        for mark in ['\u{301}', '\u{302}', '\u{303}', '\u{304}'] {
            {
                let mut cache = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                cache.lookups.clear();
                cache.loaded_faces.clear();
            }
            let cluster = format!("\u{10ffff}{mark}");
            assert!(catalog.face_for("Another missing family", true, true, &cluster).is_err());
        }
        let after = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counters;
        assert_eq!(after.coverage_checks, before.coverage_checks, "known uncovered scalars must bypass subsequent face scans");
        assert_eq!(after.system_face_loads, before.system_face_loads, "evicting font bytes must not discard scalar coverage knowledge");
        assert_eq!(after.system_face_parses, before.system_face_parses);
        assert!(catalog.face_for("Missing", false, false, "A").is_ok(), "a failed compound lookup must not poison its covered scalars");
    }

    #[test]
    fn scalar_coverage_cache_is_bounded_and_new_fonts_invalidate_it() {
        let mut catalog = FontCatalog::empty();
        for scalar in 0xF0000..0xF0000 + MAX_SCALAR_COVERAGE_ENTRIES as u32 + 10 {
            let cluster = char::from_u32(scalar).unwrap().to_string();
            assert!(catalog.face_for("Missing", false, false, &cluster).is_err());
        }
        {
            let cache = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(cache.scalar_coverage.entries.len(), MAX_SCALAR_COVERAGE_ENTRIES);
            assert!(cache.scalar_coverage.bytes <= MAX_SCALAR_COVERAGE_ENTRIES * SCALAR_COVERAGE_WEIGHT);
        }
        catalog.add_font(Arc::<[u8]>::from(minimal_test_ttf(1000)), 0).unwrap();
        assert!(catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).scalar_coverage.entries.is_empty());
    }

    #[test]
    fn cff_family_miss_does_not_select_or_poison_another_family() {
        let Some(root) = std::env::var_os("CRAFT_FONTS_DIR") else {
            assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI must exercise an installed CFF family");
            return;
        };
        let path = std::path::PathBuf::from(root).join("fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf");
        if !path.exists() {
            return; // Custom font manifests need not contain this CFF fixture.
        }
        let mut catalog = FontCatalog::empty();
        catalog.db.load_font_data(std::fs::read(path).unwrap());
        assert!(catalog.has_family("Noto Sans CJK SC"));
        for (_, bytes) in CRAFT_FONTS {
            let _ = catalog.add_font(Arc::<[u8]>::from(*bytes), 0);
        }
        if catalog.face_for("Missing", false, false, "ก").is_err() {
            assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI must supply the Thai fallback fixture");
            return;
        }
        let before = catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counters;
        assert!(catalog.family_face_for("Noto Sans CJK SC", false, false, "A").is_err());
        assert!(catalog.family_face_for("Noto Sans CJK SC", true, false, "A").is_err());
        assert_eq!(
            catalog.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counters.coverage_checks,
            before.coverage_checks,
            "an unembeddable Latin family must not scan unrelated faces"
        );
        assert!(catalog.face_for("Noto Sans CJK SC", false, false, "ก").is_ok(), "a family-only miss must not poison script fallback");
    }

    #[test]
    fn bounded_cache_evicts_old_entries_and_never_exceeds_its_budget() {
        let mut cache = BoundedCache::new(2, 8);
        cache.insert(1, "one", 4);
        cache.insert(2, "two", 4);
        assert_eq!(cache.get(&1), Some("one"));
        cache.insert(3, "three", 4);
        assert_eq!(cache.get(&1), Some("one"));
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.bytes <= 8);
        cache.insert(4, "oversized", 9);
        assert_eq!(cache.get(&4), None);
    }

    #[test]
    fn face_identity_is_shared_only_by_clones() {
        let catalog = FontCatalog::system();
        let face = match catalog.face_for("Arial", false, false, "a") {
            Ok(face) => face,
            Err(error) => {
                eprintln!("skipping font identity regression; no embeddable font: {error}");
                return;
            }
        };
        let clone = face.clone();
        let reparsed = PdfFont::from_bytes(Arc::clone(&face.bytes), 0).expect("the same bytes remain valid");
        assert!(face.same_face(&clone));
        assert!(!face.same_face(&reparsed), "independently constructed faces never merge by name or bytes alone");
    }

    #[test]
    fn restricted_embedding_permissions_remain_rejected() {
        let catalog = FontCatalog::system();
        let face = match catalog.face_for("Arial", false, false, "a") {
            Ok(face) => face,
            Err(error) => {
                eprintln!("skipping restricted-font regression; no embeddable system font: {error}");
                return;
            }
        };
        let mut bytes = face.bytes.to_vec();
        let table_count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let os2_offset = (0..table_count)
            .filter_map(|index| {
                let record = 12usize.checked_add(index.checked_mul(16)?)?;
                (bytes.get(record..record + 4)? == b"OS/2")
                    .then(|| usize::try_from(u32::from_be_bytes(bytes.get(record + 8..record + 12)?.try_into().ok()?)).ok())?
            })
            .next()
            .expect("a TrueType OS/2 table supplies embedding permissions");
        bytes[os2_offset + 8..os2_offset + 10].copy_from_slice(&0x0002u16.to_be_bytes());
        let error = PdfFont::from_bytes(Arc::<[u8]>::from(bytes), 0).unwrap_err();
        assert!(error.contains("forbids PDF embedding"), "restricted permissions remain an explicit error: {error}");
    }
}
