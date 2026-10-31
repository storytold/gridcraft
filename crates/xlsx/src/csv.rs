//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::borrow::Cow;
use std::sync::Arc;

use gridcraft_core::parse::{Parsed, parse_input};
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::text_layout::{
    self, Completeness, Decimal, Evidence, Numbers, Split, TextReport, detect, detect_numbers, evidence, most_common_fields, records_before_table,
    sample_rows, split_records,
};
use crate::{CsvOptions, IoError};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Windows-1256 code points for bytes 0x80–0xFF (unicode.org `CP1256.TXT`).
pub(crate) const CP1256: [u32; 128] = [
    0x20AC, 0x067E, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0679, 0x2039, 0x0152, 0x0686, 0x0698, 0x0688, 0x06AF, 0x2018,
    0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x06A9, 0x2122, 0x0691, 0x203A, 0x0153, 0x200C, 0x200D, 0x06BA, 0x00A0, 0x060C, 0x00A2, 0x00A3,
    0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x06BE, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5,
    0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x061B, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x061F, 0x06C1, 0x0621, 0x0622, 0x0623, 0x0624, 0x0625, 0x0626, 0x0627,
    0x0628, 0x0629, 0x062A, 0x062B, 0x062C, 0x062D, 0x062E, 0x062F, 0x0630, 0x0631, 0x0632, 0x0633, 0x0634, 0x0635, 0x0636, 0x00D7, 0x0637, 0x0638,
    0x0639, 0x063A, 0x0640, 0x0641, 0x0642, 0x0643, 0x00E0, 0x0644, 0x00E2, 0x0645, 0x0646, 0x0647, 0x0648, 0x00E7, 0x00E8, 0x00E9, 0x00EA, 0x00EB,
    0x0649, 0x064A, 0x00EE, 0x00EF, 0x064B, 0x064C, 0x064D, 0x064E, 0x00F4, 0x064F, 0x0650, 0x00F7, 0x0651, 0x00F9, 0x0652, 0x00FB, 0x00FC, 0x200E,
    0x200F, 0x06D2,
];

/// Evidence that bytes which aren't UTF-8 are Windows-1256 Arabic rather than Windows-1252.
#[derive(Default)]
struct Cp1256Evidence {
    /// High bytes that decode to different characters in the two code pages, apart from invisible
    /// marks and ordinals after a digit.
    differing: usize,
    /// Of those, Arabic letters and Arabic punctuation (، ؛ ؟) in Windows-1256.
    arabic: usize,
    /// Arabic bytes next to an ASCII letter: Western accents sit inside Latin words, Arabic doesn't.
    arabic_touching_latin: usize,
    /// Accented Latin bytes the two code pages share, next to another high byte: Hebrew
    /// Windows-1255 letters land there, Arabic and Western text rarely do.
    shared_letters_in_runs: usize,
}

impl Cp1256Evidence {
    fn of(bytes: &[u8]) -> Self {
        let mut e = Self::default();
        for (i, &b) in bytes.iter().enumerate() {
            if b < 0x80 {
                continue;
            }
            let before = i.checked_sub(1).and_then(|j| bytes.get(j)).copied();
            let after = bytes.get(i + 1).copied();
            let next_to = |pred: fn(u8) -> bool| before.is_some_and(pred) || after.is_some_and(pred);
            if same_in_cp1252_and_cp1256(b) {
                if is_shared_latin_letter(b) && next_to(|x| x >= 0x80) {
                    e.shared_letters_in_runs += 1;
                }
                continue;
            }
            if is_invisible_mark(b) || is_ordinal_after_digit(b, before) {
                continue;
            }
            e.differing += 1;
            if is_cp1256_arabic(b) {
                e.arabic += 1;
                if next_to(|x| x.is_ascii_alphabetic()) {
                    e.arabic_touching_latin += 1;
                }
            }
        }
        e
    }

    /// Thresholds chosen against month, day and language names from 95 glibc locales (Western in
    /// Windows-1252, Arabic, Persian and Urdu in 1256, Hebrew and Yiddish in 1255) plus CSV samples.
    fn is_arabic(&self) -> bool {
        self.arabic >= 2
            && self.arabic * 10 >= self.differing * 9
            && self.arabic_touching_latin * 10 <= self.arabic * 3
            && self.shared_letters_in_runs * 20 <= self.arabic
    }
}

