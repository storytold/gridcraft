//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use std::borrow::Cow;

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::l10n::{Arg, Localizer, Tr, msg, tip};
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, color_palette, icon_button, small_button, split_button, toggle_button};

pub const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];

/// Leave room for the macOS traffic lights when the content extends into the title bar.
pub const TITLE_LEFT_PAD: f32 = if cfg!(target_os = "macos") { 76.0 } else { 8.0 };

pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    egui::Panel::top("title_bar").exact_size(38.0).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
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
            ui.label(egui::RichText::new(l.tr("AutoSave")).font(theme::ui_font(12.5)).color(t.text_dim));
            let on = app.grid.autosave();
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
            ui.painter().rect_filled(r, 8.0, if on { t.accent } else { Color32::TRANSPARENT });
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, if on { t.accent } else { t.text_dim }), StrokeKind::Inside);
            let knob = if on { pos2(r.right() - 8.0, r.center().y) } else { pos2(r.left() + 8.0, r.center().y) };
            ui.painter().circle_filled(knob, 5.0, if on { Color32::WHITE } else { t.text_dim });
            if resp.on_hover_text(l.tr("AutoSave: save after every change (workbooks saved to a file)")).clicked() {
                app.grid.toggle_autosave();
            }
            ui.add_space(6.0);
            if icon_button(ui, Icon::Home, t.text_dim, &l.tr("Home"), vec2(26.0, 26.0)).clicked() {
                app.open_dialog("start", json!({}));
            }
            if icon_button(ui, Icon::Save, t.text_dim, &tip(&l, app.keymap.platform(), "Save", "file.save"), vec2(26.0, 26.0)).clicked() {
                app.run_or_alert("file.save", json!({}));
            }
            let can_undo = app.session.active().is_some_and(|d| !d.undo.is_empty());
            let can_redo = app.session.active().is_some_and(|d| !d.redo.is_empty());
            if icon_button(
                ui,
                Icon::Undo,
                if can_undo { t.text_dim } else { t.text_disabled },
                &tip(&l, app.keymap.platform(), "Undo", "edit.undo"),
                vec2(26.0, 26.0),
            )
            .clicked()
                && can_undo
            {
                app.run_or_alert("edit.undo", json!({}));
            }
            let undo_list = icon_button(ui, Icon::Chevron, t.text_dim, &l.tr("Undo list"), vec2(14.0, 26.0));
            egui::Popup::menu(&undo_list).show(|ui| {
                let labels: Vec<String> = app
                    .session
                    .active()
                    .map(|d| d.undo.iter().rev().take(20).map(|e| crate::l10n::history_label(&l, &e.label)).collect())
                    .unwrap_or_default();
                if labels.is_empty() {
                    ui.label(l.tr("Can't Undo"));
                }
                for (i, entry) in labels.iter().enumerate() {
                    if ui.button(l.text("ui-ribbon-undo-entry", &[("label", Arg::from(entry))])).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
            if icon_button(
                ui,
                Icon::Redo,
                if can_redo { t.text_dim } else { t.text_disabled },
                &tip(&l, app.keymap.platform(), "Redo", "edit.redo"),
                vec2(26.0, 26.0),
            )
            .clicked()
                && can_redo
            {
                app.run_or_alert("edit.redo", json!({}));
            }
            let more = icon_button(ui, Icon::More, t.text_dim, &l.tr("More commands"), vec2(26.0, 26.0));
            egui::Popup::menu(&more).show(|ui| {
                for (label, id) in [
                    (msg!("New Workbook"), "file.new"),
                    (msg!("Open…"), "file.open"),
                    (msg!("Save As…"), "file.saveAs"),
                    (msg!("Print…"), "file.print"),
                    (msg!("Sort A to Z"), "data.sortAscending"),
                    (msg!("Calculate Now"), "formulas.calculateNow"),
                    (msg!("Options"), "file.options"),
                ] {
                    let text = l.command_label(id).unwrap_or_else(|| l.tr(label));
                    if ui.button(text).clicked() {
                        app.run_or_alert(id, json!({}));
                    }
                }
            });
            // Centered title.
            let title = app.session.active().map(|d| d.display_title()).unwrap_or_default();
            let dirty = app.session.active().is_some_and(|d| d.is_dirty());
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                format!("{title}{}", if dirty { " •" } else { "" }),
                theme::ui_bold(13.0),
                t.text,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if icon_button(ui, Icon::Search, t.text_dim, &l.tr("Search Commands"), vec2(28.0, 26.0)).clicked() {
                    app.open_dialog("commandSearch", json!({}));
                }
                let share = small_button(ui, Icon::Share, &l.tr("Share"), &l.tr("Share: export or save a copy"), true);
                egui::Popup::menu(&share).show(|ui| {
                    for (label, fmt) in [
                        (msg!("Save a Copy as Excel Workbook (.xlsx)…"), "xlsx"),
                        (msg!("Export as CSV…"), "csv"),
                        (msg!("Export as Web Page (.html)…"), "html"),
                    ] {
                        if ui.button(l.tr(label)).clicked() {
                            app.open_dialog("saveCopy", json!({"format": fmt}));
                        }
                    }
                });
                if icon_button(ui, Icon::Comment, t.text_dim, &l.tr("Comments"), vec2(28.0, 26.0)).clicked() {
                    app.open_dialog("comments", json!({}));
                }
            });
        });
    });
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    let collapsed = app.ui.ribbon_collapsed;
    let h = if collapsed { 34.0 } else { 34.0 + 92.0 };
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
                    file_menu(app, ui);
                    let mut tabs: Vec<&str> = TABS.to_vec();
                    let ctx_tabs = contextual_tabs(app);
                    tabs.extend(ctx_tabs.iter());
                    for tab in tabs {
                        let active = app.ui.ribbon_tab == tab;
                        let contextual = ctx_tabs.contains(&tab);
                        let font = theme::ui_font(13.5);
                        let label = tab_name(l, tab);
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
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, &*label, if active { theme::ui_bold(13.5) } else { font }, color);
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
                        if tab == "Home" && app.keytips.prefix() == Some("") {
                            keytip(app, ui, "H", pos2(r.center().x, r.bottom() - 1.0));
                        }
                    }
                });
            });
            if collapsed {
                return;
            }
            // Ribbon card.
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 88.0));
            ui.painter().rect_filled(card, 10.0, t.ribbon);
            ui.painter().rect_stroke(card, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let inner = card.shrink2(vec2(10.0, 6.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
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
                            _ => home(app, ui),
                        }
                    });
                });
            });
        });
}

fn file_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    let name = tab_name(l, "File");
    let font = theme::ui_font(13.5);
    let width = (ui.painter().layout_no_wrap(name.to_string(), font.clone(), t.text).size().x + 22.0).max(46.0);
    let label = egui::RichText::new(name).font(font).color(t.accent);
    let file = ui.add_sized(vec2(width, 30.0), egui::Button::new(label).frame(false));
    egui::Popup::menu(&file).show(|ui| {
        for (label, id, needs_document) in [
            (msg!("New Workbook"), "file.new", false),
            (msg!("Open…"), "file.open", false),
            (msg!("Save"), "file.save", true),
            (msg!("Save As…"), "file.saveAs", true),
            (msg!("Close"), "file.close", true),
        ] {
            if id == "file.save" || id == "file.close" {
                ui.separator();
            }
            if ui.add_enabled(!needs_document || app.session.active().is_some(), egui::Button::new(l.tr(label))).clicked() {
                // File operations must include the cell the user is still editing.
                if app.commit_edit(0, 0, false, false) {
                    app.run_or_alert(id, json!({}));
                }
                ui.close();
            }
        }
    });
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
    }
    v
}

/// Display name of a ribbon tab: the localized name, or the English id when none exists.
fn tab_name(l: Localizer, tab: &str) -> Cow<'static, str> {
    l.ribbon_tab(tab).unwrap_or_else(|| Cow::Owned(tab.to_string()))
}

fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

fn active_style(app: &SheetApp) -> Style {
    app.session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            Some(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
        })
        .unwrap_or_default()
}

fn theme_colors(app: &SheetApp) -> [u32; 12] {
    app.session.active().map(|d| d.wb.theme.colors).unwrap_or(gridcraft_engine::model::Theme::default().colors)
}

/// Runs a command from a ribbon click (menu-style: no params may open a dialog).
fn act(app: &mut SheetApp, id: &str, params: serde_json::Value) {
    app.run_or_alert(id, params);
}

/// Keytips are painted, not focusable widgets: the grid retains keyboard focus.
/// Keep the English keytip letters in every language so access sequences stay stable.
fn keytip(app: &SheetApp, ui: &Ui, sequence: &str, at: egui::Pos2) {
    let Some(rest) = app.keytips.prefix().and_then(|prefix| sequence.strip_prefix(prefix)).filter(|s| !s.is_empty()) else { return };
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("ribbon_keytips"))).with_clip_rect(ui.clip_rect());
    let text = painter.layout_no_wrap(rest.into(), theme::ui_bold(10.5), t.text);
    let rect = Rect::from_center_size(at, text.size() + vec2(7.0, 3.0));
    painter.rect_filled(rect, 2.0, t.input_bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.5, 1.5), text, t.text);
}

/// Menu entries are `(English label, command id or "dialog:<name>", params)`; `"-"` is a separator.
fn menu_items(app: &mut SheetApp, ui: &mut Ui, l: Localizer, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            ui.separator();
            continue;
        }
        let response = ui.add(egui::Button::new(&*l.tr(label)).frame(false).min_size(vec2(220.0, 22.0)));
        let sequence = match (app.keytips.prefix(), *id) {
            (Some("HV"), "dialog:pasteSpecial") => Some("HVS"),
            (Some("E"), "dialog:pasteSpecial") => Some("ES"),
            (Some("HO"), "home.autofitColumnWidth") => Some("HOI"),
            (Some("OC"), "home.autofitColumnWidth") => Some("OCA"),
            _ => None,
        };
        if let Some(sequence) = sequence {
            keytip(app, ui, sequence, pos2(response.rect.right() - 10.0, response.rect.center().y));
        }
        if response.clicked() {
            if let Some(d) = id.strip_prefix("dialog:") {
                app.open_dialog(d, p.clone());
            } else {
                act(app, id, p.clone());
            }
        }
    }
}

