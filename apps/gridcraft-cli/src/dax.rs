//! `gridcraft-cli dax`: format, dump or bind DAX text.

use std::io::{Cursor, Read, Write};

use gridcraft_dax::model::DataModel;
use gridcraft_dax::{
    Binder, Evaluator, Expr, ModelMeasure, Query, TableValue, TokenKind, parse_expression, parse_query, pivot_cross, pivot_fields, print_expression,
    print_query, read_measures, tokenize, write_measures,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{Args, USAGE};

/// Decompressed bytes a single zip entry may hold.
///
/// `read_measures` only rejects a container past [`gridcraft_dax::model_file::MAX_CONTAINER_BYTES`]
/// once it is already in memory, so the same bound is enforced while the entry is decompressed: a
/// zip bomb would otherwise allocate without limit first.
const MAX_ENTRY_BYTES: usize = gridcraft_dax::model_file::MAX_CONTAINER_BYTES;

/// Total decompressed bytes a rewritten package may hold.
///
/// The replacement package is built in memory, so per-entry limits alone are not enough: deflate
/// reaches roughly 1000:1, and many individually-legal entries could still allocate tens of
/// gigabytes. Two entries at the per-entry limit is the most any single workbook is expected to
/// need.
const MAX_PACKAGE_BYTES: usize = 2 * MAX_ENTRY_BYTES;

/// Runs the `dax` subcommand named by `args` (`format`, `parse`, `bind`, `eval`, `pivot`, `measures`).
pub fn main(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).ok_or_else(|| format!("missing dax action\n{USAGE}"))?;
    let rest = args.get(1..).unwrap_or_default();
    match action {
        "format" => format(rest),
        "parse" => parse(rest),
        "bind" => bind(rest),
        "eval" => eval(rest),
        "pivot" => pivot_cmd(rest),
        "measures" => measures(rest),
        other => Err(format!("unknown dax action `{other}`\n{USAGE}")),
    }
}

/// Prints the canonical text of a query or an expression.
fn format(args: &[String]) -> Result<(), String> {
    let text = read_input(args)?;
    match parse_any(&text)? {
        Parsed::Query(query) => outln!("{}", print_query(&query)),
        Parsed::Expression(expr) => outln!("{}", print_expression(&expr)),
    }
    Ok(())
}

/// Prints the syntax tree of a query or an expression.
fn parse(args: &[String]) -> Result<(), String> {
    let text = read_input(args)?;
    match parse_any(&text)? {
        Parsed::Query(query) => outln!("{query:#?}"),
        Parsed::Expression(expr) => outln!("{expr:#?}"),
    }
    Ok(())
}

/// Binds a formula or a model measure against a JSON data model.
fn bind(args: &[String]) -> Result<(), String> {
    let parsed = Args::parse(args, &["file", "model", "measure"], &[])?;
    let path = parsed.opt("model").ok_or("dax bind needs --model PATH")?;
    let json = std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let mut model: DataModel = serde_json::from_str(&json).map_err(|e| format!("invalid model JSON: {e}"))?;
    model.parse_formulas().map_err(|e| e.to_string())?;
    let binder = Binder::new(&model);

    if let Some(name) = parsed.opt("measure") {
        if !parsed.positional.is_empty() || parsed.opt("file").is_some() {
            return Err("give --measure or a formula, not both".into());
        }
        let bound = binder.bind_measure(name).map_err(|e| e.to_string())?;
        outln!("{bound:#?}");
        let dependencies: Vec<&str> = bound.dependencies.iter().map(String::as_str).collect();
        outln!("dependencies: {}", dependencies.join(", "));
        return Ok(());
    }

    let text = read_input_with(args, &["file", "model"], &[])?;
    let expr = parse_expression(&text).map_err(|e| e.to_string())?;
    let bound = binder.bind_expression(&expr).map_err(|e| e.to_string())?;
    outln!("{bound:#?}");
    Ok(())
}

