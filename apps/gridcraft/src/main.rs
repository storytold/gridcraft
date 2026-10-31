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
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod print;

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_engine::Session;
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
    let (html_clipboard, text_clipboard) = (clipboard.clone(), clipboard.clone());
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
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"])
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

/// Whether to open the window on X11 (XWayland) rather than Wayland. winit has no file
/// drag-and-drop on Wayland (only on X11), so dropping files from the file manager showed a "no"
/// cursor. And GNOME's compositor draws no frames for Wayland clients, so there winit falls back to
/// its own bare frame: a plain strip with square buttons and no title; through XWayland, GNOME
/// draws its own title bar, themed and with the user's button layout. `GRIDCRAFT_WAYLAND=1` keeps
/// the native Wayland backend.
///
/// winit builds one event loop per process, so a failed X11 start can't fall back to Wayland:
/// X11 is only chosen when `usable` (`x11_usable`) confirms that it will start on the local
/// display. It is only asked on Wayland, without the opt-out.
#[cfg(all(unix, not(target_os = "macos")))]
fn prefer_x11(var: impl Fn(&str) -> Option<String>, usable: impl FnOnce(&str) -> bool) -> bool {
    let wayland = var("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty());
    let opt_out = var("GRIDCRAFT_WAYLAND").is_some_and(|v| !v.is_empty() && v != "0");
    // Only a local display (`:0`, `:0.0`), the session's XWayland; remote displays aren't tried.
    // x11rb's TCP addressing adds the number to port 6000 in a u16, which overflows (a panic)
    // past 59535; the probe doesn't use TCP, but the bound keeps the number safe either way.
    let local = var("DISPLAY").filter(|d| {
        let n = d.strip_prefix(':').and_then(|s| s.split('.').next()).unwrap_or_default();
        n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u16>().is_ok_and(|n| n <= 59535)
    });
    wayland && !opt_out && local.is_some_and(|d| usable(&d))
}

/// How long `x11_usable` waits for the X server to accept the connection and answer its setup
/// request. GNOME can start XWayland on demand at the first connection, which takes about a
/// second; past this a server that accepted but never answers counts as unusable and the window
/// stays on Wayland.
#[cfg(all(unix, not(target_os = "macos")))]
const X11_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether winit's X11 backend will start on `display`: the libraries it loads are there
/// (`x11_libs_present`) and the server accepts an authorized connection (`x11_connects`). A socket
/// that exists proves neither.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_usable(display: &str) -> bool {
    if !x11_libs_present() {
        log::info!("X11 libraries missing; staying on Wayland");
        return false;
    }
    x11_connects(display, X11_PROBE_TIMEOUT)
}

/// Whether the libraries winit's X11 backend loads are there: without them it fails, or panics for
/// libxkbcommon-x11.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_libs_present() -> bool {
    use x11_dl::{xcursor::Xcursor, xinput2::XInput2, xlib::Xlib, xlib_xcb::Xlib_xcb};
    Xlib::open().is_ok()
        && Xcursor::open().is_ok()
        && Xlib_xcb::open().is_ok()
        && XInput2::open().is_ok()
        && xkbcommon_dl::x11::xkbcommon_x11_option().is_some()
}

/// Whether the X server of `display` accepts a connection over its Unix socket and completes the
/// setup handshake, with the session's Xauthority cookie, within `timeout` (`x11_handshake`).
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_connects(display: &str, timeout: std::time::Duration) -> bool {
    x11_connects_with(display, timeout, x11_handshake)
}

/// `x11_connects` with the handshake given. Waits no longer than `timeout` whatever the server
/// does: the handshake runs on its own thread, since a blocking Unix `connect` has no timeout of
/// its own (Linux blocks it while the server's listen backlog is full), and the handshake gives up
/// at the same deadline, so on a silent server the thread closes its socket and ends. Only a
/// `connect` still blocked in the kernel outlives the wait; that thread sleeps until the server
/// accepts or goes away, then finds the deadline passed and closes the socket without sending
/// anything. The probe runs once per process, so at most one such thread exists, and it ends with
/// the process.
#[cfg(all(unix, not(target_os = "macos")))]
#[allow(clippy::disallowed_methods)] // the desktop app (native only): wasm never runs it
fn x11_connects_with(
    display: &str,
    timeout: std::time::Duration,
    handshake: fn(&str, std::time::Instant) -> Result<(), x11rb::errors::ConnectError>,
) -> bool {
    let Some(deadline) = std::time::Instant::now().checked_add(timeout) else {
        return false;
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let probe_display = display.to_owned();
    let spawned = std::thread::Builder::new().name("x11-probe".into()).spawn(move || {
        // The receiver is gone once the caller stopped waiting; nothing left to tell.
        let _ = sender.send(handshake(&probe_display, deadline));
    });
    if let Err(e) = spawned {
        log::info!("X11 probe thread failed to start ({e}); staying on Wayland");
        return false;
    }
    match receiver.recv_timeout(timeout) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            log::info!("X11 display {display} unusable ({e}); staying on Wayland");
            false
        }
        Err(_) => {
            log::info!("X11 display {display} did not answer within {timeout:?}; staying on Wayland");
            false
        }
    }
}

