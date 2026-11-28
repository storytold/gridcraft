//! Ribbon drop-down menus draw their items in the interface language, as the buttons that open
//! them already did (Insert › Shapes listed "Rectangle", "Oval"… in English in every language).

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(4)
}

#[test]
fn insert_menus_are_translated() {
    for (button, items) in [("Shapes", ["Rounded Rectangle", "Triangle"]), ("Pictures", ["Place in Cell", "Place over Cells"])] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": "pt"})).unwrap();
        h.state_mut().ui.ribbon_tab = "Insert".into();
        h.run_steps(6);
        h.get_by_label(Language::PtBr.tr(button)).click();
        h.run_steps(6);
        for english in items {
            let translated = Language::PtBr.tr(english);
            assert_ne!(translated, english, "{button}: no Portuguese for {english}");
            assert!(h.query_by_label(translated).is_some(), "{button}: no {translated}");
            assert!(h.query_by_label(english).is_none(), "{button}: {english} is untranslated");
        }
    }
}
