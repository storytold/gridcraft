//! Original deterministic survival calibration, conjugate inference and validation.
//! Survival records are (entry, exit] intervals; zero means right censoring, positive causes
//! mean mutually exclusive terminal events. These are not interval-censored observations.

use super::optional_num as opt_num;

use crate::special::{ln_beta, ln_gamma, norm_inv, reg_gamma_p, reg_gamma_q, reg_inc_beta};
use crate::util::{R, arg, array_val, as_array, has, num, num_val};
use crate::{Arg, Ctx, FnSpec};
use gridcraft_core::{CellError, Value};

const MAX_RECORDS: usize = 4096;
const MAX_CAUSES: usize = 16;
const MAX_COX: usize = 256;
const MAX_P: usize = 8;

/// Returns a finite scalar, rejecting NaN and infinity with `#NUM!`.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}
/// Reads a scalar worksheet argument and rejects nonfinite values.
fn scalar(a: &[Arg], i: usize) -> R<f64> {
    finite(num(a, i)?)
}
/// Validates a strictly positive finite parameter; zero and nonfinite values are undefined.
fn positive(x: f64) -> R<f64> {
    if x.is_finite() && x > 0.0 { Ok(x) } else { Err(CellError::Num) }
}
/// Validates an exact nonnegative integer within the supplied resource bound.
fn integer(x: f64, max: usize) -> R<usize> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x <= max as f64 { Ok(x as usize) } else { Err(CellError::Num) }
}
/// Reads a numeric cell without coercing blanks, booleans or text; propagates cell errors.
fn strict(v: &Value) -> R<f64> {
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
/// Reads a bounded numeric row/column vector while preserving observation alignment.
fn vector(a: &[Arg], i: usize, cap: usize) -> R<Vec<f64>> {
    let arr = as_array(&arg(a, i)?.value);
    if arr.data.is_empty() || arr.data.len() > cap || (arr.rows > 1 && arr.cols > 1) {
        return Err(CellError::Value);
    }
    arr.iter().map(strict).collect()
}
/// Reads an optional aligned vector, or supplies a constant default without dropping cells.
fn aligned(a: &[Arg], i: usize, n: usize, default: f64) -> R<Vec<f64>> {
    if !has(a, i) {
        return Ok(vec![default; n]);
    }
    let v = vector(a, i, MAX_RECORDS)?;
    if v.len() != n { Err(CellError::Value) } else { Ok(v) }
}
/// Builds a finite numeric spill with the supplied row/column layout.
fn spill(rows: usize, cols: usize, values: Vec<f64>) -> R<Value> {
    array_val(rows, cols, values.into_iter().map(num_val).collect::<R<_>>()?)
}

#[derive(Clone, Copy)]
struct Record {
    entry: f64,
    time: f64,
    cause: usize,
}
/// Parses and orders survival records on `(entry, exit]` intervals.
/// Entry must precede positive exit time; cause zero denotes right censoring.
fn records(a: &[Arg], entry_index: usize, max_cause: usize, cap: usize) -> R<Vec<Record>> {
    let times = vector(a, 0, cap)?;
    let causes = vector(a, 1, cap)?;
    if times.len() != causes.len() {
        return Err(CellError::Value);
    }
    let entry = aligned(a, entry_index, times.len(), 0.0)?;
    let mut out = Vec::with_capacity(times.len());
    for ((time, cause), entry) in times.into_iter().zip(causes).zip(entry) {
        if time <= 0.0 || entry < 0.0 || entry >= time {
            return Err(CellError::Num);
        }
        out.push(Record { entry, time, cause: integer(cause, max_cause)? });
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(out)
}
/// Returns ordered distinct exit times from already time-sorted survival records.
fn unique_times(records: &[Record]) -> Vec<f64> {
    let mut out: Vec<f64> = records.iter().map(|r| r.time).collect();
    out.dedup_by(|a, b| *a == *b);
    out
}
/// Validates an optional confidence level strictly between zero and one.
fn confidence(a: &[Arg], i: usize) -> R<f64> {
    let c = finite(opt_num(a, i, 0.95)?)?;
    if !(0.0..1.0).contains(&c) || c == 0.0 { Err(CellError::Num) } else { Ok(c) }
}

/// Estimates product-limit survival with delayed entry and right censoring.
/// Rows contain time, risk, deaths, censors, survival, plug-in Greenwood SE and log-log bounds;
/// the two confidence-bound cells are blank at survival zero or one.
fn kaplan_meier(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let records = records(a, 2, 1, MAX_RECORDS)?;
    let times = unique_times(&records);
    let z = -norm_inv(credible_tail(confidence(a, 3)?)?);
    let (mut survival, mut greenwood) = (1.0_f64, 0.0_f64);
    let mut out = Vec::with_capacity(times.len() * 8);
    for time in &times {
        let risk = records.iter().filter(|r| r.entry < *time && r.time >= *time).count() as f64;
        let deaths = records.iter().filter(|r| r.time == *time && r.cause == 1).count() as f64;
        let censored = records.iter().filter(|r| r.time == *time && r.cause == 0).count() as f64;
        if risk <= 0.0 || deaths > risk {
            return Err(CellError::Num);
        }
        if deaths > 0.0 {
            if deaths == risk {
                survival = 0.0;
            } else {
                survival *= 1.0 - deaths / risk;
                greenwood += deaths / (risk * (risk - deaths));
            }
        }
        let se = if survival == 0.0 { 0.0 } else { survival * greenwood.sqrt() };
        out.extend([*time, risk, deaths, censored, survival, se].into_iter().map(num_val).collect::<R<Vec<_>>>()?);
        // The log-log transform is undefined at either endpoint. Keep the point estimate and
        // plug-in SE, but leave both confidence cells blank instead of inventing certainty.
        if survival == 0.0 || survival == 1.0 {
            out.extend([Value::Empty, Value::Empty]);
        } else {
            let theta = (-survival.ln()).ln();
            let s = greenwood.sqrt() / survival.ln().abs();
            out.push(num_val((-(theta + z * s).exp()).exp())?);
            out.push(num_val((-(theta - z * s).exp()).exp())?);
        }
    }
    array_val(times.len(), 8, out)
}

/// Estimates competing-risk incidence using pre-event all-cause survival.
/// Rows contain time, risk, events, censors, survival and one CIF per terminal cause.
fn aalen_johansen(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let records = records(a, 2, MAX_CAUSES, MAX_RECORDS)?;
    let times = unique_times(&records);
    let causes = records.iter().map(|r| r.cause).max().ok_or(CellError::Num)?;
    if causes == 0 {
        return Err(CellError::Num);
    }
    let mut incidence = vec![0.0; causes];
    let mut survival = 1.0;
    let cols = 5 + causes;
    let mut out = Vec::with_capacity(times.len() * cols);
    for time in &times {
        let risk = records.iter().filter(|r| r.entry < *time && r.time >= *time).count() as f64;
        if risk <= 0.0 {
            return Err(CellError::Num);
        }
        let mut counts = vec![0.0; causes];
        let mut censored = 0.0;
        for r in records.iter().filter(|r| r.time == *time) {
            if r.cause == 0 {
                censored += 1.0;
            } else {
                *counts.get_mut(r.cause - 1).ok_or(CellError::Num)? += 1.0;
            }
        }
        let deaths = sum(counts.iter().copied())?;
        for (cif, count) in incidence.iter_mut().zip(counts) {
            *cif += survival * count / risk;
        }
        survival *= 1.0 - deaths / risk;
        out.extend([*time, risk, deaths, censored, survival]);
        out.extend_from_slice(&incidence);
    }
    spill(times.len(), cols, out)
}

/// Product integral from full-state aggregated observed risk sets and off-diagonal events.
/// Counts at time k occupy rows k*n..(k+1)*n and n columns. No hidden row normalization.
fn aalen_johansen_matrix(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let risks = as_array(&arg(a, 0)?.value);
    let counts = as_array(&arg(a, 1)?.value);
    let (steps, n) = (risks.rows, risks.cols);
    if steps == 0 || steps > MAX_RECORDS || n == 0 || n > MAX_CAUSES || counts.rows != steps * n || counts.cols != n {
        return Err(CellError::Value);
    }
    let risks: Vec<f64> = risks.iter().map(strict).collect::<R<_>>()?;
    let counts: Vec<f64> = counts.iter().map(strict).collect::<R<_>>()?;
    let mut probability = if has(a, 2) {
        vector(a, 2, MAX_CAUSES)?
    } else {
        let mut p = vec![0.0; n];
        p[0] = 1.0;
        p
    };
    if probability.len() != n || probability.iter().any(|p| *p < 0.0) || (sum(probability.iter().copied())? - 1.0).abs() > 1e-12 {
        return Err(CellError::Num);
    }
    let mut out = Vec::with_capacity(steps * (n + 1));
    for step in 0..steps {
        let mut next = vec![0.0; n];
        for (i, initial_mass) in probability.iter().enumerate() {
            let risk = *risks.get(step * n + i).ok_or(CellError::Value)?;
            if risk < 0.0 {
                return Err(CellError::Num);
            }
            let row = counts.get((step * n + i) * n..(step * n + i + 1) * n).ok_or(CellError::Value)?;
            if row.iter().any(|v| *v < 0.0 || v.fract() != 0.0) || row.get(i).copied() != Some(0.0) {
                return Err(CellError::Num);
            }
            let events = sum(row.iter().copied())?;
            if events > risk {
                return Err(CellError::Num);
            }
            for (j, value) in next.iter_mut().enumerate() {
                let transition = if i == j {
                    if risk == 0.0 { 1.0 } else { 1.0 - events / risk }
                } else if risk == 0.0 {
                    0.0
                } else {
                    row[j] / risk
                };
                *value += initial_mass * transition;
            }
        }
        probability = next;
        if (sum(probability.iter().copied())? - 1.0).abs() > 1e-9 {
            return Err(CellError::Num);
        }
        out.push((step + 1) as f64);
        out.extend_from_slice(&probability);
    }
    spill(steps, n + 1, out)
}

const TAIL_RELATIVE_TOLERANCE: f64 = 2e-9;

/// Checks a direct small-tail probability against the relative inversion tolerance.
fn tail_matches(actual: f64, target: f64) -> bool {
    actual.is_finite() && (0.0..=1.0).contains(&actual) && (actual - target).abs() <= TAIL_RELATIVE_TOLERANCE * target
}

/// Bisection in log(x), with a true probability bracket and a relative small-tail criterion.
/// The probability evaluator is supplied in its numerically appropriate tail. In particular
/// the upper Gamma endpoint never constructs 1-tail, which can round to one near confidence=1.
fn tail_root(probability: impl Fn(f64) -> f64, target: f64, upper: f64, increasing: bool) -> R<f64> {
    if !(0.0..=0.5).contains(&target) || target == 0.0 || !upper.is_finite() || upper <= 0.0 {
        return Err(CellError::Num);
    }
    let mut lo = f64::from_bits(1).ln();
    let mut hi = upper.ln();
    let low_probability = probability(lo.exp());
    let high_probability = probability(upper);
    if !low_probability.is_finite()
        || !high_probability.is_finite()
        || (increasing && (low_probability > target || high_probability < target))
        || (!increasing && (low_probability < target || high_probability > target))
    {
        return Err(CellError::Num);
    }
    for _ in 0..128 {
        let mid = lo + (hi - lo) * 0.5;
        if mid <= lo || mid >= hi {
            break;
        }
        let x = mid.exp();
        let p = probability(x);
        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
            return Err(CellError::Num);
        }
        if tail_matches(p, target) {
            return positive(x);
        }
        if (p < target) == increasing {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    for endpoint in [lo, hi] {
        let x = endpoint.exp();
        if tail_matches(probability(x), target) {
            return positive(x);
        }
    }
    Err(CellError::Num)
}

/// Inverts the lower Gamma P tail or upper complementary Q tail directly.
/// Bracketing is bounded; unresolved or unrepresentable quantiles return `#NUM!`.
fn gamma_quantile(shape: f64, tail: f64, upper: bool) -> R<f64> {
    // For an upper small tail, Q's direct continued fraction is used beyond shape+1.
    // Central-region subtraction is retained by that existing helper; sufficiently extreme
    // upper quantiles lie beyond shape+1 and use its direct continued fraction.
    let probability = |x| if upper { reg_gamma_q(shape, x) } else { reg_gamma_p(shape, x) };
    let mut bound = shape.max(1.0);
    for _ in 0..80 {
        let p = probability(bound);
        if !p.is_finite() {
            return Err(CellError::Num);
        }
        if (upper && p <= tail) || (!upper && p >= tail) {
            return tail_root(probability, tail, bound, !upper);
        }
        bound = finite(bound * 2.0)?;
        if bound > 1e12 {
            break;
        }
    }
    Err(CellError::Num)
}

/// Inverts a lower Beta tail with a checked probability bracket and relative residual.
fn beta_quantile(alpha: f64, beta: f64, tail: f64) -> R<f64> {
    let q = tail_root(|x| reg_inc_beta(x, alpha, beta), tail, 1.0, true)?;
    if q >= 1.0 { Err(CellError::Num) } else { Ok(q) }
}
/// Computes a representable equal-tail probability from a confidence level.
/// Refuses rounded endpoints rather than manufacturing a credible interval.
fn credible_tail(confidence: f64) -> R<f64> {
    let tail = (1.0 - confidence) * 0.5;
    if tail > 0.0 && tail < 0.5 { Ok(tail) } else { Err(CellError::Num) }
}
/// Updates Gamma shape/rate from aligned Poisson counts and exposure.
/// Counts are integral; positive counts at zero exposure and unsupported posterior shapes fail.
fn poisson_posterior(a: &[Arg]) -> R<(f64, f64)> {
    let counts = vector(a, 0, MAX_RECORDS)?;
    let exposure = vector(a, 1, MAX_RECORDS)?;
    if counts.len() != exposure.len() {
        return Err(CellError::Value);
    }
    for (y, t) in counts.iter().zip(&exposure) {
        integer(*y, 1_000_000)?;
        if *t < 0.0 || (*t == 0.0 && *y > 0.0) {
            return Err(CellError::Num);
        }
    }
    let shape = positive(scalar(a, 2)?)? + sum(counts)?;
    let rate = positive(scalar(a, 3)?)? + sum(exposure)?;
    if !(0.001..=1e6).contains(&shape) {
        return Err(CellError::Num);
    }
    Ok((finite(shape)?, positive(rate)?))
}
/// Updates Beta parameters from aligned binomial successes and trial counts.
/// Requires integral counts with successes no greater than trials and bounded posterior shapes.
fn beta_posterior(a: &[Arg]) -> R<(f64, f64)> {
    let successes = vector(a, 0, MAX_RECORDS)?;
    let trials = vector(a, 1, MAX_RECORDS)?;
    if successes.len() != trials.len() {
        return Err(CellError::Value);
    }
    for (s, n) in successes.iter().zip(&trials) {
        integer(*s, 1_000_000)?;
        integer(*n, 1_000_000)?;
        if s > n {
            return Err(CellError::Num);
        }
    }
    let alpha = positive(scalar(a, 2)?)? + sum(successes.iter().copied())?;
    let beta = positive(scalar(a, 3)?)? + sum(successes.iter().zip(trials).map(|(s, n)| n - s))?;
    if !(0.001..=1e6).contains(&alpha) || !(0.001..=1e6).contains(&beta) {
        return Err(CellError::Num);
    }
    Ok((alpha, beta))
}
/// Returns Gamma posterior parameters, moments and credible bounds, then predictive count moments.
/// Predictive variance includes uncertainty in the Poisson intensity; parameters use shape/rate.
fn bayes_poisson(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (shape, rate) = poisson_posterior(a)?;
    let confidence = confidence(a, 4)?;
    let future = if has(a, 5) { scalar(a, 5)? } else { 1.0 };
    if future < 0.0 {
        return Err(CellError::Num);
    }
    let mean = shape / rate;
    let variance = mean / rate;
    let tail = credible_tail(confidence)?;
    let pm = mean * future;
    let pv = pm + variance * future * future;
    let lower = positive(gamma_quantile(shape, tail, false)? / rate)?;
    let upper = positive(gamma_quantile(shape, tail, true)? / rate)?;
    if lower >= upper || !tail_matches(reg_gamma_p(shape, lower * rate), tail) || !tail_matches(reg_gamma_q(shape, upper * rate), tail) {
        return Err(CellError::Num);
    }
    positive(variance)?;
    spill(1, 8, vec![shape, rate, mean, variance, lower, upper, pm, pv])
}
/// Returns Beta posterior parameters, moments and credible bounds, then beta-binomial moments.
/// The predictive variance integrates parameter uncertainty instead of plugging in a fixed probability.
fn bayes_binomial(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (alpha, beta) = beta_posterior(a)?;
    let confidence = confidence(a, 4)?;
    let total = alpha + beta;
    let future = if has(a, 5) { integer(scalar(a, 5)?, 1_000_000)? } else { 1 } as f64;
    let mean = alpha / total;
    let variance = mean * (1.0 - mean) / (total + 1.0);
    let tail = credible_tail(confidence)?;
    let lower = beta_quantile(alpha, beta, tail)?;
    let upper = 1.0 - beta_quantile(beta, alpha, tail)?;
    // Even when the small-tail root is accurately representable, 1-y can round to 1 or
    // lose most tail precision. Validate the actual returned endpoint, not the hidden y.
    if upper >= 1.0 || lower >= upper || !tail_matches(reg_inc_beta(1.0 - upper, beta, alpha), tail) {
        return Err(CellError::Num);
    }
    spill(1, 8, vec![alpha, beta, mean, variance, lower, upper, future * mean, future * mean * (1.0 - mean) * (total + future) / (total + 1.0)])
}
/// Reads an optional exact zero/one flag selecting a predictive PMF or CDF.
fn cumulative(a: &[Arg], i: usize) -> R<bool> {
    if !has(a, i) {
        Ok(false)
    } else {
        match scalar(a, i)? {
            0.0 => Ok(false),
            1.0 => Ok(true),
            _ => Err(CellError::Num),
        }
    }
}
/// Evaluates the Gamma-Poisson predictive PMF or CDF for a future exposure.
/// Zero exposure is degenerate at count zero; unsupported numerical domains return errors.
fn bayes_poisson_predict(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (shape, rate) = poisson_posterior(a)?;
    let k = integer(scalar(a, 4)?, 1_000_000)?;
    let future = if has(a, 5) { scalar(a, 5)? } else { 1.0 };
    if future < 0.0 {
        return Err(CellError::Num);
    }
    let cdf = cumulative(a, 6)?;
    if future == 0.0 {
        return num_val(if cdf || k == 0 { 1.0 } else { 0.0 });
    }
    let p = rate / finite(rate + future)?;
    if !(0.0..1.0).contains(&p) || p == 0.0 {
        return Err(CellError::Num);
    }
    let result = if cdf {
        reg_inc_beta(p, shape, k as f64 + 1.0)
    } else {
        (ln_gamma(shape + k as f64) - ln_gamma(shape) - ln_gamma(k as f64 + 1.0) + shape * p.ln() + k as f64 * (-p).ln_1p()).exp()
    };
    num_val(result)
}
/// Evaluates a beta-binomial predictive PMF or bounded cumulative probability.
/// Future trials and successes are integral; posterior uncertainty is retained.
fn bayes_binomial_predict(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (alpha, beta) = beta_posterior(a)?;
    let n = integer(scalar(a, 5)?, 4096)?;
    let k = integer(scalar(a, 4)?, n)?;
    let pmf = |k: usize| {
        (ln_gamma(n as f64 + 1.0) - ln_gamma(k as f64 + 1.0) - ln_gamma((n - k) as f64 + 1.0) + ln_beta(alpha + k as f64, beta + (n - k) as f64)
            - ln_beta(alpha, beta))
        .exp()
    };
    num_val(if cumulative(a, 6)? { sum((0..=k).map(pmf))?.min(1.0) } else { pmf(k) })
}

/// Cox proportional-hazard partial likelihood, Breslow tied events. No intercept is added.
struct CoxData {
    records: Vec<Record>,
    x: Vec<f64>,
    p: usize,
    times: Vec<f64>,
    events: usize,
}
/// Validates aligned Cox exit/event/design data and optional delayed entries.
/// Keeps design rows aligned, records distinct failure times and rejects samples without events.
fn cox_data(a: &[Arg], entry_index: usize) -> R<CoxData> {
    // Preserve row alignment before any time ordering; risk evaluation does not require sorting.
    let times = vector(a, 0, MAX_COX)?;
    let event = vector(a, 1, MAX_COX)?;
    let design = as_array(&arg(a, 2)?.value);
    if times.len() != event.len() || design.rows != times.len() || design.cols == 0 || design.cols > MAX_P || times.len() <= design.cols {
        return Err(CellError::Value);
    }
    let entry = aligned(a, entry_index, times.len(), 0.0)?;
    let mut records = Vec::with_capacity(times.len());
    let mut events = 0;
    for ((time, event), entry) in times.into_iter().zip(event).zip(entry) {
        if time <= 0.0 || entry < 0.0 || entry >= time {
            return Err(CellError::Num);
        }
        let cause = integer(event, 1)?;
        events += cause;
        records.push(Record { entry, time, cause });
    }
    if events == 0 {
        return Err(CellError::Num);
    }
    let mut times: Vec<f64> = records.iter().filter(|r| r.cause == 1).map(|r| r.time).collect();
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| *a == *b);
    Ok(CoxData { records, x: design.iter().map(strict).collect::<R<_>>()?, p: design.cols, times, events })
}
struct CoxStats {
    likelihood: f64,
    score: Vec<f64>,
    information: Vec<Vec<f64>>,
    baseline: Vec<(f64, f64)>,
}
/// Computes Breslow partial log-likelihood, score, observed information and baseline increments.
/// Risk sums are stabilized by subtracting their maximum linear predictor.
fn cox_stats(data: &CoxData, beta: &[f64]) -> R<CoxStats> {
    let p = data.p;
    let eta: Vec<f64> = data.x.chunks_exact(p).map(|row| sum(row.iter().zip(beta).map(|(x, b)| x * b))).collect::<R<_>>()?;
    let mut likelihood = 0.0;
    let mut score = vec![0.0; p];
    let mut information = vec![vec![0.0; p]; p];
    let mut baseline = Vec::with_capacity(data.times.len());
    for &time in &data.times {
        let risk: Vec<usize> = data.records.iter().enumerate().filter(|(_, r)| r.entry < time && r.time >= time).map(|(i, _)| i).collect();
        let deaths: Vec<usize> = data.records.iter().enumerate().filter(|(_, r)| r.time == time && r.cause == 1).map(|(i, _)| i).collect();
        let d = deaths.len() as f64;
        let maximum = risk.iter().map(|i| eta[*i]).fold(f64::NEG_INFINITY, f64::max);
        let mut s0 = 0.0;
        let mut s1 = vec![0.0; p];
        let mut s2 = vec![vec![0.0; p]; p];
        for &i in &risk {
            let weight = (eta[i] - maximum).exp();
            s0 += weight;
            let row = data.x.get(i * p..(i + 1) * p).ok_or(CellError::Value)?;
            for j in 0..p {
                s1[j] += weight * row[j];
                for k in 0..p {
                    s2[j][k] += weight * row[j] * row[k];
                }
            }
        }
        positive(s0)?;
        let log_denominator = finite(maximum + s0.ln())?;
        likelihood = finite(likelihood + sum(deaths.iter().map(|i| eta[*i]))? - d * log_denominator)?;
        for j in 0..p {
            score[j] = finite(score[j] + sum(deaths.iter().map(|i| data.x[*i * p + j]))? - d * s1[j] / s0)?;
            for k in 0..p {
                information[j][k] = finite(information[j][k] + d * (s2[j][k] / s0 - (s1[j] / s0) * (s1[k] / s0)))?;
            }
        }
        baseline.push((time, finite((d.ln() - log_denominator).exp())?));
    }
    Ok(CoxStats { likelihood, score, information, baseline })
}
/// Diagonal-scaled Cholesky. No ridge: a nonpositive/ill-conditioned Fisher direction is
/// unavailable, not repaired. Scaling avoids a covariate's measurement unit deciding rank.
fn solve(matrix: Vec<Vec<f64>>, rhs: Vec<f64>) -> R<Vec<f64>> {
    let n = rhs.len();
    let scales = (0..n).map(|i| positive(matrix[i][i]).map(f64::sqrt)).collect::<R<Vec<_>>>()?;
    let mut lower = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let value = finite(matrix[i][j] / scales[i] / scales[j] - sum((0..j).map(|k| lower[i][k] * lower[j][k]))?)?;
            lower[i][j] = if i == j {
                if value <= 1e-12 {
                    return Err(CellError::Num);
                }
                value.sqrt()
            } else {
                finite(value / lower[j][j])?
            };
        }
    }
    let mut intermediate = vec![0.0; n];
    for i in 0..n {
        intermediate[i] = finite((rhs[i] / scales[i] - sum((0..i).map(|j| lower[i][j] * intermediate[j]))?) / lower[i][i])?;
    }
    let mut result = vec![0.0; n];
    for i in (0..n).rev() {
        result[i] = finite((intermediate[i] - sum((i + 1..n).map(|j| lower[j][i] * result[j]))?) / lower[i][i])?;
    }
    result.into_iter().zip(scales).map(|(r, scale)| finite(r / scale)).collect()
}
/// Fits Cox coefficients by bounded Newton updates with step halving.
/// No intercept, hidden ridge or dropped covariate is introduced; singular/nonconverged fits fail.
fn cox_fit(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    let data = cox_data(a, 3)?;
    let mut beta = vec![0.0; data.p];
    let mut stats = cox_stats(&data, &beta)?;
    for _ in 0..60 {
        super::work::charge(ctx, super::work::product(&[data.p as u64, data.p as u64, data.p as u64, 32]))?;
        let delta = solve(stats.information.clone(), stats.score.clone())?;
        let change = delta.iter().zip(&beta).map(|(d, b)| d.abs() / (1.0 + b.abs())).fold(0.0_f64, f64::max);
        let score = stats.score.iter().map(|s| s.abs()).fold(0.0_f64, f64::max);
        if change < 1e-8 && score < 1e-7 * (1.0 + data.events as f64) {
            return spill(data.p, 1, beta);
        }
        let mut accepted = None;
        let mut step = 1.0;
        for _ in 0..30 {
            super::work::charge(
                ctx,
                super::work::product(&[data.records.len() as u64, data.records.len() as u64, data.p as u64, data.p as u64, 32]),
            )?;
            let candidate: Vec<f64> = beta.iter().zip(&delta).map(|(b, d)| b + step * d).collect();
            if let Ok(next) = cox_stats(&data, &candidate)
                && next.likelihood >= stats.likelihood - 1e-12 * (1.0 + stats.likelihood.abs())
            {
                accepted = Some((candidate, next));
                break;
            }
            step *= 0.5;
        }
        (beta, stats) = accepted.ok_or(CellError::Num)?;
    }
    Err(CellError::Num)
}
/// Returns Breslow event-time, baseline hazard increment and cumulative hazard rows.
/// Uses supplied coefficients without silently refitting them.
fn cox_baseline(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let data = cox_data(a, 4)?;
    let beta = vector(a, 3, MAX_P)?;
    if beta.len() != data.p {
        return Err(CellError::Value);
    }
    let stats = cox_stats(&data, &beta)?;
    let mut total = 0.0;
    let mut out = Vec::with_capacity(stats.baseline.len() * 3);
    for (time, hazard) in &stats.baseline {
        total = finite(total + hazard)?;
        out.extend([*time, *hazard, total]);
    }
    spill(stats.baseline.len(), 3, out)
}

