use gridcraft_core::{CellRef, DateSystem, MAX_COLS, Value};
use gridcraft_model::{Sheet, Workbook};

use crate::csv::{decode_text, parse_records, sniff_delimiter};
use crate::{CsvOptions, Format, IoError, read_csv, sniff, write_csv, write_csv_limited};

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
    assert_eq!(v(&wb, 0, 8), Value::text("'007"));
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
fn windows_1256_table_matches_the_unicode_mapping() {
    let at = |b: u8| crate::csv::CP1256.get(usize::from(b - 0x80)).and_then(|&c| char::from_u32(c));
    let expected = [
        (0x80, '€'),
        (0x81, 'پ'),
        (0x8D, 'چ'),
        (0x98, 'ک'),
        (0x9D, '\u{200C}'),
        (0xA1, '،'),
        (0xBA, '؛'),
        (0xBF, '؟'),
        (0xC7, 'ا'),
        (0xC8, 'ب'),
        (0xDC, 'ـ'),
        (0xE1, 'ل'),
        (0xE3, 'م'),
        (0xE9, 'é'),
        (0xFD, '\u{200E}'),
        (0xFE, '\u{200F}'),
        (0xFF, 'ے'),
    ];
    for (b, c) in expected {
        assert_eq!(at(b), Some(c), "0x{b:02X}");
    }
}

#[test]
fn arabic_csv_in_windows_1256_reads_as_arabic() {
    let bytes = [
        0xC7, 0xE1, 0xC7, 0xD3, 0xE3, b',', 0xC7, 0xE1, 0xDE, 0xED, 0xE3, 0xC9, b'\n', 0xE3, 0xD1, 0xCD, 0xC8, 0xC7, b' ', b'1', b'2', b'3', 0xA1,
        b' ', b'4', b'2', 0xBF, b',', 0xC8, 0xED, 0xC7, 0xE4, 0xC7, 0xCA, b' ', b'7', b'\n',
    ];
    let wb = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("الاسم"));
    assert_eq!(v(&wb, 0, 1), Value::text("القيمة"));
    assert_eq!(v(&wb, 1, 0), Value::text("مرحبا 123، 42؟"));
    assert_eq!(v(&wb, 1, 1), Value::text("بيانات 7"));
}

fn reads_as_arabic(bytes: &[u8]) -> bool {
    decode_text(bytes).chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
}

#[test]
fn western_1252_text_stays_latin() {
    let french = [
        b'C', b'r', 0xE8, b'm', b'e', b' ', b'b', b'r', 0xFB, b'l', 0xE9, b'e', b',', b' ', 0xE0, b' ', b'c', 0xF4, b't', 0xE9, b',', b' ', 0xE9,
        b't', 0xE9,
    ];
    let german = [
        b'G', b'r', 0xF6, 0xDF, b'e', b',', b' ', 0xDC, b'b', b'e', b'r', b'm', b'a', 0xDF, b',', b' ', b'F', b'u', 0xDF, b'g', 0xE4, b'n', b'g',
        b'e', b'r',
    ];
    assert_eq!(decode_text(&french), "Crème brûlée, à côté, été");
    assert_eq!(decode_text(&german), "Größe, Übermaß, Fußgänger");
    let portuguese = [
        b'A', 0xC7, 0xC3, b'O', b',', b' ', b'i', b'n', b'f', b'o', b'r', b'm', b'a', 0xE7, 0xE3, b'o', b',', b' ', b's', b'i', b't', b'u', b'a',
        0xE7, 0xE3, b'o', b',', b' ', b'P', 0xE3, b'o',
    ];
    assert_eq!(decode_text(&portuguese), "AÇÃO, informação, situação, Pão");
    let nordic = [
        0xC6, b'r', 0xF8, b',', 0xD8, b's', b't', b',', 0xC5, b's', b'e', b',', 0xDE, 0xF3, b'r', 0xF0, b'u', b'r', b',', b'G', b'u', 0xF0, b'r',
        0xFA, b'n', b',', 0xC4, 0xD6, 0xDC,
    ];
    assert_eq!(decode_text(&nordic), "Ærø,Øst,Åse,Þórður,Guðrún,ÄÖÜ");
    let ordinals = [b'1', 0xBA, b',', b'2', 0xBA, b',', b'3', 0xBA, b'\n', b'1', 0xAA, b',', b'2', 0xAA];
    assert_eq!(decode_text(&ordinals), "1º,2º,3º\n1ª,2ª");
    let inside_words = [b'K', 0xE4, b's', b'e', b',', b'B', 0xE4, b'c', b'k', b'e', b'r'];
    assert_eq!(decode_text(&inside_words), "Käse,Bäcker");
}

