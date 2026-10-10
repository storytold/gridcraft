//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times.
//!
//! The `*_in` functions read the text the way a given region and language write it (decimal and
//! thousands separators, the region's currency symbol, day/month/year order, month names, AM/PM
//! designators, local boolean and error literals). The functions without the suffix use the
//! invariant en-US conventions.

use std::borrow::Cow;

use gridcraft_locale::{DateOrder, INVARIANT, Language, Regional};

use crate::date::{DateSystem, serial_from_ymd, time_fraction};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas) in the invariant en-US conventions. Text that isn't
/// a number, date, boolean or error stays text. A leading apostrophe forces text.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    parse_input_in(s, sys, &INVARIANT.regional, INVARIANT.formula)
}

/// Interprets typed cell input the way `region` and `lang` write it: `1,5`, `R$ 3` and
/// `10/10/2026` in pt-BR, `VERDADEIRO` and `#N/D` for the Portuguese literals. The returned
/// format codes are canonical (the region only changes how they are displayed).
pub fn parse_input_in(s: &str, sys: DateSystem, region: &Regional, lang: &Language) -> Parsed {
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if let Some(b) = lang.parse_bool(t) {
        return Parsed { value: Value::Bool(b), format: None };
    }
    if let Some(e) = parse_error_in(t, lang) {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = parse_numeric(t, sys, region) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// A local error literal (`#N/D`), or a canonical one the language has no local spelling for
/// (`#CIRC!`).
pub fn parse_error_in(t: &str, lang: &Language) -> Option<CellError> {
    if let Some(canonical) = lang.canonical_error(t) {
        return CellError::parse(canonical);
    }
    let e = CellError::parse(t)?;
    lang.errors.iter().all(|(c, _)| !c.eq_ignore_ascii_case(e.as_str())).then_some(e)
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00").
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900, &INVARIANT.regional)
}

/// [`parse_number_text`] in a region's conventions and a workbook's date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem, region: &Regional) -> Option<f64> {
    parse_numeric(s.trim(), sys, region).map(|(n, _)| n)
}

/// A plain number in a region's spelling: sign, currency, thousands separators, percent and
/// exponent, but no fractions, dates or times (number boxes such as a font size or a width).
pub fn parse_plain_number_in(s: &str, region: &Regional) -> Option<f64> {
    let t = s.trim();
    if t.len() > MAX_NUMERIC_TEXT {
        return None;
    }
    parse_plain(t, region).map(|(n, _)| n)
}

/// Longest text that is read as a number or date (Excel's limit for typed numbers). Longer text
/// stays text and fails numeric coercion, before anything is allocated for it.
const MAX_NUMERIC_TEXT: usize = 255;

fn parse_numeric(t: &str, sys: DateSystem, r: &Regional) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() || t.len() > MAX_NUMERIC_TEXT {
        return None;
    }
    // Parenthesised negatives: (100) = -100.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric(inner.trim(), sys, r).map(|(n, f)| (-n, f));
    }
    if let Some(res) = parse_plain(t, r) {
        return Some(res);
    }
    if let Some(res) = parse_fraction(t) {
        return Some((res, Some("# ?/?")));
    }
    if let Some(res) = parse_datetime(t, sys, r) {
        return Some(res);
    }
    None
}

