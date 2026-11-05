//! Shared helpers for the category-specific function implementations.
//!
//! Each function family lives in its own module (`financial`, `statistical`, ...) and exposes a
//! `call`/`call_table` dispatcher. Those dispatchers receive the unevaluated arguments and the
//! evaluator's [`Context`](super::Context), so they can choose lazy or eager evaluation and can
//! push row frames for iterators. This module holds the small, boring pieces they share: scalar
//! argument coercion, Excel date serials, rounding and row-frame construction.
#![allow(dead_code)] // helpers are consumed by the per-family modules as they land

use std::cell::RefCell;
use std::cmp::Ordering;

use super::*;
use chrono::{Datelike, NaiveDate};

/// The evaluated arguments of a scalar function call.
pub(crate) struct Args<'a> {
    pub(crate) name: &'a str,
    pub(crate) values: &'a [Scalar],
}

impl<'a> Args<'a> {
    pub(crate) fn arg(&self, index: usize) -> Result<&'a Scalar, EvalError> {
        self.values.get(index).ok_or_else(|| EvalError::new(format!("{} is missing argument {}", self.name, index + 1)))
    }

    pub(crate) fn optional(&self, index: usize) -> Option<&'a Scalar> {
        self.values.get(index)
    }

    pub(crate) fn number(&self, index: usize) -> Result<f64, EvalError> {
        self.arg(index)?.to_number().ok_or_else(|| EvalError::new(format!("{} argument {} must be numeric", self.name, index + 1)))
    }

    pub(crate) fn int(&self, index: usize) -> Result<i64, EvalError> {
        Ok(self.number(index)? as i64)
    }

    pub(crate) fn text(&self, index: usize) -> Result<String, EvalError> {
        Ok(self.arg(index)?.to_text())
    }

    pub(crate) fn boolean(&self, index: usize) -> Result<bool, EvalError> {
        self.arg(index)?.to_bool().ok_or_else(|| EvalError::new(format!("{} argument {} must be Boolean", self.name, index + 1)))
    }

    pub(crate) fn date(&self, index: usize) -> Result<f64, EvalError> {
        let value = self.number(index)?;
        if !value.is_finite() {
            return Err(EvalError::new(format!("{} argument {} must be a date", self.name, index + 1)));
        }
        Ok(value)
    }

    pub(crate) fn optional_number(&self, index: usize, default: f64) -> Result<f64, EvalError> {
        match self.values.get(index) {
            Some(value) if !value.is_blank() => {
                value.to_number().ok_or_else(|| EvalError::new(format!("{} argument {} must be numeric", self.name, index + 1)))
            }
            _ => Ok(default),
        }
    }

    pub(crate) fn optional_bool(&self, index: usize, default: bool) -> Result<bool, EvalError> {
        match self.values.get(index) {
            Some(value) if !value.is_blank() => {
                value.to_bool().ok_or_else(|| EvalError::new(format!("{} argument {} must be Boolean", self.name, index + 1)))
            }
            _ => Ok(default),
        }
    }
}

