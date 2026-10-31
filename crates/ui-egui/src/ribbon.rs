//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::keytips::keytip_table;
use crate::theme::{self, Cat, Tokens};
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
pub(crate) use controls::{ComboResult, editable_combo};
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

pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ch = TITLE_CONTROL_H;
    egui::Panel::top("title_bar").exact_size(TITLE_BAR_H).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Dragging the empty title area moves the window.
        let bg = ui.interact(rect, ui.id().with("drag_title"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.horizontal_centered(|ui| {
            ui.add_space(TITLE_LEFT_PAD);
            // AutoSave toggle (saves after each change when the workbook has a file).
            ui.label(egui::RichText::new(app.ui.language.tr("AutoSave")).font(theme::ui_font(12.5)).color(t.text_dim));
            let on = app.ui.autosave;
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
            ui.painter().rect_filled(r, 8.0, if on { t.accent } else { Color32::TRANSPARENT });
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, if on { t.accent } else { t.text_dim }), StrokeKind::Inside);
            let knob = if on { pos2(r.right() - 8.0, r.center().y) } else { pos2(r.left() + 8.0, r.center().y) };
            ui.painter().circle_filled(knob, 5.0, if on { Color32::WHITE } else { t.text_dim });
            if resp.on_hover_text(app.ui.language.tr("AutoSave: save after every change (workbooks saved to a file)")).clicked() {
                app.toggle_autosave();
            }
            ui.add_space(6.0);
            let __k = icon_button(ui, Icon::Home, t.text_dim, "Home", vec2(26.0, ch));
            control_keytip(app, ui, &__k, "start");
            if __k.clicked() {
                app.open_dialog("start", json!({}));
            }
            if icon_button(ui, Icon::Save, t.text_dim, "Save", vec2(26.0, ch)).clicked()
                // File operations must include the cell the user is still editing (as the File > Save menu item already does).
                && app.commit_edit(0, 0, false, false)
            {
                app.run_or_alert("file.save", json!({}));
            }
            // File › Close next to Save (a host embedding GridCraft reports the closing back):
            // `file.close` asks about unsaved changes first, like Ctrl+S does on the desktop.
            if icon_button(ui, Icon::Close, t.text_dim, "Close", vec2(26.0, ch)).clicked() && app.commit_edit(0, 0, false, false) {
                app.run_or_alert("file.close", json!({}));
            }
            // The commands' own availability: there must be a step, and no undo group may be open on
            // the document (an action that spans several calls is still running).
            let can_undo = gridcraft_engine::cmd::can_undo(&app.session).is_ok();
            let can_redo = gridcraft_engine::cmd::can_redo(&app.session).is_ok();
            // Named and marked enabled or not for assistive technology (and the UI tests).
            let undo = icon_button(ui, Icon::Undo, if can_undo { t.text_dim } else { t.text_disabled }, "Undo", vec2(26.0, ch));
            undo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, can_undo, "Undo"));
            if undo.clicked() && can_undo {
                app.run_or_alert("edit.undo", json!({}));
            }
            let undo_list = icon_button(ui, Icon::Chevron, t.text_dim, "Undo list", vec2(14.0, ch));
            egui::Popup::menu(&undo_list).show(|ui| {
                let labels: Vec<String> = if can_undo {
                    app.session.active().map(|d| d.undo.iter().rev().take(20).map(|e| e.label.clone()).collect()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if labels.is_empty() {
                    ui.label("Can't Undo");
                }
                for (i, l) in labels.iter().enumerate() {
                    if ui.button(format!("Undo {l}")).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
            let redo = icon_button(ui, Icon::Redo, if can_redo { t.text_dim } else { t.text_disabled }, "Redo", vec2(26.0, ch));
            redo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, can_redo, "Redo"));
            if redo.clicked() && can_redo {
                app.run_or_alert("edit.redo", json!({}));
            }
            let more = icon_button(ui, Icon::More, t.text_dim, "More commands", vec2(26.0, ch));
            egui::Popup::menu(&more).show(|ui| {
                for (label, id) in [
                    ("New Workbook", "file.new"),
                    ("Open…", "file.open"),
                    ("Save As…", "file.saveAs"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Calculate Now", "formulas.calculateNow"),
                ] {
                    if ui.button(label).clicked() {
                        app.run_or_alert(id, json!({}));
                    }
                }
                if ui.button("Print…").clicked() {
                    app.print_now(json!({}));
                }
            });
            let left_end = ui.min_rect().right();
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let __k = icon_button(ui, Icon::Search, t.text_dim, "Search commands", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "commandSearch");
                if __k.clicked() {
                    app.open_dialog("commandSearch", json!({}));
                }
                let share = small_button(ui, Icon::Share, "Share", "Share: export or save a copy", true);
                egui::Popup::menu(&share).show(|ui| {
                    if ui.button("Export as PDF…").clicked() {
                        app.run_or_alert("file.exportPdf", json!({}));
                    }
                    for (label, fmt) in
                        [("Save a Copy as Excel Workbook (.xlsx)…", "xlsx"), ("Export as CSV…", "csv"), ("Export as Web Page (.html)…", "html")]
                    {
                        if ui.button(label).clicked() {
                            app.open_dialog("saveCopy", json!({"format": fmt}));
                        }
                    }
                });
                let __k = icon_button(ui, Icon::Comment, t.text_dim, "Comments", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "comments");
                if __k.clicked() {
                    app.open_dialog("comments", json!({}));
                }
                // Quick light/dark switch (#194); right-click offers System as well. The same
                // choice lives in File › Display theme and the View tab.
                let lang = app.ui.language;
                let theme = icon_button(ui, Icon::Appearance, t.text_dim, "Dark Mode", vec2(28.0, 26.0));
                let dark = ui.ctx().theme() == egui::Theme::Dark;
                theme.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, dark, lang.tr("Dark Mode")));
                if theme.clicked() {
                    act(app, "view.darkMode", json!({}));
                }
                theme.context_menu(|ui| theme_choices(app, ui));
                ui.min_rect().left()
            });
            // Centred title, cut with "…" so it never runs under the controls on either side.
            let title = app.session.active().map(|d| d.display_title()).unwrap_or_default();
            let dirty = app.session.active().is_some_and(|d| d.is_dirty());
            let max_w = title_max_width(rect.center().x, left_end, right.inner);
            paint_title(ui, rect.center(), &title, dirty, max_w, t.text);
        });
    });
}

