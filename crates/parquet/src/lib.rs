//! Flat Parquet tables: row one contains names, subsequent rows contain typed values.
//! No Arrow runtime, database, Python, or system service is required.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use bytes::Bytes;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};
use parquet::{
    basic::{ConvertedType, LogicalType, Repetition, TimeUnit, Type as Physical},
    data_type::{BoolType, ByteArray, ByteArrayType, DoubleType},
    errors::ParquetError,
    file::{reader::FileReader, serialized_reader::SerializedFileReader, writer::SerializedFileWriter},
    record::Field,
    schema::types::Type,
};
use std::{collections::HashSet, sync::Arc};

mod limits;
#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Parquet(#[from] ParquetError),
}
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(message: &str) -> Error {
    Error::Format(message.into())
}

/// Bounds decoded worksheet expansion, independently of the compressed input file size.
const MAX_CELLS: u64 = 5_000_000;
const MAX_TEXT_BYTES: usize = 1 << 20;

/// Recognizes Parquet even when it has an unrelated extension.
pub fn is_parquet(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PAR1") || bytes.starts_with(b"PARE")
}

/// Import notices for conversions that a worksheet cannot represent losslessly.
#[derive(Debug, Default)]
pub struct ReadReport {
    pub warnings: Vec<String>,
}

/// Imports a borrowed buffer. Call [`read_owned`] when the input is already owned to avoid a copy.
pub fn read(bytes: &[u8]) -> Result<(Workbook, ReadReport)> {
    let footer_start = limits::footer(bytes)?;
    read_buffer(Bytes::copy_from_slice(bytes), footer_start)
}

/// Imports an owned buffer, transferring its allocation to the decoder without copying its payload.
pub fn read_owned(bytes: Vec<u8>) -> Result<(Workbook, ReadReport)> {
    let footer_start = limits::footer(&bytes)?;
    read_buffer(Bytes::from(bytes), footer_start)
}

