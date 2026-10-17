//! Opening and saving files: XLSX, legacy XLS import, XLSB and ODS data import, CSV/TSV, JSON (debug), HTML export.

use std::fmt::Write as _;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{Sheet, Workbook};

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
    if gridcraft_xlsx::sniff(bytes) != gridcraft_xlsx::Format::Encrypted {
        let (mut wb, warnings) = open_plain(name, bytes)?;
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
    let (mut wb, mut warnings) = open_plain(name, &plain)?;
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

/// Parses bytes that aren't encrypted.
fn open_plain(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
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
            let opts = gridcraft_xlsx::CsvOptions { delimiter: b'\t', ..Default::default() };
            let mut wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            // Tab is what makes it a .tsv/.txt; saved as .csv it gets the CSV default.
            wb.csv_delimiter = None;
            rename_first_sheet(&mut wb, name);
            Ok((wb, vec![]))
        }
        _ => {
            let opts = gridcraft_xlsx::CsvOptions { delimiter: 0, ..Default::default() };
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
    // An unknown or missing extension is refused rather than written as an XLSX under that name:
    // a typo like `.cvs` would otherwise produce a file no other app recognises.
    let Some(kind) = FileKind::from_path(path) else {
        let name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
        return Err(EngineError::Other(format!("{name}: unknown file format. Save as .xlsx, .csv, .tsv, .json or .html.")));
    };
    save_bytes_as(wb, path, kind)
}

/// Encodes a workbook as `kind`, whatever `path`'s extension says.
pub fn save_bytes_as(wb: &Workbook, path: &str, kind: FileKind) -> Result<Vec<u8>> {
    let sheet = wb.active_sheet;
    match kind {
        // The extension picks the package kind; only the macro-enabled kinds (.xlsm, .xltm) carry
        // the VBA project, so any other name saves without it, as Excel does.
        FileKind::Xlsx => gridcraft_xlsx::write_xlsx_as(wb, gridcraft_xlsx::XlsxKind::from_path(path)).map_err(|e| EngineError::Other(e.to_string())),
        FileKind::Xls => {
            Err(EngineError::Other("Excel 97-2003 (.xls) is supported for import only. Save as .xlsx or another supported export format.".into()))
        }
        FileKind::Xlsb => Err(EngineError::Other("XLSB is supported for data import only. Save as .xlsx or another supported export format.".into())),
        FileKind::Ods => Err(EngineError::Other("ODS is supported for data import only. Save as .xlsx or another supported export format.".into())),
        // A workbook read from CSV keeps the separator it was read with (a `;` file stays `;`).
        FileKind::Csv => wb.sheet(sheet).map(|sh| csv(wb, sh, wb.csv_delimiter.unwrap_or(b','))).transpose().map(Option::unwrap_or_default),
        FileKind::Tsv => wb.sheet(sheet).map(|sh| csv(wb, sh, b'\t')).transpose().map(Option::unwrap_or_default),
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
fn csv(wb: &Workbook, sh: &Sheet, delimiter: u8) -> Result<Vec<u8>> {
    gridcraft_xlsx::write_csv(sh, delimiter, |c| {
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

fn html_cell_css(wb: &Workbook, sh: &Sheet, c: CellRef) -> String {
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
        gridcraft_model::HAlign::Center => "center",
        gridcraft_model::HAlign::Right => "right",
        gridcraft_model::HAlign::Left => "left",
        _ if v.is_number() => "right",
        _ => "left",
    };
    let _ = write!(css, "text-align:{align};");
    css
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
    out.write_str("<table style=\"border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt\">").ok()?;
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
            let css = html_cell_css(wb, sh, c);
            write!(out, "<td style=\"border:1px solid #d4d4d4;padding:2px 4px;white-space:pre-wrap;{css}\"{span}>").ok()?;
            // Use the visible selected cell, even when the merge's original anchor
            // is outside the range or hidden. This matches the plain-text copy.
            let text = crate::display::cell_text(wb, sh, c);
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
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>table{{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt}}td{{border:1px solid #d4d4d4;padding:2px 4px;white-space:nowrap}}</style></head><body><table>",
        esc(&sh.name)
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
                let css = html_cell_css(wb, sh, c);
                let span = sh
                    .merges
                    .iter()
                    .find(|m| m.start == c)
                    .map(|m| format!(" rowspan=\"{}\" colspan=\"{}\"", m.height(), m.width()))
                    .unwrap_or_default();
                let _ = write!(out, "<td style=\"{css}\"{span}>{}</td>", esc(&crate::display::cell_text(wb, sh, c)));
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
    use std::io::Write as _;
    // Write to a private temporary file, then rename: a crash never leaves a half-written
    // workbook, and two saves of the same workbook never share a temporary file.
    let tmp = temporary_sibling(path);
    let put = || -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        // A save keeps the permissions the workbook already had; a new file takes the defaults.
        #[cfg(unix)]
        if let Ok(meta) = std::fs::metadata(path) {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(meta.permissions().mode() & 0o7777))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    };
    put().map_err(|e| {
        // A write or rename that fails (full disk, the workbook locked by another app) leaves
        // no temporary file beside the user's workbook.
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{path}: {e}"))
    })
}

/// A sibling path private to this save: concurrent saves never collide on it, an existing
/// `<name>.sctmp` file or symlink is never followed or removed, and a leftover temporary file
/// from a crashed save is never mistaken for this one's.
#[cfg(not(target_arch = "wasm32"))]
fn temporary_sibling(path: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = gridcraft_core::date::unix_now_ms().unwrap_or(0.0) as u64;
    format!("{path}.sctmp-{}-{ms}-{seq}", std::process::id())
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

    /// A save never writes through a sibling `<name>.sctmp` symlink: the predictable shared path
    /// used to let anything that could plant one redirect the save over another file.
    #[cfg(unix)]
    #[test]
    fn save_does_not_follow_a_temporary_file_symlink() {
        let dir = std::env::temp_dir().join(format!("gridcraft-write-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        let book = dir.join("Book3.xlsx");
        std::os::unix::fs::symlink(&victim, dir.join("Book3.xlsx.sctmp")).unwrap();
        write_file(book.to_str().unwrap(), b"workbook").unwrap();
        assert_eq!(std::fs::read(&book).unwrap(), b"workbook");
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me", "the save didn't write through the sibling symlink");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A save keeps the permissions the workbook already had; replacing it used to recreate it
    /// with the umask defaults, silently relaxing a restrictive workbook.
    #[cfg(unix)]
    #[test]
    fn save_keeps_the_workbook_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("gridcraft-write-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let book = dir.join("Book4.xlsx");
        std::fs::write(&book, b"old").unwrap();
        std::fs::set_permissions(&book, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_file(book.to_str().unwrap(), b"workbook").unwrap();
        assert_eq!(std::fs::read(&book).unwrap(), b"workbook");
        let mode = std::fs::metadata(&book).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640, "the save kept the workbook's permissions");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A failed save cleans up only its own temporary file: the old shared `<name>.sctmp` path
    /// meant one save could truncate or delete another save's in-flight temporary file.
    #[test]
    fn failed_save_cleans_up_only_its_own_temporary_file() {
        let dir = std::env::temp_dir().join(format!("gridcraft-write-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sibling = dir.join("Book5.xlsx.sctmp");
        std::fs::write(&sibling, b"another save's data").unwrap();
        let blocked = dir.join("Book5.xlsx");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_file(blocked.to_str().unwrap(), b"workbook").is_err());
        assert_eq!(std::fs::read(&sibling).unwrap(), b"another save's data", "the sibling temporary file is left alone");
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
