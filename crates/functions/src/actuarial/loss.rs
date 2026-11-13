//! Actuarial loss distributions, contract layers and capital aggregation.
//!
//! All algorithms are deterministic. No sampler or Monte Carlo RNG is introduced here.
//! The empirical tail mean is the integral of the left-inverse quantile, including only the
//! required portion of probability at its threshold (not E[X | X >= VaR]). Compound distributions
//! use positive Panjer recursion for Poisson/negative binomial and positive convolution for
//! binomial, whose signed Panjer recursion can be numerically unstable.

use gridcraft_core::{CellError, Value};

use super::optional_num as opt_num;

use crate::special::{norm_cdf, norm_inv, norm_pdf, reg_gamma_p, reg_gamma_q};
use crate::util::{R, S, arg, array_val, as_array, has, num, num_val, text};
use crate::{Arg, Ctx, FnSpec};

const MAX_OBSERVATIONS: usize = 100_000;
const MAX_GRID: usize = 4096;
const MAX_OPERATIONS: usize = 20_000_000;
const MAX_RISKS: usize = 64;
const PROB_TOLERANCE: f64 = 1e-12;
/// Relative truncation error against a lower bound on the true nonnegative TVaR.
const TVAR_TAIL_TOLERANCE: f64 = 1e-10;
const CHERNOFF_EXPONENTS: usize = 47;

#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}

impl Sum {
    /// Accumulate a term with Neumaier compensation; the consuming calculation validates the final sum.
    fn add(&mut self, x: f64) {
        let total = self.value + x;
        self.correction += if self.value.abs() >= x.abs() { (self.value - total) + x } else { (x - total) + self.value };
        self.value = total;
    }

    /// Return the corrected accumulated sum for the caller's numerical-domain checks.
    fn total(&self) -> f64 {
        self.value + self.correction
    }
}

/// Require a finite numerical result, mapping NaN and infinity to `#NUM!`.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}

/// Read a coerced worksheet scalar and require a finite result.
fn number(a: &[Arg], i: usize) -> R<f64> {
    finite(num(a, i)?)
}

/// Validate a finite nonnegative moment, loss, exposure or other model parameter.
fn nonnegative(x: f64) -> R<f64> {
    if x.is_finite() && x >= 0.0 { Ok(x) } else { Err(CellError::Num) }
}

/// Validate a finite strictly positive scale, denominator or model parameter.
fn positive(x: f64) -> R<f64> {
    if x.is_finite() && x > 0.0 { Ok(x) } else { Err(CellError::Num) }
}

/// Validate a finite probability in the closed unit interval.
fn probability(x: f64) -> R<f64> {
    if x.is_finite() && (0.0..=1.0).contains(&x) { Ok(x) } else { Err(CellError::Num) }
}

/// Validate an exact nonnegative integer against the supplied finite-work bound.
fn integer(x: f64, max: usize) -> R<usize> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x <= max as f64 { Ok(x as usize) } else { Err(CellError::Num) }
}

/// Strict numeric vectors: missing values must never silently drop an associated weight.
fn vector(a: &[Arg], i: usize, max: usize) -> R<Vec<f64>> {
    let array = as_array(&arg(a, i)?.value);
    if array.data.len() > max {
        return Err(CellError::Num);
    }
    if array.data.is_empty() || (array.rows > 1 && array.cols > 1) {
        return Err(CellError::Value);
    }
    array
        .data
        .iter()
        .map(|v| match v {
            Value::Number(x) => finite(*x),
            Value::Error(e) => Err(*e),
            _ => Err(CellError::Value),
        })
        .collect()
}

/// Compute compound loss mean as expected claim count times expected severity.
fn aggregate_mean(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    num_val(nonnegative(number(a, 0)?)? * nonnegative(number(a, 1)?)?)
}

/// Compute compound loss variance under independent count and iid severities.
/// The formula separates expected-count severity variance from count-variance mean-loss risk.
fn aggregate_variance(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let count_mean = nonnegative(number(a, 0)?)?;
    let count_variance = nonnegative(number(a, 1)?)?;
    let severity_mean = nonnegative(number(a, 2)?)?;
    let severity_variance = nonnegative(number(a, 3)?)?;
    num_val(count_mean * severity_variance + count_variance * severity_mean * severity_mean)
}

/// Sorted support with normalized positive masses; zero-weight observations do not affect
/// support endpoints. Scale weights first so large finite weights never overflow their sum.
fn empirical(a: &[Arg], weights_index: usize) -> R<Vec<(f64, f64)>> {
    let values = vector(a, 0, MAX_OBSERVATIONS)?;
    let omitted_weights = !has(a, weights_index);
    let weights = if omitted_weights { vec![1.0; values.len()] } else { vector(a, weights_index, MAX_OBSERVATIONS)? };
    if values.len() != weights.len() {
        return Err(CellError::Value);
    }
    let mut largest = 0.0f64;
    for &w in &weights {
        largest = largest.max(nonnegative(w)?);
    }
    if largest == 0.0 {
        return Err(CellError::Num);
    }
    // Keep ordinary probability boundaries intact when summing unscaled weights is safe.
    let divisor = if largest <= f64::MAX / weights.len() as f64 { 1.0 } else { largest };
    let mut total = Sum::default();
    for w in &weights {
        total.add(*w / divisor);
    }
    let total = positive(total.total())?;
    let mut distribution: Vec<_> = values.into_iter().zip(weights).filter(|(_, w)| *w > 0.0).map(|(x, w)| (x, (w / divisor) / total)).collect();
    distribution.sort_by(|(x, _), (y, _)| x.total_cmp(y));
    Ok(distribution)
}

/// Find the left-inverse quantile of a complete empirical distribution, including support endpoints.
fn inverse(distribution: &[(f64, f64)], alpha: f64) -> R<f64> {
    probability(alpha)?;
    if alpha == 0.0 {
        return distribution.first().map(|(x, _)| *x).ok_or(CellError::Num);
    }
    let mut mass = Sum::default();
    for &(x, p) in distribution {
        mass.add(p);
        if mass.total() >= alpha {
            return Ok(x);
        }
    }
    // Every empirical distribution is complete. This fallback only absorbs summation rounding
    // at its endpoint, and never applies to the truncated compound distribution below.
    distribution.last().map(|(x, _)| *x).ok_or(CellError::Num)
}

/// Return a weighted empirical left-inverse loss quantile without interpolation between atoms.
fn value_at_risk(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let distribution = empirical(a, 2)?;
    num_val(inverse(&distribution, number(a, 1)?)?)
}

/// Integrate the exact fraction of upper empirical probability mass for TVaR.
/// Mass at the VaR threshold is allocated fractionally; confidence one is undefined.
fn tail_value_at_risk(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let distribution = empirical(a, 2)?;
    let alpha = probability(number(a, 1)?)?;
    if alpha == 1.0 {
        return Err(CellError::Num);
    }
    // Integrate from the upper tail down. This avoids subtracting nearly equal means and
    // computes precisely the fraction of mass needed at the VaR point, including ties.
    let mut remaining = 1.0 - alpha;
    let mut mean = Sum::default();
    for &(x, p) in distribution.iter().rev() {
        let take = remaining.min(p);
        mean.add(x * (take / (1.0 - alpha)));
        remaining -= take;
        if remaining <= 0.0 {
            break;
        }
    }
    num_val(mean.total())
}

