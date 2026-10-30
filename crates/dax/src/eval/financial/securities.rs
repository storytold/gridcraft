//! Financial functions: securities, coupons and yields.
//!
//! The coupon functions implement Excel's settlement/maturity coupon schedule: coupon dates run
//! backwards from the maturity date in steps of `12 / frequency` months, clamping the day to the
//! end of the target month. The day-count bases are the Excel ones: 0 US (NASD) 30/360, 1
//! actual/actual, 2 actual/360, 3 actual/365 and 4 European 30/360.
//!
//! The closed forms mirror the formulas published for the Excel worksheet functions
//! (<https://learn.microsoft.com/en-us/dax/price-function-dax> and friends), which is why, for
//! example, `PRICE` uses simple interest when exactly one coupon is left and compounds
//! otherwise.

#![allow(unused_imports)]

use super::*;

use super::super::support;

/// Upper bound on coupon periods between settlement and maturity (a year-9999 quarterly bond is
/// about 32000 periods). Schedules are rejected beyond this instead of looping without a bound.
const MAX_COUPONS: i64 = 40_000;

/// Tolerance used when solving yields: Excel iterates to roughly 1e-7 of par, we converge to
/// double precision.
const YIELD_TOLERANCE: f64 = 1e-12;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    if !handles(&upper) {
        return None;
    }
    let values = match support::eval_all(ev, args, ctx) {
        Ok(values) => values,
        Err(error) => return Some(Err(error)),
    };
    let a = support::args(name, &values);
    Some(dispatch(&upper, &a))
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

fn handles(name: &str) -> bool {
    matches!(
        name,
        "ACCRINT"
            | "ACCRINTM"
            | "COUPDAYBS"
            | "COUPDAYS"
            | "COUPDAYSNC"
            | "COUPNCD"
            | "COUPNUM"
            | "COUPPCD"
            | "DISC"
            | "DURATION"
            | "INTRATE"
            | "MDURATION"
            | "ODDFPRICE"
            | "ODDFYIELD"
            | "ODDLPRICE"
            | "ODDLYIELD"
            | "PRICE"
            | "PRICEDISC"
            | "PRICEMAT"
            | "RECEIVED"
            | "TBILLEQ"
            | "TBILLPRICE"
            | "TBILLYIELD"
            | "YIELD"
            | "YIELDDISC"
            | "YIELDMAT"
    )
}

fn dispatch(name: &str, a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    match name {
        "ACCRINT" => accrint(a),
        "ACCRINTM" => accrintm(a),
        "COUPDAYBS" => coupon_days_before_settlement(a),
        "COUPDAYS" => coupon_period_days(a),
        "COUPDAYSNC" => coupon_days_to_next(a),
        "COUPNCD" => {
            let (_, next, _) = coupon_arguments(a)?;
            Ok(Scalar::DateTime(next))
        }
        "COUPNUM" => {
            let (_, _, count) = coupon_arguments(a)?;
            Ok(Scalar::Integer(count))
        }
        "COUPPCD" => {
            let (previous, _, _) = coupon_arguments(a)?;
            Ok(Scalar::DateTime(previous))
        }
        "DISC" => disc(a),
        "DURATION" => duration(a, false),
        "MDURATION" => duration(a, true),
        "INTRATE" => intrate(a),
        "ODDFPRICE" => oddfprice(a),
        "ODDFYIELD" => oddfyield(a),
        "ODDLPRICE" => oddlprice(a),
        "ODDLYIELD" => oddlyield(a),
        "PRICE" => price(a),
        "PRICEDISC" => pricedisc(a),
        "PRICEMAT" => pricemat(a),
        "RECEIVED" => received(a),
        "TBILLEQ" => tbilleq(a),
        "TBILLPRICE" => tbillprice(a),
        "TBILLYIELD" => tbillyield(a),
        "YIELD" => yield_(a),
        "YIELDDISC" => yielddisc(a),
        "YIELDMAT" => yieldmat(a),
        _ => Err(EvalError::not_implemented(name)),
    }
}

// ---------------------------------------------------------------------------------------------
// Argument handling
// ---------------------------------------------------------------------------------------------

fn num_error(name: &str, detail: &str) -> EvalError {
    EvalError::new(format!("{name}: {detail}"))
}

/// Truncated date argument (Excel truncates dates to whole days).
fn date_arg(a: &support::Args<'_>, index: usize) -> Result<f64, EvalError> {
    let serial = a.date(index)?;
    if !(0.0..=2_958_465.0).contains(&serial) {
        return Err(num_error(a.name, "date argument is out of range"));
    }
    Ok(serial.floor())
}

/// Optional day-count basis: 0..4, rounded to the nearest integer, defaulting to 0.
fn basis_arg(a: &support::Args<'_>, index: usize) -> Result<i64, EvalError> {
    let raw = a.optional_number(index, 0.0)?;
    if !raw.is_finite() {
        return Err(num_error(a.name, "basis must be a number"));
    }
    let basis = raw.round();
    if !(0.0..=4.0).contains(&basis) {
        return Err(num_error(a.name, "basis must be between 0 and 4"));
    }
    Ok(basis as i64)
}

