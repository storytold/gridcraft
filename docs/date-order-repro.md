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
cargo test -p gridcraft-engine --test it regional_date_arguments
```

The generated tests cover both ambiguous and unambiguous January text, two XLSX
save/open/refresh cycles, MDY/DMY/YMD, pre-open cached formulas and both date systems,
runtime invalidation in several documents, undo/redo returning to the saved state,
unchanged canonical API input and F2 editing of imported numeric dates.

Implicit date arguments use the same region without needing a `DATEVALUE` wrapper:
`YEAR("31/01/2026")` is 2026 and `MONTH("03/02/2026")` is 2 under `it-IT`.
The separate `regional_date_arguments` test generates an XLSX in memory, reopens it,
and checks direct `YEAR`/`MONTH`/`DAY` references and a dependent month label. Changing
the region recalculates those formulas while retaining the original date text and
clean/dirty state. `01/13/2026` remains invalid under `it-IT`, with no en-US fallback.

Numeric serial text keeps the canonical coercion rules: `HOUR("46035.5")` stays 12,
`1,234` stays 1234, and dollar currency, percentages, fractions and accounting
parentheses follow the same rules as general arithmetic. Numeric values, errors and
the workbook's 1900/1904 calendar are also covered by function regressions:

```sh
cargo test -p gridcraft-functions --lib date::tests
```

## Known coercion boundary and whitespace

This follow-up does not make all formula text coercion regional. The generated
`regional_dates_expose_canonical_value_and_arithmetic_boundary` regression records this
remaining compatibility gap explicitly, rather than treating it as Excel parity:

| Formula | en-US | it-IT |
|---|---:|---:|
| `MONTH("02/01/2026")` | 2 | 1 |
| `VALUE("02/01/2026")` | 46054 | 46054 |
| `"02/01/2026"+0` | 46054 | 46054 |
| `MONTH(VALUE("02/01/2026"))` | 2 | 2 |

The table uses the 1900 calendar. In it-IT, the direct date argument means 2 January,
but passing through `VALUE` or arithmetic still produces the en-US serial for 1 February.
Numeric serial text such as `"45000"` remains consistent through all three paths.

An [Excel desktop COM report](https://github.com/storytold/gridcraft/pull/2429#issuecomment-6459063104)
measured matching date-function, `VALUE` and arithmetic interpretations in en-US.
A later [de-DE report by KNUFFIXX](https://github.com/storytold/gridcraft/pull/2429#issuecomment-6659734560)
compared desktop Excel on Windows with GridCraft CLI at `97b6ead`. The tested date
functions agreed, while Excel returned `44227` for `A1+0` when A1 held the text
`31.01.2021`. GridCraft returned `#VALUE!`. The report covers one German date
format and does not test `VALUE` in de-DE. Arithmetic coercion therefore remains
a separate compatibility change with its own test contract.

The earlier en-US COM report measured `DAY("   01/02/2026  ")` as `#VALUE!`. Implicit date arguments
now reject surrounding whitespace on date/time text, including referenced dates and
holiday arrays. Numeric serial text retains its existing padding and number syntax rules:
`HOUR(" 45000.5 ")` stays 12. Core/function regressions cover both calendars and regions,
and a generated XLSX round trip preserves the original padded text and its error.
Typed cell input and the separate `DATEVALUE`/`TIMEVALUE` paths retain their existing
whitespace behavior; this fix is scoped to implicit date arguments.

The branch is stacked on #1886 and includes main at `c4029538`. File opening retains `csvAsText` while
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
checks and its accounting-parentheses rules. The ISO-week regressions cover numeric
serials and Italian date text at the start of both supported calendars. The
desktop-theme watcher uses the upstream implementation; the earlier platform workaround is no longer part of this
contribution. This #2429 follow-up extends implicit date arguments such as
`MONTH(text)` to Region while numeric serial text remains canonical en-US.

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
| 2026-11-05 | Record the external de-DE comparison and align the branch with current main |
| 2026-10-28 | Reject padded implicit date text without changing numeric serial coercion; document and test the remaining VALUE/arithmetic regional boundary and the limits of reported Excel observations |
| 2026-10-27 | Add transient native overrides and synthetic grouped rollback, nested history and save-point regressions; preserve the regional CLI oracle after main's module split and inherit these fixes in the implicit-date follow-up |
| 2026-10-26 | Verify shared workbook windows and preserve parser bounds, regional implicit date text and canonical numeric coercion |


Panic recovery also reapplies the effective application Region after restoring content
and history. A command that edits a cell, changes `app.region`, and then panics cannot
restore formulas interpreted in the previous Region. A synthetic regression covers
clean and modified documents, an existing redo branch, recalculated dates, and undo/redo
returning to the original save point. The original region-only panic control is retained.


Rollback graph work is conditional: restoring a different snapshot rebuilds the graph once, except when applying the effective Region already rebuilt it through full recalculation. An idle rollback leaves the graph alone. A fictional regression checks all four snapshot/Region combinations, exact rollback rebuild counts without timing assertions, restored formulas, dependencies, undo/redo lengths, dirty state and saved snapshot identity.
