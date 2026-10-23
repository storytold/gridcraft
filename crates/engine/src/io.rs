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
    open_bytes_with(name, bytes, password, false)
}

/// [`open_bytes_with_password`], and with `csv_as_text` a CSV/TSV/TXT file keeps every field as
/// the text it holds, with no number, date or boolean conversion (identifiers such as GTINs, SKUs
/// and ZIP codes survive). It does not apply to other formats.
pub fn open_bytes_with(name: &str, bytes: &[u8], password: Option<&str>, csv_as_text: bool) -> Result<(Workbook, Vec<String>)> {
    if gridcraft_xlsx::sniff(bytes) != gridcraft_xlsx::Format::Encrypted {
        let (mut wb, warnings) = open_plain(name, bytes, csv_as_text)?;
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
    let (mut wb, mut warnings) = open_plain(name, &plain, csv_as_text)?;
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
fn open_plain(name: &str, bytes: &[u8], csv_as_text: bool) -> Result<(Workbook, Vec<String>)> {
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
        let (wb, report) = gridcraft_xlsx::read_xlsx(bytes).map_err(|e| {
            EngineError::Other(match (&e, gridcraft_xlsx::oversized_part(bytes)) {
                (gridcraft_xlsx::IoError::TooLarge(_), Some((part, size))) => part_too_large_message(name, &part, size),
                _ => format!("We can't open '{name}': {e}"),
            })
        })?;
        return Ok((wb, report.warnings));
    }
    match kind {
        Some(FileKind::Json) => {
            let mut wb: Workbook = serde_json::from_slice(bytes).map_err(|e| EngineError::Other(format!("not a GridCraft JSON workbook: {e}")))?;
            wb.styles.rebuild_index();
            if wb.sheets.is_empty() {
                wb = Workbook::new();
            }
            // The file's band placement and cell count are derived state: rebuild them from the
            // cells themselves, dropping what is outside the sheet grid (#1008).
            let dropped: usize = wb.sheets.iter_mut().map(|sh| std::sync::Arc::make_mut(sh).cells.rebuild()).sum();
            let mut warnings = Vec::new();
            if dropped > 0 {
                warnings.push(format!("{name}: {dropped} cells outside the 1,048,576 × 16,384 grid were dropped"));
            }
            Ok((wb, warnings))
        }
        Some(FileKind::Tsv) => {
            let opts = gridcraft_xlsx::CsvOptions { delimiter: b'\t', parse_values: !csv_as_text, ..Default::default() };
            let mut wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            // Tab is what makes it a .tsv/.txt; saved as .csv it gets the CSV default.
            wb.csv_delimiter = None;
            rename_first_sheet(&mut wb, name);
            Ok((wb, vec![]))
        }
        _ => {
            let opts = gridcraft_xlsx::CsvOptions { delimiter: 0, parse_values: !csv_as_text, ..Default::default() };
            let mut wb = gridcraft_xlsx::read_csv(bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            rename_first_sheet(&mut wb, name);
            Ok((wb, vec![]))
        }
    }
}

/// What to tell the user when one part of an XLSX package unpacks to more than the reader's
/// per-part limit: the sizes, why the limit is there, and what gets the data in. The limit is
/// per part, so smaller sheets fit; the shared strings are one part for every sheet, so only a
/// CSV, which has no parts, helps there.
fn part_too_large_message(name: &str, part: &str, size: u64) -> String {
    let limit = gridcraft_xlsx::MAX_PART >> 20;
    let mb = size >> 20;
    let size = if mb > limit { format!("{mb} MB") } else { format!("more than {limit} MB") };
    let remedy = if part.contains("worksheets/") {
        "To open it, split the data across several sheets, since the limit applies to each part, or save the sheet as CSV, which keeps only its values."
    } else {
        "To open it, save the data as CSV, which keeps only its values."
    };
    format!(
        "We can't open '{name}': its part {part} unpacks to {size}, over GridCraft's limit of {limit} MB for one part \
         (a guard against damaged or malicious files). {remedy}"
    )
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
        FileKind::Xlsx => {
            let book = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
            gridcraft_xlsx::write_xlsx_named(wb, gridcraft_xlsx::XlsxKind::from_path(path), book).map_err(|e| EngineError::Other(e.to_string()))
        }
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

/// The file at `path` as `CELL("filename")` reports it: absolute, so the folder is the one the
/// user sees. A workbook that arrived as bytes has no path and keeps only its name.
pub fn source_path(path: &str) -> Option<std::path::PathBuf> {
    std::path::absolute(path).ok()
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

pub(crate) fn esc(s: &str) -> String {
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

/// A sheet as a standalone HTML table (Save as Web Page). A sheet with pictures gets the sheet's
/// own column widths and row heights so each picture and chart (inline SVG) can sit at its anchor.
pub fn to_html(wb: &Workbook, sheet: usize) -> Result<String> {
    let Some(sh) = wb.sheet(sheet) else { return Ok(String::new()) };
    let fixed = !(sh.images.is_empty() && sh.charts.is_empty());
    let used = sh.used_range();
    let mut out = String::new();
    let _ = write!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>table{{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt{}}}td{{border:1px solid #d4d4d4;padding:{};white-space:nowrap{}}}</style></head><body>",
        esc(&sh.name),
        if fixed { ";table-layout:fixed" } else { "" },
        if fixed { "0 4px" } else { "2px 4px" },
        if fixed { ";overflow:hidden;line-height:1" } else { "" }
    );
    if fixed {
        out.push_str("<div style=\"position:relative\">");
    }
    let _ = write!(out, "<table{}", table_dir(sh));
    if let (true, Some(r)) = (fixed, used) {
        let width: f32 = (r.start.col..=r.end.col).map(|c| sh.col_width(c)).sum();
        let _ = write!(out, " style=\"width:{width}px\"");
    }
    out.push('>');
    if let Some(r) = used {
        if r.height() > HTML_EXPORT_MAX_ROWS {
            return Err(EngineError::Other(format!(
                "the sheet is too tall for a web page ({} rows, at most {HTML_EXPORT_MAX_ROWS}). Save as .xlsx or CSV instead.",
                r.height()
            )));
        }
        if fixed {
            out.push_str("<colgroup>");
            for col in (r.start.col..=r.end.col).filter(|&c| !sh.is_col_hidden(c)) {
                let _ = write!(out, "<col style=\"width:{}px\">", sh.col_width(col));
            }
            out.push_str("</colgroup>");
        }
        for row in r.start.row..=r.end.row {
            if sh.is_row_hidden(row) {
                continue;
            }
            if fixed {
                let _ = write!(out, "<tr style=\"height:{}px\">", sh.row_height(row));
            } else {
                out.push_str("<tr>");
            }
            for col in r.start.col..=r.end.col {
                let c = CellRef::new(row, col);
                if sh.merges.iter().any(|m| m.contains(c) && m.start != c) || (fixed && sh.is_col_hidden(col)) {
                    continue;
                }
                let text = crate::display::cell_text(wb, sh, c);
                let dir = CellDir::of(sh, c, &text);
                let css = html_cell_css(wb, sh, c, dir);
                let span = sh
                    .merges
                    .iter()
                    .find(|m| m.start == c)
                    .map(|m| {
                        if fixed {
                            let rows = (m.start.row..=m.end.row).filter(|&y| !sh.is_row_hidden(y)).count();
                            let cols = (m.start.col..=m.end.col).filter(|&x| !sh.is_col_hidden(x)).count();
                            format!(" rowspan=\"{rows}\" colspan=\"{cols}\"")
                        } else {
                            format!(" rowspan=\"{}\" colspan=\"{}\"", m.height(), m.width())
                        }
                    })
                    .unwrap_or_default();
                let _ = write!(out, "<td style=\"{css}\"{}{span}>{}</td>", dir.attr(), esc(&text));
            }
            out.push_str("</tr>");
        }
    }
    out.push_str("</table>");
    if fixed {
        // Sheet units are CSS pixels; the table starts at the used range, not at A1.
        let (left, top) = used.map_or((0.0, 0.0), |r| (sh.col_left(r.start.col), sh.row_top(r.start.row)));
        let place = |a: &gridcraft_model::Anchor| {
            format!(
                "position:absolute;left:{}px;top:{}px;width:{}px;height:{}px",
                sh.col_left(a.cell.col) - left + f64::from(a.dx),
                sh.row_top(a.cell.row) - top + f64::from(a.dy),
                a.width.max(0.0),
                a.height.max(0.0)
            )
        };
        for im in &sh.images {
            let _ = write!(
                out,
                "<img src=\"data:{};base64,{}\" alt=\"{}\" style=\"{}\">",
                esc(&im.mime),
                base64_encode(&im.data),
                esc(&im.alt),
                place(&im.anchor)
            );
        }
        for ch in &sh.charts {
            if let Some(svg) = crate::chart_svg::chart_svg(wb, sheet, ch, &place(&ch.anchor)) {
                out.push_str(&svg);
            }
        }
        out.push_str("</div>");
    }
    out.push_str("</body></html>");
    Ok(out)
}

thread_local! {
    /// How many [`ScriptSandbox`]es are alive on this thread.
    static SCRIPT_SANDBOX: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// While one is alive, [`read_file`] and [`write_file`] on this thread refuse every path: a
/// stored script's steps run inside one (#2178), so no command reaches the file system from a
/// script whatever its parameters are called. Dropping it (also on unwind) lifts the refusal.
pub(crate) struct ScriptSandbox(());

impl ScriptSandbox {
    pub(crate) fn enter() -> Self {
        let _ = SCRIPT_SANDBOX.try_with(|c| c.set(c.get().saturating_add(1)));
        ScriptSandbox(())
    }
}

impl Drop for ScriptSandbox {
    fn drop(&mut self) {
        let _ = SCRIPT_SANDBOX.try_with(|c| c.set(c.get().saturating_sub(1)));
    }
}

/// Refuses file access while a [`ScriptSandbox`] is alive (and, failing closed, while the
/// thread is being torn down).
fn script_sandbox_check(path: &str) -> Result<()> {
    if SCRIPT_SANDBOX.try_with(|c| c.get() > 0).unwrap_or(true) {
        return Err(EngineError::Other(format!("{path}: scripts can't read or write files")));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    script_sandbox_check(path)?;
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if meta.len() > 2 * 1024 * 1024 * 1024 {
        return Err(EngineError::Other("files larger than 2 GB aren't supported".into()));
    }
    std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    script_sandbox_check(path)?;
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    script_sandbox_check(path)?;
    // Write to a private temporary file, then rename: a crash never leaves a half-written
    // workbook, and two saves of the same workbook never share a temporary file.
    let tmp = temporary_sibling(path);
    write_file_with_temp(path, bytes, &tmp)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file_with_temp(path: &str, bytes: &[u8], tmp: &str) -> Result<()> {
    use std::io::Write as _;
    let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(tmp) {
        Ok(file) => file,
        Err(e) => return Err(EngineError::Other(format!("{path}: {e}"))),
    };
    let written = (|| -> std::io::Result<()> {
        // A save keeps the permissions the workbook already had; a new file takes the defaults.
        #[cfg(unix)]
        if let Ok(meta) = std::fs::metadata(path) {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(meta.permissions().mode() & 0o7777))?;
        }
        file.write_all(bytes)?;
        file.sync_all()
    })();
    drop(file);
    written.and_then(|()| std::fs::rename(tmp, path)).map_err(|e| {
        // A write or rename that fails (full disk, the workbook locked by another app) leaves
        // no temporary file beside the user's workbook. The temp file exists only if this call
        // successfully opened it with create_new above.
        let _ = std::fs::remove_file(tmp);
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
    script_sandbox_check(path)?;
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
    fn html_export_embeds_images_as_data_uris() {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().images.push(gridcraft_model::Image {
            id: 1,
            anchor: gridcraft_model::Anchor { cell: CellRef::new(0, 0), dx: 0.0, dy: 0.0, width: 72.0, height: 36.0, mode: Default::default() },
            data: vec![1, 2, 3].into(),
            mime: "image/png".into(),
            alt: "a \"logo\"".into(),
        });
        let html = to_html(&wb, 0).unwrap();
        assert!(
            html.contains("<img src=\"data:image/png;base64,AQID\" alt=\"a &quot;logo&quot;\" style=\"position:absolute;left:0px;top:0px;width:72px;height:36px\"></div></body>"),
            "{html}"
        );
        assert!(html.contains("<div style=\"position:relative\"><table"), "{html}");
        assert!(!to_html(&Workbook::new(), 0).unwrap().contains("<img"), "no images, no <img>");
    }

    #[test]
    fn html_export_places_images_at_their_anchor_using_sheet_geometry() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        sh.cols.insert(1, gridcraft_model::LineInfo { size: Some(100.0), ..Default::default() });
        sh.rows.insert(1, gridcraft_model::LineInfo { size: Some(30.0), ..Default::default() });
        sh.images.push(gridcraft_model::Image {
            id: 1,
            anchor: gridcraft_model::Anchor { cell: CellRef::new(3, 2), dx: 5.0, dy: 7.0, width: 10.0, height: 20.0, mode: Default::default() },
            data: vec![1].into(),
            mime: "image/png".into(),
            alt: String::new(),
        });
        sh.set_value(CellRef::new(1, 1), gridcraft_core::Value::Number(1.0));
        let html = to_html(&wb, 0).unwrap();
        // The table starts at B2; the picture is at C4: one 100 px column and two rows (30 + 20) further on.
        assert!(html.contains("<col style=\"width:100px\">"), "{html}");
        assert!(html.contains("<tr style=\"height:30px\">"), "{html}");
        assert!(html.contains("left:105px;top:57px;width:10px;height:20px"), "{html}");
    }

    fn book_with_picture(anchor: &str, cell_at: (u32, u32)) -> Workbook {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(cell_at.0, cell_at.1), gridcraft_core::Value::Number(1.0));
        sh.images.push(gridcraft_model::Image {
            id: 1,
            anchor: gridcraft_model::Anchor {
                cell: CellRef::parse(anchor).unwrap(),
                dx: 0.0,
                dy: 0.0,
                width: 10.0,
                height: 10.0,
                mode: Default::default(),
            },
            data: vec![1].into(),
            mime: "image/png".into(),
            alt: String::new(),
        });
        wb
    }

    #[test]
    fn html_export_places_images_past_hidden_columns_and_rows() {
        let mut wb = book_with_picture("C3", (0, 0));
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(2, 2), gridcraft_core::Value::Number(2.0));
        sh.cols.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
        sh.rows.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
        let html = to_html(&wb, 0).unwrap();
        // Column B and row 2 take no room: C3 is one default column and row from A1.
        assert_eq!(html.matches("<col ").count(), 2, "{html}");
        assert_eq!(html.matches("<tr ").count(), 2, "{html}");
        assert!(html.contains("left:64px;top:20px"), "{html}");
    }

    #[test]
    fn html_export_places_images_above_and_left_of_the_table() {
        let html = to_html(&book_with_picture("A1", (2, 2)), 0).unwrap();
        // The table starts at C3, so a picture at A1 sits before it instead of failing.
        assert!(html.contains("left:-128px;top:-40px"), "{html}");
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
        wb.vba_project = Some(b"vba".to_vec().into());
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn failed_temp_creation_does_not_remove_the_existing_collision() {
        let dir = std::env::temp_dir().join(format!("gridcraft-write-collision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Book6.xlsx");
        let tmp = dir.join("forced-collision.sctmp");
        std::fs::write(&tmp, b"another save's data").unwrap();

        assert!(write_file_with_temp(target.to_str().unwrap(), b"workbook", tmp.to_str().unwrap()).is_err());
        assert_eq!(std::fs::read(&tmp).unwrap(), b"another save's data", "failed create_new must leave the existing file alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// While a script runs (#2178), file reads and writes are refused whatever command or
    /// parameter asked for them; nesting and dropping the sandbox restore access.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn script_sandbox_refuses_file_access_until_dropped() {
        let dir = std::env::temp_dir().join(format!("gridcraft-sandbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("in.csv");
        std::fs::write(&existing, b"a,b\n").unwrap();
        let (existing, out) = (existing.to_string_lossy().to_string(), dir.join("out.xlsx").to_string_lossy().to_string());
        {
            let _outer = ScriptSandbox::enter();
            {
                let _inner = ScriptSandbox::enter();
                assert!(read_file(&existing).is_err());
            }
            assert!(read_file(&existing).is_err(), "the outer sandbox still holds");
            assert!(write_file(&out, b"leak").is_err());
            assert!(!std::path::Path::new(&out).exists());
        }
        assert_eq!(read_file(&existing).unwrap(), b"a,b\n");
        write_file(&out, b"ok").unwrap();
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

    /// Where `part`'s entry starts in the central directory of `package`.
    fn central_entry(package: &[u8], part: &str) -> usize {
        let mut at = 0;
        while let Some(i) = package.get(at..).and_then(|rest| rest.windows(4).position(|w| w == b"PK\x01\x02")) {
            let entry = at + i;
            let len = u16::from_le_bytes([package[entry + 28], package[entry + 29]]) as usize;
            if &package[entry + 46..entry + 46 + len] == part.as_bytes() {
                return entry;
            }
            at = entry + 46;
        }
        panic!("{part} is not in the package");
    }

    /// `package` with `part` declaring `size` unpacked bytes: the reader refuses the part on the
    /// declared size, so no real 512 MB part has to be built.
    fn declaring(mut package: Vec<u8>, part: &str, size: u32) -> Vec<u8> {
        let at = central_entry(&package, part);
        package[at + 24..at + 28].copy_from_slice(&size.to_le_bytes());
        package
    }

    #[test]
    fn a_part_over_the_size_limit_says_how_to_get_the_data_in() {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::text("big"));
        let package = save_bytes(&wb, "big.xlsx").unwrap();
        // The size reported in storytold/gridcraft#1242.
        let msg = open_bytes("big.xlsx", &declaring(package.clone(), "xl/worksheets/sheet1.xml", 540_278_731)).unwrap_err().to_string();
        assert!(msg.starts_with("We can't open 'big.xlsx': "), "{msg}");
        assert!(msg.contains("xl/worksheets/sheet1.xml") && msg.contains("515 MB") && msg.contains("512 MB"), "{msg}");
        assert!(msg.contains("several sheets") && msg.contains("CSV"), "{msg}");
        assert!(!msg.contains("bytes"), "{msg}");
        // Splitting sheets doesn't shrink the shared strings, which every sheet uses: only CSV helps.
        let msg = open_bytes("big.xlsx", &declaring(package.clone(), "xl/sharedStrings.xml", 540_278_731)).unwrap_err().to_string();
        assert!(msg.contains("xl/sharedStrings.xml") && msg.contains("515 MB") && msg.contains("CSV"), "{msg}");
        assert!(!msg.contains("several sheets"), "{msg}");
        // Just over the limit never reads as a size equal to it.
        let msg = open_bytes("big.xlsx", &declaring(package, "xl/worksheets/sheet1.xml", 512 * 1024 * 1024 + 1)).unwrap_err().to_string();
        assert!(msg.contains("more than 512 MB"), "{msg}");
        // A package that fails for another reason keeps its own error, oversized part or not.
        let mut broken = declaring(save_bytes(&wb, "big.xlsx").unwrap(), "xl/worksheets/sheet1.xml", 540_278_731);
        let at = central_entry(&broken, "xl/workbook.xml");
        broken[at + 46 + "xl/workbook.xml".len() - 1] = b'x';
        let msg = open_bytes("big.xlsx", &broken).unwrap_err().to_string();
        assert!(msg.starts_with("We can't open 'big.xlsx': ") && !msg.contains("unpacks"), "{msg}");
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
