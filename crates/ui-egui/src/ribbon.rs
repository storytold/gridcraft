//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use std::collections::HashMap;

use egui::{Align2, Color32, Key, Modifiers, Order, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::keytips::keytip_table;
use crate::theme::{self, Cat, Tokens};
use crate::widgets::{
    big_button, big_button_width, big_split_button, color_palette, icon_button, launcher_button, small_button, small_split_button,
    small_toggle_button, split_button, toggle_button,
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
pub(crate) use controls::{ComboResult, editable_combo};
use controls::{act, keytip, sep};
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

/// Width of the separator drawn between two ribbon groups (matches `controls::sep`).
const GROUP_SEP_W: f32 = 13.0;
/// Spacing between the groups of a ribbon row (and inside them).
const GROUP_ROW_SPACING: f32 = 2.0;
/// What the separator before a group adds to the row: its width and the spacing on both sides.
const GROUP_LEAD: f32 = GROUP_SEP_W + 2.0 * GROUP_ROW_SPACING;

/// Draws the ribbon tabs and the active tab's horizontally scrollable card, resetting an unavailable contextual tab to Home.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    // A collapsed group's dropdown can be open when the ribbon stops drawing it (the tab changed
    // without a pointer click carrying the switch, or the group expanded): only this frame's
    // named launcher ever renders, so nothing there would ever clear it.
    ui.ctx().data_mut(|d| {
        let open: Option<(String, String)> = d.get_temp(group_dropdown_open_id()).unwrap_or_default();
        if open.as_ref().is_some_and(|(tab, _)| tab != &app.ui.ribbon_tab) {
            d.insert_temp::<Option<(String, String)>>(group_dropdown_open_id(), None);
        }
    });
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
                // Fresh measurements for this frame's row; `group` fills the vec in as it renders.
                ui.ctx().data_mut(|d| d.insert_temp(group_measures_id(), Vec::<GroupMeasure>::new()));
                // And this tab's collapse plan, where `group` can find it. Three regimes:
                // · Key tips being typed, or a key tip that will open a control's popup this
                //   frame (consumed by the control as the body renders below): every group
                //   renders expanded, so each control keeps its key tip badge and its popup
                //   anchor — the card scrolls when that row is too wide, as without the collapse
                //   engine.
                // · Any popup open otherwise: the plan that was in use when it opened stays
                //   frozen. A combo menu inside a group dropdown keeps that group collapsed (its
                //   dropdown is an Area, not a popup), and a popup a key tip opened keeps the
                //   expanded row it was anchored in.
                // · Otherwise the tab's stored plan, freshly computed each frame below.
                let invoking = app.ui.keytip_invoke.is_some();
                let popup_open = egui::Popup::is_any_open(ui.ctx());
                let plan = if app.keytips.prefix().is_some() || invoking {
                    Vec::new()
                } else if popup_open {
                    ui.ctx().data(|d| d.get_temp::<Vec<String>>(group_plan_frozen_id())).unwrap_or_default()
                } else {
                    ui.ctx().data(|d| d.get_temp::<Vec<String>>(group_plan_id(&app.ui.ribbon_tab))).unwrap_or_default()
                };
                ui.ctx().data_mut(|d| d.insert_temp(group_plan_active_id(), plan.clone()));
                ui.ctx().data_mut(|d| d.insert_temp(group_plan_frozen_id(), plan));
                ui.ctx().data_mut(|d| d.insert_temp(active_tab_id(), app.ui.ribbon_tab.clone()));
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
                        ui.spacing_mut().item_spacing = vec2(GROUP_ROW_SPACING, GROUP_ROW_SPACING);
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
                // File the frame's group measurements into the stored overflow, in render order.
                let groups = group_measures(ui.ctx()).into_iter().map(|m| (m.name, m.width)).collect();
                ui.ctx().data_mut(|d| {
                    let mut overflow = d.get_temp::<RibbonOverflow>(RibbonOverflow::id()).unwrap_or_default();
                    overflow.groups = groups;
                    d.insert_temp(RibbonOverflow::id(), overflow);
                });
                // The collapse engine: with this frame's measurements, keep what fits expanded
                // and collapse the rest rightmost-first; the next frame renders from the plan.
                // Key-tip navigation renders everything expanded instead (above), so it never
                // files a plan of its own; nor does a frame that ends with a popup open or a
                // key tip invoke pending — a popup must not have its anchor group collapsed (or
                // re-expanded) out from under it while it is open.
                if !(app.keytips.prefix().is_some() || popup_open || invoking) {
                    let measures = group_measures(ui.ctx());
                    let budget = RibbonOverflow::get(ui.ctx()).visible;
                    let current: Vec<String> = ui.ctx().data(|d| d.get_temp(group_plan_id(&app.ui.ribbon_tab))).unwrap_or_default();
                    let plan = plan_for(&measures, budget, &current);
                    ui.ctx().data_mut(|d| d.insert_temp(group_plan_id(&app.ui.ribbon_tab), plan));
                }
            });
        });
}

