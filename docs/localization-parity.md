# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-15 · **Change:** minor (numbers and dates are written in the interface language's conventions) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~10% ready, 100–165 h** (estimated).

## Where we are (measured from source on `main`, after the 2026-10-10 merges)

- **A first catalog exists, for the ribbon only.** `crates/ui-egui/src/i18n.rs` (merged 10-10 in
  #31, #104, #82; German added 10-11) adds an interface-language preference that defaults to the
  system language (closes #20) and a View-tab switcher, with English-keyed lookup tables:
  Japanese 268 labels, Simplified Chinese, Korean, Russian, Brazilian Portuguese, German and Czech 266
  each. The tables are used by
  the ribbon, sheet tabs and widgets (`ribbon.rs`, `tabs.rs`, `widgets.rs`), the Format Cells
  dialog, every dialog title and the shared OK/Cancel buttons; the other dialogs, menus,
  context menus, task panes, status messages, command labels and function descriptions are still
  English literals. Untranslated labels fall back to English.
- **About 2,200 user-visible strings** in all (heuristic source scan: ~1,700 UI and command
  strings plus ~500 function descriptions), so each shipped language covers **~12%** (measured
  count / estimated total).
- **Localized number and date output, German.** The interface language picks the conventions
  numbers and dates are *written* with (`crates/core/src/locale.rs`, `Workbook::locale`,
  `app.locale.set`): German separators (`1.234,56`) and German month and weekday names
  (`15. Oktober 2026`, `Donnerstag`) instead of `1,234.56` and `October 15, 2026`, in the grid,
  the clipboard and CSV, printing/PDF, chart axis and data
  labels and `TEXT()`. German is the only other locale; every other interface language keeps
  en-US. Format codes (`#,##0.00`, `m/d/yyyy`), function names, the `,` argument separator and
  typed input are still en-US: reading German formats and swapping `;` for `,` is the rest of
  PR #163, and localized function names (#45) are still missing. Numbers read from a clipboard
  HTML table are still recognised in en-US only.
- **en-US formulas only.** Function names, the `,` argument separator and locale *entry* of
  numbers and dates are en-US. Users ask for localized function names and `;` separators (#45).
  German with localized numbers, formats and formulas is in PR #163 (open); the output half of
  that work is what landed here.
- **Script support.** System-font fallback for CJK, Thai and Hebrew landed in #38 (closes #32;
  #142 is still open pending confirmation). There is no complex-text shaping: Arabic and
  Devanagari don't join or reorder, and there is no bidirectional layout. **Sheet Right-to-Left**
  (`view.rightToLeft`, Page Layout › Sheet Options, XLSX `rightToLeft`) mirrors the grid (#146):
  column A and the row headers on the right, the vertical scrollbar on the left; hit-testing,
  arrow keys, editor, popups and floating objects follow; text stays left-to-right inside its box
  (no bidi), and the sheet tabs, horizontal scrollbar and printing/PDF are not mirrored yet. IME input goes
  through egui/winit and is unverified for Chinese, Japanese and Korean.
- **PDF export** uses only the 14 standard PDF fonts: non-Latin text cannot export.

## Languages

UI strings translated are measured counts against the ~2,200 estimate. Estimates to `full`
include the rest of the catalog work below (10–20 h, counted once), localized function names,
separators, number/date formats, machine translation with agent review, and script work.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to full |
|---|---|---|---|---|---|---|---|
| English | en | ~2,200 / ~2,200 (100%, source) | yes | Latin: yes | yes | full | — |
| Simplified Chinese (Mandarin) | zh-Hans | 266 (~12%) | no | CJK system-font fallback (#38); IME unverified; PDF can't export | no | menus only | 8–13 h |
| Spanish | es | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Hindi | hi | 0 (0%) | no | Devanagari: **no shaping** | no | none | 12–18 h |
| Arabic | ar | 0 (0%) | no | **no shaping, no RTL UI**; Sheet Right-to-Left mirrors the grid (#146) | no | none | 25–35 h |
| French | fr | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Portuguese | pt-BR | 923 (~42%) | dialogs and tooltips (dialogs take effect with #669) | Latin: yes | no | menus, File view, dialogs | 2–4 h |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 5–8 h |
| Japanese | ja | 268 (~12%) | no | CJK system-font fallback; IME unverified; no furigana display | no | menus only | 8–12 h |
| German | de | 266 (~12%) | no | Latin: yes | no | menus only | 4–8 h |
| Korean | ko | 266 (~12%) | no | Hangul via the CJK fallback; IME unverified | no | menus only | 6–10 h |
| Vietnamese | vi | 0 (0%) | no | Latin with stacked diacritics: unverified | no | none | 6–10 h |

Other shipped languages: Russian (ru) and German (de), 266 labels each (~12%), menus only.

**Total:** catalog work 10–20 h + languages 92–144 h ≈ **100–165 h**. Parallelizes almost fully
once the catalog covers dialogs and commands. **Needs a human:** native-speaker review of every
language (none so far) and of localized function names.

## Infrastructure (do first)

1. Extend the catalog from the ribbon to every user-visible string (dialogs, menus, task panes,
   status and error messages, command labels, function descriptions), with a missing-key check
   in `cargo xtask ci`. Language switching and the system-language default already exist (#31).
2. Locale-aware formulas: localized function-name tables and separators at the formula-bar
   boundary only (files keep en-US invariant names), list separators in CSV. Numbers and dates
   are already *written* in the locale's conventions (German; `crates/core/src/locale.rs`), but
   their localized *entry* — reading `1.234,56`, German format codes and `;` separators — is not.
3. Text shaping and fallback: a shaping engine for grid, editor and PDF text (Arabic, Devanagari,
   Thai), system font fallback for CJK, bidirectional runs, right-to-left sheet tabs and printing;
   embedded subset fonts in PDF.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Numbers and dates are written in the interface language's conventions: a `Locale` in `crates/core` (`en`/`de`) reaches `crates/numfmt` (`format_value`, `text_function`, General), `Workbook::locale` and the new `app.locale.set`; picking German in View ▸ Interface language (or a saved `ui.json`) switches the grid, clipboard, CSV, printing, chart labels and `TEXT()` to `1.234,56` / `15. Oktober 2026`. Format codes, function names, the `,` separator and typed input stay en-US (PR #163) |
| 2026-10-12 | minor | Format Cells looks up its tabs, labels, options and note through `Language::tr` (Fill tab keyed `Format Cells\|Fill`); dialog titles and OK/Cancel too. Labels already in a table (Number, Currency, Fill…) show translated; the rest fall back to English until the tables add them |
| 2026-10-12 | minor | Brazilian Portuguese catalog (`i18n/pt.rs`) grows from 377 to 923 entries: every key the other catalogs translate, including the dialog strings #669 starts looking up (~42%) |
| 2026-10-11 | minor | German ribbon catalog (`i18n/de.rs`, 266 labels, ~12%); `de` locale tags (de-DE, de-AT, de-CH…) resolve to German. PR #163 (localized German formulas/numbers) still open |
| 2026-10-11 | minor | Sheet Right-to-Left mirrors the grid (#146) |
| 2026-10-11 | minor | Czech (cs) ribbon table: 266 labels (~12%), menus only |
| 2026-10-10 | minor | Re-measured after #31, #104, #82, #38 merged: ribbon translated into ja, zh, ko, pt-BR and ru (266–268 labels, ~12%), system-language default, CJK/Thai/Hebrew font fallback; ~10% ready, 100–165 h |
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
