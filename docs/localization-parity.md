# Localization parity

> **Last reviewed:** 2026-11-14 · **Last updated:** 2026-11-14 · **Change:** major
> (replaced historical coverage estimates with the current catalog report;
> clarified script, RTL and PDF limitations; Spanish function help now has
> 761 entries and no obsolete keys) · **Target:** Microsoft Excel (Microsoft 365)

This document describes the current localization state and the evidence used
to measure it. Historical entries are retained in the revision history, but
their counts and estimates must not be read as current measurements.

## Scope and measurement

GridCraft registers 19 values in the `Language` enum, including English as
the source/fallback language. There are 18 translated UI catalog files under
`crates/ui-egui/src/i18n/`. English is not represented by a separate
translated catalog. `es_functions.rs` is a separate Spanish function-help
catalog, not another UI language.

`cargo xtask i18n-report` reads the UI catalog source files. Its reference set
is the union of keys found in the translated UI catalogs: currently 1,199
distinct keys across 18 catalogs. For each language, the report counts keys
missing from that union. A missing key falls back to English.

These figures measure **key presence only**. They do not establish that a
translation differs from the English source, is accurate, has been reviewed
by a native speaker, or covers every string visible in the application.
The 1,199-key union is not a complete inventory of all user-visible text.
The report is informational: missing translations do not fail CI.

The report's source scanner recognizes the catalog patterns used by the
current Rust files, including match arms and table rows. It is not a complete
Rust parser. The report also checks Spanish function-help keys against the
current English function catalog and reports obsolete entries.

## Interface catalogs

The following figures are the current output of `cargo xtask i18n-report`:

| Language | Code | Keys present | Keys missing |
|---|---:|---:|---:|
| Czech | cs | 579 | 620 |
| German | de | 496 | 703 |
| Greek | el | 995 | 204 |
| Spanish | es | 504 | 695 |
| French | fr | 464 | 735 |
| Italian | it | 996 | 203 |
| Japanese | ja | 1,138 | 61 |
| Korean | ko | 474 | 725 |
| Dutch | nl | 545 | 654 |
| Polish | pl | 995 | 204 |
| Portuguese (Brazil) | pt | 1,004 | 195 |
| Romanian | ro | 1,040 | 159 |
| Russian | ru | 474 | 725 |
| Serbian | sr | 463 | 736 |
| Turkish | tr | 434 | 765 |
| Ukrainian | uk | 995 | 204 |
| Vietnamese | vi | 434 | 765 |
| Simplified Chinese | zh | 474 | 725 |

The enum uses the internal codes `pt` and `zh`, not `pt-BR` and `zh-Hans`.
Regional or script locale tags may be accepted as aliases, but they do not
change the catalog identifiers. Hindi (`hi`), Arabic (`ar`) and Indonesian
(`id`) are not registered in the enum and have no UI catalogs.

## Spanish function help

`crates/ui-egui/src/i18n/es_functions.rs` contains 761 entries:

- 479 function descriptions.
- 282 argument names.
- 0 entries keyed by English text that the current function catalog no
  longer contains.

Function-help coverage is separate from the UI-catalog counts above. The
report's obsolete-key check establishes that these entries still correspond
to the current English function catalog; it does not assess translation
quality or native-speaker review.

## Script rendering, input and directionality

Font fallback and text shaping are separate capabilities. Existing system-font
fallbacks for scripts such as CJK, Thai and Hebrew do not by themselves prove
that text is shaped or laid out correctly for every writing system. IME input
for Chinese, Japanese and Korean also requires separate verification.

The source audit did not find evidence sufficient to claim complete Unicode
bidirectional layout or complex-script shaping. In particular, the project
should not claim complete Arabic or Devanagari shaping and bidi support on
the basis of the features listed below.

### Right-to-left worksheets

GridCraft reads and writes the XLSX worksheet `rightToLeft` property and
preserves the corresponding legacy `.xls` worksheet setting. The grid can
therefore represent right-to-left sheet geometry. XLSX cell-style
`readingOrder` values are also read and written, including the right-to-left
value.