/// Like [`read_input`], but leaves the `model`/`measure` options to the caller.
fn read_input_with(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<String, String> {
    let parsed = Args::parse(args, with_value, switches)?;
    if let Some(path) = parsed.opt("file") {
        if !parsed.positional.is_empty() {
            return Err("give either TEXT or --file, not both".into());
        }
        return std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"));
    }
    if !parsed.positional.is_empty() {
        return Ok(parsed.positional.join(" "));
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|e| format!("can't read stdin: {e}"))?;
    Ok(text)
}

/// Groups a model by row fields and evaluates a measure per group.
fn pivot_cmd(args: &[String]) -> Result<(), String> {
    let parsed = Args::parse(args, &["model", "measure", "rows", "columns"], &[])?;
    // `dax pivot` takes everything from options, so a positional is an argument the command would
    // silently drop: `--rows Sales[Region] Sales[Year]` would group by region alone.
    parsed.no_extra(0)?;
    let path = parsed.opt("model").ok_or("dax pivot needs --model PATH")?;
    let measure = parsed.opt("measure").ok_or("dax pivot needs --measure NAME")?;
    let rows = parsed.opt("rows").ok_or("dax pivot needs --rows 'Sales[Region]'")?;
    let json = std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let mut model: DataModel = serde_json::from_str(&json).map_err(|e| format!("invalid model JSON: {e}"))?;
    model.parse_formulas().map_err(|e| e.to_string())?;
    let fields = pivot_fields(&model, &split_fields(rows)).map_err(|e| e.to_string())?;
    let columns = parsed.opt("columns").map(|columns| pivot_fields(&model, &split_fields(columns)));
    let column_fields = match columns {
        Some(Ok(fields)) => fields,
        Some(Err(error)) => return Err(error.to_string()),
        None => Vec::new(),
    };
    let table = pivot_cross(&model, measure, &fields, &column_fields).map_err(|e| e.to_string())?;
    let columns: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
    let rows = table_rows_json(&table)?;
    outln!("{}", serde_json::json!({"columns": columns, "rows": rows}));
    Ok(())
}

/// Splits a `--rows`/`--columns` list on the commas that separate fields.
///
/// A comma inside a quoted table name or a bracketed column name belongs to the reference
/// (`'Sales, EU'[Region]`, `Sales[Last, First]`), and DAX escapes those delimiters by doubling them:
/// `]]` stays inside a column name and `''` inside a table name.
fn split_fields(list: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    let mut brackets = 0usize;
    let mut chars = list.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\'' if brackets == 0 => {
                if quoted {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quoted = false;
                    }
                } else {
                    quoted = true;
                }
            }
            // An opening `[` inside a bracketed column name is literal — the DAX lexer only ends the
            // name at an unescaped `]`, so `Sales[A[B,C]` is one reference, not two fields.
            '[' if !quoted && brackets == 0 => brackets += 1,
            ']' if !quoted && brackets > 0 => {
                if chars.peek().is_some_and(|(_, next)| *next == ']') {
                    // An escaped `]` inside the column name, not the closing bracket.
                    chars.next();
                } else {
                    brackets -= 1;
                }
            }
            ',' if !quoted && brackets == 0 => {
                push_field(&mut fields, &list[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    push_field(&mut fields, &list[start..]);
    fields
}

/// Adds one trimmed field, dropping the empty segments a trailing or doubled comma would produce.
fn push_field(fields: &mut Vec<String>, field: &str) {
    let field = field.trim();
    if !field.is_empty() {
        fields.push(field.to_string());
    }
}

/// Evaluates a formula or measure against a JSON data model.
fn eval(args: &[String]) -> Result<(), String> {
    let parsed = Args::parse(args, &["file", "model", "measure"], &["table"])?;
    let path = parsed.opt("model").ok_or("dax eval needs --model PATH")?;
    let json = std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let mut model: DataModel = serde_json::from_str(&json).map_err(|e| format!("invalid model JSON: {e}"))?;
    model.parse_formulas().map_err(|e| e.to_string())?;
    let evaluator = Evaluator::new(&model).map_err(|e| e.to_string())?;

    if let Some(name) = parsed.opt("measure") {
        // A measure result is a scalar: a formula or a table request alongside it would be silently
        // ignored, so the same conflicts `dax bind` rejects are rejected here.
        if parsed.opt("file").is_some() || !parsed.positional.is_empty() {
            return Err("dax eval takes either --measure or a formula, not both".into());
        }
        if parsed.has("table") {
            return Err("dax eval --measure returns a scalar; drop --table".into());
        }
        let value = evaluator.evaluate_measure(name, Default::default()).map_err(|e| e.to_string())?;
        outln!("{}", scalar_json(&value)?);
        return Ok(());
    }

    let text = read_input_with(args, &["file", "model"], &["table"])?;
    let expr = parse_expression(&text).map_err(|e| e.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|e| e.to_string())?;
    if parsed.has("table") {
        let table = evaluator.evaluate_table(&bound).map_err(|e| e.to_string())?;
        let columns: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
        let rows = table_rows_json(&table)?;
        outln!("{}", serde_json::json!({"columns": columns, "rows": rows}));
    } else {
        let value = evaluator.evaluate(&bound).map_err(|e| e.to_string())?;
        outln!("{}", scalar_json(&value)?);
    }
    Ok(())
}

/// Reads or rewrites Power Pivot measures in `xl/model/item.data` inside an `.xlsx` package.
fn measures(args: &[String]) -> Result<(), String> {
    let parsed = Args::parse(args, &["file", "write", "out"], &[])?;
    // Every input is an option, so a positional would be silently ignored.
    parsed.no_extra(0)?;
    let path = parsed.opt("file").ok_or("dax measures needs --file PATH")?;
    // Without `--write` the command only prints the current measures, so `--out` would be silently
    // ignored and the command could report success without creating or updating that file.
    if parsed.opt("write").is_none() && parsed.opt("out").is_some() {
        return Err("dax measures --out needs --write".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let item_data = read_zip_entry(&bytes, "xl/model/item.data")?;
    let current = read_measures(&item_data).map_err(|e| e.to_string())?;

    let Some(write_path) = parsed.opt("write") else {
        outln!("{}", serde_json::to_string_pretty(&current).map_err(|e| e.to_string())?);
        return Ok(());
    };
    let out = parsed.opt("out").ok_or("dax measures --write needs --out PATH")?;
    let json = std::fs::read_to_string(write_path).map_err(|e| format!("can't read {write_path}: {e}"))?;
    let measures: Vec<ModelMeasure> = serde_json::from_str(&json).map_err(|e| format!("invalid measures JSON: {e}"))?;
    let updated = write_measures(&item_data, &measures).map_err(|e| e.to_string())?;
    let package = replace_zip_entry(&bytes, "xl/model/item.data", &updated)?;
    write_atomically(out, &package)?;
    outln!("{}", serde_json::to_string_pretty(&measures).map_err(|e| e.to_string())?);
    Ok(())
}

/// Writes `bytes` to a temporary file beside `path`, then renames it over `path`.
///
/// `--out` may name the workbook `--file` read, and an interrupted or full-disk write would then
/// leave no complete copy: the temporary file is flushed to disk before the rename, so the rename
/// never publishes bytes the filesystem has not stored.
fn write_atomically(path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = std::path::Path::new(path);
    let directory = target.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    let file_name = target.file_name().ok_or_else(|| format!("can't write {path}: no file name"))?;
    // The name is unpredictable and the file is created exclusively: the process id alone is
    // guessable, and `File::create` would truncate a link another user placed under that name
    // before the rename. The counter is process-local (each process draws a distinct sequence)
    // and the address makes two writers that share a pid and a counter value unlikely to collide.
    // No clock is read: this code also builds for wasm, where clocks trap.
    static ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let attempt = ATTEMPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = directory.join(format!(
            ".{}.{}.{}.{:x}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            attempt,
            (&ATTEMPT as *const std::sync::atomic::AtomicU64) as usize ^ attempt as usize,
        ));
        match create_temporary(&candidate).map_err(|e| format!("can't write {path}: {e}"))? {
            Some(file) => break (candidate, file),
            // A taken name means someone else holds it: draw the next sequence value and retry
            // instead of truncating.
            None => continue,
        }
    };
    let written = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        #[cfg(unix)]
        copy_target_permissions(target, &temporary)?;
        file.sync_all()
    })();
    written.map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("can't write {path}: {e}")
    })?;
    std::fs::rename(&temporary, target).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("can't write {path}: {e}")
    })
}

