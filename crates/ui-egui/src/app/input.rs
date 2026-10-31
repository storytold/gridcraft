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
        let offer = !self.synthetic_pending || self.synthetic_injected_at_pass.is_some_and(|pass| ctx.cumulative_pass_nr() > pass);
        if offer && let Some(batch) = self.synthetic.front() {
            raw.events.extend(batch.iter().cloned());
            self.synthetic_pending = true;
            self.synthetic_injected_at_pass = Some(ctx.cumulative_pass_nr());
        }
        if cfg!(target_os = "macos") {
            control_click_as_secondary(&mut raw.events, &mut self.control_click);
        }
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
    use super::control_click_as_secondary;
    use egui::{Event, Modifiers, PointerButton, pos2};

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
}
