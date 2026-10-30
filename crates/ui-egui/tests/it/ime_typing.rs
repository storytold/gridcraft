//! An entry typed over a selected cell through an input method keeps what it composes whole:
//! the grid lets the input method compose before an edit starts, and the composition carries
//! into the in-cell editor without being cancelled. Korean 2-Set typing lost or split its first
//! syllable. The events below have the shape macOS sends for Korean (a syllable is committed
//! together with the start of the next one); the composed text is Vietnamese Telex, which the
//! harness's bundled fonts can draw.
use egui::{Event, ImeEvent, Key};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;

use crate::common::{Harness, app, builder, harness_with};

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    harness_with(builder().with_size(egui::vec2(1200.0, 800.0)), app(session), 4)
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn commit(text: &str) -> Event {
    Event::Ime(ImeEvent::Commit(text.into()))
}

fn a1(h: &Harness) -> Option<String> {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().cell(CellRef::new(0, 0)).map(|c| c.input_text())
}

fn draft(h: &Harness) -> Option<&str> {
    h.state().editor.as_ref().map(|ed| ed.text.as_str())
}

/// One key per frame; the composition in progress must survive every one of them.
fn type_keys(h: &mut Harness, keys: Vec<Vec<Event>>) {
    for events in keys {
        h.input_mut().events.extend(events);
        h.run_steps(1);
        let ime = h.output().platform_output.ime.as_ref();
        assert!(ime.is_some(), "the input method stays allowed while the entry is typed");
        assert!(!ime.is_some_and(|ime| ime.should_interrupt_composition), "the composition in progress is never cancelled");
    }
}

#[test]
fn the_selected_cell_lets_the_input_method_compose() {
    let h = harness();
    let ime = h.output().platform_output.ime.as_ref().expect("the grid allows the input method");
    assert!(ime.rect.is_positive(), "the candidate window opens at the active cell");
}

#[test]
fn a_composition_typed_over_a_cell_stays_whole() {
    let mut h = harness();
    // a a e e: â is committed when e starts the next letter.
    type_keys(&mut h, vec![vec![preedit("a")], vec![preedit("â")], vec![preedit(""), commit("â"), preedit("e")], vec![preedit("ê")]]);
    assert_eq!(draft(&h), Some("âê"), "the draft shows the letter being composed");
    // Return ends the composition (the input method keeps that key); the next one enters the cell.
    h.input_mut().events.extend([preedit(""), commit("ê")]);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(a1(&h).as_deref(), Some("âê"));
}

#[test]
fn an_input_method_that_replaces_the_letter_just_typed_finds_it() {
    let mut h = harness();
    // macOS input methods may type the first letter plainly, then replace it with the composed one.
    type_keys(
        &mut h,
        vec![vec![Event::Text("o".into())], vec![Event::Ime(ImeEvent::DeleteSurroundingBytes { before_bytes: 1, after_bytes: 0 }), commit("ô")]],
    );
    assert_eq!(draft(&h), Some("ô"));
}

#[test]
fn typing_after_the_composition_goes_on_in_the_cell_editor() {
    let mut h = harness();
    type_keys(&mut h, vec![vec![preedit("o")], vec![preedit("ô")]]);
    // Space ends the composition; the in-cell editor then has the keyboard for the rest.
    h.input_mut().events.extend([preedit(""), commit("ô"), Event::Text(" ".into())]);
    h.run_steps(2);
    for c in ["1", "2"] {
        h.input_mut().events.push(Event::Text(c.into()));
        h.run_steps(2);
    }
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(a1(&h).as_deref(), Some("ô 12"));
}

#[test]
fn deleting_the_whole_composition_leaves_an_empty_draft() {
    let mut h = harness();
    type_keys(&mut h, vec![vec![preedit("a")]]);
    // Backspace inside the composition: the input method clears it.
    h.input_mut().events.push(preedit(""));
    h.run_steps(2);
    assert_eq!(draft(&h), Some(""));
}
