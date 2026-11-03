//! Desktop: each workbook window is a native window of its own.
//!
//! The engine owns the windows ([`Session::windows`]); [`DocWindows`] mirrors the visible ones in
//! a [`WindowList`] and draws each through [`Viewports`]: the root viewport shows one window and
//! every other window is an immediate viewport. Before a window draws, it becomes the engine's
//! window and its interface state is swapped in ([`SheetApp::show_window`]); after the frame the
//! focused window is active again, so menu commands, scripts and shortcuts outside a window's own
//! input go to the window the user worked in last.
//!
//! Close and quit:
//! - A window's close button closes that window (`window.close`): one of several windows on a
//!   workbook closes without asking; a workbook's last window asks Save / Don't Save / Cancel when
//!   it has unsaved changes.
//! - Quit ([`SheetApp::request_quit`]) asks about each unsaved workbook in its window; Cancel stops.
//! - When the root's window closes while others are open, another window moves into the root.
//! - With no window left, the app quits, or (macOS) keeps running with its root hidden and the
//!   menu bar showing until a workbook is created or opened.
//!
//! [`Session::windows`]: gridcraft_engine::Session::windows

use craft_window::{CASCADE_STEP, Frame, Rect, Viewports, WindowEvent, WindowId, WindowList, cascade};
use serde_json::json;

use crate::SheetApp;

/// The native windows (see the module docs).
pub struct DocWindows {
    /// Engine window id per native window.
    list: WindowList<u64>,
    viewports: Viewports,
    /// The builder new windows start from: the root window's size, icon and frame settings.
    base: egui::ViewportBuilder,
    /// Quit when the last window closes (Windows, Linux); macOS keeps running without a window.
    quit_with_last_window: bool,
    /// The root viewport is hidden because no window is open.
    root_hidden: bool,
}

impl DocWindows {
    /// Takes over `app`'s windows. `base` is the root window's builder, used for new windows.
    pub fn new(app: &mut SheetApp, base: egui::ViewportBuilder, quit_with_last_window: bool) -> DocWindows {
        app.native_windows = true;
        // The root window keeps the desktop's frame, so every window does.
        DocWindows { list: WindowList::new(), viewports: Viewports::new(Frame::Desktop), base, quit_with_last_window, root_hidden: false }
    }

    /// The platform default for the last window closing: macOS keeps running, others quit.
    pub fn platform_quits_with_last_window() -> bool {
        !cfg!(target_os = "macos")
    }

    /// The native window showing engine window `window`.
    fn find(&self, window: u64) -> Option<WindowId> {
        self.list.find(|w| *w == window)
    }

    /// The engine window drawn in the root viewport.
    pub fn root_window(&self) -> Option<u64> {
        self.viewports.root().and_then(|w| self.list.get(w).copied())
    }

    /// The engine windows that have a native window, in opening order.
    pub fn shown_windows(&self) -> Vec<u64> {
        self.list.iter().map(|(_, w)| *w).collect()
    }

    /// The egui viewport showing engine window `window`.
    pub fn viewport_of(&self, window: u64) -> Option<egui::ViewportId> {
        self.find(window).map(|id| self.viewports.viewport_id(id))
    }

    /// The egui viewport of the focused window, where menu commands that act on input (copy,
    /// paste, undo in a text box) go.
    pub fn focused_viewport(&self) -> egui::ViewportId {
        self.list.active().map(|w| self.viewports.viewport_id(w)).unwrap_or(egui::ViewportId::ROOT)
    }

    /// Call each frame after [`SheetApp::logic`], also while the root is hidden (eframe runs only
    /// the logic then): shows windows created or opened by menu commands, files and scripts.
    pub fn logic(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        self.sync(app, ctx);
        if let Some((arrange, windows)) = app.arrange_windows.take() {
            self.arrange(ctx, &arrange, &windows);
        }
    }

