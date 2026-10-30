//! Data Model (Power Pivot prototype) commands: load a JSON model, edit its measures and
//! relationships, evaluate DAX, and group a measure into a pivot-shaped table.
//!
//! The editing commands mutate `Session::dax_model` / `Session::dax_view` / `Session::dax_metadata`
//! — application state, not the workbook — so they record no undo step; each one revalidates the
//! whole model through [`DataModel::parse_formulas`] before it is stored, so a failed edit leaves
//! the model untouched.
//!
//! `dataModel.sortTable`, `dataModel.setFilter` and their siblings change the window's *view*
//! state, never the model: they are not model data, are not saved and are reset by
//! `dataModel.load`. Hidden columns, date tables and default summarization are model metadata;
//! they travel in the model JSON (`hidden`, `date_column`, `summarize_by`) and are read and
//! validated by [`ModelMetadata`] at the load boundary.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use gridcraft_core::date::{datetime_from_serial, serial_from_ymd};
use gridcraft_core::{DateSystem, Value as CoreValue};
use gridcraft_dax::model::{ColumnMetadata, ModelMetadata, summarize_by_name};
use gridcraft_dax::{
    Binder, DataModel, Evaluator, Expr, ExprKind, Measure, Relationship, Scalar, ScalarKey, parse_expression, pivot_cross, pivot_fields,
};
use gridcraft_numfmt::{NumberFormat, format_value};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, always, bad, bool_param, cmd, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "dataModel.status", "Data Model", ["Data", "Data Model"], None, "{} → {tables, measures}", always, status),
        cmd!("dataModel.load", "Load Data Model", ["Data", "Data Model"], None, "{path | json | model} → {tables, measures}", always, load),
        cmd!(
            query
            "dataModel.evaluate",
            "Evaluate DAX",
            ["Data", "Data Model"],
            None,
            "{formula?, measure?, table?: bool} → scalar or table",
            always,
            evaluate
        ),
        cmd!(
            query
            "dataModel.pivot",
            "Data Model Pivot",
            ["Data", "Data Model"],
            None,
            "{measure, rows: [\"Sales[Region]\"]} → {columns, rows}",
            always,
            pivot_cmd
        ),
        cmd!(
            "dataModel.addMeasure",
            "Add Measure",
            ["Data", "Data Model"],
            None,
            "{name, formula, table?, format?} → status",
            has_model,
            add_measure
        ),
        cmd!(
            "dataModel.updateMeasure",
            "Update Measure",
            ["Data", "Data Model"],
            None,
            "{name, formula?, format?, new_name?} → status",
            has_model,
            update_measure
        ),
        cmd!("dataModel.deleteMeasure", "Delete Measure", ["Data", "Data Model"], None, "{name} → status", has_model, delete_measure),
        cmd!(
            "dataModel.addRelationship",
            "Create Relationship",
            ["Data", "Data Model"],
            None,
            "{from_table, from_column, to_table, to_column, bidirectional?, inactive?} → status",
            has_model,
            add_relationship
        ),
        cmd!(
            "dataModel.deleteRelationship",
            "Delete Relationship",
            ["Data", "Data Model"],
            None,
            "{from_table, from_column, to_table, to_column} → status",
            has_model,
            delete_relationship
        ),
        // ---------------------------------------------------------------- view state
        cmd!(
            query
            "dataModel.view",
            "Data View Rows",
            ["Data", "Data Model"],
            None,
            "{table, show_hidden?} → {columns, rows, text_rows, row_numbers, hidden, sort, filters}",
            has_model,
            view
        ),
        cmd!(
            query
            "dataModel.find",
            "Find in Data Model",
            ["Data", "Data Model"],
            None,
            "{query, table?, column?, case?} → {matches: [{table, column, row, value}]}",
            has_model,
            find
        ),
        cmd!(
            noundo
            "dataModel.sortTable",
            "Sort Data View",
            ["Data", "Data Model"],
            None,
            "{table, column, descending?: bool} → view state (session only)",
            has_model,
            sort_table
        ),
        cmd!(
            noundo
            "dataModel.clearSort",
            "Clear Data View Sort",
            ["Data", "Data Model"],
            None,
            "{table} → view state (session only)",
            has_model,
            clear_sort
        ),
        cmd!(
            noundo
            "dataModel.setFilter",
            "Filter Data View",
            ["Data", "Data Model"],
            None,
            "{table, column, values: [text] (empty clears)} → view state (session only)",
            has_model,
            set_filter
        ),
        cmd!(
            noundo
            "dataModel.clearFilters",
            "Clear Data View Filters",
            ["Data", "Data Model"],
            None,
            "{table} → view state (session only)",
            has_model,
            clear_filters
        ),
        cmd!(
            noundo
            "dataModel.sortByColumn",
            "Sort by Column",
            ["Data", "Data Model"],
            None,
            "{table, by_table, by_column, column?, descending?: bool} → view state (session only)",
            has_model,
            sort_by_column
        ),
        cmd!(
            noundo
            "dataModel.setShowHidden",
            "Show Hidden Columns",
            ["Data", "Data Model"],
            None,
            "{show: bool} → status (window view state)",
            has_model,
            set_show_hidden
        ),
        cmd!(
            noundo
            "dataModel.setShowImplicitMeasures",
            "Show Implicit Measures",
            ["Data", "Data Model"],
            None,
            "{show: bool} → status (window view state)",
            has_model,
            set_show_implicit_measures
        ),
        // ---------------------------------------------------------------- model metadata
        cmd!(
            noundo
            "dataModel.setFormat",
            "Format Measure",
            ["Data", "Data Model"],
            None,
            "{name, format?, preset?, decimals?} → status",
            has_model,
            set_format
        ),
        cmd!(
            noundo
            "dataModel.setColumnHidden",
            "Hide Column",
            ["Data", "Data Model"],
            None,
            "{table, column, hidden?: bool} → status",
            has_model,
            set_column_hidden
        ),
        cmd!(
            noundo
            "dataModel.setColumnType",
            "Data Type",
            ["Data", "Data Model"],
            None,
            "{table, column, data_type} → status (rejects a value the type cannot hold)",
            has_model,
            set_column_type
        ),
        cmd!(
            noundo
            "dataModel.setSummarizeBy",
            "Summarize By",
            ["Data", "Data Model"],
            None,
            "{table, column, summarize_by: Sum|Average|Count|Min|Max|None|null} → status",
            has_model,
            set_summarize_by
        ),
        cmd!(
            noundo
            "dataModel.markDateTable",
            "Mark as Date Table",
            ["Data", "Data Model"],
            None,
            "{table, column} → status (the column must be contiguous unique dates)",
            has_model,
            mark_date_table
        ),
        cmd!(
            noundo
            "dataModel.unmarkDateTable",
            "Unmark Date Table",
            ["Data", "Data Model"],
            None,
            "{table} → status",
            has_model,
            unmark_date_table
        ),
        cmd!(
            query
            "dataModel.validate",
            "Validate Data Model",
            ["Data", "Data Model"],
            None,
            "{} → {tables, rows, relationships, findings}",
            has_model,
            validate
        ),
    ]
}

/// Measure and relationship commands need a model, exactly as the Power Pivot window does: they
/// are greyed out until `dataModel.load` (or a workbook adapter) fills `Session::dax_model`.
fn has_model(s: &Session) -> std::result::Result<(), String> {
    if s.dax_model.is_some() { Ok(()) } else { Err("no data model loaded".into()) }
}

// ---------------------------------------------------------------- window state

/// `Session::dax_view`: the Data View's sorts, filters and visibility toggles.
///
/// This is **view state, not model data**: it changes what the window shows and what
/// `dataModel.view` returns, never the model, and it is reset by `dataModel.load`. Nothing here is
/// saved in the workbook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelView {
    /// Show columns marked hidden (`dataModel.setShowHidden`).
    pub show_hidden: bool,
    /// Expose implicit measures and resolve them in `dataModel.evaluate`
    /// (`dataModel.setShowImplicitMeasures`).
    pub show_implicit_measures: bool,
    /// Per table, keyed by lowercased name.
    pub tables: BTreeMap<String, TableView>,
}

/// One table's view settings; both are session state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableView {
    pub sort: Option<TableSort>,
    pub filters: Vec<ColumnFilter>,
}

/// How the Data View orders a table. With `by_table`/`by_column` set the sort key is a column of a
/// related table, reached through the relationship that joins the two (`dataModel.sortByColumn`).
#[derive(Clone, Debug, PartialEq)]
pub struct TableSort {
    /// The sorted table's column: the sort key itself, or the key column of the relationship when
    /// the sort follows a related table.
    pub column: String,
    pub by_table: Option<String>,
    pub by_column: Option<String>,
    pub descending: bool,
}

/// One column's allowed values. A row shows when every filter table keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnFilter {
    pub column: String,
    pub values: Vec<String>,
}

/// The most rows `dataModel.view` returns; the Data View draws fewer. Filters and the sort apply
/// first, so the cap never changes the order.
const MAX_VIEW_ROWS: usize = 5_000;
/// The most matches `dataModel.find` returns.
const MAX_FIND_MATCHES: usize = 500;
/// The most allowed values one column filter keeps.
const MAX_FILTER_VALUES: usize = 1_000;
/// The longest format code `dataModel.setFormat` accepts.
const MAX_FORMAT_LEN: usize = 128;

