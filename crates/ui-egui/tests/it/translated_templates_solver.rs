//! Parameterized prompts and Solver controls use the selected interface language.

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

fn harness(language: Language) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
    h
}

#[test]
fn save_templates_translate_before_inserting_user_values() {
    for language in [Language::En, Language::Tr] {
        for (dialog, params, expected) in [
            (
                "saveChanges",
                json!({"title": "Budget|2026.xlsx"}),
                language.tr("Do you want to save the changes you made to {title}?").replace("{title}", "Budget|2026.xlsx"),
            ),
            (
                "saveChanges",
                json!({}),
                language.tr("Do you want to save the changes you made to {title}?").replace("{title}", language.tr("this workbook")),
            ),
            ("saveCopy", json!({"format": "csv"}), language.tr("Save a copy as .{format}").replace("{format}", "csv")),
            ("saveCopy", json!({}), language.tr("Save a copy as .{format}").replace("{format}", "xlsx")),
        ] {
            let mut h = harness(language);
            h.state_mut().open_dialog(dialog, params);
            h.run_steps(3);
            h.get_by_label(&expected);
        }
    }
}

#[test]
fn goal_seek_success_translates_the_template_and_keeps_the_value() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.state_mut().run("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
        h.state_mut().run("cell.set", json!({"cell": "B1", "input": "=A1*2"})).unwrap();
        h.state_mut().open_dialog("goalSeek", json!({}));
        h.state_mut().dialog.as_mut().unwrap().values.extend([
            ("set".into(), json!("B1")),
            ("to".into(), json!("10")),
            ("changing".into(), json!("A1")),
        ]);
        h.run_steps(3);
        h.get_by_label_contains(language.tr("OK")).click();
        h.run_steps(3);
        let toast = &h.state().toast.as_ref().unwrap().0;
        let prefix = language.tr("Goal Seeking found a solution: {value}").replace("{value}", "");
        assert!(toast.starts_with(&prefix), "{toast}");
        let value: f64 = toast.strip_prefix(&prefix).unwrap().parse().unwrap();
        assert!((value - 5.0).abs() < 0.001, "{toast}");
    }
}

#[test]
fn solver_controls_translate_without_changing_model_values() {
    let mut h = harness(Language::Tr);
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(3);
    for key in [
        "Set Objective:",
        "Max",
        "Value Of:",
        "By Changing Variable Cells:",
        "Subject to the Constraints:",
        "Add",
        "Delete",
        "Make Unconstrained Variables Non-Negative",
        "Select a Solving Method:",
        "GRG Nonlinear",
        "Solve",
        "Close",
    ] {
        assert!(h.query_all_by_label_contains(Language::Tr.tr(key)).next().is_some(), "missing translated {key}");
    }
    assert!(h.query_by_label("Hedef:").is_some());
    h.get_by_role_and_label(egui::accesskit::Role::RadioButton, Language::Tr.tr("Value Of:")).click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["goal"], "value");
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, Language::Tr.tr("Select a Solving Method:")).click();
    h.run_steps(2);
    h.get_by_label(Language::Tr.tr("Evolutionary")).click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["method"], "evolutionary");
    let dialog = h.state_mut().dialog.as_mut().unwrap();
    dialog.values.insert("newLhs".into(), json!("A1"));
    dialog.values.insert("newRhs".into(), json!("10"));
    h.query_all_by_label(Language::Tr.tr("Add")).last().expect("Solver Add button").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["constraints"], json!(["A1 <= 10"]));
}

#[test]
fn solver_results_translate_choices_and_keep_report_ids() {
    let mut h = harness(Language::Tr);
    h.state_mut().open_dialog("solverResults", json!({"message": "Solver completed", "changed": true, "values": {"A1": 5}}));
    h.run_steps(3);
    for key in ["Keep Solver Solution", "Restore Original Values", "Reports:", "Answer", "Limits"] {
        assert!(h.query_by_label(Language::Tr.tr(key)).is_some(), "missing translated {key}");
    }
    h.get_by_label(Language::Tr.tr("Restore Original Values")).click();
    h.run_steps(2);
    h.get_by_label(Language::Tr.tr("Answer")).click();
    h.run_steps(2);
    let values = &h.state().dialog.as_ref().unwrap().values;
    assert_eq!(values["restore"], true);
    assert_eq!(values["answerReport"], true);
}

#[test]
fn analysis_toolpak_uses_translated_tool_names_and_output_controls() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.state_mut().open_dialog("analysisToolPak", json!({}));
        h.run_steps(3);
        h.get_by_label(language.tr("Analysis Tools"));
        h.get_by_label(language.tr("Descriptive Statistics")).click();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().values["pick"], "descriptive");
        h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
        h.run_steps(3);
        for label in ["Output options", "Output Range:", "New Worksheet Ply:"] {
            h.get_by_label(language.tr(label));
        }
    }
}
