//! Filter functions.
//!
//! Two groups live here:
//!
//! * relation navigation and lookups (`INDEX`, `OFFSET`, `WINDOW`, `ORDERBY`, `PARTITIONBY`,
//!   `MATCHBY`, `LOOKUP`, `LOOKUPVALUE`, `LOOKUPWITHTOTALS`), implemented with real row/window
//!   semantics;
//! * analysis-services apply/cluster forms (`ALLCROSSFILTERED`, `ALLSELECTEDAPPLY`,
//!   `ALLSELECTEDREMOVE`, `ALWAYSAPPLY`, `DEPENDON`, `FILTERCLUSTER`, `GROUPCROSSAPPLY`,
//!   `GROUPCROSSAPPLYTABLE`, `NONFILTER`, `SHADOWCLUSTER`). Those are documented as internal to
//!   Power BI composite models; the bounded equivalents here materialise the same rows the
//!   documented syntax describes instead of returning `not_implemented`.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::ast::Literal;

use super::support::{eval_table as support_eval_table, frame_for};
use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "LOOKUPVALUE" => Some(lookup_value(ev, args, ctx)),
        "LOOKUP" => Some(lookup(ev, args, ctx, false)),
        "LOOKUPWITHTOTALS" => Some(lookup(ev, args, ctx, true)),
        // Clauses and table-only functions: a scalar call is a documented misuse, not a gap.
        "ORDERBY" | "PARTITIONBY" | "MATCHBY" => {
            Some(Err(EvalError::new(format!("{} is a clause and can only be used inside a window function", name.to_ascii_uppercase()))))
        }
        "INDEX" | "OFFSET" | "WINDOW" => {
            Some(Err(EvalError::new(format!("{} returns a table and cannot be used as a scalar", name.to_ascii_uppercase()))))
        }
        "ALLCROSSFILTERED" => Some(Err(EvalError::new("ALLCROSSFILTERED can only be used as a CALCULATE or CALCULATETABLE modifier"))),
        "ALLSELECTEDAPPLY"
        | "ALLSELECTEDREMOVE"
        | "ALWAYSAPPLY"
        | "DEPENDON"
        | "FILTERCLUSTER"
        | "GROUPCROSSAPPLY"
        | "GROUPCROSSAPPLYTABLE"
        | "NONFILTER"
        | "SHADOWCLUSTER" => Some(Err(EvalError::new(format!("{} returns a table", name.to_ascii_uppercase())))),
        _ => None,
    }
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "ALLCROSSFILTERED" => Some(all_cross_filtered(ev, args)),
        "ALLSELECTEDAPPLY" | "ALLSELECTEDREMOVE" | "ALWAYSAPPLY" | "NONFILTER" | "SHADOWCLUSTER" => Some(apply_wrapper(ev, name, args, ctx)),
        "DEPENDON" => Some(depend_on(ev, args, ctx)),
        "INDEX" => Some(index_table(ev, args, ctx)),
        "OFFSET" => Some(offset_table(ev, args, ctx)),
        "WINDOW" => Some(window_table(ev, args, ctx)),
        "FILTERCLUSTER" => Some(filter_cluster(ev, args, ctx)),
        "GROUPCROSSAPPLY" => Some(group_cross_apply(ev, args, ctx, false)),
        "GROUPCROSSAPPLYTABLE" => Some(group_cross_apply(ev, args, ctx, true)),
        "ORDERBY" | "PARTITIONBY" | "MATCHBY" => {
            Some(Err(EvalError::new(format!("{} is a clause and can only be used inside a window function", name.to_ascii_uppercase()))))
        }
        _ => None,
    }
}

/// How blank values are ordered by a window function. `DEFAULT` follows the documented DAX
/// ordering (numerical blanks just below zero, string blanks before every string); `FIRST` and
/// `LAST` pin blanks regardless of the sort direction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlankPlacement {
    Default,
    First,
    Last,
}

#[derive(Clone)]
struct OrderKey {
    expr: BoundExpr,
    desc: bool,
}

#[derive(Clone)]
struct WindowClause {
    order: Vec<OrderKey>,
    partitions: Vec<(usize, usize)>,
    match_by: Vec<(usize, usize)>,
    blanks: BlankPlacement,
}

impl Default for WindowClause {
    fn default() -> Self {
        WindowClause { order: Vec::new(), partitions: Vec::new(), match_by: Vec::new(), blanks: BlankPlacement::Default }
    }
}

/// One relation row with the values used for partitioning and ordering.
struct RowEntry {
    row: Vec<Scalar>,
    partition: Vec<ScalarKey>,
    order: Vec<Scalar>,
}

// ---------------------------------------------------------------------------------------------
// Window functions: INDEX / OFFSET / WINDOW
// ---------------------------------------------------------------------------------------------

fn index_table(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let first = args.first().ok_or_else(|| EvalError::new("INDEX needs a position"))?;
    let position = whole_number(ev.eval(first, ctx)?, "INDEX position")?;
    let (relation, clause) = relation_and_clause(ev, args, 1, ctx)?;
    let entries = sorted_entries(ev, &relation, &clause, ctx)?;
    let ranges = partition_ranges(&entries);
    let mut rows = Vec::new();
    if position != 0 {
        for range in ranges {
            let length = (range.end - range.start) as i64;
            let index = if position > 0 { position - 1 } else { length.saturating_add(position) };
            if index >= 0 && index < length {
                rows.push(entries[range.start + index as usize].row.clone());
            }
        }
    }
    Ok(TableValue::new(relation.columns, rows).with_lineage_if(relation.lineage))
}

