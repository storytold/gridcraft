//! Typing into a tall cell puts the text where the cell's vertical alignment will draw it, as in
//! Excel, so it doesn't jump from the top to the middle or bottom on Enter.

use egui::accesskit::Role;
use egui::{Event, Rect};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(align: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.rowHeight", json!({"rows": "3:3", "height": 90})).unwrap();
    session.execute(align, json!({"range": "B3"})).unwrap();
    session.execute("selection.set", json!({"cell": "B3"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn cell_rect(h: &Harness) -> Rect {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, CellRef::parse("B3").unwrap())
}

/// The in-cell text field after typing into B3.
fn typed_field(h: &mut Harness) -> Rect {
    h.input_mut().events.push(Event::Text("abc".into()));
    h.run_steps(3);
    assert!(h.state().editor.is_some(), "editing");
    let cell = cell_rect(h);
    h.get_all_by_role(Role::TextInput).map(|n| n.rect()).find(|r| cell.intersects(*r)).expect("the in-cell editor")
}

#[test]
fn typing_follows_the_cells_vertical_alignment() {
    let mut top = harness("home.alignTop");
    let mut middle = harness("home.alignMiddle");
    let mut bottom = harness("home.alignBottom");
    let cell = cell_rect(&top);
    let (t, m, b) = (typed_field(&mut top), typed_field(&mut middle), typed_field(&mut bottom));
    assert!(t.top() < cell.top() + 6.0, "top: {t:?} in {cell:?}");
    assert!((m.center().y - cell.center().y).abs() < 6.0, "middle: {m:?} in {cell:?}");
    assert!(b.bottom() > cell.bottom() - 8.0 && b.top() > m.top(), "bottom: {b:?} in {cell:?}");
}
