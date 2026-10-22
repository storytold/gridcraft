//! A model-backed `PivotTable` object: Power Pivot-style field areas over the in-memory model.
//!
//! [`PivotTable`] describes a pivot declaratively (source measure, row/column fields, filters,
//! the values area, layout and totals) and [`PivotTable::render`] materializes it as a
//! [`TableValue`] that the engine, the GUI or a future export adapter can consume. The object is
//! data only: it holds no XLSX or pivot-cache XML.
//!
//! # Rendering model
//!
//! * Grouping rows come from the source table's inline rows ([`crate::model::ModelTable::rows`]).
//!   The source table is [`PivotTable::source_table`] when set, otherwise the home table of the
//!   [`PivotTable::source`] measure.
//! * Filters are applied first: a row must match every filter. A filter's `allowed` list is a set
//!   of exact scalar values (case-sensitive for strings); an empty `allowed` list keeps no rows.
//! * Row- and column-field values are grouped ascending in [`ScalarKey`] order, so BLANK sorts
//!   first and is a group of its own (`(Blank)` in labels).
//! * Cells are evaluated through the normal evaluator with a [`FilterContext`] holding the filters
//!   plus the row/column group values, so measures keep time-intelligence and relationship
//!   semantics. A row/column subtotal cell constrains only the fields of its prefix group.
//! * Every grouping field (row, column or filter) must belong to the source table, the same
//!   restriction [`crate::pivot::pivot_cross`] applies. Fields on related tables are not supported
//!   yet.
//!
//! # Layout
//!
//! [`PivotLayout`] controls the row-label columns only; the measure columns are laid out the same
//! way in all three layouts.
//!
//! | Layout | Row labels |
//! | --- | --- |
//! | [`PivotLayout::Tabular`] | One column per row field. Leaf rows repeat every ancestor value; a subtotal row fills its prefix values and leaves the rest blank; the grand total writes `Grand Total` in the first column. |
//! | [`PivotLayout::Outline`] | One column per row field. A leaf row leaves an ancestor cell blank when it repeats the previous leaf row; a subtotal row writes `<value> Total` in the subtotal field's own column. |
//! | [`PivotLayout::Compact`] | One `Row Labels` column. A leaf row shows its deepest value indented two spaces per row-field level; a subtotal row shows `<Field> <value> Total` indented. |
//!
//! Row subtotals are emitted for every prefix group of two or more row fields
//! ([`PivotTable::row_subtotals`]); column subtotals do the same for column fields
//! ([`PivotTable::column_subtotals`]). Grand totals add one row and one column
//! ([`PivotTable::grand_totals`]). The grand-total row is omitted when there are no row fields (the
//! single leaf row already is the total) and when no row survived the filters; likewise the
//! grand-total column needs at least one column field and one surviving column group.
//!
//! # Bounds
//!
//! Rendering is bounded by [`PivotTable::max_cells`]: the number of measure cells
//! (`rows * value columns * values`) must not exceed it or [`PivotTableError::TooManyCells`]
//! is returned. Nothing in this module panics on empty models, empty tables, unknown names or
//! out-of-range columns; a row shorter than a field's column index groups as BLANK.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::eval::{Evaluator, FilterContext};
use crate::model::DataModel;
use crate::pivot::{PivotError, PivotField, pivot_fields};
use crate::value::{Scalar, ScalarKey, TableColumn, TableValue};

/// The default [`PivotTable::max_cells`] budget.
pub const DEFAULT_MAX_CELLS: usize = 1_000_000;

/// Separator used when a header collapses several column fields.
const COLUMN_SEPARATOR: &str = " | ";
/// Text used for a BLANK group in a label.
const BLANK_LABEL: &str = "(Blank)";
/// The grand-total label.
const TOTAL_LABEL: &str = "Grand Total";
/// Name of the single label column of the compact layout.
const ROW_LABELS: &str = "Row Labels";
/// Indent unit of the compact layout, per row-field level.
const INDENT: &str = "  ";

/// The row-label layout of a rendered pivot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PivotLayout {
    /// One label column holding the deepest value of the row, indented per level.
    #[default]
    Compact,
    /// One label column per row field, with every ancestor value repeated.
    Tabular,
    /// One label column per row field, with repeated ancestors blank and named subtotals.
    Outline,
}

