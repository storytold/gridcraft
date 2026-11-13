//! Peaks-over-threshold extreme-value inference and deterministic bivariate copulas.
//! Parameterizations, numerical domains and uncertainty limitations are in the pack docs.
//! No sampler, random-number generator or copied external implementation is used.

use super::optional_num as opt_num;

use crate::special::{norm_cdf, norm_inv, reg_inc_beta};
use crate::util::{R, S, arg, array_val, as_array, num, num_val, text};
use crate::{Arg, Ctx, FnSpec};
use gridcraft_core::{CellError, Value};

const MAX_SAMPLE: usize = 2048;
const MAX_PAIRS: usize = 4_194_304;
const XI_MIN: f64 = -0.49;
const XI_MAX: f64 = 2.0;

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
/// Validates a finite probability in the closed unit interval.
fn probability(x: f64) -> R<f64> {
    if x.is_finite() && (0.0..=1.0).contains(&x) { Ok(x) } else { Err(CellError::Num) }
}
/// Reads a numeric cell without coercing blanks, booleans or text; propagates cell errors.
fn strict(v: &Value) -> R<f64> {
    match v {
        Value::Number(x) => finite(*x),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}
/// Reads a bounded numeric row/column vector while preserving observation alignment.
fn vector(a: &[Arg], i: usize, max: usize) -> R<Vec<f64>> {
    let v = as_array(&arg(a, i)?.value);
    if v.rows > 1 && v.cols > 1 {
        return Err(CellError::Value);
    }
    if v.data.is_empty() || v.data.len() > max {
        return Err(CellError::Num);
    }
    v.iter().map(strict).collect()
}
/// Accumulates finite terms with Neumaier compensation; rejects nonfinite intermediate sums.
fn sum(values: impl IntoIterator<Item = f64>) -> R<f64> {
    let (mut s, mut c) = (0.0_f64, 0.0_f64);
    for x in values {
        finite(x)?;
        let next = finite(s + x)?;
        c += if s.abs() >= x.abs() { (s - next) + x } else { (x - next) + s };
        s = next;
    }
    finite(s + c)
}
/// Builds a numeric spill of the requested dimensions, rejecting nonfinite output cells.
fn output(rows: usize, cols: usize, values: Vec<f64>) -> R<Value> {
    let data = values.into_iter().map(num_val).collect::<R<Vec<_>>>()?;
    array_val(rows, cols, data)
}

/// Evaluates the Generalized Pareto excess quantile with stable exponential-limit arithmetic.
/// Probability one is finite only for a negative-shape bounded tail.
fn gp_quantile(p: f64, xi: f64, beta: f64) -> R<f64> {
    probability(p)?;
    positive(beta)?;
    finite(xi)?;
    if p == 1.0 {
        return if xi < 0.0 { finite(-beta / xi) } else { Err(CellError::Num) };
    }
    let log_tail = (-p).ln_1p();
    let z = -xi * log_tail;
    if xi == 0.0 || z.abs() < 1e-5 {
        finite(-beta * log_tail * (1.0 + z / 2.0 + z * z / 6.0 + z * z * z / 24.0 + z.powi(4) / 120.0))
    } else {
        finite(beta * (z.exp_m1() / xi))
    }
}
/// Returns the Generalized Pareto excess mean, rejecting shape at or above one.
fn gp_mean(xi: f64, beta: f64) -> R<f64> {
    positive(beta)?;
    finite(xi)?;
    if xi >= 1.0 { Err(CellError::Num) } else { finite(beta / (1.0 - xi)) }
}
/// Returns the Generalized Pareto integrated upper-tail mean for `alpha < 1` and shape below one.
fn gp_tail(p: f64, xi: f64, beta: f64) -> R<f64> {
    probability(p)?;
    if p == 1.0 || xi >= 1.0 {
        return Err(CellError::Num);
    }
    let q = gp_quantile(p, xi, beta)?;
    finite((q + beta) / (1.0 - xi))
}
/// Evaluates the Generalized Pareto excess CDF, including its exponential and bounded-support limits.
fn gpd_cdf(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let x = scalar(a, 0)?;
    let xi = scalar(a, 1)?;
    let beta = scalar(a, 2)?;
    if x <= 0.0 {
        positive(beta)?;
        return num_val(0.0);
    }
    if xi < 0.0 && x >= -positive(beta)? / xi {
        return num_val(1.0);
    }
    positive(beta)?;
    let log_survival = if xi == 0.0 {
        -x / beta
    } else {
        let log_z = xi.abs().ln() + x.ln() - beta.ln();
        if log_z < -20.0 {
            let z = xi.signum() * log_z.exp();
            -(x / beta) * (1.0 - z / 2.0 + z * z / 3.0)
        } else if xi > 0.0 {
            let log_one_plus_z = if log_z > 0.0 { log_z + (-log_z).exp().ln_1p() } else { log_z.exp().ln_1p() };
            -log_one_plus_z / xi
        } else {
            -(-log_z.exp()).ln_1p() / xi
        }
    };
    if log_survival == f64::NEG_INFINITY {
        return num_val(1.0);
    }
    num_val(-finite(log_survival)?.exp_m1())
}
/// Exposes the Generalized Pareto excess quantile as a scalar worksheet function.
fn gpd_inv(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    num_val(gp_quantile(scalar(a, 0)?, scalar(a, 1)?, scalar(a, 2)?)?)
}
/// Exposes the finite Generalized Pareto excess mean; nonexistent moments return `#NUM!`.
fn gpd_mean(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    num_val(gp_mean(scalar(a, 0)?, scalar(a, 1)?)?)
}
/// Exposes TVaR of the excess distribution, not an unconditional portfolio tail.
fn gpd_tvar(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    num_val(gp_tail(scalar(a, 0)?, scalar(a, 1)?, scalar(a, 2)?)?)
}
/// Validates a threshold, exceedance probability and GPD parameters for portfolio-tail evaluation.
/// The requested confidence must lie above the threshold probability.
fn pot_params(a: &[Arg]) -> R<(f64, f64, f64, f64, f64)> {
    let threshold = scalar(a, 0)?;
    let exceed = probability(scalar(a, 1)?)?;
    let xi = scalar(a, 2)?;
    let beta = positive(scalar(a, 3)?)?;
    let alpha = probability(scalar(a, 4)?)?;
    if exceed == 0.0 || alpha < 1.0 - exceed {
        return Err(CellError::Num);
    }
    Ok((threshold, exceed, xi, beta, alpha))
}
/// Maps unconditional portfolio confidence to an excess quantile and adds the threshold.
fn pot_var(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (threshold, exceed, xi, beta, alpha) = pot_params(a)?;
    let conditional = if alpha == 1.0 { 1.0 } else { 1.0 - (1.0 - alpha) / exceed };
    num_val(threshold + gp_quantile(conditional.clamp(0.0, 1.0), xi, beta)?)
}
/// Maps unconditional portfolio confidence to an excess TVaR and adds the threshold.
/// The exceedance probability is essential; confidence one has no finite tail-mean contract.
fn pot_tvar(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (threshold, exceed, xi, beta, alpha) = pot_params(a)?;
    if alpha == 1.0 {
        return Err(CellError::Num);
    }
    let conditional = 1.0 - (1.0 - alpha) / exceed;
    num_val(threshold + gp_tail(conditional.clamp(0.0, 1.0), xi, beta)?)
}

/// Reads and sorts a bounded, nonconstant sample of nonnegative excess losses.
fn excesses(a: &[Arg]) -> R<Vec<f64>> {
    let mut x = vector(a, 0, MAX_SAMPLE)?;
    if x.len() < 8 || x.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    x.sort_by(f64::total_cmp);
    if x.first() == x.last() {
        return Err(CellError::Num);
    }
    Ok(x)
}
/// PWM based on unbiased empirical E[X(1-F(X))], using survival-order weights.
fn pwm(x: &[f64]) -> R<(f64, f64)> {
    let n = x.len();
    if n < 3 {
        return Err(CellError::Num);
    }
    let b0 = sum(x.iter().map(|x| *x / n as f64))?;
    let b1 = sum(x.iter().enumerate().map(|(i, x)| (*x / n as f64) * ((n - 1 - i) as f64 / (n - 1) as f64)))?;
    let denominator = positive(b0 - 2.0 * b1)?;
    let xi = finite(2.0 - b0 / denominator)?;
    let beta = positive(b0 * (1.0 - xi))?;
    if x.iter().any(|v| 1.0 + xi * (*v / beta) <= 0.0) {
        return Err(CellError::Num);
    }
    Ok((xi, beta))
}
/// Evaluates the GPD log-likelihood on its valid support, including the exponential limit.
fn log_likelihood(x: &[f64], xi: f64, beta: f64) -> R<f64> {
    positive(beta)?;
    finite(xi)?;
    sum(x.iter().map(|x| {
        let z = xi * (*x / beta);
        if z <= -1.0 || !z.is_finite() {
            return f64::NAN;
        }
        // Series avoids dividing log1p(z) by tiny xi near the exponential limit.
        let h = if z.abs() < 1e-5 {
            let y = *x / beta;
            y * (1.0 - z * 0.5 + z * z / 3.0 - z * z * z / 4.0)
        } else {
            z.ln_1p() / xi
        };
        -beta.ln() - (1.0 + xi) * h
    }))
}
/// Fits GPD shape/scale by probability-weighted moments.
/// Returns shape, scale, count, likelihood and jackknife uncertainty; uncertainty cells are
/// blank when the fitted tail lacks the required finite-variance regime.
fn gpd_fit_pwm(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let x = excesses(a)?;
    let n = x.len();
    let (xi, beta) = pwm(&x)?;
    let ll = log_likelihood(&x, xi, beta)?;
    let mut result = vec![Value::Number(xi), Value::Number(beta), Value::Number(n as f64), Value::Number(ll)];
    // Delete-one uncertainty is not reported in the infinite-variance tail regime.
    if xi >= 0.5 {
        result.extend([Value::Empty, Value::Empty, Value::Empty]);
    } else {
        let mut estimates = Vec::with_capacity(n);
        for omit in 0..n {
            let sample: Vec<f64> = x.iter().enumerate().filter_map(|(i, v)| if i == omit { None } else { Some(*v) }).collect();
            estimates.push(pwm(&sample)?);
        }
        let mean_xi = sum(estimates.iter().map(|p| p.0 / n as f64))?;
        let mean_beta = sum(estimates.iter().map(|p| p.1 / n as f64))?;
        let scale = (n - 1) as f64 / n as f64;
        let variance_xi = finite(scale * sum(estimates.iter().map(|p| (p.0 - mean_xi).powi(2)))?)?;
        let variance_beta = finite(scale * sum(estimates.iter().map(|p| (p.1 - mean_beta).powi(2)))?)?;
        let covariance = finite(scale * sum(estimates.iter().map(|p| (p.0 - mean_xi) * (p.1 - mean_beta)))?)?;
        result.extend([Value::Number(variance_xi.sqrt()), Value::Number(variance_beta.sqrt()), Value::Number(covariance)]);
    }
    array_val(1, 7, result)
}

/// Golden-section maximum over a closed interval. The caller verifies an interior optimum.
fn maximize<F: Fn(f64) -> f64>(f: &F, mut lo: f64, mut hi: f64) -> R<(f64, f64)> {
    let ratio = (5.0_f64.sqrt() - 1.0) * 0.5;
    let mut left = hi - ratio * (hi - lo);
    let mut right = lo + ratio * (hi - lo);
    let mut fl = f(left);
    let mut fr = f(right);
    for _ in 0..72 {
        if fl > fr {
            hi = right;
            right = left;
            fr = fl;
            left = hi - ratio * (hi - lo);
            fl = f(left);
        } else {
            lo = left;
            left = right;
            fl = fr;
            right = lo + ratio * (hi - lo);
            fr = f(right);
        }
    }
    let best = if fl > fr { (left, fl) } else { (right, fr) };
    finite(best.1)?;
    Ok(best)
}
/// Profiles positive GPD scale in log space at a fixed regular-region shape.
/// Bounds also respect the finite endpoint of a negative-shape distribution.
fn profile_scale(x: &[f64], xi: f64) -> R<(f64, f64)> {
    let mean = sum(x.iter().map(|v| *v / x.len() as f64))?;
    let maximum = *x.last().ok_or(CellError::Num)?;
    let floor = if xi < 0.0 { -xi * maximum * (1.0 + 1e-9) } else { mean * 1e-5 };
    let lower = positive(floor.max(mean * 1e-5))?.ln();
    let upper = positive(mean.max(maximum) * 1e4)?.ln();
    let evaluate = |log_beta: f64| log_likelihood(x, xi, log_beta.exp()).unwrap_or(f64::NEG_INFINITY);
    let (log_beta, ll) = maximize(&evaluate, lower, upper)?;
    if log_beta - lower < 1e-6 || upper - log_beta < 1e-6 {
        return Err(CellError::Num);
    }
    Ok((log_beta.exp(), ll))
}
/// Fits a bounded regular-region GPD profile likelihood and checks an interior stationary point.
/// Returns shape, scale, count, likelihood, two standard errors and covariance from positive
/// observed information; boundary or unresolved fits are rejected, not regularized.
fn gpd_fit_mle(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let x = excesses(a)?;
    // A bounded regular likelihood region avoids nonregular endpoint maxima xi<=-1/2.
    let grid = 32;
    let evaluate = |xi: f64| profile_scale(&x, xi).map(|p| p.1).unwrap_or(f64::NEG_INFINITY);
    let mut best = (0usize, f64::NEG_INFINITY);
    for i in 0..=grid {
        let ll = evaluate(XI_MIN + (XI_MAX - XI_MIN) * i as f64 / grid as f64);
        if ll > best.1 {
            best = (i, ll);
        }
    }
    if best.0 == 0 || best.0 == grid {
        return Err(CellError::Num);
    }
    let lower = XI_MIN + (XI_MAX - XI_MIN) * (best.0 - 1) as f64 / grid as f64;
    let upper = XI_MIN + (XI_MAX - XI_MIN) * (best.0 + 1) as f64 / grid as f64;
    let (xi, ll) = maximize(&evaluate, lower, upper)?;
    let (beta, _) = profile_scale(&x, xi)?;
    let eta = beta.ln();
    let hx = 2e-4 * (1.0 + xi.abs());
    let he = 2e-4;
    if xi - hx <= XI_MIN || xi + hx >= XI_MAX {
        return Err(CellError::Num);
    }
    let f = |s: f64, e: f64| log_likelihood(&x, s, e.exp());
    let fxp = f(xi + hx, eta)?;
    let fxm = f(xi - hx, eta)?;
    let fep = f(xi, eta + he)?;
    let fem = f(xi, eta - he)?;
    let score_x = (fxp - fxm) / (2.0 * hx);
    let score_e = (fep - fem) / (2.0 * he);
    if score_x.abs() > 1e-3 * x.len() as f64 || score_e.abs() > 1e-3 * x.len() as f64 {
        return Err(CellError::Num);
    }
    let ixx = -(fxp - 2.0 * ll + fxm) / (hx * hx);
    let iee = -(fep - 2.0 * ll + fem) / (he * he);
    let ixe = -(f(xi + hx, eta + he)? - f(xi + hx, eta - he)? - f(xi - hx, eta + he)? + f(xi - hx, eta - he)?) / (4.0 * hx * he);
    let determinant = positive(ixx * iee - ixe * ixe)?;
    positive(ixx)?;
    positive(iee)?;
    let var_x = positive(iee / determinant)?;
    let var_e = positive(ixx / determinant)?;
    let covariance = finite(-ixe / determinant * beta)?;
    output(1, 7, vec![xi, beta, x.len() as f64, ll, var_x.sqrt(), beta * var_e.sqrt(), covariance])
}
/// Reports threshold-by-threshold exceedance counts, fractions, mean excess and PWM parameters.
/// Every supplied threshold needs enough strict exceedances; no threshold is selected automatically.
fn gpd_thresholds(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let values = vector(a, 0, MAX_SAMPLE)?;
    let thresholds = vector(a, 1, 32)?;
    let mut out = Vec::with_capacity(thresholds.len() * 6);
    for threshold in &thresholds {
        let mut excess: Vec<f64> = values.iter().filter(|v| **v > *threshold).map(|v| *v - threshold).collect();
        if excess.len() < 8 {
            return Err(CellError::Num);
        }
        excess.sort_by(f64::total_cmp);
        let mean = sum(excess.iter().map(|v| *v / excess.len() as f64))?;
        let (xi, beta) = pwm(&excess)?;
        out.extend([*threshold, excess.len() as f64, excess.len() as f64 / values.len() as f64, mean, xi, beta]);
    }
    output(thresholds.len(), 6, out)
}

#[derive(Clone, Copy)]
enum Family {
    Independent,
    Gaussian,
    Student,
    Clayton,
    Gumbel,
    Frank,
}
/// Parses the supported bivariate copula family names without inventing a fallback family.
fn family(a: &[Arg], i: usize) -> R<Family> {
    match text(a, i)?.trim().to_ascii_lowercase().as_str() {
        "independent" | "independence" => Ok(Family::Independent),
        "gaussian" | "normal" => Ok(Family::Gaussian),
        "student" | "t" => Ok(Family::Student),
        "clayton" => Ok(Family::Clayton),
        "gumbel" => Ok(Family::Gumbel),
        "frank" => Ok(Family::Frank),
        _ => Err(CellError::Value),
    }
}
/// Checks each copula family's dependence parameter and Student degrees-of-freedom domain.
fn validate(f: Family, p: f64, df: f64) -> R<()> {
    finite(p)?;
    let valid = match f {
        Family::Independent => p == 0.0,
        Family::Gaussian | Family::Student => (-1.0..=1.0).contains(&p),
        Family::Clayton => (0.0..=10_000.0).contains(&p),
        Family::Gumbel => (1.0..=10_000.0).contains(&p),
        Family::Frank => p.abs() <= 35.0,
    };
    if !valid || matches!(f, Family::Student) && (!df.is_finite() || !(2.0..=200.0).contains(&df)) {
        return Err(CellError::Num);
    }
    Ok(())
}
/// Evaluates a univariate Student CDF through the regularized incomplete Beta identity.
fn student_cdf(x: f64, df: f64) -> R<f64> {
    let z = df / (df + x * x);
    let lower = 0.5 * reg_inc_beta(z, df * 0.5, 0.5);
    probability(if x < 0.0 { lower } else { 1.0 - lower })
}
/// Inverts the univariate Student CDF with bounded bracketing and numerical-domain checks.
fn student_inv(p: f64, df: f64) -> R<f64> {
    if p <= 0.0 || p >= 1.0 {
        return Err(CellError::Num);
    }
    if p == 0.5 {
        return Ok(0.0);
    }
    // Invert lower tail directly, avoiding 1-CDF cancellation in extreme upper tails.
    let target = if p < 0.5 { p } else { 1.0 - p };
    let mut hi = 1.0;
    for _ in 0..60 {
        if student_cdf(-hi, df)? <= target {
            break;
        }
        hi *= 2.0;
    }
    if student_cdf(-hi, df)? > target {
        return Err(CellError::Num);
    }
    let mut lo = 0.0;
    for _ in 0..96 {
        let mid = (lo + hi) * 0.5;
        if student_cdf(-mid, df)? > target {
            lo = mid
        } else {
            hi = mid
        };
    }
    let x = (lo + hi) * 0.5;
    Ok(if p < 0.5 { -x } else { x })
}
/// Iterative adaptive Simpson with local absolute tolerances and an evaluation/work cap.
fn integrate<F: Fn(f64) -> R<f64>>(f: &F, lo: f64, hi: f64, tol: f64) -> R<f64> {
    if lo == hi {
        return Ok(0.0);
    }
    if hi < lo {
        return Ok(-integrate(f, hi, lo, tol)?);
    }
    let mid = (lo + hi) * 0.5;
    let fa = f(lo)?;
    let fm = f(mid)?;
    let fb = f(hi)?;
    let whole = (hi - lo) * (fa + 4.0 * fm + fb) / 6.0;
    let mut stack = vec![(lo, hi, fa, fm, fb, whole, tol, 0usize)];
    let mut result = Vec::new();
    let mut evaluations = 3;
    while let Some((a, b, fa, fm, fb, whole, eps, depth)) = stack.pop() {
        let m = (a + b) * 0.5;
        let left_m = (a + m) * 0.5;
        let right_m = (m + b) * 0.5;
        let fl = f(left_m)?;
        let fr = f(right_m)?;
        evaluations += 2;
        if evaluations > 8192 {
            return Err(CellError::Num);
        }
        let left = (m - a) * (fa + 4.0 * fl + fm) / 6.0;
        let right = (b - m) * (fm + 4.0 * fr + fb) / 6.0;
        let delta = left + right - whole;
        if delta.abs() <= 15.0 * eps {
            result.push(left + right + delta / 15.0);
        } else {
            if depth >= 22 {
                return Err(CellError::Num);
            }
            stack.push((m, b, fm, fr, fb, right, eps * 0.5, depth + 1));
            stack.push((a, m, fa, fl, fm, left, eps * 0.5, depth + 1));
        }
    }
    sum(result)
}
/// Evaluates the Clayton generator sum on a log scale to avoid power overflow.
fn log_clayton_sum(u: f64, v: f64, p: f64) -> f64 {
    let a = -p * u.ln();
    let b = -p * v.ln();
    let m = a.max(b);
    if m < 300.0 { (a.exp_m1() + b.exp_m1()).ln_1p() } else { m + ((a - m).exp() + (b - m).exp() - (-m).exp()).ln() }
}
/// Evaluates the Gumbel generator norm using scaled powers of negative log margins.
fn gumbel_norm(u: f64, v: f64, p: f64) -> f64 {
    let a = (-u.ln()).ln() * p;
    let b = (-v.ln()).ln() * p;
    let m = a.max(b);
    ((m + ((a - m).exp() + (b - m).exp()).ln()) / p).exp()
}
/// Builds the Frank conditional-distribution denominator on a stable logarithmic scale.
fn frank_log_numerator(u: f64, v: f64, p: f64) -> R<f64> {
    let m = p * u.min(v);
    let value = (m - p * u).exp() + (m - p * v).exp() - (m - p * (u + v)).exp() - (m - p).exp();
    Ok(-m + positive(value)?.ln())
}
/// Evaluates a copula conditional CDF for an interior conditioning margin.
/// Uses the actual conditional Student law and stable Archimedean identities; unresolved
/// probabilities are rejected rather than silently clipped into a misleading distribution.
fn conditional(u: f64, v: f64, f: Family, p: f64, df: f64) -> R<f64> {
    if v == 0.0 {
        return Ok(0.0);
    }
    if v == 1.0 {
        return Ok(1.0);
    }
    if u <= 0.0 || u >= 1.0 {
        return Err(CellError::Num);
    }
    let value = match f {
        Family::Independent => v,
        Family::Gaussian => {
            if p == 1.0 {
                if v >= u { 1.0 } else { 0.0 }
            } else if p == -1.0 {
                if v >= 1.0 - u { 1.0 } else { 0.0 }
            } else {
                norm_cdf((norm_inv(v) - p * norm_inv(u)) / (1.0 - p * p).sqrt())
            }
        }
        Family::Student => {
            if p == 1.0 {
                if v >= u { 1.0 } else { 0.0 }
            } else if p == -1.0 {
                if v >= 1.0 - u { 1.0 } else { 0.0 }
            } else {
                let x = student_inv(u, df)?;
                let y = student_inv(v, df)?;
                student_cdf((y - p * x) * ((df + 1.0) / ((df + x * x) * (1.0 - p * p))).sqrt(), df + 1.0)?
            }
        }
        Family::Clayton => {
            if p == 0.0 {
                v
            } else {
                ((-p - 1.0) * u.ln() + (-1.0 / p - 1.0) * log_clayton_sum(u, v, p)).exp()
            }
        }
        Family::Gumbel => {
            if p == 1.0 {
                v
            } else {
                let s = gumbel_norm(u, v, p);
                (-s + (1.0 - p) * s.ln() + (p - 1.0) * (-u.ln()).ln() - u.ln()).exp()
            }
        }
        Family::Frank => {
            if p.abs() < 1e-8 {
                // First-order analytic independence limit. The omitted relative
                // term is O(theta^2), below f64 precision at this cutoff.
                v + 0.5 * p * v * (1.0 - v) * (1.0 - 2.0 * u)
            } else if p < 0.5 {
                let numerator = (-p * u).exp() * (-p * v).exp_m1();
                numerator / ((-p).exp_m1() + (-p * u).exp_m1() * (-p * v).exp_m1())
            } else {
                (-p * u + (-(-p * v).exp_m1()).ln() - frank_log_numerator(u, v, p)?).exp()
            }
        }
    };
    if !(-1e-10..=1.0 + 1e-10).contains(&value) {
        return Err(CellError::Num);
    }
    finite(value.clamp(0.0, 1.0))
}

/// A positive conditional-probability integral, scaled by its smaller marginal.
/// This avoids subtracting nearly equal probabilities in elliptical extreme tails.
/// The omitted normalized interval has mass at most 1e-24. If that bound would
/// dominate a very small answer, or quadrature cannot resolve it, return #NUM.
fn elliptical_lower(u: f64, v: f64, f: Family, p: f64, df: f64) -> R<f64> {
    if u == 0.0 || v == 0.0 {
        return Ok(0.0);
    }
    if u == 1.0 {
        return Ok(v);
    }
    if v == 1.0 {
        return Ok(u);
    }
    if p == 1.0 {
        return Ok(u.min(v));
    }
    if p == -1.0 {
        return Ok((u + v - 1.0).max(0.0));
    }
    if matches!(f, Family::Gaussian) && p == 0.0 {
        return finite(u * v);
    }
    if matches!(f, Family::Student) && (u.min(v) < 1e-12 || u.max(v) > 1.0 - 1e-12) {
        return Err(CellError::Num);
    }
    let margin = u.min(v);
    let other = u.max(v);
    let y = if matches!(f, Family::Gaussian) { finite(norm_inv(other))? } else { student_inv(other, df)? };
    let integrand = |r: f64| {
        let s = margin * r.powi(8);
        let x = if matches!(f, Family::Gaussian) { finite(norm_inv(s))? } else { student_inv(s, df)? };
        let conditional = if matches!(f, Family::Gaussian) {
            norm_cdf((y - p * x) / (1.0 - p * p).sqrt())
        } else {
            student_cdf((y - p * x) * ((df + 1.0) / ((df + x * x) * (1.0 - p * p))).sqrt(), df + 1.0)?
        };
        finite(8.0 * r.powi(7) * conditional)
    };
    let mut estimate = positive(integrate(&integrand, 1e-3, 1.0, 1e-9)?)?;
    for _ in 0..3 {
        if estimate < 1e-15 {
            return Err(CellError::Num);
        }
        let refined = positive(integrate(&integrand, 1e-3, 1.0, estimate * 1e-9)?)?;
        if refined >= 1e-15 && (refined - estimate).abs() <= refined * 1e-7 {
            let result = positive(margin * refined)?;
            if result > margin * (1.0 + 1e-9) {
                return Err(CellError::Num);
            }
            return Ok(result.min(margin));
        }
        estimate = refined;
    }
    Err(CellError::Num)
}

/// Evaluates the Frank copula with stable ratios and its small-parameter independence limit.
fn frank_cdf(u: f64, v: f64, p: f64) -> R<f64> {
    if p.abs() < 1e-8 {
        return finite(u * v * (1.0 + 0.5 * p * (1.0 - u) * (1.0 - v)));
    }
    // Divide first: multiplying two O(theta) expm1 terms can underflow even
    // while the copula probability itself remains an ordinary finite number.
    let ratio = (-p * u).exp_m1() * ((-p * v).exp_m1() / (-p).exp_m1());
    // log1p is essential when the CDF itself is tiny. Only the positive-theta,
    // near-upper-corner case needs the alternate log numerator to avoid log(0).
    if ratio > -0.9 { finite(-ratio.ln_1p() / p) } else { finite(-(frank_log_numerator(u, v, p)? - (-(-p).exp_m1()).ln()) / p) }
}

/// Stable positive-association survival identity:
/// S(u,v)=(1-u)(1-v)+C(u,v)*(1-exp(-d)), d=log(C/(uv)).
/// Compute d directly, not by subtracting nearly equal CDFs or logarithms.
fn archimedean_survival(u: f64, v: f64, f: Family, p: f64) -> R<f64> {
    let independent = (1.0 - u) * (1.0 - v);
    if matches!(f, Family::Clayton) && p == 0.0 || matches!(f, Family::Gumbel) && p == 1.0 {
        return Ok(independent);
    }
    let x = -u.ln();
    let y = -v.ln();
    let (d, log_cdf) = match f {
        Family::Clayton => {
            let a = p * u.ln();
            let b = p * v.ln();
            let k = (-a.exp_m1()) * (-b.exp_m1());
            let log_remaining = if k < 0.5 {
                (-k).ln_1p()
            } else {
                let m = a.max(b);
                m + ((a - m).exp() + (b - m).exp() - (a + b - m).exp()).ln()
            };
            (-log_remaining / p, -log_clayton_sum(u, v, p) / p)
        }
        Family::Gumbel => {
            let largest = x.max(y);
            let ratio = x.min(y) / largest;
            if ratio == 0.0 {
                return Ok(independent);
            }
            let log_ratio = (((p - 1.0) * ratio.ln()).exp_m1() * (ratio / (1.0 + ratio))).ln_1p();
            let delta = (log_ratio - (p - 1.0) * ratio.ln_1p()) / p;
            let d = largest * (1.0 + ratio) * -delta.exp_m1();
            (d, -gumbel_norm(u, v, p))
        }
        _ => return Err(CellError::Value),
    };
    if !d.is_finite() || d < 0.0 {
        return Err(CellError::Num);
    }
    positive(independent + log_cdf.exp() * -(-d).exp_m1())
}

/// Evaluates a validated bivariate copula, honoring uniform margins and singular dependence limits.
/// Elliptical extreme tails use positive conditional integration rather than CDF subtraction.
fn copula(u: f64, v: f64, f: Family, p: f64, df: f64) -> R<f64> {
    probability(u)?;
    probability(v)?;
    validate(f, p, df)?;
    if u == 0.0 || v == 0.0 {
        return Ok(0.0);
    }
    if u == 1.0 {
        return Ok(v);
    }
    if v == 1.0 {
        return Ok(u);
    }
    let result = match f {
        Family::Independent => u * v,
        Family::Gaussian => {
            if p == 1.0 {
                u.min(v)
            } else if p == -1.0 {
                (u + v - 1.0).max(0.0)
            } else {
                let x = norm_inv(u);
                let y = norm_inv(v);
                let integrand = |theta: f64| {
                    let r = theta.sin();
                    let c = theta.cos();
                    finite((-(x * x - 2.0 * r * x * y + y * y) / (2.0 * c * c)).exp() / (2.0 * std::f64::consts::PI))
                };
                let result = u * v + integrate(&integrand, 0.0, p.asin(), 2e-10)?;
                if result < 1e-8 { elliptical_lower(u, v, f, p, df)? } else { result }
            }
        }
        Family::Student => {
            if p == 1.0 {
                u.min(v)
            } else if p == -1.0 {
                (u + v - 1.0).max(0.0)
            } else if u == 0.5 && v == 0.5 {
                0.25 + p.asin() / (2.0 * std::f64::consts::PI)
            } else {
                elliptical_lower(u, v, f, p, df)?
            }
        }
        Family::Clayton => {
            if p == 0.0 {
                u * v
            } else {
                (-log_clayton_sum(u, v, p) / p).exp()
            }
        }
        Family::Gumbel => (-gumbel_norm(u, v, p)).exp(),
        Family::Frank => frank_cdf(u, v, p)?,
    };
    let lower = (u + v - 1.0).max(0.0);
    let upper = u.min(v);
    if result == 0.0 && !(matches!(f, Family::Gaussian | Family::Student) && p == -1.0) {
        return Err(CellError::Num);
    }
    if !result.is_finite() || result < lower - 1e-8 || result > upper + 1e-8 {
        return Err(CellError::Num);
    }
    Ok(result.clamp(lower, upper))
}
/// Reads and validates two copula margins, family, dependence parameter and optional degrees of freedom.
fn copula_args(a: &[Arg]) -> R<(f64, f64, Family, f64, f64)> {
    let u = probability(scalar(a, 0)?)?;
    let v = probability(scalar(a, 1)?)?;
    let f = family(a, 2)?;
    let p = scalar(a, 3)?;
    let df = finite(opt_num(a, 4, 4.0)?)?;
    validate(f, p, df)?;
    Ok((u, v, f, p, df))
}
/// Exposes the supported bivariate copula CDF with numerical-domain validation.
fn copula_cdf(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (u, v, f, p, df) = copula_args(a)?;
    num_val(copula(u, v, f, p, df)?)
}
/// Evaluates the survival copula using family-specific positive/complementary-margin identities.
/// Avoids inclusion-exclusion cancellation; a numerically unresolved positive tail returns `#NUM!`.
fn copula_joint(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (u, v, f, p, df) = copula_args(a)?;
    let left = 1.0 - u;
    let right = 1.0 - v;
    if left == 0.0 || right == 0.0 {
        return num_val(0.0);
    }
    if u == 0.0 {
        return num_val(right);
    }
    if v == 0.0 {
        return num_val(left);
    }
    let result = match f {
        Family::Independent => left * right,
        Family::Gaussian | Family::Student => elliptical_lower(left, right, f, p, df)?,
        Family::Frank => frank_cdf(left, right, p)?,
        Family::Clayton | Family::Gumbel => archimedean_survival(u, v, f, p)?,
    };
    if result == 0.0 && !(matches!(f, Family::Gaussian | Family::Student) && p == -1.0) {
        return Err(CellError::Num);
    }
    let lower = (left + right - 1.0).max(0.0);
    let upper = left.min(right);
    if !result.is_finite() || result < lower - 1e-10 || result > upper * (1.0 + 1e-9) {
        return Err(CellError::Num);
    }
    num_val(result.clamp(lower, upper))
}
/// Transforms an externally supplied uniform into a conditional copula margin.
/// Implements an inverse Rosenblatt step without adding a sampler or random-number generator.
fn copula_cond_inv(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let (u, z, f, p, df) = copula_args(a)?;
    if u == 0.0 || u == 1.0 {
        return Err(CellError::Num);
    }
    if matches!(f, Family::Gaussian | Family::Student) && p.abs() == 1.0 {
        return num_val(if p == 1.0 { u } else { 1.0 - u });
    }
    if z == 0.0 || z == 1.0 {
        return num_val(z);
    }
    let answer = match f {
        Family::Independent => z,
        Family::Frank => {
            if p.abs() < 1e-8 {
                z - 0.5 * p * z * (1.0 - z) * (1.0 - 2.0 * u)
            } else {
                // Closed conditional quantile. Positive weighted exponentials
                // provide the upper-tail logarithm without subtracting from one.
                let base = (-p * u).exp();
                let denominator = base * (1.0 - z) + z;
                let ratio = z * ((-p).exp_m1() / denominator);
                if ratio > -0.9 { -ratio.ln_1p() / p } else { (denominator.ln() - (base * (1.0 - z) + z * (-p).exp()).ln()) / p }
            }
        }
        Family::Gaussian => {
            if p == 1.0 {
                u
            } else if p == -1.0 {
                1.0 - u
            } else {
                norm_cdf(p * norm_inv(u) + (1.0 - p * p).sqrt() * norm_inv(z))
            }
        }
        Family::Student => {
            if p == 1.0 {
                u
            } else if p == -1.0 {
                1.0 - u
            } else {
                let x = student_inv(u, df)?;
                student_cdf(p * x + ((df + x * x) * (1.0 - p * p) / (df + 1.0)).sqrt() * student_inv(z, df + 1.0)?, df)?
            }
        }
        _ => {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..80 {
                let mid = (lo + hi) * 0.5;
                if conditional(u, mid, f, p, df)? < z {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) * 0.5
        }
    };
    num_val(answer)
}
/// Returns lower/upper asymptotic tail-dependence coefficients, not marginal correlations.
fn copula_tail(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let f = family(a, 0)?;
    let p = scalar(a, 1)?;
    let df = finite(opt_num(a, 2, 4.0)?)?;
    validate(f, p, df)?;
    let (lower, upper) = match f {
        Family::Independent | Family::Frank => (0.0, 0.0),
        Family::Gaussian => {
            if p == 1.0 {
                (1.0, 1.0)
            } else {
                (0.0, 0.0)
            }
        }
        Family::Student => {
            let lambda = if p == 1.0 {
                1.0
            } else if p == -1.0 {
                0.0
            } else {
                2.0 * student_cdf(-((df + 1.0) * (1.0 - p) / (1.0 + p)).sqrt(), df + 1.0)?
            };
            (lambda, lambda)
        }
        Family::Clayton => (if p == 0.0 { 0.0 } else { (-std::f64::consts::LN_2 / p).exp() }, 0.0),
        Family::Gumbel => (0.0, 2.0 - 2.0_f64.powf(1.0 / p)),
    };
    output(1, 2, vec![lower, upper])
}
/// Counts concordant, discordant and tied pairs for bounded Kendall tau-b evaluation.
/// Also reports whether ties prevent continuous-margin parameter inversion.
fn kendall(x: &[f64], y: &[f64]) -> R<(f64, bool)> {
    if x.len() != y.len() || x.len() < 2 || x.len().saturating_mul(x.len()) > MAX_PAIRS {
        return Err(CellError::Num);
    }
    let (mut concordance, mut tx, mut ty) = (0.0, 0.0, 0.0);
    let mut ties = false;
    for i in 0..x.len() {
        for j in i + 1..x.len() {
            let dx = x[i].total_cmp(&x[j]);
            let dy = y[i].total_cmp(&y[j]);
            let tied_x = x[i] == x[j];
            let tied_y = y[i] == y[j];
            if tied_x {
                tx += 1.0;
                ties = true;
            }
            if tied_y {
                ty += 1.0;
                ties = true;
            }
            if !tied_x && !tied_y {
                concordance += if dx == dy { 1.0 } else { -1.0 };
            }
        }
    }
    let pairs = x.len() as f64 * (x.len() - 1) as f64 * 0.5;
    let denominator = positive(((pairs - tx) * (pairs - ty)).sqrt())?;
    Ok((concordance / denominator, ties))
}
/// Exposes tie-adjusted Kendall tau-b for aligned observations; degenerate ranks are rejected.
fn kendall_tau(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let x = vector(a, 0, MAX_SAMPLE)?;
    let y = vector(a, 1, MAX_SAMPLE)?;
    num_val(kendall(&x, &y)?.0)
}
/// Evaluates the Frank Kendall concordance relation using a bounded Debye integral.
fn frank_tau(theta: f64) -> R<f64> {
    if theta == 0.0 {
        return Ok(0.0);
    }
    if theta < 0.0 {
        return Ok(-frank_tau(-theta)?);
    }
    if theta < 1e-3 {
        return Ok(theta / 9.0 - theta.powi(3) / 900.0 + theta.powi(5) / 52920.0);
    }
    let f = |x: f64| if x == 0.0 { Ok(1.0) } else { finite(x / x.exp_m1()) };
    Ok(1.0 - 4.0 / theta + 4.0 * integrate(&f, 0.0, theta, 1e-11)? / (theta * theta))
}
/// Fits a copula dependence parameter by continuous-margin Kendall concordance inversion.
/// Rejects ties; Student degrees of freedom are not identified or fitted by this procedure.
fn copula_fit(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let x = vector(a, 0, MAX_SAMPLE)?;
    let y = vector(a, 1, MAX_SAMPLE)?;
    let f = family(a, 2)?;
    let (tau, ties) = kendall(&x, &y)?;
    // Tau-parameter identities assume continuous margins. Report ties via a refusal, not an
    // unannounced pseudo-observation convention. ACT.KENDALL.TAU itself supports tau-b ties.
    if ties {
        return Err(CellError::Num);
    }
    let parameter = match f {
        Family::Independent => 0.0,
        Family::Gaussian | Family::Student => (std::f64::consts::FRAC_PI_2 * tau).sin(),
        Family::Clayton => {
            if tau < 0.0 || tau == 1.0 {
                return Err(CellError::Num);
            }
            2.0 * tau / (1.0 - tau)
        }
        Family::Gumbel => {
            if tau < 0.0 || tau == 1.0 {
                return Err(CellError::Num);
            }
            1.0 / (1.0 - tau)
        }
        Family::Frank => {
            if tau.abs() > frank_tau(35.0)? {
                return Err(CellError::Num);
            }
            let (mut lo, mut hi) = (0.0, 35.0);
            for _ in 0..64 {
                let mid = (lo + hi) * 0.5;
                if frank_tau(mid)? < tau.abs() {
                    lo = mid
                } else {
                    hi = mid
                };
            }
            tau.signum() * (lo + hi) * 0.5
        }
    };
    validate(f, parameter, 4.0)?;
    output(1, 3, vec![tau, parameter, x.len() as f64])
}

/// Registers Generalized Pareto inference, portfolio tails and deterministic copula transformations.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.GPD.CDF",
            3,
            3,
            Statistical,
            S,
            "ACT.GPD.CDF(excess,shape,scale)",
            "Generalized Pareto excess CDF; shape may be negative with finite upper endpoint.",
            gpd_cdf
        ),
        act!(
            "ACT.GPD.INV",
            3,
            3,
            Statistical,
            S,
            "ACT.GPD.INV(probability,shape,scale)",
            "Generalized Pareto excess quantile, including exponential limit.",
            gpd_inv
        ),
        act!(
            "ACT.GPD.MEAN",
            2,
            2,
            Statistical,
            S,
            "ACT.GPD.MEAN(shape,scale)",
            "Generalized Pareto excess mean; undefined moment gives #NUM!.",
            gpd_mean
        ),
        act!(
            "ACT.GPD.TVAR",
            3,
            3,
            Statistical,
            S,
            "ACT.GPD.TVAR(alpha,shape,scale)",
            "Generalized Pareto conditional-excess tail mean; shape must be below one.",
            gpd_tvar
        ),
        act!(
            "ACT.POT.VAR",
            5,
            5,
            Financial,
            S,
            "ACT.POT.VAR(threshold,exceedance_probability,shape,scale,alpha)",
            "Unconditional portfolio tail quantile from threshold exceedance frequency and GPD excesses.",
            pot_var
        ),
        act!(
            "ACT.POT.TVAR",
            5,
            5,
            Financial,
            S,
            "ACT.POT.TVAR(threshold,exceedance_probability,shape,scale,alpha)",
            "Portfolio tail mean above a supported peaks-over-threshold confidence level.",
            pot_tvar
        ),
        act!(
            "ACT.GPD.FIT.PWM",
            1,
            1,
            Statistical,
            &[false],
            "ACT.GPD.FIT.PWM(excesses)",
            "PWM fit: shape/scale/n/loglik/jackknife SE shape/SE scale/covariance; uncertainty blank for shape>=0.5.",
            gpd_fit_pwm
        ),
        act!(
            "ACT.GPD.FIT.MLE",
            1,
            1,
            Statistical,
            &[false],
            "ACT.GPD.FIT.MLE(excesses)",
            "Bounded regular GPD MLE (shape -0.49..2) with observed-information uncertainty; rejects boundary fits.",
            gpd_fit_mle
        ),
        act!(
            "ACT.GPD.THRESHOLDS",
            2,
            2,
            Statistical,
            &[false, false],
            "ACT.GPD.THRESHOLDS(losses,thresholds)",
            "Threshold/exceedances/fraction/mean excess/PWM shape/PWM scale; no automatic threshold selection.",
            gpd_thresholds
        ),
        act!(
            "ACT.COPULA.CDF",
            4,
            5,
            Statistical,
            S,
            "ACT.COPULA.CDF(u,v,family,parameter,[df=4])",
            "Bivariate independent/Gaussian/Student/Clayton/Gumbel/Frank copula with bounded quadrature.",
            copula_cdf
        ),
        act!(
            "ACT.COPULA.JOINTTAIL",
            4,
            5,
            Statistical,
            S,
            "ACT.COPULA.JOINTTAIL(u,v,family,parameter,[df=4])",
            "Joint probability that both uniform margins exceed supplied thresholds.",
            copula_joint
        ),
        act!(
            "ACT.COPULA.CONDITIONALINV",
            4,
            5,
            Statistical,
            S,
            "ACT.COPULA.CONDITIONALINV(u,z,family,parameter,[df=4])",
            "Inverse conditional uniform margin; deterministic Rosenblatt step from supplied u and z.",
            copula_cond_inv
        ),
        act!(
            "ACT.COPULA.TAIL",
            2,
            3,
            Statistical,
            S,
            "ACT.COPULA.TAIL(family,parameter,[df=4])",
            "Lower and upper asymptotic tail-dependence coefficients, not Pearson correlation.",
            copula_tail
        ),
        act!(
            "ACT.KENDALL.TAU",
            2,
            2,
            Statistical,
            &[false, false],
            "ACT.KENDALL.TAU(x,y)",
            "Tie-adjusted Kendall tau-b, bounded pair enumeration.",
            kendall_tau
        ),
        act!(
            "ACT.COPULA.FIT",
            3,
            3,
            Statistical,
            &[false, false, true],
            "ACT.COPULA.FIT(x,y,family)",
            "Kendall concordance inversion: tau/parameter/n; continuous margins without ties required.",
            copula_fit
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;
    fn values(v: Value) -> Vec<f64> {
        match v {
            Value::Number(x) => vec![x],
            Value::Array(a) => a.iter().map(|v| strict(v).unwrap()).collect(),
            other => panic!("{other:?}"),
        }
    }
    fn close(v: Value, want: f64, tol: f64) {
        let x = values(v)[0];
        assert!((x - want).abs() <= tol * (1.0 + want.abs()), "{x} != {want}");
    }
    fn vecarg(v: Vec<f64>) -> Arg {
        av(arr(v.into_iter().map(|x| vec![nv(x)]).collect()))
    }
    fn cop_args(u: f64, v: f64, f: &str, p: f64) -> Vec<Arg> {
        vec![n(u), n(v), t(f), n(p)]
    }
    #[test]
    fn gpd_limits_and_bounded_support() {
        close(ev("ACT.GPD.CDF", vec![n(2.0), n(0.0), n(2.0)]), 1.0 - (-1.0_f64).exp(), 1e-12);
        close(ev("ACT.GPD.INV", vec![n(0.95), n(0.0), n(2.0)]), -2.0 * 0.05_f64.ln(), 1e-12);
        close(ev("ACT.GPD.CDF", vec![n(5.0), n(-0.5), n(2.0)]), 1.0, 1e-12);
        close(ev("ACT.GPD.INV", vec![n(1.0), n(-0.5), n(2.0)]), 4.0, 1e-12);
        close(ev("ACT.GPD.TVAR", vec![n(0.95), n(0.0), n(2.0)]), -2.0 * 0.05_f64.ln() + 2.0, 1e-12);
        assert_eq!(ev("ACT.GPD.MEAN", vec![n(1.0), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.GPD.TVAR", vec![n(0.5), n(1.0), n(1.0)]), Value::Error(CellError::Num));
    }
    #[test]
    fn pot_uses_unconditional_exceedance_frequency() {
        close(ev("ACT.POT.VAR", vec![n(100.0), n(0.1), n(0.0), n(20.0), n(0.99)]), 100.0 + 20.0 * 10.0_f64.ln(), 1e-12);
        close(ev("ACT.POT.TVAR", vec![n(100.0), n(0.1), n(0.0), n(20.0), n(0.99)]), 120.0 + 20.0 * 10.0_f64.ln(), 1e-12);
        assert_eq!(ev("ACT.POT.VAR", vec![n(100.0), n(0.1), n(0.0), n(20.0), n(0.8)]), Value::Error(CellError::Num));
    }
    #[test]
    fn gpd_parameter_recovery_on_deterministic_quantile_grid() {
        let sample: Vec<f64> = (0..256).map(|i| gp_quantile((i as f64 + 0.5) / 256.0, 0.15, 2.0).unwrap()).collect();
        let fit = values(ev("ACT.GPD.FIT.PWM", vec![vecarg(sample.clone())]));
        assert!((fit[0] - 0.15).abs() < 0.04, "{fit:?}");
        assert!((fit[1] - 2.0).abs() < 0.08, "{fit:?}");
        assert!(fit[4] > 0.0 && fit[5] > 0.0);
        let mle = values(ev("ACT.GPD.FIT.MLE", vec![vecarg(sample)]));
        assert!((mle[0] - 0.15).abs() < 0.04, "{mle:?}");
        assert!((mle[1] - 2.0).abs() < 0.08, "{mle:?}");
        assert!(mle[4] > 0.0 && mle[5] > 0.0);
    }
    #[test]
    fn copula_margins_and_known_dependence() {
        for (f, p) in [("independent", 0.0), ("gaussian", 0.5), ("student", 0.5), ("clayton", 2.0), ("gumbel", 2.0), ("frank", 3.0)] {
            close(ev("ACT.COPULA.CDF", cop_args(1.0, 0.3, f, p)), 0.3, 1e-10);
            close(ev("ACT.COPULA.CDF", cop_args(0.0, 0.3, f, p)), 0.0, 1e-10);
        }
        close(ev("ACT.COPULA.CDF", cop_args(0.5, 0.5, "gaussian", 0.5)), 1.0 / 3.0, 1e-9);
        close(ev("ACT.COPULA.CDF", cop_args(0.5, 0.5, "student", 0.5)), 1.0 / 3.0, 1e-9);
        close(ev("ACT.COPULA.CDF", cop_args(0.5, 0.5, "clayton", 2.0)), 1.0 / 7.0_f64.sqrt(), 1e-12);
        close(ev("ACT.COPULA.CDF", cop_args(0.5, 0.5, "gumbel", 2.0)), (-2.0_f64.sqrt() * std::f64::consts::LN_2).exp(), 1e-12);
        close(ev("ACT.COPULA.JOINTTAIL", cop_args(0.9, 0.9, "independent", 0.0)), 0.01, 1e-12);
    }
    #[test]
    fn student_is_not_gaussian_or_independent_and_tail_coefficients_match() {
        let student = values(ev("ACT.COPULA.CDF", cop_args(0.8, 0.8, "student", 0.0)))[0];
        assert!(student > 0.64, "{student}");
        let tail = values(ev("ACT.COPULA.TAIL", vec![t("student"), n(0.5), n(4.0)]));
        assert!((tail[0] - 0.2531699951003227).abs() < 1e-10, "{tail:?}");
        assert_eq!(tail[0], tail[1]);
        close(ev("ACT.COPULA.TAIL", vec![t("clayton"), n(2.0)]), 1.0 / 2.0_f64.sqrt(), 1e-12);
        let g = values(ev("ACT.COPULA.TAIL", vec![t("gumbel"), n(2.0)]));
        assert!((g[1] - (2.0 - 2.0_f64.sqrt())).abs() < 1e-12);
    }
    #[test]
    fn joint_extreme_tails_do_not_cancel_to_false_zero() {
        let u = 0.999999999;
        // Independently evaluated with Python decimal precision75 using exact f64 inputs.
        for (family, param, expected) in [
            ("independent", 0.0, 9.999999434361379e-19),
            ("gaussian", 0.0, 9.999999434361379e-19),
            ("clayton", 2.0, 2.999999824308414e-18),
            ("gumbel", 2.0, 5.857864213526263e-10),
            ("frank", 5.0, 5.033917964624073e-18),
        ] {
            let actual = values(ev("ACT.COPULA.JOINTTAIL", cop_args(u, u, family, param)))[0];
            assert!(actual > 0.0, "{family}: false zero");
            assert!((actual / expected - 1.0).abs() < 2e-8, "{family}: {actual} != {expected}");
        }
        let gaussian = values(ev("ACT.COPULA.JOINTTAIL", cop_args(u, u, "gaussian", 0.5)))[0];
        assert!(gaussian > 1e-18 && gaussian < 1.0 - u);
        let student = values(ev("ACT.COPULA.JOINTTAIL", cop_args(u, u, "student", 0.5)))[0];
        assert!(student > (1.0 - u) * 0.15 && student < (1.0 - u) * 0.4, "{student}");
        // A numerically unresolvable negative-correlation tail must be refused, not zeroed.
        let unresolved = ev("ACT.COPULA.JOINTTAIL", cop_args(u, u, "gaussian", -0.5));
        assert!(matches!(unresolved, Value::Error(CellError::Num)) || matches!(unresolved,Value::Number(x) if x>0.0));
        let negative_frank = values(ev("ACT.COPULA.JOINTTAIL", cop_args(u, u, "frank", -35.0)))[0];
        assert!(negative_frank > 0.0 && negative_frank < 1e-18);
    }
    #[test]
    fn inverse_rosenblatt_roundtrips_conditional_probabilities() {
        for (f, p) in [("independent", 0.0), ("gaussian", 0.6), ("student", 0.6), ("clayton", 2.0), ("gumbel", 2.0), ("frank", 5.0), ("frank", -5.0)]
        {
            let inv = values(ev("ACT.COPULA.CONDITIONALINV", cop_args(0.3, 0.7, f, p)))[0];
            let fam = family(&[t(f)], 0).unwrap();
            assert!((conditional(0.3, inv, fam, p, 4.0).unwrap() - 0.7).abs() < 1e-8, "{f}: {inv}");
        }
    }
    #[test]
    fn frank_independence_limit_is_continuous_for_both_tiny_parameter_signs() {
        for theta in [1e-200, -1e-200, f64::from_bits(1), -f64::from_bits(1)] {
            for (u, v) in [(0.2, 0.3), (0.3, 0.8), (0.999999999, 0.999999999)] {
                let actual = values(ev("ACT.COPULA.CDF", cop_args(u, v, "frank", theta)))[0];
                assert!((actual / (u * v) - 1.0).abs() < 1e-14, "theta={theta} C={actual}");
                let tail = values(ev("ACT.COPULA.JOINTTAIL", cop_args(u, v, "frank", theta)))[0];
                assert!((tail / ((1.0 - u) * (1.0 - v)) - 1.0).abs() < 1e-14, "theta={theta} tail={tail}");
            }
            for z in [0.7, 1e-200] {
                let value = values(ev("ACT.COPULA.CONDITIONALINV", cop_args(0.2, z, "frank", theta)))[0];
                assert!((value / z - 1.0).abs() < 1e-14, "theta={theta} inverse={value}");
                assert!((conditional(0.2, value, Family::Frank, theta, 4.0).unwrap() / z - 1.0).abs() < 1e-14);
            }
        }
    }
    #[test]
    fn kendall_handles_ties_while_continuous_copula_fit_rejects_them() {
        close(ev("ACT.KENDALL.TAU", vec![vecarg(vec![1.0, 1.0, 2.0]), vecarg(vec![1.0, 2.0, 3.0])]), 2.0 / 6.0_f64.sqrt(), 1e-12);
        assert_eq!(ev("ACT.COPULA.FIT", vec![vecarg(vec![1.0, 1.0, 2.0]), vecarg(vec![1.0, 2.0, 3.0]), t("clayton")]), Value::Error(CellError::Num));
        let fit = values(ev("ACT.COPULA.FIT", vec![vecarg(vec![1.0, 2.0, 3.0]), vecarg(vec![1.0, 3.0, 2.0]), t("clayton")]));
        assert!((fit[0] - 1.0 / 3.0).abs() < 1e-12);
        assert!((fit[1] - 1.0).abs() < 1e-12);
    }
    #[test]
    fn invalid_data_and_domains_are_rejected() {
        assert_eq!(ev("ACT.GPD.FIT.PWM", vec![vecarg(vec![1.0; 8])]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.GPD.INV", vec![n(1.0), n(0.0), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COPULA.CDF", cop_args(0.5, 0.5, "gumbel", 0.5)), Value::Error(CellError::Num));
        let mut args = cop_args(0.5, 0.5, "student", 0.5);
        args.push(n(1.0));
        assert_eq!(ev("ACT.COPULA.CDF", args), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.COPULA.CONDITIONALINV", cop_args(0.0, 0.5, "gaussian", 0.5)), Value::Error(CellError::Num));
    }

    #[test]
    fn skipped_student_degrees_of_freedom_use_four_but_explicit_zero_is_invalid() {
        let default = ev("ACT.COPULA.TAIL", vec![t("student"), n(0.5)]);
        let skipped = ev("ACT.COPULA.TAIL", vec![t("student"), n(0.5), empty()]);
        assert_eq!(default, skipped);
        let Value::Array(values) = skipped else {
            panic!("expected tail coefficients");
        };
        close(values.data[0].clone(), 0.2531699951003227, 1e-12);
        assert_eq!(ev("ACT.COPULA.TAIL", vec![t("student"), n(0.5), Arg::reference(Value::Empty)]), Value::Error(CellError::Num));
        assert_eq!(
            ev("ACT.COPULA.CDF", vec![n(0.5), n(0.5), t("student"), n(0.5), empty()]),
            ev("ACT.COPULA.CDF", vec![n(0.5), n(0.5), t("student"), n(0.5)])
        );
    }
}
