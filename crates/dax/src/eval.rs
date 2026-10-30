//! A small, deterministic DAX evaluator over an in-memory [`DataModel`].
//!
//! Supported today: literals, operators, variables, column/measure/table references, the 22
//! aggregation functions, `CALCULATE`/`FILTER`/`ALL`/`VALUES`/`DISTINCT`/`SELECTEDVALUE`, common
//! logical/table/relationship/date/text/math/information functions, and basic pivot grouping.
//! Functions outside this subset return [`EvalError::not_implemented`]; the parser and catalog
//! still accept them.
//!
//! # Columnar evaluation substrate
//!
//! [`Evaluator::new`] materializes the model once into [`ModelStorage`]: typed columns with null
//! bitmaps and dictionaries plus [`RelationshipIndex`] key indexes. Evaluation then reads those
//! columns instead of rescanning [`crate::model::ModelTable::rows`]:
//!
//! * column filters are applied with [`TableStorage::filter_rows`] and intersected, and row sets
//!   stay row ids until a function actually needs row slices;
//! * filters propagate across relationships through [`RelationshipIndex::rows_from`] /
//!   [`RelationshipIndex::rows_to`], so key columns are never rescanned;
//! * `SUM`/`AVERAGE`/`PRODUCT`/`MIN`/`MAX`, `COUNT`/`COUNTA`/`COUNTBLANK`, `DISTINCTCOUNT` and the
//!   `*X` iterators whose expression is a plain column are answered from columns;
//! * `VALUES`/`DISTINCT` dedupe through column storage (and the string dictionary);
//! * `COUNTROWS`, `RELATED`, `RELATEDTABLE` and pivot grouping use row ids and the indexes.
//!
//! The substitution is only made when [`ModelStorage::is_faithful`] holds, i.e. the columnar copy
//! is lossless and rectangular. Models with ragged rows or promoted columns (mixed numeric
//! families, booleans mixed with other values, NaNs) keep the row-scan path, and so do the
//! functions that evaluate an expression per row (`FILTER`, `ADDCOLUMNS`, `TOPN`, measure
//! iterators) because they need a row context. [`Evaluator::storage_stats`] reports which path
//! served a call.

// Every family module shares the evaluator's vocabulary through `use super::*` (`Evaluator`,
// `Context`, `Scalar`, `TableValue`, `EvalError`, the value helpers), so each one reads as DAX
// functions rather than an import list.
#![allow(clippy::wildcard_imports)]

mod aggregation;
mod date_time;
mod filter;
mod financial;
mod information;
mod logical;
mod math_trig;
mod model_info;
mod other;
mod parent_child;
mod statistical;
mod support;
mod table_manipulation;
mod text;
mod time_intelligence;
mod visual;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::ast::{BinaryOp, Literal, UnaryOp};
use crate::binder::{BindError, Binder, BoundExpr};
use crate::model::DataModel;
use crate::storage::{ColumnKind, ModelStorage, StorageError, TableStorage};
use crate::value::{EvalError, Scalar, ScalarKey, TableColumn, TableValue};

use support::*;

/// A set of column-value filters. A column with several keys is an OR; different columns are
/// ANDed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterContext {
    pub filters: BTreeMap<(usize, usize), BTreeSet<ScalarKey>>,
    /// Relationship behaviour set by `USERELATIONSHIP`/`CROSSFILTER` inside CALCULATE:
    /// relationship index -> `(active, bidirectional)`.
    pub relationships: BTreeMap<usize, (bool, bool)>,
}

impl FilterContext {
    pub fn clear(&mut self) {
        self.filters.clear();
    }

    pub fn set(&mut self, table: usize, column: usize, values: impl IntoIterator<Item = ScalarKey>) {
        self.filters.insert((table, column), values.into_iter().collect());
    }

    pub fn remove_table(&mut self, table: usize) {
        self.filters.retain(|(t, _), _| *t != table);
    }

    fn matches(&self, table: usize, column: usize, value: &Scalar) -> bool {
        match self.filters.get(&(table, column)) {
            Some(values) => values.contains(&ScalarKey::from(value)),
            None => true,
        }
    }
}

/// One row of the evaluation row context.
///
/// `values` is the row's data, never a position in a derived table: iterators over `FILTER` or
/// `ADDCOLUMNS` results must still resolve the original values. `names` mirrors `values` so
/// unqualified `[Column]` references can find columns added by `ADDCOLUMNS`/`SELECTCOLUMNS`.
/// `model_columns` maps a value back to its model column in `lineage` (`None` for computed
/// columns), which lets a measure's context transition turn the row back into filters without
/// guessing from positions.
#[derive(Clone, Debug)]
pub(crate) struct RowFrame {
    pub(crate) lineage: Option<usize>,
    pub(crate) names: Vec<String>,
    pub(crate) values: Vec<Scalar>,
    pub(crate) model_columns: Vec<Option<usize>>,
}

impl RowFrame {
    fn get(&self, column: usize) -> Option<&Scalar> {
        self.model_columns.iter().position(|candidate| *candidate == Some(column)).and_then(|index| self.values.get(index))
    }

    fn get_name(&self, name: &str) -> Option<Scalar> {
        self.names.iter().position(|candidate| candidate.eq_ignore_ascii_case(name)).and_then(|index| self.values.get(index)).cloned()
    }
}

/// Instrumentation counters for the columnar evaluation substrate.
///
/// Every counter counts *calls served by* [`ModelStorage`], not bytes or timing, so a test can
/// assert that a formula really took the storage path. [`Evaluator::storage_stats`] snapshots
/// them. They are atomic, so an [`Evaluator`] stays `Sync` and can be shared; an evaluation itself
/// never reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageStats {
    /// Column aggregate fast paths (`SUM`, `AVERAGE`, `PRODUCT`, `MIN`, `MAX`, `COUNT`, `COUNTA`,
    /// `COUNTBLANK`, `DISTINCTCOUNT`, `APPROXIMATEDISTINCTCOUNT`).
    pub aggregates: u64,
    /// `*X` iterators answered from a column instead of a per-row frame.
    pub iterators: u64,
    /// `COUNTROWS` answered from a filtered row-id set.
    pub count_rows: u64,
    /// `RELATED` resolved through a [`RelationshipIndex`](crate::storage::RelationshipIndex).
    pub related: u64,
    /// `RELATEDTABLE` answered through a relationship index.
    pub related_table: u64,
    /// `VALUES`/`DISTINCT` served from column storage (string columns via the dictionary).
    pub distinct_values: u64,
    /// Pivot rows grouped from columns.
    pub pivot_group_rows: u64,
    /// Column filter scans executed by [`TableStorage::filter_rows`].
    pub filter_scans: u64,
    /// Relationship index probes (`rows_from`/`rows_to`).
    pub relationship_lookups: u64,
    /// Model rows served from columns instead of materialized model row vectors.
    pub rows_from_storage: u64,
    /// Calls that had to fall back to a model row scan because the storage is absent, not
    /// faithful, or cannot answer the call.
    pub row_scan_fallbacks: u64,
}

#[derive(Debug, Default)]
struct StorageCounters {
    aggregates: AtomicU64,
    iterators: AtomicU64,
    count_rows: AtomicU64,
    related: AtomicU64,
    related_table: AtomicU64,
    distinct_values: AtomicU64,
    pivot_group_rows: AtomicU64,
    filter_scans: AtomicU64,
    relationship_lookups: AtomicU64,
    rows_from_storage: AtomicU64,
    row_scan_fallbacks: AtomicU64,
}

impl StorageCounters {
    fn bump(&self, counter: fn(&StorageCounters) -> &AtomicU64) {
        counter(self).fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn add(&self, counter: fn(&StorageCounters) -> &AtomicU64, amount: usize) {
        let amount = u64::try_from(amount).unwrap_or(u64::MAX);
        counter(self).fetch_add(amount, AtomicOrdering::Relaxed);
    }

    fn snapshot(&self) -> StorageStats {
        StorageStats {
            aggregates: self.aggregates.load(AtomicOrdering::Relaxed),
            iterators: self.iterators.load(AtomicOrdering::Relaxed),
            count_rows: self.count_rows.load(AtomicOrdering::Relaxed),
            related: self.related.load(AtomicOrdering::Relaxed),
            related_table: self.related_table.load(AtomicOrdering::Relaxed),
            distinct_values: self.distinct_values.load(AtomicOrdering::Relaxed),
            pivot_group_rows: self.pivot_group_rows.load(AtomicOrdering::Relaxed),
            filter_scans: self.filter_scans.load(AtomicOrdering::Relaxed),
            relationship_lookups: self.relationship_lookups.load(AtomicOrdering::Relaxed),
            rows_from_storage: self.rows_from_storage.load(AtomicOrdering::Relaxed),
            row_scan_fallbacks: self.row_scan_fallbacks.load(AtomicOrdering::Relaxed),
        }
    }
}

/// An evaluator with pre-bound measures and one columnar copy of the model.
pub struct Evaluator<'a> {
    model: &'a DataModel,
    measures: Vec<BoundExpr>,
    /// The columnar substrate, when [`ModelStorage::from_model`] succeeded.
    storage: Option<ModelStorage>,
    /// Why no substrate exists. Kept instead of failing evaluation: a model with duplicate table
    /// names or a relationship naming a missing table still evaluates through the row scan, which
    /// is the behaviour this evaluator had before storage existed.
    storage_error: Option<StorageError>,
    counters: StorageCounters,
}

impl<'a> Evaluator<'a> {
    pub fn new(model: &'a DataModel) -> Result<Self, BindError> {
        let binder = Binder::new(model);
        // Reject cyclic measure dependencies here: `eval_measure` recurses through measure
        // references and would otherwise overflow the stack on every evaluation path.
        binder.dependency_graph()?;
        let mut measures = Vec::with_capacity(model.measures.len());
        for measure in &model.measures {
            let Some(expr) = &measure.expr else {
                return Err(BindError { message: format!("measure `{}` has no parsed formula", measure.name), span: crate::Span::empty() });
            };
            measures.push(binder.bind_expression(expr)?);
        }
        // One columnar copy for the evaluator's lifetime. A model that cannot be materialized is
        // not an evaluation error: the row-scan path stays available and the reason is reported by
        // `storage_error`.
        let (storage, storage_error) = match ModelStorage::from_model(model) {
            Ok(storage) => (Some(storage), None),
            Err(error) => (None, Some(error)),
        };
        Ok(Evaluator { model, measures, storage, storage_error, counters: StorageCounters::default() })
    }

