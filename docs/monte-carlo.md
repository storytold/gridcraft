# Monte Carlo simulation

> **Last reviewed:** 2026-10-29 · **Last updated:** 2026-10-29 · **Change:** bounded work policies and explicit UI/print rendering allowances

**Data → Monte Carlo** samples numeric input cells, recalculates the workbook's own formulas,
and creates a static report. The same command is available through CLI, the control channel,
and MCP's `execute_command`. It is built into the Rust engine; no Python, proprietary add-in,
or financial-model template is required.

## Configure a simulation

1. Add each uncertain input cell. Use a sheet-qualified reference for another sheet, such as
   `'Sales assumptions'!B2`. The input must be a numeric constant, not a formula.
2. Choose a distribution: **uniform** (minimum/maximum), **triangular**
   (minimum/mode/maximum), **normal** (mean/standard deviation), **lognormal**
   (arithmetic mean/CV), **Poisson** (mean), **binomial** (trials/probability), or
   **hypergeometric** (draws/successes/population). See [seeded worksheet processes](monte-carlo-processes.md)
   for conditional counts, stationary AR1, numerical limits and the explicit deterministic forecast guard.
3. Enter output cells, one per line. They may be on different sheets. Outputs must evaluate to
   finite numbers in every trial.
4. Choose random Monte Carlo or Latin Hypercube sampling (LHS), total iterations, and a
   non-negative seed. The dialog additionally shows independent replicas for LHS: the default
   is eight and the minimum is two. LHS iterations must be divisible by replica count, with at
   least two samples per replica. The random dialog uses IID trials without a replica-count
   restriction. Optionally omit the raw sample table.
5. Optionally enter a correlation matrix: rows on separate lines, numbers separated by spaces.
   The matrix follows input order and specifies **latent Gaussian correlation**, not the
   Pearson correlation of non-normal sampled variables.
6. Run. The dialog shows progress and **Cancel simulation** (also Escape). The source inputs
   remain unchanged; adding the completed report is one undoable action. Workbook editing is
   disabled while a job is pending. In action recording or an undo group the dialog retains
   the synchronous command behavior.

The report contains mean, sample standard deviation, minimum, P10/P50/P90, maximum, probability
above zero, raw samples when requested, and a conditional tornado for the **first output**.
It also reports Monte Carlo standard errors, approximate 95% intervals, convergence by completed
blocks/replicas, and requested versus observed input dependence. A second native chart shows
convergence of the first output mean and its sampling-error interval.

It also stores the canonical JSON configuration, with sheet-qualified references, so the model
can be run again after reopening the workbook. This is not a live recalculating report: rerun
after changing the model or the distributions.

## Programmatic example

Create a synthetic model, then run the command:

```sh
gridcraft-cli run \
  --cmd 'range.setValues={"range":"A1","values":[[100,20]]}' \
  --cmd 'cell.set={"cell":"C1","input":"=A1*B1-1500"}' \
  --cmd 'data.monteCarlo={"inputs":[{"cell":"Sheet1!A1","distribution":"triangular","min":50,"mode":100,"max":150},{"cell":"Sheet1!B1","distribution":"normal","mean":20,"stdDev":2}],"outputs":["Sheet1!C1"],"iterations":10000,"seed":42}' \
  --out risk.xlsx
```

In MCP, call `execute_command` with `command: "data.monteCarlo"` and the same JSON as `params`.
Omitting `newSheet` creates a unique `Monte Carlo` report name. An explicit existing or invalid
sheet name is rejected. Reuse the report's configuration, or record the action as an automation.

### Cooperative jobs over MCP, control or CLI

`data.monteCarlo` remains synchronous for existing callers. For explicit progress and cancellation,
use the following engine commands through the same `execute_command` interface:

1. `analysis.start` with `{"params":{...simulation configuration...}}` returns a positive `job`
   identity. Optional `autoAdvance: true` lets an attached UI drive it; the default is false.
2. `analysis.step` with `{"job":1,"trials":512}` advances work without publishing. The first
   step prepares samples; later steps evaluate up to 1–2048 trials, yielding between trials
   after approximately 8 ms or 250,000 charged units. Poll `analysis.status` with that identity
   for `stage`, `completed`, `total`, `progress`, `workUsed` and limits.
