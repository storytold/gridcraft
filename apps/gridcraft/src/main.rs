//! GridCraft desktop app.
//!
//! Usage: `gridcraft [--control <port>] [--sample <name>] [files…]`
//!
//! `--control <port>` (or `GRIDCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
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

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp, WorkbookWindowAction, WorkbookWindowInfo};
use serde_json::json;

struct App {
    windows: Vec<SheetApp>,
    focused: Option<u64>,
    focus_request: Option<u64>,
    #[cfg(target_os = "macos")]
    root_hidden: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    #[cfg(target_os = "macos")]
    apple_events: Option<fmv_macos_events::Inbox>,
}

impl App {
    fn new(app: SheetApp) -> Self {
        let focused = app.session.active().map(|d| d.uid);
        let mut this = Self {
            windows: vec![app],
            focused,
            focus_request: None,
            #[cfg(target_os = "macos")]
            root_hidden: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            #[cfg(target_os = "macos")]
            apple_events: None,
        };
        this.split_workbooks();
        this
    }

    fn viewport_id(id: u64) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("workbook", id))
    }

    fn viewport_for_window(&self, id: u64) -> egui::ViewportId {
        if self.windows.first().is_some_and(|app| Self::window_id(app) == Some(id)) { egui::ViewportId::ROOT } else { Self::viewport_id(id) }
    }

    fn window_id(app: &SheetApp) -> Option<u64> {
        app.session.active().map(|d| d.uid)
    }

    fn window_builder(app: &SheetApp, active: bool) -> egui::ViewportBuilder {
        let title = app.session.active().map(|d| d.display_title()).unwrap_or_else(|| "GridCraft".into());
        let mut viewport = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([640.0, 420.0])
            .with_app_id("ai.storyteller.gridcraft")
            .with_active(active)
            .with_drag_and_drop(true);
        if cfg!(target_os = "macos") {
            viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
        }
        if let Some(icon) = icon() {
            viewport = viewport.with_icon(icon);
        }
        viewport
    }

    fn summaries(&self, active: u64) -> Vec<WorkbookWindowInfo> {
        self.windows
            .iter()
            .filter_map(|app| {
                let document = app.session.active()?;
                Some(WorkbookWindowInfo {
                    id: document.uid,
                    title: document.display_title(),
                    dirty: document.is_dirty(),
                    active: document.uid == active,
                })
            })
            .collect()
    }

    fn split_workbooks(&mut self) {
        let mut extracted = Vec::new();
        for app in &mut self.windows {
            while app.session.documents().len() > 1 {
                let Some(document) = app.session.take_document(1) else { break };
                extracted.push((app.ui.clone(), app.session.prefs.clone(), document));
            }
        }
        for (ui_state, prefs, mut document) in extracted {
            if let Some(path) = document.path.as_deref()
                && let Some(existing) = self.windows.iter().find_map(|app| {
                    let open = app.session.active()?;
                    (open.path.as_deref() == Some(path)).then_some(open.uid)
                })
            {
                self.focus_request = Some(existing);
                continue;
            }
            if document.path.is_none()
                && document.title.strip_prefix("Book").is_some_and(|suffix| suffix.parse::<usize>().is_ok())
                && self.windows.iter().any(|app| app.session.active().is_some_and(|open| open.path.is_none() && open.title == document.title))
            {
                let number = (1..)
                    .find(|number| {
                        let title = format!("Book{number}");
                        !self.windows.iter().any(|app| app.session.active().is_some_and(|open| open.path.is_none() && open.title == title))
                    })
                    .unwrap_or(1);
                document.title = format!("Book{number}");
            }
            let id = document.uid;
            let mut session = Session::new();
            session.prefs = prefs;
            session.add_document(document);
            let mut app = SheetApp::new(session, services());
            app.ui = ui_state;
            self.windows.push(app);
            self.focus_request = Some(id);
        }
    }

    fn handle_action(&mut self, action: WorkbookWindowAction) {
        match action {
            WorkbookWindowAction::Focus(id) => self.focus_request = Some(id),
            WorkbookWindowAction::Close(id) => {
                self.focus_request = Some(id);
                if let Some(app) = self.windows.iter_mut().find(|app| Self::window_id(app) == Some(id)) {
                    app.close_document(app.session.active_index(), false);
                }
            }
        }
    }

    fn remove_closed(&mut self, ctx: &egui::Context) {
        let control_rx = self.windows.iter_mut().find(|app| app.session.documents().is_empty()).and_then(|app| app.control_rx.take());
        if self.windows.first().is_some_and(|app| app.session.documents().is_empty()) {
            if self.windows.len() == 1 {
                #[cfg(not(target_os = "macos"))]
                self.windows.clear();
            } else {
                let promoted_id = Self::window_id(&self.windows[1]);
                let geometry = promoted_id.map(|id| {
                    ctx.input_for(Self::viewport_id(id), |input| {
                        (
                            input.viewport().outer_rect.map(|rect| rect.min),
                            input.viewport().inner_rect.map(|rect| rect.size()),
                            input.viewport().maximized,
                            input.viewport().fullscreen,
                        )
                    })
                });
                self.windows.remove(0);
                if let Some((position, size, maximized, fullscreen)) = geometry {
                    if let Some(position) = position {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                    }
                    if let Some(size) = size {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                    }
                    if let Some(maximized) = maximized {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(maximized));
                    }
                    if let Some(fullscreen) = fullscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(fullscreen));
                    }
                }
                self.focus_request = promoted_id;
            }
        }
        let mut index = 1;
        while index < self.windows.len() {
            if self.windows[index].session.documents().is_empty() {
                self.windows.remove(index);
            } else {
                index += 1;
            }
        }
        if self.focused.is_some_and(|id| !self.windows.iter().any(|app| Self::window_id(app) == Some(id))) {
            self.focused = self.windows.first().and_then(Self::window_id);
        }
        if let Some(receiver) = control_rx {
            let target = self.focused.and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
            if let Some(app) = self.windows.get_mut(target) {
                app.control_rx = Some(receiver);
            }
        }
    }

    fn move_control_channel_to_focused(&mut self) {
        let Some(focused) = self.focused else { return };
        let Some(target) = self.windows.iter().position(|app| Self::window_id(app) == Some(focused)) else { return };
        let Some(source) = self.windows.iter().position(|app| app.control_rx.is_some()) else { return };
        if source != target
            && let Some(receiver) = self.windows[source].control_rx.take()
        {
            self.windows[target].control_rx = Some(receiver);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.native_menu.is_none() && std::env::var_os("GRIDCRAFT_NO_NATIVE_MENU").is_none() {
                self.native_menu = Some(native_menu::NativeMenu::install(ctx));
            }
            if let Some(menu) = &self.native_menu {
                let target = self.focused.and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
                if let Some(app) = self.windows.get_mut(target) {
                    menu.poll(app, ctx);
                }
            }
            if let Some(inbox) = &self.apple_events
                && let Some(app) = self.focused.and_then(|id| self.windows.iter_mut().find(|app| Self::window_id(app) == Some(id)))
            {
                apple_events::poll(inbox, app, ctx);
            }
        }
        if let Some(app) = self.windows.first_mut() {
            app.logic_for_workbook_window(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(app) = self.windows.first_mut() {
                app.close_document(app.session.active_index(), false);
            }
        }
        self.move_control_channel_to_focused();
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let viewport_id = ctx.viewport_id();
        let root_id = self.windows.first().and_then(Self::window_id);
        let Some(app) = self.windows.iter_mut().find(|app| {
            Self::window_id(app).is_some_and(|id| {
                let expected = if root_id == Some(id) { egui::ViewportId::ROOT } else { Self::viewport_id(id) };
                expected == viewport_id
            })
        }) else {
            return;
        };
        app.raw_input_hook(raw);
        // Files dropped on the window open as workbooks.
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().map(str::to_string).filter(|p| !p.is_empty()) {
                app.open_path(&p);
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.split_workbooks();
        let activate = self.focus_request.take();
        let ctx = ui.ctx().clone();
        let mut actions = Vec::new();
        let mut focused = self.focused;
        if let Some(root_id) = self.windows.first().and_then(Self::window_id) {
            let summaries = self.summaries(root_id);
            if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
                focused = Some(root_id);
            }
            if let Some(action) = self.windows[0].ui_with_workbook_windows(ui, Some(&summaries)) {
                actions.push(action);
            }
        }
        for index in 1..self.windows.len() {
            let Some(id) = Self::window_id(&self.windows[index]) else { continue };
            let summaries = self.summaries(id);
            let builder = Self::window_builder(&self.windows[index], activate == Some(id));
            let app = &mut self.windows[index];
            ctx.show_viewport_immediate(Self::viewport_id(id), builder, |ui, _class| {
                app.logic_for_workbook_window(ui.ctx());
                if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
                    focused = Some(id);
                }
                if ui.input(|i| i.viewport().close_requested()) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    actions.push(WorkbookWindowAction::Close(id));
                }
                if let Some(action) = app.ui_with_workbook_windows(ui, Some(&summaries)) {
                    actions.push(action);
                }
            });
        }
        self.focused = focused;
        for action in actions {
            self.handle_action(action);
        }
        self.split_workbooks();
        self.remove_closed(&ctx);
        #[cfg(target_os = "macos")]
        if let Some(root_id) = self.windows.first().and_then(Self::window_id)
            && self.root_hidden
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.root_hidden = false;
            self.focus_request = Some(root_id);
        }
        if let Some(id) = self.focus_request.or(activate)
            && self.windows.iter().any(|app| Self::window_id(app) == Some(id))
        {
            ctx.send_viewport_cmd_to(self.viewport_for_window(id), egui::ViewportCommand::Focus);
            self.focused = Some(id);
            if self.focus_request.is_some() {
                ctx.request_repaint();
            }
        }
        if self.windows.first().is_none_or(|app| app.session.documents().is_empty()) {
            self.focused = None;
            #[cfg(target_os = "macos")]
            if !self.root_hidden {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.root_hidden = true;
            }
            #[cfg(not(target_os = "macos"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn on_exit(&mut self) {
        if let Some(app) = self.windows.first() {
            save_prefs(app);
        }
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
    let Some(dir) = config_dir() else { return };
    if let Ok(b) = std::fs::read(dir.join("ui.json"))
        && let Ok(ui) = serde_json::from_slice::<gridcraft_ui_egui::UiState>(&b)
    {
        let saved = serde_json::from_slice(&b).ok().and_then(|v| gridcraft_ui_egui::i18n::saved_language(&v));
        let language = saved.unwrap_or(app.ui.language);
        app.ui = ui;
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
    static ICON: std::sync::OnceLock<Option<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let bytes = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.gridcraft.png");
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
    })
    .clone()
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
    load_prefs(&mut app);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GridCraft")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([640.0, 420.0])
        .with_app_id("ai.storyteller.gridcraft")
        .with_drag_and_drop(true);
    if cfg!(target_os = "macos") {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
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
            SheetApp::setup_context_for_language(&cc.egui_ctx, app.ui.dark, app.ui.language);
            if let Some(port) = control_port {
                match control_server::start(port, cc.egui_ctx.clone()) {
                    Ok(rx) => app.control_rx = Some(rx),
                    Err(e) => log::error!("control channel on port {port}: {e}"),
                }
            }
            let mut shell = App::new(app);
            #[cfg(target_os = "macos")]
            {
                shell.apple_events = Some(apple_events.connect(&cc.egui_ctx));
            }
            Ok(Box::new(shell))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_open_document_gets_its_own_window() {
        let mut session = Session::new();
        session.new_workbook();
        session.new_workbook();
        let ids: Vec<u64> = session.documents().iter().map(|document| document.uid).collect();

        let app = App::new(SheetApp::new(session, Services::default()));

        assert_eq!(app.windows.len(), 2);
        assert!(app.windows.iter().all(|window| window.session.documents().len() == 1));
        assert_eq!(app.windows.iter().filter_map(App::window_id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn blank_workbook_titles_are_unique_across_windows() {
        let mut session = Session::new();
        session.new_workbook();
        if let Some(document) = session.active_mut() {
            document.title = "Report".into();
        }
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.windows[0].session.new_workbook();
        app.split_workbooks();

        let titles: Vec<String> = app.windows.iter().filter_map(|window| window.session.active().map(|document| document.title.clone())).collect();
        assert_eq!(titles, ["Report", "Book1", "Book2"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn closing_the_last_workbook_keeps_an_empty_app_coordinator() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.windows[0].close_document(0, false);
        app.remove_closed(&egui::Context::default());

        assert_eq!(app.windows.len(), 1);
        assert!(app.windows[0].session.documents().is_empty());
        assert_eq!(app.focused, None);
    }
}
