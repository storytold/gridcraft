//! Statistical aggregates and iterators: combinatorics, geometric mean, median, percentiles,
//! variance/standard deviation and ranking.
//!
//! Numeric semantics follow Excel/DAX: the column forms ignore BLANK, logical and text values,
//! while the iterator (X) forms evaluate the expression in row context. MEDIANX and the
//! PERCENTILEX forms include BLANK results (they sort before numbers and stay BLANK when the
//! requested position lands on them); MEDIAN and PERCENTILE.EXC/INC ignore BLANK rows.
#![allow(unused_imports)]

use std::cmp::Ordering;

use super::super::support::{eval_all, frame_for};
use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "COMBIN" => combinatoric(ev, args, ctx, Combinatoric::Combin),
        "COMBINA" => combinatoric(ev, args, ctx, Combinatoric::Combina),
        "PERMUT" => combinatoric(ev, args, ctx, Combinatoric::Permut),
        "GEOMEAN" => column_statistic(ev, args, ctx, Statistic::GeoMean),
        "GEOMEANX" => iterator_statistic(ev, args, ctx, Statistic::GeoMean),
        "MEDIAN" => column_statistic(ev, args, ctx, Statistic::Median),
        "MEDIANX" => iterator_statistic(ev, args, ctx, Statistic::Median),
        "PERCENTILE.EXC" => percentile_column(ev, args, ctx, true),
        "PERCENTILE.INC" => percentile_column(ev, args, ctx, false),
        "PERCENTILEX.EXC" => percentile_iterator(ev, args, ctx, true),
        "PERCENTILEX.INC" => percentile_iterator(ev, args, ctx, false),
        "STDEV.P" => column_statistic(ev, args, ctx, Statistic::StdevP),
        "STDEV.S" => column_statistic(ev, args, ctx, Statistic::StdevS),
        "STDEVX.P" => iterator_statistic(ev, args, ctx, Statistic::StdevP),
        "STDEVX.S" => iterator_statistic(ev, args, ctx, Statistic::StdevS),
        "VAR.P" => column_statistic(ev, args, ctx, Statistic::VarP),
        "VAR.S" => column_statistic(ev, args, ctx, Statistic::VarS),
        "VARX.P" => iterator_statistic(ev, args, ctx, Statistic::VarP),
        "VARX.S" => iterator_statistic(ev, args, ctx, Statistic::VarS),
        "RANK.EQ" => rank_eq(ev, args, ctx),
        "RANKX" => rank_x(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "SAMPLE" => Some(sample(ev, args, ctx)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Combinatorics
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Combinatoric {
    Combin,
    Combina,
    Permut,
}

impl Combinatoric {
    fn name(self) -> &'static str {
        match self {
            Combinatoric::Combin => "COMBIN",
            Combinatoric::Combina => "COMBINA",
            Combinatoric::Permut => "PERMUT",
        }
    }
}

fn combinatoric(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, kind: Combinatoric) -> Result<Scalar, EvalError> {
    let name = kind.name();
    let values = eval_all(ev, args, ctx)?;
    let arguments = super::super::support::args(name, &values);
    let number = rounded_argument(arguments.number(0)?, name, 1)?;
    let chosen = rounded_argument(arguments.number(1)?, name, 2)?;
    match kind {
        Combinatoric::Combin => {
            if number < 0 || chosen < 0 || number < chosen {
                return Err(EvalError::new("COMBIN requires Number >= Number_chosen >= 0"));
            }
            combinations_to_integer(number, chosen, name)
        }
        Combinatoric::Combina => {
            if number < 0 || chosen < 0 || number < chosen {
                return Err(EvalError::new("COMBINA requires Number >= Number_chosen >= 0"));
            }
            // C(N+M-1, M); choosing nothing from nothing is the single empty selection.
            if number == 0 && chosen == 0 {
                return Ok(Scalar::Integer(1));
            }
            combinations_to_integer(number + chosen - 1, chosen, name)
        }
        Combinatoric::Permut => {
            if number <= 0 || chosen < 0 || chosen > number {
                return Err(EvalError::new("PERMUT requires Number > 0, Number_chosen >= 0 and Number >= Number_chosen"));
            }
            if chosen > 1_000_000 {
                return Err(EvalError::new("PERMUT result is too large"));
            }
            let mut result = 1.0_f64;
            for index in 0..chosen {
                result *= (number - index) as f64;
                if !result.is_finite() {
                    return Err(EvalError::new("PERMUT result is too large"));
                }
            }
            Ok(Scalar::Decimal(result))
        }
    }
}

/// Rounds like Excel: `COMBIN(8.9, 3.9)` counts 9 choose 4, not 8 choose 3.
fn rounded_argument(value: f64, name: &str, position: usize) -> Result<i64, EvalError> {
    if !value.is_finite() || value.abs() > 9.0e15 {
        return Err(EvalError::new(format!("{name} argument {position} must be a finite number")));
    }
    Ok(value.round() as i64)
}

/// Exact combination count for small k, accumulated in `u128` so integer results within the
/// `Integer` range are not rounded by floating-point arithmetic.
fn combinations_to_integer(number: i64, chosen: i64, name: &str) -> Result<Scalar, EvalError> {
    let k = chosen.min(number - chosen);
    if k > 1_000_000 {
        return Err(EvalError::new(format!("{name} result is too large")));
    }
    let n = number as u128;
    let k = k as u128;
    let mut result: u128 = 1;
    for index in 1..=k {
        // C(n, k) partial products are always divisible by `index`.
        result = result.checked_mul(n - k + index).ok_or_else(|| EvalError::new(format!("{name} result is too large")))? / index;
    }
    let value = i64::try_from(result).map_err(|_| EvalError::new(format!("{name} result is too large")))?;
    Ok(Scalar::Integer(value))
}

// ---------------------------------------------------------------------------------------------
// Value collection
// ---------------------------------------------------------------------------------------------

/// One observation of a statistical function. Blanks are kept as their own variant because the
/// iterator forms include them while the column forms filter them out first.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StatValue {
    Blank,
    Number(f64),
}

