use super::*;

const S: DateSystem = DateSystem::D1900;

fn f(code: &str, n: f64) -> String {
    format_value(&Value::Number(n), &NumberFormat::parse(code), S).text
}

fn full(code: &str, v: Value) -> Formatted {
    format_value(&v, &NumberFormat::parse(code), S)
}

fn t(code: &str, s: &str) -> String {
    format_value(&Value::from(s), &NumberFormat::parse(code), S).text
}

#[test]
fn numbers() {
    assert_eq!(f("0.00", 1234.567), "1234.57");
    assert_eq!(f("#,##0", 1234567.0), "1,234,567");
    assert_eq!(f("#,##0", -1234567.89), "-1,234,568");
    assert_eq!(f("#,##0.00", 0.0), "0.00");
    assert_eq!(f("0", 0.5), "1");
    assert_eq!(f("0", -2.5), "-3");
    assert_eq!(f("0.00", 2.675), "2.68");
    assert_eq!(f("0.00", 1.005), "1.01");
    assert_eq!(f("0.0", -0.04), "-0.0");
    assert_eq!(f("00000", 123.0), "00123");
    assert_eq!(f("000-00-0000", 123456789.0), "123-45-6789");
    assert_eq!(f("#.##", 0.5), ".5");
    assert_eq!(f("0.##", 1.0), "1.");
    assert_eq!(f("#", 0.0), "");
    assert_eq!(f("0.0?", 1.5), "1.5 ");
    assert_eq!(f("??0.0", 1.5), "  1.5");
    assert_eq!(f(".00", 12.5), "12.50");
    assert_eq!(f("0", 1e20), "100000000000000000000");
    assert_eq!(f("0.0", 1e-20), "0.0");
    assert_eq!(f("0,000", 5.0), "0,005");
    assert_eq!(f("#,##0.000", 1234.5), "1,234.500");
}

#[test]
fn scaling_and_percent() {
    assert_eq!(f("#,##0,", 1234567.0), "1,235");
    assert_eq!(f("0.0,,", 12345678.0), "12.3");
    assert_eq!(f("0,\"K\"", 12345.0), "12K");
    assert_eq!(f("0%", 0.256), "26%");
    assert_eq!(f("0.00%", 0.12345), "12.35%");
    assert_eq!(f("0%%", 0.5), "5000%%");
    assert_eq!(f("0%", -0.5), "-50%");
    assert!(NumberFormat::parse("0.0%").is_percent());
    assert!(!NumberFormat::parse("0.0").is_percent());
}

#[test]
fn sections_and_colors() {
    let r = full("#,##0.00;[Red](#,##0.00)", Value::Number(-5.0));
    assert_eq!(r.text, "(5.00)");
    assert_eq!(r.color, Some(FormatColor::Red));
    assert!(r.numeric);
    let r = full("#,##0.00;[Red](#,##0.00)", Value::Number(5.0));
    assert_eq!((r.text.as_str(), r.color), ("5.00", None));
    assert_eq!(f("0;-0;;@", 0.0), "");
    assert_eq!(t("0;-0;;@", "hi"), "hi");
    assert_eq!(f("0;-0;\"zero\"", 0.0), "zero");
    assert_eq!(f("0;-0;\"zero\"", 0.001), "0");
    assert_eq!(f("0_);(0)", 5.0), "5 ");
    assert_eq!(f("0_);(0)", -5.0), "(5)");
    assert_eq!(f(";;;", 5.0), "");
    assert_eq!(t(";;;", "x"), "");
    assert_eq!(full("[Color10]0", Value::Number(1.0)).color, Some(FormatColor::Indexed(10)));
    assert_eq!(full("[blue]0", Value::Number(1.0)).color, Some(FormatColor::Blue));
    assert_eq!(full("[Color99]0", Value::Number(1.0)).color, None);
}

#[test]
fn conditions() {
    let c = "[>=100]\"big\";[<0]\"neg\";\"small\"";
    assert_eq!(f(c, 150.0), "big");
    assert_eq!(f(c, -5.0), "neg");
    assert_eq!(f(c, 50.0), "small");
    assert_eq!(f("[<1]0.00;0", 0.5), "0.50");
    assert_eq!(f("[<1]0.00;0", 5.0), "5");
    // Like Excel, a multi-section format never adds the minus sign itself.
    assert_eq!(f("[<1]0.00;0", -5.0), "5.00");
    assert_eq!(f("[>=1000]0,\"K\";0", -5.0), "5");
    assert_eq!(f("[>=1000]0,\"K\"", -5.0), "-5");
    let phone = "[<=9999999]###-####;(###) ###-####";
    assert_eq!(f(phone, 5551234.0), "555-1234");
    assert_eq!(f(phone, 2125551234.0), "(212) 555-1234");
    assert_eq!(full("[Red][<=100]0;[Blue][>100]0", Value::Number(150.0)).color, Some(FormatColor::Blue));
}

