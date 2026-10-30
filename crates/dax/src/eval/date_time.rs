//! Date and time functions.
//!
//! Dates are OLE/Excel serials (`Scalar::DateTime`), the same values Excel stores: whole days
//! since 1899-12-30 plus a fraction for the time of day. The scalar functions follow Excel's
//! documented results (`WEEKNUM` return types, `YEARFRAC` day-count bases, `NETWORKDAYS` weekend
//! codes), and the table functions build date tables from literal bounds or from the model's
//! DateTime columns.
#![allow(unused_imports)]

use std::collections::BTreeSet;

use super::*;

/// Last serial Excel/DAX accepts as a date: 9999-12-31.
const MAX_DATE_SERIAL: f64 = 2_958_465.0;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "DATEDIFF" => datediff(ev, args, ctx),
        "DATEVALUE" => datevalue(ev, args, ctx),
        "TIME" => time(ev, args, ctx),
        "WEEKDAY" => weekday(ev, args, ctx),
        "NETWORKDAYS" => networkdays(ev, args, ctx),
        "NOW" => now(),
        "QUARTER" => quarter(ev, args, ctx),
        "TIMEVALUE" => timevalue(ev, args, ctx),
        "TODAY" => today(),
        "UTCNOW" => utc_now(),
        "UTCTODAY" => utc_today(),
        "WEEKNUM" => weeknum(ev, args, ctx),
        "YEARFRAC" => yearfrac(ev, args, ctx),
        // Table functions only make sense where a table is expected. Returning an explanatory
        // error beats reporting them as unimplemented when they appear in a scalar context.
        "CALENDAR" | "CALENDARAUTO" => Err(EvalError::new(format!("{upper} returns a table, not a scalar value"))),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "CALENDAR" => calendar(ev, args, ctx),
        "CALENDARAUTO" => calendar_auto(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------------------------
// Argument coercion
// ---------------------------------------------------------------------------------------------

/// Coerces a scalar to a date serial. BLANK is 0 like every other DAX numeric coercion, text is
/// parsed with the common date forms, and numbers are already serials.
fn coerce_date(value: &Scalar) -> Option<f64> {
    match value {
        Scalar::Blank => Some(0.0),
        Scalar::String(text) => parse_date_serial(text).or_else(|| text.trim().parse::<f64>().ok().filter(|serial| serial.is_finite())),
        other => other.to_number().filter(|serial| serial.is_finite()),
    }
}

fn date_arg(name: &str, values: &[Scalar], index: usize) -> Result<f64, EvalError> {
    let value = values.get(index).ok_or_else(|| EvalError::new(format!("{name} is missing argument {}", index + 1)))?;
    let serial = coerce_date(value).ok_or_else(|| EvalError::new(format!("{name} argument {} must be a date", index + 1)))?;
    // Validating the range bounds every date loop (NETWORKDAYS, YEARFRAC) by the calendar.
    if !(0.0..=MAX_DATE_SERIAL).contains(&serial) {
        return Err(EvalError::new(format!("{name} argument {} is outside the supported date range", index + 1)));
    }
    Ok(serial)
}

fn optional_int(name: &str, values: &[Scalar], index: usize, default: i64) -> Result<i64, EvalError> {
    match values.get(index) {
        Some(value) if !value.is_blank() => {
            let number = value.to_number().ok_or_else(|| EvalError::new(format!("{name} argument {} must be numeric", index + 1)))?;
            Ok(number as i64)
        }
        _ => Ok(default),
    }
}

// ---------------------------------------------------------------------------------------------
// Text parsing
// ---------------------------------------------------------------------------------------------

/// Splits `DATE[ T]TIME` text at the first `T` or space. A string without a separator is all
/// date (or all time); the caller decides which.
fn split_date_time(text: &str) -> (&str, Option<&str>) {
    match text.split_once(['T', ' ']) {
        Some((date, time)) => (date.trim(), Some(time.trim())),
        None => (text.trim(), None),
    }
}

/// Parses the date forms `DATEVALUE` accepts on a US-locale client: `YYYY-MM-DD`, `YYYY/MM/DD`,
/// `MM/DD/YYYY` (two-digit years add 2000 below 30 and 1900 otherwise) and the same dates with a
/// trailing time. Returns whole-day serials; the time of day is ignored.
fn parse_date_serial(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (date_part, _) = split_date_time(text);
    let parts: Vec<&str> = date_part.split(['-', '/']).filter(|part| !part.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let numbers: Option<Vec<i64>> = parts.iter().map(|part| part.parse::<i64>().ok()).collect();
    let numbers = numbers?;
    let leading_year = parts[0].len() >= 4 && parts[0].chars().all(|c| c.is_ascii_digit());
    if leading_year {
        // ISO order. The shared parser already knows this shape; re-reading the parts rejects
        // impossible dates (2024-02-30) that its serial rollover would silently accept.
        let serial = support::parse_date_text(text)?;
        if support::date_parts(serial) != (numbers[0], numbers[1], numbers[2]) || !(0.0..=MAX_DATE_SERIAL).contains(&serial) {
            return None;
        }
        return Some(serial.floor());
    }
    // Month-first order, the default of the documented en-US client.
    let (mut year, month, day) = (numbers[2], numbers[0], numbers[1]);
    if year < 100 {
        year += if year < 30 { 2000 } else { 1900 };
    }
    if !(1..=12).contains(&month) || day < 1 || day > support::days_in_month(year, month) {
        return None;
    }
    let serial = support::date_serial(year, month, day, 0, 0, 0);
    (0.0..=MAX_DATE_SERIAL).contains(&serial).then_some(serial)
}

/// Parses a clock time into a day fraction: `HH:MM`, `HH:MM:SS`, fractional seconds optional, and
/// an `AM`/`PM` suffix on a 12-hour clock.
fn parse_clock(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let upper = text.to_ascii_uppercase();
    let (clock, meridiem) = if let Some(rest) = upper.strip_suffix("PM") {
        (rest.trim(), Some(true))
    } else if let Some(rest) = upper.strip_suffix("AM") {
        (rest.trim(), Some(false))
    } else {
        (text, None)
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let hour: i64 = parts[0].trim().parse().ok()?;
    let minute: i64 = parts[1].trim().parse().ok()?;
    let second: f64 = match parts.get(2) {
        Some(part) => part.trim().parse().ok()?,
        None => 0.0,
    };
    if !(0..=59).contains(&minute) || !(0.0..60.0).contains(&second) {
        return None;
    }
    let hour = match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            match (hour, pm) {
                (12, true) => 12,
                (12, false) => 0,
                (hour, true) => hour + 12,
                (hour, false) => hour,
            }
        }
        None => {
            if !(0..=23).contains(&hour) {
                return None;
            }
            hour
        }
    };
    Some((hour as f64 * 3600.0 + minute as f64 * 60.0 + second) / 86_400.0)
}

// ---------------------------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------------------------

/// `DATEVALUE ( <DateText> )` — the date part of a text date as a whole-day serial.
fn datevalue(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let value = values.first().ok_or_else(|| EvalError::new("DATEVALUE is missing argument 1"))?;
    if let Scalar::DateTime(serial) = value {
        return Ok(Scalar::DateTime(serial.floor()));
    }
    let text = value.to_text();
    let serial = parse_date_serial(&text).ok_or_else(|| EvalError::new(format!("DATEVALUE cannot convert `{text}` to a date")))?;
    Ok(Scalar::DateTime(serial))
}

/// `DATEDIFF ( <Date1>, <Date2>, <Interval> )`: the number of interval boundaries crossed between
/// the two dates, positively when Date2 is later. `Interval` is a keyword (the binder keeps it as
/// a keyword expression, which evaluates to its text).
fn datediff(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let start = date_arg("DATEDIFF", &values, 0)?;
    let end = date_arg("DATEDIFF", &values, 1)?;
    let interval = values.get(2).ok_or_else(|| EvalError::new("DATEDIFF is missing argument 3"))?.to_text();
    let (start_year, start_month, _) = support::date_parts(start);
    let (end_year, end_month, _) = support::date_parts(end);
    let elapsed_days = end.floor() - start.floor();
    let count = match interval.trim().to_ascii_uppercase().as_str() {
        "SECOND" => ((end - start) * 86_400.0).trunc(),
        "MINUTE" => ((end - start) * 1_440.0).trunc(),
        "HOUR" => ((end - start) * 24.0).trunc(),
        "DAY" => elapsed_days,
        // Whole weeks crossed, truncated toward zero so the sign follows Date2 - Date1.
        "WEEK" => (elapsed_days / 7.0).trunc(),
        "MONTH" => ((end_year - start_year) * 12 + (end_month - start_month)) as f64,
        "QUARTER" => ((end_year - start_year) * 4 + (end_month - 1) / 3 - (start_month - 1) / 3) as f64,
        "YEAR" => (end_year - start_year) as f64,
        other => {
            return Err(EvalError::new(format!("DATEDIFF interval `{other}` must be SECOND, MINUTE, HOUR, DAY, WEEK, MONTH, QUARTER or YEAR")));
        }
    };
    if !count.is_finite() || count.abs() >= 9_223_372_036_854_775_808.0 {
        return Err(EvalError::new("DATEDIFF result is outside the 64-bit integer range"));
    }
    Ok(Scalar::Integer(count as i64))
}

/// `TIME ( <Hour>, <Minute>, <Second> )`: a time on the 1899-12-30 epoch. Each argument is capped
/// at 32,767 and the total wraps inside one day, as the catalog documents (`TIME ( 50, 0, 0 )` is
/// 02:00, not 02:00 the next day).
fn time(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    const MAX_TIME_PART: f64 = 32_767.0;
    let values = support::eval_all(ev, args, ctx)?;
    let mut parts = [0.0f64; 3];
    for (index, part) in parts.iter_mut().enumerate() {
        let value = values.get(index).ok_or_else(|| EvalError::new(format!("TIME is missing argument {}", index + 1)))?.to_number();
        let value = value.filter(|value| value.is_finite()).ok_or_else(|| EvalError::new(format!("TIME argument {} must be numeric", index + 1)))?;
        if !(0.0..=MAX_TIME_PART).contains(&value) {
            return Err(EvalError::new(format!("TIME argument {} must be between 0 and 32767", index + 1)));
        }
        *part = value;
    }
    let seconds = parts[0] * 3_600.0 + parts[1] * 60.0 + parts[2];
    Ok(Scalar::DateTime(seconds.rem_euclid(86_400.0) / 86_400.0))
}

/// `WEEKDAY ( <Date> [, <ReturnType>] )`: return type 1 numbers Sunday 1 through Saturday 7,
/// 2 numbers Monday 1 through Sunday 7, 3 numbers Monday 0 through Sunday 6, and 11-17 start the
/// week on Monday through Sunday with the first day numbered 1.
fn weekday(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let serial = date_arg("WEEKDAY", &values, 0)?;
    let return_type = optional_int("WEEKDAY", &values, 1, 1)?;
    // Days since the epoch, 1899-12-30 was a Saturday, so serial 0 is Saturday and the
    // zero-based day of week with Sunday first is (days + 6) mod 7.
    let days = serial.floor() as i64;
    let sunday_first = (days + 6).rem_euclid(7);
    let index = match return_type {
        1 => sunday_first + 1,
        2 => (sunday_first + 6).rem_euclid(7) + 1,
        3 => (sunday_first + 6).rem_euclid(7),
        11..=17 => {
            // The first day of the week is Monday for 11 through Sunday for 17.
            let first = (return_type - 10).rem_euclid(7);
            (sunday_first - first).rem_euclid(7) + 1
        }
        other => {
            return Err(EvalError::new(format!("WEEKDAY return type `{other}` must be 1, 2, 3 or 11 to 17")));
        }
    };
    Ok(Scalar::Integer(index))
}

/// `TIMEVALUE ( <TimeText> )` — the time of day as a fraction, ignoring any date portion.
fn timevalue(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let value = values.first().ok_or_else(|| EvalError::new("TIMEVALUE is missing argument 1"))?;
    if let Scalar::DateTime(serial) = value {
        return Ok(Scalar::DateTime(serial.rem_euclid(1.0)));
    }
    let text = value.to_text();
    let fraction = parse_time_text(&text).ok_or_else(|| EvalError::new(format!("TIMEVALUE cannot convert `{text}` to a time")))?;
    Ok(Scalar::DateTime(fraction))
}

fn parse_time_text(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (date_part, time_part) = split_date_time(text);
    if parse_date_serial(date_part).is_some() {
        // A full date-time string keeps only its time; a date alone is midnight.
        return match time_part {
            Some(time) => parse_clock(time),
            None => Some(0.0),
        };
    }
    parse_clock(text)
}

/// `QUARTER ( <Date> )` — 1 (January-March) through 4 (October-December).
fn quarter(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let serial = date_arg("QUARTER", &values, 0)?;
    let (_, month, _) = support::date_parts(serial);
    Ok(Scalar::Integer((month - 1).div_euclid(3) + 1))
}

/// `WEEKNUM ( <Date> [, <ReturnType>] )`.
///
/// Return types 1 and 2-17 use week system 1 (the week containing January 1 is week 1) with the
/// week starting on Sunday (1), Monday (2, 11), Tuesday (12), Wednesday (13), Thursday (14),
/// Friday (15), Saturday (16) or Sunday (17). Return type 21 is ISO 8601, where the week with the
/// first Thursday is week 1 and the result follows the ISO week even across a year boundary.
fn weeknum(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let serial = date_arg("WEEKNUM", &values, 0)?.floor();
    let return_type = optional_int("WEEKNUM", &values, 1, 1)?;
    let week = match return_type {
        21 => iso_week(serial),
        1 => week_system1(serial, 7),
        2 | 11 => week_system1(serial, 1),
        12 => week_system1(serial, 2),
        13 => week_system1(serial, 3),
        14 => week_system1(serial, 4),
        15 => week_system1(serial, 5),
        16 => week_system1(serial, 6),
        17 => week_system1(serial, 7),
        other => return Err(EvalError::new(format!("WEEKNUM return type {other} is not supported"))),
    };
    Ok(Scalar::Integer(week))
}

/// Week number in week system 1 for a week starting on `week_start` (Monday 1 .. Sunday 7).
fn week_system1(serial: f64, week_start: i64) -> i64 {
    let (year, _, _) = support::date_parts(serial);
    let jan1 = support::date_serial(year, 1, 1, 0, 0, 0);
    let day_of_year = (serial - jan1) as i64 + 1;
    let lead = (iso_weekday(jan1) - week_start).rem_euclid(7);
    (day_of_year + lead - 1).div_euclid(7) + 1
}

/// ISO 8601 week number. Dates that belong to the neighbouring ISO year report that year's week
/// number (2021-01-01 is week 53, 2018-12-31 is week 1), matching Excel's return type 21.
fn iso_week(serial: f64) -> i64 {
    // The Thursday of the week decides the ISO year: week 1 is the week that contains it.
    let thursday = serial as i64 - iso_weekday(serial) + 4;
    let year = if thursday < 1 { 1900 } else { support::date_parts(thursday as f64).0 };
    let jan1 = support::date_serial(year, 1, 1, 0, 0, 0) as i64;
    (thursday - jan1).div_euclid(7) + 1
}

/// Weekday with Monday 1 .. Sunday 7. The shared helper returns Excel's default Sunday-first
/// numbering (`date_weekday`), which week-number arithmetic and NETWORKDAYS do not use.
fn iso_weekday(serial: f64) -> i64 {
    ((support::date_weekday(serial) + 5) % 7) + 1
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// `YEARFRAC ( <StartDate>, <EndDate> [, <Basis>] )`.
///
/// Basis 0 is US (NASD) 30/360, 1 actual/actual, 2 actual/360, 3 actual/365 and 4 European 30/360,
/// matching Excel's `YEARFRAC` (the same rules the worksheet engine in `crates/functions` uses).
/// Excel ignores the argument order and returns the positive fraction for a reversed pair.
fn yearfrac(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let start = date_arg("YEARFRAC", &values, 0)?.floor();
    let end = date_arg("YEARFRAC", &values, 1)?.floor();
    let basis = optional_int("YEARFRAC", &values, 2, 0)?;
    if !(0..=4).contains(&basis) {
        return Err(EvalError::new(format!("YEARFRAC basis {basis} is not supported")));
    }
    let (start, end) = if start <= end { (start, end) } else { (end, start) };
    let (year1, month1, day1) = support::date_parts(start);
    let (year2, month2, day2) = support::date_parts(end);
    let days = end - start;
    let fraction = match basis {
        0 => {
            let mut day1 = day1;
            let mut day2 = day2;
            let start_feb_end = month1 == 2 && day1 == support::days_in_month(year1, 2);
            let end_feb_end = month2 == 2 && day2 == support::days_in_month(year2, 2);
            if day1 == 31 && day2 == 31 {
                day1 = 30;
                day2 = 30;
            } else if day1 == 31 {
                day1 = 30;
            } else if day1 == 30 && day2 == 31 {
                day2 = 30;
            } else if start_feb_end && end_feb_end {
                day1 = 30;
                day2 = 30;
            } else if start_feb_end {
                day1 = 30;
            }
            ((year2 - year1) * 360 + (month2 - month1) * 30 + (day2 - day1)) as f64 / 360.0
        }
        1 => {
            let within_year = year1 == year2 || (year2 == year1 + 1 && (month1 > month2 || (month1 == month2 && day1 >= day2)));
            let year_length = if within_year {
                let leap_day_between =
                    (year1..=year2).any(|year| is_leap(year) && (start..=end).contains(&support::date_serial(year, 2, 29, 0, 0, 0)));
                if (year1 == year2 && is_leap(year1)) || leap_day_between || (month2 == 2 && day2 == 29) { 366.0 } else { 365.0 }
            } else {
                let years = (year2 - year1 + 1) as f64;
                let span = support::date_serial(year2 + 1, 1, 1, 0, 0, 0) - support::date_serial(year1, 1, 1, 0, 0, 0);
                span / years
            };
            days / year_length
        }
        2 => days / 360.0,
        3 => days / 365.0,
        _ => {
            let day1 = day1.min(30);
            let day2 = day2.min(30);
            ((year2 - year1) * 360 + (month2 - month1) * 30 + (day2 - day1)) as f64 / 360.0
        }
    };
    Ok(Scalar::Decimal(fraction))
}

/// `NETWORKDAYS ( <start_date>, <end_date> [, <weekend>] [, <holidays>] )` — whole working days,
/// both ends included. Weekend codes are 1 (Saturday/Sunday, the default), 2-7 for the pairs
/// starting on Sunday through Friday, and 11-17 for a single day starting on Sunday. A reversed
/// range returns the negative count, as Excel does.
fn networkdays(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    // The optional holidays argument is a table, so it cannot go through eager scalar evaluation.
    let scalars = args.iter().take(3).map(|arg| ev.eval(arg, ctx)).collect::<Result<Vec<_>, EvalError>>()?;
    let start = date_arg("NETWORKDAYS", &scalars, 0)?.floor();
    let end = date_arg("NETWORKDAYS", &scalars, 1)?.floor();
    let code = optional_int("NETWORKDAYS", &scalars, 2, 1)?;
    let weekends = weekend_days(code)?;
    let holidays = match args.get(3) {
        Some(expr) => {
            let table = support::eval_table(ev, expr, ctx)?;
            holiday_days(&table)?
        }
        None => BTreeSet::new(),
    };
    let (start, end, sign) = if start <= end { (start, end, 1) } else { (end, start, -1) };
    let mut count = 0i64;
    let mut day = start as i64;
    let last = end as i64;
    while day <= last {
        let weekday = iso_weekday(day as f64) as usize;
        if !weekends[weekday] && !holidays.contains(&day) {
            count += 1;
        }
        day += 1;
    }
    Ok(Scalar::Integer(sign * count))
}

/// Weekend lookup indexed by weekday (Monday 1 .. Sunday 7).
fn weekend_days(code: i64) -> Result<[bool; 8], EvalError> {
    let mut days = [false; 8];
    match code {
        1..=7 => {
            let first = ((code + 4) % 7) + 1;
            days[first as usize] = true;
            days[(first % 7 + 1) as usize] = true;
        }
        11..=17 => {
            days[(((code - 11 + 6) % 7) + 1) as usize] = true;
        }
        other => return Err(EvalError::new(format!("NETWORKDAYS weekend code {other} is not supported"))),
    }
    Ok(days)
}

fn holiday_days(table: &TableValue) -> Result<BTreeSet<i64>, EvalError> {
    let mut days = BTreeSet::new();
    if table.columns.is_empty() {
        return Ok(days);
    }
    for row in &table.rows {
        let Some(value) = row.first() else { continue };
        if value.is_blank() {
            continue;
        }
        let serial = coerce_date(value).ok_or_else(|| EvalError::new("NETWORKDAYS holidays must be dates"))?;
        days.insert(serial.floor() as i64);
    }
    Ok(days)
}

// ---------------------------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------------------------

/// `NOW ()` — the current date and time. The clock is read once per evaluation and uses the
/// host's local timezone, as Excel does.
fn now() -> Result<Scalar, EvalError> {
    Ok(Scalar::DateTime(support::now_serial()))
}

/// `TODAY ()` — the current local date at midnight.
fn today() -> Result<Scalar, EvalError> {
    Ok(Scalar::DateTime(support::today_serial()))
}

/// `UTCNOW ()` — the current Coordinated Universal Time date and time.
fn utc_now() -> Result<Scalar, EvalError> {
    Ok(Scalar::DateTime(support::utc_now_serial()))
}

/// `UTCTODAY ()` — the current UTC date at midnight.
fn utc_today() -> Result<Scalar, EvalError> {
    Ok(Scalar::DateTime(support::utc_today_serial()))
}

// ---------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------

/// `CALENDAR ( <StartDate>, <EndDate> )` — one `Date` column with every day in the closed range.
fn calendar(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let start = date_arg("CALENDAR", &values, 0)?.floor();
    let end = date_arg("CALENDAR", &values, 1)?.floor();
    date_table(start, end, "CALENDAR")
}

/// `CALENDARAUTO ( [<FiscalYearEndMonth>] )` — a contiguous date table covering the whole fiscal
/// years spanned by the model's DateTime columns.
fn calendar_auto(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let values = support::eval_all(ev, args, ctx)?;
    let fiscal_end = optional_int("CALENDARAUTO", &values, 0, 12)?;
    if !(1..=12).contains(&fiscal_end) {
        return Err(EvalError::new("CALENDARAUTO FiscalYearEndMonth must be between 1 and 12"));
    }
    let mut bounds: Option<(f64, f64)> = None;
    for table in &ev.model().tables {
        for (index, column) in table.columns.iter().enumerate() {
            let is_datetime = column.data_type.as_deref().is_some_and(|kind| kind.eq_ignore_ascii_case("DateTime"));
            if !is_datetime {
                continue;
            }
            for row in &table.rows {
                let Some(value) = row.get(index) else { continue };
                if value.is_blank() {
                    continue;
                }
                let Some(serial) = coerce_date(value) else { continue };
                let serial = serial.floor();
                bounds = Some(match bounds {
                    Some((min, max)) => (min.min(serial), max.max(serial)),
                    None => (serial, serial),
                });
            }
        }
    }
    let Some((min, max)) = bounds else {
        return Err(EvalError::new("CALENDARAUTO requires a model with at least one DateTime column"));
    };
    let start = fiscal_year_start(min, fiscal_end);
    let end = fiscal_year_end(max, fiscal_end);
    date_table(start, end, "CALENDARAUTO")
}

/// First day of the fiscal year containing `serial` when the fiscal year ends in `end_month`.
fn fiscal_year_start(serial: f64, end_month: i64) -> f64 {
    let (year, month, _) = support::date_parts(serial);
    let fiscal_year = if month <= end_month { year } else { year + 1 };
    support::date_serial(fiscal_year - 1, end_month + 1, 1, 0, 0, 0)
}

/// Last day of the fiscal year containing `serial` when the fiscal year ends in `end_month`.
fn fiscal_year_end(serial: f64, end_month: i64) -> f64 {
    let (year, month, _) = support::date_parts(serial);
    let fiscal_year = if month <= end_month { year } else { year + 1 };
    support::date_serial(fiscal_year, end_month, support::days_in_month(fiscal_year, end_month), 0, 0, 0)
}

/// Builds the one-column `Date` table shared by CALENDAR and CALENDARAUTO.
fn date_table(start: f64, end: f64, name: &str) -> Result<TableValue, EvalError> {
    if end < start {
        return Err(EvalError::new(format!("{name} requires the start date to be on or before the end date")));
    }
    if start < 0.0 || end > MAX_DATE_SERIAL {
        return Err(EvalError::new(format!("{name} dates must be between 1899-12-30 and 9999-12-31")));
    }
    let count = (end - start) as i64 + 1;
    let rows = (0..count).map(|offset| vec![Scalar::DateTime(start + offset as f64)]).collect();
    Ok(TableValue::new(vec![TableColumn { name: "Date".into() }], rows))
}
