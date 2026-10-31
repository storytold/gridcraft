//! The M lexer: identifiers (dotted names like `Table.AddColumn` are one identifier), quoted
//! identifiers (`#"Changed Type"`), text with `#(…)` escapes, numbers, keywords, `#date`-style
//! hash keywords, operators and comments, as the Power Query M language specification describes
//! its lexical grammar.

use super::value::MError;

/// Most tokens one formula is split into.
const MAX_TOKENS: usize = 2_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// A regular identifier (`Source`, `Table.AddColumn`, `_`).
    Ident(String),
    /// `#"…"`: a quoted identifier.
    Quoted(String),
    Text(String),
    Number(f64),
    /// A keyword (`let`, `each`, `true`…).
    Kw(&'static str),
    /// A hash keyword without its `#` (`date`, `table`, `infinity`…).
    Hash(String),
    /// An operator or punctuator.
    P(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    /// Byte offsets in the formula.
    pub start: usize,
    pub end: usize,
}

const KEYWORDS: &[&str] = &[
    "and",
    "as",
    "each",
    "else",
    "error",
    "false",
    "if",
    "in",
    "is",
    "let",
    "meta",
    "not",
    "null",
    "or",
    "otherwise",
    "section",
    "shared",
    "then",
    "true",
    "try",
    "type",
    "catch",
];

const PUNCT: &[&str] =
    &["...", "=>", "<=", ">=", "<>", "??", "..", "=", "<", ">", "+", "-", "*", "/", "&", "(", ")", "[", "]", "{", "}", ",", ";", "?", "@", "!"];

fn ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn ident_part(c: char) -> bool {
    c == '_' || c.is_alphanumeric() || matches!(c, '\u{0300}'..='\u{036f}' | '\u{200c}' | '\u{200d}')
}

fn err(msg: impl Into<String>) -> MError {
    MError::new("Expression.SyntaxError", msg)
}

/// Splits a formula into tokens (ending with [`Tok::Eof`]).
pub fn lex(src: &str) -> Result<Vec<Token>, MError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let pos = |i: usize| chars.get(i).map(|(p, _)| *p).unwrap_or(src.len());
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    while let Some(c) = at(i) {
        if out.len() >= MAX_TOKENS {
            return Err(err("The formula is too long."));
        }
        let start = pos(i);
        // Whitespace and comments.
        if c.is_whitespace() || c == '\u{feff}' {
            i += 1;
            continue;
        }
        if c == '/' && at(i + 1) == Some('/') {
            while at(i).is_some_and(|c| c != '\n' && c != '\r' && c != '\u{2028}' && c != '\u{2029}') {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == Some('*') {
            i += 2;
            loop {
                match at(i) {
                    None => return Err(err("A comment isn't closed (*/ missing).")),
                    Some('*') if at(i + 1) == Some('/') => {
                        i += 2;
                        break;
                    }
                    Some(_) => i += 1,
                }
            }
            continue;
        }
        // Text.
        if c == '"' {
            let (text, next) = text_literal(&chars, i + 1)?;
            i = next;
            out.push(Token { tok: Tok::Text(text), start, end: pos(i) });
            continue;
        }
        // Hash forms: quoted identifiers, hash keywords.
        if c == '#' {
            if at(i + 1) == Some('"') {
                let (text, next) = text_literal(&chars, i + 2)?;
                i = next;
                out.push(Token { tok: Tok::Quoted(text), start, end: pos(i) });
                continue;
            }
            let mut j = i + 1;
            let mut name = String::new();
            while let Some(c) = at(j).filter(|c| c.is_ascii_alphabetic()) {
                name.push(c);
                j += 1;
            }
            if name.is_empty() {
                return Err(err("A '#' must start a quoted identifier (#\"…\") or a keyword like #date."));
            }
            i = j;
            out.push(Token { tok: Tok::Hash(name), start, end: pos(i) });
            continue;
        }
        // Numbers.
        if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let (n, next) = number_literal(&chars, i)?;
            i = next;
            out.push(Token { tok: Tok::Number(n), start, end: pos(i) });
            continue;
        }
        // Identifiers and keywords.
        if ident_start(c) {
            let mut j = i;
            let mut name = String::new();
            loop {
                while let Some(c) = at(j).filter(|c| ident_part(*c)) {
                    name.push(c);
                    j += 1;
                }
                // A dot continues the identifier when another part follows (`Table.AddColumn`).
                if at(j) == Some('.') && at(j + 1).is_some_and(ident_start) {
                    name.push('.');
                    j += 1;
                    continue;
                }
                break;
            }
            i = j;
            let tok = match KEYWORDS.iter().find(|k| **k == name) {
                Some(k) => Tok::Kw(k),
                None => Tok::Ident(name),
            };
            out.push(Token { tok, start, end: pos(i) });
            continue;
        }
        // Operators.
        let rest = src.get(start..).unwrap_or("");
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                i += p.chars().count();
                out.push(Token { tok: Tok::P(p), start, end: pos(i) });
            }
            None => return Err(err(format!("Invalid character '{c}'."))),
        }
    }
    out.push(Token { tok: Tok::Eof, start: src.len(), end: src.len() });
    Ok(out)
}

