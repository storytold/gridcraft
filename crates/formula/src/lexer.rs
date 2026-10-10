//! Tokenizer for Excel formulas (A1 style, canonical or selected UI separators).
//!
//! Token spans are byte offsets into the source, which the editor uses to colour references.

use gridcraft_core::CellError;

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

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '\\' | '?')
}

/// Tokenizes formula text (without the leading `=`).
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    tokenize_locale(src, crate::FormulaLocale::En)
}

/// Tokenizes UI syntax while retaining byte spans in the original text.
pub fn tokenize_locale(src: &str, locale: crate::FormulaLocale) -> Result<Vec<Token>, LexError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let n = chars.len();
    let pos_of = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(src.len());
    let ch = |i: usize| chars.get(i).map(|c| c.1);
    let mut out = Vec::new();
    let mut i = 0;
    let mut array_depth = 0usize;
    while i < n {
        let start = pos_of(i);
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
                let mut found = None;
                for e in CellError::ALL {
                    let t = e.as_str();
                    if rest.len() >= t.len() && rest.get(..t.len()).is_some_and(|p| p.eq_ignore_ascii_case(t)) {
                        found = Some(e);
                        break;
                    }
                }
                let Some(e) = found else { return Err(LexError { msg: "unknown error literal".into(), pos: start }) };
                let len = e.as_str().chars().count();
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
            c if c.is_ascii_digit() || (c == locale.decimal_separator() && ch(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                // Number, unless it is a row range like 1:3 (handled by parser via Word) — numbers
                // followed by ':' and digits are row refs: lex as Word.
                let mut j = i;
                while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                    j += 1;
                }
                let is_row_ref = ch(j) == Some(':') && {
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
                if ch(j) == Some(locale.decimal_separator()) {
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
                let text = src.get(start..pos_of(j)).unwrap_or("");
                let v: f64 = text
                    .replace(locale.decimal_separator(), ".")
                    .parse()
                    .map_err(|_| LexError { msg: format!("bad number `{text}`"), pos: start })?;
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
                array_depth += 1;
                push(&mut out, Tok::LBrace, i + 1);
                i += 1;
            }
            '}' => {
                array_depth = array_depth.saturating_sub(1);
                push(&mut out, Tok::RBrace, i + 1);
                i += 1;
            }
            c if (array_depth == 0 && c == locale.list_separator()) || (array_depth > 0 && c == locale.array_column_separator()) => {
                push(&mut out, Tok::Comma, i + 1);
                i += 1;
            }
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
            c if is_word_char(c) => {
                let mut j = i;
                while ch(j).is_some_and(|c| is_word_char(c) && !(array_depth > 0 && c == locale.array_column_separator())) {
                    j += 1;
                }
                let w: String = chars.get(i..j).map(|s| s.iter().map(|c| c.1).collect()).unwrap_or_default();
                if ch(j) == Some('!') {
                    push(&mut out, Tok::Sheet(w, None), j + 1);
                    i = j + 1;
                } else if ch(j) == Some(':') && {
                    // Sheet1:Sheet3!A1 — look ahead for word then '!'
                    let mut k = j + 1;
                    while ch(k).is_some_and(|c| is_word_char(c) && !(array_depth > 0 && c == locale.array_column_separator())) {
                        k += 1;
                    }
                    k > j + 1 && ch(k) == Some('!')
                } {
                    let mut k = j + 1;
                    while ch(k).is_some_and(|c| is_word_char(c) && !(array_depth > 0 && c == locale.array_column_separator())) {
                        k += 1;
                    }
                    let w2: String = chars.get(j + 1..k).map(|s| s.iter().map(|c| c.1).collect()).unwrap_or_default();
                    push(&mut out, Tok::Sheet(w, Some(w2)), k + 1);
                    i = k + 1;
                } else if ch(j) == Some('(') {
                    push(&mut out, Tok::Func(w), j + 1);
                    i = j + 1;
                } else if w.eq_ignore_ascii_case("TRUE") && ch(j) != Some('[') {
                    push(&mut out, Tok::Bool(true), j);
                    i = j;
                } else if w.eq_ignore_ascii_case("FALSE") && ch(j) != Some('[') {
                    push(&mut out, Tok::Bool(false), j);
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