/// Returns the inverse observed Cox information at supplied coefficients.
/// This is model-based coefficient covariance, not a robust sandwich estimate or a refit.
fn cox_covariance(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let data = cox_data(a, 4)?;
    let beta = vector(a, 3, MAX_P)?;
    if beta.len() != data.p {
        return Err(CellError::Value);
    }
    let information = cox_stats(&data, &beta)?.information;
    let mut covariance = vec![vec![0.0; data.p]; data.p];
    for j in 0..data.p {
        let mut basis = vec![0.0; data.p];
        basis[j] = 1.0;
        let column = solve(information.clone(), basis)?;
        for (row, x) in covariance.iter_mut().zip(column) {
            row[j] = x;
        }
    }
    spill(data.p, data.p, covariance.into_iter().flatten().collect())
}

/// Predicts subject-by-baseline-time survival as `exp(-H0 * exp(X beta))`.
/// Uses bounded output dimensions and log-hazard evaluation for extreme linear predictors.
fn cox_survival(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let design = as_array(&arg(a, 0)?.value);
    if design.rows == 0 || design.rows > MAX_RECORDS || design.cols == 0 || design.cols > MAX_P {
        return Err(CellError::Value);
    }
    let x: Vec<f64> = design.iter().map(strict).collect::<R<_>>()?;
    let beta = vector(a, 1, MAX_P)?;
    let baseline = vector(a, 2, MAX_RECORDS)?;
    if beta.len() != design.cols || design.rows.saturating_mul(baseline.len()) > 65_536 {
        return Err(CellError::Value);
    }
    if baseline.iter().any(|h| *h < 0.0) || baseline.windows(2).any(|h| h[1] < h[0]) {
        return Err(CellError::Num);
    }
    let mut out = Vec::with_capacity(design.rows * baseline.len());
    for row in x.chunks_exact(design.cols) {
        let eta = sum(row.iter().zip(&beta).map(|(x, b)| x * b))?;
        for h in &baseline {
            out.push(if *h == 0.0 { 1.0 } else { (-(h.ln() + eta).exp()).exp() });
        }
    }
    spill(design.rows, baseline.len(), out)
}

