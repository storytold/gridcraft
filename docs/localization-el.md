# Greek (el) interface localization

The UI now offers **Ελληνικά** via View > Interface language and uses Greek automatically
on a fresh installation when the system locale is `el` or `el-GR`.

The localization follows the existing GridCraft interface translation infrastructure:
- `crates/ui-egui/src/i18n/el.rs` supplies Greek translations of UI strings.
- `crates/ui-egui/src/i18n.rs` registers `Language::El` (`el`), language selection,
  persisted preferences, and translation tests.
- Command identifiers, spreadsheet content, function names and references remain stable.
- Greek UI translation **does not add a Greek spelling dictionary** or translate formula
  function names, locale-specific separators or number formats.
- Unknown UI labels continue to fall back to the original English text.

The Greek strings were translated specifically for GridCraft. They are not extracted from
proprietary spreadsheet software or localization assets. The MIT OR Apache-2.0 repository
licenses apply to this contribution.
