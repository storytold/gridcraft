//! GridCraft desktop app.
//!
//! Usage: `gridcraft [--control <port> [--control-token <token>]] [--sample <name>] [files…]`
//!
//! `--control <port>` (or `GRIDCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! The first line of each connection must carry the token (`docs/control-protocol.md`).
//! See `gridcraft_ui_egui::control` and `docs/control-protocol.md` for the methods.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
mod apple_events;
mod control_server;
#[cfg(any(target_os = "windows", target_os = "macos", test))]
mod graphics;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod print;
mod user_name;

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_engine::{DEFAULT_USER_NAME, Session};
use gridcraft_ui_egui::doc_windows::DocWindows;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

/// The app, its native window per workbook window, and (macOS) the menu bar and Finder events.
struct App(SheetApp, DocWindows, #[cfg(target_os = "macos")] Option<native_menu::NativeMenu>, #[cfg(target_os = "macos")] fmv_macos_events::Inbox);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.2.is_none() && std::env::var_os("GRIDCRAFT_NO_NATIVE_MENU").is_none() {
                self.2 = Some(native_menu::NativeMenu::install(ctx));
            }
            if let Some(m) = &mut self.2 {
                m.poll(&mut self.0, ctx, self.1.focused_viewport());
            }
            apple_events::poll(&self.3, &mut self.0, ctx);
        }
        self.0.logic(ctx);
        self.1.logic(&mut self.0, ctx);
        if ctx.input(|input| input.viewport().close_requested()) && self.0.printing.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
    /// Forwards raw input to the shared app, then opens every non-empty dropped file path as a workbook.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(ctx, raw);
        // Files dropped on the window open as workbooks.
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().map(str::to_string).filter(|p| !p.is_empty()) {
                self.0.open_path(&p);
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.1.show(&mut self.0, ui);
    }
    fn on_exit(&mut self) {
        print::finish_pending_jobs();
        save_prefs(&self.0);
        // A ready update installs once the process has exited. The macOS exit below skips
        // dropping the app, so the helper is started here.
        self.0.start_pending_update();
        // macOS (#117): end the process here, before eframe tears the window down. AccessKit's
        // macOS adapter subclasses the window's content view and restores its class on teardown;
        // AppKit's Touch Bar support observes that view (KVO), so in the next display cycle
        // `-[_NSTouchBarFinderObservation invalidate]` throws "not registered" and the quit ends
        // in a crash report. eframe calls `on_exit` on every way of quitting (Quit/⌘Q, the Dock,
        // the close button, `app.quit`), and nothing after it is needed: the preferences are
        // saved and the log file is unbuffered. eframe does the same when it doesn't
        // run-and-return. `on_exit` only runs once the app exists, never on a failed start.
        #[cfg(target_os = "macos")]
        std::process::exit(0);
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

/// Runs without preferences (`GRIDCRAFT_NO_PREFS`, agents' test runs) neither read nor write
/// them, and keep no log file.
fn prefs_enabled() -> bool {
    std::env::var_os("GRIDCRAFT_NO_PREFS").is_none()
}

/// Where the log files live: `logs` in the settings folder, next to `ui.json` (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join("logs"))
}

fn load_prefs(app: &mut SheetApp) {
    if !prefs_enabled() {
        return;
    }
    // First run (or a ui.json without a usable language): follow the desktop's language.
    app.ui.language = gridcraft_ui_egui::i18n::Language::system();
    // ...and its light or dark appearance (#239), until a theme is chosen and saved in ui.json.
    app.ui.system_theme = true;
    let Some(dir) = config_dir() else { return };
    if let Ok(b) = std::fs::read(dir.join("ui.json"))
        && let Ok(ui) = serde_json::from_slice::<gridcraft_ui_egui::UiState>(&b)
    {
        let saved = serde_json::from_slice(&b).ok().and_then(|v| gridcraft_ui_egui::i18n::saved_language(&v));
        let language = saved.unwrap_or(app.ui.language);
        app.ui = ui.at_startup();
        app.ui.language = language;
    }
    if let Ok(b) = std::fs::read(dir.join("prefs.json"))
        && let Ok(p) = serde_json::from_slice(&b)
    {
        app.session.prefs = p;
    }
}

