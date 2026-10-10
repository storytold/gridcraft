//! Math & trigonometry functions, plus the criteria aggregates (SUMIF(S), COUNTIF(S),
//! AVERAGEIF(S), MAXIFS, MINIFS).

use std::f64::consts::PI;

use gridcraft_core::{Array, CellError, Value};

use crate::criteria::Criterion;
use crate::util::{
    A, MAX_CELLS, R, S, array_val, as_array, check_size, has, num, num_val, numbers, opt_num, round_half_away, text, text_val, to_sig_digits,
};
use crate::{Arg, Ctx, FnSpec, VAR};

// ---------------------------------------------------------------------------------------------
// Lifting tables

const fn alt_table(first_crit: usize) -> [bool; 256] {
    // Criteria positions: first_crit, first_crit + 2, …
    let mut t = [false; 256];
    let mut i = first_crit;
    while i < 256 {
        t[i] = true;
        i += 2;
    }
    t
}
/// COUNTIFS(range, crit, range, crit, …)
static COUNTIFS_SCAL: [bool; 256] = alt_table(1);
/// SUMIFS(sum_range, range, crit, …)
static SUMIFS_SCAL: [bool; 256] = alt_table(2);
const IF2: &[bool] = &[false, true, false];

// ---------------------------------------------------------------------------------------------
// Small helpers

const TRIG_LIMIT: f64 = 134_217_728.0; // 2^27: Excel's argument limit for trig functions

fn unary(a: &[Arg], f: impl Fn(f64) -> R<f64>) -> R<Value> {
    num_val(f(num(a, 0)?)?)
}

fn trig_arg(x: f64) -> R<f64> {
    if x.abs() >= TRIG_LIMIT { Err(CellError::Num) } else { Ok(x) }
}

fn nonzero(x: f64) -> R<f64> {
    if x == 0.0 { Err(CellError::Div0) } else { Ok(x) }
}

/// Removes binary noise before ceil/floor (e.g. 2.0000000000000004 → 2).
fn clean(x: f64) -> f64 {
    to_sig_digits(x, 15)
}

fn digits_arg(a: &[Arg], i: usize) -> R<i32> {
    let d = opt_num(a, i, 0.0)?.trunc();
    Ok(d.clamp(-400.0, 400.0) as i32)
}

/// Rounds |x| scaled by 10^digits with `op` (ceil / floor), keeping sign.
fn round_dir(x: f64, digits: i32, up: bool) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let ax = clean(x.abs());
    let (scaled, f, neg) = if digits >= 0 {
        let f = 10f64.powi(digits);
        (ax * f, f, false)
    } else {
        let f = 10f64.powi(-digits);
        (ax / f, f, true)
    };
    if !scaled.is_finite() || scaled >= 1e17 {
        return x;
    }
    let s = clean(scaled);
    let r = if up { s.ceil() } else { s.floor() };
    let out = if neg { r * f } else { clean(r / f) };
    if x < 0.0 { -out } else { out }
}

// ---------------------------------------------------------------------------------------------
// Sums and products

fn sum(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(numbers(a)?.iter().sum())
}

fn sumsq(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(numbers(a)?.iter().map(|x| x * x).sum())
}

fn product(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = numbers(a)?;
    if v.is_empty() {
        return Ok(Value::Number(0.0));
    }
    num_val(v.iter().product())
}

fn sumproduct(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let arrays: Vec<_> = a.iter().map(|x| as_array(&x.value)).collect();
    let Some(first) = arrays.first() else { return Err(CellError::Value) };
    let (rows, cols) = (first.rows, first.cols);
    if arrays.iter().any(|x| x.rows != rows || x.cols != cols) {
        return Err(CellError::Value);
    }
    let mut total = 0.0;
    for i in 0..rows.saturating_mul(cols) {
        let mut p = 1.0;
        for arr in &arrays {
            match arr.data.get(i) {
                Some(Value::Number(n)) => p *= n,
                Some(Value::Error(e)) => return Err(*e),
                _ => p = 0.0,
            }
        }
        total += p;
    }
    num_val(total)
}

fn pair_sum(a: &[Arg], f: fn(f64, f64) -> f64) -> R<Value> {
    let x = as_array(&crate::util::arg(a, 0)?.value);
    let y = as_array(&crate::util::arg(a, 1)?.value);
    if x.data.len() != y.data.len() {
        return Err(CellError::NA);
    }
    let mut total = 0.0;
    let mut any = false;
    for (u, v) in x.data.iter().zip(y.data.iter()) {
        if let Value::Error(e) = u {
            return Err(*e);
        }
        if let Value::Error(e) = v {
            return Err(*e);
        }
        if let (Value::Number(p), Value::Number(q)) = (u, v) {
            total += f(*p, *q);
            any = true;
        }
    }
    if !any {
        return Err(CellError::Div0);
    }
    num_val(total)
}

fn sumx2my2(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    pair_sum(a, |x, y| x * x - y * y)
}
fn sumx2py2(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    pair_sum(a, |x, y| x * x + y * y)
}
fn sumxmy2(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    pair_sum(a, |x, y| (x - y) * (x - y))
}

// ---------------------------------------------------------------------------------------------
// Rounding

fn round(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(round_half_away(num(a, 0)?, digits_arg(a, 1)?))
}
fn roundup(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(round_dir(num(a, 0)?, digits_arg(a, 1)?, true))
}
fn rounddown(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(round_dir(num(a, 0)?, digits_arg(a, 1)?, false))
}
fn trunc(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(round_dir(num(a, 0)?, digits_arg(a, 1)?, false))
}
fn int_fn(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.floor()))
}

fn mround(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, m) = (num(a, 0)?, num(a, 1)?);
    if m == 0.0 || x == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if (x > 0.0) != (m > 0.0) {
        return Err(CellError::Num);
    }
    let q = round_half_away(x / m, 0);
    num_val(clean(q * m))
}

fn ceiling(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = num(a, 1)?;
    if x == 0.0 || s == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if x > 0.0 && s < 0.0 {
        return Err(CellError::Num);
    }
    num_val(clean(clean(x / s).ceil() * s))
}

fn floor(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = num(a, 1)?;
    if s == 0.0 {
        return if x == 0.0 { Ok(Value::Number(0.0)) } else { Err(CellError::Div0) };
    }
    if x == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if x > 0.0 && s < 0.0 {
        return Err(CellError::Num);
    }
    num_val(clean(clean(x / s).floor() * s))
}

/// CEILING.MATH / FLOOR.MATH / *.PRECISE: `up` picks the direction for positive numbers;
/// negative numbers round away from zero when `away`, toward zero otherwise.
fn ceil_floor_math(x: f64, s: f64, up: bool, away: bool) -> R<Value> {
    let s = s.abs();
    if s == 0.0 || x == 0.0 {
        return Ok(Value::Number(0.0));
    }
    let q = clean(x / s);
    let r = if x >= 0.0 {
        if up { q.ceil() } else { q.floor() }
    } else if away {
        q.floor()
    } else {
        q.ceil()
    };
    num_val(clean(r * s))
}

fn ceiling_math(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = if has(a, 1) { num(a, 1)? } else { 1.0 };
    let mode = opt_num(a, 2, 0.0)?;
    ceil_floor_math(x, s, true, mode != 0.0)
}
fn floor_math(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = if has(a, 1) { num(a, 1)? } else { 1.0 };
    let mode = opt_num(a, 2, 0.0)?;
    // Negative numbers: mode 0 rounds away from zero (down), non-zero rounds toward zero.
    ceil_floor_math(x, s, false, mode == 0.0)
}
fn ceiling_precise(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = if has(a, 1) { num(a, 1)? } else { 1.0 };
    ceil_floor_math(x, s, true, false)
}
fn floor_precise(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let s = if has(a, 1) { num(a, 1)? } else { 1.0 };
    ceil_floor_math(x, s, false, true)
}

fn even_odd(x: f64, odd: bool) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = clean(x.abs());
    let mut v = ax.ceil();
    if odd {
        if v % 2.0 == 0.0 {
            v += 1.0;
        }
    } else if v % 2.0 != 0.0 {
        v += 1.0;
    }
    sign * v
}
fn even(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(even_odd(x, false)))
}
fn odd(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(even_odd(x, true)))
}

// ---------------------------------------------------------------------------------------------
// Arithmetic