/// Evaluates every argument eagerly, left to right.
pub(crate) fn eval_all(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Vec<Scalar>, EvalError> {
    args.iter().map(|arg| ev.eval(arg, ctx)).collect()
}

/// Evaluates one argument as a table.
pub(crate) fn eval_table(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &mut Context) -> Result<TableValue, EvalError> {
    ev.eval_table(expr, ctx)
}

/// Builds a row frame for one row of a table value, mapping columns back to the model by name
/// when the table keeps its lineage.
pub(crate) fn frame_for(ev: &Evaluator<'_>, table: &TableValue, row: &[Scalar]) -> RowFrame {
    let names: Vec<String> = table.columns.iter().map(|column| column.name.clone()).collect();
    let model_columns: Vec<Option<usize>> = match table.lineage {
        Some(lineage) => names.iter().map(|name| ev.model().column_index(lineage, name)).collect(),
        None => vec![None; names.len()],
    };
    RowFrame { lineage: table.lineage, names, values: row.to_vec(), model_columns }
}

/// Builds a frame for a plain model table row whose values follow the model column order.
pub(crate) fn model_row_frame(ev: &Evaluator<'_>, table: usize, values: Vec<Scalar>) -> RowFrame {
    let names: Vec<String> =
        ev.model().tables.get(table).map(|table| table.columns.iter().map(|column| column.name.clone()).collect()).unwrap_or_default();
    let model_columns: Vec<Option<usize>> = (0..names.len()).map(Some).collect();
    RowFrame { lineage: Some(table), names, values, model_columns }
}

/// Builds a frame for a derived table row whose columns are given explicitly.
pub(crate) fn derived_row_frame(lineage: Option<usize>, names: Vec<String>, values: Vec<Scalar>, model_columns: Vec<Option<usize>>) -> RowFrame {
    RowFrame { lineage, names, values, model_columns }
}

/// Eagerly evaluated scalar argument access for functions that already hold `Vec<Scalar>`.
pub(crate) fn args<'a>(name: &'a str, values: &'a [Scalar]) -> Args<'a> {
    Args { name, values }
}

/// Orders two scalars for sorting: deterministic, transitive and total over every variant.
///
/// [`Scalar::compare`] follows DAX's `=`/`<` coercion rules and answers `None` for pairs DAX
/// does not compare: text against a number, and anything against NaN. A sort cannot read `None`
/// as equality, because text would then equal every number while the numbers still order among
/// themselves — not a total order, which `sort_by` may panic on (Rust 1.81+).
///
/// This ordering ranks whole classes instead, so mixed columns sort predictably:
///
/// 1. numbers (`Integer`, `Currency`, `Decimal`, `DateTime`) by numeric value, through
///    `f64::total_cmp`, so NaN has a fixed place (after every other number) and `-0.0` sorts
///    before `0.0`;
/// 2. text, case-insensitively (Unicode lowercase, matching [`Scalar::compare`]); strings equal
///    under lowercasing tie-break on the raw strings, so `"A"` and `"a"` still have a fixed
///    order;
/// 3. Booleans, `FALSE` before `TRUE`.
///
/// BLANK is the caller's business. Callers with a blank placement policy (`FIRST`/`LAST` in
/// `RANK`/`ROWNUMBER`, the blank-aware window ordering) place blanks before calling. A blank
/// that does reach this function orders as the number zero, which is DAX's default comparison
/// and keeps the order transitive.
pub(crate) fn total_order(left: &Scalar, right: &Scalar) -> Ordering {
    fn class(value: &Scalar) -> u8 {
        match value {
            Scalar::String(_) => 1,
            Scalar::Boolean(_) => 2,
            Scalar::Blank | Scalar::Integer(_) | Scalar::Currency(_) | Scalar::Decimal(_) | Scalar::DateTime(_) => 0,
        }
    }

    // BLANK counts as zero, as in DAX's default comparison; every other number-class variant has
    // a numeric value, so the NaN fallback is unreachable but keeps the function total.
    fn number(value: &Scalar) -> f64 {
        value.to_number().unwrap_or(f64::NAN)
    }

    match class(left).cmp(&class(right)) {
        Ordering::Equal => match (left, right) {
            (Scalar::String(a), Scalar::String(b)) => a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)),
            (Scalar::Boolean(a), Scalar::Boolean(b)) => a.cmp(b),
            _ => number(left).total_cmp(&number(right)),
        },
        ordering => ordering,
    }
}

/// 400 Gregorian years, after which the calendar (and every month length) repeats.
const GREGORIAN_CYCLE_DAYS: i64 = 146_097;
/// chrono counts days from 0001-01-01; 1970-01-01 is the 719,163rd of them.
const UNIX_EPOCH_CE_DAYS: i64 = 719_163;
/// Excel/OLE serial 0 is 1899-12-30, which is 25,569 days before 1970-01-01.
const UNIX_EPOCH_SERIAL: i64 = 25_569;
/// The CE day of Excel/OLE serial 0 (1899-12-30): `UNIX_EPOCH_CE_DAYS - UNIX_EPOCH_SERIAL`.
const EXCEL_EPOCH_CE_DAYS: i64 = 693_594;

