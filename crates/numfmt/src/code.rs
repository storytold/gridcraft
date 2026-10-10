//! Number format codes as an interface locale writes them (Format Cells ▸ Custom).
//!
//! Codes are stored in en-US (`#,##0.00`, `m/d/yyyy`, `General`). German Excel shows and takes
//! the same codes with its own letters and separators: `#.##0,00`, `TT.MM.JJJJ`, `Standard`,
//! `[Rot]`. Text in quotes, escaped characters, currency and locale brackets never change.

use gridcraft_core::Locale;

use crate::locale_pattern_code;

const DE_COLORS: [(&str, &str); 8] = [
    ("Black", "Schwarz"),
    ("Blue", "Blau"),
    ("Cyan", "Zyan"),
    ("Green", "Grün"),
    ("Magenta", "Magenta"),
    ("Red", "Rot"),
    ("White", "Weiß"),
    ("Yellow", "Gelb"),
];

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    /// `"text"`, without the quotes.
    Quoted(String),
    /// `\x`, `_x`, `*x`: the marker and its character.
    Escaped(char, char),
    /// `[...]`, without the brackets.
    Bracket(String),
    /// `AM/PM`, `A/P` as written.
    AmPm(String),
    /// `General` (`Standard` in German).
    General,
    /// A run of one letter, lowercased, and its length.
    Letters(char, usize),
    Sep,
    Char(char),
}

fn starts_with_ci(chars: &[char], at: usize, word: &str) -> bool {
    let mut i = at;
    for w in word.chars() {
        match chars.get(i) {
            Some(c) if c.to_lowercase().eq(w.to_lowercase()) => i += 1,
            _ => return false,
        }
    }
    true
}

fn pieces(code: &str, general_words: &[&str]) -> Vec<Piece> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    'scan: while let Some(&c) = chars.get(i) {
        match c {
            '"' => {
                let mut s = String::new();
                i += 1;
                while let Some(&d) = chars.get(i) {
                    i += 1;
                    if d == '"' {
                        break;
                    }
                    s.push(d);
                }
                out.push(Piece::Quoted(s));
            }
            '\\' | '_' | '*' => {
                match chars.get(i + 1) {
                    Some(&d) => out.push(Piece::Escaped(c, d)),
                    None => out.push(Piece::Char(c)),
                }
                i += 2;
            }
            '[' => {
                let mut s = String::new();
                i += 1;
                while let Some(&d) = chars.get(i) {
                    i += 1;
                    if d == ']' {
                        break;
                    }
                    s.push(d);
                }
                out.push(Piece::Bracket(s));
            }
            ';' => {
                out.push(Piece::Sep);
                i += 1;
            }
            c if c.is_alphabetic() => {
                for word in general_words {
                    if starts_with_ci(&chars, i, word) {
                        out.push(Piece::General);
                        i += word.chars().count();
                        continue 'scan;
                    }
                }
                for word in ["AM/PM", "A/P"] {
                    if starts_with_ci(&chars, i, word) {
                        let n = word.chars().count();
                        out.push(Piece::AmPm(chars.get(i..i + n).map(|s| s.iter().collect()).unwrap_or_default()));
                        i += n;
                        continue 'scan;
                    }
                }
                let lower = c.to_lowercase().next().unwrap_or(c);
                let mut n = 0;
                while chars.get(i + n).is_some_and(|d| d.to_lowercase().next() == Some(lower)) {
                    n += 1;
                }
                out.push(Piece::Letters(lower, n.max(1)));
                i += n.max(1);
            }
            c => {
                out.push(Piece::Char(c));
                i += 1;
            }
        }
    }
    out
}

/// The date or time letter a piece stands for (`d`, `m`, `y`, `h`, `s`), in en-US terms.
fn date_letter(p: &Piece, german: bool) -> Option<char> {
    match p {
        Piece::Letters(c, _) => match (*c, german) {
            ('d' | 'm' | 'y' | 'h' | 's', _) => Some(*c),
            ('t', true) => Some('d'),
            ('j', true) => Some('y'),
            _ => None,
        },
        Piece::Bracket(b) => {
            let l = b.to_lowercase();
            (!l.is_empty() && l.chars().all(|c| c == l.chars().next().unwrap_or(' ')) && matches!(l.chars().next(), Some('h' | 'm' | 's')))
                .then(|| l.chars().next().unwrap_or('h'))
        }
        Piece::AmPm(_) => Some('a'),
        _ => None,
    }
}

fn is_date_section(section: &[Piece], german: bool) -> bool {
    section.iter().any(|p| date_letter(p, german).is_some())
}

