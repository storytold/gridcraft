//! Design tokens and fonts.
//!
//! Colours are our own, tuned to read like a modern desktop spreadsheet. Fonts come from the
//! operating system at runtime (nothing bundled): the UI uses the platform UI font, cells use the
//! best available match for the workbook font (Calibri/Carlito/Aptos/Arial…) with egui's built-in
//! fonts as the last resort.

use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

use crate::script::{ScriptClass, ScriptSet};

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
    /// Behind the pages in Page Layout view.
    pub desk: Color32,
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
    /// Background error checking: the corner triangle, and the warning button's diamond and mark.
    pub error_check_green: Color32,
    pub warning_fill: Color32,
    pub warning_ink: Color32,
    /// Info-bar (notice strip) fills, by severity.
    pub notice_warn_bg: Color32,
    pub notice_warn_border: Color32,
    pub notice_info_bg: Color32,
    pub notice_info_border: Color32,
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
            desk: Color32::from_rgb(0xDE, 0xDE, 0xDE),
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
            menu_bg: Color32::WHITE,
            shadow: Color32::from_black_alpha(40),
            danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),
            error_check_green: Color32::from_rgb(0x1E, 0x8E, 0x3E),
            warning_fill: Color32::from_rgb(0xF2, 0xB7, 0x05),
            warning_ink: Color32::BLACK,
            notice_warn_bg: Color32::from_rgb(0xFF, 0xF4, 0xCE),
            notice_warn_border: Color32::from_rgb(0xF0, 0xC3, 0x6E),
            notice_info_bg: Color32::from_rgb(0xEA, 0xF2, 0xFB),
            notice_info_border: Color32::from_rgb(0xB9, 0xD2, 0xEC),
        }
    }

    pub fn dark() -> Tokens {
        Tokens {
            dark: true,
            window: Color32::from_rgb(0x29, 0x29, 0x29),
            ribbon: Color32::from_rgb(0x33, 0x33, 0x33),
            ribbon_border: Color32::from_rgb(0x40, 0x40, 0x40),
            text: Color32::from_rgb(0xEE, 0xEE, 0xEE),
            text_dim: Color32::from_rgb(0xB0, 0xB0, 0xB0),
            text_disabled: Color32::from_rgb(0x70, 0x70, 0x70),
            accent: Color32::from_rgb(0x5C, 0xC5, 0x87),
            accent_dark: Color32::from_rgb(0x3F, 0xA8, 0x6C),
            accent_soft: Color32::from_rgb(0x23, 0x45, 0x31),
            hover: Color32::from_rgb(0x42, 0x42, 0x42),
            pressed: Color32::from_rgb(0x4D, 0x4D, 0x4D),
            separator: Color32::from_rgb(0x48, 0x48, 0x48),
            grid_bg: Color32::WHITE,
            desk: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            gridline: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_bg: Color32::from_rgb(0x30, 0x30, 0x30),
            header_text: Color32::from_rgb(0xCC, 0xCC, 0xCC),
            header_line: Color32::from_rgb(0x45, 0x45, 0x45),
            header_sel_bg: Color32::from_rgb(0x45, 0x45, 0x45),
            header_sel_text: Color32::from_rgb(0x5C, 0xC5, 0x87),
            header_all_bg: Color32::from_rgb(0x2E, 0x55, 0x3E),
            sel_fill: Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x24),
            sel_border: Color32::from_rgb(0x10, 0x7C, 0x41),
            input_bg: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            input_border: Color32::from_rgb(0x50, 0x50, 0x50),
            tab_bar: Color32::from_rgb(0x29, 0x29, 0x29),
            tab_active: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            status_bar: Color32::from_rgb(0x29, 0x29, 0x29),
            cell_text: Color32::BLACK,
            menu_bg: Color32::from_rgb(0x30, 0x30, 0x30),
            shadow: Color32::from_black_alpha(90),
            danger: Color32::from_rgb(0xF1, 0x70, 0x5F),
            error_check_green: Color32::from_rgb(0x4C, 0xC2, 0x6B),
            warning_fill: Color32::from_rgb(0xF2, 0xB7, 0x05),
            warning_ink: Color32::BLACK,
            notice_warn_bg: Color32::from_rgb(0x4A, 0x3F, 0x1E),
            notice_warn_border: Color32::from_rgb(0x7A, 0x67, 0x30),
            notice_info_bg: Color32::from_rgb(0x24, 0x33, 0x44),
            notice_info_border: Color32::from_rgb(0x3B, 0x55, 0x72),
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

/// Whether a family name reads as serif. Used both to route an unbound cell font to the serif role
/// and to decide whether the serif script fallbacks have to be loaded.
pub(crate) fn family_name_is_serif(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("times")
        || lower.contains("georgia")
        || lower.contains("cambria")
        || lower.contains("serif") && !lower.contains("sans")
        || lower.contains("garamond")
}

/// The font family to use for a cell font name and weight.
///
/// When the name is an installed font whose faces were bound by [`font_definitions`], that family
/// is used; otherwise the name is bucketed to one of the built-in roles (mono/serif/cell). egui
/// panics on a family that was never bound, so the installed branch is gated on
/// [`crate::fonts::is_bound`] and never names a family egui hasn't been given.
pub fn cell_family(name: &str, bold: bool, italic: bool) -> FontFamily {
    let role = crate::fonts::role_key(name, bold, italic);
    if crate::fonts::is_bound(&role) {
        return FontFamily::Name(role.into());
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("courier") || lower.contains("mono") || lower.contains("consolas") {
        return FontFamily::Name(MONO.into());
    }
    if family_name_is_serif(name) {
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

/// Width in points, at 100% zoom, of one line of cell text drawn as the grid draws it: in the
/// face [`cell_family`] picks, which on a machine without the named font is a wider or narrower
/// substitute (#1316). A family the active font set doesn't have yet (bound fonts apply from the
/// next frame) falls back to the engine's estimate rather than letting egui panic on it.
///
/// `exact` lays the text out (a galley kept until the end of the frame); otherwise the glyph
/// advances are summed without kerning, which is cheap and never narrower. This takes the
/// Context lock (`fonts_mut`): never call it, or run a command, while an egui lock is held.
pub fn cell_text_width(ctx: &egui::Context, text: &str, font: &gridcraft_engine::model::Font, exact: bool) -> f32 {
    let family = cell_family(&font.name, font.bold, font.italic);
    let id = FontId::new(font.size * 96.0 / 72.0, family);
    ctx.fonts_mut(|f| {
        f.families().contains(&id.family).then(|| {
            if exact {
                f.layout_no_wrap(text.to_owned(), id, Color32::PLACEHOLDER).size().x
            } else {
                // Half a point covers the galley's rounding of its width up to the pixel grid.
                text.chars().map(|c| f.glyph_width(&id, c)).sum::<f32>() + 0.5
            }
        })
    })
    .unwrap_or_else(|| gridcraft_engine::cmd::format::approx_text_width(text, font.size, font.bold))
}

/// Whether epaint can open face `index` of this font file and it maps characters to glyphs.
/// epaint panics (in `Context::set_fonts`, i.e. on startup) on a face it can't open, so every font
/// file we did not ship is checked first with the same parser and the same call epaint makes
/// (`skrifa::FontRef::from_index`): a corrupt file, something that is not an sfnt at all, or a
/// collection index past the end is rejected instead of crashing the app (#226). A face whose
/// character map is unreadable (a truncated file) would open but draw nothing, so it is refused
/// too and the next candidate gets its turn.
pub(crate) fn font_parses(bytes: &[u8], index: u32) -> bool {
    use skrifa::MetadataProvider;
    skrifa::FontRef::from_index(bytes, index).is_ok_and(|font| font.charmap().has_map())
}

/// Splits the index `fc-match` reports into the face index (low 16 bits) and the named instance
/// (high 16 bits, which fontconfig stores 1-based; 0 means the font's default instance).
pub(crate) fn fc_index_parts(raw: u32) -> (u32, Option<u32>) {
    (raw & 0xFFFF, (raw >> 16).checked_sub(1))
}

/// Applies a variable font's named instance through `FontTweak::coords`. `FontData::index` only
/// selects a face, so without this a variable font always renders its default instance: fontconfig
/// resolves `Noto Sans:weight=bold` to named instance 3 (wght 700) but the `fc-match` index is 4
/// (= 1-based instance in the high 16 bits), and Noto Sans CJK's default instance is Thin (100).
pub(crate) fn apply_named_instance(fd: &mut FontData, instance: u32) {
    use skrifa::MetadataProvider as _;
    let Ok(font) = skrifa::FontRef::from_index(fd.font.as_ref(), fd.index) else { return };
    let Some(named) = usize::try_from(instance).ok().and_then(|i| font.named_instances().get(i)) else {
        log::warn!("font face {:?} has no named instance {instance}", fd.index);
        return;
    };
    let mut coords = egui::epaint::text::VariationCoords::default();
    for (axis, value) in font.axes().iter().zip(named.user_coords()) {
        // `u32` is infallible for `IntoTag`; `&str`/`[u8; 4]` would panic on a malformed tag.
        coords.push(u32::from_be_bytes(axis.tag().into_bytes()), value);
    }
    fd.tweak.coords = coords;
}

/// Loads the first candidate that exists and parses. A file that exists but can't be parsed is
/// skipped with a warning and the next candidate is tried.
#[cfg(not(target_arch = "wasm32"))]
fn try_load(paths: &[(&str, u32)]) -> Option<FontData> {
    for (p, raw) in paths {
        let (index, instance) = fc_index_parts(*raw);
        if let Some(bytes) = font_file(p, index) {
            let mut fd = FontData::from_static(bytes);
            fd.index = index;
            if let Some(instance) = instance {
                apply_named_instance(&mut fd, instance);
            }
            return Some(fd);
        }
    }
    None
}

/// The interface and script fallback font files, each read once per run and kept for the rest of
/// it. egui copies an owned font's bytes into its text engine and keeps the original too
/// (`epaint`'s `blob_from_font_data`), so every one of these files was in memory twice, about
/// 100 MB on a Mac; bytes it borrows for `'static` it shares instead. These faces are loaded by
/// every font rebuild for as long as the app runs, so keeping them costs nothing, and rebuilds no
/// longer read them from disk again. Only a file that parses is kept: a missing or broken one is
/// looked for again next time.
#[cfg(not(target_arch = "wasm32"))]
fn font_file(path: &str, index: u32) -> Option<&'static [u8]> {
    static FILES: std::sync::Mutex<std::collections::BTreeMap<String, &'static [u8]>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
    let mut files = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&bytes) = files.get(path) {
        return font_parses(bytes, index).then_some(bytes);
    }
    let bytes = std::fs::read(path).ok()?;
    if !font_parses(&bytes, index) {
        log::warn!("skipping font {path} (face {index}): not a font file egui can read");
        return None;
    }
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    files.insert(path.to_string(), bytes);
    Some(bytes)
}

/// Last line of defence before [`FontDefinitions`] reach egui: drops every face epaint could not
/// parse (and its key from every family), so a bad font costs a fallback, never a crash.
pub(crate) fn drop_unparseable_fonts(fonts: &mut FontDefinitions) {
    let bad: Vec<String> = fonts.font_data.iter().filter(|(_, fd)| !font_parses(&fd.font, fd.index)).map(|(k, _)| k.clone()).collect();
    for key in &bad {
        log::warn!("dropping font {key:?}: egui can't parse it");
        fonts.font_data.remove(key);
    }
    if !bad.is_empty() {
        for chain in fonts.families.values_mut() {
            chain.retain(|k| !bad.contains(k));
        }
    }
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
    // ChromeOS's Linux container (Crostini) mounts the host's fonts under `chromeos/` (#1762).
    let lin = ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF", "/usr/share/fonts/chromeos"];
    let mut v: Vec<(String, u32)> = Vec::new();
    // fontconfig is the source of truth on Linux; the hard-coded paths below stay as a fallback
    // for systems where it is missing or returns nothing usable.
    #[cfg(target_os = "linux")]
    v.extend(linux_fontconfig_candidates(role));
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
            // Fedora keeps each family in its own package folder (#54).
            add("/usr/share/fonts/abattis-cantarell-fonts/Cantarell-Regular.otf".into(), 0);
            add("/usr/share/fonts/google-noto/NotoSans-Regular.ttf".into(), 0);
            add("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf".into(), 0);
            add("/usr/share/fonts/liberation-sans-fonts/LiberationSans-Regular.ttf".into(), 0);
        }
        "ui-bold" => {
            add(format!("{mac_sys}/HelveticaNeue.ttc"), 1);
            add(format!("{win}\\segoeuib.ttf"), 0);
            for d in lin {
                add(format!("{d}/noto/NotoSans-Bold.ttf"), 0);
                add(format!("{d}/dejavu/DejaVuSans-Bold.ttf"), 0);
                add(format!("{d}/liberation/LiberationSans-Bold.ttf"), 0);
            }
            add("/usr/share/fonts/google-noto/NotoSans-Bold.ttf".into(), 0);
            add("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf".into(), 0);
            add("/usr/share/fonts/liberation-sans-fonts/LiberationSans-Bold.ttf".into(), 0);
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
            // Fedora keeps each family in its own package folder, as for the UI fonts (#54).
            add(format!("/usr/share/fonts/google-carlito-fonts/{carlito}"), 0);
            add(home_fonts(carlito), 0);
            add(format!("{mac_sup}/{arial_mac}"), 0);
            add(format!("{win}\\{arial_win}"), 0);
            for d in lin {
                add(format!("{d}/liberation/{lib}"), 0);
                add(format!("{d}/dejavu/{dejavu}"), 0);
            }
            add(format!("/usr/share/fonts/liberation-sans-fonts/{lib}"), 0);
            add(format!("/usr/share/fonts/dejavu-sans-fonts/{dejavu}"), 0);
        }
        "serif" => {
            add(format!("{mac_sup}/Times New Roman.ttf"), 0);
            add(format!("{win}\\times.ttf"), 0);
            for d in lin {
                add(format!("{d}/liberation/LiberationSerif-Regular.ttf"), 0);
                add(format!("{d}/dejavu/DejaVuSerif.ttf"), 0);
            }
            add("/usr/share/fonts/liberation-serif-fonts/LiberationSerif-Regular.ttf".into(), 0);
            add("/usr/share/fonts/dejavu-serif-fonts/DejaVuSerif.ttf".into(), 0);
        }
        "mono" => {
            add(format!("{mac_sys}/SFNSMono.ttf"), 0);
            add(format!("{mac_sys}/Menlo.ttc"), 0);
            add(format!("{win}\\consola.ttf"), 0);
            for d in lin {
                add(format!("{d}/dejavu/DejaVuSansMono.ttf"), 0);
            }
            add("/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf".into(), 0);
            // Fedora installs DejaVu Sans Mono only on request. Liberation Mono comes with LibreOffice
            // (in Workstation and KDE by default), Noto Sans Mono (variable, Regular by default) always.
            for d in lin {
                add(format!("{d}/liberation/LiberationMono-Regular.ttf"), 0);
            }
            add("/usr/share/fonts/liberation-mono-fonts/LiberationMono-Regular.ttf".into(), 0);
            add("/usr/share/fonts/google-noto-vf/NotoSansMono[wght].ttf".into(), 0);
        }
        _ => {}
    }
    v
}

