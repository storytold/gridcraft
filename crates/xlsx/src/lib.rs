//! GridCraft file formats: XLSX (transitional and strict) and CSV import/export, plus XLSB and ODS data import.
//!
//! Clean-room implementation from ECMA-376 / ISO/IEC 29500 and observed behaviour. All entry
//! points work on byte slices (no file system access), so the crate builds for WebAssembly.
//!
//! Reading is lenient: unsupported or broken optional parts are skipped and reported in
//! [`ReadReport::warnings`]; only a missing/invalid package or workbook part is an error. Hostile
//! input is bounded: decompressed parts are capped at 512 MB (1 GB per package), cells outside
//! Excel's grid are dropped and XML nesting is limited.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod array_formula;
mod cfb;
mod chart;
mod chartex;
mod csv;
mod drawing;
mod fmla;
mod ods;
mod package;
mod pivot;
mod read;
mod richdata;
mod sheet_read;
mod sheet_write;
mod styles;
mod tables;
mod theme;
mod write;
mod xlsb;
mod xml;

#[cfg(test)]
mod tests;

use gridcraft_core::DateSystem;
use gridcraft_model::Workbook;

pub use csv::{read_csv, write_csv};
pub use ods::read_ods;
pub use xlsb::read_xlsb;

/// Errors from reading or writing files.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("the file is not a valid zip package: {0}")]
    Zip(String),
    #[error("the file contains invalid XML: {0}")]
    Xml(String),
    #[error("the file is not a valid spreadsheet: {0}")]
    Format(String),
    #[error("the file is too large: {0}")]
    TooLarge(String),
}

/// What a lenient read skipped or repaired.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadReport {
    pub warnings: Vec<String>,
    /// What to recalculate after loading. `None`: everything (the file asks for it with
    /// `fullCalcOnLoad`, or has no calculation properties). Otherwise the file's cached values
    /// stand, as in Excel, except for these formulas (sheet index, cell): those stored without a
    /// value, and array formulas whose other cells' values aren't kept.
    pub recalc: Option<Vec<(usize, gridcraft_core::CellRef)>>,
}

impl ReadReport {
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        if self.warnings.len() < 1000 && !self.warnings.contains(&m) {
            self.warnings.push(m);
        }
    }
}

/// Reads an `.xlsx` / `.xlsm` package.
pub fn read_xlsx(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    read::read_xlsx(bytes)
}

/// Writes a workbook as an `.xlsx` package.
pub fn write_xlsx(wb: &Workbook) -> Result<Vec<u8>, IoError> {
    write::write_xlsx(wb)
}

/// CSV import options.
#[derive(Clone, Debug, PartialEq)]
pub struct CsvOptions {
    /// Field separator; `0` auto-detects comma, semicolon or tab.
    pub delimiter: u8,
    pub quote: u8,
    pub date_system: DateSystem,
    /// Interpret fields like typed input (numbers, dates, booleans…); otherwise keep text.
    pub parse_values: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        CsvOptions { delimiter: b',', quote: b'"', date_system: DateSystem::D1900, parse_values: true }
    }
}

/// A file format guessed from content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Xlsx,
    Xlsb,
    Ods,
    Csv,
    /// A password-protected (encrypted) workbook: a compound file with an `EncryptedPackage`.
    Encrypted,
    /// Another compound (CFB/OLE2) file: an Excel 97-2003 `.xls` or other legacy Office binary.
    LegacyBinary,
    Unknown,
}

/// Guesses the format from content: ODS mimetype, XLSB package content types, other ZIP packages as XLSX, text as CSV.
pub fn sniff(bytes: &[u8]) -> Format {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08") {
        return if ods::has_mimetype(bytes) {
            Format::Ods
        } else if xlsb::is_xlsb(bytes) {
            Format::Xlsb
        } else {
            Format::Xlsx
        };
    }
    // CFB/OLE2 compound file: a password-protected workbook when it holds an encryption
    // stream, otherwise a legacy binary (.xls, .doc…).
    if bytes.starts_with(&cfb::SIGNATURE) {
        return if cfb::is_encrypted(bytes) { Format::Encrypted } else { Format::LegacyBinary };
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Format::Csv;
    }
    if bytes.is_empty() {
        return Format::Unknown;
    }
    let head = bytes.get(..bytes.len().min(4096)).unwrap_or(bytes);
    // Binary files (old .xls, images…) contain NULs and many control characters.
    let control = head.iter().filter(|&&b| b < 0x09 || (0x0E..0x20).contains(&b) || b == 0x7F).count();
    if head.contains(&0) || control * 20 > head.len() {
        return Format::Unknown;
    }
    Format::Csv
}
