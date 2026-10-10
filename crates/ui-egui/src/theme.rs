//! Design tokens and fonts.
//!
//! Colours are our own, tuned to read like a modern desktop spreadsheet. Fonts come from the
//! operating system at runtime (nothing bundled): the UI uses the platform UI font, cells use the
//! best available match for the workbook font (Calibri/Carlito/Aptos/Arial…) with egui's built-in
//! fonts as the last resort.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    pub window: Color32,
    pub ribbon: Color32,
    pub ribbon_border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub accent: Color32,
    pub accent_dark: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub separator: Color32,
    pub grid_bg: Color32,
    pub gridline: Color32,
    pub header_bg: Color32,
    pub header_text: Color32,
    pub header_line: Color32,
    pub header_sel_bg: Color32,
    pub header_sel_text: Color32,
    pub header_all_bg: Color32,
    pub sel_fill: Color32,
    pub sel_border: Color32,
    pub input_bg: Color32,
    pub input_border: Color32,
    pub tab_bar: Color32,
    pub tab_active: Color32,
    pub status_bar: Color32,
    pub cell_text: Color32,
    pub menu_bg: Color32,
    pub shadow: Color32,
    pub danger: Color32,
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            dark: false,
            window: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            ribbon: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            ribbon_border: Color32::from_rgb(0xE3, 0xE3, 0xE3),
            text: Color32::from_rgb(0x24, 0x24, 0x24),
            text_dim: Color32::from_rgb(0x61, 0x61, 0x61),
            text_disabled: Color32::from_rgb(0xB0, 0xB0, 0xB0),
            accent: Color32::from_rgb(0x10, 0x7C, 0x41),
            accent_dark: Color32::from_rgb(0x0B, 0x5E, 0x31),
            accent_soft: Color32::from_rgb(0xDF, 0xF1, 0xE6),
            hover: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            pressed: Color32::from_rgb(0xDD, 0xDD, 0xDD),
            separator: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            grid_bg: Color32::WHITE,
            gridline: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_bg: Color32::from_rgb(0xF8, 0xF8, 0xF8),
            header_text: Color32::from_rgb(0x42, 0x42, 0x42),
            header_line: Color32::from_rgb(0xD6, 0xD6, 0xD6),
            header_sel_bg: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_sel_text: Color32::from_rgb(0x10, 0x7C, 0x41),
            header_all_bg: Color32::from_rgb(0xC8, 0xE6, 0xD3),
            sel_fill: Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x1C),
            sel_border: Color32::from_rgb(0x10, 0x7C, 0x41),
            input_bg: Color32::WHITE,
            input_border: Color32::from_rgb(0xD1, 0xD1, 0xD1),
            tab_bar: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            tab_active: Color32::WHITE,
            status_bar: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            cell_text: Color32::BLACK,
            // Translucent: menus and dialogs are glass over the frost `glass::pass` draws.
            menu_bg: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0xB8),
            shadow: Color32::from_black_alpha(40),
            danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),
        }
    }

    /// "Construct" dark: near-black teal surfaces, hairline chrome, phosphor-green accent.
    pub fn dark() -> Tokens {
        let accent = Color32::from_rgb(0x3D, 0xF5, 0x8C);
        Tokens {
            dark: true,
            window: Color32::from_rgb(0x07, 0x0D, 0x0D),
            ribbon: Color32::from_rgb(0x0A, 0x13, 0x13),
            ribbon_border: Color32::from_rgb(0x16, 0x26, 0x25),
            text: Color32::from_rgb(0xCD, 0xE2, 0xDC),
            text_dim: Color32::from_rgb(0x6B, 0x8A, 0x84),
            text_disabled: Color32::from_rgb(0x34, 0x4D, 0x49),
            accent,
            accent_dark: Color32::from_rgb(0x22, 0xC4, 0x6A),
            accent_soft: Color32::from_rgb(0x0E, 0x2A, 0x1D),
            hover: Color32::from_rgb(0x10, 0x1E, 0x1D),
            pressed: Color32::from_rgb(0x15, 0x2A, 0x27),
            separator: Color32::from_rgb(0x14, 0x23, 0x22),
            grid_bg: Color32::from_rgb(0x05, 0x0A, 0x0A),
            gridline: Color32::from_rgb(0x10, 0x1B, 0x1A),
            header_bg: Color32::from_rgb(0x08, 0x10, 0x10),
            header_text: Color32::from_rgb(0x4F, 0x6D, 0x67),
            header_line: Color32::from_rgb(0x14, 0x22, 0x21),
            header_sel_bg: Color32::from_rgb(0x0C, 0x22, 0x1A),
            header_sel_text: accent,
            header_all_bg: Color32::from_rgb(0x10, 0x34, 0x24),
            sel_fill: Color32::from_rgba_unmultiplied(0x3D, 0xF5, 0x8C, 0x1A),
            sel_border: accent,
            input_bg: Color32::from_rgb(0x05, 0x0B, 0x0B),
            input_border: Color32::from_rgb(0x1A, 0x2D, 0x2B),
            tab_bar: Color32::from_rgb(0x07, 0x0D, 0x0D),
            tab_active: Color32::from_rgb(0x0E, 0x1A, 0x19),
            status_bar: Color32::from_rgb(0x07, 0x0D, 0x0D),
            cell_text: Color32::from_rgb(0xCD, 0xE2, 0xDC),
            menu_bg: Color32::from_rgba_unmultiplied(0x0A, 0x16, 0x15, 0xA0),
            shadow: Color32::from_black_alpha(170),
            danger: Color32::from_rgb(0xFF, 0x6B, 0x5E),
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { Tokens::dark() } else { Tokens::light() }
    }
}

