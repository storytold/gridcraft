//! What a cell is, and the rules that keep formulas, numbers, dates, booleans, errors and empty
//! cells out of translation. The engine maps a model cell to a [`ValueKind`]; the rules live here
//! so the CLI, tests and the pane agree.

use serde::Serialize;

use crate::batch::Limits;

/// What kind of value a cell holds, before any text is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    Number,
    /// A numeric date or time (a number with a date/time number format).
    Date,
    Boolean,
    Error,
    /// A formula or a Python cell. Never translated.
    Formula,
    Empty,
}

/// Why a cell was left out. The strings are stable JSON values, not UI labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    Formula,
    Number,
    Date,
    Boolean,
    Error,
    Empty,
    TooLong,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::Formula => "formula",
            SkipReason::Number => "number",
            SkipReason::Date => "date",
            SkipReason::Boolean => "boolean",
            SkipReason::Error => "error",
            SkipReason::Empty => "empty",
            SkipReason::TooLong => "tooLong",
        }
    }
}

/// The skip rule for a cell. `text` is the cell's stored text; it is used for the empty and
/// length rules only, and never copied into a result.
pub fn skip_reason(kind: ValueKind, text: &str, limits: &Limits) -> Option<SkipReason> {
    match kind {
        ValueKind::Formula => Some(SkipReason::Formula),
        ValueKind::Number => Some(SkipReason::Number),
        ValueKind::Date => Some(SkipReason::Date),
        ValueKind::Boolean => Some(SkipReason::Boolean),
        ValueKind::Error => Some(SkipReason::Error),
        ValueKind::Empty => Some(SkipReason::Empty),
        ValueKind::Text => {
            if text.trim().is_empty() {
                Some(SkipReason::Empty)
            } else if text.chars().count() > limits.max_cell_chars {
                Some(SkipReason::TooLong)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::DEFAULT_LIMITS;

    #[test]
    fn non_text_kinds_are_skipped_with_their_reason() {
        assert_eq!(skip_reason(ValueKind::Formula, "=SUM(A1:A2)", &DEFAULT_LIMITS), Some(SkipReason::Formula));
        assert_eq!(skip_reason(ValueKind::Number, "42", &DEFAULT_LIMITS), Some(SkipReason::Number));
        assert_eq!(skip_reason(ValueKind::Date, "2026-01-02", &DEFAULT_LIMITS), Some(SkipReason::Date));
        assert_eq!(skip_reason(ValueKind::Boolean, "TRUE", &DEFAULT_LIMITS), Some(SkipReason::Boolean));
        assert_eq!(skip_reason(ValueKind::Error, "#N/A", &DEFAULT_LIMITS), Some(SkipReason::Error));
        assert_eq!(skip_reason(ValueKind::Empty, "", &DEFAULT_LIMITS), Some(SkipReason::Empty));
    }

    #[test]
    fn empty_text_is_empty_not_translatable() {
        assert_eq!(skip_reason(ValueKind::Text, "   \n\t", &DEFAULT_LIMITS), Some(SkipReason::Empty));
        assert_eq!(skip_reason(ValueKind::Text, "hello", &DEFAULT_LIMITS), None);
    }

    #[test]
    fn oversized_text_is_skipped() {
        let limits = Limits { max_cell_chars: 4, ..DEFAULT_LIMITS };
        assert_eq!(skip_reason(ValueKind::Text, "hello", &limits), Some(SkipReason::TooLong));
        assert_eq!(skip_reason(ValueKind::Text, "hi", &limits), None);
    }

    #[test]
    fn reasons_serialise_as_camel_case() {
        assert_eq!(serde_json::to_string(&SkipReason::TooLong).unwrap_or_default(), "\"tooLong\"");
    }
}
