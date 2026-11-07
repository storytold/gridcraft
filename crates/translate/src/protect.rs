//! Protecting numbers, dates, URLs, e-mail addresses and placeholder tokens from the model.
//!
//! Each protected span becomes an ASCII marker (`[[0]]`, `[[1]]`, …). ASCII brackets survive
//! subword tokenisers better than private-use characters, and a model that inserts spaces inside
//! a marker still restores. [`restore`] verifies that every marker comes back exactly once; if one
//! is lost, duplicated or the output has an unexpected marker, the segment fails rather than
//! corrupting a number or a URL.
//!
//! Marker syntax in the input is refused ([`ProtectError::MarkerCollision`]) so a mask can never
//! be confused with the user's own text.

use std::sync::LazyLock;

use regex::Regex;

/// One span taken out of the text for the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub marker: String,
    pub text: String,
    pub kind: TokenKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Url,
    Email,
    Placeholder,
    Date,
    Time,
    Number,
}

/// Text with protected spans replaced by markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protected {
    pub masked: String,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtectError {
    #[error("a placeholder starting at character {offset} is not closed")]
    MalformedPlaceholder { offset: usize },
    #[error("the text already contains a `[[number]]` marker")]
    MarkerCollision,
    #[error("the translation lost protection marker {index}")]
    Missing { index: usize },
    #[error("the translation repeated protection marker {index}")]
    Duplicate { index: usize },
    #[error("the translation contains a protection marker that was never issued")]
    UnknownMarker,
    #[error("the translation left marker syntax behind")]
    MalformedOutput,
}

static URL: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?://|www\.)[^\s<>"']+"#).ok());
static EMAIL: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b").ok());
static PLACEHOLDER: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\{\{[^{}]{1,64}\}\}|\{[^{}]{1,64}\}|\$\{[A-Za-z_][A-Za-z0-9_.\-]{0,63}\}|%[0-9]*\$?[sdif]").ok());
static ISO_DATE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d{4}-\d{1,2}-\d{1,2}\b").ok());
static NUM_DATE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d{1,4}[/.]\d{1,2}[/.]\d{1,4}\b").ok());
static TIME: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d{1,2}:\d{2}(?::\d{2})?\b").ok());
static NUMBER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d+(?:[.,'\u{2019}]\d+)*\b").ok());
static ANY_MARKER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[\[\s*\d+\s*\]\]").ok());
static FOUND_MARKER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[\s*\[\s*(\d{1,6})\s*\]\s*\]").ok());

fn push_match(out: &mut Vec<(usize, usize, TokenKind)>, re: &Option<Regex>, text: &str, kind: TokenKind) {
    let Some(re) = re else { return };
    for m in re.find_iter(text) {
        let (start, mut end) = (m.start(), m.end());
        // A URL at the end of a sentence keeps its full stop outside the marker.
        while end > start {
            let Some(last) = text[start..end].chars().next_back() else { break };
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'') {
                end -= last.len_utf8();
            } else {
                break;
            }
        }
        if end > start {
            out.push((start, end, kind));
        }
    }
}

/// Masks the protected spans in `text`. Errors on an unclosed placeholder or on input that
/// already looks like a marker.
pub fn protect(text: &str) -> Result<Protected, ProtectError> {
    if ANY_MARKER.as_ref().is_some_and(|re| re.is_match(text)) {
        return Err(ProtectError::MarkerCollision);
    }
    check_placeholders(text)?;
    let mut found = Vec::new();
    push_match(&mut found, &URL, text, TokenKind::Url);
    push_match(&mut found, &EMAIL, text, TokenKind::Email);
    push_match(&mut found, &PLACEHOLDER, text, TokenKind::Placeholder);
    push_match(&mut found, &ISO_DATE, text, TokenKind::Date);
    push_match(&mut found, &NUM_DATE, text, TokenKind::Date);
    push_match(&mut found, &TIME, text, TokenKind::Time);
    push_match(&mut found, &NUMBER, text, TokenKind::Number);
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)).then_with(|| kind_rank(a.2).cmp(&kind_rank(b.2))));
    let mut masked = String::with_capacity(text.len());
    let mut tokens = Vec::new();
    let mut last = 0usize;
    for (start, end, kind) in found {
        if start < last {
            continue;
        }
        masked.push_str(&text[last..start]);
        let marker = format!("[[{}]]", tokens.len());
        masked.push_str(&marker);
        tokens.push(Token { marker, text: text[start..end].to_string(), kind });
        last = end;
    }
    masked.push_str(&text[last..]);
    Ok(Protected { masked, tokens })
}

fn kind_rank(kind: TokenKind) -> u8 {
    // When two patterns start together, the more specific one wins.
    match kind {
        TokenKind::Url => 0,
        TokenKind::Email => 1,
        TokenKind::Placeholder => 2,
        TokenKind::Date => 3,
        TokenKind::Time => 4,
        TokenKind::Number => 5,
    }
}

fn check_placeholders(text: &str) -> Result<(), ProtectError> {
    let covered: Vec<(usize, usize)> = PLACEHOLDER.as_ref().map(|re| re.find_iter(text).map(|m| (m.start(), m.end())).collect()).unwrap_or_default();
    for (offset, ch) in text.char_indices() {
        if ch != '{' || covered.iter().any(|(s, _)| *s <= offset && offset < *s + 1) {
            continue;
        }
        let tail = &text[offset..];
        let window: usize = tail.char_indices().take(66).map(|(i, c)| i + c.len_utf8()).last().unwrap_or(tail.len());
        if !tail[..window.min(tail.len())].contains('}') {
            return Err(ProtectError::MalformedPlaceholder { offset });
        }
    }
    Ok(())
}

