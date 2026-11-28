//! Find All says "nothing found" instead of quietly keeping an empty list: the search's absence
//! must be reported, not swallowed by the UI's `.ok()`. An empty query is not a search and says
//! the same thing; errors that are not "nothing found" keep the message box, titled in the
//! interface language.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// Opens Find and Replace with Ctrl+F and types `what` into the field through real key events.
fn open_find_and_type(what: &str) -> Harness {
    let mut h = crate::support::blank_harness(6);
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    h.input_mut().events.push(Event::Text(what.into()));
    h.run_steps(6);
    h
}

/// Rewrite the "Find what" field the way people do: click it, select all, delete, type. A search
/// changed through the field, not through hand-inserted dialog data, is what must clear old hits.
fn retype_field(h: &mut Harness, text: &'static str) {
    h.query_all_by_label("Find what:").next().unwrap().click();
    h.run();
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: false, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key {
        key: Key::Delete,
        physical_key: Some(Key::Delete),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    h.input_mut().events.push(Event::Key {
        key: Key::Delete,
        physical_key: Some(Key::Delete),
        pressed: false,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    h.input_mut().events.push(Event::Text(text.into()));
    h.run();
}

/// A Find All over a query typed into the field reports a miss as the dialog's own quoted
/// no-result line; the earlier count line goes and no message box opens.
#[test]
fn find_all_says_there_is_nothing_to_report() {
    let mut h = open_find_and_type("hello");
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("1 cell(s) found").next().is_some(), "the first search reports its hit");
    // The failed search is typed into the field, the way people change a query.
    retype_field(&mut h, "absent");
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

/// A no-result line does not stick: after a failed search, a later Find All that hits restores
/// the count line and drops the quoted query.
#[test]
fn find_all_after_a_no_match_restores_the_hits() {
    let mut h = open_find_and_type("absent");
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("Nothing found for “absent”").next().is_some(), "the failed search says it");
    retype_field(&mut h, "hello");
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("1 cell(s) found").next().is_some(), "the count line comes back");
    assert!(h.query_all_by_value("Nothing found").next().is_none(), "the no-result line is gone");
    assert!(h.query_all_by_value("Nothing found for “hello”").next().is_none(), "nothing is quoted after a hit");
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.values.get("notFound").and_then(serde_json::Value::as_bool), Some(false));
    assert_eq!(d.values.get("foundFor"), None, "the quoted query is dropped after a hit");
}

/// An empty Find All query is not a search: the plain no-result line says it in the dialog,
/// and the engine's bad-params message box never opens.
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

/// An empty Find Next query takes the same path: the dialog says the plain line, no message box.
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

/// An empty Find Next drops the hit list Find All left behind, and a following hit cannot bring
/// that old list back.
#[test]
fn an_empty_find_next_drops_the_retained_hit_list() {
    let mut h = open_find_and_type("hello");
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("1 cell(s) found").next().is_some(), "the hit list is up");
    retype_field(&mut h, "");
    h.get_by_label("Find Next").click();
    h.run();
    assert!(h.query_all_by_value("Nothing found").next().is_some(), "the empty query says it");
    assert!(h.query_all_by_value("1 cell(s) found").next().is_none(), "the count line is gone");
    assert!(h.state().dialog.as_ref().unwrap().result.is_none(), "the old list is dropped, not merely hidden");
    retype_field(&mut h, "hello");
    h.get_by_label("Find Next").click();
    h.run();
    assert!(h.state().message.is_none(), "a hit opens no message box");
    assert!(h.query_all_by_value("1 cell(s) found").next().is_none(), "a hit cannot resurrect the dropped list");
    assert!(h.query_all_by_value("Nothing found").next().is_none(), "the no-result line is gone after a hit");
}

/// A failed Find Next is a real error, not a no-result: it keeps the message box, titled
/// "Find and Replace", and leaves no red line inside the dialog.
#[test]
fn a_failed_find_next_still_uses_the_message_box() {
    let mut h = open_find_and_type("absent");
    h.get_by_label("Find Next").click();
    h.run();
    let state = h.state();
    assert!(state.message.is_some(), "a failed Find Next says it in a message box");
    assert_eq!(state.message.as_ref().unwrap().0, "Find and Replace");
    assert_eq!(state.message.as_ref().unwrap().1, "Nothing found for “absent”", "the message quotes the query in the interface language");
    assert_ne!(
        state.dialog.as_ref().unwrap().values.get("notFound").and_then(serde_json::Value::as_bool),
        Some(true),
        "a failed Find Next leaves no red line in the dialog itself"
    );
}