/// Opens the temporary file `write_atomically` writes into.
///
/// `File::create` would give the temporary the umask-dependent default mode, and renaming it over
/// an existing `0600` workbook would then widen that workbook's permissions. Unix temporaries
/// therefore start at `0600` and only ever inherit the target's own permissions. Creation is
/// exclusive (`create_new`): `Ok(None)` means this attempt's unpredictable name is already taken,
/// so the caller retries with another one instead of truncating it.
#[cfg(unix)]
fn create_temporary(temporary: &std::path::Path) -> std::io::Result<Option<std::fs::File>> {
    use std::os::unix::fs::OpenOptionsExt;

    match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(temporary) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(error),
    }
}

/// Opens the temporary file `write_atomically` writes into, with the platform default mode.
/// Creation is exclusive like on Unix: a taken name retries instead of truncating.
#[cfg(not(unix))]
fn create_temporary(temporary: &std::path::Path) -> std::io::Result<Option<std::fs::File>> {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(temporary) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(error),
    }
}

/// Gives `temporary` the permissions of an existing `target`, so an in-place rewrite keeps the
/// workbook's mode instead of the temporary's `0600`.
#[cfg(unix)]
fn copy_target_permissions(target: &std::path::Path, temporary: &std::path::Path) -> std::io::Result<()> {
    match std::fs::metadata(target) {
        Ok(metadata) => std::fs::set_permissions(temporary, metadata.permissions()),
        // A new output file keeps the restrictive mode the temporary was created with.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Reads one zip entry, refusing to decompress past [`MAX_ENTRY_BYTES`].
fn read_zip_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let mut entry = archive.by_name(name).map_err(|e| format!("missing {name}: {e}"))?;
    if entry.size() > MAX_ENTRY_BYTES as u64 {
        return Err(format!("{name} exceeds the supported size limit"));
    }
    let mut content = Vec::new();
    let mut bounded = (&mut entry).take(MAX_ENTRY_BYTES as u64 + 1);
    let read = bounded.read_to_end(&mut content).map_err(|e| format!("can't read {name}: {e}"))?;
    if read > MAX_ENTRY_BYTES {
        return Err(format!("{name} exceeds the supported size limit"));
    }
    Ok(content)
}

/// Rewrites one entry of the package, copying the other entries and charging every decompressed byte
/// against [`MAX_PACKAGE_BYTES`]. Fails when the entry is missing.
fn replace_zip_entry(bytes: &[u8], name: &str, content: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut replaced = false;
    // Decompressed bytes written so far; the whole rewritten package is held in memory.
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("can't read zip entry: {e}"))?;
        let entry_name = entry.name().to_string();
        if entry_name == name {
            charge_package(&mut total, content.len() as u64, name)?;
            writer.start_file(name, options).map_err(|e| format!("can't write {name}: {e}"))?;
            writer.write_all(content).map_err(|e| format!("can't write {name}: {e}"))?;
            replaced = true;
        } else if entry.is_dir() {
            writer.add_directory(entry_name, options).map_err(|e| format!("can't copy zip directory: {e}"))?;
        } else {
            if entry.size() > MAX_ENTRY_BYTES as u64 {
                return Err(format!("can't copy {entry_name}: exceeds the supported size limit"));
            }
            let remaining = (MAX_PACKAGE_BYTES as u64).saturating_sub(total);
            writer.start_file(&entry_name, options).map_err(|e| format!("can't copy {entry_name}: {e}"))?;
            let copied = copy_entry(&mut entry, &mut writer, &entry_name, remaining)?;
            charge_package(&mut total, copied, &entry_name)?;
        }
    }
    if !replaced {
        return Err(format!("missing {name}"));
    }
    let cursor = writer.finish().map_err(|e| format!("can't finish zip: {e}"))?;
    Ok(cursor.into_inner())
}

