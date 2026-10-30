# Scientific actuarial methods

> Updated: 2026-10-28. Contracts for 43 extensions beyond the
> [52 foundational actuarial functions](actuarial.md). Implementations are original,
> deterministic Rust at L1. Numerical checks and independent review are separate evidence;
> this document does not itself certify scientific adequacy or regulatory compliance.

The methods compose in worksheet, CLI and MCP workflows. They use evaluated numerical
ranges, preserve aligned observations and return cell errors for unsupported domains,
unidentifiable fits, failed convergence or resource limits. They add no RNG or Monte Carlo
marginals. The existing [Monte Carlo contract](monte-carlo.md) remains unchanged.

## Survival and competing-risk calibration (7 functions)

Individual survival intervals are `(entry,exit]`. Exit times must be positive, entry must
be nonnegative and earlier than exit. Event and censoring observations tied at exit are
both in the risk set; entry at an event time enters afterward. These methods assume the
specified independent censoring/truncation mechanism. They do not handle interval-censored
observations, clustered robust covariance, or unmodeled informative censoring.

| Signature | Result and estimand |
|---|---|
| `ACT.KAPLANMEIER(times,event,[entry],[confidence=.95])` | Unique exit-time rows: time, at risk, deaths, censored, survival, Greenwood SE, lower/upper log-log interval. Event 0=censored, 1=death. |
| `ACT.AALENJOHANSEN(times,cause,[entry])` | Unique-time rows: time, risk, all events, censored, overall survival, CIF for each cause 1..max(cause); 0=censored. At least one cause is needed to infer columns. |
| `ACT.AALENJOHANSEN.MATRIX(risk_sets,transition_counts,[initial])` | Full-state product integral from sufficient event-time risk/count tables. Rows: step index, then state probabilities. |
| `ACT.COX.FIT(times,event,x,[entry])` | Column of Breslow partial-likelihood coefficients. No intercept is added; an unidentifiable intercept/design returns an error. |
| `ACT.COX.BASELINE(times,event,x,coefficients,[entry])` | Event-time rows: time, baseline hazard increment, cumulative baseline hazard; does not refit supplied coefficients. |
| `ACT.COX.COVARIANCE(times,event,x,coefficients,[entry])` | Inverse observed partial-likelihood information, p×p model-based covariance; not robust sandwich covariance. |
| `ACT.COX.SURVIVAL(x,coefficients,baseline_cumulative_hazard)` | Subject rows × ordered baseline-hazard columns: `exp(-H0*exp(X beta))`. |

KM log-log confidence bounds are blank at survival 0/1, where that transformation is undefined.
Point survival and plug-in Greenwood SE remain numerical; zero plug-in SE is not a coverage
guarantee. Aalen–Johansen cause incidence uses overall pre-event survival;
it is not a cause-specific KM estimate. No CIF confidence intervals are returned.

For the matrix product integral, risk sets are T×n and transition counts are `(T*n)×n`,
stacking one origin-by-destination block per ordered event time. Counts are nonnegative
integers, diagonals zero and row event totals no greater than the corresponding risk set.
Default initial distribution is `(1,0,…)`. A zero-risk/no-event row has zero hazard increment;
this convention does not provide evidence for extrapolation. Limits: 16 states/causes and
4,096 general records/steps.

Cox uses Newton updates, step halving and diagonal-scaled Cholesky information solves.
No hidden ridge, dropped covariate or successful nonconvergence is allowed. A converged
fit requires a normalized full Newton step below 1e-8 and score below
`1e-7*(1+event_count)`. Limits: 256 subjects, eight covariates, 60 updates and 30 step halvings.
No-event, separated, rank-deficient or numerically unresolved fits return errors.
Center/scale covariates to ordinary numerical units before fitting. Extreme units can
make the score criterion unresolvable and produce #NUM rather than a false coefficient.
There are no strata or time-dependent covariates. Survival predictions cap output at
65,536 cells and use supplied baseline hazards; they do not verify that those hazards
were fitted to the same model.

