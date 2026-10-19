use gridcraft_core::{CellRef, DateSystem, Value};
use gridcraft_model::{CsvEncoding, Sheet, Workbook};

use crate::csv::{decode_text, decode_text_as, sniff_delimiter};
use crate::{CsvOptions, Format, IoError, read_csv, sniff, write_csv, write_csv_encoded, write_csv_limited};

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

/// A CP932 (Shift-JIS) CSV built in code — `日本語,漢字,ｶﾅ,"a,b"` — never a binary fixture.
fn japanese_cp932() -> Vec<u8> {
    let mut b = vec![0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]; // 日本語
    b.push(b',');
    b.extend_from_slice(&[0x8A, 0xBF, 0x8E, 0x9A]); // 漢字
    b.push(b',');
    b.extend_from_slice(&[0xB6, 0xC5]); // ｶﾅ (halfwidth katakana)
    b.push(b',');
    b.extend_from_slice(b"\"a,b\"\r\n");
    b
}

/// A sheet holding `日本語,漢字,ｶﾅ,"a,b"` in row 1.
fn japanese_sheet() -> Workbook {
    let mut wb = Workbook::new();
    let mut s = Sheet::new("S");
    for (col, text) in ["日本語", "漢字", "ｶﾅ", "a,b"].into_iter().enumerate() {
        s.set_value(CellRef::new(0, col as u32), Value::text(text));
    }
    wb.sheets = vec![std::sync::Arc::new(s)];
    wb
}

/// The Shift-JIS reading of a CSV: `Auto` picks it up from the Japanese double-byte text (the
/// fixture also holds half-width katakana, but that is not what the detection counts), an explicit
/// `ShiftJis` (or its `cp932` spelling) forces it, and `Windows1252` stays the Latin-1 reading of
/// the same bytes.
#[test]
fn shift_jis_import_under_auto_and_explicit() {
    let bytes = japanese_cp932();
    for (label, encoding) in [("auto", CsvEncoding::Auto), ("shift_jis", CsvEncoding::ShiftJis)] {
        let wb = read_csv(&bytes, &CsvOptions { encoding, ..Default::default() }).unwrap();
        for (col, text) in ["日本語", "漢字", "ｶﾅ", "a,b"].into_iter().enumerate() {
            assert_eq!(v(&wb, 0, col as u32), Value::text(text), "{label} column {col}");
        }
        assert_eq!(wb.csv_encoding, Some(CsvEncoding::ShiftJis), "{label}");
    }
    assert_eq!("cp932".parse::<CsvEncoding>(), Ok(CsvEncoding::ShiftJis));
    // The same bytes forced through Windows-1252 stay exactly that reading:
    // 0x93 0xFA 0x96 0x7B 0x8C 0xEA is “ú–{Œê, and 0xB6 0xC5 is ¶Å.
    let latin = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::Windows1252, ..Default::default() }).unwrap();
    assert_eq!(v(&latin, 0, 0), Value::text("\u{201C}\u{FA}\u{2013}{\u{152}\u{EA}"));
    assert_eq!(v(&latin, 0, 2), Value::text("\u{B6}\u{C5}"));
    assert_eq!(latin.csv_encoding, Some(CsvEncoding::Windows1252));
}

