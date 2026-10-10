//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, color_palette, icon_button, small_button, split_button, toggle_button};

pub const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];

/// Leave room for the macOS traffic lights when the content extends into the title bar.
pub const TITLE_LEFT_PAD: f32 = if cfg!(target_os = "macos") { 76.0 } else { 8.0 };

pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
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
            ui.label(egui::RichText::new("AutoSave").font(theme::ui_font(12.5)).color(t.text_dim));
            let on = app.grid.autosave();
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
            ui.painter().rect_filled(r, 8.0, if on { t.accent } else { Color32::TRANSPARENT });
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, if on { t.accent } else { t.text_dim }), StrokeKind::Inside);
            let knob = if on { pos2(r.right() - 8.0, r.center().y) } else { pos2(r.left() + 8.0, r.center().y) };
            ui.painter().circle_filled(knob, 5.0, if on { Color32::WHITE } else { t.text_dim });
            if resp.on_hover_text("AutoSave: save after every change (workbooks saved to a file)").clicked() {
                app.grid.toggle_autosave();
            }
            ui.add_space(6.0);
            if icon_button(ui, Icon::Home, t.text_dim, "Home", vec2(26.0, 26.0)).clicked() {
                app.open_dialog("start", json!({}));
            }
            if icon_button(ui, Icon::Save, t.text_dim, "Save (⌘S)", vec2(26.0, 26.0)).clicked() {
                app.run_or_alert("file.save", json!({}));
            }
            let can_undo = app.session.active().is_some_and(|d| !d.undo.is_empty());
            let can_redo = app.session.active().is_some_and(|d| !d.redo.is_empty());
            if icon_button(ui, Icon::Undo, if can_undo { t.text_dim } else { t.text_disabled }, "Undo (⌘Z)", vec2(26.0, 26.0)).clicked() && can_undo
            {
                app.run_or_alert("edit.undo", json!({}));
            }
            let undo_list = icon_button(ui, Icon::Chevron, t.text_dim, "Undo list", vec2(14.0, 26.0));
            egui::Popup::menu(&undo_list).show(|ui| {
                let labels: Vec<String> =
                    app.session.active().map(|d| d.undo.iter().rev().take(20).map(|e| e.label.clone()).collect()).unwrap_or_default();
                if labels.is_empty() {
                    ui.label("Can't Undo");
                }
                for (i, l) in labels.iter().enumerate() {
                    if ui.button(format!("Undo {l}")).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
            if icon_button(ui, Icon::Redo, if can_redo { t.text_dim } else { t.text_disabled }, "Redo (⌘Y)", vec2(26.0, 26.0)).clicked() && can_redo
            {
                app.run_or_alert("edit.redo", json!({}));
            }
            let more = icon_button(ui, Icon::More, t.text_dim, "More commands", vec2(26.0, 26.0));
            egui::Popup::menu(&more).show(|ui| {
                for (label, id) in [
                    ("New Workbook", "file.new"),
                    ("Open…", "file.open"),
                    ("Save As…", "file.saveAs"),
                    ("Print…", "file.print"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Calculate Now", "formulas.calculateNow"),
                ] {
                    if ui.button(label).clicked() {
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
                if icon_button(ui, Icon::Search, t.text_dim, "Search commands (⌘⇧Space)", vec2(28.0, 26.0)).clicked() {
                    app.open_dialog("commandSearch", json!({}));
                }
                let share = small_button(ui, Icon::Share, "Share", "Share: export or save a copy", true);
                egui::Popup::menu(&share).show(|ui| {
                    for (label, fmt) in
                        [("Save a Copy as Excel Workbook (.xlsx)…", "xlsx"), ("Export as CSV…", "csv"), ("Export as Web Page (.html)…", "html")]
                    {
                        if ui.button(label).clicked() {
                            app.open_dialog("saveCopy", json!({"format": fmt}));
                        }
                    }
                });
                if icon_button(ui, Icon::Comment, t.text_dim, "Comments", vec2(28.0, 26.0)).clicked() {
                    app.open_dialog("comments", json!({}));
                }
            });
        });
    });
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
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
    let label = egui::RichText::new("File").font(theme::ui_font(13.5)).color(t.accent);
    let file = ui.add_sized(vec2(46.0, 30.0), egui::Button::new(label).frame(false));
    egui::Popup::menu(&file).show(|ui| {
        for (label, id, needs_document) in [
            ("New Workbook", "file.new", false),
            ("Open…", "file.open", false),
            ("Save", "file.save", true),
            ("Save As…", "file.saveAs", true),
            ("Close", "file.close", true),
        ] {
            if id == "file.save" || id == "file.close" {
                ui.separator();
            }
            if ui.add_enabled(!needs_document || app.session.active().is_some(), egui::Button::new(label)).clicked() {
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

fn menu_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            ui.separator();
            continue;
        }
        let lang = app.ui.language;
        let response = ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(220.0, 22.0)));
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
    let t = Tokens::get(ui.ctx());
    let st = active_style(app);
    // Clipboard
    let paste = big_button(ui, Icon::Paste, "Paste", "Paste (⌘V)", true);
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
            &[
                ("Paste", "edit.paste", json!({})),
                ("Paste Values", "edit.pasteSpecial", json!({"what": "values"})),
                ("Paste Formulas", "edit.pasteSpecial", json!({"what": "formulas"})),
                ("Paste Formatting", "edit.pasteSpecial", json!({"what": "formats"})),
                ("Transpose", "edit.pasteSpecial", json!({"what": "all", "transpose": true})),
                ("Paste Link", "edit.pasteSpecial", json!({"what": "all", "link": true})),
                ("-", "", json!(null)),
                ("Paste Special…", "dialog:pasteSpecial", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        if icon_button(ui, Icon::Cut, t.text, "Cut (⌘X)", vec2(26.0, 23.0)).clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.cut");
        }
        if icon_button(ui, Icon::Copy, t.text, "Copy (⌘C)", vec2(26.0, 23.0)).clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.copy");
        }
        let fp_on = app.session.format_painter.is_some();
        let fp = toggle_button(ui, Icon::Brush, fp_on, "Format Painter (double-click to keep it on)");
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
            if icon_button(ui, Icon::Plus, t.text, "Increase Font Size", vec2(22.0, 23.0)).clicked() {
                act(app, "home.increaseFontSize", json!({}));
            }
            if icon_button(ui, Icon::Minus, t.text, "Decrease Font Size", vec2(22.0, 23.0)).clicked() {
                act(app, "home.decreaseFontSize", json!({}));
            }
        });
        ui.horizontal(|ui| {
            if toggle_button(ui, Icon::Bold, st.font.bold, "Bold (⌘B)").clicked() {
                act(app, "home.bold", json!({}));
            }
            if toggle_button(ui, Icon::Italic, st.font.italic, "Italic (⌘I)").clicked() {
                act(app, "home.italic", json!({}));
            }
            let (u, ua) = split_button(ui, Icon::Underline, None, "Underline (⌘U)");
            if u {
                act(app, "home.underline", json!({}));
            }
            egui::Popup::menu(&ua).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Underline", "home.underline", json!({"style": "single"})),
                        ("Double Underline", "home.underline", json!({"style": "double"})),
                    ],
                );
            });
            if toggle_button(ui, Icon::Strike, st.font.strike, "Strikethrough").clicked() {
                act(app, "home.strikethrough", json!({}));
            }
            let (b, ba) = split_button(ui, Icon::Borders, None, "Borders");
            if b {
                act(app, "home.borders", json!({"preset": app.grid.last_border.clone()}));
            }
            ba.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Border presets"));
            egui::Popup::menu(&ba).show(|ui| {
                for (label, preset) in [
                    ("Bottom Border", "bottom"),
                    ("Top Border", "top"),
                    ("Left Border", "left"),
                    ("Right Border", "right"),
                    ("No Border", "none"),
                    ("All Borders", "all"),
                    ("Outside Borders", "outside"),
                    ("Thick Outside Borders", "thickOutside"),
                    ("Bottom Double Border", "doubleBottom"),
                    ("Thick Bottom Border", "thickBottom"),
                    ("Top and Bottom Border", "topBottom"),
                    ("Top and Thick Bottom Border", "topThickBottom"),
                    ("Top and Double Bottom Border", "topDoubleBottom"),
                    ("Inside Borders", "inside"),
                ] {
                    if crate::border_preview::preset_button(ui, label, preset).clicked() {
                        app.grid.last_border = preset.to_string();
                        act(app, "home.borders", json!({"preset": preset}));
                    }
                }
                ui.separator();
                if ui.button("More Borders…").clicked() {
                    app.open_dialog("formatCells", json!({"tab": "Border"}));
                }
            });
            let fill = app.grid.last_fill.clone();
            let fill_c = gridcraft_engine::model::Color::from_hex(&fill).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (f, fa) = split_button(ui, Icon::Fill, fill_c, "Fill Color");
            if f {
                act(app, "home.fillColor", json!({"color": fill}));
            }
            egui::Popup::menu(&fa).show(|ui| {
                if let Some(c) = color_palette(ui, &theme_colors(app), "No Fill") {
                    if c != "none" {
                        app.grid.last_fill = c.clone();
                    }
                    act(app, "home.fillColor", json!({"color": c}));
                    ui.close();
                }
            });
            let fc = app.grid.last_font_color.clone();
            let fc_c = gridcraft_engine::model::Color::from_hex(&fc).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (c, ca) = split_button(ui, Icon::FontColor, fc_c, "Font Color");
            if c {
                act(app, "home.fontColor", json!({"color": fc}));
            }
            egui::Popup::menu(&ca).show(|ui| {
                if let Some(c) = color_palette(ui, &theme_colors(app), "Automatic") {
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
            for (icon, v, id, tip) in [
                (Icon::AlignTop, VAlign::Top, "home.alignTop", "Top Align"),
                (Icon::AlignMiddle, VAlign::Center, "home.alignMiddle", "Middle Align"),
                (Icon::AlignBottom, VAlign::Bottom, "home.alignBottom", "Bottom Align"),
            ] {
                if toggle_button(ui, icon, st.align.v == v, tip).clicked() {
                    act(app, id, json!({}));
                }
            }
            let o = small_button(ui, Icon::Orientation, "", "Orientation", true);
            egui::Popup::menu(&o).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Angle Counterclockwise", "home.orientation", json!({"angle": "counterclockwise"})),
                        ("Angle Clockwise", "home.orientation", json!({"angle": "clockwise"})),
                        ("Vertical Text", "home.orientation", json!({"angle": "vertical"})),
                        ("Rotate Text Up", "home.orientation", json!({"angle": "up"})),
                        ("Rotate Text Down", "home.orientation", json!({"angle": "down"})),
                        ("Horizontal", "home.orientation", json!({"angle": 0})),
                        ("-", "", json!(null)),
                        ("Format Cell Alignment…", "dialog:formatCells", json!({"tab": "Alignment"})),
                    ],
                );
            });
            let wrap = toggle_button(ui, Icon::Wrap, st.align.wrap, "Wrap Text");
            if wrap.clicked() {
                act(app, "home.wrapText", json!({}));
            }
            ui.label(egui::RichText::new(app.ui.language.tr("Wrap Text")).font(theme::ui_font(12.5)));
        });
        ui.horizontal(|ui| {
            for (icon, h, id, tip) in [
                (Icon::AlignLeft, HAlign::Left, "home.alignLeft", "Align Left"),
                (Icon::AlignCenter, HAlign::Center, "home.alignCenter", "Center"),
                (Icon::AlignRight, HAlign::Right, "home.alignRight", "Align Right"),
            ] {
                let response = toggle_button(ui, icon, st.align.h == h, tip);
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
            if icon_button(ui, Icon::IndentDec, t.text, "Decrease Indent", vec2(24.0, 23.0)).clicked() {
                act(app, "home.decreaseIndent", json!({}));
            }
            if icon_button(ui, Icon::IndentInc, t.text, "Increase Indent", vec2(24.0, 23.0)).clicked() {
                act(app, "home.increaseIndent", json!({}));
            }
            let (m, ma) = split_button(ui, Icon::Merge, None, "Merge & Center");
            if m {
                act(app, "home.mergeCenter", json!({}));
            }
            ui.label(egui::RichText::new(app.ui.language.tr("Merge & Center")).font(theme::ui_font(12.5)));
            egui::Popup::menu(&ma).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Merge & Center", "home.mergeCenter", json!({})),
                        ("Merge Across", "home.mergeAcross", json!({})),
                        ("Merge Cells", "home.mergeCells", json!({})),
                        ("Unmerge Cells", "home.unmergeCells", json!({})),
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
            let (c, ca) = split_button(ui, Icon::Currency, None, "Accounting Number Format");
            if c {
                act(app, "home.accounting", json!({}));
            }
            egui::Popup::menu(&ca).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        (
                            "$ English (United States)",
                            "home.numberFormat",
                            json!({"code": "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"}),
                        ),
                        (
                            "£ English (United Kingdom)",
                            "home.numberFormat",
                            json!({"code": "_-[$£-809]* #,##0.00_-;-[$£-809]* #,##0.00_-;_-[$£-809]* \"-\"??_-;_-@_-"}),
                        ),
                        (
                            "€ Euro",
                            "home.numberFormat",
                            json!({"code": "_-[$€-x-euro2] * #,##0.00_-;-[$€-x-euro2] * #,##0.00_-;_-[$€-x-euro2] * \"-\"??_-;_-@_-"}),
                        ),
                        ("¥ Japanese", "home.numberFormat", json!({"code": "_-[$¥-411]* #,##0_-;-[$¥-411]* #,##0_-;_-[$¥-411]* \"-\"_-;_-@_-"})),
                        ("-", "", json!(null)),
                        ("More Accounting Formats…", "dialog:formatCells", json!({"tab": "Number"})),
                    ],
                );
            });
            if icon_button(ui, Icon::Percent, t.text, "Percent Style", vec2(24.0, 23.0)).clicked() {
                act(app, "home.percent", json!({}));
            }
            if icon_button(ui, Icon::Comma, t.text, "Comma Style", vec2(24.0, 23.0)).clicked() {
                act(app, "home.comma", json!({}));
            }
            if icon_button(ui, Icon::DecInc, t.text, "Increase Decimal", vec2(26.0, 23.0)).clicked() {
                act(app, "home.increaseDecimal", json!({}));
            }
            if icon_button(ui, Icon::DecDec, t.text, "Decrease Decimal", vec2(26.0, 23.0)).clicked() {
                act(app, "home.decreaseDecimal", json!({}));
            }
        });
    });
    sep(ui);
    // Styles
    let cf = big_button(ui, Icon::CondFormat, "Conditional\nFormatting", "Conditional Formatting", true);
    egui::Popup::menu(&cf).show(|ui| cf_menu(app, ui));
    let ft = big_button(ui, Icon::FormatTable, "Format\nas Table", "Format as Table", true);
    egui::Popup::menu(&ft).show(|ui| table_gallery(app, ui, "home.formatAsTable"));
    let cs = big_button(ui, Icon::CellStyles, "Cell\nStyles", "Cell Styles", true);
    egui::Popup::menu(&cs).show(|ui| cell_style_gallery(app, ui));
    sep(ui);
    // Cells
    ui.vertical(|ui| {
        let ins = small_button(ui, Icon::Insert, "Insert", "Insert", true);
        egui::Popup::menu(&ins).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Insert Cells…", "dialog:insertCells", json!({})),
                    ("Insert Sheet Rows", "home.insertRows", json!({})),
                    ("Insert Sheet Columns", "home.insertColumns", json!({})),
                    ("Insert Sheet", "home.insertSheet", json!({})),
                ],
            );
        });
        let del = small_button(ui, Icon::Delete, "Delete", "Delete", true);
        egui::Popup::menu(&del).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Delete Cells…", "dialog:deleteCells", json!({})),
                    ("Delete Sheet Rows", "home.deleteRows", json!({})),
                    ("Delete Sheet Columns", "home.deleteColumns", json!({})),
                    ("Delete Sheet", "home.deleteSheet", json!({})),
                ],
            );
        });
        let fmt = small_button(ui, Icon::Format, "Format", "Format", true);
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
                &[
                    ("Row Height…", "dialog:rowHeight", json!({})),
                    ("AutoFit Row Height", "home.autofitRowHeight", json!({})),
                    ("Column Width…", "dialog:columnWidth", json!({})),
                    ("AutoFit Column Width", "home.autofitColumnWidth", json!({})),
                    ("Default Width…", "dialog:defaultWidth", json!({})),
                    ("-", "", json!(null)),
                    ("Hide Rows", "home.hideRows", json!({})),
                    ("Hide Columns", "home.hideColumns", json!({})),
                    ("Unhide Rows", "home.unhideRows", json!({})),
                    ("Unhide Columns", "home.unhideColumns", json!({})),
                    ("Hide Sheet", "sheet.hide", json!({})),
                    ("Unhide Sheet…", "sheet.unhide", json!({})),
                    ("-", "", json!(null)),
                    ("Rename Sheet", "dialog:renameSheet", json!({})),
                    ("Move or Copy Sheet…", "dialog:moveSheet", json!({})),
                    ("-", "", json!(null)),
                    ("Protect Sheet…", "dialog:protectSheet", json!({})),
                    ("Lock Cell", "home.lockCell", json!({})),
                    ("Format Cells…", "dialog:formatCells", json!({})),
                ],
            );
        });
    });
    sep(ui);
    // Editing
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let (s, sa) = split_button(ui, Icon::Sum, None, "AutoSum (⌘⇧T)");
            if s {
                act(app, "formulas.autoSum", json!({}));
            }
            egui::Popup::menu(&sa).show(|ui| {
                for f in ["SUM", "AVERAGE", "COUNT", "MAX", "MIN"] {
                    if ui.button(f).clicked() {
                        act(app, "formulas.autoSum", json!({"function": f}));
                    }
                }
                ui.separator();
                if ui.button("More Functions…").clicked() {
                    app.open_dialog("insertFunction", json!({}));
                }
            });
        });
        let fill = small_button(ui, Icon::FillDown, "", "Fill", true);
        egui::Popup::menu(&fill).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Down", "edit.fillDown", json!({})),
                    ("Right", "edit.fillRight", json!({})),
                    ("Up", "edit.fillUp", json!({})),
                    ("Left", "edit.fillLeft", json!({})),
                    ("Series…", "dialog:series", json!({})),
                    ("Flash Fill", "edit.flashFill", json!({})),
                ],
            );
        });
        let clear = small_button(ui, Icon::Eraser, "", "Clear", true);
        egui::Popup::menu(&clear).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Clear All", "edit.clearAll", json!({})),
                    ("Clear Formats", "edit.clearFormats", json!({})),
                    ("Clear Contents", "edit.clearContents", json!({})),
                    ("Clear Comments and Notes", "edit.clearComments", json!({})),
                    ("Clear Hyperlinks", "edit.clearHyperlinks", json!({})),
                ],
            );
        });
    });
    let sf = big_button(ui, Icon::SortFilter, "Sort &\nFilter", "Sort & Filter", true);
    egui::Popup::menu(&sf).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Sort A to Z", "data.sortAscending", json!({})),
                ("Sort Z to A", "data.sortDescending", json!({})),
                ("Custom Sort…", "dialog:sort", json!({})),
                ("-", "", json!(null)),
                ("Filter", "data.filter", json!({})),
                ("Clear", "data.clearFilter", json!({})),
                ("Reapply", "data.reapply", json!({})),
            ],
        );
    });
    let fs = big_button(ui, Icon::Find, "Find &\nSelect", "Find & Select", true);
    egui::Popup::menu(&fs).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Find…", "dialog:find", json!({})),
                ("Replace…", "dialog:find", json!({"replace": true})),
                ("Go To…", "dialog:goTo", json!({})),
                ("Go To Special…", "dialog:goToSpecial", json!({})),
                ("-", "", json!(null)),
                ("Formulas", "edit.goToSpecial", json!({"kind": "formulas"})),
                ("Notes", "edit.goToSpecial", json!({"kind": "notes"})),
                ("Conditional Formatting", "edit.goToSpecial", json!({"kind": "conditionalFormats"})),
                ("Constants", "edit.goToSpecial", json!({"kind": "constants"})),
                ("Data Validation", "edit.goToSpecial", json!({"kind": "dataValidation"})),
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
    let label = if size.fract() == 0.0 { format!("{}", size as i32) } else { format!("{size}") };
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
    let code = st.num_fmt.as_str().to_string();
    let kind = gridcraft_engine::display::number_format(&code).kind();
    let current = format!("{kind:?}");
    let lang = app.ui.language;
    let sample = app.session.active().and_then(|d| d.wb.active().map(|sh| sh.value(d.selection.active))).unwrap_or_default();
    let mut picked: Option<&str> = None;
    egui::ComboBox::from_id_salt("number_format")
        .width(150.0)
        .selected_text(egui::RichText::new(lang.tr(&current)).font(theme::ui_font(12.5)))
        .show_ui(ui, |ui| {
            for name in
                ["General", "Number", "Currency", "Accounting", "Short Date", "Long Date", "Time", "Percentage", "Fraction", "Scientific", "Text"]
            {
                let code = gridcraft_engine::cmd::format::format_code_for(name);
                let preview = app.session.active().map(|d| gridcraft_engine::display::format(&sample, code, &d.wb).text).unwrap_or_default();
                let r = ui.add(egui::Button::selectable(current == name, format!("{:<12}   {preview}", lang.tr(name))).min_size(vec2(240.0, 22.0)));
                if r.clicked() {
                    picked = Some(name);
                }
            }
            ui.separator();
            if ui.button(lang.tr("More Number Formats…")).clicked() {
                picked = Some("__more");
            }
        });
    match picked {
        Some("__more") => app.open_dialog("formatCells", json!({"tab": "Number"})),
        Some(n) => act(app, "home.numberFormat", json!({"format": n})),
        None => {}
    }
}

