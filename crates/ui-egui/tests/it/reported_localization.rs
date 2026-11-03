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

/// Formula localization must not rewrite upstream Python code or retain it when a literal replaces the cell.
#[test]
fn localized_editor_preserves_python_code_and_literal_fill_replaces_it() {
    use gridcraft_engine::core::{CellRef, Value};
    let mut h = harness();
    let code = "x = 1.5\nprint(\"SUM\", x)";
    h.state_mut().run("python.set", json!({"cell":"A1", "code":code})).unwrap();
    h.state_mut().begin_edit(None, true);
    let editor = h.state_mut().editor.as_mut().unwrap();
    assert!(editor.python);
    assert_eq!(editor.text, code);
    let edited = format!("{code}\n# unchanged decimal and comma");
    editor.text = edited.clone();
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    let cell = h.state().session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap();
    assert_eq!(cell.py.as_ref().unwrap().code, edited);
    assert!(cell.formula.is_none());
    h.state_mut().run("range.fill", json!({"range":"A1", "input":"1,5", "literal":true})).unwrap();
    let cell = h.state().session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap();
    assert!(cell.py.is_none());
    assert_eq!(cell.value, Value::text("1,5"));
}

/// Invalid generated sheet names are rejected before a document is created; valid apostrophes remain allowed.
#[test]
fn generated_first_sheet_name_respects_the_sheet_name_contract() {
    let mut session = gridcraft_engine::Session::new();
    for prefix in ["'Data", "'", "Bad/Name"] {
        assert!(session.execute("file.new", json!({"sheetPrefix":prefix})).is_err());
        assert!(session.documents().is_empty());
    }
    session.execute("file.new", json!({"bookPrefix":"'Book", "sheetPrefix":"Data'"})).unwrap();
    let wb = &session.active().unwrap().wb;
    let name = &wb.active().unwrap().name;
    assert_eq!(name, "Data'1");
    assert!(wb.check_sheet_name(name, Some(0)).is_ok());
}

/// Native-language blank filtering matches the visible caption and preserves the engine's blank criterion.
#[test]
fn blank_filter_search_uses_its_localized_label() {
    use egui::{Event, Key, Modifiers};
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range":"A1", "values":[["Header"],["Apple"],[null],["Banana"]]})).unwrap();
    h.state_mut().run("data.filter", json!({"range":"A1:A4"})).unwrap();
    h.state_mut().run("selection.set", json!({"cell":"A1"})).unwrap();
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key: Key::ArrowDown,
            physical_key: Some(Key::ArrowDown),
            pressed,
            repeat: false,
            modifiers: Modifiers::ALT,
        });
    }
    h.run_steps(4);
    let label = Language::Ru.tr("(Blanks)");
    assert_ne!(label, "(Blanks)");
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, label);
    h.input_mut().events.push(Event::Text(label.trim_matches(['(', ')']).into()));
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, label);
    assert!(h.query_by_label("Apple").is_none());
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1));
    assert!(!sh.is_row_hidden(2));
    assert!(sh.is_row_hidden(3));
}

/// Searches typed Russian Boolean/error captions but sends the canonical values to the engine.
#[test]
fn filter_search_localizes_constants_without_translating_text() {
    use egui::{Event, Key, Modifiers, accesskit::Role};
    for (caption, visible_row) in [("ИСТИНА", 1), ("#ДЕЛ/0!", 4)] {
        let mut h = harness();
        h.state_mut().run("range.setValues", json!({"range":"A1", "values":[["Header"],[true],[false],["FALSE"]], "parse":false})).unwrap();
        h.state_mut().run("cell.set", json!({"cell":"A5", "input":"=1/0"})).unwrap();
        h.state_mut().run("data.filter", json!({"range":"A1:A5"})).unwrap();
        h.state_mut().run("selection.set", json!({"cell":"A1"})).unwrap();
        press(&mut h, Key::ArrowDown, Modifiers::ALT);
        h.run_steps(4);
        h.get_by_role_and_label(Role::CheckBox, "ЛОЖЬ");
        h.get_by_role_and_label(Role::CheckBox, "FALSE");
        h.get_by_role_and_label(Role::CheckBox, caption);
        h.input_mut().events.push(Event::Text(caption.into()));
        h.run_steps(3);
        h.get_by_role_and_label(Role::CheckBox, caption);
        assert!(h.query_by_label("FALSE").is_none());
        press(&mut h, Key::Enter, Modifiers::NONE);
        h.run_steps(3);
        let sheet = h.state().session.active().unwrap().wb.active().unwrap();
        for row in 1..=4 {
            assert_eq!(sheet.is_row_hidden(row), row != visible_row, "{caption}: row {row}");
        }
    }
}