/// Connects to `display`'s Unix socket (the abstract one first on Linux, then the file, as Xlib
/// does) and completes the X11 setup handshake by `deadline`, sending the session's Xauthority
/// cookie as x11rb would. Never TCP: x11rb's `connect` falls back to localhost:6000+N, which could
/// pass for a server winit can't reach and would hand the cookie to whatever listens on that port.
/// The connection is closed on return.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_handshake(display: &str, deadline: std::time::Instant) -> Result<(), x11rb::errors::ConnectError> {
    use std::os::fd::AsFd as _;
    use x11rb::reexports::x11rb_protocol::{
        parse_display::{ConnectAddress, parse_display},
        xauth::get_auth,
    };
    use x11rb::rust_connection::{DefaultStream, RustConnection};

    let parsed = parse_display(Some(display))?;
    let mut error = None;
    for address in parsed.connect_instruction().filter(|a| matches!(a, ConnectAddress::Socket(_))) {
        match DefaultStream::connect(&address) {
            Ok((stream, (family, peer))) => {
                // x11rb's stream waits for the server without a timeout; hand its socket to one
                // that stops at the deadline (a duplicate descriptor, the original closed here).
                let socket = std::os::unix::net::UnixStream::from(stream.as_fd().try_clone_to_owned()?);
                drop(stream);
                socket.set_nonblocking(false)?;
                // As x11rb: an unreadable Xauthority means connecting without a cookie.
                let (auth_name, auth_data) = get_auth(family, &peer, parsed.display).unwrap_or(None).unwrap_or_default();
                let stream = DeadlineStream { socket, deadline };
                RustConnection::connect_to_stream_with_auth_info(stream, parsed.screen.into(), auth_name, auth_data)?;
                return Ok(());
            }
            Err(e) => error = Some(e),
        }
    }
    Err(error.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no Unix socket for this display")).into())
}

/// An x11rb stream over a blocking Unix socket whose reads and writes give up at `deadline` with
/// `TimedOut`. Only used for the setup handshake (`connect_to_stream_with_auth_info`), where x11rb
/// polls and then reads or writes, and retries only on `WouldBlock`; the reads and writes here
/// wait themselves, bounded by the deadline, so `poll` doesn't. That bends `Stream`'s
/// "never blocks" rule, which matters for a connection shared between threads; this one is
/// dropped as soon as the handshake ends, and every call, however x11rb sequences them, returns
/// by the deadline. Past it, `poll` fails before anything (the cookie included) is written.
#[cfg(all(unix, not(target_os = "macos")))]
struct DeadlineStream {
    socket: std::os::unix::net::UnixStream,
    deadline: std::time::Instant,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl DeadlineStream {
    /// The time left before the deadline, or `TimedOut` once it has passed.
    #[allow(clippy::disallowed_methods)] // the desktop app (native only): wasm never runs it
    fn remaining(&self) -> std::io::Result<std::time::Duration> {
        self.deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "the X server did not answer in time"))
    }

    /// Runs one read or write with the socket's timeout set to the time left, retrying when
    /// interrupted. A socket timeout (`WouldBlock` on Unix) becomes `TimedOut`, which x11rb
    /// doesn't retry.
    fn bounded(
        &self,
        set_timeout: impl Fn(Option<std::time::Duration>) -> std::io::Result<()>,
        mut io: impl FnMut() -> std::io::Result<usize>,
    ) -> std::io::Result<usize> {
        use std::io::ErrorKind;
        loop {
            set_timeout(Some(self.remaining()?))?;
            match io() {
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Err(std::io::Error::new(ErrorKind::TimedOut, "the X server did not answer in time"));
                }
                result => return result,
            }
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl x11rb::rust_connection::Stream for DeadlineStream {
    fn poll(&self, _mode: x11rb::rust_connection::PollMode) -> std::io::Result<()> {
        self.remaining().map(drop)
    }

    fn read(&self, buf: &mut [u8], _fds: &mut Vec<x11rb::utils::RawFdContainer>) -> std::io::Result<usize> {
        use std::io::Read as _;
        self.bounded(|t| self.socket.set_read_timeout(t), || (&self.socket).read(buf))
    }

    fn write(&self, buf: &[u8], fds: &mut Vec<x11rb::utils::RawFdContainer>) -> std::io::Result<usize> {
        use std::io::Write as _;
        if !fds.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "the X11 probe sends no file descriptors"));
        }
        self.bounded(|t| self.socket.set_write_timeout(t), || (&self.socket).write(buf))
    }
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
    let window_builder = viewport.clone();
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(all(unix, not(target_os = "macos")))]
    let options = {
        let mut options = options;
        if prefer_x11(|k| std::env::var(k).ok(), x11_usable) {
            log::info!("Wayland: using X11 (XWayland) for file drag-and-drop and the desktop's title bar (GRIDCRAFT_WAYLAND=1 keeps Wayland)");
            options.event_loop_builder = Some(Box::new(|builder| {
                use winit::platform::x11::EventLoopBuilderExtX11 as _;
                builder.with_x11();
            }));
        }
        options
    };
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