fn cf_menu(app: &mut SheetApp, ui: &mut Ui) {
    ui.menu_button("Highlight Cells Rules", |ui| {
        for (label, op) in [("Greater Than…", "greater"), ("Less Than…", "less"), ("Between…", "between"), ("Equal To…", "equal")] {
            if ui.button(label).clicked() {
                app.open_dialog("cfQuick", json!({"type": "cellIs", "operator": op, "title": label}));
            }
        }
        if ui.button("Text that Contains…").clicked() {
            app.open_dialog("cfQuick", json!({"type": "containsText", "title": "Text that Contains"}));
        }
        if ui.button("A Date Occurring…").clicked() {
            app.open_dialog("cfQuick", json!({"type": "timePeriod", "title": "A Date Occurring"}));
        }
        if ui.button("Duplicate Values…").clicked() {
            act(app, "home.conditionalFormat", json!({"rule": {"type": "duplicate"}}));
        }
    });
    ui.menu_button("Top/Bottom Rules", |ui| {
        menu_items(
            app,
            ui,
            &[
                ("Top 10 Items", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10}})),
                ("Top 10%", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "percent": true}})),
                ("Bottom 10 Items", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true}})),
                ("Bottom 10%", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true, "percent": true}})),
                ("Above Average", "home.conditionalFormat", json!({"rule": {"type": "aboveAverage"}})),
                ("Below Average", "home.conditionalFormat", json!({"rule": {"type": "aboveAverage", "below": true}})),
            ],
        );
    });
    ui.menu_button("Data Bars", |ui| {
        for (label, c) in
            [("Blue", "#638EC6"), ("Green", "#63BE7B"), ("Red", "#F8696B"), ("Orange", "#FFB628"), ("Light Blue", "#008AEF"), ("Purple", "#D6007B")]
        {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "dataBar", "color": c}}));
            }
        }
    });
    ui.menu_button("Color Scales", |ui| {
        for (label, cols) in [
            ("Green - Yellow - Red", vec!["#63BE7B", "#FFEB84", "#F8696B"]),
            ("Red - Yellow - Green", vec!["#F8696B", "#FFEB84", "#63BE7B"]),
            ("Green - White - Red", vec!["#63BE7B", "#FCFCFF", "#F8696B"]),
            ("Blue - White - Red", vec!["#5A8AC6", "#FCFCFF", "#F8696B"]),
            ("White - Green", vec!["#FCFCFF", "#63BE7B"]),
            ("White - Red", vec!["#FCFCFF", "#F8696B"]),
        ] {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "colorScale", "colors": cols}}));
            }
        }
    });
    ui.menu_button("Icon Sets", |ui| {
        for (label, set) in [
            ("3 Arrows", "3Arrows"),
            ("3 Traffic Lights", "3TrafficLights1"),
            ("3 Symbols", "3Symbols"),
            ("4 Ratings", "4Rating"),
            ("5 Arrows", "5Arrows"),
            ("5 Quarters", "5Quarters"),
        ] {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "iconSet", "set": set}}));
            }
        }
    });
    ui.separator();
    if ui.button("New Rule…").clicked() {
        app.open_dialog("cfQuick", json!({"type": "expression", "title": "New Formatting Rule"}));
    }
    ui.menu_button("Clear Rules", |ui| {
        menu_items(
            app,
            ui,
            &[
                ("Clear Rules from Selected Cells", "home.clearRules", json!({})),
                ("Clear Rules from Entire Sheet", "home.clearRules", json!({"sheet": true})),
            ],
        );
    });
    if ui.button("Manage Rules…").clicked() {
        app.open_dialog("manageRules", json!({}));
    }
}