fn sort_json(sort: Option<&TableSort>) -> Json {
    match sort {
        None => Json::Null,
        Some(sort) => json!({
            "column": sort.column,
            "by_table": sort.by_table,
            "by_column": sort.by_column,
            "descending": sort.descending,
        }),
    }
}

fn filters_json(filters: &[ColumnFilter]) -> Json {
    Json::Array(filters.iter().map(|filter| json!({"column": filter.column, "values": filter.values})).collect())
}

/// A model value as the Data View paints it and as a filter compares it: blank for BLANK, Excel
/// booleans, no trailing `.0`, and Currency as its decimal amount rather than its scaled integer.
fn scalar_text(value: &Scalar) -> String {
    match value {
        Scalar::Blank => String::new(),
        Scalar::Boolean(value) => if *value { "TRUE" } else { "FALSE" }.to_string(),
        Scalar::Integer(value) => value.to_string(),
        Scalar::Currency(value) => number_text(*value as f64 / 10_000.0),
        Scalar::Decimal(value) => number_text(*value),
        Scalar::DateTime(value) => number_text(*value),
        Scalar::String(value) => value.clone(),
    }
}

/// A number the way the Data View prints it: integers without `.0`, everything else as Rust's
/// shortest round-trip text (the same text `serde_json` writes).
fn number_text(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".into()
        } else if value > 0.0 {
            "INF".into()
        } else {
            "-INF".into()
        };
    }
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{value:.0}") } else { value.to_string() }
}

/// The value of a row cell, `BLANK` when the row is shorter than the table's columns.
fn cell(rows: &[Vec<Scalar>], row: usize, column: usize) -> Scalar {
    rows.get(row).and_then(|row| row.get(column)).cloned().unwrap_or(Scalar::Blank)
}

// ---------------------------------------------------------------- model types

/// A model column type the engine can check stored values against. Model type names are a
/// different vocabulary from DAX expression types, and `parse` accepts Excel's names for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelType {
    Int64,
    Double,
    Decimal,
    Currency,
    String,
    Boolean,
    DateTime,
}

impl ModelType {
    fn parse(name: &str) -> Option<ModelType> {
        let key: String = name.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        Some(match key.as_str() {
            "int64" | "int" | "integer" | "wholenumber" => ModelType::Int64,
            "double" | "number" | "decimalnumber" | "float" | "real" => ModelType::Double,
            "decimal" | "fixeddecimal" => ModelType::Decimal,
            "currency" | "fixeddecimalnumber" | "money" => ModelType::Currency,
            "string" | "text" => ModelType::String,
            "boolean" | "bool" | "truefalse" => ModelType::Boolean,
            "datetime" | "date" => ModelType::DateTime,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            ModelType::Int64 => "Int64",
            ModelType::Double => "Double",
            ModelType::Decimal => "Decimal",
            ModelType::Currency => "Currency",
            ModelType::String => "String",
            ModelType::Boolean => "Boolean",
            ModelType::DateTime => "DateTime",
        }
    }

    /// Numbers default to Sum; text, dates and booleans default to Count (Excel's default
    /// summarization).
    fn is_numeric(self) -> bool {
        matches!(self, ModelType::Int64 | ModelType::Double | ModelType::Decimal | ModelType::Currency)
    }

    /// Whether one stored value can be held as this type. `dataModel.setColumnType` validates the
    /// whole column before it commits, so a rejected value leaves the type unchanged.
    fn accepts(self, value: &Scalar) -> bool {
        let integral_in_range = |number: f64| number.is_finite() && number.fract() == 0.0 && number >= -(2f64.powi(63)) && number < 2f64.powi(63);
        let scaled_in_range = |number: f64| {
            let scaled = number * 10_000.0;
            scaled.is_finite() && scaled >= -(2f64.powi(63)) && scaled < 2f64.powi(63)
        };
        match self {
            ModelType::String => true,
            ModelType::Boolean => match value {
                Scalar::Blank | Scalar::Boolean(_) => true,
                Scalar::Integer(number) => *number == 0 || *number == 1,
                Scalar::Decimal(number) => *number == 0.0 || *number == 1.0,
                _ => false,
            },
            ModelType::Int64 => match value {
                Scalar::Blank | Scalar::Integer(_) | Scalar::Boolean(_) => true,
                Scalar::Decimal(number) => integral_in_range(*number),
                Scalar::Currency(number) => number % 10_000 == 0,
                _ => false,
            },
            ModelType::Double | ModelType::Decimal => match value {
                Scalar::Blank | Scalar::Integer(_) | Scalar::Boolean(_) | Scalar::Currency(_) => true,
                Scalar::Decimal(number) => number.is_finite(),
                _ => false,
            },
            ModelType::Currency => match value {
                Scalar::Blank | Scalar::Currency(_) => true,
                Scalar::Integer(number) => number.checked_mul(10_000).is_some(),
                Scalar::Boolean(_) => true,
                Scalar::Decimal(number) => scaled_in_range(*number),
                _ => false,
            },
            ModelType::DateTime => match value {
                Scalar::Blank | Scalar::DateTime(_) => true,
                Scalar::String(text) => iso_date_serial(text).is_some(),
                _ => false,
            },
        }
    }
}

/// The OLE Automation serial of an ISO `YYYY-MM-DD` date (an optional midnight time part is
/// accepted), as a model file may store one. `None` for anything else, including an out-of-range
/// day such as `2024-02-31`.
fn iso_date_serial(text: &str) -> Option<f64> {
    let text = text.trim();
    let date = text.get(..10)?;
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: i64 = date.get(5..7)?.parse().ok()?;
    let day: i64 = date.get(8..10)?.parse().ok()?;
    // Serial dates start in 1900; `serial_from_ymd` applies DATE()'s 1900 offset below that, which
    // would silently reinterpret an ISO year, so reject those.
    if !(1900..=9999).contains(&year) {
        return None;
    }
    if let Some(rest) = text.get(10..) {
        let rest = rest.trim_start_matches(['T', ' ']);
        if !matches!(rest, "" | "00:00" | "00:00:00") {
            return None;
        }
    }
    let serial = serial_from_ymd(DateSystem::D1900, year, month, day)?;
    let parsed = datetime_from_serial(DateSystem::D1900, serial)?;
    if parsed.year != year as i32 || parsed.month != month as u32 || parsed.day != day as u32 {
        return None;
    }
    Some(serial)
}

// ---------------------------------------------------------------- formats

/// The Excel format code behind Excel's named model formats, or `None` when the value is not a
/// preset (the caller then treats it as a format code).
fn preset_code(name: &str) -> Option<&'static str> {
    let key: String = name.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).collect::<String>().to_ascii_lowercase();
    match key.as_str() {
        "currency" => Some("$#,##0.00"),
        "percent" | "percentage" => Some("0.00%"),
        "comma" | "commastyle" => Some("#,##0.00"),
        "decimal" | "number" => Some("0.00"),
        "general" => Some("General"),
        _ => None,
    }
}

/// A model value as the window shows it under a measure's format. Text and BLANK are unchanged;
/// a number goes through the workbook's number-format engine, which is what the grid uses.
fn display_scalar(value: &Scalar, format: Option<&str>) -> String {
    let core_value = match value {
        Scalar::Blank | Scalar::String(_) => return scalar_text(value),
        Scalar::Boolean(value) => CoreValue::Bool(*value),
        Scalar::Integer(value) => CoreValue::Number(*value as f64),
        Scalar::Currency(value) => CoreValue::Number(*value as f64 / 10_000.0),
        Scalar::Decimal(value) => CoreValue::Number(*value),
        Scalar::DateTime(value) => CoreValue::Number(*value),
    };
    let Some(format) = format.map(str::trim).filter(|format| !format.is_empty()) else {
        return scalar_text(value);
    };
    let number_format = NumberFormat::parse(preset_code(format).unwrap_or(format));
    if number_format.is_general() {
        return scalar_text(value);
    }
    format_value(&core_value, &number_format, DateSystem::D1900).text
}

/// Byte positions of the decimal placeholders after the decimal point (format codes are ASCII).
fn decimal_digits(head: &str, dot: usize) -> Vec<usize> {
    head.get(dot + 1..)
        .map(|rest| rest.char_indices().filter(|(_, c)| matches!(c, '0' | '#' | '?')).map(|(index, _)| dot + 1 + index).collect())
        .unwrap_or_default()
}

/// One more decimal place in the first format section.
fn increase_decimal(head: &mut String) {
    match head.find('.') {
        Some(dot) => {
            let digits = decimal_digits(head, dot);
            let at = digits.last().map(|index| index + 1).unwrap_or(dot + 1);
            head.insert(at, '0');
        }
        None => {
            let at = head.rfind(['0', '#', '?']).map(|index| index + 1).unwrap_or(head.len());
            head.insert_str(at, ".0");
        }
    }
}

/// One fewer decimal place in the first format section; at zero places the decimal point goes too,
/// as Excel does.
fn decrease_decimal(head: &mut String) {
    let Some(dot) = head.find('.') else { return };
    let digits = decimal_digits(head, dot);
    let Some(&last) = digits.last() else { return };
    head.remove(last);
    if decimal_digits(head, dot).is_empty() {
        head.remove(dot);
    }
}

