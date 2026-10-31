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
    /// Formatted Text (Space delimited), Excel's `.prn`: the sheet as fixed-width text.
    Prn,
    Json,
    Html,
}

/// How a text file (CSV, TSV, TXT, PRN) is read: the Text Import Wizard's settings (#2553).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextOpen {
    /// Keep every field as the text it holds: no number, date or boolean conversion.
    pub as_text: bool,
    pub split: SplitChoice,
    /// Records skipped before the first row. `None` keeps every line; with
    /// [`SplitChoice::Detect`] it skips a title detected before the table.
    pub start_row: Option<usize>,
    /// The decimal separator numbers are read with; `None` detects it from the values.
    pub decimal: Option<gridcraft_xlsx::text_layout::Decimal>,
    pub qualifier: Qualifier,
}

/// How the lines of a text file split into fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SplitChoice {
    /// The file kind decides: detection for `.csv` (and unknown kinds), tab for `.txt`, `.tsv`
    /// and `.tab`, whole lines for `.prn` (each line in one cell, as Excel's File › Open).
    #[default]
    ByKind,
    /// Detected from the file, title rows included (`delimiter: "auto"`).
    Detect,
    Given(gridcraft_xlsx::text_layout::Split),
}

/// The text qualifier: the character that quotes a field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Qualifier {
    #[default]
    Double,
    Single,
    /// None: quotes are ordinary characters.
    None,
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
            "prn" => FileKind::Prn,
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
    let text = TextOpen { as_text: csv_as_text, ..Default::default() };
    open_bytes_text(name, bytes, password, &text).map(|(wb, warnings, _)| (wb, warnings))
}

