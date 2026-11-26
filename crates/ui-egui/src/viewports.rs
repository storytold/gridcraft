//! Shows each open workbook window as an egui immediate viewport, one native window each.
//!
//! Call [`Viewports::show`] once per frame from the root viewport's UI. The root viewport stays the
//! app's: the window mapped to it with [`Viewports::set_root`] is drawn into the root UI, and every
//! other window gets its own viewport, placed one cascade step from the window used last. Close and
//! quit policy stays with the app: the driver reports [`WindowEvent`]s.

use std::collections::HashMap;

use craft_window::{WindowId, WindowList};
use egui::{Context, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};

/// How far right and down a new window opens from the window used last, in points: about one
/// title bar.
pub const CASCADE_STEP: f32 = 28.0;

/// How far a maximized window's frame may reach past its monitor's edges: a window rectangle that
/// starts up to this far left of or above a monitor is on that monitor.
const FRAME_OVERSHOOT: f32 = 16.0;

/// Something that happened to a window this frame. The app decides what follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// The window got focus. The driver has already made it the list's active window.
    Focused(WindowId),
    /// The user asked to close the window (its close button, Alt+F4, ⌘W through the desktop).
    /// A child window stays open until the app closes it in the [`WindowList`], so the app can ask
    /// to save first. The root window's close request is the app's close path to hold.
    CloseRequested(WindowId),
}

/// What the driver remembers about a window between frames.
#[derive(Default)]
struct Seen {
    focused: bool,
    /// The window's outer rectangle as it last reported it.
    outer: Option<Rect>,
    inner_size: Option<Vec2>,
    monitor_size: Option<Vec2>,
    /// The cascade position chosen when the window was created; repeated every frame so egui sees
    /// an unchanged builder.
    position: Option<Pos2>,
    /// The root viewport's title, sent as a command when it changes.
    title: Option<String>,
}

/// The windows of a [`WindowList`] as egui viewports (see the module docs).
#[derive(Default)]
pub struct Viewports {
    root: Option<WindowId>,
    seen: HashMap<WindowId, Seen>,
}

impl Viewports {
    /// The window drawn into the root viewport.
    pub fn root(&self) -> Option<WindowId> {
        self.root
    }

    /// Maps window `id` to the root viewport, or none.
    pub fn set_root(&mut self, id: Option<WindowId>) {
        if let Some(id) = id {
            // The root is a different native window: forget the child's focus and title.
            self.seen.insert(id, Seen::default());
        }
        self.root = id;
    }

