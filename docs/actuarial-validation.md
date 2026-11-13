# Actuarial validation evidence

Latest integrated verification completed 2026-10-28: **PASS** for the documented implemented scope.
All ten quality gates passed with 2,714 native tests across 24 binaries (one preexisting
ignored test), including WASM on upstream release 0.13.0. Validation resumed in unchanged-source
stages after a linker disk-space failure, as recorded in the final review follow-up below.

Initial numerical integration base: `02e4d50088e532f81a96f80eab539cabf77da7e4`, local branch
`feat/actuarial-pack`. The supported estimands/domains are defined in
[the pack contracts](actuarial.md) and [scientific-method contracts](actuarial-scientific.md).
Validation applies to those implemented domains, not every actuarial model or a particular
real portfolio. All fixtures are original synthetic data or independently calculated values.

## Reproducible artifacts

- [Foundational workbook command script](examples/actuarial.jsonl): life, losses, reserves,
  Markov and pricing. Recalculation, XLSX round-trip, undo and MC source isolation are tested.
- [Scientific workbook command script](examples/actuarial-scientific.jsonl): censoring,
  Bayesian prediction, Cox, EVT, dual sensitivities, reinsurance, ALM, validated surrogate,
  semi-Markov and externally supplied bootstrap replications. Also round-tripped through XLSX.
- [Independent numerical reference cases](../crates/engine/tests/fixtures/actuarial-reference.json):
  62 engine-boundary fixtures with closed-form/Decimal oracles, including intentionally invalid
  models. Expected values are not copied from the product's observed outputs.
- Mathematical unit tests in each `functions::actuarial` module cover model identities,
  invalid/hostile input, finite-work limits and numerical edge cases.

## Independent scientific review

A separate, explicitly authorized reviewer uses the actual CLI/MCP executable, not mocks.
Checks include closed-form identities, negative controls, GPD parameter recovery, independent
Cox score roots, a 200-dataset illustrative Beta interval experiment, uniform copula margins
and 2-increasing rectangles, a direct-PDF Gamma integration for bootstrap process draws and
hand-calculated Mack cross-origin covariance. Empirical coverage from a finite synthetic
experiment is not a universal coverage guarantee or a real-data calibration claim.

Independent review identified and drove corrections for cancellation in compound TVaR,
small-shape negative-binomial recurrence, tiny joint copula tails and Frank's independence
limit, extreme Gamma credible endpoints, undefined KM confidence bounds at survival 0/1,
and IPCW event/censor tie ordering. The pre-fix executable was preserved outside the repo as
a RED control, and the same oracle cases are run against the corrected executable.

## Initial mathematical-pack gate (`0a16c19`)

- [x] Implement all 95 worksheet functions with explicit contracts and no duplicate RNG.
- [x] Ship two synthetic replay scripts and source-level analytic/regression tests.
- [x] Archive independent expected numerical values at the engine boundary.
- [x] Pass final complete `cargo xtask ci`: all 10 gates, 2,637 workspace tests across
  24 test binaries, including 84 actuarial unit tests and four workbook/engine tests.
- [x] Independent review: 52/52 closed-form probes; 6/6 bootstrap/Mack cases;
  6/6 recovery/interval checks; 9/9 copula property configurations (324 CDF queries).
  Real MCP stdio initializes successfully and discovers all 95 functions.
- [x] RED control: the same 52-case independent battery passes 42 cases on the old
  executable and all 52 on the corrected executable, preserving ten expected failures.
- [x] Rustdoc: all 274 production functions/methods in the extension have attached `///`;
  non-rustdoc source content was verified unchanged by the documentation pass.

The full CI passed i18n-report, formatting, Clippy, workspace tests, doctests, assets,
dependency layers, WASM, command parity and generated keytips. The independent executable
probes additionally verified the CLI/MCP path and deliberately invalid models. The original 54 engine-boundary cases preserve the closed-form/Decimal expectations in normal repository CI.
`cargo doc -p gridcraft-functions --no-deps --document-private-items` succeeded with zero
diagnostics in the actuarial modules; preexisting financial/private-link warnings remain
outside the extension. No mathematical defect reproduced by the independent reviewer remains open.