/// [`open_bytes_with_password`], reading a text file (CSV, TSV, TXT, PRN) as `text` says, with
/// what reading it found: the split, the decimal separator and how complete the rows are.
pub fn open_bytes_text(
    name: &str,
    bytes: &[u8],
    password: Option<&str>,
    text: &TextOpen,
) -> Result<(Workbook, Vec<String>, Option<gridcraft_xlsx::text_layout::TextReport>)> {
    if gridcraft_xlsx::sniff(bytes) != gridcraft_xlsx::Format::Encrypted {
        let (mut wb, warnings, report) = open_plain(name, bytes, text)?;
        fit_imported_rows(&mut wb);
        return Ok((wb, warnings, report));
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
    let (mut wb, mut warnings, report) = open_plain(name, &plain, text)?;
    fit_imported_rows(&mut wb);
    warnings.insert(0, DECRYPTED_NOTICE.into());
    Ok((wb, warnings, report))
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

/// What a text file's reading found ([`open_bytes_text`]); `None` for other formats.
type TextFound = Option<gridcraft_xlsx::text_layout::TextReport>;

/// Parses bytes that aren't encrypted, then takes the workbook's macros out of their hidden names.
fn open_plain(name: &str, bytes: &[u8], text: &TextOpen) -> Result<(Workbook, Vec<String>, TextFound)> {
    let (mut wb, mut warnings, found) = read_plain(name, bytes, text)?;
    warnings.extend(crate::macros::load(&mut wb));
    Ok((wb, warnings, found))
}

fn read_plain(name: &str, bytes: &[u8], text: &TextOpen) -> Result<(Workbook, Vec<String>, TextFound)> {
    let kind = FileKind::from_path(name);
    let sniffed = gridcraft_xlsx::sniff(bytes);
    if sniffed == gridcraft_xlsx::Format::Xls {
        let (wb, report) = gridcraft_xlsx::read_xls(bytes).map_err(|e| EngineError::Other(format!("We can't open '{name}': {e}")))?;
        return Ok((wb, report.warnings, None));
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
        return Ok((wb, report.warnings, None));
    }
    // Text named `.xlsx` is read as text.
    if sniffed == gridcraft_xlsx::Format::Xlsx || (kind == Some(FileKind::Xlsx) && sniffed != gridcraft_xlsx::Format::Csv) {
        let (wb, report) = gridcraft_xlsx::read_xlsx(bytes).map_err(|e| {
            EngineError::Other(match (&e, gridcraft_xlsx::oversized_part(bytes)) {
                (gridcraft_xlsx::IoError::TooLarge(_), Some((part, size))) => part_too_large_message(name, &part, size),
                _ => format!("We can't open '{name}': {e}"),
            })
        })?;
        return Ok((wb, report.warnings, None));
    }
    match kind {
        Some(FileKind::Json) => {
            let mut wb: Workbook = serde_json::from_slice(bytes).map_err(|e| EngineError::Other(format!("not a GridCraft JSON workbook: {e}")))?;
            wb.styles.rebuild_index();
            if wb.sheets.is_empty() {
                // The file's macros are read after this (`macros::load`), so they come along.
                let macros = std::mem::take(&mut wb.macros_read);
                wb = Workbook::new();
                wb.macros_read = macros;
            }
            // The file's band placement and cell count are derived state: rebuild them from the
            // cells themselves, dropping what is outside the sheet grid (#1008).
            let dropped: usize = wb.sheets.iter_mut().map(|sh| std::sync::Arc::make_mut(sh).cells.rebuild()).sum();
            let mut warnings = Vec::new();
            if dropped > 0 {
                warnings.push(format!("{name}: {dropped} cells outside the 1,048,576 × 16,384 grid were dropped"));
            }
            Ok((wb, warnings, None))
        }
        _ => {
            let (mut wb, report) = read_text_file(kind, bytes, text)?;
            rename_first_sheet(&mut wb, name);
            Ok((wb, text_warnings(name, &report), Some(report)))
        }
    }
}

/// What reading a text file guessed or kept about its numbers: a `.csv` opens without a
/// preview, so the open says it.
fn text_warnings(name: &str, r: &gridcraft_xlsx::text_layout::TextReport) -> Vec<String> {
    use gridcraft_xlsx::text_layout::{Decimal, DecimalBasis};
    let mut out = vec![];
    if r.numbers.basis == DecimalBasis::Delimiter && !r.numbers.ambiguous.is_empty() {
        let with = if r.numbers.decimal == Decimal::Comma { "a decimal comma" } else { "a decimal point" };
        out.push(format!("{name}: {} could be read either way and were read with {with}", r.numbers.ambiguous.join(", ")));
    }
    if !r.numbers.mixed.is_empty() {
        let columns: Vec<String> = r.numbers.mixed.iter().map(|c| gridcraft_core::col_to_letters(u32::try_from(*c).unwrap_or(u32::MAX))).collect();
        let noun = if columns.len() == 1 { "column" } else { "columns" };
        out.push(format!(
            "{name}: {noun} {} holds numbers with a decimal point and with a decimal comma; those that depend on it were kept as text",
            columns.join(", ")
        ));
    }
    for (row, number) in &r.split_numbers {
        out.push(format!("{name}: in row {row} of the file, {number} was split at its thousands separator"));
    }
    out
}

/// Reads a CSV, TSV, TXT, PRN or other text file with the Text Import Wizard's settings.
fn read_text_file(kind: Option<FileKind>, bytes: &[u8], text: &TextOpen) -> Result<(Workbook, gridcraft_xlsx::text_layout::TextReport)> {
    let (mut wb, report) = gridcraft_xlsx::read_text(bytes, &csv_options(kind, text)).map_err(|e| EngineError::Other(e.to_string()))?;
    // Tab is what makes it a .tsv/.txt; saved as .csv it gets the CSV default.
    if kind == Some(FileKind::Tsv) && wb.csv_delimiter == Some(b'\t') {
        wb.csv_delimiter = None;
    }
    Ok((wb, report))
}

/// The most of a file the Text Import preview reads.
pub const PREVIEW_BYTES: usize = 1 << 20;

/// The Text Import Wizard's preview of the text file `name`: its first `rows` rows split as
/// `text` says (fields as the text they hold), and what reading found, from at most the first
/// [`PREVIEW_BYTES`] (up to the last line end in them). The flag says whether the file is longer.
pub fn preview_text(
    name: &str,
    bytes: &[u8],
    text: &TextOpen,
    rows: usize,
) -> Result<(Vec<Vec<String>>, gridcraft_xlsx::text_layout::TextReport, bool)> {
    let cut = if bytes.len() > PREVIEW_BYTES {
        let head = bytes.get(..PREVIEW_BYTES).unwrap_or(bytes);
        head.iter().rposition(|b| *b == b'\n').map_or(head, |i| head.get(..=i).unwrap_or(head))
    } else {
        bytes
    };
    let (rows, report) =
        gridcraft_xlsx::preview_text(cut, &csv_options(FileKind::from_path(name), text), rows).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok((rows, report, cut.len() < bytes.len()))
}

/// The xlsx crate's options for a text file of `kind` read as `text` says.
fn csv_options(kind: Option<FileKind>, text: &TextOpen) -> gridcraft_xlsx::CsvOptions {
    use gridcraft_xlsx::text_layout::{Split, TextKind};
    let mut opts = gridcraft_xlsx::CsvOptions {
        parse_values: !text.as_text,
        decimal: text.decimal,
        kind: match kind {
            Some(FileKind::Csv) => TextKind::Csv,
            Some(FileKind::Tsv) => TextKind::Tab,
            Some(FileKind::Prn) => TextKind::Prn,
            _ => TextKind::Other,
        },
        start_row: Some(text.start_row.unwrap_or(0)),
        ..Default::default()
    };
    match &text.split {
        SplitChoice::ByKind => match kind {
            Some(FileKind::Tsv) => opts.delimiter = b'\t',
            Some(FileKind::Prn) => opts.split = Some(Split::Lines),
            _ => opts.delimiter = 0,
        },
        SplitChoice::Detect => {
            opts.delimiter = 0;
            opts.detect_any = true;
            opts.start_row = text.start_row;
        }
        SplitChoice::Given(split) => opts.split = Some(split.clone()),
    }
    match text.qualifier {
        Qualifier::Double => opts.quote = b'"',
        Qualifier::Single => opts.quote = b'\'',
        Qualifier::None => opts.no_qualifier = true,
    }
    opts
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
    let stem = std::path::Path::new(path).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Sheet1");
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
        let name = std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path);
        return Err(EngineError::Other(format!("{name}: unknown file format. Save as .xlsx, .csv, .tsv, .prn, .json or .html.")));
    };
    save_bytes_as(wb, path, kind)
}

