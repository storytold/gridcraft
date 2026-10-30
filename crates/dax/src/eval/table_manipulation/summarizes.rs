//! Table manipulation: grouping, summarization and rollups.
//!
//! `SUMMARIZE`, `SUMMARIZECOLUMNS` and `GROUPBY` share the grouping machinery here: the group-by
//! columns define the group keys and every added expression is evaluated in a filter context that
//! only keeps the current group's column values. `ROLLUP`/`ROLLUPGROUP` and
//! `ROLLUPADDISSUBTOTAL` add subtotal rows on top of those groups, `ROLLUPISSUBTOTAL` pairs the
//! subtotal indicators with their columns for `ADDMISSINGITEMS`, `CURRENTGROUP` reads the group
//! pushed by `GROUPBY`, and `IGNORE`/`NONVISUAL` are annotations that pass their argument through.
//! `TOPNPERLEVEL` and `TOPNSKIP` are the row selectors in this family.
//!
//! Row order is not guaranteed by DAX. This implementation sorts detail rows by their group
//! values and places each subtotal after the rows it summarizes (blanks sort last), so results
//! are deterministic.
#![allow(unused_imports)]

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::super::FilterContext;
use super::super::support::frame_for;
use super::*;
use crate::ast::{BinaryOp, Literal};
use crate::value::{Scalar, ScalarKey, TableColumn, TableValue};

/// Hard cap on the rows a grouping function materializes: group sets come from the model and the
/// cartesian product of column domains is unbounded otherwise.
const MAX_GROUP_ROWS: usize = 250_000;

thread_local! {
    /// Group rows pushed by `GROUPBY` while its added expressions are evaluated. `CURRENTGROUP()`
    /// is only valid there; evaluation is single-threaded, so a thread-local stack is enough. The
    /// guard pops on drop, so errors cannot leave a stale group behind.
    static CURRENT_GROUPS: RefCell<Vec<TableValue>> = const { RefCell::new(Vec::new()) };

    /// Columns that are rolled up (blanked) in the SUMMARIZE row currently being evaluated.
    /// `None` outside a SUMMARIZE expression, so `ISSUBTOTAL` can tell "detail row" from
    /// "not in a SUMMARIZE at all".
    static SUBTOTAL_COLUMNS: RefCell<Option<Vec<(usize, usize)>>> = const { RefCell::new(None) };
}

/// The rolled-up columns of the SUMMARIZE row being evaluated, or `None` outside SUMMARIZE.
pub(crate) fn subtotal_columns() -> Option<Vec<(usize, usize)>> {
    SUBTOTAL_COLUMNS.with(|columns| columns.borrow().clone())
}

struct SubtotalGuard;

impl SubtotalGuard {
    fn push(columns: Vec<(usize, usize)>) -> Self {
        SUBTOTAL_COLUMNS.with(|current| *current.borrow_mut() = Some(columns));
        SubtotalGuard
    }
}

impl Drop for SubtotalGuard {
    fn drop(&mut self) {
        SUBTOTAL_COLUMNS.with(|current| *current.borrow_mut() = None);
    }
}

struct CurrentGroupGuard;

impl CurrentGroupGuard {
    fn push(table: TableValue) -> Self {
        CURRENT_GROUPS.with(|groups| groups.borrow_mut().push(table));
        CurrentGroupGuard
    }
}

impl Drop for CurrentGroupGuard {
    fn drop(&mut self) {
        CURRENT_GROUPS.with(|groups| {
            groups.borrow_mut().pop();
        });
    }
}

/// A resolved `Table[Column]` reference used by the grouping functions.
#[derive(Clone, Debug, PartialEq)]
struct ColumnRef {
    table: usize,
    column: usize,
    name: String,
}

/// One group-by argument, before it is expanded into output columns.
#[derive(Clone, Debug)]
enum GroupArg {
    Column(ColumnRef),
    /// `ROLLUP(...)`: one entry per level; a `ROLLUPGROUP` collapses its columns into one level.
    Rollup(Vec<Vec<ColumnRef>>),
    /// `ROLLUPGROUP(...)` used directly: a plain column group without subtotals.
    Group(Vec<ColumnRef>),
}

/// The expanded output columns of a `SUMMARIZE`/`GROUPBY` call plus its rollup levels.
struct GroupLayout {
    columns: Vec<ColumnRef>,
    /// Rollup levels as `(start, len)` ranges into `columns`, from the coarsest to the finest.
    levels: Vec<(usize, usize)>,
}

struct SummarizeArgs<'a> {
    group: Vec<GroupArg>,
    expressions: Vec<(String, &'a BoundExpr)>,
}

/// A `SUMMARIZECOLUMNS` rollup: its levels and the subtotal indicator column of each level.
struct RollupDimension {
    /// Column indices (into the global group column list) per level.
    levels: Vec<Vec<usize>>,
    /// Subtotal indicator name per level, in argument order.
    flags: Vec<(usize, String)>,
}

struct SummarizeColumnsArgs<'a> {
    columns: Vec<ColumnRef>,
    dimensions: Vec<RollupDimension>,
    /// Filters with a `non_visual` marker: NONVISUAL filters restrict the grouping only.
    filters: Vec<(&'a BoundExpr, bool)>,
    /// Added expression columns with an `IGNORE` marker.
    expressions: Vec<(String, &'a BoundExpr, bool)>,
}

#[derive(Default)]
struct LevelDef {
    visible: Vec<String>,
    order: Vec<String>,
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        // IGNORE and NONVISUAL are annotations: outside their host function they are the identity.
        "IGNORE" | "NONVISUAL" => match args.first() {
            Some(arg) => ev.eval(arg, ctx),
            None => Err(EvalError::new(format!("{upper} needs an expression"))),
        },
        "SUMMARIZE" | "SUMMARIZECOLUMNS" | "GROUPBY" | "ADDMISSINGITEMS" | "TOPNPERLEVEL" | "TOPNSKIP" | "CURRENTGROUP" => {
            Err(EvalError::new(format!("{upper} returns a table and cannot be used as a scalar")))
        }
        "ROLLUP" | "ROLLUPGROUP" | "ROLLUPADDISSUBTOTAL" | "ROLLUPISSUBTOTAL" => Err(EvalError::new(companion_error(&upper))),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "SUMMARIZE" => summarize(ev, args, ctx),
        "SUMMARIZECOLUMNS" => summarize_columns(ev, args, ctx),
        "GROUPBY" => group_by(ev, args, ctx),
        "ADDMISSINGITEMS" => add_missing_items(ev, args, ctx),
        "TOPNPERLEVEL" => top_n_per_level(ev, args, ctx),
        "TOPNSKIP" => top_n_skip(ev, args, ctx),
        "CURRENTGROUP" => current_group(),
        "IGNORE" | "NONVISUAL" => match args.first() {
            Some(arg) => ev.eval_table(arg, ctx),
            None => Err(EvalError::new(format!("{upper} needs a table"))),
        },
        "ROLLUP" | "ROLLUPGROUP" | "ROLLUPADDISSUBTOTAL" | "ROLLUPISSUBTOTAL" => Err(EvalError::new(companion_error(&upper))),
        _ => return None,
    };
    Some(result)
}

