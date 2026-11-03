# Reproduce a missing January row with regional date text

> **Last updated:** 2026-11-03

This fictional two-row workbook is generated entirely in Rust. It contains date text,
`DATEVALUE`, a `YEAR`/`MONTH` label and amounts; a PivotTable sums amounts by month.
It contains no private data, Excel-authored files or GETPIVOTDATA. Choose a new path:

```sh
cargo run -p gridcraft-engine --example date_order_pivot -- /tmp/regional-date-pivot.xlsx
```

| Date text | it-IT date | Month | Amount |
|---|---|---|---|
| `13/01/2026` | 2026-01-13 | 2026-01 | 17 |
| `02/02/2026` | 2026-02-02 | 2026-02 | 23 |

The saved report at F3:G6 has January **17**, February **23**, Grand Total **40**.
Region is an application preference, not an XLSX property. Opening in the default
en-US region recalculates the January DATEVALUE to `#VALUE!`. Refreshing the pivot
then removes January, reproducing the former en-US-only behavior:

```sh
cargo run -p gridcraft-cli -- run --in /tmp/regional-date-pivot.xlsx \
  --cmd 'data.refreshAll' --print A1:G7
```

Select a DMY region before the first recalculation to retain the report:

```sh
cargo run -p gridcraft-cli -- run --region it-IT --in /tmp/regional-date-pivot.xlsx \
  --cmd 'data.refreshAll' --print A1:G7
cargo run -p gridcraft -- --region it-IT /tmp/regional-date-pivot.xlsx
```

January stays **17**, February **23**, total **40**, with unchanged source text and formulas.
To recover an already-open en-US session, execute `app.region` with `{"region":"it-IT"}`,
then `data.refreshAll`. The first recalculates formulas; the second rebuilds the report.

```sh
cargo test -p gridcraft-engine --test it region_
```

The generated tests cover both ambiguous and unambiguous January text, two XLSX
save/open/refresh cycles, MDY/DMY/YMD, pre-open cached formulas and both date systems,
runtime invalidation in several documents, undo/redo returning to the saved state,
unchanged canonical API input and F2 editing of imported numeric dates.

The branch integrates main at `7bb397a3`. File opening retains `csvAsText` while
applying the selected formula region before recalculation. Text-only import keeps
`00123` and `13/01/2026` as text. Automatic CSV import preserves main's per-column
date-order inference from #3043: the identifier becomes 123 and the unambiguous
DMY date becomes its serial, independently of the formula region. Subsequent
DATEVALUE text still follows that region. Neither mode changes the workbook's
save point on opening.

Opening documents preserves main's shared-workbook window lifecycle and control-token
authentication. An embedder using `DocState::from_saved` keeps cached values when the
workbook already has the session Region; attaching a workbook with a different Region
recalculates it while preserving its saved state. All windows of the same workbook
share one regional recalculation and one undo history.

The regional parser retains main's finite-number and bounded date/time parsing
checks and its accounting-parentheses rules. The desktop-theme watcher uses the
upstream implementation; the earlier platform workaround is no longer part of this
contribution. Implicit date arguments such as `MONTH(text)` keep their canonical
en-US interpretation here; the separate #2429 extends those arguments to Region.

This change retains the Region data associated with #1524 and the original
`region-typed-input` parser history at `ccd3e1c`; it introduces
no separate DateOrder preference. Regional display, localized formula syntax and wider
input/coercion changes remain separate work. The example refuses to overwrite an
existing file; all fixture data is original and fictional.

The native `--region` override lasts for this launch; serializing preferences retains the
previous saved choice. A deliberate `app.region` change replaces the override and persists,
even if the user selects the same tag as the override. Undo groups retain the effective
region when rolled back, preserve clean/dirty and undo/redo identity, and respect a real
save made while the group was open. Region-only groups do not manufacture an undo step.

## Revision history

| Date | Change |
| --- | --- |
| 2026-10-27 | Add transient native overrides and synthetic grouped rollback, nested history and save-point regressions; preserve the regional CLI oracle after main's module split |
| 2026-10-26 | Verify shared workbook windows and preserve parser bounds and canonical implicit-date coercion |


Panic recovery also reapplies the effective application Region after restoring content
and history. A command that edits a cell, changes `app.region`, and then panics cannot
restore formulas interpreted in the previous Region. A synthetic regression covers
clean and modified documents, an existing redo branch, recalculated dates, and undo/redo
returning to the original save point. The original region-only panic control is retained.


Rollback graph work is conditional: restoring a different snapshot rebuilds the graph once, except when applying the effective Region already rebuilt it through full recalculation. An idle rollback leaves the graph alone. A fictional regression checks all four snapshot/Region combinations, exact rollback rebuild counts without timing assertions, restored formulas, dependencies, undo/redo lengths, dirty state and saved snapshot identity.