/// Marker indices that appear in `text`, in order of first appearance.
fn marker_indices(text: &str) -> Vec<usize> {
    let Some(found) = FOUND_MARKER.as_ref() else { return Vec::new() };
    let mut out: Vec<usize> = Vec::new();
    for m in found.find_iter(text) {
        let index = m.as_str().chars().filter(char::is_ascii_digit).collect::<String>().parse::<usize>().ok();
        if let Some(index) = index
            && !out.contains(&index)
        {
            out.push(index);
        }
    }
    out
}

/// Puts the protected spans back into `translated`. Every marker the mask put in this piece must
/// appear exactly once. Used for a whole masked cell (all of `tokens` expected).
pub fn restore(translated: &str, tokens: &[Token]) -> Result<String, ProtectError> {
    let expected: Vec<usize> = (0..tokens.len()).collect();
    restore_expected(translated, &expected, tokens)
}

/// Restores one sentence of a masked cell: only the markers that were in `source_piece` are
/// expected here, so a cell's other sentences cannot make this piece look incomplete.
pub fn restore_piece(translated: &str, source_piece: &str, tokens: &[Token]) -> Result<String, ProtectError> {
    restore_expected(translated, &marker_indices(source_piece), tokens)
}

fn restore_expected(translated: &str, expected: &[usize], tokens: &[Token]) -> Result<String, ProtectError> {
    let mut seen = vec![0usize; tokens.len()];
    let mut out = String::with_capacity(translated.len());
    let mut last = 0usize;
    let Some(found) = FOUND_MARKER.as_ref() else { return Err(ProtectError::MalformedOutput) };
    for m in found.find_iter(translated) {
        let index = m.as_str().chars().filter(char::is_ascii_digit).collect::<String>().parse::<usize>().ok();
        let Some(index) = index.filter(|i| *i < tokens.len() && expected.contains(i)) else { return Err(ProtectError::UnknownMarker) };
        seen[index] += 1;
        out.push_str(&translated[last..m.start()]);
        out.push_str(&tokens[index].text);
        last = m.end();
    }
    out.push_str(&translated[last..]);
    if let Some(index) = expected.iter().find(|i| seen[**i] == 0) {
        return Err(ProtectError::Missing { index: *index });
    }
    if let Some(index) = expected.iter().find(|i| seen[**i] > 1) {
        return Err(ProtectError::Duplicate { index: *index });
    }
    // Anything that still looks like a marker was not one we issued.
    if out.contains("[[") && FOUND_MARKER.as_ref().is_some_and(|re| re.is_match(&out)) {
        return Err(ProtectError::MalformedOutput);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) {
        let p = protect(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        let back = restore(&p.masked, &p.tokens).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        assert_eq!(back, text);
    }

    #[test]
    fn protects_numbers_and_dates() {
        let p = protect("The total is 42 dollars on 2026-11-07 at 14:30.").expect("protect");
        assert!(p.masked.contains("[[0]]"), "{}", p.masked);
        assert!(p.tokens.iter().any(|t| t.kind == TokenKind::Date));
        assert!(p.tokens.iter().any(|t| t.kind == TokenKind::Time));
        assert_eq!(p.tokens.iter().filter(|t| t.kind == TokenKind::Number).count(), 1);
        round_trip("The total is 42 dollars on 2026-11-07 at 14:30.");
    }

    #[test]
    fn protects_urls_emails_and_placeholders() {
        let text = "Write to ana@example.com or https://example.com/a?b=1 with {{name}}, {0} and %s.";
        let p = protect(text).expect("protect");
        assert!(p.tokens.iter().any(|t| t.kind == TokenKind::Email));
        assert!(p.tokens.iter().any(|t| t.kind == TokenKind::Url));
        assert_eq!(p.tokens.iter().filter(|t| t.kind == TokenKind::Placeholder).count(), 3);
        round_trip(text);
    }

    #[test]
    fn round_trips_ordinary_text_unchanged() {
        for text in ["Hello, world!", "Line one\nLine two with 1,234.56 and 7%.", "Order #A-17 ships 2026/12/01.", "No protected spans here.", ""] {
            round_trip(text);
        }
    }

    #[test]
    fn normalises_spaces_the_model_may_insert() {
        let p = protect("Pay 10 USD now.").expect("protect");
        let spaced = p.masked.replace("[[0]]", "[ [ 0 ] ]");
        assert_eq!(restore(&spaced, &p.tokens).unwrap_or_default(), "Pay 10 USD now.");
    }

    #[test]
    fn refuses_malformed_placeholders() {
        assert!(matches!(protect("Hello {name"), Err(ProtectError::MalformedPlaceholder { .. })));
        assert!(matches!(protect("Hello ${name"), Err(ProtectError::MalformedPlaceholder { .. })));
    }

    #[test]
    fn refuses_marker_collisions() {
        assert_eq!(protect("Already [[0]] here").unwrap_err(), ProtectError::MarkerCollision);
    }

    #[test]
    fn restore_rejects_lost_duplicate_and_unknown_markers() {
        let p = protect("Pay 10 now.").expect("protect");
        assert!(matches!(restore("Paga ahora.", &p.tokens), Err(ProtectError::Missing { .. })));
        let twice = format!("{} {}", p.masked, p.masked);
        assert!(matches!(restore(&twice, &p.tokens), Err(ProtectError::Duplicate { .. })));
        assert!(matches!(restore("[[9]]", &p.tokens), Err(ProtectError::UnknownMarker)));
    }
}
