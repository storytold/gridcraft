//! Text layout detection and reading (#2553): delimiters by complete rows and clean cells, the
//! decimal separator from the values, and the completeness report.

use gridcraft_core::{CellRef, Value};
use gridcraft_model::Workbook;

use crate::text_layout::{Decimal, DecimalBasis, Evidence, Split, TextKind, comma_to_point, detect, evidence, split_records};
use crate::{CsvOptions, read_text};

fn v(wb: &Workbook, r: u32, c: u32) -> Value {
    wb.sheet(0).unwrap().value(CellRef::new(r, c))
}

fn fmt(wb: &Workbook, r: u32, c: u32) -> String {
    let sh = wb.sheet(0).unwrap();
    wb.styles.get(sh.style_id(CellRef::new(r, c))).num_fmt.as_str().to_string()
}

/// Detection and number reading on, as File › Open with `delimiter: "auto"` does.
fn auto(kind: TextKind) -> CsvOptions {
    CsvOptions { delimiter: 0, detect_any: true, start_row: None, decimal: None, kind, ..Default::default() }
}

fn kind_of(name: &str) -> TextKind {
    match name.rsplit('.').next() {
        Some("csv") => TextKind::Csv,
        Some("prn") => TextKind::Prn,
        Some("txt" | "tsv") => TextKind::Tab,
        _ => TextKind::Other,
    }
}

