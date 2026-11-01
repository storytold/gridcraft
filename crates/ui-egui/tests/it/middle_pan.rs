//! The middle mouse button pans the grid, AutoCAD style: the sheet follows the pointer from the
//! press on, nothing selects, the cursor is a grabbing hand and the pan stops dead on release.

use egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::Drag;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    crate::support::stepped_harness(session)
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

/// Events that arrive in the same frame, in window order.
fn frame_events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(1);
}

fn move_to(h: &mut Harness, pos: Pos2) {
    frame(h, Event::PointerMoved(pos));
}

fn button(h: &mut Harness, pos: Pos2, button: PointerButton, pressed: bool) {
    frame(h, Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
}

/// A middle-button drag the way the window sends it: press, interpolated moves, release.
fn middle_drag(h: &mut Harness, from: Pos2, by: egui::Vec2) {
    move_to(h, from);
    button(h, from, PointerButton::Middle, true);
    for i in 1..=5 {
        move_to(h, from + by * (i as f32 / 5.0));
    }
    button(h, from + by, PointerButton::Middle, false);
}

/// The zoom factor screen points per sheet point, as the grid is drawn.
fn zoom(h: &Harness) -> f32 {
    h.state().grid.geo.as_ref().map_or(1.0, |geo| geo.z)
}

fn assert_a1_still_selected(h: &Harness) {
    let sel = &h.state().session.active().unwrap().selection;
    assert_eq!(sel.active, CellRef::new(0, 0), "panning must not move the active cell: {sel:?}");
    assert_eq!(sel.ranges.len(), 1, "{sel:?}");
    assert_eq!(sel.ranges[0].start, CellRef::new(0, 0), "{sel:?}");
    assert_eq!(sel.ranges[0].end, CellRef::new(0, 0), "panning must not select cells: {sel:?}");
}

#[test]
fn middle_drag_pans_the_grid_instead_of_selecting() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let z = zoom(&h);
    let from = cells.center() + vec2(0.0, 100.0);
    middle_drag(&mut h, from, vec2(0.0, -200.0));
    let scroll = h.state().view().scroll;
    assert!((scroll.y - 200.0 / z).abs() < 1.0, "the sheet follows the pointer exactly: scroll = {scroll:?}, z = {z}");
    assert_eq!(scroll.x, 0.0, "{scroll:?}");
    assert_a1_still_selected(&h);
    assert!(!h.state().grid.middle_pan, "the pan ends on release");

    let from = cells.center() + vec2(100.0, 0.0);
    middle_drag(&mut h, from, vec2(-200.0, 0.0));
    let scroll = h.state().view().scroll;
    assert!((scroll.x - 200.0 / z).abs() < 1.0, "dragging left scrolls right: scroll = {scroll:?}");
    assert_a1_still_selected(&h);
    assert!(!h.state().grid.middle_pan);
}

#[test]
fn a_middle_click_without_moving_leaves_the_view_alone() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let p = cells.center();
    move_to(&mut h, p);
    button(&mut h, p, PointerButton::Middle, true);
    button(&mut h, p, PointerButton::Middle, false);
    assert_eq!(h.state().view().scroll, egui::Vec2::ZERO);
    assert_a1_still_selected(&h);
    assert!(!h.state().grid.middle_pan);
}

#[test]
fn the_cursor_is_a_grabbing_hand_while_the_pan_lasts() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let from = cells.center();
    move_to(&mut h, from);
    button(&mut h, from, PointerButton::Middle, true);
    move_to(&mut h, from - vec2(0.0, 40.0));
    assert!(h.state().grid.middle_pan);
    assert_eq!(h.output().platform_output.cursor_icon, CursorIcon::Grabbing, "middle-button pan shows a grabbing hand");
    button(&mut h, from - vec2(0.0, 40.0), PointerButton::Middle, false);
    assert!(!h.state().grid.middle_pan);
}

/// A middle-button drag over a chart pans the sheet and leaves the chart where it is: the pan
/// swallows the drag before any object or cell drag can start.
#[test]
fn a_middle_drag_over_a_chart_does_not_move_it() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    session.execute("cell.set", json!({"cell": "A2", "input": "2"})).unwrap();
    session.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "D3", "width": 300, "height": 200})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    let mut h = crate::support::stepped_harness(session);
    let geo = h.state().grid.geo.clone().unwrap();
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let anchor = sh.charts.iter().find(|c| c.id == id).unwrap().anchor;
    // The chart's top-left as it is drawn (see `chartview`), a little inside its frame.
    let start = geo.cell_rect(sh, anchor.cell).min + vec2(anchor.dx, anchor.dy) * geo.z + vec2(60.0, 60.0);
    middle_drag(&mut h, start, vec2(0.0, -150.0));
    assert!(h.state().view().scroll.y > 0.0, "the sheet pans under the chart");
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let moved = sh.charts.iter().find(|c| c.id == id).unwrap().anchor;
    assert_eq!(
        (moved.dx, moved.dy, moved.width, moved.height),
        (anchor.dx, anchor.dy, anchor.width, anchor.height),
        "a middle drag must not move a chart"
    );
    assert_ne!(h.state().selected_chart, Some(id), "a middle drag must not select the chart");
}

