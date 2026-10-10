//! Resizing a chart by dragging its bottom-right corner or with the Format Chart Area Size fields, through real input.

use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn fixture() -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    session.execute("cell.set", json!({"cell": "A2", "input": "2"})).unwrap();
    session.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "D3", "width": 300, "height": 200})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    (h, id)
}

fn size(h: &Harness, id: u32) -> (f32, f32) {
    let a = h.state().session.doc().unwrap().wb.active().unwrap().charts.iter().find(|c| c.id == id).unwrap().anchor;
    (a.width, a.height)
}

fn corner(h: &Harness, id: u32) -> egui::Pos2 {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let a = sh.charts.iter().find(|c| c.id == id).unwrap().anchor;
    let min = geo.cell_rect(sh, CellRef::parse("D3").unwrap()).min + vec2(a.dx, a.dy) * geo.z;
    min + vec2(a.width, a.height) * geo.z
}

fn button(h: &mut Harness, pos: egui::Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn dragging_the_corner_handle_resizes_the_chart() {
    let (mut h, id) = fixture();
    let (w0, h0) = size(&h, id);
    let start = corner(&h, id);
    // Select the chart first, as the user does before grabbing a handle.
    let inside = start - vec2(40.0, 40.0);
    h.input_mut().events.push(Event::PointerMoved(inside));
    h.run_steps(2);
    button(&mut h, inside, true);
    button(&mut h, inside, false);
    assert_eq!(h.state().selected_chart, Some(id));

    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeNwSe, "the corner shows a resize cursor");
    button(&mut h, start, true);
    // The pointer moves well past the drag threshold in one step, like a quick drag.
    let end = start + vec2(60.0, 30.0);
    h.input_mut().events.push(Event::PointerMoved(pos2(end.x, end.y)));
    h.run_steps(3);
    button(&mut h, end, false);
    let (w1, h1) = size(&h, id);
    assert!(w1 > w0 + 40.0 && h1 > h0 + 20.0, "chart grew from {w0}x{h0} to {w1}x{h1}");
}

/// Opens Format Chart Area for the chart and expands its Size section; returns the Width field's rect.
fn open_size_fields(h: &mut Harness, id: u32) -> egui::Rect {
    h.state_mut().selected_chart = Some(id);
    h.state_mut().grid.pane = Some("formatChart".into());
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Size").click();
    h.run_steps(2);
    h.get_all_by_role(Role::SpinButton).next().unwrap().rect()
}

#[test]
fn typing_a_width_in_the_size_field_resizes_the_chart() {
    let (mut h, id) = fixture();
    let (_, h0) = size(&h, id);
    let field = open_size_fields(&mut h, id);
    h.input_mut().events.push(Event::PointerMoved(field.center()));
    button(&mut h, field.center(), true);
    button(&mut h, field.center(), false);
    // Clicking the field selects its text; each keystroke must survive the next frame.
    for digit in ["6", "4", "0"] {
        h.input_mut().events.push(Event::Text(digit.into()));
        h.run_steps(2);
    }
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(size(&h, id), (640.0, h0));
}

#[test]
fn dragging_the_size_field_resizes_the_chart() {
    let (mut h, id) = fixture();
    let (w0, _) = size(&h, id);
    let field = open_size_fields(&mut h, id);
    h.input_mut().events.push(Event::PointerMoved(field.center()));
    button(&mut h, field.center(), true);
    for step in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(field.center() + vec2(10.0 * step as f32, 0.0)));
        h.run_steps(1);
    }
    button(&mut h, field.center() + vec2(60.0, 0.0), false);
    let (w1, _) = size(&h, id);
    assert!(w1 > w0 + 20.0, "width went from {w0} to {w1}");
}
