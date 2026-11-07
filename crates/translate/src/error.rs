//! Structured errors for the translation pipeline. Messages describe the *shape* of the input
//! (an index, a count, a tag); they never contain cell text.

/// Why a translation request could not be completed at all (as opposed to one cell failing).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("unsupported language pair: {from} to {to}")]
    UnsupportedPair { from: String, to: String },
    #[error("the request has {count} cells; the limit is {limit}")]
    TooManyCells { count: usize, limit: usize },
    #[error("cell {index} has {count} characters; the limit is {limit}")]
    CellTooLong { index: usize, count: usize, limit: usize },
    #[error("the request has {count} characters; the limit is {limit}")]
    TooManyChars { count: usize, limit: usize },
    #[error("the request needs {tokens} model tokens in one batch; the limit is {limit}")]
    TooManyTokens { tokens: usize, limit: usize },
    #[error("a model or backend limit was exceeded")]
    LimitExceeded,
    #[error("the translation was cancelled")]
    Cancelled,
    #[error("the translation timed out")]
    Timeout,
    #[error("no translation model is available")]
    NoModel,
    #[error("{0}")]
    Backend(String),
    #[error("{0}")]
    InvalidRequest(String),
}

impl From<crate::backend::BackendError> for TranslateError {
    fn from(e: crate::backend::BackendError) -> Self {
        use crate::backend::BackendError as B;
        match e {
            B::Cancelled => TranslateError::Cancelled,
            B::Timeout => TranslateError::Timeout,
            B::UnsupportedPair { from, to } => TranslateError::UnsupportedPair { from, to },
            other => TranslateError::Backend(other.to_string()),
        }
    }
}