    /// The columnar substrate, when the model could be materialized.
    pub fn storage(&self) -> Option<&ModelStorage> {
        self.storage.as_ref()
    }

    /// Why the columnar substrate is absent, when it is.
    ///
    /// Absent storage is deliberately not a construction error: a model with duplicate table or
    /// column names, or a relationship naming a missing endpoint, used to evaluate through the row
    /// scan and still does. The evaluator reports the reason here (and counts every fallback in
    /// [`StorageStats::row_scan_fallbacks`]) instead of failing every formula in such a model.
    pub fn storage_error(&self) -> Option<&StorageError> {
        self.storage_error.as_ref()
    }

    /// A snapshot of the storage instrumentation counters.
    pub fn storage_stats(&self) -> StorageStats {
        self.counters.snapshot()
    }

    /// The substrate when it is a transparent substitute for the model rows.
    ///
    /// Every caller that asks and does not get one takes the row-scan path, which is exactly the
    /// `row_scan_fallbacks` counter.
    fn substrate(&self) -> Option<&ModelStorage> {
        let storage = self.storage.as_ref().filter(|storage| storage.is_faithful());
        if storage.is_none() {
            self.bump(|counters| &counters.row_scan_fallbacks);
        }
        storage
    }

    fn bump(&self, counter: fn(&StorageCounters) -> &AtomicU64) {
        self.counters.bump(counter);
    }

    fn note_rows(&self, rows: usize) {
        self.counters.add(|counters| &counters.rows_from_storage, rows);
    }

    /// Records that pivot grouping read `rows` source rows from columns.
    pub(crate) fn note_pivot_rows(&self, rows: usize) {
        self.counters.add(|counters| &counters.pivot_group_rows, rows);
    }

