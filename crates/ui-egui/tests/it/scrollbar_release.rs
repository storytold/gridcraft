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

#[test]
fn activating_scrollbar_presses_survive_initial_canvas_focus_ordering() {
    for (track_click, poll_focus) in [(false, false), (true, false), (false, true), (true, true)] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let mut h = crate::support::blank_harness(4);
            let p = if track_click { track_end(axis, &h) } else { axis.start(&h) };
            // Web pointerdown can run before canvas focus. The first eframe focus
            // poll can also append Focused(false) after that activating press.
            h.input_mut().focused = false;
            let mut press = vec![Event::PointerMoved(p), button(p, true)];
            if poll_focus {
                press.push(Event::WindowFocused(false));
            }
            events(&mut h, press);
            h.input_mut().focused = true;
            let next = if track_click { button(p, false) } else { Event::PointerMoved(p + axis.movement()) };
            events(&mut h, [Event::WindowFocused(true), next]);
            assert!(axis.scroll(&h) > 0.0, "{axis:?}: activating click={track_click}, focus poll={poll_focus}");
            if !track_click {
                let before = axis.scroll(&h);
                h.input_mut().focused = false;
                events(&mut h, []);
                h.input_mut().focused = true;
                events(&mut h, [Event::PointerMoved(p + axis.movement() * 2.0)]);
                assert_eq!(axis.scroll(&h), before, "{axis:?}: later focus loss still cancels the gesture");
            }
        }
    }
}

#[test]
fn losing_focus_in_the_activating_press_batch_cancels_the_scrollbar() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        h.input_mut().focused = true;
        events(&mut h, [Event::WindowFocused(true)]);
        let p = axis.start(&h);
        h.input_mut().focused = false;
        events(&mut h, [Event::PointerMoved(p), button(p, true), Event::WindowFocused(false)]);
        h.input_mut().focused = true;
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: focus lost after the press must end capture");
    }
}

#[test]
fn focus_acquired_and_lost_after_a_new_press_in_one_batch_still_cancels_it() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        let p = axis.start(&h);
        h.input_mut().focused = false;
        events(&mut h, [Event::PointerMoved(p), button(p, true), Event::WindowFocused(true), Event::WindowFocused(false)]);
        h.input_mut().focused = true;
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: observed acquisition followed by loss must cancel even in the press batch");
    }
}

#[test]
fn focus_acquired_during_hidden_ticks_is_tracked_before_a_later_loss() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        let p = axis.start(&h);
        h.input_mut().focused = false;
        events(&mut h, [Event::PointerMoved(p), button(p, true)]);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        assert!(axis.scroll(&h) > 0.0, "{axis:?}: an unfocused activating drag must continue");
        let ctx = h.ctx.clone();
        let _ = ctx.run_logic(&egui::RawInput { focused: true, ..Default::default() }, |ctx| h.state_mut().logic(ctx));
        let _ = ctx.run_logic(&egui::RawInput { focused: false, ..Default::default() }, |ctx| h.state_mut().logic(ctx));
        assert!(ctx.dragged_id().is_none(), "{axis:?}: the later hidden focus loss ends capture");
        let stopped = axis.scroll(&h);
        h.input_mut().focused = true;
        events(&mut h, [Event::PointerMoved(p + axis.movement() * 2.0)]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: the hidden cancellation survives until input is processed");
    }
}

#[test]
fn non_primary_thumb_drags_keep_their_existing_behavior() {
    for button_kind in [PointerButton::Secondary, PointerButton::Middle] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let mut h = crate::support::blank_harness(4);
            let p = axis.start(&h);
            events(&mut h, [Event::PointerMoved(p), Event::PointerButton { pos: p, button: button_kind, pressed: true, modifiers: Modifiers::NONE }]);
            events(&mut h, [Event::PointerMoved(p + axis.movement())]);
            assert!(axis.scroll(&h) > 0.0, "{axis:?}: {button_kind:?} dragging must retain baseline behavior");
        }
    }
}

fn dragging(axis: Axis) -> (Harness, Pos2) {
    dragging_with_policy(axis, false)
}

fn dragging_with_policy(axis: Axis, release_on_pointer_gone: bool) -> (Harness, Pos2) {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().services.scrollbar_release_on_pointer_gone = release_on_pointer_gone;
    let p = begin_drag(&mut h, axis);
    (h, p)
}

