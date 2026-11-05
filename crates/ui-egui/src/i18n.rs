//! Interface translations. Command ids and document text stay stable; only interface chrome is
//! translated. Untranslated labels fall back to English, so coverage can grow incrementally.
//!
//! The interface language is a preference ([`crate::UiState::language`]): on first run it defaults
//! to the system's language ([`Language::system`]), and the View tab switches it by hand.
//!
//! CJK fonts stay system-provided: [`crate::theme`] appends available CJK faces to every family so
//! Japanese, Chinese and Korean labels and workbook text can render without bundling font files.

use serde::{Deserialize, Serialize};

mod cs;
mod de;
mod el;
mod es;
mod es_functions;
mod fr;
mod it;
mod ja;
mod ko;
mod nl;
mod pl;
mod pt;
mod ro;
mod ru;
mod sr;
mod tr;
mod uk;
mod vi;
mod zh;
mod zh_errors;
mod zh_functions;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Es,
    El,
    Zh,
    Ja,
    Ko,
    Ru,
    #[serde(rename = "pt")]
    PtBr,
    De,
    Cs,
    Fr,
    Sr,
    Tr,
    Pl,
    It,
    Uk,
    Vi,
    Nl,
    Ro,
}

impl Language {
    pub const ALL: [Self; 19] = [
        Self::En,
        Self::Es,
        Self::El,
        Self::Zh,
        Self::Ja,
        Self::Ko,
        Self::Ru,
        Self::PtBr,
        Self::De,
        Self::Cs,
        Self::Fr,
        Self::Sr,
        Self::Pl,
        Self::It,
        Self::Uk,
        Self::Vi,
        Self::Nl,
        Self::Ro,
        Self::Tr,
    ];

    /// Every language in the order the switcher lists them: English first (the source language),
    /// then the rest by their own name with accents folded, so Čeština sits under C. Names in
    /// other scripts follow the Latin ones in code point order: Cyrillic, then the CJK languages.
    pub fn for_picker() -> Vec<Self> {
        let mut rest: Vec<Self> = Self::ALL.into_iter().filter(|l| *l != Self::En).collect();
        rest.sort_by_cached_key(|l| name_sort_key(l.name()));
        std::iter::once(Self::En).chain(rest).collect()
    }

