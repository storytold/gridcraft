//! Seeded Monte Carlo over arbitrary workbook formulas. Trials never modify source cells.
//! Tornado sensitivity is computed from the same samples (conditional decile medians), not
//! from one-at-a-time substitutions. Sampling supports replicated LHS and Gaussian dependence.

use std::collections::HashSet;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Cell, Chart, ChartKind, LegendPos, Series, Sheet, StyleId};
use serde_json::{Value as Json, json};

use super::whatif::Trial;
use super::{CommandSpec, EngineError, Result, Session, Workbook, bad, cmd, edit, has_doc, split_sheet, str_param};

mod discrete;
mod sampling;
mod statistics;
mod stochastic;
use sampling::Plan;
use statistics::Diagnostics;

const CMD: &str = "data.monteCarlo";
const MAX_INPUTS: usize = 32;
const MAX_OUTPUTS: usize = 64;
const MAX_ITERATIONS: u64 = 100_000;
const MAX_SAMPLE_CELLS: usize = 1_000_000;

/// Registers the generic, undoable Monte Carlo command for UI and programmatic callers.
pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "data.monteCarlo",
        "Monte Carlo…",
        ["Data", "Analysis"],
        None,
        "{inputs: [{cell: \"Inputs!B2\", distribution: uniform|triangular|normal|lognormal|poisson|binomial|hypergeometric, min?, mode?, max?, mean?, stdDev?, cv?, trials?, probability?, draws?, successes?, population?}], outputs: [\"Model!F30\", ..], iterations?: 1000 (2–100000, ≤1000000 sampled cells), seed?: 0, samples?: true, maxWork?: positive integer ≤ 100000000, newSheet?: name, method?: random|latinHypercube, replicates?: 1 (LHS default 8, at least 2), correlation?: [[1,0.5],[0.5,1]] (Gaussian latent correlation, input order)}. Includes Monte Carlo error estimates and convergence; formula outputs are recalculated on isolated workbook copies. Creates a static report with P10/P50/P90, probability > 0, samples and a conditional Monte Carlo tornado for the first output. Source inputs remain unchanged. Seed controls inputs and MC.* worksheet streams (stochasticVersion 1), not RAND/NOW. Lognormal uses arithmetic mean and CV; conditional counts and stationary AR1 use MC.* formulas.",
        has_doc,
        run
    )]
}

#[derive(Clone, Debug)]
enum Distribution {
    Uniform(f64, f64),
    Triangular(f64, f64, f64),
    Normal(f64, f64),
    Lognormal(f64, f64),
    Poisson(f64),
    Binomial(u64, f64),
    Hypergeometric(u64, u64, u64),
}

