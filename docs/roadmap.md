# Roadmap detail

> **Last reviewed:** 2026-11-04 · **Last updated:** 2026-11-04 · **Change:** minor (what works today and what comes next, checked against the code; earlier: test command) · **Target:** Microsoft Excel (Microsoft 365)

Forward-looking plan. The one-page summary is [`ROADMAP.md`](../ROADMAP.md); the ranked work
list is [`gaps.md`](gaps.md); numbers and method are in
[`target-app-parity.md`](target-app-parity.md). Estimates are Opus 5.5 agent-hours.

## What works today

| Area | Works today |
|---|---|
| Formulas and calculation | 504 of Excel's 523 worksheet functions, dynamic arrays and spill, LET and LAMBDA, structured and 3-D references, names, iterative calculation |
| Editing | Fill handle and series, Flash Fill, Find and Replace, Go To Special, several native windows on one workbook |
| Formatting | The number-format language, cell styles, conditional formatting, table styles |
| Data | Sort, AutoFilter, Advanced Filter, Remove Duplicates, Text to Columns, Consolidate, validation, outlines, tables |
| PivotTables | Field list, layouts, grouping, show-values-as, calculated fields, Show Details, PivotCharts; slicers while the workbook is open (not saved yet) |
| Charts | 2-D chart kinds that keep their kind in `.xlsx`, trendlines, data tables, multi-level category axes, combo charts with a secondary line axis, sparklines |
| What-if and analysis | Goal Seek, Data Tables, Scenario Manager, Solver with its reports, all 19 Analysis ToolPak tools, Forecast Sheet |
| Objects and review | Pictures (floating and in cells), shapes, ink, equations, checkboxes, threaded comments, sheet and workbook protection |
| Page layout and print | Page Layout view, Page Break Preview, headers and footers, system printing, PDF export |
| Files | `.xlsx` and `.xlsm` read and write (VBA kept, not run); `.xls`, `.xlsb` and `.ods` import; password-protected workbooks open; CSV, TSV, JSON and HTML |
| Automation | Every command reachable from the CLI, MCP server and authenticated control channel; recorded scripts and typed macros |
| Platforms | macOS, Windows, Linux, BSD and the web |

## What's next

Ordered by what hurts most: first anything that loses or changes a person's data, then exchanging
files with Excel users, then the CLI and MCP servers, then the larger features. An open PR named
here is the first place to look, since it may need only review. Unless a row says otherwise, the
behaviour was checked on 2026-11-04 with `gridcraft-cli` 0.16.0 on synthetic workbooks.

### 1. Never lose or change data

| Kind | Work | Where |
|---|---|---|
| Bug | A worksheet over about 500k cells is cut off on open: a 1M-row file keeps 100,000 rows, and saving writes the shorter file. The cap counts streamed cells (`MAX_XML_NODES`). | #2526, #2819, #3286; PRs #2925, #3210, #3317 |
| Feature | Crash recovery: there is none. AutoSave is off by default and ignores a failed save. | PRs #3326, #2973 |
| Bug | Formulas saved without the dynamic-array flag spill where Excel intersects: `=ISBLANK(B:B)` shows `#SPILL!`, `=A:A*2` spills down the column. | #410; PRs #2677, #3067 |
| Bug | Dynamic arrays left stale or empty until a full recalculation. | #2053, #2340, #2320 |
| Bug | Crashes around the native save dialog, which can lose unsaved work. | #2906, #2464, #2042 |

### 2. Exchange files with Excel users

