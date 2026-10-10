//! Whole-column reordering driven through real egui pointer events.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "C", "D", "E"]]})).unwrap();
    session.execute("selection.set", json!({"range": "B:C"})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    harness.run_steps(4);
    harness.state_mut().session.active_mut().unwrap().undo.clear();
    harness.state_mut().session.journal.clear();
    harness
}

fn header_center(harness: &egui_kittest::Harness<'_, SheetApp>, col: u32) -> Pos2 {
    let app = harness.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    pos2(geo.x(sheet, col) + sheet.col_width(col) * geo.z * 0.5, geo.rect.top() + geo.header_h * 0.5)
}

fn pointer_button(harness: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    harness
        .input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    harness.step();
}

#[test]
fn dragging_selected_column_headers_reorders_the_block() {
    let mut harness = harness();
    let start = header_center(&harness, 1);
    let target = {
        let center = header_center(&harness, 4);
        pos2(center.x - 20.0, center.y)
    };

    pointer_button(&mut harness, start, true);
    harness.input_mut().events.push(Event::PointerMoved(target));
    harness.step();
    harness.input_mut().events.push(Event::PointerMoved(target + vec2(1.0, 0.0)));
    harness.step();
    pointer_button(&mut harness, target + vec2(1.0, 0.0), false);
    harness.run_steps(2);

    let app = harness.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let values: Vec<_> = (0..5).map(|col| sheet.value(CellRef::new(0, col))).collect();
    assert_eq!(values, vec!["A".into(), "D".into(), "B".into(), "C".into(), "E".into()]);
    assert_eq!(app.session.active().unwrap().selection.current().a1(), "C:D");
    assert_eq!(app.session.journal.iter().filter(|(id, _)| id == "sheet.moveColumns").count(), 1);
    assert_eq!(app.session.active().unwrap().undo.len(), 1);
}