/// How much of the active tab's groups the ribbon card shows, recorded each frame for the scroll
/// arrows and for tests.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// The tab's groups this frame, in render order, with each one's width.
    pub groups: Vec<(String, f32)>,
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
        groups: Vec::new(),
    };
    ui.ctx().data_mut(|d| d.insert_temp(RibbonOverflow::id(), overflow.clone()));
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

/// One ribbon group's measured layout for this frame, recorded by [`group`] in render order.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupMeasure {
    /// The group's name.
    pub name: String,
    /// Width of the group's row slot this frame, its lead separator included. While the group
    /// renders collapsed (its body not laid out) this is its cached expanded width, so the plan
    /// always computes with the width a collapse hands back.
    pub width: f32,
    /// Width of the group's collapsed row: the launcher button plus the lead separator, whether
    /// the group renders collapsed (measured) or expanded (what its launcher would take).
    pub launcher: f32,
}

/// Where this frame's [`GroupMeasure`]s live between `group` recording them and `show()` filing
/// them into the stored [`RibbonOverflow`].
pub fn group_measures_id() -> egui::Id {
    egui::Id::unique("ribbon_group_measures")
}

/// The active tab's groups as measured this frame, in render order (empty if no ribbon row was
/// drawn yet).
pub fn group_measures(ctx: &egui::Context) -> Vec<GroupMeasure> {
    ctx.data(|d| d.get_temp(group_measures_id())).unwrap_or_default()
}

/// Where a tab's collapsed-group plan lives: the group names rendered as launchers (empty or unset
/// = every group expanded). The collapse engine writes it; [`group`] consults the active tab's
/// copy, which [`show`] re-seeds each frame.
pub fn group_plan_id(tab: &str) -> egui::Id {
    egui::Id::unique(("ribbon_group_plan", tab))
}

/// The slack a collapse plan must leave before it hands a launcher its body back, so a group at
/// the fitting boundary does not collapse and expand again every other frame.
const COLLAPSE_HYSTERESIS: f32 = 24.0;

/// The width the row of `measures` needs with `collapsed` rendered as launchers: each collapsed
/// group contributes its launcher width, each expanded one its body.
fn row_width(measures: &[GroupMeasure], collapsed: &[String]) -> f32 {
    measures.iter().map(|m| if collapsed.iter().any(|n| n == &m.name) { m.launcher } else { m.width }).sum()
}

/// The collapse plan for a tab whose groups measured `measures` into a `budget`-wide card: the
/// names to render as launchers so the row fits. Groups already collapsed in `current` (the
/// tab's previous plan) stay collapsed unless there is room; the rest collapse rightmost-first,
/// and a group whose launcher saves nothing (`saving <= 0.5`) is never collapsed. Expansion runs
/// leftmost-first and only with [`COLLAPSE_HYSTERESIS`] to spare, so plans are stable at the
/// fitting boundary. The returned names follow `measures` (render) order, and every loop walks
/// the groups at most once.
fn plan_for(measures: &[GroupMeasure], budget: f32, current: &[String]) -> Vec<String> {
    let saving = |m: &GroupMeasure| (m.width - m.launcher).max(0.0);
    // Groups already collapsed keep their launcher; a group whose measures show no saving is
    // never worth a launcher.
    let mut collapsed: Vec<String> =
        measures.iter().filter(|m| saving(m) > 0.5 && current.iter().any(|n| n == &m.name)).map(|m| m.name.clone()).collect();
    // Expand leftmost first, and only with the hysteresis margin.
    while let Some(next) = measures.iter().find(|m| collapsed.iter().any(|n| n == &m.name)) {
        let expanded = row_width(measures, &collapsed) - next.launcher + next.width;
        if expanded <= budget - COLLAPSE_HYSTERESIS {
            collapsed.retain(|n| n != &next.name);
        } else {
            break;
        }
    }
    // Collapse rightmost first until the row fits.
    while row_width(measures, &collapsed) > budget {
        let Some(candidate) = measures.iter().rev().find(|m| !collapsed.iter().any(|n| n == &m.name) && saving(m) > 0.5) else {
            break;
        };
        collapsed.push(candidate.name.clone());
    }
    // In render order, so plans read like the row they describe.
    measures.iter().filter(|m| collapsed.iter().any(|n| n == &m.name)).map(|m| m.name.clone()).collect()
}