    /// View › Arrange All: moves and sizes the native windows of `windows` (engine ids, the first
    /// on top or at the top left) over the root window's monitor.
    fn arrange(&self, ctx: &egui::Context, arrange: &str, windows: &[u64]) {
        let ids: Vec<WindowId> = windows.iter().filter_map(|w| self.find(*w)).collect();
        let (monitor, infos) = ctx.input(|i| (i.viewport().monitor_size, i.raw.viewports.clone()));
        // Platforms that don't report the monitor (Wayland, tests) get a common laptop screen.
        let size = monitor.filter(|m| m.x > 0.0 && m.y > 0.0).unwrap_or(egui::vec2(1280.0, 800.0));
        let mut rects = arrange_rects(arrange, ids.len(), Rect::new(0.0, 0.0, size.x, size.y));
        if arrange == "cascade" {
            // The active window goes in front: the last step of the cascade.
            rects.reverse();
        }
        for (id, r) in ids.iter().zip(rects) {
            let vp = self.viewports.viewport_id(*id);
            // Inner size is what can be set: take off the frame the window has now.
            let frame = infos.get(&vp).and_then(|v| Some(v.outer_rect?.size() - v.inner_rect?.size())).unwrap_or(egui::Vec2::ZERO);
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::OuterPosition(egui::pos2(r.x, r.y)));
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::InnerSize((egui::vec2(r.width, r.height) - frame).max(egui::vec2(240.0, 160.0))));
        }
        if let Some(first) = ids.first() {
            self.viewports.focus(ctx, *first);
        }
    }

    /// Mirrors the engine's visible windows: closed or hidden windows lose their native window
    /// (the root adopting another), new ones get one, and a window a command activated comes to
    /// the front. With no window left, quits or hides the root.
    fn sync(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        let visible = visible_windows(app);
        let gone: Vec<WindowId> = self.list.iter().filter(|(_, w)| !visible.contains(w)).map(|(id, _)| id).collect();
        let mut root_closed = false;
        for id in gone {
            root_closed |= self.viewports.root() == Some(id);
            self.list.close(id);
        }
        if root_closed {
            // The window the engine brought forward, else the most recently focused.
            let next = app.session.active_window().and_then(|w| self.find(w)).or_else(|| self.list.active());
            match next {
                Some(id) => self.viewports.adopt(ctx, &self.list, id),
                None => self.viewports.set_root(None),
            }
        }
        for w in visible {
            if self.find(w).is_none() {
                self.list.open(w);
            }
        }
        if self.viewports.root().is_none() {
            let first = app.session.active_window().and_then(|w| self.find(w)).or_else(|| self.list.active());
            self.viewports.set_root(first);
        }
        if self.list.is_empty() {
            if self.quit_with_last_window {
                if !app.allow_close {
                    app.allow_close = true;
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            } else if !self.root_hidden {
                self.root_hidden = true;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(false));
            }
            return;
        }
        if self.root_hidden {
            self.root_hidden = false;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        // A command or script activated another window: bring its native window forward.
        if let Some(id) = app.session.active_window().and_then(|w| self.find(w))
            && self.list.active() != Some(id)
        {
            self.list.focus(id);
            self.viewports.focus(ctx, id);
        }
    }

    /// Draws every window. Call from the root viewport's UI each frame, in place of
    /// [`SheetApp::ui`].
    pub fn show(&mut self, app: &mut SheetApp, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.sync(app, &ctx);
        let focused = self.list.active().and_then(|id| self.list.get(id).copied());
        let titles: std::collections::HashMap<u64, String> = self.list.iter().map(|(_, w)| (*w, window_title(app, *w))).collect();
        let base = &self.base;
        // The window a command run in some window's UI activated (File › New, Switch Windows, a close).
        let mut requested: Option<u64> = None;
        let events = self.viewports.show(
            ui,
            &mut self.list,
            |_, w| base.clone().with_title(titles.get(w).cloned().unwrap_or_default()),
            |ui, _, w| {
                app.show_window(*w);
                // Files dropped on a window open as workbooks (the root's arrive through the app's
                // raw input hook; a child window's input reaches only its own pass).
                let dropped: Vec<String> = ui
                    .ctx()
                    .input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path().to_str().map(str::to_string)).filter(|p| !p.is_empty()).collect());
                for path in dropped {
                    app.open_path(&path);
                }
                app.ui(ui);
                if app.session.active_window() != Some(*w) {
                    requested = app.session.active_window();
                }
            },
        );
        for event in events {
            match event {
                WindowEvent::Focused(_) => {}
                WindowEvent::CloseRequested(id) => {
                    if let Some(w) = self.close_requested(app, &ctx, id) {
                        requested = Some(w);
                    }
                }
            }
        }
        let target = requested.or_else(|| self.list.active().and_then(|id| self.list.get(id).copied()));
        if let Some(target) = target.filter(|w| app.session.window(*w).is_some()) {
            if requested.is_none()
                && let Some(previous) = focused.filter(|p| *p != target && app.session.window(*p).is_some())
            {
                // The user moved to another window: the previous window's draft is entered there,
                // as Enter would. One its workbook refuses stays open in that window.
                app.enter_window(previous);
                let _ = app.settle_drafts();
            }
            app.enter_window(target);
        }
        self.sync(app, &ctx);
    }

    /// A window's close button. Returns the window to bring forward when the close needs an
    /// answer (or moved focus); `None` when nothing changed.
    fn close_requested(&mut self, app: &mut SheetApp, ctx: &egui::Context, id: WindowId) -> Option<u64> {
        let is_root = self.viewports.root() == Some(id);
        if app.allow_close {
            // Quitting: every workbook has been answered for; the root's close ends the app.
            return None;
        }
        if is_root {
            // eframe would end the app with the root window; closing it is the app's decision.
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::CancelClose);
        }
        let window = *self.list.get(id)?;
        app.enter_window(window);
        app.run_or_alert("window.close", json!({"window": window}));
        app.session.active_window()
    }
}

