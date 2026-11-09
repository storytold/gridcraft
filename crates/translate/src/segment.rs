//! Segmenting cell text by line and sentence without losing a byte.
//!
//! [`segment`] returns contiguous ranges that cover the whole input in order, so joining the
//! translated text pieces with the untouched whitespace and newline pieces reproduces the cell
//! exactly. Only [`SegmentKind::Text`] pieces are sent to a model.
//!
//! Sentence ends cover Latin `.`, `!`, `?`, `…`, CJK `。！？；…` and the right-to-left
//! `؟` (Arabic question mark), `؛` (Arabic semicolon) and `۔` (Urdu full stop). A `.` inside a
//! number, a URL, an abbreviation or an initial does not end a sentence. Protection runs before
//! segmentation, so numbers and URLs are already markers when this sees the text.

use std::ops::Range;

/// One piece of a cell's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    pub range: Range<usize>,
    pub kind: SegmentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    /// Prose to translate.
    Text,
    /// Spaces, tabs and other non-newline whitespace between pieces.
    Whitespace,
    /// A line break: `\n`, `\r` or a CRLF's two bytes.
    Newline,
}

const CLOSERS: [char; 10] = ['"', '\'', '”', '’', ')', ']', '}', '»', '›', '」'];

/// Common abbreviations whose full stop does not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g", "i.e", "fig", "figs", "no", "nos", "inc", "ltd", "co", "approx", "dept",
    "est", "min", "max", "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "vs.", "al",
];

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？' | '；' | '؟' | '؛' | '۔' | '।' | '॥')
}

fn is_cjk_terminator(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '；' | '…' | '؟' | '؛' | '۔' | '।' | '॥')
}

fn is_closer(c: char) -> bool {
    CLOSERS.contains(&c)
}

/// Segments `text` into prose, whitespace and line breaks. The ranges are contiguous and cover
/// the entire string.
pub fn segment(text: &str) -> Vec<Segment<'_>> {
    let mut out: Vec<Segment<'_>> = Vec::new();
    let mut i = 0usize;
    let mut text_start: Option<usize> = None;
    while i < text.len() {
        let Some(ch) = text[i..].chars().next() else { break };
        let len = ch.len_utf8();
        if ch == '\n' || ch == '\r' {
            flush(&mut out, text, &mut text_start, i);
            out.push(Segment { text: &text[i..i + len], range: i..i + len, kind: SegmentKind::Newline });
            i += len;
            continue;
        }
        if text_start.is_none() && ch.is_whitespace() {
            let start = i;
            while i < text.len() {
                let Some(c) = text[i..].chars().next() else { break };
                if c == '\n' || c == '\r' || !c.is_whitespace() {
                    break;
                }
                i += c.len_utf8();
            }
            out.push(Segment { text: &text[start..i], range: start..i, kind: SegmentKind::Whitespace });
            continue;
        }
        if text_start.is_none() {
            text_start = Some(i);
        }
        if is_terminator(ch) && ends_sentence(text, i, ch) {
            let mut end = i + len;
            // Keep the sentence's closing quote or bracket, and a run like "?!" together.
            while let Some(c) = text[end..].chars().next() {
                if is_closer(c) || (is_terminator(c) && c != '.') {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            if let Some(start) = text_start.take() {
                out.push(Segment { text: &text[start..end], range: start..end, kind: SegmentKind::Text });
            }
            i = end;
            continue;
        }
        i += len;
    }
    flush(&mut out, text, &mut text_start, text.len());
    out
}

fn flush<'a>(out: &mut Vec<Segment<'a>>, text: &'a str, start: &mut Option<usize>, end: usize) {
    if let Some(s) = start.take()
        && s < end
    {
        out.push(Segment { text: &text[s..end], range: s..end, kind: SegmentKind::Text });
    }
}

fn ends_sentence(text: &str, at: usize, ch: char) -> bool {
    if is_cjk_terminator(ch) {
        return true;
    }
    if ch != '.' {
        // `!` and `?` end a sentence unless they run into a word ("a!b" is rare); accept them.
        return true;
    }
    let next = text[at + 1..].chars().next();
    // 3.14, version 1.2.3 and URLs: the dot is inside a token.
    if next.is_some_and(|c| !c.is_whitespace() && !is_closer(c) && !is_terminator(c)) {
        return false;
    }
    // An ellipsis ("..." or "…"): no sentence end at the first dots.
    if next == Some('.') {
        return false;
    }
    let before = &text[..at];
    let word: String = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '.').collect::<String>().chars().rev().collect();
    let lower = word.to_ascii_lowercase();
    if ABBREVIATIONS.contains(&lower.as_str()) {
        return false;
    }
    // A single capital letter is an initial ("J. Smith").
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() == 1 && letters[0].is_uppercase() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(text: &str) -> String {
        segment(text).into_iter().map(|s| s.text).collect()
    }

    fn prose(text: &str) -> Vec<&str> {
        segment(text).into_iter().filter(|s| s.kind == SegmentKind::Text).map(|s| s.text).collect()
    }

    #[test]
    fn ranges_cover_the_text_without_loss() {
        for text in [
            "Hello. World!",
            "Line one\nLine two\r\nLine three",
            "你好。世界！",
            "مرحبا بالعالم؟ نعم۔",
            "  leading and trailing  ",
            "Dr. Smith met Mr. Jones at 3.14 p.m. today.",
            "Visit https://example.com/a.b for 1.2.3.",
            "שלום עולם. מה שלומך?",
            "",
        ] {
            assert_eq!(join(text), text, "segments must reproduce {text:?}");
            let segments = segment(text);
            let mut at = 0;
            for s in &segments {
                assert_eq!(s.range.start, at);
                assert_eq!(s.range.end, s.range.start + s.text.len());
                at = s.range.end;
            }
            assert_eq!(at, text.len());
        }
    }

    #[test]
    fn splits_latin_sentences_and_keeps_newlines() {
        let s = segment("First. Second!\nThird?");
        let kinds: Vec<SegmentKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![SegmentKind::Text, SegmentKind::Whitespace, SegmentKind::Text, SegmentKind::Newline, SegmentKind::Text]);
        assert_eq!(prose("First. Second!\nThird?"), vec!["First.", "Second!", "Third?"]);
    }

    #[test]
    fn keeps_abbreviations_and_initials_together() {
        assert_eq!(prose("Dr. Smith arrived. He left."), vec!["Dr. Smith arrived.", "He left."]);
        assert_eq!(prose("J. K. Rowling wrote it."), vec!["J. K. Rowling wrote it."]);
        assert_eq!(prose("Version 1.2.3 is out. Good."), vec!["Version 1.2.3 is out.", "Good."]);
    }

    #[test]
    fn splits_cjk_without_spaces() {
        assert_eq!(prose("你好。世界！これはテストです。"), vec!["你好。", "世界！", "これはテストです。"]);
    }

    #[test]
    fn splits_rtl_punctuation() {
        assert_eq!(prose("مرحبا؟ نعم"), vec!["مرحبا؟", "نعم"]);
        assert_eq!(prose("اردو۔ اگلا"), vec!["اردو۔", "اگلا"]);
    }

    #[test]
    fn keeps_closing_quotes_with_the_sentence() {
        assert_eq!(prose("He said \"stop.\" Then left."), vec!["He said \"stop.\"", "Then left."]);
        assert_eq!(prose("真的吗？」是的。"), vec!["真的吗？」", "是的。"]);
    }

    #[test]
    fn blank_text_has_no_segments() {
        assert!(segment("").is_empty());
        assert!(segment("   ").iter().all(|s| s.kind != SegmentKind::Text));
    }
}
