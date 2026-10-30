//! Shared helpers for the category-specific function implementations.
//!
//! Each function family lives in its own module (`financial`, `statistical`, ...) and exposes a
//! `call`/`call_table` dispatcher. Those dispatchers receive the unevaluated arguments and the
//! evaluator's [`Context`](super::Context), so they can choose lazy or eager evaluation and can
//! push row frames for iterators. This module holds the small, boring pieces they share: scalar
//! argument coercion, Excel date serials, rounding and row-frame construction.
#![allow(dead_code)] // helpers are consumed by the per-family modules as they land

use super::*;

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

/// Excel/OLE date serial for a calendar date and time. Dates before 1900-03-01 ignore the
/// 1900 leap-year compatibility bug; DAX/Excel only differ there.
pub(crate) fn date_serial(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> f64 {
    let mut y = year;
    let mut m = month;
    // Normalize out-of-range months and days the way Excel does (DATE(2024, 13, 1) is 2025-01-01).
    y += (m - 1).div_euclid(12);
    m = (m - 1).rem_euclid(12) + 1;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468 + 25569;
    days as f64 + (hour as f64 * 3600.0 + minute as f64 * 60.0 + second as f64) / 86400.0
}

pub(crate) fn date_parts(serial: f64) -> (i64, i64, i64) {
    let days = serial.floor() as i64;
    let z = days - 25569 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2), m, d)
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 30,
    }
}

pub(crate) fn date_weekday(serial: f64) -> i64 {
    let days = serial.floor() as i64;
    let weekday = days.rem_euclid(7);
    if weekday == 0 { 7 } else { weekday }
}

pub(crate) fn eomonth(serial: f64, months: i64) -> f64 {
    let (year, month, _) = date_parts(serial);
    let total = year * 12 + month - 1 + months;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    date_serial(y, m, days_in_month(y, m), 0, 0, 0)
}

pub(crate) fn edate(serial: f64, months: i64) -> f64 {
    let (year, month, day) = date_parts(serial);
    let total = year * 12 + month - 1 + months;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    date_serial(y, m, day.min(days_in_month(y, m)), 0, 0, 0)
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
    let factor = 10f64.powi(digits);
    (value * factor).round() / factor
}

/// ROUNDUP/ROUNDDOWN: `up` rounds away from zero, otherwise toward zero. Excel and DAX define
/// both in terms of the sign, not the floor/ceiling, so `ROUNDDOWN ( -2.9, 0 )` is -2 and
/// `ROUNDUP ( -2.1, 0 )` is -3.
pub(crate) fn round_direction(value: f64, digits: i32, up: bool) -> f64 {
    let factor = 10f64.powi(digits);
    let scaled = value * factor;
    let rounded = if up { scaled.abs().ceil().copysign(scaled) } else { scaled.trunc() };
    rounded / factor
}

pub(crate) fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

pub(crate) fn lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 { 0 } else { (a / gcd(a, b) * b).abs() }
}

/// Applies a scalar aggregation over already-evaluated values.
pub(crate) fn aggregate_values(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let numbers: Vec<f64> = values.iter().filter(|v| !v.is_blank()).filter_map(Scalar::to_number).collect();
    match name.to_ascii_uppercase().as_str() {
        "SUM" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().sum()) }),
        "AVERAGE" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().sum::<f64>() / numbers.len() as f64) }),
        "AVERAGEA" => {
            // Blank cells are empty cells, not zeroes: DAX (and Excel) exclude them from the
            // denominator. Non-numeric text and logical values count as 0.
            let present: Vec<&Scalar> = values.iter().filter(|value| !value.is_blank()).collect();
            if present.is_empty() {
                return Ok(Scalar::Blank);
            }
            let sum: f64 = present
                .iter()
                .map(|value| match value {
                    Scalar::String(text) => text.trim().parse().unwrap_or(0.0),
                    Scalar::Boolean(value) => f64::from(u8::from(*value)),
                    other => other.to_number().unwrap_or(0.0),
                })
                .sum();
            Ok(Scalar::Decimal(sum / present.len() as f64))
        }
        "PRODUCT" => Ok(if numbers.is_empty() { Scalar::Blank } else { Scalar::Decimal(numbers.iter().product()) }),
        "MIN" => Ok(numbers.iter().copied().reduce(f64::min).map(Scalar::Decimal).unwrap_or(Scalar::Blank)),
        "MAX" => Ok(numbers.iter().copied().reduce(f64::max).map(Scalar::Decimal).unwrap_or(Scalar::Blank)),
        "MINA" => Ok(values
            .iter()
            .filter(|value| !value.is_blank())
            .map(|value| match value {
                Scalar::String(text) => text.trim().parse().unwrap_or(0.0),
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
                Scalar::String(text) => text.trim().parse().unwrap_or(0.0),
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
        [year, month, day] => {
            let year: i64 = year.parse().ok()?;
            let month: i64 = month.parse().ok()?;
            let day: i64 = day.parse().ok()?;
            if year < 100 { (year + if year < 30 { 2000 } else { 1900 }, month, day) } else { (year, month, day) }
        }
        _ => return None,
    };
    let (hour, minute, second) = time
        .and_then(|time| {
            let parts: Vec<&str> = time.split(':').collect();
            let hour: i64 = parts.first()?.parse().ok()?;
            let minute: i64 = parts.get(1).unwrap_or(&"0").parse().ok()?;
            let second: i64 = parts.get(2).unwrap_or(&"0").parse().ok()?;
            Some((hour, minute, second))
        })
        .unwrap_or_default();
    Some(date_serial(year, month, day, hour, minute, second))
}

/// Current evaluation frame's innermost row frame, if any.
pub(crate) fn current_frame(ctx: &Context) -> Option<&RowFrame> {
    ctx.rows.last()
}