/// A gallery of table styles drawn as mini tables.
fn table_gallery(app: &mut SheetApp, ui: &mut Ui, cmd: &str) {
    ui.set_width(430.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    for (fam, n) in [("Light", 21u32), ("Medium", 28), ("Dark", 11)] {
        ui.label(egui::RichText::new(fam).strong());
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

fn cell_style_gallery(app: &mut SheetApp, ui: &mut Ui) {
    ui.set_width(520.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let groups: &[(&str, &[&str])] = &[
        ("Good, Bad and Neutral", &["Normal", "Bad", "Good", "Neutral"]),
        ("Data and Model", &["Calculation", "Check Cell", "Explanatory Text", "Input", "Linked Cell", "Note", "Output", "Warning Text"]),
        ("Titles and Headings", &["Heading 1", "Heading 2", "Heading 3", "Heading 4", "Title", "Total"]),
        (
            "Themed Cell Styles",
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
        ("Number Format", &["Comma", "Comma [0]", "Currency", "Currency [0]", "Percent"]),
    ];
    for (g, names) in groups {
        ui.label(egui::RichText::new(*g).strong());
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
                    *n,
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

fn insert(app: &mut SheetApp, ui: &mut Ui) {
    if big_button(ui, Icon::PivotTable, "PivotTable", "PivotTable", false).clicked() {
        act(app, "insert.pivotTable", json!({}));
    }
    if big_button(ui, Icon::Table, "Table", "Table (⌘T)", false).clicked() {
        act(app, "insert.table", json!({}));
    }
    sep(ui);
    let pic = big_button(ui, Icon::Picture, "Pictures", "Insert a picture", true);
    pic.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Pictures"));
    egui::Popup::menu(&pic).show(|ui| {
        for (label, placement) in [("Place in Cell", "cell"), ("Place over Cells", "overCells")] {
            if ui.button(label).clicked() {
                app.open_dialog("insertPicture", json!({"placement": placement}));
                ui.close();
            }
        }
    });
    let shapes = big_button(ui, Icon::Shapes, "Shapes", "Shapes", true);
    egui::Popup::menu(&shapes).show(|ui| {
        for (label, kind) in [
            ("Rectangle", "rectangle"),
            ("Rounded Rectangle", "roundedRectangle"),
            ("Oval", "ellipse"),
            ("Triangle", "triangle"),
            ("Line", "line"),
            ("Arrow", "arrow"),
            ("Text Box", "textBox"),
        ] {
            if ui.button(label).clicked() {
                act(app, "insert.shape", json!({"kind": kind}));
            }
        }
    });
    let icn = big_button(ui, Icon::Theme, "Icons", "Insert an icon", true);
    egui::Popup::menu(&icn).show(|ui| {
        ui.set_width(260.0);
        egui::Grid::new("icon_lib").show(ui, |ui| {
            for (i, (name, icon)) in icons::LIBRARY.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, Tokens::get(ui.ctx()).hover);
                }
                icons::paint(ui.painter(), r.shrink(6.0), *icon, Tokens::get(ui.ctx()).text);
                if resp.on_hover_text(*name).clicked() {
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
    if big_button(ui, Icon::Chart, "Recommended\nCharts", "Recommended Charts", false).clicked() {
        act(app, "insert.recommendedCharts", json!({}));
    }
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, tip, kind, subs) in [
                (
                    Icon::ChartBar,
                    "Column or Bar",
                    "column",
                    vec![
                        ("Clustered Column", "column", ""),
                        ("Stacked Column", "column", "stacked"),
                        ("100% Stacked Column", "column", "stacked100"),
                        ("Clustered Bar", "bar", ""),
                        ("Stacked Bar", "bar", "stacked"),
                    ],
                ),
                (
                    Icon::ChartLine,
                    "Line or Area",
                    "line",
                    vec![
                        ("Line", "line", ""),
                        ("Line with Markers", "line", "markers"),
                        ("Stacked Line", "line", "stacked"),
                        ("Area", "area", ""),
                        ("Stacked Area", "area", "stacked"),
                    ],
                ),
                (Icon::ChartPie, "Pie or Doughnut", "pie", vec![("Pie", "pie", ""), ("Doughnut", "doughnut", "")]),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                let _ = kind;
                egui::Popup::menu(&r).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            for (icon, tip, subs) in [
                (
                    Icon::ChartScatter,
                    "Scatter or Bubble",
                    vec![("Scatter", "scatter", ""), ("Scatter with Lines", "scatter", "lines"), ("Bubble", "bubble", "")],
                ),
                (Icon::Chart, "Statistic", vec![("Histogram", "histogram", ""), ("Box & Whisker", "boxWhisker", "")]),
                (
                    Icon::Theme,
                    "Hierarchy & more",
                    vec![
                        ("Treemap", "treemap", ""),
                        ("Sunburst", "sunburst", ""),
                        ("Waterfall", "waterfall", ""),
                        ("Funnel", "funnel", ""),
                        ("Radar", "radar", ""),
                        ("Stock", "stock", ""),
                        ("Combo", "combo", ""),
                    ],
                ),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
    });
    sep(ui);
    let sp = big_button(ui, Icon::Sparkline, "Sparklines", "Sparklines", true);
    egui::Popup::menu(&sp).show(|ui| {
        for (label, kind) in [("Line", "line"), ("Column", "column"), ("Win/Loss", "winLoss")] {
            if ui.button(label).clicked() {
                app.open_dialog("sparkline", json!({"type": kind}));
            }
        }
    });
    sep(ui);
    if big_button(ui, Icon::Link, "Link", "Insert Link (⌘K)", false).clicked() {
        app.open_dialog("insertLink", json!({}));
    }
    if big_button(ui, Icon::Comment, "Comment", "New Comment", false).clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    sep(ui);
    if big_button(ui, Icon::TextBox, "Text\nBox", "Text Box", false).clicked() {
        act(app, "insert.textBox", json!({}));
    }
    if big_button(ui, Icon::PageLayout, "Header &\nFooter", "Header & Footer", false).clicked() {
        app.open_dialog("headerFooter", json!({}));
    }
    let sym = big_button(ui, Icon::Symbol, "Symbol", "Symbol", true);
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
    let tool = app.session.draw_tool.clone();
    if big_button(ui, Icon::Shapes, "Select\nObjects", "Select and move ink and shapes", false).clicked() {
        act(app, "draw.lasso", json!({}));
    }
    sep(ui);
    for (label, color) in [("Pen\nBlue", "#1F5FC9"), ("Pen\nBlack", "#000000"), ("Pen\nRed", "#D13B2F"), ("Highlighter", "#F2E33A")] {
        let on = tool == "pen" && app.session.draw_color.eq_ignore_ascii_case(color);
        let r = big_button(ui, Icon::Pen, label, "Draw with ink", false);
        if on {
            ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
        }
        if r.clicked() {
            let width = if label == "Highlighter" { 12.0 } else { 2.0 };
            act(app, "draw.pen", json!({"on": !on, "color": color, "width": width}));
        }
    }
    let er = big_button(ui, Icon::Eraser, "Eraser", "Erase ink strokes", false);
    if tool == "eraser" {
        ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if er.clicked() {
        act(app, "draw.eraser", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false).clicked() {
        act(app, "draw.inkToShape", json!({}));
    }
}

fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    let th = big_button(ui, Icon::Theme, "Themes", "Themes", true);
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
    let m = big_button(ui, Icon::Margins, "Margins", "Margins", true);
    egui::Popup::menu(&m).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Normal", "pageLayout.margins", json!({"preset": "normal"})),
                ("Wide", "pageLayout.margins", json!({"preset": "wide"})),
                ("Narrow", "pageLayout.margins", json!({"preset": "narrow"})),
                ("Custom Margins…", "dialog:pageSetup", json!({})),
            ],
        );
    });
    let o = big_button(ui, Icon::Orient, "Page Layout|Orientation", "Page Layout|Orientation", true);
    egui::Popup::menu(&o).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Portrait", "pageLayout.orientation", json!({"orientation": "portrait"})),
                ("Landscape", "pageLayout.orientation", json!({"orientation": "landscape"})),
            ],
        );
    });
    let s = big_button(ui, Icon::Paper, "Size", "Paper Size", true);
    egui::Popup::menu(&s).show(|ui| {
        for p in ["Letter", "Legal", "Tabloid", "Executive", "A3", "A4", "A5"] {
            if ui.button(p).clicked() {
                act(app, "pageLayout.size", json!({"paper": p}));
            }
        }
    });
    let pa = big_button(ui, Icon::PrintArea, "Print\nArea", "Print Area", true);
    egui::Popup::menu(&pa).show(|ui| {
        menu_items(
            app,
            ui,
            &[("Set Print Area", "pageLayout.printArea", json!({})), ("Clear Print Area", "pageLayout.printArea", json!({"clear": true}))],
        );
    });
    let br = big_button(ui, Icon::PageBreak, "Breaks", "Page Breaks", true);
    egui::Popup::menu(&br).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Insert Page Break", "pageLayout.breaks", json!({"insert": true})),
                ("Remove Page Break", "pageLayout.breaks", json!({"remove": true})),
                ("Reset All Page Breaks", "pageLayout.breaks", json!({"reset": true})),
            ],
        );
    });
    if big_button(ui, Icon::Sheet, "Print\nTitles", "Print Titles", false).clicked() {
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
    let screen = app.ui.language.tr("Sheet Options|View");
    let print = app.ui.language.tr("Print");
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Gridlines")).strong().small());
        let mut v = sh.0;
        if ui.checkbox(&mut v, screen).changed() {
            act(app, "view.gridlines", json!({"on": v}));
        }
        let mut p = sh.2;
        if ui.checkbox(&mut p, print).changed() {
            act(app, "pageLayout.printGridlines", json!({"on": p}));
        }
    });
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Headings")).strong().small());
        let mut v = sh.1;
        if ui.checkbox(&mut v, screen).changed() {
            act(app, "view.headings", json!({"on": v}));
        }
        let mut p = sh.3;
        if ui.checkbox(&mut p, print).changed() {
            act(app, "pageLayout.printHeadings", json!({"on": p}));
        }
    });
}

fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    if big_button(ui, Icon::Function, "Insert\nFunction", "Insert Function (⇧F3)", false).clicked() {
        app.open_dialog("insertFunction", json!({}));
    }
    let s = big_button(ui, Icon::Sum, "AutoSum", "AutoSum", true);
    if s.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    for (label, cat) in [
        ("Financial", "Financial"),
        ("Logical", "Logical"),
        ("Text", "Text"),
        ("Date &\nTime", "DateTime"),
        ("Lookup &\nReference", "Lookup"),
        ("Math &\nTrig", "MathTrig"),
        ("More\nFunctions", "Statistical"),
    ] {
        let r = big_button(ui, Icon::Book, label, label, true);
        egui::Popup::menu(&r).show(|ui| {
            ui.set_max_height(420.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in gridcraft_engine::cmd::formulas::function_list() {
                    if f["category"].as_str() == Some(cat)
                        && let Some(n) = f["name"].as_str()
                    {
                        let locale = app.ui.language.formula_locale();
                        let name =
                            app.session.active().map(|d| crate::formula_locale::completion_name(n, locale, &d.wb, d.wb.active_sheet)).unwrap_or(n);
                        let desc = crate::formula_locale::description(n, locale).unwrap_or_default();
                        if ui.button(name).on_hover_text(desc).clicked() {
                            app.run_or_alert("formulas.insertFunction", json!({"name": n}));
                            ui.close();
                        }
                    }
                }
            });
        });
    }
    sep(ui);
    if big_button(ui, Icon::Name, "Name\nManager", "Name Manager", false).clicked() {
        app.open_dialog("nameManager", json!({}));
    }
    ui.vertical(|ui| {
        if small_button(ui, Icon::Name, "Define Name", "Define Name", false).clicked() {
            app.open_dialog("defineName", json!({}));
        }
        let use_in = small_button(ui, Icon::Function, "Use in Formula", "Use in Formula", true);
        egui::Popup::menu(&use_in).show(|ui| {
            let names: Vec<String> = app.session.active().map(|d| d.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
            for n in names {
                if ui.button(&n).clicked() {
                    let cur = app.editor.as_ref().map(|e| e.text.clone()).unwrap_or_else(|| "=".into());
                    app.begin_edit(Some(format!("{cur}{n}")), false);
                }
            }
        });
        if small_button(ui, Icon::Table, "Create from Selection", "Create from Selection", false).clicked() {
            act(app, "formulas.createFromSelection", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Trace, "Trace Precedents", "Trace Precedents", false).clicked()
            && let Ok(r) = app.run("formulas.tracePrecedents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Trace, "Trace Dependents", "Trace Dependents", false).clicked()
            && let Ok(r) = app.run("formulas.traceDependents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Close, "Remove Arrows", "Remove Arrows", false).clicked() {
            app.grid.trace = None;
        }
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Fx, "Show Formulas", "Show Formulas (⌃`)", false).clicked() {
            act(app, "formulas.showFormulas", json!({}));
        }
        if small_button(ui, Icon::Validation, "Error Checking", "Error Checking", false).clicked() {
            app.open_dialog("errorChecking", json!({}));
        }
        if small_button(ui, Icon::Calc, "Evaluate Formula", "Evaluate Formula", false).clicked() {
            app.open_dialog("evaluateFormula", json!({}));
        }
        if small_button(ui, Icon::Search, "Watch Window", "Watch Window", false).clicked() {
            app.grid.pane = Some("watch".into());
        }
    });
    sep(ui);
    let co = big_button(ui, Icon::Calc, "Calculation\nOptions", "Calculation Options", true);
    egui::Popup::menu(&co).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Automatic", "formulas.calculationOptions", json!({"mode": "automatic"})),
                ("Automatic Except for Data Tables", "formulas.calculationOptions", json!({"mode": "automaticExceptTables"})),
                ("Manual", "formulas.calculationOptions", json!({"mode": "manual"})),
            ],
        );
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Calc, "Calculate Now", "Calculate Now (F9)", false).clicked() {
            act(app, "formulas.calculateNow", json!({}));
        }
        if small_button(ui, Icon::Sheet, "Calculate Sheet", "Calculate Sheet (⇧F9)", false).clicked() {
            act(app, "formulas.calculateSheet", json!({}));
        }
    });
}

