//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::keytips::keytip_table;
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, big_split_button, color_palette, icon_button, small_button, small_split_button, split_button, toggle_button};

mod automate;
mod contextual;
mod controls;
mod data;
mod formulas;
mod galleries;
mod home;
mod insert;
mod page_layout;
mod review;
mod shortcuts;
mod title_bar;
mod view;

use automate::automate;
use contextual::{chart_design, table_design};
pub(crate) use controls::sep;
use controls::{CAPTION_H, GROUP_H};
pub(crate) use controls::{ComboResult, editable_combo};
use controls::{act, begin_groups, end_group, keytip};
use data::data;
use formulas::formulas;
pub use home::FONTS;
use home::home;
use insert::{draw, insert};
use page_layout::page_layout;
use review::review;
pub use shortcuts::shortcut;
pub use title_bar::{MAC_CONTENT_LEFT, MAC_TITLE_BAR, TITLE_LEFT_PAD, title_bar};
use title_bar::{TITLE_BAR_H, TITLE_CONTROL_H};
use view::view;

pub const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];
const _: () = assert!(TITLE_CONTROL_H < TITLE_BAR_H, "title-bar controls keep a margin above and below");

/// Draws the ribbon tabs and the active tab's horizontally scrollable card, resetting an unavailable contextual tab to Home.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let collapsed = app.ui.ribbon_collapsed;
    let h = if collapsed { TAB_ROW_H } else { TAB_ROW_H + CARD_H + 8.0 };
    // A contextual tab exists only while its object is selected (Excel hides Table Design once the
    // cell leaves the table); a remembered one that is gone would draw an empty ribbon.
    if CONTEXTUAL_TABS.contains(&app.ui.ribbon_tab.as_str()) && !contextual_tabs(app).contains(&app.ui.ribbon_tab.as_str()) {
        app.ui.ribbon_tab = "Home".to_string();
    }
    egui::Panel::top("ribbon")
        .show_separator_line(false)
        .exact_size(h)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 8 }))
        .show(ui, |ui| {
            // Tabs row.
            let tabs_rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), TAB_ROW_H));
            // The active tab is a folder tab that joins the card below it.
            let mut active_rect: Option<Rect> = None;
            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(6.0);
                    // With the title bar, its Home button opens the File view; the File tab is
                    // only drawn when a host leaves the title bar out.
                    if app.chrome.file_tab && !app.chrome.title_bar {
                        file_menu(app, ui);
                    }
                    let ctx_tabs = contextual_tabs(app);
                    // Contextual tabs announce themselves: each one has an appearance (0 to 1) that
                    // eases in when its object is selected and out when it is not, and a tab that
                    // is leaving is still drawn (but no longer clickable) until it has faded.
                    let mut tabs: Vec<(&str, f32)> = TABS.iter().map(|tab| (*tab, 1.0)).collect();
                    for tab in CONTEXTUAL_TABS {
                        let shown = contextual_appearance(ui.ctx(), tab, ctx_tabs.contains(&tab));
                        if shown > 0.0 {
                            tabs.push((tab, shown));
                        }
                    }
                    for (tab, shown) in tabs {
                        let active = app.ui.ribbon_tab == tab;
                        let contextual = CONTEXTUAL_TABS.contains(&tab);
                        let leaving = contextual && !ctx_tabs.contains(&tab);
                        let font = theme::ui_font(12.5);
                        let label = app.ui.language.tr(tab);
                        let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x;
                        // The tab's slot widens as it appears, so the tabs before the card make room.
                        let (r, resp) =
                            ui.allocate_exact_size(vec2((tw + 24.0) * shown, TAB_ROW_H - 4.0), if leaving { Sense::hover() } else { Sense::click() });
                        if contextual && shown < 1.0 {
                            // Slide in from the right and fade, then draw the underline in.
                            let mut painter = ui.painter().clone();
                            painter.multiply_opacity(shown);
                            let center = pos2(r.left() + (tw + 24.0) / 2.0 + (1.0 - shown) * 10.0, r.center().y);
                            painter.text(center, Align2::CENTER_CENTER, label, font, t.accent);
                            paint_contextual_underline(&painter, &t, center.x, r.bottom(), tw, ((shown - 0.35) / 0.65).clamp(0.0, 1.0));
                            ui.ctx().request_repaint();
                            continue;
                        }
                        if contextual && !active {
                            paint_contextual_underline(ui.painter(), &t, r.center().x, r.bottom(), tw, 1.0);
                        }
                        if resp.hovered() && !active {
                            ui.painter().rect_filled(r, 7.0, t.hover);
                        }
                        if active {
                            // The active tab is a neutral pill with a bold label, like the active sheet
                            // tab, gliding to a newly chosen tab.
                            let pill = if crate::widgets::animations_on(ui.ctx()) {
                                let ctx = ui.ctx();
                                let left = ctx.animate_value_with_time(egui::Id::unique("ribbon.pill.left"), r.left(), 0.14);
                                let right = ctx.animate_value_with_time(egui::Id::unique("ribbon.pill.right"), r.right(), 0.14);
                                Rect::from_min_max(pos2(left, r.top()), pos2(right, r.bottom()))
                            } else {
                                r
                            };
                            ui.painter().rect_filled(pill, 7.0, t.pressed);
                            active_rect = Some(r);
                        }
                        let color = if contextual {
                            t.accent
                        } else if active {
                            t.text
                        } else {
                            t.text.lerp_to_gamma(t.text_dim, 0.3)
                        };
                        let text_center = r.center();
                        ui.painter().text(text_center, Align2::CENTER_CENTER, label, if active { theme::ui_bold(12.5) } else { font }, color);
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
            // Ribbon card. The row above it can be wider than a narrow window, so the card is kept
            // within the window (with the margin it has on the left), or its right scroll arrow
            // would be drawn off screen.
            let left = ui.cursor().min.x;
            let screen = ui.ctx().content_rect();
            let fits = (screen.right() - left - (left - screen.left()).max(0.0)).max(0.0);
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width().min(fits), CARD_H));
            ui.painter().rect_filled(card, 10.0, t.ribbon);
            ui.painter().rect_stroke(card, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let _ = active_rect;
            let inner = Rect::from_min_max(card.min + vec2(6.0, 8.0), card.max - vec2(6.0, 8.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                // One scroll state per tab: a tab opened after scrolling another starts at its
                // left edge, and a tab keeps its own offset while others are used.
                let tab = app.ui.ribbon_tab.clone();
                let mut area = egui::ScrollArea::horizontal()
                    .id_salt(("ribbon_scroll", tab.as_str()))
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                // egui 0.37 keeps `ScrollArea`'s offset private; `scroll_offset` is the supported
                // way to move it. An arrow click records the requested offset for the next frame.
                // Expire it if this tab stops rendering, rather than replaying it much later.
                if let Some((frame, x)) = ui.ctx().data_mut(|d| d.remove_temp::<(u64, f32)>(RibbonOverflow::scroll_target_id(tab.as_str())))
                    && ui.ctx().cumulative_frame_nr().saturating_sub(frame) <= 1
                {
                    area = area.horizontal_scroll_offset(x);
                }
                // The new tab's groups fade in.
                if crate::widgets::animations_on(ui.ctx()) {
                    let changed = ui.ctx().data_mut(|d| {
                        let key = egui::Id::unique("ribbon.shown_tab");
                        let was = d.get_temp::<String>(key);
                        d.insert_temp(key, tab.clone());
                        was.is_some_and(|w| w != tab)
                    });
                    let fade = egui::Id::unique(("ribbon.fade", tab.as_str()));
                    if changed {
                        ui.ctx().animate_value_with_time(fade, 0.0, 0.0);
                    }
                    let a = ui.ctx().animate_value_with_time(fade, 1.0, 0.16);
                    ui.multiply_opacity(a);
                    crate::icons::set_ribbon_fade(ui.ctx(), a);
                }
                let scroll = area.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                        begin_groups(ui);
                        match app.ui.ribbon_tab.as_str() {
                            "Home" => home(app, ui),
                            "Insert" => insert(app, ui),
                            "Draw" => draw(app, ui),
                            "Page Layout" => page_layout(app, ui),
                            "Formulas" => formulas(app, ui),
                            "Data" => data(app, ui),
                            "Review" => review(app, ui),
                            "View" => view(app, ui),
                            "Automate" => automate(app, ui),
                            "Table Design" => table_design(app, ui),
                            "Chart Design" => chart_design(app, ui),
                            crate::paper_view::TAB => crate::paper_view::ribbon_tab(app, ui),
                            _ => home(app, ui),
                        }
                        end_group(ui, last_group(app.ui.ribbon_tab.as_str()));
                    });
                });
                crate::icons::set_ribbon_fade(ui.ctx(), 1.0);
                scroll_arrows(ui, &t, tab.as_str(), scroll);
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
    fn id() -> egui::Id {
        egui::Id::unique("ribbon_overflow")
    }

    /// Where an arrow click asks the next frame's `ScrollArea` to scroll to.
    fn scroll_target_id(tab: &str) -> egui::Id {
        egui::Id::unique(("ribbon_scroll_target", tab))
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

/// When the tab's groups are wider than the card, arrows at its edges scroll them (horizontal wheel or
/// trackpad input over the card still does too).
fn scroll_arrows(ui: &mut Ui, t: &Tokens, tab: &str, scroll: egui::scroll_area::ScrollAreaOutput<()>) {
    let inner = scroll.inner_rect;
    let max = (scroll.content_size.x - inner.width()).max(0.0);
    let overflow = RibbonOverflow {
        content: scroll.content_size.x,
        visible: inner.width(),
        offset: scroll.state.clamped_offset().x,
        content_height: scroll.content_size.y,
        visible_height: inner.height(),
    };
    ui.ctx().data_mut(|d| d.insert_temp(RibbonOverflow::id(), overflow));
    if !overflow.overflows() {
        return;
    }
    let step = (inner.width() * 0.6).max(60.0);
    let mut target = None;
    for (left, show) in [(true, overflow.offset > 0.5), (false, overflow.offset < max - 0.5)] {
        if !show {
            continue;
        }
        let r = if left {
            Rect::from_min_size(inner.left_top(), vec2(18.0, inner.height()))
        } else {
            Rect::from_min_size(pos2(inner.right() - 18.0, inner.top()), vec2(18.0, inner.height()))
        };
        let resp = ui.interact(r, ui.scope_id().with(("ribbon_scroll_arrow", left)), Sense::click());
        ui.painter().rect_filled(r, 4.0, if resp.hovered() { t.hover } else { t.ribbon });
        ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), if left { Icon::Left } else { Icon::Right }, t.text);
        let name = if left { "Scroll ribbon left" } else { "Scroll ribbon right" };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        if resp.on_hover_text(crate::i18n::current(ui.ctx()).tr(name)).clicked() {
            target = Some(if left { overflow.offset - step } else { overflow.offset + step });
        }
    }
    if let Some(x) = target {
        let frame = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|d| d.insert_temp(RibbonOverflow::scroll_target_id(tab), (frame, x.clamp(0.0, max))));
        ui.ctx().request_repaint();
    }
}

