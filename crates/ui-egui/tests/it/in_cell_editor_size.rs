//! The in-cell editor covers the edited cell exactly, as in Excel, at every zoom (#1755).

use egui::{Color32, Event, Modifiers, PointerButton, Rect, Shape};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn cell_rect(h: &Harness, cell: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap())
}

fn click_cell(h: &mut Harness, cell: &str) {
    let pos = cell_rect(h, cell).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(3);
}

/// The white editor background painted from the cell's top-left corner.
fn editor_rect(h: &Harness, cell: Rect) -> Rect {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == Color32::WHITE && (r.rect.min - cell.min).length() < 0.01 => Some(r.rect),
            _ => None,
        })
        .next_back()
        .expect("in-cell editor background")
}

fn assert_close(a: Rect, b: Rect, what: &str) {
    assert!((a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01, "{what}: editor {a:?} must match cell {b:?}");
}

#[test]
fn editor_matches_the_cell_at_every_zoom() {
    for zoom in [50u16, 100, 200, 400] {
        let mut h = harness();
        h.state_mut().run("view.zoom", json!({"percent": zoom})).unwrap();
        h.run_steps(3);
        click_cell(&mut h, "B2");
        type_text(&mut h, "abc");
        assert!(h.state().editor.is_some());
        let cell = cell_rect(&h, "B2");
        assert_close(editor_rect(&h, cell), cell, &format!("zoom {zoom}"));
    }
}

#[test]
fn one_long_line_widens_the_editor_but_keeps_the_row_height() {
    let mut h = harness();
    click_cell(&mut h, "B2");
    type_text(&mut h, "a much longer line of text than the cell can hold");
    let cell = cell_rect(&h, "B2");
    let ed = editor_rect(&h, cell);
    assert!(ed.width() > cell.width() + 1.0, "long text widens the editor: {ed:?} vs {cell:?}");
    assert!((ed.height() - cell.height()).abs() < 0.01, "one line keeps the row height: {ed:?} vs {cell:?}");
}
