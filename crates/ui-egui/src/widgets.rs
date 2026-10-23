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

/// Keyboard shortcuts shown after a ribbon tooltip's command name: Excel for Mac's on macOS,
/// Excel for Windows' elsewhere (each one GridCraft binds). Only the name is translated.
const TOOLTIP_SHORTCUTS: &[(&str, &str, &str)] = &[
    ("Save", "⌘S", "Ctrl+S"),
    ("Undo", "⌘Z", "Ctrl+Z"),
    ("Redo", "⌘Y", "Ctrl+Y"),
    ("Paste", "⌘V", "Ctrl+V"),
    ("Cut", "⌘X", "Ctrl+X"),
    ("Copy", "⌘C", "Ctrl+C"),
    ("Bold", "⌘B", "Ctrl+B"),
    ("Italic", "⌘I", "Ctrl+I"),
    ("Underline", "⌘U", "Ctrl+U"),
    ("AutoSum", "⌘⇧T", "Alt+="),
    ("Table", "⌘T", "Ctrl+T"),
    ("Insert Link", "⌘K", "Ctrl+K"),
    ("Filter", "⌘⇧F", "Ctrl+Shift+L"),
    ("Flash Fill", "⌘E", "Ctrl+E"),
];

/// The shortcut a ribbon tooltip shows after `tip`, for this platform (`mac`).
pub fn tooltip_shortcut(tip: &str, mac: bool) -> Option<&'static str> {
    TOOLTIP_SHORTCUTS.iter().find(|(name, _, _)| *name == tip).map(|(_, m, other)| if mac { *m } else { *other })
}

/// A ribbon tooltip: the translated command name, then its shortcut for this platform, as Excel
/// shows them ("Bold (Ctrl+B)" on Windows, "Bold (⌘B)" on macOS).
fn tip_text(ui: &Ui, tip: &str) -> String {
    // A name shared with a two-line ribbon label (Czech "Automatický\nsoučet") reads as one line.
    let name = tr(ui, tip).replace('\n', " ");
    match tooltip_shortcut(tip, cfg!(target_os = "macos")) {
        Some(keys) => format!("{name} ({keys})"),
        None => name.to_string(),
    }
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
    resp.on_hover_text(tip_text(ui, tip))
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
    resp.on_hover_text(tip_text(ui, tip))
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
    resp.on_hover_text(tip_text(ui, tip))
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
    resp.on_hover_text(tip_text(ui, tip))
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
    (m.on_hover_text(tip_text(ui, tip)).clicked(), a)
}

/// Large split button, the size of [`big_button`]: icon over a one- or two-line label. The body runs
/// the default action and the arrow strip on the right opens a menu, so the two have separate hit
/// areas. Returns (body clicked, arrow response, the whole button rect for anchoring keytips).
pub fn large_split_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, menu_label: &str) -> (bool, Response, Rect) {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(11.5);
    let label = tr(ui, label);
    let menu_label = tr(ui, menu_label);
    let lines: Vec<_> = label.split('\n').map(|line| ui.painter().layout_no_wrap(line.to_string(), font.clone(), t.text)).collect();
    let text_w = lines.iter().map(|line| line.size().x).fold(0.0, f32::max);
    let main_w = (text_w + 12.0).max(44.0);
    let arrow_w = 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(main_w + arrow_w, 72.0), Sense::hover());
    let main = Rect::from_min_size(rect.min, vec2(main_w, rect.height()));
    let arrow = Rect::from_min_max(pos2(main.right(), rect.top()), rect.max);
    let m = ui.interact(main, ui.id().with((tip, "main")), Sense::click());
    let a = ui.interact(arrow, ui.id().with((tip, "arrow")), Sense::click());
    if m.is_pointer_button_down_on() {
        ui.painter().rect_filled(main, 5.0, t.pressed);
    } else if a.is_pointer_button_down_on() {
        ui.painter().rect_filled(arrow, 5.0, t.pressed);
    } else if m.hovered() {
        ui.painter().rect_filled(main, 5.0, t.hover);
    } else if a.hovered() {
        ui.painter().rect_filled(arrow, 5.0, t.hover);
    }
    ui.painter().line_segment([pos2(main.right(), rect.top() + 6.0), pos2(main.right(), rect.bottom() - 6.0)], Stroke::new(1.0, t.separator));
    icons::paint(ui.painter(), Rect::from_center_size(pos2(main.center().x, rect.top() + 21.0), vec2(30.0, 30.0)), icon, t.text);
    for (i, line) in lines.iter().enumerate() {
        ui.painter().galley(
            pos2(main.center().x - line.size().x / 2.0, rect.top() + 46.0 + i as f32 * 13.0 - line.size().y / 2.0),
            line.clone(),
            t.text,
        );
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(arrow.center().x, rect.top() + 49.0), vec2(10.0, 10.0)), Icon::Chevron, t.text_dim);
    m.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    a.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), menu_label));
    (m.on_hover_text(tr(ui, tip)).clicked(), a, rect)
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