    /// The language's own name, shown in the switcher.
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Es => "Español",
            Self::El => "Ελληνικά",
            Self::Zh => "简体中文",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Ru => "Русский",
            Self::PtBr => "Português (Brasil)",
            Self::De => "Deutsch",
            Self::Cs => "Čeština",
            Self::Fr => "Français",
            Self::Sr => "Srpski",
            Self::Tr => "Türkçe",
            Self::Pl => "Polski",
            Self::It => "Italiano",
            Self::Uk => "Українська",
            Self::Vi => "Tiếng Việt",
            Self::Nl => "Nederlands",
            Self::Ro => "Română",
        }
    }

    /// The language's code, as saved in `ui.json` and taken by `app.language.set`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
            Self::El => "el",
            Self::Zh => "zh",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ru => "ru",
            Self::PtBr => "pt",
            Self::De => "de",
            Self::Cs => "cs",
            Self::Fr => "fr",
            Self::Sr => "sr",
            Self::Tr => "tr",
            Self::Pl => "pl",
            Self::It => "it",
            Self::Uk => "uk",
            Self::Vi => "vi",
            Self::Nl => "nl",
            Self::Ro => "ro",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        Self::from_tag(code)
    }

    /// Every code `app.language.set` takes, quoted and joined (`"en"|"es"|…`), for command docs
    /// and errors.
    pub fn code_list(separator: &str) -> String {
        Self::ALL.iter().map(|l| format!("\"{}\"", l.code())).collect::<Vec<_>>().join(separator)
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().split(['-', '_']).next()?.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "es" => Some(Self::Es),
            "el" => Some(Self::El),
            "zh" => Some(Self::Zh),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "ru" => Some(Self::Ru),
            "pt" => Some(Self::PtBr),
            "de" => Some(Self::De),
            "cs" => Some(Self::Cs),
            "fr" => Some(Self::Fr),
            "sr" => Some(Self::Sr),
            "tr" => Some(Self::Tr),
            "pl" => Some(Self::Pl),
            "it" => Some(Self::It),
            "uk" => Some(Self::Uk),
            "vi" => Some(Self::Vi),
            "nl" => Some(Self::Nl),
            "ro" => Some(Self::Ro),
            _ => None,
        }
    }

    /// The system's preferred interface language, best effort; English when it is unknown or
    /// unsupported. Used as the first-run default while nothing is saved.
    pub fn system() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            sys_locale::get_locale().and_then(|tag| Self::from_tag(&tag)).unwrap_or(Self::En)
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window().and_then(|window| window.navigator().language()).and_then(|tag| Self::from_tag(&tag)).unwrap_or(Self::En)
        }
    }

    pub fn formula_locale(self) -> gridcraft_engine::formula::FormulaLocale {
        if self == Self::Es { gridcraft_engine::formula::FormulaLocale::Es } else { gridcraft_engine::formula::FormulaLocale::En }
    }

    /// Translates an interface label; untranslated labels fall back to English.
    ///
    /// A label that needs two translations (Home's text "Orientation" vs Page Layout's print
    /// "Orientation") is keyed `context|label`: English shows only the part after the `|`.
    pub fn tr(self, text: &str) -> &str {
        self.raw(text).unwrap_or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            record_untranslated(self, text);
            text.rsplit_once('|').map_or(text, |(_, label)| label)
        })
    }

    /// The catalog translation of `text`, or `None`. `tr` adds the untranslated-label trace on
    /// top; callers holding runtime data (an error message with a file name in it) use `raw`
    /// directly so the trace stays a report of interface labels only.
    fn raw(self, text: &str) -> Option<&'static str> {
        match self {
            Self::En => None,
            Self::Es => es::translate(text),
            Self::El => el::translate(text),
            Self::Ja => ja::translate(text),
            Self::Zh => zh::translate(text),
            Self::Ko => ko::translate(text),
            Self::Ru => ru::translate(text),
            Self::PtBr => pt::translate(text),
            Self::De => de::translate(text),
            Self::Cs => cs::translate(text),
            Self::Fr => fr::translate(text),
            Self::Sr => sr::translate(text),
            Self::Tr => tr::translate(text),
            Self::Pl => pl::translate(text),
            Self::It => it::translate(text),
            Self::Uk => uk::translate(text),
            Self::Vi => vi::translate(text),
            Self::Nl => nl::translate(text),
            Self::Ro => ro::translate(text),
        }
    }

    /// A sentence built with data (`Saved macro “Budget” (3 steps)`). The table entry spells the
    /// same `{slot}` names, so word order and measure words follow the language; a language
    /// without the entry gets the English with the same slots filled.
    pub fn tr_fill(self, english: &str, slots: &[(&str, &str)]) -> String {
        let template = self.tr(english);
        // Each placeholder is read from the original template: a value may itself look like a
        // placeholder (a macro can be named `{book}`), so a search-and-replace loop would rewrite
        // data inserted earlier.
        let mut out = String::with_capacity(template.len() + slots.iter().map(|(_, v)| v.len()).sum::<usize>());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) => {
                    let name = &after[..close];
                    match slots.iter().find(|(n, _)| *n == name).map(|(_, v)| *v) {
                        Some(value) => out.push_str(value),
                        // Not one of this sentence's slots: keep the braces as written.
                        None => out.push_str(&rest[open..open + close + 2]),
                    }
                    rest = &after[close + 1..];
                }
                None => {
                    out.push_str(&rest[open..]);
                    rest = "";
                    break;
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// The display form of a comment or note author. A stored author is user data and shows as
    /// written — a name may carry a `|`, which `tr` reads as a context separator — so only the
    /// engine's built-in default name is translated, and it reads in the interface language.
    pub fn author_name(self, author: &str) -> String {
        if author == "GridCraft User" { self.tr(author).to_owned() } else { author.to_owned() }
    }

    pub(crate) fn tr_error(self, message: &str) -> String {
        let message = if message.starts_with("command `") { crate::clean_error(message) } else { message.to_owned() };

        // Runtime data (a file name, a cell reference) must not enter the untranslated-label
        // trace, so the complete message is looked up without tracing; only the template match
        // below fills the slots, and it translates captured values from the catalog without
        // tracing them either.
        let fallback = message.rsplit_once('|').map_or(message.as_str(), |(_, label)| label);
        let translated = self.raw(&message).unwrap_or(fallback);

        if translated == fallback
            && let Some(filled) = self.error_template(&message)
        {
            return filled;
        }

        if translated == fallback { message } else { translated.to_owned() }
    }

    /// A worksheet function's description in this language, looked up by the English text of the
    /// function catalog; `None` when untranslated, and the help shows the English.
    pub fn function_description(self, english: &str) -> Option<&'static str> {
        lookup(self.function_help()?.0, english)
    }

    /// A function argument name in this language, given as the catalog spells it without
    /// brackets or index (`number`, `logical_test`).
    pub fn function_argument(self, english: &str) -> Option<&'static str> {
        lookup(self.function_help()?.1, english)
    }

    /// The function help tables (descriptions, argument names) of the languages that have them.
    fn function_help(self) -> Option<(HelpTable, HelpTable)> {
        match self {
            Self::Es => Some((es_functions::DESCRIPTIONS, es_functions::ARGUMENTS)),
            Self::Zh => Some((zh_functions::DESCRIPTIONS, zh_functions::ARGUMENTS)),
            _ => None,
        }
    }

    /// A message the engine built with data in it (`We can't open 'book.xlsx': …`), matched
    /// against the templates of this language: the fixed text must line up and the data is
    /// carried over, translated too when it is an interface phrase of its own.
    fn error_template(self, message: &str) -> Option<String> {
        let table = match self {
            Self::Zh => zh_errors::TEMPLATES,
            _ => return None,
        };
        table.iter().find_map(|(english, localized)| {
            let values = fill_template(english, message)?;
            // A captured value is user data (a file or sheet name may carry a `|`, which `tr`
            // reads as a context separator): translate it only when it is a known interface
            // phrase, and look it up without tracing — this is not an interface label.
            let values: Vec<String> =
                values.iter().map(|value| if value.contains('|') { value.clone() } else { self.raw(value).unwrap_or(value).to_string() }).collect();
            splice_template(localized, &values)
        })
    }
}