fn modulo(x: f64, d: f64) -> R<f64> {
    if d == 0.0 {
        return Err(CellError::Div0);
    }
    let q = (x / d).floor();
    let r = x - d * q;
    // Keep the sign of the divisor and avoid returning |d| from rounding.
    if r != 0.0 && (r < 0.0) != (d < 0.0) {
        return Ok(r + d);
    }
    if r.abs() >= d.abs() {
        return Ok(0.0);
    }
    Ok(clean(r))
}
fn mod_fn(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(modulo(num(a, 0)?, num(a, 1)?)?)
}
fn quotient(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, d) = (num(a, 0)?, num(a, 1)?);
    num_val(clean(x / nonzero(d)?).trunc())
}
fn abs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.abs()))
}
fn sign(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| {
        Ok(if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        })
    })
}
fn sqrt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x < 0.0 { Err(CellError::Num) } else { Ok(x.sqrt()) })
}
fn sqrtpi(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x < 0.0 { Err(CellError::Num) } else { Ok((x * PI).sqrt()) })
}

fn power_of(x: f64, y: f64) -> R<f64> {
    if x == 0.0 {
        if y == 0.0 {
            return Err(CellError::Num);
        }
        if y < 0.0 {
            return Err(CellError::Div0);
        }
        return Ok(0.0);
    }
    if x < 0.0 && y.fract() != 0.0 {
        return Err(CellError::Num);
    }
    let r = x.powf(y);
    if r.is_finite() { Ok(r) } else { Err(CellError::Num) }
}
fn power(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(power_of(num(a, 0)?, num(a, 1)?)?)
}
fn exp(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.exp()))
}
fn ln(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x <= 0.0 { Err(CellError::Num) } else { Ok(x.ln()) })
}
fn log10(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x <= 0.0 { Err(CellError::Num) } else { Ok(x.log10()) })
}
fn log(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let base = opt_num(a, 1, 10.0)?;
    if x <= 0.0 || base <= 0.0 {
        return Err(CellError::Num);
    }
    if base == 1.0 {
        return Err(CellError::Div0);
    }
    let r = if base == 10.0 {
        x.log10()
    } else if base == 2.0 {
        x.log2()
    } else {
        x.ln() / base.ln()
    };
    num_val(r)
}
fn pi(_a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(PI)
}
fn rand(_a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(c.random())
}
fn randbetween(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let lo = num(a, 0)?.ceil();
    let hi = num(a, 1)?.floor();
    if lo > hi {
        return Err(CellError::Num);
    }
    let r = c.random().clamp(0.0, 1.0);
    let v = (lo + (r * (hi - lo + 1.0)).floor()).min(hi);
    num_val(v)
}

// ---------------------------------------------------------------------------------------------
// Combinatorics

fn fact_of(n: f64) -> R<f64> {
    let n = n.trunc();
    if n < 0.0 {
        return Err(CellError::Num);
    }
    if n > 170.0 {
        return Err(CellError::Num);
    }
    let mut f = 1.0f64;
    let mut i = 2.0;
    while i <= n {
        f *= i;
        i += 1.0;
    }
    Ok(f)
}
fn fact(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(fact_of(num(a, 0)?)?)
}
fn factdouble(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    if n < -1.0 {
        return Err(CellError::Num);
    }
    if n > 300.0 {
        return Err(CellError::Num);
    }
    let mut f = 1.0f64;
    let mut i = n;
    while i > 1.0 {
        f *= i;
        i -= 2.0;
    }
    num_val(f)
}

fn combin_of(n: f64, k: f64) -> R<f64> {
    let (n, k) = (n.trunc(), k.trunc());
    if n < 0.0 || k < 0.0 || k > n {
        return Err(CellError::Num);
    }
    let k = k.min(n - k);
    if k > 1e7 {
        let r = (crate::special::ln_gamma(n + 1.0) - crate::special::ln_gamma(k + 1.0) - crate::special::ln_gamma(n - k + 1.0)).exp();
        return if r.is_finite() { Ok(r) } else { Err(CellError::Num) };
    }
    let mut r = 1.0f64;
    let mut i = 1.0;
    while i <= k {
        r = r * (n - k + i) / i;
        if !r.is_finite() {
            return Err(CellError::Num);
        }
        i += 1.0;
    }
    Ok(if r < 9e15 { r.round() } else { r })
}
fn combin(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(combin_of(num(a, 0)?, num(a, 1)?)?)
}
fn combina(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, k) = (num(a, 0)?.trunc(), num(a, 1)?.trunc());
    if n < 0.0 || k < 0.0 || n < k && n == 0.0 && k > 0.0 {
        return Err(CellError::Num);
    }
    if k == 0.0 {
        return Ok(Value::Number(1.0));
    }
    num_val(combin_of(n + k - 1.0, k)?)
}
fn permut(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, k) = (num(a, 0)?.trunc(), num(a, 1)?.trunc());
    if n < 0.0 || k < 0.0 || k > n {
        return Err(CellError::Num);
    }
    let mut r = 1.0f64;
    let mut i = 0.0;
    while i < k {
        r *= n - i;
        if !r.is_finite() {
            return Err(CellError::Num);
        }
        i += 1.0;
    }
    num_val(r)
}
fn permutationa(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, k) = (num(a, 0)?.trunc(), num(a, 1)?.trunc());
    if n < 0.0 || k < 0.0 {
        return Err(CellError::Num);
    }
    num_val(n.powf(k))
}

fn integer_args(a: &[Arg]) -> R<Vec<u64>> {
    let v = numbers(a)?;
    v.iter()
        .map(|x| {
            let t = x.trunc();
            if !(0.0..9.007_199_254_740_992e15).contains(&t) { Err(CellError::Num) } else { Ok(t as u64) }
        })
        .collect()
}
fn gcd2(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}
fn gcd(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = integer_args(a)?;
    num_val(v.iter().fold(0u64, |acc, &x| gcd2(acc, x)) as f64)
}
fn lcm(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = integer_args(a)?;
    let mut acc = 1f64;
    for &x in &v {
        if x == 0 {
            return Ok(Value::Number(0.0));
        }
        let accu = acc as u64;
        let g = gcd2(accu, x);
        acc = acc / g as f64 * x as f64;
        if acc >= 9.007_199_254_740_992e15 {
            return Err(CellError::Num);
        }
    }
    num_val(acc)
}
fn multinomial(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = numbers(a)?;
    let mut total = 0.0;
    let mut r = 1.0f64;
    for x in v {
        let k = x.trunc();
        if k < 0.0 {
            return Err(CellError::Num);
        }
        total += k;
        r *= combin_of(total, k)?;
        if !r.is_finite() {
            return Err(CellError::Num);
        }
    }
    num_val(r)
}

// ---------------------------------------------------------------------------------------------
// Trigonometry

fn sin(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(trig_arg(x)?.sin()))
}
fn cos(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(trig_arg(x)?.cos()))
}
fn tan(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(trig_arg(x)?.tan()))
}
fn asin(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x.abs() > 1.0 { Err(CellError::Num) } else { Ok(x.asin()) })
}
fn acos(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x.abs() > 1.0 { Err(CellError::Num) } else { Ok(x.acos()) })
}
fn atan(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.atan()))
}
fn atan2(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, y) = (num(a, 0)?, num(a, 1)?);
    if x == 0.0 && y == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(y.atan2(x))
}
fn sinh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.sinh()))
}
fn cosh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.cosh()))
}
fn tanh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.tanh()))
}
fn asinh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x.asinh()))
}
fn acosh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x < 1.0 { Err(CellError::Num) } else { Ok(x.acosh()) })
}
fn atanh(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x.abs() >= 1.0 { Err(CellError::Num) } else { Ok(x.atanh()) })
}
fn cot(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / nonzero(trig_arg(x)?)?.tan()))
}
fn coth(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / nonzero(trig_arg(x)?)?.tanh()))
}
fn csc(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / nonzero(trig_arg(x)?)?.sin()))
}
fn csch(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / nonzero(trig_arg(x)?)?.sinh()))
}
fn sec(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / trig_arg(x)?.cos()))
}
fn sech(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(1.0 / trig_arg(x)?.cosh()))
}
fn acot(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(PI / 2.0 - x.atan()))
}
fn acoth(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| if x.abs() <= 1.0 { Err(CellError::Num) } else { Ok(0.5 * ((x + 1.0) / (x - 1.0)).ln()) })
}
fn degrees(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x * 180.0 / PI))
}
fn radians(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    unary(a, |x| Ok(x * PI / 180.0))
}