| Kind | Work | Where |
|---|---|---|
| Bug | Saved files Excel refuses: number literals out of range, sheet and defined names it can't read, a picture placed in a cell, a legacy array over a spill, overlong text. | #2947, #2009, #1663, #2339, #2949, #2635 |
| Bug | PivotTables after a save: a TRUE/FALSE column in the source makes the file unopenable; fields and layouts change. | #1447; PRs #1680, #1681, #1682, #1684, #3222 |
| Bug | The corpus run has 21 of 397 saved real-world files failing to open in Excel without an error or repair, unchanged since v0.13. The failures need sorting by cause. | #891 |
| Feature | Chart options survive a save: error bars, per-point colours, log scale, axis bounds, reversed axes and axis number formats are all dropped, even on charts nobody edited. | #1668, #3117; PRs #1290, #1895, #1219 |
| Feature | Rich text in a cell: mixed fonts and colours flatten to plain text on open and stay lost on save. | #774; PR #2268 |
| Feature | Slicers and timelines in `.xlsx`: slicers made in GridCraft aren't written, Excel's are dropped, and timelines don't exist. | [gaps §3](gaps.md), [§8](gaps.md) |
| Bug | Table and result details lost on save: table filter criteria (rows stay hidden), table formats, results of functions GridCraft can't calculate, spilled text. | #1544, #1209, #2770, #2285, #2137 |
| Feature | Save with a password. Encrypted files open, but only an unencrypted copy can be saved. | #770; PR #1587 |
| Feature | `GETPIVOTDATA` (`#NAME?` today) and calculated items. | PR #3317 |
| Bug | Regional files: text coerced with en-US separators (silent 1000× values), SORT in code-point order. | #889, #890 |
| Feature | CSV import that keeps IDs: `0042` becomes `42`, `01234` becomes `1234`. Excel's Automatic Data Conversion options, delimiter and quote handling. | #506, #1446 |
| Feature | Smaller losses: What-If scenarios, cell-linked text boxes, gradient fills. | #1444, #2805, #1818 |
| Bug | `gridcraft-cli check` reports "round trip: all equal" for a file that just lost its rich text and chart options, so it can't catch the two losses above. Compare them too. | |

### 3. CLI and MCP servers

| Kind | Work | Where |
|---|---|---|
| Feature | Make lost data impossible to miss: `run` exits 0 and MCP `open_workbook` returns `isError: false` for a sheet that was cut off, with only a warning. A strict mode that fails, and a refusal to save over the original. | #2526 |
| Bug | The MCP stdio server reads a line into an unbounded buffer before checking its size. | #2164 |
| Feature | Confine a server to chosen folders and commands. | #445; PRs #2857, #2739 |
| Bug | `execute_command` returns a bare `null` for layout commands, so an agent can't tell what changed. | #1291 |
| Feature | Render a sheet to PNG from the headless server; `screenshot` only works against a running app. | #2246 |
| Feature | Attach to a running app without a restart, and turn its control port on from the app. | #2280 |
| Feature | Push events for cell edits and selection changes. | #3060 |
| Bug | With two windows, UI input and screenshots go to the wrong one; MCP can take over a window it opened. | #3036, #3069; PR #3045 |
| Feature | A custom-function API. | #2962, [gaps §21](gaps.md) |
| Feature | Report hidden information before sharing a file; CLI and MCP setup for desktop users. | #1829, #220 |

### 4. Speed

Recalculation is single-threaded, but an edit that updates a chain of 80,000 formulas took about
0.1 s, so targeted fixes come before threading.

| Kind | Work | Where |
|---|---|---|
| Bug | Running `COUNTIF` over a growing range is quadratic: 4× the rows took about 15× the time. | #2813 |
| Bug | Clearing a cell scans every formula; Select All on a large table recomputes statistics each draw. | #2684, #3156 |
| Bug | Hangs on open. | #2751, #2183 |
| Bug | Memory per edit: 10 edits on an 80,000-formula workbook used 729 MB against 215 MB for one. Cause not yet found. | |

### 5. Larger features

| Kind | Work | Where |
|---|---|---|
| Feature | Non-Latin text in PDF export, and shaping for Arabic and Devanagari. | #446; PRs #2821, #2935; [gaps §5](gaps.md) |
| Feature | Power Query, connections, the data model and DAX. | #431; PRs #1203, #1532, #2228 |
| Feature | Running Python cells: stored and saved today, but they don't run. | [function-parity.md](function-parity.md) |
| Feature | Remaining chart kinds: 3-D, surface, Pareto, pie of pie, maps. | [chart-parity.md](chart-parity.md) |

