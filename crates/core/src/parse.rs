//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times. Locale: en-US.

use crate::date::{DateOrder, DateSystem, MONTHS, serial_from_ymd, time_fraction};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas). Text that isn't a number, date, boolean or error
/// stays text. A leading apostrophe forces text. Defaults to `DateOrder::Mdy`.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    parse_input_with_order(s, sys, DateOrder::Mdy)
}

/// Interprets typed cell input, using the given calendar date order (`Dmy`, `Mdy`, `Ymd`).
pub fn parse_input_with_order(s: &str, sys: DateSystem, order: DateOrder) -> Parsed {
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
    if let Some((n, f)) = parse_numeric(t, sys, order) {
        return Parsed { value: Value::number(n), format: f };
    }
    // Day-first fallback is a convenience for typed cell entry, not arithmetic coercion.
    if order == DateOrder::Mdy
        && let Some((n, f)) = parse_numeric(t, sys, DateOrder::Dmy)
        && f.is_some_and(|f| matches!(f, "dd/mm/yyyy" | "dd-mm-yyyy" | "dd.mm.yyyy"))
    {
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
    parse_number_text_with_order(s, sys, DateOrder::Mdy)
}

/// Like [`parse_number_text_in`], with the given calendar date order.
pub fn parse_number_text_with_order(s: &str, sys: DateSystem, order: DateOrder) -> Option<f64> {
    parse_numeric(s.trim(), sys, order).map(|(n, _)| n)
}

/// Text that Excel's "Number Stored as Text" check flags: it would be a number if typed (signs,
/// thousands separators, currency, percent, exponent, a parenthesised negative). Dates, times and
/// fractions kept as text are not flagged, as in Excel.
pub fn number_stored_as_text(s: &str) -> Option<f64> {
    let t = s.trim();
    // The same parentheses typed entry reads, accounting forms included: `$(5)`, `(5)%`.
    match parenthesized(t) {
        Some(inner) if inner.strip_prefix('$').map_or(inner.as_str(), str::trim_start).starts_with(['-', '+', '(']) => None,
        Some(inner) => parse_plain(&inner).map(|(n, _)| -n),
        None => parse_plain(t).map(|(n, _)| n),
    }
}

fn parse_numeric(t: &str, sys: DateSystem, order: DateOrder) -> Option<(f64, Option<&'static str>)> {
    // Parenthesised negatives: (100) = -100, and the accounting forms `$(100)` and `(5)%`.
    if let Some(inner) = parenthesized(t) {
        // Excel reads no sign and no nested parentheses inside: `(-5)`, `($-5)`, `((5))`.
        if inner.strip_prefix('$').map_or(inner.as_str(), str::trim_start).starts_with(['-', '+', '(']) {
            return None;
        }
        let r = parse_plain(&inner).or_else(|| parse_fraction(&inner).map(|n| (n, Some("# ?/?"))));
        return r.map(|(n, f)| (-n, f));
    }
    parse_unparenthesised(t, sys, order)
}

fn parse_unparenthesised(t: &str, sys: DateSystem, order: DateOrder) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    if let Some(r) = parse_plain(t) {
        return Some(r);
    }
    if let Some(r) = parse_fraction(t) {
        return Some((r, Some("# ?/?")));
    }
    if let Some(r) = parse_datetime(t, sys, order) {
        return Some(r);
    }
    None
}

