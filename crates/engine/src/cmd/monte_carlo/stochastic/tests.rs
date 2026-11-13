//! Runtime contracts use deterministic seeds and statistical tolerances tied to material moments.
use std::sync::Arc;

use gridcraft_calc::{SimulationRequest, SimulationSite, WorkHook};
use gridcraft_core::{CellError, CellRef, Value};

use super::{MAX_CACHED_VALUES, MAX_PROCESSES, MAX_STEPS, Runtime};

fn runtime(seed: u64, trial: u64) -> Runtime {
    Runtime::new(seed, trial, Arc::new(vec!["Assumptions".into(), "Forecast".into()]))
}

fn site(sheet: usize, row: u32, col: u32) -> SimulationSite {
    SimulationSite { sheet, cell: CellRef::new(row, col) }
}

/// Scoped aliases preserve the exact seeded count stream of each physical caller, not the defining sheet.
#[test]
fn scoped_name_counts_match_direct_calls_at_each_physical_site() {
    use gridcraft_model::{Cell, DefinedName, Formula, Sheet, Workbook};

    let mut named = Workbook::new();
    named.sheets.push(Arc::new(Sheet::new("Forecast")));
    named.names.push(DefinedName {
        name: "Draw".into(),
        scope: Some(0),
        formula: "MC.POISSON($B$1,\"orders\")".into(),
        comment: String::new(),
        hidden: false,
    });
    for sheet in 0..2 {
        named.sheet_mut(sheet).unwrap().cells.set(CellRef::new(0, 1), Cell::value(Value::Number(if sheet == 0 { 10.0 } else { 99.0 })));
        named.sheet_mut(sheet).unwrap().cells.set(CellRef::new(0, 0), Cell::formula(Formula::new("=Sheet1!Draw")));
    }
    let mut direct = named.clone();
    direct.names.clear();
    for sheet in 0..2 {
        direct.sheet_mut(sheet).unwrap().cells.set(CellRef::new(0, 0), Cell::formula(Formula::new("=MC.POISSON(10,\"orders\")")));
    }
    let sheets = Arc::new(vec!["Sheet1".into(), "Forecast".into()]);
    for trial in 0..64 {
        for workbook in [&mut named, &mut direct] {
            let runtime = Runtime::new(0x1234_5678, trial, sheets.clone());
            let _scope = runtime.hook().enter().unwrap();
            gridcraft_calc::Calc::new().recalc_all(workbook);
            runtime.check().unwrap();
            assert_eq!(runtime.stats().count_requests, 2);
        }
        for sheet in 0..2 {
            let expected = direct.sheet(sheet).unwrap().value(CellRef::new(0, 0));
            assert!(matches!(expected, Value::Number(_)));
            assert_eq!(named.sheet(sheet).unwrap().value(CellRef::new(0, 0)), expected, "trial {trial}, physical sheet {sheet}");
        }
    }
}

/// Extracting a finite number also makes a silent worksheet error fail a regression.
fn number(runtime: &Runtime, site: SimulationSite, request: SimulationRequest<'_>) -> f64 {
    match runtime.dispatch(site, request) {
        Value::Number(value) if value.is_finite() => value,
        value => panic!("expected finite stochastic value, got {value:?}"),
    }
}

fn normal(runtime: &Runtime, process: &str, step: u32, phi: f64) -> f64 {
    number(runtime, site(0, step, 0), SimulationRequest::Ar1Normal { process, step, mean: 10.0, std_dev: 2.0, phi })
}

#[derive(Default)]
struct Moments {
    sum: f64,
    squares: f64,
}
impl Moments {
    fn add(&mut self, value: f64) {
        self.sum += value;
        self.squares += value * value;
    }
    fn mean(&self, n: usize) -> f64 {
        self.sum / n as f64
    }
    fn variance(&self, n: usize) -> f64 {
        self.squares / n as f64 - self.mean(n).powi(2)
    }
}

/// A named process is keyed independently of formula sites and evaluation order;
/// generating month24 first retains exactly the same 24-value prefix and counters.
#[test]
fn ar1_request_order_and_named_processes_are_independent() {
    let forward = runtime(0x1234_5678, 37);
    let reverse = runtime(0x1234_5678, 37);
    let expected: Vec<f64> = (1..=24).map(|step| normal(&forward, "monthly-demand", step, 0.7)).collect();
    for step in (1..=24).rev() {
        let actual =
            number(&reverse, site(1, 90, step), SimulationRequest::Ar1Normal { process: "monthly-demand", step, mean: 10.0, std_dev: 2.0, phi: 0.7 });
        assert_eq!(actual, expected[(step - 1) as usize]);
    }
    assert_eq!(forward.stats().process_steps, 24);
    assert_eq!(reverse.stats().process_steps, 24);
    assert_eq!(reverse.stats().process_queries, 24);
    let alternate = normal(&reverse, "monthly-pricing", 24, 0.7);
    let independent = normal(&runtime(0x1234_5678, 37), "monthly-pricing", 24, 0.7);
    assert_eq!(alternate, independent);
    assert_ne!(alternate, expected[23]);
    reverse.check().unwrap();
}

