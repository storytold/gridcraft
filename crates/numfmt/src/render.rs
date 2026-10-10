//! Rendering a value with a parsed section.

use gridcraft_core::date::datetime_from_serial;
use gridcraft_core::{DateSystem, Locale};

use crate::decimal::{decimal_digits, fixed, fixed_from_digits, format_general_fit};
use crate::parse::{DatePart, Section, Tok};

/// Text being built plus the first repeat-fill position.
#[derive(Default)]
pub(crate) struct Out {
    pub text: String,
    pub fill: Option<(char, usize)>,
}

impl Out {
    fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }
    fn fill(&mut self, c: char) {
        if self.fill.is_none() {
            self.fill = Some((c, self.text.len()));
        }
    }
    pub(crate) fn prepend(&mut self, s: &str) {
        self.text.insert_str(0, s);
        if let Some((_, p)) = &mut self.fill {
            *p += s.len();
        }
    }
}

/// Renders a text value through a text section (`@` is replaced by the text).
pub(crate) fn render_text(sec: &Section, text: &str) -> Out {
    let mut out = Out::default();
    for t in &sec.toks {
        match t {
            Tok::At => out.push(text),
            Tok::Fill(c) => out.fill(*c),
            other => out.push(&other.literal_text()),
        }
    }
    out
}

/// Renders a number through a General section (literals around the General keyword).
pub(crate) fn render_general(sec: &Section, v: f64, width: usize, loc: Locale) -> Out {
    let mut out = Out::default();
    for t in &sec.toks {
        match t {
            Tok::General => out.push(&format_general_fit(v, width).map(|s| loc.number_literal(&s)).unwrap_or_else(|| "#".repeat(width.clamp(1, 11)))),
            Tok::Fill(c) => out.fill(*c),
            other => out.push(&other.literal_text()),
        }
    }
    out
}

/// Renders a date/time. `None` when the serial is outside the supported range.
pub(crate) fn render_date(sec: &Section, v: f64, sys: DateSystem, loc: Locale) -> Option<Out> {
    if !v.is_finite() || !(0.0..2_958_466.0).contains(&v) {
        return None;
    }
    let sub_digits = sec.toks.iter().filter_map(|t| if let Tok::Date(DatePart::SubSec(n)) = t { Some(*n) } else { None }).max().unwrap_or(0).min(3);
    let unit: i64 = 10i64.pow(3 - sub_digits as u32);
    let mut day = v.floor() as i64;
    let mut ms = (((v - v.floor()) * 86_400_000.0) / unit as f64).round() as i64 * unit;
    if ms >= 86_400_000 {
        ms -= 86_400_000;
        day += 1;
    }
    let total_ms = day.saturating_mul(86_400_000).saturating_add(ms);
    let needs_date = sec.toks.iter().any(|t| matches!(t, Tok::Date(d) if d.is_calendar()));
    let dt = if needs_date { datetime_from_serial(sys, day as f64)? } else { datetime_from_serial(sys, 0.0).unwrap_or_default() };
    let ampm = sec.toks.iter().any(|t| matches!(t, Tok::Date(DatePart::AmPm | DatePart::AP(..))));
    let hour = ms / 3_600_000;
    let minute = (ms / 60_000) % 60;
    let second = (ms / 1000) % 60;
    let milli = ms % 1000;
    let pad = |n: i64, w: u8| if w >= 2 { format!("{:0w$}", n, w = w.min(20) as usize) } else { n.to_string() };
    let month_name = loc.month_name(dt.month, false);
    let month_short = loc.month_name(dt.month, true);
    let day_name = loc.day_name(dt.weekday, false);
    let day_short = loc.day_name(dt.weekday, true);
    let mut out = Out::default();
    for t in &sec.toks {
        match t {
            Tok::Date(d) => {
                let s = match *d {
                    DatePart::Year(n) => {
                        if n <= 2 {
                            format!("{:02}", dt.year.rem_euclid(100))
                        } else {
                            format!("{:04}", dt.year)
                        }
                    }
                    DatePart::Month(n) => match n {
                        1 | 2 => pad(dt.month as i64, n),
                        3 => month_short.to_string(),
                        5 => month_name.chars().take(1).collect(),
                        _ => month_name.to_string(),
                    },
                    DatePart::Day(n) => match n {
                        1 | 2 => pad(dt.day as i64, n),
                        3 => day_short.to_string(),
                        _ => day_name.to_string(),
                    },
                    DatePart::Hour(n) => {
                        let h = if ampm {
                            let h = hour % 12;
                            if h == 0 { 12 } else { h }
                        } else {
                            hour
                        };
                        pad(h, n)
                    }
                    DatePart::Minute(n) => pad(minute, n),
                    DatePart::Second(n) => pad(second, n),
                    DatePart::SubSec(n) => {
                        let mut s = format!("{}{:03}", loc.decimal(), milli);
                        s.truncate(1 + (n as usize).min(3));
                        if n > 3 {
                            s.push_str(&"0".repeat(n as usize - 3));
                        }
                        s
                    }
                    DatePart::ElapsedH(n) => pad(total_ms / 3_600_000, n),
                    DatePart::ElapsedM(n) => pad(total_ms / 60_000, n),
                    DatePart::ElapsedS(n) => pad(total_ms / 1000, n),
                    DatePart::AmPm => (if hour >= 12 { "PM" } else { "AM" }).to_string(),
                    DatePart::AP(a, p) => (if hour >= 12 { p } else { a }).to_string(),
                };
                out.push(&s);
            }
            Tok::Fill(c) => out.fill(*c),
            other => out.push(&other.literal_text()),
        }
    }
    Some(out)
}