/// Creates, edits and deletes a sheet-scoped name even when the sheet is called Workbook.
#[test]
fn name_manager_distinguishes_a_sheet_named_workbook() {
    use egui::{Key, Modifiers, accesskit::Role};
    let mut h = harness();
    h.state_mut().run("sheet.rename", json!({"name":"Workbook"})).unwrap();
    h.state_mut().run("home.insertSheet", json!({"name":"Other"})).unwrap();
    h.state_mut().run("formulas.defineName", json!({"name":"GlobalRate", "refersTo":"=0.2", "scope":"Workbook"})).unwrap();
    h.state_mut().open_dialog("nameManager", json!({}));
    h.run_steps(4);
    h.get_by_label(Language::Ru.tr("New…")).click();
    h.run_steps(3);
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("scope".into(), json!({"sheet":"Workbook"}));
    d.values.insert("name".into(), json!("LocalRate"));
    d.values.insert("refersTo".into(), json!("=0.1"));
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, Language::Ru.tr("Scope:")).value().as_deref(), Some("Workbook (Лист)"));
    press(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(3);
    let wb = &h.state().session.active().unwrap().wb;
    let local = wb.sheet_index("Workbook").unwrap();
    let other = wb.sheet_index("Other").unwrap();
    assert_eq!(wb.name("LocalRate", local).unwrap().scope, Some(local));
    assert!(wb.name("LocalRate", other).is_none());
    assert_eq!(wb.name("GlobalRate", other).unwrap().scope, None);

    h.get_by_label("LocalRate").click();
    h.run_steps(2);
    h.get_by_label(Language::Ru.tr("Edit…")).click();
    h.run_steps(3);
    let d = h.state_mut().dialog.as_mut().unwrap();
    assert_eq!(d.values["oldScope"], json!({"sheet":"Workbook"}));
    d.values.insert("name".into(), json!("RenamedRate"));
    h.run_steps(2);
    press(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(3);
    let wb = &h.state().session.active().unwrap().wb;
    assert!(wb.name("LocalRate", local).is_none());
    assert_eq!(wb.name("RenamedRate", local).unwrap().scope, Some(local));
    h.get_by_label("RenamedRate").click();
    h.run_steps(2);
    h.get_all_by_label(Language::Ru.tr("Delete")).last().unwrap().click();
    h.run_steps(3);
    let wb = &h.state().session.active().unwrap().wb;
    assert!(wb.name("RenamedRate", local).is_none());
    assert_eq!(wb.name("GlobalRate", other).unwrap().scope, None);
}

/// The Select Data dialog shares the chart's generated series captions.
#[test]
fn chart_select_data_localizes_generated_series() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range":"A1", "values":[[1,2],[3,4],[5,6]]})).unwrap();
    h.state_mut().run("insert.chart", json!({"range":"A1:B3", "type":"column"})).unwrap();
    let id = h.state().session.active().unwrap().wb.active().unwrap().charts[0].id;
    h.state_mut().open_dialog("selectData", json!({"chart":id}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["names"], json!(["Ряд1", "Ряд2"]));
}

/// Sends a complete key press to the harness, including any shortcut modifiers.
fn press(h: &mut egui_kittest::Harness<'static, SheetApp>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
    }
}