/// The literal parts of a template, with a slot between each pair: `a{x}b` → `["a", "b"]`.
fn template_parts(template: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        parts.push(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    parts.push(rest);
    parts
}

/// The values a `template`'s slots hold in `text`, or `None` when the fixed text doesn't line up.
fn fill_template(template: &str, text: &str) -> Option<Vec<String>> {
    let parts = template_parts(template);
    let mut rest = text.strip_prefix(*parts.first()?)?;
    let mut values = Vec::new();
    for part in &parts[1..] {
        // A template that ends in a slot has an empty last literal: its value runs to the end.
        let at = if part.is_empty() { rest.len() } else { rest.find(part)? };
        values.push(rest[..at].to_string());
        rest = &rest[at + part.len()..];
    }
    // The last literal must also end the text: a template that only matches a prefix would eat
    // the rest of the sentence.
    rest.is_empty().then_some(values)
}

/// `template` with its slots filled by `values`.
fn splice_template(template: &str, values: &[String]) -> Option<String> {
    let parts = template_parts(template);
    if parts.len() != values.len() + 1 {
        return None;
    }
    let mut out = String::with_capacity(template.len() + values.iter().map(String::len).sum::<usize>());
    for (i, part) in parts.iter().enumerate() {
        out.push_str(part);
        if let Some(value) = values.get(i) {
            out.push_str(value);
        }
    }
    Some(out)
}

/// English text and its translation, sorted by the English.
type HelpTable = &'static [(&'static str, &'static str)];

/// The translation of `english` in a help table (binary search, so the table must stay sorted).
fn lookup(table: HelpTable, english: &str) -> Option<&'static str> {
    let i = table.binary_search_by(|(key, _)| (*key).cmp(english)).ok()?;
    table.get(i).map(|(_, translated)| *translated)
}