/// A text literal's value from just after its opening quote; returns the index after the
/// closing quote.
fn text_literal(chars: &[(usize, char)], mut i: usize) -> Result<(String, usize), MError> {
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let mut s = String::new();
    loop {
        match at(i) {
            None => return Err(err("A text value isn't closed (\" missing).")),
            Some('"') if at(i + 1) == Some('"') => {
                s.push('"');
                i += 2;
            }
            Some('"') => return Ok((s, i + 1)),
            Some('#') if at(i + 1) == Some('(') => {
                let mut j = i + 2;
                let mut body = String::new();
                while let Some(c) = at(j).filter(|c| *c != ')') {
                    body.push(c);
                    j += 1;
                    if body.len() > 256 {
                        return Err(err("An escape sequence is too long."));
                    }
                }
                if at(j) != Some(')') {
                    return Err(err("An escape sequence isn't closed (')' missing)."));
                }
                for part in body.split(',') {
                    match part.trim() {
                        "cr" => s.push('\r'),
                        "lf" => s.push('\n'),
                        "tab" => s.push('\t'),
                        "#" => s.push('#'),
                        hex if (hex.len() == 4 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                            let ch = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                            s.push(ch.ok_or_else(|| err(format!("Invalid character escape #({hex}).")))?);
                        }
                        other => return Err(err(format!("Invalid escape sequence #({other})."))),
                    }
                }
                i = j + 1;
            }
            Some(c) => {
                s.push(c);
                i += 1;
            }
        }
    }
}

fn number_literal(chars: &[(usize, char)], mut i: usize) -> Result<(f64, usize), MError> {
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    if at(i) == Some('0') && matches!(at(i + 1), Some('x' | 'X')) {
        let mut j = i + 2;
        let mut v: f64 = 0.0;
        let mut any = false;
        while let Some(d) = at(j).and_then(|c| c.to_digit(16)) {
            v = v * 16.0 + f64::from(d);
            any = true;
            j += 1;
        }
        if !any {
            return Err(err("A hexadecimal number needs digits after 0x."));
        }
        return Ok((v, j));
    }
    let mut s = String::new();
    while let Some(c) = at(i).filter(char::is_ascii_digit) {
        s.push(c);
        i += 1;
    }
    if at(i) == Some('.') && at(i + 1).is_some_and(|c| c.is_ascii_digit()) {
        s.push('.');
        i += 1;
        while let Some(c) = at(i).filter(char::is_ascii_digit) {
            s.push(c);
            i += 1;
        }
    }
    if matches!(at(i), Some('e' | 'E')) {
        let sign = at(i + 1).filter(|c| *c == '+' || *c == '-');
        let first = if sign.is_some() { i + 2 } else { i + 1 };
        if at(first).is_some_and(|c| c.is_ascii_digit()) {
            s.push('e');
            if let Some(sg) = sign {
                s.push(sg);
            }
            i = first;
            while let Some(c) = at(i).filter(char::is_ascii_digit) {
                s.push(c);
                i += 1;
            }
        }
    }
    if s.len() > 400 {
        return Err(err("A number is too long."));
    }
    let n: f64 = s.parse().map_err(|_| err(format!("Invalid number '{s}'.")))?;
    Ok((n, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn identifiers_text_numbers_and_operators() {
        assert_eq!(
            toks(r##"Table.AddColumn(#"Changed Type", "A ""q""#(cr,lf)", each [x] >= 1.5e2) // c"##),
            vec![
                Tok::Ident("Table.AddColumn".into()),
                Tok::P("("),
                Tok::Quoted("Changed Type".into()),
                Tok::P(","),
                Tok::Text("A \"q\"\r\n".into()),
                Tok::P(","),
                Tok::Kw("each"),
                Tok::P("["),
                Tok::Ident("x".into()),
                Tok::P("]"),
                Tok::P(">="),
                Tok::Number(150.0),
                Tok::P(")"),
                Tok::Eof,
            ]
        );
        assert_eq!(toks("{1..0x1F}"), vec![Tok::P("{"), Tok::Number(1.0), Tok::P(".."), Tok::Number(31.0), Tok::P("}"), Tok::Eof]);
        assert_eq!(toks("#date /* x */ 1")[..2], [Tok::Hash("date".into()), Tok::Number(1.0)]);
        assert_eq!(toks("\"#(0041)#(#)(\"")[0], Tok::Text("A#(".into()));
        assert_eq!(toks("Column1.1")[..2], [Tok::Ident("Column1".into()), Tok::Number(0.1)]);
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        for s in ["\"open", "#\"open", "/* open", "\"#(zz)\"", "\"#(00", "0x", "#", "$", "1e", "\"#(110000)\""] {
            let _ = lex(s);
        }
        assert!(lex("\"open").is_err());
        assert!(lex("$").is_err());
    }
}
