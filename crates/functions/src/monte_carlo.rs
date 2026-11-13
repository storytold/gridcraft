//! Scalar requests for seeded, engine-owned Monte Carlo services; no sampler or RNG lives here.

use gridcraft_core::{CellError, Value};

use crate::util::{A, R, arg, num};
use crate::{Arg, Ctx, FnSpec};

const MAX_COUNT: u64 = 1_000_000_000;
const MAX_SUPPORT: u64 = 100_000;
const MAX_POISSON_MEAN: f64 = 50_000.0;
const MAX_AR1_STEP: u64 = 1200;
const MAX_ID_BYTES: usize = 128;

type RequestParser = for<'a> fn(&'a [Arg]) -> R<SimulationRequest<'a>>;

/// A host-neutral request from a scalar worksheet function to the active simulation trial.
/// Counts are validated whole numbers. Explicit stream/process identifiers are borrowed only
/// for this call; hosts must not retain them without taking their own bounded copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimulationRequest<'a> {
    /// Whether a seeded Monte Carlo trial is active; an absent host means `false`.
    Active,
    /// Latch an invalid worksheet request or invalid host reply through the active trial.
    Invalid,
    /// An integral Poisson draw, with a finite mean in `[0, 50_000]`.
    Poisson { mean: f64, stream: &'a str },
    /// An integral binomial draw. `trials <= 1e9`; nondegenerate support has at most 100,000 outcomes.
    Binomial { trials: u64, probability: f64, stream: &'a str },
    /// An integral draw without replacement, with feasible parameters and at most 100,000 outcomes.
    Hypergeometric { draws: u64, successes: u64, population: u64, stream: &'a str },
    /// A normal AR1 value at step `1..=1200`, using the stationary marginal standard deviation.
    /// `phi` is finite and strictly between -1 and 1; the host owns stationary initialization.
    Ar1Normal { process: &'a str, step: u32, mean: f64, std_dev: f64, phi: f64 },
    /// A positive lognormal value driven by a stationary latent-normal AR1 process.
    /// `mean` and `cv` describe the arithmetic marginal, not the log-space parameters.
    Ar1Lognormal { process: &'a str, step: u32, mean: f64, cv: f64, phi: f64 },
}

/// Registers scalar-only volatile functions. Arrays are explicitly rejected, rather than
/// implicitly intersected or lifted without a stable random-stream identity per output.
pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!(volatile; "MC.ACTIVE", 0, 0, Statistical, A, "MC.ACTIVE()", "TRUE inside a seeded native Monte Carlo trial; FALSE otherwise.", active),
        f!(volatile; "MC.POISSON", 2, 2, Statistical, A, "MC.POISSON(mean, stream)", "Seeded scalar Poisson count (mean <= 50000); #N/A outside Monte Carlo. Use a nonblank text stream ID.", poisson),
        f!(volatile; "MC.BINOMIAL", 3, 3, Statistical, A, "MC.BINOMIAL(trials, probability, stream)", "Seeded scalar binomial count; whole trials <= 1e9, support <= 100000 outcomes; #N/A outside Monte Carlo.", binomial),
        f!(volatile; "MC.HYPERGEOMETRIC", 4, 4, Statistical, A, "MC.HYPERGEOMETRIC(draws, successes, population, stream)", "Seeded scalar count without replacement; whole feasible counts <= 1e9, support <= 100000; #N/A outside Monte Carlo.", hypergeometric),
        f!(volatile; "MC.AR1.NORMAL", 5, 5, Statistical, A, "MC.AR1.NORMAL(process, step, mean, stationary_std_dev, phi)", "Seeded normal AR1 path, step 1..1200 and abs(phi) < 1; stationary marginal standard deviation; #N/A outside Monte Carlo.", ar1_normal),
        f!(volatile; "MC.AR1.LOGNORMAL", 5, 5, Statistical, A, "MC.AR1.LOGNORMAL(process, step, mean, cv, phi)", "Seeded lognormal path with latent-normal AR1 dependence; positive arithmetic mean, nonnegative CV; #N/A outside Monte Carlo.", ar1_lognormal),
    ]
}

/// Rejects non-finite values after the same numeric coercion as other worksheet functions.
fn finite(a: &[Arg], i: usize) -> R<f64> {
    let value = num(a, i)?;
    if value.is_finite() { Ok(value) } else { Err(CellError::Num) }
}

/// Count arguments are never truncated or rounded: fractional populations are invalid.
fn count(a: &[Arg], i: usize) -> R<u64> {
    let value = finite(a, i)?;
    if value < 0.0 || value > MAX_COUNT as f64 || value.fract() != 0.0 {
        return Err(CellError::Num);
    }
    Ok(value as u64)
}

/// A stream ID is explicit scalar text, bounded before any host can copy it.
fn id(a: &[Arg], i: usize) -> R<&str> {
    match &arg(a, i)?.value {
        Value::Text(value) if !value.trim().is_empty() && value.len() <= MAX_ID_BYTES => Ok(value),
        Value::Error(error) => Err(*error),
        _ => Err(CellError::Value),
    }
}

/// Validates a finite probability, retaining exact zero/one degenerate distributions.
fn probability(a: &[Arg], i: usize) -> R<f64> {
    let value = finite(a, i)?;
    if (0.0..=1.0).contains(&value) { Ok(value) } else { Err(CellError::Num) }
}

/// Reports a worksheet failure to the host before propagating its original error value.
fn invalid(ctx: &mut dyn Ctx, error: CellError) -> R<Value> {
    let _ = ctx.simulation(SimulationRequest::Invalid);
    Err(error)
}

/// Counts returned by the host must remain feasible integers; AR1 responses must be finite.
fn valid_number(request: SimulationRequest<'_>, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match request {
        SimulationRequest::Poisson { .. } => value >= 0.0 && value <= MAX_COUNT as f64 && value.fract() == 0.0,
        SimulationRequest::Binomial { trials, .. } => value >= 0.0 && value <= trials as f64 && value.fract() == 0.0,
        SimulationRequest::Hypergeometric { draws, successes, population, .. } => {
            let low = draws.saturating_sub(population - successes);
            value >= low as f64 && value <= draws.min(successes) as f64 && value.fract() == 0.0
        }
        SimulationRequest::Ar1Normal { .. } => true,
        SimulationRequest::Ar1Lognormal { .. } => value > 0.0,
        SimulationRequest::Active | SimulationRequest::Invalid => false,
    }
}

/// Dispatches only validated scalar requests. An absent simulation host is an explicit #N/A
/// for draws, never a hidden expectation or unseeded RNG; MC.ACTIVE remains usable as a guard.
fn dispatch(a: &[Arg], ctx: &mut dyn Ctx, parse: RequestParser) -> R<Value> {
    if a.iter().any(|arg| matches!(arg.value, Value::Array(_))) {
        return invalid(ctx, CellError::Value);
    }
    let request = match parse(a) {
        Ok(request) => request,
        Err(error) => return invalid(ctx, error),
    };
    let Some(value) = ctx.simulation(request) else {
        return if request == SimulationRequest::Active { Ok(Value::Bool(false)) } else { Err(CellError::NA) };
    };
    match value {
        Value::Bool(value) if request == SimulationRequest::Active => Ok(Value::Bool(value)),
        Value::Number(value) if valid_number(request, value) => Ok(Value::Number(value)),
        Value::Error(error) => invalid(ctx, error),
        Value::Number(_) => invalid(ctx, CellError::Num),
        _ => invalid(ctx, CellError::Value),
    }
}

/// Constructs the status query without consuming a stochastic stream.
fn active_request(_a: &[Arg]) -> R<SimulationRequest<'_>> {
    Ok(SimulationRequest::Active)
}

