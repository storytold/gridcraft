//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times. [`parse_input`] reads them the en-US way;
//! [`parse_input_in`] reads what someone types in another [`Region`]: its decimal and group
//! separators (`1.234,56`), currency symbol, day-month-year order (`25/09/2026`), month names
//! and AM/PM words.

use crate::date::{DateSystem, serial_from_ymd, time_fraction};
use crate::region::{DateOrder, EN_US, Grouping, Region};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas) the en-US way. Text that isn't a number, date,
/// boolean or error stays text. A leading apostrophe forces text. Files and imports use this.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    parse_input_in(s, sys, &EN_US)
}

/// [`parse_input`] for what a person types in `region`. The formats it returns are canonical
/// format codes (`m/d/yyyy` is the system short date, which the region's display decides).
pub fn parse_input_in(s: &str, sys: DateSystem, region: &Region) -> Parsed {
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
    if let Some((n, f)) = parse_numeric(t, sys, region) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00"). en-US:
/// worksheet text coercion does not follow the region.
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900)
}

/// Like [`parse_number_text`], with date text read in the given workbook date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem) -> Option<f64> {
    parse_numeric(s.trim(), sys, &EN_US).map(|(n, _)| n)
}

fn parse_numeric(t: &str, sys: DateSystem, r: &Region) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    // Parenthesised negatives: (100) = -100.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric(inner.trim(), sys, r).map(|(n, f)| (-n, f));
    }
    if let Some(res) = parse_plain(t, r) {
        return Some(res);
    }
    if let Some(res) = parse_fraction(t, r) {
        return Some((res, Some("# ?/?")));
    }
    if let Some(res) = parse_datetime(t, sys, r) {
        return Some(res);
    }
    None
}

/// Digit groups of an integer part, accepted when they follow the region's grouping.
fn groups_ok(groups: &[&str], grouping: Grouping) -> bool {
    let (Some(first), Some(last)) = (groups.first(), groups.last()) else { return false };
    if first.is_empty() || first.len() > 3 {
        return false;
    }
    let thousands = groups.iter().skip(1).all(|g| g.len() == 3);
    match grouping {
        Grouping::Thousands => thousands,
        // Indian: 12,34,567. The thousands pattern is read too.
        Grouping::Indian => {
            let middle = groups.get(1..groups.len().saturating_sub(1)).unwrap_or(&[]);
            thousands || (first.len() <= 2 && last.len() == 3 && middle.iter().all(|g| g.len() == 2))
        }
    }
}