/// The File tab opens the Backstage view (New, Open, Info, Save As, Print, Export, Options…), like
/// Excel's; the ribbon's tab and collapsed state are left as they were for when it closes.
fn file_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let label = egui::RichText::new(app.ui.language.tr("File")).font(theme::ui_bold(12.5)).color(t.accent);
    let file = ui.add(egui::Button::new(label).frame(false).min_size(vec2(46.0, TAB_ROW_H - 4.0)));
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
    for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark"), ("darkGreen", "Dark Green"), ("omarchy", "Omarchy")] {
        if ui.radio(mode == value, lang.tr(label)).clicked() {
            act(app, "view.theme", json!({"mode": value}));
            ui.close();
        }
    }
}

/// The ribbon card: a row of controls with a caption line under each group.
/// Height of the row of ribbon tabs (the folder tab is 4 px shorter).
const TAB_ROW_H: f32 = 30.0;
const CARD_H: f32 = GROUP_H + CAPTION_H + 16.0;

/// The caption of a tab's last group (the others are named where their separator is drawn).
fn last_group(tab: &str) -> &'static str {
    match tab {
        "Insert" => "Text",
        "Draw" => "Convert",
        "Page Layout" => "Sheet Options",
        "Formulas" => "Calculation",
        "Data" => "Solver",
        "Review" => "Protect",
        "View" => "Interface",
        "Automate" => "GridCraft",
        "Table Design" => "Table Styles",
        "Chart Design" => "Chart",
        crate::paper_view::TAB => "Options",
        _ => "Editing",
    }
}

