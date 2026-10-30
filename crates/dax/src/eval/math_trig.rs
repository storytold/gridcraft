//! Math and trigonometry functions.
//!
//! The inverse trigonometric and hyperbolic families follow Excel's domains: a value outside the
//! documented range raises an error instead of returning NaN. `CONVERT` follows the catalog and
//! dax.guide, i.e. `CONVERT ( <Expression>, <DataType> )` with a bare type keyword.
//!
//! `RAND`/`RANDBETWEEN` use a local xorshift64* generator seeded from the clock and process id:
//! the crate takes no dependencies and the catalog already flags both functions as volatile.

use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

use super::support::{args as arg_values, eval_all, parse_date_text, round_direction};
use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let name = name.to_ascii_uppercase();
    match name.as_str() {
        "ACOS" | "ACOSH" | "ACOT" | "ACOTH" | "ASIN" | "ASINH" | "ATAN" | "ATANH" | "CONVERT" | "COSH" | "COT" | "COTH" | "CURRENCY" | "EVEN"
        | "MOD" | "MROUND" | "ODD" | "QUOTIENT" | "RAND" | "RANDBETWEEN" | "ROUNDDOWN" | "SINH" | "SQRTPI" | "TANH" => {}
        _ => return None,
    }
    Some(evaluate(&name, ev, args, ctx))
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

fn evaluate(name: &str, ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = arg_values(name, &values);
    match name {
        "ACOS" => {
            let x = domain_number(&a, -1.0, 1.0)?;
            Ok(Scalar::Decimal(x.acos()))
        }
        "ACOSH" => {
            let x = domain_number(&a, 1.0, f64::INFINITY)?;
            Ok(Scalar::Decimal(x.acosh()))
        }
        "ACOT" => {
            // Principal value in (0, pi), unlike ATAN's (-pi/2, pi/2).
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(std::f64::consts::FRAC_PI_2 - x.atan()))
        }
        "ACOTH" => {
            let x = finite(a.number(0)?, name)?;
            if x.abs() <= 1.0 {
                return Err(EvalError::new("ACOTH argument must have an absolute value greater than 1"));
            }
            Ok(Scalar::Decimal(finite(0.5 * ((x + 1.0) / (x - 1.0)).ln(), name)?))
        }
        "ASIN" => {
            let x = domain_number(&a, -1.0, 1.0)?;
            Ok(Scalar::Decimal(x.asin()))
        }
        "ASINH" => {
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(x.asinh()))
        }
        "ATAN" => {
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(x.atan()))
        }
        "ATANH" => {
            let x = finite(a.number(0)?, name)?;
            if x <= -1.0 || x >= 1.0 {
                return Err(EvalError::new("ATANH argument must be greater than -1 and less than 1"));
            }
            Ok(Scalar::Decimal(x.atanh()))
        }
        "CONVERT" => convert(&values),
        "COSH" => {
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(finite(x.cosh(), name)?))
        }
        "COT" => {
            let x = finite(a.number(0)?, name)?;
            let tangent = x.tan();
            if tangent == 0.0 {
                return Err(EvalError::new("COT is undefined at multiples of pi"));
            }
            Ok(Scalar::Decimal(finite(1.0 / tangent, name)?))
        }
        "COTH" => {
            let x = finite(a.number(0)?, name)?;
            let tangent = x.tanh();
            if tangent == 0.0 {
                return Err(EvalError::new("COTH is undefined at zero"));
            }
            Ok(Scalar::Decimal(finite(1.0 / tangent, name)?))
        }
        "CURRENCY" => currency(a.arg(0)?),
        "EVEN" => Ok(Scalar::Integer(round_away_to_even(finite(a.number(0)?, name)?)?)),
        "MROUND" => {
            let number = finite(a.number(0)?, name)?;
            let multiple = finite(a.number(1)?, name)?;
            Ok(Scalar::Decimal(mround(number, multiple)?))
        }
        "MOD" => modulo(a.arg(0)?, a.arg(1)?),
        // ROUNDDOWN rounds toward zero (support::round_direction), so negatives go up.
        "ROUNDDOWN" => Ok(Scalar::Decimal(round_direction(finite(a.number(0)?, name)?, a.number(1)? as i32, false))),
        "ODD" => Ok(Scalar::Integer(round_away_to_odd(finite(a.number(0)?, name)?)?)),
        "QUOTIENT" => {
            let numerator = finite(a.number(0)?, name)?;
            let denominator = finite(a.number(1)?, name)?;
            if denominator == 0.0 {
                return Err(EvalError::new("QUOTIENT denominator must not be zero"));
            }
            Ok(Scalar::Integer(truncate_i64(finite(numerator / denominator, name)?)?))
        }
        "RAND" => Ok(Scalar::Decimal(next_random_fraction())),
        "RANDBETWEEN" => {
            let bottom = truncate_i64(finite(a.number(0)?, name)?)?;
            let top = truncate_i64(finite(a.number(1)?, name)?)?;
            if bottom > top {
                return Err(EvalError::new("RANDBETWEEN bottom must not be greater than top"));
            }
            let span = (top as i128) - (bottom as i128) + 1;
            let offset = (next_random_u64() as u128 % span as u128) as i128;
            Ok(Scalar::Integer((bottom as i128 + offset) as i64))
        }
        "SINH" => {
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(finite(x.sinh(), name)?))
        }
        "SQRTPI" => {
            let x = finite(a.number(0)?, name)?;
            if x < 0.0 {
                return Err(EvalError::new("SQRTPI argument must not be negative"));
            }
            Ok(Scalar::Decimal(finite((x * std::f64::consts::PI).sqrt(), name)?))
        }
        "TANH" => {
            let x = finite(a.number(0)?, name)?;
            Ok(Scalar::Decimal(x.tanh()))
        }
        _ => Err(EvalError::new(format!("{name} is not a math function"))),
    }
}

