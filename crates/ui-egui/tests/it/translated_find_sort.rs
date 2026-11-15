//! Translated Find/Replace and Sort controls still submit the engine's stable values.

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(language: Language) -> Harness {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().ui.language = language;
    h
}

#[test]
fn find_replace_translates_count_templates_before_substitution() {
    for (lang, found, replaced, zero) in [
        (Language::Tr, "2 hücre bulundu", "Tamamlandı. 2 değiştirme yapıldı.", "Tamamlandı. 0 değiştirme yapıldı."),
        (Language::En, "2 cell(s) found", "All done. We made 2 replacements.", "All done. We made 0 replacements."),
    ] {
        let mut h = harness(lang);
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["apple"], ["apple"], ["pear"]]})).unwrap();
        h.state_mut().open_dialog("find", json!({}));
        h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!("apple"));
        h.run_steps(3);
        for label in ["Find", "Replace", "Match case", "Match entire cell contents", "Within workbook", "Find All", "Find Next"] {
            assert!(h.query_by_label(lang.tr(label)).is_some(), "missing {label} in {lang:?}");
        }
        h.get_by_label(lang.tr("Match case")).click();
        h.run_steps(2);
        h.get_by_label(lang.tr("Find All")).click();
        h.run_steps(3);
        assert!(h.query_by_label(found).is_some());
        assert_eq!(h.state().dialog.as_ref().unwrap().result.as_ref().unwrap()["results"].as_array().unwrap().len(), 2);
        h.get_by_label(lang.tr("Replace")).click();
        h.run_steps(3);
        assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Replace");
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("with".into(), json!("orange"));
        assert_eq!(d.values["within"], "sheet");
        assert_eq!(d.values["matchCase"], true);
        h.get_by_label(lang.tr("Replace All")).click();
        h.run_steps(3);
        assert_eq!(h.state().toast.as_ref().unwrap().0, replaced);
        assert!(h.query_by_label(replaced).is_some());
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A1"})).unwrap()["value"], "orange");
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A2"})).unwrap()["value"], "orange");
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A3"})).unwrap()["value"], "pear");
        h.get_by_label(lang.tr("Replace All")).click();
        h.run_steps(3);
        assert_eq!(h.state().toast.as_ref().unwrap().0, zero);
    }
}

#[test]
fn translated_sort_controls_preserve_keys_and_sort_data() {
    for lang in [Language::Tr, Language::En] {
        let mut h = harness(lang);
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["apple", 1], ["pear", 2]]})).unwrap();
        h.state_mut().run("selection.set", json!({"cell": "A1"})).unwrap();
        h.state_mut().open_dialog("sort", json!({}));
        h.run_steps(3);
        for label in ["My list has headers", "Sort by", "Sort On", "A to Z", "Z to A", "Remove level", "+ Add Level"] {
            assert!(h.query_by_label(lang.tr(label)).is_some(), "missing {label} in {lang:?}");
        }
        h.get_by_label(lang.tr("Sort On")).click();
        h.run_steps(2);
        for label in ["Cell Values", "Cell Color", "Font Color"] {
            assert!(h.query_all_by_label(lang.tr(label)).next().is_some());
        }
        h.get_by_role_and_label(egui::accesskit::Role::Button, lang.tr("Cell Color")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["levels"][0]["by"], "cellColor");
        h.get_by_label(lang.tr("Sort On")).click();
        h.run_steps(2);
        h.get_by_role_and_label(egui::accesskit::Role::Button, lang.tr("Cell Values")).click();
        h.run_steps(2);
        h.get_by_label(lang.tr("Z to A")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["levels"][0], json!({"column": "A", "by": "values", "order": "desc"}));
        h.get_by_label(lang.tr("+ Add Level")).click();
        h.run_steps(2);
        assert!(h.query_by_label(lang.tr("Then by")).is_some());
        assert_eq!(h.state().dialog.as_ref().unwrap().values["levels"][1]["column"], "B");
        h.get_by_label(&format!("   {}   ", lang.tr("OK"))).click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A1"})).unwrap()["value"], "Name");
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A2"})).unwrap()["value"], "pear");
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "B2"})).unwrap()["value"].as_f64(), Some(2.0));
    }
}