/// Whether the `m` at `i` means minutes: right after hours or right before seconds.
fn is_minute(section: &[Piece], i: usize, german: bool) -> bool {
    let prev = section.get(..i).and_then(|s| s.iter().rev().find_map(|p| date_letter(p, german)));
    let next = section.get(i + 1..).and_then(|s| s.iter().find_map(|p| date_letter(p, german)));
    prev == Some('h') || next == Some('s')
}

fn translate_bracket(inner: &str, to_local: bool) -> String {
    for (en, de) in DE_COLORS {
        let (from, to) = if to_local { (en, de) } else { (de, en) };
        if inner.to_lowercase() == from.to_lowercase() {
            return to.to_string();
        }
    }
    let (from, to) = if to_local { ("color", "Farbe") } else { ("farbe", "Color") };
    if let Some(n) = inner.get(..from.len()).filter(|p| p.eq_ignore_ascii_case(from)).and_then(|_| inner.get(from.len()..))
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
    {
        return format!("{to}{n}");
    }
    if inner.starts_with(['<', '>', '=']) {
        return if to_local { inner.replace('.', ",") } else { inner.replace(',', ".") };
    }
    inner.to_string()
}

fn write_section(out: &mut String, section: &[Piece], to_local: bool) {
    let german_in = !to_local;
    let date = is_date_section(section, german_in);
    for (i, p) in section.iter().enumerate() {
        match p {
            Piece::Quoted(s) => {
                // German Excel writes date punctuation unquoted (`TT.MM.JJJJ`).
                if to_local && date && !s.is_empty() && s.chars().all(|c| matches!(c, '.' | ' ' | ',' | ':' | '/' | '-')) {
                    out.push_str(s);
                } else {
                    out.push('"');
                    out.push_str(s);
                    out.push('"');
                }
            }
            Piece::Escaped(m, c) => {
                out.push(*m);
                out.push(*c);
            }
            Piece::Bracket(b) => {
                out.push('[');
                out.push_str(&translate_bracket(b, to_local));
                out.push(']');
            }
            Piece::AmPm(s) => out.push_str(s),
            Piece::General => out.push_str(if to_local { "Standard" } else { "General" }),
            Piece::Letters(c, n) => {
                let letter = match (date_letter(p, german_in), to_local) {
                    (Some('d'), true) if date => 'T',
                    (Some('y'), true) if date => 'J',
                    // German writes months `M` and minutes `m`.
                    (Some('m'), true) if date => {
                        if *n <= 2 && is_minute(section, i, german_in) {
                            'm'
                        } else {
                            'M'
                        }
                    }
                    (Some(l @ ('d' | 'y' | 'm')), false) if date => l,
                    _ if date && matches!(c, 'h' | 's') => *c,
                    _ => {
                        // Other letters (`E` of an exponent, calendar codes) keep their case.
                        out.push_str(&section_letters(*c, *n));
                        continue;
                    }
                };
                for _ in 0..*n {
                    out.push(letter);
                }
            }
            Piece::Char(c) => {
                let next_is_zero = matches!(section.get(i + 1), Some(Piece::Char('0')));
                let after_seconds = section.get(..i).and_then(|s| s.iter().rev().find_map(|p| date_letter(p, german_in))) == Some('s');
                let c = match (*c, date, to_local) {
                    // Fractions of a second: `ss.000` is `ss,000` in German.
                    ('.', true, true) if next_is_zero && after_seconds => ',',
                    (',', true, false) if next_is_zero && after_seconds => '.',
                    (_, true, _) => *c,
                    // Numbers swap the decimal and the thousands separator.
                    ('.', false, _) => ',',
                    (',', false, _) => '.',
                    _ => *c,
                };
                out.push(c);
            }
            Piece::Sep => out.push(';'),
        }
    }
}

/// A letter run that isn't a date part, as written (the case was lost when scanning, so `E`
/// of an exponent is written in upper case, other letters in lower case).
fn section_letters(c: char, n: usize) -> String {
    let c = if c == 'e' { 'E' } else { c };
    std::iter::repeat_n(c, n).collect()
}

fn translate(code: &str, loc: Locale, to_local: bool) -> String {
    if loc.is_en() {
        return code.to_string();
    }
    let words: &[&str] = if to_local { &["General"] } else { &["Standard", "General"] };
    let all = pieces(code, words);
    let mut out = String::with_capacity(code.len() + 4);
    for (k, section) in all.split(|p| *p == Piece::Sep).enumerate() {
        if k > 0 {
            out.push(';');
        }
        write_section(&mut out, section, to_local);
    }
    out
}

