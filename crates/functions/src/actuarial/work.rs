//! Conservative operational estimates, not wall-clock forecasts or regulatory capital measures.
//! Every ACT kernel reserves before entry. Adaptive fits and compound-tail extensions also
//! charge at algorithm checkpoints; hosts retain exhaustion outside formula error handling.
use crate::{Arg, Ctx};
use gridcraft_core::{CellError, Value};

/// Refuses before the next work block; the host owns shared accounting and cancellation state.
pub(super) fn charge(ctx: &mut dyn Ctx, units: u64) -> Result<(), CellError> {
    if ctx.charge_work(units) { Ok(()) } else { Err(CellError::Num) }
}

/// Multiplies work dimensions without allowing overflow to turn expensive work into a small bill.
pub(super) fn product(dimensions: &[u64]) -> u64 {
    dimensions.iter().fold(1, |units, dimension| units.saturating_mul(*dimension))
}

/// Reads shape metadata without copying arrays or walking possibly hostile nested values.
fn shape(args: &[Arg], index: usize) -> (u64, u64) {
    match args.get(index).map(|a| &a.value) {
        Some(Value::Array(a)) => (a.rows as u64, a.cols as u64),
        Some(_) => (1, 1),
        None => (0, 0),
    }
}

/// Includes both declared shape and actual storage; malformed public arrays cannot undercharge.
fn cells(args: &[Arg], index: usize) -> u64 {
    let (rows, cols) = shape(args, index);
    let stored = match args.get(index).map(|a| &a.value) {
        Some(Value::Array(a)) => a.data.len() as u64,
        Some(_) => 1,
        None => 0,
    };
    rows.saturating_mul(cols).max(stored)
}

/// Interprets a nonnegative model-size parameter for billing only, without replacing validation.
fn size(args: &[Arg], index: usize, default: u64) -> u64 {
    if !crate::util::has(args, index) {
        return default;
    }
    match crate::util::num(args, index) {
        Ok(x) if x.is_finite() && x >= 0.0 => x.ceil() as u64,
        _ => u64::MAX,
    }
}

/// Counts polynomial features through the supplied degree without building exponent vectors.
/// The kernel supports at most six inputs and degree three; invalid sizes reserve maximally.
fn feature_count(dimension: u64, degree: u64) -> u64 {
    if dimension > 6 || degree > 3 {
        return u64::MAX;
    }
    let mut count = 1u64;
    for i in 1..=degree {
        count = count.saturating_mul(dimension.saturating_add(i)) / i;
    }
    count
}

/// Distinguishes closed copula formulas from quadrature and inverse searches. Cheap analytic
/// families must not pay the elliptical integration reserve on every Monte Carlo trial.
fn copula_work(name: &str, args: &[Arg]) -> u64 {
    let index = if name == "ACT.COPULA.TAIL" { 0 } else { 2 };
    let family = match args.get(index).map(|a| &a.value) {
        Some(Value::Text(value)) => value.trim(),
        _ => return 1_000_000,
    };
    let student = family.eq_ignore_ascii_case("student") || family.eq_ignore_ascii_case("t");
    let gaussian = family.eq_ignore_ascii_case("gaussian") || family.eq_ignore_ascii_case("normal");
    if name == "ACT.COPULA.TAIL" {
        return if student { 100_000 } else { 256 };
    }
    if name == "ACT.COPULA.CONDITIONALINV" {
        if student {
            return 1_000_000;
        }
        if gaussian {
            return 10_000;
        }
        if family.eq_ignore_ascii_case("independent") || family.eq_ignore_ascii_case("independence") || family.eq_ignore_ascii_case("frank") {
            return 256;
        }
        return 10_000; // Bounded 80-step inversion for Clayton/Gumbel.
    }
    if student || gaussian { 1_000_000 } else { 512 }
}

