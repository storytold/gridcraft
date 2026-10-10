//! Design tokens and fonts.
//!
//! Colours are our own, tuned to read like a modern desktop spreadsheet. Fonts come from the
//! operating system at runtime (nothing bundled): the UI uses the platform UI font, cells use the
//! best available match for the workbook font (Calibri/Carlito/Aptos/Arial…) with egui's built-in
//! fonts as the last resort.

use std::sync::Arc;

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
            menu_bg: Color32::WHITE,
            shadow: Color32::from_black_alpha(40),
            danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),
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

/// Font file bytes as egui font data, or `None` when they are not a TrueType/OpenType font (or a
/// collection with face `index`). epaint panics on font data it cannot parse, and font files are
/// untrusted: system files, and on the web whatever the server answered.
pub fn font_data(bytes: Vec<u8>, index: u32) -> Option<FontData> {
    skrifa::FontRef::from_index(&bytes, index).ok()?;
    let mut fd = FontData::from_owned(bytes);
    fd.index = index;
    Some(fd)
}

#[cfg(not(target_arch = "wasm32"))]
fn try_load(paths: &[(&str, u32)], loaded: &mut Vec<(String, u32, Arc<FontData>)>) -> Option<Arc<FontData>> {
    for (p, index) in paths {
        if let Some((_, _, data)) = loaded.iter().find(|(path, face, _)| path.as_str() == *p && face == index) {
            return Some(Arc::clone(data));
        }
        if let Some(fd) = std::fs::read(p).ok().and_then(|bytes| font_data(bytes, *index)) {
            let data = Arc::new(fd);
            loaded.push(((*p).to_owned(), *index, Arc::clone(&data)));
            return Some(data);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn try_load(_paths: &[(&str, u32)], _loaded: &mut Vec<(String, u32, Arc<FontData>)>) -> Option<Arc<FontData>> {
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

/// A font from the optional craft-fonts build input (empty unless built with `CRAFT_FONTS_DIR`).
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    /// ISO 15924 scripts the font is for, e.g. `"Jpan"`.
    pub scripts: &'static [&'static str],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// Whether `c` belongs to a script that needs a CJK font (Han, kana, Hangul, full-width forms).
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x303F | 0x3040..=0x30FF | 0x3100..=0x318F | 0x31A0..=0x31FF
        | 0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// Whether `s` contains a character that needs a CJK font.
pub fn has_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// Whether the interface language itself is written with CJK characters.
pub fn is_cjk_language(tag: &str) -> bool {
    tag.split('-').next().is_some_and(|language| ["ja", "ko", "zh"].contains(&language))
}

/// Han glyph shapes used by the interface. Non-CJK languages share the widest Chinese coverage;
/// switching between them does not rebuild the font atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HanOrder {
    Japanese,
    Chinese,
    TraditionalChinese,
    Korean,
}

impl HanOrder {
    pub fn of(tag: &str) -> Self {
        match tag.split('-').next() {
            Some("ja") => Self::Japanese,
            Some("ko") => Self::Korean,
            Some("zh") => {
                // An explicit script wins over the region, just as locale negotiation does.
                if tag.split('-').any(|part| part == "Hant") {
                    Self::TraditionalChinese
                } else if tag.split('-').any(|part| part == "Hans") {
                    Self::Chinese
                } else if tag.split('-').any(|part| ["TW", "HK", "MO"].contains(&part)) {
                    Self::TraditionalChinese
                } else {
                    Self::Chinese
                }
            }
            _ => Self::Chinese,
        }
    }

    fn script(self) -> &'static str {
        match self {
            Self::Japanese => "Jpan",
            Self::Chinese => "Hans",
            Self::TraditionalChinese => "Hant",
            Self::Korean => "Kore",
        }
    }
}

/// File below the deployment's `fonts/` directory, downloaded on demand. Fonts are never part
/// of this repository; the host reports a failed download if the deployment does not ship it.
pub fn web_cjk_font(tag: &str) -> &'static str {
    match HanOrder::of(tag) {
        HanOrder::Japanese => "cjk-ja.ttf",
        HanOrder::Chinese => "cjk-zh-CN.ttf",
        HanOrder::TraditionalChinese => "cjk-zh-TW.ttf",
        HanOrder::Korean => "cjk-ko.ttf",
    }
}

