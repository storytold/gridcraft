use super::{SheetApp, control};

impl SheetApp {
    /// Records capture geometry from the fresh raw frame, then offers exactly one pending synthetic batch and tracks whether it was applied.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Capture geometry from this frame's real input before any synthetic event is added. This
        // runs on every native frame, including hidden/minimized logic-only ticks, so a resize,
        // DPI/zoom change, Wayland frame without `inner_rect`, or minimize/restore is recorded
        // even though egui's last UI pass still describes the old layout.
        let geometry = control::CaptureGeometry::from_raw_input(raw, ctx);
        control::note_capture_geometry(self, geometry);
        self.capture_geometry_from_raw = true;
        // One unapplied gesture step at a time. eframe retains an unconsumed `RawInput` for the
        // next pass and for a hidden logic-only tick, so the batch is offered once and survives
        // until a `ui` pass applies it (which pops it). Press and release stay distinct batches.
        //
        // A completed egui pass may have consumed the injected events without applying them: the
        // font gate returns before input handling, but `begin_pass` has already taken the events.
        // `cumulative_pass_nr` only advances when a pass ends, so a newer value with the batch
        // still pending means exactly that, and the step is offered again for the next real pass.
        if self.synthetic_pending {
            let consumed_by_an_unapplied_pass = self.synthetic_injected_at_pass.is_some_and(|pass| ctx.cumulative_pass_nr() > pass);
            if !consumed_by_an_unapplied_pass {
                return;
            }
        }
        if let Some(batch) = self.synthetic.front_mut() {
            // Real input was converted per window by `ControlClicks`; a scripted Control-click
            // gets the same, with its own press state so it never ends a real one. In the queued
            // batch itself: a step offered again keeps its converted buttons.
            if cfg!(target_os = "macos") {
                control_click_as_secondary(batch, &mut self.control_click);
            }
            raw.events.extend(batch.iter().cloned());
            self.synthetic_pending = true;
            self.synthetic_injected_at_pass = Some(ctx.cumulative_pass_nr());
        }
    }
}

/// Turns macOS Control-clicks into secondary clicks in every window's input (see
/// [`control_click_as_secondary`]). The desktop app runs it from eframe's per-viewport raw-input
/// hook, which also reaches the extra workbook windows that `SheetApp::raw_input_hook` never sees.
/// Each window keeps its own press, so a release ends the press made in its window.
#[derive(Default)]
pub struct ControlClicks {
    held: std::sync::Mutex<std::collections::HashMap<egui::ViewportId, bool>>,
}

impl ControlClicks {
    /// Converts one window's raw input (`raw.viewport_id`).
    pub fn convert(&self, raw: &mut egui::RawInput) {
        let mut held = self.held.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let window = held.entry(raw.viewport_id).or_default();
        control_click_as_secondary(&mut raw.events, window);
    }
}

