//! The document tab strip: one tab per open workbook window, as WPS and Excel show them.
//!
//! The engine still owns the windows ([`Session::windows`]); on the desktop [`crate::doc_windows`]
//! draws them all in the root window, and the tabs live in the title bar row (next to AutoSave) so
//! the strip costs no vertical space. The strip only reports what the user did — [`DocTabs::picked`],
//! [`DocTabs::closed`] and [`DocTabs::reordered`] — so the window rules (unsaved changes, the last
//! window closing the app) stay where they already live.
//!
//! [`Session::windows`]: gridcraft_engine::Session::windows

use egui::{Button, Id, RichText, ScrollArea, Stroke, Ui, vec2};

use crate::OpenWindow;
use crate::theme::Tokens;

/// Padding after a tab, so the labels do not touch.
const TAB_PAD: f32 = 8.0;

/// One frame of the strip: what to draw, and what the user did with it.
#[derive(Default)]
pub struct DocTabs {
    pub tabs: Vec<OpenWindow>,
    /// The window of the tab the user clicked, if any.
    pub picked: Option<u64>,
    /// Windows whose close button (or Close / Close others) was used, in order.
    pub closed: Vec<u64>,
    /// A tab dropped before another one: `(moved window, window it was dropped on)`.
    pub reordered: Option<(u64, u64)>,
}

impl DocTabs {
    pub fn new(tabs: Vec<OpenWindow>) -> DocTabs {
        DocTabs { tabs, ..Default::default() }
    }

    /// The strip for the windows on screen, in `order` (windows missing from it go last, in
    /// session order). Hidden windows (View › Hide) stay out; when all are hidden the active one
    /// is still shown, so there is a tab to unhide them from.
    pub fn from_app(app: &crate::SheetApp, order: &[u64]) -> DocTabs {
        let shown: Vec<u64> = app.session.windows().iter().filter(|w| !w.hidden).map(|w| w.id).collect();
        let mut tabs: Vec<OpenWindow> =
            app.open_windows().into_iter().filter(|w| shown.is_empty() || shown.contains(&w.window)).collect();
        tabs.sort_by_key(|w| order.iter().position(|id| *id == w.window).unwrap_or(usize::MAX));
        DocTabs::new(tabs)
    }
}

/// The localised labels the strip needs from the app's language.
pub struct Labels {
    pub close: String,
    pub others: String,
    pub all: String,
}

/// Draws the tabs into the current row — the title bar, beside AutoSave. `max_w` caps how much of
/// the bar the strip takes; the rest scrolls. `close_tooltip` is the localised tooltip for `×`.
pub fn show_inline(ui: &mut Ui, tabs: &mut DocTabs, close_tooltip: &str, labels: &Labels, max_w: f32) {
    let t = Tokens::get(ui.ctx());
    // Collected here so the loop can borrow `tabs.tabs` immutably while reporting clicks.
    let mut picked = None;
    let mut closed: Vec<u64> = Vec::new();
    let mut reordered = None;
    let all: Vec<u64> = tabs.tabs.iter().map(|t| t.window).collect();
    ScrollArea::horizontal().max_width(max_w).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for tab in &tabs.tabs {
                let color = if tab.active { t.accent } else { t.text_dim };
                // The label is a drag source, so a tab can be dropped onto another to reorder.
                let id = Id::new(("doc_tab", tab.window));
                let label = ui.dnd_drag_source(id, tab.window, |ui| {
                    ui.add(Button::new(RichText::new(tab.label()).size(12.5).color(color)).frame(false).min_size(vec2(0.0, 24.0)))
                });
                let resp = label.response;
                if resp.dnd_hover_payload::<u64>().is_some() {
                    ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                }
                if let Some(payload) = resp.dnd_release_payload::<u64>()
                    && *payload != tab.window
                {
                    reordered = Some((*payload, tab.window));
                }
                if tab.active {
                    // Underline the active tab, the way the ribbon marks the open tab.
                    let r = resp.rect;
                    ui.painter().line_segment(
                        [egui::pos2(r.left() + TAB_PAD * 0.5, r.bottom()), egui::pos2(r.right() - TAB_PAD * 0.5, r.bottom())],
                        Stroke::new(2.0, t.accent),
                    );
                }
                if resp.clicked() {
                    picked = Some(tab.window);
                }
                resp.context_menu(|ui| {
                    if ui.button(&labels.close).clicked() {
                        closed.push(tab.window);
                        ui.close();
                    }
                    let others: Vec<u64> = all.iter().copied().filter(|w| *w != tab.window).collect();
                    if ui.add_enabled(!others.is_empty(), Button::new(&labels.others)).clicked() {
                        closed.extend(others);
                        ui.close();
                    }
                    if ui.add_enabled(all.len() > 1, Button::new(&labels.all)).clicked() {
                        closed.extend(all.iter().copied());
                        ui.close();
                    }
                });
                let x = ui.add(Button::new(RichText::new("×").size(12.0).color(t.text_dim)).frame(false).min_size(vec2(16.0, 16.0)));
                if x.on_hover_text(close_tooltip).clicked() {
                    closed.push(tab.window);
                }
                ui.add_space(TAB_PAD);
            }
        });
    });
    tabs.picked = picked;
    tabs.closed = closed;
    tabs.reordered = reordered;
}