/// Validates the supported Poisson domain before delegating numerical sampling to the host.
fn poisson_request(a: &[Arg]) -> R<SimulationRequest<'_>> {
    let mean = finite(a, 0)?;
    if !(0.0..=MAX_POISSON_MEAN).contains(&mean) {
        return Err(CellError::Num);
    }
    Ok(SimulationRequest::Poisson { mean, stream: id(a, 1)? })
}

/// Validates finite binomial support, allowing arbitrarily large supported degenerate counts.
fn binomial_request(a: &[Arg]) -> R<SimulationRequest<'_>> {
    let trials = count(a, 0)?;
    let probability = probability(a, 1)?;
    if probability > 0.0 && probability < 1.0 && trials.saturating_add(1) > MAX_SUPPORT {
        return Err(CellError::Num);
    }
    Ok(SimulationRequest::Binomial { trials, probability, stream: id(a, 2)? })
}

/// Computes feasible hypergeometric support without unsigned subtraction or allocation hazards.
fn hypergeometric_request(a: &[Arg]) -> R<SimulationRequest<'_>> {
    let draws = count(a, 0)?;
    let successes = count(a, 1)?;
    let population = count(a, 2)?;
    if draws > population || successes > population {
        return Err(CellError::Num);
    }
    let low = draws.saturating_sub(population - successes);
    let high = draws.min(successes);
    if high.saturating_sub(low).saturating_add(1) > MAX_SUPPORT {
        return Err(CellError::Num);
    }
    Ok(SimulationRequest::Hypergeometric { draws, successes, population, stream: id(a, 3)? })
}

