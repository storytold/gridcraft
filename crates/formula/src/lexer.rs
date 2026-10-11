//! Tokenizer for Excel formulas.
//!
//! The text is read in a [`Dialect`]: numbers use the region's decimal separator, the region's
//! list separator lexes as [`Tok::Comma`] outside array constants, and inside `{}` the region's
//! column and row separators lex as [`Tok::Comma`] and [`Tok::Semicolon`]. Boolean and error
//! literals are the dialect language's. The parser therefore only sees abstract tokens.
//!
//! Token spans are byte offsets into the source, which the editor uses to colour references.

use gridcraft_core::CellError;
use gridcraft_locale::{Dialect, Regional};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(CellError),
    /// A reference-looking or name-looking word, possibly sheet-qualified: `A1`, `$B$2`, `A:A`
    /// is lexed as Word(A) Colon Word(A); `Sheet1!A1` → `Sheet(Some("Sheet1"))` then `Word`.
    Word(String),
    /// Sheet prefix `Sheet1!`, `'My Sheet'!`, `Sheet1:Sheet3!` (first, last).
    Sheet(String, Option<String>),
    /// Function name (word immediately followed by `(`), the `(` is consumed.
    Func(String),
    /// Structured reference body after a table name or bare: the raw text inside `[...]`.
    Struct(String),
    Op(&'static str),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semicolon,
    Colon,
    Space,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{msg} at {pos}")]
pub struct LexError {
    pub msg: String,
    pub pos: usize,
}

/// Characters that may be part of a word. Inside array constants the region's column and row
/// separators are not word characters (`.` in German, `\` in Portuguese).
fn is_word_char(c: char, in_array: bool, r: &Regional) -> bool {
    c.is_alphanumeric() || (matches!(c, '_' | '.' | '$' | '\\' | '?') && !(in_array && (c == r.array_col || c == r.array_row)))
}

/// Tokenizes canonical (en-US) formula text (without the leading `=`).
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    tokenize_local(src, &Dialect::INVARIANT)
}

