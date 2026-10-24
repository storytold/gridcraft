//! Excel criteria (COUNTIF, SUMIFS, DCOUNT…) and wildcard matching.

use std::cmp::Ordering;

use gridcraft_core::parse::parse_number_text;
use gridcraft_core::{CellError, Value, compare_numbers, compare_text};

/// Longest text criterion COUNTIF and its relatives accept, in UTF-16 code units as Excel
/// counts them (😀 is two units); a longer one is `#VALUE!`.
const MAX_CRITERION_UTF16_UNITS: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
enum Operand {
    Number(f64),
    Bool(bool),
    Error(CellError),
    /// Text without `*` or `?`: `=` and `<>` compare it as plain text, tildes included.
    Text(String),
    /// Text holding `*` or `?`: `=` and `<>` match it as a wildcard pattern, where `~` escapes
    /// the next character.
    Pattern(String),
    /// A criterion cut to its first 255 UTF-16 units in the middle of a surrogate pair: the
    /// complete characters before the cut, followed (in Excel) by a lone high surrogate.
    ///
    /// No cell text holds a lone surrogate, so `=` matches nothing and `<>` everything, as
    /// Excel gives 0 for SUMIFS with a criterion of 128 emoji. For `<`, `<=`, `>` and `>=` the
    /// criterion sorts just above the kept text: a cell matches `>`/`>=` when it sorts above
    /// that text, and `<`/`<=` otherwise. This is an approximation (Excel wasn't checked) and
    /// only matters for comparison criteria over 255 units.
    CutText(String),
    /// Nothing after the operator (`"="`, `"<>"`) or an empty criterion.
    Blank,
}

/// A parsed criterion.
#[derive(Clone, Debug)]
pub(crate) struct Criterion {
    op: Op,
    operand: Operand,
    /// The criterion was a bare `""` (or empty cell text): matches blanks and empty strings.
    bare_empty: bool,
}

impl Criterion {
    /// Parses a criterion value the way COUNTIF does.
    pub(crate) fn parse(v: &Value) -> Criterion {
        match v {
            Value::Number(n) => Criterion { op: Op::Eq, operand: Operand::Number(*n), bare_empty: false },
            Value::Bool(b) => Criterion { op: Op::Eq, operand: Operand::Bool(*b), bare_empty: false },
            Value::Error(e) => Criterion { op: Op::Eq, operand: Operand::Error(*e), bare_empty: false },
            Value::Empty => Criterion { op: Op::Eq, operand: Operand::Number(0.0), bare_empty: false },
            Value::Array(a) => Criterion::parse(a.data.first().unwrap_or(&Value::Empty)),
            Value::Text(t) => Criterion::parse_text(t),
        }
    }

    /// Parses the criteria argument of COUNTIF, SUMIF, AVERAGEIF and the single-pair *IFS
    /// functions: like `parse`, but text longer than 255 UTF-16 code units is `#VALUE!`, as in
    /// Excel (checked on Excel 2016 and Microsoft 365: `"a"×254 & "😀"` is 255 characters but
    /// 256 units, and is `#VALUE!`).
    pub(crate) fn parse_arg(v: &Value) -> Result<Criterion, CellError> {
        match v {
            Value::Array(a) => Criterion::parse_arg(a.data.first().unwrap_or(&Value::Empty)),
            // A UTF-8 string has at least as many bytes as UTF-16 units, so short text skips the count.
            Value::Text(t) if t.len() > MAX_CRITERION_UTF16_UNITS && t.encode_utf16().nth(MAX_CRITERION_UTF16_UNITS).is_some() => {
                Err(CellError::Value)
            }
            other => Ok(Criterion::parse(other)),
        }
    }

    /// Parses a criteria argument of an *IFS function with two or more criteria pairs. Excel
    /// doesn't reject a longer text criterion there: it matches its first 255 UTF-16 code units
    /// (checked on Excel 2016 and Microsoft 365 with COUNTIFS, SUMIFS, AVERAGEIFS, MAXIFS and
    /// MINIFS). A cut through a surrogate pair matches nothing; see `Operand::CutText`.
    pub(crate) fn parse_arg_truncated(v: &Value) -> Criterion {
        match v {
            Value::Array(a) => Criterion::parse_arg_truncated(a.data.first().unwrap_or(&Value::Empty)),
            Value::Text(t) if t.len() > MAX_CRITERION_UTF16_UNITS => {
                let mut units = 0;
                for (i, c) in t.char_indices() {
                    if units + c.len_utf16() > MAX_CRITERION_UTF16_UNITS {
                        if units == MAX_CRITERION_UTF16_UNITS {
                            return Criterion::parse_text(&t[..i]);
                        }
                        // The cut falls between the two halves of `c`.
                        let (op, rest) = split_op(&t[..i]);
                        return Criterion { op, operand: Operand::CutText(rest.to_string()), bare_empty: false };
                    }
                    units += c.len_utf16();
                }
                Criterion::parse_text(t)
            }
            other => Criterion::parse(other),
        }
    }