/// Excel's Increase/Decrease Decimal on a format code: `delta` places more (or fewer), the rest of
/// the format (currency sign, thousands separator, percent sign) unchanged.
fn adjust_decimals(format: &str, delta: i32) -> String {
    let code = preset_code(format).unwrap_or(format).to_string();
    let first_end = code.find(';').unwrap_or(code.len());
    let (head, tail) = code.split_at(first_end);
    let mut head = head.to_string();
    for _ in 0..delta.unsigned_abs().min(10) {
        if delta > 0 {
            increase_decimal(&mut head);
        } else {
            decrease_decimal(&mut head);
        }
    }
    format!("{head}{tail}")
}

// ---------------------------------------------------------------- implicit measures

/// One of Excel's implicit measures: a column's default aggregate, exposed only while the window
/// shows implicit measures.
#[derive(Clone, Debug, PartialEq)]
struct ImplicitMeasure {
    name: String,
    table: String,
    column: String,
    aggregate: &'static str,
    formula: String,
}

/// A DAX table reference for a model name, quoted when the name is not a plain identifier.
fn dax_table_ref(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if plain { name.to_string() } else { format!("'{}'", name.replace('\'', "''")) }
}

/// A DAX column reference for a model name.
fn dax_column_ref(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

/// The aggregate Excel summarises a column by when the model does not say: numbers sum,
/// everything else counts.
fn default_summarize(column: &gridcraft_dax::ModelColumn, metadata: Option<&ColumnMetadata>) -> &'static str {
    if let Some(name) = metadata.and_then(|metadata| metadata.summarize_by.as_deref())
        && let Some(canonical) = summarize_by_name(name)
    {
        return canonical;
    }
    match column.data_type.as_deref().and_then(ModelType::parse) {
        Some(kind) if kind.is_numeric() => "Sum",
        _ => "Count",
    }
}

/// Every implicit measure the model exposes: one per visible column with a summarization other
/// than `None`.
fn implicit_measure_list(model: &DataModel, metadata: &ModelMetadata) -> Vec<ImplicitMeasure> {
    let mut out = Vec::new();
    for table in &model.tables {
        for column in &table.columns {
            let column_metadata = metadata.column(&table.name, &column.name);
            if column_metadata.is_some_and(|metadata| metadata.hidden) {
                continue;
            }
            let aggregate = default_summarize(column, column_metadata);
            if aggregate == "None" {
                continue;
            }
            let table_ref = dax_table_ref(&table.name);
            let column_ref = dax_column_ref(&column.name);
            let formula = match aggregate {
                "Sum" => format!("SUM({table_ref}{column_ref})"),
                "Average" => format!("AVERAGE({table_ref}{column_ref})"),
                "Min" => format!("MIN({table_ref}{column_ref})"),
                "Max" => format!("MAX({table_ref}{column_ref})"),
                _ => format!("COUNTROWS({table_ref})"),
            };
            out.push(ImplicitMeasure {
                name: format!("{aggregate} of {}", column.name),
                table: table.name.clone(),
                column: column.name.clone(),
                aggregate,
                formula,
            });
        }
    }
    out
}

/// The model's status: tables, columns with their model metadata, measures, relationships, the
/// implicit measures when the window shows them, and the window's display toggles.
fn status(s: &mut Session, _: &Json) -> Result<Json> {
    let Some(model) = &s.dax_model else {
        return Ok(json!({"loaded": false, "tables": [], "measures": []}));
    };
    let model_metadata = &s.dax_metadata;
    let view = &s.dax_view;
    let tables: Vec<Json> = model
        .tables
        .iter()
        .map(|table| {
            let metadata = model_metadata.table(&table.name);
            json!({
                "name": table.name,
                "columns": table
                    .columns
                    .iter()
                    .map(|column| {
                        let column_metadata = model_metadata.column(&table.name, &column.name);
                        json!({
                            "name": column.name,
                            "type": column.data_type,
                            "hidden": column_metadata.is_some_and(|metadata| metadata.hidden),
                            "summarize_by": column_metadata.and_then(|metadata| metadata.summarize_by.clone()),
                        })
                    })
                    .collect::<Vec<_>>(),
                "rows": table.rows.len(),
                "date_column": metadata.and_then(|metadata| metadata.date_column.clone()),
            })
        })
        .collect();
    let measures: Vec<Json> = model
        .measures
        .iter()
        .map(|measure| json!({"name": measure.name, "table": measure.table, "formula": measure.formula, "format": measure.format}))
        .collect();
    let relationships: Vec<Json> = model
        .relationships
        .iter()
        .map(|relationship| {
            json!({
                "from_table": relationship.from_table,
                "from_column": relationship.from_column,
                "to_table": relationship.to_table,
                "to_column": relationship.to_column,
                "bidirectional": relationship.bidirectional,
                "inactive": relationship.inactive,
            })
        })
        .collect();
    let implicit: Vec<Json> = if view.show_implicit_measures {
        implicit_measure_list(model, model_metadata)
            .into_iter()
            .map(|measure| {
                json!({
                    "name": measure.name,
                    "table": measure.table,
                    "column": measure.column,
                    "aggregate": measure.aggregate,
                    "formula": measure.formula,
                    "format": Json::Null,
                    "implicit": true,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "loaded": true,
        "tables": tables,
        "measures": measures,
        "relationships": relationships,
        "implicit_measures": implicit,
        "view": {"show_hidden": view.show_hidden, "show_implicit_measures": view.show_implicit_measures},
    }))
}

fn load(s: &mut Session, p: &Json) -> Result<Json> {
    // Native hosts pass a path; browser hosts pass the JSON text or object they read from the
    // file picker (wasm has no filesystem).
    let text = if let Some(json) = p.get("json").and_then(Json::as_str) {
        json.to_string()
    } else if let Some(model) = p.get("model") {
        serde_json::to_string(model).map_err(|error| EngineError::Other(format!("invalid model JSON: {error}")))?
    } else if let Some(path) = p.get("path").and_then(Json::as_str) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read_to_string(path).map_err(|error| EngineError::Other(format!("can't read {path}: {error}")))?
        }
        #[cfg(target_arch = "wasm32")]
        {
            return Err(EngineError::Other(format!("dataModel.load cannot read the filesystem path {path} in the browser; pass `json` or `model`")));
        }
    } else {
        return Err(EngineError::Other("dataModel.load needs `path`, `json` or `model`".into()));
    };
    let mut model: DataModel = serde_json::from_str(&text).map_err(|error| EngineError::Other(format!("invalid model JSON: {error}")))?;
    model.parse_formulas().map_err(|error| EngineError::Other(error.to_string()))?;
    // The same document carries the model metadata the binder does not use. A document written
    // before those keys existed yields defaults, so it loads exactly as it did.
    let metadata = ModelMetadata::from_model_json(&text).map_err(|error| EngineError::Other(error.to_string()))?;
    s.dax_model = Some(model);
    // A new model starts with a fresh window: old sorts and filters name tables that may be gone.
    s.dax_view = ModelView::default();
    s.dax_metadata = metadata;
    status(s, &Json::Null)
}

fn evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let model = s.dax_model.as_ref().ok_or_else(|| EngineError::Other("no data model loaded".into()))?;
    let evaluator = Evaluator::new(model).map_err(|error| EngineError::Other(error.to_string()))?;
    if let Some(measure) = p.get("measure").and_then(Json::as_str) {
        if let Some(index) = model.measure_index(measure) {
            let value =
                evaluator.evaluate_measure(&model.measures[index].name, Default::default()).map_err(|error| EngineError::Other(error.to_string()))?;
            let format = model.measures[index].format.clone();
            return Ok(json!({
                "value": value_json(&value),
                "display": display_scalar(&value, format.as_deref()),
                "format": format,
            }));
        }
        // Implicit measures are not stored, so an explicit measure of the same name always wins.
        if s.dax_view.show_implicit_measures
            && let Some(implicit) = implicit_measure_list(model, &s.dax_metadata).into_iter().find(|entry| entry.name.eq_ignore_ascii_case(measure))
        {
            let expr = parse_expression(&implicit.formula).map_err(|error| EngineError::Other(error.to_string()))?;
            let bound = Binder::new(model).bind_expression(&expr).map_err(|error| EngineError::Other(error.to_string()))?;
            let value = evaluator.evaluate(&bound).map_err(|error| EngineError::Other(error.to_string()))?;
            return Ok(json!({
                "value": value_json(&value),
                "display": display_scalar(&value, None),
                "implicit": true,
                "aggregate": implicit.aggregate,
                "formula": implicit.formula,
            }));
        }
        return Err(EngineError::Other(format!("unknown measure `{measure}`")));
    }
    let formula =
        p.get("formula").and_then(Json::as_str).ok_or_else(|| EngineError::Other("dataModel.evaluate needs `formula` or `measure`".into()))?;
    let expr = parse_expression(formula).map_err(|error| EngineError::Other(error.to_string()))?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| EngineError::Other(error.to_string()))?;
    if p.get("table").and_then(Json::as_bool).unwrap_or(false) {
        let table = evaluator.evaluate_table(&bound).map_err(|error| EngineError::Other(error.to_string()))?;
        return Ok(table_json(&table));
    }
    let value = evaluator.evaluate(&bound).map_err(|error| EngineError::Other(error.to_string()))?;
    Ok(json!({"value": value_json(&value), "display": display_scalar(&value, None)}))
}