/// The release is a window-wide pointer event, not a grid hit test: letting go over a header or
/// outside the window ends the pan just the same.
#[test]
fn a_middle_release_outside_the_grid_ends_the_pan() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let from = cells.center();
    move_to(&mut h, from);
    button(&mut h, from, PointerButton::Middle, true);
    move_to(&mut h, from - vec2(0.0, 60.0));
    assert!(h.state().grid.middle_pan);

    let outside = cells.left_top() - vec2(20.0, 20.0);
    button(&mut h, outside, PointerButton::Middle, false);
    assert!(!h.state().grid.middle_pan, "a release outside the grid ends the pan");
    let scroll = h.state().view().scroll;
    move_to(&mut h, outside + vec2(0.0, -100.0));
    assert_eq!(h.state().view().scroll, scroll, "the sheet must not keep following the pointer after the release");
}

/// If the button is released while the window is unfocused, the release event never reaches egui,
/// which clears its keys but not its pointer buttons. Losing focus must end the pan anyway, and
/// focus coming back with the button still latched must not resume it.
#[test]
fn losing_window_focus_ends_the_pan_even_without_a_release() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let from = cells.center();
    move_to(&mut h, from);
    button(&mut h, from, PointerButton::Middle, true);
    move_to(&mut h, from - vec2(0.0, 60.0));
    assert!(h.state().grid.middle_pan);

    h.input_mut().focused = false;
    h.input_mut().events.push(Event::WindowFocused(false));
    h.run_steps(1);
    assert!(!h.state().grid.middle_pan, "losing focus ends the pan");
    assert_ne!(h.output().platform_output.cursor_icon, CursorIcon::Grabbing, "the grabbing hand goes with the pan");

    h.input_mut().focused = true;
    h.input_mut().events.push(Event::WindowFocused(true));
    h.run_steps(1);
    let scroll = h.state().view().scroll;
    move_to(&mut h, from - vec2(0.0, 120.0));
    assert_eq!(h.state().view().scroll, scroll, "a latched middle button must not resume panning after focus returns");
    assert!(!h.state().grid.middle_pan);
}

/// Panning scrolls the scrolling pane only: frozen rows and columns stay where they are.
#[test]
fn a_middle_drag_keeps_frozen_rows_and_columns_in_place() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.freezePanes", json!({"cell": "C3"})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    let geo = h.state().grid.geo.clone().unwrap();
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.freeze, Some((2, 2)), "two frozen columns and rows");
    let frozen = geo.cell_rect(sh, CellRef::new(1, 1));
    let scrolling = geo.cell_rect(sh, CellRef::new(10, 10));
    let from = geo.cell_rect(sh, CellRef::new(10, 10)).center();

    middle_drag(&mut h, from, vec2(-160.0, -160.0));

    let geo = h.state().grid.geo.clone().unwrap();
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    assert!(h.state().view().scroll.x > 0.0 && h.state().view().scroll.y > 0.0, "the sheet pans");
    assert_eq!(geo.cell_rect(sh, CellRef::new(1, 1)), frozen, "frozen cells stay put");
    let moved = geo.cell_rect(sh, CellRef::new(10, 10));
    assert!(moved.left() < scrolling.left() && moved.top() < scrolling.top(), "cells below and right of the freeze move: {moved:?} vs {scrolling:?}");
}

/// On a right-to-left sheet the sheet is mirrored, so dragging left scrolls the other way.
#[test]
fn a_middle_drag_pans_a_right_to_left_sheet_mirrored() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    let cells = h.state().grid.cells_rect.unwrap();
    let z = zoom(&h);
    // The sheet is mirrored, so the drag direction is flipped: dragging right moves the content
    // right (towards the sheet's first columns) and increases `scroll.x`.
    middle_drag(&mut h, cells.center(), vec2(200.0, 0.0));
    let scroll = h.state().view().scroll;
    assert!((scroll.x - 200.0 / z).abs() < 1.0, "dragging right pans a right-to-left sheet: scroll = {scroll:?}, z = {z}");
}

/// The 1:1 follow is in screen points, so it still holds at 200% zoom.
#[test]
fn a_middle_drag_follows_the_pointer_at_two_hundred_percent_zoom() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.zoom", json!({"percent": 200})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    let cells = h.state().grid.cells_rect.unwrap();
    let z = zoom(&h);
    assert!(z > 1.5, "200% zoom: z = {z}");
    middle_drag(&mut h, cells.center(), vec2(0.0, -200.0));
    let scroll = h.state().view().scroll;
    assert!((scroll.y - 200.0 / z).abs() < 1.0, "the sheet follows the pointer at 200%: scroll = {scroll:?}, z = {z}");
}

