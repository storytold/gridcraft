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
    // counts on from the first of that month, so day 0 is the last day of the month before. The
    // arithmetic is in `i128`: `month - 1` overflows for `i64::MIN`, which a saturating cast of a
    // huge negative literal produces.
    let months = i128::from(month) - 1;
    let year = i64::try_from(i128::from(year) + months.div_euclid(12)).unwrap_or(if months < 0 { i64::MIN } else { i64::MAX });
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

/// The largest magnitude that fits an `i64`, as `f64` (2^63).
pub(crate) const I64_MAX_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

/// Truncates a numeric argument to an `i64`, rejecting values that cannot be represented.
///
/// `as i64` saturates instead: `1e19` becomes `i64::MAX`, whose month arithmetic then overflows.
pub(crate) fn truncated_integer(value: f64, name: &str, position: usize) -> Result<i64, EvalError> {
    if !value.is_finite() || value < -I64_MAX_EXCLUSIVE || value >= I64_MAX_EXCLUSIVE {
        return Err(EvalError::new(format!("{name} argument {position} is out of range for an integer")));
    }
    Ok(value.trunc() as i64)
}

pub(crate) fn eomonth(serial: f64, months: i64) -> Result<f64, EvalError> {
    let (year, month, _) = date_parts(serial);
    let total = month_offset_total(year, month, months, "EOMONTH")?;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    Ok(date_serial(y, m, days_in_month(y, m), 0, 0, 0))
}

pub(crate) fn edate(serial: f64, months: i64) -> Result<f64, EvalError> {
    let (year, month, day) = date_parts(serial);
    let total = month_offset_total(year, month, months, "EDATE")?;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    Ok(date_serial(y, m, day.min(days_in_month(y, m)), 0, 0, 0))
}

/// The zero-based month number reached `months` after `year`-`month`, checked so an offset near
/// the `i64` limits reports an error instead of overflowing.
fn month_offset_total(year: i64, month: i64, months: i64, name: &str) -> Result<i64, EvalError> {
    year.checked_mul(12)
        .and_then(|total| total.checked_add(month - 1))
        .and_then(|total| total.checked_add(months))
        .ok_or_else(|| EvalError::new(format!("{name} result is out of range")))
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

pub(crate) fn round_to(value: f64, digits: i32) -> f64 {
    // The scale factor overflows or underflows outside f64's exponent range: past +308 digits the
    // value cannot change, past -308 every digit is rounded away.
    if digits > 308 {
        return value;
    }
    if digits < -308 {
        return 0.0;
    }
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    // Excel rounds the 15-significant-digit decimal the user typed, not the binary double it
    // became: 1.005 is stored just below the midpoint, and 1.005 * 100 = 100.49999999999999, so
    // scaling and rounding gives 1.00 instead of DAX's 1.01. Round the decimal string instead,
    // half away from zero.
    let digits = i64::from(digits.clamp(-400, 400));
    let text = format!("{:.14e}", value.abs());
    let Some((mantissa, exponent)) = text.split_once('e') else { return value };
    let Ok(exponent) = exponent.parse::<i64>() else { return value };
    let mut decimal: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|byte| byte - b'0').collect();
    // value = 0.d1d2…d15 × 10^(exponent+1); keep the digits down to the requested place.
    let keep = exponent + 1 + digits;
    if keep >= decimal.len() as i64 {
        // Nothing to round off, but Excel still keeps only 15 significant digits, which drops
        // binary noise such as 0.1 + 0.2 = 0.30000000000000004.
        return format!("{:.14e}", value).parse().unwrap_or(value);
    }
    if keep < 0 {
        return 0.0;
    }
    let keep = keep as usize;
    let round_up = decimal.get(keep).is_some_and(|&digit| digit >= 5);
    decimal.truncate(keep);
    let mut scale = exponent + 1;
    if round_up {
        let mut index = keep;
        loop {
            if index == 0 {
                decimal.insert(0, 1);
                scale += 1;
                break;
            }
            index -= 1;
            match decimal.get_mut(index) {
                Some(digit) if *digit == 9 => *digit = 0,
                Some(digit) => {
                    *digit += 1;
                    break;
                }
                None => break,
            }
        }
    }
    if decimal.is_empty() {
        return 0.0;
    }
    let digits_text: String = decimal.iter().map(|digit| char::from(b'0' + digit)).collect();
    let rounded: f64 = format!("0.{digits_text}e{scale}").parse().unwrap_or(value.abs());
    if value < 0.0 { -rounded } else { rounded }
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
    let factor = 10f64.powi(digits);
    let scaled = value * factor;
    if !scaled.is_finite() {
        return value;
    }
    let rounded = if up { scaled.abs().ceil().copysign(scaled) } else { scaled.trunc() };
    rounded / factor
}

