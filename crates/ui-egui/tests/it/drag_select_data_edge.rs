//! #4546: a selection dragged down past the bottom of the grid keeps scrolling while the pointer
//! rests there, slows down near the last row of the data and rests on it, then carries on, through
//! real pointer input.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// 1 to 300 down A1:A300, A1 selected.
fn harness() -> Harness {
    let mut session = crate::support::blank_session();
    let values: Vec<_> = (1..=300).map(|v| json!([v])).collect();
    session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    crate::support::stepped_harness(session)
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(h: &mut Harness, pos: Pos2, pressed: bool) {
    frame(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

/// The bottom row of the selection (0-based).
fn end_row(h: &Harness) -> u32 {
    h.state().session.active().unwrap().selection.current().end.row
}

/// Presses on `a1` and moves the pointer just below the grid, leaving the button down; returns
/// where the pointer rests.
fn drag_below_the_grid(h: &mut Harness, a1: &str) -> Pos2 {
    drag_across_and_below_the_grid(h, a1, a1)
}

/// Presses on `a1`, moves over to `across` and on down to just below the grid in its column,
/// leaving the button down; returns where the pointer rests.
fn drag_across_and_below_the_grid(h: &mut Harness, a1: &str, across: &str) -> Pos2 {
    let geo = h.state().grid.geo.clone().unwrap();
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let at = |a1: &str| geo.screen_cell_rect(&sh, CellRef::parse(a1).unwrap()).center();
    let (from, over) = (at(a1), at(across));
    let to = pos2(over.x, geo.cells.bottom() + 10.0);
    frame(h, Event::PointerMoved(from));
    button(h, from, true);
    for (a, b) in [(from, over), (over, to)] {
        for i in 1..=8 {
            frame(h, Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        }
    }
    to
}

/// Runs frames with the pointer at rest until the selection ends below `row` (0-based) or `limit`
/// frames pass; returns how many frames the selection ended on each row.
fn hold_until_past(h: &mut Harness, row: u32, limit: usize) -> std::collections::BTreeMap<u32, usize> {
    let mut frames = std::collections::BTreeMap::new();
    for _ in 0..limit {
        h.run_steps(1);
        *frames.entry(end_row(h)).or_insert(0) += 1;
        if end_row(h) > row {
            break;
        }
    }
    frames
}

#[test]
fn a_resting_pointer_keeps_the_drag_scrolling() {
    let mut h = harness();
    drag_below_the_grid(&mut h, "A1");
    let before = end_row(&h);
    // `try_run` steps until the UI stops asking for frames: an app that only draws on input
    // settles, and the drag would stall until the pointer moved again.
    assert!(h.try_run().is_err(), "an auto-scrolling drag keeps asking for frames without pointer movement");
    assert!(end_row(&h) > before, "the selection keeps growing: {before} -> {}", end_row(&h));
}

#[test]
fn the_drag_keeps_asking_for_frames_while_the_brake_holds_it_still() {
    let mut h = harness();
    drag_below_the_grid(&mut h, "A1");
    hold_until_past(&mut h, 288, 1200);
    assert_eq!(end_row(&h), 289, "full speed stops 10 rows short of the last data row");
    // Holding still scrolls nothing, but the brake's clock must keep running to let it go on.
    assert!(h.try_run().is_err(), "a braked drag keeps asking for frames");
}

#[test]
fn the_drag_slows_down_and_rests_on_the_last_row_of_the_data_then_carries_on() {
    let mut h = harness();
    drag_below_the_grid(&mut h, "A1");
    let frames = hold_until_past(&mut h, 310, 1200);
    // 0-based 299 is row 300, the last value. Excel rests there for a little over half a second.
    let rest = frames.get(&299).copied().unwrap_or(0);
    assert!(rest >= 30, "the selection rests on the last data row for at least half a second: {rest} frames ({frames:?})");
    assert!(frames.get(&297).copied().unwrap_or(0) >= 10, "it creeps over the last rows: {frames:?}");
    assert!(end_row(&h) > 310, "held on, it carries on past the data: {frames:?}");
}

#[test]
fn letting_go_while_it_rests_selects_to_the_last_row_of_the_data() {
    let mut h = harness();
    let at = drag_below_the_grid(&mut h, "A1");
    hold_until_past(&mut h, 298, 1200);
    h.run_steps(10);
    button(&mut h, at, false);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:A300");
}

#[test]
fn a_drag_that_starts_outside_the_data_does_not_slow_down() {
    let mut h = harness();
    drag_below_the_grid(&mut h, "B1");
    let frames = hold_until_past(&mut h, 310, 1200);
    assert!(frames.values().all(|&n| n < 5), "column B is empty: no row holds the drag: {frames:?}");
}

#[test]
fn across_columns_of_different_lengths_the_drag_rests_on_the_longest() {
    // As observed in Excel: A ends at row 300, B at 250 and C at 350; dragged from A1 across to C
    // and down, it went straight past 250 and 300 and slowed down only at 350.
    let mut session = crate::support::blank_session();
    for (col, rows) in [("A", 300), ("B", 250), ("C", 350)] {
        let values: Vec<_> = (1..=rows).map(|v| json!([v])).collect();
        session.execute("range.setValues", json!({"range": format!("{col}1"), "values": values})).unwrap();
    }
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    drag_across_and_below_the_grid(&mut h, "A1", "C1");
    let frames = hold_until_past(&mut h, 360, 1500);
    assert!(frames.get(&349).copied().unwrap_or(0) >= 30, "it rests on C's last row: {frames:?}");
    for shorter in [249, 299] {
        assert!(frames.get(&shorter).copied().unwrap_or(0) < 5, "it passes the shorter columns' ends: {frames:?}");
    }
}