fn home(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let t = Tokens::get(ui.ctx());
    let st = active_style(app);
    let formula_lang = app.session.locale().formula;
    // Clipboard
    let paste = big_button(ui, Icon::Paste, &l.tr("Paste"), &tip(&l, app.keymap.platform(), "Paste", "edit.paste"), true);
    if app.keytips.prefix() == Some("H") {
        keytip(app, ui, "HV", paste.rect.center_bottom());
    }
    if paste.clicked() {
        if let Some(text) = app.grid.system_clipboard.take() {
            act(app, "edit.paste", json!({"text": text}));
        } else {
            act(app, "edit.paste", json!({}));
        }
    }
    let paste_keys = matches!(app.keytips.prefix(), Some("HV" | "E"));
    // Keep a keyboard-opened menu anchored to the button when a pointer click
    // cancels keytips, so its items remain in place and can still be clicked.
    let anchor_id = paste.id.with("keytip_anchor");
    let anchored = ui.ctx().data_mut(|data| {
        let anchored = if paste_keys {
            true
        } else if paste.secondary_clicked() {
            false
        } else {
            data.get_temp::<bool>(anchor_id).unwrap_or(false)
        };
        data.insert_temp(anchor_id, anchored);
        anchored
    });
    let mut paste_popup = egui::Popup::context_menu(&paste);
    if anchored {
        paste_popup = paste_popup.anchor(&paste);
    }
    if paste_keys {
        paste_popup = paste_popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    paste_popup.show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Paste"), "edit.paste", json!({})),
                (msg!("Paste Values"), "edit.pasteSpecial", json!({"what": "values"})),
                (msg!("Paste Formulas"), "edit.pasteSpecial", json!({"what": "formulas"})),
                (msg!("Paste Formatting"), "edit.pasteSpecial", json!({"what": "formats"})),
                (msg!("Transpose"), "edit.pasteSpecial", json!({"what": "all", "transpose": true})),
                (msg!("Paste Link"), "edit.pasteSpecial", json!({"what": "all", "link": true})),
                ("-", "", json!(null)),
                (msg!("Paste Special…"), "dialog:pasteSpecial", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        if icon_button(ui, Icon::Cut, t.text, &tip(&l, app.keymap.platform(), "Cut", "edit.cut"), vec2(26.0, 23.0)).clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.cut");
        }
        if icon_button(ui, Icon::Copy, t.text, &tip(&l, app.keymap.platform(), "Copy", "edit.copy"), vec2(26.0, 23.0)).clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.copy");
        }
        let fp_on = app.session.format_painter.is_some();
        let fp = toggle_button(ui, Icon::Brush, fp_on, &l.tr("Format Painter (double-click to keep it on)"));
        if fp.double_clicked() {
            act(app, "edit.formatPainter", json!({"sticky": true}));
        } else if fp.clicked() {
            if fp_on {
                app.session.format_painter = None;
            } else {
                act(app, "edit.formatPainter", json!({}));
            }
        }
    });
    sep(ui);
    // Font
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            font_combo(app, ui, &st);
            size_combo(app, ui, &st);
            if icon_button(ui, Icon::Plus, t.text, &l.tr("Increase Font Size"), vec2(22.0, 23.0)).clicked() {
                act(app, "home.increaseFontSize", json!({}));
            }
            if icon_button(ui, Icon::Minus, t.text, &l.tr("Decrease Font Size"), vec2(22.0, 23.0)).clicked() {
                act(app, "home.decreaseFontSize", json!({}));
            }
        });
        ui.horizontal(|ui| {
            if toggle_button(ui, Icon::Bold, st.font.bold, &tip(&l, app.keymap.platform(), "Bold", "home.bold")).clicked() {
                act(app, "home.bold", json!({}));
            }
            if toggle_button(ui, Icon::Italic, st.font.italic, &tip(&l, app.keymap.platform(), "Italic", "home.italic")).clicked() {
                act(app, "home.italic", json!({}));
            }
            let (u, ua) = split_button(ui, Icon::Underline, None, &tip(&l, app.keymap.platform(), "Underline", "home.underline"));
            if u {
                act(app, "home.underline", json!({}));
            }
            egui::Popup::menu(&ua).show(|ui| {
                menu_items(
                    app,
                    ui,
                    l,
                    &[
                        (msg!("Underline"), "home.underline", json!({"style": "single"})),
                        (msg!("Double Underline"), "home.underline", json!({"style": "double"})),
                    ],
                );
            });
            if toggle_button(ui, Icon::Strike, st.font.strike, &l.tr("Strikethrough")).clicked() {
                act(app, "home.strikethrough", json!({}));
            }
            let (b, ba) = split_button(ui, Icon::Borders, None, &l.tr("Borders"));
            if b {
                act(app, "home.borders", json!({"preset": app.grid.last_border.clone()}));
            }
            ba.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), l.tr("Border presets")));
            egui::Popup::menu(&ba).show(|ui| {
                for (label, preset) in [
                    (msg!("Bottom Border"), "bottom"),
                    (msg!("Top Border"), "top"),
                    (msg!("Left Border"), "left"),
                    (msg!("Right Border"), "right"),
                    (msg!("No Border"), "none"),
                    (msg!("All Borders"), "all"),
                    (msg!("Outside Borders"), "outside"),
                    (msg!("Thick Outside Borders"), "thickOutside"),
                    (msg!("Bottom Double Border"), "doubleBottom"),
                    (msg!("Thick Bottom Border"), "thickBottom"),
                    (msg!("Top and Bottom Border"), "topBottom"),
                    (msg!("Top and Thick Bottom Border"), "topThickBottom"),
                    (msg!("Top and Double Bottom Border"), "topDoubleBottom"),
                    (msg!("Inside Borders"), "inside"),
                ] {
                    if crate::border_preview::preset_button(ui, &l.tr(label), preset).clicked() {
                        app.grid.last_border = preset.to_string();
                        act(app, "home.borders", json!({"preset": preset}));
                    }
                }
                ui.separator();
                if ui.button(l.tr("More Borders…")).clicked() {
                    app.open_dialog("formatCells", json!({"tab": "Border"}));
                }
            });
            let fill = app.grid.last_fill.clone();
            let fill_c = gridcraft_engine::model::Color::from_hex(&fill).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (f, fa) = split_button(ui, Icon::Fill, fill_c, &l.tr("Fill Color"));
            if f {
                act(app, "home.fillColor", json!({"color": fill}));
            }
            egui::Popup::menu(&fa).show(|ui| {
                if let Some(c) = color_palette(ui, l, &theme_colors(app), &l.tr("No Fill")) {
                    if c != "none" {
                        app.grid.last_fill = c.clone();
                    }
                    act(app, "home.fillColor", json!({"color": c}));
                    ui.close();
                }
            });
            let fc = app.grid.last_font_color.clone();
            let fc_c = gridcraft_engine::model::Color::from_hex(&fc).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (c, ca) = split_button(ui, Icon::FontColor, fc_c, &l.tr("Font Color"));
            if c {
                act(app, "home.fontColor", json!({"color": fc}));
            }
            egui::Popup::menu(&ca).show(|ui| {
                if let Some(c) = color_palette(ui, l, &theme_colors(app), &l.tr("Automatic")) {
                    if c != "none" {
                        app.grid.last_font_color = c.clone();
                    }
                    act(app, "home.fontColor", json!({"color": if c == "none" { "auto".to_string() } else { c }}));
                    ui.close();
                }
            });
        });
    });
    sep(ui);
    // Alignment
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, v, id, caption) in [
                (Icon::AlignTop, VAlign::Top, "home.alignTop", msg!("Top Align")),
                (Icon::AlignMiddle, VAlign::Center, "home.alignMiddle", msg!("Middle Align")),
                (Icon::AlignBottom, VAlign::Bottom, "home.alignBottom", msg!("Bottom Align")),
            ] {
                if toggle_button(ui, icon, st.align.v == v, &l.tr(caption)).clicked() {
                    act(app, id, json!({}));
                }
            }
            let o = small_button(ui, Icon::Orientation, "", &l.tr("Orientation"), true);
            egui::Popup::menu(&o).show(|ui| {
                menu_items(
                    app,
                    ui,
                    l,
                    &[
                        (msg!("Angle Counterclockwise"), "home.orientation", json!({"angle": "counterclockwise"})),
                        (msg!("Angle Clockwise"), "home.orientation", json!({"angle": "clockwise"})),
                        (msg!("Vertical Text"), "home.orientation", json!({"angle": "vertical"})),
                        (msg!("Rotate Text Up"), "home.orientation", json!({"angle": "up"})),
                        (msg!("Rotate Text Down"), "home.orientation", json!({"angle": "down"})),
                        (msg!("Horizontal"), "home.orientation", json!({"angle": 0})),
                        ("-", "", json!(null)),
                        (msg!("Format Cell Alignment…"), "dialog:formatCells", json!({"tab": "Alignment"})),
                    ],
                );
            });
            let wrap = toggle_button(ui, Icon::Wrap, st.align.wrap, &l.text("ui-ribbon-wrap-text", &[]));
            if wrap.clicked() {
                act(app, "home.wrapText", json!({}));
            }
            ui.label(egui::RichText::new(l.text("ui-ribbon-wrap-text", &[])).font(theme::ui_font(12.5)));
        });
        ui.horizontal(|ui| {
            for (icon, h, id, caption) in [
                (Icon::AlignLeft, HAlign::Left, "home.alignLeft", msg!("Align Left")),
                (Icon::AlignCenter, HAlign::Center, "home.alignCenter", msg!("Center")),
                (Icon::AlignRight, HAlign::Right, "home.alignRight", msg!("Align Right")),
            ] {
                let response = toggle_button(ui, icon, st.align.h == h, &l.tr(caption));
                if matches!(app.keytips.prefix(), Some("H" | "HA")) {
                    let sequence = match h {
                        HAlign::Left => "HAL",
                        HAlign::Center => "HAC",
                        _ => "HAR",
                    };
                    keytip(app, ui, sequence, response.rect.center_bottom());
                }
                if response.clicked() {
                    act(app, id, json!({}));
                }
            }
            if icon_button(ui, Icon::IndentDec, t.text, &l.tr("Decrease Indent"), vec2(24.0, 23.0)).clicked() {
                act(app, "home.decreaseIndent", json!({}));
            }
            if icon_button(ui, Icon::IndentInc, t.text, &l.tr("Increase Indent"), vec2(24.0, 23.0)).clicked() {
                act(app, "home.increaseIndent", json!({}));
            }
            let (m, ma) = split_button(ui, Icon::Merge, None, &l.tr("Merge & Center"));
            if m {
                act(app, "home.mergeCenter", json!({}));
            }
            ui.label(egui::RichText::new(l.tr("Merge & Center")).font(theme::ui_font(12.5)));
            egui::Popup::menu(&ma).show(|ui| {
                menu_items(
                    app,
                    ui,
                    l,
                    &[
                        (msg!("Merge & Center"), "home.mergeCenter", json!({})),
                        (msg!("Merge Across"), "home.mergeAcross", json!({})),
                        (msg!("Merge Cells"), "home.mergeCells", json!({})),
                        (msg!("Unmerge Cells"), "home.unmergeCells", json!({})),
                    ],
                );
            });
        });
    });
    sep(ui);
    // Number
    ui.vertical(|ui| {
        number_combo(app, ui, &st);
        ui.horizontal(|ui| {
            let (c, ca) = split_button(ui, Icon::Currency, None, &l.tr("Accounting Number Format"));
            if c {
                act(app, "home.accounting", json!({}));
            }
            egui::Popup::menu(&ca).show(|ui| {
                menu_items(
                    app,
                    ui,
                    l,
                    &[
                        (
                            msg!("$ English (United States)"),
                            "home.numberFormat",
                            json!({"code": "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"}),
                        ),
                        (
                            msg!("£ English (United Kingdom)"),
                            "home.numberFormat",
                            json!({"code": "_-[$£-809]* #,##0.00_-;-[$£-809]* #,##0.00_-;_-[$£-809]* \"-\"??_-;_-@_-"}),
                        ),
                        (
                            msg!("€ Euro"),
                            "home.numberFormat",
                            json!({"code": "_-[$€-x-euro2] * #,##0.00_-;-[$€-x-euro2] * #,##0.00_-;_-[$€-x-euro2] * \"-\"??_-;_-@_-"}),
                        ),
                        (
                            msg!("¥ Japanese"),
                            "home.numberFormat",
                            json!({"code": "_-[$¥-411]* #,##0_-;-[$¥-411]* #,##0_-;_-[$¥-411]* \"-\"_-;_-@_-"}),
                        ),
                        ("-", "", json!(null)),
                        (msg!("More Accounting Formats…"), "dialog:formatCells", json!({"tab": "Number"})),
                    ],
                );
            });
            if icon_button(ui, Icon::Percent, t.text, &l.tr("Percent Style"), vec2(24.0, 23.0)).clicked() {
                act(app, "home.percent", json!({}));
            }
            if icon_button(ui, Icon::Comma, t.text, &l.tr("Comma Style"), vec2(24.0, 23.0)).clicked() {
                act(app, "home.comma", json!({}));
            }
            if icon_button(ui, Icon::DecInc, t.text, &l.tr("Increase Decimal"), vec2(26.0, 23.0)).clicked() {
                act(app, "home.increaseDecimal", json!({}));
            }
            if icon_button(ui, Icon::DecDec, t.text, &l.tr("Decrease Decimal"), vec2(26.0, 23.0)).clicked() {
                act(app, "home.decreaseDecimal", json!({}));
            }
        });
    });
    sep(ui);
    // Styles
    let cf = big_button(ui, Icon::CondFormat, &l.text("ui-ribbon-label-conditional-formatting", &[]), &l.tr("Conditional Formatting"), true);
    egui::Popup::menu(&cf).show(|ui| cf_menu(app, ui));
    let ft = big_button(ui, Icon::FormatTable, &l.text("ui-ribbon-label-format-as-table", &[]), &l.tr("Format as Table"), true);
    egui::Popup::menu(&ft).show(|ui| table_gallery(app, ui, "home.formatAsTable"));
    let cs = big_button(ui, Icon::CellStyles, &l.text("ui-ribbon-label-cell-styles", &[]), &l.tr("Cell Styles"), true);
    egui::Popup::menu(&cs).show(|ui| cell_style_gallery(app, ui));
    sep(ui);
    // Cells
    ui.vertical(|ui| {
        let ins = small_button(ui, Icon::Insert, &l.tr("Insert"), &l.tr("Insert"), true);
        egui::Popup::menu(&ins).show(|ui| {
            menu_items(
                app,
                ui,
                l,
                &[
                    (msg!("Insert Cells…"), "dialog:insertCells", json!({})),
                    (msg!("Insert Sheet Rows"), "home.insertRows", json!({})),
                    (msg!("Insert Sheet Columns"), "home.insertColumns", json!({})),
                    (msg!("Insert Sheet"), "home.insertSheet", json!({})),
                ],
            );
        });
        let del = small_button(ui, Icon::Delete, &l.tr("Delete"), &l.tr("Delete"), true);
        egui::Popup::menu(&del).show(|ui| {
            menu_items(
                app,
                ui,
                l,
                &[
                    (msg!("Delete Cells…"), "dialog:deleteCells", json!({})),
                    (msg!("Delete Sheet Rows"), "home.deleteRows", json!({})),
                    (msg!("Delete Sheet Columns"), "home.deleteColumns", json!({})),
                    (msg!("Delete Sheet"), "home.deleteSheet", json!({})),
                ],
            );
        });
        let fmt = small_button(ui, Icon::Format, &l.tr("Format"), &l.tr("Format"), true);
        match app.keytips.prefix() {
            Some("H") => keytip(app, ui, "HO", fmt.rect.center_bottom()),
            Some("O") => keytip(app, ui, "OC", fmt.rect.center_bottom()),
            _ => {}
        }
        let format_keys = matches!(app.keytips.prefix(), Some("HO" | "OC"));
        let mut format_popup = egui::Popup::menu(&fmt);
        if format_keys {
            format_popup = format_popup.open_memory(egui::SetOpenCommand::Bool(true));
        }
        format_popup.show(|ui| {
            menu_items(
                app,
                ui,
                l,
                &[
                    (msg!("Row Height…"), "dialog:rowHeight", json!({})),
                    (msg!("AutoFit Row Height"), "home.autofitRowHeight", json!({})),
                    (msg!("Column Width…"), "dialog:columnWidth", json!({})),
                    (msg!("AutoFit Column Width"), "home.autofitColumnWidth", json!({})),
                    (msg!("Default Width…"), "dialog:defaultWidth", json!({})),
                    ("-", "", json!(null)),
                    (msg!("Hide Rows"), "home.hideRows", json!({})),
                    (msg!("Hide Columns"), "home.hideColumns", json!({})),
                    (msg!("Unhide Rows"), "home.unhideRows", json!({})),
                    (msg!("Unhide Columns"), "home.unhideColumns", json!({})),
                    (msg!("Hide Sheet"), "sheet.hide", json!({})),
                    (msg!("Unhide Sheet…"), "sheet.unhide", json!({})),
                    ("-", "", json!(null)),
                    (msg!("Rename Sheet"), "dialog:renameSheet", json!({})),
                    (msg!("Move or Copy Sheet…"), "dialog:moveSheet", json!({})),
                    ("-", "", json!(null)),
                    (msg!("Protect Sheet…"), "dialog:protectSheet", json!({})),
                    (msg!("Lock Cell"), "home.lockCell", json!({})),
                    (msg!("Format Cells…"), "dialog:formatCells", json!({})),
                ],
            );
        });
    });
    sep(ui);
    // Editing
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let (s, sa) = split_button(ui, Icon::Sum, None, &tip(&l, app.keymap.platform(), "AutoSum", "formulas.autoSum"));
            if s {
                act(app, "formulas.autoSum", json!({}));
            }
            egui::Popup::menu(&sa).show(|ui| {
                for f in ["SUM", "AVERAGE", "COUNT", "MAX", "MIN"] {
                    if ui.button(formula_lang.local_function(f).unwrap_or(f)).clicked() {
                        act(app, "formulas.autoSum", json!({"function": f}));
                    }
                }
                ui.separator();
                if ui.button(l.tr("More Functions…")).clicked() {
                    app.open_dialog("insertFunction", json!({}));
                }
            });
        });
        let fill = small_button(ui, Icon::FillDown, "", &l.tr("Fill"), true);
        egui::Popup::menu(&fill).show(|ui| {
            menu_items(
                app,
                ui,
                l,
                &[
                    (msg!("Down"), "edit.fillDown", json!({})),
                    (msg!("Right"), "edit.fillRight", json!({})),
                    (msg!("Up"), "edit.fillUp", json!({})),
                    (msg!("Left"), "edit.fillLeft", json!({})),
                    (msg!("Series…"), "dialog:series", json!({})),
                    (msg!("Flash Fill"), "edit.flashFill", json!({})),
                ],
            );
        });
        let clear = small_button(ui, Icon::Eraser, "", &l.tr("Clear"), true);
        egui::Popup::menu(&clear).show(|ui| {
            menu_items(
                app,
                ui,
                l,
                &[
                    (msg!("Clear All"), "edit.clearAll", json!({})),
                    (msg!("Clear Formats"), "edit.clearFormats", json!({})),
                    (msg!("Clear Contents"), "edit.clearContents", json!({})),
                    (msg!("Clear Comments and Notes"), "edit.clearComments", json!({})),
                    (msg!("Clear Hyperlinks"), "edit.clearHyperlinks", json!({})),
                ],
            );
        });
    });
    let sf = big_button(ui, Icon::SortFilter, &l.text("ui-ribbon-label-sort-filter", &[]), &l.tr("Sort & Filter"), true);
    egui::Popup::menu(&sf).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Sort A to Z"), "data.sortAscending", json!({})),
                (msg!("Sort Z to A"), "data.sortDescending", json!({})),
                (msg!("Custom Sort…"), "dialog:sort", json!({})),
                ("-", "", json!(null)),
                (msg!("Filter"), "data.filter", json!({})),
                (msg!("Clear"), "data.clearFilter", json!({})),
                (msg!("Reapply"), "data.reapply", json!({})),
            ],
        );
    });
    let fs = big_button(ui, Icon::Find, &l.text("ui-ribbon-label-find-select", &[]), &l.tr("Find & Select"), true);
    egui::Popup::menu(&fs).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Find…"), "dialog:find", json!({})),
                (msg!("Replace…"), "dialog:find", json!({"replace": true})),
                (msg!("Go To…"), "dialog:goTo", json!({})),
                (msg!("Go To Special…"), "dialog:goToSpecial", json!({})),
                ("-", "", json!(null)),
                (msg!("Formulas"), "edit.goToSpecial", json!({"kind": "formulas"})),
                (msg!("Notes"), "edit.goToSpecial", json!({"kind": "notes"})),
                (msg!("Conditional Formatting"), "edit.goToSpecial", json!({"kind": "conditionalFormats"})),
                (msg!("Constants"), "edit.goToSpecial", json!({"kind": "constants"})),
                (msg!("Data Validation"), "edit.goToSpecial", json!({"kind": "dataValidation"})),
            ],
        );
    });
}