fn pivot_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let model = s.dax_model.as_ref().ok_or_else(|| EngineError::Other("no data model loaded".into()))?;
    let measure = p.get("measure").and_then(Json::as_str).ok_or_else(|| EngineError::Other("dataModel.pivot needs `measure`".into()))?;
    let rows: Vec<String> = p
        .get("rows")
        .and_then(Json::as_array)
        .map(|rows| rows.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .ok_or_else(|| EngineError::Other("dataModel.pivot needs `rows`".into()))?;
    let fields = pivot_fields(model, &rows).map_err(|error| EngineError::Other(error.to_string()))?;
    let columns: Vec<String> = p
        .get("columns")
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let column_fields = pivot_fields(model, &columns).map_err(|error| EngineError::Other(error.to_string()))?;
    let table = pivot_cross(model, measure, &fields, &column_fields).map_err(|error| EngineError::Other(error.to_string()))?;
    let mut out = table_json(&table);
    // The measure's format is applied to a parallel grid, so the numeric `rows` stay exact for
    // agents while the window and a reader of `display_rows` see what Excel would paint.
    if let Some(index) = model.measure_index(measure) {
        let format = model.measures[index].format.clone();
        let display: Vec<Json> = table
            .rows
            .iter()
            .map(|row| Json::Array(row.iter().map(|value| Json::String(display_scalar(value, format.as_deref()))).collect()))
            .collect();
        if let Some(object) = out.as_object_mut() {
            object.insert("format".into(), format.map(Json::String).unwrap_or(Json::Null));
            object.insert("display_rows".into(), Json::Array(display));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- model editing

/// Every editing command takes the current model, applies the edit to a copy, revalidates it
/// (which re-parses every measure formula) and only then stores it. A rejected edit — a duplicate
/// name, an unknown table, an unparsable formula — therefore leaves the model exactly as it was.
fn commit(s: &mut Session, cmd: &str, model: DataModel) -> Result<Json> {
    store_model(s, cmd, model)?;
    status(s, &Json::Null)
}

/// [`commit`] without the status reply, for a command that also stores metadata and builds the
/// status after both are in place.
fn store_model(s: &mut Session, cmd: &str, mut model: DataModel) -> Result<()> {
    model.parse_formulas().map_err(|error| bad(cmd, error.to_string()))?;
    s.dax_model = Some(model);
    Ok(())
}

/// A copy of the loaded model, or the "no model" error for a caller that skipped the enabled check.
fn edit_model(s: &Session, cmd: &str) -> Result<DataModel> {
    s.dax_model.clone().ok_or_else(|| bad(cmd, "no data model loaded"))
}

/// A required, non-blank string parameter, trimmed.
fn required_str<'a>(p: &'a Json, cmd: &str, key: &str) -> Result<&'a str> {
    str_param(p, key).map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| bad(cmd, format!("`{key}` is required")))
}

/// An optional string parameter; absent and JSON `null` both mean "none".
fn optional_string(p: &Json, key: &str) -> Option<String> {
    p.get(key).and_then(Json::as_str).map(str::to_string)
}

/// The model's own spelling of a table name (names match case-insensitively).
fn known_table(model: &DataModel, cmd: &str, table: &str) -> Result<String> {
    let index = model.table_index(table).ok_or_else(|| bad(cmd, format!("unknown table `{table}`")))?;
    Ok(model.tables[index].name.clone())
}

/// The model's own spelling of a column, rejecting an unknown table or column.
fn known_column(model: &DataModel, cmd: &str, table: &str, column: &str) -> Result<String> {
    let table_index = model.table_index(table).ok_or_else(|| bad(cmd, format!("unknown table `{table}`")))?;
    let name = &model.tables[table_index].name;
    let column_index = model.column_index(table_index, column).ok_or_else(|| bad(cmd, format!("unknown column `{column}` in table `{name}`")))?;
    Ok(model.tables[table_index].columns[column_index].name.clone())
}

fn measure_or_error(model: &DataModel, cmd: &str, name: &str) -> Result<usize> {
    model.measure_index(name).ok_or_else(|| bad(cmd, format!("unknown measure `{name}`")))
}

/// Measures that reference `target` from their formulas. Renaming or deleting the target would
/// leave those references pointing at an unknown name (which the binder would otherwise read as
/// an unqualified column), so edits must be rejected while dependents exist.
fn dependents(model: &DataModel, cmd: &str, target: &str) -> Result<BTreeSet<String>> {
    let binder = Binder::new(model);
    let mut found = BTreeSet::new();
    for measure in &model.measures {
        if measure.name.eq_ignore_ascii_case(target) {
            continue;
        }
        match binder.bind_measure(&measure.name) {
            Ok(bound) => {
                if bound.dependencies.iter().any(|dependency| dependency.eq_ignore_ascii_case(target)) {
                    found.insert(measure.name.clone());
                }
            }
            Err(_) => {
                // A broken formula can still contain `[Target]` after the error point. Scan the
                // parsed AST so a rename/delete cannot leave a dangling reference behind, while
                // an unrelated broken measure does not block the edit.
                if measure.expr.as_ref().is_some_and(|expr| references_measure(expr, target)) {
                    found.insert(measure.name.clone());
                }
            }
        }
    }
    let _ = cmd;
    Ok(found)
}

/// Whether a parsed expression contains `[name]` as an unqualified measure reference.
fn references_measure(expr: &Expr, name: &str) -> bool {
    let mut found = false;
    expr.walk(&mut |node| {
        if let ExprKind::Name(reference) = &node.kind
            && reference.table.is_none()
            && reference.name.text.eq_ignore_ascii_case(name)
        {
            found = true;
        }
    });
    found
}

fn dangling_reference_error(cmd: &str, action: &str, target: &str, dependent: &BTreeSet<String>) -> EngineError {
    let names: Vec<&str> = dependent.iter().map(String::as_str).collect();
    bad(cmd, format!("cannot {action} measure `{target}`: referenced by {}", names.join(", ")))
}

/// Relationship identity: the four endpoint names, compared the way the model compares names.
fn same_ends(r: &Relationship, from_table: &str, from_column: &str, to_table: &str, to_column: &str) -> bool {
    r.from_table.eq_ignore_ascii_case(from_table)
        && r.from_column.eq_ignore_ascii_case(from_column)
        && r.to_table.eq_ignore_ascii_case(to_table)
        && r.to_column.eq_ignore_ascii_case(to_column)
}

fn add_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.addMeasure";
    let name = required_str(p, CMD, "name")?;
    let formula = required_str(p, CMD, "formula")?;
    let mut model = edit_model(s, CMD)?;
    if model.measure_index(name).is_some() {
        return Err(bad(CMD, format!("a measure named `{name}` already exists")));
    }
    let table = match str_param(p, "table") {
        Some(table) => Some(known_table(&model, CMD, table.trim())?),
        None => None,
    };
    let expr = parse_expression(formula).map_err(|error| bad(CMD, format!("measure `{name}`: {error}")))?;
    model.measures.push(Measure {
        name: name.to_string(),
        formula: formula.to_string(),
        table,
        format: optional_string(p, "format"),
        expr: Some(expr),
    });
    commit(s, CMD, model)
}

fn update_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.updateMeasure";
    let name = required_str(p, CMD, "name")?;
    let mut model = edit_model(s, CMD)?;
    let index = measure_or_error(&model, CMD, name)?;
    let old_name = model.measures[index].name.clone();
    if p.get("new_name").is_some() {
        let new_name = required_str(p, CMD, "new_name")?;
        if model.measure_index(new_name).is_some_and(|other| other != index) {
            return Err(bad(CMD, format!("a measure named `{new_name}` already exists")));
        }
        if !new_name.eq_ignore_ascii_case(&old_name) {
            let dependent = dependents(&model, CMD, &old_name)?;
            if !dependent.is_empty() {
                return Err(dangling_reference_error(CMD, "rename", &old_name, &dependent));
            }
        }
        model.measures[index].name = new_name.to_string();
    }
    if p.get("formula").is_some() {
        let formula = required_str(p, CMD, "formula")?;
        let expr = parse_expression(formula).map_err(|error| bad(CMD, format!("measure `{}`: {error}", model.measures[index].name)))?;
        model.measures[index].formula = formula.to_string();
        model.measures[index].expr = Some(expr);
    }
    if p.get("format").is_some() {
        model.measures[index].format = optional_string(p, "format");
    }
    commit(s, CMD, model)
}

fn delete_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.deleteMeasure";
    let name = required_str(p, CMD, "name")?;
    let mut model = edit_model(s, CMD)?;
    let index = measure_or_error(&model, CMD, name)?;
    let dependent = dependents(&model, CMD, name)?;
    if !dependent.is_empty() {
        return Err(dangling_reference_error(CMD, "delete", name, &dependent));
    }
    model.measures.remove(index);
    commit(s, CMD, model)
}

