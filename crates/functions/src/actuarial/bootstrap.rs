//! Replayable Pearson-residual chain-ladder bootstrap with optional Gamma process draws.
//!
//! Published basis: England (2002), sections 2–3, DOI S0167-6687(02)00161-0.
//! Randomness is supplied as explicit uniform matrices; no RNG/sampler lives here.
//! Centered residuals are variance-calibrated to the original Pearson dispersion. An
//! all-pool positivity guard avoids conditioning the experiment on accepted/clipped draws.

use crate::special::{reg_gamma_p, reg_gamma_q};
use crate::util::{R, arg, array_val, as_array, has, num_val};
use crate::{Arg, Ctx, FnSpec};
use gridcraft_core::{CellError, Value};

const MAX_ORIGINS: usize = 20;
const MAX_REPLICATES: usize = 1024;
const MAX_GAMMA_DRAWS: usize = 4096;
const MAX_WORK: usize = 2_000_000;

/// Returns a finite scalar, rejecting NaN and infinity with `#NUM!`.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}
/// Reads a finite numeric observation without coercing missing or nonnumeric cells.
fn numeric(v: &Value) -> R<f64> {
    match v {
        Value::Number(x) => finite(*x),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}
/// Accumulates finite terms with Neumaier compensation; rejects nonfinite intermediate sums.
fn sum(xs: impl IntoIterator<Item = f64>) -> R<f64> {
    let (mut value, mut correction) = (0.0_f64, 0.0_f64);
    for x in xs {
        finite(x)?;
        let next = finite(value + x)?;
        correction += if value.abs() >= x.abs() { (value - next) + x } else { (x - next) + value };
        value = next;
    }
    finite(value + correction)
}

struct Fitted {
    factors: Vec<f64>,
    fitted: Vec<Vec<f64>>,
    pool: Vec<f64>,
    phi: f64,
}

/// Parses a bounded cumulative development triangle with strictly blank unobserved cells.
/// Requires positive first claims and nonnegative observed increments; malformed support is rejected.
fn triangle(a: &[Arg]) -> R<Vec<Vec<f64>>> {
    let arr = as_array(&arg(a, 0)?.value);
    let n = arr.rows;
    if !(3..=MAX_ORIGINS).contains(&n) || arr.cols != n || arr.data.len() != n * n {
        return Err(CellError::Num);
    }
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = Vec::with_capacity(n - i);
        let mut previous = 0.0;
        for j in 0..n {
            let value = arr.data.get(i * n + j).ok_or(CellError::Value)?;
            if j < n - i {
                let cumulative = numeric(value)?;
                if cumulative < previous || (j == 0 && cumulative <= 0.0) {
                    return Err(CellError::Num);
                }
                row.push(cumulative);
                previous = cumulative;
            } else if !matches!(value, Value::Empty) {
                return Err(CellError::Value);
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Calculates positive volume-weighted development factors from observed cumulative rows.
fn factors(rows: &[Vec<f64>]) -> R<Vec<f64>> {
    let n = rows.len();
    (0..n - 1)
        .map(|j| {
            let before = sum(rows.iter().take(n - j - 1).map(|r| r[j]))?;
            let after = sum(rows.iter().take(n - j - 1).map(|r| r[j + 1]))?;
            if before <= 0.0 || after < before {
                return Err(CellError::Num);
            }
            finite(after / before)
        })
        .collect()
}

/// Builds backward fitted increments and a centered Pearson residual pool calibrated to dispersion.
/// Checks the entire pool's support before drawing, preventing hidden clipping or conditional resampling.
fn fit(rows: &[Vec<f64>]) -> R<Fitted> {
    let n = rows.len();
    let count = n * (n + 1) / 2;
    let df = count.checked_sub(2 * n - 1).filter(|d| *d > 0).ok_or(CellError::Num)?;
    let factors = factors(rows)?;
    // Zero fitted increment makes the Pearson residual undefined, not zero.
    if factors.iter().any(|f| *f <= 1.0) {
        return Err(CellError::Num);
    }
    let mut fitted = Vec::with_capacity(n);
    let mut residuals = Vec::with_capacity(count);
    let mut minimum_mean = f64::INFINITY;
    for row in rows {
        let mut cumulative = row.clone();
        for j in (0..row.len() - 1).rev() {
            cumulative[j] = finite(cumulative[j + 1] / factors[j])?;
        }
        let (mut previous_fit, mut previous_observed) = (0.0, 0.0);
        let mut means = Vec::with_capacity(row.len());
        for (observed, mean_cumulative) in row.iter().zip(cumulative) {
            let mean = finite(mean_cumulative - previous_fit)?;
            if mean <= 0.0 {
                return Err(CellError::Num);
            }
            let incremental = observed - previous_observed;
            residuals.push(finite((incremental - mean) / mean.sqrt())?);
            minimum_mean = minimum_mean.min(mean);
            means.push(mean);
            previous_fit = mean_cumulative;
            previous_observed = *observed;
        }
        fitted.push(means);
    }
    let phi = finite(sum(residuals.iter().map(|r| r * r))? / df as f64)?;
    let center = sum(residuals.iter().copied())? / count as f64;
    let centered_variance = sum(residuals.iter().map(|r| (r - center) * (r - center)))? / count as f64;
    let pool = if phi == 0.0 {
        vec![0.0; count]
    } else {
        if centered_variance <= 0.0 {
            return Err(CellError::Num);
        }
        let scale = finite((phi / centered_variance).sqrt())?;
        residuals.iter().map(|r| finite((r - center) * scale)).collect::<R<Vec<_>>>()?
    };
    let smallest = pool.iter().copied().fold(f64::INFINITY, f64::min);
    if minimum_mean.sqrt() + smallest <= 0.0 {
        // The ENTIRE experiment is unsupported: never silently discard bad random draws.
        return Err(CellError::Num);
    }
    Ok(Fitted { factors, fitted, pool, phi })
}

/// Reads aligned external uniform rows in `[0,1)`, one row per replication.
/// A correctly sized column vector is accepted as one draw; missing values are never recycled.
fn uniforms(a: &[Arg], index: usize, columns: usize) -> R<Vec<Vec<f64>>> {
    let arr = as_array(&arg(a, index)?.value);
    let (rows, cols) = if arr.cols == 1 && arr.rows == columns { (1, columns) } else { (arr.rows, arr.cols) };
    if cols != columns || rows == 0 || rows > MAX_REPLICATES || arr.data.len() != rows * cols {
        return Err(CellError::Value);
    }
    let values = arr.iter().map(numeric).collect::<R<Vec<_>>>()?;
    if values.iter().any(|u| !(0.0..1.0).contains(u)) {
        return Err(CellError::Num);
    }
    Ok(values.chunks(columns).map(<[f64]>::to_vec).collect())
}

/// Gamma process inverse with a checked relative tail residual. The mathematical model is
/// Gamma(mean/phi, scale=phi); its first two moments match ODP, its shape is an extra assumption.
fn gamma_draw(u: f64, mean: f64, phi: f64, ctx: &mut dyn Ctx) -> R<f64> {
    super::work::charge(ctx, 64)?;
    if mean == 0.0 || phi == 0.0 || u == 0.0 {
        return Ok(if phi == 0.0 { mean } else { 0.0 });
    }
    let shape = finite(mean / phi)?;
    if !(0.05..=1000.0).contains(&shape) {
        return Err(CellError::Num);
    }
    if shape == 1.0 {
        let result = finite(-phi * (-u).ln_1p())?;
        return if result > 0.0 { Ok(result) } else { Err(CellError::Num) };
    }
    let upper = u > 0.5;
    let target = if upper { 1.0 - u } else { u };
    let mut tail = |x: f64| -> R<f64> {
        super::work::charge(ctx, 20_000)?;
        Ok(if upper { reg_gamma_q(shape, x) } else { reg_gamma_p(shape, x) })
    };
    let mut hi = shape.max(1.0);
    for _ in 0..64 {
        let value = finite(tail(hi)?)?;
        if (upper && value <= target) || (!upper && value >= target) {
            break;
        }
        hi = finite(hi * 2.0)?;
    }
    let high_value = tail(hi)?;
    if (upper && high_value > target) || (!upper && high_value < target) {
        return Err(CellError::Num);
    }
    let mut lo_log = f64::MIN_POSITIVE.ln();
    let low_value = tail(f64::MIN_POSITIVE)?;
    if (upper && low_value < target) || (!upper && low_value > target) {
        return Err(CellError::Num);
    }
    let mut hi_log = hi.ln();
    for _ in 0..160 {
        let mid = (0.5 * (lo_log + hi_log)).exp();
        let value = finite(tail(mid)?)?;
        if (value - target).abs() <= 1e-9 * target {
            let result = finite(mid * phi)?;
            return if result > 0.0 { Ok(result) } else { Err(CellError::Num) };
        }
        if (upper && value > target) || (!upper && value < target) {
            lo_log = mid.ln();
        } else {
            hi_log = mid.ln();
        }
    }
    Err(CellError::Num)
}

/// Replays batched Pearson-residual reserve bootstrap draws from explicit uniform inputs.
/// Each row contains per-origin parameter means and total, optional Gamma predictive reserves
/// and total, then original dispersion; absent process draws produce blank predictive cells.
fn bootstrap(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    let original = triangle(a)?;
    let n = original.len();
    let observed = n * (n + 1) / 2;
    let future = n * (n - 1) / 2;
    let model = fit(&original)?;
    let resampling = uniforms(a, 1, observed)?;
    let count = resampling.len();
    if count.saturating_mul(n).saturating_mul(n).saturating_mul(n) > MAX_WORK {
        return Err(CellError::Num);
    }
    let process = if has(a, 2) {
        let u = uniforms(a, 2, future)?;
        if u.len() != count || count.saturating_mul(future) > MAX_GAMMA_DRAWS {
            return Err(CellError::Num);
        }
        Some(u)
    } else {
        None
    };
    let columns = 2 * n + 3;
    super::work::charge(ctx, (count as u64).saturating_mul(columns as u64))?;
    let mut out = Vec::with_capacity(count * columns);
    for (replicate, draws) in resampling.iter().enumerate() {
        super::work::charge(ctx, super::work::product(&[n as u64, n as u64, n as u64, 32]))?;
        let mut cursor = 0;
        let mut pseudo = Vec::with_capacity(n);
        for row in &model.fitted {
            let mut cumulative = 0.0;
            let mut observed_row = Vec::with_capacity(row.len());
            for mean in row {
                let u = *draws.get(cursor).ok_or(CellError::Value)?;
                let index = ((u * observed as f64) as usize).min(observed - 1);
                let residual = *model.pool.get(index).ok_or(CellError::Value)?;
                let incremental = finite(mean + mean.sqrt() * residual)?;
                if incremental < 0.0 {
                    return Err(CellError::Num);
                }
                cumulative = finite(cumulative + incremental)?;
                observed_row.push(cumulative);
                cursor += 1;
            }
            pseudo.push(observed_row);
        }
        let projected_factors = if model.phi == 0.0 { model.factors.clone() } else { factors(&pseudo)? };
        let mut means = Vec::with_capacity(n);
        let mut predictive = Vec::with_capacity(n);
        let mut future_cursor = 0;
        for row in &pseudo {
            let mut latest = *row.last().ok_or(CellError::Value)?;
            let (mut expected_reserve, mut sampled_reserve) = (0.0, 0.0);
            for factor in projected_factors.iter().skip(row.len() - 1) {
                let cumulative = finite(latest * factor)?;
                let mean = finite(cumulative - latest)?;
                if mean < 0.0 {
                    return Err(CellError::Num);
                }
                expected_reserve = finite(expected_reserve + mean)?;
                if let Some(u) = &process {
                    let draw = u.get(replicate).and_then(|r| r.get(future_cursor)).copied().ok_or(CellError::Value)?;
                    sampled_reserve = finite(sampled_reserve + gamma_draw(draw, mean, model.phi, ctx)?)?;
                }
                latest = cumulative;
                future_cursor += 1;
            }
            means.push(expected_reserve);
            predictive.push(sampled_reserve);
        }
        let total_mean = sum(means.iter().copied())?;
        out.extend(means.into_iter().map(Value::Number));
        out.push(Value::Number(total_mean));
        if process.is_some() {
            let total_predictive = sum(predictive.iter().copied())?;
            out.extend(predictive.into_iter().map(Value::Number));
            out.push(Value::Number(total_predictive));
        } else {
            out.extend(std::iter::repeat_n(Value::Empty, n + 1));
        }
        out.push(num_val(model.phi)?);
    }
    array_val(count, columns, out)
}

/// Registers the externally driven ODP-moment reserve bootstrap without introducing an RNG.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![act!(
        "ACT.RESERVE.BOOTSTRAP.ODP",
        2,
        3,
        Financial,
        &[false, false, false],
        "ACT.RESERVE.BOOTSTRAP.ODP(triangle,residual_uniforms,[process_uniforms])",
        "Replayable centered Pearson-residual reserve bootstrap, optional Gamma process draws; uniforms are supplied explicitly.",
        bootstrap
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    fn triangle_exact() -> Arg {
        av(arr(vec![vec![nv(100.0), nv(200.0), nv(400.0)], vec![nv(100.0), nv(200.0), Value::Empty], vec![nv(100.0), Value::Empty, Value::Empty]]))
    }
    fn triangle_noisy() -> Arg {
        av(arr(vec![vec![nv(100.0), nv(160.0), nv(400.0)], vec![nv(120.0), nv(280.0), Value::Empty], vec![nv(130.0), Value::Empty, Value::Empty]]))
    }
    fn u(rows: usize, cols: usize, value: f64) -> Arg {
        av(arr(vec![vec![nv(value); cols]; rows]))
    }
    fn raw(value: Value) -> Vec<Value> {
        match value {
            Value::Array(a) => a.data.clone(),
            other => panic!("{other:?}"),
        }
    }
    fn close(actual: &Value, expected: f64) {
        let actual = numeric(actual).unwrap();
        assert!((actual - expected).abs() < 1e-8 * (1.0 + expected.abs()), "{actual} != {expected}");
    }
    #[test]
    fn zero_dispersion_bootstrap_replays_exact_chain_ladder_without_fake_process() {
        let result = raw(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_exact(), u(2, 6, 0.25)]));
        assert_eq!(result.len(), 18);
        for row in result.chunks(9) {
            for (actual, expected) in row[..4].iter().zip([0.0, 200.0, 300.0, 500.0]) {
                close(actual, expected);
            }
            assert!(row[4..8].iter().all(|v| matches!(v, Value::Empty)));
            close(&row[8], 0.0);
        }
        let result = raw(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_exact(), u(1, 6, 0.75), u(1, 3, 0.6)]));
        close(&result[7], 500.0);
    }
    #[test]
    fn centered_pool_preserves_dispersion_and_zero_residual_refit() {
        let rows = triangle(&[triangle_noisy()]).unwrap();
        let model = fit(&rows).unwrap();
        assert!(sum(model.pool.iter().copied()).unwrap().abs() < 1e-12);
        assert!((sum(model.pool.iter().map(|r| r * r)).unwrap() / 6.0 - model.phi).abs() < 1e-12);
        assert!((model.phi - 110.0 / 7.0).abs() < 1e-12);
        // Pool index2 is a structurally zero residual; all sampled pseudo means retain CL fit.
        let result = raw(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(1, 6, 0.4)]));
        close(&result[3], 940.0);
        let changed = raw(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(1, 6, 0.0)]));
        assert!((numeric(&changed[3]).unwrap() - 940.0).abs() > 1.0);
    }
    #[test]
    fn gamma_process_draws_have_published_mean_variance_parameterization() {
        let value = gamma_draw(0.75, 50.0, 50.0, &mut TestCtx::default()).unwrap();
        assert!((value + 50.0 * 0.25_f64.ln()).abs() < 1e-12);
        let result = raw(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(1, 6, 0.4), u(1, 3, 0.5)]));
        close(&result[3], 940.0);
        assert!(numeric(&result[7]).unwrap() > 0.0);
        assert_ne!(result[3], result[7]);
        assert_eq!(gamma_draw(0.0, 50.0, 50.0, &mut TestCtx::default()).unwrap(), 0.0);
    }
    #[test]
    fn uniforms_dimensions_process_domains_and_entire_residual_support_are_checked() {
        assert_eq!(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(1, 6, 1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(1, 5, 0.5)]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![triangle_noisy(), u(2, 6, 0.5), u(1, 3, 0.5)]), Value::Error(CellError::Num));
        let invalid = av(arr(vec![
            vec![nv(1.0), nv(100.0), nv(200.0)],
            vec![nv(100.0), nv(1000.0), Value::Empty],
            vec![nv(100.0), Value::Empty, Value::Empty],
        ]));
        assert_eq!(ev("ACT.RESERVE.BOOTSTRAP.ODP", vec![invalid, u(1, 6, 0.4)]), Value::Error(CellError::Num));
        assert_eq!(gamma_draw(0.5, 1.0, 1000.0, &mut TestCtx::default()), Err(CellError::Num));
    }
}
