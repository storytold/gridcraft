//! Spelling against an installed Hunspell dictionary (German here): umlauts and compounds are
//! checked, and misspellings get suggestions. Needs the system dictionary, so it is ignored by
//! default: `GRIDCRAFT_SPELL_LANGS=de_DE cargo test -p gridcraft-engine --test it -- --ignored hunspell`.

use gridcraft_engine::Session;
use serde_json::json;

#[test]
#[ignore = "needs hunspell-de (de_DE) installed and GRIDCRAFT_SPELL_LANGS=de_DE"]
fn german_text_is_checked_with_the_installed_dictionary() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Die Straße über Münster, Käsekuchen und Tabellenkalkulation"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Ein Rechtschreibfehlr und Kaesekuchen"})).unwrap();
    let r = s.execute("review.spelling", json!({})).unwrap();
    let words: Vec<&str> = r["issues"].as_array().unwrap().iter().map(|i| i["word"].as_str().unwrap()).collect();
    assert_eq!(words, ["Rechtschreibfehlr", "Kaesekuchen"], "{r}");
    let first = r["issues"][0]["suggestions"].as_array().unwrap();
    assert!(first.iter().any(|v| v == "Rechtschreibfehler"), "{r}");
}
