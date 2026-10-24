//! Typing into a cell with a list validation searches its dropdown: the list shows the entries
//! with a word starting with the typed text, and Enter or Tab takes the highlighted one.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const CITIES: [&str; 6] = [
    "Paris - France",
    "Dubai - United Arab Emirates",
    "London - United Kingdom",
    "Boston - United States of America",
    "Los Angeles - United States of America",
    "Bogotá - Colombia",
];

fn harness_with(rule: serde_json::Value) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let rows: Vec<Vec<&str>> = CITIES.iter().map(|c| vec![*c]).collect();
    session.execute("range.setValues", json!({"range": "H1", "values": rows})).unwrap();
    session.execute("data.validation", rule).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn harness() -> Harness {
    harness_with(json!({"range": "B2:B9", "type": "list", "formula1": "=$H$1:$H$6"}))
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn text(h: &mut Harness, t: &str) {
    h.input_mut().events.push(Event::Text(t.into()));
    h.run_steps(2);
}

fn value(h: &Harness, a: &str) -> String {
    match h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))) {
        Some(Value::Text(t)) => t.to_string(),
        Some(v) => v.display(),
        None => String::new(),
    }
}

fn active(h: &Harness) -> String {
    h.state().session.active().map(|d| d.selection.active.a1()).unwrap_or_default()
}

fn shown(h: &Harness, item: &str) -> bool {
    h.query_by_label(item).is_some()
}

#[test]
fn typing_filters_the_list_to_entries_with_a_matching_word() {
    let mut h = harness();
    text(&mut h, "United");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.list_open()), "typing opens the searched list");
    for city in &CITIES[1..5] {
        assert!(shown(&h, city), "{city} matches 'United'");
    }
    assert!(!shown(&h, "Paris - France"));
    text(&mut h, " St");
    assert!(shown(&h, "Boston - United States of America"));
    assert!(shown(&h, "Los Angeles - United States of America"));
    assert!(!shown(&h, "London - United Kingdom"), "'United St' narrows to the United States");
    if let Ok(path) = std::env::var("GRIDCRAFT_LIST_SEARCH_SCREENSHOT") {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
        h.render().unwrap().save(path).unwrap();
    }
}

#[test]
fn enter_commits_the_first_match_as_the_list_spells_it() {
    let mut h = harness();
    text(&mut h, "united st");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().editor.is_none());
    assert!(h.state().message.is_none(), "no validation alert: the value is a list entry");
    assert_eq!(value(&h, "B2"), "Boston - United States of America");
    assert_eq!(active(&h), "B3");
}

#[test]
fn tab_commits_and_moves_right() {
    let mut h = harness();
    text(&mut h, "Ange");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "Los Angeles - United States of America");
    assert_eq!(active(&h), "C2");
}

#[test]
fn arrows_move_the_highlight_instead_of_committing() {
    let mut h = harness();
    text(&mut h, "United");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert!(h.state().editor.is_some(), "Down moves in the list, it doesn't commit");
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "London - United Kingdom");
}

#[test]
fn accents_and_case_are_ignored() {
    let mut h = harness();
    text(&mut h, "BOGOTA");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "Bogotá - Colombia");
}

#[test]
fn clicking_an_entry_commits_it() {
    let mut h = harness();
    text(&mut h, "United");
    h.get_by_label("London - United Kingdom").click();
    h.run_steps(3);
    assert!(h.state().editor.is_none());
    assert_eq!(value(&h, "B2"), "London - United Kingdom");
    assert_eq!(active(&h), "B2");
}

#[test]
fn escape_closes_the_list_then_cancels() {
    let mut h = harness_with(json!({"range": "B2", "type": "list", "formula1": "=$H$1:$H$6", "errorStyle": "information"}));
    text(&mut h, "United");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().editor.as_ref().is_some_and(|e| !e.list_open() && e.text == "United"), "the first Escape only hides the list");
    // With the list closed, Enter keeps the typed text (an Information rule accepts it).
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "United");

    let mut h = harness();
    text(&mut h, "United");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().editor.is_none(), "the second Escape cancels the edit");
    assert_eq!(value(&h, "B2"), "");
}