pub const UI: &str = "ui";
pub const UI_BOLD: &str = "ui-bold";
pub const CELL: &str = "cell";
pub const CELL_BOLD: &str = "cell-bold";
pub const CELL_ITALIC: &str = "cell-italic";
pub const CELL_BOLD_ITALIC: &str = "cell-bold-italic";
pub const SERIF: &str = "serif";
pub const MONO: &str = "mono";

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI.into()))
}
pub fn ui_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI_BOLD.into()))
}

/// The font family to use for a cell font name and weight.
pub fn cell_family(name: &str, bold: bool, italic: bool) -> FontFamily {
    let lower = name.to_ascii_lowercase();
    if lower.contains("courier") || lower.contains("mono") || lower.contains("consolas") {
        return FontFamily::Name(MONO.into());
    }
    if lower.contains("times")
        || lower.contains("georgia")
        || lower.contains("cambria")
        || lower.contains("serif") && !lower.contains("sans")
        || lower.contains("garamond")
    {
        return FontFamily::Name(SERIF.into());
    }
    FontFamily::Name(
        match (bold, italic) {
            (true, true) => CELL_BOLD_ITALIC,
            (true, false) => CELL_BOLD,
            (false, true) => CELL_ITALIC,
            (false, false) => CELL,
        }
        .into(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn try_load(paths: &[(&str, u32)]) -> Option<FontData> {
    for (p, index) in paths {
        if let Ok(bytes) = std::fs::read(p) {
            let mut fd = FontData::from_owned(bytes);
            fd.index = *index;
            return Some(fd);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn try_load(_paths: &[(&str, u32)]) -> Option<FontData> {
    None
}

fn home_fonts(file: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/Library/Fonts/{file}")
}

/// Candidate font files per role, in preference order.
fn candidates(role: &str) -> Vec<(String, u32)> {
    let mac_sys = "/System/Library/Fonts";
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let lin = ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF"];
    let mut v: Vec<(String, u32)> = Vec::new();
    let mut add = |s: String, i: u32| v.push((s, i));
    match role {
        "ui" => {
            add(format!("{mac_sys}/SFNS.ttf"), 0);
            add(format!("{mac_sys}/HelveticaNeue.ttc"), 0);
            add(format!("{win}\\segoeui.ttf"), 0);
            for d in lin {
                add(format!("{d}/cantarell/Cantarell-Regular.otf"), 0);
                add(format!("{d}/noto/NotoSans-Regular.ttf"), 0);
                add(format!("{d}/dejavu/DejaVuSans.ttf"), 0);
                add(format!("{d}/liberation/LiberationSans-Regular.ttf"), 0);
            }
        }
        "ui-bold" => {
            add(format!("{mac_sys}/HelveticaNeue.ttc"), 1);
            add(format!("{win}\\segoeuib.ttf"), 0);
            for d in lin {
                add(format!("{d}/noto/NotoSans-Bold.ttf"), 0);
                add(format!("{d}/dejavu/DejaVuSans-Bold.ttf"), 0);
                add(format!("{d}/liberation/LiberationSans-Bold.ttf"), 0);
            }
        }
        "cell" | "cell-bold" | "cell-italic" | "cell-bold-italic" => {
            let (carlito, calibri_win, calibri_mac, arial_mac, arial_win, lib, dejavu) = match role {
                "cell" => {
                    ("Carlito-Regular.ttf", "calibri.ttf", "Calibri.ttf", "Arial.ttf", "arial.ttf", "LiberationSans-Regular.ttf", "DejaVuSans.ttf")
                }
                "cell-bold" => (
                    "Carlito-Bold.ttf",
                    "calibrib.ttf",
                    "Calibri Bold.ttf",
                    "Arial Bold.ttf",
                    "arialbd.ttf",
                    "LiberationSans-Bold.ttf",
                    "DejaVuSans-Bold.ttf",
                ),
                "cell-italic" => (
                    "Carlito-Italic.ttf",
                    "calibrii.ttf",
                    "Calibri Italic.ttf",
                    "Arial Italic.ttf",
                    "ariali.ttf",
                    "LiberationSans-Italic.ttf",
                    "DejaVuSans-Oblique.ttf",
                ),
                _ => (
                    "Carlito-BoldItalic.ttf",
                    "calibriz.ttf",
                    "Calibri Bold Italic.ttf",
                    "Arial Bold Italic.ttf",
                    "arialbi.ttf",
                    "LiberationSans-BoldItalic.ttf",
                    "DejaVuSans-BoldOblique.ttf",
                ),
            };
            // Calibri if the user has it installed system-wide (never from an Office bundle).
            add(format!("/Library/Fonts/{calibri_mac}"), 0);
            add(home_fonts(calibri_mac), 0);
            add(format!("{win}\\{calibri_win}"), 0);
            for d in lin {
                add(format!("{d}/crosextra/{carlito}"), 0);
                add(format!("{d}/carlito/{carlito}"), 0);
            }
            add(home_fonts(carlito), 0);
            add(format!("{mac_sup}/{arial_mac}"), 0);
            add(format!("{win}\\{arial_win}"), 0);
            for d in lin {
                add(format!("{d}/liberation/{lib}"), 0);
                add(format!("{d}/dejavu/{dejavu}"), 0);
            }
        }
        "serif" => {
            add(format!("{mac_sup}/Times New Roman.ttf"), 0);
            add(format!("{win}\\times.ttf"), 0);
            for d in lin {
                add(format!("{d}/liberation/LiberationSerif-Regular.ttf"), 0);
                add(format!("{d}/dejavu/DejaVuSerif.ttf"), 0);
            }
        }
        "mono" => {
            add(format!("{mac_sys}/SFNSMono.ttf"), 0);
            add(format!("{mac_sys}/Menlo.ttc"), 0);
            add(format!("{win}\\consola.ttf"), 0);
            for d in lin {
                add(format!("{d}/dejavu/DejaVuSansMono.ttf"), 0);
            }
        }
        _ => {}
    }
    v
}

/// Builds the font set from system fonts (with egui's defaults as fallback).
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let base_prop: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
        let paths = candidates(role);
        let refs: Vec<(&str, u32)> = paths.iter().map(|(p, i)| (p.as_str(), *i)).collect();
        let mut chain: Vec<String> = Vec::new();
        if let Some(fd) = try_load(&refs) {
            let key = format!("sys-{role}");
            fonts.font_data.insert(key.clone(), Arc::new(fd));
            chain.push(key);
        } else if role == UI_BOLD {
            chain.push(format!("sys-{UI}"));
        } else if role.starts_with("cell-") {
            chain.push(format!("sys-{CELL}"));
        }
        chain.extend(if role == MONO { base_mono.clone() } else { base_prop.clone() });
        chain.retain(|k| fonts.font_data.contains_key(k));
        fonts.families.insert(FontFamily::Name(role.into()), chain);
    }
    // Default proportional text in widgets uses the UI font.
    if let Some(ui) = fonts.families.get(&FontFamily::Name(UI.into())).cloned() {
        fonts.families.insert(FontFamily::Proportional, ui);
    }
    fonts
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    // Install both palettes before System can switch between the style slots.
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let t = if dark { Tokens::dark() } else { Tokens::light() };
        let mut v = if dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = t.window;
        v.window_fill = t.menu_bg;
        v.extreme_bg_color = t.input_bg;
        v.faint_bg_color = t.ribbon;
        v.code_bg_color = t.input_bg;
        v.selection.bg_fill = t.accent_soft;
        v.selection.stroke = Stroke::new(1.0, t.accent);
        v.text_cursor.stroke = Stroke::new(1.5, t.accent);
        v.hyperlink_color = t.accent;
        v.window_stroke =
            Stroke::new(1.0, if dark { Color32::from_rgba_unmultiplied(0x96, 0xFF, 0xC8, 0x22) } else { Color32::from_black_alpha(26) });
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.active.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = t.input_bg;
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.hovered.bg_fill = t.hover;
        v.widgets.active.weak_bg_fill = t.pressed;
        v.widgets.active.bg_fill = t.pressed;
        v.widgets.open.weak_bg_fill = t.hover;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, if dark { t.accent_dark.gamma_multiply(0.6) } else { t.input_border });
        v.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = egui::CornerRadius::same(6);
            w.expansion = 0.0;
        }
        v.window_corner_radius = egui::CornerRadius::same(12);
        v.menu_corner_radius = egui::CornerRadius::same(10);
        // Soft, low shadows: a heavy one would show through the glass and muddy it.
        let shadow = Color32::from_black_alpha(if dark { 90 } else { 28 });
        v.window_shadow = egui::epaint::Shadow { offset: [0, 14], blur: 40, spread: 0, color: shadow };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 26, spread: 0, color: shadow };
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.42 };
        ctx.set_visuals_of(theme, v);
    }
    ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.animation_time = 0.18;
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.interact_size.y = 22.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.slider_rail_height = 4.0;
        s.text_styles.insert(egui::TextStyle::Body, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Button, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Small, ui_font(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, ui_bold(18.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::new(12.0, FontFamily::Name(MONO.into())));
    });
}

