//! Bounded inverse sampling of discrete counts, without another RNG or ACT dependency.
//!
//! A quantile is the smallest supported integer whose CDF reaches `u`; `u == 0`
//! explicitly means the support minimum. Mode-relative PMF recurrences avoid absolute
//! PMF underflow near the mode. Mode weight 2^512 keeps even the smallest positive
//! f64 driving probability away from subnormal rounding during inversion.
//! Compensated lower/strict-upper tables support repeated
//! O(log(width)) queries and avoid subtracting a near-one CDF.
//!
//! Poisson's omitted right mass is bounded geometrically below EPSILON²/16 times
//! its mode/right mass. General tables have finite-f64 recurrence/normalization
//! error near CDF jumps; they do not promise universally exact real-arithmetic
//! inversion. Bernoulli boundaries use an exact compensated comparison.
//! Recurrence identities follow the documented probability mass functions:
//! <https://www.itl.nist.gov/div898/handbook/eda/section3/eda366j.htm>,
//! <https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.binom.html>,
//! <https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.hypergeom.html>.

use gridcraft_calc::WorkHook;

use super::{CMD, Result, bad};

/// Wider feasible supports refuse before allocating rather than using an approximation.
pub(super) const MAX_SUPPORT: u64 = 100_000;
/// Bounds Poisson table work while allowing the required right-tail expansion.
pub(super) const MAX_POISSON_MEAN: f64 = 50_000.0;
/// Counts remain exact f64 integers and all intermediate u64 sums stay bounded.
pub(super) const MAX_POPULATION: u64 = 1_000_000_000;
const MODE_WEIGHT: f64 = 1.3407807929942597e154; // 2^512; total <= 100000 * this scale.
const POISSON_TAIL_RELATIVE: f64 = f64::EPSILON * f64::EPSILON / 16.0;

/// Compensated positive sums retain the small PMF recurrence terms without cancellation.
#[derive(Clone, Copy, Debug, Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    /// Kahan's update is sufficient because every summand is nonnegative.
    fn add(&mut self, value: f64) {
        let adjusted = value - self.correction;
        let total = self.value + adjusted;
        self.correction = (total - self.value) - adjusted;
        self.value = total;
    }
}

/// Carry a logarithmic weight only when subnormal multiplication could stick at the
/// rounded minimum positive value instead of continuing to decay toward zero.
struct Weight {
    value: f64,
    logarithm: Option<f64>,
}
impl Weight {
    fn mode() -> Self {
        Self { value: MODE_WEIGHT, logarithm: None }
    }
    /// Applies a PMF-ratio identity and rejects a non-finite numerical recurrence.
    fn next(&mut self, ratio: f64) -> Result<f64> {
        if !ratio.is_finite() || ratio < 0.0 {
            return Err(bad(CMD, "invalid discrete probability recurrence"));
        }
        if ratio == 0.0 {
            self.value = 0.0;
            self.logarithm = None;
        } else if let Some(logarithm) = &mut self.logarithm {
            *logarithm += ratio.ln();
            self.value = logarithm.exp();
        } else {
            let product = self.value * ratio;
            if product < f64::MIN_POSITIVE && self.value > 0.0 {
                let logarithm = self.value.ln() + ratio.ln();
                self.value = logarithm.exp();
                self.logarithm = Some(logarithm);
            } else {
                self.value = product;
            }
        }
        if self.value.is_finite() { Ok(self.value) } else { Err(bad(CMD, "non-finite discrete probability weight")) }
    }
}

/// Captures the active policy once and honors refusal even for a degenerate constant.
fn begin() -> Result<Option<WorkHook>> {
    let hook = WorkHook::current();
    spend(hook.as_ref(), 1)?;
    Ok(hook)
}

/// Drivers are never silently clipped into the accepted half-open probability interval.
fn validate_u(u: f64) -> Result<()> {
    if (0.0..1.0).contains(&u) { Ok(()) } else { Err(bad(CMD, "a discrete driving probability must be finite and in [0,1)")) }
}