/// Required coupon frequency: 1, 2 or 4, rounded to the nearest integer.
fn frequency_arg(a: &support::Args<'_>, index: usize) -> Result<i64, EvalError> {
    let raw = a.number(index)?;
    if !raw.is_finite() {
        return Err(num_error(a.name, "frequency must be a number"));
    }
    let frequency = raw.round();
    if frequency != 1.0 && frequency != 2.0 && frequency != 4.0 {
        return Err(num_error(a.name, "frequency must be 1, 2 or 4"));
    }
    Ok(frequency as i64)
}

fn require_positive(name: &str, what: &str, value: f64) -> Result<f64, EvalError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(num_error(name, &format!("{what} must be greater than zero")));
    }
    Ok(value)
}

fn require_non_negative(name: &str, what: &str, value: f64) -> Result<f64, EvalError> {
    if !value.is_finite() || value < 0.0 {
        return Err(num_error(name, &format!("{what} must not be negative")));
    }
    Ok(value)
}

fn finite(name: &str, value: f64) -> Result<f64, EvalError> {
    if value.is_finite() { Ok(value) } else { Err(num_error(name, "the result is not a finite number")) }
}

// ---------------------------------------------------------------------------------------------
// Calendar arithmetic
// ---------------------------------------------------------------------------------------------

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn year_of(serial: f64) -> i64 {
    support::date_parts(serial).0
}

fn month_index(serial: f64) -> i64 {
    let (year, month, _) = support::date_parts(serial);
    year * 12 + month - 1
}

/// Adds whole months to a date, clamping the day to the last day of the target month. This is the
/// rule Excel uses to derive coupon dates from the maturity date (Aug 31 minus six months is the
/// last day of February).
fn add_months(serial: f64, months: i64) -> f64 {
    let (year, month, day) = support::date_parts(serial);
    let total = year * 12 + (month - 1) + months;
    let new_year = total.div_euclid(12);
    let new_month = total.rem_euclid(12) + 1;
    let new_day = day.min(support::days_in_month(new_year, new_month));
    support::date_serial(new_year, new_month, new_day, 0, 0, 0)
}

/// Coupon boundary `k` of the schedule anchored on `anchor`: `anchor + months * k`.
fn boundary(anchor: f64, months: i64, k: i64) -> f64 {
    add_months(anchor, months * k)
}

/// Largest `k` such that `boundary(k) <= target`.
fn index_at_or_before(target: f64, anchor: f64, months: i64) -> i64 {
    let mut k = (month_index(target) - month_index(anchor)).div_euclid(months);
    let mut guard = 0;
    while boundary(anchor, months, k + 1) <= target && guard < 8 {
        k += 1;
        guard += 1;
    }
    while boundary(anchor, months, k) > target && guard < 8 {
        k -= 1;
        guard += 1;
    }
    k
}

/// Days between two whole-day serials for the 30/360 conventions, following Excel's US variant
/// (day 31 rolls to the 1st of the next month when the start day is not the 30th, February month
/// ends count as the 30th).
fn days_360(start: f64, end: f64, us: bool) -> i64 {
    let (year1, month1, mut day1) = support::date_parts(start);
    let (mut year2, mut month2, mut day2) = support::date_parts(end);
    if day1 == 31 {
        day1 -= 1;
    } else if us && month1 == 2 && (day1 == 29 || (day1 == 28 && !is_leap_year(year1))) {
        day1 = 30;
    }
    if day2 == 31 {
        if us && day1 != 30 {
            day2 = 1;
            if month2 == 12 {
                year2 += 1;
                month2 = 1;
            } else {
                month2 += 1;
            }
        } else {
            day2 = 30;
        }
    }
    (day2 + month2 * 30 + year2 * 360) - (day1 + month1 * 30 + year1 * 360)
}

/// Signed day count between two dates for a basis.
fn basis_days(start: f64, end: f64, basis: i64) -> f64 {
    if start == end {
        return 0.0;
    }
    if start > end {
        return -basis_days(end, start, basis);
    }
    match basis {
        0 | 4 => {
            let (year1, month1, day1) = support::date_parts(start);
            let (year2, month2, day2) = support::date_parts(end);
            let mut days = ((year2 - year1) * 12 + (month2 - month1)) * 30 + (day2 - day1);
            // Excel's 30/360 US counts a February start in the same year as a 30-day month.
            if basis == 0 && month1 == 2 && month2 != 2 && year1 == year2 {
                days -= if is_leap_year(year1) { 1 } else { 2 };
            }
            days as f64
        }
        _ => end - start,
    }
}