// ---------------------------------------------------------------------------------------------
// Number systems

const NUMERALS: [(char, u32); 7] = [('I', 1), ('V', 5), ('X', 10), ('L', 50), ('C', 100), ('D', 500), ('M', 1000)];

/// Tokens (text, value) for a ROMAN form, largest value first. Form 0 is classic; higher forms
/// allow subtracting progressively smaller numerals (max ratio 10, 20, 50, 100, 1000).
fn roman_tokens(form: u32) -> Vec<(String, u32)> {
    let max_ratio = [10, 20, 50, 100, 1000].get(form as usize).copied().unwrap_or(10);
    let mut toks: Vec<(String, u32)> = NUMERALS.iter().map(|(c, v)| (c.to_string(), *v)).collect();
    for (i, (sc, sv)) in NUMERALS.iter().enumerate() {
        for (lc, lv) in NUMERALS.iter().skip(i + 1) {
            let ratio = lv / sv;
            let pow10 = matches!(sc, 'I' | 'X' | 'C');
            let ok = if form == 0 { pow10 && ratio <= 10 } else { ratio <= max_ratio };
            let value = lv - sv;
            // Skip pairs that equal a single numeral (VX = V) – they are never shorter.
            if ok && !NUMERALS.iter().any(|(_, v)| *v == value) {
                toks.push((format!("{sc}{lc}"), value));
            }
        }
    }
    // Largest value first; on ties prefer the shorter / classic token.
    toks.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.len().cmp(&b.0.len())));
    toks
}

fn roman(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    let form = match a.get(1).map(|x| x.value.scalar()) {
        None => 0.0,
        Some(Value::Bool(true)) => 0.0,
        Some(Value::Bool(false)) => 4.0,
        Some(_) => num(a, 1)?.trunc(),
    };
    if !(0.0..=3999.0).contains(&n) || !(0.0..=4.0).contains(&form) {
        return Err(CellError::Value);
    }
    let toks = roman_tokens(form as u32);
    let mut rem = n as u32;
    let mut out = String::new();
    while rem > 0 {
        let Some((t, v)) = toks.iter().find(|(_, v)| *v <= rem) else { break };
        out.push_str(t);
        rem -= v;
    }
    text_val(out)
}

fn arabic(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let s = s.trim();
    if s.chars().count() > 255 {
        return Err(CellError::Value);
    }
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let mut vals = Vec::new();
    for ch in body.chars() {
        let u = ch.to_ascii_uppercase();
        let Some((_, v)) = NUMERALS.iter().find(|(c, _)| *c == u) else { return Err(CellError::Value) };
        vals.push(*v as i64);
    }
    let mut total = 0i64;
    for (i, v) in vals.iter().enumerate() {
        if vals.get(i + 1).is_some_and(|next| next > v) {
            total -= v;
        } else {
            total += v;
        }
    }
    num_val(if neg { -total as f64 } else { total as f64 })
}

fn base(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    let radix = num(a, 1)?.trunc();
    let min_len = opt_num(a, 2, 0.0)?.trunc();
    if !(0.0..9.007_199_254_740_992e15).contains(&n) || !(2.0..=36.0).contains(&radix) || !(0.0..=255.0).contains(&min_len) {
        return Err(CellError::Num);
    }
    let radix = radix as u64;
    let mut v = n as u64;
    let mut digits = Vec::new();
    loop {
        let d = (v % radix) as u32;
        digits.push(char::from_digit(d, radix as u32).unwrap_or('0').to_ascii_uppercase());
        v /= radix;
        if v == 0 {
            break;
        }
    }
    while digits.len() < min_len as usize {
        digits.push('0');
    }
    text_val(digits.iter().rev().collect::<String>())
}

fn decimal(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let radix = num(a, 1)?.trunc();
    if !(2.0..=36.0).contains(&radix) || s.chars().count() > 255 {
        return Err(CellError::Num);
    }
    let radix = radix as u32;
    let mut total = 0f64;
    for ch in s.trim().chars() {
        let Some(d) = ch.to_digit(radix) else { return Err(CellError::Num) };
        total = total * radix as f64 + d as f64;
    }
    if total >= 9.007_199_254_740_992e15 {
        return Err(CellError::Num);
    }
    num_val(total)
}

// ---------------------------------------------------------------------------------------------
// Matrices

/// A numeric matrix (every element must be a number, else `#VALUE!`).
fn matrix(v: &Value) -> R<(usize, usize, Vec<f64>)> {
    let arr = as_array(v);
    let mut out = Vec::with_capacity(arr.data.len());
    for x in arr.iter() {
        match x {
            Value::Number(n) => out.push(*n),
            Value::Error(e) => return Err(*e),
            _ => return Err(CellError::Value),
        }
    }
    Ok((arr.rows, arr.cols, out))
}

fn to_value_array(rows: usize, cols: usize, data: Vec<f64>) -> R<Value> {
    let vals = data.into_iter().map(|x| if x.is_finite() { Value::Number(x) } else { Value::Error(CellError::Num) }).collect();
    array_val(rows, cols, vals)
}

fn mmult(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (ar, ac, ad) = matrix(&crate::util::arg(a, 0)?.value)?;
    let (br, bc, bd) = matrix(&crate::util::arg(a, 1)?.value)?;
    if ac != br {
        return Err(CellError::Value);
    }
    if ar.saturating_mul(bc) > MAX_CELLS || ar.saturating_mul(bc).saturating_mul(ac) > 2_000_000_000 {
        return Err(CellError::Num);
    }
    let mut out = vec![0.0; ar * bc];
    for i in 0..ar {
        for k in 0..ac {
            let x = ad.get(i * ac + k).copied().unwrap_or(0.0);
            if x == 0.0 {
                continue;
            }
            for j in 0..bc {
                if let (Some(o), Some(y)) = (out.get_mut(i * bc + j), bd.get(k * bc + j)) {
                    *o += x * y;
                }
            }
        }
    }
    to_value_array(ar, bc, out)
}

/// LU decomposition with partial pivoting; returns (lu, perm, sign) or None when singular.
fn square(a: &[Arg]) -> R<(usize, Vec<f64>)> {
    let (r, c, d) = matrix(&crate::util::arg(a, 0)?.value)?;
    if r != c {
        return Err(CellError::Value);
    }
    if r > 2000 {
        return Err(CellError::Num);
    }
    Ok((r, d))
}

fn determinant(n: usize, mut m: Vec<f64>) -> f64 {
    let mut det = 1.0;
    for col in 0..n {
        let mut piv = col;
        let mut best = 0.0;
        for row in col..n {
            let v = m.get(row * n + col).copied().unwrap_or(0.0).abs();
            if v > best {
                best = v;
                piv = row;
            }
        }
        if best == 0.0 {
            return 0.0;
        }
        if piv != col {
            for k in 0..n {
                m.swap(piv * n + k, col * n + k);
            }
            det = -det;
        }
        let p = m.get(col * n + col).copied().unwrap_or(0.0);
        det *= p;
        for row in col + 1..n {
            let f = m.get(row * n + col).copied().unwrap_or(0.0) / p;
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                let v = m.get(col * n + k).copied().unwrap_or(0.0);
                if let Some(x) = m.get_mut(row * n + k) {
                    *x -= f * v;
                }
            }
        }
    }
    det
}

fn mdeterm(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, m) = square(a)?;
    let d = determinant(n, m);
    // Tidy results that should be integers (e.g. 0.9999999999999998 → 1).
    num_val(if d.abs() > 1e-300 { to_sig_digits(d, 15) } else { d })
}