#[test]
fn scientific() {
    assert_eq!(f("0.00E+00", 12345.0), "1.23E+04");
    assert_eq!(f("0.00E+00", 0.000123), "1.23E-04");
    assert_eq!(f("0.00E+00", 0.0), "0.00E+00");
    assert_eq!(f("0.00E-00", 12345.0), "1.23E04");
    assert_eq!(f("0.00e+00", 12345.0), "1.23e+04");
    assert_eq!(f("##0.0E+0", 12345.0), "12.3E+3");
    assert_eq!(f("0.0E+0", 0.00001), "1.0E-5");
    assert_eq!(f("0.000E+00", 1e-300), "1.000E-300");
    assert_eq!(f("0.00E+00", 9.999), "1.00E+01");
    assert_eq!(f("0.00E+00", -12345.0), "-1.23E+04");
}

#[test]
fn fractions() {
    assert_eq!(f("# ?/?", 1.5), "1 1/2");
    assert_eq!(f("# ?/?", 0.25), " 1/4");
    assert_eq!(f("# ?/?", 2.0), "2    ");
    assert_eq!(f("# ?/?", 0.0), "0    ");
    assert_eq!(f("# ?/?", -1.5), "-1 1/2");
    assert_eq!(f("# ??/??", std::f64::consts::PI), "3 14/99");
    assert_eq!(f("# ??/??", 1.0 / 3.0), "  1/3 ");
    assert_eq!(f("?/8", 0.5), "4/8");
    assert_eq!(f("# ?/8", 2.125), "2 1/8");
    assert_eq!(f("?/?", 1.5), "3/2");
    assert_eq!(f("# ?/?", 0.99), "1    ");
    assert_eq!(f("# ???/???", 0.123), " 100/813");
    assert_eq!(f("# ???/???", 0.5), "   1/2  ");
}

#[test]
fn literals_and_fill() {
    assert_eq!(f("\"$\"#,##0.00", 3.0), "$3.00");
    assert_eq!(f("$#,##0.00", -3.0), "-$3.00");
    assert_eq!(f("[$€-407]#,##0.00", 1234.5), "€1,234.50");
    assert_eq!(f("0\" units\"", 5.0), "5 units");
    assert_eq!(f("\\A0", 5.0), "A5");
    assert_eq!(f("(0)", 5.0), "(5)");
    let r = full("*-0", Value::Number(5.0));
    assert_eq!((r.text.as_str(), r.fill), ("5", Some(('-', 0))));
    let acct = builtin_format(44).unwrap();
    let r = full(acct, Value::Number(0.0));
    assert_eq!((r.text.as_str(), r.fill), (" $-   ", Some((' ', 2))));
    let r = full(acct, Value::Number(1234.5));
    assert_eq!((r.text.as_str(), r.fill), (" $1,234.50 ", Some((' ', 2))));
    let r = full(acct, Value::Number(-1234.5));
    assert_eq!((r.text.as_str(), r.fill), (" $(1,234.50)", Some((' ', 2))));
    let r = full(acct, Value::from("abc"));
    assert_eq!((r.text.as_str(), r.numeric), (" abc ", false));
    let r = full("-* 0", Value::Number(-5.0));
    assert_eq!((r.text.as_str(), r.fill), ("--5", Some((' ', 2))));
}

