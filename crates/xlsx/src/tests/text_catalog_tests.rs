//! A catalog of text files (#2553): every kind of delimiter, decimal and thousands separator,
//! date, quote, title, blank and broken line, line end and encoding, read the way Text Import's
//! Detect reads them and, for a `.csv`, the way File › Open does. Each file states what a person
//! (or Excel, given the right settings) reads in it.
//!
//! `GRIDCRAFT_TEXT_CATALOG=<dir>` also writes the files and what was read from them there.

use gridcraft_core::{CellRef, Value};
use gridcraft_model::Workbook;

use crate::text_layout::{Decimal, Split, TextKind, TextReport};
use crate::{CsvOptions, read_text};

/// What a cell holds.
#[derive(Clone, Copy, Debug)]
enum W {
    N(f64),
    T(&'static str),
    B(bool),
    E,
}

struct Case {
    name: &'static str,
    bytes: Vec<u8>,
    /// Text Import's Detect: the split, the records skipped before the table, the decimal
    /// separator and some cells.
    split: Option<Split>,
    start: usize,
    decimal: Option<Decimal>,
    cells: Vec<(&'static str, W)>,
    /// File › Open of the `.csv` (only `,`, `;` or tab, no title skipped), when it differs.
    csv: Option<(Split, Vec<(&'static str, W)>)>,
    /// A difference that is known and tracked elsewhere.
    known: Option<&'static str>,
}

fn case(name: &'static str, text: &str) -> Case {
    Case { name, bytes: text.as_bytes().to_vec(), split: None, start: 0, decimal: None, cells: vec![], csv: None, known: None }
}

impl Case {
    fn split(mut self, s: Split) -> Self {
        self.split = Some(s);
        self
    }
    fn d(self, d: &str) -> Self {
        self.split(Split::delimiter(d))
    }
    fn lines(self) -> Self {
        self.split(Split::Lines)
    }
    fn start(mut self, n: usize) -> Self {
        self.start = n;
        self
    }
    fn dec(mut self, d: Decimal) -> Self {
        self.decimal = Some(d);
        self
    }
    fn cells(mut self, c: &[(&'static str, W)]) -> Self {
        self.cells = c.to_vec();
        self
    }
    fn csv(mut self, s: Split, c: &[(&'static str, W)]) -> Self {
        self.csv = Some((s, c.to_vec()));
        self
    }
    fn bytes(mut self, b: Vec<u8>) -> Self {
        self.bytes = b;
        self
    }
    fn known(mut self, k: &'static str) -> Self {
        self.known = Some(k);
        self
    }
}

fn kind_of(name: &str) -> TextKind {
    match name.rsplit('.').next() {
        Some("csv") => TextKind::Csv,
        Some("prn") => TextKind::Prn,
        Some("txt" | "tsv") => TextKind::Tab,
        _ => TextKind::Other,
    }
}

const D: Decimal = Decimal::Comma;
const P: Decimal = Decimal::Point;
use W::{B, E, N, T};

fn catalog() -> Vec<Case> {
    let utf16 = |s: &str| -> Vec<u8> { [0xFF, 0xFE].into_iter().chain(s.encode_utf16().flat_map(u16::to_le_bytes)).collect() };
    let urls: String = (0..20).map(|i| format!("https://example.com/page{i}\r\n")).collect();
    let wide: String = {
        let row = |r: usize| (0..100).map(|c| format!("{}", r * 100 + c)).collect::<Vec<_>>().join(";");
        format!("{}\r\n{}\r\n{}\r\n", row(0), row(1), row(2))
    };
    let many: String = std::iter::once("id;Wert\r\n".to_string()).chain((1..=3000).map(|i| format!("{i};{i},5\r\n"))).collect();
    let long_field = format!("a;b\r\n{};1\r\n", "x".repeat(100_000));
    vec![
        // ---- delimiters
        case("comma.csv", "Name,Qty,Price\r\nApple,3,1.5\r\nPear,12,2.25\r\n").d(",").dec(P).cells(&[
            ("A2", T("Apple")),
            ("B3", N(12.0)),
            ("C2", N(1.5)),
        ]),
        case("semicolon.csv", "Name;Menge;Preis\r\nApfel;3;1,5\r\nBirne;12;2,25\r\n").d(";").dec(D).cells(&[
            ("A2", T("Apfel")),
            ("C2", N(1.5)),
            ("C3", N(2.25)),
        ]),
        case("tab.txt", "Name\tQty\tPrice\r\nApple\t3\t1.5\r\nPear\t12\t2.25\r\n").d("\t").dec(P).cells(&[("C3", N(2.25))]),
        case("tab_named.csv", "Name\tQty\r\nApple\t3\r\nPear\t12\r\n").d("\t").cells(&[("B3", N(12.0))]),
        case("tab_decimal_comma.txt", "Artikel\tPreis\r\nApfel\t1,99\r\nBirne\t12,50\r\n").d("\t").dec(D).cells(&[("B2", N(1.99)), ("B3", N(12.5))]),
        case("pipe.txt", "id|name|price\r\n1|Apfel|1.5\r\n2|Birne|2.25\r\n").d("|").dec(P).cells(&[("B2", T("Apfel")), ("C3", N(2.25))]),
        case("pipe.csv", "id|name|price\r\n1|Apfel|1.5\r\n2|Birne|2.25\r\n")
            .d("|")
            .cells(&[("C2", N(1.5))])
            .csv(Split::Lines, &[("A1", T("id|name|price")), ("B1", E)]),
        case("colon.txt", "Name:Qty:Code\r\nApfel:3:A1\r\nBirne:12:B2\r\nKiwi:7:C3\r\n").d(":").cells(&[("B3", N(12.0)), ("C4", T("C3"))]),
        case("caret.txt", "a^b^c\r\n1^2^3\r\n4^5^6\r\n").d("^").cells(&[("C3", N(6.0))]),
        case("tilde.txt", "a~b~c\r\nx~y~z\r\n1~2~3\r\n").d("~").cells(&[("B2", T("y"))]),
        case("hash_numbers.txt", "1#2#3\r\n4#5#6\r\n7#8#9\r\n").d("#").cells(&[("C3", N(9.0))]),
        case("section_numbers.txt", "1§2\r\n3§4\r\n5§6\r\n").d("§").cells(&[("B3", N(6.0))]),
        case("unit_separator.txt", "a\u{1f}b\u{1f}c\r\n1\u{1f}2\u{1f}3\r\n").d("\u{1f}").cells(&[("C2", N(3.0))]),
        case("double_pipe.txt", "id||name||price\r\n1||Apfel||1.5\r\n2||Birne||2.25\r\n").d("||").cells(&[("B3", T("Birne")), ("C2", N(1.5))]),
        case("hash_pipe.txt", "id#|#name#|#price\r\n1#|#Apfel#|#1.5\r\n2#|#Birne#|#2.25\r\n").d("#|#").cells(&[("C3", N(2.25))]),
        case("aligned_spaces.txt", "Name    Qty  Code\r\nApfel   3    A1\r\nBirne   12   B2\r\n")
            .split(Split::Delimited { delimiters: vec![" ".into()], consecutive: true })
            .cells(&[("B3", N(12.0)), ("C2", T("A1"))]),
        case("single_spaces.txt", "1 2 3\r\n4 5 6\r\n7 8 9\r\n").d(" ").cells(&[("C3", N(9.0))]),
        case("fixed_width.prn", "plain     a,b       1234,5\r\nr2        d2\r\n").split(Split::FixedWidth(vec![10, 20])).cells(&[
            ("B1", T("a,b")),
            ("C1", N(1234.5)),
            ("B2", T("d2")),
        ]),
        case("fixed_report.txt", "plain text   12   ok\r\nx            3    no\r\nlong name    456  ok\r\n")
            .split(Split::FixedWidth(vec![13, 18]))
            .cells(&[("A1", T("plain text")), ("B3", N(456.0))]),
        // ---- one column: punctuation inside the values is no delimiter
        case("one_column_text.csv", "Name\r\nAlice\r\nBob\r\n").lines().cells(&[("A3", T("Bob")), ("B1", E)]),
        case("one_column_de_amounts.csv", "Betrag\r\n1.234,50\r\n99,00\r\n7,5\r\n").lines().dec(D).cells(&[
            ("A2", N(1234.5)),
            ("A4", N(7.5)),
            ("B2", E),
        ]),
        case("one_column_en_amounts.csv", "Amount\r\n1,234.50\r\n99.00\r\n7.5\r\n").lines().dec(P).cells(&[("A2", N(1234.5)), ("A4", N(7.5))]),
        case("one_column_semicolon_sometimes.csv", "Comment\r\nfine\r\nyes; no\r\nok\r\nmaybe\r\n").lines().cells(&[("A3", T("yes; no"))]),
        case("urls.csv", &format!("url\r\n{urls}")).lines().cells(&[("A2", T("https://example.com/page0")), ("B2", E)]),
        case("emails.csv", "email\r\nanna@example.com\r\nb.c@example.org\r\nd_e@example.net\r\n").lines().cells(&[("A2", T("anna@example.com"))]),
        case("subjects.txt", "Re: question\r\nRe: answer\r\nFwd: report\r\nRe: Re: again\r\n").lines().cells(&[("A1", T("Re: question"))]),
        case("windows_paths.txt", "C:\\Users\\me\\a.txt\r\nD:\\data\\b.csv\r\nC:\\x\\y.txt\r\n").lines().cells(&[("A2", T("D:\\data\\b.csv"))]),
        case("order_numbers.txt", "Order #1000\r\nOrder #1001\r\nOrder #1002\r\n").lines().cells(&[("A1", T("Order #1000"))]),
        case("key_value.txt", "name=Alice\r\nage=30\r\ncity=Berlin\r\n").lines().cells(&[("A2", T("age=30"))]),
        case("breadcrumbs.csv", "Home > Garden > Tools\r\nHome > Kitchen > Pots\r\nShop > Toys > Cars\r\n")
            .csv(Split::Lines, &[("A1", T("Home > Garden > Tools"))]),
        case("times_only.txt", "12:30:00\r\n08:15:30\r\n23:59:59\r\n").lines().cells(&[("A2", N(29_730.0 / 86_400.0))]),
        case("dates_only.txt", "2026-10-25\r\n2026-10-26\r\n2026-10-27\r\n").lines().cells(&[("A1", N(46_320.0))]),
        // ---- decimal and thousands separators
        case("de_thousands.csv", "Artikel;Umsatz\r\nX;1.234,50\r\nY;12.345,00\r\nZ;999,99\r\n").d(";").dec(D).cells(&[
            ("B2", N(1234.5)),
            ("B3", N(12345.0)),
            ("B4", N(999.99)),
        ]),
        case("en_decimals_semicolon.csv", "Name;Value\r\nA;1.5\r\nB;0.125\r\nC;1.234\r\n").d(";").dec(P).cells(&[
            ("B2", N(1.5)),
            ("B3", N(0.125)),
            ("B4", N(1.234)),
        ]),
        case("en_quoted_thousands.csv", "Name,Value\r\nA,\"1,234.50\"\r\nB,99.00\r\n").d(",").dec(P).cells(&[("B2", N(1234.5)), ("B3", N(99.0))]),
        case("en_unquoted_thousands.csv", "Name,Value\r\nA,1,234.50\r\nB,99.00\r\nC,12.00\r\n").d(",").dec(P).cells(&[
            ("B2", N(1.0)),
            ("C2", N(234.5)),
            ("B3", N(99.0)),
        ]),
        case("fr_space_thousands.csv", "Nom;Valeur\r\nA;1 234,5\r\nB;12 345,25\r\n").d(";").dec(D).cells(&[("B2", N(1234.5)), ("B3", N(12345.25))]),
        case("fr_nbsp_thousands.csv", "Nom;Valeur\r\nA;1\u{a0}234,5\r\nB;12\u{202f}345,25\r\n")
            .d(";")
            .dec(D)
            .cells(&[("B2", N(1234.5)), ("B3", N(12345.25))]),
        // Swiss `'` groups are numbers; Excel outside a Swiss region keeps them as text (measured).
        case("ch_apostrophe_thousands.csv", "Name;Betrag\r\nA;1'234.50\r\nB;12'345.00\r\nC;1\u{2019}000\r\n").d(";").dec(P).cells(&[
            ("B2", N(1234.5)),
            ("B3", N(12345.0)),
            ("B4", N(1000.0)),
        ]),
        case("ambiguous_semicolon.csv", "Name;Value\r\nA;1.234\r\nB;12.345\r\n").d(";").dec(D).cells(&[("B2", N(1234.0)), ("B3", N(12345.0))]),
        case("ambiguous_comma.csv", "Name,Value\r\nA,1.234\r\nB,12.345\r\n").d(",").dec(P).cells(&[("B2", N(1.234)), ("B3", N(12.345))]),
        case("two_dot_groups.csv", "a;b\r\nx;1.234\r\ny;12.345.678\r\n").d(";").dec(D).cells(&[("B2", N(1234.0)), ("B3", N(12_345_678.0))]),
        case("two_comma_groups.csv", "a;b\r\nx;1,234\r\ny;12,345,678\r\n").d(";").dec(P).cells(&[("B2", N(1234.0)), ("B3", N(12_345_678.0))]),
        case("columns_decide_each.csv", "a;b;c\r\nx;1,5;0.125\r\ny;2,5;3.75\r\n").d(";").cells(&[("B2", N(1.5)), ("C2", N(0.125)), ("C3", N(3.75))]),
        case("both_kinds_in_a_column.csv", "a;b\r\n1;1,5\r\n2;0.125\r\n").d(";").cells(&[("B2", T("1,5")), ("B3", T("0.125"))]),
        case("negative_de.csv", "a;b\r\nx;-1,5\r\ny;-1.234,50\r\n").d(";").dec(D).cells(&[("B2", N(-1.5)), ("B3", N(-1234.5))]),
        case("plus_sign_de.csv", "a;b\r\nx;+1,5\r\ny;2,5\r\n").d(";").dec(D).cells(&[("B2", N(1.5))]),
        case("parentheses_negative_de.csv", "a;b\r\nx;(1.234,50)\r\ny;2,5\r\n").d(";").dec(D).cells(&[("B2", N(-1234.5))]),
        // A trailing minus (accounting exports) is negative; Excel keeps it as text unless the
        // import is told otherwise (measured).
        case("trailing_minus_de.csv", "Konto;Betrag\r\n4711;1.234,50-\r\n4712;99,00\r\n4713;7,50 €-\r\n4714;5-\r\n").d(";").dec(D).cells(&[
            ("B2", N(-1234.5)),
            ("B3", N(99.0)),
            ("B4", N(-7.5)),
            ("B5", N(-5.0)),
        ]),
        case("trailing_minus_en.csv", "Account,Amount\r\n4711,\"1,234.50-\"\r\n4712,99.00\r\n").d(",").dec(P).cells(&[("B2", N(-1234.5))]),
        case("dashes_stay_text.csv", "a;b\r\nx;5--\r\ny;-5-\r\nz;2026-10-\r\n").d(";").cells(&[
            ("B2", T("5--")),
            ("B3", T("-5-")),
            ("B4", T("2026-10-")),
        ]),
        case("leading_comma_de.csv", "a;b\r\nx;,5\r\ny;1,5\r\n").d(";").dec(D).cells(&[("B2", N(0.5)), ("B3", N(1.5))]),
        case("percent_de.csv", "a;b\r\nx;12,5 %\r\ny;50%\r\nz;1,5\r\n").d(";").dec(D).cells(&[("B2", N(0.125)), ("B3", N(0.5))]),
        case("percent_en.csv", "a,b\r\nx,12.5%\r\ny,50%\r\n").d(",").dec(P).cells(&[("B2", N(0.125)), ("B3", N(0.5))]),
        case("euro_de.csv", "Artikel;Preis\r\nA;1.234,50 €\r\nB;7,50 €\r\nC;3 €\r\n").d(";").dec(D).cells(&[
            ("B2", N(1234.5)),
            ("B3", N(7.5)),
            ("B4", N(3.0)),
        ]),
        case("euro_prefix_de.csv", "Artikel;Preis\r\nA;€ 1.234,50\r\nB;€7,50\r\n").d(";").dec(D).cells(&[("B2", N(1234.5)), ("B3", N(7.5))]),
        case("dollar_en.csv", "Item,Price\r\nA,\"$1,234.50\"\r\nB,$7.50\r\n").d(",").dec(P).cells(&[("B2", N(1234.5)), ("B3", N(7.5))]),
        case("scientific_en.csv", "a,b\r\nx,1.5E3\r\ny,2.5e-2\r\n").d(",").dec(P).cells(&[("B2", N(1500.0)), ("B3", N(0.025))]),
        // German Excel: 1500, -1500, 123000, 1000 (measured).
        case("scientific_de.csv", "a;b\r\nx;1,5E3\r\ny;-1,5E3\r\nz;12,3e4\r\nw;1,000E3\r\nv;2,5\r\n").d(";").dec(D).cells(&[
            ("B2", N(1500.0)),
            ("B3", N(-1500.0)),
            ("B4", N(123_000.0)),
            ("B5", N(1000.0)),
            ("B6", N(2.5)),
        ]),
        case("leading_zeros.csv", "PLZ;Ort\r\n01067;Dresden\r\n10115;Berlin\r\n").d(";").cells(&[("A2", N(1067.0)), ("A3", N(10115.0))]),
        case("phone_numbers.csv", "Name;Telefon\r\nA;+49 30 1234567\r\nB;030/1234567\r\n").d(";").cells(&[("B2", T("+49 30 1234567"))]),
        case("long_digits.csv", "Name;Nummer\r\nA;1234567890123456789\r\n").d(";").cells(&[("B2", N(1_234_567_890_123_456_789.0))]),
        case("quoted_numbers_de.csv", "\"Name\";\"Wert\"\r\n\"A\";\"1,5\"\r\n\"B\";\"1.234,50\"\r\n").d(";").dec(D).cells(&[
            ("A2", T("A")),
            ("B2", N(1.5)),
            ("B3", N(1234.5)),
        ]),
        case("padded_numbers_de.csv", "a;b\r\nx; 1,5\r\ny;2,5 \r\n").d(";").dec(D).cells(&[("B2", N(1.5)), ("B3", N(2.5))]),
        case("de_numbers_only.csv", "Menge;Preis\r\n1,5;2,30\r\n2,5;3,10\r\n").d(";").dec(D).cells(&[("A2", N(1.5)), ("B3", N(3.1))]),
        case("en_numbers_only.csv", "a,b\r\n1.5,2.25\r\n3.5,4.75\r\n").d(",").dec(P).cells(&[("B3", N(4.75))]),
        case("en_one_dot_five_in_semicolon.csv", "Name;Wert\r\nA;1.5\r\nB;3.12\r\n").d(";").dec(P).cells(&[("B2", N(1.5)), ("B3", N(3.12))]),
        // ---- dates and times
        case("dates_de.csv", "Datum;Betrag\r\n25.10.2026;1,5\r\n01.11.2026;2,5\r\n").d(";").dec(D).cells(&[
            ("A2", N(46_320.0)),
            ("A3", N(46_327.0)),
            ("B2", N(1.5)),
        ]),
        case("dates_iso.csv", "Date,Value\r\n2026-10-25,1.5\r\n2026-11-01,2\r\n").d(",").cells(&[("A2", N(46_320.0)), ("A3", N(46_327.0))]),
        case("dates_us.csv", "Date,Value\r\n10/25/2026,1.5\r\n11/01/2026,2\r\n").d(",").cells(&[("A2", N(46_320.0))]),
        case("times.csv", "Start;Ende\r\n12:30;08:15:30\r\n").d(";").cells(&[("A2", N(12.5 / 24.0)), ("B2", N(29_730.0 / 86_400.0))]),
        case("datetime_iso.csv", "When,What\r\n2026-10-25 12:30,a\r\n2026-10-26 08:00,b\r\n").d(",").cells(&[("A2", N(46_320.0 + 12.5 / 24.0))]),
        case("de_dates_times_amounts.csv", "Datum;Zeit;Betrag\r\n25.10.2026;12:30;1.234,50\r\n26.10.2026;08:00;99,00\r\n").d(";").dec(D).cells(&[
            ("A2", N(46_320.0)),
            ("B2", N(12.5 / 24.0)),
            ("C2", N(1234.5)),
        ]),
        // ---- quotes
        case("quoted_delimiter.csv", "a,b\r\n\"x,y\",z\r\n").d(",").cells(&[("A2", T("x,y")), ("B2", T("z"))]),
        case("quoted_semicolon.csv", "Artikel;Preis\r\n\"Pflaume; blau\";99,00\r\nApfel;1,50\r\n")
            .d(";")
            .dec(D)
            .cells(&[("A2", T("Pflaume; blau")), ("B2", N(99.0))]),
        case("quoted_newline.csv", "id,text\r\n1,\"line1\nline2\"\r\n2,x\r\n").d(",").cells(&[("B2", T("line1\nline2")), ("A3", N(2.0))]),
        case("doubled_quotes.csv", "id,text\r\n1,\"say \"\"hi\"\"\"\r\n2,x\r\n").d(",").cells(&[("B2", T("say \"hi\""))]),
        case("quote_inside_field.csv", "a,b\r\nx\"y,z\r\nw,v\r\n").d(",").cells(&[("A2", T("x\"y"))]),
        case("single_quotes_are_text.csv", "a,b,c\r\n'x,y',z\r\n1,2,3\r\n")
            .d(",")
            .cells(&[("A2", T("'x")), ("B2", T("y'"))])
            .known("#2476: Excel keeps the apostrophe"),
        case("empty_quoted.csv", "a,b\r\n\"\",x\r\n").d(",").cells(&[("A2", E), ("B2", T("x"))]),
        case("quoted_spaces.csv", "a;b\r\n\" x \";1\r\n").d(";").cells(&[("A2", T(" x "))]),
        case("unterminated_quote.csv", "a,b\r\nx,\"open\r\ny,z\r\n").d(",").cells(&[("A2", T("x"))]),
        // ---- structure
        case("title_rows.csv", "Report October 2026\r\n\r\nName;Value\r\nA;1,5\r\n")
            .d(";")
            .start(2)
            .dec(D)
            .cells(&[("A1", T("Name")), ("B2", N(1.5))])
            .csv(Split::delimiter(";"), &[("A1", T("Report October 2026")), ("A3", T("Name")), ("B4", N(1.5))]),
        case("two_title_rows.txt", "Bericht\r\nStand 25.10.2026\r\n\r\nName;Wert\r\nA;1,5\r\nB;2,5\r\n")
            .d(";")
            .start(3)
            .dec(D)
            .cells(&[("A1", T("Name")), ("B3", N(2.5))]),
        case("blank_lines_inside.csv", "a;b\r\n\r\n1;2\r\n\r\n3;4\r\n").d(";").cells(&[("A2", E), ("A3", N(1.0)), ("B5", N(4.0))]),
        case("trailing_blank_lines.csv", "a;b\r\n1;2\r\n\r\n\r\n").d(";").cells(&[("B2", N(2.0)), ("A3", E)]),
        case("trailing_delimiters.csv", "a;b;\r\n1;2;\r\n3;4;\r\n").d(";").cells(&[("B3", N(4.0)), ("C1", E)]),
        case("ragged_rows.csv", "a,b,c\r\n1,2\r\n3,4,5,6\r\n7,8,9\r\n").d(",").cells(&[("C2", E), ("D3", N(6.0)), ("C4", N(9.0))]),
        case("empty_fields.csv", "a,,c\r\n,,\r\n1,,3\r\n").d(",").cells(&[("B1", E), ("C3", N(3.0))]),
        case("only_delimiters.csv", ";;;\r\n;;;\r\n").cells(&[("A1", E)]),
        case("header_only.csv", "Name;Menge;Preis\r\n").d(";").cells(&[("B1", T("Menge")), ("C1", T("Preis"))]),
        case("single_row_de.csv", "1,5;2,5\r\n").d(";").dec(D).cells(&[("A1", N(1.5)), ("B1", N(2.5))]),
        case("empty.csv", "").cells(&[("A1", E)]),
        case("whitespace_only.csv", "   \r\n  \r\n").cells(&[("B1", E)]),
        case("no_final_line_end.csv", "a,b\r\n1,2").d(",").cells(&[("B2", N(2.0))]),
        case("lf_only.csv", "a;b\n1;2\n3;4\n").d(";").cells(&[("B3", N(4.0))]),
        case("cr_only.csv", "a;b\r1;2\r3;4\r").d(";").cells(&[("B3", N(4.0))]),
        case("mixed_line_ends.csv", "a;b\r\n1;2\n3;4\r5;6\r\n").d(";").cells(&[("B4", N(6.0))]),
        case("comma_text_in_semicolon_file.csv", "Name;Kommentar\r\nA;Hallo, Welt\r\nB;Ja, nein\r\n").d(";").cells(&[("B2", T("Hallo, Welt"))]),
        case("semicolon_text_in_comma_file.csv", "id,note\r\n1,a;b\r\n2,c;d\r\n").d(",").cells(&[("B2", T("a;b"))]),
        case("city_country.csv", "Berlin, Germany\r\nParis, France\r\nRome, Italy\r\n").d(",").cells(&[("B1", T(" Germany"))]),
        case("wide_100_columns.csv", &wide).d(";").cells(&[("CV3", N(299.0))]),
        case("rows_3000.csv", &many).d(";").dec(D).cells(&[("A3001", N(3000.0)), ("B3001", N(3000.5))]),
        case("long_field.csv", &long_field).d(";").cells(&[("B2", N(1.0))]),
        // ---- encodings
        case("utf8_bom.csv", "\u{feff}Name;Wert\r\nÄpfel;1,5\r\n").d(";").dec(D).cells(&[("A1", T("Name")), ("A2", T("Äpfel")), ("B2", N(1.5))]),
        case("utf16le_bom.txt", "")
            .bytes(utf16("Name\tWert\r\nÄpfel\t1,5\r\nÖl\t2,5\r\n"))
            .d("\t")
            .dec(D)
            .cells(&[("A2", T("Äpfel")), ("B3", N(2.5))]),
        case("windows1252.csv", "").bytes(b"Name;Wert\r\n\xC4pfel;1,5\r\n\xD6l;2,5\r\nStra\xDFe;3,5\r\n".to_vec()).d(";").dec(D).cells(&[
            ("A2", T("Äpfel")),
            ("A4", T("Straße")),
            ("B4", N(3.5)),
        ]),
        case("emoji.csv", "Obst;Menge\r\n🍎;1\r\n🍐;2\r\n").d(";").cells(&[("A2", T("🍎")), ("B3", N(2.0))]),
        // ---- Excel specials
        case("sep_line.csv", "sep=;\r\na;b\r\n1;2\r\n").d(";").cells(&[("A1", T("a")), ("B2", N(2.0))]).known("#2270: `sep=` names the delimiter"),
        case("booleans.csv", "a;b\r\nTRUE;FALSE\r\n").d(";").cells(&[("A2", B(true)), ("B2", B(false))]),
        case("leading_apostrophe.csv", "a;b\r\n'00123;x\r\n").d(";").cells(&[("A2", T("'00123"))]).known("#2476: Excel keeps the apostrophe"),
    ]
}

fn auto(kind: TextKind) -> CsvOptions {
    CsvOptions { delimiter: 0, detect_any: true, start_row: None, decimal: None, kind, ..Default::default() }
}

/// File › Open of a `.csv` without settings, as the engine reads it.
fn file_open() -> CsvOptions {
    CsvOptions { delimiter: 0, detect_any: false, start_row: Some(0), decimal: None, kind: TextKind::Csv, ..Default::default() }
}

fn same(got: &Value, want: W) -> bool {
    match (got, want) {
        (Value::Number(g), W::N(w)) => (g - w).abs() <= 1e-9 * w.abs().max(1.0),
        (Value::Text(g), W::T(w)) => &**g == w,
        (Value::Bool(g), W::B(w)) => *g == w,
        (Value::Empty, W::E) => true,
        (Value::Text(g), W::E) => g.is_empty(),
        _ => false,
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Number(n) => format!("{n}"),
        Value::Text(t) => format!("{:?}", t.chars().take(40).collect::<String>()),
        Value::Bool(b) => format!("{b}"),
        Value::Empty => "∅".into(),
        other => format!("{other:?}"),
    }
}

/// What differs from the expectation, one line each.
fn check(
    wb: &Workbook,
    report: &TextReport,
    split: Option<&Split>,
    start: Option<usize>,
    decimal: Option<Decimal>,
    cells: &[(&str, W)],
) -> Vec<String> {
    let mut out = vec![];
    if let Some(s) = split
        && &report.split != s
    {
        out.push(format!("split {} (want {})", report.split.describe(), s.describe()));
    }
    if let Some(n) = start
        && report.start_row != n
    {
        out.push(format!("start {} (want {n})", report.start_row));
    }
    if let Some(d) = decimal
        && report.numbers.decimal != d
    {
        out.push(format!("decimal {:?} (want {d:?})", report.numbers.decimal));
    }
    let sheet = wb.sheet(0);
    for (a1, want) in cells {
        let got = match (sheet, CellRef::parse(a1)) {
            (Some(sh), Some(r)) => sh.value(r),
            _ => Value::Empty,
        };
        if !same(&got, *want) {
            out.push(format!("{a1} {} (want {want:?})", show(&got)));
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '"' => "\\\"".into(),
            '\\' => "\\\\".into(),
            c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// One JSON line: the case and the cells read, for comparing with other readers.
fn dump_line(name: &str, mode: &str, report: &TextReport, wb: &Workbook) -> String {
    let mut cells = vec![];
    if let Some(sh) = wb.sheet(0)
        && let Some(r) = sh.used_range()
    {
        for row in r.start.row..=r.end.row.min(r.start.row + 30) {
            for col in r.start.col..=r.end.col.min(r.start.col + 30) {
                let at = CellRef::new(row, col);
                let v = match sh.value(at) {
                    Value::Number(n) => format!("n:{n}"),
                    Value::Text(t) => format!("s:{t}"),
                    Value::Bool(b) => format!("b:{b}"),
                    Value::Empty => continue,
                    other => format!("e:{other:?}"),
                };
                cells.push(format!("\"{}\":\"{}\"", at, esc(&v)));
            }
        }
    }
    format!(
        "{{\"id\":\"{}\",\"mode\":\"{mode}\",\"split\":\"{}\",\"start\":{},\"decimal\":\"{:?}\",\"cells\":{{{}}}}}",
        esc(name),
        esc(&report.split.describe()),
        report.start_row,
        report.numbers.decimal,
        cells.join(",")
    )
}

#[test]
fn every_file_of_the_catalog_reads_as_written() {
    let cases = catalog();
    assert!(cases.len() >= 60, "{} files", cases.len());
    let dir = std::env::var_os("GRIDCRAFT_TEXT_CATALOG").map(std::path::PathBuf::from);
    let mut dump = vec![];
    let mut table = vec![];
    let mut failed = vec![];
    for c in &cases {
        let (wb, report) = read_text(&c.bytes, &auto(kind_of(c.name))).unwrap();
        let mut diff = check(&wb, &report, c.split.as_ref(), Some(c.start), c.decimal, &c.cells);
        dump.push(dump_line(c.name, "detect", &report, &wb));
        let mut csv_diff = vec![];
        if c.name.ends_with(".csv") {
            let (wb2, report2) = read_text(&c.bytes, &file_open()).unwrap();
            dump.push(dump_line(c.name, "open", &report2, &wb2));
            match &c.csv {
                Some((s, cells)) => csv_diff = check(&wb2, &report2, Some(s), Some(0), None, cells),
                // Same split; the cells too when Detect skipped nothing.
                None => {
                    let cells: &[(&str, W)] = if c.start == 0 { &c.cells } else { &[] };
                    csv_diff = check(&wb2, &report2, c.split.as_ref(), Some(0), c.decimal, cells);
                }
            }
        }
        let status = match (diff.is_empty() && csv_diff.is_empty(), c.known) {
            (true, _) => "ok",
            (false, Some(_)) => "known",
            (false, None) => "FAIL",
        };
        diff.extend(csv_diff.into_iter().map(|d| format!("File › Open: {d}")));
        let what = format!(
            "{} · {:?}{}",
            report.split.describe(),
            report.numbers.decimal,
            if report.start_row > 0 { format!(" · from record {}", report.start_row + 1) } else { String::new() }
        );
        table.push(format!("{status:5} {:34} {what:38} {}{}", c.name, diff.join("; "), c.known.map(|k| format!(" [{k}]")).unwrap_or_default()));
        if status == "FAIL" {
            failed.push(c.name);
        }
        if let Some(dir) = &dir {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(c.name), &c.bytes).unwrap();
        }
    }
    if let Some(dir) = &dir {
        std::fs::write(dir.join("gridcraft.jsonl"), dump.join("\n") + "\n").unwrap();
    }
    println!("{} files\n{}", cases.len(), table.join("\n"));
    assert!(failed.is_empty(), "{} of {} files read wrong: {failed:?}\n{}", failed.len(), cases.len(), table.join("\n"));
}
