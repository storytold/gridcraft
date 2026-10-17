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

/// Loads the first candidate that exists and parses. A file that exists but can't be parsed is
/// skipped with a warning and the next candidate is tried.
#[cfg(not(target_arch = "wasm32"))]
fn try_load(paths: &[(&str, u32)]) -> Option<FontData> {
    for (p, index) in paths {
        if let Ok(bytes) = std::fs::read(p) {
            if !font_parses(&bytes, *index) {
                log::warn!("skipping font {p} (face {index}): not a font file egui can read");
                continue;
            }
            let mut fd = FontData::from_owned(bytes);
            fd.index = *index;
            return Some(fd);
        }
    }
    None
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

/// What a font added with [`add_font`] can draw: a hint from a few sample characters of each
/// script looked up in the font's character map (`가한힣`, `中日本字`, `あんアン`), not a check of
/// every character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub hangul: bool,
    pub han: bool,
    pub kana: bool,
}

/// What [`add_font`] did with a font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontAdded {
    /// False when a font of that name was already added: this one was ignored.
    pub added: bool,
    /// What the font under that name can draw.
    pub covers: Coverage,
}

impl FontAdded {
    /// `{name, added, covers: {hangul, han, kana}}`, as `app.font.add` returns it.
    pub fn to_json(&self, name: &str) -> serde_json::Value {
        let Coverage { hangul, han, kana } = self.covers;
        serde_json::json!({"name": name.trim(), "added": self.added, "covers": {"hangul": hangul, "han": han, "kana": kana}})
    }
}

/// A font the host added while the app runs ([`add_font`]).
#[derive(Clone)]
struct ExtraFont {
    name: String,
    data: Arc<FontData>,
    covers: Coverage,
}

/// The largest font [`add_font`] takes: the font scan's limit (`fonts::MAX_FONT_FILE`).
pub const MAX_EXTRA_FONT: usize = 64 << 20;
/// At most this many fonts are added, and at most `MAX_EXTRA_BYTES` in all: each one stays in
/// memory for the life of the app.
const MAX_EXTRA_FONTS: usize = 32;
const MAX_EXTRA_BYTES: usize = 256 << 20;
/// The longest font name taken, in characters.
const MAX_EXTRA_NAME: usize = 200;

/// The fonts added so far, shared by every font set the process builds.
static EXTRA_FONTS: std::sync::Mutex<Vec<ExtraFont>> = std::sync::Mutex::new(Vec::new());
/// How many fonts [`EXTRA_FONTS`] holds, read every frame without the lock.
static EXTRA_FONT_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Adds a font for every family to fall back on, after the system's fallback faces and the fonts
/// added before it: the web build has no system fonts, so this is how its host gives it CJK
/// glyphs (`app.font.add`, the web host's `gridcraft:font`). `index` picks the face in a
/// collection; TrueType and OpenType files only (not WOFF). The checks run in this order: the
/// name, a name already added (ignored: its first font stays, `added` is false), the size, how
/// many fonts there are, then the face itself: one egui can't read (it would panic) is refused,
/// and so is one it can't draw from (no glyph outlines, as in a file cut off after its character
/// map, or no units per em). The bytes are kept once, for the life of the app. Font sets built
/// from now on carry the font; the app rebuilds its own at the next frame.
pub fn add_font(name: &str, bytes: Vec<u8>, index: u32) -> Result<FontAdded, String> {
    let mut fonts = EXTRA_FONTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let added = add_extra(&mut fonts, name, bytes, index)?;
    EXTRA_FONT_COUNT.store(fonts.len(), std::sync::atomic::Ordering::Release);
    Ok(added)
}

/// The checks [`add_font`] makes before it reads a font of `len` bytes: an error, the answer for
/// a name already added, or `None` to go on and read it. Callers holding a large file elsewhere
/// (base64 text, a browser buffer) ask this before copying it.
pub fn check_new_font(name: &str, len: usize) -> Result<Option<FontAdded>, String> {
    precheck(&EXTRA_FONTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner), name, len)
}

