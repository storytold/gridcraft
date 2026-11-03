use super::*;

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
        if let Some(batch) = self.synthetic.front() {
            raw.events.extend(batch.iter().cloned());
            self.synthetic_pending = true;
            self.synthetic_injected_at_pass = Some(ctx.cumulative_pass_nr());
        }
    }
}