The full gate uses optional official `craft-fonts` inputs and local sockets for existing
control-server tests. Low-disk native builds can disable incremental/debug information:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CRAFT_FONTS_DIR=/path/to/craft-fonts cargo xtask ci
```

An initial full debug build exhausted disk; its build artifacts were cleaned. The first
compact sandboxed run hit local socket permissions. Neither attempt is counted as a pass.
Final execution evidence is recorded only after the latest source passes the complete gate.

## PR review follow-up: optional arguments and Monte Carlo keyboard handling

The original full-gate results above describe the initial actuarial contribution `0a16c19`.
Review follow-up results are recorded separately rather than attributed to that earlier run.

- [x] ACT-only defaults now distinguish skipped empty literals from explicit zeros and blank
  cell references. The shared Excel-compatible `util::opt_num` behavior is unchanged.
- [x] Nine new registered-function regressions passed with all 238 function-library tests.
  An independent consumer linked to the actual Rust library passed 15 closed-form/blank-reference
  checks; ten defect controls were reproduced on the previous actual CLI executable.
- [x] Removed the internal test-class note from this product-facing evidence document.
- [x] Integrate only MC.3 `6247a59` and MC.4 `84036feb` in order: numerical Rustdoc,
  Random/LHS dialog parameters and Enter/Shift+Enter handling. MC sampler/correlation contracts
  are unchanged. Shared budgets, progress and cancellation are a separately tracked follow-up below.
- [x] Extend the engine-boundary oracle fixture from 54 to 62 cases for parsed skipped
  arguments and blank references. The ACT-only helper brings documented production methods to 275.
- [x] Review fixes through MC.4: complete clean-target `cargo xtask ci` passed all ten gates,
  including WASM and 2,648 workspace tests and doctest checks. This result does not cover
  the newly requested operational integration listed below.

The first combined follow-up attempt reused a copied Monte Carlo target and reported four
engine failures (`ACT.*` names unavailable). It is not counted as a pass. Rebuilding with an
independent target passed all four actuarial engine tests, including all 62 reference cases;
the copied target was cleaned before the final full-gate run.

## Integrated analysis controls (MC.5)

- [x] Integrate only MC.5 `536679cb973fdb910fd795c1edfe8e9f6567f568` as `e941e9b`,
  retaining the existing equivalent WORK.1/WORK.2 commits without duplicating them.
- [x] Implement the L1 `Ctx::charge_work` contract, generic/native/lift checkpoints and
  saturating ACT estimates with adaptive GLM/Cox/bootstrap/compound-tail checkpoints.
  All 249 function-library tests and all-target Clippy passed. An independent reviewer
  additionally ran 99 actuarial tests and 14 accounting/lift checks against the reviewed binary.
  Further regressions reserve output-grid work even for zero-count compound models and
  distinguish closed copula formulas from elliptical quadrature, including family aliases.
  Two isolated source-mutation RED controls fail as expected when the ACT reservation or
  final-broadcast latch check is removed; the production checkout was unchanged.
- [x] Connect and validate the shared L1→calc→engine meter across native/lifted calls and
  Monte Carlo trials, retaining failure outside formula error handling.
- [x] Verify progress, cooperative cancellation, exhausted budgets and atomic report publication
  with actuarial workbook formulas, including the unchanged synchronous command path.
- [x] Pass the final integrated `cargo xtask ci`: 10/10 gates, 2,677 native tests across
  24 binaries, including all 249 function-library tests and seven actuarial engine tests.
- [x] Independent integrated runtime review: 126 direct tests (seven actuarial engine,
  101 ACT, eight lift, nine analysis-job and one calc-work), eight real MCP checks and
  three CLI controls passed. MCP discovers 95 ACT functions and verifies an independent
  GPD oracle, invalid PSD rejection, ordinary mathematical `IFERROR`, terminal quota
  rejection through `IFERROR`, and successful subsequent commands. The old CLI publishes
  the over-budget lifted model; the final CLI exits with a work-limit error and creates
  no XLSX. The cheap model fits the same 1,000-unit budget and creates its report.

The seven actuarial engine tests include seeded synchronous/incremental equivalence,
one-action undo, source/history/journal protection, progress and cancellation,
non-refunded shared work across cancelled jobs, and a lifted annuity hidden by `IFERROR`.
A simple `A1*2` model fits the same 1,000-unit budget that rejects that lifted model.
The original CLI publishes the latter model without enforcing this limit (RED); the new
engine rejects it before publication (GREEN). All 62 independent numerical fixtures pass.

Operational defaults are 100 million estimated units per run and 300 million per workbook
in a fixed 60-second window; `maxWork` can tighten the run ceiling. Report growth is bounded
at 128 sheets and two million stored cells including the source. Cancelled/failed work is
not refunded. Job ids, stale sources and the atomic publication boundary are checked;
publication creates one canonical `data.monteCarlo` undo action. The synchronous command
remains available alongside `analysis.start/status/step/cancel/finish`.

Cancellation occurs at actual checkpoints. Trial batches yield between trials after a soft
~8 ms/250,000-unit target (at most 2,048 trials), with sampling/finalization checks. These are
operational estimates and cooperative boundaries, not guaranteed latency or interruption of
an indivisible native kernel. Browser UI events require control to return to the event loop.

Only already-completed native UI test executables were removed during the final run to make
space for WASM metadata; source, libraries and engine/function review binaries were retained.
This cleanup did not replace or skip a gate.

## MC.6 and conditional-formatting review integration

MC.6 source commit `666aa7e408559d91ba548cfe1c19d540019f78b8` is integrated on the
actuarial base `51f5877` by `20f1cdb`. Its separate ten-step CI passed before this integration;
the final combined gate below is distinct from that earlier result.

- [x] Preserve MC.6 atomic accounting and prepaid analysis credit. Ordinary commands,
  including standalone CLI/MCP formula queries and opening without an active document,
  have a separate one-billion-unit command cap and do not consume the analysis window.
- [x] Bound conditional-formatting formulas with a persistent renderer bank and a local
  cell limit; share frame credit across misses and reject oversized kernel charges locally
  before reserving renderer credit. Cache results with bounded storage, weak workbook
  identity, revision and TTL. The precise reservation contract is in [Monte Carlo](monte-carlo.md#operational-limits-and-atomic-failure).
- [x] Author focal verification: 35 tests passed (13 conditional-formatting, 13 analysis-job,
  eight workload and one import-atomicity regression). The analysis suite includes seven edits
  of a 10,000-row running-total model without spending the analysis bank.
- [x] Prepare imported documents off-session and check work refusal before replacing a blank
  workbook or adding windows. The regression covers empty, blank and multi-document sessions,
  retained history/journal/selection/revision and a successful subsequent import.
- [x] Independent focal review: all 35 tests passed. Five isolated negative controls
  reproduced the failures when cache reuse, local-before-bank charging, import preparation,
  ordinary-command independence or render metering was disabled. Original-source positive
  reruns remained green after the controls.
- [x] Final combined `cargo xtask ci`: all ten steps passed, including Clippy, 2,693 native
  tests across 24 binaries (one preexisting ignored test), doctests, assets, layers, native UI,
  WASM, command parity and generated keytips. This gate used the combined ACT + MC.6 + renderer
  and import-atomicity source, with local-socket permission enabled.
- [x] Final compiled CLI/MCP: 11 checks passed, including discovery of all 95 ACT functions,
  a closed-form GPD oracle, invalid PSD rejection, ordinary IFERROR, terminal one-billion-unit
  refusal through IFERROR, subsequent-command recovery, the scientific XLSX example and
  atomic rejection/success controls using the same 1,000-unit simulation budget.

Render cache hits do not reserve new credit. Unused frame credit is not refunded, so sparse
misses in many frames can consume reserved capacity faster than accepted formula work. These
controls provide cooperative workload containment; they do not establish a CPU deadline or
cross-tenant hosting isolation.

## Upstream 0.13.0 and command-policy integration (`5083959`)

The contribution preserves published actuarial history through `03dbb8c` and integrates
MC.7 `df9d947` with upstream functional base `55b5a45` in `2f306ae`. The MC.8 policy
is also published separately as `029a158`. Upstream `43284377` changes only workspace
versions to 0.13.0; the final gate below was rerun against that release.

- [x] Separate finite command allowances by command metadata: journaled/undoable edits,
  imports and recalculation receive 10 billion units; read-only queries retain one billion,
  including nested queries and locally trusted workbooks. Neither consumes the analysis bank.
- [x] Measure actual `SUM` evaluation on dense 16/64-row models and verify a conservative
  50,000-row charge of 1,250,825,000 units against the production meter. This gate checks
  the policy without making 1.25 billion visits over ranges in every CI run.
- [x] Verify nested query refusal remains terminal and parent charging occurs exactly once;
  retain finite ceilings and the shared analysis and renderer limits.
- [x] Clarify that the one-second conditional-look cache TTL covers all stored results.
- [x] Final release `cargo xtask ci`: all ten gates, 2,707 native tests in 24 binaries,
  one preexisting ignored test, all 251 function-library tests, nine actuarial workbook tests,
  doctests, assets, layers, WASM, parity and keytips.
- [x] Independent review reruns 156 focal native cases against the 0.13.0 binaries.
  Runtime controls confirm CLI and MCP server version 0.13.0, discovery of
  95 ACT functions among 609 functions, 361 unique commands and the `maxWork` parameter.
  Numerical oracles, skipped defaults, mathematical `IFERROR`, terminal quota refusal and
  subsequent-command recovery pass. The same 1.1-billion-unit estimated formula is rejected
  as a query and accepted as a cell edit, with independently expected result 275.
- [x] The exact external import-admission consumer passes again against the new 0.13.0
  libraries: the blank document retains its UID, workbook `Arc` and document count; an empty
  session retains zero documents. Both cases reach a positive work checkpoint before refusal.

The policy is based on the operation, not document trust. A high finite edit allowance does
not exempt a nested query or establish a CPU deadline. See [the operational contract](monte-carlo.md#operational-limits-and-atomic-failure)
for separate screen-rendering and print/PDF allowances and embedding responsibilities.

An optional end-to-end 50,000-row CLI/MCP experiment reached its 180-second watchdog
during model setup. No final value, explicit recalculation, edit or import result was
obtained; that experiment is not counted as a pass or a quota-refusal control. The
50,000-row gate above validates conservative accounting policy, not completion time.

## CodeRabbit follow-up: scoped CF refusal and report publication

Generic commit `6eb103c` corrects two control-flow defects without changing ACT formulas
or sampling algorithms. Optional conditional-format evaluation now keeps its own sticky
refusal latch while checking the enclosing policy and forwarding actual work to it. A local
cell/frame refusal omits optional styling; a real enclosing refusal still terminates the
command. Statistical finalization retains analysis accounting, then report publication
releases that scope so structural source recalculation uses the enclosing command allowance.
The later trial check, publication boundary and rollback checks remain active.

- [x] Seven new regressions cover local refusal/reentry, real parent refusal, neighbouring
  cell styling, exhausted-frame fallback, both publication modes and parent-budget rollback.
- [x] All ten gates pass on byte-identical reviewed sources: 2,714 native tests in 24 binaries,
  one preexisting ignored test, plus formatting, Clippy with warnings denied, doctests, assets,
  layers, WASM, parity, keytips and the i18n report. The initial combined invocation passed
  formatting/Clippy but ran out of disk while linking tests. After cleanup, the native gate
  and the remaining six gates completed in stages with no source changes.
- [x] A separate reviewer reran 21 compiled focal tests: 16 CF, two publication and three
  work-hook tests, including all seven new regressions.
- [x] Six real CLI RED/GREEN controls compare the previous executable with the corrected
  one: synchronous publication, incremental publication/undo and command-driven PDF export.
  The source cells retain the independently expected values 10, 20 and 2,001,000.
- [x] PDF content and raster inspection confirm that the inexpensive neighbouring cell B2
  retains its fill while the costly B1 conditional rule is omitted. The old executable could
  leave a parseable PDF while returning a work-limit error; the corrected command succeeds.

The focused source regressions can be replayed with `cargo test -p gridcraft-engine --lib
cf::tests`, `cargo test -p gridcraft-engine --lib publication_recalculation` and
`cargo test -p gridcraft-calc --lib work::tests`. The optional 50,000-row experiment above
remains inconclusive and is not counted among these successful controls.