fn companion_error(name: &str) -> String {
    match name {
        "ROLLUP" => "ROLLUP is only valid as a SUMMARIZE group-by argument".to_string(),
        "ROLLUPGROUP" => "ROLLUPGROUP is only valid inside ROLLUP or ROLLUPADDISSUBTOTAL".to_string(),
        "ROLLUPADDISSUBTOTAL" => "ROLLUPADDISSUBTOTAL is only valid inside SUMMARIZECOLUMNS".to_string(),
        _ => "ROLLUPISSUBTOTAL is only valid inside ADDMISSINGITEMS".to_string(),
    }
}

fn current_group() -> Result<TableValue, EvalError> {
    CURRENT_GROUPS.with(|groups| groups.borrow().last().cloned()).ok_or_else(|| EvalError::new("CURRENTGROUP() is only valid inside GROUPBY"))
}

fn resolve_column(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<ColumnRef, EvalError> {
    match expr {
        BoundExpr::Column { table, column } => {
            let name = ev
                .model()
                .tables
                .get(*table)
                .and_then(|table| table.columns.get(*column))
                .map(|column| column.name.clone())
                .ok_or_else(|| EvalError::new("column reference is out of range"))?;
            Ok(ColumnRef { table: *table, column: *column, name })
        }
        BoundExpr::UnqualifiedColumn { name } => {
            let tables = ev.model().tables_with_column(name);
            if tables.len() != 1 {
                return Err(EvalError::new(format!("column [{name}] is ambiguous or unknown")));
            }
            let table = tables[0];
            let column = ev.model().column_index(table, name).ok_or_else(|| EvalError::new(format!("column [{name}] is unknown")))?;
            Ok(ColumnRef { table, column, name: name.clone() })
        }
        _ => Err(EvalError::new("a group-by argument must be a column reference")),
    }
}

/// The display name of a reference, including columns that only exist in the produced table
/// (the subtotal indicators used by `ROLLUPISSUBTOTAL`).
fn reference_name(ev: &Evaluator<'_>, expr: &BoundExpr) -> Option<String> {
    match expr {
        BoundExpr::Column { table, column } => ev.model().tables.get(*table)?.columns.get(*column).map(|column| column.name.clone()),
        BoundExpr::UnqualifiedColumn { name } => Some(name.clone()),
        BoundExpr::Measure(index) => ev.model().measures.get(*index).map(|measure| measure.name.clone()),
        _ => None,
    }
}

fn compare_scalar(left: &Scalar, right: &Scalar) -> Ordering {
    match (left.is_blank(), right.is_blank()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => ScalarKey::from(left).cmp(&ScalarKey::from(right)),
    }
}

fn compare_values(left: &[Scalar], right: &[Scalar]) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = compare_scalar(left, right);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// The value of `column` for a row of `table`: the row itself when the column belongs to the
/// table, a related-table value when the column is on a table reachable through a relationship,
/// otherwise a column with the same name (SUMMARIZE accepts columns of the summarized table and
/// of its related tables; derived tables keep their columns by name).
fn group_value(ev: &Evaluator<'_>, table: &TableValue, row: &[Scalar], column: &ColumnRef) -> Result<Scalar, EvalError> {
    if table.lineage == Some(column.table) {
        return row.get(column.column).cloned().ok_or_else(|| EvalError::new("row is out of range"));
    }
    if let Some(lineage) = table.lineage
        && let Some(value) = related_value(ev, lineage, row, column.table, column.column)
    {
        return Ok(value);
    }
    if let Some(index) = table.column_index(&column.name) {
        return row.get(index).cloned().ok_or_else(|| EvalError::new("row is out of range"));
    }
    Err(EvalError::new(format!("column [{}] is not available in this table", column.name)))
}

/// Follows an active relationship from `base` to `target` for one row.
fn related_value(ev: &Evaluator<'_>, base: usize, row: &[Scalar], target: usize, target_column: usize) -> Option<Scalar> {
    let model = ev.model();
    for relationship in &model.relationships {
        if relationship.inactive {
            continue;
        }
        let Some(from) = model.table_index(&relationship.from_table) else { continue };
        let Some(to) = model.table_index(&relationship.to_table) else { continue };
        let (base_key, target_key) = if from == base && to == target {
            (&relationship.from_column, &relationship.to_column)
        } else if relationship.bidirectional && from == target && to == base {
            (&relationship.to_column, &relationship.from_column)
        } else {
            continue;
        };
        let Some(base_column) = model.column_index(base, base_key) else { continue };
        let Some(target_key_column) = model.column_index(target, target_key) else { continue };
        let Some(key) = row.get(base_column) else { continue };
        let Some(target_table) = model.tables.get(target) else { continue };
        for target_row in &target_table.rows {
            if target_row.get(target_key_column).is_some_and(|value| value.strict_eq(key)) {
                return target_row.get(target_column).cloned();
            }
        }
    }
    None
}

fn parse_summarize_args<'a>(ev: &Evaluator<'_>, args: &'a [BoundExpr], function: &str) -> Result<SummarizeArgs<'a>, EvalError> {
    if args.is_empty() {
        return Err(EvalError::new(format!("{function} needs a table")));
    }
    let mut group = Vec::new();
    let mut expressions: Vec<(String, &BoundExpr)> = Vec::new();
    let mut index = 1usize;
    while index < args.len() {
        if let BoundExpr::Literal(Literal::String(name)) = &args[index] {
            let Some(expression) = args.get(index + 1) else {
                return Err(EvalError::new(format!("{function} needs an expression for column `{name}`")));
            };
            expressions.push((name.clone(), expression));
            index += 2;
            continue;
        }
        if !expressions.is_empty() {
            return Err(EvalError::new(format!("{function} group-by columns must come before the name/expression pairs")));
        }
        group.push(parse_group_arg(ev, &args[index], function)?);
        index += 1;
    }
    Ok(SummarizeArgs { group, expressions })
}

fn parse_group_arg(ev: &Evaluator<'_>, expr: &BoundExpr, function: &str) -> Result<GroupArg, EvalError> {
    if let BoundExpr::Call { spec, args } = expr {
        match spec.name {
            "ROLLUP" => {
                if function != "SUMMARIZE" {
                    return Err(EvalError::new("ROLLUP is only valid inside SUMMARIZE"));
                }
                if args.is_empty() {
                    return Err(EvalError::new("ROLLUP needs at least one column"));
                }
                let mut levels = Vec::with_capacity(args.len());
                for arg in args {
                    levels.push(rollup_level(ev, arg)?);
                }
                return Ok(GroupArg::Rollup(levels));
            }
            "ROLLUPGROUP" => return Ok(GroupArg::Group(rollup_level(ev, expr)?)),
            "ROLLUPADDISSUBTOTAL" | "ROLLUPISSUBTOTAL" => return Err(EvalError::new(companion_error(spec.name))),
            _ => {}
        }
    }
    Ok(GroupArg::Column(resolve_column(ev, expr)?))
}

/// The columns of one rollup level: a single column, or every column of a `ROLLUPGROUP`.
fn rollup_level(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<Vec<ColumnRef>, EvalError> {
    if let BoundExpr::Call { spec, args } = expr
        && spec.name == "ROLLUPGROUP"
    {
        if args.is_empty() {
            return Err(EvalError::new("ROLLUPGROUP needs at least one column"));
        }
        return args.iter().map(|arg| resolve_column(ev, arg)).collect();
    }
    Ok(vec![resolve_column(ev, expr)?])
}

fn group_layout(group: &[GroupArg]) -> Result<GroupLayout, EvalError> {
    let mut columns: Vec<ColumnRef> = Vec::new();
    let mut levels: Vec<(usize, usize)> = Vec::new();
    for arg in group {
        match arg {
            GroupArg::Column(column) => columns.push(column.clone()),
            GroupArg::Group(group) => columns.extend(group.iter().cloned()),
            GroupArg::Rollup(rollup_levels) => {
                if !levels.is_empty() {
                    return Err(EvalError::new("SUMMARIZE accepts a single ROLLUP argument"));
                }
                for level in rollup_levels {
                    let start = columns.len();
                    columns.extend(level.iter().cloned());
                    levels.push((start, level.len()));
                }
            }
        }
    }
    for (index, column) in columns.iter().enumerate() {
        if columns[..index].iter().any(|other| other.table == column.table && other.column == column.column) {
            return Err(EvalError::new(format!("column [{}] is specified more than once", column.name)));
        }
    }
    Ok(GroupLayout { columns, levels })
}

/// Groups the rows of `table` by `columns`; each entry holds the group values and the source rows.
#[allow(clippy::type_complexity)]
// Each entry pairs the group's key values with the rows that fall in it.
fn group_details(ev: &Evaluator<'_>, table: &TableValue, columns: &[ColumnRef]) -> Result<Vec<(Vec<Scalar>, Vec<Vec<Scalar>>)>, EvalError> {
    let mut groups: BTreeMap<Vec<ScalarKey>, (Vec<Scalar>, Vec<Vec<Scalar>>)> = BTreeMap::new();
    for row in &table.rows {
        let mut values = Vec::with_capacity(columns.len());
        for column in columns {
            values.push(group_value(ev, table, row, column)?);
        }
        let key: Vec<ScalarKey> = values.iter().map(ScalarKey::from).collect();
        groups.entry(key).or_insert_with(|| (values, Vec::new())).1.push(row.clone());
    }
    if groups.len() > MAX_GROUP_ROWS {
        return Err(EvalError::new("grouping produced too many groups"));
    }
    let mut details: Vec<(Vec<Scalar>, Vec<Vec<Scalar>>)> = groups.into_values().collect();
    details.sort_by(|left, right| compare_values(&left.0, &right.0));
    Ok(details)
}

/// The subtotal rows of a rollup: for each level boundary from the finest to the coarsest, the
/// rolled-up columns are blank and excluded from the measure filter context.
fn subtotal_rows(details: &[(Vec<Scalar>, Vec<Vec<Scalar>>)], layout: &GroupLayout) -> Vec<(Vec<Scalar>, Vec<bool>)> {
    let count = layout.levels.len();
    let mut rows: Vec<(Vec<Scalar>, Vec<bool>)> = Vec::new();
    let mut seen: BTreeSet<(Vec<ScalarKey>, Vec<bool>)> = BTreeSet::new();
    for depth in 1..=count {
        let rolled: BTreeSet<usize> = layout.levels[count - depth..].iter().flat_map(|(start, len)| *start..start + len).collect();
        let mut prefixes: BTreeSet<Vec<ScalarKey>> = BTreeSet::new();
        for (values, _) in details {
            let prefix: Vec<ScalarKey> =
                values.iter().enumerate().filter(|(index, _)| !rolled.contains(index)).map(|(_, value)| ScalarKey::from(value)).collect();
            if !prefixes.insert(prefix) {
                continue;
            }
            let mut subtotal = values.clone();
            let mut active = vec![true; layout.columns.len()];
            for index in &rolled {
                if let Some(value) = subtotal.get_mut(*index) {
                    *value = Scalar::Blank;
                }
                if let Some(slot) = active.get_mut(*index) {
                    *slot = false;
                }
            }
            let key = (subtotal.iter().map(ScalarKey::from).collect::<Vec<_>>(), active.clone());
            if seen.insert(key) {
                rows.push((subtotal, active));
            }
        }
    }
    rows
}

fn eval_in_group(
    ev: &Evaluator<'_>,
    ctx: &Context,
    expression: &BoundExpr,
    columns: &[ColumnRef],
    values: &[Scalar],
    active: &[bool],
) -> Result<Scalar, EvalError> {
    let mut filters = ctx.filters.clone();
    for ((column, value), active) in columns.iter().zip(values).zip(active) {
        if *active {
            filters.set(column.table, column.column, [ScalarKey::from(value)]);
        }
    }
    let mut inner = Context { filters, rows: Vec::new(), vars: ctx.vars.clone() };
    ev.eval(expression, &mut inner)
}

fn summarize(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let Some(first) = args.first() else {
        return Err(EvalError::new("SUMMARIZE needs a table"));
    };
    let table = ev.eval_table(first, ctx)?;
    let parsed = parse_summarize_args(ev, args, "SUMMARIZE")?;
    let layout = group_layout(&parsed.group)?;
    if layout.columns.is_empty() {
        return Ok(table.distinct_rows());
    }
    let details = group_details(ev, &table, &layout.columns)?;
    let mut rows: Vec<(Vec<Scalar>, Vec<bool>)> = details.iter().map(|(values, _)| (values.clone(), vec![true; layout.columns.len()])).collect();
    if !layout.levels.is_empty() {
        rows.extend(subtotal_rows(&details, &layout));
    }
    rows.sort_by(|left, right| compare_values(&left.0, &right.0));
    let mut seen: BTreeSet<(Vec<ScalarKey>, Vec<bool>)> = BTreeSet::new();
    rows.retain(|(values, active)| seen.insert((values.iter().map(ScalarKey::from).collect(), active.clone())));

    let mut columns: Vec<TableColumn> = layout.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect();
    for (name, _) in &parsed.expressions {
        columns.push(TableColumn { name: name.clone() });
    }
    let mut output = Vec::with_capacity(rows.len());
    for (values, active) in rows {
        let mut row = values.clone();
        // Detail rows have every column active; a ROLLUP row deactivates the rolled-up columns.
        let rolled_up: Vec<(usize, usize)> =
            layout.columns.iter().zip(&active).filter(|(_, active)| !**active).map(|(column, _)| (column.table, column.column)).collect();
        {
            let _guard = SubtotalGuard::push(rolled_up);
            for (_, expression) in &parsed.expressions {
                row.push(eval_in_group(ev, ctx, expression, &layout.columns, &values, &active)?);
            }
        }
        output.push(row);
    }
    Ok(TableValue::new(columns, output))
}

fn group_by(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let Some(first) = args.first() else {
        return Err(EvalError::new("GROUPBY needs a table"));
    };
    let table = ev.eval_table(first, ctx)?;
    let parsed = parse_summarize_args(ev, args, "GROUPBY")?;
    let mut columns = Vec::new();
    for arg in &parsed.group {
        match arg {
            GroupArg::Column(column) => columns.push(column.clone()),
            GroupArg::Group(group) => columns.extend(group.iter().cloned()),
            GroupArg::Rollup(_) => return Err(EvalError::new("GROUPBY does not support ROLLUP; use SUMMARIZE")),
        }
    }
    if columns.is_empty() {
        return Ok(table.distinct_rows());
    }
    for (index, column) in columns.iter().enumerate() {
        if columns[..index].iter().any(|other| other.table == column.table && other.column == column.column) {
            return Err(EvalError::new(format!("column [{}] is specified more than once", column.name)));
        }
    }
    let details = group_details(ev, &table, &columns)?;
    let mut output_columns: Vec<TableColumn> = columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect();
    for (name, _) in &parsed.expressions {
        output_columns.push(TableColumn { name: name.clone() });
    }
    let active = vec![true; columns.len()];
    let mut rows = Vec::with_capacity(details.len());
    for (values, group_rows) in &details {
        let current = TableValue::new(table.columns.clone(), group_rows.clone()).with_lineage_if(table.lineage);
        let mut row = values.clone();
        {
            let _guard = CurrentGroupGuard::push(current);
            for (_, expression) in &parsed.expressions {
                row.push(eval_in_group(ev, ctx, expression, &columns, values, &active)?);
            }
        }
        rows.push(row);
    }
    Ok(TableValue::new(output_columns, rows))
}

fn parse_summarize_columns<'a>(ev: &Evaluator<'_>, args: &'a [BoundExpr]) -> Result<SummarizeColumnsArgs<'a>, EvalError> {
    let mut columns: Vec<ColumnRef> = Vec::new();
    let mut dimensions: Vec<RollupDimension> = Vec::new();
    let mut filters: Vec<(&BoundExpr, bool)> = Vec::new();
    let mut expressions: Vec<(String, &BoundExpr, bool)> = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if let BoundExpr::Literal(Literal::String(name)) = arg {
            let Some(expression) = args.get(index + 1) else {
                return Err(EvalError::new(format!("SUMMARIZECOLUMNS needs an expression for column `{name}`")));
            };
            match expression {
                BoundExpr::Call { spec, args: inner } if spec.name == "IGNORE" => {
                    let inner = inner.first().ok_or_else(|| EvalError::new("IGNORE needs an expression"))?;
                    expressions.push((name.clone(), inner, true));
                }
                other => expressions.push((name.clone(), other, false)),
            }
            index += 2;
            continue;
        }
        if let BoundExpr::Call { spec, args: inner } = arg {
            match spec.name {
                "ROLLUPADDISSUBTOTAL" => {
                    dimensions.push(parse_rollup_dimension(ev, inner, &mut columns, &mut filters)?);
                    index += 1;
                    continue;
                }
                "ROLLUPGROUP" => {
                    if inner.is_empty() {
                        return Err(EvalError::new("ROLLUPGROUP needs at least one column"));
                    }
                    for column in inner {
                        columns.push(resolve_column(ev, column)?);
                    }
                    index += 1;
                    continue;
                }
                "NONVISUAL" => {
                    let inner = inner.first().ok_or_else(|| EvalError::new("NONVISUAL needs a filter"))?;
                    filters.push((inner, true));
                    index += 1;
                    continue;
                }
                "KEEPFILTERS" => {
                    let inner = inner.first().ok_or_else(|| EvalError::new("KEEPFILTERS needs a filter"))?;
                    filters.push((inner, false));
                    index += 1;
                    continue;
                }
                "ROLLUP" | "ROLLUPISSUBTOTAL" => return Err(EvalError::new(companion_error(spec.name))),
                _ => {}
            }
        }
        match arg {
            BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } => columns.push(resolve_column(ev, arg)?),
            other => filters.push((other, false)),
        }
        index += 1;
    }
    for (index, column) in columns.iter().enumerate() {
        if columns[..index].iter().any(|other| other.table == column.table && other.column == column.column) {
            return Err(EvalError::new(format!("column [{}] is specified more than once", column.name)));
        }
    }
    Ok(SummarizeColumnsArgs { columns, dimensions, filters, expressions })
}