/// The longest user name kept, in characters — the same cap PR #3550's `options.userName` puts on
/// a name typed into File › Options. It is why a cap is needed at all: the name goes into the
/// workbook properties and next to every note, comment and revision, and the system's GECOS field
/// is free text someone may have filled with anything; an unbounded one would sit in the file and
/// in every comment box forever. Only the kept characters are copied, however long `name` is.
const MAX_USER_NAME_CHARS: usize = 200;

/// A user name as the preferences keep it, before it is saved: control characters (tabs, line
/// breaks) become spaces, the ends are trimmed and at most [`MAX_USER_NAME_CHARS`] characters are
/// kept; `None` when nothing but spaces is left. This is the shape PR #3550's `clean_user_name`
/// will give a name typed into File › Options; the engine gets its own with that command, and
/// until it lands the system's name is cleaned here so both ways keep a name the same.
fn clean_user_name(name: &str) -> Option<String> {
    let name = name.trim_matches(|c: char| c.is_whitespace() || c.is_control());
    let kept: String = name.chars().take(MAX_USER_NAME_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
    let kept = kept.trim_end();
    (!kept.is_empty()).then(|| kept.to_string())
}

/// The system account's name ([`user_name`], #1449) while the user name is still unset: on the
/// first start, or with preferences saved before there was a name to keep (they hold
/// [`DEFAULT_USER_NAME`]). A name kept with the preferences always wins, and the system is asked
/// only until one is saved: `save_prefs` writes the name from then on. A name of only spaces
/// counts as unset (File › Options will not keep one), but an empty name is the person's own
/// choice to sign with nothing, and stays. The name goes through [`clean_user_name`], as one
/// typed in Options will be. A run without preferences never asks the system, so what it saves or
/// shows stays the same on every machine; that decision arrives as `prefs_on` (the caller's
/// [`prefs_enabled`]) rather than being read from the environment here, so the tests cover both
/// kinds of run.
fn use_system_user_name(app: &mut SheetApp, prefs_on: bool, system: impl FnOnce() -> Option<String>) {
    let kept = &app.session.prefs.user_name;
    let unset = kept == DEFAULT_USER_NAME || (!kept.is_empty() && kept.trim().is_empty());
    if !prefs_on || !unset {
        return;
    }
    if let Some(name) = system().as_deref().and_then(clean_user_name) {
        // The name itself stays out of the log: `gridcraft.log` is kept and rotated on the machine
        // of whoever runs it, and would carry a personal name long after the preference changed.
        log::debug!("user name: this start takes the account's name");
        app.session.prefs.user_name = name;
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
    if let Ok(b) = serde_json::to_vec_pretty(&app.session.prefs) {
        let _ = std::fs::write(dir.join("prefs.json"), b);
    }
}

/// Read the system clipboard through the shared handle, creating it on first use.
fn read_clipboard<T>(
    handle: &RefCell<Option<arboard::Clipboard>>,
    read: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Option<T> {
    let mut clipboard = handle.try_borrow_mut().ok()?;
    if clipboard.is_none() {
        *clipboard = arboard::Clipboard::new().ok();
    }
    read(clipboard.as_mut()?).ok()
}

fn services() -> Services {
    // Keep the owner alive: X11/Wayland serve clipboard data from this handle.
    let clipboard: Rc<RefCell<Option<arboard::Clipboard>>> = Rc::default();
    let (html_clipboard, text_clipboard, image_clipboard) = (clipboard.clone(), clipboard.clone(), clipboard.clone());
    Services {
        copy_html: Some(Box::new(move |html, text| {
            let mut clipboard = clipboard.borrow_mut();
            if clipboard.is_none() {
                *clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
            }
            clipboard.as_mut().ok_or("clipboard unavailable")?.set_html(html, Some(text)).map_err(|e| e.to_string())
        })),
        paste_html: Some(Box::new(move || read_clipboard(&html_clipboard, |clipboard| clipboard.get().html()))),
        paste_text: Some(Box::new(move || read_clipboard(&text_clipboard, |clipboard| clipboard.get_text()))),
        paste_image: Some(Box::new(move || {
            let image = read_clipboard(&image_clipboard, |clipboard| clipboard.get_image())?;
            image::RgbaImage::from_raw(u32::try_from(image.width).ok()?, u32::try_from(image.height).ok()?, image.bytes.into_owned())
        })),
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "slk", "json"])
                .add_filter("Excel Workbook", &["xlsx", "xlsm"])
                .add_filter("Excel 97-2003 Workbook (import)", &["xls"])
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
            let path = std::path::Path::new(suggested);
            let stem = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book1").to_string();
            // The suggested name's type (an export asks for .pdf, .csv or .html) comes first.
            const TYPES: [(&str, &str); 4] =
                [("Excel Workbook (.xlsx)", "xlsx"), ("CSV UTF-8 (.csv)", "csv"), ("Web Page (.html)", "html"), ("PDF (.pdf)", "pdf")];
            let ext = path.extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase).filter(|e| TYPES.iter().any(|(_, x)| x == e));
            let ext = ext.unwrap_or_else(|| "xlsx".to_string());
            let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.{ext}"));
            for (label, x) in TYPES.iter().filter(|(_, x)| *x == ext).chain(TYPES.iter().filter(|(_, x)| *x != ext)) {
                dialog = dialog.add_filter(*label, &[*x]);
            }
            dialog.save_file().and_then(|p| p.to_str().map(str::to_string))
        })),
        open_url: Some(Box::new(|url: &str| open_with_system(url))),
        printers: Some(print::discover()),
        print_native: Some(print::submit()),
        ..Default::default()
    }
}

