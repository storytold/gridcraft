//! Ribbon-style widgets: icon buttons, large buttons, split buttons, colour palettes, and the
//! message box / toast overlays.

use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::SheetApp;
use crate::i18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Translates a label or tooltip for the language of the frame being drawn (English if untranslated).
/// Ribbon chrome all flows through these widgets, so translating here localises it in one place.
fn tr<'a>(ui: &Ui, text: &'a str) -> &'a str {
    i18n::current(ui.ctx()).tr(text)
}

/// A flat square icon button with hover highlight and tooltip.
pub fn icon_button(ui: &mut Ui, icon: Icon, color: Color32, tip: &str, size: egui::Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = Tokens::get(ui.ctx());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, color);
    resp.on_hover_text(tr(ui, tip))
}

/// A toggle-able small button (e.g. Bold) showing a checked state.
pub fn toggle_button(ui: &mut Ui, icon: Icon, on: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    let t = Tokens::get(ui.ctx());
    if on {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, t.text);
    resp.on_hover_text(tr(ui, tip))
}

/// Large ribbon button: 32px icon over a one- or two-line label.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(11.5);
    let label = tr(ui, label);
    let lines: Vec<_> = label.split('\n').map(|line| ui.painter().layout_no_wrap(line.to_string(), font.clone(), t.text)).collect();
    let arrow_size = 10.0;
    let arrow_space = if dropdown { 4.0 + arrow_size } else { 0.0 };
    let text_w = lines.iter().enumerate().map(|(i, line)| line.size().x + if i + 1 == lines.len() { arrow_space } else { 0.0 }).fold(0.0, f32::max);
    let w = (text_w + 12.0).max(44.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 72.0), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 5.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.top() + 21.0), vec2(30.0, 30.0)), icon, t.text);
    for (i, line) in lines.iter().enumerate() {
        let has_arrow = dropdown && i + 1 == lines.len();
        let line_w = line.size().x + if has_arrow { arrow_space } else { 0.0 };
        let left = rect.center().x - line_w / 2.0;
        let y = rect.top() + 46.0 + i as f32 * 13.0;
        ui.painter().galley(pos2(left, y - line.size().y / 2.0), line.clone(), t.text);
        if has_arrow {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(left + line_w - arrow_size / 2.0, y + 1.0), vec2(arrow_size, arrow_size)),
                Icon::Chevron,
                t.text_dim,
            );
        }
    }
    resp.on_hover_text(tr(ui, tip))
}

/// Small ribbon button: 16px icon with optional label to the right.
pub fn small_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(12.5);
    let label = tr(ui, label);
    let tw = if label.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x + 6.0 };
    let w = 24.0 + tw + if dropdown { 12.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 23.0), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0)), icon, t.text);
    if !label.is_empty() {
        ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, t.text);
    }
    if dropdown {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 7.0, rect.center().y + 1.0), vec2(10.0, 10.0)),
            Icon::Chevron,
            t.text_dim,
        );
    }
    resp.on_hover_text(tr(ui, tip))
}

/// A split button: main part runs the default action, the arrow opens a menu.
/// Returns (main clicked, arrow response).
pub fn split_button(ui: &mut Ui, icon: Icon, accent: Option<Color32>, tip: &str) -> (bool, Response) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(38.0, 24.0), Sense::hover());
    let main = Rect::from_min_size(rect.min, vec2(25.0, 24.0));
    let arrow = Rect::from_min_max(pos2(main.right(), rect.top()), rect.max);
    let m = ui.interact(main, ui.id().with((tip, "main")), Sense::click());
    let a = ui.interact(arrow, ui.id().with((tip, "arrow")), Sense::click());
    if m.hovered() || a.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
        ui.painter().line_segment([pos2(main.right(), rect.top() + 3.0), pos2(main.right(), rect.bottom() - 3.0)], Stroke::new(1.0, t.separator));
    }
    icons::paint(ui.painter(), Rect::from_center_size(main.center() - vec2(0.0, 2.0), vec2(16.0, 16.0)), icon, t.text);
    if let Some(c) = accent {
        ui.painter().rect_filled(Rect::from_min_size(pos2(main.left() + 4.0, main.bottom() - 5.0), vec2(17.0, 3.0)), 0.0, c);
    }
    icons::paint(ui.painter(), Rect::from_center_size(arrow.center(), vec2(10.0, 10.0)), Icon::Chevron, t.text_dim);
    (m.on_hover_text(tr(ui, tip)).clicked(), a)
}