/// Fontconfig matches for a Linux font role, tried before the hard-coded paths.
///
/// The patterns describe intent (a family and optional weight/slant), not filesystem locations, so
/// distribution-specific font directories no longer need to be known here. `fc-match` always
/// returns its closest match, including a substitute for a missing family; `try_load` still
/// validates the file and face before using it.
#[cfg(target_os = "linux")]
fn linux_fontconfig_candidates(role: &str) -> Vec<(String, u32)> {
    let patterns: &[&str] = match role {
        "ui" => &["Cantarell", "Liberation Sans", "DejaVu Sans", "Noto Sans", "sans-serif"],
        "ui-bold" => &["Cantarell:weight=bold", "Liberation Sans:weight=bold", "DejaVu Sans:weight=bold", "sans-serif:weight=bold"],
        "cell" => &["Calibri", "Carlito", "Arial", "Liberation Sans", "DejaVu Sans", "sans-serif"],
        "cell-bold" => &["Calibri:weight=bold", "Carlito:weight=bold", "Arial:weight=bold", "Liberation Sans:weight=bold", "DejaVu Sans:weight=bold"],
        "cell-italic" => {
            &["Calibri:slant=italic", "Carlito:slant=italic", "Arial:slant=italic", "Liberation Sans:slant=italic", "DejaVu Sans:slant=italic"]
        }
        "cell-bold-italic" => &[
            "Calibri:weight=bold:slant=italic",
            "Carlito:weight=bold:slant=italic",
            "Arial:weight=bold:slant=italic",
            "Liberation Sans:weight=bold:slant=italic",
            "DejaVu Sans:weight=bold:slant=italic",
        ],
        "serif" => &["Liberation Serif", "DejaVu Serif", "serif"],
        "mono" => &["Liberation Mono", "DejaVu Sans Mono", "Noto Sans Mono", "monospace"],
        _ => &[],
    };
    fc_match_candidates(patterns)
}

/// Matches `patterns` in order, dropping duplicate file/face pairs. A missing family still yields
/// fontconfig's substitute, so this is a candidate list, not a guarantee that the named family is
/// installed.
#[cfg(target_os = "linux")]
fn fc_match_candidates(patterns: &[&str]) -> Vec<(String, u32)> {
    let mut candidates = Vec::new();
    for pattern in patterns {
        if let Some(font) = fc_match(pattern)
            && !candidates.contains(&font)
        {
            candidates.push(font);
        }
    }
    candidates
}

/// Builds the font set from system fonts (with egui's defaults as fallback), with the default
/// (non-Japanese) Han order. Never reads the host locale, so tests and snapshots are stable.
pub fn font_definitions() -> FontDefinitions {
    font_definitions_for_language(crate::i18n::Language::En)
}

/// Which Han face leads the fallback chain. Both Han faces cover kana and most kanji, so this only
/// decides glyph shapes (and which face catches a glyph the other lacks); switching between two
/// languages with the same order needs no font rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HanOrder {
    /// Japanese glyph shapes first (the interface is Japanese).
    Japanese,
    /// Simplified Chinese first: the widest Han coverage, for every other interface language.
    Chinese,
}

impl HanOrder {
    pub fn of(language: crate::i18n::Language) -> Self {
        if language == crate::i18n::Language::Ja { Self::Japanese } else { Self::Chinese }
    }
}

/// Builds the font set with the Han fallback faces ordered for the interface language.
pub fn font_definitions_for_language(language: crate::i18n::Language) -> FontDefinitions {
    font_definitions_with(language, &[], ScriptSet::default())
}

/// [`font_definitions_for_language`] that also binds the workbook fonts in `used` that are
/// installed, so cells set in them render with them; the rest fall back to the roles. The bound
/// families are recorded in [`crate::fonts`] so [`cell_family`] only names a family egui has.
pub fn font_definitions_with(language: crate::i18n::Language, used: &[String], scripts: ScriptSet) -> FontDefinitions {
    let (fonts, bound) = font_definitions_with_bound(language, used, scripts);
    crate::fonts::set_bound(bound);
    fonts
}