/// Across independent trial keys, both the first and later states have N(0,1)
/// marginals. Covariances are phi and phi^11, including negative persistence.
#[test]
fn ar1_stationary_moments_and_lag_covariances() {
    let n = 24_000;
    for phi in [0.7_f64, -0.65_f64] {
        let (mut first, mut second, mut later) = (Moments::default(), Moments::default(), Moments::default());
        let (mut adjacent_product, mut distant_product) = (0.0, 0.0);
        for trial in 0..n {
            let runtime = runtime(0x3141_5926, trial as u64);
            let a = (normal(&runtime, "stationarity", 1, phi) - 10.0) / 2.0;
            let b = (normal(&runtime, "stationarity", 2, phi) - 10.0) / 2.0;
            let c = (normal(&runtime, "stationarity", 12, phi) - 10.0) / 2.0;
            first.add(a);
            second.add(b);
            later.add(c);
            adjacent_product += a * b;
            distant_product += a * c;
        }
        for moments in [&first, &second, &later] {
            assert!(moments.mean(n).abs() < 0.035, "phi={phi}, mean={}", moments.mean(n));
            assert!((moments.variance(n) - 1.0).abs() < 0.05, "phi={phi}, variance={}", moments.variance(n));
        }
        let adjacent = adjacent_product / n as f64 - first.mean(n) * second.mean(n);
        let distant = distant_product / n as f64 - first.mean(n) * later.mean(n);
        assert!((adjacent - phi).abs() < 0.04, "phi={phi}, covariance={adjacent}");
        assert!((distant - phi.powi(11)).abs() < 0.04, "phi={phi}, lag11 covariance={distant}");
    }
}

/// Mean/CV are arithmetic lognormal parameters; phi describes the latent normal
/// process, not the Pearson correlation of the positive transformed observations.
#[test]
fn lognormal_process_preserves_arithmetic_mean_cv_and_latent_covariance() {
    let (mean, cv, phi) = (120.0_f64, 0.35_f64, 0.6_f64);
    let variance = (cv * cv).ln_1p();
    let sigma = variance.sqrt();
    let mu = mean.ln() - variance / 2.0;
    let (mut first, mut later) = (Moments::default(), Moments::default());
    let (mut z_first, mut z_next) = (Moments::default(), Moments::default());
    let mut latent_product = 0.0;
    let n = 24_000;
    for trial in 0..n {
        let runtime = runtime(0x2718_2818, trial as u64);
        let draw = |step| number(&runtime, site(0, step, 0), SimulationRequest::Ar1Lognormal { process: "media-cost", step, mean, cv, phi });
        let (a, b, c) = (draw(1), draw(2), draw(12));
        assert!(a > 0.0 && b > 0.0 && c > 0.0);
        first.add(a);
        later.add(c);
        let (x, y) = ((a.ln() - mu) / sigma, (b.ln() - mu) / sigma);
        z_first.add(x);
        z_next.add(y);
        latent_product += x * y;
    }
    for moments in [&first, &later] {
        assert!((moments.mean(n) / mean - 1.0).abs() < 0.01);
        let actual_cv = moments.variance(n).sqrt() / moments.mean(n);
        assert!((actual_cv - cv).abs() < 0.015, "CV={actual_cv}");
    }
    let covariance = latent_product / n as f64 - z_first.mean(n) * z_next.mean(n);
    assert!((covariance - phi).abs() < 0.04, "latent covariance={covariance}");
}

/// Count draws are keyed by seed/trial/sheet/cell/stream/family; repeated reads
/// and reversed formula order cannot advance a shared mutable random stream.
#[test]
fn conditional_count_streams_are_reproducible_and_order_independent() {
    let first = runtime(42, 8);
    let second = runtime(42, 8);
    let cells: Vec<_> = (0..24).map(|row| site(0, row, 3)).collect();
    let expected: Vec<_> =
        cells.iter().map(|cell| number(&first, *cell, SimulationRequest::Binomial { trials: 200, probability: 0.37, stream: "qualified" })).collect();
    for (index, cell) in cells.iter().enumerate().rev() {
        let request = || SimulationRequest::Binomial { trials: 200, probability: 0.37, stream: "qualified" };
        assert_eq!(number(&second, *cell, request()), expected[index]);
        assert_eq!(number(&second, *cell, request()), expected[index]);
    }
    assert_eq!(second.stats().count_requests, 48);
    let original = first.uniform(cells[0], "qualified", "binomial").unwrap();
    assert_ne!(original, first.uniform(cells[0], "booked", "binomial").unwrap());
    assert_ne!(original, first.uniform(site(1, 0, 3), "qualified", "binomial").unwrap());
    assert_ne!(original, first.uniform(cells[0], "qualified", "poisson").unwrap());
    assert_ne!(original, runtime(42, 9).uniform(cells[0], "qualified", "binomial").unwrap());
    first.check().unwrap();
    second.check().unwrap();
}

