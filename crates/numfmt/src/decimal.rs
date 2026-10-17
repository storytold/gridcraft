//! Decimal-string rounding and the General format.
//!
//! Excel works with 15 significant digits, so numbers are first written with 15 significant
//! digits and then rounded half away from zero on that decimal string. That makes 2.675 with
//! "0.00" show 2.68 and 1.005 show 1.01, as in Excel, even though neither is exact in binary.

use gridcraft_core::Locale;

/// Most decimals we ever compute; placeholders beyond this are padded with zeros.
pub(crate) const MAX_DECIMALS: usize = 400;

/// The 15 significant digits of a positive finite number and the power of ten of the first
/// digit: `a ≈ d0.d1d2… × 10^exp`.
///
/// The value is rounded to the nearest 15-digit decimal. An exact tie at the 16th digit (the exact
/// decimal expansion is 15 digits, a 5, then only zeros, as for 1234567890123455 or
/// 123456789012345.5) goes toward zero, as Excel does: `1234567890123455` shows `1234567890123450`,
/// not `…460`. Excel observed (Windows 16.0.20430, TEXT()).
pub(crate) fn decimal_digits(a: f64) -> ([u8; 15], i32) {
    let mut digits = [0u8; 15];
    if !a.is_finite() || a <= 0.0 {
        return (digits, 0);
    }
    // 16 digits, rounded. Unless the 16th is a 5 it decides the 15th exactly (4 or less rounds
    // down, 6 or more up), so one format serves almost every number.
    let (d16, exp) = significant_digits(a, 16);
    for (slot, d) in digits.iter_mut().zip(&d16) {
        *slot = *d;
    }
    match d16.get(15).copied().unwrap_or(0) {
        0..=4 => (digits, exp),
        5 if is_exact_tie(a) => (digits, exp),
        5 => {
            // Not a tie: round the exact value (the 16-digit rounding above may be a double rounding).
            let (d15, exp15) = significant_digits(a, 15);
            let mut out = [0u8; 15];
            for (slot, d) in out.iter_mut().zip(&d15) {
                *slot = *d;
            }
            (out, exp15)
        }
        _ => {
            let mut exp = exp;
            let mut carry = true;
            for d in digits.iter_mut().rev() {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                digits = [0; 15];
                digits[0] = 1;
                exp += 1;
            }
            (digits, exp)
        }
    }
}

/// `n` significant digits of `a` (rounded half to even on the exact binary value) and the power of
/// ten of the first.
fn significant_digits(a: f64, n: usize) -> (Vec<u8>, i32) {
    let s = format!("{:.*e}", n.saturating_sub(1), a);
    let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    (mant.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect(), exp.parse().unwrap_or(0))
}

/// True when `a`'s exact decimal expansion has a 5 as its 16th significant digit and only zeros after.
fn is_exact_tie(a: f64) -> bool {
    let is_tie = |ds: &[u8]| ds.get(15) == Some(&5) && ds.get(16..).is_some_and(|t| t.iter().all(|&d| d == 0));
    // A cheap screen first (30 digits), then the exact expansion: a double has at most 767
    // significant digits and Rust prints every one of them.
    is_tie(&significant_digits(a, 30).0) && is_tie(&significant_digits(a, 800).0)
}

/// Rounds the number `digits × 10^exp` (see [`decimal_digits`]) to `decimals` places, half away
/// from zero. Returns the integer digits ("" when the integer part is zero) and exactly
/// `decimals` fraction digits.
pub(crate) fn fixed_from_digits(digits: &[u8; 15], exp: i32, decimals: usize) -> (String, String) {
    let decimals_calc = decimals.min(MAX_DECIMALS);
    let keep = exp as i64 + decimals_calc as i64 + 1;
    let mut m: Vec<u8> = if keep <= 0 {
        if keep == 0 && digits.first().copied().unwrap_or(0) >= 5 { vec![1] } else { Vec::new() }
    } else if keep as usize >= digits.len() {
        let mut v = digits.to_vec();
        v.resize(keep as usize, 0);
        v
    } else {
        let keep = keep as usize;
        let mut v = digits.get(..keep).map(<[u8]>::to_vec).unwrap_or_default();
        if digits.get(keep).copied().unwrap_or(0) >= 5 {
            let mut carry = true;
            for d in v.iter_mut().rev() {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                v.insert(0, 1);
            }
        }
        v
    };
    let lead = m.iter().take_while(|&&d| d == 0).count();
    m.drain(..lead);
    let to_s = |ds: &[u8]| ds.iter().map(|d| char::from(b'0' + d)).collect::<String>();
    let (int, mut frac) = if m.len() <= decimals_calc {
        let mut f = "0".repeat(decimals_calc - m.len());
        f.push_str(&to_s(&m));
        (String::new(), f)
    } else {
        let split = m.len() - decimals_calc;
        (to_s(m.get(..split).unwrap_or(&[])), to_s(m.get(split..).unwrap_or(&[])))
    };
    if decimals > decimals_calc {
        frac.push_str(&"0".repeat(decimals - decimals_calc));
    }
    (int, frac)
}

/// Rounds a non-negative number to `decimals` places (see [`fixed_from_digits`]).
pub(crate) fn fixed(a: f64, decimals: usize) -> (String, String) {
    if !a.is_finite() || a <= 0.0 {
        return (String::new(), "0".repeat(decimals.min(MAX_DECIMALS * 4)));
    }
    let (d, e) = decimal_digits(a);
    fixed_from_digits(&d, e, decimals)
}