fn parse_rollup_dimension<'a>(
    ev: &Evaluator<'_>,
    args: &'a [BoundExpr],
    columns: &mut Vec<ColumnRef>,
    filters: &mut Vec<(&'a BoundExpr, bool)>,
) -> Result<RollupDimension, EvalError> {
    let mut dimension = RollupDimension { levels: Vec::new(), flags: Vec::new() };
    let mut index = 0usize;
    while index < args.len() {
        let members = match &args[index] {
            BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } => vec![resolve_column(ev, &args[index])?],
            BoundExpr::Call { spec, args: inner } if spec.name == "ROLLUPGROUP" => {
                if inner.is_empty() {
                    return Err(EvalError::new("ROLLUPGROUP needs at least one column"));
                }
                inner.iter().map(|column| resolve_column(ev, column)).collect::<Result<Vec<_>, _>>()?
            }
            // The grand-total filter before the pairs and the per-level filters between them both
            // restrict the grouping; they are applied to the whole grouping (see module notes).
            other => {
                filters.push((other, false));
                index += 1;
                continue;
            }
        };
        index += 1;
        let name = match args.get(index) {
            Some(BoundExpr::Literal(Literal::String(name))) => name.clone(),
            _ => return Err(EvalError::new("ROLLUPADDISSUBTOTAL needs a column name after each group-by column")),
        };
        index += 1;
        let level = dimension.levels.len();
        let mut positions = Vec::with_capacity(members.len());
        for member in members {
            positions.push(columns.len());
            columns.push(member);
        }
        dimension.levels.push(positions);
        dimension.flags.push((level, name));
        if let Some(next) = args.get(index) {
            let is_filter = match next {
                BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } => false,
                BoundExpr::Literal(Literal::String(_)) => false,
                BoundExpr::Call { spec, .. } if spec.name == "ROLLUPGROUP" => false,
                _ => true,
            };
            if is_filter {
                filters.push((next, false));
                index += 1;
            }
        }
    }
    if dimension.levels.is_empty() {
        return Err(EvalError::new("ROLLUPADDISSUBTOTAL needs at least one group-by column"));
    }
    Ok(dimension)
}

