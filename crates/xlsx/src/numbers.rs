//! Data-only Apple Numbers (`.numbers`) import. Formula cells become their last saved results;
//! styles, charts, images, merged cells and other Numbers features are deliberately not translated.
//!
//! Clean room: written from the publicly described container layout (a ZIP package whose
//! `Index/*.iwa` parts hold Snappy-framed Protocol Buffers messages) and from observing how
//! documents behave, never from Apple's code or assets. The Snappy block format is specified at
//! <https://github.com/google/snappy/blob/main/format_description.txt> and Protocol Buffers at
//! <https://protobuf.dev/programming-guides/encoding/>. Only the few message fields the import
//! needs are read, by number; everything else is skipped. All fixtures are authored in Rust.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::package::{MAX_ENTRIES, Package};
use crate::{IoError, ReadReport};

/// The part every Numbers package has.
const DOCUMENT_PART: &str = "Index/Document.iwa";
/// Most decompressed bytes in one Snappy chunk (Numbers writes at most 64 KiB).
const MAX_CHUNK: usize = 16 * 1024 * 1024;
/// Most decompressed message bytes kept from one package.
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const MAX_OBJECTS: usize = 2_000_000;
const MAX_CELLS: u64 = 2_000_000;
const MAX_SHEETS: usize = 1024;
const MAX_CELL_TEXT: usize = 32_767;
/// Most decoded text kept for one import.
const MAX_TEXT_BYTES: usize = 256 * 1024 * 1024;
/// Serial of 2001-01-01 (the Numbers date origin) in the 1900 date system.
const ORIGIN_SERIAL: f64 = 36_892.0;
const SECONDS_PER_DAY: f64 = 86_400.0;

// Message type identifiers that matter for a data import.
const T_DOCUMENT: u32 = 1;
const T_SHEET: u32 = 2;
const T_STORAGE: u32 = 2001;
const T_TABLE_INFO: u32 = 6000;
const T_TABLE_MODEL: u32 = 6001;
const T_TILE: u32 = 6002;
const T_DATA_LIST: u32 = 6005;
const T_RICH_PAYLOAD: u32 = 6218;

// Cell kinds in the packed cell storage.
const K_EMPTY: u8 = 0;
const K_NUMBER: u8 = 2;
const K_TEXT: u8 = 3;
const K_DATE: u8 = 5;
const K_BOOL: u8 = 6;
const K_DURATION: u8 = 7;
const K_ERROR: u8 = 8;
const K_RICH: u8 = 9;
const K_CURRENCY: u8 = 10;

/// Optional cell fields in storage order: the flag that announces each and its size in bytes.
const CELL_FIELDS: [(u32, usize); 21] = [
    (0x1, 16),
    (0x2, 8),
    (0x4, 8),
    (0x8, 4),
    (0x10, 4),
    (0x20, 4),
    (0x40, 4),
    (0x80, 4),
    (0x100, 4),
    (0x200, 4),
    (0x400, 4),
    (0x800, 4),
    (0x1000, 4),
    (0x2000, 4),
    (0x4000, 4),
    (0x8000, 4),
    (0x1_0000, 4),
    (0x2_0000, 4),
    (0x4_0000, 4),
    (0x8_0000, 4),
    (0x10_0000, 4),
];

/// True for a ZIP package that holds the Numbers document index.
pub(crate) fn is_numbers(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    zip.len() <= MAX_ENTRIES && zip.by_name(DOCUMENT_PART).is_ok()
}

/// Imports the sheets and tables of a Numbers document as worksheets of typed values. Formula
/// cells are imported as their last saved results, so editing their inputs does not recalculate
/// them. Always inspect the returned warnings. Saving to Numbers is not supported.
pub fn read_numbers(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let mut package = Package::open(bytes)?;
    if !package.has(DOCUMENT_PART) {
        return Err(IoError::Format("missing Numbers document index".into()));
    }
    let objects = load_objects(&mut package)?;
    let mut import = Import::new(objects);
    import.run()?;
    if import.wb.sheets.is_empty() {
        return Err(IoError::Format("the Numbers document contains no tables".into()));
    }
    Ok((import.wb, import.report))
}