/// `Auto` must not steal a Western file: a clean CP932 decode with no hiragana, full-width katakana
/// or ideograph keeps today's Windows-1252 reading (the existing `encodings` assertions are the
/// reference). Every single CP932 byte from `0xA1` to `0xDF` is a half-width katakana and that same
/// range is `À`–`ß` and `¡`–`¿` in Windows-1252, so counting half-width katakana would read German
/// and French files as Japanese — the maintainer's `Straße,Änderung` repro.
#[test]
fn auto_does_not_steal_windows_1252() {
    assert_eq!(decode_text(&[0x80, b'a', 0xE9]), "€aé");
    let wb = read_csv(&[b'c', b'a', b'f', 0xE9, b',', 0x80], &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("café"));
    assert_eq!(v(&wb, 0, 1), Value::text("€"));
    assert_eq!(wb.csv_encoding, Some(CsvEncoding::Windows1252));
    // The exact repro from the review: 0xDF (ß) and 0xC4 (Ä) are half-width katakana in CP932.
    let strasse = [0x53, 0x74, 0x72, 0x61, 0xDF, 0x65, 0x2C, 0xC4, 0x6E, 0x64, 0x65, 0x72, 0x75, 0x6E, 0x67];
    let wb = read_csv(&strasse, &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("Straße"));
    assert_eq!(v(&wb, 0, 1), Value::text("Änderung"));
    assert_eq!(wb.csv_encoding, Some(CsvEncoding::Windows1252));
    // `Ärger`, `Größe` and `Ça va` have the same shape: high bytes that CP932 calls half-width kana.
    for (bytes, text) in [
        (&[0xC4u8, b'r', b'g', b'e', b'r'][..], "Ärger"),
        (&[b'G', b'r', 0xF6, 0xDF, b'e'], "Größe"),
        (&[0xC7, b'a', b' ', b'v', b'a'], "Ça va"),
        (&[b'a', 0xB6], "a¶"),
    ] {
        let wb = read_csv(bytes, &CsvOptions::default()).unwrap();
        assert_eq!(v(&wb, 0, 0), Value::text(text), "{bytes:02X?}");
        assert_eq!(wb.csv_encoding, Some(CsvEncoding::Windows1252), "{bytes:02X?}");
    }
    // An accented letter followed by an ASCII letter forms one valid CP932 pair (`é` is `0xE9` and
    // `n` is a valid trail byte), so a clean CP932 read can hold lone ideographs: one pair in a row
    // is not evidence of Japanese. `Prénom,März` and `Größe,Länge` are the reviewer's cases.
    for (bytes, first, second) in [
        (&[b'P', b'r', 0xE9, b'n', b'o', b'm', b',', b'M', 0xE4, b'r', b'z'][..], "Prénom", "März"),
        (&[b'G', b'r', 0xF6, 0xDF, b'e', b',', b'L', 0xE4, b'n', b'g', b'e'], "Größe", "Länge"),
    ] {
        let wb = read_csv(bytes, &CsvOptions::default()).unwrap();
        assert_eq!(v(&wb, 0, 0), Value::text(first), "{bytes:02X?}");
        assert_eq!(v(&wb, 0, 1), Value::text(second), "{bytes:02X?}");
        assert_eq!(wb.csv_encoding, Some(CsvEncoding::Windows1252), "{bytes:02X?}");
    }
}

/// A kanji-only CSV still detects as Shift-JIS: two or more consecutive double-byte characters are
/// the evidence, so a list of names or terms needs no kana.
#[test]
fn a_kanji_only_csv_still_detects_as_shift_jis() {
    // 日本語,漢字 in CP932 (three double-byte characters in a row).
    let bytes = [0x93u8, 0xFA, 0x96, 0x7B, 0x8C, 0xEA, b',', 0x8A, 0xBF, 0x8E, 0x9A];
    let wb = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("日本語"));
    assert_eq!(v(&wb, 0, 1), Value::text("漢字"));
    assert_eq!(wb.csv_encoding, Some(CsvEncoding::ShiftJis));
    // A single lone ideograph (one pair in a row) is not enough — the Western ambiguity above.
    let lone = [0x93u8, 0xFA];
    assert_eq!(read_csv(&lone, &CsvOptions::default()).unwrap().csv_encoding, Some(CsvEncoding::Windows1252));
}