fn font_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let mut name = st.font.name.clone();
    let before = name.clone();
    egui::ComboBox::from_id_salt("font_name").width(150.0).selected_text(egui::RichText::new(&name).font(theme::ui_font(12.5))).show_ui(ui, |ui| {
        for f in FONTS {
            let fam = theme::cell_family(f, false, false);
            ui.selectable_value(&mut name, f.to_string(), egui::RichText::new(*f).font(egui::FontId::new(14.0, fam)));
        }
    });
    if name != before {
        act(app, "home.fontName", json!({"name": name}));
    }
}

pub const FONTS: &[&str] = &[
    "Aptos",
    "Aptos Narrow",
    "Arial",
    "Calibri",
    "Cambria",
    "Candara",
    "Consolas",
    "Courier New",
    "Garamond",
    "Georgia",
    "Helvetica",
    "Helvetica Neue",
    "Segoe UI",
    "Tahoma",
    "Times New Roman",
    "Trebuchet MS",
    "Verdana",
];

fn size_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let mut size = st.font.size;
    let before = size;
    // A size such as 10.5 is shown with the region's decimal separator (`10,5`).
    let label = if size.fract() == 0.0 { format!("{}", size as i32) } else { app.localize_decimal(&format!("{size}")) };
    egui::ComboBox::from_id_salt("font_size").width(52.0).selected_text(egui::RichText::new(label).font(theme::ui_font(12.5))).show_ui(ui, |ui| {
        for s in [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0] {
            ui.selectable_value(&mut size, s, format!("{}", s as i32));
        }
    });
    if size != before {
        act(app, "home.fontSize", json!({"size": size}));
    }
}

