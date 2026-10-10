//! Interface locales: how numbers, dates, booleans and errors are written in a language.
//!
//! Stored data is always en-US: formulas, number format codes, files, and every result the CLI,
//! MCP and control channel return. A locale only changes what people see and type in the app,
//! the way Excel's German edition shows `=SUMME(A1;2,5)` and `1.234,56` for a file that stores
//! `=SUM(A1,2.5)` and `1234.56`.

use crate::date::DateSystem;
use crate::parse::{Parsed, current_year, parse_input, parse_time, valid_ymd};
use crate::value::{CellError, Value};

/// A display and input locale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Locale {
    /// en-US: the storage format.
    #[default]
    EnUs,
    /// German (Germany, Austria, Switzerland share it here).
    De,
}

const DE_MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const DE_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mrz", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
const DE_DAYS: [&str; 7] = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"];
const DE_DAYS_SHORT: [&str; 7] = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];

impl Locale {
    /// A language tag (`de`, `de-AT`, `en-US` …). Languages without a locale here are en-US.
    pub fn from_tag(tag: &str) -> Locale {
        if tag.split(['-', '_']).next().is_some_and(|p| p.eq_ignore_ascii_case("de")) { Locale::De } else { Locale::EnUs }
    }

    pub fn is_en(self) -> bool {
        self == Locale::EnUs
    }

    /// Decimal separator.
    pub fn decimal(self) -> char {
        match self {
            Locale::EnUs => '.',
            Locale::De => ',',
        }
    }

    /// Thousands separator.
    pub fn group(self) -> char {
        match self {
            Locale::EnUs => ',',
            Locale::De => '.',
        }
    }

    /// Separator between function arguments (and the union operator).
    pub fn list_separator(self) -> char {
        match self {
            Locale::EnUs => ',',
            Locale::De => ';',
        }
    }

    /// Separator between the columns of an array constant (`{1,2;3,4}` in en-US).
    pub fn array_column_separator(self) -> char {
        match self {
            Locale::EnUs => ',',
            Locale::De => '.',
        }
    }

    pub fn bool_name(self, b: bool) -> &'static str {
        match (self, b) {
            (Locale::EnUs, true) => "TRUE",
            (Locale::EnUs, false) => "FALSE",
            (Locale::De, true) => "WAHR",
            (Locale::De, false) => "FALSCH",
        }
    }

    /// The boolean a word names in this locale (any case).
    pub fn parse_bool(self, s: &str) -> Option<bool> {
        [true, false].into_iter().find(|b| s.eq_ignore_ascii_case(self.bool_name(*b)) || s.to_uppercase() == self.bool_name(*b))
    }

    pub fn error_name(self, e: CellError) -> &'static str {
        match self {
            Locale::EnUs => e.as_str(),
            Locale::De => match e {
                CellError::Value => "#WERT!",
                CellError::Ref => "#BEZUG!",
                CellError::Num => "#ZAHL!",
                CellError::NA => "#NV",
                CellError::GettingData => "#DATEN_ABRUFEN",
                CellError::Spill => "#ÜBERLAUF!",
                CellError::Calc => "#KALK!",
                other => other.as_str(),
            },
        }
    }

    /// The error a literal names in this locale (any case), e.g. `#NV`.
    pub fn parse_error(self, s: &str) -> Option<CellError> {
        let u = s.to_uppercase();
        CellError::ALL.iter().chain([CellError::Circ].iter()).find(|e| self.error_name(**e) == u).copied()
    }

    /// Month name (`m` 1–12); `short` is the `mmm` abbreviation.
    pub fn month_name(self, m: u32, short: bool) -> &'static str {
        let i = (m.clamp(1, 12) - 1) as usize;
        match (self, short) {
            (Locale::EnUs, false) => crate::date::MONTHS.get(i).copied().unwrap_or(""),
            (Locale::EnUs, true) => crate::date::MONTHS.get(i).and_then(|m| m.get(..3)).unwrap_or(""),
            (Locale::De, false) => DE_MONTHS.get(i).copied().unwrap_or(""),
            (Locale::De, true) => DE_MONTHS_SHORT.get(i).copied().unwrap_or(""),
        }
    }

    /// Weekday name (`d` 0 = Sunday); `short` is the `ddd` abbreviation.
    pub fn day_name(self, d: u32, short: bool) -> &'static str {
        let i = (d % 7) as usize;
        match (self, short) {
            (Locale::EnUs, false) => crate::date::WEEKDAYS.get(i).copied().unwrap_or(""),
            (Locale::EnUs, true) => crate::date::WEEKDAYS.get(i).and_then(|d| d.get(..3)).unwrap_or(""),
            (Locale::De, false) => DE_DAYS.get(i).copied().unwrap_or(""),
            (Locale::De, true) => DE_DAYS_SHORT.get(i).copied().unwrap_or(""),
        }
    }

    /// Writes an en-US number literal (`-1234.5`, `1.5E+3`) with this locale's decimal separator.
    pub fn number_literal(self, en: &str) -> String {
        match self {
            Locale::EnUs => en.to_string(),
            Locale::De => en.replace('.', ","),
        }
    }

    /// Interprets typed cell input the way Excel does in this locale. Formulas are not handled
    /// here (see `gridcraft_formula::locale`).
    pub fn parse_input(self, s: &str, sys: DateSystem) -> Parsed {
        match self {
            Locale::EnUs => parse_input(s, sys),
            Locale::De => parse_input_de(s, sys),
        }
    }
}

