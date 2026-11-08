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
mod cs_functions;
mod da;
mod de;
mod de_functions;
mod el;
mod el_functions;
mod error_templates;
mod es;
mod es_functions;
mod fr;
mod fr_functions;
mod it;
mod it_functions;
mod ja;
mod ja_functions;
mod ka;
mod ko;
mod ko_functions;
mod lt;
mod nl;
mod nl_functions;
mod pl;
mod pl_functions;
mod pt;
mod pt_functions;
mod ro;
mod ro_functions;
mod ru;
mod ru_formats;
mod ru_functions;
mod sr;
mod sr_functions;
mod sv;
mod tr;
mod tr_functions;
mod uk;
mod uk_functions;
mod vi;
mod vi_functions;
mod zh;
mod zh_functions;
mod zh_tw;
mod zh_tw_functions;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Es,
    El,
    Zh,
    #[serde(rename = "zh-tw")]
    ZhTw,
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
    Lt,
    Ka,
    Sv,
    Da,
}

impl Language {
    /// Number-format spelling and presets for the UI; stored format codes remain canonical.
    pub(crate) fn format_locale(self) -> &'static crate::format_locale::FormatLocale {
        match self {
            Self::Ru => &ru_formats::FORMAT,
            _ => &crate::format_locale::EN,
        }
    }
    /// Regional number/date output for UI samples and painted cells; engine output stays invariant.
    pub(crate) const fn number_locale(self) -> gridcraft_engine::core::Locale {
        match self {
            Self::De => gridcraft_engine::core::Locale::De,
            _ => gridcraft_engine::core::Locale::En,
        }
    }

    pub const ALL: [Self; 24] = [
        Self::En,
        Self::Es,
        Self::El,
        Self::Zh,
        Self::ZhTw,
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
        Self::Lt,
        Self::Ka,
        Self::Sv,
        Self::Da,
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
            Self::Lt => "Lietuvių",
            Self::Ka => "ქართული",
            Self::Sv => "Svenska",
            Self::Da => "Dansk",
            Self::Es => "Español",
            Self::El => "Ελληνικά",
            Self::Zh => "简体中文",
            Self::ZhTw => "繁體中文",
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
            Self::Lt => "lt",
            Self::Ka => "ka",
            Self::Sv => "sv",
            Self::Da => "da",
            Self::Es => "es",
            Self::El => "el",
            Self::Zh => "zh",
            Self::ZhTw => "zh-tw",
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
        let mut parts = tag.trim().split('.').next()?.split(['-', '_']);
        match parts.next()?.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "lt" => Some(Self::Lt),
            "ka" => Some(Self::Ka),
            "sv" => Some(Self::Sv),
            "da" => Some(Self::Da),
            "es" => Some(Self::Es),
            "el" => Some(Self::El),
            "zh" => {
                // A script subtag decides; the region only when no script is given. `zh-Hans-HK`
                // and `zh-Hans-MO` are Simplified Chinese in Hong Kong and Macau, both valid macOS
                // identifiers, so their `hk`/`mo` region must not select Traditional.
                let parts: Vec<String> = parts.map(|part| part.to_ascii_lowercase()).collect();
                let has = |p: &str| parts.iter().any(|part| part == p);
                let traditional = if has("hant") {
                    true
                } else if has("hans") {
                    false
                } else {
                    has("tw") || has("hk") || has("mo")
                };
                Some(if traditional { Self::ZhTw } else { Self::Zh })
            }
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

    /// Selects formula syntax for the interface language; languages without alias tables use English.
    pub fn formula_locale(self) -> gridcraft_engine::formula::FormulaLocale {
        match self {
            Self::Es => gridcraft_engine::formula::FormulaLocale::Es,
            Self::De => gridcraft_engine::formula::FormulaLocale::De,
            Self::Pl => gridcraft_engine::formula::FormulaLocale::Pl,
            Self::Fr => gridcraft_engine::formula::FormulaLocale::Fr,
            Self::It => gridcraft_engine::formula::FormulaLocale::It,
            Self::PtBr => gridcraft_engine::formula::FormulaLocale::PtBr,
            Self::Ru => gridcraft_engine::formula::FormulaLocale::Ru,
            Self::Tr => gridcraft_engine::formula::FormulaLocale::Tr,
            _ => gridcraft_engine::formula::FormulaLocale::En,
        }
    }

    /// Translates an interface label; untranslated labels fall back to English.
    ///
    /// A label that needs two translations (Home's text "Orientation" vs Page Layout's print
    /// "Orientation") is keyed `context|label`: English shows only the part after the `|`.
    pub fn tr(self, text: &str) -> &str {
        self.translation(text).unwrap_or_else(|| text.rsplit_once('|').map_or(text, |(_, label)| label))
    }

    /// Raw catalog lookup for contextual labels and shared message templates.
    pub fn translation(self, text: &str) -> Option<&'static str> {
        match self {
            Self::En => None,
            Self::Es => es::translate(text),
            Self::El => el::translate(text),
            Self::Ja => ja::translate(text),
            Self::Zh => zh::translate(text),
            Self::ZhTw => zh_tw::translate(text),
            Self::Lt => lt::translate(text),
            Self::Ka => ka::translate(text),
            Self::Sv => sv::translate(text),
            Self::Da => da::translate(text),
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

    /// Translates an import warning for display in the interface. The file-format and engine
    /// layers keep returning canonical English text to the CLI, control channel, and MCP.
    pub(crate) fn open_warning(self, warning: &str) -> String {
        if let Some(target) = warning.strip_prefix("missing picture ") {
            return self.tr("missing picture {target}").replace("{target}", target);
        }
        if let Some(target) = warning.strip_prefix("chart ").and_then(|s| s.strip_suffix(" has no supported plot and was skipped")) {
            return self.tr("chart {target} has no supported plot and was skipped").replace("{target}", target);
        }
        self.tr(warning).to_owned()
    }

    /// Ribbon tabs can be nouns while their corresponding commands are verbs.
    pub fn tr_tab(self, text: &str) -> &str {
        self.translation(&format!("Tab|{text}")).unwrap_or_else(|| self.tr(text))
    }

    /// Translates known fixed or templated diagnostics after removing engine command wrappers.
    /// Unmatched parameter errors retain their command context; other command wrappers are cleaned.
    /// Captured user data stays literal.
    pub(crate) fn tr_error(self, message: &str) -> String {
        let original_parameters = message.starts_with("invalid parameters for `").then_some(message);
        let message = if message.starts_with("command `") || message.starts_with("invalid parameters for `") {
            crate::clean_error(message)
        } else {
            message.to_owned()
        };

        let translated = self.tr(&message);
        let fallback = message.rsplit_once('|').map_or(message.as_str(), |(_, label)| label);

        if translated != fallback {
            return translated.to_owned();
        }
        error_templates::format_message(&message, self).unwrap_or_else(|| original_parameters.map_or(message, str::to_owned))
    }

    /// A worksheet function's description in this language, looked up by the English text of the
    /// function catalog; `None` when untranslated, and the help shows the English.
    pub fn function_description(self, english: &str) -> Option<&'static str> {
        self.function_help().and_then(|help| lookup(help.0, english))
    }

    /// A function argument name in this language, given as the catalog spells it without
    /// brackets or index (`number`, `logical_test`).
    pub fn function_argument(self, english: &str) -> Option<&'static str> {
        self.function_help().and_then(|help| lookup(help.1, english))
    }

    /// Per-language function-help data, independent of formula aliases and message catalogs.
    fn function_help(self) -> Option<(HelpTable, HelpTable)> {
        match self {
            Self::Cs => Some((cs_functions::DESCRIPTIONS, cs_functions::ARGUMENTS)),
            Self::De => Some((de_functions::DESCRIPTIONS, de_functions::ARGUMENTS)),
            Self::El => Some((el_functions::DESCRIPTIONS, el_functions::ARGUMENTS)),
            Self::Es => Some((es_functions::DESCRIPTIONS, es_functions::ARGUMENTS)),
            Self::Fr => Some((fr_functions::DESCRIPTIONS, fr_functions::ARGUMENTS)),
            Self::It => Some((it_functions::DESCRIPTIONS, it_functions::ARGUMENTS)),
            Self::Ja => Some((ja_functions::DESCRIPTIONS, ja_functions::ARGUMENTS)),
            Self::Ko => Some((ko_functions::DESCRIPTIONS, ko_functions::ARGUMENTS)),
            Self::Nl => Some((nl_functions::DESCRIPTIONS, nl_functions::ARGUMENTS)),
            Self::Pl => Some((pl_functions::DESCRIPTIONS, pl_functions::ARGUMENTS)),
            Self::PtBr => Some((pt_functions::DESCRIPTIONS, pt_functions::ARGUMENTS)),
            Self::Ro => Some((ro_functions::DESCRIPTIONS, ro_functions::ARGUMENTS)),
            Self::Sr => Some((sr_functions::DESCRIPTIONS, sr_functions::ARGUMENTS)),
            Self::Tr => Some((tr_functions::DESCRIPTIONS, tr_functions::ARGUMENTS)),
            Self::Uk => Some((uk_functions::DESCRIPTIONS, uk_functions::ARGUMENTS)),
            Self::Vi => Some((vi_functions::DESCRIPTIONS, vi_functions::ARGUMENTS)),
            Self::Zh => Some((zh_functions::DESCRIPTIONS, zh_functions::ARGUMENTS)),
            Self::ZhTw => Some((zh_tw_functions::DESCRIPTIONS, zh_tw_functions::ARGUMENTS)),
            Self::Ru => Some((ru_functions::DESCRIPTIONS, ru_functions::ARGUMENTS)),
            Self::En | Self::Da | Self::Sv | Self::Ka | Self::Lt => None,
        }
    }
}

