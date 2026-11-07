//! Caps, de-duplication and batch planning.
//!
//! The limits are checked before anything is allocated for a model. Identical segment texts are
//! translated once; batches are packed by an estimated model-token budget, not by cell count, so
//! a few long sentences do not become one oversized decoder call.

use std::collections::HashMap;

use crate::error::TranslateError;

/// Caps for one request. Callers may lower them; no command raises them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_cells: usize,
    pub max_chars: usize,
    pub max_tokens: usize,
    pub max_cell_chars: usize,
}

pub const DEFAULT_LIMITS: Limits = Limits { max_cells: 10_000, max_chars: 2_000_000, max_tokens: 512_000, max_cell_chars: 32_000 };

/// A plan over de-duplicated segment texts.
#[derive(Clone, Debug, Default)]
pub struct BatchPlan {
    /// Each distinct text, in first-seen order.
    pub unique: Vec<String>,
    /// `mapping[i]` is the index in `unique` of input segment `i`.
    pub mapping: Vec<usize>,
    /// Indices into `unique`, grouped into model calls.
    pub batches: Vec<Vec<usize>>,
    pub chars: usize,
    pub tokens: usize,
}

/// Plans model calls for `texts`. Errors, rather than truncating, when a cap is exceeded: a
/// request that cannot be completed must not silently translate half a selection.
pub fn plan(texts: &[String], limits: &Limits) -> Result<BatchPlan, TranslateError> {
    if texts.len() > limits.max_cells {
        return Err(TranslateError::TooManyCells { count: texts.len(), limit: limits.max_cells });
    }
    let mut total_chars = 0usize;
    for (index, text) in texts.iter().enumerate() {
        let chars = text.chars().count();
        if chars > limits.max_cell_chars {
            return Err(TranslateError::CellTooLong { index, count: chars, limit: limits.max_cell_chars });
        }
        total_chars = total_chars.saturating_add(chars);
        if total_chars > limits.max_chars {
            return Err(TranslateError::TooManyChars { count: total_chars, limit: limits.max_chars });
        }
    }
    let mut unique: Vec<String> = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut mapping = Vec::with_capacity(texts.len());
    for text in texts {
        let key = text.as_str();
        if let Some(&at) = seen.get(key) {
            mapping.push(at);
        } else {
            let at = unique.len();
            unique.push(text.clone());
            seen.insert(key, at);
            mapping.push(at);
        }
    }
    let estimates: Vec<usize> = unique.iter().map(|t| estimate_tokens(t)).collect();
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_tokens = 0usize;
    let mut total_tokens = 0usize;
    for (index, estimate) in estimates.into_iter().enumerate() {
        if estimate > limits.max_tokens {
            return Err(TranslateError::TooManyTokens { tokens: estimate, limit: limits.max_tokens });
        }
        total_tokens = total_tokens.saturating_add(estimate);
        if !current.is_empty() && current_tokens.saturating_add(estimate) > limits.max_tokens {
            batches.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(index);
        current_tokens = current_tokens.saturating_add(estimate);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    Ok(BatchPlan { unique, mapping, batches, chars: total_chars, tokens: total_tokens })
}

/// A conservative token estimate for a sentencepiece model: about four Latin characters per
/// token, one token per CJK character, and never less than one for non-empty text.
pub fn estimate_tokens(text: &str) -> usize {
    let mut chars = 0usize;
    let mut wide = 0usize;
    for c in text.chars() {
        chars += 1;
        if is_wide(c) {
            wide += 1;
        }
    }
    if chars == 0 {
        return 0;
    }
    chars.div_ceil(4).max(wide).max(1) + 2
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_identical_text() {
        let plan = plan(&["Hello".into(), "Hello".into(), "World".into()], &DEFAULT_LIMITS).expect("plan");
        assert_eq!(plan.unique, vec!["Hello", "World"]);
        assert_eq!(plan.mapping, vec![0, 0, 1]);
        assert_eq!(plan.batches, vec![vec![0, 1]]);
    }

    #[test]
    fn refuses_too_many_cells_before_allocating() {
        let limits = Limits { max_cells: 2, ..DEFAULT_LIMITS };
        let err = plan(&["a".into(), "b".into(), "c".into()], &limits).unwrap_err();
        assert!(matches!(err, TranslateError::TooManyCells { count: 3, limit: 2 }));
    }

    #[test]
    fn refuses_an_oversized_cell_and_reports_its_index() {
        let limits = Limits { max_cell_chars: 3, ..DEFAULT_LIMITS };
        let err = plan(&["ok".into(), "toolong".into()], &limits).unwrap_err();
        assert!(matches!(err, TranslateError::CellTooLong { index: 1, .. }));
    }

    #[test]
    fn splits_by_token_budget() {
        let limits = Limits { max_tokens: 30, ..DEFAULT_LIMITS };
        let texts: Vec<String> = (0..10).map(|i| format!("sentence number {i} with several words in it")).collect();
        let plan = plan(&texts, &limits).expect("plan");
        assert!(plan.batches.len() > 1);
        for batch in &plan.batches {
            let tokens: usize = batch.iter().map(|&i| estimate_tokens(&plan.unique[i])).sum();
            assert!(tokens <= limits.max_tokens);
        }
        // Every unique text appears exactly once.
        let mut seen: Vec<usize> = plan.batches.iter().flatten().copied().collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..plan.unique.len()).collect::<Vec<_>>());
    }

    #[test]
    fn estimates_tokens_for_latin_and_cjk() {
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("Hello world") >= 3);
        assert!(estimate_tokens("你好世界") >= 4);
    }
}