/// The tab's computed plan (the groups rendered as launchers), in render order; empty when no
/// plan was computed yet. Test getter for what [`show`] writes to [`group_plan_id`].
pub fn collapsed_names(ctx: &egui::Context, tab: &str) -> Vec<String> {
    ctx.data(|d| d.get_temp::<Vec<String>>(group_plan_id(tab))).unwrap_or_default()
}

/// Where [`show`] files the active tab's [`group_plan_id`] plan for `group` to read (`group`
/// renders without knowing which tab it is on).
fn group_plan_active_id() -> egui::Id {
    egui::Id::unique("ribbon_group_plan_active")
}

/// Where the plan in use when the currently open popup opened lives: while any popup is open
/// the active plan is frozen to it, so a combo menu inside a group dropdown keeps that group
/// collapsed (its dropdown is an `egui::Area`, not a popup, and would otherwise never re-render)
/// and a popup opened from the expanded row keeps its anchor.
fn group_plan_frozen_id() -> egui::Id {
    egui::Id::unique("ribbon_group_plan_frozen")
}

/// Where [`show`] files the active tab's name for [`group`] to read, written next to
/// [`group_plan_active_id`]'s plan each frame (same reason: `group` renders without knowing
/// which tab it is on).
fn active_tab_id() -> egui::Id {
    egui::Id::unique("ribbon_active_tab")
}

/// Where the collapsed dropdown that is open lives, keyed by the tab and the group it was opened
/// on (None = none open; only one at a time, as in Excel). The tab in the key keeps a tab switch
/// that no pointer click carried (control channel, a command, header/footer editing) from leaving
/// the dropdown open — or re-opening it — on the tab it was left behind. The dropdown is an
/// [`egui::Area`], not a `Popup`: egui keeps a single
/// popup slot per viewport, so a `Popup`-based group popup would be replaced — and then abandoned
/// — by the first control inside it that opens a popup of its own (a font combo, a split menu).
pub(crate) fn group_dropdown_open_id() -> egui::Id {
    egui::Id::unique("ribbon_group_dropdown_open")
}

/// Where each group's last expanded row-slot width lives (filled whenever a group renders
/// expanded, read while it renders collapsed — see [`GroupMeasure::width`]). Keyed by tab and
/// group name: the same name ("Properties") names different groups on Automate and Table
/// Design, and one overwriting the other would feed `plan_for` a wrong expanded width. Never
/// cleared: stale entries for groups a tab no longer shows are simply never read.
fn group_expanded_widths_id() -> egui::Id {
    egui::Id::unique("ribbon_group_expanded_widths")
}

type ExpandedWidths = HashMap<(String, String), f32>;

fn expanded_widths(ctx: &egui::Context) -> ExpandedWidths {
    ctx.data(|d| d.get_temp(group_expanded_widths_id())).unwrap_or_default()
}