/// Collects labels that `tr` fell back to English for, so a localization pass can be checked
/// against the real interface instead of against a grep. Off unless `GRIDCRAFT_I18N_TRACE` names
/// a file; the snapshot example writes them out (`--language zh`).
#[cfg(not(target_arch = "wasm32"))]
pub fn untranslated() -> std::collections::BTreeSet<String> {
    SEEN.lock().map(|seen| seen.clone()).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn record_untranslated(language: Language, text: &str) {
    static TRACING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if language == Language::En || !*TRACING.get_or_init(|| std::env::var_os("GRIDCRAFT_I18N_TRACE").is_some()) {
        return;
    }
    if let Ok(mut seen) = SEEN.lock() {
        seen.insert(text.to_string());
    }
}

#[cfg(not(target_arch = "wasm32"))]
static SEEN: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// Deserializes a saved language leniently: an unknown or malformed value (a language removed in a
/// later version, a hand-edited `ui.json`) reads as English instead of failing the whole `UiState`,
/// which would reset every other preference with it.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Language, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Code(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Code(code) => Language::parse(&code).unwrap_or_default(),
        Raw::Other(_) => Language::default(),
    })
}

/// The language a saved `ui.json` asks for, if it names a supported one. The desktop app uses it
/// to tell "saved English" from "nothing usable saved" (first run, an older file, an unknown
/// value), which starts from [`Language::system`] instead.
pub fn saved_language(ui_json: &serde_json::Value) -> Option<Language> {
    ui_json.get("language").and_then(serde_json::Value::as_str).and_then(Language::parse)
}

/// egui context-data key holding the language of the frame being drawn. Widgets that only get a
/// `Ui` (not the app) read it to translate their labels; [`set_current`] writes it once per frame.
fn lang_key() -> egui::Id {
    egui::Id::unique("gridcraft.language")
}

/// The language the current frame is drawing in. `En` when unset.
pub fn current(ctx: &egui::Context) -> Language {
    ctx.data(|d| d.get_temp(lang_key())).unwrap_or_default()
}

/// Sets the language for the current frame; call once per frame before anything is drawn.
pub fn set_current(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|d| d.insert_temp(lang_key(), language));
}