fn transitioned_filters(ctx: &Context) -> FilterContext {
    let mut filters = ctx.filters.clone();
    if let Some(frame) = ctx.rows.last()
        && let Some(table) = frame.lineage
    {
        for (index, column) in frame.model_columns.iter().enumerate() {
            if let (Some(column), Some(value)) = (column, frame.values.get(index)) {
                filters.set(table, *column, [ScalarKey::from(value)]);
            }
        }
    }
    filters
}

fn apply_filter_arg(ev: &Evaluator<'_>, expr: &BoundExpr, filters: &mut FilterContext, ctx: &mut Context) -> Result<(), EvalError> {
    if let BoundExpr::Call { spec, args } = expr {
        match spec.name {
            "KEEPFILTERS" | "NONVISUAL" => {
                let inner = args.first().ok_or_else(|| EvalError::new(format!("{} needs a table filter", spec.name)))?;
                return apply_filter_arg(ev, inner, filters, ctx);
            }
            "TREATAS" => {
                let table = ev.eval_table(args.first().ok_or_else(|| EvalError::new("TREATAS needs a table"))?, ctx)?;
                for (index, column_expr) in args[1..].iter().enumerate() {
                    let column = resolve_column(ev, column_expr)?;
                    let position = index.min(table.columns.len().saturating_sub(1));
                    let keys: BTreeSet<ScalarKey> = table.rows.iter().filter_map(|row| row.get(position)).map(ScalarKey::from).collect();
                    filters.set(column.table, column.column, keys);
                }
                return Ok(());
            }
            _ => {}
        }
    }
    if let BoundExpr::Binary { op: BinaryOp::Eq, left, right } = expr
        && let BoundExpr::Column { table, column } = **left
    {
        let value = ev.eval(right, ctx)?;
        filters.set(table, column, [ScalarKey::from(&value)]);
        return Ok(());
    }
    let table = ev.eval_table(expr, ctx)?;
    apply_table_values(ev, &table, filters);
    Ok(())
}