/// Encodes a workbook as `kind`, whatever `path`'s extension says.
pub fn save_bytes_as(wb: &Workbook, path: &str, kind: FileKind) -> Result<Vec<u8>> {
    let sheet = wb.active_sheet();
    match kind {
        // The extension picks the package kind; only the macro-enabled kinds (.xlsm, .xltm) carry
        // the VBA project, so any other name saves without it, as Excel does.
        // Macros travel in hidden names.
        FileKind::Xlsx => {
            let named = crate::macros::with_names(wb)?;
            let book = std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default();
            gridcraft_xlsx::write_xlsx_named(named.as_ref().unwrap_or(wb), gridcraft_xlsx::XlsxKind::from_path(path), book)
                .map_err(|e| EngineError::Other(e.to_string()))
        }
        FileKind::Xls => {
            Err(EngineError::Other("Excel 97-2003 (.xls) is supported for import only. Save as .xlsx or another supported export format.".into()))
        }
        FileKind::Xlsb => Err(EngineError::Other("XLSB is supported for data import only. Save as .xlsx or another supported export format.".into())),
        FileKind::Ods => Err(EngineError::Other("ODS is supported for data import only. Save as .xlsx or another supported export format.".into())),
        // A workbook read from CSV keeps the separator it was read with (a `;` file stays `;`).
        FileKind::Csv => wb.sheet(sheet).map(|sh| csv(wb, sh, wb.csv_delimiter.unwrap_or(b','))).transpose().map(Option::unwrap_or_default),
        FileKind::Tsv => wb.sheet(sheet).map(|sh| csv(wb, sh, b'\t')).transpose().map(Option::unwrap_or_default),
        FileKind::Prn => wb.sheet(sheet).map(|sh| prn(wb, sh)).transpose().map(Option::unwrap_or_default),
        FileKind::Json => crate::macros::json_bytes(wb),
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
        let text = if wb.styles.get(sh.style_id(c)).num_fmt.as_str().eq_ignore_ascii_case("General")
            && let gridcraft_core::Value::Number(number) = sh.value(c)
            && number.is_finite()
        {
            number.to_string()
        } else {
            crate::display::value_text(wb, sh, c)
        };
        decimal_comma_text(wb, sh, c, text)
    })
    .map_err(|e| EngineError::Other(e.to_string()))
}

