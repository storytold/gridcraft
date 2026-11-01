//! Drawing a Line or Arrow from the Shapes gallery by dragging on the sheet (#2904): the shape
//! runs from the press point to the release point in any direction, Shift snaps a line to 15°,
//! and the direction survives an XLSX round trip.

use egui::{Event, Modifiers, PointerButton, Pos2, Vec2, epaint::Shape as Painted, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Shape, ShapeKind};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(session), 4)
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll)
}

fn cell_center(h: &Harness, a1: &str) -> Pos2 {
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    geo(h).cell_rect(sh, CellRef::parse(a1).unwrap()).center()
}

/// A screen point in sheet points (from the sheet's top-left corner).
fn sheet_point(h: &Harness, p: Pos2) -> [f32; 2] {
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let g = geo(h);
    let c = g.cell_at(sh, p);
    [sh.col_left(c.col) as f32 + (p.x - g.x(sh, c.col)) / g.z, sh.row_top(c.row) as f32 + (p.y - g.y(sh, c.row)) / g.z]
}

/// A line's start and end in sheet points: its anchor box plus the flips.
fn ends(h: &Harness, s: &Shape) -> ([f32; 2], [f32; 2]) {
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    let (x0, y0) = (sh.col_left(s.anchor.cell.col) as f32 + s.anchor.dx, sh.row_top(s.anchor.cell.row) as f32 + s.anchor.dy);
    let (a, b) = s.line_ends();
    ([x0 + a[0], y0 + a[1]], [x0 + b[0], y0 + b[1]])
}

fn last_shape(h: &Harness) -> Shape {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.last().cloned().expect("a shape was drawn")
}

fn pick_shape(h: &mut Harness, label: &str) {
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(4);
    h.get_by_label("Shapes").click();
    h.run_steps(4);
    h.get_by_label(label).click();
    h.run_steps(4);
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers });
    h.run_steps(2);
    for i in 1..=4 {
        let t = i as f32 / 4.0;
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * t));
        h.run_steps(1);
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers });
    h.run_steps(3);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(1);
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5
}

#[test]
fn a_dragged_line_or_arrow_runs_from_the_press_to_the_release_point() {
    // #2904: every drag gave the same top-left to bottom-right line, whatever its direction.
    let directions: [(&str, Vec2); 6] = [
        ("right", vec2(120.0, 0.0)),
        ("down", vec2(0.0, 90.0)),
        ("up-left", vec2(-110.0, -70.0)),
        ("down-left", vec2(-80.0, 60.0)),
        ("up-right", vec2(90.0, -50.0)),
        ("down-right", vec2(100.0, 40.0)),
    ];
    for kind in ["Line", "Arrow"] {
        for (name, delta) in directions {
            let mut h = harness();
            pick_shape(&mut h, kind);
            let from = cell_center(&h, "H14");
            let to = from + delta;
            drag(&mut h, from, to, Modifiers::NONE);
            let s = last_shape(&h);
            assert_eq!(s.kind, if kind == "Line" { ShapeKind::Line } else { ShapeKind::Arrow }, "{kind} {name}");
            let (start, end) = ends(&h, &s);
            let (want_start, want_end) = (sheet_point(&h, from), sheet_point(&h, to));
            assert!(close(start, want_start), "{kind} {name}: starts at {start:?}, pressed at {want_start:?}");
            // For an Arrow the end is where its head is drawn and saved (`a:tailEnd`).
            assert!(close(end, want_end), "{kind} {name}: ends at {end:?}, released at {want_end:?}");
            if delta.x == 0.0 {
                assert_eq!(s.anchor.width, 0.0, "{kind} {name}: exactly vertical");
            }
            if delta.y == 0.0 {
                assert_eq!(s.anchor.height, 0.0, "{kind} {name}: exactly horizontal");
            }
            assert!(h.state().grid.shape_tool.is_none(), "drawing one shape disarms the pointer");
            assert_eq!(h.state().selected_chart, Some(s.id), "the new shape is selected");
            // The painted line follows the drag too, and an Arrow's head is at the release point.
            h.run_steps(1);
            assert!(painted_segment(&h, from, to), "{kind} {name}: a segment is painted from the press to the release point");
            if kind == "Arrow" {
                assert!(painted_head(&h, to), "{kind} {name}: the arrowhead is painted at the release point");
            }
        }
    }
}

