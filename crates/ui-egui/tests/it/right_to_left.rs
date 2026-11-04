//! Sheet Right-to-Left: the grid is mirrored on screen and pointer/keyboard input follows it.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["first", 2], [3, "fourth"]]})).unwrap();
    session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    crate::support::harness(session, 4)
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

/// Where `a1` is drawn on screen.
fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    geo(h).screen_cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn current(h: &Harness) -> RangeRef {
    h.state().session.active().unwrap().selection.current()
}

#[test]
fn column_a_is_drawn_on_the_right_and_clicks_hit_the_mirrored_cells() {
    let mut h = harness();
    let g = geo(&h);
    assert!(g.rtl);
    let (a1, b1) = (point(&h, "A1"), point(&h, "B1"));
    assert!(a1.x > b1.x, "A1 at {a1:?} should be right of B1 at {b1:?}");
    // Row headers sit on the right edge, so A1 ends where they begin.
    let sheet_a1 = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        g.screen_cell_rect(sheet, CellRef::parse("A1").unwrap())
    };
    assert!((sheet_a1.right() - (g.rect.right() - g.header_w)).abs() < 0.5);

    // Drag from C3 to F7 at their on-screen positions.
    let (from, to) = (point(&h, "C3"), point(&h, "F7"));
    frame(&mut h, Event::PointerMoved(from));
    frame(&mut h, button(from, true));
    frame(&mut h, Event::PointerMoved(to));
    frame(&mut h, button(to, false));
    h.run_steps(1);
    assert_eq!(current(&h), RangeRef::parse("C3:F7").unwrap());
}

#[test]
fn left_and_right_arrows_follow_the_screen() {
    let mut h = harness();
    h.key_press(Key::ArrowLeft);
    h.run_steps(1);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("C2").unwrap());
    h.key_press(Key::ArrowRight);
    h.key_press(Key::ArrowRight);
    h.run_steps(1);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("A2").unwrap());
}

fn gradient_harness(rtl: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [[1]]})).unwrap();
    session
        .execute(
            "home.formatCells",
            json!({"range": "A1", "style": {"fill": {"gradient": {
                "kind": {"Linear": {"degree": 0.0}},
                "stops": [
                    {"position": 0.0, "color": {"Rgb": 16711680}},
                    {"position": 1.0, "color": {"Rgb": 255}}
                ]
            }}}}),
        )
        .unwrap();
    session.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
    session.execute("selection.set", json!({"cell": "C4"})).unwrap();
    crate::support::harness(session, 4)
}

/// The gradient mesh painted over `cell`, as its colour at the left and right edge.
fn gradient_ends(h: &Harness, cell: &str) -> (egui::Color32, egui::Color32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let screen = geo(h).screen_cell_rect(sheet, CellRef::parse(cell).unwrap());
    let mut vertices = Vec::new();
    collect_mesh_vertices(h.output().shapes.iter().map(|s| &s.shape), screen.expand(1.0), &mut vertices);
    assert!(!vertices.is_empty(), "no gradient mesh over {cell}");
    let min = vertices.iter().map(|v| v.pos.x).fold(f32::INFINITY, f32::min);
    let max = vertices.iter().map(|v| v.pos.x).fold(f32::NEG_INFINITY, f32::max);
    let at = |x: f32| vertices.iter().find(|v| (v.pos.x - x).abs() < 0.01).unwrap().color;
    (at(min), at(max))
}

fn collect_mesh_vertices<'a>(shapes: impl Iterator<Item = &'a egui::Shape>, within: egui::Rect, out: &mut Vec<egui::epaint::Vertex>) {
    for shape in shapes {
        match shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == egui::TextureId::default() => {
                if mesh.vertices.iter().all(|v| within.contains(v.pos)) {
                    out.extend(mesh.vertices.iter().copied());
                }
            }
            egui::Shape::Vec(v) => collect_mesh_vertices(v.iter(), within, out),
            _ => {}
        }
    }
}

#[test]
fn a_cell_gradient_mirrors_with_the_sheet() {
    let ltr = gradient_harness(false);
    let rtl = gradient_harness(true);
    let (ltr_left, ltr_right) = gradient_ends(&ltr, "A1");
    let (rtl_left, rtl_right) = gradient_ends(&rtl, "A1");
    eprintln!("ltr: {ltr_left:?} .. {ltr_right:?}  rtl: {rtl_left:?} .. {rtl_right:?}");
    assert_eq!(ltr_left, egui::Color32::from_rgb(255, 0, 0));
    assert_eq!(ltr_right, egui::Color32::from_rgb(0, 0, 255));
    assert_eq!(rtl_left, ltr_right);
    assert_eq!(rtl_right, ltr_left);
}
