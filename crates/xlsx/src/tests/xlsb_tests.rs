//! Original BIFF12 records and ZIP packages assembled in Rust from the public format schema.
use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::Visibility;

use super::make_zip;
use crate::{Format, FunctionInfo, read_xlsb, read_xlsb_with, read_xlsx, sniff, write_xlsx};

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
        data.extend([0x1e, 1, 0]); // PtgInt(1): the formula `=1`, with the cache as its value.
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
        assert!(report.warnings.iter().any(|w| w.contains("cell formulas were read") && w.contains("may display as numbers")));
        for (col, expected) in expected.iter().enumerate() {
            assert_eq!(&wb.sheets[0].value(CellRef::new(2, col as u32)), expected);
        }
        // The formula records (8 to 11) keep their formula, `=1`, with the cached result as value.
        for (at, c) in wb.sheets[0].cells.iter() {
            assert_eq!(c.formula.as_ref().map(|f| f.text.as_str()), (10..=13).contains(&at.col).then_some("1"), "{at:?}");
        }
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

/// A formula cell (`BrtFmlaNum`, cached 0) with its own tokens and extra data.
fn formula_cell(col: u32, rgce: &[u8], rgcb: &[u8]) -> Vec<u8> {
    let mut data = col.to_le_bytes().to_vec();
    data.extend([0; 4]);
    data.extend(0f64.to_le_bytes());
    data.extend([0; 2]);
    data.extend((rgce.len() as u32).to_le_bytes());
    data.extend(rgce);
    data.extend((rgcb.len() as u32).to_le_bytes());
    data.extend(rgcb);
    record(9, &data)
}
fn name(flags: u32, text: &str) -> Vec<u8> {
    let mut data = flags.to_le_bytes().to_vec();
    data.push(0);
    data.extend(u32::MAX.to_le_bytes()); // Workbook scope.
    data.extend(wide(text));
    data.extend([2, 0, 0, 0, 0x1c, 0x1d, 0, 0, 0, 0]); // Formula `#NAME?`, no extra data.
    data.extend(u32::MAX.to_le_bytes()); // No comment.
    record(39, &data)
}
/// `book(false)` with a supporting-link block and defined names, as Excel writes them: this
/// workbook, then another; sheet links Data, Hidden, Data:Hidden, a deleted sheet and the other
/// workbook; hidden names for CONCAT, LET and a LET parameter, then the user name Rate.
fn book_with_links(extern_sheet: &[u8]) -> Vec<u8> {
    let plain = book(false);
    let end = record(132, &[]);
    let mut out = plain[..plain.len() - end.len()].to_vec();
    out.extend(record(353, &[]));
    out.extend(record(357, &[]));
    out.extend(record(355, &wide("rId9")));
    out.extend(record(362, extern_sheet));
    out.extend(record(354, &[]));
    const FUTURE: u32 = 0x0002_000b;
    out.extend(name(FUTURE, "_xlfn.CONCAT"));
    out.extend(name(FUTURE, "_xlfn.LET"));
    out.extend(name(0x0002_0019, "_xlpm.x"));
    out.extend(name(0, "Rate"));
    out.extend(end);
    out
}
fn xti() -> Vec<u8> {
    let mut out = 5u32.to_le_bytes().to_vec();
    for (book, first, last) in [(0u32, 0i32, 0i32), (0, 1, 1), (0, 0, 1), (0, -1, -1), (1, 0, 0)] {
        out.extend(book.to_le_bytes());
        out.extend(first.to_le_bytes());
        out.extend(last.to_le_bytes());
    }
    out
}
fn loc(row: u32, col: u16) -> Vec<u8> {
    [&row.to_le_bytes()[..], &col.to_le_bytes()].concat()
}
/// Relative row and column flags of a `ColRelShort`.
const REL: u16 = 0xC000;

