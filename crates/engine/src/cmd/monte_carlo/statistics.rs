//! Stable summaries and sampling-error diagnostics. LHS inference uses independent replicas.
use super::sampling::worksheet_number;
use super::{CMD, Input, Json, Output, Plan, Result, StyleId, Value, bad, json, ranks};

/// Accumulates bounded finite terms using Neumaier compensation to retain rounding residuals.
fn sum(values: impl Iterator<Item = f64>) -> f64 {
    let (mut total, mut correction) = (0.0_f64, 0.0_f64);
    for x in values {
        let next = total + x;
        correction += if total.abs() >= x.abs() { (total - next) + x } else { (x - next) + total };
        total = next;
    }
    total + correction
}

/// Returns the mean and sample SD (n − 1 denominator) for at least two finite observations.
/// Compensated centering preserves large offsets and mixed-sign cancellation; scaled squared
/// deviations avoid unnecessary overflow. Unrepresentable final statistics return an error.
pub(super) fn moments(values: &[f64]) -> Result<(f64, f64)> {
    if values.len() < 2 || values.iter().any(|x| !x.is_finite()) {
        return Err(bad(CMD, "statistics need at least two finite samples"));
    }
    let n = values.len() as f64;
    let origin = values.first().copied().unwrap_or(0.0);
    let mixed_signs = values.iter().any(|x| *x < 0.0) && values.iter().any(|x| *x > 0.0);
    let mean = if mixed_signs {
        // Centering across opposite signs can erase a small residual (e.g. 1e16, -1e16, 1).
        // Divide before summation to bound intermediates without discarding that residual.
        sum(values.iter().map(|x| x / n))
    } else {
        // Same-sign differences are finite; centering retains variation around a large offset.
        origin + sum(values.iter().map(|x| (x - origin) / n))
    };
    let (deviations, unit) = centered(values, mean);
    let sd = if unit == 0.0 { 0.0 } else { unit * (sum(deviations.iter().map(|x| x * x)) / (n - 1.0)).max(0.0).sqrt() };
    if !mean.is_finite() || !sd.is_finite() {
        return Err(bad(CMD, "statistics overflowed; rescale the model"));
    }
    Ok((mean, sd))
}
/// Returns dimensionless centered deviations and their scale for validated finite samples.
/// If direct subtraction would overflow, normalization precedes centering; constants use scale 0.
fn centered(values: &[f64], mean: f64) -> (Vec<f64>, f64) {
    let safe = values.iter().all(|x| (x - mean).is_finite());
    let scale =
        if safe { values.iter().map(|x| (x - mean).abs()).fold(0.0_f64, f64::max) } else { values.iter().map(|x| x.abs()).fold(0.0_f64, f64::max) };
    if scale == 0.0 {
        return (vec![0.0; values.len()], 0.0);
    }
    (values.iter().map(|x| if safe { (x - mean) / scale } else { x / scale - mean / scale }).collect(), scale)
}
/// Estimates Pearson correlation using scaled centered products, clamping rounding drift to ±1.
/// Returns None for unequal lengths, constant data or nonrepresentable underlying moments.
pub(super) fn correl(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() {
        return None;
    }
    let (ma, _) = moments(a).ok()?;
    let (mb, _) = moments(b).ok()?;
    let (a, sa) = centered(a, ma);
    let (b, sb) = centered(b, mb);
    if sa == 0.0 || sb == 0.0 {
        return None;
    }
    let aa = sum(a.iter().map(|x| x * x));
    let bb = sum(b.iter().map(|x| x * x));
    let ab = sum(a.iter().zip(&b).map(|(a, b)| a * b));
    let r = ab / aa.sqrt() / bb.sqrt();
    r.is_finite().then_some(r.clamp(-1.0, 1.0))
}
/// Obtains the two-sided 95% Student t critical value from the worksheet implementation.
fn t_critical(df: usize) -> Result<f64> {
    worksheet_number("T.INV.2T", &[Value::Number(0.05), Value::Number(df as f64)])
}
/// Builds symmetric bounds, returning None when either endpoint is not representable.
fn interval(mean: f64, half: f64) -> Option<(f64, f64)> {
    let (a, b) = (mean - half, mean + half);
    (a.is_finite() && b.is_finite()).then_some((a, b))
}
/// Computes 95% Wilson score bounds for a validated binomial count with nonzero n.
/// Zero/all observed events retain a non-degenerate interval instead of implying certainty.
fn wilson(successes: usize, n: usize) -> Result<(f64, f64)> {
    let z = worksheet_number("NORM.S.INV", &[Value::Number(0.975)])?;
    let n = n as f64;
    let p = successes as f64 / n;
    let zz = z * z;
    let den = 1.0 + zz / n;
    let center = (p + zz / (2.0 * n)) / den;
    let half = z * ((p * (1.0 - p) + zz / (4.0 * n)) / n).sqrt() / den;
    Ok(((center - half).max(0.0), (center + half).min(1.0)))
}