/// Turns a table filter into column filters. When the table keeps its lineage, its columns are
/// model columns of that table; otherwise a column is matched by name against the model.
fn apply_table_values(ev: &Evaluator<'_>, table: &TableValue, filters: &mut FilterContext) {
    for (position, column) in table.columns.iter().enumerate() {
        let target = match table.lineage {
            Some(lineage) => ev.model().column_index(lineage, &column.name).map(|index| (lineage, index)),
            None => {
                let tables = ev.model().tables_with_column(&column.name);
                if tables.len() == 1 { ev.model().column_index(tables[0], &column.name).map(|index| (tables[0], index)) } else { None }
            }
        };
        let Some((table_index, column_index)) = target else { continue };
        let keys: BTreeSet<ScalarKey> = table.rows.iter().filter_map(|row| row.get(position)).map(ScalarKey::from).collect();
        filters.set(table_index, column_index, keys);
    }
}

fn column_domain(ev: &Evaluator<'_>, column: &ColumnRef, filters: &FilterContext) -> Vec<Scalar> {
    let table = ev.table(column.table, filters);
    let mut seen = BTreeSet::new();
    let mut values: Vec<Scalar> = Vec::new();
    for value in table.column(column.column) {
        if seen.insert(ScalarKey::from(&value)) {
            values.push(value);
        }
    }
    values.sort_by(compare_scalar);
    values
}

fn rollup_selections(dimensions: &[RollupDimension]) -> Result<Vec<Vec<usize>>, EvalError> {
    let mut selections: Vec<Vec<usize>> = vec![Vec::new()];
    for dimension in dimensions {
        let depths = dimension.levels.len() + 1;
        let mut next = Vec::with_capacity(selections.len() * depths);
        for selection in &selections {
            for depth in 0..depths {
                let mut extended = selection.clone();
                extended.push(depth);
                next.push(extended);
            }
        }
        if next.len() > MAX_GROUP_ROWS {
            return Err(EvalError::new("SUMMARIZECOLUMNS has too many rollup combinations"));
        }
        selections = next;
    }
    Ok(selections)
}

fn rolled_columns(dimensions: &[RollupDimension], selection: &[usize]) -> BTreeSet<usize> {
    let mut rolled = BTreeSet::new();
    for (dimension, depth) in dimensions.iter().zip(selection) {
        let keep = dimension.levels.len().saturating_sub(*depth);
        for level in &dimension.levels[keep..] {
            rolled.extend(level.iter().copied());
        }
    }
    rolled
}

