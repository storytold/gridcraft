//! GridCraft file formats: XLSX (transitional and strict) and CSV import/export, plus XLSB and ODS data import
//! and legacy Excel 97-2003 `.xls` (BIFF8) import, and password-encrypted workbooks (Agile and Standard encryption).
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
mod crypto;
mod csv;
mod drawing;
mod feature_bags;
mod fmla;
mod ods;
mod package;
mod passthrough;
mod pivot;
mod read;
mod richdata;
mod sheet_read;
mod sheet_write;
mod slicer;
mod styles;
mod tables;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
mod theme;
mod write;
mod xls;
mod xlsb;
mod xml;

#[cfg(test)]
mod tests;

use gridcraft_core::DateSystem;
use gridcraft_model::Workbook;

pub use crypto::{DecryptError, check_encryption, decrypt};
pub use csv::{MAX_CSV_BYTES, read_csv, write_csv, write_csv_limited};
pub use ods::read_ods;
pub use package::MAX_PART;
pub use passthrough::rebase_links;
pub use xls::read_xls;
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
}

impl ReadReport {
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        if self.warnings.len() < 1000 && !self.warnings.contains(&m) {
            self.warnings.push(m);
        }
    }
}

/// Reads an `.xlsx` / `.xlsm` / `.xltx` / `.xltm` package (the workbook part is found through the
/// package relationships, so every main-part content type is accepted).
pub fn read_xlsx(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    read::read_xlsx(bytes)
}

/// The part of a zip package that declares more than [`MAX_PART`] unpacked bytes, with that size:
/// what an [`IoError::TooLarge`] from reading the package is about when one part is too large.
pub fn oversized_part(bytes: &[u8]) -> Option<(String, u64)> {
    package::oversized_part(bytes)
}

/// Which member of the SpreadsheetML family a package is saved as. Only the content type of the
/// workbook part differs (ECMA-376 Part 1 Workbook part; MS-OI29500 adds the macro-enabled types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum XlsxKind {
    /// `.xlsx`
    #[default]
    Workbook,
    /// `.xlsm`
    MacroWorkbook,
    /// `.xltx`
    Template,
    /// `.xltm`
    MacroTemplate,
}

impl XlsxKind {
    /// The kind a file name's extension asks for; anything else is a plain workbook.
    pub fn from_path(path: &str) -> XlsxKind {
        let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        match ext.as_str() {
            "xlsm" => XlsxKind::MacroWorkbook,
            "xltx" => XlsxKind::Template,
            "xltm" => XlsxKind::MacroTemplate,
            _ => XlsxKind::Workbook,
        }
    }

    /// Content type of `/xl/workbook.xml`.
    pub fn main_content_type(self) -> &'static str {
        match self {
            XlsxKind::Workbook => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            XlsxKind::MacroWorkbook => "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
            XlsxKind::Template => "application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml",
            XlsxKind::MacroTemplate => "application/vnd.ms-excel.template.macroEnabled.main+xml",
        }
    }

    /// Whether this kind may carry a VBA project (`.xlsm`, `.xltm`).
    pub fn is_macro_enabled(self) -> bool {
        matches!(self, XlsxKind::MacroWorkbook | XlsxKind::MacroTemplate)
    }
}

/// Writes a workbook as an `.xlsx` package, or as an `.xlsm` one when it holds a VBA project.
pub fn write_xlsx(wb: &Workbook) -> Result<Vec<u8>, IoError> {
    write::write_xlsx(wb, if wb.vba_project.is_some() { XlsxKind::MacroWorkbook } else { XlsxKind::Workbook })
}

/// Writes a workbook as the given member of the `.xlsx` family.
pub fn write_xlsx_as(wb: &Workbook, kind: XlsxKind) -> Result<Vec<u8>, IoError> {
    write::write_xlsx(wb, kind)
}

/// Writes a workbook as the given member of the `.xlsx` family for the file `book_name` (a bare
/// file name, used for the workbook in a PivotChart's link to its PivotTable).
pub fn write_xlsx_named(wb: &Workbook, kind: XlsxKind, book_name: &str) -> Result<Vec<u8>, IoError> {
    write::write_xlsx_named(wb, kind, Some(book_name))
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
    /// An Excel 97-2003 `.xls` workbook: a compound file with a `Workbook` (or BIFF5 `Book`) stream.
    Xls,
    /// Another compound (CFB/OLE2) file: a legacy Office binary that isn't an Excel workbook.
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
    // stream, an Excel 97-2003 workbook when it holds a workbook stream, otherwise another
    // legacy binary (.doc…).
    if bytes.starts_with(&cfb::SIGNATURE) {
        return if cfb::is_encrypted(bytes) {
            Format::Encrypted
        } else if xls::is_xls(bytes) {
            Format::Xls
        } else {
            Format::LegacyBinary
        };
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
