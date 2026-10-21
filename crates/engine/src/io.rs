//! Opening and saving files: XLSX, legacy XLS import, XLSB and ODS data import, CSV/TSV, JSON (debug), HTML export.

use std::fmt::Write as _;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{CsvEncoding, Sheet, Workbook};

use crate::{EngineError, Result};

/// File formats GridCraft reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Xlsx,
    /// Excel 97-2003 workbook (import only).
    Xls,
    Xlsb,
    Ods,
    Csv,
    Tsv,
    Json,
    Html,
}

impl FileKind {
    pub fn from_path(path: &str) -> Option<FileKind> {
        let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "xlsx" | "xlsm" | "xltx" | "xltm" => FileKind::Xlsx,
            "xls" | "xlt" => FileKind::Xls,
            "xlsb" => FileKind::Xlsb,
            "ods" => FileKind::Ods,
            "csv" => FileKind::Csv,
            "tsv" | "tab" | "txt" => FileKind::Tsv,
            "json" | "scjson" => FileKind::Json,
            "html" | "htm" => FileKind::Html,
            _ => return None,
        })
    }
}

/// The error for an encrypted workbook opened without a password contains this. The interface
/// asks for the password when it sees it; programmatic callers pass `password` instead.
pub const PASSWORD_REQUIRED: &str = "is password-protected. Enter its password to open it.";
/// The error for a wrong password contains this.
pub const WRONG_PASSWORD: &str = "The password is incorrect.";
/// The first open warning of a workbook decrypted with its password.
pub const DECRYPTED_NOTICE: &str = "This workbook was password-protected. GridCraft saves it without the password.";

/// Parses a file's bytes into a workbook. `name` picks the format by extension when the bytes
/// don't say. An encrypted workbook needs [`open_bytes_with_password`].
pub fn open_bytes(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    open_bytes_with_password(name, bytes, None)
}

/// [`open_bytes`], first decrypting a password-protected workbook with `password`. Errors never
/// quote the password.
pub fn open_bytes_with_password(name: &str, bytes: &[u8], password: Option<&str>) -> Result<(Workbook, Vec<String>)> {
    open_bytes_with_encoding(name, bytes, password, false, CsvEncoding::Auto)
}

/// [`open_bytes_with_password`], and with `csv_as_text` a CSV/TSV/TXT file keeps every field as
/// the text it holds, with no number, date or boolean conversion (identifiers such as GTINs, SKUs
/// and ZIP codes survive). It does not apply to other formats.
pub fn open_bytes_with(name: &str, bytes: &[u8], password: Option<&str>, csv_as_text: bool) -> Result<(Workbook, Vec<String>)> {
    open_bytes_with_encoding(name, bytes, password, csv_as_text, CsvEncoding::Auto)
}

/// [`open_bytes_with`] in a named CSV encoding; the workbook remembers the one the file turned out
/// to be in, so a plain save writes it back.
pub fn open_bytes_encoded(name: &str, bytes: &[u8], password: Option<&str>, encoding: CsvEncoding) -> Result<(Workbook, Vec<String>)> {
    open_bytes_with_encoding(name, bytes, password, false, encoding)
}

/// [`open_bytes_with_password`], with `csv_as_text` and the CSV/TSV `encoding` both applied.
pub fn open_bytes_with_encoding(
    name: &str,
    bytes: &[u8],
    password: Option<&str>,
    csv_as_text: bool,
    encoding: CsvEncoding,
) -> Result<(Workbook, Vec<String>)> {
    if gridcraft_xlsx::sniff(bytes) != gridcraft_xlsx::Format::Encrypted {
        let (mut wb, warnings) = open_plain(name, bytes, csv_as_text, encoding)?;
        fit_imported_rows(&mut wb);
        return Ok((wb, warnings));
    }
    // A scheme no password opens is said straight away, rather than after asking for one.
    let decrypt_error = |e: gridcraft_xlsx::DecryptError| EngineError::Other(decrypt_message(name, e));
    let Some(password) = password else {
        gridcraft_xlsx::check_encryption(bytes).map_err(decrypt_error)?;
        return Err(EngineError::Other(format!("'{name}' {PASSWORD_REQUIRED}")));
    };
    let plain = gridcraft_xlsx::decrypt(bytes, password).map_err(decrypt_error)?;
    if gridcraft_xlsx::sniff(&plain) == gridcraft_xlsx::Format::Encrypted {
        return Err(EngineError::Other(format!("We can't open '{name}': the encrypted file is damaged (it holds another encrypted file).")));
    }
    let (mut wb, mut warnings) = open_plain(name, &plain, csv_as_text, encoding)?;
    fit_imported_rows(&mut wb);
    warnings.insert(0, DECRYPTED_NOTICE.into());
    Ok((wb, warnings))
}

/// Excel auto-fits rows to their tallest font when opening a file: rows without a user-set
/// height grow to fit their content. Rows the file made taller keep their height — our
/// line-height estimate can differ from Excel's, and shrinking files that already look
/// right would be a visible regression. Custom-height rows are left alone entirely.
fn fit_imported_rows(wb: &mut Workbook) {
    for si in 0..wb.sheets.len() {
        let Some(sh) = wb.sheet(si) else { continue };
        let mut rows: Vec<u32> = sh.cells.iter().map(|(c, _)| c.row).collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            let before = wb.sheet(si).and_then(|sh| sh.rows.get(&row)).and_then(|i| i.size);
            crate::cmd::auto_row_height(wb, si, row);
            let Some(before) = before else { continue };
            let after = wb.sheet(si).and_then(|sh| sh.rows.get(&row)).and_then(|i| i.size);
            if after.is_none_or(|a| before > a + 0.5)
                && let Some(sh) = wb.sheet_mut(si)
            {
                sh.rows.entry(row).or_default().size = Some(before);
            }
        }
    }
}

