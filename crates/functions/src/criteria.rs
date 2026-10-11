//! Excel criteria (COUNTIF, SUMIFS, DCOUNT…) and wildcard matching.

use std::cmp::Ordering;

use gridcraft_core::parse::{parse_error_in, parse_number_text_in};
use gridcraft_core::{CellError, DateSystem, Value, compare_numbers, compare_text};
use gridcraft_locale::Locale;

use crate::Ctx;

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
    /// Text (lower-cased chars kept for wildcard matching).
    Text(String),
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
    /// How numeric text in the cells under test is read: the same as in the criterion.
    locale: Locale,
    sys: DateSystem,
}

impl Criterion {
    /// Parses a criterion value the way COUNTIF does: numbers, `TRUE`/`FALSE` and error literals
    /// are read in the workbook's language and region (`"<1,5"` in pt-BR).
    pub(crate) fn parse(ctx: &dyn Ctx, v: &Value) -> Criterion {
        Criterion::parse_in(*ctx.locale(), ctx.date_system(), v)
    }

    fn parse_in(locale: Locale, sys: DateSystem, v: &Value) -> Criterion {
        let plain = |operand| Criterion { op: Op::Eq, operand, bare_empty: false, locale, sys };
        match v {
            Value::Number(n) => plain(Operand::Number(*n)),
            Value::Bool(b) => plain(Operand::Bool(*b)),
            Value::Error(e) => plain(Operand::Error(*e)),
            Value::Empty => plain(Operand::Number(0.0)),
            Value::Array(a) => Criterion::parse_in(locale, sys, a.data.first().unwrap_or(&Value::Empty)),
            Value::Text(t) => Criterion::parse_text(t, locale, sys),
        }
    }

    fn parse_text(t: &str, locale: Locale, sys: DateSystem) -> Criterion {
        let (op, rest) = if let Some(r) = t.strip_prefix(">=") {
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
            if t.is_empty() {
                return Criterion { op: Op::Eq, operand: Operand::Blank, bare_empty: true, locale, sys };
            }
            (Op::Eq, t)
        };
        let operand = if rest.is_empty() {
            Operand::Blank
        } else if let Some(n) = parse_number_text_in(rest, sys, &locale.regional) {
            Operand::Number(n)
        } else if let Some(b) = locale.formula.parse_bool(rest) {
            Operand::Bool(b)
        } else if let Some(e) = parse_error_in(rest, locale.formula) {
            Operand::Error(e)
        } else {
            Operand::Text(rest.to_string())
        };
        Criterion { op, operand, bare_empty: false, locale, sys }
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
                    // "<" / ">" with nothing: Excel compares against "" for text cells.
                    _ => match &v {
                        Value::Text(t) => cmp_op(self.op, compare_text(t, "")),
                        _ => false,
                    },
                }
            }
            Operand::Number(n) => {
                let cell = match &v {
                    Value::Number(x) => Some(*x),
                    // Numeric text matches only for equality (COUNTIF("1") counts text "1").
                    Value::Text(t) if matches!(self.op, Op::Eq | Op::Ne) => parse_number_text_in(t, self.sys, &self.locale.regional),
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
            Operand::Text(pat) => match self.op {
                Op::Eq | Op::Ne => {
                    let cell_text = match &v {
                        Value::Text(t) => Some(t.to_string()),
                        Value::Empty => Some(String::new()),
                        _ => None,
                    };
                    let m = cell_text.is_some_and(|t| wildcard_match(pat, &t));
                    if self.op == Op::Eq { m } else { !m }
                }
                _ => match &v {
                    Value::Text(t) => cmp_op(self.op, compare_text(t, pat)),
                    _ => false,
                },
            },
        }
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
            '~' => match it.next() {
                Some(n) => out.extend(n.to_lowercase().map(Tok::Char)),
                None => out.push(Tok::Char('~')),
            },
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::Any),
            c => out.extend(c.to_lowercase().map(Tok::Char)),
        }
    }
    out
}

fn lower_chars(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
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
    use crate::util::testutil::{TestCtx, pt_br};

    fn m(c: &str, v: Value) -> bool {
        Criterion::parse(&TestCtx::default(), &Value::from(c)).matches(&v)
    }

    fn m_pt(c: &str, v: Value) -> bool {
        Criterion::parse(&TestCtx { locale: pt_br(), ..TestCtx::default() }, &Value::from(c)).matches(&v)
    }

    #[test]
    fn local_numbers_and_literals() {
        assert!(m_pt("<1,5", Value::Number(1.0)));
        assert!(!m_pt("<1,5", Value::Number(2.0)));
        assert!(m_pt("1,5", Value::from("1,5")));
        assert!(m_pt(">=1.234,5", Value::Number(2000.0)));
        assert!(m_pt("VERDADEIRO", Value::Bool(true)));
        assert!(!m_pt("TRUE", Value::Bool(true)));
        assert!(m_pt("#N/D", Value::Error(CellError::NA)));
        // The English spelling keeps meaning text in pt-BR.
        assert!(m_pt("TRUE", Value::from("true")));
        assert!(!m("<1,5", Value::Number(1.0)));
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
        assert!(m("<b", Value::from("apple")));
        assert!(!m("<b", Value::Number(1.0)));
        assert!(m("#N/A", Value::Error(CellError::NA)));
        assert!(Criterion::parse(&TestCtx::default(), &Value::Number(3.0)).matches(&Value::Number(3.0)));
        assert!(Criterion::parse(&TestCtx::default(), &Value::Number(0.3)).matches(&Value::Number(0.1 + 0.2)));
        assert!(lookup_equal(&Value::Number(0.3), &Value::Number(0.1 + 0.2), false));
        assert!(m("<>a*", Value::from("bcd")));
        assert!(m(">1/1/2020", Value::Number(44000.0)));
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
        assert!(has_wildcards("a*"));
        assert!(!has_wildcards("abc"));
    }
}