#[test]
fn xlsb_cell_formulas_are_read_in_their_xlsx_spelling() {
    let ref3d = |ixti: u16, row: u32, col: u16| [&[0x5Au8][..], &ixti.to_le_bytes(), &loc(row, col)].concat();
    let cells: Vec<(u32, Vec<u8>, Vec<u8>)> = vec![
        (1, [&[0x44u8][..], &loc(0, REL), &[0x1E, 2, 0, 0x05]].concat(), vec![]),
        (2, [&[0x25u8][..], &0u32.to_le_bytes(), &1u32.to_le_bytes(), &0u16.to_le_bytes(), &REL.to_le_bytes(), &[0x19, 0x10, 0, 0]].concat(), vec![]),
        (3, ref3d(1, 1, 1 | REL), vec![]),
        (
            4,
            [
                &[0x3Bu8][..],
                &2u16.to_le_bytes(),
                &0u32.to_le_bytes(),
                &0xFFFFFu32.to_le_bytes(),
                &REL.to_le_bytes(),
                &REL.to_le_bytes(),
                &[0x19, 0x10, 0, 0],
            ]
            .concat(),
            vec![],
        ),
        (5, [&[0x23u8, 1, 0, 0, 0][..], &[0x17, 1, 0, b'a', 0], &[0x17, 1, 0, b'b', 0], &[0x42, 3, 0xFF, 0]].concat(), vec![]),
        (6, vec![0x1E, 1, 0, 0x1E, 0, 0, 0x06, 0x1E, 2, 0, 0x41, 0xE0, 0x01], vec![]),
        (
            7,
            [&[0x60u8][..], &[0xAB; 14]].concat(),
            [&2u32.to_le_bytes()[..], &2u32.to_le_bytes(), &[0], &1f64.to_le_bytes(), &[1, 1, 0, b'x', 0], &[2, 1], &[4, 0x2A, 0, 0, 0]].concat(),
        ),
        (
            8,
            [&[0x23u8, 2, 0, 0, 0][..], &[0x23, 3, 0, 0, 0, 0x1E, 1, 0], &[0x23, 3, 0, 0, 0, 0x1E, 1, 0, 0x03], &[0x42, 4, 0xFF, 0]].concat(),
            vec![],
        ),
        (9, ref3d(3, 0, 0), vec![]),
        (10, [&[0x25u8][..], &0u32.to_le_bytes(), &1u32.to_le_bytes(), &REL.to_le_bytes(), &REL.to_le_bytes(), &[0x41, 20, 0]].concat(), vec![]),
        // Kept as their saved results:
        (11, vec![0x43, 4, 0, 0, 0, 0x1E, 2, 0, 0x05], vec![]),
        (12, ref3d(4, 0, REL), vec![]),
        (13, vec![0x01, 0, 0, 0, 0], vec![0, 0, 0, 0]), // Part of a shared formula at A1 that isn't one.
    ];
    let mut data = row(0);
    data.extend(cell(5, 0, &3.0f64.to_le_bytes()));
    for (col, rgce, rgcb) in &cells {
        data.extend(formula_cell(*col, rgce, rgcb));
    }
    data.extend(record(49, &1u32.to_le_bytes())); // BrtCellMeta: the next formula is a dynamic array.
    data.extend(formula_cell(14, &[0x1E, 1, 0], &[]));
    data.extend(formula_cell(15, &[0x18, 0x19, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0], &[])); // A table reference.
    let bytes = package(&book_with_links(&xti()), &worksheet(&data, &[]), &sst(), ROOT, RELS, TYPES);
    let (wb, report) = read_xlsb(&bytes).unwrap();
    let sheet = &wb.sheets[0];
    let got: Vec<_> = (1..=15).map(|c| sheet.cell(CellRef::new(0, c)).and_then(|x| x.formula.as_ref()).map(|f| f.text.clone())).collect();
    let want = [
        "A1*2",
        "SUM($A$1:A2)",
        "Hidden!B2",
        "SUM(Data:Hidden!A:A)",
        // Hidden `_xlfn.` / `_xlpm.` names end up spelled as the .xlsx reader leaves them.
        "CONCAT(\"a\",\"b\")",
        "IFERROR(1/0,2)",
        // Legacy formulas get Excel's implicit `@`, as from .xlsx without `cm`.
        "@{1,\"x\";TRUE,#N/A}",
        "LET(x,1,x+1)",
        "#REF!",
        "SQRT(@A1:A2)",
    ];
    let mut want: Vec<_> = want.iter().map(|w| Some(w.to_string())).collect();
    want.resize(15, None);
    // A BrtCellMeta before a cell marks a dynamic-array formula, as `cm` does in .xlsx.
    want[13] = Some("1".into());
    assert_eq!(got, want);
    // Every formula cell keeps its saved result as its value until it recalculates.
    assert!((1..=15).all(|c| sheet.value(CellRef::new(0, c)) == Value::Number(0.0)));
    for why in ["defined names", "other workbooks", "missing from the file", "structured table references"] {
        assert!(report.warnings.iter().any(|w| w.starts_with("Some formulas use") && w.contains(why)), "{why}: {:?}", report.warnings);
    }
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheets[0].cell(CellRef::new(0, 1)).and_then(|c| c.formula.as_ref()).map(|f| f.text.as_str()), Some("A1*2"));
}