## Bayesian parameter and predictive uncertainty (4 functions)

| Signature | Result |
|---|---|
| `ACT.BAYES.POISSON(counts,exposures,prior_shape,prior_rate,[confidence=.95],[future_exposure=1])` | One row: posterior Gamma shape, rate, mean, variance, equal-tail credible low/high, predictive count mean/variance. |
| `ACT.BAYES.BINOMIAL(successes,trials,prior_alpha,prior_beta,[confidence=.95],[future_trials=1])` | One row: posterior Beta a, b, mean, variance, equal-tail credible low/high, predictive count mean/variance. |
| `ACT.BAYES.POISSON.PREDICT(counts,exposures,prior_shape,prior_rate,future_count,[future_exposure=1],[cumulative=0])` | Gamma–Poisson predictive PMF, or CDF when cumulative=1. |
| `ACT.BAYES.BINOMIAL.PREDICT(successes,trials,prior_alpha,prior_beta,future_successes,future_trials,[cumulative=0])` | Beta–binomial predictive PMF/CDF. |

Gamma uses **shape/rate**, not scale: `a'=a+sum(counts)`, `b'=b+sum(exposure)`.
For future exposure t, predictive variance is `E[lambda]*t + Var[lambda]*t²`.
Beta updates are `a'=a+sum(successes)`, `b'=b+sum(trials-successes)`;
predictive count variance is `n*p*(1-p)*(a'+b'+n)/(a'+b'+1)`.
Thus parameter uncertainty is propagated rather than replaced by a plug-in estimate.
Counts/trials are exact integers; zero Poisson exposure must accompany zero observed count.
Priors must be positive, confidence in (0,1), future exposure/trials nonnegative.

These are numerically evaluated exact conjugate distributions, not hierarchical inference
or MCMC. A credible interval for a parameter is distinct from a predictive count interval
and from a frequentist coverage claim. Posterior Gamma shape and Beta a/b are supported in
[0.001,10^6]; quantile inversion is bounded and checks relative tail residual ≤2e-9.
Gamma upper quantiles invert the complementary Q tail directly, without forming `1-tail`.
Beta upper quantiles use parameter symmetry and recheck the reflected representable endpoint.
Unsupported extreme
tails return errors. Per-observation counts/trials are capped at 10^6; beta-binomial predictive
trials at 4,096. Zero future exposure produces a count-zero point mass.

## Supplied-prediction validation (2 functions)

| Signature | Result |
|---|---|
| `ACT.CALIBRATION(observed,predicted,[weights])` | One row: n, weighted observed/predicted totals, observed:expected ratio, weighted bias (predicted−observed), MAE, RMSE, Poisson deviance. |
| `ACT.BRIER.IPCW(times,event,predicted_survival,horizon)` | Right-censored single-event Brier score with reverse-KM censoring weights. |

Calibration accepts observed≥0, predicted>0 and nonnegative weights with positive total.
It does not create a train/test split: callers must supply genuinely held-out predictions
and preserve their temporal/cohort provenance. IPCW is an unconditional independent-censoring
estimator with no delayed entry. For event by h it uses `S_i(h)^2/G(T_i−)`; for follow-up
beyond h, `(1−S_i(h))²/G(h)`; earlier censored observations contribute zero. Divide by the
original sample size. Event/censor ties follow `event time≤censor time`: failures occur just
before censorings in reverse-KM, so its censoring hazard is `censors/(risk−failures)`.
Case weights retain `G(T−)`; controls use `G(h)`. Loss of censoring support, `G(h)≤0`, returns an error. All predictions
must refer to the same supplied horizon. Computed diagnostics do not establish field validity,
correct censoring assumptions, interval coverage or absence of dataset leakage.

## Extreme-value loss inference (9 functions)

