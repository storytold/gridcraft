//! Column types: detecting them (Data Type Detection, "based on first 200 rows") and converting
//! values (Change Type, optionally Using Locale).

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_model::query::ColType;

/// Rows looked at to detect a column's type.
pub const DETECT_ROWS: usize = 200;

/// Number and date conventions of a culture (Change Type › Using Locale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Culture {
    pub decimal: char,
    pub group: char,
    pub order: DateOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
}

impl Culture {
    /// The culture for a locale tag (`en-US`, `de-DE`, `fr`…); unknown tags read as `en-US`.
    pub fn of(locale: Option<&str>) -> Culture {
        let tag = locale.unwrap_or("en-US").trim().to_ascii_lowercase().replace('_', "-");
        let lang = tag.split('-').next().unwrap_or("");
        let us = Culture { decimal: '.', group: ',', order: DateOrder::Mdy };
        match (lang, tag.as_str()) {
            (_, "en-us" | "en-ph") | ("", _) => us,
            ("en", _) => Culture { decimal: '.', group: ',', order: DateOrder::Dmy },
            ("ja" | "zh" | "ko" | "hu" | "lt", _) => {
                Culture { decimal: if lang == "hu" || lang == "lt" { ',' } else { '.' }, group: ',', order: DateOrder::Ymd }
            }
            ("de" | "nl" | "da" | "id" | "it" | "es" | "pt" | "tr" | "el" | "ro" | "sr" | "hr" | "sl", _) if tag != "de-ch" => {
                Culture { decimal: ',', group: '.', order: DateOrder::Dmy }
            }
            (_, "de-ch") => Culture { decimal: '.', group: '\'', order: DateOrder::Dmy },
            ("fr" | "ru" | "cs" | "pl" | "sv" | "fi" | "nb" | "no" | "uk" | "sk" | "bg" | "et" | "lv", _) => {
                Culture { decimal: ',', group: '\u{a0}', order: DateOrder::Dmy }
            }
            _ => us,
        }
    }
}

/// The type a column's values suggest: the first [`DETECT_ROWS`] non-null values decide.
pub fn detect<'a>(values: impl Iterator<Item = &'a Value>, sys: DateSystem) -> ColType {
    let cul = Culture::of(None);
    let mut seen = 0usize;
    // Candidates still possible, most specific first.
    let (mut whole, mut number, mut logical, mut date, mut datetime, mut time) = (true, true, true, true, true, true);
    let mut texts = 0usize;
    for v in values {
        if seen >= DETECT_ROWS {
            break;
        }
        match v {
            Value::Empty => continue,
            Value::Text(t) if t.trim().is_empty() => continue,
            Value::Number(n) => {
                whole &= n.fract() == 0.0 && n.abs() < 9.0e15;
                logical = false;
                date = false;
                datetime = false;
                time = false;
            }
            Value::Bool(_) => {
                whole = false;
                number = false;
                date = false;
                datetime = false;
                time = false;
            }
            Value::Text(t) => {
                texts += 1;
                let t = t.trim();
                let n = parse_number(t, &cul);
                whole &= n.is_some_and(|n| n.fract() == 0.0 && n.abs() < 9.0e15) && !t.contains(['%', 'e', 'E']);
                number &= n.is_some();
                logical &= t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false");
                let d = parse_datetime(t, &cul, sys);
                date &= d.is_some_and(|(_, has_time)| !has_time);
                datetime &= d.is_some_and(|(_, has_time)| has_time);
                time &= parse_time(t).is_some();
            }
            Value::Error(_) | Value::Array(_) => {
                whole = false;
                number = false;
                logical = false;
                date = false;
                datetime = false;
                time = false;
            }
        }
        seen += 1;
    }
    if seen == 0 {
        return if texts == 0 { ColType::Any } else { ColType::Text };
    }
    if whole && number {
        ColType::Whole
    } else if number {
        ColType::Number
    } else if logical {
        ColType::Logical
    } else if date {
        ColType::Date
    } else if datetime {
        ColType::DateTime
    } else if time {
        ColType::Time
    } else if texts > 0 {
        ColType::Text
    } else {
        ColType::Any
    }
}

