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

mod app_dirs;
#[cfg(target_os = "macos")]
mod apple_events;
// The build channel's own parser and icon-path tests (build.rs compiles the same file by path).
#[cfg(test)]
mod build_channel;
mod build_flavor;
mod control_server;
#[cfg(any(target_os = "windows", target_os = "macos", test))]
mod graphics;
mod launch_profile;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod nightly_session;
mod opener;
mod print;
mod process_identity;
mod profile_root;

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_engine::Session;
use gridcraft_ui_egui::doc_windows::DocWindows;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

/// The app, its native window per workbook window, and (macOS) the menu bar and Finder events.
/// The session record and its notice flag are only meaningful in the nightly channel.
struct App {
    app: SheetApp,
    windows: DocWindows,
    #[cfg(target_os = "macos")]
    menu: Option<native_menu::NativeMenu>,
    #[cfg(target_os = "macos")]
    apple_events: fmv_macos_events::Inbox,
    /// The Nightly session record, shared with the exit hook and the return path.
    nightly: nightly_session::Shared,
    /// Whether this window already told the user the previous session did not finish.
    nightly_notice_shown: bool,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.menu.is_none() && std::env::var_os("GRIDCRAFT_NO_NATIVE_MENU").is_none() {
                self.menu = Some(native_menu::NativeMenu::install(ctx));
            }
            if let Some(m) = &mut self.menu {
                m.poll(&mut self.app, ctx, self.windows.focused_viewport());
            }
            apple_events::poll(&self.apple_events, &mut self.app, ctx);
        }
        self.app.logic(ctx);
        self.windows.logic(&mut self.app, ctx);
        if ctx.input(|input| input.viewport().close_requested()) && self.app.printing.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
    /// Forwards raw input to the shared app, then opens every non-empty dropped file path as a workbook.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
        // Files dropped on the window open as workbooks.
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().map(str::to_string).filter(|p| !p.is_empty()) {
                self.app.open_path(&p);
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.windows.show(&mut self.app, ui);
    }
    fn on_exit(&mut self) {
        print::finish_pending_jobs();
        save_prefs(&self.app);
        // A ready update installs once the process has exited. The macOS exit below skips
        // dropping the app, so the helper is started here. A nightly has no updater at all.
        if gridcraft_ui_egui::channel::stable_services_allowed() {
            self.app.start_pending_update();
        }
        // The Nightly session record is finished here: every way of quitting reaches `on_exit`,
        // and on macOS AppKit's terminate ends the process right after it, so nothing after
        // `eframe::run_native` can run there. `finish_once` makes the later call a no-op.
        nightly_session::finish_once(&self.nightly, self.nightly_notice_shown);
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

/// The settings folder: the validated nightly profile, else the platform's own folder
/// (`app_dirs`). Every writer of user state goes through it.
fn config_dir() -> Option<std::path::PathBuf> {
    app_dirs::config_dir()
}

/// Runs without preferences (`GRIDCRAFT_NO_PREFS`, agents' test runs) neither read nor write
/// them, and keep no log file.
fn prefs_enabled() -> bool {
    std::env::var_os("GRIDCRAFT_NO_PREFS").is_none()
}

/// Where the log files live: `logs` in the settings folder, next to `ui.json` (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    app_dirs::log_dir()
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
    // By absolute path where the platform has one (#2176): a bare `explorer` would be looked up
    // in the application's directory first.
    let env = |key: &str| std::env::var(key).ok();
    let (system_root, windir) = (env("SystemRoot"), env("windir"));
    let Some((program, arg)) = opener::opener_command(opener::Os::current(), &target, system_root.as_deref(), windir.as_deref()) else {
        log::warn!("refused to open {target:?}: it could be read as an option");
        return;
    };
    if let Err(e) = std::process::Command::new(&program).arg(&arg).spawn() {
        log::warn!("{} {arg}: {e}", program.display());
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

fn initialize_nightly_process_identity() -> Result<(), process_identity::IdentityError> {
    process_identity::initialize(
        build_flavor::COMPILED == build_flavor::BuildFlavor::NightlyV1,
        gridcraft_ui_egui::channel::id_for(gridcraft_ui_egui::channel::Channel::NightlyV1),
    )
}

/// Tell a desktop user why a nightly launch was refused. A headless form (`--version` on a runner)
/// never gets here: the version query is answered before this and before any profile work.
fn report_launch_profile_error(error: &str) {
    eprintln!("GridCraft launch profile: {error}");
    let desktop_session = cfg!(windows)
        || cfg!(target_os = "macos")
        || (cfg!(target_os = "linux") && ["DISPLAY", "WAYLAND_DISPLAY"].into_iter().any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty())));
    if build_flavor::needs_native_error(build_flavor::COMPILED, desktop_session) {
        if let Err(error) = initialize_nightly_process_identity() {
            eprintln!("GridCraft launch profile: {error}");
            return;
        }
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        let _ = rfd::MessageDialog::new()
            .set_title("GridCraft Nightly")
            .set_description(error)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// Select the immutable launch channel and profile before any startup service or side effect.
/// `None` means the answer was printed (`--version`) and the process is done.
fn prepare_launch() -> Option<Vec<std::ffi::OsString>> {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let nightly_build = build_flavor::COMPILED == build_flavor::BuildFlavor::NightlyV1;
    // Plain `--version`, or the Launcher's `--launch-profile nightly-v1 --profile-root <dir>
    // --version`: answered before root validation, process identity, IPC or any window (a dialog
    // here would hang a headless runner and fail a Linux build).
    if nightly_build && (build_flavor::version_query(&argv) || launch_profile::version_query(&argv)) {
        use std::io::Write as _;
        let version = build_flavor::version(build_flavor::COMPILED, env!("GRIDCRAFT_BUILT_VERSION"));
        let _ = writeln!(std::io::stdout(), "{} {version}", gridcraft_ui_egui::channel::name_for(gridcraft_ui_egui::channel::Channel::Stable));
        return None;
    }
    let early = launch_profile::parse(argv)
        .and_then(|launch| {
            build_flavor::require_profile(build_flavor::COMPILED, &launch.profile)?;
            initialize_nightly_process_identity().map_err(|error| error.to_string())?;
            Ok(launch)
        })
        .and_then(|launch| {
            launch_profile::prepare(launch, |root| {
                launch_profile::validate_root(root, |key| std::env::var_os(key), std::env::current_exe().ok().as_deref())
            })
        })
        .unwrap_or_else(|error| {
            report_launch_profile_error(&error);
            std::process::exit(2)
        });
    let channel =
        if early.profile.is_nightly() { gridcraft_ui_egui::channel::Channel::NightlyV1 } else { gridcraft_ui_egui::channel::Channel::Stable };
    if let Err(error) =
        launch_profile::install(early.profile).and_then(|()| gridcraft_ui_egui::channel::select(channel, env!("GRIDCRAFT_BUILT_VERSION")))
    {
        report_launch_profile_error(&error);
        std::process::exit(2);
    }
    Some(early.args)
}

fn main() -> eframe::Result<()> {
    // A confined launcher run (`gridcraft_sandbox::launch_command`, which a Python cell starts from
    // this same executable) comes before everything else: it opens no window, writes no log, and
    // becomes the program it confines. It must stay ahead of the launch profile, or a nightly build
    // would refuse its own sandbox helper.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = gridcraft_sandbox::handle_args(&raw) {
        std::process::exit(code);
    }
    // Then the Launcher protocol and the version query (see `prepare_launch`).
    let Some(arguments) = prepare_launch() else { return Ok(()) };
    // A nightly profile owns its control tokens as well, so even a nightly build that is asked
    // for a control server can never write the stable app's token file.
    if let Some(root) = launch_profile::current().root() {
        match profile_root::control_directory(root).and_then(gridcraft_mcp::control_auth::set_token_root) {
            Ok(()) => {}
            Err(error) => {
                report_launch_profile_error(&error);
                std::process::exit(2);
            }
        }
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
    let args: Vec<String> = arguments.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let nightly = gridcraft_ui_egui::channel::current() == gridcraft_ui_egui::channel::Channel::NightlyV1;
    // The restart helper a self-update spawns: it finishes the swap and relaunches, then exits.
    // A nightly has no updater, so it never runs this and never inherits a stable swap.
    if !nightly
        && let Some(done) = craft_update::self_update::finish(&gridcraft_ui_egui::UPDATE_APP, &std::env::args_os().skip(1).collect::<Vec<_>>())
    {
        if let Err(e) = done {
            eprintln!("GridCraft update helper failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // Before `--` only: everything after it is a file, even one named `--version`.
    if arguments.iter().take_while(|a| *a != "--").any(|a| a == "--version" || a == "-V") {
        println!("GridCraft {}", gridcraft_ui_egui::channel::version());
        return Ok(());
    }
    // The control channel is a stable-only service: a nightly window owns no port, writes no
    // token and never inherits the stable app's `GRIDCRAFT_CONTROL_PORT`/`GRIDCRAFT_CONTROL_TOKEN`.
    if nightly && args.iter().any(|a| a == "--control" || a == "--control-token") {
        eprintln!("GridCraft Nightly: the control channel is not part of the nightly channel");
        std::process::exit(2);
    }
    let mut control_port: Option<u16> = (!nightly).then(|| std::env::var("GRIDCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok())).flatten();
    let mut control_token: Option<String> = (!nightly).then(|| std::env::var("GRIDCRAFT_CONTROL_TOKEN").ok()).flatten();
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
                Ok(path) => {
                    log::info!("{} {}, log file {}", gridcraft_ui_egui::channel::name(), gridcraft_ui_egui::channel::version(), path.display())
                }
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
    // The updater is a stable-only service: a nightly is installed, replaced and updated by the
    // Launcher, so it never checks for one, never prompts and never writes craft-update's state.
    if !nightly {
        app.start_update_check();
    }
    let control_port = control_port;
    load_prefs(&mut app);
    // Stamp the startup workbook's author from the persisted user name.
    if app.session.documents().is_empty() {
        app.session.new_workbook();
    }
    app.after_engine(); // Show warnings from the sample.
    // Nightly session health: the record is written before the window exists and finished by
    // `on_exit`. A record left by an interrupted run is reported once, then cleared on the next
    // clean quit.
    let session_watch = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut nightly_notice_shown = false;
    match nightly_session::Session::begin_with_health(launch_profile::current().root(), env!("GRIDCRAFT_BUILT_VERSION")) {
        Ok((watch, unavailable)) => {
            if let Some(watch) = &watch
                && watch.previous_unclean()
            {
                app.notice = Some(gridcraft_ui_egui::Notice {
                    text: "The last GridCraft Nightly session did not finish cleanly. Your saved workbooks are as you left them; only unsaved changes are affected.".into(),
                    kind: gridcraft_ui_egui::NoticeKind::Warning,
                    action: None,
                });
                nightly_notice_shown = true;
            }
            if unavailable {
                log::warn!("this window shares a GridCraft Nightly profile another window owns; its exit is not recorded");
            }
            *session_watch.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = watch;
        }
        Err(error) => log::warn!("Nightly session state was not written: {error}"),
    }
    // Files named on the command line (a double-click on Windows and Linux) open as File › Open
    // opens them: a password-protected one asks for its password, and errors are shown, not
    // only logged. Each replaces the untouched blank Book1, as a file opened from Finder does.
    // They open on the first frame, once the fonts the grid draws with can measure them.
    app.open_on_first_frame(files);
    // Linux/BSD: Excel's layout — no window-manager title bar above ours; our title bar carries the
    // window controls. `GRIDCRAFT_SYSTEM_TITLEBAR=1` keeps the system frame.
    app.window_controls = cfg!(not(any(target_os = "macos", target_os = "windows"))) && std::env::var_os("GRIDCRAFT_SYSTEM_TITLEBAR").is_none();
    // The window carries its channel's name and OS app id: a nightly window is never mistaken for
    // the stable one by a dock, a window manager rule or the taskbar (Windows AUMID above). A
    // Flatpak package carries its own id instead, which is what its .desktop file and portal name.
    let window_app_id =
        std::env::var("FLATPAK_ID").ok().filter(|id| !id.is_empty()).unwrap_or_else(|| gridcraft_ui_egui::channel::app_id().to_string());
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(gridcraft_ui_egui::channel::default_title())
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([640.0, 420.0])
        .with_app_id(window_app_id)
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
    #[cfg(target_os = "macos")]
    let mut options = options;
    // Intel Macs: Metal's compiler aborts on wgpu's indirect-validation shaders (see graphics.rs).
    #[cfg(target_os = "macos")]
    graphics::configure_indirect_validation(&mut options, graphics::skips_indirect_validation(true, cfg!(target_arch = "x86_64")));
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
            gridcraft_ui_egui::channel::name(),
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
                Ok(Box::new(App {
                    app,
                    windows,
                    #[cfg(target_os = "macos")]
                    menu: None,
                    #[cfg(target_os = "macos")]
                    apple_events: apple_events.connect(&cc.egui_ctx),
                    nightly: session_watch.clone(),
                    nightly_notice_shown,
                }))
            }),
        )
    };
    let result = run(options);
    // A graphics error before the app existed: start once more with the other backend (eframe
    // reuses its event loop for the second run). Errors after that, or from winit, are returned.
    #[cfg(target_os = "windows")]
    let result = if let Err(eframe::Error::Wgpu(e)) = &result
        && let Some(app) = pending.take()
        && let Some(backends) = graphics::fallback_backends(backend_override, first_backends)
    {
        log::error!("graphics: the renderer didn't start on {first_backends:?} ({e}); trying {backends:?}");
        pending.set(Some(app));
        let mut options = retry_options;
        graphics::configure_backends(&mut options, backends);
        run(options)
    } else {
        result
    };
    // A start that failed before the app existed never reached `on_exit`, and on the platforms
    // where the event loop returns this is also the end of a clean exit. `finish_once` makes the
    // second of the two calls a no-op, so a successor's record is never touched.
    nightly_session::finish_once(&session_watch, nightly_notice_shown);
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