fn add_relationship(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.addRelationship";
    let from_table = required_str(p, CMD, "from_table")?;
    let from_column = required_str(p, CMD, "from_column")?;
    let to_table = required_str(p, CMD, "to_table")?;
    let to_column = required_str(p, CMD, "to_column")?;
    let mut model = edit_model(s, CMD)?;
    let from_table = known_table(&model, CMD, from_table)?;
    let from_column = known_column(&model, CMD, &from_table, from_column)?;
    let to_table = known_table(&model, CMD, to_table)?;
    let to_column = known_column(&model, CMD, &to_table, to_column)?;
    if model.relationships.iter().any(|existing| same_ends(existing, &from_table, &from_column, &to_table, &to_column)) {
        return Err(bad(CMD, format!("a relationship from {from_table}[{from_column}] to {to_table}[{to_column}] already exists")));
    }
    model.relationships.push(Relationship {
        from_table,
        from_column,
        to_table,
        to_column,
        bidirectional: bool_param(p, "bidirectional").unwrap_or(false),
        inactive: bool_param(p, "inactive").unwrap_or(false),
    });
    commit(s, CMD, model)
}

fn delete_relationship(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.deleteRelationship";
    let from_table = required_str(p, CMD, "from_table")?;
    let from_column = required_str(p, CMD, "from_column")?;
    let to_table = required_str(p, CMD, "to_table")?;
    let to_column = required_str(p, CMD, "to_column")?;
    let mut model = edit_model(s, CMD)?;
    let index = model
        .relationships
        .iter()
        .position(|existing| same_ends(existing, from_table, from_column, to_table, to_column))
        .ok_or_else(|| bad(CMD, format!("no relationship from {from_table}[{from_column}] to {to_table}[{to_column}]")))?;
    model.relationships.remove(index);
    commit(s, CMD, model)
}

// ---------------------------------------------------------------- Data View (session view state)

/// `dataModel.view`: one table as the Data View shows it — hidden columns applied, the table's
/// filters kept and its sort applied — plus the text the grid paints, the original row numbers and
/// the view settings in force. View state only: the model is untouched.
fn view(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.view";
    let table = required_str(p, CMD, "table")?;
    let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
    let name = known_table(model, CMD, table)?;
    let show_hidden = bool_param(p, "show_hidden").unwrap_or(s.dax_view.show_hidden);
    view_json(model, &s.dax_view, &s.dax_metadata, &name, show_hidden)
}

/// Builds the Data View table. Filters are applied per column (a row shows when every filter
/// keeps it), then the sort orders what is left, then the size cap applies, so the order the cap
/// sees is the final one.
fn view_json(model: &DataModel, view: &ModelView, metadata: &ModelMetadata, table: &str, show_hidden: bool) -> Result<Json> {
    let index = model.table_index(table).ok_or_else(|| EngineError::Other(format!("unknown table `{table}`")))?;
    let model_table = &model.tables[index];
    let state = view.tables.get(&model_table.name.to_ascii_lowercase());
    let hidden_flags: Vec<bool> =
        model_table.columns.iter().map(|column| metadata.column(&model_table.name, &column.name).is_some_and(|metadata| metadata.hidden)).collect();
    let shown: Vec<usize> = (0..model_table.columns.len()).filter(|index| show_hidden || !hidden_flags[*index]).collect();
    let hidden: Vec<String> =
        model_table.columns.iter().zip(&hidden_flags).filter(|(_, hidden)| **hidden).map(|(column, _)| column.name.clone()).collect();
    let filters = state.map(|state| state.filters.as_slice()).unwrap_or(&[]);
    let active: Vec<(usize, BTreeSet<&str>)> = filters
        .iter()
        .filter_map(|filter| {
            let column = model.column_index(index, &filter.column)?;
            Some((column, filter.values.iter().map(String::as_str).collect()))
        })
        .collect();
    let rows = &model_table.rows;
    let mut order: Vec<usize> = (0..rows.len())
        .filter(|row| active.iter().all(|(column, values)| values.contains(scalar_text(&cell(rows, *row, *column)).as_str())))
        .collect();
    let sort = state.and_then(|state| state.sort.as_ref());
    if let Some(sort) = sort
        && let Some(column) = model.column_index(index, &sort.column)
    {
        let related: Option<BTreeMap<ScalarKey, ScalarKey>> = match (&sort.by_table, &sort.by_column) {
            (Some(by_table), Some(by_column)) => related_sort_map(model, index, column, by_table, by_column),
            _ => None,
        };
        let key_of = |row: usize| -> Option<ScalarKey> {
            let key = rows.get(row)?.get(column).map(ScalarKey::from);
            match &related {
                None => key,
                Some(map) => key.and_then(|key| map.get(&key).cloned()),
            }
        };
        order.sort_by(|left, right| {
            let ordering = match (key_of(*left), key_of(*right)) {
                (Some(left), Some(right)) => left.cmp(&right),
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
            };
            if sort.descending { ordering.reverse() } else { ordering }
        });
    }
    let total_rows = rows.len();
    let truncated = order.len() > MAX_VIEW_ROWS;
    order.truncate(MAX_VIEW_ROWS);
    let mut values = Vec::with_capacity(order.len());
    let mut texts = Vec::with_capacity(order.len());
    let mut row_numbers = Vec::with_capacity(order.len());
    for row in &order {
        let mut row_values = Vec::with_capacity(shown.len());
        let mut row_texts = Vec::with_capacity(shown.len());
        for column in &shown {
            let value = cell(rows, *row, *column);
            row_texts.push(Json::String(scalar_text(&value)));
            row_values.push(value_json(&value));
        }
        values.push(Json::Array(row_values));
        texts.push(Json::Array(row_texts));
        row_numbers.push(json!(row));
    }
    Ok(json!({
        "table": model_table.name,
        "columns": shown.iter().map(|index| model_table.columns[*index].name.clone()).collect::<Vec<_>>(),
        "column_indexes": shown,
        "rows": values,
        "text_rows": texts,
        "row_numbers": row_numbers,
        "hidden": hidden,
        "sort": sort_json(sort),
        "filters": filters_json(filters),
        "total_rows": total_rows,
        "visible_rows": order.len(),
        "truncated": truncated,
        "view_state": true,
    }))
}

/// A sort key map for a sort that follows a relationship: each of the sorted table's key values
/// maps to the related table's sort-key value. `None` when the relationship or column is gone.
fn related_sort_map(model: &DataModel, table: usize, key_column: usize, by_table: &str, by_column: &str) -> Option<BTreeMap<ScalarKey, ScalarKey>> {
    let by_index = model.table_index(by_table)?;
    let by_column = model.column_index(by_index, by_column)?;
    let by_key = related_key_column(model, table, key_column, by_index)?;
    let mut map = BTreeMap::new();
    for row in &model.tables[by_index].rows {
        if let (Some(key), Some(value)) = (row.get(by_key), row.get(by_column)) {
            map.insert(ScalarKey::from(key), ScalarKey::from(value));
        }
    }
    Some(map)
}

/// The column of `by_table` that joins it to `table`'s `key_column`, from the first relationship
/// that connects the two on that column.
fn related_key_column(model: &DataModel, table: usize, key_column: usize, by_index: usize) -> Option<usize> {
    for relationship in &model.relationships {
        let Some(from) = model.table_index(&relationship.from_table) else { continue };
        let Some(to) = model.table_index(&relationship.to_table) else { continue };
        let Some(from_column) = model.column_index(from, &relationship.from_column) else { continue };
        let Some(to_column) = model.column_index(to, &relationship.to_column) else { continue };
        if from == table && to == by_index && from_column == key_column {
            return Some(to_column);
        }
        if to == table && from == by_index && to_column == key_column {
            return Some(from_column);
        }
    }
    None
}

/// `dataModel.find`: case-insensitive (or case-sensitive) text search over model rows, scoped to
/// one table and/or one column. Hidden columns are searched too: `find` looks at the model.
fn find(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.find";
    let query = required_str(p, CMD, "query")?;
    let case = bool_param(p, "case").unwrap_or(false);
    let needle = if case { query.to_string() } else { query.to_lowercase() };
    let table_scope = optional_string(p, "table").map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
    let column_scope = optional_string(p, "column").map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
    if column_scope.is_some() && table_scope.is_none() {
        return Err(bad(CMD, "`column` needs `table`"));
    }
    let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
    let table_scope = match &table_scope {
        Some(name) => Some(known_table(model, CMD, name)?),
        None => None,
    };
    let column_scope = match (&table_scope, &column_scope) {
        (Some(table), Some(column)) => Some(known_column(model, CMD, table, column)?),
        _ => None,
    };
    let mut matches = Vec::new();
    let mut truncated = false;
    'tables: for table in &model.tables {
        if table_scope.as_deref().is_some_and(|name| !table.name.eq_ignore_ascii_case(name)) {
            continue;
        }
        for (column_index, column) in table.columns.iter().enumerate() {
            if column_scope.as_deref().is_some_and(|name| !column.name.eq_ignore_ascii_case(name)) {
                continue;
            }
            for (row, values) in table.rows.iter().enumerate() {
                let text = scalar_text(&values.get(column_index).cloned().unwrap_or(Scalar::Blank));
                let haystack = if case { text.clone() } else { text.to_lowercase() };
                if !haystack.contains(&needle) {
                    continue;
                }
                if matches.len() == MAX_FIND_MATCHES {
                    truncated = true;
                    break 'tables;
                }
                matches.push(json!({"table": table.name, "column": column.name, "row": row, "value": text}));
            }
        }
    }
    Ok(json!({"query": query, "matches": matches, "truncated": truncated}))
}

