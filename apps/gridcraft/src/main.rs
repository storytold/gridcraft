//! GridCraft desktop app.
//!
//! Usage: `gridcraft [--control <port>] [--sample <name>] [files…]`
//!
//! `--control <port>` (or `GRIDCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `gridcraft_ui_egui::control` and `docs/control-protocol.md` for the methods.
//!
//! Language and regional format: the interface language, the formula language and the regional
//! format follow the operating system (detected at every start with `sys-locale`) until changed in
//! Options › Language or with `app.setLocale`; the choice is kept in `prefs.json`.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
mod apple_events;
mod control_server;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu_model;
#[cfg(target_os = "macos")]
mod native_menu;

use gridcraft_engine::{Prefs, Session};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

struct App(SheetApp, #[cfg(target_os = "macos")] Option<native_menu::NativeMenu>, #[cfg(target_os = "macos")] fmv_macos_events::Inbox);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.1.is_none() && std::env::var_os("GRIDCRAFT_NO_NATIVE_MENU").is_none() {
                self.1 = Some(native_menu::NativeMenu::install(ctx, &self.0));
            }
            if let Some(m) = &mut self.1 {
                m.poll(&mut self.0, ctx);
            }
            apple_events::poll(&self.2, &mut self.0, ctx);
        }
        self.0.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
        // Files dropped on the window open as workbooks.
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().map(str::to_string).filter(|p| !p.is_empty()) {
                self.0.open_path(&p);
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    fn on_exit(&mut self) {
        save_prefs(&self.0);
    }
}

fn config_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("gridcraft"))
    }
}

/// The operating system's locale tag (`pt-BR`); en-US when it cannot be read.
fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
}

/// Runs without preferences (`GRIDCRAFT_NO_PREFS`, agents' test runs) neither read nor write
/// them, and keep no log file.
fn prefs_enabled() -> bool {
    std::env::var_os("GRIDCRAFT_NO_PREFS").is_none()
}

/// Where the log files live: `logs` in the settings folder, next to `ui.json` (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join("logs"))
}

fn read_prefs(dir: &std::path::Path) -> Option<Prefs> {
    serde_json::from_slice(&std::fs::read(dir.join("prefs.json")).ok()?).ok()
}

fn write_prefs(dir: &std::path::Path, prefs: &Prefs) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(b) = serde_json::to_vec_pretty(prefs) {
        let _ = std::fs::write(dir.join("prefs.json"), b);
    }
}

/// Loads `prefs.json` into the session, then resolves the language and regional format ("system"
/// follows the operating system) so that workbooks created afterwards use them.
fn load_session_prefs(session: &mut Session) {
    if prefs_enabled()
        && let Some(dir) = config_dir()
        && let Some(p) = read_prefs(&dir)
    {
        session.prefs = p;
    }
    session.set_system_locale(&system_locale());
}

fn load_ui_prefs(app: &mut SheetApp) {
    if !prefs_enabled() {
        return;
    }
    let Some(dir) = config_dir() else { return };
    if let Ok(b) = std::fs::read(dir.join("ui.json"))
        && let Ok(saved) = serde_json::from_slice::<serde_json::Value>(&b)
    {
        if let Ok(ui) = <gridcraft_ui_egui::UiState as serde::Deserialize>::deserialize(&saved) {
            app.ui = ui;
        }
        if app.migrate_ui_language(&saved) {
            // Persist both files at once: ui.json without the old `language` key, so a crash
            // before exit can't migrate it again over a later choice of "system".
            save_prefs(app);
        }
    }
}

fn save_prefs(app: &SheetApp) {
    if !prefs_enabled() {
        return;
    }
    let Some(dir) = config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(b) = serde_json::to_vec_pretty(&app.ui) {
        let _ = std::fs::write(dir.join("ui.json"), b);
    }
    write_prefs(&dir, &app.session.prefs);
}

fn services() -> Services {
    // Keep the owner alive: X11/Wayland serve clipboard data from this handle.
    let mut clipboard: Option<arboard::Clipboard> = None;
    Services {
        copy_html: Some(Box::new(move |html, text| {
            if clipboard.is_none() {
                clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
            }
            clipboard.as_mut().ok_or("clipboard unavailable")?.set_html(html, Some(text)).map_err(|e| e.to_string())
        })),
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Spreadsheets", &["xlsx", "xlsm", "xlsb", "ods", "csv", "tsv", "txt", "json"])
                .add_filter("Excel Workbook", &["xlsx", "xlsm"])
                .add_filter("Excel Binary Workbook (data import)", &["xlsb"])
                .add_filter("OpenDocument Spreadsheet (data import)", &["ods"])
                .add_filter("CSV", &["csv"])
                .pick_file()
                .and_then(|p| p.to_str().map(str::to_string))
        })),
        pick_picture: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Pictures (PNG, JPEG)", &["png", "jpg", "jpeg"])
                .pick_file()
                .and_then(|p| p.to_str().map(str::to_string))
        })),
        pick_save: Some(Box::new(|suggested: &str| {
            let stem = std::path::Path::new(suggested).file_stem().and_then(|s| s.to_str()).unwrap_or("Book1").to_string();
            rfd::FileDialog::new()
                .set_file_name(format!("{stem}.xlsx"))
                .add_filter("Excel Workbook (.xlsx)", &["xlsx"])
                .add_filter("CSV UTF-8 (.csv)", &["csv"])
                .add_filter("Web Page (.html)", &["html"])
                .save_file()
                .and_then(|p| p.to_str().map(str::to_string))
        })),
        open_url: Some(Box::new(|url: &str| {
            let cmd = if cfg!(target_os = "macos") {
                "open"
            } else if cfg!(windows) {
                "explorer"
            } else {
                "xdg-open"
            };
            let _ = std::process::Command::new(cmd).arg(url).spawn();
        })),
        ..Default::default()
    }
}

