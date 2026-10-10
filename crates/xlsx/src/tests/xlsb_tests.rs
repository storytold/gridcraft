//! Original BIFF12 records and ZIP packages assembled in Rust from the public format schema.
use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::Visibility;

use super::make_zip;
use crate::{Format, read_xlsb, read_xlsx, sniff, write_xlsx};

const ROOT: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="records/book.bin"/></Relationships>"#;
const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="one" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="../data/one.bin"/><Relationship Id="two" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="../data/two.bin"/><Relationship Id="strings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="../text/strings.bin"/></Relationships>"#;
const TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/records/book.bin" ContentType="application/vnd.ms-excel.sheet.binary.macroEnabled.main"/><Override PartName="/data/one.bin" ContentType="application/vnd.ms-excel.worksheet"/><Override PartName="/data/two.bin" ContentType="application/vnd.ms-excel.worksheet"/><Override PartName="/text/strings.bin" ContentType="application/vnd.ms-excel.sharedStrings"/></Types>"#;

fn record(id: u16, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    if id < 128 {
        out.push(id as u8);
    } else {
        out.extend([(id as u8 & 0x7f) | 0x80, (id >> 7) as u8]);
    }
    let mut len = bytes.len();
    loop {
        let byte = (len & 0x7f) as u8;
        len >>= 7;
        out.push(if len == 0 { byte } else { byte | 0x80 });
        if len == 0 {
            break;
        }
    }
    out.extend(bytes);
    out
}
fn wide(text: &str) -> Vec<u8> {
    let units: Vec<_> = text.encode_utf16().collect();
    let mut out = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        out.extend(unit.to_le_bytes());
    }
    out
}
fn rich(text: &str, extras: bool) -> Vec<u8> {
    let mut out = vec![if extras { 3 } else { 0 }];
    out.extend(wide(text));
    if extras {
        out.extend(1u32.to_le_bytes());
        out.extend([0, 0, 0, 0]); // One original formatting run.
        out.extend(wide("sound"));
        out.extend(1u32.to_le_bytes());
        out.extend([0, 0, 0, 0, 1, 0, 0, 0, 0, 0]); // One ten-byte phonetic run.
    }
    out
}
fn row(r: u32) -> Vec<u8> {
    let mut out = r.to_le_bytes().to_vec();
    out.extend([0; 13]);
    record(0, &out)
}
fn cell(id: u16, col: u32, value: &[u8]) -> Vec<u8> {
    let mut data = col.to_le_bytes().to_vec();
    data.extend([0; 4]);
    data.extend(value);
    if (8..=11).contains(&id) {
        data.extend([0; 2]); // Formula flags.
        data.extend(3u32.to_le_bytes());
        data.extend([0x1e, 1, 0]); // Original PtgInt(1); ignored because we import the cache.
        data.extend(0u32.to_le_bytes());
    }
    record(id, &data)
}
fn worksheet(data: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut out = record(129, &[]);
    out.extend(record(148, &[0xff; 16])); // A bogus dimension must never allocate a dense grid.
    out.extend(record(145, &[]));
    out.extend(data);
    out.extend(record(146, &[]));
    out.extend(tail);
    out.extend(record(130, &[]));
    out
}
fn book(date1904: bool) -> Vec<u8> {
    let mut out = record(131, &[]);
    let mut props = u32::from(date1904).to_le_bytes().to_vec();
    props.extend(0u32.to_le_bytes());
    props.extend(wide(""));
    out.extend(record(153, &props));
    out.extend(record(143, &[]));
    for (id, rel, name, state) in [(1u32, "one", "Data", 0u32), (2, "two", "Hidden", 1)] {
        let mut data = state.to_le_bytes().to_vec();
        data.extend(id.to_le_bytes());
        data.extend(wide(rel));
        data.extend(wide(name));
        out.extend(record(156, &data));
    }
    out.extend(record(144, &[]));
    out.extend(record(132, &[]));
    out
}
fn sst() -> Vec<u8> {
    let mut out = record(159, &[1, 0, 0, 0, 1, 0, 0, 0]);
    out.extend(record(19, &rich("shared 🦀 café", true)));
    out.extend(record(160, &[]));
    out
}
fn package(book: &[u8], sheet: &[u8], strings: &[u8], roots: &str, rels: &str, types: &str) -> Vec<u8> {
    make_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", roots.as_bytes()),
        ("records/book.bin", book),
        ("records/_rels/book.bin.rels", rels.as_bytes()),
        ("data/one.bin", sheet),
        ("data/two.bin", &worksheet(&[], &[])),
        ("text/strings.bin", strings),
    ])
}
fn file(sheet: &[u8]) -> Vec<u8> {
    package(&book(false), sheet, &sst(), ROOT, RELS, TYPES)
}
fn text(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn xlsb_scalar_caches_rk_unicode_visibility_and_epochs_survive_xlsx_save() {
    let mut data = row(2);
    let real_rk = (3.5f64.to_bits() >> 32) as u32;
    let encoded = [
        cell(1, 0, &[]),
        cell(2, 1, &((-123i32 << 2) | 2).to_le_bytes()),
        cell(2, 2, &((123i32 << 2) | 3).to_le_bytes()),
        cell(2, 3, &real_rk.to_le_bytes()),
        cell(2, 4, &(real_rk | 1).to_le_bytes()),
        cell(3, 5, &[0x17]),
        cell(4, 6, &[1]),
        cell(5, 7, &123.5f64.to_le_bytes()),
        cell(6, 8, &wide("inline λ")),
        cell(7, 9, &0u32.to_le_bytes()),
        cell(8, 10, &wide("cached text")),
        cell(9, 11, &2.0f64.to_le_bytes()),
        cell(10, 12, &[0]),
        cell(11, 13, &[7]),
        cell(62, 14, &rich("rich text", true)),
    ];
    for cell in encoded {
        data.extend(cell);
    }
    let mut merge = vec![];
    for n in [5u32, 6, 0, 1] {
        merge.extend(n.to_le_bytes());
    }
    let sheet = worksheet(&data, &record(176, &merge));
    let expected = [
        Value::Empty,
        Value::Number(-123.0),
        Value::Number(1.23),
        Value::Number(3.5),
        Value::Number(0.035),
        Value::Error(CellError::Ref),
        Value::Bool(true),
        Value::Number(123.5),
        text("inline λ"),
        text("shared 🦀 café"),
        text("cached text"),
        Value::Number(2.0),
        Value::Bool(false),
        Value::Error(CellError::Div0),
        text("rich text"),
    ];
    for epoch in [false, true] {
        let bytes = package(&book(epoch), &sheet, &sst(), ROOT, RELS, TYPES);
        assert_eq!(sniff(&bytes), Format::Xlsb);
        let (wb, report) = read_xlsb(&bytes).unwrap();
        assert_eq!(wb.date_system, if epoch { DateSystem::D1904 } else { DateSystem::D1900 });
        assert_eq!(wb.sheets.len(), 2);
        assert_eq!(wb.sheets[1].visibility, Visibility::Hidden);
        assert_eq!(wb.sheets[0].cells.len(), 14);
        assert_eq!(wb.sheets[0].merges, vec![RangeRef::new(CellRef::new(5, 0), CellRef::new(6, 1))]);
        assert!(report.warnings.iter().any(|w| w.contains("will not recalculate") && w.contains("may display as numbers")));
        for (col, expected) in expected.iter().enumerate() {
            assert_eq!(&wb.sheets[0].value(CellRef::new(2, col as u32)), expected);
        }
        assert!(wb.sheets[0].cells.iter().all(|(_, c)| c.formula.is_none()));
        let (roundtrip, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(roundtrip.date_system, wb.date_system);
        for (col, expected) in expected.iter().enumerate() {
            assert_eq!(&roundtrip.sheets[0].value(CellRef::new(2, col as u32)), expected);
        }
    }
}

#[test]
fn xlsb_extension_blocks_are_skipped_with_strict_boundaries_and_record_framing() {
    let mut data = row(0);
    data.extend(cell(5, 0, &1.0f64.to_le_bytes()));
    data.extend(record(35, &[0; 4]));
    data.extend(record(37, &[1, 0, 0, 0, 0, 0]));
    data.extend(cell(5, 1, &99.0f64.to_le_bytes()));
    data.extend(record(130, &[])); // Looks like EndSheet, but is inside an ignored extension.
    data.extend(record(38, &[]));
    data.extend(record(36, &[]));
    data.extend([0xe8, 0x87, 0x80, 0x80, 0x80, 0x80]); // ID1000: ignored high second-ID/fourth-length bits.
    data.extend(cell(5, 1, &2.0f64.to_le_bytes()));
    let (wb, report) = read_xlsb(&file(&worksheet(&data, &[]))).unwrap();
    assert_eq!(wb.sheets[0].cells.len(), 2);
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 1)), Value::Number(2.0));
    assert!(report.warnings.iter().any(|w| w.contains("blocks were skipped")));
    for bad in [
        vec![0x81],
        vec![0x81, 0],
        vec![1, 0x80],
        vec![1, 10, 0],
        record(36, &[]),
        [record(35, &[0; 4]), record(38, &[])].concat(),
        record(35, &[0; 4]),
        record(37, &[0, 0]),
    ] {
        assert!(read_xlsb(&file(&worksheet(&bad, &[]))).is_err(), "{bad:?}");
    }
    let nested = record(35, &[0; 4]).repeat(65);
    assert!(read_xlsb(&file(&worksheet(&nested, &[]))).unwrap_err().to_string().contains("nested too deeply"));
    let mut missing_end = worksheet(&[], &[]);
    missing_end.truncate(missing_end.len() - record(130, &[]).len());
    assert!(read_xlsb(&file(&missing_end)).is_err());
}