The Generalized Pareto distribution models nonnegative **excesses** with shape xi and
scale beta>0. Its survival is `(1+xi*x/beta)^(-1/xi)`, with the exponential limit at xi=0.
Negative shape has finite endpoint `-beta/xi`.

| Signature | Contract |
|---|---|
| `ACT.GPD.CDF(excess,shape,scale)` | CDF 0 for nonpositive excess; 1 at/beyond a finite endpoint. |
| `ACT.GPD.INV(probability,shape,scale)` | Excess quantile; probability 1 is finite only for negative shape. |
| `ACT.GPD.MEAN(shape,scale)` | `beta/(1−xi)` for xi<1; undefined/infinite moments return #NUM. |
| `ACT.GPD.TVAR(alpha,shape,scale)` | Excess tail mean `(quantile+beta)/(1−xi)` for alpha<1 and xi<1. |
| `ACT.POT.VAR(threshold,exceedance_probability,shape,scale,alpha)` | Full-portfolio tail quantile above threshold. |
| `ACT.POT.TVAR(threshold,exceedance_probability,shape,scale,alpha)` | Full-portfolio upper-tail mean above threshold. |
| `ACT.GPD.FIT.PWM(excesses)` | Row: shape, scale, n, log likelihood, shape SE, scale SE, covariance. |
| `ACT.GPD.FIT.MLE(excesses)` | Same schema for a constrained regular-region profile-likelihood fit. |
| `ACT.GPD.THRESHOLDS(losses,thresholds)` | Threshold rows: threshold, strict exceedance count, empirical exceedance fraction, mean excess, PWM shape/scale. |

POT maps confidence to excess probability `p=1−(1−alpha)/p_exceedance` and adds the threshold.
It requires positive exceedance probability and `alpha≥1−p_exceedance`; a conditional-excess
quantile cannot silently stand in for a portfolio quantile. Supplied thresholds are diagnostics,
not an automatically justified threshold choice.

Fits use 8–2,048 nonnegative, nonconstant observations. PWM uses unbiased empirical probability
weighted moments. Its delete-one jackknife SE/covariance cells are empty when fitted xi≥0.5,
where the finite-variance justification used here is unavailable. Empty uncertainty is not zero.
MLE explicitly searches xi in (−0.49,2), profiles bounded log scale, uses a 32-point shape grid
and bounded 72-step inner/outer refinements, and refuses boundary/unsupported fits. It checks
the score and positive observed negative Hessian before inverting information. Returned SEs are
local model-based observed-information estimates, not a global optimality or finite-sample
coverage guarantee. No ridge is applied. Threshold diagnostics accept at most 32 thresholds,
each needing eight strict exceedances.

## Bivariate dependence and tail transformations (6 functions)

| Signature | Contract |
|---|---|
| `ACT.COPULA.CDF(u,v,family,parameter,[df=4])` | Bivariate copula CDF with exact uniform-margin endpoints. |
| `ACT.COPULA.JOINTTAIL(u,v,family,parameter,[df])` | Survival copula, evaluated with positive/complementary-margin formulas by family; unresolved probabilities return errors instead of false zeros. |
| `ACT.COPULA.CONDITIONALINV(u,z,family,parameter,[df])` | Inverse Rosenblatt conditional margin from externally supplied uniforms; u interior. |
| `ACT.COPULA.TAIL(family,parameter,[df])` | Row of lower/upper asymptotic tail-dependence coefficients. |
| `ACT.KENDALL.TAU(x,y)` | Tie-adjusted Kendall tau-b for paired observations. |
| `ACT.COPULA.FIT(x,y,family)` | Row: empirical tau, concordance-inverted parameter, n. |

Supported families and domains: independence (parameter 0); Gaussian/normal rho∈[−1,1];
Student/t rho∈[−1,1] and df∈[2,200]; positive Clayton theta∈[0,10,000] (0 independence);
Gumbel theta∈[1,10,000] (1 independence); Frank theta∈[−35,35] (0 independence).
Rotated Archimedean families and multivariate vines are not fitted.