/// Split Noto Sans CJK files, in the same order as the collection's regional glyph shapes.
#[cfg(not(target_arch = "wasm32"))]
fn noto_otf_order(han: HanOrder) -> [&'static str; 4] {
    let (jp, sc, kr, tc) = ("NotoSansCJKjp-Regular.otf", "NotoSansCJKsc-Regular.otf", "NotoSansCJKkr-Regular.otf", "NotoSansCJKtc-Regular.otf");
    match han {
        HanOrder::Japanese => [jp, sc, kr, tc],
        HanOrder::Chinese => [sc, tc, jp, kr],
        HanOrder::TraditionalChinese => [tc, sc, jp, kr],
        HanOrder::Korean => [kr, jp, sc, tc],
    }
}

/// What the font set has to cover beyond Latin text.
#[derive(Clone, Copy, Debug, Default)]
pub struct FontNeeds<'a> {
    /// Interface language tag (`ja-JP`); picks the CJK face whose Han glyph shapes match.
    pub language: &'a str,
    /// The interface or a workbook uses CJK characters: add the CJK fallback chain.
    pub cjk: bool,
    /// Font files the host delivered (web build: `fonts/<name>` downloaded on demand), each once.
    pub downloaded: &'a [(String, Arc<FontData>)],
}

/// Only CJK coverage with its Han glyph order, and the delivered font count, change the installed
/// font set: equivalent tags (`zh-HK`, `zh-TW`) never rebuild it.
pub type FontKey = (Option<HanOrder>, usize);

impl FontNeeds<'_> {
    pub fn key(&self) -> FontKey {
        (self.cjk.then(|| HanOrder::of(self.language)), self.downloaded.len())
    }
}

/// Builds every family with egui's defaults and non-Latin fallback faces after its own fonts.
/// Cyrillic, Thai and Hebrew system/craft faces are always available. With `needs.cjk`, regional
/// craft-fonts (`CRAFT_FONTS_DIR`), web downloads and system CJK faces extend the chain as well.
pub fn font_definitions(needs: &FontNeeds) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let base_prop: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut fallback: Vec<String> = Vec::new();
    // Share a system file used by a role and a script fallback instead of reading/copying it twice.
    let mut system_files = Vec::new();
    let han = HanOrder::of(needs.language);
    let system_fallback = load_fallback(&mut fonts, han, needs.cjk, &mut system_files);
    let add_craft = |fonts: &mut FontDefinitions, chain: &mut Vec<String>, f: &CraftFont| {
        let key = format!("craft-{}-{}", f.family, f.style);
        if !fonts.font_data.contains_key(&key) {
            fonts.font_data.insert(key.clone(), Arc::new(FontData::from_static(f.bytes)));
            chain.push(key);
        }
    };
    if needs.cjk {
        let script = han.script();
        // Preferred craft fonts and downloads precede system faces. Japanese craft fonts back up
        // the regional system chain, never override the interface's Chinese or Korean glyph shapes.
        for f in CRAFT_FONTS.iter().filter(|f| f.scripts.contains(&script)) {
            add_craft(&mut fonts, &mut fallback, f);
        }
        let preferred = web_cjk_font(needs.language);
        let mut downloaded: Vec<&(String, Arc<FontData>)> = needs.downloaded.iter().collect();
        downloaded.sort_by_key(|(name, _)| name != preferred);
        for (name, data) in downloaded {
            let key = format!("downloaded-{name}");
            if !fonts.font_data.contains_key(&key) {
                fonts.font_data.insert(key.clone(), data.clone());
                fallback.push(key);
            }
        }
        fallback.extend(system_fallback.iter().filter(|key| key.starts_with("sys-cjk-")).cloned());
        if script != "Jpan" {
            for f in CRAFT_FONTS.iter().filter(|f| f.scripts.contains(&"Jpan") && !f.scripts.contains(&script)) {
                add_craft(&mut fonts, &mut fallback, f);
            }
        }
        // Last resort, other regional faces: Hangul in a workbook under a Japanese interface.
        for f in CRAFT_FONTS.iter().filter(|f| f.scripts.iter().any(|s| ["Hans", "Hant", "Kore"].contains(s))) {
            add_craft(&mut fonts, &mut fallback, f);
        }
    }
    for f in CRAFT_FONTS.iter().filter(|f| f.scripts.iter().any(|script| ["Cyrl", "Thai", "Hebr"].contains(script))) {
        add_craft(&mut fonts, &mut fallback, f);
    }
    fallback.extend(system_fallback.into_iter().filter(|key| !key.starts_with("sys-cjk-")));
    for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
        let paths = candidates(role);
        let refs: Vec<(&str, u32)> = paths.iter().map(|(p, i)| (p.as_str(), *i)).collect();
        let mut chain: Vec<String> = Vec::new();
        if let Some(fd) = try_load(&refs, &mut system_files) {
            let key = format!("sys-{role}");
            fonts.font_data.insert(key.clone(), fd);
            chain.push(key);
        } else if role == UI_BOLD {
            chain.push(format!("sys-{UI}"));
        } else if role.starts_with("cell-") {
            chain.push(format!("sys-{CELL}"));
        }
        chain.extend(if role == MONO { base_mono.clone() } else { base_prop.clone() });
        chain.extend(fallback.iter().cloned());
        chain.retain(|k| fonts.font_data.contains_key(k));
        fonts.families.insert(FontFamily::Name(role.into()), chain);
    }
    // Default proportional text in widgets uses the UI font.
    if let Some(ui) = fonts.families.get(&FontFamily::Name(UI.into())).cloned() {
        fonts.families.insert(FontFamily::Proportional, ui);
    }
    if let Some(mono) = fonts.families.get(&FontFamily::Name(MONO.into())).cloned() {
        fonts.families.insert(FontFamily::Monospace, mono);
    }
    fonts
}