impl StatValue {
    fn from_scalar(value: &Scalar) -> Option<Self> {
        match value {
            Scalar::Blank => Some(StatValue::Blank),
            // Logical values and text are ignored by MEDIANX, PERCENTILEX and the X statistics.
            Scalar::Boolean(_) | Scalar::String(_) => None,
            _ => value.to_number().filter(|number| number.is_finite()).map(StatValue::Number),
        }
    }

    fn number(self) -> f64 {
        match self {
            StatValue::Blank => 0.0,
            StatValue::Number(number) => number,
        }
    }
}

fn stat_order(left: &StatValue, right: &StatValue) -> Ordering {
    match (left, right) {
        (StatValue::Blank, StatValue::Blank) => Ordering::Equal,
        (StatValue::Blank, _) => Ordering::Less,
        (_, StatValue::Blank) => Ordering::Greater,
        (StatValue::Number(a), StatValue::Number(b)) => a.total_cmp(b),
    }
}

/// Numeric column values, ignoring BLANK, logical and text values.
fn numeric_values(values: &[Scalar]) -> Vec<f64> {
    values
        .iter()
        .filter_map(|value| match value {
            Scalar::Blank | Scalar::Boolean(_) | Scalar::String(_) => None,
            _ => value.to_number().filter(|number| number.is_finite()),
        })
        .collect()
}

/// Numeric values of a column argument, honouring the current filter context.
fn column_values(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, name: &str) -> Result<Vec<Scalar>, EvalError> {
    match expr {
        BoundExpr::Column { table, column } => Ok(ev.table(*table, &ctx.filters).column(*column)),
        BoundExpr::UnqualifiedColumn { name: column } => {
            let tables = ev.model().tables_with_column(column);
            if tables.len() != 1 {
                return Err(EvalError::new(format!("{name} column `[{column}]` is ambiguous")));
            }
            let table = tables[0];
            let index = ev.model().column_index(table, column).ok_or_else(|| EvalError::new(format!("{name} is missing column `[{column}]`")))?;
            Ok(ev.table(table, &ctx.filters).column(index))
        }
        _ => Err(EvalError::new(format!("{name} needs a column reference"))),
    }
}

