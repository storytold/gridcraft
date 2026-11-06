//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::keytips::keytip_table;
use crate::theme::{self, Cat, Tokens};
use crate::widgets::{
    big_button, big_split_button, color_palette, icon_button, small_button, small_split_button, small_toggle_button, split_button, toggle_button,
};

mod automate;
mod contextual;
mod controls;
mod data;
mod formulas;
mod galleries;
mod home;
mod insert;
mod pack;
mod page_layout;
mod review;
mod shortcuts;
mod title_bar;
mod view;

use automate::automate;
use contextual::{chart_design, table_design};
pub(crate) use controls::{ComboResult, editable_combo};
use controls::{act, keytip};
use data::data;
use formulas::formulas;
pub use home::FONTS;
use home::home;
use insert::{draw, insert};
pub(crate) use pack::pack_tab;
pub use pack::{HomeStep, PACK_SLACK, RibbonLayout};
use page_layout::page_layout;
use review::review;
pub use shortcuts::shortcut;
pub use title_bar::{MAC_CONTENT_LEFT, MAC_TITLE_BAR, TITLE_LEFT_PAD, title_bar};
use title_bar::{TITLE_BAR_H, TITLE_CONTROL_H};
use view::view;

pub const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];
const _: () = assert!(TITLE_CONTROL_H < TITLE_BAR_H, "title-bar controls keep a margin above and below");

/// Draws the ribbon tabs and the active tab's card, resetting an unavailable contextual tab to Home.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let collapsed = app.ui.ribbon_collapsed;
    let h = pack::ribbon_panel_height(collapsed);
    // A contextual tab exists only while its object is selected (Excel hides Table Design once the
    // cell leaves the table); a remembered one that is gone would draw an empty ribbon.
    if CONTEXTUAL_TABS.contains(&app.ui.ribbon_tab.as_str()) && !contextual_tabs(app).contains(&app.ui.ribbon_tab.as_str()) {
        app.ui.ribbon_tab = "Home".to_string();
    }
    egui::Panel::top("ribbon")
        .exact_size(h)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            // Tabs row.
            let tabs_rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 32.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(8.0);
                    if app.chrome.file_tab {
                        file_menu(app, ui);
                    }
                    let mut tabs: Vec<&str> = TABS.to_vec();
                    let ctx_tabs = contextual_tabs(app);
                    tabs.extend(ctx_tabs.iter());
                    for tab in tabs {
                        let active = app.ui.ribbon_tab == tab;
                        let contextual = ctx_tabs.contains(&tab);
                        let font = theme::ui_font(13.5);
                        let label = app.ui.language.tr(tab);
                        let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x;
                        let (r, resp) = ui.allocate_exact_size(vec2(tw + 22.0, 30.0), Sense::click());
                        if resp.hovered() && !active {
                            ui.painter().rect_filled(r.shrink2(vec2(2.0, 4.0)), 5.0, t.hover);
                        }
                        let color = if contextual {
                            t.accent
                        } else if active {
                            t.text
                        } else {
                            t.text_dim
                        };
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, label, if active { theme::ui_bold(13.5) } else { font }, color);
                        if active {
                            let ul = Rect::from_center_size(pos2(r.center().x, r.bottom() - 3.0), vec2(tw.clamp(20.0, 40.0), 3.0));
                            ui.painter().rect_filled(ul, 1.5, t.accent);
                        }
                        if resp.clicked() {
                            if active && !collapsed {
                                app.ui.ribbon_collapsed = true;
                            } else {
                                app.ui.ribbon_tab = tab.to_string();
                                app.ui.ribbon_collapsed = false;
                            }
                        }
                        if resp.double_clicked() {
                            app.ui.ribbon_collapsed = !collapsed;
                        }
                        if app.keytips.prefix() == Some("")
                            && let Some(sequence) = keytip_table::tab_sequence(tab)
                        {
                            keytip(app, ui, sequence, pos2(r.center().x, r.bottom() - 1.0));
                        }
                    }
                });
            });
            if collapsed {
                return;
            }
            // The tab row above can be wider than a narrow window. The card stays inside the
            // window; groups that do not fit move into the overflow menu instead of being clipped.
            let left = ui.cursor().min.x;
            let screen = ui.ctx().content_rect();
            let fits = (screen.right() - left - (left - screen.left()).max(0.0)).max(0.0);
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width().min(fits), pack::card_height()));
            ui.painter().rect_filled(card, 10.0, t.ribbon);
            ui.painter().rect_stroke(card, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let inner = card.shrink2(vec2(10.0, 6.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                let budget = inner.width();
                match app.ui.ribbon_tab.as_str() {
                    "Home" => home(app, ui, budget),
                    "Insert" => insert(app, ui, budget),
                    "Draw" => draw(app, ui, budget),
                    "Page Layout" => page_layout(app, ui, budget),
                    "Formulas" => formulas(app, ui, budget),
                    "Data" => data(app, ui, budget),
                    "Review" => review(app, ui, budget),
                    "View" => view(app, ui, budget),
                    "Automate" => automate(app, ui, budget),
                    "Table Design" => table_design(app, ui, budget),
                    "Chart Design" => chart_design(app, ui, budget),
                    crate::paper_view::TAB => crate::paper_view::ribbon_tab(app, ui, budget),
                    _ => home(app, ui, budget),
                }
            });
        });
}

