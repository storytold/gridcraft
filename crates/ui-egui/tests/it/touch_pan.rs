//! Touch input on the grid: one finger pans, a tap selects (issue #1907).

use egui::{Event, Modifiers, PointerButton, TouchDeviceId, TouchId, TouchPhase};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::harness(session, 4)
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// One finger, sent the way egui-winit does: a touch event plus the emulated pointer events.
fn touch(h: &mut egui_kittest::Harness<'static, SheetApp>, phase: TouchPhase, pos: egui::Pos2) {
    let ev = &mut h.input_mut().events;
    ev.push(Event::Touch { device_id: TouchDeviceId(1), id: TouchId(0), phase, pos, force: None });
    match phase {
        TouchPhase::Start => {
            ev.push(Event::PointerMoved(pos));
            ev.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        }
        TouchPhase::Move => ev.push(Event::PointerMoved(pos)),
        TouchPhase::End | TouchPhase::Cancel => {
            ev.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            ev.push(Event::PointerGone);
        }
    }
}

fn swipe(h: &mut egui_kittest::Harness<'static, SheetApp>, from: egui::Pos2, by: egui::Vec2) {
    touch(h, TouchPhase::Start, from);
    h.run_steps(2);
    for i in 1..=10 {
        touch(h, TouchPhase::Move, from + by * (i as f32 / 10.0));
        h.run_steps(1);
    }
    touch(h, TouchPhase::End, from + by);
    h.run_steps(3);
}

fn assert_selection_untouched(h: &egui_kittest::Harness<'static, SheetApp>) {
    let sel = &h.state().session.active().unwrap().selection;
    assert_eq!(sel.active, CellRef::new(0, 0), "a touch pan must not move the active cell: {sel:?}");
    assert_eq!(sel.ranges.len(), 1, "{sel:?}");
    assert_eq!(sel.ranges[0].start, CellRef::new(0, 0), "{sel:?}");
    assert_eq!(sel.ranges[0].end, CellRef::new(0, 0), "a touch pan must not select cells: {sel:?}");
}

#[test]
fn one_finger_drag_pans_the_grid_instead_of_selecting() {
    let mut h = harness(blank());
    let cells = h.state().grid.cells_rect.unwrap();
    swipe(&mut h, cells.center() + egui::vec2(0.0, 100.0), egui::vec2(0.0, -200.0));
    let scroll = h.state().view().scroll;
    assert!(scroll.y > 0.0, "dragging a finger up scrolls down: scroll = {scroll:?}");
    assert_selection_untouched(&h);

    swipe(&mut h, cells.center() + egui::vec2(100.0, 0.0), egui::vec2(-200.0, 0.0));
    let scroll = h.state().view().scroll;
    assert!(scroll.x > 0.0, "dragging a finger left scrolls right: scroll = {scroll:?}");
    assert_selection_untouched(&h);
}

#[test]
fn one_finger_tap_still_selects_the_cell() {
    let mut h = harness(blank());
    let cells = h.state().grid.cells_rect.unwrap();
    let p = cells.min + egui::vec2(5.0, 5.0) + egui::vec2(0.0, cells.height() / 2.0);
    touch(&mut h, TouchPhase::Start, p);
    h.run_steps(1);
    touch(&mut h, TouchPhase::End, p);
    h.run_steps(3);
    let sel = &h.state().session.active().unwrap().selection;
    assert_eq!(sel.active.col, 0, "{sel:?}");
    assert!(sel.active.row > 0, "a tap selects the tapped cell: {sel:?}");
    assert_eq!(h.state().view().scroll, egui::Vec2::ZERO);
}

/// The sheet follows the finger at any Cell grid size: at 200% a swipe covers half the sheet
/// distance it does at 100%.
#[test]
fn a_touch_pan_follows_the_finger_at_any_grid_size() {
    let pan = |percent: u32| {
        let mut h = harness(blank());
        h.state_mut().run("view.gridScale", serde_json::json!({"percent": percent})).unwrap();
        h.run_steps(2);
        let cells = h.state().grid.cells_rect.unwrap();
        swipe(&mut h, cells.center() + egui::vec2(0.0, 100.0), egui::vec2(0.0, -200.0));
        let y = h.state().view().scroll.y;
        gridcraft_ui_egui::grid::set_grid_scale(1.0);
        y
    };
    let (normal, double) = (pan(100), pan(200));
    assert!(normal > 0.0);
    assert!((double * 2.0 - normal).abs() < 1.0, "100%: {normal}, 200%: {double}");
}
