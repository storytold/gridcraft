//! Trackpad gestures are held to the axis they start along: scrolling down a column doesn't drift
//! sideways, scrolling along a row doesn't drift up or down, and a diagonal swipe moves freely.
//! Only gestures the trackpad reports with a start and an end are steered; scrolling without
//! gesture phases (the web, mouse wheels) passes untouched.

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, Vec2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// What egui says sent the event: a trackpad's gestures as macOS reports them, and the source
/// the other platforms report (Wayland sends the phases without one). egui keeps a gesture whose
/// source is a trackpad for a second without an event, any other scroll action for 150 ms.
const TRACKPAD: MouseWheelSource = MouseWheelSource::Trackpad;
const UNKNOWN: MouseWheelSource = MouseWheelSource::Unknown;
/// What egui-winit names a macOS glide after the fingers lift, and a mouse wheel's notches.
const MOMENTUM: MouseWheelSource = MouseWheelSource::Momentum;
const WHEEL: MouseWheelSource = MouseWheelSource::Wheel;

fn harness() -> Harness {
    harness_at(1.0 / 60.0)
}

/// A harness whose frames are `dt` seconds apart (a slow one stands in for a heavy workbook).
fn harness_at(dt: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(dt).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    let over_grid = h.state().grid.rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(over_grid));
    h.run_steps(2);
    h
}

fn scroll(h: &Harness) -> Vec2 {
    h.state().view().scroll
}

/// One frame with a wheel event of a trackpad gesture.
fn wheel(h: &mut Harness, phase: TouchPhase, delta: Vec2) {
    wheel_from(h, TRACKPAD, phase, delta);
}

fn wheel_from(h: &mut Harness, source: MouseWheelSource, phase: TouchPhase, delta: Vec2) {
    h.input_mut().events.push(Event::MouseWheel { source, unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE });
    h.step();
}

/// A trackpad gesture: fingers down, `frames` frames moving `delta` each, fingers up. Negative
/// deltas scroll down and right.
fn swipe(h: &mut Harness, delta: Vec2, frames: usize) {
    swipe_from(h, TRACKPAD, delta, frames);
}

