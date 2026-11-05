//! Insert › Symbol while editing a cell puts the symbol at the caret of the edit (issue #3424).

use egui::{Key, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
fn the_gallery_inserts_at_the_caret_of_the_edit() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Insert"})).unwrap();
    h.run_steps(4);
    h.state_mut().begin_edit(Some("HelloWorld".into()), false);
    // Edit mode (F2): the arrow keys move the caret instead of leaving the cell.
    if let Some(ed) = h.state_mut().editor.as_mut() {
        ed.enter_mode = false;
    }
    h.run_steps(4);
    for _ in 0..5 {
        h.key_press(Key::ArrowLeft);
        h.run_steps(2);
    }
    assert_eq!(h.state().editor.as_ref().map(|e| e.caret), Some(5), "the caret is in the middle of the text");
    h.state_mut().ui.keytip_invoke_next = Some("insert.symbol");
    h.run_steps(4);
    h.get_by_label("€").click();
    h.run_steps(4);
    let ed = h.state().editor.as_ref().expect("the cell is still being edited");
    assert_eq!(ed.text, "Hello€World");
    assert_eq!(ed.caret, 6, "the caret ends after the symbol");
}
