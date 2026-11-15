//! Direct ribbon/menu controls translate their chrome without changing workbook values.

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

fn harness(language: Language) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
    h.run_steps(3);
    h
}

#[test]
fn empty_names_dropdown_uses_the_interface_language() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.get_by_label(language.tr("Defined Names")).click();
        h.run_steps(3);
        h.get_by_label(language.tr("No names defined"));
    }
}

#[test]
fn table_options_are_translated_and_keep_their_commands() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Name", "Amount"], ["Item", 2]]})).unwrap();
        h.state_mut().run("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        h.state_mut().ui.ribbon_tab = "Table Design".into();
        h.run_steps(3);
        for key in ["Table Name:", "Header Row", "Total Row", "Banded Rows", "First Column", "Last Column", "Banded Columns"] {
            h.get_by_label(language.tr(key));
        }
        h.get_by_label(language.tr("Banded Rows")).click();
        h.run_steps(3);
        let table = h.state().session.active().unwrap().wb.active().unwrap().tables.first().unwrap();
        assert!(!table.banded_rows);
        assert_eq!(table.name, "Table1");
    }
}

#[test]
fn quick_access_more_menu_translates_and_keeps_command_binding() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.get_by_label(language.tr("More commands")).click();
        h.run_steps(3);
        for key in ["New Workbook", "Open…", "Save As…", "Sort A to Z", "Calculate Now", "Print…"] {
            h.get_by_label(language.tr(key));
        }
        let before = h.state().session.documents().len();
        h.get_by_label(language.tr("New Workbook")).click();
        h.run_steps(3);
        assert_eq!(h.state().session.documents().len(), before + 1);
    }
}

#[test]
fn undo_menu_translates_its_template_and_preserves_history_labels() {
    for language in [Language::En, Language::Tr] {
        let mut h = harness(language);
        h.state_mut().run("home.bold", json!({})).unwrap();
        h.run_steps(3);
        h.get_by_label(language.tr("Undo list")).click();
        h.run_steps(3);
        let expected = language.tr("Undo {label}").replace("{label}", language.tr("Bold"));
        h.get_by_label(&expected).click();
        h.run_steps(3);
        let document = h.state().session.active().unwrap();
        assert!(document.undo.is_empty());
        assert_eq!(document.redo.last().unwrap().label, "Bold");

        let mut h = harness(language);
        let label = "Budget|2026";
        let group = h.state_mut().session.group_begin(label).unwrap();
        h.state_mut().run("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
        h.state_mut().session.group_end(group).unwrap();
        h.run_steps(3);
        h.get_by_label(language.tr("Undo list")).click();
        h.run_steps(3);
        let expected = language.tr("Undo {label}").replace("{label}", label);
        h.get_by_label(&expected).click();
        h.run_steps(3);
        let document = h.state().session.active().unwrap();
        assert!(document.undo.is_empty());
        assert_eq!(document.redo.last().unwrap().label, label);
    }
}
