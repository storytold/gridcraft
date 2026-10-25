//! Double-clicking a PivotTable's value cell shows its source records on a new sheet (Excel's
//! Show Details) unless its Enable show details option is off; a label of the report still opens
//! the cell for editing.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 100], ["West", 80], ["East", 30]]})).unwrap();
    // Report at D3: D4 East, E4 its Sum of Sales.
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

fn double_click(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2) {
    for _ in 0..2 {
        h.input_mut().events.extend([
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ]);
        h.run_steps(2);
    }
}

#[test]
fn double_click_on_a_value_cell_shows_its_records_on_a_new_sheet() {
    let mut h = harness();
    let pos = center(&h, "E4");
    double_click(&mut h, pos);

    let app = h.state();
    assert!(app.editor.is_none(), "the value cell must not open for editing");
    let wb = &app.session.active().unwrap().wb;
    assert_eq!(wb.sheets.len(), 2);
    let sheet = wb.active().unwrap();
    assert_eq!(sheet.name, "Sheet2");
    let text = |a: &str| sheet.value(CellRef::parse(a).unwrap()).display();
    assert_eq!(
        [text("A1"), text("B1"), text("A2"), text("B2"), text("A3"), text("B3"), text("A4")],
        ["Region", "Sales", "East", "100", "East", "30", ""]
    );
}

#[test]
fn double_click_on_a_row_label_still_edits() {
    let mut h = harness();
    let pos = center(&h, "D4");
    double_click(&mut h, pos);

    let app = h.state();
    assert!(app.editor.is_some());
    assert_eq!(app.session.active().unwrap().wb.sheets.len(), 1);
}

#[test]
fn double_click_on_a_value_cell_of_a_protected_workbook_says_why_and_does_not_edit() {
    let mut h = harness();
    h.state_mut().session.execute("review.protectWorkbook", json!({"on": true})).unwrap();
    h.run_steps(2);
    let pos = center(&h, "E4");
    double_click(&mut h, pos);

    let app = h.state();
    assert!(app.editor.is_none(), "the value cell must not open for editing");
    assert!(app.message.as_ref().is_some_and(|(_, m)| m.contains("protected")), "{:?}", app.message);
    assert_eq!(app.session.active().unwrap().wb.sheets.len(), 1);
}

#[test]
fn double_click_in_an_out_of_date_report_asks_for_a_refresh() {
    let mut h = harness();
    // West goes away without a refresh: E6, the Grand Total on the sheet, has no cell in the
    // report the source now gives.
    h.state_mut().session.execute("range.setValues", json!({"range": "Sheet1!A3", "values": [["East"]]})).unwrap();
    h.run_steps(2);
    let pos = center(&h, "E6");
    double_click(&mut h, pos);

    let app = h.state();
    assert!(app.editor.is_none(), "the cell must not open for editing");
    assert!(app.message.as_ref().is_some_and(|(_, m)| m.contains("out of date")), "{:?}", app.message);
    assert_eq!(app.session.active().unwrap().wb.sheets.len(), 1);
}

#[test]
fn double_click_with_enable_show_details_off_says_why_and_does_not_edit() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.options", json!({"pivot": "PivotTable1", "enableShowDetails": false})).unwrap();
    h.run_steps(2);
    let pos = center(&h, "E4");
    double_click(&mut h, pos);

    let app = h.state();
    assert!(app.editor.is_none(), "the value cell must not open for editing");
    assert!(app.message.as_ref().is_some_and(|(_, m)| m == "Show Details is turned off for this PivotTable."), "{:?}", app.message);
    assert_eq!(app.session.active().unwrap().wb.sheets.len(), 1);
}
