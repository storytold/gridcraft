//! Time-intelligence scalar totals and balance functions.
//!
//! `TOTAL*` are CALCULATE sugar: the expression is evaluated over a period-to-date window
//! derived from the last date visible on the date column. `OPENINGBALANCE*`/`CLOSINGBALANCE*`
//! evaluate the expression on the period's boundary date (the last date, or the day before the
//! first), matching Excel's `CALCULATE(<expr>, ENDOF.../PREVIOUSDAY(STARTOF...))` definitions.
//!
//! The window is applied as a filter on the date column. Like the table filter produced by
//! `DATESMTD`/`ENDOFMONTH` inside CALCULATE, it replaces the existing filters on the date table
//! (otherwise a matrix row filtered by month could never accumulate a quarter or a year) while
//! filters on other tables keep applying. When the date column is a fact table's own date rather
//! than a date dimension key, only that column's filter is replaced and the fact table's other
//! filters keep applying — the same rows `CALCULATE (<expr>, DATESYTD (<date>))` would see.
//!
//! Week periods follow DAX's Sunday-first convention (`STARTOFWEEK`); the optional
//! `YearEndDate` argument only contributes its month and day, as documented for `TOTALYTD`.

use std::collections::BTreeSet;

use crate::ast::BinaryOp;
use crate::value::ScalarKey;

use super::super::support::{date_parts, date_serial, date_value, date_weekday, days_in_month, eomonth};
use super::*;

/// The calendar period a total or a balance slice is anchored to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Period {
    Month,
    Quarter,
    Week,
    Year,
}