#[test]
fn xlsb_bad_values_strings_and_coordinates_fail_without_silent_data_loss() {
    let malformed = [
        cell(4, 0, &[2]),
        cell(3, 0, &[0xff]),
        cell(5, 0, &f64::INFINITY.to_le_bytes()),
        cell(2, 0, &0x7ff00000u32.to_le_bytes()),
        cell(7, 0, &1u32.to_le_bytes()),
        cell(6, 0, &[1, 0, 0, 0, 0, 0xd8]),
        cell(6, 0, &u32::MAX.to_le_bytes()),
        cell(5, 16384, &1.0f64.to_le_bytes()),
        cell(5, 0, &[0; 3]),
        cell(62, 0, &[1, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]),
    ];
    for bad in malformed {
        assert!(read_xlsb(&file(&worksheet(&[row(0), bad].concat(), &[]))).is_err());
    }
    assert!(read_xlsb(&file(&worksheet(&cell(5, 0, &1.0f64.to_le_bytes()), &[]))).is_err());
    assert!(read_xlsb(&file(&worksheet(&row(1048576), &[]))).is_err());
    let repeated = [row(0), cell(5, 0, &1.0f64.to_le_bytes()), cell(5, 0, &2.0f64.to_le_bytes())].concat();
    assert!(read_xlsb(&file(&worksheet(&repeated, &[]))).is_err());
    let mut formula_payload = [0u8; 18].to_vec(); // Cell header + f64 cache + flags, but no parsed-formula structure.
    assert!(read_xlsb(&file(&worksheet(&[row(0), record(9, &formula_payload)].concat(), &[]))).is_err());
    formula_payload.extend(0u32.to_le_bytes()); // Invalid zero token count.
    assert!(read_xlsb(&file(&worksheet(&[row(0), record(9, &formula_payload)].concat(), &[]))).is_err());
    let bad_sst = [record(159, &[1, 0, 0, 0, 2, 0, 0, 0]), record(160, &[])].concat();
    assert!(read_xlsb(&package(&book(false), &worksheet(&[], &[]), &bad_sst, ROOT, RELS, TYPES)).is_err());
}

