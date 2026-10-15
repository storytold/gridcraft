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
/// Width of a [`split_button`]'s main part, left of its arrow.
pub const SPLIT_MAIN_W: f32 = 25.0;

/// The whole split button around its arrow's response: Excel drops the arrow's menu under it.
pub fn split_button_rect(arrow: &Response) -> Rect {
    arrow.rect.with_min_x(arrow.rect.min.x - SPLIT_MAIN_W)
}

pub fn split_button(ui: &mut Ui, icon: Icon, accent: Option<Color32>, tip: &str) -> (bool, Response) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(SPLIT_MAIN_W + 13.0, 24.0), Sense::hover());
    let main = Rect::from_min_size(rect.min, vec2(SPLIT_MAIN_W, 24.0));
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
    /// The selection's colour when the picker opened (Excel's Current swatch), if it has one.
    current: Option<[u8; 3]>,
    /// The Standard tab (honeycomb) rather than Custom (the picker).
    standard: bool,
}

impl MoreColors {
    fn new(rgb: [u8; 3]) -> Self {
        Self { hsva: egui::ecolor::Hsva::from_srgb(rgb), hex: hex_of(rgb), current: None, standard: true }
    }

    /// Opens on the selection's colour, as Excel does, so it can be read or adjusted; on the
    /// Custom tab, where its hex shows. Without one, on the last custom colour.
    fn open(current: Option<[u8; 3]>, recent: &[String]) -> Self {
        match current {
            Some(c) => Self { current: Some(c), standard: false, ..Self::new(c) },
            None => Self::new(recent.first().and_then(|h| parse_hex(h)).unwrap_or([0x44, 0x72, 0xC4])),
        }
    }
}

/// Our colour palette (theme row + tints + standard colours, then Recent Colors and More Colors…),
/// returns a picked hex colour, `Some("none")` for No Fill/Automatic. Arrow keys move a highlight
/// through the swatches and Enter applies; the key events are stripped so the grid behind the popup
/// does not also react (not while More Colors… is open, whose boxes take the keys).
pub fn color_palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, true, None, false)
}

/// [`color_palette`] for a selection whose colour is `current`: More Colors… opens on it.
/// `badges`: the menu was opened with keytips, so its rows show their letters as badges.
pub fn color_palette_current(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str, current: Option<[u8; 3]>, badges: bool) -> Option<String> {
    palette(ui, theme_colors, none_label, true, current, badges)
}

/// [`color_palette`] without More Colors…, for a menu that closes on any click inside it (a sub-menu
/// of a context menu), where the picker would vanish as soon as it is used. Recent Colors stays.
pub fn color_palette_fixed(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, false, None, false)
}

const SWATCH: f32 = 20.0;
const SWATCH_GAP: f32 = 5.0;
const TINT_H: f32 = 17.0;
/// Ten swatches and their gaps: the palette and everything in it keep to this width.
const GRID_W: f32 = SWATCH * 10.0 + SWATCH_GAP * 9.0;

/// A theme colour followed by the five tints and shades Excel offers under it. They depend on its
/// lightness, so white and black get greys rather than more white or black (measured in Excel 365:
/// white -5 to -50 %, black +50 to +5 %, very light -10 to -90 %, very dark +90 to +10 %, everything
/// else +80, +60, +40, -25, -50 %).
pub fn theme_tints(base: u32) -> [f64; 6] {
    let [r, g, b] = [(base >> 16) as u8, (base >> 8) as u8, base as u8].map(|v| v as f64 / 255.0);
    let l = (r.max(g).max(b) + r.min(g).min(b)) / 2.0;
    if l >= 0.999 {
        [0.0, -0.05, -0.15, -0.25, -0.35, -0.5]
    } else if l <= 0.001 {
        [0.0, 0.5, 0.35, 0.25, 0.15, 0.05]
    } else if l > 0.8 {
        [0.0, -0.1, -0.25, -0.5, -0.75, -0.9]
    } else if l < 0.2 {
        [0.0, 0.9, 0.75, 0.5, 0.25, 0.1]
    } else {
        [0.0, 0.8, 0.6, 0.4, -0.25, -0.5]
    }
}

