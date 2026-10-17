//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::sync::Arc;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, CsvEncoding, NumFmt, Sheet, StyleId, Workbook};

use crate::{CsvOptions, IoError};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

/// Bytes one encode step writes into: encoding a 512 MiB CSV must not need a second copy of it
/// (larger than any single character the code pages here need, so every step makes progress).
const ENCODE_CHUNK: usize = 8 * 1024;

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Whether text holds hiragana or katakana — including halfwidth katakana (`U+FF66`–`U+FF9F`).
/// Bytes that decode to kana under CP932 are Japanese text: Latin-1 text decoded as CP932 never
/// produces kana, so this is the signal [`CsvEncoding::Auto`] uses to choose Shift-JIS.
fn has_kana(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}'))
}

/// Windows-1252, the historical fallback: bytes 0x80–0x9F use their code points, others are Latin-1.
fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
            _ => b as char,
        })
        .collect()
}

/// Shift-JIS (CP932), with bytes it can't map replaced by `U+FFFD`. `decode_without_bom_handling`
/// keeps an explicit code page from being overridden by a BOM.
fn decode_shift_jis(bytes: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0.into_owned()
}

/// UTF-8, with invalid sequences replaced by `U+FFFD` and a leading BOM dropped (it isn't text).
fn decode_utf8(bytes: &[u8]) -> String {
    let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(rest).into_owned()
}

/// UTF-16 with the given endianness, malformed units replaced by `U+FFFD`.
fn decode_utf16(bytes: &[u8], le: bool) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// [`CsvEncoding::Auto`]: UTF-8/UTF-16 BOMs, else UTF-8, else Shift-JIS when the decoded text
/// holds kana, else Windows-1252 — today's fallback, so a file that isn't Japanese reads as before.
fn decode_auto(bytes: &[u8]) -> (String, Option<CsvEncoding>) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), Some(CsvEncoding::Utf8Bom));
    }
    // UTF-16 has no CSV encoding to write back: saving such a workbook writes UTF-8.
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (decode_utf16(rest, true), None);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (decode_utf16(rest, false), None);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), Some(CsvEncoding::Utf8)),
        Err(_) => {
            let shift_jis = decode_shift_jis(bytes);
            if has_kana(&shift_jis) { (shift_jis, Some(CsvEncoding::ShiftJis)) } else { (decode_cp1252(bytes), Some(CsvEncoding::Windows1252)) }
        }
    }
}

/// Decodes CSV bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Windows-1252.
pub fn decode_text(bytes: &[u8]) -> String {
    decode_text_as(bytes, CsvEncoding::Auto).0
}

/// [`decode_text`] with the code page chosen (or detected for [`CsvEncoding::Auto`]), and the
/// encoding the bytes turned out to be in — what the workbook remembers for saving. `None` means
/// the bytes were UTF-16: there is no CSV encoding that writes them back, so saving uses UTF-8.
pub fn decode_text_as(bytes: &[u8], encoding: CsvEncoding) -> (String, Option<CsvEncoding>) {
    match encoding {
        CsvEncoding::Auto => decode_auto(bytes),
        CsvEncoding::Utf8 | CsvEncoding::Utf8Bom => (decode_utf8(bytes), Some(encoding)),
        CsvEncoding::ShiftJis => (decode_shift_jis(bytes), Some(CsvEncoding::ShiftJis)),
        CsvEncoding::Windows1252 => (decode_cp1252(bytes), Some(CsvEncoding::Windows1252)),
    }
}

/// Encodes CSV text in a legacy code page: a character the code page cannot map becomes `?`, as
/// Word and Excel write it when saving to a legacy code page — never encoding_rs' default HTML
/// numeric character reference (`&#128512;`), which would corrupt the CSV fields.
fn encode_legacy(text: &str, encoding: &'static encoding_rs::Encoding) -> Vec<u8> {
    let mut encoder = encoding.new_encoder();
    let mut out = Vec::with_capacity(text.len());
    let mut buf = vec![0u8; ENCODE_CHUNK];
    let mut rest = text;
    loop {
        let (result, read, written) = encoder.encode_from_utf8_without_replacement(rest, &mut buf, true);
        if let Some(bytes) = buf.get(..written) {
            out.extend_from_slice(bytes);
        }
        rest = rest.get(read..).unwrap_or_default();
        match result {
            encoding_rs::EncoderResult::InputEmpty => break,
            // The chunk was filled: the next step encodes the rest.
            encoding_rs::EncoderResult::OutputFull => {}
            encoding_rs::EncoderResult::Unmappable(_) => out.push(b'?'),
        }
    }
    out
}