/// What to tell the user when an encrypted workbook can't be decrypted. Never quotes the password.
fn decrypt_message(name: &str, e: gridcraft_xlsx::DecryptError) -> String {
    match e {
        gridcraft_xlsx::DecryptError::WrongPassword => format!("{WRONG_PASSWORD} Check that Caps Lock is off and try again."),
        gridcraft_xlsx::DecryptError::Unsupported(scheme) => {
            format!("'{name}' uses {scheme} encryption, which GridCraft can't open yet. Remove the password in Excel and try again.")
        }
        gridcraft_xlsx::DecryptError::Malformed(detail) => format!("We can't open '{name}': the encrypted file is damaged ({detail})."),
    }
}

/// Parses bytes that aren't encrypted, reading a delimited text file in `encoding`.
fn open_plain(name: &str, bytes: &[u8], csv_as_text: bool, encoding: CsvEncoding) -> Result<(Workbook, Vec<String>)> {
    let kind = FileKind::from_path(name);
    let sniffed = gridcraft_xlsx::sniff(bytes);
    if sniffed == gridcraft_xlsx::Format::Xls {
        let (wb, report) = gridcraft_xlsx::read_xls(bytes).map_err(|e| EngineError::Other(format!("We can't open '{name}': {e}")))?;
        return Ok((wb, report.warnings));
    }
    if sniffed == gridcraft_xlsx::Format::LegacyBinary {
        return Err(EngineError::Other(format!(
            "'{name}' is a legacy binary file but not an Excel workbook (no workbook stream). GridCraft can't open it."
        )));
    }
    // Content wins over the extension: an ODS or XLSB package is imported as what it is,
    // whatever it is called; the extension only decides when the bytes don't say.
    let import = match sniffed {
        gridcraft_xlsx::Format::Ods => Some(FileKind::Ods),
        gridcraft_xlsx::Format::Xlsb => Some(FileKind::Xlsb),
        // An XLSX package is never read as ODS/XLSB just because of its name.
        gridcraft_xlsx::Format::Xlsx => None,
        _ => kind.filter(|k| matches!(k, FileKind::Ods | FileKind::Xlsb)),
    };
    if let Some(import) = import {
        let read = if import == FileKind::Ods { gridcraft_xlsx::read_ods } else { gridcraft_xlsx::read_xlsb };
        let (wb, report) = read(bytes).map_err(|e| EngineError::Other(format!("We can't import '{name}': {e}")))?;
        return Ok((wb, report.warnings));
    }
    // Text named `.xlsx` is read as text.
    if sniffed == gridcraft_xlsx::Format::Xlsx || (kind == Some(FileKind::Xlsx) && sniffed != gridcraft_xlsx::Format::Csv) {
        let (wb, report) = gridcraft_xlsx::read_xlsx(bytes).map_err(|e| EngineError::Other(format!("We can't open '{name}': {e}")))?;
        return Ok((wb, report.warnings));
    }
    match kind {
        Some(FileKind::Json) => {
            let mut wb: Workbook = serde_json::from_slice(bytes).map_err(|e| EngineError::Other(format!("not a GridCraft JSON workbook: {e}")))?;
            wb.styles.rebuild_index();
            if wb.sheets.is_empty() {
                wb = Workbook::new();
            }
            Ok((wb, vec![]))
        }
        Some(FileKind::Tsv) => {
            let opts = gridcraft_xlsx::CsvOptions { delimiter: b'\t', parse_values: !csv_as_text, encoding, ..Default::default() };
            let mut wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            // Tab is what makes it a .tsv/.txt; saved as .csv it gets the CSV default.
            wb.csv_delimiter = None;
            rename_first_sheet(&mut wb, name);
            Ok((wb, vec![]))
        }
        _ => {
            let opts = gridcraft_xlsx::CsvOptions { delimiter: 0, parse_values: !csv_as_text, encoding, ..Default::default() };
            let mut wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            rename_first_sheet(&mut wb, name);
            Ok((wb, vec![]))
        }
    }
}

/// A CSV opens as a sheet named after the file (like Excel).
fn rename_first_sheet(wb: &mut Workbook, path: &str) {
    let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Sheet1");
    let clean: String = stem.chars().filter(|c| !matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']')).take(31).collect();
    let clean = clean.trim_matches('\'').to_string();
    // "History" is reserved (see `Workbook::check_sheet_name`): keep the default name instead.
    if !clean.is_empty()
        && !clean.eq_ignore_ascii_case("History")
        && let Some(sh) = wb.sheet_mut(0)
    {
        sh.name = clean;
    }
}

/// Encodes a workbook in the format chosen by `path`'s extension.
pub fn save_bytes(wb: &Workbook, path: &str) -> Result<Vec<u8>> {
    save_bytes_encoded(wb, path, None)
}

/// [`save_bytes`] with the delimited-file encoding this write uses: `None` writes the encoding
/// the workbook was opened with ([`Workbook::csv_encoding`]), else UTF-8 without a BOM.
pub fn save_bytes_encoded(wb: &Workbook, path: &str, encoding: Option<CsvEncoding>) -> Result<Vec<u8>> {
    // An unknown or missing extension is refused rather than written as an XLSX under that name:
    // a typo like `.cvs` would otherwise produce a file no other app recognises.
    let Some(kind) = FileKind::from_path(path) else {
        let name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
        return Err(EngineError::Other(format!("{name}: unknown file format. Save as .xlsx, .csv, .tsv, .json or .html.")));
    };
    save_bytes_as_encoded(wb, path, kind, encoding)
}

/// Encodes a workbook as `kind`, whatever `path`'s extension says.
pub fn save_bytes_as(wb: &Workbook, path: &str, kind: FileKind) -> Result<Vec<u8>> {
    save_bytes_as_encoded(wb, path, kind, None)
}