#[test]
fn a_delivered_outside_release_ends_both_scrollbar_drags() {
    for pointer_gone in [false, true] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut h, p) = dragging(axis);
            let outside = match axis {
                Axis::Vertical => pos2(1250.0, p.y),
                Axis::Horizontal => pos2(p.x, 850.0),
            };
            if pointer_gone {
                events(&mut h, [Event::PointerGone]);
                events(&mut h, [button(p, false)]);
            } else {
                events(&mut h, [Event::PointerMoved(outside), button(outside, false)]);
            }
            let stopped = axis.scroll(&h);
            let released = if pointer_gone { p } else { outside };
            events(&mut h, [Event::PointerMoved(released + axis.movement())]);
            assert_eq!(axis.scroll(&h), stopped, "{axis:?}: mouse movement after release must not scroll, pointer gone={pointer_gone}");
        }
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
fn the_wayland_fallback_does_not_leave_either_scrollbar_captured_after_a_lost_release() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, p) = dragging_with_policy(axis, true);
        // egui-winit can discard MouseInput after CursorLeft clears its pointer position.
        events(&mut h, [Event::PointerGone]);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(p + axis.movement())]);
        events(&mut h, [Event::PointerMoved(p + axis.movement() * 2.0)]);
        assert!(!h.ctx.input(|i| i.pointer.primary_down()), "the app releases the owned gesture even when the bridge missed its release");
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
        let (mut h, p) = dragging_with_policy(axis, true);
        let next_press = axis.outside_track(p);
        let mut batch = interruption.to_vec();
        batch.extend([Event::PointerMoved(next_press), button(next_press, true)]);
        events(&mut h, batch);
        let stopped = axis.scroll(&h);
        events(&mut h, [Event::PointerMoved(next_press + axis.movement())]);
        assert_eq!(axis.scroll(&h), stopped, "{axis:?}: the old capture must not steal a new press after interruption");
        assert_ne!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0), "{axis:?}: the new cell gesture must still work");
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

#[test]
fn a_focused_logic_tick_does_not_replay_old_interruption_events() {
    for interruption in [vec![Event::PointerGone], vec![Event::WindowFocused(false), Event::WindowFocused(true)]] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let mut h = crate::support::blank_harness(4);
            h.state_mut().services.scrollbar_release_on_pointer_gone = true;
            let p = axis.start(&h);
            h.input_mut().events.extend(interruption.iter().cloned());
            h.input_mut().events.extend([Event::PointerMoved(p), button(p, true)]);
            h.run_steps(1);
            let ctx = h.ctx.clone();
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| h.state_mut().logic(ctx));
            events(&mut h, [Event::PointerMoved(p + axis.movement())]);
            assert!(axis.scroll(&h) > 0.0, "{axis:?}: stale events in a focused hidden tick must not cancel a valid held press");
        }
    }
}

fn child_harness(release_on_pointer_gone: bool) -> (Harness, std::sync::Arc<std::sync::Mutex<Vec<Event>>>) {
    let queued = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let input = queued.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
        let mut raw = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            events: std::mem::take(&mut *input.lock().unwrap()),
            ..Default::default()
        };
        raw.viewports.entry(viewport.ids.this).or_default().parent = Some(viewport.ids.parent);
        let mut draw = viewport.viewport_ui_cb;
        // As in eframe's immediate renderer, there is no App::raw_input_hook here.
        let mut out = ctx.run_ui(raw, |ui| draw(ui));
        out.textures_delta.clear(); // no renderer in this input test
    });
    let mut app = crate::common::app(crate::support::blank_session());
    app.services.scrollbar_release_on_pointer_gone = release_on_pointer_gone;
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut gridcraft_ui_egui::SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("scrollbar child"), Default::default(), |child, _| app.ui(child));
        },
        app,
    );
    h.ctx.set_embed_viewports(false);
    h.run_steps(4);
    (h, queued)
}

fn child_events(h: &mut Harness, queued: &std::sync::Mutex<Vec<Event>>, events: impl IntoIterator<Item = Event>) {
    queued.lock().unwrap().extend(events);
    h.run_steps(2);
}

#[test]
fn a_child_focus_loss_in_the_activating_batch_cancels_the_scrollbar() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(false);
        child_events(&mut h, &queued, [Event::WindowFocused(true)]);
        let p = axis.start(&h);
        child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true), Event::WindowFocused(false)]);
        child_events(&mut h, &queued, [Event::WindowFocused(true), Event::PointerMoved(p + axis.movement())]);
        assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: child focus loss after the press must end capture");
    }
}