/// Apply a finite occurrence deductible and limit to a nonnegative supplied loss.
fn layer_amount(loss: f64, deductible: f64, limit: f64) -> f64 {
    // Difference of ordered nonnegative finite values cannot overflow.
    (loss - deductible).max(0.0).min(limit)
}

/// Return the contract layer payment multiplied by an optional proportional participation share.
fn layer(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let loss = nonnegative(number(a, 0)?)?;
    let deductible = nonnegative(number(a, 1)?)?;
    let limit = nonnegative(number(a, 2)?)?;
    let share = probability(finite(opt_num(a, 3, 1.0)?)?)?;
    num_val(layer_amount(loss, deductible, limit) * share)
}

/// Return the ceded amount of a nonnegative loss under the supplied quota-share proportion.
fn quota_share(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    num_val(nonnegative(number(a, 0)?)? * probability(number(a, 1)?)?)
}

/// Compute expected stop-loss/layer recovery over complete weighted aggregate-loss observations.
fn stop_loss(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let distribution = empirical(a, 2)?;
    let attachment = nonnegative(number(a, 1)?)?;
    let limit = nonnegative(finite(opt_num(a, 3, f64::MAX)?)?)?;
    let mut expectation = Sum::default();
    for (x, p) in distribution {
        nonnegative(x)?;
        expectation.add(layer_amount(x, attachment, limit) * p);
    }
    num_val(expectation.total())
}

/// Compute the expected capped exponential loss with a cancellation-safe small-cap limit.
fn lev_exponential(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let limit = nonnegative(number(a, 0)?)?;
    let mean = positive(number(a, 1)?)?;
    let ratio = limit / mean;
    num_val(if ratio == 0.0 { limit } else { mean * -(-ratio).exp_m1() })
}

/// Normal log-CDF with a continued fraction for the Mills ratio in the far lower tail.
/// The existing normal CDF handles the central region; the fraction avoids log(0).
fn normal_log_cdf(z: f64) -> f64 {
    if z >= -8.0 {
        return norm_cdf(z).ln();
    }
    let x = -z;
    let mut fraction = 0.0;
    for k in (1..=64).rev() {
        fraction = k as f64 / (x + fraction);
    }
    -0.5 * x * x - 0.5 * (2.0 * std::f64::consts::PI).ln() - (x + fraction).ln()
}

/// Compute expected capped lognormal loss using log-loss parameters and stable lower-tail evaluation.
fn lev_lognormal(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let limit = nonnegative(number(a, 0)?)?;
    let mu = number(a, 1)?;
    let sigma = nonnegative(number(a, 2)?)?;
    if limit == 0.0 {
        return num_val(0.0);
    }
    if sigma == 0.0 {
        return num_val(if mu >= limit.ln() { limit } else { mu.exp() });
    }
    let variance = finite(sigma * sigma)?;
    let log_mean = finite(mu + 0.5 * variance)?;
    let z = (limit.ln() - mu) / sigma;
    let first = (log_mean + normal_log_cdf(z - sigma)).exp();
    let result = finite(first + limit * norm_cdf(-z))?;
    if result < 0.0 || result > limit * (1.0 + 1e-10) {
        return Err(CellError::Num);
    }
    num_val(result.min(limit))
}

/// Compute expected capped Pareto-II/Lomax loss, including the shape-one logarithmic limit.
fn lev_pareto(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let limit = nonnegative(number(a, 0)?)?;
    let shape = positive(number(a, 1)?)?;
    let scale = positive(number(a, 2)?)?;
    if limit == 0.0 {
        return num_val(0.0);
    }
    let ratio = limit / scale;
    if ratio == 0.0 {
        // Even the largest finite shape times an underflowed ratio is below machine
        // precision; E[min(X,u)] equals u to representable relative precision.
        return num_val(limit);
    }
    let log = if ratio.is_finite() { ratio.ln_1p() } else { limit.ln() - scale.ln() };
    let k = 1.0 - shape;
    let power = k * log;
    // Evaluate log(scale * expm1(k log(1+u/scale)) / k). Intermediate exponentials
    // or scale*expm1 may overflow/underflow although the bounded final value is finite.
    let log_ratio = if k == 0.0 || power == 0.0 {
        log.ln()
    } else {
        let log_expm1 = if power > 50.0 { power + (-(-power).exp()).ln_1p() } else { power.exp_m1().abs().ln() };
        log_expm1 - k.abs().ln()
    };
    num_val(finite((scale.ln() + log_ratio).exp())?.min(limit))
}

/// Compute expected capped Gamma loss on the supported numerical shape/scale domain.
/// Zero caps and extreme representable cap/scale limits are handled before incomplete-gamma evaluation.
fn lev_gamma(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let limit = nonnegative(number(a, 0)?)?;
    let shape = positive(number(a, 1)?)?;
    let scale = positive(number(a, 2)?)?;
    if limit == 0.0 {
        return num_val(0.0);
    }
    // The shared incomplete-gamma implementation uses 1-P for Q in the lower region.
    // Extremely small shapes would lose survival-tail precision; very large shapes can
    // exhaust its bounded series. Reject those domains instead of returning a false LEV.
    if !(1e-4..=1e6).contains(&shape) {
        return Err(CellError::Num);
    }
    let mean = finite(shape * scale)?;
    let z = limit / scale;
    if z == 0.0 {
        return Err(CellError::Num);
    }
    let out = finite(mean * reg_gamma_p(shape + 1.0, z) + limit * reg_gamma_q(shape, z))?;
    let bound = limit.min(mean);
    if out < 0.0 || out > bound * (1.0 + 1e-10) {
        return Err(CellError::Num);
    }
    num_val(out.min(bound))
}

#[derive(Clone, Copy)]
enum Count {
    Poisson(f64),
    Binomial(usize, f64),
    NegativeBinomial(f64, f64),
}

impl Count {
    /// Parse Poisson, binomial or failures-before-successes negative-binomial count parameters.
    fn from_args(a: &[Arg]) -> R<Self> {
        let p1 = number(a, 2)?;
        let p2 = number(a, 3)?;
        match text(a, 1)?.trim().to_ascii_lowercase().as_str() {
            "poisson" if p2 == 0.0 => Ok(Self::Poisson(nonnegative(p1)?)),
            "binomial" => Ok(Self::Binomial(integer(p1, MAX_GRID)?, probability(p2)?)),
            "negativebinomial" | "negbinomial" => Ok(Self::NegativeBinomial(positive(p1)?, positive(probability(p2)?)?)),
            _ => Err(CellError::Value),
        }
    }

    /// Return the count-family mean, rejecting an unrepresentable finite-precision result.
    fn mean(&self) -> R<f64> {
        finite(match *self {
            Self::Poisson(lambda) => lambda,
            Self::Binomial(n, p) => n as f64 * p,
            Self::NegativeBinomial(r, p) => r * ((1.0 - p) / p),
        })
    }
}