fn number_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let l = app.l10n;
    let code = st.num_fmt.as_str().to_string();
    let kind = gridcraft_engine::display::number_format(&code).kind();
    let current = format!("{kind:?}");
    let shown = match current.as_str() {
        "General" => l.tr("General"),
        "Number" => l.tr("Number"),
        "Currency" => l.tr("Currency"),
        "Accounting" => l.tr("Accounting"),
        "Date" => l.tr("Date"),
        "Time" => l.tr("Time"),
        "Percentage" => l.tr("Percentage"),
        "Fraction" => l.tr("Fraction"),
        "Scientific" => l.tr("Scientific"),
        "Text" => l.tr("Text"),
        "Special" => l.tr("Special"),
        "Custom" => l.tr("Custom"),
        other => Cow::Owned(other.to_string()),
    };
    let sample = app.session.active().and_then(|d| d.wb.active().map(|sh| sh.value(d.selection.active))).unwrap_or_default();
    let mut picked: Option<&str> = None;
    egui::ComboBox::from_id_salt("number_format").width(150.0).selected_text(egui::RichText::new(shown).font(theme::ui_font(12.5))).show_ui(
        ui,
        |ui| {
            for name in [
                msg!("General"),
                msg!("Number"),
                msg!("Currency"),
                msg!("Accounting"),
                msg!("Short Date"),
                msg!("Long Date"),
                msg!("Time"),
                msg!("Percentage"),
                msg!("Fraction"),
                msg!("Scientific"),
                msg!("Text"),
            ] {
                let code = gridcraft_engine::cmd::format::format_code_for(name);
                let preview = app.session.active().map(|d| gridcraft_engine::display::format(&sample, code, &d.wb).text).unwrap_or_default();
                let r = ui.add(egui::Button::selectable(current == name, format!("{:<12}   {preview}", l.tr(name))).min_size(vec2(240.0, 22.0)));
                if r.clicked() {
                    picked = Some(name);
                }
            }
            ui.separator();
            if ui.button(l.tr("More Number Formats…")).clicked() {
                picked = Some("__more");
            }
        },
    );
    match picked {
        Some("__more") => app.open_dialog("formatCells", json!({"tab": "Number"})),
        Some(n) => act(app, "home.numberFormat", json!({"format": n})),
        None => {}
    }
}

fn cf_menu(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    ui.menu_button(l.tr("Highlight Cells Rules"), |ui| {
        for (label, op) in
            [(msg!("Greater Than…"), "greater"), (msg!("Less Than…"), "less"), (msg!("Between…"), "between"), (msg!("Equal To…"), "equal")]
        {
            if ui.button(l.tr(label)).clicked() {
                app.open_dialog("cfQuick", json!({"type": "cellIs", "operator": op, "title": label}));
            }
        }
        if ui.button(l.tr("Text that Contains…")).clicked() {
            app.open_dialog("cfQuick", json!({"type": "containsText", "title": msg!("Text that Contains")}));
        }
        if ui.button(l.tr("A Date Occurring…")).clicked() {
            app.open_dialog("cfQuick", json!({"type": "timePeriod", "title": msg!("A Date Occurring")}));
        }
        if ui.button(l.tr("Duplicate Values…")).clicked() {
            act(app, "home.conditionalFormat", json!({"rule": {"type": "duplicate"}}));
        }
    });
    ui.menu_button(l.tr("Top/Bottom Rules"), |ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Top 10 Items"), "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10}})),
                (msg!("Top 10%"), "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "percent": true}})),
                (msg!("Bottom 10 Items"), "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true}})),
                (msg!("Bottom 10%"), "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true, "percent": true}})),
                (msg!("Above Average"), "home.conditionalFormat", json!({"rule": {"type": "aboveAverage"}})),
                (msg!("Below Average"), "home.conditionalFormat", json!({"rule": {"type": "aboveAverage", "below": true}})),
            ],
        );
    });
    ui.menu_button(l.tr("Data Bars"), |ui| {
        for (label, c) in [
            (msg!("Blue"), "#638EC6"),
            (msg!("Green"), "#63BE7B"),
            (msg!("Red"), "#F8696B"),
            (msg!("Orange"), "#FFB628"),
            (msg!("Light Blue"), "#008AEF"),
            (msg!("Purple"), "#D6007B"),
        ] {
            if ui.button(l.tr(label)).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "dataBar", "color": c}}));
            }
        }
    });
    ui.menu_button(l.tr("Color Scales"), |ui| {
        for (label, cols) in [
            (msg!("Green - Yellow - Red"), vec!["#63BE7B", "#FFEB84", "#F8696B"]),
            (msg!("Red - Yellow - Green"), vec!["#F8696B", "#FFEB84", "#63BE7B"]),
            (msg!("Green - White - Red"), vec!["#63BE7B", "#FCFCFF", "#F8696B"]),
            (msg!("Blue - White - Red"), vec!["#5A8AC6", "#FCFCFF", "#F8696B"]),
            (msg!("White - Green"), vec!["#FCFCFF", "#63BE7B"]),
            (msg!("White - Red"), vec!["#FCFCFF", "#F8696B"]),
        ] {
            if ui.button(l.tr(label)).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "colorScale", "colors": cols}}));
            }
        }
    });
    ui.menu_button(l.tr("Icon Sets"), |ui| {
        for (label, set) in [
            (msg!("3 Arrows"), "3Arrows"),
            (msg!("3 Traffic Lights"), "3TrafficLights1"),
            (msg!("3 Symbols"), "3Symbols"),
            (msg!("4 Ratings"), "4Rating"),
            (msg!("5 Arrows"), "5Arrows"),
            (msg!("5 Quarters"), "5Quarters"),
        ] {
            if ui.button(l.tr(label)).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "iconSet", "set": set}}));
            }
        }
    });
    ui.separator();
    if ui.button(l.tr("New Rule…")).clicked() {
        app.open_dialog("cfQuick", json!({"type": "expression", "title": msg!("New Formatting Rule")}));
    }
    ui.menu_button(l.tr("Clear Rules"), |ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Clear Rules from Selected Cells"), "home.clearRules", json!({})),
                (msg!("Clear Rules from Entire Sheet"), "home.clearRules", json!({"sheet": true})),
            ],
        );
    });
    if ui.button(l.tr("Manage Rules…")).clicked() {
        app.open_dialog("manageRules", json!({}));
    }
}