3. When `state` is `ready`, `analysis.finish` with `{"job":1}` publishes one report/undo step.
   An early finish returns an error and retains the job. The journal stores the canonical
   `data.monteCarlo` configuration, so replay does not depend on a transient job identity.
4. Alternatively, `analysis.cancel` with `{"job":1}` discards pending work without a report,
   journal entry or source change. Stale identities never control a newer job.

There is one pending job per session. Start outside nested scripts, action recording and undo
groups. Editing/switching commands are refused while it is pending; read-only queries remain
available. Source revision and workbook identity are checked again before publication. Embedding
hosts can retain `Session::analysis_cancellation()` and signal it from another thread; cancellation
is cooperative, and requests after the publication boundary return false.

## What the tornado means

For each input, the command finds its sampled P10 and P90, then takes the median of the first
output within the low-input group (≤P10) and high-input group (≥P90). Bars show the adverse and
favorable changes from the overall median; inputs are ordered by the spread between conditional
medians. The report retains both medians, group sizes, and the Spearman rank correlation.

This is sensitivity derived from the Monte Carlo samples, **not a one-at-a-time causal effect**.
A constant input has undefined rank correlation (blank in the report). Ties can make a group
larger than 10%. Small effects may be sampling noise; increase iterations and check stability
before interpreting their order. Percentiles describe the chosen hypothetical distributions;
they are not confidence intervals or calibrated business probabilities.

## Dependence, precision and convergence

`correlation` is a square numeric matrix in input order, symmetric with diagonal one, entries in
[-1,1], and positive semidefinite. Singular matrices (including perfect ±1 relationships) are
supported. Invalid matrices are rejected, not replaced by a nearest correlation matrix.
Cholesky validation uses a 1e-10 tolerance for floating-point residuals near zero.

The Gaussian copula transforms independent normal coordinates with this matrix, then maps to
the requested marginals. Correlation of non-normal final inputs generally differs from the
latent target. Both observed Pearson and Spearman correlations are reported. A Gaussian copula
does not model every kind of dependence or tail dependence.

For LHS, each **independent driving uniform** is stratified within each replica. Subsequent
copula transformations need not preserve Latin strata in the final input coordinates. Each
replica is randomized independently, so sampling error is estimated from replica means rather
than incorrectly treating dependent LHS rows as IID. LHS can reduce variance, particularly for
smooth/additive responses, but improvement is not guaranteed for every workbook.

- **Random Monte Carlo:** mean standard error is sample SD / sqrt(N). Mean intervals use the
  Student t critical value; probability-above-zero intervals use Wilson score bounds, including
  non-degenerate bounds when no event or every event was observed.
- **Replicated LHS:** errors come from independent replica means/proportions; Student t
  intervals use replica count - 1 degrees of freedom. When replica proportions have no observed
  variation, their confidence interval is left blank with an explicit warning: zero observed
  crossings do not prove zero tail risk.
- Mean/t intervals are approximate for non-normal data or few independent units. Percentiles
  are not confidence intervals. Confidence intervals quantify finite-simulation error under
  the chosen distributions, not uncertainty in the distributions or business assumptions.
- A convergence trace uses complete random prefixes or complete independent LHS replicas.
  Intervals need not shrink monotonically. It is a diagnostic, not an automatic stop rule or
  a guarantee that rare events have been adequately sampled.
- Means use compensated summation, centering same-sign samples around an origin and preserving
  small cancellation residuals for mixed signs. Variance calculations scale deviations to
  avoid overflow and preserve small variation around a large offset. Numeric overflow still
  aborts the report atomically instead of publishing a misleading value.

