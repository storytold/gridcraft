//! Sheet tabs and the status bar.

use std::sync::Arc;

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{Sheet, Visibility};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// The rename field asks for focus only while it has not got it yet. Asking again on the frame it lost focus (Enter or a click elsewhere)
/// would keep the field focused after it is closed, so the keyboard stayed stuck in the sheet name.
fn rename_needs_focus(has_focus: bool, lost_focus: bool) -> bool {
    !has_focus && !lost_focus
}

/// Rows of the All Sheets navigator as `(workbook index, name)`. Hidden sheets stay out, like on the tab strip:
/// Unhide is the way to reach them.
fn navigator_items(sheets: &[Arc<Sheet>]) -> Vec<(String, String)> {
    sheets.iter().enumerate().filter(|(_, s)| s.visibility == Visibility::Visible).map(|(i, s)| (i.to_string(), s.name.clone())).collect()
}

/// Switch to sheet `i` from the tab strip or the navigator. In point mode the formula being typed keeps going on the
/// other sheet; otherwise the edit is committed first.
fn activate_sheet(app: &mut SheetApp, i: usize) {
    if app.editor.as_ref().is_some_and(|e| e.can_point()) {
        let _ = app.session.run("sheet.activate", json!({"sheet": i}));
    } else {
        if app.editor.is_some() && !app.commit_edit(0, 0, false, false) {
            return;
        }
        app.run_or_alert("sheet.activate", json!({"sheet": i}));
    }
}

/// A tab released with Ctrl held (Cmd on macOS, as for the fill handle) drops a copy of the sheet. Excel for Mac uses
/// Option for this, so Option copies there too.
fn drop_copies_sheet(m: egui::Modifiers, mac: bool) -> bool {
    m.command || (mac && m.alt)
}

/// `sheet.move` params for tab `i` dropped `steps` tabs away, or `None` when it lands where it started.
fn tab_drop(i: usize, steps: i64, copy: bool) -> Option<serde_json::Value> {
    if steps == 0 {
        return None;
    }
    let to = (i as i64 + steps).max(0) as usize;
    if copy {
        // A move indexes the list without the sheet, a copy the list with it: to the right, the copy goes one further.
        let to = if steps > 0 { to + 1 } else { to };
        return Some(json!({"sheet": i, "to": to, "copy": true}));
    }
    Some(json!({"sheet": i, "to": to}))
}

/// The New sheet (+) button, its chevron and the gap before them.
const NEW_SHEET_WIDTH: f32 = 46.0;
/// The handle between the tabs and the horizontal scrollbar.
const SPLIT_WIDTH: f32 = 8.0;
/// The narrowest the scrollbar track gets when the handle is dragged to the right.
const MIN_TRACK: f32 = 100.0;

/// The share of the tab bar (`bar`: its left edge and width) the tabs and New sheet take with
/// the handle dragged to `x`: New sheet stays shown, and the track ending at `right` keeps
/// [`MIN_TRACK`].
fn dragged_share(x: f32, bar: (f32, f32), right: f32) -> f32 {
    let (left, width) = bar;
    if !width.is_finite() || width <= 0.0 || !x.is_finite() {
        return 0.6;
    }
    let min = NEW_SHEET_WIDTH / width;
    let max = ((right - SPLIT_WIDTH - MIN_TRACK - left) / width).max(min);
    ((x - left) / width).clamp(min, max)
}

/// The handle at the left edge of the horizontal scrollbar: dragging it gives the sheet tabs more
/// or less of the bar, as in Excel, for the workbook `uid`.
fn tab_split_handle(app: &mut SheetApp, ui: &mut Ui, split: Rect, bar: (f32, f32), right: f32, uid: u64, t: &Tokens) {
    app.grid.tab_split = Some(split);
    let resp = ui.interact(split, ui.scope_id().with("tab_split"), Sense::drag());
    let name = app.ui.language.tr("Split tabs");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        app.grid.tab_shares.insert(uid, dragged_share(p.x - SPLIT_WIDTH / 2.0, bar, right));
        ui.ctx().request_repaint();
    }
    // The grip shows only while it is under the pointer: no extra marks on the strip otherwise.
    if resp.hovered() || resp.dragged() {
        for dy in [-4.0, 0.0, 4.0] {
            ui.painter().circle_filled(pos2(split.center().x, split.center().y + dy), 1.0, t.text_dim);
        }
    }
}

/// The pill-shaped rename field inside tab pill `r`.
fn rename_pill(r: Rect) -> Rect {
    r.shrink2(vec2(1.0, 1.0))
}

/// The theme accents a tab takes in one click from its context menu: theme colour slots 4 to 9,
/// as `sheet.tabColor` hex and as RGB.
fn accent_hexes(colors: &[u32; 12]) -> Vec<(String, [u8; 3])> {
    colors
        .iter()
        .skip(4)
        .take(6)
        .map(|&c| {
            let rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            (format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]), rgb)
        })
        .collect()
}