/// German cell input: `1,5`, `1.234,56`, `12,5 %`, `12,50 €`, `10.10.2026`, `10.10.`, `1.5`
/// (a date, as in Excel), `10. Okt 2026`, times, `WAHR`, `#NV`.
fn parse_input_de(s: &str, sys: DateSystem) -> Parsed {
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if let Some(b) = Locale::De.parse_bool(t) {
        return Parsed { value: Value::Bool(b), format: None };
    }
    if let Some(e) = Locale::De.parse_error(t) {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = numeric_de(t, sys) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

fn numeric_de(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return numeric_de(inner.trim(), sys).map(|(n, f)| (-n, f));
    }
    // Dates come first: `1.5` is the 1st of May in German Excel, not one and a half.
    if let Some(r) = datetime_de(t, sys) {
        return Some(r);
    }
    if let Some(r) = plain_de(t) {
        return Some(r);
    }
    // `1 1/2` and times read the same as in en-US.
    match parse_input(t, sys).value {
        Value::Number(n) if t.contains([' ', ':']) && !t.contains(['.', ',']) => Some((n, parse_input(t, sys).format)),
        _ => None,
    }
}

/// Signs, `€`, thousands dots, decimal comma, `%`, exponent.
fn plain_de(t: &str) -> Option<(f64, Option<&'static str>)> {
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
    if let Some(r) = s.strip_prefix('€') {
        currency = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_suffix('€') {
        currency = true;
        s = r.trim_end();
    }
    if !neg && let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    }
    if let Some(r) = s.strip_suffix('%') {
        percent = true;
        s = r.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    let (int, frac) = match s.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let grouped = int.contains('.');
    let int_digits = if grouped {
        let groups: Vec<&str> = int.split('.').collect();
        let first = groups.first()?;
        if first.is_empty() || first.len() > 3 || groups.iter().skip(1).any(|g| g.len() != 3) {
            return None;
        }
        groups.concat()
    } else {
        int.to_string()
    };
    let mut en = int_digits;
    if let Some(f) = frac {
        en.push('.');
        en.push_str(f);
    }
    let b = en.as_bytes();
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    if !b.first().is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
        return None;
    }
    let mut n: f64 = en.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    let sci = en.contains(['e', 'E']);
    let decimals = frac.is_some_and(|f| !f.is_empty());
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if currency {
        Some(if decimals { "#,##0.00 \"€\"" } else { "#,##0 \"€\"" })
    } else if percent {
        Some(if decimals { "0.00%" } else { "0%" })
    } else if sci {
        Some("0.00E+00")
    } else if grouped {
        Some(if decimals { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

fn month_from_german(s: &str) -> Option<u32> {
    let l = s.trim_end_matches('.').to_lowercase();
    if l.len() < 3 {
        return None;
    }
    let full = DE_MONTHS.iter().position(|m| m.to_lowercase().starts_with(&l));
    let short = DE_MONTHS_SHORT.iter().position(|m| m.to_lowercase() == l);
    full.or(short).map(|i| i as u32 + 1)
}

fn two_digit_year(y: i64, len: usize) -> i64 {
    if len <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y }
}

/// `10.10.2026`, `10.10.26`, `10.10.`, `10.10`, `10/10/2026`, `2026-10-10`, `10. Okt 2026`,
/// `Okt 26`, with an optional time after a space.
fn datetime_de(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if let Some((d, rest)) = t.split_once(' ')
        && rest.contains(':')
        && let Some((date, _)) = date_de(d.trim(), sys)
        && let Some((time, _, _)) = parse_time(rest)
    {
        return Some((date + time, Some("m/d/yyyy h:mm")));
    }
    date_de(t, sys).map(|(n, f)| (n, Some(f)))
}

fn date_de(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let num = |s: &str| if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<i64>().ok() } else { None };
    // Day-first numeric dates with `.`, `/` or `-`; ISO year-first.
    for sep in ['.', '/', '-'] {
        let parts: Vec<&str> = t.split(sep).map(str::trim).collect();
        match parts.as_slice() {
            [a, b, c] if a.len() == 4 => {
                if let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c)) {
                    return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
                }
            }
            [a, b, c] if !c.is_empty() => {
                if let (Some(d), Some(m), Some(y)) = (num(a), num(b), num(c)) {
                    return valid_ymd(sys, two_digit_year(y, c.len()), m, d).map(|s| (s, "m/d/yyyy"));
                }
            }
            // `10.10.` and `10.10`: the current year.
            [a, b, c] if c.is_empty() && sep == '.' => {
                if let (Some(d), Some(m)) = (num(a), num(b)) {
                    return valid_ymd(sys, current_year(), m, d).map(|s| (s, "d-mmm"));
                }
            }
            [a, b] if sep == '.' && b.len() <= 2 => {
                if let (Some(d), Some(m)) = (num(a), num(b)) {
                    return valid_ymd(sys, current_year(), m, d).map(|s| (s, "d-mmm"));
                }
            }
            _ => {}
        }
    }
    // Month names: `10. Okt 2026`, `10. Oktober 2026`, `10. Okt`, `Okt 26`, `Oktober 2026`.
    let words: Vec<&str> = t.split([' ', '-']).filter(|w| !w.is_empty()).collect();
    let day = |w: &str| num(w.trim_end_matches('.'));
    match words.as_slice() {
        [d, m, y] => {
            if let (Some(d), Some(m), Some(yy)) = (day(d), month_from_german(m), num(y)) {
                return valid_ymd(sys, two_digit_year(yy, y.len()), m as i64, d).map(|s| (s, "d-mmm-yy"));
            }
        }
        [a, b] => {
            if let (Some(d), Some(m)) = (day(a), month_from_german(b)) {
                return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
            }
            if let (Some(m), Some(yy)) = (month_from_german(a), num(b)) {
                return valid_ymd(sys, two_digit_year(yy, b.len()), m as i64, 1).map(|s| (s, "mmm-yy"));
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn de(s: &str) -> Parsed {
        Locale::De.parse_input(s, DateSystem::D1900)
    }

    fn num(s: &str) -> Option<f64> {
        match de(s).value {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }

    #[test]
    fn german_numbers() {
        assert_eq!(num("1,5"), Some(1.5));
        assert_eq!(num("-1,5"), Some(-1.5));
        assert_eq!(num(",5"), Some(0.5));
        assert_eq!(num("1.234"), Some(1234.0));
        assert_eq!(num("1.234,56"), Some(1234.56));
        assert_eq!(de("1.234,56").format, Some("#,##0.00"));
        assert_eq!(num("12,5 %"), Some(0.125));
        assert_eq!(num("12,5%"), Some(0.125));
        assert_eq!(num("12,50 €"), Some(12.5));
        assert_eq!(de("12,50 €").format, Some("#,##0.00 \"€\""));
        assert_eq!(num("€ 1.200"), Some(1200.0));
        assert_eq!(num("(100)"), Some(-100.0));
        assert_eq!(num("1,5E+3"), Some(1500.0));
        assert_eq!(num("42"), Some(42.0));
        assert_eq!(de("1.2345").value, Value::text("1.2345"), "not a grouping and not a date");
        assert_eq!(de("1,2,3").value, Value::text("1,2,3"));
    }

    #[test]
    fn german_dates_and_times() {
        let date = |y, m, d| crate::date::serial_from_ymd(DateSystem::D1900, y, m, d);
        assert_eq!(num("10.10.2026"), date(2026, 10, 10));
        assert_eq!(de("10.10.2026").format, Some("m/d/yyyy"));
        assert_eq!(num("10.10.26"), date(2026, 10, 10));
        assert_eq!(num("2026-10-10"), date(2026, 10, 10));
        assert_eq!(num("10/10/2026"), date(2026, 10, 10));
        assert_eq!(num("10. Okt 2026"), date(2026, 10, 10));
        assert_eq!(num("10. Oktober 2026"), date(2026, 10, 10));
        assert_eq!(num("1.5"), date(current_year(), 5, 1), "German Excel reads 1.5 as the 1st of May");
        assert_eq!(num("10.10."), date(current_year(), 10, 10));
        assert!(num("31.02.2026").is_none(), "no 31st of February");
        let t = num("10:30").unwrap_or_default();
        assert!((t - 10.5 / 24.0).abs() < 1e-9);
        let dt = num("10.10.2026 10:30").unwrap_or_default();
        assert!((dt - (date(2026, 10, 10).unwrap_or_default() + 10.5 / 24.0)).abs() < 1e-9);
    }

    #[test]
    fn german_words_and_errors() {
        assert_eq!(de("WAHR").value, Value::Bool(true));
        assert_eq!(de("falsch").value, Value::Bool(false));
        assert_eq!(de("TRUE").value, Value::text("TRUE"), "English words are text in German, as in Excel");
        assert_eq!(de("#NV").value, Value::Error(CellError::NA));
        assert_eq!(de("#WERT!").value, Value::Error(CellError::Value));
        assert_eq!(de("'1,5").value, Value::text("1,5"));
        assert_eq!(de("Hallo").value, Value::text("Hallo"));
        assert_eq!(de("").value, Value::Empty);
    }

    #[test]
    fn names_and_separators() {
        assert_eq!(Locale::De.error_name(CellError::Ref), "#BEZUG!");
        assert_eq!(Locale::De.error_name(CellError::Div0), "#DIV/0!");
        assert_eq!(Locale::De.parse_error("#nv"), Some(CellError::NA));
        assert_eq!(Locale::De.month_name(3, true), "Mrz");
        assert_eq!(Locale::De.month_name(10, false), "Oktober");
        assert_eq!(Locale::De.day_name(6, false), "Samstag");
        assert_eq!(Locale::De.number_literal("-1234.5"), "-1234,5");
        assert_eq!(Locale::EnUs.parse_input("1,5", DateSystem::D1900).value, Value::text("1,5"));
        assert_eq!(Locale::from_tag("de-AT"), Locale::De);
        assert_eq!(Locale::from_tag("en"), Locale::EnUs);
    }

    #[test]
    fn hostile_input_never_panics() {
        for s in
            ["€", "%", "-", "(", "()", ".", ",", "..", ",,", "1..2", "€€1", "99999999999.999.999,9", "1.e", "10.", ".10", "10. ", "Okt", "31.13."]
        {
            let _ = de(s);
        }
    }
}