fn read_buffer(bytes: Bytes, footer_start: usize) -> Result<(Workbook, ReadReport)> {
    let reader = SerializedFileReader::new(bytes.clone())?;
    let metadata = reader.metadata();
    let info = metadata.file_metadata();
    let fields = info.schema().get_fields();
    let cols = fields.len();
    let rows = u64::try_from(info.num_rows()).map_err(|_| invalid("Negative Parquet row count"))?;
    if cols == 0 || cols > MAX_COLS as usize || rows >= u64::from(MAX_ROWS) {
        return Err(invalid("Parquet table exceeds worksheet dimensions (1,048,575 data rows plus a header, 16,384 columns)"));
    }
    if (rows + 1).saturating_mul(cols as u64) > MAX_CELLS {
        return Err(invalid("Parquet table exceeds the 5,000,000-cell import budget; select a smaller table first"));
    }
    if fields.iter().any(|f| !f.is_primitive() || !f.get_basic_info().has_repetition() || f.get_basic_info().repetition() == Repetition::REPEATED) {
        return Err(invalid("Nested, list and repeated Parquet fields are not supported; flatten the table first"));
    }
    for field in fields {
        if field.name().len() > MAX_TEXT_BYTES {
            return Err(invalid("A Parquet field name exceeds the 1 MiB text budget"));
        }
        if let Some(logical) = field.get_basic_info().logical_type_ref() {
            match logical {
                LogicalType::Time(t) | LogicalType::Timestamp(t) if t.unit == TimeUnit::NANOS => {
                    return Err(invalid("Nanosecond Parquet time/timestamps are not supported; convert to microseconds first"));
                }
                LogicalType::String
                | LogicalType::Enum
                | LogicalType::Decimal(_)
                | LogicalType::Date
                | LogicalType::Time(_)
                | LogicalType::Timestamp(_)
                | LogicalType::Integer(_)
                | LogicalType::Json
                | LogicalType::Float16 => {}
                _ => return Err(invalid("This Parquet logical field type is not supported")),
            }
        }
    }
    // Check every advertised page allocation before the decoder sees it.
    limits::pages(&bytes, metadata, footer_start)?;
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Sheet1");
    for (col, field) in fields.iter().enumerate() {
        set(&mut sheet, 0, col as u32, Value::Text(field.name().into()), StyleId::default());
    }
    let date = wb.styles.intern(gridcraft_model::Style { num_fmt: NumFmt("yyyy-mm-dd".into()), ..Default::default() });
    let time = wb.styles.intern(gridcraft_model::Style { num_fmt: NumFmt("hh:mm:ss.000".into()), ..Default::default() });
    let timestamp = wb.styles.intern(gridcraft_model::Style { num_fmt: NumFmt("yyyy-mm-dd hh:mm:ss.000".into()), ..Default::default() });
    let mut count = 0u64;
    let mut exact_text = false;
    for row in reader.get_row_iter(None)? {
        if count >= rows {
            return Err(invalid("Parquet contains more rows than its metadata declares"));
        }
        let row = row?;
        for (col, (_, field)) in row.into_columns().into_iter().enumerate() {
            let style = match field {
                Field::Date(_) => date,
                Field::TimeMillis(_) | Field::TimeMicros(_) => time,
                Field::TimestampMillis(_) | Field::TimestampMicros(_) => timestamp,
                _ => StyleId::default(),
            };
            exact_text |= matches!(&field, Field::Decimal(_))
                || matches!(&field, Field::Long(v) if v.unsigned_abs() > (1u64 << 53))
                || matches!(&field, Field::ULong(v) if *v > (1u64 << 53));
            let value = value(field)?;
            if value != Value::Empty {
                set(&mut sheet, count as u32 + 1, col as u32, value, style);
            }
        }
        count += 1;
    }
    if count != rows {
        return Err(invalid("Parquet row count does not match its metadata"));
    }
    let mut report = ReadReport::default();
    let retained_rows = sheet.used_range().map_or(0, |r| u64::from(r.end.row));
    if retained_rows < rows {
        report
            .warnings
            .push(format!("{} trailing all-null Parquet rows have no worksheet cells and are omitted from the used range.", rows - retained_rows));
    }
    if exact_text {
        report
            .warnings
            .push("Parquet decimals and integers beyond exact spreadsheet numeric precision were imported as text to preserve their digits.".into());
    }
    wb.sheets = vec![Arc::new(sheet)];
    Ok((wb, report))
}
fn set(sheet: &mut Sheet, row: u32, col: u32, value: Value, style: StyleId) {
    let mut cell = Cell::value(value);
    cell.style = style;
    sheet.set_cell(CellRef::new(row, col), cell);
}
fn value(field: Field) -> Result<Value> {
    let number = match field {
        Field::Null => return Ok(Value::Empty),
        Field::Bool(v) => return Ok(Value::Bool(v)),
        Field::Str(s) => return text(s),
        Field::Byte(v) => f64::from(v),
        Field::Short(v) => f64::from(v),
        Field::Int(v) => f64::from(v),
        Field::UByte(v) => f64::from(v),
        Field::UShort(v) => f64::from(v),
        Field::UInt(v) => f64::from(v),
        Field::Long(v) => {
            if v.unsigned_abs() > (1u64 << 53) {
                return text(v.to_string());
            }
            v as f64
        }
        Field::ULong(v) => {
            if v > (1u64 << 53) {
                return text(v.to_string());
            }
            v as f64
        }
        Field::Float16(v) => v.to_f64(),
        Field::Float(v) => f64::from(v),
        Field::Double(v) => v,
        Field::Date(v) => gridcraft_core::date::serial_from_unix_ms(f64::from(v) * 86_400_000.0),
        Field::TimeMillis(v) => f64::from(v) / 86_400_000.0,
        Field::TimeMicros(v) => v as f64 / 86_400_000_000.0,
        Field::TimestampMillis(v) => gridcraft_core::date::serial_from_unix_ms(v as f64),
        Field::TimestampMicros(v) => gridcraft_core::date::serial_from_unix_ms(v as f64 / 1000.0),
        // Keep exact decimal text rather than silently rounding arbitrary precision values.
        Field::Decimal(_) => return text(field.to_string()),
        _ => return Err(invalid("Binary and nested Parquet values are not supported")),
    };
    if !number.is_finite() {
        return Err(invalid("Non-finite Parquet numbers cannot be represented as spreadsheet numbers"));
    }
    Ok(Value::Number(number))
}
fn text(s: String) -> Result<Value> {
    if s.len() > MAX_TEXT_BYTES {
        return Err(invalid("A Parquet text cell exceeds the 1 MiB text-cell budget"));
    }
    Ok(Value::Text(s.into()))
}

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Number,
    Bool,
}
/// Writes the active sheet's used rectangle, with its first row as unique text column names.
/// Cached formula results are exported; workbook formatting and formulas are not a Parquet table.
pub fn write(sheet: &Sheet) -> Result<Vec<u8>> {
    let range = sheet.used_range().ok_or_else(|| invalid("Add a header row and data before exporting Parquet"))?;
    if range.end.row >= MAX_ROWS || range.end.col >= MAX_COLS {
        return Err(invalid("Parquet export range exceeds worksheet dimensions"));
    }
    let cols = range.end.col - range.start.col + 1;
    let rows = range.end.row - range.start.row;
    if u64::from(cols) * (u64::from(rows) + 1) > MAX_CELLS {
        return Err(invalid("Parquet export exceeds the 5,000,000-cell budget"));
    }
    let mut names = HashSet::new();
    let mut kinds = Vec::new();
    let mut fields = Vec::new();
    let mut text_bytes = 0usize;
    for col in range.start.col..=range.end.col {
        let Value::Text(name) = sheet.value(CellRef::new(range.start.row, col)) else {
            return Err(invalid("Parquet export needs a text header for every column"));
        };
        if name.len() > MAX_TEXT_BYTES {
            return Err(invalid("A Parquet field name exceeds the 1 MiB text budget"));
        }
        if name.is_empty() || !names.insert(name.to_string()) {
            return Err(invalid("Parquet column headers must be nonempty and unique"));
        }
        let mut kind = None;
        for row in range.start.row + 1..=range.end.row {
            let next = match sheet.value(CellRef::new(row, col)) {
                Value::Empty => continue,
                Value::Text(s) => {
                    if s.len() > MAX_TEXT_BYTES {
                        return Err(invalid("A text cell exceeds the 1 MiB export budget"));
                    }
                    text_bytes = text_bytes.saturating_add(s.len());
                    Kind::Text
                }
                Value::Bool(_) => Kind::Bool,
                Value::Number(v) if v.is_finite() => Kind::Number,
                _ => return Err(invalid("Resolve cell errors, arrays and non-finite numbers before exporting Parquet")),
            };
            if kind.is_some_and(|k| std::mem::discriminant(&k) != std::mem::discriminant(&next)) {
                return Err(invalid("Parquet columns must have a consistent type (text, number or Boolean); blanks are allowed"));
            }
            kind = Some(next);
        }
        if text_bytes > limits::MAX_DECODED {
            return Err(invalid("Parquet export exceeds the 512 MiB text budget"));
        }
        let kind = kind.unwrap_or(Kind::Text);
        let physical = match kind {
            Kind::Text => Physical::BYTE_ARRAY,
            Kind::Number => Physical::DOUBLE,
            Kind::Bool => Physical::BOOLEAN,
        };
        let mut builder = Type::primitive_type_builder(&name, physical).with_repetition(Repetition::OPTIONAL);
        if matches!(kind, Kind::Text) {
            builder = builder.with_converted_type(ConvertedType::UTF8);
        }
        fields.push(Arc::new(builder.build()?));
        kinds.push(kind);
    }
    let schema = Arc::new(Type::group_type_builder("gridcraft").with_fields(fields).build()?);
    let properties = parquet::file::properties::WriterProperties::builder().set_compression(parquet::basic::Compression::SNAPPY).build();
    let mut writer = SerializedFileWriter::new(Vec::new(), schema, Arc::new(properties))?;
    // Bound temporary vectors and dictionary state; never collect an entire column into memory.
    for first in (range.start.row + 1..=range.end.row).step_by(4096) {
        let last = first.saturating_add(4095).min(range.end.row);
        let mut group = writer.next_row_group()?;
        for (offset, kind) in kinds.iter().enumerate() {
            let mut column = group.next_column()?.ok_or_else(|| invalid("Missing Parquet writer column"))?;
            let col = range.start.col + offset as u32;
            let values: Vec<Value> = (first..=last).map(|row| sheet.value(CellRef::new(row, col))).collect();
            let definitions: Vec<i16> = values.iter().map(|v| i16::from(*v != Value::Empty)).collect();
            match kind {
                Kind::Text => {
                    let data: Vec<ByteArray> =
                        values.iter().filter_map(|v| if let Value::Text(s) = v { Some(ByteArray::from(s.as_ref())) } else { None }).collect();
                    column.typed::<ByteArrayType>().write_batch(&data, Some(&definitions), None)?;
                }
                Kind::Number => {
                    let data: Vec<f64> = values.iter().filter_map(|v| if let Value::Number(n) = v { Some(*n) } else { None }).collect();
                    column.typed::<DoubleType>().write_batch(&data, Some(&definitions), None)?;
                }
                Kind::Bool => {
                    let data: Vec<bool> = values.iter().filter_map(|v| if let Value::Bool(b) = v { Some(*b) } else { None }).collect();
                    column.typed::<BoolType>().write_batch(&data, Some(&definitions), None)?;
                }
            }
            column.close()?;
        }
        group.close()?;
    }
    Ok(writer.into_inner()?)
}