/// A resolved date column: the table it belongs to and its column index.
#[derive(Clone, Copy)]
struct DateColumn {
    table: usize,
    column: usize,
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "TOTALMTD" => total(ev, name, args, ctx, Period::Month),
        "TOTALQTD" => total(ev, name, args, ctx, Period::Quarter),
        "TOTALWTD" => total(ev, name, args, ctx, Period::Week),
        "TOTALYTD" => total(ev, name, args, ctx, Period::Year),
        "OPENINGBALANCEMONTH" => balance(ev, name, args, ctx, Period::Month, true),
        "OPENINGBALANCEQUARTER" => balance(ev, name, args, ctx, Period::Quarter, true),
        "OPENINGBALANCEWEEK" => balance(ev, name, args, ctx, Period::Week, true),
        "OPENINGBALANCEYEAR" => balance(ev, name, args, ctx, Period::Year, true),
        "CLOSINGBALANCEMONTH" => balance(ev, name, args, ctx, Period::Month, false),
        "CLOSINGBALANCEQUARTER" => balance(ev, name, args, ctx, Period::Quarter, false),
        "CLOSINGBALANCEWEEK" => balance(ev, name, args, ctx, Period::Week, false),
        "CLOSINGBALANCEYEAR" => balance(ev, name, args, ctx, Period::Year, false),
        "FIRSTNONBLANKVALUE" => non_blank_value(ev, name, args, ctx, true),
        "LASTNONBLANKVALUE" => non_blank_value(ev, name, args, ctx, false),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

/// `TOTALMTD`/`TOTALQTD`/`TOTALWTD`/`TOTALYTD`.
fn total(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context, period: Period) -> Result<Scalar, EvalError> {
    let max = if period == Period::Year { 4 } else { 3 };
    if args.len() < 2 || args.len() > max {
        return Err(EvalError::new(format!("{name} expects between 2 and {max} arguments")));
    }
    // These functions are CALCULATE sugar: the row context transitions to filters first, and the
    // expression then runs without row frames, so a measure argument cannot re-transition the
    // window away.
    let base = ev.transitioned_filters(ctx);
    let dates = date_column(ev, &args[1])?;
    let visible = visible_dates(ev, dates, &base);
    let Some(last) = visible.last().copied() else {
        return Ok(Scalar::Blank);
    };
    let year_end = if period == Period::Year && args.len() >= 4 { Some(year_end_serial(ev, &args[3], ctx)?) } else { None };
    let (start, _) = period_bounds(period, last, year_end);

    let mut filters = date_table_filters(ev, &base, dates);
    if let Some(filter) = args.get(2) {
        apply_filter_arg(ev, filter, &mut filters, ctx)?;
    }
    // The window only keeps dates that exist in the date column, exactly like DATESMTD/YTD; the
    // keys use the column's own values so an untyped date column still matches its rows.
    let window = dates_between(ev, dates, start, last);
    // `TOTALYTD` is `CALCULATE(expr, DATESYTD(dates), filter)`: a filter argument on the date
    // column intersects the window instead of being replaced by it.
    let window: BTreeSet<ScalarKey> = window.iter().map(|(_, value)| ScalarKey::from(value)).collect();
    let keys = match filters.filters.get(&(dates.table, dates.column)) {
        Some(existing) => existing.intersection(&window).cloned().collect(),
        None => window,
    };
    filters.set(dates.table, dates.column, keys);
    let mut inner = nested_context(filters, ctx);
    ev.eval(&args[0], &mut inner)
}

/// `OPENINGBALANCE*`/`CLOSINGBALANCE*`: `CALCULATE(<expression>, <boundary date>)`.
///
/// Excel defines the closing balance on the last date of the period and the opening balance on
/// the day before the first date (`PREVIOUSDAY(STARTOF...)`). The expression is evaluated with a
/// single-date filter, so a boundary date without data yields BLANK — including on sparse fact
/// tables, where a "last date with a value" scan would invent a value Excel does not return.
fn balance(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context, period: Period, opening: bool) -> Result<Scalar, EvalError> {
    let max = if period == Period::Year { 4 } else { 3 };
    if args.len() < 2 || args.len() > max {
        return Err(EvalError::new(format!("{name} expects between 2 and {max} arguments")));
    }
    let base = ev.transitioned_filters(ctx);
    let dates = date_column(ev, &args[1])?;
    let visible = visible_dates(ev, dates, &base);
    let Some(last) = visible.last().copied() else {
        return Ok(Scalar::Blank);
    };
    let year_end = if period == Period::Year && args.len() >= 4 { Some(year_end_serial(ev, &args[3], ctx)?) } else { None };
    let (start, end) = period_bounds(period, last, year_end);
    // Every cell of the boundary day, not one per day: a fact date column can hold several
    // timestamps that share a day, and all of them belong to the period's boundary.
    let boundary = if opening {
        // PREVIOUSDAY(STARTOF...): the stored cells of the day before the period, when they exist.
        let day = start.floor() - 1.0;
        dates_between(ev, dates, day, day)
    } else {
        // `ENDOF*` is the last day that exists in the column, not the calendar end of the period.
        let window = dates_between(ev, dates, start, end);
        let Some(boundary_day) = window.last().map(|(date, _)| *date) else {
            return Ok(Scalar::Blank);
        };
        window.into_iter().filter(|(date, _)| *date == boundary_day).collect()
    };
    if boundary.is_empty() {
        return Ok(Scalar::Blank);
    }
    let boundary_keys: BTreeSet<ScalarKey> = boundary.iter().map(|(_, value)| ScalarKey::from(value)).collect();

    let mut filters = date_table_filters(ev, &base, dates);
    if let Some(filter) = args.get(2) {
        apply_filter_arg(ev, filter, &mut filters, ctx)?;
    }
    let keys = match filters.filters.get(&(dates.table, dates.column)) {
        Some(existing) => existing.intersection(&boundary_keys).cloned().collect(),
        None => boundary_keys,
    };
    filters.set(dates.table, dates.column, keys);
    let mut inner = nested_context(filters, ctx);
    ev.eval(&args[0], &mut inner)
}

/// `FIRSTNONBLANKVALUE`/`LASTNONBLANKVALUE`: the expression value at the first (last) visible
/// value of the column whose expression result is not blank.
fn non_blank_value(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context, first: bool) -> Result<Scalar, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new(format!("{name} expects 2 arguments")));
    }
    // CALCULATE sugar, like `TOTAL*`: the row context transitions to filters first, and the
    // expression runs with no row frames so a measure argument cannot re-transition the candidate
    // filter away.
    let base = ev.transitioned_filters(ctx);
    let (table, column) = column_reference(ev, &args[0])?;
    let source = ev.table(table, &base);
    let values = source.column(column);
    let keys: BTreeSet<ScalarKey> = values.iter().filter(|value| !value.is_blank()).map(ScalarKey::from).collect();
    let ordered: Vec<ScalarKey> = if first { keys.into_iter().collect() } else { keys.into_iter().rev().collect() };
    let mut inner = nested_context(base, ctx);
    for key in ordered {
        inner.filters.set(table, column, [key]);
        let value = ev.eval(&args[1], &mut inner)?;
        if !value.is_blank() {
            return Ok(value);
        }
    }
    Ok(Scalar::Blank)
}