#[test]
fn detects_the_split_and_the_decimal_separator() {
    use Decimal::{Comma, Point};
    let d = Split::delimiter;
    let spaces = Split::Delimited { delimiters: vec![" ".into()], consecutive: true };
    // (file name, text, split, decimal separator when the file shows it)
    let cases: Vec<(&str, &str, Split, Option<Decimal>)> = vec![
        ("de_semi_header.csv", "Name;Wert;Menge\r\nA;1,5;3\r\nB;2,25;4\r\nC;10,75;5\r\n", d(";"), Some(Comma)),
        // `,` and `;` both give the same field count in every row; only the cells decide.
        ("de_semi_noheader.txt", "A;1,5;3\r\nB;2,25;4\r\nC;10,75;5\r\n", d(";"), Some(Comma)),
        ("de_semi_2col.txt", "A;1,5\r\nB;2,25\r\nC;10,75\r\n", d(";"), Some(Comma)),
        // Only numbers: `,` splits them into more fields, but only by skipping the header.
        ("de_numbers_only.csv", "Menge;Preis\r\n1,5;2,30\r\n2,5;3,10\r\n", d(";"), Some(Comma)),
        ("de_numbers_only.txt", "Menge;Preis;Rabatt\r\n1,5;2,30;0,5\r\n2,5;3,10;1,25\r\n12,5;4,75;0\r\n", d(";"), Some(Comma)),
        ("de_semi_thousands.csv", "Artikel;Umsatz\r\nX;1.234,50\r\nY;12.345,00\r\nZ;999,99\r\n", d(";"), Some(Comma)),
        ("de_title_preamble.csv", "Umsatzbericht Oktober 2026\r\n\r\nName;Wert\r\nA;1,5\r\nB;2,25\r\n", d(";"), Some(Comma)),
        ("en_comma.csv", "Name,Value,Qty\r\nA,1.5,3\r\nB,2.25,4\r\n", d(","), Some(Point)),
        ("en_comma_quoted_thousands.csv", "Name,Value\r\nA,\"1,234.50\"\r\nB,99.00\r\n", d(","), Some(Point)),
        ("en_comma_unquoted_thousands.csv", "Name,Value\r\nA,1,234.50\r\nB,99.00\r\nC,12.00\r\n", d(","), Some(Point)),
        ("en_semi_decimals.csv", "Name;Value\r\nA;1.5\r\nB;2.25\r\nC;1.234\r\n", d(";"), Some(Point)),
        ("en_semi_mixed_evidence.csv", "Name;Value\r\nA;1.234\r\nB;0.5\r\n", d(";"), Some(Point)),
        ("semi_ambiguous.csv", "Name;Value\r\nA;1.234\r\nB;12.345\r\n", d(";"), Some(Comma)),
        ("comma_ambiguous.csv", "Name,Value\r\nA,1.234\r\nB,12.345\r\n", d(","), Some(Point)),
        ("comma_quoted_decimal_comma.csv", "Name,Value\r\nA,\"1,5\"\r\nB,\"1.234,5\"\r\n", d(","), Some(Comma)),
        ("fr_space_thousands.csv", "Nom;Valeur\r\nA;1 234,5\r\nB;12 345,25\r\n", d(";"), Some(Comma)),
        ("de_dates_semi.csv", "Datum;Betrag\r\n25.10.2026;1.234,50\r\n01.11.2026;99,00\r\n", d(";"), Some(Comma)),
        ("tab_commas_in_text.txt", "Name\tAddress\r\nA\tStreet 1, City\r\nB\tRoad 2, Town\r\n", d("\t"), None),
        ("tab_commas_noheader.txt", "A\tStreet 1, City\r\nB\tRoad 2, Town\r\n", d("\t"), None),
        ("semi_text_with_commas.csv", "Name;Comment\r\nA;Hello, world\r\nB;Yes, no\r\n", d(";"), None),
        ("comma_text_with_semis.csv", "id,note\r\n1,a;b\r\n2,c;d\r\n", d(","), None),
        ("trailing_semis.csv", "a;b;\r\n1;2;\r\n3;4;\r\n", d(";"), None),
        ("mixed_quotes.csv", "id,text\r\n1,\"x,y\"\r\n2,\"say \"\"hi\"\"\"\r\n", d(","), None),
        ("city_country.csv", "Berlin, Germany\r\nParis, France\r\nRome, Italy\r\n", d(","), None),
        // Values that hold `:`, `/`, `€` or a decimal comma are never split there.
        ("one_col_times.txt", "12:30:00\r\n08:15:30\r\n23:59:59\r\n", Split::Lines, None),
        ("one_col_dates.txt", "04/03/2024\r\n12/31/2023\r\n01/01/2025\r\n", Split::Lines, None),
        ("one_col_euro.txt", "12 €\r\n7,50 €\r\n1.200 €\r\n", Split::Lines, Some(Comma)),
        ("one_col_en_thousands.txt", "1,234\r\n12,345\r\n123,456\r\n", Split::Lines, None),
        ("one_col_de_numbers.txt", "1.234,5\r\n12.345,25\r\n7,5\r\n", Split::Lines, Some(Comma)),
        ("one_col_text.txt", "hello world\r\nfoo bar\r\n", Split::Lines, None),
        ("pipe.txt", "id|name|price\r\n1|Apfel|1.5\r\n2|Birne|2.25\r\n", d("|"), Some(Point)),
        ("dpipe.txt", "id||name||price\r\n1||Apfel||1.5\r\n2||Birne||2.25\r\n", d("||"), Some(Point)),
        ("hashpipe.txt", "id#|#name#|#price\r\n1#|#Apfel#|#1.5\r\n2#|#Birne#|#2.25\r\n", d("#|#"), Some(Point)),
        ("colon.txt", "Name:Qty:Code\r\nApfel:3:A1\r\nBirne:12:B2\r\n", d(":"), None),
        // Seventeen other symbols in the header come first, but only `§` is on every line.
        ("symbols_in_header.txt", "a^~=:<>¶©®±×÷¬¤«»?b§c\r\n1§2\r\n3§4\r\n5§6\r\n", d("§"), None),
        ("aligned_spaces.txt", "Name    Qty  Code\r\nApfel   3    A1\r\nBirne   12   B2\r\n", spaces, None),
        (
            "fixed_internal_spaces.prn",
            "plain text   12   ok\r\nx            3    no\r\nlong name    456  ok\r\n",
            Split::FixedWidth(vec![13, 18]),
            None,
        ),
        ("excel_like.prn", "plain     a,b       1234,5\r\nr2        d2\r\n", Split::FixedWidth(vec![10, 20]), Some(Comma)),
        ("numeric_space_separated.txt", "1 2 3\r\n4 5 6\r\n7 8 9\r\n", d(" "), None),
    ];
    for (name, text, split, decimal) in cases {
        let (_, report) = read_text(text.as_bytes(), &auto(kind_of(name))).unwrap();
        assert_eq!(report.split, split, "{name}: split");
        assert!(report.detected, "{name}");
        if let Some(d) = decimal {
            assert_eq!(report.numbers.decimal, d, "{name}: decimal");
        }
    }
}