Gaussian CDF uses a correlation integral at ordinary probabilities and a positive conditional
integral below 1e-8. Student uses the actual conditional Student-t law: rho=0 does not make
it independent. Positive elliptical integration normalizes by the smaller margin and uses
`s=margin*r^8`; discarded normalized mass is at most 1e-24. Relative refinement targets 1e-9
with stability checked within 1e-7; normalized probabilities below 1e-15 are unresolved.
Student interior probabilities must lie in [1e-12,1−1e-12], apart from exact boundaries/median
formulas. Adaptive quadrature is capped at 8,192 evaluations and depth 22; unresolved cases
return #NUM. Independent survival is a complementary product; Clayton/Gumbel use positive
`expm1` identities and Frank/elliptical families use stable radial symmetry. This prevents
inclusion/exclusion cancellation from silently turning tiny positive tails into zero.

Gaussian/Student conditional distributions use their analytic univariate inverses;
Archimedean conditional CDFs use 80 bounded monotone bisections. Singular rho=±1 has exact
deterministic limits. No random sampling is performed. Nonsingular Gaussian same-tail
dependence is zero; Student lower/upper coefficients are
`2*T_(df+1)(−sqrt((df+1)*(1−rho)/(1+rho)))`. Clayton lower coefficient is `2^(−1/theta)`,
Gumbel upper coefficient `2−2^(1/theta)` and Frank both zero. These are not correlations.

Kendall pair enumeration is capped at 2,048 observations/4,194,304 comparisons. Tau-b accounts
for ties, but `COPULA.FIT` explicitly refuses ties because its continuous-margin inversion
does not choose pseudo-ranks. Gaussian/Student rho=`sin(pi*tau/2)`; Clayton/Gumbel require
nonnegative tau. Student df is supplied separately: concordance does not identify it.
The independence fit reports empirical tau and fixed parameter 0, not a goodness-of-fit pass.

## Calendar and duration-dependent states (5 functions)

| Signature | Result |
|---|---|
| `ACT.MARKOV.NONHOMOGENEOUS(calendar_blocks)` | Ordered product P0*P1*… of vertically stacked n×n matrices. |
| `ACT.MARKOV.NONHOMOGENEOUS.REWARD(calendar_blocks,state_payments,rate,[transition_benefits])` | Prospective-value column by initial state; period/state payments T×n. |
| `ACT.SEMIMARKOV.STATE(duration_blocks,initial,horizon,[initial_duration=0])` | State-distribution row at horizon. |
| `ACT.SEMIMARKOV.TRACE(duration_blocks,initial,horizon,[initial_duration=0])` | State probabilities at times 0..horizon, including the initial distribution. |
| `ACT.SEMIMARKOV.REWARD(duration_blocks,state_rewards,rate,horizon,[initial_duration=0],[transition_benefits])` | Prospective-value column by initial state and specified initial tenure. |

Blocks have n columns and a multiple of n rows. Calendar blocks are ordered by period;
duration blocks are ordered by completed periods in the current state, beginning at duration 0.
Staying in the same state increments duration; changing state resets duration to zero.
The initial tenure is a single nonnegative integer shared by the initial state distribution.
The duration table must cover every possible path: at least initial_duration+horizon blocks.
There is no terminal-duration saturation or extrapolation. Calendar and duration dependence
are separate interfaces, not a general jointly calendar×duration calibrated tensor model.

State payments occur in advance, transition benefits at period end, zero terminal value;
effective discount rate must exceed −1 and its period must match the transition period.
Optional transition benefits are n×n and constant across periods/durations. Limits: 32 states,
1,000-period horizon, at most 1,001 blocks and 20 million bounded matrix/recursion terms.
Probability normalization only removes accepted numerical roundoff, as in the foundational
Markov functions. Constant duration blocks recover the corresponding homogeneous Markov model.