/// Lay out the title truncated to fit `max_w` together with the unsaved marker (when `dirty`).
fn title_galleys(
    painter: &egui::Painter,
    title: &str,
    dirty: bool,
    max_w: f32,
    color: Color32,
) -> (std::sync::Arc<egui::Galley>, Option<std::sync::Arc<egui::Galley>>) {
    let font = theme::ui_bold(13.0);
    let dot = dirty.then(|| painter.layout_no_wrap(" •".to_string(), font.clone(), color));
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_w - dot_w).max(0.0));
    (painter.layout_job(job), dot)
}

/// Gap kept between the title and the controls beside it.
const TITLE_GAP: f32 = 12.0;

/// The widest the centred title may be without reaching the controls that end at `left_end` or
/// start at `right_start`.
fn title_max_width(center_x: f32, left_end: f32, right_start: f32) -> f32 {
    let half = (center_x - left_end).min(right_start - center_x) - TITLE_GAP;
    (2.0 * half).max(0.0)
}

/// Paint the workbook title centred on `center`, truncated with "…" to `max_w`. The unsaved
/// marker stays visible however short the title gets.
fn paint_title(ui: &Ui, center: egui::Pos2, title: &str, dirty: bool, max_w: f32, color: Color32) {
    let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, color);
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let w = galley.size().x + dot_w;
    let left = center.x - w / 2.0;
    let top = center.y - galley.size().y / 2.0;
    let galley_w = galley.size().x;
    ui.painter().galley(pos2(left, top), galley, color);
    if let Some(dot) = dot {
        ui.painter().galley(pos2(left + galley_w, top), dot, color);
    }
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let collapsed = app.ui.ribbon_collapsed;
    let h = if collapsed { 34.0 } else { 34.0 + 92.0 };
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
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width().min(fits), 88.0));
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
                    ui.horizontal(|ui| {
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
        if app.selected_chart.is_some_and(|id| sh.charts.iter().any(|c| c.id == id)) {
            v.push("Chart Design");
        }
        if app.grid.hf_edit.is_some() {
            v.push(crate::paper_view::TAB);
        }
    }
    v
}
