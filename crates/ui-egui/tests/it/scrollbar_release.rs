//! Scrollbar capture must not outlive the pointer gesture that started it.

use crate::common::Harness;
use egui::{Event, Modifiers, PointerButton, Pos2, Vec2, pos2, vec2};
use gridcraft_engine::core::CellRef;

#[derive(Clone, Copy, Debug)]
enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    fn start(self, h: &Harness) -> Pos2 {
        match self {
            Self::Vertical => pos2(h.state().grid.rect.unwrap().right() + 7.0, h.state().grid.cells_rect.unwrap().top() + 8.0),
            Self::Horizontal => {
                let track = h.state().grid.hscroll.unwrap();
                pos2(track.left() + 6.0, track.center().y)
            }
        }
    }

    fn movement(self) -> Vec2 {
        match self {
            Self::Vertical => vec2(0.0, 40.0),
            Self::Horizontal => vec2(40.0, 0.0),
        }
    }

    fn outside_track(self, p: Pos2) -> Pos2 {
        match self {
            Self::Vertical => pos2(p.x - 150.0, p.y),
            Self::Horizontal => pos2(p.x, p.y - 150.0),
        }
    }

    fn scroll(self, h: &Harness) -> f32 {
        match self {
            Self::Vertical => h.state().view().scroll.y,
            Self::Horizontal => h.state().view().scroll.x,
        }
    }
}

fn events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn begin_drag(h: &mut Harness, axis: Axis) -> Pos2 {
    let start = axis.start(h);
    events(h, [Event::PointerMoved(start), button(start, true)]);
    let p = start + axis.movement();
    events(h, [Event::PointerMoved(p)]);
    assert!(axis.scroll(h) > 0.0, "{axis:?}: the thumb drag started");
    p
}

fn dragging(axis: Axis) -> (Harness, Pos2) {
    let mut h = crate::support::blank_harness(4);
    let p = begin_drag(&mut h, axis);
    (h, p)
}

#[test]
fn releasing_outside_the_window_ends_both_scrollbar_drags() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        let outside = match axis {
            Axis::Vertical => pos2(1250.0, p.y),
            Axis::Horizontal => pos2(p.x, 850.0),
        };
        events(&mut h, [Event::PointerMoved(outside), button(outside, false)]);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(outside + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: mouse movement after release must not scroll");
    }
}

#[test]
fn a_held_drag_continues_outside_the_track() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        let before = axis.scroll(&h);
        let outside = axis.outside_track(p) + axis.movement();
        events(&mut h, [Event::PointerMoved(outside)]);
        assert!(axis.scroll(&h) > before, "{axis:?}: leaving the track while held must keep dragging");
        events(&mut h, [button(outside, false)]);
        assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
    }
}

#[test]
fn losing_focus_ends_both_scrollbar_drags_without_a_release_event() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        events(&mut h, [Event::WindowFocused(false)]);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: focus loss must release scrollbar capture");
    }
}

#[test]
fn a_release_after_pointer_gone_ends_both_scrollbar_drags() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        events(&mut h, [Event::PointerGone]);
        events(&mut h, [button(p, false)]);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: returning with the button up must not scroll");
    }
}

#[test]
fn the_wayland_fallback_does_not_leave_either_scrollbar_captured_after_a_lost_release() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        h.state_mut().services.scrollbar_release_on_pointer_gone = true;
        // egui-winit can discard MouseInput after CursorLeft clears its pointer position.
        events(&mut h, [Event::PointerGone]);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        events(&mut h, [Event::PointerMoved(p + axis.movement() * 2.0)]);
        assert!(h.ctx.input(|i| i.pointer.primary_down()), "the bridge's missing release leaves the reported button down");
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: a missing outside release must not leave the thumb captured");
    }
}

#[test]
fn focus_loss_cancels_capture_even_when_the_window_does_not_paint() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        let ctx = h.ctx.clone();
        let hidden = egui::RawInput { focused: false, events: vec![Event::WindowFocused(false)], ..Default::default() };
        assert!(ctx.dragged_id().is_some());
        // eframe's hidden tick updates raw.focused without interpreting the UI events.
        let _ = ctx.run_logic(&hidden, |ctx| h.state_mut().logic(ctx));
        assert!(ctx.dragged_id().is_none(), "{axis:?}: the logic-only tick must release capture immediately");
        let stopped = axis.scroll(&h);
        // eframe retains the unconsumed events for the next full pass.
        h.input_mut().events.extend(hidden.events);
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: logic-only focus loss must release capture");
    }
}

#[test]
fn hosts_with_pointer_capture_keep_a_held_drag_across_window_exit() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        let before = axis.scroll(&h);
        events(&mut h, [Event::PointerGone]);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        events(&mut h, [Event::PointerMoved(p + axis.movement() * 2.0)]);
        assert!(axis.scroll(&h) > before, "{axis:?}: a held drag must survive window exit on capture-capable hosts");
        events(&mut h, [button(p + axis.movement() * 2.0, false)]);
    }
}

#[test]
fn focus_loss_also_cancels_a_press_before_it_becomes_a_drag() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        let p = axis.start(&h);
        events(&mut h, [Event::PointerMoved(p), button(p, true)]);
        events(&mut h, [Event::WindowFocused(false)]);
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: an interrupted press must not start dragging later");
    }
}

#[test]
fn a_new_press_after_pointer_loss_in_the_same_frame_still_starts_a_drag() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        h.state_mut().services.scrollbar_release_on_pointer_gone = true;
        let p = axis.start(&h);
        events(&mut h, [Event::PointerGone, Event::PointerMoved(p), button(p, true)]);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        assert!(axis.scroll(&h) > 0.0, "{axis:?}: an earlier pointer exit must not cancel a new press");
        events(&mut h, [button(p + axis.movement(), false)]);
    }
}

fn interrupted_capture_cannot_steal_a_new_press(interruption: &[Event]) {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging(axis);
        h.state_mut().services.scrollbar_release_on_pointer_gone = true;
        let next_press = axis.outside_track(p);
        let mut batch = interruption.to_vec();
        batch.extend([Event::PointerMoved(next_press), button(next_press, true)]);
        events(&mut h, batch);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(next_press + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: the old capture must not steal a new press after interruption");
    }
}

#[test]
fn pointer_loss_ends_the_old_capture_before_a_new_press_in_the_same_frame() {
    interrupted_capture_cannot_steal_a_new_press(&[Event::PointerGone]);
}

#[test]
fn focus_loss_ends_the_old_capture_before_a_new_press_in_the_same_frame() {
    interrupted_capture_cannot_steal_a_new_press(&[Event::WindowFocused(false), Event::WindowFocused(true)]);
}
