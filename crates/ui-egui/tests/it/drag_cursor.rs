//! Drag cursors are asserted frame by frame through real egui pointer input (#936).

use egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef, Value};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Drag, Geo, freeze_handles};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["A", "B", "C", "D", "E"], [1, 2]]})).unwrap();
    let mut h = Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn geo(h: &Harness) -> Geo {
    h.state().grid.geo.clone().unwrap()
}

fn cell(h: &Harness, a1: &str) -> Rect {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    geo(h).screen_cell_rect(sh, CellRef::parse(a1).unwrap())
}

fn selection_rect(h: &Harness) -> Rect {
    let d = h.state().session.active().unwrap();
    geo(h).range_rect(d.wb.active().unwrap(), d.selection.current())
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn move_to(h: &mut Harness, pos: Pos2) {
    frame(h, Event::PointerMoved(pos));
}

fn button(h: &mut Harness, pos: Pos2, pressed: bool) {
    frame(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

fn press(h: &mut Harness, pos: Pos2) {
    move_to(h, pos);
    button(h, pos, true);
}

fn assert_cursor(h: &Harness, expected: CursorIcon) {
    assert_eq!(h.output().platform_output.cursor_icon, expected, "drag state: {:?}", h.state().grid.drag);
}

fn move_with_cursor(h: &mut Harness, pos: Pos2, expected: CursorIcon) {
    move_to(h, pos);
    assert_cursor(h, expected);
    // Check the following idle frame too: hit targets may have moved with the selection.
    h.run_steps(1);
    assert_cursor(h, expected);
}

#[test]
fn selecting_keeps_cell_cursor_over_borders_handles_frozen_panes_and_outside() {
    for rtl in [false, true] {
        let mut h = harness();
        h.state_mut().session.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
        h.state_mut().session.execute("view.freezePanes", json!({"cell": "B2"})).unwrap();
        h.run_steps(2);
        let start = cell(&h, "C3").center();
        press(&mut h, start);
        assert_eq!(h.state().grid.drag, Drag::None, "a press below the threshold is not a drag");
        move_with_cursor(&mut h, start + vec2(1.0, 1.0), CursorIcon::Cell);
        assert_eq!(h.state().grid.drag, Drag::None);
        let inside = cell(&h, "D4").center();
        move_with_cursor(&mut h, inside, CursorIcon::Cell);
        assert_eq!(h.state().grid.drag, Drag::Select);
        assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "C3:D4");

        let r = selection_rect(&h);
        let g = geo(&h);
        let border = g.mirror_pos(pos2(r.right() - 1.0, r.bottom() - 8.0));
        let handle = g.mirror_pos(r.right_bottom() - vec2(1.0, 1.0));
        move_with_cursor(&mut h, border, CursorIcon::Cell);
        move_with_cursor(&mut h, handle, CursorIcon::Cell);
        assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "C3:D4");
        // Cross into the frozen pane, then briefly leave the grid while still holding.
        let frozen = cell(&h, "A1").center();
        move_with_cursor(&mut h, frozen, CursorIcon::Cell);
        assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:C3");
        let outside = pos2(g.rect.left() - 20.0, g.rect.center().y);
        move_with_cursor(&mut h, outside, CursorIcon::Cell);
        // Leaving the mirrored grid can auto-scroll, so use its current geometry on return.
        let inside = cell(&h, "D4").center();
        move_with_cursor(&mut h, inside, CursorIcon::Cell);
        let handle = geo(&h).mirror_pos(selection_rect(&h).right_bottom() - vec2(1.0, 1.0));
        move_with_cursor(&mut h, handle, CursorIcon::Cell);
        button(&mut h, handle, false);
        assert_eq!(h.state().grid.drag, Drag::None);
        assert_cursor(&h, CursorIcon::Crosshair);
        let r = selection_rect(&h);
        let border = geo(&h).mirror_pos(pos2(r.right() - 1.0, r.bottom() - 8.0));
        move_with_cursor(&mut h, border, CursorIcon::Move);
        let inside = cell(&h, "D4").center();
        move_with_cursor(&mut h, inside, CursorIcon::Cell);
        let d = h.state().session.active().unwrap();
        assert_eq!(d.selection.current().a1(), "C3:D4");
        assert_eq!(d.wb.active().unwrap().value(CellRef::parse("B2").unwrap()), Value::Number(2.0));
    }
}

#[test]
fn a_new_press_does_not_treat_a_clicks_transient_state_as_a_drag() {
    let mut h = harness();
    let start = cell(&h, "C3").center();
    press(&mut h, start);
    button(&mut h, start, false);
    assert_eq!(h.state().grid.drag, Drag::Select, "a click briefly leaves a selection state");
    let handle = selection_rect(&h).right_bottom();
    // Another press can arrive before an idle frame clears that state.
    h.input_mut().events.push(Event::PointerMoved(handle));
    button(&mut h, handle, true);
    assert_cursor(&h, CursorIcon::Crosshair);
    button(&mut h, handle, false);
    assert_eq!(h.state().grid.drag, Drag::None);
}

#[test]
fn fill_keeps_crosshair_and_still_extends_a_series() {
    let mut h = harness();
    h.state_mut().session.execute("selection.set", json!({"range": "A2:B2"})).unwrap();
    h.run_steps(2);
    let start = selection_rect(&h).right_bottom();
    press(&mut h, start);
    let target = cell(&h, "E2").center();
    move_with_cursor(&mut h, target, CursorIcon::Crosshair);
    assert!(matches!(h.state().grid.drag, Drag::Fill { .. }));
    move_with_cursor(&mut h, target + vec2(1.0, 0.0), CursorIcon::Crosshair);
    button(&mut h, target, false);
    assert_eq!(h.state().grid.drag, Drag::None);
    assert_cursor(&h, CursorIcon::Cell);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!((0..5).map(|c| sh.value(CellRef::new(1, c))).collect::<Vec<_>>(), (1..=5).map(|n| Value::Number(n as f64)).collect::<Vec<_>>());
}

#[test]
fn moving_a_selection_keeps_move_and_still_moves_cells() {
    let mut h = harness();
    h.state_mut().session.execute("selection.set", json!({"range": "A2:B2"})).unwrap();
    h.run_steps(2);
    let r = selection_rect(&h);
    let start = pos2(r.left() + 10.0, r.top() + 1.0);
    press(&mut h, start);
    let target = cell(&h, "D5").center();
    move_with_cursor(&mut h, target, CursorIcon::Move);
    assert!(matches!(h.state().grid.drag, Drag::Move { .. }));
    move_with_cursor(&mut h, target + vec2(1.0, 0.0), CursorIcon::Move);
    button(&mut h, target, false);
    assert_eq!(h.state().grid.drag, Drag::None);
    h.run_steps(1);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(CellRef::parse("A2").unwrap()), Value::Empty);
    assert_eq!(sh.value(CellRef::parse("D5").unwrap()), Value::Number(1.0));
    assert_eq!(sh.value(CellRef::parse("E5").unwrap()), Value::Number(2.0));
}

