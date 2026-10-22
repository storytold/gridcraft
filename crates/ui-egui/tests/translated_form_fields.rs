//! Form dialogs draw their field labels and choices in the interface language, as their titles
//! and OK/Cancel buttons already were (Page Setup showed "Left margin (in):" in every language).

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

#[test]
fn form_fields_are_translated() {
    for (dialog, translated, english) in [
        ("pageSetup", ["Margem esquerda (pol.):", "Rodapé (pol.):"], ["Left margin (in):", "Footer (in):"]),
        ("insertPaste", ["Deslocar células para a direita", "Deslocar células para baixo"], ["Shift cells right", "Shift cells down"]),
    ] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": "pt"})).unwrap();
        h.state_mut().open_dialog(dialog, json!({}));
        h.run_steps(3);
        for label in translated {
            assert!(h.query_by_label_contains(label).is_some(), "{dialog}: no {label}");
        }
        for label in english {
            assert!(h.query_by_label_contains(label).is_none(), "{dialog}: {label} is untranslated");
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
