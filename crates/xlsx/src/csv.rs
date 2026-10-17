//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::sync::Arc;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::{CsvOptions, IoError};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Decodes CSV bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Windows-1255 when the bytes read as
/// Hebrew ([`looks_hebrew`]: Excel saves a Hebrew sheet's CSV in the Hebrew Windows code page),
/// else Windows-1252.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let units = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) if looks_hebrew(bytes) => bytes.iter().map(|&b| windows_1255(b)).collect(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
                _ => b as char,
            })
            .collect(),
    }
}

/// Whether 8-bit text is Hebrew in Windows-1255 (or ISO 8859-8, which shares its letters): its
/// high bytes are mostly Hebrew letters (`0xE0`–`0xFA`) standing next to each other, as the letters
/// of Hebrew words do, rather than accented Latin letters inside Latin words (French or German in
/// Windows-1252 use the same bytes). A byte Hebrew never uses (`0xD9`–`0xDF`, `0xFB`, `0xFC`,
/// `0xFF`: frequent Cyrillic, Arabic and Greek letters in their code pages) rules it out.
pub fn looks_hebrew(bytes: &[u8]) -> bool {
    let letter = |b: u8| (0xE0..=0xFA).contains(&b);
    let (mut paired, mut mixed) = (0usize, 0usize);
    for (i, &b) in bytes.iter().enumerate() {
        if matches!(b, 0xD9..=0xDF | 0xFB | 0xFC | 0xFF) {
            return false;
        }
        if !letter(b) {
            continue;
        }
        let next = bytes.get(i + 1).copied();
        let prev = i.checked_sub(1).and_then(|p| bytes.get(p)).copied();
        if next.is_some_and(letter) {
            paired = paired.saturating_add(1);
        }
        if next.is_some_and(|n| n.is_ascii_alphabetic()) || prev.is_some_and(|p| p.is_ascii_alphabetic()) {
            mixed = mixed.saturating_add(1);
        }
    }
    paired >= 2 && paired > mixed.saturating_mul(2)
}

/// One Windows-1255 byte as a character. The punctuation at `0x80`–`0x9F` is Windows-1252's.
fn windows_1255(b: u8) -> char {
    let code = match b {
        0x80..=0x9F => CP1252.get(usize::from(b - 0x80)).copied().unwrap_or(0xFFFD),
        0xA4 => 0x20AA,                              // new sheqel sign
        0xAA => 0x00D7,                              // multiplication sign
        0xBA => 0x00F7,                              // division sign
        0xC0..=0xD3 => 0x05B0 + u32::from(b - 0xC0), // points (niqqud) and punctuation
        0xD4..=0xD8 => 0x05F0 + u32::from(b - 0xD4), // Yiddish ligatures, geresh, gershayim
        0xE0..=0xFA => 0x05D0 + u32::from(b - 0xE0), // alef to tav
        0xFD => 0x200E,
        0xFE => 0x200F,
        0xD9..=0xDF | 0xFB | 0xFC | 0xFF => 0xFFFD,
        _ => u32::from(b),
    };
    char::from_u32(code).unwrap_or('\u{FFFD}')
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
    let text = decode_text(bytes);
    let quote = if opts.quote == 0 { '"' } else { opts.quote as char };
    let delim = if opts.delimiter == 0 { sniff_delimiter(&text, quote) } else { opts.delimiter } as char;
    if delim == quote {
        return Err(IoError::Format("the delimiter and quote character must differ".into()));
    }
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = u8::try_from(delim).ok();
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
    write_csv_limited(sheet, delimiter, MAX_CSV_BYTES, cell_text)
}

/// [`write_csv`] with the output cap `limit` (bytes) given.
pub fn write_csv_limited(sheet: &Sheet, delimiter: u8, limit: usize, mut cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
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
    Ok(out.into_bytes())
}