/// Excel `YEARFRAC` semantics, including the actual/actual averaging rule for periods that span
/// more than one calendar year.
fn year_frac(start: f64, end: f64, basis: i64) -> f64 {
    if start == end {
        return 0.0;
    }
    let (start, end) = if start > end { (end, start) } else { (start, end) };
    let (year1, month1, day1) = support::date_parts(start);
    let (year2, month2, day2) = support::date_parts(end);
    let day_diff = year_frac_days(start, end, basis);
    let days_in_year = match basis {
        0 | 2 | 4 => 360.0,
        3 => 365.0,
        _ => {
            // Actual/actual: the denominator is the average year length of the years covered,
            // unless the period stays within one year and does not straddle a leap day.
            let spans_many_years = year1 != year2 && (year2 != year1 + 1 || month1 < month2 || (month1 == month2 && day1 < day2));
            if spans_many_years {
                let mut total = 0.0;
                let mut year = year1;
                while year <= year2 {
                    total += if is_leap_year(year) { 366.0 } else { 365.0 };
                    year += 1;
                }
                total / ((year2 - year1 + 1) as f64)
            } else if year1 == year2 && is_leap_year(year1) {
                366.0
            } else {
                let leap_day = year1 != year2
                    && ((is_leap_year(year1) && (month1 < 2 || (month1 == 2 && day1 <= 29)))
                        || (is_leap_year(year2) && (month2 > 2 || (month2 == 2 && day2 == 29))));
                if leap_day { 366.0 } else { 365.0 }
            }
        }
    };
    day_diff / days_in_year
}

/// The day-count numerator behind [`year_frac`]: the basis-specific number of days between two
/// dates, without dividing by the year length. The odd-period functions measure each cash flow as
/// a fraction of a coupon period with this count.
fn year_frac_days(start: f64, end: f64, basis: i64) -> f64 {
    if start == end {
        return 0.0;
    }
    let (start, end) = if start > end { (end, start) } else { (start, end) };
    let (year1, month1, mut day1) = support::date_parts(start);
    let (year2, month2, mut day2) = support::date_parts(end);
    match basis {
        0 => {
            if day1 == 31 {
                day1 -= 1;
            }
            if day1 == 30 && day2 == 31 {
                day2 -= 1;
            } else if month1 == 2 && day1 == if is_leap_year(year1) { 29 } else { 28 } {
                day1 = 30;
                if month2 == 2 && day2 == if is_leap_year(year2) { 29 } else { 28 } {
                    day2 = 30;
                }
            }
            (((year2 - year1) * 12 + (month2 - month1)) * 30 + (day2 - day1)) as f64
        }
        4 => {
            if day1 == 31 {
                day1 -= 1;
            }
            if day2 == 31 {
                day2 -= 1;
            }
            (((year2 - year1) * 12 + (month2 - month1)) * 30 + (day2 - day1)) as f64
        }
        _ => end - start,
    }
}

/// Excel's `GetYearDiff` used by the discount-style functions: basis days over the number of days
/// of the first date's calendar year.
fn year_diff(start: f64, end: f64, basis: i64) -> f64 {
    let days = basis_days(start, end, basis);
    let days_in_year = match basis {
        1 => {
            if is_leap_year(year_of(start)) {
                366.0
            } else {
                365.0
            }
        }
        3 => 365.0,
        _ => 360.0,
    };
    days / days_in_year
}

// ---------------------------------------------------------------------------------------------
// Coupon schedule
// ---------------------------------------------------------------------------------------------

fn frequency_of(freq: i64) -> f64 {
    freq as f64
}

fn period_months(freq: i64) -> i64 {
    12 / freq
}

/// Returns `(previous_coupon, next_coupon, coupons_remaining)` for a settlement/maturity pair.
fn coupon_schedule(settle: f64, maturity: f64, freq: i64) -> Result<(f64, f64, i64), EvalError> {
    if settle >= maturity {
        return Err(EvalError::new("settlement must be before maturity"));
    }
    let months = period_months(freq);
    // `periods` counts whole coupon periods between settlement and maturity; the coupon at
    // `maturity - periods * months` is the first one after settlement.
    let mut periods = (month_index(maturity) - month_index(settle) + months - 1).div_euclid(months);
    let mut guard = 0;
    while boundary(maturity, months, -periods) <= settle {
        periods -= 1;
        guard += 1;
        if guard > 8 || periods < 0 {
            return Err(EvalError::new("could not derive the coupon schedule"));
        }
    }
    let next = boundary(maturity, months, -periods);
    let previous = boundary(maturity, months, -periods - 1);
    let count = periods + 1;
    if count <= 0 || count > MAX_COUPONS {
        return Err(EvalError::new("the coupon schedule is out of range"));
    }
    Ok((previous, next, count))
}

/// Number of days in the coupon period that contains settlement.
fn coupon_period_length(previous: f64, next: f64, freq: i64, basis: i64) -> f64 {
    match basis {
        1 => next - previous,
        3 => 365.0 / frequency_of(freq),
        _ => 360.0 / frequency_of(freq),
    }
}

/// Days from the beginning of the coupon period to settlement. Coupon schedules use the
/// `YEARFRAC` flavour of each basis (no February start correction, which only applies to the
/// discount-rate helpers below).
fn accrued_days(previous: f64, settle: f64, basis: i64) -> f64 {
    year_frac_days(previous, settle, basis)
}

