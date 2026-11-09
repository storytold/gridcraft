//! BCP-47 language tags: parsing, normalisation, aliases and pair matching.
//!
//! Tags are stored normalised (language lower-case, script title-case, region upper-case,
//! variants lower-case) so `en-us`, `EN-US` and `en_us` are one value. Deprecated and
//! bibliographic codes map to their modern form (`iw` → `he`, `zho` → `zh`). Parsing accepts an
//! extension or private-use tail but ignores it for translation, which only needs the language,
//! script and region.

use std::fmt;

/// A parsed, normalised BCP-47 tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LangTag {
    language: String,
    script: Option<String>,
    region: Option<String>,
    variants: Vec<String>,
}

/// Why a language tag was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    #[error("a language tag is empty")]
    Empty,
    #[error("a language tag is longer than 64 bytes")]
    TooLong,
    #[error("a language subtag is not a language")]
    BadLanguage,
    #[error("a subtag is not valid for a language tag")]
    BadSubtag,
    #[error("a language tag has more than one script or region")]
    Duplicate,
    #[error("a language tag has too many subtags")]
    TooMany,
    #[error("private-use and extension subtags are not translation languages")]
    NotALanguage,
}

/// Deprecated, legacy and bibliographic language subtags, as RFC 5646 grandfathered/redundant
/// mappings and the ISO 639-2/B codes people still type.
const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("iw", "he"),
    ("in", "id"),
    ("ji", "yi"),
    ("jw", "jv"),
    ("mo", "ro"),
    ("sh", "sr-Latn"),
    ("tl", "fil"),
    ("no", "nb"),
    ("nob", "nb"),
    ("nno", "nn"),
    ("zho", "zh"),
    ("chi", "zh"),
    ("deu", "de"),
    ("ger", "de"),
    ("fra", "fr"),
    ("fre", "fr"),
    ("spa", "es"),
    ("rus", "ru"),
    ("jpn", "ja"),
    ("kor", "ko"),
    ("por", "pt"),
    ("ita", "it"),
    ("nld", "nl"),
    ("dut", "nl"),
    ("pol", "pl"),
    ("tur", "tr"),
    ("ara", "ar"),
    ("hin", "hi"),
    ("ukr", "uk"),
    ("vie", "vi"),
    ("tha", "th"),
    ("ind", "id"),
    ("heb", "he"),
    ("ell", "el"),
    ("gre", "el"),
    ("ces", "cs"),
    ("cze", "cs"),
    ("dan", "da"),
    ("swe", "sv"),
    ("fin", "fi"),
    ("nor", "nb"),
    ("fil", "fil"),
    ("ben", "bn"),
    ("tam", "ta"),
    ("tel", "te"),
    ("mal", "ml"),
    ("urd", "ur"),
    ("fas", "fa"),
    ("per", "fa"),
    ("msa", "ms"),
    ("may", "ms"),
    ("ron", "ro"),
    ("rum", "ro"),
    ("srp", "sr"),
    ("hrv", "hr"),
    ("bos", "bs"),
    ("slk", "sk"),
    ("slo", "sk"),
    ("slv", "sl"),
    ("bul", "bg"),
    ("cat", "ca"),
    ("eus", "eu"),
    ("baq", "eu"),
    ("glg", "gl"),
    ("hun", "hu"),
    ("est", "et"),
    ("lav", "lv"),
    ("lit", "lt"),
    ("isl", "is"),
    ("ice", "is"),
    ("cym", "cy"),
    ("wel", "cy"),
    ("gle", "ga"),
    ("iri", "ga"),
    ("ben", "bn"),
    ("mar", "mr"),
    ("guj", "gu"),
    ("pan", "pa"),
    ("kan", "kn"),
    ("mya", "my"),
    ("bur", "my"),
    ("khm", "km"),
    ("lao", "lo"),
    ("nep", "ne"),
    ("sin", "si"),
    ("som", "so"),
    ("swa", "sw"),
    ("yor", "yo"),
    ("ibo", "ig"),
    ("hau", "ha"),
    ("zul", "zu"),
    ("afr", "af"),
    ("sqi", "sq"),
    ("alb", "sq"),
    ("mkd", "mk"),
    ("mac", "mk"),
    ("kat", "ka"),
    ("geo", "ka"),
    ("hye", "hy"),
    ("arm", "hy"),
    ("aze", "az"),
    ("kaz", "kk"),
    ("uzb", "uz"),
    ("tat", "tt"),
    ("mon", "mn"),
];