// ---------------------------------------------------------------- Snappy

/// Decodes one raw Snappy block; the declared length may not exceed `limit`.
fn snappy_decode(src: &[u8], limit: usize) -> Result<Vec<u8>, IoError> {
    let bad = || IoError::Format("corrupt Numbers data block".into());
    let (declared, mut pos) = read_varint(src).ok_or_else(bad)?;
    let declared =
        usize::try_from(declared).ok().filter(|n| *n <= limit).ok_or_else(|| IoError::TooLarge("Numbers data block is too large".into()))?;
    let mut out: Vec<u8> = Vec::with_capacity(declared);
    while pos < src.len() {
        let tag = *src.get(pos).ok_or_else(bad)?;
        pos += 1;
        let (len, offset) = match tag & 3 {
            0 => {
                let mut len = usize::from(tag >> 2);
                if len >= 60 {
                    let extra = len - 59;
                    let raw = src.get(pos..pos + extra).ok_or_else(bad)?;
                    pos += extra;
                    len = raw.iter().rev().fold(0usize, |n, b| (n << 8) | usize::from(*b));
                }
                let len = len.checked_add(1).ok_or_else(bad)?;
                let end = pos.checked_add(len).ok_or_else(bad)?;
                out.extend_from_slice(src.get(pos..end).ok_or_else(bad)?);
                pos = end;
                if out.len() > declared {
                    return Err(bad());
                }
                continue;
            }
            1 => {
                let low = *src.get(pos).ok_or_else(bad)?;
                pos += 1;
                (usize::from((tag >> 2) & 7) + 4, (usize::from(tag >> 5) << 8) | usize::from(low))
            }
            2 => {
                let raw = src.get(pos..pos + 2).ok_or_else(bad)?;
                pos += 2;
                (usize::from(tag >> 2) + 1, usize::from(u16::from_le_bytes([raw[0], raw[1]])))
            }
            _ => {
                let raw = src.get(pos..pos + 4).ok_or_else(bad)?;
                pos += 4;
                (usize::from(tag >> 2) + 1, usize::try_from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])).map_err(|_| bad())?)
            }
        };
        if offset == 0 || offset > out.len() || out.len() + len > declared {
            return Err(bad());
        }
        // Copies may overlap their own output, so they go byte by byte.
        let start = out.len() - offset;
        for i in 0..len {
            let byte = *out.get(start + i).ok_or_else(bad)?;
            out.push(byte);
        }
    }
    if out.len() != declared {
        return Err(bad());
    }
    Ok(out)
}

/// Decompresses an IWA part: chunks of a zero byte, a 24-bit length and a raw Snappy block.
fn iwa_decompress(data: &[u8], budget: &mut usize) -> Result<Vec<u8>, IoError> {
    let bad = || IoError::Format("corrupt Numbers index part".into());
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let head = data.get(pos..pos + 4).ok_or_else(bad)?;
        if head[0] != 0 {
            return Err(bad());
        }
        let len = usize::from(head[1]) | usize::from(head[2]) << 8 | usize::from(head[3]) << 16;
        pos += 4;
        let chunk = data.get(pos..pos.checked_add(len).ok_or_else(bad)?).ok_or_else(bad)?;
        pos += len;
        let block = snappy_decode(chunk, MAX_CHUNK.min(*budget))?;
        *budget -= block.len();
        out.extend_from_slice(&block);
    }
    Ok(out)
}

// ---------------------------------------------------------------- Protocol Buffers

fn read_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, byte) in buf.iter().take(10).enumerate() {
        value |= u64::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// A field value. Fixed-width fields are not needed and are skipped.
enum Val<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

/// Iterates the fields of one message. Malformed bytes (a bad varint, a length running past the
/// end, an unsupported wire type) end the iteration and set `malformed`, so callers can tell that
/// from the normal end of the message.
struct Fields<'a> {
    buf: &'a [u8],
    malformed: bool,
}