    fn parse_text(t: &str) -> Criterion {
        if t.is_empty() {
            return Criterion { op: Op::Eq, operand: Operand::Blank, bare_empty: true };
        }
        let (op, rest) = split_op(t);
        let operand = if rest.is_empty() {
            Operand::Blank
        } else if let Some(n) = parse_number_text(rest) {
            Operand::Number(n)
        } else if rest.eq_ignore_ascii_case("TRUE") {
            Operand::Bool(true)
        } else if rest.eq_ignore_ascii_case("FALSE") {
            Operand::Bool(false)
        } else if let Some(e) = CellError::parse(rest) {
            Operand::Error(e)
        } else if is_wildcard_criterion(rest) {
            Operand::Pattern(rest.to_string())
        } else {
            Operand::Text(rest.to_string())
        };
        Criterion { op, operand, bare_empty: false }
    }

    /// Whether a cell value satisfies the criterion.
    pub(crate) fn matches(&self, v: &Value) -> bool {
        let v = match v {
            Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Empty),
            other => other.clone(),
        };
        if self.bare_empty {
            return matches!(&v, Value::Empty) || matches!(&v, Value::Text(t) if t.is_empty());
        }
        match &self.operand {
            Operand::Blank => {
                let blank = matches!(v, Value::Empty);
                match self.op {
                    Op::Eq => blank,
                    Op::Ne => !blank,
                    // "<" / ">" with nothing: Excel compares against "" for text cells, but an
                    // empty string never matches ("<=" and ">=" count it in neither).
                    _ => match &v {
                        Value::Text(t) if !t.is_empty() => cmp_op(self.op, compare_text(t, "")),
                        _ => false,
                    },
                }
            }
            Operand::Number(n) => {
                let cell = match &v {
                    Value::Number(x) => Some(*x),
                    // Numeric text matches only for equality (COUNTIF("1") counts text "1").
                    Value::Text(t) if matches!(self.op, Op::Eq | Op::Ne) => parse_number_text(t),
                    _ => None,
                };
                match cell {
                    Some(x) => cmp_op(self.op, compare_numbers(x, *n)),
                    None => self.op == Op::Ne,
                }
            }
            Operand::Bool(b) => match &v {
                Value::Bool(x) => cmp_op(self.op, x.cmp(b)),
                _ => self.op == Op::Ne,
            },
            Operand::Error(e) => match &v {
                Value::Error(x) => cmp_op(self.op, x.code().cmp(&e.code())),
                _ => self.op == Op::Ne,
            },
            Operand::Text(pat) | Operand::Pattern(pat) => match self.op {
                Op::Eq | Op::Ne => {
                    // Blank cells hold no text: `"*"` skips them and `"<>*"` counts them.
                    let m = match &v {
                        Value::Text(t) if matches!(self.operand, Operand::Pattern(_)) => wildcard_match(pat, t),
                        Value::Text(t) => compare_text(t, pat) == Ordering::Equal,
                        _ => false,
                    };
                    if self.op == Op::Eq { m } else { !m }
                }
                _ => match &v {
                    Value::Text(t) => cmp_op(self.op, compare_text(t, pat)),
                    _ => false,
                },
            },
            Operand::CutText(kept) => match self.op {
                Op::Eq => false,
                Op::Ne => true,
                Op::Lt | Op::Le => matches!(&v, Value::Text(t) if compare_text(t, kept) != Ordering::Greater),
                Op::Gt | Op::Ge => matches!(&v, Value::Text(t) if compare_text(t, kept) == Ordering::Greater),
            },
        }
    }
}

/// Splits a leading comparison operator (`=` when there is none) off a text criterion.
fn split_op(t: &str) -> (Op, &str) {
    if let Some(r) = t.strip_prefix(">=") {
        (Op::Ge, r)
    } else if let Some(r) = t.strip_prefix("<=") {
        (Op::Le, r)
    } else if let Some(r) = t.strip_prefix("<>") {
        (Op::Ne, r)
    } else if let Some(r) = t.strip_prefix('=') {
        (Op::Eq, r)
    } else if let Some(r) = t.strip_prefix('>') {
        (Op::Gt, r)
    } else if let Some(r) = t.strip_prefix('<') {
        (Op::Lt, r)
    } else {
        (Op::Eq, t)
    }
}