#[cfg(all(test, unix, not(target_os = "macos")))]
mod backend_tests {
    use std::cell::Cell;
    #[cfg(target_os = "linux")]
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    #[cfg(target_os = "linux")]
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use super::{DeadlineStream, prefer_x11, x11_connects, x11_connects_with};

    /// `prefer_x11` with these environment variables and a probe answering `usable`; also returns
    /// the display the probe was asked about, if it was asked.
    fn prefer(vars: &[(&str, &str)], usable: bool) -> (bool, Option<String>) {
        let asked = Cell::new(None);
        let x11 = prefer_x11(
            |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()),
            |d| {
                asked.set(Some(d.to_string()));
                usable
            },
        );
        (x11, asked.take())
    }

    const GNOME_WAYLAND: &[(&str, &str)] = &[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"), ("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")];

    /// Wayland sessions go through XWayland when X11 will start there: file drag-and-drop on
    /// every desktop, and GNOME's own title bar on GNOME.
    #[test]
    fn wayland_uses_a_usable_x11() {
        assert_eq!(prefer(GNOME_WAYLAND, true), (true, Some(":0".into())));
        let kde = [("XDG_CURRENT_DESKTOP", "KDE"), ("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")];
        assert_eq!(prefer(&kde, true), (true, Some(":0".into())));
        let screen = [("XDG_CURRENT_DESKTOP", "GNOME"), ("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":1.0")];
        assert_eq!(prefer(&screen, true), (true, Some(":1.0".into())));
        let zero = [GNOME_WAYLAND, &[("GRIDCRAFT_WAYLAND", "0")]].concat();
        assert!(prefer(&zero, true).0);
    }

    /// A missing socket, a refused authorization or missing libraries all make the probe say
    /// no; the window then stays on Wayland, which still starts (an X11 failure couldn't fall back).
    #[test]
    fn unusable_x11_stays_on_wayland() {
        assert_eq!(prefer(GNOME_WAYLAND, false), (false, Some(":0".into())));
    }

    /// Everything else keeps winit's default backend, without even probing the X server.
    #[test]
    fn other_sessions_keep_the_default_backend() {
        let x11_session = [("XDG_CURRENT_DESKTOP", "GNOME"), ("DISPLAY", ":0")];
        assert_eq!(prefer(&x11_session, true), (false, None), "an X11 session already uses X11");
        let opt_out = [GNOME_WAYLAND, &[("GRIDCRAFT_WAYLAND", "1")]].concat();
        assert_eq!(prefer(&opt_out, true), (false, None));
        let no_display = [("XDG_CURRENT_DESKTOP", "GNOME"), ("WAYLAND_DISPLAY", "wayland-0")];
        assert_eq!(prefer(&no_display, true), (false, None));
        for remote in ["host:0", ":", ":x", ":+1", "unix:/tmp/sock", ":59536", ":65000", ":99999999999"] {
            let vars = [("XDG_CURRENT_DESKTOP", "GNOME"), ("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", remote)];
            assert_eq!(prefer(&vars, true), (false, None), "{remote}");
        }
    }

    /// The connection probe on its own, whatever X11 libraries this machine has: a display with no
    /// server behind it is unusable, and the probe returns rather than hanging or panicking. The
    /// highest display number `prefer_x11` passes on doesn't overflow x11rb's TCP port.
    #[test]
    fn probe_rejects_a_display_without_a_server() {
        assert!(!x11_connects(":59535", TEST_TIMEOUT));
    }

    /// The probe never connects over TCP: a listener on localhost:6000+N, with no Unix socket for
    /// `:N`, gets no connection (and so never sees the Xauthority cookie).
    #[test]
    fn probe_never_uses_tcp() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let Some((n, listener)) = (500..600u16).find_map(|n| {
            let free = !std::path::Path::new(&format!("/tmp/.X11-unix/X{n}")).exists();
            free.then(|| std::net::TcpListener::bind(("127.0.0.1", 6000 + n)).ok().map(|l| (n, l))).flatten()
        }) else {
            return; // No free port to listen on; nothing to check.
        };
        let connected = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&connected);
        // Closes each connection at once, so a regression fails fast instead of hanging. Left
        // blocked in accept when nothing connects; the test process ends it.
        std::thread::spawn(move || {
            for _ in listener.incoming() {
                seen.store(true, Ordering::SeqCst);
            }
        });
        assert!(!x11_connects(&format!(":{n}"), TEST_TIMEOUT));
        assert!(!connected.load(Ordering::SeqCst), "the probe connected to TCP port {}", 6000 + n);
    }

    /// The caller's wait is bounded even when the handshake itself never returns (a Unix `connect`
    /// blocked in the kernel, say): the probe gives up at its timeout.
    #[test]
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn probe_waits_no_longer_than_its_timeout() {
        let start = Instant::now();
        assert!(!x11_connects_with(":0", TEST_TIMEOUT, |_, _| {
            std::thread::sleep(Duration::from_secs(30));
            Ok(())
        }));
        let waited = start.elapsed();
        assert!(waited >= TEST_TIMEOUT && waited < TEST_TIMEOUT + Duration::from_secs(2), "waited {waited:?}");
    }

    /// The handshake stream gives up on a server that never answers, at its deadline.
    #[test]
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn handshake_times_out_on_silence() {
        let Ok((client, _server)) = UnixStream::pair() else {
            return;
        };
        let Ok(()) = client.set_nonblocking(false) else {
            return;
        };
        let start = Instant::now();
        let Some(deadline) = start.checked_add(TEST_TIMEOUT) else {
            return;
        };
        let stream = DeadlineStream { socket: client, deadline };
        let result = x11rb::rust_connection::RustConnection::connect_to_stream(stream, 0);
        let waited = start.elapsed();
        assert!(result.is_err());
        assert!(waited >= TEST_TIMEOUT && waited < TEST_TIMEOUT + Duration::from_secs(2), "waited {waited:?}");
    }

    /// A handshake that starts after the deadline (its `connect` was stuck until then) fails
    /// without writing anything, so the server never gets the Xauthority cookie.
    #[test]
    #[allow(clippy::disallowed_methods)] // a native-only test: the clock is read off wasm
    fn late_handshake_sends_nothing() {
        use std::io::Read as _;
        let Ok((client, mut server)) = UnixStream::pair() else {
            return;
        };
        let stream = DeadlineStream { socket: client, deadline: Instant::now() };
        let cookie = (b"MIT-MAGIC-COOKIE-1".to_vec(), vec![7u8; 16]);
        let result = x11rb::rust_connection::RustConnection::connect_to_stream_with_auth_info(stream, 0, cookie.0, cookie.1);
        assert!(result.is_err());
        drop(result);
        let mut sent = Vec::new();
        assert!(server.read_to_end(&mut sent).is_ok());
        assert!(sent.is_empty(), "sent {} bytes after the deadline", sent.len());
    }

    /// The probe's timeout in these tests; long enough for a local socket, short enough to wait
    /// out.
    const TEST_TIMEOUT: Duration = Duration::from_millis(500);

    /// A fake X server for display `:N` on the abstract Unix socket `/tmp/.X11-unix/XN`, which the
    /// probe tries first, as Xlib does; needs no file in `/tmp/.X11-unix`. `serve` handles the one
    /// connection it accepts. Returns the display and the server thread, or `None` when no display
    /// number is free.
    #[cfg(target_os = "linux")]
    fn fake_server<T: Send + 'static>(serve: impl FnOnce(UnixStream) -> T + Send + 'static) -> Option<(String, JoinHandle<Option<T>>)> {
        use std::os::linux::net::SocketAddrExt as _;
        use std::os::unix::net::{SocketAddr, UnixListener};
        let (n, listener) = (40000..41000u16).find_map(|n| {
            let path = format!("/tmp/.X11-unix/X{n}");
            if std::path::Path::new(&path).exists() {
                return None;
            }
            let address = SocketAddr::from_abstract_name(path.as_bytes()).ok()?;
            UnixListener::bind_addr(&address).ok().map(|l| (n, l))
        })?;
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().ok()?;
            // A regression that never closes the connection fails the test instead of hanging it.
            stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
            Some(serve(stream))
        });
        Some((format!(":{n}"), server))
    }

    /// Reads the client's setup request (12 bytes, then the padded authorization name and data);
    /// whether it was one.
    #[cfg(target_os = "linux")]
    fn read_setup_request(mut stream: &UnixStream) -> bool {
        let mut head = [0u8; 12];
        if stream.read_exact(&mut head).is_err() {
            return false;
        }
        let u16_at = |i: usize| match head[0] {
            b'l' => u16::from_le_bytes([head[i], head[i + 1]]),
            _ => u16::from_be_bytes([head[i], head[i + 1]]),
        };
        let padded = |n: u16| usize::from(n).div_ceil(4) * 4;
        let mut auth = vec![0u8; padded(u16_at(6)) + padded(u16_at(8))];
        matches!(head[0], b'l' | b'B') && u16_at(2) == 11 && stream.read_exact(&mut auth).is_ok()
    }

    /// Answers the setup request with `reply`, its length field filled in (the client sends its
    /// own byte order, and serializing uses this machine's, the same).
    #[cfg(target_os = "linux")]
    fn answer(mut stream: &UnixStream, reply: &[u8]) -> bool {
        let mut reply = reply.to_vec();
        let words = u16::try_from((reply.len() - 8) / 4).unwrap_or_default();
        reply[6..8].copy_from_slice(&words.to_ne_bytes());
        read_setup_request(stream) && stream.write_all(&reply).is_ok()
    }

    /// Reads until the client closes the connection; whether it did.
    #[cfg(target_os = "linux")]
    fn closed_by_client(mut stream: &UnixStream) -> bool {
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).is_ok()
    }

    /// A server that completes the setup handshake is usable.
    #[cfg(target_os = "linux")]
    #[test]
    fn probe_accepts_a_server_that_completes_setup() {
        use x11rb::protocol::xproto::{Screen, Setup};
        use x11rb::x11_utils::Serialize as _;
        let setup = Setup {
            status: 1,
            protocol_major_version: 11,
            resource_id_base: 0x0400_0000,
            resource_id_mask: 0x001f_ffff,
            maximum_request_length: 0xffff,
            roots: vec![Screen::default()],
            ..Setup::default()
        };
        let Some((display, server)) = fake_server(move |s| answer(&s, &setup.serialize()) && closed_by_client(&s)) else {
            return;
        };
        assert!(x11_connects(&display, TEST_TIMEOUT));
        assert_eq!(server.join().ok().flatten(), Some(true), "handshake answered, then the probe closed its connection");
    }

    /// A server that refuses the connection (wrong cookie, say) is unusable.
    #[cfg(target_os = "linux")]
    #[test]
    fn probe_rejects_a_refused_setup() {
        use x11rb::protocol::xproto::SetupFailed;
        use x11rb::x11_utils::Serialize as _;
        let failed = SetupFailed { status: 0, protocol_major_version: 11, reason: b"access denied by key".to_vec(), ..SetupFailed::default() };
        let Some((display, server)) = fake_server(move |s| answer(&s, &failed.serialize())) else {
            return;
        };
        assert!(!x11_connects(&display, TEST_TIMEOUT));
        assert_eq!(server.join().ok().flatten(), Some(true));
    }

    /// A server that hangs up during the handshake is unusable.
    #[cfg(target_os = "linux")]
    #[test]
    fn probe_rejects_a_dropped_connection() {
        let Some((display, server)) = fake_server(|s| read_setup_request(&s)) else {
            return;
        };
        assert!(!x11_connects(&display, TEST_TIMEOUT));
        assert_eq!(server.join().ok().flatten(), Some(true));
    }

    /// A server that accepts the connection but never answers the setup request: the probe gives up
    /// at its timeout instead of blocking start-up, and its thread then closes the connection.
    #[cfg(target_os = "linux")]
    #[test]
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn probe_times_out_on_a_silent_server() {
        let Some((display, server)) = fake_server(|s| read_setup_request(&s) && closed_by_client(&s)) else {
            return;
        };
        let start = Instant::now();
        assert!(!x11_connects(&display, TEST_TIMEOUT));
        let waited = start.elapsed();
        assert!(waited >= TEST_TIMEOUT && waited < TEST_TIMEOUT + Duration::from_secs(2), "waited {waited:?}");
        assert_eq!(server.join().ok().flatten(), Some(true), "the probe closed its connection after the timeout");
    }
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