/// Loads system script fallback faces. A regional CJK collection or a multi-script face such as
/// Tahoma serves several scripts without duplicating its bytes. Without a system or craft face,
/// glyph coverage is limited to egui's bundled fonts; no font assets are bundled by this module.
#[cfg(not(target_arch = "wasm32"))]
fn load_fallback(fonts: &mut FontDefinitions, han: HanOrder, cjk: bool, system_files: &mut Vec<(String, u32, Arc<FontData>)>) -> Vec<String> {
    let mut keys = Vec::new();
    // Files already loaded as a fallback face. A file serves at most one key: the region-merged
    // Noto Sans CJK .ttc found for Han also carries Hangul (and Tahoma carries both Thai and
    // Hebrew), so loading it again for a later script would only duplicate tens of MB in memory.
    let mut loaded: Vec<String> = Vec::new();
    for (key, cands) in fallback_faces(han) {
        if key.starts_with("sys-cjk-") && !cjk {
            continue;
        }
        for (path, index) in &cands {
            if loaded.contains(path) {
                break; // a face loaded for an earlier script already covers this one
            }
            if let Some(fd) = try_load(&[(path.as_str(), *index)], system_files) {
                fonts.font_data.insert(key.to_string(), fd);
                keys.push(key.to_string());
                loaded.push(path.clone());
                break;
            }
        }
    }
    keys
}

#[cfg(target_arch = "wasm32")]
fn load_fallback(_fonts: &mut FontDefinitions, _han: HanOrder, _cjk: bool, _system_files: &mut Vec<(String, u32, Arc<FontData>)>) -> Vec<String> {
    Vec::new()
}

/// One face per script family, with the preferred regional Han face first. A region-merged
/// collection covers all CJK scripts and is loaded only once by `load_fallback`.
#[cfg(not(target_arch = "wasm32"))]
fn fallback_faces(han: HanOrder) -> Vec<(&'static str, Vec<(String, u32)>)> {
    let ja = ("sys-cjk-han-ja", cjk_han_japanese());
    let zh = ("sys-cjk-han-zh", cjk_han_simplified());
    let tc = ("sys-cjk-han-tc", cjk_han_traditional());
    let ko = ("sys-cjk-hangul", cjk_hangul());
    let mut faces = match han {
        HanOrder::Japanese => vec![ja, zh, tc, ko],
        HanOrder::Chinese => vec![zh, tc, ja, ko],
        HanOrder::TraditionalChinese => vec![tc, zh, ja, ko],
        HanOrder::Korean => vec![ko, ja, zh, tc],
    };
    faces.extend([("sys-cyrillic", cyrillic()), ("sys-thai", thai()), ("sys-hebrew", hebrew())]);
    faces
}

