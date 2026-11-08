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