/// A row of colour dots for the tab context menu: No Color, then the six theme accents. The dot of
/// the tab's `current` colour wears a ring. Returns the picked `sheet.tabColor` colour.
fn tab_color_dots(ui: &mut Ui, colors: &[u32; 12], current: Option<[u8; 3]>, lang: crate::i18n::Language) -> Option<String> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(2.0);
        let none = (lang.tr("No Color").to_string(), None);
        let accents = accent_hexes(colors).into_iter().enumerate().map(|(n, swatch)| (format!("{} {}", lang.tr("Accent"), n + 1), Some(swatch)));
        for (name, swatch) in std::iter::once(none).chain(accents) {
            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            let c = r.center();
            if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
                ui.painter().circle_filled(c, 11.0, fill);
            }
            let p = ui.painter();
            let on = match &swatch {
                Some((_, rgb)) => current == Some(*rgb),
                None => current.is_none(),
            };
            match &swatch {
                Some((_, rgb)) => {
                    p.circle_filled(c, 7.0, theme::color32(*rgb));
                }
                None => {
                    let pen = Stroke::new(1.0, t.text_dim);
                    p.circle_stroke(c, 6.5, pen);
                    p.line_segment([c + vec2(-4.6, 4.6), c + vec2(4.6, -4.6)], pen);
                }
            }
            if on {
                p.circle_stroke(c, 9.5, Stroke::new(1.5, t.accent));
            }
            if resp.on_hover_text(&name).clicked() {
                picked = Some(swatch.map_or_else(|| "none".to_string(), |(hex, _)| hex));
            }
        }
    });
    ui.add_space(2.0);
    picked
}

/// The menu of the New sheet button's chevron (and its right-click): a blank sheet, or a copy of the active one.
fn new_sheet_menu(app: &mut SheetApp, ui: &mut Ui) {
    ui.set_min_width(180.0);
    if ui.button(app.ui.language.tr("Blank sheet")).clicked() {
        app.run_or_alert("home.insertSheet", json!({}));
    }
    if ui.button(app.ui.language.tr("Duplicate current sheet")).clicked()
        && let Some(i) = app.session.active().map(|d| d.wb.active_sheet())
    {
        app.run_or_alert("sheet.move", json!({"sheet": i, "to": i + 1, "copy": true}));
    }
}

