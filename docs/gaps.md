# Gaps

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created from the full re-measure against Excel for Mac 16.113.4 and the issue tracker) · **Target:** Microsoft Excel (Microsoft 365)

Every known shortfall against Excel, one entry each, ranked by user impact (what blocks someone
using GridCraft for real work first). This is the work list: pick from the top, check the open
PRs first (50 open on 2026-10-10; several already address entries here), and update the entry
when you land work. Estimates are Opus 5.5 agent-hours. Area detail lives in the linked parity
docs; the summary is in [target-app-parity.md](target-app-parity.md).

Kinds: **F** feature · **U** UI/UX · **FF** file format · **H** hardware · **L** localization ·
**P** performance · **S** stability.

## Beta blockers

No alpha blockers: every core workflow passes the [alpha gate](roadmap.md#alpha-gate).


### 1. XLSX output not proven in Excel or on real files (FF)
- **Missing:** a run over a real-world XLSX corpus, and confirmation that Excel opens what we save
  with no repair prompt (PivotTables, charts, CF, validation, dynamic arrays, tables).
- **Evidence:** round-trip tests cover our own files only (`crates/xlsx/src/tests/`); PivotTable
  round-trip noted as "unverified in real Excel" since 10-07; #64 (dynamic-array flag) shows the
  class of bug.
- **Impact:** the main format; a repair prompt or silent loss destroys trust.
- **Estimate:** 15–25 h, plus owner time in Excel. [file-format-parity.md](file-format-parity.md)

### 2. Large workbooks don't open; recalc is single-threaded (P, H)
- **Missing:** streaming read for ~100 MB XLSX; multi-threaded recalc; 100k-dependent edit under
  100 ms (was ~0.4 s); whole-sheet operations bounded.
- **Evidence:** #175; no threading in `crates/calc`; PR #152 (whole-sheet Fill/Remove Duplicates
  out of memory), PR #151 (current region past 10,000 rows).
- **Impact:** finance and data users routinely have big workbooks.
- **Estimate:** 25–40 h. [target-app-parity.md](target-app-parity.md#performance), [hardware-parity.md](hardware-parity.md)

### 3. Unmodelled XLSX content is dropped on save (FF)
- **Missing:** pass-through of parts and extensions we don't model (form controls, slicers,
  timelines, external links, OLE objects, custom XML, query tables, data model, chart details).
- **Evidence:** reader warns and skips (`crates/xlsx/src/read.rs`); external links "not supported".
- **Impact:** open-edit-save silently simplifies colleagues' workbooks.
- **Estimate:** 10–15 h. [file-format-parity.md](file-format-parity.md)

### 4. Charts lose type and options on round trip; shallow chart model (F, FF)
- **Missing:** kinds that survive XLSX (stock, combo, histogram, box, waterfall, funnel, treemap,
  sunburst), axis scaling, label options, trendlines, error bars, per-point formatting.
- **Evidence:** `Chart` holds only title, legend position, two flags and axis titles
  (`crates/model/src/features.rs`); PRs #158, #161, #116; #52.
- **Impact:** charts are in most business workbooks.
- **Estimate:** 25–35 h for beta (40–60 h full). [chart-parity.md](chart-parity.md)

### 5. Text in non-Latin scripts doesn't render or export (L)
- **Missing:** CJK font fallback, complex shaping (Arabic, Devanagari, Thai), bidi, non-Latin PDF.
- **Evidence:** #142 (Chinese text), #32 (Thai), #54 (symbols as boxes on Fedora), PDF writer
  uses 14 standard fonts only; PRs #103, #147 open.
- **Impact:** blocks most of the world's users even with an English UI.
- **Estimate:** 12–20 h. [localization-parity.md](localization-parity.md)

### 6. Calculation correctness bugs (F)
- **Missing:** 15-significant-digit equality; spill updates after structural edits and cleared
  blockers; 3-D span deletion; qualified names after sheet rename; iterative calculation.
- **Evidence:** #57, #133, #139, #140, #134; Enable Iterative Calculation stored but unused;
  PRs #148, #155, #156, #157.