#[test]
fn every_delimiter_of_the_comparison_files() {
    // The 5 × 4 table of the delimiter comparison (#1446): a quoted field holding the delimiter,
    // an empty field, doubled quotes, and a row of other punctuation.
    let others = ",;:=|^~#/";
    for (d, found) in [
        (",", true),
        (";", true),
        ("\t", true),
        ("|", true),
        (":", true),
        ("=", true),
        ("^", true),
        ("~", true),
        ("#", true),
        ("/", true),
        ("§", true),
        ("¦", true),
        ("•", true),
        ("\u{1f}", true),
        ("||", true),
        ("#|#", true),
        // Never guessed: amounts carry `€`, and a single space between words is running text.
        ("€", false),
        (" ", false),
    ] {
        let rest: String = others.chars().filter(|c| !d.contains(*c)).collect();
        let code = if d == " " { "K3" } else { "K 3" };
        let rows = [
            vec!["Name".to_string(), "Qty".into(), "Code".into(), "Note".into()],
            vec!["Apfel".into(), "3".into(), "A1".into(), "rot".into()],
            vec!["Birne".into(), "12".into(), format!("\"B{d}2\""), "gelb".into()],
            vec!["Kiwi".into(), String::new(), code.into(), "\"sagt \"\"hi\"\"\"".into()],
            vec!["Pflaume".into(), "7".into(), rest, "ende".into()],
        ];
        let text: String = rows.iter().map(|r| r.join(d) + "\r\n").collect();
        let (wb, report) = read_text(text.as_bytes(), &auto(TextKind::Tab)).unwrap();
        if found {
            assert_eq!(report.split, Split::delimiter(d), "{d:?}");
            assert_eq!((report.fields, report.complete, report.rows), (4, 5, 5), "{d:?}");
            assert_eq!(v(&wb, 2, 2), Value::text(format!("B{d}2")), "{d:?}");
            assert_eq!(v(&wb, 3, 3), Value::text("sagt \"hi\""), "{d:?}");
        } else {
            assert_eq!(report.split, Split::Lines, "{d:?}");
        }
    }
}

#[test]
fn numbers_read_with_the_detected_decimal_comma() {
    let text = "Artikel;Umsatz;Anteil\r\nX;1.234,50;12,5 %\r\nY;-7,25;0,5\r\nZ;999;1,5E3\r\n";
    let (wb, report) = read_text(text.as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.numbers.decimal, Decimal::Comma);
    assert_eq!(report.numbers.basis, DecimalBasis::Values);
    assert_eq!(v(&wb, 1, 1), Value::Number(1234.5));
    assert_eq!(fmt(&wb, 1, 1), "#,##0.00");
    assert_eq!(v(&wb, 2, 1), Value::Number(-7.25));
    assert_eq!(v(&wb, 2, 2), Value::Number(0.5));
    assert_eq!(v(&wb, 3, 1), Value::Number(999.0));
    // German Excel reads `1,5E3` as 1500, opened, imported or typed (measured), not 15000.
    assert_eq!(v(&wb, 3, 2), Value::Number(1500.0));
    // Saving writes the decimal comma back.
    assert!(wb.csv_decimal_comma);
    assert_eq!(wb.csv_delimiter, Some(b';'));
}

