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
mod page_layout;
mod review;
mod shortcuts;
mod title_bar;
mod view;

use automate::automate;
use contextual::{chart_design, table_design};
pub(crate) use controls::{ComboResult, GROUP_CAPTION_H, HEADER_FOOTER_CAPTIONS, editable_combo, group};
use controls::{act, keytip};
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
    let h = if collapsed { 34.0 } else { 34.0 + 92.0 + GROUP_CAPTION_H };
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
            // Ribbon card. The row above it can be wider than a narrow window, so the card is kept
            // within the window (with the margin it has on the left), or its right scroll arrow
            // would be drawn off screen.
            let left = ui.cursor().min.x;
            let screen = ui.ctx().content_rect();
            let fits = (screen.right() - left - (left - screen.left()).max(0.0)).max(0.0);
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width().min(fits), 88.0 + GROUP_CAPTION_H));
            ui.painter().rect_filled(card, 10.0, t.ribbon);
            ui.painter().rect_stroke(card, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let inner = card.shrink2(vec2(10.0, 6.0));
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
                let scroll = area.show(ui, |ui| {
                    // Top-aligned so the caption band hangs under the controls.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
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
                    });
                });
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
mod group_caption_ui {
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use super::controls::{
        AUTOMATE_CAPTIONS, CHART_CAPTIONS, DATA_CAPTIONS, DRAW_CAPTIONS, FORMULAS_CAPTIONS, HOME_CAPTIONS, INSERT_CAPTIONS, PAGE_LAYOUT_CAPTIONS,
        REVIEW_CAPTIONS, TABLE_CAPTIONS, VIEW_CAPTIONS,
    };
    use crate::SheetApp;
    use crate::i18n::Language;

    fn harness(app: SheetApp) -> egui_kittest::Harness<'static, SheetApp> {
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1680.0, 900.0))
            .with_context_setup(|ctx| {
                ctx.set_fonts(crate::theme::pinned_font_definitions());
                crate::theme::apply(ctx, false);
            })
            // A text field on a later tab has no accessible name. This test checks group
            // captions, not that unrelated gap.
            .with_accessibility_check(false)
            .renderer(egui_kittest::LazyRenderer::Uninitialized { textures_delta: Default::default(), builder: None })
            .build_ui_state(
                |ui, app: &mut SheetApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
        h.run_steps(4);
        h
    }

    fn app_on(tab: &str, session: gridcraft_engine::Session) -> egui_kittest::Harness<'static, SheetApp> {
        let mut app = SheetApp::new(session, Default::default());
        app.ui.language = Language::De;
        app.ui.ribbon_tab = tab.to_string();
        app.use_prepared_fonts();
        harness(app)
    }

    fn assert_captions(tab: &str, h: &egui_kittest::Harness<'_, SheetApp>, keys: &[&str]) {
        assert!(!keys.is_empty(), "{tab}");
        for key in keys {
            let caption = Language::De.tr(key);
            assert!(!caption.is_empty(), "{tab} {key}");
            assert!(h.query_by_label(caption).is_some(), "{tab} does not show {caption} ({key})");
        }
    }

    #[test]
    fn start_formulas_data_and_view_show_a_caption_on_every_group() {
        let tabs: &[(&str, &[&str])] = &[
            ("Home", HOME_CAPTIONS.as_slice()),
            ("Formulas", FORMULAS_CAPTIONS.as_slice()),
            ("Data", DATA_CAPTIONS.as_slice()),
            ("View", VIEW_CAPTIONS.as_slice()),
        ];
        for (tab, keys) in tabs {
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            let h = app_on(tab, session);
            assert_captions(tab, &h, keys);
        }
        assert_eq!(Language::De.tr("Ribbon group|Type format"), "Schriftformat");
        assert_eq!(Language::De.tr("Ribbon group|Number format"), "Zahlenformat");
    }

    #[test]
    fn the_other_visible_tabs_show_a_caption_on_every_group() {
        let tabs: &[(&str, &[&str])] = &[
            ("Insert", INSERT_CAPTIONS.as_slice()),
            ("Draw", DRAW_CAPTIONS.as_slice()),
            ("Page Layout", PAGE_LAYOUT_CAPTIONS.as_slice()),
            ("Review", REVIEW_CAPTIONS.as_slice()),
            ("Automate", AUTOMATE_CAPTIONS.as_slice()),
        ];
        for (tab, keys) in tabs {
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            let h = app_on(tab, session);
            assert_captions(tab, &h, keys);
        }
        let mut table = gridcraft_engine::Session::new();
        table.new_workbook();
        assert!(table.execute("range.setValues", json!({"range": "A1", "values": [["Nord", "Menge"], ["Ost", 1], ["West", 2]]})).is_ok());
        assert!(table.execute("insert.table", json!({"range": "A1:B3", "header": true})).is_ok());
        assert!(table.execute("selection.set", json!({"range": "A2"})).is_ok());
        let h = app_on("Table Design", table);
        assert_eq!(h.state().ui.ribbon_tab, "Table Design");
        assert_captions("Table Design", &h, TABLE_CAPTIONS.as_slice());
        let mut chart = gridcraft_engine::Session::new();
        chart.new_workbook();
        assert!(chart.execute("range.setValues", json!({"range": "A1", "values": [["Nord", "Menge"], ["Ost", 4], ["West", 6]]})).is_ok());
        let made = chart.execute("insert.chart", json!({"range": "A1:B3", "type": "column"}));
        let id = made.as_ref().ok().and_then(|v| v.get("chart")).and_then(|c| c.as_u64());
        assert!(id.is_some(), "chart id");
        let id = id.unwrap_or(0);
        let mut h = app_on("Chart Design", chart);
        h.state_mut().select_object_id(Some(id as u32));
        h.state_mut().ui.ribbon_tab = "Chart Design".to_string();
        h.run_steps(3);
        assert_eq!(h.state().ui.ribbon_tab, "Chart Design");
        assert_captions("Chart Design", &h, CHART_CAPTIONS.as_slice());
    }
}

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