/// Draws the worksheet tab strip: navigation, scroll/reveal, rename/drag/move with optional copy, context menu, new-sheet button, split handle and grid scrollbar.
pub fn sheet_tabs(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    // Too narrow for one row (the same test as below): the status items get a bar of their own,
    // under the tabs, so Edit/Point mode, circular references and the paste hint still show.
    if app.ui.status_bar && ui.available_width() - 2.0 * f32::from(crate::app::SHEET_GUTTER) < 760.0 {
        status_bar(app, ui);
    }
    egui::Panel::bottom("sheet_tabs")
        .show_separator_line(false)
        .exact_size(38.0)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin {
            left: crate::app::SHEET_GUTTER,
            right: crate::app::SHEET_GUTTER,
            top: 0,
            bottom: 0,
        }))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            // Every tab pill is centred on the strip, level with the arrows beside it.
            let strip_cy = rect.center().y;
            // The bottom of the sheet card: the strip continues the card's white, with rounded corners.
            ui.painter().rect_filled(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, t.sheet_bg(app.ui.dark_sheet));
            // One row: the sheet tabs (and the horizontal scrollbar) on the left, the status items on
            // the right.
            // A narrow window gives the whole row to the tabs and the scrollbar.
            let tabs_rect = if app.ui.status_bar && rect.width() >= 760.0 {
                let status_w = (rect.width() * 0.42).clamp(320.0, 620.0).min(rect.width() * 0.6);
                let status = Rect::from_min_max(pos2(rect.right() - status_w, rect.top()), rect.max).shrink2(vec2(8.0, 0.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(status).layout(egui::Layout::left_to_right(egui::Align::Center)));
                status_items(app, &mut child);
                Rect::from_min_max(rect.min, pos2(status.left() - 8.0, rect.bottom()))
            } else {
                rect
            };
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let ui = &mut ui;
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                if crate::widgets::icon_button(ui, Icon::Left, t.text_dim, app.ui.language.tr("Previous sheet"), vec2(24.0, 24.0)).clicked() {
                    app.run_or_alert("sheet.previous", json!({}));
                }
                if crate::widgets::icon_button(ui, Icon::Right, t.text_dim, app.ui.language.tr("Next sheet"), vec2(24.0, 24.0)).clicked() {
                    app.run_or_alert("sheet.next", json!({}));
                }
                let all = crate::widgets::icon_button(ui, Icon::SheetList, t.text_dim, app.ui.language.tr("All sheets"), vec2(24.0, 24.0));
                all.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, app.ui.language.tr("All sheets")));
                let items = app.session.active().map(|d| navigator_items(&d.wb.sheets)).unwrap_or_default();
                let active = app.session.active().map(|d| d.wb.active_sheet().to_string()).unwrap_or_default();
                let rows: Vec<(&str, String)> = items.iter().map(|(i, name)| (i.as_str(), name.clone())).collect();
                if let crate::ribbon::ComboResult::Picked(i) = crate::ribbon::editable_combo(ui, &all, "sheet_navigator", &active, &rows, false)
                    && let Ok(i) = i.parse::<usize>()
                {
                    activate_sheet(app, i);
                }
                ui.add_space(8.0);
                let Some(d) = app.session.active() else { return };
                let (bar_left, bar_width) = (ui.cursor().left(), ui.available_width());
                let uid = d.uid;
                // The track ends where the grid does, so it stops under the vertical scrollbar. A share
                // dragged in a wider window is held to the same limits as a drag at this width.
                let bar_right = app.grid.rect.map_or(bar_left + bar_width, |r| r.right()).min(bar_left + bar_width);
                let share = app.grid.tab_shares.get(&uid).map(|&share| dragged_share(bar_left + bar_width * share, (bar_left, bar_width), bar_right));
                let sheets: Vec<(usize, String, Option<[u8; 3]>, bool)> =
                    d.wb.sheets
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.visibility == Visibility::Visible)
                        .map(|(i, s)| (i, s.name.clone(), s.tab_color.and_then(|c| c.resolve(&d.wb.theme)), i == d.wb.active_sheet()))
                        .collect();
                // Keep worksheet tabs in their own horizontally scrollable viewport. As in Excel, the
                // tabs get up to 60% of the bar and the grid's horizontal scrollbar the rest; a strip
                // that needs less shrinks to its tabs, so New sheet (+) sits right after the last one.
                // After the handle at the scrollbar's left edge is dragged, the tabs and New sheet
                // take the share it was dragged to.
                let tabs_width = match share {
                    Some(share) => (bar_width * share - NEW_SHEET_WIDTH).max(0.0),
                    // In a narrow window the tabs give way first, so the scrollbar keeps [`MIN_TRACK`].
                    None => (bar_width * 0.6).max(100.0).min(bar_right - bar_left - NEW_SHEET_WIDTH - SPLIT_WIDTH - MIN_TRACK).max(0.0),
                };
                // Reveal the selected sheet after navigation or layout changes, but
                // allow browsing other tabs without snapping back on every frame.
                let reveal_key = egui::Id::unique((d.uid, d.wb.active_sheet(), &sheets, tabs_width.to_bits()));
                let reveal_id = ui.scope_id().with("worksheet_tabs_reveal");
                let reveal_active = ui.ctx().data_mut(|data| {
                    let changed = data.get_temp::<egui::Id>(reveal_id) != Some(reveal_key);
                    data.insert_temp(reveal_id, reveal_key);
                    changed
                });
                ui.allocate_ui_with_layout(vec2(tabs_width, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Translate vertical wheel movement into horizontal
                    // worksheet-tab scrolling.
                    let wheel_scroll = if ui.rect_contains_pointer(ui.max_rect()) {
                        ui.input(|i| {
                            let d = i.smooth_scroll_delta;
                            if d.x.abs() > d.y.abs() { d.x } else { d.y }
                        })
                    } else {
                        0.0
                    };

                    egui::ScrollArea::horizontal()
                        .id_salt("worksheet_tabs_horizontal")
                        .auto_shrink([true, false])
                        .min_scrolled_width(0.0)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            if wheel_scroll.abs() > 0.0 {
                                ui.scroll_with_delta(vec2(wheel_scroll, 0.0));
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for (i, name, color, active) in sheets {
                                    let renaming = app.grid.renaming_tab == Some(i);
                                    let font = if active { theme::ui_bold(12.0) } else { theme::ui_font(12.0) };
                                    let tw = ui.painter().layout_no_wrap(name.clone(), font.clone(), t.text).size().x;
                                    // The rename pill grows with the name being typed, so the caret never runs out of room.
                                    let w = if renaming {
                                        let typed = ui.painter().layout_no_wrap(app.grid.rename_text.clone(), theme::ui_font(12.0), t.text).size().x;
                                        (tw.max(typed) + 40.0).max(104.0)
                                    } else {
                                        (tw + 36.0).max(64.0)
                                    };
                                    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click_and_drag());
                                    if active && reveal_active {
                                        resp.scroll_to_me(None);
                                    }
                                    // Pill tabs: the active one in the accent tint, a tab colour as a dot before the name.
                                    let r = r.shrink2(vec2(1.5, 2.0));
                                    let r = Rect::from_center_size(pos2(r.center().x, strip_cy), vec2(r.width(), 26.0));
                                    if active {
                                        // The active pill glides to a newly chosen sheet.
                                        let pill = if crate::widgets::animations_on(ui.ctx()) {
                                            let ctx = ui.ctx();
                                            let left = ctx.animate_value_with_time(egui::Id::unique("sheet_tabs.pill.left"), r.left(), 0.14);
                                            let right = ctx.animate_value_with_time(egui::Id::unique("sheet_tabs.pill.right"), r.right(), 0.14);
                                            Rect::from_min_max(pos2(left, r.top()), pos2(right, r.bottom()))
                                        } else {
                                            r
                                        };
                                        ui.painter().rect_filled(pill, 7.0, t.pressed);
                                    } else if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
                                        ui.painter().rect_filled(r, 7.0, fill);
                                    }
                                    // A sheet with its own tab colour shows it as a small dot before its name.
                                    let dot = color.map(theme::color32);
                                    let text_center = if dot.is_some() { pos2(r.center().x + 5.0, r.center().y) } else { r.center() };
                                    if let Some(c) = dot
                                        && !renaming
                                    {
                                        ui.painter().circle_filled(pos2(text_center.x - tw / 2.0 - 8.0, r.center().y), 2.5, c);
                                    }
                                    if renaming {
                                        let mut text = app.grid.rename_text.clone();
                                        let pill = rename_pill(r);
                                        ui.painter().rect_filled(pill, pill.height() / 2.0, t.input_bg);
                                        ui.painter().rect_stroke(pill, pill.height() / 2.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                                        // One text row, centred in the pill.
                                        let field = Rect::from_center_size(pill.center(), vec2((pill.width() - 20.0).max(0.0), 16.0));
                                        let mut child = ui.new_child(
                                            egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)),
                                        );
                                        let ed = child
                                            .add(
                                                egui::TextEdit::singleline(&mut text)
                                                    .font(theme::ui_font(12.0))
                                                    .frame(egui::Frame::NONE)
                                                    .margin(egui::Margin::ZERO)
                                                    .desired_width(field.width()),
                                            )
                                            .accessible_name(app.ui.language.tr("Sheet Name"));
                                        app.grid.rename_text = text.clone();
                                        if rename_needs_focus(ed.has_focus(), ed.lost_focus()) {
                                            ed.request_focus();
                                        }
                                        if ed.lost_focus() {
                                            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && text != name {
                                                app.run_or_alert("sheet.rename", json!({"sheet": i, "name": text}));
                                            }
                                            app.grid.renaming_tab = None;
                                        }
                                    } else {
                                        ui.painter().text(text_center, Align2::CENTER_CENTER, &name, font, t.text);
                                    }
                                    // A right-click activates the tab too, as in Excel, so the menu's sheet commands (Protect
                                    // Sheet, Select All Sheets) act on the sheet that was clicked rather than the one that was active.
                                    if resp.clicked() || (resp.secondary_clicked() && !active) {
                                        activate_sheet(app, i);
                                    }
                                    if resp.double_clicked() {
                                        app.grid.renaming_tab = Some(i);
                                        app.grid.rename_text = name.clone();
                                    }
                                    // Drag to reorder; with the copy modifier held, drop a copy instead.
                                    let copy = drop_copies_sheet(ui.input(|inp| inp.modifiers), ui.ctx().os() == egui::os::OperatingSystem::Mac);
                                    if resp.dragged() && copy {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
                                    }
                                    if resp.drag_stopped()
                                        && let Some(p) = ui.input(|inp| inp.pointer.interact_pos())
                                    {
                                        let dx = p.x - r.center().x;
                                        let steps = (dx / w).round() as i64;
                                        if let Some(params) = tab_drop(i, steps, copy) {
                                            app.run_or_alert("sheet.move", params);
                                        }
                                    }
                                    resp.context_menu(|ui| {
                                        ui.set_min_width(180.0);
                                        if ui.button(app.ui.language.tr("Insert Sheet")).clicked() {
                                            app.run_or_alert("home.insertSheet", json!({"before": i}));
                                        }
                                        if ui.button(app.ui.language.tr("Delete")).clicked() {
                                            app.run_or_alert("home.deleteSheet", json!({"sheet": i}));
                                        }
                                        if ui.button(app.ui.language.tr("Rename")).clicked() {
                                            app.grid.renaming_tab = Some(i);
                                            app.grid.rename_text = name.clone();
                                        }
                                        if ui.button(app.ui.language.tr("Move or Copy…")).clicked() {
                                            app.open_dialog("moveSheet", json!({"sheet": i}));
                                        }
                                        if ui.button(app.ui.language.tr("Duplicate")).clicked() {
                                            app.run_or_alert("sheet.move", json!({"sheet": i, "to": i + 1, "copy": true}));
                                        }
                                        ui.separator();
                                        let theme_colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_default();
                                        if let Some(c) = tab_color_dots(ui, &theme_colors, color, app.ui.language) {
                                            app.run_or_alert("sheet.tabColor", json!({"sheet": i, "color": c}));
                                            ui.close();
                                        }
                                        ui.menu_button(app.ui.language.tr("Tab Color"), |ui| {
                                            let colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_default();
                                            if let Some(c) = crate::widgets::color_palette_fixed(ui, &colors, app.ui.language.tr("No Color")) {
                                                app.run_or_alert("sheet.tabColor", json!({"sheet": i, "color": c}));
                                                ui.close();
                                            }
                                        });
                                        ui.separator();
                                        if ui.button(app.ui.language.tr("Hide")).clicked() {
                                            app.run_or_alert("sheet.hide", json!({"sheet": i}));
                                        }
                                        if ui.button(app.ui.language.tr("Unhide…")).clicked() {
                                            app.run_or_alert("sheet.unhide", json!({}));
                                        }
                                        ui.separator();
                                        let protected = app.active_sheet_protected();
                                        if ui
                                            .button(if protected {
                                                app.ui.language.tr("Unprotect Sheet…")
                                            } else {
                                                app.ui.language.tr("Protect Sheet…")
                                            })
                                            .clicked()
                                        {
                                            app.open_dialog("protectSheet", json!({}));
                                        }
                                        if ui.button(app.ui.language.tr("Select All Sheets")).clicked() {
                                            ui.close();
                                        }
                                    });
                                }
                            });
                        });
                });
                ui.add_space(6.0);
                let plus = crate::widgets::icon_button(ui, Icon::Plus, t.text_dim, app.ui.language.tr("New sheet (⇧F11)"), vec2(26.0, 26.0));
                if plus.clicked() {
                    app.run_or_alert("home.insertSheet", json!({}));
                }
                let more_label = app.ui.language.tr("More sheet options");
                let more = crate::widgets::icon_button(ui, Icon::Chevron, t.text_dim, more_label, vec2(14.0, 26.0));
                more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, more_label));
                egui::Popup::menu(&more).show(|ui| new_sheet_menu(app, ui));
                plus.context_menu(|ui| new_sheet_menu(app, ui));
                // The track ends where the grid does, so it stops under the vertical scrollbar.
                let avail = ui.available_rect_before_wrap();
                let right = app.grid.rect.map_or(avail.right(), |r| r.right()).min(avail.right());
                // The handle sits at the track's left edge. With a dragged share, the room the tabs
                // don't fill stays empty before it.
                let split_left =
                    share.map_or(avail.left(), |share| (bar_left + bar_width * share).min(right - SPLIT_WIDTH - MIN_TRACK).max(avail.left()));
                let split = Rect::from_min_max(pos2(split_left, avail.top()), pos2(split_left + SPLIT_WIDTH, avail.bottom()));
                tab_split_handle(app, ui, split, (bar_left, bar_width), right, uid, &t);
                let track = Rect::from_min_max(pos2(split.right(), avail.center().y - 7.0), pos2(right, avail.center().y + 7.0));
                crate::grid::horizontal_scrollbar(app, ui, track, &t);
            });
        });
}