/// [`save_bytes_as`] with the delimited-file encoding for this write; `None` writes
/// [`Workbook::csv_encoding`] (UTF-8 without a BOM when the workbook has none).
pub fn save_bytes_as_encoded(wb: &Workbook, path: &str, kind: FileKind, encoding: Option<CsvEncoding>) -> Result<Vec<u8>> {
    let sheet = wb.active_sheet;
    let encoding = encoding.or(wb.csv_encoding).unwrap_or(CsvEncoding::Utf8);
    match kind {
        // The extension picks the package kind; only the macro-enabled kinds (.xlsm, .xltm) carry
        // the VBA project, so any other name saves without it, as Excel does.
        FileKind::Xlsx => gridcraft_xlsx::write_xlsx_as(wb, gridcraft_xlsx::XlsxKind::from_path(path)).map_err(|e| EngineError::Other(e.to_string())),
        FileKind::Xls => {
            Err(EngineError::Other("Excel 97-2003 (.xls) is supported for import only. Save as .xlsx or another supported export format.".into()))
        }
        FileKind::Xlsb => Err(EngineError::Other("XLSB is supported for data import only. Save as .xlsx or another supported export format.".into())),
        FileKind::Ods => Err(EngineError::Other("ODS is supported for data import only. Save as .xlsx or another supported export format.".into())),
        // A workbook read from CSV keeps the separator and the encoding it was read with (a `;`
        // Shift-JIS file stays a `;` Shift-JIS file).
        FileKind::Csv => wb.sheet(sheet).map(|sh| csv(wb, sh, wb.csv_delimiter.unwrap_or(b','), encoding)).transpose().map(Option::unwrap_or_default),
        FileKind::Tsv => wb.sheet(sheet).map(|sh| csv(wb, sh, b'\t', encoding)).transpose().map(Option::unwrap_or_default),
        FileKind::Json => serde_json::to_vec_pretty(wb).map_err(|e| EngineError::Other(e.to_string())),
        FileKind::Html => to_html(wb, sheet).map(String::into_bytes),
    }
}

/// [`save_bytes`] for the file at `path`: relative paths to linked workbooks are rewritten for
/// its folder, so they still name the same files.
pub fn save_file_bytes(wb: &Workbook, path: &str) -> Result<Vec<u8>> {
    match source_dir(path).and_then(|f| gridcraft_xlsx::rebase_links(wb, &f)) {
        Some(rebased) => save_bytes(&rebased, path),
        None => save_bytes(wb, path),
    }
}

/// The folder of the file at `path`, which relative paths to linked workbooks are relative to.
pub fn source_dir(path: &str) -> Option<std::path::PathBuf> {
    std::path::absolute(path).ok()?.parent().map(std::path::Path::to_path_buf)
}

/// Save As CSV/Text writes "the text and values as they are displayed in cells" with "all
/// characters in each cell" (Microsoft Support, *Excel formatting and features that are not
/// transferred to other file formats*): number formats applied, never column-width `#####`.
fn csv(wb: &Workbook, sh: &Sheet, delimiter: u8, encoding: CsvEncoding) -> Result<Vec<u8>> {
    gridcraft_xlsx::write_csv_encoded(sh, delimiter, encoding, |c| {
        // General's grid-width formatting rounds large identifiers to scientific notation.
        // Delimited files have no column width, so preserve the whole numeric value instead.
        if wb.styles.get(sh.style_id(c)).num_fmt.as_str().eq_ignore_ascii_case("General")
            && let gridcraft_core::Value::Number(number) = sh.value(c)
            && number.is_finite()
        {
            number.to_string()
        } else {
            crate::display::value_text(wb, sh, c)
        }
    })
    .map_err(|e| EngineError::Other(e.to_string()))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn html_cell_css(wb: &Workbook, sh: &Sheet, c: CellRef, dir: CellDir) -> String {
    let st = wb.styles.get(sh.style_id(c));
    let mut css = String::new();
    if st.font.bold {
        css.push_str("font-weight:bold;");
    }
    if st.font.italic {
        css.push_str("font-style:italic;");
    }
    if let Some(h) = st.font.color.hex(&wb.theme) {
        let _ = write!(css, "color:{h};");
    }
    if st.fill.pattern != gridcraft_model::PatternType::None
        && let Some(h) = st.fill.fg.hex(&wb.theme)
    {
        let _ = write!(css, "background:{h};");
    }
    let v = sh.value(c);
    let align = match st.align.h {
        gridcraft_model::HAlign::Center => Some("center"),
        gridcraft_model::HAlign::Right => Some("right"),
        gridcraft_model::HAlign::Left => Some("left"),
        _ if v.is_number() => Some(if sh.right_to_left { "left" } else { "right" }),
        _ if dir == CellDir::Auto => None,
        _ => Some("left"),
    };
    if let Some(align) = align {
        let _ = write!(css, "text-align:{align};");
    }
    css
}

/// How a cell marks its direction in HTML. Text on a right-to-left sheet, or text with
/// right-to-left letters, follows its first strong character (`dir="auto"`), so General
/// alignment puts Arabic and Hebrew on the right. Numbers on a right-to-left sheet stay left to
/// right so `-5` doesn't read `5-`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellDir {
    Inherit,
    Auto,
    Ltr,
}

impl CellDir {
    fn of(sh: &Sheet, c: CellRef, text: &str) -> CellDir {
        if sh.value(c).is_number() {
            if sh.right_to_left { CellDir::Ltr } else { CellDir::Inherit }
        } else if sh.right_to_left || text.chars().any(is_right_to_left_letter) {
            CellDir::Auto
        } else {
            CellDir::Inherit
        }
    }

    fn attr(self) -> &'static str {
        match self {
            CellDir::Inherit => "",
            CellDir::Auto => " dir=\"auto\"",
            CellDir::Ltr => " dir=\"ltr\"",
        }
    }
}

/// Hebrew, Arabic, Syriac, Thaana, N'Ko and their presentation forms.
fn is_right_to_left_letter(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

fn table_dir(sh: &Sheet) -> &'static str {
    if sh.right_to_left { " dir=\"rtl\"" } else { "" }
}

const CLIPBOARD_HTML_MAX_BYTES: usize = 4 * 1024 * 1024;
const CLIPBOARD_HTML_MAX_CELLS: u64 = 10_000;

