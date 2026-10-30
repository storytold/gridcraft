//! Differentiable valuations, constrained finite-set decisions and validated proxy models.
//! No random sampling or hidden invocation of the simulator is performed here.

use std::collections::HashSet;

use gridcraft_core::{CellError, Value};

use super::optional_num as opt_num;

use crate::util::{R, arg, array_val, as_array, num, num_val};
use crate::{Arg, Ctx, FnSpec};

const MAX_POINTS: usize = 4096;
const MAX_WORK: usize = 20_000_000;
const MAX_FEATURES: usize = 64;
const MAX_INPUTS: usize = 6;
const MODEL_VERSION: f64 = 1.0;

/// Require a representable finite numeric value before propagating a numerical result.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}
/// Read and validate a finite scalar worksheet parameter.
fn scalar(a: &[Arg], i: usize) -> R<f64> {
    finite(num(a, i)?)
}
/// Validate nonnegative loss, budget or actuarial exposure quantities.
fn nonnegative(x: f64) -> R<f64> {
    if x.is_finite() && x >= 0.0 { Ok(x) } else { Err(CellError::Num) }
}
/// Require an exact bounded integer for terms, degrees and other discrete model dimensions.
fn integer(x: f64, max: usize) -> R<usize> {
    if x.is_finite() && x >= 0.0 && x <= max as f64 && x.fract() == 0.0 { Ok(x as usize) } else { Err(CellError::Num) }
}
/// Read numeric matrix data without coercing blanks, booleans or text.
fn strict(v: &Value) -> R<f64> {
    match v {
        Value::Number(x) => finite(*x),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}
/// Parse a bounded numeric row/column vector while preserving observation alignment.
fn vector(a: &[Arg], i: usize, cap: usize) -> R<Vec<f64>> {
    let arr = as_array(&arg(a, i)?.value);
    if arr.rows > 1 && arr.cols > 1 {
        return Err(CellError::Value);
    }
    if arr.data.is_empty() || arr.data.len() > cap {
        return Err(CellError::Num);
    }
    arr.iter().map(strict).collect()
}
/// Accumulate finite signed terms with Neumaier compensation and overflow checks.
fn sum(xs: impl IntoIterator<Item = f64>) -> R<f64> {
    let (mut s, mut correction) = (0.0_f64, 0.0_f64);
    for x in xs {
        finite(x)?;
        let next = finite(s + x)?;
        correction += if s.abs() >= x.abs() { (s - next) + x } else { (x - next) + s };
        s = next;
    }
    finite(s + correction)
}
/// Build a finite numeric rectangular worksheet result.
fn spill(rows: usize, cols: usize, xs: Vec<f64>) -> R<Value> {
    let values = xs.into_iter().map(num_val).collect::<R<Vec<_>>>()?;
    array_val(rows, cols, values)
}

/// Two independent forward-mode derivatives: effective rate and mortality multiplier.
/// Operations obey the chain rule; no finite-difference step or perturbed workbook is used.
#[derive(Clone, Copy)]
struct Dual {
    v: f64,
    rate: f64,
    mortality: f64,
}
impl Dual {
    /// Construct a dual constant with zero rate and mortality sensitivities.
    fn constant(v: f64) -> Self {
        Self { v, rate: 0.0, mortality: 0.0 }
    }
    /// Add dual values and both directional derivatives, rejecting nonfinite results.
    fn add(self, other: Self) -> R<Self> {
        Self { v: self.v + other.v, rate: self.rate + other.rate, mortality: self.mortality + other.mortality }.checked()
    }
    /// Subtract dual values and both directional derivatives, rejecting nonfinite results.
    fn sub(self, other: Self) -> R<Self> {
        Self { v: self.v - other.v, rate: self.rate - other.rate, mortality: self.mortality - other.mortality }.checked()
    }
    /// Apply the dual product rule to rate and common mortality-scale directions.
    fn mul(self, other: Self) -> R<Self> {
        Self { v: self.v * other.v, rate: self.rate * other.v + self.v * other.rate, mortality: self.mortality * other.v + self.v * other.mortality }
            .checked()
    }
    /// Apply the dual reciprocal rule; a zero value returns a division error.
    fn reciprocal(self) -> R<Self> {
        if self.v == 0.0 {
            return Err(CellError::Div0);
        }
        let v = finite(self.v.recip())?;
        Self { v, rate: -self.rate * v * v, mortality: -self.mortality * v * v }.checked()
    }
    /// Validate the primal value and both derivatives before returning a dual result.
    fn checked(self) -> R<Self> {
        finite(self.v)?;
        finite(self.rate)?;
        finite(self.mortality)?;
        Ok(self)
    }
}

/// Construct the annual-effective discount dual, including its exact rate derivative.
fn differentiable_rate(rate: f64) -> R<Dual> {
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    Dual { v: 1.0 + rate, rate: 1.0, mortality: 0.0 }.reciprocal()
}
/// Accumulate insurance and advance-premium annuity duals over an interior mortality window.
fn life_values(covered: &[f64], v: Dual, premium_term: usize, endowment: bool) -> R<(Dual, Dual)> {
    if covered.iter().any(|q| !(0.0..1.0).contains(q)) {
        return Err(CellError::Num);
    }
    let one = Dual::constant(1.0);
    let mut discounted_survival = one;
    let mut annuity = Dual::constant(0.0);
    let mut insurance = Dual::constant(0.0);
    for (year, &probability) in covered.iter().enumerate() {
        if year < premium_term {
            annuity = annuity.add(discounted_survival)?;
        }
        let death = Dual { v: probability, rate: 0.0, mortality: probability };
        insurance = insurance.add(discounted_survival.mul(death)?.mul(v)?)?;
        discounted_survival = discounted_survival.mul(one.sub(death)?)?.mul(v)?;
    }
    if endowment {
        insurance = insurance.add(discounted_survival)?;
    }
    Ok((insurance, annuity))
}
/// Require an explicit zero/one contract option rather than interpreting arbitrary numbers as flags.
fn binary(a: &[Arg], i: usize) -> R<bool> {
    match finite(opt_num(a, i, 0.0)?)? {
        0.0 => Ok(false),
        1.0 => Ok(true),
        _ => Err(CellError::Num),
    }
}
/// Return the primal value, effective-rate derivative and mortality-scale derivative in one row.
fn dual_row(value: Dual) -> R<Value> {
    spill(1, 3, vec![value.v, value.rate, value.mortality])
}
/// Return insurance and annuity-due values with forward-mode rate and common-qx-scale sensitivities.
fn life_sensitivity(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let q = vector(a, 0, 10_000)?;
    let v = differentiable_rate(scalar(a, 1)?)?;
    let term = integer(scalar(a, 2)?, 10_000)?;
    let covered = q.get(..term).ok_or(CellError::Num)?;
    let (insurance, annuity) = life_values(covered, v, term, binary(a, 3)?)?;
    spill(2, 3, vec![insurance.v, insurance.rate, insurance.mortality, annuity.v, annuity.rate, annuity.mortality])
}
/// Differentiate the level equivalence premium through the dual quotient rule.
fn premium_sensitivity(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let q = vector(a, 0, 10_000)?;
    let v = differentiable_rate(scalar(a, 1)?)?;
    let term = integer(scalar(a, 2)?, 10_000)?;
    let premium_term = integer(finite(opt_num(a, 3, term as f64)?)?, term)?;
    let (insurance, annuity) = life_values(q.get(..term).ok_or(CellError::Num)?, v, premium_term, binary(a, 4)?)?;
    dual_row(insurance.mul(annuity.reciprocal()?)?)
}
/// Differentiate a conditional prospective reserve while holding the supplied contractual premium fixed.
fn reserve_sensitivity(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let q = vector(a, 0, 10_000)?;
    let v = differentiable_rate(scalar(a, 1)?)?;
    let term = integer(scalar(a, 2)?, 10_000)?;
    let elapsed = integer(scalar(a, 3)?, term)?;
    let premium = nonnegative(scalar(a, 4)?)?;
    let premium_term = integer(finite(opt_num(a, 6, term as f64)?)?, term)?;
    let remaining = q.get(elapsed..term).ok_or(CellError::Num)?;
    let (insurance, annuity) = life_values(remaining, v, premium_term.saturating_sub(elapsed), binary(a, 5)?)?;
    dual_row(insurance.sub(annuity.mul(Dual::constant(premium))?)?)
}

/// Validate aligned nonnegative empirical weights and normalize them without overflowing their sum.
fn weights(a: &[Arg], i: usize, n: usize) -> R<Vec<f64>> {
    let values = vector(a, i, MAX_POINTS)?;
    if values.len() != n {
        return Err(CellError::Value);
    }
    let mut scale = 0.0_f64;
    for &x in &values {
        scale = scale.max(nonnegative(x)?);
    }
    if scale == 0.0 {
        return Err(CellError::Num);
    }
    let total = sum(values.iter().map(|w| w / scale))?;
    Ok(values.into_iter().map(|w| (w / scale) / total).collect())
}
/// Integrate retained-loss tail mass for a monotone layer transform, including partial threshold atoms.
fn empirical_tvar(losses: &[(f64, f64)], alpha: f64, attachment: f64, limit: f64) -> R<f64> {
    let tail = 1.0 - alpha;
    let mut remaining = tail;
    let mut terms = Vec::with_capacity(losses.len());
    for &(loss, probability) in losses.iter().rev() {
        let take = remaining.min(probability);
        let retained = loss - (loss - attachment).max(0.0).min(limit);
        terms.push(retained * (take / tail));
        remaining -= take;
        if remaining <= 0.0 {
            break;
        }
    }
    sum(terms)
}

/// Select the minimum total-cost TVaR over explicit deductible/limit candidates subject to the premium budget.
fn reinsurance_optimize(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let losses = vector(a, 0, MAX_POINTS)?;
    for &loss in &losses {
        nonnegative(loss)?;
    }
    let weights = weights(a, 1, losses.len())?;
    let deductibles = vector(a, 2, 128)?;
    let limits = vector(a, 3, 128)?;
    for &x in deductibles.iter().chain(&limits) {
        nonnegative(x)?;
    }
    if losses.len().saturating_mul(deductibles.len()).saturating_mul(limits.len()) > MAX_WORK {
        return Err(CellError::Num);
    }
    let loading = nonnegative(scalar(a, 4)?)?;
    let budget = nonnegative(scalar(a, 5)?)?;
    let alpha = scalar(a, 6)?;
    if !(0.0..1.0).contains(&alpha) {
        return Err(CellError::Num);
    }
    let mut distribution: Vec<_> = losses.into_iter().zip(weights).filter(|(_, w)| *w > 0.0).collect();
    distribution.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    let mut best: Option<[f64; 7]> = None;
    for &deductible in &deductibles {
        for &limit in &limits {
            let ceded = sum(distribution.iter().map(|(x, w)| (x - deductible).max(0.0).min(limit) * w))?;
            let premium = ceded * (1.0 + loading);
            // An overflowing positive premium exceeds every admitted finite budget.
            if !premium.is_finite() || premium > budget {
                continue;
            }
            let retained_tvar = empirical_tvar(&distribution, alpha, deductible, limit)?;
            let objective = finite(retained_tvar + premium)?;
            if best.as_ref().is_none_or(|b| objective < b[5]) {
                best = Some([deductible, limit, ceded, premium, retained_tvar, objective, budget - premium]);
            }
        }
    }
    let best = best.ok_or(CellError::Num)?;
    spill(1, best.len(), best.to_vec())
}

/// Solve two-instrument PV and dollar-duration matching with an explicit short-position policy.
fn immunize(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let liability = nonnegative(scalar(a, 0)?)?;
    let duration = nonnegative(scalar(a, 1)?)?;
    let pvs = vector(a, 2, 2)?;
    let durations = vector(a, 3, 2)?;
    if pvs.len() != 2 || durations.len() != 2 {
        return Err(CellError::Value);
    }
    if pvs.iter().any(|p| *p <= 0.0) || durations.iter().any(|d| *d < 0.0) {
        return Err(CellError::Num);
    }
    let allow_short = finite(opt_num(a, 4, 0.0)?)?;
    if allow_short != 0.0 && allow_short != 1.0 {
        return Err(CellError::Num);
    }
    if liability == 0.0 {
        return spill(2, 1, vec![0.0, 0.0]);
    }
    let gap = finite(durations[1] - durations[0])?;
    if gap.abs() <= 1e-12 * durations.iter().copied().fold(1.0_f64, f64::max) {
        return Err(CellError::Num);
    }
    let dollars1 = finite(liability * ((durations[1] - duration) / gap))?;
    let dollars2 = finite(liability - dollars1)?;
    if allow_short == 0.0 && (dollars1 < 0.0 || dollars2 < 0.0) {
        return Err(CellError::Num);
    }
    spill(2, 1, vec![finite(dollars1 / pvs[0])?, finite(dollars2 / pvs[1])?])
}

struct Design {
    n: usize,
    d: usize,
    x: Vec<f64>,
}
/// Read a bounded finite predictor matrix for training, validation or prediction.
fn design(a: &[Arg], i: usize) -> R<Design> {
    let arr = as_array(&arg(a, i)?.value);
    if arr.rows == 0 || arr.rows > MAX_POINTS || arr.cols == 0 || arr.cols > MAX_INPUTS {
        return Err(CellError::Num);
    }
    Ok(Design { n: arr.rows, d: arr.cols, x: arr.iter().map(strict).collect::<R<Vec<_>>>()? })
}
/// Enumerate total-degree monomials in degree order; depth <= MAX_INPUTS (6).
fn exponent_rows(d: usize, degree: usize) -> R<Vec<Vec<usize>>> {
    /// Recursively append total-degree exponent vectors within the fixed feature-count cap.
    fn add(rows: &mut Vec<Vec<usize>>, powers: &mut [usize], index: usize, remaining: usize) -> R<()> {
        if index + 1 == powers.len() {
            *powers.get_mut(index).ok_or(CellError::Num)? = remaining;
            if rows.len() >= MAX_FEATURES {
                return Err(CellError::Num);
            }
            rows.push(powers.to_vec());
        } else {
            for value in 0..=remaining {
                *powers.get_mut(index).ok_or(CellError::Num)? = value;
                add(rows, powers, index + 1, remaining - value)?;
            }
        }
        Ok(())
    }
    if d == 0 || d > MAX_INPUTS || degree > 3 {
        return Err(CellError::Num);
    }
    let mut rows = Vec::new();
    let mut powers = vec![0; d];
    for order in 0..=degree {
        add(&mut rows, &mut powers, 0, order)?;
    }
    Ok(rows)
}
/// Determine per-coordinate training bounds for standardization and extrapolation protection.
fn bounds(x: &Design) -> (Vec<f64>, Vec<f64>) {
    let mut minimum = vec![f64::INFINITY; x.d];
    let mut maximum = vec![f64::NEG_INFINITY; x.d];
    for row in x.x.chunks_exact(x.d) {
        for ((lo, hi), value) in minimum.iter_mut().zip(&mut maximum).zip(row) {
            *lo = lo.min(*value);
            *hi = hi.max(*value);
        }
    }
    (minimum, maximum)
}
/// Build scaled polynomial columns and reject coordinates outside the declared training domain.
fn basis(x: &Design, minimum: &[f64], maximum: &[f64], powers: &[Vec<usize>]) -> R<Vec<Vec<f64>>> {
    let mut columns = vec![Vec::with_capacity(x.n); powers.len()];
    for row in x.x.chunks_exact(x.d) {
        let mut z = Vec::with_capacity(x.d);
        for ((value, lo), hi) in row.iter().zip(minimum).zip(maximum) {
            if value < lo || value > hi {
                return Err(CellError::Num);
            }
            let center = lo * 0.5 + hi * 0.5;
            let radius = hi * 0.5 - lo * 0.5;
            if radius == 0.0 {
                if powers.len() > 1 {
                    return Err(CellError::Num);
                }
                z.push(0.0);
            } else {
                z.push(finite((value * 0.5 - center * 0.5) / (radius * 0.5))?);
            }
        }
        for (column, exponents) in columns.iter_mut().zip(powers) {
            let mut value = 1.0;
            for (z, exponent) in z.iter().zip(exponents) {
                value *= z.powi(*exponent as i32);
            }
            column.push(finite(value)?);
        }
    }
    Ok(columns)
}
/// Original reorthogonalized modified Gram-Schmidt, with rank checks and no ridge.
fn least_squares(mut columns: Vec<Vec<f64>>, y: &[f64]) -> R<Vec<f64>> {
    let p = columns.len();
    let mut upper = vec![vec![0.0; p]; p];
    let mut projected = vec![0.0; p];
    for j in 0..p {
        for _ in 0..2 {
            for k in 0..j {
                let projection = sum(columns[k].iter().zip(&columns[j]).map(|(a, b)| a * b))?;
                upper[k][j] += projection;
                let (previous, current) = columns.split_at_mut(j);
                for (v, q) in current[0].iter_mut().zip(&previous[k]) {
                    *v -= projection * q;
                }
            }
        }
        let norm = sum(columns[j].iter().map(|x| x * x))?.sqrt();
        if norm <= 1e-11 * (y.len() as f64).sqrt() {
            return Err(CellError::Num);
        }
        upper[j][j] = norm;
        for x in &mut columns[j] {
            *x /= norm;
        }
        projected[j] = sum(columns[j].iter().zip(y).map(|(q, y)| q * y))?;
    }
    let mut coefficients = vec![0.0; p];
    for j in (0..p).rev() {
        let rest = sum((j + 1..p).map(|k| upper[j][k] * coefficients[k]))?;
        coefficients[j] = finite((projected[j] - rest) / upper[j][j])?;
    }
    Ok(coefficients)
}
/// Evaluate already constructed basis columns against the fitted coefficient vector.
fn predicted(columns: &[Vec<f64>], coefficients: &[f64], n: usize) -> R<Vec<f64>> {
    (0..n).map(|row| sum(columns.iter().zip(coefficients).map(|(column, coefficient)| column[row] * coefficient))).collect()
}
/// Compute a scaled root-mean-square error without squaring large raw residuals.
fn rms(errors: impl IntoIterator<Item = f64>) -> R<f64> {
    // Scaled sum of squares also avoids overflowing a norm whose RMS is representable.
    let mut scale = 0.0_f64;
    let mut squares = 0.0_f64;
    let mut count = 0_usize;
    for error in errors {
        let x = finite(error)?.abs();
        if x > scale {
            squares = 1.0 + squares * (scale / x).powi(2);
            scale = x;
        } else if scale > 0.0 {
            squares += (x / scale).powi(2);
        }
        count += 1;
    }
    if count == 0 {
        return Err(CellError::Num);
    }
    finite(scale * (squares / count as f64).sqrt())
}
/// Canonicalize coordinate bits for exact train/validation overlap detection, including signed zero.
fn coordinates(row: &[f64]) -> Vec<u64> {
    row.iter().map(|x| if *x == 0.0 { 0 } else { x.to_bits() }).collect()
}

/// Fit a bounded-domain polynomial proxy and retain independent holdout RMSE, maximum and tail-error diagnostics.
fn surrogate_fit(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let train = design(a, 0)?;
    let train_y = vector(a, 1, MAX_POINTS)?;
    let validation = design(a, 2)?;
    let validation_y = vector(a, 3, MAX_POINTS)?;
    if train.n != train_y.len() || validation.n != validation_y.len() || train.d != validation.d {
        return Err(CellError::Value);
    }
    let degree = integer(finite(opt_num(a, 4, 2.0)?)?, 3)?;
    let alpha = finite(opt_num(a, 5, 0.95)?)?;
    if !(0.0..1.0).contains(&alpha) {
        return Err(CellError::Num);
    }
    let powers = exponent_rows(train.d, degree)?;
    let p = powers.len();
    if train.n < p || train.n.saturating_mul(p).saturating_mul(p) > MAX_WORK {
        return Err(CellError::Num);
    }
    let training_coordinates: HashSet<_> = train.x.chunks_exact(train.d).map(coordinates).collect();
    if validation.x.chunks_exact(validation.d).any(|row| training_coordinates.contains(&coordinates(row))) {
        return Err(CellError::Value);
    }
    let (minimum, maximum) = bounds(&train);
    let columns = basis(&train, &minimum, &maximum, &powers)?;
    let response_scale = train_y.iter().map(|y| y.abs()).fold(0.0_f64, f64::max);
    let scaled_y: Vec<_> = train_y.iter().map(|y| if response_scale == 0.0 { 0.0 } else { y / response_scale }).collect();
    let coefficients = least_squares(columns.clone(), &scaled_y)?.into_iter().map(|b| finite(b * response_scale)).collect::<R<Vec<_>>>()?;
    let train_prediction = predicted(&columns, &coefficients, train.n)?;
    let validation_prediction = predicted(&basis(&validation, &minimum, &maximum, &powers)?, &coefficients, validation.n)?;
    let train_rmse = rms(train_prediction.iter().zip(&train_y).map(|(p, y)| p - y))?;
    let errors: Vec<_> = validation_prediction.iter().zip(&validation_y).map(|(p, y)| finite(p - y)).collect::<R<_>>()?;
    let rmse = rms(errors.iter().copied())?;
    let max_error = errors.iter().map(|x| x.abs()).fold(0.0_f64, f64::max);
    let mut ordered: Vec<_> = (0..validation.n).collect();
    ordered.sort_by(|i, j| validation_y[*i].total_cmp(&validation_y[*j]));
    // Condition on actual response >= its left-inverse quantile, including every tie.
    let start = ((alpha * validation.n as f64).ceil().max(1.0) as usize - 1).min(validation.n - 1);
    let threshold = validation_y[ordered[start]];
    let tail_rmse = rms(errors.iter().zip(&validation_y).filter(|(_, y)| **y >= threshold).map(|(e, _)| *e))?;
    let rows = p.max(train.d).max(7) + 1;
    let mut model = vec![0.0; rows * 4];
    model[..4].copy_from_slice(&[MODEL_VERSION, train.d as f64, degree as f64, p as f64]);
    for (i, coefficient) in coefficients.iter().enumerate() {
        model[(i + 1) * 4] = *coefficient;
    }
    for (i, (lo, hi)) in minimum.iter().zip(maximum).enumerate() {
        model[(i + 1) * 4 + 1] = *lo;
        model[(i + 1) * 4 + 2] = hi;
    }
    for (i, value) in [train.n as f64, validation.n as f64, train_rmse, rmse, max_error, tail_rmse, alpha].iter().enumerate() {
        model[(i + 1) * 4 + 3] = *value;
    }
    spill(rows, 4, model)
}

/// Predict only within recorded training bounds and only when the supplied holdout-RMSE tolerance is satisfied.
fn surrogate_predict(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let arr = as_array(&arg(a, 0)?.value);
    if arr.cols != 4 || arr.rows < 8 || arr.rows > MAX_FEATURES + 1 {
        return Err(CellError::Value);
    }
    let model = arr.iter().map(strict).collect::<R<Vec<_>>>()?;
    if model.first().copied() != Some(MODEL_VERSION) {
        return Err(CellError::Value);
    }
    let d = integer(*model.get(1).ok_or(CellError::Value)?, MAX_INPUTS)?;
    let degree = integer(*model.get(2).ok_or(CellError::Value)?, 3)?;
    let p = integer(*model.get(3).ok_or(CellError::Value)?, MAX_FEATURES)?;
    let powers = exponent_rows(d, degree)?;
    if powers.len() != p || arr.rows != p.max(d).max(7) + 1 {
        return Err(CellError::Value);
    }
    let training_count = integer(model[7], MAX_POINTS)?;
    let validation_count = integer(model[11], MAX_POINTS)?;
    if training_count < p || validation_count == 0 || model[31] < 0.0 || model[31] >= 1.0 {
        return Err(CellError::Value);
    }
    for row in 3..=6 {
        nonnegative(model[row * 4 + 3])?;
    }
    let max_rmse = nonnegative(scalar(a, 2)?)?;
    if model[19] > max_rmse {
        return Err(CellError::Num);
    }
    let input = design(a, 1)?;
    if input.d != d {
        return Err(CellError::Value);
    }
    let coefficients: Vec<_> = (1..=p).map(|i| model[i * 4]).collect();
    let minimum: Vec<_> = (1..=d).map(|i| model[i * 4 + 1]).collect();
    let maximum: Vec<_> = (1..=d).map(|i| model[i * 4 + 2]).collect();
    if minimum.iter().zip(&maximum).any(|(lo, hi)| lo > hi) {
        return Err(CellError::Num);
    }
    let columns = basis(&input, &minimum, &maximum, &powers)?;
    spill(input.n, 1, predicted(&columns, &coefficients, input.n)?)
}

/// Register dual sensitivities, finite-candidate decisions, ALM matching and validated proxy functions.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.LIFE.SENSITIVITY",
            3,
            4,
            Financial,
            &[false, true, true, true],
            "ACT.LIFE.SENSITIVITY(qx,rate,term,[endowment=0])",
            "Forward-dual insurance/annuity values and rate/mortality-scale derivatives; covered qx<1.",
            life_sensitivity
        ),
        act!(
            "ACT.PREMIUM.SENSITIVITY",
            3,
            5,
            Financial,
            &[false, true, true, true, true],
            "ACT.PREMIUM.SENSITIVITY(qx,rate,term,[premium_term],[endowment=0])",
            "Forward-dual equivalence premium and rate/mortality-scale derivatives.",
            premium_sensitivity
        ),
        act!(
            "ACT.RESERVE.SENSITIVITY",
            5,
            7,
            Financial,
            &[false, true, true, true, true, true, true],
            "ACT.RESERVE.SENSITIVITY(qx,rate,term,elapsed,premium,[endowment=0],[premium_term])",
            "Forward-dual prospective reserve derivatives with contractual premium held fixed.",
            reserve_sensitivity
        ),
        act!(
            "ACT.REINSURANCE.OPTIMIZE",
            7,
            7,
            Financial,
            &[false, false, false, false, true, true, true],
            "ACT.REINSURANCE.OPTIMIZE(losses,weights,deductibles,limits,loading,budget,alpha)",
            "Best finite-set layer by retained-plus-premium TVaR under an expected-premium budget.",
            reinsurance_optimize
        ),
        act!(
            "ACT.ALM.IMMUNIZE",
            4,
            5,
            Financial,
            &[true, true, false, false, true],
            "ACT.ALM.IMMUNIZE(liability_pv,liability_duration,asset_pvs,asset_durations,[allow_short=0])",
            "Two-instrument units matching liability PV and dollar duration.",
            immunize
        ),
        act!(
            "ACT.SURROGATE.FIT",
            4,
            6,
            Statistical,
            &[false, false, false, false, true, true],
            "ACT.SURROGATE.FIT(train_x,train_y,validation_x,validation_y,[degree=2],[tail_alpha=0.95])",
            "Scaled multivariate polynomial fit with disjoint held-out diagnostics and bounded training domain.",
            surrogate_fit
        ),
        act!(
            "ACT.SURROGATE.PREDICT",
            3,
            3,
            Statistical,
            &[false, false, true],
            "ACT.SURROGATE.PREDICT(model,x,max_validation_rmse)",
            "Proxy predictions only inside training bounds and with an explicit accepted held-out RMSE.",
            surrogate_predict
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    fn mat(rows: Vec<Vec<f64>>) -> Arg {
        av(arr(rows.into_iter().map(|r| r.into_iter().map(nv).collect()).collect()))
    }
    fn column(values: &[f64]) -> Arg {
        mat(values.iter().map(|x| vec![*x]).collect())
    }
    fn data(value: Value) -> Vec<f64> {
        match value {
            Value::Array(a) => a.iter().map(|x| strict(x).unwrap()).collect(),
            Value::Number(x) => vec![x],
            other => panic!("{other:?}"),
        }
    }
    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= 1e-8 * (1.0 + expected.abs()), "{actual} != {expected}");
    }

    #[test]
    fn dual_life_derivatives_match_closed_forms_and_multiple_central_steps() {
        let q = [0.1, 0.2, 0.3];
        let actual = data(ev("ACT.LIFE.SENSITIVITY", vec![column(&q), n(0.05), n(3.0)]));
        let insurance = |rate: f64, scale: f64| {
            let mut p = 1.0;
            let mut value = 0.0;
            for (i, q) in q.iter().enumerate() {
                value += p * q * scale / (1.0 + rate).powi(i as i32 + 1);
                p *= 1.0 - q * scale;
            }
            value
        };
        close(actual[0], insurance(0.05, 1.0));
        for step in [1e-5, 1e-6, 1e-7] {
            close(actual[1], (insurance(0.05 + step, 1.0) - insurance(0.05 - step, 1.0)) / (2.0 * step));
            close(actual[2], (insurance(0.05, 1.0 + step) - insurance(0.05, 1.0 - step)) / (2.0 * step));
        }
        let one = data(ev("ACT.LIFE.SENSITIVITY", vec![n(0.2), n(0.1), n(1.0)]));
        close(one[0], 0.2 / 1.1);
        close(one[1], -0.2 / 1.1_f64.powi(2));
        close(one[2], 0.2 / 1.1);
        close(one[3], 1.0);
        close(one[4], 0.0);
        close(one[5], 0.0);
        assert_eq!(ev("ACT.LIFE.SENSITIVITY", vec![n(1.0), n(0.05), n(1.0)]), Value::Error(CellError::Num));
    }
    #[test]
    fn dual_endowment_and_negative_rates_are_defined() {
        let values = data(ev("ACT.LIFE.SENSITIVITY", vec![column(&[0.0, 0.0]), n(-0.1), n(2.0), n(1.0)]));
        close(values[0], 1.0 / 0.9_f64.powi(2));
        close(values[1], -2.0 / 0.9_f64.powi(3));
        close(values[2], 0.0);
        let invalid = ev("ACT.LIFE.SENSITIVITY", vec![n(f64::NAN), n(0.0), n(1.0)]);
        assert_eq!(invalid, Value::Error(CellError::Num));
    }
    #[test]
    fn premium_and_fixed_contract_reserve_derivatives_follow_quotient_rule() {
        let arguments = || vec![column(&[0.1, 0.2, 0.3]), n(0.05), n(3.0)];
        let life = data(ev("ACT.LIFE.SENSITIVITY", arguments()));
        let premium = data(ev("ACT.PREMIUM.SENSITIVITY", arguments()));
        close(premium[0], life[0] / life[3]);
        close(premium[1], (life[1] * life[3] - life[0] * life[4]) / life[3].powi(2));
        close(premium[2], (life[2] * life[3] - life[0] * life[5]) / life[3].powi(2));
        let reserve = data(ev("ACT.RESERVE.SENSITIVITY", vec![column(&[0.1, 0.2, 0.3]), n(0.05), n(3.0), n(0.0), n(premium[0])]));
        close(reserve[0], 0.0);
        close(reserve[1], life[1] - premium[0] * life[4]);
        close(reserve[2], life[2] - premium[0] * life[5]);
        let terminal = data(ev("ACT.RESERVE.SENSITIVITY", vec![column(&[0.1, 0.2, 0.3]), n(0.05), n(3.0), n(3.0), n(premium[0]), n(1.0)]));
        close(terminal[0], 1.0);
        close(terminal[1], 0.0);
        close(terminal[2], 0.0);
    }
    #[test]
    fn finite_set_reinsurance_selects_known_global_candidate_and_budget() {
        let result = data(ev(
            "ACT.REINSURANCE.OPTIMIZE",
            vec![column(&[0.0, 100.0]), column(&[0.9, 0.1]), column(&[0.0, 50.0]), column(&[0.0, 50.0, 100.0]), n(0.2), n(12.0), n(0.9)],
        ));
        for (actual, expected) in result.iter().zip([0.0, 100.0, 10.0, 12.0, 0.0, 12.0, 0.0]) {
            close(*actual, expected);
        }
        let capped = data(ev(
            "ACT.REINSURANCE.OPTIMIZE",
            vec![column(&[0.0, 100.0]), column(&[0.9, 0.1]), n(0.0), column(&[0.0, 50.0, 100.0]), n(0.2), n(6.0), n(0.9)],
        ));
        close(capped[1], 50.0);
        close(capped[5], 56.0);
        assert_eq!(
            ev("ACT.REINSURANCE.OPTIMIZE", vec![column(&[100.0]), n(1.0), n(0.0), n(100.0), n(1.0), n(0.0), n(0.9)]),
            Value::Error(CellError::Num)
        );
    }
    #[test]
    fn asset_units_match_pv_and_duration_and_reject_short_positions() {
        let units = data(ev("ACT.ALM.IMMUNIZE", vec![n(1000.0), n(5.0), column(&[100.0, 200.0]), column(&[2.0, 8.0])]));
        close(units[0], 5.0);
        close(units[1], 2.5);
        close(units[0] * 100.0 + units[1] * 200.0, 1000.0);
        close(units[0] * 100.0 * 2.0 + units[1] * 200.0 * 8.0, 5000.0);
        assert_eq!(ev("ACT.ALM.IMMUNIZE", vec![n(1000.0), n(10.0), column(&[100.0, 200.0]), column(&[2.0, 8.0])]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.ALM.IMMUNIZE", vec![n(1000.0), n(5.0), column(&[100.0, 200.0]), column(&[5.0, 5.0])]), Value::Error(CellError::Num));
    }
    fn polynomial_model(degree: f64) -> Value {
        let train = vec![
            vec![-1.0, -1.0],
            vec![-1.0, 0.0],
            vec![-1.0, 1.0],
            vec![0.0, -1.0],
            vec![0.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, -1.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
        ];
        let test = vec![vec![-0.5, -0.5], vec![-0.5, 0.5], vec![0.5, -0.5], vec![0.5, 0.5]];
        let value = |row: &Vec<f64>| 1.0 + 2.0 * row[0] + 3.0 * row[1] + 4.0 * row[0] * row[1] + row[0] * row[0];
        let y: Vec<_> = train.iter().map(value).collect();
        let validation: Vec<_> = test.iter().map(value).collect();
        ev("ACT.SURROGATE.FIT", vec![mat(train), column(&y), mat(test), column(&validation), n(degree)])
    }
    #[test]
    fn multivariate_polynomial_surrogate_recovers_truth_without_extrapolation() {
        let model = polynomial_model(2.0);
        let table = data(model.clone());
        close(table[19], 0.0);
        close(table[23], 0.0);
        close(table[27], 0.0);
        let predictions = data(ev("ACT.SURROGATE.PREDICT", vec![av(model.clone()), mat(vec![vec![0.25, -0.25]]), n(1e-10)]));
        close(predictions[0], 1.0 + 0.5 - 0.75 - 0.25 + 0.0625);
        assert_eq!(ev("ACT.SURROGATE.PREDICT", vec![av(model), mat(vec![vec![1.1, 0.0]]), n(1e-10)]), Value::Error(CellError::Num));
    }
    #[test]
    fn heldout_misspecification_and_information_leakage_are_detected() {
        let model = polynomial_model(1.0);
        let diagnostics = data(model.clone());
        assert!(diagnostics[19] > 0.1);
        assert_eq!(ev("ACT.SURROGATE.PREDICT", vec![av(model), mat(vec![vec![0.25, -0.25]]), n(0.01)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.SURROGATE.FIT", vec![column(&[0.0, 1.0, 2.0]), column(&[0.0, 1.0, 4.0]), n(1.0), n(1.0)]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.SURROGATE.FIT", vec![column(&[0.0, 0.0, 0.0]), column(&[1.0, 2.0, 3.0]), n(0.1), n(1.0)]), Value::Error(CellError::Num));
    }
    #[test]
    fn scaled_rms_handles_large_finite_residuals() {
        close(rms([1e308, 1e308, 1e308, 1e308]).unwrap() / 1e308, 1.0);
    }

    #[test]
    fn skipped_sensitivity_and_proxy_options_keep_their_nonzero_defaults() {
        let q = || column(&[0.1, 0.2]);
        let v = 1.0 / 1.05;
        let insurance = 0.1 * v + 0.9 * v * v;
        let annuity = 1.0 + 0.9 * v;
        let premium = data(ev("ACT.PREMIUM.SENSITIVITY", vec![q(), n(0.05), n(2.0), empty(), n(1.0)]));
        close(premium[0], insurance / annuity);
        let reserve = data(ev("ACT.RESERVE.SENSITIVITY", vec![q(), n(0.05), n(2.0), n(0.0), n(0.1), n(1.0), empty()]));
        close(reserve[0], insurance - 0.1 * annuity);
        assert_eq!(ev("ACT.PREMIUM.SENSITIVITY", vec![q(), n(0.05), n(2.0), Arg::reference(Value::Empty), n(1.0)]), Value::Error(CellError::Div0));
        let train = || mat(vec![vec![-2.0], vec![-1.0], vec![0.0], vec![1.0], vec![2.0]]);
        let validation = || mat(vec![vec![-1.5], vec![-0.5], vec![0.5], vec![1.5]]);
        let args = vec![train(), column(&[9.0, 2.0, 1.0, 6.0, 17.0]), validation(), column(&[4.75, 0.75, 2.75, 10.75]), empty(), n(0.95)];
        let model = data(ev("ACT.SURROGATE.FIT", args.clone()));
        close(model[2], 2.0);
        close(model[19], 0.0);
        let mut referenced = args;
        referenced[4] = Arg::reference(Value::Empty);
        let model = data(ev("ACT.SURROGATE.FIT", referenced));
        close(model[2], 0.0);
    }
}
