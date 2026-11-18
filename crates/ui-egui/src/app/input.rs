use crate::window_ui::InputRoute;

use super::{SheetApp, control};

impl SheetApp {
    /// Records capture geometry from the fresh raw frame, then offers exactly one pending synthetic batch to its window and tracks whether the root's was applied.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Capture geometry from this frame's real input before any synthetic event is added. This
        // runs on every native frame, including hidden/minimized logic-only ticks, so a resize,
        // DPI/zoom change, Wayland frame without `inner_rect`, or minimize/restore is recorded
        // even though egui's last UI pass still describes the old layout.
        let geometry = control::CaptureGeometry::from_raw_input(raw, ctx);
        control::note_capture_geometry(self, geometry);
        self.capture_geometry_from_raw = true;
        // This hook runs for the root viewport (and any deferred one); every other native window
        // is an immediate viewport, whose input comes from `SyntheticInput::raw_input` (see
        // `apps/gridcraft`). Only the root viewport's frame offers the queue.
        if raw.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        // A batch queued for another window is in flight until that window's pass applies it;
        // batches are applied in the order they were queued.
        let open: Vec<egui::ViewportId> = self.native_viewports.values().copied().collect();
        self.child_input.retain_open(|viewport| viewport == egui::ViewportId::ROOT || open.contains(&viewport));
        if self.child_input.is_pending() {
            return;
        }
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
        let Some(window) = self.synthetic.front().map(|batch| batch.window) else { return };
        match self.input_route(window) {
            InputRoute::Root => {
                let Some(batch) = self.synthetic.front() else { return };
                raw.events.extend(batch.events.iter().cloned());
                self.synthetic_pending = true;
                self.synthetic_injected_at_pass = Some(ctx.cumulative_pass_nr());
            }
            InputRoute::Child(viewport) => {
                if let Some(batch) = self.synthetic.pop_front() {
                    // The previous batch is out of the way, so this one is accepted; a false
                    // answer can only mean the queue and the hand-off disagreed.
                    let _ = self.child_input.hand_off(batch, viewport);
                }
            }
            // The window was opened earlier in this batch and gets its native viewport in this
            // frame's `doc_windows` sync: the input waits for that window's first frame.
            InputRoute::Wait => {}
            InputRoute::Gone => {
                // Its window closed before the input was applied: dropping it is the only safe
                // choice, since another window may show another workbook.
                self.synthetic.pop_front();
                log::warn!("dropped synthetic input queued for a window that closed");
            }
        }
    }
}