/// Draws the sheet for a dark canvas from now on (call each frame with the effective theme, so
/// System appearance switches are followed).
pub fn set_canvas_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

/// Corner radius of the (undecorated) window; square when maximized or fullscreen.
pub fn window_radius(ctx: &egui::Context) -> u8 {
    let square = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
    if cfg!(target_os = "linux") && !square { 12 } else { 0 }
}

static DARK: AtomicBool = AtomicBool::new(false);

/// Workbook colour as drawn on the canvas. In dark mode lightness is mirrored (hue and
/// saturation kept), so black text reads light, white fills go near-black, and contrast between
/// a cell's text and its fill survives.
pub fn adapt(c: Color32) -> Color32 {
    if !DARK.load(Ordering::Relaxed) {
        return c;
    }
    let [r, g, b, a] = c.to_array();
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let (h, s) = if d < 1e-5 {
        (0.0, 0.0)
    } else {
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s)
    };
    // Mirror into [0.04, 0.86]: pure white lands on the canvas, pure black on soft white.
    let l2 = 0.04 + (1.0 - l) * 0.82;
    let c2 = (1.0 - (2.0 * l2 - 1.0).abs()) * s;
    let x = c2 * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match (h / 60.0) as u32 {
        0 => (c2, x, 0.0),
        1 => (x, c2, 0.0),
        2 => (0.0, c2, x),
        3 => (0.0, x, c2),
        4 => (x, 0.0, c2),
        _ => (c2, 0.0, x),
    };
    let m = l2 - c2 / 2.0;
    let u = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(u(r1), u(g1), u(b1), a)
}

