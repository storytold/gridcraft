//! A small, deterministic DAX evaluator over an in-memory [`DataModel`].
//!
//! Supported today: literals, operators, variables, column/measure/table references, the 22
//! aggregation functions, `CALCULATE`/`FILTER`/`ALL`/`VALUES`/`DISTINCT`/`SELECTEDVALUE`, common
//! logical/table/relationship/date/text/math/information functions, and basic pivot grouping.
//! Functions outside this subset return [`EvalError::not_implemented`]; the parser and catalog
//! still accept them.

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

use crate::ast::{BinaryOp, Literal, UnaryOp};
use crate::binder::{BindError, Binder, BoundExpr};
use crate::model::DataModel;
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

/// An evaluator with pre-bound measures.
pub struct Evaluator<'a> {
    model: &'a DataModel,
    measures: Vec<BoundExpr>,
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
        Ok(Evaluator { model, measures })
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

    pub(crate) fn table(&self, table: usize, filters: &FilterContext) -> TableValue {
        self.table_inner(table, filters, &mut BTreeSet::new())
    }

    fn table_inner(&self, table: usize, filters: &FilterContext, visited: &mut BTreeSet<usize>) -> TableValue {
        let Some(model_table) = self.model.tables.get(table) else {
            return TableValue::new(Vec::new(), Vec::new());
        };
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

    fn call(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
        match name.to_ascii_uppercase().as_str() {
            "TRUE" => return Ok(Scalar::Boolean(true)),
            "FALSE" => return Ok(Scalar::Boolean(false)),
            "BLANK" => return Ok(Scalar::Blank),
            "IF" => {
                let condition = self.eval(args.first().ok_or_else(|| EvalError::new("IF needs a condition"))?, ctx)?;
                let branch = if condition.to_bool().unwrap_or(false) { args.get(1) } else { args.get(2) };
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
                let table = match args.first() {
                    Some(arg) => self.eval_table(arg, ctx)?,
                    None => return Err(EvalError::new("COUNTROWS() without a home table is not supported")),
                };
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
                let table = self.eval_table(args.first().ok_or_else(|| EvalError::new("RELATEDTABLE needs a table"))?, ctx)?;
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
        let values = self.column_values(args.first().ok_or_else(|| EvalError::new("aggregation needs a column"))?, ctx)?;
        aggregate_values(name, &values)
    }

    fn aggregate_x(&self, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
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

    fn column_values(&self, expr: &BoundExpr, ctx: &mut Context) -> Result<Vec<Scalar>, EvalError> {
        match expr {
            BoundExpr::Column { table, column } => {
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
        "ROUNDDOWN" => Ok(Scalar::Decimal(round_direction(number(0)?, number(1)? as i32, false))),
        "INT" => Ok(Scalar::Integer(number(0)?.floor() as i64)),
        "TRUNC" => Ok(Scalar::Decimal(number(0)?.trunc())),
        "MOD" => {
            let (a, b) = (number(0)?, number(1)?);
            if b == 0.0 { Ok(Scalar::Blank) } else { Ok(Scalar::Decimal(a % b)) }
        }
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
        "TIME" => Ok(Scalar::DateTime((number(0)? * 3600.0 + number(1)? * 60.0 + number(2)?) / 86400.0)),
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
        "TRIM" => Ok(Scalar::String(text(0)?.trim().to_string())),
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
        "WEEKDAY" => Ok(Scalar::Integer(date_weekday(number(0)?))),
        "EOMONTH" => Ok(Scalar::DateTime(eomonth(number(0)?, number(1)? as i64))),
        "EDATE" => Ok(Scalar::DateTime(edate(number(0)?, number(1)? as i64))),
        "DATEDIFF" => Ok(Scalar::Integer((number(1)? - number(0)?).trunc() as i64)),
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
