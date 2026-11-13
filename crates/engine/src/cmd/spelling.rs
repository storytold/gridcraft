//! Spelling: checks text cells against an installed system word list (`/usr/share/dict/words` on
//! macOS and Linux; nothing is bundled) plus a user dictionary kept in the engine preferences.
//!
//! A command's `language` chooses the list: English is the default, and `pt*` reads the Portuguese
//! list (a plain word list or Hunspell's `pt_PT.dic`). When the requested language has no list
//! installed the command fails with that language named, rather than checking the text as English.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use gridcraft_core::{RangeRef, Value};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cell_param, cmd, has_doc, ok, str_param, target_range, target_sheet};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "review.spelling",
            "Spelling",
            ["Review", "Proofing"],
            Some("F7"),
            "{range?, sheet?, language?} → misspelled words with cells and suggestions (language: a BCP 47 tag such as `en-US` or `pt-PT`; default `en`)",
            has_doc,
            spelling
        ),
        cmd!(noundo "review.addToDictionary", "Add to Dictionary", [], None, "{word}", has_doc, add_word),
        cmd!("review.changeSpelling", "Change", [], None, "{cell, word, to, all?: bool}", has_doc, change),
        cmd!(
            query "review.thesaurus",
            "Thesaurus",
            ["Review", "Proofing"],
            None,
            "{word, language?} → related words (words sharing the stem in the dictionary; language: a BCP 47 tag, default `en`)",
            has_doc,
            thesaurus
        ),
    ]
}

/// Which installed word list a BCP 47 tag selects. Only the primary subtag matters: `pt`, `pt-BR`
/// and `pt-PT` all read the Portuguese list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    English,
    Portuguese,
    /// A tag GridCraft has no word list path for.
    Unsupported,
}

impl Language {
    fn from_tag(tag: &str) -> Self {
        match primary_subtag(tag).as_str() {
            // No tag at all is English, as it always was.
            "" | "en" => Self::English,
            "pt" => Self::Portuguese,
            _ => Self::Unsupported,
        }
    }

    /// Where the list may be installed, in load order; the first file that exists wins.
    ///
    /// `/usr/share/hunspell/pt_PT.dic` is a Hunspell dictionary: its `.aff` affix rules are not
    /// read, so the words are lemmas and inflected forms are reported. It is still a far better
    /// Portuguese list than checking Portuguese text against an English one.
    fn paths(self) -> &'static [&'static str] {
        match self {
            Self::English => &["/usr/share/dict/words", "/usr/share/dict/american-english", "/usr/share/dict/british-english"],
            Self::Portuguese => &["/usr/share/dict/portuguese", "/usr/share/hunspell/pt_PT.dic"],
            Self::Unsupported => &[],
        }
    }

    /// Letters a suggestion may substitute or insert. English keeps the a–z alphabet it always
    /// used; Portuguese adds its accented letters, so `relatorio` can suggest `relatório`.
    fn alphabet(self) -> &'static str {
        match self {
            Self::English | Self::Unsupported => "abcdefghijklmnopqrstuvwxyz",
            Self::Portuguese => "abcdefghijklmnopqrstuvwxyzáàâãéêíóôõúç",
        }
    }

    /// English's list omits simple inflections, so `known` folds a few suffixes. Portuguese words
    /// are checked as they are: its `.dic` is a lemma list and its plural rules are not English's.
    fn folds_inflections(self) -> bool {
        self == Self::English
    }
}

fn primary_subtag(tag: &str) -> String {
    tag.split(['-', '_']).next().unwrap_or("").trim().to_ascii_lowercase()
}

struct WordList {
    language: Language,
    words: HashSet<String>,
    /// Whether any word carries a non-ASCII letter. A list of ASCII words cannot contain an
    /// accented token, so such tokens stay skipped (English's behaviour); a list that has them can
    /// judge accented text (Portuguese's).
    has_non_ascii: bool,
}

