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
use super::super::support::{SubtotalGuard, frame_for};
use super::*;
use crate::ast::{BinaryOp, Literal};
use crate::value::{Scalar, ScalarKey, TableColumn, TableValue, folded_key};

/// Hard cap on the rows a grouping function materializes: group sets come from the model and the
/// cartesian product of column domains is unbounded otherwise.
const MAX_GROUP_ROWS: usize = 250_000;

thread_local! {
    /// Group rows pushed by `GROUPBY` while its added expressions are evaluated. `CURRENTGROUP()`
    /// is only valid there; evaluation is single-threaded, so a thread-local stack is enough. The
    /// guard pops on drop, so errors cannot leave a stale group behind.
    static CURRENT_GROUPS: RefCell<Vec<TableValue>> = const { RefCell::new(Vec::new()) };
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
        // `ISSUBTOTAL` reads the rolled-up columns this module publishes, so it ships with
        // `SUMMARIZE` rather than with the other information functions.
        "ISSUBTOTAL" => is_subtotal(ev, args),
        _ => return None,
    };
    Some(result)
}

/// `ISSUBTOTAL ( <column> )`: TRUE on a subtotal row of the enclosing `SUMMARIZE`/`ROLLUP*` call,
/// FALSE on a detail row, and an error outside one — the subtotal context lives beside the
/// evaluator's other row state and only `SUMMARIZE` pushes it.
fn is_subtotal(ev: &Evaluator<'_>, args: &[BoundExpr]) -> Result<Scalar, EvalError> {
    let column = args.first().ok_or_else(|| EvalError::new("ISSUBTOTAL needs a column"))?;
    let column = ev.resolve_column(column).ok_or_else(|| EvalError::new("ISSUBTOTAL needs a column of a table"))?;
    match subtotal_columns() {
        Some(rolled_up) => Ok(Scalar::Boolean(rolled_up.contains(&column))),
        None => Err(EvalError::new("ISSUBTOTAL requires a SUMMARIZE row context")),
    }
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

/// A related table's key column indexed by [`ScalarKey`]. Each bucket keeps the row positions in
/// table order, so a lookup can still apply `strict_eq` and take the first matching row.
type RelatedKeyMap = BTreeMap<ScalarKey, Vec<usize>>;

/// One active relationship that can carry a projected column from the summarized table to the
/// related table, with its key column indices resolved once up front.
struct RelatedLink {
    /// Index of the relationship in the model; it identifies its key map in the cache.
    relationship: usize,
    /// Index of the base key column in the summarized table.
    base_key: usize,
    target_table: usize,
    target_key: usize,
    target_column: usize,
}

/// A group-by column resolved against the summarized table: where a row's value comes from.
struct ColumnAccess {
    name: String,
    /// The model column index when the table has the full model layout for this column.
    direct: Option<usize>,
    /// Active relationships that can reach the column's table, in model order.
    related: Vec<RelatedLink>,
    /// Index of a column of the table with the same name, tried last.
    by_name: Option<usize>,
}

/// Resolves how to read `column` for every row of `table`: the direct model index, the active
/// relationships that reach a related table, and the same-named fallback.
fn column_access(ev: &Evaluator<'_>, table: &TableValue, column: &ColumnRef) -> ColumnAccess {
    // Only a table with the full model layout can be read at the model index; a projection
    // (`VALUES(Sales[Region])`, `ALL(Sales[Region])`) is read by column name instead.
    let direct = (table.lineage == Some(column.table) && table.column_index(&column.name) == Some(column.column)).then_some(column.column);
    ColumnAccess {
        name: column.name.clone(),
        direct,
        related: related_links(ev, table, column.table, column.column),
        by_name: table.column_index(&column.name),
    }
}

/// The active relationships that can supply `target_column` of `target` from a row of `table`.
fn related_links(ev: &Evaluator<'_>, table: &TableValue, target: usize, target_column: usize) -> Vec<RelatedLink> {
    let Some(base) = table.lineage else { return Vec::new() };
    let model = ev.model();
    let mut links = Vec::new();
    for (index, relationship) in model.relationships.iter().enumerate() {
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
        // The table may be a projection that keeps the lineage but not the model layout, so the
        // key is read by name; a projection without the key skips the relationship.
        let Some(base_column) = table.column_index(base_key) else { continue };
        let Some(target_key_column) = model.column_index(target, target_key) else { continue };
        links.push(RelatedLink { relationship: index, base_key: base_column, target_table: target, target_key: target_key_column, target_column });
    }
    links
}

/// The target key column of a relationship indexed by [`ScalarKey`]. The map is built once per
/// relationship and reused for every source row and every projected column of the same table.
fn related_key_map(model: &crate::model::DataModel, link: &RelatedLink) -> RelatedKeyMap {
    let mut map: RelatedKeyMap = BTreeMap::new();
    let Some(target_table) = model.tables.get(link.target_table) else { return map };
    for (index, target_row) in target_table.rows.iter().enumerate() {
        if let Some(value) = target_row.get(link.target_key) {
            map.entry(ScalarKey::from(value)).or_default().push(index);
        }
    }
    map
}

/// The value of a group-by column for one row: the row itself when the column belongs to the
/// table, a related-table value when the column is on a table reachable through a relationship,
/// otherwise a column with the same name (SUMMARIZE accepts columns of the summarized table and
/// of its related tables; derived tables keep their columns by name).
fn group_value(ev: &Evaluator<'_>, row: &[Scalar], access: &ColumnAccess, cache: &mut BTreeMap<usize, RelatedKeyMap>) -> Result<Scalar, EvalError> {
    if let Some(index) = access.direct {
        return row.get(index).cloned().ok_or_else(|| EvalError::new("row is out of range"));
    }
    if !access.related.is_empty() {
        // A fact row no relationship matches (blank or orphan foreign key) belongs to the related
        // table's blank member, as the pivot path already reports; never fall back to a same-named
        // base column, which would group the row under its own key.
        return Ok(related_value(ev, row, &access.related, cache).unwrap_or(Scalar::Blank));
    }
    if let Some(index) = access.by_name {
        return row.get(index).cloned().ok_or_else(|| EvalError::new("row is out of range"));
    }
    Err(EvalError::new(format!("column [{}] is not available in this table", access.name)))
}

/// Follows the resolved active relationships for one row, reusing the key map each relationship
/// built on first use. The relationships are tried in model order and the map's bucket is
/// re-checked with `strict_eq`, matching the original row-by-row linear scan.
fn related_value(ev: &Evaluator<'_>, row: &[Scalar], links: &[RelatedLink], cache: &mut BTreeMap<usize, RelatedKeyMap>) -> Option<Scalar> {
    let model = ev.model();
    for link in links {
        let Some(key) = row.get(link.base_key) else { continue };
        let map = cache.entry(link.relationship).or_insert_with(|| related_key_map(model, link));
        let Some(indices) = map.get(&ScalarKey::from(key)) else { continue };
        let Some(target_table) = model.tables.get(link.target_table) else { continue };
        for index in indices {
            let Some(target_row) = target_table.rows.get(*index) else { continue };
            if target_row.get(link.target_key).is_some_and(|value| value.strict_eq(key)) {
                return target_row.get(link.target_column).cloned();
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

/// The key of a group, subtotal prefix or domain candidate: text folds to the model's
/// case-insensitive collation, the rule `VALUES`, `DISTINCTCOUNT` and the filter sets already
/// follow. Without it a column holding `"East"` and `"east"` groups twice, and each group's
/// filters then select both spellings.
fn group_key(values: &[Scalar]) -> Vec<ScalarKey> {
    values.iter().map(|value| folded_key(&ScalarKey::from(value))).collect()
}

/// Groups the rows of `table` by `columns`; each entry holds the group values and the source rows.
#[allow(clippy::type_complexity)]
// Each entry pairs the group's key values with the rows that fall in it.
fn group_details(ev: &Evaluator<'_>, table: &TableValue, columns: &[ColumnRef]) -> Result<Vec<(Vec<Scalar>, Vec<Vec<Scalar>>)>, EvalError> {
    // Resolve every column and the related-table key maps once, then read them per row instead of
    // re-resolving relationships and scanning the related table for each row.
    let accesses: Vec<ColumnAccess> = columns.iter().map(|column| column_access(ev, table, column)).collect();
    let mut related_keys: BTreeMap<usize, RelatedKeyMap> = BTreeMap::new();
    let mut groups: BTreeMap<Vec<ScalarKey>, (Vec<Scalar>, Vec<Vec<Scalar>>)> = BTreeMap::new();
    for row in &table.rows {
        let mut values = Vec::with_capacity(accesses.len());
        for access in &accesses {
            values.push(group_value(ev, row, access, &mut related_keys)?);
        }
        let key: Vec<ScalarKey> = group_key(&values);
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
fn subtotal_rows(details: &[(Vec<Scalar>, Vec<Vec<Scalar>>)], layout: &GroupLayout) -> Vec<SummaryRow> {
    let count = layout.levels.len();
    let mut rows: Vec<SummaryRow> = Vec::new();
    let mut seen: BTreeSet<(Vec<ScalarKey>, Vec<bool>)> = BTreeSet::new();
    for depth in 1..=count {
        let rolled: BTreeSet<usize> = layout.levels[count - depth..].iter().flat_map(|(start, len)| *start..start + len).collect();
        let mut active = vec![true; layout.columns.len()];
        for index in &rolled {
            if let Some(slot) = active.get_mut(*index) {
                *slot = false;
            }
        }
        // One pass over the details per depth: every group contributes its source rows to the
        // bucket of its own subtotal prefix, so no subtotal rescans the whole detail list.
        let mut buckets: BTreeMap<Vec<ScalarKey>, usize> = BTreeMap::new();
        let mut collected: Vec<(Vec<Scalar>, Vec<Vec<Scalar>>)> = Vec::new();
        for (values, group_rows) in details {
            let mut subtotal = values.clone();
            for index in &rolled {
                if let Some(value) = subtotal.get_mut(*index) {
                    *value = Scalar::Blank;
                }
            }
            let prefix: Vec<ScalarKey> = group_key(&subtotal);
            if let Some(position) = buckets.get(&prefix).copied() {
                if let Some((_, covered)) = collected.get_mut(position) {
                    covered.extend(group_rows.iter().cloned());
                }
            } else {
                buckets.insert(prefix, collected.len());
                collected.push((subtotal, group_rows.clone()));
            }
        }
        for (subtotal, covered) in collected {
            let key = (group_key(&subtotal), active.clone());
            if seen.insert(key) {
                rows.push((subtotal, active.clone(), covered));
            }
        }
    }
    rows
}

/// One summarised row: its group values, which columns stay active, and the table-argument rows
/// the group covers (a rollup subtotal covers the union of its groups).
type SummaryRow = (Vec<Scalar>, Vec<bool>, Vec<Vec<Scalar>>);

struct GroupContext<'a> {
    columns: &'a [ColumnRef],
    values: &'a [Scalar],
    active: &'a [bool],
    source: &'a TableValue,
    rows: &'a [Vec<Scalar>],
}

#[cfg(test)]
thread_local! {
    /// Groups prepared in this thread. `prepare_group` is group-scoped work, so a row with several
    /// expressions must prepare once: the counter lets a test pin that.
    static PREPARED_GROUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Lineage tables materialised for the group leak check in this thread. The table depends only
    /// on the evaluation's own filters, so it must be built once per evaluation — not once per
    /// group, and not once per group × expression, which is what the per-row path used to do.
    static LINEAGE_MATERIALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Builds the filter context of one group: the table argument's own rows, then one value set per
/// active group-by column, then the leak check that a cross-column predicate cannot express.
/// The result depends only on the group, so callers compute it once and evaluate every
/// expression of that row against it. The second value is the row's grouping scope: the
/// active group-by columns, which is what `ISINSCOPE` reports for that row.
///
/// `base` carries the lineage table the leak check materialises. It is built on the first group
/// that needs it and reused by every later group of the same evaluation, so the group only pays
/// for the rows it narrows.
fn prepare_group(
    ev: &Evaluator<'_>,
    ctx: &Context,
    group: &GroupContext<'_>,
    operation: &str,
    base: &mut Option<LineageBase>,
) -> Result<(FilterContext, Vec<(usize, usize)>), EvalError> {
    let mut filters = ctx.filters.clone();
    #[cfg(test)]
    PREPARED_GROUPS.with(|count| count.set(count.get() + 1));
    // The value sets this group installs on top of the evaluation's own filters. They are the
    // group-dependent part of the leak check: the evaluation's filters are already applied to
    // `base`, so only these narrow it.
    let mut group_sets: Vec<(usize, usize, BTreeSet<ScalarKey>)> = Vec::new();
    // The table argument's own rows bound the group: `SUMMARIZE(FILTER(...), ...)` must not see
    // rows the filter removed. Applying the group's rows as column filters keeps the lineage
    // contract the evaluator already relies on.
    if let Some(lineage) = group.source.lineage
        && !group.rows.is_empty()
    {
        let rows = TableValue::new(group.source.columns.clone(), group.rows.to_vec()).with_lineage(lineage);
        group_sets.extend(apply_table_values(ev, &rows, &mut filters));
    }
    for ((column, value), active) in group.columns.iter().zip(group.values).zip(group.active) {
        if !*active {
            continue;
        }
        // A group column of a related table can be BLANK for two different reasons: the related
        // table has a blank member, or the fact row's key matches nothing (an orphan). Filtering
        // the related column to BLANK would drop the orphan rows, so that group is left to the
        // table argument's own rows, which already select exactly the fact rows it covers.
        // The column is the source table's own when it belongs to the source's lineage; a name
        // match is not enough, because a star schema often names a fact column after a dimension
        // column (`Sales[Product]` and `Products[Product]`).
        let on_source = match group.source.lineage {
            Some(lineage) => column.table == lineage,
            None => group.source.columns.iter().any(|candidate| candidate.name.eq_ignore_ascii_case(&column.name)),
        };
        if value.is_blank() && !on_source {
            continue;
        }
        let keys: BTreeSet<ScalarKey> = std::iter::once(folded_key(&ScalarKey::from(value))).collect();
        filters.set(column.table, column.column, keys.clone());
        group_sets.push((column.table, column.column, keys));
    }
    // One value set per column describes the group exactly only when the table argument's rows
    // are the ones those sets select. A cross-column predicate (`FILTER(Sales, Sales[Amount] >
    // Sales[Cost])`) cannot be expressed that way: each column's set independently admits rows
    // the predicate removed, so the group would aggregate rows it does not own. Comparing the
    // lineage rows under the final filters against the group's own rows catches that leak instead
    // of returning a silent wrong answer. The comparison is by identity, not by count: forms that
    // deduplicate while keeping the lineage (`VALUES`, `DISTINCT`, column-form `ALL`) legitimately
    // own fewer rows than the lineage has, so only a lineage row that projects to a column
    // combination the group does not own is a leak.
    if let Some(lineage) = group.source.lineage
        && !group.rows.is_empty()
    {
        // The table argument may be a projection that reorders or drops columns, so its columns
        // are matched to model columns by name. A column the model does not know cannot
        // participate in the comparison.
        let mapped: Vec<(usize, usize)> = group
            .source
            .columns
            .iter()
            .enumerate()
            .filter_map(|(position, column)| ev.model().column_index(lineage, &column.name).map(|index| (position, index)))
            .collect();
        if !mapped.is_empty() {
            let owned: BTreeSet<Vec<ScalarKey>> = group
                .rows
                .iter()
                .map(|row| group_key(&mapped.iter().map(|(position, _)| row.get(*position).cloned().unwrap_or(Scalar::Blank)).collect::<Vec<_>>()))
                .collect();
            // The evaluation's own filters are already applied to the base table, so the group
            // narrows its rows in memory. A set on another table reaches the lineage through a
            // relationship, which only the evaluator can resolve: that group materialises the
            // table with its own filters (the only genuinely group-dependent rebuild). A set that
            // replaces an outer filter with values it does not already admit must rebuild too:
            // `filters.set` replaces that filter, while narrowing would AND the group value with
            // the outer one baked into the base rows and compare an empty set, hiding the leak
            // this check exists to catch. A set the outer filter already admits narrows exactly,
            // so the common `CALCULATE(SUMMARIZE(...), Sales[Region] = "East")` case still
            // rebuilds nothing.
            let replaces_outer = group_sets
                .iter()
                .any(|(table, column, keys)| ctx.filters.filters.get(&(*table, *column)).is_some_and(|outer| !keys.is_subset(outer)));
            let narrowed = !replaces_outer && group_sets.iter().all(|(table, _, _)| *table == lineage);
            let lineage_rows = if narrowed {
                base.get_or_insert_with(|| LineageBase::new(ev, lineage, &ctx.filters)).matching(&group_sets)
            } else {
                ev.table(lineage, &filters).rows
            };
            let leaked = lineage_rows.iter().any(|row| {
                let key = group_key(&mapped.iter().map(|(_, index)| row.get(*index).cloned().unwrap_or(Scalar::Blank)).collect::<Vec<_>>());
                !owned.contains(&key)
            });
            if leaked {
                return Err(EvalError::new(format!(
                    "{operation} cannot evaluate the table argument: the group's column filters select rows outside the group"
                )));
            }
        }
    }
    // `ISINSCOPE` reports the group-by columns the row actually groups on, so a rolled-up column
    // leaves the scope with its `active` flag.
    let scope: Vec<(usize, usize)> =
        group.columns.iter().zip(group.active).filter(|(_, active)| **active).map(|(column, _)| (column.table, column.column)).collect();
    Ok((filters, scope))
}

/// Evaluates one expression of a summarised row against the filters and grouping scope
/// `prepare_group` built once for that row.
fn eval_in_group(
    ev: &Evaluator<'_>,
    ctx: &Context,
    expression: &BoundExpr,
    group_filters: &FilterContext,
    grouping_scope: &[(usize, usize)],
) -> Result<Scalar, EvalError> {
    let mut inner =
        Context { filters: group_filters.clone(), rows: Vec::new(), vars: ctx.vars.clone(), grouping_scope: Some(grouping_scope.to_vec()) };
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
    let mut rows: Vec<SummaryRow> =
        details.iter().map(|(values, group_rows)| (values.clone(), vec![true; layout.columns.len()], group_rows.clone())).collect();
    if !layout.levels.is_empty() {
        rows.extend(subtotal_rows(&details, &layout));
    }
    rows.sort_by(|left, right| compare_values(&left.0, &right.0));
    let mut seen: BTreeSet<(Vec<ScalarKey>, Vec<bool>)> = BTreeSet::new();
    rows.retain(|(values, active, _)| seen.insert((group_key(values), active.clone())));

    let mut columns: Vec<TableColumn> = layout.columns.iter().map(|column| TableColumn { name: column.name.clone() }).collect();
    for (name, _) in &parsed.expressions {
        columns.push(TableColumn { name: name.clone() });
    }
    let mut output = Vec::with_capacity(rows.len());
    // One lineage table for the whole evaluation: every group's leak check narrows these rows
    // instead of rebuilding the table (`prepare_group` builds it on the first group that needs it).
    let mut lineage_base: Option<LineageBase> = None;
    for (values, active, group_rows) in rows {
        let mut row = values.clone();
        // Detail rows have every column active; a ROLLUP row deactivates the rolled-up columns.
        let rolled_up: Vec<(usize, usize)> =
            layout.columns.iter().zip(&active).filter(|(_, active)| !**active).map(|(column, _)| (column.table, column.column)).collect();
        {
            let _guard = SubtotalGuard::push(rolled_up);
            if !parsed.expressions.is_empty() {
                let group = GroupContext { columns: &layout.columns, values: &values, active: &active, source: &table, rows: &group_rows };
                let (group_filters, grouping_scope) = prepare_group(ev, ctx, &group, "SUMMARIZE", &mut lineage_base)?;
                for (_, expression) in &parsed.expressions {
                    row.push(eval_in_group(ev, ctx, expression, &group_filters, &grouping_scope)?);
                }
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
    let mut rows = Vec::with_capacity(details.len());
    for (values, group_rows) in &details {
        let current = TableValue::new(table.columns.clone(), group_rows.clone()).with_lineage_if(table.lineage);
        let mut row = values.clone();
        {
            let _guard = CurrentGroupGuard::push(current);
            // `GROUPBY` does not perform an implicit `CALCULATE` for its extension columns: they
            // iterate `CURRENTGROUP()`, so the outer filter context is what they see. Only the
            // grouping scope travels with the row.
            if !parsed.expressions.is_empty() {
                let grouping_scope: Vec<(usize, usize)> = columns.iter().map(|column| (column.table, column.column)).collect();
                let group_filters = ctx.filters.clone();
                for (_, expression) in &parsed.expressions {
                    row.push(eval_in_group(ev, ctx, expression, &group_filters, &grouping_scope)?);
                }
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
                    if inner.is_empty() {
                        return Err(EvalError::new("KEEPFILTERS needs a filter"));
                    }
                    // Keep the wrapper: `apply_filter_arg` intersects a KEEPFILTERS filter with
                    // the filters already in context instead of replacing them.
                    filters.push((arg, false));
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

fn apply_filter_arg(ev: &Evaluator<'_>, expr: &BoundExpr, filters: &mut FilterContext, ctx: &mut Context) -> Result<(), EvalError> {
    if let BoundExpr::Call { spec, args } = expr {
        match spec.name {
            "NONVISUAL" => {
                let inner = args.first().ok_or_else(|| EvalError::new(format!("{} needs a table filter", spec.name)))?;
                return apply_filter_arg(ev, inner, filters, ctx);
            }
            "KEEPFILTERS" => {
                // KEEPFILTERS intersects its inner filter with the filters already in context.
                let inner = args.first().ok_or_else(|| EvalError::new("KEEPFILTERS needs a table filter"))?;
                let mut applied = filters.clone();
                apply_filter_arg(ev, inner, &mut applied, ctx)?;
                for (key, values) in applied.filters {
                    let merged = match filters.filters.get(&key) {
                        Some(existing) => existing.intersection(&values).cloned().collect(),
                        None => values,
                    };
                    filters.filters.insert(key, merged);
                }
                return Ok(());
            }
            "TREATAS" => {
                let table = ev.eval_table(args.first().ok_or_else(|| EvalError::new("TREATAS needs a table"))?, ctx)?;
                // One target column per source column; more targets would silently reuse the last.
                if table.columns.len() != args.len().saturating_sub(1) {
                    return Err(EvalError::new("TREATAS needs one target column per table column"));
                }
                for (index, column_expr) in args[1..].iter().enumerate() {
                    let column = resolve_column(ev, column_expr)?;
                    let position = index;
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
        // A column predicate follows DAX `=` semantics (numeric across types, text ignoring
        // case), so every stored value that compares equal is a candidate key; one key would miss
        // a Decimal column holding `10.0` for `= 10` and an `"East"` value for `= "east"`.
        let keys = ev
            .table(table, &FilterContext::default())
            .column(column)
            .into_iter()
            .filter(|candidate| candidate.compare(&value) == Some(Ordering::Equal))
            .map(|candidate| ScalarKey::from(&candidate));
        filters.set(table, column, keys);
        return Ok(());
    }
    let table = ev.eval_table(expr, ctx)?;
    apply_table_values(ev, &table, filters);
    Ok(())
}

/// Turns a table filter into column filters. When the table keeps its lineage, its columns are
/// model columns of that table; otherwise a column is matched by name against the model.
///
/// Returns the value sets it installed, folded exactly as [`FilterContext::set`] stores them, so a
/// caller that has already materialised the filtered table can narrow its rows with them in memory
/// instead of rebuilding the table.
fn apply_table_values(ev: &Evaluator<'_>, table: &TableValue, filters: &mut FilterContext) -> Vec<(usize, usize, BTreeSet<ScalarKey>)> {
    let mut applied = Vec::new();
    for (position, column) in table.columns.iter().enumerate() {
        let target = match table.lineage {
            Some(lineage) => ev.model().column_index(lineage, &column.name).map(|index| (lineage, index)),
            None => {
                let tables = ev.model().tables_with_column(&column.name);
                if tables.len() == 1 { ev.model().column_index(tables[0], &column.name).map(|index| (tables[0], index)) } else { None }
            }
        };
        let Some((table_index, column_index)) = target else { continue };
        let keys: BTreeSet<ScalarKey> =
            table.rows.iter().filter_map(|row| row.get(position)).map(|value| folded_key(&ScalarKey::from(value))).collect();
        filters.set(table_index, column_index, keys.clone());
        applied.push((table_index, column_index, keys));
    }
    applied
}

/// The lineage rows of the table argument under the evaluation's own filters, materialised once
/// per `SUMMARIZE` evaluation.
///
/// The group leak check needs the lineage rows the group's filters select. Those filters are the
/// evaluation's own plus the group's value sets, so the table is built once and the group only
/// narrows its rows in memory. Rebuilding it inside the group loop cost `groups × lineage rows`
/// (and `groups × expressions × rows` before `prepare_group` hoisted the per-group work).
struct LineageBase {
    lineage: usize,
    rows: TableValue,
}

impl LineageBase {
    fn new(ev: &Evaluator<'_>, lineage: usize, filters: &FilterContext) -> Self {
        #[cfg(test)]
        LINEAGE_MATERIALIZATIONS.with(|count| count.set(count.get() + 1));
        Self { lineage, rows: ev.table(lineage, filters) }
    }

    /// The base rows matching every group value set: a column with several keys is an OR and
    /// different columns are ANDed, exactly as a [`FilterContext`] reads them. Only a matching row
    /// is cloned, so a group pays for its own rows rather than for the whole lineage.
    fn matching(&self, sets: &[(usize, usize, BTreeSet<ScalarKey>)]) -> Vec<Vec<Scalar>> {
        self.rows
            .rows
            .iter()
            .filter(|row| {
                sets.iter().all(|(table, column, keys)| {
                    *table == self.lineage
                        && keys.contains(&row.get(*column).map(|value| folded_key(&ScalarKey::from(value))).unwrap_or(ScalarKey::Blank))
                })
            })
            .cloned()
            .collect()
    }
}

fn column_domain(ev: &Evaluator<'_>, column: &ColumnRef, filters: &FilterContext) -> Vec<Scalar> {
    let table = ev.table(column.table, filters);
    let mut seen = BTreeSet::new();
    let mut values: Vec<Scalar> = Vec::new();
    for value in table.column(column.column) {
        if seen.insert(folded_key(&ScalarKey::from(&value))) {
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
    let mut group_filters = ev.transitioned_filters(ctx);
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

    // The auto-exist rule asks, for every candidate combination, whether the contributing table
    // still has a row. Scanning the table per combination is quadratic in the domain sizes, so
    // when every group column belongs to one table that table is materialized once under the
    // measure filters and the projections of its group values are looked up instead. Multi-table
    // layouts keep the per-combination scan: there a filter on one table can remove rows from
    // another over a relationship, which a per-table projection cannot model.
    // Whether an expression decides visibility only depends on the IGNORE markers, so the answer
    // is the same for every combination.
    let auto_exist_only = parsed.expressions.iter().all(|(_, _, ignore)| *ignore);
    let single_table = if auto_exist_only {
        let mut tables = parsed.columns.iter().map(|column| column.table);
        match tables.next() {
            Some(first) if tables.all(|table| table == first) => Some(first),
            _ => None,
        }
    } else {
        None
    };
    let auto_exist_rows = single_table.and_then(|table| {
        let width = ev.model().tables.get(table).map_or(0, |model_table| model_table.columns.len());
        let rows = ev.table(table, &measure_filters).rows;
        // The row-scan filter treats a missing trailing cell as matching every filter, which a
        // projection key cannot express; such (ragged) models keep the per-combination check.
        rows.iter().all(|row| row.len() >= width).then_some(rows)
    });

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
        // For a single-table layout this selection's existing group values are hashed once; the
        // per-combination check below then only looks a key up. The empty projection is the grand
        // total, which exists whenever the table has a row under the measure filters.
        let auto_exist = match (auto_exist_only, &auto_exist_rows) {
            (true, Some(table_rows)) => {
                let varying: Vec<usize> = (0..parsed.columns.len()).filter(|index| !rolled.contains(index)).collect();
                let mut projections: BTreeSet<Vec<ScalarKey>> = BTreeSet::new();
                for row in table_rows {
                    // The domains above keep the first spelling of a text value, and
                    // `FilterContext::set` folds filter keys to the model collation, so the
                    // projections must fold too: otherwise a candidate built from the kept
                    // spelling misses a projection stored under another spelling.
                    projections.insert(
                        varying
                            .iter()
                            .map(|&index| {
                                row.get(parsed.columns[index].column).map(|value| folded_key(&ScalarKey::from(value))).unwrap_or(ScalarKey::Blank)
                            })
                            .collect(),
                    );
                }
                Some((varying, projections))
            }
            _ => None,
        };
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
            // `SUMMARIZECOLUMNS` groups on its group-by columns for every expression of the row.
            let grouping_scope: Vec<(usize, usize)> = parsed.columns.iter().map(|column| (column.table, column.column)).collect();
            let mut inner = Context { filters, rows: Vec::new(), vars: ctx.vars.clone(), grouping_scope: Some(grouping_scope) };
            let mut expression_values = Vec::with_capacity(parsed.expressions.len());
            for (_, expression, _) in &parsed.expressions {
                expression_values.push(ev.eval(expression, &mut inner)?);
            }
            let judged: Vec<&Scalar> =
                parsed.expressions.iter().zip(&expression_values).filter(|((_, _, ignore), _)| !*ignore).map(|(_, value)| value).collect();
            if judged.is_empty() {
                // No measure decides visibility: keep only combinations that exist in the data,
                // which is the auto-exist rule for columns of one table.
                match &auto_exist {
                    Some((varying, projections)) => {
                        let key: Vec<ScalarKey> = varying
                            .iter()
                            .map(|&index| {
                                combination
                                    .get(index)
                                    .and_then(|value| value.as_ref())
                                    .map(|value| folded_key(&ScalarKey::from(value)))
                                    .unwrap_or(ScalarKey::Blank)
                            })
                            .collect();
                        if !projections.contains(&key) {
                            continue;
                        }
                    }
                    None => {
                        let tables: BTreeSet<usize> = parsed.columns.iter().map(|column| column.table).collect();
                        if tables.iter().any(|table| ev.table(*table, &inner.filters).rows.is_empty()) {
                            continue;
                        }
                    }
                }
            } else if judged.iter().all(|value| value.is_blank()) {
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
    // The flag columns say which rows are subtotals; a subtotal blanks its rolled-up column, so
    // keying on the show-all values alone would hide a real blank member of that column.
    let flag_positions: Vec<usize> = subtotal.iter().filter_map(|(_, name)| table.column_index(name)).collect();
    let mut existing: BTreeSet<Vec<ScalarKey>> = BTreeSet::new();
    for row in &table.rows {
        if flag_positions.iter().any(|position| row.get(*position) == Some(&Scalar::Boolean(true))) {
            continue;
        }
        existing.insert(
            positions
                .iter()
                .map(|position| {
                    position.and_then(|index| row.get(index)).map(|value| folded_key(&ScalarKey::from(value))).unwrap_or(ScalarKey::Blank)
                })
                .collect(),
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
        let key: Vec<ScalarKey> = group_key(&values);
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
        // An added missing item is a detail row, never a subtotal.
        for position in &flag_positions {
            if let Some(slot) = row.get_mut(*position) {
                *slot = Scalar::Boolean(false);
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
        // `Scalar::compare` returns `None` for incomparable values (mixed text and numbers, NaN);
        // a key with those must still produce a stable order, so fall back to a total order.
        let ordering = super::super::support::total_order(left, right);
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
            // The binder turns a bare `ASC`/`DESC` into a keyword. Only an order slot takes one:
            // a string in an order-by position stays a sort key, which is why this matches
            // `Keyword` and not `Literal::String`.
            BoundExpr::Keyword(word) if keys.last().is_some() && (word.eq_ignore_ascii_case("ASC") || word.eq_ignore_ascii_case("DESC")) => {
                if let Some(last) = keys.last_mut() {
                    last.1 = word.eq_ignore_ascii_case("ASC");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Relationship;
    use crate::model::{DataModel, ModelColumn, ModelTable};
    use crate::{Binder, parse_expression};

    const ROWS: usize = 12_000;
    const REGIONS: usize = 50;
    // `ROWS / REGIONS` products, so every row is a distinct (Region, Product) pair.
    const PRODUCTS: usize = ROWS / REGIONS;

    /// `SUMMARIZECOLUMNS` without a measure keeps only combinations that exist in the data. That
    /// check must not rescan the source table per candidate combination: the fixture has
    /// `REGIONS * PRODUCTS` combinations, so a per-combination scan would serve tens of millions
    /// of rows from storage.
    /// The grouping scope a summarised row carries is its active group-by columns: `ISINSCOPE`
    /// reports those and nothing else, so a rolled-up column leaves the scope.
    #[test]
    fn summarised_rows_scope_only_their_active_group_by_columns() {
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::String("Widget".into()), Scalar::Integer(10)],
                    vec![Scalar::String("West".into()), Scalar::String("Gadget".into()), Scalar::Integer(20)],
                ],
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let ctx = Context { filters: FilterContext::default(), rows: Vec::new(), vars: Vec::new(), grouping_scope: None };
        let columns = vec![ColumnRef { table: 0, column: 0, name: "Region".into() }, ColumnRef { table: 0, column: 1, name: "Product".into() }];
        let values = vec![Scalar::String("East".into()), Scalar::String("Widget".into())];
        // A ROLLUP row keeps Region and rolls Product up.
        let active = vec![true, false];
        let source = evaluator.table(0, &FilterContext::default());
        let group = GroupContext { columns: &columns, values: &values, active: &active, source: &source, rows: &source.rows };
        let (_, scope) = prepare_group(&evaluator, &ctx, &group, "SUMMARIZE", &mut None).expect("prepare");
        assert_eq!(scope, vec![(0, 0)], "only the active group-by column is in scope");
    }

    /// A summarised row with two expressions prepares its group once, not once per expression.
    #[test]
    fn summarize_prepares_each_group_once() {
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(10)],
                    vec![Scalar::String("East".into()), Scalar::Integer(20)],
                    vec![Scalar::String("West".into()), Scalar::Integer(30)],
                ],
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = parse_expression("SUMMARIZE(Sales, Sales[Region], \"A\", SUM(Sales[Amount]), \"B\", COUNTROWS(Sales))").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        PREPARED_GROUPS.with(|count| count.set(0));
        let table = evaluator.evaluate_table(&bound).expect("evaluate");
        assert_eq!(table.rows.len(), 2, "one row per region");
        let prepared = PREPARED_GROUPS.with(|count| count.get());
        assert_eq!(prepared, 2, "one preparation per group, not per expression (4 would be per expression)");
    }

    /// The group leak check reads the lineage table once per evaluation, not once per group: the
    /// table depends only on the evaluation's own filters, so each group narrows those rows in
    /// memory. Rebuilding it per group cost `groups × lineage rows`, and per group × expression
    /// before `prepare_group` hoisted the per-group work.
    #[test]
    fn summarize_materializes_the_lineage_once_for_every_group() {
        let regions = ["East", "West", "North", "South", "Centre"];
        let mut rows = Vec::new();
        for region in regions {
            for amount in 1..=4 {
                rows.push(vec![Scalar::String(region.into()), Scalar::Integer(amount)]);
            }
        }
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                rows,
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = parse_expression("SUMMARIZE(Sales, Sales[Region], \"A\", SUM(Sales[Amount]), \"B\", COUNTROWS(Sales))").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        LINEAGE_MATERIALIZATIONS.with(|count| count.set(0));
        let table = evaluator.evaluate_table(&bound).expect("evaluate");
        assert_eq!(table.rows.len(), regions.len(), "one row per region");
        let materialized = LINEAGE_MATERIALIZATIONS.with(|count| count.get());
        assert_eq!(
            materialized,
            1,
            "one lineage table per evaluation, not per group ({}) or per group x expression ({})",
            regions.len(),
            regions.len() * 2
        );
    }

    /// A group value set that the evaluation's own filters already admit is not a replacement:
    /// `filters.set` would store the same set, so the group keeps narrowing the one lineage table
    /// instead of rebuilding it. Only a set that drops values the outer filter admits rebuilds.
    #[test]
    fn summarize_narrows_when_the_group_set_stays_inside_the_outer_filter() {
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(10)],
                    vec![Scalar::String("East".into()), Scalar::Integer(20)],
                    vec![Scalar::String("West".into()), Scalar::Integer(30)],
                ],
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr =
            parse_expression("CALCULATETABLE(SUMMARIZE(Sales, Sales[Region], \"A\", SUM(Sales[Amount])), Sales[Region] = \"East\")").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        LINEAGE_MATERIALIZATIONS.with(|count| count.set(0));
        let table = evaluator.evaluate_table(&bound).expect("evaluate");
        assert_eq!(table.rows.len(), 1, "the outer filter leaves one group");
        assert_eq!(
            LINEAGE_MATERIALIZATIONS.with(|count| count.get()),
            1,
            "the group set the outer filter already admits still narrows one lineage table"
        );
    }

    /// The leak check still rejects a table argument whose column filters select rows the
    /// cross-column predicate removed — the narrowing above must not weaken it.
    #[test]
    fn summarize_rejects_a_cross_column_table_argument() {
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Cost".into(), data_type: Some("Int64".into()) },
                ],
                // `(6, 7)` fails `Amount > Cost`, but its two values each appear in a row that
                // passes, so the per-column sets of the group admit it.
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(6), Scalar::Integer(5)],
                    vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(7)],
                    vec![Scalar::String("East".into()), Scalar::Integer(6), Scalar::Integer(7)],
                ],
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr =
            parse_expression("SUMMARIZE(FILTER(Sales, Sales[Amount] > Sales[Cost]), Sales[Region], \"A\", SUM(Sales[Amount]))").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        let error = evaluator.evaluate_table(&bound).expect_err("the cross-column leak must be rejected");
        assert!(error.to_string().contains("select rows outside the group"), "{error}");
    }

    /// A group column of a related table is group-dependent through a relationship, so that group
    /// still asks the evaluator to filter the lineage; the rows it summarizes stay correct.
    #[test]
    fn summarize_groups_a_related_table_column() {
        let model = DataModel {
            tables: vec![
                ModelTable {
                    name: "Sales".into(),
                    columns: vec![
                        ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                        ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                    ],
                    rows: vec![
                        vec![Scalar::String("A".into()), Scalar::Integer(10)],
                        vec![Scalar::String("A".into()), Scalar::Integer(20)],
                        vec![Scalar::String("B".into()), Scalar::Integer(30)],
                    ],
                },
                ModelTable {
                    name: "Products".into(),
                    columns: vec![
                        ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                        ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                    ],
                    rows: vec![
                        vec![Scalar::String("A".into()), Scalar::String("Food".into())],
                        vec![Scalar::String("B".into()), Scalar::String("Drink".into())],
                    ],
                },
            ],
            relationships: vec![Relationship {
                from_table: "Sales".into(),
                from_column: "Product".into(),
                to_table: "Products".into(),
                to_column: "Product".into(),
                bidirectional: false,
                inactive: false,
            }],
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = parse_expression("SUMMARIZE(Sales, Products[Category], \"Total\", SUM(Sales[Amount]))").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        let table = evaluator.evaluate_table(&bound).expect("evaluate");
        let rows: Vec<(Scalar, Scalar)> = table.rows.iter().map(|row| (row[0].clone(), row[1].clone())).collect();
        assert_eq!(rows, vec![(Scalar::String("Drink".into()), Scalar::Decimal(30.0)), (Scalar::String("Food".into()), Scalar::Decimal(30.0))]);
    }

    #[test]
    fn summarize_columns_auto_exist_scans_the_group_table_once() {
        let mut rows = Vec::with_capacity(ROWS);
        for row in 0..ROWS {
            // Every (Region, Product) pair occurs exactly once.
            let region = row % REGIONS;
            let product = (row / REGIONS) % PRODUCTS;
            rows.push(vec![
                Scalar::String(format!("Region {region:02}")),
                Scalar::String(format!("Product {product:03}")),
                Scalar::Integer(row as i64),
            ]);
        }
        let model = DataModel {
            tables: vec![ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                rows,
            }],
            relationships: Vec::new(),
            measures: Vec::new(),
        };
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = parse_expression("SUMMARIZECOLUMNS(Sales[Region], Sales[Product])").expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        let table = evaluator.evaluate_table(&bound).expect("evaluate");
        assert_eq!(table.rows.len(), ROWS, "one row per existing region/product pair");

        // One scan per group column to build the domains plus one for the auto-exist projections;
        // the per-combination implementation would serve REGIONS * PRODUCTS * ROWS rows instead.
        let stats = evaluator.storage_stats();
        assert!(stats.rows_from_storage < 6 * ROWS as u64, "auto-exist must not rescan the table per combination: {stats:?} for {ROWS} source rows");
    }
}