/// Evaluates the iterator's expression once per row of the table argument.
fn iterate_values(
    ev: &Evaluator<'_>,
    table_expr: &BoundExpr,
    value_expr: &BoundExpr,
    ctx: &mut Context,
    name: &str,
) -> Result<Vec<Scalar>, EvalError> {
    let table = ev.eval_table(table_expr, ctx).map_err(|error| EvalError::new(format!("{name}: {error}")))?;
    let mut values = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let value = ev.eval(value_expr, ctx);
        ctx.rows.pop();
        values.push(value?);
    }
    Ok(values)
}

// ---------------------------------------------------------------------------------------------
// GEOMEAN / MEDIAN / VAR / STDEV
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Statistic {
    GeoMean,
    Median,
    VarP,
    VarS,
    StdevP,
    StdevS,
}

impl Statistic {
    fn name(self) -> &'static str {
        match self {
            Statistic::GeoMean => "GEOMEAN",
            Statistic::Median => "MEDIAN",
            Statistic::VarP => "VAR.P",
            Statistic::VarS => "VAR.S",
            Statistic::StdevP => "STDEV.P",
            Statistic::StdevS => "STDEV.S",
        }
    }

    fn sample(self) -> bool {
        matches!(self, Statistic::VarS | Statistic::StdevS)
    }

    fn standard_deviation(self) -> bool {
        matches!(self, Statistic::StdevP | Statistic::StdevS)
    }
}

fn column_statistic(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, statistic: Statistic) -> Result<Scalar, EvalError> {
    let name = statistic.name();
    let column = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    // Storage supplies the same finite observations straight from the column; `numeric_values`
    // stays the fallback for models that cannot be stored faithfully (ragged or promoted
    // columns).
    let numbers = match ev.storage_numeric_values(column, ctx) {
        Some(numbers) => numbers,
        None => numeric_values(&column_values(ev, column, ctx, name)?),
    };
    let observations: Vec<StatValue> = numbers.into_iter().map(StatValue::Number).collect();
    statistic_result(statistic, observations)
}

fn iterator_statistic(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, statistic: Statistic) -> Result<Scalar, EvalError> {
    let name = statistic.name();
    if args.len() < 2 {
        return Err(EvalError::new(format!("{name} needs a table and an expression")));
    }
    let values = iterate_values(ev, &args[0], &args[1], ctx, name)?;
    let observations: Vec<StatValue> = values.iter().filter_map(StatValue::from_scalar).collect();
    statistic_result(statistic, observations)
}

fn statistic_result(statistic: Statistic, mut observations: Vec<StatValue>) -> Result<Scalar, EvalError> {
    let name = statistic.name();
    match statistic {
        Statistic::Median => {
            if observations.is_empty() {
                return Ok(Scalar::Blank);
            }
            observations.sort_by(stat_order);
            let count = observations.len();
            if count % 2 == 1 {
                return Ok(scalar_from_stat(&observations[count / 2]));
            }
            let lower = observations[count / 2 - 1];
            let upper = observations[count / 2];
            match (lower, upper) {
                (StatValue::Blank, StatValue::Blank) => Ok(Scalar::Blank),
                _ => Ok(Scalar::Decimal((lower.number() + upper.number()) / 2.0)),
            }
        }
        Statistic::GeoMean => {
            // dax.guide/Microsoft: only positive numbers contribute; the geometric mean is
            // undefined (BLANK) when no positive observation exists.
            let mut log_sum = 0.0;
            let mut count = 0usize;
            for observation in &observations {
                let StatValue::Number(number) = *observation else { continue };
                if number <= 0.0 {
                    continue;
                }
                log_sum += number.ln();
                count += 1;
            }
            if count == 0 {
                return Ok(Scalar::Blank);
            }
            let mean = (log_sum / count as f64).exp();
            if !mean.is_finite() {
                return Err(EvalError::new("GEOMEAN result is not a finite number"));
            }
            Ok(Scalar::Decimal(mean))
        }
        _ => {
            let numbers: Vec<f64> = observations
                .iter()
                .filter_map(|observation| match observation {
                    StatValue::Number(number) => Some(*number),
                    StatValue::Blank => None,
                })
                .collect();
            if numbers.len() < 2 {
                return Err(EvalError::new(format!("{name} requires at least two non-blank numeric values")));
            }
            let count = numbers.len() as f64;
            let mean = numbers.iter().sum::<f64>() / count;
            let sum_squares: f64 = numbers.iter().map(|number| (number - mean) * (number - mean)).sum();
            let variance = if statistic.sample() { sum_squares / (count - 1.0) } else { sum_squares / count };
            let result = if statistic.standard_deviation() { variance.sqrt() } else { variance };
            if !result.is_finite() {
                return Err(EvalError::new(format!("{name} result is not a finite number")));
            }
            Ok(Scalar::Decimal(result))
        }
    }
}