#[test]
fn punctuation_inside_text_is_no_delimiter() {
    // Each line cut once into text and leftovers, or into leftovers only: one column.
    let urls: String = (0..20).map(|i| format!("https://example.com/page{i}\r\n")).collect();
    let subjects = "Re: question\r\nRe: answer\r\nFwd: report\r\nRe: Re: again\r\n";
    let paths = "C:\\Users\\me\\file0.txt\r\nD:\\data\\b.csv\r\nC:\\x\\y.txt\r\n";
    let orders = "Order #1000\r\nOrder #1001\r\nOrder #1002\r\n";
    for text in [format!("url\r\n{urls}"), urls.clone(), subjects.into(), paths.into(), orders.into()] {
        let (wb, report) = read_text(text.as_bytes(), &auto(TextKind::Tab)).unwrap();
        assert_eq!(report.split, Split::Lines, "{text}");
        assert_eq!(v(&wb, 0, 1), Value::Empty, "{text}");
    }
    // Typed values on both sides still split at a symbol.
    let (_, report) = read_text("1§2\r\n3§4\r\n5§6\r\n".as_bytes(), &auto(TextKind::Tab)).unwrap();
    assert_eq!(report.split, Split::delimiter("§"));
}

#[test]
fn a_decimal_comma_amount_that_is_no_number_keeps_its_text() {
    let (wb, report) = read_text("a;b\r\n1,5;1.234,50 €\r\n2,5;€1,5\r\n3,5;(1,5\r\n4,5;--1,5\r\n".as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.numbers.decimal, Decimal::Comma, "{report:?}");
    assert_eq!(v(&wb, 1, 0), Value::Number(1.5));
    // Amounts with a currency sign are numbers that show it.
    assert_eq!((v(&wb, 1, 1), fmt(&wb, 1, 1)), (Value::Number(1234.5), "#,##0.00 \"€\"".to_string()));
    assert_eq!((v(&wb, 2, 1), fmt(&wb, 2, 1)), (Value::Number(1.5), "\"€\"#,##0.0".to_string()));
    // The reader's rewrite (`(1.5`) never reaches the cell.
    assert_eq!(v(&wb, 3, 1), Value::text("(1,5"));
    assert_eq!(v(&wb, 4, 1), Value::text("--1,5"));
}

#[test]
fn a_value_that_is_a_number_only_with_the_other_separator_stays_text() {
    // Under a decimal comma, `0.125` and `1,234.5` are no numbers (Excel keeps them as text).
    let (wb, _) = read_text(b"a;b\r\n1,5;0.125\r\n2,5;1,234.5\r\n", &auto(TextKind::Csv)).unwrap();
    assert_eq!(v(&wb, 1, 0), Value::Number(1.5));
    // Column B has only English evidence: it is read with a point.
    assert_eq!(v(&wb, 1, 1), Value::Number(0.125));
    assert_eq!(v(&wb, 2, 1), Value::Number(1234.5));
    let (wb, report) = read_text(b"a;b\r\n1,5;x\r\n2,5;0.125\r\n", &CsvOptions { decimal: Some(Decimal::Comma), ..auto(TextKind::Csv) }).unwrap();
    assert_eq!(report.numbers.basis, DecimalBasis::Given);
    assert_eq!(v(&wb, 2, 1), Value::text("0.125"));
}

#[test]
fn english_decimals_in_a_semicolon_file_keep_the_point() {
    let (wb, report) = read_text(b"Name;Value\r\nA;1.234\r\nB;0.5\r\n", &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.numbers.decimal, Decimal::Point);
    assert_eq!(v(&wb, 1, 1), Value::Number(1.234));
    assert!(!wb.csv_decimal_comma);
}

