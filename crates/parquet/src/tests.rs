#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
fn read(bytes: &[u8]) -> Result<Workbook> {
    let (wb, report) = super::read(bytes)?;
    let (owned, owned_report) = super::read_owned(bytes.to_vec())?;
    assert_eq!(wb, owned);
    assert_eq!(report.warnings, owned_report.warnings);
    Ok(wb)
}
use parquet::{data_type::Int64Type, file::properties::WriterProperties, schema::parser::parse_message_type};

fn sheet() -> Sheet {
    let mut s = Sheet::new("Data");
    for (col, name) in ["Name", "Amount", "Active"].iter().enumerate() {
        s.set_value(CellRef::new(0, col as u32), Value::Text((*name).into()));
    }
    s
}
fn roundtrip(s: &Sheet) -> Workbook {
    read(&write(s).unwrap()).unwrap()
}

#[test]
fn typed_values_nulls_empty_strings_and_formula_like_text_roundtrip() {
    let mut s = sheet();
    s.set_value(CellRef::new(1, 0), Value::Text("=SUM(1,2)".into()));
    s.set_value(CellRef::new(2, 0), Value::Text("".into()));
    s.set_value(CellRef::new(3, 0), Value::Text("Unicode: café 日本語".into()));
    s.set_value(CellRef::new(1, 1), Value::Number(-12.5));
    s.set_value(CellRef::new(3, 1), Value::Number(0.0));
    s.set_value(CellRef::new(2, 2), Value::Bool(false));
    s.set_value(CellRef::new(3, 2), Value::Bool(true));
    let wb = roundtrip(&s);
    let got = wb.sheet(0).unwrap();
    for row in 0..4 {
        for col in 0..3 {
            assert_eq!(got.value(CellRef::new(row, col)), s.value(CellRef::new(row, col)));
        }
    }
    assert!(got.cell(CellRef::new(1, 0)).unwrap().formula.is_none());
}

#[test]
fn multiple_row_groups_and_header_only_table() {
    let mut s = sheet();
    assert_eq!(roundtrip(&s).sheet(0).unwrap().cells.len(), 3);
    for row in 1..=8200 {
        s.set_value(CellRef::new(row, 1), Value::Number(f64::from(row)));
    }
    let bytes = write(&s).unwrap();
    let reader = SerializedFileReader::new(Bytes::from(bytes.clone())).unwrap();
    assert_eq!(reader.num_row_groups(), 3);
    let wb = read(&bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(8200, 1)), Value::Number(8200.0));
}

fn integers(converted: ConvertedType, values: &[i64], compression: parquet::basic::Compression) -> Vec<u8> {
    let field =
        Type::primitive_type_builder("value", Physical::INT64).with_repetition(Repetition::REQUIRED).with_converted_type(converted).build().unwrap();
    let schema = Arc::new(Type::group_type_builder("test").with_fields(vec![Arc::new(field)]).build().unwrap());
    let properties = Arc::new(WriterProperties::builder().set_compression(compression).build());
    let mut writer = SerializedFileWriter::new(Vec::new(), schema, properties).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<Int64Type>().write_batch(values, None, None).unwrap();
    col.close().unwrap();
    group.close().unwrap();
    writer.into_inner().unwrap()
}

#[test]
fn large_integers_are_exact_text_not_rounded() {
    let bytes = integers(ConvertedType::INT_64, &[9_007_199_254_740_992, 9_007_199_254_740_993, i64::MIN], parquet::basic::Compression::SNAPPY);
    let wb = read(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(CellRef::new(1, 0)), Value::Number(9_007_199_254_740_992.0));
    assert_eq!(s.value(CellRef::new(2, 0)), Value::Text("9007199254740993".into()));
    assert_eq!(s.value(CellRef::new(3, 0)), Value::Text(i64::MIN.to_string().into()));
}

