//! A pure, offline language detector: Unicode script first, then stopword and diacritic scoring
//! for languages that share the Latin alphabet.
//!
//! It is deliberately conservative. A short or ambiguous string comes back with a low confidence
//! so the pane can ask for a source language instead of guessing. No model, no network, no clock.

use serde::Serialize;

use crate::tag::LangTag;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DetectionMethod {
    Script,
    Stopwords,
    ScriptAndStopwords,
    None,
}

/// The detector's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub language: LangTag,
    pub confidence: f32,
    pub method: DetectionMethod,
    pub runner_up: Option<(LangTag, f32)>,
}

impl Detection {
    /// Whether the answer is sure enough to translate without asking the user. The default
    /// threshold is 0.5.
    pub fn is_confident(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Cyrillic,
    Arabic,
    Hebrew,
    Greek,
    Han,
    Hiragana,
    Katakana,
    Hangul,
    Devanagari,
    Bengali,
    Tamil,
    Telugu,
    Malayalam,
    Gujarati,
    Gurmukhi,
    Kannada,
    Sinhala,
    Thai,
    Myanmar,
    Khmer,
    Lao,
    Georgian,
    Armenian,
    Ethiopic,
    Other,
}

fn script_of(c: char) -> Script {
    let u = c as u32;
    match u {
        0x0041..=0x024F | 0x1E00..=0x1EFF => Script::Latin,
        0x0400..=0x052F => Script::Cyrillic,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0370..=0x03FF => Script::Greek,
        0x3040..=0x309F => Script::Hiragana,
        0x30A0..=0x30FF => Script::Katakana,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => Script::Han,
        0xAC00..=0xD7AF | 0x1100..=0x11FF => Script::Hangul,
        0x0900..=0x097F => Script::Devanagari,
        0x0980..=0x09FF => Script::Bengali,
        0x0B80..=0x0BFF => Script::Tamil,
        0x0C00..=0x0C7F => Script::Telugu,
        0x0D00..=0x0D7F => Script::Malayalam,
        0x0A80..=0x0AFF => Script::Gujarati,
        0x0A00..=0x0A7F => Script::Gurmukhi,
        0x0C80..=0x0CFF => Script::Kannada,
        0x0D80..=0x0DFF => Script::Sinhala,
        0x0E00..=0x0E7F => Script::Thai,
        0x1000..=0x109F | 0xA9E0..=0xA9FF => Script::Myanmar,
        0x1780..=0x17FF => Script::Khmer,
        0x0E80..=0x0EFF => Script::Lao,
        0x10A0..=0x10FF => Script::Georgian,
        0x0530..=0x058F => Script::Armenian,
        0x1200..=0x137F => Script::Ethiopic,
        _ => Script::Other,
    }
}

fn tag(code: &str) -> LangTag {
    match LangTag::parse(code) {
        Ok(t) => t,
        // Every code used here is a valid language subtag; `und` is the safe fallback.
        Err(_) => LangTag::undetermined(),
    }
}

fn und() -> LangTag {
    LangTag::undetermined()
}

/// Detects the language of `text`.
pub fn detect(text: &str) -> Detection {
    let mut counts: Vec<(Script, usize)> = Vec::new();
    let mut letters = 0usize;
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        let s = script_of(c);
        if let Some(entry) = counts.iter_mut().find(|(x, _)| *x == s) {
            entry.1 += 1;
        } else {
            counts.push((s, 1));
        }
    }
    if letters == 0 {
        return Detection { language: und(), confidence: 0.0, method: DetectionMethod::None, runner_up: None };
    }
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let dominant = counts.first().map(|(s, _)| *s).unwrap_or(Script::Other);
    let share = counts.first().map(|(_, n)| *n as f32 / letters as f32).unwrap_or(0.0);
    let (has_kana, has_han) =
        (counts.iter().any(|(s, _)| matches!(s, Script::Hiragana | Script::Katakana)), counts.iter().any(|(s, _)| *s == Script::Han));
    let scripted = match dominant {
        Script::Hiragana | Script::Katakana => Some(("ja", 0.68 + 0.3 * share)),
        Script::Hangul => Some(("ko", 0.7 + 0.28 * share)),
        Script::Hebrew => Some(("he", 0.7 + 0.28 * share)),
        Script::Greek => Some(("el", 0.7 + 0.28 * share)),
        Script::Thai => Some(("th", 0.7 + 0.28 * share)),
        Script::Devanagari => Some(("hi", 0.7 + 0.28 * share)),
        Script::Bengali => Some(("bn", 0.7 + 0.28 * share)),
        Script::Tamil => Some(("ta", 0.7 + 0.28 * share)),
        Script::Telugu => Some(("te", 0.7 + 0.28 * share)),
        Script::Malayalam => Some(("ml", 0.7 + 0.28 * share)),
        Script::Gujarati => Some(("gu", 0.7 + 0.28 * share)),
        Script::Gurmukhi => Some(("pa", 0.7 + 0.28 * share)),
        Script::Kannada => Some(("kn", 0.7 + 0.28 * share)),
        Script::Sinhala => Some(("si", 0.7 + 0.28 * share)),
        Script::Myanmar => Some(("my", 0.7 + 0.28 * share)),
        Script::Khmer => Some(("km", 0.7 + 0.28 * share)),
        Script::Lao => Some(("lo", 0.7 + 0.28 * share)),
        Script::Georgian => Some(("ka", 0.7 + 0.28 * share)),
        Script::Armenian => Some(("hy", 0.7 + 0.28 * share)),
        Script::Ethiopic => Some(("am", 0.7 + 0.28 * share)),
        Script::Han => {
            if has_kana {
                Some(("ja", 0.75))
            } else if chinese_characters(text) >= 2 {
                Some(("zh", 0.72 + 0.2 * share))
            } else {
                // Kanji-only Japanese is possible; stay above the threshold but not certain.
                Some(("zh", 0.55 + 0.2 * share))
            }
        }
        Script::Cyrillic => Some((cyrillic_variant(text), 0.7 + 0.28 * share)),
        Script::Arabic => Some((arabic_variant(text), 0.7 + 0.28 * share)),
        Script::Latin | Script::Other => None,
    };
    if let Some((code, confidence)) = scripted {
        let method = DetectionMethod::Script;
        let _ = has_han;
        return Detection {
            language: tag(code),
            confidence: confidence.clamp(0.0, 0.98),
            method,
            runner_up: counts.get(1).map(|(_, n)| (tag("und"), *n as f32 / letters as f32)),
        };
    }
    // Latin (or a mix): score stopwords and diacritics.
    let (best, second, words, shortest) = latin_scores(text);
    match best {
        Some((code, score)) if score > 0.0 => {
            let ratio = match second {
                Some((_, other)) => score / (score + other),
                None => 1.0,
            };
            let evidence = (score / 4.0).min(1.0);
            let length = (words as f32 / 4.0).min(1.0);
            let mut confidence = 0.30 + 0.35 * ratio + 0.20 * evidence + 0.15 * length;
            // A one- or two-word string is not enough to be sure, however strong one hit looks.
            if words < 3 && score < 4.0 {
                confidence = confidence.min(0.69);
            }
            // A two- or three-letter word that happens to be a stopword ("No.", "la") is not
            // evidence enough to pick a language.
            if words < 3 && score < 4.0 && shortest <= 3 {
                confidence = confidence.min(0.45);
            }
            // Two languages with near-identical evidence ("No.", "de la") are a guess even when
            // one of them sorts first.
            if let Some((_, other)) = second
                && other >= score * 0.8
            {
                confidence = confidence.min(0.45);
            }
            Detection {
                language: tag(code),
                confidence: confidence.clamp(0.0, 0.95),
                method: DetectionMethod::Stopwords,
                runner_up: second.map(|(code, _)| (tag(code), score)),
            }
        }
        _ => Detection { language: und(), confidence: 0.0, method: DetectionMethod::None, runner_up: None },
    }
}