struct Compound {
    probabilities: Vec<f64>,
    unit: f64,
    mean: f64,
    // Exact finite support endpoint when it exists. The grid itself is not a support bound.
    maximum: Option<usize>,
    severity: Vec<f64>,
    count: Count,
    mean_index: f64,
    work: usize,
}

/// Read a complete lattice severity PMF and remove only accepted unit-mass decimal roundoff.
fn severity(a: &[Arg]) -> R<Vec<f64>> {
    let mut severity = vector(a, 0, MAX_GRID + 1)?;
    let mut sum = Sum::default();
    for &p in &severity {
        sum.add(probability(p)?);
    }
    let total = sum.total();
    if (total - 1.0).abs() > PROB_TOLERANCE {
        return Err(CellError::Num);
    }
    // Only remove decimal input roundoff; this is not acceptance of truncated severity mass.
    for p in &mut severity {
        *p /= total;
    }
    Ok(severity)
}

/// Parse a bounded compound model and build its requested lattice probability grid.
fn compound(a: &[Arg], unit_index: usize) -> R<Compound> {
    let severity = severity(a)?;
    let count = Count::from_args(a)?;
    let max_index = integer(number(a, 4)?, MAX_GRID)?;
    let unit = positive(finite(opt_num(a, unit_index, 1.0)?)?)?;
    compound_grid(severity, count, max_index, unit)
}

/// Estimate saturated recurrence/convolution work before allocating or executing a compound grid.
fn compound_work(count: Count, severity_length: usize, max_index: usize) -> usize {
    let terms = (max_index + 1).saturating_mul(severity_length.min(max_index + 1));
    match count {
        Count::Binomial(n, _) => n.saturating_mul(terms),
        _ => terms,
    }
}

/// Build a compound lattice PMF with positive Panjer recursion or positive binomial convolution.
/// The retained count/severity model supports certified tail extension without normalizing omitted mass.
fn compound_grid(severity: Vec<f64>, count: Count, max_index: usize, unit: f64) -> R<Compound> {
    finite(unit * max_index as f64)?;
    let largest_claim = severity.iter().rposition(|p| *p > 0.0).ok_or(CellError::Num)?;
    let maximum = if largest_claim == 0 {
        Some(0)
    } else {
        match count {
            Count::Binomial(n, p) => Some(if p == 0.0 { 0 } else { n.saturating_mul(largest_claim) }),
            Count::Poisson(0.0) | Count::NegativeBinomial(_, 1.0) => Some(0),
            _ => None,
        }
    };
    if maximum == Some(0) {
        let mut probabilities = vec![0.0; max_index + 1];
        if let Some(first) = probabilities.first_mut() {
            *first = 1.0;
        }
        return Ok(Compound { probabilities, unit, mean: 0.0, maximum, severity, count, mean_index: 0.0, work: 0 });
    }
    let mut severity_mean = Sum::default();
    for (j, p) in severity.iter().enumerate() {
        severity_mean.add(j as f64 * *p);
    }
    // A mathematically positive mean lost to underflow cannot support a reliable tail.
    let mean_index = positive(severity_mean.total() * count.mean()?)?;
    let mean = finite(mean_index * unit)?;
    let work = compound_work(count, severity.len(), max_index);
    if work > MAX_OPERATIONS {
        return Err(CellError::Num);
    }
    let f0 = *severity.first().ok_or(CellError::Value)?;
    let mut probabilities = vec![0.0; max_index + 1];
    if let Count::Binomial(n, p) = count {
        // Repeated convolution of a Bernoulli-thinned claim is positive and stable, unlike
        // the signed binomial Panjer recurrence. Reject excessive work before allocating loops.
        if n.saturating_mul(max_index + 1).saturating_mul(severity.len().min(max_index + 1)) > MAX_OPERATIONS {
            return Err(CellError::Num);
        }
        if let Some(first) = probabilities.first_mut() {
            *first = 1.0;
        }
        let zero = 1.0 - p + p * f0;
        for _ in 0..n {
            let mut next = vec![0.0; max_index + 1];
            for (k, slot) in next.iter_mut().enumerate() {
                let mut s = Sum::default();
                s.add(probabilities.get(k).copied().ok_or(CellError::Num)? * zero);
                for (j, fj) in severity.iter().enumerate().skip(1).take(k) {
                    s.add(p * *fj * probabilities.get(k - j).copied().ok_or(CellError::Num)?);
                }
                *slot = finite(s.total())?;
            }
            probabilities = next;
        }
    } else {
        if max_index.saturating_mul(severity.len().min(max_index + 1)) > MAX_OPERATIONS {
            return Err(CellError::Num);
        }
        let (a_coeff, b_coeff, log_g0) = match count {
            Count::Poisson(lambda) => (0.0, lambda, -lambda * (1.0 - f0)),
            Count::NegativeBinomial(r, p) => {
                let a_coeff = 1.0 - p;
                let denominator = 1.0 - a_coeff * f0;
                (a_coeff, (r - 1.0) * a_coeff, r * (p.ln() - denominator.ln()))
            }
            Count::Binomial(_, _) => return Err(CellError::Num),
        };
        let g0 = log_g0.exp();
        if !g0.is_finite() || g0 < f64::MIN_POSITIVE {
            // An underflowed starting mass makes all later terms falsely zero.
            return Err(CellError::Num);
        }
        if let Some(first) = probabilities.first_mut() {
            *first = g0;
        }
        let denominator = positive(1.0 - a_coeff * f0)?;
        for k in 1..=max_index {
            let mut s = Sum::default();
            for (j, fj) in severity.iter().enumerate().skip(1).take(k) {
                let previous = probabilities.get(k - j).copied().ok_or(CellError::Num)?;
                let ratio = j as f64 / k as f64;
                let coefficient = match count {
                    // Do not form a + (r-1)a*j/k: at j=k that subtracts a from a
                    // and erases small, valid r. Both terms below are nonnegative.
                    Count::NegativeBinomial(r, p) => (1.0 - p) * (1.0 - ratio) + (r * (1.0 - p)) * ratio,
                    _ => a_coeff + b_coeff * ratio,
                };
                s.add(coefficient * *fj * previous);
            }
            let value = nonnegative(s.total() / denominator)?;
            if let Some(slot) = probabilities.get_mut(k) {
                *slot = value;
            }
        }
    }
    let mut mass = Sum::default();
    for &p in &probabilities {
        mass.add(nonnegative(p)?);
    }
    if mass.total() > 1.0 + 1e-10 {
        return Err(CellError::Num);
    }
    Ok(Compound { probabilities, unit, mean, maximum, severity, count, mean_index, work })
}

/// Spill loss, PMF, CDF and rounded omitted mass for each represented lattice point.
/// The diagnostic remainder is not proof that an extremely small unrepresented tail is absent.
fn compound_pmf(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let model = compound(a, 5)?;
    let mut total = Sum::default();
    for p in &model.probabilities {
        total.add(*p);
    }
    let omitted = (1.0 - total.total()).max(0.0);
    let mut cdf = Sum::default();
    let mut data = Vec::with_capacity(model.probabilities.len() * 4);
    for (k, p) in model.probabilities.iter().enumerate() {
        cdf.add(*p);
        data.extend([Value::Number(k as f64 * model.unit), Value::Number(*p), Value::Number(cdf.total().min(1.0)), Value::Number(omitted)]);
    }
    array_val(model.probabilities.len(), 4, data)
}

