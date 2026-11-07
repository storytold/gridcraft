//! Formulas › Python: the Insert Python button and its key tip start Python mode on the active cell.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use serde_json::json;

use crate::common::Harness;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"cell": "C2"})).unwrap();
    let mut h = crate::common::os_harness(s, egui::os::OperatingSystem::Nix);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(3);
    h
}

fn python_edit(h: &Harness) -> Option<(CellRef, String)> {
    h.state().editor.as_ref().filter(|e| e.python).map(|e| (e.cell, e.text.clone()))
}

#[test]
fn insert_python_starts_python_mode_on_the_active_cell() {
    let mut h = harness();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Insert Python").click();
    h.run_steps(3);
    assert_eq!(python_edit(&h), Some((CellRef::new(1, 2), String::new())));
    h.check_accessibility();
    // Clicking it again keeps Python mode on, with what was typed.
    h.input_mut().events.push(Event::Text("1".into()));
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Insert Python").click();
    h.run_steps(3);
    assert_eq!(python_edit(&h), Some((CellRef::new(1, 2), "1".into())));
    // An ordinary entry in progress becomes code.
    h.state_mut().cancel_edit();
    h.state_mut().begin_edit(Some("x + 1".into()), false);
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Insert Python").click();
    h.run_steps(3);
    assert_eq!(python_edit(&h), Some((CellRef::new(1, 2), "x + 1".into())));
}

fn press(h: &mut Harness, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn its_key_tip_is_m_y() {
    let mut h = harness();
    press(&mut h, Key::F10);
    for (key, c) in [(Key::M, "m"), (Key::Y, "y")] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Text(c.into()));
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
    assert_eq!(python_edit(&h), Some((CellRef::new(1, 2), String::new())), "the key tip's letter is not typed into the code");
    assert_eq!(h.state().keytips.prefix(), None);
    // The table names the control.
    let seq = gridcraft_ui_egui::keytips::keytip_table::SEQUENCES.iter().find(|(s, _)| *s == "MY").map(|(_, e)| format!("{e:?}"));
    assert_eq!(seq.as_deref(), Some("Finish(Control(\"python.insert\"))"));
}

#[test]
fn a_refused_insert_python_says_why() {
    // A cell inside another cell's spill takes no Python mode: the button says so rather than
    // doing nothing.
    let mut h = harness();
    h.state_mut().session.execute("cell.set", json!({"cell": "C1", "input": "=SEQUENCE(3)"})).unwrap();
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Insert Python").click();
    h.run_steps(3);
    assert_eq!(python_edit(&h), None, "no Python mode inside a spill");
    let message = h.state().message.as_ref().map(|m| m.1.clone()).unwrap_or_default();
    assert!(message.contains("spill"), "the reason shows: {message:?}");
}
