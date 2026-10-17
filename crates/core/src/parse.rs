//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times. Locale: en-US.

use crate::date::{DateSystem, MONTHS, serial_from_ymd, time_fraction};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Excel's File › Options › Data › Automatic Data Conversion, for entering, pasting and loading
/// text (CSV): which number-like text becomes a number. The default converts everything, like
/// Excel and like GridCraft before the options existed; each switch only keeps more input as text.
/// (Excel's fourth option, continuous letters and numbers to a date, has no counterpart: GridCraft
/// never converts `12JAN2026`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DataConversion {
    /// "Remove leading zeros and convert to a number": off keeps `0123` as text.
    pub remove_leading_zeros: bool,
    /// "Keep first 15 digits of long numbers and display in scientific notation": on converts
    /// every digit string, losing digits past the 15th and showing long ones as `1.24412E+11`.
    /// Off, nothing is cut short or turned into scientific notation: a number converts only when
    /// it has at most [`Self::long_number_digits`] digits and, with the default limit, General
    /// format shows it without scientific notation. Everything else stays text, as written.
    pub keep_first_15_digits: bool,
    /// The digit limit while `keep_first_15_digits` is off: 1–15, default [`GENERAL_DIGITS`] (11),
    /// remembered while the box is on. Above 11 the person allows scientific display for numbers
    /// up to that many digits (15 is where Excel's unchecked box stops); no digit is ever lost.
    pub long_number_digits: u8,
    /// "Convert digits surrounding the letter E to a number in scientific notation": off keeps
    /// `1244121406E6`, and any number written with an exponent, as text.
    pub digits_around_e: bool,
    /// "Remove trailing zeros after the decimal separator", GridCraft's addition to the list: on (the
    /// default, as in Excel) `720.0` shows and saves as `720`. Off, a number written with trailing
    /// decimal zeros gets a number format that shows them (`720.0` → `0.0`, `3.10` → `0.00`), so the
    /// grid shows it as written and a saved CSV writes it back the same. It stays a number.
    pub remove_trailing_zeros: bool,
}

/// `0.0` … `0.000000000000000`: the formats that keep trailing zeros
/// ([`DataConversion::remove_trailing_zeros`] off), by count.
const DECIMALS: [&str; 15] = [
    "0.0",
    "0.00",
    "0.000",
    "0.0000",
    "0.00000",
    "0.000000",
    "0.0000000",
    "0.00000000",
    "0.000000000",
    "0.0000000000",
    "0.00000000000",
    "0.000000000000",
    "0.0000000000000",
    "0.00000000000000",
    "0.000000000000000",
];

/// The same for numbers written without a leading digit (`.50` shows as `.50`, not `0.50`).
const DECIMALS_NO_INT: [&str; 15] = [
    "#.0",
    "#.00",
    "#.000",
    "#.0000",
    "#.00000",
    "#.000000",
    "#.0000000",
    "#.00000000",
    "#.000000000",
    "#.0000000000",
    "#.00000000000",
    "#.000000000000",
    "#.0000000000000",
    "#.00000000000000",
    "#.000000000000000",
];

/// The most digits a number may have and still convert with the long-number box off: past 15,
/// digits would be lost.
pub const MAX_LONG_NUMBER_DIGITS: u8 = 15;

/// The characters General format has for a number (`GENERAL_WIDTH` in gridcraft-numfmt, which
/// checks the two agree): a number that needs more is shown in scientific notation.
pub const GENERAL_DIGITS: u8 = 11;

/// Whether General format shows every digit of `n`: no scientific notation and no rounding. That
/// holds when `n` isn't below 1E-4 (General switches to scientific there) and its shortest exact
/// decimal form fits in [`GENERAL_DIGITS`] characters (gridcraft-numfmt checks this against
/// `format_general_fit`).
pub fn general_shows_exactly(n: f64) -> bool {
    n == 0.0 || (n.is_finite() && n.abs() >= 1e-4 && format!("{n}").len() <= usize::from(GENERAL_DIGITS))
}

impl Default for DataConversion {
    fn default() -> Self {
        DataConversion {
            remove_leading_zeros: true,
            keep_first_15_digits: true,
            long_number_digits: GENERAL_DIGITS,
            digits_around_e: true,
            remove_trailing_zeros: true,
        }
    }
}