/// Excel/OLE date serial for a calendar date and time. Dates before 1900-03-01 ignore the
/// 1900 leap-year compatibility bug; DAX/Excel only differ there.
pub(crate) fn date_serial(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> f64 {
    // Normalize out-of-range months the way Excel does (DATE(2024, 13, 1) is 2025-01-01). The day
    // counts on from the first of that month, so day 0 is the last day of the month before.
    let months = month - 1;
    let year = year.saturating_add(months.div_euclid(12));
    let month = u32::try_from(months.rem_euclid(12) + 1).unwrap_or(1);
    let days = month_start_ce_days(year, month) - i128::from(EXCEL_EPOCH_CE_DAYS) + i128::from(day) - 1;
    days as f64 + (hour as f64 * 3600.0 + minute as f64 * 60.0 + second as f64) / 86400.0
}

/// Calendar date of an Excel/OLE date serial's whole days.
pub(crate) fn date_parts(serial: f64) -> (i64, i64, i64) {
    let unix = (serial.floor() as i64).saturating_sub(UNIX_EPOCH_SERIAL);
    // The calendar repeats every 400 years (146,097 days), so chrono converts the day within its
    // cycle and the whole cycles go back into the year (AGENTS.md).
    let (cycles, rest) = (unix.div_euclid(GREGORIAN_CYCLE_DAYS), unix.rem_euclid(GREGORIAN_CYCLE_DAYS));
    match i32::try_from(rest + UNIX_EPOCH_CE_DAYS).ok().and_then(NaiveDate::from_num_days_from_ce_opt) {
        Some(date) => {
            let year = i128::from(date.year()) + i128::from(cycles) * 400;
            let year = i64::try_from(year).unwrap_or(if year < 0 { i64::MIN } else { i64::MAX });
            (year, i64::from(date.month()), i64::from(date.day()))
        }
        // Unreachable: `rest + 719_163` is always a valid CE day.
        None => (1970, 1, 1),
    }
}

/// Number of days in a calendar month, `30` for a month outside 1–12 (the value the caller expects
/// for a month the evaluator has already normalized).
pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    if !(1..=12).contains(&month) {
        return 30;
    }
    // The month's length repeats with the 400-year cycle, so the year within it answers and always
    // fits `NaiveDate`.
    let year = year.rem_euclid(400) as i32;
    let month = month as u32;
    let first = |year: i32, month: u32| NaiveDate::from_ymd_opt(year, month, 1);
    match (first(year, month), if month == 12 { first(year + 1, 1) } else { first(year, month + 1) }) {
        (Some(start), Some(next)) => (next - start).num_days(),
        // Unreachable: the year is in 0..=399 and the month is 1..=12.
        _ => 30,
    }
}

/// Whether a year is a Gregorian leap year, for every `i64` year.
pub(crate) fn is_leap_year(year: i64) -> bool {
    // Leap years repeat every 400 years, so the year within its cycle decides and always fits
    // `NaiveDate`: 29 February exists exactly in a leap year.
    NaiveDate::from_ymd_opt(year.rem_euclid(400) as i32, 2, 29).is_some()
}

/// Days since 0001-01-01 (chrono's `num_days_from_ce`) of the first day of a calendar month, for
/// every `i64` year. Whole 400-year Gregorian cycles are added back as days.
fn month_start_ce_days(year: i64, month: u32) -> i128 {
    let cycles = i128::from(year.div_euclid(400));
    let year = year.rem_euclid(400) as i32;
    match NaiveDate::from_ymd_opt(year, month, 1) {
        Some(first) => i128::from(first.num_days_from_ce()) + cycles * i128::from(GREGORIAN_CYCLE_DAYS),
        // Unreachable: the year within its cycle is in 0..=399 and the month is 1..=12.
        None => i128::from(NaiveDate::MIN.num_days_from_ce()),
    }
}

pub(crate) fn date_weekday(serial: f64) -> i64 {
    let days = serial.floor() as i64;
    let weekday = days.rem_euclid(7);
    if weekday == 0 { 7 } else { weekday }
}

/// The year and month `months` away from the month of `serial`, or `None` when the shift leaves
/// the years Excel dates can represent.
///
/// The month total is computed in `i128`: `months` arrives from `number(..) as i64`, which saturates
/// at `i64::MAX`, and `year * 12 + month - 1 + months` would wrap (or panic in debug builds) for
/// such an argument.
fn shift_month(serial: f64, months: i64) -> Option<(i64, i64)> {
    let (year, month, _) = date_parts(serial);
    let total = i128::from(year) * 12 + i128::from(month) - 1 + i128::from(months);
    let year = i64::try_from(total.div_euclid(12)).ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some((year, i64::try_from(total.rem_euclid(12)).ok()? + 1))
}

pub(crate) fn eomonth(serial: f64, months: i64) -> f64 {
    match shift_month(serial, months) {
        Some((year, month)) => date_serial(year, month, days_in_month(year, month), 0, 0, 0),
        // A shift Excel cannot represent leaves the date where it was instead of wrapping.
        None => serial,
    }
}

pub(crate) fn edate(serial: f64, months: i64) -> f64 {
    let (_, _, day) = date_parts(serial);
    match shift_month(serial, months) {
        Some((year, month)) => date_serial(year, month, day.min(days_in_month(year, month)), 0, 0, 0),
        None => serial,
    }
}

