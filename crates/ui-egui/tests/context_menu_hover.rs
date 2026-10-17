//! Right-click cell context menu rows must show a hover highlight.

use egui::{Color32, Event, Modifiers, PointerButton, Pos2, Shape, pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["Red", 1], ["Blue", 2]]})).unwrap();
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

fn cell_pos(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sh, gridcraft_engine::core::CellRef::parse(cell).unwrap()).center()
}

fn right_click(h: &mut Harness, pos: Pos2) {
    // Press and release must land in different frames, or the menu closes in the frame that opens it.
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// True when a shape over `rect`'s centre paints exactly `fill`.
fn paints_fill_over(h: &Harness, rect: egui::Rect, fill: Color32) -> bool {
    h.output().shapes.iter().any(|clipped| match &clipped.shape {
        Shape::Rect(shape) => shape.fill == fill && shape.rect.contains(rect.center()),
        _ => false,
    })
}

/// The reported bug: the row under the pointer keeps the popup background.
#[test]
fn cell_context_menu_row_highlights_on_hover() {
    let mut h = harness();
    let at = cell_pos(&h, "B2");
    right_click(&mut h, at);
    assert!(h.state().grid.context_menu.is_some(), "right click on a cell must open the context menu");

    // A label that only the cell menu shows, so the ribbon's own Copy button cannot match.
    let item = h.get_by_label("Filter by Selected Cell's Value");
    let rect = item.rect();
    let hover = h.ctx.global_style().visuals.widgets.hovered.weak_bg_fill;
    assert!(hover != Color32::TRANSPARENT, "the theme must define a hover fill to assert against");

    h.hover_at(rect.center());
    h.run_steps(2);

    assert!(paints_fill_over(&h, rect, hover), "the hovered context menu row must paint the hover fill {hover:?}, not just the popup background");
}

/// The pointer leaves the row: the highlight must go away.
#[test]
fn cell_context_menu_row_clears_highlight_when_left() {
    let mut h = harness();
    let at = cell_pos(&h, "B2");
    right_click(&mut h, at);
    let rect = h.get_by_label("Sort A to Z").rect();
    let hover = h.ctx.global_style().visuals.widgets.hovered.weak_bg_fill;

    h.hover_at(rect.center());
    h.run_steps(2);
    assert!(paints_fill_over(&h, rect, hover), "sanity: the row highlights while hovered");

    h.hover_at(pos2(600.0, 400.0));
    h.run_steps(2);
    assert!(!paints_fill_over(&h, rect, hover), "the highlight must clear once the pointer leaves the row");
}