fn minverse(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, mut m) = square(a)?;
    let mut inv = vec![0.0; n * n];
    for i in 0..n {
        if let Some(x) = inv.get_mut(i * n + i) {
            *x = 1.0;
        }
    }
    let scale = m.iter().fold(0.0f64, |acc, x| acc.max(x.abs())).max(1e-300);
    for col in 0..n {
        let mut piv = col;
        let mut best = 0.0;
        for row in col..n {
            let v = m.get(row * n + col).copied().unwrap_or(0.0).abs();
            if v > best {
                best = v;
                piv = row;
            }
        }
        if best <= scale * 1e-15 {
            return Err(CellError::Num);
        }
        if piv != col {
            for k in 0..n {
                m.swap(piv * n + k, col * n + k);
                inv.swap(piv * n + k, col * n + k);
            }
        }
        let p = m.get(col * n + col).copied().unwrap_or(1.0);
        for k in 0..n {
            if let Some(x) = m.get_mut(col * n + k) {
                *x /= p;
            }
            if let Some(x) = inv.get_mut(col * n + k) {
                *x /= p;
            }
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let f = m.get(row * n + col).copied().unwrap_or(0.0);
            if f == 0.0 {
                continue;
            }
            for k in 0..n {
                let mv = m.get(col * n + k).copied().unwrap_or(0.0);
                let iv = inv.get(col * n + k).copied().unwrap_or(0.0);
                if let Some(x) = m.get_mut(row * n + k) {
                    *x -= f * mv;
                }
                if let Some(x) = inv.get_mut(row * n + k) {
                    *x -= f * iv;
                }
            }
        }
    }
    let inv = inv.into_iter().map(|x| if x.abs() < 1e-15 { 0.0 } else { x }).collect();
    to_value_array(n, n, inv)
}

fn munit(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    let (r, _) = check_size(n, n)?;
    let mut data = vec![Value::Number(0.0); r * r];
    for i in 0..r {
        if let Some(x) = data.get_mut(i * r + i) {
            *x = Value::Number(1.0);
        }
    }
    array_val(r, r, data)
}

fn seriessum(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, n, m) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let coeffs = as_array(&crate::util::arg(a, 3)?.value);
    let mut total = 0.0;
    for (i, c) in coeffs.iter().enumerate() {
        let c = match c {
            Value::Number(v) => *v,
            Value::Error(e) => return Err(*e),
            _ => return Err(CellError::Value),
        };
        total += c * power_of(x, n + i as f64 * m)?;
    }
    num_val(total)
}

// ---------------------------------------------------------------------------------------------
// Criteria aggregates

/// Values at the matched positions of `target` for every (range, criteria) pair.
fn criteria_mask(a: &[Arg], first_pair: usize, rows: usize, cols: usize) -> R<Vec<bool>> {
    let rest = a.get(first_pair..).unwrap_or(&[]);
    if rest.is_empty() || rest.len() % 2 != 0 {
        return Err(CellError::Value);
    }
    let mut mask = vec![true; rows.saturating_mul(cols)];
    for pair in rest.chunks(2) {
        let (Some(range), Some(crit)) = (pair.first(), pair.get(1)) else { return Err(CellError::Value) };
        let r = as_array(&range.value);
        if r.rows != rows || r.cols != cols {
            return Err(CellError::Value);
        }
        let c = Criterion::parse(&crit.value.scalar());
        for (m, v) in mask.iter_mut().zip(r.iter()) {
            if *m && !c.matches(v) {
                *m = false;
            }
        }
    }
    Ok(mask)
}

/// Numbers in `target` where `mask` is set (errors at matched positions propagate).
fn masked_numbers(target: &Array, mask: &[bool]) -> R<Vec<f64>> {
    let mut out = Vec::new();
    for (v, m) in target.iter().zip(mask.iter()) {
        if !*m {
            continue;
        }
        match v {
            Value::Number(n) => out.push(*n),
            Value::Error(e) => return Err(*e),
            _ => {}
        }
    }
    Ok(out)
}

fn countifs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let first = as_array(&crate::util::arg(a, 0)?.value);
    let mask = criteria_mask(a, 0, first.rows, first.cols)?;
    num_val(mask.iter().filter(|m| **m).count() as f64)
}

/// The values of the `*IFS` target range with the mask from its pairs.
fn ifs_numbers(a: &[Arg]) -> R<Vec<f64>> {
    let target = as_array(&crate::util::arg(a, 0)?.value);
    let mask = criteria_mask(a, 1, target.rows, target.cols)?;
    masked_numbers(&target, &mask)
}

fn sumifs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(ifs_numbers(a)?.iter().sum())
}
fn averageifs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = ifs_numbers(a)?;
    if v.is_empty() {
        return Err(CellError::Div0);
    }
    num_val(v.iter().sum::<f64>() / v.len() as f64)
}
fn maxifs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = ifs_numbers(a)?;
    num_val(v.iter().copied().reduce(f64::max).unwrap_or(0.0))
}
fn minifs(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = ifs_numbers(a)?;
    num_val(v.iter().copied().reduce(f64::min).unwrap_or(0.0))
}

fn countif(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let range = as_array(&crate::util::arg(a, 0)?.value);
    let c = Criterion::parse(&crate::util::arg(a, 1)?.value.scalar());
    num_val(range.iter().filter(|v| c.matches(v)).count() as f64)
}

/// SUMIF / AVERAGEIF: matched numbers from the sum range, which takes the criteria range's
/// shape anchored at its own top-left.
fn if_numbers(a: &[Arg]) -> R<Vec<f64>> {
    let range = as_array(&crate::util::arg(a, 0)?.value);
    let c = Criterion::parse(&crate::util::arg(a, 1)?.value.scalar());
    let sum_range = if has(a, 2) { Some(as_array(&crate::util::arg(a, 2)?.value)) } else { None };
    let mut out = Vec::new();
    for r in 0..range.rows {
        for col in 0..range.cols {
            let Some(v) = range.get(r, col) else { continue };
            if !c.matches(v) {
                continue;
            }
            let target = match &sum_range {
                Some(s) => s.get(r, col),
                None => Some(v),
            };
            match target {
                Some(Value::Number(n)) => out.push(*n),
                Some(Value::Error(e)) => return Err(*e),
                _ => {}
            }
        }
    }
    Ok(out)
}
fn sumif(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(if_numbers(a)?.iter().sum())
}
fn averageif(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = if_numbers(a)?;
    if v.is_empty() {
        return Err(CellError::Div0);
    }
    num_val(v.iter().sum::<f64>() / v.len() as f64)
}

