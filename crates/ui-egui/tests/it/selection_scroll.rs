//! Extending the selection from the keyboard scrolls its moving end into view, not the anchor.
//! Go To and Select over the control channel scroll the active cell into view.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::control::{Outcome, handle};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };

/// Column A filled down to A200 and row 1 filled across to CV1, far past one screen each way.
fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let down: Vec<Vec<i64>> = (1..=200).map(|r| vec![r]).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": down})).unwrap();
    let across: Vec<i64> = (1..=100).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": [across]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

/// Whether the cell's centre is on screen (a whole cell can sit a few pixels under the edge when
/// scrolling far down widens the row headings).
fn visible(h: &Harness, a1: &str) -> bool {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cells.contains(geo.cell_rect(sheet, CellRef::parse(a1).unwrap()).center())
}

fn assert_extended(h: &Harness, anchor: &str, moving_end: &str) {
    let selection = &h.state().session.active().unwrap().selection;
    let (anchor_cell, end_cell) = (CellRef::parse(anchor).unwrap(), CellRef::parse(moving_end).unwrap());
    assert_eq!(selection.current(), RangeRef::new(anchor_cell, end_cell));
    assert_eq!(selection.anchor, anchor_cell);
    assert_eq!(selection.active, anchor_cell, "extending keeps the active cell on the anchor");
    assert!(visible(h, moving_end), "the moving end {moving_end} is scrolled into view");
}

#[test]
fn ctrl_shift_arrows_scroll_the_moving_end_into_view() {
    // Down, then back Up past the top once the view has scrolled.
    let mut h = harness();
    key(&mut h, Key::ArrowDown, CTRL_SHIFT);
    assert_extended(&h, "A1", "A200");
    assert!(!visible(&h, "A1"), "the view follows the moving end, away from the anchor");
    let mut h = harness();
    key(&mut h, Key::ArrowDown, CTRL);
    assert!(visible(&h, "A200") && !visible(&h, "A1"));
    key(&mut h, Key::ArrowUp, CTRL_SHIFT);
    assert_extended(&h, "A200", "A1");
    // Right, then back Left.
    let mut h = harness();
    key(&mut h, Key::ArrowRight, CTRL_SHIFT);
    assert_extended(&h, "A1", "CV1");
    let mut h = harness();
    key(&mut h, Key::ArrowRight, CTRL);
    assert!(visible(&h, "CV1") && !visible(&h, "A1"));
    key(&mut h, Key::ArrowLeft, CTRL_SHIFT);
    assert_extended(&h, "CV1", "A1");
}

#[test]
fn shift_arrows_scroll_the_moving_end_into_view() {
    let mut h = harness();
    for _ in 0..60 {
        key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    }
    assert_extended(&h, "A1", "A61");
    for _ in 0..30 {
        key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    }
    assert_extended(&h, "A1", "AE61");
}

#[test]
fn ctrl_shift_home_and_end_scroll_the_moving_end_into_view() {
    let mut h = harness();
    key(&mut h, Key::End, CTRL_SHIFT);
    assert_extended(&h, "A1", "CV200");
    let mut h = harness();
    key(&mut h, Key::End, CTRL);
    assert!(!visible(&h, "A1"));
    key(&mut h, Key::Home, CTRL_SHIFT);
    assert_extended(&h, "CV200", "A1");
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    let out = match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    };
    h.run_steps(2);
    out
}

fn active(h: &Harness) -> CellRef {
    h.state().session.active().unwrap().selection.active
}

#[test]
fn go_to_over_the_control_channel_scrolls_the_target_into_view() {
    let mut h = harness();
    assert!(!visible(&h, "Z300"));
    let r = control(&mut h, "engine.execute", json!({"command": "edit.goTo", "params": {"reference": "Z300"}}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(active(&h), CellRef::parse("Z300").unwrap());
    assert!(visible(&h, "Z300"), "Go To scrolls Z300 into view");
    assert!(!visible(&h, "A1"));
    // A command named as the method runs the same way.
    control(&mut h, "edit.goTo", json!({"reference": "A1"}));
    assert!(visible(&h, "A1") && !visible(&h, "Z300"));
}

#[test]
fn select_over_the_control_channel_scrolls_the_active_cell_into_view() {
    let mut h = harness();
    control(&mut h, "engine.execute", json!({"command": "selection.set", "params": {"range": "BZ150:CA160"}}));
    assert_eq!(active(&h), CellRef::parse("BZ150").unwrap());
    assert!(visible(&h, "BZ150") && !visible(&h, "A1"), "selecting a far range scrolls its active cell into view");
}

fn scroll(h: &Harness) -> egui::Vec2 {
    h.state().view().scroll
}

#[test]
fn page_down_and_back_up_from_row_10_shows_rows_2_to_9_again() {
    // The reported steps: row 1 frozen, row 10 selected, Page Down, Page Up, Page Up.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "A10"})).unwrap();
    h.run_steps(2);
    key(&mut h, Key::PageDown, Modifiers::NONE);
    key(&mut h, Key::PageUp, Modifiers::NONE);
    assert_eq!(active(&h), CellRef::parse("A10").unwrap());
    key(&mut h, Key::PageUp, Modifiers::NONE);
    assert_eq!(active(&h), CellRef::parse("A1").unwrap());
    assert_eq!(scroll(&h).y, 0.0);
    assert!(visible(&h, "A2") && visible(&h, "A9"));
}

#[test]
fn page_up_into_a_frozen_top_row_scrolls_the_rows_below_it_back_to_the_top() {
    // Row 1 frozen: Page Up from far down ends on A1, and rows 2 onwards show under it again
    // instead of the view staying a page down.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    key(&mut h, Key::ArrowDown, CTRL);
    assert!(!visible(&h, "A2"));
    for _ in 0..20 {
        key(&mut h, Key::PageUp, Modifiers::NONE);
    }
    assert_eq!(active(&h), CellRef::parse("A1").unwrap());
    assert_eq!(scroll(&h).y, 0.0, "the scrolled rows are back at the top");
    assert!(visible(&h, "A2"));
    // Ctrl+Up from the bottom lands on A1 the same way.
    key(&mut h, Key::ArrowDown, CTRL);
    assert!(!visible(&h, "A2"));
    key(&mut h, Key::ArrowUp, CTRL);
    assert_eq!(active(&h), CellRef::parse("A1").unwrap());
    assert!(visible(&h, "A2"));
}

#[test]
fn ctrl_left_into_a_frozen_first_column_scrolls_the_columns_beside_it_back() {
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "B1"})).unwrap();
    key(&mut h, Key::ArrowRight, CTRL);
    assert!(!visible(&h, "B1"));
    key(&mut h, Key::ArrowLeft, CTRL);
    assert_eq!(active(&h), CellRef::parse("A1").unwrap());
    assert_eq!(scroll(&h).x, 0.0);
    assert!(visible(&h, "B1"));
}

