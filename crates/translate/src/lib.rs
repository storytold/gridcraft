//! GridCraft content translation: the pure half of Review › Translate.
//!
//! This crate holds everything that does not touch a file, a network or a thread: BCP-47
//! language tags, a heuristic language detector, segmenting and protecting text, batch planning,
//! the skip rules for cell kinds, the model manifest with each model's licence and hashes, and
//! the [`Backend`] seam the native inference crate implements.
//!
//! The design rules are the feature's privacy rules: nothing here logs or formats cell text into
//! an error, every limit is enforced before allocating, and no function panics on hostile input.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod accelerator;
pub mod backend;
pub mod batch;
pub mod detect;
pub mod error;
pub mod manifest;
pub mod pipeline;
pub mod protect;
pub mod segment;
pub mod skip;
pub mod tag;

pub use accelerator::Accelerator;
pub use backend::{Backend, BackendError, BatchRequest, Cancel, CancelReason, ModelFetcher};
pub use batch::{DEFAULT_LIMITS, Limits};
pub use detect::{Detection, detect};
pub use error::TranslateError;
pub use manifest::{LanguagePair, MANIFESTS, ModelFile, ModelManifest, find as find_model, total_bytes};
pub use pipeline::{CellResult, CellStatus, Options, Report, run};
pub use protect::{Protected, protect, restore};
pub use segment::{Segment, SegmentKind, segment};
pub use skip::{SkipReason, ValueKind, skip_reason};
pub use tag::LangTag;

#[cfg(any(test, feature = "test-backend"))]
mod test_backend;

#[cfg(any(test, feature = "test-backend"))]
pub use test_backend::FixedBackend;

/// The language a command means when it says `auto`: detection, not a tag.
pub const AUTO: &str = "auto";