/// Private-use characters come from CP932's user-defined area, so they are evidence the bytes are
/// not Japanese after all: `auto` keeps the Windows-1252 reading even with a Japanese-looking run.
#[test]
fn private_use_characters_reject_the_shift_jis_reading() {
    let mut bytes = vec![0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]; // 日本語
    bytes.extend_from_slice(&[0xF0, 0x40]); // CP932 user-defined area: U+E000
    let auto = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(auto.csv_encoding, Some(CsvEncoding::Windows1252));
    let named = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::ShiftJis, ..Default::default() }).unwrap();
    assert_eq!(v(&named, 0, 0), Value::text("日本語\u{E000}"));
}

/// Half-width katakana alone is ambiguous with Windows-1252, so `Auto` reads such a file as
/// Windows-1252 (the safer default the review suggested); naming `shift_jis` reads it correctly.
#[test]
fn halfwidth_katakana_alone_stays_windows_1252_but_an_explicit_shift_jis_reads_it() {
    let bytes = [0xB6u8, 0xC5, b',', 0xB1, 0xB2]; // ｶﾅ,ｱｲ in CP932 / ¶Å,±² in Windows-1252
    let auto = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(auto.csv_encoding, Some(CsvEncoding::Windows1252));
    assert_eq!(v(&auto, 0, 0), Value::text("\u{B6}\u{C5}"));
    let sjis = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::ShiftJis, ..Default::default() }).unwrap();
    assert_eq!(v(&sjis, 0, 0), Value::text("ｶﾅ"));
    assert_eq!(v(&sjis, 0, 1), Value::text("ｱｲ"));
    assert_eq!(sjis.csv_encoding, Some(CsvEncoding::ShiftJis));
}

/// The detection wants a clean CP932 decode: one invalid byte in an otherwise Japanese file keeps
/// it on the Windows-1252 path (name `shift_jis` to read it). Documented trade-off of the review's
/// "no errors" rule.
#[test]
fn a_damaged_shift_jis_file_is_not_detected_as_japanese() {
    let mut bytes = vec![0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]; // 日本語
    bytes.push(0x81); // a lone lead byte: not a clean CP932 decode
    let auto = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(auto.csv_encoding, Some(CsvEncoding::Windows1252));
    let named = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::ShiftJis, ..Default::default() }).unwrap();
    assert_eq!(v(&named, 0, 0), Value::text("日本語\u{FFFD}"));
}

/// Forcing UTF-8 on Shift-JIS bytes never falls back to another code page: the invalid sequences
/// become `U+FFFD` (a deliberate, documented lossy read), not silently some other text.
#[test]
fn explicit_utf8_on_shift_jis_bytes_is_deliberately_lossy() {
    let bytes = japanese_cp932();
    let (text, encoding) = decode_text_as(&bytes, CsvEncoding::Utf8);
    assert_eq!(text, String::from_utf8_lossy(&bytes));
    assert!(text.contains('\u{FFFD}') && !text.contains('日'));
    assert_eq!(encoding, Some(CsvEncoding::Utf8));
    let wb = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::Utf8, ..Default::default() }).unwrap();
    assert_ne!(v(&wb, 0, 0), Value::text("日本語"));
    assert!(v(&wb, 0, 0).display().contains('\u{FFFD}'), "{}", v(&wb, 0, 0).display());
    // A UTF-8 BOM is dropped under both UTF-8 spellings.
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice("日本語".as_bytes());
    for encoding in [CsvEncoding::Utf8, CsvEncoding::Utf8Bom] {
        let wb = read_csv(&bom, &CsvOptions { encoding, ..Default::default() }).unwrap();
        assert_eq!(v(&wb, 0, 0), Value::text("日本語"), "{encoding:?}");
    }
}

