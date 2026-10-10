//! Data-only OpenDocument Spreadsheet import. Formula caches become constants; styles and
//! other ODF features are deliberately not translated. All fixtures are authored in Rust.
//! Format reference: ODF 1.3, part 3, sections 6.1 and 9.1:
//! <https://docs.oasis-open.org/office/OpenDocument/v1.3/os/part3-schema/OpenDocument-v1.3-os-part3-schema.html>.

use std::io::{Cursor, Read};
use std::sync::Arc;

use gridcraft_core::date::{days_in_month, serial_from_ymd};
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use crate::package::{MAX_ENTRIES, Package};
use crate::{IoError, ReadReport};

const MIME: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";
const OFFICE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const TABLE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const TEXT: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const CALC_EXT: &[u8] = b"urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0";
const MANIFEST: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
const MAX_CELLS: u64 = 1_000_000;
const MAX_MERGES: u64 = 100_000;
const MAX_CELL_TEXT: usize = 1024 * 1024;
const MAX_TEXT: u64 = 64 * 1024 * 1024;
const MAX_SHEETS: usize = 1024;

/// Reads only the small, exact mimetype entry, including when the ZIP uses compression.
/// Looking at a filename or at arbitrary bytes inside the archive is not sufficient.
pub(crate) fn has_mimetype(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    if zip.len() > MAX_ENTRIES {
        return false;
    }
    let Ok(mut file) = zip.by_name("mimetype") else { return false };
    if file.size() != MIME.len() as u64 {
        return false;
    }
    let mut value = Vec::new();
    (&mut file).take(MIME.len() as u64 + 1).read_to_end(&mut value).is_ok() && value == MIME
}

/// Imports ODS sheets, typed values, displayed text and merges. Cached formula results are
/// imported as constants, so editing their inputs does not recalculate the original formula.
/// Always inspect the returned warnings. Saving to ODS is not supported.
pub fn read_ods(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let mut package = Package::open(bytes)?;
    if !has_mimetype(bytes) {
        return Err(IoError::Format("missing or invalid ODS mimetype".into()));
    }
    if let Some(manifest) = package.read("META-INF/manifest.xml")? {
        check_manifest(&manifest)?;
    }
    let content = package.read("content.xml")?.ok_or_else(|| IoError::Format("ODS content.xml is missing".into()))?;
    let mut import = Import::new();
    import.parse(&content)?;
    if import.wb.sheets.is_empty() {
        return Err(IoError::Format("ODS contains no worksheets".into()));
    }
    Ok((import.wb, import.report))
}

fn is_ns(ns: ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(ns, ResolveResult::Bound(n) if n.as_ref() == expected)
}

fn attr(reader: &NsReader<&[u8]>, e: &BytesStart<'_>, ns: &[u8], name: &[u8]) -> Result<Option<String>, IoError> {
    let mut found = None;
    for a in e.attributes() {
        let a = a.map_err(|e| IoError::Xml(e.to_string()))?;
        let (resolved, local) = reader.resolve_attribute(a.key);
        if is_ns(resolved, ns) && local.as_ref() == name {
            let value = a.unescape_value().map_err(|e| IoError::Xml(e.to_string()))?;
            if value.len() > MAX_CELL_TEXT {
                return Err(IoError::TooLarge("ODS attribute exceeds 1 MiB".into()));
            }
            found = Some(value.into_owned());
        }
    }
    Ok(found)
}

fn positive(value: Option<String>, default: u32, limit: u32) -> Result<u32, IoError> {
    let n = match value {
        Some(s) => s.parse::<u32>().map_err(|_| IoError::Format("invalid ODS repeat or span count".into()))?,
        None => default,
    };
    if n == 0 || n > limit {
        return Err(IoError::TooLarge("ODS repeat or span exceeds worksheet bounds".into()));
    }
    Ok(n)
}

fn xml_error(e: impl std::fmt::Display) -> IoError {
    IoError::Xml(format!("ODS: {e}"))
}