#[test]
fn xlsb_unreadable_links_keep_every_formula_as_its_value() {
    // A sheet-link count past the spec's limit, and a truncated list: the file opens as it did
    // before formulas were read, with values only.
    let mut truncated = xti();
    truncated.truncate(20);
    for extern_sheet in [70_000u32.to_le_bytes().to_vec(), truncated] {
        let data = [row(0), formula_cell(0, &[0x1E, 1, 0], &[])].concat();
        let bytes = package(&book_with_links(&extern_sheet), &worksheet(&data, &[]), &sst(), ROOT, RELS, TYPES);
        let (wb, report) = read_xlsb(&bytes).unwrap();
        let cell = wb.sheets[0].cell(CellRef::new(0, 0)).unwrap();
        assert!(cell.formula.is_none());
        assert_eq!(cell.value, Value::Number(0.0));
        assert!(report.warnings.iter().any(|w| w.contains("could not be read")));
        // The overview doesn't claim formulas were read.
        assert!(report.warnings[0].contains("formulas were kept as their saved results"), "{:?}", report.warnings);
    }
}

#[test]
fn xlsb_formula_grouping_follows_token_order() {
    // Reverse-Polish tokens group by their order alone. Excel writes PtgParen wherever its text
    // has parentheses, but another writer need not: the text gets the ones it needs to mean the
    // same, and no more.
    let a = |row: u32| [&[0x24u8][..], &loc(row, REL)].concat();
    let int = |n: u8| vec![0x1E, n, 0];
    let cases: Vec<(Vec<u8>, &str)> = vec![
        ([int(1), int(2), vec![0x03], int(3), vec![0x05]].concat(), "(1+2)*3"),
        ([int(1), int(2), int(3), vec![0x04, 0x04]].concat(), "1-(2-3)"),
        ([int(1), int(2), vec![0x04], int(3), vec![0x04]].concat(), "1-2-3"),
        ([int(2), vec![0x13], int(2), vec![0x07]].concat(), "-2^2"),
        ([int(2), int(2), vec![0x07, 0x13]].concat(), "-(2^2)"),
        // Excel's tokens for its own `-1%`: the parser groups it the other way, to the same value.
        ([int(1), vec![0x13, 0x14]].concat(), "-1%"),
        ([int(1), int(2), vec![0x03, 0x14]].concat(), "(1+2)%"),
        ([a(0), a(1), vec![0x10], vec![0x19, 0x10, 0, 0]].concat(), "SUM((A1,A2))"),
        ([int(1), int(2), vec![0x03, 0x15], int(3), vec![0x05]].concat(), "(1+2)*3"),
    ];
    let data: Vec<u8> =
        [row(0)].into_iter().chain(cases.iter().enumerate().map(|(i, (rgce, _))| formula_cell(i as u32, rgce, &[]))).flatten().collect();
    let (wb, _) = read_xlsb(&package(&book_with_links(&xti()), &worksheet(&data, &[]), &sst(), ROOT, RELS, TYPES)).unwrap();
    for (i, (_, want)) in cases.iter().enumerate() {
        let f = wb.sheets[0].cell(CellRef::new(0, i as u32)).and_then(|c| c.formula.clone()).unwrap();
        assert_eq!(f.text, *want);
    }
}