/// Builds the font set and returns the role families it bound, without publishing them to
/// [`crate::fonts`]. Used when egui will activate the set on its next pass: publishing the roles
/// now would let [`cell_family`] name families the current pass has not installed yet.
pub fn font_definitions_with_bound(language: crate::i18n::Language, used: &[String], scripts: ScriptSet) -> (FontDefinitions, Vec<String>) {
    let mut fonts = FontDefinitions::default();
    let base_prop: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut bound: Vec<String> = Vec::new();
    // The non-Latin fallback faces have to sit at the end of each chain: a Latin font owns Latin,
    // and epaint picks the first face that has the glyph, so they only catch their own scripts.
    let han = HanOrder::of(language);
    let wanted = wanted_scripts(language, scripts);
    // A workbook that has not been scanned reports no scripts and is handed every face; there is
    // nothing to draw yet, so the catalog supplement waits for the first scan rather than loading
    // a face per installed script on startup.
    let known = !scripts.is_empty();
    let fallback = load_fallback(&mut fonts, han, false, wanted, known);
    // Serif companions (Noto Serif CJK alone is ~55 MB) are loaded only when something asks for a
    // serif face: a workbook font whose name reads as serif, or the serif role. A serif face
    // chained to the sans faces would draw CJK in the wrong style (Times New Roman → sans CJK).
    let want_serif = used.iter().any(|name| family_name_is_serif(name));
    let serif_fallback = if want_serif { load_fallback(&mut fonts, han, true, wanted, known) } else { Vec::new() };
    for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
        let paths = candidates(role);
        let refs: Vec<(&str, u32)> = paths.iter().map(|(p, i)| (p.as_str(), *i)).collect();
        let mut chain: Vec<String> = Vec::new();
        if let Some(mut fd) = try_load(&refs) {
            // Helvetica Neue Bold's own TrueType hints close Cyrillic е into ө at 13 pt on a Retina
            // screen (#1184). Core Text never runs them, so this draws it the way macOS does.
            if cfg!(target_os = "macos") && role == UI_BOLD {
                fd.tweak.hinting = Some(false);
            }
            let key = format!("sys-{role}");
            fonts.font_data.insert(key.clone(), Arc::new(fd));
            chain.push(key);
        } else if role == UI_BOLD {
            chain.push(format!("sys-{UI}"));
        } else if role.starts_with("cell-") {
            chain.push(format!("sys-{CELL}"));
        }
        chain.extend(if role == MONO { base_mono.clone() } else { base_prop.clone() });
        let role_fallback = if role == SERIF && want_serif { &serif_fallback } else { &fallback };
        chain.extend(role_fallback.iter().cloned());
        chain.retain(|k| fonts.font_data.contains_key(k));
        fonts.families.insert(FontFamily::Name(role.into()), chain);
        bound.push(role.to_string());
    }
    // Installed workbook fonts, so a cell's chosen font actually renders in it. Each bound family
    // carries the script fallbacks for its own face: sans for upright faces, serif for serif ones.
    let script_fallbacks =
        crate::fonts::ScriptFallbacks { sans: fallback.clone(), serif: if want_serif { serif_fallback.clone() } else { fallback.clone() } };
    let installed = crate::fonts::installed_among(used);
    for family in &installed {
        bound.extend(crate::fonts::bind_family(&mut fonts, family, &script_fallbacks));
    }
    // A standard Excel name that is not installed still has a substitute the desktop already picks
    // (Times New Roman → Liberation Serif). Binding that substitute under the requested name is
    // what keeps bold and italic: SERIF and MONO carry one regular-weight chain each, so a cell in
    // Times New Roman used to lose both (#1231).
    let mut substituted: Vec<&String> = used.iter().filter(|name| !installed.iter().any(|i| i == *name)).collect();
    substituted.sort();
    substituted.dedup();
    for family in substituted {
        let Some(resolved) = crate::fonts::substitute_family(family) else { continue };
        let faces = crate::fonts::faces_of(&resolved);
        bound.extend(crate::fonts::bind_faces(&mut fonts, family, &faces, &script_fallbacks));
    }
    // Interface text (formula bar, name box, dialogs) falls back to the cell font before the
    // script faces: whatever a cell can draw, the formula bar can draw too, even where no script
    // fallback face was found (#349: macOS UI font without Hebrew, cells in Arial with it).
    let cell_key = format!("sys-{CELL}");
    if fonts.font_data.contains_key(&cell_key) {
        for role in [UI, UI_BOLD, SERIF, MONO] {
            if let Some(chain) = fonts.families.get_mut(&FontFamily::Name(role.into()))
                && !chain.contains(&cell_key)
            {
                let at = chain.iter().position(|k| fallback.contains(k)).unwrap_or(chain.len());
                chain.insert(at, cell_key.clone());
            }
        }
    }
    // Default proportional text in widgets uses the UI font.
    if let Some(ui) = fonts.families.get(&FontFamily::Name(UI.into())).cloned() {
        fonts.families.insert(FontFamily::Proportional, ui);
    }
    drop_unparseable_fonts(&mut fonts);
    // Only families egui still has data for count as bound (a dropped face can empty a chain).
    bound.retain(|f| fonts.families.get(&FontFamily::Name(f.as_str().into())).is_some_and(|c| !c.is_empty()));
    (fonts, bound)
}

/// One script's fallback faces: the sans list, plus a serif list where the script ships a serif
/// companion design (CJK, Thai, Lao, Khmer, Hebrew, Arabic, Devanagari). Scripts with a single
/// design (Myanmar, Tibetan, Yi) or only symbols have no serif list; a serif cell keeps that
/// script's sans face, which is still the right script (only the style differs).
#[cfg(not(target_arch = "wasm32"))]
struct ScriptFace {
    key: &'static str,
    sans: Vec<(String, u32)>,
    serif: Option<(&'static str, Vec<(String, u32)>)>,
    /// The scripts this face exists for: it is loaded only when the text or the interface needs one.
    covers: ScriptSet,
}

/// The scripts the interface itself draws: a Chinese, Japanese or Korean interface needs its
/// script loaded even when the workbook holds only Latin text.
fn interface_scripts(language: crate::i18n::Language) -> ScriptSet {
    match language {
        crate::i18n::Language::Zh => ScriptSet::EMPTY.with(ScriptClass::Han),
        crate::i18n::Language::Ja => ScriptSet::EMPTY.with(ScriptClass::Han).with(ScriptClass::Japanese),
        crate::i18n::Language::Ko => ScriptSet::EMPTY.with(ScriptClass::Korean),
        _ => ScriptSet::EMPTY,
    }
}

/// The scripts whose fallback faces to load: the workbook's, the interface's and the symbols every
/// interface draws. An empty workbook set (no text yet, or nothing scanned) keeps the old
/// behaviour and loads every face.
fn wanted_scripts(language: crate::i18n::Language, scripts: ScriptSet) -> ScriptSet {
    if scripts.is_empty() {
        return ScriptSet::all();
    }
    scripts.union(interface_scripts(language)).with(ScriptClass::Symbols)
}

/// Loads the system non-Latin fallback faces (one file per script family) and returns their keys so
/// every family can carry them as fallback. egui's bundled fonts have no coverage beyond Latin, so
/// without this a Thai, Hebrew, Japanese or Chinese workbook shows tofu boxes even though the text
/// was read correctly. With a face installed those scripts get real glyphs instead of tofu; nothing
/// is bundled, and a machine without the face keeps the old behaviour.
///
/// `serif` loads the serif companion faces instead of the sans ones. The caller loads those only
/// when a workbook or role actually asks for a serif face: Noto Serif CJK alone is ~55 MB, so
/// paying for it on startup would be wrong for the common case.
#[cfg(not(target_arch = "wasm32"))]
fn load_fallback(fonts: &mut FontDefinitions, han: HanOrder, serif: bool, wanted: ScriptSet, known: bool) -> Vec<String> {
    let mut keys = Vec::new();
    // Files already loaded as a fallback face. A file serves at most one key: the region-merged
    // Noto Sans CJK .ttc found for Han also carries Hangul (and Tahoma carries both Thai and
    // Hebrew), so loading it again for a later script would only duplicate tens of MB in memory.
    let mut loaded: Vec<String> = Vec::new();
    // The scripts the faces above actually answered for. A script can be wanted and still have no
    // face: the chains name files by path, and a host may install the script under a family those
    // paths do not name (a different package layout, or only one weight).
    let mut covered = ScriptSet::EMPTY;
    for face in fallback_faces(han) {
        if !face.covers.intersects(wanted) {
            continue; // nothing in this workbook or interface asks for the script
        }
        let (key, cands) = if serif {
            match &face.serif {
                Some((key, cands)) => (*key, cands),
                None => continue,
            }
        } else {
            (face.key, &face.sans)
        };
        for (path, index) in cands {
            if loaded.contains(path) {
                break; // a face loaded for an earlier script already covers this one
            }
            if let Some(fd) = try_load(&[(path.as_str(), *index)]) {
                fonts.font_data.insert(key.to_string(), Arc::new(fd));
                keys.push(key.to_string());
                loaded.push(path.clone());
                covered = covered.union(face.covers);
                break;
            }
        }
    }
    if known {
        keys.extend(load_catalog_fallback(fonts, wanted, covered, serif, &mut loaded));
    }
    keys
}

/// Loads one face from the installed catalog for every wanted script the hand-written chains did
/// not cover, so a script the system can draw is drawn even when its file sits where the chains do
/// not look (a package that names the family differently, or installs only the regular weight).
/// The catalog's choice is a single face per script — a fallback face only has to have the glyph,
/// and a cell asking for bold gets the closest face epaint finds in the chain, exactly as the
/// hand-written fallbacks already behave. Returns the keys of the faces it loaded.
#[cfg(not(target_arch = "wasm32"))]
fn load_catalog_fallback(fonts: &mut FontDefinitions, wanted: ScriptSet, covered: ScriptSet, serif: bool, loaded: &mut Vec<String>) -> Vec<String> {
    let Some(catalog) = crate::catalog::FontCatalog::installed() else { return Vec::new() };
    let mut keys = Vec::new();
    for script in ScriptClass::ALL {
        // Latin is drawn by the interface font and by every bound workbook font; adding a Latin
        // fallback face would put another Latin font at the end of chains that already have one.
        if script == ScriptClass::Latin {
            continue;
        }
        let single = ScriptSet::EMPTY.with(script);
        if !wanted.intersects(single) || covered.intersects(single) {
            continue;
        }
        let Some(entry) = catalog.font_for_script(script, serif) else { continue };
        let path = entry.path.to_string_lossy().into_owned();
        // A file already in the font set serves every script it covers; loading it again under a
        // second key would duplicate its bytes in memory (a CJK collection is tens of megabytes).
        if loaded.contains(&path) {
            continue;
        }
        let Some(fd) = try_load(&[(path.as_str(), entry.index)]) else { continue };
        let key = format!("sys-script-{}{}", script.name(), if serif { "-serif" } else { "" });
        fonts.font_data.insert(key.clone(), Arc::new(fd));
        keys.push(key);
        loaded.push(path);
    }
    keys
}

#[cfg(target_arch = "wasm32")]
fn load_fallback(_fonts: &mut FontDefinitions, _han: HanOrder, _serif: bool, _wanted: ScriptSet, _known: bool) -> Vec<String> {
    Vec::new()
}

/// Every fallback face we look for, in chain order, with its sans and (where it exists) serif
/// lists. One file per script family is enough — epaint picks the first face with the glyph, and a
/// script's glyphs live in one file. Order only matters between faces that both cover a glyph
/// (Japanese vs Chinese kanji), so the interface language reorders the two Han entries and nothing
/// else.
#[cfg(not(target_arch = "wasm32"))]
fn fallback_faces(han: HanOrder) -> Vec<ScriptFace> {
    let han_scripts = ScriptSet::EMPTY.with(ScriptClass::Han);
    let ja = ScriptFace {
        key: "sys-cjk-han-ja",
        sans: cjk_han_japanese(),
        serif: Some(("sys-cjk-han-ja-serif", cjk_han_japanese_serif())),
        covers: han_scripts.with(ScriptClass::Japanese),
    };
    let zh = ScriptFace {
        key: "sys-cjk-han-zh",
        sans: cjk_han_simplified(),
        serif: Some(("sys-cjk-han-zh-serif", cjk_han_simplified_serif())),
        covers: han_scripts,
    };
    let (first, second) = match han {
        HanOrder::Japanese => (ja, zh),
        HanOrder::Chinese => (zh, ja),
    };
    vec![
        first,
        second,
        ScriptFace {
            key: "sys-cjk-hangul",
            sans: cjk_hangul(),
            serif: Some(("sys-cjk-hangul-serif", cjk_hangul_serif())),
            covers: ScriptSet::EMPTY.with(ScriptClass::Korean),
        },
        ScriptFace { key: "sys-thai", sans: thai(), serif: Some(("sys-thai-serif", thai_serif())), covers: ScriptSet::EMPTY.with(ScriptClass::Thai) },
        ScriptFace { key: "sys-lao", sans: lao(), serif: Some(("sys-lao-serif", lao_serif())), covers: ScriptSet::EMPTY.with(ScriptClass::Lao) },
        ScriptFace {
            key: "sys-khmer",
            sans: khmer(),
            serif: Some(("sys-khmer-serif", khmer_serif())),
            covers: ScriptSet::EMPTY.with(ScriptClass::Khmer),
        },
        ScriptFace {
            key: "sys-hebrew",
            sans: hebrew(),
            serif: Some(("sys-hebrew-serif", hebrew_serif())),
            covers: ScriptSet::EMPTY.with(ScriptClass::Hebrew),
        },
        ScriptFace {
            key: "sys-arabic",
            sans: arabic(),
            serif: Some(("sys-arabic-serif", arabic_serif())),
            covers: ScriptSet::EMPTY.with(ScriptClass::Arabic),
        },
        ScriptFace {
            key: "sys-devanagari",
            sans: devanagari(),
            serif: Some(("sys-devanagari-serif", devanagari_serif())),
            covers: ScriptSet::EMPTY.with(ScriptClass::Devanagari),
        },
        ScriptFace { key: "sys-myanmar", sans: myanmar(), serif: None, covers: ScriptSet::EMPTY.with(ScriptClass::Myanmar) },
        ScriptFace { key: "sys-tibetan", sans: tibetan(), serif: None, covers: ScriptSet::EMPTY.with(ScriptClass::Tibetan) },
        ScriptFace { key: "sys-yi", sans: yi(), serif: None, covers: ScriptSet::EMPTY.with(ScriptClass::Yi) },
        ScriptFace { key: "sys-symbols", sans: symbols(), serif: None, covers: ScriptSet::EMPTY.with(ScriptClass::Symbols) },
        ScriptFace { key: "sys-symbols-2", sans: symbols_more(), serif: None, covers: ScriptSet::EMPTY.with(ScriptClass::Symbols) },
    ]
}

/// Linux directories that hold CJK fonts across distributions: Debian/Ubuntu (`opentype/noto`,
/// `truetype/wqy`), Fedora (`google-noto-cjk`), Arch (`noto-cjk`, `wenquanyi`), ChromeOS's Linux
/// container (`chromeos/notocjk`, #1762), plus local and flat layouts. Flatpak exposes the same
/// host layouts through read-only font mounts. These are fallbacks used only after fontconfig has
/// been asked.
#[cfg(not(target_arch = "wasm32"))]
const LINUX_CJK_FALLBACK_DIRS: [&str; 22] = [
    // Flatpak mounts host fonts here without a broad filesystem permission. Keep the same
    // distribution layouts as below; /usr/share/fonts inside the sandbox belongs to the runtime.
    "/run/host/fonts/opentype/noto",
    "/run/host/fonts/google-noto-cjk",
    "/run/host/fonts/noto-cjk",
    "/run/host/fonts/truetype/noto",
    "/run/host/fonts/noto",
    "/run/host/fonts/truetype",
    "/run/host/fonts/TTF",
    "/run/host/fonts",
    "/run/host/local-fonts",
    "/run/host/local-fonts/noto-cjk",
    "/run/host/user-fonts",
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/google-noto-cjk",
    "/usr/share/fonts/noto-cjk",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/noto",
    "/usr/share/fonts/truetype",
    "/usr/share/fonts/TTF",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "/usr/local/share/fonts/noto-cjk",
    "/usr/share/fonts/chromeos/notocjk",
];

/// Japanese Han + Kana faces (Japanese kanji glyph shapes).
///
/// The region-merged `NotoSansCJK-Regular.ttc` holds one face per locale (JP 0, KR 1, SC 2, TC 3,
/// HK 4) with the same glyph coverage, so the index only picks glyph shapes. Whichever Han list
/// comes first loads it; [`load_fallback`] does not load the same file again for the other.
#[cfg(not(target_arch = "wasm32"))]
fn cjk_han_japanese() -> Vec<(String, u32)> {
    let mac = "/System/Library/Fonts";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\YuGothR.ttc"), 0)); // Yu Gothic
    v.push((format!("{win}\\msgothic.ttc"), 0)); // MS Gothic
    v.push((format!("{mac}/ヒラギノ角ゴシック W3.ttc"), 0)); // Hiragino Sans
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=ja", "serif:lang=ja"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSansCJKjp-Regular.otf"), 0));
        v.push((format!("{d}/NotoSansCJK-Regular.ttc"), 0)); // JP face
    }
    v
}

