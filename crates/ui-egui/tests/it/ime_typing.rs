//! Typing with an input method straight into a cell (fcitx5-unikey's Vietnamese, as XIM's
//! on-the-spot style delivers it): the word being composed opens the cell's editor and shows in
//! the cell, and the finished word goes in as typed text (before, the composition stayed in a box
//! of the input method's own until Space).

use egui::{Event, ImeEvent, Key, vec2};
use gridcraft_engine::Session;

use crate::common::{Harness, app, builder, harness_with};

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness_with(builder().with_size(vec2(1200.0, 800.0)), app(session), 0);
    for _ in 0..4 {
        step(&mut h);
    }
    h
}

/// One frame, its input through the app's `raw_input_hook` as eframe gives it.
fn step(h: &mut Harness) {
    let ctx = h.ctx.clone();
    let mut raw = std::mem::take(h.input_mut());
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.run_steps(1);
}

/// `events` in one frame's input, as the integration batches them.
fn batch(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

/// Enter, then the text A1 holds.
fn enter(h: &mut Harness) -> String {
    h.key_press(Key::Enter);
    step(h);
    step(h);
    a1(h)
}

fn preedit(text: &str) -> Event {
    let n = text.chars().count();
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: Some(n..n) })
}

fn a1(h: &Harness) -> String {
    let s = &h.state().session;
    let d = s.active().unwrap();
    gridcraft_engine::display::edit_text(&d.wb, d.wb.active().unwrap(), gridcraft_engine::core::CellRef::new(0, 0))
}

#[test]
fn composing_straight_into_a_cell_shows_in_the_cell() {
    let mut h = harness();
    assert!(h.output().platform_output.ime.is_some(), "the input method is on at the active cell");
    // Telex "chaof" as unikey composes it, one keystroke a frame (letters the test fonts have).
    for word in ["c", "ch", "cha", "chao", "chào"] {
        h.event(preedit(word));
        step(&mut h);
        step(&mut h);
    }
    let shown = h.state().editor.as_ref().map(|e| e.text.clone());
    assert_eq!(shown.as_deref(), Some("chào"), "the word being composed is in the cell's editor");
    // Space finishes the word: the composition is cleared, the word committed, the space typed.
    h.event(preedit(""));
    h.event(Event::Ime(ImeEvent::Commit("chào".into())));
    step(&mut h);
    h.event(Event::Text(" ".into()));
    step(&mut h);
    h.key_press(Key::Enter);
    step(&mut h);
    step(&mut h);
    assert_eq!(a1(&h).trim_end(), "chào");
}

/// The first letter of a word opens the cell's editor and moves the focus there; egui interrupts
/// the input method on every focus move, and fcitx5 then dropped the letter it was composing
/// ("không" came out "hong"). The input method stays on and uninterrupted throughout.
#[test]
fn opening_the_editor_keeps_the_letter_being_composed() {
    let mut h = harness();
    h.event(preedit("k"));
    for frame in 0..4 {
        step(&mut h);
        let ime = h.output().platform_output.ime.clone();
        assert!(ime.is_some(), "frame {frame}: the input method stays on");
        assert!(!ime.unwrap().should_interrupt_composition, "frame {frame}: the composition of \"k\" is not interrupted");
    }
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("k"));
    // The input method goes on with the same word.
    for word in ["kh", "kho", "khô", "khôn", "không"] {
        h.event(preedit(word));
        step(&mut h);
        assert!(!h.output().platform_output.ime.as_ref().unwrap().should_interrupt_composition);
    }
    h.event(preedit(""));
    h.event(Event::Ime(ImeEvent::Commit("không".into())));
    step(&mut h);
    h.key_press(Key::Enter);
    step(&mut h);
    step(&mut h);
    assert_eq!(a1(&h), "không");
}

/// An input method that sends only finished words (no composition) types them into the cell.
#[test]
fn a_finished_word_without_a_composition_is_typed_into_the_cell() {
    let mut h = harness();
    h.event(Event::Ime(ImeEvent::Commit("Đà".into())));
    step(&mut h);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("Đà"));
    h.key_press(Key::Enter);
    step(&mut h);
    assert_eq!(a1(&h), "Đà");
}

/// A composition and the commit that finishes it in one batch: the commit is not lost with the
/// rest of the batch when the first composition opens the editor.
#[test]
fn a_composition_finished_in_the_same_batch_reaches_the_cell() {
    let mut h = harness();
    batch(&mut h, [preedit("đ"), preedit(""), Event::Ime(ImeEvent::Commit("đ".into()))]);
    for _ in 0..6 {
        step(&mut h);
    }
    assert_eq!(editor_text(&h).as_deref(), Some("đ"));
    assert_eq!(enter(&mut h), "đ");
}

/// Two finished words in one batch: the second follows the first into the editor.
#[test]
fn two_finished_words_in_one_batch_both_reach_the_cell() {
    let mut h = harness();
    batch(&mut h, [Event::Ime(ImeEvent::Commit("Cà".into())), Event::Ime(ImeEvent::Commit(" Mau".into()))]);
    for _ in 0..6 {
        step(&mut h);
    }
    assert_eq!(editor_text(&h).as_deref(), Some("Cà Mau"));
    assert_eq!(enter(&mut h), "Cà Mau");
}

/// The input method finishes the word while the editor is still taking the focus: the first
/// composition reaches the editor before that commit, so no stale composition is left after it.
#[test]
fn a_word_finished_while_the_editor_opens_is_not_composed_again() {
    let mut h = harness();
    batch(&mut h, [preedit("k")]);
    step(&mut h);
    batch(&mut h, [preedit(""), Event::Ime(ImeEvent::Commit("k".into()))]);
    for _ in 0..6 {
        step(&mut h);
    }
    assert_eq!(editor_text(&h).as_deref(), Some("k"));
    assert_eq!(enter(&mut h), "k");
}

/// Synthetic typing (`ui.text`) while the editor an input method opened takes the focus waits for
/// the typing held for the editor, so it comes after the words the input method finished.
#[test]
fn synthetic_typing_while_the_editor_opens_follows_the_held_words() {
    let mut h = harness();
    batch(&mut h, [Event::Ime(ImeEvent::Commit("Cà".into())), Event::Ime(ImeEvent::Commit(" Mau".into()))]);
    step(&mut h);
    h.state_mut().synthetic.push_back(vec![Event::Text("!".into())]);
    for _ in 0..6 {
        step(&mut h);
    }
    assert_eq!(editor_text(&h).as_deref(), Some("Cà Mau!"));
    assert_eq!(enter(&mut h), "Cà Mau!");
}
