# Analysis ToolPak regression models

Last updated: 2026-10-23.

`data.analysisToolPak` accepts `tool: "regression"` and an optional `model`:

| Model | Fitted equation | Required positive inputs |
|---|---|---|
| `linear` (default) | Y = intercept + sum(bᵢ Xᵢ) | None |
| `exponential` | ln(Y) = intercept + sum(bᵢ Xᵢ) | Y |
| `logarithmic` | Y = intercept + b ln(X) | X; exactly one X column |
| `power` | ln(Y) = intercept + b ln(X) | Y and X; exactly one X column |

For exponential and power models, the reported intercept is **ln(amplitude)**. The amplitude
in the original equation is `exp(intercept)`. `constantZero: true` constrains the intercept
in the fitted domain; for these two models it fixes the original amplitude to 1. It does not
force the original response Y to have a zero intercept.

Coefficients, their standard errors, t tests, confidence intervals, R², adjusted R², ANOVA,
predictions, residuals, standardized residuals and probability output all describe the fitted
domain. In exponential/power reports that domain is ln(Y), and the response headings identify
it. Predictions are not automatically exponentiated or bias-corrected. These are transformed
ordinary least-squares fits, not nonlinear least squares on the original Y scale. Logarithmic
regression keeps Y on its original scale and labels its predictor ln(X).

Reports remain static cell values with the existing output-range/new-worksheet, overwrite,
undo and redo behavior. The linear model keeps the existing coefficient/statistic positions;
the previously blank second row now identifies the model.

## Worksheet range selection

Analysis inputs, optional histogram bins and the output cell offer Select… / Finish buttons.
While picking, the analysis dialog shows the selected field so the worksheet remains available.
The user may change worksheets within the same workbook. Picked references always include
the worksheet name, with spaces and apostrophes escaped by the formula printer. Relative
references already typed into other fields are qualified before a worksheet change, preserving
the worksheet they originally referred to. Output picking uses the starting cell only.

The dialog is bound to the original document UID. Switching to another workbook suspends
picking and blocks both UI confirmation and `ui.dialog.set` / `ui.dialog.confirm`; returning
to the owner resumes it. Closing the owner does not rebind the dialog to another workbook.
A drag also records its document and worksheet so a tab switch cannot extend it into another
worksheet. Invalid or stale references are rejected by the engine.

The model, first-row labels and zero-intercept option are separate controls. Generic window
resizing, scrolling, chooser layout and dialog footers belong to the responsive-interface PR.
This change does not require chart formatting, export, model or XLSX changes.

## Validation

Regression cases compare noisy transformed fits (including a forced zero intercept)
with worksheet LINEST, cover domain errors and unchanged workbooks on failure, verify
overwrite/undo/redo, and exercise cross-worksheet grid input and document ownership.

```sh
cargo test -p gridcraft-engine --lib cmd::analysis::tests
cargo test -p gridcraft-ui-egui --lib dialogs::tests::analysis_
cargo test -p gridcraft-ui-egui --lib dialogs::tests::regression_
cargo test -p gridcraft-ui-egui --test it regression_analysis::
cargo test -p gridcraft-ui-egui --test it interaction::data_analysis_dialog_runs_a_tool_and_asks_before_overwriting
cargo test -p gridcraft-ui-egui --test it dialog_confirm::
cargo test -p gridcraft-ui-egui --lib i18n::ja::tests
cargo check -p gridcraft-ui-egui --target wasm32-unknown-unknown
```

## Revision history

| Date | Change |
|---|---|
| 2026-10-23 | Sync with main; document production API behavior for regression model validation, fitted-domain reports and analysis dialog ownership |
| 2026-10-23 | Describe transformed regression diagnostics, range selection and the PR2 validation plan |