#[cfg(target_os = "linux")]
thread_local! {
    /// Answers already obtained from `fc-match`, including failed lookups. Fonts rarely change
    /// during a session, and rebuilding a font set asks for the same patterns many times.
    ///
    /// No invalidation: fonts installed while GridCraft is running are not picked up until restart.
    static FC_MATCH_CACHE: std::cell::RefCell<std::collections::HashMap<String, Option<(String, u32)>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Asks fontconfig (`fc-match`, part of the standard Linux font stack) for the best face for
/// `pattern`. Distributions install fonts in different directories, so fontconfig is the primary
/// source; the [`LINUX_CJK_FALLBACK_DIRS`] list remains as a fallback for systems where it is
/// missing or returns nothing usable. `fc-match` also returns the collection index for the
/// locale-specific face (for example SC = 2 in `NotoSansCJK-Regular.ttc`) instead of guessing it
/// from the file name.
#[cfg(target_os = "linux")]
pub(crate) fn fc_match(pattern: &str) -> Option<(String, u32)> {
    if let Some(answer) = FC_MATCH_CACHE.with(|cache| cache.borrow().get(pattern).cloned()) {
        return answer;
    }
    let answer = fc_match_uncounted(pattern);
    FC_MATCH_CACHE.with(|cache| {
        cache.borrow_mut().insert(pattern.to_owned(), answer.clone());
    });
    answer
}

#[cfg(target_os = "linux")]
fn fc_match_uncounted(pattern: &str) -> Option<(String, u32)> {
    let output = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{index}\n", pattern]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut lines = stdout.lines();
    let file = lines.next()?.trim();
    if file.is_empty() || !std::path::Path::new(file).is_file() {
        return None;
    }
    // fontconfig stores a named instance in the high 16 bits (1-based, 0 = default) and the face
    // index in the low 16. Both are kept: `try_load` splits them, and the instance is applied
    // through `FontTweak::coords` so a variable font keeps the weight fontconfig matched.
    let index = lines.next().and_then(|line| line.trim().parse().ok()).unwrap_or(0);
    Some((file.to_owned(), index))
}

/// Simplified Chinese Han faces. Loaded apart from the Japanese ones so a glyph one family lacks
/// (simplified-only hanzi in a Japanese font) still falls through to a real glyph.
#[cfg(not(target_arch = "wasm32"))]
fn cjk_han_simplified() -> Vec<(String, u32)> {
    let mac = "/System/Library/Fonts";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\msyh.ttc"), 0)); // Microsoft YaHei
    v.push((format!("{win}\\simsun.ttc"), 0)); // SimSun
    v.push((format!("{win}\\simhei.ttf"), 0)); // SimHei
    v.push((format!("{mac}/Hiragino Sans GB.ttc"), 0)); // ships on every recent macOS
    v.push((format!("{mac}/PingFang.ttc"), 0)); // older macOS only; now a downloadable asset
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=zh-cn", "serif:lang=zh-cn"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSansCJKsc-Regular.otf"), 0));
        v.push((format!("{d}/NotoSansCJK-Regular.ttc"), 2)); // SC face
    }
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/wqy/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/wenquanyi/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/wqy-microhei.ttc"), 0));
    }
    v
}

/// Hangul faces (Korean). The Han faces above that come from the region-merged Noto Sans CJK file
/// already carry Hangul; [`load_fallback`] then skips this list rather than load the file twice.
#[cfg(not(target_arch = "wasm32"))]
fn cjk_hangul() -> Vec<(String, u32)> {
    let mac = "/System/Library/Fonts";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\malgun.ttf"), 0)); // Malgun Gothic
    v.push((format!("{mac}/AppleSDGothicNeo.ttc"), 0));
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=ko", "serif:lang=ko"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSansCJKkr-Regular.otf"), 0));
        v.push((format!("{d}/NotoSansCJK-Regular.ttc"), 1)); // KR face
    }
    v
}

