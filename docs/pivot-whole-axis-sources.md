# Keep whole-axis PivotTable sources on XLSX save

> **Last updated:** 2026-10-28

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

The declared source also fixes the cache's header row and first field. If a
whole-row source's used cells begin at C, the cache retains the empty A/B fields
instead of assigning C the field index 0. A whole-column source retains its full
declared width, including unused leading or trailing columns. Whole-row sources
stop at the rightmost used column while retaining their declared first column.
The existing writer defaults (`Column1`, `Column2`, ...) apply at the declared
header row; the first used row is never silently promoted to a header. This does
not change PivotTable source-header validation in the engine.

Only candidate record rows in the used intersection are visited; empty leading
rows need no scan. The existing 10-million-cell source limit includes the header
and every padded field across those candidate rows, checked before building
fields. A resolved source that exceeds this limit now returns an explicit XLSX
save error instead of silently omitting the PivotTable. File Save and Save As
serialize before writing: a refused save preserves an existing destination file,
the document's saved snapshot and path, and its unsaved changes. Save Bytes also
returns the error instead of a partial workbook. Sources that cannot be resolved
retain the writer's existing behavior; this change concerns the source-work limit.
The two-million-cell records-snapshot limit also counts padded fields;
over it, the cache definition is retained without a records snapshot. Fixed
rectangles and table sources keep their prior bounds. Saving two whole columns
does not allocate a million cache rows.

## Reproduce with generated data

The tests construct all workbooks in Rust, without private data or binary fixtures:

```sh
cargo test -p gridcraft-xlsx --lib whole_axis_pivot_sources_survive_xlsx_with_bounded_cache_records
cargo test -p gridcraft-xlsx --lib pivot_cache_
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

Additional sparse-source regressions check field identities, row/data-field
indexes and decoded records through two saves. They cover whole rows with empty
leading columns, whole columns with empty leading rows and columns, origins other
than A1 (`C:F` and `5:9`), local/global defined names and a fixed-rectangle control.
Generated wide sparse cases verify the source limit and records-snapshot limit
after accounting for padded fields. Two cells at `XFD1` and `XFD2000` in an
`A:XFD` source exceed the source-work limit and must fail serialization. A separate
case exceeds only the records-snapshot limit and must retain its PivotTable and
declared source through save/reopen. An engine regression tries Save, Save As and
Save Bytes with an over-budget cache: existing generated XLSX files remain
byte-identical, and the workbook, saved snapshot, path, dirty state and undo/redo
history stay unchanged. These tests cover the writer's existing
placeholder-header behavior; they do not claim that an empty source header becomes
a valid input for Refresh or that a desktop Excel comparison was performed.

## Compatibility with current main

The contribution is aligned with main `43284377d2466037628e57d8ad0c891778fdb45c` (v0.13.0).
Cache generation follows the upstream `pivot/write.rs` split, including its field
and grouping helpers. The extracted `write_sheets` helper propagates cache-limit
errors before package publication; the upstream worksheet writer remains intact.
Shared helpers and the reader remain in their upstream modules.
The engine regressions run in the consolidated `it` test binary. The earlier
non-Linux desktop-theme workaround is removed from this contribution because
main provides the `NoLink` implementation; its portal timeout is retained too.
Upstream shared-window active-sheet access, original sheet IDs and the PivotTable
Enable show details option are retained alongside cache-budget error propagation.

## Revision history

| Date | Change |
|---|---|
| 2026-10-28 | Align with v0.13.0 main; retain extracted cache/grouping and sheet-writing helpers, propagating save errors without changing either cache limit |
| 2026-10-27 | Move cache serialization into the upstream writer module; preserve declared origins, bounded scans, save failures and all existing synthetic regressions |
| 2026-10-26 | Align with main at `f6b0cec`, retaining shared-window active-sheet handling, original sheet IDs and Enable show details serialization alongside declared sources, cache origins and unchanged save limits |
| 2026-10-25 | Reject XLSX saves that exceed the PivotTable source-work limit instead of silently omitting the pivot; preserve existing files and unsaved document state, without raising either cache limit |
| 2026-10-25 | Document whole-axis worksheet-source serialization and the generated round-trip test contracts; behavior and coverage thresholds unchanged |
| 2026-10-25 | Preserve declared header and field origins independently of bounded record scans; count padded fields in source and snapshot limits, with sparse-source regressions |
| 2026-10-24 | Align with current main and its consolidated engine tests; retain upstream desktop-theme fixes without a PR-specific change |
| 2026-10-21 | Preserve declared whole-axis sources while bounding cache generation; add generated XLSX and engine regressions |