/// Signs, currency, group separators, percent, exponent.
fn parse_plain(t: &str, r: &Region) -> Option<(f64, Option<&'static str>)> {
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
    if r.currency_first() {
        if let Some(rest) = s.strip_prefix(r.currency) {
            currency = true;
            s = rest.trim_start();
            if !neg && let Some(rest2) = s.strip_prefix('-') {
                neg = true;
                s = rest2;
            }
        }
    } else if let Some(rest) = s.trim_end().strip_suffix(r.currency) {
        currency = true;
        s = rest.trim_end();
    }
    if let Some(rest) = s.strip_suffix('%') {
        percent = true;
        s = rest.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    // Where the decimal is not a point, a point is only valid as a group separator.
    if r.decimal != '.' && s.contains('.') && !r.is_group('.') {
        return None;
    }
    let has_sep = s.chars().any(|c| r.is_group(c));
    let digits: String = if has_sep {
        // Group separators must split the integer part into valid groups.
        let (int, frac) = s.split_once(r.decimal).map_or((s, None), |(a, b)| (a, Some(b)));
        let groups: Vec<&str> = int.split(|c| r.is_group(c)).collect();
        if !groups_ok(&groups, r.grouping) {
            return None;
        }
        let mut d = groups.concat();
        if let Some(f) = frac {
            d.push('.');
            d.push_str(f);
        }
        d
    } else if r.decimal == '.' {
        s.to_string()
    } else {
        s.replacen(r.decimal, ".", 1)
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
    let decimals = digits.contains('.');
    let fmt = if currency {
        Some(if decimals { r.currency_format } else { r.currency_format_int })
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

/// The whole part of a mixed fraction, with the region's separators (`1.000` is a thousand in
/// de-DE). Regions with a point decimal read it as before.
fn whole_part(s: &str, r: &Region) -> Option<f64> {
    if r.decimal == '.' {
        return s.parse().ok();
    }
    match parse_plain(s, r)? {
        (n, None | Some("#,##0" | "#,##0.00")) => Some(n),
        _ => None,
    }
}

/// `1/2` (only with a whole part, else it's a date), `1 1/2`, `0 3/4`.
fn parse_fraction(t: &str, r: &Region) -> Option<f64> {
    let (whole, frac) = t.split_once(' ')?;
    let neg = whole.starts_with('-');
    let w = whole_part(whole.trim_start_matches('-'), r)?;
    let (a, b) = frac.trim().split_once('/')?;
    let a: f64 = a.parse().ok()?;
    let b: f64 = b.parse().ok()?;
    if b == 0.0 || !w.is_finite() {
        return None;
    }
    let v = w + a / b;
    Some(if neg { -v } else { v })
}

/// Strips an AM/PM word from a lowercase time: the region's, `am`/`pm`, or a bare `a`/`p`
/// after the time. CJK regions put the word before the time (`午後3:30`).
fn split_ampm(lower: &str, r: &Region) -> (String, Option<bool>) {
    let region_am = r.am_pm[0].to_lowercase();
    let region_pm = r.am_pm[1].to_lowercase();
    let words: [(&str, bool); 4] = [(&region_am, false), (&region_pm, true), ("am", false), ("pm", true)];
    for (w, pm) in words {
        // Only a non-ASCII word (`午後`) may come first.
        let stripped = lower.strip_suffix(w).or_else(|| if w.is_ascii() { None } else { lower.strip_prefix(w) });
        if let Some(b) = stripped {
            return (b.trim().to_string(), Some(pm));
        }
    }
    if let Some(b) = lower.strip_suffix('a') {
        return (b.trim().to_string(), Some(false));
    }
    if let Some(b) = lower.strip_suffix('p') {
        return (b.trim().to_string(), Some(true));
    }
    (lower.to_string(), None)
}

/// Time part: `h:mm`, `h:mm:ss`, `h:mm:ss.000`, `h AM`, with optional AM/PM.
fn parse_time(t: &str, r: &Region) -> Option<(f64, bool, bool)> {
    let lower = t.trim().to_lowercase();
    let (body, ampm) = split_ampm(&lower, r);
    let body = if r.time_sep == ':' { body } else { body.replace(r.time_sep, ":") };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut h: f64 = parts.first()?.parse().ok()?;
    let m: f64 = parts.get(1).map_or(Some(0.0), |p| p.parse().ok())?;
    let s: f64 = match parts.get(2) {
        None => 0.0,
        Some(p) if r.decimal == '.' => p.parse().ok()?,
        Some(p) => p.replacen(r.decimal, ".", 1).parse().ok()?,
    };
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

/// A two-digit year is 20xx below 30, else 19xx.
fn full_year(y: i64, digits: usize) -> i64 {
    if digits <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y }
}

/// Words that join the parts of a long date in some regions (`25 de septiembre de 2026`).
fn is_connector(w: &str, r: &Region) -> bool {
    let w = w.to_lowercase();
    (matches!(r.tag, "es-ES" | "pt-BR" | "pt-PT") && matches!(w.as_str(), "de" | "del")) || (r.tag == "ru-RU" && matches!(w.as_str(), "г" | "г."))
}

fn parse_date(t: &str, sys: DateSystem, r: &Region) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    let num = |s: &str| s.trim().parse::<i64>().ok();
    let month_name = |s: &str| r.month_from_name(s);
    let seps: &[char] = &['/', '-', '.'];
    let parts: Vec<&str> = t.split(seps).collect();
    if parts.len() == 3 {
        let (a, b, c) = (parts.first()?.trim(), parts.get(1)?.trim(), parts.get(2)?.trim());
        // Year first: ISO yyyy-mm-dd or yyyy/mm/dd.
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            // A year-first region typing its own short date keeps the system short date.
            let own = r.date_order == DateOrder::Ymd && t.contains(r.date_sep) && r.date_sep != '-';
            let fmt = if own { "m/d/yyyy" } else { "yyyy-mm-dd" };
            return valid_ymd(sys, y, m, d).map(|s| (s, fmt));
        }
        if let (Some(x), Some(y), Some(z)) = (num(a), num(b), num(c)) {
            let (m, d, yy, ydigits) = match r.date_order {
                DateOrder::Mdy => (x, y, z, c.len()),
                DateOrder::Dmy => (y, x, z, c.len()),
                DateOrder::Ymd => (y, z, x, a.len()),
            };
            return valid_ymd(sys, full_year(yy, ydigits), m, d).map(|s| (s, "m/d/yyyy"));
        }
        // 4-Mar-2026
        if let (Some(d), Some(m), Some(y)) = (num(a), month_name(b), num(c)) {
            return valid_ymd(sys, full_year(y, c.len()), m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    if parts.len() == 2 {
        let (a, b) = (parts.first()?.trim(), parts.get(1)?.trim());
        // A date without a year is in the current year (what Excel does). A name is accepted
        // with a day or a year.
        if let (Some(x), Some(y)) = (num(a), num(b)) {
            let (m, d) = match r.date_order {
                DateOrder::Dmy => (y, x),
                DateOrder::Mdy | DateOrder::Ymd => (x, y),
            };
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return valid_ymd(sys, current_year(), m, d).map(|s| (s, "d-mmm"));
            }
            // month/yyyy (and yyyy/month in year-first regions).
            if b.len() == 4 && (1..=12).contains(&x) {
                return valid_ymd(sys, y, x, 1).map(|s| (s, "mmm-yy"));
            }
            if a.len() == 4 && r.date_order == DateOrder::Ymd && (1..=12).contains(&y) {
                return valid_ymd(sys, x, y, 1).map(|s| (s, "mmm-yy"));
            }
        }
        if let (Some(d), Some(m)) = (num(a), month_name(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_name(a), num(b)) {
            return valid_ymd(sys, full_year(y, b.len()), m as i64, 1).map(|s| (s, "mmm-yy"));
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026" / "25 de septiembre de 2026" / "25. září 2026"
    let day_dot = r.date_sep == '.';
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty() && !is_connector(w, r)).collect();
    if let [w0, w1, w2] = words.as_slice() {
        let num_w = |w: &str| if day_dot { num(w.trim_end_matches('.')) } else { num(w) };
        // Two-digit years follow the same 2029 rule as the dashed forms above.
        let year_w = |w: &str| num_w(w).map(|y| full_year(y, w.trim_end_matches('.').len()));
        if let (Some(m), Some(d), Some(y)) = (month_name(w0), num_w(w1), year_w(w2)) {
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num_w(w0), month_name(w1), year_w(w2)) {
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

fn parse_datetime(t: &str, sys: DateSystem, r: &Region) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t, r) {
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

    fn pin(region: &str, s: &str) -> Parsed {
        parse_input_in(s, DateSystem::D1900, crate::region::find(region).unwrap())
    }

    #[test]
    fn en_us_is_unchanged_by_the_region_parameter() {
        for s in [
            "1,234.5",
            "$3",
            "3$",
            "9/25/2026",
            "25/09/2026",
            "Sept 25, 2026",
            "25 de septiembre de 2026",
            "Mar. 4, 2026",
            "pm 3:30",
            "3:30 pm",
            "1.234,56",
            "1,5",
        ] {
            assert_eq!(pin("en-US", s), p(s), "{s}");
        }
        assert_eq!(p("25/09/2026").value, Value::text("25/09/2026"));
        assert_eq!(p("3$").value, Value::text("3$"));
        assert_eq!(p("pm 3:30").value, Value::text("pm 3:30"));
        assert_eq!(p("Mar. 4, 2026").value, Value::text("Mar. 4, 2026"));
    }

    #[test]
    fn decimal_comma_regions_read_their_numbers() {
        let n = pin("pt-BR", "1.234,56");
        assert_eq!(n.value, Value::Number(1234.56));
        assert_eq!(n.format, Some("#,##0.00"));
        assert_eq!(pin("pt-BR", "1.234").value, Value::Number(1234.0));
        assert_eq!(pin("pt-BR", "3,5").value, Value::Number(3.5));
        assert_eq!(pin("pt-BR", "3,5").format, None);
        assert_eq!(pin("pt-BR", "12,5%").value, Value::Number(0.125));
        assert_eq!(pin("pt-BR", "-0,5").value, Value::Number(-0.5));
        assert_eq!(pin("pt-BR", "1,5e3").value, Value::Number(1500.0));
        assert_eq!(pin("de-DE", "1,234.5").value, Value::text("1,234.5"));
        assert_eq!(pin("pt-BR", "1,23,4").value, Value::text("1,23,4"));
        assert_eq!(pin("de-DE", "12.34").value.as_f64().map(|_| ()), None, "a point is not a decimal in de-DE");
        // "1.5" is 1 May there, as in Excel, not a number.
        assert_eq!(pin("fr-FR", "1.5").format, Some("d-mmm"));
    }

    #[test]
    fn mixed_fractions_read_their_whole_part_in_the_region() {
        assert_eq!(pin("de-DE", "1.000 1/2").value, Value::Number(1000.5));
        assert_eq!(pin("de-DE", "1,5 1/2").value, Value::Number(2.0));
        assert_eq!(pin("de-DE", "-2 1/4").value, Value::Number(-2.25));
        assert_eq!(pin("ru-RU", "1\u{a0}000 1/2").value, Value::Number(1000.5));
        assert_eq!(pin("en-US", "1 1/2").value, Value::Number(1.5));
        assert_eq!(pin("de-DE", "1.5 1/2").value, Value::text("1.5 1/2"));
    }

    #[test]
    fn group_separators_that_are_spaces() {
        assert_eq!(pin("ru-RU", "1\u{a0}234,5").value, Value::Number(1234.5));
        assert_eq!(pin("ru-RU", "1 234,5").value, Value::Number(1234.5));
        assert_eq!(pin("fr-FR", "1\u{202f}234").value, Value::Number(1234.0));
        assert_eq!(pin("cs-CZ", "12\u{a0}345,67").format, Some("#,##0.00"));
        assert_eq!(pin("ru-RU", "hello world").value, Value::text("hello world"));
        assert_eq!(pin("fr-FR", "1 234 5").value, Value::text("1 234 5"));
    }

    #[test]
    fn indian_grouping() {
        assert_eq!(pin("en-IN", "12,34,567.89").value, Value::Number(1234567.89));
        assert_eq!(pin("en-IN", "1,234,567.89").value, Value::Number(1234567.89));
        assert_eq!(pin("en-IN", "12,34,56").value, Value::text("12,34,56"));
        assert_eq!(pin("en-IN", "₹1,00,000").value, Value::Number(100000.0));
        assert_eq!(pin("en-IN", "₹1,00,000").format, Some("\"₹\"#,##0"));
    }

    #[test]
    fn currency_follows_the_region() {
        let a = pin("pt-BR", "R$ 3,50");
        assert_eq!((a.value, a.format), (Value::Number(3.5), Some("\"R$\" #,##0.00")));
        let b = pin("de-DE", "1.234,50 €");
        assert_eq!((b.value, b.format), (Value::Number(1234.5), Some("#,##0.00 \"€\"")));
        assert_eq!(pin("de-DE", "3 €").format, Some("#,##0 \"€\""));
        assert_eq!(pin("de-DE", "-3 €").value, Value::Number(-3.0));
        assert_eq!(pin("pt-BR", "$3").value, Value::text("$3"));
        assert_eq!(pin("en-GB", "£1,200.50").value, Value::Number(1200.5));
        assert_eq!(pin("cs-CZ", "99\u{a0}Kč").value, Value::Number(99.0));
        assert_eq!(pin("ja-JP", "¥1,000").value, Value::Number(1000.0));
    }

    #[test]
    fn dates_in_the_regions_order() {
        let d = |r: &str, s: &str| pin(r, s).value;
        assert_eq!(d("en-GB", "25/09/2026"), Value::Number(46290.0));
        assert_eq!(d("en-US", "9/25/2026"), Value::Number(46290.0));
        assert_eq!(d("de-DE", "25.09.2026"), Value::Number(46290.0));
        assert_eq!(d("pt-BR", "25/09/2026"), Value::Number(46290.0));
        assert_eq!(d("ru-RU", "25.09.2026"), Value::Number(46290.0));
        assert_eq!(d("cs-CZ", "25. 9. 2026"), Value::Number(46290.0));
        assert_eq!(d("ja-JP", "2026/9/25"), Value::Number(46290.0));
        assert_eq!(d("ko-KR", "2026-09-25"), Value::Number(46290.0));
        assert_eq!(d("zh-TW", "2026/09/25"), Value::Number(46290.0));
        assert_eq!(d("en-IN", "25/09/2026"), Value::Number(46290.0));
        // Day first: 9/25 isn't a date, and 12/1/2026 is 12 January.
        assert_eq!(d("en-GB", "09/25/2026"), Value::text("09/25/2026"));
        assert_eq!(d("en-GB", "12/1/2026"), Value::Number(46034.0));
        assert_eq!(d("en-US", "12/1/2026"), Value::Number(46357.0));
        // Two-digit years, ISO, impossible dates.
        assert_eq!(d("en-GB", "25/09/26"), Value::Number(46290.0));
        assert_eq!(d("en-GB", "2026-09-25"), Value::Number(46290.0));
        assert_eq!(d("de-DE", "31.04.2026"), Value::text("31.04.2026"));
        // The format is the system short date (built-in 14), displayed per region.
        assert_eq!(pin("en-GB", "25/09/2026").format, Some("m/d/yyyy"));
        assert_eq!(pin("ja-JP", "2026/9/25").format, Some("m/d/yyyy"));
        assert_eq!(pin("en-GB", "2026-09-25").format, Some("yyyy-mm-dd"));
        // Day and month of the current year, month and year.
        assert_eq!(pin("de-DE", "25.9").format, Some("d-mmm"));
        assert_eq!(pin("en-GB", "9/2026").format, Some("mmm-yy"));
        assert_eq!(pin("en-GB", "9/2026").value, Value::Number(46266.0));
        assert_eq!(pin("ja-JP", "2026/9").value, Value::Number(46266.0));
    }

    #[test]
    fn month_names_in_local_words() {
        let d = |r: &str, s: &str| pin(r, s).value;
        assert_eq!(d("de-DE", "25. September 2026"), Value::Number(46290.0));
        assert_eq!(d("de-DE", "25-Sep-2026"), Value::Number(46290.0));
        assert_eq!(d("fr-FR", "25 septembre 2026"), Value::Number(46290.0));
        assert_eq!(d("fr-FR", "25 SEPT. 2026"), Value::Number(46290.0));
        assert_eq!(d("es-ES", "25 de septiembre de 2026"), Value::Number(46290.0));
        assert_eq!(d("pt-BR", "25 de setembro de 2026"), Value::Number(46290.0));
        assert_eq!(d("it-IT", "25 Settembre 2026"), Value::Number(46290.0));
        assert_eq!(d("ru-RU", "25 сентября 2026"), Value::Number(46290.0));
        assert_eq!(d("ru-RU", "25 сентября 2026 г."), Value::Number(46290.0));
        assert_eq!(d("cs-CZ", "25. září 2026"), Value::Number(46290.0));
        assert_eq!(d("en-GB", "25 Sept 2026"), Value::Number(46290.0));
        assert_eq!(d("en-GB", "Sep 25, 2026"), Value::Number(46290.0));
        assert_eq!(d("de-DE", "25. Sepxx 2026"), Value::text("25. Sepxx 2026"));
        assert_eq!(pin("de-DE", "25-Sep-2026").format, Some("d-mmm-yy"));
    }

    #[test]
    fn times_in_the_regions_words() {
        let d = |r: &str, s: &str| pin(r, s).value;
        assert_eq!(d("de-DE", "15:30"), Value::Number(15.5 / 24.0));
        assert_eq!(d("de-DE", "3:30 PM"), Value::Number(15.5 / 24.0));
        assert_eq!(d("ja-JP", "午後3:30"), Value::Number(15.5 / 24.0));
        assert_eq!(d("ja-JP", "午後 3:30"), Value::Number(15.5 / 24.0));
        assert_eq!(d("ko-KR", "오후 3:30"), Value::Number(15.5 / 24.0));
        assert_eq!(d("zh-CN", "上午12:00"), Value::Number(0.0));
        assert_eq!(d("es-ES", "3:30 p. m."), Value::Number(15.5 / 24.0));
        assert_eq!(d("cs-CZ", "3:30 odp."), Value::Number(15.5 / 24.0));
        assert_eq!(pin("de-DE", "10:30:15,5").value, Value::Number(time_fraction(10.0, 30.0, 15.5)));
        assert_eq!(d("de-DE", "25.09.2026 15:30"), Value::Number(46290.0 + 15.5 / 24.0));
        assert_eq!(d("ja-JP", "2026/9/25 午後3:30"), Value::Number(46290.0 + 15.5 / 24.0));
        assert_eq!(pin("de-DE", "25.09.2026 15:30").format, Some("m/d/yyyy h:mm"));
        assert_eq!(pin("ja-JP", "午後3:30").format, Some("h:mm AM/PM"));
    }

    #[test]
    fn hostile_input_never_panics_in_any_region() {
        let nasty = [
            "",
            " ",
            "-",
            "()",
            "(",
            "€",
            "R$",
            "%",
            "1,",
            ",1",
            "1,,2",
            ".",
            "..",
            "////",
            "a/b/c",
            "2026//",
            "9999999999999999999/1/1",
            "1e999",
            "\u{0}",
            "٣٫٥",
            "1\u{a0}",
            "午後",
            "午後:",
            "::",
            ":",
            "p",
            "a",
            "de de de",
            "г.",
            "25. 25. 25.",
            "1.2.3.4.5",
            "🙂/🙂/🙂",
            "-€",
            "€-",
            "1 e 3",
        ];
        for r in crate::region::all() {
            for s in nasty {
                let _ = parse_input_in(s, DateSystem::D1900, r);
                let _ = parse_input_in(s, DateSystem::D1904, r);
            }
        }
    }
}
