# Analysis ToolPak regression models

Last updated: 2026-11-05. Reconciled with main
`c40295382886f0a18d281a9911e9506fdf53b863`. For validation status, see
"Validation status and pending work".

`data.analysisToolPak` accepts `tool: "regression"` and an optional `model`:

| Model | Fitted equation | Required positive inputs |
|---|---|---|
| `linear` (default) | Y = intercept + sum(bᵢ Xᵢ) | None |
| `exponential` | ln(Y) = intercept + sum(bᵢ Xᵢ) | Y |
| `logarithmic` | Y = intercept + b ln(X) | X; exactly one X column |
| `power` | ln(Y) = intercept + b ln(X) | Y and X; exactly one X column |

For exponential and power models, the reported intercept is **ln(amplitude)**;
the original amplitude is `exp(intercept)`. `constantZero: true` fixes that amplitude
to 1. Linear/exponential fits retain the existing maximum of 16 X columns.

Coefficients, standard errors, t-tests, confidence intervals, R², adjusted R²,
ANOVA, predictions, residuals, standardized residuals and probability output
describe the fitted domain. Exponential/power response headings explicitly name
ln(Y); logarithmic/power predictor headings name ln(X). Predictions are not
exponentiated or bias-corrected. This is transformed ordinary least squares,
not arbitrary nonlinear least-squares on the original Y scale.

Reports remain static cell values with existing output-range/new-sheet,
overwrite and undo/redo behavior. Linear report positions remain unchanged;
the formerly blank second row identifies the model. The existing OLS solver
uses normal equations and has not been replaced by QR. This change does not
establish accuracy for ill-conditioned data.

## Shared reference editing and form scope

Y, X and Output Range use upstream `Field::Ref` / `ref_edit`, including collapse,
focus, cross-sheet qualification and restoration of the home sheet/selection.
There is no separate RangePicker, AnalysisContext, document guard or pointer route.
Output Range follows the upstream range-reference contract; it is not truncated
by a custom single-cell picker. Engine validation determines valid output input.

The model dropdown, first-row labels and zero-intercept checkbox remain separate,
with a fitted-domain explanation for exponential/power. The form uses main's
scroll area and action row. All upstream ToolPak entries, dynamic fields,
defaults and validators remain: two-factor ANOVA, z-Test, Fourier, random number
generation and sampling included. Generic dialog resize belongs to separate work.

Implementation: `crates/engine/src/cmd/analysis.rs` holds model transformations and
report generation; `crates/ui-egui/src/dialogs/analysis.rs` holds model presentation.
Reference editing is upstream `dialogs/ref_edit.rs`; shared dialog/control/grid
files retain main's implementation.

## Independent exact benchmark (not a published dataset)

The synthetic fitted-domain observations are x = [0, 1, 2, 3],
y = [1, 2, 2, 5]. All expectations below are derived here, independently of
GridCraft OLS, LINEST and distribution functions. No published benchmark was
consulted or is claimed. The executable case is
`regression_models_match_independent_exact_four_observation_benchmark`.

n = 4, x̄ = 3/2, ȳ = 5/2; sum(x²) = 14, sum(xy) = 21, sum(y²) = 34.
Thus Sxx = 5, Sxy = 6, Syy = 9.
The minimizer is b = Sxy/Sxx = 6/5, a = ȳ − b x̄ = 7/10.
Predictions are [7/10, 19/10, 31/10, 43/10], giving residuals
[3/10, 1/10, −11/10, 7/10].

| Quantity | Exact result |
|---|---|
| Regression / residual / total df | 1 / 2 / 3 |
| SSR / SSE / SST | 36/5 / 9/5 / 9 |
| MS regression / MS residual; F | 36/5 / 9/10; 8 |
| R² / adjusted R² | 4/5 / 7/10 |
| Regression standard error | sqrt(9/10) |
| SE(a) / SE(b) | sqrt(63/100) / sqrt(9/50) |
| t(a) / t(b) | sqrt(7)/3 / sqrt(8) |
| Two-sided p(a) / p(b) | 1 − sqrt(7)/5 / 1 − 2/sqrt(5) |
| Significance F | 1 − 2/sqrt(5) |
| 95% / 90% two-sided t critical, df=2 | sqrt(722/39) / sqrt(162/19) |

For df=2 the normalized t density is (t²+2)^(-3/2). Integrating it gives
the two-sided tail 1 − |t|/sqrt(t²+2). Solving tail = alpha gives
t² = 2(1−alpha)²/[alpha(2−alpha)]. Each interval is coefficient ± critical × SE.
Since F(1,2) is the square of t(2), its upper tail at 8 is the slope's
two-sided p-value. SE(b)² = MSE/Sxx and SE(a)² = MSE(1/n + x̄²/Sxx).
Standardized residuals follow the existing report convention,
residual/sqrt(SSE/(n−1)) = residual/sqrt(3/5); they are not leverage-studentized.
Probability output sorts [1,2,2,5] at percentiles [12.5,37.5,62.5,87.5].