fn finite(value: f64, function: &str) -> Result<f64, EvalError> {
    if value.is_finite() { Ok(value) } else { Err(EvalError::new(format!("{function} result is not a finite number"))) }
}

/// Reads the first argument as a finite number inside `[low, high]`.
fn domain_number(a: &super::support::Args<'_>, low: f64, high: f64) -> Result<f64, EvalError> {
    let x = finite(a.number(0)?, a.name)?;
    if x < low || x > high {
        return Err(EvalError::new(format!("{} argument is outside the valid domain", a.name)));
    }
    Ok(x)
}

fn truncate_i64(value: f64) -> Result<i64, EvalError> {
    if !value.is_finite() || value.trunc() < -9_223_372_036_854_775_808.0 || value.trunc() >= 9_223_372_036_854_775_808.0 {
        return Err(EvalError::new("value is outside the 64-bit integer range"));
    }
    Ok(value.trunc() as i64)
}

/// CURRENCY rounds half away from zero to four decimals and stores the fixed-point integer.
fn currency(value: &Scalar) -> Result<Scalar, EvalError> {
    let number = value.to_number().ok_or_else(|| EvalError::new("CURRENCY argument must be convertible to a number"))?;
    let scaled = (number * 10_000.0).round();
    if !scaled.is_finite() || scaled.abs() > i64::MAX as f64 {
        return Err(EvalError::new("CURRENCY value is outside the representable range"));
    }
    Ok(Scalar::Currency(scaled as i64))
}

fn round_away_to_even(number: f64) -> Result<i64, EvalError> {
    let magnitude = magnitude_i64(number)?;
    let rounded = if magnitude % 2 == 0 {
        magnitude
    } else {
        magnitude.checked_add(1).ok_or_else(|| EvalError::new("EVEN result is outside the 64-bit integer range"))?
    };
    Ok(with_sign(rounded, number))
}

fn round_away_to_odd(number: f64) -> Result<i64, EvalError> {
    let magnitude = magnitude_i64(number)?;
    let rounded = if magnitude % 2 == 1 {
        magnitude
    } else {
        magnitude.checked_add(1).ok_or_else(|| EvalError::new("ODD result is outside the 64-bit integer range"))?
    };
    Ok(with_sign(rounded, number))
}

/// Absolute value rounded away from zero to an integer that still fits in an `i64`.
fn magnitude_i64(number: f64) -> Result<i64, EvalError> {
    let magnitude = number.abs().ceil();
    if !magnitude.is_finite() || magnitude >= 9_223_372_036_854_775_808.0 {
        return Err(EvalError::new("number is outside the 64-bit integer range"));
    }
    Ok(magnitude as i64)
}

fn with_sign(magnitude: i64, number: f64) -> i64 {
    if number < 0.0 { -magnitude } else { magnitude }
}

fn mround(number: f64, multiple: f64) -> Result<f64, EvalError> {
    if multiple == 0.0 {
        return Ok(0.0);
    }
    if (number > 0.0 && multiple < 0.0) || (number < 0.0 && multiple > 0.0) {
        return Err(EvalError::new("MROUND number and multiple must have the same sign"));
    }
    finite((number / multiple).round() * multiple, "MROUND")
}