/// A filter over one field: only rows whose value is in `allowed` pass.
///
/// An empty `allowed` list keeps nothing (the pivot renders no rows). Values compare exactly;
/// string values are case-sensitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PivotFilter {
    /// A field reference in `Table[Column]` form, resolved with [`crate::pivot::pivot_fields`].
    pub field: String,
    /// The allowed values. Membership is exact scalar equality.
    #[serde(default)]
    pub allowed: Vec<Scalar>,
}

impl PivotFilter {
    /// A filter allowing the supplied values.
    pub fn new(field: impl Into<String>, allowed: impl IntoIterator<Item = Scalar>) -> Self {
        PivotFilter { field: field.into(), allowed: allowed.into_iter().collect() }
    }
}

/// A Power Pivot-style pivot table over an in-memory [`DataModel`].
///
/// The struct is a plain data description: build it directly, with the `with_*` builders, or
/// deserialize it from JSON. Call [`PivotTable::render`] to turn it into a [`TableValue`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PivotTable {
    /// Display name; only used in error messages.
    pub name: String,
    /// The measure the pivot is built around. It must exist and supplies the grouping table
    /// unless [`PivotTable::source_table`] overrides it.
    pub source: String,
    /// The measures of the values area, in column order. Empty means `[source]`.
    #[serde(default)]
    pub values: Vec<String>,
    /// Row-field references such as `Sales[Region]`, outermost first.
    #[serde(default)]
    pub row_fields: Vec<String>,
    /// Column-field references such as `Sales[Category]`, outermost first.
    #[serde(default)]
    pub column_fields: Vec<String>,
    /// Slicer-style filters, ANDed together.
    #[serde(default)]
    pub filters: Vec<PivotFilter>,
    /// Row-label layout.
    #[serde(default)]
    pub layout: PivotLayout,
    /// Emit a subtotal row per row-field prefix group. Needs two or more row fields.
    #[serde(default)]
    pub row_subtotals: bool,
    /// Emit a subtotal column per column-field prefix group. Needs two or more column fields.
    #[serde(default)]
    pub column_subtotals: bool,
    /// Emit the grand-total row and column.
    #[serde(default)]
    pub grand_totals: bool,
    /// Overrides the grouping table. Defaults to the `source` measure's home table.
    #[serde(default)]
    pub source_table: Option<String>,
    /// Maximum number of measure cells `render` may materialize.
    #[serde(default = "default_max_cells")]
    pub max_cells: usize,
}

fn default_max_cells() -> usize {
    DEFAULT_MAX_CELLS
}

impl Default for PivotTable {
    fn default() -> Self {
        PivotTable {
            name: String::new(),
            source: String::new(),
            values: Vec::new(),
            row_fields: Vec::new(),
            column_fields: Vec::new(),
            filters: Vec::new(),
            layout: PivotLayout::default(),
            row_subtotals: false,
            column_subtotals: false,
            grand_totals: false,
            source_table: None,
            max_cells: DEFAULT_MAX_CELLS,
        }
    }
}