/// How many custom colours the palette's Recent Colors row remembers.
pub const RECENT_COLORS: usize = 10;

/// `#RRGGBB` for a colour.
pub fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A colour typed as `#RRGGBB`, `RRGGBB`, `#RGB` or `RGB` (any case, spaces around it ignored).
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    let digits: Vec<u32> = t.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    let byte = |hi: u32, lo: u32| u8::try_from(hi * 16 + lo).ok();
    match digits[..] {
        [r, g, b] => Some([byte(r, r)?, byte(g, g)?, byte(b, b)?]),
        [r1, r2, g1, g2, b1, b2] => Some([byte(r1, r2)?, byte(g1, g2)?, byte(b1, b2)?]),
        _ => None,
    }
}

/// Puts `hex` first in the recent colours, once, keeping at most [`RECENT_COLORS`].
pub fn remember_color(recent: &mut Vec<String>, hex: &str) {
    let hex = hex.to_ascii_uppercase();
    recent.retain(|h| *h != hex);
    recent.insert(0, hex);
    recent.truncate(RECENT_COLORS);
}

fn recent_colors_id() -> egui::Id {
    egui::Id::new("gridcraft.recent_colors")
}

/// The More Colors… picker's state while it is open in a palette.
#[derive(Clone)]
struct MoreColors {
    hsva: egui::ecolor::Hsva,
    hex: String,
}

impl MoreColors {
    fn new(rgb: [u8; 3]) -> Self {
        Self { hsva: egui::ecolor::Hsva::from_srgb(rgb), hex: hex_of(rgb) }
    }
}

/// Our colour palette (theme row + tints + standard colours, then Recent Colors and More Colors…),
/// returns a picked hex colour, `Some("none")` for No Fill/Automatic. Arrow keys move a highlight
/// through the swatches and Enter applies; the key events are stripped so the grid behind the popup
/// does not also react (not while More Colors… is open, whose boxes take the keys).
pub fn color_palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, true)
}

/// [`color_palette`] without More Colors…, for a menu that closes on any click inside it (a sub-menu
/// of a context menu), where the picker would vanish as soon as it is used. Recent Colors stays.
pub fn color_palette_fixed(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, false)
}