/// SplitMix64, as used by the evolutionary solver; no platform RNG or new dependency.
struct Rng(u64);
impl Rng {
    /// Shares the existing Box–Muller transform with conditional processes without another RNG.
    fn normal(&mut self) -> f64 {
        let u = self.uniform();
        (-2.0 * (1.0 - u).ln()).sqrt() * (std::f64::consts::TAU * self.uniform()).cos()
    }
    /// Advances SplitMix64 with wrapping arithmetic so the seed sequence is platform independent.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uses 53 random bits to produce a uniform value in the half-open interval `[0, 1)`.
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Draws an unbiased shuffle index below `n` using bounded rejection sampling.
    /// Rejects an empty range or failure to find an acceptable draw within 16 attempts.
    fn below(&mut self, n: usize) -> Result<usize> {
        let bound = n as u64;
        if bound == 0 {
            return Err(bad(CMD, "empty shuffle"));
        }
        let threshold = bound.wrapping_neg() % bound;
        for _ in 0..16 {
            let x = self.next();
            if x >= threshold {
                return Ok((x % bound) as usize);
            }
        }
        Err(bad(CMD, "random shuffle failed"))
    }
}
impl Distribution {
    /// Parses finite marginal parameters, allowing degenerate constants but rejecting
    /// reversed/non-finite widths, out-of-range triangular modes and negative deviations.
    fn parse(p: &Json) -> Result<Self> {
        let num = |k: &str| p.get(k).and_then(Json::as_f64).filter(|n| n.is_finite()).ok_or_else(|| bad(CMD, format!("missing or invalid `{k}`")));
        match str_param(p, "distribution").unwrap_or("uniform") {
            "uniform" | "triangular" => {
                let (a, b) = (num("min")?, num("max")?);
                if a > b || !(b - a).is_finite() {
                    return Err(bad(CMD, "min must not exceed max, with a finite width"));
                }
                if str_param(p, "distribution") == Some("triangular") {
                    let m = num("mode")?;
                    if m < a || m > b {
                        return Err(bad(CMD, "mode must be between min and max"));
                    }
                    Ok(Self::Triangular(a, m, b))
                } else {
                    Ok(Self::Uniform(a, b))
                }
            }
            "normal" => {
                let (m, sd) = (num("mean")?, num("stdDev")?);
                if sd < 0.0 {
                    return Err(bad(CMD, "stdDev must not be negative"));
                }
                Ok(Self::Normal(m, sd))
            }
            "lognormal" => {
                let (mean, cv) = (num("mean")?, num("cv")?);
                stochastic::lognormal(mean, cv, 0.0)?;
                Ok(Self::Lognormal(mean, cv))
            }
            "poisson" => {
                let mean = num("mean")?;
                if !(0.0..=discrete::MAX_POISSON_MEAN).contains(&mean) {
                    return Err(bad(CMD, "Poisson mean must be between 0 and 50000"));
                }
                Ok(Self::Poisson(mean))
            }
            "binomial" => {
                let trials = count_parameter(p, "trials")?;
                let probability = num("probability")?;
                if !(0.0..=1.0).contains(&probability)
                    || ((0.0..1.0).contains(&probability) && probability > 0.0 && trials.saturating_add(1) > discrete::MAX_SUPPORT)
                {
                    return Err(bad(CMD, "binomial requires probability 0..1 and at most 100000 nondegenerate outcomes"));
                }
                Ok(Self::Binomial(trials, probability))
            }
            "hypergeometric" => {
                let draws = count_parameter(p, "draws")?;
                let successes = count_parameter(p, "successes")?;
                let population = count_parameter(p, "population")?;
                if draws > population || successes > population {
                    return Err(bad(CMD, "hypergeometric draws/successes cannot exceed population"));
                }
                let low = draws.saturating_sub(population - successes);
                if draws.min(successes).saturating_sub(low).saturating_add(1) > discrete::MAX_SUPPORT {
                    return Err(bad(CMD, "hypergeometric support exceeds 100000 outcomes"));
                }
                Ok(Self::Hypergeometric(draws, successes, population))
            }
            _ => Err(bad(CMD, "distribution must be uniform, triangular, normal, lognormal, poisson, binomial or hypergeometric")),
        }
    }
    /// Builds reusable discrete inverse tables only within the charged sampling phase.
    fn prepare(&self) -> Result<Option<discrete::QuantileTable>> {
        match *self {
            Self::Poisson(mean) => discrete::QuantileTable::poisson(mean).map(Some),
            Self::Binomial(n, p) => discrete::QuantileTable::binomial(n, p).map(Some),
            Self::Hypergeometric(n, k, total) => discrete::QuantileTable::hypergeometric(n, k, total).map(Some),
            _ => Ok(None),
        }
    }
    /// Maps a driving probability in `[0, 1]` to this marginal; normal endpoints are
    /// clamped by the shared inverse-normal helper. Non-finite results are errors.
    fn inverse_cdf(&self, u: f64, prepared: Option<&discrete::QuantileTable>) -> Result<f64> {
        if let Some(table) = prepared {
            // A Gaussian CDF can round to1; the discrete driver remains in its open upper domain.
            return table.quantile(u.min(1.0 - f64::EPSILON / 2.0));
        }
        let x = match *self {
            Self::Uniform(a, b) => a + (b - a) * u,
            Self::Triangular(a, _, b) if a == b => a,
            Self::Triangular(a, m, b) => {
                let w = b - a;
                let cut = (m - a) / w;
                if u < cut { a + w * (u * cut).sqrt() } else { b - w * ((1.0 - u) * (1.0 - cut)).sqrt() }
            }
            Self::Normal(m, sd) => m + sd * sampling::normal_inverse(u)?,
            Self::Lognormal(mean, cv) => return stochastic::lognormal(mean, cv, sampling::normal_inverse(u)?),
            Self::Poisson(_) | Self::Binomial(_, _) | Self::Hypergeometric(_, _, _) => return Err(bad(CMD, "missing prepared discrete marginal")),
        };
        if x.is_finite() { Ok(x) } else { Err(bad(CMD, "distribution produced a non-finite value")) }
    }
    /// Draws an independent marginal sample, using Box–Muller for normals.
    /// The caller validates finiteness before applying the value to a workbook trial.
    fn sample(&self, rng: &mut Rng, prepared: Option<&discrete::QuantileTable>) -> Result<f64> {
        let u = rng.uniform();
        if let Some(table) = prepared {
            return table.quantile(u);
        }
        let x = match *self {
            Self::Uniform(a, b) => a + (b - a) * u,
            Self::Triangular(a, _, b) if a == b => a,
            Self::Triangular(a, m, b) => {
                let width = b - a;
                let mode = (m - a) / width;
                // Normalize before the square root to avoid overflowing width².
                if u < mode { a + width * (u * mode).sqrt() } else { b - width * ((1.0 - u) * (1.0 - mode)).sqrt() }
            }
            Self::Normal(m, sd) => m + sd * (-2.0 * (1.0 - u).ln()).sqrt() * (std::f64::consts::TAU * rng.uniform()).cos(),
            Self::Lognormal(mean, cv) => {
                return stochastic::lognormal(mean, cv, (-2.0 * (1.0 - u).ln()).sqrt() * (std::f64::consts::TAU * rng.uniform()).cos());
            }
            Self::Poisson(_) | Self::Binomial(_, _) | Self::Hypergeometric(_, _, _) => return Err(bad(CMD, "missing prepared discrete marginal")),
        };
        Ok(x)
    }
}