/// Reads an installed word list. WebAssembly has no filesystem, so there is none there.
#[cfg(not(target_arch = "wasm32"))]
fn read_installed(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(target_arch = "wasm32")]
fn read_installed(path: &str) -> Option<String> {
    let _ = path;
    None
}

/// Splits a word list into lowercase words, keeping at most two million lines like the old loader.
///
/// A Hunspell `.dic` starts with a count line and annotates entries (`casa/SFP`, `não [adv]`);
/// the count line, the `/flags` and the `[morphology]` are not words.
fn parse_list(language: Language, text: &str, hunspell: bool) -> WordList {
    let mut words = HashSet::new();
    let mut has_non_ascii = false;
    for (n, line) in text.lines().take(2_000_000).enumerate() {
        if hunspell && n == 0 {
            continue;
        }
        let raw = line.split_whitespace().next().unwrap_or("");
        let word = if hunspell {
            // `casa/SFP`, `não [adv]`: the slash flags and bracket notes are not words.
            raw.split('/').next().unwrap_or("").split('[').next().unwrap_or("")
        } else {
            raw
        };
        let word = word.trim().to_lowercase();
        if word.is_empty() {
            continue;
        }
        has_non_ascii |= !word.is_ascii();
        words.insert(word);
    }
    WordList { language, words, has_non_ascii }
}

/// The first list `language` names that `read` returns, or `None` when none is installed. `read` is
/// a parameter so tests can supply fixture files instead of relying on the host's dictionaries.
fn load_list(language: Language, read: &dyn Fn(&str) -> Option<String>) -> Option<WordList> {
    language.paths().iter().find_map(|path| read(path).map(|text| parse_list(language, &text, path.ends_with(".dic"))))
}

/// Loaded lists, keyed by primary subtag (`pt-PT` and `pt-BR` share `pt`). `None` records that
/// nothing was installed, so a missing dictionary is not re-read on every check.
static LISTS: LazyLock<Mutex<HashMap<String, Option<Arc<WordList>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn missing_list(tag: &str) -> EngineError {
    match Language::from_tag(tag) {
        Language::Unsupported => EngineError::Other(format!(
            "no word list is installed for `{tag}`; GridCraft reads English from /usr/share/dict/words and Portuguese from /usr/share/dict/portuguese or /usr/share/hunspell/pt_PT.dic"
        )),
        language => EngineError::Other(format!("no {tag} word list is installed; put one in {}", language.paths().join(" or "))),
    }
}

fn word_list(tag: &str) -> Result<Arc<WordList>> {
    let primary = primary_subtag(tag);
    let mut lists = LISTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(loaded) = lists.get(&primary) {
        return loaded.clone().ok_or_else(|| missing_list(tag));
    }
    let loaded = load_list(Language::from_tag(&primary), &read_installed).map(Arc::new);
    lists.insert(primary, loaded.clone());
    loaded.ok_or_else(|| missing_list(tag))
}

fn known(user_dictionary: &[String], list: &WordList, w: &str) -> bool {
    let l = w.to_lowercase();
    if list.words.contains(&l) || user_dictionary.iter().any(|u| u.to_lowercase() == l) {
        return true;
    }
    if !list.language.folds_inflections() {
        return false;
    }
    // Simple inflections the English list doesn't spell out.
    for suf in ["s", "es", "ed", "ing", "ly", "er", "ers", "'s"] {
        if let Some(stem) = l.strip_suffix(suf)
            && stem.len() > 2
            && (list.words.contains(stem) || list.words.contains(&format!("{stem}e")))
        {
            return true;
        }
    }
    if let Some(stem) = l.strip_suffix("ies")
        && list.words.contains(&format!("{stem}y"))
    {
        return true;
    }
    false
}

/// Longest word we generate suggestions for. Candidate text grows with the square of the word length, so longer
/// tokens (no real word is this long) get no suggestions instead of an unbounded allocation.
const MAX_SUGGEST_LEN: usize = 64;

