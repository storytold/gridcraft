//! Ctrl+wheel zoom steps along a ladder of percentages that always comes back to 100%.

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Builds a mouse-wheel event from its unit, source, phase, delta and modifiers.
fn wheel_event(unit: MouseWheelUnit, source: MouseWheelSource, phase: TouchPhase, delta: egui::Vec2, modifiers: Modifiers) -> Event {
    Event::MouseWheel { unit, delta, phase, modifiers, source }
}

/// Builds a blank stepped harness and leaves the pointer centered over the grid.
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    let over_grid = h.state().grid.rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(over_grid));
    h.run_steps(2);
    h
}

fn zoom(h: &Harness) -> u16 {
    h.state().session.active().unwrap().wb.active().unwrap().zoom
}

fn set_zoom(h: &mut Harness, percent: u16) {
    h.state_mut().run("view.zoom", json!({"percent": percent})).unwrap();
    h.run_steps(2);
}

/// Turns the wheel `turns` notches at a time with Ctrl held (up zooms in), each turn `frames`
/// frames after the last, then lets egui's easing play out.
fn wheel_turns(h: &mut Harness, turns: &[f32], frames: usize) {
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::COMMAND));
    for &t in turns {
        let delta = vec2(0.0, t);
        h.input_mut().events.push(wheel_event(MouseWheelUnit::Line, MouseWheelSource::Unknown, TouchPhase::Move, delta, Modifiers::COMMAND));
        h.run_steps(frames);
    }
    h.run_steps(60);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn wheel(h: &mut Harness, notches: f32) {
    wheel_turns(h, &[notches], 1);
}

/// The harness turns egui's animations off (and the zoom glide follows that); this turns them on.
fn animated() -> Harness {
    let mut h = harness();
    h.ctx.all_styles_mut(|s| s.animation_time = 0.1);
    h.step();
    h
}

/// Turns the wheel `notches` with Ctrl held and records the zoom over the next `frames` frames.
fn turn_and_watch(h: &mut Harness, notches: f32, frames: usize) -> Vec<u16> {
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::COMMAND));
    let delta = vec2(0.0, notches);
    h.input_mut().events.push(wheel_event(MouseWheelUnit::Line, MouseWheelSource::Unknown, TouchPhase::Move, delta, Modifiers::COMMAND));
    (0..frames)
        .map(|_| {
            h.step();
            zoom(h)
        })
        .collect()
}

#[test]
fn with_animations_the_zoom_glides_to_the_step() {
    let mut h = animated();
    let seen = turn_and_watch(&mut h, 1.0, 40);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "only ever towards the step: {seen:?}");
    let between = seen.iter().filter(|&&z| z > 100 && z < 120).count();
    assert!(between >= 3, "on its way over several frames: {seen:?}");
    assert_eq!(seen.last(), Some(&120), "and exactly on the step: {seen:?}");
    let seen = turn_and_watch(&mut h, -1.0, 40);
    assert!(seen.windows(2).all(|w| w[0] >= w[1]) && seen.iter().any(|&z| z > 100 && z < 120), "{seen:?}");
    assert_eq!(seen.last(), Some(&100), "{seen:?}");
}

#[test]
fn a_notch_during_a_glide_steps_on_from_where_it_is_going() {
    let mut h = animated();
    let seen = turn_and_watch(&mut h, 1.0, 6);
    assert!(seen.last().is_some_and(|&z| z > 100 && z < 120), "still gliding: {seen:?}");
    turn_and_watch(&mut h, 1.0, 60);
    assert_eq!(zoom(&h), 140);
    let seen = turn_and_watch(&mut h, -1.0, 6);
    assert!(seen.last().is_some_and(|&z| z > 120 && z < 140), "{seen:?}");
    turn_and_watch(&mut h, -1.0, 60);
    assert_eq!(zoom(&h), 100);
}