/// Assigns integer digits right-to-left to the placeholders at `idxs`; the leftmost placeholder
/// takes all remaining digits.
fn fill_int(toks: &[Tok], idxs: &[usize], digits: &str, thousands: Option<char>, force_zero: bool, pieces: &mut [String]) {
    let ds: Vec<char> = digits.chars().collect();
    let mut rem = ds.len();
    let mut pos = 0usize;
    let count = idxs.len();
    for (k, &ti) in idxs.iter().rev().enumerate() {
        let leftmost = k + 1 == count;
        let ph = match toks.get(ti) {
            Some(Tok::Digit(c)) => *c,
            _ => '0',
        };
        let mut rev = String::new();
        let push_digit = |c: char, rev: &mut String, pos: &mut usize| {
            if let Some(sep) = thousands
                && *pos > 0
                && pos.is_multiple_of(3)
            {
                rev.push(sep);
            }
            rev.push(c);
            *pos += 1;
        };
        let take = if leftmost { rem } else { rem.min(1) };
        if take == 0 {
            match ph {
                '0' => push_digit('0', &mut rev, &mut pos),
                _ if force_zero && leftmost && pos == 0 => push_digit('0', &mut rev, &mut pos),
                '?' => rev.push(' '),
                _ => {}
            }
        } else {
            for _ in 0..take {
                rem -= 1;
                push_digit(ds.get(rem).copied().unwrap_or('0'), &mut rev, &mut pos);
            }
        }
        if let Some(p) = pieces.get_mut(ti) {
            *p = rev.chars().rev().collect();
        }
    }
}

fn with_commas(digits: &str, thousands: Option<char>) -> String {
    let Some(sep) = thousands else {
        return digits.to_string();
    };
    let n = digits.chars().count();
    let mut s = String::with_capacity(n + n / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            s.push(sep);
        }
        s.push(c);
    }
    s
}

/// Fraction-digit placeholders: trailing zeros show as `0`, nothing (`#`) or a space (`?`).
fn fill_frac(toks: &[Tok], idxs: &[usize], frac: &str, pieces: &mut [String]) {
    let fd: Vec<char> = frac.chars().collect();
    let last_sig = fd.iter().rposition(|&c| c != '0');
    for (i, &ti) in idxs.iter().enumerate() {
        let ph = match toks.get(ti) {
            Some(Tok::Digit(c)) => *c,
            _ => '0',
        };
        let s = if last_sig.is_some_and(|l| i <= l) {
            fd.get(i).copied().unwrap_or('0').to_string()
        } else {
            match ph {
                '0' => "0".into(),
                '?' => " ".into(),
                _ => String::new(),
            }
        };
        if let Some(p) = pieces.get_mut(ti) {
            *p = s;
        }
    }
}

fn digit_indices(toks: &[Tok], range: std::ops::Range<usize>) -> Vec<usize> {
    range.filter(|&i| matches!(toks.get(i), Some(Tok::Digit(_)))).collect()
}