/// Count marginals never truncate fractional JSON values or exceed exact, bounded populations.
fn count_parameter(p: &Json, key: &str) -> Result<u64> {
    p.get(key)
        .and_then(Json::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= discrete::MAX_POPULATION as f64 && n.fract() == 0.0)
        .map(|n| n as u64)
        .ok_or_else(|| bad(CMD, format!("{key} must be a whole count between 0 and 1000000000")))
}

struct Input {
    sheet: usize,
    cell: CellRef,
    label: String,
    distribution: Distribution,
}
struct Output {
    sheet: usize,
    cell: CellRef,
    label: String,
}

/// Resolves a cell against its named sheet or the active sheet and returns a canonical
/// quoted reference. Missing sheets and invalid/out-of-grid cell addresses are errors.
fn address(wb: &Workbook, text: &str) -> Result<(usize, CellRef, String)> {
    let (name, body) = split_sheet(text);
    let si = match name {
        Some(name) => wb.sheet_index(&name).ok_or_else(|| bad(CMD, format!("no sheet named `{name}`")))?,
        None => wb.active_sheet(),
    };
    let cell = CellRef::parse(body.trim()).ok_or_else(|| bad(CMD, format!("not a cell: `{text}`")))?;
    let sh = wb.sheet(si).ok_or(EngineError::NoDocument)?;
    Ok((si, cell, format!("{}!{}", gridcraft_formula::quote_sheet(&sh.name), cell.a1())))
}

/// Reads a non-negative integer with a missing-value default and an inclusive resource cap.
/// A present value with the wrong type, sign or magnitude is rejected rather than defaulted.
fn integer(p: &Json, key: &str, default: u64, max: u64) -> Result<u64> {
    let n = match p.get(key) {
        None => default,
        Some(v) => v.as_u64().ok_or_else(|| bad(CMD, format!("`{key}` must be a non-negative integer")))?,
    };
    if n > max {
        return Err(bad(CMD, format!("`{key}` must not exceed {max}")));
    }
    Ok(n)
}

/// Inclusive linear interpolation, matching PERCENTILE.INC.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let k = p * sorted.len().saturating_sub(1) as f64;
    let lo = k.floor() as usize;
    let a = sorted.get(lo).copied().unwrap_or(0.0);
    let b = sorted.get(lo.saturating_add(1)).copied().unwrap_or(a);
    // Weighted average avoids overflowing b-a for opposite extreme finite values.
    a * (1.0 - k.fract()) + b * k.fract()
}
/// Copies finite samples into total numeric order without changing their trial order.
fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v
}
/// Returns zero-based average ranks in original sample order, assigning tied values
/// the same rank so constant inputs have undefined rather than spurious Spearman correlation.
fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|a, b| values.get(*a).unwrap_or(&0.0).total_cmp(values.get(*b).unwrap_or(&0.0)));
    let mut out = vec![0.0; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i + 1;
        while j < order.len() && order.get(i).and_then(|k| values.get(*k)) == order.get(j).and_then(|k| values.get(*k)) {
            j += 1;
        }
        let rank = (i + j - 1) as f64 / 2.0;
        for k in order.get(i..j).unwrap_or_default() {
            if let Some(v) = out.get_mut(*k) {
                *v = rank;
            }
        }
        i = j;
    }
    out
}
/// Delegates to the stable Pearson estimator; rank-transformed samples yield Spearman's rho.
fn correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    statistics::correl(a, b)
}

#[derive(Clone)]
struct Summary {
    label: String,
    mean: f64,
    sd: f64,
    min: f64,
    p10: f64,
    median: f64,
    p90: f64,
    max: f64,
    above: f64,
}
impl Summary {
    /// Serializes descriptive outcome statistics, distinct from sampling-error intervals.
    fn json(&self) -> Json {
        json!({"cell":self.label,"mean":self.mean,"stdDev":self.sd,"min":self.min,"p10":self.p10,"p50":self.median,"p90":self.p90,"max":self.max,"probabilityAboveZero":self.above})
    }
}
#[derive(Clone)]
struct Sensitivity {
    input: String,
    low: f64,
    high: f64,
    low_n: usize,
    high_n: usize,
    spearman: Option<f64>,
    adverse: f64,
    favorable: f64,
    span: f64,
}
impl Sensitivity {
    /// Serializes conditional decile-group effects and sizes; these are not causal OAT effects.
    fn json(&self) -> Json {
        json!({"input":self.input,"lowMedian":self.low,"highMedian":self.high,"lowCount":self.low_n,"highCount":self.high_n,"spearman":self.spearman,"adverse":self.adverse,"favorable":self.favorable,"span":self.span})
    }
}