#[test]
fn immediate_child_windows_keep_the_new_gesture_after_scrollbar_interruption() {
    for interruption in [vec![Event::PointerGone], vec![Event::WindowFocused(false), Event::WindowFocused(true)]] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut h, queued) = child_harness(true);
            let start = axis.start(&h);
            child_events(&mut h, &queued, [Event::PointerMoved(start), button(start, true)]);
            let p = start + axis.movement();
            child_events(&mut h, &queued, [Event::PointerMoved(p)]);
            assert!(axis.scroll(&h) > 0.0, "{axis:?}: the child scrollbar started dragging");
            let next_press = axis.outside_track(p);
            let mut batch = interruption.clone();
            batch.extend([Event::PointerMoved(next_press), button(next_press, true)]);
            child_events(&mut h, &queued, batch);
            let stopped = axis.scroll(&h);
            child_events(&mut h, &queued, [Event::PointerMoved(next_press + axis.movement())]);
            assert_eq!(axis.scroll(&h), stopped, "{axis:?}: the child scrollbar stopped");
            assert_ne!(
                h.state().session.active().unwrap().selection.active,
                CellRef::new(0, 0),
                "{axis:?}: the child's new cell gesture still works"
            );
        }
    }
}

#[test]
fn immediate_child_windows_preserve_held_drags_without_the_wayland_fallback() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(false);
        let start = axis.start(&h);
        child_events(&mut h, &queued, [Event::PointerMoved(start), button(start, true)]);
        let p = start + axis.movement();
        child_events(&mut h, &queued, [Event::PointerMoved(p)]);
        let before = axis.scroll(&h);
        child_events(&mut h, &queued, [Event::PointerGone]);
        child_events(&mut h, &queued, [Event::PointerMoved(p + axis.movement())]);
        child_events(&mut h, &queued, [Event::PointerMoved(p + axis.movement() * 2.0)]);
        assert!(axis.scroll(&h) > before, "{axis:?}: a held child drag must survive window exit on capture-capable hosts");
    }
}

#[test]
fn a_child_release_and_repress_on_the_same_thumb_starts_a_fresh_capture() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(true);
        let p = axis.start(&h);
        child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
        child_events(&mut h, &queued, [button(p, false), Event::PointerGone, Event::PointerMoved(p), button(p, true)]);
        child_events(&mut h, &queued, [Event::PointerMoved(p + axis.movement())]);
        assert!(axis.scroll(&h) > 0.0, "{axis:?}: a delivered release must give the next child press a fresh capture");
    }
}

#[test]
fn a_child_pending_thumb_press_cannot_steal_a_new_gesture_after_interruption() {
    for interruption in [vec![Event::PointerGone], vec![Event::WindowFocused(false), Event::WindowFocused(true)]] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut h, queued) = child_harness(true);
            let p = axis.start(&h);
            child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
            let next_press = axis.outside_track(p) + axis.movement() * 2.0;
            let mut batch = interruption.clone();
            batch.extend([Event::PointerMoved(next_press), button(next_press, true)]);
            child_events(&mut h, &queued, batch);
            child_events(&mut h, &queued, [Event::PointerMoved(next_press + axis.movement())]);
            assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: the interrupted pending thumb must stay stopped");
            assert_ne!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0), "{axis:?}: the new child gesture must work");
        }
    }
}

fn track_end(axis: Axis, h: &Harness) -> Pos2 {
    match axis {
        Axis::Vertical => pos2(h.state().grid.rect.unwrap().right() + 7.0, h.state().grid.cells_rect.unwrap().bottom() - 12.0),
        Axis::Horizontal => {
            let track = h.state().grid.hscroll.unwrap();
            pos2(track.right() - 6.0, track.center().y)
        }
    }
}

#[test]
fn cancelling_a_child_pending_track_press_does_not_page() {
    for interruption in [Event::PointerGone, Event::WindowFocused(false)] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut h, queued) = child_harness(true);
            let p = track_end(axis, &h);
            child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
            queued.lock().unwrap().push(interruption.clone());
            h.run_steps(1);
            assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: cancelling must not turn the old press into a track click");
            h.ctx.input_for(egui::ViewportId::from_hash_of("scrollbar child"), |input| {
                assert!(!input.pointer.primary_clicked(), "cancellation must not generate a click anywhere in the app");
                assert_eq!(input.pointer.delta(), Vec2::ZERO, "cancellation must not expose pointer movement");
            });
        }
    }
}