#[test]
fn only_ambiguous_values_follow_the_delimiter_and_are_reported() {
    let (wb, report) = read_text(b"Name;Value\r\nA;1.234\r\nB;12.345\r\n", &auto(TextKind::Csv)).unwrap();
    assert_eq!((report.numbers.decimal, report.numbers.basis), (Decimal::Comma, DecimalBasis::Delimiter));
    assert_eq!(report.numbers.ambiguous, vec!["1.234".to_string(), "12.345".into()]);
    assert_eq!(v(&wb, 1, 1), Value::Number(1234.0));
    let (wb, report) = read_text(b"Name,Value\r\nA,1.234\r\n", &auto(TextKind::Csv)).unwrap();
    assert_eq!((report.numbers.decimal, report.numbers.basis), (Decimal::Point, DecimalBasis::Delimiter));
    assert_eq!(v(&wb, 1, 1), Value::Number(1.234));
}

#[test]
fn a_column_with_both_kinds_keeps_them_as_text() {
    let (wb, report) = read_text(b"a;b\r\n1.5;2,5\r\n2,5;3\r\n", &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.numbers.mixed, vec![0]);
    assert_eq!(v(&wb, 1, 0), Value::text("1.5"));
    assert_eq!(v(&wb, 2, 0), Value::text("2,5"));
    assert_eq!(v(&wb, 1, 1), Value::Number(2.5));
}

#[test]
fn the_report_lists_incomplete_rows_and_split_numbers() {
    let text = "Name,Value\r\nA,1,234.50\r\nB,99.00\r\n\r\nC,12.00\r\nD\r\n";
    let (_, report) = read_text(text.as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.split, Split::delimiter(","));
    assert_eq!((report.fields, report.rows, report.complete), (2, 5, 3));
    assert_eq!(report.other_rows, vec![(2, 3), (6, 1)]);
    assert_eq!(report.split_numbers, vec![(2, "1,234.50".to_string())]);
    assert!(report.uncertain.iter().any(|u| u.contains("of 5 rows")), "{:?}", report.uncertain);
}

#[test]
fn a_title_before_the_table_is_skipped_only_when_asked() {
    let text = "Umsatzbericht Oktober 2026\r\n\r\nName;Wert\r\nA;1,5\r\n";
    let (wb, report) = read_text(text.as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.start_row, 2);
    assert_eq!(v(&wb, 0, 0), Value::text("Name"));
    assert_eq!(v(&wb, 1, 1), Value::Number(1.5));
    // File › Open without options keeps every line, like Excel.
    let (wb, report) = read_text(text.as_bytes(), &CsvOptions { start_row: Some(0), ..auto(TextKind::Csv) }).unwrap();
    assert_eq!(report.start_row, 0);
    assert_eq!(v(&wb, 0, 0), Value::text("Umsatzbericht Oktober 2026"));
    assert_eq!(v(&wb, 2, 0), Value::text("Name"));
}