/// Summarizes supplied held-out observations and predictions with weighted calibration errors.
/// Returns sample count, observed/predicted totals, O:E, bias, MAE, RMSE and Poisson deviance;
/// the caller remains responsible for a genuinely independent validation sample.
fn calibration(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let observed = vector(a, 0, MAX_RECORDS)?;
    let predicted = vector(a, 1, MAX_RECORDS)?;
    if observed.len() != predicted.len() {
        return Err(CellError::Value);
    }
    let weights = aligned(a, 2, observed.len(), 1.0)?;
    if observed.iter().any(|x| *x < 0.0) || predicted.iter().any(|x| *x <= 0.0) || weights.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    let w = positive(sum(weights.iter().copied())?)?;
    let obs = sum(observed.iter().zip(&weights).map(|(y, w)| y * w))?;
    let pred = positive(sum(predicted.iter().zip(&weights).map(|(y, w)| y * w))?)?;
    let error = sum(observed.iter().zip(&predicted).zip(&weights).map(|((y, m), w)| w * (m - y)))? / w;
    let mae = sum(observed.iter().zip(&predicted).zip(&weights).map(|((y, m), w)| w * (m - y).abs()))? / w;
    let mse = sum(observed.iter().zip(&predicted).zip(&weights).map(|((y, m), w)| w * (m - y) * (m - y)))? / w;
    let deviance =
        sum(observed.iter().zip(&predicted).zip(&weights).map(|((y, m), w)| 2.0 * w * if *y == 0.0 { *m } else { y * (y.ln() - m.ln()) - (y - m) }))?
            .max(0.0);
    spill(1, 8, vec![observed.len() as f64, obs, pred, obs / pred, error, mae, mse.sqrt(), deviance])
}