/// Validates the bounded simulation plan, evaluates isolated what-if trials, and publishes
/// a static report only after all finite-output and diagnostic checks succeed. Errors leave
/// source cells unchanged; report publication is one undoable edit.
/// Incremental simulation state. Only publication changes the source session; stepping and
/// cancellation are portable to native and WASM hosts without worker-thread requirements.
pub(crate) struct Job {
    pub id: u64,
    pub auto_advance: bool,
    source: Arc<Workbook>,
    doc: u64,
    revision: u64,
    name: String,
    config: Json,
    seed: u64,
    iterations: usize,
    keep_samples: bool,
    inputs: Vec<Input>,
    outputs: Vec<Output>,
    plan: Plan,
    xs: Option<Vec<Vec<f64>>>,
    ys: Vec<Vec<f64>>,
    trial: Trial,
    completed: usize,
    sample_work: u64,
    finish_work: u64,
    report_cells: usize,
    work_limit: u64,
    stochastic: stochastic::Stats,
    sheet_names: Arc<Vec<String>>,
}

impl Job {
    /// Parses the same configuration for synchronous and incremental execution, rejecting
    /// excessive work/report growth before copying calculation state or allocating samples.
    pub(crate) fn prepare(s: &Session, p: &Json) -> Result<Self> {
        let d = s.doc()?;
        if d.wb.protected_structure {
            return Err(bad(CMD, "the workbook structure is protected"));
        }
        let raw_inputs = p
            .get("inputs")
            .and_then(Json::as_array)
            .filter(|a| !a.is_empty() && a.len() <= MAX_INPUTS)
            .ok_or_else(|| bad(CMD, "give 1–32 inputs"))?;
        let raw_outputs = p
            .get("outputs")
            .and_then(Json::as_array)
            .filter(|a| !a.is_empty() && a.len() <= MAX_OUTPUTS)
            .ok_or_else(|| bad(CMD, "give 1–64 output cells"))?;
        let iterations = integer(p, "iterations", 1000, MAX_ITERATIONS)? as usize;
        if iterations < 2 {
            return Err(bad(CMD, "iterations must be at least 2"));
        }
        if iterations.saturating_mul(raw_inputs.len() + raw_outputs.len()) > MAX_SAMPLE_CELLS {
            return Err(bad(CMD, "too many sampled cells (at most 1000000)"));
        }
        let seed = integer(p, "seed", 0, u64::MAX)?;
        if integer(p, "stochasticVersion", 1, 1)? != 1 {
            return Err(bad(CMD, "stochasticVersion must be 1"));
        }
        let keep_samples = match p.get("samples") {
            None => true,
            Some(v) => v.as_bool().ok_or_else(|| bad(CMD, "samples must be a boolean"))?,
        };
        let name = match p.get("newSheet") {
            Some(Json::String(n)) if !n.trim().is_empty() => n.trim().to_string(),
            None | Some(Json::String(_)) => (1..=100_000)
                .map(|i| if i == 1 { "Monte Carlo".to_string() } else { format!("Monte Carlo {i}") })
                .find(|n| d.wb.sheet_index(n).is_none())
                .ok_or_else(|| bad(CMD, "no report sheet name available"))?,
            _ => return Err(bad(CMD, "newSheet must be a string")),
        };
        d.wb.check_sheet_name(&name, None).map_err(EngineError::Other)?;
        let mut used = HashSet::new();
        let mut inputs = Vec::with_capacity(raw_inputs.len());
        for item in raw_inputs {
            let text = str_param(item, "cell").ok_or_else(|| bad(CMD, "each input needs a cell"))?;
            let (sheet, cell, label) = address(&d.wb, text)?;
            if !used.insert((sheet, cell)) {
                return Err(bad(CMD, "input cells must be unique"));
            }
            let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
            if sh.cell(cell).is_some_and(|c| c.formula.is_some()) || !matches!(sh.value(cell),Value::Number(n) if n.is_finite()) {
                return Err(bad(CMD, format!("input `{label}` must be a numeric constant")));
            }
            inputs.push(Input { sheet, cell, label, distribution: Distribution::parse(item)? });
        }
        let mut outputs = Vec::with_capacity(raw_outputs.len());
        let mut seen_outputs = HashSet::new();
        for item in raw_outputs {
            let text = item.as_str().ok_or_else(|| bad(CMD, "outputs must be cell references"))?;
            let (sheet, cell, label) = address(&d.wb, text)?;
            if !seen_outputs.insert((sheet, cell)) {
                return Err(bad(CMD, "output cells must be unique"));
            }
            outputs.push(Output { sheet, cell, label });
        }
        // Canonical cross-sheet refs let the report retain a reusable model even when active sheet changes.
        let mut config = p.clone();
        if let Some(c) = config.as_object_mut() {
            c.remove("newSheet");
            c.insert("seed".into(), json!(seed));
            c.insert("stochasticVersion".into(), json!(1));
            c.insert("iterations".into(), json!(iterations));
            let canonical = raw_inputs
                .iter()
                .zip(&inputs)
                .map(|(p, i)| {
                    let mut p = p.clone();
                    if let Some(o) = p.as_object_mut() {
                        o.insert("cell".into(), json!(i.label));
                    }
                    p
                })
                .collect::<Vec<_>>();
            c.insert("inputs".into(), json!(canonical));
            c.insert("outputs".into(), json!(outputs.iter().map(|o| &o.label).collect::<Vec<_>>()));
        }
        let plan = Plan::parse(p, inputs.len(), iterations)?;
        if let Some(c) = config.as_object_mut() {
            c.insert("method".into(), json!(plan.method_name()));
            c.insert("replicates".into(), json!(plan.replicates));
        }
        let width = inputs.len().saturating_add(outputs.len()).saturating_add(1);
        let report_cells = 50_000usize.saturating_add(if keep_samples { iterations.saturating_add(1).saturating_mul(width) } else { 0 });
        crate::workload::report_capacity(&d.wb, report_cells)?;
        let work_limit = integer(p, "maxWork", crate::workload::MAX_RUN_WORK, crate::workload::MAX_RUN_WORK)?;
        if work_limit == 0 {
            return Err(bad(CMD, "maxWork must be positive"));
        }
        let dimensions = inputs.len() as u64;
        let sample_work = (iterations as u64).saturating_mul(dimensions).saturating_mul(if plan.matrix.is_some() { dimensions + 3 } else { 2 });
        let logarithm = (usize::BITS - iterations.leading_zeros()) as u64 + 2;
        let report_work = 64u64
            .saturating_add((outputs.len() as u64).saturating_mul(128))
            .saturating_add(dimensions.saturating_mul(dimensions).saturating_mul(8))
            .saturating_add(if keep_samples { (iterations as u64 + 1).saturating_mul(width as u64) } else { 0 });
        let finish_work = (iterations as u64)
            .saturating_mul((width as u64).saturating_mul(logarithm).saturating_add(dimensions.saturating_mul(dimensions).saturating_mul(4)))
            .saturating_add(report_work)
            .saturating_add(d.calc.formula_count() as u64);
        let trial_work = d.calc.formula_count().saturating_add(inputs.len()).saturating_add(outputs.len()).saturating_add(1) as u64;
        let estimate = sample_work.saturating_add(finish_work).saturating_add(trial_work.saturating_mul(iterations as u64));
        if estimate > work_limit {
            return Err(bad(CMD, format!("estimated analysis work {estimate} exceeds maxWork {work_limit}; reduce model size/iterations")));
        }
        let trial = Trial::new(d);
        trial.meter.set_limit(work_limit)?;
        Ok(Self {
            id: 0,
            auto_advance: false,
            source: d.wb.clone(),
            doc: d.uid,
            revision: d.revision,
            name,
            config,
            seed,
            iterations,
            keep_samples,
            inputs,
            outputs,
            plan,
            xs: None,
            ys: (0..raw_outputs.len()).map(|_| Vec::with_capacity(iterations)).collect(),
            trial,
            completed: 0,
            sample_work,
            finish_work,
            report_cells,
            work_limit,
            stochastic: stochastic::Stats::default(),
            sheet_names: Arc::new(d.wb.sheets.iter().map(|sheet| sheet.name.clone()).collect()),
        })
    }