impl DataConversion {
    /// Every conversion on, like Excel's "Enable all default data conversions" checkbox. Off,
    /// everything number-like is kept as written.
    pub fn all(&self) -> bool {
        self.remove_leading_zeros && self.keep_first_15_digits && self.digits_around_e && self.remove_trailing_zeros
    }

    /// Whether `plain`, text [`parse_plain`] read as the number `n`, may become one. Digits are
    /// counted as written: leading zeros count, the sign, currency, separators, the decimal point
    /// and the exponent don't.
    fn converts(&self, plain: &str, n: f64) -> bool {
        let (mantissa, exponent) = match plain.split_once(['e', 'E']) {
            Some((m, _)) => (m, true),
            None => (plain, false),
        };
        if exponent && !self.digits_around_e {
            return false;
        }
        let int: String = mantissa.split('.').next().unwrap_or("").chars().filter(char::is_ascii_digit).collect();
        if !self.remove_leading_zeros && int.len() > 1 && int.starts_with('0') {
            return false;
        }
        if self.keep_first_15_digits {
            return true;
        }
        // A hand-edited prefs.json can hold any number: stay within the limits the command allows.
        let max = self.long_number_digits.clamp(1, MAX_LONG_NUMBER_DIGITS);
        // Up to 11 digits a number converts only when General shows it as written: nothing turns
        // into scientific notation (a negative 11-digit number, 0.00001) or gets rounded. A
        // higher limit is the person allowing that for numbers that long. A number written with
        // an exponent already is scientific: the E option decides about it.
        let shown_ok = exponent || max > GENERAL_DIGITS || general_shows_exactly(n);
        mantissa.chars().filter(char::is_ascii_digit).count() <= usize::from(max) && shown_ok
    }

    /// With [`Self::remove_trailing_zeros`] off, the format that shows the plain number `plain` with
    /// the decimal places it was written with, when General would drop trailing zeros. `None` when
    /// General already shows it as written, or past 15 decimals.
    fn decimals_format(&self, plain: &str) -> Option<&'static str> {
        if self.remove_trailing_zeros {
            return None;
        }
        let body = plain.trim_start_matches(['-', '+']).trim_start();
        let (int, frac) = body.split_once('.')?;
        if !frac.ends_with('0') || !frac.bytes().all(|b| b.is_ascii_digit()) || !int.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let table = if int.is_empty() { &DECIMALS_NO_INT } else { &DECIMALS };
        table.get(frac.len().checked_sub(1)?).copied()
    }
}

/// Interprets typed cell input (not formulas). Text that isn't a number, date, boolean or error
/// stays text. A leading apostrophe forces text.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    parse_input_with(s, sys, &DataConversion::default())
}

/// [`parse_input`] under the Automatic Data Conversion options (typed input, paste, CSV import).
pub fn parse_input_with(s: &str, sys: DateSystem, conv: &DataConversion) -> Parsed {
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
    if let Some((n, f)) = parse_numeric(t, sys, conv) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00"). The data
/// conversion options don't apply here: in Excel too, `="0123"+1` is 124 whatever they say.
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900)
}

/// Like [`parse_number_text`], with date text read in the given workbook date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem) -> Option<f64> {
    parse_numeric(s.trim(), sys, &DataConversion::default()).map(|(n, _)| n)
}

