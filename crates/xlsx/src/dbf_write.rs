//! Fresh dBASE III table export, not schema-preserving database editing.

use std::collections::{HashMap, HashSet};

use gridcraft_core::date::{datetime_from_serial, days_in_month, serial_from_unix_ms, unix_now_ms};
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Sheet, StyleId, Workbook};
use yore::code_pages::CP1252;

use crate::IoError;
use crate::dbf::{MAX_CELLS, MAX_TEXT, numeric_as_text};

const MAX_BYTES: usize = 512 << 20;

/// Number-format meaning supplied by the caller. Interpretation belongs outside the
/// file-format layer; time-only and conditional date formats cannot be DBF dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbfNumberFormat {
    Number,
    Date,
    UnsupportedDate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
    Logical,
    Date,
}

struct Field {
    name: String,
    kind: Option<Kind>,
    width: usize,
    decimals: usize,
    integer_width: usize,
}

fn invalid(detail: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("DBF export: {detail}"))
}

/// Exports the active sheet's used rectangle as a classic dBASE III table. Its first row
/// must contain unique, ASCII field names (up to 10 characters). Columns must have one
/// scalar type, allowing blanks. Only values are exported, with Windows-1252 text.
/// Anything that would require truncation, rounding or replacing characters is refused.
/// `classify` interprets number-format codes; the writer caches its result per style.
pub fn write_dbf(wb: &Workbook, sheet_index: usize, classify: impl Fn(&str) -> DbfNumberFormat) -> Result<Vec<u8>, IoError> {
    write_limited(wb, sheet_index, MAX_TEXT, classify)
}