/// Thai faces. Windows ships Leelawadee UI and Tahoma/Microsoft Sans Serif (both carry Thai);
/// macOS has Thonburi; Linux usually has Noto Sans Thai or Garuda.
#[cfg(not(target_arch = "wasm32"))]
fn thai() -> Vec<(String, u32)> {
    let mac_sys = "/System/Library/Fonts";
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let lin = [
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/truetype",
        "/usr/share/fonts",
        "/usr/share/fonts/chromeos/noto", // ChromeOS's Linux container (#1762)
    ];
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\LeelawUI.ttf"), 0)); // Leelawadee UI
    v.push((format!("{win}\\tahoma.ttf"), 0)); // Tahoma
    v.push((format!("{win}\\micross.ttf"), 0)); // Microsoft Sans Serif
    v.push((format!("{mac_sup}/Thonburi.ttc"), 0));
    v.push((format!("{mac_sys}/SukhumvitSet.ttc"), 0));
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=th"]));
    for d in lin {
        v.push((format!("{d}/NotoSansThai-Regular.ttf"), 0));
        v.push((format!("{d}/tlwg/Garuda.ttf"), 0));
        v.push((format!("{d}/garuda/Garuda.ttf"), 0));
    }
    // Fedora ships the variable Noto Sans Thai; its default instance is Regular.
    v.push(("/usr/share/fonts/google-noto-vf/NotoSansThai[wght].ttf".into(), 0));
    v
}

/// Lao faces. Windows ships Leelawadee UI (and DokChampa on older versions); macOS has Lao Sangam
/// MN and Lao MN; Linux usually has Noto Sans Lao or Phetsarath OT.
#[cfg(not(target_arch = "wasm32"))]
fn lao() -> Vec<(String, u32)> {
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let lin = ["/usr/share/fonts/truetype/noto", "/usr/share/fonts/opentype/noto", "/usr/share/fonts/truetype", "/usr/share/fonts"];
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\LeelawUI.ttf"), 0)); // Leelawadee UI
    v.push((format!("{win}\\dokchamp.ttf"), 0)); // DokChampa
    v.push((format!("{mac_sup}/Lao Sangam MN.ttf"), 0));
    v.push((format!("{mac_sup}/Lao MN.ttc"), 0));
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=lo"]));
    for d in lin {
        v.push((format!("{d}/NotoSansLao-Regular.ttf"), 0));
        v.push((format!("{d}/NotoSansLao[wght].ttf"), 0));
        v.push((format!("{d}/lao/Phetsarath_OT.ttf"), 0));
        v.push((format!("{d}/Phetsarath_OT.ttf"), 0));
    }
    v.push(("/usr/share/fonts/google-noto-vf/NotoSansLao[wght].ttf".into(), 0));
    v
}

/// Hebrew faces. Windows has David and FrankRuehl (and Tahoma/Courier New carry Hebrew); macOS has
/// Arial Hebrew and David; Linux usually has Noto Sans Hebrew or DejaVu Sans.
#[cfg(not(target_arch = "wasm32"))]
fn hebrew() -> Vec<(String, u32)> {
    let mac_sys = "/System/Library/Fonts";
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let lin = [
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/truetype",
        "/usr/share/fonts",
        "/usr/share/fonts/chromeos/noto", // ChromeOS's Linux container (#1762)
    ];
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\david.ttf"), 0)); // David
    v.push((format!("{win}\\frank.ttf"), 0)); // FrankRuehl
    v.push((format!("{win}\\tahoma.ttf"), 0)); // Tahoma
    v.push((format!("{mac_sys}/ArialHB.ttc"), 0)); // Arial Hebrew
    v.push((format!("{mac_sys}/SFHebrew.ttf"), 0));
    v.push((format!("{mac_sup}/Arial Hebrew.ttf"), 0));
    v.push((format!("{mac_sup}/David.ttf"), 0));
    v.push((format!("{mac_sys}/LucidaGrande.ttc"), 0));
    v.push((format!("{mac_sys}/Lucida Grande.ttc"), 0));
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["sans-serif:lang=he"]));
    for d in lin {
        v.push((format!("{d}/NotoSansHebrew-Regular.ttf"), 0));
        v.push((format!("{d}/dejavu/DejaVuSans.ttf"), 0));
        v.push((format!("{d}/freefont/FreeSans.ttf"), 0));
    }
    v.push(("/usr/share/fonts/google-noto-vf/NotoSansHebrew[wght].ttf".into(), 0));
    v
}

/// Fallback candidates for a script's sans face: fontconfig first (distributions place fonts
/// differently), then the common file names in the standard Noto layouts.
#[cfg(not(target_arch = "wasm32"))]
fn script_face(lang: &str, files: &[&str]) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    {
        let pattern = format!("sans-serif:lang={lang}");
        v.extend(fc_match_candidates(&[pattern.as_str()]));
    }
    for d in ["/usr/share/fonts/google-noto-vf", "/usr/share/fonts/truetype/noto", "/usr/share/fonts/opentype/noto", "/usr/share/fonts"] {
        for file in files {
            v.push((format!("{d}/{file}"), 0));
        }
    }
    v
}

/// Same for a script's serif face. Asking fontconfig for `serif:lang=…` picks the serif design
/// (Noto Serif Thai, Naskh Arabic, …) where the plain name would return the sans one.
#[cfg(not(target_arch = "wasm32"))]
fn script_serif(lang: &str, files: &[&str]) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    {
        let pattern = format!("serif:lang={lang}");
        v.extend(fc_match_candidates(&[pattern.as_str()]));
    }
    for d in ["/usr/share/fonts/google-noto-vf", "/usr/share/fonts/truetype/noto", "/usr/share/fonts/opentype/noto", "/usr/share/fonts"] {
        for file in files {
            v.push((format!("{d}/{file}"), 0));
        }
    }
    v
}

#[cfg(not(target_arch = "wasm32"))]
fn lao_serif() -> Vec<(String, u32)> {
    script_serif("lo", &["NotoSerifLao-Regular.ttf", "NotoSerifLao[wght].ttf"])
}

/// Khmer faces.
#[cfg(not(target_arch = "wasm32"))]
fn khmer() -> Vec<(String, u32)> {
    script_face("km", &["NotoSansKhmer-Regular.ttf", "NotoSansKhmer[wght].ttf"])
}

#[cfg(not(target_arch = "wasm32"))]
fn khmer_serif() -> Vec<(String, u32)> {
    script_serif("km", &["NotoSerifKhmer-Regular.ttf", "NotoSerifKhmer[wght].ttf"])
}

/// Arabic faces. The serif companion is Naskh (the traditional book design), not a Latin serif.
#[cfg(not(target_arch = "wasm32"))]
fn arabic() -> Vec<(String, u32)> {
    script_face("ar", &["NotoSansArabic-Regular.ttf", "NotoSansArabic[wdth,wght].ttf"])
}

#[cfg(not(target_arch = "wasm32"))]
fn arabic_serif() -> Vec<(String, u32)> {
    script_serif("ar", &["NotoNaskhArabic-Regular.ttf", "NotoNaskhArabic[wght].ttf"])
}

/// Devanagari faces.
#[cfg(not(target_arch = "wasm32"))]
fn devanagari() -> Vec<(String, u32)> {
    script_face("hi", &["NotoSansDevanagari-Regular.ttf", "NotoSansDevanagari[wdth,wght].ttf"])
}

#[cfg(not(target_arch = "wasm32"))]
fn devanagari_serif() -> Vec<(String, u32)> {
    script_serif("hi", &["NotoSerifDevanagari-Regular.ttf", "NotoSerifDevanagari[wdth,wght].ttf"])
}

/// Myanmar faces (Padauk is the common one; it has no serif companion).
#[cfg(not(target_arch = "wasm32"))]
fn myanmar() -> Vec<(String, u32)> {
    script_face("my", &["Padauk-Regular.ttf"])
}

/// Tibetan faces (Jomolhari is the common one; no serif companion).
#[cfg(not(target_arch = "wasm32"))]
fn tibetan() -> Vec<(String, u32)> {
    script_face("bo", &["Jomolhari-Regular.ttf"])
}

/// Yi (Nuosu) faces; Noto Sans Yi has no serif companion.
#[cfg(not(target_arch = "wasm32"))]
fn yi() -> Vec<(String, u32)> {
    script_face("ii", &["NotoSansYi-Regular.ttf"])
}

/// Thai serif faces (Noto Serif Thai where installed).
#[cfg(not(target_arch = "wasm32"))]
fn thai_serif() -> Vec<(String, u32)> {
    script_serif("th", &["NotoSerifThai-Regular.ttf", "NotoSerifThai[wght].ttf"])
}

/// Hebrew serif faces (Noto Serif Hebrew where installed).
#[cfg(not(target_arch = "wasm32"))]
fn hebrew_serif() -> Vec<(String, u32)> {
    script_serif("he", &["NotoSerifHebrew-Regular.ttf", "NotoSerifHebrew[wght].ttf"])
}

/// Japanese Han serif faces (Noto Serif CJK JP). The region-merged `NotoSerifCJK-Regular.ttc`
/// carries one face per locale (JP 0, KR 1, SC 2, TC 3, HK 4).
#[cfg(not(target_arch = "wasm32"))]
fn cjk_han_japanese_serif() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["serif:lang=ja"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSerifCJKjp-Regular.otf"), 0));
        v.push((format!("{d}/NotoSerifCJK-Regular.ttc"), 0));
    }
    v
}

#[cfg(not(target_arch = "wasm32"))]
fn cjk_han_simplified_serif() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["serif:lang=zh-cn"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSerifCJKsc-Regular.otf"), 0));
        v.push((format!("{d}/NotoSerifCJK-Regular.ttc"), 2));
    }
    v
}

#[cfg(not(target_arch = "wasm32"))]
fn cjk_hangul_serif() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["serif:lang=ko"]));
    for d in LINUX_CJK_FALLBACK_DIRS {
        v.push((format!("{d}/NotoSerifCJKkr-Regular.otf"), 0));
        v.push((format!("{d}/NotoSerifCJK-Regular.ttc"), 1));
    }
    v
}

/// Symbol faces behind the UI font. Interface fonts such as Noto Sans and Cantarell (and egui's
/// built-in Ubuntu) lack arrows, check marks, geometric shapes and keyboard symbols (▾ ✓ ✕ ▲ ↳ ⇧),
/// which then render as empty boxes (#197, #54). DejaVu Sans covers most of them; the paths cover
/// Debian/Ubuntu, Fedora and Arch layouts. [`load_fallback`] skips the file if Hebrew already
/// loaded it.
#[cfg(not(target_arch = "wasm32"))]
fn symbols() -> Vec<(String, u32)> {
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = vec![(format!("{win}\\seguisym.ttf"), 0), ("/System/Library/Fonts/Apple Symbols.ttf".into(), 0)];
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["DejaVu Sans", "Symbola", "Noto Sans Symbols 2", "Noto Sans Symbols"]));
    for d in [
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/dejavu-sans-fonts",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts/dejavu",
        "/usr/share/fonts/truetype/ttf-dejavu",
        "/usr/local/share/fonts/dejavu",
        "/usr/local/share/fonts",
    ] {
        v.push((format!("{d}/DejaVuSans.ttf"), 0));
    }
    v
}