/// How much of the active tab's groups the ribbon card shows, recorded each frame for the scroll
/// arrows and for tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RibbonOverflow {
    /// Width of all the tab's groups.
    pub content: f32,
    /// Width of the card's content area.
    pub visible: f32,
    /// How far the groups are scrolled.
    pub offset: f32,
    /// Height of the tallest group (a column of small buttons) and of the content area.
    pub content_height: f32,
    pub visible_height: f32,
}

impl RibbonOverflow {
    /// Returns the stable id for the ribbon overflow state.
    pub(crate) fn id() -> egui::Id {
        egui::Id::unique("ribbon_overflow")
    }

    /// Last frame's overflow.
    pub fn get(ctx: &egui::Context) -> RibbonOverflow {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }

    /// Whether the groups are wider than the card.
    pub fn overflows(&self) -> bool {
        self.content > self.visible + 0.5
    }
}

/// The File tab opens the Backstage view (New, Open, Info, Save As, Print, Export, Options…), like
/// Excel's; the ribbon's tab and collapsed state are left as they were for when it closes.
fn file_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let label = egui::RichText::new(app.ui.language.tr("File")).font(theme::ui_font(13.5)).color(t.accent);
    let file = ui.add(egui::Button::new(label).frame(false).min_size(vec2(46.0, 30.0)));
    if file.clicked() {
        app.open_backstage(crate::backstage::Page::Home);
    }
    if app.keytips.prefix() == Some("")
        && let Some(sequence) = keytip_table::tab_sequence("File")
    {
        keytip(app, ui, sequence, pos2(file.rect.center().x, file.rect.bottom() - 1.0));
    }
}

/// System / Light / Dark, as in the View tab's Display theme box (`view.theme`).
fn theme_choices(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mode = app.ui.theme_mode();
    for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
        if ui.radio(mode == value, lang.tr(label)).clicked() {
            act(app, "view.theme", json!({"mode": value}));
            ui.close();
        }
    }
}

const CONTEXTUAL_TABS: [&str; 3] = ["Table Design", "Chart Design", crate::paper_view::TAB];

fn contextual_tabs(app: &SheetApp) -> Vec<&'static str> {
    let mut v = Vec::new();
    if let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        if sh.table_at(d.selection.active).is_some() {
            v.push("Table Design");
        }
        if app.selected_chart().is_some_and(|id| sh.charts.iter().any(|c| c.id == id)) {
            v.push("Chart Design");
        }
        if app.grid.hf_edit.is_some() {
            v.push(crate::paper_view::TAB);
        }
    }
    v
}

#[cfg(test)]
mod tooltip_tests {
    /// A ribbon tooltip names its control only; `widgets::tooltip_shortcut` adds the shortcut of
    /// the platform it runs on. A Command-key glyph written into the text would show on Windows
    /// and Linux, where the shortcut is a Ctrl or Alt one (AutoSum is Alt+=).
    #[test]
    fn tooltips_leave_the_shortcut_to_the_platform_lookup() {
        let sources = [
            include_str!("ribbon.rs"),
            include_str!("ribbon/automate.rs"),
            include_str!("ribbon/contextual.rs"),
            include_str!("ribbon/controls.rs"),
            include_str!("ribbon/data.rs"),
            include_str!("ribbon/formulas.rs"),
            include_str!("ribbon/galleries.rs"),
            include_str!("ribbon/home.rs"),
            include_str!("ribbon/insert.rs"),
            include_str!("ribbon/pack.rs"),
            include_str!("ribbon/page_layout.rs"),
            include_str!("ribbon/review.rs"),
            include_str!("ribbon/shortcuts.rs"),
            include_str!("ribbon/title_bar.rs"),
            include_str!("ribbon/view.rs"),
        ];
        let mac_only = sources.iter().flat_map(|s| s.lines()).filter(|l| l.contains('\u{2318}')).collect::<Vec<_>>();
        assert!(mac_only.is_empty(), "Command-key glyph in ribbon text: {mac_only:?}");
    }
}