fn data(app: &mut SheetApp, ui: &mut Ui) {
    let gd = big_button(ui, Icon::Folder, "Get Data\n(Text/CSV)", "Import a text or CSV file", false);
    if gd.clicked() {
        app.open_dialog("open", json!({}));
    }
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::SortAsc, "", "Sort A to Z", false).clicked() {
            act(app, "data.sortAscending", json!({}));
        }
        if small_button(ui, Icon::SortDesc, "", "Sort Z to A", false).clicked() {
            act(app, "data.sortDescending", json!({}));
        }
    });
    if big_button(ui, Icon::SortFilter, "Sort", "Custom Sort", false).clicked() {
        app.open_dialog("sort", json!({}));
    }
    if big_button(ui, Icon::Filter, "Filter", "Filter (⌘⇧F)", false).clicked() {
        act(app, "data.filter", json!({}));
    }
    ui.vertical(|ui| {
        if small_button(ui, Icon::Eraser, "Clear", "Clear Filter", false).clicked() {
            act(app, "data.clearFilter", json!({}));
        }
        if small_button(ui, Icon::Redo, "Reapply", "Reapply", false).clicked() {
            act(app, "data.reapply", json!({}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::TextColumns, "Text to\nColumns", "Text to Columns", false).clicked() {
        app.open_dialog("textToColumns", json!({}));
    }
    if big_button(ui, Icon::FillDown, "Flash\nFill", "Flash Fill (⌘E)", false).clicked() {
        act(app, "edit.flashFill", json!({}));
    }
    if big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false).clicked() {
        app.open_dialog("removeDuplicates", json!({}));
    }
    let dv = big_button(ui, Icon::Validation, "Data\nValidation", "Data Validation", true);
    egui::Popup::menu(&dv).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Data Validation…", "dialog:dataValidation", json!({})),
                ("Circle Invalid Data", "data.circleInvalid", json!({})),
                ("Clear Validation", "data.validation", json!({"clear": true})),
            ],
        );
    });
    sep(ui);
    let wi = big_button(ui, Icon::Calc, "What-If\nAnalysis", "What-If Analysis", true);
    egui::Popup::menu(&wi).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Goal Seek…", "dialog:goalSeek", json!({})),
                ("Scenario Manager…", "data.scenarioManager", json!({})),
                ("Data Table…", "data.dataTable", json!({})),
            ],
        );
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Group, "Group", "Group", false).clicked() {
            act(app, "data.group", json!({}));
        }
        if small_button(ui, Icon::Ungroup, "Ungroup", "Ungroup", false).clicked() {
            act(app, "data.ungroup", json!({}));
        }
        if small_button(ui, Icon::Subtotal, "Subtotal", "Subtotal", false).clicked() {
            app.open_dialog("subtotal", json!({}));
        }
    });
}

