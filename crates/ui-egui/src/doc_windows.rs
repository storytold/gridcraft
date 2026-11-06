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

/// One native window: the workbooks shown as tabs in it. WPS keeps one tab strip per window, and
/// a tab belongs to that window; documents opened while it has focus join it.
struct Group {
    /// Engine windows in this native window, in strip order.
    order: Vec<u64>,
    /// Tabs the user pinned; they stay first.
    pinned: Vec<u64>,
    /// The engine window drawn in this native window.
    active: Option<u64>,
}

impl Group {
    fn new(active: u64) -> Group {
        Group { order: vec![active], pinned: Vec::new(), active: Some(active) }
    }

    fn contains(&self, w: u64) -> bool {
        self.order.contains(&w)
    }

    /// Drops a closed window; the active one moves to the next tab.
    fn remove(&mut self, w: u64) {
        self.order.retain(|x| *x != w);
        self.pinned.retain(|x| *x != w);
        if self.active == Some(w) {
            self.active = self.order.first().copied();
        }
    }
}

/// Where the last input landed: new documents join that window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Root,
    Float(WindowId),
}

/// Which native window a strip action applies to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TabTarget {
    Root,
    Float(WindowId),
}

/// The native windows (see the module docs).
pub struct DocWindows {
    /// Engine window id per native window.
    list: WindowList<u64>,
    viewports: Viewports,
    /// The builder new windows start from: the root window's size, icon and frame settings.
    base: egui::ViewportBuilder,
    /// Quit when the last window closes (Windows, Linux); macOS keeps running without a window.
    quit_with_last_window: bool,
    /// Draw every window as a tab strip instead of one native window per workbook.
    tabs: bool,
    /// The main window's tabs.
    root: Group,
    /// Undocked windows, each with its own tabs.
    floats: WindowList<Group>,
    float_viewports: Viewports,
    /// The window that last had the user's input.
    focused: Focus,
    /// The tab's Properties dialog, when open.
    props: Option<Props>,
    /// The root viewport is hidden because no window is open.
    root_hidden: bool,
}

/// What the tab's Properties dialog shows: the workbook's file, read from disk each frame.
#[derive(Clone)]
struct Props {
    title: String,
    path: Option<String>,
}