fn write_limited(wb: &Workbook, sheet_index: usize, text_limit: usize, classify: impl Fn(&str) -> DbfNumberFormat) -> Result<Vec<u8>, IoError> {
    let sheet = wb.sheet(sheet_index).ok_or_else(|| invalid("missing sheet"))?;
    let range = sheet.used_range().ok_or_else(|| invalid("the sheet has no field headers"))?;
    if range.end.row >= MAX_ROWS || range.end.col >= MAX_COLS {
        return Err(invalid("the used range lies outside the worksheet grid"));
    }
    let columns = range.end.col - range.start.col + 1;
    let records = range.end.row - range.start.row;
    if columns > 255 {
        return Err(invalid("classic DBF supports at most 255 fields"));
    }
    if (u64::from(records) + 1) * u64::from(columns) > MAX_CELLS {
        return Err(IoError::TooLarge("DBF export exceeds the 5 million field-value budget".into()));
    }
    let mut names = HashSet::new();
    let mut fields = Vec::with_capacity(columns as usize);
    let mut formats = HashMap::new();
    for col in range.start.col..=range.end.col {
        let pos = CellRef::new(range.start.row, col);
        let Value::Text(name) = sheet.value(pos) else { return Err(invalid(format!("{pos}: the first row must contain text field names"))) };
        if name.is_empty()
            || name.len() > 10
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name.as_bytes().first().is_none_or(|b| !b.is_ascii_alphabetic() && *b != b'_')
        {
            return Err(invalid(format!(
                "{pos}: field names must start with a letter or underscore and contain at most 10 ASCII letters, digits or underscores"
            )));
        }
        if !names.insert(name.to_ascii_uppercase()) {
            return Err(invalid(format!("{pos}: duplicate field name '{name}'")));
        }
        let mut field = Field { name: name.to_string(), kind: None, width: 1, decimals: 0, integer_width: 1 };
        for row in range.start.row + 1..=range.end.row {
            let pos = CellRef::new(row, col);
            let value = sheet.value(pos);
            if value == Value::Empty {
                continue;
            }
            let kind = kind(wb, sheet, pos, &value, &mut formats, &classify)?;
            if field.kind.is_some_and(|previous| previous != kind) {
                return Err(invalid(format!("{pos}: field '{}' mixes data types; use one type per column", field.name)));
            }
            field.kind = Some(kind);
            match &value {
                Value::Text(text) => {
                    let text = encode(text, pos)?;
                    if text.len() > 254 {
                        return Err(invalid(format!("{pos}: text exceeds the 254-byte field limit")));
                    }
                    field.width = field.width.max(text.len());
                }
                Value::Number(number) if kind == Kind::Number => {
                    let text = number.to_string();
                    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
                    field.integer_width = field.integer_width.max(integer.len());
                    field.decimals = field.decimals.max(fraction.len());
                    if field.decimals > 15 {
                        return Err(invalid(format!("{pos}: the value requires more than 15 decimal places")));
                    }
                    field.width = field.integer_width + if field.decimals == 0 { 0 } else { 1 + field.decimals };
                    if field.width > 20 {
                        return Err(invalid(format!("{pos}: the numeric column exceeds the 20-byte field limit")));
                    }
                }
                Value::Number(_) => field.width = 8,
                Value::Bool(_) => field.width = 1,
                _ => return Err(invalid(format!("{pos}: unsupported value"))),
            }
        }
        fields.push(field);
    }
    let record_len = 1 + fields.iter().map(|field| field.width).sum::<usize>();
    if record_len > 4000 {
        return Err(invalid("classic DBF records cannot exceed 4,000 bytes; export fewer or narrower columns"));
    }
    let header_len = 33 + fields.len() * 32;
    let size = (records as usize)
        .checked_mul(record_len)
        .and_then(|n| n.checked_add(header_len + 1))
        .filter(|n| *n <= MAX_BYTES)
        .ok_or_else(|| IoError::TooLarge("DBF output exceeds 512 MiB".into()))?;
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|_| IoError::TooLarge("not enough memory for DBF output".into()))?;
    out.resize(header_len, 0);
    put(&mut out, 0, &[3])?;
    let date = unix_now_ms()
        .and_then(|ms| datetime_from_serial(DateSystem::D1900, serial_from_unix_ms(ms)))
        .filter(|date| (1900..=2155).contains(&date.year))
        .ok_or_else(|| invalid("the system date cannot be represented in the DBF header"))?;
    put(&mut out, 1, &[(date.year - 1900) as u8, date.month as u8, date.day as u8])?;
    put(&mut out, 4, &records.to_le_bytes())?;
    put(&mut out, 8, &(header_len as u16).to_le_bytes())?;
    put(&mut out, 10, &(record_len as u16).to_le_bytes())?;
    put(&mut out, 29, &[3])?; // Windows-1252.
    for (index, field) in fields.iter().enumerate() {
        let offset = 32 + index * 32;
        put(&mut out, offset, field.name.as_bytes())?;
        let kind = match field.kind.unwrap_or(Kind::Text) {
            Kind::Text => b'C',
            Kind::Number => b'N',
            Kind::Logical => b'L',
            Kind::Date => b'D',
        };
        put(&mut out, offset + 11, &[kind])?;
        put(&mut out, offset + 16, &[field.width as u8, field.decimals as u8])?;
    }
    put(&mut out, header_len - 1, &[0x0d])?;
    let mut decoded_text = 0usize;
    for row in range.start.row + 1..=range.end.row {
        out.push(b' '); // Live record.
        for (field, col) in fields.iter().zip(range.start.col..=range.end.col) {
            let pos = CellRef::new(row, col);
            let value = sheet.value(pos);
            let raw = match value {
                Value::Empty => Vec::new(),
                Value::Text(text) => {
                    decoded_text = decoded_text.saturating_add(text.len());
                    encode(&text, pos)?
                }
                Value::Bool(value) => vec![if value { b'T' } else { b'F' }],
                Value::Number(number) if field.kind == Some(Kind::Date) => date_bytes(wb.date_system, number, pos)?,
                Value::Number(number) => {
                    // The descriptor records the common scale; shortest decimal storage avoids
                    // manufacturing trailing digits that our importer would preserve as text.
                    let text = number.to_string();
                    if text.parse::<f64>().ok() != Some(number) {
                        return Err(invalid(format!("{pos}: the numeric column's decimal scale would round this value")));
                    }
                    if numeric_as_text(&text) {
                        decoded_text = decoded_text.saturating_add(text.len());
                    }
                    text.into_bytes()
                }
                _ => return Err(invalid(format!("{pos}: unsupported value"))),
            };
            if decoded_text > text_limit {
                return Err(IoError::TooLarge("DBF decoded text exceeds 64 MiB; export a smaller table so it can be reopened".into()));
            }
            let padding = field.width.checked_sub(raw.len()).ok_or_else(|| invalid(format!("{pos}: field width overflow")))?;
            if field.kind == Some(Kind::Number) {
                out.resize(out.len() + padding, b' ');
                out.extend_from_slice(&raw);
            } else {
                out.extend_from_slice(&raw);
                out.resize(out.len() + padding, b' ');
            }
        }
    }
    out.push(0x1a);
    Ok(out)
}