#[test]
fn a_zoom_set_another_way_stops_a_glide() {
    let mut h = animated();
    let seen = turn_and_watch(&mut h, 1.0, 6);
    assert!(seen.last().is_some_and(|&z| z > 100 && z < 120), "{seen:?}");
    h.state_mut().run("view.zoom", json!({"percent": 150})).unwrap();
    h.run_steps(60);
    assert_eq!(zoom(&h), 150);
    // Setting the percentage the glide had got to doesn't start it again.
    let stopped_at = seen.last().copied().unwrap();
    set_zoom(&mut h, stopped_at);
    h.run_steps(60);
    assert_eq!(zoom(&h), stopped_at);
}

#[test]
fn with_view_animations_off_a_notch_steps_at_once() {
    let mut h = animated();
    h.state_mut().run("view.animations", json!({"on": false})).unwrap();
    let seen = turn_and_watch(&mut h, 1.0, 40);
    assert!(seen.iter().all(|&z| z == 100 || z == 120), "no percentages in between: {seen:?}");
    assert_eq!(seen.last(), Some(&120));
}

#[test]
fn a_notch_out_returns_to_the_zoom_before_the_notch_in() {
    let mut h = harness();
    assert_eq!(zoom(&h), 100);
    // Multiplying the rounded zoom by egui's eased factor every frame drifted: 100 → 122 → 101 → 123 → 102.
    for _ in 0..4 {
        wheel(&mut h, 1.0);
        assert_eq!(zoom(&h), 120);
        wheel(&mut h, -1.0);
        assert_eq!(zoom(&h), 100);
    }
    for expect in [120, 140, 170] {
        wheel(&mut h, 1.0);
        assert_eq!(zoom(&h), expect);
    }
    wheel(&mut h, -3.0);
    assert_eq!(zoom(&h), 100);
    wheel(&mut h, -2.0);
    assert_eq!(zoom(&h), 70);
    // Notches turned quickly, each before the last one has eased in.
    wheel_turns(&mut h, &[1.0, 1.0, 1.0, 1.0], 3);
    assert_eq!(zoom(&h), 140);
    wheel_turns(&mut h, &[-1.0, -1.0], 3);
    assert_eq!(zoom(&h), 100);
}

#[test]
fn parts_of_a_notch_add_up_to_whole_steps() {
    // A fine-grained wheel (or a Windows touchpad pinch) sends a notch in pieces, not always evenly.
    let mut h = harness();
    wheel_turns(&mut h, &[0.25, 0.25, 0.25], 4);
    assert_eq!(zoom(&h), 120, "three quarters of a notch step");
    wheel_turns(&mut h, &[-0.3, -0.4, -0.3], 4);
    assert_eq!(zoom(&h), 100);
    wheel_turns(&mut h, &[0.25, 0.25, 0.25, 0.25, 0.3, 0.4, 0.3], 4);
    assert_eq!(zoom(&h), 140, "one step per notch, however it's divided");
    wheel_turns(&mut h, &[-0.5, -0.25, -0.25, -0.25, -0.25, -0.5], 4);
    assert_eq!(zoom(&h), 100);
    wheel_turns(&mut h, &[0.5], 4);
    assert_eq!(zoom(&h), 100, "half a notch doesn't step");
    // What's left of a notch is forgotten after a pause (`wheel_turns` waits a second) rather
    // than adding up with the next turn.
    wheel_turns(&mut h, &[0.4], 4);
    assert_eq!(zoom(&h), 100);
}

#[test]
fn from_a_zoom_set_another_way_a_notch_goes_to_the_next_step_or_100() {
    let mut h = harness();
    for (from, notches, to) in
        [(112, 1.0, 120), (112, -1.0, 100), (121, -1.0, 100), (99, 1.0, 100), (101, -1.0, 100), (385, 1.0, 400), (400, 1.0, 400), (11, -1.0, 10)]
    {
        set_zoom(&mut h, from);
        wheel(&mut h, notches);
        assert_eq!(zoom(&h), to, "{from}% by {notches}");
    }
    // Two notches out from 121%: 100%, then 85%.
    set_zoom(&mut h, 121);
    wheel(&mut h, -2.0);
    assert_eq!(zoom(&h), 85);
}