    pub fn model(&self) -> &'a DataModel {
        self.model
    }

    /// Evaluates an expression with an empty filter context.
    pub fn evaluate(&self, expr: &BoundExpr) -> Result<Scalar, EvalError> {
        self.evaluate_with(expr, FilterContext::default())
    }

    pub fn evaluate_with(&self, expr: &BoundExpr, filters: FilterContext) -> Result<Scalar, EvalError> {
        let mut ctx = Context { filters, rows: Vec::new(), vars: Vec::new() };
        self.eval(expr, &mut ctx)
    }

    pub fn evaluate_table(&self, expr: &BoundExpr) -> Result<TableValue, EvalError> {
        let mut ctx = Context { filters: FilterContext::default(), rows: Vec::new(), vars: Vec::new() };
        self.eval_table(expr, &mut ctx)
    }

    pub fn evaluate_measure(&self, name: &str, filters: FilterContext) -> Result<Scalar, EvalError> {
        let index = self.model.measure_index(name).ok_or_else(|| EvalError::new(format!("unknown measure `{name}`")))?;
        let mut ctx = Context { filters, rows: Vec::new(), vars: Vec::new() };
        self.eval_measure(index, &mut ctx)
    }

    pub(crate) fn eval(&self, expr: &BoundExpr, ctx: &mut Context) -> Result<Scalar, EvalError> {
        match expr {
            BoundExpr::Literal(literal) => Ok(literal_value(literal)),
            BoundExpr::Column { table, column } => self.column_value(*table, *column, ctx),
            BoundExpr::UnqualifiedColumn { name } => self.unqualified_column(name, ctx),
            BoundExpr::Measure(index) => self.eval_measure(*index, ctx),
            BoundExpr::Table(_) => Err(EvalError::new("a table expression cannot be used as a scalar")),
            BoundExpr::Call { spec, args } => self.call(spec.name, args, ctx),
            BoundExpr::Unary { op, expr } => {
                let value = self.eval(expr, ctx)?;
                unary(*op, value)
            }
            BoundExpr::Binary { op, left, right } => {
                if *op == BinaryOp::In {
                    return self.eval_in(left, right, ctx);
                }
                let left = self.eval(left, ctx)?;
                let right = self.eval(right, ctx)?;
                binary(*op, left, right)
            }
            BoundExpr::TableConstructor(_) => Err(EvalError::new("a table constructor cannot be used as a scalar")),
            BoundExpr::Tuple(_) => Err(EvalError::new("a row constructor cannot be used as a scalar")),
            BoundExpr::Variable { index, .. } => {
                ctx.vars.get(*index).cloned().ok_or_else(|| EvalError::new(format!("variable index {index} is out of scope")))
            }
            BoundExpr::Keyword(name) => Ok(Scalar::String(name.clone())),
            BoundExpr::VarReturn { vars, body } => {
                let saved = ctx.vars.len();
                for var in vars {
                    let value = self.eval(&var.expr, ctx)?;
                    ctx.vars.push(value);
                }
                let result = self.eval(body, ctx);
                ctx.vars.truncate(saved);
                result
            }
        }
    }

    pub(crate) fn eval_measure(&self, index: usize, ctx: &mut Context) -> Result<Scalar, EvalError> {
        let expr = self.measures.get(index).ok_or_else(|| EvalError::new(format!("measure index {index} is out of range")))?;
        let mut filters = ctx.filters.clone();
        if let Some(frame) = ctx.rows.last()
            && let Some(table) = frame.lineage
            && self.model.tables.get(table).is_some()
        {
            // Context transition: the current row becomes an equivalent filter over the model
            // columns it carries. Computed columns (ADDCOLUMNS/SELECTCOLUMNS) have no model
            // column and are skipped.
            for (index, column) in frame.model_columns.iter().enumerate() {
                if let (Some(column), Some(value)) = (column, frame.values.get(index)) {
                    filters.set(table, *column, [ScalarKey::from(value)]);
                }
            }
        }
        let mut inner = Context { filters, rows: Vec::new(), vars: Vec::new() };
        self.eval(expr, &mut inner)
    }

    pub(crate) fn column_value(&self, table: usize, column: usize, ctx: &mut Context) -> Result<Scalar, EvalError> {
        if let Some(frame) = ctx.rows.iter().rev().find(|frame| frame.lineage == Some(table)) {
            return frame.get(column).cloned().ok_or_else(|| EvalError::new("row is out of range"));
        }
        if let Some(values) = ctx.filters.filters.get(&(table, column))
            && values.len() == 1
        {
            return values.iter().next().map(scalar_from_key).ok_or_else(|| EvalError::new("empty filter"));
        }
        Err(EvalError::new("a column reference needs a row or a single-value filter context"))
    }

    pub(crate) fn unqualified_column(&self, name: &str, ctx: &mut Context) -> Result<Scalar, EvalError> {
        for frame in ctx.rows.iter().rev() {
            if let Some(value) = frame.get_name(name) {
                return Ok(value);
            }
            if let Some(table) = frame.lineage
                && let Some(column) = self.model.column_index(table, name)
            {
                return frame.get(column).cloned().ok_or_else(|| EvalError::new("row is out of range"));
            }
        }
        let tables = self.model.tables_with_column(name);
        if tables.len() == 1 {
            let table = tables[0];
            if let Some(column) = self.model.column_index(table, name) {
                return self.column_value(table, column, ctx);
            }
        }
        Err(EvalError::new(format!("column `[{name}]` is ambiguous or out of row context")))
    }

    /// Rows of `table` that survive `filters`, in ascending row order, computed from storage.
    ///
    /// Column filters are applied with [`TableStorage::filter_rows`] and intersected; filters on
    /// related tables propagate through [`RelationshipIndex`](crate::storage::RelationshipIndex)
    /// key indexes instead of rescanning key columns. `None` means the row scan must answer
    /// instead: no substrate, a model that is not faithfully materialized, or a filter or
    /// relationship the storage cannot resolve.
    pub(crate) fn filtered_row_ids(&self, table: usize, filters: &FilterContext) -> Option<Vec<usize>> {
        let rows = self.filtered_row_ids_inner(table, filters, &mut BTreeSet::new());
        if rows.is_none() {
            self.bump(|counters| &counters.row_scan_fallbacks);
        }
        rows
    }

    /// [`Self::filtered_row_ids`] with the relationship propagation path.
    ///
    /// `visited` mirrors the recursion of the row-based filter: a chain that leads back to a table
    /// already being filtered contributes no rows, exactly like the scan path.
    fn filtered_row_ids_inner(&self, table: usize, filters: &FilterContext, visited: &mut BTreeSet<usize>) -> Option<Vec<usize>> {
        let storage = self.substrate()?;
        let table_storage = storage.table(table)?;
        if !visited.insert(table) {
            return Some(Vec::new());
        }
        let rows = self.collect_filtered_rows(storage, table_storage, table, filters, visited);
        visited.remove(&table);
        rows
    }

    /// Applies direct filters and relationship propagation to `table`. Split out of
    /// [`Self::filtered_row_ids_inner`] so the `visited` bookkeeping happens on every exit.
    fn collect_filtered_rows(
        &self,
        storage: &ModelStorage,
        table_storage: &TableStorage,
        table: usize,
        filters: &FilterContext,
        visited: &mut BTreeSet<usize>,
    ) -> Option<Vec<usize>> {
        let column_count = self.model.tables.get(table).map_or(0, |model_table| model_table.columns.len());
        let mut rows: Option<Vec<usize>> = None;
        for ((filter_table, column), keys) in &filters.filters {
            if *filter_table != table {
                continue;
            }
            // The scan path ignores a filter on a column the table does not have; storage cannot
            // express that, so hand the call back.
            if *column >= column_count {
                return None;
            }
            let wanted: Vec<ScalarKey> = keys.iter().cloned().collect();
            let matched = match table_storage.filter_rows(*column, &wanted) {
                Ok(matched) => matched,
                Err(_) => return None,
            };
            self.bump(|counters| &counters.filter_scans);
            rows = Some(match rows {
                None => matched,
                Some(current) => intersect_rows(current, &matched),
            });
        }
        let mut rows = rows.unwrap_or_else(|| (0..table_storage.row_count()).collect());

        for (index, relationship) in storage.relationships().iter().enumerate() {
            let (active, bidirectional) = filters.relationships.get(&index).copied().unwrap_or((!relationship.inactive, relationship.bidirectional));
            if !active {
                continue;
            }
            let (other, self_column, other_column) = if relationship.from_table == table {
                (relationship.to_table, relationship.from_column, relationship.to_column)
            } else if bidirectional && relationship.to_table == table {
                (relationship.from_table, relationship.to_column, relationship.from_column)
            } else {
                continue;
            };
            let mut probe = visited.clone();
            if visited.contains(&other) || !self.table_has_filter(other, filters, &mut probe) {
                continue;
            }
            let other_rows = self.filtered_row_ids_inner(other, filters, visited)?;
            self.bump(|counters| &counters.relationship_lookups);
            let keys: BTreeSet<ScalarKey> =
                other_rows.iter().filter_map(|row| storage.value(other, other_column, *row)).map(|value| ScalarKey::from(&value)).collect();
            let mut retained = Vec::with_capacity(rows.len());
            for row in rows {
                if let Some(value) = storage.value(table, self_column, row)
                    && keys.contains(&ScalarKey::from(&value))
                {
                    retained.push(row);
                }
            }
            rows = retained;
        }
        Some(rows)
    }

    pub(crate) fn table(&self, table: usize, filters: &FilterContext) -> TableValue {
        self.table_inner(table, filters, &mut BTreeSet::new())
    }

    fn table_inner(&self, table: usize, filters: &FilterContext, visited: &mut BTreeSet<usize>) -> TableValue {
        let Some(model_table) = self.model.tables.get(table) else {
            return TableValue::new(Vec::new(), Vec::new());
        };
        // Row ids keep the filtered set in storage hands; rows are only materialized here because
        // this method's contract is a row slice.
        if let Some(storage) = self.substrate()
            && let Some(ids) = self.filtered_row_ids_inner(table, filters, visited)
        {
            let rows: Vec<Vec<Scalar>> = ids
                .iter()
                .map(|row| (0..model_table.columns.len()).map(|column| storage.value(table, column, *row).unwrap_or(Scalar::Blank)).collect())
                .collect();
            self.note_rows(ids.len());
            return TableValue::new(model_table.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect(), rows)
                .with_lineage(table);
        }
        // A relationship chain can lead back to a table already being filtered (bidirectional
        // pairs, or a cycle of single-direction relationships). Stop at the cycle: the direct
        // filters on the other table still apply to it, but re-entering here adds nothing.
        if !visited.insert(table) {
            return TableValue::new(model_table.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect(), Vec::new())
                .with_lineage(table);
        }
        let mut rows: Vec<Vec<Scalar>> = model_table
            .rows
            .iter()
            .filter(|row| {
                model_table.columns.iter().enumerate().all(|(column, _)| row.get(column).is_none_or(|value| filters.matches(table, column, value)))
            })
            .cloned()
            .collect();

        // Propagate filters across active relationships: a filtered related table restricts this
        // table to rows whose key appears in the related table. Only tables that are actually
        // filtered can restrict this one, and a table already on the current propagation path is
        // skipped.
        for (index, relationship) in self.model.relationships.iter().enumerate() {
            let (Some(from), Some(to)) = (self.model.table_index(&relationship.from_table), self.model.table_index(&relationship.to_table)) else {
                continue;
            };
            let (active, bidirectional) = filters.relationships.get(&index).copied().unwrap_or((!relationship.inactive, relationship.bidirectional));
            if !active {
                continue;
            }
            let (other, self_column, other_column) = if from == table {
                (to, &relationship.from_column, &relationship.to_column)
            } else if bidirectional && to == table {
                (from, &relationship.to_column, &relationship.from_column)
            } else {
                continue;
            };
            // Recurse when the related table is filtered directly or through a further
            // relationship; the visited set keeps cycles bounded. Seed the reachability probe
            // with the current path so a bidirectional link cannot treat this table's own filter
            // as a filter on the related table (that would drop orphan keys).
            let mut probe = visited.clone();
            if visited.contains(&other) || !self.table_has_filter(other, filters, &mut probe) {
                continue;
            }
            let (Some(self_col), Some(other_col)) = (self.model.column_index(table, self_column), self.model.column_index(other, other_column))
            else {
                continue;
            };
            let keys: BTreeSet<ScalarKey> =
                self.table_inner(other, filters, visited).rows.iter().filter_map(|row| row.get(other_col)).map(ScalarKey::from).collect();
            rows.retain(|row| row.get(self_col).is_some_and(|value| keys.contains(&ScalarKey::from(value))));
        }

        visited.remove(&table);
        TableValue::new(model_table.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect(), rows).with_lineage(table)
    }

    /// Whether `table`, or any table reachable from it over active relationships, has a filter.
    /// This lets filters propagate across several hops (`Sales -> Products -> Category`).
    fn table_has_filter(&self, table: usize, filters: &FilterContext, visited: &mut BTreeSet<usize>) -> bool {
        if !visited.insert(table) {
            return false;
        }
        if filters.filters.keys().any(|(filtered, _)| *filtered == table) {
            return true;
        }
        for (index, relationship) in self.model.relationships.iter().enumerate() {
            let (active, bidirectional) = filters.relationships.get(&index).copied().unwrap_or((!relationship.inactive, relationship.bidirectional));
            if !active {
                continue;
            }
            let (Some(from), Some(to)) = (self.model.table_index(&relationship.from_table), self.model.table_index(&relationship.to_table)) else {
                continue;
            };
            let next = if from == table {
                Some(to)
            } else if bidirectional && to == table {
                Some(from)
            } else {
                None
            };
            if let Some(next) = next
                && next != table
                && self.table_has_filter(next, filters, visited)
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn eval_table(&self, expr: &BoundExpr, ctx: &mut Context) -> Result<TableValue, EvalError> {
        match expr {
            BoundExpr::Table(table) => Ok(self.table(*table, &ctx.filters)),
            BoundExpr::Call { spec, args } => self.call_table(spec.name, args, ctx),
            BoundExpr::TableConstructor(items) => {
                let rows = items.iter().map(|item| Ok(vec![self.eval(item, ctx)?])).collect::<Result<Vec<_>, EvalError>>()?;
                Ok(TableValue::new(vec![TableColumn { name: "Value".into() }], rows))
            }
            BoundExpr::Tuple(_) => Err(EvalError::new("a row constructor is not a table")),
            _ => Err(EvalError::new("expected a table expression")),
        }
    }

    fn eval_in(&self, left: &BoundExpr, right: &BoundExpr, ctx: &mut Context) -> Result<Scalar, EvalError> {
        let left = match left {
            BoundExpr::Tuple(items) => items.iter().map(|item| self.eval(item, ctx)).collect::<Result<Vec<_>, _>>()?,
            _ => vec![self.eval(left, ctx)?],
        };
        let table = self.eval_table(right, ctx)?;
        let found = table.rows.iter().any(|row| row.len() == left.len() && row.iter().zip(&left).all(|(a, b)| a.strict_eq(b)));
        Ok(Scalar::Boolean(found))
    }

    #[allow(clippy::cognitive_complexity)]
    // The dispatcher is a flat list of function names: each arm is one documented DAX function.
    fn call(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
        match name.to_ascii_uppercase().as_str() {
            "TRUE" => return Ok(Scalar::Boolean(true)),
            "FALSE" => return Ok(Scalar::Boolean(false)),
            "BLANK" => return Ok(Scalar::Blank),
            "IF" => {
                let condition = self.eval(args.first().ok_or_else(|| EvalError::new("IF needs a condition"))?, ctx)?;
                // Excel's DAX engine rejects a condition it cannot read as a logical value instead
                // of silently taking the false branch.
                let test =
                    condition.to_bool().ok_or_else(|| EvalError::new(format!("IF argument 1 must be Boolean, found {}", condition.type_name())))?;
                let branch = if test { args.get(1) } else { args.get(2) };
                return match branch {
                    Some(expr) => self.eval(expr, ctx),
                    None => Ok(Scalar::Blank),
                };
            }
            "IFERROR" => {
                return match self.eval(args.first().ok_or_else(|| EvalError::new("IFERROR needs a value"))?, ctx) {
                    Ok(value) => Ok(value),
                    Err(_) => self.eval(args.get(1).ok_or_else(|| EvalError::new("IFERROR needs an alternative"))?, ctx),
                };
            }
            "ISERROR" => {
                let arg = args.first().ok_or_else(|| EvalError::new("ISERROR needs an argument"))?;
                return Ok(Scalar::Boolean(self.eval(arg, ctx).is_err()));
            }
            "SWITCH" => {
                let value = self.eval(args.first().ok_or_else(|| EvalError::new("SWITCH needs an expression"))?, ctx)?;
                let mut index = 1;
                while index + 1 < args.len() {
                    let candidate = self.eval(&args[index], ctx)?;
                    if value.compare(&candidate) == Some(Ordering::Equal) {
                        return self.eval(&args[index + 1], ctx);
                    }
                    index += 2;
                }
                return if index < args.len() { self.eval(&args[index], ctx) } else { Ok(Scalar::Blank) };
            }
            "COALESCE" => {
                for arg in args {
                    let value = self.eval(arg, ctx)?;
                    if !value.is_blank() {
                        return Ok(value);
                    }
                }
                return Ok(Scalar::Blank);
            }
            "AND" => {
                let mut result = true;
                for arg in args {
                    result &= self.eval(arg, ctx)?.to_bool().unwrap_or(false);
                }
                return Ok(Scalar::Boolean(result));
            }
            "OR" => {
                let mut result = false;
                for arg in args {
                    result |= self.eval(arg, ctx)?.to_bool().unwrap_or(false);
                }
                return Ok(Scalar::Boolean(result));
            }
            "NOT" => {
                let value = self.eval(args.first().ok_or_else(|| EvalError::new("NOT needs an argument"))?, ctx)?;
                return Ok(Scalar::Boolean(!value.to_bool().unwrap_or(false)));
            }
            "CALCULATE" => {
                let mut filters = ctx.filters.clone();
                for arg in &args[1..] {
                    self.apply_filter(arg, &mut filters, ctx)?;
                }
                let mut inner = Context { filters, rows: ctx.rows.clone(), vars: ctx.vars.clone() };
                return self.eval(&args[0], &mut inner);
            }
            "CALCULATETABLE" => return Err(EvalError::new("CALCULATETABLE returns a table and cannot be used as a scalar")),
            "FILTER" => {
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("FILTER needs a table"))?, ctx)?;
                let predicate = args.get(1).ok_or_else(|| EvalError::new("FILTER needs a predicate"))?;
                let mut rows = Vec::new();
                for row in &table.rows {
                    ctx.rows.push(frame_for(self, &table, row));
                    let keep =
                        self.eval(predicate, ctx).and_then(|value| value.to_bool().ok_or_else(|| EvalError::new("FILTER predicate is not Boolean")));
                    ctx.rows.pop();
                    if keep? {
                        rows.push(row.clone());
                    }
                }
                return Ok(Scalar::Blank); // table functions are handled by eval_table
            }
            "ALL" | "ALLEXCEPT" | "ALLNOBLANKROW" | "ALLSELECTED" | "REMOVEFILTERS" | "KEEPFILTERS" => {
                return Ok(Scalar::Blank);
            }
            "VALUES" | "DISTINCT" => {
                let _ = self.eval_table(args.first().ok_or_else(|| EvalError::new("VALUES needs a table"))?, ctx)?;
                return Ok(Scalar::Blank);
            }
            "SELECTEDVALUE" => {
                let value = self.eval(args.first().ok_or_else(|| EvalError::new("SELECTEDVALUE needs a column"))?, ctx);
                return match value {
                    Ok(value) if !value.is_blank() => Ok(value),
                    _ => match args.get(1) {
                        Some(alternative) => self.eval(alternative, ctx),
                        None => Ok(Scalar::Blank),
                    },
                };
            }
            "RELATED" => return self.related(args.first(), ctx),
            "RELATEDTABLE" => return Err(EvalError::new("RELATEDTABLE returns a table and cannot be used as a scalar")),
            "USERELATIONSHIP" | "CROSSFILTER" => {
                return Err(EvalError::new(format!("{name} is only valid as a CALCULATE modifier")));
            }
            "SUM" | "AVERAGE" | "COUNT" | "COUNTA" | "COUNTBLANK" | "MIN" | "MAX" | "PRODUCT" | "DISTINCTCOUNT" | "APPROXIMATEDISTINCTCOUNT" => {
                return self.aggregate(name, args, ctx);
            }
            "SUMX" | "AVERAGEX" | "COUNTX" | "COUNTAX" | "MINX" | "MAXX" | "PRODUCTX" => return self.aggregate_x(name, args, ctx),
            "COUNTROWS" => {
                let Some(arg) = args.first() else {
                    return Err(EvalError::new("COUNTROWS() without a home table is not supported"));
                };
                // A plain model table needs no materialized rows: the filtered row-id set already
                // has the count.
                if let BoundExpr::Table(table) = arg
                    && let Some(rows) = self.filtered_row_ids(*table, &ctx.filters)
                {
                    self.bump(|counters| &counters.count_rows);
                    self.note_rows(rows.len());
                    return Ok(if rows.is_empty() { Scalar::Blank } else { Scalar::Integer(rows.len() as i64) });
                }
                let table = self.eval_table(arg, ctx)?;
                return Ok(if table.rows.is_empty() { Scalar::Blank } else { Scalar::Integer(table.rows.len() as i64) });
            }
            _ => {}
        }

        if let Some(result) = self.call_categories(name, args, ctx) {
            return result;
        }
        let values = args.iter().map(|arg| self.eval(arg, ctx)).collect::<Result<Vec<_>, _>>()?;
        scalar_function(name, &values)
    }

    /// Runs the family modules for functions the core switch does not handle.
    fn call_categories(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
        for dispatch in [
            financial::call,
            statistical::call,
            time_intelligence::call,
            information::call,
            model_info::call,
            filter::call,
            table_manipulation::call,
            text::call,
            math_trig::call,
            logical::call,
            date_time::call,
            other::call,
            parent_child::call,
            aggregation::call,
            visual::call,
        ] {
            if let Some(result) = dispatch(self, name, args, ctx) {
                return Some(result);
            }
        }
        None
    }

    fn call_table(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
        match name.to_ascii_uppercase().as_str() {
            "FILTER" => {
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("FILTER needs a table"))?, ctx)?;
                let predicate = args.get(1).ok_or_else(|| EvalError::new("FILTER needs a predicate"))?;
                let mut rows = Vec::new();
                for row in &table.rows {
                    ctx.rows.push(frame_for(self, &table, row));
                    let keep =
                        self.eval(predicate, ctx).and_then(|value| value.to_bool().ok_or_else(|| EvalError::new("FILTER predicate is not Boolean")));
                    ctx.rows.pop();
                    if keep? {
                        rows.push(row.clone());
                    }
                }
                // Keep the source lineage: CALCULATE table filters and downstream iterators need
                // to know which model table these rows came from.
                Ok(TableValue::new(table.columns, rows).with_lineage_if(table.lineage))
            }
            "CALCULATETABLE" => {
                let mut filters = ctx.filters.clone();
                for arg in &args[1..] {
                    self.apply_filter(arg, &mut filters, ctx)?;
                }
                let mut inner = Context { filters, rows: ctx.rows.clone(), vars: ctx.vars.clone() };
                self.eval_table(&args[0], &mut inner)
            }
            "RELATEDTABLE" => {
                let argument = args.first().ok_or_else(|| EvalError::new("RELATEDTABLE needs a table"))?;
                // A plain model table over a row context needs no materialized source table: the
                // index gives the matching row ids directly.
                if let BoundExpr::Table(lineage) = argument
                    && let Some(frame) = ctx.rows.last()
                    && let Some(frame_table) = frame.lineage
                    && let Some(rows) = self.related_table_via_index(*lineage, frame_table, frame, &ctx.filters)
                {
                    let columns = self
                        .model
                        .tables
                        .get(*lineage)
                        .map(|table| table.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect())
                        .unwrap_or_default();
                    return Ok(TableValue::new(columns, rows).with_lineage(*lineage));
                }
                let table = self.eval_table(argument, ctx)?;
                let Some(frame) = ctx.rows.last() else {
                    return Err(EvalError::new("RELATEDTABLE needs a row context"));
                };
                let Some(frame_table) = frame.lineage else {
                    return Err(EvalError::new("RELATEDTABLE needs a row context over a model table"));
                };
                let Some(lineage) = table.lineage else {
                    return Err(EvalError::new("RELATEDTABLE needs a model table"));
                };
                for relationship in &self.model.relationships {
                    if relationship.inactive {
                        continue;
                    }
                    let from = self.model.table_index(&relationship.from_table);
                    let to = self.model.table_index(&relationship.to_table);
                    // Many-to-one (`Sales` row -> `Products`) and one-to-many (`Products` row ->
                    // `Sales`) both use RELATEDTABLE, so either end can be the current frame.
                    let (key_column, row_column) = if from == Some(frame_table) && to == Some(lineage) {
                        (&relationship.from_column, &relationship.to_column)
                    } else if to == Some(frame_table) && from == Some(lineage) {
                        (&relationship.to_column, &relationship.from_column)
                    } else {
                        continue;
                    };
                    let key_col = self.model.column_index(frame_table, key_column).ok_or_else(|| EvalError::new("bad relationship"))?;
                    let row_col = self.model.column_index(lineage, row_column).ok_or_else(|| EvalError::new("bad relationship"))?;
                    let key = frame.get(key_col).cloned().ok_or_else(|| EvalError::new("row is out of range"))?;
                    let rows = table.rows.iter().filter(|row| row.get(row_col).is_some_and(|value| value.strict_eq(&key))).cloned().collect();
                    return Ok(TableValue::new(table.columns, rows).with_lineage(lineage));
                }
                Err(EvalError::new("no active relationship for RELATEDTABLE"))
            }
            "ALL" | "ALLNOBLANKROW" | "ALLSELECTED" => {
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("ALL needs a table"))?, ctx)?;
                Ok(table)
            }
            "ALLEXCEPT" => {
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("ALLEXCEPT needs a table"))?, ctx)?;
                Ok(table)
            }
            "VALUES" | "DISTINCT" => {
                let arg = args.first().ok_or_else(|| EvalError::new("VALUES needs a table or column"))?;
                if let BoundExpr::Column { table, column } = arg {
                    if let Some(values) = self.storage_distinct_values(*table, *column, &ctx.filters) {
                        let name = self
                            .model
                            .tables
                            .get(*table)
                            .and_then(|table| table.columns.get(*column))
                            .map(|column| column.name.clone())
                            .unwrap_or_default();
                        let rows = values.into_iter().map(|value| vec![value]).collect::<Vec<_>>();
                        return Ok(TableValue::new(vec![TableColumn { name }], rows).with_lineage(*table));
                    }
                    let source = self.table(*table, &ctx.filters);
                    let name = source.columns.get(*column).map(|column| column.name.clone()).unwrap_or_default();
                    let values: Vec<Scalar> = source.column(*column);
                    let rows = values.into_iter().map(|value| vec![value]).collect::<Vec<_>>();
                    let distinct = TableValue::new(vec![TableColumn { name }], rows).distinct_rows();
                    return Ok(distinct.with_lineage(*table));
                }
                let table = self.eval_table(arg, ctx)?;
                Ok(table.distinct_rows())
            }
            "ROW" => {
                let mut columns = Vec::new();
                let mut row = Vec::new();
                for pair in args.chunks(2) {
                    let name = match pair.first() {
                        Some(BoundExpr::Literal(Literal::String(name))) => name.clone(),
                        _ => return Err(EvalError::new("ROW needs string column names")),
                    };
                    let value = pair.get(1).ok_or_else(|| EvalError::new("ROW needs a value for each name"))?;
                    columns.push(TableColumn { name });
                    row.push(self.eval(value, ctx)?);
                }
                Ok(TableValue::new(columns, vec![row]))
            }
            "ADDCOLUMNS" | "SELECTCOLUMNS" => {
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("table function needs a table"))?, ctx)?;
                let mut columns = table.columns.clone();
                let mut expressions = Vec::new();
                for pair in args[1..].chunks(2) {
                    let name = match pair.first() {
                        Some(BoundExpr::Literal(Literal::String(name))) => name.clone(),
                        _ => return Err(EvalError::new("column name must be a string literal")),
                    };
                    let expr = pair.get(1).ok_or_else(|| EvalError::new("column needs an expression"))?;
                    columns.push(TableColumn { name });
                    expressions.push(expr);
                }
                let mut rows = Vec::new();
                for row in &table.rows {
                    ctx.rows.push(frame_for(self, &table, row));
                    let mut out = row.clone();
                    for expr in &expressions {
                        out.push(self.eval(expr, ctx)?);
                    }
                    ctx.rows.pop();
                    rows.push(out);
                }
                Ok(TableValue::new(columns, rows).with_lineage_if(table.lineage))
            }
            "UNION" => {
                let tables = args.iter().map(|arg| self.eval_table(arg, ctx)).collect::<Result<Vec<_>, _>>()?;
                let mut tables = tables.into_iter();
                let Some(first) = tables.next() else { return Ok(TableValue::new(Vec::new(), Vec::new())) };
                let mut rows = first.rows.clone();
                for table in tables {
                    rows.extend(table.rows);
                }
                Ok(TableValue::new(first.columns, rows))
            }
            "INTERSECT" | "EXCEPT" => {
                let left = self.eval_table(args.first().ok_or_else(|| EvalError::new("needs two tables"))?, ctx)?;
                let right = self.eval_table(args.get(1).ok_or_else(|| EvalError::new("needs two tables"))?, ctx)?;
                let right_keys: BTreeSet<Vec<ScalarKey>> = right.rows.iter().map(|row| row.iter().map(ScalarKey::from).collect()).collect();
                let keep = name.eq_ignore_ascii_case("INTERSECT");
                let rows = left
                    .rows
                    .into_iter()
                    .filter(|row| {
                        let key: Vec<ScalarKey> = row.iter().map(ScalarKey::from).collect();
                        right_keys.contains(&key) == keep
                    })
                    .collect();
                Ok(TableValue::new(left.columns, rows))
            }
            "CROSSJOIN" => {
                let left = self.eval_table(args.first().ok_or_else(|| EvalError::new("CROSSJOIN needs tables"))?, ctx)?;
                let right = self.eval_table(args.get(1).ok_or_else(|| EvalError::new("CROSSJOIN needs tables"))?, ctx)?;
                let mut columns = left.columns.clone();
                columns.extend(right.columns.clone());
                let mut rows = Vec::new();
                for l in &left.rows {
                    for r in &right.rows {
                        let mut row = l.clone();
                        row.extend(r.clone());
                        rows.push(row);
                    }
                }
                Ok(TableValue::new(columns, rows))
            }
            "TOPN" => {
                let n = self.eval(args.first().ok_or_else(|| EvalError::new("TOPN needs N"))?, ctx)?.to_number().unwrap_or(0.0) as usize;
                let table = self.eval_table(args.get(1).ok_or_else(|| EvalError::new("TOPN needs a table"))?, ctx)?;
                let order = args.get(2).ok_or_else(|| EvalError::new("TOPN needs an order expression"))?;
                let mut scored = Vec::with_capacity(table.rows.len());
                for row in &table.rows {
                    ctx.rows.push(frame_for(self, &table, row));
                    let score = self.eval(order, ctx)?.to_number().unwrap_or(0.0);
                    ctx.rows.pop();
                    scored.push((score, row.clone()));
                }
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
                Ok(TableValue::new(table.columns, scored.into_iter().take(n).map(|(_, row)| row).collect()).with_lineage_if(table.lineage))
            }
            "DATATABLE" => {
                // The final argument is a table constructor of rows; earlier pairs describe columns.
                let Some(last) = args.last() else { return Err(EvalError::new("DATATABLE needs rows")) };
                let BoundExpr::TableConstructor(rows) = last else {
                    return Err(EvalError::new("DATATABLE rows must be a table constructor"));
                };
                let mut columns = Vec::new();
                for pair in args[..args.len() - 1].chunks(2) {
                    if let Some(BoundExpr::Literal(Literal::String(name))) = pair.first() {
                        columns.push(TableColumn { name: name.clone() });
                    }
                }
                let mut out = Vec::new();
                for row in rows {
                    let BoundExpr::Tuple(values) = row else { continue };
                    out.push(values.iter().map(|value| self.eval(value, ctx)).collect::<Result<Vec<_>, _>>()?);
                }
                Ok(TableValue::new(columns, out))
            }
            "TREATAS" => self.eval_table(args.first().ok_or_else(|| EvalError::new("TREATAS needs a table"))?, ctx),
            _ => self.call_categories_table(name, args, ctx).unwrap_or_else(|| Err(EvalError::not_implemented(name))),
        }
    }

    fn call_categories_table(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
        for dispatch in [
            financial::call_table,
            statistical::call_table,
            time_intelligence::call_table,
            information::call_table,
            model_info::call_table,
            filter::call_table,
            table_manipulation::call_table,
            text::call_table,
            math_trig::call_table,
            logical::call_table,
            date_time::call_table,
            other::call_table,
            parent_child::call_table,
            aggregation::call_table,
            visual::call_table,
        ] {
            if let Some(result) = dispatch(self, name, args, ctx) {
                return Some(result);
            }
        }
        None
    }

    fn aggregate(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
        let arg = args.first().ok_or_else(|| EvalError::new("aggregation needs a column"))?;
        if let Some(column) = self.resolve_column(arg)
            && let Some(result) = self.storage_aggregate(name, column, &ctx.filters)
        {
            return result;
        }
        let values = self.column_values(arg, ctx)?;
        aggregate_values(name, &values)
    }

    fn aggregate_x(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
        if let Some(result) = self.storage_aggregate_x(name, args, &ctx.filters) {
            return result;
        }
        let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("iterator needs a table"))?, ctx)?;
        let expression = args.get(1).ok_or_else(|| EvalError::new("iterator needs an expression"))?;
        let mut values = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            ctx.rows.push(frame_for(self, &table, row));
            let value = self.eval(expression, ctx);
            ctx.rows.pop();
            values.push(value?);
        }
        aggregate_values(&name[..name.len() - 1], &values)
    }

    /// `*X` fast path: the iterator's table is a model table and its expression is a plain column
    /// of that table, so the aggregate runs over the column rather than evaluating a row frame per
    /// row. `None` keeps the iterator path for every other expression.
    fn storage_aggregate_x(&self, name: &str, args: &[BoundExpr], filters: &FilterContext) -> Option<Result<Scalar, EvalError>> {
        let BoundExpr::Table(table) = args.first()? else {
            return None;
        };
        let column = match args.get(1)? {
            BoundExpr::Column { table: expression_table, column } if expression_table == table => *column,
            BoundExpr::UnqualifiedColumn { name } => self.model.column_index(*table, name)?,
            _ => return None,
        };
        let aggregate = name.to_ascii_uppercase();
        let result = self.storage_aggregate(aggregate.strip_suffix('X')?, (*table, column), filters)?;
        self.bump(|counters| &counters.iterators);
        Some(result)
    }

    /// Column aggregate fast path. `None` means "use the row path", which also reproduces its
    /// errors (ambiguous references, a column that is not numeric).
    fn storage_aggregate(&self, name: &str, (table, column): (usize, usize), filters: &FilterContext) -> Option<Result<Scalar, EvalError>> {
        let storage = self.substrate()?;
        let table_storage = storage.table(table)?;
        let kind = table_storage.column(column)?.kind();
        let rows = self.filtered_row_ids(table, filters)?;
        let aggregate = name.to_ascii_uppercase();
        let scalar = match aggregate.as_str() {
            "SUM" | "AVERAGE" | "PRODUCT" | "MIN" | "MAX" => {
                // These read numbers only: booleans and text contribute nothing, so a logical or
                // promoted-to-text column keeps the row path where that rule lives.
                if !matches!(kind, ColumnKind::Integer | ColumnKind::Currency | ColumnKind::Decimal | ColumnKind::DateTime) {
                    return None;
                }
                let numbers = table_storage.numeric_values(column, &rows).ok()?;
                match aggregate.as_str() {
                    "MIN" => numbers.iter().copied().reduce(f64::min).map(Scalar::Decimal).unwrap_or(Scalar::Blank),
                    "MAX" => numbers.iter().copied().reduce(f64::max).map(Scalar::Decimal).unwrap_or(Scalar::Blank),
                    "SUM" if numbers.is_empty() => Scalar::Blank,
                    "SUM" => Scalar::Decimal(numbers.iter().sum()),
                    "PRODUCT" if numbers.is_empty() => Scalar::Blank,
                    "PRODUCT" => Scalar::Decimal(numbers.iter().product()),
                    _ if numbers.is_empty() => Scalar::Blank,
                    _ => Scalar::Decimal(numbers.iter().sum::<f64>() / numbers.len() as f64),
                }
            }
            "COUNT" | "COUNTA" => {
                // `COUNT` ignores booleans; a logical column keeps the row path so that rule is
                // applied in one place only.
                if aggregate == "COUNT" && kind == ColumnKind::Boolean {
                    return None;
                }
                let count = match table_storage.count(column, &rows).ok()? {
                    Scalar::Integer(count) => count,
                    _ => return None,
                };
                if count == 0 { Scalar::Blank } else { Scalar::Integer(count) }
            }
            "COUNTBLANK" => table_storage.count_blank(column, &rows).ok()?,
            "DISTINCTCOUNT" | "APPROXIMATEDISTINCTCOUNT" => {
                if rows.is_empty() {
                    Scalar::Blank
                } else {
                    let count = match table_storage.distinct_count(column, &rows).ok()? {
                        Scalar::Integer(count) => count,
                        _ => return None,
                    };
                    Scalar::Integer(count)
                }
            }
            _ => return None,
        };
        self.bump(|counters| &counters.aggregates);
        self.note_rows(rows.len());
        Some(Ok(scalar))
    }

    /// The `(table, column)` of a plain column reference, or `None` for every other expression.
    fn resolve_column(&self, expr: &BoundExpr) -> Option<(usize, usize)> {
        match expr {
            BoundExpr::Column { table, column } => Some((*table, *column)),
            BoundExpr::UnqualifiedColumn { name } => {
                let tables = self.model.tables_with_column(name);
                if tables.len() != 1 {
                    return None;
                }
                let table = tables[0];
                self.model.column_index(table, name).map(|column| (table, column))
            }
            _ => None,
        }
    }

    /// Values of one column over the filtered row set, read from storage.
    fn storage_column(&self, table: usize, column: usize, filters: &FilterContext) -> Option<Vec<Scalar>> {
        let storage = self.substrate()?;
        let rows = self.filtered_row_ids(table, filters)?;
        let values = rows.iter().map(|row| storage.value(table, column, *row)).collect::<Option<Vec<Scalar>>>()?;
        self.note_rows(rows.len());
        Some(values)
    }

    /// Distinct values of one column over the filtered row set, in first-occurrence row order.
    fn storage_distinct_values(&self, table: usize, column: usize, filters: &FilterContext) -> Option<Vec<Scalar>> {
        let storage = self.substrate()?;
        let table_storage = storage.table(table)?;
        let rows = self.filtered_row_ids(table, filters)?;
        let values = table_storage.distinct_values(column, &rows).ok()?;
        self.bump(|counters| &counters.distinct_values);
        self.note_rows(rows.len());
        Some(values)
    }

    /// Finite numeric observations of a column over the filtered row set.
    ///
    /// The statistical aggregates (`STDEV`, `VAR`, `MEDIAN`, `PERCENTILE`, `GEOMEAN`) need the
    /// numbers, not a scalar list; this reads them from columns. `None` keeps those functions on
    /// their column path, which also reports its errors.
    pub(crate) fn storage_numeric_values(&self, expr: &BoundExpr, ctx: &Context) -> Option<Vec<f64>> {
        let (table, column) = self.resolve_column(expr)?;
        let storage = self.substrate()?;
        let table_storage = storage.table(table)?;
        let rows = self.filtered_row_ids(table, &ctx.filters)?;
        let numbers: Vec<f64> = table_storage.numeric_values(column, &rows).ok()?.into_iter().filter(|number| number.is_finite()).collect();
        self.bump(|counters| &counters.aggregates);
        self.note_rows(rows.len());
        Some(numbers)
    }

    fn column_values(&self, expr: &BoundExpr, ctx: &mut Context) -> Result<Vec<Scalar>, EvalError> {
        match expr {
            BoundExpr::Column { table, column } => {
                if let Some(values) = self.storage_column(*table, *column, &ctx.filters) {
                    return Ok(values);
                }
                let table = self.table(*table, &ctx.filters);
                Ok(table.column(*column))
            }
            BoundExpr::UnqualifiedColumn { name } => {
                let tables = self.model.tables_with_column(name);
                if tables.len() != 1 {
                    return Err(EvalError::new(format!("column `[{name}]` is ambiguous")));
                }
                let table = tables[0];
                let column = self.model.column_index(table, name).ok_or_else(|| EvalError::new("missing column"))?;
                if let Some(values) = self.storage_column(table, column, &ctx.filters) {
                    return Ok(values);
                }
                Ok(self.table(table, &ctx.filters).column(column))
            }
            _ => Err(EvalError::new("aggregation needs a column reference")),
        }
    }

    fn apply_filter(&self, expr: &BoundExpr, filters: &mut FilterContext, ctx: &mut Context) -> Result<(), EvalError> {
        match expr {
            BoundExpr::Binary { op, left, right }
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge) =>
            {
                // DAX boolean filters accept every comparison operator, not just equality. The
                // column may be on either side; reverse the operator when it is on the right.
                let (table, column, value_expr, comparison) = match (&**left, &**right) {
                    (BoundExpr::Column { table, column }, value) => (*table, *column, value, *op),
                    (value, BoundExpr::Column { table, column }) => (*table, *column, value, reverse_comparison(*op)),
                    _ => return Err(EvalError::new("CALCULATE filter must compare a column")),
                };
                let value = self.eval(value_expr, ctx)?;
                // Build candidates without the target column's current filter: a CALCULATE filter
                // replaces that column's filter, so the outer `Region = "West"` must not hide the
                // rows that `Region = "East"` is meant to select.
                let mut candidate_filters = ctx.filters.clone();
                candidate_filters.filters.remove(&(table, column));
                let keys: BTreeSet<ScalarKey> = self
                    .table(table, &candidate_filters)
                    .column(column)
                    .into_iter()
                    .filter(|candidate| {
                        binary(comparison, candidate.clone(), value.clone()).ok().and_then(|result| result.to_bool()).unwrap_or(false)
                    })
                    .map(|candidate| ScalarKey::from(&candidate))
                    .collect();
                filters.set(table, column, keys);
                Ok(())
            }
            BoundExpr::Binary { op: BinaryOp::In, left, right } => {
                // `CALCULATE ( expr, Table[Column] IN { ... } )`: the list (or table expression)
                // is the membership set, and the filter keeps the rows whose column value is a
                // member — the same per-row truth the other comparison operators apply.
                let BoundExpr::Column { table, column } = &**left else {
                    return Err(EvalError::new("CALCULATE filter must compare a column"));
                };
                let members = self.eval_table(right, ctx)?;
                if members.columns.len() != 1 {
                    return Err(EvalError::new("CALCULATE filter IN needs a single-column table"));
                }
                let mut candidate_filters = ctx.filters.clone();
                candidate_filters.filters.remove(&(*table, *column));
                let keys: BTreeSet<ScalarKey> = self
                    .table(*table, &candidate_filters)
                    .column(*column)
                    .into_iter()
                    .filter(|candidate| members.rows.iter().any(|row| row.first().is_some_and(|member| member.strict_eq(candidate))))
                    .map(|candidate| ScalarKey::from(&candidate))
                    .collect();
                filters.set(*table, *column, keys);
                Ok(())
            }
            BoundExpr::Call { spec, args } if spec.name == "USERELATIONSHIP" || spec.name == "CROSSFILTER" => {
                self.apply_relationship_modifier(spec.name, args, filters)?;
                Ok(())
            }
            BoundExpr::Call { spec, args } if spec.name == "ALL" || spec.name == "REMOVEFILTERS" => {
                if let Some(BoundExpr::Column { table, .. }) = args.first() {
                    filters.remove_table(*table);
                } else if let Some(BoundExpr::Table(table)) = args.first() {
                    filters.remove_table(*table);
                }
                Ok(())
            }
            BoundExpr::Call { spec, args } if spec.name == "KEEPFILTERS" => {
                // KEEPFILTERS intersects its filter with the filters already in context instead of
                // replacing them; dropping the argument silently would ignore the filter.
                let Some(inner) = args.first() else {
                    return Err(EvalError::new("KEEPFILTERS needs a filter"));
                };
                let mut narrowed = filters.clone();
                self.apply_filter(inner, &mut narrowed, ctx)?;
                for (key, keys) in narrowed.filters {
                    let merged = match filters.filters.get(&key) {
                        Some(existing) => existing.intersection(&keys).cloned().collect(),
                        None => keys,
                    };
                    filters.set(key.0, key.1, merged);
                }
                for (key, state) in narrowed.relationships {
                    filters.relationships.insert(key, state);
                }
                Ok(())
            }
            _ => {
                // A table expression filter replaces the existing filters on its table (that is
                // what makes `CALCULATE(expr, PREVIOUSMONTH(Date[Date]))` override a Month
                // slicer); filters on other tables keep applying.
                let table = self.eval_table(expr, ctx)?;
                let Some(table_index) = table.lineage else {
                    return Err(EvalError::new("CALCULATE table filter has no source table"));
                };
                filters.remove_table(table_index);
                for (position, column) in table.columns.iter().enumerate() {
                    // A table expression may expose a subset/reordering of the model columns
                    // (VALUES(Sales[Amount]), DATESMTD(...)), so map by name, not by position.
                    let Some(model_column) = self.model.column_index(table_index, &column.name) else { continue };
                    let keys: BTreeSet<ScalarKey> = table.rows.iter().filter_map(|row| row.get(position)).map(ScalarKey::from).collect();
                    filters.set(table_index, model_column, keys);
                }
                Ok(())
            }
        }
    }

    /// `USERELATIONSHIP` and `CROSSFILTER` change how a relationship filters for the duration
    /// of the surrounding CALCULATE.
    fn apply_relationship_modifier(&self, name: &str, args: &[BoundExpr], filters: &mut FilterContext) -> Result<(), EvalError> {
        let column = |expr: Option<&BoundExpr>| match expr {
            Some(BoundExpr::Column { table, column }) => Ok((*table, *column)),
            _ => Err(EvalError::new(format!("{name} needs two column references"))),
        };
        let (table_a, column_a) = column(args.first())?;
        let (table_b, column_b) = column(args.get(1))?;
        if table_a == table_b {
            return Err(EvalError::new(format!("{name} needs columns from two different tables")));
        }
        let table_name = |table: usize| self.model.tables.get(table).map(|table| table.name.clone());
        let column_name =
            |table: usize, column: usize| self.model.tables.get(table).and_then(|table| table.columns.get(column)).map(|column| column.name.clone());
        let (Some(name_a), Some(name_b)) = (column_name(table_a, column_a), column_name(table_b, column_b)) else {
            return Err(EvalError::new(format!("{name} needs two column references")));
        };
        let (Some(table_a_name), Some(table_b_name)) = (table_name(table_a), table_name(table_b)) else {
            return Err(EvalError::new(format!("{name} needs two column references")));
        };
        let same = |left: &str, right: &str| left.eq_ignore_ascii_case(right);
        let found = self.model.relationships.iter().enumerate().find(|(_, candidate)| {
            let direct = same(&candidate.from_table, &table_a_name)
                && same(&candidate.from_column, &name_a)
                && same(&candidate.to_table, &table_b_name)
                && same(&candidate.to_column, &name_b);
            let reverse = same(&candidate.from_table, &table_b_name)
                && same(&candidate.from_column, &name_b)
                && same(&candidate.to_table, &table_a_name)
                && same(&candidate.to_column, &name_a);
            direct || reverse
        });
        let (index, relationship) = found.ok_or_else(|| EvalError::new(format!("{name} needs an existing relationship")))?;
        if name.eq_ignore_ascii_case("USERELATIONSHIP") {
            // Excel allows only one active relationship between two tables: activating this one
            // deactivates the others between the same pair.
            for (other_index, other) in self.model.relationships.iter().enumerate() {
                let same_pair = (same(&other.from_table, &relationship.from_table) && same(&other.to_table, &relationship.to_table))
                    || (same(&other.from_table, &relationship.to_table) && same(&other.to_table, &relationship.from_table));
                if same_pair {
                    filters.relationships.insert(other_index, (other_index == index, other.bidirectional));
                }
            }
            return Ok(());
        }
        let direction = match args.get(2) {
            Some(BoundExpr::Keyword(word)) | Some(BoundExpr::Literal(Literal::String(word))) => word.to_ascii_uppercase(),
            _ => return Err(EvalError::new("CROSSFILTER needs ONE, BOTH or NONE")),
        };
        let state = match direction.as_str() {
            "ONE" | "ONEWAY" => (true, false),
            "BOTH" | "TWOWAY" => (true, true),
            "NONE" => (false, false),
            _ => return Err(EvalError::new("CROSSFILTER direction must be ONE, ONEWAY, BOTH, TWOWAY or NONE")),
        };
        filters.relationships.insert(index, state);
        Ok(())
    }

    fn related(&self, arg: Option<&BoundExpr>, ctx: &mut Context) -> Result<Scalar, EvalError> {
        let Some(BoundExpr::Column { table, column }) = arg else {
            return Err(EvalError::new("RELATED needs a column"));
        };
        let Some(frame) = ctx.rows.last() else {
            return Err(EvalError::new("RELATED needs a row context"));
        };
        let Some(frame_table) = frame.lineage else {
            return Err(EvalError::new("RELATED needs a row context over a model table"));
        };
        if let Some(result) = self.related_via_index(*table, *column, frame_table, frame, &ctx.filters) {
            return result;
        }
        for relationship in &self.model.relationships {
            if relationship.inactive {
                continue;
            }
            let from = self.model.table_index(&relationship.from_table);
            let to = self.model.table_index(&relationship.to_table);
            if from == Some(frame_table) && to == Some(*table) {
                let from_col = self.model.column_index(frame_table, &relationship.from_column).ok_or_else(|| EvalError::new("bad relationship"))?;
                let to_col = self.model.column_index(*table, &relationship.to_column).ok_or_else(|| EvalError::new("bad relationship"))?;
                let key = frame.get(from_col).cloned().ok_or_else(|| EvalError::new("row is out of range"))?;
                let related = self.table(*table, &ctx.filters);
                for row in &related.rows {
                    if row.get(to_col).is_some_and(|value| value.strict_eq(&key)) {
                        return row.get(*column).cloned().ok_or_else(|| EvalError::new("column is out of range"));
                    }
                }
            }
        }
        Err(EvalError::new("no active relationship for RELATED"))
    }

    /// `RELATED` through the relationship index: the frame's key selects related row ids, so the
    /// related key column is never scanned. `None` keeps the row path, which also reproduces its
    /// errors (no row context, no active relationship, a key with no match).
    fn related_via_index(
        &self,
        table: usize,
        column: usize,
        frame_table: usize,
        frame: &RowFrame,
        filters: &FilterContext,
    ) -> Option<Result<Scalar, EvalError>> {
        let storage = self.substrate()?;
        for relationship in storage.relationships() {
            if relationship.inactive || relationship.from_table != frame_table || relationship.to_table != table {
                continue;
            }
            let key = frame.get(relationship.from_column)?;
            // ScalarKey folds NaN keys together while `strict_eq` does not; keep the row path for
            // that edge so the match rule stays strict.
            if is_nan_key(key) {
                return None;
            }
            let key = ScalarKey::from(key);
            self.bump(|counters| &counters.relationship_lookups);
            let rows = self.filtered_row_ids(table, filters)?;
            let found = relationship.rows_to(&key).iter().copied().find(|row| rows.binary_search(row).is_ok());
            // A matching relationship whose key is absent from the filtered rows falls through to
            // the next candidate relationship, like the key-column scan does.
            if let Some(matched) = found
                && let Some(value) = storage.value(table, column, matched)
            {
                self.bump(|counters| &counters.related);
                self.note_rows(1);
                return Some(Ok(value));
            }
        }
        None
    }

    /// `RELATEDTABLE` through the relationship index: the frame's key selects row ids in the
    /// related table, which are then intersected with its filtered rows. `None` keeps the row
    /// path, which also reproduces its errors.
    fn related_table_via_index(&self, lineage: usize, frame_table: usize, frame: &RowFrame, filters: &FilterContext) -> Option<Vec<Vec<Scalar>>> {
        let storage = self.substrate()?;
        let table_storage = storage.table(lineage)?;
        for relationship in storage.relationships() {
            if relationship.inactive {
                continue;
            }
            // `RELATEDTABLE` reads both directions: a many-side row lists its one-side rows and a
            // one-side row lists its many-side rows.
            let forward = relationship.from_table == frame_table && relationship.to_table == lineage;
            let reverse = relationship.to_table == frame_table && relationship.from_table == lineage;
            if !forward && !reverse {
                continue;
            }
            let key_column = if forward { relationship.from_column } else { relationship.to_column };
            let key = frame.get(key_column)?;
            if is_nan_key(key) {
                return None;
            }
            let key = ScalarKey::from(key);
            self.bump(|counters| &counters.relationship_lookups);
            let candidates = if forward { relationship.rows_to(&key) } else { relationship.rows_from(&key) };
            let rows = self.filtered_row_ids(lineage, filters)?;
            let columns = table_storage.columns().len();
            let matched: Vec<Vec<Scalar>> = candidates
                .iter()
                .filter(|row| rows.binary_search(row).is_ok())
                .map(|row| (0..columns).map(|column| storage.value(lineage, column, *row).unwrap_or(Scalar::Blank)).collect())
                .collect();
            self.bump(|counters| &counters.related_table);
            self.note_rows(matched.len());
            return Some(matched);
        }
        None
    }

    #[allow(dead_code)] // table families use this as they land
    fn table_index_for(&self, table: &TableValue) -> Option<usize> {
        if let Some(lineage) = table.lineage {
            return Some(lineage);
        }
        self.model.tables.iter().position(|model_table| {
            model_table.rows.len() == table.rows.len()
                && model_table.columns.len() == table.columns.len()
                && model_table.columns.iter().zip(&table.columns).all(|(a, b)| a.name.eq_ignore_ascii_case(&b.name))
        })
    }
}

