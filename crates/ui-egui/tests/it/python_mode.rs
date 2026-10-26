//! Python mode in the formula bar: entering and leaving it, its keys (applied in input order, also
//! several in one frame), committing through `python.set`, and the PY badge.

use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellError, CellRef, Value};
use serde_json::json;

use crate::common::Harness;

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn harness(session: Session) -> Harness {
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(session), 4)
}

fn mac_harness(session: Session) -> Harness {
    crate::common::os_harness(session, egui::os::OperatingSystem::Mac)
}

fn press(key: Key, modifiers: Modifiers) -> [Event; 2] {
    [
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

/// Sends `events` in one frame, then lets the app settle.
fn send(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn type_text(h: &mut Harness, text: &str) {
    send(h, [Event::Text(text.into())]);
}

const CTRL_ALT_SHIFT: Modifiers = Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true };
const CMD_OPTION_SHIFT: Modifiers = Modifiers { alt: true, ctrl: false, shift: true, mac_cmd: true, command: true };

fn shortcut(h: &mut Harness) {
    send(h, press(Key::P, CTRL_ALT_SHIFT));
}

fn ctrl_enter(h: &mut Harness) {
    send(h, press(Key::Enter, Modifiers::COMMAND));
}

/// Whether an edit is open, and in Python mode.
fn mode(h: &Harness) -> Option<bool> {
    h.state().editor.as_ref().map(|e| e.python)
}

fn text(h: &Harness) -> String {
    h.state().editor.as_ref().map(|e| e.text.clone()).unwrap_or_default()
}

fn code(h: &Harness, cell: &str) -> Option<String> {
    let d = h.state().session.active()?;
    d.wb.active()?.cell(CellRef::parse(cell)?)?.py.as_ref().map(|p| p.code.clone())
}

fn value(h: &Harness, cell: &str) -> Value {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

#[test]
fn the_shortcut_enters_and_leaves_python_mode_and_esc_cancels() {
    let mut h = harness(blank());
    shortcut(&mut h);
    assert_eq!(mode(&h), Some(true), "Ctrl+Alt+Shift+P starts Python mode on the active cell");
    let ed = h.state().editor.as_ref().unwrap();
    assert!(ed.from_formula_bar, "Python mode edits in the formula bar");
    assert_eq!(ed.cell, CellRef::new(0, 0));
    h.check_accessibility();
    type_text(&mut h, "x = 1");
    assert_eq!(text(&h), "x = 1");
    shortcut(&mut h);
    assert_eq!(mode(&h), Some(false), "the shortcut again leaves Python mode");
    assert_eq!(text(&h), "x = 1", "and the edit keeps its text");
    shortcut(&mut h);
    assert_eq!(mode(&h), Some(true));
    send(&mut h, press(Key::Escape, Modifiers::NONE));
    assert_eq!(mode(&h), None, "Esc cancels the edit");
    assert_eq!(code(&h, "A1"), None, "and changes nothing");
    assert_eq!(value(&h, "A1"), Value::Empty);
}

#[test]
fn the_mac_shortcut_is_cmd_option_shift_p_and_cmd_enter_commits() {
    let mut h = mac_harness(blank());
    send(&mut h, press(Key::P, CMD_OPTION_SHIFT));
    assert_eq!(mode(&h), Some(true));
    type_text(&mut h, "1 + 1");
    send(&mut h, press(Key::Enter, Modifiers::MAC_CMD | Modifiers::COMMAND));
    assert_eq!(mode(&h), None);
    assert_eq!(code(&h, "A1").as_deref(), Some("1 + 1"));
}

#[test]
fn typing_py_then_tab_enters_python_mode_as_in_excel() {
    let mut h = harness(blank());
    type_text(&mut h, "=PY");
    assert_eq!(mode(&h), Some(false), "typing starts an ordinary entry in the cell");
    send(&mut h, press(Key::Tab, Modifiers::NONE));
    assert_eq!(mode(&h), Some(true), "=PY then Tab is Python mode");
    assert_eq!(text(&h), "", "the trigger is not code");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.from_formula_bar));
    let active = h.state().session.active().map(|d| d.selection.active);
    assert_eq!(active, Some(CellRef::new(0, 0)), "Tab did not move on");
    type_text(&mut h, "len('abc')");
    ctrl_enter(&mut h);
    assert_eq!(code(&h, "A1").as_deref(), Some("len('abc')"));
    assert_eq!(value(&h, "A1"), Value::Error(CellError::Blocked), "this build stores the code and doesn't run it");
    // Any other entry still commits with Tab.
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.run_steps(2);
    type_text(&mut h, "=PI");
    send(&mut h, press(Key::Tab, Modifiers::NONE));
    assert_eq!(mode(&h), None);
}

#[test]
fn enter_and_tab_edit_the_code_and_ctrl_enter_commits_through_python_set() {
    let mut h = harness(blank());
    h.state_mut().session.execute("selection.set", json!({"cell": "C3"})).unwrap();
    shortcut(&mut h);
    type_text(&mut h, "for i in range(3):");
    send(&mut h, press(Key::Enter, Modifiers::NONE));
    type_text(&mut h, "print(i)");
    send(&mut h, press(Key::Enter, Modifiers::NONE));
    send(&mut h, press(Key::Tab, Modifiers::SHIFT));
    type_text(&mut h, "x");
    assert_eq!(text(&h), "for i in range(3):\n    print(i)\nx", "Enter keeps the indentation, Shift+Tab takes it back");
    send(&mut h, press(Key::Tab, Modifiers::NONE));
    assert_eq!(text(&h), "for i in range(3):\n    print(i)\nx    ");
    assert_eq!(mode(&h), Some(true), "Enter and Tab stay in the edit");
    ctrl_enter(&mut h);
    assert_eq!(mode(&h), None);
    assert_eq!(code(&h, "C3").as_deref(), Some("for i in range(3):\n    print(i)\nx    "));
    assert_eq!(h.state().session.active().map(|d| d.selection.active), Some(CellRef::new(2, 2)), "Ctrl+Enter stays on the cell");
    // One undo takes the code away.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(code(&h, "C3"), None);
}

#[test]
fn several_keys_in_one_frame_are_applied_in_order() {
    let mut h = harness(blank());
    shortcut(&mut h);
    type_text(&mut h, "a");
    send(&mut h, press(Key::Enter, Modifiers::NONE));
    type_text(&mut h, "b");
    // Select all, then Tab, in one frame: Tab indents the selected lines rather than replacing
    // them (the #456 review regression).
    let mut events = Vec::new();
    events.extend(press(Key::A, Modifiers::COMMAND));
    events.extend(press(Key::Tab, Modifiers::NONE));
    send(&mut h, events);
    assert_eq!(text(&h), "    a\n    b");
    let mut events = Vec::new();
    events.extend(press(Key::A, Modifiers::COMMAND));
    events.extend(press(Key::Tab, Modifiers::SHIFT));
    send(&mut h, events);
    assert_eq!(text(&h), "a\nb", "Shift+Tab after select-all dedents every line");
    // A keystroke sequence as the platform sends it: a key before each text, then Enter.
    send(&mut h, press(Key::End, Modifiers::COMMAND));
    let mut events = Vec::new();
    for (k, c) in [(Key::I, "i"), (Key::F, "f"), (Key::Space, " "), (Key::Y, "y")] {
        events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        events.push(Event::Text(c.into()));
    }
    events.push(Event::Key { key: Key::Colon, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT });
    events.push(Event::Text(":".into()));
    events.extend(press(Key::Enter, Modifiers::NONE));
    events.push(Event::Text("z".into()));
    send(&mut h, events);
    assert_eq!(text(&h), "a\nbif y:\n    z");
    // Select all, Tab and Ctrl+Enter in one frame: the commit waits for the Tab.
    let mut events = Vec::new();
    events.extend(press(Key::A, Modifiers::COMMAND));
    events.extend(press(Key::Tab, Modifiers::NONE));
    events.extend(press(Key::Enter, Modifiers::COMMAND));
    send(&mut h, events);
    assert_eq!(mode(&h), None);
    assert_eq!(code(&h, "A1").as_deref(), Some("    a\n    bif y:\n        z"));
}

#[test]
fn keys_typed_after_the_commit_key_in_the_same_frame_are_not_committed() {
    let mut h = harness(blank());
    shortcut(&mut h);
    let mut events = vec![Event::Text("1".into())];
    events.extend(press(Key::Enter, Modifiers::COMMAND));
    events.push(Event::Text("2".into()));
    send(&mut h, events);
    assert_eq!(code(&h, "A1").as_deref(), Some("1"));
    assert_eq!(value(&h, "B1"), Value::Empty);
    assert!(h.state().editor.is_none(), "the text after the commit key starts nothing");
}

#[test]
fn code_too_long_for_a_cell_shows_the_commands_message_and_keeps_the_code() {
    let mut h = harness(blank());
    shortcut(&mut h);
    let long = "y".repeat(8_080);
    if let Some(ed) = h.state_mut().editor.as_mut() {
        ed.text = long.clone();
        ed.caret = long.chars().count();
        ed.sync_caret = true;
    }
    h.run_steps(2);
    ctrl_enter(&mut h);
    let message = h.state().message.clone().map(|m| m.1).unwrap_or_default();
    assert!(message.contains("8192"), "the command's message: {message}");
    assert_eq!(mode(&h), Some(true), "the edit stays open in Python mode");
    assert_eq!(text(&h), long, "with the code as typed");
    assert_eq!(code(&h, "A1"), None);
    // Keys go to the message box, not the code, until it closes.
    send(&mut h, press(Key::Enter, Modifiers::NONE));
    assert!(h.state().message.is_none(), "Enter closed the message box");
    assert_eq!(text(&h), long, "and added no line to the code");
    // The editor has the keyboard back: shortening the code commits it.
    send(&mut h, press(Key::Backspace, Modifiers::NONE));
    assert_eq!(text(&h).len(), 8_079);
    ctrl_enter(&mut h);
    assert_eq!(code(&h, "A1").map(|c| c.len()), Some(8_079));
}

#[test]
fn editing_a_python_cell_edits_its_code_and_typing_replaces_it() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "B2", "code": "x = 1\nx"})).unwrap();
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = harness(s);
    // The formula bar shows the code, with the PY badge.
    assert!(h.query_by_label("Python Mode").is_some(), "the PY badge shows on a selected Python cell");
    send(&mut h, press(Key::F2, Modifiers::NONE));
    assert_eq!(mode(&h), Some(true), "F2 on a Python cell is Python mode");
    assert_eq!(text(&h), "x = 1\nx");
    // The badge leaves Python mode, as the shortcut does.
    h.get_by_label("Python Mode").click();
    h.run_steps(3);
    assert_eq!(mode(&h), Some(false));
    send(&mut h, press(Key::Escape, Modifiers::NONE));
    assert_eq!(mode(&h), None);
    assert!(h.query_by_label("Python Mode").is_some());
    // Typing replaces the cell, as in Excel.
    type_text(&mut h, "7");
    assert_eq!(mode(&h), Some(false));
    send(&mut h, press(Key::Enter, Modifiers::NONE));
    assert_eq!(code(&h, "B2"), None);
    assert_eq!(value(&h, "B2"), Value::Number(7.0));
    // No badge on an ordinary cell.
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Python Mode").is_none());
}