    /// Refuses stale jobs if an embedding host edited or closed the source outside command guards.
    pub fn check_source(&self, s: &Session) -> Result<()> {
        let unchanged = s.documents().iter().any(|d| d.uid == self.doc && d.revision == self.revision && Arc::ptr_eq(&d.wb, &self.source));
        if unchanged { Ok(()) } else { Err(bad(CMD, "source workbook changed or closed; the analysis was discarded")) }
    }

    /// Stable source identity for targeted publication without changing another workbook.
    pub fn source_doc(&self) -> u64 {
        self.doc
    }

    /// Canonical synchronous parameters for journal replay without ephemeral job identities.
    pub fn configuration(&self) -> Json {
        self.config.clone()
    }

    /// Reports progress without publishing results or claiming that sampling is business certainty.
    pub fn status(&self) -> Json {
        json!({"job":self.id,"doc":self.doc,"state":if self.ready(){"ready"}else{"running"},"stage":if self.ready(){"ready"}else if self.xs.is_none(){"sampling"}else{"simulating"},"completed":self.completed,"total":self.iterations,"progress":self.completed as f64/self.iterations as f64,"workUsed":self.trial.meter.spent().ok(),"maxWork":self.work_limit,"autoAdvance":self.auto_advance})
    }

    /// A clone can request cooperative cancellation while an embedding caller owns the session.
    pub fn cancellation(&self) -> crate::workload::Cancellation {
        self.trial.meter.cancellation.clone()
    }

    /// Whether all formula trials completed; statistical finalization/publication is still pending.
    pub fn ready(&self) -> bool {
        self.completed == self.iterations
    }