/// Days from settlement to the next coupon date. Excel uses `E - A` for the 30/360 bases and the
/// actual count for the others, which is why `A + DSC` can differ from `E` for act/360.
fn days_to_next_coupon(settle: f64, next: f64, previous: f64, freq: i64, basis: i64) -> f64 {
    if basis == 0 || basis == 4 {
        coupon_period_length(previous, next, freq, basis) - accrued_days(previous, settle, basis)
    } else {
        year_frac_days(settle, next, basis)
    }
}

/// Common settlement/maturity/frequency handling for the coupon date functions.
fn coupon_arguments(a: &support::Args<'_>) -> Result<(f64, f64, i64), EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let freq = frequency_arg(a, 2)?;
    let _ = basis_arg(a, 3)?;
    coupon_schedule(settle, maturity, freq)
}

/// Full coupon context used by the price/yield functions.
fn coupon_context(settle: f64, maturity: f64, freq: i64, basis: i64) -> Result<(f64, f64, f64, f64), EvalError> {
    let (previous, next, count) = coupon_schedule(settle, maturity, freq)?;
    let period = coupon_period_length(previous, next, freq, basis);
    let accrued = accrued_days(previous, settle, basis);
    let to_next = days_to_next_coupon(settle, next, previous, freq, basis);
    Ok((period, accrued, to_next, count as f64))
}

// ---------------------------------------------------------------------------------------------
// COUP* functions
// ---------------------------------------------------------------------------------------------

fn coupon_period_from_args(a: &support::Args<'_>) -> Result<f64, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let freq = frequency_arg(a, 2)?;
    let basis = basis_arg(a, 3)?;
    let (previous, next, _) = coupon_schedule(settle, maturity, freq)?;
    Ok(coupon_period_length(previous, next, freq, basis))
}

fn coupon_days_before_settlement(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let freq = frequency_arg(a, 2)?;
    let basis = basis_arg(a, 3)?;
    let (previous, _, _) = coupon_schedule(settle, maturity, freq)?;
    Ok(Scalar::Integer(accrued_days(previous, settle, basis).round() as i64))
}

fn coupon_period_days(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    Ok(Scalar::Decimal(coupon_period_from_args(a)?))
}

fn coupon_days_to_next(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let freq = frequency_arg(a, 2)?;
    let basis = basis_arg(a, 3)?;
    let (previous, next, _) = coupon_schedule(settle, maturity, freq)?;
    Ok(Scalar::Integer(days_to_next_coupon(settle, next, previous, freq, basis).round() as i64))
}

// ---------------------------------------------------------------------------------------------
// ACCRINT / ACCRINTM
// ---------------------------------------------------------------------------------------------

fn accrint(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let issue = date_arg(a, 0)?;
    let first_interest = date_arg(a, 1)?;
    let settle = date_arg(a, 2)?;
    let rate = a.number(3)?;
    let par = a.number(4)?;
    let freq = frequency_arg(a, 5)?;
    let basis = basis_arg(a, 6)?;
    let calc_method = a.optional_bool(7, true)?;
    if issue >= settle {
        return Err(num_error(a.name, "issue must be before settlement"));
    }
    require_positive(a.name, "rate", rate)?;
    require_positive(a.name, "par", par)?;

    let months = period_months(freq);
    let start = if calc_method { issue } else { boundary(first_interest, months, index_at_or_before(issue, first_interest, months) + 1) };
    if settle <= start {
        return Ok(Scalar::Decimal(0.0));
    }

    let mut total = 0.0;
    let mut k = index_at_or_before(start, first_interest, months);
    let mut guard = 0;
    loop {
        let period_start = boundary(first_interest, months, k);
        if period_start >= settle {
            break;
        }
        let period_end = boundary(first_interest, months, k + 1);
        let segment_start = if start > period_start { start } else { period_start };
        let segment_end = if settle < period_end { settle } else { period_end };
        if segment_end > segment_start {
            let normal_length = match basis {
                1 => period_end - period_start,
                3 => 365.0 / frequency_of(freq),
                _ => 360.0 / frequency_of(freq),
            };
            if normal_length > 0.0 {
                if segment_start == period_start && segment_end == period_end {
                    total += 1.0;
                } else {
                    total += year_frac_days(segment_start, segment_end, basis) / normal_length;
                }
            }
        }
        k += 1;
        guard += 1;
        if guard > MAX_COUPONS {
            return Err(num_error(a.name, "the accrual schedule is out of range"));
        }
    }
    finite(a.name, par * rate / frequency_of(freq) * total).map(Scalar::Decimal)
}

