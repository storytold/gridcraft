//! Sheet tabs and the status bar.

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::Visibility;
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

pub fn sheet_tabs(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    // The bottom-most bar carries the window's lower rounded corners.
    let rad = if app.ui.status_bar { 0 } else { theme::window_radius(ui.ctx()) };
    let frame = egui::Frame::NONE.fill(t.tab_bar).corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: rad, se: rad });
    egui::Panel::bottom("sheet_tabs").exact_size(32.0).frame(frame).show(ui, |ui| {
        let rect = ui.max_rect();
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.ribbon_border));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(8.0);
            if crate::widgets::icon_button(ui, Icon::Left, t.text_dim, "Previous sheet", vec2(24.0, 24.0)).clicked() {
                app.run_or_alert("sheet.previous", json!({}));
            }
            if crate::widgets::icon_button(ui, Icon::Right, t.text_dim, "Next sheet", vec2(24.0, 24.0)).clicked() {
                app.run_or_alert("sheet.next", json!({}));
            }
            ui.add_space(8.0);
            let Some(d) = app.session.active() else { return };
            let sheets: Vec<(usize, String, Option<[u8; 3]>, bool)> =
                d.wb.sheets
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.visibility == Visibility::Visible)
                    .map(|(i, s)| (i, s.name.clone(), s.tab_color.and_then(|c| c.resolve(&d.wb.theme)), i == d.wb.active_sheet))
                    .collect();
            let mut active_band: Option<Rect> = None;
            for (i, name, color, active) in sheets {
                let renaming = app.grid.renaming_tab == Some(i);
                let font = theme::ui_font(13.0);
                let tw = ui.painter().layout_no_wrap(name.clone(), font.clone(), t.text).size().x;
                let w = (tw + 32.0).max(72.0);
                let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click_and_drag());
                let r = r.shrink2(vec2(0.0, 1.0));
                let k = theme::fade(ui.ctx(), resp.id.with("on"), active);
                if k > 0.0 {
                    let pill = r.shrink2(vec2(2.0, 3.0));
                    ui.painter().rect_filled(pill, 6.0, t.tab_active.gamma_multiply(k));
                    ui.painter().rect_stroke(pill, 6.0, Stroke::new(1.0, t.ribbon_border.gamma_multiply(k)), StrokeKind::Inside);
                }
                if !active {
                    theme::hover_fill(ui, &resp, r.shrink2(vec2(2.0, 3.0)), 6.0);
                }
                if let Some(c) = color {
                    let band = Rect::from_min_size(pos2(r.left() + 8.0, r.bottom() - 5.0), vec2(r.width() - 16.0, 2.0));
                    ui.painter().rect_filled(band, 1.0, theme::color32(c));
                } else if active {
                    active_band = Some(Rect::from_center_size(pos2(r.center().x, r.bottom() - 4.0), vec2(tw.min(44.0), 2.0)));
                }
                if renaming {
                    let mut text = app.grid.rename_text.clone();
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(6.0, 5.0))));
                    let ed = child.add(egui::TextEdit::singleline(&mut text).font(theme::ui_font(13.0)).desired_width(r.width() - 12.0));
                    app.grid.rename_text = text.clone();
                    ed.request_focus();
                    if ed.lost_focus() {
                        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && text != name {
                            app.run_or_alert("sheet.rename", json!({"sheet": i, "name": text}));
                        }
                        app.grid.renaming_tab = None;
                    }
                } else {
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, &name, font, t.text_dim.lerp_to_gamma(t.text, k));
                }
                if resp.clicked() {
                    if app.editor.as_ref().is_some_and(|e| e.can_point()) {
                        // Point mode across sheets: switch without committing.
                        let _ = app.session.run("sheet.activate", json!({"sheet": i}));
                    } else {
                        if app.editor.is_some() {
                            app.commit_edit(0, 0, false, false);
                        }
                        app.run_or_alert("sheet.activate", json!({"sheet": i}));
                    }
                }
                if resp.double_clicked() {
                    app.grid.renaming_tab = Some(i);
                    app.grid.rename_text = name.clone();
                }
                // Drag to reorder.
                if resp.drag_stopped()
                    && let Some(p) = ui.input(|inp| inp.pointer.interact_pos())
                {
                    let dx = p.x - r.center().x;
                    let steps = (dx / w).round() as i64;
                    if steps != 0 {
                        let to = (i as i64 + steps).max(0) as usize;
                        app.run_or_alert("sheet.move", json!({"sheet": i, "to": to}));
                    }
                }
                resp.context_menu(|ui| {
                    ui.set_min_width(180.0);
                    if ui.button("Insert Sheet").clicked() {
                        app.run_or_alert("home.insertSheet", json!({"before": i}));
                    }
                    if ui.button("Delete").clicked() {
                        app.run_or_alert("home.deleteSheet", json!({"sheet": i}));
                    }
                    if ui.button("Rename").clicked() {
                        app.grid.renaming_tab = Some(i);
                        app.grid.rename_text = name.clone();
                    }
                    if ui.button("Move or Copy…").clicked() {
                        app.open_dialog("moveSheet", json!({"sheet": i}));
                    }
                    if ui.button("Duplicate").clicked() {
                        app.run_or_alert("sheet.move", json!({"sheet": i, "to": i + 1, "copy": true}));
                    }
                    ui.menu_button("Tab Color", |ui| {
                        let colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_default();
                        if let Some(c) = crate::widgets::color_palette(ui, &colors, "No Color") {
                            app.run_or_alert("sheet.tabColor", json!({"sheet": i, "color": c}));
                            ui.close();
                        }
                    });
                    ui.separator();
                    if ui.button("Hide").clicked() {
                        app.run_or_alert("sheet.hide", json!({"sheet": i}));
                    }
                    if ui.button("Unhide…").clicked() {
                        app.run_or_alert("sheet.unhide", json!({}));
                    }
                    ui.separator();
                    if ui.button("Protect Sheet…").clicked() {
                        app.open_dialog("protectSheet", json!({}));
                    }
                    if ui.button("Select All Sheets").clicked() {
                        ui.close();
                    }
                });
            }
            if let Some(band) = active_band {
                let band = theme::glide_rect(ui.ctx(), ui.id().with("sheet_band"), band, 0.055);
                ui.painter().rect_filled(band, 1.0, t.accent);
            }
            ui.add_space(6.0);
            if crate::widgets::icon_button(ui, Icon::Plus, t.text_dim, "New sheet (⇧F11)", vec2(26.0, 26.0)).clicked() {
                app.run_or_alert("home.insertSheet", json!({}));
            }
            // Horizontal position indicator (a slim scrollbar).
            let avail = ui.available_rect_before_wrap();
            if avail.width() > 120.0 {
                let bar = Rect::from_min_max(pos2(avail.left() + 40.0, avail.center().y - 4.0), pos2(avail.right() - 16.0, avail.center().y + 4.0));
                let used_w = app
                    .session
                    .active()
                    .and_then(|d| d.wb.active().and_then(|s| s.used_range().map(|u| s.col_left(u.end.col + 10) as f32)))
                    .unwrap_or(2000.0)
                    .max(2000.0);
                let view = app.view();
                let vis = app.grid.cells_rect.map(|r| r.width()).unwrap_or(1000.0);
                let total = used_w.max(view.scroll.x + vis);
                let frac0 = (view.scroll.x / total).clamp(0.0, 1.0);
                let frac1 = ((view.scroll.x + vis) / total).clamp(0.0, 1.0);
                let thumb = Rect::from_min_max(
                    pos2(bar.left() + bar.width() * frac0, bar.top()),
                    pos2(bar.left() + bar.width() * frac1.max(frac0 + 0.03), bar.bottom()),
                );
                let resp = ui.interact(bar, ui.id().with("hscroll"), Sense::click_and_drag());
                ui.painter().rect_filled(thumb, 4.0, t.text_dim.gamma_multiply(if resp.hovered() || resp.dragged() { 0.75 } else { 0.4 }));
                if resp.dragged() {
                    let dx = resp.drag_delta().x / bar.width() * total;
                    if let Some(v) = app.view_mut() {
                        v.scroll.x = (v.scroll.x + dx).max(0.0);
                    }
                }
            }
        });
    });
}