/// Renders a non-negative number through a number section, with `loc`'s separators.
pub(crate) fn render_number(sec: &Section, v: f64, loc: Locale) -> Out {
    let thousands = sec.thousands.then_some(loc.group());
    let mut x = v.abs();
    if sec.percent > 0 {
        x *= 100f64.powi(sec.percent.min(20));
    }
    if sec.scale > 0 {
        x /= 1000f64.powi(sec.scale.min(110));
    }
    if !x.is_finite() {
        x = f64::MAX;
    }
    let toks = &sec.toks;
    let n = toks.len();
    // Pieces replace the token's own output; `before` is inserted ahead of a token.
    let mut pieces: Vec<String> = vec![String::new(); n];
    let mut before: Vec<String> = vec![String::new(); n + 1];
    if sec.fraction {
        render_fraction(sec, x, thousands, &mut pieces);
    } else {
        let point = toks.iter().position(|t| matches!(t, Tok::Point));
        let exp = toks.iter().position(|t| matches!(t, Tok::Exp { .. }));
        let int_end = point.or(exp).unwrap_or(n);
        let frac_end = exp.unwrap_or(n);
        let int_idx = digit_indices(toks, 0..int_end);
        let frac_idx = match point {
            Some(p) => digit_indices(toks, p + 1..frac_end),
            None => Vec::new(),
        };
        let (int_s, frac_s) = if let Some(e_at) = exp {
            let exp_idx = digit_indices(toks, e_at + 1..n);
            let step = int_idx.len().max(1) as i32;
            let (mut e, int_s, frac_s) = if x == 0.0 {
                (0i32, String::new(), "0".repeat(frac_idx.len()))
            } else {
                let (digits, xe) = decimal_digits(x);
                let mut e = xe - xe.rem_euclid(step);
                let (mut i, mut f) = fixed_from_digits(&digits, xe - e, frac_idx.len());
                if i.len() > step as usize {
                    e += step;
                    (i, f) = fixed_from_digits(&digits, xe - e, frac_idx.len());
                }
                (e, i, f)
            };
            // Exponent text.
            let (upper, plus) = match toks.get(e_at) {
                Some(Tok::Exp { upper, plus }) => (*upper, *plus),
                _ => (true, true),
            };
            if int_s.is_empty() && frac_s.chars().all(|c| c == '0') {
                e = 0;
            }
            let mut head = String::from(if upper { "E" } else { "e" });
            if e < 0 {
                head.push('-');
            } else if plus {
                head.push('+');
            }
            let width = exp_idx.iter().filter(|&&i| matches!(toks.get(i), Some(Tok::Digit('0')))).count().min(20);
            let digits = format!("{:0w$}", e.unsigned_abs(), w = width);
            match exp_idx.first() {
                Some(&first) => {
                    if let Some(p) = pieces.get_mut(e_at) {
                        *p = head;
                    }
                    if let Some(p) = pieces.get_mut(first) {
                        *p = digits;
                    }
                }
                None => {
                    if let Some(p) = pieces.get_mut(e_at) {
                        *p = head + &digits;
                    }
                }
            }
            (int_s, frac_s)
        } else {
            fixed(x, frac_idx.len())
        };
        fill_int(toks, &int_idx, &int_s, thousands, false, &mut pieces);
        if int_idx.is_empty()
            && !int_s.is_empty()
            && let Some(p) = point.or(exp)
            && let Some(b) = before.get_mut(p)
        {
            *b = with_commas(&int_s, thousands);
        }
        fill_frac(toks, &frac_idx, &frac_s, &mut pieces);
        if let Some(p) = point
            && let Some(s) = pieces.get_mut(p)
        {
            *s = loc.decimal().to_string();
        }
    }
    let mut out = Out::default();
    for (i, t) in toks.iter().enumerate() {
        if let Some(b) = before.get(i) {
            out.push(b);
        }
        match t {
            Tok::Lit(s) => out.push(s),
            Tok::Percent => out.push("%"),
            Tok::Fill(c) => out.fill(*c),
            Tok::At | Tok::General | Tok::Date(_) | Tok::Comma => {}
            _ => out.push(pieces.get(i).map(String::as_str).unwrap_or("")),
        }
    }
    out
}

/// Best rational approximation of `x` with denominator at most `max_den` (continued fractions).
fn approximate(x: f64, max_den: u64) -> (u64, u64) {
    if x.is_nan() || x <= 0.0 || !x.is_finite() {
        return (0, 1);
    }
    if x >= 1e15 {
        return (x.round() as u64, 1);
    }
    let (mut p0, mut q0, mut p1, mut q1) = (0u64, 1u64, 1u64, 0u64);
    let mut r = x;
    for _ in 0..64 {
        let a_f = r.floor();
        if a_f > 1e15 {
            break;
        }
        let a = a_f as u64;
        let Some(q2) = a.checked_mul(q1).and_then(|v| v.checked_add(q0)) else { break };
        if q2 > max_den {
            break;
        }
        let Some(p2) = a.checked_mul(p1).and_then(|v| v.checked_add(p0)) else { break };
        (p0, q0, p1, q1) = (p1, q1, p2, q2);
        let frac = r - a_f;
        if frac < 1e-12 {
            break;
        }
        r = 1.0 / frac;
    }
    if q1 == 0 {
        return (x.round() as u64, 1);
    }
    // Semiconvergent bound.
    let k = (max_den - q0) / q1;
    let (pb, qb) = (p0.saturating_add(k.saturating_mul(p1)), q0.saturating_add(k.saturating_mul(q1)));
    let err_a = (p1 as f64 / q1 as f64 - x).abs();
    let err_b = if qb > 0 { (pb as f64 / qb as f64 - x).abs() } else { f64::INFINITY };
    if err_b < err_a { (pb, qb) } else { (p1, q1) }
}

