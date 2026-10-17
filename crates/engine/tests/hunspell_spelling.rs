//! Spelling in languages other than English reads an installed Hunspell dictionary through
//! `craft-proof`; a synthetic one stands in for the system's here.
use gridcraft_engine::Session;
use gridcraft_engine::cmd::spelling::set_dictionary_dir;
use serde_json::{Value as Json, json};

fn words(result: &Json) -> Vec<String> {
    result["issues"].as_array().unwrap().iter().map(|i| i["word"].as_str().unwrap().to_string()).collect()
}

#[test]
fn polish_text_is_checked_against_the_installed_polish_dictionary() {
    let dir = std::env::temp_dir().join(format!("gridcraft-hunspell-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pl_PL.aff"), "SET UTF-8\nTRY aeoiyułkrsz\n\nSFX A Y 2\nSFX A 0 y [^y]\nSFX A 0 ami .\n").unwrap();
    std::fs::write(dir.join("pl_PL.dic"), "3\nkot/A\nżółw/A\npies\n").unwrap();
    set_dictionary_dir(Some(dir.clone()));

    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Żółwy i koty"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "kotami żółwami kotx"})).unwrap();
    let r = s.execute("review.spelling", json!({"lang": "pl-PL"})).unwrap();
    // `i` is a one-letter word (skipped); everything else is in the dictionary except `kotx`.
    assert_eq!(words(&r), ["kotx"], "{r}");
    let sugg: Vec<&str> = r["issues"][0]["suggestions"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    // One edit away: dropping the `x` and replacing it with a `TRY` letter.
    assert!(sugg.contains(&"kot") && sugg.contains(&"koty"), "{sugg:?}");

    // The user dictionary applies in every language.
    s.execute("review.addToDictionary", json!({"word": "kotx"})).unwrap();
    assert!(words(&s.execute("review.spelling", json!({"lang": "pl"})).unwrap()).is_empty());

    // A language with no installed dictionary is an error, not English marks on Polish words.
    let err = s.execute("review.spelling", json!({"lang": "xx-XX"})).unwrap_err();
    assert!(err.to_string().contains("xx-XX"), "{err}");

    set_dictionary_dir(None);
    let _ = std::fs::remove_dir_all(&dir);
}
