# Actuarial worksheet pack

> Updated: 2026-10-28. Rust-native deterministic extension, based on Monte Carlo
> commit `02e4d50088e532f81a96f80eab539cabf77da7e4`. Implementation in
> `crates/functions/src/actuarial/`. This is a GridCraft extension, not an actuarial
> certification, regulatory filing engine, or claim to cover every actuarial method.

The 95 `ACT.*` functions compose with ordinary workbook formulas, named ranges, charts,
CLI and MCP. They use the existing calculation engine and spill results into cells. No
additional Python runtime, engine dependency, RNG, native-only dependency, or UI is needed.
This pack is compiled into the worksheet library; it is not a dynamically loaded plugin.
Excel does not implement these names: Excel can display cached values but cannot recalculate
`ACT.*` without an equivalent extension. GridCraft preserves/recalculates them on XLSX import.

This page specifies the 52 foundational functions. The [scientific-methods guide](actuarial-scientific.md)
specifies the other 43: censored survival and Bayesian calibration, extreme-value inference,
tail dependence, sensitivities, decision methods, semi-Markov models and treaty reinstatements.

## Start with an executable workbook

```sh
cargo run -p gridcraft-cli -- run --script docs/examples/actuarial.jsonl --out actuarial.xlsx --quiet
cargo run -p gridcraft-cli -- functions --search ACT.
cargo run -p gridcraft-cli -- eval '=ACT.DISCOUNT(0.05,10)'
```

The original, synthetic template has five sheets: Loss (compound loss and tail measures),
Life (mortality, annuity, premium and reserve), Reserves (Chain Ladder/BF/Mack), Markov
(active/disabled/dead and continuous-time mortality), Pricing (Poisson GLM). These are
teaching inputs, not official mortality, loss or regulatory data. The engine integration
test executes this exact script, inspects every cell for errors, saves/reopens XLSX and
checks recalculation, undo and Monte Carlo compatibility.

Strict numerical vectors preserve alignment: blank, text and boolean entries do not silently
disappear from mortality ages, responses, weights or cash flows. Invalid shape gives a cell
error; nonfinite/domain/resource/convergence failures give errors rather than plausible defaults.

Optional ACT arguments use their documented defaults when absent or skipped as empty literal
slots, for example `ACT.ANNUITY(qx,rate,,0)`. An explicit zero remains a supplied zero.
References to blank cells remain supplied references: scalar coercion can produce zero,
while strict vector/matrix contracts can reject them. This does not change coercion for
existing Excel-compatible functions outside the ACT extension.

## Financial curves, life and pensions (15 functions)

All interest rates are effective per period. Life periods are annual; `qx[0]` is the death
probability in the first year from attained age. Rates must exceed −1. The user supplies
mortality and its provenance. Terms/deferment/elapsed durations are exact nonnegative integers.

| Function | Contract |
|---|---|
| `ACT.DISCOUNT(rate,time)` | `(1+rate)^(-time)`; fractional nonnegative time allowed. |
| `ACT.CURVEPV(cashflows,times,spot_rates)` | Aligned cash flows/times and a scalar or aligned effective spot-rate vector; compensated discounted sum. |
| `ACT.CURVEDURATION(cashflows,times,spot_rates)` | PV-weighted payment time; zero PV is undefined. |
| `ACT.CURVECONVEXITY(cashflows,times,spot_rates)` | Normalized second derivative under parallel additive effective-rate shifts: `sum[t(t+1)PV/(1+r)^2]/PV`. |
| `ACT.SURVIVAL(qx,term,[start=0])` | Conditional product of annual survival probabilities over the supplied window. |
| `ACT.DEATH(qx,term,[start=0])` | Complement of survival using `expm1` for small death probabilities. |
| `ACT.LIFEEXPECTANCY(qx,[udd=0],[term])` | Restricted curtate expectation; UDD=1 integrates piecewise-linear survival, giving `E[min(T,term)]`. |
| `ACT.ANNUITY(qx,rate,[term],[due=1],[defer=0],[growth=0])` | Unit life payments from deferment (due) or deferment+1 (immediate); first payment 1. |
| `ACT.INSURANCE(qx,rate,[term],[endowment=0],[defer=0])` | Unit death benefit at end of year of death during covered term; optional survival benefit at maturity. |
| `ACT.PREMIUM(qx,rate,term,[premium_term=term],[endowment=0])` | Level equivalence net premium, paid in advance while alive. |
| `ACT.RESERVE(qx,rate,term,elapsed,premium,[endowment=0],[premium_term=term])` | Prospective value conditional alive at elapsed, immediately before that period's premium/maturity. |
| `ACT.JOINTANNUITY(qx1,qx2,rate,term,[last_survivor=0],[due=1],[defer=0])` | Independent lives: survival `p1*p2` or `p1+p2-p1*p2`. |
| `ACT.MORTALITYIMPROVE(qx,improvement,years)` | `qx*(1-improvement)^years`, including the final mortality entry. |
| `ACT.GOMPERTZSURVIVAL(makeham,gompertz,base,age,term)` | Analytic survival for force `makeham+gompertz*base^age`, including constant-force limit. |
| `ACT.PENSION(qx,rate,defer,salary,accrual,service,[salary_growth=0],[pension_growth=0],[due=1])` | Projected salary × accrued service × accrual × deferred annuity. No future service accrual or accounting claim. |

