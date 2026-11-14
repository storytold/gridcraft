//! Data › Data Validation has Excel's three items: Data Validation…, Circle Invalid Data and
//! Clear Validation Circles (#3104). Circle Invalid Data circles the invalid cells, which the
//! engine keeps and the grid draws, and Clear Validation Circles removes the circles and leaves
//! the rules alone (#3102). The key tips Alt, A, V, I and Alt, A, V, R do the same.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The sheet of #3102 on the Data tab: Qty in A1:A6, whole numbers from 1 to 10 on A2:A6, the
/// column selected.
fn harness(language: Language) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Qty"], [5], [20], ["abc"], [0], [7]]})).unwrap();
    s.execute("data.validation", json!({"range": "A2:A6", "type": "whole", "operator": "between", "formula1": "1", "formula2": "10"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:A6"})).unwrap();
    let mut app = crate::common::app(s);
    app.ui.language = language;
    app.ui.ribbon_tab = "Data".into();
    let mut h = crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1400.0, 800.0)), app, 4);
    h.run_steps(2);
    h
}

fn open_menu(h: &mut Harness, button: &str) {
    h.get_by_role_and_label(Role::Button, button).click();
    h.run_steps(4);
}

fn choose(h: &mut Harness, item: &str) {
    h.get_by_label(item).click();
    h.run_steps(4);
}

fn circles(h: &Harness) -> Vec<String> {
    h.state().session.active().unwrap().validation_circles_on("Sheet1").iter().map(|c| c.a1()).collect()
}

/// The rules are still there: A3 refuses 20.
fn rules_kept(h: &mut Harness) -> bool {
    let d = h.state().session.active().unwrap();
    let one_rule = d.wb.active().unwrap().validations.len() == 1;
    one_rule && h.state_mut().session.execute("data.validate", json!({"cell": "A3", "input": "20"})).unwrap()["ok"] == json!(false)
}

fn last_command(h: &Harness) -> Option<String> {
    h.state().session.journal.last().map(|(id, _)| id.clone())
}

#[test]
fn the_menu_circles_invalid_data_and_clears_the_circles_without_touching_the_rules() {
    let mut h = harness(Language::En);
    open_menu(&mut h, "Data Validation");
    for item in ["Data Validation…", "Circle Invalid Data", "Clear Validation Circles"] {
        assert!(h.query_by_label(item).is_some(), "the menu has {item}");
    }
    assert!(h.query_by_label("Clear Validation").is_none(), "the item that deleted the selected cells' rules is gone");
    choose(&mut h, "Circle Invalid Data");
    assert_eq!(circles(&h), ["A3", "A4", "A5"], "the menu item circles the invalid cells");
    assert!(h.state().message.is_none(), "with no message in the way");
    if let Ok(path) = std::env::var("GRIDCRAFT_VALIDATION_CIRCLES_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    open_menu(&mut h, "Data Validation");
    choose(&mut h, "Clear Validation Circles");
    assert_eq!(last_command(&h).as_deref(), Some("data.clearValidationCircles"));
    assert!(circles(&h).is_empty(), "the circles are gone");
    assert!(rules_kept(&mut h), "the rules stay");
    assert_eq!(h.state().session.active().unwrap().undo.len(), 2, "neither item made an undo step (only Set Values and Data Validation)");
}

#[test]
fn the_german_menu_names_the_items() {
    let mut h = harness(Language::De);
    // The button's two-line label ("Daten-" over "überprüfung") is its accessible name.
    open_menu(&mut h, "Daten- überprüfung");
    for item in ["Datenüberprüfung…", "Ungültige Daten einkreisen", "Gültigkeitskreise löschen"] {
        assert!(h.query_by_label(item).is_some(), "the German menu has {item}");
    }
    choose(&mut h, "Gültigkeitskreise löschen");
    assert_eq!(last_command(&h).as_deref(), Some("data.clearValidationCircles"));
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn key_tips(h: &mut Harness, letters: &[Key]) {
    key(h, Key::F10);
    for &k in letters {
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Text(k.name().to_ascii_lowercase()));
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
}

#[test]
fn key_tips_avi_and_avr_circle_and_clear() {
    let mut h = harness(Language::En);
    key_tips(&mut h, &[Key::A, Key::V, Key::I]);
    assert_eq!(circles(&h), ["A3", "A4", "A5"], "Alt, A, V, I circles the invalid cells");
    key_tips(&mut h, &[Key::A, Key::V, Key::R]);
    assert!(circles(&h).is_empty(), "Alt, A, V, R clears them");
    assert!(rules_kept(&mut h));
}