/// Linux directories that hold CJK fonts across distributions: Debian/Ubuntu (`opentype/noto`,
/// `truetype/wqy`), Fedora (`google-noto-cjk`), Arch (`noto-cjk`, `wenquanyi`), plus local and
/// flat layouts.
#[cfg(not(target_arch = "wasm32"))]
const LINUX_CJK_DIRS: [&str; 13] = [
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/google-noto-cjk",
    "/usr/share/fonts/noto-cjk",
    "/usr/share/fonts/google-noto-sans-cjk-fonts",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/noto",
    "/usr/share/fonts/truetype",
    "/usr/share/fonts/TTF",
    "/usr/share/fonts/OTF",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "/usr/local/share/fonts/noto-cjk",
    "/usr/local/share/fonts/noto",
];

#[cfg(not(target_arch = "wasm32"))]
fn noto_candidates(han: HanOrder) -> Vec<(String, u32)> {
    let index = match han {
        HanOrder::Japanese => 0,
        HanOrder::Chinese => 2,
        HanOrder::TraditionalChinese => 3,
        HanOrder::Korean => 1,
    };
    let mut paths = Vec::new();
    for dir in LINUX_CJK_DIRS {
        paths.push((format!("{dir}/{}", noto_otf_order(han)[0]), 0));
        paths.push((format!("{dir}/NotoSansCJK-Regular.ttc"), index));
        paths.push((format!("{dir}/NotoSansCJK-VF.otf.ttc"), index));
    }
    paths
}

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
    v.push((format!("{win}\\meiryo.ttc"), 0));
    v.push((format!("{mac}/ヒラギノ角ゴシック W3.ttc"), 0)); // Hiragino Sans
    v.extend(noto_candidates(HanOrder::Japanese));
    v
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
    v.extend(noto_candidates(HanOrder::Chinese));
    for d in LINUX_CJK_DIRS {
        v.push((format!("{d}/wqy/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/wenquanyi/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/wenquanyi/wqy-microhei/wqy-microhei.ttc"), 0));
        v.push((format!("{d}/droid/DroidSansFallbackFull.ttf"), 0));
    }
    v
}

/// Traditional Chinese glyph shapes precede simplified Chinese for Hant interfaces.
#[cfg(not(target_arch = "wasm32"))]
fn cjk_han_traditional() -> Vec<(String, u32)> {
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut paths = vec![(format!("{win}\\msjh.ttc"), 0), ("/System/Library/Fonts/PingFang.ttc".into(), 1)];
    paths.extend(noto_candidates(HanOrder::TraditionalChinese));
    paths
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
    v.extend(noto_candidates(HanOrder::Korean));
    v
}

/// Broad Cyrillic coverage even when the chosen UI/cell face contains only Latin glyphs.
#[cfg(not(target_arch = "wasm32"))]
fn cyrillic() -> Vec<(String, u32)> {
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut paths = vec![
        (format!("{win}\\segoeui.ttf"), 0),
        (format!("{win}\\arial.ttf"), 0),
        ("/System/Library/Fonts/Supplemental/Arial.ttf".into(), 0),
        ("/System/Library/Fonts/HelveticaNeue.ttc".into(), 0),
    ];
    for dir in LINUX_CJK_DIRS {
        for file in [
            "NotoSans-Regular.ttf",
            "DejaVuSans.ttf",
            "LiberationSans-Regular.ttf",
            "FreeSans.ttf",
            "noto/NotoSans-Regular.ttf",
            "dejavu/DejaVuSans.ttf",
            "liberation/LiberationSans-Regular.ttf",
            "liberation2/LiberationSans-Regular.ttf",
            "freefont/FreeSans.ttf",
        ] {
            paths.push((format!("{dir}/{file}"), 0));
        }
    }
    paths
}

/// Thai faces. Windows ships Leelawadee UI and Tahoma/Microsoft Sans Serif (both carry Thai);
/// macOS has Thonburi; Linux usually has Noto Sans Thai or Garuda.
#[cfg(not(target_arch = "wasm32"))]
fn thai() -> Vec<(String, u32)> {
    let mac_sys = "/System/Library/Fonts";
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\LeelawUI.ttf"), 0)); // Leelawadee UI
    v.push((format!("{win}\\tahoma.ttf"), 0)); // Tahoma
    v.push((format!("{win}\\micross.ttf"), 0)); // Microsoft Sans Serif
    v.push((format!("{mac_sup}/Thonburi.ttc"), 0));
    v.push((format!("{mac_sys}/SukhumvitSet.ttc"), 0));
    for d in LINUX_CJK_DIRS {
        v.push((format!("{d}/NotoSansThai-Regular.ttf"), 0));
        v.push((format!("{d}/tlwg/Garuda.ttf"), 0));
        v.push((format!("{d}/garuda/Garuda.ttf"), 0));
    }
    v
}