impl PivotTable {
    /// A pivot over `source` with no fields, no filters and no totals.
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        PivotTable { name: name.into(), source: source.into(), ..PivotTable::default() }
    }

    /// Sets the measures of the values area. An empty list restores `[source]`.
    #[must_use]
    pub fn with_values(mut self, values: Vec<String>) -> Self {
        self.values = values;
        self
    }

    /// Sets the row fields, outermost first.
    #[must_use]
    pub fn with_row_fields(mut self, fields: Vec<String>) -> Self {
        self.row_fields = fields;
        self
    }

    /// Sets the column fields, outermost first.
    #[must_use]
    pub fn with_column_fields(mut self, fields: Vec<String>) -> Self {
        self.column_fields = fields;
        self
    }

    /// Appends a filter.
    #[must_use]
    pub fn with_filter(mut self, filter: PivotFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Sets the row-label layout.
    #[must_use]
    pub fn with_layout(mut self, layout: PivotLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Enables or disables row subtotals.
    #[must_use]
    pub fn with_row_subtotals(mut self, enabled: bool) -> Self {
        self.row_subtotals = enabled;
        self
    }

    /// Enables or disables column subtotals.
    #[must_use]
    pub fn with_column_subtotals(mut self, enabled: bool) -> Self {
        self.column_subtotals = enabled;
        self
    }

    /// Enables or disables the grand-total row and column.
    #[must_use]
    pub fn with_grand_totals(mut self, enabled: bool) -> Self {
        self.grand_totals = enabled;
        self
    }

    /// Overrides the grouping table.
    #[must_use]
    pub fn with_source_table(mut self, table: impl Into<String>) -> Self {
        self.source_table = Some(table.into());
        self
    }

    /// Sets the cell budget.
    #[must_use]
    pub fn with_max_cells(mut self, max_cells: usize) -> Self {
        self.max_cells = max_cells;
        self
    }

    /// The measures of the values area, defaulting to the source measure.
    pub fn value_measures(&self) -> Vec<String> {
        if self.values.is_empty() { vec![self.source.clone()] } else { self.values.clone() }
    }

    /// Renders the pivot against `model`.
    ///
    /// The returned table has the row-label columns of the layout followed by one measure column
    /// per column group and value, and one row per row group, subtotal and grand total. Its
    /// lineage is the source table index. All errors are returned, never panicked.
    pub fn render(&self, model: &DataModel) -> Result<TableValue, PivotTableError> {
        Plan::resolve(self, model)?.render()
    }

    /// The name used in error messages.
    fn display_name(&self) -> String {
        if self.name.trim().is_empty() { "<unnamed>".to_string() } else { self.name.clone() }
    }
}

/// A resolved, ready-to-render pivot: field indices bound against one model.
struct Plan<'a> {
    model: &'a DataModel,
    evaluator: Evaluator<'a>,
    pivot: &'a PivotTable,
    table: usize,
    measures: Vec<String>,
    row_fields: Vec<PivotField>,
    column_fields: Vec<PivotField>,
    filters: Vec<(PivotField, BTreeSet<ScalarKey>)>,
}

impl<'a> Plan<'a> {
    /// Binds every name in `pivot` against `model`, or reports the first problem.
    fn resolve(pivot: &'a PivotTable, model: &'a DataModel) -> Result<Self, PivotTableError> {
        let name = pivot.display_name();
        let source = pivot.source.trim();
        if source.is_empty() {
            return Err(PivotTableError::MissingSource { pivot: name });
        }
        if model.measure_index(source).is_none() {
            return Err(PivotTableError::UnknownMeasure { pivot: name.clone(), measure: source.to_string() });
        }
        let measures = pivot.value_measures();
        for measure in &measures {
            if model.measure_index(measure).is_none() {
                return Err(PivotTableError::UnknownMeasure { pivot: name.clone(), measure: measure.clone() });
            }
        }

        let table = match &pivot.source_table {
            Some(table) => model.table_index(table).ok_or_else(|| PivotTableError::UnknownTable(table.clone()))?,
            None => model
                .measure_index(source)
                .and_then(|index| model.measures.get(index))
                .and_then(|measure| measure.table.as_deref())
                .and_then(|table| model.table_index(table))
                .ok_or_else(|| PivotTableError::NoSourceTable { pivot: name.clone(), measure: source.to_string() })?,
        };
        let table_name = model.tables.get(table).map(|table| table.name.clone()).unwrap_or_else(|| table.to_string());

        let row_fields = pivot_fields(model, &pivot.row_fields)?;
        let column_fields = pivot_fields(model, &pivot.column_fields)?;
        for field in row_fields.iter().chain(&column_fields) {
            require_owned(field, table, &table_name)?;
        }

        let mut filters = Vec::with_capacity(pivot.filters.len());
        for filter in &pivot.filters {
            let resolved = pivot_fields(model, std::slice::from_ref(&filter.field))?;
            let field =
                resolved.into_iter().next().ok_or_else(|| PivotTableError::UnknownField { pivot: name.clone(), field: filter.field.clone() })?;
            require_owned(&field, table, &table_name)?;
            let allowed = filter.allowed.iter().map(ScalarKey::from).collect();
            filters.push((field, allowed));
        }

        let evaluator = Evaluator::new(model).map_err(PivotError::from)?;
        Ok(Plan { model, evaluator, pivot, table, measures, row_fields, column_fields, filters })
    }

