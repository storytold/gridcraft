# GridCraft roadmap

GridCraft aims at full Microsoft Excel parity — and to be better: faster, open (XLSX is the
native format), fully scriptable by agents (MCP + control channel + CLI), and available on the
web.

## Status (2026-10-07)

### Where we are

| Measure | Value | Notes |
|---|---|---|
| Ribbon/menu command catalog | **89%** (258 / 290) | `cargo xtask parity` → [`docs/parity.md`](docs/parity.md). Home, Draw, Page Layout, Formulas, Automate, Chart Design and PivotTable Design are at 100%. |
| Worksheet functions | **~93%** of Excel's ~545 | Missing: ODDF*/ODDL*, BAHTTEXT, IMAGE, TRANSLATE, COPILOT/PY, STOCKHISTORY, RTD |
| Feature depth (weighted) | **~65%** | The catalog counts a command once it exists; many are shallower than Excel's (see "What's left") |
| Tests | 332 passing | engine, formula, functions, calc, XLSX (incl. PivotTable round-trip), PDF, MCP, CLI, kittest UI |

The catalog number is generous: it says "this button does something real", not "it does
everything Excel's does". The weighted depth figure is our honest estimate of how much of what an
Excel power user relies on works as well as in Excel.

### How close to an alpha?

**About 80% of the way to an alpha. Estimate: ~35 wall-clock hours of Claude Opus 5.5 agent
work** (2–3 days with 2–3 parallel agents), plus a few minutes of human time to set up release
secrets.

Alpha means: someone can download a signed build, open their real workbooks, edit and save them
without losing data, and not hit a crash in a normal session. The gates:

| Alpha gate | State | Est. hours |
|---|---|---|
| Signed release builds on every platform | Workflows and packaging ready; **blocked on the `release` environment and secrets, which need a human** (see [`docs/releasing.md`](docs/releasing.md)). Then a test build per platform | 4 |
| XLSX fidelity on real files | Round-trip tests pass for our own files. Needs a corpus run on real-world workbooks, and confirming Excel opens our output (including PivotTables) with no repair prompt | 10 |
| Performance | Virtualised grid and O(log n) geometry are in. Still need: 100k-cell edit recalc under 100 ms (currently ~0.4 s), faster write-back, 1M-row open | 8 |
| Never-crash soak | Every-command fuzz test exists. Needs a longer randomised UI soak and an autosave/recovery check | 4 |
| Papercuts | Dark-mode row/column headers, a true Page Layout (paper) view, focus edge cases, error messages | 6 |
| Docs and first-run | README, CLI/MCP docs exist; needs a "getting started" page and release notes | 3 |
| **Total** | | **≈ 35** |

### How far to 100% parity?

**Estimate: ~180 wall-clock hours of Claude Opus 5.5 agent work beyond the alpha** (about 1–1.5
weeks running continuously with 3–4 parallel agents), so **≈ 215 hours in total from today**.
Breakdown under "What's left" below.

Scope decisions that affect the count: VBA is out of scope (GridCraft has its own recorded and
editable scripts instead), and features that need Microsoft cloud services (Copilot, live Stocks/
Geography data types, co-authoring through OneDrive, Smart Lookup, Translate) will be replaced
by open equivalents or left out, not cloned.

### What's left

| Area | What's missing | Est. hours |
|---|---|---|
| PivotTables | Slicers and timelines, calculated fields/items, PivotCharts, value/label filters, number grouping | 24 |
| Charts | Trendlines, error bars, full axis options (log scale, units, crossing), element-level Format pane, chart sheets, Recommended Charts dialog, maps | 26 |
| Data | Solver, Analysis ToolPak, Forecast Sheet, Get & Transform beyond CSV/JSON/HTML (Power Query editor lite), relationships / data model | 34 |
| Page layout & print | True Page Layout view, header/footer editor on the page, print preview polish | 12 |
| Insert & Review | Equations, WordArt, SmartArt-lite, signature line, comment navigation, Show Changes, Allow Edit Ranges, notes ↔ comments conversion | 20 |
| XLSX depth | Remaining OOXML parts (form controls, slicer caches, external links, OLE stubs), round-trip of everything we don't model | 16 |
| Functions | The remaining ~35 functions, exact Excel edge-case behaviour across the long tail | 10 |
| Performance | Parallel recalc, 1M-row workbooks everywhere (sort, filter, fill), memory | 14 |
| Fidelity | Side-by-side pixel tuning against Excel, keyboard shortcut completeness, accessibility, localisation | 24 |
| **Total** | | **≈ 180** |

**Working today**
- Engine: A1/R1C1 formula parser with reference adjustment, dependency-graph recalculation with
  dynamic arrays (spill + `#SPILL!`), cycle detection, LET/LAMBDA/MAP/REDUCE/SCAN/BYROW/BYCOL/
  MAKEARRAY, structured table references, defined names, INDIRECT/OFFSET/INDEX references,
  478 worksheet functions (math, statistics and distributions, text and regex, lookup incl.
  XLOOKUP/FILTER/SORT/UNIQUE/VSTACK, dates, financial incl. bonds, engineering incl. complex
  numbers and Bessel, database, information).
- Number formats: Excel's full format-code language (sections, conditions, colours, dates and
  elapsed time, fractions, scientific, accounting fill), General narrowing to column width.
- Files: XLSX read/write (styles, themes, merges, CF, validation, tables, comments, hyperlinks,
  charts, pictures, sparklines, print settings, defined names, shared/array/dynamic formulas),
  CSV/TSV with encoding and delimiter detection, HTML export, JSON.
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
- Static in-cell PNG/JPEG pictures: file insertion, aspect-preserving fit, cell operations and
  XLSX rich-value read/write. Clipboard images, floating/cell conversion and `IMAGE()` remain open.
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

## Milestones

| # | Milestone | Status |
|---|---|---|
| M0 | Foundation: core, number formats, formula language, functions, model, calc, XLSX/CSV, engine | ✅ done |
| M1 | Excel look: chrome, ribbon, formula bar, grid, tabs, status bar, control channel | ✅ done (polish continues) |
| M2 | Formatting completeness | ✅ done |
| M3 | Editing power (AutoComplete, spelling, drag-move/copy) | ✅ done |
| M4 | Data (advanced filter, consolidate, what-if) | 🟡 most; Solver, ToolPak and Power Query remain |
| M5 | Formulas tab (trace arrows, watch window, evaluate, error checking) | ✅ done |
| M6 | Conditional formatting manager parity | ✅ done |
| M7 | Charts parity | 🟡 partial; Format pane in, trendlines/axes/error bars remain |
| M8 | Insert & Review | 🟡 partial; ink, icons, comments pane in |
| M9 | Page layout & print | 🟡 most; true Page Layout view remains |
| M10 | PivotTables, Solver, Analysis ToolPak | 🟡 PivotTables in; slicers, Solver, ToolPak remain |
| M11 | Automation (record actions, scripts) | ✅ done |
| M12 | Performance, accessibility, i18n | 🟡 partial |
| M13 | Release & polish | 🟡 tooling ready; needs release secrets |

## How to measure

- `cargo xtask parity` — catalog coverage (feature names from Excel's ribbon and menus).
- `cargo test --workspace` — engine, formula, function, XLSX and UI behaviour.
- `cargo run --release -p gridcraft-ui-egui --example snapshot -- --sample sales out.png` — look at the UI offscreen.