/// Opens a URL or file with the system's default application.
fn open_with_system(target: &str) {
    // Last line of defence behind `link.open` and the click prompt: the system only ever gets
    // http, https and mailto addresses.
    let gridcraft_engine::links::LinkKind::External(target) = gridcraft_engine::links::classify(target) else {
        log::warn!("refused to open {target:?}: only http, https and mailto links are opened");
        return;
    };
    let target = target.as_str();
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(cmd).arg(target).spawn() {
        log::warn!("{cmd} {target}: {e}");
    }
}

fn icon() -> Option<egui::IconData> {
    #[cfg(target_os = "macos")]
    let bytes = include_bytes!("../../../assets/app-icon/gridcraft-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let bytes = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.gridcraft.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() -> eframe::Result<()> {
    // A confined launcher run (`gridcraft_sandbox::launch_command`) comes before everything else:
    // it opens no window, writes no log, and becomes the program it confines.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = gridcraft_sandbox::handle_args(&raw) {
        std::process::exit(code);
    }
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
    // The restart helper a self-update spawns: it finishes the swap and relaunches, then exits.
    if let Some(done) = craft_update::self_update::finish(&gridcraft_ui_egui::UPDATE_APP, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        if let Err(e) = done {
            eprintln!("GridCraft update helper failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GridCraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut control_port: Option<u16> = std::env::var("GRIDCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut control_token: Option<String> = std::env::var("GRIDCRAFT_CONTROL_TOKEN").ok();
    let mut sample: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--control" => {
                i += 1;
                control_port = args.get(i).and_then(|p| p.parse().ok());
            }
            "--control-token" => {
                i += 1;
                control_token = args.get(i).cloned();
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
    if let Some(s) = &sample
        && let Err(e) = session.execute("file.new", json!({"sample": s}))
    {
        log::warn!("sample {s}: {e}");
    }
    let mut app = SheetApp::new(session, services());
    app.watch_desktop_theme();
    app.start_update_check();
    let control_port = control_port;
    load_prefs(&mut app);
    use_system_user_name(&mut app, prefs_enabled(), user_name::system);
    // Stamp the startup workbook's author from the persisted user name.
    if app.session.documents().is_empty() {
        app.session.new_workbook();
    }
    app.after_engine(); // Show warnings from the sample.
    // Files named on the command line (a double-click on Windows and Linux) open as File › Open
    // opens them: a password-protected one asks for its password, and errors are shown, not
    // only logged. Each replaces the untouched blank Book1, as a file opened from Finder does.
    for f in &files {
        app.open_path(f);
    }
    // Linux/BSD: Excel's layout — no window-manager title bar above ours; our title bar carries the
    // window controls. `GRIDCRAFT_SYSTEM_TITLEBAR=1` keeps the system frame.
    app.window_controls = cfg!(not(any(target_os = "macos", target_os = "windows"))) && std::env::var_os("GRIDCRAFT_SYSTEM_TITLEBAR").is_none();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GridCraft")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([640.0, 420.0])
        .with_app_id("ai.storyteller.gridcraft")
        .with_drag_and_drop(true)
        .with_decorations(!app.window_controls);
    if cfg!(target_os = "macos") {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let window_builder = viewport.clone();
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(unix)]
    let mut options = options;
    // Intel Macs: Metal's compiler aborts on wgpu's indirect-validation shaders (see graphics.rs).
    #[cfg(target_os = "macos")]
    graphics::configure_indirect_validation(&mut options, graphics::skips_indirect_validation(true, cfg!(target_arch = "x86_64")));
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; GRIDCRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) && std::env::var_os("GRIDCRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Before eframe creates the wgpu instance: Windows starts on DirectX 12, or OpenGL when
    // DirectX 12 has no GPU driver, and never touches Vulkan unless asked (see graphics.rs).
    #[cfg(target_os = "windows")]
    let backend_override = eframe::wgpu::Backends::from_env();
    #[cfg(target_os = "windows")]
    let (options, first_backends) = {
        let mut options = options;
        let backends = graphics::configure(&mut options, backend_override);
        (options, backends)
    };
    #[cfg(target_os = "windows")]
    let retry_options = options.clone();
    // Registered before the event loop starts, so it catches the Finder event that launched us as
    // well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    // The app waits here until a start gets as far as creating it; a start that fails before
    // (no graphics adapter, device or surface) leaves it here for the next attempt.
    let pending = std::cell::Cell::new(Some(app));
    let run = |options: eframe::NativeOptions| {
        eframe::run_native(
            "GridCraft",
            options,
            Box::new(|cc| {
                let Some(mut app) = pending.take() else {
                    return Err("the app was already created".into());
                };
                SheetApp::setup_context_for_language(&cc.egui_ctx, app.ui.dark, app.ui.language);
                if let Some(port) = control_port {
                    match control_server::start(port, control_token.clone(), cc.egui_ctx.clone()) {
                        Ok(rx) => app.control_rx = Some(rx),
                        Err(e) => log::error!("control channel on port {port}: {e}"),
                    }
                }
                let windows = DocWindows::new(&mut app, window_builder.clone(), DocWindows::platform_quits_with_last_window());
                Ok(Box::new(App(
                    app,
                    windows,
                    #[cfg(target_os = "macos")]
                    None,
                    #[cfg(target_os = "macos")]
                    apple_events.connect(&cc.egui_ctx),
                )))
            }),
        )
    };
    let result = run(options);
    // A graphics error before the app existed: start once more with the other backend (eframe
    // reuses its event loop for the second run). Errors after that, or from winit, are returned.
    #[cfg(target_os = "windows")]
    if let Err(eframe::Error::Wgpu(e)) = &result
        && let Some(app) = pending.take()
        && let Some(backends) = graphics::fallback_backends(backend_override, first_backends)
    {
        log::error!("graphics: the renderer didn't start on {first_backends:?} ({e}); trying {backends:?}");
        pending.set(Some(app));
        let mut options = retry_options;
        graphics::configure_backends(&mut options, backends);
        return run(options);
    }
    result
}

#[cfg(test)]
mod packaging_tests {
    use gridcraft_engine::io::FileKind;

    /// The text between `start` and `end` that follows each `marker` in `text`, sorted (e.g. the
    /// `xlsb` in `Classes\.xlsb\OpenWithProgids`).
    fn extensions(text: &str, marker: &str, start: &str, end: char) -> Vec<String> {
        let mut v: Vec<String> = text.split(marker).skip(1).filter_map(|s| s.split_once(start)?.1.split(end).next()).map(str::to_string).collect();
        v.sort();
        v
    }

    /// macOS Open With (`CFBundleDocumentTypes`) offers GridCraft for the same files as Windows
    /// Open with, and GridCraft opens each of them. #321 added `.xlsb` and `.ods` on Windows and
    /// Linux only.
    #[test]
    fn macos_and_windows_offer_the_same_file_types() {
        let plist = include_str!("../../../packaging/macos/Info.plist.in");
        let wxs = include_str!("../../../packaging/windows/gridcraft.wxs");
        let mac = extensions(plist, "<key>CFBundleTypeExtensions</key>", "<string>", '<');
        let win = extensions(wxs, "Key=\"Software\\Classes\\.", "", '\\');
        assert!(mac.iter().any(|e| e == "xlsx"), "Info.plist.in extensions not found: {mac:?}");
        assert_eq!(mac, win);
        for ext in &mac {
            assert!(FileKind::from_path(&format!("book.{ext}")).is_some(), "GridCraft doesn't open .{ext}");
        }
    }
}

#[cfg(test)]
mod user_name_tests {
    use super::*;

    fn app() -> SheetApp {
        SheetApp::new(Session::new(), Services::default())
    }

    #[test]
    fn the_system_user_name_replaces_only_the_default() {
        let mut a = app();
        use_system_user_name(&mut a, true, || Some("  Ann\tLee \n".into()));
        assert_eq!(a.session.prefs.user_name, "Ann Lee", "cleaned as a name typed in Options will be");

        // A chosen name, saved or taken this run, never asks the system again.
        let mut asked = false;
        use_system_user_name(&mut a, true, || {
            asked = true;
            Some("Grace Hopper".into())
        });
        assert!(!asked);
        assert_eq!(a.session.prefs.user_name, "Ann Lee");
        let mut b = app();
        b.session.prefs = serde_json::from_str(r#"{"userName": "Grace Hopper"}"#).unwrap();
        use_system_user_name(&mut b, true, || Some("Ada Lovelace".into()));
        assert_eq!(b.session.prefs.user_name, "Grace Hopper", "a saved name wins");
    }

    #[test]
    fn preferences_holding_the_default_name_take_the_system_one() {
        // Saved before #1449, when every run saved the default; or a name of only spaces, which
        // File › Options never keeps; or a prefs.json without the name at all (serde default).
        for saved in [r#"{"userName": "GridCraft User"}"#, r#"{"userName": "  "}"#, r#"{}"#] {
            let mut a = app();
            a.session.prefs = serde_json::from_str(saved).unwrap();
            use_system_user_name(&mut a, true, || Some("Ada Lovelace".into()));
            assert_eq!(a.session.prefs.user_name, "Ada Lovelace", "{saved}");
        }
    }

    #[test]
    fn a_name_cleared_in_options_stays_cleared() {
        // An empty name in prefs.json is the person's own choice, not a missing name (#1449).
        let mut a = app();
        a.session.prefs = serde_json::from_str(r#"{"userName": ""}"#).unwrap();
        use_system_user_name(&mut a, true, || Some("Ada Lovelace".into()));
        assert_eq!(a.session.prefs.user_name, "", "the cleared name stays cleared");
    }

    #[test]
    fn a_run_without_preferences_never_asks_the_system() {
        // Agents' and test runs (`GRIDCRAFT_NO_PREFS`, which `prefs_enabled` reads) keep the
        // default, so what they save or show stays the same on every machine.
        let mut asked = false;
        let mut a = app();
        use_system_user_name(&mut a, false, || {
            asked = true;
            Some("Ada Lovelace".into())
        });
        assert!(!asked, "no lookup at all");
        assert_eq!(a.session.prefs.user_name, DEFAULT_USER_NAME);
    }

    #[test]
    fn no_usable_system_name_keeps_the_default() {
        for system in [None, Some(String::new()), Some(" \t\n".into()), Some("\u{0}\u{1b}".into())] {
            let mut a = app();
            use_system_user_name(&mut a, true, || system.clone());
            assert_eq!(a.session.prefs.user_name, DEFAULT_USER_NAME, "{system:?}");
        }
    }

    #[test]
    fn a_long_system_name_is_cut_like_a_typed_one_will_be() {
        // Characters, not bytes, as PR #3550's `clean_user_name` counts them.
        let mut a = app();
        use_system_user_name(&mut a, true, || Some("é".repeat(500)));
        let taken = a.session.prefs.user_name;
        assert_eq!(taken.chars().count(), MAX_USER_NAME_CHARS);
        assert_eq!(taken, "é".repeat(MAX_USER_NAME_CHARS));
    }
}
