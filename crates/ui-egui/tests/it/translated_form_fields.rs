//! Form dialogs draw their field labels and choices in the interface language, as their titles
//! and OK/Cancel buttons already were (Page Setup showed "Left margin (in):" in every language).

use egui::accesskit::Role;
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
fn analysis_overwrite_prompt_is_translated_and_preserves_actions() {
    for (language, prompt, replace, cancel) in [
        ("en", "The output range already holds data. Replace it with the report?", "Replace", "Cancel"),
        ("tr", "Çıkış aralığında zaten veri var. Raporla değiştirilsin mi?", "Değiştir", "İptal"),
    ] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": language})).unwrap();
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
        h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
        let d = h.state_mut().dialog.as_mut().unwrap();
        for (key, value) in [("range", json!("A1:A3")), ("outputTo", json!("range")), ("output", json!("C1")), ("askOverwrite", json!(true))] {
            d.values.insert(key.into(), value);
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains(prompt).is_some());
        h.get_by_role_and_label(Role::Button, cancel).click();
        h.run_steps(2);
        let d = h.state_mut().dialog.as_mut().unwrap();
        assert!(!d.values.contains_key("askOverwrite"));
        assert_ne!(d.values.get("overwrite"), Some(&json!(true)));
        d.values.insert("askOverwrite".into(), json!(true));
        h.run_steps(2);
        h.get_by_role_and_label(Role::Button, replace).click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none(), "{language}: Replace still runs the report");
        assert!(h.state().message.is_none());
    }
}

#[test]
fn select_data_is_translated_without_changing_series_data_or_actions() {
    for (language, range, legend, categories, series, remove, keep) in [
        ("en", "Chart data range:", "Legend Entries (Series)", "Horizontal (Category) Axis Labels:", "Series1", "Remove", "Keep"),
        ("tr", "Grafik veri aralığı:", "Gösterge Girişleri (Seriler)", "Yatay (Kategori) Eksen Etiketleri:", "Seri1", "Kaldır", "Koru"),
    ] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": language})).unwrap();
        h.state_mut()
            .run("range.setValues", json!({"range": "A1", "values": [["Quarter", "North", "South"], ["Q1", 10, 30], ["Q2", 30, 10]]}))
            .unwrap();
        h.state_mut().run("selection.set", json!({"range": "A1:C3"})).unwrap();
        let chart = h.state_mut().run("insert.chart", json!({"type": "column"})).unwrap();
        h.state_mut().open_dialog("selectData", json!({"chart": chart["chart"]}));
        h.state_mut().dialog.as_mut().unwrap().values.insert("names".into(), json!(["", "Revenue|2026"]));
        h.run_steps(3);
        for label in [range, legend, categories, series, "Revenue|2026"] {
            assert!(h.query_by_label_contains(label).is_some(), "{language}: missing {label}");
        }
        h.get_all_by_role_and_label(Role::Button, remove).next().unwrap().click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["remove"], json!([0]));
        h.get_by_role_and_label(Role::Button, keep).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["remove"], json!([]));
        h.get_all_by_role_and_label(Role::Button, remove).next().unwrap().click();
        h.run_steps(2);
        h.get_by_role_and_label(Role::Button, "   OK   ").click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
        assert!(h.state().message.is_none());
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().charts[0].series.len(), 1);
    }
}
