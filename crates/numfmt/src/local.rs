//! Format codes in a locale's spelling (`#.##0,00`, `dd/mm/aaaa`, `TT.MM.JJJJ`, `Geral`) and back
//! to the canonical spelling stored in files.
//!
//! Excel shows and accepts codes in the display language's letters and the region's separators,
//! while the file always holds the invariant code. The conversion is lexical: literals (quoted
//! text, `\x`, `_x`, `*x`), colours, `[$…]` tags and fill characters pass through unchanged.
//! Within a section the decimal and group characters are translated only in sections that hold
//! digit placeholders, and in date sections only a decimal point before `0` (fractions of a
//! second) is translated, because every other `.` or `,` there is literal text.
//!
//! Date letters are case-insensitive, so a letter written in the other case than the language's
//! own spelling (`Y` in a canonical code, `t` in a German one) is written back with its case
//! swapped, which makes the conversion exact in both directions. An `m` is a minute when it
//! follows an hour or precedes a second, as when the code is parsed, and then uses the language's
//! minute letter (`m` where the month letter is `M`).
//!
//! An unquoted literal character that would be syntax in the other spelling (`a` in `0.0 a`, a
//! year letter in Portuguese) is escaped with a backslash on the way, and the escape is removed
//! again on the way back, so a code keeps its meaning and round-trips exactly. The `General`
//! keyword keeps the case it was written in (`general`, `GENERAL`).

use gridcraft_locale::Dialect;

/// Excel's limit for the length of one format code, in characters. Longer codes are neither
/// converted nor accepted by `TEXT()`.
pub(crate) const MAX_CODE_CHARS: usize = 255;

pub(crate) fn too_long(code: &str) -> bool {
    code.chars().nth(MAX_CODE_CHARS).is_some()
}

/// How one side of the conversion spells things.
#[derive(Clone, Copy)]
struct Spelling<'a> {
    year: char,
    month: char,
    minute: char,
    day: char,
    hour: char,
    second: char,
    general: &'a str,
    decimal: char,
    group: char,
}

const CANONICAL: Spelling<'static> =
    Spelling { year: 'y', month: 'm', minute: 'm', day: 'd', hour: 'h', second: 's', general: "General", decimal: '.', group: ',' };

