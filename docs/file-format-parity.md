# File-format parity

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-24 · **Change:** minor (whole-axis PivotTable sources survive XLSX saves while cache scans stay bounded; .xls dialog sheets are skipped instead of read as worksheets; `.xls` import reads each sheet's display options from WINDOW2: right-to-left, gridlines, headings, zeros, formulas; XLSX text boxes and shapes keep their paragraphs' right-to-left direction on save; CSV/TSV/TXT can open with every field kept as text via `file.open {csvAsText}`; formulas over Excel's 8,192 characters, and `%` or call chains past the nesting limit, are parse errors instead of stack overflows when a file is opened; XLSX PivotTables read and write `rowGrandTotals`/`colGrandTotals` the way Excel does, so a table saved with only one grand total opens with the right one; HTML export keeps right-to-left; CSV and text import detects likely Windows-1256 Arabic when invalid UTF-8 has Arabic byte runs; other invalid UTF-8 remains Windows-1252; HTML export places pictures and charts, as inline SVG, at their anchors) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Every format Excel opens or saves, with GridCraft's read and write support, fidelity and tests.
Part of [target-app-parity.md](target-app-parity.md). **File formats: ~50% ready, 80–120 h**
(estimated).

Excel's list comes from Microsoft's public "File formats that are supported in Excel"
documentation (the Office bundle is off limits under this repo's clean-room rule). GridCraft's
support comes from `crates/engine/src/io.rs` (`FileKind`), `crates/xlsx/src` and the tests in
`crates/xlsx/src/tests/` (round-trip, fixtures, malformed input, CSV, ODS, XLSB, XLS, PivotTables).

Legacy scalar `INDEX(range, MATCH(...))` formulas remain ordinary formulas on save, including when
shared formulas are expanded during import (`array_export.rs`, #1505).

Whole-column and whole-row PivotTable sources retain their declared extent across XLSX saves;
Refresh All after reopening includes newly appended data. Cache generation remains bounded to
the used range. Generated regressions and limits: [pivot-whole-axis-sources.md](pivot-whole-axis-sources.md).

## Workbook formats

XLSX column declarations are limited to 1,000,000 expanded column assignments per worksheet,
including repeated overlaps across `<cols>` blocks. Spans are clipped to the grid and records
that would be skipped as defaults are excluded before charging the limit. Excess work returns
the existing file-too-large error; accepted overlaps retain their existing last-wins behavior
(#1022).

Ink created with Draw is written as standard, unfilled DrawingML polylines, without visible point
captions or private metadata. Supported numeric polylines reopen as editable ink, retaining points,
colour, integer width and anchor. Curves, filled paths, transforms and unsupported line styles are
not interpreted as ink. This does not implement Office InkML or fix inserted icons (#551).
Command and package tests cover repeated saves and unrelated cell edits; external Excel rendering
and repair-free opening remain unverified.

Dynamic-array formulas keep their mark on save. A formula read with dynamic-array cell metadata
(`cm`, the `XLDAPR` record in `xl/metadata.xml`) is written back with it, together with the
metadata part, even when it returns one value and GridCraft's own shape analysis would not need
array evaluation. Dropping the mark turned such formulas into legacy ones, which Excel evaluates
with implicit intersection: `SUMPRODUCT(IFERROR(B1:B3/C1:C3,0))` recalculated to a different
value in Excel. Moving the formula, inserting or deleting rows and columns, and renaming sheets or
names keep the mark; retyping the formula drops it, and the writer then decides from the
formula's shape. Rich-value metadata other than cell pictures (data types, `#SPILL!` error
values) is still dropped.

| Format | Excel | GridCraft read | GridCraft write | Fidelity / notes | Tests |
|---|---|---|---|---|---|
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, shared strings (with their Japanese phonetic runs — ふりがな — kept through open + save: `<rPh>` offsets and readings and `<phoneticPr>` are written back next to the same string; the table holds each text once, so duplicate `<si>` items whose text is the same but whose readings differ keep only the first reading, with a warning on open), styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings, show objects (`workbookPr showObjects`). **Kept unchanged on save (pass-through, `passthrough.rs`):** custom document properties, ribbon customisation and add-in task panes, custom XML, links to external workbooks (never followed or refreshed), data connections, sheet query tables, printer settings (#324), form controls, ActiveX controls, OLE objects and sheet backgrounds (with their VML shapes), and chart sheets (#727: the sheet, drawing, chart, style and colour parts are written back unchanged in their place, and left out when a sheet they plot was deleted or renamed or a worksheet took their name; they can't be edited or created); dropped when their sheet is deleted or their parts point at content GridCraft rewrites. **Not preserved:** slicers and slicer caches, timelines, data model, query tables attached to tables, sheet and workbook `extLst` entries we don't model, chart details; Strict Open XML files keep nothing. Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) A picture used on several sheets is written once (#985). | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, shared strings (with their Japanese phonetic runs — ふりがな — kept through open + save: `<rPh>` offsets and readings and `<phoneticPr>` are written back next to the same string; the table holds each text once, so duplicate `<si>` items whose text is the same but whose readings differ keep only the first reading, with a warning on open), styles, themes, merges, CF, validation, tables, notes (with their shown/hidden state) and threaded comments with replies, resolved state and timestamps (`threadedComments` and `persons` parts, #300), hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings, show objects (`workbookPr showObjects`). **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) A picture used on several sheets is written once (#985). | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, shared strings (with their Japanese phonetic runs — ふりがな — kept through open + save: `<rPh>` offsets and readings and `<phoneticPr>` are written back next to the same string; the table holds each text once, so duplicate `<si>` items whose text is the same but whose readings differ keep only the first reading, with a warning on open), styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, Allow Edit Ranges (`protectedRanges` with their password hashes, #307), calc settings, show objects (`workbookPr showObjects`). **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) A picture used on several sheets is written once (#985). | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes (the major/minor `latin`, East Asian `ea` and complex-script `cs` typefaces and the per-script `<a:font script>` lists are kept through a save), merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Kept unchanged on save (pass-through, `passthrough.rs`):** custom document properties, ribbon customisation and add-in task panes, custom XML, links to external workbooks (never followed or refreshed), data connections, sheet query tables, printer settings (#324), form controls, ActiveX controls, OLE objects and sheet backgrounds (with their VML shapes), pictures in headers and footers (`&G`, #1257: their VML drawing and images are written back unchanged; they are not shown, and can't be inserted), and chart sheets (#727: the sheet, drawing, chart, style and colour parts are written back unchanged in their place, and left out when a sheet they plot was deleted or renamed or a worksheet took their name; they can't be edited or created); dropped when their sheet is deleted or their parts point at content GridCraft rewrites. **Not preserved:** slicers and slicer caches, timelines, data model, query tables attached to tables, sheet and workbook `extLst` entries we don't model, chart details; Strict Open XML files keep nothing. Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes (the major/minor `latin`, East Asian `ea` and complex-script `cs` typefaces and the per-script `<a:font script>` lists are kept through a save), merges, CF, validation, tables, notes (with their shown/hidden state) and threaded comments with replies, resolved state and timestamps (`threadedComments` and `persons` parts, #300), hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes (the major/minor `latin`, East Asian `ea` and complex-script `cs` typefaces and the per-script `<a:font script>` lists are kept through a save), merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, Allow Edit Ranges (`protectedRanges` with their password hashes, #307), calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Kept unchanged on save (pass-through, `passthrough.rs`):** custom document properties, ribbon customisation and add-in task panes, custom XML, links to external workbooks (never followed or refreshed), data connections, sheet query tables, printer settings (#324), form controls, ActiveX controls, OLE objects and sheet backgrounds (with their VML shapes), and chart sheets (#727: the sheet, drawing, chart, style and colour parts are written back unchanged in their place, and left out when a sheet they plot was deleted or renamed or a worksheet took their name; they can't be edited or created); dropped when their sheet is deleted or their parts point at content GridCraft rewrites. **Not preserved:** slicers and slicer caches, timelines, data model, query tables attached to tables, sheet and workbook `extLst` entries we don't model, chart details; Strict Open XML files keep nothing. Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, notes (with their shown/hidden state and their box's place and size, #1608) and threaded comments with replies, resolved state and timestamps (`threadedComments` and `persons` parts, #300), hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, Allow Edit Ranges (`protectedRanges` with their password hashes, #307), calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Strict Open XML | R/W | yes | no (writes transitional) | Excel also saves transitional by default | fixtures |
| `.xlsm` Macro-Enabled Workbook | R/W | yes | yes, macros kept, not run | The VBA project is kept byte-for-byte and written back only into `.xlsm`/`.xltm` (#6); macros never run. Saved with the macro-enabled workbook content type | `roundtrip.rs` (`vba_project_roundtrip`, `package_kind_sets_workbook_content_type`), engine `io.rs` (`macros_are_saved_only_into_xlsm`) |
| `.xltx` / `.xltm` templates | R/W | yes | partial | Saved with the template (and macro-enabled template) content type, chosen by extension; `.xltm` keeps a VBA project, `.xltx` drops it; "new from template" behaviour missing | `roundtrip.rs` (`package_kind_sets_workbook_content_type`), engine `io.rs` |
| `.xlsb` Binary Workbook | R/W | **values only** (#101) | no | Names, visibility, numbers, text, booleans, errors, merges, cached formula results; no formulas, formatting, charts, names | `xlsb_tests.rs` |
| `.xls` Excel 97-2003 (BIFF8) | R/W | **yes, partial** (#121) | no (Save As `.xlsx`) | Clean-room from [MS-CFB] and [MS-XLS]. Read: worksheets and visibility, shared strings, numbers, text, booleans, errors, formulas (text for the common tokens, shared and array formulas, 3-D refs, names, newer functions called by name) with cached results, number formats (dates display as dates), 1904 date system, merges, column widths, row heights, defined names, print area and titles, calculation settings (mode, iteration with its maximum iterations and change, precision as displayed), sheet display options (right-to-left, gridlines, headings, zero values, formulas; a truncated WINDOW2 is ignored). Not read: fonts, fills, borders, alignment, charts, pictures, comments, CF, validation, hyperlinks, freeze panes, external links (formula kept as its value). Chart and macro sheets skipped with a warning; encrypted (FILEPASS) and BIFF5 (Excel 5.0/95) files refused with a message. Opens as a new unsaved workbook. Unverified on a real-world corpus | `xls_tests.rs` |
| `.xlt` Excel 97-2003 template | R/W | yes (read as `.xls`) | no | | — |
| `.xlam` / `.xla` add-ins | R/W | no | no | VBA; out of scope | — |
| `.xml` XML Spreadsheet 2003 | R/W | no | no | Simple to support | — |
| `.ods` OpenDocument Spreadsheet | R/W | **values only** (#100) | no | Sheets, values, dates, merges, repeated cells, cached formula results; no formulas, formatting, charts; encrypted ODS refused | `ods_tests.rs` |
| Password-encrypted workbooks (Agile/Standard encryption) | R | **yes** (#11): asks for the password on open; Agile (AES-128/192/256, SHA-1/256/384/512) and Standard (AES, SHA-1) | no (opens with no save path; Save As writes it without the password) | Clean-room from [MS-OFFCRYPTO] on the CFB reader. RC4, Extensible and certificate-only encryption refused with a message. Data-integrity HMAC not checked | `crypto/tests.rs`, `encrypted_tests.rs` |
| `.xlw` Excel 4.0 workbook, `.wk*`, `.dbf` | open only (some) | no | no | Rare | — |

## Text and data formats

| Format | Excel | GridCraft read | GridCraft write | Notes |
|---|---|---|---|---|
| `.csv` UTF-8 / Windows / Mac / MS-DOS | R/W | yes (UTF-8 and UTF-16 BOMs, UTF-8, likely Windows-1256 Arabic detection, otherwise Windows-1252 fallback; delimiter detection) | yes (UTF-8 without BOM, CRLF; a file opened as CSV is saved with the separator it was read with, so a `;` file stays `;`; otherwise `,`) | Excel writes the **displayed** values; we write raw values. No encoding choice on save. Re-import of a single-column export splits text (#127) A used range that would write more than 512 MiB (two far-apart cells span the whole rectangle) is refused with an error instead of being expanded (#528). |
| `.txt` tab-delimited, Unicode text | R/W | yes (`.txt`, `.tsv`, `.tab`; likely Windows-1256 Arabic detection for invalid UTF-8) | yes | Text Import Wizard options partly via Text to Columns |
| `.csv` UTF-8 / Windows / Mac / MS-DOS | R/W | yes (UTF-8 and UTF-16 BOMs, UTF-8, likely Windows-1256 Arabic detection, otherwise Windows-1252 fallback; delimiter detection) | yes (UTF-8 without BOM, CRLF) | Writes the **displayed** values like Excel (number formats applied, formula results, never column-width `#####`). No encoding choice on save. Re-import of a single-column export splits text (#127) A used range that would write more than 512 MiB (two far-apart cells span the whole rectangle) is refused with an error instead of being expanded (#528). |
| `.txt` tab-delimited, Unicode text | R/W | yes (`.txt`, `.tsv`, `.tab`; likely Windows-1256 Arabic detection for invalid UTF-8) | yes (displayed values) | Text Import Wizard options partly via Text to Columns |
| `.prn` formatted text (space-delimited) | R/W | no | no | |
| `.dif`, `.slk` (SYLK) | R/W | no | no | Small, legacy |
| `.htm` / `.html` web page | R/W (Windows) | yes, via Get Data ▸ From HTML | yes (one sheet as a table; a used range taller than 100,001 rows fails with an error instead of being cut short) | Formatting not exported. A sheet with pictures or charts is written with the sheet's own column widths and row heights, a picture as a `data:` URI `<img>` and a chart as inline SVG, each placed at its anchor; shapes, text boxes and comments are not exported |
| `.mht` / `.mhtml` single-file web page | R/W (Windows) | no | no | |
| `.json` | via Power Query | GridCraft's own workbook JSON (debug) and JSON import | GridCraft JSON | Not Power Query's JSON shaping; notes and hyperlinks break export (#126) |
| `.xml` with XML maps | Developer ▸ Import/Export | no | no | |

## Export and print

| Format | Excel | GridCraft | Notes |
|---|---|---|---|
| PDF | Save As / Export | yes (own PDF 1.7 writer; CLI `convert` output) | Printed cell hyperlinks export as clickable HTTP(S)/mailto links or destinations within printed pages (#954); unsafe schemes and destinations outside the print range are omitted. Only the 14 standard PDF fonts, no embedding: **non-Latin text (CJK, Arabic, Devanagari, Thai) does not export**; web build exports without a file prompt (#141) |
| XPS | Windows | no | Low priority |
| Print | yes | yes (via PDF, system dialog on desktop; PR #143 for web) | |
| Images of ranges/charts (Copy as Picture) | yes | chart raster in `crates/chart` | No range-to-image command |

## Import sources (Get & Transform)

Excel's Get Data reaches files (Excel, CSV, XML, JSON, PDF, Parquet, folders), databases (SQL
Server, Access, ODBC, …), web pages, SharePoint and online services, through Power Query.
GridCraft imports CSV/TSV, JSON and HTML tables only, with no query editor, no connections and no
refresh. Counted under features (Get & Transform, ~10% ready), not here.

## Work (76–114 h)

| Work | Hours |
|---|---|
| Real-world XLSX corpus run (craftrules test-corpora standard), and confirm Excel opens our output with no repair prompt (owner runs Excel) | 15–25 |
| Preserve the rest of the unmodelled XLSX content (slicers, timelines, data model, table query tables, `extLst` entries); pass-through landed for the rest (#330) | 6–10 |
| Streaming XLSX reader for very large files (#175), part-size limits that degrade instead of failing | 10–15 |
| Legacy `.xls` (BIFF8): styles (fonts, fills, borders, alignment), comments, CF, validation, pictures and charts on import; corpus run; then write | 10–15 |
| Encrypted workbooks: save with password (#11) | 4–6 |
| XLSB and ODS: formulas and formatting on import; then write | 10–15 |
| CSV/text: encoding choice, `.prn`, `.dif`, `.slk`, XML Spreadsheet 2003 | 4–7 |
| PDF: embed subsetted system fonts so every script exports | (counted in [localization](localization-parity.md) script work) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-24 | trivial | Align the whole-axis source contribution with main; retain bounded cache scans and move its engine regressions into the consolidated integration suite |
| 2026-10-21 | minor | Preserve whole-axis PivotTable sources across XLSX saves, including sheet-local names, while scanning only existing data for cache records; generated round-trip and refresh regressions |
| 2026-10-21 | minor | HTML (Save as Web Page) writes pictures (`data:` URI `<img>`) and charts (inline SVG from the chart crate's drawing primitives, text included) at their anchors; the table uses the sheet's column widths and row heights when the sheet has either, and output for sheets without pictures or charts is unchanged |
| 2026-10-20 | trivial | XLSX shared strings: Japanese phonetic runs (ふりがな, `<rPh sb="…" eb="…">` with `<phoneticPr>`) are read into the workbook and written back next to the string with the same text, so an open + save no longer deletes them; bad offsets and readings are dropped with a warning, a `<phoneticPr>` without runs is kept on its own, and when duplicate `<si>` items share a text but not a reading only the first reading is written (reported on open) (`phonetic_tests.rs`) |
| 2026-10-20 | minor | A workbook theme's major/minor per-script typefaces (`<a:font script="Arab" typeface="…"/>`, also Hebr, Jpan and the rest) are read and written back in file order after `<a:cs>`; they were dropped before. A hostile file is held to 64 entries of 256 characters |
| 2026-10-20 | trivial | A workbook theme's major/minor East Asian (`<a:ea>`) and complex-script (`<a:cs>`) typefaces are read and written back, so a Japanese theme's fonts (e.g. 游ゴシック) survive open + save; they were deleted before because the model had nowhere to keep them. Rendering still resolves fonts without them (follow-up) |
| 2026-10-19 | minor | XLSX font charset and family values are preserved on save, retaining the character set and family used for substitute font selection |
| 2026-10-19 | minor | CSV and text import decodes likely Windows-1256 Arabic; other invalid UTF-8 retains the Windows-1252 fallback |
| 2026-10-19 | minor | .xls import skips dialog sheets (WsBool `fDialog`), which share the worksheet BOF type and were read as empty worksheets |
| 2026-10-19 | minor | `.xls` import reads WINDOW2: right-to-left sheets (common in older Arabic and Hebrew files) open right-to-left, and hidden gridlines, headings and zero values and Show Formulas carry over |
| 2026-10-19 | trivial | XLSX text boxes and shapes keep right-to-left paragraph direction (`a:pPr rtl`, ECMA-376 Part 1 §21.1.2.2.7): read from the first paragraph and written on every paragraph; output without it is unchanged |
| 2026-10-18 | trivial | `file.open {csvAsText: true}` keeps every CSV/TSV/TXT field as the text the file holds, so identifiers (GTINs, SKUs, ZIP codes) keep their digits and leading zeros (#1808; a preference and a CLI flag are separate work) |
| 2026-10-18 | trivial | XLSX PivotTables: `rowGrandTotals` is the Grand Total row and `colGrandTotals` the Grand Total column. Both were read and written the other way round, which a GridCraft round trip hid, so an Excel file with only one grand total opened showing the other |
| 2026-10-19 | minor | HTML (Save as Web Page and clipboard) marks right-to-left sheets and right-to-left cell text with `dir` |
| 2026-10-16 | trivial | XLSX charts: `[0]!Name` (a workbook-scoped defined name) in series references is read as the name, so the series resolve instead of showing `#NAME?`. Names in chart references are written qualified (`[0]!Name`, or `Sheet!Name` for a sheet-scoped name): Excel draws a chart with a bare name blank (#1686) |
| 2026-10-16 | trivial | XLSX reads and writes the workbook's show-objects setting (`workbookPr showObjects`: all, placeholders, none), which Ctrl+6 switches; a file without it is saved without it |
| 2026-10-16 | trivial | XLSX: pictures in headers and footers (`&G`, the VML drawing `<legacyDrawingHF>` names and its images) are kept on save; they are still not shown (#1257) |
| 2026-10-16 | minor | XLSX column imports refuse more than 1,000,000 cumulative expanded assignments per sheet before expanding the next span; repeated overlaps cannot bypass the work limit (#1022) |
| 2026-10-16 | minor | XLSX: a note keeps its box's place and size (`<x:Anchor>` and the VML width and height) through open and save, moving with its cell; notes made here get the default box (#1608) |
| 2026-10-16 | minor | XLSX PivotTables: two value fields without a column field are written with `firstHeaderRow="0"` and `hideValuesRow`, so Excel 365 doesn't add a `Values` row that can turn the report into `#SPILL!` (#1447, item 6) |
| 2026-10-16 | minor | XLSX PivotTables: a PivotChart's link to its PivotTable is written behind the workbook in brackets (`[Q1 Report.xlsx]Sheet2!PivotTable1`; `Workbook.xlsx` when the file name is unknown), so Excel binds the chart to the PivotTable instead of showing an ordinary chart; the reader drops the prefix (#1447, item 7) |
| 2026-10-15 | minor | XLSX: formulas and PivotTable sources quote sheet names like Excel (a leading `.`, R1C1-like and cell-like names), so `'.x'!A1` no longer saves as `.x!A1`, which Excel could not open (#1458) |
| 2026-10-15 | minor | XLSX text preserves CR and CRLF across save/reopen; shared strings use `_x000D_` and other XML text uses `&#13;` (#1488) |
| 2026-10-15 | minor | XLSX save keeps legacy `INDEX`/`MATCH` formulas scalar instead of marking them as dynamic arrays (#1505) |
| 2026-10-14 | trivial | Formula parser: text longer than Excel's 8,192 characters, and postfix chains (`1%%%…`, `f(1)(1)…`) past the 256-level nesting limit, are parse errors (the cell keeps its formula text and cached value) instead of trees deep enough to overflow the stack when a hostile file is opened, saved or dropped |
| 2026-10-14 | minor | XLSX formulas read with dynamic-array metadata (`cm`) keep it on save, so Excel does not recalculate them with implicit intersection |
| 2026-10-14 | minor | PDF export writes clickable annotations for safe cell URLs and destinations on printed pages; CLI `convert` accepts `.pdf` output (#954) |
| 2026-10-13 | trivial | XLSX export keeps a picture shared by several sheets as one media part (#985) |
| 2026-10-12 | trivial | XLSX chart sheets are kept on save (#727) |
| 2026-10-12 | trivial | CSV and TSV export is capped at 512 MiB and refuses a larger used range with an error (#528) |
| 2026-10-12 | minor | `.xls` import reads the calculation settings (CalcMode, CalcIter, CalcCount, CalcDelta, CalcPrecision), so circular models calculate instead of showing `#CIRC!` (#724) |
| 2026-10-12 | minor | Ink paths and editing survive XLSX save/reopen; strict polyline import and malformed-input controls |
| 2026-10-12 | minor | `.xls` import: the other cells of an array formula ({=FREQUENCY(...)}, {=TREND(...)}) drop their cached results, as `.xlsx` import does, so the formula fills its range on recalculation instead of showing `#SPILL!` against its own old values; 79 fewer mismatching formulas and 16 fewer round-trip differences over 4,224 real-world files |
| 2026-10-12 | minor | macOS: the app bundle declares `.xlsb` and `.ods`, so Finder's Open With offers GridCraft for them, as Linux and Windows already did (#321) |
| 2026-10-12 | minor | CSV: saving a file opened as CSV keeps its detected separator (`;` files were rewritten with `,`) |
| 2026-10-11 | minor | XLSX: cell/shared/array formulas read through the file→model conversion (`_xlpm.` LET/LAMBDA parameter prefixes stripped); table column names keep line breaks; table names may start with `\` |
| 2026-10-11 | minor | CSV/TSV export writes displayed values (number formats applied, no `#####`); `.xltx`, `.xltm` and `.xlsm` saved with their own workbook content types |
| 2026-10-11 | minor | Table AutoFilter criteria are read and written (they were dropped, leaving only the filter button), and `iconFilter` criteria are kept on table and sheet AutoFilters |
| 2026-10-11 | minor | Password-encrypted workbooks open with their password (Agile and Standard encryption, #11) |
| 2026-10-11 | minor | `.xls` (BIFF8) import: values, formulas, number formats, merges, sizes, names (#121) |
| 2026-10-11 | minor | XLSX pass-through: custom XML, external links, connections, query tables, printer settings, form/ActiveX controls, OLE objects, sheet backgrounds kept on save (#330, #324) |
| 2026-10-11 | minor | XLSX: threaded comments and persons parts written (they came back as notes before); note visibility read from VML (#300) |
| 2026-10-11 | minor | XLSX: Allow Edit Ranges (`protectedRanges`) read and written with their password hashes (#307) |
| 2026-10-10 | minor | Encrypted and `.xls` files are now recognised and refused with a message (#17) |
| 2026-10-10 | major | Created: every Excel format with our read/write support; XLSB and ODS values-only imports recorded; `.xls`, encryption and large files flagged as beta gates |