/// A gesture as `swipe` makes one, from `source`.
fn swipe_from(h: &mut Harness, source: MouseWheelSource, delta: Vec2, frames: usize) {
    wheel_from(h, source, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..frames {
        wheel_from(h, source, TouchPhase::Move, delta);
    }
    wheel_from(h, source, TouchPhase::End, Vec2::ZERO);
}

#[test]
fn a_mostly_vertical_swipe_does_not_drift_sideways() {
    let mut h = harness();
    let before = scroll(&h);
    // About 7° off vertical, as fingers drift.
    swipe(&mut h, vec2(-1.5, -12.0), 12);
    // The glide after the fingers lift arrives as a gesture of its own (macOS reports it so) and
    // keeps to the same axis.
    swipe(&mut h, vec2(-1.0, -6.0), 6);
    let after = scroll(&h);
    assert!(after.y > before.y + 100.0, "scrolled down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "no sideways drift: {before:?} -> {after:?}");
}

#[test]
fn a_mostly_horizontal_swipe_does_not_drift_up_or_down() {
    let mut h = harness();
    let before = scroll(&h);
    swipe(&mut h, vec2(-12.0, -1.5), 12);
    swipe(&mut h, vec2(-6.0, -1.0), 6);
    let after = scroll(&h);
    assert!(after.x > before.x + 100.0, "scrolled right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "no vertical drift: {before:?} -> {after:?}");
}

#[test]
fn a_diagonal_swipe_moves_both_ways() {
    let mut h = harness();
    let before = scroll(&h);
    swipe(&mut h, vec2(-8.0, -8.0), 12);
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0 && after.y > before.y + 50.0, "{before:?} -> {after:?}");
}

#[test]
fn a_short_diagonal_swipe_keeps_both_directions() {
    // Before a swipe has gone far enough to pick an axis, a diagonal movement moves both ways.
    let mut h = harness();
    let before = scroll(&h);
    swipe(&mut h, vec2(-3.0, -3.0), 1);
    let after = scroll(&h);
    assert!(after.x > before.x && after.y > before.y, "{before:?} -> {after:?}");
}

#[test]
fn each_gesture_picks_its_own_axis() {
    // Gestures in a row, each with its start and end: a horizontal one after a vertical one is
    // held to its own axis, and a diagonal one after that moves both ways.
    let mut h = harness();
    swipe(&mut h, vec2(-1.0, -12.0), 10);
    let before = scroll(&h);
    swipe(&mut h, vec2(-12.0, -1.0), 10);
    let after = scroll(&h);
    assert!(after.x > before.x + 100.0, "the second gesture goes right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and only right: {before:?} -> {after:?}");
    let before = scroll(&h);
    swipe(&mut h, vec2(-8.0, -8.0), 10);
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0 && after.x > before.x + 50.0, "a diagonal gesture moves both ways: {before:?} -> {after:?}");
}

#[test]
fn a_glide_reported_as_its_own_gesture_picks_its_own_axis() {
    // macOS reports the momentum after the fingers lift as a gesture of its own (start, moves,
    // end, with the source egui-winit calls Momentum), which the rail treats like fingers touching
    // down again. So a glide is a swipe of its own: one that keeps the swipe's direction stays on
    // its axis, a diagonal one moves freely.
    let mut h = harness();
    swipe(&mut h, vec2(-1.5, -12.0), 12);
    let before = scroll(&h);
    swipe_from(&mut h, MOMENTUM, vec2(-1.0, -6.0), 6);
    let after = scroll(&h);
    assert!(after.y > before.y + 30.0, "the glide scrolls on: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "a glide along the swipe stays on its axis: {before:?} -> {after:?}");
    h.run_steps(20);
    swipe(&mut h, vec2(-1.5, -12.0), 12);
    let before = scroll(&h);
    swipe_from(&mut h, MOMENTUM, vec2(-5.0, -8.0), 6);
    let after = scroll(&h);
    assert!(after.x > before.x && after.y > before.y, "a diagonal glide moves both ways: {before:?} -> {after:?}");
}

#[test]
fn a_swipe_starting_mid_frame_decides_its_axis_from_its_own_movement() {
    // One frame can hold the end of one gesture and the start of the next (a slow frame on a
    // heavy workbook, or a quick lift and touch): the earlier gesture's last movement must not
    // decide the new gesture's axis. Here the new gesture is horizontal with a little drift; counted
    // with the vertical movement before its start it would come out diagonal and the drift would pass.
    let mut h = harness();
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    let before = scroll(&h);
    for (phase, delta) in
        [(TouchPhase::Move, vec2(-1.5, -12.0)), (TouchPhase::End, Vec2::ZERO), (TouchPhase::Start, Vec2::ZERO), (TouchPhase::Move, vec2(-12.0, -1.5))]
    {
        h.input_mut().events.push(Event::MouseWheel { source: TRACKPAD, unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE });
    }
    h.step();
    for _ in 0..8 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "scrolled right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "the earlier gesture's movement does not steer the new one: {before:?} -> {after:?}");
}

#[test]
fn a_gesture_keeps_its_axis_through_a_cross_frame_and_a_rest() {
    // With gesture phases the trackpad says when a swipe ends: a frame across the axis inside
    // the gesture is drift, not a new swipe, and so is one after the fingers rested a while.
    let mut h = harness();
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::Move, vec2(-12.0, -2.0));
    for _ in 0..3 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    // Fingers resting on the trackpad for a third of a second, well within egui's one-second
    // trackpad pause, then a drift frame first thing.
    h.run_steps(20);
    wheel(&mut h, TouchPhase::Move, vec2(-12.0, -2.0));
    for _ in 0..3 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 100.0, "scrolled down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "no sideways jump inside the gesture: {before:?} -> {after:?}");
}

#[test]
fn a_gesture_keeps_its_axis_when_it_wanders_later() {
    // The axis is decided from the gesture's first points and kept: fingers that start down a
    // column and wander diagonally later are still reading down the column.
    let mut h = harness();
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    for _ in 0..10 {
        wheel(&mut h, TouchPhase::Move, vec2(-8.0, -8.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 100.0, "scrolled down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "held to the axis it started along: {before:?} -> {after:?}");
}

#[test]
fn a_gestures_end_and_the_next_start_in_one_frame_leave_a_gesture_in_progress() {
    // Fingers lifting and the glide starting can share a frame (macOS reports the glide as a
    // gesture of its own); applied in order, the start wins, so the glide is a gesture: a drift
    // frame inside it doesn't start a new swipe.
    let mut h = harness();
    swipe(&mut h, vec2(-1.5, -12.0), 6);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::End,
        modifiers: Modifiers::NONE,
    });
    h.input_mut().events.push(Event::MouseWheel {
        source: MOMENTUM,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for _ in 0..6 {
        wheel_from(&mut h, MOMENTUM, TouchPhase::Move, vec2(-1.0, -6.0));
    }
    let before = scroll(&h);
    wheel_from(&mut h, MOMENTUM, TouchPhase::Move, vec2(-12.0, -2.0));
    for _ in 0..3 {
        wheel_from(&mut h, MOMENTUM, TouchPhase::Move, vec2(-1.0, -6.0));
    }
    wheel_from(&mut h, MOMENTUM, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 20.0, "the glide scrolls on: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "held to its axis through the drift: {before:?} -> {after:?}");
}

#[test]
fn a_gesture_held_in_one_frame_still_ends() {
    // A slow frame can hold a whole gesture, start to end (a brief two-finger touch): the phases
    // apply in order, so the gesture is over and what follows without phases passes untouched.
    let mut h = harness();
    for (phase, delta) in [(TouchPhase::Start, Vec2::ZERO), (TouchPhase::Move, vec2(-1.5, -12.0)), (TouchPhase::End, Vec2::ZERO)] {
        h.input_mut().events.push(Event::MouseWheel { source: TRACKPAD, unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE });
    }
    h.step();
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.0, -12.0));
    }
    let before = scroll(&h);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.0));
    }
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "the turn goes right: {before:?} -> {after:?}");
}

#[test]
fn a_large_step_inside_a_gesture_decides_its_axis_like_any_movement() {
    // A step lying on an axis, however large (a fast flick), is the gesture's own: it decides
    // the axis like any movement.
    let mut h = harness();
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    wheel(&mut h, TouchPhase::Move, vec2(0.0, -60.0));
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 100.0, "scrolls down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "held to its axis: {before:?} -> {after:?}");
}

#[test]
fn shift_turns_a_vertical_wheel_sideways() {
    // Off the Mac, Shift+wheel arrives as vertical movement and egui turns it horizontal. The
    // events carry no phases, so the rail never acts on them; this checks it stays that way.
    let mut h = harness();
    let before = scroll(&h);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    for _ in 0..3 {
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, -2.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::SHIFT,
        });
        h.run_steps(4);
    }
    h.run_steps(30);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "Shift+wheel scrolls right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and not down: {before:?} -> {after:?}");
}