pub(crate) struct Context {
    pub(crate) filters: FilterContext,
    pub(crate) rows: Vec<RowFrame>,
    pub(crate) vars: Vec<Scalar>,
}

fn literal_value(literal: &Literal) -> Scalar {
    match literal {
        Literal::Integer(value) => Scalar::Integer(*value),
        Literal::Decimal(value) => Scalar::Decimal(*value),
        Literal::DateTime(value) => Scalar::DateTime(date_serial(
            value.year as i64,
            value.month as i64,
            value.day as i64,
            value.hour as i64,
            value.minute as i64,
            value.second as i64,
        )),
        Literal::String(value) => Scalar::String(value.clone()),
        Literal::Boolean(value) => Scalar::Boolean(*value),
    }
}

fn scalar_from_key(key: &ScalarKey) -> Scalar {
    key.to_scalar()
}

/// The rows of `rows` that also appear in `keep`. Both are ascending row-id lists.
fn intersect_rows(rows: Vec<usize>, keep: &[usize]) -> Vec<usize> {
    rows.into_iter().filter(|row| keep.binary_search(row).is_ok()).collect()
}

/// Whether a scalar is a NaN number. [`ScalarKey`] folds NaNs into one key while `strict_eq`
/// never matches them, so relationship lookups fall back for this value.
fn is_nan_key(value: &Scalar) -> bool {
    match value {
        Scalar::Decimal(number) | Scalar::DateTime(number) => number.is_nan(),
        _ => false,
    }
}