/// Draws status mode/messages, zoom controls/view-mode buttons and selection statistics.
/// The status items as a bar of their own, for a host that draws no sheet tabs (the app puts them
/// in the sheet-tab row).
pub fn status_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("status_bar")
        .show_separator_line(false)
        .exact_size(28.0)
        .frame(egui::Frame::NONE.fill(t.status_bar).inner_margin(egui::Margin::symmetric(16, 0)))
        .show(ui, |ui| status_items(app, ui));
}

/// The status items, right-aligned in the sheet-tab row: messages (Edit or Point mode, macros,
/// manual calculation, circular references, the paste hint), the statistic chips, the view
/// button and zoom. Ready, the usual state, shows nothing.
fn status_items(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    // A right-click anywhere on the status bar opens the statistics menu, the text included, so its labels are not
    // selectable. The zoom and view controls sense clicks and handle their own.
    let background = ui.interact(ui.max_rect(), egui::Id::unique("gridcraft.status_bar"), Sense::click());
    background.context_menu(|ui| status_bar_menu(app, ui));
    let text = |ui: &mut Ui, rich: egui::RichText| {
        ui.add(egui::Label::new(rich).selectable(false));
    };
    ui.horizontal_centered(|ui| {
        let mode = match app.session.mode {
            gridcraft_engine::Mode::Ready => "Ready",
            gridcraft_engine::Mode::Enter => "Enter",
            gridcraft_engine::Mode::Edit => "Edit",
            gridcraft_engine::Mode::Point => "Point",
        };
        // Ready is the usual state and shows nothing; Enter, Edit and Point are named.
        if mode != "Ready" {
            text(ui, egui::RichText::new(app.ui.language.tr(mode)).font(theme::ui_font(12.0)).color(t.text_dim));
        }
        crate::macro_ui::status(app, ui);
        if let Some(d) = app.session.active() {
            if d.wb.vba_project.is_some() {
                ui.add_space(12.0);
                text(ui, egui::RichText::new("Macros preserved (not executed)").font(theme::ui_font(12.5)).color(t.danger));
            }
            if d.wb.calc.mode == gridcraft_engine::model::CalcMode::Manual {
                ui.add_space(12.0);
                text(ui, egui::RichText::new(app.ui.language.tr("Calculate")).font(theme::ui_font(12.5)).color(t.text_dim));
            }
            if !d.calc.circular.is_empty() {
                ui.add_space(12.0);
                // Only a loop on the active sheet is named, so the address needs no sheet.
                let label = match d.calc.circular.iter().find(|(si, _)| *si == d.wb.active_sheet()) {
                    Some((_, c)) => format!("Circular References: {}", c.a1()),
                    None => "Circular References".to_owned(),
                };
                text(ui, egui::RichText::new(label).font(theme::ui_font(12.5)).color(t.danger));
            }
            if app.session.clipboard.is_some() && app.editor.is_none() {
                ui.add_space(12.0);
                text(
                    ui,
                    egui::RichText::new(app.ui.language.tr("Select destination and press Enter or choose Paste"))
                        .font(theme::ui_font(12.5))
                        .color(t.text_dim),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Zoom percentage and slider; Ctrl+wheel and the View tab step the zoom.
            let zoom = app.session.active().and_then(|d| d.wb.active().map(|s| s.zoom)).unwrap_or(100);
            let zl =
                ui.add(egui::Label::new(egui::RichText::new(format!("{zoom}%")).font(theme::ui_font(12.5)).color(t.text_dim)).sense(Sense::click()));
            if zl.clicked() {
                app.open_dialog("zoom", json!({}));
            }
            ui.add_space(6.0);
            let mut z = zoom as f32;
            // An accent-filled track and a round handle.
            let slider = ui
                .scope(|ui| {
                    let v = &mut ui.style_mut().visuals;
                    v.slider_trailing_fill = true;
                    v.selection.bg_fill = t.accent;
                    v.handle_shape = egui::style::HandleShape::Circle;
                    // The rail (and the resting handle, which egui fills alike) is a shade of
                    // the strip it sits on, and the handle's outline sets it apart.
                    let rail = t.ribbon.lerp_to_gamma(t.text, 0.2);
                    v.widgets.inactive.bg_fill = rail;
                    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.5, t.text_dim);
                    v.widgets.hovered.bg_fill = t.ribbon;
                    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, t.accent);
                    v.widgets.active.bg_fill = t.ribbon;
                    v.widgets.active.fg_stroke = egui::Stroke::new(1.5, t.accent);
                    ui.spacing_mut().slider_width = 96.0;
                    ui.spacing_mut().interact_size.y = 13.0;
                    ui.spacing_mut().slider_rail_height = 4.0;
                    ui.add(egui::Slider::new(&mut z, 10.0..=400.0).show_value(false).logarithmic(true))
                })
                .inner;
            ui.ctx().accesskit_node_builder(slider.id, |node| node.set_label(app.ui.language.tr("Zoom")));
            if slider.changed() {
                let _ = app.session.run("view.zoom", json!({"percent": z.round()}));
            }
            ui.add_space(10.0);
            // The three sheet views as one button: it shows the current view and steps to
            // the next on a click; right-click lists all three.
            const VIEWS: [(Icon, &str, &str, &str); 3] = [
                (Icon::Normal, "Normal", "normal", "view.normal"),
                (Icon::PageLayout, "Page Layout", "pageLayout", "view.pageLayout"),
                (Icon::PageBreak, "Page Break Preview", "pageBreakPreview", "view.pageBreakPreview"),
            ];
            let cur = app.session.active().and_then(|d| d.wb.active().map(|s| s.view.as_str())).unwrap_or("normal");
            let at = VIEWS.iter().position(|v| v.2 == cur).unwrap_or(0);
            let (icon, name, _, _) = VIEWS[at];
            let (_, next_name, _, next_cmd) = VIEWS[(at + 1) % VIEWS.len()];
            let lang = app.ui.language;
            let tip = format!("{}: {} ({} {})", lang.tr("View"), lang.tr(name), lang.tr("click for"), lang.tr(next_name));
            let (r, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Sheet view"));
            if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
                ui.painter().rect_filled(r, 5.0, fill);
            }
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, t.text);
            let resp = resp.on_hover_text(tip);
            if resp.clicked() {
                app.run_or_alert(next_cmd, json!({}));
            }
            resp.context_menu(|ui| {
                for (_, label, mode, cmd) in VIEWS {
                    if ui.selectable_label(cur == mode, lang.tr(label)).clicked() {
                        app.run_or_alert(cmd, json!({}));
                        ui.close();
                    }
                }
            });
            ui.add_space(16.0);
            // The layout runs right to left, so the last statistic is drawn first.
            for stat in selection_statistics(app).into_iter().rev() {
                let label = app.ui.language.tr(stat.label);
                let chip = stat_chip(ui, &t, label, &stat.value).on_hover_text(app.ui.language.tr("Click to copy"));
                if chip.clicked() {
                    ui.ctx().copy_text(stat.value.clone());
                    app.toast = Some((format!("{} {label}", app.ui.language.tr("Copied")), crate::now_ms()));
                }
                chip.context_menu(|ui| status_bar_menu(app, ui));
                ui.add_space(2.0);
            }
        });
    });
}

