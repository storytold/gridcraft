//! Format code parsing: sections, bracket tags and tokens.

use crate::FormatColor;

/// At most this many sections are honoured (`pos;neg;zero;text`); extra ones are ignored.
const MAX_SECTIONS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DatePart {
    Year(u8),
    Month(u8),
    Minute(u8),
    Day(u8),
    Hour(u8),
    Second(u8),
    /// `.0`, `.00`, `.000` after seconds.
    SubSec(u8),
    ElapsedH(u8),
    ElapsedM(u8),
    ElapsedS(u8),
    AmPm,
    /// `A/P` with the letters as typed.
    AP(char, char),
}

impl DatePart {
    pub(crate) fn is_calendar(self) -> bool {
        matches!(self, DatePart::Year(_) | DatePart::Month(_) | DatePart::Day(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Lit(String),
    /// `0`, `#` or `?`.
    Digit(char),
    /// The decimal point (only the first one in a number section).
    Point,
    /// Unresolved comma (resolved during analysis into thousands/scale markers or a literal).
    Comma,
    Percent,
    Exp {
        upper: bool,
        plus: bool,
    },
    /// The fraction bar (only after analysis decides the section is a fraction).
    Slash,
    /// A fixed fraction denominator such as the `8` in `?/8`.
    FixedDenom(String),
    Fill(char),
    At,
    General,
    Date(DatePart),
}

impl Tok {
    /// The text this token shows when it has no special meaning in its section.
    pub(crate) fn literal_text(&self) -> String {
        match self {
            Tok::Lit(s) | Tok::FixedDenom(s) => s.clone(),
            Tok::Digit(c) => c.to_string(),
            Tok::Point => ".".into(),
            Tok::Comma => ",".into(),
            Tok::Percent => "%".into(),
            Tok::Exp { upper, plus } => format!("{}{}", if *upper { 'E' } else { 'e' }, if *plus { '+' } else { '-' }),
            Tok::Slash => "/".into(),
            Tok::Fill(_) | Tok::At | Tok::General | Tok::Date(_) => String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CondOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cond {
    pub op: CondOp,
    pub value: f64,
}

impl Cond {
    pub(crate) fn test(&self, v: f64) -> bool {
        match self.op {
            CondOp::Eq => v == self.value,
            CondOp::Ne => v != self.value,
            CondOp::Lt => v < self.value,
            CondOp::Le => v <= self.value,
            CondOp::Gt => v > self.value,
            CondOp::Ge => v >= self.value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SecKind {
    Number,
    Date,
    Text,
    General,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub toks: Vec<Tok>,
    pub color: Option<FormatColor>,
    pub cond: Option<Cond>,
    pub kind: SecKind,
    pub thousands: bool,
    /// Number of scaling commas (each divides by 1000).
    pub scale: i32,
    pub percent: i32,
    pub has_at: bool,
    pub has_digits: bool,
    pub exp: bool,
    pub fraction: bool,
    pub currency: bool,
    pub fill: bool,
    /// Locale id from a `[$-409]` tag: month/day names and AM/PM designators follow it.
    pub lcid: Option<u16>,
}

/// Splits a code into sections at unquoted, unescaped, unbracketed semicolons.
fn split_sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut escape = false;
    for c in code.chars() {
        let cur = match out.last_mut() {
            Some(c) => c,
            None => break,
        };
        if escape {
            cur.push(c);
            escape = false;
            continue;
        }
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '\\' | '_' | '*' if !quoted && !bracket => {
                escape = true;
                cur.push(c);
            }
            '[' if !quoted => {
                bracket = true;
                cur.push(c);
            }
            ']' if !quoted => {
                bracket = false;
                cur.push(c);
            }
            ';' if !quoted && !bracket => {
                if out.len() >= MAX_SECTIONS {
                    break;
                }
                out.push(String::new());
            }
            _ => cur.push(c),
        }
    }
    out
}

fn starts_with_ci(cs: &[char], i: usize, pat: &str) -> bool {
    let mut k = i;
    for p in pat.chars() {
        match cs.get(k) {
            Some(c) if c.eq_ignore_ascii_case(&p) => k += 1,
            _ => return false,
        }
    }
    true
}

fn push_lit(toks: &mut Vec<Tok>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Tok::Lit(last)) = toks.last_mut() {
        last.push_str(s);
    } else {
        toks.push(Tok::Lit(s.to_string()));
    }
}

fn is_currency_char(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥' | '₹' | '₩' | '₽' | '¢' | '₪' | '₫' | '฿')
}

fn parse_color(lc: &str) -> Option<FormatColor> {
    Some(match lc {
        "black" => FormatColor::Black,
        "blue" => FormatColor::Blue,
        "cyan" => FormatColor::Cyan,
        "green" => FormatColor::Green,
        "magenta" => FormatColor::Magenta,
        "red" => FormatColor::Red,
        "white" => FormatColor::White,
        "yellow" => FormatColor::Yellow,
        _ => {
            let n: u8 = lc.strip_prefix("color")?.trim().parse().ok()?;
            if (1..=56).contains(&n) {
                FormatColor::Indexed(n)
            } else {
                return None;
            }
        }
    })
}

fn parse_cond(s: &str) -> Option<Cond> {
    let s = s.trim();
    let (op, rest) = if let Some(r) = s.strip_prefix("<=") {
        (CondOp::Le, r)
    } else if let Some(r) = s.strip_prefix(">=") {
        (CondOp::Ge, r)
    } else if let Some(r) = s.strip_prefix("<>") {
        (CondOp::Ne, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (CondOp::Lt, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (CondOp::Gt, r)
    } else {
        (CondOp::Eq, s.strip_prefix('=')?)
    };
    let value: f64 = rest.trim().parse().ok()?;
    value.is_finite().then_some(Cond { op, value })
}

struct Raw {
    toks: Vec<Tok>,
    color: Option<FormatColor>,
    cond: Option<Cond>,
    currency: bool,
    lcid: Option<u16>,
}

fn tokenize(src: &str) -> Raw {
    let cs: Vec<char> = src.chars().collect();
    let mut raw = Raw { toks: Vec::new(), color: None, cond: None, currency: false, lcid: None };
    let toks = &mut raw.toks;
    let mut i = 0usize;
    while let Some(&c) = cs.get(i) {
        match c {
            '"' => {
                let mut s = String::new();
                i += 1;
                while let Some(&d) = cs.get(i) {
                    i += 1;
                    if d == '"' {
                        break;
                    }
                    s.push(d);
                }
                if s.chars().any(is_currency_char) {
                    raw.currency = true;
                }
                push_lit(toks, &s);
                continue;
            }
            '\\' => {
                if let Some(&d) = cs.get(i + 1) {
                    if is_currency_char(d) {
                        raw.currency = true;
                    }
                    push_lit(toks, d.encode_utf8(&mut [0; 4]));
                }
                i += 2;
                continue;
            }
            '_' => {
                if cs.get(i + 1).is_some() {
                    push_lit(toks, " ");
                }
                i += 2;
                continue;
            }
            '*' => {
                if let Some(&d) = cs.get(i + 1) {
                    toks.push(Tok::Fill(d));
                }
                i += 2;
                continue;
            }
            '[' => {
                let close = cs.get(i + 1..).and_then(|r| r.iter().position(|&d| d == ']'));
                if let Some(p) = close {
                    let content: String = cs.get(i + 1..i + 1 + p).unwrap_or(&[]).iter().collect();
                    bracket(&mut raw_parts(toks, &mut raw.color, &mut raw.cond, &mut raw.currency, &mut raw.lcid), &content);
                    i += p + 2;
                } else {
                    push_lit(toks, "[");
                    i += 1;
                }
                continue;
            }
            '0' | '#' | '?' => toks.push(Tok::Digit(c)),
            '.' => toks.push(Tok::Point),
            ',' => toks.push(Tok::Comma),
            '%' => toks.push(Tok::Percent),
            '@' => toks.push(Tok::At),
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
                if matches!(cs.get(i), Some('1'..='9')) {
                    let mut s = String::new();
                    while let Some(&d) = cs.get(i) {
                        if !d.is_ascii_digit() {
                            break;
                        }
                        s.push(d);
                        i += 1;
                    }
                    toks.push(Tok::FixedDenom(s));
                }
                continue;
            }
            'e' | 'E' if matches!(cs.get(i + 1), Some('+' | '-')) => {
                toks.push(Tok::Exp { upper: c == 'E', plus: cs.get(i + 1) == Some(&'+') });
                i += 2;
                continue;
            }
            'G' | 'g' if starts_with_ci(&cs, i, "general") => {
                toks.push(Tok::General);
                i += 7;
                continue;
            }
            'A' | 'a' if starts_with_ci(&cs, i, "am/pm") => {
                toks.push(Tok::Date(DatePart::AmPm));
                i += 5;
                continue;
            }
            'A' | 'a' if starts_with_ci(&cs, i, "a/p") => {
                let p = cs.get(i + 2).copied().unwrap_or('P');
                toks.push(Tok::Date(DatePart::AP(c, p)));
                i += 3;
                continue;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | 'e' | 'E' => {
                let lc = c.to_ascii_lowercase();
                let mut n = 0usize;
                while cs.get(i + n).is_some_and(|d| d.to_ascii_lowercase() == lc) {
                    n += 1;
                }
                let n8 = n.min(255) as u8;
                toks.push(Tok::Date(match lc {
                    'y' => DatePart::Year(n8),
                    'e' => DatePart::Year(4),
                    'm' => DatePart::Month(n8),
                    'd' => DatePart::Day(n8),
                    'h' => DatePart::Hour(n8),
                    _ => DatePart::Second(n8),
                }));
                i += n.max(1);
                continue;
            }
            _ => {
                if is_currency_char(c) {
                    raw.currency = true;
                }
                push_lit(toks, c.encode_utf8(&mut [0; 4]));
            }
        }
        i += 1;
    }
    raw
}

struct RawParts<'a> {
    toks: &'a mut Vec<Tok>,
    color: &'a mut Option<FormatColor>,
    cond: &'a mut Option<Cond>,
    currency: &'a mut bool,
    lcid: &'a mut Option<u16>,
}

fn raw_parts<'a>(
    toks: &'a mut Vec<Tok>,
    color: &'a mut Option<FormatColor>,
    cond: &'a mut Option<Cond>,
    currency: &'a mut bool,
    lcid: &'a mut Option<u16>,
) -> RawParts<'a> {
    RawParts { toks, color, cond, currency, lcid }
}

/// Handles the content of a `[...]` tag.
fn bracket(p: &mut RawParts<'_>, content: &str) {
    let lc = content.trim().to_ascii_lowercase();
    if let Some(color) = parse_color(&lc) {
        *p.color = Some(color);
        return;
    }
    if lc.starts_with(['<', '>', '=']) {
        if let Some(c) = parse_cond(&lc) {
            *p.cond = Some(c);
        }
        return;
    }
    if let Some(body) = content.strip_prefix('$') {
        let sym = body.split('-').next().unwrap_or("");
        if !sym.is_empty() {
            *p.currency = true;
            push_lit(p.toks, sym);
        }
        // `[$-407]`, `[$€-407]`: the hex locale id after the last dash; the high word carries
        // calendar and numeral flags.
        if let Some((_, id)) = body.rsplit_once('-')
            && let Ok(v) = u32::from_str_radix(id.trim(), 16)
        {
            *p.lcid = Some((v & 0xFFFF) as u16);
        }
        return;
    }
    let mut chars = lc.chars();
    if let Some(first) = chars.next()
        && matches!(first, 'h' | 'm' | 's')
        && lc.chars().all(|c| c == first)
    {
        let n = lc.chars().count().min(255) as u8;
        p.toks.push(Tok::Date(match first {
            'h' => DatePart::ElapsedH(n),
            'm' => DatePart::ElapsedM(n),
            _ => DatePart::ElapsedS(n),
        }));
    }
    // Anything else ([DBNum1], [natural], locale tags…) is ignored.
}

fn date_part(t: &Tok) -> Option<DatePart> {
    if let Tok::Date(d) = t { Some(*d) } else { None }
}

fn analyze(raw: Raw) -> Section {
    let Raw { mut toks, color, cond, currency, lcid } = raw;
    let is_date = toks.iter().any(|t| matches!(t, Tok::Date(_)));
    let has_general = toks.iter().any(|t| matches!(t, Tok::General));
    let mut sec = Section {
        toks: Vec::new(),
        color,
        cond,
        kind: SecKind::Number,
        thousands: false,
        scale: 0,
        percent: 0,
        has_at: toks.iter().any(|t| matches!(t, Tok::At)),
        has_digits: false,
        exp: false,
        fraction: false,
        currency,
        fill: toks.iter().any(|t| matches!(t, Tok::Fill(_))),
        lcid,
    };
    if is_date {
        sec.kind = SecKind::Date;
        // m/mm after hours or before seconds means minutes.
        for i in 0..toks.len() {
            let Some(Tok::Date(DatePart::Month(n))) = toks.get(i) else { continue };
            let n = *n;
            if n > 2 {
                continue;
            }
            let prev = toks.get(..i).and_then(|s| s.iter().rev().find_map(date_part));
            let next = toks.get(i + 1..).and_then(|s| s.iter().find_map(date_part));
            let after_h = matches!(prev, Some(DatePart::Hour(_) | DatePart::ElapsedH(_)));
            let before_s = matches!(next, Some(DatePart::Second(_) | DatePart::ElapsedS(_)));
            if (after_h || before_s)
                && let Some(t) = toks.get_mut(i)
            {
                *t = Tok::Date(DatePart::Minute(n));
            }
        }
        let mut out = Vec::with_capacity(toks.len());
        let mut i = 0;
        while let Some(t) = toks.get(i) {
            if matches!(t, Tok::Point) && toks.get(i + 1) == Some(&Tok::Digit('0')) {
                let mut n = 0usize;
                while toks.get(i + 1 + n) == Some(&Tok::Digit('0')) {
                    n += 1;
                }
                out.push(Tok::Date(DatePart::SubSec(n.min(255) as u8)));
                i += 1 + n;
                continue;
            }
            match t {
                Tok::Date(_) | Tok::Fill(_) => out.push(t.clone()),
                other => push_lit(&mut out, &other.literal_text()),
            }
            i += 1;
        }
        sec.toks = out;
        return sec;
    }
    if has_general {
        sec.kind = SecKind::General;
        let mut out = Vec::with_capacity(toks.len());
        for t in &toks {
            match t {
                Tok::General | Tok::Fill(_) => out.push(t.clone()),
                other => push_lit(&mut out, &other.literal_text()),
            }
        }
        sec.toks = out;
        return sec;
    }
    sec.has_digits = toks.iter().any(|t| matches!(t, Tok::Digit(_)));
    if sec.has_at && !sec.has_digits {
        sec.kind = SecKind::Text;
        let mut out = Vec::with_capacity(toks.len());
        for t in &toks {
            match t {
                Tok::At | Tok::Fill(_) => out.push(t.clone()),
                other => push_lit(&mut out, &other.literal_text()),
            }
        }
        sec.toks = out;
        return sec;
    }
    // Number section.
    let mut seen_point = false;
    let mut seen_exp = false;
    let mut seen_digit = false;
    let mut fraction_slash = false;
    for i in 0..toks.len() {
        let Some(t) = toks.get(i).cloned() else { break };
        let replace = match t {
            Tok::Digit(_) => {
                seen_digit = true;
                None
            }
            Tok::Point => {
                if seen_point || seen_exp {
                    Some(Tok::Lit(".".into()))
                } else {
                    seen_point = true;
                    None
                }
            }
            Tok::Percent => {
                sec.percent += 1;
                None
            }
            Tok::Exp { .. } => {
                if seen_exp || !seen_digit {
                    Some(Tok::Lit(t.literal_text()))
                } else {
                    seen_exp = true;
                    sec.exp = true;
                    None
                }
            }
            Tok::Comma => {
                let next = toks.get(i + 1..).and_then(|s| s.iter().find(|t| !matches!(t, Tok::Comma)));
                let next_digit = matches!(next, Some(Tok::Digit(_)));
                if seen_digit && next_digit && !seen_point && !seen_exp {
                    sec.thousands = true;
                    Some(Tok::Lit(String::new()))
                } else if seen_digit && !next_digit {
                    sec.scale += 1;
                    Some(Tok::Lit(String::new()))
                } else {
                    Some(Tok::Lit(",".into()))
                }
            }
            Tok::Slash => {
                let next_ok = matches!(toks.get(i + 1), Some(Tok::Digit(_) | Tok::FixedDenom(_)));
                let has_exp = toks.iter().any(|t| matches!(t, Tok::Exp { .. }));
                if !fraction_slash && seen_digit && next_ok && !seen_point && !has_exp {
                    fraction_slash = true;
                    sec.fraction = true;
                    None
                } else {
                    Some(Tok::Lit("/".into()))
                }
            }
            Tok::FixedDenom(ref s) => {
                if matches!(i.checked_sub(1).and_then(|p| toks.get(p)), Some(Tok::Slash)) && sec.fraction {
                    None
                } else {
                    Some(Tok::Lit(s.clone()))
                }
            }
            _ => None,
        };
        if let Some(r) = replace
            && let Some(slot) = toks.get_mut(i)
        {
            *slot = r;
        }
    }
    if sec.fraction {
        // A decimal point in a fraction section is literal.
        for t in toks.iter_mut() {
            if matches!(t, Tok::Point) {
                *t = Tok::Lit(".".into());
            }
        }
    }
    // Merge literals (removed commas left empty ones).
    let mut out = Vec::with_capacity(toks.len());
    for t in toks {
        match t {
            Tok::Lit(s) => push_lit(&mut out, &s),
            other => out.push(other),
        }
    }
    sec.toks = out;
    sec
}

/// Parses a whole format code into its sections.
pub(crate) fn parse_sections(code: &str) -> Vec<Section> {
    split_sections(code).iter().map(|s| analyze(tokenize(s))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_split() {
        assert_eq!(split_sections("0;-0;;@").len(), 4);
        assert_eq!(split_sections("\"a;b\"0").len(), 1);
        assert_eq!(split_sections("\\;0").len(), 1);
        assert_eq!(split_sections("0;0;0;0;0;0").len(), 4);
    }

    #[test]
    fn minutes_vs_months() {
        let s = parse_sections("h:mm");
        assert!(s[0].toks.contains(&Tok::Date(DatePart::Minute(2))));
        let s = parse_sections("m/d");
        assert!(s[0].toks.contains(&Tok::Date(DatePart::Month(1))));
        let s = parse_sections("mm:ss");
        assert!(s[0].toks.contains(&Tok::Date(DatePart::Minute(2))));
    }

    #[test]
    fn commas() {
        let s = &parse_sections("#,##0,")[0];
        assert!(s.thousands);
        assert_eq!(s.scale, 1);
        let s = &parse_sections("0.0,,")[0];
        assert!(!s.thousands);
        assert_eq!(s.scale, 2);
    }
}
