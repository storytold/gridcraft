//! Ctrl+Shift+U (Control+Shift+U in Excel for Mac) expands and collapses the formula bar.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl+Shift on Windows and Linux sets `ctrl`, `command` and `shift`.
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
/// Control+Shift on a Mac: `ctrl` without `command`.
const MAC_CONTROL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: false };

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "text"})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    crate::support::harness(session, 4)
}

fn ctrl_shift_u(h: &mut Harness, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: Key::U, physical_key: Some(Key::U), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: Key::U, physical_key: Some(Key::U), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn expanded(h: &Harness) -> bool {
    h.state().ui.formula_bar_expanded
}

fn underlined(h: &Harness) -> bool {
    h.state().session.journal.iter().any(|(id, _)| id == "home.underline")
}

#[test]
fn ctrl_shift_u_on_the_grid_expands_and_collapses_the_formula_bar() {
    let mut h = harness();
    assert!(!expanded(&h));
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(expanded(&h), "the first Ctrl+Shift+U expands the formula bar");
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(!expanded(&h), "the second Ctrl+Shift+U collapses it");
    assert!(!underlined(&h), "Ctrl+Shift+U is not Underline");
}

#[test]
fn control_shift_u_expands_the_formula_bar_in_excel_for_mac() {
    let mut h = harness();
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.run_steps(2);
    ctrl_shift_u(&mut h, MAC_CONTROL_SHIFT);
    assert!(expanded(&h));
    assert!(h.state().editor.is_none(), "Control+Shift+U does not start editing as Control+U does");
    let expand = Some(Shortcut::Run("view.expandFormulaBar", json!({})));
    assert_eq!(lookup(Key::U, Some(Key::U), MAC_CONTROL_SHIFT, true), expand);
    assert_eq!(lookup(Key::U, Some(Key::U), CTRL_SHIFT, false), expand);
    // Control+U on a Mac still edits the active cell.
    assert_eq!(lookup(Key::U, Some(Key::U), Modifiers::CTRL, true), Some(Shortcut::Edit));
}

#[test]
fn ctrl_shift_u_while_editing_keeps_the_entry() {
    let mut h = harness();
    h.state_mut().begin_edit(Some("draft".into()), false);
    h.run_steps(2);
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(expanded(&h), "Ctrl+Shift+U works in the cell editor too");
    let ed = h.state().editor.as_ref().expect("the edit stays open");
    assert_eq!(ed.text, "draft");
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(!expanded(&h));
    assert!(h.state().editor.is_some());
}

#[test]
fn expand_formula_bar_command_sets_or_toggles_the_state() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"on": true})), Ok(json!({"on": true})));
    assert!(expanded(&h));
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({})), Ok(json!({"on": false})));
    assert!(!expanded(&h));
}