/// Downstream conditional trials cannot exceed upstream integer counts. A finite
/// capacity allocation without replacement preserves selected and remaining totals.
#[test]
fn conditional_funnel_and_hypergeometric_capacity_conserve_counts() {
    for trial in 0..128 {
        let runtime = runtime(180, trial);
        let leads = number(&runtime, site(0, 1, 0), SimulationRequest::Poisson { mean: 80.0, stream: "arrivals" }) as u64;
        let qualified = number(&runtime, site(0, 2, 0), SimulationRequest::Binomial { trials: leads, probability: 0.65, stream: "qualified" }) as u64;
        let booked = number(&runtime, site(0, 3, 0), SimulationRequest::Binomial { trials: qualified, probability: 0.55, stream: "booked" }) as u64;
        assert!(booked <= qualified && qualified <= leads);
        let population = 41;
        let successes = 19;
        let capacity = 15;
        let selected =
            number(&runtime, site(0, 4, 0), SimulationRequest::Hypergeometric { draws: capacity, successes, population, stream: "human-capacity" });
        assert_eq!(selected, selected.floor());
        let selected = selected as u64;
        let selected_other = capacity - selected;
        let remaining_success = successes - selected;
        let remaining_other = population - successes - selected_other;
        assert_eq!(selected + selected_other, capacity);
        assert_eq!(remaining_success + remaining_other, population - capacity);
        assert_eq!(selected + remaining_success, successes);
        runtime.check().unwrap();
    }
}

/// A worksheet may replace #NUM through IFERROR, but it cannot clear the trial's
/// failure latch; even a later valid draw must still refuse and check() must fail.
#[test]
fn invalid_sampler_and_iferror_equivalent_recovery_remain_latched() {
    for request in [
        SimulationRequest::Invalid,
        SimulationRequest::Binomial { trials: 12, probability: 1.1, stream: "bad" },
        SimulationRequest::Poisson { mean: -1.0, stream: "bad" },
    ] {
        let runtime = runtime(91, 1);
        let failed = runtime.dispatch(site(0, 0, 0), request);
        assert_eq!(failed, Value::Error(CellError::Num));
        let worksheet_iferror_result = match failed {
            Value::Error(_) => Value::Number(7.0),
            value => value,
        };
        assert_eq!(worksheet_iferror_result, Value::Number(7.0));
        assert!(runtime.check().is_err());
        assert_eq!(runtime.dispatch(site(0, 1, 0), SimulationRequest::Poisson { mean: 2.0, stream: "valid" }), Value::Error(CellError::Num));
        assert!(runtime.clone().check().is_err());
    }
}

/// Reusing a process ID with another distribution or any changed parameter is
/// an invalid model, rather than silently producing inconsistent shared paths.
#[test]
fn process_parameter_mismatches_latch_the_trial() {
    for mismatch in [
        SimulationRequest::Ar1Normal { process: "shared", step: 2, mean: 11.0, std_dev: 2.0, phi: 0.7 },
        SimulationRequest::Ar1Normal { process: "shared", step: 2, mean: 10.0, std_dev: 3.0, phi: 0.7 },
        SimulationRequest::Ar1Normal { process: "shared", step: 2, mean: 10.0, std_dev: 2.0, phi: -0.7 },
        SimulationRequest::Ar1Lognormal { process: "shared", step: 2, mean: 10.0, cv: 2.0, phi: 0.7 },
    ] {
        let runtime = runtime(4, 2);
        normal(&runtime, "shared", 1, 0.7);
        assert_eq!(runtime.dispatch(site(0, 1, 0), mismatch), Value::Error(CellError::Num));
        assert!(runtime.check().is_err());
    }
}