/// A gallery of table styles drawn as mini tables.
fn table_gallery(app: &mut SheetApp, ui: &mut Ui, cmd: &str) {
    let l = app.l10n;
    ui.set_width(430.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    for (fam, n) in [(msg!("Light"), 21u32), (msg!("Medium"), 28), (msg!("Dark"), 11)] {
        ui.label(egui::RichText::new(l.tr(fam)).strong());
        egui::Grid::new(("tg", fam)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for i in 1..=n {
                let name = format!("TableStyle{fam}{i}");
                let (r, resp) = ui.allocate_exact_size(vec2(54.0, 38.0), Sense::click());
                paint_table_swatch(ui, r, &wb, &name);
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.on_hover_text(&name).clicked() {
                    act(app, cmd, json!({"style": name}));
                    ui.close();
                }
                if i % 7 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}

fn paint_table_swatch(ui: &Ui, r: Rect, wb: &gridcraft_engine::model::Workbook, style: &str) {
    let lk = gridcraft_engine::tables::look(style);
    let res = |c: Option<gridcraft_engine::model::Color>| c.and_then(|c| c.resolve(&wb.theme)).map(theme::color32);
    let p = ui.painter();
    p.rect_filled(r, 0.0, Color32::WHITE);
    let rows = 5;
    let rh = r.height() / rows as f32;
    for i in 0..rows {
        let rr = Rect::from_min_size(pos2(r.left(), r.top() + i as f32 * rh), vec2(r.width(), rh));
        let fill = if i == 0 {
            res(lk.header_fill)
        } else if i % 2 == 1 {
            res(lk.band_fill)
        } else {
            res(lk.body_fill)
        };
        if let Some(f) = fill {
            p.rect_filled(rr, 0.0, f);
        }
        if let Some(l) = res(lk.line) {
            p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, l));
        }
        for k in 0..3 {
            let x = rr.left() + 4.0 + k as f32 * (r.width() - 8.0) / 3.0;
            let col = if i == 0 { res(Some(lk.header_font)).unwrap_or(Color32::BLACK) } else { Color32::from_gray(120) };
            p.line_segment([pos2(x, rr.center().y), pos2(x + 10.0, rr.center().y)], Stroke::new(1.0, col));
        }
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(210)), StrokeKind::Inside);
}

/// Display name of a built-in cell style: the localized message `ui-ribbon-style-<slug>`, or the
/// English name. The English name stays the style id passed to the engine.
fn style_name(l: Localizer, name: &str) -> Cow<'static, str> {
    l.get(&format!("ui-ribbon-style-{}", gridcraft_l10n::slug(name)), &[]).unwrap_or_else(|| Cow::Owned(name.to_string()))
}