#[test]
fn short_arabic_words_read_as_arabic() {
    assert_eq!(decode_text(&[0xDD, 0xED, b',', 0xE3, 0xE4, b',', 0xDA, 0xE4, b'\n']), "في,من,عن\n");
    assert_eq!(decode_text(&[0xE4, 0xDA, 0xE3, b',', 0xE1, 0xC7, b'\n', 0xE1, 0xC7, b',', 0xE4, 0xDA, 0xE3, b'\n']), "نعم,لا\nلا,نعم\n");
    assert_eq!(decode_text(&[0x98, 0xCA, 0xC7, 0xC8, b' ', 0x81, 0xC7, 0xD1, 0xD3, 0xED]), "کتاب پارسي");
    let with_marks = [
        0xDF, 0xCA, 0xC7, 0xC8, 0x9D, 0xE5, 0xC7, b',', 0xCE, 0xC7, 0xE4, 0xE5, 0x9D, 0xE5, 0xC7, b',', 0xE3, 0xCD, 0xE3, 0xCF, 0xFE, b',', 0xDA,
        0xE1, 0xED, 0xFE,
    ];
    assert_eq!(decode_text(&with_marks), "كتاب\u{200C}ها,خانه\u{200C}ها,محمد\u{200F},علي\u{200F}");
}

#[test]
fn hebrew_1255_bytes_are_not_read_as_arabic() {
    let plain = [0xF9, 0xEC, 0xE5, 0xED, b',', 0xF2, 0xE5, 0xEC, 0xED];
    let pointed = [0xF9, 0xC8, 0xD1, 0xEC, 0xE5, 0xC9, 0xED, b' ', 0xF2, 0xE5, 0xC9, 0xEC, 0xC8, 0xED];
    let maqaf = [0xFA, 0xEC, 0xCE, 0xE0, 0xE1, 0xE9, 0xE1, b',', 0xE1, 0xEF, 0xCE, 0xE2, 0xE5, 0xF8, 0xE9, 0xE5, 0xEF];
    let gershayim = [0xF6, 0xE4, 0xD8, 0xEC, b',', 0xE3, 0xD8, 0xF8];
    for bytes in [&plain[..], &pointed, &maqaf, &gershayim] {
        assert!(!reads_as_arabic(bytes), "{:?}", decode_text(bytes));
    }
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
    // The workbook remembers the separator it was read with, sniffed or given.
    assert_eq!(wb.csv_delimiter, Some(b';'));
    assert_eq!(read_csv(b"a\tb", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap().csv_delimiter, Some(b'\t'));
    assert_eq!(read_csv(b"a;b", &CsvOptions::default()).unwrap().csv_delimiter, Some(b','));
    assert_eq!(Workbook::new().csv_delimiter, None);
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
    // The caller supplies each field's text; quoting and line ends are the writer's.
    let sh = wb.sheet(0).unwrap();
    let raw = |c: CellRef| match sh.value(c) {
        Value::Empty => String::new(),
        Value::Number(n) => gridcraft_core::number_to_text(n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => String::new(),
    };
    let out = String::from_utf8(write_csv(sh, b',', raw).unwrap()).unwrap();
    // The first cell is B2: the file starts at A1, so the leading empty row and column stay.
    assert_eq!(out, ",,,\r\n,\"a,b\",\"q\"\"t\",\r\n,0.1,FALSE,\r\n,#DIV/0!,,\"x\ny\"\r\n");
    let semi = String::from_utf8(write_csv(sh, b';', raw).unwrap()).unwrap();
    assert!(semi.starts_with(";;;\r\n;a,b;"));
    assert!(write_csv(&Sheet::new("E"), b',', raw).unwrap().is_empty());
    // Round trip.
    let back = read_csv(out.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&back, 1, 1), Value::text("a,b"));
    assert_eq!(v(&back, 3, 3), Value::text("x\ny"));
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

/// A sheet with `a` in A1 and `b` in `corner`.
fn two_cells(corner: (u32, u32)) -> Sheet {
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(0, 0), Value::text("a"));
    s.set_value(CellRef::new(corner.0, corner.1), Value::text("b"));
    s
}

fn text_of(sh: &Sheet) -> impl FnMut(CellRef) -> String + '_ {
    |c| match sh.value(c) {
        Value::Text(t) => t.to_string(),
        _ => String::new(),
    }
}