/// Microsoft CP932 has a byte for the wave dash and the double vertical line that WHATWG
/// `shift_jis` only maps when *decoding*; encoding them directly would write `?` (so `1〜5` would
/// export as `1?5`). Excel writes `0x8160`/`0x8161`, which is what their CP932 equivalents encode
/// to, so translate those two before encoding.
fn cp932_text(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(['\u{301C}', '\u{2016}']) {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.chars()
            .map(|c| match c {
                '\u{301C}' => '\u{FF5E}',
                '\u{2016}' => '\u{2225}',
                c => c,
            })
            .collect(),
    )
}

/// Encodes CSV text in `encoding`. `Auto` writes UTF-8: it applies to reading only.
pub(crate) fn encode_text(text: &str, encoding: CsvEncoding) -> Vec<u8> {
    match encoding {
        CsvEncoding::Auto | CsvEncoding::Utf8 => text.as_bytes().to_vec(),
        CsvEncoding::Utf8Bom => {
            let mut out = Vec::with_capacity(text.len() + 3);
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            out.extend_from_slice(text.as_bytes());
            out
        }
        CsvEncoding::ShiftJis => encode_legacy(&cp932_text(text), encoding_rs::SHIFT_JIS),
        CsvEncoding::Windows1252 => encode_legacy(text, encoding_rs::WINDOWS_1252),
    }
}

/// Picks comma, semicolon or tab: the candidate with the most consistent count per line
/// (outside quotes) over the first non-blank lines. A candidate must appear on every such line, otherwise comma wins.
pub fn sniff_delimiter(text: &str, quote: char) -> u8 {
    let mut best = (b',', 0usize, 0usize);
    for d in [',', ';', '\t'] {
        let mut counts = vec![];
        let mut n = 0usize;
        let mut in_q = false;
        let mut blank = true;
        for ch in text.chars().take(64 * 1024) {
            if !in_q && ch == '\n' {
                // Blank lines carry no delimiter evidence.
                if !blank {
                    counts.push(n);
                }
                n = 0;
                blank = true;
                if counts.len() >= 20 {
                    break;
                }
                continue;
            }
            if ch != '\r' {
                blank = false;
            }
            if ch == quote {
                in_q = !in_q;
            } else if !in_q && ch == d {
                n += 1;
            }
        }
        if !blank {
            counts.push(n);
        }
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        if min > 0 && (min, total) > (best.1, best.2) {
            best = (d as u8, min, total);
        }
    }
    best.0
}