#[test]
fn ui_python_mode_switches_from_the_control_channel_without_a_dialog() {
    let mut h = harness(blank());
    let r = h.state_mut().run("ui.pythonMode", json!({})).unwrap();
    assert_eq!(r["on"], true);
    assert_eq!(h.state_mut().run("ui.pythonMode", json!({"on": true})).unwrap()["on"], true, "already on");
    assert_eq!(h.state_mut().run("ui.pythonMode", json!({"on": false})).unwrap()["on"], false);
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    h.state_mut().cancel_edit();
    assert_eq!(h.state_mut().run("ui.pythonMode", json!({"on": false})).unwrap()["on"], false, "off with no edit does nothing");
    assert!(h.state().editor.is_none());
}

/// Clicks the middle of `a1` on screen.
fn click_cell(h: &mut Harness, a1: &str) {
    let at = {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        app.grid.geo.as_ref().unwrap().screen_cell_rect(sh, CellRef::parse(a1).unwrap()).center()
    };
    for event in [
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
}

#[test]
fn the_enter_button_and_a_click_on_the_sheet_commit_the_code_through_python_set() {
    let mut h = harness(blank());
    shortcut(&mut h);
    type_text(&mut h, "'=SUM(1)'");
    h.get_by_label("Enter").click();
    h.run_steps(3);
    assert_eq!(code(&h, "A1").as_deref(), Some("'=SUM(1)'"), "the code, not a formula or text");
    h.state_mut().session.execute("selection.set", json!({"cell": "A2"})).unwrap();
    shortcut(&mut h);
    type_text(&mut h, "2");
    click_cell(&mut h, "D4");
    assert_eq!(code(&h, "A2").as_deref(), Some("2"));
    assert_eq!(h.state().session.active().map(|d| d.selection.active), Some(CellRef::new(3, 3)));
    assert!(h.state().editor.is_none());
}

#[test]
fn the_formula_bar_shows_while_python_mode_is_open_when_the_preference_hides_it() {
    let mut h = harness(blank());
    h.state_mut().session.prefs.formula_bar = false;
    h.run_steps(2);
    shortcut(&mut h);
    assert_eq!(mode(&h), Some(true));
    type_text(&mut h, "x = 1");
    assert_eq!(text(&h), "x = 1", "the code editor is drawn and takes the typing");
    ctrl_enter(&mut h);
    assert!(h.state().editor.is_none());
    assert_eq!(code(&h, "A1").as_deref(), Some("x = 1"));
    assert!(!h.state().session.prefs.formula_bar, "the preference stays as it was");
}

#[test]
fn committing_no_code_clears_a_python_cell_and_writes_nothing_elsewhere() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "1 + 1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "5"})).unwrap();
    let mut h = harness(s);
    // A Python cell whose code is deleted is cleared, as an empty entry clears any other cell.
    shortcut(&mut h);
    h.state_mut().editor.as_mut().unwrap().text.clear();
    ctrl_enter(&mut h);
    assert!(h.state().editor.is_none(), "{:?}", h.state().message);
    assert_eq!(code(&h, "A1"), None);
    assert_eq!(value(&h, "A1"), Value::Empty);
    // Python mode on a value cell, committed with no code: the value stays.
    h.state_mut().run("selection.set", json!({"range": "A2"})).unwrap();
    h.run_steps(2);
    shortcut(&mut h);
    assert_eq!(text(&h), "");
    ctrl_enter(&mut h);
    assert!(h.state().editor.is_none());
    assert_eq!(code(&h, "A2"), None);
    assert_eq!(value(&h, "A2"), Value::Number(5.0));
}