/// A face number for [`add_font`] from a request: none (or null) is 0; otherwise a whole number
/// from 0 to `u32::MAX`.
pub fn face_index(index: Option<&serde_json::Value>) -> Result<u32, String> {
    match index {
        None | Some(serde_json::Value::Null) => Ok(0),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(f)).map(|f| f as u64))
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| "`index` must be a face number (0 for a single font)".to_string()),
    }
}

/// How many fonts [`add_font`] has added: a font set built when there were fewer is out of date.
pub fn extra_font_count() -> usize {
    EXTRA_FONT_COUNT.load(std::sync::atomic::Ordering::Acquire)
}

fn extra_fonts() -> Vec<ExtraFont> {
    EXTRA_FONTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

fn precheck(fonts: &[ExtraFont], name: &str, len: usize) -> Result<Option<FontAdded>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("missing `name` for the font".into());
    }
    if name.chars().count() > MAX_EXTRA_NAME {
        return Err(format!("the font name is longer than {MAX_EXTRA_NAME} characters"));
    }
    if let Some(font) = fonts.iter().find(|f| f.name == name) {
        return Ok(Some(FontAdded { added: false, covers: font.covers }));
    }
    if len > MAX_EXTRA_FONT {
        return Err(format!("font {name:?} is larger than 64 MiB"));
    }
    let total = fonts.iter().map(|f| f.data.font.len()).sum::<usize>().saturating_add(len);
    if fonts.len() >= MAX_EXTRA_FONTS || total > MAX_EXTRA_BYTES {
        return Err(format!("too many fonts added (at most {MAX_EXTRA_FONTS}, 256 MiB in all)"));
    }
    Ok(None)
}

fn add_extra(fonts: &mut Vec<ExtraFont>, name: &str, bytes: Vec<u8>, index: u32) -> Result<FontAdded, String> {
    if let Some(known) = precheck(fonts, name, bytes.len())? {
        return Ok(known);
    }
    let name = name.trim();
    if let Some(why) = cannot_draw(&bytes, index) {
        return Err(format!("font {name:?} (face {index}) {why}"));
    }
    let covers = coverage(&bytes, index);
    // Kept for the life of the app (fonts are never removed): as static bytes, which epaint shares
    // when it builds its fonts, where owned bytes would be copied each time.
    let mut data = FontData::from_static(Box::leak(bytes.into_boxed_slice()));
    data.index = index;
    fonts.push(ExtraFont { name: name.to_string(), data: Arc::new(data), covers });
    Ok(FontAdded { added: true, covers })
}

/// Why face `index` of a font the host hands over can't be added, or `None` when it can. Past
/// [`font_parses`], the face has to be one epaint can draw: epaint picks the face for a character
/// from its character map alone, so a face with no glyph outlines (a file cut off after its
/// character map, a bitmap-only font) would take characters from the faces after it and draw them
/// blank, and one with no units per em (no `head` table) scales every glyph by infinity, which
/// trips a debug assertion in epaint and leaves sizes infinite or NaN in a release build. Checked
/// with the calls epaint makes when it loads a face.
fn cannot_draw(bytes: &[u8], index: u32) -> Option<&'static str> {
    use skrifa::MetadataProvider;
    let font = match skrifa::FontRef::from_index(bytes, index) {
        Ok(font) if font_parses(bytes, index) => font,
        _ => return Some("isn't a font file egui can read"),
    };
    if font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default()).units_per_em == 0 {
        return Some("has no units per em (a missing or broken head table)");
    }
    if font.outline_glyphs().format().is_none() {
        return Some("has no glyph outlines egui can draw");
    }
    None
}

/// Which CJK scripts face `index` of `bytes` maps: every sample character must have a glyph.
fn coverage(bytes: &[u8], index: u32) -> Coverage {
    use skrifa::MetadataProvider;
    let Ok(font) = skrifa::FontRef::from_index(bytes, index) else { return Coverage::default() };
    let charmap = font.charmap();
    let maps = |sample: &str| sample.chars().all(|c| charmap.map(c).is_some());
    Coverage { hangul: maps("가한힣"), han: maps("中日本字"), kana: maps("あんアン") }
}

