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
//! filters on other tables keep applying.
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

    let mut filters = date_table_filters(&base, dates);
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
    let mut inner = Context { filters, rows: Vec::new(), vars: ctx.vars.clone() };
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
    let boundary = if opening {
        // PREVIOUSDAY(STARTOF...): the stored value of the day before the period, when it exists.
        let day = start.floor() - 1.0;
        match dates_between(ev, dates, day, day).into_iter().next() {
            Some((_, value)) => value,
            None => return Ok(Scalar::Blank),
        }
    } else {
        // `ENDOF*` is the last date that exists in the column, not the calendar end of the period.
        match dates_between(ev, dates, start, end).pop() {
            Some((_, value)) => value,
            None => return Ok(Scalar::Blank),
        }
    };

    let mut filters = date_table_filters(&base, dates);
    if let Some(filter) = args.get(2) {
        apply_filter_arg(ev, filter, &mut filters, ctx)?;
    }
    let keys = match filters.filters.get(&(dates.table, dates.column)) {
        Some(existing) if !existing.contains(&ScalarKey::from(&boundary)) => BTreeSet::new(),
        _ => BTreeSet::from([ScalarKey::from(&boundary)]),
    };
    filters.set(dates.table, dates.column, keys);
    let mut inner = Context { filters, rows: Vec::new(), vars: ctx.vars.clone() };
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
    let mut inner = Context { filters: base, rows: Vec::new(), vars: ctx.vars.clone() };
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
/// reference, so a table argument is accepted too and its first date column is used.
fn date_column(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<DateColumn, EvalError> {
    if let BoundExpr::Table(table) = expr {
        let Some(model_table) = ev.model().tables.get(*table) else {
            return Err(EvalError::new("the date table is out of range"));
        };
        let column = model_table
            .columns
            .iter()
            .position(|column| column.name.eq_ignore_ascii_case("Date") && is_date_type(column.data_type.as_deref()))
            .or_else(|| model_table.columns.iter().position(|column| is_date_type(column.data_type.as_deref())))
            .ok_or_else(|| EvalError::new(format!("table `{}` has no date column", model_table.name)))?;
        return Ok(DateColumn { table: *table, column });
    }
    let (table, column) = column_reference(ev, expr)?;
    let Some(model_table) = ev.model().tables.get(table) else {
        return Err(EvalError::new("the date table is out of range"));
    };
    let Some(model_column) = model_table.columns.get(column) else {
        return Err(EvalError::new("the date column is out of range"));
    };
    if !is_date_type(model_column.data_type.as_deref()) {
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

fn is_date_type(data_type: Option<&str>) -> bool {
    match data_type {
        None => true,
        Some(name) => {
            let name = name.to_ascii_lowercase();
            name == "datetime" || name == "date"
        }
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

/// The dates of the date column that exist in the model and fall inside `[start, end]`.
/// The dates of the column in `[start, end]`, each with the column's own value so a filter built
/// from them matches the stored cells even when the column has no declared date type.
fn dates_between(ev: &Evaluator<'_>, dates: DateColumn, start: f64, end: f64) -> Vec<(f64, Scalar)> {
    let Some(model_table) = ev.model().tables.get(dates.table) else {
        return Vec::new();
    };
    let mut values: Vec<(f64, Scalar)> = model_table
        .rows
        .iter()
        .filter_map(|row| row.get(dates.column).map(|value| (value, date_value(value))))
        .filter_map(|(value, serial)| serial.map(|serial| (serial, value.clone())))
        .filter(|(date, _)| *date >= start - 1e-9 && *date <= end + 1e-9)
        .collect();
    values.sort_by(|left, right| left.0.total_cmp(&right.0));
    values.dedup_by(|left, right| left.0 == right.0);
    values
}

/// Sorts ascending and drops duplicate days.
fn normalized(mut dates: Vec<f64>) -> Vec<f64> {
    dates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dates.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    dates
}

/// Removes every filter on the date table, so the computed window replaces them.
fn date_table_filters(filters: &FilterContext, dates: DateColumn) -> FilterContext {
    let mut filters = filters.clone();
    filters.filters.retain(|(table, _), _| *table != dates.table);
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
/// the documented `TOTALYTD(..., "6/30/2008")` examples.
fn parse_year_end(value: &Scalar) -> Option<f64> {
    match value {
        Scalar::String(text) => {
            let text = text.trim();
            let parts: Vec<&str> = text.split(['-', '/']).collect();
            // `TOTALYTD(..., "6/30")` names the month and day only; a leap year keeps `2/29`
            // valid and the year is never read back.
            if let [month, day] = parts.as_slice() {
                let month: i64 = month.trim().parse().ok()?;
                let day: i64 = day.trim().parse().ok()?;
                if !(1..=12).contains(&month) || !(1..=31).contains(&day) || day > days_in_month(2000, month) {
                    return None;
                }
                return Some(date_serial(2000, month, day, 0, 0, 0));
            }
            let [first, second, third] = parts.as_slice() else {
                return date_value(value);
            };
            let first: i64 = first.trim().parse().ok()?;
            let second: i64 = second.trim().parse().ok()?;
            let third: i64 = third.trim().parse().ok()?;
            let (year, month, day) = if first > 31 {
                (first, second, third)
            } else if third > 31 {
                (third, first, second)
            } else if (0..=99).contains(&third) {
                // `M/D/YY`: the year is ignored, as only the month and day matter.
                (2000, first, second)
            } else {
                return None;
            };
            // The three-part forms reject an impossible month or day too, instead of letting
            // `date_serial` roll them into another fiscal year.
            if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
                return None;
            }
            Some(date_serial(year, month, day, 0, 0, 0))
        }
        other => date_value(other),
    }
}

/// Applies one CALCULATE-style filter argument to `filters`, the way the core CALCULATE does:
/// column predicate, `ALL`/`REMOVEFILTERS` (no argument, column or table), or a table expression
/// over a model table. Removal runs before the period window is applied, so the window still
/// lands even when the modifier cleared the date table's own filters.
fn apply_filter_arg(ev: &Evaluator<'_>, expr: &BoundExpr, filters: &mut FilterContext, ctx: &mut Context) -> Result<(), EvalError> {
    match expr {
        BoundExpr::Binary { op, left, right } if matches!(op, BinaryOp::Eq | BinaryOp::Ne) => {
            let BoundExpr::Column { table, column } = **left else {
                return Err(EvalError::new("a filter must compare a column"));
            };
            let value = ev.eval(right, ctx)?;
            // `col = v` / `col <> v` is FILTER(ALL(col), …): the candidates come from the whole
            // column and compare with DAX `=` semantics (numeric across types, text ignoring case).
            let equal = |candidate: &Scalar| candidate.compare(&value) == Some(std::cmp::Ordering::Equal);
            let keep_equal = *op == BinaryOp::Eq;
            let keys = ev
                .table(table, &FilterContext::default())
                .column(column)
                .into_iter()
                .filter(|candidate| equal(candidate) == keep_equal)
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
