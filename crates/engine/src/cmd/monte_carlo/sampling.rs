//! Independent randomized Latin hypercubes, optionally transformed by a Gaussian copula.
//! LHS stratification is on the independent driving uniforms, before the copula transform.
use super::{CMD, Distribution, Input, Json, Result, Rng, Value, bad, integer};

const MATRIX_TOLERANCE: f64 = 1e-10;

/// Reuses the worksheet's statistical implementation and rejects errors or non-finite results.
pub(super) fn worksheet_number(name: &str, args: &[Value]) -> Result<f64> {
    super::super::analysis::call(name, args)
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| bad(CMD, format!("{name} could not evaluate a sampling statistic")))
}
/// Evaluates the standard-normal quantile, clamping endpoints to finite f64 tail probabilities.
pub(super) fn normal_inverse(p: f64) -> Result<f64> {
    worksheet_number("NORM.S.INV", &[Value::Number(p.clamp(f64::EPSILON / 2.0, 1.0 - f64::EPSILON / 2.0))])
}
/// Evaluates the worksheet's standard-normal CDF for mapping latent draws to other marginals.
fn normal_cdf(z: f64) -> Result<f64> {
    worksheet_number("NORM.S.DIST", &[Value::Number(z), Value::Bool(true)])
}

pub(super) struct Plan {
    pub latin: bool,
    pub replicates: usize,
    pub per_replica: usize,
    pub matrix: Option<Vec<Vec<f64>>>,
    factor: Option<Vec<Vec<f64>>>,
}
impl Plan {
    /// Validates the sampling method, complete replicas and optional latent correlation matrix.
    /// LHS needs at least two independent replicas; every replica needs at least two samples.
    /// Matrices must be finite, square, symmetric, unit-diagonal and positive semidefinite.
    pub fn parse(p: &Json, dimensions: usize, iterations: usize) -> Result<Self> {
        let latin = match p.get("method") {
            None => false,
            Some(Json::String(t)) if t == "random" => false,
            Some(Json::String(t)) if t == "latinHypercube" => true,
            _ => return Err(bad(CMD, "method must be random or latinHypercube")),
        };
        let replicates = integer(p, "replicates", if latin { 8 } else { 1 }, 128)? as usize;
        if replicates == 0 || (latin && replicates < 2) || !iterations.is_multiple_of(replicates) || iterations / replicates < 2 {
            return Err(bad(
                CMD,
                "iterations must divide into equal replicas of at least 2 samples; LHS needs at least 2 independent replicas (default 8)",
            ));
        }
        let matrix = match p.get("correlation") {
            None | Some(Json::Null) => None,
            Some(Json::Array(rows)) if rows.len() == dimensions => {
                let mut matrix = Vec::with_capacity(dimensions);
                for row in rows {
                    let row =
                        row.as_array().filter(|r| r.len() == dimensions).ok_or_else(|| bad(CMD, "correlation must be an inputs × inputs matrix"))?;
                    let row = row
                        .iter()
                        .map(|v| {
                            v.as_f64()
                                .filter(|n| n.is_finite() && (-1.0..=1.0).contains(n))
                                .ok_or_else(|| bad(CMD, "correlations must be finite numbers between -1 and 1"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    matrix.push(row);
                }
                Some(matrix)
            }
            _ => return Err(bad(CMD, "correlation must be an inputs × inputs matrix")),
        };
        let factor = matrix.as_ref().map(|m| cholesky(m)).transpose()?;
        Ok(Self { latin, replicates, per_replica: iterations / replicates, matrix, factor })
    }
    /// Returns the command/configuration spelling of the selected sampling method.
    pub fn method_name(&self) -> &'static str {
        if self.latin { "latinHypercube" } else { "random" }
    }
    /// Reports whether inputs have no nonzero latent cross-correlation; this does not
    /// assert independence between the stratified rows within an LHS replica.
    pub fn independent(&self) -> bool {
        self.matrix.as_ref().is_none_or(|m| m.iter().enumerate().all(|(i, row)| row.iter().enumerate().all(|(j, v)| i == j || *v == 0.0)))
    }
    /// Generates input-major sample columns in consecutive, independently randomized replicas.
    /// LHS stratifies driving uniforms before the optional Gaussian-copula transform, which
    /// need not preserve final marginal strata. Draw failures and non-finite samples are errors.
    pub fn sample(&self, inputs: &[Input], seed: u64) -> Result<Vec<Vec<f64>>> {
        let n = self.per_replica;
        let d = inputs.len();
        let total = n * self.replicates;
        let mut rng = Rng(seed);
        let prepared = inputs.iter().map(|input| input.distribution.prepare()).collect::<Result<Vec<_>>>()?;
        let mut xs = (0..d).map(|_| Vec::with_capacity(total)).collect::<Vec<_>>();
        if !self.latin && self.factor.is_none() {
            for row in 0..total {
                if row.is_multiple_of(128) && !gridcraft_calc::WorkHook::allowed() {
                    return Err(bad(CMD, "sampling stopped by work limit or cancellation"));
                }
                for ((input, table), column) in inputs.iter().zip(&prepared).zip(&mut xs) {
                    let x = input.distribution.sample(&mut rng, table.as_ref())?;
                    if !x.is_finite() {
                        return Err(bad(CMD, "non-finite sampled value"));
                    }
                    column.push(x);
                }
            }
            return Ok(xs);
        }
        for _ in 0..self.replicates {
            if !gridcraft_calc::WorkHook::allowed() {
                return Err(bad(CMD, "sampling stopped by work limit or cancellation"));
            }
            let mut uniforms = Vec::with_capacity(d);
            for _ in 0..d {
                let mut column = if self.latin {
                    (0..n).map(|i| (i as f64 + rng.uniform()) / n as f64).collect::<Vec<_>>()
                } else {
                    (0..n).map(|_| rng.uniform()).collect::<Vec<_>>()
                };
                if self.latin {
                    for i in (1..n).rev() {
                        let j = rng.below(i + 1)?;
                        column.swap(i, j);
                    }
                }
                uniforms.push(column);
            }
            for k in 0..n {
                if k.is_multiple_of(128) && !gridcraft_calc::WorkHook::allowed() {
                    return Err(bad(CMD, "sampling stopped by work limit or cancellation"));
                }
                let z = if self.factor.is_some() {
                    uniforms.iter().map(|c| normal_inverse(c.get(k).copied().unwrap_or(0.5))).collect::<Result<Vec<_>>>()?
                } else {
                    Vec::new()
                };
                for (i, (input, column)) in inputs.iter().zip(&mut xs).enumerate() {
                    let x = if let Some(factor) = &self.factor {
                        let row = factor.get(i).ok_or_else(|| bad(CMD, "missing correlation factor"))?;
                        let correlated = row.iter().zip(&z).map(|(a, z)| a * z).sum::<f64>();
                        match input.distribution {
                            Distribution::Normal(m, sd) => m + sd * correlated,
                            Distribution::Lognormal(mean, cv) => super::stochastic::lognormal(mean, cv, correlated)?,
                            _ => input.distribution.inverse_cdf(normal_cdf(correlated)?, prepared.get(i).and_then(Option::as_ref))?,
                        }
                    } else {
                        input
                            .distribution
                            .inverse_cdf(uniforms.get(i).and_then(|c| c.get(k)).copied().unwrap_or(0.5), prepared.get(i).and_then(Option::as_ref))?
                    };
                    if !x.is_finite() {
                        return Err(bad(CMD, "non-finite correlated sample"));
                    }
                    column.push(x);
                }
            }
        }
        Ok(xs)
    }
}

/// PSD Cholesky accepts singular ±1 correlations, but never repairs an invalid matrix.
fn cholesky(matrix: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = matrix.len();
    let mut factor = vec![vec![0.0; n]; n];
    let at = |i: usize, j: usize| matrix.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
    for i in 0..n {
        if (at(i, i) - 1.0).abs() > MATRIX_TOLERANCE {
            return Err(bad(CMD, "correlation diagonal must be 1"));
        }
        for j in 0..=i {
            if (at(i, j) - at(j, i)).abs() > MATRIX_TOLERANCE {
                return Err(bad(CMD, "correlation matrix must be symmetric"));
            }
            let product = factor.get(i).into_iter().flatten().zip(factor.get(j).into_iter().flatten()).take(j).map(|(a, b)| a * b).sum::<f64>();
            let residual = at(i, j) - product;
            let entry = if i == j {
                if residual < -MATRIX_TOLERANCE {
                    return Err(bad(CMD, "correlation matrix must be positive semidefinite"));
                }
                residual.max(0.0).sqrt()
            } else {
                let pivot = factor.get(j).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
                if pivot > 0.0 {
                    residual / pivot
                } else if residual.abs() <= MATRIX_TOLERANCE {
                    0.0
                } else {
                    return Err(bad(CMD, "correlation matrix must be positive semidefinite"));
                }
            };
            if !entry.is_finite() {
                return Err(bad(CMD, "invalid correlation factor"));
            }
            if let Some(slot) = factor.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot = entry;
            }
        }
    }
    Ok(factor)
}
