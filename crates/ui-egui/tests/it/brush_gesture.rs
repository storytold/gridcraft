use egui::{Event, Modifiers, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::{Session, brush};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("draw.brush", json!({"on": true, "preset": "softRound"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn pointer(h: &mut Harness, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.step();
}

#[test]
fn brush_preset_selected_from_the_draw_ribbon_is_used_for_the_next_stroke() {
    let mut h = app();
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Draw"})).unwrap();
    h.run_steps(4);
    let arrow = h.get_by_label("PhotoCraft Brush options").rect().center();
    pointer(&mut h, arrow, true);
    pointer(&mut h, arrow, false);
    h.run_steps(4);

    let chalk = h.get_by_label("Chalk").rect().center();
    pointer(&mut h, chalk, true);
    pointer(&mut h, chalk, false);
    h.run_steps(4);
    assert_eq!(h.state().session.draw_brush, brush::Style::for_preset(brush::Preset::Chalk));
    assert_eq!(h.state().session.draw_tool, "brush");
    assert!(h.query_by_label("Pressure controls size").is_some());

    let start = h.state().grid.cells_rect.unwrap().min + vec2(80.0, 320.0);
    pointer(&mut h, start, true);
    pointer(&mut h, start + vec2(140.0, 40.0), false);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.images.len(), 1);
    let source = brush::source(&sheet.images[0].data).unwrap();
    assert_eq!(source.style.preset, brush::Preset::Chalk);
}

#[test]
fn mouse_brush_commits_on_release_and_eraser_undo_restores_it() {
    let mut h = app();
    let start = h.state().grid.cells_rect.unwrap().min + vec2(80.0, 70.0);
    let end = start + vec2(150.0, 0.0);
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(start + vec2(70.0, 0.0)));
    h.step();
    assert!(h.state().session.active().unwrap().wb.active().unwrap().images.is_empty());
    pointer(&mut h, end, false);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.images.len(), 1);
    assert!(brush::source(&sheet.images[0].data).is_some());
    h.state_mut().run("draw.eraser", json!({"on": true})).unwrap();
    pointer(&mut h, start + vec2(70.0, 0.0), true);
    pointer(&mut h, start + vec2(70.0, 0.0), false);
    assert!(h.state().session.active().unwrap().wb.active().unwrap().images.is_empty());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().images.len(), 1);
}

#[test]
fn touch_force_is_saved_and_cancellation_creates_no_stroke() {
    let mut h = app();
    let start = h.state().grid.cells_rect.unwrap().min + vec2(80.0, 70.0);
    let events = [(TouchPhase::Start, 0.2), (TouchPhase::Move, 0.8), (TouchPhase::End, 0.8)];
    for (i, (phase, force)) in events.into_iter().enumerate() {
        let pos = start + vec2(i as f32 * 40.0, i as f32 * 10.0);
        h.input_mut().events.push(Event::Touch { device_id: TouchDeviceId(1), id: TouchId(1), phase, pos, force: Some(force) });
        h.step();
    }
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let points = brush::source(&sheet.images[0].data).unwrap().samples;
    assert_eq!([points[0].pressure, points[1].pressure], [0.2, 0.8]);
    for phase in [TouchPhase::Start, TouchPhase::Cancel] {
        h.input_mut().events.push(Event::Touch { device_id: TouchDeviceId(1), id: TouchId(2), phase, pos: start, force: Some(0.5) });
        h.step();
    }
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().images.len(), 1);
}

#[test]
fn long_drag_at_reduced_zoom_is_one_editable_stroke() {
    let mut h = app();
    h.state_mut().run("view.zoom", json!({"percent": 50})).unwrap();
    h.run_steps(4);
    let cells = h.state().grid.cells_rect.unwrap();
    let start = cells.min + vec2(48.0, 44.0);
    let end = cells.max - vec2(50.0, 50.0);
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(start + (end - start) * 0.5));
    h.step();
    pointer(&mut h, end, false);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.images.len(), 1);
    let source = brush::source(&sheet.images[0].data).unwrap();
    assert_eq!(source.samples.len(), 3);
    assert!(source.pixels_per_unit < brush::PIXELS_PER_UNIT);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert!(h.state().session.active().unwrap().wb.active().unwrap().images.is_empty());
}
