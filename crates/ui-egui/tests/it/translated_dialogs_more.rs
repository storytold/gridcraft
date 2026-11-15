//! Translated dialog controls preserve command IDs, data and internal selection values.

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

#[test]
fn translated_spelling_actions_keep_suggestions_and_change_cells() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().run("cell.set", json!({"cell": "A1", "input": "helo"})).unwrap();
        h.state_mut().open_dialog("spelling", json!({}));
        h.state_mut().dialog.as_mut().unwrap().result = Some(json!({"issues": [
            {"cell": "A1", "word": "helo", "suggestions": ["hello"]}
        ]}));
        h.run_steps(3);
        assert!(h.query_by_label(&lang.tr("Not in Dictionary ({cell}):").replace("{cell}", "A1")).is_some());
        for label in ["Suggestions:", "Ignore Once", "Add to Dictionary"] {
            assert!(h.query_by_label(lang.tr(label)).is_some());
        }
        h.get_by_label("hello").click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["pick"], "hello");
        h.get_by_label(lang.tr("Change")).click();
        h.run_steps(3);
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A1"})).unwrap()["value"], "hello");
        assert!(h.query_by_label(lang.tr("The spelling check is complete for the entire sheet.")).is_some());
        h.get_by_label(lang.tr("OK")).click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
    }
}

#[test]
fn translated_name_manager_buttons_edit_and_delete_the_original_name() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("nameManager", json!({}));
        h.run_steps(3);
        for label in ["Name", "Value", "Refers To", "Scope", "No names defined"] {
            assert!(h.query_all_by_label(lang.tr(label)).next().is_some());
        }
        h.get_by_label(lang.tr("New…")).click();
        h.run_steps(2);
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("name".into(), json!("Total"));
        d.values.insert("refersTo".into(), json!("=Sheet1!$A$1"));
        h.run_steps(2);
        h.get_by_label(&format!("   {}   ", lang.tr("OK"))).click();
        h.run_steps(3);
        assert_eq!(h.state().dialog.as_ref().unwrap().name(), "nameManager");
        assert!(h.query_by_label(lang.tr("Workbook")).is_some());
        h.get_by_label(lang.tr("Edit…")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["oldName"], "Total");
        h.state_mut().dialog.as_mut().unwrap().values.insert("name".into(), json!("GrandTotal"));
        h.run_steps(2);
        h.get_by_label(&format!("   {}   ", lang.tr("OK"))).click();
        h.run_steps(3);
        assert!(h.query_by_label("GrandTotal").is_some());
        h.get_by_role_and_label(egui::accesskit::Role::Button, lang.tr("Delete")).click();
        h.run_steps(3);
        assert!(h.state().session.active().unwrap().wb.names.is_empty());
        h.get_by_label(&format!(" {} ", lang.tr("Close"))).click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
    }
}

#[test]
fn translated_macro_list_substitutes_counts_without_translating_script_names() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("macros", json!({}));
        h.run_steps(3);
        assert!(h.query_by_label(lang.tr("This workbook has no scripts or macros.")).is_some());
        h.state_mut().dialog.as_mut().unwrap().result = Some(json!([
            {"name": "Original Script", "kind": "script", "steps": 1, "trusted": false},
            {"kind": "script", "steps": 2, "trusted": false},
            {"name": "VBA", "kind": "vba"}
        ]));
        h.run_steps(3);
        for label in
            ["Trust Scripts", "Scripts in this workbook came from a file and are disabled.", "(unnamed)", "— VBA project (preserved, not run)"]
        {
            assert!(h.query_by_label(lang.tr(label)).is_some());
        }
        assert!(h.query_by_label("Original Script").is_some());
        for (key, count) in [("— {steps} step", "1"), ("— {steps} steps", "2")] {
            assert!(h.query_by_label(&lang.tr(key).replace("{steps}", count)).is_some());
        }
        assert_eq!(h.state().dialog.as_ref().unwrap().result.as_ref().unwrap()[0]["trusted"], false);
    }
}

#[test]
fn translated_command_search_runs_the_stable_command_and_about_keeps_tab_keys() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("commandSearch", json!({}));
        h.state_mut().dialog.as_mut().unwrap().search = lang.tr("Bold").to_string();
        h.run_steps(3);
        let label = format!("{}    {}", lang.tr("Bold"), ["Home", "Font"].map(|part| lang.tr(part)).join(" › "));
        h.get_by_label(&label).click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state().session.journal.last().unwrap().0, "home.bold");
        h.state_mut().open_dialog("about", json!({}));
        h.run_steps(3);
        for label in ["About", "Contributors", "Models", "Join the ArtCraft Discord"] {
            assert!(h.query_by_label(lang.tr(label)).is_some());
        }
        h.get_by_label(lang.tr("Models")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Models");
    }
}

#[test]
fn translated_rule_summary_keeps_ranges_and_delete_removes_the_rule() {
    for lang in [Language::Tr, Language::En] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("manageRules", json!({}));
        h.run_steps(3);
        assert!(h.query_by_label(lang.tr("There are no conditional formatting rules on this sheet.")).is_some());
        h.state_mut()
            .run(
                "home.conditionalFormat",
                json!({"range": "A1:A5", "rule": {
                    "type": "expression", "formula": "A1>5", "preset": "lightRedFill"
                }}),
            )
            .unwrap();
        h.run_steps(3);
        let label = lang
            .tr("{number}. {kind}  applies to {ranges}")
            .replace("{number}", "1")
            .replace("{kind}", lang.tr("Formula"))
            .replace("{ranges}", "A1:A5");
        assert!(h.query_by_label(&label).is_some());
        h.get_by_role_and_label(egui::accesskit::Role::Button, lang.tr("Delete")).click();
        h.run_steps(3);
        assert!(h.state_mut().run("home.manageRules", json!({})).unwrap()["rules"].as_array().unwrap().is_empty());
    }
}

#[test]
fn turkish_command_search_matches_uppercase_dotless_i_and_keeps_english_ids_searchable() {
    for (query, command_label, command_id) in [("KALIN", "Bold", "home.bold"), ("HOME.ITALIC", "Italic", "home.italic")] {
        let mut h = crate::support::blank_harness(4);
        let lang = Language::Tr;
        h.state_mut().ui.language = lang;
        h.state_mut().open_dialog("commandSearch", json!({}));
        h.state_mut().dialog.as_mut().unwrap().search = query.into();
        h.run_steps(3);
        let label = format!("{}    {}", lang.tr(command_label), ["Home", "Font"].map(|part| lang.tr(part)).join(" \u{203a} "));
        h.get_by_label(&label).click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state().session.journal.last().unwrap().0, command_id);
    }
}