/// The active sheet as CSV of raw cell values, for automation: numbers at full precision, dates and
/// times as ISO 8601, booleans `TRUE`/`FALSE`, errors as their text, text as stored. Save As CSV
/// writes displayed text instead ([`save_bytes`]) and stays the default.
pub fn csv_values_bytes(wb: &Workbook) -> Result<Vec<u8>> {
    let Some(sh) = wb.sheet(wb.active_sheet()) else { return Ok(Vec::new()) };
    gridcraft_xlsx::write_csv(sh, wb.csv_delimiter.unwrap_or(b','), |c| match sh.value(c) {
        gridcraft_core::Value::Number(n) if n.is_finite() => {
            let shape = crate::display::number_format(wb.styles.get(sh.style_id(c)).num_fmt.as_str()).date_shape(n);
            match shape.zip(gridcraft_core::date::datetime_from_serial(wb.date_system, n)) {
                Some(((date, time), d)) => iso(&d, date, time),
                None => n.to_string(),
            }
        }
        gridcraft_core::Value::Text(t) => t.to_string(),
        v => crate::display::value_text_as(wb, &v, "General"),
    })
    .map_err(|e| EngineError::Other(e.to_string()))
}

/// `date` and `time` say which parts the cell's format shows; a format with neither is not a date.
fn iso(d: &gridcraft_core::date::DateTime, date: bool, time: bool) -> String {
    let day = format!("{:04}-{:02}-{:02}", d.year, d.month, d.day);
    let mut clock = format!("{:02}:{:02}:{:02}", d.hour, d.minute, d.second);
    if d.milli != 0 {
        clock.push_str(&format!(".{:03}", d.milli));
    }
    match (date, time) {
        (true, true) => format!("{day}T{clock}"),
        (false, true) => clock,
        _ => day,
    }
}

/// A number's text with a decimal comma when the workbook was read from a file written that
/// way (`1,234.5` → `1.234,5`): only separators between two digits swap, so the literal text of
/// a format (`0.0" kg."`) and dates stay as they are.
fn decimal_comma_text(wb: &Workbook, sh: &Sheet, c: CellRef, text: String) -> String {
    if !wb.csv_decimal_comma || !sh.value(c).is_number() || crate::display::number_format(wb.styles.get(sh.style_id(c)).num_fmt.as_str()).is_date() {
        return text;
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let between =
                i.checked_sub(1).and_then(|j| chars.get(j)).is_some_and(char::is_ascii_digit) && chars.get(i + 1).is_some_and(char::is_ascii_digit);
            match ch {
                '.' if between => ',',
                ',' if between => '.',
                _ => *ch,
            }
        })
        .collect()
}