fn chinese_characters(text: &str) -> usize {
    const COMMON: &str = "的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能下过子说产种面而方后多定行学法所民得经十三之进着等部度家电力里如水化高自二理起小物现实加量都两体制机当使点从业本去把性好应开它合还因由其些然前外天政四日那社义事平形相全表间样与关各重新线内数正心反你明看原又么利比或但质气第向道命此变条只没结解问意建月公无系军很情者最立代想已通并提直题党程展五果料象员革位入常文总次品式活设及管特件长求老头基资边流路级少图山统接知较将组见计别她手角期根论运农指几九区强放决西被干做必战先回则任取据处府";
    text.chars().filter(|c| COMMON.contains(*c)).count()
}

fn cyrillic_variant(text: &str) -> &'static str {
    if text.chars().any(|c| matches!(c, 'і' | 'ї' | 'є' | 'ґ' | 'І' | 'Ї' | 'Є' | 'Ґ')) {
        "uk"
    } else if text.chars().any(|c| matches!(c, 'ђ' | 'ћ' | 'џ' | 'љ' | 'њ' | 'Ђ' | 'Ћ' | 'Џ' | 'Љ' | 'Њ')) {
        "sr"
    } else if text.chars().any(|c| matches!(c, 'ѓ' | 'ќ' | 'ѕ' | 'ј' | 'Ѓ' | 'Ќ' | 'Ѕ' | 'Ј')) {
        "mk"
    } else if text.chars().any(|c| matches!(c, 'ъ' | 'Ђ')) && !text.chars().any(|c| matches!(c, 'ы' | 'э' | 'ё' | 'Ы' | 'Э' | 'Ё')) {
        "bg"
    } else {
        "ru"
    }
}

