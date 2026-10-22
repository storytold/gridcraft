//! Time-intelligence range functions (period tables).
//!
//! Every function here works the same way: it reads the dates visible in the current filter
//! context for a DateTime column, derives a calendar window from them, and materializes the
//! window as a one-column table whose rows are the *model's own* date values (so the result keeps
//! the date table's lineage and `CALCULATE` can use it as a filter).
//!
//! * The visible range comes from the filter context when the date column (or the date table) is
//!   filtered, otherwise from the whole table. The window is anchored on the first visible date
//!   (`PREVIOUS*`), the last visible date (`NEXT*`, `START*`, `END*`, `DATES*TD`), or both
//!   (`DATEADD`, `DATESBETWEEN`, ...).
//! * The window is intersected with the dates that exist in the column, so a non-contiguous date
//!   table only loses the missing days.
//! * The table forms replace any filter on the date column, per the documented time-intelligence
//!   contract; the scalar conveniences (`FIRSTDATE` used as a value, ...) only accept a
//!   single-row result.
//!
//! Week-based functions (`DATESWTD`, `STARTOFWEEK`, `ENDOFWEEK`, `NEXTWEEK`, `PREVIOUSWEEK`) take
//! a calendar reference in the catalog. GridCraft has no calendar objects, so the reference is
//! resolved to the calendar table's date column and weeks are Sunday-based, which is the Excel /
//! `WEEKDAY` default.
//!
//! https://dax.guide/datesinperiod/ (boundary date excluded), https://dax.guide/dateadd/

#![allow(unused_imports)]

use std::collections::BTreeSet;

use super::super::support::{date_parts, date_serial, date_value, date_weekday, days_in_month, edate, model_row_frame};
use super::*;

/// Every function that this module answers for.
const FUNCTIONS: &[&str] = &[
    "DATEADD",
    "DATESBETWEEN",
    "DATESINPERIOD",
    "DATESMTD",
    "DATESQTD",
    "DATESWTD",
    "DATESYTD",
    "ENDOFMONTH",
    "ENDOFQUARTER",
    "ENDOFWEEK",
    "ENDOFYEAR",
    "FIRSTDATE",
    "FIRSTNONBLANK",
    "LASTDATE",
    "LASTNONBLANK",
    "NEXTDAY",
    "NEXTMONTH",
    "NEXTQUARTER",
    "NEXTWEEK",
    "NEXTYEAR",
    "PARALLELPERIOD",
    "PREVIOUSDAY",
    "PREVIOUSMONTH",
    "PREVIOUSQUARTER",
    "PREVIOUSWEEK",
    "PREVIOUSYEAR",
    "SAMEPERIODLASTYEAR",
    "STARTOFMONTH",
    "STARTOFQUARTER",
    "STARTOFWEEK",
    "STARTOFYEAR",
];