    fn render(&self) -> Result<TableValue, PivotTableError> {
        let name = self.pivot.display_name();
        let table = self.model.tables.get(self.table).ok_or_else(|| PivotTableError::UnknownTable(name.clone()))?;

        // Group the source rows, ascending, skipping rows the filters reject.
        let mut row_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
        let mut column_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
        for row in &table.rows {
            if !self.passes(row) {
                continue;
            }
            let values = self.group_values(row, &self.row_fields);
            row_groups.entry(values.iter().map(ScalarKey::from).collect()).or_insert(values);
            if !self.column_fields.is_empty() {
                let values = self.group_values(row, &self.column_fields);
                column_groups.entry(values.iter().map(ScalarKey::from).collect()).or_insert(values);
            }
        }
        if self.column_fields.is_empty() {
            column_groups.insert(Vec::new(), Vec::new());
        }

        let columns = self.column_layout(&column_groups);
        let rows = self.row_layout(&row_groups);
        self.check_budget(rows.len(), columns.len())?;

        let mut header = self.label_columns();
        for group in &columns {
            for measure in &self.measures {
                header.push(TableColumn { name: self.value_column_name(group, measure) });
            }
        }

        let mut out_rows = Vec::with_capacity(rows.len());
        let mut previous_leaf: Option<Vec<Scalar>> = None;
        for row_group in &rows {
            let mut out = self.row_header_cells(row_group, previous_leaf.as_deref());
            if row_group.kind == RowKind::Leaf {
                previous_leaf = Some(row_group.values.clone());
            }
            for group in &columns {
                let context = self.filter_context(&row_group.key, &group.key);
                for measure in &self.measures {
                    let value = self.evaluator.evaluate_measure(measure, context.clone()).map_err(PivotError::from)?;
                    out.push(value);
                }
            }
            out_rows.push(out);
        }
        Ok(TableValue::new(header, out_rows).with_lineage(self.table))
    }

    /// Whether every filter accepts `row`.
    fn passes(&self, row: &[Scalar]) -> bool {
        self.filters.iter().all(|(field, allowed)| {
            let value = row.get(field.column).cloned().unwrap_or(Scalar::Blank);
            allowed.contains(&ScalarKey::from(&value))
        })
    }

    /// The values of `fields` in `row`; a missing column groups as BLANK.
    fn group_values(&self, row: &[Scalar], fields: &[PivotField]) -> Vec<Scalar> {
        fields.iter().map(|field| row.get(field.column).cloned().unwrap_or(Scalar::Blank)).collect()
    }

    /// The filter context of one cell: the filters plus the row and column group values.
    fn filter_context(&self, row_key: &[ScalarKey], column_key: &[ScalarKey]) -> FilterContext {
        let mut context = FilterContext::default();
        for (field, allowed) in &self.filters {
            context.set(field.table, field.column, allowed.iter().cloned());
        }
        for (field, key) in self.row_fields.iter().zip(row_key) {
            context.set(field.table, field.column, [key.clone()]);
        }
        for (field, key) in self.column_fields.iter().zip(column_key) {
            context.set(field.table, field.column, [key.clone()]);
        }
        context
    }

    /// Every rendered column group: leaf groups plus subtotal and grand-total groups.
    fn column_layout(&self, groups: &BTreeMap<Vec<ScalarKey>, Vec<Scalar>>) -> Vec<ColumnGroup> {
        let levels = self.column_fields.len();
        let leaves: Vec<(&Vec<ScalarKey>, &Vec<Scalar>)> = groups.iter().collect();
        let mut out = Vec::with_capacity(leaves.len());
        for (index, entry) in leaves.iter().enumerate() {
            let key: &[ScalarKey] = entry.0.as_slice();
            let values: &[Scalar] = entry.1.as_slice();
            out.push(ColumnGroup { key: key.to_vec(), values: values.to_vec(), is_total: false });
            if !self.pivot.column_subtotals {
                continue;
            }
            let next = leaves.get(index + 1).map(|entry| entry.0.as_slice());
            for level in (1..levels).rev() {
                if prefix_closed(key, next, level) {
                    out.push(ColumnGroup { key: key[..level].to_vec(), values: values[..level].to_vec(), is_total: true });
                }
            }
        }
        if self.pivot.grand_totals && levels > 0 && !groups.is_empty() {
            out.push(ColumnGroup { key: Vec::new(), values: Vec::new(), is_total: true });
        }
        out
    }