/// Locate a represented compound quantile; confidence one requires fully enumerated finite support.
fn compound_quantile(model: &Compound, alpha: f64) -> R<usize> {
    probability(alpha)?;
    if alpha == 1.0 {
        return model.maximum.filter(|maximum| *maximum < model.probabilities.len()).ok_or(CellError::Num);
    }
    let mut cdf = Sum::default();
    for (k, p) in model.probabilities.iter().enumerate() {
        cdf.add(*p);
        if (alpha == 0.0 && *p > 0.0) || (alpha > 0.0 && cdf.total() >= alpha) {
            return Ok(k);
        }
    }
    Err(CellError::Num)
}

/// Return a compound lattice VaR only when the requested initial grid reaches its confidence level.
fn compound_var(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let model = compound(a, 6)?;
    let alpha = probability(number(a, 5)?)?;
    num_val(compound_quantile(&model, alpha)? as f64 * model.unit)
}

/// Compute compound TVaR from positive upper contributions with a certified omitted-moment bound.
/// The grid can extend within resource limits; unresolved truncation returns `#NUM!` instead of false precision.
fn compound_tvar(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    let model = compound(a, 6)?;
    let alpha = probability(number(a, 5)?)?;
    if alpha == 1.0 {
        return Err(CellError::Num);
    }
    if alpha == 0.0 {
        return num_val(model.mean);
    }
    // The user-provided grid must still reach the requested quantile. Tail certification
    // may extend that grid internally, but cannot turn an insufficient VaR grid into success.
    compound_quantile(&model, alpha)?;
    let initial_grid = model.probabilities.len().saturating_sub(1);
    let model = tvar_grid(model, 1.0 - alpha, ctx)?;
    let mut remaining = 1.0 - alpha;
    let mut upper_mean = Sum::default();
    let mut threshold = 0;
    // Integrate down from the positive upper probabilities, never from E[S] minus its
    // nearly equal lower partial moment. This also avoids CDF rounding to 1 near alpha=1.
    // Omitted mass is completed at zero rather than renormalized; the Chernoff first-moment
    // bound limits the resulting TVaR error to TVAR_TAIL_TOLERANCE times a true lower bound.
    for (k, p) in model.probabilities.iter().enumerate().rev() {
        let take = remaining.min(*p);
        upper_mean.add(k as f64 * (take / (1.0 - alpha)));
        remaining -= take;
        if take > 0.0 {
            threshold = k;
        }
        if remaining <= 0.0 {
            break;
        }
    }
    if threshold > initial_grid {
        return Err(CellError::Num);
    }
    num_val(upper_mean.total() * model.unit)
}

/// log E[Z exp(t Z)] for lattice-index aggregate Z. For severity MGF H and count PGF G,
/// E[Z exp(t Z)] = H'(t) G'(H(t)). Positive sums use expm1 to preserve small t and
/// slightly inflate H-1 and H' before the increasing count PGF to allow rounding slack.
fn chernoff_log_moment(model: &Compound, t: f64) -> R<f64> {
    let mut delta = Sum::default();
    let mut derivative = Sum::default();
    for (j, p) in model.severity.iter().enumerate().skip(1) {
        let z = t * j as f64;
        delta.add(*p * z.exp_m1());
        derivative.add((j as f64 * *p) * z.exp());
    }
    let delta = nonnegative(delta.total() * (1.0 + 1e-10))?;
    let derivative = positive(derivative.total() * (1.0 + 1e-10))?;
    let log_count_derivative = match model.count {
        Count::Poisson(lambda) => lambda.ln() + lambda * delta,
        Count::Binomial(n, p) => (n as f64).ln() + p.ln() + (n as f64 - 1.0) * (p * delta).ln_1p(),
        Count::NegativeBinomial(r, p) => {
            let a = 1.0 - p;
            let ratio = (a * delta) / p;
            if ratio >= 1.0 {
                return Err(CellError::Num); // Outside the negative-binomial MGF domain.
            }
            r.ln() + a.ln() - p.ln() - (r + 1.0) * (-ratio).ln_1p()
        }
    };
    finite(derivative.ln() + log_count_derivative + 1e-8)
}

/// E[Z 1{Z>K}] <= exp(-t(K+1)) E[Z exp(tZ)] for any admissible t>0.
/// Completing omitted mass at zero changes upper expected shortfall by no more than
/// this omitted first moment divided by beta=1-alpha. Since `TVaR >= E[Z]`, require
/// `bound/beta <= 1e-10 E[Z]`. We search a bounded positive t grid, then recompute once
/// to the smallest certified K. Failure to certify within 4096/20M terms is #NUM!.
fn tvar_grid(model: Compound, beta: f64, ctx: &mut dyn Ctx) -> R<Compound> {
    let current = model.probabilities.len().saturating_sub(1);
    if let Some(maximum) = model.maximum
        && maximum <= current
    {
        return Ok(model);
    }
    let bound_work = CHERNOFF_EXPONENTS.saturating_mul(model.severity.len());
    super::work::charge(ctx, (bound_work as u64).saturating_mul(32))?;
    let base_work = model.work.saturating_add(bound_work);
    if base_work > MAX_OPERATIONS {
        return Err(CellError::Num);
    }
    let log_budget = TVAR_TAIL_TOLERANCE.ln() + positive(model.mean_index)?.ln() + beta.ln();
    let max_claim = model.severity.iter().rposition(|p| *p > 0.0).ok_or(CellError::Num)?;
    let mut best =
        model.maximum.filter(|&k| k <= MAX_GRID && base_work.saturating_add(compound_work(model.count, model.severity.len(), k)) <= MAX_OPERATIONS);
    // t*max_claim ranges 2^-40..2^6, safely bounding each exponential by exp(64).
    for exponent in -40..=6 {
        let t = 2.0f64.powi(exponent) / max_claim as f64;
        let Ok(log_moment) = chernoff_log_moment(&model, t) else { continue };
        // One extra lattice point prevents an integer-rounding equality from accepting
        // a bound microscopically larger than its requested budget.
        let required = (((log_moment - log_budget) / t - 1.0).ceil() + 1.0).max(current as f64);
        if !required.is_finite() || required > MAX_GRID as f64 {
            continue;
        }
        let k = required as usize;
        if k == current {
            return Ok(model);
        }
        if base_work.saturating_add(compound_work(model.count, model.severity.len(), k)) <= MAX_OPERATIONS && best.is_none_or(|previous| k < previous)
        {
            best = Some(k);
        }
    }
    let required = best.ok_or(CellError::Num)?;
    super::work::charge(ctx, (compound_work(model.count, model.severity.len(), required) as u64).saturating_mul(16))?;
    compound_grid(model.severity, model.count, required, model.unit)
}

struct Capital {
    scaled: Vec<f64>,
    correlation: Vec<f64>,
    scale: f64,
    normalized_sd: f64,
}