pub(super) struct Accuracy {
    label: String,
    n: usize,
    mean: f64,
    se: f64,
    mean_ci: Option<(f64, f64)>,
    prob: f64,
    prob_se: f64,
    prob_ci: Option<(f64, f64)>,
    units: usize,
    method: &'static str,
}
impl Accuracy {
    /// Estimates finite-simulation error for a complete prefix of random trials or LHS replicas.
    /// IID means use t intervals and probabilities use Wilson bounds; LHS uses variation between
    /// independent replica summaries. A constant LHS proportion has no reported probability CI.
    /// Incomplete replicas and overflowing mean intervals are errors.
    fn calculate(label: &str, values: &[f64], plan: &Plan, replicas: usize) -> Result<Self> {
        let n = values.len();
        let (mean, sd) = moments(values)?;
        let successes = values.iter().filter(|v| **v > 0.0).count();
        let prob = successes as f64 / n as f64;
        let (se, prob_se, mean_ci, prob_ci, units, method) = if plan.latin {
            let means = values.chunks_exact(plan.per_replica).map(|x| moments(x).map(|m| m.0)).collect::<Result<Vec<_>>>()?;
            let probs =
                values.chunks_exact(plan.per_replica).map(|x| x.iter().filter(|v| **v > 0.0).count() as f64 / x.len() as f64).collect::<Vec<_>>();
            if means.len() != replicas {
                return Err(bad(CMD, "incomplete independent LHS replica"));
            }
            let (_, sm) = moments(&means)?;
            let (_, sp) = moments(&probs)?;
            let factor = t_critical(replicas - 1)?;
            let se = sm / (replicas as f64).sqrt();
            let pse = sp / (replicas as f64).sqrt();
            // A zero estimated variance of rare-event replica proportions is not proof of p=0 or p=1.
            let pci = if pse > 0.0 { interval(prob, factor * pse).map(|(a, b)| (a.max(0.0), b.min(1.0))) } else { None };
            (se, pse, interval(mean, factor * se), pci, replicas, "Independent LHS replicas; Student t approximation")
        } else {
            let se = sd / (n as f64).sqrt();
            let factor = t_critical(n - 1)?;
            (
                se,
                (prob * (1.0 - prob) / n as f64).sqrt(),
                interval(mean, factor * se),
                Some(wilson(successes, n)?),
                n,
                "IID Monte Carlo; Student t mean / Wilson probability",
            )
        };
        if mean_ci.is_none() {
            return Err(bad(CMD, "mean confidence interval overflowed; rescale the model"));
        }
        Ok(Self { label: label.into(), n, mean, se, mean_ci, prob, prob_se, prob_ci, units, method })
    }
    /// Serializes error estimates and explicitly distinguishes unavailable probability bounds
    /// from zero tail risk. Relative mean error is absent for zero/unrepresentable denominators.
    fn json(&self) -> Json {
        json!({"cell":self.label,"samples":self.n,"independentUnits":self.units,"mean":self.mean,"meanMCSE":self.se,"meanCI95":self.mean_ci,"probabilityAboveZero":self.prob,"probabilityMCSE":self.prob_se,"probabilityCI95":self.prob_ci,"method":self.method,"probabilityIntervalAvailable":self.prob_ci.is_some(),"probabilityIntervalNote":if self.prob_ci.is_none(){Some("Replica proportions have no observed variation; confidence interval unavailable. This does not prove zero tail risk.")}else{None},"relativeMeanMCSE":if self.mean!=0.0{(self.se/self.mean.abs()).is_finite().then_some(self.se/self.mean.abs())}else{None}})
    }
}
struct Pair {
    a: String,
    b: String,
    target: f64,
    pearson: Option<f64>,
    spearman: Option<f64>,
}
pub(super) struct Diagnostics {
    accuracy: Vec<Accuracy>,
    trace: Vec<Accuracy>,
    pairs: Vec<Pair>,
    latin: bool,
}
impl Diagnostics {
    /// Builds per-output accuracy and complete-prefix convergence traces, plus pairwise observed
    /// input correlations. LHS trace points end on independent replica boundaries, never midway
    /// through dependent rows; construction propagates any invalid numeric diagnostic.
    pub fn new(plan: &Plan, inputs: &[Input], outputs: &[Output], xs: &[Vec<f64>], ys: &[Vec<f64>]) -> Result<Self> {
        let mut accuracy = Vec::new();
        let mut trace = Vec::new();
        for (output, values) in outputs.iter().zip(ys) {
            accuracy.push(Accuracy::calculate(&output.label, values, plan, plan.replicates)?);
            let final_count = if plan.latin { plan.replicates } else { values.len() };
            let mut points = Vec::new();
            let mut k = if plan.latin { 2 } else { 32 };
            while k < final_count {
                points.push(k);
                k = k.saturating_mul(2);
            }
            points.push(final_count);
            for k in points {
                let count = if plan.latin { k * plan.per_replica } else { k };
                if let Some(prefix) = values.get(..count) {
                    trace.push(Accuracy::calculate(&output.label, prefix, plan, if plan.latin { k } else { 1 })?);
                }
            }
        }
        let rank_inputs = xs.iter().map(|x| ranks(x)).collect::<Vec<_>>();
        let mut pairs = Vec::new();
        for (i, a) in inputs.iter().enumerate() {
            for (j, b) in inputs.iter().enumerate().skip(i + 1) {
                if let (Some(x), Some(y), Some(rx), Some(ry)) = (xs.get(i), xs.get(j), rank_inputs.get(i), rank_inputs.get(j)) {
                    pairs.push(Pair {
                        a: a.label.clone(),
                        b: b.label.clone(),
                        target: plan.matrix.as_ref().and_then(|m| m.get(i)).and_then(|r| r.get(j)).copied().unwrap_or(0.0),
                        pearson: correl(x, y),
                        spearman: correl(rx, ry),
                    });
                }
            }
        }
        Ok(Self { accuracy, trace, pairs, latin: plan.latin })
    }
    /// Returns sampling-error summaries for all configured formula outputs.
    pub fn accuracy_json(&self) -> Vec<Json> {
        self.accuracy.iter().map(Accuracy::json).collect()
    }
    /// Returns only the first output's convergence points for the command's compact response.
    pub fn first_trace_json(&self) -> Vec<Json> {
        let first = self.accuracy.first().map(|a| a.label.as_str());
        self.trace.iter().filter(|a| Some(a.label.as_str()) == first).map(Accuracy::json).collect()
    }
    /// Writes static accuracy, convergence and dependence tables through the report cell sink.
    /// Returns the next free row and the inclusive first-output trace row bounds for charting.
    /// LHS probability-interval warnings keep absent bounds visible as a statistical limitation.
    pub fn write(&self, put: &mut dyn FnMut(u32, u32, Value, StyleId), start: u32, bold: StyleId) -> (u32, u32, u32) {
        let mut r = start;
        let normal = StyleId::DEFAULT;
        put(
            r,
            0,
            Value::text(
                "Sampling error: 95% intervals estimate numerical error, not business uncertainty. Student t is approximate for non-normal or small samples.",
            ),
            bold,
        );
        r += 1;
        for (c, h) in [
            "Output",
            "Mean MCSE",
            "Mean CI95 lower",
            "Mean CI95 upper",
            "P(>0) MCSE",
            "P(>0) CI95 lower",
            "P(>0) CI95 upper",
            "Independent units",
            "Inference method",
        ]
        .iter()
        .enumerate()
        {
            put(r, c as u32, Value::text(*h), bold);
        }
        r += 1;
        for a in &self.accuracy {
            put(r, 0, Value::text(a.label.as_str()), normal);
            for (c, v) in [
                Some(a.se),
                a.mean_ci.map(|x| x.0),
                a.mean_ci.map(|x| x.1),
                Some(a.prob_se),
                a.prob_ci.map(|x| x.0),
                a.prob_ci.map(|x| x.1),
                Some(a.units as f64),
            ]
            .into_iter()
            .enumerate()
            {
                put(r, c as u32 + 1, v.map(Value::Number).unwrap_or_default(), normal);
            }
            put(r, 8, Value::text(a.method), normal);
            r += 1;
        }
        if self.latin {
            put(
                r,
                0,
                Value::text(
                    "Blank LHS probability intervals mean replica proportions had no variation; do not interpret this as zero tail risk. Increase independent replicas.",
                ),
                normal,
            );
            r += 1;
        }
        r += 1;
        put(r, 0, Value::text("Convergence trace: complete prefixes / independent LHS replicas, not an automatic stopping guarantee."), bold);
        r += 1;
        for (c, h) in
            ["Output", "Samples", "Mean", "Mean MCSE", "Mean CI95 lower", "Mean CI95 upper", "P(>0)", "Independent units"].iter().enumerate()
        {
            put(r, c as u32, Value::text(*h), bold);
        }
        r += 1;
        let first_trace_start = r;
        let first_label = self.accuracy.first().map(|a| a.label.as_str());
        let first_trace_count = self.trace.iter().take_while(|a| Some(a.label.as_str()) == first_label).count() as u32;
        for a in &self.trace {
            put(r, 0, Value::text(a.label.as_str()), normal);
            for (c, v) in
                [Some(a.n as f64), Some(a.mean), Some(a.se), a.mean_ci.map(|x| x.0), a.mean_ci.map(|x| x.1), Some(a.prob), Some(a.units as f64)]
                    .into_iter()
                    .enumerate()
            {
                put(r, c as u32 + 1, v.map(Value::Number).unwrap_or_default(), normal);
            }
            r += 1;
        }
        r += 1;
        put(
            r,
            0,
            Value::text(
                "Input dependence: requested latent Gaussian correlation vs observed sample correlations. These differ for non-normal marginals.",
            ),
            bold,
        );
        r += 1;
        for (c, h) in ["Input 1", "Input 2", "Requested latent correlation", "Observed Pearson", "Observed Spearman"].iter().enumerate() {
            put(r, c as u32, Value::text(*h), bold);
        }
        r += 1;
        for p in &self.pairs {
            put(r, 0, Value::text(p.a.as_str()), normal);
            put(r, 1, Value::text(p.b.as_str()), normal);
            put(r, 2, Value::Number(p.target), normal);
            put(r, 3, p.pearson.map(Value::Number).unwrap_or_default(), normal);
            put(r, 4, p.spearman.map(Value::Number).unwrap_or_default(), normal);
            r += 1;
        }
        (r, first_trace_start, first_trace_start + first_trace_count.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Compares mean/t and boundary-count Wilson intervals with independent NIST reference values.
    #[test]
    fn monte_carlo_mean_error_and_wilson_match_reference_values() {
        let plan = Plan::parse(&json!({}), 1, 5).unwrap();
        let a = Accuracy::calculate("X", &[1., 2., 3., 4., 5.], &plan, 1).unwrap();
        assert!((a.mean - 3.).abs() < 1e-12);
        assert!((a.se - 0.5_f64.sqrt()).abs() < 1e-12);
        let (lo, hi) = a.mean_ci.unwrap();
        assert!((lo - 1.036756838522439).abs() < 1e-8);
        assert!((hi - 4.963243161477561).abs() < 1e-8);
        let (lo, hi) = wilson(0, 10).unwrap();
        assert!(lo.abs() < 1e-12);
        assert!((hi - 0.2775327998628892).abs() < 1e-10);
        let (lo, hi) = wilson(10, 10).unwrap();
        assert!((lo - 0.7224672001371107).abs() < 1e-10);
        assert!((hi - 1.).abs() < 1e-12);
    }
}