fn reverse_comparison(op: BinaryOp) -> BinaryOp {
    match op {
        BinaryOp::Lt => BinaryOp::Gt,
        BinaryOp::Gt => BinaryOp::Lt,
        BinaryOp::Le => BinaryOp::Ge,
        BinaryOp::Ge => BinaryOp::Le,
        other => other,
    }
}

fn unary(op: UnaryOp, value: Scalar) -> Result<Scalar, EvalError> {
    match op {
        UnaryOp::Not => Ok(Scalar::Boolean(!value.to_bool().unwrap_or(false))),
        UnaryOp::Plus => Ok(value),
        UnaryOp::Neg => match value {
            Scalar::Blank => Ok(Scalar::Blank),
            Scalar::Integer(value) => Ok(Scalar::Integer(-value)),
            Scalar::Currency(value) => Ok(Scalar::Currency(-value)),
            other => other.to_number().map(|value| Scalar::Decimal(-value)).ok_or_else(|| EvalError::new("unary minus needs a number")),
        },
    }
}

fn binary(op: BinaryOp, left: Scalar, right: Scalar) -> Result<Scalar, EvalError> {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Pow => {
            if op == BinaryOp::Mul && (left.is_blank() || right.is_blank()) {
                return Ok(Scalar::Blank);
            }
            let a = left.to_number().ok_or_else(|| EvalError::new("arithmetic needs numbers"))?;
            let b = right.to_number().ok_or_else(|| EvalError::new("arithmetic needs numbers"))?;
            if op == BinaryOp::Div && b == 0.0 {
                return Err(EvalError::new("division by zero"));
            }
            let value = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => a / b,
                BinaryOp::Pow => a.powf(b),
                _ => return Err(EvalError::new("unsupported arithmetic operator")),
            };
            if value.is_nan() {
                return Ok(Scalar::Blank);
            }
            if matches!(left, Scalar::Integer(_)) && matches!(right, Scalar::Integer(_)) && !matches!(op, BinaryOp::Div | BinaryOp::Pow) {
                return Ok(Scalar::Integer(value as i64));
            }
            Ok(Scalar::Decimal(value))
        }
        BinaryOp::Concat => Ok(Scalar::String(format!("{}{}", left.to_text(), right.to_text()))),
        BinaryOp::Eq | BinaryOp::StrictEq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            let ordering = if op == BinaryOp::StrictEq {
                if left.strict_eq(&right) { Some(Ordering::Equal) } else { Some(Ordering::Less) }
            } else {
                left.compare(&right)
            };
            let Some(ordering) = ordering else { return Ok(Scalar::Boolean(false)) };
            let result = match op {
                BinaryOp::Eq | BinaryOp::StrictEq => ordering == Ordering::Equal,
                BinaryOp::Ne => ordering != Ordering::Equal,
                BinaryOp::Lt => ordering == Ordering::Less,
                BinaryOp::Gt => ordering == Ordering::Greater,
                BinaryOp::Le => ordering != Ordering::Greater,
                BinaryOp::Ge => ordering != Ordering::Less,
                _ => return Err(EvalError::new("unsupported comparison operator")),
            };
            Ok(Scalar::Boolean(result))
        }
        BinaryOp::And => Ok(Scalar::Boolean(left.to_bool().unwrap_or(false) && right.to_bool().unwrap_or(false))),
        BinaryOp::Or => Ok(Scalar::Boolean(left.to_bool().unwrap_or(false) || right.to_bool().unwrap_or(false))),
        BinaryOp::In => Err(EvalError::new("IN needs a table operand")),
    }
}