fn same_in_cp1252_and_cp1256(b: u8) -> bool {
    matches!(
        b,
        0x80 | 0x82..=0x89 | 0x8B..=0x8C | 0x91..=0x97 | 0x99 | 0x9B..=0x9C | 0xA0 | 0xA2..=0xA9 | 0xAB..=0xB9 | 0xBB..=0xBE | 0xD7 | 0xE0 | 0xE2 | 0xE7..=0xEB
            | 0xEE..=0xEF | 0xF4 | 0xF7 | 0xF9 | 0xFB..=0xFC
    )
}

/// ZWNJ, ZWJ, LRM and RLM: common inside Persian words and around Arabic cells, but no evidence
/// either way.
fn is_invisible_mark(b: u8) -> bool {
    matches!(b, 0x9D | 0x9E | 0xFD | 0xFE)
}

/// `º` and `ª` after a digit (`1º`, `2ª`) are Western ordinals, though Windows-1256 reads the bytes
/// as `؛` and `ھ`; either way they are no evidence.
fn is_ordinal_after_digit(b: u8, before: Option<u8>) -> bool {
    matches!(b, 0xAA | 0xBA) && before.is_some_and(|x| x.is_ascii_digit())
}

fn is_shared_latin_letter(b: u8) -> bool {
    matches!(b, 0xE0 | 0xE2 | 0xE7..=0xEB | 0xEE..=0xEF | 0xF4 | 0xF9 | 0xFB..=0xFC)
}

fn is_cp1256_arabic(b: u8) -> bool {
    matches!(
        b,
        0x81 | 0x8A | 0x8D..=0x90 | 0x98 | 0x9A | 0x9F | 0xA1 | 0xAA | 0xBA | 0xBF..=0xD6 | 0xD8..=0xDB | 0xDD..=0xDF | 0xE1 | 0xE3..=0xE6 | 0xEC..=0xED | 0xFF
    )
}

/// Decodes CSV bytes with BOM-aware UTF-8 and UTF-16 handling. Without a BOM, valid UTF-8 is kept.
/// Other bytes decode as Windows-1256 when they read as Arabic (see [`Cp1256Evidence`]), else as
/// Windows-1252.
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
        Err(_) => {
            let use_windows_1256 = Cp1256Evidence::of(bytes).is_arabic();
            bytes
                .iter()
                .map(|&b| match b {
                    0x80..=0xFF if use_windows_1256 => {
                        char::from_u32(CP1256.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}')
                    }
                    0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
                    _ => b as char,
                })
                .collect()
        }
    }
}

