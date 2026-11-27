//! A mention is explicit workbook context, not an instruction hidden in pasted code or an email.

use super::MENTIONS;

pub(super) fn boundary(before: &str) -> bool {
    before.chars().next_back().is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\'' | '“' | '‘'))
}

#[derive(Default)]
struct Code {
    delimiter: u8,
    len: usize,
    fence: bool,
    quote_depth: usize,
    line_quotes: usize,
    fence_at: Option<usize>,
}

impl Code {
    fn new(text: &str) -> Self {
        let mut code = Self::default();
        code.line_prefix(text.as_bytes(), 0);
        code
    }

    // Read a line's blockquote prefix once. A tab advances to a four-column stop;
    // the optional separator after '>' consumes just one of those columns.
    fn line_prefix(&mut self, bytes: &[u8], mut at: usize) {
        let mut column = 0;
        let mut base = 0;
        self.line_quotes = 0;
        self.fence_at = None;
        loop {
            match bytes.get(at) {
                Some(b' ') => column += 1,
                Some(b'\t') => column += 4 - column % 4,
                Some(b'>') => {
                    self.line_quotes += 1;
                    column += 1;
                    base = column + usize::from(matches!(bytes.get(at + 1), Some(b' ' | b'\t')));
                }
                _ => {
                    self.fence_at = Some(at);
                    break;
                }
            }
            at += 1;
            if column > base + 3 {
                break;
            }
        }
        if self.fence && self.line_quotes < self.quote_depth {
            self.len = 0;
        }
    }

    /// Advance over a Markdown code delimiter. Tildes only open fenced blocks at line start.
    fn advance(&mut self, text: &str, at: usize) -> usize {
        let bytes = text.as_bytes();
        let byte = bytes[at];
        if byte == b'\n' || (byte == b'\r' && bytes.get(at + 1) != Some(&b'\n')) {
            self.line_prefix(bytes, at + 1);
        }
        if !matches!(byte, b'`' | b'~') {
            return 1;
        }
        let len = bytes[at..].iter().take_while(|&&c| c == byte).count();
        let at_prefix = self.fence_at == Some(at);
        if self.len == 0 {
            let fence = len >= 3 && at_prefix;
            if (byte == b'`' || fence) && !text[..at].ends_with('\\') {
                self.delimiter = byte;
                self.len = len;
                self.fence = fence;
                self.quote_depth = if fence { self.line_quotes } else { 0 };
            }
        } else if byte == self.delimiter {
            let close = if self.fence {
                len >= self.len
                    && at_prefix
                    && self.line_quotes == self.quote_depth
                    && bytes[at + len..].iter().take_while(|&&b| !matches!(b, b'\r' | b'\n')).all(|&b| matches!(b, b' ' | b'\t'))
            } else {
                len == self.len
            };
            if close {
                self.len = 0;
            }
        }
        len
    }
}

pub(super) fn in_code(text: &str) -> bool {
    let mut code = Code::new(text);
    let mut at = 0;
    while at < text.len() {
        at += code.advance(text, at);
    }
    code.len != 0
}

// Syntax only. Bounds and actual cell resolution belong to workbook::mention.
pub(super) fn looks_like_a1(token: &str) -> bool {
    fn cell(s: &str) -> bool {
        let s = s.strip_prefix('$').unwrap_or(s);
        let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
        let row = s[letters..].strip_prefix('$').unwrap_or(&s[letters..]);
        (1..=3).contains(&letters) && !row.is_empty() && row.bytes().all(|c| c.is_ascii_digit())
    }
    match token.split_once(':') {
        Some((a, b)) => cell(a) && cell(b),
        None => cell(token),
    }
}

fn token_len(s: &str) -> usize {
    s.bytes().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b':')).count()
}

const BAD_TARGET: &str = "Use an A1 cell or range, optionally on a visible sheet: @read 'Q1 Sales'!B2:D8.";

struct Target<'a> {
    sheet: Option<String>,
    address: &'a str,
    consumed: usize,
}

