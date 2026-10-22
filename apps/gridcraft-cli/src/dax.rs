//! `gridcraft-cli dax`: format, dump or bind DAX text.

use std::io::{Cursor, Read, Write};

use gridcraft_dax::model::DataModel;
use gridcraft_dax::{
    Binder, Evaluator, Expr, ModelMeasure, Query, TokenKind, parse_expression, parse_query, pivot_cross, pivot_fields, print_expression, print_query,
    read_measures, tokenize, write_measures,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{Args, USAGE};

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

fn format(args: &[String]) -> Result<(), String> {
    let text = read_input(args)?;
    match parse_any(&text)? {
        Parsed::Query(query) => outln!("{}", print_query(&query)),
        Parsed::Expression(expr) => outln!("{}", print_expression(&expr)),
    }
    Ok(())
}

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
    let path = parsed.opt("model").ok_or("dax pivot needs --model PATH")?;
    let measure = parsed.opt("measure").ok_or("dax pivot needs --measure NAME")?;
    let rows = parsed.opt("rows").ok_or("dax pivot needs --rows 'Sales[Region]'")?;
    let json = std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let mut model: DataModel = serde_json::from_str(&json).map_err(|e| format!("invalid model JSON: {e}"))?;
    model.parse_formulas().map_err(|e| e.to_string())?;
    let fields = pivot_fields(&model, &rows.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    let columns = parsed
        .opt("columns")
        .map(|columns| pivot_fields(&model, &columns.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>()));
    let column_fields = match columns {
        Some(Ok(fields)) => fields,
        Some(Err(error)) => return Err(error.to_string()),
        None => Vec::new(),
    };
    let table = pivot_cross(&model, measure, &fields, &column_fields).map_err(|e| e.to_string())?;
    let columns: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
    let rows: Vec<Vec<serde_json::Value>> = table.rows.iter().map(|row| row.iter().map(scalar_json).collect()).collect();
    outln!("{}", serde_json::json!({"columns": columns, "rows": rows}));
    Ok(())
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
        let value = evaluator.evaluate_measure(name, Default::default()).map_err(|e| e.to_string())?;
        outln!("{}", scalar_json(&value));
        return Ok(());
    }

    let text = read_input_with(args, &["file", "model"], &["table"])?;
    let expr = parse_expression(&text).map_err(|e| e.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|e| e.to_string())?;
    if parsed.has("table") {
        let table = evaluator.evaluate_table(&bound).map_err(|e| e.to_string())?;
        let columns: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
        let rows: Vec<Vec<serde_json::Value>> = table.rows.iter().map(|row| row.iter().map(scalar_json).collect()).collect();
        outln!("{}", serde_json::json!({"columns": columns, "rows": rows}));
    } else {
        let value = evaluator.evaluate(&bound).map_err(|e| e.to_string())?;
        outln!("{}", scalar_json(&value));
    }
    Ok(())
}

/// Reads or rewrites Power Pivot measures in `xl/model/item.data` inside an `.xlsx` package.
fn measures(args: &[String]) -> Result<(), String> {
    let parsed = Args::parse(args, &["file", "write", "out"], &[])?;
    let path = parsed.opt("file").ok_or("dax measures needs --file PATH")?;
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
    std::fs::write(out, package).map_err(|e| format!("can't write {out}: {e}"))?;
    outln!("{}", serde_json::to_string_pretty(&measures).map_err(|e| e.to_string())?);
    Ok(())
}

fn read_zip_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let mut entry = archive.by_name(name).map_err(|e| format!("missing {name}: {e}"))?;
    let mut content = Vec::new();
    entry.read_to_end(&mut content).map_err(|e| format!("can't read {name}: {e}"))?;
    Ok(content)
}

fn replace_zip_entry(bytes: &[u8], name: &str, content: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut replaced = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("can't read zip entry: {e}"))?;
        let entry_name = entry.name().to_string();
        if entry_name == name {
            writer.start_file(name, options).map_err(|e| format!("can't write {name}: {e}"))?;
            writer.write_all(content).map_err(|e| format!("can't write {name}: {e}"))?;
            replaced = true;
        } else if entry.is_dir() {
            writer.add_directory(entry_name, options).map_err(|e| format!("can't copy zip directory: {e}"))?;
        } else {
            writer.start_file(&entry_name, options).map_err(|e| format!("can't copy {entry_name}: {e}"))?;
            std::io::copy(&mut entry, &mut writer).map_err(|e| format!("can't copy {entry_name}: {e}"))?;
        }
    }
    if !replaced {
        return Err(format!("missing {name}"));
    }
    let cursor = writer.finish().map_err(|e| format!("can't finish zip: {e}"))?;
    Ok(cursor.into_inner())
}

fn scalar_json(value: &gridcraft_dax::Scalar) -> serde_json::Value {
    match value {
        gridcraft_dax::Scalar::Blank => serde_json::Value::Null,
        other => serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
    }
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
}