/// A second symbol face for what the first lacks (⌘ and other technical symbols): Noto Sans
/// Symbols 2 where it is installed, or Segoe UI Symbol behind Segoe UI.
#[cfg(not(target_arch = "wasm32"))]
fn symbols_more() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    #[cfg(target_os = "linux")]
    v.extend(fc_match_candidates(&["Noto Sans Symbols 2", "Symbola", "Noto Sans Symbols"]));
    for d in [
        "/usr/share/fonts/noto",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/google-noto-sans-symbols-2-fonts",
        "/usr/share/fonts/google-noto",
        "/usr/local/share/fonts/noto",
        "/usr/share/fonts/chromeos/noto",
    ] {
        v.push((format!("{d}/NotoSansSymbols2-Regular.ttf"), 0));
    }
    v
}

/// Whether any symbol fallback face is installed (tests skip their glyph checks without one).
pub fn symbol_face_installed() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        symbols().iter().chain(symbols_more().iter()).any(|(p, _)| std::path::Path::new(p).exists())
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
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
        v.selection.bg_fill = t.accent_soft;
        v.selection.stroke = Stroke::new(1.0, t.accent);
        v.hyperlink_color = t.accent;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.active.weak_bg_fill = t.pressed;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
        v.window_corner_radius = egui::CornerRadius::same(10);
        v.menu_corner_radius = egui::CornerRadius::same(8);
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
        ctx.set_visuals_of(theme, v);
    }
    ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Button, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Small, ui_font(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, ui_bold(18.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::new(12.0, FontFamily::Name(MONO.into())));
    });
}

