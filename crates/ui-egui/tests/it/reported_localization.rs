//! Regressions from manual Russian interface review: chrome, generated names and file semantics.
use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::{SheetApp, i18n::Language};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("app.language.set", json!({"language":"ru"})).unwrap();
    h
}

/// Language-dependent generated names are real document names, unique and stable on a language switch.
#[test]
fn generated_names_are_localized_and_existing_names_stay_unchanged() {
    let mut h = harness();
    h.state_mut().run("file.new", json!({})).unwrap();
    assert_eq!(h.state().session.active().unwrap().title, "Книга1");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Лист1");
    h.state_mut().run("home.insertSheet", json!({})).unwrap();
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Лист2");
    h.state_mut().run("insert.table", json!({"range":"A1:B2","header":false})).unwrap();
    let doc = h.state().session.active().unwrap();
    let table = &doc.wb.active().unwrap().tables[0];
    assert_eq!(table.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Столбец1", "Столбец2"]);
    let saved = h.state_mut().run("file.save", json!({"format":"xlsx"})).unwrap();
    h.state_mut().run("file.open", json!({"name":"localized.xlsx", "base64":saved["base64"]})).unwrap();
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Лист2");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().tables[0].columns[0].name, "Столбец1");
    h.state_mut().run("app.language.set", json!({"language":"en"})).unwrap();
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Лист2");
    h.state_mut().run("file.new", json!({})).unwrap();
    assert!(h.state().session.active().unwrap().title.starts_with("Book"));
}

/// Search labels and macro/name-manager controls go through the same catalog as their window titles.
#[test]
fn reported_dialogs_translate_their_contents() {
    for (dialog, labels) in [
        ("find", vec!["Find what:", "Match case", "Find All", "Find Next"]),
        ("nameManager", vec!["New…", "Name", "Value", "Refers To", "Scope", "Close"]),
        ("recordMacro", vec!["Macro name:", "Shortcut key:", "Description:", "Use Relative References"]),
    ] {
        let mut h = harness();
        h.state_mut().open_dialog(dialog, json!({}));
        h.run_steps(4);
        for label in labels {
            let translated = Language::Ru.tr(label);
            assert_ne!(translated, label, "missing catalog row: {label}");
            assert!(h.query_by_label_contains(translated).is_some(), "{dialog}: missing {translated}");
        }
    }
}

/// A real engine error is localized when drawn; API error text is still canonical English.
#[test]
fn empty_clipboard_message_is_translated_at_display_time() {
    let mut h = harness();
    let error = h.state_mut().run("edit.paste", json!({})).unwrap_err();
    assert!(error.contains("Nothing to paste."));
    h.state_mut().message = Some(("GridCraft".into(), error));
    h.run_steps(3);
    assert!(h.query_by_label_contains(Language::Ru.tr("Nothing to paste.")).is_some());
}

/// An unknown function produces a typed NAME error, displayed with Russian error spelling.
#[test]
fn russian_formula_failure_keeps_the_typed_error() {
    use gridcraft_engine::core::{CellError, CellRef, Value};
    let mut h = harness();
    h.state_mut().begin_edit(Some("=НЕСУЩЕСТВУЮЩАЯ(1)".into()), false);
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Error(CellError::Name));
    h.state_mut().begin_edit(None, false);
    assert!(h.state().editor.as_ref().unwrap().text.contains("НЕСУЩЕСТВУЮЩАЯ"));
}

/// The function help's search field and cancellation keep their localized accessible controls in every language.
#[test]
fn function_dialog_controls_follow_each_interface_language() {
    for language in Language::ALL {
        let mut h = harness();
        h.state_mut().ui.language = language;
        h.state_mut().open_dialog("insertFunction", json!({"category":"Financial"}));
        h.run_steps(4);
        h.get_by_role_and_label(egui::accesskit::Role::TextInput, language.tr("Search:"));
        h.get_by_role_and_label(egui::accesskit::Role::Button, &format!(" {} ", language.tr("Cancel"))).click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none(), "{language:?}: Cancel must close the function dialog");
    }
}