/// The local date as an Excel serial (TODAY).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn today_serial() -> f64 {
    now_serial().floor()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn today_serial() -> f64 {
    // The web build has no clock; TODAY/NOW return a fixed serial instead of trapping.
    25569.0
}

/// The local date and time as an Excel serial (NOW).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_serial() -> f64 {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    date_serial(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
    ) + f64::from(now.nanosecond()) / 86_400_000_000_000.0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_serial() -> f64 {
    25569.0
}

/// The UTC date and time as an Excel serial (UTCNOW).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn utc_now_serial() -> f64 {
    use chrono::{Datelike, Timelike};
    let now = chrono::Utc::now();
    date_serial(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
    ) + f64::from(now.nanosecond()) / 86_400_000_000_000.0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn utc_now_serial() -> f64 {
    25569.0
}

/// The UTC date at midnight as an Excel serial (UTCTODAY).
pub(crate) fn utc_today_serial() -> f64 {
    utc_now_serial().floor()
}

/// Rounds `value` to 15 significant digits, the precision Excel and DAX compute with.
///
/// `value * 10^digits` carries the binary representation error of its factors, and a directed
/// rounding makes that error visible: `0.29 * 100` is `28.999999999999996`, so `ROUNDDOWN ( 0.29,
/// 2 )` would truncate to `0.28` and `ROUNDUP ( 0.07, 2 )` would ceil `7.000000000000001` to
/// `0.08`. Snap that noise away before rounding, exactly as `to_dax_text` does when it prints.
fn excel_precision(value: f64) -> f64 {
    // An exact whole number under 2^53 has no binary noise to remove, and snapping it would drop
    // digits: `ROUND ( 1234567890123457, 0 )` must stay that number.
    if is_exact_whole(value) {
        return value;
    }
    format!("{value:.14e}").parse().unwrap_or(value)
}

/// Whether `value` is a whole number held exactly in an `f64`.
fn is_exact_whole(value: f64) -> bool {
    value.fract() == 0.0 && value.abs() < 2f64.powi(53)
}

pub(crate) fn round_to(value: f64, digits: i32) -> f64 {
    // The scale factor overflows or underflows outside f64's exponent range: past +308 digits the
    // value cannot change, past -308 every digit is rounded away.
    if digits > 308 {
        return value;
    }
    if digits < -308 {
        return 0.0;
    }
    // An exact whole number is already rounded: scaling it past 2^53 would only make the snap
    // lose digits, so `ROUND ( 1234567890123457, 1 )` stays that number.
    if digits >= 0 && is_exact_whole(value) {
        return value;
    }
    let factor = 10f64.powi(digits);
    let scaled = excel_precision(value * factor);
    if !scaled.is_finite() {
        return value;
    }
    scaled.round() / factor
}

/// ROUNDUP/ROUNDDOWN: `up` rounds away from zero, otherwise toward zero. Excel and DAX define
/// both in terms of the sign, not the floor/ceiling, so `ROUNDDOWN ( -2.9, 0 )` is -2 and
/// `ROUNDUP ( -2.1, 0 )` is -3.
pub(crate) fn round_direction(value: f64, digits: i32, up: bool) -> f64 {
    // `10^digits` overflows or underflows outside f64's exponent range; the result is the value
    // itself when rounding cannot move any digit, and zero when every digit is rounded away.
    if digits > 308 {
        return value;
    }
    if digits < -308 {
        return if up && value != 0.0 { f64::INFINITY.copysign(value) } else { 0.0 };
    }
    if digits >= 0 && is_exact_whole(value) {
        return value;
    }
    let factor = 10f64.powi(digits);
    let scaled = excel_precision(value * factor);
    if !scaled.is_finite() {
        return value;
    }
    let rounded = if up { scaled.abs().ceil().copysign(scaled) } else { scaled.trunc() };
    rounded / factor
}

/// The greatest common divisor of the two magnitudes, so `i64::MIN` is represented without
/// overflowing the way `i64::abs` would.
pub(crate) fn gcd(a: i64, b: i64) -> u64 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// The least common multiple, or `None` when it does not fit in an `i64` (Excel answers `#NUM!`).
pub(crate) fn lcm(a: i64, b: i64) -> Option<i64> {
    if a == 0 || b == 0 {
        return Some(0);
    }
    (a.unsigned_abs() / gcd(a, b)).checked_mul(b.unsigned_abs()).and_then(|value| i64::try_from(value).ok())
}