/// Splits CSV text into records of fields. A record keeps at most [`MAX_COLS`] fields; the rest are
/// dropped as they are read, so a row of endless delimiters cannot allocate a string per field.
pub(crate) fn parse_records(text: &str, delim: char, quote: char, mut on_record: impl FnMut(Vec<String>) -> bool) {
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
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
            if !on_record(std::mem::take(&mut record)) {
                return;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !record.is_empty() || field_started_quoted {
        if record.len() < MAX_COLS as usize {
            record.push(field);
        }
        on_record(record);
    }
}

pub fn read_csv(bytes: &[u8], opts: &CsvOptions) -> Result<Workbook, IoError> {
    read_text(bytes, opts).map(|(wb, _)| wb)
}

/// The settings a text file is read with, given or detected.
struct Resolved {
    text: String,
    quote: Option<char>,
    split: Split,
    layout: Option<text_layout::Layout>,
    start: usize,
    fields: usize,
    numbers: Numbers,
}

impl Resolved {
    fn new(bytes: &[u8], opts: &CsvOptions) -> Result<Resolved, IoError> {
        let text = decode_text(bytes);
        let quote = (!opts.no_qualifier).then_some(if opts.quote == 0 { '"' } else { opts.quote as char });
        let (split, layout) = match (&opts.split, opts.delimiter) {
            (Some(split), _) => (split.clone(), None),
            (None, 0) => {
                let layout = detect(&text, opts.kind, quote, opts.detect_any);
                (layout.split.clone(), Some(layout))
            }
            (None, d) => (Split::delimiter(d as char), None),
        };
        if let (Some(q), Split::Delimited { delimiters, .. }) = (quote, &split)
            && delimiters.iter().any(|d| d.contains(q))
        {
            return Err(IoError::Format("the delimiter and quote character must differ".into()));
        }
        let start = match opts.start_row {
            Some(n) => n,
            None => layout.as_ref().map_or(0, |l| records_before_table(&text, &split, quote, l.start_row)),
        };
        let sample = sample_rows(&text, &split, quote, start);
        let fields = layout.as_ref().map_or_else(|| most_common_fields(&sample), |l| l.fields);
        let numbers = match opts.decimal {
            Some(d) => Numbers::given(d),
            None => detect_numbers(&sample, &split),
        };
        Ok(Resolved { text, quote, split, layout, start, fields, numbers })
    }

    fn report(self, done: Completeness) -> TextReport {
        TextReport {
            split: self.split,
            detected: self.layout.is_some(),
            start_row: self.start,
            numbers: self.numbers,
            fields: self.fields,
            rows: done.rows,
            complete: done.complete,
            other_rows: done.other_rows,
            split_numbers: done.split_numbers,
            uncertain: self.layout.map(|l| l.uncertain).unwrap_or_default(),
        }
    }
}

/// The Text Import Wizard's preview: the first `rows` rows of the file as `opts` splits them,
/// fields as the text they hold, and what reading found (counted over the whole of `bytes`).
pub fn preview_text(bytes: &[u8], opts: &CsvOptions, rows: usize) -> Result<(Vec<Vec<String>>, TextReport), IoError> {
    let r = Resolved::new(bytes, opts)?;
    let mut out = vec![];
    let mut record = 0usize;
    let mut done = Completeness { fields: r.fields, ..Default::default() };
    split_records(&r.text, &r.split, r.quote, |rec| {
        record += 1;
        if record > r.start {
            done.add(record, &rec, &r.split, r.numbers.decimal);
            if out.len() < rows {
                out.push(rec);
            }
        }
        record < MAX_ROWS as usize
    });
    Ok((out, r.report(done)))
}

/// [`read_csv`], with what reading found: the split used (detected when neither `split` nor
/// `delimiter` is given), the decimal separator, and how complete the rows are over the file.
pub fn read_text(bytes: &[u8], opts: &CsvOptions) -> Result<(Workbook, TextReport), IoError> {
    let resolved = Resolved::new(bytes, opts)?;
    let Resolved { text, quote, split, start, numbers, .. } = &resolved;
    let (quote, start) = (*quote, *start);
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = split.single_byte();
    wb.csv_decimal_comma = opts.parse_values && numbers.decimal == Decimal::Comma;
    let mut sheet = Sheet::new("Sheet1");
    let mut styles = std::mem::take(&mut wb.styles);
    let mut fmt_cache: Vec<(Cow<'static, str>, StyleId)> = vec![];
    let mut row: u32 = 0;
    let mut record = 0usize;
    let mut done = Completeness { fields: resolved.fields, ..Default::default() };
    split_records(text, split, quote, |rec| {
        record += 1;
        if record <= start {
            return true;
        }
        if row >= MAX_ROWS {
            return false;
        }
        done.add(record, &rec, split, numbers.decimal);
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
                let p = read_field(&f, numbers.column(col), opts.date_system);
                let style = match p.format {
                    Some(code) => match fmt_cache.iter().find(|(c, _)| *c == code) {
                        Some((_, id)) => *id,
                        None => {
                            let id = styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(&code));
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
    Ok((wb, resolved.report(done)))
}

/// A field's value and the number format it gets.
struct Read {
    value: Value,
    format: Option<Cow<'static, str>>,
}

impl From<Parsed> for Read {
    fn from(p: Parsed) -> Read {
        Read { value: p.value, format: p.format.map(Cow::Borrowed) }
    }
}

/// One field read like typed input, with the column's decimal separator: a decimal comma
/// number is rewritten with a point first (`1.234,5` → `1,234.5`); a value that is a number
/// only with the other separator stays text, as it does in Excel (`0.125` under a decimal
/// comma), and `1,5E3` is 1500. In a column holding both kinds (`None`), any value that depends
/// on the separator stays text. Dates with dots are day first (`01.11.2026`), amounts carry any
/// currency sign (`7,50 €`), Swiss `'` groups and a trailing minus read (`1'234.50`, `7,50-`).
fn read_field(f: &str, decimal: Option<Decimal>, sys: gridcraft_core::DateSystem) -> Read {
    let text = || Read { value: Value::text(f), format: None };
    if let Some(date) = dotted_date(f, sys) {
        return date;
    }
    let number = |p: Parsed| matches!(p.value, Value::Number(_)).then(|| Read::from(p));
    let amount = |d: Decimal| text_layout::currency_amount(f, d).map(|(v, code)| Read { value: Value::Number(v), format: Some(Cow::Owned(code)) });
    // Typed input, else the same number with a point and `,` groups, else an amount.
    let with_point = |p: Parsed| {
        number(p.clone())
            .or_else(|| text_layout::to_point(f, Decimal::Point).and_then(|t| number(parse_input(&t, sys))))
            .or_else(|| amount(Decimal::Point))
            .unwrap_or_else(|| p.into())
    };
    match decimal {
        Some(Decimal::Point) => with_point(parse_input(f, sys)),
        Some(Decimal::Comma) => match text_layout::comma_to_point(f) {
            // Rewritten for the reader only: what doesn't read as a number keeps the file's text.
            Some(point) => number(parse_input(&point, sys)).or_else(|| amount(Decimal::Comma)).unwrap_or_else(text),
            None if evidence(f) == Some(Evidence::Point) => text(),
            None => {
                let p = parse_input(f, sys);
                let date = p.format.is_some_and(|code| code.chars().any(|c| matches!(c.to_ascii_lowercase(), 'y' | 'd' | 'h' | 'm' | 's')));
                if matches!(p.value, Value::Number(_)) && f.contains(['.', ',']) && !date { text() } else { p.into() }
            }
        },
        None => match evidence(f) {
            Some(Evidence::Point | Evidence::Comma | Evidence::Ambiguous) => text(),
            _ => with_point(parse_input(f, sys)),
        },
    }
}

/// `25.10.2026`, `1.11.26`, with a time or without (`25.10.2026 12:30`): day first, as
/// everywhere dates are written with dots; the input parser reads them month first.
fn dotted_date(f: &str, sys: gridcraft_core::DateSystem) -> Option<Read> {
    let t = f.trim();
    let (date, time) = t.split_once(' ').map_or((t, None), |(d, tm)| (d, Some(tm.trim())));
    let parts: Vec<&str> = date.split('.').collect();
    let [d, m, y] = parts.as_slice() else { return None };
    let digits = |s: &str, lens: &[usize]| lens.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(d, &[1, 2]) || !digits(m, &[1, 2]) || !digits(y, &[2, 4]) {
        return None;
    }
    let (d, m, y): (i64, i64, i64) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
    let y = match y {
        0..30 => 2000 + y,
        30..100 => 1900 + y,
        _ => y,
    };
    let month = u32::try_from(m).ok().filter(|m| (1..=12).contains(m))?;
    if y < 1900 || d < 1 || d > i64::from(gridcraft_core::date::days_in_month_in(sys, i32::try_from(y).ok()?, month)) {
        return None;
    }
    let day = gridcraft_core::date::serial_from_ymd(sys, y, m, d)?;
    match time {
        None => Some(Read { value: Value::Number(day), format: Some(Cow::Borrowed("dd.mm.yyyy")) }),
        Some(tm) => match parse_input(tm, sys) {
            Parsed { value: Value::Number(fr), format: Some(code) } if (0.0..1.0).contains(&fr) && code.contains('h') => {
                Some(Read { value: Value::Number(day + fr), format: Some(Cow::Borrowed("dd.mm.yyyy hh:mm")) })
            }
            _ => None,
        },
    }
}

/// Writes the sheet as CSV from A1 to the end of its used range, so leading empty columns and rows
/// stay as empty fields and every cell keeps its position (CRLF line ends, UTF-8 without BOM). `cell_text` gives
/// each cell's field text: the caller decides how values look (Save As CSV writes them as
/// displayed, number formats applied); quoting is done here.
///
/// The output is capped at [`MAX_CSV_BYTES`]: a cell far from A1 makes the file as big as the whole
/// rectangle between them (`A1` and `XFD1048576` are 16 GiB of commas).
/// An export that would pass the cap is refused before anything is allocated, or at the first
/// field or line end that takes it past the cap (a field is measured before it is written), with
/// [`IoError::TooLarge`]; accepted exports keep every field in its place.
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
    let (rows, cols) = (u64::from(r.end.row) + 1, u64::from(r.end.col) + 1);
    let per_row = (cols - 1).saturating_mul(delim.len_utf8() as u64).saturating_add(2);
    let least = rows.saturating_mul(per_row);
    if least > limit as u64 {
        return Err(too_large(format!("at least {} MiB ({rows} rows by {cols} columns)", least >> 20)));
    }
    for row in 0..=r.end.row {
        for col in 0..=r.end.col {
            if col > 0 {
                out.push(delim);
            }
            let t = cell_text(CellRef::new(row, col));
            let quoted = t.contains(delim) || t.contains('"') || t.contains('\n') || t.contains('\r');
            // The exact size of the field as written (the delimiter was pushed above), so a row
            // never grows past the cap unseen and no doubled-quote copy of the text is built.
            let field_len = if quoted { t.len() + t.matches('"').count() + 2 } else { t.len() };
            if out.len().saturating_add(field_len) > limit {
                return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
            }
            if quoted {
                out.push('"');
                for ch in t.chars() {
                    if ch == '"' {
                        out.push('"');
                    }
                    out.push(ch);
                }
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
