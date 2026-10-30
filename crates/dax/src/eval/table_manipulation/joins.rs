//! Table manipulation: joins, generation and row shaping.
//!
//! The functions here are table-valued: `DETAILROWS`, `FILTERS`, `GENERATE`, `GENERATEALL`,
//! `GENERATESERIES`, the `NATURAL*` joins, `NATURALJOINUSAGE`, `SAMPLEAXISWITHLOCALMINMAX` and
//! `SUBSTITUTEWITHINDEX`. Every dispatcher returns `None` for names owned by another family.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::support::{eval_all, frame_for};
use super::*;
use crate::ast::Literal;

/// Hard ceiling for tables produced row-by-row by `GENERATE*`/`GENERATESERIES`. The limit keeps
/// a pathological argument from exhausting memory; real DAX results never approach it.
const MAX_GENERATED_ROWS: usize = 1_000_000;

pub(crate) fn call(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    None
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "DETAILROWS" => detail_rows(ev, args, ctx),
        "FILTERS" => filters(ev, args, ctx),
        "GENERATE" => generate(ev, args, ctx, false),
        "GENERATEALL" => generate(ev, args, ctx, true),
        "GENERATESERIES" => generate_series(ev, args, ctx),
        "NATURALINNERJOIN" => natural_join(ev, args, ctx, false),
        "NATURALLEFTOUTERJOIN" => natural_join(ev, args, ctx, true),
        "NATURALJOINUSAGE" => natural_join_usage(ev, args, ctx),
        "SAMPLEAXISWITHLOCALMINMAX" => sample_axis_with_local_min_max(ev, args, ctx),
        "SUBSTITUTEWITHINDEX" => substitute_with_index(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

/// The model table and column behind a column argument. `None` means the argument is not a
/// column reference (for example a literal or a computed expression).
fn column_name(ev: &Evaluator<'_>, expr: &BoundExpr) -> Option<String> {
    match expr {
        BoundExpr::Column { table, column } => ev.model().tables.get(*table)?.columns.get(*column).map(|column| column.name.clone()),
        BoundExpr::UnqualifiedColumn { name } => Some(name.clone()),
        _ => None,
    }
}

/// Resolves a column argument against the columns of an already evaluated table.
fn column_position(ev: &Evaluator<'_>, table: &TableValue, expr: &BoundExpr) -> Result<usize, EvalError> {
    let name = column_name(ev, expr).ok_or_else(|| EvalError::new("expected a column reference"))?;
    table.column_index(&name).ok_or_else(|| EvalError::new(format!("column `{name}` is not part of the table")))
}

/// `DETAILROWS(<Measure>)`: with no DetailRows expression in the model, DAX returns the entire
/// home table of the measure in the current filter context. The in-memory model cannot store a
/// DetailRows expression, so this fallback is the only path.
fn detail_rows(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let arg = args.first().ok_or_else(|| EvalError::new("DETAILROWS needs a measure reference"))?;
    let BoundExpr::Measure(index) = arg else {
        return Err(EvalError::new("DETAILROWS needs a measure reference"));
    };
    let measure = ev.model().measures.get(*index).ok_or_else(|| EvalError::new("DETAILROWS measure is out of range"))?;
    let home = measure.table.as_ref().ok_or_else(|| EvalError::new(format!("measure `{}` has no home table", measure.name)))?;
    let table = ev.model().table_index(home).ok_or_else(|| EvalError::new(format!("unknown table `{home}`")))?;
    Ok(ev.table(table, &ctx.filters))
}

/// `FILTERS(<Column>)`: the values a filter applies *directly* to the column. Cross-filters do
/// not count, and values that do not exist in the model are not considered filters
/// (<https://dax.guide/filters/>). Without a direct filter the whole column is returned, which is
/// also what the engine does for `COUNTROWS(FILTERS(col))`.
fn filters(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let arg = args.first().ok_or_else(|| EvalError::new("FILTERS needs a column reference"))?;
    let BoundExpr::Column { table, column } = arg else {
        return Err(EvalError::new("FILTERS needs a column reference"));
    };
    let name = ev
        .model()
        .tables
        .get(*table)
        .and_then(|model_table| model_table.columns.get(*column))
        .map(|model_column| model_column.name.clone())
        .ok_or_else(|| EvalError::new("FILTERS column is out of range"))?;
    let source = ev.table(*table, &FilterContext::default());
    let direct = ctx.filters.filters.get(&(*table, *column));
    let mut seen = BTreeSet::new();
    let rows = source
        .column(*column)
        .into_iter()
        .filter(|value| direct.is_none_or(|keys| keys.contains(&ScalarKey::from(value))))
        .filter(|value| seen.insert(ScalarKey::from(value)))
        .map(|value| vec![value])
        .collect();
    Ok(TableValue::new(vec![TableColumn { name }], rows).with_lineage(*table))
}

/// `GENERATE`/`GENERATEALL`: evaluates the second table expression in the row context of every
/// first-table row. `GENERATE` drops rows whose second table is empty; `GENERATEALL` keeps them
/// with blanks in the second table's columns.
fn generate(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, keep_empty: bool) -> Result<TableValue, EvalError> {
    let first = args.first().ok_or_else(|| EvalError::new("GENERATE needs two table arguments"))?;
    let second = args.get(1).ok_or_else(|| EvalError::new("GENERATE needs two table arguments"))?;
    let left = ev.eval_table(first, ctx)?;
    let mut evaluated = Vec::with_capacity(left.rows.len());
    for row in &left.rows {
        ctx.rows.push(frame_for(ev, &left, row));
        let right = ev.eval_table(second, ctx);
        ctx.rows.pop();
        evaluated.push((row.clone(), right?));
    }

    // The second table's schema is the same for every row. Take it from the first evaluation that
    // produced columns (an empty result still carries its columns).
    let mut schema: Option<Vec<TableColumn>> = None;
    for (_, right) in &evaluated {
        if right.columns.is_empty() {
            continue;
        }
        match &schema {
            None => {
                for column in &right.columns {
                    if left.columns.iter().any(|candidate| candidate.name.eq_ignore_ascii_case(&column.name)) {
                        return Err(EvalError::new(format!("GENERATE column `{}` appears in both tables", column.name)));
                    }
                }
                schema = Some(right.columns.clone());
            }
            Some(known) => {
                let same = known.len() == right.columns.len() && known.iter().zip(&right.columns).all(|(a, b)| a.name.eq_ignore_ascii_case(&b.name));
                if !same {
                    return Err(EvalError::new("GENERATE second table changed its columns between rows"));
                }
            }
        }
    }

    let fill = schema.as_ref().map_or(0, Vec::len);
    let mut output_rows = 0usize;
    for (_, right) in &evaluated {
        output_rows = output_rows.saturating_add(if right.rows.is_empty() && keep_empty { 1 } else { right.rows.len() });
    }
    if output_rows > MAX_GENERATED_ROWS {
        return Err(EvalError::new("GENERATE result is too large"));
    }

    let mut columns = left.columns.clone();
    if let Some(schema) = &schema {
        columns.extend(schema.iter().cloned());
    }
    let mut rows = Vec::with_capacity(output_rows);
    for (left_row, right) in evaluated {
        if right.rows.is_empty() {
            if keep_empty {
                let mut row = left_row;
                row.extend(vec![Scalar::Blank; fill]);
                rows.push(row);
            }
            continue;
        }
        for right_row in right.rows {
            let mut row = left_row.clone();
            row.extend(right_row);
            rows.push(row);
        }
    }
    Ok(TableValue::new(columns, rows))
}

/// `GENERATESERIES(<Start>, <End> [, <Increment>])`: a `Value` column from start to end in
/// positive increments (default 1). An end below the start yields an empty table.
fn generate_series(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let call_args = super::support::args("GENERATESERIES", &values);
    let start = call_args.number(0)?;
    let end = call_args.number(1)?;
    let increment = call_args.optional_number(2, 1.0)?;
    if !start.is_finite() || !end.is_finite() || !increment.is_finite() {
        return Err(EvalError::new("GENERATESERIES arguments must be finite numbers"));
    }
    if increment <= 0.0 {
        return Err(EvalError::new("GENERATESERIES increment must be a positive value"));
    }
    let columns = vec![TableColumn { name: "Value".into() }];
    if end < start {
        return Ok(TableValue::new(columns, Vec::new()));
    }
    let steps = ((end - start) / increment).floor();
    if !steps.is_finite() || steps < 0.0 || steps > MAX_GENERATED_ROWS as f64 {
        return Err(EvalError::new("GENERATESERIES result is too large"));
    }
    let count = steps as usize + 1;
    if count > MAX_GENERATED_ROWS {
        return Err(EvalError::new("GENERATESERIES result is too large"));
    }
    // The result type follows the arguments: dates stay dates, all-integer arguments stay
    // integers, anything else is a decimal.
    let date = matches!(values.first(), Some(Scalar::DateTime(_)));
    let integer = matches!(values.first(), Some(Scalar::Integer(_)))
        && matches!(values.get(1), Some(Scalar::Integer(_)))
        && values.get(2).is_none_or(|value| matches!(value, Scalar::Integer(_)));
    let mut rows = Vec::with_capacity(count);
    for step in 0..count {
        let value = start + step as f64 * increment;
        let scalar = if date {
            Scalar::DateTime(value)
        } else if integer {
            Scalar::Integer(value.round() as i64)
        } else {
            Scalar::Decimal(value)
        };
        rows.push(vec![scalar]);
    }
    Ok(TableValue::new(columns, rows))
}

/// Matches the natural-join columns and returns the matched pairs plus the right-only columns.
///
/// The engine compares columns with the same name, the same data type and a compatible lineage,
/// using strict (no coercion) semantics. Lineage is tracked per table here: two model tables with
/// a same-named column have different lineage and therefore raise an error, while a column with
/// lineage and one without are ignored as join keys (as documented for the natural joins).
#[allow(clippy::type_complexity)]
// The pair is (matching column index pairs, ones holding a common column on the right).
fn natural_join_columns(left: &TableValue, right: &TableValue) -> Result<(Vec<(usize, usize)>, Vec<usize>), EvalError> {
    let mut common = Vec::new();
    let mut right_common = vec![false; right.columns.len()];
    for (left_index, left_column) in left.columns.iter().enumerate() {
        let Some(right_index) = right.columns.iter().position(|candidate| candidate.name.eq_ignore_ascii_case(&left_column.name)) else {
            continue;
        };
        match (left.lineage, right.lineage) {
            (Some(a), Some(b)) if a == b => {}
            (Some(_), Some(_)) => {
                return Err(EvalError::new(format!("natural join column `{}` has a different lineage on each side", left_column.name)));
            }
            (Some(_), None) | (None, Some(_)) => continue,
            (None, None) => {}
        }
        right_common[right_index] = true;
        common.push((left_index, right_index));
    }
    if common.is_empty() {
        return Err(EvalError::new("natural join requires at least one column with a matching name"));
    }
    let right_only = (0..right.columns.len()).filter(|index| !right_common[*index]).collect();
    Ok((common, right_only))
}

/// `NATURALINNERJOIN`/`NATURALLEFTOUTERJOIN`. The result keeps every left column and appends the
/// right columns that did not take part in the join.
fn natural_join(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, outer: bool) -> Result<TableValue, EvalError> {
    let left_expr = args.first().ok_or_else(|| EvalError::new("natural join needs two tables"))?;
    let right_expr = args.get(1).ok_or_else(|| EvalError::new("natural join needs two tables"))?;
    let left = ev.eval_table(left_expr, ctx)?;
    let right = ev.eval_table(right_expr, ctx)?;
    let (common, right_only) = natural_join_columns(&left, &right)?;

    let mut columns = left.columns.clone();
    columns.extend(right_only.iter().map(|index| right.columns[*index].clone()));

    let mut matches: BTreeMap<Vec<ScalarKey>, Vec<usize>> = BTreeMap::new();
    for (index, row) in right.rows.iter().enumerate() {
        let key: Vec<ScalarKey> =
            common.iter().map(|(_, right_column)| row.get(*right_column).map(ScalarKey::from).unwrap_or(ScalarKey::Blank)).collect();
        matches.entry(key).or_default().push(index);
    }

    let mut rows = Vec::new();
    for left_row in &left.rows {
        let key: Vec<ScalarKey> =
            common.iter().map(|(left_column, _)| left_row.get(*left_column).map(ScalarKey::from).unwrap_or(ScalarKey::Blank)).collect();
        match matches.get(&key) {
            Some(indices) => {
                for index in indices {
                    let mut row = left_row.clone();
                    row.extend(right_only.iter().map(|column| right.rows[*index].get(*column).cloned().unwrap_or(Scalar::Blank)));
                    rows.push(row);
                }
            }
            None if outer => {
                let mut row = left_row.clone();
                row.extend(vec![Scalar::Blank; right_only.len()]);
                rows.push(row);
            }
            None => {}
        }
        if rows.len() > MAX_GENERATED_ROWS {
            return Err(EvalError::new("natural join result is too large"));
        }
    }
    Ok(TableValue::new(columns, rows))
}

/// `NATURALJOINUSAGE(<Table>)` only has meaning as a value filter of `SUMMARIZECOLUMNS`, where it
/// marks the table as participating in the natural join. Standalone, the argument is already the
/// table that the filter would add, so returning it is the identity behavior.
fn natural_join_usage(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let arg = args.first().ok_or_else(|| EvalError::new("NATURALJOINUSAGE needs a table expression"))?;
    ev.eval_table(arg, ctx)
}

/// One point of the sampled axis: its position in the source table plus the numeric axis and
/// measure values.
struct AxisPoint {
    row: usize,
    axis: f64,
    measure: f64,
}

/// Picks, inside every bin of the global axis range, the rows holding the local minimum and the
/// local maximum of the measure for each series.
fn sample_bins(groups: &[Vec<AxisPoint>], bins: usize, min_axis: f64, max_axis: f64) -> Vec<Vec<usize>> {
    let width = max_axis - min_axis;
    groups
        .iter()
        .map(|points| {
            let mut buckets: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
            for (index, point) in points.iter().enumerate() {
                let raw = if width > 0.0 { ((point.axis - min_axis) / width * bins as f64).floor() } else { 0.0 };
                let bin = (raw.max(0.0) as usize).min(bins.saturating_sub(1));
                buckets
                    .entry(bin)
                    .and_modify(|(min_index, max_index)| {
                        if point.measure < points[*min_index].measure {
                            *min_index = index;
                        }
                        if point.measure > points[*max_index].measure {
                            *max_index = index;
                        }
                    })
                    .or_insert((index, index));
            }
            let mut selected = Vec::with_capacity(buckets.len() * 2);
            for (min_index, max_index) in buckets.values() {
                selected.push(points[*min_index].row);
                if min_index != max_index {
                    selected.push(points[*max_index].row);
                }
            }
            selected
        })
        .collect()
}

/// `SAMPLEAXISWITHLOCALMINMAX(<Size>, <Table>, <Axis>, <Measure>, <MinResolution>, ...)`.
///
/// The axis is divided into `Size` equal bins over its global range; every bin keeps the rows with
/// the local minimum and maximum measure for each series. `MinResolution`/`MaxResolution` bound
/// the number of rows selected per series, and the bin count is adjusted for a few iterations to
/// satisfy them. Series selection accepts the documented `NONE`/`ALPHABETICAL` criteria and the
/// ascending/descending order. The remaining engine-only tuning arguments are validated and
/// bounded but do not change the result.
fn sample_axis_with_local_min_max(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 5 {
        return Err(EvalError::new("SAMPLEAXISWITHLOCALMINMAX needs at least five arguments"));
    }
    let size_value = ev.eval(&args[0], ctx)?.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX Size must be numeric"))?;
    if !size_value.is_finite() || size_value < 1.0 {
        return Err(EvalError::new("SAMPLEAXISWITHLOCALMINMAX Size must be a positive integer"));
    }
    let size = (size_value.floor() as usize).min(MAX_GENERATED_ROWS);

    let table = ev.eval_table(&args[1], ctx)?;
    let axis_index = column_position(ev, &table, &args[2])?;
    let measure_expr = &args[3];
    let min_resolution_value =
        ev.eval(&args[4], ctx)?.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX MinResolution must be numeric"))?;
    if !min_resolution_value.is_finite() || min_resolution_value < 1.0 {
        return Err(EvalError::new("SAMPLEAXISWITHLOCALMINMAX MinResolution must be a positive integer"));
    }
    let min_resolution = (min_resolution_value.floor() as usize).min(MAX_GENERATED_ROWS);

    // Optional dynamic series is a column argument; any other trailing argument is an option.
    let dynamic_series = args.get(5).and_then(|expr| {
        let name = column_name(ev, expr)?;
        table.column_index(&name)
    });
    let mut option_index = if dynamic_series.is_some() { 6 } else { 5 };
    let mut take_option = |ctx: &mut Context| -> Option<Result<Scalar, EvalError>> {
        let value = args.get(option_index).map(|expr| ev.eval(expr, ctx));
        option_index += 1;
        value
    };

    let mut criteria = String::from("NONE");
    let mut ascending = true;
    let mut max_resolution: Option<usize> = None;
    let mut max_series: Option<usize> = None;
    let mut max_iterations = 10usize;

    if let Some(value) = take_option(ctx) {
        let value = value?;
        if !value.is_blank() {
            criteria = value.to_text().to_ascii_uppercase();
            if criteria != "NONE" && criteria != "ALPHABETICAL" {
                return Err(EvalError::new("SAMPLEAXISWITHLOCALMINMAX DynamicSeriesSelectionCriteria must be NONE or ALPHABETICAL"));
            }
        }
    }
    if let Some(value) = take_option(ctx) {
        let value = value?;
        if !value.is_blank() {
            ascending = value.to_bool().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX DynamicSeriesSelectionOrder must be Boolean"))?;
        }
    }
    if let Some(value) = take_option(ctx) {
        let value = value?;
        if !value.is_blank() {
            let number = value.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX MaxResolution must be numeric"))?;
            let number = (number.floor() as usize).max(1);
            max_resolution = Some(number);
        }
    }
    if let Some(value) = take_option(ctx) {
        let value = value?;
        if !value.is_blank() {
            let number = value.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX MaxDynamicSeries must be numeric"))?;
            max_series = Some((number.floor() as usize).max(1));
        }
    }
    if let Some(value) = take_option(ctx) {
        let value = value?;
        if !value.is_blank() {
            let number = value.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX MaxIterations must be numeric"))?;
            max_iterations = (number.floor() as usize).clamp(1, 10_000);
        }
    }

    // Evaluate the axis and measure once per row; a blank axis row cannot be binned and is
    // ignored, matching the chart behavior of dropping empty points.
    let mut point_rows: Vec<Option<AxisPoint>> = Vec::with_capacity(table.rows.len());
    for (row_index, row) in table.rows.iter().enumerate() {
        let axis = row.get(axis_index).filter(|value| !value.is_blank()).and_then(Scalar::to_number);
        let Some(axis) = axis else {
            point_rows.push(None);
            continue;
        };
        ctx.rows.push(frame_for(ev, &table, row));
        let measure = ev.eval(measure_expr, ctx);
        ctx.rows.pop();
        let measure = measure?.to_number().ok_or_else(|| EvalError::new("SAMPLEAXISWITHLOCALMINMAX Measure must be numeric"))?;
        point_rows.push(Some(AxisPoint { row: row_index, axis, measure }));
    }

    // Partition the points by the dynamic series, in first-seen order.
    let mut groups: Vec<(ScalarKey, Vec<AxisPoint>)> = Vec::new();
    for (row_index, point) in point_rows.into_iter().enumerate() {
        let Some(point) = point else { continue };
        let key = match dynamic_series {
            Some(column) => table.rows.get(row_index).and_then(|row| row.get(column)).map(ScalarKey::from).unwrap_or(ScalarKey::Blank),
            None => ScalarKey::Blank,
        };
        match groups.iter_mut().find(|(candidate, _)| *candidate == key) {
            Some((_, points)) => points.push(point),
            None => groups.push((key, vec![point])),
        }
    }
    if criteria == "ALPHABETICAL" {
        groups.sort_by(|a, b| a.0.cmp(&b.0));
    }
    if !ascending {
        groups.reverse();
    }
    if let Some(limit) = max_series {
        groups.truncate(limit);
    }
    let groups: Vec<Vec<AxisPoint>> = groups.into_iter().map(|(_, points)| points).collect();
    if groups.is_empty() {
        return Ok(TableValue::new(table.columns, Vec::new()).with_lineage_if(table.lineage));
    }
    if max_resolution.is_some_and(|limit| limit < min_resolution) {
        return Err(EvalError::new("SAMPLEAXISWITHLOCALMINMAX MaxResolution cannot be lower than MinResolution"));
    }

    let (Some(min_axis), Some(max_axis)) =
        (groups.iter().flatten().map(|point| point.axis).reduce(f64::min), groups.iter().flatten().map(|point| point.axis).reduce(f64::max))
    else {
        return Ok(TableValue::new(table.columns, Vec::new()).with_lineage_if(table.lineage));
    };

    let row_total: usize = groups.iter().map(Vec::len).sum();
    let mut bin_count = size.min(row_total.max(1)).max(1);
    let mut selected = sample_bins(&groups, bin_count, min_axis, max_axis);
    for _ in 0..max_iterations {
        let counts: Vec<usize> = selected.iter().map(Vec::len).collect();
        let smallest = counts.iter().copied().min().unwrap_or(0);
        let largest = counts.iter().copied().max().unwrap_or(0);
        if smallest < min_resolution && bin_count < row_total {
            bin_count = bin_count.saturating_mul(2).min(row_total);
            selected = sample_bins(&groups, bin_count, min_axis, max_axis);
            continue;
        }
        if max_resolution.is_some_and(|limit| largest > limit) && bin_count > 1 {
            bin_count /= 2;
            selected = sample_bins(&groups, bin_count, min_axis, max_axis);
            continue;
        }
        break;
    }

    let mut rows: Vec<usize> = selected.into_iter().flatten().collect();
    rows.sort_unstable();
    rows.dedup();
    let rows = rows.into_iter().filter_map(|index| table.rows.get(index).cloned()).collect();
    Ok(TableValue::new(table.columns, rows).with_lineage_if(table.lineage))
}

/// `SUBSTITUTEWITHINDEX(<Table>, <Name>, <SemiJoinIndexTable>, <Expression> [, <Order>] ...)`.
///
/// Semijoins the two tables on their common columns, replacing those columns with a zero-based
/// index taken from the position of the matching row in the second table. The second table is
/// ordered by the supplied expressions before the index is assigned.
fn substitute_with_index(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 4 {
        return Err(EvalError::new("SUBSTITUTEWITHINDEX needs at least four arguments"));
    }
    let table = ev.eval_table(&args[0], ctx)?;
    let name = match args.get(1) {
        Some(BoundExpr::Literal(Literal::String(name))) => name.clone(),
        _ => return Err(EvalError::new("SUBSTITUTEWITHINDEX needs a literal index column name")),
    };
    let index_table = ev.eval_table(&args[2], ctx)?;
    let (common, _) = natural_join_columns(&table, &index_table)?;

    // The remaining arguments alternate an order expression and an optional order flag.
    let mut order_specs: Vec<(&BoundExpr, bool)> = Vec::new();
    let mut index = 3;
    while index < args.len() {
        let ascending = match args.get(index + 1) {
            Some(expr) => {
                let value = ev.eval(expr, ctx)?;
                value.to_bool().ok_or_else(|| EvalError::new("SUBSTITUTEWITHINDEX order must be Boolean"))?
            }
            None => true,
        };
        order_specs.push((&args[index], ascending));
        index += 2;
    }

    let mut order: Vec<usize> = (0..index_table.rows.len()).collect();
    if !order_specs.is_empty() {
        let mut keys: Vec<Vec<Scalar>> = Vec::with_capacity(index_table.rows.len());
        for row in &index_table.rows {
            ctx.rows.push(frame_for(ev, &index_table, row));
            let row_keys = order_specs.iter().map(|(expr, _)| ev.eval(expr, ctx)).collect::<Result<Vec<_>, _>>();
            ctx.rows.pop();
            keys.push(row_keys?);
        }
        order.sort_by(|a, b| {
            for (position, (_, ascending)) in order_specs.iter().enumerate() {
                let ordering = keys[*a].get(position).and_then(|left| keys[*b].get(position).and_then(|right| left.compare(right)));
                match ordering.unwrap_or(Ordering::Equal) {
                    Ordering::Equal => {}
                    other => return if *ascending { other } else { other.reverse() },
                }
            }
            Ordering::Equal
        });
    }

    let mut positions: BTreeMap<Vec<ScalarKey>, usize> = BTreeMap::new();
    for (position, row_index) in order.iter().enumerate() {
        let Some(row) = index_table.rows.get(*row_index) else { continue };
        let key: Vec<ScalarKey> =
            common.iter().map(|(_, right_column)| row.get(*right_column).map(ScalarKey::from).unwrap_or(ScalarKey::Blank)).collect();
        positions.entry(key).or_insert(position);
    }

    let dropped: BTreeSet<usize> = common.iter().map(|(left_column, _)| *left_column).collect();
    let mut columns: Vec<TableColumn> =
        table.columns.iter().enumerate().filter(|(column, _)| !dropped.contains(column)).map(|(_, column)| column.clone()).collect();
    columns.push(TableColumn { name });

    let mut rows = Vec::new();
    for row in &table.rows {
        let key: Vec<ScalarKey> =
            common.iter().map(|(left_column, _)| row.get(*left_column).map(ScalarKey::from).unwrap_or(ScalarKey::Blank)).collect();
        let Some(position) = positions.get(&key) else { continue };
        let mut output: Vec<Scalar> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(column, _)| !dropped.contains(column))
            .filter_map(|(index, _)| row.get(index).cloned())
            .collect();
        output.push(Scalar::Integer(*position as i64));
        rows.push(output);
    }
    Ok(TableValue::new(columns, rows))
}