    /// Every rendered row: leaf groups plus subtotal and grand-total rows.
    fn row_layout(&self, groups: &BTreeMap<Vec<ScalarKey>, Vec<Scalar>>) -> Vec<RowGroup> {
        let levels = self.row_fields.len();
        let leaves: Vec<(&Vec<ScalarKey>, &Vec<Scalar>)> = groups.iter().collect();
        let mut out = Vec::with_capacity(leaves.len());
        for (index, entry) in leaves.iter().enumerate() {
            let key: &[ScalarKey] = entry.0.as_slice();
            let values: &[Scalar] = entry.1.as_slice();
            out.push(RowGroup { key: key.to_vec(), values: values.to_vec(), kind: RowKind::Leaf });
            if !self.pivot.row_subtotals || levels < 2 {
                continue;
            }
            let next = leaves.get(index + 1).map(|entry| entry.0.as_slice());
            for level in (1..levels).rev() {
                if prefix_closed(key, next, level) {
                    out.push(RowGroup { key: key[..level].to_vec(), values: values[..level].to_vec(), kind: RowKind::Subtotal(level) });
                }
            }
        }
        if self.pivot.grand_totals && levels > 0 && !groups.is_empty() {
            out.push(RowGroup { key: Vec::new(), values: Vec::new(), kind: RowKind::GrandTotal });
        }
        out
    }

    /// The row-label columns of the layout.
    fn label_columns(&self) -> Vec<TableColumn> {
        match self.pivot.layout {
            PivotLayout::Compact => vec![TableColumn { name: ROW_LABELS.to_string() }],
            PivotLayout::Tabular | PivotLayout::Outline => self.row_fields.iter().map(|field| TableColumn { name: field.name.clone() }).collect(),
        }
    }

    /// The header of one measure column of `group`.
    fn value_column_name(&self, group: &ColumnGroup, measure: &str) -> String {
        if self.column_fields.is_empty() {
            return measure.to_string();
        }
        let label = group.label();
        if self.measures.len() > 1 { format!("{label}{COLUMN_SEPARATOR}{measure}") } else { label }
    }

    /// The label cells of one rendered row.
    fn row_header_cells(&self, row: &RowGroup, previous_leaf: Option<&[Scalar]>) -> Vec<Scalar> {
        let levels = self.row_fields.len();
        match self.pivot.layout {
            PivotLayout::Tabular => {
                let mut cells = vec![Scalar::Blank; levels];
                match row.kind {
                    RowKind::Leaf | RowKind::Subtotal(_) => {
                        for (cell, value) in cells.iter_mut().zip(&row.values) {
                            *cell = value.clone();
                        }
                    }
                    RowKind::GrandTotal => set_total_label(&mut cells, TOTAL_LABEL),
                }
                cells
            }
            PivotLayout::Outline => {
                let mut cells = vec![Scalar::Blank; levels];
                match row.kind {
                    RowKind::Leaf => {
                        let deepest = row.values.len().saturating_sub(1);
                        for (level, (cell, value)) in cells.iter_mut().zip(&row.values).enumerate() {
                            let repeats = level < deepest && previous_leaf.is_some_and(|previous| previous.get(level) == Some(value));
                            *cell = if repeats { Scalar::Blank } else { value.clone() };
                        }
                    }
                    RowKind::Subtotal(level) => {
                        if let Some(cell) = cells.get_mut(level.saturating_sub(1)) {
                            *cell = Scalar::String(format!("{} Total", display_value(&row.values[level - 1])));
                        }
                    }
                    RowKind::GrandTotal => set_total_label(&mut cells, TOTAL_LABEL),
                }
                cells
            }
            PivotLayout::Compact => {
                let text = match row.kind {
                    RowKind::Leaf if levels == 0 => TOTAL_LABEL.to_string(),
                    RowKind::Leaf => {
                        let indent = INDENT.repeat(row.values.len().saturating_sub(1));
                        let deepest = row.values.last().cloned().unwrap_or(Scalar::Blank);
                        format!("{indent}{}", display_value(&deepest))
                    }
                    RowKind::Subtotal(level) => {
                        let indent = INDENT.repeat(level.saturating_sub(1));
                        let field = self.row_fields.get(level - 1).map(|field| field.name.as_str()).unwrap_or_default();
                        let value = row.values.get(level - 1).cloned().unwrap_or(Scalar::Blank);
                        format!("{indent}{field} {} Total", display_value(&value))
                    }
                    RowKind::GrandTotal => TOTAL_LABEL.to_string(),
                };
                vec![Scalar::String(text)]
            }
        }
    }