#[test]
fn releasing_shift_mid_swipe_keeps_scrolling() {
    // egui keeps a gesture's modifiers after they're released (more of the gesture may arrive) and
    // turns its whole movement, drift included, onto one axis. The rail must pass that as it is,
    // not decide an axis from the raw movement and steer egui's turned scroll by it. The gesture
    // here goes sideways with a little vertical drift, which egui folds into the horizontal
    // scroll; a rail deciding Horizontal on its own would leave the drift's share scrolling up.
    let mut h = harness();
    // Away from the top, so an upward leak would show.
    swipe(&mut h, vec2(0.0, -12.0), 10);
    let before = scroll(&h);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..5 {
        h.input_mut().events.push(Event::MouseWheel {
            source: TRACKPAD,
            unit: MouseWheelUnit::Point,
            delta: vec2(-12.0, -1.5),
            phase: TouchPhase::Move,
            modifiers: Modifiers::SHIFT,
        });
        h.step();
    }
    let midway = scroll(&h);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    for _ in 0..5 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    let rested = scroll(&h);
    // Fingers resting mid-gesture for a third of a second, within egui's one-second trackpad
    // pause: the gesture keeps its modifiers (egui does), so it keeps scrolling right afterwards too.
    h.run_steps(20);
    for _ in 0..5 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(midway.x > before.x + 50.0, "Shift+gesture scrolls right: {before:?} -> {midway:?}");
    assert!(rested.x > midway.x + 50.0, "and keeps scrolling right after Shift is released: {midway:?} -> {rested:?}");
    assert!(after.x > rested.x + 50.0, "and after a rest: {rested:?} -> {after:?}");
    assert_eq!(after.y, before.y, "never up or down: {before:?} -> {after:?}");
}

#[test]
fn shift_held_at_a_gestures_start_and_released_before_it_moves_holds_the_gesture() {
    // egui keeps the modifiers a gesture starts with, so the whole gesture scrolls horizontally
    // even when Shift is released before the first move; the rail takes them from the start too.
    let mut h = harness();
    swipe(&mut h, vec2(0.0, -12.0), 10);
    let before = scroll(&h);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::SHIFT,
    });
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    for _ in 0..10 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "scrolls right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and not up or down: {before:?} -> {after:?}");
}

#[test]
fn touching_the_trackpad_does_not_swallow_a_wheel_notch() {
    // A mouse-wheel notch is smoothed out over a few frames; fingers touching the trackpad right
    // after it start a swipe, but the notch is not that swipe's to cut.
    let notch_then = |touch: bool| {
        let mut h = harness();
        let before = scroll(&h);
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, -3.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.step();
        if touch {
            wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
        }
        h.run_steps(60);
        scroll(&h).y - before.y
    };
    let (alone, touched) = (notch_then(false), notch_then(true));
    assert!(alone > 100.0, "the notch scrolls: {alone}");
    assert_eq!(touched, alone, "the whole notch scrolls with fingers touching down after it");
}

#[test]
fn a_wheel_notch_in_the_frame_a_gesture_starts_does_not_free_the_gesture() {
    // A trackpad touch and then a sideways mouse-wheel notch in lines in one frame: the notch passes
    // whole, which takes a moment of smoothing, and isn't the gesture's own movement, so the
    // vertical gesture that starts is held to its axis once the notch is through (counted as the
    // gesture's, the notch would decide it sideways or free).
    let mut h = harness();
    let start = scroll(&h);
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::NONE,
    });
    h.input_mut().events.push(Event::MouseWheel {
        source: WHEEL,
        unit: MouseWheelUnit::Line,
        delta: vec2(-6.0, 0.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for _ in 0..12 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    h.run_steps(30);
    let settled = scroll(&h);
    assert!(settled.x > start.x, "the notch scrolled sideways: {start:?} -> {settled:?}");
    for _ in 0..8 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > settled.y + 50.0, "the gesture scrolls down: {settled:?} -> {after:?}");
    assert_eq!(after.x, settled.x, "and is held to its axis once the notch is through: {settled:?} -> {after:?}");
}

#[test]
fn a_wheel_notch_during_a_gesture_scrolls_its_own_way() {
    // A mouse-wheel tilt in lines while a vertical trackpad gesture is going (a mouse and a trackpad
    // side by side): the notch isn't the gesture's and scrolls sideways, as far as the same notch
    // alone, while the gesture stays held to its axis.
    let notch = |during_gesture: bool| {
        let mut h = harness();
        if during_gesture {
            wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
            for _ in 0..6 {
                wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
            }
        }
        let before = scroll(&h);
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Line,
            delta: vec2(-3.0, 0.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.step();
        if during_gesture {
            for _ in 0..6 {
                wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
            }
            wheel(&mut h, TouchPhase::End, Vec2::ZERO);
        } else {
            h.run_steps(7);
        }
        h.run_steps(60);
        (scroll(&h) - before).x
    };
    let (alone, during) = (notch(false), notch(true));
    assert!(alone > 100.0, "the notch alone: {alone}");
    assert!((during - alone).abs() <= alone * 0.02, "the tilt notch scrolls sideways as far during a gesture: {during} vs {alone}");
}

#[test]
fn a_slow_frame_does_not_swallow_a_wheel_notch() {
    // A mouse-wheel notch in lines, smoothed by egui over frames longer than usual (a heavy
    // workbook): without gesture phases the rail leaves it alone, so it scrolls the same distance
    // as at 60 fps.
    let notch = |h: &mut Harness, frames: usize, delta: Vec2| {
        let before = scroll(h);
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Line,
            delta,
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(frames);
        scroll(h) - before
    };
    // A vertical notch and a horizontal one (a tilt wheel, or a wheel with Shift held off the
    // Mac, which egui has already turned sideways by the time the rail sees it).
    for delta in [vec2(0.0, -3.0), vec2(-3.0, 0.0)] {
        let fast = notch(&mut harness(), 60, delta);
        let slow = notch(&mut harness_at(0.3), 4, delta);
        assert!(fast.length() > 50.0, "a notch scrolls: {fast:?}");
        assert!((slow - fast).length() < 1.0, "the same notch {delta:?} over slow frames: {slow:?} vs {fast:?}");
    }
}

#[test]
fn without_gesture_phases_scrolling_is_untouched() {
    // Where no gesture phases arrive (the web, mouse wheels) nothing says where a swipe begins or
    // ends, so the rail leaves the scroll as it is: the small off-axis part of each movement
    // passes, as it does on main.
    let mut h = harness();
    let before = scroll(&h);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0 && after.x > before.x, "both components pass: {before:?} -> {after:?}");
    let before = scroll(&h);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0 && after.y > before.y, "both components pass: {before:?} -> {after:?}");
}