fn cmp_op(op: Op, o: Ordering) -> bool {
    match op {
        Op::Eq => o == Ordering::Equal,
        Op::Ne => o != Ordering::Equal,
        Op::Lt => o == Ordering::Less,
        Op::Le => o != Ordering::Greater,
        Op::Gt => o == Ordering::Greater,
        Op::Ge => o != Ordering::Less,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Char(char),
    Any,
    Star,
}

fn tokenize(pattern: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = pattern.chars();
    while let Some(c) = it.next() {
        match c {
            // `~` takes the next character literally; a trailing `~` is dropped, as in Excel.
            '~' => match it.next() {
                Some(n) => out.push(Tok::Char(lower_char(n))),
                // A pattern that is only `~` would be empty and match empty text. Excel wasn't
                // measured for it, so it stays a literal `~`, as before #1455.
                None if out.is_empty() => out.push(Tok::Char('~')),
                None => {}
            },
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::Any),
            c => out.extend(std::iter::once(lower_char(c)).map(Tok::Char)),
        }
    }
    out
}

/// Lowercase one char to exactly one char, so positions in the lowered text match the original
/// (U+0130 `İ` folds to `i`, like `fold` in text.rs).
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn lower_chars(s: &str) -> Vec<char> {
    s.chars().map(lower_char).collect()
}

/// Whether a criterion's text is a wildcard pattern: in COUNTIF and its relatives only text
/// holding `*` or `?` is (Excel 365, #1455); any other text, tildes included, compares as it is.
fn is_wildcard_criterion(t: &str) -> bool {
    t.contains('*') || t.contains('?')
}

/// Whether the pattern contains an unescaped wildcard.
pub(crate) fn has_wildcards(pattern: &str) -> bool {
    tokenize(pattern).iter().any(|t| !matches!(t, Tok::Char(_))) || pattern.contains('~')
}

/// Case-insensitive whole-text match with `*`, `?` and `~` escapes.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p = tokenize(pattern);
    let t = lower_chars(text);
    match_from(&p, &t, true).is_some()
}

/// Greedy-free matcher: returns the matched length of `t` when `p` matches a prefix of `t`
/// (or all of `t` when `whole`). Iterative with backtracking on the last star (linear-ish).
fn match_from(p: &[Tok], t: &[char], whole: bool) -> Option<usize> {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi == p.len() && (!whole || ti == t.len()) {
            return Some(ti);
        }
        let step = match (p.get(pi), t.get(ti)) {
            (Some(Tok::Star), _) => {
                star = Some((pi, ti));
                pi += 1;
                continue;
            }
            (Some(Tok::Any), Some(_)) => true,
            (Some(Tok::Char(c)), Some(x)) => c == x,
            _ => false,
        };
        if step {
            pi += 1;
            ti += 1;
            continue;
        }
        match star {
            Some((sp, st)) if st < t.len() => {
                star = Some((sp, st + 1));
                pi = sp + 1;
                ti = st + 1;
            }
            _ => return None,
        }
    }
}

/// SEARCH-style find: the char index (0-based) of the first position at or after `start` where
/// the wildcard pattern matches, case-insensitively.
pub(crate) fn wildcard_find(pattern: &str, text: &str, start: usize) -> Option<usize> {
    let p = tokenize(pattern);
    let t = lower_chars(text);
    if p.is_empty() {
        return if start <= t.len() { Some(start) } else { None };
    }
    (start..t.len()).find(|&i| t.get(i..).is_some_and(|rest| match_from(&p, rest, false).is_some()))
}

