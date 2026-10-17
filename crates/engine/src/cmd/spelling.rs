//! Spelling: checks text cells against a dictionary plus a user dictionary kept in the engine
//! preferences. Nothing is bundled:
//! - English (no `lang`, or an `en` tag) uses the system word list (`/usr/share/dict/words` on
//!   macOS and Linux), as before;
//! - any other language uses the Hunspell dictionary installed for it (OS packages, Homebrew,
//!   LibreOffice, `GRIDCRAFT_DICTIONARIES` or [`set_dictionary_dir`]), read with `craft-proof`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use craft_proof::AffixDict;

use gridcraft_core::{RangeRef, Value};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "review.spelling", "Spelling", ["Review", "Proofing"], Some("F7"), "{range?, sheet?, lang?: BCP 47 tag (default English)} → misspelled words with cells and suggestions", has_doc, spelling),
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

fn known(s: &Session, w: &str) -> bool {
    let l = w.to_lowercase();
    let dict = dictionary();
    if dict.contains(&l) || s.prefs.user_dictionary.iter().any(|u| u.eq_ignore_ascii_case(w)) {
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

/// A folder searched for Hunspell dictionaries before the system locations (a setting).
static DICTIONARY_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Search `dir` for Hunspell dictionaries before the system locations; `None` clears it. Takes
/// effect for languages not loaded yet.
pub fn set_dictionary_dir(dir: Option<PathBuf>) {
    *DICTIONARY_DIR.write().unwrap_or_else(|e| e.into_inner()) = dir;
    hunspell_cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Loaded Hunspell dictionaries by file-name candidates, `None` when none is installed.
fn hunspell_cache() -> &'static Mutex<HashMap<Vec<String>, Option<Arc<AffixDict>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Vec<String>, Option<Arc<AffixDict>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Hunspell file names to try for a tag: `pl-PL` → `pl_PL`, `pl`; a bare `pl` → `pl_PL`, `pl`.
fn hunspell_names(tag: &str) -> Vec<String> {
    let Some(primary) = craft_proof::tag::primary_subtag(tag) else { return Vec::new() };
    let region = craft_proof::tag::normalize_tag(tag)
        .and_then(|t| t.split('-').skip(1).find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_uppercase())).map(str::to_string));
    let full = format!("{primary}_{}", region.unwrap_or_else(|| primary.to_ascii_uppercase()));
    vec![full, primary]
}

/// The installed Hunspell dictionary for `tag`, loaded once.
fn hunspell(tag: &str) -> Option<Arc<AffixDict>> {
    let names = hunspell_names(tag);
    if names.is_empty() {
        return None;
    }
    let mut cache = hunspell_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = cache.get(&names) {
        return d.clone();
    }
    let extra_dir = DICTIONARY_DIR.read().unwrap_or_else(|e| e.into_inner()).clone();
    let search = craft_proof::hunspell::Search { extra_dir, ..craft_proof::hunspell::Search::for_app("GridCraft") };
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let loaded = search.find(&refs).and_then(|(aff, dic)| match craft_proof::hunspell::load(&aff, &dic) {
        Ok(d) => Some(Arc::new(d)),
        Err(e) => {
            log::warn!("spelling dictionary {}: {e}", dic.display());
            None
        }
    });
    cache.insert(names, loaded.clone());
    loaded
}

/// Is `w` correct in the affix dictionary, as typed or (sentence start, title case) lower-cased?
fn affix_known(s: &Session, d: &AffixDict, w: &str) -> bool {
    d.check(w) || d.check(&w.to_lowercase()) || s.prefs.user_dictionary.iter().any(|u| u.eq_ignore_ascii_case(w))
}

/// Up to six dictionary words one edit away, trying the dictionary's own `TRY` letters, with the
/// word's capitalisation kept.
fn affix_suggestions(d: &AffixDict, w: &str) -> Vec<String> {
    let chars: Vec<char> = w.to_lowercase().chars().collect();
    let letters: Vec<char> = d.try_chars().chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |cand: Vec<char>| {
        let cand: String = cand.into_iter().collect();
        if out.len() < 6 && !out.contains(&cand) && d.check(&cand) {
            out.push(cand);
        }
    };
    for i in 0..=chars.len() {
        if i < chars.len() {
            for &c in &letters {
                let mut r = chars.clone();
                r[i] = c;
                push(r);
            }
            let mut del = chars.clone();
            del.remove(i);
            push(del);
            if i + 1 < chars.len() {
                let mut t = chars.clone();
                t.swap(i, i + 1);
                push(t);
            }
        }
        for &c in &letters {
            let mut ins = chars.clone();
            ins.insert(i, c);
            push(ins);
        }
    }
    let cap = w.chars().next().is_some_and(char::is_uppercase);
    out.into_iter().map(|s| if cap { craft_proof::affix::capitalize(&s) } else { s }).collect()
}

fn spelling(s: &mut Session, p: &Json) -> Result<Json> {
    let lang = str_param(p, "lang").filter(|t| craft_proof::tag::primary_subtag(t).is_some_and(|l| l != "en"));
    let affix = match lang {
        Some(tag) => Some(hunspell(tag).ok_or_else(|| {
            EngineError::Other(format!("No {tag} spelling dictionary is installed (GridCraft reads installed Hunspell dictionaries)."))
        })?),
        None => None,
    };
    if affix.is_none() && dictionary().is_empty() {
        return Err(EngineError::Other("No dictionary is available on this system (GridCraft uses the system word list).".into()));
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
            if w.chars().count() < 2 || w.chars().all(char::is_uppercase) {
                continue;
            }
            match &affix {
                Some(d) => {
                    if !affix_known(s, d, w) {
                        issues.push(json!({"cell": c.a1(), "word": w, "suggestions": affix_suggestions(d, w)}));
                    }
                }
                // The English word list is ASCII; other scripts aren't English misspellings.
                None if !w.is_ascii() => {}
                None => {
                    if !known(s, w) {
                        issues.push(json!({"cell": c.a1(), "word": w, "suggestions": suggestions(w)}));
                    }
                }
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
}