#[test]
fn moving_within_a_frozen_row_leaves_the_scrolled_rows_where_they_are() {
    // Selecting in row 1 while the rows below are scrolled down (without a reveal, as a click
    // does), then moving along the frozen row, must not jump the view back to the top.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    key(&mut h, Key::ArrowDown, CTRL);
    let down = scroll(&h).y;
    assert!(down > 0.0);
    h.state_mut().session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    h.run_steps(2);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(active(&h), CellRef::parse("B1").unwrap());
    assert_eq!(scroll(&h).y, down, "the view stays scrolled down");
}

#[test]
fn alt_page_up_into_a_frozen_first_column_scrolls_the_columns_beside_it_back() {
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "B1"})).unwrap();
    key(&mut h, Key::ArrowRight, CTRL);
    assert!(!visible(&h, "B1"));
    for _ in 0..20 {
        key(&mut h, Key::PageUp, Modifiers::ALT);
    }
    assert_eq!(active(&h), CellRef::parse("A1").unwrap());
    assert_eq!(scroll(&h).x, 0.0);
    assert!(visible(&h, "B1"));
}

#[test]
fn extending_up_into_a_frozen_top_row_scrolls_the_rows_below_it_back_to_the_top() {
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    key(&mut h, Key::ArrowDown, CTRL);
    assert!(!visible(&h, "A2"));
    key(&mut h, Key::ArrowUp, CTRL_SHIFT);
    assert_extended(&h, "A200", "A1");
    assert!(visible(&h, "A2"));
}

#[test]
fn other_reveals_onto_a_frozen_cell_leave_the_scrolled_rows_where_they_are() {
    // Only arrow and paging keys bring the scrolled rows back; selecting a header cell from the
    // control channel (as Find, Go To or Undo would) keeps the place the user scrolled to.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    key(&mut h, Key::ArrowDown, CTRL);
    let down = scroll(&h).y;
    control(&mut h, "engine.execute", json!({"command": "selection.set", "params": {"cell": "B1"}}));
    assert_eq!(active(&h), CellRef::parse("B1").unwrap());
    assert_eq!(scroll(&h).y, down);
}

#[test]
fn pointing_along_a_frozen_row_keeps_the_edited_cell_in_view() {
    // Editing a formula far down and pointing at header cells with the arrows must not scroll
    // the cell being edited away.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    key(&mut h, Key::ArrowDown, CTRL);
    let down = scroll(&h).y;
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    let ed = h.state_mut().editor.as_mut().expect("editing");
    ed.caret = ed.text.chars().count();
    ed.point_cell = Some(CellRef::parse("B1").unwrap());
    h.run_steps(2);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.point_cell), Some(CellRef::parse("C1").unwrap()), "the arrow moved the point");
    assert_eq!(scroll(&h).y, down);
    assert!(visible(&h, "A200"), "the edited cell stays in view");
}

#[test]
fn down_from_a_frozen_header_row_goes_to_row_2_at_the_top_from_far_down() {
    // Excel's quick way back to the top: select a cell in the frozen header row while far down,
    // press Down, and the view jumps to row 2 under the header. Selecting the header cell alone
    // doesn't scroll.
    let mut h = harness();
    h.state_mut().session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "A100000"})).unwrap();
    h.state_mut().grid.ensure_visible = true;
    h.run_steps(2);
    assert!(!visible(&h, "A2") && visible(&h, "A100000"));
    h.state_mut().session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    h.run_steps(2);
    assert!(visible(&h, "A100000"));
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(active(&h), CellRef::parse("A2").unwrap());
    assert_eq!(scroll(&h).y, 0.0);
    assert!(visible(&h, "A2"));
}