/// An HTML fragment for the same range as `plain_text`, previously produced by
/// `display::range_text`. Large copies keep plain text only; never return a partial table.
pub(crate) fn range_html(wb: &Workbook, sheet: usize, r: RangeRef, plain_text: &str) -> Option<String> {
    let r = crate::display::clipboard_range(r);
    if r.count() > CLIPBOARD_HTML_MAX_CELLS || plain_text.len() > CLIPBOARD_HTML_MAX_BYTES {
        return None;
    }
    let sh = wb.sheet(sheet)?;
    let mut out = ClipboardHtml(String::new());
    write!(out, "<table{} style=\"border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt\">", table_dir(sh)).ok()?;
    for row in r.start.row..=r.end.row {
        if sh.is_row_hidden(row) {
            continue;
        }
        out.write_str("<tr>").ok()?;
        for col in r.start.col..=r.end.col {
            let c = CellRef::new(row, col);
            let span = if let Some(merged) = sh.merges.iter().find(|m| m.contains(c)).and_then(|m| m.intersection(&r)) {
                let first_row = (merged.start.row..=merged.end.row).find(|&row| !sh.is_row_hidden(row))?;
                if c != CellRef::new(first_row, merged.start.col) {
                    continue;
                }
                let rows = (first_row..=merged.end.row).filter(|&row| !sh.is_row_hidden(row)).count();
                format!(" rowspan=\"{rows}\" colspan=\"{}\"", merged.width())
            } else {
                String::new()
            };
            // Use the visible selected cell, even when the merge's original anchor
            // is outside the range or hidden. This matches the plain-text copy.
            let text = crate::display::cell_text(wb, sh, c);
            let dir = CellDir::of(sh, c, &text);
            let css = html_cell_css(wb, sh, c, dir);
            write!(out, "<td style=\"border:1px solid #d4d4d4;padding:2px 4px;white-space:pre-wrap;{css}\"{}{span}>", dir.attr()).ok()?;
            let mut chars = text.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '&' => out.write_str("&amp;"),
                    '<' => out.write_str("&lt;"),
                    '>' => out.write_str("&gt;"),
                    '"' => out.write_str("&quot;"),
                    '\r' => {
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        out.write_str("<br>")
                    }
                    '\n' => out.write_str("<br>"),
                    _ => out.write_char(ch),
                }
                .ok()?;
            }
            out.write_str("</td>").ok()?;
        }
        out.write_str("</tr>").ok()?;
    }
    out.write_str("</table>").ok()?;
    Some(out.0)
}

/// Enforce the budget while escaping, before HTML expansion can allocate a large string.
struct ClipboardHtml(String);

impl std::fmt::Write for ClipboardHtml {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        if s.len() > CLIPBOARD_HTML_MAX_BYTES.saturating_sub(self.0.len()) {
            return Err(std::fmt::Error);
        }
        self.0.push_str(s);
        Ok(())
    }
}

/// Most rows Save as Web Page writes. A sheet whose used range is taller fails instead of losing cells.
const HTML_EXPORT_MAX_ROWS: u32 = 100_001;

/// A sheet as a standalone HTML table (Save as Web Page).
pub fn to_html(wb: &Workbook, sheet: usize) -> Result<String> {
    let Some(sh) = wb.sheet(sheet) else { return Ok(String::new()) };
    let mut out = String::new();
    let _ = write!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>table{{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt}}td{{border:1px solid #d4d4d4;padding:2px 4px;white-space:nowrap}}</style></head><body><table{}>",
        esc(&sh.name),
        table_dir(sh)
    );
    if let Some(r) = sh.used_range() {
        if r.height() > HTML_EXPORT_MAX_ROWS {
            return Err(EngineError::Other(format!(
                "the sheet is too tall for a web page ({} rows, at most {HTML_EXPORT_MAX_ROWS}). Save as .xlsx or CSV instead.",
                r.height()
            )));
        }
        for row in r.start.row..=r.end.row {
            if sh.is_row_hidden(row) {
                continue;
            }
            out.push_str("<tr>");
            for col in r.start.col..=r.end.col {
                let c = CellRef::new(row, col);
                if sh.merges.iter().any(|m| m.contains(c) && m.start != c) {
                    continue;
                }
                let text = crate::display::cell_text(wb, sh, c);
                let dir = CellDir::of(sh, c, &text);
                let css = html_cell_css(wb, sh, c, dir);
                let span = sh
                    .merges
                    .iter()
                    .find(|m| m.start == c)
                    .map(|m| format!(" rowspan=\"{}\" colspan=\"{}\"", m.height(), m.width()))
                    .unwrap_or_default();
                let _ = write!(out, "<td style=\"{css}\"{}{span}>{}</td>", dir.attr(), esc(&text));
            }
            out.push_str("</tr>");
        }
    }
    out.push_str("</table></body></html>");
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if meta.len() > 2 * 1024 * 1024 * 1024 {
        return Err(EngineError::Other("files larger than 2 GB aren't supported".into()));
    }
    std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    // Write to a temporary file, then rename: a crash never leaves a half-written workbook.
    let tmp = format!("{path}.sctmp");
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        // A write or rename that fails (full disk, the workbook locked by another app) leaves
        // no temporary file beside the user's workbook.
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{path}: {e}"))
    })
}

