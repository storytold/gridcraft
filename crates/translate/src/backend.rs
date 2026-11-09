//! The inference seam. A backend translates a batch of texts for one language pair. It must be
//! `Send + Sync` (the engine runs it on a worker thread) and must not panic.
//!
//! Native backends load a user-downloaded model; this crate never does I/O. A backend reports the
//! accelerator it actually uses so the pane can say `cpu` (or, later, `openvino`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::accelerator::Accelerator;
use crate::tag::LangTag;

/// One model call: all texts are in the same source language.
#[derive(Clone, Debug)]
pub struct BatchRequest<'a> {
    /// `None` lets the model detect the source (the pane's Auto-detect).
    pub from: Option<&'a LangTag>,
    pub to: &'a LangTag,
    pub texts: &'a [String],
}

/// Why a backend could not translate a batch. Messages name the model or the shape of the
/// problem, never the text being translated.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    #[error("unsupported language pair: {from} to {to}")]
    UnsupportedPair { from: String, to: String },
    #[error("the translation was cancelled")]
    Cancelled,
    #[error("the translation timed out")]
    Timeout,
    #[error("model error: {0}")]
    Model(String),
    #[error("invalid batch: {0}")]
    Input(String),
    #[error("inference error: {0}")]
    Internal(String),
}

/// Why a cancellation happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelReason {
    User,
    Timeout,
}

impl CancelReason {
    fn code(self) -> u8 {
        match self {
            CancelReason::User => 1,
            CancelReason::Timeout => 2,
        }
    }

    fn from_code(code: u8) -> Option<CancelReason> {
        match code {
            1 => Some(CancelReason::User),
            2 => Some(CancelReason::Timeout),
            _ => None,
        }
    }
}

/// A cooperative cancellation token, shared between the job and its worker. The first cancellation
/// wins, so a timeout that races a user's Cancel still reports which one ended the job.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicU8>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Cancels because the person asked.
    pub fn cancel(&self) {
        self.cancel_with(CancelReason::User);
    }

    /// Cancels with a reason; the first reason recorded is kept.
    pub fn cancel_with(&self, reason: CancelReason) {
        let _ = self.0.compare_exchange(0, reason.code(), Ordering::AcqRel, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) != 0
    }

    pub fn reason(&self) -> Option<CancelReason> {
        CancelReason::from_code(self.0.load(Ordering::Acquire))
    }

    /// `Ok` until cancelled, then the matching backend error.
    pub fn check(&self) -> Result<(), BackendError> {
        match self.reason() {
            None => Ok(()),
            Some(CancelReason::Timeout) => Err(BackendError::Timeout),
            Some(CancelReason::User) => Err(BackendError::Cancelled),
        }
    }
}

/// A translation engine. Implementations are injected into the session by the host (the desktop
/// app or the CLI), never constructed by the engine itself.
pub trait Backend: Send + Sync {
    /// The manifest id of the loaded model (`qwen3-1.7b`).
    fn model_id(&self) -> &str;

    /// Where it runs. CPU today.
    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }

    /// Whether this model has this directed pair, ignoring region and equal scripts.
    fn supports(&self, from: &LangTag, to: &LangTag) -> bool;

    /// Whether this model can translate into `to` without being told the source. Large
    /// multilingual models can; the small fixed test backend cannot.
    fn supports_target(&self, to: &LangTag) -> bool {
        let _ = to;
        false
    }

    /// Whether `from: auto` may reach the model instead of being resolved by the detector first.
    fn auto_source(&self) -> bool {
        false
    }

    /// Translates `req.texts` in order. Returns one string per input; the caller verifies the
    /// count. Check `cancel` between texts and between decoder steps.
    fn translate(&self, req: &BatchRequest<'_>, cancel: &Cancel) -> Result<Vec<String>, BackendError>;
}

/// The transport for a user-initiated model download. The host injects one (the desktop app's
/// HTTPS client); the engine never creates one, so the web build and headless tests have no
/// network path. `fetch` writes the file to `dest` and must refuse a body larger than
/// `max_bytes` (the manifest's exact size for that file); the caller verifies size and SHA-256
/// before the staging folder is renamed into place. Streaming instead of returning a `Vec<u8>`
/// keeps a 1.8 GB model from being held in memory twice. URLs come from the shipped manifest,
/// never from a command parameter.
pub trait ModelFetcher: Send + Sync {
    fn fetch(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_keeps_the_first_reason() {
        let c = Cancel::new();
        assert!(!c.is_cancelled());
        c.cancel_with(CancelReason::Timeout);
        c.cancel();
        assert_eq!(c.reason(), Some(CancelReason::Timeout));
        assert_eq!(c.check(), Err(BackendError::Timeout));
    }

    #[test]
    fn cancellation_is_shared_by_clones() {
        let c = Cancel::new();
        let other = c.clone();
        other.cancel();
        assert!(c.is_cancelled());
        assert_eq!(c.check(), Err(BackendError::Cancelled));
    }
}