fn scalar_function(name: &str, args: &[Scalar]) -> Result<Scalar, EvalError> {
    let arg = |index: usize| args.get(index).ok_or_else(|| EvalError::new(format!("{name} is missing argument {}", index + 1)));
    let number = |index: usize| arg(index)?.to_number().ok_or_else(|| EvalError::new(format!("{name} argument {} must be numeric", index + 1)));
    let text = |index: usize| Ok::<String, EvalError>(arg(index)?.to_text());
    match name.to_ascii_uppercase().as_str() {
        "ABS" => Ok(Scalar::Decimal(number(0)?.abs())),
        "ROUND" => Ok(Scalar::Decimal(round_to(number(0)?, number(1)? as i32))),
        "ROUNDUP" => Ok(Scalar::Decimal(round_direction(number(0)?, number(1)? as i32, true))),
        "INT" => Ok(Scalar::Integer(number(0)?.floor() as i64)),
        "TRUNC" => Ok(Scalar::Decimal(number(0)?.trunc())),
        "POWER" => Ok(Scalar::Decimal(number(0)?.powf(number(1)?))),
        "SQRT" => Ok(Scalar::Decimal(number(0)?.sqrt())),
        "EXP" => Ok(Scalar::Decimal(number(0)?.exp())),
        "LN" => Ok(Scalar::Decimal(number(0)?.ln())),
        "LOG10" => Ok(Scalar::Decimal(number(0)?.log10())),
        "LOG" => Ok(Scalar::Decimal(if args.len() > 1 { number(0)?.log(number(1)?) } else { number(0)?.log10() })),
        "SIGN" => Ok(Scalar::Integer(number(0)?.signum() as i64)),
        "CEILING" | "ISO.CEILING" => Ok(Scalar::Decimal(number(0)?.ceil())),
        "FLOOR" => Ok(Scalar::Decimal(number(0)?.floor())),
        "DIVIDE" => {
            let (a, b) = (number(0)?, number(1)?);
            if b == 0.0 { Ok(args.get(2).cloned().unwrap_or(Scalar::Blank)) } else { Ok(Scalar::Decimal(a / b)) }
        }
        "FIND" | "SEARCH" => {
            let needle = text(0)?;
            let haystack = text(1)?;
            let start = if args.len() > 2 { number(2)?.max(1.0) as usize - 1 } else { 0 };
            let found =
                if name.eq_ignore_ascii_case("SEARCH") { haystack.to_lowercase().find(&needle.to_lowercase()) } else { haystack.find(&needle) };
            Ok(found.filter(|index| *index >= start).map(|index| Scalar::Integer((index + 1) as i64)).unwrap_or(Scalar::Blank))
        }
        "UNICHAR" => Ok(char::from_u32(number(0)?.max(0.0) as u32).map(|c| Scalar::String(c.to_string())).unwrap_or(Scalar::Blank)),
        "GCD" => Ok(Scalar::Integer(gcd(number(0)? as i64, number(1)? as i64))),
        "LCM" => Ok(Scalar::Integer(lcm(number(0)? as i64, number(1)? as i64))),
        "PI" => Ok(Scalar::Decimal(std::f64::consts::PI)),
        "DEGREES" => Ok(Scalar::Decimal(number(0)?.to_degrees())),
        "RADIANS" => Ok(Scalar::Decimal(number(0)?.to_radians())),
        "SIN" => Ok(Scalar::Decimal(number(0)?.sin())),
        "COS" => Ok(Scalar::Decimal(number(0)?.cos())),
        "TAN" => Ok(Scalar::Decimal(number(0)?.tan())),
        "FACT" => {
            let n = number(0)?;
            if !(0.0..=170.0).contains(&n) {
                return Err(EvalError::new("FACT argument is out of range"));
            }
            Ok(Scalar::Decimal((1..=n as u64).fold(1.0, |a, b| a * b as f64)))
        }
        "LEN" => Ok(Scalar::Integer(text(0)?.chars().count() as i64)),
        "UPPER" => Ok(Scalar::String(text(0)?.to_uppercase())),
        "LOWER" => Ok(Scalar::String(text(0)?.to_lowercase())),
        "LEFT" => Ok(Scalar::String(text(0)?.chars().take(number(1)? as usize).collect())),
        "RIGHT" => {
            let value = text(0)?;
            let n = number(1)? as usize;
            Ok(Scalar::String(value.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()))
        }
        "MID" => {
            let value = text(0)?;
            let start = number(1)?.max(1.0) as usize - 1;
            let count = number(2)?.max(0.0) as usize;
            Ok(Scalar::String(value.chars().skip(start).take(count).collect()))
        }
        "CONCATENATE" => Ok(Scalar::String(format!("{}{}", text(0)?, text(1)?))),
        "EXACT" => Ok(Scalar::Boolean(text(0)? == text(1)?)),
        "REPT" => {
            let value = text(0)?;
            let count = number(1)?.max(0.0) as usize;
            if value.len().checked_mul(count).is_none_or(|len| len > 32_767) {
                return Err(EvalError::new("REPT result is too long"));
            }
            Ok(Scalar::String(value.repeat(count)))
        }
        "SUBSTITUTE" => {
            let value = text(0)?;
            let old = text(1)?;
            let new = text(2)?;
            if old.is_empty() { Ok(Scalar::String(value)) } else { Ok(Scalar::String(value.replace(&old, &new))) }
        }
        "VALUE" => Ok(Scalar::Decimal(number(0)?)),
        "ISBLANK" => Ok(Scalar::Boolean(arg(0)?.is_blank())),
        "ISNUMBER" => Ok(Scalar::Boolean(matches!(arg(0)?, Scalar::Integer(_) | Scalar::Decimal(_) | Scalar::Currency(_)))),
        "ISTEXT" => Ok(Scalar::Boolean(matches!(arg(0)?, Scalar::String(_)))),
        "ISNONTEXT" => Ok(Scalar::Boolean(!matches!(arg(0)?, Scalar::String(_)))),
        "ISLOGICAL" => Ok(Scalar::Boolean(matches!(arg(0)?, Scalar::Boolean(_)))),
        "ISERROR" => Ok(Scalar::Boolean(false)),
        "ISEVEN" => Ok(Scalar::Boolean(number(0)? as i64 % 2 == 0)),
        "ISODD" => Ok(Scalar::Boolean(number(0)? as i64 % 2 != 0)),
        "ISINTEGER" => Ok(Scalar::Boolean(matches!(arg(0)?, Scalar::Integer(_)))),
        "ISCURRENCY" => Ok(Scalar::Boolean(matches!(arg(0)?, Scalar::Currency(_)))),
        "DATE" => Ok(Scalar::DateTime(date_serial(number(0)? as i64, number(1)? as i64, number(2)? as i64, 0, 0, 0))),
        "YEAR" => Ok(Scalar::Integer(date_parts(number(0)?).0)),
        "MONTH" => Ok(Scalar::Integer(date_parts(number(0)?).1)),
        "DAY" => Ok(Scalar::Integer(date_parts(number(0)?).2)),
        "HOUR" => Ok(Scalar::Integer((number(0)?.fract() * 24.0).floor() as i64)),
        "MINUTE" => Ok(Scalar::Integer(((number(0)?.fract() * 24.0).fract() * 60.0).floor() as i64)),
        "SECOND" => Ok(Scalar::Integer((((number(0)?.fract() * 24.0).fract() * 60.0).fract() * 60.0).round() as i64)),
        "EOMONTH" => Ok(Scalar::DateTime(eomonth(number(0)?, number(1)? as i64))),
        "EDATE" => Ok(Scalar::DateTime(edate(number(0)?, number(1)? as i64))),
        "PATH" => Ok(Scalar::String(format!("{}{}", text(0)?, text(1)?))),
        "PATHLENGTH" => Ok(Scalar::Integer(text(0)?.split('|').count() as i64)),
        "PATHCONTAINS" => {
            let path = text(0)?;
            let needle = text(1)?;
            Ok(Scalar::Boolean(path.split('|').any(|part| part == needle)))
        }
        "PATHITEM" => {
            let path = text(0)?;
            let parts: Vec<&str> = path.split('|').collect();
            let index = number(1)?.max(1.0) as usize - 1;
            Ok(parts.get(index).map(|value| Scalar::String((*value).to_string())).unwrap_or(Scalar::Blank))
        }
        "BLANK" => Ok(Scalar::Blank),
        "ERROR" => Err(EvalError::new(arg(0)?.to_text())),
        _ => Err(EvalError::not_implemented(name)),
    }
}