/// Structural bounds and invalid stationary parameters refuse through the same
/// persistent model-error path, including an invalid sheet identity for counts.
#[test]
fn invalid_process_domains_and_count_sites_refuse() {
    for request in [
        SimulationRequest::Ar1Normal { process: "", step: 1, mean: 0.0, std_dev: 1.0, phi: 0.0 },
        SimulationRequest::Ar1Normal { process: "id", step: 0, mean: 0.0, std_dev: 1.0, phi: 0.0 },
        SimulationRequest::Ar1Normal { process: "id", step: MAX_STEPS + 1, mean: 0.0, std_dev: 1.0, phi: 0.0 },
        SimulationRequest::Ar1Normal { process: "id", step: 1, mean: 0.0, std_dev: -1.0, phi: 0.0 },
        SimulationRequest::Ar1Normal { process: "id", step: 1, mean: 0.0, std_dev: 1.0, phi: 1.0 },
        SimulationRequest::Ar1Normal { process: "id", step: 1, mean: 0.0, std_dev: 1.0, phi: f64::NAN },
        SimulationRequest::Ar1Lognormal { process: "id", step: 1, mean: 0.0, cv: 0.2, phi: 0.0 },
        SimulationRequest::Poisson { mean: 2.0, stream: "" },
    ] {
        let runtime = runtime(4, 0);
        assert_eq!(runtime.dispatch(site(0, 0, 0), request), Value::Error(CellError::Num));
        assert!(runtime.check().is_err());
    }
    let runtime = runtime(4, 0);
    assert_eq!(runtime.dispatch(site(99, 0, 0), SimulationRequest::Poisson { mean: 2.0, stream: "leads" }), Value::Error(CellError::Num));
    assert!(runtime.check().is_err());
}

/// Refusal happens before process step allocation/generation, and survives later zero
/// checks even when the caller handles the immediate numeric error.
#[test]
fn process_work_refusal_is_atomic_and_latched() {
    let runtime = runtime(4, 0);
    let hook = WorkHook::new(|units| units < 100);
    let _scope = hook.enter().unwrap();
    assert_eq!(
        runtime.dispatch(site(0, 0, 0), SimulationRequest::Ar1Normal { process: "cost", step: 1200, mean: 0.0, std_dev: 1.0, phi: 0.7 }),
        Value::Error(CellError::Num)
    );
    assert!(!WorkHook::allowed());
    assert!(runtime.check().is_err());
    assert_eq!(runtime.stats().process_steps, 0);
    assert_eq!(runtime.0.processes.lock().unwrap().stored, 0);
}

/// Cache limits are per trial and cannot be bypassed by generating independent
/// named paths; rejected requests never increase stored/generated step counts.
#[test]
fn process_count_and_cached_step_bounds_remain_finite() {
    let runtime = runtime(4, 0);
    for index in 0..MAX_PROCESSES {
        normal(&runtime, &format!("p{index}"), 1, 0.0);
    }
    assert_eq!(
        runtime.dispatch(site(0, 0, 0), SimulationRequest::Ar1Normal { process: "one-too-many", step: 1, mean: 10.0, std_dev: 2.0, phi: 0.0 }),
        Value::Error(CellError::Num)
    );
    assert_eq!(runtime.stats().process_steps, MAX_PROCESSES as u64);
    let runtime = self::runtime(4, 0);
    let complete_paths = MAX_CACHED_VALUES / MAX_STEPS as usize;
    for index in 0..complete_paths {
        normal(&runtime, &format!("p{index}"), MAX_STEPS, 0.0);
    }
    let before = runtime.stats().process_steps;
    assert_eq!(
        runtime.dispatch(site(0, 0, 0), SimulationRequest::Ar1Normal { process: "overflow", step: MAX_STEPS, mean: 10.0, std_dev: 2.0, phi: 0.0 }),
        Value::Error(CellError::Num)
    );
    assert_eq!(runtime.stats().process_steps, before);
    assert_eq!(runtime.0.processes.lock().unwrap().stored as u64, before);
}

/// Large finite CVs can have representable samples despite cv² overflowing;
/// actual overflow/underflow still refuses publication rather than returning zero/infinity.
#[test]
fn lognormal_large_finite_cv_uses_a_stable_log_variance() {
    for cv in [1e155_f64, 1e200, 1e300] {
        let median = super::lognormal(1.0, cv, 0.0).unwrap();
        assert!((median * cv - 1.0).abs() < 2e-12, "cv={cv}, median={median}");
        let sample = super::lognormal(1.0, cv, 1.0).unwrap();
        let expected_log = -cv.ln() + (2.0 * cv.ln()).sqrt();
        assert!((sample.ln() - expected_log).abs() < 2e-12);
        super::super::Distribution::parse(&super::super::json!({"distribution":"lognormal","mean":1,"cv":cv})).unwrap();
    }
    assert_eq!(super::lognormal(7.0, 0.0, 42.0).unwrap(), 7.0);
    assert!(super::lognormal(f64::MIN_POSITIVE, 1e200, -10.0).is_err());
    assert!(super::lognormal(f64::MAX, 1.0, 100.0).is_err());
}
