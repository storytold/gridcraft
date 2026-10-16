//! macOS document-window state: the title bar's file icon, its drag, and the edited dot.
//!
//! A macOS document window carries its file: the close button wears the edited dot while there
//! are unsaved changes, and the file's icon drags out of the window (into Finder, Mail, a chat).
//! winit 0.30 exposes the edited flag (`WindowExtMacOS::set_document_edited`, safe); the icon and
//! the drag are [`gridcraft_macos_window`]'s (see its docs for why the interop lives there).

/// Applies the active document's state to the window, only when it changes.
#[derive(Default)]
pub struct DocumentWindow {
    /// The edited flag the window was last told about.
    last_edited: Option<bool>,
    /// The window-level fixes were applied.
    configured: bool,
    /// The file the title-bar icon was loaded for, so it is loaded only when the file changes.
    pub doc_icon_path: Option<String>,
    /// A native drag took the mouse over: egui never sees that gesture's release, so the next
    /// frame's input owes it one ([`Self::take_pending_release`]), and the window's position is
    /// pinned until the session ends ([`Self::pin`]) — a drag cancelled under the held button
    /// hands the gesture back to the system, which would otherwise drag the window along with
    /// the cursor.
    drag_in_flight: bool,
    /// The window position at hand-off, in logical points.
    drag_origin: Option<egui::Pos2>,
}

impl DocumentWindow {
    /// Tell the window about the active document's edited state. Call once per frame with the
    /// frame the callback got; a frame without a window yet (still starting up) is skipped and
    /// retried next frame.
    pub fn sync(&mut self, frame: &eframe::Frame, dirty: bool) {
        let Some(window) = frame.winit_window() else { return };
        if !self.configured {
            self.configured = true;
            // Once: the window server moves a titled window itself for drags that begin in its
            // title-bar band — silently, before the app sees anything, exactly where the document
            // icon sits. The app moves the window itself where its design wants (empty title-bar
            // areas, through egui's window-drag command); the server's silent move only competes
            // with that.
            let _ = gridcraft_macos_window::set_movable(window.as_ref(), false);
        }
        if self.last_edited == Some(dirty) {
            return;
        }
        self.last_edited = Some(dirty);
        use winit::platform::macos::WindowExtMacOS;
        // The dot on the close button; standard even for an untitled document with changes.
        window.set_document_edited(dirty);
    }

    /// Mark the mouse as taken over: called when the native drag session accepted the gesture.
    /// Remembers where the window stood, so the pin ([`Self::pin`]) can hold it there.
    pub fn drag_began(&mut self, window: &winit::window::Window) {
        self.drag_in_flight = true;
        let scale = window.scale_factor() as f32;
        self.drag_origin = window.outer_position().ok().map(|p| egui::Pos2::new(p.x as f32 / scale, p.y as f32 / scale));
    }

    /// Hold the window where the hand-off found it while the drag session is live: a drag
    /// cancelled under the held button hands the gesture back to the system, which would
    /// otherwise drag the window along with the cursor.
    pub fn pin(&mut self, window: &winit::window::Window) {
        if !self.drag_in_flight {
            return;
        }
        let scale = window.scale_factor() as f32;
        if let (Some(o), Some(p)) = (self.drag_origin, window.outer_position().ok()) {
            let now = egui::Pos2::new(p.x as f32 / scale, p.y as f32 / scale);
            if (now.x - o.x).abs() > 0.5 || (now.y - o.y).abs() > 0.5 {
                window.set_outer_position(winit::dpi::LogicalPosition::new(o.x, o.y));
            }
        }
    }

    /// The release egui is owed after a taken-over gesture, once: the drag session consumes the
    /// mouse, so winit delivers no mouse-up, and egui would otherwise believe the button still
    /// down — every later move re-entering a drag (the title bar's window drag included).
    pub fn take_pending_release(&mut self) -> bool {
        let was = std::mem::take(&mut self.drag_in_flight);
        self.drag_origin = None;
        was
    }
}

/// The active document's Finder icon as a texture, or `None` without a file (an untitled book has
/// nothing to show or drag).
pub fn load_doc_icon(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let png = gridcraft_macos_window::file_icon_png(path, 32).ok()?;
    let img = image::load_from_memory(&png).ok()?.to_rgba8();
    let size = [img.width() as _, img.height() as _];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("document_icon", color, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::DocumentWindow;

    #[test]
    fn default_starts_with_no_state() {
        // The first sync must reach the window: nothing has been applied before it.
        let window = DocumentWindow::default();
        assert_eq!(window.last_edited, None);
        assert!(!window.configured);
        assert_eq!(window.doc_icon_path, None);
        assert!(!window.drag_in_flight);
        assert_eq!(window.drag_origin, None);
    }
}
