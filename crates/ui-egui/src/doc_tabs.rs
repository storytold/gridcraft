//! The document tab strip: one tab per open workbook window, as WPS and Excel show them.
//!
//! The engine still owns the windows ([`Session::windows`]); on the desktop [`crate::doc_windows`]
//! draws them all in the root window and this strip says which one is on screen. The strip only
//! reports what the user did — [`DocTabs::picked`] and [`DocTabs::closed`] — so the window rules
//! (unsaved changes, the last window closing the app) stay where they already live.
//!
//! [`Session::windows`]: gridcraft_engine::Session::windows

use egui::{Button, RichText, Stroke, Ui, vec2};

use crate::OpenWindow;
use crate::theme::Tokens;

/// Height of the strip, the same as the sheet tab bar so the two read as one family.
const HEIGHT: f32 = 32.0;

/// One frame of the strip: what to draw, and what the user did with it.
#[derive(Default)]
pub struct DocTabs {
    pub tabs: Vec<OpenWindow>,
    /// The window of the tab the user clicked, if any.
    pub picked: Option<u64>,
    /// The window of the tab whose close button the user clicked, if any.
    pub closed: Option<u64>,
}

impl DocTabs {
    pub fn new(tabs: Vec<OpenWindow>) -> DocTabs {
        DocTabs { tabs, ..Default::default() }
    }

    /// The strip for the windows on screen: every window not hidden (View › Hide), or the active
    /// one when all are hidden, so the app still has a tab to unhide them from.
    pub fn from_app(app: &crate::SheetApp) -> DocTabs {
        let shown: Vec<u64> = app.session.windows().iter().filter(|w| !w.hidden).map(|w| w.id).collect();
        let keep = |w: &OpenWindow| shown.is_empty() || shown.contains(&w.window);
        DocTabs::new(app.open_windows().into_iter().filter(keep).collect())
    }
}

/// Draws the strip along the top of `ui`. Clicking a tab sets [`DocTabs::picked`]; its `×` sets
/// [`DocTabs::closed`]. `close_tooltip` is the localised tooltip for the close button.
pub fn show(ui: &mut Ui, tabs: &mut DocTabs, close_tooltip: &str) {
    let t = Tokens::get(ui.ctx());
    // Collected here so the loop can borrow `tabs.tabs` immutably while reporting clicks.
    let mut picked = None;
    let mut closed = None;
    egui::Panel::top("doc_tabs").exact_size(HEIGHT).frame(egui::Frame::NONE.fill(t.tab_bar)).show(ui, |ui| {
        let rect = ui.max_rect();
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.ribbon_border));
        egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(6.0);
                for tab in &tabs.tabs {
                    let label = ui.selectable_label(tab.active, RichText::new(tab.label()).color(t.text));
                    if label.clicked() {
                        picked = Some(tab.window);
                    }
                    let close = ui.add(Button::new(RichText::new("×").size(12.0).color(t.text_dim)).frame(false).min_size(vec2(16.0, 16.0)));
                    if close.on_hover_text(close_tooltip).clicked() {
                        closed = Some(tab.window);
                    }
                }
            });
        });
    });
    tabs.picked = picked;
    tabs.closed = closed;
}
