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
