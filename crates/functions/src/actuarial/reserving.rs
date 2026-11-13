//! Deterministic claims reserving, credibility and log-link generalized linear models.
//!
//! Mathematical conventions and spill layouts are part of the public worksheet contract.
//! Cumulative triangles have n rows/columns, n-i observed numeric cells in row i and truly
//! empty remaining cells. Mack uses alpha=1, independent origin years and no tail factor.

use gridcraft_core::{CellError, Value};

use super::optional_num as opt_num;

use crate::util::{R, S, arg, array_val, as_array, has, num, num_val, text};
use crate::{Arg, Ctx, FnSpec};

const MAX_TRIANGLE: usize = 128;
const MAX_OBSERVATIONS: usize = 4096;
const MAX_REGRESSORS: usize = 16;
const MAX_CREDIBILITY_CELLS: usize = 65_536;

/// Register claim-development, credibility and log-link pricing worksheet functions and range masks.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.CHAINLADDER.FACTORS",
            1,
            1,
            Financial,
            &[false],
            "ACT.CHAINLADDER.FACTORS(triangle)",
            "Volume-weighted cumulative development factors (one row).",
            chain_factors
        ),
        act!(
            "ACT.CHAINLADDER",
            1,
            2,
            Financial,
            &[false, true],
            "ACT.CHAINLADDER(triangle, [tail_factor])",
            "Per-origin latest, ultimate and reserve; deterministic tail defaults to one.",
            chain_ladder
        ),
        act!(
            "ACT.BORNHUETTERFERGUSON",
            2,
            3,
            Financial,
            &[false, false, true],
            "ACT.BORNHUETTERFERGUSON(triangle, expected_ultimate, [tail_factor])",
            "Per-origin latest, BF ultimate and reserve from aligned prior ultimate claims.",
            bornhuetter_ferguson
        ),
        act!(
            "ACT.MACK",
            1,
            2,
            Financial,
            &[false, true],
            "ACT.MACK(triangle, [last_sigma2])",
            "Mack latest/ultimate/reserve/process SE/parameter SE/total SE, with final total row.",
            mack
        ),
        act!(
            "ACT.CREDIBILITY",
            3,
            3,
            Financial,
            S,
            "ACT.CREDIBILITY(exposure, epv, vhm)",
            "Buhlmann credibility weight from exposure and supplied variance components.",
            credibility
        ),
        act!(
            "ACT.BUHLMANNSTRAUB",
            2,
            2,
            Financial,
            &[false, false],
            "ACT.BUHLMANNSTRAUB(experience, exposure)",
            "Group mean/exposure/Z/credibility mean/EPV/VHM using moment estimates.",
            buhlmann_straub
        ),
        act!(
            "ACT.GROSSPREMIUM",
            4,
            4,
            Financial,
            S,
            "ACT.GROSSPREMIUM(pure_premium, fixed_expense, expense_ratio, profit_ratio)",
            "Gross premium with fixed expenses and proportional expenses/profit.",
            gross_premium
        ),
        act!(
            "ACT.GLM.FIT",
            3,
            5,
            Statistical,
            &[false, false, true, false, false],
            "ACT.GLM.FIT(y, x, family, [offset], [weights])",
            "Poisson or Gamma log-link coefficients; supply an intercept column explicitly.",
            glm_fit
        ),
        act!(
            "ACT.GLM.PREDICT",
            2,
            3,
            Statistical,
            &[false, false, false],
            "ACT.GLM.PREDICT(x, coefficients, [offset])",
            "Log-link expected responses from an explicit design matrix and coefficients.",
            glm_predict
        ),
        act!(
            "ACT.GLM.DEVIANCE",
            3,
            4,
            Statistical,
            &[false, false, true, false],
            "ACT.GLM.DEVIANCE(y, mean, family, [weights])",
            "Weighted unit deviance for Poisson or Gamma log-link models.",
            glm_deviance
        ),
    ]
}

/// Require a finite scalar or return `#NUM!` for an unsupported numerical result.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}

/// Validate a finite nonnegative claim, variance component or loading parameter.
fn nonnegative(x: f64) -> R<f64> {
    let x = finite(x)?;
    if x >= 0.0 { Ok(x) } else { Err(CellError::Num) }
}