## Contract reinstatements and surplus share (2 functions)

| Signature | Contract |
|---|---|
| `ACT.REINSURANCE.REINSTATEMENTS(losses,deductible,limit,reinstatements,base_premium,[reinstatement_rate=1],[times],[time_prorata=0])` | Ordered occurrence XOL claim cash flows with finite paid-reinstatement capacity. |
| `ACT.REINSURANCE.SURPLUS(sum_insured,retention,lines,loss)` | Ceded share = `min(max(sum_insured−retention,0),retention*lines)/sum_insured`; apply to loss. |

For reinstatements, every claim row has six columns: gross loss, ceded recovery, retained
loss, amount reinstated, additional reinstatement premium, available limit after reinstatement.
The final summary row has gross/ceded/retained totals, total reinstated amount,
**base premium plus all reinstatement premiums**, and final available limit. Therefore the
premium column in the final row differs from the claim-row incremental-premium convention.

Initial available cover is limit. Each ordered claim recovers no more than its occurrence
layer and current available cover, then restores up to the remaining `reinstatements*limit`
capacity. Additional premium is `base_premium*reinstatement_rate*(restored/limit)`, multiplied
by `1−time` when time_prorata=1. Restoration after the final claim is still charged by this
automatic contractual convention. Limit must be positive, reinstatements an integer 0..100,
and all monetary inputs nonnegative. Supplied claim times must be nondecreasing in [0,1];
they are required for time-prorated premiums. At most 100,000 claims. This is the specified
amount/time-pro-rata contract, not every possible treaty wording or reinstatement convention.

## Differentiable values and constrained decisions (5 functions)

| Signature | Result |
|---|---|
| `ACT.LIFE.SENSITIVITY(qx,rate,term,[endowment=0])` | 2×3: insurance and annuity-due rows; columns value, rate derivative, mortality-scale derivative. |
| `ACT.PREMIUM.SENSITIVITY(qx,rate,term,[premium_term=term],[endowment=0])` | Row: equivalence premium, rate derivative, mortality-scale derivative. |
| `ACT.RESERVE.SENSITIVITY(qx,rate,term,elapsed,premium,[endowment=0],[premium_term=term])` | Row: prospective reserve and the two derivatives, conditional alive at elapsed. |
| `ACT.REINSURANCE.OPTIMIZE(losses,weights,deductibles,limits,loading,budget,alpha)` | Best candidate row: deductible, limit, expected ceded, premium, retained TVaR, total-cost TVaR, budget slack. |
| `ACT.ALM.IMMUNIZE(liability_pv,liability_duration,asset_pvs,asset_durations,[allow_short=0])` | Two-instrument units matching liability PV and dollar duration. |

Sensitivities use actual forward-mode dual arithmetic, not finite differences. Mortality
derivative means a **common multiplicative qx scale evaluated at 1**, not independent age
gradients. Covered qx must lie in [0,1) to support a two-sided admissible scale neighborhood;
terminal qx=1 included in the covered term is rejected. Life terms/vectors are capped at
10,000. Death benefits are paid at end of death year, premiums/annuity in advance. The reserve
derivative holds the supplied contractual premium fixed; it does not reprice the contract.

Treaty optimization minimizes `TVaR(retained)+(1+loading)*E[ceded]` subject to premium≤budget.
It is globally exhaustive **over supplied finite deductible×limit candidates**, not over all
possible treaty designs. Equal objective values keep the first candidate in supplied order.
No feasible candidate returns #NUM; include limit 0 if no treaty is an admissible choice.
Inputs are nonnegative, weights normalized and alpha in [0,1). Limits: 4,096 losses,
128 values per candidate vector and 20 million loss×candidate operations.

