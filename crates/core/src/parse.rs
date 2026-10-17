//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`, `1Jan`,
//! `Mar 15`, `Jan 2021`, `15Mar2024`), times
//! (`10:30`, `10:30:15 PM`) and date-times. Locale: en-US.

use crate::date::{DateSystem, MONTHS, serial_from_ymd, time_fraction};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas). Text that isn't a number, date, boolean or error
/// stays text. A leading apostrophe forces text.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("TRUE") {
        return Parsed { value: Value::Bool(true), format: None };
    }
    if t.eq_ignore_ascii_case("FALSE") {
        return Parsed { value: Value::Bool(false), format: None };
    }
    if let Some(e) = CellError::parse(t) {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = parse_numeric(t, sys) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00").
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900)
}

/// Like [`parse_number_text`], with date text read in the given workbook date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem) -> Option<f64> {
    parse_numeric(s.trim(), sys).map(|(n, _)| n)
}

fn parse_numeric(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    // Parenthesised negatives: (100) = -100.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric(inner.trim(), sys).map(|(n, f)| (-n, f));
    }
    if let Some(r) = parse_plain(t) {
        return Some(r);
    }
    if let Some(r) = parse_fraction(t) {
        return Some((r, Some("# ?/?")));
    }
    if let Some(r) = parse_datetime(t, sys) {
        return Some(r);
    }
    None
}