#[test]
fn a_new_child_click_after_cancellation_is_not_a_double_click() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(true);
        let p = axis.start(&h);
        child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
        let next_press = p + axis.movement().normalized() * 2.0;
        queued.lock().unwrap().extend([
            Event::WindowFocused(false),
            Event::WindowFocused(true),
            Event::PointerMoved(next_press),
            button(next_press, true),
            button(next_press, false),
        ]);
        h.run_steps(1);
        h.ctx.input_for(egui::ViewportId::from_hash_of("scrollbar child"), |input| {
            assert!(input.pointer.primary_clicked(), "the new click must work");
            assert!(!input.pointer.button_double_clicked(PointerButton::Primary), "the cancelled press must not count towards a double-click");
        });
    }
}

#[test]
fn ordinary_child_track_clicks_page_when_released_in_a_later_frame() {
    for fallback in [false, true] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut h, queued) = child_harness(fallback);
            let p = track_end(axis, &h);
            child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
            assert_eq!(axis.scroll(&h), 0.0, "the track waits for the release");
            child_events(&mut h, &queued, [button(p, false)]);
            assert!(axis.scroll(&h) > 0.0, "{axis:?}: an ordinary child track click must page");
        }
    }
}

#[test]
fn a_child_track_click_completed_before_pointer_exit_still_pages() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(true);
        let p = track_end(axis, &h);
        child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
        child_events(&mut h, &queued, [button(p, false), Event::PointerGone]);
        assert!(axis.scroll(&h) > 0.0, "{axis:?}: a release before exit is a completed click");
    }
}

#[test]
fn a_root_pending_thumb_press_does_not_steal_a_later_cell_click() {
    for interruption in [vec![Event::PointerGone], vec![Event::WindowFocused(false), Event::WindowFocused(true)]] {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let mut h = crate::support::blank_harness(4);
            h.state_mut().services.scrollbar_release_on_pointer_gone = true;
            let p = axis.start(&h);
            events(&mut h, [Event::PointerMoved(p), button(p, true)]);
            let next_press = axis.outside_track(p) + axis.movement() * 2.0;
            let mut batch = interruption.clone();
            batch.extend([Event::PointerMoved(next_press), button(next_press, true), button(next_press, false)]);
            events(&mut h, batch);
            assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: the old thumb must stay stopped");
            assert_ne!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0), "{axis:?}: the new root click must select its cell");
        }
    }
}

#[test]
fn a_hidden_root_focus_loss_does_not_let_a_pending_press_steal_the_next_click() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut h = crate::support::blank_harness(4);
        let p = axis.start(&h);
        events(&mut h, [Event::PointerMoved(p), button(p, true)]);
        let ctx = h.ctx.clone();
        let hidden = egui::RawInput { focused: false, events: vec![Event::WindowFocused(false)], ..Default::default() };
        let _ = ctx.run_logic(&hidden, |ctx| h.state_mut().logic(ctx));
        h.input_mut().events.extend(hidden.events);
        let next_press = axis.outside_track(p) + axis.movement() * 2.0;
        events(&mut h, [Event::WindowFocused(true), Event::PointerMoved(next_press), button(next_press, true), button(next_press, false)]);
        assert_eq!(axis.scroll(&h), 0.0, "{axis:?}: hidden cancellation must not page the old thumb");
        assert_ne!(
            h.state().session.active().unwrap().selection.active,
            CellRef::new(0, 0),
            "{axis:?}: the next root click must work after a hidden tick"
        );
    }
}

#[test]
fn a_real_child_track_click_after_interruption_in_the_same_batch_still_pages() {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let (mut h, queued) = child_harness(true);
        let p = axis.start(&h);
        child_events(&mut h, &queued, [Event::PointerMoved(p), button(p, true)]);
        let next_press = track_end(axis, &h);
        child_events(
            &mut h,
            &queued,
            [
                Event::WindowFocused(false),
                Event::WindowFocused(true),
                Event::PointerMoved(next_press),
                button(next_press, true),
                button(next_press, false),
            ],
        );
        assert!(axis.scroll(&h) > 0.0, "{axis:?}: cancellation must not swallow a real later track click");
    }
}