/// Draws one ribbon group: a separator before it (none before the row's first group, as in Excel),
/// then either its body, or — when the tab's plan at [`group_plan_id`] names it — a launcher
/// button whose dropdown re-runs the body. Under an id salt derived from `name`, either way, so
/// the group's widget temp-state (combo drafts, popup memory) survives the move. Records the
/// width it took and the launcher size for this frame.
pub(crate) fn group(ui: &mut Ui, name: &str, hero: Icon, body: impl FnOnce(&mut Ui)) {
    let tab: String = ui.ctx().data(|d| d.get_temp(active_tab_id())).unwrap_or_default();
    let collapsed = ui.ctx().data(|d| d.get_temp::<Vec<String>>(group_plan_active_id())).is_some_and(|plan| plan.iter().any(|g| g == name));
    // The measure starts before the separator: a group's row slot is the lead (separator and its
    // spacing) plus the content plus the row's trailing spacing, so plans sum to the real row.
    let start = ui.cursor().min.x;
    let has_lead = ui.cursor().min.x > ui.max_rect().left() + 0.5;
    if has_lead {
        sep(ui);
    }
    let (launcher, width) = if collapsed {
        let r = launcher_button(ui, hero, name);
        let dropdown_id = egui::Id::unique(("ribbon_group_dropdown", name));
        let shared: Option<(String, String)> = ui.ctx().data(|d| d.get_temp(group_dropdown_open_id())).unwrap_or_default();
        // The tab in the key must be this one: a dropdown opened on another tab (and left there by
        // a switch that no click carried) is not open here.
        let was_open = matches!(&shared, Some((t, g)) if t.as_str() == tab && g == name);
        let mut open = was_open;
        if r.clicked() {
            open = !open;
        } else if was_open {
            // Menu-style dismissal (the Area cousin of the grid's `popup_dismissed`): a click on
            // the world behind us, Escape, a lost window, or the launcher scrolling out of the
            // card. A click on a floating layer must NOT dismiss — our own dropdown is Tooltip,
            // and nested control popups (combos, split menus) open on Foreground.
            let clicked_outside = ui.input(|i| i.pointer.any_click())
                && ui
                    .input(|i| i.pointer.interact_pos())
                    .is_none_or(|pos| ui.ctx().layer_id_at(pos).is_none_or(|l| matches!(l.order, Order::Background | Order::Middle)));
            let escape_pressed = ui.input(|i| i.key_pressed(Key::Escape));
            // Not inside `input`: it write-locks the context, and `is_any_open` read-locks it.
            let escape = ui.input(|i| !i.focused) || (escape_pressed && !egui::Popup::is_any_open(ui.ctx()));
            let scrolled_away = !ui.clip_rect().intersects(r.rect);
            if clicked_outside || escape || scrolled_away {
                open = false;
            }
        }
        ui.ctx().data_mut(|d| {
            let current: Option<(String, String)> = d.get_temp(group_dropdown_open_id()).unwrap_or_default();
            if open {
                d.insert_temp(group_dropdown_open_id(), Some((tab.clone(), name.to_owned())));
            } else if matches!(&current, Some((t, g)) if t.as_str() == tab && g == name) {
                d.insert_temp::<Option<(String, String)>>(group_dropdown_open_id(), None);
            }
        });
        if open {
            egui::Area::new(dropdown_id).order(Order::Tooltip).fixed_pos(r.rect.left_bottom() + vec2(0.0, 2.0)).constrain(true).show(
                ui.ctx(),
                |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| ui.push_id(("ribbon_group", name), |ui| body(ui)));
                    });
                },
            );
        }
        // The rendered launcher row, and the width the body would take back: the plan computes
        // with true sizes, so a collapsed group reports its cached expanded slot.
        let row = (ui.cursor().min.x - start).max(0.0);
        let expanded = expanded_widths(ui.ctx()).get(&(tab.clone(), name.to_owned())).copied();
        (row, expanded.unwrap_or(row))
    } else {
        // The group is expanded while its dropdown temp names it (on this tab): the plan changed
        // under the open dropdown, so close it — it must not re-open without a click on the
        // launcher.
        ui.ctx().data_mut(|d| {
            let open: Option<(String, String)> = d.get_temp(group_dropdown_open_id()).unwrap_or_default();
            if matches!(&open, Some((t, g)) if t.as_str() == tab && g == name) {
                d.insert_temp::<Option<(String, String)>>(group_dropdown_open_id(), None);
            }
        });
        ui.push_id(("ribbon_group", name), |ui| body(ui));
        let row = (ui.cursor().min.x - start).max(0.0);
        ui.ctx().data_mut(|d| {
            d.get_temp_mut_or::<ExpandedWidths>(group_expanded_widths_id(), HashMap::new()).insert((tab.clone(), name.to_owned()), row);
        });
        // What this group's collapsed row would take: the launcher button, the lead separator if
        // the row draws one for it (the first group has none), and the row's trailing spacing.
        let launcher_row = big_button_width(ui, name) + if has_lead { GROUP_LEAD } else { GROUP_ROW_SPACING };
        (launcher_row, row)
    };
    let measure = GroupMeasure { name: name.to_owned(), width, launcher };
    ui.ctx().data_mut(|d| {
        d.get_temp_mut_or::<Vec<GroupMeasure>>(group_measures_id(), Vec::new()).push(measure);
    });
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