/// English text and its translation, sorted by the English.
type HelpTable = &'static [(&'static str, &'static str)];

/// The translation of `english` in a help table (binary search, so the table must stay sorted).
fn lookup(table: HelpTable, english: &str) -> Option<&'static str> {
    let i = table.binary_search_by(|(key, _)| (*key).cmp(english)).ok()?;
    table.get(i).map(|(_, translated)| *translated)
}

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

    /// Descriptions must not collapse distinct operations into the same translated operation.
    #[test]
    fn translated_help_keeps_distinct_math_and_statistics() {
        let pairs = [
            ("Arccosine in radians.", "Arcsine in radians."),
            ("Arccotangent in radians (0 to pi).", "Arctangent in radians."),
            ("Rounds a number down to a multiple of significance.", "Rounds a number up to a multiple of significance."),
            ("Rounds a number down to the nearest integer or multiple.", "Rounds a number up to the nearest integer or multiple."),
            ("Rounds a number toward negative infinity to a multiple.", "Rounds a number toward positive infinity to a multiple."),
            ("Rounds a number away from zero to a given number of digits.", "Rounds a number toward zero to a given number of digits."),
            ("Sample variance.", "Population variance."),
            ("Sample standard deviation.", "Population standard deviation."),
            ("Sample skewness.", "Population skewness."),
        ];
        for language in Language::ALL {
            for (first, second) in pairs {
                if let (Some(a), Some(b)) = (language.function_description(first), language.function_description(second)) {
                    assert_ne!(a, b, "{language:?}: distinct operations collapsed: {first} / {second}");
                }
            }
        }
    }

    /// Programmatic identifiers inside descriptions are mathematical notation, not words to translate.
    #[test]
    fn translated_help_preserves_mathematical_identifiers() {
        let examples = [
            ("Bessel function of the first kind, J_n(x).", "J_n(x)"),
            ("Bessel function of the second kind, Y_n(x).", "Y_n(x)"),
            ("Modified Bessel function of the first kind, I_n(x).", "I_n(x)"),
            ("Modified Bessel function of the second kind, K_n(x).", "K_n(x)"),
            ("Complementary error function, 1 - ERF(x).", "ERF(x)"),
            ("Extracts data from XML with an XPath (not available; returns #VALUE!).", "XPath"),
        ];
        for language in Language::ALL {
            for (english, identifier) in examples {
                if let Some(translated) = language.function_description(english) {
                    assert!(translated.contains(identifier), "{language:?}: missing {identifier} in {translated}");
                }
            }
        }
    }

    /// Help tables are sorted and unique: lookups binary-search them.
    #[test]
    fn function_help_tables_are_sorted_and_unique() {
        for language in Language::ALL {
            let Some((description_table, argument_table)) = language.function_help() else { continue };
            for table in [description_table, argument_table] {
                assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0), "{language:?} function help is not sorted and unique");
                for (english, translated) in table {
                    assert!(!translated.is_empty(), "{language:?}: empty translation of {english:?}");
                    assert!(!translated.contains(['\n', '\r']), "{language:?}: multiline translation of {english:?}");
                    assert!(
                        !translated.contains(['\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}']),
                        "{language:?}: invisible formatting in {english:?}"
                    );
                }
            }
        }
    }

    /// Dynamic UI messages must use the same placeholder names as their callers.
    #[test]
    fn dynamic_ui_messages_resolve_and_substitute_their_placeholders() {
        let key = "{count} engine commands are available. See docs/mcp.md and docs/control-protocol.md.";
        for (language, expected) in [
            (Language::Pl, "Dostępne polecenia silnika: 368. Zobacz docs/mcp.md i docs/control-protocol.md."),
            (Language::ZhTw, "當前可用 368 個引擎命令。參見 docs/mcp.md 與 docs/control-protocol.md。"),
        ] {
            assert_eq!(craft_i18n::fmt(language.tr(key), &[("count", "368")]), expected);
        }
        assert_eq!(
            craft_i18n::fmt(Language::Pl.tr("{contributors} contributors · {commits} commits"), &[("contributors", "16"), ("commits", "148")]),
            "Współtwórcy: 16 · commity: 148"
        );
    }

    /// AppKit's system dialogs should recognize every language offered by the interface.
    #[test]
    fn macos_bundle_declares_every_interface_language() {
        let plist = include_str!("../../../packaging/macos/Info.plist.in");
        let array = plist.split_once("<key>CFBundleLocalizations</key>").unwrap().1.split_once("</array>").unwrap().0;
        let tags: Vec<&str> = array.split("<string>").skip(1).map(|s| s.split_once("</string>").unwrap().0).collect();
        assert_eq!(tags.len(), Language::ALL.len());
        for language in Language::ALL {
            let tag = match language {
                Language::PtBr => "pt-BR",
                Language::Zh => "zh-Hans",
                Language::ZhTw => "zh-Hant",
                _ => language.code(),
            };
            assert!(tags.contains(&tag), "{language:?} is missing from the macOS bundle");
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
    fn paste_special_add_and_text_to_columns_comma_have_their_own_keys() {
        // "Add" is also the Solver button and "Comma" also the cell style (Spanish Excel: Sumar
        // vs Agregar, Coma vs Millares), so the two dialog options are keyed by context. English
        // shows the plain label, and a catalog that translates the plain label translates the
        // keyed one too, so the dialog option doesn't fall back to English.
        for (keyed, plain) in [("Paste Special|Add", "Add"), ("Text to Columns|Comma", "Comma")] {
            assert_eq!(Language::En.tr(keyed), plain);
            for language in Language::ALL {
                let shown = language.tr(keyed);
                assert!(!shown.contains('|'), "{} shows a context prefix for {keyed}: {shown}", language.code());
                if language.tr(plain) != plain {
                    assert_ne!(shown, plain, "{} translates {plain:?} but not {keyed:?}", language.code());
                }
            }
        }
    }

    /// Generated names should follow the interface language even in smaller catalogs.
    #[test]
    fn generated_names_have_translations_in_every_catalog() {
        for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
            for key in ["Book", "Sheet", "Generated|Column", "Generated|Row", "Series{n}"] {
                let fallback = Language::En.tr(key);
                assert_ne!(language.tr(key), fallback, "{language:?}: {key}");
            }
            assert_eq!(language.tr("Series{n}").matches("{n}").count(), 1, "{language:?}");
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
        assert_eq!(Language::from_tag("zh-Hant-TW"), Some(Language::ZhTw));
        assert_eq!(Language::from_tag("zh_HK.UTF-8"), Some(Language::ZhTw));
        // A script subtag outranks the region: Hong Kong and Macau are also valid macOS locales
        // for Simplified Chinese.
        assert_eq!(Language::from_tag("zh-Hans-HK"), Some(Language::Zh));
        assert_eq!(Language::from_tag("zh-Hans-MO"), Some(Language::Zh));
        assert_eq!(Language::from_tag("zh-Hant-HK"), Some(Language::ZhTw));
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
}

#[cfg(test)]
mod russian_tests {
    use super::*;
    /// Unknown parameter errors retain the engine's command context without affecting translated details.
    #[test]
    fn unmatched_parameter_errors_keep_the_command_context() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let error = session.execute("data.showLevels", serde_json::json!({})).unwrap_err().to_string();
        assert!(error.starts_with("invalid parameters for `data.showLevels`: "), "{error}");
        for language in [Language::En, Language::Ru] {
            assert_eq!(language.tr_error(&error), error);
            assert_eq!(language.tr_error("command `data.showLevels`: Untranslated outline failure {value}"), "Untranslated outline failure {value}");
        }
        assert_eq!(Language::Ru.tr_error("invalid parameters for `chart`: Select a chart first."), Language::Ru.tr("Select a chart first."));
        assert_eq!(Language::Ru.tr_error("invalid parameters for `cell.set`: missing `cell`"), "Не указан параметр `cell`");
    }

    /// Checks Russian diagnostics remove command wrappers while preserving captured names and unknown messages.
    #[test]
    fn russian_error_messages_preserve_data_and_clean_command_wrappers() {
        let ru = Language::Ru;
        let syntax = ru.tr_error("There's a problem with this formula: expected `,` or `)`, found Semicolon");
        assert!(syntax.starts_with("Ошибка в формуле:"));
        assert!(!syntax.contains("Semicolon"));
        assert!(syntax.contains(ru.tr("Semicolon")));
        assert_eq!(ru.tr_error("command `chart` is not available right now: Select a chart first."), ru.tr("Select a chart first."));
        assert_eq!(
            ru.tr_error("invalid parameters for `cell.set`: invalid cell reference `Лист{key}` for `cell`"),
            "Неверная ссылка на ячейку `Лист{key}` в параметре `cell`"
        );
        assert_eq!(ru.tr_error("unexpected end of formula"), ru.tr("unexpected end of formula"));
        assert_eq!(ru.tr_tab("Insert"), ru.tr("Tab|Insert"));
        assert_eq!(ru.tr_error("Sheet|Unknown failure"), "Sheet|Unknown failure");
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let error = session.execute("cell.set", serde_json::json!({"cell":"A1","input":"=SUM()"})).unwrap_err().to_string();
        let translated = ru.tr_error(&error);
        assert!(translated.contains(ru.formula_locale().function_name("SUM")), "{translated}");
        assert!(translated.contains(ru.function_argument("number").unwrap()), "{translated}");
        let error = session.execute("cell.set", serde_json::json!({"cell":"Лист{key}","input":"1"})).unwrap_err().to_string();
        let translated = ru.tr_error(&error);
        assert!(translated.contains("Лист{key}"));
        assert_ne!(translated, error);
    }
}
