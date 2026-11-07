//! As in Excel, a lone `+` or `-` typed into a cell starts a formula: clicking a cell or pressing
//! an arrow key next inserts a reference instead of leaving the cell (#2023).

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn typed(text: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"range": "F8"})).unwrap();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some(text.into()), false);
    h.run_steps(2);
    h
}

fn cell_center(h: &egui_kittest::Harness<'_, SheetApp>, row: u32, col: u32) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    pos2(geo.x(sheet, col) + sheet.col_width(col) * geo.z * 0.5, geo.y(sheet, row) + sheet.row_height(row) * geo.z * 0.5)
}

fn editor_text(h: &egui_kittest::Harness<'_, SheetApp>) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

#[test]
fn clicking_a_cell_after_a_lone_sign_inserts_its_reference() {
    for sign in ["+", "-"] {
        let mut h = typed(sign);
        let pos = cell_center(&h, 2, 2);
        for pressed in [true, false] {
            h.input_mut().events.extend([
                Event::PointerMoved(pos),
                Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE },
            ]);
            h.step();
        }
        h.run_steps(2);
        assert_eq!(editor_text(&h), Some(format!("{sign}C3")), "{sign}");
        assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "F8", "{sign}: the edit stays in F8");
    }
}

#[test]
fn an_arrow_key_after_a_lone_plus_inserts_a_reference() {
    let mut h = typed("+");
    h.input_mut().events.push(Event::Key { key: Key::ArrowUp, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: Key::ArrowUp, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(editor_text(&h).as_deref(), Some("+F7"));
}
