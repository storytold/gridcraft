# European Portuguese (pt-pt) interface localization

The UI now offers **Português (Portugal)** via View > Interface language and uses European
Portuguese automatically on a fresh installation when the system locale is `pt-PT` or
`pt_PT.UTF-8`. `pt` and `pt-BR` stay Brazilian Portuguese, so preferences saved before this
language existed keep working.

The localization follows the existing GridCraft interface translation infrastructure:
- `crates/ui-egui/src/i18n/pt_pt.rs` supplies European Portuguese translations of UI strings.
- `crates/ui-egui/src/i18n.rs` registers `Language::PtPt` (`pt-pt`), language selection,
  persisted preferences, and translation tests.
- Command identifiers, spreadsheet content, function names and references remain stable.
- European Portuguese UI translation **does not add a Portuguese spelling dictionary** or
  translate formula function names, locale-specific separators or number formats.
- Unknown UI labels continue to fall back to the original English text.

The Portuguese strings were translated specifically for GridCraft. They are not extracted from
proprietary spreadsheet software or localization assets. The MIT OR Apache-2.0 repository
licenses apply to this contribution.