/// Tokenizes formula text (without the leading `=`) written in `d`.
pub fn tokenize_local(src: &str, d: &Dialect) -> Result<Vec<Token>, LexError> {
    let r = d.regional;
    let mut braces = 0usize;
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let n = chars.len();
    let pos_of = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(src.len());
    let ch = |i: usize| chars.get(i).map(|c| c.1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let start = pos_of(i);
        let in_array = braces > 0;
        let word = |x: char| is_word_char(x, in_array, r);
        let Some(c) = ch(i) else { break };
        let push = |out: &mut Vec<Token>, tok: Tok, end_i: usize| out.push(Token { tok, start, end: pos_of(end_i) });
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                let mut j = i;
                while matches!(ch(j), Some(' ' | '\t' | '\n' | '\r')) {
                    j += 1;
                }
                push(&mut out, Tok::Space, j);
                i = j;
            }
            '"' => {
                let mut s = String::new();
                let mut j = i + 1;
                loop {
                    match ch(j) {
                        None => return Err(LexError { msg: "unterminated text".into(), pos: start }),
                        Some('"') if ch(j + 1) == Some('"') => {
                            s.push('"');
                            j += 2;
                        }
                        Some('"') => {
                            j += 1;
                            break;
                        }
                        Some(x) => {
                            s.push(x);
                            j += 1;
                        }
                    }
                }
                push(&mut out, Tok::Text(s), j);
                i = j;
            }
            '\'' => {
                // Quoted sheet name: 'My Sheet'!  or 'A:B'!
                let mut s = String::new();
                let mut j = i + 1;
                loop {
                    match ch(j) {
                        None => return Err(LexError { msg: "unterminated sheet name".into(), pos: start }),
                        Some('\'') if ch(j + 1) == Some('\'') => {
                            s.push('\'');
                            j += 2;
                        }
                        Some('\'') => {
                            j += 1;
                            break;
                        }
                        Some(x) => {
                            s.push(x);
                            j += 1;
                        }
                    }
                }
                if ch(j) != Some('!') {
                    return Err(LexError { msg: "expected ! after sheet name".into(), pos: pos_of(j) });
                }
                let (a, b) = match s.split_once(':') {
                    Some((a, b)) => (a.to_string(), Some(b.to_string())),
                    None => (s, None),
                };
                push(&mut out, Tok::Sheet(a, b), j + 1);
                i = j + 1;
            }
            '#' if out.last().is_some_and(|t: &Token| t.end == start && matches!(t.tok, Tok::Word(_))) => {
                // Spill range operator right after a reference: `A1#`.
                push(&mut out, Tok::Op("#"), i + 1);
                i += 1;
            }
            '#' => {
                // Error literal, or #-prefixed struct specifier (only inside [] which we lex as Struct).
                let rest = src.get(start..).unwrap_or("");
                let Some((canonical, bytes)) = d.names.error_prefix(rest) else {
                    return Err(LexError { msg: "unknown error literal".into(), pos: start });
                };
                let Some(e) = CellError::parse(canonical) else {
                    return Err(LexError { msg: "unknown error literal".into(), pos: start });
                };
                // Never advance by zero characters: that would loop forever.
                let Some(len) = rest.get(..bytes).map(|s| s.chars().count()).filter(|n| *n > 0) else {
                    return Err(LexError { msg: "unknown error literal".into(), pos: start });
                };
                // `#REF!A1` style is not valid; `#REF!` is an error literal.
                push(&mut out, Tok::Error(e), i + len);
                i += len;
            }
            '[' => {
                // Structured reference body: balanced brackets.
                let mut depth = 0;
                let mut j = i;
                loop {
                    match ch(j) {
                        None => return Err(LexError { msg: "unbalanced [".into(), pos: start }),
                        Some('\'') => j += 2, // escape char inside struct refs
                        Some('[') => {
                            depth += 1;
                            j += 1;
                        }
                        Some(']') => {
                            depth -= 1;
                            j += 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Some(_) => j += 1,
                    }
                }
                let inner = src.get(pos_of(i + 1)..pos_of(j.saturating_sub(1))).unwrap_or("").to_string();
                push(&mut out, Tok::Struct(inner), j);
                i = j;
            }
            c if c.is_ascii_digit() || (c == r.decimal && ch(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                // Number, unless it is a row range like 1:3 (handled by parser via Word) — numbers
                // followed by ':' and digits are row refs: lex as Word. Array constants hold no
                // references, and `:` separates their rows in some regions (ru-RU `{1;2:3;4}`).
                let mut j = i;
                while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                    j += 1;
                }
                let is_row_ref = !in_array && ch(j) == Some(':') && {
                    let mut k = j + 1;
                    if ch(k) == Some('$') {
                        k += 1;
                    }
                    ch(k).is_some_and(|d| d.is_ascii_digit())
                };
                // A word like "1A" can't happen in valid formulas; numbers then.
                if is_row_ref {
                    let w: String = chars.get(i..j).map(|s| s.iter().map(|c| c.1).collect()).unwrap_or_default();
                    push(&mut out, Tok::Word(w), j);
                    i = j;
                    continue;
                }
                if ch(j) == Some(r.decimal) {
                    j += 1;
                    while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                        j += 1;
                    }
                }
                if matches!(ch(j), Some('e' | 'E')) {
                    let mut k = j + 1;
                    if matches!(ch(k), Some('+' | '-')) {
                        k += 1;
                    }
                    if ch(k).is_some_and(|d| d.is_ascii_digit()) {
                        while ch(k).is_some_and(|d| d.is_ascii_digit()) {
                            k += 1;
                        }
                        j = k;
                    }
                }
                let text: String = src.get(start..pos_of(j)).unwrap_or("").chars().map(|c| if c == r.decimal { '.' } else { c }).collect();
                let v: f64 = text.parse().map_err(|_| LexError { msg: format!("bad number `{text}`"), pos: start })?;
                push(&mut out, Tok::Number(v), j);
                i = j;
            }
            '(' => {
                push(&mut out, Tok::LParen, i + 1);
                i += 1;
            }
            ')' => {
                push(&mut out, Tok::RParen, i + 1);
                i += 1;
            }
            '{' => {
                braces += 1;
                push(&mut out, Tok::LBrace, i + 1);
                i += 1;
            }
            '}' => {
                braces = braces.saturating_sub(1);
                push(&mut out, Tok::RBrace, i + 1);
                i += 1;
            }
            c if in_array && c == r.array_col => {
                push(&mut out, Tok::Comma, i + 1);
                i += 1;
            }
            c if in_array && c == r.array_row => {
                push(&mut out, Tok::Semicolon, i + 1);
                i += 1;
            }
            c if !in_array && c == r.list => {
                push(&mut out, Tok::Comma, i + 1);
                i += 1;
            }
            // A list or row separator that does not belong to this dialect: the parser rejects it.
            ';' => {
                push(&mut out, Tok::Semicolon, i + 1);
                i += 1;
            }
            ':' => {
                push(&mut out, Tok::Colon, i + 1);
                i += 1;
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '%' | '@' | '=' => {
                let op = match c {
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '^' => "^",
                    '&' => "&",
                    '%' => "%",
                    '@' => "@",
                    _ => "=",
                };
                push(&mut out, Tok::Op(op), i + 1);
                i += 1;
            }
            '<' | '>' => {
                let two = ch(i + 1);
                let (op, len) = match (c, two) {
                    ('<', Some('=')) => ("<=", 2),
                    ('<', Some('>')) => ("<>", 2),
                    ('>', Some('=')) => (">=", 2),
                    ('<', _) => ("<", 1),
                    _ => (">", 1),
                };
                push(&mut out, Tok::Op(op), i + len);
                i += len;
            }
            c if word(c) => {
                let mut j = i;
                while ch(j).is_some_and(word) {
                    j += 1;
                }
                let w: String = chars.get(i..j).map(|s| s.iter().map(|c| c.1).collect()).unwrap_or_default();
                if ch(j) == Some('!') {
                    push(&mut out, Tok::Sheet(w, None), j + 1);
                    i = j + 1;
                } else if ch(j) == Some(':') && {
                    // Sheet1:Sheet3!A1 — look ahead for word then '!'
                    let mut k = j + 1;
                    while ch(k).is_some_and(word) {
                        k += 1;
                    }
                    k > j + 1 && ch(k) == Some('!')
                } {
                    let mut k = j + 1;
                    while ch(k).is_some_and(word) {
                        k += 1;
                    }
                    let w2: String = chars.get(j + 1..k).map(|s| s.iter().map(|c| c.1).collect()).unwrap_or_default();
                    push(&mut out, Tok::Sheet(w, Some(w2)), k + 1);
                    i = k + 1;
                } else if ch(j) == Some('(') {
                    push(&mut out, Tok::Func(w), j + 1);
                    i = j + 1;
                } else if ch(j) != Some('[')
                    && let Some(b) = d.names.parse_bool(&w)
                {
                    push(&mut out, Tok::Bool(b), j);
                    i = j;
                } else {
                    push(&mut out, Tok::Word(w), j);
                    i = j;
                }
            }
            other => return Err(LexError { msg: format!("unexpected character `{other}`"), pos: start }),
        }
    }
    out.push(Token { tok: Tok::Eof, start: src.len(), end: src.len() });
    Ok(out)
}