#[test]
fn a_python_draft_belongs_to_the_window_it_was_typed_in() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "B1", "code": "2"})).unwrap();
    let mut h = harness(s);
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run_or_alert("view.newWindow", json!({}));
    h.run_steps(2);
    let second = h.state().session.active_window().unwrap();
    assert_ne!(first, second);
    // Both ways into Python mode record the window, as an ordinary edit does.
    shortcut(&mut h);
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.window), Some(second));
    send(&mut h, press(Key::Escape, Modifiers::NONE));
    h.state_mut().run("selection.set", json!({"range": "B1"})).unwrap();
    send(&mut h, press(Key::F2, Modifiers::NONE));
    assert_eq!(mode(&h), Some(true));
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.window), Some(second));
    // A script brings window 1 forward; the draft is committed in its own window, and when its
    // workbook refuses it (too long for a cell), that window comes back with the draft open.
    h.state_mut().editor.as_mut().unwrap().text = "y".repeat(9_000);
    h.state_mut().session.activate_window(first);
    assert!(!h.state_mut().commit_edit(0, 0, false, false));
    assert_eq!(h.state().session.active_window(), Some(second));
    assert_eq!(mode(&h), Some(true), "the draft is still open");
}

/// A locked cell on a protected sheet: Python mode is refused before a draft exists, and a draft
/// whose cell sheet protection locks while it is open is dropped with the warning, as an ordinary
/// edit's is, so no draft is left that can never be entered and blocks Save (#2945).
#[test]
fn python_mode_leaves_no_draft_on_a_locked_cell() {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": "1"})).unwrap();
    s.execute("python.set", json!({"cell": "B1", "code": "2"})).unwrap();
    s.execute("home.formatCells", json!({"range": "B1", "style": {"protection": {"locked": false}}})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    let mut h = harness(s);
    shortcut(&mut h);
    assert_eq!(mode(&h), None, "no draft on the locked A1");
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("protected sheet")), "{:?}", h.state().message);
    h.state_mut().run("ui.message.dismiss", json!({})).unwrap();
    // The unlocked B1 edits; then protection locks it while the draft is open.
    h.state_mut().run("selection.set", json!({"range": "B1"})).unwrap();
    h.run_steps(2);
    shortcut(&mut h);
    assert_eq!(mode(&h), Some(true));
    type_text(&mut h, " + 1");
    h.state_mut().run("review.unprotectSheet", json!({})).unwrap();
    h.state_mut().run("home.formatCells", json!({"range": "B1", "style": {"protection": {"locked": true}}})).unwrap();
    h.state_mut().run("review.protectSheet", json!({})).unwrap();
    ctrl_enter(&mut h);
    assert_eq!(mode(&h), None, "the draft is dropped, not left open");
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("protected sheet")));
    assert_eq!(code(&h, "B1").as_deref(), Some("2"));
    assert!(h.state_mut().run("file.saveBytes", json!({"format": "xlsx"})).is_ok());
}
