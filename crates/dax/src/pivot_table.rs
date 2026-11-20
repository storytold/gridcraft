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
//!   [`PivotTable::source`] measure. When the model materializes faithfully, grouping reads the
//!   evaluator's columnar substrate instead: filters become per-column row-id sets and each field
//!   value is a column read, so the model rows are neither walked nor cloned
//!   ([`PivotTable::render_with_stats`] reports the counters).
//! * Filters are applied first: a row must match every filter. A filter's `allowed` list is a set
//!   of scalar values matched with the model collation (text ignores case, like `=`); an empty
//!   `allowed` list keeps no rows.
//! * Row- and column-field values are grouped ascending in [`ScalarKey`] order, so BLANK sorts
//!   first and is a group of its own (`(Blank)` in labels).
//! * Cells are evaluated through the normal evaluator with a [`FilterContext`] holding the filters
//!   plus the row/column group values, so measures keep time-intelligence and relationship
//!   semantics. A row/column subtotal cell constrains only the fields of its prefix group.
//! * A grouping field (row, column or filter) may live on the source table or on a table the
//!   model's relationships reach from it, which is what makes a star or snowflake pivot work:
//!   a fact table with fields from its dimensions. Each related field is bound once, at plan time,
//!   to its column plus a *lookup path*: a chain of **active** relationships whose every step runs
//!   from its `from` (many) side to its `to` (one) side. That is the only direction one source row
//!   can follow — it reads the `from` key and matches the `to` key, like `RELATED` — so a field
//!   only reachable the other way round, through a one-to-many step, is rejected
//!   ([`PivotTableError::FanOutField`]): grouping by it would have to fan one source row out into
//!   several groups, which a pivot cell cannot express.
//! * A source row's related value is the related row's column, found through the storage's
//!   [`RelationshipIndex`](crate::storage::RelationshipIndex) key indexes (or through the model
//!   rows when the model is not faithfully materialized). A key with no related row groups as
//!   BLANK. A key duplicated on the one side resolves to the **lowest row id**, so a source row
//!   still reaches exactly one related row and its measure value is counted once.
//! * The path must be unique. Two active relationships between the same pair of tables, or a
//!   second chain to the same table (what a cycle in the relationship graph offers), are rejected
//!   ([`PivotTableError::AmbiguousFieldPath`]) because a pivot field names a table and a column,
//!   not a relationship. The search is bounded, so a relationship graph dense enough to exhaust its
//!   path budget is rejected the same way rather than searched forever. A field reachable only
//!   through an inactive relationship is rejected
//!   ([`PivotTableError::InactiveRelationship`]): `USERELATIONSHIP` is not supported in pivots, so
//!   the model has to mark the relationship active. A field on a table no relationship connects is
//!   rejected ([`PivotTableError::ForeignField`]).
//! * A cell of a related field's group is evaluated with the group value as a filter on the
//!   *related table*, so the evaluator's own relationship propagation (through the same indexes,
//!   over any number of hops) restricts the source table. A group value the relationship cannot
//!   resolve — an orphan key, shown as `(Blank)` — therefore selects no source rows and the
//!   measure returns its empty-set value (BLANK for `SUM`), the model's inner-join semantics.
//!   A BLANK foreign key is an orphan for the same reason and never joins a one-side row, even one
//!   whose key is BLANK: the cell filter and the relationship join both drop it, which is the
//!   VertiPaq rule the related-pivot suites pin.
//!   A related field whose one side repeats a key resolves to its lowest row id for the *label*,
//!   while the cell's filter passes every related key, so a source row can be counted in more than
//!   one leaf cell in such a model; `integrity` reports that duplicate one-side key.
//!   A label column of a related field is named `Table[Column]`, so two dimensions with a column
//!   of the same name stay distinguishable; source-table fields keep the bare column name.
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

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::eval::{Evaluator, FilterContext, StorageStats};
use crate::model::DataModel;
use crate::pivot::{PivotError, PivotField, pivot_fields};
use crate::storage::ModelStorage;
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
/// Maximum number of relationships one related field may follow. Star and snowflake models need
/// one hop per dimension level; the bound only exists so a cyclic relationship graph cannot make
/// the path search unbounded.
const MAX_RELATIONSHIP_HOPS: usize = 8;

