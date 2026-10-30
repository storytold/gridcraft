//! Seeded conditional counts and stationary latent AR1 paths, scoped to one workbook trial.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use gridcraft_calc::{SimulationHook, SimulationRequest, SimulationSite, WorkHook};
use gridcraft_core::{CellError, Value};

use super::{CMD, Result, Rng, bad, discrete};

const MAX_PROCESSES: usize = 64;
const MAX_STEPS: u32 = 1200;
const MAX_CACHED_VALUES: usize = 65_536;

/// Operational counters count requests, not an assertion of distinct independent draws.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Stats {
    pub count_requests: u64,
    pub process_queries: u64,
    pub process_steps: u64,
}

impl Stats {
    /// Accumulates completed trials without allowing accounting overflow.
    pub fn add(&mut self, other: Self) {
        self.count_requests = self.count_requests.saturating_add(other.count_requests);
        self.process_queries = self.process_queries.saturating_add(other.process_queries);
        self.process_steps = self.process_steps.saturating_add(other.process_steps);
    }

    /// Retains the seeded-worksheet sampling contract and measured request counters in reports.
    pub fn json(self) -> super::Json {
        super::json!({"version":1,"countRequests":self.count_requests,"ar1Queries":self.process_queries,"ar1GeneratedSteps":self.process_steps})
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ar1Spec {
    mean: f64,
    scale: f64,
    phi: f64,
    lognormal: bool,
}

struct Process {
    spec: Ar1Spec,
    rng: Rng,
    latent: Vec<f64>,
}

#[derive(Default)]
struct Processes {
    paths: BTreeMap<String, Process>,
    stored: usize,
}

struct State {
    seed: u64,
    trial: u64,
    sheets: Arc<Vec<String>>,
    failed: AtomicBool,
    reason: Mutex<Option<String>>,
    processes: Mutex<Processes>,
    count_requests: AtomicU64,
    process_queries: AtomicU64,
    process_steps: AtomicU64,
}

/// An engine-owned sampling context. Clones share the failure latch and this trial's AR1 cache.
#[derive(Clone)]
pub(super) struct Runtime(Arc<State>);

impl Runtime {
    /// Starts a new trial using the absolute row index, shared by synchronous and stepped runs.
    pub fn new(seed: u64, trial: u64, sheets: Arc<Vec<String>>) -> Self {
        Self(Arc::new(State {
            seed,
            trial,
            sheets,
            failed: AtomicBool::new(false),
            reason: Mutex::new(None),
            processes: Mutex::new(Processes::default()),
            count_requests: AtomicU64::new(0),
            process_queries: AtomicU64::new(0),
            process_steps: AtomicU64::new(0),
        }))
    }

    /// Supplies only a neutral callback to calc; the guard must never span report publication.
    pub fn hook(&self) -> SimulationHook {
        let runtime = self.clone();
        SimulationHook::new(move |site, request| runtime.dispatch(site, request))
    }

    /// Refuses a trial even when worksheet IFERROR handled a sampler's numeric error.
    pub fn check(&self) -> Result<()> {
        if !self.0.failed.load(Ordering::Acquire) {
            return Ok(());
        }
        let reason = self.0.reason.try_lock().ok().and_then(|r| r.clone()).unwrap_or_else(|| "invalid stochastic worksheet request".into());
        Err(bad(CMD, reason))
    }

    /// Reads counters only after a completed trial, without changing sampling or resource credit.
    pub fn stats(&self) -> Stats {
        Stats {
            count_requests: self.0.count_requests.load(Ordering::Relaxed),
            process_queries: self.0.process_queries.load(Ordering::Relaxed),
            process_steps: self.0.process_steps.load(Ordering::Relaxed),
        }
    }

    /// Stores a persistent, bounded error without exposing parameters or workbook contents.
    fn fail(&self, reason: String) -> Value {
        self.0.failed.store(true, Ordering::Release);
        if let Ok(mut current) = self.0.reason.try_lock()
            && current.is_none()
        {
            *current = Some(reason);
        }
        Value::Error(CellError::Num)
    }

    /// Dispatches a scalar request; ordinary RAND retains its separate historical semantics.
    fn dispatch(&self, site: SimulationSite, request: SimulationRequest<'_>) -> Value {
        if self.0.failed.load(Ordering::Acquire) || !WorkHook::allowed() {
            return self.fail("stochastic sampling stopped by a prior failure or cancellation".into());
        }
        if matches!(request, SimulationRequest::Active) {
            return Value::Bool(true);
        }
        if matches!(request, SimulationRequest::Invalid) {
            return self.fail("invalid MC.* worksheet parameters or arity".into());
        }
        let result = match request {
            SimulationRequest::Poisson { mean, stream } => {
                self.0.count_requests.fetch_add(1, Ordering::Relaxed);
                self.uniform(site, stream, "poisson").and_then(|u| discrete::poisson_quantile(mean, u))
            }
            SimulationRequest::Binomial { trials, probability, stream } => {
                self.0.count_requests.fetch_add(1, Ordering::Relaxed);
                self.uniform(site, stream, "binomial").and_then(|u| discrete::binomial_quantile(trials, probability, u))
            }
            SimulationRequest::Hypergeometric { draws, successes, population, stream } => {
                self.0.count_requests.fetch_add(1, Ordering::Relaxed);
                self.uniform(site, stream, "hypergeometric").and_then(|u| discrete::hypergeometric_quantile(draws, successes, population, u))
            }
            SimulationRequest::Ar1Normal { process, step, mean, std_dev, phi } => {
                self.process(process, step, Ar1Spec { mean, scale: std_dev, phi, lognormal: false })
            }
            SimulationRequest::Ar1Lognormal { process, step, mean, cv, phi } => {
                self.process(process, step, Ar1Spec { mean, scale: cv, phi, lognormal: true })
            }
            SimulationRequest::Active | SimulationRequest::Invalid => unreachable_request(),
        };
        match result {
            Ok(value) if value.is_finite() => Value::Number(value),
            Ok(_) => self.fail("stochastic sampling produced a non-finite value".into()),
            Err(error) => self.fail(error.to_string()),
        }
    }

    /// Formula identity plus explicit stream/family gives evaluation-order-independent draws.
    fn uniform(&self, site: SimulationSite, stream: &str, family: &str) -> Result<f64> {
        if stream.is_empty() || stream.len() > 128 {
            return Err(bad(CMD, "sampling stream IDs must contain 1–128 bytes"));
        }
        let sheet = self.0.sheets.get(site.sheet).ok_or_else(|| bad(CMD, "sampling sheet identity is unavailable"))?;
        charge(16 + sheet.len() as u64 + stream.len() as u64)?;
        let cell = u64::from(site.cell.row) << 32 | u64::from(site.cell.col);
        let mut rng = Rng(keyed_seed(self.0.seed, self.0.trial, &["cell", sheet, family, stream], cell));
        Ok(rng.uniform())
    }

    /// A named process is shared by cells; requesting month24 first generates the same prefix.
    fn process(&self, name: &str, step: u32, spec: Ar1Spec) -> Result<f64> {
        validate_process(name, step, spec)?;
        charge(16 + name.len() as u64)?;
        self.0.process_queries.fetch_add(1, Ordering::Relaxed);
        let mut processes = self.0.processes.try_lock().map_err(|_| bad(CMD, "AR1 context is busy or unavailable"))?;
        let current = processes.paths.get(name).map_or(0, |p| p.latent.len());
        let extra = (step as usize).saturating_sub(current);
        if processes.stored.saturating_add(extra) > MAX_CACHED_VALUES {
            return Err(bad(CMD, "too many cached AR1 steps in one trial"));
        }
        if !processes.paths.contains_key(name) {
            if processes.paths.len() >= MAX_PROCESSES {
                return Err(bad(CMD, "too many named AR1 processes in one trial"));
            }
            let seed = keyed_seed(self.0.seed, self.0.trial, &["ar1", name], 0);
            processes.paths.insert(name.to_string(), Process { spec, rng: Rng(seed), latent: Vec::new() });
        }
        let path = processes.paths.get_mut(name).ok_or_else(|| bad(CMD, "missing AR1 context"))?;
        if path.spec != spec {
            return Err(bad(CMD, "one AR1 process ID cannot change its parameters within a trial"));
        }
        charge((extra as u64).saturating_mul(16))?;
        path.latent.reserve(extra);
        let innovation = ((1.0 - spec.phi) * (1.0 + spec.phi)).sqrt();
        while path.latent.len() < step as usize {
            if path.latent.len().is_multiple_of(128) && !WorkHook::allowed() {
                return Err(bad(CMD, "AR1 generation stopped by work limit or cancellation"));
            }
            let fresh = path.rng.normal();
            let z = path.latent.last().map_or(fresh, |previous| spec.phi * previous + innovation * fresh);
            path.latent.push(z);
        }
        let z = path.latent.get(step as usize - 1).copied().ok_or_else(|| bad(CMD, "missing AR1 step"))?;
        let value = if spec.lognormal { lognormal(spec.mean, spec.scale, z)? } else { spec.mean + spec.scale * z };
        processes.stored = processes.stored.saturating_add(extra);
        self.0.process_steps.fetch_add(extra as u64, Ordering::Relaxed);
        Ok(value)
    }
}

/// Keeps impossible internal variants fallible rather than using a production panic.
fn unreachable_request() -> Result<f64> {
    Err(bad(CMD, "unexpected stochastic request"))
}

/// Charges before allocation/iteration and observes a previously latched work refusal.
fn charge(units: u64) -> Result<()> {
    if WorkHook::current().is_none_or(|hook| hook.charge(units)) {
        Ok(())
    } else {
        Err(bad(CMD, "stochastic sampling stopped by work limit or cancellation"))
    }
}

/// Length-prefixing prevents component-boundary collisions; SplitMix avalanches the final key.
/// This is deterministic simulation stream separation, not a cryptographic hash or secret RNG.
fn keyed_seed(seed: u64, trial: u64, parts: &[&str], cell: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut absorb = |byte: u8| {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    };
    for byte in seed.to_le_bytes().into_iter().chain(trial.to_le_bytes()).chain(cell.to_le_bytes()) {
        absorb(byte);
    }
    for part in parts {
        for byte in (part.len() as u64).to_le_bytes().into_iter().chain(part.bytes()) {
            absorb(byte);
        }
    }
    Rng(hash).next()
}

/// Converts an arithmetic mean and coefficient of variation to a strictly positive lognormal.
pub(super) fn lognormal(mean: f64, cv: f64, z: f64) -> Result<f64> {
    if !mean.is_finite() || mean <= 0.0 || !cv.is_finite() || cv < 0.0 || !z.is_finite() {
        return Err(bad(CMD, "lognormal requires a positive arithmetic mean and finite nonnegative CV"));
    }
    if cv == 0.0 {
        return Ok(mean);
    }
    // Avoid cv² overflow for large but finite CVs; do not reject representable samples.
    let variance = if cv > 1.0 {
        let reciprocal = cv.recip();
        2.0 * cv.ln() + (reciprocal * reciprocal).ln_1p()
    } else {
        (cv * cv).ln_1p()
    };
    let value = (mean.ln() - variance / 2.0 + variance.sqrt() * z).exp();
    if value.is_finite() && value > 0.0 { Ok(value) } else { Err(bad(CMD, "lognormal sample overflowed or underflowed")) }
}

/// Restricts this first process contract to stationary AR1 paths with bounded state.
fn validate_process(name: &str, step: u32, spec: Ar1Spec) -> Result<()> {
    if name.is_empty() || name.len() > 128 || step == 0 || step > MAX_STEPS {
        return Err(bad(CMD, "AR1 requires an ID of 1–128 bytes and step 1–1200"));
    }
    if !spec.mean.is_finite() || !spec.scale.is_finite() || spec.scale < 0.0 || !spec.phi.is_finite() || spec.phi.abs() >= 1.0 {
        return Err(bad(CMD, "AR1 requires finite parameters, nonnegative scale and abs(phi)<1"));
    }
    if spec.lognormal {
        lognormal(spec.mean, spec.scale, 0.0)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
