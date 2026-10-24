//! Double-clicking the fill handle fills down to the neighbouring column's last used row,
//! even with the pointer up for a frame between the two clicks.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4], [5], [6]]})).unwrap();
    session.execute("range.setValues", json!({"range": "B1", "values": [[10], [20], [30]]})).unwrap();
    session.execute("selection.set", json!({"range": "B1:B3"})).unwrap();
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

fn handle(h: &egui_kittest::Harness<'_, SheetApp>) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::new(2, 1)).right_bottom() - vec2(1.0, 1.0)
}

fn button(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

fn number(h: &egui_kittest::Harness<'_, SheetApp>, row: u32) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(row, 1))
}

#[test]
fn double_click_on_the_fill_handle_fills_down_with_a_frame_between_the_clicks() {
    let mut h = harness();
    let pos = handle(&h);
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.step(); // One idle frame between the clicks, well inside the double-click interval.
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.run_steps(2);

    assert_eq!([number(&h, 3), number(&h, 4), number(&h, 5)], [Value::Number(40.0), Value::Number(50.0), Value::Number(60.0)]);
    assert_eq!(h.state().session.active().unwrap().selection.a1(), "B1:B6");
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "edit.autoFill").count(), 1);
}
