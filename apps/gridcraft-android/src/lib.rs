//! GridCraft on Android — GameActivity + eframe/wgpu, reusing the desktop engine and egui UI.
//!
//! The engine, the ribbon, the grid and the theme are untouched: the same
//! [`gridcraft_engine::Session`] and [`gridcraft_ui_egui::SheetApp`] the desktop shell builds are
//! wired up here. What the desktop shell gets from the OS, this crate takes from the activity:
//!
//! | Desktop (`apps/gridcraft`) | Android (this crate) |
//! |---|---|
//! | `std::env::args()` (files, `--sample`) | none: the app starts on a blank workbook |
//! | `config_dir()` from `HOME`/`XDG_CONFIG_HOME` | [`AndroidApp::internal_data_path`] |
//! | `rfd` file dialogs, `arboard` clipboard | not wired yet: `Services` is left at its default |
//! | `sys-locale` interface language | English until a language picker is wired |
//! | `craft-print` printers | none |
//! | desktop-sized UI (1440×900, hover menus) | [`TouchShell`] enlarges touch targets |
//!
//! Known gaps are listed in the crate README; none of them stop the UI from coming up.
#![cfg(target_os = "android")]

use std::path::PathBuf;

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use winit::platform::android::activity::AndroidApp;

/// How much to scale the desktop layout up on a phone.
///
/// Measured on a 1080×2400 @ ~420 dpi phone: egui's logical size is about 411×914 pt at
/// `pixels_per_point ≈ 2.625`, while the ribbon + grid layout is designed for ≥900 pt wide.
/// Scaling up only shrinks the logical viewport further and makes the ribbon overlap itself, so
/// the shell does not scale; the layout needs a touch rework (collapsed ribbon, bottom sheet),
/// not a zoom.
const TOUCH_SCALE: f32 = 1.0;

/// System font files that carry CJK, in preference order. egui's default fonts are Latin-only, so
/// without one of these every Chinese/Japanese/Korean glyph renders as a blank box.
const CJK_CANDIDATES: &[&str] = &[
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/NotoSansCJKsc-Regular.otf",
    "/system/fonts/NotoSansSC-Regular.otf",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/DroidSansFallback.ttf",
];

/// The Android entry point. winit's `android-activity` glue looks this symbol up in the cdylib,
/// so the name and the signature are fixed — hence `unsafe(no_mangle)` and the crate-level
/// `unsafe_code = "deny"` rather than the workspace's `forbid`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("GridCraft"));
    log::info!("gridcraft-android: starting (GridCraft {})", env!("CARGO_PKG_VERSION"));

    let data_dir = app.internal_data_path().or_else(|| app.external_data_path());
    if let Some(dir) = &data_dir {
        log::info!("gridcraft-android: data dir {}", dir.display());
        std::fs::create_dir_all(dir).ok();
    }

    let mut session = Session::new();
    session.new_workbook();
    let mut sheet = SheetApp::new(session, services());
    sheet.after_engine();
    // Preferences live in the app's private directory; `load_prefs`/`save_prefs` mirror the
    // desktop's ui.json / prefs.json pair.
    if let Some(dir) = &data_dir {
        load_prefs(&mut sheet, dir);
    }

    let options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: egui::ViewportBuilder::default().with_title("GridCraft").with_app_id("ai.storyteller.gridcraft"),
        ..Default::default()
    };

    let result = eframe::run_native(
        "GridCraft",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(TouchShell::new(sheet, data_dir)))
        }),
    );
    if let Err(e) = result {
        log::error!("gridcraft-android: eframe exited: {e}");
    }
}

/// What the platform shell can do for the UI. Android has no `rfd` and no `arboard`, so every
/// picker and clipboard hook is left `None`: the UI already handles that (its menus fall back to
/// GridCraft's own clipboard, and an unavailable picker reports itself), which keeps the first
/// port honest instead of half-wired.
fn services() -> Services {
    Services::default()
}

/// GridCraft's own preferences, in the app's private data directory.
fn prefs_paths(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    (dir.join("ui.json"), dir.join("prefs.json"))
}

fn load_prefs(sheet: &mut SheetApp, dir: &std::path::Path) {
    let (ui_path, prefs_path) = prefs_paths(dir);
    if let Ok(bytes) = std::fs::read(&ui_path)
        && let Ok(ui) = serde_json::from_slice::<gridcraft_ui_egui::UiState>(&bytes)
    {
        sheet.ui = ui;
    }
    if let Ok(bytes) = std::fs::read(&prefs_path)
        && let Ok(p) = serde_json::from_slice(&bytes)
    {
        sheet.session.prefs = p;
    }
}