#[test]
fn resizing_headers_keeps_the_axis_cursor_when_leaving_the_header() {
    for cols in [true, false] {
        let mut h = harness();
        let g = geo(&h);
        let r = cell(&h, "C3");
        let start = if cols { pos2(r.right(), g.head_top() + g.header_h * 0.5) } else { pos2(g.head_left() + g.header_w * 0.5, r.bottom()) };
        let cursor = if cols { CursorIcon::ResizeColumn } else { CursorIcon::ResizeRow };
        press(&mut h, start);
        let first = start + if cols { vec2(12.0, 0.0) } else { vec2(0.0, 12.0) };
        move_with_cursor(&mut h, first, cursor);
        assert!(if cols { matches!(h.state().grid.drag, Drag::ResizeCol { .. }) } else { matches!(h.state().grid.drag, Drag::ResizeRow { .. }) });
        let target = cell(&h, "E6").center();
        move_with_cursor(&mut h, target, cursor);
        button(&mut h, target, false);
        assert_eq!(h.state().grid.drag, Drag::None);
        assert_cursor(&h, CursorIcon::Cell);
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        assert!(if cols { sh.col_width(2) > sh.default_col_width } else { sh.row_height(2) > sh.default_row_height });
    }
}

#[test]
fn freezing_keeps_the_axis_cursor_away_from_the_handle() {
    for cols in [true, false] {
        let mut h = harness();
        let g = geo(&h);
        let handles = freeze_handles(&g);
        let start = if cols { handles.0.center() } else { handles.1.center() };
        let cursor = if cols { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical };
        press(&mut h, start);
        let r = cell(&h, "C4");
        let target = if cols { pos2(r.left() + 2.0, r.center().y) } else { pos2(r.center().x, r.top() + 2.0) };
        move_with_cursor(&mut h, target, cursor);
        assert!(matches!(h.state().grid.drag, Drag::Freeze { .. }));
        move_with_cursor(&mut h, target + vec2(1.0, 1.0), cursor);
        button(&mut h, target, false);
        assert_eq!(h.state().grid.drag, Drag::None);
        assert_cursor(&h, CursorIcon::Cell);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().freeze, Some(if cols { (0, 2) } else { (3, 0) }));
    }
}

