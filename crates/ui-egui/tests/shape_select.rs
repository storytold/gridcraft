//! Selecting, moving and resizing an inserted shape through real pointer input.

use egui::{Event, Modifiers, PointerButton, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Anchor;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn fixture() -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    let shape = session.execute("insert.shape", json!({"kind": "rectangle", "at": "D3", "width": 160, "height": 100})).unwrap();
    let id = shape["shape"].as_u64().unwrap() as u32;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().selected_chart = None;
    h.run_steps(2);
    (h, id)
}

fn anchor(h: &Harness, id: u32) -> Anchor {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.iter().find(|s| s.id == id).unwrap().anchor
}

fn rect(h: &Harness, id: u32) -> egui::Rect {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let a = anchor(h, id);
    let min = geo.cell_rect(sh, a.cell).min + vec2(a.dx, a.dy) * geo.z;
    egui::Rect::from_min_size(min, vec2(a.width, a.height) * geo.z)
}

fn button(h: &mut Harness, pos: egui::Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn drag(h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(2);
    button(h, from, true);
    h.input_mut().events.push(Event::PointerMoved(to));
    h.run_steps(3);
    button(h, to, false);
}

#[test]
fn clicking_a_shape_selects_it() {
    let (mut h, id) = fixture();
    let c = rect(&h, id).center();
    h.input_mut().events.push(Event::PointerMoved(c));
    h.run_steps(2);
    button(&mut h, c, true);
    button(&mut h, c, false);
    assert_eq!(h.state().selected_chart, Some(id));
    assert_eq!(h.state().session.active().unwrap().selection.current().start, CellRef::parse("A1").unwrap());
}

fn handle_at(h: &Harness, at: egui::Pos2) -> bool {
    h.output().shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Circle(k) if (k.center - at).length() < 1.0 && k.radius == 4.0))
}

#[test]
fn a_selected_shape_shows_its_handles() {
    let (mut h, id) = fixture();
    let r = rect(&h, id);
    assert!(!handle_at(&h, r.center_top()), "no handles before selecting");
    h.input_mut().events.push(Event::PointerMoved(r.center()));
    h.run_steps(2);
    button(&mut h, r.center(), true);
    button(&mut h, r.center(), false);
    for at in [r.left_top(), r.center_top(), r.right_bottom()] {
        assert!(handle_at(&h, at), "handle at {at:?}");
    }
}

#[test]
fn dragging_a_shape_moves_it() {
    let (mut h, id) = fixture();
    let before = rect(&h, id);
    drag(&mut h, before.center(), before.center() + vec2(120.0, 80.0));
    let after = rect(&h, id);
    assert!((after.min - before.min - vec2(120.0, 80.0)).length() < 4.0, "moved from {before:?} to {after:?}");
    assert_eq!(after.size(), before.size());
}

fn select(h: &mut Harness, id: u32) {
    let c = rect(h, id).center();
    h.input_mut().events.push(Event::PointerMoved(c));
    h.run_steps(2);
    button(h, c, true);
    button(h, c, false);
}

#[test]
fn every_handle_shows_its_resize_cursor() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let r = rect(&h, id);
    for (at, cursor) in [
        (r.left_top(), egui::CursorIcon::ResizeNwSe),
        (r.right_top(), egui::CursorIcon::ResizeNeSw),
        (r.left_center(), egui::CursorIcon::ResizeHorizontal),
        (r.center_bottom(), egui::CursorIcon::ResizeVertical),
    ] {
        h.input_mut().events.push(Event::PointerMoved(at));
        h.run_steps(2);
        assert_eq!(h.output().platform_output.cursor_icon, cursor, "at {at:?}");
    }
}

#[test]
fn dragging_the_left_handle_moves_only_the_left_edge() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let before = rect(&h, id);
    drag(&mut h, before.left_center(), before.left_center() + vec2(-50.0, 40.0));
    let after = rect(&h, id);
    assert!((after.left() - (before.left() - 50.0)).abs() < 2.0, "{before:?} -> {after:?}");
    assert!((after.right() - before.right()).abs() < 2.0, "{before:?} -> {after:?}");
    assert!((after.top() - before.top()).abs() < 2.0 && (after.bottom() - before.bottom()).abs() < 2.0, "{before:?} -> {after:?}");
}