/// The Standard tab's colours: a honeycomb of hexagons, white at the centre, hues around it growing
/// stronger and then darker towards the edge (six rings, 127 cells), and a row of greys below.
/// Returns the clicked colour; `current` gets an outline.
fn honeycomb(ui: &mut Ui, current: [u8; 3]) -> Option<[u8; 3]> {
    const RINGS: i32 = 6;
    const R: f32 = 8.5; // hexagon radius
    let t = Tokens::get(ui.ctx());
    let w = R * 3f32.sqrt();
    let size = vec2(GRID_W, (2 * RINGS + 1) as f32 * R * 1.5 + R * 0.5 + 34.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let center = pos2(rect.center().x, rect.top() + R + RINGS as f32 * R * 1.5);
    let mut picked = None;
    let mut cell = |ui: &mut Ui, at: egui::Pos2, rgb: [u8; 3], id: (i32, i32)| {
        let pts: Vec<egui::Pos2> = (0..6)
            .map(|k| {
                let a = std::f32::consts::FRAC_PI_3 * k as f32 + std::f32::consts::FRAC_PI_6;
                at + vec2(a.cos(), a.sin()) * (R - 0.6)
            })
            .collect();
        let resp = ui.interact(Rect::from_center_size(at, vec2(w * 0.9, R * 1.6)), ui.id().with(("honeycomb", id)), Sense::click());
        let fill = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let stroke = if resp.hovered() || rgb == current { Stroke::new(1.5, t.text) } else { Stroke::NONE };
        ui.painter().add(egui::Shape::convex_polygon(pts, fill, stroke));
        if resp.clicked() {
            picked = Some(rgb);
        }
    };
    for q in -RINGS..=RINGS {
        for r in (-RINGS).max(-q - RINGS)..=RINGS.min(-q + RINGS) {
            let ring = q.abs().max(r.abs()).max((q + r).abs());
            let at = center + vec2(w * (q as f32 + r as f32 / 2.0), R * 1.5 * r as f32);
            let rgb = if ring == 0 {
                [255, 255, 255]
            } else {
                let d = at - center;
                // Blues at the top, reds and oranges at the bottom, greens on the left, as Excel lays it out.
                let hue = (d.y.atan2(d.x) / std::f32::consts::TAU + 0.85).rem_euclid(1.0);
                // Pale near the centre, full colour in the middle rings, dark at the edge.
                let k = ring as f32 / RINGS as f32;
                let value = if k <= 0.5 { 1.0 } else { 1.0 - (k - 0.5) * 1.3 };
                let sat = (k * 1.9).min(1.0);
                egui::ecolor::Hsva::new(hue, sat, value, 1.0).to_srgb()
            };
            cell(ui, at, rgb, (q, r));
        }
    }
    // Greys from white to black.
    let y = rect.bottom() - R - 4.0;
    for i in 0..13 {
        let v = (255.0 * (1.0 - i as f32 / 12.0)).round() as u8;
        let at = pos2(rect.left() + R + 2.0 + i as f32 * (GRID_W - 2.0 * R - 4.0) / 12.0, y);
        cell(ui, at, [v, v, v], (100, i));
    }
    picked
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(tr(ui, text)).strong());
}

enum RowIcon {
    Swatch,
    Palette,
}

/// A full-width menu row with an icon and its access letter underlined, like Excel's No Fill and
/// More Colors… rows.
fn menu_row(ui: &mut Ui, label: &str, letter: char, icon: RowIcon, badge: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = tr(ui, label).to_string();
    let font = ui.style().text_styles.get(&egui::TextStyle::Button).cloned().unwrap_or_else(|| theme::ui_font(13.0));
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat { font_id: font, color: t.text, ..Default::default() };
    match text.char_indices().find(|&(_, c)| c == letter.to_ascii_uppercase()) {
        Some((i, c)) => {
            let end = i + c.len_utf8();
            job.append(&text[..i], 0.0, plain.clone());
            job.append(&text[i..end], 0.0, egui::TextFormat { underline: Stroke::new(1.0, t.text), ..plain.clone() });
            job.append(&text[end..], 0.0, plain);
        }
        None => job.append(&text, 0.0, plain),
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let width = GRID_W.max(galley.size().x + 40.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text));
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(18.0, 18.0));
    match icon {
        RowIcon::Swatch => {
            ui.painter().rect_stroke(icon_rect, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
        }
        RowIcon::Palette => icons::paint(ui.painter(), icon_rect, Icon::Palette, t.text),
    }
    ui.painter().galley(pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
    if badge {
        keytip_badge(ui, &letter.to_ascii_uppercase().to_string(), icon_rect.right_bottom());
    }
    response
}

/// A keytip badge (the letter to press) centred on `at`, drawn like the ribbon's.
fn keytip_badge(ui: &Ui, letter: &str, at: egui::Pos2) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("palette_keytips")));
    let text = painter.layout_no_wrap(letter.into(), theme::ui_bold(10.5), t.text);
    let rect = Rect::from_center_size(at, text.size() + vec2(7.0, 3.0));
    painter.rect_filled(rect, 2.0, t.input_bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.5, 1.5), text, t.text);
}