/// AR1 initialization is stationary; unit-root and explosive parameters are deliberately refused.
struct Ar1Parameters<'a> {
    process: &'a str,
    step: u32,
    mean: f64,
    spread: f64,
    phi: f64,
}

/// Reads common, bounded scalar AR1 arguments without constructing any process state.
fn ar1_parameters(a: &[Arg]) -> R<Ar1Parameters<'_>> {
    let process = id(a, 0)?;
    let step = count(a, 1)?;
    let mean = finite(a, 2)?;
    let spread = finite(a, 3)?;
    let phi = finite(a, 4)?;
    if step == 0 || step > MAX_AR1_STEP || spread < 0.0 || phi.abs() >= 1.0 {
        return Err(CellError::Num);
    }
    Ok(Ar1Parameters { process, step: step as u32, mean, spread, phi })
}

/// Normal spread is the stationary marginal standard deviation, not innovation volatility.
fn ar1_normal_request(a: &[Arg]) -> R<SimulationRequest<'_>> {
    let p = ar1_parameters(a)?;
    Ok(SimulationRequest::Ar1Normal { process: p.process, step: p.step, mean: p.mean, std_dev: p.spread, phi: p.phi })
}

/// Finite nonnegative CV admits a finite log variance using a stable host-side log transform;
/// squaring a large CV here would reject otherwise valid finite input through overflow.
fn ar1_lognormal_request(a: &[Arg]) -> R<SimulationRequest<'_>> {
    let p = ar1_parameters(a)?;
    if p.mean <= 0.0 {
        return Err(CellError::Num);
    }
    Ok(SimulationRequest::Ar1Lognormal { process: p.process, step: p.step, mean: p.mean, cv: p.spread, phi: p.phi })
}