These file-format properties are not equivalent to full bidirectional text
layout. Text direction in a cell, paragraph direction in a shape, sheet
geometry and Unicode bidi reordering are distinct concerns. Shapes can retain
right-to-left paragraph metadata in XLSX without GridCraft necessarily
rendering their text with the corresponding shaping and visual ordering.

### PDF export

PDF export uses the standard PDF fonts without embedding for WinAnsi text.
The PDF implementation also has a CID route using the predefined Adobe-GB1
character collection and `UniGB-UTF16-H` for certain characters outside
WinAnsi, including CJK and other characters covered by that route.

This is not universal non-Latin typography support. Coverage depends on the
available encoding and CID character collection; complete shaping, bidi
layout, font coverage and consistent text measurement are not guaranteed for
Arabic, Devanagari or every other complex writing system. The sheet-printing
path also measures text using the project's text-width machinery, so correct
measurement and visual output must be verified for scripts using the CID path.

## Locale-aware formulas and regional formats

The UI-catalog counts above do not measure localized function names, argument
separators, decimal/list separators, or localized number and date formats.
These are separate localization dimensions and must be assessed independently.
The historical estimates in this document are not current effort estimates.

## Human linguistic review

Catalog presence is not evidence of human linguistic review. The repository
documentation describes translations as AI-assisted and not yet reviewed by
native speakers. In particular, the former `yes` entry for Turkish native
review is not supported by the inspected commit history or review records.
Its status should be treated as **not documented / pending verification**,
not as a confirmed review and not as proof that no review has ever happened.

Review status should be recorded per language only when there is evidence of
the review, ideally identifying the reviewed catalog/version and the review
scope. Automated code review is not linguistic review.

## Remaining work

1. Expand and maintain UI catalogs, using `cargo xtask i18n-report` to identify
   missing keys. The report is a presence check, not a quality assessment.
2. Assess localized function names, argument separators and regional number,
   date and CSV formats independently of UI translation.
3. Verify font fallback, shaping, bidirectional layout and IME input for each
   relevant script and platform; do not infer complete support from font
   fallback or worksheet RTL metadata.
4. Verify PDF output for non-Latin scripts, including glyph coverage,
   shaping, text direction and measurement.