/// Splits CSV text into records of fields.
fn parse_records(text: &str, delim: char, quote: char, mut on_record: impl FnMut(Vec<String>) -> bool) {
    let mut field = String::new();
    let mut record: Vec<String> = vec![];
    let mut in_q = false;
    let mut chars = text.chars().peekable();
    let mut field_started_quoted = false;
    while let Some(c) = chars.next() {
        if in_q {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    field.push(quote);
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if c == quote && field.is_empty() && !field_started_quoted {
            in_q = true;
            field_started_quoted = true;
        } else if c == delim {
            record.push(std::mem::take(&mut field));
            field_started_quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            record.push(std::mem::take(&mut field));
            field_started_quoted = false;
            if !on_record(std::mem::take(&mut record)) {
                return;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !record.is_empty() || field_started_quoted {
        record.push(field);
        on_record(record);
    }
}

pub fn read_csv(bytes: &[u8], opts: &CsvOptions) -> Result<Workbook, IoError> {
    let (text, encoding) = decode_text_as(bytes, opts.encoding);
    let quote = if opts.quote == 0 { '"' } else { opts.quote as char };
    let delim = if opts.delimiter == 0 { sniff_delimiter(&text, quote) } else { opts.delimiter } as char;
    if delim == quote {
        return Err(IoError::Format("the delimiter and quote character must differ".into()));
    }
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = u8::try_from(delim).ok();
    // Like the separator: saving this workbook as CSV writes the encoding it was read in.
    wb.csv_encoding = encoding;
    let mut sheet = Sheet::new("Sheet1");
    let mut styles = std::mem::take(&mut wb.styles);
    let mut fmt_cache: Vec<(&'static str, StyleId)> = vec![];
    let mut row: u32 = 0;
    parse_records(&text, delim, quote, |rec| {
        if row >= MAX_ROWS {
            return false;
        }
        // A blank line is an empty row.
        for (col, f) in rec.into_iter().enumerate() {
            if col as u32 >= MAX_COLS {
                break;
            }
            if f.is_empty() {
                continue;
            }
            let pos = CellRef::new(row, col as u32);
            let cell = if opts.parse_values {
                let p = parse_input(&f, opts.date_system);
                let style = match p.format {
                    Some(code) => match fmt_cache.iter().find(|(c, _)| *c == code) {
                        Some((_, id)) => *id,
                        None => {
                            let id = styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
                            fmt_cache.push((code, id));
                            id
                        }
                    },
                    None => StyleId::DEFAULT,
                };
                Cell { value: p.value, formula: None, style }
            } else {
                Cell::value(Value::Text(Arc::from(f)))
            };
            sheet.cells.set(pos, cell);
        }
        row += 1;
        true
    });
    wb.styles = styles;
    wb.sheets = vec![Arc::new(sheet)];
    Ok(wb)
}

/// Writes the sheet's used range as CSV (CRLF line ends, UTF-8 without BOM). `cell_text` gives
/// each cell's field text: the caller decides how values look (Save As CSV writes them as
/// displayed, number formats applied); quoting is done here.
///
/// The output is capped at [`MAX_CSV_BYTES`]: two cells far apart make the used range, and so the
/// file, as big as the whole rectangle between them (`A1` and `XFD1048576` are 16 GiB of commas).
/// An export that would pass the cap is refused before anything is allocated, or as soon as the
/// text written so far passes it, with [`IoError::TooLarge`]; accepted exports keep every field in
/// its place.
pub fn write_csv(sheet: &Sheet, delimiter: u8, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_in(sheet, delimiter, CsvEncoding::Utf8, MAX_CSV_BYTES, cell_text)
}

/// [`write_csv`] with the output cap `limit` (bytes) given.
pub fn write_csv_limited(sheet: &Sheet, delimiter: u8, limit: usize, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_in(sheet, delimiter, CsvEncoding::Utf8, limit, cell_text)
}

/// [`write_csv`] in `encoding`: [`CsvEncoding::Utf8Bom`] writes `EF BB BF` before the UTF-8 bytes,
/// [`CsvEncoding::ShiftJis`] and [`CsvEncoding::Windows1252`] encode in that code page (a character
/// it cannot map becomes `?`), and [`CsvEncoding::Auto`] writes UTF-8 without a BOM.
pub fn write_csv_encoded(sheet: &Sheet, delimiter: u8, encoding: CsvEncoding, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_in(sheet, delimiter, encoding, MAX_CSV_BYTES, cell_text)
}

fn write_csv_in(
    sheet: &Sheet,
    delimiter: u8,
    encoding: CsvEncoding,
    limit: usize,
    mut cell_text: impl FnMut(CellRef) -> String,
) -> Result<Vec<u8>, IoError> {
    let delim = if delimiter == 0 { ',' } else { delimiter as char };
    let mut out = String::new();
    let Some(r) = sheet.used_range() else { return Ok(Vec::new()) };
    let too_large =
        |what: String| IoError::TooLarge(format!("the CSV would be {what}, over the limit of {} MiB; export a smaller range", limit >> 20));
    // Every row has a delimiter between its fields and a CRLF, whatever the cells hold: a lower
    // bound on the output that needs no cell text.
    let (rows, cols) = (u64::from(r.end.row - r.start.row) + 1, u64::from(r.end.col - r.start.col) + 1);
    let per_row = (cols - 1).saturating_mul(delim.len_utf8() as u64).saturating_add(2);
    let least = rows.saturating_mul(per_row);
    if least > limit as u64 {
        return Err(too_large(format!("at least {} MiB ({rows} rows by {cols} columns)", least >> 20)));
    }
    for row in r.start.row..=r.end.row {
        for col in r.start.col..=r.end.col {
            if col > r.start.col {
                out.push(delim);
            }
            let t = cell_text(CellRef::new(row, col));
            if t.contains(delim) || t.contains('"') || t.contains('\n') || t.contains('\r') {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push_str("\r\n");
        if out.len() > limit {
            return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
        }
    }
    // UTF-8 is the widest of the encodings here, so the check above is the conservative one; this
    // exact check catches the BOM pushing a just-under-the-limit export over it.
    let bytes = encode_text(&out, encoding);
    if bytes.len() > limit {
        return Err(too_large(format!("more than {} MiB when encoded as {}", limit >> 20, encoding.as_str())));
    }
    Ok(bytes)
}