/// The number inside accounting parentheses, as plain text: `(5)` gives `5`. A dollar sign may
/// stand before them (`$(5)` reads as `($5)`) and a percent sign after them (`(5)%` reads as
/// `(5%)`), with spaces around either. The caller reads only a plain number or a fraction from it.
fn parenthesized(t: &str) -> Option<String> {
    let (currency, rest) = match t.strip_prefix('$') {
        Some(r) => ("$", r.trim_start()),
        None => ("", t),
    };
    let rest = rest.strip_prefix('(')?;
    let (body, percent) = match rest.strip_suffix(')') {
        Some(b) => (b, ""),
        None => (rest.strip_suffix('%')?.trim_end().strip_suffix(')')?, "%"),
    };
    Some(format!("{currency}{}{percent}", body.trim()))
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
    // Excel reads `$5` and `5%` but not `$5%`.
    if s.is_empty() || currency && percent {
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

/// A plain ASCII number: digits, with an optional `.digits` fraction when `frac` is set. Rejects
/// `nan`, `inf`, signs and exponents that `f64::from_str` would accept, and digits too many for a
/// finite number (#2163).
fn plain_number(s: &str, frac: bool) -> Option<f64> {
    let (int, dec) = match s.split_once('.') {
        Some((i, d)) if frac => (i, Some(d)),
        _ => (s, None),
    };
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
    if !digits(int) || dec.is_some_and(|d| !digits(d)) {
        return None;
    }
    s.parse().ok().filter(|x: &f64| x.is_finite())
}

/// `1/2` (only with a whole part, else it's a date), `1 1/2`, `0 3/4`.
fn parse_fraction(t: &str) -> Option<f64> {
    let (whole, frac) = t.split_once(' ')?;
    let neg = whole.starts_with('-');
    let w = plain_number(whole.trim_start_matches('-'), false)?;
    let (a, b) = frac.trim().split_once('/')?;
    let a = plain_number(a, false)?;
    let b = plain_number(b, false)?;
    if b == 0.0 || !w.is_finite() {
        return None;
    }
    let v = w + a / b;
    if !v.is_finite() {
        return None;
    }
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
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || p.starts_with('+')) {
        return None;
    }
    let mut h = plain_number(parts.first()?, false)?;
    let m = parts.get(1).map_or(Some(0.0), |p| plain_number(p, false))?;
    let s = parts.get(2).map_or(Some(0.0), |p| plain_number(p, true))?;
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

/// Determines the calendar date order (`Dmy`, `Mdy`, `Ymd`) from a number format code.
/// Returns `None` if the format does not specify a calendar date order.
pub fn date_order_from_format(code: &str) -> Option<DateOrder> {
    if code.trim().eq_ignore_ascii_case("general") {
        return None;
    }
    let mut in_bracket = false;
    let mut in_quotes = false;
    let mut escape = false;
    let mut first_cal = None;
    let mut second_cal = None;
    let mut has_day_or_year = false;
    let mut after_h = false;

    let chars: Vec<char> = code.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let Some(&c) = chars.get(i) else { break };
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if c == '\\' {
            escape = true;
            i += 1;
            continue;
        }
        if c == '"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if in_quotes {
            i += 1;
            continue;
        }
        if c == '[' {
            in_bracket = true;
            i += 1;
            continue;
        }
        if c == ']' {
            in_bracket = false;
            i += 1;
            continue;
        }
        if in_bracket {
            i += 1;
            continue;
        }
        if c == ';' {
            break;
        }
        let lc = c.to_ascii_lowercase();
        if (lc == 'a' || lc == 'p') && (i + 4 < chars.len()) {
            let tail: String = chars.get(i..i + 5).unwrap_or(&[]).iter().flat_map(|x| x.to_lowercase()).collect();
            if tail == "am/pm" {
                i += 5;
                continue;
            }
        }
        if (lc == 'a' || lc == 'p') && (i + 2 < chars.len()) {
            let tail: String = chars.get(i..i + 3).unwrap_or(&[]).iter().flat_map(|x| x.to_lowercase()).collect();
            if tail == "a/p" {
                i += 3;
                continue;
            }
        }
        if lc == 'g' && i + 6 < chars.len() {
            let tail: String = chars.get(i..i + 7).unwrap_or(&[]).iter().flat_map(|x| x.to_lowercase()).collect();
            if tail == "general" {
                i += 7;
                continue;
            }
        }
        if lc == 'h' {
            after_h = true;
        } else if lc == 'd' || lc == 'y' {
            after_h = false;
        }
        let cal_part = match lc {
            'y' => {
                has_day_or_year = true;
                Some('y')
            }
            'd' => {
                has_day_or_year = true;
                Some('d')
            }
            'm' if !after_h => Some('m'),
            _ => None,
        };
        if let Some(part) = cal_part {
            while chars.get(i + 1).is_some_and(|&d| d.to_ascii_lowercase() == lc) {
                i += 1;
            }
            if first_cal.is_none() {
                first_cal = Some(part);
            } else if second_cal.is_none() && first_cal != Some(part) {
                second_cal = Some(part);
            }
        }
        i += 1;
    }

    if !has_day_or_year {
        return None;
    }

    match (first_cal, second_cal) {
        (Some('d'), _) => Some(DateOrder::Dmy),
        (Some('m'), _) => Some(DateOrder::Mdy),
        (Some('y'), Some('d')) => Some(DateOrder::Dmy),
        (Some('y'), _) => Some(DateOrder::Ymd),
        _ => None,
    }
}

