//! Page Break Preview labels repaint in every interface language with cached pagination.
use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// Opens a synthetic paginated sheet using the existing Page Break Preview commands.
fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("pageLayout.pageSetup", json!({"printArea":"B2:L30","rowBreaks":[12],"colBreaks":["G"]})).unwrap();
    session.execute("view.pageBreakPreview", json!({})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 1100.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Page labels repaint in every interface language without changing the cached sheet pagination.
#[test]
fn page_labels_follow_every_interface_language() {
    use gridcraft_ui_egui::i18n::Language;
    let mut h = harness();
    let translations = [
        (Language::En, "Page 1"),
        (Language::Es, "Página 1"),
        (Language::Zh, "第 1 页"),
        (Language::Ja, "1 ページ"),
        (Language::Ko, "1 페이지"),
        (Language::Ru, "Страница 1"),
        (Language::PtBr, "Página 1"),
        (Language::De, "Seite 1"),
        (Language::Cs, "Stránka 1"),
        (Language::Fr, "Page 1"),
        (Language::Sr, "Stranica 1"),
    ];
    assert_eq!(translations.len(), Language::ALL.len());
    for (language, label) in translations {
        h.state_mut().ui.language = language;
        h.run_steps(4);
        assert!(
            h.output().shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==label)),
            "{language:?}: {label} must be painted"
        );
    }
}

/// A language command repaints cached page labels on the first frame without a font-order change.
#[test]
fn page_label_repaints_on_language_change_with_cached_pagination() {
    let mut h = harness();
    assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Page 1")));
    h.state_mut().run("app.language.set", json!({"language":"es"})).unwrap();
    h.run_steps(1);
    assert!(
        h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Página 1")),
        "the translated label must be painted on the first frame after the language change"
    );
}
