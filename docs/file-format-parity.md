# File-format parity

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-14 · **Change:** minor (XLSX keeps every core document property and chart sheets on save) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Every format Excel opens or saves, with GridCraft's read and write support, fidelity and tests.
Part of [target-app-parity.md](target-app-parity.md). **File formats: ~50% ready, 80–120 h**
(estimated).

Excel's list comes from Microsoft's public "File formats that are supported in Excel"
documentation (the Office bundle is off limits under this repo's clean-room rule). GridCraft's
support comes from `crates/engine/src/io.rs` (`FileKind`), `crates/xlsx/src` and the tests in
`crates/xlsx/src/tests/` (round-trip, fixtures, malformed input, CSV, ODS, XLSB, XLS, PivotTables).

## Workbook formats

Ink created with Draw is written as standard, unfilled DrawingML polylines, without visible point
captions or private metadata. Supported numeric polylines reopen as editable ink, retaining points,
colour, integer width and anchor. Curves, filled paths, transforms and unsupported line styles are
not interpreted as ink. This does not implement Office InkML or fix inserted icons (#551).
Command and package tests cover repeated saves and unrelated cell edits; external Excel rendering
and repair-free opening remain unverified.

| Format | Excel | GridCraft read | GridCraft write | Fidelity / notes | Tests |
|---|---|---|---|---|---|
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings, document properties (every `docProps/core.xml` property: title, subject, author, keywords, comments, last modified by, created, modified, last printed, category, status, revision, version, identifier, language; `Company` from `docProps/app.xml`; Save sets modified and last modified by, and created the first time, as Excel does). **Kept unchanged on save (pass-through, `passthrough.rs`):** custom document properties, ribbon customisation and add-in task panes, custom XML, links to external workbooks (never followed or refreshed), data connections, sheet query tables, printer settings (#324), form controls, ActiveX controls, OLE objects and sheet backgrounds (with their VML shapes), and chart sheets (#727: the sheet, drawing, chart, style and colour parts are written back unchanged in their place, and left out when a sheet they plot was deleted or renamed or a worksheet took their name; they can't be edited or created); dropped when their sheet is deleted or their parts point at content GridCraft rewrites. **Not preserved:** slicers and slicer caches, timelines, data model, query tables attached to tables, sheet and workbook `extLst` entries we don't model, chart details, the other `docProps/app.xml` properties (Manager, Hyperlink base); Strict Open XML files keep none of this pass-through content. Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, notes (with their shown/hidden state) and threaded comments with replies, resolved state and timestamps (`threadedComments` and `persons` parts, #300), hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, Allow Edit Ranges (`protectedRanges` with their password hashes, #307), calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Strict Open XML | R/W | yes | no (writes transitional) | Excel also saves transitional by default | fixtures |
| `.xlsm` Macro-Enabled Workbook | R/W | yes | yes, macros kept, not run | The VBA project is kept byte-for-byte and written back only into `.xlsm`/`.xltm` (#6); macros never run. Saved with the macro-enabled workbook content type | `roundtrip.rs` (`vba_project_roundtrip`, `package_kind_sets_workbook_content_type`), engine `io.rs` (`macros_are_saved_only_into_xlsm`) |
| `.xltx` / `.xltm` templates | R/W | yes | partial | Saved with the template (and macro-enabled template) content type, chosen by extension; `.xltm` keeps a VBA project, `.xltx` drops it; "new from template" behaviour missing | `roundtrip.rs` (`package_kind_sets_workbook_content_type`), engine `io.rs` |
| `.xlsb` Binary Workbook | R/W | **values only** (#101) | no | Names, visibility, numbers, text, booleans, errors, merges, cached formula results; no formulas, formatting, charts, names | `xlsb_tests.rs` |
| `.xls` Excel 97-2003 (BIFF8) | R/W | **yes, partial** (#121) | no (Save As `.xlsx`) | Clean-room from [MS-CFB] and [MS-XLS]. Read: worksheets and visibility, shared strings, numbers, text, booleans, errors, formulas (text for the common tokens, shared and array formulas, 3-D refs, names, newer functions called by name) with cached results, number formats (dates display as dates), 1904 date system, merges, column widths, row heights, defined names, print area and titles, calculation settings (mode, iteration with its maximum iterations and change, precision as displayed). Not read: fonts, fills, borders, alignment, charts, pictures, comments, CF, validation, hyperlinks, freeze panes, external links (formula kept as its value). Chart and macro sheets skipped with a warning; encrypted (FILEPASS) and BIFF5 (Excel 5.0/95) files refused with a message. Opens as a new unsaved workbook. Unverified on a real-world corpus | `xls_tests.rs` |
| `.xlt` Excel 97-2003 template | R/W | yes (read as `.xls`) | no | | — |
| `.xlam` / `.xla` add-ins | R/W | no | no | VBA; out of scope | — |
| `.xml` XML Spreadsheet 2003 | R/W | no | no | Simple to support | — |
| `.ods` OpenDocument Spreadsheet | R/W | **values only** (#100) | no | Sheets, values, dates, merges, repeated cells, cached formula results; no formulas, formatting, charts; encrypted ODS refused | `ods_tests.rs` |
| Password-encrypted workbooks (Agile/Standard encryption) | R | **yes** (#11): asks for the password on open; Agile (AES-128/192/256, SHA-1/256/384/512) and Standard (AES, SHA-1) | no (opens with no save path; Save As writes it without the password) | Clean-room from [MS-OFFCRYPTO] on the CFB reader. RC4, Extensible and certificate-only encryption refused with a message. Data-integrity HMAC not checked | `crypto/tests.rs`, `encrypted_tests.rs` |
| `.xlw` Excel 4.0 workbook, `.wk*`, `.dbf` | open only (some) | no | no | Rare | — |

## Text and data formats

| Format | Excel | GridCraft read | GridCraft write | Notes |
|---|---|---|---|---|
| `.csv` UTF-8 / Windows / Mac / MS-DOS | R/W | yes (UTF-8 and UTF-16 BOMs, UTF-8, Windows-1252 fallback; delimiter detection) | yes (UTF-8 without BOM, CRLF; a file opened as CSV is saved with the separator it was read with, so a `;` file stays `;`; otherwise `,`) | Excel writes the **displayed** values; we write raw values. No encoding choice on save. Re-import of a single-column export splits text (#127) A used range that would write more than 512 MiB (two far-apart cells span the whole rectangle) is refused with an error instead of being expanded (#528). |
| `.txt` tab-delimited, Unicode text | R/W | yes (`.txt`, `.tsv`, `.tab`) | yes | Text Import Wizard options partly via Text to Columns |
| `.csv` UTF-8 / Windows / Mac / MS-DOS | R/W | yes (UTF-8 and UTF-16 BOMs, UTF-8, Windows-1252 fallback; delimiter detection) | yes (UTF-8 without BOM, CRLF) | Writes the **displayed** values like Excel (number formats applied, formula results, never column-width `#####`). No encoding choice on save. Re-import of a single-column export splits text (#127) A used range that would write more than 512 MiB (two far-apart cells span the whole rectangle) is refused with an error instead of being expanded (#528). |
| `.txt` tab-delimited, Unicode text | R/W | yes (`.txt`, `.tsv`, `.tab`) | yes (displayed values) | Text Import Wizard options partly via Text to Columns |
| `.prn` formatted text (space-delimited) | R/W | no | no | |
| `.dif`, `.slk` (SYLK) | R/W | no | no | Small, legacy |
| `.htm` / `.html` web page | R/W (Windows) | yes, via Get Data ▸ From HTML | yes (one sheet as a table; a used range taller than 100,001 rows fails with an error instead of being cut short) | Formatting not exported |
| `.mht` / `.mhtml` single-file web page | R/W (Windows) | no | no | |
| `.json` | via Power Query | GridCraft's own workbook JSON (debug) and JSON import | GridCraft JSON | Not Power Query's JSON shaping; notes and hyperlinks break export (#126) |
| `.xml` with XML maps | Developer ▸ Import/Export | no | no | |

## Export and print

| Format | Excel | GridCraft | Notes |
|---|---|---|---|
| PDF | Save As / Export | yes (own PDF 1.7 writer) | Only the 14 standard PDF fonts, no embedding: **non-Latin text (CJK, Arabic, Devanagari, Thai) does not export**; web build exports without a file prompt (#141) |
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
| 2026-10-14 | trivial | Formula parser: text longer than Excel's 8,192 characters, and postfix chains (`1%%%…`, `f(1)(1)…`) past the 256-level nesting limit, are parse errors (the cell keeps its formula text and cached value) instead of trees deep enough to overflow the stack when a hostile file is opened, saved or dropped |
| 2026-10-12 | minor | XLSX: the core document properties beyond title, subject, author, keywords and comments (created, modified, last modified by, last printed, category, status, revision, version, identifier, language) are read and written back (they were dropped on save), and Save records the save time and the user name (`modified`, `lastModifiedBy`; `created` the first time) as Excel does |
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
