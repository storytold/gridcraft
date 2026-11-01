//! The document tab strip: one tab per open workbook window, as WPS and Excel show them.
//!
//! The engine still owns the windows ([`Session::windows`]); on the desktop [`crate::doc_windows`]
//! draws them in the root window, and the tabs live in the title bar row (next to AutoSave) so the
//! strip costs no vertical space. The strip only reports what the user did ([`DocTabs::action`] and
//! [`DocTabs::reordered`]), so the window rules (unsaved changes, the last window closing the app)
//! stay where they already live.
//!
//! [`Session::windows`]: gridcraft_engine::Session::windows

use egui::{Button, RichText, ScrollArea, Stroke, Ui, vec2};

use crate::OpenWindow;
use crate::theme::Tokens;

/// Padding after a tab, so the labels do not touch.
const TAB_PAD: f32 = 8.0;

/// What the user asked for from a tab (click or context menu). The shell applies it, because the
/// window rules live there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabAction {
    /// Bring this window's tab forward.
    Pick(u64),
    Save(u64),
    Close(u64),
    /// Close every tab to the right of this one.
    CloseRight(u64),
    /// Close every tab but this one.
    CloseOthers(u64),
    /// Close every tab.
    CloseAll,
    /// Undock the tab: the workbook gets a native window of its own.
    Detach(u64),
    /// Keep the tab first in the strip.
    Pin(u64),
    /// Move the tab to the start or the end of the strip.
    MoveStart(u64),
    MoveEnd(u64),
}

/// The localised labels the strip needs from the app's language.
pub struct Labels {
    pub save: String,
    pub close: String,
    pub pin: String,
    pub detach: String,
    pub move_to: String,
    pub start: String,
    pub end: String,
    pub others: String,
    pub right: String,
    pub all: String,
}

/// One frame of the strip: what to draw, and what the user did with it.
#[derive(Default)]
pub struct DocTabs {
    pub tabs: Vec<OpenWindow>,
    /// Windows shown pinned first, in pin order.
    pub pinned: Vec<u64>,
    /// The tab the user acted on this frame.
    pub action: Option<TabAction>,
    /// A tab dragged to a new position: `(window, index in the strip without the dragged tab)`.
    pub reordered: Option<(u64, usize)>,
}

impl DocTabs {
    pub fn new(tabs: Vec<OpenWindow>) -> DocTabs {
        DocTabs { tabs, ..Default::default() }
    }

    /// The strip for the windows on screen, pinned first and then in `order` (windows missing from
    /// it go last, in session order). Hidden windows (View › Hide) stay out; when all are hidden
    /// the active one is still shown, so there is a tab to unhide them from.
    pub fn from_app(app: &crate::SheetApp, order: &[u64], pinned: &[u64], detached: &std::collections::HashSet<u64>) -> DocTabs {
        let shown: Vec<u64> = app.session.windows().iter().filter(|w| !w.hidden).map(|w| w.id).collect();
        let mut tabs: Vec<OpenWindow> = app
            .open_windows()
            .into_iter()
            .filter(|w| (shown.is_empty() || shown.contains(&w.window)) && !detached.contains(&w.window))
            .collect();
        tabs.sort_by_key(|w| {
            let pin = pinned.iter().position(|id| *id == w.window);
            (pin.is_none(), pin.unwrap_or(0), order.iter().position(|id| *id == w.window).unwrap_or(usize::MAX))
        });
        DocTabs { tabs, pinned: pinned.to_vec(), ..Default::default() }
    }

    /// A tab is pinned when the shell says so.
    pub fn is_pinned(&self, window: u64) -> bool {
        self.pinned.contains(&window)
    }
}