#[test]
fn xlsb_requires_internal_typed_relationships_and_sniffs_only_root_workbook() {
    let sheet = worksheet(&[], &[]);
    for (roots, rels, types) in [
        (ROOT.replace("officeDocument\"", "wrong\""), RELS.to_owned(), TYPES.to_owned()),
        (ROOT.replace("Target=", "TargetMode=\"External\" Target="), RELS.to_owned(), TYPES.to_owned()),
        (ROOT.to_owned(), RELS.replace("Target=\"../data/one.bin\"", "Target=\"missing.bin\""), TYPES.to_owned()),
        (
            ROOT.to_owned(),
            RELS.replace("Target=\"../data/one.bin\"", "TargetMode=\"External\" Target=\"https://example.com/data.bin\""),
            TYPES.to_owned(),
        ),
        (
            ROOT.to_owned(),
            RELS.replace("Target=\"../text/strings.bin\"", "TargetMode=\"External\" Target=\"https://example.com/strings.bin\""),
            TYPES.to_owned(),
        ),
        (ROOT.to_owned(), RELS.to_owned(), TYPES.replace("application/vnd.ms-excel.worksheet", "application/xml")),
        (ROOT.to_owned(), RELS.trim_end_matches("</Relationships>").to_owned(), TYPES.to_owned()),
        (ROOT.to_owned(), RELS.to_owned(), TYPES.replace("http://schemas.openxmlformats.org/package/2006/content-types", "wrong")),
    ] {
        assert!(read_xlsb(&package(&book(false), &sheet, &sst(), &roots, &rels, &types)).is_err());
    }
    let chartsheet = RELS.replace(
        "Id=\"two\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet",
        "Id=\"two\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet",
    );
    let (wb, report) = read_xlsb(&package(&book(false), &sheet, &sst(), ROOT, &chartsheet, TYPES)).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert!(report.warnings.iter().any(|w| w.contains("non-worksheet")));
    // An unrelated binary part must not change the format of an XML workbook package.
    let xml_book = TYPES.replace(
        "/records/book.bin\" ContentType=\"application/vnd.ms-excel.sheet.binary.macroEnabled.main",
        "/elsewhere.bin\" ContentType=\"application/vnd.ms-excel.sheet.binary.macroEnabled.main",
    );
    assert_eq!(sniff(&package(&book(false), &sheet, &sst(), ROOT, RELS, &xml_book)), Format::Xlsx);
    let spec_type = TYPES.replace("application/vnd.ms-excel.sheet.binary.macroEnabled.main", "application/vnd.ms-excel.main");
    assert_eq!(sniff(&package(&book(false), &sheet, &sst(), ROOT, RELS, &spec_type)), Format::Xlsb);
    assert!(read_xlsb(&package(&book(false), &sheet, &sst(), ROOT, RELS, &spec_type)).is_ok());
}