/// Precharges conservative work and provides zero-unit cancellation/failure checkpoints.
fn spend(hook: Option<&WorkHook>, units: u64) -> Result<()> {
    if hook.is_none_or(|h| h.charge(units)) { Ok(()) } else { Err(bad(CMD, "discrete sampling stopped by work limit or cancellation")) }
}

#[derive(Clone, Copy)]
struct Support {
    low: u64,
    high: u64,
    mode: u64,
}

/// Precharges eight units per outcome before allocating: PMF/two-tail storage,
/// recurrence, normalization and two cumulative passes. Every 128 positions also
/// consults the persistent latch. Peak storage is bounded by 40 bytes per outcome.
fn prepare_table(
    support: Support,
    left_ratio: impl Fn(u64) -> f64,
    right_ratio: impl Fn(u64) -> f64,
    hook: Option<&WorkHook>,
) -> Result<QuantileTable> {
    let length = support
        .high
        .checked_sub(support.low)
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n <= MAX_SUPPORT)
        .ok_or_else(|| bad(CMD, "discrete support exceeds 100000 outcomes"))?;
    if !(support.low..=support.high).contains(&support.mode) {
        return Err(bad(CMD, "discrete mode is outside its support"));
    }
    spend(hook, length.saturating_mul(8))?;
    let length = usize::try_from(length).map_err(|_| bad(CMD, "discrete support does not fit this platform"))?;
    let mut weights = vec![0.0; length];
    put(&mut weights, support.mode - support.low, MODE_WEIGHT)?;
    let mut weight = Weight::mode();
    for k in (support.low + 1..=support.mode).rev() {
        if k.is_multiple_of(128) {
            spend(hook, 0)?;
        }
        put(&mut weights, k - 1 - support.low, weight.next(left_ratio(k))?)?;
    }
    weight = Weight::mode();
    for k in support.mode..support.high {
        if k.is_multiple_of(128) {
            spend(hook, 0)?;
        }
        put(&mut weights, k + 1 - support.low, weight.next(right_ratio(k))?)?;
    }
    prepare_tails(&weights, support, hook)
}

/// Checked indexing keeps any internal recurrence failure on the normal error path.
fn put(weights: &mut [f64], index: u64, value: f64) -> Result<()> {
    let index = usize::try_from(index).map_err(|_| bad(CMD, "discrete index does not fit this platform"))?;
    let slot = weights.get_mut(index).ok_or_else(|| bad(CMD, "discrete recurrence escaped its support"))?;
    *slot = value;
    Ok(())
}

/// A reusable inverse table: static marginals prepare it once under the sampling
/// scope, then use logarithmic lookups rather than rebuilding a support every trial.
#[derive(Debug)]
pub(super) struct QuantileTable {
    low: u64,
    high: u64,
    lower: Vec<Sum>,
    upper: Vec<Sum>,
    total: Sum,
    bernoulli_probability: Option<f64>,
    half_quantile: Option<u64>,
}

/// Prepares strict upper mass separately, never by subtracting a near-one lower CDF.
fn prepare_tails(weights: &[f64], support: Support, hook: Option<&WorkHook>) -> Result<QuantileTable> {
    let mut total = Sum::default();
    for (i, weight) in weights.iter().enumerate() {
        if i.is_multiple_of(128) {
            spend(hook, 0)?;
        }
        total.add(*weight);
    }
    if !total.value.is_finite() || total.value <= 0.0 {
        return Err(bad(CMD, "discrete probability normalization failed"));
    }
    let mut lower = Vec::with_capacity(weights.len());
    let mut tail = Sum::default();
    for (i, weight) in weights.iter().enumerate() {
        if i.is_multiple_of(128) {
            spend(hook, 0)?;
        }
        tail.add(*weight);
        lower.push(tail);
    }
    let mut upper = Vec::with_capacity(weights.len());
    tail = Sum::default();
    for (i, weight) in weights.iter().enumerate().rev() {
        if i.is_multiple_of(128) {
            spend(hook, 0)?;
        }
        upper.push(tail);
        tail.add(*weight);
    }
    upper.reverse();
    spend(hook, 0)?;
    Ok(QuantileTable { low: support.low, high: support.high, lower, upper, total, bernoulli_probability: None, half_quantile: None })
}

