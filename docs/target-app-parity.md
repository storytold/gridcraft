# Target-app parity: GridCraft vs Microsoft Excel

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-11 · **Change:** minor (Performance: recalc order and chain length; shared ranges and lookup indexes; multi-threaded recalc; measured with the `perf` example) · **Target:** Microsoft Excel (Microsoft 365)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[functions](function-parity.md), [charts](chart-parity.md), [file formats](file-format-parity.md),
[UI](ui-parity.md), [hardware](hardware-parity.md), [localization](localization-parity.md). The
generated ribbon checklist is [`parity-checklist.md`](parity-checklist.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Feature breadth (weighted by use) | **~75%** | estimated from the area table below; anchored on two measurements |
| Ribbon and menu catalog | **258 / 290 (89.0%)** | measured: `cargo xtask parity` (`crates/engine/src/catalog.rs` ∩ registered command ids) |
| Worksheet functions, by name | **501 / 523 (95.8%)** | measured: registered functions vs Microsoft's published function list (method below) |
| Worksheet functions that do real work | **492 / 523 (94.1%)** | measured: excludes 9 stubs (7 CUBE functions, WEBSERVICE, FILTERXML) |
| **Ready for real work** | **~50%** | estimated; written weights below (51.6 computed) |
| **Mainstream practitioner** | **~50%** | estimated; [method](#mainstream-practitioner) (69.5 depth × 0.90 × 0.92 × 0.88 = 50.6) |
| **Essentials user** | **~63%** | estimated; [method](#essentials-user) (80.2 depth × 0.92 × 0.95 × 0.90 = 63.1) |
| Stage | **alpha** | see [Stage](#stage) |
| Remaining to beta | **~160–240 Opus 5.5 agent-hours** | estimated; [roadmap.md](roadmap.md#beta-gates) |
| Remaining to full parity | **~700–1,100 Opus 5.5 agent-hours** | estimated; sum of the rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~50% | ~650–1,050 | Get & Transform and the data model, localization (twelve languages), charts depth, file fidelity (`.xls`, corpus, pass-through), PivotTables, collaboration, mobile |
| Mainstream practitioner | ~50% | ~200–310 | File exchange with Excel users (corpus and Excel verification, pass-through, large files, `.xls`, encryption: 63–97 h), common-chart depth (30–45 h), PivotTables (20–30 h), keyboard and interaction (18–28 h), calc correctness and functions (20–33 h) |
| Essentials user | ~63% | ~60–100 | Opening files people send (`.xls` and encrypted import 28–42 h; macOS Finder open 2–4 h), launch stability (6–10 h), basic chart resize and polish (6–10 h), print and Page Layout (8–12 h), small editing and UI fixes (10–16 h) |

Each tier is a subset of the one above, so its hours are too. Calibrated as in [Effort and
calibration](#effort-and-calibration) (~6–10 h per readiness point for the remaining, verification-heavy
work; single features measured from PRs #66, #100, #101). Parallelism: the full and mainstream
work splits ~65–70% across 4–6 agents by area; the essentials work is mostly small fixes plus one
long `.xls` track, so about half parallelizes (≈30–50 wall-clock hours with three agents).

## What we measured against, and how

- **Target:** Microsoft Excel (Microsoft 365 channel). Version measured: **Excel for Mac
  16.113.4**, installed on the owner's Mac (version read from Spotlight metadata with `mdls`).
  Where Excel for Windows has more (Power Pivot, VBA editor depth, COM add-ins, 3D Maps), the
  Windows feature counts as part of the target.
- **Clean-room limits.** This repo's rule (AGENTS.md, "Non-negotiables") forbids reading anything
  inside the Office bundle, so the bundle's `Info.plist`, `.lproj` folders and resources were
  deliberately **not** inspected, and Excel was not launched for this pass. Excel's file formats,
  functions and features come from Microsoft's public documentation, the owner's earlier
  black-box observation (`plan/excel/observed-ui.md`, local only), and knowledge of the product.
- **Function list:** Microsoft's "Excel functions (alphabetical)" support page, fetched
  2026-10-10 and parsed (506 rows), plus 15 names the page lists under combined entries (FIND/FINDB,
  LEFT/LEFTB, LEN/LENB, MID/MIDB, REPLACE/REPLACEB, RIGHT/RIGHTB, SEARCH/SEARCHB, BETA.INV) and
  COPILOT and PY: **523 functions**. Ours: every `f!(…)` spec in `crates/functions/src/*.rs` plus
  `SPECIAL_FUNCTIONS` in `crates/calc/src/eval.rs` (evaluator special forms), minus the test-only
  `ADD2`: **501**, all of them Excel names.
- **Ribbon catalog:** `crates/engine/src/catalog.rs` (290 unique ids across 15 tabs) against the
  313 command ids registered in `crates/engine/src/cmd/*.rs`, recomputed from source by script and
  identical to the committed `parity-checklist.md`. The catalog is our own transcription of
  Excel's main ribbon tabs; it leaves out most contextual tabs (Picture Format, Shape Format,
  Header & Footer, Sparkline, Slicer, Timeline), the Developer and Help tabs and sub-menu items, so
  89% of it is roughly **60–65% of Excel's full command surface** (estimated).
- **Depth and readiness:** source reading (the model types in `crates/model/src/features.rs`,
  readers and writers in `crates/xlsx/src`, the evaluator), 478 `#[test]` functions, and the
  issue tracker: 60 open issues and 50 open PRs on 2026-10-10, 82 issues filed in total.
- No cargo build or test ran for this pass (disk constraints); numbers come from source.

## Weights

Percentages are weighted by what Excel users actually do, not by item count. Feature-area
weights (sum 100) reflect a typical business, finance and analyst workload: formulas, functions,
editing and formatting dominate; Power Query, VBA and the data model matter to fewer users but
are decisive for those who use them.

Dimension weights for **ready for real work**: features 45, file formats 15, UI/UX 12,
performance 8, stability 8, localization 4, hardware 2, platforms 2, ecosystem 2, AI 2.

## Feature areas

| Area | Weight | Breadth | Ready | Hours | Evidence |
|---|---|---|---|---|---|
| Formula language and calculation | 18 | ~92% | ~72% | 20–30 | A1/R1C1 parser, reference adjustment, dependency-graph recalc, dynamic arrays and `#SPILL!`, `A1#`, LET/LAMBDA and helpers, structured references, names, 3-D references. Open correctness bugs: 15-digit equality (#57), stale spills after structural edits and cleared blockers (#133, #139), 3-D span endpoint deletion (#140), sheet rename and qualified names (#134). Single-threaded |
| Worksheet functions | 12 | 95.8% (measured) | ~82% | 18–30 | 501 / 523 names, 492 working. Missing: FORECAST.ETS family, ODDF*/ODDL*, GETPIVOTDATA, TRIMRANGE, PERCENTOF, BAHTTEXT, IMAGE plus cloud functions. No conformance suite against Excel-computed oracles; edge-case bugs keep surfacing (#137, #138). [function-parity.md](function-parity.md) |
| Editing, navigation, clipboard, fill | 12 | ~90% | ~70% | 15–25 | Point mode, F4, AutoComplete, Paste Special, AutoFill, Flash Fill, Find/Replace, Go To Special. Bugs: rectangular drag-select (#28), monthly fill (#132), cut across sheets (#128), Insert Copied Cells missing (#21), key tips only on part of the Home tab (#43; #33, #49). [ui-parity.md](ui-parity.md) |
| Cell formatting, styles, conditional formatting | 10 | ~95% | ~80% | 8–14 | Full number-format language, borders, 47 cell styles, 60 table styles, all CF rule kinds including data bars, colour scales and icon sets. Gaps: font weights beyond regular/bold (#168), system fonts list (#4), themes |
| Sort, filter, tables, validation, outline | 8 | ~82% | ~70% | 10–16 | Multi-level sort, AutoFilter, Advanced Filter, tables with totals, validation, outline, subtotals, consolidate. Bugs: subtotal header (#131), validation edits (#129), table style panic caught (#130); no slicers on tables |
| PivotTables | 7 | ~65% | ~45% | 25–40 | Insert/Recommended, field list, three layouts, date grouping, show-values-as, refresh, XLSX round-trip. Missing: slicers, timelines, calculated fields/items, PivotCharts, value/label filters, number grouping, GETPIVOTDATA, data-model pivots. Round-trip never checked in Excel |
| Charts and sparklines | 8 | ~50% | ~30% | 40–60 | 25 chart kinds, all 2-D. The chart model holds title, legend position, a data-labels flag, gridlines flag and axis titles; no axis scaling, trendlines, error bars, per-point formatting, chart sheets, 3-D, surface, map, Pareto. Several kinds don't survive XLSX round trip (open PRs #158, #161). [chart-parity.md](chart-parity.md) |
| Page layout, print, PDF | 5 | ~80% | ~55% | 12–20 | Page setup, print titles, breaks, Page Break Preview, headers/footers, PDF export and printing. No true Page Layout view; the PDF writer uses only the 14 standard fonts without embedding (no CJK or other non-Latin text in PDF); web print only exports (#141, #143) |
| What-if and analysis | 4 | ~50% | ~40% | 22–35 | Goal Seek, Scenario Manager, data tables. Missing: Solver, Analysis ToolPak, Forecast Sheet, Analyze Data |
| Get & Transform, external data, data model | 5 | ~15% | ~10% | 60–100 | Import of CSV/TSV, JSON, HTML. No Power Query editor, no From Web, database or folder sources, no connections or refresh, no relationships or data model, no external workbook links (dropped with a warning) |
| Insert objects | 3 | ~55% | ~45% | 25–40 | Pictures, 8 shape kinds (Excel has ~170), text boxes, own icon set, ink with Ink to Shape, hyperlinks, checkboxes. Missing: SmartArt, WordArt, equations, signature line, objects, 3-D models, screenshot, pictures in cells (PR #105) |
| Review, comments, protection, collaboration | 4 | ~55% | ~40% | 35–55 | Threaded comments, notes, spelling (system dictionary), thesaurus, sheet and workbook protection. Missing: comment navigation, Show Changes, Allow Edit Ranges, notes-to-comments conversion, accessibility checker, co-authoring, opening password-encrypted files (refused with a message since #17) |
| Automation | 4 | ~30% | ~20% | 12–20 | Recorded, editable GridCraft scripts (Automate tab). VBA is out of scope by owner decision and is **dropped** when an `.xlsm` is opened (warning shown). No Office Scripts compatibility |
| **Weighted** | 100 | **~75%** | **~59%** | **300–485** | |

## Dimensions

| Dimension | Weight | Ready | Hours | Evidence |
|---|---|---|---|---|
| Features | 45 | ~59% | 300–485 | Table above |
| File formats | 15 | ~50% | 80–120 | XLSX read/write is broad but unproven on a real corpus and in Excel; XLSB and ODS are values-only import; no XLS, XML Spreadsheet 2003, SYLK, DIF, PRN; VBA dropped; 100 MB XLSX fails (#175). [file-format-parity.md](file-format-parity.md) |
| UI/UX fidelity | 12 | ~55% | 50–80 | Excel-style ribbon, galleries, dialogs, formula bar, status bar; 59 commands with shortcuts vs Excel's 200+; key tips on part of the Home tab only; short context menu; single window. [ui-parity.md](ui-parity.md) |
| Performance | 8 | ~35% | 35–55 | [Performance](#performance) |
| Stability | 8 | ~60% | 15–25 | Never-crash rules enforced by lint, every command fuzzed with hostile params, last-resort guard in `Session::execute`. Field reports: Windows Intel GPU crash (#120), error on close (#117), freeze opening from Finder (#166), screen flashing (#76) |
| Localization | 4 | ~10% | 100–165 | Ribbon-only catalog in five languages (ja, zh, ko, pt-BR, ru; ~12% of strings each), CJK/Thai/Hebrew font fallback, no shaping or RTL. [localization-parity.md](localization-parity.md) |
| Hardware | 2 | ~40% | 25–40 | GPU UI via wgpu; single-threaded calc; no pen pressure; one window. [hardware-parity.md](hardware-parity.md) |
| Platforms | 2 | ~80% | 30–60 | [Platforms](#platforms) |
| Ecosystem / add-ins | 2 | ~10% | 25–40 | [Ecosystem and AI](#ecosystem-and-ai) |
| AI features | 2 | ~15% | 30–50 | [Ecosystem and AI](#ecosystem-and-ai) |
| **Ready for real work** | 100 | **~50%** (51.6 computed: 0.45 × 59.0 features + the other rows × their weights) | **~700–1,100** | |

How the full number is built: features ready is the weighted mean of the area table (59.0 with
charts at 30%); ready for real work is the weighted mean of this table with the weights written
under [Weights](#weights). Re-checked after the 10-10 merges: charts were corrected from 38% to
30% (round-trip loss, #190 chart resize) and localization rose from 5% to 10% (#31, #104, #82,
#38). The two moves cancel: 51.6 before and after, so **~50%** stands.

## Mainstream practitioner

The typical professional Excel user: an analyst, accountant or manager who builds and maintains
models, cleans and summarizes data, makes charts and pivots, and exchanges workbooks with Excel
users every week. Left out: Power Query and the data model, VBA and add-ins, Copilot and cloud
functions, co-authoring and admin, specialist hardware, and languages beyond the user's own
(localization is reported separately).

| Area (weekly use) | Weight | Depth | Basis |
|---|---|---|---|
| Formula language and calculation | 20 | 72 | Feature-area table |
| Common worksheet functions | 15 | 85 | Missing functions are niche (ETS, odd-period bonds, cloud); edge bugs in common ones (#137, #138, #57) |
| Editing, navigation, clipboard, fill | 15 | 70 | Feature-area table |
| Cell formatting, styles, conditional formatting | 13 | 80 | Feature-area table |
| Sort, filter, tables, validation | 10 | 70 | Feature-area table |
| PivotTables (core: fields, layouts, grouping, refresh) | 8 | 60 | Slicers and calculated fields excluded as less frequent |
| Charts (column, bar, line, pie, scatter, combo) | 9 | 45 | Common kinds work and survive save; no axis options or trendlines; resize broken (#190) |
| Page layout, print, PDF | 5 | 60 | Latin text exports; no Page Layout view |
| Comments, notes, protection | 3 | 55 | Feature-area table |
| Pictures and shapes | 2 | 45 | Feature-area table |
| **Average depth** | 100 | **69.5** | |

Discounts for what still stops real work:

| Discount | Factor | Evidence |
|---|---|---|
| Interaction fidelity | × 0.90 | 59 command shortcuts vs Excel's 200+; key tips only on part of the Home tab (#43); rectangular selection bug (#28); charts can't be resized (#190); one window (#162); missing horizontal scrollbar report (#176) |
| Stability on real machines | × 0.92 | Never-crash lints, fuzzed commands and the `Session::execute` guard; but field reports of a startup crash on Intel UHD (#120), an error on close (#117), a freeze opening from Finder (#166) and screen flashing (#76) |
| File exchange with Excel users | × 0.88 | Excel opening our output is unverified across features; unmodelled parts dropped on save; advanced chart kinds lose their type; 100 MB files fail (#175); `.xls` and encrypted files are refused with a clear message (#17) rather than opened. Harsher than a drawing app's discount because spreadsheets are exchanged constantly |

**Mainstream practitioner: 69.5 × 0.90 × 0.92 × 0.88 = 50.6 → ~50%** (estimated). Unlike most
apps it is not higher than the full number: GridCraft's breadth is wide (functions, formatting,
ribbon), so excluding the long tail removes little, while the discounts land on exactly what
mainstream users do every week (exchange files with Excel users).

## Essentials user

Someone who only touches the core: opens a workbook someone sent, types data and simple formulas,
formats, sorts, makes a basic chart, saves and prints. Advanced options, pro workflows and exchange
edge cases are left out, along with everything excluded from the mainstream number.

| Core feature | Weight | Depth | Basis |
|---|---|---|---|
| Typing data, simple formulas, AutoSum (SUM, AVERAGE, arithmetic) | 20 | 88 | Solid; 15-digit equality (#57) is rare at this level |
| Basic formatting: fonts, bold, fills, borders, currency/percent/date formats | 15 | 85 | |
| Copy, paste, fill handle, undo/redo | 15 | 78 | Selection bug (#28), monthly fill (#132) |
| Rows and columns: insert, delete, widths, AutoFit | 10 | 82 | AutoFit fixed (#42, #88) |
| Sort and filter | 10 | 82 | |
| Save, Save As, reopen XLSX | 10 | 85 | |
| A basic chart | 8 | 55 | Created easily; can't be resized (#190) |
| Print and PDF | 7 | 65 | Desktop works; web PDF gives no file or feedback (#141) |
| Find and replace | 5 | 85 | |
| **Average depth** | 100 | **80.2** | |

| Discount | Factor | Evidence |
|---|---|---|
| Launch and stability | × 0.92 | Startup crash on some Intel GPUs (#120), "Issue executing" (#59), error on close (#117) |
| Discoverability and UI clarity | × 0.95 | The Excel-style ribbon is familiar and labelled; system language by default (#31); small gaps (#176 scrollbar, #170 macOS window controls) |
| Opening files people send them | × 0.90 | Double-clicking a workbook in Finder freezes or errors on macOS (#166, #69; drag-and-drop works; fixes in PRs #164, #167); `.xls` and encrypted files are refused with a message |

**Essentials user: 80.2 × 0.92 × 0.95 × 0.90 = 63.1 → ~63%** (estimated).

## User evidence

Counted from all 83 GitHub issues and 109 PRs on 2026-10-10; none of the issues were filed by
the maintainer.

- **Praise:** 3 explicit ("this is brilliant. Tiny fast native binaries", #16; "Thanks for the
  great work on GridCraft", #61; "i didnt have excel kind app on my macos", #69), plus polite
  thanks in feature requests (#39, #45, #142).
- **"Switched from Excel" reports:** 0. One counter-signal: "without macro support most
  professionals cannot switch from excel" (#6).
- **Issues by kind:** 50 core-path bugs (30 open, 20 closed) and 33 niche or feature requests
  (25 open, 8 closed). 18 of the core-path bugs came from one auditing contributor (coygeek).
- **Contributors:** 104 PRs from 28 people outside the maintainer in five days, which shows
  strong interest but not yet daily use.

## Performance

Excel recalculates on every core (multi-threaded recalculation), streams large files, and handles
1,048,576 × 16,384 sheets with millions of formulas.

| Measure | GridCraft | Excel | Kind |
|---|---|---|---|
| Sheet size limits | 1,048,576 × 16,384 | same | measured (`crates/core/src/addr.rs`) |
| Recalc threads | all cores (or a manual count, or off: Excel's Calculation Options, saved as XLSX `concurrentCalc` / `concurrentManualCount`); 1 on the web | all cores | measured: each independent level of the dependency graph with 512+ formulas is split across threads; results match one thread exactly (test `multi_threaded_recalc_matches_one_thread`) |
| 10,000 independent formulas over a 1,000-cell array each, recalc after an edit | 1.89 s on 1 thread, 0.99 s on 2, 0.57 s on 4, 0.35 s on all 10 cores | scales with cores | GridCraft measured: `perf` example, `heavy`, Apple M2 Pro (6 performance + 4 efficiency cores) |
| Edit → recalc, 100k dependents | ~50 ms (2026-10-11; was ~0.4 s on 2026-10-07) | tens of ms | measured: `perf` example, `basic`, 50,000 rows (a formula column and a running sum), Apple M2 Pro |
| Long dependency chains | correct in any direction and at any length (topological order; formulas wait on a heap stack, not the call stack) | correct | measured: `perf` example, `chain`; tests `long_chains_*`. Before 2026-10-11 a chain read against row order past 2,000 cells gave `#CIRC!` |
| Exact lookups, 100,000 VLOOKUPs over a 100,000-row table | 0.72 s (XLOOKUP 0.76 s, MATCH 0.63 s); an edit to the table with 300,000 dependent lookups 0.25 s. Before 2026-10-11: 2.6 s for 10,000, growing as n² | well under 1 s | GridCraft measured: `perf` example, `lookup`, 500,000 rows, Apple M2 Pro; Excel not measured. A range read by many formulas is built once per recalculation and lookups index it |
| 1,000 × `SUM(A:A)` over 50,000 rows, recalc after an edit | 20 ms on all cores (86 ms on one) | fast | GridCraft measured: `perf` example, `colsum` |
| Grid scrolling | virtualised, O(log n) geometry with custom row heights | smooth | measured in code |
| Open a ~100 MB XLSX | fails (#175) | opens | user report |
| Whole-sheet operations | Fill and Remove Duplicates on a whole-sheet selection run out of memory or time (fix in PR #152) | fine | open PR |

Work: streaming XLSX reader with shared-string and style dedup, recalculation off the UI thread
with interruption and progress, whole-column reference clamping (#205). The numbers above come from
`cargo run --release -p gridcraft-engine --example perf -- [rows]`, which checks every answer.

## Platforms

| Platform | Excel | GridCraft |
|---|---|---|
| Windows x64 / arm64 | yes | yes (signed MSI and portable zip; also x86) |
| macOS (Apple silicon, Intel) | yes | yes (signed, notarized universal DMG) |
| Web | Excel for the web | yes (WASM; local files, no cloud) |
| iOS / iPadOS / Android | yes | no |
| Linux | no | yes (AppImage, Flatpak, deb, rpm, tarball; x86_64, aarch64, riscv64) |
| FreeBSD | no | yes |

Mobile is the gap (30–60 h for a touch layout over the same engine). GridCraft is ahead on
Linux, BSD and RISC-V, where Excel doesn't run.

## Ecosystem and AI

- **Add-ins:** Excel has Office JavaScript add-ins, COM/XLL add-ins, VBA, Office Scripts and
  Power Automate. GridCraft has its own recorded scripts, plus a control channel, MCP server and
  CLI that reach every command. VBA is out of scope (owner decision). A plugin or custom-function
  API (for example LAMBDA libraries and WASM custom functions) is the open-equivalent path: 25–40 h.
- **AI:** Excel has Copilot, the COPILOT() function, Analyze Data, Python in Excel and Flash
  Fill. GridCraft has Flash Fill and best-in-class agent control (any MCP client can drive every
  command headless or live), but no in-app assistant, no natural-language formulas and no
  Analyze Data. Local-model equivalents: 30–50 h; model choice needs the owner.

## Stage

**Alpha.** All six core workflows in the [alpha gate](roadmap.md#alpha-gate) pass end to end on macOS with save and reopen (two pass with non-blocking fidelity loss). Core workflows exist end to end, but depth, fidelity and file compatibility are rough
(ready ~50%, inside the 40–75% alpha band). Not beta: the main format, XLSX, has not been proven
on real-world files or confirmed to open in Excel without repair, large files fail, `.xls` is
missing and text in several scripts doesn't render. Distance to beta: ~25 points of readiness and
~160–240 h, itemized in [roadmap.md](roadmap.md#beta-gates).

## Effort and calibration

Hours are **Opus 5.5 agent wall-clock hours**, one agent working sequentially, including tests
and verification.

Calibrated against this repo's history:

- **First pass, 2026-10-05 → 10-07:** M0–M13 (about 72,000 lines of Rust: engine, 478 functions,
  XLSX, UI, charts, PivotTables, PDF, MCP, CLI, web, packaging) landed in commits from 22:09 on
  10-05 to 02:08 on 10-06, built by several parallel agents with work that preceded the first
  commits; we put it at **~50–80 agent-hours** for 0 → ~50% ready, so **~1–1.5 h per point**
  for the broad first pass.
- **Recent single features:** ODS import (#100, +2,232 lines with tests) and XLSB import (#101,
  +1,212 lines) each fit in one agent session of roughly 2–3 h; the linear-recalc fix (#66,
  +477 lines) about 1 h; single-function fixes (ROUND, DATE 1900, SUMIF sizing) 0.3–0.5 h each.
  That is roughly **600–1,000 lines per agent-hour** including tests.
- **Tail factor:** what remains is verification-heavy (Excel as the oracle for files, functions
  and rendering), and much of it is large self-contained subsystems (Power Query, Solver,
  localization, `.xls`). We apply **5–7× the first-pass rate**: ~6–10 h per point of readiness.

**Parallelism:** ~70% of the work splits cleanly by area (functions, charts, pivots, formats,
localization, Power Query) across 4–6 agents with one integrator; with five agents, beta is about
40–60 wall-clock hours away. **Needs a human:** checking our files in Excel (the owner's Mac),
native-speaker review of every language, scope decisions (co-authoring, cloud functions,
in-app AI models, mobile), and Windows hardware for GPU-driver crashes.

## What works today

The inventory carried over from the 2026-10-07 ROADMAP.md, updated for what landed since. Presence, not parity: the tables above grade depth.

- Engine: A1/R1C1 formula parser with reference adjustment, dependency-graph recalculation with
  dynamic arrays (spill + `#SPILL!`), cycle detection, LET/LAMBDA/MAP/REDUCE/SCAN/BYROW/BYCOL/
  MAKEARRAY, structured table references, defined names, INDIRECT/OFFSET/INDEX references,
  501 worksheet functions (478 at M0) (math, statistics and distributions, text and regex, lookup incl.
  XLOOKUP/FILTER/SORT/UNIQUE/VSTACK, dates, financial incl. bonds, engineering incl. complex
  numbers and Bessel, database, information).
- Number formats: Excel's full format-code language (sections, conditions, colours, dates and
  elapsed time, fractions, scientific, accounting fill), General narrowing to column width.
- Files: XLSX read/write (styles, themes, merges, CF, validation, tables, comments, hyperlinks,
  charts, pictures, sparklines, print settings, defined names, shared/array/dynamic formulas),
  CSV/TSV with encoding and delimiter detection, HTML export, JSON; XLSB and ODS data import (values only, 10-10).
- Editing: cell entry with type detection and auto-formats, in-cell and formula-bar editing,
  point mode (click/drag/arrow references), reference colouring, F4 anchor cycling, function
  autocomplete and argument hints, copy/cut/paste and Paste Special (values, formats, transpose,
  operations, link, skip blanks), AutoFill series (numbers, dates, months, weekdays, custom lists,
  text+number), Fill Series, Flash Fill, find/replace with wildcards, Go To / Go To Special,
  undo/redo with history, format painter. Quick selection drags retain their final endpoint;
  the fill handle explains how to fill versus select cells.
- Formatting: fonts, fills, all border styles, alignment (wrap, indent, rotation, merge), number
  formats, 47 cell styles, 60 table styles, conditional formatting (cell rules, text, dates,
  duplicates, top/bottom, averages, data bars, colour scales, icon sets, formulas), automatic
  row heights, autofit, hide/unhide, freeze panes.
  Border presets include visual diagrams; Format Cells previews draft borders before applying.
- Data: sort (multi-level, by colour, custom lists), AutoFilter (values, custom, top 10,
  average, colour), tables with totals rows, remove duplicates, text to columns, data validation
  with error alerts, grouping/outline, subtotals.
- PivotTables: Insert/Recommended PivotTables, field list pane, compact/outline/tabular layouts,
  date grouping, sort, filters, show-values-as, refresh, and XLSX round-trip.
- Page layout & print: Page Break Preview, print settings, PDF export and printing.
- What-if: Goal Seek, scenarios, data tables. Spelling (system word list) and thesaurus.
- Draw: pen, highlighter, eraser, ink strokes and Ink to Shape. Task panes: Comments (threaded
  replies, resolve), Watch Window, Selection, Format Chart.
- Charts: column, bar, line, area, pie, doughnut, scatter, bubble, radar, histogram, waterfall,
  funnel, treemap, sunburst, box & whisker, stock and combo charts; sparklines.
- UI (egui): Excel-style title bar with Quick Access Toolbar and AutoSave, the full ribbon with
  galleries (cell styles, table styles, colour palettes, conditional formatting menus),
  contextual Table/Chart Design tabs, formula bar with Name Box, virtualised grid (smooth
  scrolling, zoom, frozen panes, text overflow), sheet tabs (rename, reorder, colour, hide),
  status bar with Sum/Count/Average and zoom, dialogs (Format Cells, Formula Builder, Find and
  Replace, Sort, Name Manager, Data Validation, Paste Special…), command palette, dark mode.
- Agents: every action is an engine command; JSON control channel in the desktop app (pointer,
  keyboard, dialogs, screenshots); MCP server (headless or bridged); CLI (`info`, `convert`,
  `eval`, `cat`, `run`, `commands`, `functions`, `mcp`, `send`).
- Platforms: macOS, Windows, Linux, FreeBSD; web (WASM via trunk). Release workflows for
  signed/notarized macOS universal DMG, signed Windows x64/x86 MSI + zip, Linux AppImage/deb/rpm/
  tar.gz + Flatpak manifest, FreeBSD tarball and a web zip.

## Not measured yet

- Pixel comparison against Excel screenshots (planned; screenshots stay in `plan/excel/`).
- Function results against Excel-computed oracles (a corpus repo is the planned home).
- Recalc and open-time benchmarks on fixed workbooks (no `cargo xtask bench` yet).
- Whether Excel opens our XLSX output without repair, across features.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-11 | minor | Performance: multi-threaded recalculation (every core by default; Calculation Options and XLSX `concurrentCalc`/`concurrentManualCount` like Excel; one thread on the web); 10,000 heavy formulas 5.4× faster on 10 cores |
| 2026-10-11 | minor | Performance: ranges read by many formulas are shared per recalculation and lookups (VLOOKUP, HLOOKUP, LOOKUP, MATCH, XLOOKUP, XMATCH) index them: 100,000 VLOOKUPs over 100,000 rows in 0.72 s, was O(n²) |
| 2026-10-11 | minor | Performance: recalc evaluates in topological order without recursion, so long chains are correct in either direction; edit-recalc re-timed at ~50 ms for 100k formulas; lookup scaling measured (O(n²)). Numbers from the extended `perf` example |
| 2026-10-10 | minor | Readiness-by-audience table: hours to ~95% for each of the three numbers (full 650–1,050, mainstream 200–310, essentials 60–100). Full number confirmed as the additive weighted sum over the written dimension weights (no change) |
| 2026-10-10 | minor | Added Mainstream practitioner (~50%) and Essentials user (~63%) with written weights and discounts, and user-evidence counts; re-checked the full number after the 10-10 merges (charts 38→30, localization 5→10; still 51.6, ~50%) |
| 2026-10-10 | minor | Stage checked against the new alpha gate (six core workflows, all pass): stays alpha |
| 2026-10-10 | major | Created. Full re-measure against Excel for Mac 16.113.4: functions 501 / 523 measured against Microsoft's list, catalog 258 / 290, breadth ~75%, ready ~50%, alpha; replaces the estimate tables previously in ROADMAP.md and keeps its "Working today" inventory |