// --- Motion -------------------------------------------------------------------------------
// Zero-bounce, decelerating moves; entrances ~160 ms, exits ~25% quicker.

fn frame_dt(ctx: &egui::Context) -> f32 {
    ctx.input(|i| i.stable_dt).clamp(0.0, 1.0 / 30.0)
}

/// Critically damped glide of a value toward `target` (no overshoot). `tau` is the time constant:
/// ~95% of the way after 3·tau.
pub fn glide(ctx: &egui::Context, id: egui::Id, target: f32, tau: f32) -> f32 {
    let k = 1.0 - (-frame_dt(ctx) / tau).exp();
    let pass = ctx.cumulative_pass_nr();
    let v = ctx.data_mut(|d| {
        // (value, pass it last advanced): repeated calls within a pass don't speed it up.
        let (v, at) = d.get_temp_mut_or(id, (target, pass));
        if *at != pass {
            *at = pass;
            *v += (target - *v) * k;
            if (target - *v).abs() < 0.2 {
                *v = target;
            }
        }
        *v
    });
    if v != target {
        ctx.request_repaint();
    }
    v
}

pub fn glide_rect(ctx: &egui::Context, id: egui::Id, target: egui::Rect, tau: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(glide(ctx, id.with(0), target.min.x, tau), glide(ctx, id.with(1), target.min.y, tau)),
        egui::pos2(glide(ctx, id.with(2), target.max.x, tau), glide(ctx, id.with(3), target.max.y, tau)),
    )
}

