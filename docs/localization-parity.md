# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-11-10 · **Change:** minor (Ukrainian `Chart` is `Діаграма`, matching [craft-i18n#211](https://github.com/storytold/craft-i18n/pull/211) and distinct from `Diagram` (`Схема`); retain Diagram and Decorative Text labels in the non-English catalogs; merge current main's CJK fallback baseline alignment updates (#1265, #2621) while retaining localized formula and catalog changes; earlier: reconcile the language counts and Languages table with shipped catalogs; correct catalog, native menu and regional-format review findings; Arabic bidi display, editing and cell reading direction; Arabic font fallback; number formats honour the `B1`/`B2` calendar prefixes) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~10% ready, 100–165 h** (estimated).

## Russian update (2026-10-29)

Russian now extends #676's catalog with the current catalog union and newer main-only keys.
The Insert ribbon tab uses the noun «Вставка» separately from the Insert command «Вставить»;
the Start and Save Changes dialogs look up their visible text at draw time. Missing strings
continue to fall back to English; this is coverage of catalog keys, not every literal in the UI.

Formula input adapts #1850's 199 aliases to the current parser and help architecture. Unicode
LET/LAMBDA name precedence and iterative handling of long flat expressions are retained.
Russian help for all 515 currently registered functions (479 distinct descriptions and 283
argument keys) is English-keyed in `i18n/ru_functions.rs`. Original keys are retained, with terminology corrections;
new descriptions translate GridCraft's own catalog. Localized error
literals round-trip at the editor boundary, with canonical storage/API/XLSX syntax. Fixed
messages use `tr_error`; selected dynamic parser/parameter messages preserve captured user data.
The Format Cells editor displays Russian date/time tokens while keeping canonical format
codes in storage. Full regional number/date rendering remains outstanding.

## Czech catalog audit (2026-11-05)

The Czech interface catalog explicitly covers the 2,427 keys in the current audited
inventory, including diagnostic templates, generated names, macro controls, chart and
filter dialogs, printing and update prompts. A regression checks this inventory.
This is catalog coverage; connecting every remaining UI literal is a separate audit.
The 479 function descriptions and 283 argument labels were read against the English
catalog, with corrections to statistical, financial and text-function terminology.
This pass uses direct language review without an external translation service;
native-speaker review remains welcome. Formula aliases and stored XLSX syntax are unchanged.
F.TEST/FTEST help now describes a two-tailed p-value rather than a probability that
variances are equal; its English key and all existing translated help rows change together.

## Function help coverage

Twenty languages have descriptions and argument labels for the current
515-function catalog: the English source plus Czech, German, Greek, Spanish, French, Italian,
Japanese, Korean, Dutch, Polish, Brazilian Portuguese, Romanian, Russian, Serbian, Turkish,
Ukrainian, Vietnamese, Simplified Chinese and Traditional Chinese. Danish, Swedish, Georgian
and Lithuanian use English help. Each translated table covers 479 distinct
descriptions and 283 argument keys. Existing Spanish/Russian help and the previously added
translated hints are reviewed alongside the new rows, preserving their keys and correcting mistranslations. Mathematical variables such as `x`, `y` and `n`, and identifiers
such as XML/XPath, intentionally stay recognizable.

The existing `i18n/<lang>_functions.rs` representation is shared by all languages. A single
hint builder combines those labels with canonical metadata and the selected formula syntax;
optional brackets, argument indices and repeated-argument markers keep their structure.
Help strings are not formula aliases: this expansion does not invent function names for
languages without a formula-name table, nor change engine/API/XLSX syntax. Unknown future
catalog strings still fall back to English instead of blocking a new function.

New non-Russian rows are machine-assisted translation drafts of GridCraft's own English text
(Google Translate), with manual corrections for terminology, rounding direction/ties,
forecast-statistic codes and untranslated rows. A separate agent reviews each translated package
against the English catalog and argument contexts, including existing rows. Existing GridCraft
translations take priority where correct. The Traditional Chinese catalog comes from #3271.
The new rows
need native-speaker review, especially specialized financial/statistical terminology. Automated
coverage and syntax checks establish completeness and structure, not linguistic certification.
Serbian additions use the Latin script of the existing interface catalog.