ALM requires two positive instrument PVs and nonnegative liability PV/durations using the
same duration/rate convention. Equal/near-equal instrument durations are rejected; negative
units require explicit allow_short=1. This is local first-order immunization, not convexity
matching, stochastic dynamic ALM or general optimal investment allocation.

## Independently held-out polynomial proxy (2 functions)

| Signature | Contract |
|---|---|
| `ACT.SURROGATE.FIT(train_x,train_y,validation_x,validation_y,[degree=2],[tail_alpha=.95])` | Versioned numeric model with coefficients, training domain and holdout diagnostics. |
| `ACT.SURROGATE.PREDICT(model,x,max_validation_rmse)` | Prediction column, rejecting domain extrapolation or a model whose recorded validation RMSE exceeds the explicit tolerance. |

The proxy is a multivariate total-degree polynomial with centered/scaled inputs, response
scaling and reorthogonalized QR. It adds no ridge or silently removed column. Degree 0..3,
one to six inputs, at most 64 features and 4,096 training/validation rows; `n*p²≤20 million`.
Validation coordinates must be disjoint from training coordinates (exact numeric equality,
±0 unified) and within the training rectangular bounds. These checks guard direct scenario
reuse, not every form of data leakage or statistical dependence.

The model has four columns and `max(feature_count,input_count,7)+1` rows. Its header is
`[version=1,input_count,degree,feature_count]`. Subsequent rows hold:

- Column 0, rows 1..p: coefficients of scaled-power monomials.
- Columns 1/2, rows 1..d: training coordinate minima/maxima.
- Column 3, rows 1..7: training count, validation count, training RMSE, validation RMSE,
  validation maximum absolute error, validation tail RMSE, tail alpha. Other cells are zero.

Monomials are ordered by total degree, then recursively by exponent of input 0 from zero
upward (for degree 1/dimension 2: intercept,z2,z1). Standardization is
`z=(x−center)/radius`, with center/radius from training bounds. Degree 0 permits constant
input bounds; otherwise a singular/constant design returns an error. Tail RMSE conditions
on validation **actual responses** at/above their left-inverse empirical alpha-quantile,
including ties; it is not CVaR. Prediction gates the overall held-out RMSE only: callers
must separately assess maximum/tail errors and intended use. The numeric model is editable,
not tamper-proof, and rectangular-domain inclusion alone does not establish proxy adequacy.

For nested-stochastic work, obtain training/validation targets from separately computed inner
valuations and predict appropriate outer scenarios. This module supplies the fitted proxy,
not inner-model simulation data, an automatic replacement policy or a universal speed/accuracy
claim. No unexecuted coverage experiment or external benchmark is claimed here.

## Residual bootstrap reserving (1 function)

`ACT.RESERVE.BOOTSTRAP.ODP(triangle,residual_uniforms,[process_uniforms])` implements a
centered, variance-calibrated Pearson-residual variation of the two-stage chain-ladder
bootstrap. All uniforms are explicit external inputs: no RNG or MC sampler is duplicated.
Independent IID uniforms are a modeling assumption, not guaranteed by numeric input validation.

For n origins, N=n(n+1)/2 observed cells and M=n(n−1)/2 future cells, residual inputs are
R×N and optional process inputs R×M; one row/column vector can represent one replication.
Inputs are in [0,1), with pool index floor(N*u). The output is R×(2n+3): n parameter-bootstrap
reserve means, total mean, n predictive reserves, total predictive reserve, original phi.
Without process inputs, predictive columns are **blank**, not fabricated predictive draws.

Fitted cumulative paths run backward from original latest observations; differences supply
incremental means. The original dispersion is `phi=sum(raw Pearson residual²)/(N−(2n−1))`.
The empirical residual pool is centered and scaled to variance phi before resampling. Each
pseudo-triangle is refitted and projected. Process draws use Gamma(mean/phi,scale=phi),
matching ODP first two moments but specifying a different predictive shape from scaled Poisson.
Phi is estimated once and held fixed, not itself resampled. Phi=0 gives a degenerate fitted
process, without implying real-world risk is absent. These are pseudo-triangle forecasts,
not a Bayesian posterior conditioned on every original observed value.

