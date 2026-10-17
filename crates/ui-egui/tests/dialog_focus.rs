//! Form dialogs open ready to type, as in Excel: Alt+H, O, W then a number and Enter sets the
//! column width without clicking into the field first.

use egui::{Event, Key, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn type_and_enter(h: &mut egui_kittest::Harness<'static, SheetApp>, text: &str) {
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(Key::Enter);
    h.run_steps(6);
}

#[test]
fn column_width_opens_with_the_width_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    type_and_enter(&mut h, "12");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let chars = {
        let d = h.state().session.active().unwrap();
        let sheet = d.wb.active().unwrap();
        gridcraft_engine::cmd::format::points_to_chars(sheet.col_width(CellRef::parse("C3").unwrap().col) as f64)
    };
    assert!((chars - 12.0).abs() < 0.2, "column C is 12 characters wide, got {chars}");
}

#[test]
fn row_height_opens_with_the_height_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("rowHeight", json!({}));
    h.run_steps(6);
    type_and_enter(&mut h, "40");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let height = {
        let d = h.state().session.active().unwrap();
        d.wb.active().unwrap().row_height(CellRef::parse("C3").unwrap().row)
    };
    assert!((height as f64 - 40.0).abs() < 0.6, "row 3 is 40 high, got {height}");
}
