//! `Number.*`.

use super::super::eval::Ev;
use super::super::value::{MError, MResult, MValue};
use super::{Args, cant};

pub fn call(_ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Number.Round" | "Number.RoundUp" | "Number.RoundDown" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => {
                let digits = a.int(1, 0)?.clamp(-15, 15) as i32;
                let mode = match name {
                    "Number.RoundUp" => 0,
                    "Number.RoundDown" => 1,
                    _ => a.int(2, 4)?,
                };
                MValue::Number(round(*n, digits, mode))
            }
            other => return Err(cant(other, "Number")),
        },
        "Number.Abs" => num1(a.at(0), f64::abs)?,
        "Number.Sqrt" => num1(a.at(0), f64::sqrt)?,
        "Number.Power" => match (a.at(0), a.at(1)) {
            (MValue::Null, _) | (_, MValue::Null) => MValue::Null,
            _ => MValue::Number(a.num(0)?.powf(a.num(1)?)),
        },
        "Number.Mod" | "Number.IntegerDivide" => match (a.at(0), a.at(1)) {
            (MValue::Null, _) | (_, MValue::Null) => MValue::Null,
            _ => {
                let (x, y) = (a.num(0)?, a.num(1)?);
                if y == 0.0 {
                    return Err(MError::expr("Division by zero."));
                }
                MValue::Number(if name == "Number.Mod" { x % y } else { (x / y).trunc() })
            }
        },
        "Number.ToText" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => {
                let culture = a.opt_text(2)?;
                MValue::text(match a.opt_text(1)? {
                    None => super::convert::text_of(&MValue::Number(*n), culture.as_deref()).unwrap_or_default(),
                    Some(f) => format_number(*n, &f, culture.as_deref())?,
                })
            }
            other => return Err(cant(other, "Number")),
        },
        "Number.IsEven" | "Number.IsOdd" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => MValue::Logical(n.fract() == 0.0 && ((n.rem_euclid(2.0) == 0.0) == (name == "Number.IsEven"))),
            other => return Err(cant(other, "Number")),
        },
        "Number.IsNaN" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => MValue::Logical(n.is_nan()),
            other => return Err(cant(other, "Number")),
        },
        "Number.Sign" => num1(a.at(0), |n| {
            if n > 0.0 {
                1.0
            } else if n < 0.0 {
                -1.0
            } else {
                n
            }
        })?,
        "Number.Exp" => num1(a.at(0), f64::exp)?,
        "Number.Ln" => num1(a.at(0), f64::ln)?,
        "Number.Log10" => num1(a.at(0), f64::log10)?,
        "Number.Log" => match (a.at(0), a.at(1)) {
            (MValue::Null, _) => MValue::Null,
            (_, MValue::Null) => num1(a.at(0), f64::ln)?,
            _ => MValue::Number(a.num(0)?.log(a.num(1)?)),
        },
        "Number.Factorial" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= 170.0 => MValue::Number((1..=*n as u32).map(f64::from).product()),
            MValue::Number(_) => return Err(MError::expr("Number.Factorial needs a whole number from 0 to 170.")),
            other => return Err(cant(other, "Number")),
        },
        "Number.RoundTowardZero" | "Number.RoundAwayFromZero" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => MValue::Number(round(*n, a.int(1, 0)?.clamp(-15, 15) as i32, if name == "Number.RoundTowardZero" { 3 } else { 2 })),
            other => return Err(cant(other, "Number")),
        },
        "Number.Sin" => num1(a.at(0), f64::sin)?,
        "Number.Cos" => num1(a.at(0), f64::cos)?,
        "Number.Tan" => num1(a.at(0), f64::tan)?,
        "Number.Asin" => num1(a.at(0), f64::asin)?,
        "Number.Acos" => num1(a.at(0), f64::acos)?,
        "Number.Atan" => num1(a.at(0), f64::atan)?,
        "Number.Atan2" => match (a.at(0), a.at(1)) {
            (MValue::Null, _) | (_, MValue::Null) => MValue::Null,
            _ => MValue::Number(a.num(0)?.atan2(a.num(1)?)),
        },
        _ => return Ok(None),
    };
    Ok(Some(r))
}

