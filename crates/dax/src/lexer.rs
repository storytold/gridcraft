//! Tokenizer for DAX expressions.
//!
//! Spans are byte offsets into the source. Keywords are recognised case-insensitively; the
//! parser keeps the source spelling of identifiers and function names. `TRUE`/`FALSE` stay
//! identifiers here so the parser can tell the boolean literal from a call such as `TRUE()`.

use crate::ast::Span;

/// Longest input accepted, in bytes. Mirrors DAX's expression limit and bounds the size of the
/// tree a long operator chain can build.
pub const MAX_INPUT_LEN: usize = 8_192;

/// A lexical token.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Number(f64),
    String(String),
    /// A bare identifier, function name or table name (`Sales`, `STDEV.P`, `TRUE`).
    Ident(String),
    /// A single-quoted table name, with `''` escapes resolved.
    QuotedIdent(String),
    /// A bracketed column name, with `]]` escapes resolved.
    BracketIdent(String),
    /// The `NOT` operator (case-insensitive).
    Not,
    /// The `IN` operator (case-insensitive).
    In,
    /// The `EVALUATE` statement keyword (case-insensitive).
    Evaluate,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Eq,
    StrictEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    /// `=>`, reserved for user-defined functions (a later PR).
    Arrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Eof,
}

/// A token and its byte span.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message} at bytes {}..{}", span.start, span.end)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl LexError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        LexError { message: message.into(), span }
    }
}

