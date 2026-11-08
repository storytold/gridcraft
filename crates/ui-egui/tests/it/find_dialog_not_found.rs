//! Find All says "nothing found" instead of quietly keeping an empty list: the search's absence
//! must be reported, not swallowed by the UI's `.ok()`.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

fn open_find_and_type(what: &str) -> Harness {
    let mut h = crate::support::blank_harness(6);
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    h.input_mut().events.push(Event::Text(what.into()));
    h.run_steps(6);
    h
}

#[test]
fn find_all_says_there_is_nothing_to_report() {
    let mut h = open_find_and_type("hello");
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("1 cell(s) found").next().is_some(), "the first search reports its hit");
    // The failed search must take the results area over, not keep the previous hits.
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!("absent"));
    h.get_by_label("Find All").click();
    h.run();
    let state = h.state();
    assert!(state.dialog.is_some(), "the dialog stays open");
    assert!(state.message.is_none(), "Find All reports in the dialog, not in a message box");
    let d = state.dialog.as_ref().unwrap();
    assert!(d.result.is_none(), "the results list is cleared, not left stale");
    assert_eq!(d.values.get("notFound").and_then(serde_json::Value::as_bool), Some(true));
    assert!(h.query_all_by_value("Nothing found for “absent”").next().is_some(), "the dialog's own line says it");
    assert!(h.query_all_by_value("1 cell(s) found").next().is_none(), "the earlier count line is gone");
}

#[test]
fn an_empty_find_all_query_says_nothing_was_found() {
    let mut h = open_find_and_type("");
    h.get_by_label("Find All").click();
    h.run();
    let state = h.state();
    assert!(state.message.is_none(), "an empty query reports in the dialog, not in a message box");
    let d = state.dialog.as_ref().unwrap();
    assert!(d.result.is_none(), "the results list is cleared, not left stale");
    assert_eq!(d.values.get("notFound").and_then(serde_json::Value::as_bool), Some(true));
    assert!(h.query_all_by_value("Nothing found").next().is_some(), "the plain line says it");
    assert!(h.query_all_by_value("Nothing found for “”").next().is_none(), "an empty query is not quoted");
}

#[test]
fn an_empty_find_next_says_nothing_was_found() {
    let mut h = open_find_and_type("");
    h.get_by_label("Find Next").click();
    h.run();
    let state = h.state();
    assert!(state.message.is_none(), "an empty query reports in the dialog, not in a message box");
    assert_eq!(state.dialog.as_ref().unwrap().values.get("notFound").and_then(serde_json::Value::as_bool), Some(true));
    assert!(h.query_all_by_value("Nothing found").next().is_some(), "the plain line says it");
}

#[test]
fn find_all_no_match_renders_in_the_interface_language() {
    let mut h = crate::support::blank_harness(6);
    h.state_mut().run("app.language.set", json!({"language": "ru"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    h.run_steps(3);
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!("absent"));
    h.get_by_label("Найти всё").click();
    h.run();
    assert!(h.query_all_by_value("Ничего не найдено для “absent”").next().is_some(), "the no-match line renders in Russian");
    // The empty query takes the same path, with nothing to quote.
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!(""));
    h.get_by_label("Найти всё").click();
    h.run();
    assert!(h.state().message.is_none(), "the Russian dialog reports an empty query without a message box");
    assert!(h.query_all_by_value("Ничего не найдено").next().is_some(), "the plain line renders in Russian");
}

#[test]
fn the_find_form_shows_in_the_interface_language() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("app.language.set", json!({"language": "ru"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    h.run_steps(3);
    let ru = gridcraft_ui_egui::i18n::Language::Ru;
    for english in ["Find what:", "Find All", "Match case", "Find tab|Find", "Replace tab|Replace", "Nothing found for “{}”", "{count} cell(s) found"]
    {
        assert_ne!(ru.tr(english), english, "{english} has no Russian entry");
    }
    assert!(h.query_by_label_contains("Поиск и замена").is_some(), "the window title translates");
    assert!(h.query_by_role_and_label(egui::Role::Button, "Поиск").is_some(), "the tab shows in Russian");
    for label in ["Искать:", "Найти всё", "Найти далее", "Учитывать регистр", "В пределах книги", "Замена"]
    {
        assert!(
            h.query_all_by_value(label).next().is_some() || h.query_all_by_label_contains(label).next().is_some(),
            "no {label} in the drawn dialog"
        );
    }
}
