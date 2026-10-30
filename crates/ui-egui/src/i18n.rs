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
mod fr_functions;
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
        use gridcraft_engine::formula::FormulaLocale;
        match self {
            Self::Es => FormulaLocale::Es,
            Self::Fr => FormulaLocale::Fr,
            _ => FormulaLocale::En,
        }
    }

    /// Translates an interface label; untranslated labels fall back to English.
    ///
    /// A label that needs two translations (Home's text "Orientation" vs Page Layout's print
    /// "Orientation") is keyed `context|label`: English shows only the part after the `|`, and a
    /// catalog without the context key uses its plain `label` translation.
    pub fn tr(self, text: &str) -> &str {
        let label = text.rsplit_once('|').map_or(text, |(_, label)| label);
        self.lookup(text).or_else(|| self.lookup(label)).unwrap_or(label)
    }

    fn lookup(self, text: &str) -> Option<&str> {
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

    pub(crate) fn tr_error(self, message: &str) -> String {
        let message = if message.starts_with("command `") { crate::clean_error(message) } else { message.to_owned() };

        // Exact lookup only: `tr`'s `context|label` fallback would swap a runtime message holding `|` for a label.
        if let Some(translated) = self.lookup(&message) {
            return translated.to_owned();
        }
        // Messages built with `format!`: the template with the most literal text wins.
        self.error_templates()
            .iter()
            .filter_map(|(english, local)| Some((literal_len(english), fill_template(english, &message, local)?)))
            .max_by_key(|(len, _)| *len)
            .map_or(message, |(_, filled)| filled)
    }

    /// The templates of engine messages built with `format!`, for the languages that have them.
    fn error_templates(self) -> HelpTable {
        match self {
            Self::Fr => fr::ERROR_TEMPLATES,
            _ => &[],
        }
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
            Self::Fr => Some((fr_functions::DESCRIPTIONS, fr_functions::ARGUMENTS)),
            _ => None,
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

/// A `{name}` template split into its literal texts and placeholder names; there is always one
/// more literal than names (empty at the ends when a placeholder starts or ends the template).
fn template_parts(template: &str) -> Option<(Vec<&str>, Vec<&str>)> {
    let (mut literals, mut names) = (Vec::new(), Vec::new());
    let mut rest = template;
    while let Some((literal, tail)) = rest.split_once('{') {
        let (name, tail) = tail.split_once('}')?;
        literals.push(literal);
        names.push(name);
        rest = tail;
    }
    literals.push(rest);
    Some((literals, names))
}

/// How much literal text a template holds: a match on more of it is a closer match.
fn literal_len(template: &str) -> usize {
    template_parts(template).map_or(0, |(literals, _)| literals.iter().map(|l| l.chars().count()).sum())
}

/// `translated` with its placeholders filled from `message`, if `message` matches `template`.
/// Each placeholder captures the text up to the next literal; captures stay untranslated.
fn fill_template(template: &str, message: &str, translated: &str) -> Option<String> {
    let (literals, names) = template_parts(template)?;
    let (first, rest) = literals.split_first()?;
    let (last, middle) = rest.split_last()?;
    let mut body = message.strip_prefix(first)?.strip_suffix(last)?;
    let mut captures = Vec::new();
    for literal in middle {
        let (capture, tail) = body.split_once(literal)?;
        captures.push(capture);
        body = tail;
    }
    captures.push(body);
    // One pass over `translated`, so a capture that contains `{…}` is never substituted again.
    let (pieces, slots) = template_parts(translated)?;
    let mut out = String::new();
    for (piece, slot) in pieces.iter().zip(slots.iter().map(Some).chain([None])) {
        out.push_str(piece);
        if let Some(slot) = slot {
            out.push_str(names.iter().position(|n| n == slot).and_then(|i| captures.get(i))?);
        }
    }
    Some(out)
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

    /// A context key wins where the catalog has it, else falls back to the plain label's translation.
    #[test]
    fn context_keys_fall_back_to_the_plain_label() {
        assert_eq!(Language::Fr.tr("border|Outline"), "Contour");
        assert_eq!(Language::Fr.tr("Outline"), "Plan");
        assert_eq!(Language::Es.tr("delimiter|Comma"), Language::Es.tr("Comma"));
        assert_eq!(Language::En.tr("delimiter|Comma"), "Comma");
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
    fn placeholders(s: &str) -> Vec<String> {
        s.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string())).collect()
    }

    #[test]
    fn translations_keep_the_shape_of_their_english() {
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

    #[test]
    fn error_templates_keep_their_placeholders() {
        for language in Language::ALL {
            let table = language.error_templates();
            for (i, (english, translated)) in table.iter().enumerate() {
                let mut names = placeholders(english);
                assert!(!names.is_empty(), "{language:?}: no placeholder in {english:?}");
                assert!(!english.contains("}{"), "{language:?}: adjacent placeholders in {english:?}");
                assert!(literal_len(english) >= 8, "{language:?}: too little literal text in {english:?}");
                let mut local = placeholders(translated);
                names.sort();
                local.sort();
                assert_eq!(names, local, "{language:?}: {english:?} => {translated:?}");
                assert!(table.iter().skip(i + 1).all(|(other, _)| other != english), "{language:?}: duplicate {english:?}");
            }
        }
    }

    #[test]
    fn formatted_engine_messages_translate_by_template() {
        // As shown: `clean_error` drops `invalid parameters for `cmd`: ` before the dialog translates.
        let shown = crate::clean_error("invalid parameters for `data.scenarioShow`: no scenario named `Best case`");
        assert_eq!(Language::Fr.tr_error(&shown), "aucun scénario nommé « Best case »");
        assert_eq!(Language::Fr.tr_error("command `x`: no sheet named `Q1`"), "aucune feuille nommée « Q1 »");
        assert_eq!(Language::En.tr_error(&shown), shown);
        assert_eq!(Language::Fr.tr_error("Printer Office: permission denied"), "Printer Office: permission denied");
        assert_eq!(Language::Fr.tr_error("no sheet named ``"), "aucune feuille nommée «  »", "an empty capture still matches");
        // A runtime message holding `|` is never swapped for the label after it.
        assert_eq!(Language::Fr.tr_error("could not read draft|Outline"), "could not read draft|Outline");
        // File import warnings, plain and built with `format!`.
        assert_eq!(
            Language::Fr.tr_error("macros (VBA project) were kept but not executed"),
            "les macros (projet VBA) ont été conservées mais pas exécutées"
        );
        assert_eq!(
            Language::Fr.tr_error("sheet \"Test\": embedded objects and form controls are not shown; they are kept for saving"),
            "feuille « Test » : les objets incorporés et les contrôles de formulaire ne sont pas affichés ; ils sont conservés à l'enregistrement"
        );
        // The template with the most literal text wins over a looser one that also matches.
        assert_eq!(Language::Fr.tr_error("`Region` isn't in the values area"), "« Region » ne figure pas dans la zone Valeurs");
        assert_eq!(Language::Fr.tr_error("Too many constraints (at most 100 cells)."), "Trop de contraintes (100 cellules au maximum).");
        assert_eq!(Language::Fr.tr_error("Too many constraints (at most 100)."), "Trop de contraintes (100 au maximum).");
        // Formula syntax errors, as the French parser reports them.
        assert_eq!(Language::Fr.tr_error("unexpected `;`"), "`;` inattendu");
        assert_eq!(Language::Fr.tr_error("unexpected end of formula"), "Fin de formule inattendue.");
        assert_eq!(Language::Fr.tr_error("expected `)`, found end of formula"), "`)` attendu avant la fin de la formule");
    }

    /// Real engine errors stay matched by the templates, so a reworded engine message shows up here.
    #[test]
    fn real_engine_errors_translate_to_french() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        for (id, params) in [
            ("cell.set", serde_json::json!({"cell": "A1", "input": "=SUM()"})),
            ("sheet.activate", serde_json::json!({"sheet": "Nope"})),
            ("cell.set", serde_json::json!({"cell": "A1", "input": "=ABS(1,2)"})),
            ("home.cellStyle", serde_json::json!({"name": "Nope"})),
            ("pageLayout.theme", serde_json::json!({"name": "Nope"})),
            ("data.scenarioShow", serde_json::json!({"name": "Nope"})),
            ("formulas.selectPrecedents", serde_json::json!({"cell": "B9"})),
        ] {
            let err = crate::clean_error(&session.execute(id, params).unwrap_err().to_string());
            assert_ne!(Language::Fr.tr_error(&err), err, "{id}: {err}");
        }
    }

    /// Open-time notices are dynamic text: a `|` in a file name is not a context separator.
    #[test]
    fn notice_text_keeps_literal_pipes() {
        let warning = "could not read draft|final.json";
        for lang in [Language::En, Language::Fr, Language::Es] {
            assert_eq!(lang.tr_error(warning), warning, "{lang:?}");
        }
        let vba = "This workbook contains VBA macros. GridCraft keeps them when you save as .xlsm, but does not run them.";
        assert_ne!(Language::Fr.tr_error(vba), vba);
    }

    #[test]
    fn fixed_engine_errors_translate_to_french() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        for (id, params, french) in [
            ("draw.inkToShape", serde_json::json!({}), "Aucun trait d'encre à convertir."),
            ("view.zoom", serde_json::json!({}), "`percent` manquant."),
            ("edit.undo", serde_json::json!({}), "Rien à annuler."),
        ] {
            let err = crate::clean_error(&session.execute(id, params).unwrap_err().to_string());
            assert_eq!(Language::Fr.tr_error(&err), french, "{id}: {err}");
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
}