#[test]
fn two_far_apart_cells_are_refused_before_anything_is_allocated() {
    // A1 and XFD1048576 span the whole grid: 16 GiB of commas. Refused at once, with the numbers.
    let s = two_cells((gridcraft_core::MAX_ROWS - 1, gridcraft_core::MAX_COLS - 1));
    let err = write_csv(&s, b',', text_of(&s)).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("1048576 rows by 16384 columns") && msg.contains("512 MiB"), "{msg}");
    // A single row or column across the grid is refused too when it is over the cap.
    let wide = two_cells((gridcraft_core::MAX_ROWS - 1, 0));
    assert!(write_csv_limited(&wide, b',', 1 << 20, text_of(&wide)).is_err());
}

#[test]
fn the_cap_counts_the_bytes_the_cells_hold_and_accepted_exports_keep_every_position() {
    // 3 rows x 3 columns, 14 bytes: accepted at the cap, refused one byte under.
    let s = two_cells((2, 2));
    let ok = write_csv_limited(&s, b',', 14, text_of(&s)).unwrap();
    assert_eq!(String::from_utf8(ok).unwrap(), "a,,\r\n,,\r\n,,b\r\n");
    assert!(write_csv_limited(&s, b',', 13, text_of(&s)).is_err());
    // Rows that pass the lower bound but hold long text stop as soon as the cap is passed.
    let mut long = Sheet::new("L");
    for r in 0..50u32 {
        long.set_value(CellRef::new(r, 0), Value::text("x".repeat(100)));
    }
    let err = write_csv_limited(&long, b',', 1000, text_of(&long)).unwrap_err();
    assert!(err.to_string().contains("stopped at row"), "{err}");
    assert!(write_csv_limited(&long, b',', 50 * 102, text_of(&long)).is_ok());
    // Within one row the cap is checked before each field is written: a wide row of long and
    // quote-heavy fields is refused at the field that crosses it, without asking for the rest.
    let mut wide = Sheet::new("W");
    for c in 0..10u32 {
        wide.set_value(CellRef::new(0, c), Value::text("\"".repeat(100)));
    }
    let mut asked = 0;
    let err = write_csv_limited(&wide, b',', 500, |c| {
        asked += 1;
        text_of(&wide)(c)
    })
    .unwrap_err();
    assert!(err.to_string().contains("stopped at row 1"), "{err}");
    assert_eq!(asked, 3, "each field is 202 bytes; the third passes 500");
    // Exact boundary: a quoted field of 202 bytes plus CRLF fits in 204 and not in 203.
    let mut one = Sheet::new("O");
    one.set_value(CellRef::new(0, 0), Value::text("\"".repeat(100)));
    assert_eq!(write_csv_limited(&one, b',', 204, text_of(&one)).unwrap().len(), 204);
    assert!(write_csv_limited(&one, b',', 203, text_of(&one)).is_err());
    // A tab is one byte, like a comma.
    let tabs = write_csv_limited(&s, b'\t', 14, text_of(&s)).unwrap();
    assert_eq!(tabs.len(), 14);
}

#[test]
fn parser_stops_collecting_fields_at_max_cols() {
    let extra = 50;
    // MAX_COLS + extra fields on the first row (the last one filled), then a short second row.
    let mut text = ",".repeat((MAX_COLS + extra) as usize - 1);
    text.push_str("end\nx,y\n");
    let mut lens = vec![];
    parse_records(&text, ',', '"', |rec| {
        lens.push(rec.len());
        true
    });
    assert_eq!(lens, vec![MAX_COLS as usize, 2]);
    // An unterminated last row is capped the same way.
    let mut lens = vec![];
    parse_records(&",".repeat(MAX_COLS as usize + extra as usize), ',', '"', |rec| {
        lens.push(rec.len());
        true
    });
    assert_eq!(lens, vec![MAX_COLS as usize]);
}

#[test]
fn leading_apostrophe_is_data() {
    let wb = read_csv(b"'quoted' text,it's,'123\n", &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("'quoted' text"));
    assert_eq!(v(&wb, 0, 1), Value::text("it's"));
    assert_eq!(v(&wb, 0, 2), Value::text("'123"));
    // A cell whose text starts with an apostrophe survives a CSV round trip.
    let mut sheet = Sheet::new("Sheet1");
    sheet.set_value(CellRef::new(0, 0), Value::text("'abc"));
    let bytes = write_csv(&sheet, b',', |c| match sheet.value(c) {
        Value::Text(t) => t.to_string(),
        _ => String::new(),
    })
    .unwrap();
    let back = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(v(&back, 0, 0), Value::text("'abc"));
}