/// Tokenizes `src` and appends an [`TokenKind::Eof`] token at `src.len()`.
///
/// Whitespace is insignificant and comments (`//`, `--` to end of line, and `/* ... */`) are
/// skipped. A block comment does not nest.
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    if src.len() > MAX_INPUT_LEN {
        return Err(LexError::new(format!("input is too long ({} bytes, limit {MAX_INPUT_LEN})", src.len()), Span::new(0, src.len())));
    }

    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let n = chars.len();
    let ch = |i: usize| chars.get(i).map(|(_, c)| *c);
    let pos = |i: usize| chars.get(i).map(|(p, _)| *p).unwrap_or(src.len());
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;

    while i < n {
        let start = pos(i);
        let Some(c) = ch(i) else { break };
        match c {
            c if c.is_whitespace() => {
                i += 1;
            }
            '/' if ch(i + 1) == Some('/') => {
                i += 2;
                while ch(i).is_some_and(|c| c != '\n' && c != '\r') {
                    i += 1;
                }
            }
            '-' if ch(i + 1) == Some('-') => {
                i += 2;
                while ch(i).is_some_and(|c| c != '\n' && c != '\r') {
                    i += 1;
                }
            }
            '/' if ch(i + 1) == Some('*') => {
                let mut j = i + 2;
                loop {
                    match ch(j) {
                        None => {
                            return Err(LexError::new("unterminated block comment", Span::new(start, src.len())));
                        }
                        Some('*') if ch(j + 1) == Some('/') => {
                            j += 2;
                            break;
                        }
                        Some(_) => j += 1,
                    }
                }
                i = j;
            }
            '"' => {
                let mut text = String::new();
                let mut j = i + 1;
                loop {
                    match ch(j) {
                        None => return Err(LexError::new("unterminated string literal", Span::new(start, src.len()))),
                        Some('"') if ch(j + 1) == Some('"') => {
                            text.push('"');
                            j += 2;
                        }
                        Some('"') => {
                            j += 1;
                            break;
                        }
                        Some(c) => {
                            text.push(c);
                            j += 1;
                        }
                    }
                }
                out.push(Token { kind: TokenKind::String(text), span: Span::new(start, pos(j)) });
                i = j;
            }
            '\'' => {
                let mut text = String::new();
                let mut j = i + 1;
                loop {
                    match ch(j) {
                        None => return Err(LexError::new("unterminated quoted identifier", Span::new(start, src.len()))),
                        Some('\'') if ch(j + 1) == Some('\'') => {
                            text.push('\'');
                            j += 2;
                        }
                        Some('\'') => {
                            j += 1;
                            break;
                        }
                        Some(c) => {
                            text.push(c);
                            j += 1;
                        }
                    }
                }
                if text.is_empty() {
                    return Err(LexError::new("empty quoted identifier", Span::new(start, pos(j))));
                }
                out.push(Token { kind: TokenKind::QuotedIdent(text), span: Span::new(start, pos(j)) });
                i = j;
            }
            '[' => {
                let mut text = String::new();
                let mut j = i + 1;
                loop {
                    match ch(j) {
                        None => return Err(LexError::new("unterminated bracketed identifier", Span::new(start, src.len()))),
                        Some(']') if ch(j + 1) == Some(']') => {
                            text.push(']');
                            j += 2;
                        }
                        Some(']') => {
                            j += 1;
                            break;
                        }
                        Some(c) => {
                            text.push(c);
                            j += 1;
                        }
                    }
                }
                if text.is_empty() {
                    return Err(LexError::new("empty bracketed identifier", Span::new(start, pos(j))));
                }
                out.push(Token { kind: TokenKind::BracketIdent(text), span: Span::new(start, pos(j)) });
                i = j;
            }
            '0'..='9' | '.' if c != '.' || ch(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                let mut j = i;
                let mut text = String::new();
                if c == '.' {
                    text.push_str("0.");
                    j += 1;
                    while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                        if let Some(d) = ch(j) {
                            text.push(d);
                        }
                        j += 1;
                    }
                } else {
                    while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                        if let Some(d) = ch(j) {
                            text.push(d);
                        }
                        j += 1;
                    }
                    if ch(j) == Some('.') {
                        text.push('.');
                        j += 1;
                        let fraction = j;
                        while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                            if let Some(d) = ch(j) {
                                text.push(d);
                            }
                            j += 1;
                        }
                        if j == fraction {
                            // `1.` / `1.e3`: keep the literal parseable.
                            text.push('0');
                        }
                    }
                }
                if matches!(ch(j), Some('e' | 'E')) {
                    text.push('e');
                    j += 1;
                    if matches!(ch(j), Some('+' | '-')) {
                        if let Some(sign) = ch(j) {
                            text.push(sign);
                        }
                        j += 1;
                    }
                    let digits = j;
                    while ch(j).is_some_and(|d| d.is_ascii_digit()) {
                        if let Some(d) = ch(j) {
                            text.push(d);
                        }
                        j += 1;
                    }
                    if j == digits {
                        return Err(LexError::new("numeric literal has no exponent digits", Span::new(start, pos(j))));
                    }
                }
                if ch(j).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.')) {
                    return Err(LexError::new("invalid numeric literal", Span::new(start, pos(j + 1))));
                }
                let value = text.parse::<f64>().map_err(|_| LexError::new("invalid numeric literal", Span::new(start, pos(j))))?;
                if !value.is_finite() {
                    return Err(LexError::new("numeric literal is out of range", Span::new(start, pos(j))));
                }
                out.push(Token { kind: TokenKind::Number(value), span: Span::new(start, pos(j)) });
                i = j;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut text = String::new();
                let mut j = i;
                while ch(j).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.')) {
                    if let Some(c) = ch(j) {
                        text.push(c);
                    }
                    j += 1;
                }
                let kind = if text.eq_ignore_ascii_case("NOT") {
                    TokenKind::Not
                } else if text.eq_ignore_ascii_case("IN") {
                    TokenKind::In
                } else if text.eq_ignore_ascii_case("EVALUATE") {
                    TokenKind::Evaluate
                } else {
                    TokenKind::Ident(text)
                };
                out.push(Token { kind, span: Span::new(start, pos(j)) });
                i = j;
            }
            '+' => {
                out.push(Token { kind: TokenKind::Plus, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '-' => {
                out.push(Token { kind: TokenKind::Minus, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '*' => {
                out.push(Token { kind: TokenKind::Star, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '/' => {
                out.push(Token { kind: TokenKind::Slash, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '^' => {
                out.push(Token { kind: TokenKind::Caret, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '&' if ch(i + 1) == Some('&') => {
                out.push(Token { kind: TokenKind::AndAnd, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '&' => {
                out.push(Token { kind: TokenKind::Amp, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '|' if ch(i + 1) == Some('|') => {
                out.push(Token { kind: TokenKind::OrOr, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '|' => return Err(LexError::new("unexpected `|`; the DAX logical OR operator is `||`", Span::new(start, pos(i + 1)))),
            '=' if ch(i + 1) == Some('=') => {
                out.push(Token { kind: TokenKind::StrictEq, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '=' if ch(i + 1) == Some('>') => {
                out.push(Token { kind: TokenKind::Arrow, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '=' => {
                out.push(Token { kind: TokenKind::Eq, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '<' if ch(i + 1) == Some('>') => {
                out.push(Token { kind: TokenKind::NotEq, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '<' if ch(i + 1) == Some('=') => {
                out.push(Token { kind: TokenKind::Le, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '<' => {
                out.push(Token { kind: TokenKind::Lt, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '>' if ch(i + 1) == Some('=') => {
                out.push(Token { kind: TokenKind::Ge, span: Span::new(start, pos(i + 2)) });
                i += 2;
            }
            '>' => {
                out.push(Token { kind: TokenKind::Gt, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '(' => {
                out.push(Token { kind: TokenKind::LParen, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            ')' => {
                out.push(Token { kind: TokenKind::RParen, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '{' => {
                out.push(Token { kind: TokenKind::LBrace, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            '}' => {
                out.push(Token { kind: TokenKind::RBrace, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            ',' => {
                out.push(Token { kind: TokenKind::Comma, span: Span::new(start, pos(i + 1)) });
                i += 1;
            }
            other => {
                return Err(LexError::new(format!("unexpected character `{other}`"), Span::new(start, pos(i + 1))));
            }
        }
    }

    out.push(Token { kind: TokenKind::Eof, span: Span::new(src.len(), src.len()) });
    Ok(out)
}
