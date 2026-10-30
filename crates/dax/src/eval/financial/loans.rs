//! Financial functions: loans, depreciation, cash flows and rates.
//!
//! Excel-compatible time-value-of-money, depreciation and irregular cash-flow functions.
//! Payments are made at the end of each period unless `Type` is non-zero. Rate solving uses a
//! bounded Newton iteration with a bracketing bisection fallback; interest-rate domains are
//! validated before any logarithm or fractional power so invalid input yields `EvalError`
//! rather than NaN.
#![allow(unused_imports)]

use super::support::{args, date_parts, date_serial, date_value, days_in_month, eval_all, eval_table, frame_for};
use super::*;

/// Upper bound for depreciation loops: keeps hostile `Life` values from stalling evaluation.
const MAX_DEPRECIATION_PERIODS: f64 = 1_000_000.0;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let name = upper.as_str();
    let result = match name {
        "AMORDEGRC" => eager(ev, name, args, ctx, amordegrc),
        "AMORLINC" => eager(ev, name, args, ctx, amorlinc),
        "CUMIPMT" => eager(ev, name, args, ctx, cumipmt),
        "CUMPRINC" => eager(ev, name, args, ctx, cumprinc),
        "DB" => eager(ev, name, args, ctx, db),
        "DDB" => eager(ev, name, args, ctx, ddb),
        "DOLLARDE" => eager(ev, name, args, ctx, dollarde),
        "DOLLARFR" => eager(ev, name, args, ctx, dollarfr),
        "EFFECT" => eager(ev, name, args, ctx, effect),
        "FV" => eager(ev, name, args, ctx, fv),
        "IPMT" => eager(ev, name, args, ctx, ipmt),
        "ISPMT" => eager(ev, name, args, ctx, ispmt),
        "NOMINAL" => eager(ev, name, args, ctx, nominal),
        "NPER" => eager(ev, name, args, ctx, nper),
        "PDURATION" => eager(ev, name, args, ctx, pduration),
        "PMT" => eager(ev, name, args, ctx, pmt),
        "PPMT" => eager(ev, name, args, ctx, ppmt),
        "PV" => eager(ev, name, args, ctx, pv),
        "RATE" => eager(ev, name, args, ctx, rate),
        "RRI" => eager(ev, name, args, ctx, rri),
        "SLN" => eager(ev, name, args, ctx, sln),
        "SYD" => eager(ev, name, args, ctx, syd),
        "VDB" => eager(ev, name, args, ctx, vdb),
        "XIRR" => xirr(ev, name, args, ctx),
        "XNPV" => xnpv(ev, name, args, ctx),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

fn eager(
    ev: &Evaluator<'_>,
    name: &str,
    args: &[BoundExpr],
    ctx: &mut Context,
    function: fn(&str, &[Scalar]) -> Result<Scalar, EvalError>,
) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    function(name, &values)
}

fn decimal(name: &str, value: f64) -> Result<Scalar, EvalError> {
    if value.is_finite() { Ok(Scalar::Decimal(value)) } else { Err(EvalError::new(format!("{name} result is not a finite number"))) }
}

fn invalid(name: &str, message: &str) -> EvalError {
    EvalError::new(format!("{name} {message}"))
}

fn division_by_zero(name: &str) -> EvalError {
    invalid(name, "cannot divide by zero")
}

fn type_flag(value: f64) -> bool {
    value != 0.0
}

/// `(1 + rate) ^ nper` via `ln1p`/`exp`, valid only for `rate > -1`.
fn growth(rate: f64, nper: f64) -> f64 {
    (nper * rate.ln_1p()).exp()
}