No terminal death is invented. Omitting an annuity/insurance term requires final qx=1;
otherwise supply a finite term. Expectancy is explicitly restricted unless terminal mortality
closes the lifetime. Deferred values include survival and discounting from time zero.
Endowment maturity reserve is 1 before payment, 0 for a pure death benefit. Vectors and
horizons are bounded at 10,000. Signed financial cash flows are supported; log survival,
`ln1p`/`expm1` and compensated sums limit cancellation.

## Losses, reinsurance and economic risk (19 functions)

| Function | Contract |
|---|---|
| `ACT.AGGREGATE.MEAN(count_mean,severity_mean)` | `E[N]E[X]`. |
| `ACT.AGGREGATE.VARIANCE(count_mean,count_variance,severity_mean,severity_variance)` | `E[N]Var[X]+Var[N]E[X]^2`; independent count and iid severities. |
| `ACT.VAR(losses,alpha,[weights])` | Left-inverse quantile of a complete empirical distribution; 0=min support, 1=max. |
| `ACT.TVAR(losses,alpha,[weights])` | Integrated upper quantile for `0≤alpha<1`, allocating fractional mass at the threshold. |
| `ACT.LAYER(loss,deductible,limit,[share=1])` | `share*min(max(loss-deductible,0),limit)`; per supplied loss, without implicit aggregation. |
| `ACT.QUOTASHARE(loss,ceded_share)` | Ceded payment; retained = loss minus ceded. |
| `ACT.STOPLOSS(losses,attachment,[weights],[limit])` | Expected layer payment over supplied aggregate-loss observations. |
| `ACT.LEV.EXPONENTIAL(limit,mean)` | Expected capped exponential loss. |
| `ACT.LEV.LOGNORMAL(limit,mu,sigma)` | Expected capped loss; parameters describe **log loss**. |
| `ACT.LEV.PARETO(limit,shape,scale)` | Expected capped Pareto **II/Lomax** loss, survival `(1+x/scale)^(-shape)`. |
| `ACT.LEV.GAMMA(limit,shape,scale)` | Expected capped Gamma loss; shape/scale parameterization. |
| `ACT.COMPOUND.PMF(severity_pmf,model,param1,param2,max_index,[unit=1])` | Compound-discrete PMF; layout below. |
| `ACT.COMPOUND.VAR(severity_pmf,model,param1,param2,max_index,alpha,[unit=1])` | Exact lattice quantile; grid must reach alpha. |
| `ACT.COMPOUND.TVAR(severity_pmf,model,param1,param2,max_index,alpha,[unit=1])` | Positive upper-tail sum with controlled omitted first moment; initial grid must reach VaR. |
| `ACT.CAPITAL.SD(standard_deviations,correlation)` | `sqrt(s'Rs)` with PSD validation; standard deviation, not regulatory capital. |
| `ACT.CAPITAL.ALLOCATION(standard_deviations,correlation)` | Column of Euler SD contributions, possibly negative for hedges. |
| `ACT.VAR.NORMAL(mean,standard_deviation,alpha)` | Analytic quantile under an explicit normal-loss assumption. |
| `ACT.TVAR.NORMAL(mean,standard_deviation,alpha)` | Normal upper-tail expectation; alpha=0 is mean, alpha=1 undefined. |
| `ACT.RUIN.EXPONENTIAL(capital,claim_rate,claim_mean,premium_rate)` | Ultimate ruin under the classical homogeneous Poisson/exponential Cramér–Lundberg model, constant premium inflow and no investment. |

