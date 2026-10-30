//! Tabular data-model metadata: tables, columns, relationships and measures.
//!
//! This is the binder/evaluator's model view. It deliberately does not implement XLSX storage:
//! callers load the metadata from JSON or from a workbook adapter and parse the measure formulas
//! with [`DataModel::parse_formulas`]. Rows are not part of this layer; the evaluator supplies
//! table values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::value::Scalar;
use crate::{Expr, ParseError, parse_expression};

/// A table in the model. Column order is preserved because `RELATEDTABLE`, `ROW` and table
/// constructors expose it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelTable {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<ModelColumn>,
    /// Optional inline rows. Workbook adapters can fill these; the metadata-only binder ignores
    /// them. Each row has one scalar per column, in column order.
    #[serde(default)]
    pub rows: Vec<Vec<Scalar>>,
}

/// A model column. `data_type` is the model's declared type name (`Int64`, `Double`, `String`,
/// `DateTime`, `Boolean`, `Decimal`, ...); it is metadata, not a parsed [`crate::types::DataType`],
/// because model type names and DAX expression types are not the same vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelColumn {
    pub name: String,
    #[serde(default)]
    pub data_type: Option<String>,
}

/// Model metadata the binder does not need: hidden columns, the marked date column and each
/// column's default summarization. It travels inside the same model JSON document, on the table
/// and column objects (`"date_column"`, `"hidden"`, `"summarize_by"`), so a model stays one file;
/// the engine reads and validates it at the model boundary (`dataModel.load`) and reports it from
/// `dataModel.status`.
///
/// The type doubles as the loader: unknown document keys (`rows`, `data_type`, `relationships`,
/// `measures`, ...) are ignored, so a document written before these fields existed loads with
/// every field at its default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelMetadata {
    pub tables: Vec<TableMetadata>,
}

/// One table's metadata slice of a model document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableMetadata {
    pub name: String,
    /// The column marked as this table's date column, in the model's spelling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_column: Option<String>,
    pub columns: Vec<ColumnMetadata>,
}

/// One column's metadata slice of a model document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnMetadata {
    pub name: String,
    /// Hidden from the Data View and the field list (`dataModel.setColumnHidden`). Model
    /// metadata, as in Excel; the window's "Show Hidden" toggle is only a view setting.
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Excel's "Summarize By" (`Sum`, `Average`, `Count`, `Min`, `Max`, `None`), canonicalized.
    /// Absent falls back to the type default: numbers sum, everything else counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summarize_by: Option<String>,
}

/// The canonical spelling of a `Summarize By` name, or `None` when it is not one. Names match
/// case-insensitively and ignore spaces, hyphens and underscores, so `"do not summarize"`,
/// `"DoNotSummarize"` and `"none"` all mean `None`.
pub fn summarize_by_name(name: &str) -> Option<&'static str> {
    let key: String = name.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).collect::<String>().to_ascii_lowercase();
    match key.as_str() {
        "sum" => Some("Sum"),
        "average" | "avg" => Some("Average"),
        "count" => Some("Count"),
        "min" | "minimum" => Some("Min"),
        "max" | "maximum" => Some("Max"),
        "none" | "donotsummarize" => Some("None"),
        _ => None,
    }
}

/// Whether a serde field should be left out of the serialized JSON (its default).
fn is_false(value: &bool) -> bool {
    !*value
}

impl ModelMetadata {
    /// Parses and validates the metadata of a model JSON document.
    ///
    /// The type itself is the loader: unknown document keys (`rows`, `data_type`,
    /// `relationships`, `measures`, ...) are ignored, so a document written before the metadata
    /// fields existed yields every table and column with default metadata.
    pub fn from_model_json(text: &str) -> Result<ModelMetadata, ModelError> {
        let mut metadata: ModelMetadata = serde_json::from_str(text).map_err(|error| ModelError::new(format!("invalid model JSON: {error}")))?;
        metadata.validate()?;
        Ok(metadata)
    }

    /// Canonicalizes the names and rejects unknown summarizations and a date column that is not
    /// one of its table's columns. Duplicate table/column names are the [`DataModel`]'s check;
    /// they are rejected here too so a document cannot produce an ambiguous lookup.
    pub fn validate(&mut self) -> Result<(), ModelError> {
        let mut tables = BTreeMap::new();
        for table in &mut self.tables {
            if tables.insert(table.name.to_ascii_lowercase(), ()).is_some() {
                return Err(ModelError::new(format!("duplicate model table `{}`", table.name)));
            }
            let mut columns = BTreeMap::new();
            for column in &mut table.columns {
                if columns.insert(column.name.to_ascii_lowercase(), ()).is_some() {
                    return Err(ModelError::new(format!("duplicate column `{}` in table `{}`", column.name, table.name)));
                }
                if let Some(name) = column.summarize_by.clone() {
                    let Some(canonical) = summarize_by_name(&name) else {
                        return Err(ModelError::new(format!("column `{}` has unknown summarization `{name}`", column.name)));
                    };
                    column.summarize_by = Some(canonical.to_string());
                }
            }
            if let Some(date) = table.date_column.clone() {
                let Some(column) = table.columns.iter().find(|column| column.name.eq_ignore_ascii_case(&date)) else {
                    return Err(ModelError::new(format!("table `{}` marks unknown column `{date}` as its date column", table.name)));
                };
                table.date_column = Some(column.name.clone());
            }
        }
        Ok(())
    }

