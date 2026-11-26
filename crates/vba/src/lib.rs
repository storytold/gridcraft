//! Reads VBA projects (`vbaProject.bin`) from their public specifications ([MS-OVBA], later
//! [MS-VBAL]). Read-only: nothing here runs VBA.
//!
//! The crate depends on `gridcraft-core` only, so it stays small, wasm-safe and usable outside
//! the workbook stack. It never opens the compound file itself: callers hand it the streams.
//!
//! [MS-OVBA]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-ovba/
//! [MS-VBAL]: https://learn.microsoft.com/en-us/openspecs/microsoft_general_purpose_programming_languages/ms-vbal/
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
pub mod ovba;
