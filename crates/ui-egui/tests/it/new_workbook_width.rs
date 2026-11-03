//! File > Options sets the column width for future blank workbooks.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

#[test]
fn options_width_field_applies_to_new_workbooks_only() {
    let mut session = Session::new();
    let original = session.new_workbook();

    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Services::default())),
    );

    h.run_steps(4);
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);

    let label = "Default column width for new workbooks (pixels)";

    assert_eq!(h.get_by_role_and_label(Role::TextInput, label).value().as_deref(), Some("64"),);

    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);

    for c in "112.5".chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(2);
    }

    h.key_press(egui::Key::Enter);
    h.run_steps(3);

    assert_eq!(h.state().session.prefs.default_column_width, 112.5,);

    assert_eq!(h.state().session.documents()[original].wb.sheets[0].default_col_width, 64.0,);

    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(3);

    assert_eq!(h.state().session.doc().unwrap().wb.active().unwrap().default_col_width, 112.5,);
}
