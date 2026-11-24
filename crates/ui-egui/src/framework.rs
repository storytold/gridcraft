//! The craft-ui runtime, hosted from GridCraft's own eframe `App` (craft-libs docs/craft-framework.md, section 2.2).
//! The desktop app calls [`install`] once, [`SheetApp::before_input`] in `raw_input_hook`, [`SheetApp::close_path`]
//! last in `logic`, and [`before_ui`] first in `ui`; the browser build and tests install with [`install_headless`].
//! The framework dialogs run on [`SheetApp::framework_dialogs`] after GridCraft's own UI.
//!
//! The runtime takes Escape, Tab and the clipboard events for the whole window unless one of GridCraft's own egui
//! widgets has the keyboard. The grid takes egui's focus whenever nothing else has it, so its keys and the clipboard
//! events stay in egui's input; the window's Escape, while none of GridCraft's widgets has the keyboard, is read with
//! [`escape_pressed`] and taken with [`consume_escape`].

use std::sync::{Arc, OnceLock};

use craft_host::target::ShellInfo;
use craft_host::{AppInfo, Host};
use craft_keymap::Overrides;
use craft_ui::fonts::{I18nScripts, SystemFonts, UiFonts};
use craft_ui::runtime::commands::{CommandInput, CommandSet as _, QuitCommand};
use craft_ui::runtime::env::{RootEnv, Scale};
use craft_ui::runtime::frame::{FrameInput, FrameOutput};
use craft_ui::runtime::keys::{Intent, with_window_intents};
use craft_ui::shell::embed;
use craft_window::system::WindowSystem;

use crate::SheetApp;

/// Each pass of the root window, first in `App::ui`: hands the runtime the native window it answers the hit test of
/// (Windows) and fits the window to its monitor once.
#[cfg(not(target_arch = "wasm32"))]
pub use craft_ui::shell::embed::before_ui;

/// The id every OS window of the app carries, as the main window's builder sets it.
const APP_ID: &str = "ai.storyteller.gridcraft";

/// The scripts of the interface languages, for the font stack.
static SCRIPTS: I18nScripts = I18nScripts;

/// The runtime's first font stack: egui's bundled faces and no installed system fonts. GridCraft's own definitions
/// ([`SheetApp::setup_context_for_language`]) replace it before anything is drawn.
fn ui_fonts() -> UiFonts<'static> {
    UiFonts::new(&SCRIPTS).system_fonts(SystemFonts::Off)
}

fn install_with(ctx: &egui::Context, shell: ShellInfo, windows: Arc<dyn WindowSystem>) -> Result<(), String> {
    let spec = craft_ui::InstallSpec::new(shell, windows).app_id(APP_ID);
    embed::install(ctx, spec, &ui_fonts()).map(drop).map_err(|error| format!("the UI runtime could not be installed: {error}"))
}

/// Installs the runtime on the desktop window's context, once, before the first pass. GridCraft draws its own title
/// bar (and, where the desktop leaves the frame to the app, its own window buttons), so no frame decision is handed
/// over and the runtime answers no hit test of the window.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(ctx: &egui::Context) -> Result<(), String> {
    let shell = ShellInfo::default().backend(craft_host::target::Backend::from_env());
    install_with(ctx, shell, Arc::new(craft_window::system::SystemWindows))
}

/// Installs the runtime on a context with no native window of its own: the browser build's canvas and test windows.
pub fn install_headless(ctx: &egui::Context) {
    if let Err(error) = install_with(ctx, ShellInfo::default(), Arc::new(craft_window::system::NoWindows)) {
        log::warn!("{error}");
    }
}

/// Who GridCraft is to the framework's dialogs (the update dialogs show its name and icon).
pub(crate) fn host() -> Arc<Host> {
    Arc::new(
        Host::builder(AppInfo { id: "gridcraft", name: "GridCraft", version: env!("CARGO_PKG_VERSION") })
            .icon_png(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.gridcraft.png"))
            .build(),
    )
}

/// GridCraft quits through its own Quit (the macOS app menu, the last window closing, an update's restart), which
/// asks about every unsaved workbook in its window, and Ctrl+Q is no quit key on Windows and Linux (Excel's Quick
/// Analysis), so the framework's `window.quit` is unbound. Overrides are keyed by command id, which the command set
/// gives.
fn command_overrides() -> Arc<Overrides> {
    static OVERRIDES: OnceLock<Arc<Overrides>> = OnceLock::new();
    OVERRIDES
        .get_or_init(|| {
            let mut overrides = Overrides::new();
            overrides.insert(QuitCommand::Quit.id(), Vec::new());
            Arc::new(overrides)
        })
        .clone()
}

impl SheetApp {
    /// The framework's part of `App::raw_input_hook`, once per frame before the first pass: takes what the runtime
    /// gathered since the last frame and gives the coming passes the root values (theme mode, interface scale).
    pub fn before_input(&mut self, ctx: &egui::Context) {
        embed::before_input(ctx, |output| self.frame_input(output));
    }

    /// The root values of this frame. GridCraft registers no commands with the runtime (it unbinds the framework's
    /// quit, [`command_overrides`]) and lets it toggle no chrome, so the output carries nothing it acts on. The
    /// interface scale is the Interface Size preference: the runtime is the one writer of egui's zoom factor.
    fn frame_input(&mut self, _output: FrameOutput) -> FrameInput {
        let mode = if self.effective_dark { craft_theme::Mode::Dark } else { craft_theme::Mode::Light };
        let root = RootEnv::new().mode(mode).scale(Scale::new(self.ui.zoom_factor()));
        FrameInput::new(root).commands(CommandInput::new().overrides(command_overrides())).host(Arc::clone(&self.framework_host))
    }

    /// The framework's part of `App::logic`, last, after GridCraft's own logic and the native windows': the root
    /// window's close path, decided by [`SheetApp::close_decision`].
    pub fn close_path(&mut self, ctx: &egui::Context) {
        embed::logic(ctx, || self.close_decision(ctx));
    }

    /// The framework dialogs open on [`SheetApp::framework_dialogs`], once per frame after GridCraft's own UI.
    pub(crate) fn run_framework_dialogs(&mut self, ui: &mut egui::Ui) {
        self.framework_dialogs.run(ui);
    }
}

/// Whether Escape was pressed in this pass: from egui while one of GridCraft's own widgets or popups has the
/// keyboard, otherwise the window's cancel, which the runtime routes to no component. Leaves it for later readers.
pub(crate) fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape)) || window_cancel(ctx, false)
}

/// Takes a plain Escape pressed in this pass, from egui or as the window's cancel: no later reader sees it.
pub(crate) fn consume_escape(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) || take_window_cancel(ctx)
}

/// Takes the window's cancel (an Escape while nothing has framework focus) when it is there: no later reader sees it.
pub(crate) fn take_window_cancel(ctx: &egui::Context) -> bool {
    window_cancel(ctx, true)
}

/// Whether the window's cancel is there and untaken; `take` takes it.
fn window_cancel(ctx: &egui::Context, take: bool) -> bool {
    let mut pending = false;
    with_window_intents(ctx, false, |intents| {
        pending = if take { intents.take(Intent::Cancel) } else { intents.untaken(&Intent::Cancel) };
        take && pending
    });
    pending
}
