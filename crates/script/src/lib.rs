//! Office Scripts for GridCraft: the part that does not need a JavaScript runtime.
//!
//! Office Scripts are TypeScript programs that drive a workbook through Microsoft's
//! `ExcelScript` object model. GridCraft stores them next to the application's settings
//! ([`store`]), checks them for obvious syntax damage without running them ([`syntax`]),
//! speaks the parent's half of the bridge ([`bridge`]) and turns recorded journal entries into
//! a script ([`recorder`]). The runtime itself is a host: [`host::ScriptHost`] is implemented by
//! the native crate that discovers and drives Bun, and by [`host::FakeHost`] in tests.
//!
//! The type names (`Workbook`, `Worksheet`, `Range`) follow the documented Office Scripts
//! object model; no Microsoft code is used. See `docs/office-scripts.md` for the supported
//! subset and limits.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod bridge;
pub mod coverage;
pub mod host;
pub mod osts;
pub mod recorder;
pub mod store;
pub mod syntax;

pub use bridge::{BridgeCall, BridgeError, ErrorCode, Request, Response, ScriptError};
pub use host::{BridgeExecutor, Limits, RunReport, RuntimeInfo, ScriptHost};
pub use osts::{OstsError, OstsFile, OstsMeta, RecordMeta};
pub use store::{ScriptRecord, ScriptStore, StoreError};
pub use syntax::SyntaxError;

/// Longest script accepted, in characters. Office Scripts allow 100,000 characters, and so do
/// we, so a script copied from Excel's editor fits.
pub const MAX_CODE_CHARS: usize = 100_000;
/// Longest script name accepted, in characters.
pub const MAX_NAME_CHARS: usize = 120;
/// Most scripts the store keeps.
pub const MAX_SCRIPTS: usize = 500;
/// Longest script id accepted, in bytes. Ids are `[a-z0-9-]` only.
pub const MAX_ID_LEN: usize = 64;
/// Where Bun is downloaded from when the user asks for it in the Office Scripts pane.
pub const BUN_DOWNLOAD_URL: &str = "https://bun.sh";
