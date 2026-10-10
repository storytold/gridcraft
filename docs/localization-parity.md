# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (re-measured after the 10-10 language and font-fallback merges) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~10% ready, 100–165 h** (estimated).

## Where we are (measured from source on `main`, after the 2026-10-10 merges)

- **A first catalog exists, for the ribbon only.** `crates/ui-egui/src/i18n.rs` (merged 10-10 in
  #31, #104, #82) adds an interface-language preference that defaults to the system language
  (closes #20) and a View-tab switcher, with English-keyed lookup tables: Japanese 268 labels,
  Simplified Chinese, Korean, Russian and Brazilian Portuguese 266 each. The tables are used by
  the ribbon, sheet tabs and widgets (`ribbon.rs`, `tabs.rs`, `widgets.rs`); dialogs, menus,
  context menus, task panes, status messages, command labels and function descriptions are still
  English literals. Untranslated labels fall back to English.
- **About 2,200 user-visible strings** in all (heuristic source scan: ~1,700 UI and command
  strings plus ~500 function descriptions), so each shipped language covers **~12%** (measured
  count / estimated total).
- **en-US only formulas and formats.** Function names, `,` argument separator, `.` decimal
  separator, en-US number and date formats (`crates/numfmt` says "en-US only"). Users ask for
  localized function names and `;` separators (#45). German with localized numbers and formulas
  is in PR #163 (open).
- **Script support.** System-font fallback for CJK, Thai and Hebrew landed in #38 (closes #32;
  #142 is still open pending confirmation). There is no complex-text shaping: Arabic and
  Devanagari don't join or reorder, and there is no bidirectional layout. **Sheet Right-to-Left**
  is a stored flag (`view.rightToLeft`) but the grid doesn't mirror (#146). IME input goes
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
| Arabic | ar | 0 (0%) | no | **no shaping, no RTL UI, RTL sheet flag not rendered** (#146) | no | none | 25–35 h |
| French | fr | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Portuguese | pt-BR | 266 (~12%) | no | Latin: yes | no | menus only | 4–8 h |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 5–8 h |
| Japanese | ja | 268 (~12%) | no | CJK system-font fallback; IME unverified; no furigana display | no | menus only | 8–12 h |
| German | de | 0 (0%; PR #163 open) | no | Latin: yes | no | none | 6–10 h |
| Korean | ko | 266 (~12%) | no | Hangul via the CJK fallback; IME unverified | no | menus only | 6–10 h |
| Vietnamese | vi | 0 (0%) | no | Latin with stacked diacritics: unverified | no | none | 6–10 h |

Other shipped languages: Russian (ru), 266 labels (~12%), menus only.

**Total:** catalog work 10–20 h + languages 92–144 h ≈ **100–165 h**. Parallelizes almost fully
once the catalog covers dialogs and commands. **Needs a human:** native-speaker review of every
language (none so far) and of localized function names.

## Infrastructure (do first)

1. Extend the catalog from the ribbon to every user-visible string (dialogs, menus, task panes,
   status and error messages, command labels, function descriptions), with a missing-key check
   in `cargo xtask ci`. Language switching and the system-language default already exist (#31).
2. Locale-aware formulas: localized function-name tables and separators at the formula-bar
   boundary only (files keep en-US invariant names), locale number and date formats, list
   separators in CSV.
3. Text shaping and fallback: a shaping engine for grid, editor and PDF text (Arabic, Devanagari,
   Thai), system font fallback for CJK, bidirectional runs, and a real right-to-left sheet layout;
   embedded subset fonts in PDF.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Re-measured after #31, #104, #82, #38 merged: ribbon translated into ja, zh, ko, pt-BR and ru (266–268 labels, ~12%), system-language default, CJK/Thai/Hebrew font fallback; ~10% ready, 100–165 h |
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
