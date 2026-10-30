//! Typed numbers and dates in Spain's and Ukraine's formats reach the engine as en-US input.

use gridcraft_core::parse::{delocalize_input, localize_number, parse_input};
use gridcraft_core::{DateSystem, Region, Value};

/// What `typed` becomes in `region`: the value and the automatic format.
fn read(typed: &str, region: Region) -> (Value, Option<&'static str>) {
    let en = delocalize_input(typed, region);
    let p = parse_input(en.as_deref().unwrap_or(typed), DateSystem::D1900);
    (p.value, p.format)
}

fn number(typed: &str, region: Region) -> Option<f64> {
    match read(typed, region).0 {
        Value::Number(n) => Some(n),
        _ => None,
    }
}

#[test]
fn spanish_numbers_use_a_decimal_comma_and_dot_groups() {
    let es = Region::EsEs;
    assert_eq!(number("1,5", es), Some(1.5));
    assert_eq!(number("-0,25", es), Some(-0.25));
    assert_eq!(read("1.234,50", es), (Value::Number(1234.5), Some("#,##0.00")));
    assert_eq!(read("1.234.567", es), (Value::Number(1234567.0), Some("#,##0")));
    assert_eq!(read("12,5%", es), (Value::Number(0.125), Some("0.00%")));
    assert_eq!(read("12,50 €", es), (Value::Number(12.5), Some("#,##0.00 \"€\"")));
    assert_eq!(read("1.250 €", es), (Value::Number(1250.0), Some("#,##0 \"€\"")));
    assert_eq!(read("-3,75€", es), (Value::Number(-3.75), Some("#,##0.00 \"€\"")));
    assert_eq!(number("1,5E+3", es), Some(1500.0));
    assert_eq!(read("verdadero", es).0, Value::Bool(true));
    // Not numbers in Spain: bad groups, two decimal commas, words.
    for text in ["1.23,5", "1,2,3", "12,5 kg", "abc 1,5"] {
        assert!(matches!(read(text, es).0, Value::Text(_)), "{text}");
    }
    // Plain integers, ISO dates and times need no rewriting.
    for text in ["42", "2026-11-05", "10:30", "=SUMA(1;2)", "'1,5"] {
        assert_eq!(delocalize_input(text, es), None, "{text}");
    }
}

#[test]
fn ukrainian_numbers_group_with_spaces_and_take_the_hryvnia() {
    let uk = Region::UkUa;
    assert_eq!(read("1 234,50", uk), (Value::Number(1234.5), Some("#,##0.00")));
    assert_eq!(read("1\u{a0}234\u{a0}567", uk), (Value::Number(1234567.0), Some("#,##0")));
    assert_eq!(read("99,90 ₴", uk), (Value::Number(99.9), Some("#,##0.00 \"₴\"")));
    assert_eq!(read("1 500 грн", uk), (Value::Number(1500.0), Some("#,##0 \"грн\"")));
    assert_eq!(number("0,5", uk), Some(0.5));
    // A dot decimal after grouped digits mixes two conventions: text, not 1234.5.
    for text in ["1 234.5", "1\u{a0}234.50", "-1 234.5 ₴"] {
        assert!(matches!(read(text, uk).0, Value::Text(_)), "{text}");
    }
}

#[test]
fn dates_are_day_first_outside_the_united_states() {
    let serial = |y, m, d| gridcraft_core::date::serial_from_ymd(DateSystem::D1900, y, m, d);
    let es = Region::EsEs;
    let uk = Region::UkUa;
    assert_eq!(read("5/11/2026", es), (Value::Number(serial(2026, 11, 5).unwrap()), Some("m/d/yyyy")));
    assert_eq!(number("31/12/2026", es), serial(2026, 12, 31));
    assert_eq!(number("05.11.2026", uk), serial(2026, 11, 5));
    assert_eq!(number("5-11-26", es), serial(2026, 11, 5));
    // With a time.
    let with_time = number("05.11.2026 10:30", uk).unwrap();
    assert!((with_time - serial(2026, 11, 5).unwrap() - 10.5 / 24.0).abs() < 1e-9);
    // en-US keeps month first.
    assert_eq!(number("5/11/2026", Region::EnUs), serial(2026, 5, 11));
    // No 31st month.
    assert_eq!(read("12/31/2026", es).0, Value::text("12/31/2026"));
    assert_eq!(read("31/02/2026", uk).0, Value::text("31/02/2026"));
    // `1.5` is not 1 May.
    assert_eq!(delocalize_input("1.5", uk), None);
}

#[test]
fn constants_are_edited_with_the_regions_decimal_separator() {
    assert_eq!(localize_number("1234.5", Region::EsEs), "1234,5");
    assert_eq!(localize_number("-0.25", Region::UkUa), "-0,25");
    assert_eq!(localize_number("1.5E+20", Region::EsEs), "1,5E+20");
    assert_eq!(localize_number("1234.5", Region::EnUs), "1234.5");
    assert_eq!(localize_number("Total 1.5", Region::EsEs), "Total 1.5");
    assert_eq!(localize_number("42", Region::EsEs), "42");
    // What is shown for editing reads back as the same number.
    for raw in ["1234.5", "-0.001", "3.14159", "1E-7"] {
        let shown = localize_number(raw, Region::EsEs);
        assert_eq!(number(&shown, Region::EsEs), raw.parse().ok(), "{raw}");
    }
}

#[test]
fn hostile_input_never_panics() {
    let long = "1.".repeat(10_000);
    for text in ["", " ", ",", ".", "€", "₴", "-", "1,", ",5", "1 ", "\u{a0}", "//", "1/2/3/4", "31/02/2026", "ñ1,5", long.as_str()] {
        for region in Region::ALL {
            let _ = read(text, region);
            let _ = localize_number(text, region);
        }
    }
}

#[test]
fn arithmetic_reads_only_the_en_us_currency() {
    use gridcraft_core::parse::{number_stored_as_text, parse_number_text};
    // Excel en-US coerces "$3" in a formula but not "250 грн" ("250 грн"+1 is #VALUE!).
    assert_eq!(parse_number_text("$3"), Some(3.0));
    for text in ["250 грн", "12.50 €", "€5", "100 ₴", "7 грн."] {
        assert_eq!(parse_number_text(text), None, "{text}");
    }
    // Typed entry and the stored-as-text check still read them.
    assert_eq!(number_stored_as_text("12.50 €"), Some(12.5));
}

#[test]
fn ukrainian_booleans_read_in_any_case() {
    let uk = Region::UkUa;
    for (typed, value) in [("ІСТИНА", true), ("істина", true), ("Хибність", false), ("хибність", false)] {
        assert_eq!(read(typed, uk).0, Value::Bool(value), "{typed}");
    }
    // Each region reads its own words only.
    assert!(matches!(read("істина", Region::EsEs).0, Value::Text(_)));
    assert!(matches!(read("verdadero", uk).0, Value::Text(_)));
}

#[test]
fn a_sign_after_the_euro_inside_parentheses_is_not_a_number() {
    use gridcraft_core::parse::number_stored_as_text;
    // Excel reads no sign inside accounting parentheses, after a currency symbol either.
    for text in ["(€-5)", "(€ -5)", "(€+5)", "($-5)"] {
        assert_eq!(parse_input(text, DateSystem::D1900).value, Value::text(text), "{text}");
        assert_eq!(number_stored_as_text(text), None, "{text}");
    }
    assert_eq!(parse_input("(€5)", DateSystem::D1900).value, Value::Number(-5.0));
    assert_eq!(number_stored_as_text("(€5)"), Some(-5.0));
}
