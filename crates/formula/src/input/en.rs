//! Canonical English syntax, used by files/APIs and by untranslated formula locales.

pub(super) static DATA: super::LocaleData = super::LocaleData {
    functions: &[],
    errors: &[],
    table_items: None,
    separators: super::ENGLISH_SEPARATORS,
    fold_alias: str::to_lowercase,
    aliases: std::sync::OnceLock::new(),
};
