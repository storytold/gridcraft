//! Enter in the AutoFilter menu works like its Apply button, as Excel's OK (issue #1516): from the
//! search box or with a value focused it applies the filter and closes the menu, and the key never
//! reaches the grid underneath.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A1:A4 holds a header and three fruits under an AutoFilter, with the menu opened by Alt+Down on A1.
fn filter_menu_open() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Fruit"], ["Apple"], ["Banana"], ["Cherry"]]})).unwrap();
    session.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert!(h.state().grid.filter_menu.is_some(), "Alt+Down opens the filter menu");
    h
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

/// Rows 2..=4 (Apple, Banana, Cherry): which ones the filter hides.
fn hidden(h: &Harness) -> Vec<bool> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    (1..=3).map(|r| sh.is_row_hidden(r)).collect()
}

fn active(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

#[test]
fn enter_in_the_search_box_applies_the_filter() {
    let mut h = filter_menu_open();
    h.input_mut().events.push(Event::Text("an".into()));
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(hidden(&h), [true, false, true], "only Banana matches the search");
    assert!(h.state().grid.filter_menu.is_none(), "Enter closes the menu");
    assert_eq!(active(&h), "A1", "Enter must not move the active cell");
}

#[test]
fn enter_with_a_value_clicked_applies_the_filter() {
    let mut h = filter_menu_open();
    let apple = h.get_by_label("Apple").rect().center();
    h.input_mut().events.push(Event::PointerMoved(apple));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: apple, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
    assert!(h.state().grid.filter_menu.is_some(), "clicking a value keeps the menu open");
    key(&mut h, Key::Enter, Modifiers::NONE);
    // Enter applies the list as it is: it must not toggle the value back on.
    assert_eq!(hidden(&h), [true, false, false], "Apple was unchecked");
    assert!(h.state().grid.filter_menu.is_none(), "Enter closes the menu");
    assert_eq!(active(&h), "A1", "Enter must not move the active cell");
}

#[test]
fn enter_with_nothing_checked_keeps_the_menu_open() {
    let mut h = filter_menu_open();
    // A search that matches nothing leaves no value to show, as Excel's OK is then disabled.
    h.input_mut().events.push(Event::Text("Bananna".into()));
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(hidden(&h), [false, false, false], "an empty selection must not hide every row");
    assert!(h.state().grid.filter_menu.is_some(), "the menu stays open");
    assert_eq!(active(&h), "A1");
}