/// Hebrew faces. Windows has David and FrankRuehl (and Tahoma/Courier New carry Hebrew); macOS has
/// Arial Hebrew and David; Linux usually has Noto Sans Hebrew or DejaVu Sans.
#[cfg(not(target_arch = "wasm32"))]
fn hebrew() -> Vec<(String, u32)> {
    let mac_sys = "/System/Library/Fonts";
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\david.ttf"), 0)); // David
    v.push((format!("{win}\\frank.ttf"), 0)); // FrankRuehl
    v.push((format!("{win}\\tahoma.ttf"), 0)); // Tahoma
    v.push((format!("{mac_sup}/Arial Hebrew.ttf"), 0));
    v.push((format!("{mac_sup}/David.ttf"), 0));
    v.push((format!("{mac_sys}/Lucida Grande.ttc"), 0));
    for d in LINUX_CJK_DIRS {
        v.push((format!("{d}/NotoSansHebrew-Regular.ttf"), 0));
        v.push((format!("{d}/DejaVuSans.ttf"), 0));
        v.push((format!("{d}/FreeSans.ttf"), 0));
        v.push((format!("{d}/dejavu/DejaVuSans.ttf"), 0));
        v.push((format!("{d}/freefont/FreeSans.ttf"), 0));
    }
    v
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn cjk_detection() {
        assert!(has_cjk("売上 2026"));
        assert!(has_cjk("한글"));
        assert!(!has_cjk("Planilha1 ação Русский"));
        assert!(is_cjk_language("ja-JP"));
        assert!(is_cjk_language("zh-TW"));
        assert!(!is_cjk_language("pt-BR"));
    }

    #[test]
    fn web_fonts_follow_the_language() {
        assert_eq!(web_cjk_font("ko-KR"), "cjk-ko.ttf");
        assert_eq!(web_cjk_font("zh-TW"), "cjk-zh-TW.ttf");
        assert_eq!(web_cjk_font("zh-HK"), "cjk-zh-TW.ttf");
        assert_eq!(web_cjk_font("zh-Hant"), "cjk-zh-TW.ttf");
        assert_eq!(web_cjk_font("zh-CN"), "cjk-zh-CN.ttf");
        assert_eq!(web_cjk_font("ja-JP"), "cjk-ja.ttf");
        assert_eq!(web_cjk_font("ru-RU"), "cjk-zh-CN.ttf");
    }

    #[test]
    fn downloaded_fonts_extend_every_family_without_duplicate_keys() {
        let data = Arc::new(FontData::from_static(&[0u8; 4]));
        let downloaded = vec![("cjk-ja.ttf".into(), data.clone()), ("cjk-ja.ttf".into(), data)];
        let fonts = font_definitions(&FontNeeds { language: "ja-JP", cjk: true, downloaded: &downloaded });
        for family in [FontFamily::Proportional, FontFamily::Name(MONO.into()), FontFamily::Name(UI.into())] {
            let chain = fonts.families.get(&family).cloned().unwrap_or_default();
            assert_eq!(chain.iter().filter(|name| *name == "downloaded-cjk-ja.ttf").count(), 1, "{family:?}");
        }
        let plain = font_definitions(&FontNeeds::default());
        assert!(!plain.font_data.contains_key("downloaded-cjk-ja.ttf"));
    }

    #[test]
    fn the_font_of_the_interface_language_comes_first() {
        let data = Arc::new(FontData::from_static(&[0u8; 4]));
        let downloaded = vec![("cjk-ja.ttf".to_string(), data.clone()), ("cjk-ko.ttf".to_string(), data)];
        let position = |language: &str, font: &str| {
            let fonts = font_definitions(&FontNeeds { language, cjk: true, downloaded: &downloaded });
            let chain = fonts.families.get(&FontFamily::Name(UI.into())).cloned().unwrap_or_default();
            chain.iter().position(|n| n == font)
        };
        let (ko_in_ko, ja_in_ko) = (position("ko-KR", "downloaded-cjk-ko.ttf"), position("ko-KR", "downloaded-cjk-ja.ttf"));
        assert!(ko_in_ko.is_some() && ko_in_ko < ja_in_ko);
        let (ko_in_ja, ja_in_ja) = (position("ja-JP", "downloaded-cjk-ko.ttf"), position("ja-JP", "downloaded-cjk-ja.ttf"));
        assert!(ja_in_ja.is_some() && ja_in_ja < ko_in_ja);
    }

    #[test]
    fn the_font_key_tracks_coverage_and_shapes_not_language_names() {
        let needs = |language: &'static str, cjk: bool| FontNeeds { language, cjk, downloaded: &[] };
        assert_eq!(needs("pt-BR", false).key(), needs("ru-RU", false).key());
        assert_eq!(needs("pt-BR", true).key(), needs("ru-RU", true).key());
        assert_eq!(needs("zh-HK", true).key(), needs("zh-TW", true).key());
        assert_ne!(needs("ja-JP", true).key(), needs("ko-KR", true).key());
        assert_ne!(needs("pt-BR", false).key(), needs("pt-BR", true).key());
        let fonts = [("cjk-ja.ttf".to_string(), Arc::new(FontData::from_static(&[0u8; 4])))];
        assert_ne!(needs("ja-JP", true).key(), FontNeeds { downloaded: &fonts, ..needs("ja-JP", true) }.key());
    }

    #[test]
    fn split_noto_files_are_ordered_by_language() {
        let first = |tag: &str| noto_otf_order(HanOrder::of(tag))[0];
        assert!(first("ko-KR").contains("kr"));
        assert!(first("zh-TW").contains("tc") && first("zh-HK").contains("tc"));
        assert!(first("zh-CN").contains("sc") && first("pt-BR").contains("sc"));
        assert!(first("ja-JP").contains("jp"));
        for tag in ["ko-KR", "zh-TW", "zh-CN", "ja-JP"] {
            let mut all = noto_otf_order(HanOrder::of(tag)).to_vec();
            all.sort_unstable();
            all.dedup();
            assert_eq!(all.len(), 4, "{tag}");
        }
    }

    #[test]
    fn fallback_faces_are_appended_to_every_family() {
        // Structural: whatever loaded, the non-Latin fallback keys must sit at the end of each chain
        // (after the Latin/default fonts) so Latin still wins for Latin text, in the language's order.
        for language in ["en-US", "ja-JP", "zh-TW", "ko-KR"] {
            let han = HanOrder::of(language);
            let fonts = font_definitions(&FontNeeds { language, cjk: true, downloaded: &[] });
            let fb_keys: Vec<String> =
                fallback_faces(han).into_iter().map(|(k, _)| k.to_string()).filter(|k| fonts.font_data.contains_key(k)).collect();
            if fb_keys.is_empty() {
                continue; // no fallback face on this machine (minimal Linux)
            }
            for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
                let chain = fonts.families.get(&FontFamily::Name(role.into())).map(Vec::as_slice).unwrap_or_default();
                let actual: Vec<&String> = chain.iter().filter(|key| fb_keys.contains(key)).collect();
                assert_eq!(actual, fb_keys.iter().collect::<Vec<_>>(), "{role} must keep the system fallback order");
                let first_fallback = chain.iter().position(|key| fb_keys.contains(key));
                let last_default =
                    chain.iter().rposition(|key| !key.starts_with("sys-") && !key.starts_with("craft-") && !key.starts_with("downloaded-"));
                assert!(last_default.is_none() || first_fallback > last_default, "{role} must put defaults before fallbacks");
            }
        }
    }

    #[test]
    fn language_reorders_only_the_han_faces() {
        let keys = |han| fallback_faces(han).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(
            keys(HanOrder::Japanese),
            ["sys-cjk-han-ja", "sys-cjk-han-zh", "sys-cjk-han-tc", "sys-cjk-hangul", "sys-cyrillic", "sys-thai", "sys-hebrew"]
        );
        assert_eq!(
            keys(HanOrder::Chinese),
            ["sys-cjk-han-zh", "sys-cjk-han-tc", "sys-cjk-han-ja", "sys-cjk-hangul", "sys-cyrillic", "sys-thai", "sys-hebrew"]
        );
        assert_eq!(HanOrder::of("ja-JP"), HanOrder::Japanese);
        assert_eq!(HanOrder::of("ko-KR"), HanOrder::Korean);
        assert_eq!(HanOrder::of("zh-Hant-CN"), HanOrder::TraditionalChinese);
        assert_eq!(HanOrder::of("zh-Hans-HK"), HanOrder::Chinese);
        for l in ["en-US", "zh-CN", "ru-RU", "pt-BR"] {
            assert_eq!(HanOrder::of(l), HanOrder::Chinese, "{l:?} shares the default order (no font rebuild)");
        }
    }

    #[test]
    fn fallback_faces_load_each_file_once() {
        // The region-merged Noto Sans CJK .ttc serves Japanese, Chinese and Hangul; it must be loaded
        // once, not once per script (tens of MB each).
        for language in ["en-US", "ja-JP", "zh-TW", "ko-KR"] {
            let fonts = font_definitions(&FontNeeds { language, cjk: true, downloaded: &[] });
            let present: Vec<&Arc<FontData>> =
                fallback_faces(HanOrder::of(language)).into_iter().filter_map(|(k, _)| fonts.font_data.get(k)).collect();
            for (i, a) in present.iter().enumerate() {
                for b in present.iter().skip(i + 1) {
                    assert!(a.font != b.font, "the same font file is loaded for two fallback scripts");
                }
            }
        }
    }

    #[test]
    fn cyrillic_fallback_is_available_without_cjk_and_reuses_system_bytes() {
        let fonts = font_definitions(&FontNeeds { language: "ru-RU", cjk: false, downloaded: &[] });
        assert!(!fonts.font_data.keys().any(|key| key.starts_with("sys-cjk-")));
        if !fonts.font_data.contains_key("sys-cyrillic") {
            return; // no system Cyrillic face on this machine
        }
        for family in [FontFamily::Proportional, FontFamily::Monospace, FontFamily::Name(CELL.into())] {
            assert!(fonts.families.get(&family).is_some_and(|chain| chain.iter().any(|key| key == "sys-cyrillic")));
        }
        let paths = cyrillic();
        let refs: Vec<(&str, u32)> = paths.iter().map(|(path, index)| (path.as_str(), *index)).collect();
        let mut loaded = Vec::new();
        let first = try_load(&refs, &mut loaded);
        let second = try_load(&refs, &mut loaded);
        assert!(first.zip(second).is_some_and(|(a, b)| Arc::ptr_eq(&a, &b)));
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        out.textures_delta.clear(); // no renderer consumes the atlas in this headless test
        for family in [FontFamily::Proportional, FontFamily::Monospace, FontFamily::Name(CELL.into())] {
            let font = FontId::new(14.0, family);
            let glyphs = |text: &str| {
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::BLACK));
                galley.rows.iter().flat_map(|row| row.glyphs.iter().filter(|glyph| glyph.chr != ' ').map(|glyph| glyph.uv_rect)).collect::<Vec<_>>()
            };
            let tofu = glyphs("\u{25FB}");
            let drawn = glyphs("Русский Ёжик");
            assert!(!drawn.is_empty() && drawn.iter().all(|glyph| !tofu.contains(glyph)), "{:?}", font.family);
        }
    }

    #[test]
    fn fallback_covers_its_script() {
        // Real render check, per script: the proportional family (what cells and the UI use) must
        // resolve a sample string to a real glyph, not the tofu replacement char. Skips a script
        // whose face is absent (a minimal machine) and fails if a face is present but not wired in.
        let cases: [(&str, Vec<(String, u32)>, &str); 7] = [
            ("sys-cjk-han-ja", cjk_han_japanese(), "カテゴリ"),
            ("sys-cjk-han-zh", cjk_han_simplified(), "页面布局 客户端配置"),
            ("sys-cjk-han-tc", cjk_han_traditional(), "頁面配置"),
            ("sys-cjk-hangul", cjk_hangul(), "한글"),
            ("sys-thai", thai(), "ทดสอบ"),
            ("sys-hebrew", hebrew(), "עברית"),
            ("sys-cyrillic", cyrillic(), "Русский Ёжик"),
        ];
        for language in ["en-US", "ja-JP", "zh-TW", "ko-KR", "ru-RU"] {
            let ctx = egui::Context::default();
            ctx.set_fonts(font_definitions(&FontNeeds { language, cjk: true, downloaded: &[] }));
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
                for (key, cands, sample) in &cases {
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
}
