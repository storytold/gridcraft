//! The punctuation key that ends a Hangul composition, as the macOS Korean 2-Set IME sends it.
//!
//! While a cell is edited, Apple Korean commits the syllable being composed and then inserts the
//! punctuation the user pressed. On macOS a key press that committed text sends no `KeyboardInput`
//! (`committed_in_key_down` in `storytold/winit`), so the punctuation only reaches the app when the
//! input method's own `insertText` is committed too: `Ime::Commit(syllable)` then `Ime::Commit(",")`
//! in the same pass, from `storytold/winit` W28. Before that pin the comma was left to the key's
//! `KeyboardInput` and silently dropped, so `가,` stored `가` (#2703).
//!
//! These tests pin the editor side of the contract: the syllable's commit and the punctuation that
//! follows it each land once in the cell being edited, whichever editor takes the stream, and
//! ordinary typing is unchanged. The events are synthetic — the harness's bundled fonts carry no
//! Hangul, so the syllables are Latin stand-ins (`h`, `ha`, `ga` for `ㅎ`, `하`, `가`), and no real
//! macOS IME was driven here.

use egui::{Event, ImeEvent, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::formula_bar::BarLayout;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// B2 active on a blank workbook, a few frames in so the grid owns the keyboard.
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    crate::support::harness(session, 4)
}

/// A key press and release, then the frames that apply them.
fn key(h: &mut Harness, k: Key) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// One frame of input events (they arrive together, as they do from an input method).
fn frame(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.to_string(), active_range_chars: None })
}

fn commit(text: &str) -> Event {
    Event::Ime(ImeEvent::Commit(text.to_string()))
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|ed| ed.text.clone())
}

/// The text stored in `B2` after Enter commits the edit.
fn stored(h: &mut Harness) -> Value {
    key(h, Key::Enter);
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().cell(CellRef::new(1, 1)).map(|c| c.value.clone()).unwrap_or(Value::Empty)
}

/// `r k ,`: two preedits for the syllable, then the comma's key press commits the syllable and
/// inserts the comma. Both commits arrive in one pass and the cell stores `ga,`.
#[test]
fn punctuation_committed_after_a_syllable_lands_once() {
    let mut h = harness();
    key(&mut h, Key::F2);
    frame(&mut h, [preedit("h")]);
    frame(&mut h, [preedit("ha")]);
    assert_eq!(editor_text(&h).as_deref(), Some("ha"), "the composition shows as it is typed");
    frame(&mut h, [commit("ga"), commit(",")]);
    let ed = h.state().editor.as_ref().expect("still editing");
    assert_eq!(ed.text, "ga,", "the comma that ended the composition was lost or doubled");
    assert_eq!(ed.caret, 3, "the caret sits after the comma");
    assert_eq!(stored(&mut h), Value::Text("ga,".into()), "the cell stores the comma");
}

/// An input method that commits the punctuation with the syllable (`Ime::Commit("ga,")`) inserts it
/// once: no other path types it again.
#[test]
fn a_punctuation_committed_with_the_syllable_lands_once() {
    let mut h = harness();
    key(&mut h, Key::F2);
    frame(&mut h, [commit("ga,")]);
    assert_eq!(editor_text(&h).as_deref(), Some("ga,"));
    assert_eq!(stored(&mut h), Value::Text("ga,".into()));
}

/// Typing with no composition still types the punctuation once, as text (the no-IME path).
#[test]
fn typed_punctuation_without_a_composition_lands_once() {
    let mut h = harness();
    key(&mut h, Key::F2);
    frame(&mut h, [Event::Text("ga".into())]);
    frame(&mut h, [Event::Text(",".into())]);
    assert_eq!(editor_text(&h).as_deref(), Some("ga,"));
    assert_eq!(stored(&mut h), Value::Text("ga,".into()));
}

/// The formula bar edits the same draft and takes the same stream, so it lands there once too.
#[test]
fn the_formula_bar_takes_the_commit_and_its_punctuation_once() {
    let mut h = harness();
    let field = BarLayout::get(&h.ctx).field;
    let at = field.center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let ed = h.state().editor.as_ref().expect("clicking the formula bar edits the cell");
    assert!(ed.from_formula_bar, "the draft belongs to the formula bar");
    frame(&mut h, [preedit("ha")]);
    frame(&mut h, [commit("ga"), commit(",")]);
    assert_eq!(editor_text(&h).as_deref(), Some("ga,"));
    assert_eq!(stored(&mut h), Value::Text("ga,".into()));
}