/// The windows that get a native window: every window not hidden (View › Hide), or the active one
/// when all are hidden, so the app still has a window to unhide them from.
fn visible_windows(app: &SheetApp) -> Vec<u64> {
    let shown: Vec<u64> = app.session.windows().iter().filter(|w| !w.hidden).map(|w| w.id).collect();
    if shown.is_empty() { app.session.active_window().into_iter().collect() } else { shown }
}

/// A native window's title: the window's ("Book1", "Book1:2"), marked while its workbook has
/// unsaved changes.
fn window_title(app: &SheetApp, window: u64) -> String {
    let title = app.session.window_title(window).unwrap_or_default();
    let dirty = app.session.window(window).and_then(|w| app.session.documents().iter().find(|d| d.uid == w.doc_uid)).is_some_and(|d| d.is_dirty());
    if dirty { format!("{title} — Edited") } else { title }
}

/// Outer rectangles for `n` windows arranged within `bounds` as View › Arrange All's `arrange`
/// says: `tiled` (a grid, the last row's windows widened to fill it), `horizontal` (stacked top
/// to bottom), `vertical` (side by side) or `cascade` (each one title bar below and right of the
/// previous, wrapping at the edges).
pub fn arrange_rects(arrange: &str, n: usize, bounds: Rect) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    let count = n as f32;
    match arrange {
        "horizontal" => {
            let h = bounds.height / count;
            (0..n).map(|i| Rect::new(bounds.x, bounds.y + h * i as f32, bounds.width, h)).collect()
        }
        "vertical" => {
            let w = bounds.width / count;
            (0..n).map(|i| Rect::new(bounds.x + w * i as f32, bounds.y, w, bounds.height)).collect()
        }
        "cascade" => {
            let shift = CASCADE_STEP * (count - 1.0);
            let size = [(bounds.width - shift).max(bounds.width * 0.5), (bounds.height - shift).max(bounds.height * 0.5)];
            let mut previous = Rect::new(bounds.x - CASCADE_STEP, bounds.y - CASCADE_STEP, size[0], size[1]);
            (0..n)
                .map(|_| {
                    previous = cascade(previous, size, bounds, CASCADE_STEP);
                    previous
                })
                .collect()
        }
        _ => {
            let cols = (count.sqrt().ceil() as usize).max(1);
            let rows = n.div_ceil(cols);
            let h = bounds.height / rows as f32;
            (0..n)
                .map(|i| {
                    let (row, col) = (i / cols, i % cols);
                    let in_row = if row + 1 == rows { n - row * cols } else { cols };
                    let w = bounds.width / in_row as f32;
                    Rect::new(bounds.x + w * col as f32, bounds.y + h * row as f32, w, h)
                })
                .collect()
        }
    }
}
