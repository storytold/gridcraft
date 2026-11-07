//! The option for how many rows a notch of the mouse wheel moves the sheet: a global preference.

use egui::{Event, MouseWheelSource, MouseWheelUnit, TouchPhase, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    let middle = h.state().grid.cells_rect.expect("the grid is drawn").center();
    h.input_mut().events.push(Event::PointerMoved(middle));
    h.run_steps(2);
    h
}

fn wheel(h: &mut Harness, unit: MouseWheelUnit, y: f32) -> f32 {
    let before = h.state().view().scroll.y;
    h.input_mut().events.push(Event::MouseWheel {
        unit,
        source: MouseWheelSource::Unknown,
        delta: vec2(0.0, y),
        phase: TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    // Wheel movement is smoothed over the following frames.
    h.run_steps(80);
    h.state().view().scroll.y - before
}

fn default_row(h: &Harness) -> f32 {
    h.state().session.active().unwrap().wb.active().unwrap().default_row_height
}

#[test]
fn a_notch_moves_the_chosen_rows() {
    let mut h = harness();
    let row = default_row(&h);
    for rows in [1, 3, 5] {
        assert_eq!(h.state_mut().run("options.wheelRows", json!({"rows": rows})), Ok(json!({"rows": rows})));
        h.run_steps(1);
        let moved = wheel(&mut h, MouseWheelUnit::Line, -1.0);
        assert!((moved - rows as f32 * row).abs() < 0.5, "{rows} rows: moved {moved}, a row is {row}");
    }
}

#[test]
fn off_by_default_and_the_wheel_keeps_its_speed() {
    assert_eq!(UiState::default().wheel_rows, 0);
    let mut plain = harness();
    let usual = wheel(&mut plain, MouseWheelUnit::Line, -1.0);
    assert!(usual > 0.0);
    let mut set_to_zero = harness();
    set_to_zero.state_mut().run("options.wheelRows", json!({"rows": 0})).unwrap();
    assert!((wheel(&mut set_to_zero, MouseWheelUnit::Line, -1.0) - usual).abs() < 0.5);
}

#[test]
fn a_trackpad_keeps_following_the_fingers() {
    let mut plain = harness();
    let usual = wheel(&mut plain, MouseWheelUnit::Point, -30.0);
    let mut h = harness();
    h.state_mut().run("options.wheelRows", json!({"rows": 5})).unwrap();
    assert!((wheel(&mut h, MouseWheelUnit::Point, -30.0) - usual).abs() < 0.5);
}

#[test]
fn it_is_clamped_and_kept_as_a_preference() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("options.wheelRows", json!({"rows": 500})), Ok(json!({"rows": 20})));
    let saved = serde_json::to_string(&UiState { wheel_rows: 7, ..UiState::default() }).unwrap();
    assert_eq!(serde_json::from_str::<UiState>(&saved).unwrap().wheel_rows, 7);
    assert_eq!(serde_json::from_str::<UiState>("{}").unwrap().wheel_rows, 0);
}
