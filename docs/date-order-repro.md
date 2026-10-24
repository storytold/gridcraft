# Reproduce a missing January row with regional date text

> **Last updated:** 2026-10-24

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

File opening preserves `csvAsText` while setting the workbook region before
recalculation. A generated CSV regression checks that text-only import keeps `00123`
and `13/01/2026` as text, that normal CSV import still converts the identifier to a
number, and that regional `DATEVALUE` yields January in either mode.

This change uses Region from #1524 and its `region-typed-input` parser; it introduces
no separate DateOrder preference. Regional display, localized formula syntax and wider
input/coercion changes remain separate work. The example refuses to overwrite an
existing file; all fixture data is original and fictional.