#[test]
fn a_mouse_wheel_in_lines_is_not_held_to_an_axis() {
    // A wheel's notches come in lines, without gesture phases: a tilt notch right after three
    // vertical ones scrolls sideways as far as the tilt alone.
    let notches = |tilt_after_vertical: bool| {
        let mut h = harness();
        let before = scroll(&h);
        let deltas: &[Vec2] =
            if tilt_after_vertical { &[vec2(0.0, -3.0), vec2(0.0, -3.0), vec2(0.0, -3.0), vec2(-3.0, 0.0)] } else { &[vec2(-3.0, 0.0)] };
        for &delta in deltas {
            h.input_mut().events.push(Event::MouseWheel {
                source: UNKNOWN,
                unit: MouseWheelUnit::Line,
                delta,
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(2);
        }
        h.run_steps(30);
        scroll(&h) - before
    };
    let (alone, after_vertical) = (notches(false), notches(true));
    assert!(alone.x > 100.0 && alone.y == 0.0, "a tilt notch alone: {alone:?}");
    assert!(after_vertical.y > 100.0, "the vertical notches scroll: {after_vertical:?}");
    assert_eq!(after_vertical.x, alone.x, "the tilt scrolls as far after them: {after_vertical:?} vs {alone:?}");
}

#[test]
fn a_mouse_wheel_reporting_points_is_not_held_to_an_axis() {
    // On the web a mouse wheel reports points with no gesture phases, like a trackpad there;
    // without phases nothing is steered, so a tilt notch right after a vertical one scrolls
    // sideways as far as the tilt alone, and the vertical notch isn't cut by it.
    let notches = |tilt: bool| {
        let mut h = harness();
        let before = scroll(&h);
        let deltas: &[Vec2] = if tilt { &[vec2(0.0, -100.0), vec2(-100.0, 0.0)] } else { &[vec2(0.0, -100.0)] };
        for &delta in deltas {
            h.input_mut().events.push(Event::MouseWheel {
                source: UNKNOWN,
                unit: MouseWheelUnit::Point,
                delta,
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(2);
        }
        h.run_steps(60);
        scroll(&h) - before
    };
    let (alone, both) = (notches(false), notches(true));
    assert!(alone.y > 100.0 && alone.x == 0.0, "a vertical notch alone: {alone:?}");
    assert!(both.x > 100.0, "the tilt scrolls sideways: {both:?}");
    assert_eq!(both.y, alone.y, "the vertical notch is not cut by the tilt: {both:?} vs {alone:?}");
}

#[test]
fn shift_carried_only_by_the_wheel_events_still_holds_the_scroll() {
    // On the web without keyboard focus no modifier change reaches egui, but the wheel events
    // carry Shift and egui turns them horizontal from that; without phases the rail leaves the
    // scroll alone, so the swipe scrolls right.
    let mut h = harness();
    let before = scroll(&h);
    for _ in 0..8 {
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -12.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::SHIFT,
        });
        h.step();
    }
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "scrolls right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and not down: {before:?} -> {after:?}");
}

#[test]
fn a_gestures_end_leaves_nothing_of_it() {
    // The trackpad reported the gesture's end: a diagonal movement without phases right after it
    // (another device) moves both ways, not along the gesture's axis.
    let mut h = harness();
    swipe(&mut h, vec2(-1.0, -12.0), 6);
    let before = scroll(&h);
    for _ in 0..8 {
        wheel(&mut h, TouchPhase::Move, vec2(-8.0, -8.0));
    }
    let after = scroll(&h);
    assert!(after.x > before.x + 30.0 && after.y > before.y + 30.0, "both ways: {before:?} -> {after:?}");
}

#[test]
fn a_gesture_right_after_a_wheel_notch_steers_only_its_own_movement() {
    // A mouse and a trackpad side by side: a tilt notch in lines, then a vertical gesture starting
    // the next frame. What egui is still smoothing of the notch isn't the gesture's and goes
    // through whole, as far as the notch alone; the gesture itself is held to its axis.
    let notch_then = |gesture: bool| {
        let mut h = harness();
        let before = scroll(&h);
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Line,
            delta: vec2(-3.0, 0.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.step();
        if gesture {
            swipe(&mut h, vec2(0.0, -12.0), 10);
        }
        h.run_steps(60);
        scroll(&h) - before
    };
    let (alone, with_gesture) = (notch_then(false), notch_then(true));
    assert!(alone.x > 100.0 && alone.y == 0.0, "the notch alone: {alone:?}");
    // egui itself drops what little of the notch is still being smoothed when the gesture ends
    // (about 1 % after ten frames); the rail passes all of it until then.
    assert!(with_gesture.x >= alone.x * 0.98, "the notch scrolls beside the gesture: {with_gesture:?} vs {alone:?}");
    assert!(with_gesture.y > 100.0, "and the gesture scrolls down: {with_gesture:?}");
}

#[test]
fn the_axis_is_decided_after_eight_points_not_on_the_first() {
    // A gesture's first point can lean the wrong way; the axis is decided once the gesture has
    // gone the deciding distance, from everything so far, and then stays decided: two points
    // down after a sideways lean settle it, and a wander the other way afterwards is held.
    let mut h = harness();
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    wheel(&mut h, TouchPhase::Move, vec2(-2.0, -1.0));
    for _ in 0..2 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.0, -12.0));
    }
    let decided = scroll(&h);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(decided.y > before.y + 15.0, "the gesture goes down despite its first point: {before:?} -> {decided:?}");
    assert!(decided.x <= before.x + 3.0, "only the first point's lean passed: {before:?} -> {decided:?}");
    assert_eq!(after.x, decided.x, "held once decided: {decided:?} -> {after:?}");
    assert!(after.y > decided.y, "and still scrolling down: {decided:?} -> {after:?}");
}