/// A stored (en-US) format code as `loc` writes it: `#,##0.00` → `#.##0,00`, `m/d/yyyy` →
/// `TT.MM.JJJJ`, `General` → `Standard` in German.
pub fn code_to_local(code: &str, loc: Locale) -> String {
    match locale_pattern_code(code, loc) {
        Some(pattern) => translate(pattern, loc, true),
        None => translate(code, loc, true),
    }
}

/// A format code typed in `loc` as the en-US code to store: `TT.MM.JJJJ` → `dd.mm.yyyy`,
/// `#.##0,00 €` → `#,##0.00 €`. English codes typed in German are read as German, as German
/// Excel does, except `General`.
pub fn code_from_local(code: &str, loc: Locale) -> String {
    translate(code, loc, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NumberFormat, format_value_in};
    use gridcraft_core::{DateSystem, Value};

    const DE: Locale = Locale::De;

    #[test]
    fn german_codes() {
        let cases = [
            ("General", "Standard"),
            ("0", "0"),
            ("0.00", "0,00"),
            ("#,##0", "#.##0"),
            ("#,##0.00", "#.##0,00"),
            ("#,##0;[Red]-#,##0", "#.##0;[Rot]-#.##0"),
            ("0.00%", "0,00%"),
            ("0.00E+00", "0,00E+00"),
            ("# ?/?", "# ?/?"),
            ("#,##0.00 \"€\"", "#.##0,00 \"€\""),
            ("[$€-407]#,##0.00", "[$€-407]#.##0,00"),
            ("[>=1000.5][Color10]#,##0", "[>=1000,5][Farbe10]#.##0"),
            ("m/d/yyyy", "TT.MM.JJJJ"),
            ("d-mmm-yy", "TT. MMM JJ"),
            ("[$-F800]dddd, mmmm dd, yyyy", "TTTT, T. MMMM JJJJ"),
            ("[$-F400]h:mm:ss AM/PM", "hh:mm:ss"),
            ("mmmm d, yyyy", "MMMM T, JJJJ"),
            ("h:mm AM/PM", "h:mm AM/PM"),
            ("[h]:mm:ss", "[h]:mm:ss"),
            ("mm:ss.0", "mm:ss,0"),
            ("dd.mm.yyyy hh:mm:ss.000", "TT.MM.JJJJ hh:mm:ss,000"),
            ("@", "@"),
            ("\"Total: \"0.0", "\"Total: \"0,0"),
        ];
        for (en, de) in cases {
            assert_eq!(code_to_local(en, DE), de, "{en}");
        }
        for (de, en) in [
            ("Standard", "General"),
            ("standard", "General"),
            ("TT.MM.JJJJ", "dd.mm.yyyy"),
            ("T. MMMM JJJJ", "d. mmmm yyyy"),
            ("#.##0,00 €", "#,##0.00 €"),
            ("#.##0;[Rot]-#.##0", "#,##0;[Red]-#,##0"),
            ("hh:mm:ss,000", "hh:mm:ss.000"),
            ("0,00E+00", "0.00E+00"),
            ("[Farbe3]0", "[Color3]0"),
        ] {
            assert_eq!(code_from_local(de, DE), en, "{de}");
        }
    }

    #[test]
    fn translated_codes_show_the_same() {
        let f = |code: &str, v: f64| format_value_in(&Value::Number(v), &NumberFormat::parse(code), DateSystem::D1900, DE).text;
        for code in [
            "General",
            "0.00",
            "#,##0.00",
            "#,##0;[Red]-#,##0",
            "0.00%",
            "0.00E+00",
            "#,##0.00 \"€\"",
            "m/d/yyyy",
            "d-mmm-yy",
            "mmmm d, yyyy",
            "h:mm:ss",
            "[h]:mm:ss",
            "dd.mm.yyyy hh:mm:ss.000",
            "[$-F800]dddd, mmmm dd, yyyy",
        ] {
            let back = code_from_local(&code_to_local(code, DE), DE);
            for v in [0.0, 1234.5678, -3.25, 46305.4375] {
                assert_eq!(f(&back, v), f(code, v), "{code} → {back} for {v}");
            }
        }
    }

    #[test]
    fn english_is_unchanged_and_junk_is_safe() {
        for c in ["#,##0.00", "m/d/yyyy", "General"] {
            assert_eq!(code_to_local(c, Locale::EnUs), c);
            assert_eq!(code_from_local(c, Locale::EnUs), c);
        }
        for junk in ["", "[", "\"", "\\", "_", "*", "[[", ";;", "[Farbe]", "[Color]", "ÄÖÜ", "AM/", "\u{0130}"] {
            let _ = code_to_local(junk, DE);
            let _ = code_from_local(junk, DE);
        }
    }
}