fn cell_style_gallery(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    ui.set_width(520.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let groups: &[(&str, &[&str])] = &[
        (msg!("Good, Bad and Neutral"), &["Normal", "Bad", "Good", "Neutral"]),
        (msg!("Data and Model"), &["Calculation", "Check Cell", "Explanatory Text", "Input", "Linked Cell", "Note", "Output", "Warning Text"]),
        (msg!("Titles and Headings"), &["Heading 1", "Heading 2", "Heading 3", "Heading 4", "Title", "Total"]),
        (
            msg!("Themed Cell Styles"),
            &[
                "20% - Accent1",
                "20% - Accent2",
                "20% - Accent3",
                "20% - Accent4",
                "20% - Accent5",
                "20% - Accent6",
                "40% - Accent1",
                "40% - Accent2",
                "40% - Accent3",
                "40% - Accent4",
                "40% - Accent5",
                "40% - Accent6",
                "60% - Accent1",
                "60% - Accent2",
                "60% - Accent3",
                "60% - Accent4",
                "60% - Accent5",
                "60% - Accent6",
                "Accent1",
                "Accent2",
                "Accent3",
                "Accent4",
                "Accent5",
                "Accent6",
            ],
        ),
        (msg!("Number Format"), &["Comma", "Comma [0]", "Currency", "Currency [0]", "Percent"]),
    ];
    for (g, names) in groups {
        ui.label(egui::RichText::new(l.tr(g)).strong());
        egui::Grid::new(("csg", *g)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, n) in names.iter().enumerate() {
                let st = gridcraft_engine::cmd::format::builtin_cell_style(n, &wb.theme).unwrap_or_default();
                let (r, resp) = ui.allocate_exact_size(vec2(82.0, 24.0), Sense::click());
                let fill = st
                    .fill
                    .fg
                    .resolve(&wb.theme)
                    .filter(|_| st.fill.pattern != gridcraft_engine::model::PatternType::None)
                    .map(theme::color32)
                    .unwrap_or(Color32::WHITE);
                ui.painter().rect_filled(r, 0.0, fill);
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(210)), StrokeKind::Inside);
                if !st.borders.bottom.is_none() {
                    let c = st.borders.bottom.color.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::BLACK);
                    ui.painter().line_segment(
                        [r.left_bottom() + vec2(0.0, -1.5), r.right_bottom() + vec2(0.0, -1.5)],
                        Stroke::new(st.borders.bottom.style.width(), c),
                    );
                }
                let col = st.font.color.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::BLACK);
                let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
                ui.painter().text(
                    r.left_center() + vec2(4.0, 0.0),
                    Align2::LEFT_CENTER,
                    style_name(l, n),
                    egui::FontId::new((st.font.size).clamp(10.0, 15.0), fam),
                    col,
                );
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.clicked() {
                    act(app, "home.cellStyle", json!({"name": n}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}

/// Hover name of a library icon (`line chart`): the localized message `ui-ribbon-icon-<slug>`, or the English name.
fn icon_name(l: Localizer, name: &str) -> Cow<'static, str> {
    l.get(&format!("ui-ribbon-icon-{}", gridcraft_l10n::slug(name)), &[]).unwrap_or_else(|| Cow::Owned(name.to_string()))
}

fn insert(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    if big_button(ui, Icon::PivotTable, &l.tr("PivotTable"), &l.tr("PivotTable"), false).clicked() {
        act(app, "insert.pivotTable", json!({}));
    }
    if big_button(ui, Icon::Table, &l.tr("Table"), &tip(&l, app.keymap.platform(), "Table", "insert.table"), false).clicked() {
        act(app, "insert.table", json!({}));
    }
    sep(ui);
    let pictures = l.tr("Pictures");
    let pic = big_button(ui, Icon::Picture, &pictures, &l.tr("Insert a picture"), true);
    pic.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &*pictures));
    egui::Popup::menu(&pic).show(|ui| {
        for (label, placement) in [(msg!("Place in Cell"), "cell"), (msg!("Place over Cells"), "overCells")] {
            if ui.button(&*l.tr(label)).clicked() {
                app.open_dialog("insertPicture", json!({"placement": placement}));
                ui.close();
            }
        }
    });
    let shapes = big_button(ui, Icon::Shapes, &l.tr("Shapes"), &l.tr("Shapes"), true);
    egui::Popup::menu(&shapes).show(|ui| {
        for (label, kind) in [
            (msg!("Rectangle"), "rectangle"),
            (msg!("Rounded Rectangle"), "roundedRectangle"),
            (msg!("Oval"), "ellipse"),
            (msg!("Triangle"), "triangle"),
            (msg!("Line"), "line"),
            (msg!("Arrow"), "arrow"),
            (msg!("Text Box"), "textBox"),
        ] {
            if ui.button(l.tr(label)).clicked() {
                act(app, "insert.shape", json!({"kind": kind}));
            }
        }
    });
    let icn = big_button(ui, Icon::Theme, &l.tr("Icons"), &l.tr("Insert an icon"), true);
    egui::Popup::menu(&icn).show(|ui| {
        ui.set_width(260.0);
        egui::Grid::new("icon_lib").show(ui, |ui| {
            for (i, (name, icon)) in icons::LIBRARY.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, Tokens::get(ui.ctx()).hover);
                }
                icons::paint(ui.painter(), r.shrink(6.0), *icon, Tokens::get(ui.ctx()).text);
                if resp.on_hover_text(icon_name(l, name)).clicked() {
                    act(app, "insert.icons", json!({"name": name}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    });
    sep(ui);
    if big_button(ui, Icon::Chart, &l.text("ui-ribbon-label-recommended-charts", &[]), &l.tr("Recommended Charts"), false).clicked() {
        act(app, "insert.recommendedCharts", json!({}));
    }
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, caption, kind, subs) in [
                (
                    Icon::ChartBar,
                    msg!("Column or Bar"),
                    "column",
                    vec![
                        (msg!("Clustered Column"), "column", ""),
                        (msg!("Stacked Column"), "column", "stacked"),
                        (msg!("100% Stacked Column"), "column", "stacked100"),
                        (msg!("Clustered Bar"), "bar", ""),
                        (msg!("Stacked Bar"), "bar", "stacked"),
                    ],
                ),
                (
                    Icon::ChartLine,
                    msg!("Line or Area"),
                    "line",
                    vec![
                        (msg!("Line"), "line", ""),
                        (msg!("Line with Markers"), "line", "markers"),
                        (msg!("Stacked Line"), "line", "stacked"),
                        (msg!("Area"), "area", ""),
                        (msg!("Stacked Area"), "area", "stacked"),
                    ],
                ),
                (Icon::ChartPie, msg!("Pie or Doughnut"), "pie", vec![(msg!("Pie"), "pie", ""), (msg!("Doughnut"), "doughnut", "")]),
            ] {
                let r = small_button(ui, icon, "", &l.tr(caption), true);
                let _ = kind;
                egui::Popup::menu(&r).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(l.tr(label)).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            for (icon, caption, subs) in [
                (
                    Icon::ChartScatter,
                    msg!("Scatter or Bubble"),
                    vec![(msg!("Scatter"), "scatter", ""), (msg!("Scatter with Lines"), "scatter", "lines"), (msg!("Bubble"), "bubble", "")],
                ),
                (Icon::Chart, msg!("Statistic"), vec![(msg!("Histogram"), "histogram", ""), (msg!("Box & Whisker"), "boxWhisker", "")]),
                (
                    Icon::Theme,
                    msg!("Hierarchy & more"),
                    vec![
                        (msg!("Treemap"), "treemap", ""),
                        (msg!("Sunburst"), "sunburst", ""),
                        (msg!("Waterfall"), "waterfall", ""),
                        (msg!("Funnel"), "funnel", ""),
                        (msg!("Radar"), "radar", ""),
                        (msg!("Stock"), "stock", ""),
                        (msg!("Combo"), "combo", ""),
                    ],
                ),
            ] {
                let r = small_button(ui, icon, "", &l.tr(caption), true);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(l.tr(label)).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
    });
    sep(ui);
    let sp = big_button(ui, Icon::Sparkline, &l.tr("Sparklines"), &l.tr("Sparklines"), true);
    egui::Popup::menu(&sp).show(|ui| {
        for (label, kind) in [(msg!("Line"), "line"), (msg!("Column"), "column"), (msg!("Win/Loss"), "winLoss")] {
            if ui.button(l.tr(label)).clicked() {
                app.open_dialog("sparkline", json!({"type": kind}));
            }
        }
    });
    sep(ui);
    if big_button(ui, Icon::Link, &l.tr("Link"), &tip(&l, app.keymap.platform(), "Insert Link", "insert.link"), false).clicked() {
        app.open_dialog("insertLink", json!({}));
    }
    if big_button(ui, Icon::Comment, &l.text("ui-ribbon-label-comment", &[]), &l.tr("New Comment"), false).clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    sep(ui);
    if big_button(ui, Icon::TextBox, &l.text("ui-ribbon-label-text-box", &[]), &l.tr("Text Box"), false).clicked() {
        act(app, "insert.textBox", json!({}));
    }
    if big_button(ui, Icon::PageLayout, &l.text("ui-ribbon-label-header-footer", &[]), &l.tr("Header & Footer"), false).clicked() {
        app.open_dialog("headerFooter", json!({}));
    }
    let sym = big_button(ui, Icon::Symbol, &l.tr("Symbol"), &l.tr("Symbol"), true);
    egui::Popup::menu(&sym).show(|ui| {
        ui.set_width(260.0);
        ui.horizontal_wrapped(|ui| {
            for ch in [
                "€", "£", "¥", "©", "®", "™", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Σ", "Ω", "°", "•", "…", "→", "←", "↑", "↓",
                "✓", "★", "½", "¼",
            ] {
                if ui.button(ch).clicked() {
                    act(app, "insert.symbol", json!({"char": ch}));
                }
            }
        });
    });
}

fn draw(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let tool = app.session.draw_tool.clone();
    if big_button(ui, Icon::Shapes, &l.text("ui-ribbon-label-select-objects", &[]), &l.tr("Select and move ink and shapes"), false).clicked() {
        act(app, "draw.lasso", json!({}));
    }
    sep(ui);
    for (label, color, highlighter) in [
        (l.text("ui-ribbon-label-pen-blue", &[]), "#1F5FC9", false),
        (l.text("ui-ribbon-label-pen-black", &[]), "#000000", false),
        (l.text("ui-ribbon-label-pen-red", &[]), "#D13B2F", false),
        (l.tr("Highlighter"), "#F2E33A", true),
    ] {
        let on = tool == "pen" && app.session.draw_color.eq_ignore_ascii_case(color);
        let r = big_button(ui, Icon::Pen, &label, &l.tr("Draw with ink"), false);
        if on {
            ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
        }
        if r.clicked() {
            let width = if highlighter { 12.0 } else { 2.0 };
            act(app, "draw.pen", json!({"on": !on, "color": color, "width": width}));
        }
    }
    let er = big_button(ui, Icon::Eraser, &l.tr("Eraser"), &l.tr("Erase ink strokes"), false);
    if tool == "eraser" {
        ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if er.clicked() {
        act(app, "draw.eraser", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Shapes, &l.text("ui-ribbon-label-ink-to-shape", &[]), &l.tr("Convert the last ink stroke to a shape"), false).clicked() {
        act(app, "draw.inkToShape", json!({}));
    }
}

fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let th = big_button(ui, Icon::Theme, &l.tr("Themes"), &l.tr("Themes"), true);
    egui::Popup::menu(&th).show(|ui| {
        for (name, colors) in gridcraft_engine::cmd::view::themes() {
            ui.horizontal(|ui| {
                for c in colors.iter().skip(4).take(6) {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
                }
                if ui.button(name).clicked() {
                    act(app, "pageLayout.theme", json!({"name": name}));
                }
            });
        }
    });
    sep(ui);
    let m = big_button(ui, Icon::Margins, &l.tr("Margins"), &l.tr("Margins"), true);
    egui::Popup::menu(&m).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Normal"), "pageLayout.margins", json!({"preset": "normal"})),
                (msg!("Wide"), "pageLayout.margins", json!({"preset": "wide"})),
                (msg!("Narrow"), "pageLayout.margins", json!({"preset": "narrow"})),
                (msg!("Custom Margins…"), "dialog:pageSetup", json!({})),
            ],
        );
    });
    let orientation = l.text("ui-ribbon-page-layout-orientation", &[]);
    let o = big_button(ui, Icon::Orient, &orientation, &orientation, true);
    egui::Popup::menu(&o).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Portrait"), "pageLayout.orientation", json!({"orientation": "portrait"})),
                (msg!("Landscape"), "pageLayout.orientation", json!({"orientation": "landscape"})),
            ],
        );
    });
    let s = big_button(ui, Icon::Paper, &l.tr("Size"), &l.tr("Paper Size"), true);
    egui::Popup::menu(&s).show(|ui| {
        for p in [msg!("Letter"), msg!("Legal"), msg!("Tabloid"), msg!("Executive"), msg!("A3"), msg!("A4"), msg!("A5")] {
            if ui.button(l.tr(p)).clicked() {
                act(app, "pageLayout.size", json!({"paper": p}));
            }
        }
    });
    let pa = big_button(ui, Icon::PrintArea, &l.text("ui-ribbon-label-print-area", &[]), &l.tr("Print Area"), true);
    egui::Popup::menu(&pa).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Set Print Area"), "pageLayout.printArea", json!({})),
                (msg!("Clear Print Area"), "pageLayout.printArea", json!({"clear": true})),
            ],
        );
    });
    let br = big_button(ui, Icon::PageBreak, &l.tr("Breaks"), &l.tr("Page Breaks"), true);
    egui::Popup::menu(&br).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Insert Page Break"), "pageLayout.breaks", json!({"insert": true})),
                (msg!("Remove Page Break"), "pageLayout.breaks", json!({"remove": true})),
                (msg!("Reset All Page Breaks"), "pageLayout.breaks", json!({"reset": true})),
            ],
        );
    });
    if big_button(ui, Icon::Sheet, &l.text("ui-ribbon-label-print-titles", &[]), &l.tr("Print Titles"), false).clicked() {
        app.open_dialog("pageSetup", json!({"tab": "Sheet"}));
    }
    sep(ui);
    let sh = app
        .session
        .active()
        .and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings, s.print.gridlines, s.print.headings)))
        .unwrap_or((true, true, false, false));
    // These two checkboxes mean the screen view vs. the printed page. English shows "View" (as in
    // Excel), but the key is distinct from the "View" tab so languages can word them apart.
    let screen = l.text("ui-ribbon-sheet-options-view", &[]);
    let print = l.tr("Print");
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(l.tr("Gridlines")).strong().small());
        let mut v = sh.0;
        if ui.checkbox(&mut v, &*screen).changed() {
            act(app, "view.gridlines", json!({"on": v}));
        }
        let mut p = sh.2;
        if ui.checkbox(&mut p, &*print).changed() {
            act(app, "pageLayout.printGridlines", json!({"on": p}));
        }
    });
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(l.tr("Headings")).strong().small());
        let mut v = sh.1;
        if ui.checkbox(&mut v, &*screen).changed() {
            act(app, "view.headings", json!({"on": v}));
        }
        let mut p = sh.3;
        if ui.checkbox(&mut p, &*print).changed() {
            act(app, "pageLayout.printHeadings", json!({"on": p}));
        }
    });
}

fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    if big_button(
        ui,
        Icon::Function,
        &l.text("ui-ribbon-label-insert-function", &[]),
        &tip(&l, app.keymap.platform(), "Insert Function", "formulas.insertFunction"),
        false,
    )
    .clicked()
    {
        app.open_dialog("insertFunction", json!({}));
    }
    let s = big_button(ui, Icon::Sum, &l.tr("AutoSum"), &l.tr("AutoSum"), true);
    if s.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    for (label, cat) in [
        (l.tr("Financial"), "Financial"),
        (l.tr("Logical"), "Logical"),
        (l.tr("Text"), "Text"),
        (l.text("ui-ribbon-label-date-time", &[]), "DateTime"),
        (l.text("ui-ribbon-label-lookup-reference", &[]), "Lookup"),
        (l.text("ui-ribbon-label-math-trig", &[]), "MathTrig"),
        (l.text("ui-ribbon-label-more-functions", &[]), "Statistical"),
    ] {
        let r = big_button(ui, Icon::Book, &label, &label, true);
        let formula_lang = app.session.locale().formula;
        egui::Popup::menu(&r).show(|ui| {
            ui.set_max_height(420.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in gridcraft_engine::cmd::formulas::function_list() {
                    if f["category"].as_str() == Some(cat)
                        && let Some(n) = f["name"].as_str()
                    {
                        let local = formula_lang.local_function(n).unwrap_or(n);
                        let desc = match l.function_description(n) {
                            Some(d) => d.into_owned(),
                            None => f["description"].as_str().unwrap_or("").to_string(),
                        };
                        if ui.button(local).on_hover_text(desc).clicked() {
                            // The engine starts the edit with the name in the formula language.
                            app.run_or_alert("formulas.insertFunction", json!({"name": n}));
                            ui.close();
                        }
                    }
                }
            });
        });
    }
    sep(ui);
    if big_button(ui, Icon::Name, &l.text("ui-ribbon-label-name-manager", &[]), &l.tr("Name Manager"), false).clicked() {
        app.open_dialog("nameManager", json!({}));
    }
    ui.vertical(|ui| {
        if small_button(ui, Icon::Name, &l.tr("Define Name"), &l.tr("Define Name"), false).clicked() {
            app.open_dialog("defineName", json!({}));
        }
        let use_in = small_button(ui, Icon::Function, &l.tr("Use in Formula"), &l.tr("Use in Formula"), true);
        egui::Popup::menu(&use_in).show(|ui| {
            let names: Vec<String> = app.session.active().map(|d| d.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
            for n in names {
                if ui.button(&n).clicked() {
                    let cur = app.editor.as_ref().map(|e| e.text.clone()).unwrap_or_else(|| "=".into());
                    app.begin_edit(Some(format!("{cur}{n}")), false);
                }
            }
        });
        if small_button(ui, Icon::Table, &l.tr("Create from Selection"), &l.tr("Create from Selection"), false).clicked() {
            act(app, "formulas.createFromSelection", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Trace, &l.tr("Trace Precedents"), &l.tr("Trace Precedents"), false).clicked()
            && let Ok(r) = app.run("formulas.tracePrecedents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Trace, &l.tr("Trace Dependents"), &l.tr("Trace Dependents"), false).clicked()
            && let Ok(r) = app.run("formulas.traceDependents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Close, &l.tr("Remove Arrows"), &l.tr("Remove Arrows"), false).clicked() {
            app.grid.trace = None;
        }
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Fx, &l.tr("Show Formulas"), &tip(&l, app.keymap.platform(), "Show Formulas", "formulas.showFormulas"), false)
            .clicked()
        {
            act(app, "formulas.showFormulas", json!({}));
        }
        if small_button(ui, Icon::Validation, &l.tr("Error Checking"), &l.tr("Error Checking"), false).clicked() {
            app.open_dialog("errorChecking", json!({}));
        }
        if small_button(ui, Icon::Calc, &l.tr("Evaluate Formula"), &l.tr("Evaluate Formula"), false).clicked() {
            app.open_dialog("evaluateFormula", json!({}));
        }
        if small_button(ui, Icon::Search, &l.tr("Watch Window"), &l.tr("Watch Window"), false).clicked() {
            app.grid.pane = Some("watch".into());
        }
    });
    sep(ui);
    let co = big_button(ui, Icon::Calc, &l.text("ui-ribbon-label-calculation-options", &[]), &l.tr("Calculation Options"), true);
    egui::Popup::menu(&co).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Automatic"), "formulas.calculationOptions", json!({"mode": "automatic"})),
                (msg!("Automatic Except for Data Tables"), "formulas.calculationOptions", json!({"mode": "automaticExceptTables"})),
                (msg!("Manual"), "formulas.calculationOptions", json!({"mode": "manual"})),
            ],
        );
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Calc, &l.tr("Calculate Now"), &tip(&l, app.keymap.platform(), "Calculate Now", "formulas.calculateNow"), false)
            .clicked()
        {
            act(app, "formulas.calculateNow", json!({}));
        }
        if small_button(
            ui,
            Icon::Sheet,
            &l.tr("Calculate Sheet"),
            &tip(&l, app.keymap.platform(), "Calculate Sheet", "formulas.calculateSheet"),
            false,
        )
        .clicked()
        {
            act(app, "formulas.calculateSheet", json!({}));
        }
    });
}

fn data(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let gd = big_button(ui, Icon::Folder, &l.text("ui-ribbon-label-get-data-text-csv", &[]), &l.tr("Import a text or CSV file"), false);
    if gd.clicked() {
        app.open_dialog("open", json!({}));
    }
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::SortAsc, "", &l.tr("Sort A to Z"), false).clicked() {
            act(app, "data.sortAscending", json!({}));
        }
        if small_button(ui, Icon::SortDesc, "", &l.tr("Sort Z to A"), false).clicked() {
            act(app, "data.sortDescending", json!({}));
        }
    });
    if big_button(ui, Icon::SortFilter, &l.tr("Sort"), &l.tr("Custom Sort"), false).clicked() {
        app.open_dialog("sort", json!({}));
    }
    if big_button(ui, Icon::Filter, &l.tr("Filter"), &tip(&l, app.keymap.platform(), "Filter", "data.filter"), false).clicked() {
        act(app, "data.filter", json!({}));
    }
    ui.vertical(|ui| {
        if small_button(ui, Icon::Eraser, &l.tr("Clear"), &l.tr("Clear Filter"), false).clicked() {
            act(app, "data.clearFilter", json!({}));
        }
        if small_button(ui, Icon::Redo, &l.tr("Reapply"), &l.tr("Reapply"), false).clicked() {
            act(app, "data.reapply", json!({}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::TextColumns, &l.text("ui-ribbon-label-text-to-columns", &[]), &l.tr("Text to Columns"), false).clicked() {
        app.open_dialog("textToColumns", json!({}));
    }
    if big_button(
        ui,
        Icon::FillDown,
        &l.text("ui-ribbon-label-flash-fill", &[]),
        &tip(&l, app.keymap.platform(), "Flash Fill", "edit.flashFill"),
        false,
    )
    .clicked()
    {
        act(app, "edit.flashFill", json!({}));
    }
    if big_button(ui, Icon::Duplicates, &l.text("ui-ribbon-label-remove-duplicates", &[]), &l.tr("Remove Duplicates"), false).clicked() {
        app.open_dialog("removeDuplicates", json!({}));
    }
    let dv = big_button(ui, Icon::Validation, &l.text("ui-ribbon-label-data-validation", &[]), &l.tr("Data Validation"), true);
    egui::Popup::menu(&dv).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Data Validation…"), "dialog:dataValidation", json!({})),
                (msg!("Circle Invalid Data"), "data.circleInvalid", json!({})),
                (msg!("Clear Validation"), "data.validation", json!({"clear": true})),
            ],
        );
    });
    sep(ui);
    let wi = big_button(ui, Icon::Calc, &l.text("ui-ribbon-label-what-if-analysis", &[]), &l.tr("What-If Analysis"), true);
    egui::Popup::menu(&wi).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Goal Seek…"), "dialog:goalSeek", json!({})),
                (msg!("Scenario Manager…"), "data.scenarioManager", json!({})),
                (msg!("Data Table…"), "data.dataTable", json!({})),
            ],
        );
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Group, &l.tr("Group"), &l.tr("Group"), false).clicked() {
            act(app, "data.group", json!({}));
        }
        if small_button(ui, Icon::Ungroup, &l.tr("Ungroup"), &l.tr("Ungroup"), false).clicked() {
            act(app, "data.ungroup", json!({}));
        }
        if small_button(ui, Icon::Subtotal, &l.tr("Subtotal"), &l.tr("Subtotal"), false).clicked() {
            app.open_dialog("subtotal", json!({}));
        }
    });
}

fn review(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    if big_button(ui, Icon::Spell, &l.tr("Spelling"), &tip(&l, app.keymap.platform(), "Spelling", "review.spelling"), false).clicked() {
        app.open_dialog("spelling", json!({}));
    }
    if big_button(ui, Icon::Calc, &l.text("ui-ribbon-label-workbook-statistics", &[]), &l.tr("Workbook Statistics"), false).clicked() {
        app.open_dialog("statistics", json!({}));
    }
    if big_button(ui, Icon::Validation, &l.text("ui-ribbon-label-check-accessibility", &[]), &l.tr("Check Accessibility"), false).clicked() {
        app.open_dialog("accessibility", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Comment, &l.text("ui-ribbon-label-new-comment", &[]), &l.tr("New Comment"), false).clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    if big_button(ui, Icon::Comment, &l.text("ui-ribbon-label-show-comments", &[]), &l.tr("Comments pane"), false).clicked() {
        app.grid.pane = Some("comments".into());
    }
    if big_button(ui, Icon::Delete, &l.tr("Delete"), &l.tr("Delete Comment"), false).clicked() {
        act(app, "review.deleteComment", json!({}));
    }
    let notes = big_button(ui, Icon::Note, &l.tr("Notes"), &l.tr("Notes"), true);
    egui::Popup::menu(&notes).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[(msg!("New Note"), "dialog:comment", json!({"threaded": false})), (msg!("Show/Hide Note"), "review.showNote", json!({}))],
        );
    });
    sep(ui);
    let protected = app.session.active().and_then(|d| d.wb.active().map(|s| s.is_protected())).unwrap_or(false);
    let protect_label = if protected { l.text("ui-ribbon-label-unprotect-sheet", &[]) } else { l.text("ui-ribbon-label-protect-sheet", &[]) };
    if big_button(ui, Icon::Lock, &protect_label, &l.tr("Protect Sheet"), false).clicked() {
        if protected {
            app.open_dialog("unprotectSheet", json!({}));
        } else {
            app.open_dialog("protectSheet", json!({}));
        }
    }
    if big_button(ui, Icon::Book, &l.text("ui-ribbon-label-protect-workbook", &[]), &l.tr("Protect Workbook structure"), false).clicked() {
        act(app, "review.protectWorkbook", json!({}));
    }
}