impl<'a> Spelling<'a> {
    fn of(d: &Dialect<'a>) -> Spelling<'a> {
        let f = &d.names.format;
        Spelling {
            year: f.year,
            month: f.month,
            minute: f.minute,
            day: f.day,
            hour: f.hour,
            second: f.second,
            general: f.general,
            decimal: d.regional.decimal,
            group: d.regional.group,
        }
    }

    /// The kind of a date letter, ignoring case. The month and minute letters are told apart later
    /// by context, so both read as `Month` here.
    fn kind_of(&self, c: char) -> Option<Kind> {
        let same = |l: char| l.to_lowercase().eq(c.to_lowercase());
        if same(self.year) {
            Some(Kind::Year)
        } else if same(self.month) || same(self.minute) {
            Some(Kind::Month)
        } else if same(self.day) {
            Some(Kind::Day)
        } else if same(self.hour) {
            Some(Kind::Hour)
        } else if same(self.second) {
            Some(Kind::Second)
        } else {
            None
        }
    }

    fn letter(&self, k: Kind) -> char {
        match k {
            Kind::Year => self.year,
            Kind::Month => self.month,
            Kind::Minute => self.minute,
            Kind::Day => self.day,
            Kind::Hour => self.hour,
            Kind::Second => self.second,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Year,
    Month,
    Minute,
    Day,
    Hour,
    Second,
}

enum T {
    /// Copied unchanged.
    Lit(String),
    /// A date letter as written.
    Letter(Kind, char),
    /// `am/pm` or `a/p`: copied unchanged, but marks the section as a time format.
    AmPm(String),
    /// `[hh]`, `[mm]`, `[ss]`: elapsed time with its letters as written.
    Elapsed(Vec<(Kind, char)>),
    /// `[>1.5]`-style condition: the decimal point inside is translated.
    Cond(String),
    /// The `General` keyword as written: the source spelling's own word or the canonical one.
    General(String),
    /// An unquoted literal character.
    Plain(char),
    /// A backslash-escaped literal character.
    Esc(char),
    /// `0`, `#`, `?`.
    Digit(char),
    /// The source spelling's decimal character.
    Decimal(char),
    /// The source spelling's group character (the char as written).
    Group(char),
    /// ASCII space: a group separator only between two `#`/`0` placeholders when the source group
    /// character is white space.
    Space,
    Sep,
}

fn strip_prefix_ci(s: &str, word: &str) -> Option<usize> {
    if word.is_empty() {
        return None;
    }
    let mut it = s.char_indices();
    let mut end = 0;
    for w in word.chars() {
        let (i, c) = it.next()?;
        if !c.to_uppercase().eq(w.to_uppercase()) {
            return None;
        }
        end = i + c.len_utf8();
    }
    Some(end)
}

fn is_group_char(from: &Spelling<'_>, c: char) -> bool {
    c == from.group || (from.group.is_whitespace() && matches!(c, '\u{a0}' | '\u{202f}'))
}

fn lex(code: &str, from: &Spelling<'_>) -> Vec<T> {
    let mut out = Vec::new();
    let mut i = 0;
    // Once a `[` has no closing `]`, no later one has either: do not search the suffix again.
    let mut no_close = false;
    while let Some(rest) = code.get(i..).filter(|r| !r.is_empty()) {
        let Some(c) = rest.chars().next() else { break };
        let mut len = c.len_utf8();
        match c {
            '"' => {
                len = rest.get(1..).and_then(|r| r.find('"')).map_or(rest.len(), |p| p + 2);
                out.push(T::Lit(rest.get(..len).unwrap_or(rest).to_string()));
            }
            '\\' => match rest.get(len..).and_then(|r| r.chars().next()) {
                Some(n) => {
                    len += n.len_utf8();
                    out.push(T::Esc(n));
                }
                None => out.push(T::Lit("\\".into())),
            },
            '_' | '*' => {
                len += rest.get(len..).and_then(|r| r.chars().next()).map_or(0, char::len_utf8);
                out.push(T::Lit(rest.get(..len).unwrap_or(rest).to_string()));
            }
            '[' => match if no_close { None } else { rest.find(']') } {
                Some(p) => {
                    len = p + 1;
                    out.push(bracket(rest.get(1..p).unwrap_or(""), rest.get(..len).unwrap_or(rest), from));
                }
                None => {
                    no_close = true;
                    out.push(T::Lit("[".into()));
                }
            },
            ';' => out.push(T::Sep),
            'e' | 'E' if matches!(rest.chars().nth(1), Some('+' | '-')) => {
                len = 2;
                out.push(T::Lit(rest.get(..2).unwrap_or(rest).to_string()));
            }
            '0' | '#' | '?' => out.push(T::Digit(c)),
            ' ' => out.push(T::Space),
            _ => {
                if let Some(n) = strip_prefix_ci(rest, from.general).or_else(|| strip_prefix_ci(rest, CANONICAL.general)) {
                    len = n;
                    out.push(T::General(rest.get(..n).unwrap_or(rest).to_string()));
                } else if let Some(n) = strip_prefix_ci(rest, "am/pm").or_else(|| strip_prefix_ci(rest, "a/p")) {
                    len = n;
                    out.push(T::AmPm(rest.get(..n).unwrap_or(rest).to_string()));
                } else if let Some(k) = from.kind_of(c) {
                    out.push(T::Letter(k, c));
                } else if c == from.decimal {
                    out.push(T::Decimal(c));
                } else if is_group_char(from, c) {
                    out.push(T::Group(c));
                } else {
                    out.push(T::Plain(c));
                }
            }
        }
        i += len;
    }
    out
}

/// A `[...]` tag: elapsed time, a condition, or something copied unchanged (colours, `[$-409]`).
fn bracket(content: &str, whole: &str, from: &Spelling<'_>) -> T {
    if content.starts_with('$') {
        return T::Lit(whole.to_string());
    }
    if content.starts_with(['<', '>', '=']) {
        return T::Cond(content.to_string());
    }
    let kinds: Option<Vec<(Kind, char)>> = content
        .chars()
        .map(|c| {
            from.kind_of(c)
                .filter(|k| matches!(k, Kind::Hour | Kind::Month | Kind::Second))
                .map(|k| (if k == Kind::Month { Kind::Minute } else { k }, c))
        })
        .collect();
    match kinds {
        Some(k) if k.first().is_some_and(|first| k.iter().all(|(kind, _)| *kind == first.0)) => T::Elapsed(k),
        _ => T::Lit(whole.to_string()),
    }
}

/// What a run of date tokens is, for deciding which `m` is a minute.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Hour,
    Minute,
    Second,
    Month,
    Other,
}

/// Turns the month-or-minute letters of one section into minutes where the code parser would: a
/// run of at most two `m` right after an hour or right before a second.
fn resolve_minutes(sec: &mut [T]) {
    // (first token, run length, kind)
    let mut parts: Vec<(usize, usize, PartKind)> = Vec::new();
    for (i, t) in sec.iter().enumerate() {
        match t {
            T::Letter(k, _) => {
                let kind = match k {
                    Kind::Hour => PartKind::Hour,
                    Kind::Minute => PartKind::Minute,
                    Kind::Second => PartKind::Second,
                    Kind::Month => PartKind::Month,
                    Kind::Year | Kind::Day => PartKind::Other,
                };
                match parts.last_mut() {
                    Some((first, len, last)) if *last == kind && *first + *len == i => *len += 1,
                    _ => parts.push((i, 1, kind)),
                }
            }
            T::Elapsed(ks) => {
                let kind = match ks.first().map(|k| k.0) {
                    Some(Kind::Hour) => PartKind::Hour,
                    Some(Kind::Minute) => PartKind::Minute,
                    Some(Kind::Second) => PartKind::Second,
                    _ => PartKind::Other,
                };
                parts.push((i, 1, kind));
            }
            T::AmPm(_) => parts.push((i, 1, PartKind::Other)),
            _ => {}
        }
    }
    for j in 0..parts.len() {
        let Some(&(first, len, PartKind::Month)) = parts.get(j) else { continue };
        if len > 2 {
            continue;
        }
        let after_hour = j.checked_sub(1).and_then(|p| parts.get(p)).is_some_and(|p| p.2 == PartKind::Hour);
        let before_second = parts.get(j + 1).is_some_and(|p| p.2 == PartKind::Second);
        if after_hour || before_second {
            if let Some(p) = parts.get_mut(j) {
                p.2 = PartKind::Minute;
            }
            for t in sec.iter_mut().skip(first).take(len) {
                if let T::Letter(k, _) = t {
                    *k = Kind::Minute;
                }
            }
        }
    }
}

fn is_digit(t: Option<&T>) -> bool {
    matches!(t, Some(T::Digit('#' | '0')))
}

/// Whether the section holds a fraction (`# ?/?`, `# #/#`, `0 0/0`, `?/8`): a slash between a
/// digit placeholder and a digit. Spaces in a fraction are literal, never group separators.
fn has_fraction(sec: &[T]) -> bool {
    sec.iter().enumerate().any(|(j, t)| {
        matches!(t, T::Plain('/'))
            && matches!(j.checked_sub(1).and_then(|p| sec.get(p)), Some(T::Digit(_)))
            && matches!(sec.get(j + 1), Some(T::Digit(_) | T::Plain('0'..='9')))
    })
}

/// Whether `c` written without a backslash is syntax in this spelling rather than a literal: a
/// date letter, or in a number section the decimal or the group separator (in date and text
/// sections those two are plain text).
fn collides(sp: &Spelling<'_>, c: char, numeric: bool) -> bool {
    sp.kind_of(c).is_some() || (numeric && (c == sp.decimal || is_group_char(sp, c)))
}

/// Writes a date letter of the target spelling, swapping its case when the source letter did not
/// have the case of the source language's own spelling.
fn push_letter(out: &mut String, from: &Spelling<'_>, to: &Spelling<'_>, kind: Kind, written: char) {
    let letter = to.letter(kind);
    if written == from.letter(kind) {
        out.push(letter);
    } else if letter.is_uppercase() {
        out.extend(letter.to_lowercase());
    } else {
        out.extend(letter.to_uppercase());
    }
}

/// The `General` keyword of the target spelling, in the case the source used. A case variant is
/// used only where the target word has one; any other spelling (mixed case, or a case the target
/// word cannot show) is the canonical alias and is kept as written.
fn general_word(written: &str, from: &Spelling<'_>, to: &Spelling<'_>) -> String {
    let (lower, upper) = (to.general.to_lowercase(), to.general.to_uppercase());
    if written == from.general {
        to.general.to_string()
    } else if written == from.general.to_lowercase() && lower != to.general {
        lower
    } else if written == from.general.to_uppercase() && upper != to.general {
        upper
    } else {
        written.to_string()
    }
}

/// Translates `code` from one spelling to another.
fn convert(code: &str, from: &Spelling<'_>, to: &Spelling<'_>) -> String {
    let mut toks = lex(code, from);
    let mut out = String::with_capacity(code.len() + 8);
    for sec in toks.split_mut(|t| matches!(t, T::Sep)) {
        resolve_minutes(sec);
        let date = sec.iter().any(|t| matches!(t, T::Letter(..) | T::AmPm(_) | T::Elapsed(_)));
        let numeric = !date && sec.iter().any(|t| matches!(t, T::Digit(_)));
        let fraction = has_fraction(sec);
        for (i, t) in sec.iter().enumerate() {
            match t {
                T::Lit(s) | T::AmPm(s) => out.push_str(s),
                T::Plain(c) => {
                    if collides(to, *c, numeric) {
                        out.push('\\');
                    }
                    out.push(*c);
                }
                T::Esc(c) => {
                    if !(collides(from, *c, numeric) && !collides(to, *c, numeric)) {
                        out.push('\\');
                    }
                    out.push(*c);
                }
                T::Letter(k, c) => push_letter(&mut out, from, to, *k, *c),
                T::Elapsed(ks) => {
                    out.push('[');
                    for (k, c) in ks {
                        push_letter(&mut out, from, to, *k, *c);
                    }
                    out.push(']');
                }
                T::Cond(s) => {
                    out.push('[');
                    out.extend(s.chars().map(|c| if c == from.decimal { to.decimal } else { c }));
                    out.push(']');
                }
                T::General(w) => out.push_str(&general_word(w, from, to)),
                T::Digit(c) => out.push(*c),
                T::Decimal(c) => {
                    let sub_second = matches!(sec.get(i + 1), Some(T::Digit('0')));
                    out.push(if numeric || (date && sub_second) { to.decimal } else { *c });
                }
                T::Group(c) => out.push(if numeric { to.group } else { *c }),
                T::Space => {
                    let group = numeric
                        && !fraction
                        && from.group.is_whitespace()
                        && is_digit(i.checked_sub(1).and_then(|p| sec.get(p)))
                        && is_digit(sec.get(i + 1));
                    out.push(if group { to.group } else { ' ' });
                }
                T::Sep => {}
            }
        }
        out.push(';');
    }
    out.pop();
    out
}

/// The canonical code written in the dialect's letters and separators, as Excel shows it in the
/// Format Cells dialog and accepts it in `TEXT()`.
pub fn to_local_code(code: &str, d: &Dialect<'_>) -> String {
    if d.is_invariant() || too_long(code) {
        return code.to_string();
    }
    convert(code, &CANONICAL, &Spelling::of(d))
}

/// The canonical code of a code typed in the dialect's letters and separators.
pub fn from_local_code(code: &str, d: &Dialect<'_>) -> String {
    if d.is_invariant() || too_long(code) {
        return code.to_string();
    }
    convert(code, &Spelling::of(d), &CANONICAL)
}
