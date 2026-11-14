//! Keyboard-only references to another sheet: Ctrl+PageDown / Ctrl+PageUp while a formula waits
//! for a reference switch sheets without ending the edit, and the arrows point on the sheet shown.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// Two sheets with B2 selected on the first, `=` typed there.
fn pointing(text: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({})).unwrap();
    session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    session.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = crate::support::harness(session, 4);
    h.state_mut().begin_edit(Some(text.into()), false);
    h.run_steps(4);
    h
}

fn press(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

/// A key pressed with Shift held, as the OS reports it: the modifier changes first.
fn press_shift(h: &mut Harness, k: Key) {
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    press(h, k, Modifiers::SHIFT);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(1);
}

fn text(h: &Harness) -> String {
    h.state().editor.as_ref().map(|e| e.text.clone()).unwrap_or_default()
}

fn shown(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active_sheet()
}

fn second_name(h: &Harness) -> String {
    h.state().session.active().unwrap().wb.sheet(1).unwrap().name.clone()
}

#[test]
fn ctrl_page_down_goes_on_pointing_on_the_next_sheet() {
    let mut h = pointing("=");
    press(&mut h, Key::PageDown, CTRL);
    assert_eq!(shown(&h), 1, "the next sheet is shown");
    assert!(h.state().editor.is_some(), "the edit goes on");
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(text(&h), format!("={}!A2", second_name(&h)), "the arrow points from the cell selected on that sheet and names the sheet");
}

#[test]
fn ctrl_page_up_comes_back_and_the_reference_loses_its_sheet() {
    let mut h = pointing("=");
    press(&mut h, Key::PageDown, CTRL);
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    press(&mut h, Key::PageUp, CTRL);
    assert_eq!(shown(&h), 0);
    press(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(text(&h), "=C2", "the pointed reference is replaced by one on the sheet being edited, from its selected cell");
}

#[test]
fn enter_commits_on_the_sheet_the_edit_started_on() {
    let mut h = pointing("=");
    press(&mut h, Key::PageDown, CTRL);
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().editor.is_none(), "Enter ends the edit");
    assert_eq!(shown(&h), 0, "the sheet the formula was typed on is shown again");
    let wb = &h.state().session.active().unwrap().wb;
    assert!(wb.sheet(0).unwrap().cell(CellRef::parse("B2").unwrap()).is_some_and(|c| c.formula.is_some()));
}

#[test]
fn text_that_is_not_a_formula_leaves_ctrl_page_down_alone() {
    let mut h = pointing("abc");
    press(&mut h, Key::PageDown, CTRL);
    assert_eq!(shown(&h), 0, "only a formula waiting for a reference switches sheets");
}

#[test]
fn a_formula_edited_in_the_formula_bar_leaves_ctrl_page_down_alone() {
    let mut h = pointing("=");
    h.state_mut().cancel_edit();
    h.state_mut().begin_edit(Some("=".into()), true);
    h.run_steps(4);
    press(&mut h, Key::PageDown, CTRL);
    assert_eq!(shown(&h), 0, "the arrows do not point there, so the sheet does not switch");
}

#[test]
fn on_the_last_sheet_ctrl_page_down_keeps_the_pointing_where_it_was() {
    let mut h = pointing("=");
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(text(&h), "=B3");
    press(&mut h, Key::PageDown, CTRL);
    press(&mut h, Key::PageDown, CTRL);
    assert_eq!(shown(&h), 1, "the last sheet is as far as it goes");
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(text(&h), format!("={}!A2", second_name(&h)));
    press(&mut h, Key::PageDown, CTRL);
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(text(&h), format!("={}!A3", second_name(&h)), "no sheet change, so the pointing goes on from A2");
}

#[test]
fn shift_arrow_after_a_sheet_switch_starts_the_range_at_the_selected_cell() {
    let mut h = pointing("=");
    press(&mut h, Key::ArrowRight, Modifiers::NONE);
    press_shift(&mut h, Key::ArrowRight);
    assert_eq!(text(&h), "=C2:D2");
    press(&mut h, Key::PageDown, CTRL);
    press_shift(&mut h, Key::ArrowDown);
    assert_eq!(text(&h), format!("={}!A1:A2", second_name(&h)));
}