/// Puts the added fonts at the end of every family in `fonts`, after its own and fallback faces.
fn append_extra_fonts(fonts: &mut FontDefinitions, extra: &[ExtraFont]) {
    for font in extra {
        let key = format!("extra:{}", font.name);
        fonts.font_data.insert(key.clone(), font.data.clone());
        for chain in fonts.families.values_mut() {
            if !chain.contains(&key) {
                chain.push(key.clone());
            }
        }
    }
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
    font_definitions_with(language, &[])
}

/// [`font_definitions_for_language`] that also binds the workbook fonts in `used` that are
/// installed, so cells set in them render with them; the rest fall back to the roles. The bound
/// families are recorded in [`crate::fonts`] so [`cell_family`] only names a family egui has.
/// Every family ends with the fonts the host added ([`add_font`]).
pub fn font_definitions_with(language: crate::i18n::Language, used: &[String]) -> FontDefinitions {
    font_definitions_from(language, used, &extra_fonts())
}

fn font_definitions_from(language: crate::i18n::Language, used: &[String], extra: &[ExtraFont]) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let base_prop: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut bound: Vec<String> = Vec::new();
    // The non-Latin fallback faces have to sit at the end of each chain: a Latin font owns Latin,
    // and epaint picks the first face that has the glyph, so they only catch their own scripts.
    let fallback = load_fallback(&mut fonts, HanOrder::of(language));
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
        chain.extend(fallback.iter().cloned());
        chain.retain(|k| fonts.font_data.contains_key(k));
        fonts.families.insert(FontFamily::Name(role.into()), chain);
        bound.push(role.to_string());
    }
    // Installed workbook fonts, so a cell's chosen font actually renders in it.
    for family in crate::fonts::installed_among(used) {
        bound.extend(crate::fonts::bind_family(&mut fonts, &family));
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
    append_extra_fonts(&mut fonts, extra);
    drop_unparseable_fonts(&mut fonts);
    // Only families egui still has data for count as bound (a dropped face can empty a chain).
    bound.retain(|f| fonts.families.get(&FontFamily::Name(f.as_str().into())).is_some_and(|c| !c.is_empty()));
    crate::fonts::set_bound(bound);
    fonts
}

/// Loads the system non-Latin fallback faces (one file per script family) and returns their keys so
/// every family can carry them as fallback. egui's bundled fonts have no coverage beyond Latin, so
/// without this a Thai, Hebrew, Japanese or Chinese workbook shows tofu boxes even though the text
/// was read correctly. With a face installed those scripts get real glyphs instead of tofu; nothing
/// is bundled, and a machine without the face keeps the old behaviour.
#[cfg(not(target_arch = "wasm32"))]
fn load_fallback(fonts: &mut FontDefinitions, han: HanOrder) -> Vec<String> {
    let mut keys = Vec::new();
    // Files already loaded as a fallback face. A file serves at most one key: the region-merged
    // Noto Sans CJK .ttc found for Han also carries Hangul (and Tahoma carries both Thai and
    // Hebrew), so loading it again for a later script would only duplicate tens of MB in memory.
    let mut loaded: Vec<String> = Vec::new();
    for (key, cands) in fallback_faces(han) {
        for (path, index) in &cands {
            if loaded.contains(path) {
                break; // a face loaded for an earlier script already covers this one
            }
            if let Some(fd) = try_load(&[(path.as_str(), *index)]) {
                fonts.font_data.insert(key.to_string(), Arc::new(fd));
                keys.push(key.to_string());
                loaded.push(path.clone());
                break;
            }
        }
    }
    keys
}

#[cfg(target_arch = "wasm32")]
fn load_fallback(_fonts: &mut FontDefinitions, _han: HanOrder) -> Vec<String> {
    Vec::new()
}

