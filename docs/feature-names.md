# Feature naming

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (Hungarian Diagram is Ábra, distinct from Chart; Decorative Text is Díszítő szöveg); previously minor (Ukrainian Chart is Діаграма); previously minor (Diagram words aligned with craft-i18n#211)

ArtCraft apps use descriptive names for their own features. Product names used to describe compatibility, file formats, attribution or external software remain accurate.

## Naming map

SmartArt → Diagram; WordArt → Decorative Text. These are catalog labels for pending features, not a claim that the features are implemented.

## Compatibility boundary

The insert.smartArt and insert.wordArt command IDs and pending keytips remain unchanged. OOXML data, imported names and interoperability references are unchanged. No assets, attribution, dependencies or rendering behavior change.

## Localization and verification

Each shipped non-English interface catalog includes the new labels. Diagram uses a word distinct from Chart in every catalog: German (`Schema`), Ukrainian (`Схема`), Romanian (`Schemă`) and Hungarian (`Ábra`) avoid their chart word. Hungarian Chart remains `Diagram`; Decorative Text is `Díszítő szöveg`. The German and Ukrainian terms match the shared wording in [craft-i18n#211](https://github.com/storytold/craft-i18n/pull/211). Regression tests cover the labels, every shipped catalog and stable command identities. The translations are AI-assisted and not yet reviewed by native speakers.

## Regression coverage

The standard-library-only `display_names` integration tests scan production Rust string literals for the retired display names and preserve explicit compatibility exceptions. They also test identifier, comment and raw-string handling. These focused checks do not replace application or visual testing.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Hungarian Diagram (`Ábra`) remains distinct from Chart (`Diagram`); Decorative Text is `Díszítő szöveg` |
| 2026-11-10 | minor | Ukrainian Chart is `Діаграма`, matching [craft-i18n#211](https://github.com/storytold/craft-i18n/pull/211) and distinct from Diagram (`Схема`) |
| 2026-11-10 | minor | Diagram no longer repeats the chart word: German `Schema`, Ukrainian `Схема` (as craft-i18n#211), Romanian `Schemă`; a test covers every shipped catalog |
| 2026-11-08 | minor | Created: SmartArt → Diagram and WordArt → Decorative Text as GridCraft's own catalog labels |