fn parse_numeric(t: &str, sys: DateSystem, conv: &DataConversion) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    // Parenthesised negatives: (100) = -100.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric(inner.trim(), sys, conv).map(|(n, f)| (-n, f));
    }
    if let Some((n, format)) = parse_plain(t) {
        // A number the options keep as text is text: it isn't a fraction or date either.
        if !conv.converts(t, n) {
            return None;
        }
        // Currency, percent, separators and exponents bring their own formats.
        return Some((n, format.or_else(|| conv.decimals_format(t))));
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

fn parse_date(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
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
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m, d).map(|s| (s, "m/d/yyyy"));
        }
        // 4-Mar-2026
        if let (Some(d), Some(m), Some(y)) = (num(a), month_from_name(b), num(c)) {
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    if parts.len() == 2 {
        let (a, b) = (parts[0].trim(), parts[1].trim());
        // m/d in the current year is ambiguous without a clock; use month/day of 2026? Excel uses
        // the current year. We accept d-mmm / mmm-d only with month names, and m/d → current year.
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
        if let (Some(d), Some(m)) = (num(a), month_from_name(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_from_name(a), num(b)) {
            let y = if b.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, 1).map(|s| (s, "mmm-yy"));
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026"
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        // Two-digit years follow the same 2029 rule as the dashed forms above.
        let expand = |w: &str, y: i64| if w.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
        if let (Some(m), Some(d), Some(y)) = (month_from_name(words[0]), num(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num(words[0]), month_from_name(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
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

    #[test]
    fn data_conversion_options() {
        let with = |s: &str, conv: DataConversion| parse_input_with(s, DateSystem::D1900, &conv).value;
        let all = DataConversion::default();
        // The default converts as before.
        assert!(all.all());
        assert_eq!(with("0123", all), Value::Number(123.0));
        assert_eq!(with("1244121406E6", all), Value::Number(1.244121406e15));
        assert_eq!(with("12345678901234567890", all), Value::Number(12345678901234567890.0));
        assert_eq!(p("124412140710"), parse_input_with("124412140710", DateSystem::D1900, &all));

        let zeros = DataConversion { remove_leading_zeros: false, ..all };
        for kept in ["0123", "00", "-0123", "$0123", "0,123", "012.5", "(0123)"] {
            assert_eq!(with(kept, zeros), Value::text(kept), "{kept}");
        }
        assert_eq!(with("0", zeros), Value::Number(0.0));
        assert_eq!(with("0.5", zeros), Value::Number(0.5));
        assert_eq!(with("-0.25", zeros), Value::Number(-0.25));
        assert_eq!(with("120", zeros), Value::Number(120.0));
        // Only numbers: dates and times with leading zeros still convert.
        assert_eq!(with("01:30", zeros), Value::Number(1.5 / 24.0));
        assert_eq!(with("2026-01-05", zeros), p("2026-01-05").value);

        let e = DataConversion { digits_around_e: false, ..all };
        for kept in ["1244121406E6", "1144121408E7", "1e3", "1.5E-3", "-2E+5"] {
            assert_eq!(with(kept, e), Value::text(kept), "{kept}");
        }
        assert_eq!(with("1244121406FB", e), Value::text("1244121406FB"));
        assert_eq!(with("720.0", e), Value::Number(720.0));

        // Box off with the default limit (11): nothing is cut short or shown in scientific
        // notation; the 12-digit part codes of a CAD/CAM cut list stay text, exactly as written.
        let off = DataConversion { keep_first_15_digits: false, ..all };
        assert!(!off.all());
        assert_eq!(off.long_number_digits, GENERAL_DIGITS);
        let ids = DataConversion { keep_first_15_digits: false, long_number_digits: 11, ..all };
        assert_eq!(off, ids);
        // Within 11 digits, what General would still show in scientific notation or rounded stays
        // text too.
        for kept in ["-12345678901", "0.00001", "0.0000123", "-0.00005", "12345.678901", "-9999999999.9"] {
            assert_eq!(with(kept, off), Value::text(kept), "{kept}");
        }
        for converted in ["-1234567890", "0.0001", "99999999999", "1234.5678", "0"] {
            assert!(with(converted, off).is_number(), "{converted}");
        }
        // Excel's unchecked box stops at 15: a limit of 15 allows scientific display up to 15
        // digits and still never loses one.
        let excel_off = DataConversion { keep_first_15_digits: false, long_number_digits: 15, ..all };
        assert_eq!(with("1234567890123456", excel_off), Value::text("1234567890123456"));
        assert_eq!(with("123456789012345", excel_off), Value::Number(123456789012345.0));
        assert_eq!(with("0.00001", excel_off), Value::Number(0.00001));
        for kept in ["124412140710", "114412140641", "000000000012", "1,244,121,407,10", "12345678901.5", "-124412140710"] {
            assert_eq!(with(kept, ids), Value::text(kept), "{kept}");
        }
        assert_eq!(with("12345678901", ids), Value::Number(12345678901.0));
        assert_eq!(with("2320.0", ids), Value::Number(2320.0));
        // The limit counts the mantissa only; the exponent is the E option's business.
        assert_eq!(with("1.5E+20", ids), Value::Number(1.5e20));
        // The limit only counts while the box is off.
        assert_eq!(with("124412140710", DataConversion { long_number_digits: 11, ..all }), Value::Number(124412140710.0));
        // Out-of-range limits (a hand-edited prefs.json) are held to 1–15.
        let wild = |n| DataConversion { keep_first_15_digits: false, long_number_digits: n, ..all };
        assert_eq!(with("1234567890123456", wild(200)), Value::text("1234567890123456"));
        assert_eq!(with("7", wild(0)), Value::Number(7.0));
        assert_eq!(with("12", wild(0)), Value::text("12"));
        // A leading apostrophe and arithmetic coercion are unaffected.
        let none = DataConversion {
            remove_leading_zeros: false,
            keep_first_15_digits: false,
            long_number_digits: 1,
            digits_around_e: false,
            remove_trailing_zeros: false,
        };
        assert_eq!(with("'5", none), Value::text("5"));
        assert_eq!(with("TRUE", none), Value::Bool(true));
        assert_eq!(parse_number_text("0123"), Some(123.0));
        assert_eq!(parse_number_text("1E3"), Some(1000.0));
    }

    #[test]
    fn trailing_zeros_kept_when_not_removed() {
        let on = DataConversion { remove_trailing_zeros: false, ..DataConversion::default() };
        let with = |s: &str, conv: &DataConversion| parse_input_with(s, DateSystem::D1900, conv);
        // Trailing decimal zeros get a format that shows them; the value is the same number.
        for (input, n, format) in [
            ("720.0", 720.0, "0.0"),
            ("3.10", 3.1, "0.00"),
            ("-0.50", -0.5, "0.00"),
            ("+1.0", 1.0, "0.0"),
            (".50", 0.5, "#.00"),
            ("-.50", -0.5, "#.00"),
            ("(720.0)", -720.0, "0.0"),
            ("0.000000000000000", 0.0, "0.000000000000000"),
        ] {
            assert_eq!(with(input, &on), Parsed { value: Value::Number(n), format: Some(format) }, "{input}");
        }
        // Nothing to keep, or more decimals than a format can show: General, as before.
        for (input, n) in [("720", 720.0), ("720.5", 720.5), ("720.", 720.0), ("0.0000000000000000", 0.0)] {
            assert_eq!(with(input, &on), Parsed { value: Value::Number(n), format: None }, "{input}");
        }
        // Inputs with their own format keep it.
        assert_eq!(with("$1.50", &on).format, Some("\"$\"#,##0.00"));
        assert_eq!(with("1,234.50", &on).format, Some("#,##0.00"));
        assert_eq!(with("12.50%", &on).format, Some("0.00%"));
        assert_eq!(with("1.50E+3", &on).format, Some("0.00E+00"));
        // Removed by default, as in Excel; keeping them is one of the conversions "all" turns off.
        assert_eq!(p("720.0"), Parsed { value: Value::Number(720.0), format: None });
        assert!(!on.all());
    }

    #[test]
    fn data_conversion_json() {
        // Prefs saved before the options existed (or with some missing) get Excel's defaults.
        let old: DataConversion = serde_json::from_str("{}").unwrap();
        assert_eq!(old, DataConversion::default());
        let some: DataConversion = serde_json::from_str(r#"{"digitsAroundE": false, "keepFirst15Digits": false, "longNumberDigits": 11}"#).unwrap();
        assert_eq!(
            some,
            DataConversion {
                remove_leading_zeros: true,
                keep_first_15_digits: false,
                long_number_digits: 11,
                digits_around_e: false,
                remove_trailing_zeros: true
            }
        );
        assert_eq!(
            serde_json::to_value(some).unwrap(),
            serde_json::json!({"removeLeadingZeros": true, "keepFirst15Digits": false, "longNumberDigits": 11, "digitsAroundE": false, "removeTrailingZeros": true})
        );
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