fn kind(
    wb: &Workbook,
    sheet: &Sheet,
    pos: CellRef,
    value: &Value,
    formats: &mut HashMap<StyleId, DbfNumberFormat>,
    classify: &impl Fn(&str) -> DbfNumberFormat,
) -> Result<Kind, IoError> {
    match value {
        Value::Text(_) => Ok(Kind::Text),
        Value::Bool(_) => Ok(Kind::Logical),
        Value::Number(number) if number.is_finite() => {
            let style = sheet.style_id(pos);
            if formats.len() >= 256 && !formats.contains_key(&style) {
                formats.clear();
            }
            let format = *formats.entry(style).or_insert_with(|| classify(wb.styles.get(style).num_fmt.as_str()));
            if format == DbfNumberFormat::UnsupportedDate {
                return Err(invalid(format!(
                    "{pos}: use an unconditional calendar date format; DBF cannot represent time-only or conditional date fields"
                )));
            }
            if format == DbfNumberFormat::Date {
                date_bytes(wb.date_system, *number, pos)?;
                Ok(Kind::Date)
            } else {
                Ok(Kind::Number)
            }
        }
        _ => Err(invalid(format!("{pos}: error values and non-finite numbers cannot be exported"))),
    }
}

fn encode(text: &str, pos: CellRef) -> Result<Vec<u8>, IoError> {
    // Check the length first: an untrusted cell must not cause an unbounded encoding allocation.
    if text.chars().take(255).count() > 254 {
        return Err(invalid(format!("{pos}: text exceeds the 254-byte field limit")));
    }
    if text.contains('\0') || text.ends_with(' ') {
        return Err(invalid(format!("{pos}: NUL characters and significant trailing spaces cannot round-trip through DBF")));
    }
    CP1252.encode(text).map(|s| s.into_owned()).map_err(|_| invalid(format!("{pos}: text cannot be encoded as Windows-1252")))
}

fn date_bytes(system: DateSystem, value: f64, pos: CellRef) -> Result<Vec<u8>, IoError> {
    let date = datetime_from_serial(system, value)
        .filter(|date| value.fract() == 0.0 && date.year >= 1900 && date.day <= days_in_month(date.year, date.month))
        .ok_or_else(|| invalid(format!("{pos}: DBF dates must be real calendar dates from 1900 through 9999 with no time component")))?;
    Ok(format!("{:04}{:02}{:02}", date.year, date.month, date.day).into_bytes())
}