/// Validate standard deviations and a symmetric unit-diagonal PSD correlation matrix.
/// A scaled Cholesky norm supports singular correlations without subtractive variance cancellation.
fn capital(a: &[Arg]) -> R<Capital> {
    let risks = vector(a, 0, MAX_RISKS)?;
    let dimension = risks.len();
    let matrix = as_array(&arg(a, 1)?.value);
    if matrix.rows != dimension || matrix.cols != dimension {
        return Err(CellError::Value);
    }
    let mut correlation = Vec::with_capacity(dimension * dimension);
    for x in &matrix.data {
        match x {
            Value::Number(x) if x.is_finite() && (-1.0..=1.0).contains(x) => correlation.push(*x),
            Value::Error(e) => return Err(*e),
            _ => return Err(CellError::Num),
        }
    }
    let get = |r: usize, c: usize| correlation.get(r * dimension + c).copied().ok_or(CellError::Num);
    for i in 0..dimension {
        if (get(i, i)? - 1.0).abs() > 1e-10 {
            return Err(CellError::Num);
        }
        for j in 0..i {
            if (get(i, j)? - get(j, i)?).abs() > 1e-10 {
                return Err(CellError::Num);
            }
        }
    }
    // Canonicalize accepted roundoff so the factorization and Euler gradient use precisely
    // the same symmetric unit-diagonal matrix.
    for i in 0..dimension {
        if let Some(diagonal) = correlation.get_mut(i * dimension + i) {
            *diagonal = 1.0;
        }
        for j in 0..i {
            let mean = 0.5
                * (correlation.get(i * dimension + j).copied().ok_or(CellError::Num)?
                    + correlation.get(j * dimension + i).copied().ok_or(CellError::Num)?);
            if let Some(lower) = correlation.get_mut(i * dimension + j) {
                *lower = mean;
            }
            if let Some(upper) = correlation.get_mut(j * dimension + i) {
                *upper = mean;
            }
        }
    }
    let get = |r: usize, c: usize| correlation.get(r * dimension + c).copied().ok_or(CellError::Num);
    let mut lower = vec![0.0; dimension * dimension];
    for i in 0..dimension {
        for j in 0..=i {
            let mut residual = Sum::default();
            residual.add(get(i, j)?);
            for k in 0..j {
                residual.add(
                    -lower.get(i * dimension + k).copied().ok_or(CellError::Num)? * lower.get(j * dimension + k).copied().ok_or(CellError::Num)?,
                );
            }
            let residual = residual.total();
            let value = if i == j {
                if residual < -1e-10 {
                    return Err(CellError::Num);
                }
                residual.max(0.0).sqrt()
            } else {
                let pivot: f64 = lower.get(j * dimension + j).copied().ok_or(CellError::Num)?;
                if pivot == 0.0 {
                    if residual.abs() > 1e-10 {
                        return Err(CellError::Num);
                    }
                    0.0
                } else {
                    residual / pivot
                }
            };
            if let Some(slot) = lower.get_mut(i * dimension + j) {
                *slot = value;
            }
        }
    }
    let mut scale = 0.0f64;
    for &risk in &risks {
        scale = scale.max(nonnegative(risk)?);
    }
    let scaled: Vec<_> = risks.iter().map(|x| if scale == 0.0 { 0.0 } else { *x / scale }).collect();
    // ||L' sd|| is more stable than the subtractive quadratic form near singularity.
    let mut normalized_sd = 0.0f64;
    for j in 0..dimension {
        let mut projection = Sum::default();
        for (i, x) in scaled.iter().enumerate().skip(j) {
            projection.add(lower.get(i * dimension + j).copied().ok_or(CellError::Num)? * *x);
        }
        normalized_sd = normalized_sd.hypot(projection.total());
    }
    Ok(Capital { scaled, correlation, scale, normalized_sd })
}

/// Return portfolio standard deviation from supplied risks and a validated correlation matrix.
fn capital_sd(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let model = capital(a)?;
    num_val(model.scale * model.normalized_sd)
}

/// Spill Euler contributions to portfolio standard deviation.
/// Nonzero exactly hedged portfolios have an undefined gradient and return `#NUM!`.
fn capital_allocation(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let model = capital(a)?;
    let dimension = model.scaled.len();
    if model.normalized_sd == 0.0 && model.scale > 0.0 {
        // The norm has no unique gradient at a fully hedged zero-risk portfolio.
        return Err(CellError::Num);
    }
    let mut data = Vec::with_capacity(dimension);
    for (i, s) in model.scaled.iter().enumerate() {
        let mut covariance = Sum::default();
        for (j, t) in model.scaled.iter().enumerate() {
            covariance.add(model.correlation.get(i * dimension + j).copied().ok_or(CellError::Num)? * *t);
        }
        let allocation = if model.scale == 0.0 { 0.0 } else { (s * covariance.total() / model.normalized_sd) * model.scale };
        data.push(Value::Number(finite(allocation)?));
    }
    array_val(dimension, 1, data)
}

/// Return an analytic normal-loss VaR, including a zero-deviation point mass.
fn normal_var(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let mean = number(a, 0)?;
    let sd = nonnegative(number(a, 1)?)?;
    let alpha = probability(number(a, 2)?)?;
    if sd == 0.0 {
        return num_val(mean);
    }
    num_val(mean + sd * norm_inv(alpha))
}

/// Return normal upper-tail expected loss for confidence below one; confidence zero is the mean.
fn normal_tvar(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let mean = number(a, 0)?;
    let sd = nonnegative(number(a, 1)?)?;
    let alpha = probability(number(a, 2)?)?;
    if alpha == 1.0 {
        return Err(CellError::Num);
    }
    if sd == 0.0 || alpha == 0.0 {
        return num_val(mean);
    }
    num_val(mean + sd * (norm_pdf(norm_inv(alpha)) / (1.0 - alpha)))
}

/// Compute ultimate ruin in the homogeneous Poisson/exponential Cramér–Lundberg model.
/// The model has independent claims, constant premium inflow and no investment income.
fn ruin_exponential(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let capital = nonnegative(number(a, 0)?)?;
    let rate = nonnegative(number(a, 1)?)?;
    let mean = positive(number(a, 2)?)?;
    let premium = positive(number(a, 3)?)?;
    if rate == 0.0 {
        return num_val(0.0);
    }
    let log_load = rate.ln() + mean.ln() - premium.ln();
    if log_load >= 0.0 {
        return num_val(1.0);
    }
    let load = log_load.exp();
    // Classical homogeneous Poisson/exponential Cramér–Lundberg model; constant premium
    // inflow, no investment income, independent claims, ruin strictly below zero.
    num_val(load * (-(capital / mean) * (1.0 - load)).exp())
}