/// Charges `bytes` against the running decompressed total of the rewritten package.
fn charge_package(total: &mut u64, bytes: u64, name: &str) -> Result<(), String> {
    *total = total.saturating_add(bytes);
    if *total > MAX_PACKAGE_BYTES as u64 {
        return Err(format!("can't copy {name}: package exceeds the supported size limit of {MAX_PACKAGE_BYTES} bytes"));
    }
    Ok(())
}

/// Copies one archive entry into `writer` and returns the actual bytes copied.
///
/// `remaining` is the decompressed budget left for the whole package. Copying stops at the smaller
/// of [`MAX_ENTRY_BYTES`] and `remaining` so it cannot write past either bound: a zip bomb declares
/// a small compressed entry that expands without bound, and the declared size checked by the caller
/// can lie. Both limits are verified against the bytes actually copied, not the declared size.
fn copy_entry(entry: &mut impl Read, writer: &mut impl Write, name: &str, remaining: u64) -> Result<u64, String> {
    let per_entry = MAX_ENTRY_BYTES as u64;
    let allowance = per_entry.min(remaining);
    let copied = std::io::copy(&mut (&mut *entry).take(allowance), writer).map_err(|e| format!("can't copy {name}: {e}"))?;
    if copied == allowance {
        // Exactly at the bound: probe for one more byte without writing it, to tell "fits" from
        // "lies about its size".
        let mut probe = [0u8; 1];
        let extra = entry.read(&mut probe).map_err(|e| format!("can't copy {name}: {e}"))?;
        if extra > 0 {
            if allowance == per_entry {
                return Err(format!("can't copy {name}: exceeds the supported size limit"));
            }
            return Err(format!("can't copy {name}: package exceeds the supported size limit of {MAX_PACKAGE_BYTES} bytes"));
        }
    }
    Ok(copied)
}