// ---------------------------------------------------------------------------------------------
// Registry

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!("SUM", 1, VAR, MathTrig, A, "SUM(number1, [number2], ...)", "Adds all the numbers in the arguments.", sum),
        f!("SUMSQ", 1, VAR, MathTrig, A, "SUMSQ(number1, [number2], ...)", "Adds the squares of the numbers.", sumsq),
        f!(
            "SUMPRODUCT",
            1,
            VAR,
            MathTrig,
            A,
            "SUMPRODUCT(array1, [array2], ...)",
            "Multiplies matching elements of equally sized arrays and adds the products.",
            sumproduct
        ),
        f!("SUMX2MY2", 2, 2, MathTrig, A, "SUMX2MY2(array_x, array_y)", "Sums the differences of squares of matching elements.", sumx2my2),
        f!("SUMX2PY2", 2, 2, MathTrig, A, "SUMX2PY2(array_x, array_y)", "Sums the sums of squares of matching elements.", sumx2py2),
        f!("SUMXMY2", 2, 2, MathTrig, A, "SUMXMY2(array_x, array_y)", "Sums the squared differences of matching elements.", sumxmy2),
        f!("PRODUCT", 1, VAR, MathTrig, A, "PRODUCT(number1, [number2], ...)", "Multiplies all the numbers in the arguments.", product),
        f!("ROUND", 2, 2, MathTrig, S, "ROUND(number, num_digits)", "Rounds a number to a given number of digits, halves away from zero.", round),
        f!("ROUNDUP", 2, 2, MathTrig, S, "ROUNDUP(number, num_digits)", "Rounds a number away from zero to a given number of digits.", roundup),
        f!("ROUNDDOWN", 2, 2, MathTrig, S, "ROUNDDOWN(number, num_digits)", "Rounds a number toward zero to a given number of digits.", rounddown),
        f!("MROUND", 2, 2, MathTrig, S, "MROUND(number, multiple)", "Rounds a number to the nearest multiple.", mround),
        f!("CEILING", 2, 2, Compatibility, S, "CEILING(number, significance)", "Rounds a number up to a multiple of significance.", ceiling),
        f!(
            "CEILING.MATH",
            1,
            3,
            MathTrig,
            S,
            "CEILING.MATH(number, [significance], [mode])",
            "Rounds a number up to the nearest integer or multiple.",
            ceiling_math
        ),
        f!(
            "CEILING.PRECISE",
            1,
            2,
            MathTrig,
            S,
            "CEILING.PRECISE(number, [significance])",
            "Rounds a number toward positive infinity to a multiple.",
            ceiling_precise
        ),
        f!(
            "ISO.CEILING",
            1,
            2,
            MathTrig,
            S,
            "ISO.CEILING(number, [significance])",
            "Rounds a number toward positive infinity to a multiple.",
            ceiling_precise
        ),
        f!("FLOOR", 2, 2, Compatibility, S, "FLOOR(number, significance)", "Rounds a number down to a multiple of significance.", floor),
        f!(
            "FLOOR.MATH",
            1,
            3,
            MathTrig,
            S,
            "FLOOR.MATH(number, [significance], [mode])",
            "Rounds a number down to the nearest integer or multiple.",
            floor_math
        ),
        f!(
            "FLOOR.PRECISE",
            1,
            2,
            MathTrig,
            S,
            "FLOOR.PRECISE(number, [significance])",
            "Rounds a number toward negative infinity to a multiple.",
            floor_precise
        ),
        f!("INT", 1, 1, MathTrig, S, "INT(number)", "Rounds a number down to the nearest integer.", int_fn),
        f!("TRUNC", 1, 2, MathTrig, S, "TRUNC(number, [num_digits])", "Cuts off the fractional part of a number at the given digits.", trunc),
        f!("MOD", 2, 2, MathTrig, S, "MOD(number, divisor)", "Remainder of a division, with the sign of the divisor.", mod_fn),
        f!("QUOTIENT", 2, 2, MathTrig, S, "QUOTIENT(numerator, denominator)", "Integer part of a division.", quotient),
        f!("ABS", 1, 1, MathTrig, S, "ABS(number)", "Absolute value of a number.", abs),
        f!("SIGN", 1, 1, MathTrig, S, "SIGN(number)", "1 for positive, -1 for negative and 0 for zero.", sign),
        f!("SQRT", 1, 1, MathTrig, S, "SQRT(number)", "Positive square root.", sqrt),
        f!("SQRTPI", 1, 1, MathTrig, S, "SQRTPI(number)", "Square root of the number times pi.", sqrtpi),
        f!("POWER", 2, 2, MathTrig, S, "POWER(number, power)", "Raises a number to a power.", power),
        f!("EXP", 1, 1, MathTrig, S, "EXP(number)", "e raised to the given power.", exp),
        f!("LN", 1, 1, MathTrig, S, "LN(number)", "Natural logarithm.", ln),
        f!("LOG", 1, 2, MathTrig, S, "LOG(number, [base])", "Logarithm to the given base (10 by default).", log),
        f!("LOG10", 1, 1, MathTrig, S, "LOG10(number)", "Base-10 logarithm.", log10),
        f!("PI", 0, 0, MathTrig, S, "PI()", "The constant pi to 15 digits.", pi),
        f!(volatile; "RAND", 0, 0, MathTrig, S, "RAND()", "A random number between 0 and 1, recalculated every time.", rand),
        f!(volatile; "RANDBETWEEN", 2, 2, MathTrig, S, "RANDBETWEEN(bottom, top)", "A random integer between two bounds, inclusive.", randbetween),
        f!("FACT", 1, 1, MathTrig, S, "FACT(number)", "Factorial of a number.", fact),
        f!("FACTDOUBLE", 1, 1, MathTrig, S, "FACTDOUBLE(number)", "Double factorial of a number.", factdouble),
        f!("COMBIN", 2, 2, MathTrig, S, "COMBIN(number, number_chosen)", "Number of combinations without repetition.", combin),
        f!("COMBINA", 2, 2, MathTrig, S, "COMBINA(number, number_chosen)", "Number of combinations with repetition.", combina),
        f!("PERMUT", 2, 2, Statistical, S, "PERMUT(number, number_chosen)", "Number of ordered selections without repetition.", permut),
        f!(
            "PERMUTATIONA",
            2,
            2,
            Statistical,
            S,
            "PERMUTATIONA(number, number_chosen)",
            "Number of ordered selections with repetition.",
            permutationa
        ),
        f!("GCD", 1, VAR, MathTrig, A, "GCD(number1, [number2], ...)", "Greatest common divisor of integers.", gcd),
        f!("LCM", 1, VAR, MathTrig, A, "LCM(number1, [number2], ...)", "Least common multiple of integers.", lcm),
        f!("EVEN", 1, 1, MathTrig, S, "EVEN(number)", "Rounds away from zero to the nearest even integer.", even),
        f!("ODD", 1, 1, MathTrig, S, "ODD(number)", "Rounds away from zero to the nearest odd integer.", odd),
        f!("SIN", 1, 1, MathTrig, S, "SIN(number)", "Sine of an angle in radians.", sin),
        f!("COS", 1, 1, MathTrig, S, "COS(number)", "Cosine of an angle in radians.", cos),
        f!("TAN", 1, 1, MathTrig, S, "TAN(number)", "Tangent of an angle in radians.", tan),
        f!("ASIN", 1, 1, MathTrig, S, "ASIN(number)", "Arcsine in radians.", asin),
        f!("ACOS", 1, 1, MathTrig, S, "ACOS(number)", "Arccosine in radians.", acos),
        f!("ATAN", 1, 1, MathTrig, S, "ATAN(number)", "Arctangent in radians.", atan),
        f!("ATAN2", 2, 2, MathTrig, S, "ATAN2(x_num, y_num)", "Angle of the point (x, y) from the x-axis in radians.", atan2),
        f!("SINH", 1, 1, MathTrig, S, "SINH(number)", "Hyperbolic sine.", sinh),
        f!("COSH", 1, 1, MathTrig, S, "COSH(number)", "Hyperbolic cosine.", cosh),
        f!("TANH", 1, 1, MathTrig, S, "TANH(number)", "Hyperbolic tangent.", tanh),
        f!("ASINH", 1, 1, MathTrig, S, "ASINH(number)", "Inverse hyperbolic sine.", asinh),
        f!("ACOSH", 1, 1, MathTrig, S, "ACOSH(number)", "Inverse hyperbolic cosine.", acosh),
        f!("ATANH", 1, 1, MathTrig, S, "ATANH(number)", "Inverse hyperbolic tangent.", atanh),
        f!("COT", 1, 1, MathTrig, S, "COT(number)", "Cotangent of an angle in radians.", cot),
        f!("COTH", 1, 1, MathTrig, S, "COTH(number)", "Hyperbolic cotangent.", coth),
        f!("CSC", 1, 1, MathTrig, S, "CSC(number)", "Cosecant of an angle in radians.", csc),
        f!("CSCH", 1, 1, MathTrig, S, "CSCH(number)", "Hyperbolic cosecant.", csch),
        f!("SEC", 1, 1, MathTrig, S, "SEC(number)", "Secant of an angle in radians.", sec),
        f!("SECH", 1, 1, MathTrig, S, "SECH(number)", "Hyperbolic secant.", sech),
        f!("ACOT", 1, 1, MathTrig, S, "ACOT(number)", "Arccotangent in radians (0 to pi).", acot),
        f!("ACOTH", 1, 1, MathTrig, S, "ACOTH(number)", "Inverse hyperbolic cotangent.", acoth),
        f!("DEGREES", 1, 1, MathTrig, S, "DEGREES(angle)", "Converts radians to degrees.", degrees),
        f!("RADIANS", 1, 1, MathTrig, S, "RADIANS(angle)", "Converts degrees to radians.", radians),
        f!("ROMAN", 1, 2, MathTrig, S, "ROMAN(number, [form])", "Converts a number to Roman numerals in one of five styles.", roman),
        f!("ARABIC", 1, 1, MathTrig, S, "ARABIC(text)", "Converts Roman numerals to a number.", arabic),
        f!("BASE", 2, 3, MathTrig, S, "BASE(number, radix, [min_length])", "Writes a number in another base, padded with zeros.", base),
        f!("DECIMAL", 2, 2, MathTrig, S, "DECIMAL(text, radix)", "Reads text in a given base as a number.", decimal),
        f!("MMULT", 2, 2, MathTrig, A, "MMULT(array1, array2)", "Matrix product of two arrays.", mmult),
        f!("MDETERM", 1, 1, MathTrig, A, "MDETERM(array)", "Determinant of a square matrix.", mdeterm),
        f!("MINVERSE", 1, 1, MathTrig, A, "MINVERSE(array)", "Inverse of a square matrix.", minverse),
        f!("MUNIT", 1, 1, MathTrig, S, "MUNIT(dimension)", "Identity matrix of the given size.", munit),
        f!("SERIESSUM", 4, 4, MathTrig, &[true, true, true, false], "SERIESSUM(x, n, m, coefficients)", "Sum of a power series.", seriessum),
        f!(
            "MULTINOMIAL",
            1,
            VAR,
            MathTrig,
            A,
            "MULTINOMIAL(number1, [number2], ...)",
            "Ratio of the factorial of a sum to the product of factorials.",
            multinomial
        ),
        f!("SUMIF", 2, 3, MathTrig, IF2, "SUMIF(range, criteria, [sum_range])", "Adds the cells that meet a condition.", sumif),
        f!(
            "SUMIFS",
            3,
            VAR,
            MathTrig,
            &SUMIFS_SCAL,
            "SUMIFS(sum_range, criteria_range1, criteria1, ...)",
            "Adds the cells that meet every condition.",
            sumifs
        ),
        f!("COUNTIF", 2, 2, Statistical, NUM_RANGE_REV, "COUNTIF(range, criteria)", "Counts the cells that meet a condition.", countif),
        f!(
            "COUNTIFS",
            2,
            VAR,
            Statistical,
            &COUNTIFS_SCAL,
            "COUNTIFS(criteria_range1, criteria1, ...)",
            "Counts the cells that meet every condition.",
            countifs
        ),
        f!(
            "AVERAGEIF",
            2,
            3,
            Statistical,
            IF2,
            "AVERAGEIF(range, criteria, [average_range])",
            "Average of the cells that meet a condition.",
            averageif
        ),
        f!(
            "AVERAGEIFS",
            3,
            VAR,
            Statistical,
            &SUMIFS_SCAL,
            "AVERAGEIFS(average_range, criteria_range1, criteria1, ...)",
            "Average of the cells that meet every condition.",
            averageifs
        ),
        f!(
            "MAXIFS",
            3,
            VAR,
            Statistical,
            &SUMIFS_SCAL,
            "MAXIFS(max_range, criteria_range1, criteria1, ...)",
            "Largest value among cells that meet every condition.",
            maxifs
        ),
        f!(
            "MINIFS",
            3,
            VAR,
            Statistical,
            &SUMIFS_SCAL,
            "MINIFS(min_range, criteria_range1, criteria1, ...)",
            "Smallest value among cells that meet every condition.",
            minifs
        ),
    ]
}