    /// Moves window `id` into the root viewport, for when the root's own window closes while others
    /// stay open. The root window takes `id`'s position and size and gets focus; `id`'s data and
    /// [`WindowId`] are unchanged, so its UI state carries over.
    pub fn adopt(&mut self, ctx: &Context, id: WindowId) {
        let (outer, inner_size) = self.seen.get(&id).map_or((None, None), |seen| (seen.outer, seen.inner_size));
        self.set_root(Some(id));
        if let Some(rect) = outer {
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::OuterPosition(rect.min));
        }
        if let Some(size) = inner_size {
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::InnerSize(size));
        }
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
    }

    /// The egui viewport showing window `id`.
    pub fn viewport_id(&self, id: WindowId) -> ViewportId {
        if self.root == Some(id) { ViewportId::ROOT } else { child_viewport_id(id) }
    }

    /// Brings window `id` to the front.
    pub fn focus(&self, ctx: &Context, id: WindowId) {
        ctx.send_viewport_cmd_to(self.viewport_id(id), ViewportCommand::Focus);
    }

    /// Draws every window of `windows`: the root's into `ui`, every other one in an immediate
    /// viewport built by `builder` (with system decorations) and drawn by `content`. Returns what
    /// happened to the windows this frame.
    pub fn show<T>(
        &mut self,
        ui: &mut Ui,
        windows: &mut WindowList<T>,
        mut builder: impl FnMut(WindowId, &T) -> ViewportBuilder,
        mut content: impl FnMut(&mut Ui, WindowId, &mut T),
    ) -> Vec<WindowEvent> {
        self.seen.retain(|id, _| windows.contains(*id));
        let ctx = ui.ctx().clone();
        let mut events = Vec::new();

        if let Some(root) = self.root.filter(|&id| windows.contains(id)) {
            if let Some(title) = windows.get(root).and_then(|data| builder(root, data).title) {
                let seen = self.seen.entry(root).or_default();
                if seen.title.as_ref() != Some(&title) {
                    ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Title(title.clone()));
                    seen.title = Some(title);
                }
            }
            if let Some(data) = windows.get_mut(root) {
                content(ui, root, data);
            }
            self.record(Snapshot::read(&ctx), windows, root, &mut events);
        }

        let children: Vec<WindowId> = windows.ids().filter(|&id| self.root != Some(id)).collect();
        for id in children {
            let Some(data) = windows.get(id) else { continue };
            let mut viewport = builder(id, data).with_decorations(true);
            let position = match self.seen.get(&id) {
                Some(seen) => seen.position,
                None => self.place(windows, id, &viewport),
            };
            if let Some(position) = position {
                viewport = viewport.with_position(position);
            }
            self.seen.entry(id).or_default().position = position;

            let Some(data) = windows.get_mut(id) else { continue };
            let snapshot = ctx.show_viewport_immediate(child_viewport_id(id), viewport, |child_ui, _class| {
                content(child_ui, id, data);
                Snapshot::read(child_ui.ctx())
            });
            self.record(snapshot, windows, id, &mut events);
        }
        events
    }

    fn record<T>(&mut self, snapshot: Snapshot, windows: &mut WindowList<T>, id: WindowId, events: &mut Vec<WindowEvent>) {
        let Snapshot { close, focused, outer, inner, monitor } = snapshot;
        let seen = self.seen.entry(id).or_default();
        seen.outer = outer;
        seen.inner_size = inner.map(|r| r.size()).or(seen.inner_size);
        seen.monitor_size = monitor.or(seen.monitor_size);
        if focused && !seen.focused {
            windows.focus(id);
            events.push(WindowEvent::Focused(id));
        }
        seen.focused = focused;
        if close {
            events.push(WindowEvent::CloseRequested(id));
        }
    }

    /// Where a new window opens: one cascade step from the window used most recently whose
    /// rectangle is known, within that window's monitor. `None` when no window's rectangle is known.
    fn place<T>(&self, windows: &WindowList<T>, id: WindowId, viewport: &ViewportBuilder) -> Option<Pos2> {
        let (previous_id, previous) = windows.recent().filter(|&w| w != id).find_map(|w| Some((w, self.seen.get(&w)?.outer?)))?;
        let seen = self.seen.get(&previous_id);
        let frame = seen.and_then(|s| s.inner_size).map(|inner| previous.size() - inner).unwrap_or_default();
        let size = viewport.inner_size.map(|s| s + frame).unwrap_or(previous.size());
        let bounds = monitor_bounds(previous, seen?.monitor_size?);
        Some(craft_behavior::placement::cascade(previous, size, bounds, CASCADE_STEP).min)
    }
}

/// What a window reported in its pass.
struct Snapshot {
    close: bool,
    focused: bool,
    outer: Option<Rect>,
    inner: Option<Rect>,
    monitor: Option<Vec2>,
}

impl Snapshot {
    fn read(ctx: &Context) -> Self {
        ctx.input(|i| {
            let v = i.viewport();
            Self {
                close: v.close_requested(),
                focused: v.focused.unwrap_or(false),
                outer: v.outer_rect,
                inner: v.inner_rect,
                monitor: v.monitor_size,
            }
        })
    }
}

/// The egui viewport of a window that is not in the root viewport.
pub fn child_viewport_id(id: WindowId) -> ViewportId {
    ViewportId::from_hash_of(("craft_window", id.get()))
}

/// The monitor `window` is on, for a desktop of monitors of one `monitor` size side by side: egui
/// reports only the size of the window's monitor, not where it is.
fn monitor_bounds(window: Rect, monitor: Vec2) -> Rect {
    let tile = |pos: f32, len: f32| if len > 0.0 { ((pos + FRAME_OVERSHOOT) / len).floor() * len } else { 0.0 };
    Rect::from_min_size(egui::pos2(tile(window.min.x, monitor.x), tile(window.min.y, monitor.y)), monitor)
}
