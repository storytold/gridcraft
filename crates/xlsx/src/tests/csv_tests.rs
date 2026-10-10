use gridcraft_core::{CellRef, DateSystem, Value};
use gridcraft_model::{Sheet, Workbook};

use crate::csv::{decode_text, sniff_delimiter};
use crate::{CsvOptions, Format, read_csv, sniff, write_csv};

fn v(wb: &Workbook, r: u32, c: u32) -> Value {
    wb.sheet(0).unwrap().value(CellRef::new(r, c))
}

#[test]
fn quoting_and_newlines() {
    let src = "a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,3\n\nlast";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(wb.sheets[0].name, "Sheet1");
    assert_eq!(v(&wb, 0, 0), Value::text("a"));
    assert_eq!(v(&wb, 0, 1), Value::text("b,c"));
    assert_eq!(v(&wb, 0, 2), Value::text("say \"hi\""));
    assert_eq!(v(&wb, 1, 0), Value::text("multi\nline"));
    assert_eq!(v(&wb, 1, 1), Value::Empty);
    assert_eq!(v(&wb, 1, 2), Value::Number(3.0));
    assert_eq!(v(&wb, 2, 0), Value::Empty);
    assert_eq!(v(&wb, 3, 0), Value::text("last"));
}

#[test]
fn values_and_formats() {
    let src = "1,2.5,\"1,234\",50%,TRUE,#N/A,2026-03-04,$5.00,'007,text";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::Number(1.0));
    assert_eq!(v(&wb, 0, 1), Value::Number(2.5));
    assert_eq!(v(&wb, 0, 2), Value::Number(1234.0));
    assert_eq!(v(&wb, 0, 3), Value::Number(0.5));
    assert_eq!(v(&wb, 0, 4), Value::Bool(true));
    assert!(v(&wb, 0, 5).is_error());
    assert!(v(&wb, 0, 6).is_number());
    let s = wb.sheet(0).unwrap();
    let st = wb.styles.get(s.cell(CellRef::new(0, 6)).unwrap().style);
    assert_ne!(st.num_fmt.as_str(), "General");
    assert_eq!(v(&wb, 0, 8), Value::text("007"));
    // 1904 dates differ from 1900 dates.
    let wb2 = read_csv(b"2026-03-04", &CsvOptions { date_system: DateSystem::D1904, ..Default::default() }).unwrap();
    assert_eq!(wb2.date_system, DateSystem::D1904);
    assert_eq!(v(&wb, 0, 6).as_f64().unwrap() - v(&wb2, 0, 0).as_f64().unwrap(), 1462.0);
    // parse_values off keeps text.
    let wb3 = read_csv(b"1,TRUE", &CsvOptions { parse_values: false, ..Default::default() }).unwrap();
    assert_eq!(v(&wb3, 0, 0), Value::text("1"));
}

#[test]
fn encodings() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice("é,x".as_bytes());
    assert_eq!(decode_text(&b), "é,x");
    let mut le = vec![0xFF, 0xFE];
    for u in "ü;1".encode_utf16() {
        le.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(decode_text(&le), "ü;1");
    let mut be = vec![0xFE, 0xFF];
    for u in "ß".encode_utf16() {
        be.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(decode_text(&be), "ß");
    assert_eq!(decode_text(&[0x80, b'a', 0xE9]), "€aé");
    let wb = read_csv(&le, &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("ü"));
    assert_eq!(v(&wb, 0, 1), Value::Number(1.0));
}

#[test]
fn delimiter_sniffing() {
    assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", '"'), b';');
    assert_eq!(sniff_delimiter("a\tb\n1\t2\n", '"'), b'\t');
    assert_eq!(sniff_delimiter("a,b\n\"x;y;z\",2\n", '"'), b',');
    assert_eq!(sniff_delimiter("single", '"'), b',');
    // A delimiter missing from some lines is text, not a separator (single-column export round trip).
    assert_eq!(sniff_delimiter("alpha;beta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("alpha\tbeta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("a;b\r\n\r\nc;d\r\n", '"'), b';');
    let wb = read_csv(b"x;y\n1,5;2", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 1, 1), Value::Number(2.0));
    assert!(read_csv(b"a", &CsvOptions { delimiter: b'"', ..Default::default() }).is_err());
}

#[test]
fn writing() {
    let mut wb = Workbook::new();
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(1, 1), Value::text("a,b"));
    s.set_value(CellRef::new(1, 2), Value::text("q\"t"));
    s.set_value(CellRef::new(2, 1), Value::Number(0.1));
    s.set_value(CellRef::new(2, 2), Value::Bool(false));
    s.set_value(CellRef::new(3, 3), Value::text("x\ny"));
    s.set_value(CellRef::new(3, 1), Value::Error(gridcraft_core::CellError::Div0));
    wb.sheets = vec![std::sync::Arc::new(s)];
    let out = String::from_utf8(write_csv(wb.sheet(0).unwrap(), &wb, b',')).unwrap();
    assert_eq!(out, "\"a,b\",\"q\"\"t\",\r\n0.1,FALSE,\r\n#DIV/0!,,\"x\ny\"\r\n");
    let semi = String::from_utf8(write_csv(wb.sheet(0).unwrap(), &wb, b';')).unwrap();
    assert!(semi.starts_with("a,b;"));
    assert!(write_csv(&Sheet::new("E"), &wb, b',').is_empty());
    // Round trip.
    let back = read_csv(out.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&back, 0, 0), Value::text("a,b"));
    assert_eq!(v(&back, 2, 2), Value::text("x\ny"));
}

#[test]
fn sniffing_formats() {
    assert_eq!(sniff(b"PK\x03\x04rest"), Format::Xlsx);
    assert_eq!(sniff(b"a,b\n1,2"), Format::Csv);
    // An Excel password-protected file is a CFB/OLE2 compound file with an EncryptedPackage
    // stream; any other compound file is a legacy binary (.xls), not an encrypted workbook.
    assert_eq!(sniff(&crate::cfb::tests::build(&["EncryptionInfo", "EncryptedPackage"])), Format::Encrypted);
    assert_eq!(sniff(&crate::cfb::tests::build(&["Workbook"])), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0]), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0, 1, 2]), Format::Unknown);
    assert_eq!(sniff(b""), Format::Unknown);
    assert_eq!(sniff(&[0xFF, 0xFE, b'a', 0]), Format::Csv);
}

#[test]
fn hostile_csv() {
    let _ = read_csv(&[0xFF; 1000], &CsvOptions::default()).unwrap();
    let _ = read_csv(b"\"unterminated,quote\n1,2", &CsvOptions::default()).unwrap();
    let wide = ",".repeat(20_000);
    let wb = read_csv(format!("{wide}x").as_bytes(), &CsvOptions::default()).unwrap();
    assert!(wb.sheet(0).unwrap().cells.is_empty(), "fields past column XFD are dropped");
}