fn scalar_from_stat(value: &StatValue) -> Scalar {
    match value {
        StatValue::Blank => Scalar::Blank,
        StatValue::Number(number) => Scalar::Decimal(*number),
    }
}

// ---------------------------------------------------------------------------------------------
// Percentiles
// ---------------------------------------------------------------------------------------------

fn percentile_column(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, exclusive: bool) -> Result<Scalar, EvalError> {
    let name = if exclusive { "PERCENTILE.EXC" } else { "PERCENTILE.INC" };
    let column = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    let values = column_values(ev, column, ctx, name)?;
    let k = percentile_k(ev, args, 1, ctx, name)?;
    let mut observations: Vec<StatValue> = numeric_values(&values).into_iter().map(StatValue::Number).collect();
    percentile_result(&mut observations, k, exclusive, name)
}

fn percentile_iterator(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, exclusive: bool) -> Result<Scalar, EvalError> {
    let name = if exclusive { "PERCENTILEX.EXC" } else { "PERCENTILEX.INC" };
    if args.len() < 3 {
        return Err(EvalError::new(format!("{name} needs a table, an expression and K")));
    }
    let k = percentile_k(ev, args, 2, ctx, name)?;
    let values = iterate_values(ev, &args[0], &args[1], ctx, name)?;
    let mut observations: Vec<StatValue> = values.iter().filter_map(StatValue::from_scalar).collect();
    percentile_result(&mut observations, k, exclusive, name)
}

fn percentile_k(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context, name: &str) -> Result<f64, EvalError> {
    let expr = args.get(index).ok_or_else(|| EvalError::new(format!("{name} needs K")))?;
    let value = ev.eval(expr, ctx)?;
    value.to_number().filter(|number| number.is_finite()).ok_or_else(|| EvalError::new(format!("{name} K must be numeric")))
}

fn percentile_result(observations: &mut [StatValue], k: f64, exclusive: bool, name: &str) -> Result<Scalar, EvalError> {
    let count = observations.len();
    if count == 0 {
        return Ok(Scalar::Blank);
    }
    if !(0.0..=1.0).contains(&k) {
        return Err(EvalError::new(format!("{name} K must be between 0 and 1")));
    }
    if exclusive {
        let lower = 1.0 / (count as f64 + 1.0);
        let upper = count as f64 / (count as f64 + 1.0);
        // A tiny tolerance keeps exact boundaries such as K = 1/(n+1) from failing on rounding.
        let tolerance = 1e-12;
        if k < lower - tolerance || k > upper + tolerance {
            return Err(EvalError::new(format!("{name} K must be between {lower} and {upper} for {count} values")));
        }
    }
    observations.sort_by(stat_order);
    let position = if exclusive { k * (count as f64 + 1.0) - 1.0 } else { k * (count as f64 - 1.0) };
    let position = position.clamp(0.0, count as f64 - 1.0);
    let lower_index = position.floor() as usize;
    let upper_index = position.ceil() as usize;
    let lower = observations[lower_index];
    if lower_index == upper_index {
        return Ok(scalar_from_stat(&lower));
    }
    let upper = observations[upper_index.min(count - 1)];
    let fraction = position - position.floor();
    match (lower, upper) {
        (StatValue::Blank, StatValue::Blank) => Ok(Scalar::Blank),
        _ => Ok(Scalar::Decimal(lower.number() * (1.0 - fraction) + upper.number() * fraction)),
    }
}