/// General format squeezed into `max_chars` characters, written in `loc`'s conventions. Excel
/// shows fewer decimals, then scientific notation, then "####" when a column is narrow; `None`
/// means the caller paints "####".
pub fn format_general_fit(n: f64, max_chars: usize, loc: Locale) -> Option<String> {
    if !n.is_finite() {
        return None;
    }
    if n == 0.0 {
        return (max_chars >= 1).then(|| "0".to_string());
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let (digits, x) = decimal_digits(n.abs());
    let int_len = sign.len() + (x.max(0) as usize) + 1;
    // Fixed notation for magnitudes from 1E-4 up to what fits.
    if x >= -4 && int_len <= max_chars {
        let avail = max_chars - int_len;
        let mut decimals = if avail >= 2 { avail - 1 } else { 0 };
        loop {
            let (i, f) = fixed_from_digits(&digits, x, decimals);
            let f = f.trim_end_matches('0');
            if i.is_empty() && f.is_empty() {
                break; // rounded away to zero: use scientific
            }
            let mut s = String::from(sign);
            s.push_str(if i.is_empty() { "0" } else { &i });
            if !f.is_empty() {
                s.push(loc.decimal());
                s.push_str(f);
            }
            if s.chars().count() <= max_chars {
                return Some(s);
            }
            if decimals == 0 {
                break;
            }
            decimals -= 1;
        }
    }
    for md in (0..=max_chars.min(14)).rev() {
        let mut e = x;
        let (mut mi, mut mf) = fixed_from_digits(&digits, 0, md);
        if mi.len() > 1 {
            e += 1;
            (mi, mf) = fixed_from_digits(&digits, -1, md);
        }
        let mf = mf.trim_end_matches('0');
        let mut s = String::from(sign);
        s.push_str(if mi.is_empty() { "0" } else { &mi });
        if !mf.is_empty() {
            s.push(loc.decimal());
            s.push_str(mf);
        }
        s.push_str(&format!("E{}{:02}", if e < 0 { '-' } else { '+' }, e.unsigned_abs()));
        if s.chars().count() <= max_chars {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rounding tests below are en-US; the shim keeps their call sites as they were while
    /// `format_general_fit` takes a locale. The German test calls the function directly.
    fn format_general_fit(n: f64, max_chars: usize) -> Option<String> {
        super::format_general_fit(n, max_chars, Locale::En)
    }

    #[test]
    fn rounding() {
        assert_eq!(fixed(2.675, 2), ("2".into(), "68".into()));
        assert_eq!(fixed(1.005, 2), ("1".into(), "01".into()));
        assert_eq!(fixed(0.5, 0), ("1".into(), "".into()));
        assert_eq!(fixed(0.04, 1), ("".into(), "0".into()));
        assert_eq!(fixed(0.05, 1), ("".into(), "1".into()));
        assert_eq!(fixed(9.999, 2), ("10".into(), "00".into()));
        assert_eq!(fixed(0.0004, 3), ("".into(), "000".into()));
        assert_eq!(fixed(0.0005, 3), ("".into(), "001".into()));
        assert_eq!(fixed(1e20, 0).0, "100000000000000000000");
        assert_eq!(fixed(0.0, 2), ("".into(), "00".into()));
    }

    #[test]
    fn general_fit() {
        assert_eq!(format_general_fit(1e11, 11).as_deref(), Some("1E+11"));
        assert_eq!(format_general_fit(12345678901.0, 11).as_deref(), Some("12345678901"));
        assert_eq!(format_general_fit(123456789012.0, 11).as_deref(), Some("1.23457E+11"));
        assert_eq!(format_general_fit(0.1 + 0.2, 11).as_deref(), Some("0.3"));
        assert_eq!(format_general_fit(1.0 / 3.0, 11).as_deref(), Some("0.333333333"));
        assert_eq!(format_general_fit(123456.789012, 11).as_deref(), Some("123456.789"));
        assert_eq!(format_general_fit(1e-10, 11).as_deref(), Some("1E-10"));
        assert_eq!(format_general_fit(0.0000123456789, 11).as_deref(), Some("1.23457E-05"));
        assert_eq!(format_general_fit(0.0001, 11).as_deref(), Some("0.0001"));
        assert_eq!(format_general_fit(-5.0, 11).as_deref(), Some("-5"));
        assert_eq!(format_general_fit(0.0, 11).as_deref(), Some("0"));
        assert_eq!(format_general_fit(1234.5678, 6).as_deref(), Some("1234.6"));
        assert_eq!(format_general_fit(1234.5678, 4).as_deref(), Some("1235"));
        assert_eq!(format_general_fit(123456.0, 5).as_deref(), Some("1E+05"));
        assert_eq!(format_general_fit(123456.0, 3), None);
        assert_eq!(format_general_fit(9.99999999999, 5).as_deref(), Some("10"));
        assert_eq!(format_general_fit(f64::NAN, 11), None);
        assert_eq!(format_general_fit(1.7976931348623157e308, 11).as_deref(), Some("1.7977E+308"));
    }

    #[test]
    fn general_fit_writes_the_locale_decimal_separator() {
        assert_eq!(super::format_general_fit(1234.5, 11, Locale::De).as_deref(), Some("1234,5"));
        assert_eq!(super::format_general_fit(0.3, 11, Locale::De).as_deref(), Some("0,3"));
        assert_eq!(super::format_general_fit(123456.789012, 11, Locale::De).as_deref(), Some("123456,789"));
        // Scientific notation too.
        assert_eq!(super::format_general_fit(123456789012.0, 11, Locale::De).as_deref(), Some("1,23457E+11"));
        assert_eq!(super::format_general_fit(1234.5, 11, Locale::En).as_deref(), Some("1234.5"));
    }
}