/// Model colour → egui colour.
pub fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod font_binding_tests {
    use super::*;
    use crate::i18n::Language;

    /// The families [`font_definitions`] always binds, whatever is installed.
    fn always_bound() -> Vec<String> {
        [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_name_that_is_not_an_installed_font_gets_a_built_in_role() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Nothing installed: every name buckets to a role that font_definitions bound.
        crate::fonts::set_bound(always_bound());
        for name in ["", "Calibri", "Courier New", "Times New Roman", "Grossly Missing Font", "\u{1F600}"] {
            for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
                let FontFamily::Name(fam) = cell_family(name, b, i) else { panic!("{name:?} should name a family") };
                assert!(crate::fonts::is_bound(&fam), "{name:?} {b} {i} → unbound {fam:?}");
            }
        }
    }

    #[test]
    fn a_bound_installed_font_is_used_by_name() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let role = crate::fonts::role_key("Ubuntu", true, false);
        crate::fonts::set_bound(always_bound().into_iter().chain([role.clone()]));
        assert!(matches!(cell_family("Ubuntu", true, false), FontFamily::Name(f) if *f == *role));
        // The same family in a style that wasn't bound falls back to a built-in role that is.
        let FontFamily::Name(fallback) = cell_family("Ubuntu", false, true) else { panic!("should name a family") };
        assert!(crate::fonts::is_bound(&fallback), "the fallback is a bound role");
        crate::fonts::set_bound(always_bound());
    }

    /// A standard workbook font this machine doesn't have still binds four roles, under the name the
    /// workbook uses, to the substitute the desktop picks for it (Times New Roman → Liberation
    /// Serif). Without that, SERIF and MONO carry one regular-weight chain and the cell loses bold
    /// and italic entirely (#1231).
    #[cfg(target_os = "linux")]
    #[test]
    fn an_uninstalled_standard_font_binds_its_substitute_in_all_four_roles() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::fonts::catalog(); // the substitute is looked up in the scanned catalog
        if crate::fonts::substitute_family("Times New Roman").is_none() {
            return; // installed already, or no fontconfig and no folder-scan substitute
        }
        let (fonts, bound) = font_definitions_with_bound(Language::En, &["Times New Roman".to_string()], ScriptSet::default());
        crate::fonts::set_bound(bound.clone());
        let face = |bold: bool, italic: bool| -> String {
            let family = cell_family("Times New Roman", bold, italic);
            fonts.families.get(&family).and_then(|chain| chain.first()).cloned().unwrap_or_default()
        };
        let regular = face(false, false);
        assert!(!regular.is_empty(), "the regular role has a face");
        assert_ne!(face(true, false), regular, "bold is a different face, not the regular one");
        assert_ne!(face(false, true), regular, "italic is a different face, not the regular one");
        for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
            assert!(bound.contains(&crate::fonts::role_key("Times New Roman", b, i)), "role {b} {i} is bound");
        }
        crate::fonts::set_bound(always_bound());
    }

    #[test]
    fn arbitrary_names_never_yield_an_unbound_family() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A workbook can hold any font name; none may name a family egui has no data for.
        crate::fonts::set_bound(always_bound());
        for seed in 0..500u32 {
            let name: String =
                (0..(seed % 40)).map(|k| char::from_u32(0x20 + (seed.wrapping_mul(31).wrapping_add(k) % 0x5F)).unwrap_or('x')).collect();
            let FontFamily::Name(fam) = cell_family(&name, seed % 2 == 0, seed % 3 == 0) else { continue };
            assert!(crate::fonts::is_bound(&fam), "{name:?} → unbound {fam:?}");
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn fallback_faces_are_appended_to_every_family() {
        // Structural: whatever loaded, the non-Latin fallback keys must sit at the end of each chain
        // (after the Latin/default fonts) so Latin still wins for Latin text, in the language's order.
        for han in [HanOrder::Chinese, HanOrder::Japanese] {
            let language = if han == HanOrder::Japanese { Language::Ja } else { Language::Zh };
            let fonts = font_definitions_for_language(language);
            let fb_keys: Vec<String> =
                fallback_faces(han).into_iter().map(|f| f.key.to_string()).filter(|k| fonts.font_data.contains_key(k)).collect();
            if fb_keys.is_empty() {
                return; // no fallback face on this machine (minimal Linux)
            }
            for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
                let chain = fonts.families.get(&FontFamily::Name(role.into())).map(Vec::as_slice).unwrap_or_default();
                assert!(chain.len() >= fb_keys.len(), "{role} chain too short for its fallback");
                let tail = &chain[chain.len() - fb_keys.len()..];
                assert_eq!(tail, fb_keys.as_slice(), "{role} must end its chain with the fallback faces");
            }
        }
    }

    #[test]
    fn serif_family_names_route_to_the_serif_role() {
        for name in ["Times New Roman", "Georgia", "Cambria", "Garamond", "Noto Serif", "DejaVu Serif"] {
            assert!(family_name_is_serif(name), "{name}");
        }
        for name in ["Arial", "Calibri", "DejaVu Sans", "Noto Sans", "Source Sans 3"] {
            assert!(!family_name_is_serif(name), "{name}");
        }
    }

    #[test]
    fn interface_families_fall_back_to_the_cell_font() {
        // Structural (#349): the formula bar, name box and dialogs draw every script the cells can,
        // so each interface chain carries the cell face, ahead of the script fallback faces.
        let fonts = font_definitions_for_language(Language::En);
        let cell = format!("sys-{CELL}");
        if !fonts.font_data.contains_key(&cell) {
            return; // no system cell font on this machine
        }
        let fb: Vec<String> = fallback_faces(HanOrder::Chinese).into_iter().map(|f| f.key.to_string()).collect();
        for family in [FontFamily::Proportional, FontFamily::Name(UI.into()), FontFamily::Name(UI_BOLD.into()), FontFamily::Name(MONO.into())] {
            let chain = fonts.families.get(&family).cloned().unwrap_or_default();
            let at = chain.iter().position(|k| *k == cell);
            assert!(at.is_some(), "{family:?} lacks the cell face: {chain:?}");
            let first_fb = chain.iter().position(|k| fb.contains(k)).unwrap_or(chain.len());
            assert!(at.unwrap_or(usize::MAX) < first_fb, "{family:?}: cell face must precede the script faces");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_latin_sheet_skips_the_cjk_and_thai_faces() {
        let (fonts, _) = font_definitions_with_bound(Language::En, &[], ScriptSet::of_text("Total 42"));
        assert!(!fonts.font_data.keys().any(|k| k.starts_with("sys-cjk")), "Latin text loads no CJK face");
        assert!(!fonts.font_data.keys().any(|k| k.starts_with("sys-thai")), "Latin text loads no Thai face");
    }

    #[test]
    fn a_wanted_script_gets_a_face_from_the_chains_or_the_catalog() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(catalog) = crate::catalog::FontCatalog::installed() else { return };
        if !catalog.covers(ScriptClass::Thai) {
            return; // no Thai face to find on this machine
        }
        let (fonts, _) = font_definitions_with_bound(Language::En, &[], ScriptSet::of_text("สวัสดี"));
        assert!(
            fonts.font_data.keys().any(|k| k == "sys-thai" || k == "sys-script-thai"),
            "a Thai workbook needs a Thai face: {:?}",
            fonts.font_data.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_bengali_or_ethiopic_sheet_asks_for_its_own_family() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(catalog) = crate::catalog::FontCatalog::installed() else { return };
        for (text, script, chain) in [("ঢাকা", ScriptClass::Bengali, "sys-script-bengali"), ("አማርኛ", ScriptClass::Ethiopic, "sys-script-ethiopic")]
        {
            if !catalog.covers(script) {
                continue; // this machine has no face for the script
            }
            let (fonts, _) = font_definitions_with_bound(Language::En, &[], ScriptSet::of_text(text));
            assert!(
                fonts.font_data.keys().any(|k| k == chain),
                "{script:?} needs a face from the catalog: {:?}",
                fonts.font_data.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn only_the_scripts_in_use_load_their_fallbacks() {
        // Nothing known yet: every face, as before.
        assert_eq!(wanted_scripts(Language::En, ScriptSet::EMPTY), ScriptSet::all());
        let latin = wanted_scripts(Language::En, ScriptSet::of_text("Total"));
        assert!(latin.has(ScriptClass::Latin) && latin.has(ScriptClass::Symbols));
        assert!(!latin.has(ScriptClass::Han) && !latin.has(ScriptClass::Thai), "no CJK or Thai face for a Latin sheet");
        // A Chinese, Japanese or Korean interface needs its script even when the cells are Latin.
        assert!(wanted_scripts(Language::Zh, latin).has(ScriptClass::Han));
        assert!(wanted_scripts(Language::Ja, latin).has(ScriptClass::Japanese));
        assert!(wanted_scripts(Language::Ko, latin).has(ScriptClass::Korean));
        assert!(!wanted_scripts(Language::En, latin).has(ScriptClass::Han), "an English interface adds no script");
        assert!(wanted_scripts(Language::En, ScriptSet::of_text("สวัสดี")).has(ScriptClass::Thai));
    }

    #[test]
    fn language_reorders_only_the_han_faces() {
        let keys = |han| fallback_faces(han).into_iter().map(|f| f.key).collect::<Vec<_>>();
        assert_eq!(
            keys(HanOrder::Japanese),
            [
                "sys-cjk-han-ja",
                "sys-cjk-han-zh",
                "sys-cjk-hangul",
                "sys-thai",
                "sys-lao",
                "sys-khmer",
                "sys-hebrew",
                "sys-arabic",
                "sys-devanagari",
                "sys-myanmar",
                "sys-tibetan",
                "sys-yi",
                "sys-symbols",
                "sys-symbols-2"
            ]
        );
        assert_eq!(
            keys(HanOrder::Chinese),
            [
                "sys-cjk-han-zh",
                "sys-cjk-han-ja",
                "sys-cjk-hangul",
                "sys-thai",
                "sys-lao",
                "sys-khmer",
                "sys-hebrew",
                "sys-arabic",
                "sys-devanagari",
                "sys-myanmar",
                "sys-tibetan",
                "sys-yi",
                "sys-symbols",
                "sys-symbols-2"
            ]
        );
        assert_eq!(HanOrder::of(Language::Ja), HanOrder::Japanese);
        for l in [Language::En, Language::Zh, Language::Ko, Language::Ru, Language::PtBr, Language::De, Language::Cs, Language::Fr, Language::Tr] {
            assert_eq!(HanOrder::of(l), HanOrder::Chinese, "{l:?} shares the default order (no font rebuild)");
        }
    }

    #[test]
    fn serif_fallbacks_are_a_separate_set() {
        let faces = fallback_faces(HanOrder::Chinese);
        for face in &faces {
            if let Some((key, _)) = &face.serif {
                assert_ne!(*key, face.key, "serif key must differ from the sans key");
            }
        }
        let mut keys: Vec<&str> = faces.iter().map(|f| f.key).collect();
        keys.extend(faces.iter().filter_map(|f| f.serif.as_ref().map(|(k, _)| *k)));
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys.len(), sorted.len(), "fallback keys must be unique");
    }

    #[test]
    fn serif_workbook_fonts_load_the_serif_fallbacks() {
        let (fonts, _) = font_definitions_with_bound(Language::En, &["Times New Roman".into()], ScriptSet::default());
        if !cjk_han_simplified_serif().iter().any(|(p, _)| std::path::Path::new(p).is_file()) {
            return; // no serif CJK face on this machine
        }
        assert!(fonts.font_data.contains_key("sys-cjk-han-zh-serif"), "serif CJK face was not loaded");
        let chain = fonts.families.get(&FontFamily::Name(SERIF.into())).cloned().unwrap_or_default();
        assert!(
            chain.iter().any(|k| k == "sys-cjk-han-zh-serif" || k == "sys-cjk-han-ja-serif"),
            "serif role must carry the serif fallbacks: {chain:?}"
        );
    }

    #[test]
    fn sans_workbook_fonts_skip_the_serif_fallbacks() {
        // Noto Serif CJK is ~55 MB; a workbook that asks for no serif face must not load it.
        let (fonts, _) = font_definitions_with_bound(Language::En, &["Arial".into()], ScriptSet::default());
        let serif_keys = ["sys-cjk-han-zh-serif", "sys-cjk-han-ja-serif", "sys-thai-serif"];
        assert!(!serif_keys.iter().any(|k| fonts.font_data.contains_key(*k)), "sans-only workbook loaded serif faces");
    }

    #[test]
    fn cjk_fallback_probes_flatpak_fonts_with_the_correct_locale_faces() {
        // Exercise the candidates the fallback loader actually consumes, not just the directory
        // constant. This must work on CI without an installed CJK font or a Flatpak runtime.
        for (candidates, face) in [(cjk_han_japanese(), 0), (cjk_han_simplified(), 2), (cjk_hangul(), 1)] {
            for directory in [
                "/run/host/fonts/opentype/noto",
                "/run/host/fonts/google-noto-cjk",
                "/run/host/fonts/noto-cjk",
                "/run/host/fonts/truetype/noto",
                "/run/host/fonts/noto",
                "/run/host/fonts/truetype",
                "/run/host/fonts/TTF",
                "/run/host/fonts",
                "/run/host/local-fonts",
                "/run/host/local-fonts/noto-cjk",
                "/run/host/user-fonts",
            ] {
                let path = format!("{directory}/NotoSansCJK-Regular.ttc");
                assert!(candidates.contains(&(path.clone(), face)), "missing Flatpak CJK candidate {path}, face {face}");
            }
        }
        let chinese = cjk_han_simplified();
        assert!(chinese.contains(&("/run/host/fonts/truetype/wqy/wqy-microhei.ttc".into(), 0)));
        assert!(chinese.contains(&("/run/host/fonts/wenquanyi/wqy-microhei.ttc".into(), 0)));
    }

    #[test]
    fn fallback_faces_load_each_file_once() {
        // The region-merged Noto Sans CJK .ttc serves Japanese, Chinese and Hangul; it must be loaded
        // once, not once per script (tens of MB each).
        for language in [Language::En, Language::Ja] {
            let fonts = font_definitions_for_language(language);
            let present: Vec<&Arc<FontData>> =
                fallback_faces(HanOrder::of(language)).into_iter().filter_map(|f| fonts.font_data.get(f.key)).collect();
            for (i, a) in present.iter().enumerate() {
                for b in present.iter().skip(i + 1) {
                    assert!(a.font != b.font, "the same font file is loaded for two fallback scripts");
                }
            }
        }
    }

    #[test]
    fn fallback_covers_its_script() {
        // Real render check, per script: the proportional family (what cells and the UI use) must
        // resolve a sample string to a real glyph, not the tofu replacement char. Skips a script
        // whose face is absent (a minimal machine) and fails if a face is present but not wired in.
        let cases: [(&str, Vec<(String, u32)>, &str, Option<&str>); 8] = [
            ("sys-cjk-han-ja", cjk_han_japanese(), "カテゴリ", Some("ja")),
            ("sys-cjk-han-zh", cjk_han_simplified(), "页面布局 客户端配置", Some("zh-cn")),
            ("sys-cjk-hangul", cjk_hangul(), "한글", Some("ko")),
            ("sys-thai", thai(), "ทดสอบ", Some("th")),
            ("sys-lao", lao(), "ພາສາລາວ", Some("lo")),
            ("sys-hebrew", hebrew(), "עברית", Some("he")),
            ("ui", candidates(UI), "Русский", None),
            // Czech interface labels: every Czech accented letter, in both cases.
            ("ui", candidates(UI), "Příliš žluťoučký kůň úpěl ďábelské ódy ŘŠČŽÝÁÍÉÚŮĚŤĎŇÓ", None),
        ];
        let has_language = |lang: &str| {
            let pattern = format!(":lang={lang}");
            std::process::Command::new("fc-list")
                .args(["-f", "%{file}\n", &pattern])
                .output()
                .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
        };
        for language in [Language::En, Language::Ja] {
            let ctx = egui::Context::default();
            ctx.set_fonts(font_definitions_for_language(language));
            let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
            out.textures_delta.clear(); // nothing consumes the atlas in a headless test
            // `has_glyphs` is a false negative when the face that owns the script is also the face
            // that draws the replacement glyph (DejaVu Sans as the UI font owns Hebrew), so compare
            // the laid out glyphs with the tofu glyph itself instead.
            for family in [FontFamily::Proportional, FontFamily::Name(CELL.into())] {
                let font = FontId::new(14.0, family);
                let glyphs = |text: &str| {
                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::BLACK));
                    galley.rows.iter().flat_map(|r| r.glyphs.iter().filter(|g| g.chr != ' ').map(|g| g.uv_rect)).collect::<Vec<_>>()
                };
                let tofu = glyphs("\u{25FB}");
                for (key, cands, sample, lang) in &cases {
                    if let Some(lang) = *lang
                        && !has_language(lang)
                    {
                        continue; // fontconfig has no face for this language on this machine
                    }
                    if !cands.iter().any(|(p, _)| std::path::Path::new(p).exists()) {
                        continue; // no face for this script on this machine; nothing to assert
                    }
                    let drawn = glyphs(sample);
                    assert!(
                        !drawn.is_empty() && drawn.iter().all(|g| !tofu.contains(g)),
                        "{key} is present but {sample:?} still renders as tofu ({language:?}, {:?})",
                        font.family
                    );
                }
            }
        }
    }

    #[test]
    fn chromeos_font_layout_is_probed() {
        // #1762: Crostini mounts the host's fonts under /usr/share/fonts/chromeos, one folder per
        // family set; a script whose list misses it renders as tofu with the font installed.
        let probes = |cands: Vec<(String, u32)>, path: &str| cands.iter().any(|(p, _)| p == path);
        let cros = "/usr/share/fonts/chromeos";
        assert!(probes(thai(), &format!("{cros}/noto/NotoSansThai-Regular.ttf")));
        assert!(probes(hebrew(), &format!("{cros}/noto/NotoSansHebrew-Regular.ttf")));
        assert!(probes(cjk_han_japanese(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
        assert!(probes(cjk_han_simplified(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
        assert!(probes(cjk_hangul(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
        assert!(probes(symbols_more(), &format!("{cros}/noto/NotoSansSymbols2-Regular.ttf")));
        assert!(probes(candidates(UI), &format!("{cros}/noto/NotoSans-Regular.ttf")));
        assert!(probes(candidates(CELL), &format!("{cros}/crosextra/Carlito-Regular.ttf")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fc_index_parts_splits_named_instance() {
        // fontconfig's index packs the 1-based named instance in the high 16 bits and the face in
        // the low 16: a plain face 2 is 2, Noto Serif CJK's Regular instance on face 2 is 196610,
        // and Noto Sans' Bold instance 3 is 262144.
        assert_eq!(fc_index_parts(2), (2, None));
        assert_eq!(fc_index_parts(196610), (2, Some(2)));
        assert_eq!(fc_index_parts(262144), (0, Some(3)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bold_match_keeps_its_named_instance_weight() {
        // #1231: a variable font matched as bold must render bold. Masking the index to the face
        // dropped the named instance, so egui drew the default (often regular or thin) cut. Noto
        // Sans ships as a variable font (Noto Sans CJK's bold cut is a separate static file); skip
        // when no variable bold face is installed.
        let mut chosen = None;
        for pattern in ["Noto Sans:weight=bold", "sans-serif:weight=bold"] {
            let Some(answer) = fc_match(pattern) else { continue };
            if fc_index_parts(answer.1).1.is_some() {
                chosen = Some(answer);
                break;
            }
        }
        let Some((path, raw)) = chosen else { return };
        let Some(fd) = try_load(&[(path.as_str(), raw)]) else { return };
        let wght = fd.tweak.coords.as_ref().iter().find(|(tag, _)| tag.to_be_bytes() == *b"wght").map(|(_, value)| *value);
        assert!(
            wght.is_some_and(|value| value >= 600.0),
            "fontconfig named a bold instance of {path} (index {raw}) but no wght >= 600 was applied: {wght:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_named_instance_is_ignored() {
        // A stale or malformed index must not crash or reach egui: the default instance is used.
        let Some((path, _)) = fc_match("sans-serif") else { return };
        let Some(fd) = try_load(&[(path.as_str(), 0xFFFF_0000)]) else { return };
        assert!(fd.tweak.coords.as_ref().is_empty(), "an out-of-range named instance must leave the default coords");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fontconfig_candidates_are_ordered_and_deduplicated() {
        // No CJK needed: any fontconfig installation can answer `sans-serif`.
        // The first match must stay first, and the same match must not be added twice. The index
        // keeps fontconfig's named-instance bits; `try_load` splits them.
        let Some(expected) = fc_match("sans-serif") else { return };
        let candidates = fc_match_candidates(&["sans-serif", "sans-serif"]);
        assert_eq!(candidates.first(), Some(&expected));
        assert_eq!(candidates.len(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fontconfig_serif_match_precedes_hard_coded_paths() {
        // The hard-coded paths remain as a fallback, but a fontconfig answer must win first.
        let Some((path, index)) = fc_match("Liberation Serif") else { return };
        let candidates = candidates(SERIF);
        assert!(candidates.first().is_some_and(|(p, i)| p == &path && *i == index));
        assert!(candidates.iter().any(|(p, _)| p == "/usr/share/fonts/liberation/LiberationSerif-Regular.ttf"));
    }

    #[test]
    fn unparseable_font_files_are_rejected_not_fatal() {
        // #226: epaint panics in `set_fonts` on a face it can't parse ("An offset was out of
        // bounds"). Garbage, a truncated real font and a bad collection index must all be refused.
        let real = FontDefinitions::default().font_data.values().next().map(|fd| fd.font.to_vec()).unwrap_or_default();
        assert!(font_parses(&real, 0), "egui's own font must parse");
        assert!(!font_parses(b"", 0));
        assert!(!font_parses(&[0xAB; 64], 0));
        assert!(!font_parses(real.get(..20).unwrap_or_default(), 0), "a truncated font must be refused");
        assert!(!font_parses(&real, 3), "a single-face file has no face 3");

        // A bad face that slipped into the definitions is dropped (key and family entries), and the
        // context builds its fonts without panicking.
        let mut fonts = font_definitions();
        fonts.font_data.insert("sys-hebrew".into(), Arc::new(FontData::from_owned(vec![0, 1, 0, 0, 0, 0xFF, 0, 0])));
        for chain in fonts.families.values_mut() {
            chain.push("sys-hebrew".into());
        }
        drop_unparseable_fonts(&mut fonts);
        assert!(!fonts.font_data.contains_key("sys-hebrew"));
        assert!(fonts.families.values().all(|c| !c.iter().any(|k| k == "sys-hebrew")));
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("Hebrew עברית");
        });
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn try_load_skips_a_broken_candidate() {
        // A candidate that exists but is not a readable font must not win over a good one behind it.
        let dir = std::env::temp_dir().join(format!("gridcraft-font-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let bad = dir.join("broken.ttf");
        let good = dir.join("good.ttf");
        let real = FontDefinitions::default().font_data.values().next().map(|fd| fd.font.to_vec()).unwrap_or_default();
        let _ = std::fs::write(&bad, real.get(..40).unwrap_or_default());
        let _ = std::fs::write(&good, &real);
        let (bad_s, good_s) = (bad.to_string_lossy().into_owned(), good.to_string_lossy().into_owned());
        assert!(try_load(&[(bad_s.as_str(), 0)]).is_none());
        let loaded = try_load(&[(bad_s.as_str(), 0), (good_s.as_str(), 0)]);
        assert_eq!(loaded.map(|fd| fd.font.len()), Some(real.len()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn interface_fonts_are_read_once_and_shared_with_egui() {
        // egui copies owned font bytes into its text engine and keeps the original too; borrowed
        // bytes it shares. A rebuild of the font set reuses the file read the first time.
        let path = std::env::temp_dir().join(format!("gridcraft-font-shared-{}.ttf", std::process::id()));
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        let p = path.to_string_lossy().into_owned();
        let first = try_load(&[(p.as_str(), 0)]).unwrap();
        std::fs::remove_file(&path).unwrap();
        let second = try_load(&[(p.as_str(), 0)]).expect("kept from the first read");
        let (std::borrow::Cow::Borrowed(a), std::borrow::Cow::Borrowed(b)) = (&first.font, &second.font) else {
            panic!("interface font bytes must be borrowed, so egui shares them instead of copying");
        };
        assert!(std::ptr::eq(*a, *b));
        assert_eq!(*a, epaint_default_fonts::UBUNTU_LIGHT);
        assert!(try_load(&[(p.as_str(), 3)]).is_none(), "a face the kept file doesn't have is still refused");
    }

    #[test]
    fn macos_bold_interface_face_is_drawn_unhinted() {
        // #1184: Helvetica Neue Bold's own TrueType hints close Cyrillic е into ө at 13 pt on a
        // Retina screen. Core Text never runs them, so on macOS the face goes unhinted like every
        // other app draws it; the regular face and the other platforms keep egui's default.
        let fonts = font_definitions_for_language(Language::En);
        let hinting = |key: &str| fonts.font_data.get(key).map(|fd| fd.tweak.hinting);
        if let Some(bold) = hinting(&format!("sys-{UI_BOLD}")) {
            assert_eq!(bold, if cfg!(target_os = "macos") { Some(false) } else { None });
        }
        if let Some(regular) = hinting(&format!("sys-{UI}")) {
            assert_eq!(regular, None, "only the bold face is affected");
        }
    }

    #[test]
    fn fedora_package_folders_are_searched_after_the_other_linux_layouts() {
        // #1694: Fedora keeps each font family in its own package folder, so without these entries
        // no cell face is found there and bold and italic cells draw as regular text. Each comes
        // after the existing Linux folders for the same font, so wherever one of those matched,
        // nothing changes.
        let paths = |cands: Vec<(String, u32)>| cands.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        let cases: [(&str, Vec<String>, &str, &str); 9] = [
            (CELL, paths(candidates(CELL)), "google-carlito-fonts/Carlito-Regular.ttf", "/carlito/Carlito-Regular.ttf"),
            (CELL_BOLD, paths(candidates(CELL_BOLD)), "google-carlito-fonts/Carlito-Bold.ttf", "/carlito/Carlito-Bold.ttf"),
            (CELL_ITALIC, paths(candidates(CELL_ITALIC)), "google-carlito-fonts/Carlito-Italic.ttf", "/carlito/Carlito-Italic.ttf"),
            (CELL_BOLD_ITALIC, paths(candidates(CELL_BOLD_ITALIC)), "google-carlito-fonts/Carlito-BoldItalic.ttf", "/carlito/Carlito-BoldItalic.ttf"),
            (CELL_BOLD, paths(candidates(CELL_BOLD)), "liberation-sans-fonts/LiberationSans-Bold.ttf", "/liberation/LiberationSans-Bold.ttf"),
            (SERIF, paths(candidates(SERIF)), "liberation-serif-fonts/LiberationSerif-Regular.ttf", "/liberation/LiberationSerif-Regular.ttf"),
            (MONO, paths(candidates(MONO)), "liberation-mono-fonts/LiberationMono-Regular.ttf", "/dejavu/DejaVuSansMono.ttf"),
            ("thai", paths(thai()), "google-noto-vf/NotoSansThai[wght].ttf", "/NotoSansThai-Regular.ttf"),
            ("hebrew", paths(hebrew()), "google-noto-vf/NotoSansHebrew[wght].ttf", "/NotoSansHebrew-Regular.ttf"),
        ];
        for (role, list, fedora_file, earlier) in &cases {
            // fontconfig candidates come first and may repeat a path; compare the last occurrence,
            // which is the hard-coded fallback this test is about.
            let at = list.iter().rposition(|p| *p == format!("/usr/share/fonts/{fedora_file}"));
            let last_earlier = list.iter().rposition(|p| p.ends_with(earlier));
            assert!(at.is_some(), "{role}: Fedora's {fedora_file} is not a candidate");
            assert!(last_earlier.is_some(), "{role}: no existing {earlier} candidate to compare with");
            assert!(at > last_earlier, "{role}: {fedora_file} must come after every existing {earlier}");
        }
        // Liberation Mono only stands in for DejaVu Sans Mono, wherever that is installed.
        let mono = paths(candidates(MONO));
        let liberation = mono.iter().rposition(|p| p.ends_with("LiberationMono-Regular.ttf"));
        let last_dejavu = mono.iter().rposition(|p| p.ends_with("DejaVuSansMono.ttf"));
        assert!(liberation.is_some() && liberation > last_dejavu, "Liberation Mono must follow DejaVu Sans Mono: {mono:?}");
        let noto = mono.iter().rposition(|p| p == "/usr/share/fonts/google-noto-vf/NotoSansMono[wght].ttf");
        assert!(noto.is_some() && noto.unwrap_or(0) + 1 == mono.len(), "Fedora's default Noto Sans Mono must be the last resort: {mono:?}");
    }
}