#[test]
fn dates_and_times() {
    assert_eq!(f("m/d/yyyy", 46302.0), "10/7/2026");
    assert_eq!(f("dddd, mmmm d, yyyy", 46302.0), "Wednesday, October 7, 2026");
    assert_eq!(f("ddd mmm yy", 46302.0), "Wed Oct 26");
    assert_eq!(f("mmmmm", 46302.0), "O");
    assert_eq!(f("dd/mm/yyyy", 46302.0), "07/10/2026");
    assert_eq!(f("d-mmm-yy", 46302.0), "7-Oct-26");
    assert_eq!(f("e", 46302.0), "2026");
    assert_eq!(f("YYYY-MM-DD", 46302.0), "2026-10-07");
    assert_eq!(f("yyyy-mm-dd hh:mm:ss", 46302.5), "2026-10-07 12:00:00");
    assert_eq!(f("h:mm AM/PM", 0.75), "6:00 PM");
    assert_eq!(f("h AM/PM", 0.0), "12 AM");
    assert_eq!(f("h a/p", 0.25), "6 a");
    assert_eq!(f("hh:mm", 0.75), "18:00");
    assert_eq!(f("[h]:mm", 1.5), "36:00");
    assert_eq!(f("[h]:mm:ss", 2.25), "54:00:00");
    assert_eq!(f("[mm]:ss", 1.0 / 24.0), "60:00");
    assert_eq!(f("[s]", 1.0 / 1440.0), "60");
    assert_eq!(f("mm:ss.0", 61.2 / 86400.0), "01:01.2");
    assert_eq!(f("mm:ss.00", 61.25 / 86400.0), "01:01.25");
    assert_eq!(f("h:mm:ss", 0.5 + 59.6 / 86400.0), "12:01:00");
    assert_eq!(f("h:mm", 0.5 + 40.0 / 86400.0), "12:00");
    assert_eq!(f("m/d/yyyy", 60.0), "2/29/1900");
    assert_eq!(f("m/d/yyyy", 0.0), "1/0/1900");
    assert_eq!(format_value(&Value::Number(0.0), &NumberFormat::parse("m/d/yyyy"), DateSystem::D1904).text, "1/1/1904");
    assert_eq!(f("m/d/yyyy", -1.0), "########");
    assert_eq!(f("m/d/yyyy", 3e6), "########");
    assert_eq!(f("[$-409]m/d/yyyy", 46302.0), "10/7/2026");
    assert_eq!(f("m/d/yyyy h:mm", 46302.75), "10/7/2026 18:00");
    assert_eq!(f("mmss.0", 0.0), "0000.0");
    assert!(NumberFormat::parse("h:mm").is_date());
    assert!(!NumberFormat::parse("0.00").is_date());
}

#[test]
fn text_and_other_values() {
    assert_eq!(t("@", "hello"), "hello");
    assert_eq!(t("\"<\"@\">\"", "x"), "<x>");
    assert_eq!(t("0.00", "abc"), "abc");
    assert_eq!(f("@", 5.0), "5");
    assert!(NumberFormat::parse("@").is_text());
    assert!(!NumberFormat::parse("0;@").is_text());
    let g = NumberFormat::general();
    assert_eq!(format_value(&Value::Bool(true), &g, S).text, "TRUE");
    assert_eq!(format_value(&Value::Bool(false), &NumberFormat::parse("0.00"), S).text, "FALSE");
    assert_eq!(format_value(&Value::Error(CellError::Div0), &g, S).text, "#DIV/0!");
    assert_eq!(format_value(&Value::Empty, &g, S), Formatted::default());
    let r = format_value(&Value::from("x"), &g, S);
    assert!(!r.numeric);
}

#[test]
fn general() {
    assert_eq!(f("General", 1e11), "1E+11");
    assert_eq!(f("General", 12345678901.0), "12345678901");
    assert_eq!(f("General", 0.1 + 0.2), "0.3");
    assert_eq!(f("General", -1234.5), "-1234.5");
    assert_eq!(f("", 1.0 / 3.0), "0.333333333");
    assert_eq!(f("general", 123456.789012), "123456.789");
    assert_eq!(f("General\" units\"", 5.0), "5 units");
    assert_eq!(f("General;[Red]-General", -5.0), "-5");
    assert!(NumberFormat::parse("General").is_general());
    assert!(NumberFormat::default().is_general());
    assert_eq!(NumberFormat::parse("0.0").code(), "0.0");
}

#[test]
fn kinds() {
    assert_eq!(NumberFormat::general().kind(), FormatKind::General);
    assert_eq!(NumberFormat::parse("0.00").kind(), FormatKind::Number);
    assert_eq!(NumberFormat::parse("#,##0.00;[Red](#,##0.00)").kind(), FormatKind::Number);
    assert_eq!(NumberFormat::parse("\"$\"#,##0.00").kind(), FormatKind::Currency);
    assert_eq!(NumberFormat::parse(builtin_format(44).unwrap()).kind(), FormatKind::Accounting);
    assert_eq!(NumberFormat::parse("m/d/yyyy").kind(), FormatKind::Date);
    assert_eq!(NumberFormat::parse("h:mm AM/PM").kind(), FormatKind::Time);
    assert_eq!(NumberFormat::parse("0%").kind(), FormatKind::Percentage);
    assert_eq!(NumberFormat::parse("# ?/?").kind(), FormatKind::Fraction);
    assert_eq!(NumberFormat::parse("0.00E+00").kind(), FormatKind::Scientific);
    assert_eq!(NumberFormat::parse("@").kind(), FormatKind::Text);
    assert_eq!(NumberFormat::parse("00000").kind(), FormatKind::Special);
    assert_eq!(NumberFormat::parse("\"hello\"").kind(), FormatKind::Custom);
}