References (mathematical descriptions, no implementation code copied):
[NIST mean intervals](https://www.itl.nist.gov/div898/handbook/eda/section3/eda352.htm),
[NIST proportion intervals](https://www.itl.nist.gov/div898/handbook/prc/section2/prc241.htm),
[Sandia Dakota sampling](https://snl-dakota.github.io/docs/latest_release/users/usingdakota/studytypes/uq.html).

Macro steps refuse the `analysis.*` lifecycle because jobs belong to the live session; a macro dry run must not advance or cancel one.

## Limits and reproducibility

- Non-Gaussian copulas and percentile fan charts are future work. Discrete marginals and
  conditional count/AR1 worksheet functions are native; see the [process contract](monte-carlo-processes.md).
  The report supports up to 64 formula outputs for monthly forecasts.
- At most 32 inputs, 64 outputs, 2–100,000 iterations, and 1,000,000 input/output sample cells.
  Limits apply even when the sample table is omitted. Large formula models may take time.
- The seed controls the input RNG and versioned `MC.*` worksheet streams. See
  [counts and AR1](monte-carlo-processes.md) for stream identity and process limits. Workbook formulas such as `RAND`/`NOW`, external data and
  floating-point platform differences are outside that reproducibility contract. Use
  deterministic formulas when reproducibility matters.
- What-if trials always recalculate, including in manual calculation mode, without changing the
  workbook's calculation setting. Trial failures and non-finite sampled/results values abort
  without modifying the workbook. Report creation is refused when structure is protected.
- Results and charts round-trip as ordinary XLSX data. Simulation runs in GridCraft's Rust engine.

## Implementation

`crates/engine/src/cmd/monte_carlo.rs` reuses `whatif::Trial`: copy-on-write workbooks and
incremental recalculation. `crates/ui-egui/src/dialogs/monte_carlo.rs` is a thin parameter editor.
Distribution moments, analytic covariance, singular/invalid matrices, LHS strata, NIST interval
reference values, high-offset numerical stability, repeatability, cross-sheet/manual recalculation, error atomicity, undo/redo and
XLSX round-trip tests use synthetic workbooks built in Rust.

## Operational limits and atomic failure

Each analysis run has a ceiling of **100,000,000 estimated work units**. Optional `maxWork`
is a positive integer no greater than 100,000,000 and lowers that ceiling. What-if trials alone
share **300,000,000 reserved units per open workbook per fixed 60-second window**, including
retries and failed/cancelled work. Read-only queries have an independent **1,000,000,000-unit cap**; edits, full recalculation
and explicit imports have a larger finite **10,000,000,000-unit logical-command cap**.
Both kinds stay outside the analysis bank. Selection follows command metadata, not document
trust: a query remains capped even against a locally trusted workbook. A nested query also
charges its enclosing operation and cannot escape either limit. `file.open` receives
that independent protection even without an existing document and never bills the old document.
Clones retain the analysis bank; reopening creates a new bank. Clock rollback or an unavailable
clock does not refill it. These are conservative workload estimates, not measured CPU time or
a process/tenant security sandbox.

Workbook-authored conditional-formatting formulas have a separate **1,000,000-unit reserved
window per document** and a **100,000-unit local limit per cell look**. Each frame shares one
bank-backed meter between uncached looks, retaining its prepaid credit instead of discarding
a reservation per cell. Charges check the local limit, any ambient policy, then the renderer
bank; a rejected oversized native kernel does not reserve its rejected charge from the bank.
Earlier accepted work and unused frame credit are not refunded. Sparse misses across many
frames can therefore consume reserved capacity faster than their exact accepted work.
A local cell or renderer refusal omits that optional look without marking the enclosing
command as failed. Ambient work/cancellation refusal remains terminal and is still observed
before local evaluation and after the local scope returns.

A bounded 4,096-entry document cache reuses conditional looks, including `None`, across
repaints for the same workbook identity/revision and sheet/cell, with a **cache-wide one-second TTL for every result**, including nonvolatile looks and
fallback `None` results. All entries expire together; this also bounds volatile-result staleness. Cached hits check the ambient policy without reserving renderer credit.
Weak workbook identity preserves copy-on-write. Missing/backward clocks invalidate results;
cache invalidation or a new frame never replenishes the renderer bank. Refused evaluations
omit the conditional look even when a formula handles its numeric error.

Print/PDF rendering currently uses a **separate fresh 1,000,000-unit bank per laid-out
sheet per render call**, shared across that sheet's pages, with the same 100,000-unit
per-cell look limit. It does not debit the UI document's persistent rendering bank or
reuse its result cache; repeated exports receive fresh sheet allowances. When rendering
runs through a session command, conditional expressions also charge the enclosing command
policy. A direct `render_pdf` library caller must supply its own ambient hook if it needs
that enclosing limit. These allowances do not establish a cross-workbook, process-wide
or service-wide quota. Sharing the persistent UI bank with printing remains a possible
policy change, rather than a control implemented by this contribution.

Run counters and zero-unit failure/cancellation checks use atomics. Small analysis charges use
prepaid credit: clock reads and shared-window locking occur only on credit renewal or an
explicit bank query. A renewal reserves up to 4,096 units, bounded by the run's actual remaining
allowance and bank availability; a large native charge is fully reserved before its kernel runs.
Sampling, trial evaluation and statistical finalization use the analysis run meter. The
structural full-workbook recalculation during report publication instead uses the enclosing
command allowance; it does not consume the analysis run or its shared bank. Run checks before
and after publication, enclosing-command refusal and the atomic rollback barrier remain active.

`workUsed` reports exact accepted run charges. `sharedWorkRemaining` reports unreserved bank
capacity and can fall faster because unused reservations are not refunded. An observed window
rollover changes the bank generation and expires prepaid old-window credit. A run consuming
already-paid credit observes elapsed time when it next renews; these are reservation windows,
not a strict per-second CPU execution limit. A 1,000-unit run never requires a 4,096-unit grant.

A dense 50,000-row family of prefix sums has at least 1,250,075,000 estimated units before
additional AST work. It can exceed the query allowance while fitting the finite bulk-recalculation
allowance. Tests measure real small prefix-sum recalculations (16/64 rows), then charge a
conservative 50k bound to the real meters without traversing billions of cells. The estimate is
a policy regression fixture, not an exact cost or wall-clock measurement for the large workbook.

Charges cover trial setup, expressions, materialized ranges, native function calls, lifted
broadcast inputs/outputs and kernels, sampling, diagnostics and report preparation. Functions
can supply model-specific estimates/checkpoints through the neutral `Ctx::charge_work` hook;
the function crate's default context accepts work, while the engine connects it to the shared
meter. Nested commands inherit the enclosing logical run instead of resetting its allowance.
A refusal stays latched through later zero-unit checks, even if `IFERROR` handles the worksheet
`#NUM` value. An optional pack's cost model belongs to that pack, without a functions-to-engine
dependency or a second simulation RNG.

Cancellation and work refusal leave no partial report or source edit. The command boundary
restores workbook, selection, revision, undo/redo and journal on terminal work failure, including
handlers that directly replace the workbook. A failed start also discards its newly retained
runtime job. Accepted cancellation occurs before the atomic publication boundary.

Progress uses bounded batches on native and WASM front ends. Checkpoints also run during
sampling and report preparation. They do **not** interrupt an indivisible native call, sort,
allocation or workbook copy; one such operation may exceed the between-trial 8 ms target.
This release does not promise a strict wall-clock deadline or worker-thread isolation.

Report creation is refused above **128 total sheets** or **2,000,000 stored cells**, including
source sheets and a conservative report reservation. Existing history entry/retained-byte caps
remain separate. Tests cover repeated report growth, source/error atomicity, volatile/erroring
formulas, cancellation through UI/MCP, sync/stepped equivalence, nested-meter inheritance,
terminal rollback and cancellation/publication ordering. External services are not exercised;
unsupported `WEBSERVICE`/`FILTERXML` calls remain worksheet errors rather than network requests.

## Revision history

| Date | Change |
| --- | --- |
| 2026-10-27 | Native Monte Carlo, correlation matrix, numerical diagnostics and conditional tornado report. |
| 2026-10-28 | Documented numerical contracts and limited replica controls to LHS. |
| 2026-10-28 | Kept Enter/Shift+Enter inside multiline configuration fields. |
| 2026-10-28 | Shared work budgets, cooperative jobs/progress/cancellation, growth limits and terminal rollback. |
| 2026-10-28 | Atomic hot-path counters, bounded prepaid analysis credit and independent ordinary-command caps; expose maxWork in discovery. |
| 2026-10-28 | Independent conditional-format rendering limits/cache; admit imported documents only after successful recalculation; integrate current upstream rules. |
| 2026-10-28 | Clarified the separate per-sheet print/PDF allowance, ambient command accounting and lack of cross-workbook containment. |
| 2026-10-28 | Extended native marginals and documented seeded conditional counts and stationary AR1. |
| 2026-10-29 | Bill sparse spill traversal by the key interval actually scanned, with a bounded recalculation regression. |
