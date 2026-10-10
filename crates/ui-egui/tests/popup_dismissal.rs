//! Custom worksheet popups must keep inside interactions and dismiss outside clicks.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

#[derive(Clone, Copy, Debug)]
enum Menu {
    Header,
    Filter,
    Validation,
}

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["Red", 1], ["Blue", 2]]})).unwrap();
    session.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    session.execute("insert.table", json!({"range": "A1:B3"})).unwrap();
    session.execute("range.setValues", json!({"range": "H1", "values": [["Other", "Qty"], ["Green", 3]]})).unwrap();
    session.execute("insert.table", json!({"range": "H1:I2"})).unwrap();
    session.execute("data.validation", json!({"range": "E3", "type": "list", "formula1": "\"First,Second\""})).unwrap();
    session.execute("home.insertSheet", json!({})).unwrap();
    session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
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

fn geo(h: &Harness) -> grid::Geo {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll)
}

fn cell_pos(h: &Harness, cell: &str) -> Pos2 {
    geo(h).cell_rect(h.state().session.active().unwrap().wb.active().unwrap(), CellRef::parse(cell).unwrap()).center()
}

fn click(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn open(h: &mut Harness, menu: Menu) {
    let geometry = geo(h);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    let (pos, button) = match menu {
        Menu::Header => (pos2(cell_pos(h, "A1").x, (geometry.rect.top() + geometry.cells.top()) / 2.0), PointerButton::Secondary),
        Menu::Filter => (grid::filter_button_rect(&geometry, sh, CellRef::parse("A1").unwrap()).center(), PointerButton::Primary),
        Menu::Validation => {
            let cell = CellRef::parse("E3").unwrap();
            let pos = grid::validation_arrow(sh, &geometry, cell).unwrap().center();
            h.state_mut().run("selection.set", json!({"cell": "E3"})).unwrap();
            h.run_steps(2);
            (pos, PointerButton::Primary)
        }
    };
    click(h, pos, button);
    assert!(is_open(h, menu), "{menu:?} must survive its opening click");
}

fn is_open(h: &Harness, menu: Menu) -> bool {
    match menu {
        Menu::Header => h.state().grid.header_menu.is_some(),
        Menu::Filter => h.state().grid.filter_menu.is_some(),
        Menu::Validation => h.state().grid.list_picker.is_some(),
    }
}

#[test]
fn worksheet_popups_close_on_grid_and_ribbon_clicks() {
    for menu in [Menu::Header, Menu::Filter, Menu::Validation] {
        for ribbon in [false, true] {
            let mut h = harness();
            open(&mut h, menu);
            let outside = if ribbon { pos2(1050.0, 52.0) } else { cell_pos(&h, "H7") };
            click(&mut h, outside, PointerButton::Primary);
            assert!(!is_open(&h, menu), "{menu:?} stayed open after outside click (ribbon={ribbon})");
            if !ribbon {
                assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "H7");
            }
        }
    }
}

#[test]
fn filter_closes_when_clicking_another_table_or_sheet() {
    for sheet in [false, true] {
        let mut h = harness();
        open(&mut h, Menu::Filter);
        let outside = if sheet { pos2(44.0, h.state().grid.rect.unwrap().bottom() + 16.0) } else { cell_pos(&h, "H2") };
        click(&mut h, outside, PointerButton::Primary);
        assert!(!is_open(&h, Menu::Filter), "filter stayed open after switching table/sheet");
        if sheet {
            assert_eq!(h.state().session.active().unwrap().wb.active_sheet, 1);
        } else {
            assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "H2");
        }
    }
}

#[test]
fn popup_actions_still_execute_and_filter_checkboxes_stay_open() {
    let mut h = harness();
    open(&mut h, Menu::Header);
    h.get_by_label("Copy").click();
    h.run_steps(2);
    assert!(!is_open(&h, Menu::Header));
    assert!(h.state().session.clipboard.is_some());

    open(&mut h, Menu::Filter);
    h.get_by_label("(Select All)").click();
    h.run_steps(2);
    assert!(is_open(&h, Menu::Filter), "clicking a checkbox must keep the filter open");
    let outside = cell_pos(&h, "H7");
    click(&mut h, outside, PointerButton::Primary);
    open(&mut h, Menu::Filter);
    h.get_by_label("Apply").click();
    h.run_steps(2);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1) && !sh.is_row_hidden(2), "cancelled checkbox edits must not be applied after reopening");

    open(&mut h, Menu::Filter);
    h.get_by_label("(Select All)").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(2);
    assert!(!is_open(&h, Menu::Filter));
    assert!(h.state().session.active().unwrap().wb.active().unwrap().is_row_hidden(1));
    h.state_mut().run("data.clearFilter", json!({})).unwrap();
    h.run_steps(2);

    open(&mut h, Menu::Validation);
    h.get_by_label("Second").click();
    h.run_steps(2);
    assert!(!is_open(&h, Menu::Validation));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("E3").unwrap()), Value::from("Second"));
}

#[test]
fn worksheet_popups_close_on_escape_or_window_focus_loss() {
    for menu in [Menu::Header, Menu::Filter, Menu::Validation] {
        for focus_loss in [false, true] {
            let mut h = harness();
            open(&mut h, menu);
            if focus_loss {
                h.input_mut().focused = false;
                h.input_mut().events.push(Event::WindowFocused(false));
            } else {
                h.input_mut().events.push(Event::Key {
                    key: Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            h.run_steps(2);
            assert!(!is_open(&h, menu), "{menu:?} stayed open (focus_loss={focus_loss})");
        }
    }
}