/// Resolves a date-column argument. The week functions in the catalog take a calendar/table
/// reference, so a table argument is accepted too and its date column is chosen the same way
/// `ranges::date_axis` chooses one: a column whose declared type is a date type first, a column
/// named `Date` only as the fallback. An untyped column is never preferred over a typed one.
fn date_column(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<DateColumn, EvalError> {
    if let BoundExpr::Table(table) = expr {
        let Some(model_table) = ev.model().tables.get(*table) else {
            return Err(EvalError::new("the date table is out of range"));
        };
        let column = table_date_column(model_table).ok_or_else(|| EvalError::new(format!("table `{}` has no date column", model_table.name)))?;
        return Ok(DateColumn { table: *table, column });
    }
    let (table, column) = column_reference(ev, expr)?;
    let Some(model_table) = ev.model().tables.get(table) else {
        return Err(EvalError::new("the date table is out of range"));
    };
    let Some(model_column) = model_table.columns.get(column) else {
        return Err(EvalError::new("the date column is out of range"));
    };
    if !is_date_column(model_column.data_type.as_deref()) {
        return Err(EvalError::new(format!("`{}`[{}] is not a date column", model_table.name, model_column.name)));
    }
    Ok(DateColumn { table, column })
}

/// Resolves a plain column argument without a type restriction.
fn column_reference(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<(usize, usize), EvalError> {
    match expr {
        BoundExpr::Column { table, column } => {
            let Some(model_table) = ev.model().tables.get(*table) else {
                return Err(EvalError::new("the column's table is out of range"));
            };
            if model_table.columns.get(*column).is_none() {
                return Err(EvalError::new("the column is out of range"));
            }
            Ok((*table, *column))
        }
        BoundExpr::UnqualifiedColumn { name } => {
            let tables = ev.model().tables_with_column(name);
            let [table] = tables.as_slice() else {
                return Err(EvalError::new(format!("column `[{name}]` is ambiguous or not in the model")));
            };
            let column = ev.model().column_index(*table, name).ok_or_else(|| EvalError::new(format!("column `[{name}]` is not in the model")))?;
            Ok((*table, column))
        }
        _ => Err(EvalError::new("this function needs a column reference")),
    }
}

/// The date values visible in the current filter context. Filters propagate to the date table
/// only along bidirectional relationships, which `Evaluator::table` already models; filters on
/// the fact table of a single-direction date relationship must not shrink the calendar.
fn visible_dates(ev: &Evaluator<'_>, dates: DateColumn, filters: &FilterContext) -> Vec<f64> {
    let table = ev.table(dates.table, filters);
    let values: Vec<f64> = table.column(dates.column).iter().filter_map(date_value).collect();
    normalized(values)
}

/// The dates of the column in `[start, end]`, each with the column's own value so a filter built
/// from them matches the stored cells even when the column has no declared date type. Serials are
/// rounded down to their day, so a DateTime cell that carries a time of day still matches the
/// whole-day bounds; every cell of a day is kept, because the filter keys are the exact cell
/// values and a day may hold more than one timestamp.
fn dates_between(ev: &Evaluator<'_>, dates: DateColumn, start: f64, end: f64) -> Vec<(f64, Scalar)> {
    let Some(model_table) = ev.model().tables.get(dates.table) else {
        return Vec::new();
    };
    let mut values: Vec<(f64, Scalar)> = model_table
        .rows
        .iter()
        .filter_map(|row| row.get(dates.column).map(|value| (value, date_value(value))))
        .filter_map(|(value, serial)| serial.filter(|serial| serial.is_finite()).map(|serial| (serial.floor(), value.clone())))
        .filter(|(date, _)| *date >= start - 1e-9 && *date <= end + 1e-9)
        .collect();
    values.sort_by(|left, right| left.0.total_cmp(&right.0));
    values
}

/// Sorts ascending and drops duplicate days.
fn normalized(mut dates: Vec<f64>) -> Vec<f64> {
    dates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dates.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    dates
}

/// The context a `TOTAL*`/`BALANCE*`/`NONBLANKVALUE` expression runs in: the window's filters with
/// no row frames, carrying the caller's variables and grouping scope. A measure evaluated inside a
/// `SUMMARIZE`/`SUMMARIZECOLUMNS` group must still see the group's scope (`ISINSCOPE`), so the scope
/// is propagated rather than reset.
fn nested_context(filters: FilterContext, ctx: &Context) -> Context {
    Context { filters, rows: Vec::new(), vars: ctx.vars.clone(), grouping_scope: ctx.grouping_scope.clone() }
}

/// Removes the filters the computed window replaces. A date dimension key replaces every filter on
/// the whole date table; any other date column (a fact table's own date) replaces only that
/// column's filter, so the fact table keeps its other slices.
fn date_table_filters(ev: &Evaluator<'_>, filters: &FilterContext, dates: DateColumn) -> FilterContext {
    let model = ev.model();
    let is_dimension_key = model.relationships.iter().any(|relationship| {
        model.table_index(&relationship.to_table) == Some(dates.table)
            && model.column_index(dates.table, &relationship.to_column) == Some(dates.column)
    });
    let mut filters = filters.clone();
    if is_dimension_key {
        filters.filters.retain(|(table, _), _| *table != dates.table);
    } else {
        filters.filters.remove(&(dates.table, dates.column));
    }
    filters
}

/// The first and last day of the period containing `anchor`.
fn period_bounds(period: Period, anchor: f64, year_end: Option<f64>) -> (f64, f64) {
    let anchor = anchor.floor();
    let (year, month, _) = date_parts(anchor);
    match period {
        Period::Month => (date_serial(year, month, 1, 0, 0, 0), eomonth(anchor, 0)),
        Period::Quarter => {
            let first_month = (month - 1) / 3 * 3 + 1;
            let start = date_serial(year, first_month, 1, 0, 0, 0);
            (start, eomonth(start, 2))
        }
        Period::Week => {
            // WEEKDAY numbers are Sunday = 1 ... Saturday = 7.
            let start = anchor - (date_weekday(anchor) - 1) as f64;
            (start, start + 6.0)
        }
        Period::Year => match year_end {
            Some(year_end) => fiscal_bounds(anchor, year_end),
            None => (date_serial(year, 1, 1, 0, 0, 0), date_serial(year, 12, 31, 0, 0, 0)),
        },
    }
}

/// The fiscal year containing `anchor` for a year-end month/day. The year-end date itself
/// belongs to the fiscal year that ends on it.
fn fiscal_bounds(anchor: f64, year_end: f64) -> (f64, f64) {
    let (_, end_month, end_day) = date_parts(year_end);
    let (year, _, _) = date_parts(anchor);
    let year_end_in = |year: i64| date_serial(year, end_month, end_day.min(days_in_month(year, end_month)), 0, 0, 0);
    let end_this_year = year_end_in(year);
    if anchor <= end_this_year + 1e-9 { (year_end_in(year - 1) + 1.0, end_this_year) } else { (end_this_year + 1.0, year_end_in(year + 1)) }
}

/// Evaluates the optional `YearEndDate` argument. Only its month and day matter.
fn year_end_serial(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context) -> Result<f64, EvalError> {
    let value = ev.eval(expr, ctx)?;
    let serial = parse_year_end(&value).ok_or_else(|| EvalError::new("YearEndDate must be a date"))?;
    if !serial.is_finite() {
        return Err(EvalError::new("YearEndDate must be a date"));
    }
    Ok(serial.floor())
}

/// Parses a year-end date. Unlike `parse_date_text`, this accepts the `M/D/YYYY` form used in
/// Parses a year-end date through the shared parser, then pins it to a leap year: only the month
/// and day are used, so `"6/30/2008"` and `"6/30"` name the same fiscal year end.
fn parse_year_end(value: &Scalar) -> Option<f64> {
    let (month, day) = super::year_end_parts(value)?;
    Some(date_serial(2000, month, day, 0, 0, 0))
}

/// Applies one CALCULATE-style filter argument to `filters`, the way the core CALCULATE does:
/// column predicate, `ALL`/`REMOVEFILTERS` (no argument, column or table), or a table expression
/// over a model table. Removal runs before the period window is applied, so the window still
/// lands even when the modifier cleared the date table's own filters.
fn apply_filter_arg(ev: &Evaluator<'_>, expr: &BoundExpr, filters: &mut FilterContext, ctx: &mut Context) -> Result<(), EvalError> {
    match expr {
        BoundExpr::Binary { op, left, right }
            if matches!(op, BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge)
                && matches!(**left, BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. }) =>
        {
            let (table, column) = column_reference(ev, left)?;
            let value = ev.eval(right, ctx)?;
            // `col <op> v` is FILTER(ALL(col), …): the candidates come from the whole column and
            // compare with DAX semantics (numeric across types, text ignoring case).
            use std::cmp::Ordering::{Equal, Greater, Less};
            let keep = |candidate: &Scalar| {
                let order = candidate.compare(&value);
                match op {
                    BinaryOp::Eq => order == Some(Equal),
                    BinaryOp::Ne => order != Some(Equal),
                    BinaryOp::Lt => order == Some(Less),
                    BinaryOp::Le => matches!(order, Some(Less | Equal)),
                    BinaryOp::Gt => order == Some(Greater),
                    _ => matches!(order, Some(Greater | Equal)),
                }
            };
            let keys = ev
                .table(table, &FilterContext::default())
                .column(column)
                .into_iter()
                .filter(|candidate| keep(candidate))
                .map(|candidate| ScalarKey::from(&candidate));
            filters.set(table, column, keys);
            Ok(())
        }
        BoundExpr::Call { spec, args } if spec.name.eq_ignore_ascii_case("ALL") || spec.name.eq_ignore_ascii_case("REMOVEFILTERS") => {
            match args.first() {
                // No argument removes every filter: `ALL ()` is `ALL(<every table>)`.
                None => filters.clear(),
                // A column argument clears only that column; a table argument clears the table.
                // Unqualified columns resolve to the single model column with that name.
                Some(BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. }) => {
                    for arg in args {
                        let (table, column) = column_reference(ev, arg)?;
                        filters.filters.remove(&(table, column));
                    }
                }
                Some(BoundExpr::Table(table)) => filters.remove_table(*table),
                Some(_) => {}
            }
            Ok(())
        }
        BoundExpr::Call { spec, args } if spec.name.eq_ignore_ascii_case("KEEPFILTERS") => {
            // KEEPFILTERS applies its inner filter and intersects it with what is already there.
            let inner = args.first().ok_or_else(|| EvalError::new("KEEPFILTERS needs a filter"))?;
            let mut applied = filters.clone();
            apply_filter_arg(ev, inner, &mut applied, ctx)?;
            for (key, values) in applied.filters {
                let merged = match filters.filters.get(&key) {
                    Some(existing) => existing.intersection(&values).cloned().collect(),
                    None => values,
                };
                filters.filters.insert(key, merged);
            }
            Ok(())
        }
        _ => {
            let table = ev.eval_table(expr, ctx)?;
            let Some(index) = table.lineage else {
                return Err(EvalError::new("a filter table needs a source table"));
            };
            for (position, column) in table.columns.iter().enumerate() {
                let Some(model_column) = ev.model().column_index(index, &column.name) else { continue };
                let keys: BTreeSet<ScalarKey> = table.rows.iter().filter_map(|row| row.get(position)).map(ScalarKey::from).collect();
                filters.set(index, model_column, keys);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_context_keeps_the_grouping_scope() {
        // A date total called from inside a `SUMMARIZE` group runs its expression in a nested
        // context; the group's scope has to survive so `ISINSCOPE` still sees the group.
        let ctx = Context {
            filters: FilterContext::default(),
            rows: Vec::new(),
            vars: vec![Scalar::Integer(7)],
            grouping_scope: Some(vec![(0, 1), (0, 2)]),
        };
        let inner = nested_context(FilterContext::default(), &ctx);
        assert_eq!(inner.grouping_scope, Some(vec![(0, 1), (0, 2)]));
        assert_eq!(inner.vars, vec![Scalar::Integer(7)]);
        assert!(inner.rows.is_empty());
    }

    #[test]
    fn nested_context_starts_outside_any_group() {
        // The same call outside a grouping has no scope to carry.
        let ctx = Context { filters: FilterContext::default(), rows: Vec::new(), vars: Vec::new(), grouping_scope: None };
        assert_eq!(nested_context(FilterContext::default(), &ctx).grouping_scope, None);
    }
}