/// Maximum number of search steps one field-path lookup may spend: one per table state popped from
/// the work stack and one per relationship examined for it. A search that spends the budget cannot
/// prove a field's path is unique, so the field is rejected as ambiguous instead of being bound to
/// a partial or empty chain: a dense relationship graph then cannot stall a pivot.
const MAX_PATH_SEARCH_STEPS: usize = 100_000;

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
/// An empty `allowed` list keeps nothing (the pivot renders no rows), unless [`PivotFilter::all`]
/// is set: that is the page field's `(All)` — a filter that names its field but keeps every value.
/// Values compare exactly; string values are case-sensitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PivotFilter {
    /// A field reference in `Table[Column]` form, resolved with [`crate::pivot::pivot_fields`].
    pub field: String,
    /// The allowed values, matched with the model collation: text compares case-insensitively,
    /// everything else exactly.
    #[serde(default)]
    pub allowed: Vec<Scalar>,
    /// Keep every value (`(All)` in Excel): the filter is shown but restricts nothing. `allowed`
    /// is then empty; a filter with neither members nor this flag keeps nothing.
    #[serde(default)]
    pub all: bool,
}

impl PivotFilter {
    /// A filter allowing the supplied values.
    pub fn new(field: impl Into<String>, allowed: impl IntoIterator<Item = Scalar>) -> Self {
        PivotFilter { field: field.into(), allowed: allowed.into_iter().collect(), all: false }
    }

