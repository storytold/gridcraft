# Simplified Chinese (zh) interface localization

The UI now offers **简体中文** fully via View > Interface language, and uses Chinese
automatically on a fresh installation when the system locale is `zh` (`zh-CN`, `zh-SG`, …).

The localization follows the existing GridCraft interface translation infrastructure:

- `crates/ui-egui/src/i18n/zh.rs` supplies Simplified Chinese translations of the UI strings:
  the ribbon and every ribbon path, the sheet tabs, the status bar, the formula bar, the
  Backstage/File view, every dialog (including Goal Seek, Solver, the analysis toolpak dialogs,
  Format Cells, Insert Function, the macro editor and step editor), the context menus, the task
  panes, the message boxes and the command labels.
- `crates/ui-egui/src/i18n/zh_errors.rs` adds sentence templates for engine messages that carry
  data (`'{name}' is protected.`, `The timeline must hold numbers or dates ({} isn't one).`).
  `Language::tr_error` (also added here) matches the fixed parts of a finished sentence against
  the template and carries the data across, so a message whose middle is a file name still reads
  naturally in Chinese.
- `crates/ui-egui/src/i18n/zh_functions.rs` adds function help in Chinese: a `DESCRIPTIONS` table
  (keyed by the English catalog description text, so functions that share a description share an
  entry) and an `ARGUMENTS` table of translated argument names, so the argument hint, the
  autocomplete list, Insert Function and the Formulas-tab menus all read in Chinese. Function
  *names* stay English — formulas keep `=SUM(A1:A9)` — matching the project's convention that
  command and function identifiers are stable.
- `Language::tr_fill` fills `{slot}` templates for composed labels; dialog notes that combine a
  sentence with live data (the Allow Edit Ranges summary, the Move or Copy sheet list, the
  hidden-sheet list, the protected-file name, the hyperlink warning) build their text from
  translated parts instead of a pre-formatted English string.
- Chinese family names: `fonts.rs` reads a face's `zh` name-table records, so with a Chinese
  interface the font list offers `思源黑体` next to `Source Han Sans`; the font-file scan cap
  rises to admit the large CJK `.ttc` collections, and `theme/font_setup.rs` prefers CJK-capable
  system faces for Chinese text.
- Tests: `every_command_label_reads_in_chinese` walks the whole command catalog and fails on any
  English label; the template machinery has its own tests, and `GRIDCRAFT_I18N_TRACE=1` records
  every lookup that falls back to English (report, never failure — see localization-parity).

Not part of this change (kept for future work): localized function *names* in formulas, the
localized argument separator, locale number and date formats, a Chinese spelling dictionary, and
the update dialogs (they follow the interface language through a separate `craft-update` change).

Command identifiers, spreadsheet content, function names and references remain stable. The
Chinese strings were translated specifically for GridCraft. They are not extracted from
proprietary spreadsheet software or localization assets. The MIT OR Apache-2.0 repository
licenses apply to this contribution.