fn put(out: &mut [u8], offset: usize, bytes: &[u8]) -> Result<(), IoError> {
    let end = offset.checked_add(bytes.len()).ok_or_else(|| invalid("header size overflow"))?;
    out.get_mut(offset..end).ok_or_else(|| invalid("header field out of bounds"))?.copy_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_dbf;
    use gridcraft_core::date::serial_from_ymd;
    use gridcraft_model::NumFmt;

    // Fixtures specify format meaning; engine tests exercise the actual parser.
    fn classify(code: &str) -> DbfNumberFormat {
        match code {
            "yyyy-mm-dd" | "yyyy-mm-dd hh:mm" => DbfNumberFormat::Date,
            "hh:mm" | "[<0]yyyy-mm-dd;0" => DbfNumberFormat::UnsupportedDate,
            _ => DbfNumberFormat::Number,
        }
    }

    fn write_dbf(wb: &Workbook, sheet_index: usize) -> Result<Vec<u8>, IoError> {
        super::write_dbf(wb, sheet_index, classify)
    }

    fn column(name: &str, values: &[Value]) -> Workbook {
        let mut wb = Workbook::new();
        let sheet = wb.sheet_mut(0).unwrap();
        sheet.set_value(CellRef::new(0, 0), Value::Text(name.into()));
        for (row, value) in values.iter().enumerate() {
            sheet.set_value(CellRef::new(row as u32 + 1, 0), value.clone());
        }
        wb
    }

    #[test]
    fn dbf_export_roundtrips_typed_rectangles_and_writes_classic_layout() {
        let mut wb = Workbook::new();
        let date_style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("yyyy-mm-dd"));
        let sheet = wb.sheet_mut(0).unwrap();
        let date = serial_from_ymd(DateSystem::D1900, 2026, 10, 17).unwrap();
        for (col, (name, value)) in
            [("Name", Value::Text("€".into())), ("Amount", Value::Number(12.5)), ("Active", Value::Bool(true)), ("Date", Value::Number(date))]
                .into_iter()
                .enumerate()
        {
            sheet.set_value(CellRef::new(2, col as u32 + 2), Value::Text(name.into()));
            sheet.set_value(CellRef::new(3, col as u32 + 2), value);
        }
        sheet.set_style(CellRef::new(3, 5), date_style);
        let bytes = write_dbf(&wb, 0).unwrap();
        assert_eq!(bytes[0], 3);
        assert_eq!(&bytes[4..8], &1u32.to_le_bytes());
        assert_eq!(&bytes[8..10], &161u16.to_le_bytes());
        assert_eq!(&bytes[10..12], &15u16.to_le_bytes());
        assert_eq!(bytes[29], 3);
        assert_eq!([bytes[43], bytes[75], bytes[107], bytes[139]], *b"CNLD");
        assert_eq!(&bytes[161..], b" \x8012.5T20261017\x1a");
        let (loaded, report) = read_dbf(&bytes).unwrap();
        assert!(report.warnings.is_empty());
        let sheet = loaded.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Text("€".into()));
        assert_eq!(sheet.value(CellRef::new(1, 1)), Value::Number(12.5));
        assert_eq!(sheet.value(CellRef::new(1, 2)), Value::Bool(true));
        assert_eq!(sheet.value(CellRef::new(1, 3)), Value::Number(date));
    }

    #[test]
    fn dbf_export_uses_shared_numeric_scale_without_rounding() {
        let wb = column("Amount", &[Value::Number(1.0), Value::Empty, Value::Number(-12.25)]);
        let bytes = write_dbf(&wb, 0).unwrap();
        assert_eq!(&bytes[48..50], &[6, 2]);
        assert_eq!(&bytes[65..], b"      1        -12.25\x1a");
        let (loaded, _) = read_dbf(&bytes).unwrap();
        let sheet = loaded.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Number(1.0));
        assert_eq!(sheet.value(CellRef::new(2, 0)), Value::Empty);
        assert_eq!(sheet.value(CellRef::new(3, 0)), Value::Number(-12.25));
    }

    #[test]
    fn dbf_export_checks_headers_instead_of_renaming_or_truncating() {
        for name in ["", "TooLongName1", "1Name", "Two words", "Náme", "A\0"] {
            assert!(write_dbf(&column(name, &[Value::Number(1.0)]), 0).is_err(), "{name}");
        }
        let mut wb = column("Name", &[Value::Number(1.0)]);
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 1), Value::Text("NAME".into()));
        assert!(write_dbf(&wb, 0).is_err());
        assert!(write_dbf(&Workbook::new(), 0).is_err());
        assert!(write_dbf(&wb, 99).is_err());
    }

    #[test]
    fn dbf_export_rejects_mixed_types_and_unrepresentable_values() {
        assert!(write_dbf(&column("Mixed", &[Value::Text("1".into()), Value::Number(2.0)]), 0).is_err());
        for text in ["🐈".to_string(), "x".repeat(255), "trailing ".to_string(), "a\0b".to_string()] {
            assert!(write_dbf(&column("Text", &[Value::Text(text.into())]), 0).is_err());
        }
        for number in [f64::NAN, f64::INFINITY, 1e21, 1e-16, 1.0 / 3.0] {
            assert!(write_dbf(&column("Number", &[Value::Number(number)]), 0).is_err(), "{number}");
        }
        assert!(write_dbf(&column("Error", &[Value::Error(gridcraft_core::CellError::Value)]), 0).is_err());
    }

    #[test]
    fn dbf_export_keeps_literal_formula_text_and_full_field_width() {
        let wb = column("Text", &[Value::Text("=1+1".into()), Value::Text("x".repeat(254).into())]);
        let (loaded, _) = read_dbf(&write_dbf(&wb, 0).unwrap()).unwrap();
        let sheet = loaded.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Text("=1+1".into()));
        assert!(sheet.cells.get(CellRef::new(1, 0)).unwrap().formula.is_none());
        assert_eq!(sheet.value(CellRef::new(2, 0)), Value::Text("x".repeat(254).into()));
    }

    #[test]
    fn dbf_export_converts_1904_dates_and_rejects_times_and_fake_dates() {
        let mut wb = column("Date", &[Value::Number(0.0)]);
        wb.date_system = DateSystem::D1904;
        let date_style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("yyyy-mm-dd"));
        wb.sheet_mut(0).unwrap().set_style(CellRef::new(1, 0), date_style);
        let bytes = write_dbf(&wb, 0).unwrap();
        assert_eq!(&bytes[65..], b" 19040101\x1a");
        for (system, value, format) in [
            (DateSystem::D1900, 60.0, "yyyy-mm-dd"),
            (DateSystem::D1904, 0.5, "yyyy-mm-dd hh:mm"),
            (DateSystem::D1900, 0.0, "hh:mm"),
            (DateSystem::D1900, 25.0, "[<0]yyyy-mm-dd;0"),
        ] {
            wb.date_system = system;
            let style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(format));
            let sheet = wb.sheet_mut(0).unwrap();
            sheet.set_value(CellRef::new(1, 0), Value::Number(value));
            sheet.set_style(CellRef::new(1, 0), style);
            assert!(write_dbf(&wb, 0).is_err());
        }
    }

    #[test]
    fn dbf_export_rejects_large_sparse_ranges_and_wide_records() {
        let mut wb = column("Text", &[Value::Text("x".repeat(254).into())]);
        for col in 1..16 {
            let sheet = wb.sheet_mut(0).unwrap();
            sheet.set_value(CellRef::new(0, col), Value::Text(format!("Field{col}").into()));
            sheet.set_value(CellRef::new(1, col), Value::Text("x".repeat(254).into()));
        }
        assert!(write_dbf(&wb, 0).unwrap_err().to_string().contains("4,000"));
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 255), Value::Text("Last".into()));
        assert!(write_dbf(&wb, 0).unwrap_err().to_string().contains("255"));
        let mut wb = column("Text", &[]);
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(gridcraft_core::MAX_ROWS - 1, 5), Value::Number(1.0));
        assert!(matches!(write_dbf(&wb, 0), Err(IoError::TooLarge(_))));
        for pos in [CellRef::new(u32::MAX, 0), CellRef::new(0, u32::MAX)] {
            let mut wb = column("Header", &[]);
            wb.sheet_mut(0).unwrap().set_value(pos, Value::Text("Header".into()));
            assert!(write_dbf(&wb, 0).unwrap_err().to_string().contains("outside"));
        }
    }

    #[test]
    fn dbf_export_handles_header_only_and_all_blank_columns() {
        let mut wb = column("Blank", &[]);
        let (loaded, _) = read_dbf(&write_dbf(&wb, 0).unwrap()).unwrap();
        assert_eq!(loaded.sheet(0).unwrap().cells.len(), 1);
        let sheet = wb.sheet_mut(0).unwrap();
        sheet.set_value(CellRef::new(0, 1), Value::Text("Flag".into()));
        sheet.set_value(CellRef::new(2, 1), Value::Bool(false));
        let bytes = write_dbf(&wb, 0).unwrap();
        assert_eq!(bytes[43], b'C');
        assert_eq!(bytes[48], 1);
        let (loaded, _) = read_dbf(&bytes).unwrap();
        assert_eq!(loaded.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Empty);
        assert_eq!(loaded.sheet(0).unwrap().value(CellRef::new(2, 1)), Value::Bool(false));
    }

    #[test]
    fn dbf_export_bounds_reopened_utf8_text_not_encoded_byte_count() {
        // Each euro is one CP1252 byte but three decoded UTF-8 bytes.
        let wb = column("Text", &[Value::Text("€".into()), Value::Text("€".into())]);
        assert!(matches!(write_limited(&wb, 0, 5, classify), Err(IoError::TooLarge(_))));
        assert!(write_limited(&wb, 0, 6, classify).is_ok());
        let wb = column("Id", &[Value::Number(9007199254740992.0)]);
        // The importer keeps this 16-digit identifier as text rather than rounding it.
        assert!(matches!(write_limited(&wb, 0, 15, classify), Err(IoError::TooLarge(_))));
        assert!(write_limited(&wb, 0, 16, classify).is_ok());
    }
}
