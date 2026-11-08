//! Lasso Select: dragging a loop selects the ink inside it, and dragging that ink moves it.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::model::ShapeKind;
use gridcraft_ui_egui::grid::Geo;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    // One stroke inside the loop the test draws and one well outside it.
    session.execute("draw.stroke", json!({"points": [[130, 130], [150, 140], [170, 130]], "color": "#C00000", "width": 3})).unwrap();
    session.execute("draw.stroke", json!({"points": [[400, 400], [450, 420], [500, 400]], "color": "#008040", "width": 3})).unwrap();
    let mut h =
        crate::common::harness_with(crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)), crate::common::app(session), 4);
    h.state_mut().session.execute("draw.lasso", json!({"on": true})).unwrap();
    h.run_steps(1);
    h
}

fn screen(h: &Harness, x: f32, y: f32) -> Pos2 {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let (col, row) = (sh.col_at(f64::from(x).max(0.0)), sh.row_at(f64::from(y).max(0.0)));
    geo.mirror_pos(egui::pos2(geo.x(sh, col) + (x - sh.col_left(col) as f32) * geo.z, geo.y(sh, row) + (y - sh.row_top(row) as f32) * geo.z))
}

fn press(h: &mut Harness, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn drag(h: &mut Harness, path: &[Pos2]) {
    let Some(&first) = path.first() else { return };
    h.input_mut().events.push(Event::PointerMoved(first));
    h.run_steps(3);
    press(h, first, true);
    // Several frames on the way, as a real drag reports.
    for &p in &path[1..] {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.run_steps(1);
    }
    press(h, path.last().copied().unwrap_or(first), false);
    h.run_steps(2);
}

fn selected(h: &mut Harness) -> Vec<u32> {
    let r = h.state_mut().session.execute("draw.lassoSelection", json!({})).unwrap();
    r["selected"].as_array().unwrap().iter().filter_map(|v| v.as_u64().map(|v| v as u32)).collect()
}

fn inside_id(h: &Harness) -> u32 {
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    sh.shapes.iter().find(|s| s.kind == ShapeKind::Ink).unwrap().id
}

fn ink(h: &Harness, id: u32) -> gridcraft_engine::model::Shape {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.iter().find(|s| s.id == id).unwrap().clone()
}

#[test]
fn a_lasso_loop_selects_the_ink_inside_it_and_dragging_it_moves_it() {
    let mut h = harness();
    let inside = inside_id(&h);
    // A loop around the first stroke only.
    let loop_pts = [(120.0, 120.0), (210.0, 120.0), (210.0, 210.0), (120.0, 210.0), (120.0, 120.0)];
    let path: Vec<Pos2> = loop_pts.iter().map(|&(x, y)| screen(&h, x, y)).collect();
    drag(&mut h, &path);
    assert_eq!(selected(&mut h), vec![inside], "only the ink inside the loop is selected");
    assert_eq!(h.state().session.draw_tool, "lasso");

    let before = ink(&h, inside);
    let steps = h.state().session.doc().unwrap().undo.len();
    // Dragging the selected stroke moves the selection, live, as one undo step: the drag runs
    // from sheet point (150, 135) to (210, 170).
    let (from, to) = (screen(&h, 150.0, 135.0), screen(&h, 210.0, 170.0));
    drag(&mut h, &[from, to]);
    let after = ink(&h, inside);
    assert!((after.anchor.dx - (before.anchor.dx + 60.0)).abs() < 1.0, "dx {} -> {}", before.anchor.dx, after.anchor.dx);
    assert!((after.anchor.dy - (before.anchor.dy + 35.0)).abs() < 1.0, "dy {} -> {}", before.anchor.dy, after.anchor.dy);
    assert_eq!(after.text, before.text, "moving changes the anchor, not the stroke's own points");
    assert_eq!(h.state().session.doc().unwrap().undo.len(), steps + 1, "a drag is one undo step");
    assert_eq!(selected(&mut h), vec![inside], "the selection stays after the move");

    // A second drag is a step of its own: one Undo reverts only it, never both drags.
    let mid = ink(&h, inside);
    let (from2, to2) = (screen(&h, 210.0, 175.0), screen(&h, 240.0, 185.0));
    drag(&mut h, &[from2, to2]);
    let end = ink(&h, inside);
    assert!((end.anchor.dx - (mid.anchor.dx + 30.0)).abs() < 1.0, "dx {} -> {}", mid.anchor.dx, end.anchor.dx);
    assert!((end.anchor.dy - (mid.anchor.dy + 10.0)).abs() < 1.0, "dy {} -> {}", mid.anchor.dy, end.anchor.dy);
    assert_eq!(h.state().session.doc().unwrap().undo.len(), steps + 2, "each drag is one undo step");
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    let back = ink(&h, inside);
    assert!((back.anchor.dx - mid.anchor.dx).abs() < 0.01, "Undo reverts only the second drag: dx {} != {}", back.anchor.dx, mid.anchor.dx);
    assert!((back.anchor.dy - mid.anchor.dy).abs() < 0.01, "Undo reverts only the second drag: dy {} != {}", back.anchor.dy, mid.anchor.dy);
}
