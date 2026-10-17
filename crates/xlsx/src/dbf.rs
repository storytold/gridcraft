//! Data-only dBASE III/IV import. Layout reference: Visual FoxPro's published table-file
//! structure (the classic 32-byte descriptors), not dBASE 7's incompatible 48-byte layout.
//! https://www.vfphelp.com/help/html/465e7a94-51b7-4e0c-98f9-432864fe5bcc.htm
//! No filesystem access: memo/index companions are never followed.

use std::sync::Arc;

use gridcraft_core::date::{days_in_month, serial_from_ymd};
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, StyleId, Workbook};
use yore::CodePage;

use crate::{IoError, ReadReport};

const MAX_CELLS: u64 = 5_000_000;
const MAX_TEXT: usize = 64 << 20;

struct Field {
    name: String,
    kind: u8,
    width: usize,
}

fn invalid(detail: &str) -> IoError {
    IoError::Format(format!("DBF: {detail}"))
}

/// Imports classic dBASE III/IV tables as one sheet, with field names in row 1.
/// Deleted records are omitted. Memo/binary fields, encrypted files and newer dialects
/// are refused explicitly rather than importing pointers or misinterpreting their layout.
pub fn read_dbf(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let header = bytes.get(..32).ok_or_else(|| invalid("truncated header"))?;
    if !matches!(header.first(), Some(0x03 | 0x83 | 0x8b)) {
        return Err(invalid("unsupported version; only classic dBASE III/IV tables are supported"));
    }
    if header.get(14..16).is_none_or(|flags| flags.iter().any(|b| *b != 0)) {
        return Err(invalid("encrypted tables and incomplete transactions are not supported"));
    }
    let count = u32::from_le_bytes(header.get(4..8).and_then(|s| s.try_into().ok()).ok_or_else(|| invalid("record count"))?);
    let header_len = u16::from_le_bytes(header.get(8..10).and_then(|s| s.try_into().ok()).ok_or_else(|| invalid("header length"))?) as usize;
    let record_len = u16::from_le_bytes(header.get(10..12).and_then(|s| s.try_into().ok()).ok_or_else(|| invalid("record length"))?) as usize;
    let descriptor_len = header_len.checked_sub(33).ok_or_else(|| invalid("invalid header length"))?;
    let columns = descriptor_len / 32;
    if !descriptor_len.is_multiple_of(32) || columns == 0 || columns > MAX_COLS as usize || record_len < 2 {
        return Err(invalid("invalid field descriptors or record length"));
    }
    if u64::from(count).saturating_add(1).saturating_mul(columns as u64) > MAX_CELLS {
        return Err(IoError::TooLarge("DBF exceeds the 5 million field-value import budget".into()));
    }
    let end = (count as usize).checked_mul(record_len).and_then(|n| header_len.checked_add(n)).ok_or_else(|| invalid("record size overflow"))?;
    let records = bytes.get(header_len..end).ok_or_else(|| invalid("truncated records or header"))?;
    if bytes.get(header_len - 1) != Some(&0x0d) {
        return Err(invalid("missing field descriptor terminator"));
    }
    let mut report = ReadReport::default();
    let codepage = header.get(29).copied().unwrap_or_default();
    if codepage == 0 {
        report.warn("DBF has no code-page marker; text is interpreted as Windows-1252.");
    }
    let mut fields = Vec::with_capacity(columns);
    let mut width = 1usize;
    for descriptor in bytes.get(32..header_len - 1).ok_or_else(|| invalid("truncated descriptors"))?.as_chunks::<32>().0 {
        let name_bytes = descriptor.get(..11).ok_or_else(|| invalid("field name"))?;
        let name_end = name_bytes.iter().position(|b| *b == 0).unwrap_or(name_bytes.len());
        let name = decode(name_bytes.get(..name_end).ok_or_else(|| invalid("field name"))?, codepage)?;
        if name.is_empty() || fields.iter().any(|f: &Field| f.name == name) {
            return Err(invalid("empty or duplicate field name"));
        }
        let kind = descriptor.get(11).copied().ok_or_else(|| invalid("field type"))?;
        let size = descriptor.get(16).copied().unwrap_or_default() as usize;
        if kind == b'M' {
            return Err(invalid("memo fields need a companion .dbt file, which is not supported yet"));
        }
        if !matches!(kind, b'C' | b'N' | b'F' | b'L' | b'D') {
            return Err(invalid("unsupported field type; supported types are character, numeric, float, logical and date"));
        }
        if size == 0 || (kind == b'L' && size != 1) || (kind == b'D' && size != 8) {
            return Err(invalid("invalid field width"));
        }
        width += size; // At most 2046 descriptors, each at most 255 bytes.
        fields.push(Field { name, kind, width: size });
    }
    if width != record_len {
        return Err(invalid("field widths do not match the record length"));
    }
    // Validate record flags and output dimensions before constructing any cells.
    let mut active = 0u32;
    let mut deleted = false;
    for record in records.chunks_exact(record_len) {
        match record.first() {
            Some(b' ') => active += 1,
            Some(b'*') => deleted = true,
            _ => return Err(invalid("invalid record deletion flag")),
        }
    }
    if deleted {
        report.warn("Records marked as deleted in the DBF table were omitted.");
    }
    if active >= MAX_ROWS {
        return Err(IoError::TooLarge("DBF records plus the header exceed the worksheet row limit".into()));
    }
    let mut wb = Workbook::new();
    let date_style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("yyyy-mm-dd"));
    let sheet = wb.sheet_mut(0).ok_or_else(|| invalid("missing destination sheet"))?;
    for (column, field) in fields.iter().enumerate() {
        sheet.set_value(CellRef::new(0, column as u32), Value::Text(Arc::from(field.name.as_str())));
    }
    let mut text_bytes = 0usize;
    for (row, record) in (1..).zip(records.chunks_exact(record_len).filter(|r| r.first() == Some(&b' '))) {
        let mut offset = 1;
        for (column, field) in fields.iter().enumerate() {
            let raw = record.get(offset..offset + field.width).ok_or_else(|| invalid("truncated field"))?;
            offset += field.width;
            let (value, date) = value(raw, field.kind, codepage, &mut report)?;
            if let Value::Text(text) = &value {
                text_bytes = text_bytes.saturating_add(text.len());
                if text_bytes > MAX_TEXT {
                    return Err(IoError::TooLarge("DBF decoded text exceeds 64 MiB".into()));
                }
            }
            if value != Value::Empty {
                let cell = Cell { style: if date { date_style } else { StyleId::DEFAULT }, ..Cell::value(value) };
                sheet.cells.set(CellRef::new(row, column as u32), cell);
            }
        }
    }
    Ok((wb, report))
}