5. Record human linguistic review with evidence for each language.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-10 | minor | Ukrainian Chart is `Діаграма`, matching [craft-i18n#211](https://github.com/storytold/craft-i18n/pull/211) and distinct from Diagram (`Схема`) |
| 2026-11-10 | minor | Diagram and Decorative Text labels in all 18 non-English catalogs; Diagram never repeats the catalog's chart word (German `Schema`, Ukrainian `Схема`, Romanian `Schemă`), see [feature-names.md](feature-names.md) |
| 2026-10-31 | minor | System and embedded CJK text fallbacks share the Latin baseline in cells, editors and window titles; other scripts, symbols and emoji keep their alignment |
| 2026-10-26 | historical claim, not revalidated | Earlier work recorded shared bidi layout for Arabic clusters, marks, visual cursor navigation and directional accessibility. The 2026-11-14 source audit did not revalidate this as complete Unicode bidi or complex-script support; do not treat this entry as the current capability statement |
| 2026-10-26 | minor | Arabic text renders in cells and the interface: a system Arabic face (Tahoma, Arial or Segoe UI on Windows, Geeza Pro on macOS, Noto Sans or Naskh Arabic on Linux) joins the script fallbacks, fontconfig's Arabic choice, when it has Arabic letters, leads the Arabic fallbacks and draws Arabic wherever the UI or cell font has none, such as the default cell fonts (part of #229; a UI font with Arabic of its own, such as DejaVu Sans in the Flatpak, still draws interface Arabic, left for a follow-up), and craft-fonts' Noto Sans Arabic is a required face allowed on the web. Workbook fonts without Arabic keep every script fallback. Shaping and bidirectional layout remain separate work |
| 2026-10-27 | minor | Number formats honour the `B1`/`B2` calendar prefixes: never shown; `B2` renders year, month, day and Arabic month and weekday names in the tabular Hijri calendar (1 Jan 2001 = 6 Shawwal 1421; leap years 2, 5, 7, 10, 13, 15, 18, 21, 24, 26 and 29 of the 30-year cycle, which is what Excel shows). Checked against Excel 16.0 (build 20527, en-US) on every serial and about 1,700 format/value pairs: a locale or currency tag before `B2` keeps the date Gregorian, a misplaced `B` and digit or a prefix before a number format is rejected, serial 60 has no Hijri date, serial 0 shows as serial 1, two-word month names use a no-break space. Not matched: Excel's weekday/month-name mix-ups in one `B2` section and its weekday-only quirks |
| 2026-10-27 | minor | AutoFit column width, wrapped-row height and PivotTable column fit estimate Arabic letters at ~0.48 em (measured 0.36–0.53 em shaped, Noto Sans/Naskh Arabic and DejaVu Sans) instead of 1 em, Arabic-Indic digits as wide as ASCII digits, and give vowel marks and direction marks no width; vowelled Arabic no longer fits ~3.5× too wide |
| 2026-10-23 | minor | Missing translations are reported, never a failure: `cargo xtask i18n-report` prints, per language, the strings other catalogs translate and it lacks (plus Spanish function help rows keyed by English the function catalog no longer has) and always passes; `cargo xtask ci` runs it first. Coverage and per-language parity tests and tests of one language's exact wording are gone; UI tests take the expected text from the catalog. Duplicate-key, placeholder, context-prefix and line-shape checks stay |
| 2026-10-21 | minor | Romanian interface (`ro`, Română): 970 labels covering every key of the other catalogs (`crates/ui-egui/src/i18n/ro.rs`), `ro-*` system locales, the View-tab switcher and `app.language.set`; context keys show no prefix (File › Options test). English formulas and formats; AI-assisted draft pending native-speaker review |
| 2026-10-20 | minor | Japanese: furigana (`rPh` phonetic runs and `<phoneticPr>` in `xl/sharedStrings.xml`) is no longer deleted by an open + save; it is kept on the workbook and written back (a `<phoneticPr>` without runs is kept too, while duplicate texts with different readings keep only the first, reported on open). Display of the readings is still missing (`crates/xlsx/src/tests/phonetic_tests.rs`) |
| 2026-10-20 | minor | Japanese catalog (`i18n/ja.rs`) covers every key the Brazilian Portuguese catalog translates: the 398-label shared core moved verbatim plus 545 dialog, message, Backstage and menu strings — 943 keys, ~12% → ~43%. A test parses `pt.rs`'s source, so the two catalogs can't drift apart again. Portuguese re-measured at 943 (was 923) |
| 2026-10-20 | minor | Dutch (nl) interface catalog (`i18n/nl.rs`, 518 labels, ~24%): the shared key set every language must cover (ribbon, context menus, File view, dialog titles, Options page), matching the German, French and Czech catalogs; `nl-NL`, `nl-BE` locale tags resolve to Dutch |
| 2026-10-20 | minor | XLSX: a theme's per-script typefaces (`<a:font script="Arab">`, Hebr, Jpan…) are kept through open + save, so Arabic Excel's theme fonts for shapes, charts and theme-font cells survive a GridCraft save |
| 2026-10-20 | trivial | XLSX: a workbook theme's major/minor East Asian (`<a:ea>`) and complex-script (`<a:cs>`) typefaces are kept through open + save, so a Japanese workbook keeps its theme's East Asian font; rendering does not resolve them yet (follow-up) |
| 2026-10-19 | minor | Text order (Sort, SORT/SORTBY, `<`/`>`, approximate MATCH/XLOOKUP, `"<x"` criteria) compares letters first: Arabic vowel and Quranic marks, tatweel and invisible format characters (bidi marks, embeddings and isolates, ZWJ/ZWNJ, BOM, soft hyphen) no longer decide where text sorts, and Arabic-Indic digits sort as digits. Text with the same letters then sorts letter by letter, unmarked before marked, marks in Excel's order (sukun before shadda) and digits ASCII, Arabic-Indic, Extended Arabic-Indic; checked against Excel 16.0 build 20527 (en-US). Equality is unchanged |
| 2026-10-19 | minor | XLSX fonts retain charset and family metadata so systems can select a substitute font with the intended script when the named font is unavailable |
| 2026-10-19 | minor | Arabic CSV and text imports decode Windows-1256 text; Arabic shaping and bidirectional UI remain unsupported |
| 2026-10-19 | minor | Defined-name length counts characters, not bytes: Arabic names up to 255 letters are accepted, and Create from Selection keeps long Arabic headers |
| 2026-10-19 | minor | Table names are limited to 255 Unicode characters |
| 2026-10-19 | minor | Right-to-left sheets in legacy `.xls` files (common in older Arabic workbooks) open right-to-left |
| 2026-10-19 | trivial | Text boxes and shapes keep right-to-left paragraph direction (`a:pPr rtl`) on XLSX save, so Excel-authored Arabic and Hebrew text boxes reopen right to left |
| 2026-10-19 | minor | Save as Web Page and copied HTML mark right-to-left sheets `dir="rtl"`, let General-aligned Arabic and Hebrew text align by its own direction (`dir="auto"`), and keep numbers on right-to-left sheets left to right and on the left |
| 2026-10-19 | minor | French and Serbian kept the context in three `context\|label` translations ("Options\|Général", "Opcije\|Opšte" for `Options\|General`; also `Options\|Save`, `Sheet Options\|View`). `tr` shows a found translation as it is, so File › Options headings and the Sheet Options "View" checkboxes showed the prefix. Fixed; a test checks that no catalog shows a `\|` |
| 2026-10-17 | minor | Vietnamese catalog (`i18n/vi.rs`, every key in the master table); `vi` locale tags resolve to Vietnamese |
| 2026-10-17 | minor | Lao text falls back to an installed Lao face (`sys-lao`) instead of tofu boxes |
| 2026-10-15 | minor | Function help in the interface language: argument hints, autocomplete, Insert Function (search, categories) and the Formulas-tab menus read per-language tables of descriptions and argument names keyed by the catalog's English text; Spanish keeps its 10 functions in the new format |
| 2026-10-15 | minor | Italian catalog (`i18n/it.rs`, 971 entries, ~44%): every key the Brazilian Portuguese catalog translates plus the PivotTable Fields pane strings; `it` locale tags (it-IT, it-CH…) resolve to Italian. Labels follow Excel's Italian terminology; native-speaker review still pending |
| 2026-10-14 | minor | CJK fallback probes the read-only Flatpak host font mounts with the existing distribution layouts and TTC locale indices; no new sandbox permissions or bundled fonts |
| 2026-10-12 | minor | Turkish ribbon catalog (`i18n/tr.rs`, 382 labels); `tr` locale tags (tr-TR, tr_TR…) resolve to Turkish |
| 2026-10-12 | minor | Format Cells looks up its tabs, labels, options and note through `Language::tr` (Fill tab keyed `Format Cells\|Fill`); dialog titles and OK/Cancel too. Labels already in a table (Number, Currency, Fill…) show translated; the rest fall back to English until the tables add them |
| 2026-10-12 | minor | Brazilian Portuguese catalog (`i18n/pt.rs`) grows from 377 to 923 entries: every key the other catalogs translate, including the dialog strings #669 starts looking up (~42%) |
| 2026-10-11 | minor | German ribbon catalog (`i18n/de.rs`, 266 labels, ~12%); `de` locale tags (de-DE, de-AT, de-CH…) resolve to German. PR #163 (localized German formulas/numbers) still open |
| 2026-10-11 | minor | Sheet Right-to-Left mirrors the grid (#146) |
| 2026-10-11 | minor | Czech (cs) ribbon table: 266 labels (~12%), menus only |
| 2026-10-10 | minor | Re-measured after #31, #104, #82, #38 merged: ribbon translated into ja, zh, ko, pt-BR and ru (266–268 labels, ~12%), system-language default, CJK/Thai/Hebrew font fallback; ~10% ready, 100–165 h |
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