fn review(app: &mut SheetApp, ui: &mut Ui) {
    if big_button(ui, Icon::Spell, "Spelling", "Spelling (F7)", false).clicked() {
        app.open_dialog("spelling", json!({}));
    }
    if big_button(ui, Icon::Calc, "Workbook\nStatistics", "Workbook Statistics", false).clicked() {
        app.open_dialog("statistics", json!({}));
    }
    if big_button(ui, Icon::Validation, "Check\nAccessibility", "Check Accessibility", false).clicked() {
        app.open_dialog("accessibility", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Comment, "New\nComment", "New Comment", false).clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    if big_button(ui, Icon::Comment, "Show\nComments", "Comments pane", false).clicked() {
        app.grid.pane = Some("comments".into());
    }
    if big_button(ui, Icon::Delete, "Delete", "Delete Comment", false).clicked() {
        act(app, "review.deleteComment", json!({}));
    }
    let notes = big_button(ui, Icon::Note, "Notes", "Notes", true);
    egui::Popup::menu(&notes).show(|ui| {
        menu_items(app, ui, &[("New Note", "dialog:comment", json!({"threaded": false})), ("Show/Hide Note", "review.showNote", json!({}))]);
    });
    sep(ui);
    let protected = app.session.active().and_then(|d| d.wb.active().map(|s| s.is_protected())).unwrap_or(false);
    if big_button(ui, Icon::Lock, if protected { "Unprotect\nSheet" } else { "Protect\nSheet" }, "Protect Sheet", false).clicked() {
        if protected {
            app.open_dialog("unprotectSheet", json!({}));
        } else {
            app.open_dialog("protectSheet", json!({}));
        }
    }
    if big_button(ui, Icon::Book, "Protect\nWorkbook", "Protect Workbook structure", false).clicked() {
        act(app, "review.protectWorkbook", json!({}));
    }
}

fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _ = t;
    for (icon, label, cmd) in [
        (Icon::Normal, "Normal", "view.normal"),
        (Icon::PageBreak, "Page Break\nPreview", "view.pageBreakPreview"),
        (Icon::PageLayout, "Page\nLayout", "view.pageLayout"),
    ] {
        if big_button(ui, icon, label, label, false).clicked() {
            act(app, cmd, json!({}));
        }
    }
    sep(ui);
    let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
    let lang = app.ui.language;
    ui.vertical(|ui| {
        let mut fb = app.ui.formula_bar;
        if ui.checkbox(&mut fb, lang.tr("Formula Bar")).changed() {
            app.ui.formula_bar = fb;
        }
        let mut g = s.0;
        if ui.checkbox(&mut g, lang.tr("Gridlines")).changed() {
            act(app, "view.gridlines", json!({"on": g}));
        }
        let mut h = s.1;
        if ui.checkbox(&mut h, lang.tr("Headings")).changed() {
            act(app, "view.headings", json!({"on": h}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::Zoom, "Zoom", "Zoom…", false).clicked() {
        app.open_dialog("zoom", json!({}));
    }
    if big_button(ui, Icon::Search, "100%", "Zoom to 100%", false).clicked() {
        act(app, "view.zoom", json!({"percent": 100}));
    }
    if big_button(ui, Icon::Zoom, "Zoom to\nSelection", "Zoom to Selection", false).clicked() {
        let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
        act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
    }
    sep(ui);
    let fp = big_button(ui, Icon::Freeze, "Freeze\nPanes", "Freeze Panes", true);
    egui::Popup::menu(&fp).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Freeze Panes", "view.freezePanes", json!({})),
                ("Freeze Top Row", "view.freezeTopRow", json!({})),
                ("Freeze First Column", "view.freezeFirstColumn", json!({})),
                ("Unfreeze Panes", "view.unfreezePanes", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let label = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        egui::ComboBox::from_id_salt("display_theme").selected_text(lang.tr(label)).width(88.0).show_ui(ui, |ui| {
            for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                    act(app, "view.theme", json!({"mode": value}));
                }
            }
        });
    });
    sep(ui);
    // Interface language: settings live in the View tab, like Excel's Options. The label follows
    // the current language so it stays readable; the choices are always shown in their own script.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Interface language")).font(theme::ui_font(11.5)).color(t.text_dim));
        egui::ComboBox::from_id_salt("ui_language").width(120.0).selected_text(egui::RichText::new(lang.name()).font(theme::ui_font(12.5))).show_ui(
            ui,
            |ui| {
                for l in crate::i18n::Language::ALL {
                    if ui.selectable_label(l == lang, l.name()).clicked() {
                        act(app, "app.language.set", json!({"language": l.code()}));
                    }
                }
            },
        );
    });
}