/// Applies a scalar aggregation over already-evaluated values.
pub(crate) fn aggregate_values(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    // Plain aggregations take numeric values only: DAX specifies that SUM/AVERAGE/MIN/MAX/PRODUCT
    // ignore logicals and text instead of coercing them. The `*A` variants below keep their own
    // coercion and read `values` directly.
    let numbers: Vec<f64> = values
        .iter()
        .filter(|value| matches!(value, Scalar::Integer(_) | Scalar::Currency(_) | Scalar::Decimal(_) | Scalar::DateTime(_)))
        .filter_map(Scalar::to_number)
        .collect();
    match name.to_ascii_uppercase().as_str() {
        "SUM" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().sum()) }),
        "AVERAGE" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().sum::<f64>() / numbers.len() as f64) }),
        "AVERAGEA" => {
            // Every row counts: text (numeric or not) counts as 0, TRUE/FALSE as 1/0, and a BLANK
            // cell as 0, so it stays in the denominator — Microsoft's example averages 1, 20,
            // "n/a", BLANK and TRUE as 22/5. A column with no rows at all is BLANK.
            if values.is_empty() {
                return Ok(Scalar::Blank);
            }
            let sum: f64 = values
                .iter()
                .map(|value| match value {
                    Scalar::String(_) => 0.0,
                    Scalar::Boolean(value) => f64::from(u8::from(*value)),
                    other => other.to_number().unwrap_or(0.0),
                })
                .sum();
            Ok(Scalar::Decimal(sum / values.len() as f64))
        }
        "PRODUCT" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().product()) }),
        "MIN" | "MAX" => {
            // The winner keeps its own type — a date column answers a DateTime and an integer
            // column an Integer — so this cannot read the `f64` projection the sum-like
            // aggregations use. Only numbers compete: text and logicals are not observations.
            let want = if name.eq_ignore_ascii_case("MIN") { Ordering::Less } else { Ordering::Greater };
            let mut best: Option<&Scalar> = None;
            for value in
                values.iter().filter(|value| matches!(value, Scalar::Integer(_) | Scalar::Currency(_) | Scalar::Decimal(_) | Scalar::DateTime(_)))
            {
                match best {
                    None => best = Some(value),
                    Some(current) if value.compare(current) == Some(want) => best = Some(value),
                    Some(_) => {}
                }
            }
            Ok(best.cloned().unwrap_or(Scalar::Blank))
        }
        "MINA" => Ok(values
            .iter()
            .filter(|value| !value.is_blank())
            .map(|value| match value {
                Scalar::String(_) => 0.0,
                Scalar::Boolean(value) => f64::from(u8::from(*value)),
                other => other.to_number().unwrap_or(0.0),
            })
            .reduce(f64::min)
            .map(Scalar::Decimal)
            .unwrap_or(if values.is_empty() { Scalar::Blank } else { Scalar::Decimal(0.0) })),
        "MAXA" => Ok(values
            .iter()
            .filter(|value| !value.is_blank())
            .map(|value| match value {
                Scalar::String(_) => 0.0,
                Scalar::Boolean(value) => f64::from(u8::from(*value)),
                other => other.to_number().unwrap_or(0.0),
            })
            .reduce(f64::max)
            .map(Scalar::Decimal)
            .unwrap_or(if values.is_empty() { Scalar::Blank } else { Scalar::Decimal(0.0) })),
        "COUNT" => {
            let count = values.iter().filter(|v| !v.is_blank() && !matches!(v, Scalar::Boolean(_))).count();
            Ok(if count == 0 { Scalar::Blank } else { Scalar::Integer(count as i64) })
        }
        "COUNTA" => {
            let count = values.iter().filter(|v| !v.is_blank()).count();
            Ok(if count == 0 { Scalar::Blank } else { Scalar::Integer(count as i64) })
        }
        "COUNTBLANK" => Ok(Scalar::Integer(values.iter().filter(|v| v.is_blank() || v.to_text().is_empty()).count() as i64)),
        "DISTINCTCOUNT" | "APPROXIMATEDISTINCTCOUNT" => {
            let distinct: BTreeSet<ScalarKey> = values.iter().map(distinct_key).collect();
            Ok(if values.is_empty() { Scalar::Blank } else { Scalar::Integer(distinct.len() as i64) })
        }
        "DISTINCTCOUNTNOBLANK" => {
            let distinct: BTreeSet<ScalarKey> = values.iter().filter(|value| !value.is_blank()).map(distinct_key).collect();
            Ok(Scalar::Integer(distinct.len() as i64))
        }
        _ => Err(EvalError::not_implemented(name)),
    }
}