fn edits1(w: &str, letters: &str) -> Vec<String> {
    if w.chars().count() > MAX_SUGGEST_LEN {
        return Vec::new();
    }
    let chars: Vec<char> = w.chars().collect();
    let mut out = Vec::new();
    for i in 0..=chars.len() {
        if i < chars.len() {
            let mut d = chars.clone();
            d.remove(i);
            out.push(d.iter().collect());
            if i + 1 < chars.len() {
                let mut t = chars.clone();
                t.swap(i, i + 1);
                out.push(t.iter().collect());
            }
            for c in letters.chars() {
                let mut r = chars.clone();
                r[i] = c;
                out.push(r.iter().collect());
            }
        }
        for c in letters.chars() {
            let mut ins = chars.clone();
            ins.insert(i, c);
            out.push(ins.iter().collect());
        }
    }
    out
}

fn suggestions(list: &WordList, w: &str) -> Vec<String> {
    let l = w.to_lowercase();
    let mut out: Vec<String> = edits1(&l, list.language.alphabet()).into_iter().filter(|c| list.words.contains(c)).collect();
    out.sort();
    out.dedup();
    out.truncate(6);
    // Keep the original capitalisation.
    let cap = w.chars().next().is_some_and(char::is_uppercase);
    out.into_iter()
        .map(|s| {
            if cap {
                let mut c = s.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(s)
            } else {
                s
            }
        })
        .collect()
}

fn spelling(s: &mut Session, p: &Json) -> Result<Json> {
    let language = str_param(p, "language").unwrap_or("en");
    let list = word_list(language)?;
    let si = target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let scope = match str_param(p, "range") {
        Some(_) => target_range(s, p)?,
        None => sh.used_range().unwrap_or(RangeRef::cell(d.selection.active)),
    };
    let mut issues = Vec::new();
    for (c, cell) in sh.cells.iter_range(scope) {
        if cell.formula.is_some() {
            continue;
        }
        let Value::Text(t) = &cell.value else { continue };
        for w in t.split(|ch: char| !(ch.is_alphabetic() || ch == '\'')) {
            let w = w.trim_matches('\'');
            if w.chars().count() < 2 || w.chars().all(char::is_uppercase) {
                continue;
            }
            // English's ASCII list cannot hold an accented word, so it never checked one. A list
            // that carries accented words can, and skipping them made accented Portuguese
            // invisible to the check.
            if !w.is_ascii() && !list.has_non_ascii {
                continue;
            }
            if !known(&s.prefs.user_dictionary, &list, w) {
                issues.push(json!({"cell": c.a1(), "word": w, "suggestions": suggestions(&list, w)}));
            }
        }
        if issues.len() >= 500 {
            break;
        }
    }
    Ok(json!({"issues": issues, "done": issues.is_empty(), "language": language}))
}

fn add_word(s: &mut Session, p: &Json) -> Result<Json> {
    let w = str_param(p, "word").ok_or_else(|| bad("review.addToDictionary", "missing `word`"))?.to_string();
    if !s.prefs.user_dictionary.iter().any(|u| u.eq_ignore_ascii_case(&w)) {
        s.prefs.user_dictionary.push(w);
    }
    ok()
}