/// Renders a scalar as the JSON value the CLI prints, mapping blank to `null`.
///
/// JSON has no infinity: `serde_json` would print a non-finite number as `null`, which is also how
/// a blank is printed, so an overflow or a division by zero would look like an empty result. Those
/// values are reported instead.
fn scalar_json(value: &gridcraft_dax::Scalar) -> Result<serde_json::Value, String> {
    if let gridcraft_dax::Scalar::Decimal(number) = value
        && !number.is_finite()
    {
        return Err(format!("result is not a finite number: {number}"));
    }
    match value {
        gridcraft_dax::Scalar::Blank => Ok(serde_json::Value::Null),
        other => serde_json::to_value(other).map_err(|error| format!("can't serialize result: {error}")),
    }
}

/// Renders a result table's cells as JSON, reporting the first cell [`scalar_json`] rejects.
fn table_rows_json(table: &TableValue) -> Result<Vec<Vec<serde_json::Value>>, String> {
    table.rows.iter().map(|row| row.iter().map(scalar_json).collect()).collect()
}

enum Parsed {
    Query(Query),
    Expression(Expr),
}

/// Reads `TEXT`, `--file PATH`, or stdin (in that order of preference). Multiple positionals
/// are joined with spaces, so an unquoted `dax format 1 + 2` works.
fn read_input(args: &[String]) -> Result<String, String> {
    let parsed = Args::parse(args, &["file"], &[])?;
    if let Some(path) = parsed.opt("file") {
        if !parsed.positional.is_empty() {
            return Err("give either TEXT or --file, not both".into());
        }
        return std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"));
    }
    if !parsed.positional.is_empty() {
        return Ok(parsed.positional.join(" "));
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|e| format!("can't read stdin: {e}"))?;
    Ok(text)
}

