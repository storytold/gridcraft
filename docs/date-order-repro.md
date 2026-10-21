# Reproduce a missing January row with regional date text

> **Last updated:** 2026-10-21

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
cargo test -p gridcraft-engine --test region_loading --test region_dates
```

The generated tests cover both ambiguous and unambiguous January text, two XLSX
save/open/refresh cycles, MDY/DMY/YMD, pre-open cached formulas and both date systems,
runtime invalidation in several documents, undo/redo returning to the saved state,
unchanged canonical API input and F2 editing of imported numeric dates.

The branch integrates the official `v0.9.0` tag. File opening preserves its
`csvAsText` option while setting the workbook region before recalculation. A generated
CSV regression checks that text-only import keeps `00123` and `13/01/2026` as text,
that normal CSV import still converts the identifier to a number, and that regional
`DATEVALUE` yields January in either mode. The integration also uses a zero-sized
non-Linux desktop-theme handle instead of `()`, so strict Clippy accepts the shared
watcher on macOS; the Linux portal connection and theme behavior are unchanged.

This change uses Region from #1524 and its `region-typed-input` parser; it introduces
no separate DateOrder preference. Regional display, localized formula syntax and wider
input/coercion changes remain separate work. The example refuses to overwrite an
existing file; all fixture data is original and fictional.
