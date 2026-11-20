//! Whole-row reordering through real egui pointer events.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::grid::{Drag, Geo};
use serde_json::json;

fn harness(selection: &str) -> crate::common::Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1", "values":[["A"],["B"],["C"],["D"],["E"]]})).unwrap();
    s.execute("selection.set", json!({"range":selection})).unwrap();
    let mut h =
        crate::common::harness_with(crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)), crate::common::app(s), 4);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().session.journal.clear();
    h
}

fn header(h: &crate::common::Harness, row: u32, fraction: f32) -> Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    geo.mirror_pos(pos2(geo.rect.left() + geo.header_w * 0.5, geo.y(sh, row) + sh.row_height(row) * geo.z * fraction))
}

fn button(h: &mut crate::common::Harness, p: Pos2, pressed: bool, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.extend([Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers }]);
    h.step();
}

fn move_to(h: &mut crate::common::Harness, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
}

fn start_drag(h: &mut crate::common::Harness, start: Pos2, target: Pos2) {
    button(h, start, true, Modifiers::NONE);
    move_to(h, target);
    move_to(h, target + vec2(0.0, 1.0));
}

fn values(h: &crate::common::Harness) -> Vec<gridcraft_engine::core::Value> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    (0..5).map(|row| sh.value(CellRef::new(row, 0))).collect()
}

#[test]
fn selected_row_headers_move_down_with_marker_and_one_undo_step() {
    let mut h = harness("2:3");
    let start = header(&h, 1, 0.5);
    let target = header(&h, 4, 0.25);
    start_drag(&mut h, start, target);
    assert!(matches!(h.state().grid.drag, Drag::MoveRows { before: Some(4), .. }));
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Move);
    h.step(); // Paint the marker after interaction updated its destination.
    assert!(h.output().shapes.iter().any(|s| matches!(&s.shape,egui::Shape::LineSegment{points,stroke} if stroke.width == 3.0 && (points[0].y-points[1].y).abs()<0.01 && (points[1].x-points[0].x).abs()>500.0)));
    button(&mut h, target, false, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(values(&h), vec!["A".into(), "D".into(), "B".into(), "C".into(), "E".into()]);
    let app = h.state();
    assert_eq!(app.session.active().unwrap().selection.current().a1(), "3:4");
    assert_eq!(app.session.active().unwrap().undo.len(), 1);
    assert_eq!(app.session.journal.iter().filter(|(id, _)| id == "sheet.moveRows").count(), 1);
}

#[test]
fn row_drop_uses_release_position() {
    let mut h = harness("4:5");
    let start = header(&h, 3, 0.5);
    let target = header(&h, 2, 0.25);
    start_drag(&mut h, start, target);
    let release = header(&h, 1, 0.25);
    button(&mut h, release, false, Modifiers::NONE);
    assert_eq!(values(&h), vec!["A".into(), "D".into(), "E".into(), "B".into(), "C".into()]);
}

#[test]
fn escape_cancels_row_move() {
    let mut h = harness("2:3");
    let start = header(&h, 1, 0.5);
    let target = header(&h, 4, 0.25);
    start_drag(&mut h, start, target);
    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    button(&mut h, target, false, Modifiers::NONE);
    assert_eq!(values(&h), vec!["A".into(), "B".into(), "C".into(), "D".into(), "E".into()]);
    assert_eq!(h.state().session.active().unwrap().undo.len(), 0);
}

#[test]
fn dragging_unselected_headers_selects_rows() {
    let mut h = harness("A1");
    let start = header(&h, 1, 0.5);
    let target = header(&h, 3, 0.5);
    start_drag(&mut h, start, target);
    assert!(matches!(h.state().grid.drag, Drag::Rows(_)));
    button(&mut h, target, false, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "2:4");
    assert_eq!(h.state().session.active().unwrap().undo.len(), 0);
}

#[test]
fn row_edge_keeps_resizing_selected_rows() {
    let mut h = harness("2:3");
    let start = header(&h, 1, 1.0);
    start_drag(&mut h, start, start + vec2(0.0, 12.0));
    assert!(matches!(h.state().grid.drag, Drag::ResizeRow { .. }));
}

#[test]
fn dropping_inside_selected_rows_does_nothing() {
    let mut h = harness("2:3");
    let start = header(&h, 1, 0.5);
    let target = header(&h, 2, 0.5);
    start_drag(&mut h, start, target);
    button(&mut h, target, false, Modifiers::NONE);
    assert_eq!(values(&h), vec!["A".into(), "B".into(), "C".into(), "D".into(), "E".into()]);
    assert_eq!(h.state().session.active().unwrap().undo.len(), 0);
}

#[test]
fn shift_drag_extends_row_selection_instead_of_moving_it() {
    let mut h = harness("2:3");
    let start = header(&h, 1, 0.5);
    let target = header(&h, 4, 0.5);
    button(&mut h, start, true, Modifiers::SHIFT);
    move_to(&mut h, target);
    move_to(&mut h, target + vec2(0.0, 1.0));
    assert!(matches!(h.state().grid.drag, Drag::Rows(_)));
    button(&mut h, target, false, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "2:5");
    assert_eq!(h.state().session.active().unwrap().undo.len(), 0);
}

#[test]
fn row_drag_works_with_zoom_rtl_and_frozen_rows() {
    let mut h = harness("2:3");
    h.state_mut().session.execute("view.zoom", json!({"percent":150})).unwrap();
    h.state_mut().session.execute("view.freezePanes", json!({"cell":"B2"})).unwrap();
    let d = h.state_mut().session.active_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    wb.sheet_mut(wb.active_sheet()).unwrap().right_to_left = true;
    h.run_steps(4);
    let start = header(&h, 1, 0.5);
    let target = header(&h, 4, 0.25);
    start_drag(&mut h, start, target);
    button(&mut h, target, false, Modifiers::NONE);
    assert_eq!(values(&h), vec!["A".into(), "D".into(), "B".into(), "C".into(), "E".into()]);
}