/// `Number.Round`'s rounding modes: Up 0, Down 1, AwayFromZero 2, TowardZero 3, ToEven 4.
pub fn round(n: f64, digits: i32, mode: i64) -> f64 {
    if !n.is_finite() {
        return n;
    }
    let scale = 10f64.powi(digits);
    let x = n * scale;
    if !x.is_finite() {
        return n;
    }
    let r = match mode {
        0 => x.ceil(),
        1 => x.floor(),
        2 => x.round(),
        3 => x.trunc(),
        _ => {
            let r = x.round();
            if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - x.signum() } else { r }
        }
    };
    r / scale
}

pub fn num1(v: &MValue, f: impl Fn(f64) -> f64) -> MResult<MValue> {
    match v {
        MValue::Null => Ok(MValue::Null),
        MValue::Number(n) => Ok(MValue::Number(f(*n))),
        other => Err(cant(other, "Number")),
    }
}

/// `Number.ToText`'s format: standard (`N2`, `F0`, `P1`, `D5`, `E3`, `C`, `X`, `G`) or custom
/// (`#,##0.00`, `0.0%`, `000`, sections `pos;neg;zero`, `'literals'`), in the culture's
/// separators.
pub fn format_number(n: f64, fmt: &str, culture: Option<&str>) -> MResult<String> {
    let cul = crate::query::types::Culture::of(culture);
    if !n.is_finite() {
        return Ok(super::convert::number_text(n));
    }
    let mut cs = fmt.chars();
    let (kind, rest) = (cs.next().unwrap_or('G'), cs.as_str());
    let standard = kind.is_ascii_alphabetic() && rest.chars().all(|c| c.is_ascii_digit()) && rest.len() <= 2;
    if standard {
        let prec: Option<usize> = rest.parse().ok();
        let fixed = |d: usize, group: bool| grouped(n, d, d, group, &cul);
        return Ok(match kind.to_ascii_uppercase() {
            'N' => fixed(prec.unwrap_or(2), true),
            'F' => fixed(prec.unwrap_or(2), false),
            'P' => format!("{} %", grouped(n * 100.0, prec.unwrap_or(2), prec.unwrap_or(2), true, &cul)),
            'C' => {
                let v = grouped(n.abs(), prec.unwrap_or(2), prec.unwrap_or(2), true, &cul);
                let sign = if n < 0.0 { "-" } else { "" };
                if cul.decimal == ',' { format!("{sign}{v} €") } else { format!("{sign}${v}") }
            }
            'D' => {
                if n.fract() != 0.0 {
                    return Err(MError::format("Format 'D' needs a whole number."));
                }
                let w = prec.unwrap_or(1);
                let sign = if n < 0.0 { "-" } else { "" };
                format!("{sign}{:0w$}", n.abs() as u64)
            }
            'X' => {
                if n.fract() != 0.0 || n < 0.0 || n > u64::MAX as f64 {
                    return Err(MError::format("Format 'X' needs a whole number from 0."));
                }
                let w = prec.unwrap_or(1);
                let s = format!("{:0w$X}", n as u64);
                if kind == 'x' { s.to_lowercase() } else { s }
            }
            'E' => {
                let d = prec.unwrap_or(6);
                let s = format!("{n:.d$E}");
                let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
                let e: i32 = e.parse().unwrap_or(0);
                let m = if cul.decimal == ',' { m.replace('.', ",") } else { m.to_string() };
                format!("{m}{}{}{:03}", if kind == 'e' { 'e' } else { 'E' }, if e < 0 { '-' } else { '+' }, e.abs())
            }
            'G' | 'R' => {
                let s = super::convert::number_text(n);
                if cul.decimal == ',' { s.replace('.', ",") } else { s }
            }
            _ => return Err(MError::format(format!("'{fmt}' isn't a number format."))),
        });
    }
    // Custom: pick the section (positive; negative; zero), then its placeholders.
    let sections = split_sections(fmt);
    let pick = |i: usize| sections.get(i).cloned().unwrap_or_default();
    let (section, negate) = if sections.len() == 1 {
        (pick(0), n < 0.0)
    } else if n < 0.0 {
        (pick(1), false)
    } else if n == 0.0 && sections.len() >= 3 {
        (pick(2), false)
    } else {
        (pick(0), false)
    };
    let value = n.abs() * if section.contains('%') { 100.0 } else { 1.0 };
    let tokens = tokenize(&section);
    let placeholders: Vec<usize> = tokens.iter().enumerate().filter(|(_, t)| matches!(t, Tok::Digit(_))).map(|(i, _)| i).collect();
    let (Some(&first), Some(&last)) = (placeholders.first(), placeholders.last()) else {
        // No digits: the literal text alone.
        return Ok(tokens.iter().map(Tok::text).collect());
    };
    let point = tokens.iter().position(|t| matches!(t, Tok::Point)).filter(|p| *p > first && *p < last + 1);
    let int_end = point.unwrap_or(last + 1);
    let int_tokens = tokens.get(first..int_end).unwrap_or(&[]);
    let frac_tokens = point.map(|p| tokens.get(p + 1..=last).unwrap_or(&[])).unwrap_or(&[]);
    let min_int = int_tokens.iter().filter(|t| matches!(t, Tok::Digit(true))).count();
    let group = int_tokens.iter().any(|t| matches!(t, Tok::Comma));
    let min_frac = frac_tokens.iter().filter(|t| matches!(t, Tok::Digit(true))).count();
    let max_frac = frac_tokens.iter().filter(|t| matches!(t, Tok::Digit(_))).count();
    let mut body = grouped(value, min_frac, max_frac, group, &cul);
    // Leading zeros up to the integer placeholders' count.
    let int_digits = body.split(cul.decimal).next().map(|s| s.chars().filter(char::is_ascii_digit).count()).unwrap_or(0);
    if min_int > int_digits {
        body = format!("{}{body}", "0".repeat(min_int - int_digits));
    } else if min_int == 0 && body.starts_with('0') && body.len() > 1 && body.chars().nth(1) == Some(cul.decimal) {
        body.remove(0);
    }
    let prefix: String = tokens.get(..first).unwrap_or(&[]).iter().map(Tok::text).collect();
    let suffix: String = tokens.get(last + 1..).unwrap_or(&[]).iter().map(Tok::text).collect();
    let sign = if negate && value != 0.0 { "-" } else { "" };
    Ok(format!("{sign}{prefix}{body}{suffix}"))
}