#[test]
fn builtins() {
    assert_eq!(builtin_format(0), Some("General"));
    assert_eq!(builtin_format(14), Some("m/d/yyyy"));
    assert_eq!(builtin_format(49), Some("@"));
    assert_eq!(builtin_format(30), None);
    assert_eq!(builtin_format(50), None);
    assert_eq!(builtin_id("m/d/yyyy"), Some(14));
    assert_eq!(builtin_id("0.00%"), Some(10));
    assert_eq!(builtin_id("GENERAL"), Some(0));
    assert_eq!(builtin_id("[h]:mm:ss"), Some(46));
    assert_eq!(builtin_id("#,##0.00_);(#,##0.00)"), None);
    for id in 0..=60 {
        if let Some(code) = builtin_format(id) {
            assert_eq!(builtin_id(code), Some(id));
        }
    }
    assert_eq!(f(builtin_format(5).unwrap(), -1234.0), "($1,234)");
    assert_eq!(f(builtin_format(12).unwrap(), 0.5), " 1/2");
    assert_eq!(f(builtin_format(47).unwrap(), 61.2 / 86400.0), "0101.2");
}

#[test]
fn text_fn() {
    assert_eq!(text_function(&Value::Number(1234.5), "$#,##0.00", S), Ok("$1,234.50".into()));
    assert_eq!(text_function(&Value::from("5"), "0.00", S), Ok("5.00".into()));
    assert_eq!(text_function(&Value::from("abc"), "0.00", S), Ok("abc".into()));
    assert_eq!(text_function(&Value::Error(CellError::NA), "0", S), Err(CellError::NA));
    assert_eq!(text_function(&Value::Number(46302.0), "yyyy", S), Ok("2026".into()));
    assert_eq!(text_function(&Value::Number(0.5), "General", S), Ok("0.5".into()));
    assert_eq!(text_function(&Value::Number(1.0 / 3.0), "0.000", S), Ok("0.333".into()));
    assert_eq!(text_function(&Value::Empty, "0.00", S), Ok("0.00".into()));
    assert_eq!(text_function(&Value::Number(5.0), "", S), Ok("".into()));
    assert_eq!(text_function(&Value::Number(-1.0), "m/d/yyyy", S), Err(CellError::Value));
    assert_eq!(text_function(&Value::Bool(true), "0", S), Ok("TRUE".into()));
}

/// Deterministic xorshift for fuzzing.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