#[test]
fn common_compression_codecs() {
    use parquet::basic::Compression as C;
    let codecs = [C::UNCOMPRESSED, C::SNAPPY, C::GZIP(Default::default()), C::BROTLI(Default::default()), C::LZ4_RAW];
    for codec in codecs {
        let bytes = integers(ConvertedType::INT_64, &[5, 10], codec);
        assert!(read(&bytes).is_ok(), "{codec:?}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    assert!(read(&integers(ConvertedType::INT_64, &[5], C::ZSTD(Default::default()))).is_ok());
}

#[test]
fn timestamp_and_date_values_get_number_formats() {
    let wb = read(&integers(ConvertedType::TIMESTAMP_MICROS, &[0, 1_000_000], parquet::basic::Compression::SNAPPY)).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(CellRef::new(1, 0)), Value::Number(25569.0));
    assert!(wb.styles.get(s.style_id(CellRef::new(1, 0))).num_fmt.as_str().contains("yyyy"));
    assert_eq!(value(Field::Date(0)).unwrap(), Value::Number(25569.0));
    assert_eq!(value(Field::TimeMillis(43_200_000)).unwrap(), Value::Number(0.5));
}

#[test]
fn export_refuses_mixed_types_bad_headers_and_errors() {
    let mut s = sheet();
    s.set_value(CellRef::new(1, 1), Value::Number(1.0));
    s.set_value(CellRef::new(2, 1), Value::Text("2".into()));
    assert!(write(&s).unwrap_err().to_string().contains("consistent type"));
    s.remove_cell(CellRef::new(2, 1));
    s.set_value(CellRef::new(0, 1), Value::Text("Name".into()));
    assert!(write(&s).unwrap_err().to_string().contains("unique"));
    s.set_value(CellRef::new(0, 1), Value::Number(5.0));
    assert!(write(&s).unwrap_err().to_string().contains("text header"));
    s.set_value(CellRef::new(0, 1), Value::Text("Amount".into()));
    s.set_value(CellRef::new(1, 1), Value::Error(gridcraft_core::CellError::Value));
    assert!(write(&s).unwrap_err().to_string().contains("cell errors"));
}

#[test]
fn empty_and_sparse_huge_sheets_fail_without_expansion() {
    assert!(write(&Sheet::new("Empty")).is_err());
    let mut s = sheet();
    s.set_value(CellRef::new(MAX_ROWS - 1, MAX_COLS - 1), Value::Bool(true));
    assert!(write(&s).unwrap_err().to_string().contains("budget"));
}

#[test]
fn truncated_and_random_bytes_never_panic() {
    let bytes = integers(ConvertedType::INT_64, &[1, 2], parquet::basic::Compression::SNAPPY);
    for end in 0..bytes.len() {
        assert!(read(&bytes[..end]).is_err());
        assert!(super::read_owned(bytes[..end].to_vec()).is_err());
    }
    let mut seed = 1u64;
    for len in 0..256 {
        let mut bytes = vec![0; len];
        for b in &mut bytes {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (seed >> 32) as u8;
        }
        assert!(read(&bytes).is_err());
    }
}

#[test]
fn nested_schema_is_rejected_before_decoder() {
    let schema = Arc::new(parse_message_type("message test { OPTIONAL group nested { REQUIRED INT64 n; } }").unwrap());
    let bytes = SerializedFileWriter::new(Vec::new(), schema, Default::default()).unwrap().into_inner().unwrap();
    assert!(read(&bytes).unwrap_err().to_string().contains("Nested"));
}

#[test]
fn text_and_nonfinite_limits_are_explicit() {
    assert!(value(Field::Double(f64::NAN)).is_err());
    assert!(value(Field::Str("x".repeat(MAX_TEXT_BYTES + 1))).is_err());
    let mut s = sheet();
    s.set_value(CellRef::new(1, 0), Value::Text("x".repeat(MAX_TEXT_BYTES + 1).into()));
    assert!(write(&s).is_err());
}

#[test]
fn mutated_real_envelopes_do_not_panic() {
    let original = integers(ConvertedType::INT_64, &[1, 2, 3], parquet::basic::Compression::SNAPPY);
    for at in 0..original.len() {
        let mut bytes = original.clone();
        bytes[at] ^= 0x80;
        let result = std::panic::catch_unwind(|| read(&bytes));
        assert!(result.is_ok(), "panic at mutated byte {at}");
        let owned = std::panic::catch_unwind(|| super::read_owned(bytes));
        assert!(owned.is_ok(), "owned panic at mutated byte {at}");
    }
}

#[test]
fn null_only_rows_have_an_explicit_import_notice() {
    let schema = Arc::new(parse_message_type("message test { OPTIONAL INT64 value; }").unwrap());
    let mut writer = SerializedFileWriter::new(Vec::new(), schema, Default::default()).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<Int64Type>().write_batch(&[], Some(&[0, 0]), None).unwrap();
    col.close().unwrap();
    group.close().unwrap();
    let bytes = writer.into_inner().unwrap();
    let (wb, report) = super::read(&bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().used_range().unwrap().a1(), "A1");
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("2 trailing"));
}

#[test]
fn invalid_model_coordinates_are_refused_before_arithmetic() {
    let mut s = Sheet::new("Invalid");
    s.set_value(CellRef::new(0, 0), Value::Text("Header".into()));
    s.set_value(CellRef::new(u32::MAX, u32::MAX), Value::Text("Header".into()));
    assert!(write(&s).unwrap_err().to_string().contains("worksheet dimensions"));
}