fn summarize_columns(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let parsed = parse_summarize_columns(ev, args)?;
    if parsed.columns.is_empty() {
        return Err(EvalError::new("SUMMARIZECOLUMNS needs at least one group-by column"));
    }
    let mut group_filters = transitioned_filters(ctx);
    let mut measure_filters = group_filters.clone();
    for (expr, non_visual) in &parsed.filters {
        apply_filter_arg(ev, expr, &mut group_filters, ctx)?;
        if !*non_visual {
            apply_filter_arg(ev, expr, &mut measure_filters, ctx)?;
        }
    }
    let mut columns: Vec<TableColumn> = parsed.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect();
    for dimension in &parsed.dimensions {
        for (_, name) in &dimension.flags {
            columns.push(TableColumn { name: name.clone() });
        }
    }
    for (name, _, _) in &parsed.expressions {
        columns.push(TableColumn { name: name.clone() });
    }

    let mut rows: Vec<Vec<Scalar>> = Vec::new();
    for selection in rollup_selections(&parsed.dimensions)? {
        let rolled = rolled_columns(&parsed.dimensions, &selection);
        let mut domains: Vec<Vec<Option<Scalar>>> = Vec::with_capacity(parsed.columns.len());
        let mut combinations = 1usize;
        for (index, column) in parsed.columns.iter().enumerate() {
            if rolled.contains(&index) {
                domains.push(vec![None]);
            } else {
                let values = column_domain(ev, column, &group_filters);
                combinations = combinations.saturating_mul(values.len().max(1));
                domains.push(values.into_iter().map(Some).collect());
            }
        }
        if combinations > MAX_GROUP_ROWS {
            return Err(EvalError::new("SUMMARIZECOLUMNS produced too many rows"));
        }
        let mut flags = Vec::new();
        for (dimension, depth) in parsed.dimensions.iter().zip(&selection) {
            let keep = dimension.levels.len().saturating_sub(*depth);
            for (level, _) in &dimension.flags {
                flags.push(Scalar::Boolean(*level >= keep));
            }
        }
        let mut products: Vec<Vec<Option<Scalar>>> = vec![Vec::new()];
        for domain in &domains {
            if domain.is_empty() {
                products.clear();
                break;
            }
            let mut next = Vec::with_capacity(products.len() * domain.len());
            for prefix in &products {
                for value in domain {
                    let mut values = prefix.clone();
                    values.push(value.clone());
                    next.push(values);
                }
            }
            products = next;
        }
        for combination in products {
            let mut filters = measure_filters.clone();
            for (column, value) in parsed.columns.iter().zip(&combination) {
                if let Some(value) = value {
                    filters.set(column.table, column.column, [ScalarKey::from(value)]);
                }
            }
            let mut inner = Context { filters, rows: Vec::new(), vars: ctx.vars.clone() };
            let mut expression_values = Vec::with_capacity(parsed.expressions.len());
            for (_, expression, _) in &parsed.expressions {
                expression_values.push(ev.eval(expression, &mut inner)?);
            }
            let judged: Vec<&Scalar> =
                parsed.expressions.iter().zip(&expression_values).filter(|((_, _, ignore), _)| !*ignore).map(|(_, value)| value).collect();
            if !judged.is_empty() && judged.iter().all(|value| value.is_blank()) {
                continue;
            }
            let mut row = Vec::with_capacity(columns.len());
            for value in &combination {
                row.push(value.clone().unwrap_or(Scalar::Blank));
            }
            row.extend(flags.iter().cloned());
            row.extend(expression_values);
            if rows.len() >= MAX_GROUP_ROWS {
                return Err(EvalError::new("SUMMARIZECOLUMNS produced too many rows"));
            }
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| compare_values(left, right));
    let mut seen: BTreeSet<Vec<ScalarKey>> = BTreeSet::new();
    rows.retain(|row| seen.insert(row.iter().map(ScalarKey::from).collect()));
    Ok(TableValue::new(columns, rows))
}

#[allow(clippy::cognitive_complexity)]
// ADDMISSINGITEMS merges explicit rows, companion columns and the invisible-total filter, so the
// shape it builds reads best in one place.
fn add_missing_items(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.is_empty() {
        return Err(EvalError::new("ADDMISSINGITEMS needs a table"));
    }
    // The table argument is the first argument that is neither a column nor a companion marker.
    let mut table_index = None;
    for (index, arg) in args.iter().enumerate() {
        let column = matches!(arg, BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. });
        let companion = matches!(arg, BoundExpr::Call { spec, .. } if matches!(spec.name, "ROLLUPGROUP" | "ROLLUPISSUBTOTAL"));
        if !column && !companion {
            table_index = Some(index);
            break;
        }
    }
    let Some(table_index) = table_index else {
        return Err(EvalError::new("ADDMISSINGITEMS needs a table argument"));
    };
    let table = ev.eval_table(&args[table_index], ctx)?;
    let mut show_all: Vec<ColumnRef> = Vec::new();
    for arg in &args[..table_index] {
        match arg {
            BoundExpr::Call { spec, args: inner } if spec.name == "ROLLUPGROUP" => {
                for column in inner {
                    show_all.push(resolve_column(ev, column)?);
                }
            }
            other => show_all.push(resolve_column(ev, other)?),
        }
    }
    let mut subtotal: Vec<(ColumnRef, String)> = Vec::new();
    let mut filter_exprs: Vec<&BoundExpr> = Vec::new();
    for arg in &args[table_index + 1..] {
        match arg {
            BoundExpr::Call { spec, args: inner } if spec.name == "ROLLUPISSUBTOTAL" => {
                let mut index = 0usize;
                while index < inner.len() {
                    let column = match &inner[index] {
                        BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } => resolve_column(ev, &inner[index])?,
                        other => {
                            filter_exprs.push(other);
                            index += 1;
                            continue;
                        }
                    };
                    index += 1;
                    let Some(name) = inner.get(index).and_then(|arg| reference_name(ev, arg)) else {
                        return Err(EvalError::new("ROLLUPISSUBTOTAL needs an IsSubtotal column name after each group-by column"));
                    };
                    index += 1;
                    subtotal.push((column, name));
                    if let Some(next) = inner.get(index) {
                        let is_filter =
                            !matches!(next, BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } | BoundExpr::Literal(Literal::String(_)));
                        if is_filter {
                            filter_exprs.push(next);
                            index += 1;
                        }
                    }
                }
            }
            BoundExpr::Call { spec, .. } if spec.name == "ROLLUPGROUP" => {}
            BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. } => {}
            other => filter_exprs.push(other),
        }
    }
    if show_all.is_empty() {
        return Ok(table);
    }
    let mut domain_filters = ctx.filters.clone();
    for expr in &filter_exprs {
        apply_filter_arg(ev, expr, &mut domain_filters, ctx)?;
    }
    let mut domains: Vec<Vec<Scalar>> = Vec::with_capacity(show_all.len());
    let mut combinations = 1usize;
    for column in &show_all {
        let values = column_domain(ev, column, &domain_filters);
        combinations = combinations.saturating_mul(values.len().max(1));
        domains.push(values);
    }
    if combinations > MAX_GROUP_ROWS {
        return Err(EvalError::new("ADDMISSINGITEMS has too many missing items"));
    }
    let positions: Vec<Option<usize>> = show_all.iter().map(|column| table.column_index(&column.name)).collect();
    let mut existing: BTreeSet<Vec<ScalarKey>> = BTreeSet::new();
    for row in &table.rows {
        existing.insert(
            positions.iter().map(|position| position.and_then(|index| row.get(index)).map(ScalarKey::from).unwrap_or(ScalarKey::Blank)).collect(),
        );
    }
    let mut products: Vec<Vec<Scalar>> = vec![Vec::new()];
    for domain in &domains {
        if domain.is_empty() {
            products.clear();
            break;
        }
        let mut next = Vec::with_capacity(products.len() * domain.len());
        for prefix in &products {
            for value in domain {
                let mut values = prefix.clone();
                values.push(value.clone());
                next.push(values);
            }
        }
        products = next;
    }
    let mut missing = Vec::new();
    for values in products {
        let key: Vec<ScalarKey> = values.iter().map(ScalarKey::from).collect();
        if existing.contains(&key) {
            continue;
        }
        let mut row = vec![Scalar::Blank; table.columns.len()];
        for (position, value) in positions.iter().zip(&values) {
            if let Some(position) = position
                && let Some(slot) = row.get_mut(*position)
            {
                *slot = value.clone();
            }
        }
        for (column, flag_name) in &subtotal {
            let Some(position) = table.column_index(flag_name) else { continue };
            let show_all_position = show_all.iter().position(|candidate| candidate.table == column.table && candidate.column == column.column);
            let blank = match show_all_position {
                Some(index) => values.get(index).is_none_or(|value| value.is_blank()),
                None => true,
            };
            if let Some(slot) = row.get_mut(position) {
                *slot = Scalar::Boolean(blank);
            }
        }
        missing.push(row);
    }
    missing.sort_by(|left, right| compare_values(left, right));
    let mut rows = table.rows.clone();
    rows.extend(missing);
    Ok(TableValue::new(table.columns.clone(), rows).with_lineage_if(table.lineage))
}

