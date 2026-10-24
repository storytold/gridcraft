# Keep whole-axis PivotTable sources on XLSX save

> **Last updated:** 2026-10-24

A PivotTable whose source is `Sheet1!$A:$B` used to save the source as the current
used rectangle, for example `A1:B3`. After reopening that XLSX file, Refresh All
could no longer include data appended below row 3.

The writer now separates the declared source from the bounded rectangle used to
build cache records. A whole-column source is written with explicit cell endpoints
(`A1:B1048576`), and a whole-row source with the last column (`A1:XFD9` for `1:9`).
The reader already recognizes these endpoints and restores the whole-axis source.
Sheet-local defined names retain the same declared extent when resolved to a range;
workbook-scoped names keep their name in the worksheet source. Fixed rectangles
remain fixed. This does not enable refresh of external workbook sources.

Only the intersection with the source sheet's used range is scanned for cache
generation. Existing source-size limits still apply; saving two whole columns does
not allocate a million cache rows.

## Reproduce with generated data

The tests construct all workbooks in Rust, without private data or binary fixtures:

```sh
cargo test -p gridcraft-xlsx --lib whole_axis_pivot_sources_survive_xlsx_with_bounded_cache_records
cargo test -p gridcraft-engine --test it whole_axis_pivot_sources
```

The XLSX test checks the worksheet-source XML, eight cache records, whole rows,
whole columns, explicit sheet-limit endpoints and sheet-local names across two
round trips. The engine test uses a separate report sheet, monthly labels and
amounts 17 and 23. After each save/reopen, it adds January amounts 5 and 7 beyond
the previous used range; Refresh All must produce January totals 22 and 29 and
grand totals 45 and 52. A fixed-range control must remain at 17 and 40.

These tests do not depend on regional date parsing, GETPIVOTDATA or source-repair
commands. External Excel reopening is outside this regression's coverage.

## Compatibility with current main

The contribution is aligned with main `3099df934eda27cd457a9b8d1525d3fc973a1b7b`.
The engine regressions run in the consolidated `it` test binary. The earlier
non-Linux desktop-theme workaround is removed from this contribution because
main provides the `NoLink` implementation; its portal timeout is retained too.

## Revision history

| Date | Change |
|---|---|
| 2026-10-24 | Align with current main and its consolidated engine tests; retain upstream desktop-theme fixes without a PR-specific change |
| 2026-10-21 | Preserve declared whole-axis sources while bounding cache generation; add generated XLSX and engine regressions |