/// A value converted to `to`. Null stays null and errors stay errors; a value that can't be
/// converted becomes an error value (`#VALUE!`), as Power Query shows `Error` in the cell.
pub fn convert(v: &Value, to: ColType, cul: &Culture, sys: DateSystem) -> Value {
    let fail = Value::Error(CellError::Value);
    match v {
        Value::Empty | Value::Error(_) => return v.clone(),
        Value::Array(_) => return fail,
        Value::Text(t) if t.is_empty() && to != ColType::Text && to != ColType::Any => return Value::Empty,
        _ => {}
    }
    match to {
        ColType::Any => v.clone(),
        ColType::Text => match v {
            Value::Text(_) => v.clone(),
            Value::Bool(b) => Value::text(if *b { "true" } else { "false" }),
            Value::Number(n) => Value::text(gridcraft_core::number_to_text(*n)),
            _ => fail,
        },
        ColType::Number | ColType::Currency | ColType::Whole | ColType::Percentage => {
            let n = match v {
                Value::Number(n) => Some(*n),
                Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
                Value::Text(t) => parse_number(t.trim(), cul).or_else(|| parse_datetime(t.trim(), cul, sys).map(|(n, _)| n)),
                _ => None,
            };
            match n.filter(|n| n.is_finite()) {
                Some(n) => Value::number(match to {
                    // Int64: to the nearest whole number, halves to even (Number.Round's default).
                    ColType::Whole => round_half_even(n),
                    ColType::Currency => (n * 10_000.0).round() / 10_000.0,
                    _ => n,
                }),
                None => fail,
            }
        }
        ColType::Logical => match v {
            Value::Bool(_) => v.clone(),
            Value::Number(n) => Value::Bool(*n != 0.0),
            Value::Text(t) if t.trim().eq_ignore_ascii_case("true") => Value::Bool(true),
            Value::Text(t) if t.trim().eq_ignore_ascii_case("false") => Value::Bool(false),
            _ => fail,
        },
        ColType::Date | ColType::DateTime | ColType::Time => {
            let n = match v {
                Value::Number(n) => Some(*n),
                Value::Text(t) => {
                    let t = t.trim();
                    if to == ColType::Time {
                        parse_time(t).or_else(|| parse_datetime(t, cul, sys).map(|(n, _)| n.fract()))
                    } else {
                        parse_datetime(t, cul, sys).map(|(n, _)| n)
                    }
                }
                _ => None,
            };
            match n.filter(|n| n.is_finite() && *n >= 0.0) {
                Some(n) => Value::number(match to {
                    ColType::Date => n.floor(),
                    ColType::Time => n.fract(),
                    _ => n,
                }),
                None => fail,
            }
        }
    }
}

fn round_half_even(n: f64) -> f64 {
    let r = n.round();
    if (n - n.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - n.signum() } else { r }
}

/// A number written in `cul`'s conventions: sign, group separators, decimal separator, a
/// currency symbol, a percent sign, parentheses for negatives, exponents.
pub fn parse_number(t: &str, cul: &Culture) -> Option<f64> {
    let mut s: String = t.chars().filter(|c| !c.is_whitespace() && *c != '\u{a0}' && *c != '\u{202f}').collect();
    if s.is_empty() || s.len() > 400 {
        return None;
    }
    let mut neg = false;
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        neg = true;
        s = inner.to_string();
    }
    let mut percent = false;
    if let Some(x) = s.strip_suffix('%') {
        percent = true;
        s = x.to_string();
    }
    s = s.trim_matches(['$', '€', '£', '¥']).to_string();
    if let Some(x) = s.strip_prefix('-') {
        neg = !neg;
        s = x.trim_start_matches(['$', '€', '£', '¥']).to_string();
    } else if let Some(x) = s.strip_prefix('+') {
        s = x.to_string();
    }
    if s.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut seen_decimal = false;
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_digit() {
            out.push(c);
        } else if c == cul.decimal && !seen_decimal {
            seen_decimal = true;
            out.push('.');
        } else if c == cul.group && !seen_decimal && i > 0 {
            continue;
        } else if (c == 'e' || c == 'E') && !out.is_empty() {
            out.push('e');
        } else if (c == '-' || c == '+') && out.ends_with('e') {
            out.push(c);
        } else {
            return None;
        }
    }
    if !out.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: f64 = out.parse().ok()?;
    let n = if percent { n / 100.0 } else { n };
    let n = if neg { -n } else { n };
    n.is_finite().then_some(n)
}