fn check_rate(name: &str, rate: f64) -> Result<(), EvalError> {
    if rate <= -1.0 || !rate.is_finite() {
        return Err(invalid(name, "rate must be greater than -1"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Time value of money
// ---------------------------------------------------------------------------

fn pmt(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let nper = a.number(1)?;
    let pv = a.number(2)?;
    let fv = a.optional_number(3, 0.0)?;
    let advance = type_flag(a.optional_number(4, 0.0)?);
    decimal(name, payment(name, rate, nper, pv, fv, advance)?)
}

fn payment(name: &str, rate: f64, nper: f64, pv: f64, fv: f64, advance: bool) -> Result<f64, EvalError> {
    check_rate(name, rate)?;
    if rate == 0.0 {
        if nper == 0.0 {
            return Err(division_by_zero(name));
        }
        return Ok(-(pv + fv) / nper);
    }
    let factor = growth(rate, nper);
    let annuity = factor - 1.0;
    if annuity == 0.0 {
        return Err(division_by_zero(name));
    }
    let advance_factor = if advance { 1.0 + rate } else { 1.0 };
    Ok(-(rate * (pv * factor + fv)) / (advance_factor * annuity))
}

fn fv(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let nper = a.number(1)?;
    let pmt = a.number(2)?;
    let pv = a.optional_number(3, 0.0)?;
    let advance = type_flag(a.optional_number(4, 0.0)?);
    decimal(name, future_value(name, rate, nper, pmt, pv, advance)?)
}

fn future_value(name: &str, rate: f64, nper: f64, pmt: f64, pv: f64, advance: bool) -> Result<f64, EvalError> {
    check_rate(name, rate)?;
    if rate == 0.0 {
        return Ok(-(pv + pmt * nper));
    }
    let factor = growth(rate, nper);
    let advance_factor = if advance { 1.0 + rate } else { 1.0 };
    Ok(-(pv * factor + pmt * advance_factor * (factor - 1.0) / rate))
}

fn pv(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let nper = a.number(1)?;
    let pmt = a.number(2)?;
    let fv = a.optional_number(3, 0.0)?;
    let advance = type_flag(a.optional_number(4, 0.0)?);
    decimal(name, present_value(name, rate, nper, pmt, fv, advance)?)
}

fn present_value(name: &str, rate: f64, nper: f64, pmt: f64, fv: f64, advance: bool) -> Result<f64, EvalError> {
    check_rate(name, rate)?;
    if rate == 0.0 {
        return Ok(-(fv + pmt * nper));
    }
    let factor = growth(rate, nper);
    if factor == 0.0 {
        return Err(division_by_zero(name));
    }
    let advance_factor = if advance { 1.0 + rate } else { 1.0 };
    Ok(-(fv + pmt * advance_factor * (factor - 1.0) / rate) / factor)
}

fn nper(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let pmt = a.number(1)?;
    let pv = a.number(2)?;
    let fv = a.optional_number(3, 0.0)?;
    let advance = type_flag(a.optional_number(4, 0.0)?);
    check_rate(name, rate)?;
    if pv + fv == 0.0 {
        return Ok(Scalar::Decimal(0.0));
    }
    if rate == 0.0 {
        if pmt == 0.0 {
            return Err(division_by_zero(name));
        }
        return decimal(name, -(pv + fv) / pmt);
    }
    let (numerator, denominator) =
        if advance { (pmt * (1.0 + rate) - rate * fv, rate * pv + pmt * (1.0 + rate)) } else { (pmt - rate * fv, rate * pv + pmt) };
    if denominator == 0.0 {
        return Err(division_by_zero(name));
    }
    let ratio = numerator / denominator;
    if ratio <= 0.0 || !ratio.is_finite() {
        return Err(invalid(name, "has no solution for these arguments"));
    }
    decimal(name, ratio.ln() / rate.ln_1p())
}

fn ipmt(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let per = a.number(1)?;
    let nper = a.number(2)?;
    let pv = a.number(3)?;
    let fv = a.optional_number(4, 0.0)?;
    let advance = type_flag(a.optional_number(5, 0.0)?);
    let (_, interest) = interest_and_payment(name, rate, per, nper, pv, fv, advance)?;
    decimal(name, interest)
}

fn ppmt(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let per = a.number(1)?;
    let nper = a.number(2)?;
    let pv = a.number(3)?;
    let fv = a.optional_number(4, 0.0)?;
    let advance = type_flag(a.optional_number(5, 0.0)?);
    let (pmt, interest) = interest_and_payment(name, rate, per, nper, pv, fv, advance)?;
    decimal(name, pmt - interest)
}

fn interest_and_payment(name: &str, rate: f64, per: f64, nper: f64, pv: f64, fv: f64, advance: bool) -> Result<(f64, f64), EvalError> {
    if per < 1.0 || per > nper || !per.is_finite() {
        return Err(invalid(name, "period must be between 1 and nper"));
    }
    let pmt = payment(name, rate, nper, pv, fv, advance)?;
    let interest = if per == 1.0 {
        if advance { 0.0 } else { -pv }
    } else if advance {
        future_value(name, rate, per - 2.0, pmt, pv, true)? - pmt
    } else {
        future_value(name, rate, per - 1.0, pmt, pv, false)?
    };
    Ok((pmt, interest * rate))
}

fn cumipmt(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let nper = a.number(1)?;
    let pv = a.number(2)?;
    let start = a.number(3)?;
    let end = a.number(4)?;
    let advance = type_flag(a.number(5)?);
    decimal(name, cumulative_interest(name, rate, nper, pv, start, end, advance)?)
}

fn cumprinc(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let nper = a.number(1)?;
    let pv = a.number(2)?;
    let start = a.number(3)?;
    let end = a.number(4)?;
    let advance = type_flag(a.number(5)?);
    let interest = cumulative_interest(name, rate, nper, pv, start, end, advance)?;
    let pmt = payment(name, rate, nper, pv, 0.0, advance)?;
    let count = end.floor() - start.floor() + 1.0;
    decimal(name, count * pmt - interest)
}

fn cumulative_interest(name: &str, rate: f64, nper: f64, pv: f64, start: f64, end: f64, advance: bool) -> Result<f64, EvalError> {
    if rate <= 0.0 || nper <= 0.0 || pv <= 0.0 {
        return Err(invalid(name, "rate, nper and pv must be positive"));
    }
    if start < 1.0 || end < start || end > nper {
        return Err(invalid(name, "start_period and end_period must lie within 1..nper"));
    }
    let start = start.floor();
    let end = end.floor();
    let pmt = payment(name, rate, nper, pv, 0.0, advance)?;
    // Closed form for the sum of IPMT over a period range; avoids a loop whose length is
    // proportional to `nper`.
    let (prefix_end, prefix_start) = if advance {
        (advance_interest_prefix(rate, end, pv, pmt), if start > 1.0 { advance_interest_prefix(rate, start - 1.0, pv, pmt) } else { 0.0 })
    } else {
        (arrear_interest_prefix(rate, end, pv, pmt), if start > 1.0 { arrear_interest_prefix(rate, start - 1.0, pv, pmt) } else { 0.0 })
    };
    Ok(prefix_end - prefix_start)
}

/// Sum of `IPMT` for periods `1..=end` when payments are in arrears.
fn arrear_interest_prefix(rate: f64, end: f64, pv: f64, pmt: f64) -> f64 {
    if end <= 0.0 {
        return 0.0;
    }
    let factor = growth(rate, end);
    -(pv * rate + pmt) * (factor - 1.0) / rate + end * pmt
}

/// Sum of `IPMT` for periods `1..=end` when payments are in advance.
fn advance_interest_prefix(rate: f64, end: f64, pv: f64, pmt: f64) -> f64 {
    if end <= 1.0 {
        return 0.0;
    }
    let factor = growth(rate, end - 1.0);
    -(pv * rate + pmt * (1.0 + rate)) * (factor - 1.0) / rate + (end - 1.0) * pmt
}

fn ispmt(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let per = a.number(1)?;
    let nper = a.number(2)?;
    let pv = a.number(3)?;
    let share = if nper == 0.0 { 0.0 } else { per / nper };
    decimal(name, pv * rate * (share - 1.0))
}

fn rate(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let nper = a.number(0)?;
    let pmt = a.number(1)?;
    let pv = a.number(2)?;
    let fv = a.optional_number(3, 0.0)?;
    let advance = type_flag(a.optional_number(4, 0.0)?);
    let has_guess = values.get(5).is_some_and(|value| !value.is_blank());
    let guess = a.optional_number(5, 0.1)?;
    if nper <= 0.0 {
        return Err(invalid(name, "nper must be positive"));
    }
    if let Some(found) = rate_iteration(nper, pmt, pv, fv, advance, guess) {
        return decimal(name, found);
    }
    if !has_guess {
        // Excel retries with scaled default guesses when the initial 10% guess fails.
        for step in 2..=10 {
            for factor in [step as f64, 1.0 / step as f64] {
                if let Some(found) = rate_iteration(nper, pmt, pv, fv, advance, 0.1 * factor) {
                    return decimal(name, found);
                }
            }
        }
    }
    Err(invalid(name, "did not converge"))
}

/// Bounded Newton-Raphson solve of `fv + pv*(1+r)^n + pmt*((1+r)^n-1)/r = 0`.
fn rate_iteration(nper: f64, pmt: f64, mut pv: f64, mut fv: f64, advance: bool, guess: f64) -> Option<f64> {
    let pmt_value = pmt;
    if advance {
        fv -= pmt_value;
        pv += pmt_value;
    }
    let mut value = guess;
    if !value.is_finite() {
        return None;
    }
    if nper.fract() != 0.0 && value < -1.0 {
        value = -1.0;
    }
    for _ in 0..150 {
        if value <= -1.0 || !value.is_finite() {
            return None;
        }
        let factor = (1.0 + value).powf(nper);
        let (series, derivative_series) = if value == 0.0 {
            (nper, nper * (nper - 1.0) / 2.0)
        } else {
            let series = (factor - 1.0) / value;
            (series, nper * (1.0 + value).powf(nper - 1.0) / value - series / value)
        };
        let term = fv + pv * factor + pmt_value * series;
        let derivative = pv * nper * (1.0 + value).powf(nper - 1.0) + pmt_value * derivative_series;
        if !term.is_finite() || !derivative.is_finite() {
            return None;
        }
        if term.abs() < 1e-14 {
            return Some(value);
        }
        let next = if derivative == 0.0 { value + 1e-14 } else { value - term / derivative };
        if !next.is_finite() {
            return None;
        }
        if (next - value).abs() < 1e-13 {
            return if next > -1.0 { Some(next) } else { None };
        }
        value = next;
    }
    if value > -1.0 && value.is_finite() { Some(value) } else { None }
}

fn pduration(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let rate = a.number(0)?;
    let pv = a.number(1)?;
    let fv = a.number(2)?;
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Err(invalid(name, "rate, pv and fv must be positive"));
    }
    decimal(name, (fv / pv).ln() / rate.ln_1p())
}

fn rri(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let nper = a.number(0)?;
    let pv = a.number(1)?;
    let fv = a.number(2)?;
    if nper <= 0.0 || pv == 0.0 {
        return Err(invalid(name, "nper must be positive and pv must not be zero"));
    }
    let ratio = fv / pv;
    if ratio < 0.0 {
        return Err(invalid(name, "has no real solution"));
    }
    decimal(name, ratio.powf(1.0 / nper) - 1.0)
}

fn effect(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let nominal = a.number(0)?;
    let periods = a.number(1)?.floor();
    if periods < 1.0 || nominal < 0.0 {
        return Err(invalid(name, "nominal_rate must be non-negative and npery at least 1"));
    }
    if nominal == 0.0 {
        return Ok(Scalar::Decimal(0.0));
    }
    decimal(name, (1.0 + nominal / periods).powf(periods) - 1.0)
}

fn nominal(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let effective = a.number(0)?;
    let periods = a.number(1)?.floor();
    if periods < 1.0 || effective <= 0.0 {
        return Err(invalid(name, "effect_rate must be positive and npery at least 1"));
    }
    decimal(name, ((effective + 1.0).powf(1.0 / periods) - 1.0) * periods)
}

fn sln(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let salvage = a.number(1)?;
    let life = a.number(2)?;
    if life == 0.0 {
        return Err(division_by_zero(name));
    }
    decimal(name, (cost - salvage) / life)
}

fn syd(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let salvage = a.number(1)?;
    let life = a.number(2)?;
    let per = a.number(3)?;
    if life <= 0.0 || per < 1.0 || per > life {
        return Err(invalid(name, "life must be positive and per must lie within 1..life"));
    }
    decimal(name, (cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

// ---------------------------------------------------------------------------
// Depreciation
// ---------------------------------------------------------------------------

fn ddb(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let salvage = a.number(1)?;
    let life = a.number(2)?;
    let period = a.number(3)?;
    let factor = a.optional_number(4, 2.0)?;
    if cost < 0.0 || salvage < 0.0 || factor <= 0.0 || salvage > cost || life <= 0.0 || period < 1.0 || period > life {
        return Err(invalid(name, "arguments are out of range"));
    }
    decimal(name, declining_balance(cost, salvage, life, period, factor))
}

fn declining_balance(cost: f64, salvage: f64, life: f64, period: f64, factor: f64) -> f64 {
    let mut rate = factor / life;
    let old_value;
    if rate >= 1.0 {
        rate = 1.0;
        old_value = if period == 1.0 { cost } else { 0.0 };
    } else {
        old_value = cost * (1.0 - rate).powf(period - 1.0);
    }
    let new_value = cost * (1.0 - rate).powf(period);
    let depreciation = if new_value < salvage { old_value - salvage } else { old_value - new_value };
    depreciation.max(0.0)
}

fn db(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let salvage = a.number(1)?;
    let life = a.number(2)?;
    let period = a.number(3)?;
    let months = a.optional_number(4, 12.0)?.floor();
    if !(1.0..=12.0).contains(&months)
        || life > 1200.0
        || life <= 0.0
        || salvage < 0.0
        || salvage > cost
        || cost <= 0.0
        || period <= 0.0
        || period > life + 1.0
    {
        return Err(invalid(name, "arguments are out of range"));
    }
    decimal(name, fixed_declining(cost, salvage, life, period, months))
}

fn fixed_declining(cost: f64, salvage: f64, life: f64, period: f64, months: f64) -> f64 {
    let mut rate = 1.0 - (salvage / cost).powf(1.0 / life);
    rate = (rate * 1000.0 + 0.5).floor() / 1000.0;
    let first = cost * rate * months / 12.0;
    if period.floor() == 1.0 {
        return first;
    }
    let mut accumulated = first;
    let limit = if life > period { period } else { life }.floor();
    let mut depreciation = first;
    let mut i = 2.0;
    while i <= limit {
        depreciation = (cost - accumulated) * rate;
        accumulated += depreciation;
        i += 1.0;
    }
    if period > life {
        depreciation = (cost - accumulated) * rate * (12.0 - months) / 12.0;
    }
    depreciation
}

fn vdb(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let salvage = a.number(1)?;
    let life = a.number(2)?;
    let start = a.number(3)?;
    let end = a.number(4)?;
    let factor = a.optional_number(5, 2.0)?;
    let no_switch = a.optional_bool(6, false)?;
    if start < 0.0
        || end < start
        || end > life
        || cost < 0.0
        || salvage < 0.0
        || salvage > cost
        || life <= 0.0
        || factor <= 0.0
        || end > MAX_DEPRECIATION_PERIODS
    {
        return Err(invalid(name, "arguments are out of range"));
    }
    decimal(name, variable_declining(cost, salvage, life, start, end, factor, no_switch))
}

fn variable_declining(cost: f64, salvage: f64, life: f64, start: f64, end: f64, factor: f64, no_switch: bool) -> f64 {
    let int_start = start.floor();
    let int_end = end.ceil();
    if no_switch {
        let mut total = 0.0;
        let loop_start = int_start as i64;
        let loop_end = int_end as i64;
        let mut i = loop_start + 1;
        while i <= loop_end {
            let mut term = declining_balance(cost, salvage, life, i as f64, factor);
            if i == loop_start + 1 {
                term *= end.min(int_start + 1.0) - start;
            } else if i == loop_end {
                term *= end + 1.0 - int_end;
            }
            total += term;
            i += 1;
        }
        return total;
    }
    let mut part = 0.0;
    if start > int_start {
        let remaining_cost = cost - inter_vdb(cost, salvage, life, life, int_start, factor);
        part += (start - int_start) * inter_vdb(remaining_cost, salvage, life, life - int_start, 1.0, factor);
    }
    if end < int_end {
        let last_start = int_end - 1.0;
        let remaining_cost = cost - inter_vdb(cost, salvage, life, life, last_start, factor);
        part += (int_end - end) * inter_vdb(remaining_cost, salvage, life, life - last_start, 1.0, factor);
    }
    let remaining_cost = cost - inter_vdb(cost, salvage, life, life, int_start, factor);
    inter_vdb(remaining_cost, salvage, life, life - int_start, int_end - int_start, factor) - part
}

fn inter_vdb(cost: f64, salvage: f64, life: f64, life1: f64, period: f64, factor: f64) -> f64 {
    let int_end = period.ceil();
    let loop_end = int_end as i64;
    let mut total = 0.0;
    let mut remaining = cost - salvage;
    let mut linear = false;
    let mut straight = 0.0;
    let mut i = 1;
    while i <= loop_end {
        let term;
        if !linear {
            let declining = declining_balance(cost, salvage, life, i as f64, factor);
            straight = remaining / (life1 - (i as f64 - 1.0));
            if straight > declining {
                term = straight;
                linear = true;
            } else {
                term = declining;
                remaining -= declining;
            }
        } else {
            term = straight;
        }
        let mut term = term;
        if i == loop_end {
            term *= period + 1.0 - int_end;
        }
        total += term;
        i += 1;
    }
    total
}

// ---------------------------------------------------------------------------
// French accounting depreciation
// ---------------------------------------------------------------------------

fn amordegrc(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let purchased = a.date(1)?;
    let first_period = a.date(2)?;
    let salvage = a.number(3)?;
    let period = a.number(4)? as i64;
    let rate = a.number(5)?;
    let basis = a.optional_number(6, 0.0)? as i64;
    validate_amor(name, cost, salvage, period, rate, (purchased, first_period), basis)?;
    if rate <= 0.0 {
        return Err(invalid(name, "rate must be positive"));
    }
    let life = 1.0 / rate;
    let coefficient = if life < 3.0 {
        return Err(invalid(name, "asset life (1/rate) is not supported"));
    } else if life <= 4.0 {
        1.5
    } else if life < 5.0 {
        return Err(invalid(name, "asset life (1/rate) is not supported"));
    } else if life <= 6.0 {
        2.0
    } else {
        2.5
    };
    let rate = rate * coefficient;
    let mut cost = cost;
    let first = (year_fraction(purchased, first_period, basis)? * rate * cost).round();
    cost -= first;
    let mut remaining = cost - salvage;
    let mut depreciation = first;
    let mut step = 0;
    while step < period {
        depreciation = (rate * cost).round();
        if depreciation == 0.0 {
            return decimal(name, 0.0);
        }
        remaining -= depreciation;
        if remaining < 0.0 {
            let left = period - step;
            let result = if left <= 1 { (cost / 2.0).round() } else { 0.0 };
            return decimal(name, result);
        }
        cost -= depreciation;
        step += 1;
    }
    decimal(name, depreciation)
}

fn amorlinc(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let cost = a.number(0)?;
    let purchased = a.date(1)?;
    let first_period = a.date(2)?;
    let salvage = a.number(3)?;
    let period = a.number(4)? as i64;
    let rate = a.number(5)?;
    let basis = a.optional_number(6, 0.0)? as i64;
    validate_amor(name, cost, salvage, period, rate, (purchased, first_period), basis)?;
    let one_rate = cost * rate;
    let cost_delta = cost - salvage;
    let first_rate = year_fraction(purchased, first_period, basis)? * rate * cost;
    if one_rate == 0.0 {
        return decimal(name, if period == 0 { first_rate } else { 0.0 });
    }
    let full_periods = ((cost_delta - first_rate) / one_rate) as i64;
    let result = if period == 0 {
        first_rate
    } else if period <= full_periods {
        one_rate
    } else if period == full_periods + 1 {
        cost_delta - one_rate * full_periods as f64 - first_rate
    } else {
        0.0
    };
    decimal(name, result)
}

fn validate_amor(name: &str, cost: f64, salvage: f64, period: i64, rate: f64, dates: (f64, f64), basis: i64) -> Result<(), EvalError> {
    let (purchased, first_period) = dates;
    if !matches!(basis, 0 | 1 | 3 | 4) {
        return Err(invalid(name, "basis must be 0, 1, 3 or 4"));
    }
    if cost < 0.0 || salvage < 0.0 || period < 0 || rate < 0.0 || !purchased.is_finite() || !first_period.is_finite() {
        return Err(invalid(name, "arguments are out of range"));
    }
    if purchased > first_period {
        return Err(invalid(name, "date_purchased must be earlier than first_period"));
    }
    Ok(())
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Excel `YEARFRAC` restricted to the bases AMORDEGRC/AMORLINC accept (0, 1, 3, 4).
fn year_fraction(start: f64, end: f64, basis: i64) -> Result<f64, EvalError> {
    let start = start.floor();
    let end = end.floor();
    match basis {
        0 => Ok(thirty_360(start, end)),
        1 => Ok(actual_actual(start, end)),
        3 => Ok((end - start) / 365.0),
        4 => Ok(european_360(start, end)),
        _ => Err(EvalError::new("basis must be 0, 1, 3 or 4")),
    }
}

fn thirty_360(start: f64, end: f64) -> f64 {
    let (y1, m1, d1) = date_parts(start);
    let (y2, m2, d2) = date_parts(end);
    // US (NASD) 30/360: the start day is capped at 30, but an end day of 31 is only pulled back
    // to 30 when the (adjusted) start day is 30 or 31. Excel therefore counts 2008-08-19 to
    // 2008-12-31 as 132 days, not 131.
    let d1 = if m1 == 2 && d1 >= 28 { 30 } else { d1.min(30) };
    let d2 = if (m2 == 2 && d2 >= 28) || (d2 == 31 && d1 >= 30) { 30 } else { d2 };
    (360.0 * (y2 - y1) as f64 + 30.0 * (m2 - m1) as f64 + (d2 - d1) as f64) / 360.0
}

fn european_360(start: f64, end: f64) -> f64 {
    let (y1, m1, d1) = date_parts(start);
    let (y2, m2, d2) = date_parts(end);
    let d1 = if m1 == 2 && d1 >= 28 { 30 } else { d1.min(30) };
    let d2 = if m2 == 2 && d2 >= 28 { 30 } else { d2.min(30) };
    (360.0 * (y2 - y1) as f64 + 30.0 * (m2 - m1) as f64 + (d2 - d1) as f64) / 360.0
}

fn actual_actual(start: f64, end: f64) -> f64 {
    if start == end {
        return 0.0;
    }
    if start > end {
        return -actual_actual(end, start);
    }
    let (y1, m1, d1) = date_parts(start);
    let (y2, _, _) = date_parts(end);
    let day_count = end - start;
    let plus_one = date_serial(y1 + 1, m1, d1.min(days_in_month(y1 + 1, m1)), 0, 0, 0);
    if end > plus_one {
        // More than one year: actual days divided by the average year length.
        let spanned_years = (y2 - y1 + 1) as f64;
        let first_of_start = date_serial(y1, 1, 1, 0, 0, 0);
        let first_of_after = date_serial(y2 + 1, 1, 1, 0, 0, 0);
        let average = (first_of_after - first_of_start) / spanned_years;
        return day_count / average;
    }
    let leap_start = is_leap_year(y1);
    let leap_end = is_leap_year(y2);
    let denominator = if leap_start && leap_end {
        366.0
    } else if leap_start || leap_end {
        let feb29 = if leap_start { date_serial(y1, 2, 29, 0, 0, 0) } else { date_serial(y2, 2, 29, 0, 0, 0) };
        if start <= feb29 && end >= feb29 { 366.0 } else { 365.0 }
    } else {
        365.0
    };
    day_count / denominator
}

// ---------------------------------------------------------------------------
// Fractional dollar prices
// ---------------------------------------------------------------------------

fn fractional_digits(fraction: i64) -> f64 {
    10f64.powi((fraction as f64).log10().ceil() as i32)
}

fn dollarde(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let value = a.number(0)?;
    let fraction = a.number(1)?;
    if fraction < 0.0 {
        return Err(invalid(name, "fraction must not be negative"));
    }
    let fraction = fraction.trunc() as i64;
    if fraction < 1 {
        return Err(division_by_zero(name));
    }
    let integer = value.trunc();
    let part = value - integer;
    decimal(name, integer + part * fractional_digits(fraction) / fraction as f64)
}

fn dollarfr(name: &str, values: &[Scalar]) -> Result<Scalar, EvalError> {
    let a = args(name, values);
    let value = a.number(0)?;
    let fraction = a.number(1)?;
    if fraction < 0.0 {
        return Err(invalid(name, "fraction must not be negative"));
    }
    let fraction = fraction.trunc() as i64;
    if fraction < 1 {
        return Err(division_by_zero(name));
    }
    let integer = value.trunc();
    let part = value - integer;
    decimal(name, integer + part * fraction as f64 / fractional_digits(fraction))
}

// ---------------------------------------------------------------------------
// Irregular cash flows
// ---------------------------------------------------------------------------

struct CashFlow {
    value: f64,
    date: f64,
    offset: f64,
}

fn collect_cash_flows(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Vec<CashFlow>, EvalError> {
    let table = eval_table(ev, args.first().ok_or_else(|| invalid(name, "needs a table"))?, ctx)?;
    let values = args.get(1).ok_or_else(|| invalid(name, "needs a values expression"))?;
    let dates = args.get(2).ok_or_else(|| invalid(name, "needs a dates expression"))?;
    let mut flows = Vec::with_capacity(table.rows.len().min(4096));
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let value = ev.eval(values, ctx);
        let date = ev.eval(dates, ctx);
        ctx.rows.pop();
        let value = value?;
        let date = date?;
        if value.is_blank() && date.is_blank() {
            continue;
        }
        if value.is_blank() || date.is_blank() {
            return Err(invalid(name, "cash flow rows must have both a value and a date"));
        }
        let value = value.to_number().ok_or_else(|| invalid(name, "cash flow values must be numeric"))?;
        let date = date_value(&date).ok_or_else(|| invalid(name, "cash flow dates must be valid"))?;
        if !value.is_finite() || !date.is_finite() || date < 0.0 {
            return Err(invalid(name, "cash flow values and dates are out of range"));
        }
        flows.push(CashFlow { value, date, offset: 0.0 });
    }
    if flows.is_empty() {
        return Err(invalid(name, "needs at least one cash flow"));
    }
    let anchor = flows[0].date;
    for flow in &mut flows {
        if flow.date < anchor {
            return Err(invalid(name, "all dates must be on or after the first date"));
        }
        flow.offset = (flow.date - anchor) / 365.0;
    }
    Ok(flows)
}

fn xnpv(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let rate = match args.get(3) {
        Some(expr) => ev.eval(expr, ctx)?,
        None => return Err(invalid(name, "needs a rate")),
    };
    let rate = rate.to_number().ok_or_else(|| invalid(name, "rate must be numeric"))?;
    check_rate(name, rate)?;
    let flows = collect_cash_flows(ev, name, args, ctx)?;
    let mut total = 0.0;
    for flow in &flows {
        let factor = growth(rate, flow.offset);
        if factor == 0.0 || !factor.is_finite() {
            return Err(invalid(name, "rate produces an invalid discount factor"));
        }
        total += flow.value / factor;
    }
    decimal(name, total)
}

fn xirr(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let guess = match args.get(3) {
        Some(expr) => {
            let value = ev.eval(expr, ctx)?;
            if value.is_blank() { 0.1 } else { value.to_number().ok_or_else(|| invalid(name, "guess must be numeric"))? }
        }
        None => 0.1,
    };
    let flows = collect_cash_flows(ev, name, args, ctx)?;
    let has_positive = flows.iter().any(|flow| flow.value > 0.0);
    let has_negative = flows.iter().any(|flow| flow.value < 0.0);
    let solved = if has_positive && has_negative && guess > -1.0 && guess.is_finite() { solve_xirr(&flows, guess) } else { None };
    match solved {
        Some(rate) => decimal(name, rate),
        None => {
            if let Some(alternate) = args.get(4) {
                return ev.eval(alternate, ctx);
            }
            Err(invalid(name, "did not converge; cash flows need both positive and negative values"))
        }
    }
}

fn xirr_value(flows: &[CashFlow], rate: f64) -> (f64, f64) {
    let mut value = 0.0;
    let mut derivative = 0.0;
    for flow in flows {
        let exponent = flow.offset;
        let factor = (exponent * rate.ln_1p()).exp();
        if factor == 0.0 || !factor.is_finite() {
            return (f64::NAN, f64::NAN);
        }
        value += flow.value / factor;
        derivative -= exponent * flow.value / (factor * (1.0 + rate));
    }
    (value, derivative)
}

fn solve_xirr(flows: &[CashFlow], guess: f64) -> Option<f64> {
    let scale = flows.iter().map(|flow| flow.value.abs()).sum::<f64>().max(1.0);
    let tolerance = 1e-12 * scale;
    let mut value = guess;
    for _ in 0..100 {
        if value <= -1.0 || !value.is_finite() {
            break;
        }
        let (f, df) = xirr_value(flows, value);
        if !f.is_finite() || !df.is_finite() {
            break;
        }
        if f.abs() < tolerance {
            return Some(value);
        }
        if df == 0.0 {
            break;
        }
        let next = value - f / df;
        if !next.is_finite() {
            break;
        }
        if (next - value).abs() < 1e-14 {
            let (f, _) = xirr_value(flows, next);
            return if f.abs() < 1e-6 * scale { Some(next) } else { None };
        }
        value = next;
    }
    // Newton can wander out of the domain or stall; bracket the root and bisect.
    let mut lower = -1.0 + 1e-9;
    let mut lower_value = xirr_value(flows, lower).0;
    if !lower_value.is_finite() {
        return None;
    }
    let mut step = 1e-6;
    let mut upper = lower;
    let mut found = false;
    while upper < 1e9 {
        upper = (lower + step).min(1e9);
        let point_value = xirr_value(flows, upper).0;
        if point_value.is_finite() && point_value.signum() != lower_value.signum() {
            found = true;
            break;
        }
        lower = upper;
        lower_value = point_value;
        step *= 10.0;
    }
    if !found {
        return None;
    }
    for _ in 0..200 {
        let middle = (lower + upper) / 2.0;
        let middle_value = xirr_value(flows, middle).0;
        if !middle_value.is_finite() {
            return None;
        }
        if middle_value.abs() < tolerance {
            return Some(middle);
        }
        if middle_value.signum() == lower_value.signum() {
            lower = middle;
            lower_value = middle_value;
        } else {
            upper = middle;
        }
    }
    Some((lower + upper) / 2.0)
}