#[test]
fn dragging_the_top_left_handle_keeps_the_bottom_right_corner() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let before = rect(&h, id);
    drag(&mut h, before.left_top(), before.left_top() + vec2(30.0, 20.0));
    let after = rect(&h, id);
    assert!((after.right_bottom() - before.right_bottom()).length() < 2.0, "{before:?} -> {after:?}");
    assert!((after.left_top() - before.left_top() - vec2(30.0, 20.0)).length() < 2.0, "{before:?} -> {after:?}");
}

#[test]
fn dragging_the_corner_resizes_a_shape() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let before = rect(&h, id);
    drag(&mut h, before.right_bottom(), before.right_bottom() + vec2(60.0, 30.0));
    let after = rect(&h, id);
    assert!(after.width() > before.width() + 40.0 && after.height() > before.height() + 20.0, "{before:?} -> {after:?}");
}

fn rotation(h: &Harness, id: u32) -> f32 {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.iter().find(|s| s.id == id).unwrap().rotation
}

#[test]
fn dragging_the_rotation_handle_turns_the_shape() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let r = rect(&h, id);
    let knob = gridcraft_ui_egui::frame::rotate_handle(r, 0.0);
    assert!(handle_at(&h, knob), "the rotation handle shows");
    h.input_mut().events.push(Event::PointerMoved(knob));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Crosshair);
    drag(&mut h, knob, r.center() + vec2(200.0, 0.0));
    assert_eq!(rotation(&h, id), 90.0);
    assert_eq!(rect(&h, id), r, "turning keeps the box");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(rotation(&h, id), 0.0, "one drag is one undo step");
}

#[test]
fn a_turned_shape_is_hit_inside_its_turned_outline() {
    let (mut h, id) = fixture();
    h.state_mut().run("object.move", json!({"kind": "shape", "id": id, "rotation": 90})).unwrap();
    h.run_steps(2);
    let r = rect(&h, id);
    // Turned 90°, the shape reaches half its width above and below its centre.
    let below = r.center() + vec2(0.0, r.width() / 2.0 - 5.0);
    assert!(!r.contains(below));
    h.input_mut().events.push(Event::PointerMoved(below));
    h.run_steps(2);
    button(&mut h, below, true);
    button(&mut h, below, false);
    assert_eq!(h.state().selected_chart, Some(id));
}

fn key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.run_steps(2);
}

#[test]
fn arrow_keys_nudge_the_selected_shape_not_the_cell_cursor() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    let before = anchor(&h, id);
    key(&mut h, egui::Key::ArrowRight, Modifiers::NONE);
    key(&mut h, egui::Key::ArrowDown, Modifiers::CTRL);
    let after = anchor(&h, id);
    let x = |a: &Anchor| h.state().session.doc().unwrap().wb.active().unwrap().col_left(a.cell.col) as f32 + a.dx;
    let y = |a: &Anchor| h.state().session.doc().unwrap().wb.active().unwrap().row_top(a.cell.row) as f32 + a.dy;
    assert!((x(&after) - x(&before) - 6.0).abs() < 0.01 && (y(&after) - y(&before) - 1.0).abs() < 0.01, "{before:?} -> {after:?}");
    assert_eq!(h.state().session.active().unwrap().selection.current().start, CellRef::parse("A1").unwrap());
}

#[test]
fn escape_deselects_the_shape() {
    let (mut h, id) = fixture();
    select(&mut h, id);
    key(&mut h, egui::Key::Escape, Modifiers::NONE);
    assert_eq!(h.state().selected_chart, None);
}

#[test]
fn the_body_of_a_shape_shows_the_move_cursor() {
    let (mut h, id) = fixture();
    let c = rect(&h, id).center();
    h.input_mut().events.push(Event::PointerMoved(c));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Move);
}