    /// Advances one bounded batch, checking cancellation and shared work before each trial.
    /// The eight-millisecond target is checked between trials, not inside arbitrary formulas.
    /// The initial bounded sampling phase yields separately before any formula trials.
    pub fn advance(&mut self, max_trials: usize) -> Result<()> {
        self.trial.check()?;
        let _scope = self.trial.meter.hook().enter().ok_or_else(|| bad(CMD, "cannot enter simulation work scope"))?;
        if self.xs.is_none() {
            self.trial.charge(self.sample_work)?;
            self.xs = Some(self.plan.sample(&self.inputs, self.seed)?);
            return self.trial.check();
        }
        let started = gridcraft_core::date::unix_now_ms();
        let initial_work = self.trial.meter.spent()?;
        let reads = self.outputs.iter().map(|o| (o.sheet, o.cell)).collect::<Vec<_>>();
        let xs = self.xs.as_ref().ok_or_else(|| bad(CMD, "missing prepared samples"))?;
        for _ in 0..max_trials.clamp(1, 2048) {
            if self.ready() {
                break;
            }
            let k = self.completed;
            let sets = self
                .inputs
                .iter()
                .zip(xs)
                .map(|(i, values)| (i.sheet, i.cell, Value::Number(values.get(k).copied().unwrap_or(0.0))))
                .collect::<Vec<_>>();
            let runtime = stochastic::Runtime::new(self.seed, k as u64, self.sheet_names.clone());
            let values = {
                let _simulation = runtime.hook().enter().ok_or_else(|| bad(CMD, "cannot enter stochastic sampling scope"))?;
                let values = self.trial.run(&sets, &reads)?;
                runtime.check()?;
                values
            };
            self.stochastic.add(runtime.stats());
            for ((output, column), value) in self.outputs.iter().zip(&mut self.ys).zip(values) {
                match value {
                    Value::Number(n) if n.is_finite() => column.push(n),
                    _ => return Err(bad(CMD, format!("output `{}` is not a finite number at iteration {}", output.label, k + 1))),
                }
            }
            self.completed += 1;
            let elapsed = started.zip(gridcraft_core::date::unix_now_ms()).is_some_and(|(a, b)| b - a >= 8.0);
            if elapsed || self.trial.meter.spent()?.saturating_sub(initial_work) >= 250_000 {
                break;
            }
        }
        self.trial.check()
    }

    /// Finalizes a complete job and publishes one undoable report only after all checks succeed.
    pub fn publish(self, s: &mut Session) -> Result<Json> {
        self.check_source(s)?;
        if !self.ready() {
            return Err(bad(CMD, "analysis is not ready to publish"));
        }
        let Self {
            name,
            config,
            seed,
            iterations,
            keep_samples,
            inputs,
            outputs,
            plan,
            xs,
            ys,
            mut trial,
            finish_work,
            report_cells,
            stochastic,
            ..
        } = self;
        let _scope = trial.meter.hook().enter().ok_or_else(|| bad(CMD, "cannot enter finalization work scope"))?;
        trial.charge(finish_work)?;
        let xs = xs.ok_or_else(|| bad(CMD, "missing samples"))?;
        let mut summaries = Vec::with_capacity(outputs.len());
        for (output, values) in outputs.iter().zip(&ys) {
            trial.check()?;
            let ordered = sorted(values);
            let n = iterations as f64;
            let (mean, sd) = statistics::moments(values)?;
            let summary = Summary {
                label: output.label.clone(),
                mean,
                sd,
                min: ordered.first().copied().unwrap_or(0.0),
                p10: quantile(&ordered, 0.1),
                median: quantile(&ordered, 0.5),
                p90: quantile(&ordered, 0.9),
                max: ordered.last().copied().unwrap_or(0.0),
                above: values.iter().filter(|x| **x > 0.0).count() as f64 / n,
            };
            if !mean.is_finite() || !sd.is_finite() {
                return Err(bad(CMD, "summary statistics overflowed; rescale the model"));
            }
            summaries.push(summary);
        }
        let first = ys.first().ok_or_else(|| bad(CMD, "missing output samples"))?;
        let center = quantile(&sorted(first), 0.5);
        let output_ranks = ranks(first);
        let mut sensitivity = Vec::with_capacity(inputs.len());
        for (input, values) in inputs.iter().zip(&xs) {
            trial.check()?;
            let ordered = sorted(values);
            let (p10, p90) = (quantile(&ordered, 0.1), quantile(&ordered, 0.9));
            let low = values.iter().zip(first).filter_map(|(x, y)| (*x <= p10).then_some(*y)).collect::<Vec<_>>();
            let high = values.iter().zip(first).filter_map(|(x, y)| (*x >= p90).then_some(*y)).collect::<Vec<_>>();
            let (lm, hm) = (quantile(&sorted(&low), 0.5), quantile(&sorted(&high), 0.5));
            let (a, b) = (lm - center, hm - center);
            if !a.is_finite() || !b.is_finite() || !(hm - lm).is_finite() {
                return Err(bad(CMD, "tornado statistics overflowed; rescale the model"));
            }
            sensitivity.push(Sensitivity {
                input: input.label.clone(),
                low: lm,
                high: hm,
                low_n: low.len(),
                high_n: high.len(),
                spearman: correlation(&ranks(values), &output_ranks),
                adverse: a.min(b).min(0.0),
                favorable: a.max(b).max(0.0),
                span: (hm - lm).abs(),
            });
        }
        sensitivity.sort_by(|a, b| b.span.total_cmp(&a.span));
        let diagnostics = Diagnostics::new(&plan, &inputs, &outputs, &xs, &ys)?;
        trial.check()?;
        // Structural publication recalculates the source under the enclosing command policy,
        // after sampling/finalization have finished charging the analysis run and shared bank.
        drop(_scope);
        crate::workload::report_capacity(&s.doc()?.wb, report_cells)?;
        trial.meter.cancellation.begin_commit()?;
        let publication = write_report(
            s,
            Report {
                name: &name,
                config: &config,
                seed,
                iterations,
                keep_samples,
                inputs: &inputs,
                outputs: &outputs,
                xs: &xs,
                ys: &ys,
                summaries: &summaries,
                sensitivity: &sensitivity,
                diagnostics: &diagnostics,
                stochastic,
            },
        );
        trial.meter.cancellation.complete();
        trial.check()?;
        publication?;
        Ok(
            json!({"sheet":name,"iterations":iterations,"seed":seed,"static":true,"independentInputs":plan.independent(),"sampling":plan.method_name(),"stochastic":stochastic.json(),"replicates":plan.replicates,"accuracy":diagnostics.accuracy_json(),"convergence":diagnostics.first_trace_json(),"config":config,"outputs":summaries.iter().map(Summary::json).collect::<Vec<_>>(),"tornadoOutput":outputs.first().map(|o|&o.label),"tornado":sensitivity.iter().map(Sensitivity::json).collect::<Vec<_>>()}),
        )
    }
}