/// `MOD ( <Number>, <Divisor> )`: the remainder whose sign follows the *divisor*
/// (`number - divisor * FLOOR ( number / divisor )`), unlike Rust's `%`, which follows the
/// dividend. A zero divisor is a division-by-zero error in DAX, not BLANK. Two integer operands
/// keep the Integer type the catalog documents; anything else returns a Decimal.
fn modulo(number: &Scalar, divisor: &Scalar) -> Result<Scalar, EvalError> {
    let divisor_number = finite(divisor.to_number().ok_or_else(|| EvalError::new("MOD argument 2 must be numeric"))?, "MOD")?;
    if divisor_number == 0.0 {
        return Err(EvalError::new("MOD divisor must not be zero"));
    }
    if let (Scalar::Integer(number), Scalar::Integer(divisor)) = (number, divisor) {
        let divisor = *divisor;
        let remainder = number.checked_rem(divisor).ok_or_else(|| EvalError::new("MOD result is outside the 64-bit integer range"))?;
        if remainder != 0 && (remainder < 0) != (divisor < 0) {
            return Ok(Scalar::Integer(
                remainder.checked_add(divisor).ok_or_else(|| EvalError::new("MOD result is outside the 64-bit integer range"))?,
            ));
        }
        return Ok(Scalar::Integer(remainder));
    }
    let number = finite(number.to_number().ok_or_else(|| EvalError::new("MOD argument 1 must be numeric"))?, "MOD")?;
    Ok(Scalar::Decimal(finite(number - divisor_number * (number / divisor_number).floor(), "MOD")?))
}

// ---------------------------------------------------------------------------------------------
// Random numbers
// ---------------------------------------------------------------------------------------------

/// Clock entropy for the PRNG seed. `SystemTime::now` traps on wasm, so it is only compiled for
/// native targets; wasm falls back to the counter that the caller mixes in.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)]
fn clock_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos() as u64).unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn clock_nanos() -> u64 {
    0
}

fn next_random_u64() -> u64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    static SALT: AtomicU64 = AtomicU64::new(0);
    let mut state = STATE.load(Ordering::Relaxed);
    if state == 0 {
        let salt = SALT.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        state = clock_nanos() ^ salt;
        if state == 0 {
            state = 0x9E37_79B9_7F4A_7C15;
        }
    }
    // xorshift64*: full-period, small and dependency-free.
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    STATE.store(state, Ordering::Relaxed);
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn next_random_fraction() -> f64 {
    // 53 random mantissa bits give a uniform value in [0, 1).
    (next_random_u64() >> 11) as f64 / 9_007_199_254_740_992.0
}

// ---------------------------------------------------------------------------------------------
// CONVERT
// ---------------------------------------------------------------------------------------------

fn convert(values: &[Scalar]) -> Result<Scalar, EvalError> {
    match values {
        [value, target] => convert_type(value, target),
        _ => Err(EvalError::new("CONVERT expects an expression and a data type")),
    }
}

fn convert_type(value: &Scalar, target: &Scalar) -> Result<Scalar, EvalError> {
    let target = target.to_text().trim().to_ascii_uppercase();
    match target.as_str() {
        "BOOLEAN" | "LOGICAL" => {
            let boolean = value.to_bool().ok_or_else(|| EvalError::new("CONVERT cannot convert the value to BOOLEAN"))?;
            Ok(Scalar::Boolean(boolean))
        }
        // "DECIMAL" is the DAX synonym for the fixed-decimal Currency type, not a double.
        "CURRENCY" | "DECIMAL" | "FIXEDDECIMAL" => currency(value),
        "DATETIME" => convert_datetime(value),
        "DOUBLE" => {
            let number = value.to_number().ok_or_else(|| EvalError::new("CONVERT cannot convert the value to DOUBLE"))?;
            Ok(Scalar::Decimal(finite(number, "CONVERT")?))
        }
        "INTEGER" | "INT64" => {
            let number = value.to_number().ok_or_else(|| EvalError::new("CONVERT cannot convert the value to INTEGER"))?;
            Ok(Scalar::Integer(truncate_i64(number)?))
        }
        "STRING" | "TEXT" => Ok(Scalar::String(value.to_text())),
        other => Err(EvalError::new(format!("CONVERT does not recognize the data type `{other}`"))),
    }
}

fn convert_datetime(value: &Scalar) -> Result<Scalar, EvalError> {
    match value {
        Scalar::DateTime(_) => Ok(value.clone()),
        Scalar::String(text) => {
            let serial = parse_date_text(text).ok_or_else(|| EvalError::new("CONVERT cannot parse the text as DATETIME"))?;
            Ok(Scalar::DateTime(serial))
        }
        other => {
            let number = other.to_number().ok_or_else(|| EvalError::new("CONVERT cannot convert the value to DATETIME"))?;
            Ok(Scalar::DateTime(finite(number, "CONVERT")?))
        }
    }
}
