//! Finite homogeneous Markov chains and multi-state actuarial rewards.
//! Row-stochastic convention: distributions are row vectors. No random simulation.

use crate::util::{R, arg, array_val, as_array, has, num, num_val};
use crate::{Arg, Ctx, FnSpec};
use gridcraft_core::{CellError, Value};

const MAX_STATES: usize = 32;
const TOL: f64 = 1e-12;
type Matrix = Vec<Vec<f64>>;

mod temporal;

/// Read a finite numeric cell without coercing blank, text or boolean state data.
fn number(v: &Value) -> R<f64> {
    match v {
        Value::Number(x) if x.is_finite() => Ok(*x),
        Value::Error(e) => Err(*e),
        Value::Number(_) => Err(CellError::Num),
        _ => Err(CellError::Value),
    }
}
/// Read a scalar argument and reject nonfinite model parameters.
fn scalar(a: &[Arg], i: usize) -> R<f64> {
    let x = num(a, i)?;
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}
/// Require an exact nonnegative integer horizon within the supplied work bound.
fn integer(a: &[Arg], i: usize, max: usize) -> R<usize> {
    let x = scalar(a, i)?;
    if x < 0.0 || x.fract() != 0.0 || x > max as f64 { Err(CellError::Num) } else { Ok(x as usize) }
}
/// Parse a strict square numeric matrix with at most 32 states.
fn matrix(a: &[Arg], i: usize) -> R<Matrix> {
    let arr = as_array(&arg(a, i)?.value);
    if arr.rows == 0 || arr.rows != arr.cols || arr.rows > MAX_STATES {
        return Err(CellError::Value);
    }
    let values = arr.iter().map(number).collect::<R<Vec<_>>>()?;
    Ok(values.chunks(arr.cols).map(<[f64]>::to_vec).collect())
}
/// Read exactly one numeric state vector without dropping missing entries.
fn vector(a: &[Arg], i: usize, n: usize) -> R<Vec<f64>> {
    let arr = as_array(&arg(a, i)?.value);
    if (arr.rows > 1 && arr.cols > 1) || arr.rows.checked_mul(arr.cols) != Some(n) {
        return Err(CellError::Value);
    }
    arr.iter().map(number).collect()
}
/// Validate probability entries and unit row sums before accepting a transition model.
fn stochastic(p: &Matrix) -> R<()> {
    for row in p {
        if row.iter().any(|x| *x < 0.0 || *x > 1.0) || (row.iter().sum::<f64>() - 1.0).abs() > TOL {
            return Err(CellError::Num);
        }
    }
    Ok(())
}
/// Load a row-stochastic matrix and remove only accepted decimal roundoff.
fn transition(a: &[Arg], i: usize) -> R<Matrix> {
    let mut p = matrix(a, i)?;
    stochastic(&p)?;
    // Remove only accepted decimal roundoff; never repair a non-stochastic input.
    normalize(&mut p)?;
    Ok(p)
}
/// Normalize near-unit probability rows; reject larger mass errors instead of repairing a model.
fn normalize(p: &mut Matrix) -> R<()> {
    for row in p {
        let mass = row.iter().sum::<f64>();
        if !mass.is_finite() || (mass - 1.0).abs() > 1e-10 || row.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err(CellError::Num);
        }
        for x in row {
            *x /= mass;
        }
    }
    Ok(())
}
/// Construct the identity transition for a validated bounded state dimension.
fn identity(n: usize) -> Matrix {
    (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect()
}
/// Multiply internally shape-checked matrices, refusing nonfinite intermediate results.
fn multiply(a: &Matrix, b: &Matrix) -> R<Matrix> {
    let n = a.len();
    let cols = b.first().map_or(0, Vec::len);
    let mut out = vec![vec![0.0; cols]; n];
    // Shapes are private invariants after bounded square-matrix validation.
    for i in 0..n {
        for k in 0..b.len() {
            for j in 0..cols {
                out[i][j] = a[i][k].mul_add(b[k][j], out[i][j]);
                if !out[i][j].is_finite() {
                    return Err(CellError::Num);
                }
            }
        }
    }
    Ok(out)
}
/// Compute a stochastic matrix power by squaring, controlling roundoff mass after each product.
fn power(mut p: Matrix, mut n: usize) -> R<Matrix> {
    let mut out = identity(p.len());
    while n > 0 {
        if n & 1 == 1 {
            out = multiply(&out, &p)?;
            normalize(&mut out)?;
        }
        n >>= 1;
        if n > 0 {
            p = multiply(&p, &p)?;
            normalize(&mut p)?;
        }
    }
    Ok(out)
}
/// Return a rectangular matrix spill with finite numeric cells.
fn spill(p: Matrix) -> R<Value> {
    let rows = p.len();
    let cols = p.first().map_or(0, Vec::len);
    let data = p.into_iter().flatten().map(num_val).collect::<R<Vec<_>>>()?;
    array_val(rows, cols, data)
}
/// Partial-pivot Gaussian elimination with multiple right sides and residual validation.
/// Tiny pivots return #NUM instead of inventing an equilibrium or finite absorption time.
fn solve(mut a: Matrix, mut rhs: Matrix) -> R<Matrix> {
    let n = a.len();
    let m = rhs.first().map_or(0, Vec::len);
    let original = a.clone();
    let original_rhs = rhs.clone();
    let norm = a.iter().map(|r| r.iter().map(|x| x.abs()).sum::<f64>()).fold(0.0, f64::max);
    for k in 0..n {
        let mut pivot = k;
        for i in k + 1..n {
            if a[i][k].abs() > a[pivot][k].abs() {
                pivot = i;
            }
        }
        if a[pivot][k].abs() <= 64.0 * f64::EPSILON * norm {
            return Err(CellError::Num);
        }
        a.swap(k, pivot);
        rhs.swap(k, pivot);
        let pivot_row = a[k].clone();
        let pivot_rhs = rhs[k].clone();
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            a[i][k] = 0.0;
            for (value, pivot) in a[i].iter_mut().zip(&pivot_row).skip(k + 1) {
                *value -= f * pivot;
            }
            for (value, pivot) in rhs[i].iter_mut().zip(&pivot_rhs) {
                *value -= f * pivot;
            }
        }
    }
    let mut x = vec![vec![0.0; m]; n];
    for i in (0..n).rev() {
        for j in 0..m {
            let s = (i + 1..n).map(|k| a[i][k] * x[k][j]).sum::<f64>();
            x[i][j] = (rhs[i][j] - s) / a[i][i];
            if !x[i][j].is_finite() {
                return Err(CellError::Num);
            }
        }
    }
    let xn = x.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
    let bn = original_rhs.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
    for i in 0..n {
        for j in 0..m {
            let r = (0..n).map(|k| original[i][k] * x[k][j]).sum::<f64>() - original_rhs[i][j];
            if r.abs() > 1e-10 * (norm * xn + bn).max(f64::MIN_POSITIVE) {
                return Err(CellError::Num);
            }
        }
    }
    Ok(x)
}

