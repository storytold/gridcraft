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
    assert!(h.state().grid.list_picker_items.is_none(), "and drops the entries it read");
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

#[test]
fn a_list_from_a_huge_column_is_read_within_the_source_budget() {
    // Typing into a cell whose list is a whole column used far past the budget reads only the
    // budget's worth of it, and the edit knows the list was cut short.
    let budget = gridcraft_engine::cmd::data::LIST_SOURCE_BUDGET;
    let rows: Vec<Vec<String>> = (0..budget + 1_000).map(|r| vec![format!("Item {r}")]).collect();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "H1", "values": rows})).unwrap();
    session.execute("data.validation", json!({"range": "B2", "type": "list", "formula1": "=$H:$H"})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    text(&mut h, "zzz");
    let list = h.state().editor.as_ref().and_then(|e| e.list.as_ref()).map(|l| (l.items.len(), l.truncated, l.matches.len()));
    assert_eq!(list, Some((budget, true, 0)));
}

#[test]
fn the_dropdown_arrow_reads_a_huge_list_once_and_draws_only_its_visible_rows() {
    // The full list the arrow opens comes from a whole column used far past the search budget: it
    // is read once within the budget (not again every frame) and only the rows in view are drawn.
    let budget = gridcraft_engine::cmd::data::LIST_SOURCE_BUDGET;
    let rows: Vec<Vec<String>> = (0..budget + 1_000).map(|r| vec![format!("Item {r}")]).collect();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "H1", "values": rows})).unwrap();
    session.execute("data.validation", json!({"range": "B2", "type": "list", "formula1": "=$H:$H"})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert!(h.state().grid.list_picker.is_some(), "Alt+Down opens the full list");
    let read = |h: &Harness| h.state().grid.list_picker_items.as_ref().map(|l| (l.source.items.len(), l.source.truncated, l.source.items.as_ptr()));
    let first = read(&h);
    assert_eq!(first.map(|(n, t, _)| (n, t)), Some((budget, true)));
    h.run_steps(3);
    assert_eq!(read(&h), first, "the open list is not read again");
    assert!(shown(&h, "Item 0"));
    assert!(!shown(&h, "Item 500"), "rows out of view are not drawn");
    assert!(shown(&h, "More entries not shown"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().grid.list_picker.is_none());
    assert!(h.state().grid.list_picker_items.is_none(), "closing drops the list");
}

#[test]
fn the_highlighted_row_stays_inside_the_popup() {
    // 50,000 entries match "item": the list is cut at the match cap, so the note sits right under
    // the rows. A highlighted row drawn below the note's top is outside the popup's visible rows
    // (rows used to be laid out 18 px apart while drawn 24 px tall).
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
    let inside = |h: &Harness, item: &str| {
        let row = h.get_by_label(item).rect();
        let note = h.get_by_label("More entries not shown").rect();
        assert!(row.bottom() <= note.top() + 0.5, "{item} at {row:?} runs under the note at {note:?}");
    };
    let visible = gridcraft_ui_egui::editor::ListSearch::VISIBLE;
    for _ in 1..visible {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
    }
    // The last row that fits without scrolling.
    inside(&h, &format!("Item {:05}", visible - 1));
    assert!(shown(&h, "Item 00000"), "the list did not need to scroll yet");
    // Up from the first entry wraps to the last kept match: the end of the list is reachable.
    for _ in 1..visible {
        key(&mut h, Key::ArrowUp, Modifiers::NONE);
    }
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    let last = format!("Item {:05}", gridcraft_engine::cmd::data::LIST_MATCH_CAP - 1);
    inside(&h, &last);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), last);
}

#[test]
fn the_searched_list_opens_above_a_cell_near_the_bottom_of_the_window() {
    let mut h = harness_with(json!({"range": "B2:B200", "type": "list", "formula1": "=$H$1:$H$6"}));
    // The last row of column B that is wholly in view.
    let (cell, top) = {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        let rect = app.grid.rect.unwrap();
        let geo = grid::Geo::new(sh, rect, app.view().scroll);
        let row = (1..200).take_while(|&r| geo.cell_rect(sh, CellRef::new(r, 1)).bottom() <= rect.bottom()).last().unwrap();
        let cell = CellRef::new(row, 1);
        (cell, geo.cell_rect(sh, cell).top())
    };
    h.state_mut().session.execute("selection.set", json!({"cell": cell.a1()})).unwrap();
    h.run_steps(2);
    text(&mut h, "united");
    let row = h.get_by_label("Boston - United States of America").rect();
    assert!(row.bottom() <= top + 0.5, "the list at {row:?} is above the cell (top {top})");
    assert!(row.top() >= 0.0);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, &cell.a1()), "Dubai - United Arab Emirates");
}