/// Sanity bound for interval counts: the arithmetic stays exact and the loops over the date
/// table stay bounded by the model, so only absurd inputs need rejecting.
const MAX_INTERVALS: f64 = 1_000_000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interval {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let name = name.to_ascii_uppercase();
    if !FUNCTIONS.contains(&name.as_str()) {
        return None;
    }
    let result = match name.as_str() {
        "FIRSTNONBLANK" => nonblank(ev, args, ctx, true).and_then(|table| scalar_result(&name, table)),
        "LASTNONBLANK" => nonblank(ev, args, ctx, false).and_then(|table| scalar_result(&name, table)),
        _ => compute_table(ev, &name, args, ctx).and_then(|table| scalar_result(&name, table)),
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let name = name.to_ascii_uppercase();
    if !FUNCTIONS.contains(&name.as_str()) {
        return None;
    }
    Some(match name.as_str() {
        "FIRSTNONBLANK" => nonblank(ev, args, ctx, true),
        "LASTNONBLANK" => nonblank(ev, args, ctx, false),
        _ => compute_table(ev, &name, args, ctx),
    })
}

/// Converts a one-row, one-column result to its value; anything else cannot be used as a scalar.
fn scalar_result(name: &str, table: TableValue) -> Result<Scalar, EvalError> {
    match table.rows.as_slice() {
        [] => Ok(Scalar::Blank),
        [row] => Ok(row.first().cloned().unwrap_or(Scalar::Blank)),
        rows => Err(EvalError::new(format!("{name} returns a table with {} rows; use it as a CALCULATE filter instead of a scalar", rows.len()))),
    }
}

/// The date column a time-intelligence call is anchored on.
struct DateAxis {
    table: usize,
    name: String,
    /// Every date that exists in the column, ascending, as (day serial, original value).
    dates: Vec<(f64, Scalar)>,
    /// First and last visible date, absent when the filter context selects no date.
    min: Option<f64>,
    max: Option<f64>,
}

impl DateAxis {
    fn empty(&self) -> TableValue {
        TableValue::new(vec![TableColumn { name: self.name.clone() }], Vec::new()).with_lineage(self.table)
    }

    /// The dates of the column between two inclusive boundaries.
    fn table(&self, start: f64, end: f64) -> TableValue {
        let start = start.floor();
        let end = end.floor();
        let rows = if start > end {
            Vec::new()
        } else {
            self.dates.iter().filter(|(serial, _)| (start..=end).contains(serial)).map(|(_, value)| vec![value.clone()]).collect()
        };
        TableValue::new(vec![TableColumn { name: self.name.clone() }], rows).with_lineage(self.table)
    }

    fn single(&self, date: f64) -> TableValue {
        self.table(date, date)
    }

    fn first(&self) -> TableValue {
        match self.min {
            Some(min) => self.single(min),
            None => self.empty(),
        }
    }

    fn last(&self) -> TableValue {
        match self.max {
            Some(max) => self.single(max),
            None => self.empty(),
        }
    }

    fn visible(&self) -> Option<(f64, f64)> {
        Some((self.min?, self.max?))
    }
}

fn is_date_type(data_type: &str) -> bool {
    data_type.eq_ignore_ascii_case("DateTime") || data_type.eq_ignore_ascii_case("Date")
}

/// Resolves the `Dates` argument (a DateTime column, or a calendar table for the week-based
/// functions) and reads the dates visible in the current filter context.
fn date_axis(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &Context, name: &str) -> Result<DateAxis, EvalError> {
    let model = ev.model();
    let (table, column) = match expr {
        BoundExpr::Column { table, column } => (*table, *column),
        BoundExpr::UnqualifiedColumn { name: column } => {
            let tables = model.tables_with_column(column);
            let table = match tables.as_slice() {
                [table] => *table,
                [] => return Err(EvalError::new(format!("{name} names the unknown column `[{column}]`"))),
                _ => return Err(EvalError::new(format!("{name} needs a qualified column; `[{column}]` is ambiguous"))),
            };
            let index = model.column_index(table, column).ok_or_else(|| EvalError::new(format!("{name} names the unknown column `[{column}]`")))?;
            (table, index)
        }
        BoundExpr::Table(table) => {
            let model_table = model.tables.get(*table).ok_or_else(|| EvalError::new(format!("{name} needs a date table")))?;
            let index = model_table
                .columns
                .iter()
                .position(|column| column.data_type.as_deref().is_some_and(is_date_type))
                .or_else(|| model_table.columns.iter().position(|column| column.name.eq_ignore_ascii_case("Date")))
                .ok_or_else(|| EvalError::new(format!("{name} needs a calendar with a date column")))?;
            (*table, index)
        }
        _ => return Err(EvalError::new(format!("{name} needs a date column reference"))),
    };

    let model_table = model.tables.get(table).ok_or_else(|| EvalError::new(format!("{name} needs a valid date table")))?;
    let model_column = model_table.columns.get(column).ok_or_else(|| EvalError::new(format!("{name} needs a valid date column")))?;
    if let Some(data_type) = &model_column.data_type
        && !is_date_type(data_type)
    {
        return Err(EvalError::new(format!("{name} needs a DateTime column; `{}[{}]` is {data_type}", model_table.name, model_column.name)));
    }

    let mut dates: Vec<(f64, Scalar)> = model_table
        .rows
        .iter()
        .filter_map(|row| row.get(column))
        .filter_map(|value| value.to_number().map(|serial| (serial.floor(), value.clone())))
        .collect();
    dates.sort_by(|left, right| left.0.total_cmp(&right.0));
    dates.dedup_by(|left, right| left.0 == right.0);

    // The visible dates honour the filter context. Only filters on the date table itself count:
    // a filter on a fact table does not restrict the dimension it points at.
    let mut min: Option<f64> = None;
    let mut max: Option<f64> = None;
    for row in visible_rows(ev, table, ctx) {
        if let Some(serial) = row.get(column).and_then(Scalar::to_number) {
            let serial = serial.floor();
            min = Some(min.map_or(serial, |value| value.min(serial)));
            max = Some(max.map_or(serial, |value| value.max(serial)));
        }
    }

    Ok(DateAxis { table, name: model_column.name.clone(), dates, min, max })
}

/// The model rows of a table that survive the filters currently set on that table.
fn visible_rows(ev: &Evaluator<'_>, table: usize, ctx: &Context) -> Vec<Vec<Scalar>> {
    let model = ev.model();
    let Some(model_table) = model.tables.get(table) else {
        return Vec::new();
    };
    model_table
        .rows
        .iter()
        .filter(|row| {
            model_table.columns.iter().enumerate().all(|(index, _)| match ctx.filters.filters.get(&(table, index)) {
                Some(values) => row.get(index).is_none_or(|value| values.contains(&ScalarKey::from(value))),
                None => true,
            })
        })
        .cloned()
        .collect()
}

/// Resolves a `ColumnName` argument to (table, column, column name).
fn column_target(ev: &Evaluator<'_>, expr: &BoundExpr, name: &str) -> Result<(usize, usize, String), EvalError> {
    let model = ev.model();
    let (table, column) = match expr {
        BoundExpr::Column { table, column } => (*table, *column),
        BoundExpr::UnqualifiedColumn { name: column_name } => {
            let tables = model.tables_with_column(column_name);
            let table = match tables.as_slice() {
                [table] => *table,
                [] => return Err(EvalError::new(format!("{name} names the unknown column `[{column_name}]`"))),
                _ => return Err(EvalError::new(format!("{name} needs a qualified column; `[{column_name}]` is ambiguous"))),
            };
            let index =
                model.column_index(table, column_name).ok_or_else(|| EvalError::new(format!("{name} names the unknown column `[{column_name}]`")))?;
            (table, index)
        }
        _ => return Err(EvalError::new(format!("{name} needs a column reference"))),
    };
    let model_table = model.tables.get(table).ok_or_else(|| EvalError::new(format!("{name} needs a valid table")))?;
    let model_column = model_table.columns.get(column).ok_or_else(|| EvalError::new(format!("{name} needs a valid column")))?;
    Ok((table, column, model_column.name.clone()))
}

fn number_arg(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context, name: &str) -> Result<f64, EvalError> {
    let expr = args.get(index).ok_or_else(|| EvalError::new(format!("{name} is missing argument {}", index + 1)))?;
    let value = ev.eval(expr, ctx)?;
    value.to_number().ok_or_else(|| EvalError::new(format!("{name} argument {} must be numeric", index + 1)))
}

fn interval_count(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context, name: &str) -> Result<i64, EvalError> {
    let value = number_arg(ev, args, index, ctx, name)?;
    if !value.is_finite() || value.abs() > MAX_INTERVALS {
        return Err(EvalError::new(format!("{name} interval count is out of range")));
    }
    Ok(value.trunc() as i64)
}

fn interval_arg(
    ev: &Evaluator<'_>,
    args: &[BoundExpr],
    index: usize,
    ctx: &mut Context,
    name: &str,
    allow_day: bool,
    allow_week: bool,
) -> Result<Interval, EvalError> {
    let expr = args.get(index).ok_or_else(|| EvalError::new(format!("{name} is missing argument {}", index + 1)))?;
    let value = ev.eval(expr, ctx)?;
    let text = value.to_text();
    match text.trim().to_ascii_uppercase().as_str() {
        "DAY" if allow_day => Ok(Interval::Day),
        "WEEK" if allow_week => Ok(Interval::Week),
        "MONTH" => Ok(Interval::Month),
        "QUARTER" => Ok(Interval::Quarter),
        "YEAR" => Ok(Interval::Year),
        other => Err(EvalError::new(format!("{name} does not support the interval `{other}`"))),
    }
}

/// A date argument that accepts BLANK (meaning "use the visible boundary").
fn optional_date(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context, name: &str) -> Result<Option<f64>, EvalError> {
    let expr = args.get(index).ok_or_else(|| EvalError::new(format!("{name} is missing argument {}", index + 1)))?;
    let value = ev.eval(expr, ctx)?;
    if value.is_blank() {
        return Ok(None);
    }
    date_value(&value).map(|serial| Some(serial.floor())).ok_or_else(|| EvalError::new(format!("{name} argument {} must be a date", index + 1)))
}

fn year_end_arg(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context, name: &str) -> Result<Option<(i64, i64)>, EvalError> {
    let Some(expr) = args.get(index) else {
        return Ok(None);
    };
    let value = ev.eval(expr, ctx)?;
    if value.is_blank() {
        return Ok(None);
    }
    year_end_parts(&value).map(Some).ok_or_else(|| EvalError::new(format!("{name} YearEndDate must be a date such as \"6/30/2024\"")))
}

/// Reads the month/day of a `YearEndDate`. Excel writes it as `M/D/YYYY` (`"6/30/2008"`);
/// `YYYY-MM-DD` and a real DateTime value are accepted as well. `support::parse_date_text` reads
/// the first component as the year, so it cannot be used for the Excel form.
fn year_end_parts(value: &Scalar) -> Option<(i64, i64)> {
    let (month, day) = match value {
        Scalar::String(text) => {
            let numbers =
                text.split(['/', '-']).map(str::trim).filter(|part| !part.is_empty()).map(str::parse::<i64>).collect::<Result<Vec<i64>, _>>().ok()?;
            match numbers.as_slice() {
                [month, day] => (*month, *day),
                [year, month, day] if *year > 31 => (*month, *day),
                [month, day, _] => (*month, *day),
                _ => return None,
            }
        }
        other => {
            let (_, month, day) = date_parts(date_value(other)?);
            (month, day)
        }
    };
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((month, day))
}

fn month_start(serial: f64) -> f64 {
    let (year, month, _) = date_parts(serial);
    date_serial(year, month, 1, 0, 0, 0)
}

fn month_end(serial: f64) -> f64 {
    let (year, month, _) = date_parts(serial);
    date_serial(year, month, days_in_month(year, month), 0, 0, 0)
}

fn quarter_start(serial: f64) -> f64 {
    let (year, month, _) = date_parts(serial);
    let first = (month - 1).div_euclid(3) * 3 + 1;
    date_serial(year, first, 1, 0, 0, 0)
}

fn quarter_end(serial: f64) -> f64 {
    let (year, month, _) = date_parts(serial);
    let last = (month - 1).div_euclid(3) * 3 + 3;
    date_serial(year, last, days_in_month(year, last), 0, 0, 0)
}

fn year_start(serial: f64) -> f64 {
    date_serial(date_parts(serial).0, 1, 1, 0, 0, 0)
}

fn week_start(serial: f64) -> f64 {
    // `date_weekday` uses the Excel WEEKDAY convention: Sunday = 1 ... Saturday = 7.
    serial.floor() - (date_weekday(serial) - 1) as f64
}

fn week_end(serial: f64) -> f64 {
    week_start(serial) + 6.0
}

/// End of the period (month/quarter/year) containing a date; day and week intervals are whole
/// numbers of days, so their aligned end is the date itself.
fn align_end(serial: f64, interval: Interval) -> f64 {
    match interval {
        Interval::Day | Interval::Week => serial.floor(),
        Interval::Month => month_end(serial),
        Interval::Quarter => quarter_end(serial),
        Interval::Year => year_end_default(serial),
    }
}

/// Start of the period (month/quarter/year) containing a date.
fn align_start(serial: f64, interval: Interval) -> f64 {
    match interval {
        Interval::Day | Interval::Week => serial.floor(),
        Interval::Month => month_start(serial),
        Interval::Quarter => quarter_start(serial),
        Interval::Year => year_start(serial),
    }
}

fn year_end_default(serial: f64) -> f64 {
    date_serial(date_parts(serial).0, 12, 31, 0, 0, 0)
}

fn shift(serial: f64, count: i64, interval: Interval) -> f64 {
    match interval {
        Interval::Day => serial + count as f64,
        Interval::Week => serial + 7.0 * count as f64,
        Interval::Month => edate(serial, count),
        Interval::Quarter => edate(serial, count.saturating_mul(3)),
        Interval::Year => edate(serial, count.saturating_mul(12)),
    }
}

/// The last day of a fiscal year that starts the day after the previous year's end date.
fn fiscal_end(year: i64, month: i64, day: i64) -> f64 {
    date_serial(year, month, day.min(days_in_month(year, month)), 0, 0, 0)
}

/// The (start, end) of the fiscal year containing `serial`; the default year ends December 31.
fn fiscal_bounds(serial: f64, year_end: Option<(i64, i64)>) -> (f64, f64) {
    let (month, day) = year_end.unwrap_or((12, 31));
    let (year, _, _) = date_parts(serial);
    let end_this_year = fiscal_end(year, month, day);
    let end_year = if serial.floor() <= end_this_year { year } else { year + 1 };
    let end = fiscal_end(end_year, month, day);
    let start = fiscal_end(end_year - 1, month, day) + 1.0;
    (start, end)
}

/// The previous calendar month's window.
fn previous_month(serial: f64) -> (f64, f64) {
    let last = month_start(serial) - 1.0;
    (month_start(last), last)
}

fn next_month(serial: f64) -> (f64, f64) {
    let first = month_end(serial) + 1.0;
    (first, month_end(first))
}

fn previous_quarter(serial: f64) -> (f64, f64) {
    let last = quarter_start(serial) - 1.0;
    (quarter_start(last), last)
}

fn next_quarter(serial: f64) -> (f64, f64) {
    let first = quarter_end(serial) + 1.0;
    (first, quarter_end(first))
}

fn previous_week(serial: f64) -> (f64, f64) {
    let last = week_start(serial) - 1.0;
    (week_start(last), last)
}

fn next_week(serial: f64) -> (f64, f64) {
    let first = week_end(serial) + 1.0;
    (first, week_end(first))
}

fn compute_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let dates_expr = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a date column")))?;
    let axis = date_axis(ev, dates_expr, ctx, name)?;

    match name {
        "FIRSTDATE" => Ok(axis.first()),
        "LASTDATE" => Ok(axis.last()),
        "DATEADD" => {
            // Arguments 4/5 (Extension/Truncation) exist only for model calendars. GridCraft has
            // no calendar objects, so the date-column rules always apply: a selection that ends on
            // the last day of a period extends to the end of the shifted period, and day positions
            // that do not exist in the target month move to its last day (dax.guide/DATEADD).
            let count = interval_count(ev, args, 1, ctx, name)?;
            let interval = interval_arg(ev, args, 2, ctx, name, true, false)?;
            let Some((min, max)) = axis.visible() else {
                return Ok(axis.empty());
            };
            let start = shift(min, count, interval);
            let mut end = shift(max, count, interval);
            // A selection that ends on the last day of its period selects the whole target period:
            // "if the Dates argument includes all the days in April (30 rows), the result for the
            // previous month includes all the days in March (31 days)" (dax.guide/DATEADD).
            if align_end(max, interval).floor() == max {
                end = align_end(end, interval);
            }
            Ok(axis.table(start.min(end), start.max(end)))
        }
        "DATESBETWEEN" => {
            let start = optional_date(ev, args, 1, ctx, name)?.or(axis.min);
            let end = optional_date(ev, args, 2, ctx, name)?.or(axis.max);
            match (start, end) {
                (Some(start), Some(end)) => Ok(axis.table(start, end)),
                _ => Ok(axis.empty()),
            }
        }
        "DATESINPERIOD" => {
            let start = optional_date(ev, args, 1, ctx, name)?.or(axis.min);
            let count = interval_count(ev, args, 2, ctx, name)?;
            let interval = interval_arg(ev, args, 3, ctx, name, true, true)?;
            let aligned = match args.get(4) {
                Some(expr) => {
                    let value = ev.eval(expr, ctx)?;
                    match value.to_text().trim().to_ascii_uppercase().as_str() {
                        "ENDALIGNED" => true,
                        "" | "PRECISE" => false,
                        other => return Err(EvalError::new(format!("{name} does not support the EndBehavior `{other}`"))),
                    }
                }
                None => false,
            };
            let Some(start) = start else {
                return Ok(axis.empty());
            };
            if count == 0 {
                return Ok(axis.empty());
            }
            let mut boundary = shift(start, count, interval);
            if aligned && align_end(start, interval).floor() == start.floor() {
                boundary = align_end(boundary, interval);
            }
            let (low, high) = if count > 0 { (start, boundary - 1.0) } else { (boundary + 1.0, start) };
            Ok(axis.table(low, high))
        }
        "DATESMTD" => match axis.max {
            Some(max) => Ok(axis.table(month_start(max), max)),
            None => Ok(axis.empty()),
        },
        "DATESQTD" => match axis.max {
            Some(max) => Ok(axis.table(quarter_start(max), max)),
            None => Ok(axis.empty()),
        },
        "DATESWTD" => match axis.max {
            Some(max) => Ok(axis.table(week_start(max), max)),
            None => Ok(axis.empty()),
        },
        "DATESYTD" => {
            let year_end = year_end_arg(ev, args, 1, ctx, name)?;
            match axis.max {
                Some(max) => {
                    let (start, _) = fiscal_bounds(max, year_end);
                    Ok(axis.table(start, max))
                }
                None => Ok(axis.empty()),
            }
        }
        "ENDOFMONTH" => match axis.max {
            Some(max) => Ok(axis.single(month_end(max))),
            None => Ok(axis.empty()),
        },
        "ENDOFQUARTER" => match axis.max {
            Some(max) => Ok(axis.single(quarter_end(max))),
            None => Ok(axis.empty()),
        },
        "ENDOFWEEK" => match axis.max {
            Some(max) => Ok(axis.single(week_end(max))),
            None => Ok(axis.empty()),
        },
        "ENDOFYEAR" => {
            let year_end = year_end_arg(ev, args, 1, ctx, name)?;
            match axis.max {
                Some(max) => {
                    let (_, end) = fiscal_bounds(max, year_end);
                    Ok(axis.single(end))
                }
                None => Ok(axis.empty()),
            }
        }
        "NEXTDAY" => match axis.max {
            Some(max) => Ok(axis.single(max + 1.0)),
            None => Ok(axis.empty()),
        },
        "PREVIOUSDAY" => match axis.min {
            Some(min) => Ok(axis.single(min - 1.0)),
            None => Ok(axis.empty()),
        },
        "NEXTMONTH" => match axis.max {
            Some(max) => {
                let (start, end) = next_month(max);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "PREVIOUSMONTH" => match axis.min {
            Some(min) => {
                let (start, end) = previous_month(min);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "NEXTQUARTER" => match axis.max {
            Some(max) => {
                let (start, end) = next_quarter(max);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "PREVIOUSQUARTER" => match axis.min {
            Some(min) => {
                let (start, end) = previous_quarter(min);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "NEXTWEEK" => match axis.max {
            Some(max) => {
                let (start, end) = next_week(max);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "PREVIOUSWEEK" => match axis.min {
            Some(min) => {
                let (start, end) = previous_week(min);
                Ok(axis.table(start, end))
            }
            None => Ok(axis.empty()),
        },
        "NEXTYEAR" => {
            let year_end = year_end_arg(ev, args, 1, ctx, name)?;
            match axis.max {
                Some(max) => {
                    let (_, end) = fiscal_bounds(max, year_end);
                    let (start, next_end) = fiscal_bounds(end + 1.0, year_end);
                    Ok(axis.table(start, next_end))
                }
                None => Ok(axis.empty()),
            }
        }
        "PREVIOUSYEAR" => {
            let year_end = year_end_arg(ev, args, 1, ctx, name)?;
            match axis.min {
                Some(min) => {
                    let (start, _) = fiscal_bounds(min, year_end);
                    let (previous_start, previous_end) = fiscal_bounds(start - 1.0, year_end);
                    Ok(axis.table(previous_start, previous_end))
                }
                None => Ok(axis.empty()),
            }
        }
        "PARALLELPERIOD" => {
            let count = interval_count(ev, args, 1, ctx, name)?;
            let interval = interval_arg(ev, args, 2, ctx, name, false, false)?;
            let Some((min, max)) = axis.visible() else {
                return Ok(axis.empty());
            };
            let start = align_start(shift(min, count, interval), interval);
            let end = align_end(shift(max, count, interval), interval);
            Ok(axis.table(start, end))
        }
        "SAMEPERIODLASTYEAR" => {
            let Some((min, max)) = axis.visible() else {
                return Ok(axis.empty());
            };
            let start = edate(min, -12);
            let end = edate(max, -12);
            Ok(axis.table(start.min(end), start.max(end)))
        }
        "STARTOFMONTH" => match axis.max {
            Some(max) => Ok(axis.single(month_start(max))),
            None => Ok(axis.empty()),
        },
        "STARTOFQUARTER" => match axis.max {
            Some(max) => Ok(axis.single(quarter_start(max))),
            None => Ok(axis.empty()),
        },
        "STARTOFWEEK" => match axis.max {
            Some(max) => Ok(axis.single(week_start(max))),
            None => Ok(axis.empty()),
        },
        "STARTOFYEAR" => {
            let year_end = year_end_arg(ev, args, 1, ctx, name)?;
            match axis.max {
                Some(max) => {
                    let (start, _) = fiscal_bounds(max, year_end);
                    Ok(axis.single(start))
                }
                None => Ok(axis.empty()),
            }
        }
        _ => Err(EvalError::new(format!("DAX function `{name}` is not implemented by this evaluator"))),
    }
}

fn is_blank_scalar(value: &Scalar) -> bool {
    value.is_blank() || matches!(value, Scalar::String(text) if text.is_empty())
}

/// `FIRSTNONBLANK`/`LASTNONBLANK`: walk the visible values of the column in sort order and return
/// the value whose accompanying expression is not blank.
fn nonblank(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, first: bool) -> Result<TableValue, EvalError> {
    let name = if first { "FIRSTNONBLANK" } else { "LASTNONBLANK" };
    let target = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    let expression = args.get(1).ok_or_else(|| EvalError::new(format!("{name} needs a value expression")))?;
    let (table, column, column_name) = column_target(ev, target, name)?;

    let visible = visible_rows(ev, table, ctx);
    let mut entries: Vec<(ScalarKey, Scalar, usize)> = Vec::new();
    let mut seen: BTreeSet<ScalarKey> = BTreeSet::new();
    for (index, row) in visible.iter().enumerate() {
        if let Some(value) = row.get(column) {
            let key = ScalarKey::from(value);
            if seen.insert(key.clone()) {
                entries.push((key, value.clone(), index));
            }
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    if !first {
        entries.reverse();
    }

    for (key, value, index) in entries {
        let previous = ctx.filters.filters.insert((table, column), BTreeSet::from([key]));
        let row = visible.get(index).cloned().unwrap_or_default();
        ctx.rows.push(model_row_frame(ev, table, row));
        let evaluated = ev.eval(expression, ctx);
        ctx.rows.pop();
        match previous {
            Some(values) => {
                ctx.filters.filters.insert((table, column), values);
            }
            None => {
                ctx.filters.filters.remove(&(table, column));
            }
        }
        if !is_blank_scalar(&evaluated?) {
            return Ok(TableValue::new(vec![TableColumn { name: column_name }], vec![vec![value]]).with_lineage(table));
        }
    }

    Ok(TableValue::new(vec![TableColumn { name: column_name }], Vec::new()).with_lineage(table))
}