/// Export: the default stays BOM-less UTF-8 byte for byte, `Utf8Bom` puts the BOM first, and a
/// Shift-JIS export is exactly the CP932 a Japanese Excel writes — reading it back is lossless.
#[test]
fn csv_export_encodings_round_trip() {
    let wb = japanese_sheet();
    let sh = wb.sheet(0).unwrap();
    let expected = "日本語,漢字,ｶﾅ,\"a,b\"\r\n";
    let utf8 = write_csv(sh, b',', text_of(sh)).unwrap();
    assert_eq!(utf8, expected.as_bytes(), "the default export is still BOM-less UTF-8");
    assert_eq!(write_csv_encoded(sh, b',', CsvEncoding::Utf8, text_of(sh)).unwrap(), utf8);
    // `Auto` is import-only: writing it is UTF-8 without a BOM.
    assert_eq!(write_csv_encoded(sh, b',', CsvEncoding::Auto, text_of(sh)).unwrap(), utf8);
    let bom = write_csv_encoded(sh, b',', CsvEncoding::Utf8Bom, text_of(sh)).unwrap();
    assert!(bom.starts_with(&[0xEF, 0xBB, 0xBF]), "{bom:?}");
    assert_eq!(bom.get(3..), Some(utf8.as_slice()), "the bytes after the BOM are UTF-8");
    let shift_jis = write_csv_encoded(sh, b',', CsvEncoding::ShiftJis, text_of(sh)).unwrap();
    assert_eq!(shift_jis, japanese_cp932());
    let back = read_csv(&shift_jis, &CsvOptions::default()).unwrap();
    for (col, text) in ["日本語", "漢字", "ｶﾅ", "a,b"].into_iter().enumerate() {
        assert_eq!(v(&back, 0, col as u32), Value::text(text), "column {col}");
    }
    assert_eq!(back.csv_encoding, Some(CsvEncoding::ShiftJis));
    assert_eq!(write_csv_encoded(sh, b',', CsvEncoding::Windows1252, text_of(sh)).unwrap(), b"???,??,??,\"a,b\"\r\n");
}

fn utf16le_with_bom(text: &str) -> Vec<u8> {
    [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect()
}

/// Excel's *Unicode Text* is UTF-16LE with a BOM. An Arabic file in it is remembered as UTF-16LE
/// and written back byte for byte, whether detected or named on open.
#[test]
fn utf16le_arabic_round_trips() {
    let bytes = utf16le_with_bom("مرحبا,عالم\r\n١٢٣,\"أ,ب\"\r\n");
    let wb = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(wb.csv_encoding, Some(CsvEncoding::Utf16Le));
    assert_eq!((v(&wb, 0, 0), v(&wb, 0, 1), v(&wb, 1, 1)), (Value::text("مرحبا"), Value::text("عالم"), Value::text("أ,ب")));
    let sh = wb.sheet(0).unwrap();
    assert_eq!(write_csv_encoded(sh, b',', CsvEncoding::Utf16Le, text_of(sh)).unwrap(), bytes);
    let named = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::Utf16Le, ..Default::default() }).unwrap();
    assert_eq!(v(&named, 0, 0), Value::text("مرحبا"));
    let without_bom = bytes.get(2..).unwrap();
    assert_eq!(decode_text_as(without_bom, CsvEncoding::Utf16Le), ("مرحبا,عالم\r\n١٢٣,\"أ,ب\"\r\n".to_string(), Some(CsvEncoding::Utf16Le)));
    for spelling in ["utf-16le", "UTF-16", "unicode"] {
        assert_eq!(spelling.parse::<CsvEncoding>(), Ok(CsvEncoding::Utf16Le), "{spelling}");
    }
    assert_eq!(CsvEncoding::Utf16Le.as_str(), "utf-16le");
    let mut truncated = utf16le_with_bom("مرحبا");
    truncated.push(0x41);
    assert_eq!(decode_text(&truncated), "مرحبا\u{FFFD}", "a dangling last byte is a replacement character, not dropped");
}

/// UTF-16 doubles ASCII text, so an export under the cap in UTF-8 can pass it in UTF-16; it is
/// refused, not written.
#[test]
fn utf16le_export_respects_the_size_cap() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), Value::text("a".repeat(600)));
    let sh = wb.sheet(0).unwrap();
    assert!(write_csv_limited(sh, b',', 1000, text_of(sh)).is_ok());
    assert!(matches!(crate::csv::write_csv_in(sh, b',', CsvEncoding::Utf16Le, 1000, text_of(sh)), Err(IoError::TooLarge(_))));
}