fn render_fraction(sec: &Section, x: f64, thousands: Option<char>, pieces: &mut [String]) {
    let toks = &sec.toks;
    let Some(slash) = toks.iter().position(|t| matches!(t, Tok::Slash)) else { return };
    let mut num_start = slash;
    while num_start > 0 && matches!(toks.get(num_start - 1), Some(Tok::Digit(_))) {
        num_start -= 1;
    }
    let whole_idx = digit_indices(toks, 0..num_start);
    let num_idx = digit_indices(toks, num_start..slash);
    let mut den_end = slash + 1;
    let fixed_den: Option<u64> = match toks.get(slash + 1) {
        Some(Tok::FixedDenom(s)) => {
            den_end += 1;
            Some(s.parse::<u64>().unwrap_or(u64::MAX).clamp(1, 1_000_000_000))
        }
        _ => None,
    };
    if fixed_den.is_none() {
        while matches!(toks.get(den_end), Some(Tok::Digit(_))) {
            den_end += 1;
        }
    }
    let den_idx = digit_indices(toks, slash + 1..den_end);
    let has_whole = !whole_idx.is_empty();
    let (mut whole, f) = if has_whole { (x.floor(), x - x.floor()) } else { (0.0, x) };
    let (mut num, den) = match fixed_den {
        Some(d) => ((f * d as f64).round() as u64, d),
        None => {
            let digits = den_idx.len().clamp(1, 7) as u32;
            approximate(f, 10u64.pow(digits) - 1)
        }
    };
    if has_whole && num >= den && den > 0 {
        whole += (num / den) as f64;
        num %= den;
    }
    let whole_s = if whole >= 1.0 { fixed(whole, 0).0 } else { String::new() };
    let blank = has_whole && num == 0;
    fill_int(toks, &whole_idx, &whole_s, thousands, blank, pieces);
    let num_s = num.to_string();
    let den_s = den.to_string();
    if blank {
        let width = |idx: &[usize]| idx.len();
        let nw = width(&num_idx).max(1);
        if let Some(&first) = num_idx.first()
            && let Some(p) = pieces.get_mut(first)
        {
            *p = " ".repeat(nw);
        }
        if let Some(p) = pieces.get_mut(slash) {
            *p = " ".into();
        }
        let dw = if fixed_den.is_some() { den_s.len() } else { den_idx.len() };
        if let Some(p) = pieces.get_mut(slash + 1) {
            *p = " ".repeat(dw);
        }
        // Literals between the whole part and the numerator stay as they are.
        return;
    }
    fill_int(toks, &num_idx, &num_s, None, true, pieces);
    if let Some(p) = pieces.get_mut(slash) {
        *p = "/".into();
    }
    if fixed_den.is_some() {
        if let Some(p) = pieces.get_mut(slash + 1) {
            *p = den_s;
        }
    } else {
        // Denominator digits are left aligned; `?` pads with spaces after them.
        let dc: Vec<char> = den_s.chars().collect();
        let count = den_idx.len();
        for (k, &ti) in den_idx.iter().enumerate() {
            let s = if k + 1 == count {
                dc.get(k..).map(|r| r.iter().collect::<String>()).unwrap_or_default()
            } else {
                dc.get(k).map(|c| c.to_string()).unwrap_or_default()
            };
            let s = if s.is_empty() {
                match toks.get(ti) {
                    Some(Tok::Digit('?')) => " ".into(),
                    Some(Tok::Digit('0')) => "0".into(),
                    _ => String::new(),
                }
            } else {
                s
            };
            if let Some(p) = pieces.get_mut(ti) {
                *p = s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximations() {
        assert_eq!(approximate(0.5, 9), (1, 2));
        assert_eq!(approximate(0.333333, 9), (1, 3));
        assert_eq!(approximate(std::f64::consts::PI - 3.0, 99), (14, 99));
        assert_eq!(approximate(std::f64::consts::PI - 3.0, 9), (1, 7));
        assert_eq!(approximate(0.0, 9), (0, 1));
        assert_eq!(approximate(0.999, 9), (1, 1));
    }
}