    /// The page field's `(All)`: names the field and keeps every value.
    pub fn all(field: impl Into<String>) -> Self {
        PivotFilter { field: field.into(), allowed: Vec::new(), all: true }
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

    /// Renders the pivot and returns the evaluator's [`StorageStats`] alongside the table.
    ///
    /// The counters show whether the grouping and the measure cells were served by the columnar
    /// substrate, which is how tests prove a pivot does not silently walk model rows.
    pub fn render_with_stats(&self, model: &DataModel) -> Result<(TableValue, StorageStats), PivotTableError> {
        let plan = Plan::resolve(self, model)?;
        let table = plan.render()?;
        Ok((table, plan.evaluator.storage_stats()))
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
    row_fields: Vec<BoundField>,
    column_fields: Vec<BoundField>,
    filters: Vec<(BoundField, BTreeSet<ScalarKey>)>,
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

        let row_fields = bind_fields(model, table, &table_name, &pivot.row_fields)?;
        let column_fields = bind_fields(model, table, &table_name, &pivot.column_fields)?;

        let mut filters = Vec::with_capacity(pivot.filters.len());
        for filter in &pivot.filters {
            // `(All)` names the field but restricts nothing, so it contributes no filter and no
            // filter context.
            if filter.all {
                // Still resolve the field: a filter over an unknown column is an error either way.
                pivot_fields(model, std::slice::from_ref(&filter.field))?;
                continue;
            }
            let resolved = pivot_fields(model, std::slice::from_ref(&filter.field))?;
            let field =
                resolved.into_iter().next().ok_or_else(|| PivotTableError::UnknownField { pivot: name.clone(), field: filter.field.clone() })?;
            let bound = bind_field(model, table, &table_name, field)?;
            let allowed = filter.allowed.iter().map(|value| crate::value::folded_key(&ScalarKey::from(value))).collect();
            filters.push((bound, allowed));
        }

        let evaluator = Evaluator::new(model).map_err(PivotError::from)?;
        Ok(Plan { model, evaluator, pivot, table, measures, row_fields, column_fields, filters })
    }

    fn render(&self) -> Result<TableValue, PivotTableError> {
        let name = self.pivot.display_name();
        if self.model.tables.get(self.table).is_none() {
            return Err(PivotTableError::UnknownTable(name));
        }

        // Group the source rows, ascending, skipping rows the filters reject. With a faithful
        // columnar substrate the groups are read straight from columns: the filters become
        // row-id sets and each field value a column read (following relationship indexes for a
        // related field), so the model rows are neither walked nor cloned.
        let mut row_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
        let mut column_groups: BTreeMap<Vec<ScalarKey>, Vec<Scalar>> = BTreeMap::new();
        match self.source_rows() {
            SourceRows::Columns { storage, rows } => {
                for row in rows {
                    let values = self.column_group_values(storage, row, &self.row_fields);
                    // The group key folds text exactly like the cell filter, so `East` and
                    // `east` are one leaf; the first spelling seen is the one displayed.
                    row_groups.entry(values.iter().map(crate::value::distinct_key).collect()).or_insert(values);
                    if !self.column_fields.is_empty() {
                        let values = self.column_group_values(storage, row, &self.column_fields);
                        column_groups.entry(values.iter().map(crate::value::distinct_key).collect()).or_insert(values);
                    }
                }
            }
            SourceRows::Rows(rows) => {
                let lookups = hop_key_lookups(self.model, self.row_fields.iter().chain(&self.column_fields));
                for row in rows.iter() {
                    let values = self.model_group_values(&lookups, row, &self.row_fields);
                    row_groups.entry(values.iter().map(crate::value::distinct_key).collect()).or_insert(values);
                    if !self.column_fields.is_empty() {
                        let values = self.model_group_values(&lookups, row, &self.column_fields);
                        column_groups.entry(values.iter().map(crate::value::distinct_key).collect()).or_insert(values);
                    }
                }
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

    /// The source rows the pivot groups: row ids from the substrate when it can answer, the model
    /// rows otherwise.
    fn source_rows(&self) -> SourceRows<'_> {
        if let Some(storage) = self.usable_storage()
            && let Some(rows) = self.column_rows()
        {
            self.evaluator.note_pivot_rows(rows.len());
            return SourceRows::Columns { storage, rows };
        }
        SourceRows::Rows(self.filtered_model_rows())
    }

    /// The substrate when grouping may read it: faithful, and its relationship list mirrors the
    /// model's so a [`Hop`]'s model index selects the same relationship.
    fn usable_storage(&self) -> Option<&ModelStorage> {
        let storage = self.evaluator.storage()?;
        if !storage.is_faithful() || !storage_mirrors_model(storage, self.model) {
            return None;
        }
        Some(storage)
    }

    /// Source-table row ids that survive the pivot filters, read from the columnar substrate.
    ///
    /// The filters become a [`FilterContext`] and the evaluator answers with
    /// [`Evaluator::filtered_row_ids`](crate::eval::Evaluator::filtered_row_ids): its own column
    /// scans plus its relationship propagation, so a filter on a related table restricts the source
    /// table through the same indexes the cells use. `None` when the model is not faithfully
    /// materialized; the caller then groups the model rows.
    fn column_rows(&self) -> Option<Vec<usize>> {
        self.evaluator.filtered_row_ids(self.table, &self.filter_context(&[], &[]))
    }

    /// The source rows that survive the pivot filters, read from the model.
    ///
    /// Filters on the source table alone keep the plain row test the row scan always used (and are
    /// borrowed when there is no filter at all). A filter on a related table asks the evaluator,
    /// which propagates it along the active relationship chain when the storage cannot answer.
    fn filtered_model_rows(&self) -> Cow<'_, [Vec<Scalar>]> {
        let Some(table) = self.model.tables.get(self.table) else { return Cow::Owned(Vec::new()) };
        if !self.filters.iter().all(|(field, _)| field.field.table == self.table) {
            return Cow::Owned(self.evaluator.table(self.table, &self.filter_context(&[], &[])).rows);
        }
        if self.filters.is_empty() {
            return Cow::Borrowed(&table.rows);
        }
        Cow::Owned(table.rows.iter().filter(|row| self.passes(row)).cloned().collect())
    }

    /// Whether every filter accepts `row`.
    fn passes(&self, row: &[Scalar]) -> bool {
        self.filters.iter().all(|(field, allowed)| {
            let value = row.get(field.field.column).cloned().unwrap_or(Scalar::Blank);
            allowed.contains(&crate::value::distinct_key(&value))
        })
    }

    /// The values of `fields` for one source row id, read from columns.
    fn column_group_values(&self, storage: &ModelStorage, row: usize, fields: &[BoundField]) -> Vec<Scalar> {
        fields.iter().map(|field| column_field_value(storage, self.table, row, field)).collect()
    }

    /// The values of `fields` for one source model row, through the prebuilt hop indexes.
    fn model_group_values(&self, lookups: &HopKeyLookups, row: &[Scalar], fields: &[BoundField]) -> Vec<Scalar> {
        fields.iter().map(|field| model_field_value(self.model, lookups, row, field)).collect()
    }

    /// The filter context of one cell: the filters plus the row and column group values.
    ///
    /// A related field's group value is set on the related table, so the evaluator's relationship
    /// propagation restricts the source table exactly as a slicer on that dimension would.
    fn filter_context(&self, row_key: &[ScalarKey], column_key: &[ScalarKey]) -> FilterContext {
        let mut context = FilterContext::default();
        // Several filters on one column intersect, the same way `passes` ANDs them for the groups:
        // the last one must not silently win in the cells while the groups use all of them.
        let mut merged: BTreeMap<(usize, usize), BTreeSet<ScalarKey>> = BTreeMap::new();
        for (field, allowed) in &self.filters {
            let key = (field.field.table, field.field.column);
            match merged.get_mut(&key) {
                Some(existing) => *existing = existing.intersection(allowed).cloned().collect(),
                None => {
                    merged.insert(key, allowed.clone());
                }
            }
        }
        for ((table, column), allowed) in merged {
            context.set(table, column, allowed);
        }
        for (field, key) in self.row_fields.iter().zip(row_key) {
            narrow_to_key(&mut context, field.field.table, field.field.column, key);
        }
        for (field, key) in self.column_fields.iter().zip(column_key) {
            narrow_to_key(&mut context, field.field.table, field.field.column, key);
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
            PivotLayout::Tabular | PivotLayout::Outline => self.row_fields.iter().map(|field| TableColumn { name: field.label.clone() }).collect(),
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
                        // Blank a label only while every level up to it repeats the previous leaf:
                        // a new ancestor starts a new group, so its inner labels show again.
                        let mut same_prefix = previous_leaf.is_some();
                        for (level, (cell, value)) in cells.iter_mut().zip(&row.values).enumerate() {
                            same_prefix = same_prefix && previous_leaf.is_some_and(|previous| previous.get(level) == Some(value));
                            let repeats = level < deepest && same_prefix;
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
                        let field = self.row_fields.get(level - 1).map(|field| field.label.as_str()).unwrap_or_default();
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

/// The source rows a render groups: row ids from the substrate, or model rows.
enum SourceRows<'a> {
    /// Source row ids that survive the filters, ascending, read from the columnar substrate.
    Columns { storage: &'a ModelStorage, rows: Vec<usize> },
    /// The surviving source rows: borrowed from the model when its own filters answer, owned when
    /// the evaluator had to propagate a filter from a related table.
    Rows(Cow<'a, [Vec<Scalar>]>),
}

/// One hop of a related field's path: the value of `from_column` in the current table's row is the
/// key of the row of `to_table` whose `to_column` equals it.
///
/// A hop always runs from the `from` (many) side to the `to` (one) side of an active relationship,
/// the only direction one row can follow to reach a single related row.
#[derive(Clone, Debug)]
struct Hop {
    /// Index into [`DataModel::relationships`], and — because
    /// [`storage_mirrors_model`] checks it — into the substrate's relationship list.
    relationship: usize,
    /// The key column in the table the hop starts from.
    from_column: usize,
    /// The table the hop reaches.
    to_table: usize,
    /// The key column in the table the hop reaches.
    to_column: usize,
}

/// A grouping field (row, column or filter) bound to the pivot's source table.
struct BoundField {
    field: PivotField,
    /// The active relationships from the source table to the field's table, outermost first.
    /// Empty when the field is on the source table itself.
    hops: Vec<Hop>,
    /// The header and subtotal name: the bare column name on the source table, `Table[Column]` on
    /// a related table so two dimensions with the same column name stay distinguishable.
    label: String,
}

/// Binds every field reference in `fields` to `source`.
fn bind_fields(model: &DataModel, source: usize, source_name: &str, fields: &[String]) -> Result<Vec<BoundField>, PivotTableError> {
    pivot_fields(model, fields)?.into_iter().map(|field| bind_field(model, source, source_name, field)).collect()
}

/// Binds one field to the source table, resolving a field on a related table to its lookup path.
fn bind_field(model: &DataModel, source: usize, source_name: &str, field: PivotField) -> Result<BoundField, PivotTableError> {
    let label = match model.tables.get(field.table) {
        Some(table) if field.table != source => format!("{table_name}[{name}]", table_name = table.name, name = field.name),
        _ => field.name.clone(),
    };
    if field.table == source {
        return Ok(BoundField { field, hops: Vec::new(), label });
    }
    let table_name = model.tables.get(field.table).map(|table| table.name.clone()).unwrap_or_else(|| field.table.to_string());
    let chains = match lookup_chains(model, source, field.table, 2, false) {
        ChainSearch::Complete(chains) => chains,
        ChainSearch::Exhausted => return Err(ambiguous_field_path(&field.name, &table_name, source_name)),
    };
    let chain = match chains.as_slice() {
        [chain] => chain,
        [] => {
            // Distinguish the three ways a foreign field can be out of reach; the fix differs.
            let inactive = match lookup_chains(model, source, field.table, 1, true) {
                ChainSearch::Complete(chains) => chains,
                ChainSearch::Exhausted => return Err(ambiguous_field_path(&field.name, &table_name, source_name)),
            };
            if !inactive.is_empty() {
                return Err(PivotTableError::InactiveRelationship { field: field.name, table: table_name, source_table: source_name.to_string() });
            }
            if reachable(model, source, field.table) {
                return Err(PivotTableError::FanOutField { field: field.name, table: table_name, source_table: source_name.to_string() });
            }
            return Err(PivotTableError::ForeignField { field: field.name, table: source_name.to_string() });
        }
        chains => {
            return Err(PivotTableError::AmbiguousFieldPath {
                field: field.name,
                table: table_name,
                source_table: source_name.to_string(),
                paths: chains.len(),
            });
        }
    };
    let hops = chain
        .iter()
        .map(|index| {
            let relationship = model.relationships.get(*index)?;
            let from_table = model.table_index(&relationship.from_table)?;
            let to_table = model.table_index(&relationship.to_table)?;
            Some(Hop {
                relationship: *index,
                from_column: model.column_index(from_table, &relationship.from_column)?,
                to_table,
                to_column: model.column_index(to_table, &relationship.to_column)?,
            })
        })
        .collect::<Option<Vec<Hop>>>()
        // `DataModel::parse_formulas` validates every relationship, so a model whose relationships
        // do not resolve to columns was built without it; report it as the unusable field it is.
        .ok_or_else(|| PivotTableError::ForeignField { field: field.name.clone(), table: source_name.to_string() })?;
    Ok(BoundField { field, hops, label })
}

/// The error for a related field whose lookup path cannot be proven unique because the path search
/// exhausted [`MAX_PATH_SEARCH_STEPS`]. An exhausted search cannot count the chains, so report the
/// two-chain threshold the uniqueness probe was looking for rather than a misleading partial count.
fn ambiguous_field_path(field: &str, table: &str, source: &str) -> PivotTableError {
    PivotTableError::AmbiguousFieldPath { field: field.to_string(), table: table.to_string(), source_table: source.to_string(), paths: 2 }
}

/// The outcome of a bounded [`lookup_chains`] search.
enum ChainSearch {
    /// The search finished: every chain up to `limit`, or every chain that exists.
    Complete(Vec<Vec<usize>>),
    /// The search spent [`MAX_PATH_SEARCH_STEPS`] steps before it could prove how many chains
    /// exist, so the field's lookup path cannot be treated as unique.
    Exhausted,
}

/// Every simple chain of at most [`MAX_RELATIONSHIP_HOPS`] many-to-one lookups from `source` to
/// `target`, in model relationship order, stopping after `limit` chains.
///
/// Only lookup steps are followed (`from_table` to `to_table`, the direction one row can resolve to
/// a single related row) and a table is never entered twice on one chain, so a cyclic relationship
/// graph yields finitely many chains instead of looping: a cycle shows up as a second chain to the
/// same table, which [`bind_field`] rejects as ambiguous.
///
/// The search is bounded by [`MAX_PATH_SEARCH_STEPS`]: a dense graph whose path count explodes
/// returns [`ChainSearch::Exhausted`] instead of enumerating paths, and never a partial chain list.
fn lookup_chains(model: &DataModel, source: usize, target: usize, limit: usize, include_inactive: bool) -> ChainSearch {
    let mut steps = 0usize;
    let mut chains = Vec::new();
    let mut stack: Vec<(usize, Vec<usize>, Vec<usize>)> = vec![(source, Vec::new(), vec![source])];
    while let Some((table, chain, visited)) = stack.pop() {
        if !spend_step(&mut steps) {
            return ChainSearch::Exhausted;
        }
        if chain.len() >= MAX_RELATIONSHIP_HOPS {
            continue;
        }
        for (index, relationship) in model.relationships.iter().enumerate() {
            if !spend_step(&mut steps) {
                return ChainSearch::Exhausted;
            }
            if relationship.inactive && !include_inactive {
                continue;
            }
            let Some(from) = model.table_index(&relationship.from_table) else { continue };
            let Some(to) = model.table_index(&relationship.to_table) else { continue };
            if from != table || id_visited(&visited, to) {
                continue;
            }
            let mut next_chain = chain.clone();
            next_chain.push(index);
            if to == target {
                chains.push(next_chain);
                if chains.len() >= limit {
                    return ChainSearch::Complete(chains);
                }
                continue;
            }
            let mut next_visited = visited.clone();
            next_visited.push(to);
            stack.push((to, next_chain, next_visited));
        }
    }
    ChainSearch::Complete(chains)
}

/// Charges one step to `steps`, returning `false` once [`MAX_PATH_SEARCH_STEPS`] is spent.
fn spend_step(steps: &mut usize) -> bool {
    if *steps >= MAX_PATH_SEARCH_STEPS {
        return false;
    }
    *steps += 1;
    true
}

/// Whether `table` is in `visited`. Table ids are dense, but a `Vec` keeps the search allocation
/// small for the handful of tables a model has.
fn id_visited(visited: &[usize], table: usize) -> bool {
    visited.contains(&table)
}

/// Whether any chain of active relationships connects `source` and `target`, in either direction.
///
/// Used only to explain a failure: a table connected this way but not by a lookup chain is on the
/// one-to-many side of its path.
fn reachable(model: &DataModel, source: usize, target: usize) -> bool {
    let mut seen = vec![source];
    let mut pending = vec![source];
    while let Some(table) = pending.pop() {
        if table == target {
            return true;
        }
        for relationship in &model.relationships {
            if relationship.inactive {
                continue;
            }
            let Some(from) = model.table_index(&relationship.from_table) else { continue };
            let Some(to) = model.table_index(&relationship.to_table) else { continue };
            let next = if from == table {
                to
            } else if to == table {
                from
            } else {
                continue;
            };
            if !id_visited(&seen, next) && next != table {
                seen.push(next);
                pending.push(next);
            }
        }
    }
    false
}

/// Whether the substrate's relationship list is the model's list, in order.
///
/// [`ModelStorage::from_model`](crate::storage::ModelStorage::from_model) materializes every model
/// relationship in model order or fails as a whole, which is what lets a [`Hop`]'s model index read
/// the substrate. The endpoints are compared anyway: a storage that ever skips a relationship
/// degrades the pivot to the model-row path instead of following the wrong index.
fn storage_mirrors_model(storage: &ModelStorage, model: &DataModel) -> bool {
    let relationships = storage.relationships();
    relationships.len() == model.relationships.len()
        && relationships.iter().zip(&model.relationships).all(|(index, relationship)| {
            index.inactive == relationship.inactive
                && storage.tables().get(index.from_table).is_some_and(|table| table.name().eq_ignore_ascii_case(&relationship.from_table))
                && storage.tables().get(index.to_table).is_some_and(|table| table.name().eq_ignore_ascii_case(&relationship.to_table))
        })
}

/// The value of `field` for row `row` of `table`, read from columns and following the field's
/// relationship hops through the substrate's key indexes.
///
/// A hop whose key has no related row (an orphan key) yields BLANK, like `RELATED`. A key
/// duplicated on the one side resolves to the lowest row id, so a source row reaches at most one
/// related row and its measure value is never counted twice.
fn column_field_value(storage: &ModelStorage, table: usize, row: usize, field: &BoundField) -> Scalar {
    let mut current_table = table;
    let mut current_row = row;
    for hop in &field.hops {
        let key = storage.value(current_table, hop.from_column, current_row).unwrap_or(Scalar::Blank);
        let key = ScalarKey::from(&key);
        // A BLANK key never joins, even when the related table has its own BLANK key row: DAX
        // treats it as an orphan, and `model_field_value` groups it as one too.
        if key == ScalarKey::Blank {
            return Scalar::Blank;
        }
        let Some(next) = storage.relationships().get(hop.relationship).and_then(|relationship| relationship.rows_to(&key).first().copied()) else {
            return Scalar::Blank;
        };
        current_table = hop.to_table;
        current_row = next;
    }
    storage.value(current_table, field.field.column, current_row).unwrap_or(Scalar::Blank)
}

/// The value of `field` for `row` of the source table, read from the model rows.
///
/// The row-scan counterpart of [`column_field_value`]: the same hops, matched by key equality over
/// the related table's rows and resolving a duplicate key to the lowest row, so both paths group
/// identically.
fn model_field_value(model: &DataModel, lookups: &HopKeyLookups, row: &[Scalar], field: &BoundField) -> Scalar {
    let mut current = row;
    for hop in &field.hops {
        let key = ScalarKey::from(&current.get(hop.from_column).cloned().unwrap_or(Scalar::Blank));
        // A BLANK key is an orphan here too — a relationship never joins one, even when the
        // related table has a BLANK key row of its own — so both paths group the row the same way.
        if key == ScalarKey::Blank {
            return Scalar::Blank;
        }
        let matched = lookups
            .get(&(hop.to_table, hop.to_column))
            .and_then(|index| index.get(&key).copied())
            .and_then(|row_id| model.tables.get(hop.to_table).and_then(|table| table.rows.get(row_id)));
        let Some(next) = matched else { return Scalar::Blank };
        current = next;
    }
    current.get(field.field.column).cloned().unwrap_or(Scalar::Blank)
}

/// Key indexes for the model-row grouping path, one per distinct hop: `(to_table, to_column)` to
/// the lowest row id carrying each key.
type HopKeyLookups = BTreeMap<(usize, usize), BTreeMap<ScalarKey, usize>>;

/// Builds a [`HopKeyLookups`] entry for every hop of `fields`.
///
/// A duplicated key keeps the lowest row id, matching the `find` the lookup replaces, so a source
/// row still reaches at most one related row. A row too short to hold the key column has no key,
/// exactly as that `find` never matched it.
fn hop_key_lookups<'a>(model: &DataModel, fields: impl IntoIterator<Item = &'a BoundField>) -> HopKeyLookups {
    let mut lookups = HopKeyLookups::new();
    for field in fields {
        for hop in &field.hops {
            if lookups.contains_key(&(hop.to_table, hop.to_column)) {
                continue;
            }
            let mut index = BTreeMap::new();
            if let Some(table) = model.tables.get(hop.to_table) {
                for (row_id, row) in table.rows.iter().enumerate() {
                    if let Some(value) = row.get(hop.to_column) {
                        index.entry(ScalarKey::from(value)).or_insert(row_id);
                    }
                }
            }
            lookups.insert((hop.to_table, hop.to_column), index);
        }
    }
    lookups
}

/// Narrows `context` to the single group value `key` on `(table, column)`, intersecting with any
/// filter already set on the column.
///
/// A field used as both a row field and a column field receives one filter per axis, and a cell's
/// rows must satisfy both: the second axis therefore intersects instead of replacing. An
/// off-diagonal combination leaves the column's filter empty, and an empty filter matches no row,
/// so the cell is BLANK rather than the other axis's total.
pub(crate) fn narrow_to_key(context: &mut FilterContext, table: usize, column: usize, key: &ScalarKey) {
    // Filter keys fold text, so the group key is folded before it is compared with them.
    let key = crate::value::folded_key(key);
    match context.filters.get(&(table, column)) {
        // The intersection with `{key}` is empty: no row can satisfy both axes.
        Some(allowed) if !allowed.contains(&key) => context.set(table, column, std::iter::empty()),
        _ => context.set(table, column, [key]),
    }
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
    if value.is_blank() { BLANK_LABEL.to_string() } else { value.to_dax_text() }
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
    /// A row, column or filter field belongs to a table no relationship connects to the source.
    #[error("pivot field `{field}` is not on source table `{table}` and no active relationship connects either")]
    ForeignField { field: String, table: String },
    /// A related field can be reached by more than one chain of active relationships.
    #[error(
        "pivot field `{field}` on table `{table}` is reachable from `{source_table}` through {paths} relationship chains; a related pivot field needs exactly one"
    )]
    AmbiguousFieldPath { field: String, table: String, source_table: String, paths: usize },
    /// A related field is reachable only through an inactive relationship.
    #[error(
        "pivot field `{field}` on table `{table}` is reachable from `{source_table}` only through an inactive relationship; activate it in the model (`USERELATIONSHIP` is not supported in pivots)"
    )]
    InactiveRelationship { field: String, table: String, source_table: String },
    /// A related field is reachable only from the one side of a relationship.
    #[error(
        "pivot field `{field}` on table `{table}` is reachable from `{source_table}` only through the one-to-many side of a relationship; a pivot field must have a many-to-one lookup path"
    )]
    FanOutField { field: String, table: String, source_table: String },
    /// The rendered table would exceed the cell budget.
    #[error("pivot `{pivot}` would materialize {rows} rows x {columns} columns x {measures} values, over the limit of {limit} cells")]
    TooManyCells { pivot: String, rows: usize, columns: usize, measures: usize, limit: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelColumn, ModelTable, Relationship};