/// Coerces a scalar to a date serial, treating blanks as 0.
pub(crate) fn date_value(value: &Scalar) -> Option<f64> {
    match value {
        Scalar::DateTime(serial) => Some(*serial),
        Scalar::String(text) => parse_date_text(text),
        Scalar::Blank => None,
        other => other.to_number(),
    }
}

/// Parses the common textual date forms Excel accepts (`YYYY-MM-DD`, `MM/DD/YYYY`,
/// `YYYY-MM-DDTHH:MM:SS`). Locale-specific `DD/MM/YYYY` is deliberately ambiguous and rejected.
/// A component outside the calendar (month 13, 30 February) is rejected rather than rolled over.
pub(crate) fn parse_date_text(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (date, time) = match text.split_once(['T', ' ']) {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let parts: Vec<&str> = date.split(['-', '/']).collect();
    let (year, month, day) = match parts.as_slice() {
        [first, second, third] => {
            let first_value: i64 = first.parse().ok()?;
            let second_value: i64 = second.parse().ok()?;
            let third_value: i64 = third.parse().ok()?;
            // A four-digit leading part is `YYYY-MM-DD`; anything else is `MM/DD/YYYY`.
            let (year, month, day) =
                if first.len() >= 4 { (first_value, second_value, third_value) } else { (third_value, first_value, second_value) };
            let year = if year < 100 { year + if year < 30 { 2000 } else { 1900 } } else { year };
            if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
                return None;
            }
            (year, month, day)
        }
        _ => return None,
    };
    let (hour, minute, second) = match time.map(str::trim).filter(|time| !time.is_empty()) {
        None => (0, 0, 0),
        Some(time) => {
            // `datetime_text` prints a date as `M/D/YYYY H:MM:SS AM/PM`, so the form the crate
            // itself writes has to parse back. The suffix decides the half of the day and the
            // hour is then 1-12; without it the hour is 0-23.
            let (clock, meridiem) = match time.rsplit_once(' ') {
                Some((clock, suffix)) if suffix.eq_ignore_ascii_case("AM") || suffix.eq_ignore_ascii_case("PM") => {
                    (clock.trim(), Some(suffix.eq_ignore_ascii_case("PM")))
                }
                _ => (time, None),
            };
            // A present time must parse and be in range: `25:00:00` is rejected rather than
            // rolled into the next day, like an impossible date.
            let parts: Vec<&str> = clock.split(':').collect();
            let hour: i64 = parts.first()?.trim().parse().ok()?;
            let minute: i64 = parts.get(1).unwrap_or(&"0").trim().parse().ok()?;
            let second: i64 = parts.get(2).unwrap_or(&"0").trim().parse().ok()?;
            let hour = match meridiem {
                // 12 AM is midnight and 12 PM is noon; the other PM hours move to the afternoon.
                Some(pm) if (1..=12).contains(&hour) => match (hour, pm) {
                    (12, false) => 0,
                    (12, true) => 12,
                    (hour, true) => hour + 12,
                    (hour, false) => hour,
                },
                Some(_) => return None,
                None => hour,
            };
            if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..60).contains(&second) {
                return None;
            }
            (hour, minute, second)
        }
    };
    Some(date_serial(year, month, day, hour, minute, second))
}

/// Current evaluation frame's innermost row frame, if any.
pub(crate) fn current_frame(ctx: &Context) -> Option<&RowFrame> {
    ctx.rows.last()
}

thread_local! {
    /// Columns that are rolled up (blanked) in the `SUMMARIZE` row currently being evaluated.
    /// `None` outside a `SUMMARIZE` expression, so `ISSUBTOTAL` can tell "detail row" from "not in
    /// a `SUMMARIZE` at all". The state lives here, next to the evaluator's other row-context
    /// state, because the function that reads it (`ISSUBTOTAL`) and the one that writes it
    /// (`SUMMARIZE`) ship in different evaluator families.
    static SUBTOTAL_COLUMNS: RefCell<Option<Vec<(usize, usize)>>> = const { RefCell::new(None) };
}

/// The rolled-up columns of the `SUMMARIZE` row being evaluated, or `None` outside `SUMMARIZE`.
pub(crate) fn subtotal_columns() -> Option<Vec<(usize, usize)>> {
    SUBTOTAL_COLUMNS.with(|columns| columns.borrow().clone())
}

/// Pushes the rolled-up columns for one `SUMMARIZE` row and restores the outer state on drop, so
/// a nested `SUMMARIZE` and an error path both leave the context as they found it.
pub(crate) struct SubtotalGuard(Option<Vec<(usize, usize)>>);