fn decode(raw: &[u8], codepage: u8) -> Result<String, IoError> {
    use yore::code_pages::*;
    let page: &dyn CodePage = match codepage {
        0x00 | 0x03 | 0x57 => &CP1252,
        0x01 => &CP437,
        0x02 => &CP850,
        0x64 => &CP852,
        0x65 => &CP866,
        0x66 => &CP865,
        0x67 => &CP861,
        0xc8 => &CP1250,
        0xc9 => &CP1251,
        0xca => &CP1254,
        0xcb => &CP1253,
        _ => return Err(invalid(&format!("unsupported code-page marker 0x{codepage:02x}"))),
    };
    page.decode(raw).map(|s| s.into_owned()).map_err(|_| invalid("text contains an invalid byte for its declared code page"))
}

fn value(raw: &[u8], kind: u8, codepage: u8, report: &mut ReadReport) -> Result<(Value, bool), IoError> {
    if kind == b'C' {
        let end = raw.iter().rposition(|b| *b != b' ').map_or(0, |i| i + 1);
        let text = decode(raw.get(..end).ok_or_else(|| invalid("character value"))?, codepage)?;
        return Ok((if text.is_empty() { Value::Empty } else { Value::Text(Arc::from(text)) }, false));
    }
    let text = std::str::from_utf8(raw).map_err(|_| invalid("non-ASCII typed value"))?.trim();
    if text.is_empty() || (kind == b'L' && text == "?") || (kind == b'D' && text == "00000000") {
        return Ok((Value::Empty, false));
    }
    match kind {
        b'N' | b'F' => {
            let number = text.parse::<f64>().ok().filter(|n| n.is_finite()).ok_or_else(|| invalid("invalid numeric value"))?;
            // Preserve high-precision identifiers/decimals rather than silently rounding them.
            let significant = text.trim_start_matches(['+', '-', '0', '.']).chars().filter(char::is_ascii_digit).count();
            if significant <= 15 {
                return Ok((Value::Number(number), false));
            }
            report.warn("DBF numbers with more than 15 significant digits were preserved as text to avoid precision loss.");
        }
        b'L' => match text {
            "T" | "t" | "Y" | "y" => return Ok((Value::Bool(true), false)),
            "F" | "f" | "N" | "n" => return Ok((Value::Bool(false), false)),
            _ => return Err(invalid("invalid logical value")),
        },
        b'D' => {
            if text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()) {
                let year = text.get(..4).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                let month = text.get(4..6).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                let day = text.get(6..).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                if (1900..=9999).contains(&year)
                    && (1..=12).contains(&month)
                    && day >= 1
                    && day <= days_in_month(year, month)
                    && let Some(serial) = serial_from_ymd(DateSystem::D1900, year.into(), month.into(), day.into())
                {
                    return Ok((Value::Number(serial), true));
                }
            }
            report.warn("DBF dates outside the supported calendar range or with invalid components were preserved as text.");
        }
        _ => return Err(invalid("unsupported field type")),
    }
    Ok((Value::Text(Arc::from(text)), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original byte-level fixtures: never generated by Excel or copied from a corpus.
    fn table(fields: &[(&str, u8, u8)], rows: &[&[u8]]) -> Vec<u8> {
        let header_len = 33 + fields.len() * 32;
        let record_len = 1 + fields.iter().map(|(_, _, width)| *width as usize).sum::<usize>();
        let mut bytes = vec![0; header_len];
        bytes[0] = 3;
        bytes[4..8].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        bytes[8..10].copy_from_slice(&(header_len as u16).to_le_bytes());
        bytes[10..12].copy_from_slice(&(record_len as u16).to_le_bytes());
        bytes[29] = 3;
        for (i, (name, kind, width)) in fields.iter().enumerate() {
            let start = 32 + i * 32;
            bytes[start..start + name.len()].copy_from_slice(name.as_bytes());
            bytes[start + 11] = *kind;
            bytes[start + 16] = *width;
        }
        bytes[header_len - 1] = 0x0d;
        for row in rows {
            assert_eq!(row.len(), record_len);
            bytes.extend_from_slice(row);
        }
        bytes.push(0x1a);
        bytes
    }

    #[test]
    fn typed_fields_preserve_schema_order_and_literal_text() {
        let bytes = table(&[("Text", b'C', 8), ("Amount", b'N', 6), ("Enabled", b'L', 1), ("Date", b'D', 8)], &[b" =1+1     12.50T20261017"]);
        let (wb, report) = read_dbf(&bytes).unwrap();
        assert!(report.warnings.is_empty());
        let sheet = wb.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(0, 1)), Value::Text("Amount".into()));
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Text("=1+1".into()));
        assert!(sheet.cells.get(CellRef::new(1, 0)).unwrap().formula.is_none());
        assert_eq!(sheet.value(CellRef::new(1, 1)), Value::Number(12.5));
        assert_eq!(sheet.value(CellRef::new(1, 2)), Value::Bool(true));
        assert_eq!(sheet.value(CellRef::new(1, 3)), Value::Number(serial_from_ymd(DateSystem::D1900, 2026, 10, 17).unwrap()));
        assert_eq!(wb.styles.get(sheet.style_id(CellRef::new(1, 3))).num_fmt.as_str(), "yyyy-mm-dd");
    }

    #[test]
    fn deleted_and_blank_records_do_not_turn_into_values() {
        let bytes = table(&[("N", b'N', 3), ("L", b'L', 1), ("D", b'D', 8)], &[b"* 99T20261017", b"    ?00000000", b"   7F20261017"]);
        let (wb, report) = read_dbf(&bytes).unwrap();
        let sheet = wb.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Empty);
        assert_eq!(sheet.value(CellRef::new(2, 0)), Value::Number(7.0));
        assert_eq!(sheet.value(CellRef::new(2, 1)), Value::Bool(false));
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn decodes_oem_and_windows_text_without_guessing_utf8() {
        for (marker, raw, expected) in [(1, 0x82, "é"), (2, 0x9c, "£"), (3, 0x80, "€")] {
            let mut bytes = table(&[("Text", b'C', 1)], &[&[b' ', raw]]);
            bytes[29] = marker;
            let (wb, report) = read_dbf(&bytes).unwrap();
            assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text(expected.into()));
            assert!(report.warnings.is_empty());
        }
        let mut bytes = table(&[("Text", b'C', 2)], &[b" \xc3\xa9"]);
        let (wb, _) = read_dbf(&bytes).unwrap();
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text("Ã©".into()));
        bytes[29] = 0;
        assert!(read_dbf(&bytes).unwrap().1.warnings[0].contains("Windows-1252"));
        bytes[29] = 0xff;
        assert!(read_dbf(&bytes).is_err());
    }

    #[test]
    fn keeps_large_identifiers_and_unrepresentable_dates_as_text() {
        let bytes = table(&[("Id", b'N', 18), ("Date", b'D', 8)], &[b" 9007199254740993  18991231", b"                 1220260230"]);
        let (wb, report) = read_dbf(&bytes).unwrap();
        let sheet = wb.sheet(0).unwrap();
        assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Text("9007199254740993".into()));
        assert_eq!(sheet.value(CellRef::new(1, 1)), Value::Text("18991231".into()));
        assert_eq!(sheet.value(CellRef::new(2, 0)), Value::Number(12.0));
        assert_eq!(sheet.value(CellRef::new(2, 1)), Value::Text("20260230".into()));
        assert_eq!(report.warnings.len(), 2);
    }

    #[test]
    fn does_not_silently_drop_memos_or_guess_newer_dialects() {
        let bytes = table(&[("Memo", b'M', 10)], &[b"          1"]);
        assert!(read_dbf(&bytes).unwrap_err().to_string().contains("companion .dbt"));
        for version in [2, 4, 0x30, 0xf5] {
            let mut bytes = table(&[("Text", b'C', 1)], &[b" x"]);
            bytes[0] = version;
            assert!(read_dbf(&bytes).is_err());
        }
        for version in [3, 0x83, 0x8b] {
            let mut bytes = table(&[("Text", b'C', 1)], &[b" x"]);
            bytes[0] = version;
            assert!(read_dbf(&bytes).is_ok());
        }
    }

    #[test]
    fn rejects_malformed_headers_records_and_values() {
        let good = table(&[("N", b'N', 3)], &[b"  12"]);
        for end in 0..good.len() - 1 {
            assert!(read_dbf(&good[..end]).is_err(), "accepted truncation at {end}");
        }
        for (offset, byte) in [(14, 1), (15, 1), (8, 1), (10, 0), (48, 0), (64, 0), (65, b'!')] {
            let mut bytes = good.clone();
            bytes[offset] = byte;
            assert!(read_dbf(&bytes).is_err(), "accepted invalid byte at {offset}");
        }
        for raw in [b" NaN", b" inf", b" abc"] {
            assert!(read_dbf(&table(&[("N", b'N', 3)], &[raw])).is_err());
        }
        assert!(read_dbf(&table(&[("L", b'L', 2)], &[b" TT"])).is_err());
        assert!(read_dbf(&table(&[("D", b'D', 7)], &[b" 2026101"])).is_err());
        assert!(read_dbf(&table(&[("N", b'N', 1), ("N", b'N', 1)], &[b" 12"])).is_err());
    }

    #[test]
    fn refuses_declared_work_before_allocating_and_allows_empty_tables() {
        let mut bytes = table(&[("Text", b'C', 1)], &[]);
        let (wb, _) = read_dbf(&bytes).unwrap();
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Text("Text".into()));
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(read_dbf(&bytes), Err(IoError::TooLarge(_))));
    }
}
