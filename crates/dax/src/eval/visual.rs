//! Visual-calculation functions.
//!
//! Visual calculations run over an ordered visual frame rather than over an arbitrary filter
//! context. The evaluator has no pivot engine, so the "visual row" is the innermost row frame of
//! the current evaluation (the row produced by an iterator such as `ADDCOLUMNS`, `FILTER` or
//! `SUMX`). The axis is that frame's model table restricted by the current filter context, in
//! model order; the current position is found by matching the frame's model columns against the
//! axis rows.
//!
//! `orderBy`, `blanks` and `reset` arguments are accepted and ignored: without a visual shape
//! there is nothing to sort or partition by, and model order is the documented fallback ordering
//! ("If `orderBy` is not provided, the data is sorted by the grouping columns on the default axis
//! in ascending order by default", <https://learn.microsoft.com/dax/range-function-dax>).

use std::cmp::Ordering;

use super::support::model_row_frame;
use super::*;

/// Error text shared by every function that needs a visual row. The contract asks for a
/// documented "no visual frame" error when a visual calculation runs outside a visual row.
const NO_FRAME: &str = "visual calculation requires a visual frame, but the current context has no row";

/// The ordered axis a visual calculation navigates, plus the position of the current row.
struct Axis {
    table: usize,
    columns: Vec<TableColumn>,
    rows: Vec<Vec<Scalar>>,
    current: usize,
}

/// Resolves the model table of the innermost row frame, rejecting callers without one.
fn frame_table(ctx: &Context) -> Result<usize, EvalError> {
    let frame = ctx.rows.last().ok_or_else(|| EvalError::new(NO_FRAME))?;
    frame.lineage.ok_or_else(|| EvalError::new(NO_FRAME))
}

/// Builds the axis from the current filter context and locates the current frame on it.
fn axis(ev: &Evaluator<'_>, ctx: &Context) -> Result<Axis, EvalError> {
    let Some(frame) = ctx.rows.last() else { return Err(EvalError::new(NO_FRAME)) };
    let Some(table) = frame.lineage else { return Err(EvalError::new(NO_FRAME)) };
    // Only model columns identify a row: computed columns added by ADDCOLUMNS/SELECTCOLUMNS are
    // skipped, exactly like context transition does.
    let key: Vec<(usize, Scalar)> =
        frame.model_columns.iter().zip(frame.values.iter()).filter_map(|(column, value)| (*column).map(|column| (column, value.clone()))).collect();
    if key.is_empty() {
        return Err(EvalError::new("visual calculation cannot locate the current row: the row carries no model columns"));
    }
    let value = ev.table(table, &ctx.filters);
    let current = value
        .rows
        .iter()
        .position(|row| key.iter().all(|(column, key)| row.get(*column).is_some_and(|value| same_value(key, value))))
        .ok_or_else(|| EvalError::new("the current row is not part of the visual axis"))?;
    Ok(Axis { table, columns: value.columns, rows: value.rows, current })
}

/// DAX equality for locating a row: blanks only equal blanks; everything else uses `compare`
/// so that `Integer`/`Decimal` keys and case-insensitive strings still match.
fn same_value(left: &Scalar, right: &Scalar) -> bool {
    match (left.is_blank(), right.is_blank()) {
        (true, true) => true,
        (true, false) | (false, true) => false,
        _ => left.compare(right) == Some(Ordering::Equal),
    }
}

/// Evaluates an expression with the axis row at `position` as the row context.
fn eval_at(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, axis: &Axis, position: usize) -> Result<Scalar, EvalError> {
    let Some(row) = axis.rows.get(position) else { return Ok(Scalar::Blank) };
    ctx.rows.push(model_row_frame(ev, axis.table, row.clone()));
    let result = ev.eval(expr, ctx);
    ctx.rows.pop();
    result
}

/// Evaluates `expr` over `start..=end` and returns the non-blank numeric values.
fn window(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context, axis: &Axis, start: usize, end: usize) -> Result<Vec<f64>, EvalError> {
    if start > end || axis.rows.is_empty() {
        return Ok(Vec::new());
    }
    let last = end.min(axis.rows.len() - 1);
    if start > last {
        return Ok(Vec::new());
    }
    let mut values = Vec::with_capacity(last - start + 1);
    for position in start..=last {
        let value = eval_at(ev, expr, ctx, axis, position)?;
        if !value.is_blank()
            && let Some(number) = value.to_number()
        {
            values.push(number);
        }
    }
    Ok(values)
}

/// Reads an optional positive step/window argument. `FIRST`-style functions default to one and
/// require at least one, matching "Steps defaults to 1 and must be at least 1".
fn positive_count(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, index: usize, label: &str) -> Result<usize, EvalError> {
    let Some(expr) = args.get(index) else { return Ok(1) };
    let value = ev.eval(expr, ctx)?;
    let number = value.to_number().ok_or_else(|| EvalError::new(format!("{label} must be a number")))?;
    if !number.is_finite() || number < 1.0 {
        return Err(EvalError::new(format!("{label} must be at least 1")));
    }
    Ok(if number >= usize::MAX as f64 { usize::MAX } else { number.trunc() as usize })
}

/// Reads an optional Boolean argument.
fn boolean_arg(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, index: usize, default: bool) -> Result<bool, EvalError> {
    let Some(expr) = args.get(index) else { return Ok(default) };
    let value = ev.eval(expr, ctx)?;
    value.to_bool().ok_or_else(|| EvalError::new("expected a Boolean argument"))
}

