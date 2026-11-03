//! A right-click on a cell opens its context menu, and the menu stays open until the user picks
//! an item, presses Escape or clicks elsewhere.

use egui::{Event, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    harness_at(1.0)
}

/// `ppp` physical pixels per point: 2 on a Retina display.
fn harness_at(ppp: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, CellRef::parse(cell).unwrap()).center()
}

fn button(h: &mut Harness, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
}

/// Press and release arrive in the same frame (a quick click).
fn quick_right_click(h: &mut Harness, cell: &str) {
    let pos = center(h, cell);
    h.input_mut().events.push(Event::PointerMoved(pos));
    button(h, pos, true);
    button(h, pos, false);
    h.run_steps(4);
}

/// Press and release in separate frames (a slower click).
fn slow_right_click(h: &mut Harness, cell: &str) {
    let pos = center(h, cell);
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    button(h, pos, true);
    h.run_steps(1);
    button(h, pos, false);
    h.run_steps(4);
}

#[test]
fn a_quick_right_click_keeps_the_menu_open() {
    let mut h = harness();
    quick_right_click(&mut h, "C5");
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}

#[test]
fn a_slow_right_click_keeps_the_menu_open() {
    let mut h = harness();
    slow_right_click(&mut h, "C5");
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}

#[test]
fn right_clicking_another_cell_moves_the_menu_there() {
    for quick in [true, false] {
        let mut h = harness();
        slow_right_click(&mut h, "C5");
        if quick {
            quick_right_click(&mut h, "M2");
        } else {
            slow_right_click(&mut h, "M2");
        }
        let at = h.state().grid.context_menu;
        assert!(at.is_some(), "the menu stays open (quick: {quick})");
        assert_eq!(at, Some(center(&h, "M2")), "at the second cell (quick: {quick})");
    }
}

/// A real pointer sits between pixels (x = 252.46), and the menu's corner snaps to the nearest
/// physical pixel (252.5 on a Retina display), just past the pointer. The press that comes in the
/// same frame as the release then counted as a press outside the menu and closed it at once.
#[test]
fn a_quick_right_click_between_pixels_keeps_the_menu_open_on_retina() {
    let mut h = harness_at(2.0);
    let pos = center(&h, "C5") + egui::vec2(0.46, 0.37);
    h.input_mut().events.push(Event::PointerMoved(pos));
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.run_steps(4);
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}