#[test]
fn xlsb_hostile_formula_tokens_keep_the_saved_result() {
    let mut deep = vec![0x1E, 1, 0];
    deep.resize(16384, 0x15); // Thousands of parentheses: refused by the shared depth guard.
    let empty_array = [&[0x60u8][..], &[0; 14]].concat();
    let bad: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (vec![0x1E, 1], vec![]),                                             // Truncated PtgInt.
        (vec![0x17, 1, 0, 0x00, 0xD8], vec![]),                              // Unpaired surrogate.
        (vec![0x17, 0, 1], vec![]),                                          // Text constant over 255.
        ([&[0x24u8][..], &loc(1_048_576, 0)].concat(), vec![]),              // Row past the grid.
        (empty_array.clone(), vec![0; 8]),                                   // Empty array constant.
        (empty_array, vec![0xFF; 8]),                                        // Huge one.
        (vec![0x1E, 1, 0, 0x1E, 2, 0], vec![]),                              // Unused operand.
        (vec![0x21, 0xD4, 0x01], vec![]),                                    // Unassigned function number.
        (vec![0x2C, 0, 0, 0, 0, 0, 0], vec![]),                              // PtgRefN outside a shared formula.
        (vec![0x23, 0xFF, 0xFF, 0xFF, 0xFF], vec![]),                        // Name index past the list.
        ([&[0x5Au8][..], &9u16.to_le_bytes(), &loc(0, 0)].concat(), vec![]), // Missing sheet link.
        (deep, vec![]),
    ];
    for (rgce, rgcb) in bad {
        let data = [row(0), formula_cell(0, &rgce, &rgcb)].concat();
        let (wb, _) = read_xlsb(&package(&book_with_links(&xti()), &worksheet(&data, &[]), &sst(), ROOT, RELS, TYPES)).unwrap();
        let cell = wb.sheets[0].cell(CellRef::new(0, 0)).unwrap();
        assert!(cell.formula.is_none(), "{rgce:?}");
        assert_eq!(cell.value, Value::Number(0.0));
    }
}

#[test]
fn xlsb_python_formulas_need_their_code_in_the_cell() {
    // `PY("code", 0)` carries its code and stays a formula the engine turns into a Python cell;
    // `PY(0, 0)` names a script elsewhere in the package and keeps its saved result.
    let mut book = book_with_links(&xti());
    let end = record(132, &[]);
    book.truncate(book.len() - end.len());
    book.extend(name(0x0002_000b, "_xlfn._xlws.PY"));
    book.extend(end);
    let py = |arg: &[u8]| [&[0x23u8, 5, 0, 0, 0][..], arg, &[0x1E, 0, 0, 0x42, 3, 0xFF, 0]].concat();
    // Code longer than one string constant is written as constants joined with `&`.
    let joined = [&[0x17u8, 1, 0, b'1', 0][..], &[0x17, 2, 0, b'+', 0, b'1', 0], &[0x08]].concat();
    let data =
        [row(0), formula_cell(0, &py(&[0x17, 1, 0, b'1', 0]), &[]), formula_cell(1, &py(&[0x1E, 0, 0]), &[]), formula_cell(2, &py(&joined), &[])]
            .concat();
    let bytes = package(&book, &worksheet(&data, &[]), &sst(), ROOT, RELS, TYPES);
    // The engine's facts, where PY is no built-in function: a legacy formula calling it would get
    // an implicit `@`, which the Python-cell reading wouldn't recognize. PY formulas don't.
    let engine = FunctionInfo { is_builtin: |n| !n.eq_ignore_ascii_case("PY"), is_scalar_arg: |_, _| true };
    for (wb, report) in [read_xlsb(&bytes).unwrap(), read_xlsb_with(&bytes, engine).unwrap()] {
        let formula = |c| wb.sheets[0].cell(CellRef::new(0, c)).and_then(|x| x.formula.as_ref()).map(|f| f.text.clone());
        let code = |c| gridcraft_model::PyCode::from_formula(&formula(c).unwrap()).map(|p| p.code);
        assert_eq!(code(0), Some("1".into()), "{:?}", formula(0));
        assert_eq!(code(2), Some("1+1".into()), "{:?}", formula(2));
        assert_eq!(formula(1), None);
        assert!(report.warnings.iter().any(|w| w.contains("Python code stored outside")));
    }
}