/// One notch in `unit`: a line event or its equivalent in points (egui's 40-point line speed).
fn turn_in_unit(h: &mut Harness, unit: MouseWheelUnit, delta: egui::Vec2, frames: usize) -> Vec<u16> {
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::COMMAND));
    h.input_mut().events.push(wheel_event(unit, MouseWheelSource::Wheel, TouchPhase::Move, delta, Modifiers::COMMAND));
    let seen = (0..frames)
        .map(|_| {
            h.step();
            zoom(h)
        })
        .collect();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    seen
}

/// A trackpad/momentum gesture: the starting phase carries no delta, the move does.
fn gesture(h: &mut Harness, source: MouseWheelSource, delta: f32, frames: usize) -> Vec<u16> {
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::COMMAND));
    h.input_mut().events.push(wheel_event(MouseWheelUnit::Point, source, TouchPhase::Start, vec2(0.0, 0.0), Modifiers::COMMAND));
    h.input_mut().events.push(wheel_event(MouseWheelUnit::Point, source, TouchPhase::Move, vec2(0.0, delta), Modifiers::COMMAND));
    let seen = (0..frames)
        .map(|_| {
            h.step();
            zoom(h)
        })
        .collect();
    h.run_steps(2);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    seen
}

#[test]
/// A 40-point notch follows the same monotonic ladder and reaches 120%.
fn point_units_use_the_same_zoom_ladder_as_lines() {
    let mut h = animated();
    // egui converts a line to 40 points (native); the same physical notch must reach 120%.
    let seen = turn_in_unit(&mut h, MouseWheelUnit::Point, vec2(0.0, 40.0), 40);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert_eq!(seen.last(), Some(&120), "{seen:?}");
}

#[test]
/// Trackpad and momentum sources follow the same monotonic ladder to 120%.
fn trackpad_and_momentum_sources_keep_the_notch_ladder() {
    for source in [MouseWheelSource::Trackpad, MouseWheelSource::Momentum] {
        let mut h = animated();
        let seen = gesture(&mut h, source, 40.0, 40);
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{source:?}: {seen:?}");
        assert_eq!(seen.last(), Some(&120), "{source:?}: {seen:?}");
    }
}

#[test]
/// A missing touch End is released by the idle timeout, so the next gesture still reaches 140%.
fn a_trackpad_gesture_without_an_end_still_steps_on_after_the_idle_timeout() {
    let mut h = animated();
    let first = gesture(&mut h, MouseWheelSource::Trackpad, 40.0, 40);
    assert_eq!(first.last(), Some(&120), "{first:?}");
    // No `TouchPhase::End` arrives; egui's scroll-action timeout must still release the gesture.
    h.run_steps(60);
    let second = gesture(&mut h, MouseWheelSource::Trackpad, 40.0, 40);
    assert_eq!(second.last(), Some(&140), "a stale trackpad gesture must not swallow the next notch: {second:?}");
}

#[test]
/// A page-unit scroll moves roughly a viewport, while 40 points scroll less than a page.
fn page_and_point_units_scroll_the_grid_by_their_converted_distance() {
    let mut h = harness();
    let grid = h.state().grid.cells_rect.unwrap();
    let over = grid.center();
    h.input_mut().events.push(Event::PointerMoved(over));
    h.input_mut().events.push(wheel_event(MouseWheelUnit::Page, MouseWheelSource::Wheel, TouchPhase::Move, vec2(0.0, -1.0), Modifiers::NONE));
    h.run_steps(60);
    let page = h.state().view().scroll.y;
    assert!(page > 100.0, "one page down scrolls roughly a viewport, not zero: {page}");

    h.state_mut().view_mut().unwrap().scroll.y = 0.0;
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerMoved(over));
    h.input_mut().events.push(wheel_event(MouseWheelUnit::Point, MouseWheelSource::Trackpad, TouchPhase::Move, vec2(0.0, -40.0), Modifiers::NONE));
    h.run_steps(60);
    let point = h.state().view().scroll.y;
    assert!(point > 0.0 && point < page, "40 points scroll less than a page: point {point}, page {page}");
}