/// Microsoft CP932 has a byte for the wave dash and the double vertical line that WHATWG
/// `shift_jis` only maps when decoding: Excel writes `1〜5` as `1 0x81 0x60 5`, so they are encoded
/// through their CP932 equivalents instead of becoming `?`.
#[test]
fn cp932_symbols_whatwg_only_maps_when_decoding_still_export() {
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(0, 0), Value::text("1〜5"));
    s.set_value(CellRef::new(0, 1), Value::text("a‖b"));
    let bytes = write_csv_encoded(&s, b',', CsvEncoding::ShiftJis, text_of(&s)).unwrap();
    assert_eq!(bytes, b"1\x81\x60\x35,a\x81\x61\x62\r\n", "{bytes:?}");
    // The bytes are the ones Excel writes; CP932-decoding them gives the equivalents the code page
    // maps those bytes to (`～` U+FF5E, `∥` U+2225), exactly as Excel and encoding_rs do.
    let back = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::ShiftJis, ..Default::default() }).unwrap();
    assert_eq!(v(&back, 0, 0), Value::text("1～5"));
    assert_eq!(v(&back, 0, 1), Value::text("a∥b"));
}

/// A character the code page cannot map becomes `?`, as Word and Excel write it — never encoding_rs'
/// default HTML numeric character reference, which would corrupt the field — and the row stays put.
#[test]
fn unmappable_characters_become_question_marks_not_entities() {
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(0, 0), Value::text("Ǎ😀x"));
    s.set_value(CellRef::new(0, 1), Value::text("ok"));
    let bytes = write_csv_encoded(&s, b',', CsvEncoding::ShiftJis, text_of(&s)).unwrap();
    assert_eq!(bytes, b"??x,ok\r\n");
    assert!(!bytes.windows(2).any(|w| w == b"&#"), "never an HTML character reference: {bytes:?}");
    let back = read_csv(&bytes, &CsvOptions { encoding: CsvEncoding::ShiftJis, ..Default::default() }).unwrap();
    assert_eq!(v(&back, 0, 0), Value::text("??x"));
    assert_eq!(v(&back, 0, 1), Value::text("ok"));
    // More than one encode chunk, in ASCII and in multi-byte text: the chunked encoder keeps
    // every character and every position.
    let long = "a".repeat(20_000);
    let mut wide = Sheet::new("W");
    wide.set_value(CellRef::new(0, 0), Value::text(long.as_str()));
    assert_eq!(write_csv_encoded(&wide, b',', CsvEncoding::ShiftJis, text_of(&wide)).unwrap(), format!("{long}\r\n").as_bytes());
    let repeated = "日本語".repeat(4000);
    let mut jp = Sheet::new("J");
    jp.set_value(CellRef::new(0, 0), Value::text(repeated.as_str()));
    let cp932: Vec<u8> = std::iter::repeat_n([0x93u8, 0xFA, 0x96, 0x7B, 0x8C, 0xEA], 4000).flatten().chain(*b"\r\n").collect();
    assert_eq!(write_csv_encoded(&jp, b',', CsvEncoding::ShiftJis, text_of(&jp)).unwrap(), cp932);
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
    assert_eq!(out, "\"a,b\",\"q\"\"t\",\r\n0.1,FALSE,\r\n#DIV/0!,,\"x\ny\"\r\n");
    let semi = String::from_utf8(write_csv(sh, b';', raw).unwrap()).unwrap();
    assert!(semi.starts_with("a,b;"));
    assert!(write_csv(&Sheet::new("E"), b',', raw).unwrap().is_empty());
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
    // A tab is one byte, like a comma.
    let tabs = write_csv_limited(&s, b'\t', 14, text_of(&s)).unwrap();
    assert_eq!(tabs.len(), 14);
}