fn near(a: Pos2, b: Pos2) -> bool {
    (a - b).length() < 1.0
}

fn painted_segment(h: &Harness, from: Pos2, to: Pos2) -> bool {
    h.output().shapes.iter().any(|c| matches!(&c.shape, Painted::LineSegment { points: [a, b], .. } if near(*a, from) && near(*b, to)))
}

fn painted_head(h: &Harness, tip: Pos2) -> bool {
    h.output().shapes.iter().any(|c| matches!(&c.shape, Painted::Path(p) if p.closed && p.points.len() == 3 && near(p.points[0], tip)))
}

#[test]
fn shift_snaps_a_dragged_line_to_15_degrees() {
    for (delta, degrees) in [(vec2(100.0, 8.0), 0.0f32), (vec2(-6.0, 90.0), 90.0), (vec2(80.0, -75.0), -45.0), (vec2(-100.0, -30.0), -165.0)] {
        let mut h = harness();
        pick_shape(&mut h, "Line");
        let from = cell_center(&h, "H14");
        drag(&mut h, from, from + delta, Modifiers::SHIFT);
        let s = last_shape(&h);
        let (start, end) = ends(&h, &s);
        assert!(close(start, sheet_point(&h, from)), "the snapped line still starts at the press point");
        let angle = (end[1] - start[1]).atan2(end[0] - start[0]).to_degrees();
        assert!((angle - degrees).abs() < 0.5, "{delta:?} snapped to {angle}°, expected {degrees}°");
        if degrees == 0.0 {
            assert_eq!(s.anchor.height, 0.0);
        }
        if degrees == 90.0 {
            assert_eq!(s.anchor.width, 0.0);
        }
    }
}

#[test]
fn clicking_without_a_drag_inserts_the_default_line_at_the_click() {
    let mut h = harness();
    pick_shape(&mut h, "Line");
    let p = cell_center(&h, "D6");
    drag(&mut h, p, p, Modifiers::NONE);
    let s = last_shape(&h);
    assert_eq!(s.anchor.cell, CellRef::parse("D6").unwrap());
    assert_eq!((s.anchor.width, s.anchor.height, s.flip_h, s.flip_v), (144.0, 0.0, false, false));
}

#[test]
fn escape_disarms_the_shape_pointer() {
    let mut h = harness();
    pick_shape(&mut h, "Arrow");
    assert_eq!(h.state().grid.shape_tool.as_deref(), Some("arrow"));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().grid.shape_tool.is_none());
    let from = cell_center(&h, "H14");
    drag(&mut h, from, from + vec2(80.0, 40.0), Modifiers::NONE);
    assert!(h.state().session.doc().unwrap().wb.active().unwrap().shapes.is_empty(), "a plain drag after Esc selects cells");
}

#[test]
fn a_drawn_lines_direction_survives_saving_and_reopening() {
    let mut h = harness();
    pick_shape(&mut h, "Arrow");
    let from = cell_center(&h, "H14");
    drag(&mut h, from, from + vec2(-90.0, -60.0), Modifiers::NONE);
    let before = last_shape(&h);
    assert!(before.flip_h && before.flip_v);
    let wb = &h.state().session.doc().unwrap().wb;
    let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
    let back = gridcraft_xlsx::read_xlsx(&bytes).unwrap().0;
    let after = back.sheet(0).unwrap().shapes.last().unwrap();
    assert_eq!(after.kind, ShapeKind::Arrow);
    assert_eq!((after.flip_h, after.flip_v), (true, true));
    assert_eq!(after.anchor.cell, before.anchor.cell);
    assert!((after.anchor.width - before.anchor.width).abs() < 1.0 && (after.anchor.height - before.anchor.height).abs() < 1.0);
}