fn arabic_variant(text: &str) -> &'static str {
    if text.chars().any(|c| matches!(c, 'پ' | 'چ' | 'ژ' | 'گ')) {
        "fa"
    } else if text.chars().any(|c| matches!(c, 'ٹ' | 'ڈ' | 'ڑ' | 'ں' | 'ے' | 'ھ' | 'ہ')) {
        "ur"
    } else {
        "ar"
    }
}

/// Stopword and diacritic scores for languages written in the Latin alphabet.
type LatinScore = Option<(&'static str, f32)>;
type LatinScores = (LatinScore, LatinScore, usize, usize);

fn latin_scores(text: &str) -> LatinScores {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').filter(|w| !w.is_empty()).collect();
    let mut scores: Vec<(&'static str, f32)> = LANGUAGES.iter().map(|(code, _, _)| (*code, 0.0)).collect();
    for (code, stopwords, diacritics) in LANGUAGES {
        let mut score = 0.0f32;
        for word in &words {
            if stopwords.iter().any(|s| s == word) {
                score += 2.0;
            }
        }
        for c in lower.chars() {
            if diacritics.contains(c) {
                score += 1.5;
            }
        }
        for trigram in trigrams_for(code) {
            if lower.contains(trigram) {
                score += 0.4;
            }
        }
        if let Some(entry) = scores.iter_mut().find(|(c, _)| *c == *code) {
            entry.1 = score;
        }
    }
    let (best_index, second_index) = best_two(&scores);
    let best = best_index.map(|i| scores[i]).filter(|(_, s)| *s > 0.0);
    let second = second_index.map(|i| scores[i]).filter(|(_, s)| *s > 0.0);
    (best, second, words.len(), words.iter().map(|w| w.chars().count()).min().unwrap_or(0))
}

fn best_two(scores: &[(&'static str, f32)]) -> (Option<usize>, Option<usize>) {
    let mut best: Option<usize> = None;
    let mut second: Option<usize> = None;
    for (i, (_, score)) in scores.iter().enumerate() {
        match best {
            None => best = Some(i),
            Some(b) if *score > scores[b].1 => {
                second = best;
                best = Some(i);
            }
            _ => match second {
                None => second = Some(i),
                Some(s) if *score > scores[s].1 => second = Some(i),
                _ => {}
            },
        }
    }
    (best, second)
}

fn trigrams_for(code: &str) -> &'static [&'static str] {
    match code {
        "en" => &["th", "wh", "ing"],
        "es" => &["ción", "que", "los"],
        "fr" => &["que", "les", "ent"],
        "de" => &["sch", "ich", "ein"],
        "pt" => &["ção", "não", "que"],
        "it" => &["che", "gli", "zione"],
        "nl" => &["ij", "het", "een"],
        "sv" => &["och", "att"],
        "da" => &["og", "ikke"],
        "nb" => &["og", "ikke"],
        "fi" => &["ja", "on"],
        "pl" => &["rz", "sz"],
        "tr" => &["bir", "lar"],
        "id" => &["ang", "dan"],
        "vi" => &["ng", "th"],
        "cs" => &["pro", "př"],
        "ro" => &["și", "este"],
        "hu" => &["és", "hogy"],
        "ca" => &["els", "que"],
        _ => &[],
    }
}

/// (code, high-frequency stopwords, diacritics that point at it).
type LanguageFeatures = (&'static str, &'static [&'static str], &'static str);
const LANGUAGES: &[LanguageFeatures] = &[
    (
        "en",
        &[
            "the", "and", "is", "are", "of", "to", "in", "that", "it", "for", "you", "with", "this", "was", "be", "on", "as", "at", "hello",
            "please", "thanks", "good", "world",
        ],
        "",
    ),
    (
        "es",
        &[
            "el", "la", "de", "que", "y", "en", "los", "las", "una", "por", "para", "con", "no", "se", "su", "al", "del", "es", "hola", "gracias",
            "como", "más", "pero",
        ],
        "ñ¿¡áéíóú",
    ),
    (
        "fr",
        &[
            "le", "la", "les", "des", "une", "et", "est", "que", "pour", "dans", "avec", "sur", "pas", "plus", "bonjour", "merci", "vous", "nous",
            "ce", "du",
        ],
        "çœàèêëîïôûù",
    ),
    (
        "de",
        &[
            "der", "die", "das", "und", "ist", "nicht", "mit", "für", "sich", "ein", "eine", "auf", "den", "dem", "hallo", "danke", "sie", "wir",
            "auch",
        ],
        "ßäöü",
    ),
    ("pt", &["o", "a", "os", "as", "é", "que", "para", "com", "uma", "não", "do", "da", "olá", "obrigado", "você", "mais", "por"], "ãõçáâêô"),
    ("it", &["il", "la", "che", "di", "per", "una", "non", "con", "sono", "gli", "ciao", "grazie", "questo", "più"], "àèéìòù"),
    ("nl", &["de", "het", "een", "en", "van", "is", "dat", "niet", "op", "te", "hallo", "dank", "voor", "met"], "ijéë"),
    ("sv", &["och", "att", "det", "som", "är", "på", "för", "med", "inte", "hej", "tack"], "åäö"),
    ("da", &["og", "at", "det", "er", "som", "på", "for", "med", "ikke", "hej", "tak"], "æøå"),
    ("nb", &["og", "at", "det", "er", "som", "på", "for", "med", "ikke", "hei", "takk", "jeg"], "æøå"),
    ("fi", &["ja", "on", "ei", "että", "se", "oli", "mutta", "hei", "kiitos"], "äö"),
    ("pl", &["i", "w", "nie", "się", "na", "z", "do", "jest", "że", "cześć", "dziękuję"], "ąćęłńóśźż"),
    ("tr", &["ve", "bir", "bu", "için", "ile", "çok", "da", "merhaba", "teşekkür"], "ğışçöü"),
    ("id", &["dan", "yang", "di", "ini", "untuk", "dengan", "tidak", "halo", "terima"], ""),
    ("vi", &["và", "của", "là", "không", "có", "trong", "người", "xin", "chào"], "ơưđạảấầẩẫậắằẳẵặẹẻẽếềểễệỉịọỏốồổỗộớờởỡợụủứừửữựỳỵỷỹ"),
    ("cs", &["a", "v", "se", "na", "je", "že", "to", "ahoj", "děkuji"], "čďěňřšťůž"),
    ("ro", &["și", "de", "la", "cu", "o", "un", "este", "salut", "mulțumesc"], "ăâîșț"),
    ("hu", &["a", "az", "és", "hogy", "nem", "van", "szia", "köszönöm"], "áéíóöőúüű"),
    ("ca", &["el", "la", "els", "les", "i", "de", "que", "hola", "gràcies"], "àèéíïòóúüç"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn code(d: &Detection) -> String {
        d.language.canonical()
    }

    #[test]
    fn detects_common_latin_languages() {
        assert_eq!(code(&detect("The total is 42 dollars and the report is ready for review.")), "en");
        assert_eq!(code(&detect("El total es de 42 dólares y el informe está listo para revisar.")), "es");
        assert_eq!(code(&detect("Le total est de 42 dollars et le rapport est prêt pour la révision.")), "fr");
        assert_eq!(code(&detect("Der Gesamtbetrag ist 42 Dollar und der Bericht ist nicht fertig.")), "de");
        assert_eq!(code(&detect("O total é de 42 dólares e o relatório não está pronto para revisão.")), "pt");
        assert_eq!(code(&detect("Il totale è di 42 dollari e il rapporto non è pronto per la revisione.")), "it");
    }

    #[test]
    fn detects_scripts() {
        assert_eq!(code(&detect("こんにちは世界、これはテストです。")), "ja");
        assert_eq!(code(&detect("你好世界，这是一个测试。")), "zh");
        assert_eq!(code(&detect("안녕하세요 세계")), "ko");
        assert_eq!(code(&detect("Привет, мир! Это проверка.")), "ru");
        assert_eq!(code(&detect("Привіт, світе! Це перевірка.")), "uk");
        assert_eq!(code(&detect("مرحبا بالعالم، هذا اختبار")), "ar");
        assert_eq!(code(&detect("שלום עולם, זה מבחן")), "he");
    }

    #[test]
    fn confidence_is_high_for_a_sentence() {
        let d = detect("The total is 42 dollars and the report is ready.");
        assert!(d.confidence >= 0.5, "{d:?}");
    }

    #[test]
    fn short_or_ambiguous_text_is_uncertain() {
        let d = detect("No.");
        assert!(d.confidence < 0.5, "{d:?}");
        let none = detect("12345 67");
        assert_eq!(none.confidence, 0.0);
        assert_eq!(code(&none), "und");
    }

    #[test]
    fn a_single_clear_greeting_is_recognised_but_not_certain() {
        let d = detect("Hello");
        assert_eq!(code(&d), "en");
        assert!(d.confidence < 0.8, "{d:?}");
        assert!(d.is_confident(0.5));
    }
}