/// Computes the right-censored IPCW Brier score at a fixed horizon.
/// Reverse-KM orders tied failures before censoring; cases use `G(T-)`, controls use `G(h)`,
/// and lost censoring support returns an error instead of an artificially low score.
fn brier(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let times = vector(a, 0, MAX_RECORDS)?;
    let event = vector(a, 1, MAX_RECORDS)?;
    let predicted = vector(a, 2, MAX_RECORDS)?;
    if times.len() != event.len() || times.len() != predicted.len() {
        return Err(CellError::Value);
    }
    let horizon = scalar(a, 3)?;
    if horizon < 0.0 {
        return Err(CellError::Num);
    }
    let mut records = Vec::with_capacity(times.len());
    for (&time, &event) in times.iter().zip(&event) {
        if time <= 0.0 {
            return Err(CellError::Num);
        }
        records.push(Record { entry: 0.0, time, cause: integer(event, 1)? });
    }
    if predicted.iter().any(|p| !(0.0..=1.0).contains(p)) {
        return Err(CellError::Num);
    }
    let mut grid = times.clone();
    grid.sort_by(f64::total_cmp);
    grid.dedup_by(|a, b| *a == *b);
    let mut g = 1.0;
    let mut estimates = Vec::with_capacity(grid.len());
    for time in grid {
        let risk = records.iter().filter(|r| r.time >= time).count() as f64;
        let deaths = records.iter().filter(|r| r.time == time && r.cause == 1).count() as f64;
        let censor = records.iter().filter(|r| r.time == time && r.cause == 0).count() as f64;
        let before = g;
        // Delta=I(T<=C): tied failures precede losses to follow-up. The reverse-KM
        // censor hazard is m/(Y-d), not m/Y (Satten–Datta, tied-event product identity).
        // With no censor at this time there is no increment, including an all-event tie
        // where Y-d=0; avoid turning that legitimate case into 0/0.
        if censor > 0.0 {
            g *= 1.0 - censor / positive(risk - deaths)?;
        }
        estimates.push((time, before, g));
    }
    let gh = estimates.iter().rfind(|(t, _, _)| *t <= horizon).map_or(1.0, |(_, _, g)| *g);
    // Positivity is needed even when no observed control remains after this horizon.
    if gh <= 0.0 {
        return Err(CellError::Num);
    }
    let score = sum(times.iter().zip(event).zip(predicted).map(|((time, event), s)| {
        if *time <= horizon && event == 1.0 {
            let gt = estimates.iter().find(|(t, _, _)| t == time).map_or(f64::NAN, |(_, g, _)| *g);
            if gt <= 0.0 { f64::NAN } else { s * s / gt }
        } else if *time > horizon {
            if gh <= 0.0 { f64::NAN } else { (1.0 - s) * (1.0 - s) / gh }
        } else {
            0.0
        }
    }))?;
    num_val(score / times.len() as f64)
}

