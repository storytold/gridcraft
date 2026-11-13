//! Calendar-dependent transition products and duration-dependent semi-Markov rewards.
//! Duration is the number of completed periods in the current state; a state change resets
//! it to zero. Supplied duration tables must cover every possible path over the horizon.

use super::{
    Arg, CellError, Ctx, FnSpec, MAX_STATES, Matrix, R, Value, arg, array_val, as_array, has, identity, integer, matrix, multiply, normalize, number,
    scalar, spill, stochastic, vector,
};

const MAX_PERIODS: usize = 1000;
const MAX_WORK: usize = 20_000_000;

struct Blocks {
    matrices: Vec<Matrix>,
    n: usize,
}

/// Parse vertically stacked stochastic matrices with bounded dimensions and total multiplication work.
fn blocks(a: &[Arg], i: usize) -> R<Blocks> {
    let arr = as_array(&arg(a, i)?.value);
    let n = arr.cols;
    if n == 0 || n > MAX_STATES || arr.rows == 0 || !arr.rows.is_multiple_of(n) {
        return Err(CellError::Value);
    }
    let count = arr.rows / n;
    if count > MAX_PERIODS + 1 || count.saturating_mul(n).saturating_mul(n).saturating_mul(n) > MAX_WORK {
        return Err(CellError::Num);
    }
    let values = arr.iter().map(number).collect::<R<Vec<_>>>()?;
    let mut matrices = Vec::with_capacity(count);
    for block in values.chunks(n * n) {
        let mut p: Matrix = block.chunks(n).map(<[f64]>::to_vec).collect();
        stochastic(&p)?;
        normalize(&mut p)?;
        matrices.push(p);
    }
    Ok(Blocks { matrices, n })
}
/// Multiply calendar-specific transitions in chronological order and return the final transition matrix.
fn nonhomogeneous(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let data = blocks(a, 0)?;
    let mut out = identity(data.n);
    for p in data.matrices {
        out = multiply(&out, &p)?;
        normalize(&mut out)?;
    }
    spill(out)
}
/// Value calendar-specific advance state payments and fixed arrears transition benefits by backward recursion.
fn nonhomogeneous_reward(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let data = blocks(a, 0)?;
    let n = data.n;
    let periods = data.matrices.len();
    let payments = as_array(&arg(a, 1)?.value);
    if payments.rows != periods || payments.cols != n {
        return Err(CellError::Value);
    }
    let rewards = payments.iter().map(number).collect::<R<Vec<_>>>()?;
    let rate = scalar(a, 2)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let benefits = if has(a, 3) { matrix(a, 3)? } else { vec![vec![0.0; n]; n] };
    if benefits.len() != n {
        return Err(CellError::Value);
    }
    let mut values = vec![0.0; n];
    let discount = 1.0 / (1.0 + rate);
    for t in (0..periods).rev() {
        let mut next = vec![0.0; n];
        for (i, row) in data.matrices[t].iter().enumerate() {
            let continuation = row.iter().enumerate().map(|(j, p)| p * (benefits[i][j] + values[j])).sum::<f64>();
            next[i] = rewards[t * n + i] + discount * continuation;
            if !next[i].is_finite() {
                return Err(CellError::Num);
            }
        }
        values = next;
    }
    spill(values.into_iter().map(|x| vec![x]).collect())
}
/// Validate a duration table and require coverage of every possible tenure over the requested horizon.
fn duration_parameters(a: &[Arg], table: usize, horizon: usize, duration: usize) -> R<(Blocks, usize, usize)> {
    let data = blocks(a, table)?;
    let horizon = integer(a, horizon, MAX_PERIODS)?;
    let duration = if has(a, duration) { integer(a, duration, MAX_PERIODS)? } else { 0 };
    let end = horizon.checked_add(duration).ok_or(CellError::Num)?;
    if end > data.matrices.len() || horizon.saturating_mul(end + 1).saturating_mul(data.n).saturating_mul(data.n) > MAX_WORK {
        return Err(CellError::Num);
    }
    Ok((data, horizon, duration))
}
/// Propagate joint state/tenure mass, resetting tenure after state changes; optionally retain the full probability path.
fn semi_evolve(a: &[Arg], trace: bool) -> R<Value> {
    // (duration transition blocks, initial probabilities, horizon, [initial duration])
    let (data, horizon, duration) = duration_parameters(a, 0, 2, 3)?;
    let n = data.n;
    let dmax = duration + horizon;
    let initial = vector(a, 1, n)?;
    let mut initial_matrix = vec![initial];
    stochastic(&initial_matrix)?;
    normalize(&mut initial_matrix)?;
    let mut current = vec![vec![0.0; n]; dmax + 1];
    current[duration] = initial_matrix.remove(0);
    let mut output = Vec::with_capacity(if trace { (horizon + 1) * n } else { n });
    if trace {
        output.extend(current[duration].iter().copied().map(Value::Number));
    }
    for t in 0..horizon {
        let mut next = vec![vec![0.0; n]; dmax + 1];
        for (d, row) in current.iter().enumerate().take(duration + t + 1) {
            for (i, mass) in row.iter().copied().enumerate() {
                if mass == 0.0 {
                    continue;
                }
                let p = &data.matrices[d][i];
                for (j, probability) in p.iter().enumerate() {
                    let dest = if i == j { d + 1 } else { 0 };
                    next[dest][j] += mass * probability;
                }
            }
        }
        let total = next.iter().flatten().sum::<f64>();
        if !total.is_finite() || (total - 1.0).abs() > 1e-10 {
            return Err(CellError::Num);
        }
        for mass in next.iter_mut().flatten() {
            *mass /= total;
        }
        current = next;
        if trace {
            for i in 0..n {
                output.push(Value::Number(current.iter().map(|row| row[i]).sum()));
            }
        }
    }
    if !trace {
        for i in 0..n {
            output.push(Value::Number(current.iter().map(|row| row[i]).sum()));
        }
    }
    array_val(if trace { horizon + 1 } else { 1 }, n, output)
}
/// Return the finite-horizon state distribution of a supplied duration-dependent transition model.
fn semi_state(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    semi_evolve(a, false)
}
/// Return state probabilities from time zero through the semi-Markov horizon.
fn semi_trace(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    semi_evolve(a, true)
}
/// Value duration-dependent state and transition payments without extrapolating beyond supplied tenure probabilities.
fn semi_reward(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    // (duration transition blocks, state rewards, rate, horizon, [initial duration], [benefits])
    let (data, horizon, duration) = duration_parameters(a, 0, 3, 4)?;
    let n = data.n;
    let dmax = duration + horizon;
    let rewards = vector(a, 1, n)?;
    let rate = scalar(a, 2)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let benefits = if has(a, 5) { matrix(a, 5)? } else { vec![vec![0.0; n]; n] };
    if benefits.len() != n {
        return Err(CellError::Value);
    }
    let discount = 1.0 / (1.0 + rate);
    let mut values = vec![vec![0.0; n]; dmax + 1];
    for t in (0..horizon).rev() {
        let mut next = vec![vec![0.0; n]; dmax + 1];
        for d in 0..=duration + t {
            for i in 0..n {
                let future = data.matrices[d][i]
                    .iter()
                    .enumerate()
                    .map(|(j, p)| p * (benefits[i][j] + values[if i == j { d + 1 } else { 0 }][j]))
                    .sum::<f64>();
                next[d][i] = rewards[i] + discount * future;
                if !next[d][i].is_finite() {
                    return Err(CellError::Num);
                }
            }
        }
        values = next;
    }
    spill(values[duration].iter().map(|x| vec![*x]).collect())
}
/// Register calendar-dependent transition products and duration-dependent state/reward functions.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.MARKOV.NONHOMOGENEOUS",
            1,
            1,
            Statistical,
            &[false],
            "ACT.MARKOV.NONHOMOGENEOUS(calendar_blocks)",
            "Ordered product of calendar-specific transition matrices stacked vertically.",
            nonhomogeneous
        ),
        act!(
            "ACT.MARKOV.NONHOMOGENEOUS.REWARD",
            3,
            4,
            Financial,
            &[false, false, true, false],
            "ACT.MARKOV.NONHOMOGENEOUS.REWARD(calendar_blocks,state_payments,rate,[transition_benefits])",
            "Prospective values for calendar-specific transitions and period/state advance payments.",
            nonhomogeneous_reward
        ),
        act!(
            "ACT.SEMIMARKOV.STATE",
            3,
            4,
            Statistical,
            &[false, false, true, true],
            "ACT.SEMIMARKOV.STATE(duration_blocks,initial,horizon,[initial_duration])",
            "Duration-dependent state distribution; a state change resets duration to zero.",
            semi_state
        ),
        act!(
            "ACT.SEMIMARKOV.TRACE",
            3,
            4,
            Statistical,
            &[false, false, true, true],
            "ACT.SEMIMARKOV.TRACE(duration_blocks,initial,horizon,[initial_duration])",
            "State probability path including time zero for a bounded duration-dependent model.",
            semi_trace
        ),
        act!(
            "ACT.SEMIMARKOV.REWARD",
            4,
            6,
            Financial,
            &[false, false, true, true, true, false],
            "ACT.SEMIMARKOV.REWARD(duration_blocks,state_rewards,rate,horizon,[initial_duration],[transition_benefits])",
            "Duration-dependent prospective values with advance state and arrears transition payments.",
            semi_reward
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{Arg, CellError, Value, number};
    use crate::util::testutil::{arr, av, empty, ev, n, nv};
    fn m(rows: Vec<Vec<f64>>) -> Arg {
        av(arr(rows.into_iter().map(|r| r.into_iter().map(nv).collect()).collect()))
    }
    fn vals(value: Value) -> Vec<f64> {
        match value {
            Value::Array(a) => a.iter().map(|v| number(v).unwrap()).collect(),
            other => panic!("{other:?}"),
        }
    }
    fn check(value: Value, expected: &[f64]) {
        let result = vals(value);
        assert_eq!(result.len(), expected.len());
        for (x, y) in result.iter().zip(expected) {
            assert!((x - y).abs() < 1e-10 * (1.0 + y.abs()), "{x} != {y}");
        }
    }
    fn homogeneous(periods: usize) -> Arg {
        m((0..periods).flat_map(|_| vec![vec![0.8, 0.2], vec![0.1, 0.9]]).collect())
    }
    #[test]
    fn calendar_product_and_rewards() {
        check(ev("ACT.MARKOV.NONHOMOGENEOUS", vec![homogeneous(2)]), &[0.66, 0.34, 0.17, 0.83]);
        check(ev("ACT.MARKOV.NONHOMOGENEOUS", vec![m(vec![vec![0.8, 0.2], vec![0.0, 1.0], vec![0.5, 0.5], vec![0.0, 1.0]])]), &[0.4, 0.6, 0.0, 1.0]);
        let value = ev("ACT.MARKOV.NONHOMOGENEOUS.REWARD", vec![homogeneous(2), m(vec![vec![1.0, 0.0], vec![1.0, 0.0]]), n(0.05)]);
        check(value, &[1.0 + 0.8 / 1.05, 0.1 / 1.05]);
    }
    #[test]
    fn duration_independence_recovers_markov_model() {
        let initial = || m(vec![vec![1.0, 0.0]]);
        check(ev("ACT.SEMIMARKOV.STATE", vec![homogeneous(3), initial(), n(3.0)]), &[0.562, 0.438]);
        check(ev("ACT.SEMIMARKOV.TRACE", vec![homogeneous(2), initial(), n(2.0)]), &[1.0, 0.0, 0.8, 0.2, 0.66, 0.34]);
        let semi = ev("ACT.SEMIMARKOV.REWARD", vec![homogeneous(3), m(vec![vec![1.0], vec![0.0]]), n(0.05), n(3.0)]);
        let markov = ev("ACT.MARKOV.REWARD", vec![m(vec![vec![0.8, 0.2], vec![0.1, 0.9]]), m(vec![vec![1.0], vec![0.0]]), n(0.05), n(3.0)]);
        check(semi, &vals(markov));
    }
    #[test]
    fn real_duration_dependence_resets_and_conditions_on_initial_tenure() {
        let table = || m(vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![0.0, 1.0], vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 0.0]]);
        check(ev("ACT.SEMIMARKOV.TRACE", vec![table(), m(vec![vec![1.0, 0.0]]), n(3.0)]), &[1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
        check(ev("ACT.SEMIMARKOV.STATE", vec![table(), m(vec![vec![1.0, 0.0]]), n(1.0), n(1.0)]), &[0.0, 1.0]);
    }
    #[test]
    fn duration_horizon_and_shapes_are_not_extrapolated() {
        assert_eq!(ev("ACT.SEMIMARKOV.STATE", vec![homogeneous(2), m(vec![vec![1.0, 0.0]]), n(3.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.MARKOV.NONHOMOGENEOUS", vec![m(vec![vec![0.8, 0.2]])]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.SEMIMARKOV.STATE", vec![homogeneous(2), m(vec![vec![1.0, 1.0]]), n(1.0)]), Value::Error(CellError::Num));
    }

    #[test]
    fn skipped_temporal_options_preserve_zero_rewards_and_zero_initial_duration() {
        check(
            ev("ACT.MARKOV.NONHOMOGENEOUS.REWARD", vec![homogeneous(2), m(vec![vec![1.0, 0.0], vec![1.0, 0.0]]), n(0.05), empty()]),
            &[1.0 + 0.8 / 1.05, 0.1 / 1.05],
        );
        let value = ev("ACT.SEMIMARKOV.REWARD", vec![homogeneous(2), m(vec![vec![1.0], vec![0.0]]), n(0.05), n(2.0), empty(), empty()]);
        check(value, &[1.0 + 0.8 / 1.05, 0.1 / 1.05]);
    }
}
