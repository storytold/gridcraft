//! Right-clicking a PivotTable report offers Refresh in the cell menu; choosing it recomputes the
//! report from its source. Ordinary cells and cells outside the report do not offer it.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 100], ["West", 80]]})).unwrap();
    // Report at D3: D4 East, E4 its Sum of Sales, E6 the Grand Total.
    session
        .execute("insert.pivotTable", json!({"source": "Sheet1!A1:B3", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Sales"]}))
        .unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    crate::support::stepped_harness(session)
}

fn center(h: &egui_kittest::Harness<'_, SheetApp>, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn right_click(h: &mut egui_kittest::Harness<'_, SheetApp>, cell: &str) {
    let pos = center(h, cell);
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    assert!(h.state().grid.context_menu.is_some(), "the cell menu opens on {cell}");
}

fn escape(h: &mut egui_kittest::Harness<'_, SheetApp>) {
    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// The cell-menu item `label`; the PivotTable pane's Refresh button is outside the grid's rect.
fn menu_item<'a>(h: &'a egui_kittest::Harness<'_, SheetApp>, label: &'a str) -> Option<egui_kittest::Node<'a>> {
    let grid = h.state().grid.rect.expect("the grid is drawn");
    h.query_all_by_label(label).find(|n| grid.contains_rect(n.rect()))
}

fn text(h: &egui_kittest::Harness<'_, SheetApp>, cell: &str) -> String {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    sheet.value(CellRef::parse(cell).unwrap()).display()
}

#[test]
fn refresh_is_offered_inside_a_pivot_report() {
    let mut h = harness();
    for cell in ["E4", "D4", "D3"] {
        right_click(&mut h, cell);
        assert!(menu_item(&h, "Refresh").is_some(), "{cell} offers Refresh");
        escape(&mut h);
    }
    h.check_accessibility();

    // Refresh is independent of Show Details: value cells still refresh when that option is off.
    h.state_mut().session.execute("pivot.options", json!({"enableShowDetails": false, "pivot": "PivotTable1"})).unwrap();
    h.run_steps(2);
    right_click(&mut h, "E4");
    assert!(menu_item(&h, "Refresh").is_some(), "E4 still offers Refresh");
    assert!(menu_item(&h, "Show Details").is_none(), "Show Details is turned off");
}

#[test]
fn refresh_updates_the_report_from_its_source() {
    let mut h = harness();
    assert_eq!(text(&h, "E4"), "100");
    assert_eq!(text(&h, "E6"), "180");

    h.state_mut().session.execute("range.setValues", json!({"range": "B2", "values": [[150]]})).unwrap();
    h.run_steps(2);
    assert_eq!(text(&h, "E4"), "100", "the report is stale until refreshed");
    assert_eq!(text(&h, "E6"), "180");

    right_click(&mut h, "E4");
    menu_item(&h, "Refresh").unwrap().click();
    h.run_steps(2);

    assert!(h.state().grid.context_menu.is_none(), "the menu closes");
    assert_eq!(text(&h, "E4"), "150");
    assert_eq!(text(&h, "E6"), "230");
}

#[test]
fn ordinary_cells_have_no_refresh_item() {
    let mut h = harness();
    for cell in ["A2", "F4"] {
        right_click(&mut h, cell);
        assert!(h.query_by_label("Sort A to Z").is_some(), "the cell menu is up on {cell}");
        assert!(menu_item(&h, "Refresh").is_none(), "{cell} offers no Refresh");
        escape(&mut h);
    }
}

#[test]
fn refresh_can_be_activated_from_the_keyboard() {
    let mut h = harness();
    h.state_mut().session.execute("range.setValues", json!({"range": "B2", "values": [[150]]})).unwrap();
    h.run_steps(2);
    assert_eq!(text(&h, "E4"), "100");

    right_click(&mut h, "E4");
    menu_item(&h, "Refresh").unwrap().focus();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);

    assert_eq!(text(&h, "E4"), "150");
}