fn save_prefs(sheet: &SheetApp, dir: &std::path::Path) {
    let (ui_path, prefs_path) = prefs_paths(dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(&sheet.ui) {
        let _ = std::fs::write(ui_path, bytes);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&sheet.session.prefs) {
        let _ = std::fs::write(prefs_path, bytes);
    }
}

/// Bind a system CJK face as the last fallback of every family. egui applies new definitions on
/// the next pass, so this is called once, before the first frame.
fn setup_fonts(ctx: &egui::Context) {
    let Some((path, bytes)) = CJK_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().map(|b| (*p, b))) else {
        log::warn!("gridcraft-android: no CJK system font found; CJK text will render as boxes");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("system-cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("system-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    log::info!("gridcraft-android: CJK fallback font {path}");
}

/// The eframe app: forward everything to [`SheetApp`] and apply the phone adjustments.
struct TouchShell {
    sheet: SheetApp,
    data_dir: Option<PathBuf>,
    /// The one-shot touch tuning has run (after GridCraft's own theme is in place).
    tuned: bool,
    /// GridCraft's theme owns the font families; the system CJK face is appended after the first
    /// `ui` pass, because replacing the font set before the theme installs would drop it.
    font_done: bool,
    /// GridCraft's font setup has run at least once.
    theme_fonts_live: bool,
}

impl TouchShell {
    fn new(sheet: SheetApp, data_dir: Option<PathBuf>) -> Self {
        Self { sheet, data_dir, tuned: false, font_done: false, theme_fonts_live: false }
    }

    fn tune(ctx: &egui::Context) {
        let before = ctx.pixels_per_point();
        if (TOUCH_SCALE - 1.0).abs() > f32::EPSILON {
            ctx.set_pixels_per_point((before * TOUCH_SCALE).min(4.0));
        }
        ctx.all_styles_mut(|style| {
            // Touch targets: comfortable minimum, roomier padding, a little more separation so
            // neighbouring ribbon buttons are harder to hit by accident.
            style.spacing.interact_size = egui::vec2(40.0, 40.0);
            style.spacing.button_padding = egui::vec2(10.0, 8.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.slider_width = style.spacing.slider_width.max(160.0);
            // A resting finger must not raise a tooltip.
            style.interaction.show_tooltips_only_when_still = true;
        });
        let rect = ctx.viewport_rect();
        log::info!("gridcraft-android: touch mode applied (ppp {before}, logical {}×{} pt)", rect.width().round(), rect.height().round(),);
    }
}

impl eframe::App for TouchShell {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.sheet.logic(ctx);
        self.sheet.poll_printing(ctx);
        // The desktop shell keeps the window alive while a print job runs; on Android there is no
        // print backend, so only the close-request guard is worth mirroring.
        let _ = frame;
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.sheet.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.sheet.ui(ui);
        if !self.tuned {
            self.tuned = true;
            Self::tune(ui.ctx());
        }
        // The theme's own font definitions only become visible a frame after the theme install;
        // appending the CJK face any earlier would drop GridCraft's named families.
        if !self.font_done && self.theme_fonts_live {
            self.font_done = true;
            append_cjk_fallback(ui.ctx());
        }
        if !self.theme_fonts_live && ui.ctx().fonts(|f| f.definitions().families.keys().any(|k| matches!(k, egui::FontFamily::Name(_)))) {
            self.theme_fonts_live = true;
        }
    }

    fn on_exit(&mut self) {
        if let Some(dir) = &self.data_dir {
            save_prefs(&self.sheet, dir);
        }
    }
}

/// Append the system CJK face to whatever families the theme has installed, keeping the theme's
/// own order (its faces win; the system face only catches glyphs they lack).
fn append_cjk_fallback(ctx: &egui::Context) {
    let Some((path, bytes)) = CJK_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().map(|b| (*p, b))) else {
        return;
    };
    ctx.all_styles_mut(|_| {});
    let mut fonts = ctx.fonts(|f| f.definitions().clone());
    fonts.font_data.insert("system-cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        if !list.iter().any(|n| n == "system-cjk") {
            list.push("system-cjk".to_owned());
        }
    }
    for family in fonts.families.keys().cloned().collect::<Vec<_>>() {
        if let egui::FontFamily::Name(_) = family {
            let list = fonts.families.entry(family).or_default();
            if !list.iter().any(|n| n == "system-cjk") {
                list.push("system-cjk".to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
    log::info!("gridcraft-android: CJK fallback appended from {path}");
}