/// A hit through Find Next after a Find All no-match clears the no-result line and opens no
/// message box.
#[test]
fn find_next_after_a_no_match_clears_the_line_when_it_hits() {
    let mut h = open_find_and_type("absent");
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    h.get_by_label("Find All").click();
    h.run();
    assert!(h.query_all_by_value("Nothing found for “absent”").next().is_some(), "the no-match line is up");
    retype_field(&mut h, "hello");
    h.get_by_label("Find Next").click();
    h.run();
    let state = h.state();
    assert!(state.message.is_none(), "a hit opens no message box");
    assert!(h.query_all_by_value("Nothing found").next().is_none(), "the no-result line is gone after a hit");
    assert!(h.query_all_by_value("Nothing found for “hello”").next().is_none());
    let d = state.dialog.as_ref().unwrap();
    assert_eq!(d.values.get("notFound").and_then(serde_json::Value::as_bool), Some(false));
    assert_eq!(d.values.get("foundFor"), None, "the quoted query is dropped after a hit");
}

/// In Russian, the no-result lines render in the interface language: the quoted line for a
/// miss and the plain line for an empty query.
#[test]
fn find_all_no_match_renders_in_the_interface_language() {
    let mut h = crate::support::blank_harness(6);
    h.state_mut().run("app.language.set", json!({"language": "ru"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    h.run_steps(3);
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!("absent"));
    h.get_by_label("Найти все").click();
    h.run();
    assert!(h.query_all_by_value("Ничего не найдено для “absent”").next().is_some(), "the no-match line renders in Russian");
    // The empty query takes the same path, with nothing to quote.
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!(""));
    h.get_by_label("Найти все").click();
    h.run();
    assert!(h.state().message.is_none(), "the Russian dialog reports an empty query without a message box");
    assert!(h.query_all_by_value("Ничего не найдено").next().is_some(), "the plain line renders in Russian");
}

/// In Russian, the message box for a failed Find Next carries its title and its quoted body in
/// the interface language.
#[test]
fn find_errors_open_a_message_box_titled_in_the_interface_language() {
    let mut h = crate::support::blank_harness(6);
    h.state_mut().run("app.language.set", json!({"language": "ru"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    h.run_steps(3);
    h.state_mut().dialog.as_mut().unwrap().values.insert("what".into(), json!("absent"));
    h.get_by_label("Найти следующее").click();
    h.run();
    let state = h.state();
    assert!(state.message.is_some(), "a failed search opens the error window");
    assert_eq!(state.message.as_ref().unwrap().0, "Найти и заменить", "the error window title follows the interface language");
    assert_eq!(state.message.as_ref().unwrap().1, "Ничего не найдено для “absent”", "the error window text follows the interface language");
}

/// The whole Find form - title, tabs, fields, buttons, checkboxes on both tabs - draws in
/// Russian, not merely carries Russian entries in the catalog.
#[test]
fn the_find_form_shows_in_the_interface_language() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("app.language.set", json!({"language": "ru"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    h.run_steps(3);
    let ru = gridcraft_ui_egui::i18n::Language::Ru;
    for english in [
        "Find what:",
        "Find All",
        "Match case",
        "Find tab|Find",
        "Replace tab|Replace",
        "Nothing found",
        "Nothing found for “{}”",
        "{count} cell(s) found",
    ] {
        assert_ne!(ru.tr(english), english, "{english} has no Russian entry");
    }
    assert!(h.query_by_label_contains("Найти и заменить").is_some(), "the window title translates");
    assert!(h.query_by_role_and_label(egui::Role::Button, "Поиск").is_some(), "the tab shows in Russian");
    for label in ["Что искать:", "Найти все", "Найти следующее", "Учитывать регистр", "В книге"]
    {
        assert!(
            h.query_all_by_value(label).next().is_some() || h.query_all_by_label_contains(label).next().is_some(),
            "no {label} in the drawn dialog"
        );
    }
    // The Replace tab has its own labels, buttons and the long whole-cell checkbox: they must all
    // draw in Russian too, not just exist in the catalog.
    h.get_by_role_and_label(egui::Role::Button, "Замена").click();
    h.run_steps(3);
    for label in ["Заменить на:", "Заменить все", "Заменить", "Совпадение со всем содержимым ячейки"]
    {
        assert!(
            h.query_all_by_value(label).next().is_some() || h.query_all_by_label_contains(label).next().is_some(),
            "no {label} on the drawn Replace tab"
        );
    }
}
