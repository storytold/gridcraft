//! Keys that arrive in the same frame as the one that starts a cell edit are typed into the cell,
//! in the order they were typed, as on a slow machine where a quick typist's keys pile up
//! between frames.

#[path = "common/mod.rs"]
mod common;

use common::Harness;
use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    common::os_harness(session, egui::os::OperatingSystem::Nix)
}

/// One key as a keyboard sends it: press, the character it types (if any), release.
fn key(events: &mut Vec<Event>, k: Key, text: Option<&str>) {
    events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE });
    if let Some(t) = text {
        events.push(Event::Text(t.into()));
    }
    events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: Modifiers::NONE });
}

/// Digits and letters with their keys; other characters (`ż`, `€`) as text only, as a keymap
/// that egui has no key for sends them.
fn typed(events: &mut Vec<Event>, s: &str) {
    for c in s.chars() {
        let t = c.to_string();
        match Key::from_name(&c.to_ascii_uppercase().to_string()) {
            Some(k) => key(events, k, Some(&t)),
            None => events.push(Event::Text(t)),
        }
    }
}

/// `events` in one frame, then enough frames for every held key.
fn send(h: &mut Harness, events: Vec<Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(8);
}

fn cell(h: &Harness, a1: &str) -> Value {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

fn active(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn every_key_of_one_frame_reaches_the_cell() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "123");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(123.0), "all three digits are in the cell");
    assert!(h.state().editor.is_none(), "Enter in the same frame commits the edit");
    assert_eq!(active(&h), "B3", "and moves down");
}

#[test]
fn typing_without_enter_leaves_the_edit_open_with_every_character() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "123");
    send(&mut h, events);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("123"));
    assert_eq!(cell(&h, "B2"), Value::Empty, "nothing is committed yet");
    let mut events = Vec::new();
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(123.0));
}

#[test]
fn a_text_event_with_several_characters_is_kept_whole() {
    let mut h = harness();
    let mut events = vec![Event::Text("ab".into())];
    typed(&mut events, "c");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), text("abc"));
}

#[test]
fn backspace_between_typed_keys_takes_back_the_character_before_it() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "12");
    key(&mut events, Key::Backspace, None);
    typed(&mut events, "3");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(13.0));
}

#[test]
fn an_arrow_commits_and_the_keys_after_it_go_to_the_next_cell() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "1");
    key(&mut events, Key::ArrowRight, None);
    typed(&mut events, "2");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(1.0));
    assert_eq!(cell(&h, "C2"), Value::Number(2.0));
}

#[test]
fn enter_and_tab_between_typed_keys_keep_their_place() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "1");
    key(&mut events, Key::Enter, None);
    typed(&mut events, "2");
    key(&mut events, Key::Tab, None);
    typed(&mut events, "3");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(1.0));
    assert_eq!(cell(&h, "B3"), Value::Number(2.0));
    assert_eq!(cell(&h, "C3"), Value::Number(3.0));
    assert!(h.state().editor.is_none());
}

#[test]
fn keys_before_the_typing_act_on_the_grid_first() {
    let mut h = harness();
    let mut events = Vec::new();
    key(&mut events, Key::Enter, None);
    typed(&mut events, "12");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Empty, "the first Enter only moves");
    assert_eq!(cell(&h, "B3"), Value::Number(12.0));
    assert_eq!(active(&h), "B4");
}

#[test]
fn escape_cancels_what_was_typed_before_it_and_nothing_after() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "12");
    key(&mut events, Key::Escape, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Empty);
    assert!(h.state().editor.is_none());

    let mut h = harness();
    let mut events = Vec::new();
    key(&mut events, Key::Escape, None);
    typed(&mut events, "5");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), Value::Number(5.0), "an Escape before the typing does not cancel it");
}

#[test]
fn a_paste_after_typed_keys_goes_into_the_cell_after_them() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "1");
    events.push(Event::Paste("xyz".into()));
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), text("1xyz"));
}

#[test]
fn characters_outside_ascii_arrive_in_order() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "żół€");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), text("żół€"));
}

#[test]
fn typing_into_an_open_editor_is_not_doubled() {
    let mut h = harness();
    let mut events = Vec::new();
    typed(&mut events, "a");
    send(&mut h, events);
    let mut events = Vec::new();
    typed(&mut events, "bc");
    send(&mut h, events);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("abc"));
    let mut events = Vec::new();
    typed(&mut events, "d");
    key(&mut events, Key::Enter, None);
    send(&mut h, events);
    assert_eq!(cell(&h, "B2"), text("abcd"));
}

#[test]
fn a_shortcut_that_types_a_space_does_not_start_an_edit() {
    let mut h = harness();
    let shift = Modifiers::SHIFT;
    let events = vec![
        Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: shift },
        Event::Text(" ".into()),
        Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: false, repeat: false, modifiers: shift },
    ];
    send(&mut h, events);
    assert!(h.state().editor.is_none(), "Shift+Space selects the row");
}
