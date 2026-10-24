//! Right-clicking a PivotTable's value cell offers Show Details in the cell menu (Excel's
//! right-click › Show Details); labels of the report and ordinary cells don't offer it.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 100], ["West", 80], ["East", 30]]})).unwrap();
    // Report at D3: D4 East, E4 its Sum of Sales, E6 the Grand Total.
    session
        .execute("insert.pivotTable", json!({"source": "Sheet1!A1:B4", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Sales"]}))
        .unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
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

#[test]
fn show_details_in_the_cell_menu_lists_a_value_cells_records_on_a_new_sheet() {
    let mut h = harness();
    right_click(&mut h, "E4");
    h.get_by_label("Show Details").click();
    h.run_steps(2);

    let app = h.state();
    assert!(app.grid.context_menu.is_none(), "the menu closes");
    let wb = &app.session.active().unwrap().wb;
    assert_eq!(wb.sheets.len(), 2);
    let sheet = wb.active().unwrap();
    let text = |a: &str| sheet.value(CellRef::parse(a).unwrap()).display();
    assert_eq!(
        [text("A1"), text("B1"), text("A2"), text("B2"), text("A3"), text("B3"), text("A4")],
        ["Region", "Sales", "East", "100", "East", "30", ""]
    );
}

#[test]
fn show_details_in_the_cell_menu_on_the_grand_total_takes_every_record() {
    let mut h = harness();
    right_click(&mut h, "E6");
    h.get_by_label("Show Details").click();
    h.run_steps(2);

    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.sheets.len(), 2);
    let sheet = wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::parse("B4").unwrap()).display(), "30");
}

#[test]
fn labels_of_the_report_and_ordinary_cells_have_no_show_details_item() {
    let mut h = harness();
    for cell in ["D4", "D3", "A2"] {
        right_click(&mut h, cell);
        assert!(h.query_by_label("Sort A to Z").is_some(), "the cell menu is up on {cell}");
        assert!(h.query_by_label("Show Details").is_none(), "{cell} offers no Show Details");
        let escape = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.input_mut().events.push(escape);
        h.run_steps(2);
    }
}

#[test]
fn the_cell_menu_computes_the_report_once_for_all_its_cells() {
    let mut h = harness();
    right_click(&mut h, "E4");
    assert!(h.query_by_label("Show Details").is_some());
    let escape = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.push(escape);
    h.run_steps(2);
    // West goes away behind the session's back, so the revision stays the same. A menu that
    // computed the report again for D4 would find it out of date and offer Show Details there.
    let doc = h.state_mut().session.active_mut().unwrap();
    let si = doc.wb.active_sheet;
    std::sync::Arc::make_mut(&mut doc.wb).sheet_mut(si).unwrap().set_value(CellRef::parse("A3").unwrap(), Value::text("East"));
    right_click(&mut h, "D4");
    assert!(h.query_by_label("Show Details").is_none(), "D4 is a label of the report computed for E4");
}

#[test]
fn show_details_in_an_out_of_date_report_asks_for_a_refresh() {
    let mut h = harness();
    // West goes away without a refresh: the report on the sheet is no longer what the source gives.
    h.state_mut().session.execute("range.setValues", json!({"range": "Sheet1!A3", "values": [["East"]]})).unwrap();
    h.run_steps(2);
    right_click(&mut h, "E4");
    h.get_by_label("Show Details").click();
    h.run_steps(2);

    let app = h.state();
    assert!(app.message.as_ref().is_some_and(|(_, m)| m.contains("out of date")), "{:?}", app.message);
    assert_eq!(app.session.active().unwrap().wb.sheets.len(), 1);
}
