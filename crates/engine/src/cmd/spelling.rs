//! Spelling: checks text cells against the system word list (`/usr/share/dict/words` on macOS
//! and Linux; nothing is bundled), the installed Hunspell dictionaries for the desktop's language
//! (`de_DE` …, see [`hunspell`]) plus a user dictionary kept in the engine preferences.

use std::collections::HashSet;
use std::sync::OnceLock;

use gridcraft_core::{RangeRef, Value};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cell_param, cmd, has_doc, ok, str_param, target_range, target_sheet};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "review.spelling", "Spelling", ["Review", "Proofing"], Some("F7"), "{range?, sheet?} → misspelled words with cells and suggestions", has_doc, spelling),
        cmd!(noundo "review.addToDictionary", "Add to Dictionary", [], None, "{word}", has_doc, add_word),
        cmd!("review.changeSpelling", "Change", [], None, "{cell, word, to, all?: bool}", has_doc, change),
        cmd!(query "review.thesaurus", "Thesaurus", ["Review", "Proofing"], None, "{word} → related words (words sharing the stem in the dictionary)", has_doc, thesaurus),
    ]
}

fn dictionary() -> &'static HashSet<String> {
    static DICT: OnceLock<HashSet<String>> = OnceLock::new();
    DICT.get_or_init(|| {
        #[cfg(target_arch = "wasm32")]
        {
            HashSet::new()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut set = HashSet::new();
            for path in ["/usr/share/dict/words", "/usr/share/dict/american-english", "/usr/share/dict/british-english"] {
                if let Ok(text) = std::fs::read_to_string(path) {
                    for w in text.lines().take(2_000_000) {
                        set.insert(w.trim().to_lowercase());
                    }
                    break;
                }
            }
            set
        }
    })
}

/// Installed Hunspell dictionaries (`/usr/share/hunspell/de_DE.{aff,dic}` …), loaded on first use:
/// the desktop's language (`GRIDCRAFT_SPELL_LANGS=de_DE,en_US` overrides it) and `en_US` when it
/// is installed. A word is correct when any of them accepts it.
#[cfg(not(target_arch = "wasm32"))]
mod hunspell {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    pub struct Dictionaries(Vec<spellbook::Dictionary>);

    impl Dictionaries {
        #[cfg(test)]
        pub fn from_dictionaries(dicts: Vec<spellbook::Dictionary>) -> Self {
            Self(dicts)
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        pub fn check(&self, word: &str) -> bool {
            self.0.iter().any(|d| d.check(word))
        }

        /// Up to `max` suggestions of the first dictionary that has any, best first.
        pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
            for d in &self.0 {
                let mut out = Vec::new();
                d.suggest(word, &mut out);
                if !out.is_empty() {
                    out.truncate(max);
                    return out;
                }
            }
            Vec::new()
        }
    }

    pub fn dictionaries() -> &'static Dictionaries {
        static DICTS: OnceLock<Dictionaries> = OnceLock::new();
        DICTS.get_or_init(|| Dictionaries(wanted().iter().filter_map(|name| load(name)).collect()))
    }

    /// Dictionary names to load, e.g. `["de_DE", "en_US"]`.
    fn wanted() -> Vec<String> {
        if let Some(list) = std::env::var_os("GRIDCRAFT_SPELL_LANGS") {
            return list.to_string_lossy().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        let mut names = Vec::new();
        let locale =
            ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty() && v != "C" && v != "POSIX"));
        if let Some(locale) = locale {
            // `de_DE.UTF-8@euro` → `de_DE`; a bare `de` has no dictionary of its own name.
            let tag = locale.split(['.', '@']).next().unwrap_or("").to_string();
            if !tag.is_empty() {
                names.push(tag);
            }
        }
        names.push("en_US".to_string());
        names
    }

    fn search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) =
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        {
            dirs.push(home.join("hunspell"));
        }
        let data = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
        for d in data.split(':').filter(|d| !d.is_empty()) {
            dirs.push(PathBuf::from(d).join("hunspell"));
            dirs.push(PathBuf::from(d).join("myspell"));
            dirs.push(PathBuf::from(d).join("myspell/dicts"));
        }
        dirs.push(PathBuf::from("/usr/share/hunspell"));
        dirs
    }

    /// `name` (`de_DE`), else another variety of its language (`de_AT`).
    fn load(name: &str) -> Option<spellbook::Dictionary> {
        let dirs = search_dirs();
        let lang = name.split('_').next().unwrap_or(name);
        for candidate in std::iter::once(name.to_string()).chain(std::iter::once(lang.to_string())) {
            if let Some(d) = load_exact(&dirs, &candidate) {
                return Some(d);
            }
        }
        // Only a variety of the language is installed (`de_AT`, `en_GB` …): any of them is better than none.
        for dir in &dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            let mut found: Vec<String> = entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter_map(|f| f.strip_suffix(".dic").map(str::to_string))
                .filter(|f| f.split(['_', '-']).next() == Some(lang))
                .collect();
            found.sort();
            for f in found {
                if let Some(d) = load_exact(&dirs, &f) {
                    return Some(d);
                }
            }
        }
        None
    }

    fn load_exact(dirs: &[PathBuf], name: &str) -> Option<spellbook::Dictionary> {
        for dir in dirs {
            let (aff, dic) = (dir.join(format!("{name}.aff")), dir.join(format!("{name}.dic")));
            let (Ok(aff), Ok(dic)) = (std::fs::read_to_string(&aff), std::fs::read_to_string(&dic)) else { continue };
            match spellbook::Dictionary::new(&aff, &dic) {
                Ok(d) => return Some(d),
                Err(e) => log::warn!("hunspell dictionary {name} in {}: {e}", dir.display()),
            }
        }
        None
    }
}