    /// A fact row whose foreign key is BLANK is an orphan: `column_field_value` never matches it,
    /// so the model-row path must not group it under the related table's own BLANK key row.
    #[test]
    fn a_blank_key_is_an_orphan_on_the_model_row_path() {
        let model = DataModel {
            tables: vec![
                ModelTable {
                    name: "Sales".into(),
                    columns: vec![
                        ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                        ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ],
                    rows: vec![vec![Scalar::Blank, Scalar::Integer(10)], vec![Scalar::Integer(1), Scalar::Integer(20)]],
                },
                ModelTable {
                    name: "Product".into(),
                    columns: vec![
                        ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                        ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                    ],
                    rows: vec![vec![Scalar::Blank, Scalar::String("Unknown".into())], vec![Scalar::Integer(1), Scalar::String("Widget".into())]],
                },
            ],
            relationships: vec![Relationship {
                from_table: "Sales".into(),
                from_column: "ProductKey".into(),
                to_table: "Product".into(),
                to_column: "ProductKey".into(),
                bidirectional: false,
                inactive: false,
            }],
            measures: Vec::new(),
        };
        let field = BoundField {
            field: PivotField { table: 1, column: 1, name: "Product[Name]".into() },
            hops: vec![Hop { relationship: 0, from_column: 0, to_table: 1, to_column: 0 }],
            label: "Product[Name]".into(),
        };
        let lookups = hop_key_lookups(&model, [&field]);
        let blank_key_row = model.tables[0].rows.first().cloned().unwrap_or_default();
        assert_eq!(model_field_value(&model, &lookups, &blank_key_row, &field), Scalar::Blank);
        let matched_row = model.tables[0].rows.get(1).cloned().unwrap_or_default();
        assert_eq!(model_field_value(&model, &lookups, &matched_row, &field), Scalar::String("Widget".into()));
    }
}
