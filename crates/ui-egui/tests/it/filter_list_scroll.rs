//! The AutoFilter value list scrolls with the pointer anywhere over it, not only over the values
//! themselves (issue #432): the list takes the menu's full width, so its scroll bar and wheel area do too.

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, TouchPhase, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A1:A81 holds a header and 80 distinct values, with the AutoFilter's menu open on column A.
fn filter_menu_on_a_long_list() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut values = vec![vec![json!("Code")]];
    values.extend((1..=80).map(|i| vec![json!(format!("ITEM-{i:03}"))]));
    session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    session.execute("data.filter", json!({"range": "A1:A81"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    let button = {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
        grid::filter_button_rect(&geo, sh, CellRef::parse("A1").unwrap()).center()
    };
    h.input_mut().events.push(Event::PointerMoved(button));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: button, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
    assert!(h.state().grid.filter_menu.is_some(), "the filter menu opens");
    h
}

#[test]
/// A wheel event over the menu's blank right area scrolls the value list.
fn filter_list_scrolls_with_the_pointer_right_of_the_values() {
    let mut h = filter_menu_on_a_long_list();
    let first = h.get_by_label("ITEM-001").rect();
    // The menu is 230 points wide; the value labels end well before this point.
    let pointer = pos2(first.left() + 200.0, first.center().y);
    h.input_mut().events.push(Event::PointerMoved(pointer));
    h.run_steps(2);
    h.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        source: egui::MouseWheelSource::Unknown,
        delta: vec2(0.0, -3.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(20);
    let now = h.query_by_label("ITEM-001").map(|n| n.rect().top());
    assert!(now.is_none_or(|y| y < first.top() - 20.0), "the wheel scrolls the list: ITEM-001 was at {}, now {now:?}", first.top());
}