fn check_manifest(bytes: &[u8]) -> Result<(), IoError> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let (ns, name) = reader.resolve_element(e.name());
                if is_ns(ns.clone(), MANIFEST) && name.as_ref() == b"encryption-data" {
                    return Err(IoError::Format("encrypted ODS files are not supported".into()));
                }
                if depth == 0 {
                    if root_seen || !is_ns(ns, MANIFEST) || name.as_ref() != b"manifest" {
                        return Err(xml_error("expected one manifest:manifest root"));
                    }
                    root_seen = true;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > crate::xml::MAX_DEPTH {
                        return Err(xml_error("manifest nested too deeply"));
                    }
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or_else(|| xml_error("unexpected manifest closing element"))?,
            Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 {
        return Err(xml_error("incomplete manifest"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Document,
    Body,
    Spreadsheet,
    Table,
    Rows,
    Row,
    Cell,
    Covered,
    Paragraph,
    Inline,
    Space,
    Tab,
    Break,
    Other,
}

fn tag(reader: &NsReader<&[u8]>, e: &BytesStart<'_>) -> Tag {
    let (ns, name) = reader.resolve_element(e.name());
    let local = name.as_ref();
    if is_ns(ns.clone(), OFFICE) {
        return match local {
            b"document-content" => Tag::Document,
            b"body" => Tag::Body,
            b"spreadsheet" => Tag::Spreadsheet,
            _ => Tag::Other,
        };
    }
    if is_ns(ns.clone(), TABLE) {
        return match local {
            b"table" => Tag::Table,
            b"table-row-group" | b"table-header-rows" | b"table-rows" => Tag::Rows,
            b"table-row" => Tag::Row,
            b"table-cell" => Tag::Cell,
            b"covered-table-cell" => Tag::Covered,
            _ => Tag::Other,
        };
    }
    if is_ns(ns, TEXT) {
        return match local {
            b"p" | b"h" => Tag::Paragraph,
            b"span" | b"a" | b"meta" | b"meta-field" | b"ruby" | b"ruby-base" => Tag::Inline,
            b"s" => Tag::Space,
            b"tab" => Tag::Tab,
            b"line-break" => Tag::Break,
            _ => Tag::Other,
        };
    }
    Tag::Other
}

struct SheetDraft {
    sheet: Sheet,
    depth: usize,
    row: u32,
}
struct RowDraft {
    depth: usize,
    repeat: u32,
    col: u32,
    cells: Vec<RepeatedCell>,
}
struct RepeatedCell {
    col: u32,
    repeat: u32,
    width: u32,
    height: u32,
    cell: Cell,
}
struct CellDraft {
    depth: usize,
    repeat: u32,
    width: u32,
    height: u32,
    covered: bool,
    kind: Option<String>,
    raw: Option<String>,
    formula: bool,
    text: String,
    paragraph: Option<usize>,
    paragraphs: usize,
    paragraph_started: bool,
    pending_space: bool,
}
struct Import {
    wb: Workbook,
    report: ReadReport,
    stack: Vec<Tag>,
    sheet: Option<SheetDraft>,
    row: Option<RowDraft>,
    cell: Option<CellDraft>,
    cells: u64,
    merges: u64,
    text: u64,
    formats: Vec<(&'static str, StyleId)>,
    spreadsheet_seen: bool,
}

impl Import {
    fn new() -> Self {
        let mut wb = Workbook::new();
        wb.sheets.clear();
        let mut report = ReadReport::default();
        report.warn("ODS data-only import: formulas become their saved results and will not recalculate. Source formatting, charts, images and other features are not imported. Save a new .xlsx file; ODS saving is not supported.");
        Self { wb, report, stack: vec![], sheet: None, row: None, cell: None, cells: 0, merges: 0, text: 0, formats: vec![], spreadsheet_seen: false }
    }

    fn parse(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        let mut reader = NsReader::from_reader(bytes);
        let mut root_seen = false;
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(e) => {
                    if self.stack.len() >= crate::xml::MAX_DEPTH {
                        return Err(xml_error("XML nested too deeply"));
                    }
                    if self.stack.is_empty() {
                        if root_seen || tag(&reader, &e) != Tag::Document {
                            return Err(xml_error("expected one office:document-content root"));
                        }
                        root_seen = true;
                    }
                    self.start(&reader, &e)?;
                }
                Event::Empty(e) => {
                    if self.stack.is_empty() {
                        return Err(xml_error("expected office:document-content with a spreadsheet"));
                    }
                    self.start(&reader, &e)?;
                    self.end()?;
                }
                Event::End(_) => self.end()?,
                Event::Text(t) => {
                    let text = t.xml_content().map_err(xml_error)?;
                    if self.stack.is_empty() && !text.trim().is_empty() {
                        return Err(xml_error("text outside the document"));
                    }
                    self.append(&text)?;
                }
                Event::CData(t) => self.append(&t.decode().map_err(xml_error)?)?,
                Event::GeneralRef(r) => {
                    let ch = match r.resolve_char_ref().map_err(xml_error)? {
                        Some(c) => c,
                        None => match r.as_ref() {
                            b"amp" => '&',
                            b"lt" => '<',
                            b"gt" => '>',
                            b"apos" => '\'',
                            b"quot" => '"',
                            _ => return Err(xml_error("unsupported XML entity")),
                        },
                    };
                    self.append(ch.encode_utf8(&mut [0; 4]))?;
                }
                Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
                Event::Eof => break,
                _ => {}
            }
        }
        if !root_seen || !self.stack.is_empty() || !self.spreadsheet_seen {
            return Err(xml_error("incomplete ODS document"));
        }
        Ok(())
    }

    fn start(&mut self, reader: &NsReader<&[u8]>, e: &BytesStart<'_>) -> Result<(), IoError> {
        let t = tag(reader, e);
        let depth = self.stack.len();
        match t {
            Tag::Spreadsheet if self.stack == [Tag::Document, Tag::Body] => {
                if self.spreadsheet_seen {
                    return Err(xml_error("multiple spreadsheets in one document"));
                }
                self.spreadsheet_seen = true;
            }
            Tag::Table if self.stack == [Tag::Document, Tag::Body, Tag::Spreadsheet] => {
                if self.wb.sheets.len() >= MAX_SHEETS {
                    return Err(IoError::TooLarge("ODS contains more than 1024 sheets".into()));
                }
                let original = attr(reader, e, TABLE, b"name")?.unwrap_or_default();
                let mut name = crate::read::unique_name(&self.wb, original.trim_matches('\''), self.wb.sheets.len());
                if self.wb.check_sheet_name(&name, None).is_err() {
                    name = self.wb.next_sheet_name();
                }
                if name != original {
                    self.report.warn("Some ODS sheet names were shortened or renamed to valid, unique XLSX sheet names.");
                }
                self.sheet = Some(SheetDraft { sheet: Sheet::new(name), depth, row: 0 });
            }
            Tag::Row if self.sheet.as_ref().is_some_and(|s| self.stack.get(s.depth + 1..).is_some_and(|p| p.iter().all(|t| *t == Tag::Rows))) => {
                self.row =
                    Some(RowDraft { depth, repeat: positive(attr(reader, e, TABLE, b"number-rows-repeated")?, 1, u32::MAX)?, col: 0, cells: vec![] });
            }
            Tag::Cell | Tag::Covered if self.row.as_ref().is_some_and(|r| depth == r.depth + 1) => {
                let kind = if attr(reader, e, CALC_EXT, b"value-type")?.as_deref() == Some("error") {
                    Some("error".into())
                } else {
                    attr(reader, e, OFFICE, b"value-type")?
                };
                let key: &[u8] = match kind.as_deref() {
                    Some("boolean") => b"boolean-value",
                    Some("date") => b"date-value",
                    Some("time") => b"time-value",
                    Some("string") => b"string-value",
                    _ => b"value",
                };
                self.cell = Some(CellDraft {
                    depth,
                    repeat: positive(attr(reader, e, TABLE, b"number-columns-repeated")?, 1, u32::MAX)?,
                    width: positive(attr(reader, e, TABLE, b"number-columns-spanned")?, 1, MAX_COLS)?,
                    height: positive(attr(reader, e, TABLE, b"number-rows-spanned")?, 1, MAX_ROWS)?,
                    covered: t == Tag::Covered,
                    kind,
                    raw: attr(reader, e, OFFICE, key)?,
                    formula: attr(reader, e, TABLE, b"formula")?.is_some(),
                    text: String::new(),
                    paragraph: None,
                    paragraphs: 0,
                    paragraph_started: false,
                    pending_space: false,
                });
            }
            Tag::Paragraph if self.cell.as_ref().is_some_and(|c| depth == c.depth + 1) => {
                if let Some(cell) = self.cell.as_mut() {
                    if cell.paragraphs > 0 {
                        push_text(&mut cell.text, "\n")?;
                    }
                    cell.paragraph = Some(depth);
                    cell.paragraphs += 1;
                    cell.paragraph_started = false;
                    cell.pending_space = false;
                }
            }
            Tag::Space | Tag::Tab | Tag::Break if self.captures_text() => {
                let count = if t == Tag::Space {
                    let n = attr(reader, e, TEXT, b"c")?.map(|s| s.parse::<u32>()).transpose().map_err(xml_error)?.unwrap_or(1);
                    if n as usize > MAX_CELL_TEXT {
                        return Err(IoError::TooLarge("ODS repeated spaces exceed 1 MiB".into()));
                    }
                    n
                } else {
                    1
                };
                let value = match t {
                    Tag::Space => " ",
                    Tag::Tab => "\t",
                    _ => "\n",
                };
                if let Some(cell) = self.cell.as_mut() {
                    if cell.pending_space {
                        push_text(&mut cell.text, " ")?;
                        cell.pending_space = false;
                    }
                    cell.paragraph_started = true;
                    if cell.text.len().saturating_add(count as usize) > MAX_CELL_TEXT {
                        return Err(IoError::TooLarge("ODS cell text exceeds 1 MiB".into()));
                    }
                    for _ in 0..count {
                        cell.text.push_str(value);
                    }
                }
            }
            _ => {}
        }
        self.stack.push(t);
        Ok(())
    }

    fn captures_text(&self) -> bool {
        self.cell
            .as_ref()
            .and_then(|c| c.paragraph)
            .is_some_and(|depth| self.stack.get(depth..).is_some_and(|p| p.iter().all(|t| matches!(t, Tag::Paragraph | Tag::Inline))))
    }

    fn append(&mut self, text: &str) -> Result<(), IoError> {
        if self.captures_text()
            && let Some(cell) = self.cell.as_mut()
        {
            // ODF 1.3 section 6.1.2: collapse XML whitespace across inline runs,
            // but expand explicit text:s/tab/line-break elements separately.
            for ch in text.chars() {
                if matches!(ch, ' ' | '\t' | '\n' | '\r') {
                    cell.pending_space = cell.paragraph_started;
                } else {
                    if cell.pending_space {
                        push_text(&mut cell.text, " ")?;
                        cell.pending_space = false;
                    }
                    push_text(&mut cell.text, ch.encode_utf8(&mut [0; 4]))?;
                    cell.paragraph_started = true;
                }
            }
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), IoError> {
        self.stack.pop().ok_or_else(|| xml_error("unexpected closing element"))?;
        let depth = self.stack.len();
        if let Some(cell) = self.cell.as_mut()
            && cell.paragraph == Some(depth)
        {
            cell.paragraph = None;
        }
        if self.cell.as_ref().is_some_and(|c| c.depth == depth) {
            self.finish_cell()?;
        }
        if self.row.as_ref().is_some_and(|r| r.depth == depth) {
            self.finish_row()?;
        }
        if self.sheet.as_ref().is_some_and(|s| s.depth == depth)
            && let Some(sheet) = self.sheet.take()
        {
            self.wb.sheets.push(Arc::new(sheet.sheet));
        }
        Ok(())
    }

    fn finish_cell(&mut self) -> Result<(), IoError> {
        let Some(draft) = self.cell.take() else { return Ok(()) };
        let cell = self.convert(&draft);
        let (width, height) = if draft.covered { (1, 1) } else { (draft.width, draft.height) };
        let Some(row) = self.row.as_mut() else { return Ok(()) };
        let kept = !cell.is_blank() || width > 1 || height > 1;
        // Writers pad rows with blank repeats up to their own (possibly wider) sheet edge;
        // blank cells past ours carry no data, so clip them instead of rejecting the file.
        let end = match row.col.checked_add(draft.repeat).filter(|n| *n <= MAX_COLS) {
            Some(end) => end,
            None if !kept => MAX_COLS,
            None => return Err(IoError::TooLarge("ODS row exceeds worksheet width".into())),
        };
        if kept {
            row.cells.push(RepeatedCell { col: row.col, repeat: draft.repeat, width, height, cell });
        }
        row.col = end;
        Ok(())
    }

    fn convert(&mut self, draft: &CellDraft) -> Cell {
        let raw = draft.raw.as_deref();
        let mut format = None;
        let value = match draft.kind.as_deref() {
            Some("float" | "percentage" | "currency") => raw.and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite()).map(|n| {
                if draft.kind.as_deref() == Some("percentage") {
                    format = Some("0.00%");
                }
                Value::Number(n)
            }),
            Some("boolean") => match raw {
                Some("true" | "1") => Some(Value::Bool(true)),
                Some("false" | "0") => Some(Value::Bool(false)),
                _ => None,
            },
            Some("string") if raw.is_some() || draft.paragraphs > 0 || !draft.formula => Some(Value::Text(Arc::from(raw.unwrap_or(&draft.text)))),
            Some("void") if !draft.formula => Some(Value::Empty),
            Some("date") => raw.and_then(parse_date).map(|(n, f)| {
                format = Some(f);
                Value::Number(n)
            }),
            Some("time") => raw.and_then(parse_duration).map(|n| {
                format = Some("[h]:mm:ss.000");
                Value::Number(n)
            }),
            None if !draft.formula => Some(if draft.text.is_empty() { Value::Empty } else { Value::Text(Arc::from(draft.text.as_str())) }),
            _ => None,
        };
        let value = value.unwrap_or_else(|| {
            self.report.warn("Some ODS values or formula caches were unsupported or missing; their displayed text (or raw value) was kept as text. A formula without either is marked [Unavailable ODS formula result].");
            let text = if !draft.text.is_empty() { draft.text.as_str() } else if let Some(raw) = raw { raw } else if draft.formula { "[Unavailable ODS formula result]" } else { "[Unsupported ODS value]" };
            Value::Text(Arc::from(text))
        });
        let style = match format {
            Some(code) => match self.formats.iter().find(|(f, _)| *f == code) {
                Some((_, id)) => *id,
                None => {
                    let id = self.wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
                    self.formats.push((code, id));
                    id
                }
            },
            None => StyleId::DEFAULT,
        };
        Cell { style, ..Cell::value(value) }
    }

    fn finish_row(&mut self) -> Result<(), IoError> {
        let Some(row) = self.row.take() else { return Ok(()) };
        let Some(sheet) = self.sheet.as_mut() else { return Ok(()) };
        // Blank trailing row repeats past the sheet edge (e.g. from larger-sheet writers) are clipped.
        let end = match sheet.row.checked_add(row.repeat).filter(|n| *n <= MAX_ROWS) {
            Some(end) => end,
            None if row.cells.is_empty() => MAX_ROWS,
            None => return Err(IoError::TooLarge("ODS exceeds worksheet height".into())),
        };
        for entry in row.cells {
            let count = u64::from(entry.repeat) * u64::from(row.repeat);
            if !entry.cell.is_blank() {
                self.cells = self.cells.saturating_add(count);
                if let Value::Text(t) = &entry.cell.value {
                    self.text = self.text.saturating_add(count.saturating_mul(t.len() as u64));
                }
            }
            let merged = entry.width > 1 || entry.height > 1;
            if merged {
                self.merges = self.merges.saturating_add(count);
            }
            if self.cells > MAX_CELLS || self.merges > MAX_MERGES || self.text > MAX_TEXT {
                return Err(IoError::TooLarge("ODS expands beyond the import limit (1 million cells, 100000 merges or 64 MiB of text)".into()));
            }
            let last_col = entry.col.checked_add(entry.repeat - 1).and_then(|c| c.checked_add(entry.width - 1));
            let last_row = end.checked_add(entry.height - 1);
            if last_col.is_none_or(|c| c >= MAX_COLS) || last_row.is_none_or(|r| r > MAX_ROWS) {
                return Err(IoError::TooLarge("ODS merged cell exceeds worksheet bounds".into()));
            }
            if merged && ((entry.repeat > 1 && entry.width > 1) || (row.repeat > 1 && entry.height > 1)) {
                return Err(IoError::Format("repeated ODS cells have overlapping merges".into()));
            }
            for r in sheet.row..end {
                for c in entry.col..entry.col + entry.repeat {
                    let pos = CellRef::new(r, c);
                    if !entry.cell.is_blank() {
                        sheet.sheet.cells.set(pos, entry.cell.clone());
                    }
                    if merged {
                        sheet.sheet.merges.push(RangeRef::new(pos, CellRef::new(r + entry.height - 1, c + entry.width - 1)));
                    }
                }
            }
        }
        sheet.row = end;
        Ok(())
    }
}

fn push_text(out: &mut String, text: &str) -> Result<(), IoError> {
    if out.len().saturating_add(text.len()) > MAX_CELL_TEXT {
        return Err(IoError::TooLarge("ODS cell text exceeds 1 MiB".into()));
    }
    out.push_str(text);
    Ok(())
}

/// Support unzoned ISO calendar dates and times. A timezone, invalid date or out-of-range
/// calendar year remains text; never silently discard an offset or roll an invalid date.
fn parse_date(s: &str) -> Option<(f64, &'static str)> {
    let (date, time) = s.split_once('T').map_or((s, None), |(d, t)| (d, Some(t)));
    if date.len() != 10 || date.get(4..5)? != "-" || date.get(7..8)? != "-" {
        return None;
    }
    let year: i32 = digits(date.get(..4)?)?.parse().ok()?;
    let month: u32 = digits(date.get(5..7)?)?.parse().ok()?;
    let day: u32 = digits(date.get(8..10)?)?.parse().ok()?;
    if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let day = serial_from_ymd(DateSystem::D1900, year.into(), month.into(), day.into())?;
    let Some(time) = time else { return Some((day, "yyyy-mm-dd")) };
    let mut parts = time.split(':');
    let h = parts.next()?;
    let m = parts.next()?;
    let sec = parts.next()?;
    if parts.next().is_some() || h.len() != 2 || m.len() != 2 || sec.split('.').next()?.len() != 2 {
        return None;
    }
    let h: u32 = digits(h)?.parse().ok()?;
    let m: u32 = digits(m)?.parse().ok()?;
    let sec = decimal(sec)?;
    if h >= 24 || m >= 60 || sec >= 60.0 {
        return None;
    }
    Some((day + (h as f64 * 3600.0 + m as f64 * 60.0 + sec) / 86400.0, "yyyy-mm-dd hh:mm:ss.000"))
}

fn digits(s: &str) -> Option<&str> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}
fn decimal(s: &str) -> Option<f64> {
    match s.split_once('.') {
        Some((a, b)) => {
            digits(a)?;
            digits(b)?;
        }
        None => {
            digits(s)?;
        }
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A bounded ISO day/time duration. Calendar years/months cannot be represented as days.
fn parse_duration(s: &str) -> Option<f64> {
    let (negative, s) = s.strip_prefix('-').map_or((false, s), |s| (true, s));
    let mut rest = s.strip_prefix('P')?;
    let mut total = 0.0;
    let mut any = false;
    if let Some((days, tail)) = rest.split_once('D') {
        total += digits(days)?.parse::<f64>().ok()? * 86400.0;
        rest = tail;
        any = true;
    }
    if let Some(mut time) = rest.strip_prefix('T') {
        let mut time_any = false;
        for (unit, factor) in [('H', 3600.0), ('M', 60.0), ('S', 1.0)] {
            if let Some((n, tail)) = time.split_once(unit) {
                let value = if unit == 'S' { decimal(n)? } else { digits(n)?.parse::<f64>().ok()? };
                total += value * factor;
                time = tail;
                time_any = true;
            }
        }
        if !time.is_empty() || !time_any {
            return None;
        }
        any = true;
    } else if !rest.is_empty() {
        return None;
    }
    let days = total / 86400.0;
    (any && days.is_finite() && days <= 2_958_465.0).then_some(if negative { -days } else { days })
}
