//! The All Sheets navigator next to the sheet tabs: type part of a name, Enter, and that sheet is active (#839).

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut s = Session::new();
    s.new_workbook();
    for _ in 0..3 {
        s.run("home.insertSheet", json!({})).unwrap();
    }
    for (i, name) in ["Data", "Hidden", "Notes", "Summary"].iter().enumerate() {
        s.run("sheet.rename", json!({"sheet": i, "name": name})).unwrap();
    }
    s.run("sheet.hide", json!({"sheet": 1})).unwrap();
    s.run("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

#[test]
fn typing_a_name_and_enter_activates_that_sheet() {
    let mut h = harness();
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet, 0);
    h.get_by_label("All sheets").click();
    h.run_steps(4);
    h.input_mut().events.push(Event::Text("summ".into()));
    h.run_steps(3);
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet, 3, "Enter on the filtered row must activate Summary");
}