fn palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str, more_colors: bool, current: Option<[u8; 3]>, badges: bool) -> Option<String> {
    let mut picked = None;
    const ROWS: usize = 7; // 6 tint rows + 1 standard row
    const COLS: usize = 10;
    let pal_id = ui.id().with("__color_palette");
    let mut cur: Option<(usize, usize)> = ui.data_mut(|d| d.get_temp(pal_id)).unwrap_or(None);
    let more_id = pal_id.with("more");
    let mut more: Option<MoreColors> = ui.data_mut(|d| d.get_temp(more_id)).unwrap_or(None);
    // More Colors… starts collapsed each time the menu opens (a palette not drawn last pass has
    // just been opened).
    let seen_id = pal_id.with("seen");
    let pass = ui.ctx().cumulative_pass_nr();
    let last_seen: Option<u64> = ui.data_mut(|d| d.get_temp(seen_id));
    if last_seen.is_none_or(|p| p + 1 < pass) {
        more = None;
    }
    ui.data_mut(|d| d.insert_temp(seen_id, pass));
    let (mut dl, mut dr, mut du, mut dd, mut ent) = (false, false, false, false, false);
    // The rows' access letters, as in Excel's menu: N for No Fill / Automatic, M for More Colors….
    let (mut key_none, mut key_more) = (false, false);
    // Automatic (font colour) is A; No Fill / No Color is N.
    let none_key = if none_label.starts_with("No ") { egui::Key::N } else { egui::Key::A };
    if more.is_none() {
        ui.input_mut(|i| {
            key_none = i.consume_key(egui::Modifiers::NONE, none_key);
            key_more = more_colors && i.consume_key(egui::Modifiers::NONE, egui::Key::M);
            // The letter's typed text goes too, or the grid behind would start editing with it.
            if key_none || key_more {
                let letter = if key_more {
                    "m"
                } else if none_key == egui::Key::N {
                    "n"
                } else {
                    "a"
                };
                i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case(letter)));
            }
        });
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
            theme_hex(base, theme_tints(base)[r])
        } else {
            std_hex[c].to_string()
        });
        ui.data_mut(|d| d.remove_temp::<Option<(usize, usize)>>(pal_id));
    }
    ui.set_max_width(GRID_W);
    let t = Tokens::get(ui.ctx());
    let hover_ring = |ui: &Ui, r: Rect| {
        ui.painter().rect_stroke(r.expand(1.0), 0.0, Stroke::new(1.0, t.text), StrokeKind::Outside);
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.window), StrokeKind::Inside);
    };
    // Excel puts Automatic (font and border colours) above the swatches, No Fill / No Color below.
    let none_first = !none_label.starts_with("No ");
    if none_first && menu_row(ui, none_label, 'A', RowIcon::Swatch, badges && more.is_none()).clicked() {
        picked = Some("none".to_string());
    }
    heading(ui, "Theme Colors");
    // As in Excel: the theme colours on a row of their own, then each colour's five tints and
    // shades as one unbroken column.
    let tints: Vec<[f64; 6]> = (0..COLS).map(|i| theme_tints(theme_colors.get(i).copied().unwrap_or(0))).collect();
    // No space between the tint rows, so each column reads as one strip.
    let (row_gap, row_min) = (ui.spacing().item_spacing.y, ui.spacing().interact_size.y);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.spacing_mut().interact_size.y = 0.0;
    for row in 0..6 {
        if row == 1 {
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(SWATCH_GAP, 0.0);
            for (col, tint) in tints.iter().enumerate() {
                let base = theme_colors.get(col).copied().unwrap_or(0);
                let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
                let c = gridcraft_engine::model::style::apply_tint(rgb, tint[row]);
                let h = if row == 0 { SWATCH } else { TINT_H };
                let (r, resp) = ui.allocate_exact_size(vec2(SWATCH, h), Sense::click());
                ui.painter().rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
                if row == 0 {
                    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
                }
                if resp.hovered() || cur == Some((row, col)) {
                    hover_ring(ui, r);
                }
                if resp.clicked() {
                    picked = Some(theme_hex(base, tint[row]));
                }
            }
        });
    }
    ui.spacing_mut().item_spacing.y = row_gap;
    ui.spacing_mut().interact_size.y = row_min;
    heading(ui, "Standard Colors");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SWATCH_GAP;
        for (col, hex) in std_hex.iter().enumerate() {
            let c = gridcraft_engine::model::Color::from_hex(hex)
                .and_then(|c| c.resolve(&Default::default()))
                .map(theme::color32)
                .unwrap_or(Color32::BLACK);
            let (r, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
            ui.painter().rect_filled(r, 0.0, c);
            if resp.hovered() || cur == Some((ROWS - 1, col)) {
                hover_ring(ui, r);
            }
            if resp.clicked() {
                picked = Some(hex.to_string());
            }
        }
    });
    // Recent Colors: the custom colours picked below, newest first.
    let mut recent: Vec<String> = ui.ctx().data(|d| d.get_temp(recent_colors_id())).unwrap_or_default();
    if !recent.is_empty() {
        heading(ui, "Recent Colors");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SWATCH_GAP;
            for hex in &recent {
                let c = parse_hex(hex).map_or(Color32::BLACK, |c| Color32::from_rgb(c[0], c[1], c[2]));
                let (r, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                ui.painter().rect_filled(r, 0.0, c);
                if resp.hovered() {
                    hover_ring(ui, r);
                }
                if resp.on_hover_text(hex).clicked() {
                    picked = Some(hex.clone());
                }
            }
        });
    }
    ui.separator();
    if !none_first && menu_row(ui, none_label, 'N', RowIcon::Swatch, badges && more.is_none()).clicked() {
        picked = Some("none".to_string());
    }
    if key_none {
        picked = Some("none".to_string());
    }
    if more_colors {
        let row = menu_row(ui, "More Colors…", 'M', RowIcon::Palette, badges && more.is_none());
        let chevron = Rect::from_center_size(pos2(row.rect.right() - 12.0, row.rect.center().y), vec2(10.0, 10.0));
        let t = Tokens::get(ui.ctx());
        if more.is_some() {
            icons::paint(ui.painter(), chevron, Icon::Chevron, t.text_dim);
        } else {
            // Pointing right while collapsed: the chevron turned a quarter.
            let c = chevron.center();
            ui.painter().line(vec![c + vec2(-1.5, -3.5), c + vec2(2.0, 0.0), c + vec2(-1.5, 3.5)], Stroke::new(1.2, t.text_dim));
        }
        if row.clicked() || key_more {
            more = match more {
                Some(_) => None,
                None => Some(MoreColors::open(current, &recent)),
            };
        }
    }
    if let Some(m) = more.as_mut() {
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.selectable_label(m.standard, tr(ui, "Standard")).clicked() {
                m.standard = true;
            }
            if ui.selectable_label(!m.standard, tr(ui, "Custom")).clicked() {
                m.standard = false;
            }
        });
        if m.standard {
            // A honeycomb of colours around white, then greys, as on Excel's Standard tab.
            if let Some(c) = honeycomb(ui, m.hsva.to_srgb()) {
                m.hsva = egui::ecolor::Hsva::from_srgb(c);
                m.hex = hex_of(c);
            }
        } else {
            ui.spacing_mut().slider_width = GRID_W - 20.0;
            // The saturation / value area with its hue bar, then the hex box with the new colour.
            if egui::color_picker::color_picker_hsva_2d(ui, &mut m.hsva, egui::color_picker::Alpha::Opaque) {
                m.hex = hex_of(m.hsva.to_srgb());
            }
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
            if let Some(c) = m.current {
                let (sw, resp) = ui.allocate_exact_size(vec2(36.0, 18.0), Sense::click());
                ui.painter().rect_filled(sw, 1.0, Color32::from_rgb(c[0], c[1], c[2]));
                ui.painter().rect_stroke(sw, 1.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
                // Clicking Current goes back to it after trying others.
                if resp.on_hover_text(hex_of(c)).clicked() {
                    m.hsva = egui::ecolor::Hsva::from_srgb(c);
                    m.hex = hex_of(c);
                }
                ui.label(egui::RichText::new("Current").small());
            }
        });
        ui.horizontal(|ui| {
            // Enter is OK, as in Excel's dialog, while the hex box holds a colour (a half-typed
            // one is not applied).
            let enter = parse_hex(&m.hex).is_some() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if ui.button("  OK  ").clicked() || enter {
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