fn offset_table(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let first = args.first().ok_or_else(|| EvalError::new("OFFSET needs a delta"))?;
    let delta = whole_number(ev.eval(first, ctx)?, "OFFSET delta")?;
    let (relation, clause) = relation_and_clause(ev, args, 1, ctx)?;
    let entries = sorted_entries(ev, &relation, &clause, ctx)?;
    let Some(found) = current_entry(ev, &relation, &clause, &entries, ctx)? else {
        return Err(EvalError::new("OFFSET needs a row context that identifies the current row"));
    };
    let ranges = partition_ranges(&entries);
    let range = ranges
        .into_iter()
        .find(|range| found >= range.start && found < range.end)
        .ok_or_else(|| EvalError::new("OFFSET could not identify the current partition"))?;
    let length = (range.end - range.start) as i64;
    let index = (found - range.start) as i64;
    let index = index.saturating_add(delta);
    let rows = if index >= 0 && index < length { vec![entries[range.start + index as usize].row.clone()] } else { Vec::new() };
    Ok(TableValue::new(relation.columns, rows).with_lineage_if(relation.lineage))
}

fn window_table(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::new("WINDOW needs a start and an end boundary"));
    }
    // The 2-argument form is `WINDOW(<start>, <end>)`; every longer form names both boundary
    // types, so arity decides the layout.
    let typed = args.len() >= 3;
    if typed && args.len() < 4 {
        return Err(EvalError::new("WINDOW needs an end boundary after its start type"));
    }
    let from_value = whole_number(ev.eval(&args[0], ctx)?, "WINDOW start")?;
    let from_type = if typed { boundary_type(&args[1])? } else { BoundaryType::Rel };
    let to_index = if typed { 2 } else { 1 };
    let to_value = whole_number(ev.eval(&args[to_index], ctx)?, "WINDOW end")?;
    let to_type = if typed { boundary_type(&args[3])? } else { BoundaryType::Rel };
    let relation_start = if typed { 4 } else { 2 };
    let (relation, clause) = relation_and_clause(ev, args, relation_start, ctx)?;
    let entries = sorted_entries(ev, &relation, &clause, ctx)?;
    let ranges = partition_ranges(&entries);
    let relative = from_type == BoundaryType::Rel || to_type == BoundaryType::Rel;
    let anchored = if relative { current_entry(ev, &relation, &clause, &entries, ctx)? } else { None };
    if relative && anchored.is_none() {
        return Err(EvalError::new("WINDOW with REL boundaries needs a row context that identifies the current row"));
    }

    let mut rows = Vec::new();
    for range in ranges {
        if let Some(anchor) = anchored
            && !(anchor >= range.start && anchor < range.end)
        {
            continue;
        }
        let length = (range.end - range.start) as i64;
        let anchor_position = anchored.map(|anchor| (anchor - range.start) as i64);
        let start = boundary_position(from_value, from_type, length, anchor_position);
        let end = boundary_position(to_value, to_type, length, anchor_position);
        // A window wholly outside the partition (or with its start after its end) is empty;
        // otherwise the boundaries are clamped to the partition edges.
        if length == 0 || start > length - 1 || end < 0 || start > end {
            continue;
        }
        let start = start.max(0) as usize;
        let end = end.min(length - 1) as usize;
        for entry in &entries[range.start + start..=range.start + end] {
            rows.push(entry.row.clone());
        }
    }
    Ok(TableValue::new(relation.columns, rows).with_lineage_if(relation.lineage))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryType {
    Abs,
    Rel,
}

fn boundary_type(expr: &BoundExpr) -> Result<BoundaryType, EvalError> {
    let word = match expr {
        BoundExpr::Keyword(word) => word.clone(),
        BoundExpr::Literal(Literal::String(word)) => word.clone(),
        _ => return Err(EvalError::new("WINDOW boundary type must be ABS or REL")),
    };
    if word.eq_ignore_ascii_case("ABS") {
        Ok(BoundaryType::Abs)
    } else if word.eq_ignore_ascii_case("REL") {
        Ok(BoundaryType::Rel)
    } else {
        Err(EvalError::new(format!("WINDOW boundary type must be ABS or REL, found `{word}`")))
    }
}

/// Zero-based position of one window boundary inside a partition.
fn boundary_position(value: i64, kind: BoundaryType, length: i64, anchor: Option<i64>) -> i64 {
    match kind {
        // ABS is 1-based, 0 is treated as 1, and negative positions count from the end.
        BoundaryType::Abs => {
            if value > 0 {
                value - 1
            } else if value < 0 {
                length.saturating_add(value)
            } else {
                0
            }
        }
        BoundaryType::Rel => anchor.unwrap_or(0).saturating_add(value),
    }
}