fn automate(app: &mut SheetApp, ui: &mut Ui) {
    if big_button(ui, Icon::Script, "Command\nPalette", "Search and run any command", false).clicked() {
        app.open_dialog("commandSearch", json!({}));
    }
    if big_button(ui, Icon::Script, "Agent\nControl", "How agents drive GridCraft (MCP / control channel)", false).clicked() {
        app.open_dialog("agents", json!({}));
    }
    if big_button(ui, Icon::Script, "Action\nJournal", "Every command run in this session (replayable)", false).clicked() {
        app.open_dialog("journal", json!({}));
    }
    if big_button(ui, Icon::Script, "About\nGridCraft", "Version, contributors and the AI models that helped", false).clicked() {
        app.open_dialog("about", json!({}));
    }
}

fn table_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some((tname, header, totals, banded, banded_c, first, last, filter)) = app.session.active().and_then(|d| {
        let sh = d.wb.active()?;
        let t = sh.table_at(d.selection.active)?;
        Some((t.name.clone(), t.header_row, t.totals_row, t.banded_rows, t.banded_cols, t.first_col, t.last_col, t.filter_button))
    }) else {
        return;
    };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Table Name:").small());
        let field = egui::Id::new(("gridcraft.table_name", &tname));
        if let Some(name) = crate::widgets::committed_text(ui, field, &tname, Some(120.0))
            && name != tname
        {
            act(app, "table.rename", json!({"table": tname, "name": name}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false).clicked() {
        act(app, "data.removeDuplicates", json!({}));
    }
    if big_button(ui, Icon::Table, "Convert\nto Range", "Convert to Range", false).clicked() {
        act(app, "table.convertToRange", json!({"table": tname}));
    }
    sep(ui);
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("Header Row", "table.headerRow", header && filter),
            ("Total Row", "table.totalRow", totals),
            ("Banded Rows", "table.bandedRows", banded),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("First Column", "table.firstColumn", first),
            ("Last Column", "table.lastColumn", last),
            ("Banded Columns", "table.bandedColumns", banded_c),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    sep(ui);
    let st = big_button(ui, Icon::FormatTable, "Table\nStyles", "Table Styles", true);
    egui::Popup::menu(&st).show(|ui| table_gallery(app, ui, "table.style"));
}

fn chart_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some(id) = app.selected_chart else { return };
    let ae = big_button(ui, Icon::Chart, "Add Chart\nElement", "Add Chart Element", true);
    egui::Popup::menu(&ae).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Legend: Bottom", "chart.set", json!({"chart": id, "legend": "bottom"})),
                ("Legend: Right", "chart.set", json!({"chart": id, "legend": "right"})),
                ("Legend: Top", "chart.set", json!({"chart": id, "legend": "top"})),
                ("Legend: None", "chart.set", json!({"chart": id, "legend": "none"})),
                ("-", "", json!(null)),
                ("Data Labels: Show", "chart.set", json!({"chart": id, "dataLabels": true})),
                ("Data Labels: None", "chart.set", json!({"chart": id, "dataLabels": false})),
                ("Gridlines: Show", "chart.set", json!({"chart": id, "gridlines": true})),
                ("Gridlines: None", "chart.set", json!({"chart": id, "gridlines": false})),
            ],
        );
    });
    if big_button(ui, Icon::Redo, "Switch\nRow/Column", "Switch Row/Column", false).clicked() {
        act(app, "chart.switchRowColumn", json!({"chart": id}));
    }
    let ct = big_button(ui, Icon::ChartBar, "Change\nChart Type", "Change Chart Type", true);
    egui::Popup::menu(&ct).show(|ui| {
        for (label, k, s) in [
            ("Clustered Column", "column", ""),
            ("Stacked Column", "column", "stacked"),
            ("Bar", "bar", ""),
            ("Line", "line", "markers"),
            ("Area", "area", ""),
            ("Pie", "pie", ""),
            ("Doughnut", "doughnut", ""),
            ("Scatter", "scatter", ""),
            ("Radar", "radar", ""),
            ("Waterfall", "waterfall", ""),
            ("Funnel", "funnel", ""),
            ("Treemap", "treemap", ""),
        ] {
            if ui.button(label).clicked() {
                act(app, "chart.set", json!({"chart": id, "type": k, "subtype": s}));
            }
        }
    });
    if big_button(ui, Icon::Settings, "Format\nPane", "Format Chart pane", false).clicked() {
        app.grid.pane = Some("formatChart".into());
    }
    if big_button(ui, Icon::TextBox, "Chart\nTitle", "Edit chart title", false).clicked() {
        app.open_dialog("chartTitle", json!({"chart": id}));
    }
    if big_button(ui, Icon::Delete, "Delete\nChart", "Delete chart", false).clicked() {
        act(app, "chart.delete", json!({"chart": id}));
        app.selected_chart = None;
    }
}