    /// A table's metadata, matching names case-insensitively.
    pub fn table(&self, name: &str) -> Option<&TableMetadata> {
        self.tables.iter().find(|table| table.name.eq_ignore_ascii_case(name))
    }

    /// A table's metadata for editing.
    pub fn table_mut(&mut self, name: &str) -> Option<&mut TableMetadata> {
        self.tables.iter_mut().find(|table| table.name.eq_ignore_ascii_case(name))
    }

    /// A column's metadata, matching table and column names case-insensitively.
    pub fn column(&self, table: &str, column: &str) -> Option<&ColumnMetadata> {
        self.table(table)?.columns.iter().find(|entry| entry.name.eq_ignore_ascii_case(column))
    }

    /// A column's metadata for editing.
    pub fn column_mut(&mut self, table: &str, column: &str) -> Option<&mut ColumnMetadata> {
        let table = self.table_mut(table)?;
        table.columns.iter_mut().find(|entry| entry.name.eq_ignore_ascii_case(column))
    }
}

/// A single-column relationship between two model tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    /// `true` when the relationship filters in both directions.
    #[serde(default)]
    pub bidirectional: bool,
    /// Inactive relationships can be activated by `USERELATIONSHIP`.
    #[serde(default)]
    pub inactive: bool,
}

/// A measure definition. The formula is DAX text; [`DataModel::parse_formulas`] parses it into
/// [`Measure::expr`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    pub name: String,
    pub formula: String,
    /// The home table, used by `COUNTROWS()` without an argument and for field-list placement.
    #[serde(default)]
    pub table: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(skip)]
    pub expr: Option<Expr>,
}

/// A model definition loaded from JSON or another workbook adapter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DataModel {
    #[serde(default)]
    pub tables: Vec<ModelTable>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    #[serde(default)]
    pub measures: Vec<Measure>,
}

/// A metadata or formula error found while preparing a model for binding.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct ModelError {
    pub message: String,
}

impl ModelError {
    fn new(message: impl Into<String>) -> Self {
        ModelError { message: message.into() }
    }
}

impl DataModel {
    /// Parses every measure formula and validates table/column/relationship names.
    ///
    /// Names are compared case-insensitively. Duplicate table names, duplicate column names
    /// within a table, and relationships naming missing tables/columns are rejected.
    pub fn parse_formulas(&mut self) -> Result<(), ModelError> {
        let mut tables: BTreeMap<String, usize> = BTreeMap::new();
        for (index, table) in self.tables.iter().enumerate() {
            let key = table.name.to_ascii_lowercase();
            if tables.insert(key, index).is_some() {
                return Err(ModelError::new(format!("duplicate model table `{}`", table.name)));
            }
            let mut columns = BTreeMap::new();
            for column in &table.columns {
                if columns.insert(column.name.to_ascii_lowercase(), ()).is_some() {
                    return Err(ModelError::new(format!("duplicate column `{}` in table `{}`", column.name, table.name)));
                }
            }
        }

        for relationship in &self.relationships {
            self.require_column(&relationship.from_table, &relationship.from_column)?;
            self.require_column(&relationship.to_table, &relationship.to_column)?;
        }

        for measure in &mut self.measures {
            let expr = parse_expression(&measure.formula).map_err(|error| ModelError::new(format!("measure `{}`: {error}", measure.name)))?;
            measure.expr = Some(expr);
        }
        Ok(())
    }

    /// The index of a table, ignoring case.
    pub fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|table| table.name.eq_ignore_ascii_case(name))
    }

    /// The index of a column within a table, ignoring case.
    pub fn column_index(&self, table: usize, name: &str) -> Option<usize> {
        self.tables.get(table)?.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
    }

    /// The index of a measure, ignoring case.
    pub fn measure_index(&self, name: &str) -> Option<usize> {
        self.measures.iter().position(|measure| measure.name.eq_ignore_ascii_case(name))
    }

    /// Tables that contain a column with this name.
    pub fn tables_with_column(&self, name: &str) -> Vec<usize> {
        self.tables
            .iter()
            .enumerate()
            .filter(|(_, table)| table.columns.iter().any(|column| column.name.eq_ignore_ascii_case(name)))
            .map(|(index, _)| index)
            .collect()
    }

    fn require_column(&self, table: &str, column: &str) -> Result<(), ModelError> {
        let Some(table_index) = self.table_index(table) else {
            return Err(ModelError::new(format!("relationship names missing table `{table}`")));
        };
        if self.column_index(table_index, column).is_none() {
            return Err(ModelError::new(format!("relationship names missing column `{column}` in table `{table}`")));
        }
        Ok(())
    }
}

