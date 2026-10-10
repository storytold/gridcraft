# File-format parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created; every format Excel reads or writes, against the code on main) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Every format Excel opens or saves, with GridCraft's read and write support, fidelity and tests.
Part of [target-app-parity.md](target-app-parity.md). **File formats: ~50% ready, 80–120 h**
(estimated).

Excel's list comes from Microsoft's public "File formats that are supported in Excel"
documentation (the Office bundle is off limits under this repo's clean-room rule). GridCraft's
support comes from `crates/engine/src/io.rs` (`FileKind`), `crates/xlsx/src` and the tests in
`crates/xlsx/src/tests/` (round-trip, fixtures, malformed input, CSV, ODS, XLSB, PivotTables).

## Workbook formats

| Format | Excel | GridCraft read | GridCraft write | Fidelity / notes | Tests |
|---|---|---|---|---|---|
| `.xlsx` Excel Workbook (transitional) | R/W, native | yes | yes | Cells, shared/array/dynamic formulas, styles, themes, merges, CF, validation, tables, comments and notes, hyperlinks, names, freeze panes, outline, print settings, pictures, shapes, charts (simplified, see [chart-parity.md](chart-parity.md)), sparklines, PivotTables (caches and tables), protection, calc settings. **Not preserved:** unmodelled parts and extensions (form controls, slicer caches, timelines, external links, OLE objects, custom XML, data model, query tables, chart details). Never checked in Excel across a corpus: repair prompts unknown. 100 MB files fail (#175) | `roundtrip.rs`, `fixtures.rs`, `malformed.rs`, `pivot_tests.rs` |
| `.xlsx` Strict Open XML | R/W | yes | no (writes transitional) | Excel also saves transitional by default | fixtures |
| `.xlsm` Macro-Enabled Workbook | R/W | yes | yes, **without macros** | VBA project dropped with a warning; saving as `.xlsm` loses the macros (#6) | — |
| `.xltx` / `.xltm` templates | R/W | yes | partial | Saved with the workbook content type, not the template one; "new from template" behaviour missing | — |
| `.xlsb` Binary Workbook | R/W | **values only** (#101) | no | Names, visibility, numbers, text, booleans, errors, merges, cached formula results; no formulas, formatting, charts, names | `xlsb_tests.rs` |
| `.xls` Excel 97-2003 (BIFF8) | R/W | **no** (#121); recognised and refused with a message since #17 | no | Still common in finance, government and legacy systems | — |
| `.xlt` Excel 97-2003 template | R/W | no | no | | — |
| `.xlam` / `.xla` add-ins | R/W | no | no | VBA; out of scope | — |
| `.xml` XML Spreadsheet 2003 | R/W | no | no | Simple to support | — |
| `.ods` OpenDocument Spreadsheet | R/W | **values only** (#100) | no | Sheets, values, dates, merges, repeated cells, cached formula results; no formulas, formatting, charts; encrypted ODS refused | `ods_tests.rs` |
| Password-encrypted workbooks (Agile/Standard encryption) | R/W | **no**; recognised and refused with a clear message since #17 | no | Common in business; needs AES decryption on top of the new CFB reader (`crates/xlsx/src/cfb.rs`) | — |
| `.xlw` Excel 4.0 workbook, `.wk*`, `.dbf` | open only (some) | no | no | Rare | — |

## Text and data formats

| Format | Excel | GridCraft read | GridCraft write | Notes |
|---|---|---|---|---|
| `.csv` UTF-8 / Windows / Mac / MS-DOS | R/W | yes (UTF-8 and UTF-16 BOMs, UTF-8, Windows-1252 fallback; delimiter detection) | yes (UTF-8 without BOM, CRLF) | Excel writes the **displayed** values; we write raw values. No encoding choice on save. Re-import of a single-column export splits text (#127) |
| `.txt` tab-delimited, Unicode text | R/W | yes (`.txt`, `.tsv`, `.tab`) | yes | Text Import Wizard options partly via Text to Columns |
| `.prn` formatted text (space-delimited) | R/W | no | no | |
| `.dif`, `.slk` (SYLK) | R/W | no | no | Small, legacy |
| `.htm` / `.html` web page | R/W (Windows) | yes, via Get Data ▸ From HTML | yes (one sheet as a table) | Formatting not exported |
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

## Work (80–120 h)

| Work | Hours |
|---|---|
| Real-world XLSX corpus run (craftrules test-corpora standard), and confirm Excel opens our output with no repair prompt (owner runs Excel) | 15–25 |
| Preserve unmodelled XLSX parts and extensions on round trip (pass-through with relationship fix-up) | 10–15 |
| Streaming XLSX reader for very large files (#175), part-size limits that degrade instead of failing | 10–15 |
| Legacy `.xls` (BIFF8) import: cells, formulas, styles, names; then write | 20–30 |
| Encrypted workbooks: open with password prompt, save with password (#11) | 8–12 |
| XLSB and ODS: formulas and formatting on import; then write | 10–15 |
| CSV/text: displayed-value export, encoding choice, `.prn`, `.dif`, `.slk`, XML Spreadsheet 2003 | 5–8 |
| PDF: embed subsetted system fonts so every script exports | (counted in [localization](localization-parity.md) script work) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Encrypted and `.xls` files are now recognised and refused with a message (#17) |
| 2026-10-10 | major | Created: every Excel format with our read/write support; XLSB and ODS values-only imports recorded; `.xls`, encryption and large files flagged as beta gates |
