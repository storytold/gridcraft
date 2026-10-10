//! Tokenizer. VBA is line-oriented (a newline usually ends a statement) rather than
//! semicolon-oriented, so [`Token::Newline`] is a real token, not whitespace — the parser treats
//! it (and `:`) as a statement separator.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    Newline,
    Colon,
    Comma,
    Dot,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

pub fn lex(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let n = chars.len();
    while i < n {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '_' if line_continuation(&chars, i) => {
                // `_` at the end of a physical line joins it with the next: skip past the
                // newline so no `Newline` token is emitted here.
                i += 1;
                while i < n && (chars[i] == ' ' || chars[i] == '\t' || chars[i] == '\r') {
                    i += 1;
                }
                if i < n && chars[i] == '\n' {
                    i += 1;
                }
            }
            '\n' => {
                out.push(Token::Newline);
                i += 1;
            }
            '\'' => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            ':' => {
                out.push(Token::Colon);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '.' => {
                out.push(Token::Dot);
                i += 1;
            }
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            '+' => {
                out.push(Token::Plus);
                i += 1;
            }
            '-' => {
                out.push(Token::Minus);
                i += 1;
            }
            '*' => {
                out.push(Token::Star);
                i += 1;
            }
            '/' => {
                out.push(Token::Slash);
                i += 1;
            }
            '^' => {
                out.push(Token::Caret);
                i += 1;
            }
            '&' => {
                out.push(Token::Amp);
                i += 1;
            }
            '=' => {
                out.push(Token::Eq);
                i += 1;
            }
            '<' => {
                if i + 1 < n && chars[i + 1] == '>' {
                    out.push(Token::Ne);
                    i += 2;
                } else if i + 1 < n && chars[i + 1] == '=' {
                    out.push(Token::Le);
                    i += 2;
                } else {
                    out.push(Token::Lt);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    out.push(Token::Ge);
                    i += 2;
                } else {
                    out.push(Token::Gt);
                    i += 1;
                }
            }
            '"' => {
                let (s, next) = lex_string(&chars, i);
                out.push(Token::Str(s));
                i = next;
            }
            _ if c.is_ascii_digit() => {
                let (v, next) = lex_number(&chars, i);
                out.push(Token::Number(v));
                i = next;
            }
            _ if c.is_alphabetic() || c == '_' => {
                let (s, next) = lex_ident(&chars, i);
                // `Rem` is a comment keyword, equivalent to `'`.
                if s.eq_ignore_ascii_case("rem") {
                    let mut j = next;
                    while j < n && chars[j] != '\n' {
                        j += 1;
                    }
                    i = j;
                } else {
                    out.push(Token::Ident(s));
                    i = next;
                }
            }
            _ => i += 1, // Skip anything unrecognized rather than failing the whole module.
        }
    }
    out.push(Token::Eof);
    out
}

/// True when the `_` at `i` is a line-continuation: only whitespace between it and the newline.
fn line_continuation(chars: &[char], i: usize) -> bool {
    let mut j = i + 1;
    while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\r') {
        j += 1;
    }
    j >= chars.len() || chars[j] == '\n'
}

fn lex_string(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start + 1;
    let mut s = String::new();
    let n = chars.len();
    while i < n {
        if chars[i] == '"' {
            if i + 1 < n && chars[i + 1] == '"' {
                s.push('"');
                i += 2;
            } else {
                i += 1;
                break;
            }
        } else if chars[i] == '\n' {
            break; // Unterminated string: stop at end of line rather than consuming the file.
        } else {
            s.push(chars[i]);
            i += 1;
        }
    }
    (s, i)
}

fn lex_number(chars: &[char], start: usize) -> (f64, usize) {
    let mut i = start;
    let n = chars.len();
    while i < n && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < n && chars[i] == '.' {
        i += 1;
        while i < n && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    let text: String = chars[start..i].iter().collect();
    (text.parse().unwrap_or(0.0), i)
}

fn lex_ident(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let n = chars.len();
    while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}
