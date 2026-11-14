use super::{SheetApp, control};

impl SheetApp {
    /// Records capture geometry from the fresh raw frame, then offers exactly one pending synthetic batch and tracks whether it was applied.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !self.deferred_input.is_empty() {
            let mut events = std::mem::take(&mut self.deferred_input);
            events.append(&mut raw.events);
            raw.events = events;
        }
        numpad_decimal_in_region(raw, self.session.region().decimal, &mut self.numpad_decimal_pending);
        if let Some(rest) = split_for_next_frame(&mut raw.events, self.editor.is_some()) {
            self.deferred_input = rest;
            ctx.request_repaint();
        }
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

/// Splits a batch so a quick typist's keystrokes keep their order, and returns the rest for the
/// next frame. Two places read a frame's input out of order:
/// - the cell editor inserts all of a frame's text before it reads Enter, Tab, Esc or an arrow as
///   flags, so the next cell's first character would land in the cell being committed (`1,5`
///   Enter `2` gave `1,52`); the batch is cut after such a key when text follows it
/// - with no cell being edited, the grid opens the editor on the first character and drops the
///   rest of the frame (`1,5` gave `1`); the batch is cut after that character
fn split_for_next_frame(events: &mut Vec<egui::Event>, editing: bool) -> Option<Vec<egui::Event>> {
    use egui::Key;
    let is_text = |event: &egui::Event| matches!(event, egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_)));
    let ends_edit = |event: &egui::Event| {
        matches!(
            event,
            egui::Event::Key {
                key: Key::Enter | Key::Tab | Key::Escape | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight,
                pressed: true,
                ..
            }
        )
    };
    let at = events.iter().enumerate().position(|(i, event)| {
        let rest = events.get(i + 1..).unwrap_or_default();
        let pressed_after = rest.iter().any(|e| is_text(e) || matches!(e, egui::Event::Key { pressed: true, .. }));
        (ends_edit(event) && rest.iter().any(is_text)) || (!editing && is_text(event) && pressed_after)
    })?;
    Some(events.split_off(at + 1))
}

/// The numpad's decimal key types the region's decimal separator (`1,5` in pt-BR), whatever the
/// keyboard layout: a US layout sends `.` from that key, which pt-BR reads as a date or as text.
/// egui-winit maps no physical key for the numpad decimal, so a Period press with no physical key
/// tells it apart from the main `.` key, which keeps typing `.`. The `.` arrives as text, or as an
/// input-method commit when one is running (IBus on GNOME), possibly in a later frame, so
/// `from_numpad` carries the press over to the next frame.
fn numpad_decimal_in_region(raw: &mut egui::RawInput, decimal: char, from_numpad: &mut bool) {
    if decimal == '.' {
        *from_numpad = false;
        return;
    }
    for event in &mut raw.events {
        match event {
            egui::Event::Key { key: egui::Key::Period, physical_key: None, pressed: true, .. } => *from_numpad = true,
            egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) if *from_numpad && text == "." => {
                *text = decimal.to_string();
                *from_numpad = false;
            }
            egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_)) | egui::Event::Key { pressed: true, .. } => *from_numpad = false,
            _ => {}
        }
    }
}