// ---------------------------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    fn from_scalar(value: Option<&Scalar>, name: &str) -> Result<Self, EvalError> {
        let Some(value) = value else { return Ok(SortOrder::Descending) };
        if value.is_blank() {
            return Ok(SortOrder::Descending);
        }
        if let Scalar::String(text) = value {
            let text = text.trim();
            if text.eq_ignore_ascii_case("ASC") || text.eq_ignore_ascii_case("TRUE") {
                return Ok(SortOrder::Ascending);
            }
            if text.eq_ignore_ascii_case("DESC") || text.eq_ignore_ascii_case("FALSE") {
                return Ok(SortOrder::Descending);
            }
            return Err(EvalError::new(format!("{name} Order must be 0, 1, ASC or DESC")));
        }
        match value.to_number() {
            Some(number) => {
                if number == 1.0 {
                    Ok(SortOrder::Ascending)
                } else if number == 0.0 {
                    Ok(SortOrder::Descending)
                } else {
                    Err(EvalError::new(format!("{name} Order must be 0, 1, ASC or DESC")))
                }
            }
            None => Err(EvalError::new(format!("{name} Order must be 0, 1, ASC or DESC"))),
        }
    }

    fn better(self, candidate: f64, reference: f64) -> bool {
        match self {
            SortOrder::Ascending => candidate < reference,
            SortOrder::Descending => candidate > reference,
        }
    }
}

fn rank_of(value: f64, values: &[f64], order: SortOrder, dense: bool) -> i64 {
    let mut better = Vec::new();
    for candidate in values {
        if order.better(*candidate, value) && !better.contains(candidate) {
            better.push(*candidate);
        }
    }
    if dense {
        1 + better.len() as i64
    } else {
        let count = values.iter().filter(|candidate| order.better(**candidate, value)).count();
        1 + count as i64
    }
}

fn rank_eq(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let name = "RANK.EQ";
    let value_expr = args.first().ok_or_else(|| EvalError::new("RANK.EQ needs a value"))?;
    let column_expr = args.get(1).ok_or_else(|| EvalError::new("RANK.EQ needs a column"))?;
    let value = ev.eval(value_expr, ctx)?;
    let order = match args.get(2) {
        Some(expr) => SortOrder::from_scalar(Some(&ev.eval(expr, ctx)?), name)?,
        None => SortOrder::Descending,
    };
    // A BLANK value can never match the column, so RANK.EQ returns BLANK (Microsoft docs).
    if value.is_blank() {
        return Ok(Scalar::Blank);
    }
    let Some(value_number) = value.to_number().filter(|number| number.is_finite()) else {
        return Err(EvalError::new("RANK.EQ value must be numeric"));
    };
    let column = column_values(ev, column_expr, ctx, name)?;
    let numbers = numeric_values(&column);
    if !numbers.contains(&value_number) {
        return Ok(Scalar::Blank);
    }
    Ok(Scalar::Integer(rank_of(value_number, &numbers, order, false)))
}

fn rank_x(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let name = "RANKX";
    if args.len() < 2 {
        return Err(EvalError::new("RANKX needs a table and an expression"));
    }
    let table = ev.eval_table(&args[0], ctx)?;
    let expression = &args[1];
    let mut values = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let value = ev.eval(expression, ctx);
        ctx.rows.pop();
        values.push(value?);
    }
    // A BLANK value or expression result counts as 0 for numeric ranks.
    let value = match args.get(2) {
        Some(expr) => ev.eval(expr, ctx)?.to_number().unwrap_or(0.0),
        // Without an explicit value, RANKX ranks the expression at the current row.
        None => ev.eval(expression, ctx)?.to_number().unwrap_or(0.0),
    };
    let order = match args.get(3) {
        Some(expr) => SortOrder::from_scalar(Some(&ev.eval(expr, ctx)?), name)?,
        None => SortOrder::Descending,
    };
    let dense = match args.get(4) {
        Some(expr) => {
            let value = ev.eval(expr, ctx)?;
            dense_ties(&value)?
        }
        None => false,
    };
    let numbers: Vec<f64> = values.iter().filter_map(|value| StatValue::from_scalar(value).map(StatValue::number)).collect();
    if !value.is_finite() {
        return Err(EvalError::new("RANKX value must be numeric"));
    }
    Ok(Scalar::Integer(rank_of(value, &numbers, order, dense)))
}