/// `FIRST`/`LAST`: the expression at the first or last row of the axis.
fn first_last(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let axis = axis(ev, ctx)?;
    let expr = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    let position = if name.eq_ignore_ascii_case("FIRST") { Some(0) } else { axis.rows.len().checked_sub(1) };
    match position {
        Some(position) => eval_at(ev, expr, ctx, &axis, position),
        None => Ok(Scalar::Blank),
    }
}

/// `PREVIOUS`/`NEXT`: the expression `steps` rows before/after the current row. Rows outside the
/// axis return BLANK, matching the documented visual-calculation navigation behavior.
fn previous_next(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let axis = axis(ev, ctx)?;
    let expr = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    let steps = positive_count(ev, args, ctx, 1, "Steps")?;
    let position = if name.eq_ignore_ascii_case("PREVIOUS") { axis.current.checked_sub(steps) } else { axis.current.checked_add(steps) };
    match position {
        Some(position) => eval_at(ev, expr, ctx, &axis, position),
        None => Ok(Scalar::Blank),
    }
}

/// `RUNNINGSUM`: the cumulative sum from the first axis row through the current row.
fn running_sum(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let axis = axis(ev, ctx)?;
    let expr = args.first().ok_or_else(|| EvalError::new("RUNNINGSUM needs a column"))?;
    let values = window(ev, expr, ctx, &axis, 0, axis.current)?;
    if values.is_empty() { Ok(Scalar::Blank) } else { Ok(Scalar::Decimal(values.iter().sum())) }
}

/// `MOVINGAVERAGE`: the average of a window of `windowSize` rows. With `includeCurrent` (the
/// default) the window ends at the current row; otherwise it ends at the previous row.
fn moving_average(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let axis = axis(ev, ctx)?;
    let expr = args.first().ok_or_else(|| EvalError::new("MOVINGAVERAGE needs a column"))?;
    if args.len() < 2 {
        return Err(EvalError::new("MOVINGAVERAGE needs a window size"));
    }
    let window_size = positive_count(ev, args, ctx, 1, "WindowSize")?;
    let include_current = boolean_arg(ev, args, ctx, 2, true)?;
    let end = if include_current {
        axis.current
    } else {
        match axis.current.checked_sub(1) {
            Some(end) => end,
            None => return Ok(Scalar::Blank),
        }
    };
    let start = end.saturating_add(1).saturating_sub(window_size);
    let values = window(ev, expr, ctx, &axis, start, end)?;
    if values.is_empty() { Ok(Scalar::Blank) } else { Ok(Scalar::Decimal(values.iter().sum::<f64>() / values.len() as f64)) }
}

/// `COLLAPSE`, `COLLAPSEALL`, `EXPAND`, `EXPANDALL`: layout hints that keep the current axis.
/// Without a visual shape there is no axis to collapse or expand, so they error; with one they
/// return their first argument unchanged (the row-layout does not alter values in this model).
fn layout(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    frame_table(ctx)?;
    let first = args.first().ok_or_else(|| EvalError::new(format!("{name} needs an expression or an axis")))?;
    ev.eval(first, ctx)
}

/// `RANGE`: the axis rows relative to the current row. A negative step selects the `-step` rows
/// before the current row; a non-negative step selects the `step` rows after it. `includeCurrent`
/// defaults to TRUE, so `RANGE(-5, ...)` is the current row plus the previous five. See
/// <https://learn.microsoft.com/dax/range-function-dax>.
fn range_table(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let axis = axis(ev, ctx)?;
    let step_expr = args.first().ok_or_else(|| EvalError::new("RANGE needs a step"))?;
    let step_value = ev.eval(step_expr, ctx)?;
    let number = step_value.to_number().ok_or_else(|| EvalError::new("RANGE step must be a number"))?;
    if !number.is_finite() {
        return Err(EvalError::new("RANGE step must be finite"));
    }
    let step = number.trunc().clamp(-(i64::MAX as f64), i64::MAX as f64) as i64;
    let include_current = boolean_arg(ev, args, ctx, 1, true)?;
    let current = axis.current as i64;
    let (start, end) = if step < 0 {
        let end = if include_current { current } else { current.saturating_sub(1) };
        (current.saturating_add(step), end)
    } else {
        let start = if include_current { current } else { current.saturating_add(1) };
        (start, current.saturating_add(step))
    };
    let lowest = start.max(0);
    let highest = end.min(axis.rows.len() as i64 - 1);
    let rows = if lowest > highest { Vec::new() } else { axis.rows[lowest as usize..=highest as usize].to_vec() };
    Ok(TableValue::new(axis.columns, rows).with_lineage(axis.table))
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "COLLAPSE" | "COLLAPSEALL" | "EXPAND" | "EXPANDALL" => layout(ev, name, args, ctx),
        "FIRST" | "LAST" => first_last(ev, name, args, ctx),
        "PREVIOUS" | "NEXT" => previous_next(ev, name, args, ctx),
        "RUNNINGSUM" => running_sum(ev, args, ctx),
        "MOVINGAVERAGE" => moving_average(ev, args, ctx),
        "RANGE" => Err(EvalError::new("RANGE returns a table and cannot be used as a scalar")),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "RANGE" => Some(range_table(ev, args, ctx)),
        _ => None,
    }
}
