//! Menu items highlight under the pointer, as in Excel (#1174): no background at rest, the hover
//! background while the pointer is on an item.

use egui::{Event, Modifiers, PointerButton, Pos2, Shape, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn cell_center(h: &Harness, cell: &str) -> egui::Rect {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sh, CellRef::parse(cell).unwrap())
}

fn right_click(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

/// Is a filled rectangle in the hovered-widget colour painted under `at`?
fn hover_fill_under(h: &Harness, at: Pos2) -> bool {
    let fill = h.ctx.global_style().visuals.widgets.hovered.weak_bg_fill;
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r) if r.fill == fill && r.rect.contains(at)))
}

fn check_item(h: &mut Harness, label: &str) {
    let at = h.get_by_role_and_label(Role::Button, label).rect().center();
    assert!(!hover_fill_under(h, at), "{label} has no background before the pointer is on it");
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    assert!(hover_fill_under(h, at), "{label} is highlighted under the pointer");
}

#[test]
fn cell_menu_items_highlight_under_the_pointer() {
    let mut h = harness();
    let pos = cell_center(&h, "C5").center();
    right_click(&mut h, pos);
    assert!(h.state().grid.context_menu.is_some());
    check_item(&mut h, "Copy");
}

#[test]
fn header_menu_items_highlight_under_the_pointer() {
    let mut h = harness();
    let c1 = cell_center(&h, "C1");
    right_click(&mut h, egui::pos2(c1.center().x, c1.top() - 6.0));
    assert!(h.state().grid.header_menu.is_some());
    check_item(&mut h, "Copy");
}