/// Registers deterministic survival, conjugate inference, Cox and validation worksheet contracts.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.KAPLANMEIER",
            2,
            4,
            Statistical,
            &[false, false, false, true],
            "ACT.KAPLANMEIER(times,event,[entry],[confidence])",
            "Censored/delayed-entry survival: time/risk/events/censors/S/plug-in Greenwood SE/log-log bounds (blank at S=0/1).",
            kaplan_meier
        ),
        act!(
            "ACT.AALENJOHANSEN",
            2,
            3,
            Statistical,
            &[false, false, false],
            "ACT.AALENJOHANSEN(times,cause,[entry])",
            "Competing-risk product integral: time/risk/events/censors/survival/one CIF per positive cause.",
            aalen_johansen
        ),
        act!(
            "ACT.AALENJOHANSEN.MATRIX",
            2,
            3,
            Statistical,
            &[false, false, false],
            "ACT.AALENJOHANSEN.MATRIX(risk_sets,transition_counts,[initial])",
            "Multi-state occupation product integral from ordered risk sets and stacked off-diagonal event counts.",
            aalen_johansen_matrix
        ),
        act!(
            "ACT.BAYES.POISSON",
            4,
            6,
            Statistical,
            &[false, false, true, true, true, true],
            "ACT.BAYES.POISSON(counts,exposures,prior_shape,prior_rate,[confidence],[future_exposure])",
            "Gamma posterior shape/rate/mean/variance/credible bounds and predictive count mean/variance.",
            bayes_poisson
        ),
        act!(
            "ACT.BAYES.BINOMIAL",
            4,
            6,
            Statistical,
            &[false, false, true, true, true, true],
            "ACT.BAYES.BINOMIAL(successes,trials,prior_alpha,prior_beta,[confidence],[future_trials])",
            "Beta posterior alpha/beta/mean/variance/credible bounds and beta-binomial predictive moments.",
            bayes_binomial
        ),
        act!(
            "ACT.BAYES.POISSON.PREDICT",
            5,
            7,
            Statistical,
            &[false, false, true, true, true, true, true],
            "ACT.BAYES.POISSON.PREDICT(counts,exposures,prior_shape,prior_rate,future_count,[future_exposure],[cumulative])",
            "Exact Gamma-Poisson posterior predictive count PMF or CDF.",
            bayes_poisson_predict
        ),
        act!(
            "ACT.BAYES.BINOMIAL.PREDICT",
            6,
            7,
            Statistical,
            &[false, false, true, true, true, true, true],
            "ACT.BAYES.BINOMIAL.PREDICT(successes,trials,prior_alpha,prior_beta,future_successes,future_trials,[cumulative])",
            "Exact beta-binomial posterior predictive count PMF or CDF.",
            bayes_binomial_predict
        ),
        act!(
            "ACT.COX.FIT",
            3,
            4,
            Statistical,
            &[false, false, false, false],
            "ACT.COX.FIT(times,event,x,[entry])",
            "Breslow-tie Cox partial-likelihood coefficients; no intercept; singular/nonconvergent fits rejected.",
            cox_fit
        ),
        act!(
            "ACT.COX.BASELINE",
            4,
            5,
            Statistical,
            &[false, false, false, false, false],
            "ACT.COX.BASELINE(times,event,x,coefficients,[entry])",
            "Breslow baseline: event time, hazard increment, cumulative hazard.",
            cox_baseline
        ),
        act!(
            "ACT.COX.COVARIANCE",
            4,
            5,
            Statistical,
            &[false, false, false, false, false],
            "ACT.COX.COVARIANCE(times,event,x,coefficients,[entry])",
            "Inverse observed Cox partial-likelihood information, conditional on supplied coefficients.",
            cox_covariance
        ),
        act!(
            "ACT.COX.SURVIVAL",
            3,
            3,
            Statistical,
            &[false, false, false],
            "ACT.COX.SURVIVAL(x,coefficients,baseline_cumulative_hazard)",
            "Cox survival: subject rows by supplied ordered cumulative baseline hazards.",
            cox_survival
        ),
        act!(
            "ACT.CALIBRATION",
            2,
            3,
            Statistical,
            &[false, false, false],
            "ACT.CALIBRATION(observed,predicted,[weights])",
            "Holdout count calibration: n/observed/predicted/O:E/bias/MAE/RMSE/Poisson deviance.",
            calibration
        ),
        act!(
            "ACT.BRIER.IPCW",
            4,
            4,
            Statistical,
            &[false, false, false, true],
            "ACT.BRIER.IPCW(times,event,predicted_survival,horizon)",
            "Right-censored survival Brier score using reverse-KM IPCW; independent censoring assumed.",
            brier
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;
    fn vec_arg(values: &[f64]) -> Arg {
        av(arr(values.iter().map(|x| vec![nv(*x)]).collect()))
    }
    fn mat(values: &[&[f64]]) -> Arg {
        av(arr(values.iter().map(|r| r.iter().map(|x| nv(*x)).collect()).collect()))
    }
    fn data(value: Value) -> Vec<f64> {
        match value {
            Value::Array(a) => a.iter().map(|v| strict(v).unwrap()).collect(),
            Value::Number(x) => vec![x],
            other => panic!("{other:?}"),
        }
    }
    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-8 * (1.0 + expected.abs()), "{actual} != {expected}");
    }
    #[test]
    fn km_ties_greenwood_and_terminal_endpoints() {
        let Value::Array(d) = ev("ACT.KAPLANMEIER", vec![vec_arg(&[1.0, 2.0, 2.0, 3.0]), vec_arg(&[1.0, 1.0, 0.0, 1.0])]) else {
            panic!("expected spill")
        };
        let n = |i: usize| strict(&d.data[i]).unwrap();
        close(n(4), 0.75);
        close(n(5), (0.75_f64 * 0.75 / 12.0).sqrt());
        close(n(12), 0.5);
        close(n(20), 0.0);
        assert!(n(6) < n(4) && n(7) > n(4));
        assert_eq!(d.data[22], Value::Empty);
        assert_eq!(d.data[23], Value::Empty);
        for (event, estimate) in [(0.0, 1.0), (1.0, 0.0)] {
            let Value::Array(endpoint) = ev("ACT.KAPLANMEIER", vec![vec_arg(&[1.0]), vec_arg(&[event])]) else { panic!("expected spill") };
            assert_eq!(endpoint.data[4], Value::Number(estimate));
            assert_eq!(endpoint.data[5], Value::Number(0.0));
            assert_eq!(endpoint.data[6], Value::Empty);
            assert_eq!(endpoint.data[7], Value::Empty);
        }
    }
    #[test]
    fn delayed_entry_has_open_left_closed_right_risk_set() {
        let d = data(ev("ACT.KAPLANMEIER", vec![vec_arg(&[1.0, 2.0, 3.0]), vec_arg(&[1.0, 1.0, 0.0]), vec_arg(&[0.0, 1.0, 0.0])]));
        close(d[1], 2.0);
        close(d[4], 0.5);
        close(d[9], 2.0);
        close(d[12], 0.25);
        assert_eq!(ev("ACT.KAPLANMEIER", vec![vec_arg(&[1.0]), vec_arg(&[1.0]), vec_arg(&[1.0])]), Value::Error(CellError::Num));
    }
    #[test]
    fn aj_preserves_competing_risk_probability() {
        let d = data(ev("ACT.AALENJOHANSEN", vec![vec_arg(&[1.0, 2.0, 3.0, 4.0]), vec_arg(&[1.0, 2.0, 0.0, 1.0])]));
        close(d[4], 0.75);
        close(d[5], 0.25);
        close(d[6], 0.0);
        close(d[11], 0.5);
        close(d[12], 0.25);
        close(d[13], 0.25);
        close(d[25], 0.0);
        close(d[26], 0.75);
        close(d[27], 0.25);
        for r in d.chunks(7) {
            close(r[4] + r[5] + r[6], 1.0);
        }
    }
    #[test]
    fn aggregated_aj_equals_known_markov_product() {
        let risks = mat(&[&[10.0, 10.0], &[10.0, 10.0]]);
        let counts = mat(&[&[0.0, 2.0], &[1.0, 0.0], &[0.0, 2.0], &[1.0, 0.0]]);
        let d = data(ev("ACT.AALENJOHANSEN.MATRIX", vec![risks, counts]));
        close(d[1], 0.8);
        close(d[2], 0.2);
        close(d[4], 0.66);
        close(d[5], 0.34);
    }
    #[test]
    fn gamma_poisson_posterior_and_predictive_are_distinct() {
        let args = || vec![vec_arg(&[2.0, 3.0]), vec_arg(&[1.0, 2.0]), n(1.0), n(2.0)];
        let d = data(ev("ACT.BAYES.POISSON", args()));
        close(d[0], 6.0);
        close(d[1], 5.0);
        close(d[2], 1.2);
        close(d[3], 0.24);
        close(d[6], 1.2);
        close(d[7], 1.44);
        assert!(d[4] < d[2] && d[5] > d[2]);
        let mut a = args();
        a.push(n(0.0));
        close(data(ev("ACT.BAYES.POISSON.PREDICT", a))[0], (5.0_f64 / 6.0).powi(6));
    }
    #[test]
    fn beta_binomial_updates_and_predictive_moments() {
        let mut a = vec![vec_arg(&[2.0]), vec_arg(&[4.0]), n(1.0), n(1.0), n(0.95), n(10.0)];
        let d = data(ev("ACT.BAYES.BINOMIAL", a.clone()));
        close(d[0], 3.0);
        close(d[1], 3.0);
        close(d[2], 0.5);
        close(d[6], 5.0);
        close(d[7], 40.0 / 7.0);
        a = vec![vec_arg(&[0.0]), vec_arg(&[0.0]), n(1.0), n(1.0), n(2.0), n(4.0)];
        close(data(ev("ACT.BAYES.BINOMIAL.PREDICT", a.clone()))[0], 0.2);
        a.push(n(1.0));
        close(data(ev("ACT.BAYES.BINOMIAL.PREDICT", a))[0], 0.6);
    }
    #[test]
    fn posterior_zero_exposure_preserves_prior_and_invalid_counts_fail() {
        let d = data(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(2.0), n(4.0)]));
        close(d[2], 0.5);
        assert_eq!(ev("ACT.BAYES.POISSON", vec![n(1.0), n(0.0), n(2.0), n(4.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.BAYES.BINOMIAL", vec![n(2.0), n(1.0), n(1.0), n(1.0)]), Value::Error(CellError::Num));
    }

    #[test]
    fn high_exposure_recovery_and_predictive_mass_normalization() {
        let posterior = data(ev("ACT.BAYES.POISSON", vec![n(100_000.0), n(100_000.0), n(2.0), n(3.0)]));
        assert!((posterior[2] - 1.0).abs() < 1e-4);
        assert!(posterior[4] < 1.0 && posterior[5] > 1.0);
        let mass: f64 = (0..=10).map(|k| data(ev("ACT.BAYES.BINOMIAL.PREDICT", vec![n(3.0), n(5.0), n(2.0), n(4.0), n(k as f64), n(10.0)]))[0]).sum();
        close(mass, 1.0);
    }

    #[test]
    fn extreme_gamma_credible_bounds_use_direct_small_tails() {
        // Independent exponential quantiles: P^-1(tail)=-log1p(-tail), Q^-1(tail)=-log(tail).
        for confidence in [0.95_f64, 0.999_999_999_999_999, 0.999_999_999_999_999_9] {
            let tail = (1.0 - confidence) * 0.5;
            let expected_lower = -(-tail).ln_1p();
            let expected_upper = -tail.ln();
            let posterior = data(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(1.0), n(1.0), n(confidence)]));
            assert!((posterior[4] / expected_lower - 1.0).abs() < 5e-9);
            assert!((posterior[5] - expected_upper).abs() < 5e-9);
            assert!(tail_matches(reg_gamma_p(1.0, posterior[4]), tail));
            assert!(tail_matches(reg_gamma_q(1.0, posterior[5]), tail));
        }
    }

    #[test]
    fn credible_interval_representation_and_parameter_boundaries_are_explicit() {
        let uniform = data(ev("ACT.BAYES.BINOMIAL", vec![n(0.0), n(0.0), n(1.0), n(1.0)]));
        close(uniform[4], 0.025);
        close(uniform[5], 0.975);
        // Uniform upper bound 1-tail cannot represent the required extreme tail accurately.
        assert_eq!(ev("ACT.BAYES.BINOMIAL", vec![n(0.0), n(0.0), n(1.0), n(1.0), n(0.999_999_999_999_999_9)]), Value::Error(CellError::Num));
        // The Gamma(.001,1) 2.5% quantile is below the smallest positive f64.
        assert_eq!(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(0.001), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(0.0009), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.BAYES.BINOMIAL", vec![n(0.0), n(0.0), n(1_000_001.0), n(1.0)]), Value::Error(CellError::Num));
        // A credible interval cannot be reported with a rounded zero-width confidence input.
        assert_eq!(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(1.0), n(1.0), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.BAYES.POISSON", vec![n(0.0), n(0.0), n(1.0), n(1.0), n(1e-30)]), Value::Error(CellError::Num));
    }
    #[test]
    fn cox_null_effect_and_breslow_baseline() {
        // Each tied event time has one death in each covariate group; exact beta=0.
        let t = || vec_arg(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
        let e = || vec_arg(&[1.0; 6]);
        let x = || mat(&[&[0.0], &[1.0], &[0.0], &[1.0], &[0.0], &[1.0]]);
        close(data(ev("ACT.COX.FIT", vec![t(), e(), x()]))[0], 0.0);
        let b = data(ev("ACT.COX.BASELINE", vec![t(), e(), x(), n(0.0)]));
        close(b[1], 1.0 / 3.0);
        close(b[4], 0.5);
        close(b[7], 1.0);
        let covariance = data(ev("ACT.COX.COVARIANCE", vec![t(), e(), x(), n(0.0)]));
        close(covariance[0], 2.0 / 3.0);
        let survival = data(ev("ACT.COX.SURVIVAL", vec![mat(&[&[0.0], &[1.0]]), n(2.0_f64.ln()), vec_arg(&[0.0, 0.5, 1.0])]));
        close(survival[1], (-0.5_f64).exp());
        close(survival[5], (-2.0_f64).exp());
        assert_eq!(ev("ACT.COX.FIT", vec![t(), e(), mat(&[&[1.0], &[1.0], &[1.0], &[1.0], &[1.0], &[1.0]])]), Value::Error(CellError::Num));
    }
    #[test]
    fn cox_nonzero_exact_partial_likelihood_score() {
        // Risk set contrasts chosen to have finite nonzero MLE; verify recovered score.
        let t = vec_arg(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let e = vec_arg(&[1.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
        let x = mat(&[&[1.0], &[0.0], &[1.0], &[0.0], &[1.0], &[0.0]]);
        let args = vec![t, e, x];
        let beta = data(ev("ACT.COX.FIT", args.clone()));
        let d = cox_data(&args, 3).unwrap();
        let st = cox_stats(&d, &beta).unwrap();
        assert!(beta[0] > 0.0);
        assert!(st.score[0].abs() < 1e-7);
    }
    #[test]
    fn holdout_calibration_and_uncensored_brier_are_known() {
        let d = data(ev("ACT.CALIBRATION", vec![vec_arg(&[1.0, 3.0]), vec_arg(&[2.0, 2.0])]));
        close(d[3], 1.0);
        close(d[4], 0.0);
        close(d[5], 1.0);
        close(d[6], 1.0);
        close(data(ev("ACT.BRIER.IPCW", vec![vec_arg(&[1.0, 3.0]), vec_arg(&[1.0, 1.0]), vec_arg(&[0.2, 0.8]), n(2.0)]))[0], 0.04);
        // One of three subjects censored before horizon, one case and one control.
        close(data(ev("ACT.BRIER.IPCW", vec![vec_arg(&[1.0, 2.0, 3.0]), vec_arg(&[0.0, 1.0, 1.0]), vec_arg(&[0.5, 0.2, 0.8]), n(2.5)]))[0], 0.04);
    }

    #[test]
    fn ipcw_failure_before_censor_ties_preserve_independent_population_score() {
        // Enumerate the four equally likely independent combinations:
        // (T,C)=(1,1),(1,4),(3,1),(3,4), delta=I(T<=C). At h=2 and S_hat=.5,
        // every complete-data squared error is exactly .25. Repetition must not change it.
        for repeats in [1, 2, 17] {
            let times: Vec<f64> = (0..repeats).flat_map(|_| [1.0, 1.0, 1.0, 3.0]).collect();
            let events: Vec<f64> = (0..repeats).flat_map(|_| [1.0, 1.0, 0.0, 1.0]).collect();
            close(data(ev("ACT.BRIER.IPCW", vec![vec_arg(&times), vec_arg(&events), vec_arg(&vec![0.5; times.len()]), n(2.0)]))[0], 0.25);
        }
    }

    #[test]
    fn ipcw_pure_event_and_pure_censor_ties_handle_empty_denominators() {
        // A pure-event tie leaves G unchanged, including the last risk set Y=d.
        close(data(ev("ACT.BRIER.IPCW", vec![vec_arg(&[1.0, 1.0, 1.0]), vec_arg(&[1.0, 1.0, 1.0]), vec_arg(&[0.5, 0.5, 0.5]), n(2.0)]))[0], 0.25);
        // Pure censor ties still use the full risk set (d=0); one observed control remains.
        close(
            data(ev("ACT.BRIER.IPCW", vec![vec_arg(&[1.0, 1.0, 1.0, 3.0]), vec_arg(&[0.0, 0.0, 0.0, 1.0]), vec_arg(&[0.5, 0.5, 0.5, 0.5]), n(2.0)]))
                [0],
            0.25,
        );
        // No follow-up at all after a pure censor tie cannot support IPCW at the horizon.
        assert_eq!(
            ev("ACT.BRIER.IPCW", vec![vec_arg(&[1.0, 1.0]), vec_arg(&[0.0, 0.0]), vec_arg(&[0.5, 0.5]), n(2.0)]),
            Value::Error(CellError::Num)
        );
    }
    #[test]
    fn strict_alignment_nonfinite_and_resource_domains_fail() {
        assert_eq!(ev("ACT.KAPLANMEIER", vec![vec_arg(&[1.0, 2.0]), n(1.0)]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.KAPLANMEIER", vec![n(f64::NAN), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.AALENJOHANSEN", vec![n(1.0), n(17.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COX.FIT", vec![vec_arg(&[1.0, 2.0]), vec_arg(&[0.0, 0.0]), mat(&[&[0.0], &[1.0]])]), Value::Error(CellError::Num));
    }

    #[test]
    fn skipped_credible_confidence_defaults_before_future_exposure_or_trials() {
        let args = vec![n(0.0), n(0.0), n(1.0), n(1.0), empty(), n(2.0)];
        let posterior = data(ev("ACT.BAYES.POISSON", args.clone()));
        close(posterior[4], -0.975_f64.ln());
        close(posterior[5], -0.025_f64.ln());
        close(posterior[6], 2.0);
        close(posterior[7], 6.0);
        let beta = data(ev("ACT.BAYES.BINOMIAL", vec![n(0.0), n(0.0), n(1.0), n(1.0), empty(), n(10.0)]));
        close(beta[4], 0.025);
        close(beta[5], 0.975);
        close(beta[6], 5.0);
        close(beta[7], 10.0);
        let mut referenced = args;
        referenced[4] = Arg::reference(Value::Empty);
        assert_eq!(ev("ACT.BAYES.POISSON", referenced), Value::Error(CellError::Num));
    }
}