/// True when a Hunspell dictionary accepts `w` (always false where none is installed).
fn hunspell_knows(w: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        hunspell::dictionaries().check(w)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = w;
        false
    }
}

fn hunspell_loaded() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        !hunspell::dictionaries().is_empty()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

fn hunspell_suggest(w: &str, max: usize) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        hunspell::dictionaries().suggest(w, max)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (w, max);
        Vec::new()
    }
}

fn known(s: &Session, w: &str) -> bool {
    let l = w.to_lowercase();
    let dict = dictionary();
    if dict.contains(&l) || s.prefs.user_dictionary.iter().any(|u| u.eq_ignore_ascii_case(w)) || hunspell_knows(w) {
        return true;
    }
    // Simple inflections the list doesn't spell out.
    for suf in ["s", "es", "ed", "ing", "ly", "er", "ers", "'s"] {
        if let Some(stem) = l.strip_suffix(suf)
            && stem.len() > 2
            && (dict.contains(stem) || dict.contains(&format!("{stem}e")))
        {
            return true;
        }
    }
    if let Some(stem) = l.strip_suffix("ies")
        && dict.contains(&format!("{stem}y"))
    {
        return true;
    }
    false
}

/// Longest word we generate suggestions for. Candidate text grows with the square of the word length, so longer
/// tokens (no real word is this long) get no suggestions instead of an unbounded allocation.
const MAX_SUGGEST_LEN: usize = 64;

fn edits1(w: &str) -> Vec<String> {
    if w.chars().count() > MAX_SUGGEST_LEN {
        return Vec::new();
    }
    let chars: Vec<char> = w.chars().collect();
    let mut out = Vec::new();
    let letters = "abcdefghijklmnopqrstuvwxyz";
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

fn suggestions(w: &str) -> Vec<String> {
    let from_hunspell = hunspell_suggest(w, 6);
    if !from_hunspell.is_empty() {
        return from_hunspell;
    }
    let dict = dictionary();
    let l = w.to_lowercase();
    let mut out: Vec<String> = edits1(&l).into_iter().filter(|c| dict.contains(c)).collect();
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
    let hunspell = hunspell_loaded();
    if dictionary().is_empty() && !hunspell {
        return Err(EngineError::Other(
            "No dictionary is available on this system (GridCraft uses the system word list and the installed Hunspell dictionaries).".into(),
        ));
    }
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
            if w.chars().count() < 2 || w.chars().all(char::is_uppercase) || (!hunspell && !w.is_ascii()) {
                continue;
            }
            if !known(s, w) {
                issues.push(json!({"cell": c.a1(), "word": w, "suggestions": suggestions(w)}));
            }
        }
        if issues.len() >= 500 {
            break;
        }
    }
    Ok(json!({"issues": issues, "done": issues.is_empty()}))
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
    let stem: String = w.chars().take(w.chars().count().saturating_sub(2).max(3)).collect();
    let mut related: Vec<&String> = dictionary().iter().filter(|d| d.starts_with(&stem) && **d != w && d.len() <= w.len() + 4).collect();
    related.sort();
    related.truncate(20);
    Ok(json!(related))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits1_is_capped_for_very_long_words() {
        assert_eq!(edits1("helo").len(), 54 * 4 + 25);
        assert_eq!(edits1(&"a".repeat(MAX_SUGGEST_LEN)).len(), 54 * MAX_SUGGEST_LEN + 25);
        assert!(edits1(&"a".repeat(MAX_SUGGEST_LEN + 1)).is_empty());
        assert!(edits1(&"a".repeat(1_000_000)).is_empty());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn hunspell_dictionary_accepts_umlauts_and_suggests() {
        let d = spellbook::Dictionary::new("SET UTF-8\n", "3\nStraße\nKäse\nMaß\n").expect("tiny dictionary parses");
        let dicts = hunspell::Dictionaries::from_dictionaries(vec![d]);
        assert!(dicts.check("Käse") && dicts.check("Straße"));
        assert!(!dicts.check("Kaese"));
        assert!(dicts.suggest("Kase", 6).contains(&"Käse".to_string()));
    }
}