Empirical weights are nonnegative and normalized after safe scaling; zero-weight observations
do not affect support endpoints. Signed losses are allowed for empirical portfolio P&L.
Example: masses 0.95 at 0 and 0.05 at 100 give VaR(0.95)=0 and TVaR(0.95)=100.
This differs from interpolated `PERCENTILE.INC` and from `E[L|L>VaR]`.

Compound severity is a **complete** probability vector at lattice points `0,unit,2*unit,…`,
sum 1 within 1e-12. Poisson uses `(lambda,0)`; binomial `(integer trials,p)`;
negativebinomial `(r,p)` counts failures before r successes, mean `r(1-p)/p`.
Positive Panjer recursion handles Poisson/negative binomial; positive convolution handles
binomial without the instability of its signed recursion. There is no random-number generator.
The spill has `max_index+1` rows and four columns: loss, PMF, CDF, omitted upper-tail mass
(the same omitted total repeated). The represented grid is **never renormalized** to one.
Do not feed a truncated PMF into empirical `ACT.VAR/TVAR`: use the compound functions.
Compound TVaR sums positive upper probabilities, avoiding subtraction of nearly equal moments.
For lattice aggregate Z, the omitted first moment is bounded by
`exp(-t*(K+1))*H'(t)*G'(H(t))`, where H is the severity MGF and G the count PGF.
Internal grid extension (at most 4,096, cumulative work at most 20 million terms) must make
the bound divided by `(1-alpha)` no larger than `1e-10*E[Z]`, or the function returns #NUM.
This controls truncation relative to TVaR, not every floating-point recurrence error.
Finite fully represented support needs no residual bound; alpha=0 uses the analytic mean.
The PMF's rounded `1-CDF` diagnostic is not proof that an extreme tail is absent.
Alpha=1 quantile requires fully represented finite support.

Limits: 100,000 empirical observations; 4,096 grid index; 20 million compound recursion/
convolution terms; 64 risks. PSD tolerance 1e-10 admits singular ±1 correlations but rejects
indefinite matrices. Exactly hedged nonzero-risk SD allocations are undefined. An underflowed
Panjer starting probability is rejected, never interpreted as a zero-risk result.
For Gamma limited means, positive caps require shape in [1e-4,1e6], finite mean and a
representable positive cap/scale ratio; numerically unsupported cases return #NUM.

## Reserving, credibility and pricing (10 functions)

| Function | Contract / spill |
|---|---|
| `ACT.CHAINLADDER.FACTORS(triangle)` | One row of volume-weighted cumulative link ratios. |
| `ACT.CHAINLADDER(triangle,[tail_factor=1])` | n×3: latest cumulative, ultimate, reserve. |
| `ACT.BORNHUETTERFERGUSON(triangle,expected_ultimate,[tail_factor=1])` | n×3: latest, BF ultimate, reserve. Expected ultimate aligned by origin. |
| `ACT.MACK(triangle,[last_sigma2])` | (n+1)×6: latest, ultimate, reserve, process SE, parameter SE, total SE. Final row is total, including shared-factor covariance. |
| `ACT.CREDIBILITY(exposure,epv,vhm)` | Bühlmann weight `w/(w+EPV/VHM)` with defined zero boundaries. |
| `ACT.BUHLMANNSTRAUB(experience,exposure)` | Group rows: mean, exposure, Z, credibility mean, EPV, VHM. |
| `ACT.GROSSPREMIUM(pure_premium,fixed_expense,expense_ratio,profit_ratio)` | `(pure+fixed)/(1-expense_ratio-profit_ratio)`; positive denominator. |
| `ACT.GLM.FIT(y,x,family,[offset],[weights])` | Coefficient column; family `poisson` or `gamma`, log link. Supply intercept column in x. |
| `ACT.GLM.PREDICT(x,coefficients,[offset])` | Column `exp(X beta + offset)`; exposure enters as log exposure. |
| `ACT.GLM.DEVIANCE(y,mean,family,[weights])` | Weighted unit deviance; no standard-error/dispersion estimate implied. |