fn parse_date(t: &str, sys: DateSystem, order: DateOrder) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    let num = |s: &str| {
        let s = s.trim();
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<i64>().ok()
    };
    let expand_year = |w: &str, y: i64| if w.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
    // ISO yyyy-mm-dd or yyyy/mm/dd
    let seps: &[char] = &['/', '-', '.'];
    let parts: Vec<&str> = t.split(seps).collect();
    if parts.len() == 3 {
        let a = parts.first()?.trim();
        let b = parts.get(1)?.trim();
        let c = parts.get(2)?.trim();
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
        }
        // 4-Mar-2026
        if let (Some(d), Some(m), Some(y)) = (num(a), month_from_name(b), num(c)) {
            let y = expand_year(c, y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        // Mar-4-2026
        if let (Some(m), Some(d), Some(y)) = (month_from_name(a), num(b), num(c)) {
            let y = expand_year(c, y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(x), Some(y_val), Some(y)) = (num(a), num(b), num(c)) {
            let y = expand_year(c, y);
            if order == DateOrder::Dmy {
                let separator = t.as_bytes().get(a.len()).copied();
                let fmt = match separator {
                    Some(b'.') => "dd.mm.yyyy",
                    Some(b'-') => "dd-mm-yyyy",
                    _ => "dd/mm/yyyy",
                };
                return valid_ymd(sys, y, y_val, x).map(|serial| (serial, fmt));
            }
            let has_dot = t.contains('.');
            let has_dash = t.contains('-');
            let dmy_fmt: &'static str = if has_dot {
                "dd.mm.yyyy"
            } else if has_dash {
                "dd-mm-yyyy"
            } else {
                "dd/mm/yyyy"
            };
            let mdy_fmt: &'static str = "m/d/yyyy";

            if order == DateOrder::Dmy {
                // An explicit DMY cell format makes the interpretation unambiguous.
                return valid_ymd(sys, y, y_val, x).map(|serial| (serial, dmy_fmt));
            } else {
                // Numeric coercion preserves its established month-first interpretation.
                return valid_ymd(sys, y, x, y_val).map(|serial| (serial, mdy_fmt));
            }
        }
    }
    if parts.len() == 2 {
        let a = parts.first()?.trim();
        let b = parts.get(1)?.trim();
        if let (Some(d), Some(m)) = (num(a), month_from_name(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(d)) = (month_from_name(a), num(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_from_name(a), num(b)) {
            let y = expand_year(b, y);
            return valid_ymd(sys, y, m as i64, 1).map(|s| (s, "mmm-yy"));
        }
        if let (Some(x), Some(y_val)) = (num(a), num(b)) {
            let y = current_year();
            // m/yyyy
            if b.len() == 4 && (1..=12).contains(&x) {
                return valid_ymd(sys, y_val, x, 1).map(|s| (s, "mmm-yy"));
            }
            if order == DateOrder::Dmy {
                // Two components in an explicitly formatted DMY cell stay day-first.
                if (1..=12).contains(&y_val)
                    && (1..=31).contains(&x)
                    && let Some(s) = valid_ymd(sys, y, y_val, x)
                {
                    return Some((s, "d-mmm"));
                }
                // Primary: Day = x, Month = y_val
                if (1..=12).contains(&y_val)
                    && (1..=31).contains(&x)
                    && let Some(s) = valid_ymd(sys, y, y_val, x)
                {
                    return Some((s, "d-mmm"));
                }
                // Fallback: Month = x, Day = y_val
                if (1..=12).contains(&x)
                    && (1..=31).contains(&y_val)
                    && let Some(s) = valid_ymd(sys, y, x, y_val)
                {
                    return Some((s, "d-mmm"));
                }
            } else {
                // Primary: Month = x, Day = y_val
                if (1..=12).contains(&x)
                    && (1..=31).contains(&y_val)
                    && let Some(s) = valid_ymd(sys, y, x, y_val)
                {
                    return Some((s, "d-mmm"));
                }
                // Fallback: Day = x, Month = y_val (e.g. 25/9)
                if (1..=12).contains(&y_val)
                    && (1..=31).contains(&x)
                    && let Some(s) = valid_ymd(sys, y, y_val, x)
                {
                    return Some((s, "d-mmm"));
                }
            }
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026"
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        let expand = |w: &str, y: i64| if w.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
        let w0 = words.first()?;
        let w1 = words.get(1)?;
        let w2 = words.get(2)?;
        if let (Some(m), Some(d), Some(y)) = (month_from_name(w0), num(w1), num(w2)) {
            let y = expand(w2, y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num(w0), month_from_name(w1), num(w2)) {
            let y = expand(w2, y);
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

fn parse_datetime(t: &str, sys: DateSystem, order: DateOrder) -> Option<(f64, Option<&'static str>)> {
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
    if let Some((d, f)) = parse_date(t, sys, order) {
        return Some((d, Some(f)));
    }
    // date + time: split at the last space(s) before a time-looking tail. Only splits where the
    // tail could be a time are tried: a long tail can't parse, so this stays linear in the text
    // instead of reparsing every prefix for every space (one long text field quadratic).
    for (i, _) in t.match_indices(' ') {
        let Some(b) = t.get(i + 1..) else { continue };
        let bytes = b.as_bytes();
        if b.len() > 36 || !bytes.first().is_some_and(u8::is_ascii_digit) || !b.contains(':') {
            continue;
        }
        let parsed_date = t.get(..i).and_then(|a| parse_date(a, sys, order));
        let parsed_dmy_date = if order == DateOrder::Mdy { t.get(..i).and_then(|a| parse_date(a, sys, DateOrder::Dmy)) } else { None };
        if let (Some((d, date_fmt)), Some((tf, _, _))) = (parsed_date.or(parsed_dmy_date), parse_time(b)) {
            let fmt = match date_fmt {
                "dd/mm/yyyy" => "dd/mm/yyyy h:mm",
                "dd-mm-yyyy" => "dd-mm-yyyy h:mm",
                "dd.mm.yyyy" => "dd.mm.yyyy h:mm",
                _ if order == DateOrder::Dmy => "dd/mm/yyyy h:mm",
                _ => "m/d/yyyy h:mm",
            };
            return Some((d + tf, Some(fmt)));
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

    /// #2163: digits too many for a finite number don't make one (∞/∞ was NaN).
    #[test]
    fn overlong_digits_stay_text() {
        let big = "9".repeat(400);
        for s in [format!("1 {big}/{big}"), format!("1 {big}/2"), format!("1 1/{big}0"), format!("{big}:00"), format!("1:00:{big}.5")] {
            assert_eq!(p(&s).value, Value::text(s.as_str()), "{s}");
            assert_eq!(parse_number_text(&s), None, "{s}");
        }
        assert_eq!(parse_number_text("1 1/2"), Some(1.5));
    }

    #[test]
    fn overlong_time_fields_stay_text() {
        // #2168: all-digit field lengths are gated before allocation/derivation,
        // so hostile strings never spin the time-derivation machinery.
        let big = "9".repeat(500);
        for s in [format!("{big}:00"), format!("1:00:{big}"), format!("{big}:{big}:{big}"), format!("1:0{big}"), format!("00:00:{big}.1")] {
            assert_eq!(p(&s).value, Value::text(s.as_str()), "{s}");
            assert_eq!(parse_number_text(&s), None, "{s}");
        }
        // Within the gate, times still work.
        assert_eq!(p("01:02:03").value, Value::number(3.0 / 86_400.0 + 1.0 / 24.0 + 2.0 / 1440.0));
        assert_eq!(parse_number_text("13:14:15"), Some(0.5515625));
    }

    #[test]
    fn non_digit_time_and_fraction_parts_stay_text() {
        for s in ["1:nan", "nan:30", "1:30:NaN", "1 nan/2", "1 1/nan", "inf:00", "1e1:00", "+5:00"] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
        }
    }

    #[test]
    fn signed_date_parts_stay_text() {
        for s in ["+3/4/2026", "3/+4/2026", "3/4/+2026", "1/1/2020 +5:00"] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
        }
        assert_eq!(p("3/4/2026").value, Value::Number(46085.0));
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

    /// Observed in Excel 365 (en-US): typed entry, VALUE and `--"text"` agree.
    #[test]
    fn accounting_parentheses() {
        let accepted: [(&str, f64, Option<&str>); 22] = [
            ("$(5)", -5.0, Some("\"$\"#,##0")),
            ("$ (5)", -5.0, Some("\"$\"#,##0")),
            ("$( 5 )", -5.0, Some("\"$\"#,##0")),
            ("$ ( 1,234 )", -1234.0, Some("\"$\"#,##0")),
            ("$(1,234)", -1234.0, Some("\"$\"#,##0")),
            ("$(1,000.50)", -1000.5, Some("\"$\"#,##0.00")),
            ("$(1,234.)", -1234.0, Some("\"$\"#,##0.00")),
            ("$(.5)", -0.5, Some("\"$\"#,##0.00")),
            ("$(0)", 0.0, Some("\"$\"#,##0")),
            (" $(5) ", -5.0, Some("\"$\"#,##0")),
            ("($5)", -5.0, Some("\"$\"#,##0")),
            ("( $ 1,234 )", -1234.0, Some("\"$\"#,##0")),
            ("(5)", -5.0, None),
            ("( 5 )", -5.0, None),
            ("(1,234.5)", -1234.5, Some("#,##0.00")),
            ("(1e-3)", -0.001, Some("0.00E+00")),
            ("(5)%", -0.05, Some("0%")),
            ("( 5 ) %", -0.05, Some("0%")),
            ("(5%)", -0.05, Some("0%")),
            ("(5.)%", -0.05, Some("0.00%")),
            ("(1,234)%", -12.34, Some("0%")),
            ("(1 1/2)", -1.5, Some("# ?/?")),
        ];
        for (s, n, f) in accepted {
            assert_eq!(p(s), Parsed { value: Value::Number(n), format: f }, "{s}");
            assert_eq!(parse_number_text(s), Some(n), "{s}");
        }
        for s in [
            "$5%",
            "$(5%)",
            "$(5)%",
            "($5%)",
            "($5)%",
            "$ ( 5 ) %",
            "(5)%%",
            "(5%)%",
            "((5))",
            "(-5)",
            "(+5)",
            "($-5)",
            "($ -5)",
            "($+5)",
            "(-$5)",
            "$(-5)",
            "$ (-5)",
            "$(+5)",
            "-(5)",
            "-$(5)",
            "+$(5)",
            "$+(5)",
            "$$(5)",
            "$ $(5)",
            "$((5))",
            "($(5))",
            "(-1 1/2)",
            "()",
            "$()",
            "(5)$",
            "$(5)$",
            "(5)(5)",
            "$(5)0",
            "(5)e3",
            "(5)-",
            "(5-)",
            "$(5-)",
            "$(5",
            "$5)",
            "(5",
            "$(1,23)",
            "$(,5)",
            "(5e)",
            "(1/2)",
            "$(1/2)",
            "(3/4/2026)",
            "$(3/4/2026)",
            "(10:30)",
            "$(10:30)",
        ] {
            assert_eq!(p(s).value, Value::text(s), "{s}");
            assert_eq!(parse_number_text(s), None, "{s}");
        }
    }

    #[test]
    fn deeply_nested_parentheses_do_not_overflow() {
        // #2180: coercion used to recurse once per parenthesis and overflow the stack. Excel reads
        // one pair only, so nesting is text now, and it is rejected without recursing.
        let depth = 100_000;
        let s = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(parse_number_text(&s), None);
        assert_eq!(p(&s).value, Value::text(s.as_str()));
        let odd = format!("{}1{}", "(".repeat(depth + 1), ")".repeat(depth + 1));
        assert_eq!(parse_number_text(&odd), None);
        let unbalanced = format!("{}1{}", "(".repeat(depth), ")".repeat(depth - 1));
        assert_eq!(parse_number_text(&unbalanced), None);
        // A single pair still reads.
        assert_eq!(parse_number_text("(100)"), Some(-100.0));
        assert_eq!(parse_number_text("( $1,200.50 )"), Some(-1200.5));
        assert_eq!(parse_number_text("()"), None);
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

    #[test]
    fn numbers_stored_as_text() {
        for (text, n) in [
            ("123", 123.0),
            (" 42 ", 42.0),
            ("-1,000.5", -1000.5),
            ("$3", 3.0),
            ("50%", 0.5),
            ("1E3", 1000.0),
            ("(100)", -100.0),
            ("$(5)", -5.0),
            ("(5)%", -0.05),
        ] {
            assert_eq!(number_stored_as_text(text), Some(n), "{text}");
        }
        // Dates, times, fractions, words and blanks are not flagged.
        for text in ["3/4/2026", "10:30", "1 1/2", "abc", "12abc", "", "  ", "TRUE", "#N/A", "((1))", "(-5)", "$(5)%", "(1/2)"] {
            assert_eq!(number_stored_as_text(text), None, "{text}");
        }
    }

    #[test]
    fn date_order_and_dd_mm_yyyy_input() {
        // Issue #3085: dd/mm/yyyy input format support
        // In DMY mode: 05/04/2026 is 5 April 2026 (serial 46117.0).
        let dmy = parse_input_with_order("05/04/2026", DateSystem::D1900, DateOrder::Dmy);
        assert_eq!(dmy.value, Value::Number(46117.0));
        assert_eq!(dmy.format, Some("dd/mm/yyyy"));

        // In MDY mode: 05/04/2026 is 4 May 2026 (serial 46146.0).
        let mdy = parse_input_with_order("05/04/2026", DateSystem::D1900, DateOrder::Mdy);
        assert_eq!(mdy.value, Value::Number(46146.0));
        assert_eq!(mdy.format, Some("m/d/yyyy"));

        // Unambiguous day > 12: 25/09/2026 is 25 September 2026 in both DMY and MDY (fallback).
        let p_dmy_25 = parse_input_with_order("25/09/2026", DateSystem::D1900, DateOrder::Dmy);
        assert_eq!(p_dmy_25.value, Value::Number(46290.0));
        assert_eq!(p_dmy_25.format, Some("dd/mm/yyyy"));

        let p_mdy_25 = parse_input_with_order("25/09/2026", DateSystem::D1900, DateOrder::Mdy);
        assert_eq!(p_mdy_25.value, Value::Number(46290.0));
        assert_eq!(p_mdy_25.format, Some("dd/mm/yyyy"));
        assert_eq!(parse_number_text_in("25/09/2026", DateSystem::D1900), None);
        assert_eq!(parse_number_text_in("05/04/2026", DateSystem::D1900), Some(46146.0));

        let dmy_datetime = parse_input_with_order("25.09.2026 18:30", DateSystem::D1900, DateOrder::Mdy);
        assert_eq!(dmy_datetime.value, Value::Number(46290.0 + 18.5 / 24.0));
        assert_eq!(dmy_datetime.format, Some("dd.mm.yyyy h:mm"));

        // Dashed and dotted forms
        assert_eq!(parse_input_with_order("25-09-2026", DateSystem::D1900, DateOrder::Dmy).value, Value::Number(46290.0));
        assert_eq!(parse_input_with_order("25-09-2026", DateSystem::D1900, DateOrder::Dmy).format, Some("dd-mm-yyyy"));
        assert_eq!(parse_input_with_order("25.09.2026", DateSystem::D1900, DateOrder::Dmy).value, Value::Number(46290.0));
        assert_eq!(parse_input_with_order("25.09.2026", DateSystem::D1900, DateOrder::Dmy).format, Some("dd.mm.yyyy"));

        // Two-part dates
        let cur_y = current_year();
        let s25_9 = serial_from_ymd(DateSystem::D1900, cur_y, 9, 25).unwrap_or(0.0);
        assert_eq!(parse_input_with_order("25/09", DateSystem::D1900, DateOrder::Dmy).value, Value::Number(s25_9));
        assert_eq!(parse_input_with_order("25/09", DateSystem::D1900, DateOrder::Mdy).value, Value::Number(s25_9));
    }

    #[test]
    fn date_order_detection_from_format_code() {
        assert_eq!(date_order_from_format("dd/mm/yyyy"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("d/m/yyyy"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("dd.mm.yyyy"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("dd-mm-yyyy"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("d-mmm-yy"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("[$-409]dd/mm/yyyy;@"), Some(DateOrder::Dmy));
        assert_eq!(date_order_from_format("m/d/yyyy"), Some(DateOrder::Mdy));
        assert_eq!(date_order_from_format("mm/dd/yyyy"), Some(DateOrder::Mdy));
        assert_eq!(date_order_from_format("yyyy-mm-dd"), Some(DateOrder::Ymd));
        assert_eq!(date_order_from_format("General"), None);
        assert_eq!(date_order_from_format("0.00"), None);
        assert_eq!(date_order_from_format("h:mm:ss AM/PM"), None);
    }
}
