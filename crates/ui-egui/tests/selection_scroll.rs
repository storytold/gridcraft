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