Chart rendering supplies translated generated series names through a resolver callback, including
when a name formula or reference evaluates to an empty string. Explicit names remain unchanged;
the default resolver used by files and APIs still generates canonical English names. Blank-filter
search uses the displayed translation while commands retain their canonical blank-value payload.
Generated sheet prefixes follow the existing sheet-name rules, and command metadata documents
the localized book, sheet and table-column prefix parameters.

## Arabic bidi and cell reading direction (2026-10-27)

Cell text uses shared Unicode bidi layout while retaining logical source indexes for editing,
copy and XLSX. Format Cells exposes Context, Left to right and Right to left reading direction,
independent of sheet geometry. OOXML Context uses the first non-whitespace character. Optional
direction preserves absent versus explicit Context in differential styles. The renderer is
pinned to a reviewed egui 0.36.2 backport with real-font, editing, accessibility and WASM
checks. Physical screen-reader behavior remains unverified.

## Where we are (measured from source on `main`, after the 2026-10-10 merges)

- **A first catalog exists, for the ribbon only.** `crates/ui-egui/src/i18n.rs` (merged 10-10 in
  #31, #104, #82; German added 10-11) adds an interface-language preference that defaults to the
  system language (closes #20) and a View-tab switcher, with English-keyed lookup tables:
  Japanese 268 labels, Simplified Chinese, Korean, Russian, Brazilian Portuguese, German and Czech 266
  each. The tables are used by
  the ribbon, sheet tabs and widgets (`ribbon.rs`, `tabs.rs`, `widgets.rs`), the Format Cells
  dialog, every dialog title and the shared OK/Cancel buttons; the other dialogs, menus,
  context menus, task panes, status messages and command labels are still English literals.
  Untranslated labels fall back to English.
- **Function help** (the argument hint, autocomplete, Insert Function and the Formulas-tab
  menus) follows the interface language through per-language tables keyed by the catalog's
  English text (`i18n/<lang>_functions.rs`): descriptions, and argument names from which the
  hint is assembled with the formula syntax's function name and separator. The current 515-function
  catalog has help in 20 languages, including English and Traditional Chinese. Of the 24 interface
  languages, Danish, Swedish, Georgian and Lithuanian currently use English help.
- **A first catalog exists.** `crates/ui-egui/src/i18n.rs` (merged 10-10 in #31, #104, #82; German
  added 10-11) adds an interface-language preference that defaults to the system language (closes
  #20) and a View-tab switcher, with English-keyed lookup tables: Simplified Chinese, Korean,
  Russian, German and Czech 266–518 labels each, Brazilian Portuguese and Japanese 943 each. The
  tables are used by the ribbon, sheet tabs and widgets (`ribbon.rs`, `tabs.rs`, `widgets.rs`), the
  Format Cells dialog, every dialog title and the shared OK/Cancel buttons; Japanese and Portuguese
  also cover the other dialog labels, message boxes, Backstage/File-view items, template names and
  native menus (Japanese, 10-17: `i18n/ja.rs` keeps the shared 398-label core and adds 545 of those
  strings). The remaining menus, context menus, task panes, status messages, command labels and
  function descriptions are still English literals. Untranslated labels fall back to English.
- **About 2,200 user-visible strings** in all (heuristic source scan: ~1,700 UI and command
  strings plus ~500 function descriptions), so the ribbon-only languages cover **~12%** and
  Japanese and Portuguese **~43%** (measured count / estimated total).
- **Canonical file/API formulas, localized cell entry.** Files and APIs keep English names,
  comma arguments and decimal points. Cell and formula-bar entry support localized aliases and
  separators for Spanish, Russian, Polish, French, German, Italian, Brazilian Portuguese and Turkish.
  Spanish also localizes structured-reference specifiers. Russian date/time format editing and
  German format previews retain canonical storage codes.
- **Calendars in number formats.** The `B1` (Gregorian) and `B2` (Hijri) calendar prefixes that
  Arabic Excel writes at the start of a format section are hidden; under `B2` the year, month
  number, day and the Arabic month and weekday names (`mmm` and `ddd` show the full name) count
  in the tabular Hijri calendar (leap years 2, 5, 7, 10, 13, 15, 18, 21, 24, 26 and 29 of each
  30-year cycle), while month and weekday names of a Gregorian date follow the display
  locale's language. Checked against Excel 16.0 (build 20527, en-US) on every serial from 1 to
  2,958,465 and about 1,700 format/value pairs: a `[$...]` locale or currency tag before `B2`
  keeps the date Gregorian; a repeated prefix (`B2B2`) is accepted; any other `B` and digit
  (`B3`, `B12`, `B1B2`, `dd B2`, `\B2`), or a prefix before a number format (`B10.00`), is
  rejected (`TEXT` gives `#VALUE!`, a cell shows General); serial 60 shows `#VALUE!` under `B2`
  for any date or clock part; two-word month names use a no-break space. Differs from Excel:
  in a `B2` section with a weekday and a month name, Excel prints the weekday for
  `mmmm`/`mmmmm` after `ddd`/`dddd`, and the weekday numbered by the month (Saturday from
  month 7) for `ddd`/`dddd` after `mmm`/`mmmm`; a `B2` section showing only the weekday uses
  the real weekday for serials 0–59 and gives `#VALUE!` for the last days of 9999. Excel's
  Buddhist-year `b`/`bb`/`bbbb` tokens are not supported.
- **Script support.** System-font fallback for CJK, Thai and Hebrew landed in #38 (closes #32;
  #142 is still open pending confirmation). Native CJK fallback also probes Flatpak's read-only
  `/run/host/fonts`, `/run/host/local-fonts` and `/run/host/user-fonts` layouts, so installed
  Noto CJK and WenQuanYi faces can render Chinese without a broad filesystem grant. This does
  not change the installed-font catalogue or add bundled fonts. System CJK fallback faces and
  embedded faces tagged with CJK scripts share the Latin baseline in cells, editors and titles
  (#1265, #2621); other script, emoji and symbol fallbacks retain their alignment. Lao falls back
  to Lao Sangam MN / Lao MN (macOS), Leelawadee UI / DokChampa (Windows) or Noto Sans Lao /
  Phetsarath OT (Linux). Arabic falls back to a system Arabic face, fontconfig's Arabic choice or
  craft-fonts' Noto Sans Arabic; fontconfig's choice leads these fallbacks, but a UI font with
  Arabic of its own (DejaVu Sans in the Flatpak) still draws interface Arabic (#229). egui uses
  harfrust shaping and Unicode bidi layout, so Arabic joins and runs both ways; coverage of
  scripts other than Arabic still needs verification. **Sheet Right-to-Left**
  (`view.rightToLeft`, Page Layout › Sheet Options, XLSX `rightToLeft`, `.xls` WINDOW2 fRightToLeft) mirrors the grid (#146):
  column A and the row headers on the right, the vertical scrollbar on the left; hit-testing,
  arrow keys, editor, popups and floating objects follow; text follows its cell reading direction
  inside its box, and the sheet tabs, horizontal scrollbar and printing/PDF are not mirrored yet.
  Text boxes and shapes keep their paragraphs' right-to-left direction (`a:pPr rtl`) through XLSX
  save, though GridCraft still draws that text left to right. IME input goes
  through egui/winit and is unverified for Chinese, Japanese and Korean.
- **Names in any script.** Defined names (Define Name, Create from Selection) are limited to
  255 characters, counted as characters rather than UTF-8 bytes, so a 255-letter Arabic name is
  accepted as in Excel. Sheet names already counted characters (31).
- **PDF export** uses only the 14 standard PDF fonts: non-Latin text cannot export.

## Languages

UI strings translated are measured counts against the ~2,200 estimate. Estimates to `full`
include the rest of the catalog work below (10–20 h, counted once), localized function names,
separators, number/date formats, machine translation with agent review, and script work.
The counts were measured on 2026-10-10 and are kept as history; every interface language now has a
catalog (`crates/ui-egui/src/i18n/`), and a string a catalog lacks shows in English
(`cargo xtask i18n-report`).

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to full |
|---|---|---|---|---|---|---|---|
| English | en | ~2,200 / ~2,200 (100%, source) | yes | Latin: yes | yes | full | — |
| Simplified Chinese (Mandarin) | zh-Hans | 266 (~12%) | no | CJK system-font fallback (#38); IME unverified; PDF can't export | no | menus only | 8–13 h |
| Spanish | es | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Hindi | hi | 0 (0%) | no | Devanagari: **no shaping** | no | none | 12–18 h |
| Arabic | ar | 0 (0%) | no | shared shaping and bidi layout; no Arabic UI catalog; Sheet Right-to-Left mirrors the grid (#146) | no | none | 25–35 h |
| French | fr | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Portuguese | pt-BR | 943 (~43%) | dialogs and tooltips (dialogs take effect with #669) | Latin: yes | no | menus, File view, dialogs | 2–4 h |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 5–8 h |
| Japanese | ja | 943 (~43%) | dialogs and messages | CJK system-font fallback; the theme's East Asian typeface (`<a:ea>`) survives open + save (rendering does not use it yet); IME unverified; no furigana display, but phonetic runs (ふりがな) survive an XLSX open + save | no | menus, File view, dialogs, messages | 3–5 h |
| German | de | 266 (~12%) | no | Latin: yes | no | menus only | 4–8 h |
| Italian | it | 971 (~44%) | dialogs, tooltips, pivot pane | Latin: yes | no | menus, File view, dialogs, pivot pane | 3–5 h |
| Korean | ko | 266 (~12%) | no | Hangul via the CJK fallback; IME unverified | no | menus only | 6–10 h |
| Turkish | tr | 382 (~17%) | no | Latin: yes | yes | menus only | 4–8 h |
| Vietnamese | vi | 398 (~18%) | no | Latin with stacked diacritics: unverified | no | menus, File view | 5–9 h |

Russian is described in the current update above. Other shipped languages in this historical table
include German (de), Czech (cs), French (fr) and Turkish (tr); the German (de) and Dutch (nl) counts
below predate the current Russian catalog.

**Total:** catalog work 10–20 h + languages 92–144 h ≈ **100–165 h**. Parallelizes almost fully
once the catalog covers dialogs and commands. **Needs a human:** native-speaker review of every
language (none so far) and of localized function names.

## Infrastructure (do first)

1. Extend the catalog from the ribbon to every user-visible string (dialogs, menus, task panes,
   status and error messages, command labels, function descriptions). A string a catalog lacks
   shows in English and never fails a check: `cargo xtask i18n-report` lists the missing strings
   per language, and `cargo xtask ci` runs it first. Language switching and the system-language default already exist (#31).
2. Extend locale-aware formulas through the shared `LocaleData` and per-language modules at the
   formula-bar boundary (files keep invariant names); regional number/date formats and CSV
   separator policies remain separate work.
3. Text shaping and fallback: a shaping engine for grid, editor and PDF text (Arabic, Devanagari,
   Thai), system font fallback for CJK, bidirectional runs, right-to-left sheet tabs and printing;
   embedded subset fonts in PDF.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-10 | minor | Merge current main's CJK fallback baseline alignment updates (#1265, #2621) while retaining localized formula and catalog changes |
| 2026-11-06 | minor | Correct Greek argument and Options labels, Ukrainian contextual/shared labels, and the Danish linked-cell caption; retain the established Danish Paste Link term and context-appropriate permission infinitives |
| 2026-11-06 | minor | Preserve regional output alongside main display-precision helpers; align Czech logical tokens with its formula syntax, correct Chinese help labels and Traditional Chinese terms, and repair the Languages table |
| 2026-11-05 | minor | Complete the Czech 2,427-key interface inventory and review all 762 Czech help rows; correct mathematical and financial terms and synchronize the clarified F-test help key across all 19 translated tables |
| 2026-11-05 | minor | Search translated command paths with contextual ribbon-tab names; localize Boolean/error format samples and numeric filter captions without changing canonical filter values; correct French agreement and replace 100 English fallback entries in the Ukrainian interface |
| 2026-11-05 | minor | Localize the initial desktop workbook after preference loading, enforce Unicode-aware sheet uniqueness, print totals formulas through the shared structured-reference printer and correct Traditional Chinese rate-series help |
| 2026-11-05 | minor | Localize selection statistics, correct Thai date IDs and Traditional Chinese security-price help; keep generated-name defaults local to UI invocation, share prefix validation and document/test name-scope identities without changing legacy deletion semantics |
| 2026-11-05 | minor | Align grid and format previews with the UI number locale while keeping engine output canonical; accept Unicode case variants of General, match observed German Excel March abbreviation, and correct Chinese price/direction and Spanish/Italian gallery labels |
| 2026-11-05 | minor | Fix Polish/Traditional Chinese template lookups, Greek coupon-rate and Chinese analysis/preview terms, native Insert menu context and macOS language declarations; correct Thai built-in time format IDs and localized fractional seconds |
| 2026-11-03 | minor | Keep unchanged text typed on Ctrl+Shift+Enter, distinguish sheet and workbook name scopes, preserve parser tokens, localize typed filter labels and chart data choices, retain 12-hour suggestions, make style previews scrollable and correct help terminology |
| 2026-11-02 | minor | Review translated help by language; correct mathematical/statistical/financial terminology, localize empty resolved chart names and blank-filter search, validate generated sheet prefixes and assert complete hint signatures |
| 2026-11-02 | minor | Complete help for the current 515 functions in all 19 translated interface languages; retain existing wording, separate help from formula syntax and test optional/repeated-argument structure |
| 2026-11-02 | minor | Extend Russian help to 479 distinct descriptions and 283 argument keys; preserve original translations and canonical formula syntax; cover reported XLOOKUP, COLUMN and RADIANS tooltips |
| 2026-11-02 | minor | Ctrl+Enter fills unchanged stored text as a typed literal without adding an apostrophe; fresh localized numeric entry retains normal parsing |
| 2026-10-29 | minor | Russian: refresh #676 catalog, adapt #1850 formula aliases to current help/name handling, localize error literals and selected parameter/parser diagnostics at the UI boundary |
| 2026-11-09 | minor | One row per language and current script status in the Languages table, the 19 translated help tables named consistently, and generated-name prefixes tested against every catalog |
| 2026-11-10 | minor | Ukrainian Chart is `Діаграма`, matching [craft-i18n#211](https://github.com/storytold/craft-i18n/pull/211) and distinct from Diagram (`Схема`) |
| 2026-11-10 | minor | Diagram and Decorative Text labels in all 18 non-English catalogs; Diagram never repeats the catalog's chart word (German `Schema`, Ukrainian `Схема`, Romanian `Schemă`), see [feature-names.md](feature-names.md) |
| 2026-10-31 | minor | System and embedded CJK text fallbacks share the Latin baseline in cells, editors and window titles; other scripts, symbols and emoji keep their alignment |
| 2026-10-26 | minor | Shared bidi layout supports Arabic clusters, marks, visual cursor navigation and directional accessibility; cell Context/LTR/RTL is stored separately from sheet RTL and round-trips through XLSX |
| 2026-10-26 | minor | Arabic text renders in cells and the interface: a system Arabic face (Tahoma, Arial or Segoe UI on Windows, Geeza Pro on macOS, Noto Sans or Naskh Arabic on Linux) joins the script fallbacks, fontconfig's Arabic choice, when it has Arabic letters, leads the Arabic fallbacks and draws Arabic wherever the UI or cell font has none, such as the default cell fonts (part of #229; a UI font with Arabic of its own, such as DejaVu Sans in the Flatpak, still draws interface Arabic, left for a follow-up), and craft-fonts' Noto Sans Arabic is a required face allowed on the web. Workbook fonts without Arabic keep every script fallback. Shaping and bidirectional layout remain separate work |
| 2026-10-27 | minor | Number formats honour the `B1`/`B2` calendar prefixes: never shown; `B2` renders year, month, day and Arabic month and weekday names in the tabular Hijri calendar (1 Jan 2001 = 6 Shawwal 1421; leap years 2, 5, 7, 10, 13, 15, 18, 21, 24, 26 and 29 of the 30-year cycle, which is what Excel shows). Checked against Excel 16.0 (build 20527, en-US) on every serial and about 1,700 format/value pairs: a locale or currency tag before `B2` keeps the date Gregorian, a misplaced `B` and digit or a prefix before a number format is rejected, serial 60 has no Hijri date, serial 0 shows as serial 1, two-word month names use a no-break space. Not matched: Excel's weekday/month-name mix-ups in one `B2` section and its weekday-only quirks |
| 2026-10-27 | minor | AutoFit column width, wrapped-row height and PivotTable column fit estimate Arabic letters at ~0.48 em (measured 0.36–0.53 em shaped, Noto Sans/Naskh Arabic and DejaVu Sans) instead of 1 em, Arabic-Indic digits as wide as ASCII digits, and give vowel marks and direction marks no width; vowelled Arabic no longer fits ~3.5× too wide |
| 2026-10-26 | minor | Shared bidi layout supports Arabic clusters, marks, visual cursor navigation and directional accessibility; cell Context/LTR/RTL is stored separately from sheet RTL and round-trips through XLSX |
| 2026-10-26 | minor | Arabic text renders in cells and the interface: a system Arabic face (Tahoma, Arial or Segoe UI on Windows, Geeza Pro on macOS, Noto Sans or Naskh Arabic on Linux) joins the script fallbacks, fontconfig's Arabic choice, when it has Arabic letters, leads the Arabic fallbacks and draws Arabic wherever the UI or cell font has none, such as the default cell fonts (part of #229; a UI font with Arabic of its own, such as DejaVu Sans in the Flatpak, still draws interface Arabic, left for a follow-up), and craft-fonts' Noto Sans Arabic is a required face allowed on the web. Workbook fonts without Arabic keep every script fallback. Shaping and bidirectional layout remain separate work |
| 2026-10-23 | minor | Missing translations are reported, never a failure: `cargo xtask i18n-report` prints, per language, the strings other catalogs translate and it lacks (plus Spanish function help rows keyed by English the function catalog no longer has) and always passes; `cargo xtask ci` runs it first. Coverage and per-language parity tests and tests of one language's exact wording are gone; UI tests take the expected text from the catalog. Duplicate-key, placeholder, context-prefix and line-shape checks stay |
| 2026-10-21 | minor | Romanian interface (`ro`, Română): 970 labels covering every key of the other catalogs (`crates/ui-egui/src/i18n/ro.rs`), `ro-*` system locales, the View-tab switcher and `app.language.set`; context keys show no prefix (File › Options test). English formulas and formats; AI-assisted draft pending native-speaker review |
| 2026-10-20 | minor | Japanese: furigana (`rPh` phonetic runs and `<phoneticPr>` in `xl/sharedStrings.xml`) is no longer deleted by an open + save; it is kept on the workbook and written back (a `<phoneticPr>` without runs is kept too, while duplicate texts with different readings keep only the first, reported on open). Display of the readings is still missing (`crates/xlsx/src/tests/phonetic_tests.rs`) |
| 2026-10-20 | minor | Japanese catalog (`i18n/ja.rs`) covers every key the Brazilian Portuguese catalog translates: the 398-label shared core moved verbatim plus 545 dialog, message, Backstage and menu strings — 943 keys, ~12% → ~43%. A test parses `pt.rs`'s source, so the two catalogs can't drift apart again. Portuguese re-measured at 943 (was 923) |
| 2026-10-20 | minor | Dutch (nl) interface catalog (`i18n/nl.rs`, 518 labels, ~24%): the shared key set every language must cover (ribbon, context menus, File view, dialog titles, Options page), matching the German, French and Czech catalogs; `nl-NL`, `nl-BE` locale tags resolve to Dutch |
| 2026-10-20 | minor | XLSX: a theme's per-script typefaces (`<a:font script="Arab">`, Hebr, Jpan…) are kept through open + save, so Arabic Excel's theme fonts for shapes, charts and theme-font cells survive a GridCraft save |
| 2026-10-20 | trivial | XLSX: a workbook theme's major/minor East Asian (`<a:ea>`) and complex-script (`<a:cs>`) typefaces are kept through open + save, so a Japanese workbook keeps its theme's East Asian font; rendering does not resolve them yet (follow-up) |
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
