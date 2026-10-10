//! Cell selection consumes every pointer position without editing cell contents.

use egui::{Event, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::{Value, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(initial: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["sentinel", 2], [3, "untouched"]]})).unwrap();
    session.execute("cell.set", json!({"cell": "C3", "input": "123"})).unwrap();
    session.execute("cell.set", json!({"cell": "F7", "input": "=C3*2"})).unwrap();
    session.execute("selection.set", json!({"cell": initial})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn cells(h: &Harness) -> Value {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    json!(sheet.cells.iter().map(|(pos, cell)| (pos.a1(), cell)).collect::<Vec<_>>())
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn press(h: &mut Harness, cell: &str) {
    let from = point(h, cell);
    frame(h, Event::PointerMoved(from));
    frame(h, button(from, true));
}

fn assert_selection(h: &Harness, start: &str, end: &str) {
    let selection = &h.state().session.active().unwrap().selection;
    let start = CellRef::parse(start).unwrap();
    assert_eq!(selection.current(), RangeRef::new(start, CellRef::parse(end).unwrap()));
    assert_eq!(selection.anchor, start);
    assert_eq!(selection.active, start);
}

#[test]
fn single_movement_selects_from_the_pressed_cell_in_both_directions() {
    for (initial, start, end) in [("A1", "C3", "F7"), ("C3", "C3", "F7"), ("A1", "F7", "C3"), ("F7", "F7", "C3")] {
        let mut h = harness(initial);
        let before = cells(&h);
        press(&mut h, start);
        let to = point(&h, end);
        frame(&mut h, Event::PointerMoved(to));
        assert_selection(&h, start, end);
        frame(&mut h, button(to, false));
        h.run_steps(1);
        assert_selection(&h, start, end);
        assert_eq!(cells(&h), before);
    }
}

#[test]
fn release_consumes_a_new_endpoint_without_an_intermediate_movement_frame() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    assert_selection(&h, "C3", "F7");
    assert_eq!(cells(&h), before);
}

#[test]
fn multiple_movement_frames_keep_the_anchor_and_cell_contents() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    for end in ["D4", "F7", "B2"] {
        let to = point(&h, end);
        frame(&mut h, Event::PointerMoved(to));
        assert_selection(&h, "C3", end);
    }
    let to = point(&h, "B2");
    frame(&mut h, button(to, false));
    assert_selection(&h, "C3", "B2");
    assert_eq!(cells(&h), before);
}

#[test]
fn clicking_then_hovering_does_not_extend_selection() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    let from = point(&h, "C3");
    frame(&mut h, button(from, false));
    let to = point(&h, "F7");
    frame(&mut h, Event::PointerMoved(to));
    assert_selection(&h, "C3", "C3");
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_point_mode_consumes_first_movement_and_release() {
    let mut h = harness("A1");
    let before = cells(&h);
    h.state_mut().begin_edit(Some("=".into()), false);
    h.run_steps(2);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=C3:D4");
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    h.run_steps(1);
    let editor = h.state().editor.as_ref().unwrap();
    assert_eq!(editor.text, "=C3:F7");
    assert_eq!(editor.point_cell, CellRef::parse("C3"));
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_point_drag_keeps_the_sheet_qualifier() {
    let mut h = harness("A1");
    let before = cells(&h);
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.state_mut().begin_edit(Some("=".into()), false);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Other sheet"})).unwrap();
    h.run_steps(2);
    let other_before = cells(&h);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "='Other sheet'!C3:D4");
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "='Other sheet'!C3:F7");
    assert_eq!(cells(&h), other_before);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!(cells(&h), before);
}