/// Objects other than charts take the same guard: a middle drag over a text box pans the sheet
/// and leaves the text box untouched.
#[test]
fn a_middle_drag_over_a_text_box_does_not_move_it() {
    let mut session = Session::new();
    session.new_workbook();
    let id = session.execute("insert.textBox", json!({"at": "D3", "width": 240, "height": 120, "text": "note"})).unwrap()["shape"].as_u64().unwrap()
        as u32;
    let mut h = crate::support::stepped_harness(session);
    let geo = h.state().grid.geo.clone().unwrap();
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let anchor = sh.shapes.iter().find(|s| s.id == id).unwrap().anchor;
    let start = geo.cell_rect(sh, anchor.cell).min + vec2(anchor.dx, anchor.dy) * geo.z + vec2(60.0, 60.0);

    middle_drag(&mut h, start, vec2(0.0, -150.0));

    assert!(h.state().view().scroll.y > 0.0, "the sheet pans under the text box");
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let moved = sh.shapes.iter().find(|s| s.id == id).unwrap().anchor;
    assert_eq!(
        (moved.dx, moved.dy, moved.width, moved.height),
        (anchor.dx, anchor.dy, anchor.width, anchor.height),
        "a middle drag must not move a text box"
    );
}

/// An open cell editor keeps the middle click (X11 pastes PRIMARY with it): the grid does not pan,
/// and the paste reaches the editor's text field.
#[test]
fn a_middle_click_in_an_open_cell_editor_pastes_and_does_not_pan() {
    let mut h = harness();
    h.state_mut().begin_edit(Some("AB".into()), false);
    h.run_steps(2);
    let cells = h.state().grid.cells_rect.unwrap();
    let p = cells.left_top() + vec2(10.0, 10.0);

    move_to(&mut h, p);
    button(&mut h, p, PointerButton::Middle, true);
    assert!(!h.state().grid.middle_pan, "an open editor keeps the middle click");
    assert_eq!(h.state().view().scroll, egui::Vec2::ZERO, "no pan while editing");
    button(&mut h, p, PointerButton::Middle, false);

    h.input_mut().events.push(Event::MiddleClickPaste { pos: p, text: "XY".into() });
    h.run_steps(1);
    let text = h.state().editor.as_ref().map(|ed| ed.text.clone()).unwrap_or_default();
    assert!(text.contains("XY"), "the middle click pastes into the editor: {text:?}");
}

/// egui's pointer delta covers the whole frame. When the pointer moves and presses in one frame,
/// the movement is before the gesture and must not jump the sheet on the press.
#[test]
fn a_move_in_the_press_frame_does_not_jump_the_sheet() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let from = cells.center();
    move_to(&mut h, from);
    let moved = from - vec2(0.0, 200.0);
    frame_events(
        &mut h,
        [Event::PointerMoved(moved), Event::PointerButton { pos: moved, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE }],
    );
    assert!(h.state().grid.middle_pan);
    assert_eq!(h.state().view().scroll, egui::Vec2::ZERO, "the move before the press must not count");
}

/// And when the pointer moves and releases in one frame, that last movement still counts.
#[test]
fn a_move_in_the_release_frame_still_moves_the_sheet() {
    let mut h = harness();
    let cells = h.state().grid.cells_rect.unwrap();
    let z = zoom(&h);
    let from = cells.center();
    move_to(&mut h, from);
    button(&mut h, from, PointerButton::Middle, true);
    let end = from - vec2(0.0, 200.0);
    frame_events(
        &mut h,
        [Event::PointerMoved(end), Event::PointerButton { pos: end, button: PointerButton::Middle, pressed: false, modifiers: Modifiers::NONE }],
    );
    assert!(!h.state().grid.middle_pan, "the pan ends on the release");
    let scroll = h.state().view().scroll;
    assert!((scroll.y - 200.0 / z).abs() < 1.0, "the move sharing the release frame counts: scroll = {scroll:?}, z = {z}");
}

/// A fill drag commits its destination on release. A middle press starts a pan, which skips the
/// grid's interactions; the pending drag must be dropped rather than committed later to a stale
/// target.
#[test]
fn a_middle_press_drops_a_pending_fill_drag_instead_of_committing_it_later() {
    let mut h = harness();
    h.state_mut().session.execute("selection.set", json!({"range": "A2:B2"})).unwrap();
    h.run_steps(2);
    let (start, target, mid) = {
        let geo = h.state().grid.geo.clone().unwrap();
        let d = h.state().session.active().unwrap();
        let sh = d.wb.active().unwrap();
        (
            geo.range_rect(sh, d.selection.current()).right_bottom(),
            geo.cell_rect(sh, CellRef::new(1, 4)).center(),
            geo.cell_rect(sh, CellRef::new(6, 6)).center(),
        )
    };

    button(&mut h, start, PointerButton::Primary, true);
    move_to(&mut h, target);
    assert!(matches!(h.state().grid.drag, Drag::Fill { .. }), "the fill handle drag is armed");
    button(&mut h, mid, PointerButton::Middle, true);
    assert_eq!(h.state().grid.drag, Drag::None, "starting a pan drops the pending fill");
    assert!(h.state().grid.middle_pan);
    button(&mut h, mid, PointerButton::Middle, false);
    button(&mut h, target, PointerButton::Primary, false);
    h.run_steps(2);

    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    for col in 2..=4 {
        assert_eq!(sh.value(CellRef::new(1, col)), Value::Empty, "a stale autoFill target must not be committed (column {col})");
    }
}
