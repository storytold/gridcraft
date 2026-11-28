//! A mention is explicit workbook context, not an instruction hidden in pasted code or an email.

use std::ops::Range;

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

// Recognize the shape separately so an out-of-bounds address cannot silently fall back to @read.
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

fn sheet_word(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '$')
}

fn has_sheet_operand(tail: &str) -> bool {
    tail.strip_prefix('!').and_then(|s| s.chars().next()).is_some_and(|c| sheet_word(c) || matches!(c, '[' | '\'' | ':'))
}

pub(super) fn starts_qualified(s: &str) -> bool {
    has_sheet_operand(s.trim_start_matches(sheet_word))
}

#[derive(Clone, Copy, PartialEq)]
enum ReadForm {
    Direct,   // @B2:D8
    Optional, // @read with an optional address
    Required, // @read: must be followed by an address
}

fn sheet_prefix(s: &str, explicit: bool) -> Result<Option<(String, usize)>, String> {
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
    let tail = s.trim_start_matches(sheet_word);
    if has_sheet_operand(tail) || (explicit && tail.starts_with('!') && !looks_like_a1(&s[..s.len() - tail.len()])) {
        let end = s.len() - tail.len();
        if end == 0 || end > 256 {
            return Err(BAD_TARGET.into());
        }
        return Ok(Some((s[..end].to_owned(), end + 1)));
    }
    // Do not silently attach the selection (or a range like Q1) when an unquoted sheet
    // name with a space was intended, as in @read Q1 Sales!B3.
    if explicit && tail.starts_with([' ', '\t']) {
        let words = tail.trim_start_matches(|ch| sheet_word(ch) || matches!(ch, ' ' | '\t'));
        if let Some(rest) = words.strip_prefix('!')
            && looks_like_a1(&rest[..token_len(rest)])
        {
            return Err(BAD_TARGET.into());
        }
    }
    Ok(None)
}

fn target_prefix(s: &str, form: ReadForm) -> Result<Option<Target<'_>>, String> {
    let (sheet, start) = sheet_prefix(s, form != ReadForm::Direct)?.map_or((None, 0), |(s, n)| (Some(s), n));
    let rest = &s[start..];
    let n = token_len(rest);
    let address = &rest[..n];
    if looks_like_a1(address) && !has_sheet_operand(&rest[n..]) {
        if gridcraft_engine::core::RangeRef::parse(address).is_none() {
            return Err(BAD_TARGET.into());
        }
        Ok(Some(Target { sheet, address, consumed: start + n }))
    } else if form == ReadForm::Required || sheet.is_some() || (address.contains(':') && address.bytes().any(|c| c.is_ascii_digit())) {
        Err(BAD_TARGET.into())
    } else {
        Ok(None)
    }
}

pub(super) fn quote_sheet(sheet: &str) -> String {
    format!("'{}'", sheet.replace('\'', "''"))
}

enum Reference<'a> {
    Object(&'static str),
    Cells(Target<'a>),
}

impl Reference<'_> {
    fn canonical(self) -> String {
        match self {
            Self::Object(name) => name.to_owned(),
            Self::Cells(target) => {
                let addr = target.address.to_ascii_uppercase();
                match target.sheet {
                    Some(s) => format!("@read:{}!{addr}", quote_sheet(&s)),
                    None => format!("@read:{addr}"),
                }
            }
        }
    }
}

/// Decode a canonical range token for the workbook. Never look up a sheet while parsing;
/// the workbook layer rechecks visibility and range size against the active document.
pub(in super::super) fn read_target(token: &str) -> Result<(Option<String>, String), String> {
    let arg = token.strip_prefix("@read:").or_else(|| token.strip_prefix('@')).ok_or(BAD_TARGET)?;
    let t = target_prefix(arg, ReadForm::Required)?.filter(|t| t.consumed == arg.len()).ok_or(BAD_TARGET)?;
    Ok((t.sheet, t.address.to_ascii_uppercase()))
}

fn explicit_read(after: &str) -> Option<(&str, ReadForm)> {
    if !after.get(..4)?.eq_ignore_ascii_case("read") {
        return None;
    }
    let tail = &after[4..];
    if let Some(s) = tail.strip_prefix(':') {
        Some((s, ReadForm::Required))
    } else if tail.starts_with([' ', '\t']) {
        Some((tail.trim_start_matches([' ', '\t']), ReadForm::Optional))
    } else {
        None
    }
}