fn sheet_prefix(s: &str) -> Result<Option<(String, usize)>, String> {
    if s.starts_with('[') {
        return Err("Workbook references cannot attach an external workbook.".into());
    }
    if s.starts_with('\'') {
        let mut chars = s.char_indices().skip(1).peekable();
        let mut name = String::new();
        while let Some((at, ch)) = chars.next() {
            if at > 512 || name.len() > 256 {
                return Err(BAD_TARGET.into());
            }
            match ch {
                '\'' if chars.next_if(|(_, c)| *c == '\'').is_some() => name.push('\''),
                '\'' if s[at + 1..].starts_with('!') && !name.is_empty() => return Ok(Some((name, at + 2))),
                '\'' | '[' | ']' | '\n' | '\r' => return Err(BAD_TARGET.into()),
                c => name.push(c),
            }
        }
        return Err(BAD_TARGET.into());
    }
    let end = s.find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '$'))).unwrap_or(s.len());
    if s[end..].starts_with('!') {
        if end == 0 || end > 256 {
            return Err(BAD_TARGET.into());
        }
        return Ok(Some((s[..end].to_owned(), end + 1)));
    }
    // Do not silently attach the selection (or a range like Q1) when an unquoted sheet
    // name with a space was intended, as in @read Q1 Sales!B3.
    let end = s.find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, ' ' | '\t' | '_' | '-' | '.' | '$'))).unwrap_or(s.len());
    if let Some(rest) = s[end..].strip_prefix('!')
        && looks_like_a1(&rest[..token_len(rest)])
    {
        return Err(BAD_TARGET.into());
    }
    Ok(None)
}

fn target_prefix(s: &str, required: bool) -> Result<Option<Target<'_>>, String> {
    let (sheet, start) = sheet_prefix(s)?.map_or((None, 0), |(s, n)| (Some(s), n));
    let rest = &s[start..];
    let n = token_len(rest);
    let address = &rest[..n];
    if looks_like_a1(address) && !rest[n..].starts_with('!') {
        Ok(Some(Target { sheet, address, consumed: start + n }))
    } else if required || sheet.is_some() || (address.contains(':') && address.bytes().any(|c| c.is_ascii_digit())) {
        Err(BAD_TARGET.into())
    } else {
        Ok(None)
    }
}

pub(super) fn quote_sheet(sheet: &str) -> String {
    format!("'{}'", sheet.replace('\'', "''"))
}

fn canonical(target: Target<'_>) -> String {
    let addr = target.address.to_ascii_uppercase();
    match target.sheet {
        Some(s) => format!("@read:{}!{addr}", quote_sheet(&s)),
        None => format!("@read:{addr}"),
    }
}

/// Decode a canonical range token for the workbook. Never look up a sheet while parsing;
/// the workbook layer rechecks visibility and range size against the active document.
pub(in super::super) fn read_target(token: &str) -> Result<(Option<String>, String), String> {
    let arg = token.strip_prefix("@read:").or_else(|| token.strip_prefix('@')).ok_or(BAD_TARGET)?;
    let t = target_prefix(arg, true)?.filter(|t| t.consumed == arg.len()).ok_or(BAD_TARGET)?;
    Ok((t.sheet, t.address.to_ascii_uppercase()))
}

fn explicit_read(after: &str) -> Option<(&str, bool)> {
    if !after.get(..4)?.eq_ignore_ascii_case("read") {
        return None;
    }
    let tail = &after[4..];
    if let Some(s) = tail.strip_prefix(':') {
        Some((s, true))
    } else if tail.starts_with([' ', '\t']) {
        Some((tail.trim_start_matches([' ', '\t']), false))
    } else {
        None
    }
}

/// Capture each referenced workbook object once. A bare @read attaches the selection.
pub(in super::super) fn mentions(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut code = Code::new(text);
    while at < text.len() {
        let b = text.as_bytes()[at];
        if b != b'@' || code.len != 0 || !boundary(&text[..at]) {
            at += code.advance(text, at);
            continue;
        }
        let after = &text[at + 1..];
        at += 1;
        let tag = if let Some((arg, required)) = explicit_read(after) {
            at += after.len() - arg.len();
            if let Some(target) = target_prefix(arg, required)? {
                at += target.consumed;
                canonical(target)
            } else {
                "@read".into()
            }
        } else {
            let n = token_len(after);
            let raw = &after[..n];
            at += n;
            if looks_like_a1(raw) {
                if after[n..].starts_with('!') {
                    return Err(BAD_TARGET.into());
                }
                format!("@read:{}", raw.to_ascii_uppercase())
            } else {
                let name = format!("@{}", raw.to_ascii_lowercase());
                if !MENTIONS.contains(&name.as_str()) {
                    continue;
                }
                name
            }
        };
        if !out.contains(&tag) {
            if out.len() == 8 {
                return Err("Attach up to eight workbook references per message.".into());
            }
            out.push(tag);
        }
    }
    Ok(out)
}