impl LangTag {
    /// Parses a tag, normalising case and following the common aliases. `en`, `en-US`,
    /// `en_us`, `EN-us`, `pt-BR` and `zh-Hans` all parse; an empty string does not.
    pub fn parse(input: &str) -> Result<LangTag, TagError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(TagError::Empty);
        }
        if text.len() > 64 {
            return Err(TagError::TooLong);
        }
        let mut parts = text.split(['-', '_']).filter(|p| !p.is_empty());
        let Some(first) = parts.next() else { return Err(TagError::Empty) };
        if !first.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(TagError::BadLanguage);
        }
        let (language, mut inferred_script) = normalise_language(first)?;
        let mut script: Option<String> = None;
        let mut region: Option<String> = None;
        let mut variants: Vec<String> = Vec::new();
        let mut subtags = 1usize;
        let mut extension = false;
        for part in parts {
            subtags += 1;
            if subtags > 8 {
                return Err(TagError::TooMany);
            }
            if extension {
                // Extension and private-use tails are well-formed but irrelevant here.
                if part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err(TagError::BadSubtag);
                }
                continue;
            }
            if part.len() == 1 {
                // A singleton introduces an extension or the private-use tail.
                extension = true;
                continue;
            }
            if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                if script.is_some() {
                    return Err(TagError::Duplicate);
                }
                script = Some(title_case(part));
                continue;
            }
            if (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic())) || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())) {
                if region.is_some() {
                    return Err(TagError::Duplicate);
                }
                region = Some(part.to_ascii_uppercase());
                continue;
            }
            if (5..=8).contains(&part.len()) || (part.len() == 4 && part.chars().next().is_some_and(|c| c.is_ascii_digit())) {
                if !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err(TagError::BadSubtag);
                }
                variants.push(part.to_ascii_lowercase());
                continue;
            }
            return Err(TagError::BadSubtag);
        }
        // `zh-CN` and `zh-TW` mean Simplified and Traditional Chinese in practice; making the
        // script explicit keeps a pair match from confusing them.
        if language == "zh" && script.is_none() {
            match region.as_deref() {
                Some("CN" | "SG" | "MY") => inferred_script = Some("Hans".to_string()),
                Some("TW" | "HK" | "MO") => inferred_script = Some("Hant".to_string()),
                _ => {}
            }
        }
        if script.is_none() {
            script = inferred_script;
        }
        Ok(LangTag { language, script, region, variants })
    }

    /// The detector's "no language" value (ISO 639-2 `und`). Not a translation target.
    pub(crate) fn undetermined() -> LangTag {
        LangTag { language: "und".to_string(), script: None, region: None, variants: Vec::new() }
    }

    /// Parses `auto` (any case) as `None`, an explicit tag as `Some`.
    pub fn parse_auto(input: &str) -> Result<Option<LangTag>, TagError> {
        if input.trim().eq_ignore_ascii_case(crate::AUTO) { Ok(None) } else { LangTag::parse(input).map(Some) }
    }

    /// The primary language subtag (`en` for `en-US`).
    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn script(&self) -> Option<&str> {
        self.script.as_deref()
    }

    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// The normalised tag: `en-US`, `zh-Hans`, `sr-Latn-RS`.
    pub fn canonical(&self) -> String {
        let mut out = self.language.clone();
        if let Some(s) = &self.script {
            out.push('-');
            out.push_str(s);
        }
        if let Some(r) = &self.region {
            out.push('-');
            out.push_str(r);
        }
        for v in &self.variants {
            out.push('-');
            out.push_str(v);
        }
        out
    }

    /// Whether this tag is the same language as a model's tag, ignoring an unspecified or equal
    /// script. Regions do not matter: `en-GB` and `en-US` are both the `en` side of a pair.
    pub fn matches(&self, other: &LangTag) -> bool {
        if self.language != other.language {
            return false;
        }
        match (&self.script, &other.script) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => true,
        }
    }

    /// The English name of the language, for a pane's menu. Unknown codes fall back to the
    /// canonical tag.
    pub fn english_name(&self) -> &'static str {
        language_name(&self.language).unwrap_or("Unknown")
    }
}

impl fmt::Display for LangTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

impl std::str::FromStr for LangTag {
    type Err = TagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LangTag::parse(s)
    }
}

impl serde::Serialize for LangTag {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.canonical())
    }
}

fn title_case(part: &str) -> String {
    let mut chars = part.chars();
    let mut out = String::with_capacity(part.len());
    if let Some(c) = chars.next() {
        out.extend(c.to_uppercase());
    }
    out.extend(chars.flat_map(|c| c.to_lowercase()));
    out
}

fn normalise_language(first: &str) -> Result<(String, Option<String>), TagError> {
    let lower = first.to_ascii_lowercase();
    if let Some((_, replacement)) = LANGUAGE_ALIASES.iter().find(|(from, _)| *from == lower) {
        let mut pieces = replacement.split('-');
        let language = pieces.next().unwrap_or(&lower).to_string();
        let script = pieces.next().map(title_case);
        return Ok((language, script));
    }
    if !(2..=8).contains(&lower.len()) {
        return Err(TagError::BadLanguage);
    }
    Ok((lower, None))
}

fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "af" => "Afrikaans",
        "ar" => "Arabic",
        "az" => "Azerbaijani",
        "be" => "Belarusian",
        "bg" => "Bulgarian",
        "bn" => "Bengali",
        "bs" => "Bosnian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "cy" => "Welsh",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "eu" => "Basque",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fil" => "Filipino",
        "fr" => "French",
        "ga" => "Irish",
        "gl" => "Galician",
        "gu" => "Gujarati",
        "ha" => "Hausa",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "hy" => "Armenian",
        "id" => "Indonesian",
        "ig" => "Igbo",
        "is" => "Icelandic",
        "it" => "Italian",
        "ja" => "Japanese",
        "ka" => "Georgian",
        "kk" => "Kazakh",
        "km" => "Khmer",
        "kn" => "Kannada",
        "ko" => "Korean",
        "lo" => "Lao",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "mk" => "Macedonian",
        "ml" => "Malayalam",
        "mn" => "Mongolian",
        "mr" => "Marathi",
        "ms" => "Malay",
        "my" => "Burmese",
        "nb" => "Norwegian Bokmål",
        "ne" => "Nepali",
        "nl" => "Dutch",
        "nn" => "Norwegian Nynorsk",
        "pa" => "Punjabi",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "si" => "Sinhala",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "so" => "Somali",
        "sq" => "Albanian",
        "sr" => "Serbian",
        "sv" => "Swedish",
        "sw" => "Swahili",
        "ta" => "Tamil",
        "te" => "Telugu",
        "th" => "Thai",
        "tr" => "Turkish",
        "tt" => "Tatar",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "uz" => "Uzbek",
        "vi" => "Vietnamese",
        "yi" => "Yiddish",
        "yo" => "Yoruba",
        "zh" => "Chinese",
        "zu" => "Zulu",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalises_common_tags() {
        assert_eq!(LangTag::parse("en").map(|t| t.canonical()), Ok("en".into()));
        assert_eq!(LangTag::parse("EN-us").map(|t| t.canonical()), Ok("en-US".into()));
        assert_eq!(LangTag::parse("pt_br").map(|t| t.canonical()), Ok("pt-BR".into()));
        assert_eq!(LangTag::parse("zh-hans").map(|t| t.canonical()), Ok("zh-Hans".into()));
        assert_eq!(LangTag::parse("sr-latn-rs").map(|t| t.canonical()), Ok("sr-Latn-RS".into()));
    }

    #[test]
    fn aliases_map_to_modern_codes() {
        assert_eq!(LangTag::parse("iw").map(|t| t.canonical()), Ok("he".into()));
        assert_eq!(LangTag::parse("in").map(|t| t.canonical()), Ok("id".into()));
        assert_eq!(LangTag::parse("zho").map(|t| t.canonical()), Ok("zh".into()));
        assert_eq!(LangTag::parse("sh").map(|t| t.canonical()), Ok("sr-Latn".into()));
        assert_eq!(LangTag::parse("no").map(|t| t.canonical()), Ok("nb".into()));
    }

    #[test]
    fn chinese_regions_get_a_script() {
        assert_eq!(LangTag::parse("zh-CN").map(|t| t.canonical()), Ok("zh-Hans-CN".into()));
        assert_eq!(LangTag::parse("zh-TW").map(|t| t.canonical()), Ok("zh-Hant-TW".into()));
        assert_eq!(LangTag::parse("zh-Hans").map(|t| t.canonical()), Ok("zh-Hans".into()));
    }

    #[test]
    fn auto_is_not_a_tag() {
        assert_eq!(LangTag::parse_auto("AUTO"), Ok(None));
        assert!(LangTag::parse_auto("en").is_ok_and(|t| t.is_some()));
        assert!(LangTag::parse_auto("").is_err());
    }

    #[test]
    fn malformed_tags_are_refused() {
        assert!(LangTag::parse("").is_err());
        assert!(LangTag::parse("e").is_err());
        assert!(LangTag::parse("en-").is_ok(), "an empty subtag is skipped, as browsers do");
        assert!(LangTag::parse("en-US-GB").is_err());
        assert!(LangTag::parse("en--US").is_ok(), "empty subtags are skipped");
        assert!(LangTag::parse("en-US-abcdefghi").is_err());
        assert!(LangTag::parse(&"a".repeat(70)).is_err());
    }

    #[test]
    fn extension_tails_are_ignored() {
        let t = LangTag::parse("en-US-u-ca-gregory").expect("tag");
        assert_eq!(t.canonical(), "en-US");
    }

    #[test]
    fn pairs_match_on_language_and_script() {
        let en = LangTag::parse("en-GB").expect("en");
        let es = LangTag::parse("es").expect("es");
        assert!(en.matches(&LangTag::parse("en-US").expect("en")));
        assert!(!en.matches(&es));
        let hans = LangTag::parse("zh-Hans").expect("zh");
        let hant = LangTag::parse("zh-Hant").expect("zh");
        assert!(hans.matches(&LangTag::parse("zh-CN").expect("zh")));
        assert!(!hans.matches(&hant));
    }

    #[test]
    fn names_cover_common_languages() {
        assert_eq!(LangTag::parse("es").map(|t| t.english_name()), Ok("Spanish"));
        assert_eq!(LangTag::parse("qq").map(|t| t.english_name()), Ok("Unknown"));
    }
}