/// The view settings in force for one table: the small reply of a view-state command. It is
/// session view state, not model data, so the reply says so.
fn view_state_json(view: &ModelView, table: &str) -> Json {
    let state = view.tables.get(&table.to_ascii_lowercase());
    json!({
        "table": table,
        "sort": sort_json(state.and_then(|state| state.sort.as_ref())),
        "filters": filters_json(state.map(|state| state.filters.as_slice()).unwrap_or(&[])),
        "view_state": true,
    })
}

fn sort_table(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.sortTable";
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let descending = bool_param(p, "descending").unwrap_or(false);
    let (table, column) = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        let table = known_table(model, CMD, table_name)?;
        let column = known_column(model, CMD, &table, column_name)?;
        (table, column)
    };
    s.dax_view.tables.entry(table.to_ascii_lowercase()).or_default().sort = Some(TableSort { column, by_table: None, by_column: None, descending });
    Ok(view_state_json(&s.dax_view, &table))
}

fn clear_sort(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.clearSort";
    let table = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        known_table(model, CMD, required_str(p, CMD, "table")?)?
    };
    if let Some(state) = s.dax_view.tables.get_mut(&table.to_ascii_lowercase()) {
        state.sort = None;
    }
    Ok(view_state_json(&s.dax_view, &table))
}

fn set_filter(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setFilter";
    let values = filter_values(p, CMD)?;
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let (table, column) = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        let table = known_table(model, CMD, table_name)?;
        let column = known_column(model, CMD, &table, column_name)?;
        (table, column)
    };
    let state = s.dax_view.tables.entry(table.to_ascii_lowercase()).or_default();
    state.filters.retain(|filter| !filter.column.eq_ignore_ascii_case(&column));
    if !values.is_empty() {
        state.filters.push(ColumnFilter { column, values });
    }
    Ok(view_state_json(&s.dax_view, &table))
}

fn clear_filters(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.clearFilters";
    let table = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        known_table(model, CMD, required_str(p, CMD, "table")?)?
    };
    if let Some(state) = s.dax_view.tables.get_mut(&table.to_ascii_lowercase()) {
        state.filters.clear();
    }
    Ok(view_state_json(&s.dax_view, &table))
}

/// The `values` parameter of `dataModel.setFilter`: an array of strings, or null/absent for "clear
/// this column's filter".
fn filter_values(p: &Json, cmd: &str) -> Result<Vec<String>> {
    match p.get("values") {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Array(items)) if items.len() <= MAX_FILTER_VALUES => {
            items.iter().map(|value| value.as_str().map(str::to_string).ok_or_else(|| bad(cmd, "`values` must be an array of strings"))).collect()
        }
        Some(Json::Array(_)) => Err(bad(cmd, format!("`values` keeps at most {MAX_FILTER_VALUES} values"))),
        Some(_) => Err(bad(cmd, "`values` must be an array of strings or null")),
    }
}

/// `dataModel.sortByColumn`: order a table by a column of a related table, through the
/// relationship that joins them (Power Pivot's "Sort by Column", across the model).
fn sort_by_column(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.sortByColumn";
    let table_name = required_str(p, CMD, "table")?;
    let by_table_name = required_str(p, CMD, "by_table")?;
    let by_column_name = required_str(p, CMD, "by_column")?;
    let descending = bool_param(p, "descending").unwrap_or(false);
    let key_name = optional_string(p, "column").map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
    let (table, by_table, by_column, key_column) = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        let table = known_table(model, CMD, table_name)?;
        let by_table = known_table(model, CMD, by_table_name)?;
        let by_column = known_column(model, CMD, &by_table, by_column_name)?;
        if table.eq_ignore_ascii_case(&by_table) {
            return Err(bad(CMD, "sort a table by a related table's column, not by its own"));
        }
        let table_index = model.table_index(&table).ok_or_else(|| bad(CMD, format!("unknown table `{table}`")))?;
        let by_index = model.table_index(&by_table).ok_or_else(|| bad(CMD, format!("unknown table `{by_table}`")))?;
        let candidates = relationship_candidates(model, table_index, by_index);
        if candidates.is_empty() {
            return Err(bad(CMD, format!("no relationship connects `{table}` and `{by_table}`")));
        }
        let chosen = match &key_name {
            Some(name) => {
                let column = known_column(model, CMD, &table, name)?;
                let index = model.column_index(table_index, &column).ok_or_else(|| bad(CMD, format!("unknown column `{column}`")))?;
                candidates
                    .iter()
                    .copied()
                    .find(|(table_column, _)| *table_column == index)
                    .ok_or_else(|| bad(CMD, format!("no relationship joins `{table}` and `{by_table}` on `{column}`")))?
            }
            None => candidates.first().copied().ok_or_else(|| bad(CMD, "no relationship to sort by"))?,
        };
        if key_name.is_none() && candidates.len() > 1 {
            return Err(bad(CMD, format!("more than one relationship connects `{table}` and `{by_table}`; pass `column` to choose the key column")));
        }
        let key_column =
            model.tables[table_index].columns.get(chosen.0).map(|column| column.name.clone()).ok_or_else(|| bad(CMD, "no key column"))?;
        (table, by_table, by_column, key_column)
    };
    s.dax_view.tables.entry(table.to_ascii_lowercase()).or_default().sort =
        Some(TableSort { column: key_column, by_table: Some(by_table), by_column: Some(by_column), descending });
    Ok(view_state_json(&s.dax_view, &table))
}

/// The `(table key column, other table's key column)` pairs of relationships connecting two
/// tables, in the model's relationship order.
fn relationship_candidates(model: &DataModel, table: usize, by_table: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for relationship in &model.relationships {
        let Some(from) = model.table_index(&relationship.from_table) else { continue };
        let Some(to) = model.table_index(&relationship.to_table) else { continue };
        let Some(from_column) = model.column_index(from, &relationship.from_column) else { continue };
        let Some(to_column) = model.column_index(to, &relationship.to_column) else { continue };
        if from == table && to == by_table {
            out.push((from_column, to_column));
        } else if to == table && from == by_table {
            out.push((to_column, from_column));
        }
    }
    out
}

fn set_show_hidden(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setShowHidden";
    let show = p.get("show").and_then(Json::as_bool).ok_or_else(|| bad(CMD, "`show` is required (true or false)"))?;
    s.dax_view.show_hidden = show;
    status(s, &Json::Null)
}

fn set_show_implicit_measures(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setShowImplicitMeasures";
    let show = p.get("show").and_then(Json::as_bool).ok_or_else(|| bad(CMD, "`show` is required (true or false)"))?;
    s.dax_view.show_implicit_measures = show;
    status(s, &Json::Null)
}

// ---------------------------------------------------------------- model metadata

fn set_format(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setFormat";
    let name = required_str(p, CMD, "name")?;
    let mut model = edit_model(s, CMD)?;
    let index = measure_or_error(&model, CMD, name)?;
    let current = model.measures[index].format.clone();
    let next = if let Some(format) = p.get("format") {
        match format.as_str() {
            Some(text) => {
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    if text.chars().count() > MAX_FORMAT_LEN {
                        return Err(bad(CMD, format!("a format code holds at most {MAX_FORMAT_LEN} characters")));
                    }
                    Some(text.to_string())
                }
            }
            None if format.is_null() => None,
            None => return Err(bad(CMD, "`format` must be a format code or null")),
        }
    } else if let Some(preset) = str_param(p, "preset") {
        let code =
            preset_code(preset).ok_or_else(|| bad(CMD, format!("unknown format preset `{preset}`; use Currency, Percent, Comma or Decimal")))?;
        if code.eq_ignore_ascii_case("General") { None } else { Some(code.to_string()) }
    } else if let Some(decimals) = p.get("decimals").and_then(Json::as_i64) {
        if !(-10..=10).contains(&decimals) {
            return Err(bad(CMD, "`decimals` is a change of at most 10 places"));
        }
        if decimals == 0 { current } else { Some(adjust_decimals(current.as_deref().unwrap_or("0.00"), decimals as i32)) }
    } else {
        return Err(bad(CMD, "setFormat needs `format`, `preset` or `decimals`"));
    };
    model.measures[index].format = next;
    commit(s, CMD, model)
}

fn set_column_hidden(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setColumnHidden";
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let hidden = bool_param(p, "hidden").unwrap_or(true);
    let (table, column) = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        let table = known_table(model, CMD, table_name)?;
        let column = known_column(model, CMD, &table, column_name)?;
        (table, column)
    };
    let mut metadata = s.dax_metadata.clone();
    let entry = metadata.column_mut(&table, &column).ok_or_else(|| bad(CMD, format!("no metadata for `{table}[{column}]`")))?;
    entry.hidden = hidden;
    s.dax_metadata = metadata;
    status(s, &Json::Null)
}

