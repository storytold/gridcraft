//! Opening and saving files: XLSX, XLSB and ODS data import, CSV/TSV, JSON (debug), HTML export.

use std::fmt::Write as _;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{Sheet, Workbook};

use crate::{EngineError, Result};

/// File formats GridCraft reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Xlsx,
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

/// Parses a file's bytes into a workbook. `name` picks the format by extension when the bytes
/// don't say.
pub fn open_bytes(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    let (mut wb, warnings) = read_bytes(name, bytes)?;
    share_formulas(&mut wb);
    Ok((wb, warnings))
}

/// Shares the formulas of a loaded workbook that are the same relative to their cells (filled
/// blocks): many files don't mark them, and a filled block then parses once and is held once.
/// The keys are worked out on every processor.
fn share_formulas(wb: &mut Workbook) {
    for si in 0..wb.sheets.len() {
        let Some(sh) = wb.sheets.get(si) else { continue };
        let formulas: Vec<(gridcraft_core::CellRef, std::sync::Arc<gridcraft_model::Formula>)> =
            sh.cells.iter().filter_map(|(c, cell)| cell.formula.clone().map(|f| (c, f))).collect();
        if formulas.len() < 2 {
            continue;
        }
        let keys = gridcraft_calc::par::filter_map(&formulas, |(c, f)| Some(gridcraft_model::FormulaSharer::key(*c, f)));
        let mut sharer = gridcraft_model::FormulaSharer::default();
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for ((c, f), key) in formulas.into_iter().zip(keys) {
            if let Some(shared) = sharer.share_existing(si, c, &f, key)
                && let Some(cell) = sheet.cells.get_mut(c)
            {
                cell.formula = Some(shared);
            }
        }
    }
}

fn read_bytes(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    let kind = FileKind::from_path(name);
    let sniffed = gridcraft_xlsx::sniff(bytes);
    if sniffed == gridcraft_xlsx::Format::Encrypted {
        return Err(EngineError::Other(format!(
            "'{name}' is password-protected. GridCraft can't open encrypted workbooks yet; remove the password in Excel and try again."
        )));
    }
    if sniffed == gridcraft_xlsx::Format::LegacyBinary {
        return Err(EngineError::Other(format!(
            "'{name}' is an Excel 97-2003 (.xls) workbook or another legacy binary file. GridCraft doesn't open these yet; save it as .xlsx in Excel and try again."
        )));
    }
    // Content wins over the extension: an ODS or XLSB package is imported as what it is,
    // whatever it is called; the extension only decides when the bytes don't say.
    let import = match sniffed {
        gridcraft_xlsx::Format::Ods => Some(FileKind::Ods),
        gridcraft_xlsx::Format::Xlsb => Some(FileKind::Xlsb),
        _ => kind.filter(|k| matches!(k, FileKind::Ods | FileKind::Xlsb)),
    };
    if let Some(import) = import {
        let read = if import == FileKind::Ods { gridcraft_xlsx::read_ods } else { gridcraft_xlsx::read_xlsb };
        let (wb, report) = read(bytes).map_err(|e| EngineError::Other(format!("We can't import '{name}': {e}")))?;
        return Ok((wb, report.warnings));
    }
    if sniffed == gridcraft_xlsx::Format::Xlsx || kind == Some(FileKind::Xlsx) {
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
    if !clean.is_empty()
        && let Some(sh) = wb.sheet_mut(0)
    {
        sh.name = clean;
    }
}

/// Encodes a workbook in the format chosen by `path`'s extension.
pub fn save_bytes(wb: &Workbook, path: &str) -> Result<Vec<u8>> {
    let sheet = wb.active_sheet;
    match FileKind::from_path(path).unwrap_or(FileKind::Xlsx) {
        FileKind::Xlsx => gridcraft_xlsx::write_xlsx(wb).map_err(|e| EngineError::Other(e.to_string())),
        FileKind::Xlsb => Err(EngineError::Other("XLSB is supported for data import only. Save as .xlsx or another supported export format.".into())),
        FileKind::Ods => Err(EngineError::Other("ODS is supported for data import only. Save as .xlsx or another supported export format.".into())),
        FileKind::Csv => Ok(wb.sheet(sheet).map(|sh| gridcraft_xlsx::write_csv(sh, wb, b',')).unwrap_or_default()),
        FileKind::Tsv => Ok(wb.sheet(sheet).map(|sh| gridcraft_xlsx::write_csv(sh, wb, b'\t')).unwrap_or_default()),
        FileKind::Json => serde_json::to_vec_pretty(wb).map_err(|e| EngineError::Other(e.to_string())),
        FileKind::Html => Ok(to_html(wb, sheet).into_bytes()),
    }
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

/// A sheet as a standalone HTML table (Save as Web Page).
pub fn to_html(wb: &Workbook, sheet: usize) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let _ = write!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>table{{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt}}td{{border:1px solid #d4d4d4;padding:2px 4px;white-space:nowrap}}</style></head><body><table>",
        esc(&sh.name)
    );
    if let Some(r) = sh.used_range() {
        for row in r.start.row..=r.end.row.min(r.start.row + 100_000) {
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
    out
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
    std::fs::write(&tmp, bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
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

    #[test]
    fn encrypted_workbook_is_reported_not_misread() {
        let err = open_bytes("secret.xlsx", &cfb(&["EncryptionInfo", "EncryptedPackage"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("password-protected"), "got: {msg}");
        assert!(!msg.contains("zip"), "must not surface the zip error: {msg}");
    }

    #[test]
    fn legacy_xls_is_not_called_encrypted() {
        let err = open_bytes("old.xls", &cfb(&["Workbook", "\u{5}SummaryInformation"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("97-2003") && msg.contains(".xlsx"), "got: {msg}");
        assert!(!msg.contains("password"), "got: {msg}");
    }
}