- **Impact:** wrong numbers are worse than missing features.
- **Estimate:** 10–15 h. [function-parity.md](function-parity.md#formula-language)

### 7. No legacy `.xls`; no password-protected workbooks (FF)
- **Evidence:** #121, #11. `FileKind` has no XLS; no CFB/encryption code.
- **Impact:** common in finance, government and anything older than 2007; encrypted files are
  routine in business.
- **Estimate:** 28–42 h (`.xls` read 20–30, encryption 8–12). [file-format-parity.md](file-format-parity.md)

### 8. PivotTables: no slicers, calculated fields, PivotCharts; unverified in Excel (F)
- **Evidence:** `parity-checklist.md` missing `insert.slicer`, `insert.timeline`,
  `insert.pivotChart`, `pivot.calculatedField`, `table.insertSlicer`; GETPIVOTDATA missing.
- **Impact:** pivots are the analyst's main tool.
- **Estimate:** 20–30 h for beta (25–40 h full).

### 9. Keyboard-first users: key tips, shortcuts, context menus (U)
- **Evidence:** #33, #49 (key tips: initial Home-tab support landed in #43 on 10-10, the rest of the ribbon not yet), #110, #21 (Insert Copied Cells), #28 (selection),
  59 command shortcuts vs Excel's 200+.
- **Estimate:** 12–20 h. [ui-parity.md](ui-parity.md)

### 10. Crashes and freezes reported from the field (S)
- **Evidence:** #120 (Intel UHD crash on Windows), #117 (error on close, macOS), #166 (freeze opening
  from Finder; PRs #164, #167), #76 (flashing on Windows 11), #130 (caught panic in table styles).
- **Estimate:** 8–12 h plus an autosave/recovery soak.

## After beta

### 11. No localization at all (L)
- English-only literals, no catalog, en-US function names and separators (#20, #45, #79; PRs #104,
  #163). **Estimate:** 110–175 h including #5. [localization-parity.md](localization-parity.md)

### 12. Get & Transform, external data, data model (F)
- No Power Query editor, connections, refresh, From Web/database/folder sources, relationships or
  data model (`data.fromWeb`, `data.relationships` missing). **Estimate:** 60–100 h.

### 13. Analysis tools: Solver, Analysis ToolPak, Forecast Sheet (F)
- `data.solver`, `data.analysisToolPak`, `data.forecastSheet` missing; FORECAST.ETS family missing.
  **Estimate:** 22–35 h.

### 14. Page Layout view and print polish (U, F)
- No paper view with rulers and in-place header/footer editing; web print exports only (#141, PR
  #143). **Estimate:** 12–20 h.

### 15. Remaining functions and an oracle suite (F)
- 22 missing, 9 stubs; no Excel-computed oracle corpus; #137, #138. **Estimate:** 18–30 h.
  [function-parity.md](function-parity.md)

### 16. Collaboration and review (F)
- No co-authoring, Show Changes, Allow Edit Ranges, comment navigation, notes ↔ comments
  conversion; cloud storage requested (#86). **Estimate:** 35–55 h (co-authoring scope needs the owner).

### 17. Insert objects (F)
- SmartArt, WordArt, equations, signature line, objects, 3-D models, screenshot; 8 shape kinds vs
  ~170; pictures in cells (#61, PR #105). **Estimate:** 25–40 h.

### 18. Accessibility (U)
- AccessKit on, but no accessible grid/ribbon tree, no high contrast, no checker. **Estimate:** 12–18 h.
  [ui-parity.md](ui-parity.md#accessibility)

### 19. XLSB/ODS beyond values; minor formats (FF)
- XLSB and ODS import values only, no write; no `.prn`, `.dif`, `.slk`, XML Spreadsheet 2003; CSV
  writes raw instead of displayed values; templates saved with the workbook content type.
  **Estimate:** 15–23 h.

### 20. Multiple windows; pen pressure (H)
- One window (#162, PR #171); no pressure or tilt in ink. **Estimate:** 10–14 h.
  [hardware-parity.md](hardware-parity.md)

### 21. Automation and add-ins (F)
- VBA out of scope (dropped on open, #6); no Office Scripts compatibility; no plugin or custom
  function API. **Estimate:** 25–40 h plus an owner decision.

### 22. AI features (F)
- No in-app assistant, natural-language formulas or Analyze Data; agent control via MCP is
  already strong. **Estimate:** 30–50 h; model choice needs the owner.

### 23. Mobile (platform)
- No iOS, iPadOS or Android layout. **Estimate:** 30–60 h.

### 24. Fonts and look (U)
- Font weights beyond regular/bold (#168), system font list (#4), Calibri metrics (#29), macOS
  chrome (#169, #170), Linux title bar (#54), icon consistency across apps (#173). **Estimate:** 7–12 h.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Noted that no gap blocks the alpha gate |
| 2026-10-10 | major | Created: 24 ranked gaps with evidence, from the full re-measure and the 60 open issues |