/// `name` lowercased with the Latin accents that occur in language names folded away, so a
/// byte-wise sort reads alphabetically. Other scripts are left as they are.
fn name_sort_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ē' | 'ě' | 'ę' | 'ế' | 'ệ' => 'e',
            'ì'..='ï' | 'ī' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ś' | 'š' => 's',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Help tables are sorted and unique: lookups binary-search them.
    #[test]
    fn function_help_tables_are_sorted_and_unique() {
        for language in Language::ALL {
            let Some((description_table, argument_table)) = language.function_help() else { continue };
            for table in [description_table, argument_table] {
                assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0), "{language:?} function help is not sorted and unique");
                for (english, translated) in table {
                    assert!(!translated.is_empty(), "{language:?}: empty translation of {english:?}");
                }
            }
        }
    }

    #[test]
    fn untranslated_text_falls_back_to_english() {
        let label = |key: &'static str| key.rsplit_once('|').map_or(key, |(_, label)| label);
        for (english, _) in ja::CORE {
            assert_eq!(Language::En.tr(english), label(english));
        }
        for language in Language::ALL {
            assert_eq!(language.tr("Sheet1!A1"), "Sheet1!A1");
            assert_eq!(language.tr("Not in any catalog|Untranslated"), "Untranslated");
        }
        // A `context|label` key shows only its label in English.
        assert_eq!(Language::En.tr("Page Layout|Orientation"), "Orientation");
        // Errors drop the `command `id`:` prefix before translating; an untranslated one stays whole.
        assert_eq!(Language::PtBr.tr_error("command `chart`: Select a chart first."), Language::PtBr.tr("Select a chart first."));
        assert_eq!(Language::PtBr.tr_error("Sheet|Unknown failure"), "Sheet|Unknown failure");
        assert_eq!(Language::PtBr.tr_error("Printer Office: permission denied"), "Printer Office: permission denied");
        assert_eq!(Language::parse("xx"), None);
    }

    /// A catalog whose translations slip onto their neighbours' keys still covers every key, so
    /// check the shape of each translation against its English: placeholders survive, and a
    /// one-line label (a menu item, a tooltip) doesn't turn into a two-line ribbon label.
    #[test]
    fn translations_keep_the_shape_of_their_english() {
        let placeholders =
            |s: &str| s.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string())).collect::<Vec<_>>();
        for language in Language::ALL {
            for (english, _) in ja::CORE {
                let label = english.rsplit_once('|').map_or(*english, |(_, label)| label);
                let text = language.tr(english);
                assert_eq!(placeholders(text), placeholders(label), "{language:?}: {english:?} => {text:?}");
                // Big ribbon buttons whose English fits on one line may still need two.
                if !label.contains('\n') && !["AutoSum", "PivotTable"].contains(english) {
                    assert!(!text.contains('\n'), "{language:?}: one-line {english:?} => {text:?}");
                }
            }
        }
    }

    /// The screen-reader names for the formula-bar controls must be translated in every catalog
    /// (no silent English fallback) and stay single-line labels.
    #[test]
    fn accessible_control_names_are_translated_in_every_catalog() {
        for key in ["Name Box", "Cell Editor", "Font Size", "Sheet Name", "Text Box"] {
            for language in Language::ALL {
                let shown = language.tr(key);
                assert!(!shown.contains('\n'), "{}: {key:?} is not a single-line label: {shown:?}", language.code());
                if language != Language::En {
                    assert_ne!(shown, key, "{} falls back to English for {key:?}", language.code());
                }
            }
        }
    }

    #[test]
    fn translations_never_show_the_context_prefix() {
        // `tr` returns a found translation as it is, so a catalog that keeps `context|` in its
        // value shows it on screen (File › Options read "Options|Général" in French).
        let keys = ja::CORE.iter().map(|(english, _)| *english).chain(["Options|General", "Options|Save", "Sheet Options|View"]);
        for english in keys {
            for language in Language::ALL {
                let shown = language.tr(english);
                assert!(!shown.contains('|'), "{} shows a context prefix for {english}: {shown}", language.code());
            }
        }
    }

    #[test]
    fn unknown_saved_language_keeps_the_other_preferences() {
        let ui: crate::UiState = serde_json::from_str(r#"{"dark": true, "language": "xx"}"#).unwrap();
        assert!(ui.dark);
        assert_eq!(ui.language, Language::En);
        let ui: crate::UiState = serde_json::from_str(r#"{"dark": true, "language": 7}"#).unwrap();
        assert!(ui.dark);
        let ui: crate::UiState = serde_json::from_str(r#"{"language": "ja"}"#).unwrap();
        assert_eq!(ui.language, Language::Ja);
        assert_eq!(crate::UiState::default().language, Language::En, "the default never reads the host locale");
        assert_eq!(saved_language(&serde_json::json!({"language": "ja"})), Some(Language::Ja));
        assert_eq!(saved_language(&serde_json::json!({"language": "xx"})), None);
        assert_eq!(saved_language(&serde_json::json!({})), None);
    }

    #[test]
    fn tags_reduce_to_supported_languages() {
        assert_eq!(Language::from_tag("ja-JP"), Some(Language::Ja));
        assert_eq!(Language::from_tag("en_US"), Some(Language::En));
        assert_eq!(Language::from_tag("es_MX"), Some(Language::Es));
        assert_eq!(Language::from_tag("el-GR"), Some(Language::El));
        assert_eq!(Language::from_tag("el_GR.UTF-8"), Some(Language::El));
        assert_eq!(Language::parse("el"), Some(Language::El));
        assert_eq!(Language::from_tag("zh-Hans-CN"), Some(Language::Zh));
        assert_eq!(Language::from_tag("ko-KR"), Some(Language::Ko));
        assert_eq!(Language::from_tag("RU_ru"), Some(Language::Ru));
        assert_eq!(Language::from_tag("de-DE"), Some(Language::De));
        assert_eq!(Language::from_tag("de_AT"), Some(Language::De));
    }

    #[test]
    fn tags_reduce_to_supported_languages_with_locale_suffixes() {
        assert_eq!(Language::from_tag("de-CH"), Some(Language::De));
        assert_eq!(Language::from_tag("pt-BR"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt-br"), Some(Language::PtBr));
        assert_eq!(Language::from_tag("cs-CZ"), Some(Language::Cs));
        assert_eq!(Language::from_tag("cs_CZ.UTF-8"), Some(Language::Cs));
        assert_eq!(Language::parse("cs"), Some(Language::Cs));
        assert_eq!(Language::from_tag("fr-FR"), Some(Language::Fr));
        assert_eq!(Language::from_tag("fr_CA"), Some(Language::Fr));
        assert_eq!(Language::parse("fr"), Some(Language::Fr));
        assert_eq!(Language::from_tag("sr-Latn-RS"), Some(Language::Sr));
    }

    #[test]
    fn tags_reduce_to_other_supported_languages() {
        assert_eq!(Language::from_tag("sr-RS"), Some(Language::Sr));
        assert_eq!(Language::parse("sr"), Some(Language::Sr));
        assert_eq!(Language::from_tag("tr-TR"), Some(Language::Tr));
        assert_eq!(Language::from_tag("tr_TR.UTF-8"), Some(Language::Tr));
        assert_eq!(Language::parse("tr"), Some(Language::Tr));
        assert_eq!(Language::from_tag("pl-PL"), Some(Language::Pl));
        assert_eq!(Language::from_tag("pl_PL.UTF-8"), Some(Language::Pl));
        assert_eq!(Language::parse("pl"), Some(Language::Pl));
        assert_eq!(Language::from_tag("it-IT"), Some(Language::It));
        assert_eq!(Language::from_tag("it_CH.UTF-8"), Some(Language::It));
        assert_eq!(Language::parse("it"), Some(Language::It));
    }

    #[test]
    fn tags_reduce_to_remaining_supported_languages() {
        assert_eq!(Language::from_tag("uk-XX"), Some(Language::Uk));
        assert_eq!(Language::parse("uk"), Some(Language::Uk));
        assert_eq!(Language::from_tag("vi-VN"), Some(Language::Vi));
        assert_eq!(Language::parse("vi"), Some(Language::Vi));
        assert_eq!(Language::from_tag("nl-NL"), Some(Language::Nl));
        assert_eq!(Language::from_tag("nl_BE"), Some(Language::Nl));
        assert_eq!(Language::parse("nl"), Some(Language::Nl));
        assert_eq!(Language::from_tag("ro-RO"), Some(Language::Ro));
        assert_eq!(Language::from_tag("ro_MD.UTF-8"), Some(Language::Ro));
        assert_eq!(Language::parse("ro"), Some(Language::Ro));
    }
    #[test]
    fn an_unset_language_preference_is_english_until_the_app_applies_the_system_one() {
        // Deserializing never reads the host locale (tests stay deterministic); the desktop app
        // applies `Language::system()` itself when `ui.json` names no usable language.
        let restored: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(restored.language, Language::En);
        assert_eq!(saved_language(&serde_json::json!({"language": "zh"})), Some(Language::Zh));
    }

    #[test]
    fn language_commands_switch_and_persist() {
        let mut app = crate::SheetApp::new(gridcraft_engine::Session::default(), Default::default());
        for language in Language::ALL {
            app.run("app.language.set", serde_json::json!({"language": language.code()})).unwrap();
            assert_eq!(app.ui.language, language);
            let restored: crate::UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
            assert_eq!(restored.language, language);
        }
        app.run("app.language.set", serde_json::json!({"code": "ja"})).unwrap();
        assert_eq!(app.ui.language, Language::Ja, "`code` is an alias for `language`");
        assert!(app.run("app.language.set", serde_json::json!({})).is_err(), "no language is an error, not a dialog");
        let unknown = app.run("app.language.set", serde_json::json!({"language": "xx"})).unwrap_err().to_string();
        for l in Language::ALL {
            assert!(unknown.contains(&format!("\"{}\"", l.code())), "the error lists {:?}: {unknown}", l.code());
        }
        app.run("app.language.english", serde_json::json!({})).unwrap();
        assert_eq!(app.ui.language, Language::En);
        for l in Language::ALL {
            assert_eq!(Language::parse(l.code()), Some(l));
        }
    }

    #[test]
    fn picker_lists_english_first_then_names_alphabetically() {
        let order = Language::for_picker();
        assert_eq!(order.first(), Some(&Language::En));
        assert_eq!(order.len(), Language::ALL.len());
        let keys: Vec<String> = order[1..].iter().map(|l| name_sort_key(l.name())).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
        assert_eq!(name_sort_key("Čeština"), "cestina");
    }
    /// Every command label (what its ribbon button, menu item and tooltip read) and every ribbon or
    /// menu path segment has a Chinese translation, so no tooltip, menu or command search result
    /// falls back to English. Tooltips and open menus are never painted in an offscreen snapshot,
    /// so the render trace cannot see them; this test can.
    #[test]
    fn every_command_label_reads_in_chinese() {
        fn label_of(key: &str) -> &str {
            key.rsplit_once('|').map_or(key, |(_, label)| label)
        }
        let mut missing: Vec<&str> = vec![];
        // Labels that read the same in every language: zoom levels, units, function names.
        const NEUTRAL: &[&str] = &["100%"];
        for spec in gridcraft_engine::command_specs() {
            if !NEUTRAL.contains(&spec.label) && Language::Zh.tr(spec.label) == label_of(spec.label) {
                missing.push(spec.label);
            }
            for segment in spec.menu {
                if !NEUTRAL.contains(segment) && Language::Zh.tr(segment) == *segment {
                    missing.push(segment);
                }
            }
        }
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "Simplified Chinese missing {} labels: {missing:#?}", missing.len());
    }

    /// A message the engine built with data in it is matched by its fixed text, and the data
    /// (a file name, a range, a count) comes across.
    #[test]
    fn error_templates_carry_the_data_across() {
        let zh = Language::Zh;
        assert_eq!(zh.tr_error("We can't open 'book.xlsx': permission denied"), "无法打开“book.xlsx”：permission denied");
        assert_eq!(zh.tr_error("Too many variable cells (at most 64)."), "可变单元格过多（最多 64 个）。");
        // A value that is itself an interface phrase is translated inside the sentence too.
        assert_eq!(
            zh.tr_error("Sorting isn't allowed on this protected sheet (Review › Unprotect Sheet)."),
            "受保护的工作表不允许排序操作（审阅 › 撤销工作表保护）。"
        );
        // A sentence the templates don't describe stays whole rather than half-matching.
        assert_eq!(zh.tr_error("Printer Office: permission denied"), "Printer Office: permission denied");
        // A sentence with no data takes the plain table.
        assert_eq!(zh.tr_error("no such sheet"), "没有该工作表");
        assert_eq!(Language::En.tr_error("no such sheet"), "no such sheet");
    }

    #[test]
    fn template_slots_split_and_fill() {
        assert_eq!(template_parts("a{x}b"), vec!["a", "b"]);
        assert_eq!(template_parts("no slots"), vec!["no slots"]);
        assert_eq!(template_parts("a{x}b{y}"), vec!["a", "b", ""]);
        assert_eq!(fill_template("a{x}b", "a1b"), Some(vec!["1".to_string()]));
        assert_eq!(fill_template("a{x}b", "a1b tail"), None, "the last literal must end the text");
        assert_eq!(fill_template("a{x}b", "zb"), None);
        assert_eq!(splice_template("X{v}Y", &["1".to_string()]), Some("X1Y".to_string()));
        assert_eq!(splice_template("X{v}Y{w}", &["1".to_string()]), None);
    }
}
