//! While editing a formula, its coloured reference outlines on the grid can be dragged: the border
//! moves the reference to other cells, a corner resizes it. The formula text follows.

use egui::{Event, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn editing(formula: &str) -> Harness {
    editing_with(formula, |_| {})
}

/// `editing`, with `setup` run on the app before the edit starts.
fn editing_with(formula: &str, setup: impl FnOnce(&mut SheetApp)) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "C1", "input": formula})).unwrap();
    session.execute("selection.set", json!({"cell": "C1"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    setup(h.state_mut());
    h.run_steps(2);
    // F2: edit the existing formula, caret at the end (not in point mode).
    h.state_mut().begin_edit(None, false);
    h.run_steps(2);
    assert!(!h.state().editor.as_ref().unwrap().can_point());
    h
}

fn rect(h: &Harness, range: &str) -> egui::Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).range_rect(sheet, RangeRef::parse(range).unwrap())
}

fn center(h: &Harness, a1: &str) -> Pos2 {
    rect(h, &CellRef::parse(a1).unwrap().a1()).center()
}

/// Press at `from`, move to `to` and release; `from == to` is a plain click.
fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    drag_with(h, PointerButton::Primary, from, to);
}

fn drag_with(h: &mut Harness, button: PointerButton, from: Pos2, to: Pos2) {
    let moves = if from == to { vec![] } else { vec![Event::PointerMoved(from + egui::vec2(6.0, 6.0)), Event::PointerMoved(to)] };
    send(h, button, from, to, moves);
}

fn send(h: &mut Harness, button: PointerButton, from: Pos2, to: Pos2, moves: Vec<Event>) {
    let press = Event::PointerButton { pos: from, button, pressed: true, modifiers: Modifiers::NONE };
    let release = Event::PointerButton { pos: to, button, pressed: false, modifiers: Modifiers::NONE };
    for event in [Event::PointerMoved(from), press].into_iter().chain(moves).chain([release]) {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn text(h: &Harness) -> String {
    h.state().editor.as_ref().map(|e| e.text.clone()).unwrap_or_default()
}

#[test]
fn dragging_a_reference_border_moves_it() {
    let mut h = editing("=A1*B1");
    // Grab B1 by its top edge, a little right of the corner, and drop on D3.
    let b1 = rect(&h, "B1");
    let grab = Pos2::new(b1.center().x, b1.top() + 1.0);
    let to = center(&h, "D3") + (grab - b1.center());
    drag(&mut h, grab, to);
    assert_eq!(text(&h), "=A1*D3");
    assert!(h.state().editor.is_some(), "the edit stays open after the drag");
}

#[test]
fn dragging_from_inside_a_reference_cell_moves_it() {
    let mut h = editing("=A1*B5");
    let (from, to) = (center(&h, "B5"), center(&h, "B4"));
    drag(&mut h, from, to);
    assert_eq!(text(&h), "=A1*B4");
}

#[test]
fn in_point_mode_a_drag_inside_the_pointed_cell_points_a_range() {
    let mut h = editing("=1");
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    let (from, to) = (center(&h, "A1"), center(&h, "A5"));
    drag(&mut h, from, from);
    assert_eq!(text(&h), "=SUM(A1");
    drag(&mut h, from, to);
    assert_eq!(text(&h), "=SUM(A1:A5");
}

#[test]
fn dragging_a_reference_corner_resizes_it() {
    let mut h = editing("=SUM($A$1)");
    let a1 = rect(&h, "A1");
    let to = center(&h, "B4");
    drag(&mut h, a1.right_bottom(), to);
    assert_eq!(text(&h), "=SUM($A$1:$B$4)");
}

#[test]
fn clicking_inside_a_reference_still_commits_the_edit() {
    let mut h = editing("=A1*B1");
    let at = center(&h, "B1");
    drag(&mut h, at, at);
    assert!(h.state().editor.is_none(), "only the outline is a handle; inside the cell behaves as before");
}

#[test]
fn a_right_drag_leaves_the_reference_alone() {
    let mut h = editing("=A1*B5");
    let (from, to) = (center(&h, "B5"), center(&h, "B4"));
    drag_with(&mut h, PointerButton::Secondary, from, to);
    assert!(!text(&h).contains("B4"), "only the primary button drags a reference");
}

#[test]
fn a_drag_released_right_after_one_move_still_moves_the_reference() {
    let mut h = editing("=A1*B5");
    let (from, to) = (center(&h, "B5"), center(&h, "B3"));
    send(&mut h, PointerButton::Primary, from, to, vec![Event::PointerMoved(to)]);
    assert_eq!(text(&h), "=A1*B3");
}

#[test]
fn a_released_reference_stays_put_and_the_next_click_commits() {
    let mut h = editing("=A1*B5");
    let (from, to) = (center(&h, "B5"), center(&h, "B3"));
    drag(&mut h, from, to);
    let at = center(&h, "E8");
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    assert_eq!(text(&h), "=A1*B3", "the reference no longer follows the pointer");
    drag(&mut h, at, at);
    assert!(h.state().editor.is_none(), "a click after the drag commits the edit");
}

#[test]
fn a_reference_scrolled_under_frozen_rows_is_not_grabbed_there() {
    // Row 1 frozen, scrolled two rows: B3:B10 is drawn under the frozen B1, but isn't in that pane.
    let mut h = editing_with("=SUM(B3:B10)", |app| {
        app.session.run("view.freezeTopRow", json!({})).unwrap();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let rows = (sheet.row_top(3) - sheet.row_top(1)) as f32;
        app.view_mut().unwrap().scroll.y = rows;
    });
    let b1 = rect(&h, "B1");
    assert!(rect(&h, "B3:B10").contains(b1.center()), "the range lies under B1");
    let to = b1.center() + egui::vec2(0.0, b1.height() * 4.0);
    drag(&mut h, b1.center(), to);
    assert!(h.state().editor.is_none() || text(&h) == "=SUM(B3:B10)", "got {}", text(&h));
}