/// Parses the optional relation plus the `ORDERBY`, `PARTITIONBY`, `MATCHBY`, blanks and reset
/// slots of a window function. Slots are matched by their clause name, not by position, because
/// the documented syntax allows the relation and several clauses to be omitted.
fn relation_and_clause(ev: &Evaluator<'_>, args: &[BoundExpr], start: usize, ctx: &mut Context) -> Result<(TableValue, WindowClause), EvalError> {
    let mut relation: Option<TableValue> = None;
    let mut clause = WindowClause::default();
    for arg in args.iter().skip(start) {
        match arg {
            BoundExpr::Call { spec, .. } if spec.name.eq_ignore_ascii_case("ORDERBY") => clause.order = parse_order_by(arg)?,
            BoundExpr::Call { spec, .. } if spec.name.eq_ignore_ascii_case("PARTITIONBY") => {
                clause.partitions = parse_column_clause(arg, "PARTITIONBY")?
            }
            BoundExpr::Call { spec, .. } if spec.name.eq_ignore_ascii_case("MATCHBY") => clause.match_by = parse_column_clause(arg, "MATCHBY")?,
            BoundExpr::Keyword(word) => {
                if let Some(placement) = blank_placement(word)? {
                    clause.blanks = placement;
                }
            }
            BoundExpr::Literal(Literal::String(word)) => {
                if let Some(placement) = blank_placement(word)? {
                    clause.blanks = placement;
                }
            }
            _ => {
                if relation.is_some() {
                    return Err(EvalError::new("a window function accepts only one relation"));
                }
                relation = Some(support_eval_table(ev, arg, ctx)?);
            }
        }
    }
    let relation = match relation {
        Some(relation) => relation,
        None => default_relation(ev, &clause)?,
    };
    Ok((relation, clause))
}

fn blank_placement(word: &str) -> Result<Option<BlankPlacement>, EvalError> {
    if word.eq_ignore_ascii_case("FIRST") {
        Ok(Some(BlankPlacement::First))
    } else if word.eq_ignore_ascii_case("LAST") {
        Ok(Some(BlankPlacement::Last))
    } else if word.eq_ignore_ascii_case("DEFAULT") || word.eq_ignore_ascii_case("KEEP") {
        Ok(Some(BlankPlacement::Default))
    } else if word.eq_ignore_ascii_case("NONE") || word.eq_ignore_ascii_case("LOWESTPARENT") || word.eq_ignore_ascii_case("HIGHESTPARENT") {
        // `reset` is a visual-calculation slot; the evaluator has no visual shape, so it is
        // accepted and ignored without disturbing the `blanks` placement.
        Ok(None)
    } else {
        Err(EvalError::new(format!("unknown window clause value `{word}`")))
    }
}

fn parse_order_by(expr: &BoundExpr) -> Result<Vec<OrderKey>, EvalError> {
    let BoundExpr::Call { args, .. } = expr else {
        return Err(EvalError::new("ORDERBY needs a call"));
    };
    let mut keys = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].clone();
        if matches!(key, BoundExpr::Keyword(_)) {
            return Err(EvalError::new("ORDERBY needs an expression before its direction"));
        }
        let next = args.get(index + 1).and_then(direction_of);
        match next {
            Some(desc) => {
                keys.push(OrderKey { expr: key, desc });
                index += 2;
            }
            None => {
                keys.push(OrderKey { expr: key, desc: false });
                index += 1;
            }
        }
    }
    Ok(keys)
}

fn direction_of(expr: &BoundExpr) -> Option<bool> {
    let word = match expr {
        BoundExpr::Keyword(word) => word.as_str(),
        BoundExpr::Literal(Literal::String(word)) => word.as_str(),
        _ => return None,
    };
    if word.eq_ignore_ascii_case("ASC") {
        Some(false)
    } else if word.eq_ignore_ascii_case("DESC") {
        Some(true)
    } else {
        None
    }
}

fn parse_column_clause(expr: &BoundExpr, name: &str) -> Result<Vec<(usize, usize)>, EvalError> {
    let BoundExpr::Call { args, .. } = expr else {
        return Err(EvalError::new(format!("{name} needs a call")));
    };
    args.iter().map(|arg| column_ref(arg, name)).collect()
}

fn column_ref(expr: &BoundExpr, context: &str) -> Result<(usize, usize), EvalError> {
    match expr {
        BoundExpr::Column { table, column } => Ok((*table, *column)),
        _ => Err(EvalError::new(format!("{context} needs a fully qualified column reference"))),
    }
}

/// `ALLSELECTED()` of the ordering and partitioning columns, used when a window function omits
/// its relation. This evaluator has no outer shadow filter context, so the unfiltered model table
/// is the bounded equivalent.
fn default_relation(ev: &Evaluator<'_>, clause: &WindowClause) -> Result<TableValue, EvalError> {
    let mut columns: Vec<(usize, usize)> = Vec::new();
    let mut seen_columns = BTreeSet::new();
    for key in &clause.order {
        let column = column_ref(&key.expr, "an ORDERBY expression without a relation")?;
        if seen_columns.insert(column) {
            columns.push(column);
        }
    }
    for column in &clause.partitions {
        if seen_columns.insert(*column) {
            columns.push(*column);
        }
    }
    let Some(&(table, _)) = columns.first() else {
        return Err(EvalError::new("a window function needs a relation or an ORDERBY clause"));
    };
    if columns.iter().any(|(candidate, _)| *candidate != table) {
        return Err(EvalError::new("ORDERBY and PARTITIONBY columns must come from one table when no relation is given"));
    }
    let model = ev.model();
    let Some(model_table) = model.tables.get(table) else {
        return Err(EvalError::new("window relation is out of range"));
    };
    let mut names = Vec::with_capacity(columns.len());
    let mut selected = Vec::with_capacity(columns.len());
    for (_, column) in &columns {
        let Some(model_column) = model_table.columns.get(*column) else {
            return Err(EvalError::new("window column is out of range"));
        };
        names.push(TableColumn { name: model_column.name.clone() });
        selected.push(*column);
    }
    let source = ev.table(table, &FilterContext::default());
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for row in &source.rows {
        let projected: Vec<Scalar> = selected.iter().filter_map(|column| row.get(*column).cloned()).collect();
        if projected.len() != selected.len() {
            continue;
        }
        let key: Vec<ScalarKey> = projected.iter().map(ScalarKey::from).collect();
        if seen.insert(key) {
            rows.push(projected);
        }
    }
    Ok(TableValue::new(names, rows).with_lineage(table))
}