/// Draws the tabs into the current row — the title bar, beside AutoSave. `max_w` caps how much of
/// the bar the strip takes; the rest scrolls.
pub fn show_inline(ui: &mut Ui, tabs: &mut DocTabs, labels: &Labels, max_w: f32) {
    let t = Tokens::get(ui.ctx());
    let mut action: Option<TabAction> = None;
    let mut reordered = None;
    let all: Vec<u64> = tabs.tabs.iter().map(|t| t.window).collect();
    let pinned = tabs.pinned.clone();
    let mut rects: Vec<(u64, egui::Rect)> = Vec::new();
    let mut stopped: Option<u64> = None;
    let area = ScrollArea::horizontal().max_width(max_w).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (index, tab) in tabs.tabs.iter().enumerate() {
                let is_pinned = pinned.contains(&tab.window);
                let color = if tab.active { t.accent } else { t.text_dim };
                // A plain clickable widget, drag-and-drop handled below: a drag source swallows the
                // secondary click the tab menu needs.
                let text = if is_pinned { format!("• {}", tab.label()) } else { tab.label() };
                let resp = ui
                    .add(Button::new(RichText::new(text).size(12.5).color(color)).frame(false).min_size(vec2(0.0, 24.0)))
                    .interact(egui::Sense::click_and_drag());
                rects.push((tab.window, resp.rect));
                if resp.dragged() {
                    // Highlight the tab being dragged.
                    ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                }
                if resp.drag_stopped() {
                    stopped = Some(tab.window);
                }
                if tab.active {
                    let r = resp.rect;
                    ui.painter().line_segment(
                        [egui::pos2(r.left() + TAB_PAD * 0.5, r.bottom()), egui::pos2(r.right() - TAB_PAD * 0.5, r.bottom())],
                        Stroke::new(2.0, t.accent),
                    );
                }
                if resp.clicked() {
                    action = Some(TabAction::Pick(tab.window));
                }
                resp.context_menu(|ui| {
                    let w = tab.window;
                    if ui.button(&labels.save).clicked() {
                        action = Some(TabAction::Save(w));
                        ui.close();
                    }
                    if ui.button(&labels.close).clicked() {
                        action = Some(TabAction::Close(w));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(&labels.pin).clicked() {
                        action = Some(TabAction::Pin(w));
                        ui.close();
                    }
                    if ui.button(&labels.detach).clicked() {
                        action = Some(TabAction::Detach(w));
                        ui.close();
                    }
                    ui.menu_button(&labels.move_to, |ui| {
                        if ui.button(&labels.start).clicked() {
                            action = Some(TabAction::MoveStart(w));
                            ui.close();
                        }
                        if ui.button(&labels.end).clicked() {
                            action = Some(TabAction::MoveEnd(w));
                            ui.close();
                        }
                    });
                    ui.separator();
                    let to_the_right: Vec<u64> = all.iter().copied().skip(index + 1).collect();
                    if ui.add_enabled(!to_the_right.is_empty(), Button::new(&labels.right)).clicked() {
                        action = Some(TabAction::CloseRight(w));
                        ui.close();
                    }
                    let others: Vec<u64> = all.iter().copied().filter(|x| *x != w).collect();
                    if ui.add_enabled(!others.is_empty(), Button::new(&labels.others)).clicked() {
                        action = Some(TabAction::CloseOthers(w));
                        ui.close();
                    }
                    if ui.add_enabled(all.len() > 1, Button::new(&labels.all)).clicked() {
                        action = Some(TabAction::CloseAll);
                        ui.close();
                    }
                });
                let x = ui.add(Button::new(RichText::new("×").size(12.0).color(t.text_dim)).frame(false).min_size(vec2(16.0, 16.0)));
                if x.on_hover_text(&labels.close).clicked() {
                    action = Some(TabAction::Close(tab.window));
                }
                ui.add_space(TAB_PAD);
            }
        });
    });
    // A drag that ends on another tab reorders; one that ends outside the strip undocks the tab
    // into a window of its own, as WPS and Excel do.
    let strip = area.inner_rect.expand2(vec2(0.0, 32.0));
    if let Some(w) = stopped
        && let Some(p) = ui.ctx().pointer_latest_pos()
    {
        if strip.contains(p) {
            // Insertion point: how many other tabs' centres are left of the pointer.
            let at = rects.iter().filter(|(id, r)| *id != w && p.x > r.center().x).count();
            reordered = Some((w, at));
        } else {
            action = Some(TabAction::Detach(w));
        }
    }
    tabs.action = action;
    tabs.reordered = reordered;
}
