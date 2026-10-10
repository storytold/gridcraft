# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (re-measured for M12: Fluent catalog, local formulas and regional formats in 12 languages, #191) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~50% ready, 45–65 h** (estimated).

## Where we are (measured from source with `cargo xtask locales`, M12 / #191)

- **The whole interface is in one catalog.** Fluent messages in `locales/<tag>/` (`commands`,
  `errors`, `functions`, `menu`, `ribbon`, `ui`: 1,955 keys per language) are read through the
  `gridcraft-l10n` crate by the ribbon, menus, context menus, dialogs, task panes, status and
  error messages, command labels and function descriptions. A scan of `crates/ui-egui/src` for
  raw label, button and heading literals finds only the product name. All 12 languages carry every key;
  `cargo xtask locales` (a `cargo xtask ci` step) fails on a missing or extra key. The upstream
  ribbon tables (#31, #104, #82) were folded into the catalog; the View-tab language switcher and
  the system-language default (#20) stay, and Options › Language shares the same preference.
- **Formulas follow Excel.** Function names follow the interface language, separators follow the
  regional format, and files and the journal stay canonical (English names, `,` and `.`), so a
  workbook opens the same in every language: `=SOMA(1,5;2)` (pt-BR), `=SUMME(1,5;2)` (de-DE),
  `=СУММ({1;2:3;4})` (ru-RU), `{1\2;3\4}` (pt-BR array constants). Error values, booleans,
  `CELL`/`INFO` keywords and structured-reference items are localized too. Japanese, Korean and
  Chinese keep English function names, as Excel does. Every catalog entry records its sources in
  `crates/locale/data/languages/*.toml`.
- **Names win over translations.** Defined names visible from the sheet, table names and
  LET/LAMBDA bindings in scope take precedence over a localized function name or boolean, in the
  editor, the formula bar, Show Formulas, `*Local` command parameters, `FORMULATEXT` and
  `INDIRECT` (#98): with a name `FALSO`, Spanish `=FALSO+1` reads the name. A built-in whose local
  spelling a name has taken is written with its file prefix (`=_xlfn.SUM(1;_xlfn.FALSE())`) so it
  reads back as the built-in. An edit in progress is rewritten when the language changes.
- **Regional formats.** Twelve regions (CLDR conventions): decimal and group separators, list
  separator (formulas and CSV), date order and separators, month and day names, AM/PM, currency.
  Typed numbers and dates parse in the region, General display uses its separators, and the
  `TEXT` format-code letters and the General keyword are localized (`ДД.ММ.ГГГГ`,
  `TT.MM.JJJJ`, `"Основной"`). Interface language and regional format are chosen separately
  (Options › Language, `app.setLocale`, `--locale` for the CLI and MCP) and default to the
  system or browser; they persist in `prefs.json` (localStorage on the web).
- **Keyboard shortcuts** follow Excel where it localizes them (de-DE 7, pt-BR 6, pt-PT 5, it-IT 4,
  es-ES 2, fr-FR 2 overrides; pt-BR Ctrl+N is bold). Ribbon key tips keep their English letters in
  every language; only captions translate.
- **Script support.** System-font fallback for CJK, Thai and Hebrew (#38). IME preedit and commit
  are handled in the grid and the formula bar, but not verified with a real Chinese, Japanese or
  Korean input method. There is no complex-text shaping: Arabic and Devanagari don't join or
  reorder, and there is no bidirectional layout. **Sheet Right-to-Left** is a stored flag
  (`view.rightToLeft`) but the grid doesn't mirror (#146).
- **PDF export** uses only the 14 standard PDF fonts: non-Latin text cannot export.
- **Translations are agent-made.** No language has had native-speaker review.

| Language data | Function names translated | Kept English | Unsourced (English until found) | Extra accepted spellings |
|---|---|---|---|---|
| de-DE | 403 / 501 | 97 | 1 | 0 |
| es-ES | 424 / 501 | 75 | 2 | 0 |
| fr-FR | 398 / 501 | 103 | 0 | 0 |
| it-IT | 452 / 501 | 48 | 1 | 0 |
| pt-BR | 412 / 501 | 87 | 2 | 1 |
| pt-PT | 418 / 501 | 82 | 1 | 2 |
| ru-RU | 459 / 501 | 42 | 0 | 3 |
| ja-JP, ko-KR, zh-CN, zh-TW | English, as in Excel | 501 | 0 | 0 |

"Kept English" counts names Excel spells the same in that language (`LAMBDA`, `PI` and `INFO` in
German); the unsourced ones are counted separately and also stay English until a source is found.

## Languages

The twelve target languages, by number of speakers. UI strings are measured key counts.
Estimates to `full` cover the remaining agent work; native review is a human task in every row.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to full |
|---|---|---|---|---|---|---|---|
| English | en | 1,955 / 1,955 (100%, source) | yes | Latin: yes | yes | full | — |
| Simplified Chinese (Mandarin) | zh-Hans (zh-CN) | 1,955 / 1,955 (100%) | yes | CJK system-font fallback (#38); IME unverified; PDF can't export | no | interface and formats; English function names, as Excel | 3–5 h |
| Spanish | es (es-ES) | 1,955 / 1,955 (100%) | yes | Latin: yes | no | interface, function names, formats | review only |
| Hindi | hi | 0 (0%) | no | Devanagari: **no shaping** | no | none | 9–13 h |
| Arabic | ar | 0 (0%) | no | **no shaping, no RTL UI, RTL sheet flag not rendered** (#146) | no | none | 20–28 h |
| French | fr (fr-FR) | 1,955 / 1,955 (100%) | yes | Latin: yes | no | interface, function names, formats | review only |
| Portuguese | pt-BR | 1,955 / 1,955 (100%) | yes | Latin: yes | no | interface, function names, formats | review only |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 3–5 h |
| Japanese | ja (ja-JP) | 1,955 / 1,955 (100%) | yes | CJK system-font fallback; IME unverified; no furigana display; PDF can't export | no | interface and formats; English function names, as Excel | 3–5 h |
| German | de (de-DE) | 1,955 / 1,955 (100%) | yes | Latin: yes | no | interface, function names, formats | review only |
| Korean | ko (ko-KR) | 1,955 / 1,955 (100%) | yes | Hangul via the CJK fallback; IME unverified; PDF can't export | no | interface and formats; English function names, as Excel | 2–4 h |
| Vietnamese | vi | 0 (0%) | no | Latin with stacked diacritics: unverified | no | none | 3–6 h |

Other shipped languages, at the same depth as the Latin rows (Traditional Chinese as the CJK
rows): Traditional Chinese (zh-TW), European Portuguese (pt-PT), Italian (it-IT), Russian (ru-RU).
"Help" means function descriptions and dialog text; GridCraft has no help pages in any language.

**Total:** ≈ **45–65 h**, most of it Hindi and Arabic shaping, right-to-left layout and
non-Latin PDF fonts (gap #5). A new Latin-script language is data only (no Rust change): a
language file and a region file in `crates/locale/data/` and `locales/<tag>/` (six Fluent files
and `keymap.toml`). **Needs a human:** native-speaker
review of every language (none so far) and of the localized function-name catalogs.

**Readiness basis:** eight of the twelve target languages work end to end (interface, formulas,
regional formats). Five of them are Latin-script and complete apart from review; the three CJK
ones lack a verified IME and PDF export. Four have nothing, and two of those need shaping and
right-to-left layout. No language is reviewed. Measured against the 100–165 h this dimension
needed with English only, about 60% of the work is done; with no native review, **~50%**.

## What's left

1. Text shaping and layout (gap #5): a shaping engine for grid, editor and PDF text (Arabic,
   Devanagari, Thai), bidirectional runs and a real right-to-left sheet layout; embedded subset
   fonts in PDF so CJK and other scripts export.
2. Hindi, Arabic, Indonesian and Vietnamese: catalogs, function names (Excel keeps English names
   in some of them; to be sourced) and regions.
3. IME verification with real Chinese, Japanese and Korean input methods.
4. Native-speaker review of every shipped language.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Re-measured for M12 (#191): 1,955-key Fluent catalog in 12 languages (the ribbon tables of #31, #104, #82 folded in), localized function names and separators, 12 regional formats, localized shortcuts; ~50% ready, 45–65 h |
| 2026-10-10 | minor | Re-measured after #31, #104, #82, #38 merged: ribbon translated into ja, zh, ko, pt-BR and ru (266–268 labels, ~12%), system-language default, CJK/Thai/Hebrew font fallback; ~10% ready, 100–165 h |
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
