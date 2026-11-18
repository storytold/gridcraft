//! File › Options › Editing › Automatically insert a decimal point (#3540): while it is on, a whole
//! number typed into a cell or the formula bar gets a decimal point; `cell.set` and the other
//! commands enter what they're given. The status bar shows "Fixed Decimal" meanwhile.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn value(h: &Harness, a1: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

fn formula(h: &Harness, a1: &str) -> Option<String> {
    let sheet = h.state().session.active()?.wb.active()?;
    sheet.cell(CellRef::parse(a1).unwrap()).and_then(|c| c.formula.as_ref()).map(|f| f.text.to_string())
}

/// Types `text` into the active cell and presses Enter, as a person does.
fn type_and_enter(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
}

fn select(h: &mut Harness, a1: &str) {
    h.state_mut().run("selection.set", json!({"range": a1})).unwrap();
    h.run_steps(1);
}

#[test]
fn typing_a_whole_number_gets_the_decimal_point_only_while_the_option_is_on() {
    let mut h = crate::support::blank_harness(4);
    select(&mut h, "A1");
    type_and_enter(&mut h, "2834");
    assert_eq!(value(&h, "A1"), Value::Number(2834.0), "off by default");

    assert_eq!(h.state_mut().run("options.fixedDecimal", json!({"on": true})), Ok(json!({"on": true, "places": 2})));
    assert!(h.state().dialog.is_none() && h.state().backstage.is_none(), "no dialog");
    for (cell, typed, stored) in [
        ("A2", "2834", Value::Number(28.34)),
        ("A3", "-25", Value::Number(-0.25)),
        ("A4", "1,234", Value::Number(12.34)),
        ("A5", "1.5", Value::Number(1.5)),
        ("A6", "=2834", Value::Number(2834.0)),
        ("A7", "abc", Value::from("abc")),
        ("A8", "5%", Value::Number(0.05)),
    ] {
        select(&mut h, cell);
        type_and_enter(&mut h, typed);
        assert_eq!(value(&h, cell), stored, "{typed}");
    }
    assert_eq!(formula(&h, "A6").as_deref(), Some("2834"), "a formula stays a formula");

    // The formula bar commits the same way.
    select(&mut h, "B1");
    h.state_mut().begin_edit(Some("4321".into()), true);
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    assert_eq!(value(&h, "B1"), Value::Number(43.21));
    // Ctrl+Enter fills the selection with the entry it typed.
    select(&mut h, "B2:B3");
    h.input_mut().events.push(Event::Text("7".into()));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(2);
    assert_eq!((value(&h, "B2"), value(&h, "B3")), (Value::Number(0.07), Value::Number(0.07)));

    // `cell.set` (the API, a script, the control channel) enters what it's given.
    h.state_mut().run("cell.set", json!({"cell": "C1", "input": "2834"})).unwrap();
    assert_eq!(value(&h, "C1"), Value::Number(2834.0));

    h.state_mut().run("options.fixedDecimal", json!({"places": -2})).unwrap();
    select(&mut h, "D1");
    type_and_enter(&mut h, "2834");
    assert_eq!(value(&h, "D1"), Value::Number(283400.0));

    h.state_mut().run("options.fixedDecimal", json!({"on": false})).unwrap();
    select(&mut h, "D2");
    type_and_enter(&mut h, "2834");
    assert_eq!(value(&h, "D2"), Value::Number(2834.0), "off again");
    assert_eq!(value(&h, "A2"), Value::Number(28.34), "numbers already entered stay");
}

