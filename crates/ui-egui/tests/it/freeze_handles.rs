//! Freezing panes by dragging the freeze handles on the header edges (#195).

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, freeze_handles};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    crate::support::stepped_harness(session)
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.run_steps(3);
}

fn freeze(h: &Harness) -> Option<(u32, u32)> {
    h.state().session.active().unwrap().wb.active().unwrap().freeze
}

#[test]
fn dragging_the_handles_freezes_moves_and_unfreezes() {
    let mut h = harness();
    assert_eq!(freeze(&h), None);
    // Column handle from the corner to just past column C's left edge: two columns frozen.
    let g = geo(&h);
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let (cols, _) = freeze_handles(&g);
    let target = pos2(g.cell_rect(&sh, CellRef::new(0, 2)).left() + 3.0, g.cells.top() + 40.0);
    drag(&mut h, cols.center(), target);
    assert_eq!(freeze(&h), Some((0, 2)));

    // Row handle down to row 4's top: three rows frozen, columns kept.
    let g = geo(&h);
    let (_, rows) = freeze_handles(&g);
    let target = pos2(g.cells.left() + 40.0, g.cell_rect(&sh, CellRef::new(3, 4)).top() + 2.0);
    drag(&mut h, rows.center(), target);
    assert_eq!(freeze(&h), Some((3, 2)));

    // Both handles back to the corner unfreeze.
    let g = geo(&h);
    let (cols, _) = freeze_handles(&g);
    drag(&mut h, cols.center(), pos2(g.cells.left() - 5.0, cols.center().y));
    assert_eq!(freeze(&h), Some((3, 0)));
    let g = geo(&h);
    let (_, rows) = freeze_handles(&g);
    drag(&mut h, rows.center(), pos2(rows.center().x, g.cells.top() - 5.0));
    assert_eq!(freeze(&h), None);
}

/// The last frozen column ends at the freeze line however far the other columns scroll, while
/// the next column's left edge moves with the scroll (#2544).
#[test]
fn last_frozen_column_ends_at_the_freeze_line_when_scrolled() {
    let mut h = harness();
    h.state_mut().run("view.freezePanes", serde_json::json!({"cell": "C3"})).unwrap();
    h.state_mut().view_mut().unwrap().scroll = egui::vec2(300.0, 200.0);
    h.run_steps(2);
    let g = geo(&h);
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let split = g.cells.min + egui::vec2(g.frozen_w, g.frozen_h);
    assert!((g.x_end(&sh, 1) - split.x).abs() < 0.01 && (g.y_end(&sh, 1) - split.y).abs() < 0.01);
    assert!(g.x(&sh, 2) < split.x && g.y(&sh, 2) < split.y, "C and row 3 are scrolled under the frozen panes");
    assert!(g.same_col_pane(0, 1) && g.same_col_pane(2, 9) && !g.same_col_pane(1, 2));
}

/// Dropping the column handle on the left part of the last frozen column while scrolled freezes
/// up to its left edge: the column's right edge is the freeze line, not the next column's
/// scrolled position (#2544).
#[test]
fn dragging_the_column_handle_when_scrolled_measures_the_last_frozen_column() {
    let mut h = harness();
    h.state_mut().run("view.freezePanes", serde_json::json!({"cell": "C1"})).unwrap();
    h.state_mut().view_mut().unwrap().scroll = egui::vec2(300.0, 0.0);
    h.run_steps(2);
    let g = geo(&h);
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let (cols, _) = freeze_handles(&g);
    assert!(g.x(&sh, 2) < g.x(&sh, 1), "C is scrolled left of B");
    drag(&mut h, cols.center(), pos2(g.x(&sh, 1) + 10.0, g.cells.top() + 40.0));
    assert_eq!(freeze(&h), Some((0, 1)));
}

/// A pen tool must not swallow a freeze-handle release that ends outside the cells: there is no
/// pending stroke, so the release belongs to the freeze drag and must finish on that frame
/// (#3556).
#[test]
fn a_pen_tool_does_not_swallow_the_freeze_handle_release() {
    let mut h = harness();
    h.state_mut().session.draw_tool = "pen".into();
    h.state_mut().run("view.freezePanes", serde_json::json!({"cell": "C1"})).unwrap();
    h.run_steps(1);
    assert_eq!(freeze(&h), Some((0, 2)));
    let g = geo(&h);
    let (cols, _) = freeze_handles(&g);
    // The handle dragged back to the corner releases left of the cells, outside the grid.
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let (from, to) = (cols.center(), pos2(g.cells.left() - 5.0, cols.center().y));
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for i in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.step();
    assert_eq!(freeze(&h), None, "the release outside the cells unfreezes on the release frame");
}