#[test]
fn xlsb_declared_counts_and_decoded_text_are_bounded() {
    let bad_sst = record(159, &[0xff; 8]);
    assert!(read_xlsb(&package(&book(false), &worksheet(&[], &[]), &bad_sst, ROOT, RELS, TYPES)).is_err());
    let mut count_mismatch = record(159, &[1, 0, 0, 0, 1, 0, 0, 0]);
    count_mismatch.extend(record(160, &[]));
    assert!(read_xlsb(&package(&book(false), &worksheet(&[], &[]), &count_mismatch, ROOT, RELS, TYPES)).is_err());
    let mut data = row(0);
    let text = wide(&"x".repeat(32767));
    for col in 0..2050 {
        data.extend(cell(6, col, &text));
    }
    assert!(read_xlsb(&file(&worksheet(&data, &[]))).unwrap_err().to_string().contains("64 MiB"));
}

#[test]
fn xlsb_phonetic_runs_use_documented_size_and_tolerate_padding() {
    // [MS-XLSB] 2.5.103: a PhRun is 10 bytes. Phonetic data is not imported, so
    // padding after the documented runs (e.g. 12-byte runs) is ignored, while
    // a run shorter than documented is still rejected.
    let phonetic = |runs: u32, bytes: usize| {
        let mut out = vec![2];
        out.extend(wide("ruby"));
        out.extend(wide("ルビ"));
        out.extend(runs.to_le_bytes());
        out.extend(vec![0; bytes]);
        out
    };
    for padded in [phonetic(1, 10), phonetic(1, 12), phonetic(2, 24)] {
        let (wb, _) = read_xlsb(&file(&worksheet(&[row(0), cell(62, 0, &padded)].concat(), &[]))).unwrap();
        assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), text("ruby"));
    }
    assert!(read_xlsb(&file(&worksheet(&[row(0), cell(62, 0, &phonetic(2, 12))].concat(), &[]))).is_err());
}