/// A quiet status bar chip: a dim `label` and its `value`, with a soft rounded fill on hover.
fn stat_chip(ui: &mut Ui, t: &Tokens, label: &str, value: &str) -> egui::Response {
    let label_galley = ui.painter().layout_no_wrap(label.to_string(), theme::ui_font(11.5), t.text_dim);
    let value_galley = ui.painter().layout_no_wrap(value.to_string(), theme::ui_font(12.0), t.text);
    let (gap, pad) = (5.0, 8.0);
    let w = label_galley.size().x + gap + value_galley.size().x + 2.0 * pad;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{label}: {value}")));
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, r.height() / 2.0, t.pressed);
    } else if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(r, r.height() / 2.0, fill);
    }
    let p = ui.painter();
    let lx = r.left() + pad;
    p.galley(pos2(lx, r.center().y - label_galley.size().y / 2.0), label_galley.clone(), t.text_dim);
    let vx = lx + label_galley.size().x + gap;
    p.galley(pos2(vx, r.center().y - value_galley.size().y / 2.0), value_galley, t.text);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// One status bar statistic: its English label and its value as the selection's number format shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Statistic {
    pub label: &'static str,
    pub value: String,
}

/// The statistics the status bar shows for a selection of more than one cell, left to right: Average, Count,
/// Numerical Count, Minimum, Maximum, Sum. Each one shows when it is turned on (`view.statusBar`)
/// and the selection has a value for it.
pub(crate) fn selection_statistics(app: &SheetApp) -> Vec<Statistic> {
    let Some(d) = app.session.active() else { return vec![] };
    let Some(sh) = d.wb.active() else { return vec![] };
    if d.selection.is_single_cell() {
        return vec![];
    }
    let on = app.session.prefs.status_bar;
    let st = gridcraft_engine::display::stats(sh, &d.selection.ranges);
    let fmt = |v: f64| {
        let code = d.wb.styles.get(sh.style_id(d.selection.active)).num_fmt.as_str().to_string();
        let code = if code == "General" { "#,##0.##########".to_string() } else { code };
        gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(v), &code, &d.wb).text.trim_end_matches('.').to_string()
    };
    let mut parts = Vec::new();
    let mut push = |label: &'static str, value: String| parts.push(Statistic { label, value });
    if on.average
        && let Some(a) = st.average
    {
        push("Average", fmt(a));
    }
    if on.count && st.count > 0 {
        push("Count", st.count.to_string());
    }
    if on.numerical_count && st.count > 0 {
        push("Numerical Count", st.numeric.to_string());
    }
    if on.minimum && st.numeric > 0 {
        push("Minimum", fmt(st.min));
    }
    if on.maximum && st.numeric > 0 {
        push("Maximum", fmt(st.max));
    }
    if on.sum && st.numeric > 0 {
        push("Sum", fmt(st.sum));
    }
    parts
}

