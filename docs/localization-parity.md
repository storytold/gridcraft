# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created; measured from source and open PRs) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~5% ready, 110–175 h** (estimated).

## Where we are (measured from source on `main`)

- **No string catalog.** Every user-visible string is an English literal in
  `crates/ui-egui/src` and in the command specs (`crates/engine/src/cmd`). A source scan finds
  **~1,700 UI and command strings** plus **~500 function descriptions** (one per function spec):
  about **2,200 strings** to translate (estimated by heuristic scan; argument names extra).
- **en-US only formulas and formats.** Function names, `,` argument separator, `.` decimal
  separator, en-US number and date formats (`crates/numfmt` says "en-US only"). Users ask for
  localized function names and `;` separators (#45) and for the UI to follow the system
  language (#20).
- **Script support in cells** depends on system fonts through egui, with no complex-text
  shaping: Chinese text doesn't render on some systems (#142; CJK font fallback PRs #103, #147
  open), Thai renders incorrectly on Windows (#32), Arabic and Devanagari are not shaped, there is
  no bidirectional layout. **Sheet Right-to-Left** is a stored flag (`view.rightToLeft`) but the
  grid doesn't mirror (#146). IME input goes through egui/winit and is unverified for
  Chinese, Japanese and Korean.
- **PDF export** uses only the 14 standard PDF fonts: non-Latin text cannot export.
- **Open community PRs:** German UI, numbers and formulas (#163); Simplified Chinese, Korean and
  Russian UI (#104). Portuguese (Brazil) requested (#79).

## Languages

Strings translated is measured (0 outside English, no catalog). Estimates to `full` assume the
shared infrastructure below is done first (15–25 h, counted once) and include localized function
names, separators, number/date formats, machine translation with agent review, and script work.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to full |
|---|---|---|---|---|---|---|---|
| English | en | ~2,200 / ~2,200 (100%, source) | yes | Latin: yes | yes | full | — |
| Simplified Chinese (Mandarin) | zh-Hans | 0 (0%; PR #104 open) | no | CJK rendering depends on system fonts (#142); IME unverified | no | none | 10–16 h |
| Spanish | es | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Hindi | hi | 0 (0%) | no | Devanagari: **no shaping** | no | none | 12–18 h |
| Arabic | ar | 0 (0%) | no | **no shaping, no RTL UI, RTL sheet flag not rendered** (#146) | no | none | 25–35 h |
| French | fr | 0 (0%) | no | Latin: yes | no | none | 6–10 h |
| Portuguese | pt | 0 (0%; pt-BR requested #79) | no | Latin: yes | no | none | 6–10 h |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 5–8 h |
| Japanese | ja | 0 (0%) | no | CJK rendering depends on system fonts; IME unverified; no phonetic (furigana) display | no | none | 10–14 h |
| German | de | 0 (0%; PR #163 open) | no | Latin: yes | no | none | 6–10 h |
| Korean | ko | 0 (0%; PR #104 open) | no | Hangul rendering depends on system fonts; IME unverified | no | none | 8–12 h |
| Vietnamese | vi | 0 (0%) | no | Latin with stacked diacritics: unverified | no | none | 6–10 h |

Other shipped languages: none. (Russian is in PR #104.)

**Total:** infrastructure 15–25 h + languages 100–153 h ≈ **110–175 h**. Parallelizes almost
fully after the infrastructure lands. **Needs a human:** native-speaker review of every language
(none so far) and of localized function names.

## Infrastructure (do first)

1. String catalog (for example Fluent or gettext-style, Rust only) with English as the source,
   a missing-key check in `cargo xtask ci`, and runtime language switching that follows the
   system language (#20).
2. Locale-aware formulas: localized function-name tables and separators at the formula-bar
   boundary only (files keep en-US invariant names), locale number and date formats, list
   separators in CSV.
3. Text shaping and fallback: a shaping engine for grid, editor and PDF text (Arabic, Devanagari,
   Thai), system font fallback for CJK, bidirectional runs, and a real right-to-left sheet layout;
   embedded subset fonts in PDF.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