The rest is under [After beta](#after-beta-ranked).

## Alpha gate

The core workflows a typical Excel user runs every day, checked end to end on the main platform
(macOS desktop), including saving and reopening the work. Any "no", or a "partial" that blocks the
workflow, would make GridCraft pre-alpha.

| Core workflow | Works end to end? | Evidence | Hours to pass |
|---|---|---|---|
| Build a model: enter data and formulas, recalc, save XLSX, reopen | yes | 492 working functions, dynamic arrays, LET/LAMBDA, names, tables; XLSX round-trip tests (`crates/xlsx/src/tests/roundtrip.rs`). Edge-case bugs (#57 15-digit equality, stale spills #133/#139) give wrong results in specific cases, not in the workflow | 0 (fixes are beta work, gap #6) |
| Open a colleague's XLSX, edit, save, hand it back | partial, not blocking | Typical workbooks open and save with cells, styles, CF, validation, tables, names and pivots; unmodelled parts are dropped with a warning (gap #3); Excel-side repair prompts unverified (gap #1); ~100 MB files fail (#175) | 0 for alpha (gaps #1–#3 are beta gates) |
| Format and present: number formats, styles, conditional formatting, print or PDF | yes | Full number-format language, 47 cell styles, all CF kinds, Page Break Preview, PDF export and printing. PDF can't carry non-Latin text (gap #5) | 0 |
| Analyze: sort, filter, tables, PivotTable | yes | Multi-level sort, AutoFilter, tables with totals, PivotTables with field list, layouts, grouping, refresh and XLSX round-trip. Calculated fields done; slicers missing (gap #8) | 0 |
| Chart the data | partial, not blocking | Column, bar, line, pie, area, scatter, combo, histogram, box, waterfall, funnel, treemap, sunburst and stock charts are created, edited and survive save and reopen; trendlines are drawn; no axis options (gap #4) | 0 for alpha |
| Import and export CSV | yes | Encoding, delimiter and date-order detection on read; CSV/TSV write displayed values | 0 |

**Passes:** every core workflow works end to end on macOS with save and reopen; the two partial
rows lose fidelity only on less common content (very large files, unmodelled parts, advanced
chart kinds), which are beta gates, not alpha blockers. Ready for real work ~50% is inside the
40–75% alpha band.

## Beta gates

Beta means: ~75% ready for real work, opens and saves XLSX reliably, remaining gaps are known bugs
and edge features. Each gate links its gap.

| Gate | Gap | Hours | Status |
|---|---|---|---|
| XLSX proven on a real-world corpus; Excel opens our output without repair | #1 | 15–25 | Corpus run repeats each release; failures remain |
| Unmodelled XLSX parts preserved on save | #3 | 10–15 | Most kept; slicers, timelines and the data model are not |
| 100 MB workbooks open; 1M rows usable; multi-threaded recalc; 100k-edit recalc < 100 ms | #2 | 25–40 | Open: sheets over about 500k cells are cut off |
| Charts: kinds and options survive round trip; axes, trendlines, error bars | #4 | 25–35 | Kinds and trendlines done; options open |
| Text in every script renders and exports | #5 | 12–20 | Open |
| Calculation correctness bugs closed; iterative calculation | #6 | 10–15 | Iterative done; bugs open |
| `.xls` import, password-protected workbooks | #7 | 28–42 | Import and opening done; `.xls` objects and saving with a password open |
| PivotTables: slicers, calculated fields, PivotCharts, verified in Excel | #8 | 20–30 | Calculated fields and PivotCharts done; saving slicers and Excel verification open |
| Keyboard: key tips, shortcut audit, context menus | #9 | 12–20 | In progress |
| Field crashes and freezes closed; autosave/recovery soak | #10 | 8–12 | Open: no crash recovery |
| **Total** | | **~165–255 (≈160–240 after overlap with the PR queue)** | |

With 4–5 agents and an integrator, about 40–60 wall-clock hours. Needs the owner: running Excel
to check our files, and Windows machines with the reported GPU drivers.

## Milestones

| # | Milestone | Status |
|---|---|---|
| M0 | Foundation: core, number formats, formula language, functions, model, calc, XLSX/CSV, engine | done |
| M1 | Excel look: chrome, ribbon, formula bar, grid, tabs, status bar, control channel | done (polish continues) |
| M2 | Formatting completeness | done |
| M3 | Editing power (AutoComplete, spelling, drag-move/copy) | done |
| M4 | Data (advanced filter, consolidate, what-if) | done; Solver and ToolPak in; Power Query moved to M16 |
| M5 | Formulas tab (trace arrows, watch window, evaluate, error checking) | done |
| M6 | Conditional formatting manager parity | done |
| M7 | Charts parity | partial; Format pane, trendlines and data tables in; axis options, error bars and per-point formats remain |
| M8 | Insert & Review | partial; ink, icons, equations and the comments pane in; SmartArt, WordArt and more shapes remain |
| M9 | Page layout & print | done; polish remains (gap #14) |
| M10 | PivotTables, Solver, Analysis ToolPak | done; saving slicers, timelines and calculated items remain |
| M11 | Automation (record actions, scripts) | done |
| M12 | Performance, accessibility, i18n | partial; ribbon translated, dialogs in some languages |
| M13 | Release & polish | releases shipping (v0.16.0, signed) |
| M14 | **Beta gates** (table above) | in progress; see their status |
| M15 | Localization infrastructure and the twelve languages | started; see [localization-parity.md](localization-parity.md) |
| M16 | Get & Transform, data model | in open PRs (#1203, #1532, #2228) |

## After beta (ranked)

| Work | Gap | Hours |
|---|---|---|
| Localization: catalog past the ribbon, then twelve languages | #11 | 100–165 |
| Get & Transform and the data model | #12 | 60–100 |
| Solver constraints and ToolPak chart output | #13 | 12–20 |
| Remaining functions, oracle suite | #15 | 16–26 |
| Collaboration and review | #16 | 35–55 |
| Insert objects | #17 | 25–40 |
| Accessibility | #18 | 12–18 |
| Print polish | #14 | 5–10 |
| Mobile layout | #23 | 30–60 |
| AI features with local models | #22 | 30–50 |
| Plugin / custom-function API | #21 | 25–40 |

## Measuring

- `cargo xtask parity` regenerates [`parity-checklist.md`](parity-checklist.md) (ribbon
  catalog vs registered commands); `cargo xtask ci` checks it is current.
- `cargo xtask test`: engine, formula, function, XLSX and UI behaviour (cargo-nextest when
  installed, else `cargo test`), then the doctests.
- `cargo run --release -p gridcraft-ui-egui --example snapshot -- --sample sales out.png`: look at
  the UI offscreen.
- Function coverage: the script method in [function-parity.md](function-parity.md). A
  `cargo xtask functions` that diffs the registry against a checked-in Excel name list would make
  it a CI number (1–2 h).
- No `cargo xtask scorecard` or `bench` yet.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-04 | minor | Replaced Current focus with What works today and What's next; beta gates and milestones show their current status |
| 2026-10-23 | minor | Measuring: tests run through `cargo xtask test` |
| 2026-10-11 | minor | PivotTable calculated fields landed |
| 2026-10-10 | minor | Current focus and M15 updated for the 10-10 merges |
| 2026-10-10 | minor | Added the alpha gate table (six core workflows; passes, stays alpha) |
| 2026-10-10 | major | Created: current focus, beta gates (~160–240 h), milestones moved from ROADMAP.md, post-beta ranking |