impl<'a> Fields<'a> {
    /// `Some(Some(field))` for a field, `Some(None)` for a skipped fixed-width one, `None` when malformed.
    fn step(&mut self) -> Option<Option<(u32, Val<'a>)>> {
        let (key, n) = read_varint(self.buf)?;
        self.buf = self.buf.get(n..)?;
        let number = u32::try_from(key >> 3).ok()?;
        match key & 7 {
            0 => {
                let (v, n) = read_varint(self.buf)?;
                self.buf = self.buf.get(n..)?;
                Some(Some((number, Val::Varint(v))))
            }
            1 => {
                self.buf = self.buf.get(8..)?;
                Some(None)
            }
            2 => {
                let (len, n) = read_varint(self.buf)?;
                let end = n.checked_add(usize::try_from(len).ok()?)?;
                let value = self.buf.get(n..end)?;
                self.buf = self.buf.get(end..)?;
                Some(Some((number, Val::Bytes(value))))
            }
            5 => {
                self.buf = self.buf.get(4..)?;
                Some(None)
            }
            _ => None,
        }
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Val<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.buf.is_empty() {
            match self.step() {
                Some(Some(field)) => return Some(field),
                Some(None) => {}
                None => {
                    self.malformed = true;
                    self.buf = &[];
                }
            }
        }
        None
    }
}

fn fields(buf: &[u8]) -> Fields<'_> {
    Fields { buf, malformed: false }
}

/// True when `buf` is a complete message: every field decodes up to the end.
fn well_formed(buf: &[u8]) -> bool {
    let mut f = fields(buf);
    f.by_ref().for_each(drop);
    !f.malformed
}

fn varint_field(buf: &[u8], number: u32) -> Option<u64> {
    fields(buf).find_map(|(n, v)| match v {
        Val::Varint(x) if n == number => Some(x),
        _ => None,
    })
}

fn bytes_field(buf: &[u8], number: u32) -> Option<&[u8]> {
    bytes_fields(buf, number).next()
}

fn bytes_fields(buf: &[u8], number: u32) -> impl Iterator<Item = &[u8]> {
    fields(buf).filter_map(move |(n, v)| match v {
        Val::Bytes(b) if n == number => Some(b),
        _ => None,
    })
}

/// The identifier a reference message points at (`{ identifier = 1 }`).
fn reference(buf: &[u8]) -> Option<u64> {
    varint_field(buf, 1)
}

fn reference_field(buf: &[u8], number: u32) -> Option<u64> {
    bytes_field(buf, number).and_then(reference)
}

// ---------------------------------------------------------------- Objects

struct Objects {
    items: HashMap<u64, (u32, Vec<u8>)>,
}

impl Objects {
    /// The payload of object `id` when it has message type `kind`.
    fn get(&self, id: u64, kind: u32) -> Option<&[u8]> {
        self.items.get(&id).filter(|(k, _)| *k == kind).map(|(_, p)| p.as_slice())
    }

    fn document(&self) -> Option<&[u8]> {
        self.items.iter().filter(|(_, (k, _))| *k == T_DOCUMENT).min_by_key(|(id, _)| **id).map(|(_, (_, p))| p.as_slice())
    }
}

fn wanted(kind: u32) -> bool {
    matches!(kind, T_DOCUMENT | T_SHEET | T_STORAGE | T_TABLE_INFO | T_TABLE_MODEL | T_TILE | T_DATA_LIST | T_RICH_PAYLOAD)
}

fn load_objects(package: &mut Package<'_>) -> Result<Objects, IoError> {
    let names: Vec<String> = package.names().into_iter().filter(|n| n.starts_with("index/") && n.ends_with(".iwa")).collect();
    let mut items = HashMap::new();
    let mut budget = MAX_TOTAL;
    for name in names {
        let Some(data) = package.read(&name)? else { continue };
        let decoded = iwa_decompress(&data, &mut budget)?;
        read_records(&decoded, &mut items)?;
    }
    Ok(Objects { items })
}