/// Register deterministic compound-loss, contract-layer and economic-risk worksheet functions.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.AGGREGATE.MEAN",
            2,
            2,
            Financial,
            S,
            "ACT.AGGREGATE.MEAN(count_mean, severity_mean)",
            "Mean aggregate loss for independent iid severities and count.",
            aggregate_mean
        ),
        act!(
            "ACT.AGGREGATE.VARIANCE",
            4,
            4,
            Financial,
            S,
            "ACT.AGGREGATE.VARIANCE(count_mean, count_variance, severity_mean, severity_variance)",
            "Compound loss variance under independent count and iid severity.",
            aggregate_variance
        ),
        act!(
            "ACT.VAR",
            2,
            3,
            Statistical,
            &[false, true, false],
            "ACT.VAR(losses, alpha, [weights])",
            "Left-inverse quantile of a complete empirical weighted loss distribution.",
            value_at_risk
        ),
        act!(
            "ACT.TVAR",
            2,
            3,
            Statistical,
            &[false, true, false],
            "ACT.TVAR(losses, alpha, [weights])",
            "Upper quantile-integral mean, with partial mass at VaR; alpha in [0,1).",
            tail_value_at_risk
        ),
        act!(
            "ACT.LAYER",
            3,
            4,
            Financial,
            S,
            "ACT.LAYER(loss, deductible, limit, [share])",
            "Per-loss indemnity: share times min(max(loss-deductible,0),limit).",
            layer
        ),
        act!(
            "ACT.QUOTASHARE",
            2,
            2,
            Financial,
            S,
            "ACT.QUOTASHARE(loss, ceded_share)",
            "Loss ceded under a proportional quota-share treaty.",
            quota_share
        ),
        act!(
            "ACT.STOPLOSS",
            2,
            4,
            Financial,
            &[false, true, false, true],
            "ACT.STOPLOSS(losses, attachment, [weights], [limit])",
            "Expected aggregate stop-loss layer over complete weighted aggregate-loss observations.",
            stop_loss
        ),
        act!(
            "ACT.LEV.EXPONENTIAL",
            2,
            2,
            Financial,
            S,
            "ACT.LEV.EXPONENTIAL(limit, mean)",
            "Expected minimum of an exponential loss and a finite limit.",
            lev_exponential
        ),
        act!(
            "ACT.LEV.LOGNORMAL",
            3,
            3,
            Financial,
            S,
            "ACT.LEV.LOGNORMAL(limit, mu, sigma)",
            "Lognormal limited mean; mu and sigma parameterize the log loss.",
            lev_lognormal
        ),
        act!(
            "ACT.LEV.PARETO",
            3,
            3,
            Financial,
            S,
            "ACT.LEV.PARETO(limit, shape, scale)",
            "Limited mean of Pareto II (Lomax), survival (1+x/scale)^(-shape).",
            lev_pareto
        ),
        act!(
            "ACT.LEV.GAMMA",
            3,
            3,
            Financial,
            S,
            "ACT.LEV.GAMMA(limit, shape, scale)",
            "Gamma limited mean with shape/scale parameterization.",
            lev_gamma
        ),
        act!(
            "ACT.COMPOUND.PMF",
            5,
            6,
            Statistical,
            &[false, true, true, true, true, true],
            "ACT.COMPOUND.PMF(severity_pmf, model, param1, param2, max_index, [unit])",
            "Compound distribution: spill loss, PMF, CDF, omitted-tail mass; no tail normalization.",
            compound_pmf
        ),
        act!(
            "ACT.COMPOUND.VAR",
            6,
            7,
            Statistical,
            &[false, true, true, true, true, true, true],
            "ACT.COMPOUND.VAR(severity_pmf, model, param1, param2, max_index, alpha, [unit])",
            "Compound left-inverse quantile; errors if the enumerated grid cannot reach alpha.",
            compound_var
        ),
        act!(
            "ACT.COMPOUND.TVAR",
            6,
            7,
            Statistical,
            &[false, true, true, true, true, true, true],
            "ACT.COMPOUND.TVAR(severity_pmf, model, param1, param2, max_index, alpha, [unit])",
            "Compound upper-tail mean with positive-tail summation and bounded omitted first moment.",
            compound_tvar
        ),
        act!(
            "ACT.CAPITAL.SD",
            2,
            2,
            Financial,
            &[false, false],
            "ACT.CAPITAL.SD(standard_deviations, correlation)",
            "Portfolio standard deviation with validated positive-semidefinite correlation.",
            capital_sd
        ),
        act!(
            "ACT.CAPITAL.ALLOCATION",
            2,
            2,
            Financial,
            &[false, false],
            "ACT.CAPITAL.ALLOCATION(standard_deviations, correlation)",
            "Euler contributions to portfolio standard deviation; may be negative for hedges.",
            capital_allocation
        ),
        act!(
            "ACT.VAR.NORMAL",
            3,
            3,
            Statistical,
            S,
            "ACT.VAR.NORMAL(mean, standard_deviation, alpha)",
            "Analytic normal-model VaR; not a general loss-distribution approximation guarantee.",
            normal_var
        ),
        act!(
            "ACT.TVAR.NORMAL",
            3,
            3,
            Statistical,
            S,
            "ACT.TVAR.NORMAL(mean, standard_deviation, alpha)",
            "Analytic normal-model upper-tail mean.",
            normal_tvar
        ),
        act!(
            "ACT.RUIN.EXPONENTIAL",
            4,
            4,
            Financial,
            S,
            "ACT.RUIN.EXPONENTIAL(capital, claim_rate, claim_mean, premium_rate)",
            "Infinite-horizon classical Poisson/exponential ruin probability, no investment.",
            ruin_exponential
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    fn values(result: Value) -> Vec<f64> {
        match result {
            Value::Array(a) => a.data.iter().map(|v| if let Value::Number(x) = v { *x } else { panic!("expected number: {v:?}") }).collect(),
            other => panic!("expected array: {other:?}"),
        }
    }

    fn compound_args(model: &str, p1: f64, p2: f64, max: f64) -> Vec<Arg> {
        vec![av(col(&[0.0, 1.0])), t(model), n(p1), n(p2), n(max)]
    }

    #[test]
    fn aggregate_moments_match_compound_poisson() {
        close(ev("ACT.AGGREGATE.MEAN", vec![n(5.0), n(10.0)]), 50.0);
        close(ev("ACT.AGGREGATE.VARIANCE", vec![n(5.0), n(5.0), n(10.0), n(100.0)]), 1000.0);
        close(ev("ACT.AGGREGATE.VARIANCE", vec![n(3.0), n(0.0), n(10.0), n(4.0)]), 12.0);
    }

    #[test]
    fn discrete_tvar_integrates_fraction_of_threshold_atom() {
        let losses = || av(col(&[0.0, 100.0]));
        let weights = || av(col(&[0.95, 0.05]));
        close(ev("ACT.VAR", vec![losses(), n(0.95), weights()]), 0.0);
        close(ev("ACT.TVAR", vec![losses(), n(0.90), weights()]), 50.0);
        close(ev("ACT.TVAR", vec![losses(), n(0.95), weights()]), 100.0);
        close(ev("ACT.TVAR", vec![losses(), n(0.0), weights()]), 5.0);
        assert_eq!(ev("ACT.TVAR", vec![losses(), n(1.0)]), Value::Error(CellError::Num));
        close(ev("ACT.VAR", vec![av(col(&[-100.0, 2.0, 8.0])), n(0.0), av(col(&[0.0, 1.0, 1.0]))]), 2.0);
        close(ev("ACT.VAR", vec![losses(), n(1.0)]), 100.0);
    }

    #[test]
    fn empirical_weights_scale_without_overflow_and_strict_pairing() {
        close(ev("ACT.TVAR", vec![av(col(&[2.0, 8.0])), n(0.0), av(col(&[1e308, 1e308]))]), 5.0);
        assert_eq!(ev("ACT.VAR", vec![av(col(&[1.0, 2.0])), n(0.5), av(col(&[1.0]))]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.VAR", vec![av(col(&[1.0])), n(0.5), av(col(&[0.0]))]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.TVAR", vec![av(arr(vec![vec![nv(1.0), Value::Empty]])), n(0.5)]), Value::Error(CellError::Value));
    }

    #[test]
    fn contract_layers_and_stop_loss_identity() {
        close(ev("ACT.LAYER", vec![n(120.0), n(20.0), n(50.0), n(0.8)]), 40.0);
        close(ev("ACT.QUOTASHARE", vec![n(120.0), n(0.25)]), 30.0);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(20.0), av(col(&[0.5, 0.5]))]), 40.0);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(20.0), av(col(&[0.5, 0.5])), n(30.0)]), 15.0);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(0.0)]), 50.0);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(20.0), empty(), n(30.0)]), 15.0);
        assert_eq!(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(20.0), rf(Value::Empty), n(30.0)]), Value::Error(CellError::Value));
    }

    #[test]
    fn analytic_limited_expectations() {
        close(ev("ACT.LEV.EXPONENTIAL", vec![n(10.0), n(10.0)]), 10.0 * (1.0 - (-1.0f64).exp()));
        close(ev("ACT.LEV.GAMMA", vec![n(10.0), n(1.0), n(10.0)]), 10.0 * (1.0 - (-1.0f64).exp()));
        close(ev("ACT.LEV.PARETO", vec![n(10.0), n(1.0), n(10.0)]), 10.0 * 2.0f64.ln());
        close(ev("ACT.LEV.PARETO", vec![n(10.0), n(2.0), n(10.0)]), 5.0);
        close(ev("ACT.LEV.LOGNORMAL", vec![n(1.0), n(0.0), n(1.0)]), 0.761_578_291_865_123_4);
        close(ev("ACT.LEV.LOGNORMAL", vec![n(2.0), n(0.0), n(0.0)]), 1.0);
        close(ev("ACT.LEV.LOGNORMAL", vec![n(0.5), n(0.0), n(0.0)]), 0.5);
        close(ev("ACT.LEV.EXPONENTIAL", vec![n(1e-10), n(1e10)]), 1e-10);
        close(ev("ACT.LEV.LOGNORMAL", vec![n(1.0), n(1000.0), n(1.0)]), 1.0);
    }

    #[test]
    fn limited_means_keep_extreme_finite_caps_and_reject_unreliable_gamma_domain() {
        close(ev("ACT.LEV.GAMMA", vec![n(0.0), n(1e308), n(1e308)]), 0.0);
        close(ev("ACT.LEV.GAMMA", vec![n(1e308), n(1.0), n(1e-300)]), 1e-300);
        assert_eq!(ev("ACT.LEV.GAMMA", vec![n(1.0), n(1e-308), n(1e308)]), Value::Error(CellError::Num));
        let Value::Number(pareto) = ev("ACT.LEV.PARETO", vec![n(1e308), n(1e-300), n(1e-300)]) else { panic!("finite Pareto LEV") };
        assert!((pareto / 1e308 - 1.0).abs() < 1e-12);
        let Value::Number(exponential) = ev("ACT.LEV.EXPONENTIAL", vec![n(1e-308), n(1e308)]) else { panic!("finite exponential LEV") };
        assert_eq!(exponential, 1e-308);
        let Value::Number(pareto) = ev("ACT.LEV.PARETO", vec![n(1e-308), n(2.0), n(1e308)]) else { panic!("finite Pareto LEV") };
        assert_eq!(pareto, 1e-308);
    }

    #[test]
    fn compound_poisson_matches_analytic_count_pmf_without_tail_renormalization() {
        let result = values(ev("ACT.COMPOUND.PMF", compound_args("poisson", 2.0, 0.0, 2.0)));
        assert_eq!(result.len(), 12);
        let p0 = (-2.0f64).exp();
        assert!((result[1] - p0).abs() < 1e-13);
        assert!((result[5] - 2.0 * p0).abs() < 1e-13);
        assert!((result[9] - 2.0 * p0).abs() < 1e-13);
        assert!((result[3] - (1.0 - 5.0 * p0)).abs() < 1e-13);
        let mut args = compound_args("poisson", 2.0, 0.0, 2.0);
        args.push(n(0.9));
        assert_eq!(ev("ACT.COMPOUND.VAR", args), Value::Error(CellError::Num));
    }

    #[test]
    fn compound_binomial_and_negative_binomial_are_correct() {
        let binomial = values(ev("ACT.COMPOUND.PMF", compound_args("binomial", 2.0, 0.5, 2.0)));
        assert_eq!(binomial, vec![0.0, 0.25, 0.25, 0.0, 1.0, 0.5, 0.75, 0.0, 2.0, 0.25, 1.0, 0.0]);
        let negative = values(ev("ACT.COMPOUND.PMF", compound_args("negativebinomial", 1.0, 0.5, 2.0)));
        assert_eq!(negative, vec![0.0, 0.5, 0.5, 0.125, 1.0, 0.25, 0.75, 0.125, 2.0, 0.125, 0.875, 0.125]);
        let zero = values(ev("ACT.COMPOUND.PMF", compound_args("poisson", 0.0, 0.0, 1.0)));
        assert_eq!(zero, vec![0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
        let mut args = compound_args("binomial", 2.0, 0.5, 2.0);
        args.push(n(0.5));
        close(ev("ACT.COMPOUND.TVAR", args), 1.5);
    }

    #[test]
    fn compound_tvar_includes_unenumerated_tail_first_moment() {
        let mut args = compound_args("poisson", 2.0, 0.0, 1.0);
        args.push(n(0.2)); // quantile is 1; probability above grid is ~0.594.
        let p0 = (-2.0f64).exp();
        close(ev("ACT.COMPOUND.TVAR", args), 1.0 + (1.0 + p0) / 0.8);
        let mut thinned = vec![av(col(&[0.5, 0.5])), t("poisson"), n(4.0), n(0.0), n(1.0)];
        thinned.push(n(0.2));
        close(ev("ACT.COMPOUND.TVAR", thinned), 1.0 + (1.0 + p0) / 0.8);
    }

    #[test]
    fn compound_tvar_extreme_tail_matches_independent_decimal80_oracle() {
        // Independent 80-decimal-digit Poisson(1) series, using each alpha's exact binary
        // value. The old analytic-mean-minus-lower-moment formula was ~4% wrong here.
        for (alpha, expected) in [(0.999_999_999_999, 14.319_844_689_344_53), (0.999_999_999_999_999, 17.064_043_603_884_746)] {
            let mut args = compound_args("poisson", 1.0, 0.0, 64.0);
            args.push(n(alpha));
            let Value::Number(actual) = ev("ACT.COMPOUND.TVAR", args) else { panic!("certified Poisson TVaR") };
            assert!((actual / expected - 1.0).abs() < 1e-12, "{alpha}: {actual} versus {expected}");
        }
    }

    #[test]
    fn negative_binomial_small_shape_preserves_positive_probabilities() {
        for r in [1e-12, 1e-20] {
            let result = values(ev("ACT.COMPOUND.PMF", compound_args("negativebinomial", r, 0.5, 2.0)));
            let p0 = (r * 0.5f64.ln()).exp();
            let p1 = r * 0.5 * p0;
            let p2 = r * (r + 1.0) * 0.125 * p0;
            assert!((result[5] / p1 - 1.0).abs() < 1e-13);
            assert!((result[9] / p2 - 1.0).abs() < 1e-13);
        }
    }

    #[test]
    fn compound_tvar_extends_grid_or_refuses_uncertified_residual() {
        let mut short = compound_args("poisson", 2.0, 0.0, 1.0);
        short.push(n(0.2));
        close(ev("ACT.COMPOUND.TVAR", short), 1.0 + (1.0 + (-2.0f64).exp()) / 0.8);
        // A geometric tail with mean~100000 cannot have its missing first moment
        // certified on 4096 points. Returning the represented positive tail would be false.
        let mut heavy = compound_args("negativebinomial", 1.0, 1e-5, 1.0);
        heavy.push(n(1e-6));
        assert_eq!(ev("ACT.COMPOUND.TVAR", heavy), Value::Error(CellError::Num));
    }

    #[test]
    fn finite_support_endpoints_and_zero_severity_are_not_grid_endpoints() {
        let mut args = compound_args("binomial", 2.0, 0.5, 4.0);
        args.push(n(1.0));
        close(ev("ACT.COMPOUND.VAR", args), 2.0);
        let mut args = compound_args("poisson", 2.0, 0.0, 100.0);
        args.push(n(1.0));
        assert_eq!(ev("ACT.COMPOUND.VAR", args), Value::Error(CellError::Num));
        let mut args = compound_args("binomial", 2.0, 0.5, 4.0);
        args.push(n(0.99));
        close(ev("ACT.COMPOUND.TVAR", args), 2.0);
        let all_zero = vec![av(col(&[1.0])), t("negativebinomial"), n(2.0), n(1e-300), n(1.0)];
        assert_eq!(values(ev("ACT.COMPOUND.PMF", all_zero)), vec![0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn compound_rejects_lost_starting_mass_and_excess_work() {
        assert_eq!(ev("ACT.COMPOUND.PMF", compound_args("poisson", 1000.0, 0.0, 100.0)), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COMPOUND.PMF", compound_args("binomial", 4096.0, 0.5, 4096.0)), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COMPOUND.PMF", compound_args("poisson", 2.0, 0.0, 4097.0)), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COMPOUND.PMF", vec![av(col(&[0.0, 0.5])), t("poisson"), n(1.0), n(0.0), n(2.0)]), Value::Error(CellError::Num));
    }

    fn corr(rows: &[&[f64]]) -> Arg {
        av(arr(rows.iter().map(|row| row.iter().copied().map(nv).collect()).collect()))
    }

    #[test]
    fn covariance_aggregation_and_euler_additivity() {
        let risks = || av(col(&[3.0, 4.0]));
        let matrix = || corr(&[&[1.0, 0.0], &[0.0, 1.0]]);
        close(ev("ACT.CAPITAL.SD", vec![risks(), matrix()]), 5.0);
        let allocation = values(ev("ACT.CAPITAL.ALLOCATION", vec![risks(), matrix()]));
        assert!((allocation[0] - 1.8).abs() < 1e-12);
        assert!((allocation[1] - 3.2).abs() < 1e-12);
        close(ev("ACT.CAPITAL.SD", vec![risks(), corr(&[&[1.0, 1.0], &[1.0, 1.0]])]), 7.0);
        close(ev("ACT.CAPITAL.SD", vec![risks(), corr(&[&[1.0, -1.0], &[-1.0, 1.0]])]), 1.0);
        assert_eq!(ev("ACT.CAPITAL.ALLOCATION", vec![av(col(&[3.0, 3.0])), corr(&[&[1.0, -1.0], &[-1.0, 1.0]])]), Value::Error(CellError::Num));
    }

    #[test]
    fn covariance_rejects_indefinite_and_asymmetric_matrices() {
        assert_eq!(
            ev("ACT.CAPITAL.SD", vec![av(col(&[1.0, 1.0, 1.0])), corr(&[&[1.0, -0.9, -0.9], &[-0.9, 1.0, -0.9], &[-0.9, -0.9, 1.0]])]),
            Value::Error(CellError::Num)
        );
        assert_eq!(ev("ACT.CAPITAL.SD", vec![av(col(&[1.0, 1.0])), corr(&[&[1.0, 0.0], &[0.2, 1.0]])]), Value::Error(CellError::Num));
    }

    #[test]
    fn normal_tails_and_exponential_ruin_analytic_limits() {
        close(ev("ACT.VAR.NORMAL", vec![n(10.0), n(2.0), n(0.5)]), 10.0);
        close(ev("ACT.TVAR.NORMAL", vec![n(10.0), n(2.0), n(0.5)]), 10.0 + 4.0 / (2.0 * std::f64::consts::PI).sqrt());
        close(ev("ACT.RUIN.EXPONENTIAL", vec![n(0.0), n(1.0), n(10.0), n(20.0)]), 0.5);
        close(ev("ACT.RUIN.EXPONENTIAL", vec![n(20.0), n(1.0), n(10.0), n(20.0)]), 0.5 * (-1.0f64).exp());
        close(ev("ACT.RUIN.EXPONENTIAL", vec![n(20.0), n(1.0), n(10.0), n(10.0)]), 1.0);
        close(ev("ACT.RUIN.EXPONENTIAL", vec![n(0.0), n(0.0), n(10.0), n(20.0)]), 0.0);
    }

    #[test]
    fn hostile_numeric_arguments_are_errors() {
        for invalid in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(ev("ACT.LAYER", vec![n(10.0), n(invalid), n(5.0)]), Value::Error(CellError::Num));
            assert_eq!(ev("ACT.LEV.GAMMA", vec![n(10.0), n(invalid), n(5.0)]), Value::Error(CellError::Num));
        }
        assert_eq!(ev("ACT.COMPOUND.PMF", compound_args("binomial", 2.5, 0.5, 2.0)), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.VAR", vec![av(col(&[f64::NAN])), n(0.5)]), Value::Error(CellError::Num));
    }

    #[test]
    fn skipped_optional_slots_preserve_layer_limit_and_lattice_defaults() {
        close(ev("ACT.LAYER", vec![n(100.0), n(25.0), n(50.0), empty()]), 50.0);
        close(ev("ACT.LAYER", vec![n(100.0), n(25.0), n(50.0), Arg::reference(Value::Empty)]), 0.0);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(25.0), empty(), empty()]), 37.5);
        close(ev("ACT.STOPLOSS", vec![av(col(&[0.0, 100.0])), n(25.0), empty(), n(0.0)]), 0.0);
        let mut args = vec![av(col(&[0.0, 1.0])), t("poisson"), n(1.0), n(0.0), n(16.0), n(0.95), empty()];
        close(ev("ACT.COMPOUND.VAR", args.clone()), 3.0);
        args[6] = Arg::reference(Value::Empty);
        assert_eq!(ev("ACT.COMPOUND.VAR", args), Value::Error(CellError::Num));
    }
}