#[test]
fn given_splits() {
    let read = |text: &str, split: Split| read_text(text.as_bytes(), &CsvOptions { split: Some(split), ..Default::default() }).unwrap().0;
    let wb = read("a||b||c\r\n", Split::delimiter("||"));
    assert_eq!((v(&wb, 0, 1), v(&wb, 0, 2)), (Value::text("b"), Value::text("c")));
    // Several delimiters at once, and runs of them as one.
    let wb = read("a;b,c\r\n", Split::Delimited { delimiters: vec![";".into(), ",".into()], consecutive: false });
    assert_eq!(v(&wb, 0, 2), Value::text("c"));
    let wb = read("a   b\r\n", Split::Delimited { delimiters: vec![" ".into()], consecutive: true });
    assert_eq!(v(&wb, 0, 1), Value::text("b"));
    let wb = read("plain     a,b       1234,5\r\nr2        d2\r\n", Split::FixedWidth(vec![10, 20]));
    assert_eq!((v(&wb, 0, 0), v(&wb, 0, 1), v(&wb, 1, 1)), (Value::text("plain"), Value::text("a,b"), Value::text("d2")));
    let wb = read("plain     a,b\r\n  12\r\n", Split::Lines);
    assert_eq!((v(&wb, 0, 0), v(&wb, 0, 1)), (Value::text("plain     a,b"), Value::Empty));
    assert_eq!(v(&wb, 1, 0), Value::Number(12.0));
    // Without a text qualifier, quotes are text (Excel's {none}).
    let (wb, _) = read_text(b"1,\"x,y\",2\r\n", &CsvOptions { no_qualifier: true, ..Default::default() }).unwrap();
    assert_eq!((v(&wb, 0, 1), v(&wb, 0, 2)), (Value::text("\"x"), Value::text("y\"")));
    // Start at the given record.
    let (wb, _) = read_text(b"skip\r\nkeep,1\r\n", &CsvOptions { start_row: Some(1), ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("keep"));
}

#[test]
fn value_evidence() {
    for (value, e) in [
        ("1.5", Some(Evidence::Point)),
        ("0.125", Some(Evidence::Point)),
        ("1,234.5", Some(Evidence::Point)),
        ("1,5", Some(Evidence::Comma)),
        ("1.234,5", Some(Evidence::Comma)),
        ("1 234,5", Some(Evidence::Comma)),
        ("-7,25 €", Some(Evidence::Comma)),
        ("1.234", Some(Evidence::Ambiguous)),
        ("1,234", Some(Evidence::Ambiguous)),
        ("1234", Some(Evidence::Same)),
        ("1 234", Some(Evidence::Same)),
        ("1.2.3", None),
        ("1,2,3", None),
        ("25.10.2026", None),
        ("A1", None),
        ("12:30", None),
        ("", None),
    ] {
        assert_eq!(evidence(value), e, "{value}");
    }
    assert_eq!(comma_to_point("1.234,5").as_deref(), Some("1,234.5"));
    assert_eq!(comma_to_point("12,5 %").as_deref(), Some("12.5 %"));
    assert_eq!(comma_to_point("(1,5)").as_deref(), Some("(1.5)"));
    assert_eq!(comma_to_point("1.5"), None);
}

#[test]
fn hostile_text_stays_bounded() {
    // One long line of every punctuation character, a megabyte of delimiters, a fixed-width
    // file far wider than the detection window, and a line of a million fields.
    let punct: String = (0x21u8..0x7f).filter(|b| !b.is_ascii_alphanumeric()).map(char::from).collect();
    let long = punct.repeat(20_000);
    let _ = detect(&long, TextKind::Csv, Some('"'), true);
    let lines = format!("{punct}\n").repeat(5_000);
    let _ = detect(&lines, TextKind::Other, Some('"'), true);
    let wide = format!("{}x\n{}y\n", " ".repeat(100_000), " ".repeat(100_000));
    let _ = detect(&wide, TextKind::Prn, None, true);
    let fields = ";".repeat(1_000_000);
    let (_, report) = read_text(fields.as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert!(report.fields <= gridcraft_core::MAX_COLS as usize);
    let (wb, report) = read_text(fields.as_bytes(), &CsvOptions { delimiter: b';', ..Default::default() }).unwrap();
    assert!(wb.sheet(0).unwrap().cells.is_empty(), "fields past column XFD are dropped");
    assert_eq!(report.fields, gridcraft_core::MAX_COLS as usize);
    // A megabyte of rows as wide as the sheet, empty or with both kinds of numbers in every
    // column: every cell is looked up by index or binary search, not by a scan.
    let empty = format!("{}\n", ";".repeat(16_383)).repeat(64);
    let (_, report) = read_text(empty.as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.split, Split::delimiter(";"));
    let mixed = [vec!["1,5"; 16_384].join(";"), vec!["0.5"; 16_384].join(";")].join("\n") + "\n";
    let (wb, report) = read_text(mixed.repeat(8).as_bytes(), &auto(TextKind::Csv)).unwrap();
    assert_eq!(report.numbers.mixed.len(), 16_384);
    assert_eq!(v(&wb, 1, 16_383), Value::text("0.5"));
    let mut n = 0;
    split_records(&"a||b\n".repeat(10), &Split::delimiter("||"), None, |r| {
        n += r.len();
        true
    });
    assert_eq!(n, 20);
}
