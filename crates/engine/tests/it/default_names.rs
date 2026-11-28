//! New workbooks and sheets are named with the words a front end sets from its interface language
//! (`Session::default_names`); names already in a workbook keep theirs.

use gridcraft_engine::{DefaultNames, Session};
use serde_json::json;

fn names(s: &Session) -> Vec<String> {
    s.active().unwrap().wb.sheets.iter().map(|sh| sh.name.clone()).collect()
}

#[test]
fn new_workbooks_and_sheets_use_the_set_words() {
    let mut s = Session::new();
    let english = s.new_workbook();
    s.default_names.set("Pasta", "Planilha");
    let translated = s.new_workbook();
    assert!(s.documents()[translated].title.starts_with("Pasta"), "{}", s.documents()[translated].title);
    assert!(names(&s).iter().all(|n| n.starts_with("Planilha")), "{:?}", names(&s));
    s.execute("home.insertSheet", json!({})).unwrap();
    let after = names(&s);
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|n| n.starts_with("Planilha")), "an inserted sheet uses the word too: {after:?}");
    assert_ne!(after[0], after[1], "and a number not in use");
    // The workbook made before the words changed keeps its English names.
    assert!(s.documents()[english].title.starts_with("Book"));
    assert!(s.documents()[english].wb.sheets.iter().all(|sh| sh.name.starts_with("Sheet")));
}

#[test]
fn a_word_that_cannot_name_a_sheet_keeps_english() {
    let mut names = DefaultNames::default();
    for bad in ["", "  ", "Plan:ilha", "Folha[", "'Hoja", "Tabelle2"] {
        names.set(bad, bad);
        assert_eq!(names, DefaultNames::default(), "{bad:?}");
    }
}

#[test]
fn the_accessibility_check_flags_only_a_word_and_a_number() {
    let mut s = Session::new();
    s.default_names.set("Pasta", "Planilha");
    s.default_names.set_known_sheet_words(["Sheet", "Planilha"]);
    s.new_workbook();
    for name in ["Planilha de vendas", "Sheets", "Sheet3"] {
        s.execute("home.insertSheet", json!({"name": name})).unwrap();
    }
    // The interface goes back to English: a Portuguese default is still a default.
    s.default_names.set("Book", "Sheet");
    let issues = s.execute("review.checkAccessibility", json!({})).unwrap();
    let flagged: Vec<&str> =
        issues.as_array().unwrap().iter().filter(|i| i["issue"] == "Default sheet name").filter_map(|i| i["sheet"].as_str()).collect();
    assert!(flagged.contains(&"Planilha1") && flagged.contains(&"Sheet3"), "{flagged:?}");
    assert!(!flagged.contains(&"Planilha de vendas") && !flagged.contains(&"Sheets"), "{flagged:?}");
}