fn active(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, active_request)
}
fn poisson(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, poisson_request)
}
fn binomial(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, binomial_request)
}
fn hypergeometric(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, hypergeometric_request)
}
fn ar1_normal(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, ar1_normal_request)
}
fn ar1_lognormal(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    dispatch(a, ctx, ar1_lognormal_request)
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, DateSystem, Value};

    use super::SimulationRequest;
    use crate::util::testutil::{av, col, e, ev, n, t};
    use crate::{Arg, Ctx};

    /// Owned protocol observations prove argument identity/coercion without creating any RNG.
    #[derive(Debug, PartialEq)]
    struct Observed {
        kind: &'static str,
        counts: Vec<u64>,
        numbers: Vec<f64>,
        key: String,
    }

    struct Host {
        invalid: usize,
        seen: Vec<Observed>,
        response: Value,
    }
    impl Default for Host {
        fn default() -> Self {
            Self { invalid: 0, seen: Vec::new(), response: Value::Number(2.0) }
        }
    }
    impl Ctx for Host {
        fn date_system(&self) -> DateSystem {
            DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            panic!("MC wrappers must not use the host's unseeded random generator")
        }
        /// Captures exact typed requests; numerical samplers remain an engine responsibility.
        fn simulation(&mut self, request: SimulationRequest<'_>) -> Option<Value> {
            let observed = match request {
                SimulationRequest::Active => return Some(Value::Bool(true)),
                SimulationRequest::Invalid => {
                    self.invalid += 1;
                    return Some(Value::Error(CellError::Num));
                }
                SimulationRequest::Poisson { mean, stream } => Observed { kind: "poisson", counts: vec![], numbers: vec![mean], key: stream.into() },
                SimulationRequest::Binomial { trials, probability, stream } => {
                    Observed { kind: "binomial", counts: vec![trials], numbers: vec![probability], key: stream.into() }
                }
                SimulationRequest::Hypergeometric { draws, successes, population, stream } => {
                    Observed { kind: "hypergeometric", counts: vec![draws, successes, population], numbers: vec![], key: stream.into() }
                }
                SimulationRequest::Ar1Normal { process, step, mean, std_dev, phi } => {
                    Observed { kind: "normal", counts: vec![u64::from(step)], numbers: vec![mean, std_dev, phi], key: process.into() }
                }
                SimulationRequest::Ar1Lognormal { process, step, mean, cv, phi } => {
                    Observed { kind: "lognormal", counts: vec![u64::from(step)], numbers: vec![mean, cv, phi], key: process.into() }
                }
            };
            self.seen.push(observed);
            Some(self.response.clone())
        }
    }

    /// Uses the public dispatch path, including its arity gate and lifting policy.
    fn run(name: &str, args: Vec<Arg>, host: &mut Host) -> Value {
        let spec = crate::lookup(name).unwrap();
        crate::call(spec, &args, host)
    }

    /// The default Ctx extension is inert and cannot silently turn forecasts into random values.
    #[test]
    fn outside_a_trial_active_is_false_and_all_draws_are_na() {
        assert_eq!(ev("MC.ACTIVE", vec![]), Value::Bool(false));
        for (name, args) in valid_cases() {
            assert_eq!(ev(name, args), Value::Error(CellError::NA), "{name}");
        }
    }

    /// Defines valid scalar calls reused for array rejection and host-response checks.
    fn valid_cases() -> Vec<(&'static str, Vec<Arg>)> {
        vec![
            ("MC.POISSON", vec![n(3.0), t("s")]),
            ("MC.BINOMIAL", vec![n(10.0), n(0.3), t("s")]),
            ("MC.HYPERGEOMETRIC", vec![n(5.0), n(3.0), n(10.0), t("s")]),
            ("MC.AR1.NORMAL", vec![t("p"), n(12.0), n(-1.0), n(2.0), n(-0.6)]),
            ("MC.AR1.LOGNORMAL", vec![t("p"), n(24.0), n(1.0), n(0.1), n(0.6)]),
        ]
    }

    /// Native services receive the original numeric contract after ordinary numeric-text coercion.
    #[test]
    fn dispatch_preserves_typed_parameters_and_explicit_streams() {
        let mut host = Host::default();
        assert_eq!(run("MC.ACTIVE", vec![], &mut host), Value::Bool(true));
        assert_eq!(run("MC.POISSON", vec![t("3"), t("poisson-key")], &mut host), Value::Number(2.0));
        assert_eq!(run("MC.BINOMIAL", vec![t("10"), t("0.3"), t("binomial-key")], &mut host), Value::Number(2.0));
        assert_eq!(run("MC.HYPERGEOMETRIC", vec![n(5.0), n(3.0), n(10.0), t("hyper-key")], &mut host), Value::Number(2.0));
        assert_eq!(run("MC.AR1.NORMAL", vec![t("normal-key"), t("12"), n(-1.0), n(2.0), n(-0.6)], &mut host), Value::Number(2.0));
        assert_eq!(run("MC.AR1.LOGNORMAL", vec![t("log-key"), n(24.0), n(1.0), n(0.1), n(0.6)], &mut host), Value::Number(2.0));
        assert_eq!(host.invalid, 0);
        assert_eq!(
            host.seen,
            vec![
                Observed { kind: "poisson", counts: vec![], numbers: vec![3.0], key: "poisson-key".into() },
                Observed { kind: "binomial", counts: vec![10], numbers: vec![0.3], key: "binomial-key".into() },
                Observed { kind: "hypergeometric", counts: vec![5, 3, 10], numbers: vec![], key: "hyper-key".into() },
                Observed { kind: "normal", counts: vec![12], numbers: vec![-1.0, 2.0, -0.6], key: "normal-key".into() },
                Observed { kind: "lognormal", counts: vec![24], numbers: vec![1.0, 0.1, 0.6], key: "log-key".into() },
            ]
        );
    }

    /// No argument may be auto-lifted or silently intersected into a repeated random stream.
    #[test]
    fn every_array_position_is_rejected_and_latched() {
        for (name, args) in valid_cases() {
            let spec = crate::lookup(name).unwrap();
            assert!(spec.volatile);
            assert!((0..args.len()).all(|i| !spec.is_scalar_arg(i)));
            for index in 0..args.len() {
                let mut input = args.clone();
                input[index] = av(col(&[1.0]));
                let mut host = Host::default();
                assert_eq!(run(name, input, &mut host), Value::Error(CellError::Value), "{name} arg {index}");
                assert_eq!(host.invalid, 1);
                assert!(host.seen.is_empty());
            }
        }
    }

    /// Bad support/probability/counts fail before a sampler or allocation can be reached.
    #[test]
    fn malformed_count_parameters_latch_before_host_sampling() {
        let cases = [
            ("MC.POISSON", vec![n(-1.0), t("s")]),
            ("MC.POISSON", vec![n(50_001.0), t("s")]),
            ("MC.POISSON", vec![n(f64::NAN), t("s")]),
            ("MC.BINOMIAL", vec![n(2.5), n(0.5), t("s")]),
            ("MC.BINOMIAL", vec![n(1e9 + 1.0), n(0.0), t("s")]),
            ("MC.BINOMIAL", vec![n(100_000.0), n(0.5), t("s")]),
            ("MC.BINOMIAL", vec![n(5.0), n(1.1), t("s")]),
            ("MC.HYPERGEOMETRIC", vec![n(11.0), n(3.0), n(10.0), t("s")]),
            ("MC.HYPERGEOMETRIC", vec![n(1.0), n(11.0), n(10.0), t("s")]),
            ("MC.HYPERGEOMETRIC", vec![n(200_000.0), n(200_000.0), n(400_000.0), t("s")]),
        ];
        for (name, args) in cases {
            let mut host = Host::default();
            assert_eq!(run(name, args, &mut host), Value::Error(CellError::Num), "{name}");
            assert_eq!(host.invalid, 1);
            assert!(host.seen.is_empty());
        }
    }

    /// Explicit stream IDs cannot become implicit, blank, oversized, numeric or array identities.
    #[test]
    fn invalid_ids_and_wrong_arity_notify_the_trial() {
        for key in [t(""), t("  "), t(&"a".repeat(129)), n(1.0)] {
            let mut host = Host::default();
            assert_eq!(run("MC.POISSON", vec![n(3.0), key], &mut host), Value::Error(CellError::Value));
            assert_eq!(host.invalid, 1);
            assert!(host.seen.is_empty());
        }
        let mut host = Host::default();
        assert_eq!(run("MC.POISSON", vec![n(3.0)], &mut host), Value::Error(CellError::Value));
        assert_eq!(run("MC.ACTIVE", vec![n(1.0)], &mut host), Value::Error(CellError::Value));
        assert_eq!(host.invalid, 2);
    }

    /// Stationary AR1 domains and integer time indices are enforced before process creation.
    #[test]
    fn invalid_ar1_domains_are_latched() {
        let cases = [
            ("MC.AR1.NORMAL", vec![t("p"), n(0.0), n(0.0), n(1.0), n(0.5)]),
            ("MC.AR1.NORMAL", vec![t("p"), n(1.5), n(0.0), n(1.0), n(0.5)]),
            ("MC.AR1.NORMAL", vec![t("p"), n(1201.0), n(0.0), n(1.0), n(0.5)]),
            ("MC.AR1.NORMAL", vec![t("p"), n(1.0), n(0.0), n(-1.0), n(0.5)]),
            ("MC.AR1.NORMAL", vec![t("p"), n(1.0), n(0.0), n(1.0), n(1.0)]),
            ("MC.AR1.NORMAL", vec![t("p"), n(1.0), n(0.0), n(1.0), n(-1.0)]),
            ("MC.AR1.LOGNORMAL", vec![t("p"), n(1.0), n(0.0), n(0.1), n(0.5)]),
            ("MC.AR1.LOGNORMAL", vec![t("p"), n(1.0), n(1.0), n(-0.1), n(0.5)]),
            ("MC.AR1.LOGNORMAL", vec![t("p"), n(1.0), n(1.0), n(0.1), n(f64::INFINITY)]),
        ];
        for (name, args) in cases {
            let mut host = Host::default();
            assert_eq!(run(name, args, &mut host), Value::Error(CellError::Num), "{name}");
            assert_eq!(host.invalid, 1);
            assert!(host.seen.is_empty());
        }
    }

    /// Exact zero/one distributions retain their large-count support without an unnecessary table.
    #[test]
    fn degenerate_distributions_are_valid_requests() {
        let mut host = Host { response: Value::Number(0.0), ..Host::default() };
        assert_eq!(run("MC.POISSON", vec![n(0.0), t("s")], &mut host), Value::Number(0.0));
        assert_eq!(run("MC.BINOMIAL", vec![n(1e9), n(0.0), t("s")], &mut host), Value::Number(0.0));
        assert_eq!(run("MC.HYPERGEOMETRIC", vec![n(0.0), n(0.0), n(0.0), t("s")], &mut host), Value::Number(0.0));
        host.response = Value::Number(1e9);
        assert_eq!(run("MC.BINOMIAL", vec![n(1e9), n(1.0), t("s")], &mut host), Value::Number(1e9));
        assert_eq!(host.invalid, 0);
    }

    /// Invalid numerical host replies and referenced worksheet errors cannot escape the trial latch.
    #[test]
    fn invalid_replies_and_source_errors_are_latched() {
        for reply in [Value::Number(f64::NAN), Value::Number(0.5), Value::Number(-1.0), Value::Error(CellError::Div0)] {
            let mut host = Host { response: reply, ..Host::default() };
            assert!(matches!(run("MC.POISSON", vec![n(3.0), t("s")], &mut host), Value::Error(_)));
            assert_eq!(host.invalid, 1);
        }
        let mut host = Host::default();
        assert_eq!(run("MC.POISSON", vec![e(CellError::Div0), t("s")], &mut host), Value::Error(CellError::Div0));
        assert_eq!(host.invalid, 1);
        assert!(host.seen.is_empty());
        host.response = Value::Number(4.0);
        assert_eq!(run("MC.HYPERGEOMETRIC", vec![n(5.0), n(3.0), n(10.0), t("s")], &mut host), Value::Error(CellError::Num));
        assert_eq!(host.invalid, 2);
    }
}