fn sorted_entries(ev: &Evaluator<'_>, relation: &TableValue, clause: &WindowClause, ctx: &mut Context) -> Result<Vec<RowEntry>, EvalError> {
    let split = partition_values(clause, ctx);
    let mut entries = Vec::with_capacity(relation.rows.len());
    for row in &relation.rows {
        ctx.rows.push(frame_for(ev, relation, row));
        let computed = (|| -> Result<(Vec<ScalarKey>, Vec<Scalar>), EvalError> {
            let mut partition = Vec::with_capacity(clause.partitions.len());
            for (table, column) in &clause.partitions {
                let value = ev.eval(&BoundExpr::Column { table: *table, column: *column }, ctx)?;
                partition.push(ScalarKey::from(&value));
            }
            let mut order = Vec::with_capacity(clause.order.len());
            for key in &clause.order {
                order.push(ev.eval(&key.expr, ctx)?);
            }
            Ok((partition, order))
        })();
        ctx.rows.pop();
        let (partition, order) = computed?;
        // An outer single value on a PARTITIONBY column selects that partition; the documented
        // engine returns the union of partitions only for the columns without an outer value.
        if let Some(split) = &split
            && partition != *split
        {
            continue;
        }
        entries.push(RowEntry { row: row.clone(), partition, order });
    }
    entries.sort_by(|left, right| left.partition.cmp(&right.partition).then_with(|| compare_orders(&left.order, &right.order, clause)));
    Ok(entries)
}

/// The partition selected by the outer filter context, when a `PARTITIONBY` column is filtered
/// down to exactly one value.
fn partition_values(clause: &WindowClause, ctx: &Context) -> Option<Vec<ScalarKey>> {
    if clause.partitions.is_empty() {
        return None;
    }
    let mut values = Vec::with_capacity(clause.partitions.len());
    for (table, column) in &clause.partitions {
        match ctx.filters.filters.get(&(*table, *column)) {
            Some(keys) if keys.len() == 1 => values.push(keys.iter().next()?.clone()),
            // Without a single outer value every partition is evaluated and unioned.
            _ => return None,
        }
    }
    Some(values)
}