/// Every fallback face we look for, in chain order: `(key, candidate files)`. One file per script
/// family is enough — epaint picks the first face with the glyph, and a script's glyphs live in one
/// file. Order only matters between faces that both cover a glyph (Japanese vs Chinese kanji), so
/// the interface language reorders the two Han entries and nothing else.
#[cfg(not(target_arch = "wasm32"))]
fn fallback_faces(han: HanOrder) -> Vec<(&'static str, Vec<(String, u32)>)> {
    let ja = ("sys-cjk-han-ja", cjk_han_japanese());
    let zh = ("sys-cjk-han-zh", cjk_han_simplified());
    let (first, second) = match han {
        HanOrder::Japanese => (ja, zh),
        HanOrder::Chinese => (zh, ja),
    };
    vec![
        first,
        second,
        ("sys-cjk-hangul", cjk_hangul()),
        ("sys-thai", thai()),
        ("sys-hebrew", hebrew()),
        ("sys-symbols", symbols()),
        ("sys-symbols-2", symbols_more()),
    ]
}

/// Linux directories that hold CJK fonts across distributions: Debian/Ubuntu (`opentype/noto`,
/// `truetype/wqy`), Fedora (`google-noto-cjk`), Arch (`noto-cjk`, `wenquanyi`), plus local and
/// flat layouts.
#[cfg(not(target_arch = "wasm32"))]
const LINUX_CJK_DIRS: [&str; 10] = [
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
    for d in LINUX_CJK_DIRS {
        v.push((format!("{d}/NotoSansCJKjp-Regular.otf"), 0));
        v.push((format!("{d}/NotoSansCJK-Regular.ttc"), 0)); // JP face
    }
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
    for d in LINUX_CJK_DIRS {
        v.push((format!("{d}/NotoSansCJKsc-Regular.otf"), 0));
        v.push((format!("{d}/NotoSansCJK-Regular.ttc"), 2)); // SC face
    }
    for d in LINUX_CJK_DIRS {
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
    for d in LINUX_CJK_DIRS {
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
    let lin = ["/usr/share/fonts/truetype/noto", "/usr/share/fonts/opentype/noto", "/usr/share/fonts/truetype", "/usr/share/fonts"];
    let mut v: Vec<(String, u32)> = Vec::new();
    v.push((format!("{win}\\LeelawUI.ttf"), 0)); // Leelawadee UI
    v.push((format!("{win}\\tahoma.ttf"), 0)); // Tahoma
    v.push((format!("{win}\\micross.ttf"), 0)); // Microsoft Sans Serif
    v.push((format!("{mac_sup}/Thonburi.ttc"), 0));
    v.push((format!("{mac_sys}/SukhumvitSet.ttc"), 0));
    for d in lin {
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
    let lin = ["/usr/share/fonts/truetype/noto", "/usr/share/fonts/opentype/noto", "/usr/share/fonts/truetype", "/usr/share/fonts"];
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
    for d in lin {
        v.push((format!("{d}/NotoSansHebrew-Regular.ttf"), 0));
        v.push((format!("{d}/dejavu/DejaVuSans.ttf"), 0));
        v.push((format!("{d}/freefont/FreeSans.ttf"), 0));
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
    for d in [
        "/usr/share/fonts/noto",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/google-noto-sans-symbols-2-fonts",
        "/usr/share/fonts/google-noto",
        "/usr/local/share/fonts/noto",
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
                fallback_faces(han).into_iter().map(|(k, _)| k.to_string()).filter(|k| fonts.font_data.contains_key(k)).collect();
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
    fn interface_families_fall_back_to_the_cell_font() {
        // Structural (#349): the formula bar, name box and dialogs draw every script the cells can,
        // so each interface chain carries the cell face, ahead of the script fallback faces.
        let fonts = font_definitions_for_language(Language::En);
        let cell = format!("sys-{CELL}");
        if !fonts.font_data.contains_key(&cell) {
            return; // no system cell font on this machine
        }
        let fb: Vec<String> = fallback_faces(HanOrder::Chinese).into_iter().map(|(k, _)| k.to_string()).collect();
        for family in [FontFamily::Proportional, FontFamily::Name(UI.into()), FontFamily::Name(UI_BOLD.into()), FontFamily::Name(MONO.into())] {
            let chain = fonts.families.get(&family).cloned().unwrap_or_default();
            let at = chain.iter().position(|k| *k == cell);
            assert!(at.is_some(), "{family:?} lacks the cell face: {chain:?}");
            let first_fb = chain.iter().position(|k| fb.contains(k)).unwrap_or(chain.len());
            assert!(at.unwrap_or(usize::MAX) < first_fb, "{family:?}: cell face must precede the script faces");
        }
    }

    #[test]
    fn language_reorders_only_the_han_faces() {
        let keys = |han| fallback_faces(han).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(
            keys(HanOrder::Japanese),
            ["sys-cjk-han-ja", "sys-cjk-han-zh", "sys-cjk-hangul", "sys-thai", "sys-hebrew", "sys-symbols", "sys-symbols-2"]
        );
        assert_eq!(
            keys(HanOrder::Chinese),
            ["sys-cjk-han-zh", "sys-cjk-han-ja", "sys-cjk-hangul", "sys-thai", "sys-hebrew", "sys-symbols", "sys-symbols-2"]
        );
        assert_eq!(HanOrder::of(Language::Ja), HanOrder::Japanese);
        for l in [Language::En, Language::Zh, Language::Ko, Language::Ru, Language::PtBr, Language::De, Language::Cs] {
            assert_eq!(HanOrder::of(l), HanOrder::Chinese, "{l:?} shares the default order (no font rebuild)");
        }
    }

    #[test]
    fn fallback_faces_load_each_file_once() {
        // The region-merged Noto Sans CJK .ttc serves Japanese, Chinese and Hangul; it must be loaded
        // once, not once per script (tens of MB each).
        for language in [Language::En, Language::Ja] {
            let fonts = font_definitions_for_language(language);
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
    fn fallback_covers_its_script() {
        // Real render check, per script: the proportional family (what cells and the UI use) must
        // resolve a sample string to a real glyph, not the tofu replacement char. Skips a script
        // whose face is absent (a minimal machine) and fails if a face is present but not wired in.
        let cases: [(&str, Vec<(String, u32)>, &str); 7] = [
            ("sys-cjk-han-ja", cjk_han_japanese(), "カテゴリ"),
            ("sys-cjk-han-zh", cjk_han_simplified(), "页面布局 客户端配置"),
            ("sys-cjk-hangul", cjk_hangul(), "한글"),
            ("sys-thai", thai(), "ทดสอบ"),
            ("sys-hebrew", hebrew(), "עברית"),
            ("ui", candidates(UI), "Русский"),
            // Czech interface labels: every Czech accented letter, in both cases.
            ("ui", candidates(UI), "Příliš žluťoučký kůň úpěl ďábelské ódy ŘŠČŽÝÁÍÉÚŮĚŤĎŇÓ"),
        ];
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
}

#[cfg(test)]
mod extra_font_tests {
    use super::*;
    use crate::i18n::Language;

    const ADDED: &str = "extra:Ubuntu Light";

    fn ubuntu() -> Vec<u8> {
        epaint_default_fonts::UBUNTU_LIGHT.to_vec()
    }

    fn added(covers: Coverage) -> Result<FontAdded, String> {
        Ok(FontAdded { added: true, covers })
    }

    /// Where table record `i` of a font's table directory starts.
    fn record(i: usize) -> usize {
        12 + 16 * i
    }

    fn table_count(font: &[u8]) -> usize {
        u16::from_be_bytes([font[4], font[5]]) as usize
    }

    /// Ubuntu Light with table `from` renamed to `to` in its table directory: every byte is still
    /// there, but nothing finds the table any more.
    fn ubuntu_renaming(from: &[u8; 4], to: &[u8; 4]) -> Vec<u8> {
        let mut font = ubuntu();
        for i in 0..table_count(&font) {
            let at = record(i);
            if &font[at..at + 4] == from {
                font[at..at + 4].copy_from_slice(to);
            }
        }
        font
    }

    /// `font` rewritten with its outlines (`glyf`) last, the order most fonts are laid out in
    /// (character map and metrics first), and where `glyf` now starts: cut there, a file keeps
    /// everything but its outlines, like a font whose transfer broke off.
    fn outlines_last(font: &[u8]) -> (Vec<u8>, usize) {
        let tables = table_count(font);
        let mut order: Vec<usize> = (0..tables).collect();
        order.sort_by_key(|&i| &font[record(i)..record(i) + 4] == b"glyf");
        let mut out = font[..record(tables)].to_vec();
        let mut glyf = 0;
        for i in order {
            let at = record(i);
            let offset = u32::from_be_bytes([font[at + 8], font[at + 9], font[at + 10], font[at + 11]]) as usize;
            let len = u32::from_be_bytes([font[at + 12], font[at + 13], font[at + 14], font[at + 15]]) as usize;
            out.resize(out.len().next_multiple_of(4), 0);
            if &font[at..at + 4] == b"glyf" {
                glyf = out.len();
            }
            let moved = u32::try_from(out.len()).unwrap().to_be_bytes();
            out[at + 8..at + 12].copy_from_slice(&moved);
            out.extend_from_slice(&font[offset..offset + len]);
        }
        (out, glyf)
    }

    /// A Korean face installed on this machine, if any (a minimal machine has none: the checks
    /// that need one are skipped there).
    fn korean_face() -> Option<(Vec<u8>, u32)> {
        #[cfg(not(target_arch = "wasm32"))]
        for (path, index) in cjk_hangul() {
            if let Ok(bytes) = std::fs::read(&path)
                && bytes.len() <= MAX_EXTRA_FONT
                && font_parses(&bytes, index)
            {
                return Some((bytes, index));
            }
        }
        None
    }

    /// Whether `font` alone draws `text` with real glyphs (not the replacement box), without
    /// panicking on the way.
    fn draws(font: Arc<FontData>, text: &str) -> bool {
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert("probe".into(), font);
        fonts.families.insert(FontFamily::Name("probe".into()), vec!["probe".into()]);
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
        let font = FontId::new(14.0, FontFamily::Name("probe".into()));
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, Color32::BLACK));
        galley.rows.iter().flat_map(|r| r.glyphs.iter()).any(|g| g.uv_rect.size.x > 0.0)
    }

    #[test]
    fn an_added_font_ends_every_family() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut extra = Vec::new();
        assert_eq!(add_extra(&mut extra, "Ubuntu Light", ubuntu(), 0), added(Coverage::default()));
        for language in [Language::En, Language::Ja] {
            let fonts = font_definitions_from(language, &[], &extra);
            assert!(fonts.font_data.contains_key(ADDED));
            for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
                assert!(fonts.families.contains_key(&FontFamily::Name(role.into())), "{role} is still bound");
            }
            for (family, chain) in &fonts.families {
                assert_eq!(chain.last().map(String::as_str), Some(ADDED), "{family:?} must end with the added font: {chain:?}");
                assert_eq!(chain.iter().filter(|k| *k == ADDED).count(), 1, "{family:?} carries it once");
            }
        }
        let plain = font_definitions_from(Language::En, &[], &[]);
        assert!(plain.families.values().all(|chain| !chain.iter().any(|k| k.starts_with("extra:"))), "nothing added, nothing appended");
    }

    #[test]
    fn fonts_added_earlier_come_first() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut extra = Vec::new();
        assert!(add_extra(&mut extra, "First", ubuntu(), 0).is_ok());
        assert!(add_extra(&mut extra, "Second", ubuntu(), 0).is_ok());
        let fonts = font_definitions_from(Language::En, &[], &extra);
        for chain in fonts.families.values() {
            let tail: Vec<&str> = chain.iter().rev().take(2).rev().map(String::as_str).collect();
            assert_eq!(tail, ["extra:First", "extra:Second"]);
        }
    }

    #[test]
    fn a_bad_or_oversized_font_is_refused_without_a_panic() {
        let mut extra = Vec::new();
        let real = ubuntu();
        for (bytes, index) in [(Vec::new(), 0), (vec![0xAB; 64], 0), (real.get(..20).unwrap_or_default().to_vec(), 0), (real.clone(), 3)] {
            assert!(add_extra(&mut extra, "Broken", bytes, index).is_err(), "index {index}");
        }
        let huge = add_extra(&mut extra, "Huge", vec![0; MAX_EXTRA_FONT + 1], 0);
        assert!(huge.as_ref().is_err_and(|e| e.contains("64 MiB")), "{huge:?}");
        assert!(add_extra(&mut extra, "  ", real.clone(), 0).is_err(), "a font needs a name");
        assert!(add_extra(&mut extra, &"x".repeat(1_000), real, 0).is_err(), "a name is short");
        assert!(extra.is_empty(), "nothing was added");
    }

    #[test]
    fn a_font_that_cannot_draw_is_refused() {
        // epaint picks the face for a character from its character map alone: a face with no
        // outlines would take characters from the faces after it and draw them blank, and one with
        // no units per em (no `head`) scales every glyph by infinity, a debug assertion panic
        // ("Bad px_scale_factor: inf").
        let mut extra = Vec::new();
        let no_outlines = add_extra(&mut extra, "No Outlines", ubuntu_renaming(b"glyf", b"gly_"), 0);
        assert!(no_outlines.as_ref().is_err_and(|e| e.contains("glyph outlines")), "{no_outlines:?}");
        let no_head = add_extra(&mut extra, "No Head", ubuntu_renaming(b"head", b"hea_"), 0);
        assert!(no_head.as_ref().is_err_and(|e| e.contains("units per em")), "{no_head:?}");
        // A real font whose transfer broke off inside its outlines: its character map, head and
        // metrics all arrived.
        let (whole, glyf) = outlines_last(&ubuntu());
        let cut = add_extra(&mut extra, "Cut Off", whole.get(..glyf + 1_000).unwrap_or_default().to_vec(), 0);
        assert!(cut.as_ref().is_err_and(|e| e.contains("glyph outlines")), "{cut:?}");
        assert!(extra.is_empty(), "nothing was added");
        // Laid out that way but whole, it is a font like any other.
        assert!(add_extra(&mut extra, "Outlines Last", whole, 0).is_ok());
        assert!(extra.first().is_some_and(|f| draws(f.data.clone(), "Hello")));
    }

    #[test]
    fn a_name_added_twice_keeps_its_first_font() {
        let mut extra = Vec::new();
        assert_eq!(add_extra(&mut extra, "Body", ubuntu(), 0), added(Coverage::default()));
        // The second time is ignored, whatever it carries, and says so.
        assert_eq!(add_extra(&mut extra, " Body ", vec![0xAB; 64], 0), Ok(FontAdded { added: false, covers: Coverage::default() }));
        assert_eq!(extra.len(), 1);
        assert_eq!(extra.first().map(|f| f.data.font.len()), Some(epaint_default_fonts::UBUNTU_LIGHT.len()));
    }

    #[test]
    fn a_name_added_before_or_a_size_over_the_limit_is_known_before_the_bytes_are_read() {
        let mut extra = Vec::new();
        assert!(add_extra(&mut extra, "Body", ubuntu(), 0).is_ok());
        assert_eq!(precheck(&extra, " Body ", usize::MAX), Ok(Some(FontAdded { added: false, covers: Coverage::default() })));
        assert_eq!(precheck(&extra, "Other", 1_000), Ok(None), "a new font goes on to be read");
        assert!(precheck(&extra, "Other", MAX_EXTRA_FONT + 1).is_err_and(|e| e.contains("64 MiB")));
        assert!(precheck(&extra, "", 10).is_err(), "a font needs a name");
    }

    #[test]
    fn at_most_32_fonts_are_added() {
        let mut extra = Vec::new();
        for i in 0..MAX_EXTRA_FONTS {
            assert!(add_extra(&mut extra, &format!("Font {i}"), ubuntu(), 0).is_ok(), "font {i}");
        }
        let one_more = add_extra(&mut extra, "One more", ubuntu(), 0);
        assert!(one_more.as_ref().is_err_and(|e| e.contains("too many")), "{one_more:?}");
        assert!(precheck(&extra, "One more", 10).is_err(), "known before the bytes are read");
        assert_eq!(extra.len(), MAX_EXTRA_FONTS);
    }

    #[test]
    fn face_numbers_are_whole_and_in_range() {
        use serde_json::json;
        assert_eq!(face_index(None), Ok(0));
        assert_eq!(face_index(Some(&json!(null))), Ok(0));
        assert_eq!(face_index(Some(&json!(2))), Ok(2));
        assert_eq!(face_index(Some(&json!(2.0))), Ok(2), "a whole number written with a point");
        assert_eq!(face_index(Some(&json!(4_294_967_295_u64))), Ok(u32::MAX));
        for bad in [json!(-1), json!(1.5), json!(4_294_967_296_u64), json!("1"), json!(true), json!([0])] {
            assert!(face_index(Some(&bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_added_font_is_kept_once_not_copied_for_each_font_set() {
        let mut extra = Vec::new();
        assert!(add_extra(&mut extra, "Shared", ubuntu(), 0).is_ok());
        // epaint copies owned font bytes every time it builds its fonts; borrowed ones it shares.
        assert!(extra.first().is_some_and(|f| matches!(f.data.font, std::borrow::Cow::Borrowed(_))));
    }

    #[test]
    fn damaged_fonts_are_refused_or_drawn_without_a_panic() {
        let real = ubuntu();
        let mut damaged: Vec<Vec<u8>> = Vec::new();
        for i in 0..48_usize {
            let mut bytes = real.clone();
            let at = i.wrapping_mul(7_919).wrapping_mul(13) % bytes.len();
            if let Some(b) = bytes.get_mut(at) {
                *b ^= 1 << (i % 8);
            }
            damaged.push(bytes);
        }
        for cut in [1, 12, 100, 1_000, real.len() / 3, real.len() / 2, real.len() - 1] {
            damaged.push(real.get(..cut).unwrap_or_default().to_vec());
        }
        let mut accepted = 0;
        for (i, bytes) in damaged.into_iter().enumerate() {
            let mut extra = Vec::new();
            if add_extra(&mut extra, &format!("Damaged {i}"), bytes, 0).is_ok()
                && let Some(font) = extra.first()
            {
                accepted += 1;
                let _ = draws(font.data.clone(), "Hello, 한글 漢字 カナ ÀÉÎ 123");
            }
        }
        assert!(accepted > 0, "some single-bit damage still parses, and it draws without a panic");
    }

    #[test]
    fn coverage_comes_from_the_character_map() {
        assert_eq!(coverage(&ubuntu(), 0), Coverage { hangul: false, han: false, kana: false }, "Ubuntu has no Hangul, Han or kana");
        assert_eq!(coverage(b"not a font", 0), Coverage::default());
        if let Some((bytes, index)) = korean_face() {
            assert!(coverage(&bytes, index).hangul, "a Korean face covers Hangul");
        }
    }

    #[test]
    fn an_added_korean_face_draws_hangul_with_no_system_fonts() {
        let Some((bytes, index)) = korean_face() else {
            return; // no Korean face on this machine; nothing to draw with
        };
        // The web build has no system fonts: its families hold egui's own, which have no Hangul.
        let draws_hangul = |fonts: FontDefinitions| {
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts);
            let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
            out.textures_delta.clear(); // nothing consumes the atlas in a headless test
            let font = FontId::new(14.0, FontFamily::Proportional);
            let glyphs = |text: &str| {
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::BLACK));
                galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| g.uv_rect)).collect::<Vec<_>>()
            };
            let tofu = glyphs("\u{25FB}");
            let drawn = glyphs("한글");
            !drawn.is_empty() && drawn.iter().all(|g| !tofu.contains(g))
        };
        let mut web = FontDefinitions::default();
        assert!(!draws_hangul(web.clone()), "egui's own fonts have no Hangul");
        let mut extra = Vec::new();
        assert!(add_extra(&mut extra, "Korean", bytes, index).is_ok_and(|a| a.added && a.covers.hangul));
        append_extra_fonts(&mut web, &extra);
        assert!(draws_hangul(web), "the added face draws Hangul");
    }
}