/// `BrtShrFmla` (427) or `BrtArrFmla` (426) over `r0:c0`–`r1:c1`.
fn range_formula(id: u16, (r0, c0, r1, c1): (u32, u32, u32, u32), rgce: &[u8]) -> Vec<u8> {
    let mut data = vec![];
    for n in [r0, r1, c0, c1] {
        data.extend(n.to_le_bytes());
    }
    if id == 426 {
        data.push(0x08); // The flags Excel writes for an ordinary array.
    }
    data.extend((rgce.len() as u32).to_le_bytes());
    data.extend(rgce);
    data.extend(0u32.to_le_bytes());
    record(id, &data)
}
/// A cell of the shared or array formula whose first cell is `row`, `col` (PtgExp + PtgExtraCol).
fn part(col: u32, (row, first_col): (u32, u32)) -> Vec<u8> {
    formula_cell(col, &[&[0x01u8][..], &row.to_le_bytes()].concat(), &first_col.to_le_bytes())
}
fn ref_n(row: i32, col: u16) -> Vec<u8> {
    [&[0x4Cu8][..], &row.to_le_bytes(), &col.to_le_bytes()].concat()
}

#[test]
fn xlsb_shared_array_and_dynamic_formulas_read_as_from_xlsx() {
    // As Excel writes them: C1:C3 share `A1*10` (PtgRefN, column offset -2 as 16382); D1:E2
    // share `$A1+A$1`; F1:F2 hold the Ctrl+Shift+Enter array {=A1:A2*2}; G1 spills `=A1:A2`.
    let area = [&[0x45u8][..], &0u32.to_le_bytes(), &1u32.to_le_bytes(), &REL.to_le_bytes(), &REL.to_le_bytes()].concat();
    let mut data = row(0);
    data.extend(cell(5, 0, &1.0f64.to_le_bytes()));
    data.extend(part(2, (0, 2)));
    data.extend(range_formula(427, (0, 2, 2, 2), &[ref_n(0, 16382 | REL), vec![0x1E, 10, 0, 0x05]].concat()));
    data.extend(part(3, (0, 3)));
    data.extend(range_formula(427, (0, 3, 1, 4), &[ref_n(0, 0x8000), ref_n(0, 16381 | 0x4000), vec![0x03]].concat()));
    data.extend(part(4, (0, 3)));
    data.extend(part(5, (0, 5)));
    data.extend(range_formula(426, (0, 5, 1, 5), &[area.clone(), vec![0x1E, 2, 0, 0x05]].concat()));
    data.extend(record(49, &1u32.to_le_bytes()));
    data.extend(record(50, &1u32.to_le_bytes())); // BrtValueMeta in between, as Excel writes for a spill error.
    data.extend(part(6, (0, 6)));
    data.extend(range_formula(426, (0, 6, 1, 6), &area));
    data.extend(row(1));
    data.extend(cell(5, 0, &2.0f64.to_le_bytes()));
    for (col, first) in [(2, (0, 2)), (3, (0, 3)), (4, (0, 3)), (5, (0, 5)), (6, (0, 6))] {
        data.extend(part(col, first));
    }
    data.extend(row(2));
    data.extend(part(2, (0, 2)));
    let (wb, report) = read_xlsb(&file(&worksheet(&data, &[]))).unwrap();
    let sheet = &wb.sheets[0];
    let formula = |a: &str| sheet.cell(CellRef::parse(a).unwrap()).and_then(|c| c.formula.clone());
    let text = |a: &str| formula(a).map(|f| f.text.clone());
    for (at, want) in [("C1", "A1*10"), ("C2", "A2*10"), ("C3", "A3*10"), ("D1", "$A1+A$1"), ("E1", "$A1+B$1"), ("D2", "$A2+A$1"), ("E2", "$A2+B$1")]
    {
        assert_eq!(text(at).as_deref(), Some(want), "{at}");
        assert!(formula(at).is_some_and(|f| !f.dynamic && f.array.is_none()));
    }
    // The array keeps its range; the dynamic array is marked and spills. Their other cells
    // drop their cached values, as from .xlsx, for recalculation to fill.
    let f1 = formula("F1").unwrap();
    assert_eq!((f1.text.as_str(), f1.array, f1.dynamic), ("A1:A2*2", RangeRef::parse("F1:F2"), false));
    let g1 = formula("G1").unwrap();
    assert_eq!((g1.text.as_str(), g1.array, g1.dynamic), ("A1:A2", None, true));
    for at in ["F2", "G2"] {
        assert_eq!(sheet.value(CellRef::parse(at).unwrap()), Value::Empty, "{at}");
        assert!(formula(at).is_none());
    }
    assert_eq!(sheet.value(CellRef::parse("F1").unwrap()), Value::Number(0.0));
    assert!(!report.warnings.iter().any(|w| w.starts_with("Some formulas")), "{:?}", report.warnings);
    // Saved as .xlsx and read back, every formula is the same.
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    for at in ["C3", "E2", "F1", "G1"] {
        let a = formula(at).unwrap();
        let b = back.sheets[0].cell(CellRef::parse(at).unwrap()).and_then(|c| c.formula.clone()).unwrap();
        assert_eq!((&a.text, a.array, a.dynamic), (&b.text, b.array, b.dynamic), "{at}");
    }
}

