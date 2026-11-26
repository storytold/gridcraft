#[cfg(not(target_arch = "wasm32"))]
use super::updates_ui;
use super::{SheetApp, desktop_theme};

impl SheetApp {
    /// Runs a command and shows its error in a message box (for menu/ribbon clicks).
    /// Follow the desktop's light/dark choice while the app runs (Linux; the desktop app calls
    /// this once — tests and embedders don't start the watcher thread).
    pub fn watch_desktop_theme(&mut self) {
        self.desktop_theme = desktop_theme::DesktopTheme::start();
    }

    /// Follows the desktop/system theme preference and records the effective theme.
    pub(super) fn sync_theme_preference(&mut self, ctx: &egui::Context) {
        let preference = match (
            self.ui.system_theme,
            ctx.system_theme().or_else(|| self.desktop_dark.map(|d| if d { egui::Theme::Dark } else { egui::Theme::Light })),
        ) {
            (true, Some(egui::Theme::Dark)) => egui::ThemePreference::Dark,
            (true, Some(egui::Theme::Light)) => egui::ThemePreference::Light,
            _ => self.ui.theme_preference(),
        };
        if ctx.options(|o| o.theme_preference) != preference {
            ctx.set_theme(preference);
            ctx.request_repaint();
        }
        self.effective_dark = ctx.theme() == egui::Theme::Dark;
        crate::theme::set_dark_sheet(ctx, self.ui.dark_sheet);
        crate::theme::set_dark_charts(ctx, self.ui.dark_charts);
        crate::theme::set_dark_pictures(ctx, self.ui.dark_pictures);
    }

    /// Look for a newer release once, in the background, as the desktop app starts (a no-op on the
    /// web). A found release opens the update prompt; nothing is shown when GridCraft is current.
    /// A nightly channel has no update feed of its own: the Launcher installs and replaces it.
    pub fn start_update_check(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if crate::channel::stable_services_allowed() {
            updates_ui::start(self, false);
        }
    }

    /// Help ▸ Check for Updates…: look now, and say so when GridCraft is up to date or the check fails.
    pub fn check_for_updates(&mut self) {
        // Never from the nightly channel: a check would download into, and record in, the stable
        // app's folders, and an install would replace a Launcher-managed copy.
        if !crate::channel::stable_services_allowed() {
            self.message = Some((crate::channel::name().into(), "GridCraft Nightly is updated by ArtCraft Launcher.".into()));
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::start(self, true);
        #[cfg(target_arch = "wasm32")]
        {
            self.message = Some(("GridCraft".into(), "The web version is always the latest release.".into()));
        }
    }

    /// Starts the install helper of an update that is ready, so it installs once the process has
    /// exited (a no-op on the web, and when no update is ready). Call it in `on_exit` when the
    /// process ends there, since the app is then never dropped.
    pub fn start_pending_update(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::start_pending_update(self);
    }
}