fn accrintm(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let issue = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let rate = a.number(2)?;
    let par = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if issue >= maturity {
        return Err(num_error(a.name, "issue must be before maturity"));
    }
    require_positive(a.name, "rate", rate)?;
    require_positive(a.name, "par", par)?;
    finite(a.name, par * rate * year_diff(issue, maturity, basis)).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// Simple discount securities
// ---------------------------------------------------------------------------------------------

fn disc(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let price = a.number(2)?;
    let redemption = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_positive(a.name, "price", price)?;
    require_positive(a.name, "redemption", redemption)?;
    let fraction = year_frac(settle, maturity, basis);
    if fraction == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    finite(a.name, (1.0 - price / redemption) / fraction).map(Scalar::Decimal)
}

fn intrate(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let investment = a.number(2)?;
    let redemption = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_positive(a.name, "investment", investment)?;
    require_positive(a.name, "redemption", redemption)?;
    let fraction = year_diff(settle, maturity, basis);
    if fraction == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    finite(a.name, (redemption / investment - 1.0) / fraction).map(Scalar::Decimal)
}

fn received(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let investment = a.number(2)?;
    let discount = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_positive(a.name, "investment", investment)?;
    require_positive(a.name, "discount", discount)?;
    let denominator = 1.0 - discount * year_diff(settle, maturity, basis);
    if denominator <= 0.0 {
        return Err(num_error(a.name, "the discount rate is too large for the period"));
    }
    finite(a.name, investment / denominator).map(Scalar::Decimal)
}

fn pricedisc(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let discount = a.number(2)?;
    let redemption = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_positive(a.name, "discount", discount)?;
    require_positive(a.name, "redemption", redemption)?;
    finite(a.name, redemption * (1.0 - discount * year_diff(settle, maturity, basis))).map(Scalar::Decimal)
}

fn yielddisc(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let price = a.number(2)?;
    let redemption = a.number(3)?;
    let basis = basis_arg(a, 4)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_positive(a.name, "price", price)?;
    require_positive(a.name, "redemption", redemption)?;
    let fraction = year_frac(settle, maturity, basis);
    if fraction == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    finite(a.name, (redemption / price - 1.0) / fraction).map(Scalar::Decimal)
}

fn pricemat(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let issue = date_arg(a, 2)?;
    let rate = a.number(3)?;
    let yld = a.number(4)?;
    let basis = basis_arg(a, 5)?;
    if maturity <= settle || settle <= issue {
        return Err(num_error(a.name, "maturity must be after settlement, which must be after issue"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_non_negative(a.name, "yield", yld)?;
    let issue_to_maturity = year_frac(issue, maturity, basis);
    let issue_to_settle = year_frac(issue, settle, basis);
    let settle_to_maturity = year_frac(settle, maturity, basis);
    let denominator = 1.0 + settle_to_maturity * yld;
    if denominator <= 0.0 {
        return Err(num_error(a.name, "the yield is too negative for the period"));
    }
    let value = (1.0 + issue_to_maturity * rate) / denominator - issue_to_settle * rate;
    finite(a.name, value * 100.0).map(Scalar::Decimal)
}

fn yieldmat(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let issue = date_arg(a, 2)?;
    let rate = a.number(3)?;
    let price = a.number(4)?;
    let basis = basis_arg(a, 5)?;
    if maturity <= settle || settle <= issue {
        return Err(num_error(a.name, "maturity must be after settlement, which must be after issue"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_positive(a.name, "price", price)?;
    let issue_to_maturity = year_frac(issue, maturity, basis);
    let issue_to_settle = year_frac(issue, settle, basis);
    let settle_to_maturity = year_frac(settle, maturity, basis);
    if settle_to_maturity == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    let denominator = price / 100.0 + issue_to_settle * rate;
    if denominator <= 0.0 {
        return Err(num_error(a.name, "the price is too small for the accrued interest"));
    }
    let value = ((1.0 + issue_to_maturity * rate) / denominator - 1.0) / settle_to_maturity;
    finite(a.name, value).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// Treasury bills
// ---------------------------------------------------------------------------------------------

fn tbill_args(a: &support::Args<'_>, value_index: usize) -> Result<(f64, f64, f64), EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let value = a.number(value_index)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    if maturity - settle > 365.0 {
        return Err(num_error(a.name, "maturity cannot be more than one year after settlement"));
    }
    Ok((settle, maturity, value))
}

fn tbillprice(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let (settle, maturity, discount) = tbill_args(a, 2)?;
    require_positive(a.name, "discount", discount)?;
    // A Treasury bill matures one day after the quoted maturity for day-count purposes.
    let fraction = year_frac(settle, maturity + 1.0, 0);
    if fraction == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    finite(a.name, 100.0 * (1.0 - discount * fraction)).map(Scalar::Decimal)
}

fn tbillyield(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let (settle, maturity, price) = tbill_args(a, 2)?;
    require_positive(a.name, "price", price)?;
    let days = days_360(settle, maturity, true) as f64 + 1.0;
    if days <= 0.0 || days > 360.0 {
        return Err(num_error(a.name, "the settlement to maturity period is out of range"));
    }
    finite(a.name, (100.0 / price - 1.0) * 360.0 / days).map(Scalar::Decimal)
}

fn tbilleq(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let (settle, maturity, discount) = tbill_args(a, 2)?;
    require_positive(a.name, "discount", discount)?;
    let days = days_360(settle, maturity + 1.0, true) as f64;
    if days <= 0.0 || days > 360.0 {
        return Err(num_error(a.name, "the settlement to maturity period is out of range"));
    }
    let denominator = 360.0 - discount * days;
    if denominator <= 0.0 {
        return Err(num_error(a.name, "the discount rate is too large for the period"));
    }
    finite(a.name, 365.0 * discount / denominator).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// PRICE / YIELD
// ---------------------------------------------------------------------------------------------

fn price_value(settle: f64, maturity: f64, rate: f64, yld: f64, redemption: f64, freq: i64, basis: i64) -> Result<f64, EvalError> {
    let (period, accrued, to_next, count) = coupon_context(settle, maturity, freq, basis)?;
    if period <= 0.0 {
        return Err(EvalError::new("the coupon period is empty"));
    }
    let frequency = frequency_of(freq);
    let coupon = 100.0 * rate / frequency;
    let factor = 1.0 + yld / frequency;
    if factor <= 0.0 {
        return Err(EvalError::new("the yield is too negative for the coupon frequency"));
    }
    let dsc = to_next / period;
    let a = accrued / period;
    if count == 1.0 {
        let dsr = period - accrued;
        let numerator = coupon + redemption;
        let denominator = 1.0 + yld / frequency * dsr / period;
        if denominator == 0.0 {
            return Err(EvalError::new("the yield produces an undefined price"));
        }
        return Ok(numerator / denominator - coupon * a);
    }
    let mut value = redemption / factor.powf(count - 1.0 + dsc) - coupon * a;
    let mut k = 0.0;
    while k < count {
        value += coupon / factor.powf(k + dsc);
        k += 1.0;
    }
    Ok(value)
}

fn price(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let rate = a.number(2)?;
    let yld = a.number(3)?;
    let redemption = a.number(4)?;
    let freq = frequency_arg(a, 5)?;
    let basis = basis_arg(a, 6)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_non_negative(a.name, "yield", yld)?;
    require_positive(a.name, "redemption", redemption)?;
    finite(a.name, price_value(settle, maturity, rate, yld, redemption, freq, basis)?).map(Scalar::Decimal)
}

#[allow(clippy::too_many_arguments)]
// The signature mirrors YIELD's documented argument list (settlement, maturity, rate, price,
// redemption, frequency, basis) plus the function name for its error messages.
fn yield_from_price(settle: f64, maturity: f64, rate: f64, price: f64, redemption: f64, freq: i64, basis: i64, name: &str) -> Result<f64, EvalError> {
    let (period, accrued, to_next, count) = coupon_context(settle, maturity, freq, basis)?;
    if count == 1.0 {
        let frequency = frequency_of(freq);
        let first = redemption / 100.0 + rate / frequency;
        let second = price / 100.0 + accrued / period * rate / frequency;
        if second == 0.0 || to_next == 0.0 {
            return Err(num_error(name, "the price is out of range"));
        }
        return Ok((first - second) / second * frequency * period / to_next);
    }
    solve_yield(name, |candidate| price_value(settle, maturity, rate, candidate, redemption, freq, basis), price)
}

/// Bracketing bisection for the yield. `price` is strictly decreasing in the yield, so a bracket
/// always exists for a positive price.
fn solve_yield(name: &str, value_at: impl Fn(f64) -> Result<f64, EvalError>, target: f64) -> Result<f64, EvalError> {
    let at_zero = value_at(0.0)?;
    if (at_zero - target).abs() <= YIELD_TOLERANCE {
        return Ok(0.0);
    }
    // Price is decreasing in the yield, so a price above the target means a positive yield.
    let mut low;
    let mut high;
    if at_zero > target {
        low = 0.0;
        let mut delta = 0.5;
        loop {
            let candidate = delta;
            if candidate > 1.0e7 {
                return Err(num_error(name, "no yield reproduces the price"));
            }
            if value_at(candidate)? <= target {
                high = candidate;
                break;
            }
            delta *= 2.0;
        }
    } else {
        high = 0.0;
        let mut delta = 0.5;
        loop {
            let candidate = -delta;
            if candidate <= -1_000_000.0 {
                return Err(num_error(name, "no yield reproduces the price"));
            }
            if value_at(candidate)? >= target {
                low = candidate;
                break;
            }
            delta *= 2.0;
        }
    }
    for _ in 0..256 {
        let middle = 0.5 * (low + high);
        if middle == low || middle == high {
            break;
        }
        let middle_value = value_at(middle)?;
        if (middle_value - target).abs() <= YIELD_TOLERANCE * target.abs().max(1.0) {
            return Ok(middle);
        }
        if middle_value > target {
            low = middle;
        } else {
            high = middle;
        }
    }
    Ok(0.5 * (low + high))
}

fn yield_(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let rate = a.number(2)?;
    let price = a.number(3)?;
    let redemption = a.number(4)?;
    let freq = frequency_arg(a, 5)?;
    let basis = basis_arg(a, 6)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_positive(a.name, "price", price)?;
    require_positive(a.name, "redemption", redemption)?;
    let value = yield_from_price(settle, maturity, rate, price, redemption, freq, basis, a.name)?;
    finite(a.name, value).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// DURATION / MDURATION
// ---------------------------------------------------------------------------------------------

fn duration_value(settle: f64, maturity: f64, coupon_rate: f64, yld: f64, freq: i64, basis: i64) -> Result<f64, EvalError> {
    let (period, _, to_next, count) = coupon_context(settle, maturity, freq, basis)?;
    if period <= 0.0 {
        return Err(EvalError::new("the coupon period is empty"));
    }
    let frequency = frequency_of(freq);
    let coupon = 100.0 * coupon_rate / frequency;
    let factor = 1.0 + yld / frequency;
    if factor <= 0.0 {
        return Err(EvalError::new("the yield is too negative for the coupon frequency"));
    }
    // Excel measures each cash flow at `k - 1 + DSC/E` coupon periods from settlement.
    let offset = to_next / period - 1.0;
    let mut weighted = 0.0;
    let mut present = 0.0;
    let mut k = 1.0;
    while k < count {
        let exponent = k + offset;
        let discounted = coupon / factor.powf(exponent);
        weighted += exponent * discounted;
        present += discounted;
        k += 1.0;
    }
    let exponent = count + offset;
    let discounted = (coupon + 100.0) / factor.powf(exponent);
    weighted += exponent * discounted;
    present += discounted;
    if present == 0.0 {
        return Err(EvalError::new("the duration is undefined for this yield"));
    }
    Ok(weighted / present / frequency)
}

fn duration(a: &support::Args<'_>, modified: bool) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let coupon = a.number(2)?;
    let yld = a.number(3)?;
    let freq = frequency_arg(a, 4)?;
    let basis = basis_arg(a, 5)?;
    if settle >= maturity {
        return Err(num_error(a.name, "settlement must be before maturity"));
    }
    require_non_negative(a.name, "coupon", coupon)?;
    require_non_negative(a.name, "yield", yld)?;
    let mut value = duration_value(settle, maturity, coupon, yld, freq, basis)?;
    if modified {
        value /= 1.0 + yld / frequency_of(freq);
    }
    finite(a.name, value).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// Odd first period
// ---------------------------------------------------------------------------------------------

/// Fractions of coupon periods covered by `[from, to]`, summed over the quasi-coupon periods
/// anchored on a coupon date. This is Excel's `Σ days / normal length` used by the odd-period
/// formulas; for every basis except actual/actual the normal length is the fixed coupon period.
fn quasi_period_units(from: f64, to: f64, anchor: f64, months: i64, period: f64, basis: i64) -> f64 {
    if to <= from {
        return 0.0;
    }
    let mut k = index_at_or_before(from, anchor, months);
    let mut total = 0.0;
    let mut guard = 0;
    while boundary(anchor, months, k) < to {
        let start = boundary(anchor, months, k);
        let end = boundary(anchor, months, k + 1);
        let segment_start = if from > start { from } else { start };
        let segment_end = if to < end { to } else { end };
        if segment_end > segment_start {
            let normal = if basis == 1 { end - start } else { period };
            if normal > 0.0 {
                total += year_frac_days(segment_start, segment_end, basis) / normal;
            }
        }
        k += 1;
        guard += 1;
        if guard > MAX_COUPONS {
            break;
        }
    }
    total
}

#[allow(clippy::too_many_arguments)]
// ODDFPRICE takes eight date/rate arguments plus the function name, as Excel documents them.
fn oddfprice_value(
    settle: f64,
    maturity: f64,
    issue: f64,
    first_coupon: f64,
    rate: f64,
    yld: f64,
    redemption: f64,
    freq: i64,
    basis: i64,
) -> Result<f64, EvalError> {
    let (previous, next, _) = coupon_schedule(settle, maturity, freq)?;
    let period = coupon_period_length(previous, next, freq, basis);
    if period <= 0.0 {
        return Err(EvalError::new("the coupon period is empty"));
    }
    let frequency = frequency_of(freq);
    let coupon = 100.0 * rate / frequency;
    let factor = 1.0 + yld / frequency;
    if factor <= 0.0 {
        return Err(EvalError::new("the yield is too negative for the coupon frequency"));
    }
    let months = period_months(freq);

    // The odd first coupon is the prorated sum of the quasi-coupon periods between the issue
    // date and the first real coupon date; the buyer owes the part accrued since the issue date.
    let first_coupon_units = quasi_period_units(issue, first_coupon, first_coupon, months, period, basis);
    let accrued_units = quasi_period_units(issue, settle, first_coupon, months, period, basis);
    let to_first = quasi_period_units(settle, first_coupon, first_coupon, months, period, basis);

    let remaining = coupon_schedule(first_coupon, maturity, freq)?.2 as f64;
    let mut value = coupon * first_coupon_units / factor.powf(to_first);
    value += redemption / factor.powf(to_first + remaining);
    value -= coupon * accrued_units;
    let mut j = 1.0;
    while j <= remaining {
        value += coupon / factor.powf(to_first + j);
        j += 1.0;
    }
    Ok(value)
}

fn oddfprice(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let issue = date_arg(a, 2)?;
    let first_coupon = date_arg(a, 3)?;
    let rate = a.number(4)?;
    let yld = a.number(5)?;
    let redemption = a.number(6)?;
    let freq = frequency_arg(a, 7)?;
    let basis = basis_arg(a, 8)?;
    if !(maturity > first_coupon && first_coupon > settle && settle > issue) {
        return Err(num_error(a.name, "maturity, first coupon, settlement and issue must be in order"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_non_negative(a.name, "yield", yld)?;
    require_positive(a.name, "redemption", redemption)?;
    finite(a.name, oddfprice_value(settle, maturity, issue, first_coupon, rate, yld, redemption, freq, basis)?).map(Scalar::Decimal)
}

fn oddfyield(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let issue = date_arg(a, 2)?;
    let first_coupon = date_arg(a, 3)?;
    let rate = a.number(4)?;
    let price = a.number(5)?;
    let redemption = a.number(6)?;
    let freq = frequency_arg(a, 7)?;
    let basis = basis_arg(a, 8)?;
    if !(maturity > first_coupon && first_coupon > settle && settle > issue) {
        return Err(num_error(a.name, "maturity, first coupon, settlement and issue must be in order"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_positive(a.name, "price", price)?;
    require_positive(a.name, "redemption", redemption)?;
    let value =
        solve_yield(a.name, |candidate| oddfprice_value(settle, maturity, issue, first_coupon, rate, candidate, redemption, freq, basis), price)?;
    finite(a.name, value).map(Scalar::Decimal)
}

// ---------------------------------------------------------------------------------------------
// Odd last period
// ---------------------------------------------------------------------------------------------

fn oddl_units(settle: f64, maturity: f64, last_interest: f64, freq: i64, basis: i64) -> Result<(f64, f64, f64), EvalError> {
    let (previous, next, _) = coupon_schedule(settle, maturity, freq)?;
    let period = coupon_period_length(previous, next, freq, basis);
    if period <= 0.0 {
        return Err(EvalError::new("the coupon period is empty"));
    }
    // Quasi-coupon periods of the odd last period are anchored on the maturity date.
    let months = period_months(freq);
    let to_maturity = quasi_period_units(settle, maturity, maturity, months, period, basis);
    let odd_period = quasi_period_units(last_interest, maturity, maturity, months, period, basis);
    let accrued = quasi_period_units(last_interest, settle, maturity, months, period, basis);
    Ok((to_maturity, odd_period, accrued))
}

fn oddlprice(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let last_interest = date_arg(a, 2)?;
    let rate = a.number(3)?;
    let yld = a.number(4)?;
    let redemption = a.number(5)?;
    let freq = frequency_arg(a, 6)?;
    let basis = basis_arg(a, 7)?;
    if !(maturity > settle && settle > last_interest) {
        return Err(num_error(a.name, "maturity must be after settlement, which must be after the last interest date"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_non_negative(a.name, "yield", yld)?;
    require_positive(a.name, "redemption", redemption)?;
    let (to_maturity, odd_period, accrued) = oddl_units(settle, maturity, last_interest, freq, basis)?;
    let frequency = frequency_of(freq);
    let coupon = 100.0 * rate / frequency;
    let denominator = 1.0 + yld / frequency * to_maturity;
    if denominator <= 0.0 {
        return Err(num_error(a.name, "the yield is too negative for the period"));
    }
    let value = (redemption + odd_period * coupon) / denominator - accrued * coupon;
    finite(a.name, value).map(Scalar::Decimal)
}

fn oddlyield(a: &support::Args<'_>) -> Result<Scalar, EvalError> {
    let settle = date_arg(a, 0)?;
    let maturity = date_arg(a, 1)?;
    let last_interest = date_arg(a, 2)?;
    let rate = a.number(3)?;
    let price = a.number(4)?;
    let redemption = a.number(5)?;
    let freq = frequency_arg(a, 6)?;
    let basis = basis_arg(a, 7)?;
    if !(maturity > settle && settle > last_interest) {
        return Err(num_error(a.name, "maturity must be after settlement, which must be after the last interest date"));
    }
    require_non_negative(a.name, "rate", rate)?;
    require_positive(a.name, "price", price)?;
    require_positive(a.name, "redemption", redemption)?;
    let (to_maturity, odd_period, accrued) = oddl_units(settle, maturity, last_interest, freq, basis)?;
    if to_maturity == 0.0 {
        return Err(num_error(a.name, "the settlement to maturity period is empty"));
    }
    let frequency = frequency_of(freq);
    let coupon = 100.0 * rate / frequency;
    let denominator = price + accrued * coupon;
    if denominator == 0.0 {
        return Err(num_error(a.name, "the price is too small for the accrued interest"));
    }
    let value = ((redemption + odd_period * coupon) / denominator - 1.0) * frequency / to_maturity;
    finite(a.name, value).map(Scalar::Decimal)
}