const NUM_RANGE_REV: &[bool] = &[false, true];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;
    use CellError::{Div0, NA, Num};

    fn ev1(name: &str, x: f64) -> Value {
        ev(name, vec![n(x)])
    }
    fn ev2(name: &str, x: f64, y: f64) -> Value {
        ev(name, vec![n(x), n(y)])
    }

    #[test]
    fn sums() {
        close(ev("SUM", vec![n(1.0), n(2.0), t("3"), b(true)]), 7.0);
        let r = arr(vec![vec![nv(1.0), tv("x"), Value::Bool(true), Value::Empty, nv(4.0)]]);
        close(ev("SUM", vec![rf(r.clone())]), 5.0);
        is_err(ev("SUM", vec![t("abc")]), CellError::Value);
        is_err(ev("SUM", vec![rf(arr(vec![vec![nv(1.0), Value::Error(Div0)]]))]), Div0);
        close(ev("SUM", vec![rf(tv("abc"))]), 0.0);
        close(ev("SUMSQ", vec![n(3.0), n(4.0)]), 25.0);
        close(ev("PRODUCT", vec![rf(r), n(2.0)]), 8.0);
        close(ev("PRODUCT", vec![rf(arr(vec![vec![tv("a")]]))]), 0.0);
        let a1 = arr(vec![vec![nv(3.0), nv(4.0)], vec![nv(8.0), nv(6.0)], vec![nv(1.0), nv(9.0)]]);
        let a2 = arr(vec![vec![nv(2.0), nv(7.0)], vec![nv(6.0), nv(7.0)], vec![nv(5.0), nv(3.0)]]);
        close(ev("SUMPRODUCT", vec![rf(a1.clone()), rf(a2)]), 156.0);
        close(ev("SUMPRODUCT", vec![rf(a1.clone())]), 31.0);
        is_err(ev("SUMPRODUCT", vec![rf(a1), rf(row(&[1.0, 2.0]))]), CellError::Value);
        close(ev("SUMPRODUCT", vec![av(row(&[1.0, 2.0])), av(arr(vec![vec![tv("x"), nv(3.0)]]))]), 6.0);
        let x = row(&[2.0, 3.0, 9.0, 1.0, 8.0, 7.0, 5.0]);
        let y = row(&[6.0, 5.0, 11.0, 7.0, 5.0, 4.0, 4.0]);
        close(ev("SUMX2MY2", vec![av(x.clone()), av(y.clone())]), -55.0);
        close(ev("SUMX2PY2", vec![av(x.clone()), av(y.clone())]), 521.0);
        close(ev("SUMXMY2", vec![av(x.clone()), av(y)]), 79.0);
        is_err(ev("SUMXMY2", vec![av(x), av(row(&[1.0]))]), NA);
    }

    #[test]
    fn rounding() {
        close(ev2("ROUND", 2.675, 2.0), 2.68);
        close(ev2("ROUND", -1.5, 0.0), -2.0);
        close(ev2("ROUND", 1234.567, -2.0), 1200.0);
        close(ev2("ROUND", 2.15, 1.0), 2.2);
        close(ev2("ROUNDUP", 3.2, 0.0), 4.0);
        close(ev2("ROUNDUP", -3.14259, 1.0), -3.2);
        close(ev2("ROUNDUP", 31415.92654, -2.0), 31500.0);
        close(ev2("ROUNDDOWN", -3.9, 0.0), -3.0);
        close(ev2("ROUNDDOWN", 2.71989, 3.0), 2.719);
        close(ev2("ROUNDDOWN", 31415.92654, -2.0), 31400.0);
        close(ev2("ROUNDUP", 0.1 + 0.2, 1.0), 0.3);
        close(ev2("TRUNC", 8.987, 2.0), 8.98);
        close(ev1("TRUNC", -8.9), -8.0);
        close(ev1("INT", -8.9), -9.0);
        close(ev1("INT", 8.9), 8.0);
        close(ev2("MROUND", 10.0, 3.0), 9.0);
        close(ev2("MROUND", -10.0, -3.0), -9.0);
        close(ev2("MROUND", 1.3, 0.2), 1.4);
        is_err(ev2("MROUND", 5.0, -2.0), Num);
        close(ev2("CEILING", 2.5, 1.0), 3.0);
        close(ev2("CEILING", -2.5, -2.0), -4.0);
        close(ev2("CEILING", -2.5, 2.0), -2.0);
        close(ev2("CEILING", 1.5, 0.1), 1.5);
        close(ev2("CEILING", 0.234, 0.01), 0.24);
        is_err(ev2("CEILING", 2.5, -1.0), Num);
        close(ev2("FLOOR", 3.7, 2.0), 2.0);
        close(ev2("FLOOR", -2.5, -2.0), -2.0);
        close(ev2("FLOOR", -2.5, 2.0), -4.0);
        is_err(ev2("FLOOR", 2.5, -2.0), Num);
        is_err(ev2("FLOOR", 2.5, 0.0), Div0);
        close(ev2("FLOOR", 1.58, 0.1), 1.5);
        close(ev2("FLOOR", 0.234, 0.01), 0.23);
        close(ev2("CEILING.MATH", 24.3, 5.0), 25.0);
        close(ev1("CEILING.MATH", 6.7), 7.0);
        close(ev2("CEILING.MATH", -8.1, 2.0), -8.0);
        close(ev("CEILING.MATH", vec![n(-5.5), n(2.0), n(-1.0)]), -6.0);
        close(ev2("FLOOR.MATH", 24.3, 5.0), 20.0);
        close(ev1("FLOOR.MATH", 6.7), 6.0);
        close(ev2("FLOOR.MATH", -8.1, 2.0), -10.0);
        close(ev("FLOOR.MATH", vec![n(-5.5), n(2.0), n(-1.0)]), -4.0);
        close(ev2("CEILING.PRECISE", -4.6, 2.0), -4.0);
        close(ev2("CEILING.PRECISE", 4.3, -2.0), 6.0);
        close(ev2("ISO.CEILING", 4.3, 0.0), 0.0);
        close(ev2("FLOOR.PRECISE", -3.2, -1.0), -4.0);
        close(ev1("FLOOR.PRECISE", 3.2), 3.0);
        close(ev1("EVEN", 1.5), 2.0);
        close(ev1("EVEN", 3.0), 4.0);
        close(ev1("EVEN", 2.0), 2.0);
        close(ev1("EVEN", -1.0), -2.0);
        close(ev1("EVEN", 0.0), 0.0);
        close(ev1("ODD", 1.5), 3.0);
        close(ev1("ODD", 3.0), 3.0);
        close(ev1("ODD", 2.0), 3.0);
        close(ev1("ODD", -1.0), -1.0);
        close(ev1("ODD", -2.0), -3.0);
        close(ev1("ODD", 0.0), 1.0);
    }

    #[test]
    fn arithmetic() {
        close(ev2("MOD", 3.0, 2.0), 1.0);
        close(ev2("MOD", -3.0, 2.0), 1.0);
        close(ev2("MOD", 3.0, -2.0), -1.0);
        close(ev2("MOD", -3.0, -2.0), -1.0);
        close(ev2("MOD", 5.5, 1.0), 0.5);
        is_err(ev2("MOD", 1.0, 0.0), Div0);
        close(ev2("QUOTIENT", 5.0, 2.0), 2.0);
        close(ev2("QUOTIENT", -10.0, 3.0), -3.0);
        is_err(ev2("QUOTIENT", 1.0, 0.0), Div0);
        close(ev1("ABS", -4.0), 4.0);
        close(ev1("SIGN", -0.0001), -1.0);
        close(ev1("SIGN", 0.0), 0.0);
        close(ev1("SQRT", 16.0), 4.0);
        is_err(ev1("SQRT", -16.0), Num);
        close(ev1("SQRTPI", 1.0), 1.772_453_850_905_516);
        close(ev2("POWER", 5.0, 2.0), 25.0);
        close(ev2("POWER", 4.0, 1.25), 5.656_854_249_492_381);
        close(ev2("POWER", -2.0, 3.0), -8.0);
        is_err(ev2("POWER", 0.0, 0.0), Num);
        is_err(ev2("POWER", 0.0, -1.0), Div0);
        is_err(ev2("POWER", -8.0, 1.0 / 3.0), Num);
        is_err(ev2("POWER", 10.0, 400.0), Num);
        close(ev1("EXP", 1.0), std::f64::consts::E);
        close(ev2("LOG", 8.0, 2.0), 3.0);
        close(ev1("LOG", 10.0), 1.0);
        close(ev2("LOG", 81.0, 3.0), 4.0);
        is_err(ev1("LN", 0.0), Num);
        is_err(ev2("LOG", 10.0, 1.0), Div0);
        is_err(ev1("LOG10", -1.0), Num);
        close(ev1("LN", std::f64::consts::E), 1.0);
        close(ev1("LOG10", 1e5), 5.0);
        close(ev("PI", vec![]), PI);
        close(ev("SQRT", vec![t("16")]), 4.0);
        is_err(ev("SQRT", vec![t("x")]), CellError::Value);
    }

    #[test]
    fn combinatorics() {
        close(ev1("FACT", 5.0), 120.0);
        close(ev1("FACT", 1.9), 1.0);
        close(ev1("FACT", 0.0), 1.0);
        is_err(ev1("FACT", -1.0), Num);
        is_err(ev1("FACT", 171.0), Num);
        close(ev1("FACTDOUBLE", 6.0), 48.0);
        close(ev1("FACTDOUBLE", 7.0), 105.0);
        close(ev1("FACTDOUBLE", -1.0), 1.0);
        is_err(ev1("FACTDOUBLE", -2.0), Num);
        close(ev2("COMBIN", 8.0, 2.0), 28.0);
        close(ev2("COMBIN", 8.9, 2.9), 28.0);
        is_err(ev2("COMBIN", 2.0, 3.0), Num);
        close(ev2("COMBIN", 60.0, 30.0), 118_264_581_564_861_424.0);
        close(ev2("COMBINA", 4.0, 3.0), 20.0);
        close(ev2("COMBINA", 10.0, 3.0), 220.0);
        close(ev2("PERMUT", 100.0, 3.0), 970_200.0);
        close(ev2("PERMUT", 3.0, 2.0), 6.0);
        is_err(ev2("PERMUT", 2.0, 3.0), Num);
        close(ev2("PERMUTATIONA", 3.0, 2.0), 9.0);
        close(ev2("GCD", 24.0, 36.0), 12.0);
        close(ev2("GCD", 5.0, 0.0), 5.0);
        close(ev2("GCD", 7.0, 1.0), 1.0);
        is_err(ev2("GCD", -1.0, 5.0), Num);
        close(ev("GCD", vec![av(row(&[12.0, 18.0, 30.0]))]), 6.0);
        close(ev2("LCM", 24.0, 36.0), 72.0);
        close(ev2("LCM", 5.0, 2.0), 10.0);
        close(ev2("LCM", 5.0, 0.0), 0.0);
        close(ev("MULTINOMIAL", vec![n(2.0), n(3.0), n(4.0)]), 1260.0);
        is_err(ev("MULTINOMIAL", vec![n(-1.0), n(2.0)]), Num);
    }

    #[test]
    fn trig() {
        close(ev1("SIN", PI / 2.0), 1.0);
        close(ev1("COS", 0.0), 1.0);
        close(ev1("TAN", PI / 4.0), 1.0);
        is_err(ev1("SIN", 1e10), Num);
        close(ev1("ASIN", 1.0), PI / 2.0);
        is_err(ev1("ACOS", 1.5), Num);
        close(ev1("ACOT", 2.0), 0.463_647_609_000_806);
        close(ev1("ACOTH", 6.0), 0.168_236_118_310_606);
        is_err(ev1("ACOTH", 0.5), Num);
        close(ev1("COT", 30.0), -0.156_119_952_161_659);
        is_err(ev1("COT", 0.0), Div0);
        close(ev1("COTH", 2.0), 1.037_314_720_727_548);
        close(ev1("CSC", 15.0), 1.537_780_562_208_209);
        close(ev1("CSCH", 1.5), 0.469_642_440_595_653);
        close(ev1("SEC", 45.0), 1.903_594_407_014_6);
        close_tol(ev1("SECH", 45.0), 5.725_037_161_098_787e-20, 1e-9);
        close(ev2("ATAN2", 1.0, 1.0), PI / 4.0);
        close(ev2("ATAN2", -1.0, -1.0), -3.0 * PI / 4.0);
        is_err(ev2("ATAN2", 0.0, 0.0), Div0);
        close(ev1("ASINH", -2.5), -1.647_231_146_371_096);
        close(ev1("ACOSH", 10.0), 2.993_222_846_126_381);
        is_err(ev1("ACOSH", 0.5), Num);
        close(ev1("ATANH", 0.5), 0.549_306_144_334_055);
        is_err(ev1("ATANH", 1.0), Num);
        close(ev1("SINH", 1.0), 1.175_201_193_643_801);
        close(ev1("DEGREES", PI), 180.0);
        close(ev1("RADIANS", 270.0), 4.712_388_980_384_69);
    }

    #[test]
    fn number_systems() {
        is_text(ev1("ROMAN", 499.0), "CDXCIX");
        is_text(ev2("ROMAN", 499.0, 1.0), "LDVLIV");
        is_text(ev2("ROMAN", 499.0, 2.0), "XDIX");
        is_text(ev2("ROMAN", 499.0, 3.0), "VDIV");
        is_text(ev2("ROMAN", 499.0, 4.0), "ID");
        is_text(ev("ROMAN", vec![n(499.0), b(false)]), "ID");
        is_text(ev1("ROMAN", 1999.0), "MCMXCIX");
        is_text(ev1("ROMAN", 3999.0), "MMMCMXCIX");
        is_text(ev1("ROMAN", 0.0), "");
        is_err(ev1("ROMAN", 4000.0), CellError::Value);
        is_err(ev1("ROMAN", -1.0), CellError::Value);
        close(ev("ARABIC", vec![t("LVII")]), 57.0);
        close(ev("ARABIC", vec![t("mcmxii")]), 1912.0);
        close(ev("ARABIC", vec![t("")]), 0.0);
        close(ev("ARABIC", vec![t("-XIV")]), -14.0);
        is_err(ev("ARABIC", vec![t("ABC")]), CellError::Value);
        is_text(ev2("BASE", 7.0, 2.0), "111");
        is_text(ev2("BASE", 100.0, 16.0), "64");
        is_text(ev("BASE", vec![n(15.0), n(2.0), n(10.0)]), "0000001111");
        is_text(ev2("BASE", 0.0, 36.0), "0");
        is_err(ev2("BASE", 7.0, 1.0), Num);
        is_err(ev2("BASE", -7.0, 2.0), Num);
        close(ev("DECIMAL", vec![t("FF"), n(16.0)]), 255.0);
        close(ev("DECIMAL", vec![t("111"), n(2.0)]), 7.0);
        close(ev("DECIMAL", vec![t("zap"), n(36.0)]), 45745.0);
        is_err(ev("DECIMAL", vec![t("12"), n(2.0)]), Num);
    }

    #[test]
    fn matrices() {
        let a = arr(vec![vec![nv(1.0), nv(2.0)], vec![nv(3.0), nv(4.0)]]);
        let b2 = arr(vec![vec![nv(5.0), nv(6.0)], vec![nv(7.0), nv(8.0)]]);
        assert_eq!(rows_of(&ev("MMULT", vec![av(a.clone()), av(b2)])), vec![vec![nv(19.0), nv(22.0)], vec![nv(43.0), nv(50.0)]]);
        is_err(ev("MMULT", vec![av(a.clone()), av(row(&[1.0, 2.0]))]), CellError::Value);
        is_err(ev("MMULT", vec![av(arr(vec![vec![tv("x")]])), av(row(&[1.0]))]), CellError::Value);
        close(ev("MDETERM", vec![av(a.clone())]), -2.0);
        let m3 = arr(vec![vec![nv(3.0), nv(6.0), nv(1.0)], vec![nv(1.0), nv(1.0), nv(0.0)], vec![nv(3.0), nv(10.0), nv(2.0)]]);
        close(ev("MDETERM", vec![av(m3)]), 1.0);
        let m4 = arr(vec![
            vec![nv(1.0), nv(3.0), nv(8.0), nv(5.0)],
            vec![nv(1.0), nv(3.0), nv(6.0), nv(1.0)],
            vec![nv(1.0), nv(1.0), nv(1.0), nv(0.0)],
            vec![nv(7.0), nv(3.0), nv(10.0), nv(2.0)],
        ]);
        close(ev("MDETERM", vec![av(m4)]), 88.0);
        is_err(ev("MDETERM", vec![av(row(&[1.0, 2.0]))]), CellError::Value);
        let inv = ev("MINVERSE", vec![av(arr(vec![vec![nv(4.0), nv(-1.0)], vec![nv(2.0), nv(0.0)]]))]);
        let r = rows_of(&inv);
        close(r[0][0].clone(), 0.0);
        close(r[0][1].clone(), 0.5);
        close(r[1][0].clone(), -1.0);
        close(r[1][1].clone(), 2.0);
        is_err(ev("MINVERSE", vec![av(arr(vec![vec![nv(1.0), nv(2.0)], vec![nv(2.0), nv(4.0)]]))]), Num);
        assert_eq!(rows_of(&ev1("MUNIT", 2.0)), vec![vec![nv(1.0), nv(0.0)], vec![nv(0.0), nv(1.0)]]);
        is_err(ev1("MUNIT", 0.0), CellError::Value);
        is_err(ev1("MUNIT", 1e6), Num);
        let coeffs = row(&[1.0, -0.5, 1.0 / 24.0, -1.0 / 720.0]);
        close_tol(ev("SERIESSUM", vec![n(PI / 4.0), n(0.0), n(2.0), av(coeffs)]), 0.707_103_214_823, 1e-9);
    }

    #[test]
    fn random() {
        let mut ctx = TestCtx::default();
        let spec = crate::lookup("RAND").unwrap();
        assert!(spec.volatile);
        for _ in 0..50 {
            let v = crate::call(spec, &[], &mut ctx);
            let x = v.as_f64().unwrap();
            assert!((0.0..1.0).contains(&x));
        }
        let spec = crate::lookup("RANDBETWEEN").unwrap();
        for _ in 0..50 {
            let x = crate::call(spec, &[n(1.0), n(6.0)], &mut ctx).as_f64().unwrap();
            assert!((1.0..=6.0).contains(&x) && x.fract() == 0.0);
        }
        is_err(ev2("RANDBETWEEN", 5.0, 1.0), Num);
    }

    #[test]
    fn lifting() {
        assert_eq!(rows_of(&ev("ABS", vec![av(row(&[-1.0, 2.0, -3.0]))])), vec![vec![nv(1.0), nv(2.0), nv(3.0)]]);
        let v = ev("POWER", vec![av(col(&[1.0, 2.0])), av(row(&[2.0, 3.0]))]);
        assert_eq!(rows_of(&v), vec![vec![nv(1.0), nv(1.0)], vec![nv(4.0), nv(8.0)]]);
        close(ev("ROUND", vec![av(arr(vec![vec![nv(1.25)]])), n(1.0)]), 1.3);
    }

    fn data() -> (Value, Value) {
        let names = arr(vec![vec![tv("apple")], vec![tv("banana")], vec![tv("Apple")], vec![Value::Empty], vec![tv("cherry")]]);
        let qty = col(&[10.0, 20.0, 30.0, 40.0, 50.0]);
        (names, qty)
    }

    #[test]
    fn criteria_functions() {
        let (names, qty) = data();
        close(ev("COUNTIF", vec![rf(names.clone()), t("apple")]), 2.0);
        close(ev("COUNTIF", vec![rf(names.clone()), t("a*")]), 2.0);
        close(ev("COUNTIF", vec![rf(names.clone()), t("<>apple")]), 3.0);
        close(ev("COUNTIF", vec![rf(names.clone()), t("")]), 1.0);
        close(ev("COUNTIF", vec![rf(qty.clone()), t(">20")]), 3.0);
        close(ev("COUNTIF", vec![rf(qty.clone()), n(30.0)]), 1.0);
        let multi = ev("COUNTIF", vec![rf(names.clone()), av(arr(vec![vec![tv("apple"), tv("cherry")]]))]);
        assert_eq!(rows_of(&multi), vec![vec![nv(2.0), nv(1.0)]]);
        close(ev("SUMIF", vec![rf(names.clone()), t("apple"), rf(qty.clone())]), 40.0);
        close(ev("SUMIF", vec![rf(qty.clone()), t(">=30")]), 120.0);
        close(ev("SUMIF", vec![rf(names.clone()), t("?????"), rf(col(&[1.0]))]), 1.0);
        close(ev("AVERAGEIF", vec![rf(qty.clone()), t(">20")]), 40.0);
        is_err(ev("AVERAGEIF", vec![rf(qty.clone()), t(">100")]), Div0);
        close(ev("SUMIFS", vec![rf(qty.clone()), rf(names.clone()), t("apple"), rf(qty.clone()), t(">15")]), 30.0);
        close(ev("COUNTIFS", vec![rf(names.clone()), t("*a*"), rf(qty.clone()), t("<25")]), 2.0);
        close(ev("AVERAGEIFS", vec![rf(qty.clone()), rf(names.clone()), t("<>banana")]), 32.5);
        close(ev("MAXIFS", vec![rf(qty.clone()), rf(names.clone()), t("apple")]), 30.0);
        close(ev("MINIFS", vec![rf(qty.clone()), rf(names.clone()), t("apple")]), 10.0);
        close(ev("MAXIFS", vec![rf(qty.clone()), rf(names.clone()), t("zzz")]), 0.0);
        is_err(ev("SUMIFS", vec![rf(qty.clone()), rf(row(&[1.0, 2.0])), t("1")]), CellError::Value);
        is_err(ev("COUNTIFS", vec![rf(qty.clone()), t(">1"), rf(qty)]), CellError::Value);
        is_err(ev("SUMIF", vec![rf(col(&[1.0, 2.0])), t(">0"), rf(arr(vec![vec![Value::Error(NA)], vec![nv(1.0)]]))]), NA);
    }
}