fn set_column_type(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setColumnType";
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let type_name = required_str(p, CMD, "data_type")?;
    let target = ModelType::parse(type_name)
        .ok_or_else(|| bad(CMD, format!("unknown model type `{type_name}`; use Int64, Double, Decimal, Currency, String, Boolean or DateTime")))?;
    let mut model = edit_model(s, CMD)?;
    let table_index = model.table_index(table_name).ok_or_else(|| bad(CMD, format!("unknown table `{table_name}`")))?;
    let column = known_column(&model, CMD, table_name, column_name)?;
    let column_index = model.column_index(table_index, &column).ok_or_else(|| bad(CMD, format!("unknown column `{column}`")))?;
    let table = model.tables[table_index].name.clone();
    if s.dax_metadata.table(&table).and_then(|metadata| metadata.date_column.as_deref()).is_some_and(|date| date.eq_ignore_ascii_case(&column))
        && target != ModelType::DateTime
    {
        return Err(bad(CMD, format!("unmark `{table}` as a date table (dataModel.unmarkDateTable) before changing `{column}` away from DateTime")));
    }
    for (row, values) in model.tables[table_index].rows.iter().enumerate() {
        let value = values.get(column_index).cloned().unwrap_or(Scalar::Blank);
        if !target.accepts(&value) {
            return Err(bad(
                CMD,
                format!("row {} of `{table}[{column}]` holds `{}`, which is not a valid {}", row + 1, scalar_text(&value), target.name()),
            ));
        }
    }
    model.tables[table_index].columns[column_index].data_type = Some(target.name().to_string());
    commit(s, CMD, model)
}

fn set_summarize_by(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.setSummarizeBy";
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let summarize = match p.get("summarize_by") {
        None => return Err(bad(CMD, "`summarize_by` is required; use null to reset the type default")),
        Some(Json::Null) => None,
        Some(Json::String(name)) => Some(
            summarize_by_name(name)
                .ok_or_else(|| bad(CMD, format!("unknown summarization `{name}`; use Sum, Average, Count, Min, Max or None")))?
                .to_string(),
        ),
        Some(_) => return Err(bad(CMD, "`summarize_by` must be a string or null")),
    };
    let (table, column) = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        let table = known_table(model, CMD, table_name)?;
        let column = known_column(model, CMD, &table, column_name)?;
        (table, column)
    };
    let mut metadata = s.dax_metadata.clone();
    let entry = metadata.column_mut(&table, &column).ok_or_else(|| bad(CMD, format!("no metadata for `{table}[{column}]`")))?;
    entry.summarize_by = summarize;
    s.dax_metadata = metadata;
    status(s, &Json::Null)
}

/// `dataModel.markDateTable`: marks a column as the table's date table after proving it is a
/// contiguous run of unique dates, in any row order. Time-of-day and gaps both reject the command.
fn mark_date_table(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.markDateTable";
    let table_name = required_str(p, CMD, "table")?;
    let column_name = required_str(p, CMD, "column")?;
    let mut model = edit_model(s, CMD)?;
    let table_index = model.table_index(table_name).ok_or_else(|| bad(CMD, format!("unknown table `{table_name}`")))?;
    let column = known_column(&model, CMD, table_name, column_name)?;
    let column_index = model.column_index(table_index, &column).ok_or_else(|| bad(CMD, format!("unknown column `{column}`")))?;
    let table = model.tables[table_index].name.clone();
    if let Some(declared) = model.tables[table_index].columns[column_index].data_type.clone()
        && ModelType::parse(&declared) != Some(ModelType::DateTime)
    {
        return Err(bad(CMD, format!("`{table}[{column}]` is `{declared}`, not a date column; set its data type to DateTime first")));
    }
    let rows = &model.tables[table_index].rows;
    if rows.is_empty() {
        return Err(bad(CMD, format!("`{table}[{column}]` has no rows to validate")));
    }
    // Excel does not care about the row order: the column is a date table's date key whatever
    // order the rows arrived in. Collect each serial with its row so a rejection still names rows.
    let mut serials: Vec<(f64, usize)> = Vec::with_capacity(rows.len());
    for (row, values) in rows.iter().enumerate() {
        let value = values.get(column_index).cloned().unwrap_or(Scalar::Blank);
        match value {
            Scalar::DateTime(serial) if serial.is_finite() => serials.push((serial, row)),
            Scalar::String(text) => match iso_date_serial(&text) {
                Some(serial) => serials.push((serial, row)),
                None => return Err(bad(CMD, format!("row {} of `{table}[{column}]` holds `{text}`, which is not a date", row + 1))),
            },
            other => {
                return Err(bad(CMD, format!("row {} of `{table}[{column}]` holds `{}`, which is not a date", row + 1, scalar_text(&other))));
            }
        }
    }
    serials.sort_by(|left, right| left.0.total_cmp(&right.0));
    for pair in serials.windows(2) {
        let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else { continue };
        let step = second.0 - first.0;
        if step == 0.0 {
            return Err(bad(
                CMD,
                format!("`{table}[{column}]` repeats the date at rows {} and {}; a date table needs one row per date", first.1 + 1, second.1 + 1),
            ));
        }
        if (step - 1.0).abs() > 1e-9 {
            return Err(bad(
                CMD,
                format!("`{table}[{column}]` is not contiguous between rows {} and {}; a date table needs every date", first.1 + 1, second.1 + 1),
            ));
        }
    }
    if model.tables[table_index].columns[column_index].data_type.is_none() {
        model.tables[table_index].columns[column_index].data_type = Some("DateTime".to_string());
    }
    let mut metadata = s.dax_metadata.clone();
    let entry = metadata.table_mut(&table).ok_or_else(|| bad(CMD, format!("no metadata for table `{table}`")))?;
    entry.date_column = Some(column);
    store_model(s, CMD, model)?;
    s.dax_metadata = metadata;
    status(s, &Json::Null)
}

fn unmark_date_table(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.unmarkDateTable";
    let table = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        known_table(model, CMD, required_str(p, CMD, "table")?)?
    };
    let mut metadata = s.dax_metadata.clone();
    let entry = metadata.table_mut(&table).ok_or_else(|| bad(CMD, format!("no metadata for table `{table}`")))?;
    if entry.date_column.is_none() {
        return Err(bad(CMD, format!("`{table}` is not marked as a date table")));
    }
    entry.date_column = None;
    s.dax_metadata = metadata;
    status(s, &Json::Null)
}

/// `dataModel.validate`: referential integrity and relationship cardinality, read from the model's
/// own rows. Read-only: the report is not stored and the model is not touched.
fn validate(s: &mut Session, _: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.validate";
    let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
    serde_json::to_value(gridcraft_dax::validate_model(model)).map_err(|error| bad(CMD, format!("cannot serialize the report: {error}")))
}

fn value_json(value: &gridcraft_dax::Scalar) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