#[test]
fn ctrl_enter_gives_each_cell_the_decimal_point_its_own_format_allows() {
    // The active cell's format decides only for itself: a cell formatted as Text keeps the digits
    // typed, a percentage reads them as a percent, and the others get the decimal point.
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("options.fixedDecimal", json!({"on": true})).unwrap();
    for (range, format) in [("A2", "Text"), ("A3", "Percentage"), ("B1", "Text")] {
        h.state_mut().run("home.numberFormat", json!({"range": range, "format": format})).unwrap();
    }
    let ctrl_enter = |h: &mut Harness, range: &str, typed: &str| {
        select(h, range);
        h.input_mut().events.push(Event::Text(typed.into()));
        h.run_steps(2);
        h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
        h.run_steps(2);
    };
    ctrl_enter(&mut h, "A1:A3", "2834");
    assert_eq!(
        [value(&h, "A1"), value(&h, "A2"), value(&h, "A3")],
        [Value::Number(28.34), Value::from("2834"), Value::Number(28.34)],
        "from a General cell: A2 is Text, A3 a percentage (2834%)"
    );
    ctrl_enter(&mut h, "B1:B2", "2834");
    assert_eq!((value(&h, "B1"), value(&h, "B2")), (Value::from("2834"), Value::Number(28.34)), "from a Text cell");
    // It is still one fill: one undo takes it back.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!((value(&h, "B1"), value(&h, "B2")), (Value::Empty, Value::Empty));
}

#[test]
fn an_interface_with_a_decimal_comma_keeps_a_typed_decimal_comma() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("options.fixedDecimal", json!({"on": true})).unwrap();
    h.state_mut().run("app.language.set", json!({"language": Language::Es.code()})).unwrap();
    for (cell, typed, stored) in [("A1", "2834", Value::Number(28.34)), ("A2", "1,5", Value::Number(1.5)), ("A3", "-25", Value::Number(-0.25))] {
        select(&mut h, cell);
        type_and_enter(&mut h, typed);
        assert_eq!(value(&h, cell), stored, "Spanish {typed}");
    }
    // German formula entry uses a decimal point for now: "1,5" is text there, with the option or without.
    h.state_mut().run("app.language.set", json!({"language": Language::De.code()})).unwrap();
    for (cell, typed, stored) in [("B1", "2834", Value::Number(28.34)), ("B2", "1,5", Value::from("1,5"))] {
        select(&mut h, cell);
        type_and_enter(&mut h, typed);
        assert_eq!(value(&h, cell), stored, "German {typed}");
    }
}

#[test]
fn options_editing_sets_it_and_the_status_bar_shows_it() {
    let mut h = crate::common::harness_with(
        crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)),
        crate::common::app(crate::support::blank_session()),
        4,
    );
    assert!(h.query_by_label("Fixed Decimal").is_none(), "nothing on the status bar while it is off");
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    let places = |h: &Harness| h.get_by_role_and_label(Role::SpinButton, "Places:").accesskit_node().is_disabled();
    assert!(places(&h), "Places is greyed out while the option is off");

    h.get_by_role_and_label(Role::CheckBox, "Automatically insert a decimal point").click();
    h.run_steps(3);
    assert!(h.state().session.prefs.fixed_decimal);
    assert!(!places(&h));
    assert_eq!(h.get_by_role_and_label(Role::SpinButton, "Places:").accesskit_node().numeric_value(), Some(2.0));

    // Typing in the field sets the places, within ±100.
    for (typed, kept) in [("3", 3), ("-4", -4), ("500", 100)] {
        h.get_by_role_and_label(Role::SpinButton, "Places:").click();
        h.run_steps(2);
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.run_steps(1);
        h.input_mut().events.push(Event::Text(typed.into()));
        h.run_steps(2);
        h.key_press(Key::Enter);
        h.run_steps(2);
        assert_eq!(h.state().session.prefs.fixed_decimal_places, kept, "{typed}");
    }

    h.state_mut().run("ui.backstage", json!({"page": "close"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Fixed Decimal").is_some(), "the status bar shows it while it is on");
    h.state_mut().run("app.language.set", json!({"language": Language::De.code()})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Feste Dezimalstelle").is_some());
    h.state_mut().run("options.fixedDecimal", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Feste Dezimalstelle").is_none());
}