const CONTEXTUAL_TABS: [&str; 3] = ["Table Design", "Chart Design", crate::paper_view::TAB];

/// How far contextual tab `tab` has appeared (0 hidden, 1 fully shown): it eases in over 200 ms
/// when it becomes `present` and fades out when it goes, or snaps when animations are off.
fn contextual_appearance(ctx: &egui::Context, tab: &str, present: bool) -> f32 {
    let id = egui::Id::unique(("ribbon.contextual", tab));
    if crate::widgets::animations_on(ctx) {
        ctx.animate_bool_with_time_and_easing(id, present, 0.2, egui::emath::easing::cubic_out)
    } else {
        // Keep the animation's memory in step, so turning animations on later does not replay it.
        ctx.animate_bool_with_time(id, present, 0.0);
        f32::from(u8::from(present))
    }
}

/// The short accent line under a contextual tab's label, drawn out from its centre as `progress`
/// goes from 0 to 1.
fn paint_contextual_underline(p: &egui::Painter, t: &Tokens, center_x: f32, bottom: f32, label_w: f32, progress: f32) {
    if progress <= 0.0 {
        return;
    }
    let w = label_w.clamp(12.0, 18.0) * progress;
    let line = Rect::from_center_size(pos2(center_x, bottom - 1.0), vec2(w, 2.0));
    p.rect_filled(line, 1.0, t.accent.gamma_multiply(0.75));
}

