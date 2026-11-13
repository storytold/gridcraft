# Seeded worksheet counts and AR1 processes

Last updated: 2026-10-28. Target: GridCraft's Rust engine, native CLI/MCP, desktop and WASM.
This extends [Monte Carlo](monte-carlo.md); it does not require a Python simulator or a separate add-on install.

## Keep the deterministic forecast explicit

`MC.ACTIVE()` returns FALSE outside a native Monte Carlo trial. The five draw functions return
`#N/A` without a simulation host; they never silently draw from an unseeded RNG or substitute a mean.
Use **scalar** `IF` to retain an ordinary forecast:

```text
=IF(MC.ACTIVE(),MC.POISSON(Budget/CPL,"raw-leads"),Budget/CPL)
=IF(MC.ACTIVE(),MC.BINOMIAL(RawLeads,ContactRate,"contact"),RawLeads*ContactRate)
=IF(MC.ACTIVE(),MC.AR1.LOGNORMAL("monthly-cpl",Month,1,0.10,0.60),1)
```

The examples use illustrative named cells. Create those names or replace them with real references.
Array arguments are rejected; automatic lifting would otherwise make stream identity ambiguous.
Counts passed to conditional functions must already be whole numbers: deterministic expected counts
usually must be guarded at every stochastic stage. Scalar IF is lazy; array-valued IF is not this contract.

| Function | Parameters and meaning |
|---|---|
| `MC.ACTIVE()` | TRUE only during a hosted trial; consumes no random draw. |
| `MC.POISSON(mean, stream)` | Integer event count with nonnegative mean. |
| `MC.BINOMIAL(trials, probability, stream)` | Integer successes from an integer upstream count. |
| `MC.HYPERGEOMETRIC(draws, successes, population, stream)` | Integer allocation without replacement; draws and successes cannot exceed population. |
| `MC.AR1.NORMAL(process, step, mean, stationary_std_dev, phi)` | Normal stationary marginal; the spread is not the innovation standard deviation. |
| `MC.AR1.LOGNORMAL(process, step, mean, cv, phi)` | Positive arithmetic marginal mean and coefficient of variation; AR1 dependence is in latent normal space. |

For finite capacity shared across categories, allocate sequentially with hypergeometric draws,
then subtract each allocation and category population before drawing the next category. This
preserves both the total capacity and each category's available count. Independent binomial
rounding of the category shares does not provide those conservation properties.

## Stationary initialization and parameterization

A process starts with `Z1 ~ N(0,1)` and evolves as
`Zt = phi*Z(t-1) + sqrt((1-phi)*(1+phi))*epsilon(t)`, with independent standard normal innovations.
Normal outputs are `mean + stationary_std_dev*Zt`. Lognormal outputs use
`sigma2 = log(1+cv^2)` and `exp(log(mean)-sigma2/2+sqrt(sigma2)*Zt)`.
Thus the arithmetic mean/CV remain the specified marginal parameters. `phi` is the latent-normal
lag-one correlation; it generally differs from the Pearson correlation of the lognormal output.
Each process ID must keep the same distribution and parameters throughout one trial.

Sources: [NIST lognormal](https://www.itl.nist.gov/div898/handbook/eda/section3/eda3669.htm),
[AR stationarity](https://otexts.com/fpp3/AR.html),
[NIST Poisson](https://www.itl.nist.gov/div898/handbook/eda/section3/eda366j.htm),
[binomial definition](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.binom.html),
[hypergeometric definition](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.hypergeom.html).
Implementations are original Rust, not copied numerical-library code.

## Reproducibility and dependence

Configuration records `stochasticVersion: 1`. This version uses the existing SplitMix64 generator
and Box–Muller normal transform. Count stream keys include the seed, absolute trial number,
physical formula sheet **name**, row/column, distribution family, and explicit text stream ID.
Evaluating a sheet-scoped defined name keeps the caller's physical identity: the name's
scope changes reference resolution only, so callers on different sheets retain distinct streams.
Repeated calls at that identity reuse the same driving uniform; use different IDs for independent
count stages in the same cell. Moving/renaming a formula can change its draws.

Named AR1 processes use seed, absolute trial number and process ID, independently of formula
location. Asking for month 24 first generates the same prefix as asking for months 1 through 24.
Synchronous and incremental runs use the same absolute trial keys. Process state is discarded
after each trial, before report publication and the ordinary source recalculation.

The configured correlation matrix applies to **static input marginals only**. Different named
AR1 processes and count streams have independent driving keys; conditional funnel stages still
inherit dependence through their upstream counts. Correlated AR1 innovations or a configurable
cross-process copula are not included. The seed does not control historical `RAND`, `NOW`,
external data or script effects. A stream hash separates simulation streams; it is not cryptographic.

Static marginals support random or replicated Latin hypercube sampling and the existing Gaussian
copula. New discrete marginals introduce ties; final Pearson correlations need not equal latent
matrix entries. LHS stratifies the static driving uniforms, not every conditional worksheet draw.
Existing uniform/triangular/normal-only input sequences retain their original RNG consumption.

## Numerical and operational limits

- Poisson mean: 0 through 50,000. Whole populations/trials: 0 through 1,000,000,000.
- A nondegenerate discrete support has at most 100,000 outcomes. Exact zero/one probabilities
  retain degenerate large-population cases without allocating a full support.
- Bernoulli inversion compares the compensated sum `u+p` with 1, avoiding a rounded complement at a jump.
  General discrete inverses use mode-relative probability recurrences and compensated lower/upper sums;
  Poisson's finite right-tail truncation uses a geometric omitted-mass bound. These are finite
  `f64` calculations, not arbitrary precision or a guarantee of bitwise equality across all libm implementations.
  Drivers extremely close to a CDF jump can choose an adjacent count because of recurrence/normalization
  rounding. Regression fixtures verify absolute probability-bracket residual at most `1e-12` for
  representative binomial/Poisson grids, not universal exact inversion at every floating-point jump.
  Lognormal log-variance avoids squaring large CVs; actual sample overflow/underflow still aborts.
- Discrete preparation charges before allocation (`8 * support`, plus bounded tail expansion),
  and lookup charges logarithmic work. Static input tables are prepared once per run. Conditional
  tables are built for the current request and can exhaust maxWork in count-heavy models.
- A prepared table retains about 32 bytes per outcome; construction peaks around 40 bytes per
  outcome. Up to 32 full static supports can therefore consume about 102 MB of retained tables.
- Stream/process IDs: nonblank text, at most 128 bytes. AR1 steps: integer 1 through 1,200;
  absolute phi below 1; at most 64 named processes and 65,536 cached latent values per trial.
- Work refusal, invalid stochastic parameters/arity, incompatible process reuse and nonfinite
  results abort the whole run even if a worksheet `IFERROR` catches the cell error. No report is
  published and source assumptions remain unchanged. Cancellation is cooperative at checkpoints;
  this is not a hard wall-clock interruption guarantee.
- Existing per-run/shared work banks, sampled-cell/report limits, protection and undo rules still
  apply. Reduce iterations/model cost rather than bypassing them. Reports retain version and
  operational request/AR1-step counters; these counters are not counts of independent samples.

## Revision history

- 2026-10-28: specify native conditional counts, stationary AR1, static marginals and versioned streams.