#[test]
fn xlsb_unreadable_shared_formulas_keep_their_values() {
    // A shared formula with an unsupported token, a range that doesn't start at its first cell,
    // a truncated record, and a first cell with no formula record after it: every cell of each
    // keeps its saved result, and the file opens.
    let unknown = range_formula(427, (0, 0, 1, 0), &[0xFF]);
    let wrong_start = range_formula(427, (0, 1, 1, 1), &[0x1E, 1, 0]);
    // A well-framed record whose payload stops before rgcb's length.
    let short: Vec<u8> = [&[0u8; 4][..], &1u32.to_le_bytes(), &[0; 8], &3u32.to_le_bytes(), &[0x1E, 1, 0]].concat();
    let truncated = record(427, &short);
    for after_first in [unknown, wrong_start, truncated, cell(5, 1, &1.0f64.to_le_bytes())] {
        let data = [row(0), part(0, (0, 0)), after_first, row(1), part(0, (0, 0))].concat();
        let (wb, report) = read_xlsb(&file(&worksheet(&data, &[]))).unwrap();
        for r in [0, 1] {
            let cell = wb.sheets[0].cell(CellRef::new(r, 0)).unwrap();
            assert!(cell.formula.is_none());
            assert_eq!(cell.value, Value::Number(0.0));
        }
        assert!(report.warnings.iter().any(|w| w.starts_with("Some formulas")));
    }
}