fn table_json(table: &gridcraft_dax::TableValue) -> Json {
    json!({
        "columns": table.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>(),
        "rows": table.rows.iter().map(|row| row.iter().map(value_json).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model in its own temp directory, so tests that run in parallel can't delete each other's
    /// model file.
    fn write_model(name: &str, json: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("gridcraft-dax-model-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("model.json");
        std::fs::write(&path, json).expect("write model");
        (path, dir.to_string_lossy().into_owned())
    }

    /// [`write_model`] with the standard one-table model.
    fn fixture(name: &str) -> (std::path::PathBuf, String) {
        write_model(name, SALES_MODEL)
    }

    /// The one-table model the load/evaluate/pivot tests run against.
    const SALES_MODEL: &str = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                    "rows":[["East",10],["West",20],["East",30]]}],
        "relationships": [],
        "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
    }"#;

    #[test]
    fn load_accepts_json_text_and_objects() {
        let mut s = Session::new();
        let text = r#"{"tables":[{"name":"Sales","columns":[{"name":"Amount"}],"rows":[[10]]}],"relationships":[],"measures":[]}"#;
        let status = s.execute("dataModel.load", json!({"json": text})).expect("load json text");
        assert_eq!(status["tables"][0]["name"], "Sales");
        let object: Json = serde_json::from_str(text).expect("json");
        let status = s.execute("dataModel.load", json!({"model": object})).expect("load model object");
        assert_eq!(status["tables"][0]["name"], "Sales");
        assert!(s.execute("dataModel.load", json!({"json": "{not json"})).is_err());
        assert!(s.execute("dataModel.load", json!({})).is_err());
    }

    #[test]
    fn load_status_evaluate_and_pivot() {
        let (path, dir) = fixture("load-status-evaluate-pivot");
        let mut s = Session::new();
        let status = s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert_eq!(status["tables"][0]["name"], "Sales");
        assert_eq!(s.execute("dataModel.status", json!({})).expect("status")["measures"][0]["name"], "Total Sales");

        let value = s.execute("dataModel.evaluate", json!({"formula": "SUM(Sales[Amount])"})).expect("eval");
        assert_eq!(value["value"], 60.0);
        let value = s.execute("dataModel.evaluate", json!({"measure": "Total Sales"})).expect("measure");
        assert_eq!(value["value"], 60.0);

        let table = s.execute("dataModel.evaluate", json!({"formula": "FILTER(Sales, Sales[Amount] > 10)", "table": true})).expect("table");
        assert_eq!(table["rows"].as_array().expect("rows").len(), 2);

        let pivot = s.execute("dataModel.pivot", json!({"measure": "Total Sales", "rows": ["Sales[Region]"]})).expect("pivot");
        assert_eq!(pivot["rows"][0][0], "East");
        assert_eq!(pivot["rows"][0][1], 40.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_model_and_bad_formula_report_errors() {
        let mut s = Session::new();
        assert!(s.execute("dataModel.evaluate", json!({"formula": "1"})).is_err());
        let (path, dir) = fixture("missing-model-and-bad-formula");
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert!(s.execute("dataModel.evaluate", json!({"formula": "SUM(Missing[Amount])"})).is_err());
        assert!(s.execute("dataModel.pivot", json!({"measure": "Missing", "rows": ["Sales[Region]"]})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn measure<'a>(status: &'a Json, name: &str) -> &'a Json {
        status["measures"].as_array().expect("measures").iter().find(|m| m["name"] == name).expect("measure")
    }

    #[allow(clippy::cognitive_complexity)] // Add, update and delete of a measure, each with its error cases.
    #[test]
    fn measure_add_update_and_delete() {
        let (path, dir) = write_model(
            "measure-edit",
            r#"{
                "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                            "rows":[["East",10],["West",20],["East",30]]}],
                "relationships": [],
                "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
            }"#,
        );
        let mut s = Session::new();
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Double Sales", "formula": "1"})).is_err(), "no model loaded yet");
        assert!(s.commands().iter().any(|c| c.id == "dataModel.addMeasure" && !c.enabled), "the edit commands are greyed out with no model");
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert!(s.commands().iter().any(|c| c.id == "dataModel.addMeasure" && c.enabled), "the edit commands are live with a model");

        let status = s
            .execute(
                "dataModel.addMeasure",
                json!({"name": "Double Sales", "formula": "SUM(Sales[Amount]) * 2", "table": "sales", "format": "#,##0"}),
            )
            .expect("add");
        assert_eq!(status["measures"].as_array().expect("measures").len(), 2);
        assert_eq!(measure(&status, "Double Sales")["table"], "Sales", "the measure keeps the model's spelling of the table");
        assert_eq!(measure(&status, "Double Sales")["format"], "#,##0");
        assert!(measure(&status, "Total Sales")["format"].is_null());
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Double Sales"})).expect("eval")["value"], 120.0);
        assert_eq!(s.execute("dataModel.pivot", json!({"measure": "Double Sales", "rows": ["Sales[Region]"]})).expect("pivot")["rows"][0][1], 80.0);

        assert!(s.execute("dataModel.addMeasure", json!({"name": "double sales", "formula": "1"})).is_err(), "duplicate name");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "1", "table": "Missing"})).is_err(), "unknown table");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "SUM("})).is_err(), "unparsable formula");
        assert!(s.execute("dataModel.addMeasure", json!({"formula": "1"})).is_err(), "`name` is required");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "  "})).is_err(), "a blank formula is required too");
        assert_eq!(
            s.execute("dataModel.status", json!({})).expect("status")["measures"].as_array().expect("measures").len(),
            2,
            "a rejected add changes nothing"
        );

        let status = s
            .execute(
                "dataModel.updateMeasure",
                json!({"name": "double sales", "formula": "SUM(Sales[Amount]) * 3", "new_name": "Triple Sales", "format": "0.00"}),
            )
            .expect("update");
        assert_eq!(measure(&status, "Triple Sales")["format"], "0.00");
        assert_eq!(measure(&status, "Triple Sales")["formula"], "SUM(Sales[Amount]) * 3");
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).expect("eval")["value"], 180.0);
        assert!(s.execute("dataModel.evaluate", json!({"measure": "Double Sales"})).is_err(), "the old name is gone");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Missing", "formula": "1"})).is_err(), "unknown measure");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "new_name": "Total Sales"})).is_err(), "duplicate new name");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "formula": "SUM("})).is_err(), "unparsable formula");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "new_name": "  "})).is_err(), "blank new name");
        assert_eq!(
            s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).expect("eval")["value"],
            180.0,
            "a rejected update keeps the old formula"
        );
        let status = s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "format": null})).expect("clear format");
        assert!(measure(&status, "Triple Sales")["format"].is_null(), "`format: null` clears the format");

        let status = s.execute("dataModel.deleteMeasure", json!({"name": "triple sales"})).expect("delete");
        assert_eq!(status["measures"].as_array().expect("measures").len(), 1);
        assert!(s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).is_err(), "the deleted measure is gone");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name": "Triple Sales"})).is_err(), "already deleted");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name": "Total Sales"})).is_ok(), "the last measure can go");
        assert!(s.execute("dataModel.status", json!({})).expect("status")["measures"].as_array().expect("measures").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relationship_add_and_delete_with_status_types() {
        let (path, dir) = write_model(
            "relationship-edit",
            r#"{
                "tables": [
                    {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Product","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                     "rows":[["East","Widget",10],["West","Gadget",20]]},
                    {"name":"Products","columns":[{"name":"Name","data_type":"String"},{"name":"Price","data_type":"Double"}],
                     "rows":[["Widget",3],["Gadget",5]]}
                ],
                "relationships": [],
                "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
            }"#,
        );
        let mut s = Session::new();
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");

        let status = s.execute("dataModel.status", json!({})).expect("status");
        assert_eq!(status["tables"][0]["columns"][2]["type"], "Double", "a column reports its declared model type");
        assert_eq!(status["tables"][1]["columns"][0]["name"], "Name");
        assert!(status["relationships"].as_array().expect("relationships").is_empty());

        let status = s
            .execute(
                "dataModel.addRelationship",
                json!({"from_table": "sales", "from_column": "product", "to_table": "Products", "to_column": "name"}),
            )
            .expect("add relationship");
        let relationship = &status["relationships"][0];
        assert_eq!(relationship["from_table"], "Sales", "the relationship keeps the model's spelling");
        assert_eq!(relationship["from_column"], "Product");
        assert_eq!(relationship["to_table"], "Products");
        assert_eq!(relationship["to_column"], "Name");
        assert!(!relationship["bidirectional"].as_bool().expect("bool"));
        assert!(!relationship["inactive"].as_bool().expect("bool"));

        assert!(
            s.execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "duplicate relationship"
        );
        assert!(
            s.execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Nope", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "unknown column"
        );
        assert!(
            s.execute("dataModel.addRelationship", json!({"from_table": "Missing", "from_column": "X", "to_table": "Products", "to_column": "Name"}))
                .is_err(),
            "unknown table"
        );
        assert!(
            s.execute("dataModel.addRelationship", json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products"})).is_err(),
            "`to_column` is required"
        );
        assert_eq!(s.execute("dataModel.status", json!({})).expect("status")["relationships"].as_array().expect("relationships").len(), 1);
        // Evaluate while the only relationship is one-directional: the evaluator's filter
        // propagation recurses forever on a bidirectional relationship (see `dax::eval::table`),
        // which is a pre-existing dax-crate bug, not something this command introduces.
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Total Sales"})).expect("eval")["value"], 30.0);

        let status = s
            .execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Region", "to_table": "Products", "to_column": "Name", "bidirectional": true, "inactive": true}),
            )
            .expect("second relationship");
        assert!(status["relationships"][1]["bidirectional"].as_bool().expect("bool"));
        assert!(status["relationships"][1]["inactive"].as_bool().expect("bool"));

        let status = s
            .execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "sales", "from_column": "product", "to_table": "products", "to_column": "name"}),
            )
            .expect("delete relationship");
        assert_eq!(status["relationships"].as_array().expect("relationships").len(), 1);
        assert_eq!(status["relationships"][0]["from_column"], "Region", "the other relationship survives");
        assert!(
            s.execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "already deleted"
        );
        assert!(
            s.execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "Sales", "from_column": "Region", "to_table": "Products", "to_column": "Name"})
            )
            .is_ok()
        );
        assert!(s.execute("dataModel.status", json!({})).expect("status")["relationships"].as_array().expect("relationships").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_formulas_still_protect_their_references() {
        let json = r#"{
            "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                        "rows":[["East",10]]}],
            "relationships": [],
            "measures": [
                {"name":"Total","formula":"SUM(Sales[Amount])","table":"Sales"},
                {"name":"Broken","formula":"Missing[Value] + [Total]","table":"Sales"},
                {"name":"Unrelated","formula":"SUM(Sales[Amount])","table":"Sales"}
            ]
        }"#;
        let (path, dir) = write_model("broken-reference", json);
        let mut s = Session::new();
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");

        // The broken formula never binds, but it still contains `[Total]`.
        assert!(s.execute("dataModel.updateMeasure", json!({"name":"Total","new_name":"Renamed"})).is_err());
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Total"})).is_err());
        // An unrelated broken measure does not block deleting another measure.
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Unrelated"})).is_ok());
        // Removing the broken formula releases the reference.
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Broken"})).is_ok());
        assert!(s.execute("dataModel.updateMeasure", json!({"name":"Total","new_name":"Renamed"})).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn measure_edits_reject_dangling_references() {
        let json = r#"{
            "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                        "rows":[["East",10],["West",20]]}],
            "relationships": [],
            "measures": [
                {"name":"Base","formula":"SUM(Sales[Amount])","table":"Sales"},
                {"name":"Derived","formula":"[Base] * 2","table":"Sales"}
            ]
        }"#;
        let (path, dir) = write_model("dangling", json);
        let mut s = Session::new();
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");

        let error = s.execute("dataModel.updateMeasure", json!({"name":"Base","new_name":"Renamed"})).expect_err("rename must fail");
        assert!(error.to_string().contains("Derived"), "{error}");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Base"})).is_err());

        // Removing the dependent first makes the rename and deletion valid.
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Derived"})).is_ok());
        assert!(s.execute("dataModel.updateMeasure", json!({"name":"Base","new_name":"Renamed"})).is_ok());
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Renamed"})).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
