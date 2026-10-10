//! Interface language tests.

use super::catalog::{Catalog, parse_entries, placeholders};
use super::*;

fn lang(code: &str) -> Lang {
    Lang::from_code(code).unwrap_or_else(|| panic!("{code} registered"))
}

#[test]
fn system_locales_map_to_languages() {
    assert_eq!(lang_from_tag("de-DE"), Some(lang("de")));
    assert_eq!(lang_from_tag("de_AT.UTF-8"), Some(lang("de")));
    assert_eq!(lang_from_tag("de-CH"), Some(lang("de")));
    assert_eq!(lang_from_tag("de"), Some(lang("de")));
    assert_eq!(lang_from_tag("en-GB"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C"), Some(Lang::EN));
    assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
    assert_eq!(lang_from_tag("fr-FR"), None);
    assert_eq!(lang_from_tag(""), None);
    assert_eq!(lang_from_tag("_"), None);
    assert_eq!(candidates("zh_TW.UTF-8"), ["zh-tw", "zh-hant", "zh"]);
}

#[test]
fn the_first_supported_preferred_language_wins() {
    // Windows and macOS list several preferred languages in order.
    assert_eq!(first_supported(["fr-FR", "de-DE", "en-US"]), Some(lang("de")));
    assert_eq!(first_supported(["en-US", "de-DE"]), Some(Lang::EN));
    assert_eq!(first_supported(["fr-FR", "it-IT"]), None);
    assert_eq!(first_supported([]), None);
}

#[test]
fn settings_resolve_with_fallback() {
    assert_eq!(Lang::from_pref("de"), lang("de"));
    assert_eq!(Lang::from_pref("DE"), lang("de"));
    assert_eq!(Lang::from_pref("en"), Lang::EN);
    // `auto` and unknown codes follow the system (English under test).
    assert_eq!(Lang::from_pref(AUTO), Lang::EN);
    assert_eq!(Lang::from_pref("xx-unknown"), Lang::EN);
    assert_eq!(normalize_pref("De"), Some("de"));
    assert_eq!(normalize_pref("Auto"), Some(AUTO));
    assert_eq!(normalize_pref("de-DE"), None, "only exact codes are settings");
    assert_eq!(normalize_pref("xx"), None);
}

#[test]
fn lookups_fall_back_to_english() {
    let de = lang("de");
    assert_eq!(tr(de, "Home"), "Start");
    assert_eq!(tr(de, "no such label"), "no such label");
    assert_eq!(tr(Lang::EN, "Home"), "Home");
    assert!(has(de, "Bold") && !has(Lang::EN, "Bold"));
}

#[test]
fn current_language_is_per_thread() {
    set_current(lang("de"));
    assert_eq!(t("Insert"), "Einfügen");
    assert_eq!(location(&["Home", "Font"]), "Start › Schriftart");
    std::thread::spawn(|| assert_eq!(current(), Lang::EN)).join().unwrap();
    set_current(Lang::EN);
    assert_eq!(t("Insert"), "Insert");
    assert_eq!(location(&["Home", "Font"]), "Home › Font");
}

#[test]
fn tooltips_keep_their_shortcut() {
    set_current(lang("de"));
    assert_eq!(tip("Bold (⌘B)"), "Fett (⌘B)");
    assert_eq!(tip("Calculate Sheet (⇧F9)"), "Blatt berechnen (⇧F9)");
    assert_eq!(tip("Bold"), "Fett");
    assert_eq!(tip("Not in the catalog (⌘K)"), "Not in the catalog (⌘K)");
    // A parenthesis that is not a shortcut is part of the text.
    assert_eq!(tip("Something (with a note)"), "Something (with a note)");
    set_current(Lang::EN);
    assert_eq!(tip("Bold (⌘B)"), "Bold (⌘B)");
}

#[test]
fn german_follows_excel_terms() {
    let de = lang("de");
    for (en, want) in [
        ("Home", "Start"),
        ("Insert", "Einfügen"),
        ("Page Layout", "Seitenlayout"),
        ("Formulas", "Formeln"),
        ("Data", "Daten"),
        ("Review", "Überprüfen"),
        ("View", "Ansicht"),
        ("Conditional Formatting", "Bedingte Formatierung"),
        ("Format as Table", "Als Tabelle formatieren"),
        ("Freeze Panes", "Fenster fixieren"),
        ("Sort & Filter", "Sortieren und Filtern"),
    ] {
        assert_eq!(tr(de, en), want);
    }
}

#[test]
fn placeholders_fill_without_reinterpreting_values() {
    let args = [("path", "a-{pages}.xlsx"), ("pages", "2")];
    assert_eq!(fmt("{pages}: {path}; {unknown}", &args), "2: a-{pages}.xlsx; {unknown}");
    assert_eq!(fmt("text {unfinished", &args), "text {unfinished");
}

#[test]
fn malformed_catalog_lines_are_skipped_not_fatal() {
    let (c, errors) = Catalog::parse(
        "# c\n\tHello\tHallo\nctx\tA\tB\n\tOpen…\tÖffnen\n\tPage {n}\tSeite {m}\nno tabs\n\tHello\tNochmal\n\tLine\\nTwo\tEins\\nZwei\n",
    );
    assert_eq!(errors.len(), 5, "{errors:?}");
    assert_eq!(c.plain("Hello"), Some("Hallo"), "the first entry wins");
    assert_eq!(c.plain("Line\nTwo"), Some("Eins\nZwei"));
    assert_eq!(c.plain("Open…"), None);
}

/// Every bundled catalog parses without errors, and its entries keep the English placeholders.
#[test]
fn bundled_catalogs_are_well_formed() {
    let mut codes = std::collections::HashSet::new();
    for l in &LANGUAGES {
        assert!(codes.insert(l.code), "duplicate code {}", l.code);
        assert_eq!(l.code, l.code.to_ascii_lowercase());
        let (entries, errors) = parse_entries(l.source);
        assert!(errors.is_empty(), "{}: {errors:?}", l.code);
        for e in &entries {
            assert_eq!(placeholders(&e.source), placeholders(&e.translation), "{}: {:?}", l.code, e.source);
            assert_eq!(e.translation.trim(), e.translation, "{}: stray whitespace in {:?}", l.code, e.translation);
            assert!(!e.translation.contains("..."), "{}: use … rather than three dots: {:?}", l.code, e.translation);
        }
    }
}

/// The ribbon tabs and every registered command's label and ribbon path are translated, in every
/// language, so menus, tooltips and command search never fall back to English.
#[test]
fn every_tab_and_command_is_translated() {
    let session = gridcraft_engine::Session::new();
    // Labels that read the same in every language.
    let universal = |s: &str| !s.chars().any(char::is_alphabetic);
    let mut missing = Vec::new();
    for l in Lang::all().filter(|l| *l != Lang::EN) {
        let tabs = crate::ribbon::TABS.iter().chain(&["Table Design", "Chart Design"]).copied();
        let commands = session.commands().into_iter().flat_map(|c| std::iter::once(c.label).chain(c.menu));
        for s in tabs.chain(commands).filter(|s| !s.is_empty() && !universal(s)) {
            if !has(l, s) {
                missing.push(format!("{}: {s:?}", l.code()));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "untranslated: {missing:#?}");
}

#[test]
fn the_language_setting_is_a_ui_command_and_persists() {
    let mut app = crate::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
    assert_eq!(app.ui.language, AUTO);
    let r = app.run("ui.language", serde_json::json!({"value": "DE"})).unwrap();
    assert_eq!(app.ui.language, "de");
    assert_eq!(r["effective"], "de");
    assert!(r["available"].as_array().is_some_and(|a| a.len() == LANGUAGES.len()));
    assert!(app.run("ui.language", serde_json::json!({"value": "xx"})).is_err());
    assert_eq!(app.ui.language, "de", "an unknown code keeps the setting");
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: crate::UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.language, "de");
    // Settings saved before this option existed follow the system.
    let legacy: crate::UiState = serde_json::from_str(r#"{"ribbonTab":"Home"}"#).unwrap();
    assert_eq!(legacy.language, AUTO);
    app.run("ui.language", serde_json::json!({"value": "auto"})).unwrap();
    assert_eq!(app.ui.language, AUTO);
    // Reading without a value reports the current setting.
    assert_eq!(app.run("ui.language", serde_json::json!({})).unwrap()["language"], AUTO);
}