fn dense_ties(value: &Scalar) -> Result<bool, EvalError> {
    if value.is_blank() {
        return Ok(false);
    }
    match value {
        Scalar::String(text) if text.trim().eq_ignore_ascii_case("Dense") => Ok(true),
        Scalar::String(text) if text.trim().eq_ignore_ascii_case("Skip") => Ok(false),
        Scalar::Boolean(flag) => Ok(*flag),
        _ => {
            let number = value.to_number().ok_or_else(|| EvalError::new("RANKX Ties must be Dense or Skip"))?;
            if number == 1.0 {
                Ok(true)
            } else if number == 0.0 {
                Ok(false)
            } else {
                Err(EvalError::new("RANKX Ties must be Dense or Skip"))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// SAMPLE
// ---------------------------------------------------------------------------------------------

fn sample(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if args.len() < 3 {
        return Err(EvalError::new("SAMPLE needs a size, a table and an order-by expression"));
    }
    let size_value = ev.eval(&args[0], ctx)?;
    let size_number = size_value.to_number().filter(|number| number.is_finite()).ok_or_else(|| EvalError::new("SAMPLE size must be numeric"))?;
    let table = ev.eval_table(&args[1], ctx)?;

    // The optional trailing Order argument is a constant (0/1/FALSE/TRUE/ASC/DESC); everything
    // between the table and it is an order-by expression.
    let mut end = args.len();
    let mut order = SortOrder::Descending;
    if args.len() >= 4
        && let Some(direction) = constant_order(&args[args.len() - 1])
    {
        order = direction;
        end -= 1;
    }
    if end <= 2 {
        return Err(EvalError::new("SAMPLE needs at least one order-by expression"));
    }

    let mut keys: Vec<Vec<Scalar>> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let row_keys = args[2..end].iter().map(|expr| ev.eval(expr, ctx)).collect::<Result<Vec<_>, _>>();
        ctx.rows.pop();
        keys.push(row_keys?);
    }

    let mut indices: Vec<usize> = (0..table.rows.len()).collect();
    indices.sort_by(|left, right| {
        for (a, b) in keys[*left].iter().zip(keys[*right].iter()) {
            let ordering = a.compare(b).unwrap_or(Ordering::Equal);
            let ordering = if order == SortOrder::Descending { ordering.reverse() } else { ordering };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });

    let available = indices.len();
    let requested = if size_number <= 0.0 { 0 } else { size_number.trunc().min(available as f64) as usize };
    let mut rows = Vec::with_capacity(requested);
    if requested > 0 && available > 0 {
        for position in 0..requested {
            let index = if requested == 1 {
                0
            } else {
                let raw = position as f64 * (available - 1) as f64 / (requested - 1) as f64;
                raw.round().min((available - 1) as f64) as usize
            };
            if let Some(&row_index) = indices.get(index)
                && let Some(row) = table.rows.get(row_index)
            {
                rows.push(row.clone());
            }
        }
    }
    Ok(TableValue::from_rows(table.columns.clone(), rows, table.lineage))
}

/// Reads a literal Order argument without evaluating arbitrary expressions, so a column-valued
/// order-by expression is never mistaken for the sort direction.
fn constant_order(expr: &BoundExpr) -> Option<SortOrder> {
    match expr {
        BoundExpr::Literal(Literal::Integer(0)) => Some(SortOrder::Descending),
        BoundExpr::Literal(Literal::Integer(1)) => Some(SortOrder::Ascending),
        BoundExpr::Literal(Literal::Boolean(false)) => Some(SortOrder::Descending),
        BoundExpr::Literal(Literal::Boolean(true)) => Some(SortOrder::Ascending),
        BoundExpr::Literal(Literal::String(text)) | BoundExpr::Keyword(text) => {
            if text.eq_ignore_ascii_case("ASC") {
                Some(SortOrder::Ascending)
            } else if text.eq_ignore_ascii_case("DESC") {
                Some(SortOrder::Descending)
            } else {
                None
            }
        }
        _ => None,
    }
}
