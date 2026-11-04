# Gaps

> **Last reviewed:** 2026-11-04 · **Last updated:** 2026-11-04 · **Change:** minor (entries checked against the code: large sheets cut off, calculation bugs, slicers not saved, CLI and MCP gaps; earlier: §17: equations; earlier: §13: Solver Sensitivity report; earlier: §4: chart resizing is no longer a gap; earlier: §13: Analysis ToolPak has all 19 tools; earlier: §4: chart data tables implemented with XLSX round-trip and print/PDF; earlier: §13: Solver Options; earlier: §13: Solver Load/Save Model and Reset All) · **Target:** Microsoft Excel (Microsoft 365)

Every known shortfall against Excel, one entry each, ranked by user impact (what blocks someone
using GridCraft for real work first). This is the work list: pick from the top, check the open
PRs first (several already address entries here), and update the entry
when you land work. Estimates are Opus 5.5 agent-hours. Area detail lives in the linked parity
docs; the summary is in [target-app-parity.md](target-app-parity.md). Entry numbers are stable ids,
because other docs link to them; [roadmap.md](roadmap.md#whats-next) gives the working order.

Kinds: **F** feature · **U** UI/UX · **FF** file format · **H** hardware · **L** localization ·
**P** performance · **S** stability.

## Beta blockers

No alpha blockers: every core workflow passes the [alpha gate](roadmap.md#alpha-gate).


### 1. Some saved XLSX files don't open in Excel (FF)
- **Missing:** no repair prompt on every saved file. A contributor's repeated corpus run (#891, real
  files, Excel 365) shows 21 of 397 saved files failing to open without an error or repair at v0.16.0, unchanged since v0.13.
  The failures still need sorting by cause.
- **Evidence:** named causes: #2947, #2009, #1663, #2339, #2949, #2635, and PivotTables (#1447, fixes
  in PRs #1680, #1681, #1682, #1684). `gridcraft-cli check` compares less than a save can lose: it
  reports "all equal" after rich text and chart options are dropped.
- **Impact:** the main format; a repair prompt or silent loss destroys trust.
- **Estimate:** 15–25 h, plus owner time in Excel. [file-format-parity.md](file-format-parity.md)

### 2. Large sheets are cut off on open; some recalculation is slow (P, H)
- **Missing:** a worksheet over about 500k cells opens cut off, and saving writes the shorter file.
  `MAX_XML_NODES` counts the cells of streamed rows (a regression from #2413): a 1M-row sheet of five
  columns keeps 100,000 rows. Fixes are in PRs #2925, #3210 and #3317. Files around 100 MB (#175,
  #3286) are limited by this first.
- **Recalculation:** single-threaded, but an edit updating a chain of 80,000 formulas took about
  0.1 s (2026-11-04, CLI). Targeted cases come first: running COUNTIFS (#2813), clearing a cell
  (#2684), hangs on open (#2751, #2183). 10 edits on that workbook used 729 MB against 215 MB for
  one; the cause isn't known yet.
- **Impact:** finance and data users routinely have big workbooks; a shortened file is data loss.
- **Estimate:** 25–40 h. [target-app-parity.md](target-app-parity.md#performance), [hardware-parity.md](hardware-parity.md)

### 3. Some unmodelled XLSX content is dropped on save (FF)
- **Missing:** slicers and their caches (including slicers made in GridCraft, which aren't written),
  timelines, the data model, query tables attached to tables, unmodelled `extLst` entries, chart
  details, and rich text in cells (flattened to plain text on open, #774; PR #2268). Pass-through
  (#330) keeps custom XML, external links, connections, sheet query tables, printer settings, form
  and ActiveX controls, OLE objects and sheet backgrounds.
- **Evidence:** `crates/xlsx/src/passthrough.rs` (what is kept, and why the rest isn't).
- **Impact:** open-edit-save still simplifies workbooks with slicers, timelines or Power Pivot.
- **Estimate:** 6–10 h. [file-format-parity.md](file-format-parity.md)

### 4. Charts lose options on round trip; shallow chart model (F, FF)
- **Missing:** axis scaling and reversal, axis number formats, label options, error bars, per-point
  formatting, and chart text sizes (#3117). All are dropped on save, even on charts nobody edited
  (#1668). PRs #1290, #1895, #1219.
- **Added:** data tables beneath column, bar, line, area and compatible combo charts, with legend keys, border controls, live source values, native XLSX round-trip and print/PDF.
- **Evidence:** `Chart` has no axis or per-point options (`crates/model/src/features.rs`); all 2-D
  kinds survive XLSX round trip (#158, #161); charts
  resize from eight handles (#190). Smooth lines (#62), Switch Row/Column (#116) and trendlines
  (read, written, drawn) landed. Multi-level category axes (year beside month) are read, drawn
  and written back.
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
- **Missing:** implicit intersection for formulas saved without the dynamic-array flag: `=ISBLANK(B:B)`
  shows `#SPILL!` and `=A:A*2` spills, where Excel gives one value per row (#410; PRs #2677, #3067).
  Stale dynamic arrays (#2053, #2340, #2320). Text coerced with en-US separators, which gives silent
  1000× values in regional files (#889), and SORT in code-point order (#890).
- **Landed:** 15-significant-digit equality (#57), spill updates after structural edits and cleared
  blockers (#133, #139), 3-D span deletion (#140), qualified names after a sheet rename (#134), and
  iterative calculation.
- **Impact:** wrong numbers are worse than missing features, so this entry is kept until the
  oracle suite of §15 can say the category is clear.
- **Estimate:** 10–15 h. [function-parity.md](function-parity.md#formula-language)

### 7. Legacy `.xls` only partly read; encrypted workbooks can't be saved with a password (FF)
- **Evidence:** #121: Excel 97-2003 (BIFF8) `.xls` files now open (values, formulas, number
  formats, merges, sizes, names, and cell formatting: fonts, fills, borders, alignment); exact theme colours (XFExt), charts, pictures, comments, conditional
  formats, validation and BIFF5 files are not read, and `.xls` is never written. Password-encrypted
  `.xlsx` workbooks (Agile and Standard encryption) open with their password since #11, but are
  saved without it: GridCraft can't write an encrypted file yet. Encrypted `.xls` (FILEPASS) and
  RC4-encrypted files are still refused with a message.
- **Impact:** common in finance, government and anything older than 2007; encrypted files are
  routine in business.
- **Estimate:** 14–21 h (`.xls` objects 10–15, encryption 4–6). [file-format-parity.md](file-format-parity.md)

### 8. PivotTables: fixes after a save, slicers not saved, calculated items (F)
- **Missing:** slicers work in the open workbook but aren't saved; `insert.timeline`, calculated
  items and `GETPIVOTDATA` (`#NAME?`; PR #3317) are missing. There is no PivotTable Options dialog
  for Enable show details (command only).
- **Bugs:** a TRUE/FALSE column in the source makes a file Excel can't open, and fields and layouts
  change after a save (#1447; PRs #1680, #1681, #1682, #1684, #3222).
- **Landed:** calculated fields, values on rows, PivotCharts, Show Details.
- **Impact:** pivots are the analyst's main tool.
- **Estimate:** 20–30 h for beta (25–40 h full).

### 9. Keyboard-first users: key tips, shortcuts, context menus (U)
- Next/previous sheet navigation now skips Hidden and VeryHidden worksheets without activating a
  hidden sheet at the workbook boundary. Other keyboard gaps below remain.
- **Evidence:** #33, #49 (key tips: initial Home-tab support landed in #43 on 10-10, the rest of the ribbon not yet), #110, #28 (selection),
  59 command shortcuts vs Excel's 200+.
- **Estimate:** 12–20 h. [ui-parity.md](ui-parity.md)

### 10. Crashes and freezes reported from the field (S)
- **Missing:** crash recovery: nothing keeps a copy of unsaved work (PRs #3326, #2973). AutoSave is off
  by default and ignores a failed save.
- **Evidence:** #2906, #2464, #2042 (native save dialog), #120 (Intel UHD crash on Windows), #76
  (flashing on Windows 11).
- **Estimate:** 8–12 h plus an autosave/recovery soak.

### 25. CLI and MCP servers (F)
- **Bugs:** a sheet that was cut off gives only a warning (`run` exits 0, MCP `open_workbook` returns
  `isError: false`, see §2). The MCP stdio server reads a line into an unbounded buffer (#2164).
  `execute_command` returns a bare `null` for layout commands (#1291). With two windows, input and
  screenshots go to the wrong one (#3036; PR #3045), and MCP can take over a window it opened (#3069).
- **Missing:** a strict mode that fails on lost data; confinement to chosen folders (#445; PRs #2857,
  #2739); PNG rendering from the headless server (#2246); attaching to a running app without a
  restart (#2280); push events (#3060); a report of hidden information before sharing (#1829); CLI
  and MCP setup for desktop users (#220).
- **Impact:** agent control is where GridCraft is meant to be ahead.
- **Estimate:** not yet estimated. [mcp.md](mcp.md), [cli.md](cli.md)

## After beta

### 11. Localization is partial (L)
- Catalog: ribbon-only in five languages (#31, #104, #82; ~12% of strings each), and 943 strings
  (~43%) in Japanese (10-17) and Brazilian Portuguese, including the dialog labels, messages, File
  view and menus. The remaining dialogs, menus and messages are English; en-US function names and
  separators (#45); German in PR #163. **Estimate:**
  100–165 h including #5. [localization-parity.md](localization-parity.md)

### 12. Get & Transform, external data, data model (F)
- No Power Query editor, connections, refresh, From Web/database/folder sources, relationships or
  data model (`data.fromWeb`, `data.relationships` missing). **Estimate:** 60–100 h.

### 13. Analysis tools: Solver, Analysis ToolPak, Forecast Sheet (F)
- Forecast Sheet (`data.forecastSheet`) and the FORECAST.ETS family are in, including Forecast
  Start (hindcast, #785).
- Solver (`data.solver`, #327) has Simplex LP with integer/binary branch and bound, a local
  nonlinear method (augmented Lagrangian, not GRG), the Evolutionary method, and Answer,
  Sensitivity and Limits reports. Report refusal leaves the solution, saved model and history unchanged; a
  successful solve and its reports share one Undo step (#1050). Load/Save Model writes the
  model on cells of its sheet and reads it back; Reset All removes it. Solver Options sets the
  evaluation limit, Integer Optimality and the Evolutionary options, which are saved with the
  model. AllDifferent constraints and the Population report remain.
- The Analysis ToolPak (`data.analysisToolPak`) has all 19 of Excel's tools; their chart output
  remains. **Estimate:** 12–20 h.

### 14. Page Layout view and print polish (U, F)
- Page Layout view landed (#329): paper view with rulers and in-place header/footer editing.
  Margins are dragged on the rulers (#792). Remaining: pictures in headers/footers, Different First Page / Odd & Even pages. Desktop now submits directly through craft-print with printer choice, copies, duplex and grayscale; browser printing stays available. Print ranges and driver tray/options UI remain. **Estimate:** 5–10 h.

### 15. Remaining functions and an oracle suite (F)
- 10 missing, 9 stubs; no Excel-computed oracle corpus; #137, #138. **Estimate:** 16–26 h.
  [function-parity.md](function-parity.md)

### 16. Collaboration and review (F)
- No co-authoring, Show Changes or notes ↔ comments conversion; cloud storage requested (#86). **Estimate:** 35–55 h (co-authoring scope needs the owner).

### 17. Insert objects (F)
- Ink strokes now survive XLSX save/reopen as editable polylines. Inserted icons still lose their
  identity/artwork on save (#551); native Office InkML import is not implemented.
- SmartArt, WordArt, signature line, objects, 3-D models, screenshot; 8 shape kinds vs ~170;
  pictures in cells (#61, PR #105). Equations exist (LaTeX source with a live preview, Office Math in xlsx); there is
  no structured (WYSIWYG) equation editing and no ink equations. **Estimate:** 25–40 h.

### 18. Accessibility (U)
- AccessKit on, but no accessible grid/ribbon tree, no high contrast, no checker. **Estimate:** 12–18 h.
  [ui-parity.md](ui-parity.md#accessibility)

### 19. XLSB/ODS beyond values; minor formats (FF)
- XLSB and ODS import values only, no write; no `.prn`, `.dif`, XML Spreadsheet 2003 (`.slk` imports values, formulas, number formats and widths); templates saved with the workbook content type.
  **Estimate:** 15–23 h.

### 20. Multiple windows; pen pressure (H)
- Desktop windows done: a native window per workbook window, New Window on the same workbook,
  Switch Windows, Arrange All. Left: Arrange All across monitors, one window on the web, no
  pressure or tilt in ink. **Estimate:** 5–8 h.
  [hardware-parity.md](hardware-parity.md)

### 21. Automation and add-ins (F)
- VBA isn't run (kept byte-for-byte on save, #6); no Office Scripts compatibility; no plugin or custom
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
| 2026-11-04 | minor | Checked against the code: §1 corpus runs exist; §2 sheets over 500k cells are cut off; §3 and §8 slicers exist but aren't saved; §6 open calculation bugs; §10 no crash recovery; §15 one count; §16 comment navigation done; §19 CSV writes displayed values; §21 VBA is kept; new §25 CLI and MCP |
| 2026-11-02 | minor | §17: equations (Insert › Equation, LaTeX source, gallery, editor) |
| 2026-11-02 | minor | §13: Solver Sensitivity report: shadow prices, reduced costs and allowable ranges for Simplex LP; Lagrange multipliers and reduced gradients for GRG Nonlinear |
| 2026-11-02 | minor | §4: charts, like every floating object, move and resize from eight handles (#190), so resizing is no longer listed as missing; the evidence is one paragraph again, without the stale "PRs #158, #161 open" |
| 2026-11-01 | minor | §13: the Analysis ToolPak has all 19 of Excel's tools (two-factor Anova with and without replication, z-Test, Fourier Analysis, Random Number Generation, Sampling); its chart output remains. The stale "`data.solver`, `data.analysisToolPak` missing" and "no Forecast Start" sentences removed |
| 2026-11-01 | minor | §4: chart data tables implemented for column, bar, line, area and compatible combo charts, with legend keys and border settings, live source values, XLSX round-trip and print/PDF; other chart gaps remain. |
| 2026-10-31 | minor | §13: Solver Options (Max Evaluations, Integer Optimality and the Evolutionary options), saved with the model |
| 2026-10-31 | minor | §13: Solver Load/Save Model (`data.solverSaveModel`, `data.solverLoadModel`) and Reset All (`data.solverReset`), with buttons in the Solver Parameters dialog |
| 2026-10-24 | minor | §8: PivotTable Enable show details: `pivot.options` `enableShowDetails` turns Show Details off for a PivotTable, read and written in XLSX as `enableDrill` |
| 2026-10-23 | minor | §8: PivotTable Show Details: double-clicking a value cell lists its source records on a new sheet (#2364) |
| 2026-10-22 | minor | §20: a native window per workbook window, New Window on the same workbook and Arrange All on the desktop; left: Arrange All across monitors, the web's one window, pen pressure (estimate 10–14 h → 5–8 h) |
| 2026-10-20 | minor | §11: the Japanese catalog covers the dialogs, messages and menus — 943 keys (~43%), matching the pt-BR coverage; a test keeps the two catalogs in step |
| 2026-10-17 | minor | §6: the five reports it tracked are closed and their PRs merged (#57, #133, #139, #140, #134; PRs #148, #155, #156, #157); the entry stays open for §15's oracle suite |
| 2026-10-15 | minor | §6: Maximum Iterations and Maximum Change in File › Options › Formulas; dropped the stale "no Options dialog yet" |
| 2026-10-15 | minor | §13: Solver prepares reports before committing the solution, model and report sheets together; report refusal preserves the workbook and Undo/Redo (#1050) |
| 2026-10-13 | trivial | §4: multi-level category axes landed |
| 2026-10-12 | minor | §9: next/previous sheet commands retain the current visible sheet and selection when there is no visible destination |
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