/// On macOS a Control-click is the secondary click: every app opens its context menu for it
/// (#2563), and winit reports it as a primary click with Control held. A primary press with
/// Control (and not Command) becomes a secondary press, and the release that ends it follows as a
/// secondary release even if Control is let go first, so egui sees a whole secondary click or
/// drag. `held` carries that press across frames.
fn control_click_as_secondary(events: &mut [egui::Event], held: &mut bool) {
    for event in events {
        let egui::Event::PointerButton { button, pressed, modifiers, .. } = event else { continue };
        if *button != egui::PointerButton::Primary {
            continue;
        }
        if *pressed {
            *held = modifiers.ctrl && !modifiers.mac_cmd;
        }
        if *held {
            *button = egui::PointerButton::Secondary;
        }
        if !*pressed {
            *held = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlClicks, control_click_as_secondary};
    use egui::{Event, Modifiers, PointerButton, RawInput, ViewportId, pos2};

    fn button(button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos: pos2(10.0, 10.0), button, pressed, modifiers }
    }

    fn buttons(events: &[Event]) -> Vec<PointerButton> {
        events.iter().filter_map(|e| if let Event::PointerButton { button, .. } = e { Some(*button) } else { None }).collect()
    }

    #[test]
    fn a_control_click_is_a_secondary_click() {
        let mut held = false;
        let mut events = [button(PointerButton::Primary, true, Modifiers::CTRL), button(PointerButton::Primary, false, Modifiers::CTRL)];
        control_click_as_secondary(&mut events, &mut held);
        assert_eq!(buttons(&events), [PointerButton::Secondary, PointerButton::Secondary]);
        assert!(!held);
    }

    #[test]
    fn the_release_follows_its_press_across_frames_after_control_is_let_go() {
        let mut held = false;
        let mut press = [button(PointerButton::Primary, true, Modifiers::CTRL)];
        control_click_as_secondary(&mut press, &mut held);
        let mut release = [button(PointerButton::Primary, false, Modifiers::NONE)];
        control_click_as_secondary(&mut release, &mut held);
        assert_eq!(buttons(&release), [PointerButton::Secondary]);
        // The next plain click is primary again.
        let mut next = [button(PointerButton::Primary, true, Modifiers::NONE), button(PointerButton::Primary, false, Modifiers::NONE)];
        control_click_as_secondary(&mut next, &mut held);
        assert_eq!(buttons(&next), [PointerButton::Primary, PointerButton::Primary]);
    }

    #[test]
    fn command_clicks_and_other_buttons_are_left_alone() {
        let mut held = false;
        // Command-click (Cmd and Control both held counts as Command) adds to the selection.
        let cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
        let cmd_ctrl = Modifiers { ctrl: true, ..cmd };
        let mut events = [
            button(PointerButton::Primary, true, cmd),
            button(PointerButton::Primary, false, cmd),
            button(PointerButton::Primary, true, cmd_ctrl),
            button(PointerButton::Primary, false, cmd_ctrl),
            button(PointerButton::Secondary, true, Modifiers::NONE),
            button(PointerButton::Secondary, false, Modifiers::NONE),
            button(PointerButton::Middle, true, Modifiers::CTRL),
            button(PointerButton::Middle, false, Modifiers::CTRL),
        ];
        control_click_as_secondary(&mut events, &mut held);
        assert_eq!(
            buttons(&events),
            [
                PointerButton::Primary,
                PointerButton::Primary,
                PointerButton::Primary,
                PointerButton::Primary,
                PointerButton::Secondary,
                PointerButton::Secondary,
                PointerButton::Middle,
                PointerButton::Middle
            ]
        );
    }

    #[test]
    fn each_window_keeps_its_own_control_click() {
        let clicks = ControlClicks::default();
        let other = ViewportId::from_hash_of("second workbook window");
        let input = |viewport_id, events| RawInput { viewport_id, events, ..Default::default() };
        // A Control-press in the main window, then a plain click in another window.
        let mut main_press = input(ViewportId::ROOT, vec![button(PointerButton::Primary, true, Modifiers::CTRL)]);
        clicks.convert(&mut main_press);
        let mut other_click =
            input(other, vec![button(PointerButton::Primary, true, Modifiers::NONE), button(PointerButton::Primary, false, Modifiers::NONE)]);
        clicks.convert(&mut other_click);
        let mut main_release = input(ViewportId::ROOT, vec![button(PointerButton::Primary, false, Modifiers::NONE)]);
        clicks.convert(&mut main_release);
        assert_eq!(buttons(&main_press.events), [PointerButton::Secondary]);
        assert_eq!(buttons(&other_click.events), [PointerButton::Primary, PointerButton::Primary]);
        assert_eq!(buttons(&main_release.events), [PointerButton::Secondary], "the release ends the main window's Control-press");
    }

    #[test]
    fn a_scripted_release_offered_again_stays_secondary() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let mut app = crate::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        app.synthetic.push_back(vec![button(PointerButton::Primary, true, Modifiers::CTRL)]);
        app.synthetic.push_back(vec![button(PointerButton::Primary, false, Modifiers::NONE)]);
        let mut press = RawInput::default();
        app.raw_input_hook(&ctx, &mut press);
        assert_eq!(buttons(&press.events), [PointerButton::Secondary]);
        // A `ui` pass applies the press step.
        app.synthetic.pop_front();
        app.synthetic_pending = false;
        let mut release = RawInput::default();
        app.raw_input_hook(&ctx, &mut release);
        assert_eq!(buttons(&release.events), [PointerButton::Secondary]);
        // A pass ends without applying it (the font gate), so the hook offers the step again.
        let mut out = ctx.run_ui(RawInput::default(), |_| {});
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
        let mut again = RawInput::default();
        app.raw_input_hook(&ctx, &mut again);
        assert_eq!(buttons(&again.events), [PointerButton::Secondary], "the re-offered release still ends the secondary press");
    }
}