Triangles are square, upper-left observed: row i contains n−i numeric observations and
truly blank lower-right cells. Missing claims are not zero. At most 128 origins; observed
claims are nonnegative. BF additionally requires link ratios≥1. Mack requires positive data,
alpha=1 and independent origins, with no tail uncertainty. Automatic final variance uses
`min(previous²/before,previous,before)` for n≥4. With 2/3 origins, an explicit nonnegative
`last_sigma2` assumption is required. Individual SEs must not be added as a total SE.

Bühlmann–Straub uses weighted within-group EPV and between-group VHM moment estimates;
negative estimated VHM is truncated to zero explicitly. The input is a fully observed
rectangular group-by-period table with strictly positive exposure in every cell and
estimable degrees of freedom; zero-exposure periods are rejected rather than omitted.

GLM uses bounded IRLS, reorthogonalized scaled weighted QR and step-halving. No hidden ridge,
dropped column or silent successful nonconvergence. Singular/nonconverged/overflowed fits
return errors. Gamma responses must be positive; Poisson responses nonnegative. Prior weights
must be positive; offsets and weights are aligned vectors. At most 4,096 observations,
16 regressors, 100 iterations, 30 line-search halvings. GLM output is a fit, not an automatic
model selection, fairness, validation or calibration claim.

## Markov / multi-state models (8 functions)

