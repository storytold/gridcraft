use super::*;

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
    }

    /// Keeps egui's own keyboard zoom and quit shortcuts off and applies GridCraft's zoom factor.
    pub(super) fn sync_input_options(&mut self, ctx: &egui::Context) {
        // Interface size: the preference owns the zoom factor, so egui's own Ctrl/Cmd +/-/0 zoom
        // keys stay off (they would also swallow GridCraft's shortcuts).
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        // egui closes the window on Ctrl+Q (and with Shift or Alt held too) without asking to save
        // (#1443). Excel for Windows has no such key (Ctrl+Q is Quick Analysis). With a native
        // window per workbook egui's Cmd+Q would close only the window it was typed in: the menu
        // bar's Quit item has the key instead (it asks about unsaved workbooks first).
        if (ctx.os() != egui::os::OperatingSystem::Mac || self.native_windows) && ctx.options(|o| !o.quit_shortcuts.is_empty()) {
            ctx.options_mut(|o| o.quit_shortcuts.clear());
        }
        let zoom = self.ui.zoom_factor();
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            ctx.request_repaint();
        }
    }

    /// Look for a newer release once, in the background, as the desktop app starts (a no-op on the
    /// web). A found release opens the update prompt; nothing is shown when GridCraft is current.
    pub fn start_update_check(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::start(self, false);
    }

    /// Help ▸ Check for Updates…: look now, and say so when GridCraft is up to date or the check fails.
    pub fn check_for_updates(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::start(self, true);
        #[cfg(target_arch = "wasm32")]
        {
            self.message = Some(("GridCraft".into(), "The web version is always the latest release.".into()));
        }
    }
}