fn compare_orders(left: &[Scalar], right: &[Scalar], clause: &WindowClause) -> Ordering {
    for (index, key) in clause.order.iter().enumerate() {
        let ordering = compare_window_values(left.get(index), right.get(index), key.desc, clause.blanks);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_window_values(left: Option<&Scalar>, right: Option<&Scalar>, desc: bool, blanks: BlankPlacement) -> Ordering {
    let left_blank = left.is_none_or(Scalar::is_blank);
    let right_blank = right.is_none_or(Scalar::is_blank);
    if left_blank && right_blank {
        return Ordering::Equal;
    }
    if left_blank || right_blank {
        let ordering = match blanks {
            BlankPlacement::First => {
                if left_blank {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            BlankPlacement::Last => {
                if left_blank {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            BlankPlacement::Default => {
                let other = if left_blank { right } else { left };
                let blank_first = other.is_none_or(|value| match value {
                    Scalar::String(_) => true,
                    _ => value.to_number().is_none_or(|number| number >= 0.0),
                });
                let ordering = if left_blank {
                    if blank_first { Ordering::Less } else { Ordering::Greater }
                } else if blank_first {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
                return if desc { ordering.reverse() } else { ordering };
            }
        };
        // FIRST/LAST pin blanks regardless of the sort direction.
        return ordering;
    }
    let ordering = left.zip(right).and_then(|(left, right)| left.compare(right)).unwrap_or(Ordering::Equal);
    if desc { ordering.reverse() } else { ordering }
}

/// Contiguous entry ranges, one per partition (the entries are already partitioned-sorted).
fn partition_ranges(entries: &[RowEntry]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for index in 1..=entries.len() {
        let boundary = index == entries.len() || entries[index].partition != entries[start].partition;
        if boundary && index > start {
            ranges.push(start..index);
            start = index;
        }
    }
    ranges
}

/// Position of the current row in the sorted relation, identified by `MATCHBY` columns or by the
/// whole relation row.
fn current_entry(
    ev: &Evaluator<'_>,
    relation: &TableValue,
    clause: &WindowClause,
    entries: &[RowEntry],
    ctx: &mut Context,
) -> Result<Option<usize>, EvalError> {
    if ctx.rows.is_empty() {
        return Ok(None);
    }
    let mut needle: Vec<Scalar> = Vec::new();
    if clause.match_by.is_empty() {
        let Some(frame) = ctx.rows.last() else { return Ok(None) };
        for (index, column) in relation.columns.iter().enumerate() {
            let value = if let Some(lineage) = relation.lineage
                && let Some(model_column) = ev.model().column_index(lineage, &column.name)
            {
                frame.get(model_column).cloned()
            } else {
                frame.get_name(&column.name)
            };
            let Some(value) = value else {
                return Err(EvalError::new("window function could not identify the current row; add a MATCHBY clause"));
            };
            needle.push(value);
            let _ = index;
        }
    } else {
        for (table, column) in &clause.match_by {
            needle.push(ev.eval(&BoundExpr::Column { table: *table, column: *column }, ctx)?);
        }
    }

    if clause.match_by.is_empty() {
        return Ok(entries.iter().position(|entry| entry.row.iter().zip(&needle).all(|(value, needle)| value.strict_eq(needle))));
    }
    let positions: Vec<usize> =
        clause.match_by.iter().map(|(table, column)| relation_position(ev, relation, *table, *column)).collect::<Result<_, _>>()?;
    Ok(entries.iter().position(|entry| {
        needle
            .iter()
            .enumerate()
            .all(|(needle_index, value)| entry.row.get(positions[needle_index]).is_some_and(|candidate| candidate.strict_eq(value)))
    }))
}

fn relation_position(ev: &Evaluator<'_>, relation: &TableValue, table: usize, column: usize) -> Result<usize, EvalError> {
    if relation.lineage == Some(table) {
        return Ok(column);
    }
    let name = ev
        .model()
        .tables
        .get(table)
        .and_then(|model_table| model_table.columns.get(column))
        .map(|column| column.name.as_str())
        .ok_or_else(|| EvalError::new("window column is out of range"))?;
    relation.column_index(name).ok_or_else(|| EvalError::new("window column is not part of the relation"))
}

// ---------------------------------------------------------------------------------------------
// LOOKUP / LOOKUPVALUE / LOOKUPWITHTOTALS
// ---------------------------------------------------------------------------------------------

fn lookup_value(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    if args.len() < 3 {
        return Err(EvalError::new("LOOKUPVALUE needs a result column and at least one search pair"));
    }
    let (result_table, result_column) = column_ref(&args[0], "LOOKUPVALUE result")?;
    let has_alternate = (args.len() - 1) % 2 == 1;
    let pair_end = if has_alternate { args.len() - 1 } else { args.len() };
    let mut pairs: Vec<(usize, Scalar)> = Vec::new();
    let mut index = 1;
    while index < pair_end {
        let (table, column) = column_ref(&args[index], "LOOKUPVALUE search column")?;
        if table != result_table {
            return Err(EvalError::new("LOOKUPVALUE search columns must belong to the result column's table"));
        }
        pairs.push((column, ev.eval(&args[index + 1], ctx)?));
        index += 2;
    }
    if pairs.is_empty() {
        return Err(EvalError::new("LOOKUPVALUE needs at least one search pair"));
    }

    let source = ev.table(result_table, &FilterContext::default());
    let mut found: Option<Scalar> = None;
    for row in &source.rows {
        let matches = pairs.iter().all(|(column, needle)| row.get(*column).is_some_and(|value| values_equal(value, needle)));
        if !matches {
            continue;
        }
        let value = row.get(result_column).cloned().unwrap_or(Scalar::Blank);
        match &found {
            Some(previous) if !previous.strict_eq(&value) => {
                return match alternate(args, has_alternate, ev, ctx) {
                    Some(value) => value,
                    None => Err(EvalError::new("LOOKUPVALUE found multiple different values; use an alternate result")),
                };
            }
            Some(_) => {}
            None => found = Some(value),
        }
    }
    match found {
        Some(value) => Ok(value),
        None => alternate(args, has_alternate, ev, ctx).unwrap_or(Ok(Scalar::Blank)),
    }
}

fn alternate(args: &[BoundExpr], has_alternate: bool, ev: &Evaluator<'_>, ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    if has_alternate { args.last().map(|expr| ev.eval(expr, ctx)) } else { None }
}

fn lookup(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, totals: bool) -> Result<Scalar, EvalError> {
    let name = if totals { "LOOKUPWITHTOTALS" } else { "LOOKUP" };
    if args.len() < 3 {
        return Err(EvalError::new(format!("{name} needs an expression and at least one search pair")));
    }
    let has_behavior = args.len().is_multiple_of(2);
    let pair_end = if has_behavior { args.len() - 1 } else { args.len() };
    let mut pairs: Vec<(usize, usize, Scalar)> = Vec::new();
    let mut index = 1;
    while index < pair_end {
        let (table, column) = column_ref(&args[index], &format!("{name} search column"))?;
        pairs.push((table, column, ev.eval(&args[index + 1], ctx)?));
        index += 2;
    }
    if pairs.is_empty() {
        return Err(EvalError::new(format!("{name} needs at least one search pair")));
    }
    let search_table = pairs[0].0;
    if pairs.iter().any(|(table, _, _)| *table != search_table) {
        return Err(EvalError::new(format!("{name} search columns must belong to one table")));
    }
    if has_behavior {
        let behavior = match args.last() {
            Some(BoundExpr::Keyword(word)) => word.clone(),
            Some(expr) => ev.eval(expr, ctx)?.to_text(),
            None => String::new(),
        };
        if !behavior.eq_ignore_ascii_case("EXPLICIT") && !behavior.eq_ignore_ascii_case("INFERRED") && !behavior.eq_ignore_ascii_case("DEFAULT") {
            return Err(EvalError::new("LOOKUP associated-columns behavior must be EXPLICIT or INFERRED"));
        }
        // INFERRED fills functionally determined axis columns; without a visual shape the
        // evaluator cannot infer additional columns, so EXPLICIT is the bounded behaviour.
    }

    let source = ev.table(search_table, &FilterContext::default());
    let matches: Vec<usize> = source
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| pairs.iter().all(|(_, column, needle)| row.get(*column).is_some_and(|value| values_equal(value, needle))))
        .map(|(index, _)| index)
        .collect();
    let Some(&first_match) = matches.first() else {
        return Err(EvalError::new(format!("{name} did not match any row")));
    };
    let row = source.rows.get(first_match).cloned().unwrap_or_default();

    let mut filters = if totals { FilterContext::default() } else { ctx.filters.clone() };
    for (_, column, value) in &pairs {
        filters.set(search_table, *column, [ScalarKey::from(value)]);
    }

    // A result column needs a single row; an aggregate/measure is evaluated with the search
    // filters only, so its argument rows may be many (LOOKUP maps visual coordinates, not rows).
    match &args[0] {
        BoundExpr::Column { table, column } if *table != search_table => related_lookup(ev, search_table, &row, *table, *column, &filters),
        BoundExpr::Column { .. } if matches.len() > 1 => Err(EvalError::new(format!("{name} result column matches more than one row"))),
        BoundExpr::Column { column, .. } => Ok(source.rows.get(first_match).and_then(|row| row.get(*column)).cloned().unwrap_or(Scalar::Blank)),
        expression => eval_with(ev, expression, ctx, &filters),
    }
}

/// Evaluates a result column that lives in a table related to the search table, using the same
/// active-relationship rules as `RELATED`.
fn related_lookup(
    ev: &Evaluator<'_>,
    search_table: usize,
    row: &[Scalar],
    target_table: usize,
    target_column: usize,
    filters: &FilterContext,
) -> Result<Scalar, EvalError> {
    let model = ev.model();
    for relationship in &model.relationships {
        if relationship.inactive {
            continue;
        }
        let from = model.table_index(&relationship.from_table);
        let to = model.table_index(&relationship.to_table);
        let (key_column, other, other_column) = if from == Some(search_table) && to == Some(target_table) {
            (&relationship.from_column, target_table, &relationship.to_column)
        } else if relationship.bidirectional && to == Some(search_table) && from == Some(target_table) {
            (&relationship.to_column, target_table, &relationship.from_column)
        } else {
            continue;
        };
        let Some(key_index) = model.column_index(search_table, key_column) else {
            continue;
        };
        let Some(key) = row.get(key_index) else {
            continue;
        };
        let Some(other_key) = model.column_index(other, other_column) else {
            continue;
        };
        for related in ev.table(other, filters).rows {
            if related.get(other_key).is_some_and(|value| value.strict_eq(key)) {
                return related.get(target_column).cloned().ok_or_else(|| EvalError::new("lookup result column is out of range"));
            }
        }
    }
    Err(EvalError::new("LOOKUP result column needs an active relationship to the search table"))
}

fn values_equal(left: &Scalar, right: &Scalar) -> bool {
    left.strict_eq(right) || matches!(left.compare(right), Some(Ordering::Equal))
}

// ---------------------------------------------------------------------------------------------
// Apply/cluster forms
// ---------------------------------------------------------------------------------------------

fn all_cross_filtered(ev: &Evaluator<'_>, args: &[BoundExpr]) -> Result<TableValue, EvalError> {
    let table = match args.first() {
        Some(BoundExpr::Table(table)) => *table,
        _ => return Err(EvalError::new("ALLCROSSFILTERED needs a table reference")),
    };
    // The documented semantics also remove filters on tables that cross-filter this one. The
    // model's active relationships are single-column and the evaluator propagates filters
    // through them, so materialising the unfiltered table is the bounded equivalent: applying it
    // as a CALCULATE filter replaces every filter on the target table.
    Ok(ev.table(table, &FilterContext::default()))
}

fn apply_wrapper(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let expression = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a table expression")))?;
    // These wrappers only change how the table participates in the filter context of the
    // enclosing GROUPCROSSAPPLY; the table rows themselves are unchanged.
    support_eval_table(ev, expression, ctx)
}

fn depend_on(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::new("DEPENDON needs a table expression and at least one column"));
    }
    let table = support_eval_table(ev, &args[0], ctx)?;
    let mut needle = Vec::with_capacity(args.len() - 1);
    for arg in &args[1..] {
        needle.push(ev.eval(arg, ctx)?);
    }
    let width = needle.len();
    if table.columns.len() < width {
        return Err(EvalError::new("DEPENDON needs the table to start with the listed outer columns"));
    }
    let mut rows = Vec::new();
    for row in &table.rows {
        let matches = needle.iter().enumerate().all(|(index, value)| row.get(index).is_some_and(|candidate| candidate.strict_eq(value)));
        if matches {
            rows.push(row[width..].to_vec());
        }
    }
    Ok(TableValue::new(table.columns[width..].to_vec(), rows).with_lineage_if(table.lineage))
}

/// Splits a `GROUPCROSSAPPLY`/`FILTERCLUSTER` argument list into group-by columns, filter tables,
/// and the optional separator/scan tail.
struct ApplyArgs<'a> {
    groups: Vec<(usize, usize)>,
    filters: Vec<&'a BoundExpr>,
    separator: Option<usize>,
}

fn split_apply_args<'a>(args: &'a [BoundExpr]) -> Result<ApplyArgs<'a>, EvalError> {
    let mut index = 0;
    let mut groups = Vec::new();
    while let Some(BoundExpr::Column { table, column }) = args.get(index) {
        groups.push((*table, *column));
        index += 1;
    }
    if groups.is_empty() {
        return Err(EvalError::new("the apply function needs at least one group-by column"));
    }
    let mut filters = Vec::new();
    let mut separator = None;
    while index < args.len() {
        if matches!(args[index], BoundExpr::Literal(Literal::String(_))) {
            separator = Some(index);
            break;
        }
        filters.push(&args[index]);
        index += 1;
    }
    Ok(ApplyArgs { groups, filters, separator })
}

fn group_cross_apply(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, table_mode: bool) -> Result<TableValue, EvalError> {
    let name = if table_mode { "GROUPCROSSAPPLYTABLE" } else { "GROUPCROSSAPPLY" };
    let split = split_apply_args(args)?;
    let group_table = split.groups[0].0;
    if split.groups.iter().any(|(table, _)| *table != group_table) {
        return Err(EvalError::new(format!("{name} group-by columns must belong to one table")));
    }
    let mut filter_tables = Vec::with_capacity(split.filters.len());
    for expr in &split.filters {
        filter_tables.push(support_eval_table(ev, expr, ctx)?);
    }

    // Tail: either name/expression pairs (GROUPCROSSAPPLY) or a separator followed by table
    // expressions (GROUPCROSSAPPLYTABLE).
    let mut expressions: Vec<(String, BoundExpr)> = Vec::new();
    let mut scans: Vec<BoundExpr> = Vec::new();
    if table_mode {
        let mut index = split.separator.ok_or_else(|| EvalError::new(format!("{name} needs a separator before its table expressions")))?;
        while index < args.len() {
            let BoundExpr::Literal(Literal::String(_)) = &args[index] else {
                return Err(EvalError::new(format!("{name} needs string separators before its table expressions")));
            };
            let expression = args.get(index + 1).ok_or_else(|| EvalError::new(format!("{name} needs a table expression after each separator")))?;
            scans.push(expression.clone());
            index += 2;
        }
        if scans.is_empty() {
            return Err(EvalError::new(format!("{name} needs at least one table expression")));
        }
    } else if let Some(mut index) = split.separator {
        while index < args.len() {
            let BoundExpr::Literal(Literal::String(column_name)) = &args[index] else {
                return Err(EvalError::new(format!("{name} needs string column names")));
            };
            let expression = args.get(index + 1).ok_or_else(|| EvalError::new(format!("{name} needs an expression for `{column_name}`")))?;
            expressions.push((column_name.clone(), expression.clone()));
            index += 2;
        }
    }

    let model = ev.model();
    let group_columns: Vec<TableColumn> = split
        .groups
        .iter()
        .map(|(_, column)| {
            model
                .tables
                .get(group_table)
                .and_then(|table| table.columns.get(*column))
                .map(|column| TableColumn { name: column.name.clone() })
                .ok_or_else(|| EvalError::new("group-by column is out of range"))
        })
        .collect::<Result<_, _>>()?;
    let groups = distinct_group_values(ev, group_table, &split.groups)?;

    let mut columns = group_columns;
    let mut scan_columns: Vec<TableColumn> = Vec::new();
    let mut rows: Vec<Vec<Scalar>> = Vec::new();
    for group in &groups {
        let mut filters = ctx.filters.clone();
        // The filter tables are cross-joined first; the group-by filter then pins the group's own
        // columns, so `ALL(Sales)` in a filter table cannot undo the grouping.
        for table in &filter_tables {
            apply_table_filter(ev, table, &mut filters);
        }
        for ((table, column), value) in split.groups.iter().zip(group) {
            filters.set(*table, *column, [ScalarKey::from(value)]);
        }

        if table_mode {
            let mut combinations: Vec<Vec<Scalar>> = vec![Vec::new()];
            for expression in &scans {
                let table = eval_table_with(ev, expression, ctx, &filters)?;
                // The first evaluated table defines the scan columns; later tables must line up.
                if scan_columns.is_empty() {
                    scan_columns = table.columns.clone();
                }
                let mut next = Vec::with_capacity(combinations.len().saturating_mul(table.rows.len()));
                for prefix in &combinations {
                    for row in &table.rows {
                        let mut combined = prefix.clone();
                        combined.extend(row.clone());
                        next.push(combined);
                    }
                }
                combinations = next;
            }
            for combination in combinations {
                let mut row = group.clone();
                row.extend(combination);
                rows.push(row);
            }
        } else {
            let mut values = Vec::with_capacity(expressions.len());
            let mut all_blank = true;
            for (_, expression) in &expressions {
                let value = eval_with(ev, expression, ctx, &filters)?;
                all_blank &= value.is_blank();
                values.push(value);
            }
            // Only rows where every supplied expression is blank are dropped; a call without
            // expressions simply lists the groups.
            if !expressions.is_empty() && all_blank {
                continue;
            }
            let mut row = group.clone();
            row.extend(values);
            rows.push(row);
        }
    }
    if table_mode {
        columns.extend(scan_columns);
    } else {
        columns.extend(expressions.iter().map(|(name, _)| TableColumn { name: name.clone() }));
    }
    Ok(TableValue::new(columns, rows))
}

fn distinct_group_values(ev: &Evaluator<'_>, table: usize, groups: &[(usize, usize)]) -> Result<Vec<Vec<Scalar>>, EvalError> {
    let source = ev.table(table, &FilterContext::default());
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for row in &source.rows {
        let mut group = Vec::with_capacity(groups.len());
        for (_, column) in groups {
            let Some(value) = row.get(*column) else { continue };
            group.push(value.clone());
        }
        if group.len() != groups.len() {
            continue;
        }
        let key: Vec<ScalarKey> = group.iter().map(ScalarKey::from).collect();
        if seen.insert(key) {
            values.push(group);
        }
    }
    Ok(values)
}

fn filter_cluster(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let split = split_apply_args(args)?;
    let separator = split.separator.ok_or_else(|| EvalError::new("FILTERCLUSTER needs a separator between filter tables and table scans"))?;
    if separator + 1 >= args.len() {
        return Err(EvalError::new("FILTERCLUSTER needs at least one table scan after the separator"));
    }
    let group_table = split.groups[0].0;
    if split.groups.iter().any(|(table, _)| *table != group_table) {
        return Err(EvalError::new("FILTERCLUSTER group-by columns must belong to one table"));
    }
    let mut filter_tables = Vec::with_capacity(split.filters.len());
    for expr in &split.filters {
        filter_tables.push(support_eval_table(ev, expr, ctx)?);
    }
    let scans: Vec<BoundExpr> = args[separator + 1..].to_vec();
    if scans.iter().any(|scan| matches!(scan, BoundExpr::Literal(Literal::String(_)))) {
        return Err(EvalError::new("FILTERCLUSTER accepts table scans only after the separator"));
    }

    let model = ev.model();
    let group_columns: Vec<TableColumn> = split
        .groups
        .iter()
        .map(|(_, column)| {
            model
                .tables
                .get(group_table)
                .and_then(|table| table.columns.get(*column))
                .map(|column| TableColumn { name: column.name.clone() })
                .ok_or_else(|| EvalError::new("group-by column is out of range"))
        })
        .collect::<Result<_, _>>()?;
    let groups = distinct_group_values(ev, group_table, &split.groups)?;

    let mut columns = group_columns;
    let mut scan_columns: Vec<TableColumn> = Vec::new();
    let mut rows = Vec::new();
    for group in &groups {
        let mut filters = ctx.filters.clone();
        for table in &filter_tables {
            apply_table_filter(ev, table, &mut filters);
        }
        for ((table, column), value) in split.groups.iter().zip(group) {
            filters.set(*table, *column, [ScalarKey::from(value)]);
        }
        let mut combinations: Vec<Vec<Scalar>> = vec![Vec::new()];
        for scan in &scans {
            let table = eval_table_with(ev, scan, ctx, &filters)?;
            if scan_columns.is_empty() {
                scan_columns = table.columns.clone();
            } else if scan_columns != table.columns {
                return Err(EvalError::new("FILTERCLUSTER table scans must have the same columns"));
            }
            let mut next = Vec::with_capacity(combinations.len().saturating_mul(table.rows.len()));
            for prefix in &combinations {
                for row in &table.rows {
                    let mut combined = prefix.clone();
                    combined.extend(row.clone());
                    next.push(combined);
                }
            }
            combinations = next;
        }
        for combination in combinations {
            let mut row = group.clone();
            row.extend(combination);
            rows.push(row);
        }
    }
    columns.extend(scan_columns);
    Ok(TableValue::new(columns, rows))
}

/// Applies a table expression as a CALCULATE filter: every column of its lineage is restricted to
/// the values present in the table.
fn apply_table_filter(ev: &Evaluator<'_>, table: &TableValue, filters: &mut FilterContext) {
    let Some(lineage) = table.lineage else {
        return;
    };
    for (position, column) in table.columns.iter().enumerate() {
        // Map by name: a table expression can expose a subset or a reordering of the model
        // columns (VALUES, DATESMTD, SELECTCOLUMNS).
        let Some(model_column) = ev.model().column_index(lineage, &column.name) else { continue };
        let keys: BTreeSet<ScalarKey> = table.rows.iter().filter_map(|row| row.get(position)).map(ScalarKey::from).collect();
        filters.set(lineage, model_column, keys);
    }
}

fn eval_with(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, filters: &FilterContext) -> Result<Scalar, EvalError> {
    let saved = std::mem::replace(&mut ctx.filters, filters.clone());
    let result = ev.eval(expr, ctx);
    ctx.filters = saved;
    result
}

fn eval_table_with(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, filters: &FilterContext) -> Result<TableValue, EvalError> {
    let saved = std::mem::replace(&mut ctx.filters, filters.clone());
    let result = support_eval_table(ev, expr, ctx);
    ctx.filters = saved;
    result
}

fn whole_number(value: Scalar, context: &str) -> Result<i64, EvalError> {
    let number = value.to_number().ok_or_else(|| EvalError::new(format!("{context} must be numeric")))?;
    if !number.is_finite() {
        return Err(EvalError::new(format!("{context} must be a finite number")));
    }
    let truncated = number.trunc();
    if (number - truncated).abs() > 1e-9 {
        return Err(EvalError::new(format!("{context} must be a whole number")));
    }
    Ok(truncated as i64)
}