/// The result of [`parse_expression`] for a measure, used by callers that want to surface the
/// parse error separately from the model metadata.
pub fn parse_measure(formula: &str) -> Result<Expr, ParseError> {
    parse_expression(formula)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model JSON `docs/power-pivot.md` documents, written before the metadata fields existed.
    /// It must keep loading (the metadata parser ignores everything it does not own), and adding
    /// the fields must not change what the document means.
    const OLD_SCHEMA: &str = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                    "rows":[["East",10],["West",20]]}],
        "relationships": [],
        "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
    }"#;

    #[test]
    fn old_schema_json_loads_with_default_metadata() {
        let mut model: DataModel = serde_json::from_str(OLD_SCHEMA).expect("old model JSON loads");
        model.parse_formulas().expect("old model JSON validates");
        assert_eq!(model.tables[0].columns.len(), 2, "unknown metadata keys never disturb the model");

        let metadata = ModelMetadata::from_model_json(OLD_SCHEMA).expect("old model JSON has valid metadata");
        let table = metadata.table("sales").expect("the table is in the metadata");
        assert_eq!(table.date_column, None);
        assert!(!metadata.column("Sales", "amount").expect("column metadata").hidden);
        assert_eq!(metadata.column("Sales", "Amount").and_then(|column| column.summarize_by.clone()), None);

        let written = serde_json::to_value(&metadata).expect("metadata serializes");
        assert!(written["tables"][0].get("date_column").is_none(), "an unmarked table writes no date column: {written}");
        assert!(written["tables"][0]["columns"][0].get("hidden").is_none(), "a visible column writes no hidden flag: {written}");
        assert!(written["tables"][0]["columns"][0].get("summarize_by").is_none(), "an absent summarization stays absent: {written}");
        let reloaded = ModelMetadata::from_model_json(&written.to_string()).expect("the written metadata reloads");
        assert_eq!(reloaded, metadata);
    }

    #[test]
    fn new_metadata_is_read_from_the_model_document() {
        let json = r#"{
            "tables": [{"name":"Calendar","date_column":"date",
                        "columns":[{"name":"Date","data_type":"DateTime"},{"name":"Secret","hidden":true,"summarize_by":"do not summarize"}],
                        "rows":[[45292,null]]}],
            "relationships": [],
            "measures": []
        }"#;
        let mut model: DataModel = serde_json::from_str(json).expect("JSON with metadata loads");
        model.parse_formulas().expect("the model validates");
        let metadata = ModelMetadata::from_model_json(json).expect("the metadata validates");
        let table = metadata.table("Calendar").expect("a table");
        assert_eq!(table.date_column.as_deref(), Some("Date"), "the stored name takes the column's spelling");
        let secret = metadata.column("calendar", "SECRET").expect("column metadata");
        assert!(secret.hidden);
        assert_eq!(secret.summarize_by.as_deref(), Some("None"), "the summarization is canonicalized");
    }

    #[test]
    fn metadata_naming_missing_columns_or_aggregates_is_rejected() {
        let bad_date = r#"{"tables":[{"name":"T","columns":[{"name":"A"}],"rows":[],"date_column":"Missing"}]}"#;
        assert!(ModelMetadata::from_model_json(bad_date).is_err(), "a date column that is not a column is rejected");

        let bad_summary = r#"{"tables":[{"name":"T","columns":[{"name":"A","summarize_by":"median"}],"rows":[]}]}"#;
        assert!(ModelMetadata::from_model_json(bad_summary).is_err(), "an unknown summarization is rejected");

        let duplicate = r#"{"tables":[{"name":"T","columns":[{"name":"A"},{"name":"a"}]}]}"#;
        assert!(ModelMetadata::from_model_json(duplicate).is_err(), "ambiguous column names are rejected");
    }

    #[test]
    fn summarize_by_names_match_loosely() {
        assert_eq!(summarize_by_name("SUM"), Some("Sum"));
        assert_eq!(summarize_by_name("average"), Some("Average"));
        assert_eq!(summarize_by_name("Do Not Summarize"), Some("None"));
        assert_eq!(summarize_by_name("median"), None);
    }
}
