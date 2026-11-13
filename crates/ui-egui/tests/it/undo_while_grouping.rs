//! Undo and Redo on the Quick Access Toolbar follow the commands' own availability: while an undo
//! group is open on the document (an action that spans several calls is still running), both
//! buttons are off, and a click neither runs the command nor raises its refusal.

use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn history(h: &Harness) -> (usize, usize) {
    let d = h.state().session.active().unwrap();
    (d.undo.len(), d.redo.len())
}

#[test]
fn undo_and_redo_wait_while_an_undo_group_is_open() {
    let mut h = harness();
    assert_eq!(history(&h), (1, 1));
    let token = h.state_mut().session.group_begin("Long action").unwrap();
    h.run_steps(2);
    for button in ["Undo", "Redo"] {
        assert!(h.get_by_label(button).accesskit_node().is_disabled(), "{button} is off while the group is open");
        h.get_by_label(button).click();
        h.run_steps(2);
        assert!(h.state().message.is_none(), "{button}: no refusal is raised while the group is open");
        assert_eq!(history(&h), (1, 1), "{button}: the history is as it was");
    }
    h.state_mut().session.group_end(token).unwrap();
    h.run_steps(2);
    assert!(!h.get_by_label("Undo").accesskit_node().is_disabled() && !h.get_by_label("Redo").accesskit_node().is_disabled());
    // With the group ended, the buttons work again.
    h.get_by_label("Undo").click();
    h.run_steps(2);
    assert_eq!(history(&h), (0, 2));
    h.get_by_label("Redo").click();
    h.run_steps(2);
    assert_eq!(history(&h), (1, 1));
}