impl SubtotalGuard {
    pub(crate) fn push(columns: Vec<(usize, usize)>) -> Self {
        let previous = SUBTOTAL_COLUMNS.with(|current| current.borrow_mut().replace(columns));
        SubtotalGuard(previous)
    }
}

impl Drop for SubtotalGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        SUBTOTAL_COLUMNS.with(|current| *current.borrow_mut() = previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_round_trip_through_chrono() {
        // Serial 0 is 1899-12-30 and 25,569 is 1970-01-01.
        assert_eq!(date_serial(1899, 12, 30, 0, 0, 0), 0.0);
        assert_eq!(date_serial(1970, 1, 1, 0, 0, 0), 25_569.0);
        assert_eq!(date_parts(0.0), (1899, 12, 30));
        assert_eq!(date_parts(25_569.0), (1970, 1, 1));
        // Leap days exist exactly in leap years.
        assert_eq!(date_serial(2024, 2, 29, 0, 0, 0), 45_351.0);
        assert_eq!(date_parts(45_351.0), (2024, 2, 29));
        assert_eq!(date_serial(2000, 2, 29, 0, 0, 0), date_serial(2000, 3, 1, 0, 0, 0) - 1.0);
        // A time of day is a fraction of the serial.
        assert_eq!(date_serial(1970, 1, 1, 12, 0, 0), 25_569.5);
        assert_eq!(date_parts(25_569.75), (1970, 1, 1));
    }

    #[test]
    fn serial_helpers_extend_past_chronos_year_range() {
        // 400 Gregorian years are 146,097 days, so year 400,000 is 999 cycles after year 400.
        let cycles = 999.0 * 146_097.0;
        assert_eq!(date_serial(400_000, 1, 1, 0, 0, 0) - cycles, date_serial(400, 1, 1, 0, 0, 0));
        assert_eq!(date_parts(date_serial(400_000, 2, 29, 0, 0, 0)), (400_000, 2, 29));
        // The cycle repeats leap-year and month-length rules too.
        assert!(is_leap_year(400_000));
        assert!(!is_leap_year(300_100));
        assert_eq!(days_in_month(400_000, 2), 29);
        assert_eq!(days_in_month(300_100, 2), 28);
    }

    #[test]
    fn out_of_range_months_and_days_roll_over_like_date() {
        // DATE(2024, 13, 1) is 2025-01-01 and day 0 is the last day of the previous month.
        assert_eq!(date_serial(2024, 13, 1, 0, 0, 0), date_serial(2025, 1, 1, 0, 0, 0));
        assert_eq!(date_serial(2024, 0, 1, 0, 0, 0), date_serial(2023, 12, 1, 0, 0, 0));
        assert_eq!(date_serial(2024, 3, 0, 0, 0, 0), date_serial(2024, 2, 29, 0, 0, 0));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
    }

    #[test]
    fn plain_aggregations_ignore_text_logicals_and_blanks() {
        // DAX specifies that SUM/AVERAGE/MIN/MAX/PRODUCT use the numeric rows only: text (even
        // numeric text), logicals and BLANK are ignored, unlike the `*A` variants.
        let values = [
            Scalar::Integer(2),
            Scalar::String("3".into()),
            Scalar::Boolean(true),
            Scalar::Blank,
            Scalar::Decimal(0.5),
            Scalar::Currency(15_000),
            Scalar::DateTime(10.0),
        ];
        assert_eq!(aggregate_values("SUM", &values).expect("SUM"), Scalar::Decimal(14.0));
        assert_eq!(aggregate_values("AVERAGE", &values).expect("AVERAGE"), Scalar::Decimal(3.5));
        // MIN/MAX return the winning value itself: its type survives, so the 10.0 date stays a
        // DateTime rather than becoming a decimal serial.
        assert_eq!(aggregate_values("MIN", &values).expect("MIN"), Scalar::Decimal(0.5));
        assert_eq!(aggregate_values("MAX", &values).expect("MAX"), Scalar::DateTime(10.0));
        assert_eq!(aggregate_values("PRODUCT", &values).expect("PRODUCT"), Scalar::Decimal(15.0));
        let integers = [Scalar::Integer(7), Scalar::Integer(3), Scalar::Blank];
        assert_eq!(aggregate_values("MIN", &integers).expect("MIN"), Scalar::Integer(3));
        assert_eq!(aggregate_values("MAX", &integers).expect("MAX"), Scalar::Integer(7));
        // A column holding only text, logicals and blanks has no numeric observations at all.
        let non_numeric = [Scalar::String("5".into()), Scalar::Boolean(false), Scalar::Blank];
        assert_eq!(aggregate_values("SUM", &non_numeric).expect("SUM"), Scalar::Blank);
        assert_eq!(aggregate_values("AVERAGE", &non_numeric).expect("AVERAGE"), Scalar::Blank);
        // The `*A` variants keep their own coercion: text counts as 0 and TRUE as 1.
        assert_eq!(aggregate_values("MINA", &non_numeric).expect("MINA"), Scalar::Decimal(0.0));
        assert_eq!(aggregate_values("COUNTA", &non_numeric).expect("COUNTA"), Scalar::Integer(2));
    }

    #[test]
    fn date_text_parses_month_first_and_rejects_impossible_dates() {
        // `MM/DD/YYYY`: the leading part is the month, not the year.
        assert_eq!(parse_date_text("1/8/2009"), Some(39_821.0));
        assert_eq!(parse_date_text("1/8/09"), Some(39_821.0));
        // ISO order, and a trailing time is kept.
        assert_eq!(parse_date_text("2024-01-15"), Some(45_306.0));
        assert_eq!(parse_date_text("2024-01-15T06:00:00"), Some(45_306.25));
        // Out-of-range components are rejected instead of rolling over.
        assert_eq!(parse_date_text("2024-02-30"), None);
        assert_eq!(parse_date_text("13/1/2024"), None);
        assert_eq!(parse_date_text("2024-00-10"), None);
        assert_eq!(parse_date_text("2023-02-29"), None);
        assert_eq!(parse_date_text("1/8"), None);
        assert_eq!(parse_date_text(""), None);
    }

    #[test]
    fn date_text_round_trips_the_ampm_form_the_crate_prints() {
        // `datetime_text` writes `M/D/YYYY H:MM:SS AM/PM`, so the same text has to parse back:
        // 12 AM is midnight and 12 PM is noon, and the other PM hours move to the afternoon.
        assert_eq!(parse_date_text("1/2/2024 12:00:00 AM"), Some(date_serial(2024, 1, 2, 0, 0, 0)));
        assert_eq!(parse_date_text("1/2/2024 12:00:00 PM"), Some(date_serial(2024, 1, 2, 12, 0, 0)));
        assert_eq!(parse_date_text("1/2/2024 1:30:15 PM"), Some(date_serial(2024, 1, 2, 13, 30, 15)));
        assert_eq!(parse_date_text("1/2/2024 1:30:15 AM"), Some(date_serial(2024, 1, 2, 1, 30, 15)));
        // A suffixed hour is 1-12, so 13 PM and 0 AM are impossible rather than 13:00 and 00:00.
        assert_eq!(parse_date_text("1/2/2024 13:00:00 PM"), None);
        assert_eq!(parse_date_text("1/2/2024 0:00:00 AM"), None);
    }

    #[test]
    fn rounding_snaps_binary_noise_at_fifteen_digits() {
        // `value * 100` carries the binary error of the input, and a directed rounding exposes it.
        assert_eq!(round_to(1.005, 2), 1.01);
        assert_eq!(round_direction(0.29, 2, false), 0.29);
        assert_eq!(round_direction(0.07, 2, true), 0.07);
        // The ordinary cases still behave: sign-aware directed rounding and half away from zero.
        assert_eq!(round_direction(-2.9, 0, false), -2.0);
        assert_eq!(round_direction(-2.1, 0, true), -3.0);
        assert_eq!(round_to(2.5, 0), 3.0);
        assert_eq!(round_to(2.345, 2), 2.35);
        // An exact whole input is already the answer: scaling it past 2^53 must not snap digits.
        assert_eq!(round_to(1_234_567_890_123_457.0, 1), 1_234_567_890_123_457.0);
        assert_eq!(round_direction(1_234_567_890_123_457.0, 2, true), 1_234_567_890_123_457.0);
    }

    #[test]
    fn gcd_and_lcm_report_results_that_do_not_fit() {
        assert_eq!(gcd(12, 18), 6);
        // `i64::MIN` has no positive counterpart: the magnitudes are used instead of `abs`.
        assert_eq!(gcd(i64::MIN, 2), 2);
        assert_eq!(lcm(4, 6), Some(12));
        assert_eq!(lcm(0, 7), Some(0));
        // Coprime operands above 2^32 overflow the product, which Excel answers with `#NUM!`.
        assert_eq!(lcm(1 << 40, (1 << 40) + 1), None);
    }
}
