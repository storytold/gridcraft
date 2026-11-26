//! Format code parsing: sections, bracket tags and tokens.

use crate::FormatColor;
use crate::calendar::Calendar;

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
    /// `g`, `gg`, `ggg`: Japanese era name (Latin initial, one kanji, full name).
    EraName(u8),
    /// `e`, `ee`: Japanese era year, unpadded or zero-padded (`元` in a gannen section).
    EraYear(u8),
}

impl DatePart {
    /// `[h]`, `[m]` or `[s]`.
    pub(crate) fn is_elapsed(self) -> bool {
        matches!(self, DatePart::ElapsedH(_) | DatePart::ElapsedM(_) | DatePart::ElapsedS(_))
    }

    pub(crate) fn is_calendar(self) -> bool {
        matches!(self, DatePart::Year(_) | DatePart::Month(_) | DatePart::Day(_) | DatePart::EraName(_) | DatePart::EraYear(_))
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
    /// A run of `g` before analysis: an era name in a Japanese-calendar section, the letters
    /// themselves anywhere else.
    EraName(String),
    /// An `e`/`ee` run before analysis: an era year in a Japanese-calendar section, a 4-digit
    /// year anywhere else.
    EraYear(u8),
}

impl Tok {
    /// Appends the text this token shows when it has no special meaning in its section.
    pub(crate) fn push_literal(&self, out: &mut String) {
        match self {
            Tok::Lit(s) | Tok::FixedDenom(s) | Tok::EraName(s) => out.push_str(s),
            Tok::Digit(c) => out.push(*c),
            Tok::Point => out.push('.'),
            Tok::Comma => out.push(','),
            Tok::Percent => out.push('%'),
            Tok::Exp { upper, plus } => {
                out.push(if *upper { 'E' } else { 'e' });
                out.push(if *plus { '+' } else { '-' });
            }
            Tok::Slash => out.push('/'),
            Tok::EraYear(n) => out.push_str(&"e".repeat(*n as usize)),
            Tok::Fill(_) | Tok::At | Tok::General | Tok::Date(_) => {}
        }
    }