/// Evaluate the discrete multi-period transition matrix for an exact integer horizon.
fn markov_power(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    spill(power(transition(a, 0)?, integer(a, 1, 1_000_000_000)?)?)
}
/// Return the initial row distribution advanced through the supplied transition horizon.
fn state(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let p = transition(a, 1)?;
    let initial = vector(a, 0, p.len())?;
    stochastic(&vec![initial.clone()])?;
    spill(multiply(&vec![initial], &power(p, integer(a, 2, 1_000_000_000)?)?)?)
}
/// Solve for a unique stationary distribution and verify its normalization and fixed-point residual.
fn stationary(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let p = transition(a, 0)?;
    let n = p.len();
    let mut system = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            system[i][j] = p[j][i] - if i == j { 1.0 } else { 0.0 };
        }
    }
    system[n - 1].fill(1.0);
    let mut rhs = vec![vec![0.0]; n];
    rhs[n - 1][0] = 1.0;
    let mut x = solve(system, rhs)?;
    for row in &mut x {
        if row[0] < -TOL || row[0] > 1.0 + TOL {
            return Err(CellError::Num);
        }
        row[0] = row[0].clamp(0.0, 1.0);
    }
    let pi: Vec<f64> = x.iter().map(|r| r[0]).collect();
    let moved = multiply(&vec![pi.clone()], &p)?;
    if moved[0].iter().zip(&pi).any(|(a, b)| (a - b).abs() > 1e-10) {
        return Err(CellError::Num);
    }
    spill(x)
}
/// Solve transient-state absorption times and destination probabilities; terminal states must follow transient states.
fn absorption(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let p = transition(a, 0)?;
    let n = p.len();
    let t = integer(a, 1, n)?;
    if t == 0 || t == n {
        return Err(CellError::Num);
    }
    for (i, row) in p.iter().enumerate().skip(t) {
        for (j, x) in row.iter().enumerate() {
            if *x != if i == j { 1.0 } else { 0.0 } {
                return Err(CellError::Num);
            }
        }
    }
    let system: Matrix = (0..t).map(|i| (0..t).map(|j| if i == j { 1.0 - p[i][j] } else { -p[i][j] }).collect()).collect();
    let rhs: Matrix = (0..t).map(|i| std::iter::once(1.0).chain(p[i][t..].iter().copied()).collect()).collect();
    let out = solve(system, rhs)?;
    for row in &out {
        if row[0] < 0.0 || row[1..].iter().any(|v| *v < -TOL || *v > 1.0 + TOL) || (row[1..].iter().sum::<f64>() - 1.0).abs() > 1e-9 {
            return Err(CellError::Num);
        }
    }
    spill(out)
}
/// Value advance state payments and arrears transition benefits over a finite discrete horizon.
fn reward(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let p = transition(a, 0)?;
    let n = p.len();
    let rewards = vector(a, 1, n)?;
    let rate = scalar(a, 2)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let periods = integer(a, 3, 10_000)?;
    let benefits = if has(a, 4) { matrix(a, 4)? } else { vec![vec![0.0; n]; n] };
    if benefits.len() != n {
        return Err(CellError::Value);
    }
    let expected: Vec<f64> = (0..n).map(|i| (0..n).map(|j| p[i][j] * benefits[i][j]).sum()).collect();
    let discount = 1.0 / (1.0 + rate);
    let mut value = vec![0.0; n];
    for _ in 0..periods {
        let next: Vec<f64> = (0..n).map(|i| rewards[i] + discount * (expected[i] + (0..n).map(|j| p[i][j] * value[j]).sum::<f64>())).collect();
        if next.iter().any(|x| !x.is_finite()) {
            return Err(CellError::Num);
        }
        value = next;
    }
    spill(value.into_iter().map(|x| vec![x]).collect())
}
/// Estimate transition rows from nonnegative counts; a zero-count row has no identifiable estimate.
fn fit(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let mut counts = matrix(a, 0)?;
    for row in &mut counts {
        if row.iter().any(|x| *x < 0.0) {
            return Err(CellError::Num);
        }
        let scale = row.iter().copied().fold(0.0, f64::max);
        if scale == 0.0 {
            return Err(CellError::Num);
        }
        let total = row.iter().map(|x| x / scale).sum::<f64>();
        for x in row {
            *x = (*x / scale) / total;
        }
    }
    spill(counts)
}
/// Validate a continuous-time generator with nonnegative off-diagonals and zero row sums.
fn generator(a: &[Arg], i: usize) -> R<Matrix> {
    let q = matrix(a, i)?;
    for (i, row) in q.iter().enumerate() {
        let exit = -row[i];
        if exit < 0.0 || row.iter().enumerate().any(|(j, v)| i != j && *v < 0.0) {
            return Err(CellError::Num);
        }
        let sum = row.iter().sum::<f64>();
        if !sum.is_finite() || sum.abs() > TOL * exit.max(1e-300) {
            return Err(CellError::Num);
        }
    }
    Ok(q)
}
/// Compute continuous-time transition probabilities by bounded Poisson uniformization and scaling/squaring.
fn continuous(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let q = generator(a, 0)?;
    let n = q.len();
    let time = scalar(a, 1)?;
    if time < 0.0 {
        return Err(CellError::Num);
    }
    let lambda = (0..n).map(|i| -q[i][i]).fold(0.0, f64::max);
    if lambda == 0.0 || time == 0.0 {
        return spill(identity(n));
    }
    let mut x = lambda * time;
    if !x.is_finite() {
        return Err(CellError::Num);
    }
    let mut squarings = 0;
    while x > 8.0 {
        x *= 0.5;
        squarings += 1;
        if squarings > 40 {
            return Err(CellError::Num);
        }
    }
    let mut r = identity(n);
    for i in 0..n {
        for j in 0..n {
            r[i][j] += q[i][j] / lambda;
        }
    }
    let mut term = identity(n);
    let mut weight = (-x).exp();
    let mut out: Matrix = term.iter().map(|row| row.iter().map(|v| v * weight).collect()).collect();
    let mut converged = false;
    for k in 1..=128 {
        term = multiply(&term, &r)?;
        weight *= x / k as f64;
        for i in 0..n {
            for j in 0..n {
                out[i][j] += weight * term[i][j];
            }
        }
        // Bound omitted Poisson series geometrically, rather than testing a term at k=0.
        if k as f64 > x && weight * (x / (k as f64 + 1.0)) / (1.0 - x / (k as f64 + 2.0)) < f64::EPSILON * 0.125 {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(CellError::Num);
    }
    for _ in 0..squarings {
        out = multiply(&out, &out)?;
    }
    // Scaling/squaring magnifies rounding; reject results that no longer represent probabilities.
    for row in &out {
        if row.iter().any(|v| *v < -TOL || *v > 1.0 + 1e-8) || (row.iter().sum::<f64>() - 1.0).abs() > 1e-8 {
            return Err(CellError::Num);
        }
    }
    spill(out)
}
/// Estimate generator intensities from observed off-diagonal counts and strictly positive time at risk.
fn continuous_fit(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let mut q = matrix(a, 0)?;
    let n = q.len();
    let exposure = vector(a, 1, n)?;
    for i in 0..n {
        if exposure[i] <= 0.0 || q[i][i] != 0.0 || q[i].iter().any(|v| *v < 0.0) {
            return Err(CellError::Num);
        }
        for value in &mut q[i] {
            *value /= exposure[i];
        }
        q[i][i] = -q[i].iter().sum::<f64>();
    }
    spill(q)
}

/// Register deterministic homogeneous, calendar-dependent and semi-Markov worksheet contracts.
pub(super) fn specs() -> Vec<FnSpec> {
    let mut result = vec![
        act!(
            "ACT.MARKOV.POWER",
            2,
            2,
            Statistical,
            &[false, true],
            "ACT.MARKOV.POWER(transition,periods)",
            "Multi-period row-stochastic transition matrix; integer horizon.",
            markov_power
        ),
        act!(
            "ACT.MARKOV.STATE",
            3,
            3,
            Statistical,
            &[false, false, true],
            "ACT.MARKOV.STATE(initial,transition,periods)",
            "Row-vector distribution after a discrete-time Markov horizon.",
            state
        ),
        act!(
            "ACT.MARKOV.STATIONARY",
            1,
            1,
            Statistical,
            &[false],
            "ACT.MARKOV.STATIONARY(transition)",
            "Unique stationary distribution; nonunique or ill-conditioned systems return #NUM!.",
            stationary
        ),
        act!(
            "ACT.MARKOV.ABSORPTION",
            2,
            2,
            Statistical,
            &[false, true],
            "ACT.MARKOV.ABSORPTION(transition,transient_states)",
            "Expected absorption steps and absorption probabilities; transient states first.",
            absorption
        ),
        act!(
            "ACT.MARKOV.REWARD",
            4,
            5,
            Financial,
            &[false, false, true, true, false],
            "ACT.MARKOV.REWARD(transition,state_rewards,rate,periods,[transition_benefits])",
            "Finite-horizon multi-state values: state payments in advance, transition payments in arrears.",
            reward
        ),
        act!(
            "ACT.MARKOV.FIT",
            1,
            1,
            Statistical,
            &[false],
            "ACT.MARKOV.FIT(transition_counts)",
            "Row-normalized empirical transition counts; empty exposure rows are rejected.",
            fit
        ),
        act!(
            "ACT.CTMC.TRANSITION",
            2,
            2,
            Statistical,
            &[false, true],
            "ACT.CTMC.TRANSITION(generator,time)",
            "Continuous-time transition probabilities by bounded uniformization and scaling/squaring.",
            continuous
        ),
        act!(
            "ACT.CTMC.FIT",
            2,
            2,
            Statistical,
            &[false, false],
            "ACT.CTMC.FIT(transition_counts,state_exposures)",
            "Homogeneous continuous-time generator MLE from off-diagonal counts and time at risk.",
            continuous_fit
        ),
    ];
    result.extend(temporal::specs());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;
    fn mat(rows: Vec<Vec<f64>>) -> Arg {
        av(arr(rows.into_iter().map(|r| r.into_iter().map(nv).collect()).collect()))
    }
    fn values(v: Value) -> Vec<f64> {
        match v {
            Value::Array(a) => a.iter().map(|v| number(v).unwrap()).collect(),
            Value::Number(x) => vec![x],
            other => panic!("{other:?}"),
        }
    }
    fn close(v: Value, expected: &[f64]) {
        let actual = values(v);
        assert_eq!(actual.len(), expected.len());
        for (x, y) in actual.iter().zip(expected) {
            assert!((x - y).abs() < 1e-9 * (1.0 + y.abs()), "{x} != {y}");
        }
    }
    #[test]
    fn discrete_closed_forms_and_stationarity() {
        let p = || mat(vec![vec![0.8, 0.2], vec![0.1, 0.9]]);
        close(ev("ACT.MARKOV.POWER", vec![p(), n(0.0)]), &[1.0, 0.0, 0.0, 1.0]);
        close(ev("ACT.MARKOV.POWER", vec![p(), n(2.0)]), &[0.66, 0.34, 0.17, 0.83]);
        close(ev("ACT.MARKOV.STATIONARY", vec![p()]), &[1.0 / 3.0, 2.0 / 3.0]);
        close(ev("ACT.MARKOV.STATE", vec![mat(vec![vec![1.0, 0.0]]), p(), n(2.0)]), &[0.66, 0.34]);
        assert_eq!(ev("ACT.MARKOV.STATIONARY", vec![mat(identity(2))]), Value::Error(CellError::Num));
    }
    #[test]
    fn absorption_geometric_and_multiple_destinations() {
        close(ev("ACT.MARKOV.ABSORPTION", vec![mat(vec![vec![0.5, 0.2, 0.3], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]]), n(1.0)]), &[2.0, 0.4, 0.6]);
        assert_eq!(ev("ACT.MARKOV.ABSORPTION", vec![mat(identity(2)), n(1.0)]), Value::Error(CellError::Num));
    }
    #[test]
    fn reward_matches_life_insurance_and_annuity() {
        let p = || mat(vec![vec![0.9, 0.1], vec![0.0, 1.0]]);
        let rewards = || mat(vec![vec![1.0], vec![0.0]]);
        let value = ev("ACT.MARKOV.REWARD", vec![p(), rewards(), n(0.05), n(3.0)]);
        close(value, &[1.0 + 0.9 / 1.05 + (0.9_f64 / 1.05).powi(2), 0.0]);
        let b = mat(vec![vec![0.0, 100.0], vec![0.0, 0.0]]);
        close(
            ev("ACT.MARKOV.REWARD", vec![p(), mat(vec![vec![0.0], vec![0.0]]), n(0.05), n(3.0), b]),
            &[100.0 * 0.1 / 1.05 * (1.0 + 0.9 / 1.05 + (0.9_f64 / 1.05).powi(2)), 0.0],
        );
    }
    #[test]
    fn continuous_exponential_and_semigroup() {
        let q = || mat(vec![vec![-0.2, 0.2], vec![0.0, 0.0]]);
        let survival = (-2.0_f64).exp();
        close(ev("ACT.CTMC.TRANSITION", vec![q(), n(10.0)]), &[survival, 1.0 - survival, 0.0, 1.0]);
        let a = values(ev("ACT.CTMC.TRANSITION", vec![q(), n(5.0)]));
        close(ev("ACT.MARKOV.POWER", vec![mat(vec![a[0..2].to_vec(), a[2..4].to_vec()]), n(2.0)]), &[survival, 1.0 - survival, 0.0, 1.0]);
        close(ev("ACT.CTMC.TRANSITION", vec![q(), n(1000.0)]), &[0.0, 1.0, 0.0, 1.0]);
    }
    #[test]
    fn estimates_and_validation() {
        close(ev("ACT.MARKOV.FIT", vec![mat(vec![vec![8.0, 2.0], vec![1.0, 9.0]])]), &[0.8, 0.2, 0.1, 0.9]);
        close(ev("ACT.CTMC.FIT", vec![mat(vec![vec![0.0, 2.0], vec![1.0, 0.0]]), mat(vec![vec![10.0, 20.0]])]), &[-0.2, 0.2, 0.05, -0.05]);
        for p in [mat(vec![vec![0.5, 0.6], vec![0.0, 1.0]]), mat(vec![vec![1.1, -0.1], vec![0.0, 1.0]])] {
            assert_eq!(ev("ACT.MARKOV.POWER", vec![p, n(2.0)]), Value::Error(CellError::Num));
        }
        assert_eq!(ev("ACT.MARKOV.POWER", vec![mat(identity(2)), n(0.5)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.MARKOV.POWER", vec![mat(identity(33)), n(2.0)]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.CTMC.TRANSITION", vec![mat(vec![vec![-1.0, 2.0], vec![0.0, 0.0]]), n(1.0)]), Value::Error(CellError::Num));
    }

    #[test]
    fn decimal_roundoff_does_not_create_probability_mass_over_long_horizons() {
        let p = mat(vec![vec![0.5, 0.5 + 4e-13], vec![0.25, 0.75]]);
        let result = values(ev("ACT.MARKOV.POWER", vec![p, n(1_000_000_000.0)]));
        for row in result.chunks(2) {
            assert!((row.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            assert!((row[0] - 1.0 / 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn skipped_transition_benefits_mean_no_transition_payment() {
        let p = || mat(vec![vec![0.8, 0.2], vec![0.1, 0.9]]);
        let rewards = || mat(vec![vec![1.0], vec![0.0]]);
        close(ev("ACT.MARKOV.REWARD", vec![p(), rewards(), n(0.05), n(2.0), empty()]), &[1.0 + 0.8 / 1.05, 0.1 / 1.05]);
        assert_eq!(ev("ACT.MARKOV.REWARD", vec![p(), rewards(), n(0.05), n(2.0), Arg::reference(Value::Empty)]), Value::Error(CellError::Value));
    }
}