/// Formatted Text (Space delimited), Excel's `.prn`: the sheet as the grid shows it, measured
/// against Excel's Save As. One line per row from A1 to the end of the used range:
/// - each visible column as wide as its width in characters, rounded down; a column of the
///   standard width is 10 characters; hidden columns are left out;
/// - text left-aligned, numbers right-aligned and narrowed like the grid (General shows fewer
///   decimals, any number that still doesn't fit is `#`s), booleans and errors centred, or the
///   cell's own horizontal alignment (right-aligned text keeps one space after it);
/// - left-aligned text runs on into empty cells to its right, and past the last column;
/// - spaces at the end of a line are dropped; CRLF line ends, UTF-8.
fn prn(wb: &Workbook, sh: &Sheet) -> Result<Vec<u8>> {
    use gridcraft_model::HAlign;
    let Some(r) = sh.used_range() else { return Ok(Vec::new()) };
    let limit = gridcraft_xlsx::MAX_CSV_BYTES;
    let standard = (gridcraft_model::points_to_chars(f64::from(sh.default_col_width)).floor() as usize).saturating_add(2);
    let cols: Vec<(u32, usize)> = (0..=r.end.col)
        .filter(|c| !sh.cols.get(c).is_some_and(|i| i.hidden))
        .map(|c| (c, sh.cols.get(&c).and_then(|i| i.size).map_or(standard, |px| gridcraft_model::points_to_chars(f64::from(px)).floor() as usize)))
        .collect();
    let line_width = cols.iter().fold(0u64, |a, (_, w)| a.saturating_add(*w as u64));
    let least = (u64::from(r.end.row) + 1).saturating_mul(line_width.saturating_add(2));
    if least > limit as u64 {
        return Err(EngineError::Other(format!(
            "the text file would be at least {} MiB, over the limit of {} MiB; export a smaller range",
            least >> 20,
            limit >> 20
        )));
    }
    // Where each column starts on a line (the sum fits: it is below the limit checked above).
    let offsets: Vec<usize> = cols
        .iter()
        .scan(0usize, |at, (_, w)| {
            let start = *at;
            *at = at.saturating_add(*w);
            Some(start)
        })
        .collect();
    let mut out = String::new();
    for row in 0..=r.end.row {
        let mut line: Vec<char> = Vec::new();
        for (i, ((col, width), at)) in cols.iter().zip(offsets.iter().copied()).enumerate() {
            let c = CellRef::new(row, *col);
            let v = sh.value(c);
            if matches!(v, gridcraft_core::Value::Empty) {
                continue;
            }
            let style = wb.styles.get(sh.style_id(c));
            let number = v.is_number();
            let text = match v {
                gridcraft_core::Value::Number(n) if style.num_fmt.as_str().eq_ignore_ascii_case("General") => {
                    gridcraft_numfmt::format_general_fit(n, *width).unwrap_or_else(|| "#".repeat(*width))
                }
                _ => {
                    let t = crate::display::value_text(wb, sh, c);
                    if number && t.chars().count() > *width { "#".repeat(*width) } else { t }
                }
            };
            let text: Vec<char> = decimal_comma_text(wb, sh, c, text).chars().collect();
            let centred = matches!(v, gridcraft_core::Value::Bool(_) | gridcraft_core::Value::Error(_));
            let align = match style.align.h {
                HAlign::Left => HAlign::Left,
                HAlign::Center | HAlign::CenterAcross => HAlign::Center,
                HAlign::Right => HAlign::Right,
                _ if number => HAlign::Right,
                _ if centred => HAlign::Center,
                _ => HAlign::Left,
            };
            // Left-aligned text overflows into the empty cells after it, and past the end.
            let room = if align == HAlign::Left && !number {
                let mut room = *width;
                let mut open = true;
                for (next, w) in cols.iter().skip(i + 1) {
                    if !matches!(sh.value(CellRef::new(row, *next)), gridcraft_core::Value::Empty) {
                        open = false;
                        break;
                    }
                    room = room.saturating_add(*w);
                }
                if open { usize::MAX } else { room }
            } else {
                *width
            };
            let shown: Vec<char> = match align {
                HAlign::Right if !number => text.iter().copied().take(width.saturating_sub(1)).collect(),
                _ => text.iter().copied().take(room).collect(),
            };
            let pad = match align {
                HAlign::Right if number => width.saturating_sub(shown.len()),
                HAlign::Right => width.saturating_sub(shown.len() + 1),
                HAlign::Center => width.saturating_sub(shown.len()) / 2,
                _ => 0,
            };
            // The cell starts at its column; text running on from the left stops there.
            line.resize(at, ' ');
            line.resize(at.saturating_add(pad), ' ');
            line.extend(shown);
        }
        let text: String = line.into_iter().collect();
        out.push_str(text.trim_end_matches(' '));
        out.push_str("\r\n");
        if out.len() > limit {
            return Err(EngineError::Other(format!("the text file would be more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
        }
    }
    Ok(out.into_bytes())
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

/// Rows × columns of one web-page table. Save as Web Page writes every kept row's full used
/// column span into one growing String, so a sparse sheet with a lone far-right cell (or a
/// tall, wide one) would otherwise be gigabytes of `<td></td>` before finishing (#2387).
const HTML_EXPORT_MAX_CELLS: u64 = 4_000_000;

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
        // The table is one growing String: every kept row writes a `<td>` for the whole used
        // column span (unless the sheet has no row that is that wide). A tall, wide sheet
        // (or a sparse one with a lone far-right cell) would be gigabytes before finishing,
        // so the rectangle itself is bounded (#2387).
        if u64::from(r.height()) * u64::from(r.width()) > HTML_EXPORT_MAX_CELLS {
            return Err(EngineError::Other(format!(
                "the sheet is too tall × wide for a web page ({} × {} cells, at most {HTML_EXPORT_MAX_CELLS}). Save as .xlsx or CSV instead.",
                r.height(),
                r.width()
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
        return Err(EngineError::Other(format!("{path}: scripts, and macro steps without the permission, can't read or write files")));
    }
    Ok(())
}

/// The largest native file the reader still touches: its own cap plus one, used to stop
/// reads whose bytes arrive faster than metadata can claim (device files, growing files).
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn read_file(path: &str) -> Result<Vec<u8>> {
    let buf = read_file_head(path, MAX_READ_BYTES + 1)?;
    if buf.len() as u64 > MAX_READ_BYTES {
        return Err(EngineError::Other("files larger than 2 GB aren't supported".into()));
    }
    Ok(buf)
}

/// At most the first `max` bytes of the file at `path`: a preview doesn't read a whole file.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_file_head(path: &str, max: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    script_sandbox_check(path)?;
    let f = std::fs::File::open(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    // A symlink to an endless device reports length 0 but streams forever, so metadata
    // alone never bounds the read: require a real file and cap the bytes actually taken.
    if !f.metadata().map_err(|e| EngineError::Other(format!("{path}: {e}")))?.is_file() {
        return Err(EngineError::Other(format!("{path}: only regular files can be opened")));
    }
    let mut buf = Vec::new();
    let mut limited = (&f).take(max);
    limited.read_to_end(&mut buf).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(buf)
}

#[cfg(target_arch = "wasm32")]
pub fn read_file_head(path: &str, _max: u64) -> Result<Vec<u8>> {
    script_sandbox_check(path)?;
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    script_sandbox_check(path)?;
    // Write to a temporary file, then rename: a crash never leaves a half-written workbook.
    // The temporary file is created exclusively, so an existing file or symlink at that
    // path (planted by another local user) makes the save fail instead of truncating the
    // link's target; and it inherits the destination's access restrictions, so saving a
    // private workbook doesn't widen who can read it.
    let tmp = format!("{path}.sctmp");
    let prior = std::fs::metadata(path).ok().map(|m| m.permissions());
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(p) = &prior {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(p.mode());
    }
    let mut f = opts.open(&tmp).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{path}: {e}"))
    })?;
    #[cfg(not(unix))]
    if let Some(p) = &prior {
        let _ = f.set_permissions(p.clone());
    }
    f.write_all(bytes).and_then(|()| f.flush()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{path}: {e}"))
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // A write or rename that fails (full disk, the workbook locked by another app) leaves
        // no temporary file beside the user's workbook.
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{path}: {e}"))
    })
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
    base64_decode_head(s, usize::MAX)
}

/// The first `max` bytes `s` decodes to; what follows them isn't looked at.
pub fn base64_decode_head(s: &str, max: usize) -> Option<Vec<u8>> {
    let s = s.split_once(',').filter(|(h, _)| h.starts_with("data:")).map(|(_, b)| b).unwrap_or(s);
    let mut out = Vec::with_capacity((s.len() / 4 * 3).min(max));
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        if out.len() >= max {
            break;
        }
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
mod tests;