    /// The text this token shows when it has no special meaning in its section.
    pub(crate) fn literal_text(&self) -> String {
        let mut s = String::new();
        self.push_literal(&mut s);
        s
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
    /// `[$-411]`, `[$-ja]` or `[$-ja-JP]`: this section's `g`/`e` tokens are Japanese era tokens.
    pub japanese: bool,
    /// The section also carried the gannen tag (`[$-ja-JP-x-gannen]`, `[$-x-gannen411]`): era
    /// year 1 shows `元`.
    pub gannen: bool,
    /// Date sections: the most digits of any `.0`/`.00`/`.000` part (at most 3), which decides how
    /// the time of day is rounded.
    pub sub_digits: u8,
    /// Date sections: has a year, month or day part, so the serial's date is needed.
    pub needs_date: bool,
    /// Date sections: has `AM/PM` or `A/P`, so hours show on the 12-hour clock.
    pub ampm: bool,
    pub calendar: Calendar,
    /// Excel rejects the whole code because of this section: a `B` and digit that is not a
    /// calendar prefix, or a prefix before a number format (`B20.00`).
    pub invalid: bool,
}

/// Splits a code into sections at unquoted, unescaped, unbracketed semicolons.
///
/// A `[` opens a bracket only when a `]` follows it; the bracket then runs to the next `]` (quotes
/// inside it are not special), exactly as `tokenize` reads it. An unclosed `[` is a literal in both
/// passes, so a later `;` still separates sections. Linear: each `[` either jumps over its bracket
/// or, with no `]` left in the code, is a literal at once.
fn split_sections(code: &str) -> Vec<String> {
    let cs: Vec<char> = code.chars().collect();
    let last_close = cs.iter().rposition(|&c| c == ']');
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut escape = false;
    let mut i = 0usize;
    while let Some(&c) = cs.get(i) {
        i += 1;
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
            '\\' | '_' | '*' if !quoted => {
                escape = true;
                cur.push(c);
            }
            '[' if !quoted && last_close.is_some_and(|l| l >= i) => {
                // `i` is already past the `[`; the closing bracket is at or after it.
                cur.push(c);
                while let Some(&d) = cs.get(i) {
                    i += 1;
                    cur.push(d);
                    if d == ']' {
                        break;
                    }
                }
            }
            ';' if !quoted => {
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
    calendar: Calendar,
    /// The section's calendar prefix as written, `'1'` or `'2'` (whether or not a locale tag
    /// cancels it).
    prefix: Option<char>,
    /// An unquoted `B` and digit that is not a calendar prefix: Excel rejects the code.
    invalid: bool,
    japanese: bool,
    gannen: bool,
}

/// Where a section's body starts: after its leading `[...]` tags, read as `tokenize` reads them.
/// Also whether one of those tags is a `[$...]` locale tag.
fn after_leading_brackets(cs: &[char]) -> (usize, bool) {
    let mut i = 0usize;
    let mut locale = false;
    while cs.get(i) == Some(&'[') {
        match cs.get(i + 1..).and_then(|r| r.iter().position(|&d| d == ']')) {
            Some(p) => {
                locale |= cs.get(i + 1) == Some(&'$');
                i += p + 2;
            }
            None => break,
        }
    }
    (i, locale)
}

fn tokenize(src: &str) -> Raw {
    let cs: Vec<char> = src.chars().collect();
    let mut raw = Raw {
        toks: Vec::new(),
        color: None,
        cond: None,
        currency: false,
        calendar: Calendar::Gregorian,
        prefix: None,
        invalid: false,
        japanese: false,
        gannen: false,
    };
    let toks = &mut raw.toks;
    // With no `]` at or after a `[`, that `[` is a literal; checking this once keeps the scan linear.
    let last_close = cs.iter().rposition(|&c| c == ']');
    let (body_start, locale_first) = after_leading_brackets(&cs);
    // Where the next calendar prefix may stand: the body's start, or right after another prefix.
    let mut prefix_at = body_start;
    let mut i = 0usize;
    while let Some(&c) = cs.get(i) {
        match c {
            // A calendar prefix, never shown: `B1` or `B2`, any case, which Excel accepts repeated
            // (`B2B2`) but not mixed (`B2B1`) or with more digits (`B12`, `B02`).
            'B' | 'b' if i == prefix_at && cs.get(i + 1).is_some_and(char::is_ascii_digit) => {
                let d = cs.get(i + 1).copied();
                let ok = matches!(d, Some('1' | '2')) && !cs.get(i + 2).is_some_and(char::is_ascii_digit) && raw.prefix.is_none_or(|p| Some(p) == d);
                raw.invalid |= !ok;
                raw.prefix = d;
                i += 2;
                prefix_at = i;
                continue;
            }
            // Any other `B` and digit (`B3dd`, `dd/mm/yyyyB2`, `dd B2 mm`, `B2[Red]B2`) makes Excel
            // reject the code.
            'B' | 'b' if cs.get(i + 1).is_some_and(char::is_ascii_digit) => {
                raw.invalid = true;
                push_lit(toks, c.encode_utf8(&mut [0; 4]));
            }
            '"' => {
                let (s, next) = quoted_literal(&cs, i);
                if s.chars().any(is_currency_char) {
                    raw.currency = true;
                }
                push_lit(toks, &s);
                i = next;
                continue;
            }
            '\\' => {
                if let Some(&d) = cs.get(i + 1) {
                    if is_currency_char(d) {
                        raw.currency = true;
                    }
                    // Excel rejects an escaped `\B2` as well.
                    raw.invalid |= matches!(d, 'B' | 'b') && cs.get(i + 2).is_some_and(char::is_ascii_digit);
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
                if let Some(next) = bracket_end(&cs, i, last_close) {
                    let content: String = cs.get(i + 1..next - 1).unwrap_or(&[]).iter().collect();
                    bracket(&mut raw_parts(toks, &mut raw.color, &mut raw.cond, &mut raw.currency, &mut raw.japanese, &mut raw.gannen), &content);
                    i = next;
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
                i = slash_token(&cs, i, toks);
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
            'G' | 'g' => {
                let (tok, next) = era_name_run(&cs, i);
                toks.push(tok);
                i = next;
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
                let (tok, next) = date_run(&cs, i, c);
                toks.push(tok);
                i = next;
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
    // Excel keeps the date Gregorian when a `[$-401]`-style locale or currency tag comes before
    // `B2` (`B2[$-401]` is still Hijri).
    if raw.prefix == Some('2') && !locale_first {
        raw.calendar = Calendar::Hijri;
    }
    raw
}

/// The `"`-quoted literal starting at `i`: its text without the quotes, and the index after it.
/// Excel treats an unterminated quote as running to the end of the format.
fn quoted_literal(cs: &[char], mut i: usize) -> (String, usize) {
    let mut s = String::new();
    i += 1;
    while let Some(&d) = cs.get(i) {
        i += 1;
        if d == '"' {
            break;
        }
        s.push(d);
    }
    (s, i)
}

/// The run of `y`/`m`/`d`/`h`/`s`/`e` at `i` (any case): its token before `analyze` resolves a
/// Japanese calendar's era year, and the index after the run.
fn date_run(cs: &[char], i: usize, c: char) -> (Tok, usize) {
    let lc = c.to_ascii_lowercase();
    let mut n = 0usize;
    while cs.get(i + n).is_some_and(|d| d.to_ascii_lowercase() == lc) {
        n += 1;
    }
    let n8 = n.min(255) as u8;
    let tok = if lc == 'e' {
        // An era year in a Japanese-calendar section, today's 4-digit year otherwise.
        Tok::EraYear(n8)
    } else {
        Tok::Date(match lc {
            'y' => DatePart::Year(n8),
            'm' => DatePart::Month(n8),
            'd' => DatePart::Day(n8),
            'h' => DatePart::Hour(n8),
            _ => DatePart::Second(n8),
        })
    };
    (tok, i + n.max(1))
}

/// The `/` at `i`: pushes it, and any run of digits after it as the fixed denominator, and
/// returns the index after them.
fn slash_token(cs: &[char], i: usize, toks: &mut Vec<Tok>) -> usize {
    toks.push(Tok::Slash);
    let mut j = i + 1;
    if matches!(cs.get(j), Some('1'..='9')) {
        let mut s = String::new();
        while let Some(&d) = cs.get(j) {
            if !d.is_ascii_digit() {
                break;
            }
            s.push(d);
            j += 1;
        }
        toks.push(Tok::FixedDenom(s));
    }
    j
}

/// The run of `g`/`G` at `i` as typed: an era name in a Japanese-calendar section, the letters as
/// typed anywhere else (`analyze` decides once the whole section, tag included, is read).
fn era_name_run(cs: &[char], i: usize) -> (Tok, usize) {
    let mut j = i;
    let mut s = String::new();
    while let Some(&d) = cs.get(j) {
        if !matches!(d, 'g' | 'G') {
            break;
        }
        s.push(d);
        j += 1;
    }
    (Tok::EraName(s), j)
}

/// The index just past the `]` that closes the `[` tag at `i`, when a `]` stands at or after `i`
/// (`last_close` is the last `]` in `cs`); `None` means the `[` is literal text.
fn bracket_end(cs: &[char], i: usize, last_close: Option<usize>) -> Option<usize> {
    if !last_close.is_some_and(|l| l > i) {
        return None;
    }
    let p = cs.get(i + 1..).and_then(|r| r.iter().position(|&d| d == ']'))?;
    Some(i + p + 2)
}

struct RawParts<'a> {
    toks: &'a mut Vec<Tok>,
    color: &'a mut Option<FormatColor>,
    cond: &'a mut Option<Cond>,
    currency: &'a mut bool,
    japanese: &'a mut bool,
    gannen: &'a mut bool,
}

fn raw_parts<'a>(
    toks: &'a mut Vec<Tok>,
    color: &'a mut Option<FormatColor>,
    cond: &'a mut Option<Cond>,
    currency: &'a mut bool,
    japanese: &'a mut bool,
    gannen: &'a mut bool,
) -> RawParts<'a> {
    RawParts { toks, color, cond, currency, japanese, gannen }
}

/// `[$-411]`, `[$-ja]` and `[$-ja-JP]` select the Japanese calendar; the tag also says whether
/// the section uses the gannen form (`[$-ja-JP-x-gannen]`, legacy `[$-x-gannen411]`), where era
/// year 1 shows `元`. Everything else is `None` and stays ignored. Case-insensitive; `lc` is the
/// lowercased trimmed bracket content.
fn japanese_tag(lc: &str) -> Option<bool> {
    let body = lc.strip_prefix("$-")?;
    let first = body.split('-').next().unwrap_or("");
    // `411` is the ja-JP LCID, `ja`/`ja-JP` the language tag; the legacy gannen tag names the
    // LCID inside its own marker. `[$-x-gannen411]` is the legacy spelling: Excel 16.0 (en-US)
    // rejects it as a number format, so it is read leniently here rather than for parity.
    let japanese = first == "411" || first == "ja" || body == "x-gannen411";
    japanese.then(|| body.contains("gannen"))
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
        if let Some(gannen) = japanese_tag(&lc) {
            *p.japanese = true;
            *p.gannen = gannen;
            return;
        }
        let sym = body.split('-').next().unwrap_or("");
        if !sym.is_empty() {
            *p.currency = true;
            push_lit(p.toks, sym);
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
    let Raw { mut toks, color, cond, currency, calendar, prefix, invalid, japanese, gannen } = raw;
    // `g` and `e` mean an era name and an era year only in a Japanese-calendar section, so they
    // are resolved here, once the section's bracket tags (which may follow them) have been read.
    for t in toks.iter_mut() {
        let resolved = match t {
            Tok::EraName(s) if japanese => Tok::Date(DatePart::EraName(s.chars().count().min(255) as u8)),
            Tok::EraYear(n) if japanese => Tok::Date(DatePart::EraYear(*n)),
            // Without the tag: `ggg` is literal text and `e` the 4-digit year it always was.
            Tok::EraName(s) => Tok::Lit(s.clone()),
            Tok::EraYear(_) => Tok::Date(DatePart::Year(4)),
            _ => continue,
        };
        *t = resolved;
    }
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
        japanese,
        gannen,
        sub_digits: 0,
        needs_date: false,
        ampm: false,
        calendar,
        invalid,
    };
    if is_date {
        sec.kind = SecKind::Date;
        return analyze_date(toks, sec);
    }
    if has_general {
        sec.kind = SecKind::General;
        return analyze_general(toks, sec);
    }
    sec.has_digits = toks.iter().any(|t| matches!(t, Tok::Digit(_)));
    if sec.has_at && !sec.has_digits {
        sec.kind = SecKind::Text;
        return analyze_text(toks, sec);
    }
    // Number section. Excel rejects a calendar prefix before digits (`B20.00`, `B1#,##0`);
    // `B2General`, `B2@` and `B2"x"` show as without the prefix.
    sec.invalid |= prefix.is_some() && sec.has_digits;
    analyze_number(toks, sec)
}

fn analyze_date(mut toks: Vec<Tok>, mut sec: Section) -> Section {
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
    sec.sub_digits = out.iter().filter_map(|t| if let Tok::Date(DatePart::SubSec(n)) = t { Some(*n) } else { None }).max().unwrap_or(0).min(3);
    sec.needs_date = out.iter().any(|t| matches!(t, Tok::Date(d) if d.is_calendar()));
    sec.ampm = out.iter().any(|t| matches!(t, Tok::Date(DatePart::AmPm | DatePart::AP(..))));
    sec.toks = out;
    sec
}

fn analyze_general(toks: Vec<Tok>, mut sec: Section) -> Section {
    let mut out = Vec::with_capacity(toks.len());
    for t in &toks {
        match t {
            Tok::General | Tok::Fill(_) => out.push(t.clone()),
            other => push_lit(&mut out, &other.literal_text()),
        }
    }
    sec.toks = out;
    sec
}

fn analyze_text(toks: Vec<Tok>, mut sec: Section) -> Section {
    let mut out = Vec::with_capacity(toks.len());
    for t in &toks {
        match t {
            Tok::At | Tok::Fill(_) => out.push(t.clone()),
            other => push_lit(&mut out, &other.literal_text()),
        }
    }
    sec.toks = out;
    sec
}

fn analyze_number(mut toks: Vec<Tok>, mut sec: Section) -> Section {
    // Number section.
    let mut seen_point = false;
    let mut seen_exp = false;
    let mut seen_digit = false;
    let mut fraction_slash = false;
    // Precomputed so each comma and slash is O(1) (a long run of them stays linear): whether the
    // first non-comma token after a position is a digit, and where the last exponent marker is.
    let mut digit_after_commas = vec![false; toks.len()];
    let mut after = false;
    for (j, t) in toks.iter().enumerate().rev() {
        if let Some(slot) = digit_after_commas.get_mut(j) {
            *slot = after;
        }
        if !matches!(t, Tok::Comma) {
            after = matches!(t, Tok::Digit(_));
        }
    }
    let last_exp = toks.iter().rposition(|t| matches!(t, Tok::Exp { .. }));
    let scientific = toks.iter().skip_while(|t| !matches!(t, Tok::Digit(_))).any(|t| matches!(t, Tok::Exp { .. }));
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
                let next_digit = digit_after_commas.get(i).copied().unwrap_or(false);
                if seen_digit && next_digit && !seen_point && !seen_exp {
                    sec.thousands = true;
                    Some(Tok::Lit(String::new()))
                } else if seen_digit && scientific {
                    // Observed: a scientific section never scales; any other comma after the
                    // first digit shows nothing (`#,e+#`, `#e+#,`, `#e+000,00`).
                    Some(Tok::Lit(String::new()))
                } else if seen_digit && !next_digit {
                    sec.scale += 1;
                    Some(Tok::Lit(String::new()))
                } else {
                    Some(Tok::Lit(",".into()))
                }
            }
            Tok::Slash => {
                // Literal text may sit between the bar and the denominator (`#/ #`, `# ?=/=?`).
                let next_ok = matches!(toks.get(i + 1), Some(Tok::FixedDenom(_)))
                    || matches!(toks.get(i + 1..).and_then(|s| s.iter().find(|t| !matches!(t, Tok::Lit(_)))), Some(Tok::Digit(_)));
                // An exponent kept before this bar (`seen_exp`) or any marker still ahead of it.
                let has_exp = seen_exp || last_exp.is_some_and(|l| l > i);
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