fn view(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let t = Tokens::get(ui.ctx());
    for (icon, label, cmd) in [
        (Icon::Normal, l.tr("Normal"), "view.normal"),
        (Icon::PageBreak, l.text("ui-ribbon-label-page-break-preview", &[]), "view.pageBreakPreview"),
        (Icon::PageLayout, l.text("ui-ribbon-label-page-layout", &[]), "view.pageLayout"),
    ] {
        if big_button(ui, icon, &label, &label, false).clicked() {
            act(app, cmd, json!({}));
        }
    }
    sep(ui);
    let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
    let lang = app.session.locale().ui;
    ui.vertical(|ui| {
        let mut fb = app.ui.formula_bar;
        if ui.checkbox(&mut fb, l.tr("Formula Bar")).changed() {
            app.ui.formula_bar = fb;
        }
        let mut g = s.0;
        if ui.checkbox(&mut g, l.tr("Gridlines")).changed() {
            act(app, "view.gridlines", json!({"on": g}));
        }
        let mut h = s.1;
        if ui.checkbox(&mut h, l.tr("Headings")).changed() {
            act(app, "view.headings", json!({"on": h}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::Zoom, &l.tr("Zoom"), &l.tr("Zoom…"), false).clicked() {
        app.open_dialog("zoom", json!({}));
    }
    if big_button(ui, Icon::Search, "100%", &l.tr("Zoom to 100%"), false).clicked() {
        act(app, "view.zoom", json!({"percent": 100}));
    }
    if big_button(ui, Icon::Zoom, &l.text("ui-ribbon-label-zoom-to-selection", &[]), &l.tr("Zoom to Selection"), false).clicked() {
        let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
        act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
    }
    sep(ui);
    let fp = big_button(ui, Icon::Freeze, &l.text("ui-ribbon-label-freeze-panes", &[]), &l.tr("Freeze Panes"), true);
    egui::Popup::menu(&fp).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Freeze Panes"), "view.freezePanes", json!({})),
                (msg!("Freeze Top Row"), "view.freezeTopRow", json!({})),
                (msg!("Freeze First Column"), "view.freezeFirstColumn", json!({})),
                (msg!("Unfreeze Panes"), "view.unfreezePanes", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        ui.label(l.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let choices = [("system", "ui-theme-system"), ("light", "ui-theme-light"), ("dark", "ui-theme-dark")];
        let label = choices.iter().find(|(value, _)| *value == mode).map(|(_, key)| l.text(key, &[]));
        egui::ComboBox::from_id_salt("display_theme").selected_text(label.unwrap_or_default()).width(88.0).show_ui(ui, |ui| {
            for (value, key) in choices {
                if ui.selectable_label(mode == value, l.text(key, &[])).clicked() {
                    act(app, "view.theme", json!({"mode": value}));
                }
            }
        });
    });
    sep(ui);
    // Interface language: settings live in the View tab, like Excel's Options. The label follows
    // the current language so it stays readable; the choices are always shown in their own script.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(l.tr("Interface language")).font(theme::ui_font(11.5)).color(t.text_dim));
        let picker = egui::ComboBox::from_id_salt("ui_language")
            .width(120.0)
            .selected_text(egui::RichText::new(lang.native_name).font(theme::ui_font(12.5)))
            .show_ui(ui, |ui| {
                for language in gridcraft_locale::LANGUAGES {
                    if ui.selectable_label(language.tag == lang.tag, language.native_name).clicked() {
                        act(app, "app.language.set", json!({"language": language.tag}));
                    }
                }
            });
        // The open list shows 日本語, 中文 and 한국어: like the Options dialog, it needs the CJK fonts.
        if picker.inner.is_some() {
            app.cjk_seen = true;
        }
    });
}

fn automate(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    if big_button(ui, Icon::Script, &l.text("ui-ribbon-label-command-palette", &[]), &l.tr("Search and run any command"), false).clicked() {
        app.open_dialog("commandSearch", json!({}));
    }
    if big_button(ui, Icon::Script, &l.text("ui-ribbon-label-agent-control", &[]), &l.tr("How agents drive GridCraft (MCP / control channel)"), false)
        .clicked()
    {
        app.open_dialog("agents", json!({}));
    }
    if big_button(ui, Icon::Script, &l.text("ui-ribbon-label-action-journal", &[]), &l.tr("Every command run in this session (replayable)"), false)
        .clicked()
    {
        app.open_dialog("journal", json!({}));
    }
    if big_button(
        ui,
        Icon::Script,
        &l.text("ui-ribbon-label-about-gridcraft", &[]),
        &l.tr("Version, contributors and the AI models that helped"),
        false,
    )
    .clicked()
    {
        app.open_dialog("about", json!({}));
    }
}

fn table_design(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let Some((tname, header, totals, banded, banded_c, first, last, filter)) = app.session.active().and_then(|d| {
        let sh = d.wb.active()?;
        let t = sh.table_at(d.selection.active)?;
        Some((t.name.clone(), t.header_row, t.totals_row, t.banded_rows, t.banded_cols, t.first_col, t.last_col, t.filter_button))
    }) else {
        return;
    };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(l.tr("Table Name:")).small());
        let mut name = tname.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
        if r.lost_focus() && name != tname {
            act(app, "table.rename", json!({"table": tname, "name": name}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::Duplicates, &l.text("ui-ribbon-label-remove-duplicates", &[]), &l.tr("Remove Duplicates"), false).clicked() {
        act(app, "data.removeDuplicates", json!({}));
    }
    if big_button(ui, Icon::Table, &l.text("ui-ribbon-label-convert-to-range", &[]), &l.tr("Convert to Range"), false).clicked() {
        act(app, "table.convertToRange", json!({"table": tname}));
    }
    sep(ui);
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            (msg!("Header Row"), "table.headerRow", header && filter),
            (msg!("Total Row"), "table.totalRow", totals),
            (msg!("Banded Rows"), "table.bandedRows", banded),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, l.tr(label)).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            (msg!("First Column"), "table.firstColumn", first),
            (msg!("Last Column"), "table.lastColumn", last),
            (msg!("Banded Columns"), "table.bandedColumns", banded_c),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, l.tr(label)).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    sep(ui);
    let st = big_button(ui, Icon::FormatTable, &l.text("ui-ribbon-label-table-styles", &[]), &l.tr("Table Styles"), true);
    egui::Popup::menu(&st).show(|ui| table_gallery(app, ui, "table.style"));
}

fn chart_design(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let Some(id) = app.selected_chart else { return };
    let ae = big_button(ui, Icon::Chart, &l.text("ui-ribbon-label-add-chart-element", &[]), &l.tr("Add Chart Element"), true);
    egui::Popup::menu(&ae).show(|ui| {
        menu_items(
            app,
            ui,
            l,
            &[
                (msg!("Legend: Bottom"), "chart.set", json!({"chart": id, "legend": "bottom"})),
                (msg!("Legend: Right"), "chart.set", json!({"chart": id, "legend": "right"})),
                (msg!("Legend: Top"), "chart.set", json!({"chart": id, "legend": "top"})),
                (msg!("Legend: None"), "chart.set", json!({"chart": id, "legend": "none"})),
                ("-", "", json!(null)),
                (msg!("Data Labels: Show"), "chart.set", json!({"chart": id, "dataLabels": true})),
                (msg!("Data Labels: None"), "chart.set", json!({"chart": id, "dataLabels": false})),
                (msg!("Gridlines: Show"), "chart.set", json!({"chart": id, "gridlines": true})),
                (msg!("Gridlines: None"), "chart.set", json!({"chart": id, "gridlines": false})),
            ],
        );
    });
    if big_button(ui, Icon::Redo, &l.text("ui-ribbon-label-switch-row-column", &[]), &l.tr("Switch Row/Column"), false).clicked() {
        act(app, "chart.switchRowColumn", json!({"chart": id}));
    }
    let ct = big_button(ui, Icon::ChartBar, &l.text("ui-ribbon-label-change-chart-type", &[]), &l.tr("Change Chart Type"), true);
    egui::Popup::menu(&ct).show(|ui| {
        for (label, k, s) in [
            (msg!("Clustered Column"), "column", ""),
            (msg!("Stacked Column"), "column", "stacked"),
            (msg!("Bar"), "bar", ""),
            (msg!("Line"), "line", "markers"),
            (msg!("Area"), "area", ""),
            (msg!("Pie"), "pie", ""),
            (msg!("Doughnut"), "doughnut", ""),
            (msg!("Scatter"), "scatter", ""),
            (msg!("Radar"), "radar", ""),
            (msg!("Waterfall"), "waterfall", ""),
            (msg!("Funnel"), "funnel", ""),
            (msg!("Treemap"), "treemap", ""),
        ] {
            if ui.button(l.tr(label)).clicked() {
                act(app, "chart.set", json!({"chart": id, "type": k, "subtype": s}));
            }
        }
    });
    if big_button(ui, Icon::Settings, &l.text("ui-ribbon-label-format-pane", &[]), &l.tr("Format Chart pane"), false).clicked() {
        app.grid.pane = Some("formatChart".into());
    }
    if big_button(ui, Icon::TextBox, &l.text("ui-ribbon-label-chart-title", &[]), &l.tr("Edit chart title"), false).clicked() {
        app.open_dialog("chartTitle", json!({"chart": id}));
    }
    if big_button(ui, Icon::Delete, &l.text("ui-ribbon-label-delete-chart", &[]), &l.tr("Delete chart"), false).clicked() {
        act(app, "chart.delete", json!({"chart": id}));
        app.selected_chart = None;
    }
}