Matrices use rows as origin states. Distributions are row vectors. The eight foundational
functions below model finite **homogeneous** Markov chains; the separate
[calendar/semi-Markov interfaces](actuarial-scientific.md#calendar-and-duration-dependent-states-5-functions)
provide the documented time/duration dependence.

| Function | Contract / spill |
|---|---|
| `ACT.MARKOV.POWER(transition,periods)` | n×n matrix P^periods, exponentiation by squaring. |
| `ACT.MARKOV.STATE(initial,transition,periods)` | One-row distribution initial×P^periods. |
| `ACT.MARKOV.STATIONARY(transition)` | Column of unique stationary probabilities; nonunique/ill-conditioned cases rejected. Periodicity does not prevent an equilibrium but may prevent convergence toward it. |
| `ACT.MARKOV.ABSORPTION(transition,transient_states)` | Transient rows: expected absorption steps, then probability of absorption in each terminal state. Transient states first; terminal rows must be identity rows. |
| `ACT.MARKOV.REWARD(transition,state_rewards,rate,periods,[transition_benefits])` | Column of prospective values by initial state: state payments at each period start, transition payments at period end. Zero terminal value. |
| `ACT.MARKOV.FIT(transition_counts)` | Empirical row-normalized counts; zero-count row returns error rather than fabricated transitions. |
| `ACT.CTMC.TRANSITION(generator,time)` | Continuous-time exp(time×Q), bounded uniformization with scaling/squaring. |
| `ACT.CTMC.FIT(transition_counts,state_exposures)` | Generator MLE from off-diagonal counts and positive time at risk; diagonal counts must be zero. |

Reward recursion: `V_k = state_reward + v*(sum_j P_ij*b_ij + P*V_(k+1))`, terminal V=0.
Absorption solves `(I−T)[t,B]=[1,R]` with residual checks. Stationarity solves pi*P=pi,
sum(pi)=1, checking uniqueness and residuals. Continuous generators have nonnegative
off-diagonal entries and zero row sums; rates/time must share units. Uniformization uses
`lambda=max(-q_ii)`, `R=I+Q/lambda` and the Poisson series; its geometric remainder is bounded.
Long horizons use at most 40 squarings and final probability checks; ill-conditioned or
numerically unresolved problems return #NUM. No probability simulation is introduced.
Limits: 32 states; discrete powers up to 10^9; reward horizon up to 10,000 periods.
Input row sums within 1e-12 of one are normalized to remove decimal roundoff; probability
matrix products are also normalized within a tight 1e-10 mass-error bound, preventing
roundoff amplification at large discrete powers. Larger errors are rejected, not repaired.
CTMC estimation assumes fully observed transitions and exposure, not panel/interval-censored
observations. DTMC estimated rows are MLE point estimates, without posterior intervals.

## Reusing Monte Carlo

The [existing MC command](monte-carlo.md) can propagate uncertain inputs through any numeric
actuarial workbook output. Example after creating the template:

```json
{"inputs":[{"cell":"Loss!B2","distribution":"uniform","min":8,"max":12}],"outputs":["Loss!B6","Loss!B10"],"iterations":1024,"seed":42,"method":"latinHypercube","replicates":8,"newSheet":"Actuarial uncertainty"}
```

Send as params to `data.monteCarlo` through `execute_command` or CLI. This simulates uncertainty
in expected frequency and its impact on expected loss, **not** realized Poisson claim counts.
MC marginal distributions remain uniform/triangular/normal and its sampler/RNG remain private.
No new progress/cancellation callbacks or discrete MC marginals are claimed by this pack.

## Scope and evidence

See [reproducible validation evidence](actuarial-validation.md), including the scientific
replay script and independently calculated engine-boundary reference cases.

Implemented methods include financial curves; supplied mortality/life/pensions; aggregate
losses; GLM/credibility pricing; CL/BF/Mack; layers/quota share; covariance/economic risk;
DTMC/CTMC rewards and the [43 scientific-method extensions](actuarial-scientific.md).
The latter include right-censored and left-truncated calibration, conjugate Bayesian
predictives, calendar/duration-dependent transitions, GPD inference and tail copulas,
forward-dual derivatives, finite-candidate reinsurance optimization, local two-instrument ALM,
validated polynomial proxies, ordered XOL reinstatements and explicit-uniform ODP reserve bootstrap.

Implementation is distinct from validation. Analytic/regression fixtures are present in
the modules; executed checks and the separately authorized mathematical review are reported
by the coordinating session. No empirical field calibration, interval-coverage experiment,
cross-platform benchmark or professional certification is implied by a registered function.
Callers must supply defensible datasets, independent validation scenarios and assumptions.

The explicit scope does not include hierarchical MCMC,
multivariate vine fitting, policy administration, official assumption feeds, general
investment-strategy optimization or regulatory/accounting rule engines. The implemented
proxy is a component for nested-stochastic workflows, not a generator of inner valuations.

## Mathematical references

Original implementations use published mathematics, not copied package implementation code.

- [SOA, multi-state transition models](https://www.soa.org/493553/globalassets/assets/files/edu/m-24-05.pdf).
- [Columbia, continuous-time chains and uniformization](https://www.columbia.edu/~ww2040/3106F11/CTMCchapter121906.pdf).
- [Mack, Measuring the Variability of Chain Ladder Reserve Estimates](https://www.casact.org/sites/default/files/2021-03/7_Mack_1994.pdf).
- [Life reserve recursion](https://actuarialmath-guide.readthedocs.io/en/latest/reserves.html).
- [Actuar risk-theory vignette, Panjer mathematics](https://stat.ethz.ch/CRAN/web/packages/actuar/vignettes/risk.pdf).
- [Statsmodels, GLM families and links](https://www.statsmodels.org/stable/glm.html).
- [Actuarial model suitability, uncertainty and validation](https://actuary.org/resources/modeling/).

## Cooperative work accounting

The worksheet library exposes the host-neutral `Ctx::charge_work(units)` checkpoint.
Its default accepts work for compatibility; embedding hosts opt into a shared bank and
cancellation. No dependency from the functions layer to the engine is introduced.

Every ACT registration checks this hook before parsing, copying input vectors or entering
its numerical kernel. Estimates depend on input sizes, matrix dimensions and model horizons;
saturated arithmetic prevents large dimensions from wrapping to a small charge. Closed copula
formulas use smaller reserves than numerical quadrature or inverse searches. Adaptive
GLM/Cox iterations, bootstrap replications/process draws and compound-tail extensions also
charge before their next expensive block. Generic calls reserve input/output work and check
each lifted/broadcast kernel, so many small invocations cannot bypass a host's shared bank.

Units are conservative operational estimates, not CPU instruction counts, elapsed-time
predictions or a real-time responsiveness guarantee. Existing per-model allocation/domain
caps apply even when the host accepts every charge. A refused charge returns `#NUM!`; an
analysis host must also retain the failure outside formula evaluation so `IFERROR` cannot
turn exhaustion or cancellation into a publishable result.

Cancellation is cooperative at these checkpoints. A native indivisible operation must
return to its next checkpoint; a browser main thread must yield before it can receive a
new UI cancellation event. The incremental Monte Carlo lifecycle and atomic publication
boundary are engine responsibilities, validated separately from these worksheet contracts.