fn change(s: &mut Session, p: &Json) -> Result<Json> {
    let word = str_param(p, "word").ok_or_else(|| bad("review.changeSpelling", "missing `word`"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad("review.changeSpelling", "missing `to`"))?.to_string();
    if bool_param(p, "all").unwrap_or(false) {
        return s.execute("edit.replace", json!({"what": word, "with": to, "matchCase": true, "all": true}));
    }
    let c = cell_param(s, p, "cell")?.ok_or_else(|| bad("review.changeSpelling", "missing `cell`"))?;
    let text = s.doc()?.wb.active().and_then(|sh| sh.cell(c)).map(|x| x.input_text()).unwrap_or_default();
    s.execute("cell.set", json!({"cell": c.a1(), "input": text.replacen(&word, &to, 1)}))
}

fn thesaurus(_: &mut Session, p: &Json) -> Result<Json> {
    let w = str_param(p, "word").unwrap_or("").to_lowercase();
    if w.len() < 3 {
        return Ok(json!([]));
    }
    let list = word_list(str_param(p, "language").unwrap_or("en"))?;
    let stem: String = w.chars().take(w.chars().count().saturating_sub(2).max(3)).collect();
    let mut related: Vec<&String> = list.words.iter().filter(|d| d.starts_with(&stem) && **d != w && d.len() <= w.len() + 4).collect();
    related.sort();
    related.truncate(20);
    Ok(json!(related))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits1_is_capped_for_very_long_words() {
        let english = Language::English.alphabet();
        assert_eq!(edits1("helo", english).len(), 54 * 4 + 25);
        assert_eq!(edits1(&"a".repeat(MAX_SUGGEST_LEN), english).len(), 54 * MAX_SUGGEST_LEN + 25);
        assert!(edits1(&"a".repeat(MAX_SUGGEST_LEN + 1), english).is_empty());
        assert!(edits1(&"a".repeat(1_000_000), english).is_empty());
    }

    #[test]
    fn pt_pt_reads_the_portuguese_paths_in_order() {
        let asked = std::cell::RefCell::new(Vec::new());
        let read = |path: &str| {
            asked.borrow_mut().push(path.to_string());
            // The plain list is absent; Hunspell's is installed.
            (path == "/usr/share/hunspell/pt_PT.dic").then(|| "3\ncasa/SFP\nrelatório\nnão [adv]\n".to_string())
        };
        let list = load_list(Language::from_tag("pt-PT"), &read).expect("the .dic fixture is installed");
        assert_eq!(*asked.borrow(), ["/usr/share/dict/portuguese", "/usr/share/hunspell/pt_PT.dic"]);
        assert!(list.has_non_ascii);
        assert_eq!(list.words.len(), 3);
    }

    #[test]
    fn hunspell_count_flags_and_annotations_are_not_words() {
        let list = parse_list(Language::Portuguese, "2\ncasa/SFP\nrelatório/… [relatar]\n", true);
        assert!(list.words.contains("casa"));
        assert!(list.words.contains("relatório"));
        assert!(!list.words.iter().any(|w| w.contains('/') || w.contains('[') || w == "2"));
    }

    #[test]
    fn portuguese_fixture_knows_its_lemmas_but_not_their_plurals() {
        let list = parse_list(Language::Portuguese, "trabalho\nrelatório\nnão\nimportante\n", false);
        let user: Vec<String> = Vec::new();
        for word in ["trabalho", "relatório", "não"] {
            assert!(known(&user, &list, word), "{word} is in the fixture");
        }
        // Hunspell's `.dic` (and a plain word list) holds lemmas; `-s` plurals come from the `.aff`
        // rules, which GridCraft does not read. English's suffix folding must not hide that, so a
        // plural is reported until the user adds it to their dictionary.
        assert!(!known(&user, &list, "trabalhos"));
    }

    #[test]
    fn portuguese_suggestions_use_the_accented_alphabet() {
        let list = parse_list(Language::Portuguese, "relatório\n", false);
        assert_eq!(suggestions(&list, "relatorio"), ["relatório"]);
    }

    #[test]
    fn english_list_keeps_its_ascii_behaviour() {
        let list = parse_list(Language::English, "hello\nworld\n", false);
        assert!(!list.has_non_ascii);
        let user: Vec<String> = Vec::new();
        // The English list's suffix folding stays.
        assert!(known(&user, &list, "hellos"));
        assert_eq!(suggestions(&list, "helo"), ["hello"]);
    }

    #[test]
    fn unknown_language_reports_a_missing_word_list() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "trabalho importante"})).unwrap();
        let err = s.execute("review.spelling", json!({"language": "xx-NONE"})).unwrap_err();
        assert!(matches!(err, EngineError::Other(_)), "{err:?}");
        assert!(err.to_string().contains("xx-NONE"), "{err}");
        assert!(err.to_string().contains("no word list"), "{err}");
    }
}
