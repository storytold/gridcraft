//! Tokenizer for DAX expressions.
//!
//! Spans are byte offsets into the source. Keywords are recognised case-insensitively; the
//! parser keeps the source spelling of identifiers and function names. `TRUE`/`FALSE` stay
//! identifiers here so the parser can tell the boolean literal from a call such as `TRUE()`.
//!
//! Numeric tokens are typed: digit-only text becomes [`TokenKind::Integer`] when it fits `i64`,
//! while a decimal point or exponent becomes [`TokenKind::Decimal`]. `dt"..."` text is
//! validated here and emitted as [`TokenKind::DateTime`]. Overflow and malformed date/time
//! literals are [`LexError`] values carrying the literal's span.

use crate::ast::{DateTimeLiteral, Span};

/// Longest input accepted, in bytes. Mirrors DAX's expression limit and bounds the size of the
/// tree a long operator chain can build.
pub const MAX_INPUT_LEN: usize = 8_192;

/// A lexical token.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// A digit-only literal that fits in `i64`, e.g. `42`.
    Integer(i64),
    /// A literal with a decimal point or exponent, e.g. `3.14`, `.5`, `1e3`.
    Decimal(f64),
    /// A validated `dt"..."` date/time literal.
    DateTime(DateTimeLiteral),
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
                // A digit-only literal is an Integer; a decimal point or exponent makes it a
                // Decimal. The sign is always a unary operator, never part of the token.
                let mut is_decimal = c == '.';
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
                        is_decimal = true;
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
                    is_decimal = true;
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
                let span = Span::new(start, pos(j));
                let kind = if is_decimal {
                    let value = text.parse::<f64>().map_err(|_| LexError::new("invalid numeric literal", span))?;
                    if !value.is_finite() {
                        return Err(LexError::new("decimal literal is out of range", span));
                    }
                    TokenKind::Decimal(value)
                } else {
                    let value =
                        text.parse::<i64>().map_err(|_| LexError::new("integer literal is out of range for a 64-bit signed integer", span))?;
                    TokenKind::Integer(value)
                };
                out.push(Token { kind, span });
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
                // `dt` immediately followed by a double-quoted string is a date/time literal.
                // Whitespace between the prefix and the quote is not allowed, so `dt "..."` is
                // two tokens and fails to parse. The prefix is case-insensitive like other DAX
                // keywords; the printer canonicalises it to lowercase `dt`.
                if text.eq_ignore_ascii_case("dt") && ch(j) == Some('"') {
                    let mut content = String::new();
                    let mut k = j + 1;
                    loop {
                        match ch(k) {
                            None => {
                                return Err(LexError::new("unterminated date/time literal", Span::new(start, src.len())));
                            }
                            Some('"') => {
                                k += 1;
                                break;
                            }
                            Some(c) => {
                                content.push(c);
                                k += 1;
                            }
                        }
                    }
                    let span = Span::new(start, pos(k));
                    let value = parse_datetime_literal(&content).map_err(|message| LexError::new(message, span))?;
                    out.push(Token { kind: TokenKind::DateTime(value), span });
                    i = k;
                    continue;
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

/// Validates the content of a `dt"..."` literal and splits it into components.
///
/// Accepted forms are `YYYY-MM-DD`, `YYYY-MM-DDThh:mm:ss` and `YYYY-MM-DD hh:mm:ss`. Date and
/// time components may be unpadded (`dt"2015-1-9T2:3:0"`), matching Microsoft's documented
/// example. Timezone offsets, fractional seconds, time-only values and invalid calendar dates
/// are rejected with a lexer error. The `dt` prefix itself is matched by the caller.
fn parse_datetime_literal(text: &str) -> Result<DateTimeLiteral, String> {
    let (date, time) = if let Some((date, time)) = text.split_once('T') {
        (date, Some(time))
    } else if let Some((date, time)) = text.split_once(' ') {
        (date, Some(time))
    } else {
        (text, None)
    };

    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second) = match time {
        Some(time) => parse_time(time)?,
        None => (0, 0, 0),
    };
    Ok(DateTimeLiteral { year, month, day, hour, minute, second, has_time: time.is_some() })
}

fn parse_date(text: &str) -> Result<(u32, u32, u32), String> {
    let mut parts = text.split('-');
    let year_text = parts.next().unwrap_or_default();
    let month_text = parts.next().unwrap_or_default();
    let day_text = parts.next().unwrap_or_default();
    if parts.next().is_some() || year_text.is_empty() || month_text.is_empty() || day_text.is_empty() {
        return Err("invalid date/time literal: the date must be YYYY-MM-DD".into());
    }
    if year_text.len() != 4 || month_text.len() > 2 || day_text.len() > 2 {
        return Err("invalid date/time literal: the date must be YYYY-MM-DD (unpadded month and day are allowed)".into());
    }
    let year = parse_component(year_text, "year")?;
    let month = parse_component(month_text, "month")?;
    let day = parse_component(day_text, "day")?;
    if year == 0 || year > 9999 {
        return Err("invalid date/time literal: the year must be between 0001 and 9999".into());
    }
    if !(1..=12).contains(&month) {
        return Err("invalid date/time literal: the month must be between 1 and 12".into());
    }
    if day == 0 || day > days_in_month(year, month) {
        return Err("invalid date/time literal: the date is not a valid calendar date".into());
    }
    Ok((year, month, day))
}

fn parse_time(text: &str) -> Result<(u32, u32, u32), String> {
    let mut parts = text.split(':');
    let hour_text = parts.next().unwrap_or_default();
    let minute_text = parts.next().unwrap_or_default();
    let second_text = parts.next().unwrap_or_default();
    if parts.next().is_some() || hour_text.is_empty() || minute_text.is_empty() || second_text.is_empty() {
        return Err("invalid date/time literal: the time must be hh:mm:ss".into());
    }
    if hour_text.len() > 2 || minute_text.len() > 2 || second_text.len() > 2 {
        return Err("invalid date/time literal: the time must be hh:mm:ss without fractional seconds".into());
    }
    let hour = parse_component(hour_text, "hour")?;
    let minute = parse_component(minute_text, "minute")?;
    let second = parse_component(second_text, "second")?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err("invalid date/time literal: the time is out of range".into());
    }
    Ok((hour, minute, second))
}

/// Parses one date/time component. Rejects signs, whitespace, fractional parts and everything
/// else that is not an ASCII digit, so timezone offsets (`Z`, `+02:00`) and fractional seconds
/// cannot slip through.
fn parse_component(text: &str, unit: &str) -> Result<u32, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid date/time literal: the {unit} must be digits"));
    }
    text.parse::<u32>().map_err(|_| format!("invalid date/time literal: the {unit} is out of range"))
}

const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}