/// A one-line text field for a value stored in the workbook, which callers reload every frame. The text being typed is
/// kept in egui memory until the field loses focus, then returned to commit (`None` when Escape cancels the edit).
pub fn committed_text(ui: &mut Ui, id: egui::Id, current: &str, width: Option<f32>) -> Option<String> {
    let width = width.unwrap_or(ui.spacing().text_edit_width);
    let draft = id.with("draft");
    let mut text = ui.data_mut(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(width));
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(draft));
        return (!ui.input(|i| i.key_pressed(egui::Key::Escape))).then_some(text);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(draft));
    }
    None
}

/// Our colour palette (theme row + tints + standard colours), returns a picked hex colour,
/// `Some("none")` for No Fill/Automatic. Arrow keys move a highlight through the swatches and
/// Enter applies; the key events are stripped so the grid behind the popup does not also react.
pub fn color_palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    let mut picked = None;
    const ROWS: usize = 7; // 6 tint rows + 1 standard row
    const COLS: usize = 10;
    let pal_id = ui.id().with("__color_palette");
    let mut cur: Option<(usize, usize)> = ui.data_mut(|d| d.get_temp(pal_id)).unwrap_or(None);
    let (mut dl, mut dr, mut du, mut dd, mut ent) = (false, false, false, false, false);
    ui.input(|i| {
        dl = i.key_pressed(egui::Key::ArrowLeft);
        dr = i.key_pressed(egui::Key::ArrowRight);
        du = i.key_pressed(egui::Key::ArrowUp);
        dd = i.key_pressed(egui::Key::ArrowDown);
        ent = i.key_pressed(egui::Key::Enter);
    });
    if dl || dr || du || dd {
        cur = match cur {
            None => Some((
                0,
                if dr {
                    1
                } else if dl {
                    COLS - 1
                } else {
                    0
                },
            )),
            Some((r, c)) => {
                let r2 = if du {
                    r.saturating_sub(1)
                } else if dd {
                    (r + 1).min(ROWS - 1)
                } else {
                    r
                };
                let c2 = if dl {
                    (c + COLS - 1) % COLS
                } else if dr {
                    (c + 1) % COLS
                } else {
                    c
                };
                Some((r2, c2))
            }
        };
    }
    if dl || dr || du || dd || ent {
        ui.ctx().input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            });
        });
    }
    let theme_hex = |base: u32, tint: f64| -> String {
        let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
        let c = gridcraft_engine::model::style::apply_tint(rgb, tint);
        let (r0, r1, r2) = (c[0], c[1], c[2]);
        format!("#{r0:02X}{r1:02X}{r2:02X}")
    };
    let std_hex = ["#C00000", "#FF0000", "#FFC000", "#FFFF00", "#92D050", "#00B050", "#00B0F0", "#0070C0", "#002060", "#7030A0"];
    if ent && let Some((r, c)) = cur {
        picked = Some(if r < ROWS - 1 {
            let base = theme_colors.get(c).copied().unwrap_or(0);
            theme_hex(base, [0.0f64, 0.8, 0.6, 0.4, -0.25, -0.5][r])
        } else {
            std_hex[c].to_string()
        });
        ui.data_mut(|d| d.remove_temp::<Option<(usize, usize)>>(pal_id));
    }
    if ui.button(none_label).clicked() {
        picked = Some("none".to_string());
    }
    ui.label(egui::RichText::new("Theme Colors").small());
    let order = [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let tints = [0.0f64, 0.8, 0.6, 0.4, -0.25, -0.5];
    egui::Grid::new(ui.id().with("theme_grid")).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for (row, tint) in tints.iter().enumerate() {
            for (col, &i) in order.iter().enumerate() {
                let base = theme_colors.get(i).copied().unwrap_or(0);
                let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
                let c = gridcraft_engine::model::style::apply_tint(rgb, *tint);
                let col32 = Color32::from_rgb(c[0], c[1], c[2]);
                let (r, resp) = ui.allocate_exact_size(vec2(16.0, 14.0), Sense::click());
                ui.painter().rect_filled(r, 1.0, col32);
                ui.painter().rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
                if resp.hovered() || cur == Some((row, col)) {
                    ui.painter().rect_stroke(r.expand(1.0), 1.0, Stroke::new(1.5, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.clicked() {
                    picked = Some(theme_hex(base, *tint));
                }
            }
            ui.end_row();
        }
    });
    ui.label(egui::RichText::new("Standard Colors").small());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (col, hex) in std_hex.iter().enumerate() {
            let c = gridcraft_engine::model::Color::from_hex(hex)
                .and_then(|c| c.resolve(&Default::default()))
                .map(theme::color32)
                .unwrap_or(Color32::BLACK);
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 14.0), Sense::click());
            ui.painter().rect_filled(r, 1.0, c);
            if resp.hovered() || cur == Some((ROWS - 1, col)) {
                ui.painter().rect_stroke(r.expand(1.0), 1.0, Stroke::new(1.5, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
            }
            if resp.clicked() {
                picked = Some(hex.to_string());
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(pal_id, cur));
    picked
}

pub fn message_box(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((title, msg)) = app.message.clone() else { return };
    let mut close = false;
    egui::Modal::new(egui::Id::new("message_box")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 16.0, Color32::from_rgb(0xF2, 0xC3, 0x2E));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "!", theme::ui_bold(20.0), Color32::WHITE);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).strong());
                ui.add(egui::Label::new(msg).wrap());
            });
        });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if close {
        app.message = None;
    }
}