Triangles have 3–20 origins, positive first cumulative amounts, nonnegative increments,
factors greater than one and strictly blank unobserved cells. The **entire** centered pool
must imply nonnegative pseudo-increments for every fitted mean before any draw is used.
Unsupported experiments are rejected; no negative pseudo-value is clipped or secretly redrawn.
Degenerate cumulative amounts/development refits can still return errors. Callers must not
discard failed experiments and label the survivors unconditional.

Limits: 1,024 replications, R*n³≤2 million, at most 4,096 Gamma process draws. Supported
Gamma shapes are [0.05,1,000], with direct-tail inversion, 160 bounded iterations and
relative probability residual≤1e-9; unresolved quantiles return errors. Summary quantiles
require many replication rows, not one draw. For small triangles the existing MC command
can supply all seeded independent uniform cells: n=5 uses 15+10=25 inputs, within its 32-input
limit. Larger experiments require recorded external uniform matrices/batches and provenance.

Reference: [England (2002), bootstrap reserving, sections 2–3](https://www1feb-uva.nl/ke/act/people/kaas/Eng02.pdf).
The centering/variance calibration is explicit rather than silently equated with the paper's
uncentered degrees-of-freedom multiplier. No package implementation source was copied.

## Evidence and reference methods

Source modules contain analytic/regression fixtures for estimands and boundary behavior.
Their presence does not mean a suite has been executed or an independent review has passed.
The coordinating session records those outcomes, including failed/unsupported cases.
Model uncertainty, parameter uncertainty, process variation and Monte Carlo integration error
are different quantities; no single returned SE covers all of them.

Original mathematical implementations were guided by primary descriptions, not copied package
implementation code:

- [de Wreede et al., multi-state estimation and mstate](https://www.jstatsoft.org/article/view/v038i07).
- [Therneau et al., competing risks and product integrals](https://stat.ethz.ch/R-manual/R-devel/RHOME/library/survival/doc/compete.pdf).
- [Cox/Breslow conventions](https://stat.ethz.ch/R-manual/R-devel/library/survival/html/coxph.html) and [survival confidence transforms](https://stat.ethz.ch/R-manual/R-devel/library/survival/html/survfit_confint.html).
- [Stan, posterior prediction](https://mc-stan.org/docs/2_37/stan-users-guide/posterior-prediction.html).
- [Graf et al., censored prediction error](https://pubmed.ncbi.nlm.nih.gov/10474158/).
- [Satten–Datta, tied-event IPCW/Kaplan–Meier identity](https://pmc.ncbi.nlm.nih.gov/articles/PMC5568678/).
- [Hosking and Wallis, GPD parameter/quantile estimation](https://doi.org/10.1080/00401706.1987.10488243).
- [Demarta and McNeil, t copulas](https://www.planchet.net/EXT/ISFA/1226.nsf/0/303eb11b4d617b79c1257b0800744575/%24FILE/t%20copula%20demarta%20mcneil.pdf).
- [SAS, Student copula specification](https://support.sas.com/documentation/cdl/en/etsug/67525/HTML/default/etsug_copula_details06.htm).
- [Homescu, automatic differentiation in finance](https://arxiv.org/abs/1107.1831).
- [SOA, validated proxies for nested stochastic modeling](https://www.soa.org/resources/research-reports/2023/predictive-analytics-and-machine-learning/).

The supported methods cover the requested numerical workflow with explicit domains. They do
not amount to every actuarial method, an empirical validation of a particular portfolio,
hierarchical MCMC, a multivariate vine engine or a regulatory
filing system. Suitability for a dataset and decision remains an evidence-based modeling task.
