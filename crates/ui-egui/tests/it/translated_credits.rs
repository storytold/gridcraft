//! Credits translate presentation text while keeping names, models and sort behavior intact.

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::credits::{CONTRIBUTORS, MODELS, NameMode, SortKey};
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

#[test]
fn translated_credits_controls_preserve_names_and_table_sorting() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::common::harness_with(
            crate::common::builder().with_size(egui::vec2(1800.0, 1100.0)),
            crate::common::app(crate::support::blank_session()),
            4,
        );
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("about", json!({}));
        h.run_steps(3);
        h.get_by_label(lang.tr("Contributors")).click();
        h.run_steps(3);
        assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Contributors");
        for key in ["Show", "Sort", "Grab bag", "Table"] {
            assert!(h.query_all_by_label(lang.tr(key)).next().is_some(), "missing {key} in {lang:?}");
        }
        for mode in NameMode::ALL {
            h.get_by_label(lang.tr(mode.label())).click();
            h.run_steps(2);
            for contributor in CONTRIBUTORS {
                assert!(h.query_by_label(&contributor.name(mode)).is_some());
            }
        }
        h.get_by_role_and_label(egui::accesskit::Role::ComboBox, lang.tr("Sort")).click();
        h.run_steps(2);
        for key in SortKey::ALL {
            assert!(h.query_all_by_label(lang.tr(key.label().0)).next().is_some());
        }
        h.get_by_role_and_label(egui::accesskit::Role::Button, lang.tr("Name (A–Z)")).click();
        h.run_steps(2);
        h.get_by_label(lang.tr("Table")).click();
        h.run_steps(2);
        if !CONTRIBUTORS.is_empty() {
            let name_header = lang.tr("Name");
            h.get_by_label(&format!("{name_header} ▲")).click();
            h.run_steps(2);
            assert!(h.query_by_label(&format!("{name_header} ▼")).is_some());
            for key in SortKey::ALL.into_iter().filter(|key| *key != SortKey::Name) {
                assert!(h.query_all_by_label(lang.tr(key.label().1)).next().is_some());
            }
        } else {
            assert!(h.query_by_label(lang.tr("No contributor data was built into this copy.")).is_some());
        }
        h.get_by_label(lang.tr("Models")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Models");
        if MODELS.is_empty() {
            assert!(h.query_by_label(lang.tr("No model credits were built into this copy.")).is_some());
        } else {
            for header in ["Company", "Model", "Version", "Commits", "% of all commits", "Lines +/−"] {
                assert!(h.query_by_label(lang.tr(header)).is_some());
            }
            for model in MODELS {
                assert!(h.query_all_by_label(model.model).next().is_some());
                assert!(h.query_all_by_label(model.company).next().is_some());
            }
        }
    }
}