pub fn toast(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((msg, at)) = app.toast.clone() else { return };
    if crate::now_ms() - at > 2500.0 {
        app.toast = None;
        return;
    }
    let Some(rect) = app.grid.rect else { return };
    egui::Area::new(egui::Id::new("toast")).fixed_pos(pos2(rect.center().x - 160.0, rect.bottom() - 48.0)).order(egui::Order::Tooltip).show(
        ctx,
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(320.0);
                ui.label(msg);
            });
        },
    );
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
}

/// The info bar under the ribbon (Excel's message-bar slot). Shows an open-time notice with an
/// optional action button and a dismiss ✕. Returns nothing; dismissing clears `app.notice`.
pub fn notice_bar(app: &mut SheetApp, ui: &mut Ui) {
    let Some(notice) = app.notice.clone() else { return };
    let t = Tokens::get(ui.ctx());
    let (bg, border) = match notice.kind {
        crate::NoticeKind::Warning => (t.notice_warn_bg, t.notice_warn_border),
        crate::NoticeKind::Info => (t.notice_info_bg, t.notice_info_border),
    };
    let mut dismiss = false;
    let mut action: Option<(String, String)> = None;
    egui::Panel::top("notice_bar").frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| {
        // Bottom border as a 1px strip spanning the bar.
        let bar = ui.max_rect();
        ui.painter().rect_stroke(bar, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
        ui.horizontal_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(
                ui.painter(),
                r,
                match notice.kind {
                    crate::NoticeKind::Warning => Icon::Lock,
                    crate::NoticeKind::Info => Icon::Note,
                },
                border,
            );
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&notice.text).font(theme::ui_font(12.5)).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, t.text_dim, "Dismiss", vec2(20.0, 20.0)).clicked() {
                    dismiss = true;
                }
                if let Some((label, dialog)) = &notice.action
                    && ui.button(egui::RichText::new(label).font(theme::ui_font(12.5))).clicked()
                {
                    action = Some((label.clone(), dialog.clone()));
                }
            });
        });
    });
    if let Some((_, dialog)) = action {
        app.open_dialog(&dialog, serde_json::json!({}));
    }
    if dismiss {
        app.notice = None;
    }
}

/// Truncates text with an ellipsis if it exceeds `max_width`.
pub fn truncate_with_ellipsis(ui: &egui::Ui, text: &str, max_width: f32) -> String {
    let font_id = egui::FontId::proportional(12.0);
    let full_width = ui.painter().layout_no_wrap(text.to_string(), font_id.clone(), egui::Color32::BLACK).size().x;
    if full_width <= max_width {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = ui.painter().layout_no_wrap(ellipsis.to_string(), font_id.clone(), egui::Color32::BLACK).size().x;
    let target_width = (max_width - ellipsis_width).max(0.0);

    let mut truncated = String::new();
    for ch in text.chars() {
        truncated.push(ch);
        let w = ui.painter().layout_no_wrap(truncated.clone(), font_id.clone(), egui::Color32::BLACK).size().x;
        if w > target_width {
            truncated.pop();
            break;
        }
    }
    truncated.push_str(ellipsis);
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_with_ellipsis() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let short = "Short";
            assert_eq!(truncate_with_ellipsis(ui, short, 100.0), "Short");

            let long = "Very Long Font Family Name Extra Super Long Text That Definitely Exceeds Width";
            let truncated = truncate_with_ellipsis(ui, long, 50.0);
            assert!(truncated.ends_with('…'));
            assert!(truncated.len() < long.len());
        });
        out.textures_delta.clear();
    }
}
