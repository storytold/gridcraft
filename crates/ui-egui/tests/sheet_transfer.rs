use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness(dark: bool) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    session.execute("cell.set", json!({"cell":"A1","input":"42"})).unwrap();
    session.new_workbook();
    session.execute("sheet.rename", json!({"name":"Destination"})).unwrap();
    session.set_active(0);
    let mut app = SheetApp::new(session, Default::default());
    app.ui.dark = dark;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().open_dialog("moveSheet", json!({}));
    h.run_steps(3);
    h
}

#[test]
fn workbook_picker_copies_to_the_selected_workbook_in_both_themes() {
    for dark in [false, true] {
        let mut h = harness(dark);
        h.get_by_value("Book1").click();
        h.run_steps(2);
        h.get_by_label("Book2").click();
        h.run_steps(2);
        h.get_by_label("(move to end)").click();
        h.run_steps(2);
        h.get_by_label("Create a copy").click();
        h.run_steps(2);
        if let Ok(dir) = std::env::var("GRIDCRAFT_SHEET_TRANSFER_SCREENSHOTS") {
            let dir = std::path::Path::new(&dir);
            std::fs::create_dir_all(dir).unwrap();
            h.render().unwrap().save(dir.join(format!("move-sheet-{dark}.png"))).unwrap();
        }
        h.get_by_role_and_label(egui::accesskit::Role::Button, "OK").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert!(h.state().message.is_none());
        let docs = h.state().session.documents();
        assert_eq!(docs[0].wb.sheets.len(), 1);
        assert_eq!(docs[1].wb.sheet(0).unwrap().name, "Destination");
        assert_eq!(docs[1].wb.sheet(1).unwrap().name, "Source");
        assert_eq!(docs[1].wb.sheet(1).unwrap().value(CellRef::new(0, 0)), Value::Number(42.0));
    }
}

#[test]
fn cancel_does_not_change_either_workbook() {
    let mut h = harness(false);
    let before: Vec<_> = h.state().session.documents().iter().map(|d| d.wb.clone()).collect();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    for (d, wb) in h.state().session.documents().iter().zip(before) {
        assert_eq!(*d.wb, *wb);
    }
}