/// Greatest common divisor of the magnitudes, so `i64::MIN` is handled without overflowing.
pub(crate) fn gcd(a: i64, b: i64) -> u64 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Least common multiple, or `None` when it does not fit an `i64` (`lcm(2^62, 2)` is 2^63).
pub(crate) fn lcm(a: i64, b: i64) -> Option<i64> {
    if a == 0 || b == 0 {
        return Some(0);
    }
    let value = (a.unsigned_abs() / gcd(a, b)).checked_mul(b.unsigned_abs())?;
    i64::try_from(value).ok()
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
        "MIN" => Ok(numbers.iter().copied().reduce(f64::min).map(Scalar::Decimal).unwrap_or(Scalar::Blank)),
        "MAX" => Ok(numbers.iter().copied().reduce(f64::max).map(Scalar::Decimal).unwrap_or(Scalar::Blank)),
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
            let distinct: BTreeSet<ScalarKey> = values.iter().map(ScalarKey::from).collect();
            Ok(if values.is_empty() { Scalar::Blank } else { Scalar::Integer(distinct.len() as i64) })
        }
        "DISTINCTCOUNTNOBLANK" => {
            let distinct: BTreeSet<ScalarKey> = values.iter().filter(|value| !value.is_blank()).map(ScalarKey::from).collect();
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
            // A present time must parse and be in range: `25:00:00` is rejected rather than
            // rolled into the next day, like an impossible date.
            let parts: Vec<&str> = time.split(':').collect();
            let hour: i64 = parts.first()?.trim().parse().ok()?;
            let minute: i64 = parts.get(1).unwrap_or(&"0").trim().parse().ok()?;
            let second: i64 = parts.get(2).unwrap_or(&"0").trim().parse().ok()?;
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
        // `i64::MIN - 1` must not overflow: it is what a huge negative month literal produces.
        assert!(date_serial(2024, i64::MIN, 1, 0, 0, 0).is_finite());
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
        assert_eq!(aggregate_values("MIN", &values).expect("MIN"), Scalar::Decimal(0.5));
        assert_eq!(aggregate_values("MAX", &values).expect("MAX"), Scalar::Decimal(10.0));
        assert_eq!(aggregate_values("PRODUCT", &values).expect("PRODUCT"), Scalar::Decimal(15.0));
        // A column holding only text, logicals and blanks has no numeric observations at all.
        let non_numeric = [Scalar::String("5".into()), Scalar::Boolean(false), Scalar::Blank];
        assert_eq!(aggregate_values("SUM", &non_numeric).expect("SUM"), Scalar::Blank);
        assert_eq!(aggregate_values("AVERAGE", &non_numeric).expect("AVERAGE"), Scalar::Blank);
        // The `*A` variants keep their own coercion: text counts as 0 and TRUE as 1.
        assert_eq!(aggregate_values("MINA", &non_numeric).expect("MINA"), Scalar::Decimal(0.0));
        assert_eq!(aggregate_values("COUNTA", &non_numeric).expect("COUNTA"), Scalar::Integer(2));
    }

    #[test]
    fn month_offsets_that_overflow_error_instead_of_wrapping() {
        let january_15th = date_serial(2024, 1, 15, 0, 0, 0);
        // A month offset near `i64::MAX` cannot be added to the date's own month number.
        assert!(edate(january_15th, i64::MAX).is_err());
        assert!(eomonth(january_15th, i64::MAX).is_err());
        // Valid offsets keep the existing end-of-month clamping.
        assert_eq!(edate(date_serial(2024, 1, 31, 0, 0, 0), 1), Ok(date_serial(2024, 2, 29, 0, 0, 0)));
        assert_eq!(eomonth(january_15th, 1), Ok(date_serial(2024, 2, 29, 0, 0, 0)));
        assert_eq!(eomonth(date_serial(2023, 12, 31, 0, 0, 0), 0), Ok(date_serial(2023, 12, 31, 0, 0, 0)));
    }

    #[test]
    fn round_to_rounds_the_decimal_midpoint_away_from_zero() {
        // Excel rounds the decimal the user typed: the binary double of 1.005 sits just below the
        // midpoint, and ROUND must still carry to 1.01 (CodeRabbit, #3412).
        assert_eq!(round_to(1.005, 2), 1.01);
        assert_eq!(round_to(2.675, 2), 2.68);
        assert_eq!(round_to(-1.005, 2), -1.01);
        assert_eq!(round_to(1.234, 2), 1.23);
        assert_eq!(round_to(10.0, 0), 10.0);
        // Binary noise below 15 significant digits is dropped, as Excel drops it.
        assert_eq!(round_to(0.1 + 0.2, 15), 0.3);
    }

    #[test]
    fn gcd_and_lcm_report_overflow_instead_of_wrapping() {
        // i64::MIN has no positive i64 counterpart, but its magnitude is a valid u64.
        assert_eq!(gcd(i64::MIN, 6), 2);
        assert_eq!(gcd(-12, 18), 6);
        assert_eq!(gcd(0, 0), 0);
        assert_eq!(lcm(0, i64::MIN), Some(0));
        assert_eq!(lcm(4, 6), Some(12));
        assert_eq!(lcm(-4, 6), Some(12));
        // 2^62 * 3 does not fit an i64.
        assert_eq!(lcm(1_i64 << 62, 3), None);
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
}