/// The region's currency format without decimals when that is a prefix of the 2-decimal code
/// (`"$"#,##0.00` → `"$"#,##0`); formats with a trailing currency symbol keep their decimals.
fn integer_currency_format(code: &'static str) -> &'static str {
    code.strip_suffix(".00").unwrap_or(code)
}

/// Signs, currency, thousands separators, percent, exponent.
fn parse_plain(t: &str, r: &Regional) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    let mut currency = false;
    let mut percent = false;
    if let Some(rest) = s.strip_prefix('-') {
        neg = true;
        s = rest.trim_start();
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.trim_start();
    }
    if !r.currency.is_empty() {
        if let Some(rest) = s.strip_prefix(r.currency) {
            currency = true;
            s = rest.trim_start();
            if !neg && let Some(rest2) = s.strip_prefix('-') {
                neg = true;
                s = rest2;
            }
        } else if let Some(rest) = s.strip_suffix(r.currency) {
            currency = true;
            s = rest.trim_end();
        }
    }
    if let Some(rest) = s.strip_suffix('%') {
        percent = true;
        s = rest.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    let has_sep = s.chars().any(|c| r.is_group_char(c));
    let digits: String = if has_sep {
        // Thousands separators must be in groups of three before the decimal point.
        let (int, frac) = s.split_once(r.decimal).map_or((s, None), |(a, b)| (a, Some(b)));
        let groups: Vec<&str> = int.split(|c| r.is_group_char(c)).collect();
        let first = groups.first()?;
        let digits_only = |g: &&str| g.bytes().all(|b| b.is_ascii_digit());
        if first.is_empty() || first.len() > 3 || groups.iter().skip(1).any(|g| g.len() != 3) || !groups.iter().all(digits_only) {
            return None;
        }
        let mut d = groups.concat();
        if let Some(f) = frac {
            d.push('.');
            d.push_str(f);
        }
        d
    } else {
        canonical_decimal(s, r.decimal)?
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
    let decimals = digits.contains('.');
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if currency {
        Some(if decimals { r.currency_format } else { integer_currency_format(r.currency_format) })
    } else if percent {
        Some(if decimals { "0.00%" } else { "0%" })
    } else if sci {
        Some("0.00E+00")
    } else if has_sep {
        Some(if decimals { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

/// Rewrites the region's decimal separator as `.`; a literal `.` that is not the decimal
/// separator makes the text invalid.
fn canonical_decimal(s: &str, decimal: char) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == decimal {
            out.push('.');
        } else if c == '.' {
            return None;
        } else {
            out.push(c);
        }
    }
    Some(out)
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

/// Month number of a name or abbreviation in the region's language: a full name, an
/// abbreviation (`jan.`), or at least three letters of a full name (`Sept`).
fn month_from_name(s: &str, r: &Regional) -> Option<u32> {
    let w = s.trim().trim_end_matches('.').to_lowercase();
    if w.is_empty() {
        return None;
    }
    let exact = (0..12).find(|&i| {
        r.months.get(i).is_some_and(|m| m.to_lowercase() == w) || r.months_abbr.get(i).is_some_and(|m| m.trim_end_matches('.').to_lowercase() == w)
    });
    if let Some(i) = exact {
        return Some(i as u32 + 1);
    }
    if w.chars().count() < 3 {
        return None;
    }
    r.months.iter().position(|m| m.to_lowercase().starts_with(&w)).map(|i| i as u32 + 1)
}

/// A number whose decimal separator is the region's (`15,5` → 15.5).
fn local_float(s: &str, decimal: char) -> Option<f64> {
    canonical_decimal(s, decimal)?.parse().ok()
}

/// Removes an AM/PM designator (the region's, or its first letter when that is ASCII) from a
/// lower-cased time; `Some(true)` for PM.
fn split_ampm<'a>(lower: &'a str, r: &Regional) -> (&'a str, Option<bool>) {
    for (i, d) in r.am_pm.iter().enumerate() {
        let d = d.to_lowercase();
        if let Some(b) = lower.strip_suffix(d.as_str()) {
            return (b.trim(), Some(i == 1));
        }
        if !d.is_ascii()
            && let Some(b) = lower.strip_prefix(d.as_str())
        {
            return (b.trim(), Some(i == 1));
        }
    }
    for (i, d) in r.am_pm.iter().enumerate() {
        if let Some(c) = d.chars().next().filter(char::is_ascii_alphabetic)
            && let Some(b) = lower.strip_suffix(c.to_ascii_lowercase())
        {
            return (b.trim(), Some(i == 1));
        }
    }
    (lower, None)
}

/// Time part: `h:mm`, `h:mm:ss`, `h:mm:ss.000`, `h AM`, with optional AM/PM.
fn parse_time(t: &str, r: &Regional) -> Option<(f64, bool, bool)> {
    let lower = t.trim().to_lowercase();
    let (body, ampm) = split_ampm(&lower, r);
    let parts: Vec<&str> = body.split(r.time_sep).collect();
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut h: f64 = local_float(parts.first()?, r.decimal)?;
    let m: f64 = parts.get(1).map_or(Some(0.0), |p| local_float(p, r.decimal))?;
    let s: f64 = parts.get(2).map_or(Some(0.0), |p| local_float(p, r.decimal))?;
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

/// East Asian dates (`2026年10月10日`, `2026년 10월 10일`) read as `2026/10/10`.
fn normalize_cjk_date(t: &str) -> Cow<'_, str> {
    if !t.contains(['年', '月', '日', '년', '월', '일']) {
        return Cow::Borrowed(t);
    }
    Cow::Owned(
        t.chars()
            .filter_map(|c| match c {
                '年' | '月' | '년' | '월' => Some('/'),
                '日' | '일' => None,
                c => Some(c),
            })
            .collect(),
    )
}

/// Short words the region's long date format puts between date parts (`de` in
/// `d "de" mmmm "de" yyyy`), lower-cased.
fn date_fillers(r: &Regional) -> Vec<String> {
    r.long_date
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|w| w.chars().count() <= 3 && w.chars().all(char::is_alphabetic))
        .map(str::to_lowercase)
        .collect()
}

fn two_digit_year(y: i64, text: &str) -> i64 {
    if text.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y }
}

fn parse_date(t: &str, sys: DateSystem, r: &Regional) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    // Month names can contain the CJK date characters (`10月`), so they are tried before those
    // characters are rewritten as separators.
    if let Some(d) = parse_date_text(t, sys, r) {
        return Some(d);
    }
    match normalize_cjk_date(t) {
        Cow::Owned(n) => parse_date_text(&n, sys, r),
        Cow::Borrowed(_) => None,
    }
}

/// Splits a date on its separators, keeping the period of an abbreviated month name (`oct.`)
/// with its word.
fn split_date(t: &str, date_sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut after_letter = false;
    for (i, c) in t.char_indices() {
        if (matches!(c, '/' | '-' | '.') || c == date_sep) && !(c == '.' && after_letter) {
            parts.push(t.get(start..i).unwrap_or(""));
            start = i + c.len_utf8();
        }
        after_letter = c.is_alphabetic();
    }
    parts.push(t.get(start..).unwrap_or(""));
    parts
}

fn parse_date_text(t: &str, sys: DateSystem, r: &Regional) -> Option<(f64, &'static str)> {
    let plain: Vec<&str> = t.split(['/', '-', '.', r.date_sep]).collect();
    if let Some(d) = date_from_parts(&plain, sys, r) {
        return Some(d);
    }
    if t.contains('.') {
        let kept = split_date(t, r.date_sep);
        if kept != plain
            && let Some(d) = date_from_parts(&kept, sys, r)
        {
            return Some(d);
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026" / "10 de outubro de 2026"
    let fillers = date_fillers(r);
    let words: Vec<&str> =
        t.split([' ', ',']).map(|w| w.trim_end_matches('.')).filter(|w| !w.is_empty() && !fillers.iter().any(|f| *f == w.to_lowercase())).collect();
    if let [w0, w1, w2] = words[..] {
        if let (Some(m), Some(d), Some(y)) = (month_from_name(w0, r), num(w1), num(w2)) {
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num(w0), month_from_name(w1, r), num(w2)) {
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    None
}

fn num(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// A date written as two or three parts split on its separators.
fn date_from_parts(parts: &[&str], sys: DateSystem, r: &Regional) -> Option<(f64, &'static str)> {
    // ISO yyyy-mm-dd or yyyy/mm/dd
    if let [a, b, c] = *parts {
        let (a, b, c) = (a.trim(), b.trim(), c.trim());
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
        }
        let (ys, ms, ds) = match r.date_order {
            DateOrder::Mdy => (c, a, b),
            DateOrder::Dmy => (c, b, a),
            DateOrder::Ymd => (a, b, c),
        };
        if let (Some(m), Some(d), Some(y)) = (num(ms), num(ds), num(ys)) {
            return valid_ymd(sys, two_digit_year(y, ys), m, d).map(|s| (s, "m/d/yyyy"));
        }
        // 4-Mar-2026
        let (ds, mn, ys) = if r.date_order == DateOrder::Ymd { (c, b, a) } else { (a, b, c) };
        if let (Some(d), Some(m), Some(y)) = (num(ds), month_from_name(mn, r), num(ys)) {
            return valid_ymd(sys, two_digit_year(y, ys), m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    if let [a, b] = *parts {
        let (a, b) = (a.trim(), b.trim());
        // m/d in the current year is ambiguous without a clock; use month/day of 2026? Excel uses
        // the current year. We accept d-mmm / mmm-d only with month names, and m/d → current year.
        if let (Some(x), Some(y)) = (num(a), num(b)) {
            let (m, d) = if r.date_order == DateOrder::Dmy { (y, x) } else { (x, y) };
            let year = current_year();
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return valid_ymd(sys, year, m, d).map(|s| (s, "d-mmm"));
            }
            // m/yyyy
            if (1..=12).contains(&x) && b.len() == 4 {
                return valid_ymd(sys, y, x, 1).map(|s| (s, "mmm-yy"));
            }
        }
        if let (Some(d), Some(m)) = (num(a), month_from_name(b, r)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_from_name(a, r), num(b)) {
            return valid_ymd(sys, two_digit_year(y, b), m as i64, 1).map(|s| (s, "mmm-yy"));
        }
    }
    None
}

fn valid_ymd(sys: DateSystem, y: i64, m: i64, d: i64) -> Option<f64> {
    if !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    let max = crate::date::days_in_month_in(sys, y as i32, m as u32) as i64;
    if d > max {
        return None;
    }
    serial_from_ymd(sys, y, m, d)
}

/// The year used for dates typed without one. Fixed per process from the system clock when
/// available (wasm without clock falls back to 2026).
#[allow(clippy::disallowed_methods)] // the clock is read only off wasm
pub fn current_year() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            let days = (d.as_secs() / 86400) as i64;
            return crate::date::civil_from_days(days).0 as i64;
        }
    }
    2026
}

fn parse_datetime(t: &str, sys: DateSystem, r: &Regional) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t, r) {
        let f = match (secs, ampm) {
            (false, false) => "h:mm",
            (true, false) => "h:mm:ss",
            (false, true) => "h:mm AM/PM",
            (true, true) => "h:mm:ss AM/PM",
        };
        return Some((tf, Some(f)));
    }
    if let Some((d, f)) = parse_date(t, sys, r) {
        return Some((d, Some(f)));
    }
    // date + time: split at the last space(s) before a time-looking tail.
    for (i, _) in t.match_indices(' ') {
        let (a, b) = (t.get(..i)?, t.get(i + 1..)?);
        if let (Some((d, _)), Some((tf, _, _))) = (parse_date(a, sys, r), parse_time(b, r)) {
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

    fn pt(s: &str) -> Parsed {
        let (Some(region), Some(lang)) = (gridcraft_locale::region("pt-BR"), gridcraft_locale::language("pt-BR")) else {
            panic!("pt-BR locale data missing")
        };
        parse_input_in(s, DateSystem::D1900, region, lang)
    }

    fn de(s: &str) -> Parsed {
        let (Some(region), Some(lang)) = (gridcraft_locale::region("de-DE"), gridcraft_locale::language("de-DE")) else {
            panic!("de-DE locale data missing")
        };
        parse_input_in(s, DateSystem::D1900, region, lang)
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
        assert_eq!(p("$3").format, Some("\"$\"#,##0"));
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
        assert_eq!(p("12:00").value, Value::Number(0.5));
        assert_eq!(p("6:00 PM").value, Value::Number(0.75));
        assert_eq!(p("12:00 AM").value, Value::Number(0.0));
        assert_eq!(p("10/7/2026 18:00").value, Value::Number(46302.75));
        assert_eq!(p("25:00").value, Value::Number(25.0 / 24.0));
    }

    #[test]
    fn pt_br_numbers() {
        assert_eq!(pt("1,5").value, Value::Number(1.5));
        assert_eq!(pt("1.234,56").value, Value::Number(1234.56));
        assert_eq!(pt("1.234,56").format, Some("#,##0.00"));
        assert_eq!(pt("1.234").value, Value::Number(1234.0));
        assert_eq!(pt("1,23.4").value, Value::text("1,23.4"));
        assert_eq!(pt("12,5%").value, Value::Number(0.125));
        assert_eq!(pt("(1,5)").value, Value::Number(-1.5));
        assert_eq!(pt("-R$ 1.200,50").value, Value::Number(-1200.5));
        // en-US spellings are not numbers in pt-BR.
        assert_eq!(pt("$3").value, Value::text("$3"));
    }

    #[test]
    fn pt_br_currency() {
        let r = pt("R$ 3");
        assert_eq!(r.value, Value::Number(3.0));
        assert_eq!(r.format, Some("\"R$\" #,##0"));
        let r = pt("R$ 3,5");
        assert_eq!(r.value, Value::Number(3.5));
        assert_eq!(r.format, Some("\"R$\" #,##0.00"));
    }

    #[test]
    fn pt_br_dates_and_times() {
        assert_eq!(pt("10/10/2026").value, Value::Number(46305.0));
        assert_eq!(pt("10/10/2026").value, p("2026-10-10").value);
        assert_eq!(pt("10/10/2026").format, Some("m/d/yyyy"));
        assert_eq!(pt("31/12/2026").value, p("12/31/2026").value);
        assert_eq!(pt("12/31/2026").value, Value::text("12/31/2026"));
        assert_eq!(pt("10 de outubro de 2026").value, p("2026-10-10").value);
        assert_eq!(pt("10-out-2026").value, p("2026-10-10").value);
        assert_eq!(pt("10/10/2026 18:00").value, Value::Number(46305.75));
        assert_eq!(pt("12:30:15,5").format, Some("h:mm:ss"));
        assert_eq!(pt("18:00").value, Value::Number(0.75));
        assert_eq!(pt("6:00 PM").value, Value::Number(0.75));
    }

    #[test]
    fn pt_br_literals() {
        assert_eq!(pt("VERDADEIRO").value, Value::Bool(true));
        assert_eq!(pt("falso").value, Value::Bool(false));
        assert_eq!(pt("TRUE").value, Value::text("TRUE"));
        assert_eq!(pt("#N/D").value, Value::Error(CellError::NA));
        assert_eq!(pt("#nome?").value, Value::Error(CellError::Name));
        assert_eq!(pt("#N/A").value, Value::text("#N/A"));
    }

    #[test]
    fn de_de_input() {
        assert_eq!(de("1.234,5").value, Value::Number(1234.5));
        assert_eq!(de("31.12.2026").value, p("12/31/2026").value);
        assert_eq!(de("31.12.2026").format, Some("m/d/yyyy"));
        assert_eq!(de("1,5").value, Value::Number(1.5));
        assert_eq!(de("3 €").value, Value::Number(3.0));
        assert_eq!(de("10. Oktober 2026").value, p("2026-10-10").value);
        assert_eq!(de("WAHR").value, Value::Bool(true));
    }

    #[test]
    fn spaces_and_asian_dates() {
        let (Some(fr), Some(ja)) = (gridcraft_locale::region("fr-FR"), gridcraft_locale::region("ja-JP")) else { panic!("region data missing") };
        assert_eq!(parse_number_text_in("1\u{202f}234,5", DateSystem::D1900, fr), Some(1234.5));
        assert_eq!(parse_number_text_in("1 234,5", DateSystem::D1900, fr), Some(1234.5));
        assert_eq!(parse_number_text_in("1.5", DateSystem::D1900, fr), parse_number_text_in("1/5", DateSystem::D1900, fr));
        let ymd = Some(46305.0);
        assert_eq!(parse_number_text_in("2026年10月10日", DateSystem::D1900, ja), ymd);
        assert_eq!(parse_number_text_in("2026/10/10", DateSystem::D1900, ja), ymd);
        assert_eq!(parse_number_text_in("午後 6:00", DateSystem::D1900, ja), Some(0.75));
    }

    #[test]
    fn grouped_numbers_need_digit_groups_and_the_regional_decimal() {
        let Some(fr) = gridcraft_locale::region("fr-FR") else { panic!("fr-FR region missing") };
        let num = |s: &str| parse_number_text_in(s, DateSystem::D1900, fr);
        assert_eq!(num("1 234,5"), Some(1234.5));
        assert_eq!(num("1 2.3"), None);
        assert_eq!(num("1 234.5"), None);
        assert_eq!(num("1 23"), None);
        assert_eq!(num("1 2a3"), None);
    }

    #[test]
    fn named_months_keep_their_periods_and_cjk_characters() {
        let region = |tag: &str| gridcraft_locale::region(tag).unwrap_or_else(|| panic!("{tag} region missing"));
        let ymd = Some(46305.0);
        let at = |s: &str, tag: &str| parse_number_text_in(s, DateSystem::D1900, region(tag));
        assert_eq!(at("10-oct.-2026", "fr-FR"), ymd);
        assert_eq!(at("10 oct. 2026", "fr-FR"), ymd);
        assert_eq!(at("10/10/2026", "fr-FR"), ymd);
        assert_eq!(at("10.Oct.2026", "en-US"), ymd);
        assert_eq!(at("2026-10月-10", "ja-JP"), ymd);
        assert_eq!(at("2026年10月10日", "ja-JP"), ymd);
        assert_eq!(at("2026-十月-10", "zh-CN"), ymd);
        assert_eq!(at("2026年10月10日", "zh-CN"), ymd);
    }

    #[test]
    fn overlong_text_is_never_a_number() {
        let long = "1".repeat(300);
        assert_eq!(parse_number_text(&long), None);
        assert_eq!(p(&long).value, Value::text(long.as_str()));
        let spaced = format!("1{}", " 000".repeat(100));
        assert_eq!(parse_number_text_in(&spaced, DateSystem::D1900, &INVARIANT.regional), None);
        assert_eq!(parse_number_text(&"1".repeat(255)), Some(1.111_111_111_111_111_1e254));
    }

    #[test]
    fn number_text_in_uses_the_region() {
        let Some(pt_br) = gridcraft_locale::region("pt-BR") else { panic!("pt-BR region missing") };
        assert_eq!(parse_number_text_in("1,5", DateSystem::D1900, pt_br), Some(1.5));
        assert_eq!(parse_number_text_in("1.5", DateSystem::D1900, &INVARIANT.regional), Some(1.5));
        assert_eq!(parse_number_text("1,5"), None);
        // The date system is honoured by the localized path.
        assert_eq!(parse_number_text_in("1/1/1904", DateSystem::D1904, &INVARIANT.regional), Some(0.0));
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
        // The regional paths agree: day/month/year in pt-BR, the 1904 system and the leap rule.
        assert_eq!(pt("29/02/1900").value, Value::Number(60.0));
        assert_eq!(pt("29 de fevereiro de 1900").value, Value::Number(60.0));
        assert_eq!(pt("29/02/1901").value, Value::text("29/02/1901"));
        assert_eq!(parse_number_text_in("2/29/1900", DateSystem::D1900, &INVARIANT.regional), Some(60.0));
        assert_eq!(parse_number_text_in("2/29/1900", DateSystem::D1904, &INVARIANT.regional), None);
    }
}