/// Returns the source length, including the sigil, without swallowing prose after a bare @read.
fn parse_mention(source: &str) -> Result<Option<(usize, Reference<'_>)>, String> {
    let after = &source[1..];
    if let Some((arg, form)) = explicit_read(after) {
        return Ok(Some(match target_prefix(arg, form)? {
            Some(target) => (source.len() - arg.len() + target.consumed, Reference::Cells(target)),
            None => ("@read".len(), Reference::Object("@read")),
        }));
    }
    let end = 1 + token_len(after);
    let named = MENTIONS.iter().copied().find(|name| source[..end].eq_ignore_ascii_case(name));
    // A sheet qualifier (e.g. "@cell.archive!B2") takes precedence over an object name.
    if let Some(name) = named
        && !starts_qualified(after)
    {
        return Ok(Some((end, Reference::Object(name))));
    }
    Ok(target_prefix(after, ReadForm::Direct)?.map(|target| (1 + target.consumed, Reference::Cells(target))))
}

/// Both the editor and submission consume this scan. Source spans are UTF-8 byte offsets;
/// every occurrence has its own span even when several spellings refer to the same attachment.
fn scan(text: &str) -> impl Iterator<Item = Result<(Range<usize>, Reference<'_>), String>> {
    let mut at = 0;
    let mut code = Code::new(text);
    std::iter::from_fn(move || {
        while at < text.len() {
            let start = at;
            if text.as_bytes()[at] != b'@' || code.len != 0 || !boundary(&text[..at]) {
                at += code.advance(text, at);
                continue;
            }
            at += 1;
            match parse_mention(&text[start..]) {
                Ok(Some((len, reference))) => {
                    at = start + len;
                    return Some(Ok((start..at, reference)));
                }
                Ok(None) => {}
                Err(err) => return Some(Err(err)),
            }
        }
        None
    })
}

pub(in super::super) fn mention_spans(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    scan(text).filter_map(|item| item.ok().map(|(span, _)| span))
}

/// Capture each referenced workbook object once. A bare @read attaches the selection.
pub(in super::super) fn mentions(text: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for item in scan(text) {
        let (_, reference) = item?;
        let tag = reference.canonical();
        if !out.iter().any(|existing| existing.eq_ignore_ascii_case(&tag)) {
            if out.len() == 8 {
                return Err("Attach up to eight workbook references per message.".into());
            }
            out.push(tag);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_spellings_paint_the_typed_text_and_share_attachments() {
        let text = "🧮 (@b2:D8), @read:B2:d8, @read B2:D8; @Période!$c$3:e4 or @READ:'Période'!$C$3:E4; @'Q1 Sales'' East'!A1.";
        let spans: Vec<_> = mention_spans(text).map(|r| &text[r]).collect();
        assert_eq!(spans, ["@b2:D8", "@read:B2:d8", "@read B2:D8", "@Période!$c$3:e4", "@READ:'Période'!$C$3:E4", "@'Q1 Sales'' East'!A1"]);
        let objects = mentions(text).unwrap();
        assert_eq!(objects, ["@read:B2:D8", "@read:'Période'!$C$3:E4", "@read:'Q1 Sales'' East'!A1"]);
        assert_eq!(read_target(&objects[2]).unwrap(), (Some("Q1 Sales' East".into()), "A1".into()));
    }

    #[test]
    fn prose_code_and_email_do_not_become_attachments() {
        let text = "@read   this report from user@A1: `@Data!A1`\n> ~~~\n> @'Q 1'!B2:D3\n> ~~~\n@cell on Sheet1!B2, @selection!";
        assert_eq!(mention_spans(text).map(|r| &text[r]).collect::<Vec<_>>(), ["@read", "@cell", "@selection"]);
        assert_eq!(mentions(text).unwrap(), ["@read", "@cell", "@selection"]);
    }

    #[test]
    fn invalid_targets_do_not_paint_or_fall_back_to_the_selection() {
        for bad in ["@read:", "@read:Q1 Sales!B2", "@B2:D", "@Sales!B2:D3:E4", "@'Q1 Sales'!", "@[External]Data!A1", "@read XFE1"] {
            let text = format!("{bad}, then @C3");
            assert!(mentions(&text).is_err(), "{bad}");
            assert_eq!(mention_spans(&text).map(|r| &text[r]).collect::<Vec<_>>(), ["@C3"], "{bad}");
        }
    }

    #[test]
    fn direct_references_do_not_consume_prose_or_sentence_punctuation() {
        let text = "Compare @A1 with Sheet2!B2. Check @Data!B2:D8! Then (@A1!), @cell! Ignore @nobody!";
        assert_eq!(mention_spans(text).map(|r| &text[r]).collect::<Vec<_>>(), ["@A1", "@Data!B2:D8", "@A1", "@cell"]);
        assert_eq!(mentions(text).unwrap(), ["@read:A1", "@read:'Data'!B2:D8", "@cell"]);
        assert!(mentions("@read Q1 Sales!B2").is_err());
        assert!(mentions("@Data!B2!C4").is_err());
    }
}