/// 0→1 presence for hover/press/appear states: decelerates in, accelerates out (faster).
pub fn fade(ctx: &egui::Context, id: egui::Id, on: bool) -> f32 {
    let dt = frame_dt(ctx);
    let v = ctx.data_mut(|d| {
        let v = d.get_temp_mut_or(id, 0.0_f32);
        *v = if on { (*v + dt / 0.16).min(1.0) } else { (*v - dt / 0.12).max(0.0) };
        *v
    });
    if (on && v < 1.0) || (!on && v > 0.0) {
        ctx.request_repaint();
    }
    // One curve serves both: ease-out while rising, ease-in while falling.
    1.0 - (1.0 - v).powi(3)
}

/// Animated hover/press background for a custom-drawn control.
pub fn hover_fill(ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, radius: f32) {
    let t = Tokens::get(ui.ctx());
    let down = resp.is_pointer_button_down_on();
    let h = fade(ui.ctx(), resp.id.with("hover"), resp.hovered() || down);
    if h > 0.0 {
        ui.painter().rect_filled(rect, radius, if down { t.pressed } else { t.hover }.gamma_multiply(h));
    }
}

/// Model colour → egui colour.
pub fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod canvas_tests {
    use super::*;

    #[test]
    fn dark_canvas_mirrors_lightness_and_keeps_hue() {
        set_canvas_dark(true);
        let near = |a: Color32, b: Color32| a.to_array().iter().zip(b.to_array()).all(|(x, y)| (*x as i16 - y as i16).abs() <= 2);
        assert!(near(adapt(Color32::BLACK), Color32::from_gray(219)));
        assert!(near(adapt(Color32::WHITE), Color32::from_gray(10)));
        let red = adapt(Color32::from_rgb(255, 0, 0));
        assert!(red.r() > 200 && red.g() < 40 && red.b() < 40);
        set_canvas_dark(false);
        assert_eq!(adapt(Color32::BLACK), Color32::BLACK);
    }
}