impl QuantileTable {
    /// A degenerate table does not allocate a probability vector.
    fn atom(value: u64) -> Self {
        Self {
            low: value,
            high: value,
            lower: Vec::new(),
            upper: Vec::new(),
            total: Sum { value: 1.0, correction: 0.0 },
            bernoulli_probability: None,
            half_quantile: None,
        }
    }

    /// Inverts the smaller tail in O(log(width)); the product residual prevents
    /// rounding `u * mass` onto a CDF jump and losing the next f64 above that jump.
    pub fn quantile(&self, u: f64) -> Result<f64> {
        let hook = begin()?;
        validate_u(u)?;
        if self.low == self.high || u == 0.0 {
            return Ok(self.low as f64);
        }
        if let Some(probability) = self.bernoulli_probability {
            spend(hook.as_ref(), 4)?;
            return Ok(if bernoulli_reaches_zero(u, probability) { 0.0 } else { 1.0 });
        }
        if u == 0.5
            && let Some(value) = self.half_quantile
        {
            return Ok(value as f64);
        }
        let length = self.lower.len();
        if length == 0 || self.upper.len() != length {
            return Err(bad(CMD, "empty or inconsistent discrete inverse table"));
        }
        spend(hook.as_ref(), u64::from(usize::BITS - length.leading_zeros()) + 1)?;
        let (mut low, mut high) = (0usize, length - 1);
        while low < high {
            spend(hook.as_ref(), 0)?;
            let mid = low + (high - low) / 2;
            let reached = if u <= 0.5 {
                let sum = self.lower.get(mid).ok_or_else(|| bad(CMD, "missing discrete lower mass"))?;
                compare_mass(*sum, self.total, u) >= 0.0
            } else {
                let sum = self.upper.get(mid).ok_or_else(|| bad(CMD, "missing discrete upper mass"))?;
                compare_mass(*sum, self.total, 1.0 - u) <= 0.0
            };
            if reached {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        spend(hook.as_ref(), 0)?;
        Ok((self.low + low as u64) as f64)
    }

    /// Prepares Poisson(lambda) without a normal approximation. Mean is capped
    /// at 50000; a checked geometric bound controls the omitted right tail.
    pub fn poisson(lambda: f64) -> Result<Self> {
        let hook = begin()?;
        if !lambda.is_finite() || !(0.0..=MAX_POISSON_MEAN).contains(&lambda) {
            return Err(bad(CMD, "Poisson mean must be finite and between 0 and 50000"));
        }
        if lambda == 0.0 {
            return Ok(Self::atom(0));
        }
        let mode = lambda.floor() as u64;
        let high = poisson_high(lambda, mode, hook.as_ref())?;
        prepare_table(Support { low: 0, high, mode }, |k| k as f64 / lambda, |k| lambda / (k + 1) as f64, hook.as_ref())
    }

    /// Prepares a finite binomial by PMF ratios. Nondegenerate requests have
    /// at most 100000 outcomes; p=0/1 and zero trials stay exact atoms.
    pub fn binomial(n: u64, p: f64) -> Result<Self> {
        let hook = begin()?;
        if n > MAX_POPULATION || !(0.0..=1.0).contains(&p) {
            return Err(bad(CMD, "binomial trials must not exceed 1000000000 and probability must be in [0,1]"));
        }
        if n == 0 || p == 0.0 {
            return Ok(Self::atom(0));
        }
        if p == 1.0 {
            return Ok(Self::atom(n));
        }
        if n == 1 {
            let mut table = Self::atom(0);
            table.high = 1;
            table.bernoulli_probability = Some(p);
            return Ok(table);
        }
        if n >= MAX_SUPPORT {
            return Err(bad(CMD, "nondegenerate binomial support exceeds 100000 outcomes"));
        }
        let mode = (((n + 1) as f64 * p).floor() as u64).min(n);
        let mut table = prepare_table(
            Support { low: 0, high: n, mode },
            |k| (k as f64 / (n - k + 1) as f64) * ((1.0 - p) / p),
            |k| ((n - k) as f64 / (k + 1) as f64) * (p / (1.0 - p)),
            hook.as_ref(),
        )?;
        if p == 0.5 {
            table.half_quantile = Some(n / 2);
        }
        Ok(table)
    }

    /// Prepares successes in draws without replacement. Population is <=1e9;
    /// feasible support must have at most 100000 outcomes, even for u=0 queries.
    pub fn hypergeometric(draws: u64, successes: u64, population: u64) -> Result<Self> {
        let hook = begin()?;
        if population > MAX_POPULATION || draws > population || successes > population {
            return Err(bad(CMD, "hypergeometric counts must fit a population no greater than 1000000000"));
        }
        let low = draws.saturating_sub(population - successes);
        let high = draws.min(successes);
        if low == high {
            return Ok(Self::atom(low));
        }
        let mode = (((u128::from(draws) + 1) * (u128::from(successes) + 1)) / (u128::from(population) + 2)) as u64;
        let mut table = prepare_table(
            Support { low, high, mode: mode.clamp(low, high) },
            |k| (k as f64 * (population - successes + k - draws) as f64) / ((successes - k + 1) as f64 * (draws - k + 1) as f64),
            |k| ((successes - k) as f64 * (draws - k) as f64) / ((k + 1) as f64 * (population - successes + k + 1 - draws) as f64),
            hook.as_ref(),
        )?;
        table.half_quantile = if successes * 2 == population {
            Some(draws / 2)
        } else if draws * 2 == population {
            Some(successes / 2)
        } else {
            None
        };
        Ok(table)
    }
}

/// Bernoulli's zero CDF is exactly 1-p for the supplied binary64 p. TwoSum
/// compares u+p with one including its rounding residual; forming 1-p first
/// would move some non-dyadic boundaries (for example p=0.2, u=0.8).
fn bernoulli_reaches_zero(u: f64, probability: f64) -> bool {
    let sum = u + probability;
    let virtual_probability = sum - u;
    let residual = (u - (sum - virtual_probability)) + (probability - virtual_probability);
    sum < 1.0 || (sum == 1.0 && residual <= 0.0)
}

/// Compares compensated mass with probability*total using an error-free product
/// residual, without an arbitrary tolerance around discrete probability jumps.
fn compare_mass(mass: Sum, total: Sum, probability: f64) -> f64 {
    let product = probability * total.value;
    let residual = probability.mul_add(total.value, -product) - probability * total.correction;
    (mass.value - product) - mass.correction - residual
}

/// To the right of the mode, decreasing ratios bound all omitted terms by the next
/// geometric series. Using only mode/right mass makes the relative bound conservative.
fn poisson_high(lambda: f64, mode: u64, hook: Option<&WorkHook>) -> Result<u64> {
    let mut weight = Weight::mode();
    let mut sum = Sum::default();
    sum.add(MODE_WEIGHT);
    for k in mode..MAX_SUPPORT - 1 {
        spend(hook, 1)?;
        let next = weight.next(lambda / (k + 1) as f64)?;
        sum.add(next);
        let ratio = lambda / (k + 2) as f64;
        let remainder = next * ratio / (1.0 - ratio);
        if remainder <= sum.value * POISSON_TAIL_RELATIVE {
            return Ok(k + 1);
        }
    }
    Err(bad(CMD, "Poisson tail exceeds the bounded discrete support"))
}

/// Conditional draw convenience wrapper; static inputs should retain a prepared table.
pub(super) fn poisson_quantile(lambda: f64, u: f64) -> Result<f64> {
    validate_u(u)?;
    QuantileTable::poisson(lambda)?.quantile(u)
}

/// Conditional binomial convenience wrapper with an integer trial count.
pub(super) fn binomial_quantile(n: u64, p: f64, u: f64) -> Result<f64> {
    validate_u(u)?;
    QuantileTable::binomial(n, p)?.quantile(u)
}

/// Conditional allocation convenience wrapper without replacement.
pub(super) fn hypergeometric_quantile(draws: u64, successes: u64, population: u64, u: f64) -> Result<f64> {
    validate_u(u)?;
    QuantileTable::hypergeometric(draws, successes, population)?.quantile(u)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    use gridcraft_calc::WorkHook;

    use super::{MAX_SUPPORT, QuantileTable, binomial_quantile, hypergeometric_quantile, poisson_quantile};

    type Oracles<'a> = &'a [(f64, u64)];

    /// Degenerate distributions retain their actual atom even for a zero driver.
    #[test]
    fn degenerate_distributions_and_zero_probability_support() {
        for u in [0.0, 0.5, 1.0 - f64::EPSILON / 2.0] {
            assert_eq!(poisson_quantile(0.0, u).unwrap(), 0.0);
            assert_eq!(binomial_quantile(0, 0.4, u).unwrap(), 0.0);
            assert_eq!(binomial_quantile(12, 0.0, u).unwrap(), 0.0);
            assert_eq!(binomial_quantile(12, 1.0, u).unwrap(), 12.0);
            assert_eq!(hypergeometric_quantile(0, 0, 0, u).unwrap(), 0.0);
            assert_eq!(hypergeometric_quantile(5, 10, 10, u).unwrap(), 5.0);
        }
        assert_eq!(poisson_quantile(50_000.0, 0.0).unwrap(), 0.0);
        assert_eq!(binomial_quantile(100, 0.5, 0.0).unwrap(), 0.0);
        assert_eq!(hypergeometric_quantile(8, 9, 10, 0.0).unwrap(), 7.0);
    }

    /// Exact small rational CDF boundaries use the lower supported outcome.
    #[test]
    fn exact_small_distribution_boundaries() {
        let above = |x: f64| f64::from_bits(x.to_bits() + 1);
        assert_eq!(binomial_quantile(1, 0.5, 0.5).unwrap(), 0.0);
        assert_eq!(binomial_quantile(1, 0.5, above(0.5)).unwrap(), 1.0);
        assert_eq!(binomial_quantile(9, 0.5, 0.5).unwrap(), 4.0);
        assert_eq!(binomial_quantile(9, 0.5, above(0.5)).unwrap(), 5.0);
        assert_eq!(hypergeometric_quantile(2, 2, 4, 1.0 / 6.0).unwrap(), 0.0);
        assert_eq!(hypergeometric_quantile(2, 2, 4, above(1.0 / 6.0)).unwrap(), 1.0);
        assert_eq!(poisson_quantile(std::f64::consts::LN_2, 0.5).unwrap(), 0.0);
    }

    /// Decimal.from_float reference comparisons retain the exact binary64 input
    /// parameters around non-dyadic Bernoulli CDF jumps rather than rounding 1-p.
    #[test]
    fn bernoulli_boundaries_compare_exact_input_sum() {
        let above = |x: f64| f64::from_bits(x.to_bits() + 1);
        let below = |x: f64| f64::from_bits(x.to_bits() - 1);
        for (p, u, expected) in [
            (0.2, 0.8, 1.0),
            (0.8, 0.2, 1.0),
            (0.3, 0.7, 0.0),
            (0.7, 0.3, 0.0),
            (0.1, 0.9, 1.0),
            (0.9, 0.1, 1.0),
            (0.2, below(0.8), 0.0),
            (0.2, above(0.8), 1.0),
            (0.3, above(0.7), 1.0),
            (0.3, below(0.7), 0.0),
            (0.125, 0.875, 0.0),
            (0.125, above(0.875), 1.0),
            (below(1.0), f64::EPSILON / 2.0, 0.0),
            (below(1.0), above(f64::EPSILON / 2.0), 1.0),
            (1e-250, below(1.0), 0.0),
        ] {
            let table = QuantileTable::binomial(1, p).unwrap();
            assert_eq!(table.quantile(u).unwrap(), expected, "p={p:.17e}, u={u:.17e}");
            assert_eq!(binomial_quantile(1, p, u).unwrap(), expected);
        }
    }

    /// Independent absolute PMF formulas bound probability-bracket error, rather
    /// than demanding identical indices at every floating-point CDF jump. Both a
    /// uniform driver grid and adjacent representable CDF values stay within 1e-12.
    #[test]
    fn finite_table_probability_brackets_have_small_absolute_residual() {
        let mut binomial_cdf = Vec::new();
        let mut sum = 0.0;
        for k in 0..=20_u32 {
            let mut choose = 1_u64;
            for j in 0..k {
                choose = choose * u64::from(20 - j) / u64::from(j + 1);
            }
            sum += choose as f64 * 0.3_f64.powi(k as i32) * 0.7_f64.powi((20 - k) as i32);
            binomial_cdf.push(sum);
        }
        let mut poisson_cdf = Vec::new();
        let (mut sum, mut factorial) = (0.0, 1.0);
        for k in 0..=64_i32 {
            if k > 0 {
                factorial *= k as f64;
            }
            sum += (-10.0_f64).exp() * 10.0_f64.powi(k) / factorial;
            poisson_cdf.push(sum);
        }
        for (table, cdf) in [(QuantileTable::binomial(20, 0.3).unwrap(), binomial_cdf), (QuantileTable::poisson(10.0).unwrap(), poisson_cdf)] {
            let mut drivers: Vec<f64> = (0..4096).map(|i| (i as f64 + 0.5) / 4096.0).collect();
            for &boundary in &cdf {
                if boundary > 0.0 && boundary < 1.0 {
                    drivers.extend([f64::from_bits(boundary.to_bits() - 1), boundary, f64::from_bits(boundary.to_bits() + 1)]);
                }
            }
            for u in drivers.into_iter().filter(|u| *u < 1.0) {
                let q = table.quantile(u).unwrap() as usize;
                let previous = if q == 0 { 0.0 } else { cdf[q - 1] };
                let current = cdf[q];
                assert!(previous - u <= 1e-12, "q={q}, u={u:.17e}, previousCDF={previous:.17e}");
                assert!(u - current <= 1e-12, "q={q}, u={u:.17e}, currentCDF={current:.17e}");
            }
        }
    }

    /// Invalid domains and support widths refuse instead of clipping or approximating.
    #[test]
    fn invalid_and_oversized_inputs_are_rejected() {
        for u in [-0.1, 1.0, f64::INFINITY, f64::NAN] {
            assert!(poisson_quantile(0.0, u).is_err());
            assert!(binomial_quantile(0, 0.5, u).is_err());
            assert!(hypergeometric_quantile(0, 0, 0, u).is_err());
        }
        for mean in [-1.0, 50_001.0, f64::NAN, f64::INFINITY] {
            assert!(poisson_quantile(mean, 0.5).is_err());
        }
        assert!(binomial_quantile(MAX_SUPPORT, 0.2, 0.5).is_err());
        assert!(binomial_quantile(10, f64::NAN, 0.5).is_err());
        assert!(hypergeometric_quantile(11, 2, 10, 0.5).is_err());
        assert!(hypergeometric_quantile(2, 11, 10, 0.5).is_err());
        assert!(hypergeometric_quantile(200_000, 200_000, 400_000, 0.5).is_err());
    }

    /// Precharged work refuses before materializing a distribution, and stays latched.
    #[test]
    fn work_refusal_and_cancellation_prevent_a_draw() {
        let used = Arc::new(AtomicU64::new(0));
        let seen = used.clone();
        let hook = WorkHook::new(move |units| {
            seen.fetch_add(units, Ordering::Relaxed);
            units <= 1
        });
        let _scope = hook.enter().unwrap();
        assert!(binomial_quantile(100, 0.3, 0.4).is_err());
        assert!(used.load(Ordering::Relaxed) >= 405);
        assert!(!WorkHook::allowed());
        assert!(poisson_quantile(0.0, 0.5).is_err());
        assert!(hypergeometric_quantile(0, 0, 0, 0.5).is_err());
    }

    /// Fixed oracles use 100-digit Decimal PMFs from the documented formulas,
    /// with exact Decimal.from_float parameter/uniform inputs, not this recurrence.
    #[test]
    fn high_precision_poisson_oracles_include_both_tails() {
        let cases: &[(f64, Oracles<'_>)] = &[
            (0.001, &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 0), (0.1, 0), (0.5, 0), (0.9, 0), (0.999999999999, 3), (0.9999999999999999, 4)]),
            (4.0, &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 0), (0.1, 2), (0.5, 4), (0.9, 7), (0.999999999999, 25), (0.9999999999999999, 29)]),
            (
                100.0,
                &[
                    (f64::from_bits(1), 0),
                    (1e-250, 0),
                    (1e-12, 38),
                    (0.1, 87),
                    (0.5, 100),
                    (0.9, 113),
                    (0.999999999999, 178),
                    (0.9999999999999999, 193),
                ],
            ),
            (
                1000.0,
                &[
                    (f64::from_bits(1), 71),
                    (1e-250, 147),
                    (1e-12, 786),
                    (0.1, 960),
                    (0.5, 1000),
                    (0.9, 1041),
                    (0.999999999999, 1230),
                    (0.9999999999999999, 1270),
                ],
            ),
            (
                50000.0,
                &[
                    (f64::from_bits(1), 41649),
                    (1e-250, 42635),
                    (1e-12, 48435),
                    (0.1, 49714),
                    (0.5, 50000),
                    (0.9, 50287),
                    (0.999999999999, 51581),
                    (0.9999999999999999, 51847),
                ],
            ),
        ];
        for (mean, expected) in cases {
            let table = QuantileTable::poisson(*mean).unwrap();
            for &(u, k) in *expected {
                assert_eq!(table.quantile(u).unwrap(), k as f64, "u={u:.17e}, mean={mean:?}");
            }
        }
    }

    /// Fixed oracles use 100-digit Decimal PMFs from the documented formulas,
    /// with exact Decimal.from_float parameter/uniform inputs, not this recurrence.
    /// At p=0.5/u=0.5, exact symmetry overrides Decimal accumulation roundoff.
    #[test]
    fn high_precision_binomial_oracles_include_both_tails() {
        let cases: &[(u64, f64, Oracles<'_>)] = &[
            (1, 0.2, &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 0), (0.1, 0), (0.5, 0), (0.9, 1), (0.999999999999, 1), (0.9999999999999999, 1)]),
            (
                20,
                0.3,
                &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 0), (0.1, 3), (0.5, 6), (0.9, 9), (0.999999999999, 20), (0.9999999999999999, 20)],
            ),
            (
                2000,
                0.73,
                &[
                    (f64::from_bits(1), 633),
                    (1e-250, 736),
                    (1e-12, 1317),
                    (0.1, 1435),
                    (0.5, 1460),
                    (0.9, 1485),
                    (0.999999999999, 1596),
                    (0.9999999999999999, 1617),
                ],
            ),
            (
                99999,
                0.5,
                &[
                    (f64::from_bits(1), 43925),
                    (1e-250, 44660),
                    (1e-12, 48887),
                    (0.1, 49797),
                    (0.5, 49999),
                    (0.9, 50202),
                    (0.999999999999, 51112),
                    (0.9999999999999999, 51297),
                ],
            ),
            (
                50000,
                0.999,
                &[
                    (f64::from_bits(1), 49485),
                    (1e-250, 49559),
                    (1e-12, 49893),
                    (0.1, 49941),
                    (0.5, 49950),
                    (0.9, 49959),
                    (0.999999999999, 49991),
                    (0.9999999999999999, 49995),
                ],
            ),
        ];
        for (n, p, expected) in cases {
            let table = QuantileTable::binomial(*n, *p).unwrap();
            for &(u, k) in *expected {
                assert_eq!(table.quantile(u).unwrap(), k as f64, "u={u:.17e}, table={:?}", (n, p));
            }
        }
    }

    /// Fixed oracles use 100-digit Decimal PMFs from the documented formulas,
    /// with exact Decimal.from_float parameter/uniform inputs, not this recurrence.
    #[test]
    fn high_precision_hypergeometric_oracles_include_both_tails() {
        let cases: &[(u64, u64, u64, Oracles<'_>)] = &[
            (
                20,
                30,
                100,
                &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 0), (0.1, 4), (0.5, 6), (0.9, 8), (0.999999999999, 19), (0.9999999999999999, 20)],
            ),
            (
                200,
                430,
                1000,
                &[
                    (f64::from_bits(1), 0),
                    (1e-250, 0),
                    (1e-12, 43),
                    (0.1, 78),
                    (0.5, 86),
                    (0.9, 94),
                    (0.999999999999, 130),
                    (0.9999999999999999, 137),
                ],
            ),
            (
                50,
                430000000,
                1000000000,
                &[(f64::from_bits(1), 0), (1e-250, 0), (1e-12, 1), (0.1, 17), (0.5, 21), (0.9, 26), (0.999999999999, 45), (0.9999999999999999, 48)],
            ),
            (
                2500,
                2000,
                4000,
                &[
                    (f64::from_bits(1), 687),
                    (1e-250, 749),
                    (1e-12, 1142),
                    (0.1, 1230),
                    (0.5, 1250),
                    (0.9, 1270),
                    (0.999999999999, 1358),
                    (0.9999999999999999, 1375),
                ],
            ),
        ];
        for (draws, successes, population, expected) in cases {
            let table = QuantileTable::hypergeometric(*draws, *successes, *population).unwrap();
            for &(u, k) in *expected {
                assert_eq!(table.quantile(u).unwrap(), k as f64, "u={u:.17e}, table={:?}", (draws, successes, population));
            }
        }
    }

    /// Repeated static draws charge logarithmic lookup work rather than rebuilding
    /// a 50000-mean support for each of 128 independent driving probabilities.
    #[test]
    fn prepared_table_reuses_support_and_charges_small_queries() {
        let table = QuantileTable::poisson(50_000.0).unwrap();
        let used = Arc::new(AtomicU64::new(0));
        let seen = used.clone();
        let hook = WorkHook::new(move |units| {
            seen.fetch_add(units, Ordering::Relaxed);
            true
        });
        let _scope = hook.enter().unwrap();
        let mut previous = 0.0;
        for i in 0..128 {
            let q = table.quantile((i as f64 + 0.5) / 128.0).unwrap();
            assert_eq!(q, q.floor());
            assert!(q >= previous);
            previous = q;
        }
        assert!(used.load(Ordering::Relaxed) < 4000);
    }

    /// Uniform midpoint quadrature recovers PMF means/variances without an RNG,
    /// with a finite-grid tolerance rather than a flaky random sampling assertion.
    #[test]
    fn inverse_tables_recover_distribution_moments() {
        let cases = [
            (QuantileTable::poisson(4.0).unwrap(), 4.0, 4.0),
            (QuantileTable::binomial(20, 0.3).unwrap(), 6.0, 4.2),
            (QuantileTable::hypergeometric(20, 30, 100).unwrap(), 6.0, 4.2 * 80.0 / 99.0),
        ];
        for (table, expected_mean, expected_variance) in cases {
            let n = 65536;
            let mut first = 0.0;
            let mut second = 0.0;
            for i in 0..n {
                let q = table.quantile((i as f64 + 0.5) / n as f64).unwrap();
                first += q;
                second += q * q;
            }
            let mean = first / n as f64;
            let variance = second / n as f64 - mean * mean;
            assert!((mean - expected_mean).abs() < 0.001);
            assert!((variance - expected_variance).abs() < 0.005);
        }
    }

    /// Mid-build cancellation returns no partially usable table, and an already
    /// prepared table still polls the current run when it is queried later.
    #[test]
    fn cancellation_is_checked_during_preparation_and_lookup() {
        let ready = QuantileTable::binomial(300, 0.3).unwrap();
        let checks = Arc::new(AtomicU64::new(0));
        let seen = checks.clone();
        let hook = WorkHook::new(move |units| units > 0 || seen.fetch_add(1, Ordering::Relaxed) < 3);
        let _scope = hook.enter().unwrap();
        assert!(QuantileTable::binomial(1000, 0.5).is_err());
        assert!(!WorkHook::allowed());
        assert!(ready.quantile(0.7).is_err());
    }
}
