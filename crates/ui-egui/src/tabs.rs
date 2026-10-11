//! Sheet tabs and the status bar.

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::Visibility;
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::l10n::{Arg, Tr, msg, tip};
use crate::theme::{self, Tokens};

/// The rename field asks for focus only while it has not got it yet. Asking again on the frame it lost focus (Enter or a click elsewhere)
/// would keep the field focused after it is closed, so the keyboard stayed stuck in the sheet name.
fn rename_needs_focus(has_focus: bool, lost_focus: bool) -> bool {
    !has_focus && !lost_focus
}

pub fn sheet_tabs(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    egui::Panel::bottom("sheet_tabs").exact_size(32.0).frame(egui::Frame::NONE.fill(t.tab_bar)).show(ui, |ui| {
        let rect = ui.max_rect();
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.ribbon_border));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(8.0);
            if crate::widgets::icon_button(ui, Icon::Left, t.text_dim, &l.tr("Previous sheet"), vec2(24.0, 24.0)).clicked() {
                app.run_or_alert("sheet.previous", json!({}));
            }
            if crate::widgets::icon_button(ui, Icon::Right, t.text_dim, &l.tr("Next sheet"), vec2(24.0, 24.0)).clicked() {
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
            for (i, name, color, active) in sheets {
                let renaming = app.grid.renaming_tab == Some(i);
                let font = if active { theme::ui_bold(13.0) } else { theme::ui_font(13.0) };
                let tw = ui.painter().layout_no_wrap(name.clone(), font.clone(), t.text).size().x;
                let w = (tw + 32.0).max(72.0);
                let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click_and_drag());
                let r = r.shrink2(vec2(0.0, 1.0));
                if active {
                    ui.painter().rect_filled(Rect::from_min_max(pos2(r.left(), r.top() - 2.0), r.max), 4.0, t.tab_active);
                    ui.painter().rect_stroke(
                        Rect::from_min_max(pos2(r.left(), r.top() - 2.0), r.max),
                        4.0,
                        Stroke::new(1.0, t.ribbon_border),
                        StrokeKind::Inside,
                    );
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, t.hover);
                }
                if let Some(c) = color {
                    let band = Rect::from_min_size(pos2(r.left() + 6.0, r.bottom() - 4.0), vec2(r.width() - 12.0, 3.0));
                    ui.painter().rect_filled(band, 1.0, theme::color32(c));
                } else if active {
                    let band = Rect::from_center_size(pos2(r.center().x, r.bottom() - 3.0), vec2(tw.min(48.0), 2.5));
                    ui.painter().rect_filled(band, 1.0, t.accent);
                }
                if renaming {
                    let mut text = app.grid.rename_text.clone();
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(6.0, 5.0))));
                    let ed = child.add(egui::TextEdit::singleline(&mut text).font(theme::ui_font(13.0)).desired_width(r.width() - 12.0));
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
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, &name, font, if active { t.accent } else { t.text });
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
                    if ui.button(l.tr("Insert Sheet")).clicked() {
                        app.run_or_alert("home.insertSheet", json!({"before": i}));
                    }
                    if ui.button(l.tr("Delete")).clicked() {
                        app.run_or_alert("home.deleteSheet", json!({"sheet": i}));
                    }
                    if ui.button(l.tr("Rename")).clicked() {
                        app.grid.renaming_tab = Some(i);
                        app.grid.rename_text = name.clone();
                    }
                    if ui.button(l.tr("Move or Copy…")).clicked() {
                        app.open_dialog("moveSheet", json!({"sheet": i}));
                    }
                    if ui.button(l.tr("Duplicate")).clicked() {
                        app.run_or_alert("sheet.move", json!({"sheet": i, "to": i + 1, "copy": true}));
                    }
                    ui.menu_button(l.tr("Tab Color"), |ui| {
                        let colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_default();
                        if let Some(c) = crate::widgets::color_palette(ui, l, &colors, &l.tr("No Color")) {
                            app.run_or_alert("sheet.tabColor", json!({"sheet": i, "color": c}));
                            ui.close();
                        }
                    });
                    ui.separator();
                    if ui.button(l.tr("Hide")).clicked() {
                        app.run_or_alert("sheet.hide", json!({"sheet": i}));
                    }
                    if ui.button(l.tr("Unhide…")).clicked() {
                        app.run_or_alert("sheet.unhide", json!({}));
                    }
                    ui.separator();
                    if ui.button(l.tr("Protect Sheet…")).clicked() {
                        app.open_dialog("protectSheet", json!({}));
                    }
                    if ui.button(l.tr("Select All Sheets")).clicked() {
                        ui.close();
                    }
                });
            }
            ui.add_space(6.0);
            if crate::widgets::icon_button(
                ui,
                Icon::Plus,
                t.text_dim,
                &tip(&l, app.keymap.platform(), msg!("New sheet"), "home.insertSheet"),
                vec2(26.0, 26.0),
            )
            .clicked()
            {
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
                ui.painter().rect_filled(
                    thumb,
                    4.0,
                    if resp.hovered() || resp.dragged() { Color32::from_gray(150) } else { Color32::from_gray(190) },
                );
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
    let l = app.l10n;
    egui::Panel::bottom("status_bar").exact_size(26.0).frame(egui::Frame::NONE.fill(t.status_bar).inner_margin(egui::Margin::symmetric(12, 0))).show(
        ui,
        |ui| {
            ui.horizontal_centered(|ui| {
                let mode = match app.session.mode {
                    gridcraft_engine::Mode::Ready => l.tr("Ready"),
                    gridcraft_engine::Mode::Enter => l.tr("Enter"),
                    gridcraft_engine::Mode::Edit => l.tr("Edit"),
                    gridcraft_engine::Mode::Point => l.tr("Point"),
                };
                ui.label(egui::RichText::new(mode).font(theme::ui_font(12.5)).color(t.text_dim));
                if let Some(d) = app.session.active() {
                    if d.wb.calc.mode == gridcraft_engine::model::CalcMode::Manual {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(l.tr("Calculate")).font(theme::ui_font(12.5)).color(t.text_dim));
                    }
                    if !d.calc.circular.is_empty() {
                        ui.add_space(12.0);
                        let (si, c) = d.calc.circular[0];
                        let name = d.wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default();
                        let text =
                            l.text("ui-status-circular-references", &[("name", Arg::from(name.as_str())), ("cell", Arg::from(c.a1().as_str()))]);
                        ui.label(egui::RichText::new(text).font(theme::ui_font(12.5)).color(t.danger));
                    }
                    if app.session.clipboard.is_some() && app.editor.is_none() {
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(l.tr("Select destination and press Enter or choose Paste"))
                                .font(theme::ui_font(12.5))
                                .color(t.text_dim),
                        );
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
                    if crate::widgets::icon_button(ui, Icon::Plus, t.text_dim, &l.tr("Zoom In"), vec2(18.0, 18.0)).clicked() {
                        let z = ((zoom as f32 / 10.0).floor() * 10.0 + 10.0).min(400.0);
                        app.run_or_alert("view.zoom", json!({"percent": z}));
                    }
                    let mut z = zoom as f32;
                    let slider = ui.add(egui::Slider::new(&mut z, 10.0..=400.0).show_value(false).logarithmic(true));
                    if slider.changed() {
                        let _ = app.session.run("view.zoom", json!({"percent": z.round()}));
                    }
                    if crate::widgets::icon_button(ui, Icon::Minus, t.text_dim, &l.tr("Zoom Out"), vec2(18.0, 18.0)).clicked() {
                        let z = ((zoom as f32 / 10.0).ceil() * 10.0 - 10.0).max(10.0);
                        app.run_or_alert("view.zoom", json!({"percent": z}));
                    }
                    ui.add_space(10.0);
                    for (icon, tip, mode, cmd) in [
                        (Icon::PageBreak, l.tr("Page Break Preview"), "pageBreakPreview", "view.pageBreakPreview"),
                        (Icon::PageLayout, l.tr("Page Layout"), "pageLayout", "view.pageLayout"),
                        (Icon::Normal, l.tr("Normal"), "normal", "view.normal"),
                    ] {
                        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
                        let cur = if app.session.view_mode.is_empty() { "normal" } else { app.session.view_mode.as_str() };
                        if cur == mode {
                            ui.painter().rect_filled(r, 3.0, t.pressed);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 3.0, t.hover);
                        }
                        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, t.text_dim);
                        if resp.on_hover_text(&*tip).clicked() {
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
                            let text = gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(v), &code, &d.wb).text;
                            // The fixed pattern leaves a bare decimal separator after whole numbers.
                            text.trim_end_matches(d.wb.locale.regional.decimal).to_string()
                        };
                        let mut parts = Vec::new();
                        if st.numeric > 0 {
                            parts.push(l.text("ui-status-sum", &[("value", Arg::from(fmt(st.sum).as_str()))]).into_owned());
                        }
                        if st.count > 0 {
                            parts.push(l.text("ui-status-count", &[("n", Arg::from(usize::try_from(st.count).unwrap_or(usize::MAX)))]).into_owned());
                        }
                        if let Some(a) = st.average {
                            parts.push(l.text("ui-status-average", &[("value", Arg::from(fmt(a).as_str()))]).into_owned());
                        }
                        for p in parts {
                            ui.label(egui::RichText::new(p).font(theme::ui_font(12.5)).color(t.text_dim));
                            ui.add_space(10.0);
                        }
                    }
                });
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::rename_needs_focus;

    #[test]
    fn rename_field_takes_focus_once_and_lets_go_on_commit() {
        assert!(rename_needs_focus(false, false), "first frame: grab focus");
        assert!(!rename_needs_focus(true, false), "already focused: do not ask again");
        assert!(!rename_needs_focus(false, true), "Enter or click-away: must not take focus back");
    }
}