fn number_arg(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, what: &str) -> Result<f64, EvalError> {
    ev.eval(expr, ctx)?.to_number().ok_or_else(|| EvalError::new(format!("{what} must be numeric")))
}

fn order_is_ascending(expr: &BoundExpr) -> bool {
    match expr {
        BoundExpr::Literal(Literal::Boolean(value)) => *value,
        BoundExpr::Literal(Literal::Integer(value)) => *value != 0,
        BoundExpr::Literal(Literal::Decimal(value)) => *value != 0.0,
        _ => false,
    }
}

fn compare_scores(left: &[Scalar], right: &[Scalar], keys: &[(&BoundExpr, bool)]) -> Ordering {
    for ((left, right), (_, ascending)) in left.iter().zip(right).zip(keys) {
        let Some(ordering) = left.compare(right) else { continue };
        // `Order` defaults to descending: reverse the natural ordering unless ascending is set.
        let ordering = if *ascending { ordering } else { ordering.reverse() };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn top_n_skip(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 3 {
        return Err(EvalError::new("TOPNSKIP needs Rows, Skip and a table"));
    }
    let rows = number_arg(ev, &args[0], ctx, "TOPNSKIP Rows")?;
    let skip = number_arg(ev, &args[1], ctx, "TOPNSKIP Skip")?;
    let table = ev.eval_table(&args[2], ctx)?;
    let rows = if rows <= 0.0 { 0 } else { rows as usize };
    let skip = if skip <= 0.0 { 0 } else { skip as usize };
    if skip > 0 && table.lineage.is_none() {
        return Err(EvalError::new("TOPNSKIP with a non-zero skip needs a table with data lineage"));
    }
    if rows == 0 {
        return Ok(TableValue::new(table.columns.clone(), Vec::new()).with_lineage_if(table.lineage));
    }
    let mut keys: Vec<(&BoundExpr, bool)> = Vec::new();
    let mut index = 3usize;
    while index < args.len() {
        match &args[index] {
            BoundExpr::Literal(Literal::Integer(_)) | BoundExpr::Literal(Literal::Boolean(_)) | BoundExpr::Literal(Literal::Decimal(_)) => {
                if let Some(last) = keys.last_mut() {
                    last.1 = order_is_ascending(&args[index]);
                }
            }
            other => keys.push((other, false)),
        }
        index += 1;
    }
    let mut scored: Vec<(Vec<Scalar>, Vec<Scalar>)> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let scores = keys.iter().map(|(expr, _)| ev.eval(expr, ctx)).collect::<Result<Vec<_>, _>>();
        ctx.rows.pop();
        scored.push((scores?, row.clone()));
    }
    scored.sort_by(|left, right| compare_scores(&left.0, &right.0, &keys));

    // TOPNSKIP returns exactly Rows rows after Skip, in order: unlike TOPN it has no ties
    // parameter and does not expand equal keys (Excel returns the requested count).
    let selected: Vec<Vec<Scalar>> = scored.into_iter().skip(skip).take(rows).map(|(_, row)| row).collect();
    Ok(TableValue::new(table.columns.clone(), selected).with_lineage_if(table.lineage))
}

fn top_n_per_level(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() != 6 {
        return Err(EvalError::new("TOPNPERLEVEL needs Rows, Table, LevelsDefinition, NodesExpanded, LevelsBoundaries and a restart column name"));
    }
    let rows = number_arg(ev, &args[0], ctx, "TOPNPERLEVEL Rows")?;
    let table = ev.eval_table(&args[1], ctx)?;
    let levels_table = ev.eval_table(&args[2], ctx)?;
    let nodes_table = ev.eval_table(&args[3], ctx)?;
    let boundaries_table = ev.eval_table(&args[4], ctx)?;
    let restart_name = ev.eval(&args[5], ctx)?.to_text();
    if restart_name.is_empty() {
        return Err(EvalError::new("TOPNPERLEVEL needs a restart indicator column name"));
    }
    if table.column_index(&restart_name).is_some() {
        return Err(EvalError::new(format!("TOPNPERLEVEL cannot add `{restart_name}`: the column already exists")));
    }
    let rows = if rows <= 0.0 { 0 } else { rows as usize };
    let mut columns = table.columns.clone();
    columns.push(TableColumn { name: restart_name });
    if rows == 0 {
        return Ok(TableValue::new(columns, Vec::new()));
    }

    // LevelsDefinition rows are positional: LevelNumber, TotalColumnName, ColumnName, FlagOrder.
    let mut levels: BTreeMap<i64, LevelDef> = BTreeMap::new();
    for row in &levels_table.rows {
        let Some(level) = row.first().and_then(Scalar::to_number) else {
            return Err(EvalError::new("LevelsDefinition needs a level number in its first column"));
        };
        let Some(column_name) = row.get(2).map(Scalar::to_text) else {
            return Err(EvalError::new("LevelsDefinition needs a column name in its third column"));
        };
        let visible = row.get(3).and_then(Scalar::to_number).is_some_and(|flag| flag != 0.0);
        let definition = levels.entry(level as i64).or_default();
        if visible {
            definition.visible.push(column_name);
        } else {
            definition.order.push(column_name);
        }
    }
    if levels.is_empty() {
        return Err(EvalError::new("TOPNPERLEVEL needs at least one level definition"));
    }

    // NodesExpanded rows: Unknown, LevelNumber, ColumnName, ItemSelected.
    let mut selected: BTreeMap<(i64, String), Vec<Scalar>> = BTreeMap::new();
    for row in &nodes_table.rows {
        let Some(level) = row.get(1).and_then(Scalar::to_number) else { continue };
        let Some(column) = row.get(2).map(Scalar::to_text) else { continue };
        let Some(item) = row.get(3) else { continue };
        selected.entry((level as i64, column.to_ascii_lowercase())).or_default().push(item.clone());
    }

    // LevelsBoundaries rows: Unknown, LevelNumber, ColumnName, BoundaryItemValue, BoundaryType.
    // BoundaryType 2 is the first displayed item, 1 the last displayed item.
    let mut boundaries: BTreeMap<(i64, String), (Option<Scalar>, Option<Scalar>)> = BTreeMap::new();
    for row in &boundaries_table.rows {
        let Some(level) = row.get(1).and_then(Scalar::to_number) else { continue };
        let Some(column) = row.get(2).map(Scalar::to_text) else { continue };
        let Some(value) = row.get(3).cloned() else { continue };
        let Some(kind) = row.get(4).and_then(Scalar::to_number) else { continue };
        let entry = boundaries.entry((level as i64, column.to_ascii_lowercase())).or_default();
        if kind == 1.0 {
            entry.1 = Some(value);
        } else if kind == 2.0 {
            entry.0 = Some(value);
        }
    }

    let mut output: Vec<Vec<Scalar>> = Vec::new();
    for (level_number, definition) in &levels {
        let mut criteria: Vec<(usize, Vec<Scalar>)> = Vec::new();
        for ((level, column), values) in &selected {
            if level != level_number {
                continue;
            }
            if let Some(position) = table.column_index(column) {
                criteria.push((position, values.clone()));
            }
        }
        if criteria.is_empty() {
            continue;
        }
        let limits: Vec<(usize, Option<Scalar>, Option<Scalar>)> = boundaries
            .iter()
            .filter(|((level, _), _)| level == level_number)
            .filter_map(|((_, column), (first, last))| table.column_index(column).map(|position| (position, first.clone(), last.clone())))
            .collect();
        let mut order_positions: Vec<usize> = definition.order.iter().filter_map(|name| table.column_index(name)).collect();
        if order_positions.is_empty() {
            order_positions = definition.visible.iter().filter_map(|name| table.column_index(name)).collect();
        }
        let matches = |row: &[Scalar]| {
            criteria
                .iter()
                .all(|(position, values)| row.get(*position).is_some_and(|value| values.iter().any(|candidate| candidate.strict_eq(value))))
        };
        let mut candidates: Vec<(Vec<Scalar>, Vec<Scalar>)> = Vec::new();
        let mut matching: Vec<(Vec<Scalar>, Vec<Scalar>)> = Vec::new();
        for row in &table.rows {
            if !matches(row) {
                continue;
            }
            let key: Vec<Scalar> = order_positions.iter().filter_map(|position| row.get(*position).cloned()).collect();
            matching.push((key.clone(), row.clone()));
            let inside = limits.iter().all(|(position, first, last)| within_boundaries(row.get(*position), first, last));
            if inside {
                candidates.push((key, row.clone()));
            }
        }
        candidates.sort_by(|left, right| compare_values(&left.0, &right.0));
        matching.sort_by(|left, right| compare_values(&left.0, &right.0));
        let window: Vec<Vec<Scalar>> = candidates.iter().take(rows).map(|(_, row)| row.clone()).collect();
        let start_truncated = window.first().and_then(|first| matching.iter().position(|(_, row)| row == first)).is_some_and(|position| position > 0);
        let end_truncated =
            window.last().and_then(|last| matching.iter().rposition(|(_, row)| row == last)).is_some_and(|position| position + 1 < matching.len());
        for (index, row) in window.iter().enumerate() {
            let mut output_row = row.clone();
            let indicator = if index == 0 && start_truncated {
                Scalar::Integer(1)
            } else if index + 1 == window.len() && end_truncated {
                Scalar::Integer(2)
            } else {
                Scalar::Blank
            };
            output_row.push(indicator);
            output.push(output_row);
        }
        if output.len() >= MAX_GROUP_ROWS {
            return Err(EvalError::new("TOPNPERLEVEL produced too many rows"));
        }
    }
    Ok(TableValue::new(columns, output))
}

fn within_boundaries(value: Option<&Scalar>, first: &Option<Scalar>, last: &Option<Scalar>) -> bool {
    let Some(value) = value else { return false };
    if let Some(first) = first
        && value.compare(first).is_none_or(|ordering| ordering == Ordering::Less)
    {
        return false;
    }
    if let Some(last) = last
        && value.compare(last).is_none_or(|ordering| ordering == Ordering::Greater)
    {
        return false;
    }
    true
}