/// Command-key shortcuts when the grid has focus.
pub fn shortcut(app: &mut SheetApp, key: Key, m: Modifiers) {
    let shift = m.shift;
    let id: Option<(&str, serde_json::Value)> = match key {
        Key::B => Some(("home.bold", json!({}))),
        Key::I => Some(("home.italic", json!({}))),
        Key::U => Some(("home.underline", json!({}))),
        Key::Z if shift => Some(("edit.redo", json!({}))),
        Key::Z => Some(("edit.undo", json!({}))),
        Key::Y => Some(("edit.redo", json!({}))),
        Key::A => Some(("edit.selectAll", json!({}))),
        Key::D => Some(("edit.fillDown", json!({}))),
        Key::R => Some(("edit.fillRight", json!({}))),
        Key::S if shift => Some(("file.saveAs", json!({}))),
        Key::S => Some(("file.save", json!({}))),
        Key::O => Some(("file.open", json!({}))),
        Key::N => Some(("file.new", json!({}))),
        Key::W => Some(("file.close", json!({}))),
        Key::K => {
            app.open_dialog("insertLink", json!({}));
            None
        }
        Key::F if shift => Some(("data.filter", json!({}))),
        Key::F => {
            app.open_dialog("find", json!({}));
            None
        }
        Key::H => {
            app.open_dialog("find", json!({"replace": true}));
            None
        }
        Key::G => {
            app.open_dialog("goTo", json!({}));
            None
        }
        Key::T if shift => Some(("formulas.autoSum", json!({}))),
        Key::T => Some(("insert.table", json!({}))),
        Key::E => Some(("edit.flashFill", json!({}))),
        Key::Num1 => {
            app.open_dialog("formatCells", json!({}));
            None
        }
        Key::Num9 if shift => Some(("home.unhideRows", json!({}))),
        Key::Num9 => Some(("home.hideRows", json!({}))),
        Key::Num0 if shift => Some(("home.unhideColumns", json!({}))),
        Key::Num0 => Some(("home.hideColumns", json!({}))),
        Key::Minus => {
            app.open_dialog("deleteCells", json!({}));
            None
        }
        Key::Plus | Key::Equals if shift => {
            app.open_dialog("insertCells", json!({}));
            None
        }
        Key::Semicolon => {
            // Insert today's date.
            let today = gridcraft_engine::calc::now_serial().floor();
            let text = app
                .session
                .active()
                .map(|d| gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(today), "m/d/yyyy", &d.wb).text)
                .unwrap_or_default();
            app.begin_edit(Some(text), false);
            None
        }
        Key::Backtick => Some(("formulas.showFormulas", json!({}))),
        _ => None,
    };
    if let Some((id, p)) = id {
        app.run_or_alert(id, p);
        app.grid.ensure_visible = true;
    }
}

#[allow(dead_code)]
fn unused(_: &Tokens, _: Icon) {
    let _ = icons::GREEN;
}