/// Preserves the synchronous command contract while reusing the incremental evaluator exactly.
fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let mut job = Job::prepare(s, p)?;
    while !job.ready() {
        job.advance(2048)?;
    }
    job.publish(s)
}

/// Immutable finalization inputs grouped without copying samples or changing their order.
struct Report<'a> {
    name: &'a str,
    config: &'a Json,
    seed: u64,
    iterations: usize,
    keep_samples: bool,
    inputs: &'a [Input],
    outputs: &'a [Output],
    xs: &'a [Vec<f64>],
    ys: &'a [Vec<f64>],
    summaries: &'a [Summary],
    sensitivity: &'a [Sensitivity],
    diagnostics: &'a Diagnostics,
    stochastic: stochastic::Stats,
}

/// Publishes summary, conditional sensitivity, diagnostics, optional samples and two native
/// charts in one report-sheet edit. The report becomes active and retains canonical rerun
/// parameters; the original model's input values and formulas are preserved.
fn write_report(s: &mut Session, report: Report<'_>) -> Result<()> {
    let Report { name, config, seed, iterations, keep_samples, inputs, outputs, xs, ys, summaries, sensitivity, diagnostics, stochastic } = report;
    let chart_id = s.doc()?.wb.next_object_id();
    let si = edit(s, |cx| {
        let mut sheet = Sheet::new(name.to_string());
        let bold = cx.wb.styles.derive(StyleId::DEFAULT, |st| st.font.bold = true);
        let mut put = |r: u32, c: u32, v: Value, style: StyleId| {
            sheet.set_cell(CellRef::new(r, c), Cell { value: v, formula: None, style });
        };
        put(0, 0, Value::text("Monte Carlo simulation"), bold);
        put(
            1,
            0,
            Value::text(
                "Static snapshot. Percentiles describe outcomes; confidence intervals describe sampling error. Conditional tornado is not a causal effect.",
            ),
            StyleId::DEFAULT,
        );
        put(
            2,
            0,
            Value::text(format!("Iterations: {iterations}; seed: {seed}. Seed controls inputs and MC.* streams v1; not RAND/NOW.")),
            StyleId::DEFAULT,
        );
        put(3, 0, Value::text("Reusable configuration (JSON)"), bold);
        put(3, 1, Value::text(config.to_string()), StyleId::DEFAULT);
        put(4, 0, Value::text("Seeded worksheet sampling"), bold);
        put(4, 1, Value::text(stochastic.json().to_string()), StyleId::DEFAULT);
        for (c, h) in ["Output", "Mean", "Sample standard deviation", "Minimum", "P10", "P50", "P90", "Maximum", "Probability > 0"].iter().enumerate()
        {
            put(5, c as u32, Value::text(*h), bold);
        }
        for (r, a) in summaries.iter().enumerate() {
            let r = r as u32 + 6;
            put(r, 0, Value::text(a.label.as_str()), StyleId::DEFAULT);
            for (c, n) in [a.mean, a.sd, a.min, a.p10, a.median, a.p90, a.max, a.above].into_iter().enumerate() {
                put(r, c as u32 + 1, Value::Number(n), StyleId::DEFAULT);
            }
        }
        let t = outputs.len() as u32 + 9;
        put(t - 1, 0, Value::text("Conditional tornado: first output, same Monte Carlo samples; tied deciles may contain more than 10%."), bold);
        for (c, h) in [
            "Input",
            "Spearman",
            "Low-group median",
            "High-group median",
            "Adverse vs median",
            "Favorable vs median",
            "Span",
            "Low group size",
            "High group size",
        ]
        .iter()
        .enumerate()
        {
            put(t, c as u32, Value::text(*h), bold);
        }
        for (r, a) in sensitivity.iter().enumerate() {
            let r = t + 1 + r as u32;
            put(r, 0, Value::text(a.input.as_str()), StyleId::DEFAULT);
            put(r, 1, a.spearman.map(Value::Number).unwrap_or_default(), StyleId::DEFAULT);
            for (c, n) in [a.low, a.high, a.adverse, a.favorable, a.span, a.low_n as f64, a.high_n as f64].into_iter().enumerate() {
                put(r, c as u32 + 2, Value::Number(n), StyleId::DEFAULT);
            }
        }
        let (diagnostics_end, trace_start, trace_end) = diagnostics.write(&mut put, t + sensitivity.len() as u32 + 3, bold);
        // Ascending helper rows put the most influential input at the top of a native bar chart.
        let helper = 28;
        for (c, h) in ["Input", "Adverse", "Favorable"].iter().enumerate() {
            put(helper, 12 + c as u32, Value::text(*h), bold);
        }
        for (r, a) in sensitivity.iter().rev().enumerate() {
            put(helper + 1 + r as u32, 12, Value::text(a.input.as_str()), StyleId::DEFAULT);
            put(helper + 1 + r as u32, 13, Value::Number(a.adverse), StyleId::DEFAULT);
            put(helper + 1 + r as u32, 14, Value::Number(a.favorable), StyleId::DEFAULT);
        }
        if keep_samples {
            let start = diagnostics_end + 2;
            put(start, 0, Value::text("Iteration"), bold);
            for (c, a) in inputs.iter().map(|i| &i.label).chain(outputs.iter().map(|o| &o.label)).enumerate() {
                put(start, c as u32 + 1, Value::text(a.as_str()), bold);
            }
            for r in 0..iterations {
                put(start + 1 + r as u32, 0, Value::Number((r + 1) as f64), StyleId::DEFAULT);
                for (c, values) in xs.iter().chain(ys).enumerate() {
                    if let Some(v) = values.get(r) {
                        put(start + 1 + r as u32, c as u32 + 1, Value::Number(*v), StyleId::DEFAULT);
                    }
                }
            }
        }
        let q = gridcraft_formula::quote_sheet(name);
        let end = helper + sensitivity.len() as u32 + 1;
        sheet.charts.push(Chart {
            id: chart_id,
            kind: ChartKind::BarStacked,
            anchor: Anchor { cell: CellRef::new(0, 12), width: 640.0, height: 420.0, ..Default::default() },
            title: Some("Monte Carlo conditional tornado".into()),
            series: (13..=14)
                .map(|col| Series {
                    name: Some(format!("{q}!${}${}", if col == 13 { "N" } else { "O" }, helper + 1)),
                    categories: Some(format!("{q}!$M${}:$M${end}", helper + 2)),
                    values: format!("{q}!${0}${1}:${0}${end}", if col == 13 { "N" } else { "O" }, helper + 2),
                    ..Default::default()
                })
                .collect(),
            legend: LegendPos::Bottom,
            data_labels: false,
            gridlines: true,
            style: 1,
            x_title: Some("Change from overall median".into()),
            y_title: None,
            source: None,
            by_rows: false,
            pivot: None,
        });
        let trace_series = [(2, "Mean"), (4, "Mean CI95 lower"), (5, "Mean CI95 upper")]
            .into_iter()
            .map(|(col, label)| {
                let letter = gridcraft_core::col_to_letters(col);
                Series {
                    name: Some(label.into()),
                    categories: Some(format!("{q}!$B${}:$B${}", trace_start + 1, trace_end + 1)),
                    values: format!("{q}!${letter}${}:${letter}${}", trace_start + 1, trace_end + 1),
                    ..Default::default()
                }
            })
            .collect();
        sheet.charts.push(Chart {
            id: chart_id.saturating_add(1),
            kind: ChartKind::LineMarkers,
            anchor: Anchor { cell: CellRef::new(helper + sensitivity.len() as u32 + 5, 12), width: 640.0, height: 360.0, ..Default::default() },
            title: Some("Mean convergence and 95% sampling-error interval".into()),
            series: trace_series,
            legend: LegendPos::Bottom,
            data_labels: false,
            gridlines: true,
            style: 1,
            x_title: Some("Completed samples".into()),
            y_title: None,
            source: None,
            by_rows: false,
            pivot: None,
        });
        for col in 0..15 {
            sheet.cols.insert(col, gridcraft_model::LineInfo { size: Some(if col == 0 || col == 12 { 180.0 } else { 140.0 }), ..Default::default() });
        }
        let si = cx.wb.sheets.len();
        cx.wb.sheets.push(Arc::new(sheet));
        cx.wb.set_active_sheet(si);
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(si)
    })?;
    if let Ok(d) = s.doc_mut() {
        d.sheet_selections.insert(si.min(d.sheet_selections.len()), crate::Selection::default());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