pub fn status_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let rad = theme::window_radius(ui.ctx());
    let frame = egui::Frame::NONE.fill(t.status_bar).inner_margin(egui::Margin::symmetric(12, 0)).corner_radius(egui::CornerRadius {
        nw: 0,
        ne: 0,
        sw: rad,
        se: rad,
    });
    egui::Panel::bottom("status_bar").exact_size(26.0).frame(frame).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            let mode = match app.session.mode {
                gridcraft_engine::Mode::Ready => "Ready",
                gridcraft_engine::Mode::Enter => "Enter",
                gridcraft_engine::Mode::Edit => "Edit",
                gridcraft_engine::Mode::Point => "Point",
            };
            ui.label(egui::RichText::new(mode).font(theme::ui_font(12.5)).color(t.text_dim));
            if let Some(d) = app.session.active() {
                if d.wb.calc.mode == gridcraft_engine::model::CalcMode::Manual {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Calculate").font(theme::ui_font(12.5)).color(t.text_dim));
                }
                if !d.calc.circular.is_empty() {
                    ui.add_space(12.0);
                    let (si, c) = d.calc.circular[0];
                    let name = d.wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default();
                    ui.label(egui::RichText::new(format!("Circular References: {name}!{}", c.a1())).font(theme::ui_font(12.5)).color(t.danger));
                }
                if app.session.clipboard.is_some() && app.editor.is_none() {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Select destination and press Enter or choose Paste").font(theme::ui_font(12.5)).color(t.text_dim));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Zoom percentage and slider.
                let zoom = app.session.active().and_then(|d| d.wb.active().map(|s| s.zoom)).unwrap_or(100);
                let zl = ui.add(
                    egui::Label::new(egui::RichText::new(format!("{zoom}%")).font(theme::ui_font(12.5)).color(t.text_dim)).sense(Sense::click()),
                );
                if zl.clicked() {
                    app.open_dialog("zoom", json!({}));
                }
                ui.add_space(4.0);
                if crate::widgets::icon_button(ui, Icon::Plus, t.text_dim, "Zoom In", vec2(18.0, 18.0)).clicked() {
                    let z = ((zoom as f32 / 10.0).floor() * 10.0 + 10.0).min(400.0);
                    app.run_or_alert("view.zoom", json!({"percent": z}));
                }
                let mut z = zoom as f32;
                ui.spacing_mut().interact_size.y = 12.0;
                ui.spacing_mut().slider_rail_height = 3.0;
                let slider = ui.add(egui::Slider::new(&mut z, 10.0..=400.0).show_value(false).logarithmic(true));
                if slider.changed() {
                    let _ = app.session.run("view.zoom", json!({"percent": z.round()}));
                }
                if crate::widgets::icon_button(ui, Icon::Minus, t.text_dim, "Zoom Out", vec2(18.0, 18.0)).clicked() {
                    let z = ((zoom as f32 / 10.0).ceil() * 10.0 - 10.0).max(10.0);
                    app.run_or_alert("view.zoom", json!({"percent": z}));
                }
                ui.add_space(10.0);
                for (icon, tip, mode, cmd) in [
                    (Icon::PageBreak, "Page Break Preview", "pageBreakPreview", "view.pageBreakPreview"),
                    (Icon::PageLayout, "Page Layout", "pageLayout", "view.pageLayout"),
                    (Icon::Normal, "Normal", "normal", "view.normal"),
                ] {
                    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
                    let cur = if app.session.view_mode.is_empty() { "normal" } else { app.session.view_mode.as_str() };
                    if cur == mode {
                        ui.painter().rect_filled(r, 3.0, t.pressed);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 3.0, t.hover);
                    }
                    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, t.text_dim);
                    if resp.on_hover_text(tip).clicked() {
                        app.run_or_alert(cmd, json!({}));
                    }
                }
                ui.add_space(16.0);
                // Average / Count / Sum.
                if let Some(d) = app.session.active()
                    && let Some(sh) = d.wb.active()
                    && !d.selection.is_single_cell()
                {
                    let st = gridcraft_engine::display::stats(sh, &d.selection.ranges);
                    let fmt = |v: f64| {
                        let code = d.wb.styles.get(sh.style_id(d.selection.active)).num_fmt.as_str().to_string();
                        let code = if code == "General" { "#,##0.##########".to_string() } else { code };
                        gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(v), &code, &d.wb)
                            .text
                            .trim_end_matches('.')
                            .to_string()
                    };
                    let mut parts = Vec::new();
                    if st.numeric > 0 {
                        parts.push(format!("Sum: {}", fmt(st.sum)));
                    }
                    if st.count > 0 {
                        parts.push(format!("Count: {}", st.count));
                    }
                    if let Some(a) = st.average {
                        parts.push(format!("Average: {}", fmt(a)));
                    }
                    for p in parts {
                        ui.label(egui::RichText::new(p).font(theme::ui_font(12.5)).color(t.text_dim));
                        ui.add_space(10.0);
                    }
                }
            });
        });
    });
}
