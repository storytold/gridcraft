//! Finding the previous match and repeating the last Find with the dialog closed, as in Excel
//! (#4873): Shift+Enter in Find and Replace, Shift+F4 and Ctrl+Shift+F4 (Cmd+G and Cmd+Shift+G on
//! a Mac).

use egui::{Event, Key, Modifiers};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(4);
}

/// Pears in A1, C1 and B2: row order A1, C1, B2.
fn pears() -> Harness {
    let mut h = crate::support::blank_harness(4);
    let app = h.state_mut();
    app.session.execute("range.setValues", json!({"range": "A1:C2", "values": [["pear", "", "pear"], ["", "pear", ""]]})).unwrap();
    app.session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    h.run_steps(2);
    h
}

fn active(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

#[test]
fn shift_f4_with_no_earlier_find_opens_the_find_dialog() {
    for m in [Modifiers::SHIFT, CTRL_SHIFT] {
        let mut h = pears();
        key(&mut h, Key::F4, m);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("find"), "{m:?}");
        assert_eq!(active(&h), "A1", "{m:?}: nothing was searched");
    }
}

#[test]
fn shift_f4_repeats_the_last_find_and_ctrl_shift_f4_goes_back() {
    let mut h = pears();
    h.state_mut().session.execute("edit.find", json!({"what": "pear"})).unwrap();
    let first = active(&h);
    key(&mut h, Key::F4, Modifiers::SHIFT);
    let second = active(&h);
    assert_ne!(second, first, "Shift+F4 moves on to the next match");
    key(&mut h, Key::F4, CTRL_SHIFT);
    assert_eq!(active(&h), first, "Ctrl+Shift+F4 goes back to the match before");
    // Back past the first match in the sheet wraps round to the last one.
    key(&mut h, Key::F4, CTRL_SHIFT);
    let start = active(&h);
    assert!(start != first && start != second, "back to the remaining match, {start}");
    key(&mut h, Key::F4, CTRL_SHIFT);
    assert_eq!(active(&h), second, "wraps from the first match to the last");
    let app = h.state();
    assert!(app.dialog.is_none() && app.message.is_none(), "no dialog or message while matches are found");
}

#[test]
fn repeating_a_find_with_no_match_says_so() {
    let mut h = pears();
    let _ = h.state_mut().session.execute("edit.find", json!({"what": "plum"}));
    for m in [Modifiers::SHIFT, CTRL_SHIFT] {
        h.state_mut().message = None;
        key(&mut h, Key::F4, m);
        assert!(h.state().message.is_some(), "{m:?}: told that nothing matched");
        assert_eq!(active(&h), "A1", "{m:?}");
    }
}

#[test]
fn shift_enter_in_find_and_replace_finds_the_previous_match() {
    let mut h = pears();
    key(&mut h, Key::F, CTRL);
    h.input_mut().events.push(Event::Text("pear".into()));
    h.run_steps(4);
    key(&mut h, Key::Enter, Modifiers::NONE);
    let next = active(&h);
    assert_ne!(next, "A1", "Enter finds the next match");
    // The dialog reads the held modifiers, which a ModifiersChanged event sets.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    h.run_steps(4);
    assert_eq!(active(&h), "A1", "Shift+Enter goes back from {next}");
}

#[test]
fn mac_cmd_g_repeats_the_find_and_other_g_shortcuts_stay() {
    let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    let g = |m, mac| lookup(Key::G, Some(Key::G), m, mac);
    assert_eq!(g(mac_cmd, true), Some(Shortcut::RepeatFind { previous: false }));
    assert_eq!(g(Modifiers { shift: true, ..mac_cmd }, true), Some(Shortcut::RepeatFind { previous: true }));
    // Go To moves to Control+G on a Mac, as in Excel for Mac.
    assert_eq!(g(Modifiers::CTRL, true), Some(Shortcut::Dialog("goTo", json!({}))));
    // Ctrl+Shift+G elsewhere is still Workbook Statistics, and plain Ctrl+G still Go To.
    assert_eq!(g(CTRL_SHIFT, false), Some(Shortcut::Dialog("statistics", json!({}))));
    assert_eq!(g(CTRL, false), None, "Ctrl+G goes on to the Ctrl shortcuts");
}