#[test]
fn a_gesture_three_times_further_along_than_across_is_held() {
    // Twice as far along as across is the boundary; a gesture at three to one (about 18° off) is
    // held, not free, and so is one at two and a half to one (about 22°), and one just past two to
    // one. One just short of two to one is free, and so is one at one and three quarters to one
    // (about 30°).
    let held = |delta: Vec2| {
        let mut h = harness();
        let before = scroll(&h);
        swipe(&mut h, delta, 10);
        let after = scroll(&h);
        assert!(after.y > before.y + 100.0, "scrolled down: {before:?} -> {after:?}");
        after.x == before.x
    };
    assert!(held(vec2(-4.0, -12.0)), "held to its axis at three to one");
    assert!(held(vec2(-4.8, -12.0)), "held to its axis at two and a half to one");
    assert!(held(vec2(-5.85, -12.0)), "held to its axis at 2.05 to one");
    assert!(!held(vec2(-6.15, -12.0)), "free at 1.95 to one");
    assert!(!held(vec2(-7.0, -12.0)), "free at one and three quarters to one");
}

#[test]
fn the_deciding_distance_is_eight_points() {
    // The axis is decided once the gesture has moved eight points, counted along both axes, and
    // kept from then on. Just short of eight (5 sideways and 6.2 down: 7.96) nothing is decided,
    // so the gesture is decided vertical only after the next move down, and a sideways move after
    // that is held; a shorter deciding distance would have decided it free at 7.96. Just past eight
    // (2 sideways and 7.8 down: 8.05) it is decided vertical there, so a sideways move right after
    // it is held; a longer deciding distance would count that move in and decide it free.
    let sideways = |h: &mut Harness, delta: Vec2| {
        let before = scroll(h);
        wheel(h, TouchPhase::Move, delta);
        scroll(h).x - before.x
    };
    let mut h = harness();
    let start = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    wheel(&mut h, TouchPhase::Move, vec2(-5.0, 0.0));
    assert!(scroll(&h).x > start.x, "the first move leans sideways and moves");
    wheel(&mut h, TouchPhase::Move, vec2(0.0, -6.2));
    wheel(&mut h, TouchPhase::Move, vec2(0.0, -4.5));
    assert_eq!(sideways(&mut h, vec2(-5.0, 0.0)), 0.0, "undecided at 7.96 points, vertical at 11.8: the sideways move is held");
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let mut h = harness();
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    wheel(&mut h, TouchPhase::Move, vec2(-2.0, 0.0));
    wheel(&mut h, TouchPhase::Move, vec2(0.0, -7.8));
    assert_eq!(sideways(&mut h, vec2(-6.0, 0.0)), 0.0, "decided vertical at 8.05 points: the sideways move is held");
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
}

#[test]
fn alt_holds_a_gesture_vertical_and_the_rail_lets_it() {
    // Alt is egui's vertical scroll modifier: it turns a gesture's whole movement vertical, drift
    // included. Like Shift, it holds the gesture, so the rail passes egui's turned scroll as it
    // is: the same distance as a plain gesture of that total movement.
    let with_alt = {
        let mut h = harness();
        let before = scroll(&h);
        h.input_mut().events.push(Event::ModifiersChanged(Modifiers::ALT));
        h.input_mut().events.push(Event::MouseWheel {
            source: TRACKPAD,
            unit: MouseWheelUnit::Point,
            delta: Vec2::ZERO,
            phase: TouchPhase::Start,
            modifiers: Modifiers::ALT,
        });
        h.step();
        for _ in 0..8 {
            h.input_mut().events.push(Event::MouseWheel {
                source: TRACKPAD,
                unit: MouseWheelUnit::Point,
                delta: vec2(-12.0, -1.5),
                phase: TouchPhase::Move,
                modifiers: Modifiers::ALT,
            });
            h.step();
        }
        wheel(&mut h, TouchPhase::End, Vec2::ZERO);
        scroll(&h) - before
    };
    let plain = {
        let mut h = harness();
        let before = scroll(&h);
        swipe(&mut h, vec2(0.0, -13.5), 8);
        scroll(&h) - before
    };
    assert!(with_alt.y > 50.0 && with_alt.x == 0.0, "Alt+gesture scrolls vertically: {with_alt:?}");
    assert_eq!(with_alt, plain, "as far as egui turned it, drift included: {with_alt:?} vs {plain:?}");
}