/// `hh:mm`, `hh:mm:ss`, with an optional AM/PM: the fraction of a day.
pub fn parse_time(t: &str) -> Option<f64> {
    let t = t.trim();
    let (clock, pm) = match t.to_ascii_lowercase() {
        s if s.ends_with("pm") => (t.get(..t.len().saturating_sub(2))?.trim(), Some(true)),
        s if s.ends_with("am") => (t.get(..t.len().saturating_sub(2))?.trim(), Some(false)),
        _ => (t, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let num = |s: &str| -> Option<f64> { if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit() || c == '.') { None } else { s.parse().ok() } };
    let mut h = num(parts.first()?)?;
    let m = num(parts.get(1)?)?;
    let s = match parts.get(2) {
        Some(x) => num(x)?,
        None => 0.0,
    };
    if let Some(pm) = pm {
        if !(1.0..=12.0).contains(&h) {
            return None;
        }
        h = h % 12.0 + if pm { 12.0 } else { 0.0 };
    }
    if h >= 24.0 || m >= 60.0 || s >= 60.0 {
        return None;
    }
    Some(gridcraft_core::date::time_fraction(h, m, s))
}

/// A date or date-time: ISO 8601 (`2026-03-04`, `2026-03-04T10:30:00Z`), or day, month and year
/// separated by `/`, `-` or `.` in `cul`'s order, with an optional time. Returns the serial and
/// whether a time was given.
pub fn parse_datetime(t: &str, cul: &Culture, sys: DateSystem) -> Option<(f64, bool)> {
    let t = t.trim();
    if t.len() < 6 || t.len() > 40 || !t.starts_with(|c: char| c.is_ascii_digit()) {
        return month_name_date(t, cul, sys);
    }
    let (date_part, time_part) = match t.find(['T', ' ']) {
        Some(i) => (t.get(..i)?, Some(t.get(i + 1..)?.trim())),
        None => (t, None),
    };
    let sep = date_part.chars().find(|c| matches!(c, '-' | '/' | '.'))?;
    let parts: Vec<&str> = date_part.split(sep).collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 4 || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let n = |i: usize| -> Option<i64> { parts.get(i)?.parse().ok() };
    let (y, m, d) = if parts.first()?.len() == 4 {
        (n(0)?, n(1)?, n(2)?)
    } else {
        match cul.order {
            DateOrder::Mdy => (n(2)?, n(0)?, n(1)?),
            DateOrder::Dmy => (n(2)?, n(1)?, n(0)?),
            DateOrder::Ymd => (n(0)?, n(1)?, n(2)?),
        }
    };
    let y = if parts.get(2).is_some_and(|p| p.len() <= 2) && parts.first().is_some_and(|p| p.len() <= 2) {
        if y < 30 { 2000 + y } else { 1900 + y }
    } else {
        y
    };
    if !(1..=12).contains(&m)
        || !(1..=31).contains(&d)
        || !(1900..=9999).contains(&y)
        || d > i64::from(gridcraft_core::date::days_in_month(y as i32, m as u32))
    {
        return None;
    }
    let serial = serial_from_ymd(sys, y, m, d)?;
    match time_part {
        None => Some((serial, false)),
        Some(tp) => {
            // Drop a zone designator (`Z`, `+01:00`) and fractional seconds.
            let tp = tp.trim_end_matches('Z');
            let tp = match tp.rfind(['+', '-']) {
                Some(i) if i >= 5 => tp.get(..i)?,
                _ => tp,
            };
            let tp = tp.split('.').next().unwrap_or(tp);
            Some((serial + parse_time(tp)?, true))
        }
    }
}

/// `Mar 4, 2026`, `4-Mar-2026`, `4 March 2026` (English month names), through the cell-input
/// parser, which reads them for en-US.
fn month_name_date(t: &str, cul: &Culture, sys: DateSystem) -> Option<(f64, bool)> {
    if cul.decimal != '.' || !t.chars().any(|c| c.is_ascii_alphabetic()) || t.len() > 40 {
        return None;
    }
    let p = gridcraft_core::parse::parse_input(t, sys);
    let f = p.format?;
    let has_date = f.contains('d') || f.contains('y');
    match p.value {
        Value::Number(n) if has_date => Some((n, f.contains('h'))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: DateSystem = DateSystem::D1900;

    #[test]
    fn numbers_parse_in_their_culture() {
        let us = Culture::of(Some("en-US"));
        let de = Culture::of(Some("de-DE"));
        let fr = Culture::of(Some("fr-FR"));
        assert_eq!(parse_number("1,234.5", &us), Some(1234.5));
        assert_eq!(parse_number("1.234,5", &de), Some(1234.5));
        assert_eq!(parse_number("1 234,5", &fr), Some(1234.5));
        assert_eq!(parse_number("(12)", &us), Some(-12.0));
        assert_eq!(parse_number("$-3.5", &us), Some(-3.5));
        assert_eq!(parse_number("12%", &us), Some(0.12));
        assert_eq!(parse_number("1e3", &us), Some(1000.0));
        for bad in ["", "abc", "1.2.3", "--1", "1,2a", "e5", "."] {
            assert_eq!(parse_number(bad, &us), None, "{bad}");
        }
    }

    #[test]
    fn dates_parse_in_their_culture() {
        let us = Culture::of(Some("en-US"));
        let de = Culture::of(Some("de-DE"));
        let d = serial_from_ymd(S, 2026, 3, 4).unwrap();
        assert_eq!(parse_datetime("2026-03-04", &us, S), Some((d, false)));
        assert_eq!(parse_datetime("3/4/2026", &us, S), Some((d, false)));
        assert_eq!(parse_datetime("04.03.2026", &de, S), Some((d, false)));
        assert_eq!(parse_datetime("Mar 4, 2026", &us, S), Some((d, false)));
        let (dt, has_time) = parse_datetime("2026-03-04T12:00:00Z", &us, S).unwrap();
        assert!(has_time && (dt - d - 0.5).abs() < 1e-9);
        assert_eq!(parse_datetime("2026-02-30", &us, S), None);
        assert_eq!(parse_datetime("13/13/2026", &us, S), None);
        assert_eq!(parse_time("6:30 PM"), Some(gridcraft_core::date::time_fraction(18.0, 30.0, 0.0)));
    }

    #[test]
    fn detection_and_conversion() {
        let col = |xs: &[&str]| xs.iter().map(|x| Value::text(*x)).collect::<Vec<_>>();
        assert_eq!(detect(col(&["1", "2", "", "30"]).iter(), S), ColType::Whole);
        assert_eq!(detect(col(&["1", "2.5"]).iter(), S), ColType::Number);
        assert_eq!(detect(col(&["TRUE", "false"]).iter(), S), ColType::Logical);
        assert_eq!(detect(col(&["2026-01-01", "2026-01-02"]).iter(), S), ColType::Date);
        assert_eq!(detect(col(&["2026-01-01 10:00", "2026-01-02 11:00"]).iter(), S), ColType::DateTime);
        assert_eq!(detect(col(&["North", "2"]).iter(), S), ColType::Text);
        assert_eq!(detect([Value::number(1.5)].iter(), S), ColType::Number);
        let us = Culture::of(None);
        assert_eq!(convert(&Value::text("2.5"), ColType::Whole, &us, S), Value::number(2.0));
        assert_eq!(convert(&Value::text("3.5"), ColType::Whole, &us, S), Value::number(4.0));
        assert_eq!(convert(&Value::text("x"), ColType::Number, &us, S), Value::Error(CellError::Value));
        assert_eq!(convert(&Value::text(""), ColType::Number, &us, S), Value::Empty);
        assert_eq!(convert(&Value::number(1.0), ColType::Text, &us, S), Value::text("1"));
        assert_eq!(convert(&Value::text("TRUE"), ColType::Logical, &us, S), Value::Bool(true));
        assert_eq!(convert(&Value::text("1,5"), ColType::Number, &Culture::of(Some("de-DE")), S), Value::number(1.5));
    }
}
