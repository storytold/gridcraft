//! The painted grid exposes the active cell to assistive tech: address, value and formula (gap 18).

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
fn active_cell_label_reads_value_formula_state_and_address_once() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "=A1*3"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    let cell = h.get_by_role_and_label(Role::Cell, "6 has formula B2");
    let node = cell.accesskit_node();
    assert_eq!(node.value(), None);
    assert_eq!(node.data().description(), None);

    h.state_mut().run("selection.set", json!({"range": "A1"})).unwrap();
    h.run_steps(2);
    let updated = h.get_by_role_and_label(Role::Cell, "2 A1").accesskit_node();
    assert_eq!(updated.value(), None);
}

#[test]
fn array_formula_label_announces_formula_state_without_reading_the_formula() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "4"})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
    s.execute("cell.set", json!({"input": "=A1:A2*2", "array": true})).unwrap();
    s.execute("selection.set", json!({"range": "B1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    let node = h.get_by_role_and_label(Role::Cell, "4 has formula B1");
    assert_eq!(node.accesskit_node().data().description(), None);
}
