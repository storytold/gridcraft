//! "Only" in the AutoFilter menu (#1409): one click filters the column to that value, without
//! unticking (Select All), ticking the value and pressing Apply. The button shows on the hovered row.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A1:A6 holds a header, fruits and a blank under an AutoFilter, with the menu open on A1.
fn filter_menu_open() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let values = json!([["Fruit"], ["Apple"], ["Banana"], [null], ["Apple"], ["Cherry"]]);
    session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    session.execute("data.filter", json!({"range": "A1:A6"})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    open_menu(&mut h);
    h
}

fn open_menu(h: &mut Harness) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key: Key::ArrowDown,
            physical_key: Some(Key::ArrowDown),
            pressed,
            repeat: false,
            modifiers: Modifiers::ALT,
        });
    }
    h.run_steps(2);
    assert!(h.state().grid.filter_menu.is_some(), "Alt+Down opens the filter menu");
}

fn hover(h: &mut Harness, at: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
}

/// Centres of the "Only" buttons that are showing.
fn only_buttons(h: &Harness) -> Vec<Pos2> {
    h.query_all_by_label("Only").filter(|n| !n.accesskit_node().is_hidden() && !n.accesskit_node().is_disabled()).map(|n| n.rect().center()).collect()
}

fn click_only(h: &mut Harness, value: &str) {
    let row = h.get_by_label(value).rect().center();
    hover(h, row);
    let shown = only_buttons(h);
    assert_eq!(shown.len(), 1, "only the hovered row shows its button: {shown:?}");
    let at = shown[0];
    assert!((at.y - row.y).abs() < 1.0, "the button sits on {value}'s row");
    hover(h, at);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

/// Rows 2..=6 (Apple, Banana, blank, Apple, Cherry): which ones the filter hides.
fn hidden(h: &Harness) -> Vec<bool> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    (1..=5).map(|r| sh.is_row_hidden(r)).collect()
}

#[test]
fn only_filters_to_the_hovered_value_and_closes_the_menu() {
    let mut h = filter_menu_open();
    assert!(only_buttons(&h).is_empty(), "no row is hovered yet");
    click_only(&mut h, "Apple");
    assert_eq!(hidden(&h), [false, true, true, false, true], "both Apple rows stay, nothing else");
    assert!(h.state().grid.filter_menu.is_none(), "Only applies at once and closes the menu");
}

#[test]
fn only_on_blanks_keeps_just_the_empty_rows() {
    let mut h = filter_menu_open();
    click_only(&mut h, "(Blanks)");
    assert_eq!(hidden(&h), [true, true, false, true, true]);
}

#[test]
fn only_replaces_an_existing_filter_on_the_column() {
    let mut h = filter_menu_open();
    click_only(&mut h, "Banana");
    assert_eq!(hidden(&h), [true, false, true, true, true]);
    // Banana is the only value left ticked; Only on Cherry swaps it, it doesn't add to it.
    open_menu(&mut h);
    click_only(&mut h, "Cherry");
    assert_eq!(hidden(&h), [true, true, true, true, false]);
}