/// Exact-or-wildcard equality used by MATCH/VLOOKUP/XLOOKUP exact modes: text with wildcards
/// matches by pattern (only when `wild`), otherwise values compare case-insensitively.
pub(crate) fn lookup_equal(needle: &Value, hay: &Value, wild: bool) -> bool {
    match (needle, hay) {
        (Value::Text(p), Value::Text(t)) => {
            if wild {
                wildcard_match(p, t)
            } else {
                compare_text(p, t) == Ordering::Equal
            }
        }
        (Value::Number(a), Value::Number(b)) => compare_numbers(*a, *b) == Ordering::Equal,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Error(a), Value::Error(b)) => a == b,
        (Value::Empty, Value::Empty) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(c: &str, v: Value) -> bool {
        Criterion::parse(&Value::from(c)).matches(&v)
    }

    #[test]
    fn criteria() {
        assert!(m(">5", Value::Number(6.0)));
        assert!(!m(">5", Value::Number(5.0)));
        assert!(m(">=5", Value::Number(5.0)));
        assert!(m("<>5", Value::from("x")));
        assert!(m("<>5", Value::Empty));
        assert!(!m(">5", Value::from("9")));
        assert!(m("5", Value::from("5")));
        assert!(m("a*", Value::from("Apple")));
        assert!(!m("a*", Value::from("banana")));
        assert!(m("?pple", Value::from("apple")));
        assert!(m("~*", Value::from("*")));
        assert!(!m("~*", Value::from("x")));
        assert!(m("", Value::Empty));
        assert!(m("", Value::from("")));
        assert!(!m("", Value::Number(0.0)));
        assert!(m("=", Value::Empty));
        assert!(!m("=", Value::from("a")));
        assert!(m("<>", Value::from("a")));
        assert!(!m("<>", Value::Empty));
        assert!(m("TRUE", Value::Bool(true)));
        // A bare "<=" or ">=" never matches an empty string (Excel 365: COUNTIF(="", "<=") is 0).
        assert!(!m("<=", Value::from("")));
        assert!(!m(">=", Value::from("")));
        assert!(m("<b", Value::from("apple")));
        assert!(!m("<b", Value::Number(1.0)));
        assert!(m("#N/A", Value::Error(CellError::NA)));
        assert!(Criterion::parse(&Value::Number(3.0)).matches(&Value::Number(3.0)));
        assert!(Criterion::parse(&Value::Number(0.3)).matches(&Value::Number(0.1 + 0.2)));
        assert!(lookup_equal(&Value::Number(0.3), &Value::Number(0.1 + 0.2), false));
        assert!(m("<>a*", Value::from("bcd")));
        assert!(m(">1/1/2020", Value::Number(44000.0)));
        // Text patterns never match blank cells, but an empty string is text.
        assert!(!m("*", Value::Empty));
        assert!(m("*", Value::from("")));
        assert!(m("<>*", Value::Empty));
        assert!(!m("<>*", Value::from("")));
        assert!(!m("?*", Value::Empty));
        assert!(m("<>a*", Value::Empty));
        // Without `*` or `?` a criterion is plain text, tildes included (#1455, Excel 365).
        assert!(m("a~b", Value::from("a~b")));
        assert!(!m("a~b", Value::from("ab")));
        assert!(!m("~~", Value::from("~")));
        assert!(m("~~", Value::from("~~")));
        assert!(!m("h~ello", Value::from("hello")));
        assert!(m("<>a~b", Value::from("ab")));
        // In a pattern a trailing `~` is dropped.
        assert!(m("?~", Value::from("y")));
        assert!(!m("?~", Value::from("x~")));
        // Plain text compares with `compare_text`, so `İ` no longer folds to `i` here
        // (Excel 365: COUNTIF of "i" over `İ`, `i`, `I` is 2).
        assert!(!m("i", Value::from("\u{130}")));
        assert!(m("i", Value::from("I")));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("a*c", "abbbc"));
        assert!(!wildcard_match("a*c", "abbbd"));
        assert!(wildcard_match("*~?", "what?"));
        assert_eq!(wildcard_find("b?d", "abcd abd", 0), Some(1));
        assert_eq!(wildcard_find("b?d", "abxxd bcd", 0), Some(6));
        assert_eq!(wildcard_find("c*", "abcd", 0), Some(2));
        assert_eq!(wildcard_find("z", "abcd", 0), None);
        assert_eq!(wildcard_find("b", "a\u{130}bc", 0), Some(2));
        assert!(wildcard_match("i*", "\u{130}stanbul"));
        assert!(has_wildcards("a*"));
        assert!(!has_wildcards("abc"));
        // A trailing `~` is dropped (#1455).
        assert!(wildcard_match("x~", "x"));
        assert!(!wildcard_match("x~", "x~"));
        assert!(wildcard_match("*~~", "xyz~"));
        assert_eq!(wildcard_find("x~", "axb", 0), Some(1));
        // A pattern that is only `~` stays a literal `~` (not measured in Excel).
        assert!(wildcard_match("~", "~"));
        assert!(!wildcard_match("~", ""));
        assert_eq!(wildcard_find("~", "abc", 0), None);
        assert_eq!(wildcard_find("~", "a~b", 0), Some(1));
    }
}