/// Splits decompressed IWA data into archives: a length-prefixed header (identifier plus one
/// entry per message, each with its type and payload length) followed by the payloads.
fn read_records(data: &[u8], items: &mut HashMap<u64, (u32, Vec<u8>)>) -> Result<(), IoError> {
    let bad = || IoError::Format("corrupt Numbers index record".into());
    let mut pos = 0usize;
    while pos < data.len() {
        let rest = data.get(pos..).ok_or_else(bad)?;
        let (head_len, n) = read_varint(rest).ok_or_else(bad)?;
        let head_start = pos + n;
        let head_end = head_start.checked_add(usize::try_from(head_len).map_err(|_| bad())?).ok_or_else(bad)?;
        let head = data.get(head_start..head_end).ok_or_else(bad)?;
        pos = head_end;
        let id = varint_field(head, 1).ok_or_else(bad)?;
        for info in bytes_fields(head, 2) {
            let kind = varint_field(info, 1).and_then(|k| u32::try_from(k).ok()).ok_or_else(bad)?;
            let len = usize::try_from(varint_field(info, 3).ok_or_else(bad)?).map_err(|_| bad())?;
            let end = pos.checked_add(len).ok_or_else(bad)?;
            let payload = data.get(pos..end).ok_or_else(bad)?;
            pos = end;
            if wanted(kind) {
                if items.len() >= MAX_OBJECTS {
                    return Err(IoError::TooLarge("Numbers document has too many objects".into()));
                }
                items.insert(id, (kind, payload.to_vec()));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- Import

struct Import {
    objects: Arc<Objects>,
    wb: Workbook,
    report: ReadReport,
    cells: u64,
    formats: Vec<(&'static str, StyleId)>,
    texts: Texts,
}

const MALFORMED: &str = "Some Numbers data was truncated or malformed; what could be read was imported and the rest was skipped.";

/// Text decoded so far, shared by the whole import: a data list that many tables reference is
/// decoded once, every cell shares its strings, and the total kept is capped.
#[derive(Default)]
struct Texts {
    strings: HashMap<u64, Arc<HashMap<u32, Arc<str>>>>,
    rich_lists: HashMap<u64, Arc<HashMap<u32, u64>>>,
    rich_text: HashMap<u64, Option<Arc<str>>>,
    bytes: usize,
    clipped: bool,
}

impl Texts {
    /// Keeps `text` (shortened to the cell limit) against the import-wide text budget.
    fn add(&mut self, text: &str) -> Result<Arc<str>, IoError> {
        let text = match text.char_indices().nth(MAX_CELL_TEXT) {
            Some((end, _)) => {
                self.clipped = true;
                text.get(..end).unwrap_or(text)
            }
            None => text,
        };
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_TEXT_BYTES {
            return Err(IoError::TooLarge("Numbers document has more than 256 MiB of text".into()));
        }
        Ok(Arc::from(text))
    }
}

/// One table's lookup lists, backed by the import's shared [`Texts`].
struct Lists {
    objects: Arc<Objects>,
    texts: Texts,
    strings: Arc<HashMap<u32, Arc<str>>>,
    rich: Arc<HashMap<u32, u64>>,
}

impl Lists {
    fn rich(&mut self, key: u32) -> Result<Option<Arc<str>>, IoError> {
        let Some(payload) = self.rich.get(&key).copied() else { return Ok(None) };
        if let Some(text) = self.texts.rich_text.get(&payload) {
            return Ok(text.clone());
        }
        let text = match rich_text(&self.objects, payload) {
            Some(t) => Some(self.texts.add(&t)?),
            None => None,
        };
        self.texts.rich_text.insert(payload, text.clone());
        Ok(text)
    }
}

fn rich_text(objects: &Objects, payload: u64) -> Option<String> {
    let storage = reference_field(objects.get(payload, T_RICH_PAYLOAD)?, 1)?;
    let parts: Vec<String> = bytes_fields(objects.get(storage, T_STORAGE)?, 3).map(|b| String::from_utf8_lossy(b).into_owned()).collect();
    Some(parts.concat())
}

fn entry_key(entry: &[u8]) -> Option<u32> {
    varint_field(entry, 1).and_then(|k| u32::try_from(k).ok())
}

fn build_lists(objects: &Arc<Objects>, store: &[u8], mut texts: Texts, report: &mut ReadReport) -> Result<Lists, IoError> {
    let mut strings = Arc::new(HashMap::new());
    if let Some(id) = reference_field(store, 4)
        && let Some(list) = objects.get(id, T_DATA_LIST)
    {
        strings = match texts.strings.get(&id) {
            Some(done) => Arc::clone(done),
            None => {
                if !well_formed(list) {
                    report.warn(MALFORMED);
                }
                let mut map = HashMap::new();
                for entry in bytes_fields(list, 3) {
                    if let (Some(key), Some(text)) = (entry_key(entry), bytes_field(entry, 3)) {
                        map.insert(key, texts.add(&String::from_utf8_lossy(text))?);
                    }
                }
                let map = Arc::new(map);
                texts.strings.insert(id, Arc::clone(&map));
                map
            }
        };
    }
    let mut rich = Arc::new(HashMap::new());
    if let Some(id) = reference_field(store, 17)
        && let Some(list) = objects.get(id, T_DATA_LIST)
    {
        rich = match texts.rich_lists.get(&id) {
            Some(done) => Arc::clone(done),
            None => {
                let map: HashMap<u32, u64> = bytes_fields(list, 3).filter_map(|e| Some((entry_key(e)?, reference_field(e, 9)?))).collect();
                let map = Arc::new(map);
                texts.rich_lists.insert(id, Arc::clone(&map));
                map
            }
        };
    }
    Ok(Lists { objects: Arc::clone(objects), texts, strings, rich })
}

/// The decoded optional fields of one packed cell.
#[derive(Default)]
struct Packed {
    kind: u8,
    decimal: Option<[u8; 16]>,
    double: Option<f64>,
    seconds: Option<f64>,
    string: Option<u32>,
    rich: Option<u32>,
}

impl Import {
    fn new(objects: Objects) -> Self {
        let mut wb = Workbook::new();
        wb.sheets.clear();
        let mut report = ReadReport::default();
        report.warn("Numbers data-only import: formulas become their saved results and will not recalculate. Source formatting, merged cells, charts, images and other features are not imported. Save a new .xlsx file; Numbers saving is not supported.");
        Self { objects: Arc::new(objects), wb, report, cells: 0, formats: vec![], texts: Texts::default() }
    }

    fn run(&mut self) -> Result<(), IoError> {
        // Payloads are borrowed from the shared objects, never copied; each sheet and table is
        // visited once however often it is referenced, and the table limit is checked first.
        let objects = Arc::clone(&self.objects);
        let Some(document) = objects.document() else {
            return Err(IoError::Format("the Numbers document has no document object".into()));
        };
        let mut names = crate::read::SheetNames::of(&self.wb);
        let (mut seen_sheets, mut seen_tables) = (HashSet::new(), HashSet::new());
        for sheet_id in bytes_fields(document, 1).filter_map(reference) {
            let Some(sheet) = objects.get(sheet_id, T_SHEET).filter(|_| seen_sheets.insert(sheet_id)) else { continue };
            let sheet_name = bytes_field(sheet, 1).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
            let models: Vec<&[u8]> = bytes_fields(sheet, 2)
                .filter_map(reference)
                .filter_map(|id| objects.get(id, T_TABLE_INFO))
                .filter_map(|info| reference_field(info, 2))
                .filter(|id| seen_tables.insert(*id))
                .filter_map(|id| objects.get(id, T_TABLE_MODEL))
                .take(MAX_SHEETS + 1)
                .collect();
            if models.is_empty() {
                self.report.warn("Numbers sheets without tables were skipped.");
            }
            let several = models.len() > 1;
            for model in models {
                if self.wb.sheets.len() >= MAX_SHEETS {
                    return Err(IoError::TooLarge("Numbers document contains more than 1024 tables".into()));
                }
                let table_name = bytes_field(model, 8).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
                let wanted = if several { format!("{sheet_name} - {table_name}") } else { sheet_name.clone() };
                let mut name = names.unique(&wanted, self.wb.sheets.len());
                if self.wb.check_sheet_name(&name, None).is_err() {
                    name = self.wb.next_sheet_name();
                }
                if name != wanted {
                    self.report.warn("Some Numbers sheet names were shortened or renamed to valid, unique XLSX sheet names.");
                }
                let mut sheet = Sheet::new(name);
                self.read_table(&objects, model, &mut sheet)?;
                self.wb.sheets.push(Arc::new(sheet));
            }
        }
        Ok(())
    }

    fn read_table(&mut self, objects: &Arc<Objects>, model: &[u8], sheet: &mut Sheet) -> Result<(), IoError> {
        let Some(store) = bytes_field(model, 4) else { return Ok(()) };
        let mut lists = build_lists(objects, store, std::mem::take(&mut self.texts), &mut self.report)?;
        let result = self.read_tiles(objects, store, &mut lists, sheet);
        if lists.texts.clipped {
            self.report.warn("Some Numbers text was longer than 32,767 characters and was shortened.");
        }
        self.texts = std::mem::take(&mut lists.texts);
        result
    }

    fn read_tiles(&mut self, objects: &Objects, store: &[u8], lists: &mut Lists, sheet: &mut Sheet) -> Result<(), IoError> {
        let Some(tiles) = bytes_field(store, 3) else { return Ok(()) };
        let height = varint_field(tiles, 2).and_then(|h| u32::try_from(h).ok()).filter(|h| (1..=65_536).contains(h)).unwrap_or(256);
        let mut seen = HashSet::new();
        for entry in bytes_fields(tiles, 1) {
            let (Some(index), Some(id)) = (varint_field(entry, 1).and_then(|i| u32::try_from(i).ok()), reference_field(entry, 2)) else { continue };
            let Some(tile) = objects.get(id, T_TILE).filter(|_| seen.insert(id)) else { continue };
            if !well_formed(tile) {
                self.report.warn(MALFORMED);
            }
            for info in bytes_fields(tile, 5) {
                self.read_row(info, index, height, lists, sheet)?;
            }
        }
        Ok(())
    }

    fn read_row(&mut self, info: &[u8], tile: u32, height: u32, lists: &mut Lists, sheet: &mut Sheet) -> Result<(), IoError> {
        if varint_field(info, 5) != Some(5) {
            self.report.warn("Some Numbers rows use an older cell storage layout and were skipped.");
            return Ok(());
        }
        if !well_formed(info) {
            self.report.warn(MALFORMED);
        }
        let in_tile = varint_field(info, 1).and_then(|r| u32::try_from(r).ok()).unwrap_or(0);
        let Some(row) = tile.checked_mul(height).and_then(|base| base.checked_add(in_tile)).filter(|r| *r < MAX_ROWS) else {
            return Err(IoError::TooLarge("Numbers table exceeds worksheet height".into()));
        };
        let (Some(buffer), Some(offsets)) = (bytes_field(info, 6), bytes_field(info, 7)) else { return Ok(()) };
        // With wide offsets each stored offset counts four bytes; 0xFFFF still marks an empty cell.
        let scale = if varint_field(info, 8).is_some_and(|wide| wide != 0) { 4 } else { 1 };
        for (col, pair) in offsets.as_chunks::<2>().0.iter().enumerate() {
            let stored = u16::from_le_bytes(*pair);
            if stored == 0xFFFF {
                continue;
            }
            let Some(packed) = buffer.get(usize::from(stored) * scale..).and_then(unpack) else { continue };
            let col =
                u32::try_from(col).ok().filter(|c| *c < MAX_COLS).ok_or_else(|| IoError::TooLarge("Numbers table exceeds worksheet width".into()))?;
            let rich = match (packed.kind, packed.rich) {
                (K_RICH, Some(key)) => lists.rich(key)?,
                _ => None,
            };
            if let Some(cell) = self.convert(&packed, lists, rich) {
                self.cells += 1;
                if self.cells > MAX_CELLS {
                    return Err(IoError::TooLarge("Numbers document has more than 2,000,000 cells".into()));
                }
                sheet.set_cell(CellRef::new(row, col), cell);
            }
        }
        Ok(())
    }

    fn convert(&mut self, p: &Packed, lists: &Lists, rich: Option<Arc<str>>) -> Option<Cell> {
        let number = p.decimal.map(decimal_to_f64).or(p.double);
        let mut format = None;
        let value = match p.kind {
            K_EMPTY => return None,
            K_NUMBER | K_CURRENCY => Value::Number(number?),
            K_BOOL => Value::Bool(number? != 0.0),
            K_DATE => {
                let seconds = p.seconds.or(number)?;
                format = Some(if seconds % SECONDS_PER_DAY == 0.0 { "yyyy-mm-dd" } else { "yyyy-mm-dd hh:mm:ss" });
                Value::Number(ORIGIN_SERIAL + seconds / SECONDS_PER_DAY)
            }
            K_DURATION => {
                format = Some("[h]:mm:ss");
                Value::Number(number? / SECONDS_PER_DAY)
            }
            K_ERROR => Value::Error(CellError::Value),
            K_TEXT => Value::Text(Arc::clone(lists.strings.get(&p.string?)?)),
            K_RICH => Value::Text(rich?),
            _ => {
                self.report.warn("Some Numbers cells have a type that is not supported and were skipped.");
                return None;
            }
        };
        if let Value::Number(n) = &value
            && !n.is_finite()
        {
            return None;
        }
        Some(Cell { style: self.style(format), ..Cell::value(value) })
    }

    fn style(&mut self, format: Option<&'static str>) -> StyleId {
        let Some(code) = format else { return StyleId::DEFAULT };
        if let Some((_, id)) = self.formats.iter().find(|(f, _)| *f == code) {
            return *id;
        }
        let id = self.wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
        self.formats.push((code, id));
        id
    }
}

/// Reads one packed cell: a 12-byte header (version, kind, flags) then the fields the flags announce.
fn unpack(cell: &[u8]) -> Option<Packed> {
    let kind = *cell.get(1)?;
    let flags = u32::from_le_bytes(cell.get(8..12)?.try_into().ok()?);
    let mut packed = Packed { kind, ..Packed::default() };
    let mut pos = 12usize;
    for (flag, size) in CELL_FIELDS {
        if flags & flag == 0 {
            continue;
        }
        let raw = cell.get(pos..pos.checked_add(size)?)?;
        pos += size;
        match flag {
            0x1 => packed.decimal = raw.try_into().ok(),
            0x2 => packed.double = Some(f64::from_le_bytes(raw.try_into().ok()?)),
            0x4 => packed.seconds = Some(f64::from_le_bytes(raw.try_into().ok()?)),
            0x8 => packed.string = Some(u32::from_le_bytes(raw.try_into().ok()?)),
            0x10 => packed.rich = Some(u32::from_le_bytes(raw.try_into().ok()?)),
            _ => {}
        }
    }
    Some(packed)
}

/// Converts an IEEE 754 decimal128 value in binary integer (BID) encoding to the nearest `f64`.
/// Non-finite and non-canonical encodings give NaN, which the caller drops.
fn decimal_to_f64(raw: [u8; 16]) -> f64 {
    let bits = u128::from_le_bytes(raw);
    let negative = bits >> 127 == 1;
    // Exponent field values starting with binary 11 mark infinities, NaNs and oversized coefficients.
    if (bits >> 125) & 3 == 3 {
        return f64::NAN;
    }
    let exponent = i32::try_from((bits >> 113) & 0x3FFF).map(|e| e - 6176).unwrap_or(0);
    let coefficient = bits & ((1u128 << 113) - 1);
    let parsed = format!("{coefficient}e{exponent}").parse::<f64>().unwrap_or(f64::NAN);
    if negative { -parsed } else { parsed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snappy_literals_and_copies_decode_including_overlap() {
        // "ab", then a 6-byte copy at offset 2 (tag type 2): the copy overlaps its own output.
        let block = [8, 1 << 2, b'a', b'b', (5 << 2) | 2, 2, 0];
        assert_eq!(snappy_decode(&block, 64).unwrap(), b"abababab");
        // Tag type 1: a 4-byte copy at offset 1 after one literal.
        let block = [5, 0, b'x', 1, 1];
        assert_eq!(snappy_decode(&block, 64).unwrap(), b"xxxxx");
    }

    #[test]
    fn snappy_rejects_bad_blocks() {
        // Offset before the start, zero offset, copy past the declared length, short output, over the limit.
        assert!(snappy_decode(&[4, (3 << 2) | 2, 1, 0], 64).is_err());
        assert!(snappy_decode(&[4, 0, b'a', 1, 0], 64).is_err());
        assert!(snappy_decode(&[3, 0, b'a', (5 << 2) | 2, 1, 0], 64).is_err());
        assert!(snappy_decode(&[4, 0, b'a'], 64).is_err());
        assert!(snappy_decode(&[100, 0, b'a'], 10).is_err());
        assert!(snappy_decode(&[], 64).is_err());
    }

    #[test]
    fn truncated_messages_are_reported_not_mistaken_for_the_end() {
        // field 1 = 5, then field 2 claiming 9 bytes with only 1 present.
        assert!(well_formed(&[0x08, 5, 0x12, 1, 7]));
        assert!(!well_formed(&[0x08, 5, 0x12, 9, 7]));
        assert!(!well_formed(&[0x08]));
        assert!(!well_formed(&[0x0B]));
        assert_eq!(varint_field(&[0x08, 5, 0x12, 9, 7], 1), Some(5));
    }

    fn field_bytes(number: u32, data: &[u8]) -> Vec<u8> {
        [&[(number as u8) << 3 | 2, data.len() as u8][..], data].concat()
    }

    #[test]
    fn a_text_list_shared_by_tables_is_decoded_once() {
        // List 7 holds one entry: key 1, string "shared".
        let entry = [&[0x08, 1][..], &field_bytes(3, b"shared")].concat();
        let list = field_bytes(3, &entry);
        let mut items = HashMap::new();
        items.insert(7, (T_DATA_LIST, list));
        let objects = Arc::new(Objects { items });
        // A table store whose string table (field 4) is a reference to object 7.
        let store = field_bytes(4, &[0x08, 7]);
        let mut report = ReadReport::default();
        let first = build_lists(&objects, &store, Texts::default(), &mut report).unwrap();
        let second = build_lists(&objects, &store, first.texts, &mut report).unwrap();
        assert_eq!(&*second.strings[&1], "shared");
        assert_eq!(second.texts.bytes, "shared".len(), "the second table reused the first table's text");
        assert_eq!(second.texts.strings.len(), 1);
    }

    #[test]
    fn retained_text_is_capped_for_the_whole_import() {
        let mut texts = Texts { bytes: MAX_TEXT_BYTES - 3, ..Texts::default() };
        assert!(texts.add("abc").is_ok());
        assert!(matches!(texts.add("d"), Err(IoError::TooLarge(_))));
    }

    #[test]
    fn decimals_convert_including_sign_and_exponent() {
        let one = 1u128 | (6176u128 << 113);
        assert_eq!(decimal_to_f64(one.to_le_bytes()), 1.0);
        let negative = 125u128 | (6174u128 << 113) | (1u128 << 127);
        assert_eq!(decimal_to_f64(negative.to_le_bytes()), -1.25);
        let big = 3u128 | (6178u128 << 113);
        assert_eq!(decimal_to_f64(big.to_le_bytes()), 300.0);
        // The marker for infinities and NaNs is dropped by the caller.
        assert!(decimal_to_f64((3u128 << 125).to_le_bytes()).is_nan());
    }
}
