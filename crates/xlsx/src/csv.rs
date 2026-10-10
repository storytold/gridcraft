//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::sync::Arc;

use gridcraft_core::parse::parse_input_in;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_locale::Locale;
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::{CsvOptions, IoError};

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Decodes CSV bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Windows-1252.
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
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
                _ => b as char,
            })
            .collect(),
    }
}

/// Picks comma, semicolon or tab: the candidate with the most consistent count per line
/// (outside quotes) over the first non-blank lines. A candidate must appear on every such line,
/// otherwise comma wins. En-US conventions; see [`sniff_delimiter_in`].
#[cfg(test)]
pub fn sniff_delimiter(text: &str, quote: char) -> u8 {
    sniff_delimiter_in(text, quote, &gridcraft_locale::INVARIANT.regional)
}

/// [`sniff_delimiter`] for a region: the region's list separator is the first candidate and the
/// default (it wins when no candidate appears on every line), and where the decimal separator is
/// a comma a comma is never chosen (it belongs to the numbers, so Excel splits such files on the
/// list separator).
pub fn sniff_delimiter_in(text: &str, quote: char, reg: &gridcraft_locale::Regional) -> u8 {
    let mut candidates: Vec<char> = vec![reg.list];
    for d in [',', ';', '\t'] {
        if !candidates.contains(&d) && !(d == ',' && reg.decimal == ',') {
            candidates.push(d);
        }
    }
    let first = u8::try_from(reg.list).unwrap_or(b',');
    let mut best = (first, 0usize, 0usize);
    for d in candidates {
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
    let delim = if opts.delimiter == 0 { sniff_delimiter_in(&text, quote, &opts.locale.regional) } else { opts.delimiter } as char;
    if delim == quote {
        return Err(IoError::Format("the delimiter and quote character must differ".into()));
    }
    let mut wb = Workbook::new_in(Arc::new(opts.locale));
    wb.date_system = opts.date_system;
    let mut sheet = Sheet::new(format!("{}1", opts.locale.ui.content("sheet")));
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
                let p = parse_input_in(&f, opts.date_system, &opts.locale.regional, opts.locale.formula);
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

/// A value as the file spells it in `loc`: the region's decimal separator, the formula
/// language's booleans and error names.
fn field_text(v: &Value, loc: &Locale) -> String {
    match v {
        Value::Error(e) => loc.formula.local_error(e.as_str()).to_string(),
        Value::Array(a) => a.get(0, 0).map(|x| field_text(x, loc)).unwrap_or_default(),
        v => v.to_text_in(loc).unwrap_or_default(),
    }
}

/// Writes the sheet's used range as CSV (raw values, CRLF line ends, UTF-8 without BOM).
/// Numbers, booleans and errors are spelled in the workbook's locale. A `delimiter` of `0`
/// uses the region's list separator (`;` where the decimal separator is a comma).
pub fn write_csv(sheet: &Sheet, wb: &Workbook, delimiter: u8) -> Vec<u8> {
    let loc: &Locale = &wb.locale;
    let delim = if delimiter == 0 { loc.regional.list } else { delimiter as char };
    let mut out = String::new();
    let Some(r) = sheet.used_range() else { return Vec::new() };
    for row in r.start.row..=r.end.row {
        for col in r.start.col..=r.end.col {
            if col > r.start.col {
                out.push(delim);
            }
            let t = field_text(&sheet.value(CellRef::new(row, col)), loc);
            if t.contains(delim) || t.contains('"') || t.contains('\n') || t.contains('\r') {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

#[cfg(test)]
mod locale_tests {
    use gridcraft_core::DateSystem;

    use super::*;

    fn pt_region() -> Locale {
        let region = gridcraft_locale::region("pt-BR").copied().unwrap_or(gridcraft_locale::INVARIANT.regional);
        Locale::new(gridcraft_locale::INVARIANT.ui, gridcraft_locale::INVARIANT.formula, region)
    }

    #[test]
    fn export_uses_the_region_list_separator_and_decimal() {
        let loc = pt_region();
        let mut wb = Workbook::new_in(Arc::new(loc));
        let mut s = Sheet::new("S");
        s.set_value(CellRef::new(0, 0), Value::Number(1.5));
        s.set_value(CellRef::new(0, 1), Value::Number(2.0));
        s.set_value(CellRef::new(0, 2), Value::text("a;b"));
        wb.sheets = vec![Arc::new(s)];
        let sheet = wb.sheet(0).cloned().unwrap_or_else(|| Sheet::new("S"));
        let default = String::from_utf8(write_csv(&sheet, &wb, 0)).unwrap_or_default();
        assert_eq!(default, "1,5;2;\"a;b\"\r\n");
        let comma = String::from_utf8(write_csv(&sheet, &wb, b'\t')).unwrap_or_default();
        assert_eq!(comma, "1,5\t2\ta;b\r\n");
        // The canonical workbook keeps comma-separated invariant numbers.
        let plain = Workbook::new();
        let invariant = String::from_utf8(write_csv(&sheet, &plain, 0)).unwrap_or_default();
        assert_eq!(invariant, "1.5,2,a;b\r\n");
    }

    #[test]
    fn import_reads_numbers_by_region() {
        let opts = CsvOptions { delimiter: 0, locale: pt_region(), ..Default::default() };
        let wb = read_csv(b"x;y\n1,5;10/10/2026", &opts).unwrap_or_default();
        let sh = wb.sheet(0);
        assert_eq!(sh.map(|s| s.value(CellRef::new(1, 0))), Some(Value::Number(1.5)));
        // Day/month/year: 10 October 2026.
        let date = sh.map(|s| s.value(CellRef::new(1, 1))).and_then(|v| v.as_f64());
        let expected = parse_input_in("2026-10-10", DateSystem::D1900, &opts.locale.regional, opts.locale.formula).value.as_f64();
        assert!(date.is_some());
        assert_eq!(date, expected);
    }
}