#[cfg(target_arch = "wasm32")]
pub fn write_file(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.split_once(',').filter(|(h, _)| h.starts_with("data:")).map(|(_, b)| b).unwrap_or(s);
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = B64.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Pixel size of a PNG or JPEG from its header.
pub fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        let w = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
        return Some((w, h));
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < data.len() {
            if *data.get(i)? != 0xFF {
                i += 1;
                continue;
            }
            let marker = *data.get(i + 1)?;
            let len = u16::from_be_bytes([*data.get(i + 2)?, *data.get(i + 3)?]) as usize;
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([*data.get(i + 5)?, *data.get(i + 6)?]) as u32;
                let w = u16::from_be_bytes([*data.get(i + 7)?, *data.get(i + 8)?]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(rtl: bool, cells: &[(&str, gridcraft_core::Value, gridcraft_model::HAlign)]) -> Workbook {
        let mut wb = Workbook::new();
        let ids: Vec<_> = cells.iter().map(|(_, _, h)| wb.styles.derive(gridcraft_model::StyleId::DEFAULT, |st| st.align.h = *h)).collect();
        let sh = wb.sheet_mut(0).unwrap();
        sh.right_to_left = rtl;
        for ((at, value, _), id) in cells.iter().zip(ids) {
            let c = CellRef::parse(at).unwrap();
            sh.set_value(c, value.clone());
            sh.set_style(c, id);
        }
        wb
    }

    fn copied(wb: &Workbook, range: &str) -> String {
        let r = RangeRef::parse(range).unwrap();
        range_html(wb, 0, r, &crate::display::range_text(wb, 0, r)).unwrap()
    }

    #[test]
    fn html_export_marks_right_to_left_sheets() {
        use gridcraft_core::Value;
        use gridcraft_model::HAlign;
        let wb = book(true, &[("A1", Value::text("Hello!"), HAlign::General), ("B1", Value::Number(-5.0), HAlign::General)]);
        let html = to_html(&wb, 0).unwrap();
        assert!(html.contains("<body><table dir=\"rtl\">"), "{html}");
        assert!(html.contains("<td style=\"\" dir=\"auto\">Hello!</td>"), "Latin text keeps its own direction: {html}");
        assert!(
            html.contains("<td style=\"text-align:left;\" dir=\"ltr\">-5</td>"),
            "numbers mirror to the left and keep their sign in front: {html}"
        );
        let clip = copied(&wb, "A1:B1");
        assert!(clip.starts_with("<table dir=\"rtl\" style="), "{clip}");
        assert_eq!(clip.matches("dir=\"auto\"").count(), 1, "{clip}");
        assert!(clip.contains("text-align:left;\" dir=\"ltr\">-5</td>"), "{clip}");

        let ltr = book(false, &[("A1", Value::text("Hello!"), HAlign::General)]);
        assert!(to_html(&ltr, 0).unwrap().contains("<body><table><tr>"));
        assert!(copied(&ltr, "A1").starts_with("<table style="));
    }

    #[test]
    fn html_export_lets_arabic_general_text_align_by_direction() {
        use gridcraft_core::Value;
        use gridcraft_model::HAlign;
        let wb = book(
            false,
            &[
                ("A1", Value::text("مرحبا!"), HAlign::General),
                ("B1", Value::text("שלום"), HAlign::Left),
                ("C1", Value::text("مرحبا"), HAlign::Center),
                ("D1", Value::text("Hello"), HAlign::General),
            ],
        );
        let html = to_html(&wb, 0).unwrap();
        assert!(html.contains("<td style=\"\" dir=\"auto\">مرحبا!</td>"), "General Arabic text aligns by its direction: {html}");
        assert!(html.contains("<td style=\"text-align:left;\" dir=\"auto\">שלום</td>"), "explicit alignment stays: {html}");
        assert!(html.contains("<td style=\"text-align:center;\" dir=\"auto\">مرحبا</td>"), "{html}");
        assert!(html.contains("<td style=\"text-align:left;\">Hello</td>"), "{html}");
        assert!(html.contains("<body><table><tr>"), "a left-to-right sheet has no table direction: {html}");

        let clip = copied(&wb, "A1:D1");
        let cells: Vec<&str> = clip.split("<td ").skip(1).collect();
        assert_eq!(cells.len(), 4, "{clip}");
        assert!(cells[0].contains("dir=\"auto\"") && !cells[0].contains("text-align"), "{clip}");
        assert!(cells[1].contains("text-align:left;") && cells[1].contains("dir=\"auto\""), "{clip}");
        assert!(cells[3].contains("text-align:left;") && !cells[3].contains("dir="), "{clip}");
    }

    #[test]
    fn html_export_of_latin_ltr_sheet_is_unchanged() {
        use gridcraft_core::Value;
        use gridcraft_model::HAlign;
        let mut wb = book(
            false,
            &[
                ("A1", Value::text("Name"), HAlign::General),
                ("B1", Value::text("Qty"), HAlign::Center),
                ("A2", Value::text("Apples & <pears>"), HAlign::General),
                ("B2", Value::Number(3.0), HAlign::General),
                ("A3", Value::text("Total"), HAlign::Right),
            ],
        );
        wb.sheet_mut(0).unwrap().merges.push(RangeRef::parse("A3:B3").unwrap());
        assert_eq!(
            to_html(&wb, 0).unwrap(),
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Sheet1</title><style>table{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt}td{border:1px solid #d4d4d4;padding:2px 4px;white-space:nowrap}</style></head><body><table>\
             <tr><td style=\"text-align:left;\">Name</td><td style=\"text-align:center;\">Qty</td></tr>\
             <tr><td style=\"text-align:left;\">Apples &amp; &lt;pears&gt;</td><td style=\"text-align:right;\">3</td></tr>\
             <tr><td style=\"text-align:right;\" rowspan=\"1\" colspan=\"2\">Total</td></tr>\
             </table></body></html>"
        );
        let cell = "border:1px solid #d4d4d4;padding:2px 4px;white-space:pre-wrap;";
        assert_eq!(
            copied(&wb, "A1:B2"),
            format!(
                "<table style=\"border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt\">\
                 <tr><td style=\"{cell}text-align:left;\">Name</td><td style=\"{cell}text-align:center;\">Qty</td></tr>\
                 <tr><td style=\"{cell}text-align:left;\">Apples &amp; &lt;pears&gt;</td><td style=\"{cell}text-align:right;\">3</td></tr></table>"
            )
        );
    }

    #[test]
    fn html_export_fails_instead_of_dropping_cells_past_the_row_limit() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("near"));
        sh.set_value(CellRef::new(149_999, 0), gridcraft_core::Value::text("far"));
        assert!(save_bytes(&wb, "book.html").is_err());
        let sh = wb.sheet_mut(0).unwrap();
        sh.remove_cell(CellRef::new(149_999, 0));
        sh.set_value(CellRef::new(100_000, 0), gridcraft_core::Value::text("far"));
        let html = String::from_utf8(save_bytes(&wb, "book.html").unwrap()).unwrap();
        assert!(html.contains(">near</td>") && html.contains(">far</td>"));
    }

    #[test]
    fn macros_are_saved_only_into_xlsm() {
        let mut wb = Workbook::new();
        wb.vba_project = Some(b"vba".to_vec());
        let has_macros = |path: &str| {
            let bytes = save_bytes(&wb, path).unwrap();
            gridcraft_xlsx::read_xlsx(&bytes).unwrap().0.vba_project.is_some()
        };
        assert!(has_macros("book.xlsm"));
        assert!(has_macros("BOOK.XLSM"));
        assert!(!has_macros("book.xlsx"), "an .xlsx never carries a VBA project");
        assert!(has_macros("book.xltm"), "a macro-enabled template keeps the VBA project");
        assert!(!has_macros("book.xltx"), "a macro-free template never carries a VBA project");
        assert!(wb.vba_project.is_some(), "the open workbook keeps its macros");
    }

    #[test]
    fn csv_named_history_does_not_get_the_reserved_sheet_name() {
        for file in ["History.csv", "hIsToRy.csv", "history.tsv"] {
            let (wb, _) = open_bytes(file, b"1\n").unwrap();
            let name = wb.sheets[0].name.clone();
            assert!(wb.check_sheet_name(&name, Some(0)).is_ok(), "{file} opened as {name:?}");
        }
        let (wb, _) = open_bytes("Sales.csv", b"1\n").unwrap();
        assert_eq!(wb.sheets[0].name, "Sales");
    }

    /// A save that can't be put in place (here a folder already has the workbook's name, as
    /// with a file another app holds locked) fails without leaving `<name>.sctmp` behind.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn failed_save_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("gridcraft-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("Book1.xlsx");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_file(blocked.to_str().unwrap(), b"workbook").is_err());
        assert!(!dir.join("Book1.xlsx.sctmp").exists(), "the temporary file is removed");
        // An ordinary save still works and leaves only the workbook.
        let ok = dir.join("Book2.xlsx");
        write_file(ok.to_str().unwrap(), b"workbook").unwrap();
        assert_eq!(std::fs::read(&ok).unwrap(), b"workbook");
        assert!(!dir.join("Book2.xlsx.sctmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A minimal compound (CFB/OLE2) file — FAT in sector 0, directory in sector 1 — holding a
    /// root entry and the given stream names.
    fn cfb(streams: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 512 * 3];
        let mut put = |at: usize, v: &[u8]| b[at..at + v.len()].copy_from_slice(v);
        put(0, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        put(30, &9u16.to_le_bytes());
        put(44, &1u32.to_le_bytes());
        put(48, &1u32.to_le_bytes());
        put(68, &0xFFFF_FFFEu32.to_le_bytes());
        for i in 1..109 {
            put(76 + i * 4, &0xFFFF_FFFFu32.to_le_bytes());
        }
        put(512, &0xFFFF_FFFDu32.to_le_bytes());
        put(516, &0xFFFF_FFFEu32.to_le_bytes());
        for (k, name) in std::iter::once("Root Entry").chain(streams.iter().copied()).enumerate() {
            let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            put(1024 + k * 128, &units);
            put(1024 + k * 128 + 64, &(units.len() as u16 + 2).to_le_bytes());
        }
        b
    }

    /// Cut-list rows as a CAD/CAM program exports them: `;`-separated, CRLF, part codes in the last
    /// two columns, a part name containing a comma.
    const CUT_LIST: &str = "1;511;184;P3_L1_led;2162PE;16;1MM_2162PE;1MM_2162PE;;;1244121406FB;\r\n\
                            1;562.5;510;Komoda, lijevo;2162PE;19;1MM_2162PE;;;;12441214071A;124412140710\r\n";

    #[test]
    fn csv_save_keeps_the_separator_it_was_opened_with() {
        let (wb, _) = open_bytes("cut list.csv", CUT_LIST.as_bytes()).unwrap();
        assert_eq!(wb.csv_delimiter, Some(b';'));
        assert_eq!(String::from_utf8(save_bytes(&wb, "cut list.csv").unwrap()).unwrap(), CUT_LIST);
        // Save As another .csv name keeps it too; a comma file stays a comma file (quoting only
        // fields that contain the separator in use).
        assert_eq!(String::from_utf8(save_bytes(&wb, "copy.CSV").unwrap()).unwrap(), CUT_LIST);
        let commas = "\"Komoda, lijevo\",b\r\n1,x;y\r\n";
        let (wb, _) = open_bytes("plain.csv", commas.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(save_bytes(&wb, "plain.csv").unwrap()).unwrap(), commas);
        // A tab-separated file named .csv round-trips with tabs.
        let tabs = "a\tb\r\n1\t2\r\n";
        let (wb, _) = open_bytes("tabs.csv", tabs.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(save_bytes(&wb, "tabs.csv").unwrap()).unwrap(), tabs);
    }

    /// A CP932 (Shift-JIS) Japanese CSV built in code — `日本語,漢字,ｶﾅ,"a,b"` — never a binary fixture.
    fn japanese_cp932_csv() -> Vec<u8> {
        let mut b = vec![0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]; // 日本語
        b.push(b',');
        b.extend_from_slice(&[0x8A, 0xBF, 0x8E, 0x9A]); // 漢字
        b.push(b',');
        b.extend_from_slice(&[0xB6, 0xC5]); // ｶﾅ (halfwidth katakana)
        b.push(b',');
        b.extend_from_slice(b"\"a,b\"\r\n");
        b
    }

    /// A delimited file is saved in the code page it was opened with (the `csv_delimiter` model):
    /// `Auto` detects Shift-JIS from the kana, a plain save writes those bytes back, and an
    /// explicit encoding on open is remembered the same way.
    #[test]
    fn csv_save_keeps_the_shift_jis_it_was_opened_with() {
        let bytes = japanese_cp932_csv();
        let (wb, _) = open_bytes("日本語.csv", &bytes).unwrap();
        assert_eq!(wb.csv_encoding, Some(CsvEncoding::ShiftJis));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::text("日本語"));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 3)), gridcraft_core::Value::text("a,b"));
        assert_eq!(save_bytes(&wb, "日本語.csv").unwrap(), bytes, "a plain save keeps Shift-JIS");
        assert_eq!(save_bytes(&wb, "copy.CSV").unwrap(), bytes, "and Save As another name too");
        let (explicit, _) = open_bytes_encoded("jp.csv", &bytes, None, CsvEncoding::ShiftJis).unwrap();
        assert_eq!(explicit.csv_encoding, Some(CsvEncoding::ShiftJis));
        assert_eq!(save_bytes(&explicit, "jp.csv").unwrap(), bytes);
        // The same bytes forced through Windows-1252 are the Latin-1 reading, and round trip as it.
        let (latin, _) = open_bytes_encoded("jp.csv", &bytes, None, CsvEncoding::Windows1252).unwrap();
        assert_eq!(latin.csv_encoding, Some(CsvEncoding::Windows1252));
        assert_eq!(latin.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::text("\u{201C}\u{FA}\u{2013}{\u{152}\u{EA}"));
        assert_eq!(save_bytes(&latin, "jp.csv").unwrap(), bytes);
        // A .tsv keeps the encoding too (only its delimiter is reset).
        let tsv = [0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA, b'\t', 0xB6, 0xC5, b'\r', b'\n'];
        let (from_tsv, _) = open_bytes("日本語.tsv", &tsv).unwrap();
        assert_eq!(from_tsv.csv_encoding, Some(CsvEncoding::ShiftJis));
        assert_eq!(save_bytes(&from_tsv, "日本語.tsv").unwrap(), tsv);
    }

    /// A workbook that was never read from a delimited file saves as BOM-less UTF-8; an explicit
    /// encoding on a save applies to that write only.
    #[test]
    fn csv_save_defaults_to_utf8_without_bom() {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::text("日本語"));
        assert_eq!(wb.csv_encoding, None);
        assert_eq!(save_bytes(&wb, "book.csv").unwrap(), "日本語\r\n".as_bytes());
        let cp932 = vec![0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA, b'\r', b'\n'];
        assert_eq!(save_bytes_encoded(&wb, "book.csv", Some(CsvEncoding::ShiftJis)).unwrap(), cp932);
        assert_eq!(wb.csv_encoding, None, "a save never changes the workbook's remembered encoding");
        assert_eq!(save_bytes(&wb, "book.csv").unwrap(), "日本語\r\n".as_bytes(), "so the next plain save is UTF-8 again");
        let bom = save_bytes_encoded(&wb, "book.csv", Some(CsvEncoding::Utf8Bom)).unwrap();
        assert!(bom.starts_with(&[0xEF, 0xBB, 0xBF]) && bom.get(3..) == Some("日本語\r\n".as_bytes()));
        // A character CP932 cannot write becomes `?`, not an HTML numeric character reference.
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(1, 0), gridcraft_core::Value::text("Ǎ😀"));
        let sjis = save_bytes_encoded(&wb, "book.csv", Some(CsvEncoding::ShiftJis)).unwrap();
        assert!(sjis.ends_with(b"??\r\n"), "{sjis:?}");
        assert!(!sjis.windows(2).any(|w| w == b"&#"), "never an HTML character reference: {sjis:?}");
    }

    /// An Arabic *Unicode Text* file (UTF-16LE, tab-delimited) is saved back as UTF-16LE, not UTF-8.
    #[test]
    fn unicode_text_save_keeps_utf16() {
        let bytes: Vec<u8> = [0xFF, 0xFE].into_iter().chain("الاسم\tالمدينة\r\nسارة\tعمّان\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let (wb, _) = open_bytes("عملاء.txt", &bytes).unwrap();
        assert_eq!(wb.csv_encoding, Some(CsvEncoding::Utf16Le));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 1)), gridcraft_core::Value::text("عمّان"));
        assert_eq!(save_bytes(&wb, "عملاء.txt").unwrap(), bytes);
    }

    #[test]
    fn csv_save_of_a_workbook_not_read_from_csv_uses_commas() {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("a"));
            sh.set_value(CellRef::new(0, 1), gridcraft_core::Value::Number(1.0));
        }
        assert_eq!(save_bytes(&wb, "new.csv").unwrap(), b"a,1\r\n");
        // Through .xlsx the CSV origin is gone, as in Excel.
        let (from_csv, _) = open_bytes("cut list.csv", CUT_LIST.as_bytes()).unwrap();
        let (reopened, _) = open_bytes("book.xlsx", &save_bytes(&from_csv, "book.xlsx").unwrap()).unwrap();
        assert_eq!(reopened.csv_delimiter, None);
        assert!(String::from_utf8(save_bytes(&reopened, "again.csv").unwrap()).unwrap().starts_with("1,511,184,"));
        // A .tsv/.txt saved as .csv gets commas; as .tsv it keeps tabs.
        let (tsv, _) = open_bytes("list.txt", b"a\tb\r\n").unwrap();
        assert_eq!(save_bytes(&tsv, "list.csv").unwrap(), b"a,b\r\n");
        assert_eq!(save_bytes(&tsv, "list.tsv").unwrap(), b"a\tb\r\n");
    }

    #[test]
    fn encrypted_workbook_is_reported_not_misread() {
        let file = gridcraft_xlsx::testing::encrypt_workbook(b"package", "pw");
        let msg = open_bytes("secret.xlsx", &file).unwrap_err().to_string();
        assert!(msg.contains(PASSWORD_REQUIRED), "got: {msg}");
        assert!(!msg.contains("zip"), "must not surface the zip error: {msg}");
        // A compound file naming the streams but unreadable is damage: no password is asked for,
        // and none helps.
        let file = cfb(&["EncryptionInfo", "EncryptedPackage"]);
        for password in [None, Some("pw")] {
            let msg = open_bytes_with_password("secret.xlsx", &file, password).unwrap_err().to_string();
            assert!(msg.contains("damaged") && !msg.contains(PASSWORD_REQUIRED), "got: {msg}");
        }
    }

    #[test]
    fn encryption_no_password_opens_is_reported_without_asking_for_one() {
        let info: &[u8] = &[3, 0, 2, 0, 4, 0, 0, 0]; // Standard header flags: RC4, not AES
        let file = gridcraft_xlsx::testing::compound_file(&[("EncryptionInfo", info), ("EncryptedPackage", &[0; 8])]);
        let msg = open_bytes("rc4.xlsx", &file).unwrap_err().to_string();
        assert!(msg.contains("RC4") && !msg.contains(PASSWORD_REQUIRED), "got: {msg}");
        let irm = gridcraft_xlsx::testing::compound_file(&[("EncryptedPackage", &[0; 8])]);
        let msg = open_bytes("irm.xlsx", &irm).unwrap_err().to_string();
        assert!(msg.contains("rights-management") && !msg.contains("damaged"), "got: {msg}");
    }

    #[test]
    fn other_compound_files_are_not_called_encrypted() {
        let err = open_bytes("old.doc", &cfb(&["WordDocument", "\u{5}SummaryInformation"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not an Excel workbook"), "got: {msg}");
        assert!(!msg.contains("password"), "got: {msg}");
    }

    /// Save As CSV/TSV writes cells as displayed: number formats applied, formula results,
    /// whole values in narrow columns (never `#####`), quoting unchanged.
    #[test]
    fn csv_writes_displayed_text() {
        use gridcraft_core::{CellError, Value};
        use gridcraft_model::{Cell, Formula, LineInfo, NumFmt, StyleId};
        let mut wb = Workbook::new();
        let mut styled = |code: &str| wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
        let (date, money, pct, frac, text, fixed, thousands) =
            (styled("yyyy-mm-dd"), styled("$#,##0.00"), styled("0.0%"), styled("# ?/?"), styled("@"), styled("0.00"), styled("#,##0"));
        let sh = wb.sheet_mut(0).unwrap();
        let cells = [
            ("A1", Cell { value: Value::Number(46023.0), formula: None, style: date }),
            ("B1", Cell { value: Value::Number(1234.5), formula: None, style: money }),
            ("C1", Cell { value: Value::Number(0.125), formula: None, style: pct }),
            ("D1", Cell { value: Value::Number(1.5), formula: None, style: frac }),
            ("E1", Cell { value: Value::text("007"), formula: None, style: text }),
            ("F1", Cell { value: Value::Error(CellError::NA), formula: None, style: money }),
            ("G1", Cell { value: Value::Bool(true), formula: None, style: StyleId::DEFAULT }),
            ("A2", Cell { value: Value::Number(46024.0), style: date, ..Cell::formula(Formula::new("A1+1")) }),
            ("B2", Cell { value: Value::Number(12345678.9), formula: None, style: fixed }),
            ("C2", Cell { value: Value::Number(-0.25), formula: None, style: StyleId::DEFAULT }),
            ("D2", Cell { value: Value::Number(1234567.0), formula: None, style: thousands }),
        ];
        for (a, c) in cells {
            sh.set_cell(CellRef::parse(a).unwrap(), c);
        }
        // Column B is far too narrow for its numbers: the grid shows #####, the file must not.
        sh.cols.insert(1, LineInfo { size: Some(1.0), ..Default::default() });
        sh.show_formulas = true;
        let csv = String::from_utf8(save_bytes(&wb, "out.csv").unwrap()).unwrap();
        assert_eq!(csv, "2026-01-01,\"$1,234.50\",12.5%,1 1/2,007,#N/A,TRUE\r\n2026-01-02,12345678.90,-0.25,\"1,234,567\",,,\r\n");
        let tsv = String::from_utf8(save_bytes(&wb, "out.tsv").unwrap()).unwrap();
        assert!(tsv.starts_with("2026-01-01\t$1,234.50\t12.5%\t"), "{tsv}");
    }

    #[test]
    fn delimited_exports_preserve_general_precision_and_explicit_number_formats() {
        use gridcraft_core::Value;
        use gridcraft_model::{Cell, NumFmt, StyleId};
        let mut workbook = Workbook::new();
        let scientific = workbook.styles.derive(StyleId::DEFAULT, |style| style.num_fmt = NumFmt::new("0.00E+00"));
        let sheet = workbook.sheet_mut(0).unwrap();
        sheet.set_cell(CellRef::new(0, 0), Cell { value: Value::Number(124412140710.0), ..Default::default() });
        sheet.set_cell(CellRef::new(0, 1), Cell { value: Value::Number(1.23456789012345), ..Default::default() });
        sheet.set_cell(CellRef::new(0, 2), Cell { value: Value::Number(124412140710.0), style: scientific, ..Default::default() });
        for (path, separator) in [("data.csv", ","), ("data.tsv", "\t")] {
            let output = String::from_utf8(save_bytes(&workbook, path).unwrap()).unwrap();
            assert_eq!(output, format!("124412140710{separator}1.23456789012345{separator}1.24E+11\r\n"));
        }
    }

    #[test]
    fn template_extension_saves_template_content_type() {
        let bytes = save_bytes(&Workbook::new(), "Budget.xltx").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut types = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("[Content_Types].xml").unwrap(), &mut types).unwrap();
        assert!(
            types.contains(
                r#"PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml""#
            ),
            "{types}"
        );
    }

    #[test]
    fn content_wins_over_a_mismatched_extension() {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::Number(7.0));
        let package = save_bytes(&wb, "book.xlsx").unwrap();
        for name in ["book.xlsx", "book.ods", "book.xlsb", "book.csv"] {
            let (opened, _) = open_bytes(name, &package).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(opened.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::Number(7.0), "{name}");
        }
        for name in ["data.xlsx", "data.csv"] {
            let (opened, _) = open_bytes(name, b"a,b\n1,2\n").unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(opened.sheet(0).unwrap().value(CellRef::new(0, 1)), gridcraft_core::Value::text("b"), "{name}");
        }
    }
}