fn icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.gridcraft.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() -> eframe::Result<()> {
    // First, so the panic hook and every start-up warning are recorded (`logging`).
    let logger = logging::install();
    // Commands catch their panics (`Session::execute`), but the default hook only prints to
    // standard error; this one sends every panic, caught or not, to the log file too, with a
    // backtrace when RUST_BACKTRACE is set.
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::capture();
        let report = if trace.status() == std::backtrace::BacktraceStatus::Captured {
            format!("internal error: {info}\n{trace}")
        } else {
            format!("internal error: {info}")
        };
        // Standard error and the log file; standard error alone when RUST_LOG turned errors off.
        if log::log_enabled!(log::Level::Error) {
            log::error!("{report}");
        } else {
            // `eprintln!` panics on a broken stderr pipe, and a panic inside the panic hook aborts.
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("gridcraft: {report}\n"));
        }
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GridCraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut control_port: Option<u16> = std::env::var("GRIDCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut sample: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--control" => {
                i += 1;
                control_port = args.get(i).and_then(|p| p.parse().ok());
            }
            "--sample" => {
                i += 1;
                sample = Some(args.get(i).cloned().unwrap_or_else(|| "sales".into()));
            }
            a if a.starts_with("-psn_") => {}
            a => files.push(a.to_string()),
        }
        i += 1;
    }
    // The log file lives in the settings folder, next to the preferences; opened after the
    // arguments, so `--version` leaves no file behind. Records logged until now are written to it
    // first. Runs without preferences (agents' test runs) log to standard error only, so they
    // don't rotate away the user's own logs.
    if let Some(logger) = logger {
        match log_dir().filter(|_| prefs_enabled()) {
            Some(dir) => match logger.attach_dir(&dir) {
                Ok(path) => log::info!("GridCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
                // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
                Err(e) => log::warn!("no log file: {e}"),
            },
            None => logger.no_file(),
        }
    }
    let mut session = Session::new();
    load_session_prefs(&mut session);
    if let Some(s) = &sample
        && let Err(e) = session.execute("file.new", json!({"sample": s}))
    {
        log::warn!("sample {s}: {e}");
    }
    for f in &files {
        if let Err(e) = session.execute("file.open", json!({"path": f})) {
            log::warn!("{f}: {e}");
        }
    }
    if session.documents().is_empty() {
        session.new_workbook();
    }
    let mut app = SheetApp::new(session, services());
    app.after_engine(); // Show warnings from files opened on the command line.
    let control_port = control_port;
    load_ui_prefs(&mut app);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GridCraft")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([640.0, 420.0])
        .with_app_id("ai.storyteller.gridcraft")
        .with_drag_and_drop(true);
    if cfg!(target_os = "macos") {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 only (see graphics.rs).
    #[cfg(target_os = "windows")]
    let options = {
        let mut options = options;
        graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
        options
    };
    // Registered before the event loop starts, so it catches the Finder event that launched us as
    // well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    eframe::run_native(
        "GridCraft",
        options,
        Box::new(move |cc| {
            SheetApp::setup_context(&cc.egui_ctx, app.ui.dark);
            if let Some(port) = control_port {
                match control_server::start(port, cc.egui_ctx.clone()) {
                    Ok(rx) => app.control_rx = Some(rx),
                    Err(e) => log::error!("control channel on port {port}: {e}"),
                }
            }
            Ok(Box::new(App(
                app,
                #[cfg(target_os = "macos")]
                None,
                #[cfg(target_os = "macos")]
                apple_events.connect(&cc.egui_ctx),
            )))
        }),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-app-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn language_prefs_survive_prefs_json() {
        let dir = temp_dir("prefs");
        let p = Prefs {
            ui_language: "pt-BR".into(),
            formula_language: "en-US".into(),
            regional_format: "de-DE".into(),
            use_system_separators: false,
            decimal_separator: ",".into(),
            thousands_separator: ".".into(),
            ..Prefs::default()
        };
        write_prefs(&dir, &p);
        let text = std::fs::read_to_string(dir.join("prefs.json")).unwrap();
        for field in ["uiLanguage", "formulaLanguage", "regionalFormat", "useSystemSeparators", "decimalSeparator", "thousandsSeparator"] {
            assert!(text.contains(field), "{field} missing from {text}");
        }
        let back = read_prefs(&dir).unwrap();
        assert_eq!(back.ui_language, "pt-BR");
        assert_eq!(back.formula_language, "en-US");
        assert_eq!(back.regional_format, "de-DE");
        assert!(!back.use_system_separators);
        assert_eq!((back.decimal_separator.as_str(), back.thousands_separator.as_str()), (",", "."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefs_file_from_an_older_version_gets_system_defaults() {
        let dir = temp_dir("old-prefs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prefs.json"), r#"{"autocomplete": false}"#).unwrap();
        let p = read_prefs(&dir).unwrap();
        assert!(!p.autocomplete);
        assert_eq!((p.ui_language.as_str(), p.formula_language.as_str(), p.regional_format.as_str()), ("system", "followUi", "system"));
        assert!(p.use_system_separators);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_choice_wins_over_the_system_locale_and_system_follows_it() {
        let mut saved = Session::new();
        saved.prefs.regional_format = "pt-BR".into();
        saved.set_system_locale("en-US");
        assert_eq!(saved.locale().regional.tag, "pt-BR");

        let mut follows = Session::new();
        follows.set_system_locale("de_DE.UTF-8");
        assert_eq!(follows.locale().regional.tag, "de-DE");
        // The system language changing between starts is picked up again.
        follows.set_system_locale("fr-FR");
        assert_eq!(follows.locale().regional.tag, "fr-FR");
    }
}
