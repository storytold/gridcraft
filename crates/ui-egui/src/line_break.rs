//! Where wrapped text may start a new row, as Excel wraps cell text.

use egui::epaint::text::{LineBreak, LineBreaker, SimpleLineBreaker};

/// egui's [`SimpleLineBreaker`] without its breaks after ASCII punctuation, which Excel doesn't
/// make: in Excel 365 wrapped text starts a new row only after a space or a hyphen. A URL or a path
/// too wide for its column fills the row and breaks mid-word, not after a `/`, `.`, `@` or `_`.
/// Measured on `AAAAAAAAAA?BBBBBBBBBB?CCCCCCCCCC` with each ASCII punctuation mark for `?` in a
/// column two parts wide: only `-` (and a space) gave three rows. Breaks between CJK characters
/// are egui's, unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExcelLineBreaker;

impl LineBreaker for ExcelLineBreaker {
    fn line_breaks(&self, text: &str, language: Option<&str>, out: &mut Vec<LineBreak>) {
        let mut breaks = Vec::new();
        SimpleLineBreaker.line_breaks(text, language, &mut breaks);
        out.extend(breaks.into_iter().filter(|b| {
            let before = text.get(..b.offset).and_then(|s| s.chars().next_back());
            !before.is_some_and(|c| c.is_ascii_punctuation() && c != '-')
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str) -> Vec<usize> {
        let mut out = Vec::new();
        ExcelLineBreaker.line_breaks(text, None, &mut out);
        let mut v: Vec<usize> = out.into_iter().map(|b| b.offset).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn rows_end_after_spaces_and_hyphens_only() {
        assert_eq!(breaks("ab cd"), [3]);
        assert_eq!(breaks("ab-cd"), [3]);
        for p in ['/', '.', '@', '_', ':', ',', ';', '?', '&', '=', '#', '%', '!', ')', '+', '*', '~', '|', '$'] {
            assert_eq!(breaks(&format!("ab{p}cd")), Vec::<usize>::new(), "no break after {p}");
        }
        // A URL breaks only where it has a space or a hyphen.
        assert_eq!(breaks("https://example.com/a-b c"), [22, 24]);
    }

    #[test]
    fn cjk_breaks_are_kept() {
        assert_eq!(breaks("日本語"), breaks_simple("日本語"));
        assert!(!breaks("日本語").is_empty());
    }

    fn breaks_simple(text: &str) -> Vec<usize> {
        let mut out = Vec::new();
        SimpleLineBreaker.line_breaks(text, None, &mut out);
        let mut v: Vec<usize> = out.into_iter().map(|b| b.offset).collect();
        v.sort_unstable();
        v
    }
}