/// Read an actual numerical cell while preserving errors and rejecting blanks/text instead of dropping them.
fn strict(v: &Value) -> R<f64> {
    match v {
        Value::Number(x) => finite(*x),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}

/// Neumaier summation; a nonfinite intermediate is rejected rather than spilling NaN.
fn sum(xs: impl IntoIterator<Item = f64>) -> R<f64> {
    let (mut s, mut c) = (0.0_f64, 0.0_f64);
    for x in xs {
        finite(x)?;
        let t = finite(s + x)?;
        c += if s.abs() >= x.abs() { (s - t) + x } else { (x - t) + s };
        s = t;
    }
    finite(s + c)
}

/// Read a bounded strict numerical row/column vector without changing observation alignment.
fn vector(a: &[Arg], i: usize, cap: usize) -> R<Vec<f64>> {
    let data = as_array(&arg(a, i)?.value);
    if data.rows > 1 && data.cols > 1 {
        return Err(CellError::Value);
    }
    if data.data.is_empty() || data.data.len() > cap {
        return Err(CellError::Num);
    }
    data.data.iter().map(strict).collect()
}

/// Build a numerical result array after checking that every output is finite.
fn spill(rows: usize, cols: usize, xs: Vec<f64>) -> R<Value> {
    if xs.iter().any(|x| !x.is_finite()) {
        return Err(CellError::Num);
    }
    array_val(rows, cols, xs.into_iter().map(Value::Number).collect())
}

struct Triangle {
    rows: Vec<Vec<f64>>,
    factors: Vec<f64>,
    sums: Vec<f64>,
}

impl Triangle {
    /// Validate an upper-left observed cumulative triangle and estimate volume-weighted development factors.
    /// The lower-right cells must be truly blank; zero factor denominators are rejected.
    fn parse(a: &[Arg]) -> R<Self> {
        let arr = as_array(&arg(a, 0)?.value);
        let n = arr.rows;
        if !(2..=MAX_TRIANGLE).contains(&n) || arr.cols != n {
            return Err(CellError::Num);
        }
        let mut rows = Vec::with_capacity(n);
        for i in 0..n {
            let mut row = Vec::with_capacity(n - i);
            for j in 0..n {
                let v = arr.data.get(i * n + j).ok_or(CellError::Value)?;
                if j < n - i {
                    row.push(nonnegative(strict(v)?)?);
                } else if !matches!(v, Value::Empty) {
                    return Err(CellError::Value);
                }
            }
            rows.push(row);
        }
        let mut factors = Vec::with_capacity(n - 1);
        let mut sums = Vec::with_capacity(n - 1);
        for j in 0..n - 1 {
            let denominator = sum(rows.iter().take(n - j - 1).map(|r| r[j]))?;
            let numerator = sum(rows.iter().take(n - j - 1).map(|r| r[j + 1]))?;
            if denominator <= 0.0 {
                return Err(CellError::Num);
            }
            factors.push(finite(numerator / denominator)?);
            sums.push(denominator);
        }
        Ok(Self { rows, factors, sums })
    }

    /// Project each latest cumulative claim to ultimate using fitted factors and a supplied deterministic tail.
    fn projections(&self, tail: f64) -> R<Vec<(f64, f64)>> {
        self.rows
            .iter()
            .map(|row| {
                let latest = *row.last().ok_or(CellError::Value)?;
                let mut ultimate = finite(latest * tail)?;
                for factor in self.factors.iter().skip(row.len() - 1) {
                    ultimate = finite(ultimate * factor)?;
                }
                Ok((latest, ultimate))
            })
            .collect()
    }
}

/// Read a finite deterministic tail factor of at least one, defaulting to no additional development.
fn tail(a: &[Arg], i: usize) -> R<f64> {
    let tail = finite(opt_num(a, i, 1.0)?)?;
    if tail < 1.0 { Err(CellError::Num) } else { Ok(tail) }
}

/// Spill one row of volume-weighted cumulative development factors from a strict claim triangle.
fn chain_factors(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let t = Triangle::parse(a)?;
    spill(1, t.factors.len(), t.factors)
}

/// Spill latest claims, projected ultimate and reserve for each origin year.
fn chain_ladder(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let t = Triangle::parse(a)?;
    let mut out = Vec::with_capacity(t.rows.len() * 3);
    for (latest, ultimate) in t.projections(tail(a, 1)?)? {
        out.extend([latest, ultimate, finite(ultimate - latest)?]);
    }
    spill(t.rows.len(), 3, out)
}

/// Spill latest, BF ultimate and reserve from aligned prior ultimate claims.
/// Unreported proportions use development factors of at least one and the supplied deterministic tail.
fn bornhuetter_ferguson(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let t = Triangle::parse(a)?;
    let prior = vector(a, 1, MAX_TRIANGLE)?;
    if prior.len() != t.rows.len() || t.factors.iter().any(|f| *f < 1.0) {
        return Err(CellError::Num);
    }
    let tail = tail(a, 2)?;
    let mut out = Vec::with_capacity(t.rows.len() * 3);
    for (row, expected) in t.rows.iter().zip(prior) {
        nonnegative(expected)?;
        let mut development = tail;
        for factor in t.factors.iter().skip(row.len() - 1) {
            development = finite(development * factor)?;
        }
        let latest = *row.last().ok_or(CellError::Value)?;
        let reserve = finite(expected * (1.0 - development.recip()))?;
        out.extend([latest, finite(latest + reserve)?, reserve]);
    }
    spill(t.rows.len(), 3, out)
}

/// Spill per-origin Mack process/parameter uncertainty and an aggregate row with shared-factor covariance.
/// Positive observations and identifiable dispersion are required; short triangles need explicit final sigma squared.
fn mack(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let t = Triangle::parse(a)?;
    let n = t.rows.len();
    if t.rows.iter().flatten().any(|v| *v <= 0.0) || t.factors.iter().any(|f| *f <= 0.0) {
        return Err(CellError::Num);
    }
    let mut sigma = Vec::with_capacity(n - 1);
    for j in 0..n - 2 {
        let count = n - j - 1;
        let residual = sum(t.rows.iter().take(count).map(|r| {
            let d = r[j + 1] / r[j] - t.factors[j];
            r[j] * d * d
        }))?;
        sigma.push(finite(residual / (count - 1) as f64)?);
    }
    let last = if has(a, 1) {
        nonnegative(num(a, 1)?)?
    } else if n >= 4 {
        let previous = *sigma.last().ok_or(CellError::Num)?;
        let before = *sigma.get(sigma.len() - 2).ok_or(CellError::Num)?;
        if before == 0.0 {
            if previous == 0.0 {
                0.0
            } else {
                return Err(CellError::Num);
            }
        } else {
            // If previous>=before, the quadratic candidate cannot be the minimum.
            // Otherwise previous*(previous/before) is finite and no larger than previous.
            if previous >= before { before } else { finite(previous * (previous / before))? }
        }
    } else {
        // A 2/3-row triangle has insufficient degrees of freedom for Mack's extrapolation.
        return Err(CellError::Num);
    };
    sigma.push(last);
    let projections = t.projections(1.0)?;
    let mut processes = Vec::with_capacity(n);
    let mut parameters = Vec::with_capacity(n);
    let mut out = Vec::with_capacity((n + 1) * 6);
    for (i, row) in t.rows.iter().enumerate() {
        let (latest, ultimate) = projections[i];
        let (mut process, mut parameter) = (0.0, 0.0);
        let mut current = latest;
        for (j, factor) in t.factors.iter().enumerate().skip(row.len() - 1) {
            let relative = finite(sigma[j] / factor / factor)?;
            // Divide before multiplying by ultimate to avoid unnecessary squared overflow.
            process = finite(process + finite((ultimate / current) * ultimate * relative)?)?;
            parameter = finite(parameter + finite((ultimate / t.sums[j]) * ultimate * relative)?)?;
            current = finite(current * factor)?;
        }
        processes.push(process);
        parameters.push(parameter);
        out.extend([latest, ultimate, finite(ultimate - latest)?, process.sqrt(), parameter.sqrt(), finite(process + parameter)?.sqrt()]);
    }
    let mut total_parameter = sum(parameters)?;
    // Common estimated factors induce cross-origin covariance, despite independent origins.
    for i in 0..n {
        for k in i + 1..n {
            let start = (t.rows[i].len() - 1).max(t.rows[k].len() - 1);
            for (j, factor) in t.factors.iter().enumerate().skip(start) {
                let covariance = finite((projections[i].1 / t.sums[j]) * projections[k].1 * (sigma[j] / factor / factor))?;
                total_parameter = finite(total_parameter + 2.0 * covariance)?;
            }
        }
    }
    let total_process = sum(processes)?;
    let latest = sum(projections.iter().map(|p| p.0))?;
    let ultimate = sum(projections.iter().map(|p| p.1))?;
    out.extend([
        latest,
        ultimate,
        finite(ultimate - latest)?,
        total_process.sqrt(),
        total_parameter.sqrt(),
        finite(total_process + total_parameter)?.sqrt(),
    ]);
    spill(n + 1, 6, out)
}

/// Compute a Bühlmann exposure weight from EPV/VHM with stable logarithmic odds and zero limits.
fn credibility_weight(exposure: f64, epv: f64, vhm: f64) -> R<f64> {
    nonnegative(exposure)?;
    nonnegative(epv)?;
    nonnegative(vhm)?;
    if exposure == 0.0 || vhm == 0.0 {
        return Ok(0.0);
    }
    if epv == 0.0 {
        return Ok(1.0);
    }
    // Logistic form avoids overflow in EPV/VHM and exposure*VHM.
    let log_odds = epv.ln() - exposure.ln() - vhm.ln();
    Ok(if log_odds >= 0.0 {
        let r = (-log_odds).exp();
        r / (1.0 + r)
    } else {
        1.0 / (1.0 + log_odds.exp())
    })
}

/// Return the Bühlmann credibility weight for supplied exposure and variance components.
fn credibility(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(credibility_weight(num(a, 0)?, num(a, 1)?, num(a, 2)?)?)
}

/// Estimate weighted Bühlmann–Straub moments and spill group mean/exposure/Z/estimate/EPV/VHM.
/// The rectangular experience/exposure matrices must be fully observed with strictly positive exposures.
fn buhlmann_straub(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let values = as_array(&arg(a, 0)?.value);
    let weights = as_array(&arg(a, 1)?.value);
    let (groups, periods) = (values.rows, values.cols);
    if groups < 2
        || periods < 2
        || groups.checked_mul(periods).is_none_or(|n| n > MAX_CREDIBILITY_CELLS)
        || weights.rows != groups
        || weights.cols != periods
    {
        return Err(CellError::Num);
    }
    let mut means = Vec::with_capacity(groups);
    let mut exposures = Vec::with_capacity(groups);
    let mut residuals = Vec::with_capacity(groups);
    for i in 0..groups {
        let start = i * periods;
        let vr = values.data.get(start..start + periods).ok_or(CellError::Value)?;
        let wr = weights.data.get(start..start + periods).ok_or(CellError::Value)?;
        let v: Vec<f64> = vr.iter().map(strict).collect::<R<_>>()?;
        let w: Vec<f64> = wr.iter().map(strict).collect::<R<_>>()?;
        if v.iter().any(|x| *x < 0.0) || w.iter().any(|x| *x <= 0.0) {
            return Err(CellError::Num);
        }
        let exposure = sum(w.iter().copied())?;
        let mean = sum(v.iter().zip(&w).map(|(x, w)| x * (w / exposure)))?;
        let residual = sum(v.iter().zip(&w).map(|(x, w)| w * (x - mean) * (x - mean)))?;
        exposures.push(exposure);
        means.push(mean);
        residuals.push(residual);
    }
    let total = sum(exposures.iter().copied())?;
    let pooled = sum(means.iter().zip(&exposures).map(|(x, w)| x * (w / total)))?;
    let epv = finite(sum(residuals)? / (groups * (periods - 1)) as f64)?;
    let between = sum(means.iter().zip(&exposures).map(|(x, w)| w * (x - pooled) * (x - pooled)))?;
    let correction = sum(exposures.iter().map(|w| w * (w / total)))?;
    let denominator = finite(total - correction)?;
    if denominator <= 0.0 {
        return Err(CellError::Num);
    }
    let vhm = finite((between - (groups - 1) as f64 * epv) / denominator)?.max(0.0);
    let z: Vec<f64> = exposures.iter().map(|w| credibility_weight(*w, epv, vhm)).collect::<R<_>>()?;
    let z_total = sum(z.iter().copied())?;
    let collective = if z_total == 0.0 { pooled } else { sum(z.iter().zip(&means).map(|(z, x)| (z / z_total) * x))? };
    let mut out = Vec::with_capacity(groups * 6);
    for i in 0..groups {
        let estimate = finite(z[i] * means[i] + (1.0 - z[i]) * collective)?;
        out.extend([means[i], exposures[i], z[i], estimate, epv, vhm]);
    }
    spill(groups, 6, out)
}

/// Gross up pure premium plus fixed expense for proportional expense/profit loadings.
/// A nonpositive remaining premium share is outside the model domain.
fn gross_premium(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let pure = nonnegative(num(a, 0)?)?;
    let fixed = nonnegative(num(a, 1)?)?;
    let expenses = nonnegative(num(a, 2)?)?;
    let profit = nonnegative(num(a, 3)?)?;
    let denominator = 1.0 - expenses - profit;
    if denominator <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((pure + fixed) / denominator)
}

#[derive(Clone, Copy)]
enum Family {
    Poisson,
    Gamma,
}

/// Select the supported Poisson or Gamma response family with a log link.
fn family(a: &[Arg], i: usize) -> R<Family> {
    match text(a, i)?.trim().to_ascii_lowercase().as_str() {
        "poisson" => Ok(Family::Poisson),
        "gamma" => Ok(Family::Gamma),
        _ => Err(CellError::Value),
    }
}

/// Read an aligned optional offset/weight vector, using a constant vector only when omitted.
fn aligned(a: &[Arg], i: usize, n: usize, default: f64) -> R<Vec<f64>> {
    if !has(a, i) {
        return Ok(vec![default; n]);
    }
    let v = vector(a, i, MAX_OBSERVATIONS)?;
    if v.len() == n { Ok(v) } else { Err(CellError::Value) }
}

/// Validate a bounded finite numerical design matrix; callers supply any intercept explicitly.
fn design(a: &[Arg], i: usize) -> R<(usize, usize, Vec<f64>)> {
    let x = as_array(&arg(a, i)?.value);
    if x.rows == 0 || x.rows > MAX_OBSERVATIONS || x.cols == 0 || x.cols > MAX_REGRESSORS {
        return Err(CellError::Num);
    }
    Ok((x.rows, x.cols, x.data.iter().map(strict).collect::<R<_>>()?))
}

/// Enforce nonnegative Poisson responses or strictly positive Gamma responses.
fn response_domain(y: &[f64], family: Family) -> R<()> {
    if y.iter().any(|v| match family {
        Family::Poisson => *v < 0.0,
        Family::Gamma => *v <= 0.0,
    }) {
        return Err(CellError::Num);
    }
    Ok(())
}

/// Compute compensated weighted unit deviance for supported Poisson/Gamma means.
/// Nonpositive means or nonfinite intermediate values invalidate the result.
fn deviance(y: &[f64], mu: &[f64], weights: &[f64], family: Family) -> R<f64> {
    sum(y.iter().zip(mu).zip(weights).map(|((y, mu), weight)| {
        if *mu <= 0.0 || !mu.is_finite() {
            return f64::NAN;
        }
        let value = match family {
            Family::Poisson => {
                if *y == 0.0 {
                    2.0 * mu
                } else {
                    let relative = (y - mu) / mu;
                    // log1p is accurate for y approximately equal to mu.
                    let log_ratio = if relative > -1.0 { relative.ln_1p() } else { y.ln() - mu.ln() };
                    2.0 * (y * log_ratio - (y - mu))
                }
            }
            Family::Gamma => {
                let relative = (y - mu) / mu;
                let log_ratio = if relative > -1.0 { relative.ln_1p() } else { y.ln() - mu.ln() };
                2.0 * (relative - log_ratio)
            }
        };
        weight * value.max(0.0)
    }))
}

/// Evaluate log-link means as exp(X beta + offset), rejecting overflow and underflow to zero.
fn predicted(x: &[f64], p: usize, beta: &[f64], offset: &[f64]) -> R<Vec<f64>> {
    let mut mu = Vec::with_capacity(offset.len());
    for (row, offset) in x.chunks_exact(p).zip(offset) {
        let eta = finite(sum(row.iter().zip(beta).map(|(x, b)| x * b))? + offset)?;
        let expected = finite(eta.exp())?;
        if expected <= 0.0 {
            return Err(CellError::Num);
        }
        mu.push(expected);
    }
    Ok(mu)
}

/// Reorthogonalized modified Gram-Schmidt on column-scaled weighted design.
/// Rank failure yields #NUM; there is no hidden ridge or dropped design column.
fn weighted_qr(x: &[f64], n: usize, p: usize, w: &[f64], z: &[f64]) -> R<Vec<f64>> {
    let max_weight = w.iter().copied().fold(0.0_f64, f64::max);
    if max_weight <= 0.0 || !max_weight.is_finite() {
        return Err(CellError::Num);
    }
    let root_w: Vec<f64> = w.iter().map(|v| (v / max_weight).sqrt()).collect();
    let mut scales = Vec::with_capacity(p);
    let mut q = vec![vec![0.0; n]; p];
    for j in 0..p {
        let scale = x.chunks_exact(p).map(|row| row[j].abs()).fold(0.0_f64, f64::max);
        if scale == 0.0 {
            return Err(CellError::Num);
        }
        scales.push(scale);
        for i in 0..n {
            q[j][i] = finite((x[i * p + j] / scale) * root_w[i])?;
        }
    }
    let mut r = vec![vec![0.0; p]; p];
    let mut rhs = vec![0.0; p];
    for j in 0..p {
        // A second pass prevents loss of orthogonality in strongly correlated designs.
        for _ in 0..2 {
            for k in 0..j {
                let projection = sum(q[k].iter().zip(&q[j]).map(|(a, b)| a * b))?;
                r[k][j] = finite(r[k][j] + projection)?;
                let (previous, current) = q.split_at_mut(j);
                for (value, prior) in current[0].iter_mut().zip(&previous[k]) {
                    *value = finite(*value - projection * prior)?;
                }
            }
        }
        let norm = sum(q[j].iter().map(|v| v * v))?.sqrt();
        if norm <= 1e-12 {
            return Err(CellError::Num);
        }
        r[j][j] = norm;
        for v in &mut q[j] {
            *v /= norm;
        }
        rhs[j] = sum(q[j].iter().zip(z).zip(&root_w).map(|((q, z), w)| q * (z * w)))?;
    }
    let mut beta = vec![0.0; p];
    for j in (0..p).rev() {
        let known = sum((j + 1..p).map(|k| r[j][k] * beta[k]))?;
        beta[j] = finite((rhs[j] - known) / r[j][j])?;
    }
    for j in 0..p {
        beta[j] = finite(beta[j] / scales[j])?;
    }
    Ok(beta)
}

/// Fit Poisson/Gamma log-link coefficients using bounded IRLS, scaled QR and step halving.
/// Singular designs and unconverged fits return errors; no hidden ridge or dropped columns are used.
fn glm_fit(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    let y = vector(a, 0, MAX_OBSERVATIONS)?;
    let (n, p, x) = design(a, 1)?;
    if y.len() != n || n < p {
        return Err(CellError::Value);
    }
    let family = family(a, 2)?;
    response_domain(&y, family)?;
    let offset = aligned(a, 3, n, 0.0)?;
    let weights = aligned(a, 4, n, 1.0)?;
    if weights.iter().any(|w| *w <= 0.0) {
        return Err(CellError::Num);
    }
    let mut beta = vec![0.0; p];
    let mut mu = predicted(&x, p, &beta, &offset)?;
    let mut dev = deviance(&y, &mu, &weights, family)?;
    for _ in 0..100 {
        super::work::charge(ctx, super::work::product(&[n as u64, p as u64, p as u64, 32]))?;
        let working_weights: Vec<f64> = weights
            .iter()
            .zip(&mu)
            .map(|(w, mu)| {
                finite(match family {
                    Family::Poisson => w * mu,
                    Family::Gamma => *w,
                })
            })
            .collect::<R<_>>()?;
        let z: Vec<f64> = mu.iter().zip(&y).zip(&offset).map(|((mu, y), offset)| finite(mu.ln() - offset + (y - mu) / mu)).collect::<R<_>>()?;
        let next = weighted_qr(&x, n, p, &working_weights, &z)?;
        let mut step = 1.0;
        let mut accepted = None;
        for _ in 0..30 {
            super::work::charge(ctx, super::work::product(&[n as u64, p as u64, 32]))?;
            let candidate: Vec<f64> = beta.iter().zip(&next).map(|(b, next)| b + step * (next - b)).collect();
            if let Ok(candidate_mu) = predicted(&x, p, &candidate, &offset)
                && let Ok(candidate_dev) = deviance(&y, &candidate_mu, &weights, family)
                && candidate_dev <= dev + 1e-12 * (1.0 + dev)
            {
                accepted = Some((candidate, candidate_mu, candidate_dev));
                break;
            }
            step *= 0.5;
        }
        let (next, next_mu, next_dev) = accepted.ok_or(CellError::Num)?;
        let change = beta.iter().zip(&next).map(|(b, next)| (next - b).abs() / (1.0 + b.abs())).fold(0.0_f64, f64::max);
        let objective_change = (next_dev - dev).abs() / (1.0 + dev);
        beta = next;
        mu = next_mu;
        dev = next_dev;
        if change < 1e-9 && objective_change < 1e-10 {
            // A clipped line-search step alone cannot establish convergence: require the full
            // Fisher step to have converged too (not merely a numerically tiny accepted step).
            let full_step = weighted_qr(&x, n, p, &working_weights, &z)?;
            let full_change = beta.iter().zip(full_step).map(|(b, full)| (full - b).abs() / (1.0 + b.abs())).fold(0.0_f64, f64::max);
            if full_change < 1e-8 {
                return spill(p, 1, beta);
            }
        }
    }
    Err(CellError::Num)
}

/// Spill fitted log-link means from an explicit design matrix, coefficients and optional offsets.
fn glm_predict(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (n, p, x) = design(a, 0)?;
    let beta = vector(a, 1, MAX_REGRESSORS)?;
    if beta.len() != p {
        return Err(CellError::Value);
    }
    let offset = aligned(a, 2, n, 0.0)?;
    spill(n, 1, predicted(&x, p, &beta, &offset)?)
}

/// Return weighted unit deviance for aligned observed and predicted responses.
fn glm_deviance(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let y = vector(a, 0, MAX_OBSERVATIONS)?;
    let mu = vector(a, 1, MAX_OBSERVATIONS)?;
    if y.len() != mu.len() {
        return Err(CellError::Value);
    }
    let family = family(a, 2)?;
    response_domain(&y, family)?;
    let weights = aligned(a, 3, y.len(), 1.0)?;
    if weights.iter().any(|w| *w <= 0.0) {
        return Err(CellError::Num);
    }
    num_val(deviance(&y, &mu, &weights, family)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    fn triangle() -> Arg {
        av(arr(vec![vec![nv(100.0), nv(150.0), nv(180.0)], vec![nv(200.0), nv(330.0), Value::Empty], vec![nv(120.0), Value::Empty, Value::Empty]]))
    }

    fn data(value: Value) -> Vec<f64> {
        match value {
            Value::Array(a) => a
                .data
                .iter()
                .map(|v| match v {
                    Value::Number(x) => *x,
                    other => panic!("non-number {other:?}"),
                })
                .collect(),
            other => panic!("expected array {other:?}"),
        }
    }

    fn near(x: f64, expected: f64) {
        assert!((x - expected).abs() < 1e-8 * expected.abs().max(1.0), "{x} != {expected}");
    }

    #[test]
    fn chain_ladder_and_bf_analytic_triangle() {
        let f = data(ev("ACT.CHAINLADDER.FACTORS", vec![triangle()]));
        near(f[0], 1.6);
        near(f[1], 1.2);
        let estimates = data(ev("ACT.CHAINLADDER", vec![triangle()]));
        near(estimates[4], 396.0);
        near(estimates[7], 230.4);
        near(estimates[8], 110.4);
        let bf = data(ev("ACT.BORNHUETTERFERGUSON", vec![triangle(), av(col(&[200.0, 400.0, 240.0]))]));
        near(bf[2], 0.0);
        near(bf[5], 400.0 / 6.0);
        near(bf[8], 115.0);
        let tail = data(ev("ACT.CHAINLADDER", vec![triangle(), n(1.1)]));
        near(tail[1], 198.0);
    }

    #[test]
    fn mack_process_parameter_and_total_covariance_hand_calculation() {
        // sigma0² = 100*(1.5-1.6)²+200*(1.65-1.6)²=1.5; final sigma² supplied=.5.
        // Row 2: process=165, parameter=363. Row 3: process=355.2, parameter=226.56.
        // Shared factor gives covariance=211.2; aggregate MSE=528+581.76+2*211.2.
        let result = data(ev("ACT.MACK", vec![triangle(), n(0.5)]));
        near(result[9].powi(2), 165.0);
        near(result[10].powi(2), 363.0);
        near(result[15].powi(2), 355.2);
        near(result[16].powi(2), 226.56);
        near(result[23].powi(2), 1532.16);
        assert_eq!(ev("ACT.MACK", vec![triangle()]), Value::Error(CellError::Num));
    }

    #[test]
    fn malformed_triangle_is_not_silently_completed() {
        let hole = av(arr(vec![vec![nv(1.0), Value::Empty], vec![nv(1.0), Value::Empty]]));
        assert_eq!(ev("ACT.CHAINLADDER", vec![hole]), Value::Error(CellError::Value));
        let numeric_missing = av(arr(vec![vec![nv(1.0), nv(2.0)], vec![nv(1.0), nv(0.0)]]));
        assert_eq!(ev("ACT.CHAINLADDER", vec![numeric_missing]), Value::Error(CellError::Value));
        let zero_denominator = av(arr(vec![vec![nv(0.0), nv(0.0)], vec![nv(1.0), Value::Empty]]));
        assert_eq!(ev("ACT.CHAINLADDER", vec![zero_denominator]), Value::Error(CellError::Num));
    }

    #[test]
    fn credibility_limits_and_premium_loading() {
        close(ev("ACT.CREDIBILITY", vec![n(10.0), n(100.0), n(5.0)]), 1.0 / 3.0);
        close(ev("ACT.CREDIBILITY", vec![n(0.0), n(0.0), n(0.0)]), 0.0);
        close(ev("ACT.CREDIBILITY", vec![n(1.0), n(0.0), n(2.0)]), 1.0);
        close(ev("ACT.CREDIBILITY", vec![n(1e300), n(1e300), n(1.0)]), 0.5);
        close(ev("ACT.GROSSPREMIUM", vec![n(90.0), n(10.0), n(0.2), n(0.1)]), 100.0 / 0.7);
        assert_eq!(ev("ACT.GROSSPREMIUM", vec![n(1.0), n(0.0), n(0.8), n(0.2)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.CREDIBILITY", vec![n(f64::NAN), n(1.0), n(1.0)]), Value::Error(CellError::Num));
    }

    #[test]
    fn buhlmann_straub_moment_estimation() {
        let experience = av(arr(vec![vec![nv(1.0), nv(3.0)], vec![nv(5.0), nv(7.0)]]));
        let exposure = av(arr(vec![vec![nv(1.0), nv(1.0)], vec![nv(1.0), nv(1.0)]]));
        let result = data(ev("ACT.BUHLMANNSTRAUB", vec![experience, exposure]));
        near(result[0], 2.0);
        near(result[1], 2.0);
        near(result[4], 2.0);
        near(result[5], 7.0);
        near(result[2], 0.875);
        near(result[3], 2.25);
        near(result[9], 5.75);
    }

    #[test]
    fn poisson_and_gamma_glm_recover_synthetic_coefficients_and_offsets() {
        let x = av(arr(vec![vec![nv(1.0), nv(-1.0)], vec![nv(1.0), nv(0.0)], vec![nv(1.0), nv(1.0)], vec![nv(1.0), nv(2.0)]]));
        let offsets = av(col(&[0.0, 2.0_f64.ln(), 3.0_f64.ln(), 0.0]));
        let coefficients = av(col(&[2.0_f64.ln(), 0.3]));
        let expected = ev("ACT.GLM.PREDICT", vec![x.clone(), coefficients, offsets.clone()]);
        for family in ["poisson", "gamma"] {
            let recovered = data(ev("ACT.GLM.FIT", vec![av(expected.clone()), x.clone(), t(family), offsets.clone()]));
            near(recovered[0], 2.0_f64.ln());
            near(recovered[1], 0.3);
            close(ev("ACT.GLM.DEVIANCE", vec![av(expected.clone()), av(expected.clone()), t(family)]), 0.0);
        }
    }

    #[test]
    fn glm_intercept_is_weighted_mean_and_invalid_design_is_rejected() {
        let intercept = av(col(&[1.0, 1.0, 1.0]));
        let y = av(col(&[0.0, 2.0, 4.0]));
        let beta = data(ev("ACT.GLM.FIT", vec![y, intercept.clone(), t("poisson")]));
        near(beta[0], 2.0_f64.ln());
        let zeros = av(col(&[0.0, 0.0, 0.0]));
        assert_eq!(ev("ACT.GLM.FIT", vec![zeros, intercept.clone(), t("poisson")]), Value::Error(CellError::Num));
        let singular = av(arr(vec![vec![nv(1.0), nv(2.0)], vec![nv(1.0), nv(2.0)], vec![nv(1.0), nv(2.0)]]));
        assert_eq!(ev("ACT.GLM.FIT", vec![av(col(&[1.0, 2.0, 3.0])), singular, t("poisson")]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.GLM.FIT", vec![av(col(&[1.0, 0.0, 3.0])), intercept, t("gamma")]), Value::Error(CellError::Num));
    }

    #[test]
    fn glm_rejects_misalignment_nonfinite_and_zero_weights() {
        assert_eq!(ev("ACT.GLM.PREDICT", vec![av(col(&[1.0, 1.0])), av(col(&[1.0])), av(col(&[0.0]))]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.GLM.DEVIANCE", vec![av(col(&[1.0])), av(col(&[f64::INFINITY])), t("poisson")]), Value::Error(CellError::Num));
        assert_eq!(
            ev("ACT.GLM.FIT", vec![av(col(&[1.0, 2.0])), av(col(&[1.0, 1.0])), t("poisson"), av(col(&[0.0, 0.0])), av(col(&[0.0, 1.0]))]),
            Value::Error(CellError::Num)
        );
    }

    #[test]
    fn skipped_reserving_defaults_preserve_auto_sigma_and_aligned_glm_options() {
        let result = data(ev("ACT.CHAINLADDER", vec![triangle(), empty()]));
        near(result[5], 66.0);
        near(result[8], 110.4);
        assert_eq!(ev("ACT.MACK", vec![triangle(), empty()]), Value::Error(CellError::Num));
        assert!(matches!(ev("ACT.MACK", vec![triangle(), Arg::reference(Value::Empty)]), Value::Array(_)));
        let y = || av(col(&[0.0, 2.0, 4.0]));
        let x = || av(col(&[1.0, 1.0, 1.0]));
        let beta = data(ev("ACT.GLM.FIT", vec![y(), x(), t("poisson"), empty(), empty()]));
        near(beta[0], 2.0_f64.ln());
        assert_eq!(ev("ACT.GLM.FIT", vec![y(), x(), t("poisson"), Arg::reference(Value::Empty)]), Value::Error(CellError::Value));
    }
}