#[test]
fn fuzz_no_panic() {
    let pieces = [
        "0",
        "#",
        "?",
        ".",
        ",",
        "%",
        "E+",
        "E-",
        "e",
        "/",
        "8",
        "16",
        "\"",
        "\\",
        "_",
        "*",
        "@",
        ";",
        "[",
        "]",
        "[Red]",
        "[>=100]",
        "[<0]",
        "[Color7]",
        "[$€-407]",
        "[$-409]",
        "[h]",
        "[mm]",
        "[s]",
        "d",
        "dd",
        "ddd",
        "dddd",
        "m",
        "mm",
        "mmm",
        "mmmmm",
        "yy",
        "yyyy",
        "h",
        "hh",
        "s",
        "ss",
        ".000",
        "AM/PM",
        "A/P",
        "General",
        " ",
        "-",
        "(",
        ")",
        "$",
        "é",
        "€",
        "💥",
        "b",
        "g",
        "[",
        "[>",
        "[=",
        "\"abc",
        "*",
        "_",
        "00000000000000000000",
        "##########",
        ",,,,",
        "%%%%%%",
        "E+0000",
        "?/?????????",
        "/0",
    ];
    let values = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        1e-300,
        -1e-300,
        1e300,
        -1e300,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        2958465.999999,
        2958466.0,
        46302.75,
        123_456_789.123_456_78,
        -987654.321,
        1.0 / 3.0,
        std::f64::consts::PI,
        1e15,
        1e16,
        0.999999999,
        f64::NAN,
        f64::INFINITY,
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let n = (rng.next() % 8) as usize;
        let mut code = String::new();
        for _ in 0..n {
            code.push_str(rng.pick(&pieces));
        }
        let fmt = NumberFormat::parse(&code);
        let _ = (fmt.kind(), fmt.is_date(), fmt.is_percent(), fmt.is_text(), fmt.code());
        for &v in &values {
            let r = format_value(&Value::Number(v), &fmt, S);
            if let Some((_, pos)) = r.fill {
                assert!(r.text.is_char_boundary(pos), "{code:?} {v}");
            }
            let _ = format_value(&Value::Number(v), &fmt, DateSystem::D1904);
            let _ = text_function(&Value::Number(v), &code, S);
        }
        let _ = format_value(&Value::from("text"), &fmt, S);
        let _ = text_function(&Value::from("12"), &code, S);
    }
    // Random bytes as codes.
    for _ in 0..500 {
        let len = (rng.next() % 24) as usize;
        let code: String = (0..len).map(|_| char::from_u32((rng.next() % 0x3000) as u32).unwrap_or('x')).collect();
        let fmt = NumberFormat::parse(&code);
        for _ in 0..5 {
            let v = f64::from_bits(rng.next());
            let _ = format_value(&Value::Number(v), &fmt, S);
        }
    }
    for w in 0..20 {
        for &v in &values {
            if let Some(s) = format_general_fit(v, w) {
                assert!(s.chars().count() <= w, "{v} {w} {s}");
            }
        }
    }
}

mod german {
    use gridcraft_core::{CellError, DateSystem, Locale, Value};

    use crate::{NumberFormat, format_value, format_value_in, text_function};

    fn de(v: Value, code: &str) -> String {
        format_value_in(&v, &NumberFormat::parse(code), DateSystem::D1900, Locale::De).text
    }

    fn day() -> Value {
        Value::Number(gridcraft_core::date::serial_from_ymd(DateSystem::D1900, 2026, 10, 10).unwrap_or_default())
    }

    #[test]
    fn numbers_use_german_separators() {
        assert_eq!(de(Value::Number(1234.56), "#,##0.00"), "1.234,56");
        assert_eq!(de(Value::Number(-1234567.0), "#,##0"), "-1.234.567");
        assert_eq!(de(Value::Number(48200.0), "#,##0 \"€\""), "48.200 €");
        assert_eq!(de(Value::Number(0.284), "0.0%"), "28,4%");
        assert_eq!(de(Value::Number(1234.5), "General"), "1234,5");
        assert_eq!(de(Value::Number(0.5), "0.00E+00"), "5,00E-01");
        assert_eq!(de(Value::Number(1.5), "# ?/?"), "1 1/2");
        // A literal dot in a code stays a dot.
        assert_eq!(de(Value::Number(5.0), "0\".\""), "5.");
    }

    #[test]
    fn dates_take_the_german_patterns() {
        assert_eq!(de(day(), "m/d/yyyy"), "10.10.2026");
        assert_eq!(de(day(), "d-mmm"), "10. Okt");
        assert_eq!(de(day(), "d-mmm-yy"), "10. Okt 26");
        assert_eq!(de(day(), "[$-F800]dddd, mmmm dd, yyyy"), "Samstag, 10. Oktober 2026");
        assert_eq!(de(day(), "mmmm yyyy"), "Oktober 2026", "custom codes keep their pattern, in German");
        assert_eq!(de(day(), "ddd"), "Sa");
        let t = Value::Number(10.5 / 24.0 + 15.0 / 86400.0);
        assert_eq!(de(t, "[$-F400]h:mm:ss AM/PM"), "10:30:15");
    }

    #[test]
    fn words_and_errors() {
        assert_eq!(de(Value::Bool(true), "General"), "WAHR");
        assert_eq!(de(Value::Error(CellError::NA), "General"), "#NV");
        assert_eq!(de(Value::text("Text"), "General"), "Text");
    }

    #[test]
    fn stored_results_stay_en_us() {
        assert_eq!(format_value(&Value::Number(1234.56), &NumberFormat::parse("#,##0.00"), DateSystem::D1900).text, "1,234.56");
        assert_eq!(text_function(&Value::Number(1234.5), "#,##0.00", DateSystem::D1900), Ok("1,234.50".into()));
        assert_eq!(format_value(&day(), &NumberFormat::parse("m/d/yyyy"), DateSystem::D1900).text, "10/10/2026");
    }
}