impl DocWindows {
    /// Takes over `app`'s windows. `base` is the root window's builder, used for new windows.
    pub fn new(app: &mut SheetApp, base: egui::ViewportBuilder, quit_with_last_window: bool) -> DocWindows {
        app.native_windows = true;
        // The root window keeps the desktop's frame, so every window does.
        DocWindows {
            list: WindowList::new(),
            viewports: Viewports::new(Frame::Desktop),
            base,
            quit_with_last_window,
            tabs: true,
            root: Group { order: Vec::new(), pinned: Vec::new(), active: None },
            floats: WindowList::new(),
            float_viewports: Viewports::new(Frame::Desktop),
            focused: Focus::Root,
            props: None,
            root_hidden: false,
        }
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
        if self.tabs {
            self.sync_tabs(app, ctx);
        } else {
            self.sync(app, ctx);
        }
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
            if !self.tabs {
                self.viewports.focus(ctx, id);
            }
        }
        // macOS ignores Focus for a minimized window or a hidden app, so restore and show first.
        if std::mem::take(&mut app.bring_forward)
            && let Some(id) = self.list.active()
        {
            let vp = self.viewports.viewport_id(id);
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::Focus);
        }
    }

    /// Draws every window. Call from the root viewport's UI each frame, in place of
    /// [`SheetApp::ui`].
    pub fn show(&mut self, app: &mut SheetApp, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.tabs {
            self.show_tabs(app, ui, &ctx);
            return;
        }
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

    /// One frame of the tabs: every native window draws its own strip (WPS model), and the strip
    /// of the window that has focus gets the documents opened from it.
    fn show_tabs(&mut self, app: &mut SheetApp, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_tabs(app, ctx);
        // Working in the main window makes it the window new documents join; without this, after
        // undocking a tab every new workbook kept landing in the undocked window.
        if ui.ctx().input(|i| i.focused) {
            self.focused = Focus::Root;
        }
        // Main window.
        if let Some(active) = self.root.active {
            app.show_window(active);
            let mut strip = crate::doc_tabs::DocTabs::from_group(app, &self.root.order, &self.root.pinned, Some(active));
            strip.windows = self.other_windows(app, None);
            app.doc_tabs = Some(strip);
            app.ui(ui);
            if let Some(t) = app.doc_tabs.take() {
                self.apply(app, TabTarget::Root, t, ctx);
            }
        }
        // Undocked windows, each with its own strip.
        if !self.floats.is_empty() {
            let base = self.base.clone();
            let titles: std::collections::HashMap<WindowId, String> =
                self.floats.iter().map(|(id, g)| (id, g.active.map(|w| window_title(app, w)).unwrap_or_default())).collect();
            let mut done: Vec<(WindowId, crate::doc_tabs::DocTabs)> = Vec::new();
            let labels: Vec<(WindowId, String)> =
                self.floats.iter().map(|(id, g)| (id, g.active.map(|w| window_title(app, w)).unwrap_or_default())).collect();
            let Self { floats, float_viewports, .. } = self;
            let events = float_viewports.show(
                ui,
                floats,
                |id, _| base.clone().with_title(titles.get(&id).cloned().unwrap_or_default()),
                |ui, id, g| {
                    if let Some(active) = g.active {
                        app.show_window(active);
                        let mut strip =
                            crate::doc_tabs::DocTabs::from_group(app, &g.order, &g.pinned, Some(active));
                        strip.windows = labels.iter().filter(|(other, _)| *other != id).map(|(other, l)| (l.clone(), Some(*other))).collect();
                        strip.windows.insert(0, (app.ui.language.tr("Main window").to_string(), None));
                        app.doc_tabs = Some(strip);
                        app.ui(ui);
                        if let Some(t) = app.doc_tabs.take() {
                            done.push((id, t));
                        }
                    }
                },
            );
            for (id, t) in done {
                self.apply(app, TabTarget::Float(id), t, ctx);
            }
            for event in events {
                match event {
                    WindowEvent::Focused(id) => self.focused = Focus::Float(id),
                    WindowEvent::CloseRequested(id) => {
                        if let Some(w) = self.floats.get(id).and_then(|g| g.active) {
                            let _ = app.run_or_alert("window.close", json!({"window": w}));
                        }
                    }
                }
            }
        }
        if let Some(w) = app.session.active_window() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(window_title(app, w)));
        }
        // The tab's Properties dialog, above every window.
        if let Some(p) = self.props.take() {
            self.props = Self::properties_window(ctx, &p);
        }
    }

    /// The tab's Properties dialog: what the file says about itself, as WPS shows it.
    fn properties_window(ctx: &egui::Context, p: &Props) -> Option<Props> {
        let mut open = true;
        let mut keep = true;
        let md = p.path.as_deref().and_then(|path| std::fs::metadata(path).ok());
        egui::Window::new(format!("Properties of {}", p.title))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                row(ui, "Type:", &file_type(p.path.as_deref()));
                row(ui, "Location:", &parent_of(p.path.as_deref()));
                row(ui, "Size:", &md.as_ref().map(|m| size_text(m.len())).unwrap_or_else(|| "—".into()));
                ui.separator();
                ui.label("MS-DOS name:");
                row(ui, "Created:", &time_text(md.as_ref().and_then(|m| m.created().ok())));
                row(ui, "Modified:", &time_text(md.as_ref().and_then(|m| m.modified().ok())));
                row(ui, "Accessed:", &time_text(md.as_ref().and_then(|m| m.accessed().ok())));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Attributes:");
                    let mut ro = md.as_ref().map(|m| m.permissions().readonly()).unwrap_or(false);
                    ui.add_enabled(false, egui::Checkbox::new(&mut ro, "Read-only"));
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        (open && keep).then(|| p.clone())
    }

    /// Applies what the user did in one window's strip to that window.
    fn apply(&mut self, app: &mut SheetApp, target: TabTarget, tabs: crate::doc_tabs::DocTabs, ctx: &egui::Context) {
        use crate::doc_tabs::TabAction as A;
        if let Some((moved, at)) = tabs.reordered
            && let Some(g) = self.group_mut(target)
        {
            g.order.retain(|w| *w != moved);
            let at = at.min(g.order.len());
            g.order.insert(at, moved);
        }
        let Some(action) = tabs.action else {
            if let Some((w, target)) = tabs.move_to {
                self.move_tab(w, target);
            }
            return;
        };
        match action {
            A::Pick(w) => self.enter(app, w, target),
            A::Save(w) => {
                self.enter(app, w, target);
                let _ = app.run_or_alert("file.save", json!({}));
            }
            A::Close(w) => {
                let _ = app.run_or_alert("window.close", json!({"window": w}));
            }
            A::CloseRight(w) | A::CloseOthers(w) => {
                let order: Vec<u64> = self.group_ref(target).map(|g| g.order.clone()).unwrap_or_default();
                let closing: Vec<u64> = match action {
                    A::CloseRight(_) => order.iter().copied().skip_while(|x| *x != w).skip(1).collect(),
                    _ => order.iter().copied().filter(|x| *x != w).collect(),
                };
                for w in closing.iter().rev() {
                    let _ = app.run_or_alert("window.close", json!({"window": *w}));
                }
            }
            A::CloseAll => {
                let order: Vec<u64> = self.group_ref(target).map(|g| g.order.clone()).unwrap_or_default();
                for w in order.iter().rev() {
                    let _ = app.run_or_alert("window.close", json!({"window": *w}));
                }
            }
            A::Pin(w) => {
                if let Some(g) = self.group_mut(target) {
                    if let Some(at) = g.pinned.iter().position(|x| *x == w) {
                        g.pinned.remove(at);
                    } else {
                        g.pinned.push(w);
                    }
                }
            }
            A::MoveStart(w) | A::MoveEnd(w) => {
                if let Some(g) = self.group_mut(target) {
                    g.order.retain(|x| *x != w);
                    if matches!(action, A::MoveStart(_)) {
                        g.order.insert(0, w);
                    } else {
                        g.order.push(w);
                    }
                }
            }
            A::Detach(w) => {
                self.detach_tab(target, w);
            }
            A::New => {
                // `file.new` creates the engine window; sync_tabs puts it in the window that has
                // focus, which is the one whose `+` was clicked.
                let _ = app.run_or_alert("file.new", json!({}));
            }
            A::OpenLocation(w) => {
                if let Some(path) = self.path_of(app, w) {
                    reveal(&path);
                }
            }
            A::Properties(w) => {
                self.props = Some(Props {
                    title: app.session.window_title(w).unwrap_or_default(),
                    path: self.path_of(app, w),
                });
            }
            A::DropOutside(w) => {
                // Dropped outside its own strip: another window takes it, or it becomes one.
                let under = self.window_under_cursor(app, ctx);
                let same = match under {
                    Some(None) => self.root.contains(w),
                    Some(Some(id)) => self.floats.get(id).is_some_and(|g| g.contains(w)),
                    None => false,
                };
                match (under, same) {
                    (Some(t), false) => self.move_tab(w, t),
                    (Some(_), true) => {}
                    (None, _) => self.detach_tab(target, w),
                }
            }
        }
    }

    /// Undocks a tab into a native window of its own, closing its window when it loses its last
    /// tab.
    fn detach_tab(&mut self, target: TabTarget, w: u64) {
        if let Some(g) = self.group_mut(target) {
            g.remove(w);
        }
        let emptied = match target {
            TabTarget::Float(id) => self.floats.get(id).is_some_and(|g| g.order.is_empty()),
            TabTarget::Root => false,
        };
        if emptied
            && let TabTarget::Float(id) = target
        {
            if self.focused == Focus::Float(id) {
                self.focused = Focus::Root;
            }
            self.floats.close(id);
        }
        let id = self.floats.open(Group::new(w));
        self.focused = Focus::Float(id);
    }

    /// The native window under the OS cursor: `Some(None)` for the main window, `Some(Some(id))`
    /// for an undocked one, `None` outside every window.
    fn window_under_cursor(&self, app: &SheetApp, ctx: &egui::Context) -> Option<Option<WindowId>> {
        let (x, y) = (app.services.cursor_px?)()?;
        let scale = ctx.pixels_per_point().max(0.1);
        let p = egui::pos2(x / scale, y / scale);
        let rects: Vec<(egui::ViewportId, egui::Rect)> =
            ctx.input(|i| i.raw.viewports.iter().filter_map(|(id, v)| v.outer_rect.map(|r| (*id, r))).collect());
        for (viewport, rect) in rects {
            if !rect.contains(p) {
                continue;
            }
            return Some(if viewport == egui::ViewportId::ROOT { None } else { self.float_viewports.window_of(viewport) });
        }
        None
    }

    /// Moves a tab into another native window (WPS' "Move tab to"), removing it from the window it
    /// came from.
    fn move_tab(&mut self, w: u64, target: Option<WindowId>) {
        if self.root.contains(w) {
            self.root.remove(w);
        }
        let ids: Vec<WindowId> = self.floats.iter().map(|(id, _)| id).collect();
        for id in ids {
            if let Some(g) = self.floats.get_mut(id)
                && g.contains(w)
            {
                g.remove(w);
            }
        }
        match target {
            None => {
                self.root.order.push(w);
                self.root.active = Some(w);
            }
            Some(id) => {
                if let Some(g) = self.floats.get_mut(id) {
                    g.order.push(w);
                    g.active = Some(w);
                }
            }
        }
    }

    /// The file behind engine window `w`, when its workbook has one.
    fn path_of(&self, app: &SheetApp, w: u64) -> Option<String> {
        let doc = app.session.window(w)?.doc_uid;
        app.session.documents().iter().find(|d| d.uid == doc).and_then(|d| d.path.clone())
    }

    /// The other native windows, for the strip's "Move tab to" list.
    fn other_windows(&self, app: &SheetApp, exclude: Option<WindowId>) -> Vec<(String, Option<WindowId>)> {
        let mut list = Vec::new();
        if exclude.is_some() {
            list.push((app.ui.language.tr("Main window").to_string(), None));
        }
        for (id, g) in self.floats.iter() {
            if Some(id) == exclude {
                continue;
            }
            let label = g.active.map(|w| window_title(app, w)).unwrap_or_default();
            list.push((label, Some(id)));
        }
        list
    }

    fn group_ref(&self, target: TabTarget) -> Option<&Group> {
        match target {
            TabTarget::Root => Some(&self.root),
            TabTarget::Float(id) => self.floats.get(id),
        }
    }

    fn group_mut(&mut self, target: TabTarget) -> Option<&mut Group> {
        match target {
            TabTarget::Root => Some(&mut self.root),
            TabTarget::Float(id) => self.floats.get_mut(id),
        }
    }

    /// Brings window `w` forward in its native window, entering the previous window's draft first
    /// (as moving to a native window does).
    fn enter(&mut self, app: &mut SheetApp, w: u64, target: TabTarget) {
        if let Some(g) = self.group_mut(target) {
            g.active = Some(w);
        }
        if app.session.active_window() == Some(w) {
            return;
        }
        if let Some(previous) = app.session.active_window().filter(|p| app.session.window(*p).is_some()) {
            app.enter_window(previous);
            let _ = app.settle_drafts();
        }
        app.enter_window(w);
        if let Some(id) = self.find(w) {
            self.list.focus(id);
        }
    }

    /// Keeps each native window's tabs in step with the engine: closed windows leave, new ones join
    /// the window that last had focus (`File › New`, an opened file, a script).
    fn sync_tabs(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        let visible = visible_windows(app);
        self.root.order.retain(|w| visible.contains(w));
        self.root.pinned.retain(|w| visible.contains(w));
        if self.root.active.is_some_and(|a| !visible.contains(&a)) {
            self.root.active = self.root.order.first().copied();
        }
        let mut emptied: Vec<WindowId> = Vec::new();
        for (id, g) in self.floats.iter_mut() {
            g.order.retain(|w| visible.contains(w));
            g.pinned.retain(|w| visible.contains(w));
            if g.active.is_some_and(|a| !visible.contains(&a)) {
                g.active = g.order.first().copied();
            }
            if g.order.is_empty() {
                emptied.push(id);
            }
        }
        for id in emptied {
            if self.focused == Focus::Float(id) {
                self.focused = Focus::Root;
            }
            self.floats.close(id);
        }
        let mut incoming: Vec<u64> = Vec::new();
        for w in &visible {
            if !self.root.contains(*w) && !self.floats.iter().any(|(_, g)| g.contains(*w)) {
                incoming.push(*w);
            }
        }
        for w in incoming {
            let target = match self.focused {
                Focus::Float(id) if self.floats.get(id).is_some() => Some(id),
                _ => None,
            };
            match target {
                Some(id) => {
                    if let Some(g) = self.floats.get_mut(id) {
                        g.order.push(w);
                        g.active = Some(w);
                    }
                }
                None => {
                    self.root.order.push(w);
                    self.root.active = Some(w);
                }
            }
        }
        if self.root.active.is_none() {
            self.root.active = self.root.order.first().copied();
        }
        if self.root.order.is_empty() && self.floats.is_empty() {
            if self.quit_with_last_window {
                if !app.allow_close {
                    app.allow_close = true;
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            } else if !self.root_hidden {
                self.root_hidden = true;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(false));
            }
        }
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

/// A label/value line in the Properties dialog.
fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.label(egui::RichText::new(value).strong());
    });
}

fn file_type(path: Option<&str>) -> String {
    match path.and_then(|p| std::path::Path::new(p).extension()).and_then(|e| e.to_str()) {
        Some(ext) => format!("{} workbook", ext.to_uppercase()),
        None => "—".to_string(),
    }
}

fn parent_of(path: Option<&str>) -> String {
    path.and_then(|p| std::path::Path::new(p).parent()).map(|p| p.display().to_string()).unwrap_or_else(|| "—".to_string())
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

fn time_text(t: Option<std::time::SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_else(|| "—".to_string())
}

/// Shows the file in the system file manager.
#[cfg(windows)]
fn reveal(path: &str) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
}

#[cfg(target_os = "macos")]
fn reveal(path: &str) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal(path: &str) {
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
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