The same exact problem is supplied to all four models: retain x,y for linear;
use exp(y),x for exponential; y,exp(x) for logarithmic; exp(y),exp(x) for power.
The test checks coefficients, ANOVA, inference, intervals, predictions,
residuals, probability values and transformed labels with absolute tolerance 1e-9
to accommodate floating-point exp/ln and distribution evaluation.

A zero-intercept rerun uses b = sum(xy)/sum(x²) = 3/2,
SSE = 5/2, uncentered SST = 34, SSR = 63/2, df = 1/3/4,
MSE = 5/6, F = 189/5, SE(b) = sqrt(5/84), R² = 63/68 and
adjusted R² = 46/51. Intercept inference is NA. This also checks the
amplitude-one constraint for exponential/power.

## Validation status and pending work

On 2026-11-12, the current reconciliation with main `a60db4f1` passed local
`cargo test -p gridcraft-engine --lib` (969 tests), the regression UI integration
tests (3 tests), and the focused dialog unit test (1 test). Formatting and
`git diff --check` also pass. `cargo xtask ci`, WASM validation and Windows/Excel
checks have not been rerun on this candidate; the Windows results below remain
historical evidence for the earlier producer checkout only.

On 2026-11-05, the reconciled candidate at main `c4029538` passed all ten
`cargo xtask ci` gates on Fedora 45: i18n report, formatting, Clippy, tests,
doctests, assets, dependency layering, WASM, parity and keytips. The run used
the candidate checkout, rebuilt the host `xtask` and the affected native and
WASM packages, and completed without source changes. Its log and source
fingerprint are retained in the local review record.

Separately, on 2026-11-04, the earlier Windows producer checkout at `5a3ed86c`
plus local IMSQRT/CLI portability fixes passed all ten CI gates. That source
also passed Excel 16.0 build 20527 recalculation and SaveAs checks for four
synthetic regression reports, with 17 cell expectations checked per model.
Those Windows results apply only to that recorded producer; they are not a
Windows or Excel test of the reconciled `c4029538` candidate.

Excel 16.0 build 20527 opened, recalculated, saved and reopened all four
synthetic regression reports. Each model's 17 cell expectations were checked
on both opens, using native INTERCEPT/SLOPE/RSQ reference formulas in the
appropriate fitted domain. All passed, and the source files remained intact.
These exact synthetic cases do not establish accuracy for ill-conditioned or
arbitrary real-world inputs.

The selected GridCraft screenshots were approved by the contributor on
2026-11-05. They are visual references, not evidence of Windows rendering or
Excel equivalence.

Existing noisy LINEST comparisons remain as internal consistency checks, not
independent benchmarks. Domain rejection without workbook mutation, invalid model
types/defaults, overwrite and undo/redo coverage remain. The model-specific UI
case checks dropdown selection, the ln(Y) note, shared collapse/expand and report
confirmation. The duplicate picker tests were removed.

Main's unchanged `crates/ui-egui/tests/it/ref_edit.rs` supplies pointer, keyboard,
focus, cross-sheet quoting, home-sheet deletion/recreation/rename/undo,
selection restoration and scripted confirmation/cancellation coverage. The
complete Fedora CI run included the integration suite.

Focused checks for the next reconciled state:

```sh
cargo test -p gridcraft-engine --lib cmd::analysis::tests
cargo test -p gridcraft-ui-egui --lib dialogs::analysis::tests
cargo test -p gridcraft-ui-egui --test it regression_analysis::
cargo test -p gridcraft-ui-egui --test it ref_edit::
cargo test -p gridcraft-ui-egui --test it interaction::data_analysis_dialog_runs_a_tool_and_asks_before_overwriting
cargo test -p gridcraft-ui-egui --test it dialog_confirm::
cargo test -p gridcraft-ui-egui --lib i18n::ja::tests
cargo check -p gridcraft-ui-egui --target wasm32-unknown-unknown
```

## Revision history

| Date | Change |
|---|---|
| 2026-11-05 | Reconcile with main c4029538 and record its verified Fedora all-ten-gate CI; separately identify the earlier Windows/Excel validation source |
| 2026-11-03 | Reconcile main df137b8; use upstream RefEdit and preserve expanded ToolPak; add independent exact benchmark; targeted checks: 8 engine and 1 power-model UI test passed; full CI, WASM and coordinator screenshots/visual review pending |
| 2026-10-27 | Move ToolPak UI and regression tests into shared dialog modules |
| 2026-10-23 | Document transformed models, diagnostics and domain validation |