#[test]
fn moving_columns_keeps_move_over_unselected_headers_and_cells() {
    let mut h = harness();
    h.state_mut().session.execute("selection.set", json!({"range": "B:C"})).unwrap();
    h.run_steps(2);
    let g = geo(&h);
    let start = pos2(cell(&h, "B1").center().x, g.head_top() + g.header_h * 0.5);
    press(&mut h, start);
    let target = pos2(cell(&h, "E1").left() + 10.0, start.y);
    move_with_cursor(&mut h, target, CursorIcon::Move);
    assert!(matches!(h.state().grid.drag, Drag::MoveCols { .. }));
    let inside = pos2(target.x, cell(&h, "E5").center().y);
    move_with_cursor(&mut h, inside, CursorIcon::Move);
    button(&mut h, target, false);
    assert_eq!(h.state().grid.drag, Drag::None);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!((0..5).map(|c| sh.value(CellRef::new(0, c))).collect::<Vec<_>>(), ["A", "D", "B", "C", "E"].map(|s| Value::Text(s.into())));
}

#[test]
fn selecting_headers_keeps_default_cursor_and_extends_whole_lines() {
    for cols in [true, false] {
        let mut h = harness();
        let g = geo(&h);
        let r = cell(&h, "C3");
        let start = if cols { pos2(r.center().x, g.head_top() + g.header_h * 0.5) } else { pos2(g.head_left() + g.header_w * 0.5, r.center().y) };
        press(&mut h, start);
        move_with_cursor(&mut h, start + if cols { vec2(12.0, 0.0) } else { vec2(0.0, 7.0) }, CursorIcon::Default);
        assert!(if cols { matches!(h.state().grid.drag, Drag::Cols(_)) } else { matches!(h.state().grid.drag, Drag::Rows(_)) });
        let target = cell(&h, "E6").center();
        move_with_cursor(&mut h, target, CursorIcon::Default);
        button(&mut h, target, false);
        assert_eq!(h.state().grid.drag, Drag::None);
        let expected = if cols { RangeRef::cols(2, 4) } else { RangeRef::rows(2, 5) };
        assert_eq!(h.state().session.active().unwrap().selection.current(), expected);
    }
}

#[test]
fn object_move_and_resize_keep_their_cursors_and_update_the_object() {
    for resize in [false, true] {
        let mut h = harness();
        let result = h.state_mut().session.execute("insert.chart", json!({"type": "column", "at": "D3", "width": 300, "height": 200})).unwrap();
        let id = result["chart"].as_u64().unwrap() as u32;
        h.state_mut().selected_chart = Some(id);
        h.run_steps(2);
        let g = geo(&h);
        let min = cell(&h, "D3").min;
        let start = if resize { min + vec2(300.0, 200.0) * g.z } else { min + vec2(40.0, 40.0) };
        press(&mut h, start);
        let target = start + vec2(60.0, 30.0);
        let cursor = if resize { CursorIcon::ResizeNwSe } else { CursorIcon::Move };
        move_with_cursor(&mut h, target, cursor);
        assert!(matches!(h.state().grid.drag, Drag::Object { grab, .. } if matches!(grab, gridcraft_ui_egui::frame::Grab::Resize(_)) == resize));
        move_with_cursor(&mut h, target + vec2(1.0, 1.0), cursor);
        button(&mut h, target, false);
        assert_eq!(h.state().grid.drag, Drag::None);
        let a = h.state().session.active().unwrap().wb.active().unwrap().charts.iter().find(|c| c.id == id).unwrap().anchor;
        if resize {
            assert!(a.width > 340.0 && a.height > 220.0);
        } else {
            assert_ne!((a.cell, a.dx, a.dy), (CellRef::parse("D3").unwrap(), 0.0, 0.0));
            assert_eq!((a.width, a.height), (300.0, 200.0));
        }
    }
}

#[test]
fn draw_tools_keep_crosshair_and_pen_still_creates_ink() {
    for tool in ["draw.pen", "draw.eraser"] {
        let mut h = harness();
        h.state_mut().session.execute(tool, json!({"on": true})).unwrap();
        let start = cell(&h, "C3").center();
        press(&mut h, start);
        assert_cursor(&h, CursorIcon::Crosshair);
        for a1 in ["D4", "E5", "F6"] {
            let p = cell(&h, a1).center();
            move_with_cursor(&mut h, p, CursorIcon::Crosshair);
            assert_eq!(h.state().grid.hover_cell, Some(CellRef::parse(a1).unwrap()), "drawing still tracks the hovered cell for notes");
        }
        let end = cell(&h, "F6").center();
        button(&mut h, end, false);
        assert_cursor(&h, CursorIcon::Crosshair);
        if tool == "draw.pen" {
            assert!(
                h.state().session.active().unwrap().wb.active().unwrap().shapes.iter().any(|s| s.kind == gridcraft_engine::model::ShapeKind::Ink)
            );
        }
    }
}