/// The status bar's context menu: a check item for each statistic, each one running `view.statusBar`.
fn status_bar_menu(app: &mut SheetApp, ui: &mut Ui) {
    ui.set_min_width(180.0);
    let st = app.session.prefs.status_bar;
    for (key, label, mut on) in [
        ("average", "Average", st.average),
        ("count", "Count", st.count),
        ("numericalCount", "Numerical Count", st.numerical_count),
        ("minimum", "Minimum", st.minimum),
        ("maximum", "Maximum", st.maximum),
        ("sum", "Sum", st.sum),
    ] {
        if ui.checkbox(&mut on, app.ui.language.tr(label)).changed() {
            app.run_or_alert("view.statusBar", json!({ key: on }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MIN_TRACK, NEW_SHEET_WIDTH, SPLIT_WIDTH, activate_sheet, dragged_share, drop_copies_sheet, navigator_items, rename_needs_focus, tab_drop,
    };
    use crate::SheetApp;
    use egui::Modifiers;
    use gridcraft_engine::model::{Sheet, Visibility};
    use serde_json::json;
    use std::sync::Arc;

    fn workbook_abc() -> SheetApp {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.run("home.insertSheet", json!({})).unwrap();
        app.run("home.insertSheet", json!({})).unwrap();
        for (i, name) in ["A", "B", "C"].into_iter().enumerate() {
            app.run("sheet.rename", json!({"sheet": i, "name": name})).unwrap();
        }
        app
    }

    fn names(app: &SheetApp) -> Vec<String> {
        app.session.active().unwrap().wb.sheets.iter().map(|s| s.name.clone()).collect()
    }

    fn stats(app: &SheetApp) -> Vec<String> {
        super::selection_statistics(app).into_iter().map(|s| format!("{}: {}", s.label, s.value)).collect()
    }

    #[test]
    fn tab_color_dots_offer_the_theme_accents_as_hex() {
        let mut colors = [0u32; 12];
        for (n, c) in colors.iter_mut().enumerate() {
            *c = 0x10_20_30 * n as u32;
        }
        let accents = super::accent_hexes(&colors);
        assert_eq!(accents.len(), 6);
        assert_eq!(accents[0], ("#4080C0".to_string(), [0x40, 0x80, 0xC0]), "the first dot is theme slot 4 (Accent 1)");
        let mut app = workbook_abc();
        app.run("sheet.tabColor", json!({"sheet": 1, "color": accents[2].0})).unwrap();
        let wb = &app.session.active().unwrap().wb;
        assert_eq!(wb.sheets[1].tab_color.and_then(|c| c.resolve(&wb.theme)), Some(accents[2].1), "a dot sets the colour it shows");
    }

    #[test]
    fn rename_pill_sits_inside_its_tab() {
        let tab = egui::Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(120.0, 26.0));
        let pill = super::rename_pill(tab);
        assert!(tab.contains_rect(pill) && pill.height() > 16.0);
    }

    #[test]
    fn status_bar_shows_the_statistics_that_are_turned_on_in_order() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        for (cell, input) in [("A1", "4"), ("A2", "-1"), ("A3", "x"), ("A4", "9")] {
            app.run("cell.set", json!({"cell": cell, "input": input})).unwrap();
        }
        app.run("selection.set", json!({"range": "A1:A5"})).unwrap();
        assert_eq!(stats(&app), ["Average: 4", "Count: 4", "Sum: 12"]);
        app.run("view.statusBar", json!({"numericalCount": true, "minimum": true, "maximum": true})).unwrap();
        assert_eq!(stats(&app), ["Average: 4", "Count: 4", "Numerical Count: 3", "Minimum: -1", "Maximum: 9", "Sum: 12"]);
        app.run("view.statusBar", json!({"average": false, "count": false, "sum": false})).unwrap();
        assert_eq!(stats(&app), ["Numerical Count: 3", "Minimum: -1", "Maximum: 9"]);
        // Text only: Count and Numerical Count, nothing that needs a number.
        app.run("view.statusBar", json!({"average": true, "count": true, "sum": true})).unwrap();
        app.run("cell.set", json!({"cell": "B1", "input": "a"})).unwrap();
        app.run("cell.set", json!({"cell": "B2", "input": "b"})).unwrap();
        app.run("selection.set", json!({"range": "B1:B2"})).unwrap();
        assert_eq!(stats(&app), ["Count: 2", "Numerical Count: 0"]);
        app.run("selection.set", json!({"range": "A1"})).unwrap();
        assert!(stats(&app).is_empty(), "one cell shows no statistics");
    }

    #[test]
    fn copy_drop_modifier_is_ctrl_or_cmd_or_option_on_mac() {
        // egui reports Ctrl as `command` too off macOS, and Cmd as `mac_cmd` and `command` on macOS.
        assert!(drop_copies_sheet(Modifiers::CTRL | Modifiers::COMMAND, false));
        assert!(!drop_copies_sheet(Modifiers::ALT, false));
        assert!(drop_copies_sheet(Modifiers::MAC_CMD | Modifiers::COMMAND, true));
        assert!(drop_copies_sheet(Modifiers::ALT, true));
        assert!(!drop_copies_sheet(Modifiers::CTRL, true), "Ctrl+click is the context menu on macOS");
        assert!(!drop_copies_sheet(Modifiers::NONE, false));
    }

    #[test]
    fn tab_dropped_with_the_copy_modifier_leaves_a_copy_where_a_move_would_land() {
        let mut app = workbook_abc();
        app.run("sheet.move", tab_drop(0, 1, true).unwrap()).unwrap();
        assert_eq!(names(&app), ["A", "B", "A (2)", "C"]);

        let mut app = workbook_abc();
        app.run("sheet.move", tab_drop(2, -1, true).unwrap()).unwrap();
        assert_eq!(names(&app), ["A", "C (2)", "B", "C"]);

        let mut app = workbook_abc();
        app.run("sheet.move", tab_drop(0, 1, false).unwrap()).unwrap();
        assert_eq!(names(&app), ["B", "A", "C"]);
        assert!(tab_drop(1, 0, true).is_none());
    }

    #[test]
    fn rename_field_takes_focus_once_and_lets_go_on_commit() {
        assert!(rename_needs_focus(false, false), "first frame: grab focus");
        assert!(!rename_needs_focus(true, false), "already focused: do not ask again");
        assert!(!rename_needs_focus(false, true), "Enter or click-away: must not take focus back");
    }

    #[test]
    fn navigator_lists_visible_sheets_by_workbook_index() {
        let mut hidden = Sheet::new("Hidden");
        hidden.visibility = Visibility::Hidden;
        let mut very_hidden = Sheet::new("VeryHidden");
        very_hidden.visibility = Visibility::VeryHidden;
        let sheets = [Sheet::new("Data"), hidden, very_hidden, Sheet::new("Summary")].map(Arc::new);
        assert_eq!(navigator_items(&sheets), vec![("0".to_string(), "Data".to_string()), ("3".to_string(), "Summary".to_string())]);
    }

    #[test]
    fn a_rejected_commit_keeps_the_sheet_and_the_edit() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.execute("home.insertSheet", json!({})).unwrap();
        session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        let rule = json!({"range": "A1", "type": "whole", "operator": "between", "formula1": "1", "formula2": "10"});
        session.execute("data.validation", rule).unwrap();
        let mut app = crate::SheetApp::new(session, Default::default());
        app.begin_edit(Some("99".into()), false);
        activate_sheet(&mut app, 1);
        assert_eq!(app.session.active().unwrap().wb.active_sheet(), 0, "a value the validation stops must not leave its sheet");
        assert!(app.editor.is_some(), "the edit stays open to be corrected");
        app.editor.as_mut().unwrap().text = "7".into();
        activate_sheet(&mut app, 1);
        assert_eq!(app.session.active().unwrap().wb.active_sheet(), 1, "the corrected value commits and the sheet changes");
        let a1 = gridcraft_engine::core::CellRef::parse("A1").unwrap();
        assert!(app.session.active().unwrap().wb.sheet(0).unwrap().cell(a1).is_some(), "the value lands on the first sheet");
    }

    #[test]
    fn a_dragged_share_keeps_new_sheet_and_the_scrollbar_shown() {
        // A 1,000 px bar from x = 100 whose track ends at x = 1,100.
        let bar = (100.0, 1000.0);
        assert_eq!(dragged_share(600.0, bar, 1100.0), 0.5);
        assert_eq!(dragged_share(0.0, bar, 1100.0), NEW_SHEET_WIDTH / 1000.0, "New sheet stays shown");
        assert_eq!(dragged_share(1100.0, bar, 1100.0), (1000.0 - SPLIT_WIDTH - MIN_TRACK) / 1000.0, "the track keeps its minimum");
        assert_eq!(dragged_share(f32::NAN, bar, 1100.0), 0.6);
        assert_eq!(dragged_share(600.0, (100.0, 0.0), 1100.0), 0.6, "an empty bar has the default share");
    }
}
