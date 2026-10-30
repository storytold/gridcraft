//! Form dialogs draw their field labels and choices in the interface language, as their titles
//! and OK/Cancel buttons already were (Page Setup showed "Left margin (in):" in every language).

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(4)
}

#[test]
fn form_fields_are_translated() {
    for (dialog, labels) in [("pageSetup", ["Left margin (in):", "Footer (in):"]), ("insertPaste", ["Shift cells right", "Shift cells down"])] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": "pt"})).unwrap();
        h.state_mut().open_dialog(dialog, json!({}));
        h.run_steps(3);
        for english in labels {
            let translated = Language::PtBr.tr(english);
            assert!(h.query_by_label_contains(translated).is_some(), "{dialog}: no {translated}");
            if translated != english {
                assert!(h.query_by_label_contains(english).is_none(), "{dialog}: {english} is untranslated");
            }
        }
    }
}

#[test]
fn notes_with_a_pipe_show_in_full() {
    // `tr` reads `|` as a context separator; the Open Hyperlink note carries the URL, which must
    // be shown whole so the user sees where the link really goes.
    let url = "https://example.com/report|evil.example";
    for language in ["en", "pt"] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": language})).unwrap();
        h.state_mut().open_dialog("openLink", json!({"url": url}));
        h.run_steps(3);
        assert!(h.query_by_label_contains(url).is_some(), "{language}: the link note lost part of {url}");
    }
}

#[test]
fn chart_format_pane_is_translated() {
    let mut h = harness();
    let session = &mut h.state_mut().session;
    session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    session.execute("cell.set", json!({"cell": "A2", "input": "2"})).unwrap();
    session.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "D3"})).unwrap();
    h.state_mut().run("app.language.set", json!({"language": "fr"})).unwrap();
    h.state_mut().selected_chart = Some(chart["chart"].as_u64().unwrap() as u32);
    h.state_mut().grid.pane = Some("formatChart".into());
    h.run_steps(2);
    // One section at a time: open together they overflow the pane and the clipped widgets leave the tree.
    let sections: [(&str, &[&str]); 7] = [
        ("Chart Title", &[]),
        ("Chart Type", &["Stacked Column", "Doughnut"]),
        ("Legend", &["Legend|None", "Bottom", "Top", "Left", "Right"]),
        ("Elements", &["Data Labels", "Gridlines", "Horizontal axis title:"]),
        ("Series", &["Fill…"]),
        ("Colors & Layout", &["Colorful", "Monochromatic", "Accent1", "Layout {n}"]),
        ("Size", &[]),
    ];
    let triangle = egui::accesskit::Role::DisclosureTriangle;
    for (section, labels) in sections {
        h.get_by_role_and_label(triangle, Language::Fr.tr(section)).click();
        h.run_steps(2);
        for english in labels {
            let translated = Language::Fr.tr(english).replace("{n}", "1");
            assert!(h.query_all_by_label(&translated).next().is_some(), "{section}: no {translated}");
            let english = english.rsplit_once('|').map_or(*english, |(_, label)| label).replace("{n}", "1");
            assert!(h.query_all_by_label(&english).next().is_none(), "{section}: {english} is untranslated");
        }
        h.get_by_role_and_label(triangle, Language::Fr.tr(section)).click();
        h.run_steps(2);
    }
}

#[test]
fn custom_dialogs_are_translated() {
    let dialogs: [(&str, &[&str]); 3] = [
        ("find", &["Find what:", "Match case", "Match entire cell contents", "Find All", "Find Next"]),
        ("goalSeek", &["Set cell:", "To value:", "By changing cell:"]),
        ("nameManager", &["New…", "Edit…"]),
    ];
    for (dialog, labels) in dialogs {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": "fr"})).unwrap();
        h.state_mut().open_dialog(dialog, json!({}));
        h.run_steps(3);
        for english in labels {
            let translated = Language::Fr.tr(english);
            assert_ne!(translated, *english, "{dialog}: no French for {english}");
            assert!(h.query_all_by_label_contains(translated).next().is_some(), "{dialog}: no {translated}");
            assert!(h.query_all_by_label(english).next().is_none(), "{dialog}: {english} is untranslated");
        }
    }
}

#[test]
fn command_search_matches_translated_and_english_labels() {
    // (typed, a result that must show): French labels, typed without case or accents, and English.
    for (typed, shown) in [
        ("gras", "Gras"),
        ("figer", "Figer les volets"),
        ("graphique", "graphique"),
        ("bold", "Gras"),
        ("INSERER UN GRAPH", "Insérer un graphique"),
        ("accueil", "Accueil"),
    ] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": "fr"})).unwrap();
        h.state_mut().open_dialog("commandSearch", json!({}));
        h.run_steps(6);
        h.input_mut().events.push(egui::Event::Text(typed.into()));
        h.run_steps(3);
        // Inside the dialog: the ribbon's Bold button also reads "Gras".
        let dialog = h.get_by_role_and_label(egui::accesskit::Role::Window, Language::Fr.tr("Search Commands"));
        assert!(dialog.query_all_by_label_contains(shown).next().is_some(), "{typed}: no result showing {shown}");
    }
}