fn palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str, more_colors: bool) -> Option<String> {
    let mut picked = None;
    const ROWS: usize = 7; // 6 tint rows + 1 standard row
    const COLS: usize = 10;
    let pal_id = ui.id().with("__color_palette");
    let mut cur: Option<(usize, usize)> = ui.data_mut(|d| d.get_temp(pal_id)).unwrap_or(None);
    let more_id = pal_id.with("more");
    let mut more: Option<MoreColors> = ui.data_mut(|d| d.get_temp(more_id)).unwrap_or(None);
    let (mut dl, mut dr, mut du, mut dd, mut ent) = (false, false, false, false, false);
    if more.is_none() {
        ui.input(|i| {
            dl = i.key_pressed(egui::Key::ArrowLeft);
            dr = i.key_pressed(egui::Key::ArrowRight);
            du = i.key_pressed(egui::Key::ArrowUp);
            dd = i.key_pressed(egui::Key::ArrowDown);
            ent = i.key_pressed(egui::Key::Enter);
        });
    }
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
    // Recent Colors: the custom colours picked below, newest first.
    let mut recent: Vec<String> = ui.ctx().data(|d| d.get_temp(recent_colors_id())).unwrap_or_default();
    if !recent.is_empty() {
        ui.label(egui::RichText::new("Recent Colors").small());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for hex in &recent {
                let c = parse_hex(hex).map_or(Color32::BLACK, |c| Color32::from_rgb(c[0], c[1], c[2]));
                let (r, resp) = ui.allocate_exact_size(vec2(16.0, 14.0), Sense::click());
                ui.painter().rect_filled(r, 1.0, c);
                ui.painter().rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.0), 1.0, Stroke::new(1.5, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.on_hover_text(hex).clicked() {
                    picked = Some(hex.clone());
                }
            }
        });
    }
    if more_colors {
        ui.separator();
    }
    if more_colors && ui.button("More Colors…").clicked() {
        let start = recent.first().and_then(|h| parse_hex(h)).unwrap_or([0x44, 0x72, 0xC4]);
        more = Some(MoreColors::new(start));
    }
    if let Some(m) = more.as_mut() {
        let mut done = false;
        // The saturation / value area with its hue bar and egui's red-green-blue boxes, then the hex
        // box with the new colour beside it.
        if egui::color_picker::color_picker_hsva_2d(ui, &mut m.hsva, egui::color_picker::Alpha::Opaque) {
            m.hex = hex_of(m.hsva.to_srgb());
        }
        ui.horizontal(|ui| {
            let label = ui.label("Hex");
            let r = ui.add(egui::TextEdit::singleline(&mut m.hex).desired_width(72.0).char_limit(7)).labelled_by(label.id);
            if r.changed()
                && let Some(c) = parse_hex(&m.hex)
            {
                m.hsva = egui::ecolor::Hsva::from_srgb(c);
            }
            let c = m.hsva.to_srgb();
            let (sw, _) = ui.allocate_exact_size(vec2(36.0, 18.0), Sense::hover());
            ui.painter().rect_filled(sw, 1.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().rect_stroke(sw, 1.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
            ui.label(egui::RichText::new("New").small());
        });
        ui.horizontal(|ui| {
            if ui.button("  OK  ").clicked() {
                // A hex still being typed that is not a colour falls back to the picker's.
                let c = parse_hex(&m.hex).unwrap_or_else(|| m.hsva.to_srgb());
                let hex = hex_of(c);
                remember_color(&mut recent, &hex);
                picked = Some(hex);
                done = true;
            }
            if ui.button("Cancel").clicked() {
                done = true;
            }
        });
        if done {
            more = None;
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(recent_colors_id(), recent));
    ui.data_mut(|d| d.insert_temp(more_id, more));
    ui.data_mut(|d| d.insert_temp(pal_id, cur));
    picked
}

pub fn message_box(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((title, msg)) = app.message.clone() else { return };
    let title = app.ui.language.tr(&title).to_owned();
    let msg = app.ui.language.tr_error(&msg);
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
        // Back to the entry the message was about (a validation alert, a formula error), so it can
        // be fixed from the keyboard without clicking the cell again.
        if let Some(ed) = &mut app.editor {
            ed.request_focus = true;
        }
    }
}

pub fn toast(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((msg, at)) = app.toast.clone() else { return };
    if crate::now_ms() - at > 2500.0 {
        app.toast = None;
        return;
    }
    let msg = app.ui.language.tr_error(&msg);
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

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn hex_is_read_in_the_forms_people_type() {
        assert_eq!(parse_hex("#1A2b3C"), Some([0x1A, 0x2B, 0x3C]));
        assert_eq!(parse_hex(" 1a2b3c "), Some([0x1A, 0x2B, 0x3C]));
        assert_eq!(parse_hex("#f80"), Some([0xFF, 0x88, 0x00]));
        for bad in ["", "#", "#12", "#1234", "#12345", "#1234567", "#12345G", "##123456", "é12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(hex_of([0, 10, 255]), "#000AFF");
    }

    #[test]
    fn recent_colours_keep_the_newest_first_once_each_and_at_most_ten() {
        let mut r = Vec::new();
        for i in 0..12u8 {
            remember_color(&mut r, &hex_of([i, 0, 0]));
        }
        assert_eq!(r.len(), RECENT_COLORS);
        assert_eq!(r[0], "#0B0000");
        remember_color(&mut r, "#050000");
        remember_color(&mut r, "#0b0000");
        assert_eq!(&r[..2], ["#0B0000", "#050000"]);
        assert_eq!(r.iter().filter(|h| *h == "#050000").count(), 1);
        assert_eq!(r.len(), RECENT_COLORS);
    }
}

#[cfg(test)]
mod tooltip_shortcut_tests {
    use super::tooltip_shortcut;

    #[test]
    fn tooltips_show_each_platforms_own_shortcut() {
        // #1191: Windows and Linux show Excel for Windows' shortcuts, never the Mac ⌘ ones.
        assert_eq!(tooltip_shortcut("Bold", false), Some("Ctrl+B"));
        assert_eq!(tooltip_shortcut("AutoSum", false), Some("Alt+="));
        assert_eq!(tooltip_shortcut("Filter", false), Some("Ctrl+Shift+L"));
        assert_eq!(tooltip_shortcut("Bold", true), Some("⌘B"));
        assert_eq!(tooltip_shortcut("Filter", true), Some("⌘⇧F"));
        // Search commands has no shortcut of its own (Ctrl+Shift+Space selects all).
        assert_eq!(tooltip_shortcut("Search commands", false), None);
        for (name, mac, other) in super::TOOLTIP_SHORTCUTS {
            assert!(!other.contains(['⌘', '⇧', '⌥']), "{name}: {other}");
            assert!(mac.starts_with('⌘'), "{name}: {mac}");
        }
    }
}