/// Reserves shape-dependent scans, sorting, matrix products and bounded numerical iterations.
/// Factors represent conservative work units; they are not a theorem about CPU instructions.
/// Existing domain/allocation caps remain in force even for hosts with the default free bank.
pub(super) fn estimate(name: &str, args: &[Arg]) -> u64 {
    let total = (0..args.len()).fold(0u64, |units, i| units.saturating_add(cells(args, i)));
    let base = 32u64.saturating_add(total.saturating_mul(32));
    let n = cells(args, 0).max(1);
    let (rows, columns) = shape(args, 0);
    let cubic = product(&[rows, rows, rows]);
    let extra = match name {
        "ACT.COMPOUND.PMF" | "ACT.COMPOUND.VAR" | "ACT.COMPOUND.TVAR" => {
            let grid = size(args, 4, 0).saturating_add(1);
            let binomial = matches!(args.get(1).map(|a| &a.value), Some(Value::Text(f)) if f.trim().eq_ignore_ascii_case("binomial"));
            product(&[grid, n.min(grid), if binomial { size(args, 2, 0).max(1) } else { 1 }, 16])
        }
        "ACT.MARKOV.POWER" | "ACT.MARKOV.STATE" => {
            let matrix = if name.ends_with("STATE") { shape(args, 1).0 } else { rows };
            product(&[matrix, matrix, matrix, 128, 16])
        }
        "ACT.CTMC.TRANSITION" => cubic.saturating_mul(168 * 16),
        "ACT.MARKOV.STATIONARY" | "ACT.MARKOV.ABSORPTION" | "ACT.CAPITAL.SD" | "ACT.CAPITAL.ALLOCATION" => {
            // The capital covariance matrix is argument 1, not the standard-deviation vector.
            let dimension = if name.starts_with("ACT.CAPITAL") { shape(args, 1).0 } else { rows };
            product(&[dimension, dimension, dimension, 32])
        }
        "ACT.MARKOV.REWARD" => product(&[rows, rows, size(args, 3, 0), 32]),
        "ACT.MARKOV.NONHOMOGENEOUS" | "ACT.MARKOV.NONHOMOGENEOUS.REWARD" => product(&[rows, columns, columns, 32]),
        "ACT.SEMIMARKOV.STATE" | "ACT.SEMIMARKOV.TRACE" | "ACT.SEMIMARKOV.REWARD" => {
            let index = if name.ends_with("REWARD") { 3 } else { 2 };
            let horizon = size(args, index, 0).saturating_add(1);
            let duration = size(args, index + 1, 0);
            product(&[columns, columns, horizon, horizon.saturating_add(duration), 32])
        }
        "ACT.GLM.FIT" => {
            let (n, p) = shape(args, 1);
            product(&[n, p, p, 32])
        }
        "ACT.COX.FIT" | "ACT.COX.BASELINE" | "ACT.COX.COVARIANCE" => {
            let p = shape(args, 2).1;
            product(&[n, n, p, p, 32])
        }
        "ACT.COX.SURVIVAL" => product(&[rows, columns.saturating_add(cells(args, 2)), 32]),
        "ACT.KAPLANMEIER" | "ACT.AALENJOHANSEN" | "ACT.BRIER.IPCW" | "ACT.KENDALL.TAU" | "ACT.COPULA.FIT" => product(&[n, n, 64]),
        "ACT.AALENJOHANSEN.MATRIX" => product(&[rows, columns, columns, columns, 32]),
        "ACT.GPD.FIT.MLE" => n.saturating_mul(100_000),
        "ACT.GPD.FIT.PWM" => product(&[n, n, 64]),
        "ACT.GPD.THRESHOLDS" => product(&[n, cells(args, 1), 128]),
        "ACT.MACK" => cubic.saturating_mul(64),
        "ACT.SURROGATE.FIT" => {
            let features = feature_count(columns, size(args, 4, 2));
            product(&[rows.saturating_add(shape(args, 2).0), features, features, 32])
        }
        "ACT.SURROGATE.PREDICT" => product(&[shape(args, 1).0, 64, 64]),
        "ACT.REINSURANCE.OPTIMIZE" => product(&[n, cells(args, 2), cells(args, 3), 64]),
        "ACT.RESERVE.BOOTSTRAP.ODP" => product(&[rows, rows, rows, 32]),
        _ if name.starts_with("ACT.COPULA.") => copula_work(name, args),
        "ACT.BAYES.BINOMIAL.PREDICT" => 100_000u64.saturating_add(size(args, 4, 0).saturating_mul(128)),
        _ if name.starts_with("ACT.BAYES.") || name == "ACT.LEV.GAMMA" => 100_000,
        _ => 0,
    };
    base.saturating_add(extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    /// Tests accounting without installing an engine, clock, RNG or thread-local state.
    struct Bank {
        remaining: u64,
        attempts: usize,
        rejected: bool,
    }
    impl Ctx for Bank {
        fn charge_work(&mut self, units: u64) -> bool {
            self.attempts += 1;
            if self.rejected || units > self.remaining {
                self.rejected = true;
                return false;
            }
            self.remaining -= units;
            true
        }
        fn date_system(&self) -> gridcraft_core::DateSystem {
            gridcraft_core::DateSystem::default()
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
    }

    #[test]
    fn every_registered_act_kernel_checks_the_host_before_entering_its_algorithm() {
        let mut count = 0;
        for spec in crate::all().iter().filter(|s| s.name.starts_with("ACT.")) {
            let args = vec![n(0.0); spec.min_args];
            let mut bank = Bank { remaining: 0, attempts: 0, rejected: false };
            assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num), "{}", spec.name);
            assert_eq!(bank.attempts, 1, "{} must reserve before parsing/allocating", spec.name);
            count += 1;
        }
        assert_eq!(count, 95);
    }

    #[test]
    fn model_sizes_and_saturating_arithmetic_cannot_wrap_expensive_work_to_zero() {
        assert_eq!(product(&[u64::MAX, 2]), u64::MAX);
        let small = vec![av(col(&[0.1, 1.0])), n(0.05)];
        let larger = vec![av(col(&[0.1; 100])), n(0.05)];
        assert!(estimate("ACT.ANNUITY", &larger) > estimate("ACT.ANNUITY", &small));
        let malformed = Arg::val(Value::from(gridcraft_core::Array { rows: usize::MAX, cols: usize::MAX, data: vec![] }));
        assert_eq!(estimate("ACT.MARKOV.POWER", &[malformed, n(1.0)]), u64::MAX);
        let severity = av(col(&[0.0, 1.0]));
        let poisson = vec![severity.clone(), t("poisson"), n(2.0), n(0.0), n(100.0)];
        let binomial = vec![severity, t("binomial"), n(100.0), n(0.5), n(100.0)];
        assert!(estimate("ACT.COMPOUND.PMF", &binomial) > estimate("ACT.COMPOUND.PMF", &poisson));
        let hostile = vec![av(col(&[0.0, 1.0])), t("binomial"), n(f64::MAX), n(0.5), n(f64::MAX)];
        assert_eq!(estimate("ACT.COMPOUND.PMF", &hostile), u64::MAX);
    }

    #[test]
    fn accepted_calls_keep_numeric_results_and_later_calls_share_exhaustion() {
        let spec = crate::lookup("ACT.LAYER").unwrap();
        let args = vec![n(100.0), n(25.0), n(50.0)];
        let mut bank = Bank { remaining: estimate(spec.name, &args), attempts: 0, rejected: false };
        assert_eq!((spec.imp)(&args, &mut bank), Value::Number(50.0));
        assert_eq!(bank.remaining, 0);
        assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num));
        assert!(bank.rejected);
    }

    #[test]
    fn adaptive_glm_and_compound_tail_charge_after_the_entry_reservation() {
        let spec = crate::lookup("ACT.GLM.FIT").unwrap();
        let args = vec![av(col(&[0.0, 2.0, 4.0])), av(col(&[1.0, 1.0, 1.0])), t("poisson")];
        let mut bank = Bank { remaining: estimate(spec.name, &args), attempts: 0, rejected: false };
        assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num));
        assert_eq!(bank.attempts, 2);
        let spec = crate::lookup("ACT.COMPOUND.TVAR").unwrap();
        let args = vec![av(col(&[0.0, 1.0])), t("poisson"), n(2.0), n(0.0), n(15.0), n(0.95)];
        let mut bank = Bank { remaining: estimate(spec.name, &args), attempts: 0, rejected: false };
        assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num));
        assert_eq!(bank.attempts, 2);
    }
    #[test]
    fn cox_and_bootstrap_stop_at_algorithm_checkpoints_with_no_partial_result() {
        let spec = crate::lookup("ACT.COX.FIT").unwrap();
        let args = vec![av(col(&[1.0, 2.0, 3.0, 4.0])), av(col(&[1.0, 1.0, 1.0, 0.0])), av(col(&[0.0, 1.0, 0.0, 1.0]))];
        let mut bank = Bank { remaining: estimate(spec.name, &args), attempts: 0, rejected: false };
        assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num));
        assert_eq!(bank.attempts, 2);
        let spec = crate::lookup("ACT.RESERVE.BOOTSTRAP.ODP").unwrap();
        let triangle = av(arr(vec![
            vec![nv(100.0), nv(200.0), nv(400.0)],
            vec![nv(100.0), nv(200.0), Value::Empty],
            vec![nv(100.0), Value::Empty, Value::Empty],
        ]));
        let args = vec![triangle, av(row(&[0.5; 6]))];
        // Allow the entry reservation and the nine-cell result allocation, but no replicate.
        let mut bank = Bank { remaining: estimate(spec.name, &args) + 9, attempts: 0, rejected: false };
        assert_eq!((spec.imp)(&args, &mut bank), Value::Error(CellError::Num));
        assert_eq!(bank.attempts, 3);
    }
    #[test]
    fn numeric_text_family_whitespace_and_initial_duration_cannot_evade_billing() {
        let numeric = vec![av(col(&[0.0, 1.0])), t("binomial"), n(50.0), n(0.5), n(100.0)];
        let text = vec![av(col(&[0.0, 1.0])), t(" binomial "), t("50"), n(0.5), t("100")];
        assert_eq!(estimate("ACT.COMPOUND.PMF", &numeric), estimate("ACT.COMPOUND.PMF", &text));
        let early = vec![av(arr(vec![vec![nv(1.0)]; 10])), n(1.0), n(2.0), n(0.0)];
        let mut late = early.clone();
        late[3] = t("7");
        assert!(estimate("ACT.SEMIMARKOV.STATE", &late) > estimate("ACT.SEMIMARKOV.STATE", &early));
        let small = vec![av(col(&[1.0, 2.0])), n(0.1), av(col(&[0.1]))];
        let mut larger = small.clone();
        larger[2] = av(col(&[0.1; 100]));
        assert!(estimate("ACT.COX.SURVIVAL", &larger) > estimate("ACT.COX.SURVIVAL", &small));
    }
    #[test]
    fn degenerate_compound_models_still_reserve_their_requested_output_grid() {
        let mut args = vec![av(col(&[0.0, 1.0])), t("binomial"), n(0.0), n(0.5), n(2.0)];
        let small = estimate("ACT.COMPOUND.PMF", &args);
        args[4] = n(4096.0);
        assert!(estimate("ACT.COMPOUND.PMF", &args) > small);
    }
    #[test]
    fn closed_copula_models_do_not_pay_the_elliptical_quadrature_reserve() {
        let mut args = vec![n(0.2), n(0.7), t("independent"), n(0.0)];
        let analytic = estimate("ACT.COPULA.CONDITIONALINV", &args);
        args[2] = t("student");
        let expensive = estimate("ACT.COPULA.CONDITIONALINV", &args);
        assert!(expensive > analytic);
        args[2] = t(" t ");
        assert_eq!(estimate("ACT.COPULA.CONDITIONALINV", &args), expensive);
        assert!(analytic < 1000, "closed independent transforms should permit practical simulation counts");
        let cdf = vec![n(0.2), n(0.7), t("frank"), n(1.0)];
        assert!(estimate("ACT.COPULA.CDF", &cdf) < 1000);
    }
}