#[test]
fn no_match_leaves_the_typed_text_to_validation() {
    let mut h = harness();
    text(&mut h, "Tokyo");
    assert!(h.state().editor.as_ref().is_some_and(|e| !e.list_open()));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().message.is_some(), "a Stop rule rejects a value outside the list");
    assert_eq!(value(&h, "B2"), "");
}

#[test]
fn formulas_are_not_searched() {
    let mut h = harness();
    text(&mut h, "=H1");
    assert!(h.state().editor.as_ref().is_some_and(|e| !e.list_open()));
}

#[test]
fn a_long_list_shows_a_few_rows_and_scrolls_to_the_highlight() {
    let mut session = Session::new();
    session.new_workbook();
    let rows: Vec<Vec<String>> = (0..50_000).map(|i| vec![format!("Item {i:05}")]).collect();
    session.execute("range.setValues", json!({"range": "H1", "values": rows})).unwrap();
    session.execute("data.validation", json!({"range": "B2", "type": "list", "formula1": "=$H$1:$H$50000"})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    text(&mut h, "item");
    assert!(shown(&h, "Item 00000"));
    assert!(!shown(&h, "Item 00100"), "only the visible rows are drawn");
    for _ in 0..12 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
    }
    assert!(shown(&h, "Item 00012"), "the highlighted row scrolls into view");
    assert!(!shown(&h, "Item 00000"));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "Item 00012");
}

#[test]
fn typing_over_the_open_dropdown_replaces_it_with_the_searched_list() {
    let mut h = harness();
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let pos = grid::validation_arrow(sh, &geo, CellRef::parse("B2").unwrap()).unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().grid.list_picker.is_some(), "the arrow opens the full list");
    assert!(shown(&h, "Paris - France"));

    text(&mut h, "United St");
    assert!(h.state().grid.list_picker.is_none(), "typing closes the full list");
    assert!(!shown(&h, "Paris - France"), "only the searched list shows");
    assert_eq!(h.query_all_by_label("Boston - United States of America").count(), 1, "one list, not two");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), "Boston - United States of America");
    assert!(h.state().grid.list_picker.is_none());
    assert!(!shown(&h, "Boston - United States of America"), "no list is left open after choosing");
}

fn click_cell(h: &mut Harness, cell: &str) {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sh, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn leaving_the_cell_without_accepting_keeps_the_typed_text() {
    // An Information rule lets the typed text in, so the commit itself succeeds.
    let mut h = harness_with(json!({"range": "B2", "type": "list", "formula1": "=$H$1:$H$6", "errorStyle": "information"}));
    text(&mut h, "United");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.list_open()));
    click_cell(&mut h, "E2"); // beside the cell: the popup hangs below it
    assert!(h.state().editor.is_none(), "clicking another cell commits the edit");
    assert_eq!(value(&h, "B2"), "United", "only Enter, Tab or a click on the list takes its entry");

    // Ctrl+Enter is an Enter: it takes the highlighted entry.
    let mut h = harness();
    text(&mut h, "Ange");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(value(&h, "B2"), "Los Angeles - United States of America");
}

#[test]
fn the_formula_bar_enter_button_takes_the_highlighted_entry() {
    let mut h = harness();
    text(&mut h, "united st");
    // The ✓ button sits right of the Name Box (118 wide), after the ✕ button: both are 22 wide
    // with 4-point gaps. The Name Box is the leftmost text field; its text starts 6 points in.
    let name_box = h.get_all_by_role(egui::accesskit::Role::TextInput).map(|n| n.rect()).min_by(|a, b| a.left().total_cmp(&b.left())).unwrap();
    let pos = egui::pos2(name_box.left() - 6.0 + 118.0 + 4.0 + 4.0 + 4.0 + 22.0 + 4.0 + 11.0, name_box.center().y);
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert!(h.state().editor.is_none(), "the Enter button commits");
    assert!(h.state().message.is_none(), "no validation alert: the list entry was taken");
    assert_eq!(value(&h, "B2"), "Boston - United States of America");
    assert_eq!(active(&h), "B2", "the Enter button doesn't move the selection");
}