    /// Rejects pivots that would materialize more than [`PivotTable::max_cells`] measure cells.
    fn check_budget(&self, rows: usize, columns: usize) -> Result<(), PivotTableError> {
        let cells = rows.checked_mul(columns).and_then(|cells| cells.checked_mul(self.measures.len()));
        match cells {
            Some(cells) if cells <= self.pivot.max_cells => Ok(()),
            _ => Err(PivotTableError::TooManyCells {
                pivot: self.pivot.display_name(),
                rows,
                columns,
                measures: self.measures.len(),
                limit: self.pivot.max_cells,
            }),
        }
    }
}

/// One rendered column group: a leaf column-field combination, or a subtotal/grand-total column.
struct ColumnGroup {
    /// The column-field values constrained by this group; a prefix for subtotals, empty for the
    /// grand total.
    key: Vec<ScalarKey>,
    /// The group's column-field values, parallel to `key`.
    values: Vec<Scalar>,
    /// `true` for a subtotal or grand-total group.
    is_total: bool,
}

impl ColumnGroup {
    /// The group's header label, before the measure name is appended.
    fn label(&self) -> String {
        if !self.is_total {
            return value_label(&self.values);
        }
        if self.values.is_empty() { TOTAL_LABEL.to_string() } else { format!("{} Total", value_label(&self.values)) }
    }
}

/// One rendered row.
struct RowGroup {
    /// The row-field values constrained by this row; a prefix for subtotals, empty for the grand
    /// total.
    key: Vec<ScalarKey>,
    /// The row's field values, parallel to `key`.
    values: Vec<Scalar>,
    kind: RowKind,
}

/// What kind of row a [`RowGroup`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    /// A leaf row: one distinct row-field combination.
    Leaf,
    /// A subtotal over the first `level` row fields.
    Subtotal(usize),
    /// The grand total.
    GrandTotal,
}

/// Whether the `level`-long field prefix of `key` differs from `next`'s.
fn prefix_closed(key: &[ScalarKey], next: Option<&[ScalarKey]>, level: usize) -> bool {
    if level > key.len() {
        return false;
    }
    match next {
        Some(next) if next.len() >= level => next[..level] != key[..level],
        // A shorter or absent next key closes every remaining prefix.
        Some(_) => true,
        None => true,
    }
}

/// Rejects a field that does not belong to the grouping table.
fn require_owned(field: &PivotField, table: usize, table_name: &str) -> Result<(), PivotTableError> {
    if field.table == table { Ok(()) } else { Err(PivotTableError::ForeignField { field: field.name.clone(), table: table_name.to_string() }) }
}

/// Writes `label` into the first of `cells`, when there is one.
fn set_total_label(cells: &mut [Scalar], label: &str) {
    if let Some(cell) = cells.first_mut() {
        *cell = Scalar::String(label.to_string());
    }
}

/// The label of a group's values, joined outermost first.
fn value_label(values: &[Scalar]) -> String {
    values.iter().map(display_value).collect::<Vec<_>>().join(COLUMN_SEPARATOR)
}

/// The label of one scalar; BLANK renders as `(Blank)`.
fn display_value(value: &Scalar) -> String {
    if value.is_blank() { BLANK_LABEL.to_string() } else { value.to_text() }
}

/// A pivot definition or rendering error.
#[derive(Debug, thiserror::Error)]
pub enum PivotTableError {
    /// A field reference or measure formula failed to bind.
    #[error(transparent)]
    Pivot(#[from] PivotError),
    /// The pivot names no source measure.
    #[error("pivot `{pivot}` has no source measure")]
    MissingSource { pivot: String },
    /// A measure name is not in the model.
    #[error("pivot `{pivot}` names unknown measure `{measure}`")]
    UnknownMeasure { pivot: String, measure: String },
    /// A table name is not in the model.
    #[error("unknown table `{0}`")]
    UnknownTable(String),
    /// The source measure has no home table and no override was set.
    #[error("measure `{measure}` has no home table; set `source_table` (pivot `{pivot}`)")]
    NoSourceTable { pivot: String, measure: String },
    /// A field reference did not resolve to a column.
    #[error("pivot `{pivot}` has an invalid field `{field}`")]
    UnknownField { pivot: String, field: String },
    /// A row, column or filter field belongs to another table.
    #[error("pivot field `{field}` must belong to source table `{table}`")]
    ForeignField { field: String, table: String },
    /// The rendered table would exceed the cell budget.
    #[error("pivot `{pivot}` would materialize {rows} rows x {columns} columns x {measures} values, over the limit of {limit} cells")]
    TooManyCells { pivot: String, rows: usize, columns: usize, measures: usize, limit: usize },
}