fn contextual_tabs(app: &SheetApp) -> Vec<&'static str> {
    let mut v = Vec::new();
    if let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        if sh.table_at(d.selection.active).is_some() {
            v.push("Table Design");
        }
        if app.selected_chart.is_some_and(|id| sh.charts.iter().any(|c| c.id == id)) {
            v.push("Chart Design");
        }
        if app.grid.hf_edit.is_some() {
            v.push(crate::paper_view::TAB);
        }
    }
    v
}

#[cfg(test)]
mod contextual_appearance_tests {
    use super::contextual_appearance;

    /// Runs one frame at `time` seconds and returns the tab's appearance.
    fn frame(ctx: &egui::Context, time: f64, present: bool) -> f32 {
        let mut shown = 0.0;
        let input = egui::RawInput { time: Some(time), ..Default::default() };
        // No renderer here: drop the frame's texture uploads explicitly (egui asserts in debug).
        ctx.run_ui(input, |ui| shown = contextual_appearance(ui.ctx(), "Table Design", present)).textures_delta.clear();
        shown
    }

    #[test]
    fn a_contextual_tab_eases_in_over_200_ms_and_fades_out() {
        let ctx = egui::Context::default();
        assert_eq!(frame(&ctx, 0.0, false), 0.0);
        let early = frame(&ctx, 0.05, true);
        assert!(early > 0.0 && early < 1.0, "part way in: {early}");
        let later = frame(&ctx, 0.12, true);
        assert!(later > early, "still easing in: {later}");
        assert_eq!(frame(&ctx, 0.5, true), 1.0);
        let leaving = frame(&ctx, 0.55, false);
        assert!(leaving > 0.0 && leaving < 1.0, "fading out: {leaving}");
        assert_eq!(frame(&ctx, 1.0, false), 0.0);
    }

    #[test]
    fn without_animations_a_contextual_tab_snaps() {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("gridcraft.animations"), false));
        assert_eq!(frame(&ctx, 0.0, false), 0.0);
        assert_eq!(frame(&ctx, 0.01, true), 1.0);
        assert_eq!(frame(&ctx, 0.02, false), 0.0);
    }
}