#[test]
fn a_start_without_an_end_before_it_starts_a_gesture_of_its_own() {
    // macOS can send a gesture's start twice (fingers resting, then moving) with no end between:
    // the Shift that held the first start must not hold the second, which is railed on its own.
    let mut h = harness();
    swipe(&mut h, vec2(0.0, -12.0), 10);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::SHIFT,
    });
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    let before = scroll(&h);
    for _ in 0..10 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0, "{before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "held to the axis: {before:?} -> {after:?}");
}

#[test]
fn a_held_gesture_ending_in_the_frame_the_next_starts_does_not_hold_the_next() {
    // The end of a Shift gesture and the start of a plain one can share a frame: the modifier on
    // the earlier gesture's moves must not hold the new gesture, which is railed on its own.
    let mut h = harness();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::SHIFT,
    });
    h.step();
    for _ in 0..4 {
        h.input_mut().events.push(Event::MouseWheel {
            source: TRACKPAD,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -12.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::SHIFT,
        });
        h.step();
    }
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    // One frame: the Shift gesture's last move and end, then the next gesture's start and move.
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -12.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::SHIFT,
    });
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::End,
        modifiers: Modifiers::SHIFT,
    });
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::NONE,
    });
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: vec2(-1.5, -12.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.step();
    let before = scroll(&h);
    for _ in 0..8 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0, "the new gesture scrolls down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "and is held to its axis, not freed by the old gesture's Shift: {before:?} -> {after:?}");
}

#[test]
fn shift_and_alt_together_pass_the_scroll_as_it_was() {
    // With both of egui's axis modifiers held egui turns nothing, and the grid's own Shift
    // handling only turns a scroll whose sideways part is zero. The rail leaves a gesture under
    // either modifier alone, so this one scrolls as it did before the rail: down, drift included.
    let mut h = harness();
    let before = scroll(&h);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT | Modifiers::ALT));
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Start,
        modifiers: Modifiers::SHIFT | Modifiers::ALT,
    });
    h.step();
    for _ in 0..8 {
        h.input_mut().events.push(Event::MouseWheel {
            source: TRACKPAD,
            unit: MouseWheelUnit::Point,
            delta: vec2(-1.5, -12.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::SHIFT | Modifiers::ALT,
        });
        h.step();
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0 && after.x > before.x, "scrolls down with its drift, as before the rail: {before:?} -> {after:?}");
}

#[test]
fn shift_on_a_wheel_notch_in_lines_holds_the_gesture_it_lands_in() {
    // egui takes a gesture's modifiers from every wheel event while it lasts, a mouse wheel's
    // lines included, and turns the gesture's movement from then on; the rail reads them there
    // too, so the turned scroll passes whole instead of being steered by the raw movement.
    let mut h = harness();
    swipe(&mut h, vec2(0.0, -12.0), 10);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..4 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.input_mut().events.push(Event::MouseWheel {
        source: UNKNOWN,
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::SHIFT,
    });
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    let turned = scroll(&h);
    for _ in 0..8 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.x > turned.x + 50.0, "keeps scrolling right: {turned:?} -> {after:?}");
    assert_eq!(after.y, turned.y, "and not up or down once egui turns the gesture: {turned:?} -> {after:?}");
}

#[test]
fn shift_on_a_move_without_movement_holds_the_gesture() {
    // egui takes a gesture's modifiers from every one of its wheel events, a move that carries no
    // movement included, and the grid doesn't scroll on such a frame at all; the rail must read
    // the modifier there too, or Shift pressed and released over a still finger would leave it
    // steering a gesture egui has turned horizontal, the drift's share scrolling up.
    let mut h = harness();
    swipe(&mut h, vec2(0.0, -12.0), 10);
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: Vec2::ZERO,
        phase: TouchPhase::Move,
        modifiers: Modifiers::SHIFT,
    });
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    for _ in 0..10 {
        wheel(&mut h, TouchPhase::Move, vec2(-12.0, -1.5));
    }
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.x > before.x + 50.0, "scrolls right: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and not up or down: {before:?} -> {after:?}");
}