/// Queries are tried first; a malformed `EVALUATE`/`DEFINE` reports its query error instead of
/// a misleading expression error.
fn parse_any(text: &str) -> Result<Parsed, String> {
    match parse_query(text) {
        Ok(query) => Ok(Parsed::Query(query)),
        Err(query_error) => {
            if starts_with_query_keyword(text) {
                return Err(query_error.to_string());
            }
            parse_expression(text).map(Parsed::Expression).map_err(|e| e.to_string())
        }
    }
}

/// Whether `text` opens with `EVALUATE` or `DEFINE`, so a malformed query reports its query error.
fn starts_with_query_keyword(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| match tokens.first().map(|token| &token.kind) {
        Some(TokenKind::Evaluate) => true,
        Some(TokenKind::Ident(word)) => word.eq_ignore_ascii_case("DEFINE"),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_entry_is_replaced_without_dropping_other_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer.start_file("xl/model/item.data", options)?;
        writer.write_all(b"old")?;
        writer.start_file("xl/workbook.xml", options)?;
        writer.write_all(b"<workbook/>")?;
        let bytes = writer.finish()?.into_inner();

        let replaced = replace_zip_entry(&bytes, "xl/model/item.data", b"new")?;
        assert_eq!(read_zip_entry(&replaced, "xl/model/item.data")?, b"new");
        assert_eq!(read_zip_entry(&replaced, "xl/workbook.xml")?, b"<workbook/>");
        Ok(())
    }

    /// Builds a one-entry stored zip whose headers claim `declared` uncompressed bytes for `data`.
    fn zip_declaring_size(name: &str, data: &[u8], declared: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes()); // version needed
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        bytes.extend_from_slice(&0u16.to_le_bytes()); // modification time
        bytes.extend_from_slice(&0u16.to_le_bytes()); // modification date
        bytes.extend_from_slice(&0u32.to_le_bytes()); // CRC-32
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes()); // compressed size
        bytes.extend_from_slice(&declared.to_le_bytes()); // uncompressed size
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // extra length
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(data);
        let directory_at = bytes.len() as u32;

        let mut directory = Vec::new();
        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&20u16.to_le_bytes()); // version made by
        directory.extend_from_slice(&20u16.to_le_bytes()); // version needed
        directory.extend_from_slice(&0u16.to_le_bytes()); // flags
        directory.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        directory.extend_from_slice(&0u16.to_le_bytes()); // modification time
        directory.extend_from_slice(&0u16.to_le_bytes()); // modification date
        directory.extend_from_slice(&0u32.to_le_bytes()); // CRC-32
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes()); // compressed size
        directory.extend_from_slice(&declared.to_le_bytes()); // uncompressed size
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // extra length
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        directory.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        directory.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        directory.extend_from_slice(name.as_bytes());
        let directory_size = directory.len() as u32;

        bytes.extend_from_slice(&directory);
        bytes.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // this disk
        bytes.extend_from_slice(&0u16.to_le_bytes()); // central directory disk
        bytes.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
        bytes.extend_from_slice(&1u16.to_le_bytes()); // entries in total
        bytes.extend_from_slice(&directory_size.to_le_bytes());
        bytes.extend_from_slice(&directory_at.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // comment length
        bytes
    }

    #[test]
    fn an_entry_declaring_past_the_bound_is_rejected_before_it_is_read() {
        let declared = (MAX_ENTRY_BYTES + 1) as u32;
        let bomb = zip_declaring_size("xl/other.bin", b"tiny", declared);
        let error = read_zip_entry(&bomb, "xl/other.bin").unwrap_err();
        assert!(error.contains("exceeds the supported size limit"), "{error}");

        let error = replace_zip_entry(&bomb, "xl/model/item.data", b"x").unwrap_err();
        assert!(error.contains("exceeds the supported size limit"), "{error}");
    }

    #[test]
    fn a_stream_past_the_bound_is_rejected_rather_than_truncated() {
        let error = copy_entry(&mut std::io::repeat(0), &mut std::io::sink(), "bomb", MAX_PACKAGE_BYTES as u64).unwrap_err();
        assert!(error.contains("exceeds the supported size limit"), "{error}");
    }

    #[test]
    fn copy_entry_reports_the_bytes_it_copied() -> Result<(), Box<dyn std::error::Error>> {
        let copy = |data: &[u8], remaining: u64| {
            let mut source = Cursor::new(data.to_vec());
            copy_entry(&mut source, &mut std::io::sink(), "entry", remaining)
        };

        assert_eq!(copy(b"hello", MAX_PACKAGE_BYTES as u64)?, 5);
        // Exactly filling the remaining budget is allowed; the probe must not count as copied.
        assert_eq!(copy(b"12345678", 8)?, 8);
        assert_eq!(copy(b"", 0)?, 0);
        Ok(())
    }

    #[test]
    fn copying_past_the_remaining_package_budget_is_rejected() {
        let error = copy_entry(&mut std::io::repeat(0), &mut std::io::sink(), "bomb", 16).unwrap_err();
        assert!(error.contains("package exceeds the supported size limit"), "{error}");
        assert!(error.contains(&MAX_PACKAGE_BYTES.to_string()), "{error}");
    }

    #[test]
    fn the_package_budget_is_charged_across_entries() {
        let mut total = 0u64;
        assert_eq!(charge_package(&mut total, MAX_ENTRY_BYTES as u64, "a"), Ok(()));
        assert_eq!(charge_package(&mut total, MAX_ENTRY_BYTES as u64, "b"), Ok(()));
        assert_eq!(total, MAX_PACKAGE_BYTES as u64);

        let error = charge_package(&mut total, 1, "c").unwrap_err();
        assert!(error.contains("package exceeds the supported size limit"), "{error}");
        assert!(error.contains(&MAX_PACKAGE_BYTES.to_string()), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_in_place_keeps_the_workbook_permissions() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!("gridcraft-cli-permissions-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("book.xlsx");
        std::fs::write(&path, b"old")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

        let written = write_atomically(path.to_str().ok_or("temporary path is not UTF-8")?, b"new");
        assert_eq!(written, Ok(()));
        assert_eq!(std::fs::read(&path)?, b"new");
        assert_eq!(std::fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(&directory)?;
        Ok(())
    }
    #[test]
    fn exclusive_creation_never_truncates_a_taken_name() -> Result<(), Box<dyn std::error::Error>> {
        // A sibling writer (or a planted link) may already hold the file the writer would create:
        // `create_new` reports it instead of truncating it. Pinning the whole unpredictable name
        // is impractical, so this test pins the mechanism directly: a taken path reports `None`
        // (the retry signal) with the planted bytes intact, and a free path is created.
        let directory = std::env::temp_dir().join(format!("gridcraft-cli-exclusive-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let taken = directory.join("taken.tmp");
        std::fs::write(&taken, b"planted")?;
        assert!(create_temporary(&taken)?.is_none(), "a taken name reports instead of truncating");
        assert_eq!(std::fs::read(&taken)?, b"planted");
        let free = directory.join("free.tmp");
        let created = create_temporary(&free)?;
        assert!(created.is_some(), "a free name is created");
        drop(created);
        assert!(std::fs::read_dir(&directory)?.count() == 2, "only the planted file and the new temporary remain");
        std::fs::remove_dir_all(&directory)?;
        Ok(())
    }
}
