//! The mouse wheel over the ribbon switches tabs, as in Excel (issue #897): down for the next tab,
//! up for the previous one, one tab per notch, stopping at the ends.

use egui::accesskit::Role;
use egui::{Event, Modifiers, MouseWheelUnit, Pos2, TouchPhase, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

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

/// A point on the tab row, right of the last tab, and one on the ribbon card below it.
fn tab_row_and_card(h: &Harness) -> (Pos2, Pos2) {
    let file = h.get_by_label("File").rect();
    (pos2(1000.0, file.center().y), pos2(600.0, file.bottom() + 40.0))
}

/// One wheel event at `at`, then `steps` frames.
fn wheel_for(h: &mut Harness, at: Pos2, unit: MouseWheelUnit, dy: f32, steps: usize) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(Event::MouseWheel { unit, delta: vec2(0.0, dy), phase: TouchPhase::Move, modifiers: Modifiers::NONE });
    h.run_steps(steps);
}

/// One wheel event, then long enough for the scroll to settle.
fn wheel(h: &mut Harness, at: Pos2, unit: MouseWheelUnit, dy: f32) {
    wheel_for(h, at, unit, dy, 30);
}

fn tab(h: &Harness) -> String {
    h.state().ui.ribbon_tab.clone()
}

#[test]
fn wheel_notches_step_through_the_tabs_without_wrapping() {
    let mut h = harness();
    let (row, card) = tab_row_and_card(&h);
    wheel(&mut h, row, MouseWheelUnit::Line, -1.0);
    assert_eq!(tab(&h), "Insert", "one notch down over the tab row selects the next tab");
    wheel(&mut h, card, MouseWheelUnit::Line, -1.0);
    assert_eq!(tab(&h), "Draw", "the ribbon card switches too");
    wheel(&mut h, row, MouseWheelUnit::Line, 1.0);
    assert_eq!(tab(&h), "Insert", "a notch up goes back");
    for _ in 0..3 {
        wheel(&mut h, row, MouseWheelUnit::Line, 1.0);
    }
    assert_eq!(tab(&h), "Home", "up stops at the first tab");
    h.state_mut().ui.ribbon_tab = "Automate".into();
    h.run_steps(2);
    wheel(&mut h, row, MouseWheelUnit::Line, -1.0);
    assert_eq!(tab(&h), "Automate", "down stops at the last tab");
}

/// A trackpad gesture: a start phase, a stream of small deltas (one every frame or two), an end.
fn swipe(h: &mut Harness, at: Pos2, dy: f32, events: usize) {
    let phase = |phase| Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 0.0), phase, modifiers: Modifiers::NONE };
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(phase(TouchPhase::Start));
    for _ in 0..events {
        wheel_for(h, at, MouseWheelUnit::Point, dy, 1);
    }
    h.input_mut().events.push(phase(TouchPhase::End));
    h.run_steps(30);
}

#[test]
fn trackpad_deltas_accumulate_to_one_tab_at_a_time() {
    let mut h = harness();
    let (row, _) = tab_row_and_card(&h);
    swipe(&mut h, row, -4.0, 4);
    assert_eq!(tab(&h), "Home", "a small swipe does not switch");
    swipe(&mut h, row, -4.0, 10);
    assert_eq!(tab(&h), "Insert", "a notch's worth of swipe moves one tab, not one per event");
}

#[test]
fn wheel_does_not_switch_tabs_with_a_menu_open_or_over_the_grid() {
    let mut h = harness();
    let (row, _) = tab_row_and_card(&h);
    h.get_by_role_and_label(Role::Button, "Paste").click_secondary();
    h.run_steps(3);
    assert!(egui::Popup::is_any_open(&h.ctx), "the paste options menu is open");
    wheel(&mut h, row, MouseWheelUnit::Line, -1.0);
    assert_eq!(tab(&h), "Home", "an open menu keeps the tab");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    wheel(&mut h, pos2(600.0, 500.0), MouseWheelUnit::Line, -1.0);
    assert_eq!(tab(&h), "Home", "the wheel over the grid scrolls the sheet, not the ribbon");
}

/// macOS scales a notched wheel by how fast it turns, and reports most mice in points with no
/// gesture phases: about 13 points for a slow notch, over 100 for a fast one. Mice that report
/// lines get a tenth of a line to several. A notch is a notch either way.
#[test]
fn a_notch_switches_one_tab_however_small_or_large_its_delta() {
    for (unit, slow, fast) in [(MouseWheelUnit::Point, 13.0, 103.0), (MouseWheelUnit::Line, 0.1, 3.0)] {
        let mut h = harness();
        let (row, _) = tab_row_and_card(&h);
        wheel(&mut h, row, unit, -slow);
        assert_eq!(tab(&h), "Insert", "a slow notch switches ({unit:?})");
        wheel(&mut h, row, unit, -slow);
        assert_eq!(tab(&h), "Draw", "and so does the next one ({unit:?})");
        wheel(&mut h, row, unit, slow);
        assert_eq!(tab(&h), "Insert", "a slow notch up goes back ({unit:?})");
        wheel(&mut h, row, unit, -fast);
        assert_eq!(tab(&h), "Draw", "a fast notch is still one tab ({unit:?})");
        // Quick notches, before the smoothed scroll of the previous one has settled.
        h.state_mut().ui.ribbon_tab = "Home".into();
        h.run_steps(2);
        for _ in 0..3 {
            wheel_for(&mut h, row, unit, -fast, 2);
        }
        h.run_steps(30);
        assert_eq!(tab(&h), "Page Layout", "three quick notches are three tabs ({unit:?})");
    }
}