/// Signs, currency, thousands separators, percent, exponent.
fn parse_plain(t: &str) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    let mut currency = false;
    let mut percent = false;
    if let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    if let Some(r) = s.strip_prefix('$') {
        currency = true;
        s = r.trim_start();
        if !neg && let Some(r2) = s.strip_prefix('-') {
            neg = true;
            s = r2;
        }
    }
    if let Some(r) = s.strip_suffix('%') {
        percent = true;
        s = r.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    let has_sep = s.contains(',');
    let digits: String = if has_sep {
        // Thousands separators must be in groups of three before the decimal point.
        let (int, frac) = s.split_once('.').map_or((s, None), |(a, b)| (a, Some(b)));
        let groups: Vec<&str> = int.split(',').collect();
        let first = groups.first()?;
        if first.is_empty() || first.len() > 3 || groups.iter().skip(1).any(|g| g.len() != 3) {
            return None;
        }
        let mut d = groups.concat();
        if let Some(f) = frac {
            d.push('.');
            d.push_str(f);
        }
        d
    } else {
        s.to_string()
    };
    let b = digits.as_bytes();
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    if !b.first().is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
        return None;
    }
    let mut n: f64 = digits.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    let sci = digits.contains(['e', 'E']);
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if currency {
        Some(if digits.contains('.') { "\"$\"#,##0.00" } else { "\"$\"#,##0" })
    } else if percent {
        Some(if digits.contains('.') { "0.00%" } else { "0%" })
    } else if sci {
        Some("0.00E+00")
    } else if has_sep {
        Some(if digits.contains('.') { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

/// `1/2` (only with a whole part, else it's a date), `1 1/2`, `0 3/4`.
fn parse_fraction(t: &str) -> Option<f64> {
    let (whole, frac) = t.split_once(' ')?;
    let neg = whole.starts_with('-');
    let w: f64 = whole.trim_start_matches('-').parse().ok()?;
    let (a, b) = frac.trim().split_once('/')?;
    let a: f64 = a.parse().ok()?;
    let b: f64 = b.parse().ok()?;
    if b == 0.0 || !w.is_finite() {
        return None;
    }
    let v = w + a / b;
    Some(if neg { -v } else { v })
}

fn month_from_name(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    if l.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.to_ascii_lowercase().starts_with(&l)).map(|i| i as u32 + 1)
}

/// Time part: `h:mm`, `h:mm:ss`, `h:mm:ss.000`, `h AM`, with optional AM/PM.
fn parse_time(t: &str) -> Option<(f64, bool, bool)> {
    let t = t.trim();
    let lower = t.to_ascii_lowercase();
    let (body, ampm) = if let Some(b) = lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        (b.trim().to_string(), Some(false))
    } else if let Some(b) = lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
        (b.trim().to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut h: f64 = parts.first()?.parse().ok()?;
    let m: f64 = parts.get(1).map_or(Some(0.0), |p| p.parse().ok())?;
    let s: f64 = parts.get(2).map_or(Some(0.0), |p| p.parse().ok())?;
    if m >= 60.0 || s >= 60.0 || h < 0.0 || m < 0.0 || s < 0.0 {
        return None;
    }
    if let Some(pm) = ampm {
        if !(0.0..=12.0).contains(&h) {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    } else if h >= 10000.0 {
        return None;
    }
    Some((time_fraction(h, m, s), parts.len() == 3, ampm.is_some()))
}

/// A piece of month-name date text: a run of digits or a month name.
enum DatePart<'a> {
    Num(&'a str),
    Month(u32),
}

/// Two-digit years: 00–29 are 2000–2029, 30–99 are 1930–1999 (Excel's window).
fn two_digit_year(y: i64) -> i64 {
    if y < 30 { 2000 + y } else { 1900 + y }
}

/// A year written with one, two or four digits. Three or five digits are not a year.
fn year_part(s: &str) -> Option<i64> {
    let y = s.parse::<i64>().ok()?;
    match s.len() {
        1 | 2 => Some(two_digit_year(y)),
        4 => Some(y),
        _ => None,
    }
}

/// A day written with one or two digits (`5`, `05`; not `005`).
fn day_part(s: &str) -> Option<i64> {
    if s.len() > 2 {
        return None;
    }
    s.parse::<i64>().ok()
}

/// Dates written with a month name, as Excel reads them: day and month (`1Jan`, `1 Jan`,
/// `1-Jan`, `1/Jan`), month and day (`Jan1`, `Mar 15`, `Jan-21`), month and year (`Jan 2021`,
/// `Jan2021`, `Jan-99`) and day, month and year (`1Jan21`, `15Mar2024`, `4-Mar-2026`,
/// `4 March 2026`). The parts may touch or be split by spaces, `-` or `/`; `.` and `,` are not
/// read here. Month names are full or at least three letters (`Sept`, `Janu`). A month name
/// followed by one or two digits is a day when that day exists in the current year (`Jan 21` is
/// 21 January), otherwise a two-digit year (`Jan 32` is January 1932). A missing day is the 1st,
/// a missing year the current year. Text with a month name in another order (`Jan 15 2021`,
/// `2021 Jan`) is not read here.
fn parse_month_name_date(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let mut parts: Vec<DatePart> = Vec::with_capacity(3);
    let mut rest = t;
    loop {
        let spaced = rest.trim_start_matches(' ');
        let (sep, after) = match spaced.strip_prefix(['-', '/']) {
            Some(a) => (true, a.trim_start_matches(' ')),
            None => (false, spaced),
        };
        if after.is_empty() {
            // A trailing `-` or `/` is not part of a date.
            if sep {
                return None;
            }
            break;
        }
        // A leading separator (`-1Jan`) is not a date either.
        if parts.is_empty() && (sep || spaced.len() != rest.len()) {
            return None;
        }
        let first = after.chars().next()?;
        let end = if first.is_ascii_digit() {
            after.find(|c: char| !c.is_ascii_digit())
        } else if first.is_ascii_alphabetic() {
            after.find(|c: char| !c.is_ascii_alphabetic())
        } else {
            return None;
        }
        .unwrap_or(after.len());
        let (run, tail) = (after.get(..end)?, after.get(end..)?);
        // At most three parts; digit runs longer than a year and words longer than "September"
        // are never part of a date, so work stays bounded for long text.
        if parts.len() == 3 {
            return None;
        }
        if first.is_ascii_digit() {
            if run.len() > 4 {
                return None;
            }
            parts.push(DatePart::Num(run));
        } else {
            if run.len() > 9 {
                return None;
            }
            parts.push(DatePart::Month(month_from_name(run)?));
        }
        rest = tail;
    }
    let year = current_year();
    match parts.as_slice() {
        [DatePart::Num(d), DatePart::Month(m)] => valid_ymd(sys, year, *m as i64, day_part(d)?).map(|s| (s, "d-mmm")),
        [DatePart::Month(m), DatePart::Num(n)] => {
            let m = *m as i64;
            let v = n.parse::<i64>().ok()?;
            match n.len() {
                // A day of this year when there is one, else a two-digit year.
                1 | 2 => match valid_ymd(sys, year, m, v) {
                    Some(s) => Some((s, "d-mmm")),
                    None => valid_ymd(sys, two_digit_year(v), m, 1).map(|s| (s, "mmm-yy")),
                },
                4 => valid_ymd(sys, v, m, 1).map(|s| (s, "mmm-yy")),
                _ => None,
            }
        }
        [DatePart::Num(d), DatePart::Month(m), DatePart::Num(y)] => valid_ymd(sys, year_part(y)?, *m as i64, day_part(d)?).map(|s| (s, "d-mmm-yy")),
        _ => None,
    }
}

fn parse_date(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    if t.bytes().any(|b| b.is_ascii_alphabetic())
        && let Some(r) = parse_month_name_date(t, sys)
    {
        return Some(r);
    }
    let num = |s: &str| s.trim().parse::<i64>().ok();
    // ISO yyyy-mm-dd or yyyy/mm/dd
    let seps: &[char] = &['/', '-', '.'];
    let parts: Vec<&str> = t.split(seps).collect();
    if parts.len() == 3 {
        let (a, b, c) = (parts[0].trim(), parts[1].trim(), parts[2].trim());
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
        }
        if let (Some(m), Some(d), Some(y)) = (num(a), num(b), num(c)) {
            let y = if c.len() <= 2 { two_digit_year(y) } else { y };
            return valid_ymd(sys, y, m, d).map(|s| (s, "m/d/yyyy"));
        }
    }
    if parts.len() == 2 {
        let (a, b) = (parts[0].trim(), parts[1].trim());
        // m/d is in the current year, like Excel.
        if let (Some(m), Some(d)) = (num(a), num(b)) {
            let y = current_year();
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return valid_ymd(sys, y, m, d).map(|s| (s, "d-mmm"));
            }
            // m/yyyy
            if (1..=12).contains(&m) && b.len() == 4 {
                return valid_ymd(sys, d, m, 1).map(|s| (s, "mmm-yy"));
            }
        }
    }
    // "Mar 4, 2026" / "March 4 2026" (day-first month-name forms are read above)
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3
        && let (Some(m), Some(d), Some(y)) = (month_from_name(words[0]), num(words[1]), num(words[2]))
    {
        // Two-digit years follow the same 2029 rule as the dashed forms above.
        let y = if words[2].len() <= 2 { two_digit_year(y) } else { y };
        return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
    }
    None
}

fn valid_ymd(sys: DateSystem, y: i64, m: i64, d: i64) -> Option<f64> {
    // Dates before 1900 stay text (`DATE()` would add 1900 to such a year).
    if !(1..=12).contains(&m) || d < 1 || y < 1900 {
        return None;
    }
    let max = crate::date::days_in_month_in(sys, y as i32, m as u32) as i64;
    if d > max {
        return None;
    }
    serial_from_ymd(sys, y, m, d)
}

/// The year used for dates typed without one, from the system clock (browser clock on wasm).
/// Falls back to 2026 only when no clock is available.
pub fn current_year() -> i64 {
    if let Some(ms) = crate::date::unix_now_ms() {
        let days = (ms / 86_400_000.0).floor() as i64;
        return crate::date::civil_from_days(days).0 as i64;
    }
    2026
}

fn parse_datetime(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t) {
        let f = match (secs, ampm) {
            // 24 hours or more: elapsed hours, so 25:00 shows as 25:00:00, not 1:00.
            _ if tf >= 1.0 => "[h]:mm:ss",
            (false, false) => "h:mm",
            (true, false) => "h:mm:ss",
            (false, true) => "h:mm AM/PM",
            (true, true) => "h:mm:ss AM/PM",
        };
        return Some((tf, Some(f)));
    }
    if let Some((d, f)) = parse_date(t, sys) {
        return Some((d, Some(f)));
    }
    // date + time: split at the last space(s) before a time-looking tail.
    for (i, _) in t.match_indices(' ') {
        let (a, b) = (t.get(..i)?, t.get(i + 1..)?);
        if let (Some((d, _)), Some((tf, _, _))) = (parse_date(a, sys), parse_time(b)) {
            return Some((d + tf, Some("m/d/yyyy h:mm")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Parsed {
        parse_input(s, DateSystem::D1900)
    }

    #[test]
    fn month_name_short_year() {
        for s in ["Mar 4, 26", "Mar 4 26", "March 4 26", "4 March 26", "Mar 4, 2026"] {
            assert_eq!(p(s).value, Value::Number(46085.0), "{s}");
        }
        assert_eq!(p("Mar 4, 99").value, p("Mar 4, 1999").value);
        assert_eq!(p("Mar 4, 1776").value, Value::text("Mar 4, 1776"));
        assert_eq!(p("Feb 30, 26").value, Value::text("Feb 30, 26"));
    }

    #[test]
    fn numbers() {
        assert_eq!(p("42").value, Value::Number(42.0));
        assert_eq!(p("-3.5").value, Value::Number(-3.5));
        assert_eq!(p("1,234").format, Some("#,##0"));
        assert_eq!(p("1,234").value, Value::Number(1234.0));
        assert_eq!(p("1,23").value, Value::text("1,23"));
        assert_eq!(p("12%").value, Value::Number(0.12));
        assert_eq!(p("$1,200.50").value, Value::Number(1200.5));
        assert_eq!(p("$1,200.50").format, Some("\"$\"#,##0.00"));
        assert_eq!(p("(100)").value, Value::Number(-100.0));
        assert_eq!(p("1e3").value, Value::Number(1000.0));
        assert_eq!(p("1 1/2").value, Value::Number(1.5));
        assert_eq!(p(".5").value, Value::Number(0.5));
    }

    #[test]
    fn texts_and_specials() {
        assert_eq!(p("hello").value, Value::text("hello"));
        assert_eq!(p("'123").value, Value::text("123"));
        assert_eq!(p("true").value, Value::Bool(true));
        assert_eq!(p("#N/A").value, Value::Error(CellError::NA));
        assert_eq!(p("e").value, Value::text("e"));
        assert_eq!(p("-").value, Value::text("-"));
        assert_eq!(p("1.2.3.4").value, Value::text("1.2.3.4"));
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(p("10/7/2026").value, Value::Number(46302.0));
        assert_eq!(p("2026-10-07").value, Value::Number(46302.0));
        assert_eq!(p("7-Oct-2026").value, Value::Number(46302.0));
        assert_eq!(p("Oct 7, 2026").value, Value::Number(46302.0));
        assert_eq!(p("2/30/2026").value, Value::text("2/30/2026"));
        assert_eq!(p("7/4/1776").value, Value::text("7/4/1776"));
        assert_eq!(p("1776-07-04").value, Value::text("1776-07-04"));
        assert_eq!(p("4-Jul-1776").value, Value::text("4-Jul-1776"));
        assert_eq!(p("12/31/1899").value, Value::text("12/31/1899"));
        assert_eq!(p("1/1/1900").value, Value::Number(1.0));
        assert_eq!(p("12:00").value, Value::Number(0.5));
        assert_eq!(p("6:00 PM").value, Value::Number(0.75));
        assert_eq!(p("12:00 AM").value, Value::Number(0.0));
        assert_eq!(p("10/7/2026 18:00").value, Value::Number(46302.75));
        assert_eq!(p("25:00").value, Value::Number(25.0 / 24.0));
        assert_eq!(p("25:00").format, Some("[h]:mm:ss"));
        assert_eq!(p("23:59").format, Some("h:mm"));
    }

    /// Serial of a date, for expectations that depend on the current year.
    fn ymd(y: i64, m: i64, d: i64) -> Value {
        Value::Number(serial_from_ymd(DateSystem::D1900, y, m, d).unwrap_or(f64::NAN))
    }

    #[test]
    fn month_name_without_separators_or_with_a_year() {
        let y = current_year();
        // Day and month, month and day: the current year (#1342).
        for (s, m, d) in [
            ("1Jan", 1, 1),
            ("1 Jan", 1, 1),
            ("Jan1", 1, 1),
            ("15Mar", 3, 15),
            ("1January", 1, 1),
            ("2 March", 3, 2),
            ("Mar 15", 3, 15),
            ("jan 1", 1, 1),
            ("05Jan", 1, 5),
            ("Jan 05", 1, 5),
            ("1Sept", 9, 1),
            ("Sep1", 9, 1),
            ("March1", 3, 1),
            ("1Marc", 3, 1),
            ("1  Jan", 1, 1),
            ("1 - Jan", 1, 1),
            ("1/Jan", 1, 1),
        ] {
            assert_eq!(p(s).value, ymd(y, m, d), "{s}");
            assert_eq!(p(s).format, Some("d-mmm"), "{s}");
        }
        // Month and year, two or four digits, with or without a space.
        for (s, yy, m) in [
            ("Jan 2021", 2021, 1),
            ("Jan2021", 2021, 1),
            ("March 2024", 2024, 3),
            ("JAN2021", 2021, 1),
            ("Sept 2024", 2024, 9),
            ("Janu 2021", 2021, 1),
            ("Jan-2021", 2021, 1),
            ("Jan/2021", 2021, 1),
            ("Jan  2021", 2021, 1),
            ("Jan 1900", 1900, 1),
            ("Jan 9999", 9999, 1),
        ] {
            assert_eq!(p(s).value, ymd(yy, m, 1), "{s}");
            assert_eq!(p(s).format, Some("mmm-yy"), "{s}");
        }
        // Day, month and year with no separators, or mixed.
        for (s, yy, m, d) in [
            ("1Jan21", 2021, 1, 1),
            ("1Jan2021", 2021, 1, 1),
            ("15Mar2024", 2024, 3, 15),
            ("01Jan2021", 2021, 1, 1),
            ("1Jan1", 2001, 1, 1),
            ("1Jan29", 2029, 1, 1),
            ("1Jan30", 1930, 1, 1),
            ("1Jan00", 2000, 1, 1),
            ("15Jan 2021", 2021, 1, 15),
            ("15 Jan2021", 2021, 1, 15),
            ("1-Jan2021", 2021, 1, 1),
            ("1Jan-2021", 2021, 1, 1),
            ("1/Jan2021", 2021, 1, 1),
            ("1Janua2021", 2021, 1, 1),
            ("29Feb2024", 2024, 2, 29),
            ("31Dec9999", 9999, 12, 31),
            ("4 March 26", 2026, 3, 4),
        ] {
            assert_eq!(p(s).value, ymd(yy, m, d), "{s}");
            assert_eq!(p(s).format, Some("d-mmm-yy"), "{s}");
        }
        assert_eq!(p("1Jan1900").value, Value::Number(1.0));
        assert_eq!(parse_input("1Jan1904", DateSystem::D1904).value, Value::Number(0.0));
        // With a time.
        assert_eq!(p("1Jan21 18:00").value, Value::Number(44197.75));
        assert_eq!(p("Jan2021 6:00 PM").value, Value::Number(44197.75));
    }

    #[test]
    fn month_name_then_a_number_is_a_day_when_the_day_exists() {
        let y = current_year();
        // Excel reads `Jan-21` as 21 January of this year, like `Jan 21` (it was January 2021).
        for s in ["Jan-21", "Jan 21", "Jan21", "Jan/21"] {
            assert_eq!(p(s).value, ymd(y, 1, 21), "{s}");
            assert_eq!(p(s).format, Some("d-mmm"), "{s}");
        }
        assert_eq!(p("Jan 31").value, ymd(y, 1, 31));
        // No such day: a two-digit year instead.
        for (s, yy, m) in [("Jan 32", 1932, 1), ("Jan32", 1932, 1), ("Feb 30", 1930, 2), ("Feb-30", 1930, 2)] {
            assert_eq!(p(s).value, ymd(yy, m, 1), "{s}");
            assert_eq!(p(s).format, Some("mmm-yy"), "{s}");
        }
        for s in ["Jan 0", "Jan00", "Jan 00"] {
            assert_eq!(p(s).value, ymd(2000, 1, 1), "{s}");
        }
        assert_eq!(p("Jan 99").value, ymd(1999, 1, 1));
        // 29 February is a day in leap years and the year 2029 otherwise.
        let leap = crate::date::days_in_month_in(DateSystem::D1900, y as i32, 2) == 29;
        assert_eq!(p("Feb 29").value, if leap { ymd(y, 2, 29) } else { ymd(2029, 2, 1) });
    }

    #[test]
    fn month_name_forms_excel_keeps_as_text() {
        for s in [
            "32Jan",
            "0Jan",
            "30Feb",
            "001Jan",
            "123Jan",
            "Jan",
            "May",
            "Sept",
            "Ma 1",
            "1Ma",
            "Jan 100",
            "Jan 999",
            "Jan 1899",
            "Jan 10000",
            "Jan-001",
            "1Jan100",
            "1Jan10000",
            "29Feb2025",
            "Jan123",
            "Jan152021",
            "Jan15 2021",
            "1Jan,2021",
            "1Jan, 2021",
            "2021Jan",
            "2021 Jan",
            "Janx",
            "1Janx",
            "x1Jan",
            "1Jan1Jan",
            "Jan1Jan",
            "-1Jan",
            "1Jan-",
            "1Mar.",
            "Mar.1",
            "1.Jan",
            "Jan.2021",
            "Jan.21",
            "4.Mar.2026",
            "4 Mar, 2026",
            "1Jan2021x",
            "1 Jan 2021 Jan",
        ] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
        }
        // Long text stays text without slow scanning.
        let long = "1Jan".repeat(10_000);
        assert_eq!(p(&long).value, Value::text(long.as_str()));
        let digits = "1".repeat(10_000) + "Jan";
        assert_eq!(p(&digits).value, Value::text(digits.as_str()));
    }

    #[test]
    fn fictitious_1900_02_29() {
        // Excel accepts 29 February 1900 (serial 60) and keeps 1 March 1900 at 61.
        assert_eq!(p("2/29/1900").value, Value::Number(60.0));
        assert_eq!(p("1900-02-29").value, Value::Number(60.0));
        assert_eq!(p("29-Feb-1900").value, Value::Number(60.0));
        assert_eq!(p("2/28/1900").value, Value::Number(59.0));
        assert_eq!(p("3/1/1900").value, Value::Number(61.0));
        assert_eq!(p("2/29/1901").value, Value::text("2/29/1901"));
        assert_eq!(p("2/30/1900").value, Value::text("2/30/1900"));
        // The 1904 system starts after it.
        assert_eq!(parse_input("2/29/1900", DateSystem::D1904).value, Value::text("2/29/1900"));
        assert_eq!(parse_input("2/29/1904", DateSystem::D1904).value, Value::Number(59.0));
    }
}
