//! A Python cell formatted Hidden on a protected sheet keeps its code out of view, as a hidden
//! formula does: the formula bar shows nothing, and F2, a double-click, the formula bar,
//! Ctrl+Alt+Shift+P and the PY badge start no edit that would show the code, or
//! that would write blank code over it.

use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use serde_json::json;

use crate::common::Harness;

const CODE: &str = "secret_total = 41\nsecret_total";

/// A Python cell at B2, selected, with its protection `hidden` flag, on a sheet protected or not.
fn harness(hidden: bool, protected: bool) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "B2", "code": CODE})).unwrap();
    s.execute("home.formatCells", json!({"range": "B2", "style": {"protection": {"locked": false, "hidden": hidden}}})).unwrap();
    if protected {
        s.execute("review.protectSheet", json!({})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(s), 4)
}

fn send(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn press(h: &mut Harness, key: Key, modifiers: Modifiers) {
    send(
        h,
        [
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
        ],
    );
}

const CTRL_ALT_SHIFT: Modifiers = Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true };

/// Two clicks on B2 in a row: a double-click on the grid.
fn double_click_b2(h: &mut Harness) {
    let at = {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        app.grid.geo.as_ref().unwrap().screen_cell_rect(sh, CellRef::parse("B2").unwrap()).center()
    };
    for _ in 0..2 {
        for event in [
            Event::PointerMoved(at),
            Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ] {
            h.input_mut().events.push(event);
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

/// The open edit's text, and whether it is in Python mode.
fn editor(h: &Harness) -> Option<(String, bool)> {
    h.state().editor.as_ref().map(|e| (e.text.clone(), e.python))
}

fn bar_shows_code(h: &Harness) -> bool {
    h.query_by_label_contains("secret_total").is_some()
}

fn stored_code(h: &Harness) -> Option<String> {
    let d = h.state().session.active()?;
    d.wb.active()?.cell(CellRef::parse("B2")?)?.py.as_ref().map(|p| p.code.clone())
}

fn cancel(h: &mut Harness) {
    h.state_mut().cancel_edit();
    h.run_steps(2);
}

#[test]
fn a_hidden_python_cell_on_a_protected_sheet_shows_no_code_in_the_formula_bar() {
    let h = harness(true, true);
    assert!(!bar_shows_code(&h), "the formula bar shows nothing for hidden code");
    assert!(h.query_by_label("Python Mode").is_some(), "the PY badge still marks a Python cell");
}

#[test]
fn f2_a_double_click_and_the_formula_bar_start_no_edit_on_hidden_code() {
    let mut h = harness(true, true);
    press(&mut h, Key::F2, Modifiers::NONE);
    assert_eq!(editor(&h), None, "F2");
    double_click_b2(&mut h);
    assert_eq!(editor(&h), None, "a double-click");
    h.state_mut().begin_edit(None, true);
    assert_eq!(editor(&h), None, "a click in the formula bar");
    assert!(!bar_shows_code(&h));
    assert_eq!(stored_code(&h).as_deref(), Some(CODE), "the code is untouched");
}

#[test]
fn python_mode_starts_no_edit_on_hidden_code() {
    let mut h = harness(true, true);
    press(&mut h, Key::P, CTRL_ALT_SHIFT);
    assert_eq!(editor(&h), None, "Ctrl+Alt+Shift+P");
    h.get_by_label("Python Mode").click();
    h.run_steps(3);
    assert_eq!(editor(&h), None, "the PY badge");
    assert_eq!(h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap()["on"], false, "ui.pythonMode");
    assert!(!bar_shows_code(&h));
    // Ctrl+Enter with no edit open writes nothing over the code.
    press(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(stored_code(&h).as_deref(), Some(CODE));
}

/// The same cell, unprotected or not hidden, shows and edits its code.
#[test]
fn the_code_shows_and_edits_when_the_sheet_is_unprotected_or_the_cell_not_hidden() {
    for (hidden, protected) in [(true, false), (false, true)] {
        let what = format!("hidden {hidden}, protected {protected}");
        let mut h = harness(hidden, protected);
        assert!(bar_shows_code(&h), "{what}: the formula bar shows the code");
        press(&mut h, Key::F2, Modifiers::NONE);
        assert_eq!(editor(&h), Some((CODE.to_string(), true)), "{what}: F2");
        cancel(&mut h);
        double_click_b2(&mut h);
        assert_eq!(editor(&h), Some((CODE.to_string(), true)), "{what}: a double-click");
        cancel(&mut h);
        press(&mut h, Key::P, CTRL_ALT_SHIFT);
        assert_eq!(editor(&h), Some((CODE.to_string(), true)), "{what}: Ctrl+Alt+Shift+P");
        cancel(&mut h);
    }
}