#[test]
fn a_pause_that_ends_the_scroll_action_for_egui_ends_the_gesture() {
    // Where nothing says what sent the events (Wayland reports the phases, not the source) egui
    // gives a scroll action up after 150 ms without an event: it takes the modifiers afresh and
    // smooths the larger events that follow as a mouse wheel's notches. The rail ends its gesture
    // with it, so the movement after the pause passes untouched, and the next start is a gesture
    // again. (Scrolled right first: the view clamps at the left edge, where a sideways jump that
    // way would not show.)
    let mut h = harness();
    swipe(&mut h, vec2(-12.0, -1.5), 10);
    let before = scroll(&h);
    wheel_from(&mut h, UNKNOWN, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel_from(&mut h, UNKNOWN, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    assert!(h.ctx.input(|i| i.is_scrolling()), "egui is in the gesture");
    h.run_steps(12);
    assert!(!h.ctx.input(|i| i.is_scrolling()), "egui gave the scroll action up after 200 ms without an event");
    let rested = scroll(&h);
    assert_eq!(rested.x, before.x, "held to its axis until then: {before:?} -> {rested:?}");
    wheel_from(&mut h, UNKNOWN, TouchPhase::Move, vec2(-12.0, -2.0));
    h.run_steps(60);
    let after = scroll(&h);
    assert!(after.x > rested.x + 10.0, "the movement after the pause passes untouched: {rested:?} -> {after:?}");
    wheel_from(&mut h, UNKNOWN, TouchPhase::End, Vec2::ZERO);
    wheel_from(&mut h, UNKNOWN, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel_from(&mut h, UNKNOWN, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel_from(&mut h, UNKNOWN, TouchPhase::End, Vec2::ZERO);
    let next = scroll(&h);
    assert!(next.y > after.y + 50.0, "the next gesture scrolls down: {after:?} -> {next:?}");
    assert_eq!(next.x, after.x, "and is held to its axis: {after:?} -> {next:?}");
}

#[test]
fn fingers_resting_on_a_trackpad_keep_the_gesture_for_a_second() {
    // Where the source is known to be a trackpad (macOS) fingers resting mid-gesture send
    // nothing, and egui keeps the scroll action for a second before giving it up: a drift frame
    // after half a second's rest is still held to the axis, one after more than a second passes.
    // (Scrolled right first, away from the left edge the view clamps at.)
    let mut h = harness();
    swipe(&mut h, vec2(-12.0, -1.5), 10);
    let before = scroll(&h);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    h.run_steps(30);
    assert!(h.ctx.input(|i| i.is_scrolling()), "egui keeps the gesture while the fingers rest");
    wheel(&mut h, TouchPhase::Move, vec2(-12.0, -2.0));
    h.run_steps(10);
    let held = scroll(&h);
    assert!(held.y > before.y + 50.0, "scrolled down: {before:?} -> {held:?}");
    assert_eq!(held.x, before.x, "held to the axis after half a second's rest: {before:?} -> {held:?}");
    h.run_steps(66);
    assert!(!h.ctx.input(|i| i.is_scrolling()), "egui gave the scroll action up after more than a second");
    wheel(&mut h, TouchPhase::Move, vec2(-12.0, -2.0));
    h.run_steps(60);
    let after = scroll(&h);
    assert!(after.x > held.x + 10.0, "after more than a second the movement passes untouched: {held:?} -> {after:?}");
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
}

#[test]
fn a_start_that_carries_movement_moves_as_egui_moves_it() {
    // Wayland's first event of a finger gesture is its start and carries movement, which egui
    // doesn't apply. The rail counts only the moves after the start, so the start's sideways part
    // neither moves the view nor decides the axis. (Scrolled right first, away from the left edge.)
    let mut h = harness();
    swipe(&mut h, vec2(-12.0, -1.5), 10);
    let before = scroll(&h);
    for phase in [TouchPhase::Start, TouchPhase::Move] {
        h.input_mut().events.push(Event::MouseWheel {
            source: UNKNOWN,
            unit: MouseWheelUnit::Point,
            delta: vec2(-1.5, -12.0),
            phase,
            modifiers: Modifiers::NONE,
        });
    }
    h.step();
    for _ in 0..8 {
        wheel_from(&mut h, UNKNOWN, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    wheel_from(&mut h, UNKNOWN, TouchPhase::End, Vec2::ZERO);
    let after = scroll(&h);
    assert!(after.y > before.y + 50.0, "scrolled down: {before:?} -> {after:?}");
    assert_eq!(after.x, before.x, "held to its axis, with nothing moved back sideways: {before:?} -> {after:?}");
}

#[test]
fn the_pause_follows_the_source_of_the_last_event() {
    // egui keeps a scroll action for a second when its last event came from a trackpad, and for
    // 150 ms otherwise. A mouse-wheel notch during a trackpad gesture makes the last event a
    // wheel's, so 0.4 s later egui has given the action up and the rail lets the next movement
    // through. (Scrolled right first, away from the left edge.)
    let mut h = harness();
    swipe(&mut h, vec2(-12.0, -1.5), 10);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    h.input_mut().events.push(Event::MouseWheel {
        source: WHEEL,
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.run_steps(24);
    assert!(!h.ctx.input(|i| i.is_scrolling()), "egui gave the scroll action up 0.4 s after the wheel's notch");
    let rested = scroll(&h);
    wheel(&mut h, TouchPhase::Move, vec2(-12.0, -2.0));
    h.run_steps(60);
    let after = scroll(&h);
    assert!(after.x > rested.x + 10.0, "the movement after the pause passes untouched: {rested:?} -> {after:?}");
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
}

#[test]
fn a_pause_without_frames_ends_the_gesture_before_the_next_event() {
    // The app draws no frames while the fingers rest, so the first frame after a pause is the one
    // carrying the next event. The pause is checked before that frame's events, as egui checks
    // it, so the movement it carries passes untouched rather than being steered across the axis.
    // Unknown sources give the action up after 150 ms, a trackpad after a second. (Scrolled right
    // first, away from the left edge.)
    for (source, gap) in [(UNKNOWN, 0.3), (TRACKPAD, 1.5)] {
        let mut h = harness();
        swipe(&mut h, vec2(-12.0, -1.5), 10);
        wheel_from(&mut h, source, TouchPhase::Start, Vec2::ZERO);
        for _ in 0..6 {
            wheel_from(&mut h, source, TouchPhase::Move, vec2(-1.5, -12.0));
        }
        let rested = scroll(&h);
        let now = h.ctx.input(|i| i.time);
        h.input_mut().time = Some(now + gap);
        wheel_from(&mut h, source, TouchPhase::Move, vec2(-12.0, -2.0));
        h.run_steps(60);
        let after = scroll(&h);
        assert!(after.x > rested.x + 10.0, "{source:?}, {gap} s without a frame: the movement after it passes: {rested:?} -> {after:?}");
    }
}

#[test]
fn shift_pressed_after_a_held_move_turns_that_frame_sideways() {
    // A frame holding a gesture's move and then Shift pressed: egui takes Shift for the gesture
    // only from its next event, so the rail holds that move vertical, and the grid's own Shift
    // handling turns a scroll with no sideways part sideways. On main the move went down for that
    // one frame; from the next frame on both scroll sideways.
    let mut h = harness();
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..4 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    h.run_steps(10);
    let before = scroll(&h);
    h.input_mut().events.push(Event::MouseWheel {
        source: TRACKPAD,
        unit: MouseWheelUnit::Point,
        delta: vec2(-1.5, -12.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    h.step();
    let after = scroll(&h);
    assert!(after.x > before.x, "turned sideways: {before:?} -> {after:?}");
    assert_eq!(after.y, before.y, "and not down: {before:?} -> {after:?}");
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
}

#[test]
fn a_gestures_end_with_the_pointer_off_the_grid_still_ends_it() {
    // The rail reads the phases and modifiers on every frame, whether or not the pointer is over
    // the grid: a gesture that ends after the pointer has left still ends, so the phase-less
    // scrolling after it moves both ways. (Scrolled right first, away from the left edge.)
    let mut h = harness();
    swipe(&mut h, vec2(-12.0, -1.5), 10);
    wheel(&mut h, TouchPhase::Start, Vec2::ZERO);
    for _ in 0..6 {
        wheel(&mut h, TouchPhase::Move, vec2(-1.5, -12.0));
    }
    let over_grid = h.state().grid.rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(egui::pos2(5.0, 5.0)));
    h.step();
    wheel(&mut h, TouchPhase::End, Vec2::ZERO);
    h.input_mut().events.push(Event::PointerMoved(over_grid));
    h.run_steps(30);
    let before = scroll(&h);
    for _ in 0..8 {
        wheel_from(&mut h, UNKNOWN, TouchPhase::Move, vec2(-8.0, -8.0));
    }
    h.run_steps(30);
    let after = scroll(&h);
    assert!(after.x > before.x && after.y > before.y, "both ways: {before:?} -> {after:?}");
}

#[test]
fn a_rest_just_short_of_the_pause_keeps_the_gesture() {
    // A gesture is kept through a rest just short of the pause that ends it for egui, and ends with it just past: 133 ms between
    // events where nothing names the source (150 ms ends it), 950 ms for a trackpad (a second ends it). (Scrolled
    // right first, away from the left edge.)
    for (source, frames) in [(UNKNOWN, 7), (TRACKPAD, 56)] {
        let mut h = harness();
        swipe(&mut h, vec2(-12.0, -1.5), 10);
        let before = scroll(&h);
        wheel_from(&mut h, source, TouchPhase::Start, Vec2::ZERO);
        for _ in 0..6 {
            wheel_from(&mut h, source, TouchPhase::Move, vec2(-1.5, -12.0));
        }
        h.run_steps(frames);
        assert!(h.ctx.input(|i| i.is_scrolling()), "{source:?}: egui still has the gesture after {frames} frames");
        wheel_from(&mut h, source, TouchPhase::Move, vec2(-12.0, -2.0));
        h.run_steps(10);
        let held = scroll(&h);
        assert_eq!(held.x, before.x, "{source:?}: held to the axis after {frames} frames' rest: {before:?} -> {held:?}");
        wheel_from(&mut h, source, TouchPhase::End, Vec2::ZERO);
    }
    // Just past the pause, egui has given the gesture up, and so has the rail: the next move isn't
    // held to the axis any more.
    for (source, frames) in [(UNKNOWN, 9), (TRACKPAD, 61)] {
        let mut h = harness();
        swipe(&mut h, vec2(-12.0, -1.5), 10);
        let before = scroll(&h);
        wheel_from(&mut h, source, TouchPhase::Start, Vec2::ZERO);
        for _ in 0..6 {
            wheel_from(&mut h, source, TouchPhase::Move, vec2(-1.5, -12.0));
        }
        h.run_steps(frames);
        assert!(!h.ctx.input(|i| i.is_scrolling()), "{source:?}: egui has given the gesture up after {frames} frames");
        wheel_from(&mut h, source, TouchPhase::Move, vec2(-12.0, -2.0));
        h.run_steps(10);
        let free = scroll(&h);
        // The move's own 12 points sideways reach the view (a held move would leave only a fraction
        // of a point there from egui's smoothing).
        assert!((free.x - before.x).abs() > 10.0, "{source:?}: no longer held after {frames} frames' rest: {before:?} -> {free:?}");
    }
}
