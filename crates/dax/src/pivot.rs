//! Minimal pivot grouping over the in-memory model.
//!
//! A pivot groups a table by one or more column references and evaluates a measure once per
//! group. This is the integration point a workbook/PivotTable adapter can call; the full
//! PivotTable layout engine remains outside this crate.

use std::collections::BTreeMap;

use crate::binder::{BindError, Binder, BoundExpr};
use crate::eval::{Evaluator, FilterContext};
use crate::model::DataModel;
use crate::value::{EvalError, Scalar, ScalarKey, TableColumn, TableValue};
use crate::{Expr, parse_expression};

/// One grouping field.
#[derive(Clone, Debug)]
pub struct PivotField {
    pub table: usize,
    pub column: usize,
    pub name: String,
}

/// Evaluates a measure grouped by the supplied row fields (no column fields).
pub fn pivot(model: &DataModel, measure: &str, row_fields: &[PivotField]) -> Result<TableValue, PivotError> {
    pivot_cross(model, measure, row_fields, &[])
}

/// Evaluates a measure grouped by row fields and optional column fields.
///
/// The result has one column per row field followed by one measure column per distinct column
/// group. With no column fields the result is the usual grouped table. Group order is ascending;
/// BLANK is a group of its own. Missing combinations are BLANK.
pub fn pivot_cross(model: &DataModel, measure: &str, row_fields: &[PivotField], column_fields: &[PivotField]) -> Result<TableValue, PivotError> {
    let evaluator = Evaluator::new(model)?;
    let table_index = model
        .measure_index(measure)
        .and_then(|index| model.measures.get(index))
        .and_then(|measure| measure.table.as_deref())
        .and_then(|table| model.table_index(table));
    let Some(table_index) = table_index else {
        return Err(PivotError::Eval(EvalError::new(format!("measure `{measure}` has no home table; pass a table explicitly"))));
    };
    let source = model.tables.get(table_index).ok_or_else(|| PivotError::Eval(EvalError::new("missing table")))?;

    let mut row_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
    let mut column_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
    for row in &source.rows {
        let row_values = group_values(row, row_fields, table_index)?;
        row_groups.entry(row_values.iter().map(ScalarKey::from).collect()).or_insert(row_values);
        if !column_fields.is_empty() {
            let column_values = group_values(row, column_fields, table_index)?;
            column_groups.entry(column_values.iter().map(ScalarKey::from).collect()).or_insert(column_values);
        }
    }
    if column_fields.is_empty() {
        column_groups.insert(Vec::new(), Vec::new());
    }

    let mut columns = row_fields.iter().map(|field| TableColumn { name: field.name.clone() }).collect::<Vec<_>>();
    let column_headers: Vec<(Vec<ScalarKey>, Vec<Scalar>)> = column_groups.into_iter().collect();
    if column_fields.is_empty() {
        columns.push(TableColumn { name: measure.to_string() });
    } else {
        for (_, values) in &column_headers {
            columns.push(TableColumn { name: values.iter().map(|value| value.to_text()).collect::<Vec<_>>().join(" | ") });
        }
    }

    let mut rows = Vec::with_capacity(row_groups.len());
    for (row_key, row_values) in row_groups {
        let mut out = row_values;
        for (column_key, _) in &column_headers {
            let mut filters = FilterContext::default();
            for (field, key) in row_fields.iter().zip(&row_key) {
                filters.set(field.table, field.column, [key.clone()]);
            }
            for (field, key) in column_fields.iter().zip(column_key) {
                filters.set(field.table, field.column, [key.clone()]);
            }
            out.push(evaluator.evaluate_measure(measure, filters)?);
        }
        rows.push(out);
    }
    Ok(TableValue::new(columns, rows))
}

fn group_values(row: &[Scalar], fields: &[PivotField], table_index: usize) -> Result<Vec<Scalar>, PivotError> {
    let mut values = Vec::with_capacity(fields.len());
    for field in fields {
        if field.table != table_index {
            return Err(PivotError::Eval(EvalError::new("pivot fields must belong to the measure table")));
        }
        values.push(row.get(field.column).cloned().unwrap_or(Scalar::Blank));
    }
    Ok(values)
}

/// Parses field references such as `Sales[Region]` against the model.
pub fn pivot_fields(model: &DataModel, fields: &[String]) -> Result<Vec<PivotField>, PivotError> {
    let binder = Binder::new(model);
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let expr: Expr = parse_expression(field).map_err(|error| PivotError::Bind(BindError { message: error.message, span: error.span }))?;
        let bound = binder.bind_expression(&expr)?;
        let BoundExpr::Column { table, column } = bound else {
            return Err(PivotError::Bind(BindError { message: format!("pivot field `{field}` is not a column reference"), span: expr.span }));
        };
        let name =
            model.tables.get(table).and_then(|table| table.columns.get(column)).map(|column| column.name.clone()).unwrap_or_else(|| field.clone());
        out.push(PivotField { table, column, name });
    }
    Ok(out)
}

#[derive(Debug, thiserror::Error)]
pub enum PivotError {
    #[error(transparent)]
    Bind(#[from] BindError),
    #[error(transparent)]
    Eval(#[from] EvalError),
}