/// `n` rounded half away from zero to at most `max` decimals (at least `min`), with group
/// separators when `group`.
fn grouped(n: f64, min: usize, max: usize, group: bool, cul: &crate::query::types::Culture) -> String {
    let max = max.min(15);
    let scaled = round(n.abs(), max as i32, 2);
    let s = format!("{scaled:.max$}");
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = {
        let mut f = frac.to_string();
        while f.len() > min && f.ends_with('0') {
            f.pop();
        }
        f
    };
    let mut int_out = String::new();
    let digits: Vec<char> = int.chars().collect();
    for (i, c) in digits.iter().enumerate() {
        if group && i > 0 && (digits.len() - i).is_multiple_of(3) {
            int_out.push(cul.group);
        }
        int_out.push(*c);
    }
    let sign = if n < 0.0 && scaled != 0.0 { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{int_out}") } else { format!("{sign}{int_out}{}{frac}", cul.decimal) }
}

#[derive(Clone, Debug)]
enum Tok {
    /// `0` (true) or `#`.
    Digit(bool),
    Point,
    Comma,
    Lit(String),
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Tok::Lit(s) => s.clone(),
            Tok::Point => ".".into(),
            Tok::Comma => ",".into(),
            Tok::Digit(_) => String::new(),
        }
    }
}

fn split_sections(fmt: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quote: Option<char> = None;
    for c in fmt.chars() {
        let starts_new = quote.is_none() && c == ';';
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '\'' || c == '"' => quote = Some(c),
            _ => {}
        }
        if starts_new {
            out.push(String::new());
        } else if let Some(last) = out.last_mut() {
            last.push(c);
        }
    }
    out
}

fn tokenize(section: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '0' => out.push(Tok::Digit(true)),
            '#' => out.push(Tok::Digit(false)),
            '.' => out.push(Tok::Point),
            ',' => out.push(Tok::Comma),
            '\'' | '"' => {
                let lit: String = chars.by_ref().take_while(|x| *x != c).collect();
                out.push(Tok::Lit(lit));
            }
            '\\' => out.push(Tok::Lit(chars.next().map(String::from).unwrap_or_default())),
            other => out.push(Tok::Lit(other.to_string())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_modes() {
        assert_eq!(round(2.5, 0, 4), 2.0);
        assert_eq!(round(3.5, 0, 4), 4.0);
        assert_eq!(round(-2.5, 0, 4), -2.0);
        assert_eq!(round(2.5, 0, 2), 3.0);
        assert_eq!(round(1.234, 2, 0), 1.24);
        assert_eq!(round(1.239, 2, 1), 1.23);
        assert_eq!(round(1234.0, -2, 4), 1200.0);
    }
}
