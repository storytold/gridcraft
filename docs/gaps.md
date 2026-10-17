# Gaps

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-12 · **Change:** minor (rows auto-fit oversized fonts on open and whole-row/column formatting; GETPIVOTDATA) · **Target:** Microsoft Excel (Microsoft 365)

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

### 3. Some unmodelled XLSX content is dropped on save (FF)
- **Missing:** slicers and their caches, timelines, the data model, query tables attached to
  tables, unmodelled `extLst` entries, chart details. Pass-through (#330) keeps custom XML,
  external links, connections, sheet query tables, printer settings, form and ActiveX controls,
  OLE objects and sheet backgrounds; never checked in Excel (repair prompts unknown).
- **Evidence:** `crates/xlsx/src/passthrough.rs` (what is kept, and why the rest isn't).
- **Impact:** open-edit-save still simplifies workbooks with slicers, timelines or Power Pivot.
- **Estimate:** 6–10 h. [file-format-parity.md](file-format-parity.md)

### 4. Charts lose type and options on round trip; shallow chart model (F, FF)
- **Missing:** kinds that survive XLSX (stock, combo, histogram, box, waterfall, funnel, treemap,
  sunburst), axis scaling, label options, error bars, per-point formatting.
- **Evidence:** `Chart` holds only title, legend position, two flags and axis titles
  (`crates/model/src/features.rs`); PRs #158, #161 open; charts can't be resized (#190). Smooth lines (#62), Switch Row/Column (#116) and trendlines (read, written, drawn) landed.
- **Impact:** charts are in most business workbooks.
- **Estimate:** 25–35 h for beta (40–60 h full). [chart-parity.md](chart-parity.md)

### 5. Text in non-Latin scripts doesn't render or export (L)
- **Missing:** CJK font fallback, complex shaping (Arabic, Devanagari, Thai), bidi, non-Latin PDF.
- **Evidence:** system-font fallback for CJK, Thai and Hebrew landed in #38 (closed #32); #142
  (Chinese text) awaits confirmation; #54 (symbols as boxes on Fedora); no shaping for Arabic or
  Devanagari; PDF writer uses 14 standard fonts only.
- **Impact:** blocks most of the world's users even with an English UI.
- **Estimate:** 12–20 h. [localization-parity.md](localization-parity.md)

### 6. Calculation correctness bugs (F)
- **Missing:** 15-significant-digit equality; spill updates after structural edits and cleared
  blockers; 3-D span deletion; qualified names after sheet rename.
- **Evidence:** #57, #133, #139, #140, #134; PRs #148, #155, #156, #157. Iterative calculation
  for circular references landed (no Options dialog yet).
- **Impact:** wrong numbers are worse than missing features.
- **Estimate:** 10–15 h. [function-parity.md](function-parity.md#formula-language)

### 7. Legacy `.xls` only partly read; encrypted workbooks can't be saved with a password (FF)
- **Evidence:** #121: Excel 97-2003 (BIFF8) `.xls` files now open (values, formulas, number
  formats, merges, sizes, names); fonts, fills, borders, charts, pictures, comments, conditional
  formats, validation and BIFF5 files are not read, and `.xls` is never written. Password-encrypted
  `.xlsx` workbooks (Agile and Standard encryption) open with their password since #11, but are
  saved without it: GridCraft can't write an encrypted file yet. Encrypted `.xls` (FILEPASS) and
  RC4-encrypted files are still refused with a message.
- **Impact:** common in finance, government and anything older than 2007; encrypted files are
  routine in business.
- **Estimate:** 14–21 h (`.xls` styles and objects 10–15, encryption 4–6). [file-format-parity.md](file-format-parity.md)

### 8. PivotTables: no slicers, calculated items, PivotCharts; unverified in Excel (F)
- **Evidence:** `parity-checklist.md` missing `insert.slicer`, `insert.timeline`,
  `insert.pivotChart`, `table.insertSlicer`; calculated items and Generate GetPivotData missing (calculated fields,
  values on rows and GETPIVOTDATA done).
- **Impact:** pivots are the analyst's main tool.
- **Estimate:** 20–30 h for beta (25–40 h full).

### 9. Keyboard-first users: key tips, shortcuts, context menus (U)
- **Evidence:** #33, #49 (key tips: initial Home-tab support landed in #43 on 10-10, the rest of the ribbon not yet), #110, #28 (selection),
  59 command shortcuts vs Excel's 200+.
- **Estimate:** 12–20 h. [ui-parity.md](ui-parity.md)

### 10. Crashes and freezes reported from the field (S)
- **Evidence:** #120 (Intel UHD crash on Windows), #117 (error on close, macOS), #166 (freeze opening
  from Finder; PRs #164, #167), #76 (flashing on Windows 11), #130 (caught panic in table styles).
- **Estimate:** 8–12 h plus an autosave/recovery soak.

## After beta

### 11. No localization at all (L)
- Ribbon-only catalog in five languages (#31, #104, #82; ~12% of strings each); dialogs, menus and
  messages English; en-US function names and separators (#45); German in PR #163. **Estimate:**
  100–165 h including #5. [localization-parity.md](localization-parity.md)

### 12. Get & Transform, external data, data model (F)
- No Power Query editor, connections, refresh, From Web/database/folder sources, relationships or
  data model (`data.fromWeb`, `data.relationships` missing). **Estimate:** 60–100 h.

### 13. Analysis tools: Solver, Analysis ToolPak, Forecast Sheet (F)
- `data.solver`, `data.analysisToolPak` missing. Forecast Sheet (`data.forecastSheet`) and the
  FORECAST.ETS family are in, including Forecast Start (hindcast, #785). **Estimate:** 15–25 h.
- Solver (`data.solver`, #327) has Simplex LP with integer/binary branch and bound, a local
  nonlinear method (augmented Lagrangian, not GRG), the Evolutionary method, and Answer and
  Limits reports; AllDifferent constraints, the Solver Options dialog, the Sensitivity and
  Population reports and Load/Save Model remain. The Analysis ToolPak
  (`data.analysisToolPak`) has 13 of 19 tools. Forecast Sheet (`data.forecastSheet`) and the
  FORECAST.ETS family are in; the sheet has no Forecast Start (hindcast) option yet. **Estimate:** 12–20 h.

### 14. Page Layout view and print polish (U, F)
- Page Layout view landed (#329): paper view with rulers and in-place header/footer editing.
  Margins are dragged on the rulers (#792). Remaining: pictures in headers/footers, Different First Page / Odd & Even pages. Desktop now submits directly through craft-print with printer choice, copies, duplex and grayscale; browser printing stays available. Print ranges and driver tray/options UI remain. **Estimate:** 5–10 h.

### 15. Remaining functions and an oracle suite (F)
- 9 missing, 9 stubs; no Excel-computed oracle corpus; #137, #138. **Estimate:** 16–26 h.
  [function-parity.md](function-parity.md)

### 16. Collaboration and review (F)
- No co-authoring, Show Changes, comment navigation, notes ↔ comments
  conversion; cloud storage requested (#86). **Estimate:** 35–55 h (co-authoring scope needs the owner).

### 17. Insert objects (F)
- Ink strokes now survive XLSX save/reopen as editable polylines. Inserted icons still lose their
  identity/artwork on save (#551); native Office InkML import is not implemented.
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
| 2026-10-12 | minor | §8, §15: GETPIVOTDATA added (9 functions missing) |
| 2026-10-12 | minor | rows auto-fit oversized fonts on open and whole-row/column formatting (#833) |
| 2026-10-12 | minor | Page Layout view: dragging margins on the rulers (#792) removed from the §14 remainder |
| 2026-10-12 | minor | §13: Forecast Sheet Forecast Start (hindcast) landed (#785) |
| 2026-10-12 | minor | §17: ink geometry survives XLSX saves; the icon portion of #551 remains |
| 2026-10-12 | minor | §8: values on rows (Σ Values in Rows, `dataOnRows`) laid out, moved and round-tripped |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | minor | §6: iterative calculation for circular references landed (Maximum Iterations, Maximum Change) |
| 2026-10-11 | minor | §15: trim-reference operators (`A1:.E10`, `A1.:E10`, `A1.:.E10`) parse and evaluate |
| 2026-10-11 | minor | §4: trendlines (six types, forecast, equation and R²) read, written and drawn |
| 2026-10-11 | minor | §7: password-encrypted workbooks open with their password; saving with one remains (#11) |
| 2026-10-11 | minor | §16: comment navigation, Show Comments, Show All Notes and Convert to Comments landed (#281) |
| 2026-10-11 | minor | §7: `.xls` (BIFF8) import of values, formulas and number formats (#121) |
| 2026-10-11 | minor | §3: XLSX pass-through keeps custom XML, external links, connections, query tables, printer settings, controls and OLE objects (#330, #324) |
| 2026-10-11 | minor | §13: Forecast Sheet landed (#322) |
| 2026-10-11 | minor | §15: ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD added (18 functions missing) |
| 2026-10-11 | minor | §15: PERCENTOF and EUROCONVERT added (20 functions missing) |
| 2026-10-11 | minor | §15: BAHTTEXT added (21 functions missing) |
| 2026-10-11 | minor | §15: TRIMRANGE added (21 functions missing; trim-reference operators still missing) |
| 2026-10-11 | minor | §16: Allow Edit Ranges landed (#307) |
| 2026-10-11 | minor | §8: calculated fields added |
| 2026-10-11 | minor | §8: PivotCharts added |
| 2026-10-11 | minor | §13, §15: FORECAST.ETS family landed (#314) |
| 2026-10-10 | minor | Updated for the 10-10 merges (#17, #31, #38, #62, #82, #104, #116) and the new chart resize bug (#190) |
| 2026-10-10 | minor | Noted that no gap blocks the alpha gate |
| 2026-10-10 | major | Created: 24 ranked gaps with evidence, from the full re-measure and the 60 open issues |
